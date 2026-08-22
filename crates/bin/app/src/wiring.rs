//! Явная сборка (Задача 13 корневого `tasks.md`): конкретные SQLite-репозитории
//! и переданный `Notifier`/`Clock` передаются в конструкторы use cases из
//! `application` (Задачи 5-9), use cases передаются в пучок `UseCases`
//! Telegram-адаптера (Задача 11) и в `PollLoop` (Задача 10). Ничего из этого
//! не знает о конкретной реализации `Notifier`/`Clock` — они приходят сюда
//! уже как `Arc<dyn Trait>`, собранные вызывающей стороной (`run()` в
//! `lib.rs`), поэтому эта функция одинаково пригодна и для продового запуска,
//! и для интеграционных тестов restart-safety с фейковым `Notifier`.

use std::sync::Arc;
use std::time::Duration;

use adapters_persistence_sqlite::{
    SqliteHourIntervalRepository, SqliteTaskRepository, SqliteTenMinCheckRepository,
    SqliteUserRepository, SqliteWorkDayRepository,
};
use adapters_scheduler_tokio::PollLoop;
use application::{
    AdvanceOpenTenMinChecks, BuildDayReport, BuildPeriodReport, Clock, ConfirmReadyToContinue,
    CreateTask, EndLunch, ExportDayReportCsv, FinishHourInterval, FinishWorkDay, IdlePromptConfig,
    ListAvailableTasks, MarkReturnedFromRest, MarkTaskDone, MarkTaskInProgress, Notifier,
    PromptContinueIfIdle, RegisterUserIfNotExists, RemindAwaitingResume, ReminderScheduleConfig,
    StartHourInterval, StartLunch, StartWorkDay, SubmitFailureReason, SubmitTenMinAnswer,
    SuggestLunchAfterNthInterval, SwitchTaskMidInterval, UpdateTask,
};
use sqlx::SqlitePool;

/// Всё, что нужно, чтобы собрать use cases — конкретные адаптеры портов, не
/// знающие друг о друге (SQLite-репозитории собираются здесь из `pool`,
/// `clock`/`notifier`/пороги конфигурации передаются вызывающей стороной).
pub struct Adapters {
    pub pool: SqlitePool,
    pub clock: Arc<dyn Clock>,
    pub notifier: Arc<dyn Notifier>,
    pub idle_prompt: IdlePromptConfig,
    pub reminder_schedule: ReminderScheduleConfig,
    pub poll_interval: Duration,
}

/// Итог сборки: пучок use cases для Telegram-адаптера (Задача 11) и
/// `PollLoop` для фонового опроса (Задача 10) — единственные два потребителя
/// use cases во всей системе.
pub struct Wired {
    pub telegram_use_cases: Arc<adapters_telegram::UseCases>,
    pub poll_loop: PollLoop,
}

pub fn wire(adapters: Adapters) -> Wired {
    let Adapters {
        pool,
        clock,
        notifier,
        idle_prompt,
        reminder_schedule,
        poll_interval,
    } = adapters;

    let user_repository = Arc::new(SqliteUserRepository::new(pool.clone()));
    let task_repository = Arc::new(SqliteTaskRepository::new(pool.clone()));
    let work_day_repository = Arc::new(SqliteWorkDayRepository::new(pool.clone()));
    let hour_interval_repository = Arc::new(SqliteHourIntervalRepository::new(pool.clone()));
    let ten_min_check_repository = Arc::new(SqliteTenMinCheckRepository::new(pool));

    let mark_task_in_progress = Arc::new(MarkTaskInProgress::new(task_repository.clone()));
    let mark_task_done = Arc::new(MarkTaskDone::new(task_repository.clone()));

    let register_user = Arc::new(RegisterUserIfNotExists::new(
        user_repository.clone(),
        clock.clone(),
    ));
    let start_work_day = Arc::new(StartWorkDay::new(
        work_day_repository.clone(),
        clock.clone(),
    ));
    let finish_work_day = Arc::new(FinishWorkDay::new(
        work_day_repository.clone(),
        hour_interval_repository.clone(),
        ten_min_check_repository.clone(),
        clock.clone(),
        notifier.clone(),
    ));

    let create_task = Arc::new(CreateTask::new(task_repository.clone(), clock.clone()));
    let update_task = Arc::new(UpdateTask::new(task_repository.clone()));
    let list_available_tasks = Arc::new(ListAvailableTasks::new(task_repository.clone()));

    let start_hour_interval = Arc::new(StartHourInterval::new(
        hour_interval_repository.clone(),
        ten_min_check_repository.clone(),
        mark_task_in_progress.clone(),
        clock.clone(),
    ));
    let submit_ten_min_answer = Arc::new(SubmitTenMinAnswer::new(
        ten_min_check_repository.clone(),
        hour_interval_repository.clone(),
        work_day_repository.clone(),
        task_repository.clone(),
        clock.clone(),
        notifier.clone(),
    ));
    let submit_failure_reason =
        Arc::new(SubmitFailureReason::new(ten_min_check_repository.clone()));
    let confirm_ready_to_continue = Arc::new(ConfirmReadyToContinue::new(
        ten_min_check_repository.clone(),
        hour_interval_repository.clone(),
        work_day_repository.clone(),
        task_repository.clone(),
        clock.clone(),
    ));
    let mark_returned_from_rest = Arc::new(MarkReturnedFromRest::new(
        ten_min_check_repository.clone(),
        clock.clone(),
    ));
    let switch_task_mid_interval = Arc::new(SwitchTaskMidInterval::new(
        hour_interval_repository.clone(),
        work_day_repository.clone(),
        task_repository.clone(),
        mark_task_done.clone(),
        mark_task_in_progress.clone(),
    ));
    let finish_hour_interval = Arc::new(FinishHourInterval::new(
        hour_interval_repository.clone(),
        ten_min_check_repository.clone(),
        work_day_repository.clone(),
        clock.clone(),
        notifier.clone(),
    ));

    let start_lunch = Arc::new(StartLunch::new(
        work_day_repository.clone(),
        hour_interval_repository.clone(),
        clock.clone(),
    ));
    let end_lunch = Arc::new(EndLunch::new(work_day_repository.clone(), clock.clone()));
    let suggest_lunch_after_nth_interval = Arc::new(SuggestLunchAfterNthInterval::new(
        hour_interval_repository.clone(),
        work_day_repository.clone(),
        notifier.clone(),
    ));

    let build_day_report = Arc::new(BuildDayReport::new(
        work_day_repository.clone(),
        hour_interval_repository.clone(),
        ten_min_check_repository.clone(),
        task_repository.clone(),
    ));
    let build_period_report = Arc::new(BuildPeriodReport::new(
        work_day_repository.clone(),
        hour_interval_repository.clone(),
        ten_min_check_repository.clone(),
        task_repository.clone(),
        user_repository.clone(),
        clock.clone(),
    ));
    let export_day_report_csv = Arc::new(ExportDayReportCsv::new(
        work_day_repository.clone(),
        hour_interval_repository.clone(),
        ten_min_check_repository.clone(),
        task_repository.clone(),
    ));

    let advance_open_ten_min_checks = Arc::new(AdvanceOpenTenMinChecks::new(
        ten_min_check_repository.clone(),
        hour_interval_repository.clone(),
        work_day_repository.clone(),
        clock.clone(),
        notifier.clone(),
        reminder_schedule,
    ));
    let remind_awaiting_resume = Arc::new(RemindAwaitingResume::new(
        ten_min_check_repository.clone(),
        hour_interval_repository.clone(),
        work_day_repository.clone(),
        clock.clone(),
        notifier.clone(),
        reminder_schedule,
    ));
    let prompt_continue_if_idle = Arc::new(PromptContinueIfIdle::new(
        work_day_repository.clone(),
        hour_interval_repository.clone(),
        clock.clone(),
        notifier.clone(),
        idle_prompt,
    ));

    let telegram_use_cases = Arc::new(adapters_telegram::UseCases::new(
        register_user,
        start_work_day,
        finish_work_day,
        create_task,
        update_task,
        list_available_tasks,
        start_hour_interval,
        submit_ten_min_answer,
        submit_failure_reason,
        confirm_ready_to_continue,
        mark_returned_from_rest,
        switch_task_mid_interval,
        finish_hour_interval,
        start_lunch,
        end_lunch,
        suggest_lunch_after_nth_interval,
        build_day_report,
        build_period_report,
        export_day_report_csv,
    ));

    let poll_loop = PollLoop::new(
        advance_open_ten_min_checks,
        remind_awaiting_resume,
        prompt_continue_if_idle,
        poll_interval,
    );

    Wired {
        telegram_use_cases,
        poll_loop,
    }
}
