use std::sync::Arc;

use domain::{LunchSuggestionDecision, WorkDayId};
use thiserror::Error;

use crate::error::{NotifierError, RepoError};
use crate::outbound_message::OutboundMessage;
use crate::ports::{HourIntervalRepository, Notifier, WorkDayRepository};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SuggestLunchAfterNthIntervalError {
    #[error(transparent)]
    Repo(#[from] RepoError),

    #[error(transparent)]
    Notifier(#[from] NotifierError),
}

/// Вызывается после того, как очередной часовой интервал закрыт
/// (`FinishHourInterval`, Задача 7) — не планировщиком. Считает закрытые
/// интервалы дня и, если их ровно `LUNCH_SUGGESTION_AFTER_INTERVALS` (см.
/// `LunchSuggestionDecision`), сам предлагает пойти на обед (README.md
/// раздел 1: "Также после 4-го интервала бот сам предлагает пойти на обед").
/// Предложение можно отклонить — это не блокирует старт следующего интервала.
pub struct SuggestLunchAfterNthInterval {
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    work_day_repository: Arc<dyn WorkDayRepository>,
    notifier: Arc<dyn Notifier>,
}

impl SuggestLunchAfterNthInterval {
    pub fn new(
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        work_day_repository: Arc<dyn WorkDayRepository>,
        notifier: Arc<dyn Notifier>,
    ) -> Self {
        Self {
            hour_interval_repository,
            work_day_repository,
            notifier,
        }
    }

    pub async fn execute(
        &self,
        work_day_id: WorkDayId,
    ) -> Result<(), SuggestLunchAfterNthIntervalError> {
        let intervals = self
            .hour_interval_repository
            .list_by_work_day(work_day_id)
            .await?;
        let closed_intervals = intervals
            .iter()
            .filter(|interval| interval.ended_at().is_some())
            .count() as u8;

        if !LunchSuggestionDecision::decide(closed_intervals) {
            return Ok(());
        }

        let work_day = self
            .work_day_repository
            .find_by_id(work_day_id)
            .await?
            .ok_or(RepoError::NotFound)?;
        self.notifier
            .send(
                work_day.user_id(),
                OutboundMessage::Text("4 интервала позади, не пора ли пообедать?".to_string()),
            )
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeNotifier, InMemoryHourIntervalRepository, InMemoryWorkDayRepository};
    use chrono::Utc;
    use domain::{HourInterval, HourIntervalId, SuccessfulCount, TelegramId, UtcTimestamp, WorkDay};

    struct Fixture {
        use_case: SuggestLunchAfterNthInterval,
        hour_interval_repository: Arc<InMemoryHourIntervalRepository>,
        notifier: Arc<FakeNotifier>,
        work_day_id: WorkDayId,
        user_id: TelegramId,
    }

    async fn fixture() -> Fixture {
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let notifier = Arc::new(FakeNotifier::new());
        let now = UtcTimestamp::new(Utc::now());

        let user_id = TelegramId::new(1).unwrap();
        let work_day = WorkDay::new(WorkDayId::new(0), user_id, now, None, None, None, None);
        let work_day = work_day_repository.create(work_day).await.unwrap();

        let use_case = SuggestLunchAfterNthInterval::new(
            hour_interval_repository.clone(),
            work_day_repository,
            notifier.clone(),
        );

        Fixture {
            use_case,
            hour_interval_repository,
            notifier,
            work_day_id: work_day.id(),
            user_id,
        }
    }

    async fn seed_closed_intervals(fixture: &Fixture, count: usize) {
        let now = UtcTimestamp::new(Utc::now());
        for _ in 0..count {
            let interval = HourInterval::new(
                HourIntervalId::new(0),
                fixture.work_day_id,
                now,
                Some(now),
                SuccessfulCount::new(5).unwrap(),
                Some("done".to_string()),
            );
            fixture.hour_interval_repository.create(interval).await.unwrap();
        }
    }

    #[test]
    fn does_not_suggest_before_the_fourth_closed_interval() {
        pollster::block_on(async {
            let fixture = fixture().await;
            seed_closed_intervals(&fixture, 3).await;

            fixture.use_case.execute(fixture.work_day_id).await.unwrap();

            assert!(fixture.notifier.sent_messages().is_empty());
        });
    }

    #[test]
    fn suggests_lunch_exactly_on_the_fourth_closed_interval() {
        pollster::block_on(async {
            let fixture = fixture().await;
            seed_closed_intervals(&fixture, 4).await;

            fixture.use_case.execute(fixture.work_day_id).await.unwrap();

            assert_eq!(fixture.notifier.sent_messages().len(), 1);
            assert_eq!(fixture.notifier.sent_messages()[0].0, fixture.user_id);
        });
    }

    #[test]
    fn does_not_suggest_again_after_the_fourth() {
        pollster::block_on(async {
            let fixture = fixture().await;
            seed_closed_intervals(&fixture, 5).await;

            fixture.use_case.execute(fixture.work_day_id).await.unwrap();

            assert!(fixture.notifier.sent_messages().is_empty());
        });
    }
}
