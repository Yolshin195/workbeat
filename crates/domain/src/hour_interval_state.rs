use crate::hour_interval::SuccessfulCount;
use crate::ten_min_check::CheckStatus;

/// Агрегирует закрытые **рабочие** десятиминутки одного часового интервала
/// (без десятиминутки отдыха — она никогда не подмешивается в этот список:
/// вызывающий код передаёт сюда только статусы рабочих попыток, см.
/// README.md раздел 2, п.2/п.4) → считает `successful_count` и решает, закрыт
/// ли рабочий этап. Провальные `Failed`/`NoResponse` в счёт не входят и не
/// занимают место одной из 5 нужных — они лишь добавляют интервалу ещё одну
/// повторную десятиминутку.
pub struct HourIntervalState;

impl HourIntervalState {
    /// Число успешных (`Worked`) десятиминуток среди уже закрытых рабочих
    /// попыток интервала.
    pub fn successful_count(closed_work_slot_statuses: &[CheckStatus]) -> u8 {
        closed_work_slot_statuses
            .iter()
            .filter(|status| **status == CheckStatus::Worked)
            .count() as u8
    }

    /// Рабочий этап интервала закрыт ровно тогда, когда `successful_count == 5`
    /// — независимо от того, сколько при этом было провальных попыток и
    /// сколько десятиминуток в интервале всего.
    pub fn is_work_stage_closed(closed_work_slot_statuses: &[CheckStatus]) -> bool {
        Self::successful_count(closed_work_slot_statuses) >= SuccessfulCount::MAX
    }

    /// Отдых положен всегда, когда рабочий этап закрыт естественно (5/5).
    /// Принудительное завершение (`/finish_day` посреди интервала) просто
    /// никогда не достигает `successful_count == 5`, поэтому отдельного флага
    /// "принудительно" здесь не требуется — это тот же самый предикат.
    pub fn is_rest_owed(closed_work_slot_statuses: &[CheckStatus]) -> bool {
        Self::is_work_stage_closed(closed_work_slot_statuses)
    }

    /// Позиционный факт "десятиминутка, которую сейчас создают — это слот
    /// отдыха": вычисляется по количеству и статусам уже существующих
    /// (закрытых) рабочих десятиминуток интервала, **до** создания новой —
    /// ровно когда среди них уже набралось 5 успешных. Этот флаг не хранится
    /// в БД; вызывающий код (Задача 7 плана) передаёт его дальше в
    /// `WorkSlotPollDecision`/`ReminderScheduleDecision`, чтобы различить
    /// поведение рабочего слота и слота отдыха.
    pub fn is_next_slot_rest(closed_work_slot_statuses: &[CheckStatus]) -> bool {
        Self::is_work_stage_closed(closed_work_slot_statuses)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_combinations_of_zero_to_five_successful() {
        for successful in 0u8..=5 {
            let statuses: Vec<CheckStatus> = (0..successful).map(|_| CheckStatus::Worked).collect();

            assert_eq!(HourIntervalState::successful_count(&statuses), successful);
            let expected_closed = successful == 5;
            assert_eq!(
                HourIntervalState::is_work_stage_closed(&statuses),
                expected_closed
            );
            assert_eq!(HourIntervalState::is_rest_owed(&statuses), expected_closed);
            assert_eq!(
                HourIntervalState::is_next_slot_rest(&statuses),
                expected_closed
            );
        }
    }

    #[test]
    fn failures_interleaved_with_successes_still_close_work_stage_at_five_worked() {
        // Worked, Failed, Worked, Worked, NoResponse, Worked, Worked — итого 7
        // десятиминуток, но successful_count = 5.
        let statuses = [
            CheckStatus::Worked,
            CheckStatus::Failed,
            CheckStatus::Worked,
            CheckStatus::Worked,
            CheckStatus::NoResponse,
            CheckStatus::Worked,
            CheckStatus::Worked,
        ];

        assert_eq!(HourIntervalState::successful_count(&statuses), 5);
        assert!(HourIntervalState::is_work_stage_closed(&statuses));
        assert!(HourIntervalState::is_rest_owed(&statuses));
    }

    #[test]
    fn single_no_response_fails_exactly_one_slot_without_resetting_progress() {
        // Сценарий "нет ответа": проваливает ровно один слот, уже набранные
        // Worked-десятиминутки сохранены, час не сбрасывается.
        let statuses = [
            CheckStatus::Worked,
            CheckStatus::Worked,
            CheckStatus::NoResponse,
        ];

        assert_eq!(HourIntervalState::successful_count(&statuses), 2);
        assert!(!HourIntervalState::is_work_stage_closed(&statuses));
    }

    #[test]
    fn forced_finish_before_five_successful_leaves_work_stage_open_and_no_rest() {
        let statuses = [CheckStatus::Worked, CheckStatus::Worked];

        assert_eq!(HourIntervalState::successful_count(&statuses), 2);
        assert!(!HourIntervalState::is_work_stage_closed(&statuses));
        assert!(!HourIntervalState::is_rest_owed(&statuses));
    }
}
