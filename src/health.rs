//! TCP/HTTP 健康探测；与进程生命周期独立，不自动重启进程。
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct HealthConfig {
    pub kind: String,
    pub port: u16,
    pub url: String,
    pub timeout: Duration,
    pub interval: Duration,
}
impl Default for HealthConfig {
    /// 创建默认探测配置。参数：无。返回：关闭探测、超时一秒、间隔十秒的配置。
    fn default() -> Self {
        Self {
            kind: String::new(),
            port: 0,
            url: String::new(),
            timeout: Duration::from_secs(1),
            interval: Duration::from_secs(10),
        }
    }
}
impl HealthConfig {
    /// 校验探测配置。参数：self 为候选配置。返回：校验结果。
    pub fn validate(&self) -> Result<()> {
        // 类型、目标和时间分别校验，禁用时允许保留目标方便再次启用。
        match self.kind.as_str() {
            "" | "none" => return Ok(()),
            "tcp" if self.port > 0 => (),
            "http" => {
                let url = reqwest::Url::parse(&self.url)
                    .map_err(|_| Error::Config("HealthUrl 必须为完整 HTTP/HTTPS URL".into()))?;
                if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                    return Err(Error::Config("HealthUrl 必须为完整 HTTP/HTTPS URL".into()));
                }
            }
            _ => {
                return Err(Error::Config(
                    "HealthType 支持 none/tcp/http；TCP 端口必须为 1～65535".into(),
                ));
            }
        }
        if self.timeout.is_zero()
            || self.timeout > Duration::from_secs(60)
            || self.interval < Duration::from_secs(1)
            || self.interval > Duration::from_secs(3600)
        {
            return Err(Error::Config(
                "健康检查超时须为 (0,60] 秒，间隔须为 1～3600 秒".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthRecord {
    pub time: String,
    pub instance: u64,
    pub healthy: bool,
    pub latency_ms: u64,
    pub detail: String,
}

/// 执行一次探测并限定整个请求时间。参数：配置、实例、HTTP 客户端。返回：带时间和诊断的结果。
pub async fn probe(config: &HealthConfig, instance: u64, client: &reqwest::Client) -> HealthRecord {
    let started = Instant::now();
    // HTTP 禁止重定向，只有配置 URL 的直接响应 200 才成功。
    let result = tokio::time::timeout(config.timeout, async {
        if config.kind == "tcp" {
            tokio::net::TcpStream::connect(("127.0.0.1", config.port))
                .await
                .map(|_| "TCP 连接成功".to_string())
                .map_err(|e| format!("TCP 连接失败：{e}"))
        } else {
            match client.get(&config.url).send().await {
                Ok(response) if response.status().as_u16() == 200 => Ok("HTTP 200".into()),
                Ok(response) => Err(format!("HTTP {}", response.status().as_u16())),
                Err(error) => Err(if error.is_timeout() {
                    "HTTP 请求超时".into()
                } else {
                    "HTTP 请求失败".into()
                }),
            }
        }
    })
    .await
    .unwrap_or_else(|_| Err("健康检查超时".into()));
    HealthRecord {
        time: chrono::Utc::now().to_rfc3339(),
        instance,
        healthy: result.is_ok(),
        latency_ms: started.elapsed().as_millis() as u64,
        detail: result.unwrap_or_else(|e| e),
    }
}
