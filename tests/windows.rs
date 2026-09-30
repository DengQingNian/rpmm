//! 真实 Windows Job、输出、参数及命名管道集成测试。
#![cfg(all(windows, feature = "test-fixtures"))]
use rpmm::{
    config::Unit,
    logging::Logger,
    manager::Manager,
    platform::{Backend, Native, Process},
};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::{
    io::BufReader,
    net::windows::named_pipe::ClientOptions,
    sync::{oneshot, watch},
};
use windows::Win32::{Foundation::*, System::Threading::*};

/// 创建独立工作目录。参数：label 为测试名称。返回：规范化目录。
fn root(label: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("temp/windows")
        .join(format!(
            "{label}-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
    std::fs::create_dir_all(root.join("units")).unwrap();
    std::fs::canonicalize(root).unwrap()
}
/// 获取辅助程序。参数：无。返回：绝对路径。
fn fixture() -> String {
    env!("CARGO_BIN_EXE_rpmm-fixture").to_string()
}
/// 构造真实进程配置。参数：mode 为模式。返回：配置。
fn unit(mode: &str) -> Unit {
    let mut unit = Unit::new("app.service");
    unit.exec_start = vec![vec![fixture(), mode.into()]];
    unit
}
/// 轮询真实进程退出。参数：child 为句柄。返回：退出码。
fn wait(child: &dyn Process) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(code) = child.poll().unwrap() {
            return code;
        }
        assert!(Instant::now() < deadline, "进程超时");
        std::thread::sleep(Duration::from_millis(10));
    }
}
/// 检测进程是否仍在运行。参数：pid 为编号。返回：是否运行。
fn alive(pid: u32) -> bool {
    unsafe {
        match OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            Ok(handle) => {
                let running = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
                let _ = CloseHandle(handle);
                running
            }
            Err(_) => false,
        }
    }
}

/// 验证标准 argv 与环境、目录传递。参数：无。返回：无。
#[test]
fn argv_roundtrip() {
    let root = root("argv");
    let logger = Logger::new(&root.join("logs")).unwrap();
    let mut unit = unit("args");
    let expected = [
        "",
        "a b",
        "中文",
        "quote\"inside",
        "C:\\trailing\\",
        "%PATH%",
        "$literal",
    ];
    unit.exec_start[0].extend(expected.iter().map(|s| s.replace('$', "$$")));
    let child = Native
        .spawn(&unit, &unit.exec_start[0], 1, None, &logger)
        .unwrap();
    assert_eq!(wait(child.as_ref()), 0);
    drop(child);
    let deadline = Instant::now() + Duration::from_secs(5);
    let text = loop {
        let text = logger
            .tail("app.service", Some("stdout"), 100)
            .unwrap()
            .iter()
            .map(|r| r.text.as_str())
            .collect::<String>();
        if text.ends_with('\n') {
            break text;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    let values: Vec<String> = serde_json::from_str(&text).unwrap();
    assert_eq!(values, expected);
}

/// 验证 UTF-16 环境块和指定工作目录传入实际进程。参数：无。返回：无。
#[test]
fn environment_and_working_directory() {
    let root = root("environment");
    let logger = Logger::new(&root.join("logs")).unwrap();
    let mut unit = unit("env");
    unit.environment
        .insert("RPMM_TEST_ENV".into(), "中文=value".into());
    unit.working_directory = Some(root.to_string_lossy().into_owned());
    let child = Native
        .spawn(&unit, &unit.exec_start[0], 1, None, &logger)
        .unwrap();
    assert_eq!(wait(child.as_ref()), 0);
    drop(child);
    let text = logger
        .tail("app.service", Some("stdout"), 100)
        .unwrap()
        .iter()
        .map(|r| r.text.as_str())
        .collect::<String>();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["value"], "中文=value");
    assert_eq!(
        std::fs::canonicalize(value["directory"].as_str().unwrap()).unwrap(),
        root
    );
}
/// 验证双流输出超过管道容量时不会死锁。参数：无。返回：无。
#[test]
fn concurrent_large_output() {
    let root = root("flood");
    let logger = Logger::new(&root.join("logs")).unwrap();
    let unit = unit("flood");
    let child = Native
        .spawn(&unit, &unit.exec_start[0], 1, None, &logger)
        .unwrap();
    assert_eq!(wait(child.as_ref()), 0);
    drop(child);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let stdout: usize = logger
            .tail("app.service", Some("stdout"), 10000)
            .unwrap()
            .iter()
            .map(|r| r.text.len())
            .sum();
        let stderr: usize = logger
            .tail("app.service", Some("stderr"), 10000)
            .unwrap()
            .iter()
            .map(|r| r.text.len())
            .sum();
        if stdout == 512 * 8192 && stderr == 512 * 8192 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "日志未完整排空 stdout={stdout} stderr={stderr}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}
/// 验证关闭 Job 自动清理子孙进程。参数：无。返回：无。
#[test]
fn job_close_cleans_tree() {
    let root = root("tree");
    let logger = Logger::new(&root.join("logs")).unwrap();
    let mut unit = unit("tree");
    let child_pid_file = root.join("child.pid");
    unit.exec_start[0].push(child_pid_file.to_string_lossy().into_owned());
    let child = Native
        .spawn(&unit, &unit.exec_start[0], 1, None, &logger)
        .unwrap();
    let parent_pid = child.pid();
    let deadline = Instant::now() + Duration::from_secs(5);
    let descendant = loop {
        if let Ok(text) = std::fs::read_to_string(&child_pid_file) {
            break text.parse::<u32>().unwrap();
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(alive(parent_pid));
    assert!(alive(descendant));
    drop(child);
    while alive(parent_pid) || alive(descendant) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
}
/// 验证 spawn 失败不会留下运行中的实例。参数：无。返回：无。
#[test]
fn missing_executable_fails() {
    let root = root("missing");
    let logger = Logger::new(&root.join("logs")).unwrap();
    let mut unit = unit("sleep");
    unit.exec_start[0][0] = root.join("missing.exe").to_string_lossy().into_owned();
    assert!(
        Native
            .spawn(&unit, &unit.exec_start[0], 1, None, &logger)
            .is_err()
    );
}
/// 向监听器发单个请求。参数：root/action 为目录与动作。返回：响应。
async fn request(root: &Path, action: rpmm::ipc::Action) -> rpmm::ipc::Response {
    let pipe = ClientOptions::new()
        .open(rpmm::ipc::pipe_name(root))
        .unwrap();
    let mut reader = BufReader::new(pipe);
    rpmm::ipc::write_frame(reader.get_mut(), &rpmm::ipc::Request { version: 1, action })
        .await
        .unwrap();
    rpmm::ipc::read_frame(&mut reader).await.unwrap()
}
/// 验证真实 IPC 控制闭环、版本诊断和单实例保护。参数：无。返回：无。
#[tokio::test(flavor = "multi_thread")]
async fn ipc_control_and_exclusive_root() {
    let root = root("ipc");
    std::fs::write(
        root.join("units/app.service"),
        format!(
            "[Service]\nExecStart=\"{}\" sleep\n[Install]\nWantedBy=multi-user.target\n",
            fixture().replace('\\', "/")
        ),
    )
    .unwrap();
    let manager = Manager::new(&root).unwrap();
    let (stop_tx, stop_rx) = watch::channel(false);
    let (ready_tx, ready_rx) = oneshot::channel();
    let server = tokio::spawn(rpmm::ipc::serve(manager.clone(), stop_rx, ready_tx));
    ready_rx.await.unwrap().unwrap();
    assert!(rpmm::scm::security::pipe(&rpmm::ipc::pipe_name(&root), true).is_err());
    assert!(
        request(
            &root,
            rpmm::ipc::Action::Start {
                unit: "app.service".into()
            }
        )
        .await
        .ok
    );
    let status = request(
        &root,
        rpmm::ipc::Action::Status {
            unit: Some("app.service".into()),
        },
    )
    .await;
    assert!(status.ok);
    assert_eq!(status.data[0]["state"], "active");
    assert!(
        request(
            &root,
            rpmm::ipc::Action::Enable {
                unit: "app.service".into()
            }
        )
        .await
        .ok
    );
    assert!(
        request(
            &root,
            rpmm::ipc::Action::Stop {
                unit: "app.service".into()
            }
        )
        .await
        .ok
    );
    assert!(request(&root, rpmm::ipc::Action::DaemonReload).await.ok);
    let pipe = ClientOptions::new()
        .open(rpmm::ipc::pipe_name(&root))
        .unwrap();
    let mut reader = BufReader::new(pipe);
    rpmm::ipc::write_frame(
        reader.get_mut(),
        &rpmm::ipc::Request {
            version: 999,
            action: rpmm::ipc::Action::List,
        },
    )
    .await
    .unwrap();
    let response: rpmm::ipc::Response = rpmm::ipc::read_frame(&mut reader).await.unwrap();
    assert_eq!(response.code, "unsupported-version");
    let _ = stop_tx.send(true);
    server.await.unwrap().unwrap();
    assert_eq!(
        manager.status(None).unwrap()[0].state,
        rpmm::manager::State::Inactive
    );
}
/// 验证管道 DACL 无 Everyone/匿名授权且禁止远程客户端。参数：无。返回：无。
#[tokio::test]
async fn pipe_acl_is_explicit() {
    use std::os::windows::io::AsRawHandle;
    use windows::{
        Win32::Security::{Authorization::*, *},
        core::PWSTR,
    };
    let root = root("acl");
    let pipe = rpmm::scm::security::pipe(&rpmm::ipc::pipe_name(&root), true).unwrap();
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        GetSecurityInfo(
            HANDLE(pipe.as_raw_handle()),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            None,
            None,
            None,
            None,
            Some(&mut descriptor),
        )
        .ok()
        .unwrap();
        let mut text = PWSTR::null();
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut text,
            None,
        )
        .unwrap();
        let sddl = text.to_string().unwrap();
        let _ = LocalFree(Some(HLOCAL(text.0.cast())));
        let _ = LocalFree(Some(HLOCAL(descriptor.0)));
        assert!(sddl.contains(";;;SY"));
        assert!(sddl.contains(";;;BA"));
        assert!(!sddl.contains(";;;WD"));
        assert!(!sddl.contains(";;;AN"));
        assert!(sddl.contains(&rpmm::scm::security::current_sid().unwrap()));
    }
}
/// 验证宿主被杀后 Job 句柄关闭，从而清理整个托管进程树。参数：无。返回：无。
#[tokio::test(flavor = "multi_thread")]
async fn manager_crash_cleans_children() {
    let root = root("crash");
    let pid_file = root.join("grandchild.pid");
    std::fs::write(
        root.join("units/app.service"),
        format!(
            "[Service]\nExecStart=\"{}\" tree \"{}\"\n",
            fixture().replace('\\', "/"),
            pid_file.to_string_lossy().replace('\\', "/")
        ),
    )
    .unwrap();
    use std::os::windows::process::CommandExt;
    let mut host = std::process::Command::new(env!("CARGO_BIN_EXE_rpmm"))
        .creation_flags(CREATE_NO_WINDOW.0)
        .arg("--root")
        .arg(&root)
        .args(["manager", "run"])
        .stdout(std::fs::File::create(root.join("host.out")).unwrap())
        .stderr(std::fs::File::create(root.join("host.err")).unwrap())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if ClientOptions::new()
            .open(rpmm::ipc::pipe_name(&root))
            .is_ok()
        {
            break;
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        request(
            &root,
            rpmm::ipc::Action::Start {
                unit: "app.service".into()
            }
        )
        .await
        .ok
    );
    let status = request(
        &root,
        rpmm::ipc::Action::Status {
            unit: Some("app.service".into()),
        },
    )
    .await;
    let parent = status.data[0]["pid"].as_u64().unwrap() as u32;
    let descendant = loop {
        if let Ok(text) = std::fs::read_to_string(&pid_file) {
            break text.parse::<u32>().unwrap();
        }
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    };
    host.kill().unwrap();
    host.wait().unwrap();
    while alive(parent) || alive(descendant) {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}
