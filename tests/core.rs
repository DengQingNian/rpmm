//! 配置语义、依赖图和生命周期策略回归。
use rpmm::{
    config::{self, Restart, Unit},
    graph,
    policy::{StartLimiter, should_restart},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

/// 构造有效基础配置。参数：name 为名称。返回：定义。
fn unit(name: &str) -> Unit {
    let mut unit = Unit::new(name);
    config::merge(&mut unit, "fixture", "[Service]\nExecStart=C:/app.exe\n").unwrap();
    unit
}

/// 验证词法与 Windows 转义。参数：无。返回：无。
#[test]
fn quoting_and_escaping() {
    assert_eq!(
        config::words(r#""C:/Program Files/app.exe" 'one two' three\sfour "" C:\\bin\\app"#)
            .unwrap(),
        [
            "C:/Program Files/app.exe",
            "one two",
            "three four",
            "",
            "C:\\bin\\app"
        ]
    );
    assert!(config::words("word\"tail\"").is_err());
    assert!(config::words(r"C:\qbad").is_err());
    assert!(config::words("'unfinished").is_err());
    assert_eq!(
        config::words(r"\x41 \u4e2d \101").unwrap(),
        ["A", "中", "A"]
    );
}
/// 验证整行注释、重复节及续行。参数：无。返回：无。
#[test]
fn ordered_merge_and_continuation() {
    let mut unit = unit("app.service");
    config::merge(&mut unit, "main.service", "[Unit]\r\nDescription=包含 # 和 ;\r\nWants=a.service \\\r\n# 跳过\r\n; 跳过\r\n b.service\r\n[Service]\r\nEnvironment=ONE=1\r\n[Service]\r\nEnvironment=TWO=2\r\n").unwrap();
    assert_eq!(unit.wants, ["a.service", "b.service"]);
    assert_eq!(unit.description, "包含 # 和 ;");
    assert_eq!(unit.environment.len(), 2);
    config::merge(&mut unit, "override.conf", "[Service]\nExecStart=\nExecStart=C:/new.exe\nEnvironment=\nEnvironment=NEW=ok\n[Unit]\nWants=\n").unwrap();
    assert_eq!(unit.exec_start[0], ["C:/new.exe"]);
    assert_eq!(unit.environment.len(), 1);
    assert_eq!(unit.wants.len(), 2);
}
/// 验证来源诊断、严格拒绝和扩展忽略。参数：无。返回：无。
#[test]
fn strict_diagnostics_and_extensions() {
    let mut unit = unit("app.service");
    config::merge(
        &mut unit,
        "file",
        "[X-Metadata]\nWhatever=ok\n[Service]\nX-Custom=ok\n",
    )
    .unwrap();
    let error = config::merge(&mut unit, "file", "[Service]\nUser=someone\n")
        .unwrap_err()
        .to_string();
    assert!(error.contains("file:2"));
    assert!(error.contains("User"));
    assert!(config::merge(&mut unit, "file", "[Socket]\n").is_err());
    assert!(config::merge(&mut unit, "file", "[Unit]\nAfter=network.target\n").is_err());
}
/// 验证时间和布尔变体。参数：无。返回：无。
#[test]
fn durations_and_boolean() {
    assert_eq!(
        config::timespan("1min 2.5s 500ms").unwrap(),
        Some(Duration::from_secs(63))
    );
    assert_eq!(config::timespan("infinity").unwrap(), None);
    assert!(config::timespan("NaN").is_err());
    assert!(config::timespan("-1s").is_err());
    let mut unit = unit("app.service");
    for value in ["yes", "true", "on", "1"] {
        config::merge(
            &mut unit,
            "file",
            &format!("[Service]\nRemainAfterExit={value}\n"),
        )
        .unwrap();
        assert!(unit.remain_after_exit);
    }
}
/// 验证变量和百分号展开。参数：无。返回：无。
#[test]
fn expansion() {
    let mut unit = unit("app.service");
    config::merge(&mut unit, "file", "[Service]\nExecStart=\nExecStart=C:/app.exe ${ONE} $MANY $$ %n %%\nEnvironment=\"ONE=hello world\" \"MANY='one two' three\"\n").unwrap();
    assert_eq!(
        config::expand_command(&unit.exec_start[0], &unit.environment, None).unwrap(),
        [
            "C:/app.exe",
            "hello world",
            "one two",
            "three",
            "$",
            "app.service",
            "%"
        ]
    );
    assert_eq!(
        config::expand_command(
            &["C:/app.exe".into(), "$MAINPID".into()],
            &BTreeMap::new(),
            Some(42)
        )
        .unwrap(),
        ["C:/app.exe", "42"]
    );
    assert!(config::merge(&mut unit, "file", "[Service]\nExecStart=C:/app.exe %i\n").is_err());
}
/// 验证类型组合和命令数。参数：无。返回：无。
#[test]
fn service_validation() {
    let mut unit = unit("app.service");
    config::merge(&mut unit, "file", "[Service]\nExecStart=C:/second.exe\n").unwrap();
    assert!(config::validate(&unit).is_err());
    config::merge(&mut unit, "file", "[Service]\nType=oneshot\n").unwrap();
    config::validate(&unit).unwrap();
    config::merge(&mut unit, "file", "[Service]\nRestart=always\n").unwrap();
    assert!(config::validate(&unit).is_err());
    for name in [
        "../x.service",
        "a@b.service",
        "CON.service",
        "x.target",
        ".x.service",
    ] {
        assert!(config::validate_name(name).is_err());
    }
}
/// 验证排序不拉入 unit，需求不隐含排序。参数：无。返回：无。
#[test]
fn dependencies_and_order_are_orthogonal() {
    let mut a = unit("a.service");
    let b = unit("b.service");
    a.after.push(b.name.clone());
    let mut units = BTreeMap::from([(a.name.clone(), a.clone()), (b.name.clone(), b.clone())]);
    assert_eq!(
        graph::start_plan(&units, &[a.name.clone()]).unwrap().layers,
        vec![vec![a.name.clone()]]
    );
    a.requires.push(b.name.clone());
    a.after.clear();
    units.insert(a.name.clone(), a.clone());
    assert_eq!(
        graph::start_plan(&units, &[a.name.clone()])
            .unwrap()
            .layers
            .len(),
        1
    );
    a.after.push(b.name.clone());
    units.insert(a.name.clone(), a.clone());
    assert_eq!(
        graph::start_plan(&units, &[a.name.clone()]).unwrap().layers,
        vec![vec![b.name], vec![a.name]]
    );
}
/// 验证拉入循环、排序循环及缺失强弱依赖。参数：无。返回：无。
#[test]
fn cycles_and_missing_units() {
    let mut a = unit("a.service");
    let mut b = unit("b.service");
    a.requires.push(b.name.clone());
    b.requires.push(a.name.clone());
    let mut units = BTreeMap::from([(a.name.clone(), a.clone()), (b.name.clone(), b.clone())]);
    assert_eq!(
        graph::start_plan(&units, &[a.name.clone()]).unwrap().layers[0].len(),
        2
    );
    a.after.push(b.name.clone());
    b.after.push(a.name.clone());
    units.insert(a.name.clone(), a.clone());
    units.insert(b.name.clone(), b);
    assert!(graph::start_plan(&units, &[a.name.clone()]).is_err());
    a.requires = vec!["missing.service".into()];
    units.insert(a.name.clone(), a.clone());
    assert!(graph::start_plan(&units, &[a.name.clone()]).is_err());
    a.requires.clear();
    a.wants = vec!["missing.service".into()];
    units.insert(a.name.clone(), a.clone());
    assert_eq!(
        graph::start_plan(&units, &[a.name]).unwrap().warnings.len(),
        1
    );
}
/// 验证反向停止闭包与逆序。参数：无。返回：无。
#[test]
fn stop_propagation_and_reverse_order() {
    let mut a = unit("a.service");
    let b = unit("b.service");
    let mut c = unit("c.service");
    a.requires.push(b.name.clone());
    a.after.push(b.name.clone());
    c.wants.push(b.name.clone());
    let units = BTreeMap::from([
        (a.name.clone(), a.clone()),
        (b.name.clone(), b.clone()),
        (c.name.clone(), c),
    ]);
    let selected = graph::stop_set(&units, std::slice::from_ref(&b.name));
    assert_eq!(selected, BTreeSet::from([a.name.clone(), b.name.clone()]));
    let order = graph::order(&units, &selected)
        .unwrap()
        .into_iter()
        .rev()
        .flatten()
        .collect::<Vec<_>>();
    assert_eq!(order, [a.name, b.name]);
}
/// 验证重启判定及停止抑制。参数：无。返回：无。
#[test]
fn restart_policy() {
    assert!(should_restart(Restart::OnFailure, false, false));
    assert!(!should_restart(Restart::OnFailure, true, false));
    assert!(should_restart(Restart::OnSuccess, true, false));
    assert!(!should_restart(Restart::Always, false, true));
}
/// 验证滑动窗口和 reset。参数：无。返回：无。
#[test]
fn start_limit() {
    let mut limiter = StartLimiter::default();
    let now = Instant::now();
    let interval = Duration::from_secs(10);
    for _ in 0..5 {
        assert!(limiter.allow(now, interval, 5));
    }
    assert!(!limiter.allow(now, interval, 5));
    assert!(limiter.allow(now + interval, interval, 5));
    limiter.reset();
    assert!(limiter.allow(now + interval, interval, 5));
}
/// 验证协议往返及版本字段。参数：无。返回：无。
#[test]
fn protocol_roundtrip() {
    let req = rpmm::ipc::Request {
        version: 1,
        action: rpmm::ipc::Action::Start {
            unit: "app.service".into(),
        },
    };
    let encoded = serde_json::to_string(&req).unwrap();
    let decoded: rpmm::ipc::Request = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded.version, 1);
    assert!(
        serde_json::from_str::<rpmm::ipc::Request>(
            r#"{"version":1,"action":"start","unit":"x.service","unknown":true}"#
        )
        .is_err()
    );
}
/// 验证 Windows 编码空参数、引号和尾部反斜杠。参数：无。返回：无。
#[test]
fn windows_command_encoding() {
    assert_eq!(
        rpmm::platform::command_line(&[
            "".into(),
            "a b".into(),
            "x\"y".into(),
            "C:\\tail\\".into()
        ]),
        "\"\" \"a b\" \"x\\\"y\" \"C:\\tail\\\\\""
    );
}

/// 验证配置诊断不泄露环境值。参数：无。返回：无。
#[test]
fn diagnostics_redact_environment_values() {
    let mut unit = unit("a.service");
    let error = config::merge(
        &mut unit,
        "secrets.conf",
        "[Service]\nEnvironment=TOP_SECRET_WITHOUT_EQUALS\n",
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("Environment"));
    assert!(error.contains("secrets.conf:2"));
    assert!(!error.contains("TOP_SECRET"));
}

/// 验证最终验证错误保留 drop-in 来源，且环境覆盖符合 Windows 大小写规则。参数：无。返回：无。
#[test]
fn origins_and_windows_environment() {
    let mut unit = unit("app.service");
    config::merge(
        &mut unit,
        "10.conf",
        "[Service]\nEnvironment=foo=old\nEnvironment=FOO=new\nExecStart=C:/second.exe\n",
    )
    .unwrap();
    assert_eq!(unit.environment.len(), 1);
    assert_eq!(
        config::expand_command(
            &["C:/app.exe".into(), "${foo}".into()],
            &unit.environment,
            None
        )
        .unwrap()[1],
        "new"
    );
    let error = config::validate(&unit).unwrap_err().to_string();
    assert!(error.contains("10.conf:4"));
    assert_eq!(unit.origins.last().unwrap().key, "ExecStart");
    let env = BTreeMap::from([
        ("ARGS".into(), r"'one two' three\ four \t".into()),
        ("PATH_ARG".into(), r"C:\Program Files\App".into()),
    ]);
    assert_eq!(
        config::expand_command(
            &["C:/app.exe".into(), "$ARGS".into(), "${PATH_ARG}".into()],
            &env,
            None
        )
        .unwrap(),
        [
            "C:/app.exe",
            "one two",
            "three four",
            "t",
            r"C:\Program Files\App"
        ]
    );
}

/// 验证字节转义组合成 UTF-8，不被逐字节错误映射为 Unicode 码点。参数：无。返回：无。
#[test]
fn byte_escapes_and_unicode_whitespace() {
    assert_eq!(
        config::words(r"\xE4\xB8\xAD \344\270\255").unwrap(),
        ["中", "中"]
    );
    assert!(config::words(r"\xFF").is_err());
    assert!(config::words(r"\000").is_err());
    assert_eq!(config::words("a\u{a0}b").unwrap(), ["a\u{a0}b"]);
}
