use std::sync::Arc;

use domain::{IdlePromptDecision, UtcTimestamp, WorkDay};
use thiserror::Error;

use crate::error::{NotifierError, RepoError};
use crate::idle_prompt_config::IdlePromptConfig;
use crate::outbound_message::OutboundMessage;
use crate::ports::{Clock, HourIntervalRepository, Notifier, WorkDayRepository};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PromptContinueIfIdleError {
    #[error(transparent)]
    Repo(#[from] RepoError),

    #[error(transparent)]
    Notifier(#[from] NotifierError),
}

/// Вызывается периодическим poll-циклом (Задача 10) на каждом тике — не
/// планировщиком, аналогично `AdvanceOpenTenMinChecks`/`RemindAwaitingResume`
/// из Задачи 7. Опрашивает открытые дни без активного часового интервала
/// (`find_open_without_active_interval`) и, если простой длится дольше `N`
/// часов с последнего напоминания (или с момента начала простоя, если ещё не
/// напоминали), спрашивает "Готовы продолжить работу?" (README.md раздел 1).
/// Единственный механизм "отмены" — как только пользователь стартует
/// следующий интервал или завершает день, `work_day` перестаёт попадать в
/// `find_open_without_active_interval()` на следующем тике.
pub struct PromptContinueIfIdle {
    work_day_repository: Arc<dyn WorkDayRepository>,
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    clock: Arc<dyn Clock>,
    notifier: Arc<dyn Notifier>,
    idle_config: IdlePromptConfig,
}

impl PromptContinueIfIdle {
    pub fn new(
        work_day_repository: Arc<dyn WorkDayRepository>,
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        clock: Arc<dyn Clock>,
        notifier: Arc<dyn Notifier>,
        idle_config: IdlePromptConfig,
    ) -> Self {
        Self {
            work_day_repository,
            hour_interval_repository,
            clock,
            notifier,
            idle_config,
        }
    }

    pub async fn execute(&self) -> Result<(), PromptContinueIfIdleError> {
        let idle_work_days = self
            .work_day_repository
            .find_open_without_active_interval()
            .await?;
        let now = self.clock.now();

        for work_day in idle_work_days {
            self.maybe_prompt(&work_day, now).await?;
        }

        Ok(())
    }

    async fn maybe_prompt(
        &self,
        work_day: &WorkDay,
        now: UtcTimestamp,
    ) -> Result<(), PromptContinueIfIdleError> {
        let idle_started_at = self.idle_started_at(work_day).await?;
        let reference = work_day.last_idle_prompt_at().unwrap_or(idle_started_at);
        let elapsed_since_last_prompt = now.value() - reference.value();

        if !IdlePromptDecision::decide(elapsed_since_last_prompt, self.idle_config.idle_threshold)
        {
            return Ok(());
        }

        self.notifier
            .send(
                work_day.user_id(),
                OutboundMessage::Text("Готовы продолжить работу?".to_string()),
            )
            .await?;

        let updated = WorkDay::new(
            work_day.id(),
            work_day.user_id(),
            work_day.started_at(),
            work_day.finished_at(),
            work_day.lunch_started_at(),
            work_day.lunch_ended_at(),
            Some(now),
        );
        self.work_day_repository.update(updated).await?;

        Ok(())
    }

    /// Момент начала текущего простоя: позже из старта дня, конца обеда и
    /// конца последнего закрытого часового интервала — какое бы событие ни
    /// случилось последним (README.md раздел 1/6).
    async fn idle_started_at(&self, work_day: &WorkDay) -> Result<UtcTimestamp, RepoError> {
        let mut reference = work_day.started_at();
        if let Some(lunch_ended_at) = work_day.lunch_ended_at() {
            reference = reference.max(lunch_ended_at);
        }

        let intervals = self
            .hour_interval_repository
            .list_by_work_day(work_day.id())
            .await?;
        if let Some(last_ended_at) = intervals.into_iter().filter_map(|i| i.ended_at()).max() {
            reference = reference.max(last_ended_at);
        }

        Ok(reference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{FakeClock, FakeNotifier, InMemoryHourIntervalRepository, InMemoryWorkDayRepository};
    use chrono::{Duration, Utc};
    use domain::{TelegramId, WorkDayId};

    struct Fixture {
        use_case: PromptContinueIfIdle,
        work_day_repository: Arc<InMemoryWorkDayRepository>,
        notifier: Arc<FakeNotifier>,
        clock: Arc<FakeClock>,
        work_day_id: WorkDayId,
        user_id: TelegramId,
    }

    async fn fixture_with_threshold(idle_threshold: Duration) -> Fixture {
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let notifier = Arc::new(FakeNotifier::new());
        let now = UtcTimestamp::new(Utc::now());
        let clock = Arc::new(FakeClock::new(now));

        let user_id = TelegramId::new(1).unwrap();
        let work_day = WorkDay::new(WorkDayId::new(0), user_id, now, None, None, None, None);
        let work_day = work_day_repository.create(work_day).await.unwrap();

        let use_case = PromptContinueIfIdle::new(
            work_day_repository.clone(),
            hour_interval_repository,
            clock.clone(),
            notifier.clone(),
            IdlePromptConfig { idle_threshold },
        );

        Fixture {
            use_case,
            work_day_repository,
            notifier,
            clock,
            work_day_id: work_day.id(),
            user_id,
        }
    }

    #[test]
    fn below_threshold_does_not_notify_or_update_last_prompt() {
        pollster::block_on(async {
            let fixture = fixture_with_threshold(Duration::hours(2)).await;
            fixture.clock.advance(Duration::hours(1) + Duration::minutes(59));

            fixture.use_case.execute().await.unwrap();

            assert!(fixture.notifier.sent_messages().is_empty());
            let work_day = fixture
                .work_day_repository
                .find_by_id(fixture.work_day_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(work_day.last_idle_prompt_at(), None);
        });
    }

    #[test]
    fn at_or_above_threshold_notifies_once_and_updates_last_prompt() {
        pollster::block_on(async {
            let fixture = fixture_with_threshold(Duration::hours(2)).await;
            fixture.clock.advance(Duration::hours(2));

            fixture.use_case.execute().await.unwrap();

            assert_eq!(
                fixture.notifier.sent_messages(),
                vec![(
                    fixture.user_id,
                    OutboundMessage::Text("Готовы продолжить работу?".to_string())
                )]
            );
            let work_day = fixture
                .work_day_repository
                .find_by_id(fixture.work_day_id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(work_day.last_idle_prompt_at(), Some(fixture.clock.now()));

            // Следующий вызов сразу после этого снова false, пока не пройдёт
            // ещё N часов с момента последнего напоминания.
            fixture.clock.advance(Duration::minutes(1));
            fixture.use_case.execute().await.unwrap();
            assert_eq!(fixture.notifier.sent_messages().len(), 1);

            fixture.clock.advance(Duration::hours(2));
            fixture.use_case.execute().await.unwrap();
            assert_eq!(fixture.notifier.sent_messages().len(), 2);
        });
    }

    #[test]
    fn work_day_with_active_interval_is_never_selected() {
        pollster::block_on(async {
            let fixture = fixture_with_threshold(Duration::hours(2)).await;
            fixture
                .work_day_repository
                .set_has_active_interval(fixture.work_day_id, true);
            fixture.clock.advance(Duration::hours(10));

            fixture.use_case.execute().await.unwrap();

            assert!(fixture.notifier.sent_messages().is_empty());
        });
    }
}
