//! Windows 绝对路径书写形式的识别与归一化。
//!
//! 配置允许 `C:\xx\yy`、`C:/xx/yy`、`C:\\xx\\yy`、`/c/xx/yy`、
//! `/cygdrive/c/xx/yy`、`\\server\share\yy` 等写法，统一归一化为 `C:\xx\yy`，
//! 便于直接交给 Windows API，也便于比较和排错。

use std::path::{Path, PathBuf};

/// 识别路径开头需要的最大字符数：`/cygdrive/c/` 共 12 个字符。
const SAMPLE: usize = 12;

/// 判断字符是否为 Windows 路径分隔符。参数：ch 为字符。返回：是否为分隔符。
fn separator(ch: char) -> bool {
    matches!(ch, '/' | '\\')
}

/// 判断文本是否以指定 ASCII 前缀开头。参数：text 为文本，prefix 为 ASCII 前缀。返回：是否匹配（忽略大小写）。
fn starts_with_ascii(text: &str, prefix: &str) -> bool {
    text.as_bytes()
        .get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix.as_bytes()))
}

/// 识别扩展长度或设备路径前缀。参数：text 为路径。返回：前缀长度；不是该形式返回 None。
/// `\\\\?\\`、`\\\\.\\` 及其正斜杠写法整体保留：其中重复分隔符可能有字面含义，不能折叠。
fn extended_prefix(text: &str) -> Option<usize> {
    let mut chars = text.chars();
    match (chars.next(), chars.next(), chars.next(), chars.next()) {
        (Some(a), Some(b), Some(c), Some(d))
            if separator(a) && separator(b) && matches!(c, '?' | '.') && separator(d) =>
        {
            Some(4)
        }
        _ => None,
    }
}

/// 解析 MSYS/Cygwin 风格的盘符前缀。参数：rest 为开头分隔符之后的文本。返回：大写盘符及剩余文本。
fn msys_drive(rest: &str) -> Option<(char, &str)> {
    let rest = if starts_with_ascii(rest, "cygdrive/") {
        &rest["cygdrive/".len()..]
    } else {
        rest
    };
    // 盘符必须是单个 ASCII 字母，避免把 /usr/bin 之类的路径当作盘符。
    if !rest.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) {
        return None;
    }
    Some((rest.chars().next()?.to_ascii_uppercase(), &rest[1..]))
}

/// 把盘符之后的分隔符统一为反斜杠并折叠重复分隔符。参数：rest 为 `:` 之后的文本。返回：以 `\\` 开头的主体；根目录为空串。
fn fold(rest: &str) -> String {
    let mut out = String::new();
    let mut previous = false;
    for ch in rest.chars() {
        if separator(ch) {
            if !previous {
                out.push('\\');
                previous = true;
            }
            continue;
        }
        previous = false;
        out.push(ch);
    }
    while out.ends_with('\\') {
        out.pop();
    }
    out
}

/// 组装盘符路径。参数：drive 为盘符，rest 为 `:` 之后的文本。返回：规范路径。
fn drive_path(drive: char, rest: &str) -> String {
    let body = fold(rest);
    if body.is_empty() {
        format!("{drive}:\\")
    } else {
        format!("{drive}:{body}")
    }
}

/// 归一化网络路径。参数：text 为以两个分隔符开头的文本。返回：`\\\\server\\share` 形式；缺少主机名返回 None。
fn network_path(text: &str) -> Option<String> {
    let parts: Vec<&str> = text
        .split(separator)
        .filter(|part| !part.is_empty())
        .collect();
    (!parts.is_empty()).then(|| format!("\\\\{}", parts.join("\\")))
}

/// 归一化 MSYS/Cygwin 盘符路径。参数：text 为 `/c/...` 或 `/cygdrive/c/...`。返回：规范路径；不是该形式返回 None。
fn msys_path(text: &str) -> Option<String> {
    let (drive, rest) = msys_drive(text.strip_prefix('/')?)?;
    // 必须写成 /c/xx 形式：单独一个 /c 更像命令开关，不能当作 C 盘根目录。
    rest.starts_with('/').then(|| drive_path(drive, rest))
}

/// 判断字符流在指定位置是否以受支持的 Windows 绝对路径开头。参数：chars 为整行字符，index 为起点，
/// word_start 为是否位于参数开头。返回：是否为路径起点。
/// `//server/share` 只在参数开头识别，避免把 `=//cdn/x` 这类 URL 值改写成网络路径。
pub fn starts_path(chars: &[char], index: usize, word_start: bool) -> bool {
    let sample: Vec<char> = chars.iter().skip(index).take(SAMPLE).copied().collect();
    let at = |offset: usize| sample.get(offset).copied();
    match at(0) {
        Some(c) if c.is_ascii_alphabetic() => at(1) == Some(':') && at(2).is_some_and(separator),
        Some('/') => (word_start && at(1) == Some('/')) || msys_sample(&sample),
        Some('\\') => at(1) == Some('\\'),
        _ => false,
    }
}

/// 判断参数开头是否为 MSYS/Cygwin 盘符路径。参数：sample 为参数开头样本。返回：是否为 `/c/...` 或 `/cygdrive/c/...`。
fn msys_sample(sample: &[char]) -> bool {
    let text: String = sample.iter().collect();
    text.strip_prefix('/')
        .and_then(msys_drive)
        .is_some_and(|(_, rest)| rest.starts_with('/'))
}

/// 归一化 Windows 绝对路径的书写形式。参数：value 为配置或命令中的路径文本。
/// 返回：`C:\\xx\\yy` 形式；不是受支持的绝对路径时返回 None（无盘符、相对路径和 Linux 路径都拒绝）。
pub fn normalize(value: &str) -> Option<String> {
    let first = value.chars().next()?;
    // 扩展长度与设备路径保留尾部原样，只统一分隔符方向。
    if extended_prefix(value).is_some() {
        return Some(value.replace('/', "\\"));
    }
    if separator(first) {
        // 两个前导分隔符是网络路径，单个则是 MSYS 盘符路径 /c/xx，也可能是被拒绝的根相对路径。
        if value.chars().nth(1).is_some_and(separator) {
            return network_path(value);
        }
        return msys_path(value);
    }
    if !first.is_ascii_alphabetic() {
        return None;
    }
    let rest = value[first.len_utf8()..].strip_prefix(':')?;
    separator(rest.chars().next()?).then(|| drive_path(first.to_ascii_uppercase(), rest))
}

/// 归一化用户输入或已保存的目录。参数：value 为目录路径。返回：`C:\...` 形式；其他形式（含相对路径）原样保留，首尾空白忽略。
pub fn normalize_path(value: &Path) -> PathBuf {
    value
        .to_str()
        .map(str::trim)
        .and_then(normalize)
        .map(PathBuf::from)
        .unwrap_or_else(|| value.to_path_buf())
}
