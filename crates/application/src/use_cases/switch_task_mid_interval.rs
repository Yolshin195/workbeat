use std::sync::Arc;

use domain::{HourIntervalId, Task, TaskId};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::{HourIntervalRepository, TaskRepository, WorkDayRepository};
use crate::use_cases::support::{find_in_progress_task, resolve_user_id};
use crate::use_cases::{MarkTaskDone, MarkTaskInProgress};

/// Ошибка "задача готова" (переключение задачи посреди часового интервала).
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SwitchTaskMidIntervalError {
    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Реализация правила "задача готова" (README.md раздел 3): закрывает
/// текущую активную задачу пользователя и переводит новую в `InProgress`.
/// Уже стартовавшая (открытая) десятиминутка НЕ прерывается и НЕ меняет
/// `task_id` задним числом — она естественно завершается под прежней
/// задачей; оставшиеся десятиминутки этого часа подхватят `new_task_id`
/// сами, потому что `SubmitTenMinAnswer`/`ConfirmReadyToContinue` при
/// создании нового слота ищут текущую `InProgress`-задачу заново, а не
/// копируют `task_id` закрываемого слота.
pub struct SwitchTaskMidInterval {
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    work_day_repository: Arc<dyn WorkDayRepository>,
    task_repository: Arc<dyn TaskRepository>,
    mark_task_done: Arc<MarkTaskDone>,
    mark_task_in_progress: Arc<MarkTaskInProgress>,
}

impl SwitchTaskMidInterval {
    pub fn new(
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        work_day_repository: Arc<dyn WorkDayRepository>,
        task_repository: Arc<dyn TaskRepository>,
        mark_task_done: Arc<MarkTaskDone>,
        mark_task_in_progress: Arc<MarkTaskInProgress>,
    ) -> Self {
        Self {
            hour_interval_repository,
            work_day_repository,
            task_repository,
            mark_task_done,
            mark_task_in_progress,
        }
    }

    pub async fn execute(
        &self,
        interval_id: HourIntervalId,
        new_task_id: TaskId,
    ) -> Result<Task, SwitchTaskMidIntervalError> {
        let user_id =
            resolve_user_id(&self.hour_interval_repository, &self.work_day_repository, interval_id)
                .await?;
        let current_task = find_in_progress_task(&self.task_repository, user_id).await?;

        self.mark_task_done.execute(current_task.id()).await?;
        let updated = self.mark_task_in_progress.execute(new_task_id).await?;

        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        InMemoryHourIntervalRepository, InMemoryTaskRepository, InMemoryWorkDayRepository,
    };
    use chrono::Utc;
    use domain::{
        HourInterval, SuccessfulCount, TaskStatus, TelegramId, UtcTimestamp, WorkDay, WorkDayId,
    };

    #[test]
    fn closes_current_task_and_marks_new_one_in_progress() {
        pollster::block_on(async {
            let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
            let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
            let task_repository = Arc::new(InMemoryTaskRepository::new());
            let now = UtcTimestamp::new(Utc::now());
            let user_id = TelegramId::new(1).unwrap();

            let work_day = WorkDay::new(WorkDayId::new(0), user_id, now, None, None, None, None);
            let work_day = work_day_repository.create(work_day).await.unwrap();

            let interval = HourInterval::new(
                HourIntervalId::new(0),
                work_day.id(),
                now,
                None,
                SuccessfulCount::zero(),
                None,
            );
            let interval = hour_interval_repository.create(interval).await.unwrap();

            let current_task = domain::Task::new(
                TaskId::new(0),
                user_id,
                "Write report",
                TaskStatus::InProgress,
                None,
                None,
                now,
            )
            .unwrap();
            let current_task = task_repository.create(current_task).await.unwrap();

            let next_task = domain::Task::new(
                TaskId::new(0),
                user_id,
                "Review PR",
                TaskStatus::Ready,
                None,
                None,
                now,
            )
            .unwrap();
            let next_task = task_repository.create(next_task).await.unwrap();

            let use_case = SwitchTaskMidInterval::new(
                hour_interval_repository,
                work_day_repository,
                task_repository.clone(),
                Arc::new(MarkTaskDone::new(task_repository.clone())),
                Arc::new(MarkTaskInProgress::new(task_repository.clone())),
            );

            let updated = use_case
                .execute(interval.id(), next_task.id())
                .await
                .unwrap();

            assert_eq!(updated.id(), next_task.id());
            assert_eq!(updated.status(), TaskStatus::InProgress);

            let old = task_repository
                .find_by_id(current_task.id())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(old.status(), TaskStatus::Done);
        });
    }
}
