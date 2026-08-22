//! Разбор входящего апдейта в намерение (`Intent`) — чистая функция без
//! ввода-вывода и без зависимости от `teloxide`/сети (критерий приёмки
//! Задачи 11: "разбор апдейта" тестируется без реального Telegram API).
//! `interpret` смотрит только на `ChatSession` (что бот сейчас ждёт от этого
//! чата плюс закэшированные идентификаторы) и на `IncomingEvent` — команду,
//! нажатую кнопку или свободный текст — и решает, какой use case вызвать
//! дальше (или что просто переспросить/показать без обращения к БД).
//! Собственно вызовы use cases и отправка сообщений — в `executor.rs`.

use chrono::NaiveDate;
use domain::{HourIntervalId, TaskId, TaskPriority, TaskStatus, TenMinCheckId, WorkDayId};

use crate::callback::CallbackAction;
use crate::commands::Command;
use crate::session::{parse_deadline_input, Awaiting, ChatSession};
use crate::text;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeriodKind {
    Week,
    Month,
}

/// Входящее событие в декодированном, но ещё не интерпретированном виде.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncomingEvent {
    Command(Command),
    Callback(CallbackAction),
    Text(String),
}

/// Результат интерпретации — что должен сделать `executor`. Часть вариантов
/// не требует обращения к use cases вообще (переспросить/показать ошибку) —
/// они помечены в комментариях как "чистые".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Intent {
    /// Чистый: показать приветствие/список команд.
    ShowWelcome,
    StartDay,
    FinishDay {
        work_day_id: WorkDayId,
    },
    ShowDayReport {
        work_day_id: WorkDayId,
    },
    ShowPeriodReport {
        period: PeriodKind,
    },
    ExportCsv {
        work_day_id: WorkDayId,
    },
    ListTasksToStartInterval {
        work_day_id: WorkDayId,
    },
    ListTasksToSwitch {
        interval_id: HourIntervalId,
    },
    ListAllTasks,
    StartInterval {
        work_day_id: WorkDayId,
        task_id: TaskId,
    },
    SwitchTask {
        interval_id: HourIntervalId,
        task_id: TaskId,
    },
    TenMinYes {
        interval_id: HourIntervalId,
    },
    /// Чистый: показать "Что случилось?" и запомнить, что ждём текст-причину.
    AskTenMinReason,
    SubmitNoReason {
        interval_id: HourIntervalId,
        reason: String,
    },
    SubmitLateReason {
        check_id: TenMinCheckId,
        reason: String,
    },
    ConfirmContinue {
        interval_id: HourIntervalId,
    },
    MarkReturnedFromRest {
        interval_id: HourIntervalId,
    },
    EndLunch {
        work_day_id: WorkDayId,
    },
    FinishInterval {
        interval_id: HourIntervalId,
        summary: String,
    },
    StartLunch {
        work_day_id: WorkDayId,
    },
    /// Чистый: запомнить название и спросить приоритет.
    NewTaskTitleProvided(String),
    /// Чистый: запомнить приоритет и спросить дедлайн.
    NewTaskPriorityChosen {
        title: String,
        priority: Option<TaskPriority>,
    },
    CreateTask {
        title: String,
        priority: Option<TaskPriority>,
        deadline: Option<NaiveDate>,
    },
    StartNewTaskDialog,
    StartEditTaskDialog,
    /// Чистый: запомнить выбранную задачу и спросить новое название.
    EditTaskChosen(TaskId),
    /// Чистый.
    EditTaskTitleProvided {
        task_id: TaskId,
        title: Option<String>,
    },
    /// Чистый.
    EditTaskPriorityChosen {
        task_id: TaskId,
        title: Option<String>,
        priority: Option<Option<TaskPriority>>,
    },
    /// Чистый.
    EditTaskDeadlineProvided {
        task_id: TaskId,
        title: Option<String>,
        priority: Option<Option<TaskPriority>>,
        deadline: Option<Option<NaiveDate>>,
    },
    UpdateTask {
        task_id: TaskId,
        title: Option<String>,
        priority: Option<Option<TaskPriority>>,
        deadline: Option<Option<NaiveDate>>,
        status: Option<TaskStatus>,
    },
    /// Чистый: сбросить текущий диалог (создание/редактирование задачи).
    CancelDialog,
    /// Чистый: показать сообщение об ошибке/недоступности, состояние не меняется.
    Unavailable(&'static str),
    /// Чистый: показать сообщение о неверном вводе, `awaiting` не меняется —
    /// пользователь может повторить попытку.
    InvalidInput(&'static str),
    /// Чистый: ничего осмысленного не распознано.
    Unrecognized,
}

fn is_task_ready_phrase(text: &str) -> bool {
    // Кириллица не ASCII, поэтому регистронезависимое сравнение — через
    // `to_lowercase()` (корректный Unicode case-folding), не
    // `eq_ignore_ascii_case`.
    text.trim().to_lowercase() == "задача готова"
}

pub fn interpret(session: &ChatSession, event: IncomingEvent) -> Intent {
    match event {
        IncomingEvent::Command(command) => interpret_command(session, command),
        IncomingEvent::Callback(action) => interpret_callback(session, action),
        IncomingEvent::Text(text) => interpret_text(session, text),
    }
}

fn interpret_command(session: &ChatSession, command: Command) -> Intent {
    match command {
        Command::Start => Intent::ShowWelcome,
        Command::StartDay => Intent::StartDay,
        Command::FinishDay => require_work_day(session, text::DAY_NOT_OPEN, |work_day_id| {
            Intent::FinishDay { work_day_id }
        }),
        Command::Report => require_work_day(session, text::REPORT_EMPTY_DAY, |work_day_id| {
            Intent::ShowDayReport { work_day_id }
        }),
        Command::ReportWeek => Intent::ShowPeriodReport {
            period: PeriodKind::Week,
        },
        Command::ReportMonth => Intent::ShowPeriodReport {
            period: PeriodKind::Month,
        },
        Command::ExportCsv => require_work_day(session, text::REPORT_EMPTY_DAY, |work_day_id| {
            Intent::ExportCsv { work_day_id }
        }),
        Command::NewTask => Intent::StartNewTaskDialog,
        Command::Tasks => Intent::ListAllTasks,
        Command::EditTask => Intent::StartEditTaskDialog,
    }
}

fn require_work_day(
    session: &ChatSession,
    unavailable: &'static str,
    build: impl FnOnce(WorkDayId) -> Intent,
) -> Intent {
    match session.work_day_id {
        Some(work_day_id) => build(work_day_id),
        None => Intent::Unavailable(unavailable),
    }
}

fn require_interval(
    session: &ChatSession,
    unavailable: &'static str,
    build: impl FnOnce(HourIntervalId) -> Intent,
) -> Intent {
    match session.interval_id {
        Some(interval_id) => build(interval_id),
        None => Intent::Unavailable(unavailable),
    }
}

fn interpret_callback(session: &ChatSession, action: CallbackAction) -> Intent {
    match action {
        CallbackAction::TenMinYes => {
            require_interval(session, text::NO_OPEN_CHECK, |interval_id| {
                Intent::TenMinYes { interval_id }
            })
        }
        CallbackAction::TenMinNo => require_interval(session, text::NO_OPEN_CHECK, |_| {
            Intent::AskTenMinReason
        }),
        CallbackAction::ConfirmContinue => {
            require_interval(session, text::NOTHING_TO_CONFIRM, |interval_id| {
                Intent::ConfirmContinue { interval_id }
            })
        }
        CallbackAction::Returned => interpret_returned(session),
        CallbackAction::TaskReady => {
            require_interval(session, text::NO_ACTIVE_TASK_TO_SWITCH, |interval_id| {
                Intent::ListTasksToSwitch { interval_id }
            })
        }
        CallbackAction::RequestStartInterval => {
            require_work_day(session, text::DAY_NOT_OPEN, |work_day_id| {
                Intent::ListTasksToStartInterval { work_day_id }
            })
        }
        CallbackAction::StartInterval(task_id) => {
            require_work_day(session, text::DAY_NOT_OPEN, |work_day_id| {
                Intent::StartInterval {
                    work_day_id,
                    task_id,
                }
            })
        }
        CallbackAction::SwitchTask(task_id) => {
            require_interval(session, text::NO_ACTIVE_TASK_TO_SWITCH, |interval_id| {
                Intent::SwitchTask {
                    interval_id,
                    task_id,
                }
            })
        }
        CallbackAction::GoToLunch => require_work_day(session, text::DAY_NOT_OPEN, |work_day_id| {
            Intent::StartLunch { work_day_id }
        }),
        CallbackAction::Priority(priority) => interpret_priority_choice(session, priority),
        CallbackAction::EditTask(task_id) => Intent::EditTaskChosen(task_id),
        CallbackAction::Status(status) => interpret_status_choice(session, status),
        CallbackAction::SkipField => interpret_skip_field(session),
        CallbackAction::Cancel => Intent::CancelDialog,
    }
}

fn interpret_returned(session: &ChatSession) -> Intent {
    if session.lunch_active {
        return require_work_day(session, text::DAY_NOT_OPEN, |work_day_id| {
            Intent::EndLunch { work_day_id }
        });
    }
    if session.rest_active {
        return require_interval(session, text::NO_OPEN_REST_OR_LUNCH, |interval_id| {
            Intent::MarkReturnedFromRest { interval_id }
        });
    }
    Intent::Unavailable(text::NO_OPEN_REST_OR_LUNCH)
}

fn interpret_priority_choice(session: &ChatSession, priority: Option<TaskPriority>) -> Intent {
    match &session.awaiting {
        Awaiting::NewTaskPriority { title } => Intent::NewTaskPriorityChosen {
            title: title.clone(),
            priority,
        },
        Awaiting::EditTaskPriority { task_id, title } => Intent::EditTaskPriorityChosen {
            task_id: *task_id,
            title: title.clone(),
            priority: Some(priority),
        },
        _ => Intent::Unrecognized,
    }
}

fn interpret_status_choice(session: &ChatSession, status: TaskStatus) -> Intent {
    match &session.awaiting {
        Awaiting::EditTaskStatus {
            task_id,
            title,
            priority,
            deadline,
        } => Intent::UpdateTask {
            task_id: *task_id,
            title: title.clone(),
            priority: *priority,
            deadline: *deadline,
            status: Some(status),
        },
        _ => Intent::Unrecognized,
    }
}

fn interpret_skip_field(session: &ChatSession) -> Intent {
    match &session.awaiting {
        Awaiting::EditTaskTitle { task_id } => Intent::EditTaskTitleProvided {
            task_id: *task_id,
            title: None,
        },
        Awaiting::EditTaskDeadline {
            task_id,
            title,
            priority,
        } => Intent::EditTaskDeadlineProvided {
            task_id: *task_id,
            title: title.clone(),
            priority: *priority,
            deadline: None,
        },
        _ => Intent::Unrecognized,
    }
}

fn interpret_text(session: &ChatSession, raw: String) -> Intent {
    match &session.awaiting {
        Awaiting::Nothing => interpret_free_text(session, raw),
        Awaiting::TenMinReasonExplicit => {
            require_interval(session, text::NO_OPEN_CHECK, |interval_id| {
                Intent::SubmitNoReason {
                    interval_id,
                    reason: raw,
                }
            })
        }
        Awaiting::ConfirmContinue => {
            // Любой непустой текст в этом состоянии трактуется как
            // подтверждение — кнопка "Готов продолжить" остаётся основным
            // путём, текст лишь резервный.
            if raw.trim().is_empty() {
                return Intent::Unrecognized;
            }
            require_interval(session, text::NOTHING_TO_CONFIRM, |interval_id| {
                Intent::ConfirmContinue { interval_id }
            })
        }
        Awaiting::HourSummary => require_interval(session, text::NO_OPEN_CHECK, |interval_id| {
            Intent::FinishInterval {
                interval_id,
                summary: raw,
            }
        }),
        Awaiting::NewTaskTitle => {
            if raw.trim().is_empty() {
                Intent::InvalidInput(text::EMPTY_TITLE)
            } else {
                Intent::NewTaskTitleProvided(raw)
            }
        }
        Awaiting::NewTaskPriority { .. } => Intent::Unrecognized,
        Awaiting::NewTaskDeadline { title, priority } => match parse_deadline_input(&raw) {
            Ok(deadline) => Intent::CreateTask {
                title: title.clone(),
                priority: *priority,
                deadline,
            },
            Err(()) => Intent::InvalidInput(text::INVALID_DATE),
        },
        Awaiting::EditTaskTitle { task_id } => {
            let trimmed = raw.trim();
            if trimmed.to_lowercase() == "нет" {
                Intent::EditTaskTitleProvided {
                    task_id: *task_id,
                    title: None,
                }
            } else if trimmed.is_empty() {
                Intent::InvalidInput(text::EMPTY_TITLE)
            } else {
                Intent::EditTaskTitleProvided {
                    task_id: *task_id,
                    title: Some(raw),
                }
            }
        }
        Awaiting::EditTaskPriority { .. } => Intent::Unrecognized,
        Awaiting::EditTaskDeadline {
            task_id,
            title,
            priority,
        } => {
            if raw.trim().to_lowercase() == "нет" {
                Intent::EditTaskDeadlineProvided {
                    task_id: *task_id,
                    title: title.clone(),
                    priority: *priority,
                    deadline: Some(None),
                }
            } else {
                match NaiveDate::parse_from_str(raw.trim(), "%Y-%m-%d") {
                    Ok(date) => Intent::EditTaskDeadlineProvided {
                        task_id: *task_id,
                        title: title.clone(),
                        priority: *priority,
                        deadline: Some(Some(date)),
                    },
                    Err(_) => Intent::InvalidInput(text::INVALID_DATE),
                }
            }
        }
        Awaiting::EditTaskStatus { .. } => Intent::Unrecognized,
    }
}

/// Свободный текст без активного диалога: единственные распознаваемые
/// случаи — фраза "задача готова" (README.md раздел 2, п.6: "пользователь
/// может в любой момент написать 'задача готова'") и поздний отклик на
/// молчание (README.md раздел 2, п.2) — десятиминутка к этому моменту уже
/// закрыта `PollLoop`-ом как `NoResponse` без участия адаптера, поэтому
/// текст трактуется как причина через `SubmitFailureReason`, а не через
/// `SubmitTenMinAnswer` (та ожидает **открытую** десятиминутку).
fn interpret_free_text(session: &ChatSession, raw: String) -> Intent {
    if is_task_ready_phrase(&raw) {
        return require_interval(session, text::NO_ACTIVE_TASK_TO_SWITCH, |interval_id| {
            Intent::ListTasksToSwitch { interval_id }
        });
    }
    if let (Some(_), Some(check_id)) = (session.interval_id, session.check_id) {
        return Intent::SubmitLateReason {
            check_id,
            reason: raw,
        };
    }
    Intent::Unrecognized
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_with(f: impl FnOnce(&mut ChatSession)) -> ChatSession {
        let mut session = ChatSession::default();
        f(&mut session);
        session
    }

    #[test]
    fn start_day_command_maps_directly() {
        let session = ChatSession::default();
        assert_eq!(
            interpret(&session, IncomingEvent::Command(Command::StartDay)),
            Intent::StartDay
        );
    }

    #[test]
    fn finish_day_without_cached_work_day_is_unavailable() {
        let session = ChatSession::default();
        assert_eq!(
            interpret(&session, IncomingEvent::Command(Command::FinishDay)),
            Intent::Unavailable(text::DAY_NOT_OPEN)
        );
    }

    #[test]
    fn finish_day_with_cached_work_day_uses_it() {
        let session = session_with(|s| s.work_day_id = Some(WorkDayId::new(5)));
        assert_eq!(
            interpret(&session, IncomingEvent::Command(Command::FinishDay)),
            Intent::FinishDay {
                work_day_id: WorkDayId::new(5)
            }
        );
    }

    #[test]
    fn report_week_and_month_do_not_need_a_cached_work_day() {
        let session = ChatSession::default();
        assert_eq!(
            interpret(&session, IncomingEvent::Command(Command::ReportWeek)),
            Intent::ShowPeriodReport {
                period: PeriodKind::Week
            }
        );
        assert_eq!(
            interpret(&session, IncomingEvent::Command(Command::ReportMonth)),
            Intent::ShowPeriodReport {
                period: PeriodKind::Month
            }
        );
    }

    #[test]
    fn ten_min_yes_requires_active_interval() {
        let session = ChatSession::default();
        assert_eq!(
            interpret(&session, IncomingEvent::Callback(CallbackAction::TenMinYes)),
            Intent::Unavailable(text::NO_OPEN_CHECK)
        );

        let session = session_with(|s| s.interval_id = Some(HourIntervalId::new(1)));
        assert_eq!(
            interpret(&session, IncomingEvent::Callback(CallbackAction::TenMinYes)),
            Intent::TenMinYes {
                interval_id: HourIntervalId::new(1)
            }
        );
    }

    #[test]
    fn ten_min_no_only_asks_for_reason_without_calling_a_use_case() {
        let session = session_with(|s| s.interval_id = Some(HourIntervalId::new(1)));
        assert_eq!(
            interpret(&session, IncomingEvent::Callback(CallbackAction::TenMinNo)),
            Intent::AskTenMinReason
        );
    }

    #[test]
    fn reason_text_after_explicit_no_submits_ten_min_answer() {
        let session = session_with(|s| {
            s.interval_id = Some(HourIntervalId::new(1));
            s.awaiting = Awaiting::TenMinReasonExplicit;
        });
        assert_eq!(
            interpret(&session, IncomingEvent::Text("отвлёкся".to_string())),
            Intent::SubmitNoReason {
                interval_id: HourIntervalId::new(1),
                reason: "отвлёкся".to_string(),
            }
        );
    }

    #[test]
    fn late_free_text_with_cached_check_submits_failure_reason() {
        let session = session_with(|s| {
            s.interval_id = Some(HourIntervalId::new(1));
            s.check_id = Some(TenMinCheckId::new(9));
        });
        assert_eq!(
            interpret(&session, IncomingEvent::Text("прости, отвлёкся".to_string())),
            Intent::SubmitLateReason {
                check_id: TenMinCheckId::new(9),
                reason: "прости, отвлёкся".to_string(),
            }
        );
    }

    #[test]
    fn task_ready_phrase_is_recognized_case_insensitively_as_free_text() {
        let session = session_with(|s| s.interval_id = Some(HourIntervalId::new(2)));
        assert_eq!(
            interpret(&session, IncomingEvent::Text("Задача Готова".to_string())),
            Intent::ListTasksToSwitch {
                interval_id: HourIntervalId::new(2)
            }
        );
    }

    #[test]
    fn returned_button_prefers_lunch_over_rest_when_both_flags_set() {
        let session = session_with(|s| {
            s.work_day_id = Some(WorkDayId::new(3));
            s.interval_id = Some(HourIntervalId::new(1));
            s.lunch_active = true;
            s.rest_active = true;
        });
        assert_eq!(
            interpret(&session, IncomingEvent::Callback(CallbackAction::Returned)),
            Intent::EndLunch {
                work_day_id: WorkDayId::new(3)
            }
        );
    }

    #[test]
    fn returned_button_uses_rest_when_lunch_not_active() {
        let session = session_with(|s| {
            s.interval_id = Some(HourIntervalId::new(1));
            s.rest_active = true;
        });
        assert_eq!(
            interpret(&session, IncomingEvent::Callback(CallbackAction::Returned)),
            Intent::MarkReturnedFromRest {
                interval_id: HourIntervalId::new(1)
            }
        );
    }

    #[test]
    fn returned_button_without_active_rest_or_lunch_is_unavailable() {
        let session = ChatSession::default();
        assert_eq!(
            interpret(&session, IncomingEvent::Callback(CallbackAction::Returned)),
            Intent::Unavailable(text::NO_OPEN_REST_OR_LUNCH)
        );
    }

    #[test]
    fn new_task_dialog_progresses_title_then_priority_then_deadline() {
        let mut session = ChatSession {
            awaiting: Awaiting::NewTaskTitle,
            ..ChatSession::default()
        };

        let after_title = interpret(&session, IncomingEvent::Text("Написать отчёт".to_string()));
        assert_eq!(
            after_title,
            Intent::NewTaskTitleProvided("Написать отчёт".to_string())
        );

        session.awaiting = Awaiting::NewTaskPriority {
            title: "Написать отчёт".to_string(),
        };
        let after_priority = interpret(
            &session,
            IncomingEvent::Callback(CallbackAction::Priority(Some(TaskPriority::High))),
        );
        assert_eq!(
            after_priority,
            Intent::NewTaskPriorityChosen {
                title: "Написать отчёт".to_string(),
                priority: Some(TaskPriority::High),
            }
        );

        session.awaiting = Awaiting::NewTaskDeadline {
            title: "Написать отчёт".to_string(),
            priority: Some(TaskPriority::High),
        };
        let after_deadline = interpret(&session, IncomingEvent::Text("2026-09-01".to_string()));
        assert_eq!(
            after_deadline,
            Intent::CreateTask {
                title: "Написать отчёт".to_string(),
                priority: Some(TaskPriority::High),
                deadline: Some(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()),
            }
        );
    }

    #[test]
    fn new_task_deadline_skip_word_creates_task_without_deadline() {
        let session = session_with(|s| {
            s.awaiting = Awaiting::NewTaskDeadline {
                title: "T".to_string(),
                priority: None,
            }
        });
        assert_eq!(
            interpret(&session, IncomingEvent::Text("нет".to_string())),
            Intent::CreateTask {
                title: "T".to_string(),
                priority: None,
                deadline: None,
            }
        );
    }

    #[test]
    fn empty_new_task_title_is_rejected_without_leaving_the_dialog() {
        let session = session_with(|s| s.awaiting = Awaiting::NewTaskTitle);
        assert_eq!(
            interpret(&session, IncomingEvent::Text("   ".to_string())),
            Intent::InvalidInput(text::EMPTY_TITLE)
        );
    }

    #[test]
    fn edit_task_skip_field_button_keeps_existing_title() {
        let session = session_with(|s| {
            s.awaiting = Awaiting::EditTaskTitle {
                task_id: TaskId::new(4),
            }
        });
        assert_eq!(
            interpret(&session, IncomingEvent::Callback(CallbackAction::SkipField)),
            Intent::EditTaskTitleProvided {
                task_id: TaskId::new(4),
                title: None,
            }
        );
    }

    #[test]
    fn edit_task_deadline_text_no_explicitly_clears_it() {
        let session = session_with(|s| {
            s.awaiting = Awaiting::EditTaskDeadline {
                task_id: TaskId::new(4),
                title: None,
                priority: None,
            }
        });
        assert_eq!(
            interpret(&session, IncomingEvent::Text("нет".to_string())),
            Intent::EditTaskDeadlineProvided {
                task_id: TaskId::new(4),
                title: None,
                priority: None,
                deadline: Some(None),
            }
        );
    }

    #[test]
    fn edit_task_deadline_skip_button_keeps_existing_value_as_dont_change() {
        let session = session_with(|s| {
            s.awaiting = Awaiting::EditTaskDeadline {
                task_id: TaskId::new(4),
                title: None,
                priority: None,
            }
        });
        assert_eq!(
            interpret(&session, IncomingEvent::Callback(CallbackAction::SkipField)),
            Intent::EditTaskDeadlineProvided {
                task_id: TaskId::new(4),
                title: None,
                priority: None,
                deadline: None,
            }
        );
    }

    #[test]
    fn status_choice_outside_edit_status_dialog_is_unrecognized() {
        let session = ChatSession::default();
        assert_eq!(
            interpret(
                &session,
                IncomingEvent::Callback(CallbackAction::Status(TaskStatus::Done))
            ),
            Intent::Unrecognized
        );
    }

    #[test]
    fn status_choice_inside_edit_status_dialog_builds_update_task() {
        let session = session_with(|s| {
            s.awaiting = Awaiting::EditTaskStatus {
                task_id: TaskId::new(1),
                title: Some("New".to_string()),
                priority: Some(Some(TaskPriority::Low)),
                deadline: Some(None),
            }
        });
        assert_eq!(
            interpret(
                &session,
                IncomingEvent::Callback(CallbackAction::Status(TaskStatus::Ready))
            ),
            Intent::UpdateTask {
                task_id: TaskId::new(1),
                title: Some("New".to_string()),
                priority: Some(Some(TaskPriority::Low)),
                deadline: Some(None),
                status: Some(TaskStatus::Ready),
            }
        );
    }

    #[test]
    fn cancel_callback_always_produces_cancel_dialog() {
        let session = session_with(|s| s.awaiting = Awaiting::NewTaskTitle);
        assert_eq!(
            interpret(&session, IncomingEvent::Callback(CallbackAction::Cancel)),
            Intent::CancelDialog
        );
    }

    #[test]
    fn confirm_continue_by_free_text_requires_non_empty_text() {
        let session = session_with(|s| {
            s.interval_id = Some(HourIntervalId::new(1));
            s.awaiting = Awaiting::ConfirmContinue;
        });
        assert_eq!(
            interpret(&session, IncomingEvent::Text(String::new())),
            Intent::Unrecognized
        );
        assert_eq!(
            interpret(&session, IncomingEvent::Text("готов".to_string())),
            Intent::ConfirmContinue {
                interval_id: HourIntervalId::new(1)
            }
        );
    }
}
