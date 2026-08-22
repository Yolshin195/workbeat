use std::sync::Arc;

use chrono::NaiveDate;
use domain::{Task, TaskPriority, TaskStatus, TelegramId};

use crate::error::RepoError;
use crate::ports::TaskRepository;

/// Фильтр пула задач для `ListAvailableTasks`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFilter {
    Ready,
    NotReady,
    All,
}

/// Список пула задач пользователя, отсортированный по приоритету
/// (`high` → `medium` → `low` → без приоритета), затем по дедлайну (раньше —
/// выше, без дедлайна — в конце).
pub struct ListAvailableTasks {
    task_repository: Arc<dyn TaskRepository>,
}

impl ListAvailableTasks {
    pub fn new(task_repository: Arc<dyn TaskRepository>) -> Self {
        Self { task_repository }
    }

    pub async fn execute(
        &self,
        user_id: TelegramId,
        filter: TaskFilter,
    ) -> Result<Vec<Task>, RepoError> {
        let mut tasks: Vec<Task> = self
            .task_repository
            .list_by_user(user_id)
            .await?
            .into_iter()
            .filter(|task| match filter {
                TaskFilter::Ready => task.status() == TaskStatus::Ready,
                TaskFilter::NotReady => task.status() == TaskStatus::NotReady,
                TaskFilter::All => true,
            })
            .collect();

        tasks.sort_by_key(|task| sort_key(task.priority(), task.deadline()));
        Ok(tasks)
    }
}

fn sort_key(priority: Option<TaskPriority>, deadline: Option<NaiveDate>) -> (u8, u8, NaiveDate) {
    let priority_rank = match priority {
        Some(TaskPriority::High) => 0,
        Some(TaskPriority::Medium) => 1,
        Some(TaskPriority::Low) => 2,
        None => 3,
    };
    // Дедлайн без значения сортируется в конец: (1, MAX) всегда больше
    // (0, любая реальная дата), а сама дата внутри "есть дедлайн" не важна.
    let (deadline_rank, deadline_value) = match deadline {
        Some(date) => (0, date),
        None => (1, NaiveDate::MAX),
    };
    (priority_rank, deadline_rank, deadline_value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::InMemoryTaskRepository;
    use chrono::Utc;
    use domain::{TaskId, UtcTimestamp};

    fn use_case() -> (ListAvailableTasks, Arc<InMemoryTaskRepository>) {
        let task_repository = Arc::new(InMemoryTaskRepository::new());
        (
            ListAvailableTasks::new(task_repository.clone()),
            task_repository,
        )
    }

    async fn seed(
        repo: &InMemoryTaskRepository,
        user_id: TelegramId,
        title: &str,
        status: TaskStatus,
        priority: Option<TaskPriority>,
        deadline: Option<NaiveDate>,
    ) -> Task {
        let draft = Task::new(
            TaskId::new(0),
            user_id,
            title,
            status,
            priority,
            deadline,
            UtcTimestamp::new(Utc::now()),
        )
        .unwrap();
        repo.create(draft).await.unwrap()
    }

    fn date(day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(2026, 1, day).unwrap()
    }

    #[test]
    fn filters_by_status() {
        let (use_case, repo) = use_case();
        let user_id = TelegramId::new(1).unwrap();

        pollster::block_on(async {
            let ready = seed(&repo, user_id, "ready", TaskStatus::Ready, None, None).await;
            let not_ready = seed(
                &repo,
                user_id,
                "not ready",
                TaskStatus::NotReady,
                None,
                None,
            )
            .await;
            seed(&repo, user_id, "done", TaskStatus::Done, None, None).await;

            assert_eq!(
                use_case.execute(user_id, TaskFilter::Ready).await.unwrap(),
                vec![ready.clone()]
            );
            assert_eq!(
                use_case
                    .execute(user_id, TaskFilter::NotReady)
                    .await
                    .unwrap(),
                vec![not_ready]
            );
            assert_eq!(
                use_case
                    .execute(user_id, TaskFilter::All)
                    .await
                    .unwrap()
                    .len(),
                3
            );
        });
    }

    #[test]
    fn sorts_by_priority_then_deadline_with_missing_values_last() {
        let (use_case, repo) = use_case();
        let user_id = TelegramId::new(1).unwrap();

        // Табличный сценарий: разные комбинации присутствия/отсутствия
        // priority и deadline вперемешку — ожидаемый порядок ниже.
        let cases: [(&str, Option<TaskPriority>, Option<NaiveDate>); 7] = [
            ("low, no deadline", Some(TaskPriority::Low), None),
            ("no priority, no deadline", None, None),
            (
                "high, late deadline",
                Some(TaskPriority::High),
                Some(date(20)),
            ),
            (
                "medium, deadline",
                Some(TaskPriority::Medium),
                Some(date(5)),
            ),
            (
                "high, early deadline",
                Some(TaskPriority::High),
                Some(date(1)),
            ),
            ("no priority, deadline", None, Some(date(1))),
            ("high, no deadline", Some(TaskPriority::High), None),
        ];

        pollster::block_on(async {
            for (title, priority, deadline) in cases {
                seed(&repo, user_id, title, TaskStatus::Ready, priority, deadline).await;
            }
        });

        let sorted = pollster::block_on(use_case.execute(user_id, TaskFilter::All)).unwrap();
        let titles: Vec<&str> = sorted.iter().map(Task::title).collect();

        assert_eq!(
            titles,
            vec![
                "high, early deadline",
                "high, late deadline",
                "high, no deadline",
                "medium, deadline",
                "low, no deadline",
                "no priority, deadline",
                "no priority, no deadline",
            ]
        );
    }
}
