use async_trait::async_trait;
use domain::{Task, TaskId, TelegramId};

use crate::error::RepoError;

/// `id` внутри `Task`, переданного в `create`, реализацией игнорируется —
/// реальный id присваивается хранилищем при вставке (SQLite `INTEGER PRIMARY
/// KEY`); возвращается сущность с этим реальным id.
#[async_trait]
pub trait TaskRepository: Send + Sync {
    async fn create(&self, task: Task) -> Result<Task, RepoError>;

    async fn update(&self, task: Task) -> Result<(), RepoError>;

    async fn find_by_id(&self, id: TaskId) -> Result<Option<Task>, RepoError>;

    /// Весь пул задач пользователя. Фильтрация по статусу и сортировка по
    /// приоритету/дедлайну (`ListAvailableTasks`, Задача 6) — бизнес-правило,
    /// поэтому живёт в `application`, а не здесь.
    async fn list_by_user(&self, user_id: TelegramId) -> Result<Vec<Task>, RepoError>;
}
