//! 与平台无关的重启和限流策略。
use crate::config::Restart;
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

/// 判断退出是否重启。参数：policy 为策略，success 为是否成功，requested 为主动停止。返回：是否重启。
pub fn should_restart(policy: Restart, success: bool, requested: bool) -> bool {
    !requested
        && match policy {
            Restart::No => false,
            Restart::Always => true,
            Restart::OnFailure => !success,
            Restart::OnSuccess => success,
        }
}

#[derive(Default)]
pub struct StartLimiter {
    attempts: VecDeque<Instant>,
}
impl StartLimiter {
    /// 记录一次启动。参数：now 为单调时间，interval/burst 为窗口和上限。返回：是否允许启动。
    pub fn allow(&mut self, now: Instant, interval: Duration, burst: usize) -> bool {
        if interval.is_zero() || burst == 0 {
            return true;
        }
        while self
            .attempts
            .front()
            .is_some_and(|t| now.duration_since(*t) >= interval)
        {
            self.attempts.pop_front();
        }
        if self.attempts.len() >= burst {
            return false;
        }
        self.attempts.push_back(now);
        true
    }
    /// 清除启动计数。参数：无。返回：无。
    pub fn reset(&mut self) {
        self.attempts.clear();
    }
}
