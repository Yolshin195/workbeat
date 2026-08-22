//! Задача 14 корневого `tasks.md`: приёмочные сценарии MVP, прогоняемые
//! end-to-end через `application`-слой поверх реальной tempfile SQLite (те же
//! репозитории и тот же composition root `app::wiring::wire`, что использует
//! продовый бинарь, см. Задачу 13/`tests/restart_safety.rs`), с управляемым
//! `Clock` и записывающим `Notifier`. Никакого mock-планировщика не нужно —
//! таймеры полностью выражены через данные в БД (README.md раздел 4):
//! сценарии просто двигают часы вперёд и либо вызывают use cases напрямую
//! (явные действия пользователя), либо дёргают `PollLoop::tick()` (то, что в
//! проде делает периодический опрос).
//!
//! Нумерация тестов соответствует нумерации сценариев в `tasks.md` (Задача
//! 14, раздел "Сценарии (минимум)"). Сценарий 15 (рестарт процесса)
//! дополняет уже существующий `tests/restart_safety.rs` (написан для Задачи
//! 13) двумя состояниями, которые тот файл не покрывает — не дублирует его.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use async_trait::async_trait;
use chrono::{Duration, NaiveDate, TimeZone, Utc};

use adapters_persistence_sqlite::{
    SqliteHourIntervalRepository, SqliteTaskRepository, SqliteTenMinCheckRepository,
    SqliteUserRepository, SqliteWorkDayRepository,
};
use adapters_scheduler_tokio::PollLoop;
use application::{
    Clock, HourIntervalRepository, IdlePromptConfig, Notifier, NotifierError, OutboundMessage,
    Period, ReminderScheduleConfig, TaskFilter, TaskRepository, TaskTimeEntry,
    TenMinCheckRepository, UserRepository, WorkDayRepository,
};
use domain::{
    day_overrun, interval_overrun, CheckStatus, HourInterval, HourIntervalId, SuccessfulCount,
    Task, TaskId, TaskPriority, TaskStatus, TelegramId, TenMinCheck, TenMinCheckAnswer,
    TenMinCheckId, TimeZoneOffset, User, UtcTimestamp, WorkDay, WorkDayId,
};

use app::wiring::{wire, Adapters};

// --- Тестовая инфраструктура --------------------------------------------

/// Временный файл SQLite — по одному на тест, удаляется при `Drop` (тот же
/// приём, что и в `tests/restart_safety.rs`).
struct TempDbFile {
    path: PathBuf,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

impl TempDbFile {
    fn new() -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let path = std::env::temp_dir().join(format!("workbeat-e2e-{pid}-{unique}.sqlite"));
        Self { path }
    }

    fn url(&self) -> String {
        format!("sqlite://{}", self.path.display())
    }
}

impl Drop for TempDbFile {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm", "-journal"] {
            let mut path = self.path.clone().into_os_string();
            path.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(path));
        }
    }
}

/// Управляемое "сейчас" — единственный способ продвигать время в этих тестах.
/// README.md раздел 4: любой таймер — это `now() - started_at`, поэтому
/// сценарии просто двигают часы и вызывают use cases/`PollLoop::tick`.
struct TestClock(Mutex<UtcTimestamp>);

impl TestClock {
    fn new(now: UtcTimestamp) -> Self {
        Self(Mutex::new(now))
    }

    fn advance(&self, delta: Duration) {
        let mut guard = self.0.lock().unwrap();
        *guard = UtcTimestamp::new(guard.value() + delta);
    }
}

impl Clock for TestClock {
    fn now(&self) -> UtcTimestamp {
        *self.0.lock().unwrap()
    }
}

#[derive(Default)]
struct RecordingNotifier {
    sent: Mutex<Vec<(TelegramId, OutboundMessage)>>,
}

impl RecordingNotifier {
    fn all(&self) -> Vec<(TelegramId, OutboundMessage)> {
        self.sent.lock().unwrap().clone()
    }

    /// Только текстовые сообщения, в порядке отправки — большинство сценариев
    /// проверяют именно текстовые проактивные уведомления.
    fn texts(&self) -> Vec<String> {
        self.all()
            .into_iter()
            .filter_map(|(_, message)| match message {
                OutboundMessage::Text(text) => Some(text),
                OutboundMessage::Document { .. } => None,
            })
            .collect()
    }
}

#[async_trait]
impl Notifier for RecordingNotifier {
    async fn send(&self, user: TelegramId, message: OutboundMessage) -> Result<(), NotifierError> {
        self.sent.lock().unwrap().push((user, message));
        Ok(())
    }
}

/// Собранная система (композиция из Задачи 13, `wire`) поверх tempfile SQLite
/// с управляемыми `Clock`/`Notifier`, плюс прямой доступ к репозиториям для
/// сборки фикстур и проверок, недостижимых через use cases телеграм-адаптера.
struct Harness {
    _db: TempDbFile,
    clock: Arc<TestClock>,
    notifier: Arc<RecordingNotifier>,
    uc: Arc<adapters_telegram::UseCases>,
    poll_loop: PollLoop,
    users: SqliteUserRepository,
    tasks: SqliteTaskRepository,
    work_days: SqliteWorkDayRepository,
    intervals: SqliteHourIntervalRepository,
    checks: SqliteTenMinCheckRepository,
}

impl Harness {
    async fn new() -> Self {
        Self::with(
            UtcTimestamp::new(Utc::now()),
            IdlePromptConfig::default(),
            ReminderScheduleConfig::default(),
        )
        .await
    }

    async fn with(
        now: UtcTimestamp,
        idle_prompt: IdlePromptConfig,
        reminder_schedule: ReminderScheduleConfig,
    ) -> Self {
        let db = TempDbFile::new();
        let pool = adapters_persistence_sqlite::connect(&db.url())
            .await
            .expect("connect tempfile sqlite db");
        let clock = Arc::new(TestClock::new(now));
        let notifier = Arc::new(RecordingNotifier::default());

        let users = SqliteUserRepository::new(pool.clone());
        let tasks = SqliteTaskRepository::new(pool.clone());
        let work_days = SqliteWorkDayRepository::new(pool.clone());
        let intervals = SqliteHourIntervalRepository::new(pool.clone());
        let checks = SqliteTenMinCheckRepository::new(pool.clone());

        let wired = wire(Adapters {
            pool,
            clock: clock.clone() as Arc<dyn Clock>,
            notifier: notifier.clone() as Arc<dyn Notifier>,
            idle_prompt,
            reminder_schedule,
            poll_interval: StdDuration::from_secs(60),
        });

        Self {
            _db: db,
            clock,
            notifier,
            uc: wired.telegram_use_cases,
            poll_loop: wired.poll_loop,
            users,
            tasks,
            work_days,
            intervals,
            checks,
        }
    }

    fn advance(&self, minutes: i64) {
        self.clock.advance(Duration::minutes(minutes));
    }

    fn now(&self) -> UtcTimestamp {
        self.clock.now()
    }

    async fn register_and_start_day(&self, telegram_id: i64) -> (TelegramId, WorkDayId) {
        let user = self
            .uc
            .register_user
            .execute(TelegramId::new(telegram_id).unwrap())
            .await
            .unwrap();
        let work_day = self.uc.start_work_day.execute(user.telegram_id()).await.unwrap();
        (user.telegram_id(), work_day.id())
    }

    async fn create_task(&self, user_id: TelegramId, title: &str) -> TaskId {
        self.uc
            .create_task
            .execute(user_id, title, None, None)
            .await
            .unwrap()
            .id()
    }

    /// Полный "успешный" часовой интервал: 5 подряд `Worked`, номинальный
    /// (ровно 10 минут) отдых, закрытие с `summary`. Часы двигаются ровно на
    /// номинальные 10 минут на каждом шаге — итоговая фактическая
    /// длительность интервала ровно 60 минут (перерасход = 0).
    async fn run_full_interval(
        &self,
        work_day_id: WorkDayId,
        task_id: TaskId,
        summary: &str,
    ) -> HourIntervalId {
        let interval = self
            .uc
            .start_hour_interval
            .execute(work_day_id, task_id)
            .await
            .unwrap();
        for _ in 0..5 {
            self.advance(10);
            self.uc
                .submit_ten_min_answer
                .execute(interval.id(), TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();
        }
        self.advance(10);
        self.uc.mark_returned_from_rest.execute(interval.id()).await.unwrap();
        self.uc
            .finish_hour_interval
            .execute(interval.id(), summary.to_string())
            .await
            .unwrap();
        interval.id()
    }
}

fn utc_date(year: i32, month: u32, day: u32, hour: u32) -> UtcTimestamp {
    UtcTimestamp::new(Utc.with_ymd_and_hms(year, month, day, hour, 0, 0).unwrap())
}

fn new_ready_task(user_id: TelegramId, title: &str, now: UtcTimestamp) -> Task {
    Task::new(TaskId::new(0), user_id, title, TaskStatus::Ready, None, None, now).unwrap()
}

/// Засевает напрямую через репозитории (в обход use cases — как и
/// `tests/restart_safety.rs`) уже закрытый рабочий день из `hours_worked`
/// полностью успешных часовых интервалов (каждый — 5×`Worked` + отдых,
/// `successful_count = 5`), весь на одну задачу. Нужен сценарию 11, где
/// важны только персистентные данные для агрегированного отчёта, а не путь
/// их получения через бота.
async fn seed_closed_work_day(
    h: &Harness,
    user_id: TelegramId,
    task_id: TaskId,
    started_at: UtcTimestamp,
    hours_worked: u8,
) -> WorkDayId {
    let finished_at = UtcTimestamp::new(started_at.value() + Duration::hours(i64::from(hours_worked)));
    let work_day = h
        .work_days
        .create(WorkDay::new(
            WorkDayId::new(0),
            user_id,
            started_at,
            Some(finished_at),
            None,
            None,
            None,
        ))
        .await
        .unwrap();

    for hour in 0..hours_worked {
        let interval_start = UtcTimestamp::new(started_at.value() + Duration::hours(i64::from(hour)));
        let interval_end = UtcTimestamp::new(interval_start.value() + Duration::hours(1));
        let interval = h
            .intervals
            .create(HourInterval::new(
                HourIntervalId::new(0),
                work_day.id(),
                interval_start,
                Some(interval_end),
                SuccessfulCount::new(5).unwrap(),
                Some("done".to_string()),
            ))
            .await
            .unwrap();

        for slot in 0..5 {
            let check_start =
                UtcTimestamp::new(interval_start.value() + Duration::minutes(i64::from(slot) * 10));
            let check_end = UtcTimestamp::new(check_start.value() + Duration::minutes(10));
            h.checks
                .create(TenMinCheck::new(
                    TenMinCheckId::new(0),
                    interval.id(),
                    task_id,
                    check_start,
                    Some(check_end),
                    CheckStatus::Worked,
                    None,
                    None,
                ))
                .await
                .unwrap();
        }
        let rest_start = UtcTimestamp::new(interval_start.value() + Duration::minutes(50));
        let rest_end = UtcTimestamp::new(rest_start.value() + Duration::minutes(10));
        h.checks
            .create(TenMinCheck::new(
                TenMinCheckId::new(0),
                interval.id(),
                task_id,
                rest_start,
                Some(rest_end),
                CheckStatus::Worked,
                None,
                None,
            ))
            .await
            .unwrap();
    }

    work_day.id()
}

// --- Сценарий 1: регистрация нового пользователя ------------------------

/// tasks.md, Задача 14, сценарий 1: регистрация нового пользователя первым
/// любым сообщением — идемпотентна.
#[tokio::test]
async fn scenario_01_registers_new_user_idempotently() {
    let h = Harness::new().await;
    let telegram_id = TelegramId::new(1001).unwrap();

    let first = h.uc.register_user.execute(telegram_id).await.unwrap();
    let second = h.uc.register_user.execute(telegram_id).await.unwrap();

    assert_eq!(first.telegram_id(), telegram_id);
    assert_eq!(first, second, "repeated registration must not create a duplicate");
    assert_eq!(
        h.users.find_by_telegram_id(telegram_id).await.unwrap().unwrap(),
        first
    );
}

// --- Сценарий 2: полный рабочий день, 8/8, норма выполнена --------------

/// tasks.md, Задача 14, сценарий 2: 8 часовых интервалов, все 5/5 — отдых
/// считается, норма 8ч выполнена.
#[tokio::test]
async fn scenario_02_full_work_day_all_intervals_5_of_5_meets_norm() {
    let h = Harness::new().await;
    let (user_id, work_day_id) = h.register_and_start_day(2002).await;
    let task_id = h.create_task(user_id, "Write the report").await;

    for i in 0..8 {
        h.run_full_interval(work_day_id, task_id, &format!("interval {i}")).await;
    }

    let summary = h.uc.finish_work_day.execute(work_day_id).await.unwrap();
    assert_eq!(summary.closed_intervals, 8);
    assert!(summary.norm_met);
    assert_eq!(summary.task_totals, vec![(task_id, Duration::hours(8))]);
    assert_eq!(summary.lunch_duration, None);
}

// --- Сценарий 3: провалы вперемешку с успехами ---------------------------

/// tasks.md, Задача 14, сценарий 3: 2 провальные десятиминутки вперемешку с
/// 5 успешными (7 десятиминуток на интервал) — рабочий этап всё равно
/// завершается на 5-й успешной, отдых начисляется как обычно, а фактическая
/// длительность интервала заметно больше номинальных 60 минут.
#[tokio::test]
async fn scenario_03_interval_with_interleaved_failures_still_closes_at_fifth_success() {
    let h = Harness::new().await;
    let (user_id, work_day_id) = h.register_and_start_day(2003).await;
    let task_id = h.create_task(user_id, "Ship the feature").await;

    let interval = h.uc.start_hour_interval.execute(work_day_id, task_id).await.unwrap();

    // Worked, Failed(+confirm), Worked, Worked, Failed(+confirm), Worked, Worked.
    h.advance(10);
    h.uc
        .submit_ten_min_answer
        .execute(interval.id(), TenMinCheckAnswer::Yes, None)
        .await
        .unwrap();

    h.advance(10);
    h.uc
        .submit_ten_min_answer
        .execute(interval.id(), TenMinCheckAnswer::No, Some("got a call".to_string()))
        .await
        .unwrap();
    h.advance(3);
    h.uc.confirm_ready_to_continue.execute(interval.id()).await.unwrap();

    h.advance(10);
    h.uc
        .submit_ten_min_answer
        .execute(interval.id(), TenMinCheckAnswer::Yes, None)
        .await
        .unwrap();
    h.advance(10);
    h.uc
        .submit_ten_min_answer
        .execute(interval.id(), TenMinCheckAnswer::Yes, None)
        .await
        .unwrap();

    h.advance(10);
    h.uc
        .submit_ten_min_answer
        .execute(interval.id(), TenMinCheckAnswer::No, Some("distracted".to_string()))
        .await
        .unwrap();
    h.advance(4);
    h.uc.confirm_ready_to_continue.execute(interval.id()).await.unwrap();

    h.advance(10);
    h.uc
        .submit_ten_min_answer
        .execute(interval.id(), TenMinCheckAnswer::Yes, None)
        .await
        .unwrap();

    let rest_messages_before = h.notifier.texts().iter().filter(|t| t.contains("отдыхай")).count();
    h.advance(10);
    let fifth = h
        .uc
        .submit_ten_min_answer
        .execute(interval.id(), TenMinCheckAnswer::Yes, None)
        .await
        .unwrap();
    assert_eq!(fifth.status(), CheckStatus::Worked);
    let rest_messages_after = h.notifier.texts().iter().filter(|t| t.contains("отдыхай")).count();
    assert_eq!(
        rest_messages_after,
        rest_messages_before + 1,
        "the work stage must close exactly on the fifth successful slot"
    );

    h.advance(10);
    h.uc.mark_returned_from_rest.execute(interval.id()).await.unwrap();
    let finished = h
        .uc
        .finish_hour_interval
        .execute(interval.id(), "shipped despite interruptions".to_string())
        .await
        .unwrap();

    assert_eq!(finished.successful_count().value(), 5);
    let actual_duration = finished.ended_at().unwrap().value() - finished.started_at().value();
    let overrun = interval_overrun(actual_duration);
    assert!(overrun > Duration::zero(), "overrun should be positive, got {overrun:?}");

    let report = h.uc.build_day_report.execute(work_day_id).await.unwrap();
    assert_eq!(
        report.task_totals,
        vec![TaskTimeEntry {
            task_id,
            task_title: "Ship the feature".to_string(),
            duration: Duration::minutes(60), // 5×10 успешных + 10 отдых, номинально
        }]
    );
    assert_eq!(report.failed_checks.len(), 2);
}

// --- Сценарий 4: "нет ответа" -------------------------------------------

/// tasks.md, Задача 14, сценарий 4: молчание проваливает ровно один слот
/// (`NoResponse`, `reason = None`, номинальная длительность), час не
/// сбрасывается; поздний отклик дописывает `reason` в уже закрытую строку, а
/// `ConfirmReadyToContinue` стартует новую десятиминутку с `started_at`,
/// равным моменту подтверждения (не моменту исходного закрытия).
#[tokio::test]
async fn scenario_04_silence_fails_exactly_one_slot_then_late_response_resumes() {
    let h = Harness::new().await;
    let (user_id, work_day_id) = h.register_and_start_day(2004).await;
    let task_id = h.create_task(user_id, "Investigate the bug").await;

    let interval = h.uc.start_hour_interval.execute(work_day_id, task_id).await.unwrap();
    let started_at = h.now();

    h.advance(10);
    h.poll_loop.tick().await;
    assert!(h.notifier.texts().iter().any(|t| t == "Ты работаешь?"));

    h.advance(1);
    h.poll_loop.tick().await;

    let closed = h.checks.list_by_interval(interval.id()).await.unwrap();
    assert_eq!(closed.len(), 1, "no cascade: exactly one failed slot");
    assert_eq!(closed[0].status(), CheckStatus::NoResponse);
    assert_eq!(closed[0].reason(), None);
    assert_eq!(
        closed[0].ended_at(),
        Some(UtcTimestamp::new(started_at.value() + Duration::minutes(10))),
        "closes at the nominal 10 minutes, not the real elapsed time"
    );
    assert!(h.checks.find_open().await.unwrap().is_empty());

    h.uc
        .submit_failure_reason
        .execute(interval.id(), "was in a meeting".to_string())
        .await
        .unwrap();
    let after_reason = h.checks.find_last_by_interval(interval.id()).await.unwrap().unwrap();
    assert_eq!(after_reason.reason(), Some("was in a meeting"));
    assert_eq!(after_reason.status(), CheckStatus::NoResponse);
    assert_eq!(after_reason.ended_at(), closed[0].ended_at(), "ended_at is untouched");

    h.advance(7);
    let confirm_at = h.now();
    let resumed = h.uc.confirm_ready_to_continue.execute(interval.id()).await.unwrap();
    assert_eq!(resumed.started_at(), confirm_at);
    assert_ne!(resumed.started_at(), after_reason.ended_at().unwrap());
    assert_eq!(resumed.ended_at(), None);
}

// --- Сценарий 5: переработанный отдых ------------------------------------

/// tasks.md, Задача 14, сценарий 5: после 5/5 пользователь не откликается на
/// "отдыхай 10 минут" вовремя — отдых остаётся открытым, бот напоминает по
/// расписанию, фактическая длительность (30 минут вместо 10) фиксируется по
/// `MarkReturnedFromRest`; это не провал и не влияет на `successful_count`.
#[tokio::test]
async fn scenario_05_overworked_rest_is_not_a_failure() {
    let h = Harness::new().await;
    let (user_id, work_day_id) = h.register_and_start_day(2005).await;
    let task_id = h.create_task(user_id, "Deep work block").await;

    let interval = h.uc.start_hour_interval.execute(work_day_id, task_id).await.unwrap();
    for _ in 0..5 {
        h.advance(10);
        h.uc
            .submit_ten_min_answer
            .execute(interval.id(), TenMinCheckAnswer::Yes, None)
            .await
            .unwrap();
    }
    let rest_started_at = h.now();
    assert!(h.notifier.texts().iter().any(|t| t.contains("отдыхай")));

    h.advance(11);
    h.poll_loop.tick().await;
    let reminders_after_first = h
        .notifier
        .texts()
        .iter()
        .filter(|t| t.contains("Перерыв затянулся"))
        .count();
    assert_eq!(reminders_after_first, 1);

    h.advance(19); // итого 30 минут с начала отдыха
    h.poll_loop.tick().await;
    let reminders_after_more_time = h
        .notifier
        .texts()
        .iter()
        .filter(|t| t.contains("Перерыв затянулся"))
        .count();
    assert!(reminders_after_more_time >= 2, "reminders keep coming while overworking the rest");

    let returned = h.uc.mark_returned_from_rest.execute(interval.id()).await.unwrap();
    assert_eq!(
        returned.ended_at(),
        Some(UtcTimestamp::new(rest_started_at.value() + Duration::minutes(30)))
    );
    assert_eq!(returned.reason(), None);
    assert_ne!(returned.status(), CheckStatus::Failed);
    assert_ne!(returned.status(), CheckStatus::NoResponse);

    let finished = h
        .uc
        .finish_hour_interval
        .execute(interval.id(), "long but productive".to_string())
        .await
        .unwrap();
    assert_eq!(finished.successful_count().value(), 5, "overworked rest does not affect successful_count");
}

// --- Сценарий 6: смена задачи посередине интервала -----------------------

/// tasks.md, Задача 14, сценарий 6: время корректно разбито между двумя
/// задачами через `task_id` на отдельных `TenMinCheck`.
#[tokio::test]
async fn scenario_06_switching_task_mid_interval_splits_time_correctly() {
    let h = Harness::new().await;
    let (user_id, work_day_id) = h.register_and_start_day(2006).await;
    let task_a = h.create_task(user_id, "Task A").await;
    let task_b = h.create_task(user_id, "Task B").await;

    let interval = h.uc.start_hour_interval.execute(work_day_id, task_a).await.unwrap();

    for _ in 0..2 {
        h.advance(10);
        h.uc
            .submit_ten_min_answer
            .execute(interval.id(), TenMinCheckAnswer::Yes, None)
            .await
            .unwrap();
    }

    // Десятиминутка #3 сейчас открыта под задачей A.
    let open_before_switch = h.checks.find_open_by_interval(interval.id()).await.unwrap().unwrap();
    assert_eq!(open_before_switch.task_id(), task_a);

    let switched = h.uc.switch_task_mid_interval.execute(interval.id(), task_b).await.unwrap();
    assert_eq!(switched.id(), task_b);
    assert_eq!(switched.status(), TaskStatus::InProgress);
    let task_a_after_switch = h.tasks.find_by_id(task_a).await.unwrap().unwrap();
    assert_eq!(task_a_after_switch.status(), TaskStatus::Done);

    let still_open = h.checks.find_open_by_interval(interval.id()).await.unwrap().unwrap();
    assert_eq!(
        still_open.task_id(),
        task_a,
        "the already-open check is not interrupted and keeps its original task"
    );

    for _ in 0..3 {
        h.advance(10);
        h.uc
            .submit_ten_min_answer
            .execute(interval.id(), TenMinCheckAnswer::Yes, None)
            .await
            .unwrap();
    }
    h.advance(10);
    h.uc.mark_returned_from_rest.execute(interval.id()).await.unwrap();
    h.uc
        .finish_hour_interval
        .execute(interval.id(), "switched tasks halfway".to_string())
        .await
        .unwrap();

    let report = h.uc.build_day_report.execute(work_day_id).await.unwrap();
    let mut totals: Vec<(TaskId, Duration)> =
        report.task_totals.iter().map(|entry| (entry.task_id, entry.duration)).collect();
    totals.sort_by_key(|(id, _)| id.value());
    assert_eq!(
        totals,
        vec![(task_a, Duration::minutes(30)), (task_b, Duration::minutes(30))],
        "3 worked slots stay with A (30 min), 2 worked + rest go to B (30 min)"
    );
}

// --- Сценарий 7: обед вручную и по предложению ---------------------------

/// tasks.md, Задача 14, сценарий 7: обед вручную посреди дня + обед по
/// предложению ровно после 4-го закрытого интервала.
#[tokio::test]
async fn scenario_07_manual_lunch_and_suggestion_after_fourth_interval() {
    let h = Harness::new().await;
    let (user_id, work_day_id) = h.register_and_start_day(2007).await;
    let task_id = h.create_task(user_id, "Routine task").await;

    // Обед нельзя начать во время активного незакрытого интервала.
    let interval = h.uc.start_hour_interval.execute(work_day_id, task_id).await.unwrap();
    let err = h.uc.start_lunch.execute(work_day_id).await.unwrap_err();
    assert_eq!(err, application::StartLunchError::ActiveIntervalOpen);
    for _ in 0..5 {
        h.advance(10);
        h.uc
            .submit_ten_min_answer
            .execute(interval.id(), TenMinCheckAnswer::Yes, None)
            .await
            .unwrap();
    }
    h.advance(10);
    h.uc.mark_returned_from_rest.execute(interval.id()).await.unwrap();
    h.uc.finish_hour_interval.execute(interval.id(), "1".to_string()).await.unwrap();
    h.uc.suggest_lunch_after_nth_interval.execute(work_day_id).await.unwrap();
    assert!(!h.notifier.texts().iter().any(|t| t.contains("не пора ли пообедать")));

    // Обед вручную между интервалами.
    let lunch_started = h.uc.start_lunch.execute(work_day_id).await.unwrap();
    assert!(lunch_started.lunch_started_at().is_some());
    h.advance(30);
    let lunch_ended = h.uc.end_lunch.execute(work_day_id).await.unwrap();
    assert!(lunch_ended.lunch_ended_at().is_some());

    for i in 2..=4 {
        h.run_full_interval(work_day_id, task_id, &format!("interval {i}")).await;
        h.uc.suggest_lunch_after_nth_interval.execute(work_day_id).await.unwrap();
        let suggested = h.notifier.texts().iter().any(|t| t.contains("не пора ли пообедать"));
        if i == 4 {
            assert!(suggested, "must suggest lunch right after the 4th closed interval");
        } else {
            assert!(!suggested, "must not suggest lunch before the 4th closed interval");
        }
    }
}

// --- Сценарий 8: /report посреди дня и /finish_day -----------------------

/// tasks.md, Задача 14, сценарий 8: `/report` в середине дня (включая
/// незавершённый интервал) и `/finish_day` — числа совпадают.
#[tokio::test]
async fn scenario_08_mid_day_report_matches_finish_day_numbers() {
    let h = Harness::new().await;
    let (user_id, work_day_id) = h.register_and_start_day(2008).await;
    let task_id = h.create_task(user_id, "Steady work").await;

    h.run_full_interval(work_day_id, task_id, "first hour").await;
    h.run_full_interval(work_day_id, task_id, "second hour").await;

    // Третий интервал — только 2 успешных, ещё не завершён.
    let open_interval = h.uc.start_hour_interval.execute(work_day_id, task_id).await.unwrap();
    for _ in 0..2 {
        h.advance(10);
        h.uc
            .submit_ten_min_answer
            .execute(open_interval.id(), TenMinCheckAnswer::Yes, None)
            .await
            .unwrap();
    }

    let mid_day_report = h.uc.build_day_report.execute(work_day_id).await.unwrap();
    assert_eq!(mid_day_report.day.unwrap().closed_intervals, 2, "the open interval doesn't count toward the norm yet");
    assert_eq!(
        mid_day_report.task_totals,
        vec![TaskTimeEntry {
            task_id,
            task_title: "Steady work".to_string(),
            duration: Duration::minutes(140), // 2×60 закрытых + 2×10 уже прошедших в открытом
        }]
    );

    let summary = h.uc.finish_work_day.execute(work_day_id).await.unwrap();
    assert_eq!(summary.closed_intervals, 3, "finish_day force-closes the open interval too");
    assert!(!summary.norm_met);
    let task_totals: std::collections::HashMap<TaskId, Duration> = summary.task_totals.into_iter().collect();
    assert_eq!(
        task_totals.get(&task_id).copied(),
        Some(Duration::minutes(140)),
        "same worked minutes as the mid-day report — finish_day doesn't fabricate unfinished slots"
    );

    let final_report = h.uc.build_day_report.execute(work_day_id).await.unwrap();
    assert_eq!(final_report.task_totals[0].duration, Duration::minutes(140));
}

// --- Сценарий 9: экспорт CSV как документ --------------------------------

/// tasks.md, Задача 14, сценарий 9: экспорт CSV — сверка вручную,
/// отправляется как документ, а не текстом.
#[tokio::test]
async fn scenario_09_csv_export_is_sent_as_a_document_not_text() {
    let h = Harness::new().await;
    let (user_id, work_day_id) = h.register_and_start_day(2009).await;
    let task_id = h.create_task(user_id, "Export me").await;
    h.run_full_interval(work_day_id, task_id, "worked an hour").await;

    let csv_bytes = h.uc.export_day_report_csv.execute(work_day_id).await.unwrap();
    let csv_text = String::from_utf8(csv_bytes.clone()).expect("csv must be valid utf-8");
    assert!(csv_text.contains("Export me"));

    // То, что реально делает Telegram-адаптер (Задача 12) с байтами экспорта.
    h.notifier.sent.lock().unwrap().clear();
    let document = OutboundMessage::Document {
        filename: "report.csv".to_string(),
        bytes: csv_bytes.clone(),
    };
    h.notifier.send(user_id, document).await.unwrap();

    let sent = h.notifier.all();
    assert_eq!(sent.len(), 1);
    match &sent[0].1 {
        OutboundMessage::Document { filename, bytes } => {
            assert_eq!(filename, "report.csv");
            assert_eq!(bytes, &csv_bytes);
        }
        OutboundMessage::Text(_) => panic!("CSV export must be sent as a document, not text"),
    }
}

// --- Сценарий 10: периодическое "готовы продолжить работу?" -------------

/// tasks.md, Задача 14, сценарий 10: день начат, интервал не стартован
/// дольше `N` часов — приходит "Готовы продолжить работу?"; после старта
/// интервала повторные напоминания прекращаются; день не завершается
/// автоматически.
#[tokio::test]
async fn scenario_10_idle_prompt_fires_after_threshold_and_stops_once_active() {
    let h = Harness::new().await; // idle_threshold = 2 часа по умолчанию
    let (user_id, work_day_id) = h.register_and_start_day(2010).await;

    h.advance(119);
    h.poll_loop.tick().await;
    assert!(!h.notifier.texts().iter().any(|t| t.contains("Готовы продолжить работу?")));

    h.advance(1); // ровно 120 минут простоя
    h.poll_loop.tick().await;
    assert_eq!(
        h.notifier.texts().iter().filter(|t| t.contains("Готовы продолжить работу?")).count(),
        1
    );

    let task_id = h.create_task(user_id, "Finally starting").await;
    h.uc.start_hour_interval.execute(work_day_id, task_id).await.unwrap();

    h.advance(180); // на 3 часа дальше — с активным интервалом напоминание не должно приходить
    h.poll_loop.tick().await;
    assert_eq!(
        h.notifier.texts().iter().filter(|t| t.contains("Готовы продолжить работу?")).count(),
        1,
        "no further idle prompts once an interval is active"
    );

    assert!(
        h.work_days.find_by_id(work_day_id).await.unwrap().unwrap().finished_at().is_none(),
        "the day is never auto-finished by idle timeout"
    );
}

// --- Сценарий 11: отчёты за неделю с учётом таймзоны ---------------------

/// tasks.md, Задача 14, сценарий 11: `/report_week`/`/report_month` за
/// период с несколькими рабочими днями — агрегированные числа совпадают с
/// ручным расчётом, с учётом таймзоны пользователя на границах периода.
#[tokio::test]
async fn scenario_11_period_reports_respect_user_timezone_at_week_boundaries() {
    // 2026-08-23 (воскресенье) 23:30 UTC — уже 2026-08-24 (понедельник) 06:30
    // локально при UTC+7: новая неделя началась локально, хотя `Clock.now()`
    // ещё воскресенье по UTC.
    let now = utc_date(2026, 8, 23, 23);
    let now = UtcTimestamp::new(now.value() + Duration::minutes(30));
    let h = Harness::with(now, IdlePromptConfig::default(), ReminderScheduleConfig::default()).await;

    let user_id = TelegramId::new(2011).unwrap();
    let timezone = TimeZoneOffset::new(7 * 60).unwrap();
    h.users.create(User::new(user_id, timezone, now)).await.unwrap();

    let previous_week_task = h
        .tasks
        .create(new_ready_task(user_id, "Previous week", now))
        .await
        .unwrap();
    seed_closed_work_day(&h, user_id, previous_week_task.id(), utc_date(2026, 8, 17, 1), 3).await;

    let current_week_task = h
        .tasks
        .create(new_ready_task(user_id, "Current week", now))
        .await
        .unwrap();
    seed_closed_work_day(&h, user_id, current_week_task.id(), utc_date(2026, 8, 24, 1), 8).await;

    // Явная дата — предыдущая (по локальному календарю) неделя.
    let previous = h
        .uc
        .build_period_report
        .execute(user_id, Period::Week, Some(NaiveDate::from_ymd_opt(2026, 8, 17).unwrap()))
        .await
        .unwrap();
    let previous_period = previous.period.unwrap();
    assert_eq!(previous_period.days_total, 1);
    assert_eq!(previous_period.days_with_norm_met, 0, "only 3/8 intervals closed that day");
    assert_eq!(
        previous.task_totals.iter().map(|e| e.task_id).collect::<Vec<_>>(),
        vec![previous_week_task.id()]
    );

    // `at = None`: `Clock.now()` ещё воскресенье по UTC, но по локали
    // (UTC+7) уже понедельник новой недели — период считается по локальной
    // дате, а не по UTC-дате.
    let default_report = h.uc.build_period_report.execute(user_id, Period::Week, None).await.unwrap();
    let default_period = default_report.period.unwrap();
    assert_eq!(default_period.start_date, NaiveDate::from_ymd_opt(2026, 8, 24).unwrap());
    assert_eq!(default_period.end_date, NaiveDate::from_ymd_opt(2026, 8, 30).unwrap());
    assert_eq!(default_period.days_total, 1);
    assert_eq!(default_period.days_with_norm_met, 1, "8/8 intervals closed that day");
    assert_eq!(
        default_report.task_totals.iter().map(|e| e.task_id).collect::<Vec<_>>(),
        vec![current_week_task.id()]
    );
}

// --- Сценарий 12: приоритет и дедлайн задачи -----------------------------

/// tasks.md, Задача 14, сценарий 12: задача с приоритетом и дедлайном
/// корректно сохраняется/отображается в пуле задач.
#[tokio::test]
async fn scenario_12_task_priority_and_deadline_round_trip() {
    let h = Harness::new().await;
    let user_id = h.uc.register_user.execute(TelegramId::new(2012).unwrap()).await.unwrap().telegram_id();

    let deadline = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();
    let task = h
        .uc
        .create_task
        .execute(user_id, "Ship the release", Some(TaskPriority::High), Some(deadline))
        .await
        .unwrap();

    assert_eq!(task.priority(), Some(TaskPriority::High));
    assert_eq!(task.deadline(), Some(deadline));
    assert_eq!(task.status(), TaskStatus::Ready);

    let reloaded = h.tasks.find_by_id(task.id()).await.unwrap().unwrap();
    assert_eq!(reloaded.priority(), Some(TaskPriority::High));
    assert_eq!(reloaded.deadline(), Some(deadline));

    let pool = h.uc.list_available_tasks.execute(user_id, TaskFilter::Ready).await.unwrap();
    assert!(pool
        .iter()
        .any(|t| t.id() == task.id() && t.priority() == Some(TaskPriority::High) && t.deadline() == Some(deadline)));
}

// --- Сценарий 13: создание/редактирование и сортировка пула --------------

/// tasks.md, Задача 14, сценарий 13: создание/редактирование задачи через
/// бота и корректная сортировка/фильтрация пула по приоритету и дедлайну.
#[tokio::test]
async fn scenario_13_create_update_and_pool_sorting_and_filtering() {
    let h = Harness::new().await;
    let user_id = h.uc.register_user.execute(TelegramId::new(2013).unwrap()).await.unwrap().telegram_id();

    let low_no_deadline = h
        .uc
        .create_task
        .execute(user_id, "Low, no deadline", Some(TaskPriority::Low), None)
        .await
        .unwrap();
    let high_far = h
        .uc
        .create_task
        .execute(
            user_id,
            "High, far deadline",
            Some(TaskPriority::High),
            Some(NaiveDate::from_ymd_opt(2026, 12, 1).unwrap()),
        )
        .await
        .unwrap();
    let high_near = h
        .uc
        .create_task
        .execute(
            user_id,
            "High, near deadline",
            Some(TaskPriority::High),
            Some(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()),
        )
        .await
        .unwrap();
    let no_priority = h
        .uc
        .create_task
        .execute(user_id, "No priority", None, None)
        .await
        .unwrap();

    let all = h.uc.list_available_tasks.execute(user_id, TaskFilter::All).await.unwrap();
    let order: Vec<TaskId> = all.iter().map(|t| t.id()).collect();
    assert_eq!(
        order,
        vec![high_near.id(), high_far.id(), low_no_deadline.id(), no_priority.id()],
        "high (nearest deadline first) > high (later deadline) > low > no priority"
    );

    let updated = h
        .uc
        .update_task
        .execute(
            low_no_deadline.id(),
            Some("Renamed".to_string()),
            Some(Some(TaskPriority::Medium)),
            Some(Some(NaiveDate::from_ymd_opt(2026, 8, 25).unwrap())),
            Some(TaskStatus::NotReady),
        )
        .await
        .unwrap();
    assert_eq!(updated.title(), "Renamed");
    assert_eq!(updated.priority(), Some(TaskPriority::Medium));
    assert_eq!(updated.status(), TaskStatus::NotReady);

    let ready_only = h.uc.list_available_tasks.execute(user_id, TaskFilter::Ready).await.unwrap();
    assert!(!ready_only.iter().any(|t| t.id() == low_no_deadline.id()));

    let not_ready = h.uc.list_available_tasks.execute(user_id, TaskFilter::NotReady).await.unwrap();
    assert_eq!(not_ready.iter().map(|t| t.id()).collect::<Vec<_>>(), vec![low_no_deadline.id()]);

    let forbidden = h
        .uc
        .update_task
        .execute(low_no_deadline.id(), None, None, None, Some(TaskStatus::InProgress))
        .await
        .unwrap_err();
    assert_eq!(forbidden, application::UpdateTaskError::InProgressNotAllowed);
}

// --- Сценарий 14: /finish_day во всех трёх состояниях --------------------

/// tasks.md, Задача 14, сценарий 14: `/finish_day` во время активного
/// часового интервала — во всех трёх состояниях незакрытого интервала.
#[tokio::test]
async fn scenario_14_finish_day_forces_closed_in_all_three_open_interval_states() {
    // Состояние 1: открытая рабочая десятиминутка (таймер идёт).
    {
        let h = Harness::new().await;
        let (user_id, work_day_id) = h.register_and_start_day(3001).await;
        let task_id = h.create_task(user_id, "Open work slot").await;
        let interval = h.uc.start_hour_interval.execute(work_day_id, task_id).await.unwrap();
        h.advance(10);
        h.uc
            .submit_ten_min_answer
            .execute(interval.id(), TenMinCheckAnswer::Yes, None)
            .await
            .unwrap();
        h.advance(4); // второй слот идёт, таймер ещё не истёк

        let summary = h.uc.finish_work_day.execute(work_day_id).await.unwrap();
        assert_eq!(summary.closed_intervals, 1);

        let closed_interval = h.intervals.find_by_id(interval.id()).await.unwrap().unwrap();
        assert_eq!(closed_interval.successful_count().value(), 1);
        assert!(closed_interval.ended_at().is_some());

        let mut checks = h.checks.list_by_interval(interval.id()).await.unwrap();
        checks.sort_by_key(|c| c.started_at());
        let last = checks.last().unwrap();
        assert_eq!(last.status(), CheckStatus::NoResponse);
        assert_eq!(last.reason(), None);
        assert!(h.checks.find_open().await.unwrap().is_empty());
    }

    // Состояние 2: открытая десятиминутка отдыха.
    {
        let h = Harness::new().await;
        let (user_id, work_day_id) = h.register_and_start_day(3002).await;
        let task_id = h.create_task(user_id, "Open rest slot").await;
        let interval = h.uc.start_hour_interval.execute(work_day_id, task_id).await.unwrap();
        for _ in 0..5 {
            h.advance(10);
            h.uc
                .submit_ten_min_answer
                .execute(interval.id(), TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();
        }
        h.advance(3); // отдыхает, ещё не вернулся

        let summary = h.uc.finish_work_day.execute(work_day_id).await.unwrap();
        assert_eq!(summary.closed_intervals, 1);

        let closed_interval = h.intervals.find_by_id(interval.id()).await.unwrap().unwrap();
        assert_eq!(closed_interval.successful_count().value(), 5);

        let mut checks = h.checks.list_by_interval(interval.id()).await.unwrap();
        checks.sort_by_key(|c| c.started_at());
        let rest = checks.last().unwrap();
        assert_eq!(rest.reason(), None);
        assert_ne!(rest.status(), CheckStatus::NoResponse, "rest overrun is not a failure");
        assert!(h.checks.find_open().await.unwrap().is_empty());
    }

    // Состояние 3: последняя десятиминутка уже закрыта, интервал "ждёт возврата".
    {
        let h = Harness::new().await;
        let (user_id, work_day_id) = h.register_and_start_day(3003).await;
        let task_id = h.create_task(user_id, "Awaiting resume").await;
        let interval = h.uc.start_hour_interval.execute(work_day_id, task_id).await.unwrap();
        h.advance(10);
        h.poll_loop.tick().await; // "Ты работаешь?"
        h.advance(1);
        h.poll_loop.tick().await; // NoResponse закрыт поллером

        let before = h.checks.list_by_interval(interval.id()).await.unwrap();
        assert_eq!(before.len(), 1);
        assert!(before[0].ended_at().is_some());

        let summary = h.uc.finish_work_day.execute(work_day_id).await.unwrap();
        assert_eq!(summary.closed_intervals, 1);

        let after = h.checks.list_by_interval(interval.id()).await.unwrap();
        assert_eq!(after.len(), 1, "no extra check is created for an already-closed awaiting-resume slot");
        assert_eq!(after[0].ended_at(), before[0].ended_at(), "already-closed row is left untouched");

        let closed_interval = h.intervals.find_by_id(interval.id()).await.unwrap().unwrap();
        assert_eq!(closed_interval.successful_count().value(), 0);
    }
}

// --- Сценарий 15: рестарт процесса (состояния, не покрытые restart_safety.rs) ---

/// tasks.md, Задача 14, сценарий 15: рестарт процесса с открытым днём.
/// `tests/restart_safety.rs` (Задача 13) уже покрывает состояние "активный
/// интервал с открытой рабочей десятиминуткой"; этот тест дополняет его
/// двумя другими состояниями из сценария 15 — "день без активного
/// интервала" и "последняя десятиминутка закрыта как провал, интервал ждёт
/// возврата" — оба восстанавливаются без единой строчки кода восстановления,
/// просто потому что `PollLoop` каждый тик заново читает состояние из БД.
#[tokio::test]
async fn scenario_15_restart_safety_idle_day_and_awaiting_resume_states() {
    // Состояние: день начат, активного интервала нет — после "рестарта"
    // периодическое "Готовы продолжить работу?" продолжает работать.
    {
        let db = TempDbFile::new();
        let pool = adapters_persistence_sqlite::connect(&db.url()).await.unwrap();
        let now = UtcTimestamp::new(Utc::now());
        let telegram_id = TelegramId::new(4001).unwrap();

        let users = SqliteUserRepository::new(pool.clone());
        users.create(User::new(telegram_id, TimeZoneOffset::utc(), now)).await.unwrap();
        let work_days = SqliteWorkDayRepository::new(pool.clone());
        let idle_started_at = UtcTimestamp::new(now.value() - Duration::hours(3));
        work_days
            .create(WorkDay::new(WorkDayId::new(0), telegram_id, idle_started_at, None, None, None, None))
            .await
            .unwrap();

        let clock: Arc<dyn Clock> = Arc::new(TestClock::new(now));
        let notifier = Arc::new(RecordingNotifier::default());
        let wired = wire(Adapters {
            pool: pool.clone(),
            clock,
            notifier: notifier.clone(),
            idle_prompt: IdlePromptConfig::default(),
            reminder_schedule: ReminderScheduleConfig::default(),
            poll_interval: StdDuration::from_secs(60),
        });

        wired.poll_loop.tick().await;

        assert_eq!(notifier.texts(), vec!["Готовы продолжить работу?".to_string()]);
        pool.close().await;
    }

    // Состояние: последняя десятиминутка интервала уже закрыта как провал
    // (`find_awaiting_resume`) — после "рестарта" напоминания "готов
    // продолжить?" продолжаются как ни в чём не бывало.
    {
        let db = TempDbFile::new();
        let pool = adapters_persistence_sqlite::connect(&db.url()).await.unwrap();
        let now = UtcTimestamp::new(Utc::now());
        let telegram_id = TelegramId::new(4002).unwrap();

        let users = SqliteUserRepository::new(pool.clone());
        users.create(User::new(telegram_id, TimeZoneOffset::utc(), now)).await.unwrap();
        let tasks = SqliteTaskRepository::new(pool.clone());
        let task = tasks
            .create(Task::new(TaskId::new(0), telegram_id, "resume me", TaskStatus::InProgress, None, None, now).unwrap())
            .await
            .unwrap();
        let work_days = SqliteWorkDayRepository::new(pool.clone());
        let work_day = work_days
            .create(WorkDay::new(WorkDayId::new(0), telegram_id, now, None, None, None, None))
            .await
            .unwrap();
        let intervals = SqliteHourIntervalRepository::new(pool.clone());
        let interval = intervals
            .create(HourInterval::new(
                HourIntervalId::new(0),
                work_day.id(),
                now,
                None,
                SuccessfulCount::zero(),
                None,
            ))
            .await
            .unwrap();
        let checks = SqliteTenMinCheckRepository::new(pool.clone());
        let closed_at = UtcTimestamp::new(now.value() - Duration::minutes(6));
        checks
            .create(TenMinCheck::new(
                TenMinCheckId::new(0),
                interval.id(),
                task.id(),
                UtcTimestamp::new(closed_at.value() - Duration::minutes(10)),
                Some(closed_at),
                CheckStatus::Failed,
                Some("distracted".to_string()),
                None,
            ))
            .await
            .unwrap();

        let clock: Arc<dyn Clock> = Arc::new(TestClock::new(now));
        let notifier = Arc::new(RecordingNotifier::default());
        let wired = wire(Adapters {
            pool: pool.clone(),
            clock,
            notifier: notifier.clone(),
            idle_prompt: IdlePromptConfig::default(),
            reminder_schedule: ReminderScheduleConfig::default(),
            poll_interval: StdDuration::from_secs(60),
        });

        wired.poll_loop.tick().await;

        assert_eq!(notifier.texts(), vec!["Готов продолжить?".to_string()]);
        pool.close().await;
    }
}

// --- Сценарий 16: план vs факт по времени (перерасход) -------------------

/// tasks.md, Задача 14, сценарий 16: день с провалом и одним переработанным
/// отдыхом → сумма перерасхода по всем интервалам сходится с ручным
/// расчётом.
#[tokio::test]
async fn scenario_16_plan_vs_actual_overrun_report_matches_manual_calculation() {
    let h = Harness::new().await;
    let (user_id, work_day_id) = h.register_and_start_day(2016).await;
    let task_id = h.create_task(user_id, "Overrun test").await;

    // Интервал 1 — без провалов, ровно номинальный час (перерасход = 0).
    h.run_full_interval(work_day_id, task_id, "on time").await;

    // Интервал 2 — 1 провал (+5 мин на подтверждение) + переработанный отдых
    // на 15 минут сверх номинала: 10 + 10 + 5 + 4×10 + 25 = 90 минут вместо
    // 60 — перерасход ровно 30 минут.
    let interval2 = h.uc.start_hour_interval.execute(work_day_id, task_id).await.unwrap();
    h.advance(10);
    h.uc
        .submit_ten_min_answer
        .execute(interval2.id(), TenMinCheckAnswer::Yes, None)
        .await
        .unwrap();
    h.advance(10);
    h.uc
        .submit_ten_min_answer
        .execute(interval2.id(), TenMinCheckAnswer::No, Some("phone call".to_string()))
        .await
        .unwrap();
    h.advance(5);
    h.uc.confirm_ready_to_continue.execute(interval2.id()).await.unwrap();
    for _ in 0..4 {
        h.advance(10);
        h.uc
            .submit_ten_min_answer
            .execute(interval2.id(), TenMinCheckAnswer::Yes, None)
            .await
            .unwrap();
    }
    h.advance(25);
    h.uc.mark_returned_from_rest.execute(interval2.id()).await.unwrap();
    let finished2 = h
        .uc
        .finish_hour_interval
        .execute(interval2.id(), "delayed but done".to_string())
        .await
        .unwrap();

    let all_intervals = h.intervals.list_by_work_day(work_day_id).await.unwrap();
    assert_eq!(all_intervals.len(), 2);

    let interval1_overrun = all_intervals
        .iter()
        .find(|i| i.id() != interval2.id())
        .map(|i| interval_overrun(i.ended_at().unwrap().value() - i.started_at().value()))
        .unwrap();
    assert_eq!(interval1_overrun, Duration::zero());

    let interval2_overrun = interval_overrun(finished2.ended_at().unwrap().value() - finished2.started_at().value());
    assert_eq!(interval2_overrun, Duration::minutes(30));

    let total_overrun = day_overrun(&[interval1_overrun, interval2_overrun]);
    assert_eq!(total_overrun, Duration::minutes(30));
}
