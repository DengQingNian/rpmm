//! 开机自启命令行编码、权限诊断与注册状态的回归验证。
use std::path::{Path, PathBuf};

/// 验证服务命令行编码含空格、中文和尾部反斜杠的路径并可反解。
#[test]
fn command_line_roundtrip() {
    let executable = Path::new(r"C:\Program Files\rpmm 管理器\rpmm.exe");
    let arguments = vec!["autostart".to_string(), "run".to_string()];
    for root in [
        r"C:\ProgramData\rpmm",
        r"C:\ProgramData\rpmm 数据\",
        r"D:\服务\rpmm",
    ] {
        let line = rpmm::service::command_line(executable, &arguments, Path::new(root));
        assert!(line.contains("autostart"), "{line}");
        assert_eq!(
            rpmm::service::root_from_command_line(&line),
            Some(PathBuf::from(root)),
            "{line}"
        );
    }
}

/// 验证缺少 --root 时不猜测数据目录。参数：无。返回：无。
#[test]
fn command_line_without_root_is_rejected() {
    assert!(rpmm::service::root_from_command_line(r#""C:\rpmm.exe" autostart run"#).is_none());
    assert!(rpmm::service::root_from_command_line("--root").is_none());
}

/// 验证用户配置目录会被提示改用机器级目录。参数：无。返回：无。
#[test]
fn profile_paths_are_warned() {
    let profile = std::env::var_os("USERPROFILE").expect("测试环境需要 USERPROFILE");
    let inside = PathBuf::from(profile).join("rpmm");
    let warning = rpmm::service::profile_warning(&inside).expect("应提示用户配置目录");
    assert!(warning.contains("ProgramData"), "{warning}");
    assert!(rpmm::service::profile_warning(Path::new(r"C:\ProgramData\rpmm")).is_none());
}

/// 验证未提升权限时注册被拒绝且不留下半成品登记。参数：无。返回：无。
/// 提升权限的会话下跳过，避免在真实机器上注册系统服务。
#[test]
fn install_requires_elevation() {
    if rpmm::service::is_elevated().unwrap() {
        return;
    }
    let error = rpmm::service::install(&rpmm::service::InstallOptions {
        executable: PathBuf::from(r"C:\ProgramData\rpmm\rpmm.exe"),
        arguments: vec!["autostart".into(), "run".into()],
        root: PathBuf::from(r"C:\ProgramData\rpmm"),
        account: None,
        password: None,
    })
    .unwrap_err()
    .to_string();
    assert!(error.contains("管理员"), "{error}");
    // 未注册时状态必须明确报告，不能把失败当成已登记。
    assert!(!rpmm::service::registration().unwrap().installed);
}

/// 验证未注册时状态自洽：installed 与 state 一致，且运行标志只在 running 时为真。参数：无。返回：无。
#[test]
fn registration_is_self_consistent() {
    let registration = rpmm::service::registration().unwrap();
    assert_eq!(registration.installed, registration.state != "absent");
    assert_eq!(registration.running(), registration.state == "running");
    if !registration.installed {
        assert!(registration.pid.is_none());
        assert!(registration.root.is_none());
    }
}
