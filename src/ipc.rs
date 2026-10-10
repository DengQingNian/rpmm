//! 有版本的本机命名管道协议；每个连接一个请求。
use crate::{Error, Result, logging::Record, manager::Manager};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{path::Path, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::{oneshot, watch},
};

pub const VERSION: u32 = 1;
pub const MAX_FRAME: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Action {
    List,
    Status {
        unit: Option<String>,
    },
    Start {
        unit: String,
    },
    Stop {
        unit: String,
    },
    Restart {
        unit: String,
    },
    Enable {
        unit: String,
    },
    Disable {
        unit: String,
    },
    DaemonReload,
    Shutdown,
    ResetFailed {
        unit: String,
    },
    Delete {
        unit: String,
        #[serde(default)]
        stop: bool,
    },
    Logs {
        unit: String,
        source: Option<String>,
        lines: usize,
        follow: bool,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub version: u32,
    #[serde(flatten)]
    pub action: Action,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub version: u32,
    pub ok: bool,
    pub code: String,
    pub data: Value,
}
impl Response {
    /// 创建成功响应。参数：data 为可序列化数据。返回：响应。
    pub fn success(data: impl Serialize) -> Result<Self> {
        Ok(Self {
            version: VERSION,
            ok: true,
            code: "ok".into(),
            data: serde_json::to_value(data)?,
        })
    }
    /// 创建失败响应。参数：error 为错误。返回：带稳定错误码的响应。
    pub fn failure(error: &Error) -> Self {
        Self {
            version: VERSION,
            ok: false,
            code: match error {
                Error::Config(_) => "invalid-config",
                Error::Operation(_) => "operation-failed",
                Error::Io(_) => "io-error",
                Error::Json(_) => "invalid-protocol",
            }
            .into(),
            data: Value::String(error.to_string()),
        }
    }
}

/// 对规范数据路径生成稳定管道名。参数：root 为已规范化目录。返回：管道路径。
pub fn pipe_name(root: &Path) -> String {
    let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.into());
    let root = &canonical;
    let normalized = root.to_string_lossy().replace('/', "\\").to_lowercase();
    let hash = Sha256::digest(normalized.as_bytes());
    format!("\\\\.\\pipe\\rpmm-{:x}", hash)
}

/// 读取一条有界 JSON 帧。参数：reader 为缓冲流。返回：解码对象。
pub async fn read_frame<T: serde::de::DeserializeOwned>(
    reader: &mut (impl tokio::io::AsyncBufRead + Unpin),
) -> Result<T> {
    let mut data = vec![];
    reader
        .take(MAX_FRAME + 1)
        .read_until(b'\n', &mut data)
        .await?;
    if data.len() as u64 > MAX_FRAME || !data.ends_with(b"\n") {
        return Err(Error::Operation("帧超限或连接在完整帧之前关闭".into()));
    }
    Ok(serde_json::from_slice(&data)?)
}
/// 发送 JSON 帧。参数：writer 为流，value 为数据。返回：结果。
pub async fn write_frame(
    writer: &mut (impl tokio::io::AsyncWrite + Unpin),
    value: &impl Serialize,
) -> Result<()> {
    let mut data = serde_json::to_vec(value)?;
    data.push(b'\n');
    if data.len() as u64 > MAX_FRAME {
        return Err(Error::Operation("响应帧超过 1 MiB".into()));
    }
    writer.write_all(&data).await?;
    writer.flush().await?;
    Ok(())
}

/// 执行非日志请求。参数：manager/action 为管理器和动作。返回：响应数据。
async fn execute(manager: &Arc<Manager>, action: Action) -> Result<Value> {
    match action {
        Action::List | Action::Status { unit: None } => {
            Ok(serde_json::to_value(manager.status(None)?)?)
        }
        Action::Status { unit: Some(unit) } => {
            Ok(serde_json::to_value(manager.status(Some(&unit))?)?)
        }
        Action::Start { unit } => {
            manager.start(&[unit]).await?;
            Ok(Value::Null)
        }
        Action::Stop { unit } => {
            manager.stop(&[unit]).await?;
            Ok(Value::Null)
        }
        Action::Restart { unit } => {
            manager.restart(&[unit]).await?;
            Ok(Value::Null)
        }
        Action::Enable { unit } => {
            manager.enable(&unit, true).await?;
            Ok(Value::Null)
        }
        Action::Disable { unit } => {
            manager.enable(&unit, false).await?;
            Ok(Value::Null)
        }
        Action::DaemonReload => Ok(serde_json::to_value(manager.reload().await?)?),
        Action::ResetFailed { unit } => {
            manager.reset_failed(&unit).await?;
            Ok(Value::Null)
        }
        Action::Delete { unit, stop } => {
            manager.delete_service(&unit, stop, None).await?;
            Ok(Value::Null)
        }
        Action::Logs { .. } | Action::Shutdown => {
            Err(Error::Operation("此请求应由连接处理器协调".into()))
        }
    }
}

/// 处理连接。参数：stream、manager、stop 为管道、管理器和关闭通知。返回：结果。
async fn connection<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    stream: S,
    manager: Arc<Manager>,
    mut stop: watch::Receiver<bool>,
    shutdown: watch::Sender<bool>,
) -> Result<()> {
    let mut stream = BufReader::new(stream);
    let request: Request =
        match tokio::time::timeout(std::time::Duration::from_secs(30), read_frame(&mut stream))
            .await
        {
            Ok(Ok(request)) => request,
            result => {
                let error = Error::Operation(format!("无效请求：{result:?}"));
                write_frame(stream.get_mut(), &Response::failure(&error)).await?;
                return Ok(());
            }
        };
    if request.version != VERSION {
        return write_frame(
            stream.get_mut(),
            &Response {
                version: VERSION,
                ok: false,
                code: "unsupported-version".into(),
                data: Value::String("协议版本不兼容".into()),
            },
        )
        .await;
    }
    if matches!(request.action, Action::Shutdown) {
        // 先确认请求已受理，再通知监听器关闭；监听器负责等待完整进程清理。
        write_frame(stream.get_mut(), &Response::success(Value::Null)?).await?;
        let _ = shutdown.send(true);
        return Ok(());
    }
    if let Action::Logs {
        unit,
        source,
        lines,
        follow,
    } = request.action
    {
        let snapshot = (|| {
            crate::config::validate_name(&unit)?;
            manager.status(Some(&unit))?;
            if source
                .as_deref()
                .is_some_and(|s| !matches!(s, "stdout" | "stderr" | "manager" | "health"))
            {
                return Err(Error::Operation(
                    "日志来源只能是 stdout/stderr/manager/health".into(),
                ));
            }
            manager
                .logger
                .tail_and_subscribe(&unit, source.as_deref(), lines)
        })();
        let (records, mut subscription) = match snapshot {
            Ok(snapshot) => snapshot,
            Err(error) => return write_frame(stream.get_mut(), &Response::failure(&error)).await,
        };
        for record in records {
            write_frame(stream.get_mut(), &Response::success(record)?).await?;
        }
        if !follow {
            write_frame(stream.get_mut(), &Response::success(Value::Null)?).await?;
            return Ok(());
        }
        loop {
            tokio::select! {
                result = subscription.recv() => match result {
                    Ok(record) if record.unit == unit && source.as_deref().is_none_or(|s| s == record.source) => write_frame(stream.get_mut(), &Response::success(record)?).await?,
                    Ok(_) => (),
                    Err(e) => { write_frame(stream.get_mut(), &Response { version: VERSION, ok: false, code: "log-stream-lagged".into(), data: Value::String(e.to_string()) }).await?; break; }
                },
                _ = stop.changed() => break,
                result = stream.fill_buf() => { if result?.is_empty() { break; } return Err(Error::Operation("日志连接不允许追加请求".into())); }
            }
        }
        Ok(())
    } else {
        let response = match execute(&manager, request.action).await {
            Ok(data) => Response::success(data)?,
            Err(e) => Response::failure(&e),
        };
        write_frame(stream.get_mut(), &response).await
    }
}

#[cfg(windows)]
/// 管道监听与管理器启动。参数：manager、stop、ready 为管理器、关闭和就绪通知。返回：关闭结果。
pub async fn serve(
    manager: Arc<Manager>,
    mut stop: watch::Receiver<bool>,
    ready: oneshot::Sender<Result<()>>,
) -> Result<()> {
    let name = pipe_name(&manager.root);
    let mut server = match crate::security::pipe(&name, true) {
        Ok(server) => server,
        Err(e) => {
            let _ = ready.send(Err(Error::Operation(e.to_string())));
            return Err(e);
        }
    };
    let _ = ready.send(Ok(()));
    let boot = manager.clone();
    tokio::spawn(async move {
        if let Err(e) = boot.boot().await {
            let _ = boot
                .logger
                .write("manager", 0, "manager", &format!("开机激活失败：{e}"));
        }
    });
    let mut connections = tokio::task::JoinSet::new();
    let (shutdown, mut requested_stop) = watch::channel(false);
    let accept_result = loop {
        if *stop.borrow() || *requested_stop.borrow() {
            break Ok(());
        }
        tokio::select! {
            result = server.connect() => {
                if let Err(e) = result { break Err(Error::Io(e)); }
                let next = match crate::security::pipe(&name, false) { Ok(pipe) => pipe, Err(e) => break Err(e) };
                let connected = std::mem::replace(&mut server, next);
                let manager = manager.clone(); let stop = stop.clone();
                let shutdown = shutdown.clone();
                // 活跃连接数量有上限，避免授权客户端无限消耗内存。
                if connections.len() >= 64 { drop(connected); continue; }
                connections.spawn(async move { if let Err(e) = connection(connected, manager.clone(), stop, shutdown).await { let _ = manager.logger.write("manager", 0, "manager", &format!("IPC 连接结束：{e}")); } });
            }
            _ = stop.changed() => break Ok(()),
            _ = requested_stop.changed() => break Ok(()),
            _ = connections.join_next(), if !connections.is_empty() => (),
        }
    };
    // 监听句柄保留到进程关闭完成，防止另一个管理器提前占用同一 root。
    manager.shutdown().await?;
    connections.abort_all();
    drop(server);
    accept_result
}

#[cfg(windows)]
/// 向运行中的管理器发送动作并输出结果。参数：root/action/json 为目录、动作和显示模式。返回：结果。
pub async fn client(root: &Path, action: Action, json: bool) -> Result<()> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let name = pipe_name(root);
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let pipe = loop {
        match ClientOptions::new().open(&name) {
            Ok(pipe) => break pipe,
            Err(e) if e.raw_os_error() == Some(231) && tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await
            }
            Err(e) => return Err(e.into()),
        }
    };
    let log_mode = matches!(action, Action::Logs { .. });
    let mut stream = BufReader::new(pipe);
    write_frame(
        stream.get_mut(),
        &Request {
            version: VERSION,
            action,
        },
    )
    .await?;
    loop {
        let response: Response = read_frame(&mut stream).await?;
        if response.version != VERSION {
            return Err(Error::Operation("响应协议版本不兼容".into()));
        }
        if !response.ok {
            return Err(Error::Operation(format!(
                "{}: {}",
                response.code, response.data
            )));
        }
        if log_mode {
            if response.data.is_null() {
                break;
            }
            if json {
                println!("{}", serde_json::to_string(&response.data)?);
            } else {
                let record: Record = serde_json::from_value(response.data)?;
                print!("{} [{}] {}", record.time, record.source, record.text);
                if !record.text.ends_with('\n') {
                    println!();
                }
            }
        } else {
            if !response.data.is_null() {
                if json {
                    println!("{}", serde_json::to_string_pretty(&response.data)?);
                } else if let Ok(statuses) =
                    serde_json::from_value::<Vec<crate::manager::Status>>(response.data.clone())
                {
                    for status in statuses {
                        println!(
                            "{}\t{:?}/{}\tPID={}\t重启={}\t版本={}\t启用={}\t{}",
                            status.name,
                            status.state,
                            status.substate,
                            status
                                .pid
                                .map(|p| p.to_string())
                                .unwrap_or_else(|| "-".into()),
                            status.restart_count,
                            status.config_version,
                            status.enabled,
                            status.reason.as_deref().unwrap_or("")
                        );
                    }
                } else {
                    println!("{}", response.data);
                }
            }
            break;
        }
    }
    Ok(())
}

#[cfg(not(windows))]
/// 非 Windows 不提供命名管道客户端。参数：目录、动作及显示格式。返回：不支持错误。
pub async fn client(_: &Path, _: Action, _: bool) -> Result<()> {
    Err(Error::Operation("CLI 托管控制仅支持 Windows".into()))
}
