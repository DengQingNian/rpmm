//! Windows 集成测试使用的普通控制台程序，仅 test-fixtures feature 构建。
use std::{io::Write, time::Duration};
/// 执行测试模式。参数：命令行模式和参数。返回：程序退出码或持续运行。
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("args") => println!("{}", serde_json::to_string(&args[1..]).unwrap()),
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
