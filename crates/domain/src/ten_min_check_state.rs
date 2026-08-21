use crate::error::DomainError;
use crate::ten_min_check::CheckStatus;

/// Машина состояний одной десятиминутки: `Pending` (открыта, таймер идёт или
/// бот ждёт ответа на "работаешь?") → `Closed(Worked | Failed | NoResponse)`
/// (README.md раздел 2, п.2). Никакого каскада на 2 десятиминутки нет: одно
/// молчание — один проваленный слот, закрытие всегда терминально для этой
/// строки (единственное легальное последующее изменение закрытой
/// `NoResponse`-строки — дозапись `reason`, см. `append_reason_to_no_response`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenMinCheckState {
    Pending,
    Closed(CheckStatus),
}

/// Ответ пользователя на "Ты работаешь?".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenMinCheckAnswer {
    Yes,
    No,
}

impl TenMinCheckState {
    /// Закрывает **открытую** десятиминутку явным ответом пользователя.
    /// Ошибка, если десятиминутка уже закрыта.
    pub fn submit_answer(self, answer: TenMinCheckAnswer) -> Result<TenMinCheckState, DomainError> {
        match self {
            TenMinCheckState::Pending => Ok(TenMinCheckState::Closed(match answer {
                TenMinCheckAnswer::Yes => CheckStatus::Worked,
                TenMinCheckAnswer::No => CheckStatus::Failed,
            })),
            TenMinCheckState::Closed(_) => Err(DomainError::TenMinCheckAlreadyClosed),
        }
    }

    /// Закрывает **открытую** десятиминутку немедленно как `NoResponse`
    /// (реакция на `WorkSlotPollOutcome::ResolveNoResponse`). Ошибка, если
    /// десятиминутка уже закрыта.
    pub fn resolve_no_response(self) -> Result<TenMinCheckState, DomainError> {
        match self {
            TenMinCheckState::Pending => Ok(TenMinCheckState::Closed(CheckStatus::NoResponse)),
            TenMinCheckState::Closed(_) => Err(DomainError::TenMinCheckAlreadyClosed),
        }
    }
}

/// Проверяет, допустима ли дозапись `reason` в уже закрытую десятиминутку —
/// единственный легальный случай изменения закрытой строки (README.md
/// раздел 2, п.2, сценарий "тишина"): пользователь наконец откликнулся и
/// объяснил, что случилось. Не затрагивает `ended_at`/`status` — эта функция
/// лишь валидирует, что дозапись допустима; сохранение самого `reason` —
/// забота вызывающего кода (application/adapters).
pub fn append_reason_to_no_response(
    status: CheckStatus,
    existing_reason: Option<&str>,
) -> Result<(), DomainError> {
    if status != CheckStatus::NoResponse {
        return Err(DomainError::ReasonNotAllowedForStatus);
    }
    if existing_reason.is_some() {
        return Err(DomainError::ReasonAlreadySet);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_answer_yes_closes_as_worked() {
        assert_eq!(
            TenMinCheckState::Pending.submit_answer(TenMinCheckAnswer::Yes),
            Ok(TenMinCheckState::Closed(CheckStatus::Worked))
        );
    }

    #[test]
    fn pending_answer_no_closes_as_failed() {
        assert_eq!(
            TenMinCheckState::Pending.submit_answer(TenMinCheckAnswer::No),
            Ok(TenMinCheckState::Closed(CheckStatus::Failed))
        );
    }

    #[test]
    fn pending_resolves_no_response_on_silence() {
        assert_eq!(
            TenMinCheckState::Pending.resolve_no_response(),
            Ok(TenMinCheckState::Closed(CheckStatus::NoResponse))
        );
    }

    #[test]
    fn closed_state_rejects_further_answers_and_resolutions() {
        for status in [
            CheckStatus::Worked,
            CheckStatus::Failed,
            CheckStatus::NoResponse,
        ] {
            let closed = TenMinCheckState::Closed(status);
            assert_eq!(
                closed.submit_answer(TenMinCheckAnswer::Yes),
                Err(DomainError::TenMinCheckAlreadyClosed)
            );
            assert_eq!(
                closed.resolve_no_response(),
                Err(DomainError::TenMinCheckAlreadyClosed)
            );
        }
    }

    #[test]
    fn reason_can_be_appended_only_to_no_response_without_existing_reason() {
        assert_eq!(
            append_reason_to_no_response(CheckStatus::NoResponse, None),
            Ok(())
        );
    }

    #[test]
    fn reason_append_rejected_for_worked_or_failed_status() {
        assert_eq!(
            append_reason_to_no_response(CheckStatus::Worked, None),
            Err(DomainError::ReasonNotAllowedForStatus)
        );
        // Failed уже имеет reason, дописанный при закрытии по явному "нет" —
        // повторная дозапись через этот путь всё равно не предназначена для
        // Failed-статуса.
        assert_eq!(
            append_reason_to_no_response(CheckStatus::Failed, None),
            Err(DomainError::ReasonNotAllowedForStatus)
        );
    }

    #[test]
    fn reason_append_rejected_if_already_set() {
        assert_eq!(
            append_reason_to_no_response(CheckStatus::NoResponse, Some("was afk")),
            Err(DomainError::ReasonAlreadySet)
        );
    }
}
