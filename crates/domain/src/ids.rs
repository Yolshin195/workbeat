/// Идентификаторы сущностей (`INTEGER PRIMARY KEY` в SQLite). Простые
/// newtype-обёртки над `i64` — без бизнес-логики, только для типобезопасности
/// на границах между сущностями.
macro_rules! entity_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(i64);

        impl $name {
            pub fn new(value: i64) -> Self {
                Self(value)
            }

            pub fn value(&self) -> i64 {
                self.0
            }
        }

        impl From<i64> for $name {
            fn from(value: i64) -> Self {
                Self::new(value)
            }
        }
    };
}

entity_id!(TaskId);
entity_id!(WorkDayId);
entity_id!(HourIntervalId);
entity_id!(TenMinCheckId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_roundtrip_their_value() {
        assert_eq!(TaskId::new(7).value(), 7);
        assert_eq!(WorkDayId::from(3).value(), 3);
        assert_eq!(HourIntervalId::new(1).value(), 1);
        assert_eq!(TenMinCheckId::new(9).value(), 9);
    }
}
