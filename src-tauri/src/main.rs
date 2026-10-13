//! 当前用户会话中的 Tauri 入口、托盘及受控命令。
//!
//! 桌面对同一数据目录有两种运行方式：没有开机自启守护进程时自己托管子进程；
//! 已注册开机自启后（登录或系统启动时）作为客户端连接守护进程，二者自动切换。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::{Parser, ValueEnum};
use rpmm::{
    desktop::{self, Document, Preferences},
    ipc::{self, Action},
    logging::Record,
    manager::{Manager, Status},
};
use serde::Serialize;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tauri::{
    Emitter, Manager as _, State,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::{oneshot, watch};

#[derive(Parser)]
#[command(about = "rpmm 桌面进程管理器")]
struct Args {
    #[arg(long)]
    root: Option<PathBuf>,
    #[arg(long)]
    hidden: bool,
    /// 服务控制管理器入口：不创建窗口，直接托管并按控制码停止
    #[arg(long)]
    service: bool,
    /// 注册或删除开机自启登记后退出，供提升权限后调用
    #[arg(long, value_enum)]
    boot: Option<BootCommand>,
}

#[derive(Clone, Copy, ValueEnum)]
enum BootCommand {
    Install,
    Uninstall,
}

/// 管理器的访问方式。
enum Link {
    /// 本进程持有管理器并监听命名管道。
    Local(Arc<Manager>),
    /// 连接由开机自启拉起的守护进程。
    Remote(PathBuf),
}

struct DesktopState {
    link: Link,
    root: PathBuf,
    preferences: Mutex<Preferences>,
    next_root: Mutex<PathBuf>,
    settings_lock: tokio::sync::Mutex<()>,
    stop: watch::Sender<bool>,
    quitting: AtomicBool,
    metrics: Arc<Mutex<rpmm::metrics::Collector>>,
}

impl DesktopState {
    /// 判断是否由守护进程托管。参数：无。返回：是否处于客户端模式。
    fn daemon(&self) -> bool {
        matches!(self.link, Link::Remote(_))
    }
    /// 在本地管理器或守护进程上执行协议动作。参数：action 为动作。返回：响应数据或诊断。
    async fn dispatch(&self, action: Action) -> Result<Value, String> {
        match &self.link {
            Link::Local(manager) => ipc::execute(manager, &self.metrics, action)
                .await
                .map_err(|error| error.to_string()),
            Link::Remote(root) => ipc::call(root, action)
                .await
                .map_err(|error| error.to_string()),
        }
    }
}

#[derive(Serialize)]
struct Settings {
    root: String,
    next_root: String,
    preferences: Preferences,
    /// 登录自启动：当前用户登录 Windows 后打开控制台。
    autostart: bool,
    /// 开机自启：由系统启动的守护进程托管子进程，不需要登录。
    boot: rpmm::service::Registration,
    /// 当前进程是否已提升为管理员，注册开机自启需要。
    elevated: bool,
    /// 是否由开机自启的守护进程托管。
    daemon: bool,
}

/// 反序列化响应数据。参数：value 为响应，message 为诊断前缀。返回：解析结果或诊断。
fn decode<T: serde::de::DeserializeOwned>(value: Value, message: &str) -> Result<T, String> {
    serde_json::from_value(value).map_err(|error| format!("{message}：{error}"))
}

/// 查询所有托管子进程。参数：state 为共享状态。返回：状态快照或诊断。
#[tauri::command]
async fn status(state: State<'_, DesktopState>) -> Result<Vec<Status>, String> {
    let value = state.dispatch(Action::List).await?;
    decode(value, "状态数据无法解析")
}

/// 调度进程操作。参数：state 为管理器，unit 为名称，action 为操作。返回：执行结果。
#[tauri::command]
async fn operate(
    state: State<'_, DesktopState>,
    unit: String,
    action: String,
) -> Result<(), String> {
    if state.quitting.load(Ordering::SeqCst) {
        return Err("应用正在退出".into());
    }
    // 操作名称使用白名单；所有变更通过核心事务锁协调。
    let action = match action.as_str() {
        "start" => Action::Start { unit },
        "stop" => Action::Stop { unit },
        "restart" => Action::Restart { unit },
        "enable" => Action::Enable { unit },
        "disable" => Action::Disable { unit },
        "reset-failed" => Action::ResetFailed { unit },
        _ => return Err("不支持的操作".into()),
    };
    state.dispatch(action).await.map(|_| ())
}

/// 重载磁盘配置。参数：state 为状态。返回：配置版本或诊断。
#[tauri::command]
async fn reload(state: State<'_, DesktopState>) -> Result<u64, String> {
    let value = state.dispatch(Action::DaemonReload).await?;
    decode(value, "配置版本无法解析")
}

/// 读取可编辑的配置列表。参数：state 为状态，unit 为名称。返回：配置正文及名称。
#[tauri::command]
async fn documents(state: State<'_, DesktopState>, unit: String) -> Result<Vec<Document>, String> {
    let value = state.dispatch(Action::Documents { unit }).await?;
    decode(value, "配置数据无法解析")
}

/// 校验、保存并重载配置。参数：state 为状态，name/text/expected 为名称、候选及编辑时原文。返回：版本或诊断。
#[tauri::command]
async fn save_document(
    state: State<'_, DesktopState>,
    name: String,
    text: String,
    expected: Option<String>,
) -> Result<u64, String> {
    let value = state
        .dispatch(Action::SaveDocument {
            name,
            text,
            expected,
        })
        .await?;
    decode(value, "配置版本无法解析")
}

/// 删除一个服务的全部配置。参数：state 为状态，unit 为名称，stop 表示是否先停止实例，expected 为编辑时主配置原文。返回：版本或诊断。
#[tauri::command]
async fn delete_service(
    state: State<'_, DesktopState>,
    unit: String,
    stop: bool,
    expected: Option<String>,
) -> Result<u64, String> {
    if state.quitting.load(Ordering::SeqCst) {
        return Err("应用正在退出".into());
    }
    let value = state
        .dispatch(Action::Delete {
            unit,
            stop,
            expected,
        })
        .await?;
    decode(value, "配置版本无法解析")
}

/// 查询历史日志。参数：state 为状态，unit/source/lines 为名称、可选来源和行数。返回：最近的有界记录。
#[tauri::command]
async fn logs(
    state: State<'_, DesktopState>,
    unit: String,
    source: Option<String>,
    lines: usize,
) -> Result<Vec<Record>, String> {
    // 查询在阻塞线程读取文件，避免持续日志影响界面和监督任务。
    rpmm::config::validate_name(&unit).map_err(|error| error.to_string())?;
    if source
        .as_deref()
        .is_some_and(|value| !matches!(value, "stdout" | "stderr" | "manager" | "health"))
    {
        return Err("无效的日志来源".into());
    }
    if !matches!(lines, 200 | 500 | 1000 | 10000) {
        return Err("日志行数必须为 200/500/1000/10000".into());
    }
    let value = match &state.link {
        Link::Local(manager) => {
            let logger = manager.logger.clone();
            let source = source.clone();
            let records = tauri::async_runtime::spawn_blocking(move || {
                logger.tail(&unit, source.as_deref(), lines)
            })
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())?;
            serde_json::to_value(records).map_err(|error| error.to_string())?
        }
        Link::Remote(_) => {
            state
                .dispatch(Action::Logs {
                    unit,
                    source,
                    lines,
                    follow: false,
                })
                .await?
        }
    };
    decode(value, "日志数据无法解析")
}

/// 获取整机与进程资源。参数：state 为共享状态，unit 为可选托管名称，details 表示是否读取连接明细。返回：资源快照和采集诊断。
#[tauri::command]
async fn metrics(
    state: State<'_, DesktopState>,
    unit: Option<String>,
    details: bool,
) -> Result<Value, String> {
    // 资源由持有子进程的一方采集：本地模式读取自身进程，客户端模式由守护进程返回。
    match &state.link {
        // 采样要枚举全机进程，放在阻塞线程上，避免持续影响界面刷新与监督任务。
        Link::Local(manager) => {
            let manager = manager.clone();
            let metrics = state.metrics.clone();
            tauri::async_runtime::spawn_blocking(move || {
                ipc::sample_metrics(&manager, &metrics, unit.as_deref(), details)
            })
            .await
            .map_err(|error| error.to_string())?
            .map_err(|error| error.to_string())
        }
        Link::Remote(_) => state.dispatch(Action::Metrics { unit, details }).await,
    }
}

/// 查询健康状态。参数：state 为管理器。返回：全部托管进程的健康检查快照。
#[tauri::command]
async fn health_status(state: State<'_, DesktopState>) -> Result<Value, String> {
    state.dispatch(Action::HealthStatus).await
}

/// 查询独立探测历史。参数：state 为管理器，unit 为托管名称。返回：最近一百次内存探测结果。
#[tauri::command]
async fn health_history(
    state: State<'_, DesktopState>,
    unit: String,
) -> Result<Vec<rpmm::health::HealthRecord>, String> {
    let value = state.dispatch(Action::HealthHistory { unit }).await?;
    decode(value, "健康历史无法解析")
}

/// 导出多选服务配置。参数：app/state 为上下文，units 为选中服务。返回：保存路径或取消。
#[tauri::command]
async fn export_configs(
    app: tauri::AppHandle,
    state: State<'_, DesktopState>,
    units: Vec<String>,
) -> Result<Option<String>, String> {
    let bundle = state.dispatch(Action::ExportBundle { units }).await?;
    let bytes = serde_json::to_vec_pretty(&bundle).map_err(|e| e.to_string())?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("配置包超过 16 MiB，请减少导出的服务数量".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .set_title("导出服务配置")
            .set_file_name("rpmm-config.json")
            .add_filter("结构化配置", &["json"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 从系统对话框导入完整配置包。参数：app/state 为上下文，overwrite 为是否替换同名服务。返回：版本或取消。
#[tauri::command]
async fn import_configs(
    app: tauri::AppHandle,
    state: State<'_, DesktopState>,
    overwrite: bool,
) -> Result<Option<u64>, String> {
    let bundle = tauri::async_runtime::spawn_blocking(move || {
        use std::io::Read;
        let Some(file) = app
            .dialog()
            .file()
            .set_title("导入服务配置")
            .add_filter("结构化配置", &["json"])
            .blocking_pick_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("配置包超过 16 MiB".to_string());
        }
        serde_json::from_slice::<desktop::ConfigBundle>(&bytes)
            .map(Some)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())??;
    match bundle {
        Some(bundle) => {
            let value = state
                .dispatch(Action::ImportBundle { bundle, overwrite })
                .await?;
            decode(value, "配置版本无法解析").map(Some)
        }
        None => Ok(None),
    }
}

/// 选择目录。参数：app 为窗口上下文。返回：绝对目录或取消。
#[tauri::command]
async fn choose_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_title("选择目录")
            .blocking_pick_folder()
            .map(|file| {
                file.into_path()
                    .map(|path| path.display().to_string())
                    .map_err(|e| e.to_string())
            })
            .transpose()
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 导出当前日志结果到用户选定文件。参数：app 为窗口应用，unit 为名称，text 为显示快照。返回：保存路径或取消。
#[tauri::command]
async fn export_logs(
    app: tauri::AppHandle,
    unit: String,
    text: String,
) -> Result<Option<String>, String> {
    rpmm::config::validate_name(&unit).map_err(|e| e.to_string())?;
    if text.len() > 100 * 1024 * 1024 {
        return Err("导出日志超过 100 MiB".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        let Some(file) = app
            .dialog()
            .file()
            .set_title("下载运行日志")
            .set_file_name(format!("{unit}.log"))
            .add_filter("日志文件", &["log", "txt"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = file.into_path().map_err(|e| e.to_string())?;
        std::fs::write(&path, text.as_bytes()).map_err(|e| e.to_string())?;
        Ok(Some(path.display().to_string()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 查询子进程配置的界面分类。参数：state 为共享状态。返回：已归一化的分类表。
#[tauri::command]
async fn categories(state: State<'_, DesktopState>) -> Result<desktop::Categories, String> {
    let value = state.dispatch(Action::Categories).await?;
    decode(value, "分类数据无法解析")
}

/// 保存子进程配置的界面分类。参数：state 为共享状态，categories 为分类表。返回：保存结果。
#[tauri::command]
async fn save_categories(
    state: State<'_, DesktopState>,
    categories: desktop::Categories,
) -> Result<(), String> {
    state
        .dispatch(Action::SaveCategories { categories })
        .await
        .map(|_| ())
}

/// 查询持久化设置、登录自启动与开机自启状态。参数：state 为共享状态。返回：设置与状态。
#[tauri::command]
async fn settings(state: State<'_, DesktopState>) -> Result<Settings, String> {
    let root = state.root.display().to_string();
    let next_root = state.next_root.lock().unwrap().display().to_string();
    let preferences = state.preferences.lock().unwrap().clone();
    let daemon = state.daemon();
    let (autostart, boot, elevated) = tauri::async_runtime::spawn_blocking(|| {
        (
            desktop::startup::is_enabled(),
            rpmm::service::registration(),
            rpmm::service::is_elevated(),
        )
    })
    .await
    .map_err(|error| error.to_string())?;
    Ok(Settings {
        root,
        next_root,
        preferences,
        autostart: autostart.map_err(|error| error.to_string())?,
        boot: boot.map_err(|error| error.to_string())?,
        elevated: elevated.map_err(|error| error.to_string())?,
        daemon,
    })
}

/// 保存设置及当前用户登录自启动。参数：state 为上下文，preferences/autostart/root 为偏好、自启动及下次启动目录。返回：结果。
#[tauri::command]
async fn save_settings(
    state: State<'_, DesktopState>,
    preferences: Preferences,
    autostart: bool,
    root: String,
) -> Result<(), String> {
    let _guard = state.settings_lock.lock().await;
    desktop::validate_preferences(&preferences).map_err(|error| error.to_string())?;
    // 用户输入接受 C:/、C:\、/c/、/cygdrive/c/ 等 Windows 绝对路径写法。
    let root = rpmm::paths::normalize_path(Path::new(&root));
    if !root.is_absolute() {
        return Err("数据目录必须为绝对路径".into());
    }
    // 新目录先验证配置与写入权限；正在运行的管理器保持当前目录，下次启动切换。
    Manager::new(&root).map_err(|error| error.to_string())?;
    let previous = desktop::startup::snapshot().map_err(|error| error.to_string())?;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let command = desktop::autostart_command(&executable, &root);
    let mut snapshots = std::collections::BTreeMap::new();
    for path in [
        root.join("state/desktop.json"),
        state.root.join("state/desktop.json"),
        desktop::default_root().join("state/data-root.json"),
    ] {
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.to_string()),
        };
        snapshots.insert(path, bytes);
    }
    let result = desktop::startup::set_enabled(autostart, &command)
        .and_then(|()| desktop::save_preferences(&root, &preferences))
        .and_then(|()| desktop::save_preferences(&state.root, &preferences))
        .and_then(|()| desktop::save_root(&root));
    // 设置写盘失败时恢复偏好、目录选择和自启动状态，完整报告恢复失败。
    if let Err(error) = result {
        let mut errors = vec![error.to_string()];
        for (path, bytes) in snapshots {
            let restored = if let Some(bytes) = bytes {
                rpmm::manager::atomic_write(&path, &bytes)
            } else if path.exists() {
                std::fs::remove_file(path).map_err(rpmm::Error::from)
            } else {
                Ok(())
            };
            if let Err(error) = restored {
                errors.push(format!("恢复设置失败：{error}"));
            }
        }
        if let Err(error) = desktop::startup::restore(previous) {
            errors.push(format!("恢复自启动失败：{error}"));
        }
        return Err(errors.join("；"));
    }
    *state.preferences.lock().unwrap() = preferences;
    *state.next_root.lock().unwrap() = root;
    Ok(())
}

/// 注册或删除开机自启服务。参数：state 为上下文，enabled 为开关。返回：需要提示用户的说明。
/// 注册需要管理员权限，界面无法自行提升，因此改为以管理员身份重新运行本程序的 --boot 子命令。
#[tauri::command]
async fn set_boot_autostart(
    state: State<'_, DesktopState>,
    enabled: bool,
) -> Result<Option<String>, String> {
    let _guard = state.settings_lock.lock().await;
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    let arguments = vec![
        "--root".into(),
        state.root.display().to_string(),
        "--boot".into(),
        if enabled { "install" } else { "uninstall" }.into(),
    ];
    let code = tauri::async_runtime::spawn_blocking(move || {
        rpmm::service::elevate(&executable, &arguments)
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;
    let registration = tauri::async_runtime::spawn_blocking(rpmm::service::registration)
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
    if registration.installed != enabled {
        return Err(format!(
            "开机自启未按预期变更（提升进程退出码 {code}）；请以管理员身份运行 rpmm autostart {} 查看原因",
            if enabled { "install" } else { "uninstall" }
        ));
    }
    // 本机正在托管同一数据目录时守护进程无法同时监听，提示用户移交方式。
    if enabled && !state.daemon() {
        return Ok(Some(
            "已注册开机自启，守护进程将在下次系统启动时接管；当前控制台退出后可用 rpmm autostart start 立即移交"
                .into(),
        ));
    }
    Ok(None)
}

/// 隐藏窗口到托盘。参数：window 为主窗口。返回：窗口操作结果。
#[tauri::command]
fn hide_window(window: tauri::WebviewWindow) -> Result<(), String> {
    window.hide().map_err(|error| error.to_string())
}

/// 请求有序退出。参数：app 为应用。返回：无，完成后由服务器结束事件循环。
#[tauri::command]
fn quit(app: tauri::AppHandle) {
    request_quit(&app);
}

/// 恢复并聚焦主窗口。参数：app 为应用。返回：无。
fn show_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 防止重复退出并通知后台清理。参数：app 为应用。返回：无。
/// 客户端模式只关闭控制台，守护进程与其托管的子进程继续运行。
fn request_quit(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<DesktopState>()
        && !state.quitting.swap(true, Ordering::SeqCst)
    {
        let _ = app.emit("quitting", ());
        if state.daemon() {
            app.exit(0);
        } else {
            let _ = state.stop.send(true);
        }
    }
}

/// 创建常驻托盘。参数：app 为应用，daemon 表示是否由守护进程托管。返回：安装结果。
fn setup_tray(app: &tauri::App, daemon: bool) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "打开进程控制台", true, None::<&str>)?;
    let quit = MenuItem::with_id(
        app,
        "quit",
        if daemon {
            "退出控制台（守护进程继续运行）"
        } else {
            "退出并停止所有子进程"
        },
        true,
        None::<&str>,
    )?;
    let menu = Menu::with_items(app, &[&show, &quit])?;
    TrayIconBuilder::with_id("rpmm")
        // 托盘直接使用加粗的单色小图，避免系统缩放大图时丢失细线和节点对比度。
        .icon(tauri::include_image!("icons/32x32.png"))
        .tooltip("rpmm · 子进程管理器")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_window(app),
            "quit" => request_quit(app),
            _ => (),
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// 注册或删除开机自启登记。参数：root 为数据目录，command 为动作。返回：结果。
fn boot_command(root: &Path, command: BootCommand) -> rpmm::Result<()> {
    match command {
        BootCommand::Install => rpmm::service::install(&rpmm::service::InstallOptions {
            executable: std::env::current_exe()?,
            // 服务入口只托管子进程，不创建窗口。
            arguments: vec!["--service".into()],
            root: root.to_path_buf(),
            account: None,
            password: None,
        }),
        BootCommand::Uninstall => rpmm::service::uninstall(),
    }
}

/// 初始化桌面、后台监督与退出协调。参数：无，读取命令行。返回：无。
fn main() {
    let args = Args::parse();
    let root =
        std::path::absolute(rpmm::paths::normalize_path(&args.root.unwrap_or_else(
            || desktop::configured_root().expect("无法读取数据目录设置"),
        )))
        .expect("无法解析数据目录");
    // 服务入口与开机自启登记都在创建窗口之前处理。
    if args.service {
        if let Err(error) = rpmm::service::run(&root) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if let Some(command) = args.boot {
        let result = boot_command(&root, command);
        if let Err(error) = &result {
            eprintln!("{error}");
        }
        std::process::exit(if result.is_ok() { 0 } else { 1 });
    }
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_window(app)
        }))
        .invoke_handler(tauri::generate_handler![
            status,
            operate,
            reload,
            documents,
            save_document,
            delete_service,
            logs,
            metrics,
            health_status,
            health_history,
            export_logs,
            export_configs,
            import_configs,
            choose_directory,
            categories,
            save_categories,
            settings,
            save_settings,
            set_boot_autostart,
            hide_window,
            quit
        ])
        .setup(move |app| {
            let preferences = desktop::load_preferences(&root)?;
            let hidden = args.hidden || preferences.start_hidden;
            let (stop, receiver) = watch::channel(false);
            // 先探测命名管道：已有守护进程时作为客户端连接，绝不重复托管同一数据目录。
            let link = match tauri::async_runtime::block_on(ipc::presence(&root)) {
                ipc::Presence::Available => Link::Remote(root.clone()),
                // 守护进程以服务账户运行时管道只允许系统账户与管理员，未提升的控制台必须明确告知。
                ipc::Presence::Denied => {
                    rpmm::platform::win::message_box(
                        "rpmm 无法连接守护进程",
                        &format!(
                            "数据目录 {} 已由开机自启的守护进程托管，但它以服务账户运行，当前权限无法连接命名管道。\n\n请以管理员身份运行控制台，或用 rpmm autostart status 查看服务状态。",
                            root.display()
                        ),
                    );
                    std::process::exit(1);
                }
                ipc::Presence::Absent => {
                let manager = Manager::new(&root)?;
                let (ready, readiness) = oneshot::channel();
                let server =
                    tauri::async_runtime::spawn(ipc::serve(manager.clone(), receiver, ready));
                tauri::async_runtime::block_on(readiness)??;
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let failure = match server.await {
                        Ok(Ok(())) => None,
                        Ok(Err(error)) => Some(error.to_string()),
                        Err(error) => Some(error.to_string()),
                    };
                    if let Some(error) = failure {
                        let state = handle.state::<DesktopState>();
                        if let Link::Local(manager) = &state.link {
                            let _ = manager.logger.write(
                                "manager",
                                0,
                                "manager",
                                &format!("管理器关闭失败：{error}"),
                            );
                        }
                    }
                    handle.exit(0);
                });
                Link::Local(manager)
                }
            };
            let daemon = matches!(link, Link::Remote(_));
            app.manage(DesktopState {
                link,
                root: root.clone(),
                preferences: Mutex::new(preferences),
                next_root: Mutex::new(root.clone()),
                settings_lock: tokio::sync::Mutex::new(()),
                stop,
                quitting: AtomicBool::new(false),
                metrics: Arc::new(Mutex::new(rpmm::metrics::Collector::default())),
            });
            setup_tray(app, daemon)?;
            if !hidden {
                show_window(app.handle());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关闭与最小化只隐藏窗口，监督任务和托盘继续运行。
            match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                tauri::WindowEvent::Resized(_) if window.is_minimized().unwrap_or(false) => {
                    let _ = window.hide();
                }
                _ => (),
            }
        })
        .build(tauri::generate_context!())
        .expect("桌面应用初始化失败");
    app.run(|app, event| {
        if let tauri::RunEvent::ExitRequested {
            code: None, api, ..
        } = event
        {
            api.prevent_exit();
            request_quit(app);
        }
    });
}
