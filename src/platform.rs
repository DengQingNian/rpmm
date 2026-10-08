//! Windows 原生进程与 Job Object 封装。
use crate::{
    Result,
    config::{Unit, expand_command},
    logging::Logger,
};
use std::collections::BTreeMap;

/// 生命周期核心使用的进程抽象。
pub trait Process: Send + Sync {
    /// 查询主进程编号。参数：无。返回：PID。
    fn pid(&self) -> u32;
    /// 非阻塞查询退出码。参数：无。返回：退出码；仍在运行时 None。
    fn poll(&self) -> Result<Option<u32>>;
    /// 终止所有关联进程。参数：无。返回：结果。
    fn terminate(&self) -> Result<()>;
}
pub trait Backend: Send + Sync {
    /// 创建并托管进程。参数：unit、command、instance、main_pid、logger 为定义和上下文。返回：进程对象。
    fn spawn(
        &self,
        unit: &Unit,
        command: &[String],
        instance: u64,
        main_pid: Option<u32>,
        logger: &Logger,
    ) -> Result<Box<dyn Process>>;
}
pub struct Native;

/// 合并继承环境；Windows 环境名不区分大小写。参数：unit 为覆盖定义。返回：子进程环境。
pub fn environment(unit: &Unit) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = std::env::vars_os()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.to_string_lossy().into_owned(),
            )
        })
        .collect();
    for (key, value) in &unit.environment {
        let existing = env.keys().find(|k| k.eq_ignore_ascii_case(key)).cloned();
        if let Some(existing) = existing {
            env.remove(&existing);
        }
        env.insert(key.clone(), value.clone());
    }
    env
}

/// 将 argv 编码为标准 Windows 命令行。参数：args 为已展开参数。返回：带正确引号的命令行。
pub fn command_line(args: &[String]) -> String {
    args.iter()
        .map(|arg| {
            let mut out = String::from("\"");
            let mut slashes = 0;
            // 引号前反斜杠翻倍，结束引号前尾部反斜杠也翻倍。
            for ch in arg.chars() {
                if ch == '\\' {
                    slashes += 1;
                    continue;
                }
                if ch == '"' {
                    out.extend(std::iter::repeat_n('\\', slashes * 2 + 1));
                } else {
                    out.extend(std::iter::repeat_n('\\', slashes));
                }
                slashes = 0;
                out.push(ch);
            }
            out.extend(std::iter::repeat_n('\\', slashes * 2));
            out.push('"');
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
pub mod win {
    use super::*;
    use std::{fs::File, os::windows::io::FromRawHandle};
    use windows::{
        Win32::{
            Foundation::*,
            Security::SECURITY_ATTRIBUTES,
            Storage::FileSystem::*,
            System::{JobObjects::*, Pipes::CreatePipe, Threading::*},
        },
        core::{PCWSTR, PWSTR},
    };

    pub struct Handle(pub HANDLE);
    // Windows 内核句柄可以跨线程使用；所有权仍唯一，查询操作不修改 Rust 内存。
    unsafe impl Send for Handle {}
    unsafe impl Sync for Handle {}
    impl Drop for Handle {
        /// 关闭拥有的句柄。参数：无。返回：无。
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }
    /// 编码 NUL 结尾 UTF-16。参数：text 为字符串。返回：UTF-16 缓冲。
    pub fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }
    /// 使用 Windows 原生规则解析命令行。参数：text 为完整命令行。返回：参数表。
    pub fn decode_command_line(text: &str) -> Result<Vec<String>> {
        unsafe {
            let mut count = 0;
            let args = windows::Win32::UI::Shell::CommandLineToArgvW(
                PCWSTR(wide(text).as_ptr()),
                &mut count,
            );
            if args.is_null() {
                return Err(crate::operation(std::io::Error::last_os_error()));
            }
            let result = std::slice::from_raw_parts(args, count as usize)
                .iter()
                .map(|arg| arg.to_string().map_err(crate::operation))
                .collect();
            let _ = LocalFree(Some(HLOCAL(args.cast())));
            result
        }
    }

    struct Attributes {
        storage: Vec<usize>,
        list: LPPROC_THREAD_ATTRIBUTE_LIST,
    }
    impl Drop for Attributes {
        /// 释放进程属性表。参数：无。返回：无。
        fn drop(&mut self) {
            unsafe {
                DeleteProcThreadAttributeList(self.list);
            }
        }
    }
    /// 构造继承句柄白名单。参数：handles 为三个标准 IO 句柄。返回：属性表。
    fn attributes(handles: &[HANDLE]) -> Result<Attributes> {
        let mut size = 0;
        unsafe {
            let _ = InitializeProcThreadAttributeList(None, 1, None, &mut size);
        }
        let mut storage = vec![0usize; size.div_ceil(std::mem::size_of::<usize>())];
        let list = LPPROC_THREAD_ATTRIBUTE_LIST(storage.as_mut_ptr().cast());
        unsafe {
            InitializeProcThreadAttributeList(Some(list), 1, None, &mut size)
                .map_err(crate::operation)?;
            let attrs = Attributes { storage, list };
            UpdateProcThreadAttribute(
                list,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,
                Some(handles.as_ptr().cast()),
                std::mem::size_of_val(handles),
                None,
                None,
            )
            .map_err(crate::operation)?;
            Ok(attrs)
        }
    }
    /// 创建一个可继承写端和不可继承读端。参数：无。返回：读写句柄。
    fn pipe() -> Result<(Handle, Handle)> {
        let security = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            bInheritHandle: true.into(),
            ..Default::default()
        };
        let mut read = HANDLE::default();
        let mut write = HANDLE::default();
        unsafe {
            CreatePipe(&mut read, &mut write, Some(&security), 0).map_err(crate::operation)?;
            let read = Handle(read);
            let write = Handle(write);
            SetHandleInformation(read.0, HANDLE_FLAG_INHERIT.0, HANDLE_FLAGS(0))
                .map_err(crate::operation)?;
            Ok((read, write))
        }
    }
    struct Child {
        process: Handle,
        job: Handle,
        pid: u32,
        readers: std::sync::Mutex<Vec<std::thread::JoinHandle<()>>>,
    }
    impl Process for Child {
        /// 返回 PID。参数：无。返回：PID。
        fn pid(&self) -> u32 {
            self.pid
        }
        /// 查询退出码。参数：无。返回：退出码或 None。
        fn poll(&self) -> Result<Option<u32>> {
            unsafe {
                let result = WaitForSingleObject(self.process.0, 0);
                if result == WAIT_TIMEOUT {
                    return Ok(None);
                }
                if result != WAIT_OBJECT_0 {
                    return Err(crate::operation(std::io::Error::last_os_error()));
                }
                let mut code = 0;
                GetExitCodeProcess(self.process.0, &mut code).map_err(crate::operation)?;
                Ok(Some(code))
            }
        }
        /// 终止 Job 中整个进程树。参数：无。返回：结果。
        fn terminate(&self) -> Result<()> {
            unsafe {
                TerminateJobObject(self.job.0, 1).map_err(crate::operation)?;
                if WaitForSingleObject(self.process.0, 5000) != WAIT_OBJECT_0 {
                    return Err(crate::Error::Operation("终止后主进程未退出".into()));
                }
            }
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let mut readers = self.readers.lock().map_err(crate::operation)?;
            // 先终止整个 Job，再等待 EOF，保证正常停机前已经排空输出。
            while readers.iter().any(|thread| !thread.is_finished()) {
                if std::time::Instant::now() >= deadline {
                    return Err(crate::Error::Operation("终止后输出管道未关闭".into()));
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            for thread in readers.drain(..) {
                thread
                    .join()
                    .map_err(|_| crate::Error::Operation("输出读取线程异常".into()))?;
            }
            Ok(())
        }
    }
    impl Drop for Child {
        /// 确保提前退出和启动超时也清理进程及日志线程。参数：无。返回：无。
        fn drop(&mut self) {
            let _ = self.terminate();
        }
    }
    impl Backend for Native {
        /// 挂起创建、关联 Job 并恢复进程。参数：配置及记录上下文。返回：受托管进程。
        fn spawn(
            &self,
            unit: &Unit,
            command: &[String],
            instance: u64,
            main_pid: Option<u32>,
            logger: &Logger,
        ) -> Result<Box<dyn Process>> {
            let env = environment(unit);
            let args = expand_command(command, &env, main_pid)?;
            if args.iter().chain(env.values()).any(|s| s.contains('\0')) {
                return Err(crate::Error::Config("命令或环境包含 NUL".into()));
            }
            let executable = wide(&args[0]);
            let mut cmd = wide(&command_line(&args));
            let default_cwd =
                std::env::var("SystemRoot").unwrap_or_else(|_| "C:/Windows".into()) + "/System32";
            let cwd = wide(unit.working_directory.as_deref().unwrap_or(&default_cwd));
            let mut variables: Vec<_> = env.iter().collect();
            variables.sort_by_key(|(k, _)| k.to_uppercase());
            let mut block: Vec<u16> = variables
                .iter()
                .flat_map(|(k, v)| wide(&format!("{k}={v}")))
                .collect();
            block.push(0);
            let (out_read, out_write) = pipe()?;
            let (err_read, err_write) = pipe()?;
            let output = unit
                .stdout_directory
                .as_ref()
                .map(|directory| {
                    std::fs::create_dir_all(directory)?;
                    std::fs::OpenOptions::new().create(true).append(true).open(
                        std::path::Path::new(directory).join(format!("{}.stdout.log", unit.name)),
                    )
                })
                .transpose()?;
            let security = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                bInheritHandle: true.into(),
                ..Default::default()
            };
            unsafe {
                let nul = Handle(
                    CreateFileW(
                        PCWSTR(wide("NUL").as_ptr()),
                        GENERIC_READ.0,
                        FILE_SHARE_READ | FILE_SHARE_WRITE,
                        Some(&security),
                        OPEN_EXISTING,
                        FILE_ATTRIBUTE_NORMAL,
                        None,
                    )
                    .map_err(crate::operation)?,
                );
                let handles = [nul.0, out_write.0, err_write.0];
                let attrs = attributes(&handles)?;
                // storage 必须保持到 CreateProcess 返回，属性表引用其中内存。
                let _keep_alive = &attrs.storage;
                let startup = STARTUPINFOEXW {
                    StartupInfo: STARTUPINFOW {
                        cb: std::mem::size_of::<STARTUPINFOEXW>() as u32,
                        dwFlags: STARTF_USESTDHANDLES,
                        hStdInput: nul.0,
                        hStdOutput: out_write.0,
                        hStdError: err_write.0,
                        ..Default::default()
                    },
                    lpAttributeList: attrs.list,
                };
                let job = Handle(CreateJobObjectW(None, PCWSTR::null()).map_err(crate::operation)?);
                let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
                    BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
                        LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                // 内存额度约束整个进程树的提交内存，阻止超过额度的新分配。
                if let Some(maximum) = unit.memory_max {
                    limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
                    limits.JobMemoryLimit = maximum;
                }
                SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    std::mem::size_of_val(&limits) as u32,
                )
                .map_err(crate::operation)?;
                if let Some(percent) = unit.cpu_quota {
                    let cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION {
                        ControlFlags: JOB_OBJECT_CPU_RATE_CONTROL_ENABLE
                            | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP,
                        Anonymous: JOBOBJECT_CPU_RATE_CONTROL_INFORMATION_0 {
                            CpuRate: percent * 100,
                        },
                    };
                    SetInformationJobObject(
                        job.0,
                        JobObjectCpuRateControlInformation,
                        &cpu as *const _ as *const _,
                        std::mem::size_of_val(&cpu) as u32,
                    )
                    .map_err(crate::operation)?;
                }
                let mut info = PROCESS_INFORMATION::default();
                CreateProcessW(
                    PCWSTR(executable.as_ptr()),
                    Some(PWSTR(cmd.as_mut_ptr())),
                    None,
                    None,
                    true,
                    CREATE_SUSPENDED
                        | CREATE_UNICODE_ENVIRONMENT
                        | EXTENDED_STARTUPINFO_PRESENT
                        | CREATE_NO_WINDOW,
                    Some(block.as_ptr().cast()),
                    PCWSTR(cwd.as_ptr()),
                    &startup.StartupInfo,
                    &mut info,
                )
                .map_err(crate::operation)?;
                let process = Handle(info.hProcess);
                let thread = Handle(info.hThread);
                if let Err(e) = AssignProcessToJobObject(job.0, process.0) {
                    let _ = TerminateProcess(process.0, 1);
                    let _ = WaitForSingleObject(process.0, 5000);
                    return Err(crate::operation(e));
                }
                if ResumeThread(thread.0) == u32::MAX {
                    return Err(crate::operation(std::io::Error::last_os_error()));
                }
                drop(out_write);
                drop(err_write);
                let mut readers = vec![];
                for (read, source, output) in
                    [(out_read, "stdout", output), (err_read, "stderr", None)]
                {
                    let file = File::from_raw_handle(read.0.0);
                    std::mem::forget(read);
                    let logger = logger.clone();
                    let name = unit.name.clone();
                    readers.push(std::thread::spawn(move || {
                        logger.drain_to(file, &name, instance, source, output)
                    }));
                }
                Ok(Box::new(Child {
                    process,
                    job,
                    pid: info.dwProcessId,
                    readers: std::sync::Mutex::new(readers),
                }))
            }
        }
    }
}

#[cfg(not(windows))]
impl Backend for Native {
    /// 非 Windows 平台明确拒绝托管。参数：配置及上下文。返回：不支持错误。
    fn spawn(
        &self,
        _: &Unit,
        _: &[String],
        _: u64,
        _: Option<u32>,
        _: &Logger,
    ) -> Result<Box<dyn Process>> {
        Err(crate::Error::Operation("仅支持 Windows 托管".into()))
    }
}
