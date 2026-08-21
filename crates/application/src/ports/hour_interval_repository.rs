use async_trait::async_trait;
use domain::{HourInterval, HourIntervalId, WorkDayId};

use crate::error::RepoError;

/// `id` внутри `HourInterval`, переданного в `create`, реализацией
/// игнорируется — см. пояснение в `TaskRepository`.
#[async_trait]
pub trait HourIntervalRepository: Send + Sync {
    async fn create(&self, interval: HourInterval) -> Result<HourInterval, RepoError>;

    async fn update(&self, interval: HourInterval) -> Result<(), RepoError>;

    async fn find_by_id(&self, id: HourIntervalId) -> Result<Option<HourInterval>, RepoError>;

    /// Незакрытый (`ended_at IS NULL`) интервал дня, если есть — нужен для
    /// проверки "предыдущий интервал не закрыт" в `StartHourInterval`
    /// (Задача 7) и для обеда/завершения дня (Задача 8).
    async fn find_open_by_work_day(
        &self,
        work_day_id: WorkDayId,
    ) -> Result<Option<HourInterval>, RepoError>;

    /// Все интервалы дня (закрытые и нет) — для отчётов (Задача 9) и подсчёта
    /// дневной нормы/автопредложения обеда после 4-го закрытого интервала
    /// (Задача 8).
    async fn list_by_work_day(
        &self,
        work_day_id: WorkDayId,
    ) -> Result<Vec<HourInterval>, RepoError>;
}
