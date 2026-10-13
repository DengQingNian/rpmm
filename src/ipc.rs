//! 有版本的本机命名管道协议；每个连接一个请求。
use crate::{Error, Result, logging::Record, manager::Manager, metrics::Collector};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    sync::{oneshot, watch},
};

pub const VERSION: u32 = 2;
/// 单个请求或响应的上限；配置包最大 16 MiB，帧上限为其两倍。
pub const MAX_FRAME: u64 = 32 * 1024 * 1024;

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
        /// 编辑时的主配置原文，用于拒绝覆盖他人修改。
        #[serde(default)]
        expected: Option<String>,
    },
    Logs {
        unit: String,
        source: Option<String>,
        lines: usize,
        follow: bool,
    },
    /// 读取可编辑的配置文档。
    Documents {
        unit: String,
    },
    /// 校验并保存配置文档，expected 为编辑时的原文。
    SaveDocument {
        name: String,
        text: String,
        expected: Option<String>,
    },
    /// 读取界面分类。
    Categories,
    /// 保存界面分类。
    SaveCategories {
        categories: crate::desktop::Categories,
    },
    /// 导出选中服务的配置包。
    ExportBundle {
        units: Vec<String>,
    },
    /// 导入整包配置。
    ImportBundle {
        bundle: crate::desktop::ConfigBundle,
        overwrite: bool,
    },
    /// 查询全部健康检查快照。
    HealthStatus,
    /// 查询单个服务的健康检查历史。
    HealthHistory {
        unit: String,
    },
    /// 采集宿主机与托管进程资源。
    Metrics {
        unit: Option<String>,
        #[serde(default)]
        details: bool,
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

/// 校验日志来源参数。参数：source 为可选来源。返回：校验结果。
fn validate_source(source: Option<&str>) -> Result<()> {
    if source.is_some_and(|value| !matches!(value, "stdout" | "stderr" | "manager" | "health")) {
        return Err(Error::Operation(
            "日志来源只能是 stdout/stderr/manager/health".into(),
        ));
    }
    Ok(())
}

/// 采集宿主机与托管进程资源。参数：manager/metrics 为管理器和采集器，unit 为可选服务，details 表示是否读取环境与连接明细。返回：资源快照。
/// 采集后再次核对实例编号，退出或重启期间不会把旧进程资源显示成新实例。
/// 采样会枚举全机进程，进程内调用方应放在阻塞线程上执行。
pub fn sample_metrics(
    manager: &Arc<Manager>,
    metrics: &Mutex<Collector>,
    unit: Option<&str>,
    details: bool,
) -> Result<Value> {
    let selected = match unit {
        Some(name) => {
            let status = manager.status(Some(name))?.remove(0);
            let definition = manager.unit_snapshot(name)?;
            Some((status, definition))
        }
        None => None,
    };
    let pid = selected.as_ref().and_then(|(status, _)| status.pid);
    let (host, mut process) = metrics.lock().unwrap().sample(pid);
    if details && let Some(resources) = process.as_mut() {
        if resources.environment.is_empty()
            && let Some((_, definition)) = &selected
        {
            resources.environment = crate::platform::environment(definition);
            resources.environment_source = "启动配置与继承环境快照（进程内部修改不在此列）".into();
        }
        match crate::metrics::connections(resources.pid) {
            Ok(connections) => resources.connections = connections,
            Err(error) => resources.connection_error = Some(error.to_string()),
        }
    }
    if let Some((status, _)) = &selected
        && !manager
            .status(Some(&status.name))
            .is_ok_and(|value| value[0].pid == status.pid && value[0].instance == status.instance)
    {
        process = None;
    }
    Ok(
        serde_json::json!({ "host": host, "process": process, "instance": selected.map(|(status, _)| status.instance) }),
    )
}

/// 执行一次请求。参数：manager 为管理器，metrics 为资源采集器，action 为动作。返回：响应数据。
/// 需要流式追加的日志跟随请求由连接处理器单独处理，其余请求都在这里完成。
pub async fn execute(
    manager: &Arc<Manager>,
    metrics: &Mutex<Collector>,
    action: Action,
) -> Result<Value> {
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
        Action::Delete {
            unit,
            stop,
            expected,
        } => {
            manager
                .delete_service(&unit, stop, expected.as_deref())
                .await?;
            Ok(Value::Null)
        }
        Action::Logs {
            unit,
            source,
            lines,
            follow: false,
        } => {
            crate::config::validate_name(&unit)?;
            manager.status(Some(&unit))?;
            validate_source(source.as_deref())?;
            Ok(serde_json::to_value(manager.logger.tail(
                &unit,
                source.as_deref(),
                lines,
            )?)?)
        }
        Action::Documents { unit } => Ok(serde_json::to_value(crate::desktop::documents(
            &manager.root,
            &unit,
        )?)?),
        Action::SaveDocument {
            name,
            text,
            expected,
        } => Ok(serde_json::to_value(
            manager
                .save_document(&name, &text, expected.as_deref())
                .await?,
        )?),
        Action::Categories => Ok(serde_json::to_value(crate::desktop::load_categories(
            &manager.root,
        )?)?),
        Action::SaveCategories { categories } => {
            crate::desktop::save_categories(&manager.root, &categories)?;
            Ok(Value::Null)
        }
        Action::ExportBundle { units } => {
            let statuses = units
                .iter()
                .map(|name| manager.status(Some(name)).map(|mut value| value.remove(0)))
                .collect::<Result<Vec<_>>>()?;
            Ok(serde_json::to_value(crate::desktop::export_bundle(
                &manager.root,
                &statuses,
            )?)?)
        }
        Action::ImportBundle { bundle, overwrite } => Ok(serde_json::to_value(
            manager.import_bundle(bundle, overwrite).await?,
        )?),
        Action::HealthStatus => Ok(serde_json::to_value(manager.health_status()?)?),
        Action::HealthHistory { unit } => Ok(serde_json::to_value(manager.health_history(&unit)?)?),
        Action::Metrics { unit, details } => {
            sample_metrics(manager, metrics, unit.as_deref(), details)
        }
        Action::Logs { follow: true, .. } | Action::Shutdown => {
            Err(Error::Operation("此请求应由连接处理器协调".into()))
        }
    }
}

/// 处理连接。参数：stream、manager、metrics、stop、shutdown 为管道、管理器、资源采集器和关闭通知。返回：结果。
async fn connection<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(
    stream: S,
    manager: Arc<Manager>,
    metrics: Arc<Mutex<Collector>>,
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
    // 只有显式要求跟随的日志请求走流式通道，其余请求统一由 execute 返回完整数据。
    let streaming = matches!(&request.action, Action::Logs { follow: true, .. });
    if streaming {
        let Action::Logs {
            unit,
            source,
            lines,
            ..
        } = request.action
        else {
            return Err(Error::Operation("日志请求解析失败".into()));
        };
        let snapshot = (|| {
            crate::config::validate_name(&unit)?;
            manager.status(Some(&unit))?;
            validate_source(source.as_deref())?;
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
        let response = match execute(&manager, &metrics, request.action).await {
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
    // 资源采集器由服务端持有，客户端只取快照；被托管的进程始终由本进程采样。
    // 首次枚举全机进程较慢，必须在管道出现之前完成，否则客户端会在就绪信号之后撞上尚未开始接收的管道实例。
    let metrics = Arc::new(Mutex::new(Collector::default()));
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
                let shutdown = shutdown.clone(); let metrics = metrics.clone();
                // 活跃连接数量有上限，避免授权客户端无限消耗内存。
                if connections.len() >= 64 { drop(connected); continue; }
                connections.spawn(async move { if let Err(e) = connection(connected, manager.clone(), metrics, stop, shutdown).await { let _ = manager.logger.write("manager", 0, "manager", &format!("IPC 连接结束：{e}")); } });
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

/// 打印一条日志记录。参数：record 为记录，json 表示输出原始 JSON。返回：结果。
fn print_record(record: &Record, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string(record)?);
        return Ok(());
    }
    print!("{} [{}] {}", record.time, record.source, record.text);
    if !record.text.ends_with('\n') {
        println!();
    }
    Ok(())
}

#[cfg(windows)]
/// 连接数据目录上的管理器管道。参数：root 为数据目录，wait 为忙等重试上限。返回：已连接的管道。
async fn connect(
    root: &Path,
    wait: Duration,
) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let name = pipe_name(root);
    let deadline = tokio::time::Instant::now() + wait;
    loop {
        // ERROR_PIPE_BUSY 表示命名管道实例正在被占用，短暂退避后重试。
        match ClientOptions::new().open(&name) {
            Ok(pipe) => return Ok(pipe),
            Err(error)
                if error.raw_os_error() == Some(231) && tokio::time::Instant::now() < deadline =>
            {
                tokio::time::sleep(Duration::from_millis(50)).await
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(windows)]
/// 向运行中的管理器发送动作并输出结果。参数：root/action/json 为目录、动作和显示模式。返回：结果。
pub async fn client(root: &Path, action: Action, json: bool) -> Result<()> {
    let pipe = connect(root, Duration::from_secs(5))
        .await
        .map_err(access_hint)?;
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
        check(&response)?;
        if log_mode {
            if response.data.is_null() {
                break;
            }
            // 非跟随请求一次性返回记录数组，跟随请求逐条返回。
            match serde_json::from_value::<Vec<Record>>(response.data.clone()) {
                Ok(records) => {
                    for record in records {
                        print_record(&record, json)?;
                    }
                    break;
                }
                Err(_) => print_record(&serde_json::from_value(response.data)?, json)?,
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

#[cfg(windows)]
/// 在运行中的管理器上执行动作并读取唯一响应。参数：root 为数据目录，action 为动作。返回：响应数据。
/// 供桌面程序以客户端方式访问守护进程；日志跟随请求不适用。
pub async fn call(root: &Path, action: Action) -> Result<Value> {
    let pipe = connect(root, Duration::from_secs(5))
        .await
        .map_err(access_hint)?;
    let mut stream = BufReader::new(pipe);
    write_frame(
        stream.get_mut(),
        &Request {
            version: VERSION,
            action,
        },
    )
    .await?;
    let response: Response = read_frame(&mut stream).await?;
    check(&response)?;
    Ok(response.data)
}

/// 数据目录上管理器的探测结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    /// 没有管理器监听，可以自行托管。
    Absent,
    /// 已有管理器且当前账户可以连接。
    Available,
    /// 已有管理器但当前账户无权连接：守护进程通常以系统账户运行，控制台需要提升权限。
    Denied,
}

#[cfg(windows)]
/// 探测数据目录上的管理器。参数：root 为数据目录。返回：探测结果。
/// 只看管道能否连接：协议版本是否兼容由后续请求报告，避免把旧版本误判成没有管理器。
pub async fn presence(root: &Path) -> Presence {
    match connect(root, Duration::from_secs(2)).await {
        Ok(_) => Presence::Available,
        // ERROR_ACCESS_DENIED 说明管理器存在但当前账户不在管道访问控制列表内。
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => Presence::Denied,
        Err(_) => Presence::Absent,
    }
}

#[cfg(windows)]
/// 把连接失败翻译成可操作的诊断。参数：error 为连接错误。返回：统一错误。
/// 守护进程以系统账户运行时，普通权限的控制台会被命名管道访问控制拒绝，这里给出提示而不是裸的拒绝访问。
fn access_hint(error: std::io::Error) -> Error {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        Error::Operation("拒绝访问：守护进程以服务账户运行，请以管理员身份运行控制台或命令".into())
    } else {
        Error::from(error)
    }
}

#[cfg(windows)]
/// 校验响应版本与结果。参数：response 为响应。返回：结果。
fn check(response: &Response) -> Result<()> {
    if response.version != VERSION {
        return Err(Error::Operation("响应协议版本不兼容".into()));
    }
    if !response.ok {
        return Err(Error::Operation(format!(
            "{}: {}",
            response.code, response.data
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
/// 非 Windows 不提供命名管道客户端。参数：目录、动作及显示格式。返回：不支持错误。
pub async fn client(_: &Path, _: Action, _: bool) -> Result<()> {
    Err(Error::Operation("CLI 托管控制仅支持 Windows".into()))
}

#[cfg(not(windows))]
/// 非 Windows 不提供命名管道客户端。参数：目录与动作。返回：不支持错误。
pub async fn call(_: &Path, _: Action) -> Result<Value> {
    Err(Error::Operation("托管控制仅支持 Windows".into()))
}

#[cfg(not(windows))]
/// 非 Windows 没有命名管道。参数：目录。返回：始终认为没有管理器。
pub async fn presence(_: &Path) -> Presence {
    Presence::Absent
}
