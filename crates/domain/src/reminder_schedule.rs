use chrono::Duration;

/// Чистая функция "пора ли слать очередное напоминание" (README.md раздел 2,
/// п.3). Общая для двух ситуаций: (а) десятиминутка отдыха, оставшаяся
/// открытой дольше 10 минут, ждёт фактического возврата; (б) уже закрытая
/// проваленная рабочая десятиминутка — последняя в интервале, следующая ещё
/// не началась — ждёт, когда пользователь объяснит причину и подтвердит
/// "готов продолжить". Пока `elapsed_since_wait_started < burst_window` —
/// сравнивает с `burst_interval`, иначе — со `steady_interval`. Пороговые
/// константы приходят параметром снаружи (конфигурируются, см. Задачу 13
/// плана) — сама функция лишь применяет их к переданным длительностям.
pub struct ReminderScheduleDecision;

impl ReminderScheduleDecision {
    pub fn decide(
        elapsed_since_last_reminder: Duration,
        elapsed_since_wait_started: Duration,
        burst_window: Duration,
        burst_interval: Duration,
        steady_interval: Duration,
    ) -> bool {
        let required_interval = if elapsed_since_wait_started < burst_window {
            burst_interval
        } else {
            steady_interval
        };
        elapsed_since_last_reminder >= required_interval
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Case {
        elapsed_since_last_reminder: Duration,
        elapsed_since_wait_started: Duration,
        expected: bool,
    }

    #[test]
    fn table_driven_burst_window_boundaries() {
        let burst_window = Duration::minutes(5);
        let burst_interval = Duration::minutes(1);
        let steady_interval = Duration::minutes(5);

        let cases = [
            // внутри burst_window, недостаточно времени с прошлого напоминания
            Case {
                elapsed_since_last_reminder: Duration::seconds(30),
                elapsed_since_wait_started: Duration::minutes(2),
                expected: false,
            },
            // внутри burst_window, ровно burst_interval с прошлого напоминания
            Case {
                elapsed_since_last_reminder: Duration::minutes(1),
                elapsed_since_wait_started: Duration::minutes(2),
                expected: true,
            },
            // ровно на границе burst_window (включается steady-режим),
            // burst_interval бы сработал, а steady_interval — ещё нет
            Case {
                elapsed_since_last_reminder: Duration::minutes(1),
                elapsed_since_wait_started: burst_window,
                expected: false,
            },
            // на границе burst_window, достаточно для steady_interval
            Case {
                elapsed_since_last_reminder: Duration::minutes(5),
                elapsed_since_wait_started: burst_window,
                expected: true,
            },
            // чуть раньше границы burst_window — ещё burst-режим
            Case {
                elapsed_since_last_reminder: Duration::minutes(1),
                elapsed_since_wait_started: burst_window - Duration::seconds(1),
                expected: true,
            },
            // далеко за burst_window — steady-режим
            Case {
                elapsed_since_last_reminder: Duration::minutes(4),
                elapsed_since_wait_started: Duration::minutes(30),
                expected: false,
            },
            Case {
                elapsed_since_last_reminder: Duration::minutes(5),
                elapsed_since_wait_started: Duration::minutes(30),
                expected: true,
            },
        ];

        for case in cases {
            assert_eq!(
                ReminderScheduleDecision::decide(
                    case.elapsed_since_last_reminder,
                    case.elapsed_since_wait_started,
                    burst_window,
                    burst_interval,
                    steady_interval,
                ),
                case.expected,
                "elapsed_since_wait_started={:?}",
                case.elapsed_since_wait_started
            );
        }
    }
}
