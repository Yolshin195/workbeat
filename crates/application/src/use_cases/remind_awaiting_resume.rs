use std::sync::Arc;

use domain::{ReminderScheduleDecision, TenMinCheck};
use thiserror::Error;

use crate::error::{NotifierError, RepoError};
use crate::outbound_message::OutboundMessage;
use crate::ports::{Clock, HourIntervalRepository, Notifier, TenMinCheckRepository, WorkDayRepository};
use crate::reminder_schedule_config::ReminderScheduleConfig;
use crate::use_cases::support::resolve_user_id;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum RemindAwaitingResumeError {
    #[error(transparent)]
    Repo(#[from] RepoError),

    #[error(transparent)]
    Notifier(#[from] NotifierError),
}

/// Вызывается периодическим poll-циклом (Задача 10) на каждом тике,
/// параллельно с `AdvanceOpenTenMinChecks`. Опрашивает закрытые
/// `Failed`/`NoResponse` десятиминутки, являющиеся последними в ещё не
/// закрытом интервале (`find_awaiting_resume`), и шлёт напоминания по
/// расписанию `ReminderScheduleDecision`, пока пользователь не вызовет
/// `ConfirmReadyToContinue`.
pub struct RemindAwaitingResume {
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    work_day_repository: Arc<dyn WorkDayRepository>,
    clock: Arc<dyn Clock>,
    notifier: Arc<dyn Notifier>,
    reminder_config: ReminderScheduleConfig,
}

impl RemindAwaitingResume {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
        hour_interval_repository: Arc<dyn HourIntervalRepository>,
        work_day_repository: Arc<dyn WorkDayRepository>,
        clock: Arc<dyn Clock>,
        notifier: Arc<dyn Notifier>,
        reminder_config: ReminderScheduleConfig,
    ) -> Self {
        Self {
            ten_min_check_repository,
            hour_interval_repository,
            work_day_repository,
            clock,
            notifier,
            reminder_config,
        }
    }

    pub async fn execute(&self) -> Result<(), RemindAwaitingResumeError> {
        let awaiting = self.ten_min_check_repository.find_awaiting_resume().await?;
        let now = self.clock.now();

        for check in awaiting {
            self.maybe_remind(&check, now).await?;
        }

        Ok(())
    }

    async fn maybe_remind(
        &self,
        check: &TenMinCheck,
        now: domain::UtcTimestamp,
    ) -> Result<(), RemindAwaitingResumeError> {
        let ended_at = check
            .ended_at()
            .expect("find_awaiting_resume returns only closed checks");
        let elapsed_since_wait_started = now.value() - ended_at.value();
        let reference = check.last_reminder_at().unwrap_or(ended_at);
        let elapsed_since_last_reminder = now.value() - reference.value();

        let should_remind = ReminderScheduleDecision::decide(
            elapsed_since_last_reminder,
            elapsed_since_wait_started,
            self.reminder_config.burst_window,
            self.reminder_config.burst_interval,
            self.reminder_config.steady_interval,
        );
        if !should_remind {
            return Ok(());
        }

        let user_id = resolve_user_id(
            &self.hour_interval_repository,
            &self.work_day_repository,
            check.hour_interval_id(),
        )
        .await?;
        let text = if check.reason().is_some() {
            "Готов продолжить?"
        } else {
            "Что случилось, почему не отвечаешь?"
        };
        self.notifier
            .send(user_id, OutboundMessage::Text(text.to_string()))
            .await?;

        let updated = TenMinCheck::new(
            check.id(),
            check.hour_interval_id(),
            check.task_id(),
            check.started_at(),
            check.ended_at(),
            check.status(),
            check.reason().map(str::to_string),
            Some(now),
        );
        self.ten_min_check_repository.update(updated).await?;

        Ok(())
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
    use domain::{
        CheckStatus, HourInterval, HourIntervalId, SuccessfulCount, TaskId, TelegramId,
        TenMinCheckId, UtcTimestamp, WorkDay, WorkDayId,
    };

    struct Fixture {
        use_case: RemindAwaitingResume,
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

        let use_case = RemindAwaitingResume::new(
            ten_min_check_repository.clone(),
            hour_interval_repository,
            work_day_repository,
            clock.clone(),
            notifier.clone(),
            ReminderScheduleConfig::default(),
        );

        Fixture {
            use_case,
            ten_min_check_repository,
            notifier,
            clock,
            interval_id: interval.id(),
            task_id: TaskId::new(1),
            user_id,
        }
    }

    #[test]
    fn reminds_no_response_without_reason_asking_what_happened() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let ended_at = fixture.clock.now();
            let draft = TenMinCheck::new(
                TenMinCheckId::new(0),
                fixture.interval_id,
                fixture.task_id,
                ended_at,
                Some(ended_at),
                CheckStatus::NoResponse,
                None,
                None,
            );
            fixture.ten_min_check_repository.create(draft).await.unwrap();

            fixture.clock.advance(chrono::Duration::minutes(1));
            fixture.use_case.execute().await.unwrap();

            assert_eq!(
                fixture.notifier.sent_messages(),
                vec![(
                    fixture.user_id,
                    OutboundMessage::Text("Что случилось, почему не отвечаешь?".to_string())
                )]
            );
        });
    }

    #[test]
    fn reminds_failed_with_reason_asking_ready_to_continue() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let ended_at = fixture.clock.now();
            let draft = TenMinCheck::new(
                TenMinCheckId::new(0),
                fixture.interval_id,
                fixture.task_id,
                ended_at,
                Some(ended_at),
                CheckStatus::Failed,
                Some("distracted".to_string()),
                None,
            );
            fixture.ten_min_check_repository.create(draft).await.unwrap();

            fixture.clock.advance(chrono::Duration::minutes(1));
            fixture.use_case.execute().await.unwrap();

            assert_eq!(
                fixture.notifier.sent_messages(),
                vec![(fixture.user_id, OutboundMessage::Text("Готов продолжить?".to_string()))]
            );
        });
    }

    #[test]
    fn stops_reminding_once_next_slot_started() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let ended_at = fixture.clock.now();
            let draft = TenMinCheck::new(
                TenMinCheckId::new(0),
                fixture.interval_id,
                fixture.task_id,
                ended_at,
                Some(ended_at),
                CheckStatus::Failed,
                Some("distracted".to_string()),
                None,
            );
            fixture.ten_min_check_repository.create(draft).await.unwrap();

            fixture.clock.advance(chrono::Duration::minutes(1));
            fixture.use_case.execute().await.unwrap();
            assert_eq!(fixture.notifier.sent_messages().len(), 1);

            // Пользователь подтвердил готовность продолжить — стартует
            // новая десятиминутка на этот же слот.
            let next = TenMinCheck::new(
                TenMinCheckId::new(0),
                fixture.interval_id,
                fixture.task_id,
                fixture.clock.now(),
                None,
                CheckStatus::Worked,
                None,
                None,
            );
            fixture.ten_min_check_repository.create(next).await.unwrap();

            fixture.clock.advance(chrono::Duration::minutes(5));
            fixture.use_case.execute().await.unwrap();

            // Никаких новых напоминаний по уже закрытой строке.
            assert_eq!(fixture.notifier.sent_messages().len(), 1);
        });
    }
}
