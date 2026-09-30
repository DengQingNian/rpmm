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
                    tokio::spawn(async move { manager.start_one(unit).await }),
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
        graph::order(&candidate, &candidate.keys().cloned().collect())?;
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
        *self.units.write().unwrap() = candidate;
        let version = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        self.event("manager", 0, &format!("配置重载版本 {version}"));
        Ok(version)
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
/// 持久化小状态文件并原子替换。参数：path、bytes 为路径和正文。返回：结果。
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temporary = path.with_extension("pending");
    let mut file = std::fs::File::create(&temporary)?;
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
