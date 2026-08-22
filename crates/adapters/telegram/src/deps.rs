//! Пучок use cases, которые вызывает этот адаптер — единственная точка входа
//! в `application` (критерий приёмки Задачи 11: ни одного прямого вызова
//! репозитория в обход use case). Конкретные реализации портов/use cases
//! собирает composition root (Задача 13); здесь только структура и
//! конструктор.

use std::sync::Arc;

use application::{
    BuildDayReport, BuildPeriodReport, ConfirmReadyToContinue, CreateTask, EndLunch,
    ExportDayReportCsv, FinishHourInterval, FinishWorkDay, ListAvailableTasks,
    MarkReturnedFromRest, RegisterUserIfNotExists, StartHourInterval, StartLunch, StartWorkDay,
    SubmitFailureReason, SubmitTenMinAnswer, SuggestLunchAfterNthInterval, SwitchTaskMidInterval,
    UpdateTask,
};

/// Все use cases, которые могут быть вызваны входящим апдейтом Telegram.
pub struct UseCases {
    pub register_user: Arc<RegisterUserIfNotExists>,
    pub start_work_day: Arc<StartWorkDay>,
    pub finish_work_day: Arc<FinishWorkDay>,
    pub create_task: Arc<CreateTask>,
    pub update_task: Arc<UpdateTask>,
    pub list_available_tasks: Arc<ListAvailableTasks>,
    pub start_hour_interval: Arc<StartHourInterval>,
    pub submit_ten_min_answer: Arc<SubmitTenMinAnswer>,
    pub submit_failure_reason: Arc<SubmitFailureReason>,
    pub confirm_ready_to_continue: Arc<ConfirmReadyToContinue>,
    pub mark_returned_from_rest: Arc<MarkReturnedFromRest>,
    pub switch_task_mid_interval: Arc<SwitchTaskMidInterval>,
    pub finish_hour_interval: Arc<FinishHourInterval>,
    pub start_lunch: Arc<StartLunch>,
    pub end_lunch: Arc<EndLunch>,
    pub suggest_lunch_after_nth_interval: Arc<SuggestLunchAfterNthInterval>,
    pub build_day_report: Arc<BuildDayReport>,
    pub build_period_report: Arc<BuildPeriodReport>,
    pub export_day_report_csv: Arc<ExportDayReportCsv>,
}

impl UseCases {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        register_user: Arc<RegisterUserIfNotExists>,
        start_work_day: Arc<StartWorkDay>,
        finish_work_day: Arc<FinishWorkDay>,
        create_task: Arc<CreateTask>,
        update_task: Arc<UpdateTask>,
        list_available_tasks: Arc<ListAvailableTasks>,
        start_hour_interval: Arc<StartHourInterval>,
        submit_ten_min_answer: Arc<SubmitTenMinAnswer>,
        submit_failure_reason: Arc<SubmitFailureReason>,
        confirm_ready_to_continue: Arc<ConfirmReadyToContinue>,
        mark_returned_from_rest: Arc<MarkReturnedFromRest>,
        switch_task_mid_interval: Arc<SwitchTaskMidInterval>,
        finish_hour_interval: Arc<FinishHourInterval>,
        start_lunch: Arc<StartLunch>,
        end_lunch: Arc<EndLunch>,
        suggest_lunch_after_nth_interval: Arc<SuggestLunchAfterNthInterval>,
        build_day_report: Arc<BuildDayReport>,
        build_period_report: Arc<BuildPeriodReport>,
        export_day_report_csv: Arc<ExportDayReportCsv>,
    ) -> Self {
        Self {
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
        }
    }
}
