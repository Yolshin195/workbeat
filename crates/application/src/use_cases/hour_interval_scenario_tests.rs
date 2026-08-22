//! Сквозные сценарные тесты (given/when/then) для полного цикла часового
//! интервала (Задача 7 корневого `tasks.md`), комбинирующие несколько use
//! cases так, как это делал бы Telegram-адаптер. Тесты отдельных use cases
//! уже покрывают их изолированно — здесь проверяются именно переходы между
//! ними.

use std::sync::Arc;

use chrono::Utc;
use domain::{
    CheckStatus, Task, TaskId, TaskStatus, TelegramId, TenMinCheckAnswer, UtcTimestamp, WorkDay,
    WorkDayId,
};

use crate::ports::{Clock, TaskRepository, TenMinCheckRepository, WorkDayRepository};
use crate::testing::{
    FakeClock, FakeNotifier, InMemoryHourIntervalRepository, InMemoryTaskRepository,
    InMemoryTenMinCheckRepository, InMemoryWorkDayRepository,
};
use crate::use_cases::{
    AdvanceOpenTenMinChecks, ConfirmReadyToContinue, FinishHourInterval, MarkReturnedFromRest,
    MarkTaskDone, MarkTaskInProgress, StartHourInterval, SubmitFailureReason, SubmitTenMinAnswer,
    SwitchTaskMidInterval,
};
use crate::ReminderScheduleConfig;

struct World {
    hour_interval_repository: Arc<InMemoryHourIntervalRepository>,
    ten_min_check_repository: Arc<InMemoryTenMinCheckRepository>,
    work_day_repository: Arc<InMemoryWorkDayRepository>,
    task_repository: Arc<InMemoryTaskRepository>,
    notifier: Arc<FakeNotifier>,
    clock: Arc<FakeClock>,
    user_id: TelegramId,
    work_day_id: WorkDayId,
}

impl World {
    async fn new() -> Self {
        let hour_interval_repository = Arc::new(InMemoryHourIntervalRepository::new());
        let ten_min_check_repository = Arc::new(InMemoryTenMinCheckRepository::new());
        let work_day_repository = Arc::new(InMemoryWorkDayRepository::new());
        let task_repository = Arc::new(InMemoryTaskRepository::new());
        let notifier = Arc::new(FakeNotifier::new());
        let clock = Arc::new(FakeClock::new(UtcTimestamp::new(Utc::now())));

        let user_id = TelegramId::new(1).unwrap();
        let work_day = WorkDay::new(WorkDayId::new(0), user_id, clock.now(), None, None, None, None);
        let work_day = work_day_repository.create(work_day).await.unwrap();

        Self {
            hour_interval_repository,
            ten_min_check_repository,
            work_day_repository,
            task_repository,
            notifier,
            clock,
            user_id,
            work_day_id: work_day.id(),
        }
    }

    async fn seed_task(&self, title: &str) -> TaskId {
        let draft = Task::new(
            TaskId::new(0),
            self.user_id,
            title,
            TaskStatus::Ready,
            None,
            None,
            self.clock.now(),
        )
        .unwrap();
        self.task_repository.create(draft).await.unwrap().id()
    }

    fn start_hour_interval(&self) -> StartHourInterval {
        StartHourInterval::new(
            self.hour_interval_repository.clone(),
            self.ten_min_check_repository.clone(),
            Arc::new(MarkTaskInProgress::new(self.task_repository.clone())),
            self.clock.clone(),
        )
    }

    fn submit_ten_min_answer(&self) -> SubmitTenMinAnswer {
        SubmitTenMinAnswer::new(
            self.ten_min_check_repository.clone(),
            self.hour_interval_repository.clone(),
            self.work_day_repository.clone(),
            self.task_repository.clone(),
            self.clock.clone(),
            self.notifier.clone(),
        )
    }

    fn mark_returned_from_rest(&self) -> MarkReturnedFromRest {
        MarkReturnedFromRest::new(self.ten_min_check_repository.clone(), self.clock.clone())
    }

    fn finish_hour_interval(&self) -> FinishHourInterval {
        FinishHourInterval::new(
            self.hour_interval_repository.clone(),
            self.ten_min_check_repository.clone(),
            self.work_day_repository.clone(),
            self.clock.clone(),
            self.notifier.clone(),
        )
    }

    fn switch_task_mid_interval(&self) -> SwitchTaskMidInterval {
        SwitchTaskMidInterval::new(
            self.hour_interval_repository.clone(),
            self.work_day_repository.clone(),
            self.task_repository.clone(),
            Arc::new(MarkTaskDone::new(self.task_repository.clone())),
            Arc::new(MarkTaskInProgress::new(self.task_repository.clone())),
        )
    }

    fn advance_open_ten_min_checks(&self) -> AdvanceOpenTenMinChecks {
        AdvanceOpenTenMinChecks::new(
            self.ten_min_check_repository.clone(),
            self.hour_interval_repository.clone(),
            self.work_day_repository.clone(),
            self.clock.clone(),
            self.notifier.clone(),
            ReminderScheduleConfig::default(),
        )
    }

    fn submit_failure_reason(&self) -> SubmitFailureReason {
        SubmitFailureReason::new(self.ten_min_check_repository.clone())
    }

    fn confirm_ready_to_continue(&self) -> ConfirmReadyToContinue {
        ConfirmReadyToContinue::new(
            self.ten_min_check_repository.clone(),
            self.hour_interval_repository.clone(),
            self.work_day_repository.clone(),
            self.task_repository.clone(),
            self.clock.clone(),
        )
    }
}

#[test]
fn full_happy_path_five_worked_slots_earns_rest_and_closes_five_of_five() {
    pollster::block_on(async {
        let world = World::new().await;
        let task_id = world.seed_task("Write report").await;

        let interval = world
            .start_hour_interval()
            .execute(world.work_day_id, task_id)
            .await
            .unwrap();

        let submit = world.submit_ten_min_answer();
        for _ in 0..5 {
            submit
                .execute(interval.id(), TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();
        }

        // Рабочий этап исчерпан — открыта десятиминутка отдыха, рабочих
        // слотов больше не появляется.
        let checks = world.ten_min_check_repository.list_by_interval(interval.id()).await.unwrap();
        assert_eq!(checks.iter().filter(|c| c.ended_at().is_some()).count(), 5);
        assert_eq!(checks.iter().filter(|c| c.ended_at().is_none()).count(), 1);

        world.clock.advance(chrono::Duration::minutes(10));
        world
            .mark_returned_from_rest()
            .execute(interval.id())
            .await
            .unwrap();

        let finished = world
            .finish_hour_interval()
            .execute(interval.id(), "wrote the report".to_string())
            .await
            .unwrap();

        assert_eq!(finished.successful_count().value(), 5);
        assert_eq!(finished.summary(), Some("wrote the report"));
    });
}

#[test]
fn switching_task_mid_interval_routes_subsequent_checks_to_new_task() {
    pollster::block_on(async {
        let world = World::new().await;
        let task_a = world.seed_task("Task A").await;
        let task_b = world.seed_task("Task B").await;

        let interval = world
            .start_hour_interval()
            .execute(world.work_day_id, task_a)
            .await
            .unwrap();

        let submit = world.submit_ten_min_answer();
        // Первая десятиминутка закрывается ещё под Task A — она уже была в
        // процессе на момент переключения и не прерывается.
        let first_closed = submit
            .execute(interval.id(), TenMinCheckAnswer::Yes, None)
            .await
            .unwrap();
        assert_eq!(first_closed.task_id(), task_a);

        world
            .switch_task_mid_interval()
            .execute(interval.id(), task_b)
            .await
            .unwrap();

        // Задача A закрыта, задача B — в процессе.
        let a = world.task_repository.find_by_id(task_a).await.unwrap().unwrap();
        assert_eq!(a.status(), TaskStatus::Done);
        let b = world.task_repository.find_by_id(task_b).await.unwrap().unwrap();
        assert_eq!(b.status(), TaskStatus::InProgress);

        // Открытая сейчас десятиминутка (созданная предыдущим submit ещё
        // под Task A) продолжает как есть — SwitchTaskMidInterval её не
        // трогает, но следующая после её закрытия уже пойдёт под Task B.
        let currently_open = world
            .ten_min_check_repository
            .find_open_by_interval(interval.id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(currently_open.task_id(), task_a);

        submit
            .execute(interval.id(), TenMinCheckAnswer::Yes, None)
            .await
            .unwrap();

        let now_open = world
            .ten_min_check_repository
            .find_open_by_interval(interval.id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(now_open.task_id(), task_b);
    });
}

#[test]
fn silence_then_late_response_appends_reason_and_confirm_starts_fresh_slot() {
    pollster::block_on(async {
        let world = World::new().await;
        let task_id = world.seed_task("Write report").await;

        let interval = world
            .start_hour_interval()
            .execute(world.work_day_id, task_id)
            .await
            .unwrap();

        // Тишина: спросили "работаешь?", ответа нет, следующий тик
        // немедленно закрывает слот как NoResponse.
        world.clock.advance(chrono::Duration::minutes(10));
        world.advance_open_ten_min_checks().execute().await.unwrap();
        world.clock.advance(chrono::Duration::minutes(1));
        world.advance_open_ten_min_checks().execute().await.unwrap();

        let closed = world
            .ten_min_check_repository
            .find_last_by_interval(interval.id())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(closed.status(), CheckStatus::NoResponse);
        assert_eq!(closed.reason(), None);

        // Пользователь наконец откликается — reason дописывается в уже
        // закрытую строку, ended_at/status не меняются.
        world.clock.advance(chrono::Duration::minutes(3));
        let with_reason = world
            .submit_failure_reason()
            .execute(interval.id(), "was in a meeting".to_string())
            .await
            .unwrap();
        assert_eq!(with_reason.reason(), Some("was in a meeting"));
        assert_eq!(with_reason.ended_at(), closed.ended_at());
        assert_eq!(with_reason.status(), CheckStatus::NoResponse);

        // "Готов продолжить?" -> новая десятиминутка стартует в момент
        // подтверждения, не в момент закрытия предыдущей.
        world.clock.advance(chrono::Duration::minutes(1));
        let confirm_at = world.clock.now();
        let next = world
            .confirm_ready_to_continue()
            .execute(interval.id())
            .await
            .unwrap();

        assert_eq!(next.started_at(), confirm_at);
        assert_ne!(next.started_at(), closed.ended_at().unwrap());
        assert_eq!(next.task_id(), task_id);
        assert_eq!(next.ended_at(), None);
    });
}
