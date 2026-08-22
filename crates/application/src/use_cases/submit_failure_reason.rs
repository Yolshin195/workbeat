use std::sync::Arc;

use domain::{append_reason_to_no_response, DomainError, TenMinCheck, TenMinCheckId};
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
        check_id: TenMinCheckId,
        reason: String,
    ) -> Result<TenMinCheck, SubmitFailureReasonError> {
        let check = self
            .ten_min_check_repository
            .find_by_id(check_id)
            .await?
            .ok_or(RepoError::NotFound)?;

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
    use domain::{CheckStatus, HourIntervalId, TaskId, UtcTimestamp};

    fn now() -> UtcTimestamp {
        UtcTimestamp::new(Utc::now())
    }

    async fn seed_no_response_check(repo: &InMemoryTenMinCheckRepository) -> TenMinCheck {
        let draft = TenMinCheck::new(
            TenMinCheckId::new(0),
            HourIntervalId::new(1),
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
            let check = seed_no_response_check(&repo).await;
            let use_case = SubmitFailureReason::new(repo.clone());

            let updated = use_case
                .execute(check.id(), "was in a meeting".to_string())
                .await
                .unwrap();

            assert_eq!(updated.reason(), Some("was in a meeting"));
            assert_eq!(updated.ended_at(), check.ended_at());
            assert_eq!(updated.status(), CheckStatus::NoResponse);
        });
    }

    #[test]
    fn rejects_when_reason_already_set() {
        pollster::block_on(async {
            let repo = Arc::new(InMemoryTenMinCheckRepository::new());
            let check = seed_no_response_check(&repo).await;
            let use_case = SubmitFailureReason::new(repo.clone());

            use_case
                .execute(check.id(), "was in a meeting".to_string())
                .await
                .unwrap();
            let error = use_case
                .execute(check.id(), "another reason".to_string())
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
            let draft = TenMinCheck::new(
                TenMinCheckId::new(0),
                HourIntervalId::new(1),
                TaskId::new(1),
                now(),
                Some(now()),
                CheckStatus::Failed,
                Some("distracted".to_string()),
                None,
            );
            let check = repo.create(draft).await.unwrap();
            let use_case = SubmitFailureReason::new(repo.clone());

            let error = use_case
                .execute(check.id(), "another reason".to_string())
                .await
                .unwrap_err();

            assert_eq!(
                error,
                SubmitFailureReasonError::Domain(DomainError::ReasonNotAllowedForStatus)
            );
        });
    }

    #[test]
    fn errors_when_check_not_found() {
        pollster::block_on(async {
            let repo = Arc::new(InMemoryTenMinCheckRepository::new());
            let use_case = SubmitFailureReason::new(repo);

            let error = use_case
                .execute(TenMinCheckId::new(999), "reason".to_string())
                .await
                .unwrap_err();

            assert_eq!(error, SubmitFailureReasonError::Repo(RepoError::NotFound));
        });
    }
}
