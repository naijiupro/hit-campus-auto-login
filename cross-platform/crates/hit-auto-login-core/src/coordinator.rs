use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunTrigger {
    Launch,
    Resume,
    ScreenResume,
    SessionActive,
    Manual,
}

impl RunTrigger {
    pub const fn is_automatic(self) -> bool {
        !matches!(self, Self::Manual)
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Launch => "登录启动",
            Self::Resume => "系统唤醒",
            Self::ScreenResume => "屏幕恢复",
            Self::SessionActive => "会话恢复",
            Self::Manual => "手动检测",
        }
    }
}

pub struct WorkflowCoordinator {
    running: AtomicBool,
    last_automatic: Mutex<Option<Instant>>,
    cooldown: Duration,
}

impl Default for WorkflowCoordinator {
    fn default() -> Self {
        Self::new(Duration::from_secs(20))
    }
}

impl WorkflowCoordinator {
    pub fn new(cooldown: Duration) -> Self {
        Self {
            running: AtomicBool::new(false),
            last_automatic: Mutex::new(None),
            cooldown,
        }
    }

    pub fn try_begin(&self, trigger: RunTrigger) -> Option<RunGuard<'_>> {
        if trigger.is_automatic() {
            let mut last = self.last_automatic.lock().ok()?;
            let now = Instant::now();
            if last.is_some_and(|previous| now.duration_since(previous) < self.cooldown) {
                return None;
            }
            *last = Some(now);
        }

        self.running
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| RunGuard { coordinator: self })
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Acquire)
    }
}

pub struct RunGuard<'a> {
    coordinator: &'a WorkflowCoordinator,
}

impl Drop for RunGuard<'_> {
    fn drop(&mut self) {
        self.coordinator.running.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Barrier},
        thread,
    };

    use super::*;

    #[test]
    fn concurrent_triggers_allow_only_one_run() {
        let coordinator = Arc::new(WorkflowCoordinator::new(Duration::ZERO));
        let barrier = Arc::new(Barrier::new(9));
        let successes = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut threads = Vec::new();
        for _ in 0..8 {
            let coordinator = coordinator.clone();
            let barrier = barrier.clone();
            let successes = successes.clone();
            threads.push(thread::spawn(move || {
                barrier.wait();
                if let Some(_guard) = coordinator.try_begin(RunTrigger::Manual) {
                    successes.fetch_add(1, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(30));
                }
            }));
        }
        barrier.wait();
        for thread in threads {
            thread.join().unwrap();
        }
        assert_eq!(successes.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn resume_events_are_deduplicated() {
        let coordinator = WorkflowCoordinator::new(Duration::from_secs(20));
        drop(coordinator.try_begin(RunTrigger::Resume).unwrap());
        assert!(coordinator.try_begin(RunTrigger::ScreenResume).is_none());
        assert!(coordinator.try_begin(RunTrigger::Manual).is_some());
    }
}
