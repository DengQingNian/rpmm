//! 桌面应用的用户设置、配置文档及安全文件操作。
use crate::{Error, Result, config, manager::atomic_write};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub const MAX_DOCUMENT: usize = 1024 * 1024;
/// 分类名称允许的最大字符数。
pub const MAX_CATEGORY_CHARS: usize = 32;
/// 单个数据目录允许的分类数量上限。
pub const MAX_CATEGORIES: usize = 50;
/// 默认分类名称；未分类和旧数据的子进程都归入该分类。
pub const DEFAULT_CATEGORY: &str = "默认";

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub name: String,
    pub text: String,
}

/// 可移植的配置包；按服务保留原文、覆盖顺序与启用状态。
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigBundle {
    pub format: String,
    pub version: u32,
    pub services: Vec<ServiceBundle>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServiceBundle {
    pub name: String,
    pub enabled: bool,
    pub documents: Vec<Document>,
}

/// 导出指定服务及覆盖文件。参数：root 为目录，statuses 为选中状态。返回：结构化配置包。
pub fn export_bundle(root: &Path, statuses: &[crate::manager::Status]) -> Result<ConfigBundle> {
    if statuses.is_empty() {
        return Err(Error::Config("请至少选择一个服务".into()));
    }
    Ok(ConfigBundle {
        format: "rpmm-config".into(),
        version: 1,
        services: statuses
            .iter()
            .map(|status| {
                Ok(ServiceBundle {
                    name: status.name.clone(),
                    enabled: status.enabled,
                    documents: documents(root, &status.name)?,
                })
            })
            .collect::<Result<_>>()?,
    })
}

/// 读取持久化的桌面数据目录选择。参数：无。返回：已配置目录或默认目录。
pub fn configured_root() -> Result<PathBuf> {
    load_root_selection(&default_root())
}

/// 从指定默认目录读取桌面数据目录选择。参数：default 为选择文件所在的默认目录。返回：已保存路径或默认目录。
pub fn load_root_selection(default: &Path) -> Result<PathBuf> {
    match std::fs::read(default.join("state/data-root.json")) {
        Ok(bytes) => {
            let root: PathBuf = serde_json::from_slice(&bytes)?;
            if !root.is_absolute() {
                return Err(Error::Config("数据目录必须为绝对路径".into()));
            }
            Ok(root)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(default.to_path_buf()),
        Err(error) => Err(error.into()),
    }
}

/// 保存下次桌面启动使用的数据目录。参数：root 为已验证绝对路径。返回：持久化结果。
pub fn save_root(root: &Path) -> Result<()> {
    save_root_selection(&default_root(), root)
}

/// 原子保存数据目录选择到指定默认目录。参数：default 为选择文件所在目录，root 为绝对目标路径。返回：保存结果。
pub fn save_root_selection(default: &Path, root: &Path) -> Result<()> {
    if !root.is_absolute() {
        return Err(Error::Config("数据目录必须为绝对路径".into()));
    }
    std::fs::create_dir_all(default.join("state"))?;
    atomic_write(
        &default.join("state/data-root.json"),
        &serde_json::to_vec(root)?,
    )
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

/// 子进程配置的界面分类；只影响界面组织，不写入 .service 文件。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Categories {
    /// 有序分类名称，默认分类固定位于首位。
    pub names: Vec<String>,
    /// 子进程名称到分类名称的映射；默认分类不记录映射。
    pub assignments: BTreeMap<String, String>,
}

impl Default for Categories {
    /// 提供首次运行的分类。参数：无。返回：仅包含默认分类的分类表。
    fn default() -> Self {
        Self {
            names: vec![DEFAULT_CATEGORY.to_string()],
            assignments: BTreeMap::new(),
        }
    }
}

/// 获取分类文件的绝对路径。参数：root 为数据目录。返回：分类文件路径。
pub fn categories_path(root: &Path) -> PathBuf {
    root.join("state/categories.json")
}

/// 校验分类名称。参数：name 为已去除首尾空白的名称。返回：校验结果。
fn validate_category_name(name: &str) -> Result<()> {
    if name.is_empty() || name.chars().count() > MAX_CATEGORY_CHARS {
        return Err(Error::Config(format!(
            "分类名称须为 1～{MAX_CATEGORY_CHARS} 个字符"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(Error::Config("分类名称不能包含控制字符".into()));
    }
    Ok(())
}

/// 严格校验界面提交的分类。参数：value 为候选分类。返回：校验结果。
/// 名称重复、缺少默认分类、映射到不存在的分类以及非法子进程名称都会被拒绝。
pub fn validate_categories(value: &Categories) -> Result<()> {
    if value.names.len() > MAX_CATEGORIES {
        return Err(Error::Config(format!(
            "分类数量不能超过 {MAX_CATEGORIES} 个"
        )));
    }
    let mut names = BTreeSet::new();
    for name in &value.names {
        validate_category_name(name)?;
        if !names.insert(name.as_str()) {
            return Err(Error::Config(format!("分类名称重复：{name}")));
        }
    }
    if !names.contains(DEFAULT_CATEGORY) {
        return Err(Error::Config(format!("缺少默认分类：{DEFAULT_CATEGORY}")));
    }
    for (unit, category) in &value.assignments {
        config::validate_name(unit)?;
        if !names.contains(category.as_str()) {
            return Err(Error::Config(format!("分类不存在：{category}")));
        }
    }
    Ok(())
}

/// 归一化分类数据。参数：value 为磁盘或界面提供的分类。返回：含默认分类且无重复、无越界映射的分类。
/// 旧版本没有分类文件，或文件被手工修改时都回退到默认分类，保证旧数据仍显示在默认分类下。
pub fn normalize_categories(value: Categories) -> Categories {
    let mut names = vec![DEFAULT_CATEGORY.to_string()];
    for name in value.names {
        let name = name.trim();
        // 非法名称、重复名称和超出上限的条目直接丢弃，避免一个坏文件让界面不可用。
        if validate_category_name(name).is_err()
            || names.iter().any(|item| item == name)
            || names.len() >= MAX_CATEGORIES
        {
            continue;
        }
        names.push(name.to_string());
    }
    let assignments = value
        .assignments
        .into_iter()
        // 指向已消失分类的映射回落到默认分类；默认分类本身不需要映射。
        .filter(|(unit, category)| {
            category != DEFAULT_CATEGORY
                && names.iter().any(|item| item == category)
                && config::validate_name(unit).is_ok()
        })
        .collect();
    Categories { names, assignments }
}

/// 读取界面分类。参数：root 为数据目录。返回：归一化后的分类；文件缺失时返回仅含默认分类的结果。
pub fn load_categories(root: &Path) -> Result<Categories> {
    match std::fs::read(categories_path(root)) {
        Ok(bytes) => Ok(normalize_categories(serde_json::from_slice(&bytes)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Categories::default()),
        Err(error) => Err(error.into()),
    }
}

/// 校验并原子保存界面分类。参数：root 为数据目录，categories 为分类。返回：保存结果。
pub fn save_categories(root: &Path, categories: &Categories) -> Result<()> {
    validate_categories(categories)?;
    std::fs::create_dir_all(root.join("state"))?;
    let normalized = normalize_categories(categories.clone());
    atomic_write(
        &categories_path(root),
        &serde_json::to_vec_pretty(&normalized)?,
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
