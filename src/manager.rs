//! 生命周期监督及串行依赖事务；长任务不占用状态锁。
use crate::{
    Error, Result,
    config::{self, ServiceType, Unit},
    graph::{self, Units},
    logging::Logger,
    platform::{Backend, Native, Process},
    policy::{StartLimiter, should_restart},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as AsyncMutex, oneshot, watch};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum State {
    Inactive,
    Activating,
    Active,
    Deactivating,
    Failed,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Status {
    pub name: String,
    pub state: State,
    pub substate: String,
    pub pid: Option<u32>,
    pub instance: u64,
    pub exit_code: Option<u32>,
    pub reason: Option<String>,
    pub restart_count: u64,
    pub config_version: u64,
    pub enabled: bool,
}
#[derive(Clone)]
struct Job {
    unit: Unit,
    task: u64,
    cancel: watch::Sender<bool>,
    done: watch::Receiver<bool>,
    limiter: Arc<Mutex<StartLimiter>>,
}

pub struct Manager {
    pub root: PathBuf,
    pub logger: Logger,
    backend: Arc<dyn Backend>,
    units: RwLock<Units>,
    statuses: Mutex<BTreeMap<String, Status>>,
    jobs: Mutex<BTreeMap<String, Job>>,
    enabled: Mutex<BTreeSet<String>>,
    transaction: AsyncMutex<()>,
    pending: Mutex<BTreeSet<String>>,
    abort: AtomicBool,
    shutting_down: AtomicBool,
    generation: AtomicU64,
    serial: AtomicU64,
    health_records: Mutex<BTreeMap<String, crate::health::HealthRecord>>,
    health_history:
        Mutex<BTreeMap<String, std::collections::VecDeque<crate::health::HealthRecord>>>,
}

impl Manager {
    /// 加载原生管理器。参数：root 为数据目录。返回：共享管理器。
    pub fn new(root: &Path) -> Result<Arc<Self>> {
        Self::with_backend(root, Arc::new(Native))
    }
    /// 注入平台用于测试。参数：root 为目录，backend 为进程平台。返回：管理器。
    pub fn with_backend(root: &Path, backend: Arc<dyn Backend>) -> Result<Arc<Self>> {
        for folder in ["units", "state", "logs"] {
            std::fs::create_dir_all(root.join(folder))?;
        }
        let units = config::load(&root.join("units"))?;
        graph::order(&units, &units.keys().cloned().collect())?;
        let enabled = match std::fs::read(root.join("state/enabled.json")) {
            Ok(bytes) => serde_json::from_slice::<BTreeSet<String>>(&bytes)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => BTreeSet::new(),
            Err(e) => return Err(e.into()),
        };
        for name in &enabled {
            config::validate_name(name)?;
        }
        Ok(Arc::new(Self {
            root: root.into(),
            logger: Logger::new(&root.join("logs"))?,
            backend,
            units: RwLock::new(units),
            statuses: Mutex::new(BTreeMap::new()),
            jobs: Mutex::new(BTreeMap::new()),
            enabled: Mutex::new(enabled),
            transaction: AsyncMutex::new(()),
            pending: Mutex::new(BTreeSet::new()),
            abort: AtomicBool::new(false),
            shutting_down: AtomicBool::new(false),
            generation: AtomicU64::new(1),
            serial: AtomicU64::new(1),
            health_records: Mutex::new(BTreeMap::new()),
            health_history: Mutex::new(BTreeMap::new()),
        }))
    }
    /// 获取一致状态快照。参数：name 为可选 unit 名。返回：状态列表。
    pub fn status(&self, name: Option<&str>) -> Result<Vec<Status>> {
        let units = self.units.read().unwrap();
        let statuses = self.statuses.lock().unwrap();
        let enabled = self.enabled.lock().unwrap();
        let names: Vec<_> = if let Some(name) = name {
            if !units.contains_key(name) {
                return Err(Error::Operation(format!("不存在 unit：{name}")));
            }
            vec![name.to_string()]
        } else {
            units.keys().cloned().collect()
        };
        Ok(names
            .into_iter()
            .map(|name| {
                let mut status = statuses.get(&name).cloned().unwrap_or(Status {
                    name: name.clone(),
                    state: State::Inactive,
                    substate: "dead".into(),
                    pid: None,
                    instance: 0,
                    exit_code: None,
                    reason: None,
                    restart_count: 0,
                    config_version: self.generation.load(Ordering::SeqCst),
                    enabled: false,
                });
                status.enabled = enabled.contains(&name);
                status
            })
            .collect())
    }
    /// 获取实例配置快照。参数：name 为进程名称。返回：运行定义或当前定义。
    pub fn unit_snapshot(&self, name: &str) -> Result<Unit> {
        if let Some(job) = self.jobs.lock().unwrap().get(name)
            && !*job.done.borrow()
        {
            return Ok(job.unit.clone());
        }
        self.units
            .read()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| Error::Operation("进程不存在".into()))
    }
    /// 查询所有健康检查状态。参数：无。返回：名称、配置和当前实例最近的检查结果。
    pub fn health_status(&self) -> Result<Vec<serde_json::Value>> {
        self.status(None)?.into_iter().map(|status| {
            let unit = self.unit_snapshot(&status.name)?;
            let record = self.health_records.lock().unwrap().get(&status.name).filter(|r| r.instance == status.instance && status.state == State::Active).cloned();
            Ok(serde_json::json!({ "unit": status.name, "config": unit.health, "latest": record }))
        }).collect()
    }
    /// 读取独立的有界健康历史。参数：name 为服务名称。返回：最近一百次内存记录，应用退出后清空。
    pub fn health_history(&self, name: &str) -> Result<Vec<crate::health::HealthRecord>> {
        config::validate_name(name)?;
        Ok(self
            .health_history
            .lock()
            .unwrap()
            .get(name)
            .map(|h| h.iter().cloned().collect())
            .unwrap_or_default())
    }
    /// 等待前置服务当前实例健康。参数：unit 为待启动定义。返回：就绪、超时、取消或配置诊断。
    async fn wait_healthy(&self, unit: &Unit) -> Result<()> {
        if unit.health_after.is_empty()
            || self
                .status(Some(&unit.name))
                .is_ok_and(|statuses| statuses[0].state == State::Active)
        {
            return Ok(());
        }
        let deadline = Instant::now() + unit.timeout_start.unwrap_or(Duration::from_secs(90));
        // 等待不持有状态锁；每次核对当前实例，停止请求可以取消整个启动事务。
        loop {
            if self.abort.load(Ordering::SeqCst) || self.shutting_down.load(Ordering::SeqCst) {
                return Err(Error::Operation("健康依赖等待已取消".into()));
            }
            let mut ready = true;
            for name in &unit.health_after {
                let dependency = self.unit_snapshot(name)?;
                if !matches!(dependency.health.kind.as_str(), "tcp" | "http") {
                    return Err(Error::Config(format!("健康前置服务 {name} 未启用健康检查")));
                }
                let status = self.status(Some(name))?.remove(0);
                if matches!(status.state, State::Failed | State::Inactive) {
                    return Err(Error::Operation(format!("健康前置服务 {name} 未运行")));
                }
                ready &= status.state == State::Active
                    && self
                        .health_records
                        .lock()
                        .unwrap()
                        .get(name)
                        .is_some_and(|record| record.instance == status.instance && record.healthy);
            }
            if ready {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Error::Operation(format!(
                    "{} 等待前置服务健康超时",
                    unit.name
                )));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    /// 按运行快照后台探测并记录结果。参数：self 为管理器，unit/task 为配置和监督所有权。返回：任务结束时返回。
    async fn monitor_health(self: Arc<Self>, unit: Unit, task: u64) {
        let Ok(client) = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
        else {
            return;
        };
        // 监督所有权与实例编号都核对，避免停止或重启时发布迟到结果。
        loop {
            let Some(current) = self.jobs.lock().unwrap().get(&unit.name).cloned() else {
                break;
            };
            if current.task != task || *current.done.borrow() || *current.cancel.borrow() {
                break;
            }
            let mut cancelled = current.cancel.subscribe();
            let mut pause = Duration::from_millis(250);
            if let Ok(statuses) = self.status(Some(&unit.name))
                && let Some(status) = statuses.first()
                && status.state == State::Active
                && status.pid.is_some()
            {
                pause = unit.health.interval;
                let record = crate::health::probe(&unit.health, status.instance, &client).await;
                if self.status(Some(&unit.name)).is_ok_and(|v| {
                    v[0].state == State::Active
                        && v[0].instance == record.instance
                        && v[0].pid == status.pid
                }) {
                    self.health_records
                        .lock()
                        .unwrap()
                        .insert(unit.name.clone(), record.clone());
                    let mut histories = self.health_history.lock().unwrap();
                    let history = histories.entry(unit.name.clone()).or_default();
                    history.push_back(record);
                    while history.len() > 100 {
                        history.pop_front();
                    }
                }
            }
            tokio::select! { _ = tokio::time::sleep(pause) => (), _ = cancelled.changed() => () }
        }
    }
    /// 原子更新实例状态。参数：name/task 为所有权标识，edit 为短时修改函数。返回：无。
    fn update(&self, name: &str, task: u64, edit: impl FnOnce(&mut Status)) {
        // 仅当前监督任务能更新状态，旧任务迟到事件不覆盖新实例。
        let jobs = self.jobs.lock().unwrap();
        if jobs.get(name).is_some_and(|job| job.task == task)
            && let Some(status) = self.statuses.lock().unwrap().get_mut(name)
        {
            edit(status);
        }
    }
    /// 写入管理器事件。参数：unit、instance、text 为上下文。返回：无。
    fn event(&self, unit: &str, instance: u64, text: &str) {
        if let Err(e) = self.logger.write(unit, instance, "manager", text) {
            eprintln!("管理器日志写入失败：{e}");
        }
    }
    /// 启动需求闭包。参数：roots 为 unit 名列表。返回：事务结果；成功依赖不会回滚。
    pub async fn start(self: &Arc<Self>, roots: &[String]) -> Result<()> {
        let _guard = self.transaction.lock().await;
        self.start_locked(roots).await
    }
    /// 执行已获得事务锁的启动。参数：roots 为根节点。返回：事务结果。
    async fn start_locked(self: &Arc<Self>, roots: &[String]) -> Result<()> {
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err(Error::Operation("管理器正在关闭".into()));
        }
        self.abort.store(false, Ordering::SeqCst);
        let units = self.snapshots();
        let plan = graph::start_plan(&units, roots)?;
        *self.pending.lock().unwrap() = plan.layers.iter().flatten().cloned().collect();
        for warning in plan.warnings {
            self.event("manager", 0, &warning);
        }
        let mut failures = BTreeMap::<String, String>::new();
        for layer in plan.layers {
            if self.abort.load(Ordering::SeqCst) || self.shutting_down.load(Ordering::SeqCst) {
                break;
            }
            let mut tasks = vec![];
            for name in layer {
                let unit = units[&name].clone();
                if unit
                    .requires
                    .iter()
                    .any(|dep| unit.after.contains(dep) && failures.contains_key(dep))
                {
                    failures.insert(name.clone(), "排序在前的 required unit 启动失败".into());
                    self.mark_failed(&unit, "dependency-failed");
                    continue;
                }
                let manager = self.clone();
                tasks.push((
                    name,
                    tokio::spawn(async move {
                        if let Err(error) = manager.wait_healthy(&unit).await {
                            manager.mark_failed(&unit, "dependency-unhealthy");
                            if let Some(status) =
                                manager.statuses.lock().unwrap().get_mut(&unit.name)
                                && status.state == State::Failed
                            {
                                status.reason = Some(error.to_string());
                            }
                            return Err(error);
                        }
                        manager.start_one(unit).await
                    }),
                ));
            }
            for (name, task) in tasks {
                match task.await {
                    Ok(Err(error)) => {
                        failures.insert(name, error.to_string());
                    }
                    Ok(Ok(())) => (),
                    Err(error) => {
                        failures.insert(name, error.to_string());
                    }
                }
            }
        }
        self.pending.lock().unwrap().clear();
        if self.abort.load(Ordering::SeqCst) || self.shutting_down.load(Ordering::SeqCst) {
            return Err(Error::Operation("启动事务已取消".into()));
        }
        let errors: Vec<_> = roots
            .iter()
            .filter_map(|n| failures.get(n).map(|e| format!("{n}: {e}")))
            .collect();
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error::Operation(errors.join("；")))
        }
    }
    /// 标记未运行 unit 的依赖失败。参数：unit/reason 为定义和原因。返回：无。
    fn mark_failed(&self, unit: &Unit, reason: &str) {
        let mut statuses = self.statuses.lock().unwrap();
        if statuses
            .get(&unit.name)
            .is_some_and(|s| s.state == State::Active)
        {
            return;
        }
        statuses.insert(
            unit.name.clone(),
            Status {
                name: unit.name.clone(),
                state: State::Failed,
                substate: reason.into(),
                pid: None,
                instance: 0,
                exit_code: None,
                reason: Some(reason.into()),
                restart_count: 0,
                config_version: self.generation.load(Ordering::SeqCst),
                enabled: false,
            },
        );
    }
    /// 启动一个独立监督任务并等待首次激活。参数：unit 为定义。返回：首次启动结果。
    async fn start_one(self: Arc<Self>, unit: Unit) -> Result<()> {
        let old = self.jobs.lock().unwrap().get(&unit.name).cloned();
        if let Some(job) = &old
            && !*job.done.borrow()
        {
            loop {
                if self.abort.load(Ordering::SeqCst) || self.shutting_down.load(Ordering::SeqCst) {
                    return Err(Error::Operation("启动事务已取消".into()));
                }
                let status = self.status(Some(&unit.name))?.remove(0);
                if status.state == State::Active {
                    return Ok(());
                }
                if status.state == State::Failed {
                    return Err(Error::Operation(
                        status.reason.unwrap_or_else(|| "已有监督任务失败".into()),
                    ));
                }
                if *job.done.borrow() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }
        let limiter = old.as_ref().map(|j| j.limiter.clone()).unwrap_or_default();
        let task = self.serial.fetch_add(1, Ordering::SeqCst);
        let (cancel, cancel_rx) = watch::channel(
            self.abort.load(Ordering::SeqCst) || self.shutting_down.load(Ordering::SeqCst),
        );
        let (done_tx, done) = watch::channel(false);
        let (started_tx, started_rx) = oneshot::channel();
        let name = unit.name.clone();
        self.jobs.lock().unwrap().insert(
            name.clone(),
            Job {
                unit: unit.clone(),
                task,
                cancel,
                done,
                limiter: limiter.clone(),
            },
        );
        self.statuses.lock().unwrap().insert(
            name.clone(),
            Status {
                name,
                state: State::Activating,
                substate: "start".into(),
                pid: None,
                instance: task,
                exit_code: None,
                reason: None,
                restart_count: 0,
                config_version: self.generation.load(Ordering::SeqCst),
                enabled: false,
            },
        );
        // 关闭发布窗口：停止可能发生在创建取消通道和发布状态之间。
        if matches!(unit.health.kind.as_str(), "tcp" | "http") {
            tokio::spawn(self.clone().monitor_health(unit.clone(), task));
        }
        if (self.abort.load(Ordering::SeqCst) || self.shutting_down.load(Ordering::SeqCst))
            && let Some(job) = self.jobs.lock().unwrap().get(&unit.name)
        {
            let _ = job.cancel.send(true);
        }
        // 任务始终发出完成通知；panic 会被 JoinHandle 转换为 failed。
        let manager = self.clone();
        tokio::spawn(async move {
            let runner = manager.clone();
            let handle = tokio::spawn(async move {
                runner
                    .supervise(unit, task, limiter, cancel_rx, started_tx)
                    .await;
            });
            if let Err(e) = handle.await {
                manager.update(&self_name(&manager, task), task, |s| {
                    s.state = State::Failed;
                    s.reason = Some(format!("监督任务异常：{e}"));
                });
            }
            let _ = done_tx.send(true);
        });
        started_rx.await.map_err(crate::operation)?
    }
    /// 监督配置快照直到终止。参数：unit、task、limiter、cancel、started 为定义、实例所有权、策略及通知。返回：无。
    async fn supervise(
        self: Arc<Self>,
        unit: Unit,
        task: u64,
        limiter: Arc<Mutex<StartLimiter>>,
        mut cancel: watch::Receiver<bool>,
        started: oneshot::Sender<Result<()>>,
    ) {
        let mut started = Some(started);
        let mut restarts = 0;
        loop {
            if *cancel.borrow() {
                break;
            }
            if !limiter
                .lock()
                .unwrap()
                .allow(Instant::now(), unit.limit_interval, unit.limit_burst)
            {
                self.update(&unit.name, task, |s| {
                    s.state = State::Failed;
                    s.substate = "start-limit-hit".into();
                    s.reason = Some("启动频率超过限制".into());
                });
                if let Some(tx) = started.take() {
                    let _ = tx.send(Err(Error::Operation("启动频率超过限制".into())));
                }
                self.event(&unit.name, task, "启动限流");
                return;
            }
            let instance = self.serial.fetch_add(1, Ordering::SeqCst);
            self.update(&unit.name, task, |s| {
                s.state = State::Activating;
                s.substate = "start".into();
                s.instance = instance;
                s.restart_count = restarts;
                s.reason = None;
                s.exit_code = None;
            });
            self.event(&unit.name, instance, "启动");
            let outcome = self
                .run_instance(&unit, task, instance, &mut cancel, &mut started)
                .await;
            let requested = *cancel.borrow();
            let (success, exit_code, reason) = match outcome {
                Ok(code) => (code == 0, Some(code), format!("退出码 {code}")),
                Err(e) => (false, None, e.to_string()),
            };
            if let Some(tx) = started.take() {
                let _ = tx.send(if success {
                    Ok(())
                } else {
                    Err(Error::Operation(reason.clone()))
                });
            }
            self.update(&unit.name, task, |s| {
                s.pid = None;
                s.exit_code = exit_code;
                s.reason = Some(reason.clone());
            });
            self.event(&unit.name, instance, &reason);
            if requested {
                self.update(&unit.name, task, |s| {
                    if !success && s.state == State::Deactivating {
                        s.state = State::Failed;
                        s.substate = "stop-failed".into();
                    } else {
                        s.state = State::Inactive;
                        s.substate = "dead".into();
                    }
                    s.pid = None;
                });
                return;
            }
            if !should_restart(unit.restart, success, false) {
                self.update(&unit.name, task, |s| {
                    s.state = if success {
                        State::Inactive
                    } else {
                        State::Failed
                    };
                    s.substate = if success { "dead" } else { "exit-code" }.into();
                });
                return;
            }
            restarts += 1;
            self.update(&unit.name, task, |s| {
                s.state = State::Activating;
                s.substate = "auto-restart".into();
                s.restart_count = restarts;
            });
            tokio::select! { _ = tokio::time::sleep(unit.restart_sec) => (), _ = cancel.changed() => () }
        }
        if let Some(tx) = started {
            let _ = tx.send(Err(Error::Operation("启动取消".into())));
        }
        self.update(&unit.name, task, |s| {
            s.state = State::Inactive;
            s.substate = "dead".into();
            s.pid = None;
        });
    }
    /// 执行一个服务周期。参数：配置及实例上下文、取消和首次就绪通知。返回：退出码。
    async fn run_instance(
        &self,
        unit: &Unit,
        task: u64,
        instance: u64,
        cancel: &mut watch::Receiver<bool>,
        started: &mut Option<oneshot::Sender<Result<()>>>,
    ) -> Result<u32> {
        let deadline = unit.timeout_start.map(|d| tokio::time::Instant::now() + d);
        let mut main: Option<Box<dyn Process>> = None;
        for command in &unit.exec_start {
            if *cancel.borrow() {
                return Err(Error::Operation("启动取消".into()));
            }
            let child = self
                .spawn(unit, command, instance, None, cancel, deadline)
                .await?;
            self.update(&unit.name, task, |s| s.pid = Some(child.pid()));
            if unit.service_type == ServiceType::Oneshot {
                let outcome = wait(child.as_ref(), cancel, deadline).await;
                child.terminate()?;
                let code = outcome?;
                if code != 0 {
                    return Ok(code);
                }
            } else {
                main = Some(child);
            }
        }
        if *cancel.borrow() {
            return Err(Error::Operation("启动取消".into()));
        }
        self.update(&unit.name, task, |s| {
            s.state = State::Active;
            s.substate = if main.is_some() { "running" } else { "exited" }.into();
            if main.is_none() {
                s.pid = None;
            }
        });
        if let Some(tx) = started.take() {
            let _ = tx.send(Ok(()));
        }
        let mut code = 0;
        let mut wait_error = None;
        let mut stop_failed = false;
        if let Some(child) = &main {
            match wait(child.as_ref(), cancel, None).await {
                Ok(value) => code = value,
                Err(e) => wait_error = Some(e),
            }
        } else if unit.remain_after_exit && !*cancel.borrow() {
            let _ = cancel.changed().await;
        }
        self.update(&unit.name, task, |s| {
            s.state = State::Deactivating;
            s.substate = "stop".into();
        });
        // 成功启动后，无论主动停止还是自行退出，都运行 ExecStop。
        let pid = main
            .as_ref()
            .filter(|child| child.poll().ok().flatten().is_none())
            .map(|child| child.pid());
        for command in &unit.exec_stop {
            let (_, mut independent) = watch::channel(false);
            let deadline = unit.timeout_stop.map(|d| tokio::time::Instant::now() + d);
            let result = async {
                let child = self
                    .spawn(unit, command, instance, pid, &mut independent, deadline)
                    .await?;
                let outcome = wait(child.as_ref(), &mut independent, deadline).await;
                child.terminate()?;
                if outcome? != 0 {
                    return Err(Error::Operation("ExecStop 非零退出".into()));
                }
                Ok(())
            }
            .await;
            if let Err(e) = result {
                self.event(&unit.name, instance, &format!("停止命令失败：{e}"));
                stop_failed = true;
                wait_error = Some(e);
                break;
            }
        }
        if let Some(child) = main {
            child.terminate()?;
        }
        if *cancel.borrow() && !stop_failed {
            return Ok(0);
        }
        if let Some(e) = wait_error {
            return Err(e);
        }
        Ok(code)
    }
    /// 在阻塞线程创建进程，并支持超时或取消。参数：配置、命令和上下文。返回：进程对象。
    async fn spawn(
        &self,
        unit: &Unit,
        command: &[String],
        instance: u64,
        pid: Option<u32>,
        cancel: &mut watch::Receiver<bool>,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<Box<dyn Process>> {
        let backend = self.backend.clone();
        let unit = unit.clone();
        let command = command.to_vec();
        let logger = self.logger.clone();
        let handle = tokio::task::spawn_blocking(move || {
            backend.spawn(&unit, &command, instance, pid, &logger)
        });
        tokio::select! {
            result = handle => result.map_err(crate::operation)?,
            _ = cancelled(cancel) => Err(Error::Operation("启动取消".into())),
            _ = deadline_wait(deadline) => Err(Error::Operation("启动超时".into())),
        }
    }
    /// 立即取消受停止请求影响的启动，不等待事务锁。参数：names 为反向闭包。返回：无。
    fn interrupt(&self, names: &BTreeSet<String>) {
        let pending = self.pending.lock().unwrap();
        let abort = names.iter().any(|n| pending.contains(n));
        if abort {
            self.abort.store(true, Ordering::SeqCst);
        }
        let statuses = self.statuses.lock().unwrap().clone();
        for (name, job) in self.jobs.lock().unwrap().iter() {
            if statuses
                .get(name)
                .is_some_and(|s| s.state == State::Activating)
                && (names.contains(name) || (abort && pending.contains(name)))
            {
                let _ = job.cancel.send(true);
            }
        }
    }
    /// 停止并传播反向 Requires。参数：roots 为请求 unit。返回：完成后结果。
    pub async fn stop(&self, roots: &[String]) -> Result<BTreeSet<String>> {
        let snapshots = self.snapshots();
        for root in roots {
            if !snapshots.contains_key(root) {
                return Err(Error::Operation(format!("不存在 unit：{root}")));
            }
        }
        let selected = graph::stop_set(&snapshots, roots);
        self.interrupt(&selected);
        let _guard = self.transaction.lock().await;
        let selected = graph::stop_set(&self.snapshots(), roots);
        self.stop_selected(&selected).await?;
        Ok(selected)
    }
    /// 合并加载配置和活跃实例依赖快照。参数：无。返回：定义表。
    fn snapshots(&self) -> Units {
        let mut units = self.units.read().unwrap().clone();
        for (name, job) in self.jobs.lock().unwrap().iter() {
            if !*job.done.borrow() {
                units.insert(name.clone(), job.unit.clone());
            }
        }
        units
    }
    /// 逆序停止选中 unit。参数：selected 为停止集合。返回：结果。
    async fn stop_selected(&self, selected: &BTreeSet<String>) -> Result<()> {
        let units = self.snapshots();
        let layers = graph::order(&units, selected)?;
        let mut errors = vec![];
        for layer in layers.into_iter().rev() {
            let mut waiting = vec![];
            for name in layer {
                if let Some(job) = self.jobs.lock().unwrap().get(&name).cloned() {
                    let _ = job.cancel.send(true);
                    if !*job.done.borrow() {
                        waiting.push((name, job.done));
                    }
                }
            }
            for (name, mut done) in waiting {
                while !*done.borrow() {
                    if done.changed().await.is_err() {
                        break;
                    }
                }
                if let Some(status) = self.statuses.lock().unwrap().get(&name)
                    && status.state == State::Failed
                    && status.substate == "stop-failed"
                {
                    errors.push(format!(
                        "{name}: {}",
                        status.reason.as_deref().unwrap_or("停止失败")
                    ));
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error::Operation(errors.join("；")))
        }
    }
    /// 显式重启及传播到依赖者。参数：roots 为请求 unit。返回：事务结果。
    pub async fn restart(self: &Arc<Self>, roots: &[String]) -> Result<()> {
        let snapshots = self.snapshots();
        for root in roots {
            if !snapshots.contains_key(root) {
                return Err(Error::Operation(format!("不存在 unit：{root}")));
            }
        }
        let selected = graph::stop_set(&snapshots, roots);
        self.interrupt(&selected);
        let _guard = self.transaction.lock().await;
        let selected = graph::stop_set(&self.snapshots(), roots);
        let active: BTreeSet<_> = self
            .jobs
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, job)| !*job.done.borrow())
            .map(|(name, _)| name.clone())
            .collect();
        let restart: Vec<_> = selected
            .iter()
            .filter(|name| active.contains(*name) || roots.contains(name))
            .cloned()
            .collect();
        self.stop_selected(&selected).await?;
        self.start_locked(&restart).await
    }
    /// 原子重载；运行实例继续使用旧快照。参数：无。返回：新配置版本。
    pub async fn reload(&self) -> Result<u64> {
        let _guard = self.transaction.lock().await;
        let candidate = config::load(&self.root.join("units"))?;
        self.validate_candidate(&candidate)?;
        Ok(self.apply_candidate(candidate))
    }
    /// 检查候选图与运行实例是否兼容。参数：candidate 为候选定义。返回：校验结果。
    fn validate_candidate(&self, candidate: &Units) -> Result<()> {
        // 同时校验新定义和运行快照，防止保存后产生跨版本依赖环或丢失活跃实例。
        graph::order(candidate, &candidate.keys().cloned().collect())?;
        for (name, job) in self.jobs.lock().unwrap().iter() {
            if !*job.done.borrow() && !candidate.contains_key(name) {
                return Err(Error::Config(format!("不能删除运行中的 unit：{name}")));
            }
        }
        let mut effective = candidate.clone();
        for (name, job) in self.jobs.lock().unwrap().iter() {
            if !*job.done.borrow() {
                effective.insert(name.clone(), job.unit.clone());
            }
        }
        graph::order(&effective, &effective.keys().cloned().collect())?;
        Ok(())
    }
    /// 提交已经校验的定义并递增版本。参数：candidate 为新定义。返回：版本号。
    fn apply_candidate(&self, candidate: Units) -> u64 {
        *self.units.write().unwrap() = candidate;
        let version = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.event("manager", 0, &format!("配置重载版本 {version}"));
        version
    }
    /// 保存并重载一份配置，拒绝覆盖外部修改。参数：name 为相对文件名，text 为正文，expected 为编辑时原文（新建为 None）。返回：新配置版本。
    pub async fn save_document(
        &self,
        name: &str,
        text: &str,
        expected: Option<&str>,
    ) -> Result<u64> {
        let _guard = self.transaction.lock().await;
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err(Error::Operation("管理器正在关闭".into()));
        }
        crate::desktop::validate_document_name(name)?;
        crate::desktop::validate_text(text)?;
        let directory = self.root.join("units");
        let path = crate::desktop::document_path(&directory, name)?;
        let current = crate::desktop::read_optional(&path)?;
        if current.as_deref() != expected {
            return Err(Error::Operation(
                "配置已被其他操作修改，请重新打开后再保存".into(),
            ));
        }
        // 所有解析与依赖检查在写盘前完成，失败时原文件和运行定义都保持原样。
        let candidate = config::load_override(&directory, Some((name, text)))?;
        if !candidate.contains_key(name.split(".d/").next().unwrap_or(name)) {
            return Err(Error::Config("请先创建对应的 .service 主配置".into()));
        }
        self.validate_candidate(&candidate)?;
        std::fs::create_dir_all(path.parent().unwrap())?;
        atomic_write(&path, text.as_bytes())?;
        Ok(self.apply_candidate(candidate))
    }
    /// 整包校验后导入配置并重载。参数：bundle 为配置包，overwrite 为是否允许替换同名服务。返回：新版本；磁盘失败时恢复原文。
    pub async fn import_bundle(
        &self,
        bundle: crate::desktop::ConfigBundle,
        overwrite: bool,
    ) -> Result<u64> {
        let _guard = self.transaction.lock().await;
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err(Error::Operation("管理器正在关闭".into()));
        }
        if bundle.format != "rpmm-config" || bundle.version != 1 || bundle.services.is_empty() {
            return Err(Error::Config("不支持的配置包格式或版本，或没有服务".into()));
        }
        let directory = self.root.join("units");
        let mut candidate = config::load(&directory)?;
        let mut enabled = self.enabled.lock().unwrap().clone();
        let mut changes = BTreeMap::<String, Option<String>>::new();
        let mut selected = BTreeSet::<String>::new();
        // 先在内存中合并整个包，允许同批服务相互依赖；所有路径及图检查均在写盘前完成。
        for service in bundle.services {
            config::validate_name(&service.name)?;
            if !selected.insert(service.name.to_ascii_lowercase()) {
                return Err(Error::Config("配置包含重复服务".into()));
            }
            if candidate.contains_key(&service.name) {
                if !overwrite {
                    return Err(Error::Config(format!(
                        "服务已存在：{}；请启用替换同名服务",
                        service.name
                    )));
                }
                for document in crate::desktop::documents(&self.root, &service.name)? {
                    changes.insert(document.name, None);
                }
            }
            let mut names = BTreeSet::new();
            for document in &service.documents {
                crate::desktop::document_path(&directory, &document.name)?;
                crate::desktop::validate_text(&document.text)?;
                if document.name != service.name
                    && !document.name.starts_with(&format!("{}.d/", service.name))
                {
                    return Err(Error::Config("配置文档不属于声明的服务".into()));
                }
                if !names.insert(document.name.to_ascii_lowercase()) {
                    return Err(Error::Config("配置包含重复文档".into()));
                }
                changes.insert(document.name.clone(), Some(document.text.clone()));
            }
            if !service
                .documents
                .iter()
                .any(|document| document.name == service.name)
            {
                return Err(Error::Config("配置包缺少主配置".into()));
            }
            let unit = config::parse_documents(&service.name, &service.documents)?;
            if service.enabled && unit.wanted_by.is_empty() {
                return Err(Error::Config("启用的服务缺少 WantedBy".into()));
            }
            if service.enabled {
                enabled.insert(service.name.clone());
            } else {
                enabled.remove(&service.name);
            }
            candidate.insert(service.name, unit);
        }
        let mut folded = BTreeSet::new();
        for name in candidate.keys() {
            if !folded.insert(name.to_ascii_lowercase()) {
                return Err(Error::Config("服务名称大小写冲突".into()));
            }
        }
        self.validate_candidate(&candidate)?;
        graph::start_plan(&candidate, &candidate.keys().cloned().collect::<Vec<_>>())?;
        let mut previous = BTreeMap::new();
        for name in changes.keys() {
            previous.insert(
                name.clone(),
                crate::desktop::read_optional(&crate::desktop::document_path(&directory, name)?)?,
            );
        }
        let enabled_path = self.root.join("state/enabled.json");
        let mut applied = Vec::new();
        let commit = (|| -> Result<()> {
            for (name, text) in &changes {
                let path = crate::desktop::document_path(&directory, name)?;
                if let Some(text) = text {
                    std::fs::create_dir_all(path.parent().unwrap())?;
                    atomic_write(&path, text.as_bytes())?;
                } else if path.exists() {
                    std::fs::remove_file(path)?;
                }
                applied.push(name.clone());
            }
            // 启用状态最后原子提交，提交失败时无需恢复尚未变化的启用状态。
            atomic_write(&enabled_path, &serde_json::to_vec_pretty(&enabled)?)
        })();
        if let Err(error) = commit {
            let mut errors = vec![error.to_string()];
            // 逆序恢复实际完成的修改，避免触碰导致提交失败但尚未变化的文件。
            for name in applied.iter().rev() {
                let text = previous.get(name).unwrap();
                let path = directory.join(name);
                let restored = if let Some(text) = text {
                    atomic_write(&path, text.as_bytes())
                } else if path.exists() {
                    std::fs::remove_file(path).map_err(Error::from)
                } else {
                    Ok(())
                };
                if let Err(error) = restored {
                    errors.push(format!("恢复配置失败：{error}"));
                }
            }
            return Err(Error::Operation(errors.join("；")));
        }
        *self.enabled.lock().unwrap() = enabled;
        Ok(self.apply_candidate(candidate))
    }
    /// 持久化开机成员关系，不立即启停。参数：name 和 enable 为名称及动作。返回：结果。
    pub async fn enable(&self, name: &str, enable: bool) -> Result<()> {
        let _guard = self.transaction.lock().await;
        config::validate_name(name)?;
        if enable {
            let unit = self
                .units
                .read()
                .unwrap()
                .get(name)
                .cloned()
                .ok_or_else(|| Error::Operation(format!("不存在 unit：{name}")))?;
            if unit.wanted_by.is_empty() {
                return Err(Error::Operation("缺少 WantedBy=multi-user.target".into()));
            }
        }
        let mut candidate = self.enabled.lock().unwrap().clone();
        if enable {
            candidate.insert(name.into());
        } else {
            candidate.remove(name);
        }
        atomic_write(
            &self.root.join("state/enabled.json"),
            &serde_json::to_vec_pretty(&candidate)?,
        )?;
        *self.enabled.lock().unwrap() = candidate;
        Ok(())
    }
    /// 清除失败和启动限流。参数：name 为 unit。返回：结果。
    pub async fn reset_failed(&self, name: &str) -> Result<()> {
        let _guard = self.transaction.lock().await;
        self.status(Some(name))?;
        if let Some(job) = self.jobs.lock().unwrap().get(name) {
            job.limiter.lock().unwrap().reset();
        }
        if let Some(status) = self.statuses.lock().unwrap().get_mut(name)
            && status.state == State::Failed
        {
            status.state = State::Inactive;
            status.substate = "dead".into();
            status.reason = None;
        }
        Ok(())
    }
    /// 激活持久化成员。参数：无。返回：事务结果。
    pub async fn boot(self: &Arc<Self>) -> Result<()> {
        let roots: Vec<_> = self.enabled.lock().unwrap().iter().cloned().collect();
        self.start(&roots).await
    }
    /// 禁止新启动、取消激活并关闭全部进程。参数：无。返回：结果。
    pub async fn shutdown(&self) -> Result<()> {
        self.shutting_down.store(true, Ordering::SeqCst);
        self.abort.store(true, Ordering::SeqCst);
        let selected: BTreeSet<_> = self.snapshots().keys().cloned().collect();
        // 先取消激活和重启等待，活跃实例按逆序停止。
        let states = self.statuses.lock().unwrap().clone();
        for (name, job) in self.jobs.lock().unwrap().iter() {
            if states
                .get(name)
                .is_some_and(|s| s.state == State::Activating)
            {
                let _ = job.cancel.send(true);
            }
        }
        let _guard = self.transaction.lock().await;
        self.stop_selected(&selected).await
    }
}

/// 查找监督任务对应名称。参数：manager、task 为管理器和所有权编号。返回：名称或空串。
fn self_name(manager: &Manager, task: u64) -> String {
    manager
        .jobs
        .lock()
        .unwrap()
        .iter()
        .find(|(_, j)| j.task == task)
        .map(|(n, _)| n.clone())
        .unwrap_or_default()
}

/// 等待取消且避免发送方提前关闭造成忙循环。参数：cancel 为通知。返回：无。
async fn cancelled(cancel: &mut watch::Receiver<bool>) {
    while !*cancel.borrow() {
        if cancel.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}
/// 等待有限或无限截止时间。参数：deadline 为可选截止时间。返回：无。
async fn deadline_wait(deadline: Option<tokio::time::Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
    } else {
        std::future::pending::<()>().await;
    }
}
/// 异步轮询进程。参数：child、cancel、deadline 为进程、取消和截止时间。返回：退出码。
async fn wait(
    child: &dyn Process,
    cancel: &mut watch::Receiver<bool>,
    deadline: Option<tokio::time::Instant>,
) -> Result<u32> {
    loop {
        if *cancel.borrow() {
            return Err(Error::Operation("操作取消".into()));
        }
        if let Some(code) = child.poll()? {
            return Ok(code);
        }
        tokio::select! { _ = tokio::time::sleep(Duration::from_millis(25)) => (), _ = cancelled(cancel) => return Err(Error::Operation("操作取消".into())), _ = deadline_wait(deadline) => return Err(Error::Operation("进程等待超时".into())) }
    }
}
struct PendingFile(PathBuf);

impl Drop for PendingFile {
    /// 清理未提交的写入文件。参数：无。返回：无。
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// 持久化小状态及配置文件并原子替换。参数：path、bytes 为路径和正文。返回：结果。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let temporary = path.with_file_name(format!(
        ".rpmm-{}-{}-{}.pending",
        path.file_name()
            .ok_or_else(|| Error::Operation("写入路径必须指向文件".into()))?
            .to_string_lossy(),
        std::process::id(),
        WRITES.fetch_add(1, Ordering::SeqCst)
    ));
    // 独占创建防止跟随遗留链接；隐藏的候选文件不会被配置加载器识别成 unit。
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let _cleanup = PendingFile(temporary.clone());
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    #[cfg(windows)]
    unsafe {
        use windows::{
            Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            },
            core::PCWSTR,
        };
        MoveFileExW(
            PCWSTR(crate::platform::win::wide(&temporary.to_string_lossy()).as_ptr()),
            PCWSTR(crate::platform::win::wide(&path.to_string_lossy()).as_ptr()),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
        .map_err(crate::operation)?;
    }
    #[cfg(not(windows))]
    std::fs::rename(temporary, path)?;
    Ok(())
}
