use async_trait::async_trait;
use domain::{TelegramId, WorkDay, WorkDayId};

use crate::error::RepoError;

/// `id` внутри `WorkDay`, переданного в `create`, реализацией игнорируется —
/// см. пояснение в `TaskRepository`.
#[async_trait]
pub trait WorkDayRepository: Send + Sync {
    async fn create(&self, work_day: WorkDay) -> Result<WorkDay, RepoError>;

    async fn update(&self, work_day: WorkDay) -> Result<(), RepoError>;

    async fn find_by_id(&self, id: WorkDayId) -> Result<Option<WorkDay>, RepoError>;

    /// Открытый (`finished_at IS NULL`) день пользователя, если есть — нужен
    /// для проверки "день уже открыт" в `StartWorkDay` (Задача 5) и для
    /// обеда/завершения дня (Задача 8).
    async fn find_open_by_user(&self, user_id: TelegramId) -> Result<Option<WorkDay>, RepoError>;

    /// Все `work_days` пользователя — основа агрегированных отчётов за
    /// неделю/месяц (`BuildPeriodReport`, Задача 9). Фильтрация по периоду
    /// делается в `application` по календарной дате в таймзоне пользователя,
    /// а не здесь — иначе конвертация таймзоны продублируется в адаптере.
    async fn list_by_user(&self, user_id: TelegramId) -> Result<Vec<WorkDay>, RepoError>;

    /// Кандидаты на `PromptContinueIfIdle` (Задача 8, вызывается poll-циклом
    /// из Задачи 10) — открытые дни, у которых сейчас нет незакрытого
    /// `HourInterval`. Единственный механизм "отмены" здесь — то, что день
    /// просто перестаёт попадать в эту выборку после `StartHourInterval`/
    /// `FinishWorkDay`, отдельного шага не требуется.
    async fn find_open_without_active_interval(&self) -> Result<Vec<WorkDay>, RepoError>;
}
