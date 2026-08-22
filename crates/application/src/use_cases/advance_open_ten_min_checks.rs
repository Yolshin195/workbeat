use std::sync::Arc;

use chrono::Duration;
use domain::{
    CheckStatus, ReminderScheduleDecision, TenMinCheck, WorkSlotPollDecision, WorkSlotPollOutcome,
    TEN_MIN_NOMINAL_MINUTES,
};
use thiserror::Error;

use crate::error::{NotifierError, RepoError};
use crate::outbound_message::OutboundMessage;
use crate::ports::{Clock, HourIntervalRepository, Notifier, TenMinCheckRepository, WorkDayRepository};
use crate::reminder_schedule_config::ReminderScheduleConfig;
use crate::use_cases::support::{classify_checks, resolve_user_id};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AdvanceOpenTenMinChecksError {
    #[error(transparent)]
    Repo(#[from] RepoError),

    #[error(transparent)]
    Notifier(#[from] NotifierError),
}

/// Вызывается периодическим poll-циклом (Задача 10) на каждом тике — не
/// планировщиком. Опрашивает все открытые `TenMinCheck` и продвигает их
/// состояние по чистым функциям `WorkSlotPollDecision`/`ReminderScheduleDecision`
/// из Задачи 2. Единственный механизм "отмены" — закрытая строка просто
/// перестаёт попадать в `find_open()` на следующем тике.
pub struct AdvanceOpenTenMinChecks {
    ten_min_check_repository: Arc<dyn TenMinCheckRepository>,
    hour_interval_repository: Arc<dyn HourIntervalRepository>,
    work_day_repository: Arc<dyn WorkDayRepository>,
    clock: Arc<dyn Clock>,
    notifier: Arc<dyn Notifier>,
    reminder_config: ReminderScheduleConfig,
}

impl AdvanceOpenTenMinChecks {
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

    pub async fn execute(&self) -> Result<(), AdvanceOpenTenMinChecksError> {
        let open_checks = self.ten_min_check_repository.find_open().await?;
        let now = self.clock.now();

        for check in open_checks {
            let siblings = self
                .ten_min_check_repository
                .list_by_interval(check.hour_interval_id())
                .await?;
            let classified = classify_checks(siblings);
            let is_rest = classified
                .rest_check
                .as_ref()
                .is_some_and(|rest| rest.id() == check.id());
            let elapsed_since_start = now.value() - check.started_at().value();

            if is_rest {
                self.advance_rest_check(&check, now, elapsed_since_start)
                    .await?;
            } else {
                self.advance_work_check(&check, now, elapsed_since_start)
                    .await?;
            }
        }

        Ok(())
    }

    async fn advance_work_check(
        &self,
        check: &TenMinCheck,
        now: domain::UtcTimestamp,
        elapsed_since_start: Duration,
    ) -> Result<(), AdvanceOpenTenMinChecksError> {
        let already_prompted = check.last_reminder_at().is_some();
        match WorkSlotPollDecision::decide(elapsed_since_start, already_prompted) {
            WorkSlotPollOutcome::NoActionYet => Ok(()),
            WorkSlotPollOutcome::AskAreYouWorking => {
                let user_id = resolve_user_id(
                    &self.hour_interval_repository,
                    &self.work_day_repository,
                    check.hour_interval_id(),
                )
                .await?;
                self.notifier
                    .send(user_id, OutboundMessage::Text("Ты работаешь?".to_string()))
                    .await?;

                let updated = TenMinCheck::new(
                    check.id(),
                    check.hour_interval_id(),
                    check.task_id(),
                    check.started_at(),
                    None,
                    check.status(),
                    check.reason().map(str::to_string),
                    Some(now),
                );
                self.ten_min_check_repository.update(updated).await?;
                Ok(())
            }
            WorkSlotPollOutcome::ResolveNoResponse => {
                let ended_at = domain::UtcTimestamp::new(
                    check.started_at().value() + Duration::minutes(TEN_MIN_NOMINAL_MINUTES),
                );
                let updated = TenMinCheck::new(
                    check.id(),
                    check.hour_interval_id(),
                    check.task_id(),
                    check.started_at(),
                    Some(ended_at),
                    CheckStatus::NoResponse,
                    None,
                    None,
                );
                self.ten_min_check_repository.update(updated).await?;
                Ok(())
            }
        }
    }

    async fn advance_rest_check(
        &self,
        check: &TenMinCheck,
        now: domain::UtcTimestamp,
        elapsed_since_start: Duration,
    ) -> Result<(), AdvanceOpenTenMinChecksError> {
        if elapsed_since_start < Duration::minutes(TEN_MIN_NOMINAL_MINUTES) {
            return Ok(());
        }

        let wait_started = domain::UtcTimestamp::new(
            check.started_at().value() + Duration::minutes(TEN_MIN_NOMINAL_MINUTES),
        );
        let elapsed_since_wait_started = now.value() - wait_started.value();
        let reference = check.last_reminder_at().unwrap_or(wait_started);
        let elapsed_since_last_reminder = now.value() - reference.value();

        let should_remind = ReminderScheduleDecision::decide(
            elapsed_since_last_reminder,
            elapsed_since_wait_started,
            self.reminder_config.burst_window,
            self.reminder_config.burst_interval,
            self.reminder_config.steady_interval,
        );

        if should_remind {
            let user_id = resolve_user_id(
                &self.hour_interval_repository,
                &self.work_day_repository,
                check.hour_interval_id(),
            )
            .await?;
            self.notifier
                .send(
                    user_id,
                    OutboundMessage::Text("Перерыв затянулся, ты вернулся?".to_string()),
                )
                .await?;

            let updated = TenMinCheck::new(
                check.id(),
                check.hour_interval_id(),
                check.task_id(),
                check.started_at(),
                None,
                check.status(),
                check.reason().map(str::to_string),
                Some(now),
            );
            self.ten_min_check_repository.update(updated).await?;
        }

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
        HourInterval, HourIntervalId, SuccessfulCount, TaskId, TelegramId, TenMinCheckId,
        UtcTimestamp, WorkDay, WorkDayId,
    };

    struct Fixture {
        use_case: AdvanceOpenTenMinChecks,
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

        let use_case = AdvanceOpenTenMinChecks::new(
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

    async fn seed_open_work_check(
        fixture: &Fixture,
        started_at: UtcTimestamp,
        last_reminder_at: Option<UtcTimestamp>,
    ) -> TenMinCheck {
        let draft = TenMinCheck::new(
            TenMinCheckId::new(0),
            fixture.interval_id,
            fixture.task_id,
            started_at,
            None,
            CheckStatus::Worked,
            None,
            last_reminder_at,
        );
        fixture
            .ten_min_check_repository
            .create(draft)
            .await
            .unwrap()
    }

    #[test]
    fn work_slot_at_nine_minutes_does_nothing() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let started_at = fixture.clock.now();
            let check = seed_open_work_check(&fixture, started_at, None).await;
            fixture.clock.advance(chrono::Duration::minutes(9));

            fixture.use_case.execute().await.unwrap();

            let reloaded = fixture
                .ten_min_check_repository
                .find_by_id(check.id())
                .await
                .unwrap()
                .unwrap();
            assert!(reloaded.ended_at().is_none());
            assert_eq!(reloaded.last_reminder_at(), None);
            assert!(fixture.notifier.sent_messages().is_empty());
        });
    }

    #[test]
    fn work_slot_at_ten_minutes_asks_are_you_working() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let started_at = fixture.clock.now();
            let check = seed_open_work_check(&fixture, started_at, None).await;
            fixture.clock.advance(chrono::Duration::minutes(10));

            fixture.use_case.execute().await.unwrap();

            let reloaded = fixture
                .ten_min_check_repository
                .find_by_id(check.id())
                .await
                .unwrap()
                .unwrap();
            assert!(reloaded.ended_at().is_none());
            assert_eq!(reloaded.last_reminder_at(), Some(fixture.clock.now()));
            assert_eq!(
                fixture.notifier.sent_messages(),
                vec![(
                    fixture.user_id,
                    OutboundMessage::Text("Ты работаешь?".to_string())
                )]
            );
        });
    }

    #[test]
    fn work_slot_without_answer_on_next_tick_resolves_no_response_immediately() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let started_at = fixture.clock.now();
            seed_open_work_check(&fixture, started_at, None).await;
            fixture.clock.advance(chrono::Duration::minutes(10));
            fixture.use_case.execute().await.unwrap();

            fixture.clock.advance(chrono::Duration::minutes(1));
            fixture.use_case.execute().await.unwrap();

            let checks = fixture.ten_min_check_repository.snapshot();
            assert_eq!(checks.len(), 1);
            let closed = &checks[0];
            assert_eq!(closed.status(), CheckStatus::NoResponse);
            assert_eq!(closed.reason(), None);
            assert_eq!(closed.last_reminder_at(), None);
            let expected_ended_at =
                UtcTimestamp::new(started_at.value() + chrono::Duration::minutes(10));
            assert_eq!(closed.ended_at(), Some(expected_ended_at));

            // Никакая новая десятиминутка не создаётся этим вызовом.
            assert!(fixture.ten_min_check_repository.find_open().await.unwrap().is_empty());
        });
    }

    async fn seed_open_rest_check(fixture: &Fixture, started_at: UtcTimestamp) -> TenMinCheck {
        // Пять закрытых Worked-десятиминуток, чтобы classify_checks увидел
        // следующую как отдых по позиции.
        for i in 0..5 {
            let s = UtcTimestamp::new(started_at.value() - chrono::Duration::minutes((5 - i) * 10));
            let e = UtcTimestamp::new(s.value() + chrono::Duration::minutes(10));
            let draft = TenMinCheck::new(
                TenMinCheckId::new(0),
                fixture.interval_id,
                fixture.task_id,
                s,
                Some(e),
                CheckStatus::Worked,
                None,
                None,
            );
            fixture.ten_min_check_repository.create(draft).await.unwrap();
        }

        let draft = TenMinCheck::new(
            TenMinCheckId::new(0),
            fixture.interval_id,
            fixture.task_id,
            started_at,
            None,
            CheckStatus::Worked,
            None,
            None,
        );
        fixture
            .ten_min_check_repository
            .create(draft)
            .await
            .unwrap()
    }

    #[test]
    fn rest_slot_at_nine_minutes_does_nothing() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let started_at = fixture.clock.now();
            seed_open_rest_check(&fixture, started_at).await;
            fixture.clock.advance(chrono::Duration::minutes(9));

            fixture.use_case.execute().await.unwrap();

            assert!(fixture.notifier.sent_messages().is_empty());
            let open = fixture
                .ten_min_check_repository
                .find_open_by_interval(fixture.interval_id)
                .await
                .unwrap()
                .unwrap();
            assert!(open.ended_at().is_none());
        });
    }

    #[test]
    fn rest_slot_reminds_on_schedule_and_stays_open_until_marked_returned() {
        pollster::block_on(async {
            let fixture = fixture().await;
            let started_at = fixture.clock.now();
            seed_open_rest_check(&fixture, started_at).await;

            // Первое напоминание — по достижении burst_interval после
            // истечения номинальных 10 минут.
            fixture.clock.advance(chrono::Duration::minutes(11));
            fixture.use_case.execute().await.unwrap();
            assert_eq!(fixture.notifier.sent_messages().len(), 1);
            assert_eq!(
                fixture.notifier.sent_messages()[0].1,
                OutboundMessage::Text("Перерыв затянулся, ты вернулся?".to_string())
            );

            // В пределах burst_interval повторно не напоминает.
            fixture.clock.advance(chrono::Duration::seconds(30));
            fixture.use_case.execute().await.unwrap();
            assert_eq!(fixture.notifier.sent_messages().len(), 1);

            // Спустя ещё burst_interval — новое напоминание.
            fixture.clock.advance(chrono::Duration::minutes(1));
            fixture.use_case.execute().await.unwrap();
            assert_eq!(fixture.notifier.sent_messages().len(), 2);

            let open = fixture
                .ten_min_check_repository
                .find_open_by_interval(fixture.interval_id)
                .await
                .unwrap();
            assert!(open.is_some(), "rest stays open until MarkReturnedFromRest");
        });
    }
}
