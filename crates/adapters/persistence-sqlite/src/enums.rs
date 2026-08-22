//! Явный маппинг rust enum ↔ TEXT в БД. Никаких строковых литералов вне этого
//! модуля — репозитории вызывают только эти функции.

use domain::{CheckStatus, TaskPriority, TaskStatus};

use crate::error::SqliteAdapterError;

pub fn task_status_to_db(status: TaskStatus) -> &'static str {
    match status {
        TaskStatus::Ready => "ready",
        TaskStatus::NotReady => "not_ready",
        TaskStatus::InProgress => "in_progress",
        TaskStatus::Done => "done",
    }
}

pub fn task_status_from_db(value: &str) -> Result<TaskStatus, SqliteAdapterError> {
    match value {
        "ready" => Ok(TaskStatus::Ready),
        "not_ready" => Ok(TaskStatus::NotReady),
        "in_progress" => Ok(TaskStatus::InProgress),
        "done" => Ok(TaskStatus::Done),
        other => Err(SqliteAdapterError::UnknownEnumValue {
            column: "tasks.status",
            value: other.to_string(),
        }),
    }
}

pub fn task_priority_to_db(priority: Option<TaskPriority>) -> Option<&'static str> {
    priority.map(|priority| match priority {
        TaskPriority::High => "high",
        TaskPriority::Medium => "medium",
        TaskPriority::Low => "low",
    })
}

pub fn task_priority_from_db(
    value: Option<String>,
) -> Result<Option<TaskPriority>, SqliteAdapterError> {
    value
        .map(|value| match value.as_str() {
            "high" => Ok(TaskPriority::High),
            "medium" => Ok(TaskPriority::Medium),
            "low" => Ok(TaskPriority::Low),
            other => Err(SqliteAdapterError::UnknownEnumValue {
                column: "tasks.priority",
                value: other.to_string(),
            }),
        })
        .transpose()
}

pub fn check_status_to_db(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Worked => "worked",
        CheckStatus::Failed => "failed",
        CheckStatus::NoResponse => "no_response",
    }
}

pub fn check_status_from_db(value: &str) -> Result<CheckStatus, SqliteAdapterError> {
    match value {
        "worked" => Ok(CheckStatus::Worked),
        "failed" => Ok(CheckStatus::Failed),
        "no_response" => Ok(CheckStatus::NoResponse),
        other => Err(SqliteAdapterError::UnknownEnumValue {
            column: "ten_min_checks.status",
            value: other.to_string(),
        }),
    }
}
