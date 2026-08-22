use std::sync::Arc;

use domain::{DomainError, HourInterval, HourIntervalId, HourIntervalState, SuccessfulCount};
use thiserror::Error;

use crate::error::{NotifierError, RepoError};
use crate::outbound_message::OutboundMessage;
use crate::ports::{Clock, HourIntervalRepository, Notifier, TenMinCheckRepository, WorkDayRepository};
use crate::use_cases::support::classify_checks;

/// Ошибка завершения часового интервала.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FinishHourIntervalError {
    #[error("hour interval is already closed")]
    AlreadyClosed,

    #[error(transparent)]
    Domain(#[from] DomainError),

    #[error(transparent)]
    Repo(#[from] RepoError),

    #[error(transparent)]
    Notifier(#[from] NotifierError),
}

/// Закрывает интервал (`ended_at = now`), считает `successful_count` по
/// фактически закрытым **рабочим** десятиминуткам (при обычном течении —
/// всегда 5), сохраняет обязательный `summary` и уведомляет пользователя
/// итогом. Вызывается после того, как рабочий этап интервала исчерпан и
/// отдых закрыт через `MarkReturnedFromRest`.
pub struct FinishHourInterval {
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    work_day_repository: Arc<dyn WorkDayRepository>,
    clock: Arc<dyn Clock>,
    notifier: Arc<dyn Notifier>,
}

impl FinishHourInterval {
    pub fn new(
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
        work_day_repository: Arc<dyn WorkDayRepository>,
        clock: Arc<dyn Clock>,
        notifier: Arc<dyn Notifier>,
    ) -> Self {
        Self {
            hour_interval_repository,
            ten_min_check_repository,
            work_day_repository,
            clock,
            notifier,
        }
    }

    pub async fn execute(
        &self,
        interval_id: HourIntervalId,
        summary: String,
    ) -> Result<HourInterval, FinishHourIntervalError> {
        let interval = self
            .hour_interval_repository
            .find_by_id(interval_id)
            .await?
            .ok_or(RepoError::NotFound)?;
        if interval.ended_at().is_some() {
            return Err(FinishHourIntervalError::AlreadyClosed);
        }

        let checks = self
            .ten_min_check_repository
            .list_by_interval(interval_id)
            .await?;
        let classified = classify_checks(checks);
        let successful_count =
            SuccessfulCount::new(HourIntervalState::successful_count(&classified.closed_work_statuses))?;

        let now = self.clock.now();
        let updated = HourInterval::new(
            interval.id(),
            interval.work_day_id(),
            interval.started_at(),
            Some(now),
            successful_count,
            Some(summary),
        );
        self.hour_interval_repository.update(updated.clone()).await?;

        let work_day = self
            .work_day_repository
            .find_by_id(interval.work_day_id())
            .await?
            .ok_or(RepoError::NotFound)?;
        let text = format!("Интервал завершён: {}/5", successful_count.value());
        self.notifier
            .send(work_day.user_id(), OutboundMessage::Text(text))
            .await?;

        Ok(updated)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{
        FakeClock, FakeNotifier, InMemoryHourIntervalRepository, InMemoryTenMinCheckRepository,
        InMemoryWorkDayRepository,
    };
    use chrono::Utc;
    use domain::{CheckStatus, TaskId, TelegramId, TenMinCheck, TenMinCheckId, UtcTimestamp, WorkDay, WorkDayId};

    struct Fixture {
        use_case: FinishHourInterval,
        hour_interval_repository: Arc<InMemoryHourIntervalRepository>,
        ten_min_check_repository: Arc<InMemoryTenMinCheckRepository>,
        notifier: Arc<FakeNotifier>,
        interval_id: HourIntervalId,
        user_id: TelegramId,
        now: UtcTimestamp,
    }

    async fn fixture() -> Fixture {
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let ten_min_check_repository = Arc::new(InMemoryTenMinCheckRepository::new());
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let notifier = Arc::new(FakeNotifier::new());
        let now = UtcTimestamp::new(Utc::now());
        let clock = Arc::new(FakeClock::new(now));

        let user_id = TelegramId::new(1).unwrap();
        let work_day = WorkDay::new(WorkDayId::new(0), user_id, now, None, None, None, None);
        let work_day = work_day_repository.create(work_day).await.unwrap();

        let interval = HourInterval::new(
            HourIntervalId::new(0),
            work_day.id(),
            now,
            None,
            SuccessfulCount::zero(),
            None,
        );
        let interval = hour_interval_repository.create(interval).await.unwrap();

        let use_case = FinishHourInterval::new(
            hour_interval_repository.clone(),
            ten_min_check_repository.clone(),
            work_day_repository,
            clock,
            notifier.clone(),
        );

        Fixture {
            use_case,
            hour_interval_repository,
            ten_min_check_repository,
            notifier,
            interval_id: interval.id(),
            user_id,
            now,
        }
    }

    async fn seed_worked(
        repo: &InMemoryTenMinCheckRepository,
        interval_id: HourIntervalId,
        count: i64,
        start: UtcTimestamp,
    ) {
        for i in 0..count {
            let started = UtcTimestamp::new(start.value() + chrono::Duration::minutes(i * 10));
            let ended = UtcTimestamp::new(started.value() + chrono::Duration::minutes(10));
            let draft = TenMinCheck::new(
                TenMinCheckId::new(0),
                interval_id,
                TaskId::new(1),
                started,
                Some(ended),
                CheckStatus::Worked,
                None,
                None,
            );
            repo.create(draft).await.unwrap();
        }
    }

    #[test]
    fn natural_completion_yields_five_successful_and_notifies() {
        pollster::block_on(async {
            let fixture = fixture().await;
            seed_worked(&fixture.ten_min_check_repository, fixture.interval_id, 5, fixture.now).await;
            let rest = TenMinCheck::new(
                TenMinCheckId::new(0),
                fixture.interval_id,
                TaskId::new(1),
                UtcTimestamp::new(fixture.now.value() + chrono::Duration::minutes(50)),
                Some(UtcTimestamp::new(
                    fixture.now.value() + chrono::Duration::minutes(60),
                )),
                CheckStatus::Worked,
                None,
                None,
            );
            fixture
                .ten_min_check_repository
                .create(rest)
                .await
                .unwrap();

            let finished = fixture
                .use_case
                .execute(fixture.interval_id, "wrote the report".to_string())
                .await
                .unwrap();

            assert_eq!(finished.successful_count().value(), 5);
            assert_eq!(finished.summary(), Some("wrote the report"));
            assert!(finished.ended_at().is_some());
            assert_eq!(
                fixture.notifier.sent_messages(),
                vec![(
                    fixture.user_id,
                    OutboundMessage::Text("Интервал завершён: 5/5".to_string())
                )]
            );

            let persisted = fixture
                .hour_interval_repository
                .find_by_id(fixture.interval_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(persisted, finished);
        });
    }

    #[test]
    fn forced_finish_before_five_successful_keeps_actual_count_and_no_rest() {
        pollster::block_on(async {
            let fixture = fixture().await;
            seed_worked(&fixture.ten_min_check_repository, fixture.interval_id, 2, fixture.now).await;

            let finished = fixture
                .use_case
                .execute(fixture.interval_id, String::new())
                .await
                .unwrap();

            assert_eq!(finished.successful_count().value(), 2);
        });
    }

    #[test]
    fn rejects_finishing_an_already_closed_interval() {
        pollster::block_on(async {
            let fixture = fixture().await;

            fixture
                .use_case
                .execute(fixture.interval_id, "done".to_string())
                .await
                .unwrap();
            let error = fixture
                .use_case
                .execute(fixture.interval_id, "done again".to_string())
                .await
                .unwrap_err();

            assert_eq!(error, FinishHourIntervalError::AlreadyClosed);
        });
    }
}
