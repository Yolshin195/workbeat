use async_trait::async_trait;
use domain::{HourIntervalId, TenMinCheck, TenMinCheckId};

use crate::error::RepoError;

/// `id` внутри `TenMinCheck`, переданного в `create`, реализацией
/// игнорируется — см. пояснение в `TaskRepository`.
#[async_trait]
pub trait TenMinCheckRepository: Send + Sync {
    async fn create(&self, check: TenMinCheck) -> Result<TenMinCheck, RepoError>;

    async fn update(&self, check: TenMinCheck) -> Result<(), RepoError>;

    async fn find_by_id(&self, id: TenMinCheckId) -> Result<Option<TenMinCheck>, RepoError>;

    /// Открытая (`ended_at IS NULL`) десятиминутка интервала — рабочая или
    /// отдых, в один момент времени она максимум одна. Нужна
    /// `SubmitTenMinAnswer`/`MarkReturnedFromRest` (Задача 7), чтобы закрыть
    /// именно её.
    async fn find_open_by_interval(
        &self,
        hour_interval_id: HourIntervalId,
    ) -> Result<Option<TenMinCheck>, RepoError>;

    /// Последняя по времени старта десятиминутка интервала (открытая или уже
    /// закрытая) — нужна `ConfirmReadyToContinue` (Задача 7), чтобы
    /// проверить, что подтверждать действительно есть что (последняя строка
    /// должна быть закрытой `Failed`/`NoResponse`).
    async fn find_last_by_interval(
        &self,
        hour_interval_id: HourIntervalId,
    ) -> Result<Option<TenMinCheck>, RepoError>;

    /// Все десятиминутки интервала — для расчёта `successful_count`
    /// (`HourIntervalState`, Задача 2/7) и для отчётов (Задача 9).
    async fn list_by_interval(
        &self,
        hour_interval_id: HourIntervalId,
    ) -> Result<Vec<TenMinCheck>, RepoError>;

    /// Все открытые десятиминутки по всем активным интервалам — кандидаты на
    /// `WorkSlotPollDecision` (если это рабочий слот) либо
    /// `ReminderScheduleDecision` (если это переработанный отдых), см.
    /// `AdvanceOpenTenMinChecks` (Задача 7, вызывается poll-циклом из Задачи
    /// 10). Единственный механизм "отмены" — закрытая (`ended_at IS NOT
    /// NULL`) строка просто перестаёт сюда попадать на следующем тике.
    async fn find_open(&self) -> Result<Vec<TenMinCheck>, RepoError>;

    /// Закрытые `Failed`/`NoResponse`-десятиминутки, являющиеся последней по
    /// времени десятиминуткой своего ещё не закрытого `HourInterval` (то есть
    /// следующая десятиминутка ещё не началась) — кандидаты на
    /// `ReminderScheduleDecision` с текстом "вернись к работе"/"что
    /// случилось" (`RemindAwaitingResume`, Задача 7/10). Как только по этому
    /// слоту стартует новая десятиминутка (или интервал закрывается), он
    /// перестаёт попадать в эту выборку на следующем тике.
    async fn find_awaiting_resume(&self) -> Result<Vec<TenMinCheck>, RepoError>;
}
