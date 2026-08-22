//! Отображение ошибок SQLite-адаптера в типизированный `application::RepoError`.

use application::RepoError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SqliteAdapterError {
    #[error("unknown value for {column}: {value}")]
    UnknownEnumValue { column: &'static str, value: String },

    #[error("column {column} was NULL but a value was expected")]
    UnexpectedNull { column: &'static str },
}

pub fn map_sqlx_error(err: sqlx::Error) -> RepoError {
    match err {
        sqlx::Error::RowNotFound => RepoError::NotFound,
        other => RepoError::Storage(other.to_string()),
    }
}

impl From<SqliteAdapterError> for RepoError {
    fn from(err: SqliteAdapterError) -> Self {
        RepoError::Storage(err.to_string())
    }
}
