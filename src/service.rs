//! 开机自启：把 rpmm 管理器注册为自动启动的 Windows 服务。
//!
//! 与桌面程序的「登录自启动」是两套独立机制：服务由 SCM 在系统启动时拉起，
//! 不需要任何用户登录，被托管的子进程运行在会话 0 的守护账户（默认 LocalSystem）
//! 下。桌面程序登录启动后既可作为客户端连接守护进程，也可在没有守护进程时自己
//! 托管；服务是否随 rpmm 启动仍由各自的启用状态决定。
use crate::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// SCM 中登记的服务名，同时作为服务主函数入口名。
pub const NAME: &str = "rpmm";
/// 服务显示名。
pub const DISPLAY: &str = "rpmm 进程管理器";
/// 服务描述，显示在服务管理器的「描述」列。
pub const DESCRIPTION: &str =
    "rpmm 子进程管理器：系统启动时自动运行，按启用状态拉起被托管的服务，不需要用户登录。";
/// 命令行程序的服务入口参数，注册时追加在程序路径之后。
pub const ENTRY: &[&str] = &["autostart", "run"];

/// 开机自启的注册与运行状态。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Registration {
    /// 是否已在 SCM 中注册。
    pub installed: bool,
    /// 服务显示名。
    pub display_name: String,
    /// SCM 报告的当前状态：running/stopped/start-pending/stop-pending/paused/unknown。
    pub state: String,
    /// 启动类型：auto/demand/disabled/boot/system。
    pub start_type: String,
    /// 服务账户，空字符串表示 LocalSystem。
    pub account: String,
    /// 注册的服务命令行。
    pub executable: String,
    /// 服务命令行中 --root 指定的数据目录。
    pub root: Option<String>,
    /// 服务进程编号，未运行时为 None。
    pub pid: Option<u32>,
}
impl Registration {
    /// 构造未注册状态。参数：无。返回：installed 为 false 的登记信息。
    pub fn absent() -> Self {
        Self {
            installed: false,
            display_name: DISPLAY.into(),
            state: "absent".into(),
            start_type: "unknown".into(),
            account: String::new(),
            executable: String::new(),
            root: None,
            pid: None,
        }
    }
    /// 判断服务是否正在运行。参数：无。返回：状态是否为 running。
    pub fn running(&self) -> bool {
        self.state == "running"
    }
}

/// 注册开机自启所需的参数。
pub struct InstallOptions {
    /// 服务主机程序，必须是可长期存在的绝对路径。
    pub executable: PathBuf,
    /// 服务入口参数，例如命令行程序的 ["autostart", "run"]。
    pub arguments: Vec<String>,
    /// 守护进程使用的数据目录，注册时写入 --root。
    pub root: PathBuf,
    /// 服务账户；None 表示 LocalSystem。
    pub account: Option<String>,
    /// 服务账户密码；LocalSystem 时忽略。
    pub password: Option<String>,
}

/// 编码服务命令行。参数：executable 为主机程序，arguments 为入口参数，root 为数据目录。返回：Windows 命令行。
pub fn command_line(executable: &Path, arguments: &[String], root: &Path) -> String {
    let mut args = vec![executable.display().to_string()];
    args.extend(arguments.iter().cloned());
    args.push("--root".into());
    args.push(root.display().to_string());
    crate::platform::command_line(&args)
}

/// 判断数据目录是否位于当前用户配置文件中。参数：root 为数据目录。返回：需要提醒时的说明文本。
/// 服务默认以 LocalSystem 运行，用户配置目录虽然通常可读写，但会随用户配置文件删除而失效。
pub fn profile_warning(root: &Path) -> Option<String> {
    let text = root.to_string_lossy().to_lowercase();
    for key in ["userprofile", "localappdata", "appdata"] {
        let Some(value) = std::env::var_os(key) else {
            continue;
        };
        let value = value.to_string_lossy().to_lowercase();
        if !value.is_empty() && text.starts_with(&value) {
            return Some(format!(
                "数据目录位于当前用户配置文件中（{value}），守护进程以系统账户运行，建议改用 C:\\ProgramData\\rpmm 之类的机器级目录"
            ));
        }
    }
    None
}

#[cfg(windows)]
mod native {
    use super::*;
    use crate::platform::win::wide;
    use std::{
        ffi::c_void,
        sync::OnceLock,
        time::{Duration, Instant},
    };
    use tokio::sync::{oneshot, watch};
    use windows::{
        Win32::{
            Foundation::*,
            Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
            Storage::FileSystem::DELETE,
            System::{
                Services::*,
                Threading::{GetCurrentProcess, OpenProcessToken},
            },
        },
        core::{PCWSTR, PWSTR},
    };

    /// 服务控制处理器的停止通知，供控制处理器与托管循环共享。
    static STOP: OnceLock<watch::Sender<bool>> = OnceLock::new();
    /// 服务启动时确定的数据目录，服务主函数不接收自定义参数。
    static ROOT: OnceLock<PathBuf> = OnceLock::new();

    /// 可跨线程共享的服务状态句柄；SCM 只在服务生命周期内保证其有效。
    struct StatusHandle(SERVICE_STATUS_HANDLE);
    // 控制处理器由 SCM 在任意线程调用，句柄本身只用于上报状态。
    unsafe impl Send for StatusHandle {}
    unsafe impl Sync for StatusHandle {}

    /// 拥有所有权的 SCM 句柄，离开作用域时关闭。
    struct ServiceHandle(SC_HANDLE);
    impl Drop for ServiceHandle {
        /// 关闭服务或管理器句柄。参数：无。返回：无。
        fn drop(&mut self) {
            unsafe {
                let _ = CloseServiceHandle(self.0);
            }
        }
    }

    /// 判断错误是否为指定 Win32 错误码。参数：error 为 API 错误，code 为期望错误码。返回：是否匹配。
    fn code_is(error: &windows::core::Error, code: u32) -> bool {
        error.code().0 as u32 == 0x8007_0000_u32 | (code & 0xffff)
    }

    /// 打开服务控制管理器。参数：access 为所需权限。返回：管理器句柄。
    fn manager(access: u32) -> Result<ServiceHandle> {
        unsafe { OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), access) }
            .map(ServiceHandle)
            .map_err(crate::operation)
    }

    /// 打开已注册的服务。参数：manager 为管理器，access 为所需权限。返回：服务句柄。
    fn open(manager: &ServiceHandle, access: u32) -> windows::core::Result<SC_HANDLE> {
        let name = wide(NAME);
        unsafe { OpenServiceW(manager.0, PCWSTR(name.as_ptr()), access) }
    }

    /// 读取服务状态。参数：handle 为服务句柄。返回：进程信息状态。
    fn status_of(handle: &ServiceHandle) -> Result<SERVICE_STATUS_PROCESS> {
        let mut status: SERVICE_STATUS_PROCESS = unsafe { std::mem::zeroed() };
        let mut needed = 0u32;
        // QueryServiceStatusEx 需要可写字节缓冲，借用同一段内存避免额外分配。
        let buffer = unsafe {
            std::slice::from_raw_parts_mut(
                (&mut status as *mut SERVICE_STATUS_PROCESS).cast::<u8>(),
                std::mem::size_of::<SERVICE_STATUS_PROCESS>(),
            )
        };
        unsafe {
            QueryServiceStatusEx(handle.0, SC_STATUS_PROCESS_INFO, Some(buffer), &mut needed)
        }
        .map_err(crate::operation)?;
        Ok(status)
    }

    /// 转换服务状态为稳定文本。参数：state 为 SCM 状态码。返回：kebab-case 文本。
    fn state_text(state: SERVICE_STATUS_CURRENT_STATE) -> String {
        match state {
            SERVICE_RUNNING => "running",
            SERVICE_START_PENDING => "start-pending",
            SERVICE_STOP_PENDING => "stop-pending",
            SERVICE_PAUSED => "paused",
            SERVICE_CONTINUE_PENDING => "continue-pending",
            SERVICE_PAUSE_PENDING => "pause-pending",
            SERVICE_STOPPED => "stopped",
            _ => "unknown",
        }
        .into()
    }

    /// 转换启动类型为稳定文本。参数：start_type 为 SCM 启动类型。返回：稳定文本。
    fn start_text(start_type: SERVICE_START_TYPE) -> String {
        if start_type == SERVICE_AUTO_START {
            "auto"
        } else if start_type == SERVICE_DEMAND_START {
            "demand"
        } else if start_type == SERVICE_DISABLED {
            "disabled"
        } else if start_type == SERVICE_BOOT_START {
            "boot"
        } else if start_type == SERVICE_SYSTEM_START {
            "system"
        } else {
            "unknown"
        }
        .into()
    }

    /// 读取服务配置中的命令行与账户。参数：handle 为服务句柄。返回：命令行、账户与启动类型。
    fn config_of(handle: &ServiceHandle) -> Result<(String, String, String)> {
        let mut needed = 0u32;
        let _ = unsafe { QueryServiceConfigW(handle.0, None, 0, &mut needed) };
        if needed == 0 {
            return Err(crate::Error::Operation("服务未返回配置".into()));
        }
        // 缓冲按指针宽度对齐，配置结构内含 PWSTR 字段。
        let mut buffer = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        unsafe {
            QueryServiceConfigW(
                handle.0,
                Some(buffer.as_mut_ptr().cast()),
                needed,
                &mut needed,
            )
        }
        .map_err(crate::operation)?;
        let config = unsafe { &*(buffer.as_ptr() as *const QUERY_SERVICE_CONFIGW) };
        let (executable, account) = unsafe {
            (
                config.lpBinaryPathName.to_string().unwrap_or_default(),
                config.lpServiceStartName.to_string().unwrap_or_default(),
            )
        };
        Ok((executable, account, start_text(config.dwStartType)))
    }

    /// 判断当前进程是否已提升为管理员。参数：无。返回：是否已提升。
    pub fn is_elevated() -> Result<bool> {
        let mut raw = HANDLE::default();
        unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) }
            .map_err(crate::operation)?;
        let token = crate::platform::win::Handle(raw);
        let mut elevation = TOKEN_ELEVATION::default();
        let mut size = 0u32;
        unsafe {
            GetTokenInformation(
                token.0,
                TokenElevation,
                Some((&mut elevation as *mut TOKEN_ELEVATION).cast()),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut size,
            )
        }
        .map_err(crate::operation)?;
        Ok(elevation.TokenIsElevated != 0)
    }

    /// 校验管理员权限。参数：无。返回：已提升时成功，否则返回明确的错误。
    pub fn require_elevation() -> Result<()> {
        if is_elevated()? {
            Ok(())
        } else {
            Err(crate::Error::Operation(
                "注册开机自启需要管理员权限，请以管理员身份运行".into(),
            ))
        }
    }

    /// 查询开机自启服务的注册与运行状态。参数：无。返回：未注册时返回 absent 状态。
    pub fn registration() -> Result<Registration> {
        let manager = manager(SC_MANAGER_CONNECT)?;
        let handle = match open(&manager, SERVICE_QUERY_STATUS | SERVICE_QUERY_CONFIG) {
            Ok(handle) => ServiceHandle(handle),
            Err(error) if code_is(&error, ERROR_SERVICE_DOES_NOT_EXIST.0) => {
                return Ok(Registration::absent());
            }
            Err(error) => return Err(crate::operation(error)),
        };
        let status = status_of(&handle)?;
        let (executable, account, start_type) = config_of(&handle)?;
        Ok(Registration {
            installed: true,
            display_name: DISPLAY.into(),
            state: state_text(status.dwCurrentState),
            start_type,
            account,
            root: root_from_command_line(&executable).map(|path| path.display().to_string()),
            executable,
            pid: (status.dwCurrentState == SERVICE_RUNNING && status.dwProcessId != 0)
                .then_some(status.dwProcessId),
        })
    }

    /// 解析服务命令行中的数据目录。参数：text 为完整命令行。返回：--root 指向的目录。
    pub fn root_from_command_line(text: &str) -> Option<PathBuf> {
        let args = crate::platform::win::decode_command_line(text).ok()?;
        let index = args.iter().position(|arg| arg == "--root")?;
        args.get(index + 1).map(PathBuf::from)
    }

    /// 写入服务描述。参数：handle 为已打开的服务。返回：写入结果。
    fn describe(handle: &ServiceHandle) -> Result<()> {
        let mut text = wide(DESCRIPTION);
        let description = SERVICE_DESCRIPTIONW {
            lpDescription: PWSTR(text.as_mut_ptr()),
        };
        unsafe {
            ChangeServiceConfig2W(
                handle.0,
                SERVICE_CONFIG_DESCRIPTION,
                Some((&description as *const SERVICE_DESCRIPTIONW).cast()),
            )
        }
        .map_err(crate::operation)
    }

    /// 设置失败恢复策略。参数：handle 为已打开的服务。返回：写入结果。
    /// 前三次失败分别在 5、10、30 秒后重启，一天后重置计数；非零退出同样触发恢复。
    fn failures(handle: &ServiceHandle) -> Result<()> {
        let mut actions = [
            SC_ACTION {
                Type: SC_ACTION_RESTART,
                Delay: 5_000,
            },
            SC_ACTION {
                Type: SC_ACTION_RESTART,
                Delay: 10_000,
            },
            SC_ACTION {
                Type: SC_ACTION_RESTART,
                Delay: 30_000,
            },
        ];
        let failures = SERVICE_FAILURE_ACTIONSW {
            dwResetPeriod: 86_400,
            lpRebootMsg: PWSTR::null(),
            lpCommand: PWSTR::null(),
            cActions: actions.len() as u32,
            lpsaActions: actions.as_mut_ptr(),
        };
        unsafe {
            ChangeServiceConfig2W(
                handle.0,
                SERVICE_CONFIG_FAILURE_ACTIONS,
                Some((&failures as *const SERVICE_FAILURE_ACTIONSW).cast()),
            )
        }
        .map_err(crate::operation)?;
        let flag = SERVICE_FAILURE_ACTIONS_FLAG {
            fFailureActionsOnNonCrashFailures: TRUE,
        };
        unsafe {
            ChangeServiceConfig2W(
                handle.0,
                SERVICE_CONFIG_FAILURE_ACTIONS_FLAG,
                Some((&flag as *const SERVICE_FAILURE_ACTIONS_FLAG).cast()),
            )
        }
        .map_err(crate::operation)
    }

    /// 注册或更新开机自启服务。参数：options 为主机程序、入口参数与账户。返回：注册结果。
    pub fn install(options: &InstallOptions) -> Result<()> {
        require_elevation()?;
        if !options.executable.is_absolute() {
            return Err(crate::Error::Config("服务程序必须为绝对路径".into()));
        }
        if !options.executable.is_file() {
            return Err(crate::Error::Config(format!(
                "服务程序不存在：{}",
                options.executable.display()
            )));
        }
        let command = wide(&command_line(
            &options.executable,
            &options.arguments,
            &options.root,
        ));
        let account = options.account.as_deref().map(wide);
        let password = options.password.as_deref().map(wide);
        let name = wide(NAME);
        let display = wide(DISPLAY);
        // CreateService 用 NULL 表示 LocalSystem；ChangeServiceConfig 用空串显式重置账户。
        let create_account = account
            .as_ref()
            .map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr()));
        let create_password = password
            .as_ref()
            .map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr()));
        let system_account = wide("");
        let change_account = account
            .as_ref()
            .map_or(PCWSTR(system_account.as_ptr()), |v| PCWSTR(v.as_ptr()));
        let change_password = password
            .as_ref()
            .map_or(PCWSTR(system_account.as_ptr()), |v| PCWSTR(v.as_ptr()));
        let manager = manager(SC_MANAGER_CREATE_SERVICE | SC_MANAGER_CONNECT)?;
        let handle = match open(&manager, SERVICE_ALL_ACCESS) {
            Ok(handle) => {
                // 已注册时按当前参数更新，保持同一服务名，便于重复调用与界面开关。
                unsafe {
                    ChangeServiceConfigW(
                        handle,
                        SERVICE_WIN32_OWN_PROCESS,
                        SERVICE_AUTO_START,
                        SERVICE_ERROR_NORMAL,
                        PCWSTR(command.as_ptr()),
                        PCWSTR::null(),
                        None,
                        PCWSTR::null(),
                        change_account,
                        change_password,
                        PCWSTR(display.as_ptr()),
                    )
                }
                .map_err(crate::operation)?;
                ServiceHandle(handle)
            }
            Err(error) if code_is(&error, ERROR_SERVICE_DOES_NOT_EXIST.0) => ServiceHandle(
                unsafe {
                    CreateServiceW(
                        manager.0,
                        PCWSTR(name.as_ptr()),
                        PCWSTR(display.as_ptr()),
                        SERVICE_ALL_ACCESS,
                        SERVICE_WIN32_OWN_PROCESS,
                        SERVICE_AUTO_START,
                        SERVICE_ERROR_NORMAL,
                        PCWSTR(command.as_ptr()),
                        PCWSTR::null(),
                        None,
                        PCWSTR::null(),
                        create_account,
                        create_password,
                    )
                }
                .map_err(crate::operation)?,
            ),
            Err(error) => return Err(crate::operation(error)),
        };
        describe(&handle)?;
        failures(&handle)
    }

    /// 等待服务进入停止状态。参数：handle 为服务句柄，limit 为最长等待。返回：是否已停止。
    fn wait_stopped(handle: &ServiceHandle, limit: Duration) -> bool {
        let deadline = Instant::now() + limit;
        loop {
            match status_of(handle) {
                Ok(status) if status.dwCurrentState == SERVICE_STOPPED => return true,
                Ok(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(200));
                }
                _ => return false,
            }
        }
    }

    /// 停止服务句柄对应的服务。参数：handle 为服务句柄。返回：结果；未运行时视为成功。
    fn stop_handle(handle: &ServiceHandle) -> Result<()> {
        let mut status: SERVICE_STATUS = unsafe { std::mem::zeroed() };
        match unsafe { ControlService(handle.0, SERVICE_CONTROL_STOP, &mut status) } {
            Ok(()) => Ok(()),
            Err(error) if code_is(&error, ERROR_SERVICE_NOT_ACTIVE.0) => Ok(()),
            Err(error) => Err(crate::operation(error)),
        }
    }

    /// 停止并删除开机自启服务。参数：无。返回：结果。
    pub fn uninstall() -> Result<()> {
        require_elevation()?;
        let manager = manager(SC_MANAGER_CONNECT)?;
        let handle = match open(&manager, SERVICE_STOP | SERVICE_QUERY_STATUS | DELETE.0) {
            Ok(handle) => ServiceHandle(handle),
            Err(error) if code_is(&error, ERROR_SERVICE_DOES_NOT_EXIST.0) => {
                return Err(crate::Error::Operation("开机自启尚未注册".into()));
            }
            Err(error) => return Err(crate::operation(error)),
        };
        // 先请求停止并等待子进程被有序回收，再删除登记。
        stop_handle(&handle)?;
        wait_stopped(&handle, Duration::from_secs(30));
        unsafe { DeleteService(handle.0) }.map_err(crate::operation)
    }

    /// 启动开机自启服务。参数：无。返回：结果；已在运行时视为成功。
    pub fn start() -> Result<()> {
        require_elevation()?;
        let manager = manager(SC_MANAGER_CONNECT)?;
        let handle = match open(&manager, SERVICE_START | SERVICE_QUERY_STATUS) {
            Ok(handle) => ServiceHandle(handle),
            Err(error) if code_is(&error, ERROR_SERVICE_DOES_NOT_EXIST.0) => {
                return Err(crate::Error::Operation("开机自启尚未注册".into()));
            }
            Err(error) => return Err(crate::operation(error)),
        };
        match unsafe { StartServiceW(handle.0, None) } {
            Ok(()) => Ok(()),
            Err(error) if code_is(&error, ERROR_SERVICE_ALREADY_RUNNING.0) => Ok(()),
            Err(error) => Err(crate::operation(error)),
        }
    }

    /// 停止正在运行的开机自启服务。参数：无。返回：结果；未运行时视为成功。
    pub fn stop() -> Result<()> {
        require_elevation()?;
        let manager = manager(SC_MANAGER_CONNECT)?;
        let handle = match open(&manager, SERVICE_STOP | SERVICE_QUERY_STATUS) {
            Ok(handle) => ServiceHandle(handle),
            Err(error) if code_is(&error, ERROR_SERVICE_DOES_NOT_EXIST.0) => {
                return Err(crate::Error::Operation("开机自启尚未注册".into()));
            }
            Err(error) => return Err(crate::operation(error)),
        };
        stop_handle(&handle)
    }

    /// 上报服务状态。参数：handle 为状态句柄，state 为状态，accepted 为接受的控制码，exit 为退出码，hint 为等待提示毫秒。返回：上报结果。
    fn report(
        handle: SERVICE_STATUS_HANDLE,
        state: SERVICE_STATUS_CURRENT_STATE,
        accepted: u32,
        exit: u32,
        hint: u32,
    ) -> Result<()> {
        let status = SERVICE_STATUS {
            dwServiceType: SERVICE_WIN32_OWN_PROCESS,
            dwCurrentState: state,
            dwControlsAccepted: accepted,
            dwWin32ExitCode: exit,
            dwServiceSpecificExitCode: 0,
            dwCheckPoint: 0,
            dwWaitHint: hint,
        };
        unsafe { SetServiceStatus(handle, &status) }.map_err(crate::operation)
    }

    /// 处理 SCM 控制码。参数：control 为控制码，其余由 SCM 提供。返回：处理结果码。
    unsafe extern "system" fn handler(
        control: u32,
        _event: u32,
        _data: *mut c_void,
        _context: *mut c_void,
    ) -> u32 {
        if matches!(
            control,
            SERVICE_CONTROL_STOP | SERVICE_CONTROL_SHUTDOWN | SERVICE_CONTROL_PRESHUTDOWN
        ) {
            // 先上报停止中并给出等待提示，避免 SCM 在子进程回收完成前强制结束服务。
            if let Some(handle) = STATUS.get() {
                let _ = report(handle.0, SERVICE_STOP_PENDING, 0, 0, 30_000);
            }
            if let Some(stop) = STOP.get() {
                let _ = stop.send(true);
            }
        }
        0
    }

    /// 托管子进程并上报运行状态。参数：handle 为状态句柄，receiver 为停止通知。返回：托管结果。
    fn supervise(handle: SERVICE_STATUS_HANDLE, receiver: watch::Receiver<bool>) -> Result<()> {
        report(handle, SERVICE_START_PENDING, 0, 0, 5_000)?;
        let root = ROOT
            .get()
            .cloned()
            .ok_or_else(|| crate::Error::Operation("服务缺少数据目录".into()))?;
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(async move {
            let manager = crate::manager::Manager::new(&root)?;
            let (ready, readiness) = oneshot::channel();
            let server = tokio::spawn(crate::ipc::serve(manager.clone(), receiver, ready));
            readiness.await.map_err(crate::operation)??;
            // 管道就绪后才接受停止与关机控制，避免停机信号早于管理器建立而丢失。
            report(
                handle,
                SERVICE_RUNNING,
                SERVICE_ACCEPT_STOP | SERVICE_ACCEPT_SHUTDOWN | SERVICE_ACCEPT_PRESHUTDOWN,
                0,
                0,
            )?;
            // serve 在关闭前会按依赖逆序停止全部子进程。
            server.await.map_err(crate::operation)?
        })
    }

    /// SCM 服务主函数。参数：argc/argv 由 SCM 提供且仅含服务名。返回：无。
    unsafe extern "system" fn service_main(_argc: u32, _argv: *mut PWSTR) {
        // 通道与数据目录都在 StartServiceCtrlDispatcherW 之前建立，缺失时无法恢复运行。
        let Some(receiver) = STOP_RECEIVER.get().cloned() else {
            return;
        };
        let name = wide(NAME);
        let handle = match unsafe {
            RegisterServiceCtrlHandlerExW(PCWSTR(name.as_ptr()), Some(handler), None)
        } {
            Ok(handle) => handle,
            // 无法注册控制处理器时 SCM 会把服务判定为启动失败。
            Err(_) => return,
        };
        let _ = STATUS.set(StatusHandle(handle));
        let code = match supervise(handle, receiver) {
            Ok(()) => 0,
            Err(_) => 1,
        };
        // 非零退出码会触发失败恢复策略重启服务。
        let _ = report(handle, SERVICE_STOPPED, 0, code, 0);
    }

    /// 停止通知的接收端；在注册控制处理器之前建立，避免早到的控制码丢失。
    static STOP_RECEIVER: OnceLock<watch::Receiver<bool>> = OnceLock::new();
    /// 服务状态句柄，控制处理器用它上报停止进度。
    static STATUS: OnceLock<StatusHandle> = OnceLock::new();

    /// 以管理员身份运行本程序的开机自启子命令。参数：executable 为程序，arguments 为完整参数。返回：被提升进程的退出码。
    /// 用户拒绝提升时返回明确错误；被提升进程的诊断无法回传，调用方应以重新查询的注册状态为准。
    pub fn elevate(executable: &Path, arguments: &[String]) -> Result<u32> {
        use windows::Win32::{
            System::Threading::{GetExitCodeProcess, WaitForSingleObject},
            UI::Shell::{
                SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
            },
        };
        let file = wide(&executable.display().to_string());
        let parameters = wide(&crate::platform::command_line(arguments));
        let verb = wide("runas");
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC,
            lpVerb: PCWSTR(verb.as_ptr()),
            lpFile: PCWSTR(file.as_ptr()),
            lpParameters: PCWSTR(parameters.as_ptr()),
            // SW_SHOWNORMAL；被提升的程序自行决定是否显示窗口。
            nShow: 1,
            ..Default::default()
        };
        unsafe { ShellExecuteExW(&mut info) }.map_err(|error| {
            if code_is(&error, ERROR_CANCELLED.0) {
                crate::Error::Operation("已取消管理员授权，开机自启未变更".into())
            } else {
                crate::operation(error)
            }
        })?;
        // hProcess 由本进程拥有，等待提升进程结束后读取退出码。
        let process = crate::platform::win::Handle(info.hProcess);
        unsafe { WaitForSingleObject(process.0, 300_000) };
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(process.0, &mut code) }.map_err(crate::operation)?;
        Ok(code)
    }

    /// 运行服务入口，由 SCM 调用。参数：root 为守护进程使用的数据目录。返回：服务结束后的结果。
    pub fn run(root: &Path) -> Result<()> {
        let (stop, receiver) = watch::channel(false);
        let _ = STOP.set(stop);
        let _ = STOP_RECEIVER.set(receiver);
        ROOT.set(root.to_path_buf())
            .map_err(|_| crate::Error::Operation("服务入口只能初始化一次".into()))?;
        let name = wide(NAME);
        let table = [
            SERVICE_TABLE_ENTRYW {
                lpServiceName: PWSTR(name.as_ptr() as *mut u16),
                lpServiceProc: Some(service_main),
            },
            SERVICE_TABLE_ENTRYW {
                lpServiceName: PWSTR::null(),
                lpServiceProc: None,
            },
        ];
        unsafe { StartServiceCtrlDispatcherW(table.as_ptr()) }.map_err(|error| {
            if code_is(&error, ERROR_FAILED_SERVICE_CONTROLLER_CONNECT.0) {
                crate::Error::Operation(
                    "该入口只能由服务控制管理器启动，请改用 rpmm autostart install 注册后启动服务"
                        .into(),
                )
            } else {
                crate::operation(error)
            }
        })
    }
}

#[cfg(not(windows))]
mod native {
    use super::*;
    /// 判断当前进程是否已提升为管理员。参数：无。返回：仅支持 Windows 的错误。
    pub fn is_elevated() -> Result<bool> {
        Err(crate::Error::Operation("开机自启仅支持 Windows".into()))
    }
    /// 校验管理员权限。参数：无。返回：仅支持 Windows 的错误。
    pub fn require_elevation() -> Result<()> {
        Err(crate::Error::Operation("开机自启仅支持 Windows".into()))
    }
    /// 查询开机自启状态。参数：无。返回：仅支持 Windows 的错误。
    pub fn registration() -> Result<Registration> {
        Err(crate::Error::Operation("开机自启仅支持 Windows".into()))
    }
    /// 解析服务命令行中的数据目录。参数：text 为命令行。返回：始终为空。
    pub fn root_from_command_line(_text: &str) -> Option<PathBuf> {
        None
    }
    /// 注册开机自启服务。参数：options 为注册参数。返回：仅支持 Windows 的错误。
    pub fn install(_options: &InstallOptions) -> Result<()> {
        Err(crate::Error::Operation("开机自启仅支持 Windows".into()))
    }
    /// 删除开机自启服务。参数：无。返回：仅支持 Windows 的错误。
    pub fn uninstall() -> Result<()> {
        Err(crate::Error::Operation("开机自启仅支持 Windows".into()))
    }
    /// 启动开机自启服务。参数：无。返回：仅支持 Windows 的错误。
    pub fn start() -> Result<()> {
        Err(crate::Error::Operation("开机自启仅支持 Windows".into()))
    }
    /// 停止开机自启服务。参数：无。返回：仅支持 Windows 的错误。
    pub fn stop() -> Result<()> {
        Err(crate::Error::Operation("开机自启仅支持 Windows".into()))
    }
    /// 以管理员身份运行子命令。参数：executable 为程序，arguments 为参数。返回：仅支持 Windows 的错误。
    pub fn elevate(_executable: &Path, _arguments: &[String]) -> Result<u32> {
        Err(crate::Error::Operation("开机自启仅支持 Windows".into()))
    }
    /// 运行服务入口。参数：root 为数据目录。返回：仅支持 Windows 的错误。
    pub fn run(_root: &Path) -> Result<()> {
        Err(crate::Error::Operation("开机自启仅支持 Windows".into()))
    }
}

pub use native::*;
