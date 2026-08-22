//! Исполнение `Intent` (Задача 11): вызывает нужный use case из `UseCases`,
//! обновляет `ChatSession` и отправляет ответ в Telegram. Сюда, а не в
//! `intent.rs`, вынесено всё, что требует ввода-вывода — сеть Telegram и
//! асинхронные вызовы use cases. Никакой бизнес-логики здесь нет: любое
//! ветвление уже сделано либо в `domain`/`application` (внутри use cases),
//! либо в чистом `intent::interpret`.

use application::{Period, TaskFilter};
use domain::TelegramId;
use teloxide::prelude::*;
use teloxide::types::{ChatId, InlineKeyboardMarkup, InputFile};

use crate::callback::CallbackAction;
use crate::deps::UseCases;
use crate::intent::{Intent, PeriodKind};
use crate::keyboards;
use crate::session::{Awaiting, ChatSession, SessionStore};
use crate::text;

pub async fn handle_intent(
    bot: &Bot,
    deps: &UseCases,
    sessions: &SessionStore,
    chat_id: ChatId,
    user_id: TelegramId,
    intent: Intent,
) {
    let mut session = sessions.get(chat_id);

    match intent {
        Intent::ShowWelcome => {
            send_text(bot, chat_id, text::WELCOME).await;
        }
        Intent::StartDay => handle_start_day(bot, deps, chat_id, user_id, &mut session).await,
        Intent::FinishDay { work_day_id } => {
            handle_finish_day(bot, deps, chat_id, user_id, &mut session, work_day_id).await
        }
        Intent::ShowDayReport { work_day_id } => {
            match deps.build_day_report.execute(work_day_id).await {
                Ok(report) => send_text(bot, chat_id, &report.to_text()).await,
                Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
            }
        }
        Intent::ShowPeriodReport { period } => {
            let period = match period {
                PeriodKind::Week => Period::Week,
                PeriodKind::Month => Period::Month,
            };
            match deps.build_period_report.execute(user_id, period, None).await {
                Ok(report) => send_text(bot, chat_id, &report.to_text()).await,
                Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
            }
        }
        Intent::ExportCsv { work_day_id } => {
            match deps.export_day_report_csv.execute(work_day_id).await {
                Ok(bytes) => {
                    let file = InputFile::memory(bytes).file_name(text::CSV_FILENAME);
                    let _ = bot.send_document(chat_id, file).await;
                }
                Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
            }
        }
        Intent::ListTasksToStartInterval { work_day_id } => {
            handle_list_tasks_to_start(bot, deps, chat_id, user_id, work_day_id).await
        }
        Intent::ListTasksToSwitch { interval_id } => {
            handle_list_tasks_to_switch(bot, deps, chat_id, user_id, interval_id).await
        }
        Intent::ListAllTasks => handle_list_all_tasks(bot, deps, chat_id, user_id).await,
        Intent::StartInterval {
            work_day_id,
            task_id,
        } => {
            handle_start_interval(bot, deps, chat_id, &mut session, work_day_id, task_id).await
        }
        Intent::SwitchTask {
            interval_id,
            task_id,
        } => handle_switch_task(bot, deps, chat_id, interval_id, task_id).await,
        Intent::TenMinYes { interval_id } => {
            handle_ten_min_yes(bot, deps, chat_id, &mut session, interval_id).await
        }
        Intent::AskTenMinReason => {
            session.awaiting = Awaiting::TenMinReasonExplicit;
            send_text(bot, chat_id, text::ASK_WHAT_HAPPENED).await;
        }
        Intent::SubmitNoReason {
            interval_id,
            reason,
        } => {
            handle_submit_no_reason(bot, deps, chat_id, &mut session, interval_id, reason).await
        }
        Intent::SubmitLateReason {
            interval_id,
            reason,
        } => {
            handle_submit_late_reason(bot, deps, chat_id, &mut session, interval_id, reason).await
        }
        Intent::ConfirmContinue { interval_id } => {
            handle_confirm_continue(bot, deps, chat_id, &mut session, interval_id).await
        }
        Intent::MarkReturnedFromRest { interval_id } => {
            handle_mark_returned_from_rest(bot, deps, chat_id, &mut session, interval_id).await
        }
        Intent::EndLunch { work_day_id } => {
            handle_end_lunch(bot, deps, chat_id, &mut session, work_day_id).await
        }
        Intent::FinishInterval {
            interval_id,
            summary,
        } => {
            handle_finish_interval(bot, deps, chat_id, &mut session, interval_id, summary).await
        }
        Intent::StartLunch { work_day_id } => {
            handle_start_lunch(bot, deps, chat_id, &mut session, work_day_id).await
        }
        Intent::NewTaskTitleProvided(title) => {
            session.awaiting = Awaiting::NewTaskPriority { title };
            send_text_kb(
                bot,
                chat_id,
                text::NEW_TASK_ASK_PRIORITY,
                keyboards::priority_keyboard(),
            )
            .await;
        }
        Intent::NewTaskPriorityChosen { title, priority } => {
            session.awaiting = Awaiting::NewTaskDeadline { title, priority };
            send_text(bot, chat_id, text::NEW_TASK_ASK_DEADLINE).await;
        }
        Intent::CreateTask {
            title,
            priority,
            deadline,
        } => {
            handle_create_task(bot, deps, chat_id, user_id, &mut session, title, priority, deadline)
                .await
        }
        Intent::StartNewTaskDialog => {
            session.awaiting = Awaiting::NewTaskTitle;
            send_text(bot, chat_id, text::NEW_TASK_ASK_TITLE).await;
        }
        Intent::StartEditTaskDialog => {
            handle_start_edit_task_dialog(bot, deps, chat_id, user_id).await
        }
        Intent::EditTaskChosen(task_id) => {
            session.awaiting = Awaiting::EditTaskTitle { task_id };
            send_text_kb(
                bot,
                chat_id,
                text::EDIT_TASK_ASK_TITLE,
                keyboards::skip_field_keyboard(),
            )
            .await;
        }
        Intent::EditTaskTitleProvided { task_id, title } => {
            session.awaiting = Awaiting::EditTaskPriority { task_id, title };
            send_text_kb(
                bot,
                chat_id,
                text::NEW_TASK_ASK_PRIORITY,
                keyboards::priority_keyboard(),
            )
            .await;
        }
        Intent::EditTaskPriorityChosen {
            task_id,
            title,
            priority,
        } => {
            session.awaiting = Awaiting::EditTaskDeadline {
                task_id,
                title,
                priority,
            };
            send_text_kb(
                bot,
                chat_id,
                text::NEW_TASK_ASK_DEADLINE,
                keyboards::skip_field_keyboard(),
            )
            .await;
        }
        Intent::EditTaskDeadlineProvided {
            task_id,
            title,
            priority,
            deadline,
        } => {
            session.awaiting = Awaiting::EditTaskStatus {
                task_id,
                title,
                priority,
                deadline,
            };
            send_text_kb(
                bot,
                chat_id,
                text::EDIT_TASK_ASK_STATUS,
                keyboards::status_keyboard(),
            )
            .await;
        }
        Intent::UpdateTask {
            task_id,
            title,
            priority,
            deadline,
            status,
        } => {
            handle_update_task(
                bot, deps, chat_id, &mut session, task_id, title, priority, deadline, status,
            )
            .await
        }
        Intent::CancelDialog => {
            session.awaiting = Awaiting::Nothing;
            send_text(bot, chat_id, text::DIALOG_CANCELLED).await;
        }
        Intent::Unavailable(message) => {
            send_text(bot, chat_id, message).await;
        }
        Intent::InvalidInput(message) => {
            send_text(bot, chat_id, message).await;
        }
        Intent::Unrecognized => {
            send_text(bot, chat_id, text::UNKNOWN_INPUT).await;
        }
    }

    sessions.set(chat_id, session);
}

async fn send_text(bot: &Bot, chat_id: ChatId, message: &str) {
    let _ = bot.send_message(chat_id, message).await;
}

async fn send_text_kb(bot: &Bot, chat_id: ChatId, message: &str, keyboard: InlineKeyboardMarkup) {
    let _ = bot.send_message(chat_id, message).reply_markup(keyboard).await;
}

async fn handle_start_day(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    user_id: TelegramId,
    session: &mut ChatSession,
) {
    match deps.start_work_day.execute(user_id).await {
        Ok(work_day) => {
            *session = ChatSession {
                work_day_id: Some(work_day.id()),
                ..ChatSession::default()
            };
            send_text(bot, chat_id, text::DAY_STARTED).await;
            handle_list_tasks_to_start(bot, deps, chat_id, user_id, work_day.id()).await;
        }
        Err(application::StartWorkDayError::AlreadyOpen) => {
            send_text(bot, chat_id, text::DAY_ALREADY_OPEN).await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn handle_finish_day(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    user_id: TelegramId,
    session: &mut ChatSession,
    work_day_id: domain::WorkDayId,
) {
    match deps.finish_work_day.execute(work_day_id).await {
        Ok(summary) => {
            *session = ChatSession::default();
            send_text(bot, chat_id, &render_finish_summary(deps, user_id, &summary).await).await;
        }
        Err(application::FinishWorkDayError::AlreadyFinished) => {
            send_text(bot, chat_id, text::DAY_ALREADY_FINISHED).await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn render_finish_summary(
    deps: &UseCases,
    user_id: TelegramId,
    summary: &application::WorkDayFinishSummary,
) -> String {
    let titles = deps
        .list_available_tasks
        .execute(user_id, TaskFilter::All)
        .await
        .unwrap_or_default();

    let mut lines = vec![format!(
        "День завершён: {}/8 интервалов{}",
        summary.closed_intervals,
        if summary.norm_met {
            ", норма выполнена"
        } else {
            ""
        }
    )];
    lines.push("Время по задачам:".to_string());
    if summary.task_totals.is_empty() {
        lines.push("  (нет)".to_string());
    } else {
        for (task_id, duration) in &summary.task_totals {
            let title = titles
                .iter()
                .find(|task| task.id() == *task_id)
                .map(|task| task.title().to_string())
                .unwrap_or_else(|| format!("задача #{}", task_id.value()));
            lines.push(format!("  {title}: {}", text::format_duration(*duration)));
        }
    }
    lines.push(format!(
        "Обед: {}",
        text::format_duration(summary.lunch_duration.unwrap_or_else(chrono::Duration::zero))
    ));
    lines.join("\n")
}

async fn handle_list_tasks_to_start(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    user_id: TelegramId,
    _work_day_id: domain::WorkDayId,
) {
    match deps
        .list_available_tasks
        .execute(user_id, TaskFilter::Ready)
        .await
    {
        Ok(tasks) if tasks.is_empty() => send_text(bot, chat_id, text::NO_TASKS_TO_START).await,
        Ok(tasks) => {
            let keyboard = keyboards::task_list_keyboard(&tasks, CallbackAction::StartInterval);
            send_text_kb(bot, chat_id, text::PICK_TASK_TO_START, keyboard).await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn handle_list_tasks_to_switch(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    user_id: TelegramId,
    _interval_id: domain::HourIntervalId,
) {
    match deps
        .list_available_tasks
        .execute(user_id, TaskFilter::Ready)
        .await
    {
        Ok(tasks) if tasks.is_empty() => send_text(bot, chat_id, text::NO_TASKS_TO_START).await,
        Ok(tasks) => {
            let keyboard = keyboards::task_list_keyboard(&tasks, CallbackAction::SwitchTask);
            send_text_kb(bot, chat_id, text::PICK_TASK_TO_SWITCH, keyboard).await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn handle_list_all_tasks(bot: &Bot, deps: &UseCases, chat_id: ChatId, user_id: TelegramId) {
    match deps
        .list_available_tasks
        .execute(user_id, TaskFilter::All)
        .await
    {
        Ok(tasks) if tasks.is_empty() => send_text(bot, chat_id, text::NO_TASKS_AVAILABLE).await,
        Ok(tasks) => {
            let mut lines = vec![text::TASK_LIST_HEADER.to_string()];
            lines.extend(tasks.iter().map(text::task_list_line));
            send_text(bot, chat_id, &lines.join("\n")).await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn handle_start_interval(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    session: &mut ChatSession,
    work_day_id: domain::WorkDayId,
    task_id: domain::TaskId,
) {
    match deps.start_hour_interval.execute(work_day_id, task_id).await {
        Ok(interval) => {
            session.interval_id = Some(interval.id());
            session.worked_in_interval = 0;
            session.rest_active = false;
            session.awaiting = Awaiting::Nothing;
            send_text_kb(bot, chat_id, text::INTERVAL_STARTED, keyboards::ten_min_keyboard()).await;
        }
        Err(application::StartHourIntervalError::PreviousIntervalNotClosed) => {
            send_text(bot, chat_id, text::PREVIOUS_INTERVAL_NOT_CLOSED).await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn handle_switch_task(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    interval_id: domain::HourIntervalId,
    task_id: domain::TaskId,
) {
    match deps
        .switch_task_mid_interval
        .execute(interval_id, task_id)
        .await
    {
        Ok(_) => send_text(bot, chat_id, text::NEXT_SLOT_STARTED).await,
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn handle_ten_min_yes(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    session: &mut ChatSession,
    interval_id: domain::HourIntervalId,
) {
    match deps
        .submit_ten_min_answer
        .execute(interval_id, domain::TenMinCheckAnswer::Yes, None)
        .await
    {
        Ok(_closed) => {
            session.worked_in_interval += 1;
            if session.worked_in_interval >= 5 {
                session.rest_active = true;
                send_text_kb(bot, chat_id, text::REST_KEYBOARD_HINT, keyboards::returned_keyboard())
                    .await;
            } else {
                send_text_kb(bot, chat_id, text::WORKED_ACK, keyboards::ten_min_keyboard()).await;
            }
        }
        Err(application::SubmitTenMinAnswerError::NoOpenCheck) => {
            send_text(bot, chat_id, text::NO_OPEN_CHECK).await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn handle_submit_no_reason(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    session: &mut ChatSession,
    interval_id: domain::HourIntervalId,
    reason: String,
) {
    match deps
        .submit_ten_min_answer
        .execute(interval_id, domain::TenMinCheckAnswer::No, Some(reason))
        .await
    {
        Ok(_) => {
            session.awaiting = Awaiting::ConfirmContinue;
            send_text_kb(
                bot,
                chat_id,
                text::REASON_RECORDED,
                keyboards::confirm_continue_keyboard(),
            )
            .await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn handle_submit_late_reason(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    session: &mut ChatSession,
    interval_id: domain::HourIntervalId,
    reason: String,
) {
    match deps.submit_failure_reason.execute(interval_id, reason).await {
        Ok(_) => {
            session.awaiting = Awaiting::ConfirmContinue;
            send_text_kb(
                bot,
                chat_id,
                text::REASON_RECORDED,
                keyboards::confirm_continue_keyboard(),
            )
            .await;
        }
        // Скорее всего это не был поздний отклик на молчание, а просто
        // непонятое сообщение — см. пояснение в `intent::interpret_free_text`.
        Err(_) => send_text(bot, chat_id, text::UNKNOWN_INPUT).await,
    }
}

async fn handle_confirm_continue(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    session: &mut ChatSession,
    interval_id: domain::HourIntervalId,
) {
    match deps.confirm_ready_to_continue.execute(interval_id).await {
        Ok(_new_check) => {
            session.awaiting = Awaiting::Nothing;
            send_text_kb(bot, chat_id, text::NEXT_SLOT_STARTED, keyboards::ten_min_keyboard())
                .await;
        }
        Err(application::ConfirmReadyToContinueError::NothingToConfirm) => {
            send_text(bot, chat_id, text::NOTHING_TO_CONFIRM).await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn handle_mark_returned_from_rest(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    session: &mut ChatSession,
    interval_id: domain::HourIntervalId,
) {
    match deps.mark_returned_from_rest.execute(interval_id).await {
        Ok(_) => {
            session.rest_active = false;
            session.awaiting = Awaiting::HourSummary;
            send_text(bot, chat_id, text::ASK_HOUR_SUMMARY).await;
        }
        Err(application::MarkReturnedFromRestError::NoOpenRestCheck) => {
            send_text(bot, chat_id, text::NO_OPEN_REST_OR_LUNCH).await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn handle_end_lunch(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    session: &mut ChatSession,
    work_day_id: domain::WorkDayId,
) {
    match deps.end_lunch.execute(work_day_id).await {
        Ok(_) => {
            session.lunch_active = false;
            send_text_kb(bot, chat_id, text::LUNCH_ENDED, keyboards::idle_continue_keyboard())
                .await;
        }
        Err(application::EndLunchError::NotStarted) => {
            send_text(bot, chat_id, text::LUNCH_NOT_STARTED).await;
        }
        Err(application::EndLunchError::AlreadyEnded) => {
            send_text(bot, chat_id, text::LUNCH_ALREADY_ENDED).await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn handle_finish_interval(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    session: &mut ChatSession,
    interval_id: domain::HourIntervalId,
    summary: String,
) {
    match deps.finish_hour_interval.execute(interval_id, summary).await {
        Ok(interval) => {
            let work_day_id = session.work_day_id;
            session.interval_id = None;
            session.worked_in_interval = 0;
            session.rest_active = false;
            session.awaiting = Awaiting::Nothing;

            send_text(
                bot,
                chat_id,
                &text::interval_finished_summary(interval.successful_count().value(), 5),
            )
            .await;

            if let Some(work_day_id) = work_day_id {
                let _ = deps
                    .suggest_lunch_after_nth_interval
                    .execute(work_day_id)
                    .await;
            }

            send_text_kb(
                bot,
                chat_id,
                text::START_NEXT_INTERVAL_BUTTON,
                keyboards::after_interval_keyboard(),
            )
            .await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

async fn handle_start_lunch(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    session: &mut ChatSession,
    work_day_id: domain::WorkDayId,
) {
    match deps.start_lunch.execute(work_day_id).await {
        Ok(_) => {
            session.lunch_active = true;
            send_text_kb(bot, chat_id, text::LUNCH_STARTED, keyboards::returned_keyboard()).await;
        }
        Err(application::StartLunchError::ActiveIntervalOpen) => {
            send_text(bot, chat_id, text::LUNCH_ACTIVE_INTERVAL_OPEN).await;
        }
        Err(application::StartLunchError::AlreadyStarted) => {
            send_text(bot, chat_id, text::LUNCH_ALREADY_STARTED).await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_create_task(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    user_id: TelegramId,
    session: &mut ChatSession,
    title: String,
    priority: Option<domain::TaskPriority>,
    deadline: Option<chrono::NaiveDate>,
) {
    match deps
        .create_task
        .execute(user_id, title, priority, deadline)
        .await
    {
        Ok(_) => {
            session.awaiting = Awaiting::Nothing;
            send_text(bot, chat_id, text::NEW_TASK_CREATED).await;
        }
        Err(_) => {
            session.awaiting = Awaiting::Nothing;
            send_text(bot, chat_id, text::GENERIC_ERROR).await;
        }
    }
}

async fn handle_start_edit_task_dialog(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    user_id: TelegramId,
) {
    match deps
        .list_available_tasks
        .execute(user_id, TaskFilter::All)
        .await
    {
        Ok(tasks) if tasks.is_empty() => send_text(bot, chat_id, text::NO_TASKS_AVAILABLE).await,
        Ok(tasks) => {
            let keyboard = keyboards::task_list_keyboard(&tasks, CallbackAction::EditTask);
            send_text_kb(bot, chat_id, text::PICK_TASK_TO_EDIT, keyboard).await;
        }
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_update_task(
    bot: &Bot,
    deps: &UseCases,
    chat_id: ChatId,
    session: &mut ChatSession,
    task_id: domain::TaskId,
    title: Option<String>,
    priority: Option<Option<domain::TaskPriority>>,
    deadline: Option<Option<chrono::NaiveDate>>,
    status: Option<domain::TaskStatus>,
) {
    session.awaiting = Awaiting::Nothing;
    match deps
        .update_task
        .execute(task_id, title, priority, deadline, status)
        .await
    {
        Ok(_) => send_text(bot, chat_id, text::TASK_UPDATED).await,
        Err(_) => send_text(bot, chat_id, text::GENERIC_ERROR).await,
    }
}
