use std::sync::Arc;

use domain::{CheckStatus, HourIntervalId, TenMinCheck, TenMinCheckId};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::{Clock, HourIntervalRepository, TaskRepository, TenMinCheckRepository, WorkDayRepository};
use crate::use_cases::support::{find_in_progress_task, resolve_user_id};

/// Ошибка "готов продолжить?".
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ConfirmReadyToContinueError {
    /// Последняя десятиминутка интервала не является закрытой
    /// `Failed`/`NoResponse` — подтверждать нечего (либо уже идёт новая
    /// десятиминутка, либо интервал принудительно завершён).
    #[error("nothing to confirm: last ten-min check is not a closed failure")]
    NothingToConfirm,

    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Реакция на "готов продолжить?" после провала рабочей десятиминутки —
/// применима после **любого** провала (README.md раздел 2, п.2): создаёт
/// новую `TenMinCheck` для следующего слота, `started_at` — момент
/// подтверждения (а не момент закрытия предыдущей).
pub struct ConfirmReadyToContinue {
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    work_day_repository: Arc<dyn WorkDayRepository>,
    task_repository: Arc<dyn TaskRepository>,
    clock: Arc<dyn Clock>,
}

impl ConfirmReadyToContinue {
    pub fn new(
        ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        work_day_repository: Arc<dyn WorkDayRepository>,
        task_repository: Arc<dyn TaskRepository>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            ten_min_check_repository,
            hour_interval_repository,
            work_day_repository,
            task_repository,
            clock,
        }
    }

    pub async fn execute(
        &self,
        interval_id: HourIntervalId,
    ) -> Result<TenMinCheck, ConfirmReadyToContinueError> {
        let last = self
            .ten_min_check_repository
            .find_last_by_interval(interval_id)
            .await?
            .ok_or(RepoError::NotFound)?;

        let is_awaiting_resume = last.ended_at().is_some()
            && matches!(last.status(), CheckStatus::Failed | CheckStatus::NoResponse);
        if !is_awaiting_resume {
            return Err(ConfirmReadyToContinueError::NothingToConfirm);
        }

        let user_id =
            resolve_user_id(&self.hour_interval_repository, &self.work_day_repository, interval_id)
                .await?;
        let active_task = find_in_progress_task(&self.task_repository, user_id).await?;

        let draft = TenMinCheck::new(
            TenMinCheckId::new(0),
            interval_id,
            active_task.id(),
            self.clock.now(),
            None,
            CheckStatus::Worked,
            None,
            None,
        );
        Ok(self.ten_min_check_repository.create(draft).await?)
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
    use domain::{HourInterval, SuccessfulCount, Task, TaskId, TaskStatus, TelegramId, UtcTimestamp, WorkDay, WorkDayId};

    struct Fixture {
        use_case: ConfirmReadyToContinue,
        ten_min_check_repository: Arc<InMemoryTenMinCheckRepository>,
        clock: Arc<FakeClock>,
        interval_id: HourIntervalId,
        task_id: TaskId,
    }

    async fn fixture() -> Fixture {
        let ten_min_check_repository = Arc::new(InMemoryTenMinCheckRepository::new());
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let task_repository = Arc::new(InMemoryTaskRepository::new());
        let now = UtcTimestamp::new(Utc::now());
        let clock = Arc::new(FakeClock::new(now));

        let user_id = TelegramId::new(1).unwrap();
        let work_day = WorkDay::new(WorkDayId::new(0), user_id, now, None, None, None, None);
        let work_day = work_day_repository.create(work_day).await.unwrap();

        let task = Task::new(
            TaskId::new(0),
            user_id,
            "Write report",
            TaskStatus::InProgress,
            None,
            None,
            now,
        )
        .unwrap();
        let task = task_repository.create(task).await.unwrap();

        let interval = HourInterval::new(
            HourIntervalId::new(0),
            work_day.id(),
            now,
            None,
            SuccessfulCount::zero(),
            None,
        );
        let interval = hour_interval_repository.create(interval).await.unwrap();

        let use_case = ConfirmReadyToContinue::new(
            ten_min_check_repository.clone(),
            hour_interval_repository,
            work_day_repository,
            task_repository,
            clock.clone(),
        );

        Fixture {
            use_case,
            ten_min_check_repository,
            clock,
            interval_id: interval.id(),
            task_id: task.id(),
        }
    }

    #[test]
    fn creates_new_check_started_at_confirmation_moment_not_close_moment() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let closed_at = fixture.clock.now();
            let failed = TenMinCheck::new(
                TenMinCheckId::new(0),
                fixture.interval_id,
                fixture.task_id,
                closed_at,
                Some(closed_at),
                CheckStatus::Failed,
                Some("distracted".to_string()),
                None,
            );
            fixture
                .ten_min_check_repository
                .create(failed)
                .await
                .unwrap();

            fixture.clock.advance(chrono::Duration::minutes(7));
            let confirm_at = fixture.clock.now();

            let created = fixture.use_case.execute(fixture.interval_id).await.unwrap();

            assert_eq!(created.started_at(), confirm_at);
            assert_ne!(created.started_at(), closed_at);
            assert_eq!(created.task_id(), fixture.task_id);
            assert_eq!(created.ended_at(), None);
        });
    }

    #[test]
    fn works_after_no_response_close_too() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let no_response = TenMinCheck::new(
                TenMinCheckId::new(0),
                fixture.interval_id,
                fixture.task_id,
                fixture.clock.now(),
                Some(fixture.clock.now()),
                CheckStatus::NoResponse,
                None,
                None,
            );
            fixture
                .ten_min_check_repository
                .create(no_response)
                .await
                .unwrap();

            let created = fixture.use_case.execute(fixture.interval_id).await.unwrap();
            assert_eq!(created.ended_at(), None);
        });
    }

    #[test]
    fn rejects_when_last_check_is_already_open() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let open = TenMinCheck::new(
                TenMinCheckId::new(0),
                fixture.interval_id,
                fixture.task_id,
                fixture.clock.now(),
                None,
                CheckStatus::Worked,
                None,
                None,
            );
            fixture
                .ten_min_check_repository
                .create(open)
                .await
                .unwrap();

            let error = fixture.use_case.execute(fixture.interval_id).await.unwrap_err();
            assert_eq!(error, ConfirmReadyToContinueError::NothingToConfirm);
        });
    }

    #[test]
    fn rejects_when_last_check_is_closed_worked() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let worked = TenMinCheck::new(
                TenMinCheckId::new(0),
                fixture.interval_id,
                fixture.task_id,
                fixture.clock.now(),
                Some(fixture.clock.now()),
                CheckStatus::Worked,
                None,
                None,
            );
            fixture
                .ten_min_check_repository
                .create(worked)
                .await
                .unwrap();

            let error = fixture.use_case.execute(fixture.interval_id).await.unwrap_err();
            assert_eq!(error, ConfirmReadyToContinueError::NothingToConfirm);
        });
    }
}
