use std::sync::Arc;

use domain::{
    CheckStatus, HourInterval, HourIntervalId, SuccessfulCount, TaskId, TenMinCheck,
    TenMinCheckId, WorkDayId,
};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::{Clock, HourIntervalRepository, TenMinCheckRepository};
use crate::use_cases::MarkTaskInProgress;

/// Ошибка команды старта часового интервала.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum StartHourIntervalError {
    /// Предыдущий интервал дня ещё не закрыт — бизнес-правило, а не сбой
    /// хранилища (README.md раздел 1: следующий интервал никогда не стартует
    /// автоматически, и открыть два одновременно нельзя).
    #[error("previous hour interval is not closed yet")]
    PreviousIntervalNotClosed,

    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Старт часового интервала только по явному действию пользователя.
/// Переводит выбранную задачу в `InProgress` (переиспользует
/// `MarkTaskInProgress` из Задачи 6) и сразу создаёт первую `TenMinCheck`
/// этого часа.
pub struct StartHourInterval {
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    mark_task_in_progress: Arc<MarkTaskInProgress>,
    clock: Arc<dyn Clock>,
}

impl StartHourInterval {
    pub fn new(
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
        mark_task_in_progress: Arc<MarkTaskInProgress>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            hour_interval_repository,
            ten_min_check_repository,
            mark_task_in_progress,
            clock,
        }
    }

    pub async fn execute(
        &self,
        work_day_id: WorkDayId,
        task_id: TaskId,
    ) -> Result<HourInterval, StartHourIntervalError> {
        if self
            .hour_interval_repository
            .find_open_by_work_day(work_day_id)
            .await?
            .is_some()
        {
            return Err(StartHourIntervalError::PreviousIntervalNotClosed);
        }

        self.mark_task_in_progress.execute(task_id).await?;

        let now = self.clock.now();

        // `HourIntervalId::new(0)` — плейсхолдер, игнорируется репозиторием:
        // id присваивается хранилищем при вставке.
        let interval_draft = HourInterval::new(
            HourIntervalId::new(0),
            work_day_id,
            now,
            None,
            SuccessfulCount::zero(),
            None,
        );
        let interval = self.hour_interval_repository.create(interval_draft).await?;

        // Плейсхолдер-статус для ещё не завершённой десятиминутки — реальное
        // значение проставляется только при закрытии (см. `SubmitTenMinAnswer`).
        let check_draft = TenMinCheck::new(
            TenMinCheckId::new(0),
            interval.id(),
            task_id,
            now,
            None,
            CheckStatus::Worked,
            None,
            None,
        );
        self.ten_min_check_repository.create(check_draft).await?;

        Ok(interval)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        FakeClock, InMemoryHourIntervalRepository, InMemoryTaskRepository,
        InMemoryTenMinCheckRepository,
    };
    use crate::ports::TaskRepository;
    use chrono::Utc;
    use domain::{Task, TaskStatus, TelegramId, UtcTimestamp};

    struct Fixture {
        use_case: StartHourInterval,
        hour_interval_repository: Arc<InMemoryHourIntervalRepository>,
        ten_min_check_repository: Arc<InMemoryTenMinCheckRepository>,
        task_repository: Arc<InMemoryTaskRepository>,
        now: UtcTimestamp,
    }

    async fn fixture() -> Fixture {
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let ten_min_check_repository = Arc::new(InMemoryTenMinCheckRepository::new());
        let task_repository = Arc::new(InMemoryTaskRepository::new());
        let now = UtcTimestamp::new(Utc::now());
        let clock = Arc::new(FakeClock::new(now));
        let mark_task_in_progress = Arc::new(MarkTaskInProgress::new(task_repository.clone()));

        let use_case = StartHourInterval::new(
            hour_interval_repository.clone(),
            ten_min_check_repository.clone(),
            mark_task_in_progress,
            clock,
        );

        Fixture {
            use_case,
            hour_interval_repository,
            ten_min_check_repository,
            task_repository,
            now,
        }
    }

    async fn seed_task(task_repository: &InMemoryTaskRepository) -> TaskId {
        let draft = Task::new(
            TaskId::new(0),
            TelegramId::new(1).unwrap(),
            "Write report",
            TaskStatus::Ready,
            None,
            None,
            UtcTimestamp::new(Utc::now()),
        )
        .unwrap();
        task_repository.create(draft).await.unwrap().id()
    }

    #[test]
    fn starts_interval_marks_task_in_progress_and_creates_first_check() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let task_id = seed_task(&fixture.task_repository).await;
            let work_day_id = WorkDayId::new(1);

            let interval = fixture
                .use_case
                .execute(work_day_id, task_id)
                .await
                .unwrap();

            assert_eq!(interval.work_day_id(), work_day_id);
            assert_eq!(interval.started_at(), fixture.now);
            assert_eq!(interval.ended_at(), None);

            let task = fixture
                .task_repository
                .find_by_id(task_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(task.status(), TaskStatus::InProgress);

            let checks = fixture.ten_min_check_repository.snapshot();
            assert_eq!(checks.len(), 1);
            assert_eq!(checks[0].hour_interval_id(), interval.id());
            assert_eq!(checks[0].task_id(), task_id);
            assert_eq!(checks[0].started_at(), fixture.now);
            assert_eq!(checks[0].ended_at(), None);
        });
    }

    #[test]
    fn rejects_start_when_previous_interval_is_still_open() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let task_id = seed_task(&fixture.task_repository).await;
            let work_day_id = WorkDayId::new(1);

            fixture.use_case.execute(work_day_id, task_id).await.unwrap();
            let error = fixture
                .use_case
                .execute(work_day_id, task_id)
                .await
                .unwrap_err();

            assert_eq!(error, StartHourIntervalError::PreviousIntervalNotClosed);
            // Второй интервал не создан.
            assert_eq!(
                fixture
                    .hour_interval_repository
                    .list_by_work_day(work_day_id)
                    .await
                    .unwrap()
                    .len(),
                1
            );
        });
    }
}
