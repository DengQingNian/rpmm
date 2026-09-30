//! 使用注入进程后端验证事务和故障处理。
use rpmm::{
    Error, Result,
    config::Unit,
    logging::Logger,
    manager::{Manager, State},
    platform::{Backend, Process},
};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};
static NEXT: AtomicU64 = AtomicU64::new(1);
#[derive(Default)]
struct Fake {
    events: Arc<Mutex<Vec<String>>>,
}
struct Child {
    name: String,
    code: Option<u32>,
    stopped: AtomicBool,
    events: Arc<Mutex<Vec<String>>>,
}
impl Process for Child {
    /// 返回测试 PID。参数：无。返回：固定 PID。
    fn pid(&self) -> u32 {
        42
    }
    /// 返回预设退出状态。参数：无。返回：退出码或 None。
    fn poll(&self) -> Result<Option<u32>> {
        Ok(if self.stopped.load(Ordering::SeqCst) {
            Some(1)
        } else {
            self.code
        })
    }
    /// 记录树终止。参数：无。返回：结果。
    fn terminate(&self) -> Result<()> {
        if !self.stopped.swap(true, Ordering::SeqCst) {
            self.events
                .lock()
                .unwrap()
                .push(format!("kill:{}", self.name));
        }
        Ok(())
    }
}
impl Drop for Child {
    /// 模拟 Job 关闭清理。参数：无。返回：无。
    fn drop(&mut self) {
        let _ = self.terminate();
    }
}
impl Backend for Fake {
    /// 创建预设进程。参数：unit/command 为定义和测试模式，其余为记录上下文。返回：假进程。
    fn spawn(
        &self,
        unit: &Unit,
        command: &[String],
        _: u64,
        _: Option<u32>,
        _: &Logger,
    ) -> Result<Box<dyn Process>> {
        self.events
            .lock()
            .unwrap()
            .push(format!("spawn:{}", unit.name));
        let mode = command.get(1).map(String::as_str).unwrap_or("pending");
        if mode == "fail" {
            return Err(Error::Operation("模拟 spawn 失败".into()));
        }
        if mode == "slow" {
            std::thread::sleep(Duration::from_millis(200));
        }
        Ok(Box::new(Child {
            name: unit.name.clone(),
            code: mode.parse().ok(),
            stopped: AtomicBool::new(false),
            events: self.events.clone(),
        }))
    }
}
/// 创建隔离的调试目录。参数：无。返回：目录。
fn root() -> PathBuf {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("temp/tests")
        .join(format!(
            "{}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
    std::fs::create_dir_all(path.join("units")).unwrap();
    path
}
/// 写入测试配置。参数：root、name、text 为目录、名称及正文。返回：无。
fn write(root: &std::path::Path, name: &str, text: &str) {
    std::fs::write(root.join("units").join(name), text).unwrap();
}
/// 等待状态，超时即失败。参数：manager/name/state 为目标。返回：无。
async fn expect(manager: &Manager, name: &str, state: State) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if manager.status(Some(name)).unwrap()[0].state == state {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

/// 验证进程启动、停止、恢复和 enabled 持久化。参数：无。返回：无。
#[tokio::test]
async fn start_stop_and_enable() {
    let root = root();
    write(
        &root,
        "a.service",
        "[Service]\nExecStart=C:/fake.exe\n[Install]\nWantedBy=multi-user.target\n",
    );
    let fake = Arc::new(Fake::default());
    let manager = Manager::with_backend(&root, fake.clone()).unwrap();
    manager.start(&["a.service".into()]).await.unwrap();
    expect(&manager, "a.service", State::Active).await;
    manager.enable("a.service", true).await.unwrap();
    assert!(manager.status(None).unwrap()[0].enabled);
    manager.stop(&["a.service".into()]).await.unwrap();
    expect(&manager, "a.service", State::Inactive).await;
    let restored = Manager::with_backend(&root, fake).unwrap();
    restored.boot().await.unwrap();
    expect(&restored, "a.service", State::Active).await;
    restored.shutdown().await.unwrap();
    manager.shutdown().await.unwrap();
}
/// 验证依赖顺序和逆序停止。参数：无。返回：无。
#[tokio::test]
async fn dependency_start_and_stop() {
    let root = root();
    write(
        &root,
        "a.service",
        "[Unit]\nRequires=b.service\nAfter=b.service\n[Service]\nExecStart=C:/fake.exe\n",
    );
    write(&root, "b.service", "[Service]\nExecStart=C:/fake.exe\n");
    let fake = Arc::new(Fake::default());
    let manager = Manager::with_backend(&root, fake.clone()).unwrap();
    manager.start(&["a.service".into()]).await.unwrap();
    manager.stop(&["b.service".into()]).await.unwrap();
    assert_eq!(
        *fake.events.lock().unwrap(),
        [
            "spawn:b.service",
            "spawn:a.service",
            "kill:a.service",
            "kill:b.service"
        ]
    );
    manager.shutdown().await.unwrap();
}
/// 验证强弱依赖失败的不同传播。参数：无。返回：无。
#[tokio::test]
async fn strong_and_weak_failure() {
    let root = root();
    write(
        &root,
        "a.service",
        "[Unit]\nRequires=b.service\nAfter=b.service\n[Service]\nExecStart=C:/fake.exe\n",
    );
    write(
        &root,
        "b.service",
        "[Service]\nType=oneshot\nExecStart=C:/fake.exe 1\n",
    );
    write(
        &root,
        "c.service",
        "[Unit]\nWants=b.service\nAfter=b.service\n[Service]\nExecStart=C:/fake.exe\n",
    );
    let fake = Arc::new(Fake::default());
    let manager = Manager::with_backend(&root, fake.clone()).unwrap();
    assert!(manager.start(&["a.service".into()]).await.is_err());
    assert!(
        !fake
            .events
            .lock()
            .unwrap()
            .contains(&"spawn:a.service".into())
    );
    manager.start(&["c.service".into()]).await.unwrap();
    expect(&manager, "c.service", State::Active).await;
    manager.shutdown().await.unwrap();
}
/// 验证无限 oneshot 启动可由停止打断，查询始终可用。参数：无。返回：无。
#[tokio::test]
async fn cancel_oneshot() {
    let root = root();
    write(
        &root,
        "a.service",
        "[Service]\nType=oneshot\nExecStart=C:/fake.exe\n",
    );
    let manager = Manager::with_backend(&root, Arc::new(Fake::default())).unwrap();
    let worker = manager.clone();
    let task = tokio::spawn(async move { worker.start(&["a.service".into()]).await });
    expect(&manager, "a.service", State::Activating).await;
    tokio::time::timeout(Duration::from_secs(3), manager.stop(&["a.service".into()]))
        .await
        .unwrap()
        .unwrap();
    assert!(task.await.unwrap().is_err());
    expect(&manager, "a.service", State::Inactive).await;
    manager.shutdown().await.unwrap();
}
/// 验证启动失败和自动重启最终被限流，reset 后可再启动。参数：无。返回：无。
#[tokio::test]
async fn restart_limit_and_reset() {
    let root = root();
    write(
        &root,
        "a.service",
        "[Unit]\nStartLimitBurst=2\n[Service]\nExecStart=C:/fake.exe fail\nRestart=on-failure\nRestartSec=5ms\n",
    );
    let fake = Arc::new(Fake::default());
    let manager = Manager::with_backend(&root, fake.clone()).unwrap();
    assert!(manager.start(&["a.service".into()]).await.is_err());
    expect(&manager, "a.service", State::Failed).await;
    assert_eq!(manager.status(None).unwrap()[0].substate, "start-limit-hit");
    assert_eq!(
        fake.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.starts_with("spawn"))
            .count(),
        2
    );
    manager.reset_failed("a.service").await.unwrap();
    assert!(manager.start(&["a.service".into()]).await.is_err());
    manager.shutdown().await.unwrap();
}
/// 验证主动停止不会触发 always 重启。参数：无。返回：无。
#[tokio::test]
async fn stop_suppresses_restart() {
    let root = root();
    write(
        &root,
        "a.service",
        "[Service]\nExecStart=C:/fake.exe\nRestart=always\nRestartSec=1ms\n",
    );
    let fake = Arc::new(Fake::default());
    let manager = Manager::with_backend(&root, fake.clone()).unwrap();
    manager.start(&["a.service".into()]).await.unwrap();
    manager.stop(&["a.service".into()]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        fake.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.starts_with("spawn"))
            .count(),
        1
    );
    manager.shutdown().await.unwrap();
}
/// 验证依赖自行退出不传播停止，且 reload 不修改正在运行的快照。参数：无。返回：无。
#[tokio::test]
async fn reload_snapshot_and_natural_exit() {
    let root = root();
    write(
        &root,
        "a.service",
        "[Unit]\nRequires=b.service\nAfter=b.service\n[Service]\nExecStart=C:/fake.exe\n",
    );
    write(
        &root,
        "b.service",
        "[Service]\nType=oneshot\nExecStart=C:/fake.exe 0\n",
    );
    let manager = Manager::with_backend(&root, Arc::new(Fake::default())).unwrap();
    manager.start(&["a.service".into()]).await.unwrap();
    expect(&manager, "b.service", State::Inactive).await;
    expect(&manager, "a.service", State::Active).await;
    write(&root, "a.service", "[Service]\nExecStart=C:/new.exe\n");
    assert_eq!(manager.reload().await.unwrap(), 2);
    assert_eq!(
        manager.status(Some("a.service")).unwrap()[0].config_version,
        1
    );
    manager.stop(&["b.service".into()]).await.unwrap();
    expect(&manager, "a.service", State::Inactive).await;
    manager.shutdown().await.unwrap();
}
/// 验证错误 reload 保留原配置和运行定义。参数：无。返回：无。
#[tokio::test]
async fn reload_errors_preserve_state() {
    let root = root();
    write(&root, "a.service", "[Service]\nExecStart=C:/fake.exe\n");
    let manager = Manager::with_backend(&root, Arc::new(Fake::default())).unwrap();
    manager.start(&["a.service".into()]).await.unwrap();
    std::fs::remove_file(root.join("units/a.service")).unwrap();
    assert!(manager.reload().await.is_err());
    assert_eq!(manager.status(None).unwrap().len(), 1);
    write(&root, "a.service", "[Service]\nUser=x\n");
    assert!(manager.reload().await.is_err());
    expect(&manager, "a.service", State::Active).await;
    manager.shutdown().await.unwrap();
}
/// 验证 oneshot 的顺序命令、退出保留及启动超时。参数：无。返回：无。
#[tokio::test]
async fn oneshot_remain_and_timeout() {
    let root = root();
    write(
        &root,
        "a.service",
        "[Service]\nType=oneshot\nExecStart=C:/fake.exe 0\nExecStart=C:/fake.exe 0\nRemainAfterExit=yes\n",
    );
    write(
        &root,
        "b.service",
        "[Service]\nType=oneshot\nExecStart=C:/fake.exe\nTimeoutStartSec=20ms\n",
    );
    let fake = Arc::new(Fake::default());
    let manager = Manager::with_backend(&root, fake.clone()).unwrap();
    manager.start(&["a.service".into()]).await.unwrap();
    expect(&manager, "a.service", State::Active).await;
    assert_eq!(
        fake.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| *e == "spawn:a.service")
            .count(),
        2
    );
    assert!(manager.start(&["b.service".into()]).await.is_err());
    expect(&manager, "b.service", State::Failed).await;
    manager.shutdown().await.unwrap();
}
/// 验证 drop-in 排序和 oneshot 默认超时。参数：无。返回：无。
#[test]
fn loading_drop_ins() {
    let root = root();
    write(
        &root,
        "a.service",
        "[Service]\nType=oneshot\nExecStart=C:/fake.exe 0\n",
    );
    let drop = root.join("units/a.service.d");
    std::fs::create_dir_all(&drop).unwrap();
    std::fs::write(drop.join("20.conf"), "[Service]\nEnvironment=VALUE=last\n").unwrap();
    std::fs::write(drop.join("10.conf"), "[Service]\nEnvironment=VALUE=first\n").unwrap();
    let units = rpmm::config::load(&root.join("units")).unwrap();
    assert_eq!(units["a.service"].environment["VALUE"], "last");
    assert_eq!(units["a.service"].timeout_start, None);
}
/// 验证日志轮转及跨块 Unicode。参数：无。返回：无。
#[test]
fn log_rotation_and_encoding() {
    let root = root();
    let logger = Logger::with_limits(&root.join("logs"), 512, 2).unwrap();
    for index in 0..20 {
        logger
            .write("a.service", 1, "stdout", &format!("行{index}"))
            .unwrap();
    }
    let records = logger.tail("a.service", Some("stdout"), 3).unwrap();
    assert_eq!(records.len(), 3);
    assert_eq!(records.last().unwrap().text, "行19");
    assert!(root.join("logs/a.service.jsonl.2").exists());
    assert!(!root.join("logs/a.service.jsonl.3").exists());
}

/// 验证重启不意外拉起从未运行的反向依赖者。参数：无。返回：无。
#[tokio::test]
async fn restart_only_live_dependents() {
    let root = root();
    write(
        &root,
        "a.service",
        "[Unit]\nRequires=b.service\nAfter=b.service\n[Service]\nExecStart=C:/fake.exe\n",
    );
    write(&root, "b.service", "[Service]\nExecStart=C:/fake.exe\n");
    let fake = Arc::new(Fake::default());
    let manager = Manager::with_backend(&root, fake.clone()).unwrap();
    manager.start(&["b.service".into()]).await.unwrap();
    manager.restart(&["b.service".into()]).await.unwrap();
    assert!(
        !fake
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e == "spawn:a.service")
    );
    expect(&manager, "a.service", State::Inactive).await;
    manager.shutdown().await.unwrap();
}
/// 验证停止命令失败仍清理主进程，并向调用方报告。参数：无。返回：无。
#[tokio::test]
async fn stop_failure_is_reported() {
    let root = root();
    write(
        &root,
        "a.service",
        "[Service]\nExecStart=C:/fake.exe\nExecStop=C:/fake.exe 1\n",
    );
    let fake = Arc::new(Fake::default());
    let manager = Manager::with_backend(&root, fake.clone()).unwrap();
    manager.start(&["a.service".into()]).await.unwrap();
    assert!(manager.stop(&["a.service".into()]).await.is_err());
    assert_eq!(manager.status(None).unwrap()[0].substate, "stop-failed");
    assert!(
        fake.events
            .lock()
            .unwrap()
            .iter()
            .any(|e| e == "kill:a.service")
    );
    manager.shutdown().await.unwrap();
}
/// 验证超时后旧 spawn 结果不能修改新实例。参数：无。返回：无。
#[tokio::test(flavor = "multi_thread")]
async fn late_spawn_does_not_corrupt_new_instance() {
    let root = root();
    write(
        &root,
        "a.service",
        "[Service]\nExecStart=C:/fake.exe slow\nTimeoutStartSec=20ms\n",
    );
    let manager = Manager::with_backend(&root, Arc::new(Fake::default())).unwrap();
    assert!(manager.start(&["a.service".into()]).await.is_err());
    expect(&manager, "a.service", State::Failed).await;
    write(&root, "a.service", "[Service]\nExecStart=C:/fake.exe\n");
    manager.reload().await.unwrap();
    manager.start(&["a.service".into()]).await.unwrap();
    let instance = manager.status(None).unwrap()[0].instance;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let status = manager.status(None).unwrap().remove(0);
    assert_eq!(status.state, State::Active);
    assert_eq!(status.instance, instance);
    assert_eq!(status.config_version, 2);
    manager.shutdown().await.unwrap();
}
/// 验证多字节字符跨块时完整保留，非法字节只产生局部替代。参数：无。返回：无。
#[test]
fn streamed_utf8_survives_invalid_prefix() {
    struct Chunks(Vec<Vec<u8>>);
    impl std::io::Read for Chunks {
        /// 返回预设分块。参数：buffer 为目标缓冲。返回：本次字节数。
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            let bytes = self.0.remove(0);
            buffer[..bytes.len()].copy_from_slice(&bytes);
            Ok(bytes.len())
        }
    }
    let root = root();
    let logger = Logger::new(&root.join("logs")).unwrap();
    logger.drain(
        Chunks(vec![vec![0xff, 0xe4], vec![0xb8], vec![0xad]]),
        "a.service",
        1,
        "stdout",
    );
    let text = logger
        .tail("a.service", Some("stdout"), 10)
        .unwrap()
        .iter()
        .map(|r| r.text.as_str())
        .collect::<String>();
    assert_eq!(text, "�中");
}
