//! Windows 服务安装、账户权限与控制。
#[cfg(windows)]
pub mod security {
    use crate::{
        Error, Result,
        platform::win::{Handle, wide},
    };
    use std::path::{Path, PathBuf};
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
    use windows::{
        Win32::{
            Foundation::*,
            Security::{Authentication::Identity::*, Authorization::*, *},
            System::Threading::*,
        },
        core::{PCWSTR, PWSTR},
    };

    pub struct Descriptor(pub PSECURITY_DESCRIPTOR);
    impl Drop for Descriptor {
        /// 释放系统分配的描述符。参数：无。返回：无。
        fn drop(&mut self) {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0.0)));
            }
        }
    }
    /// 解析 SDDL。参数：sddl 为安全描述符文本。返回：拥有所有权的描述符。
    fn descriptor(sddl: &str) -> Result<Descriptor> {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(wide(sddl).as_ptr()),
                SDDL_REVISION_1,
                &mut descriptor,
                None,
            )
            .map_err(crate::operation)?;
        }
        Ok(Descriptor(descriptor))
    }
    /// SID 转为字符串。参数：sid 为有效 SID 指针。返回：SID 文本。
    fn sid_string(sid: PSID) -> Result<String> {
        unsafe {
            let mut text = PWSTR::null();
            ConvertSidToStringSidW(sid, &mut text).map_err(crate::operation)?;
            let value = text.to_string().map_err(crate::operation);
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            value
        }
    }
    /// 查询当前进程账户 SID。参数：无。返回：SID 文本。
    pub fn current_sid() -> Result<String> {
        unsafe {
            let mut raw = HANDLE::default();
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw)
                .map_err(crate::operation)?;
            let token = Handle(raw);
            let mut size = 0;
            let _ = GetTokenInformation(token.0, TokenUser, None, 0, &mut size);
            let mut data = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
            GetTokenInformation(
                token.0,
                TokenUser,
                Some(data.as_mut_ptr().cast()),
                size,
                &mut size,
            )
            .map_err(crate::operation)?;
            sid_string((*(data.as_ptr() as *const TOKEN_USER)).User.Sid)
        }
    }
    /// 创建有显式 DACL 的仅本机管道。参数：name 为名称，first 为首实例标记。返回：异步服务端。
    pub fn pipe(name: &str, first: bool) -> Result<NamedPipeServer> {
        let sid = current_sid()?;
        let descriptor = descriptor(&format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{sid})"))?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0.0,
            bInheritHandle: false.into(),
        };
        unsafe {
            ServerOptions::new()
                .first_pipe_instance(first)
                .reject_remote_clients(true)
                .create_with_security_attributes_raw(name, &attributes as *const _ as *mut _)
                .map_err(Into::into)
        }
    }

    pub struct Account {
        data: Vec<usize>,
        pub sid: String,
    }
    impl Account {
        /// 获取 SID 指针。参数：无。返回：有效至对象销毁的指针。
        fn pointer(&self) -> PSID {
            PSID(self.data.as_ptr() as *mut _)
        }
    }
    /// 解析账户并拒绝不适用于首版的内置账户。参数：name 为 Windows 账户。返回：SID 所有者。
    pub fn account(name: &str) -> Result<Account> {
        if name.to_uppercase().starts_with("NT AUTHORITY\\")
            || name.eq_ignore_ascii_case("LocalSystem")
            || name.starts_with("NT SERVICE\\")
        {
            return Err(Error::Operation(
                "首版要求指定带密码的普通 Windows 账户".into(),
            ));
        }
        unsafe {
            let name = wide(name);
            let mut size = 0;
            let mut domain_size = 0;
            let mut kind = SID_NAME_USE::default();
            let _ = LookupAccountNameW(
                PCWSTR::null(),
                PCWSTR(name.as_ptr()),
                None,
                &mut size,
                None,
                &mut domain_size,
                &mut kind,
            );
            if size == 0 {
                return Err(crate::operation(std::io::Error::last_os_error()));
            }
            let mut data = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
            let mut domain = vec![0u16; domain_size as usize];
            let sid = PSID(data.as_mut_ptr().cast());
            LookupAccountNameW(
                PCWSTR::null(),
                PCWSTR(name.as_ptr()),
                Some(sid),
                &mut size,
                Some(PWSTR(domain.as_mut_ptr())),
                &mut domain_size,
                &mut kind,
            )
            .map_err(crate::operation)?;
            if kind != SidTypeUser {
                return Err(Error::Operation("安装账户必须是用户，不能是组".into()));
            }
            Ok(Account {
                sid: sid_string(sid)?,
                data,
            })
        }
    }
    struct Policy(LSA_HANDLE);
    impl Drop for Policy {
        /// 关闭 LSA 策略句柄。参数：无。返回：无。
        fn drop(&mut self) {
            unsafe {
                let _ = LsaClose(self.0);
            }
        }
    }
    /// 转換 LSA 状态。参数：status 为 NTSTATUS。返回：统一结果。
    fn lsa_result(status: NTSTATUS) -> Result<()> {
        if status.0 == 0 {
            Ok(())
        } else {
            Err(
                std::io::Error::from_raw_os_error(unsafe { LsaNtStatusToWinError(status) } as i32)
                    .into(),
            )
        }
    }
    /// 打开本机账户权限策略。参数：无。返回：策略句柄。
    fn policy() -> Result<Policy> {
        let attributes = LSA_OBJECT_ATTRIBUTES {
            Length: std::mem::size_of::<LSA_OBJECT_ATTRIBUTES>() as u32,
            ..Default::default()
        };
        let mut handle = LSA_HANDLE::default();
        unsafe {
            lsa_result(LsaOpenPolicy(
                None,
                &attributes,
                (POLICY_LOOKUP_NAMES | POLICY_CREATE_ACCOUNT) as u32,
                &mut handle,
            ))?;
        }
        Ok(Policy(handle))
    }
    /// 构造 LSA Unicode 字符串。参数：buffer 为 NUL 结尾 UTF-16。返回：借用结构。
    fn lsa_string(buffer: &mut [u16]) -> LSA_UNICODE_STRING {
        LSA_UNICODE_STRING {
            Length: ((buffer.len() - 1) * 2) as u16,
            MaximumLength: (buffer.len() * 2) as u16,
            Buffer: PWSTR(buffer.as_mut_ptr()),
        }
    }
    /// 确保服务登录权限。参数：account 为账户。返回：是否由本次新增。
    pub fn grant_logon(account: &Account) -> Result<bool> {
        let policy = policy()?;
        unsafe {
            let mut rights = std::ptr::null_mut();
            let mut count = 0;
            let status =
                LsaEnumerateAccountRights(policy.0, account.pointer(), &mut rights, &mut count);
            let mut existing = false;
            if status.0 == 0 {
                for right in if count == 0 {
                    &[]
                } else {
                    std::slice::from_raw_parts(rights, count as usize)
                } {
                    let text = String::from_utf16_lossy(std::slice::from_raw_parts(
                        right.Buffer.0,
                        right.Length as usize / 2,
                    ));
                    existing |= text == "SeServiceLogonRight";
                }
                let _ = LsaFreeMemory(Some(rights.cast()));
            } else if LsaNtStatusToWinError(status) != ERROR_FILE_NOT_FOUND.0 {
                lsa_result(status)?;
            }
            if existing {
                return Ok(false);
            }
            let mut buffer = wide("SeServiceLogonRight");
            lsa_result(LsaAddAccountRights(
                policy.0,
                account.pointer(),
                &[lsa_string(&mut buffer)],
            ))?;
            Ok(true)
        }
    }
    /// 移除本项目新增的登录权限。参数：account 为账户。返回：结果。
    pub fn revoke_logon(account: &Account) -> Result<()> {
        let policy = policy()?;
        let mut buffer = wide("SeServiceLogonRight");
        unsafe {
            lsa_result(LsaRemoveAccountRights(
                policy.0,
                account.pointer(),
                false,
                Some(&[lsa_string(&mut buffer)]),
            ))
        }
    }
    /// 验证服务登录凭据。参数：name/password 为账户和内存密码。返回：验证结果。
    pub fn validate_password(name: &str, password: &str) -> Result<()> {
        let (domain, user) = name
            .split_once('\\')
            .map(|(d, u)| (Some(d), u))
            .unwrap_or((None, name));
        let user = wide(user);
        let domain = domain.map(wide);
        let password = wide(password);
        let mut token = HANDLE::default();
        unsafe {
            LogonUserW(
                PCWSTR(user.as_ptr()),
                domain
                    .as_ref()
                    .map(|d| PCWSTR(d.as_ptr()))
                    .unwrap_or(PCWSTR::null()),
                PCWSTR(password.as_ptr()),
                LOGON32_LOGON_SERVICE,
                LOGON32_PROVIDER_DEFAULT,
                &mut token,
            )
            .map_err(crate::operation)?;
        }
        drop(Handle(token));
        Ok(())
    }

    #[derive(Clone)]
    pub struct AclBackup {
        path: PathBuf,
        sddl: String,
    }
    /// 读取文件 DACL 为文本。参数：path 为现有对象。返回：描述符文本。
    fn read_acl(path: &Path) -> Result<String> {
        unsafe {
            let mut size = 0;
            let path = wide(&path.to_string_lossy());
            let _ = GetFileSecurityW(
                PCWSTR(path.as_ptr()),
                DACL_SECURITY_INFORMATION.0,
                None,
                0,
                &mut size,
            );
            let mut buffer = vec![0usize; (size as usize).div_ceil(std::mem::size_of::<usize>())];
            let descriptor = PSECURITY_DESCRIPTOR(buffer.as_mut_ptr().cast());
            GetFileSecurityW(
                PCWSTR(path.as_ptr()),
                DACL_SECURITY_INFORMATION.0,
                Some(descriptor),
                size,
                &mut size,
            )
            .ok()
            .map_err(crate::operation)?;
            let mut text = PWSTR::null();
            ConvertSecurityDescriptorToStringSecurityDescriptorW(
                descriptor,
                SDDL_REVISION_1,
                DACL_SECURITY_INFORMATION,
                &mut text,
                None,
            )
            .map_err(crate::operation)?;
            let result = text.to_string().map_err(crate::operation);
            let _ = LocalFree(Some(HLOCAL(text.0.cast())));
            result
        }
    }
    /// 应用 DACL 并保持保护属性。参数：path/sddl 为对象及描述符。返回：结果。
    fn write_acl(path: &Path, sddl: &str) -> Result<()> {
        let descriptor = descriptor(sddl)?;
        let protection = if sddl.starts_with("D:P") {
            PROTECTED_DACL_SECURITY_INFORMATION
        } else {
            UNPROTECTED_DACL_SECURITY_INFORMATION
        };
        unsafe {
            SetFileSecurityW(
                PCWSTR(wide(&path.to_string_lossy()).as_ptr()),
                DACL_SECURITY_INFORMATION | protection,
                descriptor.0,
            )
            .ok()
            .map_err(crate::operation)
        }
    }
    /// 递归收集项目目录；拒绝链接，权限变更不得越过 root。
    /// 参数：root 为目录，paths 为输出列表。返回：结果。
    fn collect(root: &Path, paths: &mut Vec<PathBuf>) -> Result<()> {
        if std::fs::symlink_metadata(root)?.file_type().is_symlink() {
            return Err(Error::Operation("项目数据目录不能包含符号链接".into()));
        }
        paths.push(root.into());
        if root.is_dir() {
            for entry in std::fs::read_dir(root)? {
                collect(&entry?.path(), paths)?;
            }
        }
        Ok(())
    }
    /// 捕获所有项目对象的 DACL，供安装失败恢复。参数：root 为项目目录。返回：备份。
    pub fn backup(root: &Path) -> Result<Vec<AclBackup>> {
        let mut paths = vec![];
        collect(root, &mut paths)?;
        paths
            .into_iter()
            .map(|path| {
                Ok(AclBackup {
                    sddl: read_acl(&path)?,
                    path,
                })
            })
            .collect()
    }
    /// 应用服务账户权限。参数：root、sid、backups 为目录、账户 SID 和已收集对象。返回：结果。
    pub fn grant_directories(root: &Path, sid: &str, backups: &[AclBackup]) -> Result<()> {
        for backup in backups {
            let relative = backup.path.strip_prefix(root).map_err(crate::operation)?;
            let writable = relative.starts_with("state") || relative.starts_with("logs");
            let access = if writable { "0x1301bf" } else { "GRGX" };
            let inherit = if backup.path.is_dir() { "OICI" } else { "" };
            write_acl(
                &backup.path,
                &format!(
                    "D:P(A;{inherit};FA;;;SY)(A;{inherit};FA;;;BA)(A;{inherit};{access};;;{sid})"
                ),
            )?;
        }
        Ok(())
    }
    /// 恢复安装前权限。参数：backups 为原始 DACL。返回：结果。
    pub fn restore(backups: &[AclBackup]) -> Result<()> {
        for backup in backups {
            write_acl(&backup.path, &backup.sddl)?;
        }
        Ok(())
    }
}

#[cfg(windows)]
mod implementation {
    use super::security;
    use crate::{
        Error, Result,
        manager::{Manager, atomic_write},
    };
    use serde::{Deserialize, Serialize};
    use std::{
        ffi::OsString,
        path::{Path, PathBuf},
        sync::OnceLock,
        time::{Duration, Instant},
    };
    use tokio::sync::{oneshot, watch};
    use windows_service::{
        define_windows_service,
        service::{
            Service, ServiceAccess, ServiceAction, ServiceActionType, ServiceControl,
            ServiceControlAccept, ServiceErrorControl, ServiceExitCode, ServiceFailureActions,
            ServiceFailureResetPeriod, ServiceInfo, ServiceStartType, ServiceState, ServiceStatus,
            ServiceType,
        },
        service_control_handler::{self, ServiceControlHandlerResult},
        service_dispatcher,
        service_manager::{ServiceManager, ServiceManagerAccess},
    };
    const NAME: &str = "rpmm";
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    #[derive(Serialize, Deserialize)]
    struct Installation {
        account: String,
        sid: String,
        granted_logon: bool,
    }

    /// 打开 SCM。参数：access 为所需权限。返回：句柄。
    fn scm(access: ServiceManagerAccess) -> Result<ServiceManager> {
        ServiceManager::local_computer(None::<&str>, access).map_err(crate::operation)
    }
    /// 打开本项目服务。参数：access 为权限。返回：服务句柄。
    fn service(access: ServiceAccess) -> Result<Service> {
        scm(ServiceManagerAccess::CONNECT)?
            .open_service(NAME, access)
            .map_err(crate::operation)
    }
    /// 验证操作 root 与已安装服务一致，避免误操作其他目录的服务。
    /// 参数：root/service 为目录和句柄。返回：结果。
    fn check_root(root: &Path, service: &Service) -> Result<()> {
        let config = service.query_config().map_err(crate::operation)?;
        let args =
            crate::platform::win::decode_command_line(&config.executable_path.to_string_lossy())?;
        let root_text = root.to_string_lossy();
        if args.len() != 4
            || args[1] != "--root"
            || !args[2].eq_ignore_ascii_case(&root_text)
            || args[3] != "service-host"
        {
            return Err(Error::Operation(
                "已安装的 rpmm 服务属于另一数据目录".into(),
            ));
        }
        Ok(())
    }
    /// 安装服务并事务化授予账户权限。参数：root、account_name、password 为目录及凭据。返回：结果。
    pub fn install(root: &Path, account_name: &str, password: &str) -> Result<()> {
        let scm = scm(ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE)?;
        match scm.open_service(NAME, ServiceAccess::QUERY_STATUS) {
            Ok(_) => return Err(Error::Operation("rpmm 服务已安装".into())),
            Err(windows_service::Error::Winapi(e)) if e.raw_os_error() == Some(1060) => (),
            Err(e) => return Err(crate::operation(e)),
        }
        let account = security::account(account_name)?;
        for folder in ["units", "state", "logs"] {
            std::fs::create_dir_all(root.join(folder))?;
        }
        if root.join("installation.json").exists() {
            return Err(Error::Operation(
                "发现旧安装记录，请先确认原服务已卸载并处理记录".into(),
            ));
        }
        let backups = security::backup(root)?;
        let granted = security::grant_logon(&account)?;
        let mut created: Option<Service> = None;
        let receipt = root.join("installation.json");
        let result: Result<()> = (|| {
            security::validate_password(account_name, password)?;
            security::grant_directories(root, &account.sid, &backups)?;
            let info = ServiceInfo {
                name: NAME.into(),
                display_name: "rpmm 程序托管器".into(),
                service_type: ServiceType::OWN_PROCESS,
                start_type: ServiceStartType::AutoStart,
                error_control: ServiceErrorControl::Normal,
                executable_path: std::env::current_exe()?,
                launch_arguments: vec![
                    "--root".into(),
                    root.as_os_str().into(),
                    "service-host".into(),
                ],
                dependencies: vec![],
                account_name: Some(account_name.into()),
                account_password: Some(password.into()),
            };
            created = Some(
                scm.create_service(&info, ServiceAccess::ALL_ACCESS)
                    .map_err(crate::operation)?,
            );
            let service = created.as_ref().unwrap();
            service
                .set_description("使用 systemd 配置子集托管普通 Windows 程序")
                .map_err(crate::operation)?;
            service
                .update_failure_actions(ServiceFailureActions {
                    reset_period: ServiceFailureResetPeriod::After(Duration::from_secs(86400)),
                    reboot_msg: None,
                    command: None,
                    actions: Some(vec![ServiceAction {
                        action_type: ServiceActionType::Restart,
                        delay: Duration::from_secs(5),
                    }]),
                })
                .map_err(crate::operation)?;
            service
                .set_failure_actions_on_non_crash_failures(true)
                .map_err(crate::operation)?;
            atomic_write(
                &receipt,
                &serde_json::to_vec(&Installation {
                    account: account_name.into(),
                    sid: account.sid.clone(),
                    granted_logon: granted,
                })?,
            )?;
            Ok(())
        })();
        if let Err(error) = result {
            let mut rollback = vec![];
            if let Some(service) = created
                && let Err(e) = service.delete()
            {
                rollback.push(e.to_string());
            }
            if granted && let Err(e) = security::revoke_logon(&account) {
                rollback.push(e.to_string());
            }
            if let Err(e) = security::restore(&backups) {
                rollback.push(e.to_string());
            }
            if receipt.exists()
                && let Err(e) = std::fs::remove_file(receipt)
            {
                rollback.push(e.to_string());
            }
            return Err(Error::Operation(format!(
                "{error}；回滚诊断：{}",
                rollback.join("；")
            )));
        }
        Ok(())
    }
    /// 等待 SCM 达到指定状态。参数：service/desired 为服务和目标。返回：结果。
    fn wait_state(service: &Service, desired: ServiceState) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(1800);
        loop {
            let status = service.query_status().map_err(crate::operation)?;
            if status.current_state == desired {
                return Ok(());
            }
            if desired == ServiceState::Running && status.current_state == ServiceState::Stopped {
                return Err(Error::Operation(format!(
                    "服务启动失败：{:?}",
                    status.exit_code
                )));
            }
            if Instant::now() > deadline {
                return Err(Error::Operation("等待 SCM 状态超时".into()));
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    /// 启动管理器 SCM 服务。参数：root 为目录。返回：结果。
    pub fn start(root: &Path) -> Result<()> {
        let service = service(
            ServiceAccess::START | ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG,
        )?;
        check_root(root, &service)?;
        if service
            .query_status()
            .map_err(crate::operation)?
            .current_state
            != ServiceState::Running
        {
            service.start::<&str>(&[]).map_err(crate::operation)?;
            wait_state(&service, ServiceState::Running)?;
        }
        Ok(())
    }
    /// 停止管理器 SCM 服务。参数：root 为目录。返回：结果。
    pub fn stop(root: &Path) -> Result<()> {
        let service = service(
            ServiceAccess::STOP | ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG,
        )?;
        check_root(root, &service)?;
        if service
            .query_status()
            .map_err(crate::operation)?
            .current_state
            != ServiceState::Stopped
        {
            service.stop().map_err(crate::operation)?;
            wait_state(&service, ServiceState::Stopped)?;
        }
        Ok(())
    }
    /// 查询 SCM 状态。参数：root 为目录。返回：可显示状态。
    pub fn status(root: &Path) -> Result<String> {
        let service = service(ServiceAccess::QUERY_STATUS | ServiceAccess::QUERY_CONFIG)?;
        check_root(root, &service)?;
        Ok(format!(
            "{:?}",
            service.query_status().map_err(crate::operation)?
        ))
    }
    /// 停止并删除 SCM 注册，保留 unit 和日志；移除本次安装新增账户授权。
    /// 参数：root 为目录。返回：结果。
    pub fn uninstall(root: &Path) -> Result<()> {
        stop(root)?;
        let service = service(ServiceAccess::DELETE | ServiceAccess::QUERY_CONFIG)?;
        check_root(root, &service)?;
        service.delete().map_err(crate::operation)?;
        drop(service);
        let receipt = root.join("installation.json");
        if receipt.exists() {
            let installed: Installation = serde_json::from_slice(&std::fs::read(&receipt)?)?;
            if installed.granted_logon {
                let account = security::account(&installed.account)?;
                if account.sid != installed.sid {
                    return Err(Error::Operation("账户 SID 已改变，未移除登录权限".into()));
                }
                security::revoke_logon(&account)?;
            }
            std::fs::remove_file(receipt)?;
        }
        Ok(())
    }
    define_windows_service!(ffi_service_main, service_main);
    /// Windows 服务入口，错误由服务状态及日志报告。参数：arguments 为 SCM 参数。返回：无。
    fn service_main(_arguments: Vec<OsString>) {
        if let Err(e) = run_service()
            && let Some(root) = ROOT.get()
            && let Ok(logger) = crate::logging::Logger::new(&root.join("logs"))
        {
            let _ = logger.write("manager", 0, "manager", &format!("服务错误：{e}"));
        }
    }
    /// 在 SCM 主线程启动 dispatcher。参数：root 为数据目录。返回：dispatcher 结果。
    pub fn host(root: &Path) -> Result<()> {
        ROOT.set(root.into())
            .map_err(|_| Error::Operation("SCM root 已设置".into()))?;
        service_dispatcher::start(NAME, ffi_service_main).map_err(crate::operation)
    }
    /// 构造 SCM 状态。参数：state/checkpoint/code 为状态、进度和错误码。返回：状态结构。
    fn scm_status(state: ServiceState, checkpoint: u32, code: u32) -> ServiceStatus {
        ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state: state,
            controls_accepted: if state == ServiceState::Running {
                ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN
            } else {
                ServiceControlAccept::empty()
            },
            exit_code: ServiceExitCode::Win32(code),
            checkpoint,
            wait_hint: if matches!(
                state,
                ServiceState::StartPending | ServiceState::StopPending
            ) {
                Duration::from_secs(120)
            } else {
                Duration::ZERO
            },
            process_id: None,
        }
    }
    /// 共用管理器与 SCM 状态机。参数：无。返回：服务运行结果。
    fn run_service() -> Result<()> {
        let (stop_tx, stop_rx) = watch::channel(false);
        let requested_stop = stop_rx.clone();
        let status = service_control_handler::register(NAME, move |event| match event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = stop_tx.send(true);
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        })
        .map_err(crate::operation)?;
        status
            .set_service_status(scm_status(ServiceState::StartPending, 1, 0))
            .map_err(crate::operation)?;
        let result = (|| {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(async {
                let manager = Manager::new(ROOT.get().unwrap())?;
                let (ready_tx, ready_rx) = oneshot::channel();
                let mut stop = stop_rx.clone();
                let mut server = tokio::spawn(crate::ipc::serve(manager.clone(), stop_rx, ready_tx));
                ready_rx.await.map_err(crate::operation)??;
                status.set_service_status(scm_status(ServiceState::Running, 0, 0)).map_err(crate::operation)?;
                tokio::select! { result = &mut server => return result.map_err(crate::operation)?, _ = stop.changed() => () }
                let mut checkpoint = 1;
                status.set_service_status(scm_status(ServiceState::StopPending, checkpoint, 0)).map_err(crate::operation)?;
                let mut previous = serde_json::to_string(&manager.status(None)?)?;
                loop {
                    tokio::select! {
                        result = &mut server => return result.map_err(crate::operation)?,
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {
                            let progress = serde_json::to_string(&manager.status(None)?)?;
                            // 只在 unit 有实际进度时递增 checkpoint。
                            if progress != previous { checkpoint += 1; previous = progress; status.set_service_status(scm_status(ServiceState::StopPending, checkpoint, 0)).map_err(crate::operation)?; }
                        }
                    }
                }
            })
        })();
        status
            .set_service_status(scm_status(
                ServiceState::Stopped,
                0,
                if result.is_ok() || *requested_stop.borrow() {
                    0
                } else {
                    1
                },
            ))
            .map_err(crate::operation)?;
        result
    }
}

#[cfg(windows)]
pub use implementation::{host, install, start, status, stop, uninstall};
