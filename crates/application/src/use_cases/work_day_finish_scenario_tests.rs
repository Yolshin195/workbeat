//! Сквозные (given/when/then) golden-тесты для `FinishWorkDay` (Задача 8
//! корневого `tasks.md`): собирают несколько закрытых часовых интервалов
//! через реальные use cases Задачи 7, как это делал бы Telegram-адаптер, и
//! сверяют итог `FinishWorkDay` с ручным расчётом по заранее известным
//! фикстурам-сценариям.

use std::sync::Arc;

use chrono::{Duration, Utc};
use domain::{
    HourIntervalId, Task, TaskId, TaskStatus, TelegramId, TenMinCheckAnswer, UtcTimestamp, WorkDay,
    WorkDayId,
};

use crate::ports::{Clock, HourIntervalRepository, TaskRepository, TenMinCheckRepository, WorkDayRepository};
use crate::testing::{
    FakeClock, FakeNotifier, InMemoryHourIntervalRepository, InMemoryTaskRepository,
    InMemoryTenMinCheckRepository, InMemoryWorkDayRepository,
};
use crate::use_cases::{
    EndLunch, FinishHourInterval, FinishWorkDay, MarkReturnedFromRest, MarkTaskDone,
    MarkTaskInProgress, StartHourInterval, StartLunch, SubmitTenMinAnswer, SwitchTaskMidInterval,
};

struct World {
    hour_interval_repository: Arc<InMemoryHourIntervalRepository>,
    ten_min_check_repository: Arc<InMemoryTenMinCheckRepository>,
    work_day_repository: Arc<InMemoryWorkDayRepository>,
    task_repository: Arc<InMemoryTaskRepository>,
    notifier: Arc<FakeNotifier>,
    clock: Arc<FakeClock>,
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
            work_day_id: work_day.id(),
        }
    }

    async fn seed_task(&self, title: &str) -> TaskId {
        let draft = Task::new(
            TaskId::new(0),
            TelegramId::new(1).unwrap(),
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

    fn start_lunch(&self) -> StartLunch {
        StartLunch::new(
            self.work_day_repository.clone(),
            self.hour_interval_repository.clone(),
            self.clock.clone(),
        )
    }

    fn end_lunch(&self) -> EndLunch {
        EndLunch::new(self.work_day_repository.clone(), self.clock.clone())
    }

    fn finish_work_day(&self) -> FinishWorkDay {
        FinishWorkDay::new(
            self.work_day_repository.clone(),
            self.hour_interval_repository.clone(),
            self.ten_min_check_repository.clone(),
            self.clock.clone(),
            self.notifier.clone(),
        )
    }

    /// Проводит один часовой интервал до естественного закрытия (5 успешных
    /// десятиминуток и отдых) на одной задаче, продвигая часы на 10 минут на
    /// каждой десятиминутке и ещё на 10 минут отдыха.
    async fn run_full_interval(&self, task_id: TaskId) -> HourIntervalId {
        let interval = self
            .start_hour_interval()
            .execute(self.work_day_id, task_id)
            .await
            .unwrap();

        let submit = self.submit_ten_min_answer();
        for _ in 0..5 {
            self.clock.advance(Duration::minutes(10));
            submit
                .execute(interval.id(), TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();
        }

        self.clock.advance(Duration::minutes(10));
        self.mark_returned_from_rest()
            .execute(interval.id())
            .await
            .unwrap();
        self.finish_hour_interval()
            .execute(interval.id(), "done".to_string())
            .await
            .unwrap();

        interval.id()
    }
}

#[test]
fn golden_scenario_two_full_intervals_same_task_sum_two_hours() {
    // Фикстура: два полностью закрытых интервала подряд на одной задаче.
    // Ручной расчёт: 2 закрытых интервала, задача получает 2×60 минут (5×10
    // работы + 10 отдыха на каждый интервал), обеда не было.
    pollster::block_on(async {
        let world = World::new().await;
        let task_id = world.seed_task("Write report").await;

        world.run_full_interval(task_id).await;
        world.run_full_interval(task_id).await;

        let summary = world.finish_work_day().execute(world.work_day_id).await.unwrap();

        assert_eq!(summary.closed_intervals, 2);
        assert!(!summary.norm_met);
        assert_eq!(summary.lunch_duration, None);
        assert_eq!(summary.task_totals, vec![(task_id, Duration::minutes(120))]);
    });
}

#[test]
fn golden_scenario_task_switch_mid_interval_splits_time_and_credits_rest_to_second_task() {
    // Фикстура: один интервал, переключение задачи после 2-й из 5 успешных.
    // Ручной расчёт (см. domain::time_calculations тест
    // task_time_splits_correctly_across_two_tasks_after_mid_interval_switch):
    // task_a — 20 минут (2×10, без отдыха), task_b — 40 минут (3×10 + отдых).
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
        // Один успешный слот под Task A закрывается, следующий уже создан
        // (ещё под Task A — `find_in_progress_task` резолвится в момент
        // создания слота, до переключения).
        world.clock.advance(Duration::minutes(10));
        submit
            .execute(interval.id(), TenMinCheckAnswer::Yes, None)
            .await
            .unwrap();

        // Переключение происходит, пока второй слот ещё открыт под Task A —
        // `SwitchTaskMidInterval` не трогает уже созданный открытый слот
        // (README.md раздел 3), он закроется под старой задачей.
        world
            .switch_task_mid_interval()
            .execute(interval.id(), task_b)
            .await
            .unwrap();

        for _ in 0..4 {
            world.clock.advance(Duration::minutes(10));
            submit
                .execute(interval.id(), TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();
        }

        world.clock.advance(Duration::minutes(10));
        world
            .mark_returned_from_rest()
            .execute(interval.id())
            .await
            .unwrap();
        world
            .finish_hour_interval()
            .execute(interval.id(), "done".to_string())
            .await
            .unwrap();

        let summary = world.finish_work_day().execute(world.work_day_id).await.unwrap();

        assert_eq!(summary.closed_intervals, 1);
        let mut totals = summary.task_totals.clone();
        totals.sort_by_key(|(task_id, _)| task_id.value());
        let mut expected = vec![(task_a, Duration::minutes(20)), (task_b, Duration::minutes(40))];
        expected.sort_by_key(|(task_id, _)| task_id.value());
        assert_eq!(totals, expected);
    });
}

#[test]
fn golden_scenario_lunch_after_fourth_interval_then_forced_finish_mid_fifth() {
    // Фикстура: 4 полных интервала на task_a, обед (30 минут), затем 5-й
    // интервал начат на task_b и принудительно завершён после 2 успешных из
    // 5 — рабочая десятиминутка ещё открыта на момент /finish_day. Ручной
    // расчёт: 5 закрытых интервалов (4 полных + принудительно закрытый 5-й),
    // task_a — 4×60 = 240 минут, task_b — 2×10 = 20 минут (отдых не положен,
    // рабочий этап не закрыт естественно), обед — 30 минут отдельной строкой.
    pollster::block_on(async {
        let world = World::new().await;
        let task_a = world.seed_task("Task A").await;
        let task_b = world.seed_task("Task B").await;

        for _ in 0..4 {
            world.run_full_interval(task_a).await;
        }

        world.start_lunch().execute(world.work_day_id).await.unwrap();
        world.clock.advance(Duration::minutes(30));
        world.end_lunch().execute(world.work_day_id).await.unwrap();

        // Task A закончена явно, иначе она осталась бы `InProgress`
        // (интервалы её не переключают сами по себе) и мешала бы
        // `find_in_progress_task` однозначно найти Task B ниже.
        MarkTaskDone::new(world.task_repository.clone())
            .execute(task_a)
            .await
            .unwrap();

        let interval = world
            .start_hour_interval()
            .execute(world.work_day_id, task_b)
            .await
            .unwrap();
        let submit = world.submit_ten_min_answer();
        for _ in 0..2 {
            world.clock.advance(Duration::minutes(10));
            submit
                .execute(interval.id(), TenMinCheckAnswer::Yes, None)
                .await
                .unwrap();
        }
        // Третья рабочая десятиминутка стартовала, но пользователь не успел
        // ответить — она остаётся открытой на момент /finish_day.
        world.clock.advance(Duration::minutes(7));

        let summary = world.finish_work_day().execute(world.work_day_id).await.unwrap();

        assert_eq!(summary.closed_intervals, 5);
        assert!(!summary.norm_met);
        assert_eq!(summary.lunch_duration, Some(Duration::minutes(30)));

        let mut totals = summary.task_totals.clone();
        totals.sort_by_key(|(task_id, _)| task_id.value());
        let mut expected = vec![(task_a, Duration::minutes(240)), (task_b, Duration::minutes(20))];
        expected.sort_by_key(|(task_id, _)| task_id.value());
        assert_eq!(totals, expected);

        // Ни одной открытой десятиминутки/интервала не осталось.
        assert!(world
            .ten_min_check_repository
            .find_open()
            .await
            .unwrap()
            .is_empty());
        assert!(world
            .hour_interval_repository
            .find_open_by_work_day(world.work_day_id)
            .await
            .unwrap()
            .is_none());
    });
}
