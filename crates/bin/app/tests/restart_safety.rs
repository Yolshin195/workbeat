//! Интеграционный тест restart-safety (Задача 13 корневого `tasks.md`):
//! создаёт открытый `work_day` с активным `HourInterval` и незавершённой
//! (открытой) `TenMinCheck` **напрямую через репозитории**, на tempfile
//! SQLite — эмулируя состояние "как будто процесс уже работал и упал".
//! Затем поднимает composition root (`app::wiring::wire`) с нуля, как при
//! реальном рестарте, и проверяет, что ближайший тик `PollLoop` (Задача 10)
//! обрабатывает эту десятиминутку корректно — без какого-либо специального
//! кода восстановления: `PollLoop` просто читает текущее состояние из БД
//! (см. `README.md`, принцип таймеров).

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use application::{
    Clock, IdlePromptConfig, Notifier, NotifierError, OutboundMessage, ReminderScheduleConfig,
};
use async_trait::async_trait;
use domain::{
    CheckStatus, HourInterval, HourIntervalId, SuccessfulCount, Task, TaskId, TaskStatus,
    TelegramId, TenMinCheck, TenMinCheckId, TimeZoneOffset, User, UtcTimestamp, WorkDay, WorkDayId,
};

use adapters_persistence_sqlite::{
    SqliteHourIntervalRepository, SqliteTaskRepository, SqliteTenMinCheckRepository,
    SqliteUserRepository, SqliteWorkDayRepository,
};
use application::{
    HourIntervalRepository, TaskRepository, TenMinCheckRepository, UserRepository,
    WorkDayRepository,
};

use app::wiring::{wire, Adapters};

/// Временный файл SQLite для теста — не переиспользуем приватный
/// `test_support` крейта `adapters-persistence-sqlite` (он `#[cfg(test)]`
/// внутри своего крейта и недоступен как обычная зависимость отсюда).
struct TempDbFile {
    path: PathBuf,
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

impl TempDbFile {
    fn new() -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let path =
            std::env::temp_dir().join(format!("workbeat-restart-safety-{pid}-{unique}.sqlite"));
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

/// Фиксированное "сейчас" для теста — не привязываемся к системным часам
/// напрямую, чтобы тест был детерминирован независимо от того, когда он
/// запускается.
struct FixedClock(UtcTimestamp);

impl Clock for FixedClock {
    fn now(&self) -> UtcTimestamp {
        self.0
    }
}

#[derive(Default)]
struct RecordingNotifier {
    sent: Mutex<Vec<(TelegramId, OutboundMessage)>>,
}

#[async_trait]
impl Notifier for RecordingNotifier {
    async fn send(&self, user: TelegramId, message: OutboundMessage) -> Result<(), NotifierError> {
        self.sent.lock().unwrap().push((user, message));
        Ok(())
    }
}

#[tokio::test]
async fn poll_loop_advances_a_check_left_open_by_a_crashed_process() {
    let db = TempDbFile::new();
    let pool = adapters_persistence_sqlite::connect(&db.url())
        .await
        .expect("connect to tempfile sqlite db");

    let telegram_id = TelegramId::new(4242).unwrap();
    let now = UtcTimestamp::new(chrono::Utc::now());
    let started_20_min_ago = UtcTimestamp::new(now.value() - chrono::Duration::minutes(20));

    // --- "процесс уже работал и упал": состояние вставлено напрямую через
    // репозитории, без использования use cases (StartHourInterval и т.п.).
    let user_repository = SqliteUserRepository::new(pool.clone());
    user_repository
        .create(User::new(telegram_id, TimeZoneOffset::utc(), now))
        .await
        .expect("create user");

    let task_repository = SqliteTaskRepository::new(pool.clone());
    let task = task_repository
        .create(
            Task::new(
                TaskId::new(0),
                telegram_id,
                "write the report",
                TaskStatus::InProgress,
                None,
                None,
                now,
            )
            .unwrap(),
        )
        .await
        .expect("create task");

    let work_day_repository = SqliteWorkDayRepository::new(pool.clone());
    let work_day = work_day_repository
        .create(WorkDay::new(
            WorkDayId::new(0),
            telegram_id,
            started_20_min_ago,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("create work day");

    let hour_interval_repository = SqliteHourIntervalRepository::new(pool.clone());
    let hour_interval = hour_interval_repository
        .create(HourInterval::new(
            HourIntervalId::new(0),
            work_day.id(),
            started_20_min_ago,
            None,
            SuccessfulCount::zero(),
            None,
        ))
        .await
        .expect("create hour interval");

    let ten_min_check_repository = SqliteTenMinCheckRepository::new(pool.clone());
    ten_min_check_repository
        .create(TenMinCheck::new(
            TenMinCheckId::new(0),
            hour_interval.id(),
            task.id(),
            started_20_min_ago,
            // Открыта (`ended_at IS NULL`) — таймер "должен был" истечь 10
            // минут назад, но процесс упал раньше, чем поллер это заметил.
            None,
            CheckStatus::Worked, // плейсхолдер-статус для незакрытой строки, игнорируется, пока `ended_at IS NULL`.
            None,
            None,
        ))
        .await
        .expect("create open ten-min check");

    // --- "рестарт процесса": composition root собирается с нуля поверх той
    // же БД, без какого-либо кода восстановления.
    let clock: Arc<dyn Clock> = Arc::new(FixedClock(now));
    let notifier = Arc::new(RecordingNotifier::default());

    let wired = wire(Adapters {
        pool: pool.clone(),
        clock,
        notifier: notifier.clone(),
        idle_prompt: IdlePromptConfig::default(),
        reminder_schedule: ReminderScheduleConfig::default(),
        poll_interval: Duration::from_secs(60),
    });

    // Один тик — не бесконечный `run()` — эмулирует "первый тик поллера
    // после рестарта".
    wired.poll_loop.tick().await;

    let sent_message = {
        let sent = notifier.sent.lock().unwrap();
        assert_eq!(
            sent.len(),
            1,
            "expected exactly one proactive message on the first tick after restart"
        );
        sent[0].clone()
    };
    assert_eq!(sent_message.0, telegram_id);
    match sent_message.1 {
        OutboundMessage::Text(text) => assert!(!text.is_empty()),
        OutboundMessage::Document { .. } => panic!("expected a text prompt, not a document"),
    }

    let refreshed = ten_min_check_repository
        .find_open()
        .await
        .expect("query open checks");
    assert_eq!(
        refreshed.len(),
        1,
        "the check must still be open (10-minute timer not resolved yet)"
    );
    assert!(
        refreshed[0].last_reminder_at().is_some(),
        "AdvanceOpenTenMinChecks must record that the reminder was sent"
    );

    pool.close().await;
}

#[tokio::test]
async fn poll_loop_after_restart_behaves_like_the_original_process() {
    // Тот же сценарий, но акцент на самой идее restart-safety: второй,
    // независимо собранный `PollLoop` поверх той же БД видит то же
    // состояние и реагирует так же — никакого состояния в памяти процесса,
    // потерянного при "падении", не было нужно.
    let db = TempDbFile::new();
    let pool = adapters_persistence_sqlite::connect(&db.url())
        .await
        .expect("connect to tempfile sqlite db");

    let telegram_id = TelegramId::new(7).unwrap();
    let now = UtcTimestamp::new(chrono::Utc::now());
    let started_20_min_ago = UtcTimestamp::new(now.value() - chrono::Duration::minutes(20));

    let user_repository = SqliteUserRepository::new(pool.clone());
    user_repository
        .create(User::new(telegram_id, TimeZoneOffset::utc(), now))
        .await
        .expect("create user");

    let task_repository = SqliteTaskRepository::new(pool.clone());
    let task = task_repository
        .create(
            Task::new(
                TaskId::new(0),
                telegram_id,
                "write the report",
                TaskStatus::InProgress,
                None,
                None,
                now,
            )
            .unwrap(),
        )
        .await
        .expect("create task");

    let work_day_repository = SqliteWorkDayRepository::new(pool.clone());
    let work_day = work_day_repository
        .create(WorkDay::new(
            WorkDayId::new(0),
            telegram_id,
            started_20_min_ago,
            None,
            None,
            None,
            None,
        ))
        .await
        .expect("create work day");

    let hour_interval_repository = SqliteHourIntervalRepository::new(pool.clone());
    let hour_interval = hour_interval_repository
        .create(HourInterval::new(
            HourIntervalId::new(0),
            work_day.id(),
            started_20_min_ago,
            None,
            SuccessfulCount::zero(),
            None,
        ))
        .await
        .expect("create hour interval");

    let ten_min_check_repository = SqliteTenMinCheckRepository::new(pool.clone());
    ten_min_check_repository
        .create(TenMinCheck::new(
            TenMinCheckId::new(0),
            hour_interval.id(),
            task.id(),
            started_20_min_ago,
            None,
            CheckStatus::Worked,
            None,
            None,
        ))
        .await
        .expect("create open ten-min check");

    // "Процесс A": первый тик застаёт десятиминутку открытой уже 20 минут,
    // ещё не спрошенной — `WorkSlotPollDecision` шлёт "Ты работаешь?" и
    // проставляет `last_reminder_at`.
    {
        let clock: Arc<dyn Clock> = Arc::new(FixedClock(now));
        let notifier = Arc::new(RecordingNotifier::default());
        let wired = wire(Adapters {
            pool: pool.clone(),
            clock,
            notifier: notifier.clone(),
            idle_prompt: IdlePromptConfig::default(),
            reminder_schedule: ReminderScheduleConfig::default(),
            poll_interval: Duration::from_secs(60),
        });

        wired.poll_loop.tick().await;

        assert_eq!(notifier.sent.lock().unwrap().len(), 1);
    }
    let after_first_tick = ten_min_check_repository
        .find_open()
        .await
        .expect("query open checks");
    assert_eq!(after_first_tick.len(), 1);
    assert!(after_first_tick[0].last_reminder_at().is_some());

    // "Процесс A" падает здесь — без выполнения второго тика.

    // "Процесс B" (рестарт): независимо собранный `PollLoop` поверх той же
    // БД, без памяти о том, что уже спрашивал "процесс A". Читает
    // `last_reminder_at`, проставленный процессом A, из БД — и, так как
    // ответа так и не поступило, на этом тике корректно закрывает
    // десятиминутку как `NoResponse` (тот же исход, который получил бы один
    // непрерывно работающий процесс на своём следующем тике). Никакого
    // отдельного шага восстановления для этого не потребовалось.
    {
        let clock: Arc<dyn Clock> = Arc::new(FixedClock(now));
        let notifier = Arc::new(RecordingNotifier::default());
        let wired = wire(Adapters {
            pool: pool.clone(),
            clock,
            notifier: notifier.clone(),
            idle_prompt: IdlePromptConfig::default(),
            reminder_schedule: ReminderScheduleConfig::default(),
            poll_interval: Duration::from_secs(60),
        });

        wired.poll_loop.tick().await;

        // Молчание закрывает слот без нового проактивного сообщения (см.
        // README.md раздел 2, п.2) — уведомление после этого шлёт отдельный
        // `RemindAwaitingResume` на следующих тиках, не эта десятиминутка.
        assert_eq!(notifier.sent.lock().unwrap().len(), 0);
    }

    let refreshed = ten_min_check_repository
        .find_open()
        .await
        .expect("query open checks");
    assert!(
        refreshed.is_empty(),
        "the check must be resolved (closed) after the restart's first tick"
    );

    let closed = ten_min_check_repository
        .list_by_interval(hour_interval.id())
        .await
        .expect("list checks by interval");
    assert_eq!(closed.len(), 1);
    assert_eq!(closed[0].status(), CheckStatus::NoResponse);
    assert!(closed[0].ended_at().is_some());
    assert!(
        closed[0].reason().is_none(),
        "NoResponse closes without a reason; it may be added later via SubmitFailureReason"
    );

    pool.close().await;
}
