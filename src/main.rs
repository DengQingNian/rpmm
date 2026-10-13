//! rpmm 命令行与前台入口。
use clap::{Parser, Subcommand, ValueEnum};
#[cfg(not(windows))]
use rpmm::Error;
use rpmm::{
    Result, config, graph,
    ipc::{self, Action},
    manager::Manager,
};
use std::path::PathBuf;

#[derive(Parser)]
#[command(version, about = "rpmm 桌面进程管理器的命令行工具")]
struct Cli {
    #[arg(long, global = true, help = "数据目录，默认 %LOCALAPPDATA%/rpmm")]
    root: Option<PathBuf>,
    #[arg(long, global = true, help = "输出 JSON 状态或日志")]
    json: bool,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Manager {
        #[command(subcommand)]
        command: ManagerCommand,
    },
    /// 开机自启：注册为系统启动时运行的 Windows 服务，不需要用户登录
    Autostart {
        #[command(subcommand)]
        command: AutostartCommand,
    },
    Verify,
    List,
    Start {
        unit: String,
    },
    Stop {
        unit: String,
    },
    Restart {
        unit: String,
    },
    Status {
        unit: Option<String>,
    },
    Enable {
        unit: String,
    },
    Disable {
        unit: String,
    },
    DaemonReload,
    ResetFailed {
        unit: String,
    },
    Delete {
        unit: String,
        #[arg(long, help = "删除前先停止正在运行的实例")]
        stop: bool,
    },
    Logs {
        unit: String,
        #[arg(long)]
        follow: bool,
        #[arg(long, value_enum)]
        source: Option<Source>,
        #[arg(long, default_value_t = 100)]
        lines: usize,
    },
}
#[derive(Subcommand)]
enum ManagerCommand {
    Run,
    Stop,
}
#[derive(Subcommand)]
enum AutostartCommand {
    /// 注册开机自启服务；重复执行会按当前程序与数据目录更新登记
    Install {
        #[arg(long, help = "服务账户，默认 LocalSystem")]
        account: Option<String>,
        #[arg(long, help = "服务账户密码，配合 --account 使用")]
        password: Option<String>,
    },
    /// 停止并删除开机自启登记
    Uninstall,
    /// 启动已注册的开机自启服务
    Start,
    /// 停止正在运行的开机自启服务
    Stop,
    /// 查询开机自启的注册与运行状态
    Status,
    /// 服务控制管理器调用的入口，注册时自动写入服务命令行
    #[command(hide = true)]
    Run,
}
#[derive(Clone, ValueEnum)]
enum Source {
    Stdout,
    Stderr,
    Manager,
    Health,
}

/// 解析参数并报告错误。参数：命令行。返回：进程退出码。
fn main() -> std::process::ExitCode {
    match run(Cli::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            std::process::ExitCode::FAILURE
        }
    }
}
/// 调度校验和异步管理命令。参数：cli 为参数。返回：操作结果。
fn run(cli: Cli) -> Result<()> {
    let root = cli.root.unwrap_or_else(rpmm::desktop::default_root);
    // 数据目录同样接受 C:/、C:\、/c/ 等写法；相对路径仍按当前目录解析。
    let root = std::path::absolute(rpmm::paths::normalize_path(&root))?;
    // 控制命令不得隐式创建数据目录；只有前台管理器和守护进程可以创建。
    if matches!(
        &cli.command,
        Command::Manager {
            command: ManagerCommand::Run
        } | Command::Autostart {
            command: AutostartCommand::Run
        }
    ) {
        std::fs::create_dir_all(&root)?;
    }
    let root = if root.exists() {
        std::fs::canonicalize(root)?
    } else {
        root
    };
    match cli.command {
        Command::Verify => {
            let units = config::load(&root.join("units"))?;
            graph::order(&units, &units.keys().cloned().collect())?;
            for name in units.keys() {
                let plan = graph::start_plan(&units, std::slice::from_ref(name))?;
                for warning in plan.warnings {
                    eprintln!("警告：{warning}");
                }
            }
            println!("校验通过：{} 个 unit", units.len());
            Ok(())
        }
        Command::Manager { command } => manager_command(&root, command),
        Command::Autostart { command } => autostart_command(&root, command, cli.json),
        command => {
            let action = match command {
                Command::List => Action::List,
                Command::Start { unit } => Action::Start { unit },
                Command::Stop { unit } => Action::Stop { unit },
                Command::Restart { unit } => Action::Restart { unit },
                Command::Status { unit } => Action::Status { unit },
                Command::Enable { unit } => Action::Enable { unit },
                Command::Disable { unit } => Action::Disable { unit },
                Command::DaemonReload => Action::DaemonReload,
                Command::ResetFailed { unit } => Action::ResetFailed { unit },
                Command::Delete { unit, stop } => Action::Delete {
                    unit,
                    stop,
                    expected: None,
                },
                Command::Logs {
                    unit,
                    follow,
                    source,
                    lines,
                } => Action::Logs {
                    unit,
                    follow,
                    lines,
                    source: source.map(|s| {
                        match s {
                            Source::Stdout => "stdout",
                            Source::Stderr => "stderr",
                            Source::Manager => "manager",
                            Source::Health => "health",
                        }
                        .into()
                    }),
                },
                _ => unreachable!(),
            };
            tokio::runtime::Runtime::new()?.block_on(ipc::client(&root, action, cli.json))
        }
    }
}
/// 执行开机自启服务的注册与控制。参数：root 为数据目录，command 为动作，json 为显示模式。返回：结果。
fn autostart_command(root: &std::path::Path, command: AutostartCommand, json: bool) -> Result<()> {
    match command {
        AutostartCommand::Install { account, password } => {
            // 服务命令行固定指向当前程序，重复注册时按新路径与新数据目录更新。
            let options = rpmm::service::InstallOptions {
                executable: std::env::current_exe()?,
                arguments: rpmm::service::ENTRY
                    .iter()
                    .map(|item| item.to_string())
                    .collect(),
                root: root.to_path_buf(),
                account,
                password,
            };
            rpmm::service::install(&options)?;
            // 用户配置目录会随账户变化或删除而失效，注册时提醒改用机器级目录。
            if let Some(warning) = rpmm::service::profile_warning(root) {
                eprintln!("警告：{warning}");
            }
            println!(
                "已注册开机自启（{}）：系统启动时自动运行，不需要任何用户登录",
                rpmm::service::NAME
            );
            println!("数据目录：{}", root.display());
            println!("已启用的服务随守护进程启动，用 rpmm autostart status 查看状态");
            Ok(())
        }
        AutostartCommand::Uninstall => {
            rpmm::service::uninstall()?;
            println!("已删除开机自启登记，系统启动时不再自动运行");
            Ok(())
        }
        AutostartCommand::Start => {
            rpmm::service::start()?;
            println!("已请求启动守护进程");
            Ok(())
        }
        AutostartCommand::Stop => {
            rpmm::service::stop()?;
            println!("已请求停止守护进程，正在按依赖逆序回收子进程");
            Ok(())
        }
        AutostartCommand::Status => {
            let registration = rpmm::service::registration()?;
            if json {
                println!("{}", serde_json::to_string_pretty(&registration)?);
            } else {
                print_registration(&registration);
            }
            Ok(())
        }
        // 服务控制管理器启动本进程时进入这里，直到收到停止或关机控制码才返回。
        AutostartCommand::Run => rpmm::service::run(root),
    }
}
/// 打印开机自启状态。参数：registration 为查询结果。返回：无。
fn print_registration(registration: &rpmm::service::Registration) {
    if !registration.installed {
        println!("开机自启：未注册");
        println!("提示：以管理员身份运行 rpmm autostart install，注册后由系统在启动时拉起守护进程");
        return;
    }
    let state = match registration.state.as_str() {
        "running" => "运行中",
        "stopped" => "已停止",
        "start-pending" => "启动中",
        "stop-pending" => "停止中",
        "paused" => "已暂停",
        other => other,
    };
    let start_type = match registration.start_type.as_str() {
        "auto" => "自动",
        "demand" => "手动",
        "disabled" => "已禁用",
        other => other,
    };
    let account = if registration.account.is_empty() {
        "LocalSystem"
    } else {
        registration.account.as_str()
    };
    let pid = registration
        .pid
        .map(|value| format!("（PID {value}）"))
        .unwrap_or_default();
    println!("开机自启：已注册（{}）", rpmm::service::NAME);
    println!("状态：{state}{pid}");
    println!("启动类型：{start_type}");
    println!("账户：{account}");
    println!("程序：{}", registration.executable);
    println!(
        "数据目录：{}",
        registration.root.as_deref().unwrap_or("（未指定）")
    );
}
/// 执行不依赖系统服务的前台管理器。参数：root/command 为目录和动作。返回：结果。
fn manager_command(root: &std::path::Path, command: ManagerCommand) -> Result<()> {
    #[cfg(windows)]
    {
        match command {
            ManagerCommand::Stop => tokio::runtime::Runtime::new()?.block_on(ipc::client(root, Action::Shutdown, false)),
            ManagerCommand::Run => tokio::runtime::Runtime::new()?.block_on(async {
                let manager = Manager::new(root)?;
                let (stop_tx, stop_rx) = tokio::sync::watch::channel(false); let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
                let mut server = tokio::spawn(ipc::serve(manager, stop_rx, ready_tx));
                ready_rx.await.map_err(rpmm::operation)??;
                println!("rpmm 已就绪：{}", root.display());
                tokio::select! { result = &mut server => return result.map_err(rpmm::operation)?, result = tokio::signal::ctrl_c() => { result?; let _ = stop_tx.send(true); } }
                server.await.map_err(rpmm::operation)?
            }),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (root, command);
        Err(Error::Operation("仅支持 Windows 管理器".into()))
    }
}
