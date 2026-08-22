use std::sync::Arc;

use domain::{append_reason_to_no_response, DomainError, HourIntervalId, TenMinCheck};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::TenMinCheckRepository;

/// Ошибка дозаписи причины в уже закрытую `NoResponse`-десятиминутку.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SubmitFailureReasonError {
    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Дописывает `reason` в уже закрытую десятиминутку со статусом `NoResponse`
/// и пустым `reason` (сценарий "тишина" из README.md раздела 2, п.2):
/// пользователь наконец откликнулся и объяснил, что случилось. Не меняет
/// `ended_at`/`status` — единственный use case, разрешающий изменение уже
/// закрытой `TenMinCheck`.
///
/// Принимает `interval_id`, а не `check_id` конкретной десятиминутки:
/// вызывающая сторона (Telegram-адаптер) в сценарии "тишина" никогда не
/// узнаёт `check_id` десятиминутки, которую молча закрыл `PollLoop`
/// (`AdvanceOpenTenMinChecks`, Задача 7/10) — она создаётся и закрывается
/// целиком внутри опроса, без какого-либо вызова из адаптера, которому
/// можно было бы вернуть id. Разрешение "какая именно строка имеется в
/// виду" — последняя по времени десятиминутка интервала — поэтому сделано
/// здесь, тем же способом, что уже использует `ConfirmReadyToContinue`
/// (`find_last_by_interval`), а не переложено на вызывающую сторону.
pub struct SubmitFailureReason {
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
}

impl SubmitFailureReason {
    pub fn new(ten_min_check_repository: Arc<dyn TenMinCheckRepository>) -> Self {
        Self {
            ten_min_check_repository,
        }
    }

    pub async fn execute(
        &self,
        interval_id: HourIntervalId,
        reason: String,
    ) -> Result<TenMinCheck, SubmitFailureReasonError> {
        let check = self
            .ten_min_check_repository
            .find_last_by_interval(interval_id)
            .await?
            .ok_or(RepoError::NotFound)?;

        // Если последняя десятиминутка интервала ещё открыта, у неё
        // плейсхолдер-статус (см. `StartHourInterval`) — `append_reason_to_no_response`
        // и в этом случае корректно отклонит попытку как
        // `ReasonNotAllowedForStatus`, ничего отдельно проверять не нужно.
        append_reason_to_no_response(check.status(), check.reason())?;

        let updated = TenMinCheck::new(
            check.id(),
            check.hour_interval_id(),
            check.task_id(),
            check.started_at(),
            check.ended_at(),
            check.status(),
            Some(reason),
            check.last_reminder_at(),
        );
        self.ten_min_check_repository
            .update(updated.clone())
            .await?;

        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::InMemoryTenMinCheckRepository;
    use chrono::Utc;
    use domain::{CheckStatus, TaskId, TenMinCheckId, UtcTimestamp};

    fn now() -> UtcTimestamp {
        UtcTimestamp::new(Utc::now())
    }

    async fn seed_no_response_check(
        repo: &InMemoryTenMinCheckRepository,
        interval_id: HourIntervalId,
    ) -> TenMinCheck {
        let draft = TenMinCheck::new(
            TenMinCheckId::new(0),
            interval_id,
            TaskId::new(1),
            now(),
            Some(now()),
            CheckStatus::NoResponse,
            None,
            None,
        );
        repo.create(draft).await.unwrap()
    }

    #[test]
    fn appends_reason_without_touching_ended_at_or_status() {
        pollster::block_on(async {
            let repo = Arc::new(InMemoryTenMinCheckRepository::new());
            let interval_id = HourIntervalId::new(1);
            let check = seed_no_response_check(&repo, interval_id).await;
            let use_case = SubmitFailureReason::new(repo.clone());

            let updated = use_case
                .execute(interval_id, "was in a meeting".to_string())
                .await
                .unwrap();

            assert_eq!(updated.id(), check.id());
            assert_eq!(updated.reason(), Some("was in a meeting"));
            assert_eq!(updated.ended_at(), check.ended_at());
            assert_eq!(updated.status(), CheckStatus::NoResponse);
        });
    }

    #[test]
    fn rejects_when_reason_already_set() {
        pollster::block_on(async {
            let repo = Arc::new(InMemoryTenMinCheckRepository::new());
            let interval_id = HourIntervalId::new(1);
            seed_no_response_check(&repo, interval_id).await;
            let use_case = SubmitFailureReason::new(repo.clone());

            use_case
                .execute(interval_id, "was in a meeting".to_string())
                .await
                .unwrap();
            let error = use_case
                .execute(interval_id, "another reason".to_string())
                .await
                .unwrap_err();

            assert_eq!(
                error,
                SubmitFailureReasonError::Domain(DomainError::ReasonAlreadySet)
            );
        });
    }

    #[test]
    fn rejects_when_status_is_not_no_response() {
        pollster::block_on(async {
            let repo = Arc::new(InMemoryTenMinCheckRepository::new());
            let interval_id = HourIntervalId::new(1);
            let draft = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval_id,
                TaskId::new(1),
                now(),
                Some(now()),
                CheckStatus::Failed,
                Some("distracted".to_string()),
                None,
            );
            repo.create(draft).await.unwrap();
            let use_case = SubmitFailureReason::new(repo.clone());

            let error = use_case
                .execute(interval_id, "another reason".to_string())
                .await
                .unwrap_err();

            assert_eq!(
                error,
                SubmitFailureReasonError::Domain(DomainError::ReasonNotAllowedForStatus)
            );
        });
    }

    #[test]
    fn rejects_when_last_check_of_interval_is_still_open() {
        pollster::block_on(async {
            let repo = Arc::new(InMemoryTenMinCheckRepository::new());
            let interval_id = HourIntervalId::new(1);
            // Открытая десятиминутка — например, пользователь уже
            // подтвердил "готов продолжить" и новый слот идёт полным ходом;
            // объяснять здесь нечего.
            let draft = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval_id,
                TaskId::new(1),
                now(),
                None,
                CheckStatus::Worked,
                None,
                None,
            );
            repo.create(draft).await.unwrap();
            let use_case = SubmitFailureReason::new(repo.clone());

            let error = use_case
                .execute(interval_id, "reason".to_string())
                .await
                .unwrap_err();

            assert_eq!(
                error,
                SubmitFailureReasonError::Domain(DomainError::ReasonNotAllowedForStatus)
            );
        });
    }

    #[test]
    fn errors_when_interval_has_no_checks_at_all() {
        pollster::block_on(async {
            let repo = Arc::new(InMemoryTenMinCheckRepository::new());
            let use_case = SubmitFailureReason::new(repo);

            let error = use_case
                .execute(HourIntervalId::new(999), "reason".to_string())
                .await
                .unwrap_err();

            assert_eq!(error, SubmitFailureReasonError::Repo(RepoError::NotFound));
        });
    }
}
