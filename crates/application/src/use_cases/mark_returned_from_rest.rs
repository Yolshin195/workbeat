use std::sync::Arc;

use domain::{HourIntervalId, TenMinCheck};
use thiserror::Error;

use crate::error::RepoError;
use crate::ports::{Clock, TenMinCheckRepository};
use crate::use_cases::support::classify_checks;

/// Ошибка "Вернулся".
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum MarkReturnedFromRestError {
    #[error("no open rest ten-min check for this hour interval")]
    NoOpenRestCheck,

    #[error(transparent)]
    Repo(#[from] RepoError),
}

/// Явное действие пользователя ("Вернулся"): закрывает открытую десятиминутку
/// **отдыха** — `ended_at = now`, независимо от того, уложился пользователь в
/// номинальные 10 минут (README.md раздел 2, п.4). Переработка фиксируется как
/// есть, без статуса "провалено" и без `reason`.
pub struct MarkReturnedFromRest {
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    clock: Arc<dyn Clock>,
}

impl MarkReturnedFromRest {
    pub fn new(ten_min_check_repository: Arc<dyn TenMinCheckRepository>, clock: Arc<dyn Clock>) -> Self {
        Self {
            ten_min_check_repository,
            clock,
        }
    }

    pub async fn execute(
        &self,
        interval_id: HourIntervalId,
    ) -> Result<TenMinCheck, MarkReturnedFromRestError> {
        let open_check = self
            .ten_min_check_repository
            .find_open_by_interval(interval_id)
            .await?
            .ok_or(MarkReturnedFromRestError::NoOpenRestCheck)?;

        let siblings = self
            .ten_min_check_repository
            .list_by_interval(interval_id)
            .await?;
        let classified = classify_checks(siblings);
        let is_rest = classified
            .rest_check
            .as_ref()
            .is_some_and(|rest| rest.id() == open_check.id());
        if !is_rest {
            return Err(MarkReturnedFromRestError::NoOpenRestCheck);
        }

        let updated = TenMinCheck::new(
            open_check.id(),
            open_check.hour_interval_id(),
            open_check.task_id(),
            open_check.started_at(),
            Some(self.clock.now()),
            open_check.status(),
            None,
            None,
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
    use crate::testing::{FakeClock, InMemoryTenMinCheckRepository};
    use chrono::Utc;
    use domain::{CheckStatus, HourIntervalId, TaskId, TenMinCheckId, UtcTimestamp};

    fn now() -> UtcTimestamp {
        UtcTimestamp::new(Utc::now())
    }

    async fn seed_five_worked_and_rest(
        repo: &InMemoryTenMinCheckRepository,
        interval_id: HourIntervalId,
        task_id: TaskId,
        start: UtcTimestamp,
    ) -> TenMinCheck {
        for i in 0..5 {
            let started = UtcTimestamp::new(start.value() + chrono::Duration::minutes(i * 10));
            let ended = UtcTimestamp::new(started.value() + chrono::Duration::minutes(10));
            let draft = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval_id,
                task_id,
                started,
                Some(ended),
                CheckStatus::Worked,
                None,
                None,
            );
            repo.create(draft).await.unwrap();
        }

        let rest_started = UtcTimestamp::new(start.value() + chrono::Duration::minutes(50));
        let rest_draft = TenMinCheck::new(
            TenMinCheckId::new(0),
            interval_id,
            task_id,
            rest_started,
            None,
            CheckStatus::Worked,
            None,
            None,
        );
        repo.create(rest_draft).await.unwrap()
    }

    #[test]
    fn closes_rest_check_with_actual_duration_even_when_overworked() {
        pollster::block_on(async {
            let repo = Arc::new(InMemoryTenMinCheckRepository::new());
            let interval_id = HourIntervalId::new(1);
            let task_id = TaskId::new(1);
            let start = now();
            let rest = seed_five_worked_and_rest(&repo, interval_id, task_id, start).await;

            let return_time = UtcTimestamp::new(rest.started_at().value() + chrono::Duration::minutes(30));
            let clock = Arc::new(FakeClock::new(return_time));
            let use_case = MarkReturnedFromRest::new(repo.clone(), clock);

            let closed = use_case.execute(interval_id).await.unwrap();

            assert_eq!(closed.ended_at(), Some(return_time));
            assert_eq!(closed.reason(), None);
            assert_eq!(closed.status(), CheckStatus::Worked);
            assert!(repo.find_open_by_interval(interval_id).await.unwrap().is_none());
        });
    }

    #[test]
    fn rejects_when_open_check_is_a_work_slot_not_rest() {
        pollster::block_on(async {
            let repo = Arc::new(InMemoryTenMinCheckRepository::new());
            let interval_id = HourIntervalId::new(1);
            let task_id = TaskId::new(1);
            let draft = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval_id,
                task_id,
                now(),
                None,
                CheckStatus::Worked,
                None,
                None,
            );
            repo.create(draft).await.unwrap();

            let clock = Arc::new(FakeClock::new(now()));
            let use_case = MarkReturnedFromRest::new(repo, clock);

            let error = use_case.execute(interval_id).await.unwrap_err();
            assert_eq!(error, MarkReturnedFromRestError::NoOpenRestCheck);
        });
    }

    #[test]
    fn rejects_when_no_open_check_at_all() {
        pollster::block_on(async {
            let repo = Arc::new(InMemoryTenMinCheckRepository::new());
            let clock = Arc::new(FakeClock::new(now()));
            let use_case = MarkReturnedFromRest::new(repo, clock);

            let error = use_case.execute(HourIntervalId::new(1)).await.unwrap_err();
            assert_eq!(error, MarkReturnedFromRestError::NoOpenRestCheck);
        });
    }
}
