//! Windows 程序托管器的可测试核心与平台适配。
pub mod config;
pub mod graph;
pub mod ipc;
pub mod logging;
pub mod manager;
pub mod platform;
pub mod policy;
pub mod scm;

/// 项目统一错误，便于 CLI 和 IPC 返回稳定的错误类别。
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("配置错误：{0}")]
    Config(String),
    #[error("操作失败：{0}")]
    Operation(String),
    #[error("IO 错误：{0}")]
    Io(#[from] std::io::Error),
    #[error("JSON 错误：{0}")]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, Error>;

/// 将任意可显示的平台错误转换为操作错误。
/// 参数：error 为底层错误。返回：统一错误。
pub fn operation(error: impl std::fmt::Display) -> Error {
    Error::Operation(error.to_string())
}
