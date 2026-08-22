/// После скольких закрытых часовых интервалов бот сам предлагает пойти на
/// обед (README.md раздел 1: "Также после 4-го интервала бот сам предлагает
/// пойти на обед").
pub const LUNCH_SUGGESTION_AFTER_INTERVALS: u8 = 4;

/// Чистая функция для `SuggestLunchAfterNthInterval`: срабатывает ровно на
/// счётчике закрытых интервалов, равном `LUNCH_SUGGESTION_AFTER_INTERVALS` —
/// не раньше и не позже, независимо от того, сколько интервалов будет закрыто
/// дальше (предложение не повторяется на 5-м, 6-м и т.д.).
pub struct LunchSuggestionDecision;

impl LunchSuggestionDecision {
    pub fn decide(closed_intervals: u8) -> bool {
        closed_intervals == LUNCH_SUGGESTION_AFTER_INTERVALS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggests_exactly_on_the_fourth_closed_interval() {
        assert!(!LunchSuggestionDecision::decide(3));
        assert!(LunchSuggestionDecision::decide(4));
        assert!(!LunchSuggestionDecision::decide(5));
    }

    #[test]
    fn does_not_suggest_again_later_in_the_day() {
        for closed in [6u8, 7, 8, 9] {
            assert!(!LunchSuggestionDecision::decide(closed));
        }
    }
}
