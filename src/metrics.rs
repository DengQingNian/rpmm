//! 宿主机与主进程资源采集，保留相邻采样以计算速率。
use serde::Serialize;
use std::{collections::BTreeMap, time::Instant};
use sysinfo::{Disks, Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

pub struct Collector {
    system: System,
    disks: Disks,
    sampled: Instant,
    primed: bool,
    process_sampled: Option<(u32, u64, Instant)>,
}
#[derive(Serialize)]
pub struct Disk {
    pub mount: String,
    pub total: u64,
    pub available: u64,
    pub read_per_sec: f64,
    pub write_per_sec: f64,
}
#[derive(Serialize)]
pub struct Host {
    pub time: String,
    pub name: String,
    pub cpu: Option<f32>,
    pub memory_used: u64,
    pub memory_total: u64,
    pub read_per_sec: Option<f64>,
    pub write_per_sec: Option<f64>,
    pub disks: Vec<Disk>,
}
#[derive(Serialize)]
pub struct Resources {
    pub pid: u32,
    pub start_time: u64,
    pub cpu: Option<f32>,
    pub memory: u64,
    pub virtual_memory: u64,
    pub read_total: u64,
    pub write_total: u64,
    pub read_per_sec: Option<f64>,
    pub write_per_sec: Option<f64>,
    pub open_files: Option<usize>,
    pub environment: BTreeMap<String, String>,
    pub environment_source: String,
    pub connections: Vec<Connection>,
    pub connection_error: Option<String>,
}
#[derive(Serialize)]
pub struct Connection {
    pub protocol: String,
    pub local: String,
    pub remote: String,
    pub state: String,
}
impl Default for Collector {
    /// 初始化系统采样器。参数：无。返回：尚未完成速率采样的采集器。
    fn default() -> Self {
        Self {
            system: System::new_all(),
            disks: Disks::new_with_refreshed_list(),
            sampled: Instant::now(),
            primed: false,
            process_sampled: None,
        }
    }
}
impl Collector {
    /// 采集宿主机和指定主进程。参数：pid 为可选主进程。返回：整机和进程快照。
    pub fn sample(&mut self, pid: Option<u32>) -> (Host, Option<Resources>) {
        // 速率分别使用宿主机和进程的采样基线；首次采样或切换进程时不输出累计计数形成的伪峰值。
        let elapsed = self.sampled.elapsed().as_secs_f64().max(0.001);
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        let targets: Vec<_> = pid.into_iter().map(Pid::from_u32).collect();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&targets),
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_disk_usage()
                .with_environ(UpdateKind::Always),
        );
        self.disks.refresh(true);
        let disks: Vec<_> = self
            .disks
            .iter()
            .map(|disk| {
                let io = disk.usage();
                Disk {
                    mount: disk.mount_point().display().to_string(),
                    total: disk.total_space(),
                    available: disk.available_space(),
                    read_per_sec: if self.primed {
                        io.read_bytes as f64 / elapsed
                    } else {
                        0.0
                    },
                    write_per_sec: if self.primed {
                        io.written_bytes as f64 / elapsed
                    } else {
                        0.0
                    },
                }
            })
            .collect();
        let process = pid.and_then(|pid| {
            self.system.process(Pid::from_u32(pid)).map(|p| {
                let io = p.disk_usage();
                let process_elapsed = self
                    .process_sampled
                    .filter(|(old_pid, start, _)| *old_pid == pid && *start == p.start_time())
                    .map(|(_, _, sampled)| sampled.elapsed().as_secs_f64().max(0.001));
                let environment = p
                    .environ()
                    .iter()
                    .filter_map(|value| {
                        value
                            .to_string_lossy()
                            .split_once('=')
                            .map(|(key, val)| (key.to_string(), val.to_string()))
                    })
                    .collect();
                Resources {
                    pid,
                    start_time: p.start_time(),
                    cpu: process_elapsed
                        .map(|_| p.cpu_usage() / self.system.cpus().len().max(1) as f32),
                    memory: p.memory(),
                    virtual_memory: p.virtual_memory(),
                    read_total: io.total_read_bytes,
                    write_total: io.total_written_bytes,
                    read_per_sec: process_elapsed.map(|seconds| io.read_bytes as f64 / seconds),
                    write_per_sec: process_elapsed.map(|seconds| io.written_bytes as f64 / seconds),
                    open_files: p.open_files(),
                    environment,
                    environment_source: "从运行主进程读取的环境变量".into(),
                    connections: vec![],
                    connection_error: None,
                }
            })
        });
        self.process_sampled = process
            .as_ref()
            .map(|p| (p.pid, p.start_time, Instant::now()));
        let host = Host {
            time: chrono::Utc::now().to_rfc3339(),
            name: System::host_name().unwrap_or_default(),
            cpu: self.primed.then(|| self.system.global_cpu_usage()),
            memory_used: self.system.used_memory(),
            memory_total: self.system.total_memory(),
            read_per_sec: self
                .primed
                .then(|| disks.iter().map(|d| d.read_per_sec).sum()),
            write_per_sec: self
                .primed
                .then(|| disks.iter().map(|d| d.write_per_sec).sum()),
            disks,
        };
        self.sampled = Instant::now();
        self.primed = true;
        (host, process)
    }
}

/// 读取 Windows TCP/UDP 连接表。参数：pid 为主进程编号。返回：连接列表或采集诊断。
#[cfg(windows)]
pub fn connections(pid: u32) -> crate::Result<Vec<Connection>> {
    use std::os::windows::process::CommandExt;
    // 使用系统目录的固定程序和参数，不经 shell、不解析用户输入，不弹出控制台。
    let executable = std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| "C:/Windows".into())
        .join("System32/netstat.exe");
    let output = std::process::Command::new(executable)
        .args(["-ano"])
        .creation_flags(0x08000000)
        .output()?;
    if !output.status.success() {
        return Err(crate::Error::Operation("网络连接采集失败".into()));
    }
    Ok(parse_connections(
        &String::from_utf8_lossy(&output.stdout),
        pid,
    ))
}
/// 非 Windows 连接采集占位。参数：pid 为主进程编号。返回：平台不支持诊断。
#[cfg(not(windows))]
pub fn connections(_pid: u32) -> crate::Result<Vec<Connection>> {
    Err(crate::Error::Operation("网络连接采集仅支持 Windows".into()))
}

/// 解析数值地址连接表。参数：text 为 netstat 输出，pid 为筛选进程。返回：TCP/UDP 明细。
pub fn parse_connections(text: &str, pid: u32) -> Vec<Connection> {
    // TCP 有状态列，UDP 没有；忽略表头及不属于目标进程的行。
    text.lines()
        .filter_map(|line| {
            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() < 4 || fields.last()?.parse::<u32>().ok()? != pid {
                return None;
            }
            match fields[0] {
                "TCP" if fields.len() == 5 => Some(Connection {
                    protocol: "TCP".into(),
                    local: fields[1].into(),
                    remote: fields[2].into(),
                    state: fields[3].into(),
                }),
                "UDP" if fields.len() == 4 => Some(Connection {
                    protocol: "UDP".into(),
                    local: fields[1].into(),
                    remote: fields[2].into(),
                    state: "—".into(),
                }),
                _ => None,
            }
        })
        .collect()
}
