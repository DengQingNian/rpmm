//! 有界分块日志、JSONL 轮转和查询。
use crate::Result;
use serde::{Deserialize, Serialize};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    pub time: String,
    pub unit: String,
    pub instance: u64,
    pub source: String,
    pub text: String,
}

#[derive(Clone)]
pub struct Logger {
    directory: PathBuf,
    lock: Arc<Mutex<()>>,
    pub events: broadcast::Sender<Record>,
    maximum: u64,
    backups: usize,
}
impl Logger {
    /// 创建日志组件。参数：directory 为日志目录。返回：日志器。
    pub fn new(directory: &Path) -> Result<Self> {
        Self::with_limits(directory, 10 * 1024 * 1024, 5)
    }
    /// 创建带轮转策略的日志器。参数：directory、maximum、backups 为目录、字节阈值和备份数。返回：日志器。
    pub fn with_limits(directory: &Path, maximum: u64, backups: usize) -> Result<Self> {
        std::fs::create_dir_all(directory)?;
        Ok(Self {
            directory: directory.into(),
            lock: Arc::new(Mutex::new(())),
            events: broadcast::channel(256).0,
            maximum,
            backups,
        })
    }
    /// 写入一条记录。参数：unit、instance、source、text 为上下文和内容。返回：写入结果。
    pub fn write(&self, unit: &str, instance: u64, source: &str, text: &str) -> Result<()> {
        let record = Record {
            time: chrono::Utc::now().to_rfc3339(),
            unit: unit.into(),
            instance,
            source: source.into(),
            text: text.into(),
        };
        let mut bytes = serde_json::to_vec(&record)?;
        bytes.push(b'\n');
        let _guard = self.lock.lock().map_err(crate::operation)?;
        let path = self.directory.join(format!("{unit}.jsonl"));
        if path.metadata().map(|m| m.len()).unwrap_or(0) + bytes.len() as u64 > self.maximum {
            self.rotate(&path)?;
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(&bytes)?;
        let _ = self.events.send(record);
        Ok(())
    }
    /// 轮转已持锁的日志。参数：path 为当前文件。返回：结果。
    fn rotate(&self, path: &Path) -> Result<()> {
        for index in (1..=self.backups).rev() {
            let target = path.with_extension(format!("jsonl.{index}"));
            let source = if index == 1 {
                path.to_path_buf()
            } else {
                path.with_extension(format!("jsonl.{}", index - 1))
            };
            if target.exists() {
                std::fs::remove_file(&target)?;
            }
            if source.exists() {
                std::fs::rename(source, target)?;
            }
        }
        if self.backups == 0 && path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
    /// 查询最近日志。参数：unit、source、limit 为 unit、可选来源和条数。返回：有界记录集。
    pub fn tail(&self, unit: &str, source: Option<&str>, limit: usize) -> Result<Vec<Record>> {
        let _guard = self.lock.lock().map_err(crate::operation)?;
        self.tail_locked(unit, source, limit)
    }
    /// 原子查询及订阅，避免历史和实时记录之间出现重复或遗漏。
    /// 参数：unit/source/limit 为筛选和条数。返回：记录和实时订阅。
    pub fn tail_and_subscribe(
        &self,
        unit: &str,
        source: Option<&str>,
        limit: usize,
    ) -> Result<(Vec<Record>, broadcast::Receiver<Record>)> {
        let _guard = self.lock.lock().map_err(crate::operation)?;
        let records = self.tail_locked(unit, source, limit)?;
        Ok((records, self.events.subscribe()))
    }
    /// 查询已持锁的历史。参数：unit/source/limit 为筛选和条数。返回：记录。
    fn tail_locked(&self, unit: &str, source: Option<&str>, limit: usize) -> Result<Vec<Record>> {
        let limit = limit.min(10000);
        let mut records = Vec::new();
        if limit == 0 {
            return Ok(records);
        }
        // 从最新文件末尾向前按块读取，达到目标条数即结束，避免每次刷新扫描全部轮转文件。
        for index in 0..=self.backups {
            let path = self.directory.join(if index == 0 {
                format!("{unit}.jsonl")
            } else {
                format!("{unit}.jsonl.{index}")
            });
            let mut file = match File::open(path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            let mut offset = file.seek(SeekFrom::End(0))?;
            let mut pending = Vec::new();
            while offset > 0 && records.len() < limit {
                let count = offset.min(65536) as usize;
                offset -= count as u64;
                file.seek(SeekFrom::Start(offset))?;
                let mut chunk = vec![0; count];
                file.read_exact(&mut chunk)?;
                chunk.extend_from_slice(&pending);
                pending = chunk;
                while let Some(newline) = pending.iter().rposition(|byte| *byte == b'\n') {
                    let line = pending.split_off(newline + 1);
                    pending.truncate(newline);
                    append_record(&line, source, &mut records);
                    if records.len() == limit {
                        break;
                    }
                }
            }
            if offset == 0 && records.len() < limit {
                append_record(&pending, source, &mut records);
            }
            if records.len() == limit {
                break;
            }
        }
        records.reverse();
        Ok(records)
    }
    /// 持续读取一个进程管道。参数：reader 和记录上下文。返回：无；异常写入生命周期日志。
    pub fn drain(&self, mut reader: impl Read, unit: &str, instance: u64, source: &str) {
        let mut bytes = [0u8; 8192];
        let mut pending = Vec::with_capacity(8196);
        loop {
            let count = match reader.read(&mut bytes) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    let _ = self.write(
                        unit,
                        instance,
                        "manager",
                        &format!("读取 {source} 失败：{e}"),
                    );
                    break;
                }
            };
            pending.extend_from_slice(&bytes[..count]);
            let usable = utf8_prefix(&pending);
            if usable > 0 {
                if let Err(e) = self.write(
                    unit,
                    instance,
                    source,
                    &String::from_utf8_lossy(&pending[..usable]),
                ) {
                    eprintln!("日志写入失败（继续排空管道）：{e}");
                }
                pending.drain(..usable);
            }
        }
        if !pending.is_empty() {
            let _ = self.write(unit, instance, source, &String::from_utf8_lossy(&pending));
        }
    }
}

/// 找到不包含末尾半个 Unicode 字符的前缀；中间非法字节仍交给替代字符处理。
/// 参数：bytes 为输入块。返回：可显示前缀长度。
fn utf8_prefix(bytes: &[u8]) -> usize {
    let mut offset = 0;
    while offset < bytes.len() {
        match std::str::from_utf8(&bytes[offset..]) {
            Ok(_) => return bytes.len(),
            Err(error) => match error.error_len() {
                Some(length) => offset += error.valid_up_to() + length,
                None => return offset + error.valid_up_to(),
            },
        }
    }
    bytes.len()
}

/// 解码一条倒序读取的日志。参数：line 为 JSON 字节，source 为可选来源，records 为结果容器。返回：无；跳过空行与崩溃留下的残缺记录。
fn append_record(line: &[u8], source: Option<&str>, records: &mut Vec<Record>) {
    if let Ok(record) = serde_json::from_slice::<Record>(line)
        && source.is_none_or(|s| s == record.source)
    {
        records.push(record);
    }
}
