//! Общие приватные хелперы для use cases часового интервала/десятиминуток
//! (Задача 7), переиспользуемые несколькими файлами этого модуля.

use std::sync::Arc;

use domain::{
    CheckStatus, HourIntervalId, HourIntervalState, Task, TaskStatus, TelegramId, TenMinCheck,
};

use crate::error::RepoError;
use crate::ports::{HourIntervalRepository, TaskRepository, WorkDayRepository};

/// Результат позиционной классификации десятиминуток одного интервала:
/// `closed_work_statuses` — статусы уже закрытых **рабочих** попыток (без
/// слота отдыха, см. `HourIntervalState`), `rest_check` — сама десятиминутка
/// отдыха, если она уже была создана (открыта или уже закрыта).
pub(crate) struct ClassifiedChecks {
    pub closed_work_statuses: Vec<CheckStatus>,
    pub rest_check: Option<TenMinCheck>,
}

/// Различие "рабочий слот" vs "слот отдыха" не хранится отдельным полем — это
/// позиционный факт (README.md раздел 6): слот отдыха — единственный, который
/// создаётся только когда среди уже закрытых рабочих слотов набралось 5
/// успешных (см. `HourIntervalState::is_next_slot_rest`). Принимает список
/// десятиминуток одного интервала в любом порядке, сортирует по `started_at`.
pub(crate) fn classify_checks(mut checks: Vec<TenMinCheck>) -> ClassifiedChecks {
    checks.sort_by_key(|check| check.started_at());

    let mut closed_work_statuses = Vec::new();
    let mut rest_check = None;

    for check in checks {
        if HourIntervalState::is_next_slot_rest(&closed_work_statuses) {
            rest_check = Some(check);
        } else if check.ended_at().is_some() {
            closed_work_statuses.push(check.status());
        }
    }

    ClassifiedChecks {
        closed_work_statuses,
        rest_check,
    }
}

/// `TelegramId` пользователя, которому принадлежит часовой интервал — нужен
/// для отправки `Notifier.send` из use cases, оперирующих `HourIntervalId`/
/// `TenMinCheckId`, а не напрямую `TelegramId`.
pub(crate) async fn resolve_user_id(
    hour_interval_repository: &Arc<dyn HourIntervalRepository>,
    work_day_repository: &Arc<dyn WorkDayRepository>,
    hour_interval_id: HourIntervalId,
) -> Result<TelegramId, RepoError> {
    let interval = hour_interval_repository
        .find_by_id(hour_interval_id)
        .await?
        .ok_or(RepoError::NotFound)?;
    let work_day = work_day_repository
        .find_by_id(interval.work_day_id())
        .await?
        .ok_or(RepoError::NotFound)?;
    Ok(work_day.user_id())
}

/// Задача пользователя, сейчас находящаяся в `InProgress` — источник истины
/// для "текущей активной задачи интервала" при создании очередной рабочей
/// десятиминутки (`SubmitTenMinAnswer`/`ConfirmReadyToContinue`), а также при
/// поиске задачи, которую нужно закрыть в `SwitchTaskMidInterval`. Ошибка,
/// если такой задачи нет — не должно случаться при корректном использовании
/// (задача переводится в `InProgress` стартом интервала и остаётся им, пока
/// интервал активен).
pub(crate) async fn find_in_progress_task(
    task_repository: &Arc<dyn TaskRepository>,
    user_id: TelegramId,
) -> Result<Task, RepoError> {
    task_repository
        .list_by_user(user_id)
        .await?
        .into_iter()
        .find(|task| task.status() == TaskStatus::InProgress)
        .ok_or(RepoError::NotFound)
}
