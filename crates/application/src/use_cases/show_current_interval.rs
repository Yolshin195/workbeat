use std::sync::Arc;

use chrono::Duration;
use domain::{TaskId, TelegramId, WorkDayId};

use crate::error::RepoError;
use crate::ports::{Clock, HourIntervalRepository, TaskRepository, TenMinCheckRepository, WorkDayRepository};
use crate::use_cases::support::task_time_totals_for_interval;

/// Статус текущего часового интервала (`/current_interval`, issue #38):
/// активная задача, время, прошедшее в этом интервале, и суммарное время,
/// потраченное на эту задачу за всё время (README.md раздел 3 — сумма
/// успешных десятиминуток по всем интервалам, где эта задача была активна, не
/// только за сегодня).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentIntervalStatus {
    pub task_id: TaskId,
    pub task_title: String,
    pub elapsed_in_interval: Duration,
    pub total_task_time: Duration,
}

/// Read-only use case — не меняет состояние. Активная задача определяется по
/// последней десятиминутке текущего интервала (`find_last_by_interval`), а не
/// по кэшированному в сессии Telegram-адаптера состоянию, чтобы не зависеть
/// от рестарта процесса (`ChatSession` не переживает рестарт, см.
/// `session.rs` в адаптере). Суммарное время по задаче считается тем же
/// способом, что и `BuildPeriodReport` (обход всех `work_days` пользователя),
/// но без фильтра по датам.
pub struct ShowCurrentInterval {
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    work_day_repository: Arc<dyn WorkDayRepository>,
    task_repository: Arc<dyn TaskRepository>,
    clock: Arc<dyn Clock>,
}

impl ShowCurrentInterval {
    pub fn new(
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
        work_day_repository: Arc<dyn WorkDayRepository>,
        task_repository: Arc<dyn TaskRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            hour_interval_repository,
            ten_min_check_repository,
            work_day_repository,
            task_repository,
            clock,
        }
    }

    /// `Ok(None)`, если для этого дня нет открытого интервала.
    pub async fn execute(
        &self,
        user_id: TelegramId,
        work_day_id: WorkDayId,
    ) -> Result<Option<CurrentIntervalStatus>, RepoError> {
        let Some(open) = self
            .hour_interval_repository
            .find_open_by_work_day(work_day_id)
            .await?
        else {
            return Ok(None);
        };

        let last_check = self
            .ten_min_check_repository
            .find_last_by_interval(open.id())
            .await?
            .ok_or(RepoError::NotFound)?;
        let task_id = last_check.task_id();
        let elapsed_in_interval = self.clock.now().value() - open.started_at().value();

        let mut total_task_time = Duration::zero();
        for work_day in self.work_day_repository.list_by_user(user_id).await? {
            let intervals = self
                .hour_interval_repository
                .list_by_work_day(work_day.id())
                .await?;
            for interval in intervals {
                let checks = self
                    .ten_min_check_repository
                    .list_by_interval(interval.id())
                    .await?;
                for (id, duration) in task_time_totals_for_interval(checks) {
                    if id == task_id {
                        total_task_time += duration;
                    }
                }
            }
        }

        let task_title = self
            .task_repository
            .find_by_id(task_id)
            .await?
            .map(|task| task.title().to_string())
            .unwrap_or_default();

        Ok(Some(CurrentIntervalStatus {
            task_id,
            task_title,
            elapsed_in_interval,
            total_task_time,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        FakeClock, InMemoryHourIntervalRepository, InMemoryTaskRepository,
        InMemoryTenMinCheckRepository, InMemoryWorkDayRepository,
    };
    use chrono::Utc;
    use domain::{
        CheckStatus, HourInterval, HourIntervalId, SuccessfulCount, Task, TaskStatus, TenMinCheck,
        TenMinCheckId, UtcTimestamp, WorkDay, WorkDayId,
    };

    struct Fixture {
        use_case: ShowCurrentInterval,
        hour_interval_repository: Arc<InMemoryHourIntervalRepository>,
        ten_min_check_repository: Arc<InMemoryTenMinCheckRepository>,
        work_day_repository: Arc<InMemoryWorkDayRepository>,
        task_repository: Arc<InMemoryTaskRepository>,
        clock: Arc<FakeClock>,
        user_id: TelegramId,
        work_day_id: WorkDayId,
    }

    async fn fixture() -> Fixture {
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let ten_min_check_repository = Arc::new(InMemoryTenMinCheckRepository::new());
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let task_repository = Arc::new(InMemoryTaskRepository::new());
        let now = UtcTimestamp::new(Utc::now());
        let clock = Arc::new(FakeClock::new(now));
        let user_id = TelegramId::new(1).unwrap();

        let work_day = WorkDay::new(WorkDayId::new(0), user_id, now, None, None, None, None);
        let work_day = work_day_repository.create(work_day).await.unwrap();

        let use_case = ShowCurrentInterval::new(
            hour_interval_repository.clone(),
            ten_min_check_repository.clone(),
            work_day_repository.clone(),
            task_repository.clone(),
            clock.clone(),
        );

        Fixture {
            use_case,
            hour_interval_repository,
            ten_min_check_repository,
            work_day_repository,
            task_repository,
            clock,
            user_id,
            work_day_id: work_day.id(),
        }
    }

    async fn seed_task(repo: &InMemoryTaskRepository, user_id: TelegramId, title: &str, now: UtcTimestamp) -> TaskId {
        let draft = Task::new(TaskId::new(0), user_id, title, TaskStatus::InProgress, None, None, now)
            .unwrap();
        repo.create(draft).await.unwrap().id()
    }

    #[test]
    fn returns_none_when_no_open_interval() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let result = fixture
                .use_case
                .execute(fixture.user_id, fixture.work_day_id)
                .await
                .unwrap();
            assert_eq!(result, None);
        });
    }

    #[test]
    fn reports_active_task_and_elapsed_time_of_open_interval() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let task_id = seed_task(&fixture.task_repository, fixture.user_id, "Write report", fixture.clock.now()).await;

            let interval = HourInterval::new(
                HourIntervalId::new(0),
                fixture.work_day_id,
                fixture.clock.now(),
                None,
                SuccessfulCount::zero(),
                None,
            );
            let interval = fixture.hour_interval_repository.create(interval).await.unwrap();

            let check = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval.id(),
                task_id,
                fixture.clock.now(),
                None,
                CheckStatus::Worked,
                None,
                None,
            );
            fixture.ten_min_check_repository.create(check).await.unwrap();

            fixture.clock.advance(Duration::minutes(7));

            let status = fixture
                .use_case
                .execute(fixture.user_id, fixture.work_day_id)
                .await
                .unwrap()
                .unwrap();

            assert_eq!(status.task_id, task_id);
            assert_eq!(status.task_title, "Write report");
            assert_eq!(status.elapsed_in_interval, Duration::minutes(7));
        });
    }

    #[test]
    fn sums_task_time_across_past_closed_intervals_and_other_work_days() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let task_id = seed_task(&fixture.task_repository, fixture.user_id, "Write report", fixture.clock.now()).await;

            // Прошлый рабочий день с одним полностью закрытым интервалом
            // (5 успешных + отдых = 60 минут по этой задаче).
            let past_day = WorkDay::new(
                WorkDayId::new(0),
                fixture.user_id,
                UtcTimestamp::new(fixture.clock.now().value() - Duration::days(1)),
                Some(fixture.clock.now()),
                None,
                None,
                None,
            );
            let past_day = fixture.work_day_repository.create(past_day).await.unwrap();
            let past_interval = HourInterval::new(
                HourIntervalId::new(0),
                past_day.id(),
                fixture.clock.now(),
                Some(fixture.clock.now()),
                SuccessfulCount::new(5).unwrap(),
                Some("done".to_string()),
            );
            let past_interval = fixture
                .hour_interval_repository
                .create(past_interval)
                .await
                .unwrap();
            let mut t = fixture.clock.now();
            for _ in 0..6 {
                let check = TenMinCheck::new(
                    TenMinCheckId::new(0),
                    past_interval.id(),
                    task_id,
                    t,
                    Some(UtcTimestamp::new(t.value() + Duration::minutes(10))),
                    CheckStatus::Worked,
                    None,
                    None,
                );
                fixture.ten_min_check_repository.create(check).await.unwrap();
                t = UtcTimestamp::new(t.value() + Duration::minutes(10));
            }

            // Сегодняшний открытый интервал — ещё 1 успешная десятиминутка.
            let open_interval = HourInterval::new(
                HourIntervalId::new(0),
                fixture.work_day_id,
                fixture.clock.now(),
                None,
                SuccessfulCount::zero(),
                None,
            );
            let open_interval = fixture
                .hour_interval_repository
                .create(open_interval)
                .await
                .unwrap();
            let open_check = TenMinCheck::new(
                TenMinCheckId::new(0),
                open_interval.id(),
                task_id,
                fixture.clock.now(),
                Some(UtcTimestamp::new(fixture.clock.now().value() + Duration::minutes(10))),
                CheckStatus::Worked,
                None,
                None,
            );
            fixture.ten_min_check_repository.create(open_check).await.unwrap();
            // Открытая (текущая) десятиминутка — ещё не закрыта, в итог не входит.
            let ongoing_check = TenMinCheck::new(
                TenMinCheckId::new(0),
                open_interval.id(),
                task_id,
                UtcTimestamp::new(fixture.clock.now().value() + Duration::minutes(10)),
                None,
                CheckStatus::Worked,
                None,
                None,
            );
            fixture
                .ten_min_check_repository
                .create(ongoing_check)
                .await
                .unwrap();

            let status = fixture
                .use_case
                .execute(fixture.user_id, fixture.work_day_id)
                .await
                .unwrap()
                .unwrap();

            // 60 минут (прошлый закрытый интервал) + 10 минут (сегодняшняя
            // закрытая десятиминутка) = 70 минут. Открытая десятиминутка не
            // учитывается.
            assert_eq!(status.total_task_time, Duration::minutes(70));
        });
    }
}
