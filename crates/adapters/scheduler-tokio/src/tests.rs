//! Тесты `PollLoop` с виртуальным временем tokio (`tokio::time::pause`/
//! `advance`) — ни один тест не ждёт реальные секунды. Фейки портов ниже
//! нужны только этому крейту: `application::testing` собирается лишь при
//! `cargo test -p application`, поэтому для внешнего дев-крейта он
//! недоступен как обычная зависимость.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{TimeZone, Utc};
use domain::{
    HourInterval, HourIntervalId, TelegramId, TenMinCheck, TenMinCheckId, UtcTimestamp, WorkDay,
    WorkDayId,
};

use application::{
    AdvanceOpenTenMinChecks, Clock, HourIntervalRepository, IdlePromptConfig, Notifier,
    NotifierError, OutboundMessage, PromptContinueIfIdle, ReminderScheduleConfig,
    RemindAwaitingResume, RepoError, TenMinCheckRepository, WorkDayRepository,
};

use crate::PollLoop;

#[derive(Default)]
struct PollCounters {
    find_open: AtomicUsize,
    find_awaiting_resume: AtomicUsize,
    find_open_without_active_interval: AtomicUsize,
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> UtcTimestamp {
        UtcTimestamp::new(Utc.with_ymd_and_hms(2026, 8, 22, 12, 0, 0).unwrap())
    }
}

struct NoopNotifier;

#[async_trait]
impl Notifier for NoopNotifier {
    async fn send(&self, _user: TelegramId, _message: OutboundMessage) -> Result<(), NotifierError> {
        Ok(())
    }
}

struct CountingTenMinCheckRepository {
    counters: Arc<PollCounters>,
}

#[async_trait]
impl TenMinCheckRepository for CountingTenMinCheckRepository {
    async fn create(&self, _check: TenMinCheck) -> Result<TenMinCheck, RepoError> {
        unreachable!("PollLoop use cases never create ten-min checks")
    }

    async fn update(&self, _check: TenMinCheck) -> Result<(), RepoError> {
        unreachable!("no open/awaiting checks in these tests, nothing to update")
    }

    async fn find_by_id(&self, _id: TenMinCheckId) -> Result<Option<TenMinCheck>, RepoError> {
        unreachable!("not queried by PollLoop's use cases")
    }

    async fn find_open_by_interval(
        &self,
        _hour_interval_id: HourIntervalId,
    ) -> Result<Option<TenMinCheck>, RepoError> {
        unreachable!("not queried by PollLoop's use cases")
    }

    async fn find_last_by_interval(
        &self,
        _hour_interval_id: HourIntervalId,
    ) -> Result<Option<TenMinCheck>, RepoError> {
        unreachable!("not queried by PollLoop's use cases")
    }

    async fn list_by_interval(
        &self,
        _hour_interval_id: HourIntervalId,
    ) -> Result<Vec<TenMinCheck>, RepoError> {
        unreachable!("no open checks in these tests, siblings are never listed")
    }

    async fn find_open(&self) -> Result<Vec<TenMinCheck>, RepoError> {
        self.counters.find_open.fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }

    async fn find_awaiting_resume(&self) -> Result<Vec<TenMinCheck>, RepoError> {
        self.counters
            .find_awaiting_resume
            .fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }
}

struct CountingWorkDayRepository {
    counters: Arc<PollCounters>,
}

#[async_trait]
impl WorkDayRepository for CountingWorkDayRepository {
    async fn create(&self, _work_day: WorkDay) -> Result<WorkDay, RepoError> {
        unreachable!("PollLoop use cases never create work days")
    }

    async fn update(&self, _work_day: WorkDay) -> Result<(), RepoError> {
        unreachable!("no idle work days in these tests, nothing to update")
    }

    async fn find_by_id(&self, _id: WorkDayId) -> Result<Option<WorkDay>, RepoError> {
        unreachable!("not queried by PollLoop's use cases")
    }

    async fn find_open_by_user(&self, _user_id: TelegramId) -> Result<Option<WorkDay>, RepoError> {
        unreachable!("not queried by PollLoop's use cases")
    }

    async fn list_by_user(&self, _user_id: TelegramId) -> Result<Vec<WorkDay>, RepoError> {
        unreachable!("not queried by PollLoop's use cases")
    }

    async fn find_open_without_active_interval(&self) -> Result<Vec<WorkDay>, RepoError> {
        self.counters
            .find_open_without_active_interval
            .fetch_add(1, Ordering::SeqCst);
        Ok(Vec::new())
    }
}

struct UnusedHourIntervalRepository;

#[async_trait]
impl HourIntervalRepository for UnusedHourIntervalRepository {
    async fn create(&self, _interval: HourInterval) -> Result<HourInterval, RepoError> {
        unreachable!("PollLoop use cases never create hour intervals")
    }

    async fn update(&self, _interval: HourInterval) -> Result<(), RepoError> {
        unreachable!("not needed with no open/awaiting checks in these tests")
    }

    async fn find_by_id(&self, _id: HourIntervalId) -> Result<Option<HourInterval>, RepoError> {
        unreachable!("not queried by PollLoop's use cases")
    }

    async fn find_open_by_work_day(
        &self,
        _work_day_id: WorkDayId,
    ) -> Result<Option<HourInterval>, RepoError> {
        unreachable!("not queried by PollLoop's use cases")
    }

    async fn list_by_work_day(
        &self,
        _work_day_id: WorkDayId,
    ) -> Result<Vec<HourInterval>, RepoError> {
        unreachable!("not queried by PollLoop's use cases")
    }
}

fn build_poll_loop(counters: Arc<PollCounters>, poll_interval: StdDuration) -> PollLoop {
    let ten_min_check_repository = Arc::new(CountingTenMinCheckRepository {
        counters: counters.clone(),
    });
    let hour_interval_repository = Arc::new(UnusedHourIntervalRepository);
    let work_day_repository = Arc::new(CountingWorkDayRepository {
        counters: counters.clone(),
    });
    let clock = Arc::new(FixedClock);
    let notifier = Arc::new(NoopNotifier);

    let advance_open_ten_min_checks = Arc::new(AdvanceOpenTenMinChecks::new(
        ten_min_check_repository.clone(),
        hour_interval_repository.clone(),
        work_day_repository.clone(),
        clock.clone(),
        notifier.clone(),
        ReminderScheduleConfig::default(),
    ));
    let remind_awaiting_resume = Arc::new(RemindAwaitingResume::new(
        ten_min_check_repository,
        hour_interval_repository.clone(),
        work_day_repository.clone(),
        clock.clone(),
        notifier.clone(),
        ReminderScheduleConfig::default(),
    ));
    let prompt_continue_if_idle = Arc::new(PromptContinueIfIdle::new(
        work_day_repository,
        hour_interval_repository,
        clock,
        notifier,
        IdlePromptConfig::default(),
    ));

    PollLoop::new(
        advance_open_ten_min_checks,
        remind_awaiting_resume,
        prompt_continue_if_idle,
        poll_interval,
    )
}

#[tokio::test]
async fn single_tick_calls_all_three_use_cases_once() {
    let counters = Arc::new(PollCounters::default());
    let poll_loop = build_poll_loop(counters.clone(), StdDuration::from_secs(60));

    poll_loop.tick().await;

    assert_eq!(counters.find_open.load(Ordering::SeqCst), 1);
    assert_eq!(counters.find_awaiting_resume.load(Ordering::SeqCst), 1);
    assert_eq!(
        counters.find_open_without_active_interval.load(Ordering::SeqCst),
        1
    );
}

/// Продвигает виртуальное время на несколько периодов опроса вперёд и
/// проверяет, что `run()` вызвал use cases ровно столько раз, сколько было
/// тиков — без ожидания реальных секунд (`tokio::time::pause`/`advance`).
#[tokio::test(start_paused = true)]
async fn run_polls_on_every_tick_of_virtual_time() {
    let counters = Arc::new(PollCounters::default());
    let poll_interval = StdDuration::from_secs(60);
    let poll_loop = Arc::new(build_poll_loop(counters.clone(), poll_interval));

    let handle = tokio::spawn({
        let poll_loop = poll_loop.clone();
        async move { poll_loop.run().await }
    });

    const TICKS: usize = 5;
    for _ in 0..TICKS {
        tokio::time::advance(poll_interval).await;
    }
    tokio::task::yield_now().await;

    handle.abort();

    assert_eq!(counters.find_open.load(Ordering::SeqCst), TICKS);
    assert_eq!(counters.find_awaiting_resume.load(Ordering::SeqCst), TICKS);
    assert_eq!(
        counters.find_open_without_active_interval.load(Ordering::SeqCst),
        TICKS
    );
}

/// Эмулирует рестарт процесса: второй, полностью независимый `PollLoop` (без
/// какого-либо состояния, унаследованного от первого) тикает поверх тех же
/// данных и ведёт себя идентично — подтверждает restart-safety на уровне
/// этого адаптера (README.md, принцип таймеров: вся нужная информация
/// перечитывается из БД, а не хранится в цикле).
#[tokio::test]
async fn fresh_poll_loop_after_restart_behaves_like_continuous_one() {
    let counters = Arc::new(PollCounters::default());

    let first_run = build_poll_loop(counters.clone(), StdDuration::from_secs(60));
    first_run.tick().await;
    first_run.tick().await;

    // "Рестарт процесса": прежний `PollLoop` отброшен, создаётся новый
    // экземпляр без какой-либо связи с предыдущим.
    drop(first_run);
    let after_restart = build_poll_loop(counters.clone(), StdDuration::from_secs(60));
    after_restart.tick().await;

    assert_eq!(counters.find_open.load(Ordering::SeqCst), 3);
    assert_eq!(counters.find_awaiting_resume.load(Ordering::SeqCst), 3);
    assert_eq!(
        counters.find_open_without_active_interval.load(Ordering::SeqCst),
        3
    );
}
