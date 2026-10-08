//! 桌面应用的用户设置、配置文档及安全文件操作。
use crate::{Error, Result, config, manager::atomic_write};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const MAX_DOCUMENT: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Preferences {
    pub start_hidden: bool,
    pub refresh_ms: u64,
}

impl Default for Preferences {
    /// 提供首次运行的设置。参数：无。返回：默认设置。
    fn default() -> Self {
        Self {
            start_hidden: false,
            refresh_ms: 1000,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Document {
    pub name: String,
    pub text: String,
}

/// 获取当前用户的数据目录。参数：无。返回：桌面端和 CLI 共用的默认路径。
pub fn default_root() -> PathBuf {
    PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_else(|| ".".into())).join("rpmm")
}

/// 编码登录启动命令，保留含空格及尾部反斜杠的路径。参数：executable/root 为程序和数据目录。返回：Windows 命令行。
pub fn autostart_command(executable: &Path, root: &Path) -> String {
    crate::platform::command_line(&[
        executable.display().to_string(),
        "--hidden".into(),
        "--root".into(),
        root.display().to_string(),
    ])
}

#[cfg(windows)]
pub mod startup {
    //! 仅当前用户的登录启动登记与事务回滚，不写入系统级注册表。
    use crate::Result;
    use windows_registry::{CURRENT_USER, Key};
    const RUN: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
    const APPROVED: &str =
        "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run";
    const NAME: &str = "rpmm-desktop";

    pub struct Snapshot {
        command: Option<String>,
        approval: Option<Vec<u8>>,
    }

    /// 判断注册表值不存在。参数：code 为 HRESULT。返回：是否缺失。
    fn missing(code: i32) -> bool {
        code == 0x80070002_u32 as i32
    }

    /// 读取当前用户启动登记快照。参数：无。返回：命令与任务管理器批准状态。
    pub fn snapshot() -> Result<Snapshot> {
        let command = match CURRENT_USER.open(RUN).and_then(|key| key.get_string(NAME)) {
            Ok(value) => Some(value),
            Err(error) if missing(error.code().0) => None,
            Err(error) => return Err(crate::operation(error)),
        };
        let approval = match CURRENT_USER
            .open(APPROVED)
            .and_then(|key| key.get_value(NAME))
        {
            Ok(value) => Some(value.as_ref().to_vec()),
            Err(error) if missing(error.code().0) => None,
            Err(error) => return Err(crate::operation(error)),
        };
        Ok(Snapshot { command, approval })
    }

    /// 查询真实的当前用户自启动状态。参数：无。返回：是否登记且未被任务管理器禁用。
    pub fn is_enabled() -> Result<bool> {
        let value = snapshot()?;
        Ok(value.command.is_some()
            && !value
                .approval
                .as_ref()
                .is_some_and(|bytes| matches!(bytes.first(), Some(3 | 7))))
    }

    /// 删除可能尚未登记的值。参数：key 为键，name 为值名。返回：删除结果。
    fn delete_optional(key: &Key, name: &str) -> Result<()> {
        match key.remove_value(name) {
            Ok(()) => Ok(()),
            Err(error) if missing(error.code().0) => Ok(()),
            Err(error) => Err(crate::operation(error)),
        }
    }

    /// 更新当前用户登录启动。参数：enabled 为目标状态，command 为正确编码的命令。返回：写入结果。
    pub fn set_enabled(enabled: bool, command: &str) -> Result<()> {
        let key = CURRENT_USER.create(RUN).map_err(crate::operation)?;
        if enabled {
            key.set_string(NAME, command).map_err(crate::operation)?;
            // 仅更新已经存在的批准值；用户重新启用时清除任务管理器的禁用标记。
            if let Some(mut approval) = snapshot()?.approval {
                if let Some(status) = approval.first_mut() {
                    *status = 2;
                }
                CURRENT_USER
                    .create(APPROVED)
                    .map_err(crate::operation)?
                    .set_bytes(NAME, windows_registry::Type::Bytes, &approval)
                    .map_err(crate::operation)?;
            }
        } else {
            delete_optional(&key, NAME)?;
        }
        Ok(())
    }

    /// 恢复完整登记快照。参数：value 为写入前的状态。返回：恢复结果。
    pub fn restore(value: Snapshot) -> Result<()> {
        let key = CURRENT_USER.create(RUN).map_err(crate::operation)?;
        match value.command {
            Some(command) => key.set_string(NAME, command).map_err(crate::operation)?,
            None => delete_optional(&key, NAME)?,
        }
        let key = CURRENT_USER.create(APPROVED).map_err(crate::operation)?;
        match value.approval {
            Some(bytes) => key
                .set_bytes(NAME, windows_registry::Type::Bytes, &bytes)
                .map_err(crate::operation)?,
            None => delete_optional(&key, NAME)?,
        }
        Ok(())
    }
}

/// 校验刷新间隔。参数：preferences 为候选设置。返回：校验结果。
pub fn validate_preferences(preferences: &Preferences) -> Result<()> {
    if !(500..=10000).contains(&preferences.refresh_ms) {
        return Err(Error::Config(
            "刷新间隔必须介于 500 和 10000 毫秒之间".into(),
        ));
    }
    Ok(())
}

/// 加载用户设置。参数：root 为数据目录。返回：持久化设置或首次运行默认值。
pub fn load_preferences(root: &Path) -> Result<Preferences> {
    let preferences = match std::fs::read(root.join("state/desktop.json")) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Preferences::default(),
        Err(error) => return Err(error.into()),
    };
    validate_preferences(&preferences)?;
    Ok(preferences)
}

/// 原子保存用户设置。参数：root 为目录，preferences 为设置。返回：保存结果。
pub fn save_preferences(root: &Path, preferences: &Preferences) -> Result<()> {
    validate_preferences(preferences)?;
    atomic_write(
        &root.join("state/desktop.json"),
        &serde_json::to_vec_pretty(preferences)?,
    )
}

/// 校验受支持的配置文档名称。参数：name 为主配置或覆盖文件的相对路径。返回：校验结果。
pub fn validate_document_name(name: &str) -> Result<()> {
    // 主文件和覆盖文件分别使用严格文件名规则，拒绝目录穿越及 Windows 别名。
    if let Some((unit, file)) = name.split_once(".d/") {
        config::validate_name(unit)?;
        let stem = file
            .strip_suffix(".conf")
            .ok_or_else(|| Error::Config("覆盖文件必须使用 .conf 扩展名".into()))?;
        config::validate_name(&format!("{stem}.service"))?;
    } else {
        config::validate_name(name)?;
    }
    Ok(())
}

/// 校验文本大小与编码。参数：text 为 UTF-8 正文。返回：校验结果。
pub fn validate_text(text: &str) -> Result<()> {
    if text.len() > MAX_DOCUMENT || text.contains('\0') {
        return Err(Error::Config(
            "配置必须为不含 NUL 的 UTF-8 文本，最大 1 MiB".into(),
        ));
    }
    Ok(())
}

/// 获取文档路径并拒绝符号链接及重解析点。参数：directory 为配置目录，name 为相对名称。返回：可访问路径。
pub fn document_path(directory: &Path, name: &str) -> Result<PathBuf> {
    validate_document_name(name)?;
    let path = directory.join(name);
    // 每一级都检查，避免同名覆盖目录或目标文件链接到其他目录。
    for component in [directory, path.parent().unwrap(), path.as_path()] {
        match std::fs::symlink_metadata(component) {
            Ok(metadata) => {
                let linked = metadata.file_type().is_symlink();
                #[cfg(windows)]
                let linked = {
                    use std::os::windows::fs::MetadataExt;
                    linked || metadata.file_attributes() & 0x400 != 0
                };
                if linked {
                    return Err(Error::Config("配置编辑不支持链接或重解析点".into()));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(path)
}

/// 读取可能尚未创建的有界文本。参数：path 为文件。返回：原文或 None。
pub fn read_optional(path: &Path) -> Result<Option<String>> {
    use std::io::Read;
    // 限制实际读取量，文件在元数据检查后增长也不会造成无限内存分配。
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let mut text = String::new();
    file.take((MAX_DOCUMENT + 1) as u64)
        .read_to_string(&mut text)?;
    validate_text(&text)?;
    Ok(Some(text))
}

/// 读取主配置及按名称排序的覆盖文件。参数：root 为目录，unit 为名称。返回：可编辑文档列表。
pub fn documents(root: &Path, unit: &str) -> Result<Vec<Document>> {
    config::validate_name(unit)?;
    let directory = root.join("units");
    let mut names = vec![unit.to_string()];
    let drop_in = directory.join(format!("{unit}.d"));
    document_path(&directory, &format!("{unit}.d/check.conf"))?;
    if drop_in.exists() {
        for entry in std::fs::read_dir(drop_in)? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|ext| ext == "conf") {
                names.push(format!("{unit}.d/{}", entry.file_name().to_string_lossy()));
            }
        }
    }
    names.sort();
    names
        .into_iter()
        .map(|name| {
            let path = document_path(&directory, &name)?;
            let text = read_optional(&path)?
                .ok_or_else(|| Error::Config(format!("配置不存在：{name}")))?;
            Ok(Document { name, text })
        })
        .collect()
}
