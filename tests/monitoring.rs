//! 健康探测协议、超时、配置覆盖与 Windows 资源采集回归。
use rpmm::{
    config::{self, Unit},
    health::{HealthConfig, probe},
    metrics::{Collector, parse_connections},
};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

/// 构建不跟随跳转的探测客户端。参数：无。返回：HTTP 客户端。
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .unwrap()
}

/// 创建单次 HTTP 服务。参数：status 为状态码，delay 为响应延迟。返回：URL 与后台任务。
async fn http_server(status: u16, delay: Duration) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/health", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0u8; 4096];
        let _ = stream.read(&mut buffer).await;
        tokio::time::sleep(delay).await;
        let response = format!(
            "HTTP/1.1 {status} test\r\nContent-Length: 0\r\nLocation: /other\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(response.as_bytes()).await;
    });
    (url, task)
}

/// 验证 HTTP 只有直接 200 成功，跳转及其他成功码均失败。参数：无。返回：无。
#[tokio::test]
async fn http_requires_exact_200() {
    for status in [200, 204, 301, 500] {
        let (url, task) = http_server(status, Duration::ZERO).await;
        let config = HealthConfig {
            kind: "http".into(),
            url,
            ..Default::default()
        };
        let record = probe(&config, 7, &client()).await;
        assert_eq!(record.healthy, status == 200);
        assert_eq!(record.instance, 7);
        assert!(record.detail.contains(&status.to_string()));
        task.await.unwrap();
    }
}

/// 验证 HTTP 整体请求受超时限制。参数：无。返回：无。
#[tokio::test]
async fn http_timeout_is_bounded() {
    let (url, task) = http_server(200, Duration::from_secs(2)).await;
    let config = HealthConfig {
        kind: "http".into(),
        url,
        timeout: Duration::from_millis(50),
        ..Default::default()
    };
    let record = probe(&config, 1, &client()).await;
    assert!(!record.healthy);
    assert!(record.detail.contains("超时"));
    assert!(record.latency_ms < 1000);
    task.abort();
}

/// 验证 TCP 端口可连通时健康，关闭监听后失败。参数：无。返回：无。
#[tokio::test]
async fn tcp_checks_local_port() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = HealthConfig {
        kind: "tcp".into(),
        port: listener.local_addr().unwrap().port(),
        ..Default::default()
    };
    assert!(probe(&config, 1, &client()).await.healthy);
    drop(listener);
    assert!(!probe(&config, 1, &client()).await.healthy);
}

/// 验证尾读跨块 UTF-8、来源筛选、残缺尾行及数量边界。参数：无。返回：无。
#[test]
fn log_tail_handles_chunk_boundaries() {
    let directory = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("temp/monitoring-tests")
        .join(format!(
            "tail-{}-{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap()
        ));
    let logger = rpmm::logging::Logger::new(&directory).unwrap();
    for index in 0..600 {
        logger
            .write(
                "app.service",
                1,
                if index % 2 == 0 { "stdout" } else { "stderr" },
                &format!("{index} {}", "中文".repeat(50)),
            )
            .unwrap();
    }
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(directory.join("app.service.jsonl"))
        .unwrap()
        .write_all(b"{\"incomplete\":")
        .unwrap();
    let records = logger.tail("app.service", Some("stdout"), 200).unwrap();
    assert_eq!(records.len(), 200);
    assert!(records[0].text.starts_with("200 "));
    assert!(records[199].text.starts_with("598 "));
    assert_eq!(logger.tail("app.service", None, 10000).unwrap().len(), 600);
    assert!(logger.tail("app.service", None, 0).unwrap().is_empty());
}

/// 验证健康配置默认值、覆盖重置和目标校验。参数：无。返回：无。
#[test]
fn health_configuration_validation() {
    let mut unit = Unit::new("app.service");
    config::merge(
        &mut unit,
        "app.service",
        "[Service]\nExecStart=C:/app.exe\nHealthType=tcp\nHealthPort=8080\n",
    )
    .unwrap();
    config::validate(&unit).unwrap();
    assert_eq!(unit.health.timeout, Duration::from_secs(1));
    config::merge(&mut unit, "20-health.conf", "[Service]\nHealthType=http\nHealthUrl=https://localhost/health\nHealthTimeoutSec=500ms\nHealthIntervalSec=2s\n").unwrap();
    config::validate(&unit).unwrap();
    assert_eq!(unit.health.timeout, Duration::from_millis(500));
    for text in [
        "HealthUrl=ftp://localhost/health",
        "HealthTimeoutSec=0s",
        "HealthTimeoutSec=61s",
        "HealthIntervalSec=0s",
        "HealthType=icmp",
    ] {
        let mut invalid = unit.clone();
        config::merge(
            &mut invalid,
            "invalid.conf",
            &format!("[Service]\n{text}\n"),
        )
        .unwrap();
        assert!(config::validate(&invalid).is_err(), "{text}");
    }
    config::merge(
        &mut unit,
        "30-reset.conf",
        "[Service]\nHealthTimeoutSec=\nHealthType=none\n",
    )
    .unwrap();
    config::validate(&unit).unwrap();
    assert_eq!(unit.health.timeout, Duration::from_secs(1));
}

/// 验证 IPv4、IPv6、UDP、PID 筛选及无效表行。参数：无。返回：无。
#[test]
fn network_table_filters_pid() {
    let text = "协议 本地地址 外部地址 状态 PID\n TCP 127.0.0.1:80 127.0.0.1:5 ESTABLISHED 42\n TCP [::]:8080 [::]:0 LISTENING 42\n UDP 0.0.0.0:53 *:* 42\n TCP 0.0.0.0:80 0.0.0.0:0 LISTENING 43\n malformed 42\n";
    let connections = parse_connections(text, 42);
    assert_eq!(connections.len(), 3);
    assert_eq!(connections[1].local, "[::]:8080");
    assert_eq!(connections[2].protocol, "UDP");
}

/// 验证 Windows 原生采集包含当前进程的真实监听端口。参数：无。返回：无。
#[cfg(windows)]
#[tokio::test]
async fn native_connections_include_listener() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let connections =
        tokio::task::spawn_blocking(|| rpmm::metrics::connections(std::process::id()))
            .await
            .unwrap()
            .unwrap();
    assert!(
        connections
            .iter()
            .any(|connection| connection.protocol == "TCP" && connection.local == address)
    );
}

/// 验证真实本机采样与首次速率空值，防止累计 I/O 被当成瞬时速率。参数：无。返回：无。
#[test]
fn real_metrics_use_two_samples() {
    let mut collector = Collector::default();
    let (host, process) = collector.sample(Some(std::process::id()));
    assert!(host.memory_total > 0);
    assert!(host.cpu.is_none());
    let process = process.unwrap();
    assert!(process.memory > 0);
    assert!(process.start_time > 0);
    assert!(process.read_per_sec.is_none());
    std::thread::sleep(Duration::from_millis(250));
    let (host, process) = collector.sample(Some(std::process::id()));
    assert!(host.cpu.unwrap().is_finite());
    assert!(process.unwrap().read_per_sec.unwrap().is_finite());
    collector.sample(None);
    let (_, process) = collector.sample(Some(std::process::id()));
    assert!(process.unwrap().cpu.is_none());
}
