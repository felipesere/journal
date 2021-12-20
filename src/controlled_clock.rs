#![allow(dead_code)]

use crate::Clock;
use anyhow::Result;
use std::ops::Add;
use time::{Date, Duration, Month, Weekday};

pub(crate) struct ControlledClock {
    current_date: Date,
}

impl Clock for ControlledClock {
    fn today(&self) -> Date {
        self.current_date
    }
}

impl ControlledClock {
    pub(crate) fn new(year: i32, month: Month, day: u8) -> Result<ControlledClock> {
        let current_date = Date::from_calendar_date(year, month, day)?;
        Ok(Self { current_date })
    }

    pub(crate) fn after(&self, duration: Duration) -> Date {
        assert!(duration.is_positive());
        self.current_date.add(duration)
    }

    pub(crate) fn advance_by(&mut self, days: Duration) {
        self.current_date = self.current_date.add(days);
    }

    pub(crate) fn advance_to(&mut self, weekday: Weekday) {
        self.current_date = next_weekday(self.current_date, weekday);
    }
}

fn next_weekday(date: Date, weekday: Weekday) -> Date {
    let mut next = date;
    loop {
        if next.weekday() == weekday {
            break;
        }

        next = next.next_day().unwrap();
    }
    next
}
