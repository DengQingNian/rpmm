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
    let root = std::path::absolute(root)?;
    // 控制命令不得隐式创建数据目录；只有前台管理器可以创建。
    if matches!(
        &cli.command,
        Command::Manager {
            command: ManagerCommand::Run
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
