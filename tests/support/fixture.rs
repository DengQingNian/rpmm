//! Windows 集成测试使用的普通控制台程序，仅 test-fixtures feature 构建。
use std::{io::Write, time::Duration};
/// 执行测试模式。参数：命令行模式和参数。返回：程序退出码或持续运行。
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("args") => println!("{}", serde_json::to_string(&args[1..]).unwrap()),
        #[cfg(windows)]
        Some("limits") => report_limits(),
        #[cfg(windows)]
        Some("tree-limits") => {
            report_limits();
            let status = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("limits")
                .status()
                .unwrap();
            assert!(status.success());
        }
        Some("exit") => std::process::exit(args[1].parse().unwrap()),
        Some("env") => println!(
            "{}",
            serde_json::json!({ "value": std::env::var("RPMM_TEST_ENV").unwrap(), "directory": std::env::current_dir().unwrap().to_string_lossy() })
        ),
        Some("flood") => {
            let stderr = std::thread::spawn(|| {
                let mut stream = std::io::stderr().lock();
                for _ in 0..512 {
                    stream.write_all(&[b'e'; 8192]).unwrap();
                }
            });
            let mut stream = std::io::stdout().lock();
            for _ in 0..512 {
                stream.write_all(&[b'o'; 8192]).unwrap();
            }
            stderr.join().unwrap();
        }
        Some("tree") => {
            let mut command = std::process::Command::new(std::env::current_exe().unwrap());
            command.arg("sleep");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            let mut child = command.spawn().unwrap();
            std::fs::write(&args[1], child.id().to_string()).unwrap();
            let _ = child.wait();
        }
        _ => std::thread::sleep(Duration::from_secs(120)),
    }
}

/// 输出当前进程继承的 Windows Job 资源额度。参数：无。返回：无；查询失败令辅助程序退出失败。
#[cfg(windows)]
fn report_limits() {
    use windows::Win32::System::JobObjects::*;
    let mut memory = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let mut cpu = JOBOBJECT_CPU_RATE_CONTROL_INFORMATION::default();
    unsafe {
        QueryInformationJobObject(
            None,
            JobObjectExtendedLimitInformation,
            &mut memory as *mut _ as *mut _,
            std::mem::size_of_val(&memory) as u32,
            None,
        )
        .unwrap();
        QueryInformationJobObject(
            None,
            JobObjectCpuRateControlInformation,
            &mut cpu as *mut _ as *mut _,
            std::mem::size_of_val(&cpu) as u32,
            None,
        )
        .unwrap();
        println!(
            "{}",
            serde_json::json!({"memory": memory.JobMemoryLimit, "cpu": cpu.Anonymous.CpuRate,
            "memory_enabled": memory.BasicLimitInformation.LimitFlags.contains(JOB_OBJECT_LIMIT_JOB_MEMORY),
            "cpu_enabled": cpu.ControlFlags == JOB_OBJECT_CPU_RATE_CONTROL_ENABLE | JOB_OBJECT_CPU_RATE_CONTROL_HARD_CAP})
        );
    }
}
