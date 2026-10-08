//! 有序 unit 解析、指令校验及命令展开。
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, time::Duration};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    #[default]
    Simple,
    Exec,
    Oneshot,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Restart {
    #[default]
    No,
    Always,
    OnFailure,
    OnSuccess,
}

/// 指令的来源位置，按合并时的应用顺序保留。
#[derive(Debug, Clone)]
pub struct Origin {
    pub source: String,
    pub line: usize,
    pub section: String,
    pub key: String,
}

/// 一个完成合并及验证的服务定义；来源顺序仅在内存中保留，不暴露环境值。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Unit {
    pub name: String,
    pub description: String,
    pub requires: Vec<String>,
    pub wants: Vec<String>,
    pub after: Vec<String>,
    pub before: Vec<String>,
    pub service_type: ServiceType,
    pub exec_start: Vec<Vec<String>>,
    pub exec_stop: Vec<Vec<String>>,
    pub working_directory: Option<String>,
    pub environment: BTreeMap<String, String>,
    #[serde(default)]
    pub health: crate::health::HealthConfig,
    pub restart: Restart,
    pub restart_sec: Duration,
    pub timeout_start: Option<Duration>,
    #[serde(skip)]
    timeout_start_set: bool,
    pub timeout_stop: Option<Duration>,
    pub remain_after_exit: bool,
    pub limit_interval: Duration,
    pub limit_burst: usize,
    pub wanted_by: Vec<String>,
    #[serde(skip)]
    pub origins: Vec<Origin>,
}

impl Unit {
    /// 创建默认定义。参数：name 为 unit 名。返回：尚未验证的定义。
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            requires: vec![],
            wants: vec![],
            after: vec![],
            before: vec![],
            service_type: ServiceType::Simple,
            exec_start: vec![],
            exec_stop: vec![],
            working_directory: None,
            environment: BTreeMap::new(),
            health: crate::health::HealthConfig::default(),
            restart: Restart::No,
            restart_sec: Duration::from_millis(100),
            timeout_start: Some(Duration::from_secs(90)),
            timeout_start_set: false,
            timeout_stop: Some(Duration::from_secs(90)),
            remain_after_exit: false,
            limit_interval: Duration::from_secs(10),
            limit_burst: 5,
            wanted_by: vec![],
            origins: vec![],
        }
    }
}

/// 验证本实现支持的普通名称，防止路径逃逸及 Windows 文件别名。
/// 参数：name 为文件名。返回：合法名称或诊断。
pub fn validate_name(name: &str) -> Result<()> {
    let base = name
        .strip_suffix(".service")
        .ok_or_else(|| Error::Config(format!("仅支持 .service：{name}")))?;
    if base.is_empty()
        || name.len() > 200
        || !base
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        || base.starts_with('.')
        || base.ends_with('.')
    {
        return Err(Error::Config(format!("不支持的 unit 名称：{name}")));
    }
    let device = base.split('.').next().unwrap_or("").to_ascii_uppercase();
    if matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (device.len() == 4
            && (device.starts_with("COM") || device.starts_with("LPT"))
            && matches!(device.as_bytes()[3], b'1'..=b'9'))
    {
        return Err(Error::Config(format!("Windows 保留名称：{name}")));
    }
    Ok(())
}

/// 判断 Windows 驱动器或 UNC 绝对路径。参数：value 为路径。返回：是否绝对。
pub fn windows_absolute(value: &str) -> bool {
    let b = value.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && matches!(b[2], b'/' | b'\\'))
        || value.starts_with("\\\\")
        || value.starts_with("//")
}

/// 将逻辑行拆为参数并解码 systemd 转义。参数：input 为字符串。返回：参数列表。
pub fn words(input: &str) -> Result<Vec<String>> {
    let mut chars = input.chars().peekable();
    let mut result = vec![];
    let mut item = Vec::new();
    let mut quote = None;
    let mut started = false;
    // 引号仅能包围完整词；转义在有无引号时都使用相同规则。
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                started = true;
                let escaped = chars
                    .next()
                    .ok_or_else(|| Error::Config("不完整的转义".into()))?;
                let decoded = match escaped {
                    'a' => '\u{7}',
                    'b' => '\u{8}',
                    'f' => '\u{c}',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'v' => '\u{b}',
                    's' => ' ',
                    '\\' => '\\',
                    '\'' => '\'',
                    '"' => '"',
                    'x' => {
                        let hex: String = chars.by_ref().take(2).collect();
                        if hex.len() != 2 {
                            return Err(Error::Config("不完整的字节转义".into()));
                        }
                        let byte = u8::from_str_radix(&hex, 16).map_err(crate::operation)?;
                        if byte == 0 {
                            return Err(Error::Config("不允许 NUL".into()));
                        }
                        item.push(byte);
                        continue;
                    }
                    'u' | 'U' => {
                        let count = if escaped == 'u' { 4 } else { 8 };
                        let hex: String = chars.by_ref().take(count).collect();
                        if hex.len() != count {
                            return Err(Error::Config("不完整的数字转义".into()));
                        }
                        let code = u32::from_str_radix(&hex, 16).map_err(crate::operation)?;
                        char::from_u32(code)
                            .ok_or_else(|| Error::Config("非法 Unicode 转义".into()))?
                    }
                    '0'..='7' => {
                        let mut octal = escaped.to_string();
                        octal.extend(chars.by_ref().take(2));
                        if octal.len() != 3 {
                            return Err(Error::Config("八进制转义必须三位".into()));
                        }
                        let byte = u8::from_str_radix(&octal, 8).map_err(crate::operation)?;
                        if byte == 0 {
                            return Err(Error::Config("不允许 NUL".into()));
                        }
                        item.push(byte);
                        continue;
                    }
                    _ => return Err(Error::Config(format!("未知转义：\\{escaped}"))),
                };
                if decoded == '\0' {
                    return Err(Error::Config("不允许 NUL".into()));
                }
                item.extend_from_slice(decoded.encode_utf8(&mut [0u8; 4]).as_bytes());
            }
            '\'' | '"' if quote == Some(ch) => {
                quote = None;
                if chars.peek().is_some_and(|c| !c.is_ascii_whitespace()) {
                    return Err(Error::Config("闭引号后必须是空白".into()));
                }
            }
            '\'' | '"' if quote.is_none() => {
                if started {
                    return Err(Error::Config("引号必须位于词首".into()));
                }
                quote = Some(ch);
                started = true;
            }
            c if c.is_ascii_whitespace() && quote.is_none() => {
                if started {
                    result.push(decoded_word(std::mem::take(&mut item))?);
                    started = false;
                }
            }
            c => {
                started = true;
                if c == '\0' {
                    return Err(Error::Config("不允许 NUL".into()));
                }
                item.extend_from_slice(c.encode_utf8(&mut [0u8; 4]).as_bytes());
            }
        }
    }
    if quote.is_some() {
        return Err(Error::Config("引号未闭合".into()));
    }
    if started {
        result.push(decoded_word(item)?);
    }
    Ok(result)
}

/// 校验字节转义组合成有效 Unicode 参数。参数：bytes 为解码结果。返回：UTF-8 参数。
fn decoded_word(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).map_err(|_| Error::Config("转义后的参数不是有效 UTF-8".into()))
}

/// 解析布尔值。参数：value 为配置值。返回：布尔值或诊断。
fn boolean(value: &str) -> Result<bool> {
    match value {
        "yes" | "true" | "on" | "1" => Ok(true),
        "no" | "false" | "off" | "0" => Ok(false),
        _ => Err(Error::Config(format!("非法布尔值：{value}"))),
    }
}

/// 解析可组合时间跨度。参数：input 为时间值。返回：时长；infinity 返回 None。
pub fn timespan(input: &str) -> Result<Option<Duration>> {
    if input == "infinity" {
        return Ok(None);
    }
    let mut rest = input.trim();
    let mut seconds = 0.0;
    if rest.is_empty() {
        return Err(Error::Config("空时间值".into()));
    }
    // 每段是非负十进制数和可选单位，避免 NaN 与溢出进入 Duration。
    while !rest.is_empty() {
        let end = rest
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(rest.len());
        if end == 0 {
            return Err(Error::Config(format!("非法时间：{input}")));
        }
        let number: f64 = rest[..end].parse().map_err(crate::operation)?;
        rest = rest[end..].trim_start();
        let end = rest
            .find(|c: char| !c.is_alphabetic())
            .unwrap_or(rest.len());
        let unit = &rest[..end];
        let factor = match unit {
            "" | "s" | "sec" | "second" | "seconds" => 1.0,
            "us" | "usec" | "µs" => 0.000001,
            "ms" | "msec" => 0.001,
            "m" | "min" | "minute" | "minutes" => 60.0,
            "h" | "hr" | "hour" | "hours" => 3600.0,
            "d" | "day" | "days" => 86400.0,
            "w" | "week" | "weeks" => 604800.0,
            "month" | "months" | "M" => 2629800.0,
            "y" | "year" | "years" => 31557600.0,
            _ => return Err(Error::Config(format!("未知时间单位：{unit}"))),
        };
        seconds += number * factor;
        rest = rest[end..].trim_start();
    }
    Duration::try_from_secs_f64(seconds)
        .map(Some)
        .map_err(crate::operation)
}

/// 展开已支持的 specifier。参数：input、name 为值和 unit 名。返回：展开字符串。
fn specifiers(input: &str, name: &str) -> Result<String> {
    let mut out = String::new();
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('%') => out.push('%'),
            Some('n') => out.push_str(name),
            other => return Err(Error::Config(format!("不支持的 specifier：%{other:?}"))),
        }
    }
    Ok(out)
}

/// 展开命令环境，不经过 shell。参数：args、env 为参数和环境；main_pid 仅用于停止命令。
/// 返回：可交给平台层的 argv。
pub fn expand_command(
    args: &[String],
    env: &BTreeMap<String, String>,
    main_pid: Option<u32>,
) -> Result<Vec<String>> {
    let mut vars = env.clone();
    if let Some(pid) = main_pid {
        vars.insert("MAINPID".into(), pid.to_string());
    } else {
        vars.remove("MAINPID");
    }
    let mut out = vec![];
    for arg in args {
        if arg.starts_with('$')
            && !arg.starts_with("${")
            && arg.len() > 1
            && arg[1..]
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
        {
            if let Some(value) = lookup(&vars, &arg[1..]) {
                out.extend(environment_words(value));
            }
            continue;
        }
        let mut chars = arg.chars().peekable();
        let mut value = String::new();
        while let Some(ch) = chars.next() {
            if ch != '$' {
                value.push(ch);
                continue;
            }
            match chars.next() {
                Some('$') => value.push('$'),
                Some('{') => {
                    let mut key = String::new();
                    let mut closed = false;
                    for c in chars.by_ref() {
                        if c == '}' {
                            closed = true;
                            break;
                        }
                        key.push(c);
                    }
                    if !closed {
                        return Err(Error::Config("环境变量缺少 }".into()));
                    }
                    value.push_str(lookup(&vars, &key).unwrap_or(""));
                }
                _ => return Err(Error::Config("仅支持 ${VAR}、独立 $VAR 和 $$".into())),
            }
        }
        out.push(value);
    }
    if out.first().is_none_or(|s| !windows_absolute(s)) {
        return Err(Error::Config("可执行文件必须是 Windows 绝对路径".into()));
    }
    Ok(out)
}

/// 按 Windows 环境名规则查值。参数：vars/key 为环境和名称。返回：值或 None。
fn lookup<'a>(vars: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    vars.get(key)
        .or_else(|| {
            vars.iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(key))
                .map(|(_, v)| v)
        })
        .map(String::as_str)
}

/// 按 systemd 的 $VAR 规则分词；不再次执行 C 风格转义，允许不平衡引号。
/// 参数：input 为环境值。返回：展开参数列表。
fn environment_words(input: &str) -> Vec<String> {
    let mut out = vec![];
    let mut item = String::new();
    let mut started = false;
    let mut quote = None;
    let mut escaped = false;
    for ch in input.chars() {
        if escaped {
            item.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            started = true;
            escaped = true;
            continue;
        }
        if quote == Some(ch) {
            quote = None;
            continue;
        }
        if quote.is_none() && matches!(ch, '\'' | '"') {
            started = true;
            quote = Some(ch);
            continue;
        }
        if quote.is_none() && ch.is_ascii_whitespace() {
            if started {
                out.push(std::mem::take(&mut item));
                started = false;
            }
        } else {
            started = true;
            item.push(ch);
        }
    }
    if started {
        out.push(item);
    }
    out
}

/// 合并一个来源的逻辑行。参数：unit 为累积定义，source、text 为文件名和正文。
/// 返回：修改成功或带来源诊断。
pub fn merge(unit: &mut Unit, source: &str, text: &str) -> Result<()> {
    if let Some(offset) = text.find('\0') {
        let line = text[..offset].bytes().filter(|b| *b == b'\n').count() + 1;
        return Err(Error::Config(format!("{source}:{line}: 配置包含 NUL")));
    }
    let mut section = String::new();
    let mut logical = String::new();
    let mut start = 0;
    for (index, physical) in text.trim_start_matches('\u{feff}').lines().enumerate() {
        let line = physical.trim_ascii();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if logical.is_empty() {
            start = index + 1;
        }
        if let Some(part) = line.strip_suffix('\\') {
            logical.push_str(part);
            logical.push(' ');
            continue;
        }
        logical.push_str(line);
        let line = std::mem::take(&mut logical);
        let result = if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].into();
            if matches!(section.as_str(), "Unit" | "Service" | "Install")
                || section.starts_with("X-")
            {
                Ok(())
            } else {
                Err(Error::Config(format!("不支持的节 [{section}]")))
            }
        } else if section.starts_with("X-") {
            Ok(())
        } else if let Some((key, value)) = line.split_once('=') {
            let result = apply(unit, &section, key.trim_ascii(), value.trim_ascii());
            if result.is_ok() && !key.trim_ascii().starts_with("X-") {
                unit.origins.push(Origin {
                    source: source.into(),
                    line: start,
                    section: section.clone(),
                    key: key.trim_ascii().into(),
                });
            }
            result
        } else {
            Err(Error::Config("必须是 Key=Value".into()))
        };
        result.map_err(|e| {
            // 不回显原始值，避免 Environment 等指令中的敏感内容进入日志。
            let directive = line
                .split_once('=')
                .map(|(key, _)| key.trim_ascii())
                .unwrap_or("节或语法");
            Error::Config(format!("{source}:{start} [{section}] {directive}: {e}"))
        })?;
    }
    if !logical.is_empty() {
        return Err(Error::Config(format!("{source}:{start} 未完成的续行")));
    }
    Ok(())
}

/// 应用一条指令。参数：unit 为定义，section/key/value 为已拆分指令。返回：结果。
fn apply(unit: &mut Unit, section: &str, key: &str, value: &str) -> Result<()> {
    if key.starts_with("X-") {
        return Ok(());
    }
    let defaults = Unit::new(&unit.name);
    match (section, key) {
        ("Service", "HealthType") => unit.health.kind = value.into(),
        ("Service", "HealthPort") => {
            unit.health.port = if value.is_empty() {
                0
            } else {
                value.parse().map_err(crate::operation)?
            }
        }
        ("Service", "HealthUrl") => unit.health.url = value.into(),
        ("Service", "HealthTimeoutSec" | "HealthIntervalSec") => {
            let duration = if value.is_empty() {
                if key == "HealthTimeoutSec" {
                    Duration::from_secs(1)
                } else {
                    Duration::from_secs(10)
                }
            } else {
                timespan(value)?.ok_or_else(|| Error::Config("健康检查时间必须有限".into()))?
            };
            if key == "HealthTimeoutSec" {
                unit.health.timeout = duration;
            } else {
                unit.health.interval = duration;
            }
        }
        ("Unit", "Description") => unit.description = specifiers(value, &unit.name)?,
        ("Unit", "Requires" | "Wants" | "After" | "Before") => {
            let values = words(value)?;
            for name in &values {
                validate_name(name)?;
            }
            let list = match key {
                "Requires" => &mut unit.requires,
                "Wants" => &mut unit.wants,
                "After" => &mut unit.after,
                _ => &mut unit.before,
            };
            // systemd 的依赖列表不能通过空赋值删除已添加的依赖。
            for value in values {
                if !list.contains(&value) {
                    list.push(value);
                }
            }
        }
        ("Unit", "StartLimitIntervalSec") => {
            unit.limit_interval = if value.is_empty() {
                defaults.limit_interval
            } else {
                timespan(value)?.ok_or_else(|| Error::Config("启动限流不支持 infinity".into()))?
            }
        }
        ("Unit", "StartLimitBurst") => {
            unit.limit_burst = if value.is_empty() {
                defaults.limit_burst
            } else {
                value.parse().map_err(crate::operation)?
            }
        }
        ("Service", "Type") => {
            unit.service_type = match value {
                "" | "simple" => ServiceType::Simple,
                "exec" => ServiceType::Exec,
                "oneshot" => ServiceType::Oneshot,
                _ => return Err(Error::Config(format!("不支持 Type={value}"))),
            }
        }
        ("Service", "ExecStart" | "ExecStop") => {
            let list = if key == "ExecStart" {
                &mut unit.exec_start
            } else {
                &mut unit.exec_stop
            };
            if value.is_empty() {
                list.clear();
            } else {
                let args: Vec<_> = words(value)?
                    .iter()
                    .map(|s| specifiers(s, &unit.name))
                    .collect::<Result<_>>()?;
                if args.first().is_none_or(|s| !windows_absolute(s)) {
                    return Err(Error::Config(
                        "不支持命令前缀；可执行文件必须为 Windows 绝对路径".into(),
                    ));
                }
                list.push(args);
            }
        }
        ("Service", "WorkingDirectory") => {
            let value = specifiers(value, &unit.name)?;
            if !value.is_empty() && !windows_absolute(&value) {
                return Err(Error::Config("WorkingDirectory 必须为绝对路径".into()));
            }
            unit.working_directory = (!value.is_empty()).then_some(value);
        }
        ("Service", "Environment") => {
            if value.is_empty() {
                unit.environment.clear();
            }
            for assignment in words(value)? {
                let (key, val) = assignment
                    .split_once('=')
                    .ok_or_else(|| Error::Config("Environment 必须包含 NAME=value".into()))?;
                if key.is_empty()
                    || key.starts_with(|c: char| c.is_ascii_digit())
                    || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    return Err(Error::Config("非法环境变量名".into()));
                }
                let previous = unit
                    .environment
                    .keys()
                    .find(|name| name.eq_ignore_ascii_case(key))
                    .cloned();
                if let Some(previous) = previous {
                    unit.environment.remove(&previous);
                }
                unit.environment
                    .insert(key.into(), specifiers(val, &unit.name)?);
            }
        }
        ("Service", "Restart") => {
            unit.restart = match value {
                "" | "no" => Restart::No,
                "always" => Restart::Always,
                "on-failure" => Restart::OnFailure,
                "on-success" => Restart::OnSuccess,
                _ => return Err(Error::Config(format!("不支持 Restart={value}"))),
            }
        }
        ("Service", "RestartSec") => {
            unit.restart_sec = if value.is_empty() {
                defaults.restart_sec
            } else {
                timespan(value)?.ok_or_else(|| Error::Config("RestartSec 必须有限".into()))?
            }
        }
        ("Service", "TimeoutStartSec") => {
            unit.timeout_start_set = !value.is_empty();
            unit.timeout_start = if value.is_empty() {
                defaults.timeout_start
            } else {
                timespan(value)?
            };
        }
        ("Service", "TimeoutStopSec") => {
            unit.timeout_stop = if value.is_empty() {
                defaults.timeout_stop
            } else {
                timespan(value)?
            }
        }
        ("Service", "RemainAfterExit") => {
            unit.remain_after_exit = if value.is_empty() {
                false
            } else {
                boolean(value)?
            }
        }
        ("Install", "WantedBy") => {
            if value.is_empty() {
                unit.wanted_by.clear();
            }
            for target in words(value)? {
                if target != "multi-user.target" {
                    return Err(Error::Config(format!("不支持安装目标：{target}")));
                }
                if !unit.wanted_by.contains(&target) {
                    unit.wanted_by.push(target);
                }
            }
        }
        _ => return Err(Error::Config(format!("不支持的指令：[{section}] {key}"))),
    }
    Ok(())
}

/// 校验最终合并定义。参数：unit 为定义。返回：结果。
pub fn validate(unit: &Unit) -> Result<()> {
    validate_name(&unit.name)?;
    unit.health.validate()?;
    if unit.exec_start.is_empty() && !(unit.remain_after_exit && !unit.exec_stop.is_empty()) {
        return Err(invalid(unit, "ExecStart", "缺少 ExecStart"));
    }
    if unit.service_type != ServiceType::Oneshot && unit.exec_start.len() > 1 {
        return Err(invalid(
            unit,
            "ExecStart",
            "非 oneshot 只能有一个 ExecStart",
        ));
    }
    if unit.service_type == ServiceType::Oneshot
        && matches!(unit.restart, Restart::Always | Restart::OnSuccess)
    {
        return Err(invalid(
            unit,
            "Restart",
            "oneshot 不支持 Restart=always/on-success",
        ));
    }
    // 静态验证变量表达式，避免非法配置直到运行时才被发现。
    for (key, commands) in [
        ("ExecStart", &unit.exec_start),
        ("ExecStop", &unit.exec_stop),
    ] {
        for command in commands {
            expand_command(command, &unit.environment, Some(1))
                .map_err(|e| invalid(unit, key, &e.to_string()))?;
        }
    }
    Ok(())
}

/// 给最终语义错误附加最后赋值来源。参数：unit/key/message 为定义、指令和错误。返回：诊断。
fn invalid(unit: &Unit, key: &str, message: &str) -> Error {
    let location = unit
        .origins
        .iter()
        .rev()
        .find(|o| o.section == "Service" && o.key == key)
        .or_else(|| unit.origins.first());
    let source = location
        .map(|o| format!("{}:{}", o.source, o.line))
        .unwrap_or_else(|| unit.name.clone());
    Error::Config(format!("{source} [Service] {key}: {message}"))
}

/// 加载目录中所有 unit 及同名 drop-in。参数：directory 为 units 目录。返回：有序定义表。
pub fn load(directory: &Path) -> Result<BTreeMap<String, Unit>> {
    load_override(directory, None)
}

/// 加载候选配置，可在写盘前替换一份正文。参数：directory 为目录，replacement 为已校验的相对文件名及正文。返回：完整且已验证的定义表。
pub fn load_override(
    directory: &Path,
    replacement: Option<(&str, &str)>,
) -> Result<BTreeMap<String, Unit>> {
    let mut units = BTreeMap::new();
    let mut names = Vec::new();
    // 先收集磁盘名称，再补入新建文件；候选正文不会改变其他文件的校验规则。
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if name.ends_with(".d") {
            continue;
        }
        if !name.ends_with(".service") {
            return Err(Error::Config(format!("units 中存在不支持的文件：{name}")));
        }
        if !entry.file_type()?.is_file() {
            return Err(Error::Config(format!("不支持 unit 链接/目录：{name}")));
        }
        validate_name(&name)?;
        names.push(name);
    }
    if let Some((name, _)) = replacement {
        crate::desktop::validate_document_name(name)?;
        if !name.contains('/') && !names.contains(&name.to_string()) {
            names.push(name.into());
        }
    }
    names.sort();
    for name in names {
        if units
            .keys()
            .any(|key: &String| key.eq_ignore_ascii_case(&name))
        {
            return Err(Error::Config(format!("名称大小写冲突：{name}")));
        }
        let mut unit = Unit::new(&name);
        let path = directory.join(&name);
        let text = match replacement {
            Some((file, text)) if file == name => text.to_string(),
            _ => read_text(&path)?,
        };
        merge(&mut unit, &path.display().to_string(), &text)?;
        let drop_in = directory.join(format!("{name}.d"));
        if drop_in.exists() {
            if std::fs::symlink_metadata(&drop_in)?
                .file_type()
                .is_symlink()
            {
                return Err(Error::Config("不支持 drop-in 目录链接".into()));
            }
            let mut files = std::fs::read_dir(drop_in)?
                .map(|e| e.map(|e| e.path()))
                .collect::<std::io::Result<Vec<_>>>()?;
            if let Some((file, _)) = replacement {
                let path = directory.join(file);
                if path.parent() == Some(directory.join(format!("{name}.d")).as_path())
                    && !files.contains(&path)
                {
                    files.push(path);
                }
            }
            files.sort();
            for path in files {
                if path.extension().is_some_and(|e| e == "conf") {
                    if path.exists() && std::fs::symlink_metadata(&path)?.file_type().is_symlink() {
                        return Err(Error::Config("不支持 drop-in 链接".into()));
                    }
                    let relative = path
                        .strip_prefix(directory)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/");
                    let text = match replacement {
                        Some((file, text)) if file == relative => text.to_string(),
                        _ => read_text(&path)?,
                    };
                    merge(&mut unit, &path.display().to_string(), &text)?;
                }
            }
        } else if let Some((file, text)) = replacement
            && file.starts_with(&format!("{name}.d/"))
        {
            merge(&mut unit, &directory.join(file).display().to_string(), text)?;
        }
        if unit.service_type == ServiceType::Oneshot && !unit.timeout_start_set {
            unit.timeout_start = None;
        }
        validate(&unit)?;
        units.insert(name, unit);
    }
    Ok(units)
}

/// 读取 UTF-8 配置并附加读取/编码来源。参数：path 为文件。返回：正文。
fn read_text(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)
        .map_err(|e| Error::Config(format!("{}: 无法读取配置：{e}", path.display())))?;
    String::from_utf8(bytes).map_err(|e| {
        let offset = e.utf8_error().valid_up_to();
        let line = e.as_bytes()[..offset]
            .iter()
            .filter(|b| **b == b'\n')
            .count()
            + 1;
        Error::Config(format!("{}:{line}: 配置必须是 UTF-8", path.display()))
    })
}
