use std::collections::BTreeMap;

use time::Date;

trait Clock {
    fn today(&self) -> Date;
}

struct Reminders {
    dated: BTreeMap<Date, Vec<String>>,
}

impl Reminders {
    fn load() -> Self {
        Self {
            dated: BTreeMap::new(),
        }
    }

    fn on_date<S: Into<String>>(&mut self, date: Date, reminder: S) {
        self.dated.entry(date).or_default().push(reminder.into());
    }

    fn for_today(&self, clock: &impl Clock) -> Vec<String> {
        let today = clock.today();

        if let Some(reminders) = self.dated.get(&today) {
            reminders.to_vec()
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ops::Add;

    use anyhow::Result;
    use time::Duration;
    use time::{ext::NumericalDuration, Date, Month, Month::*};

    struct ControlledClock {
        current_date: Date,
    }

    impl Clock for ControlledClock {
        fn today(&self) -> Date {
            self.current_date.clone()
        }
    }

    impl ControlledClock {
        fn new(year: i32, month: Month, day: u8) -> Result<ControlledClock> {
            let current_date = Date::from_calendar_date(year, month, day)?;
            Ok(Self { current_date })
        }

        fn after(&self, duration: Duration) -> Date {
            assert!(duration.is_positive());
            self.current_date.add(duration)
        }

        pub(crate) fn advance_by(&mut self, days: Duration) {
            self.current_date = self.current_date.add(days);
        }
    }

    #[test]
    fn large_integration_test() -> Result<()> {
        let mut clock = ControlledClock::new(2021, July, 15)?;
        let mut reminders = Reminders::load(); // maybe something about files here?

        reminders.on_date(clock.after(3.days()), "Email someone");

        let todays_reminders = reminders.for_today(&clock);
        assert!(todays_reminders.is_empty());

        clock.advance_by(3.days());

        let todays_reminders = reminders.for_today(&clock);
        assert_eq!(todays_reminders, vec!["Email someone".to_string()]);

        clock.advance_by(1.days());
        let todays_reminders = reminders.for_today(&clock);
        assert!(todays_reminders.is_empty());

        Ok(())
    }
}
