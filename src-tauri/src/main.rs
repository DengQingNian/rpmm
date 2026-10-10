//! 当前用户会话中的 Tauri 入口、托盘及受控命令。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use clap::Parser;
use rpmm::{
    desktop::{self, Document, Preferences},
    ipc,
    logging::Record,
    manager::{Manager, Status},
};
use serde::Serialize;
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
}

struct DesktopState {
    manager: Arc<Manager>,
    preferences: Mutex<Preferences>,
    next_root: Mutex<PathBuf>,
    settings_lock: tokio::sync::Mutex<()>,
    stop: watch::Sender<bool>,
    quitting: AtomicBool,
    metrics: Arc<Mutex<rpmm::metrics::Collector>>,
}

#[derive(Serialize)]
struct Settings {
    root: String,
    next_root: String,
    preferences: Preferences,
    autostart: bool,
}

/// 查询所有托管子进程。参数：state 为共享状态。返回：状态快照或诊断。
#[tauri::command]
fn status(state: State<'_, DesktopState>) -> Result<Vec<Status>, String> {
    state
        .manager
        .status(None)
        .map_err(|error| error.to_string())
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
    let result = match action.as_str() {
        "start" => state.manager.start(&[unit]).await,
        "stop" => state.manager.stop(&[unit]).await.map(|_| ()),
        "restart" => state.manager.restart(&[unit]).await,
        "enable" => state.manager.enable(&unit, true).await,
        "disable" => state.manager.enable(&unit, false).await,
        "reset-failed" => state.manager.reset_failed(&unit).await,
        _ => return Err("不支持的操作".into()),
    };
    result.map_err(|error| error.to_string())
}

/// 重载磁盘配置。参数：state 为状态。返回：配置版本或诊断。
#[tauri::command]
async fn reload(state: State<'_, DesktopState>) -> Result<u64, String> {
    state
        .manager
        .reload()
        .await
        .map_err(|error| error.to_string())
}

/// 读取可编辑的配置列表。参数：state 为状态，unit 为名称。返回：配置正文及名称。
#[tauri::command]
fn documents(state: State<'_, DesktopState>, unit: String) -> Result<Vec<Document>, String> {
    desktop::documents(&state.manager.root, &unit).map_err(|error| error.to_string())
}

/// 校验、保存并重载配置。参数：state 为状态，name/text/expected 为名称、候选及编辑时原文。返回：版本或诊断。
#[tauri::command]
async fn save_document(
    state: State<'_, DesktopState>,
    name: String,
    text: String,
    expected: Option<String>,
) -> Result<u64, String> {
    state
        .manager
        .save_document(&name, &text, expected.as_deref())
        .await
        .map_err(|error| error.to_string())
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
    state
        .manager
        .delete_service(&unit, stop, expected.as_deref())
        .await
        .map_err(|error| error.to_string())
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
    let logger = state.manager.logger.clone();
    if !matches!(lines, 200 | 500 | 1000 | 10000) {
        return Err("日志行数必须为 200/500/1000/10000".into());
    }
    tauri::async_runtime::spawn_blocking(move || logger.tail(&unit, source.as_deref(), lines))
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())
}

/// 获取整机与进程资源。参数：state 为共享状态，unit 为可选托管名称，details 表示是否读取连接明细。返回：资源快照和采集诊断。
#[tauri::command]
async fn metrics(
    state: State<'_, DesktopState>,
    unit: Option<String>,
    details: bool,
) -> Result<serde_json::Value, String> {
    let selected = unit
        .as_deref()
        .map(|name| {
            let status = state.manager.status(Some(name))?.remove(0);
            let definition = state.manager.unit_snapshot(name)?;
            Ok::<_, rpmm::Error>((status, definition))
        })
        .transpose()
        .map_err(|e| e.to_string())?;
    let collector = state.metrics.clone();
    let manager = state.manager.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let pid = selected.as_ref().and_then(|(status, _)| status.pid);
        let (host, mut process) = collector.lock().unwrap().sample(pid);
        if details && let Some(resources) = process.as_mut() {
            if resources.environment.is_empty() && let Some((_, definition)) = &selected {
                resources.environment = rpmm::platform::environment(definition);
                resources.environment_source = "启动配置与继承环境快照（进程内部修改不在此列）".into();
            }
            match rpmm::metrics::connections(resources.pid) {
                Ok(connections) => resources.connections = connections,
                Err(error) => resources.connection_error = Some(error.to_string()),
            }
        }
        // 采集后再次核对实例，退出或重启期间的快照不显示为新进程资源。
        if let Some((status, _)) = &selected
            && !manager.status(Some(&status.name)).is_ok_and(|v| v[0].pid == status.pid && v[0].instance == status.instance) { process = None; }
        serde_json::json!({ "host": host, "process": process, "instance": selected.map(|(status, _)| status.instance) })
    }).await.map_err(|e| e.to_string())
}

/// 查询健康状态。参数：state 为管理器。返回：全部托管进程的健康检查快照。
#[tauri::command]
fn health_status(state: State<'_, DesktopState>) -> Result<Vec<serde_json::Value>, String> {
    state.manager.health_status().map_err(|e| e.to_string())
}

/// 查询独立探测历史。参数：state 为管理器，unit 为托管名称。返回：最近一百次内存探测结果。
#[tauri::command]
async fn health_history(
    state: State<'_, DesktopState>,
    unit: String,
) -> Result<Vec<rpmm::health::HealthRecord>, String> {
    state
        .manager
        .health_history(&unit)
        .map_err(|e| e.to_string())
}

/// 导出多选服务配置。参数：app/state 为上下文，units 为选中服务。返回：保存路径或取消。
#[tauri::command]
async fn export_configs(
    app: tauri::AppHandle,
    state: State<'_, DesktopState>,
    units: Vec<String>,
) -> Result<Option<String>, String> {
    let statuses = units
        .iter()
        .map(|name| state.manager.status(Some(name)).map(|mut v| v.remove(0)))
        .collect::<rpmm::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    let bundle =
        desktop::export_bundle(&state.manager.root, &statuses).map_err(|e| e.to_string())?;
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
        Some(bundle) => state
            .manager
            .import_bundle(bundle, overwrite)
            .await
            .map(Some)
            .map_err(|e| e.to_string()),
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
fn categories(state: State<'_, DesktopState>) -> Result<desktop::Categories, String> {
    desktop::load_categories(&state.manager.root).map_err(|error| error.to_string())
}

/// 保存子进程配置的界面分类。参数：state 为共享状态，categories 为分类表。返回：保存结果。
#[tauri::command]
fn save_categories(
    state: State<'_, DesktopState>,
    categories: desktop::Categories,
) -> Result<(), String> {
    desktop::save_categories(&state.manager.root, &categories).map_err(|error| error.to_string())
}

/// 查询持久化设置与真实自启动状态。参数：state 为共享状态。返回：设置及目录。
#[tauri::command]
fn settings(state: State<'_, DesktopState>) -> Result<Settings, String> {
    Ok(Settings {
        root: state.manager.root.display().to_string(),
        next_root: state.next_root.lock().unwrap().display().to_string(),
        preferences: state.preferences.lock().unwrap().clone(),
        autostart: desktop::startup::is_enabled().map_err(|error| error.to_string())?,
    })
}

/// 保存设置及当前用户自启动。参数：state 为上下文，preferences/autostart/root 为偏好、自启动及下次启动目录。返回：结果。
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
        state.manager.root.join("state/desktop.json"),
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
        .and_then(|()| desktop::save_preferences(&state.manager.root, &preferences))
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
fn request_quit(app: &tauri::AppHandle) {
    if let Some(state) = app.try_state::<DesktopState>()
        && !state.quitting.swap(true, Ordering::SeqCst)
    {
        let _ = app.emit("quitting", ());
        let _ = state.stop.send(true);
    }
}

/// 创建常驻托盘。参数：app 为应用。返回：安装结果。
fn setup_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "打开进程控制台", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出并停止所有子进程", true, None::<&str>)?;
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

/// 初始化桌面、后台监督与退出协调。参数：无，读取命令行。返回：无。
fn main() {
    let args = Args::parse();
    let root =
        std::path::absolute(rpmm::paths::normalize_path(&args.root.unwrap_or_else(
            || desktop::configured_root().expect("无法读取数据目录设置"),
        )))
        .expect("无法解析数据目录");
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
            hide_window,
            quit
        ])
        .setup(move |app| {
            // 必须先独占本机管道，再执行 boot，避免与前台管理器重复托管。
            let manager = Manager::new(&root)?;
            let preferences = desktop::load_preferences(&root)?;
            let hidden = args.hidden || preferences.start_hidden;
            let (stop, receiver) = watch::channel(false);
            let (ready, readiness) = oneshot::channel();
            let server = tauri::async_runtime::spawn(ipc::serve(manager.clone(), receiver, ready));
            tauri::async_runtime::block_on(readiness)??;
            app.manage(DesktopState {
                manager,
                preferences: Mutex::new(preferences),
                next_root: Mutex::new(root.clone()),
                settings_lock: tokio::sync::Mutex::new(()),
                stop,
                quitting: AtomicBool::new(false),
                metrics: Arc::new(Mutex::new(rpmm::metrics::Collector::default())),
            });
            setup_tray(app)?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let failure = match server.await {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error.to_string()),
                    Err(error) => Some(error.to_string()),
                };
                if let Some(error) = failure {
                    let state = handle.state::<DesktopState>();
                    let _ = state.manager.logger.write(
                        "manager",
                        0,
                        "manager",
                        &format!("管理器关闭失败：{error}"),
                    );
                }
                handle.exit(0);
            });
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
