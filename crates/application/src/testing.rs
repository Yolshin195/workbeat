//! Фейковые реализации портов из `ports` для юнит- и сценарных тестов
//! use cases (используются этой задачей и последующими — Задачи 5-9). Ручные
//! фейки вместо `mockall`-моков: они хранят состояние в памяти, что удобнее
//! для сценарных тестов вида "выполнили одну команду — проверили, что видно
//! следующей", а не только "метод был вызван с такими-то аргументами".

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Mutex;

use async_trait::async_trait;
use domain::{
    CheckStatus, HourInterval, HourIntervalId, Task, TaskId, TelegramId, TenMinCheck,
    TenMinCheckId, User, UtcTimestamp, WorkDay, WorkDayId,
};

use crate::error::{NotifierError, RepoError};
use crate::outbound_message::OutboundMessage;
use crate::ports::{
    Clock, HourIntervalRepository, Notifier, TaskRepository, TenMinCheckRepository,
    UserRepository, WorkDayRepository,
};

/// `Clock`-фейк с временем, которое тест выставляет вручную — никакого
/// реального ожидания в тестах не требуется.
pub struct FakeClock {
    now: Mutex<UtcTimestamp>,
}

impl FakeClock {
    pub fn new(now: UtcTimestamp) -> Self {
        Self {
            now: Mutex::new(now),
        }
    }

    pub fn set(&self, now: UtcTimestamp) {
        *self.now.lock().unwrap() = now;
    }

    pub fn advance(&self, duration: chrono::Duration) {
        let mut now = self.now.lock().unwrap();
        *now = UtcTimestamp::new(now.value() + duration);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> UtcTimestamp {
        *self.now.lock().unwrap()
    }
}

/// `Notifier`-фейк, который ничего никуда не отправляет, а просто копит
/// отправленные сообщения для проверки в тестах.
#[derive(Default)]
pub struct FakeNotifier {
    sent: Mutex<Vec<(TelegramId, OutboundMessage)>>,
}

impl FakeNotifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sent_messages(&self) -> Vec<(TelegramId, OutboundMessage)> {
        self.sent.lock().unwrap().clone()
    }
}

#[async_trait]
impl Notifier for FakeNotifier {
    async fn send(
        &self,
        user: TelegramId,
        message: OutboundMessage,
    ) -> Result<(), NotifierError> {
        self.sent.lock().unwrap().push((user, message));
        Ok(())
    }
}

/// `UserRepository`-фейк в памяти. `users` не имеет суррогатного id, поэтому
/// ключ — `telegram_id`.
#[derive(Default)]
pub struct InMemoryUserRepository {
    users: Mutex<Vec<User>>,
}

impl InMemoryUserRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn snapshot(&self) -> Vec<User> {
        self.users.lock().unwrap().clone()
    }
}

#[async_trait]
impl UserRepository for InMemoryUserRepository {
    async fn create(&self, user: User) -> Result<(), RepoError> {
        self.users.lock().unwrap().push(user);
        Ok(())
    }

    async fn find_by_telegram_id(
        &self,
        telegram_id: TelegramId,
    ) -> Result<Option<User>, RepoError> {
        Ok(self
            .users
            .lock()
            .unwrap()
            .iter()
            .find(|user| user.telegram_id() == telegram_id)
            .cloned())
    }
}

/// `TaskRepository`-фейк в памяти. `id` во входном `Task` для `create`
/// игнорируется — реальный id присваивается счётчиком, как это сделал бы
/// SQLite `INTEGER PRIMARY KEY`.
#[derive(Default)]
pub struct InMemoryTaskRepository {
    tasks: Mutex<Vec<Task>>,
    next_id: AtomicI64,
}

impl InMemoryTaskRepository {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(Vec::new()),
            next_id: AtomicI64::new(1),
        }
    }

    pub fn snapshot(&self) -> Vec<Task> {
        self.tasks.lock().unwrap().clone()
    }
}

#[async_trait]
impl TaskRepository for InMemoryTaskRepository {
    async fn create(&self, task: Task) -> Result<Task, RepoError> {
        let id = TaskId::new(self.next_id.fetch_add(1, Ordering::SeqCst));
        let persisted = Task::new(
            id,
            task.user_id(),
            task.title().to_string(),
            task.status(),
            task.priority(),
            task.deadline(),
            task.created_at(),
        )
        .expect("task fields were already validated by the caller");
        self.tasks.lock().unwrap().push(persisted.clone());
        Ok(persisted)
    }

    async fn update(&self, task: Task) -> Result<(), RepoError> {
        let mut tasks = self.tasks.lock().unwrap();
        let slot = tasks
            .iter_mut()
            .find(|existing| existing.id() == task.id())
            .ok_or(RepoError::NotFound)?;
        *slot = task;
        Ok(())
    }

    async fn find_by_id(&self, id: TaskId) -> Result<Option<Task>, RepoError> {
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .iter()
            .find(|task| task.id() == id)
            .cloned())
    }

    async fn list_by_user(&self, user_id: TelegramId) -> Result<Vec<Task>, RepoError> {
        Ok(self
            .tasks
            .lock()
            .unwrap()
            .iter()
            .filter(|task| task.user_id() == user_id)
            .cloned()
            .collect())
    }
}

/// `WorkDayRepository`-фейк в памяти. `find_open_without_active_interval`
/// в реальном SQLite-адаптере (Задача 4) будет JOIN'ом с `hour_intervals`;
/// здесь эквивалентное состояние ("у дня сейчас есть активный интервал")
/// тест выставляет явно через `set_has_active_interval`.
#[derive(Default)]
pub struct InMemoryWorkDayRepository {
    work_days: Mutex<Vec<WorkDay>>,
    active_intervals: Mutex<HashSet<WorkDayId>>,
    next_id: AtomicI64,
}

impl InMemoryWorkDayRepository {
    pub fn new() -> Self {
        Self {
            work_days: Mutex::new(Vec::new()),
            active_intervals: Mutex::new(HashSet::new()),
            next_id: AtomicI64::new(1),
        }
    }

    pub fn snapshot(&self) -> Vec<WorkDay> {
        self.work_days.lock().unwrap().clone()
    }

    pub fn set_has_active_interval(&self, work_day_id: WorkDayId, has_active_interval: bool) {
        let mut active = self.active_intervals.lock().unwrap();
        if has_active_interval {
            active.insert(work_day_id);
        } else {
            active.remove(&work_day_id);
        }
    }
}

#[async_trait]
impl WorkDayRepository for InMemoryWorkDayRepository {
    async fn create(&self, work_day: WorkDay) -> Result<WorkDay, RepoError> {
        let id = WorkDayId::new(self.next_id.fetch_add(1, Ordering::SeqCst));
        let persisted = WorkDay::new(
            id,
            work_day.user_id(),
            work_day.started_at(),
            work_day.finished_at(),
            work_day.lunch_started_at(),
            work_day.lunch_ended_at(),
            work_day.last_idle_prompt_at(),
        );
        self.work_days.lock().unwrap().push(persisted.clone());
        Ok(persisted)
    }

    async fn update(&self, work_day: WorkDay) -> Result<(), RepoError> {
        let mut work_days = self.work_days.lock().unwrap();
        let slot = work_days
            .iter_mut()
            .find(|existing| existing.id() == work_day.id())
            .ok_or(RepoError::NotFound)?;
        *slot = work_day;
        Ok(())
    }

    async fn find_by_id(&self, id: WorkDayId) -> Result<Option<WorkDay>, RepoError> {
        Ok(self
            .work_days
            .lock()
            .unwrap()
            .iter()
            .find(|work_day| work_day.id() == id)
            .cloned())
    }

    async fn find_open_by_user(&self, user_id: TelegramId) -> Result<Option<WorkDay>, RepoError> {
        Ok(self
            .work_days
            .lock()
            .unwrap()
            .iter()
            .find(|work_day| work_day.user_id() == user_id && work_day.finished_at().is_none())
            .cloned())
    }

    async fn list_by_user(&self, user_id: TelegramId) -> Result<Vec<WorkDay>, RepoError> {
        Ok(self
            .work_days
            .lock()
            .unwrap()
            .iter()
            .filter(|work_day| work_day.user_id() == user_id)
            .cloned()
            .collect())
    }

    async fn find_open_without_active_interval(&self) -> Result<Vec<WorkDay>, RepoError> {
        let active = self.active_intervals.lock().unwrap();
        Ok(self
            .work_days
            .lock()
            .unwrap()
            .iter()
            .filter(|work_day| work_day.finished_at().is_none() && !active.contains(&work_day.id()))
            .cloned()
            .collect())
    }
}

/// `HourIntervalRepository`-фейк в памяти.
#[derive(Default)]
pub struct InMemoryHourIntervalRepository {
    intervals: Mutex<Vec<HourInterval>>,
    next_id: AtomicI64,
}

impl InMemoryHourIntervalRepository {
    pub fn new() -> Self {
        Self {
            intervals: Mutex::new(Vec::new()),
            next_id: AtomicI64::new(1),
        }
    }

    pub fn snapshot(&self) -> Vec<HourInterval> {
        self.intervals.lock().unwrap().clone()
    }
}

#[async_trait]
impl HourIntervalRepository for InMemoryHourIntervalRepository {
    async fn create(&self, interval: HourInterval) -> Result<HourInterval, RepoError> {
        let id = HourIntervalId::new(self.next_id.fetch_add(1, Ordering::SeqCst));
        let persisted = HourInterval::new(
            id,
            interval.work_day_id(),
            interval.started_at(),
            interval.ended_at(),
            interval.successful_count(),
            interval.summary().map(str::to_string),
        );
        self.intervals.lock().unwrap().push(persisted.clone());
        Ok(persisted)
    }

    async fn update(&self, interval: HourInterval) -> Result<(), RepoError> {
        let mut intervals = self.intervals.lock().unwrap();
        let slot = intervals
            .iter_mut()
            .find(|existing| existing.id() == interval.id())
            .ok_or(RepoError::NotFound)?;
        *slot = interval;
        Ok(())
    }

    async fn find_by_id(&self, id: HourIntervalId) -> Result<Option<HourInterval>, RepoError> {
        Ok(self
            .intervals
            .lock()
            .unwrap()
            .iter()
            .find(|interval| interval.id() == id)
            .cloned())
    }

    async fn find_open_by_work_day(
        &self,
        work_day_id: WorkDayId,
    ) -> Result<Option<HourInterval>, RepoError> {
        Ok(self
            .intervals
            .lock()
            .unwrap()
            .iter()
            .find(|interval| interval.work_day_id() == work_day_id && interval.ended_at().is_none())
            .cloned())
    }

    async fn list_by_work_day(
        &self,
        work_day_id: WorkDayId,
    ) -> Result<Vec<HourInterval>, RepoError> {
        Ok(self
            .intervals
            .lock()
            .unwrap()
            .iter()
            .filter(|interval| interval.work_day_id() == work_day_id)
            .cloned()
            .collect())
    }
}

/// `TenMinCheckRepository`-фейк в памяти. `find_awaiting_resume` в реальном
/// SQLite-адаптере будет JOIN'ом с `hour_intervals.ended_at`; здесь тест
/// явно помечает интервал закрытым через `set_interval_closed`.
#[derive(Default)]
pub struct InMemoryTenMinCheckRepository {
    checks: Mutex<Vec<TenMinCheck>>,
    closed_intervals: Mutex<HashSet<HourIntervalId>>,
    next_id: AtomicI64,
}

impl InMemoryTenMinCheckRepository {
    pub fn new() -> Self {
        Self {
            checks: Mutex::new(Vec::new()),
            closed_intervals: Mutex::new(HashSet::new()),
            next_id: AtomicI64::new(1),
        }
    }

    pub fn snapshot(&self) -> Vec<TenMinCheck> {
        self.checks.lock().unwrap().clone()
    }

    pub fn set_interval_closed(&self, hour_interval_id: HourIntervalId, closed: bool) {
        let mut closed_intervals = self.closed_intervals.lock().unwrap();
        if closed {
            closed_intervals.insert(hour_interval_id);
        } else {
            closed_intervals.remove(&hour_interval_id);
        }
    }
}

#[async_trait]
impl TenMinCheckRepository for InMemoryTenMinCheckRepository {
    async fn create(&self, check: TenMinCheck) -> Result<TenMinCheck, RepoError> {
        let id = TenMinCheckId::new(self.next_id.fetch_add(1, Ordering::SeqCst));
        let persisted = TenMinCheck::new(
            id,
            check.hour_interval_id(),
            check.task_id(),
            check.started_at(),
            check.ended_at(),
            check.status(),
            check.reason().map(str::to_string),
            check.last_reminder_at(),
        );
        self.checks.lock().unwrap().push(persisted.clone());
        Ok(persisted)
    }

    async fn update(&self, check: TenMinCheck) -> Result<(), RepoError> {
        let mut checks = self.checks.lock().unwrap();
        let slot = checks
            .iter_mut()
            .find(|existing| existing.id() == check.id())
            .ok_or(RepoError::NotFound)?;
        *slot = check;
        Ok(())
    }

    async fn find_by_id(&self, id: TenMinCheckId) -> Result<Option<TenMinCheck>, RepoError> {
        Ok(self
            .checks
            .lock()
            .unwrap()
            .iter()
            .find(|check| check.id() == id)
            .cloned())
    }

    async fn find_open_by_interval(
        &self,
        hour_interval_id: HourIntervalId,
    ) -> Result<Option<TenMinCheck>, RepoError> {
        Ok(self
            .checks
            .lock()
            .unwrap()
            .iter()
            .find(|check| {
                check.hour_interval_id() == hour_interval_id && check.ended_at().is_none()
            })
            .cloned())
    }

    async fn find_last_by_interval(
        &self,
        hour_interval_id: HourIntervalId,
    ) -> Result<Option<TenMinCheck>, RepoError> {
        Ok(self
            .checks
            .lock()
            .unwrap()
            .iter()
            .filter(|check| check.hour_interval_id() == hour_interval_id)
            .max_by_key(|check| check.started_at())
            .cloned())
    }

    async fn list_by_interval(
        &self,
        hour_interval_id: HourIntervalId,
    ) -> Result<Vec<TenMinCheck>, RepoError> {
        Ok(self
            .checks
            .lock()
            .unwrap()
            .iter()
            .filter(|check| check.hour_interval_id() == hour_interval_id)
            .cloned()
            .collect())
    }

    async fn find_open(&self) -> Result<Vec<TenMinCheck>, RepoError> {
        Ok(self
            .checks
            .lock()
            .unwrap()
            .iter()
            .filter(|check| check.ended_at().is_none())
            .cloned()
            .collect())
    }

    async fn find_awaiting_resume(&self) -> Result<Vec<TenMinCheck>, RepoError> {
        let closed_intervals = self.closed_intervals.lock().unwrap();
        let checks = self.checks.lock().unwrap();

        let mut latest_by_interval: HashMap<HourIntervalId, &TenMinCheck> = HashMap::new();
        for check in checks.iter() {
            latest_by_interval
                .entry(check.hour_interval_id())
                .and_modify(|current| {
                    if check.started_at() > current.started_at() {
                        *current = check;
                    }
                })
                .or_insert(check);
        }

        Ok(latest_by_interval
            .into_iter()
            .filter(|(interval_id, _)| !closed_intervals.contains(interval_id))
            .map(|(_, check)| check)
            .filter(|check| {
                check.ended_at().is_some()
                    && matches!(check.status(), CheckStatus::Failed | CheckStatus::NoResponse)
            })
            .cloned()
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use domain::{TaskStatus, TimeZoneOffset};

    fn now() -> UtcTimestamp {
        UtcTimestamp::new(Utc::now())
    }

    #[test]
    fn fake_clock_reports_set_time_and_advances() {
        let start = now();
        let clock = FakeClock::new(start);
        assert_eq!(clock.now(), start);

        clock.advance(chrono::Duration::minutes(10));
        assert_eq!(clock.now().value(), start.value() + chrono::Duration::minutes(10));
    }

    #[test]
    fn fake_notifier_records_sent_messages() {
        let notifier = FakeNotifier::new();
        let user = TelegramId::new(1).unwrap();

        pollster::block_on(notifier.send(user, OutboundMessage::Text("hi".to_string()))).unwrap();

        assert_eq!(
            notifier.sent_messages(),
            vec![(user, OutboundMessage::Text("hi".to_string()))]
        );
    }

    #[test]
    fn in_memory_user_repository_roundtrips() {
        let repo = InMemoryUserRepository::new();
        let telegram_id = TelegramId::new(1).unwrap();
        let user = User::new(telegram_id, TimeZoneOffset::utc(), now());

        pollster::block_on(async {
            assert_eq!(repo.find_by_telegram_id(telegram_id).await.unwrap(), None);
            repo.create(user.clone()).await.unwrap();
            assert_eq!(
                repo.find_by_telegram_id(telegram_id).await.unwrap(),
                Some(user)
            );
        });
    }

    #[test]
    fn in_memory_task_repository_assigns_id_and_updates() {
        let repo = InMemoryTaskRepository::new();
        let user_id = TelegramId::new(1).unwrap();

        pollster::block_on(async {
            let draft = Task::new(
                TaskId::new(0),
                user_id,
                "write report",
                TaskStatus::Ready,
                None,
                None,
                now(),
            )
            .unwrap();

            let created = repo.create(draft).await.unwrap();
            assert_eq!(created.id(), TaskId::new(1));

            let in_progress = Task::new(
                created.id(),
                user_id,
                created.title(),
                TaskStatus::InProgress,
                created.priority(),
                created.deadline(),
                created.created_at(),
            )
            .unwrap();
            repo.update(in_progress.clone()).await.unwrap();

            assert_eq!(
                repo.find_by_id(created.id()).await.unwrap(),
                Some(in_progress.clone())
            );
            assert_eq!(repo.list_by_user(user_id).await.unwrap(), vec![in_progress]);
        });
    }

    #[test]
    fn in_memory_work_day_repository_tracks_open_day_and_active_interval() {
        let repo = InMemoryWorkDayRepository::new();
        let user_id = TelegramId::new(1).unwrap();

        pollster::block_on(async {
            let draft = WorkDay::new(WorkDayId::new(0), user_id, now(), None, None, None, None);
            let created = repo.create(draft).await.unwrap();

            assert_eq!(
                repo.find_open_by_user(user_id).await.unwrap(),
                Some(created.clone())
            );
            assert_eq!(
                repo.find_open_without_active_interval().await.unwrap(),
                vec![created.clone()]
            );

            repo.set_has_active_interval(created.id(), true);
            assert_eq!(
                repo.find_open_without_active_interval().await.unwrap(),
                Vec::new()
            );

            repo.set_has_active_interval(created.id(), false);
            assert_eq!(
                repo.find_open_without_active_interval().await.unwrap(),
                vec![created]
            );
        });
    }

    #[test]
    fn in_memory_hour_interval_repository_finds_open_interval() {
        let repo = InMemoryHourIntervalRepository::new();
        let work_day_id = WorkDayId::new(1);

        pollster::block_on(async {
            assert_eq!(repo.find_open_by_work_day(work_day_id).await.unwrap(), None);

            let draft = HourInterval::new(
                HourIntervalId::new(0),
                work_day_id,
                now(),
                None,
                domain::SuccessfulCount::zero(),
                None,
            );
            let created = repo.create(draft).await.unwrap();

            assert_eq!(
                repo.find_open_by_work_day(work_day_id).await.unwrap(),
                Some(created.clone())
            );
            assert_eq!(
                repo.list_by_work_day(work_day_id).await.unwrap(),
                vec![created]
            );
        });
    }

    #[test]
    fn in_memory_ten_min_check_repository_supports_poll_queries() {
        let repo = InMemoryTenMinCheckRepository::new();
        let interval_id = HourIntervalId::new(1);
        let task_id = TaskId::new(1);
        let started_at = now();

        pollster::block_on(async {
            let draft = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval_id,
                task_id,
                started_at,
                None,
                CheckStatus::Worked,
                None,
                None,
            );
            let open_check = repo.create(draft).await.unwrap();

            assert_eq!(repo.find_open().await.unwrap(), vec![open_check.clone()]);
            assert_eq!(
                repo.find_open_by_interval(interval_id).await.unwrap(),
                Some(open_check.clone())
            );

            let closed = TenMinCheck::new(
                open_check.id(),
                interval_id,
                task_id,
                started_at,
                Some(now()),
                CheckStatus::NoResponse,
                None,
                None,
            );
            repo.update(closed.clone()).await.unwrap();

            assert_eq!(repo.find_open().await.unwrap(), Vec::new());
            assert_eq!(
                repo.find_awaiting_resume().await.unwrap(),
                vec![closed.clone()]
            );
            assert_eq!(
                repo.find_last_by_interval(interval_id).await.unwrap(),
                Some(closed)
            );

            repo.set_interval_closed(interval_id, true);
            assert_eq!(repo.find_awaiting_resume().await.unwrap(), Vec::new());
        });
    }
}
