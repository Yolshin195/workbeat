use chrono::Duration;

/// Номинальная длительность одной десятиминутки, используемая при расчёте
/// времени задачи (README.md раздел 3: "10 мин каждая", а не фактическая
/// `ended_at - started_at`, которая живёт отдельно в перерасходе, раздел 5).
pub const TEN_MIN_NOMINAL_MINUTES: i64 = 10;

/// Номинальная длительность часового интервала для расчёта перерасхода
/// (README.md раздел 5).
pub const NOMINAL_INTERVAL_MINUTES: i64 = 60;

/// Число часовых интервалов, из которых состоит дневная норма (README.md
/// раздел 1).
pub const NOMINAL_WORK_DAY_INTERVALS: u8 = 8;

/// Время задачи = сумма только успешных (`Worked`) десятиминуток (10 мин
/// каждая) + отдых (тоже 10 мин), если в интервале, где задача была активна,
/// рабочий этап закрыт естественно (5/5) — см. `HourIntervalState::is_rest_owed`.
/// Суммируется вызывающим кодом по всем интервалам, где эта задача была
/// активна (README.md раздел 3).
pub fn task_time(worked_count: u8, rest_earned: bool) -> Duration {
    let worked = Duration::minutes(TEN_MIN_NOMINAL_MINUTES) * i32::from(worked_count);
    let rest = if rest_earned {
        Duration::minutes(TEN_MIN_NOMINAL_MINUTES)
    } else {
        Duration::zero()
    };
    worked + rest
}

/// Дневная норма: сколько из 8 часовых интервалов закрыто.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyNormProgress {
    pub closed_intervals: u8,
    pub norm_met: bool,
}

pub fn daily_norm_progress(closed_intervals: u8) -> DailyNormProgress {
    DailyNormProgress {
        closed_intervals,
        norm_met: closed_intervals >= NOMINAL_WORK_DAY_INTERVALS,
    }
}

/// Перерасход одного часового интервала: факт длительность
/// (`ended_at - started_at`) минус номинальные 60 минут (README.md раздел 5).
/// Может быть отрицательным, если интервал закрыт принудительно раньше часа.
pub fn interval_overrun(actual_duration: Duration) -> Duration {
    actual_duration - Duration::minutes(NOMINAL_INTERVAL_MINUTES)
}

/// Перерасход дня: сумма перерасходов по всем интервалам (что эквивалентно
/// сумме фактических длительностей минус номинальные 8×60 минут, когда
/// интервалов ровно 8).
pub fn day_overrun(interval_overruns: &[Duration]) -> Duration {
    sum_durations(interval_overruns)
}

fn sum_durations(durations: &[Duration]) -> Duration {
    durations
        .iter()
        .fold(Duration::zero(), |acc, duration| acc + *duration)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hour_interval_state::HourIntervalState;
    use crate::ten_min_check::CheckStatus;
    use proptest::prelude::*;

    #[test]
    fn all_combinations_of_zero_to_five_successful_yield_correct_task_time_and_rest_flag() {
        for successful in 0u8..=5 {
            let statuses: Vec<CheckStatus> = (0..successful).map(|_| CheckStatus::Worked).collect();
            let rest_owed = HourIntervalState::is_rest_owed(&statuses);

            let expected_minutes = i64::from(successful) * TEN_MIN_NOMINAL_MINUTES
                + if rest_owed {
                    TEN_MIN_NOMINAL_MINUTES
                } else {
                    0
                };

            assert_eq!(
                task_time(successful, rest_owed),
                Duration::minutes(expected_minutes)
            );
            assert_eq!(rest_owed, successful == 5);
        }
    }

    #[test]
    fn task_time_splits_correctly_across_two_tasks_after_mid_interval_switch() {
        // Смена задачи после 2-й из 5 успешных: первая задача получает 2×10
        // мин без отдыха (интервал ещё не закрыт на момент переключения),
        // вторая — оставшиеся 3×10 мин + отдых, когда интервал в итоге
        // закрывается на её десятиминутках.
        let first_task_time = task_time(2, false);
        let second_task_time = task_time(3, true);

        assert_eq!(first_task_time, Duration::minutes(20));
        assert_eq!(second_task_time, Duration::minutes(40));
        assert_eq!(first_task_time + second_task_time, Duration::minutes(60));
    }

    #[test]
    fn overworked_rest_does_not_change_task_time_or_require_reason() {
        // Переработанный отдых (например, 30 минут вместо 10) не влияет на
        // task_time — она по-прежнему считается номинально (10 мин), а
        // фактический перерасход отдыха виден только в отчёте о перерасходе
        // интервала, не в времени задачи.
        assert_eq!(task_time(5, true), Duration::minutes(60));
    }

    #[test]
    fn daily_norm_progress_table() {
        for closed in 0u8..=9 {
            let progress = daily_norm_progress(closed);
            assert_eq!(progress.closed_intervals, closed);
            assert_eq!(progress.norm_met, closed >= 8);
        }
    }

    #[test]
    fn interval_overrun_is_zero_at_nominal_duration() {
        assert_eq!(interval_overrun(Duration::minutes(60)), Duration::zero());
    }

    #[test]
    fn interval_overrun_is_positive_when_longer_than_nominal() {
        assert_eq!(
            interval_overrun(Duration::minutes(150)),
            Duration::minutes(90)
        );
    }

    #[test]
    fn interval_overrun_is_negative_when_forced_finish_before_nominal() {
        assert_eq!(
            interval_overrun(Duration::minutes(20)),
            Duration::minutes(-40)
        );
    }

    #[test]
    fn day_overrun_sums_interval_overruns() {
        let overruns = [
            Duration::minutes(90) - Duration::minutes(60),
            Duration::zero(),
            Duration::minutes(45) - Duration::minutes(60),
        ];

        assert_eq!(day_overrun(&overruns), Duration::minutes(15));
    }

    proptest! {
        // Инвариант: сумма номинального времени успешных (Worked)
        // десятиминуток по задачам в рамках одного интервала никогда не
        // превышает 60 минут — интервал ограничен ровно 5 успешными слотами
        // по 10 минут каждый (README.md раздел 2, п.4/раздел 3); перерасход
        // от провалов/отдыха в эту сумму не входит и считается отдельно
        // функцией `interval_overrun`.
        #[test]
        fn worked_time_within_interval_never_exceeds_sixty_minutes(successful_count in 0u8..=5) {
            let worked = Duration::minutes(TEN_MIN_NOMINAL_MINUTES) * i32::from(successful_count);
            prop_assert!(worked <= Duration::minutes(60));
        }
    }
}
