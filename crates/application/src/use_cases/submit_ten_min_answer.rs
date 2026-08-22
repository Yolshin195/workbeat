use std::sync::Arc;

use domain::{
    CheckStatus, DomainError, HourIntervalId, HourIntervalState, TenMinCheck, TenMinCheckAnswer,
    TenMinCheckId, TenMinCheckState,
};
use thiserror::Error;

use crate::error::{NotifierError, RepoError};
use crate::outbound_message::OutboundMessage;
use crate::ports::{Clock, HourIntervalRepository, Notifier, TaskRepository, TenMinCheckRepository, WorkDayRepository};
use crate::use_cases::support::{find_in_progress_task, resolve_user_id};

/// Ошибка ответа на "Ты работаешь?".
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SubmitTenMinAnswerError {
    /// Нет открытой десятиминутки этого интервала — отвечать нечему.
    #[error("no open ten-min check for this hour interval")]
    NoOpenCheck,

    /// `reason` обязателен для ответа `No` (README.md раздел 2, п.2).
    #[error("reason is required when the answer is No")]
    ReasonRequired,

    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error(transparent)]
    Repo(#[from] RepoError),

    #[error(transparent)]
    Notifier(#[from] NotifierError),
}

/// Закрывает открытую десятиминутку интервала явным ответом пользователя и,
/// если ответ `Yes`, сразу создаёt следующий слот (рабочий или отдых — в
/// зависимости от того, была ли это 5-я успешная). Ответ `No` лишь закрывает
/// слот как `Failed` — новый слот стартует только через
/// `ConfirmReadyToContinue`.
pub struct SubmitTenMinAnswer {
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    work_day_repository: Arc<dyn WorkDayRepository>,
    task_repository: Arc<dyn TaskRepository>,
    clock: Arc<dyn Clock>,
    notifier: Arc<dyn Notifier>,
}

impl SubmitTenMinAnswer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        work_day_repository: Arc<dyn WorkDayRepository>,
        task_repository: Arc<dyn TaskRepository>,
        clock: Arc<dyn Clock>,
        notifier: Arc<dyn Notifier>,
    ) -> Self {
        Self {
            ten_min_check_repository,
            hour_interval_repository,
            work_day_repository,
            task_repository,
            clock,
            notifier,
        }
    }

    pub async fn execute(
        &self,
        interval_id: HourIntervalId,
        answer: TenMinCheckAnswer,
        reason: Option<String>,
    ) -> Result<TenMinCheck, SubmitTenMinAnswerError> {
        if answer == TenMinCheckAnswer::No && reason.is_none() {
            return Err(SubmitTenMinAnswerError::ReasonRequired);
        }

        let open_check = self
            .ten_min_check_repository
            .find_open_by_interval(interval_id)
            .await?
            .ok_or(SubmitTenMinAnswerError::NoOpenCheck)?;

        let closed_state = TenMinCheckState::Pending.submit_answer(answer)?;
        let status = match closed_state {
            TenMinCheckState::Closed(status) => status,
            TenMinCheckState::Pending => unreachable!("submit_answer always closes"),
        };

        let now = self.clock.now();
        let persisted_reason = if answer == TenMinCheckAnswer::No {
            reason
        } else {
            None
        };

        let closed_check = TenMinCheck::new(
            open_check.id(),
            open_check.hour_interval_id(),
            open_check.task_id(),
            open_check.started_at(),
            Some(now),
            status,
            persisted_reason,
            None,
        );
        self.ten_min_check_repository
            .update(closed_check.clone())
            .await?;

        if status == CheckStatus::Worked {
            let closed_statuses: Vec<CheckStatus> = self
                .ten_min_check_repository
                .list_by_interval(interval_id)
                .await?
                .into_iter()
                .filter(|check| check.ended_at().is_some())
                .map(|check| check.status())
                .collect();

            let user_id =
                resolve_user_id(&self.hour_interval_repository, &self.work_day_repository, interval_id)
                    .await?;

            if HourIntervalState::is_work_stage_closed(&closed_statuses) {
                let rest_draft = TenMinCheck::new(
                    TenMinCheckId::new(0),
                    interval_id,
                    closed_check.task_id(),
                    now,
                    None,
                    CheckStatus::Worked,
                    None,
                    None,
                );
                self.ten_min_check_repository.create(rest_draft).await?;
                self.notifier
                    .send(
                        user_id,
                        OutboundMessage::Text("Ты молодец, отдыхай 10 минут".to_string()),
                    )
                    .await?;
            } else {
                let active_task = find_in_progress_task(&self.task_repository, user_id).await?;
                let next_draft = TenMinCheck::new(
                    TenMinCheckId::new(0),
                    interval_id,
                    active_task.id(),
                    now,
                    None,
                    CheckStatus::Worked,
                    None,
                    None,
                );
                self.ten_min_check_repository.create(next_draft).await?;
            }
        }

        Ok(closed_check)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        FakeClock, FakeNotifier, InMemoryHourIntervalRepository, InMemoryTaskRepository,
        InMemoryTenMinCheckRepository, InMemoryWorkDayRepository,
    };
    use chrono::Utc;
    use domain::{HourInterval, SuccessfulCount, Task, TaskId, TaskStatus, TelegramId, UtcTimestamp, WorkDay, WorkDayId};

    struct Fixture {
        use_case: SubmitTenMinAnswer,
        ten_min_check_repository: Arc<InMemoryTenMinCheckRepository>,
        notifier: Arc<FakeNotifier>,
        clock: Arc<FakeClock>,
        interval_id: HourIntervalId,
        task_id: TaskId,
        user_id: TelegramId,
    }

    async fn fixture() -> Fixture {
        let ten_min_check_repository = Arc::new(InMemoryTenMinCheckRepository::new());
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let task_repository = Arc::new(InMemoryTaskRepository::new());
        let notifier = Arc::new(FakeNotifier::new());
        let now = UtcTimestamp::new(Utc::now());
        let clock = Arc::new(FakeClock::new(now));

        let user_id = TelegramId::new(1).unwrap();
        let work_day = WorkDay::new(WorkDayId::new(0), user_id, now, None, None, None, None);
        let work_day = work_day_repository.create(work_day).await.unwrap();

        let task = Task::new(
            TaskId::new(0),
            user_id,
            "Write report",
            TaskStatus::InProgress,
            None,
            None,
            now,
        )
        .unwrap();
        let task = task_repository.create(task).await.unwrap();

        let interval = HourInterval::new(
            HourIntervalId::new(0),
            work_day.id(),
            now,
            None,
            SuccessfulCount::zero(),
            None,
        );
        let interval = hour_interval_repository.create(interval).await.unwrap();

        let first_check = TenMinCheck::new(
            TenMinCheckId::new(0),
            interval.id(),
            task.id(),
            now,
            None,
            CheckStatus::Worked,
            None,
            None,
        );
        ten_min_check_repository.create(first_check).await.unwrap();

        let use_case = SubmitTenMinAnswer::new(
            ten_min_check_repository.clone(),
            hour_interval_repository.clone(),
            work_day_repository.clone(),
            task_repository.clone(),
            clock.clone(),
            notifier.clone(),
        );

        Fixture {
            use_case,
            ten_min_check_repository,
            notifier,
            clock,
            interval_id: interval.id(),
            task_id: task.id(),
            user_id,
        }
    }

    #[test]
    fn yes_not_fifth_closes_worked_and_starts_next_work_slot() {
        pollster::block_on(async {
            let fixture = fixture().await;

            let closed = fixture
                .use_case
                .execute(fixture.interval_id, TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();

            assert_eq!(closed.status(), CheckStatus::Worked);
            assert!(closed.ended_at().is_some());

            let checks = fixture.ten_min_check_repository.snapshot();
            assert_eq!(checks.len(), 2);
            let open = checks.iter().find(|c| c.ended_at().is_none()).unwrap();
            assert_eq!(open.task_id(), fixture.task_id);

            assert!(fixture.notifier.sent_messages().is_empty());
        });
    }

    #[test]
    fn fifth_successful_creates_rest_and_notifies() {
        pollster::block_on(async {
            let fixture = fixture().await;

            for _ in 0..4 {
                fixture
                    .use_case
                    .execute(fixture.interval_id, TenMinCheckAnswer::Yes, None)
                    .await
                    .unwrap();
            }

            let closed = fixture
                .use_case
                .execute(fixture.interval_id, TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();
            assert_eq!(closed.status(), CheckStatus::Worked);

            let checks = fixture.ten_min_check_repository.snapshot();
            assert_eq!(checks.len(), 6);
            let closed_worked = checks
                .iter()
                .filter(|c| c.ended_at().is_some() && c.status() == CheckStatus::Worked)
                .count();
            assert_eq!(closed_worked, 5);
            let rest = checks.iter().find(|c| c.ended_at().is_none()).unwrap();
            assert_eq!(rest.task_id(), fixture.task_id);

            assert_eq!(
                fixture.notifier.sent_messages(),
                vec![(
                    fixture.user_id,
                    OutboundMessage::Text("Ты молодец, отдыхай 10 минут".to_string())
                )]
            );
        });
    }

    #[test]
    fn no_closes_failed_and_does_not_start_next_slot() {
        pollster::block_on(async {
            let fixture = fixture().await;

            let closed = fixture
                .use_case
                .execute(
                    fixture.interval_id,
                    TenMinCheckAnswer::No,
                    Some("got distracted".to_string()),
                )
                .await
                .unwrap();

            assert_eq!(closed.status(), CheckStatus::Failed);
            assert_eq!(closed.reason(), Some("got distracted"));

            let checks = fixture.ten_min_check_repository.snapshot();
            assert_eq!(checks.len(), 1);
            assert!(checks[0].ended_at().is_some());
            assert!(fixture.notifier.sent_messages().is_empty());
        });
    }

    #[test]
    fn no_without_reason_is_rejected() {
        pollster::block_on(async {
            let fixture = fixture().await;

            let error = fixture
                .use_case
                .execute(fixture.interval_id, TenMinCheckAnswer::No, None)
                .await
                .unwrap_err();

            assert_eq!(error, SubmitTenMinAnswerError::ReasonRequired);
            assert!(fixture
                .ten_min_check_repository
                .find_open_by_interval(fixture.interval_id)
                .await
                .unwrap()
                .unwrap()
                .ended_at()
                .is_none());
        });
    }

    #[test]
    fn failures_interleaved_with_successes_still_close_work_stage_at_fifth_worked() {
        pollster::block_on(async {
            let fixture = fixture().await;

            // Worked, Failed(+confirm), Worked, Worked, NoResponse-подобный Failed(+confirm), Worked, Worked
            fixture
                .use_case
                .execute(fixture.interval_id, TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();

            fixture
                .use_case
                .execute(
                    fixture.interval_id,
                    TenMinCheckAnswer::No,
                    Some("distracted".to_string()),
                )
                .await
                .unwrap();

            // Симулируем ConfirmReadyToContinue вручную новой десятиминуткой.
            fixture.clock.advance(chrono::Duration::minutes(1));
            let retry = TenMinCheck::new(
                TenMinCheckId::new(0),
                fixture.interval_id,
                fixture.task_id,
                fixture.clock.now(),
                None,
                CheckStatus::Worked,
                None,
                None,
            );
            fixture
                .ten_min_check_repository
                .create(retry)
                .await
                .unwrap();

            fixture
                .use_case
                .execute(fixture.interval_id, TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();
            fixture
                .use_case
                .execute(fixture.interval_id, TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();

            fixture
                .use_case
                .execute(
                    fixture.interval_id,
                    TenMinCheckAnswer::No,
                    Some("distracted again".to_string()),
                )
                .await
                .unwrap();

            fixture.clock.advance(chrono::Duration::minutes(1));
            let retry2 = TenMinCheck::new(
                TenMinCheckId::new(0),
                fixture.interval_id,
                fixture.task_id,
                fixture.clock.now(),
                None,
                CheckStatus::Worked,
                None,
                None,
            );
            fixture
                .ten_min_check_repository
                .create(retry2)
                .await
                .unwrap();

            fixture
                .use_case
                .execute(fixture.interval_id, TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();
            fixture
                .use_case
                .execute(fixture.interval_id, TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();

            let checks = fixture.ten_min_check_repository.snapshot();
            let worked = checks
                .iter()
                .filter(|c| c.status() == CheckStatus::Worked && c.ended_at().is_some())
                .count();
            let failed = checks
                .iter()
                .filter(|c| c.status() == CheckStatus::Failed)
                .count();
            assert_eq!(worked, 5);
            assert_eq!(failed, 2);
            assert_eq!(checks.len(), 8); // 5 worked + 2 failed + 1 rest (open)
            assert!(checks.iter().any(|c| c.ended_at().is_none()));
        });
    }
}
