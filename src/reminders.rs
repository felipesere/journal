use std::fmt::Display;
use std::num::ParseIntError;
use std::ops::Mul;
use std::path::Path;
use std::str::FromStr;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use time::{format_description, Date, Month, OffsetDateTime, Weekday};

trait WeekdayExt {
    fn next(&self, weekday: Weekday) -> Date;
}

impl WeekdayExt for Date {
    fn next(&self, weekday: Weekday) -> Date {
        let mut next = *self;
        loop {
            if next.weekday() == weekday {
                break;
            }

            next = next.next_day().unwrap();
        }
        next
    }
}

pub trait Clock {
    fn today(&self) -> Date;
}

pub struct WallClock;

impl Clock for WallClock {
    fn today(&self) -> Date {
        OffsetDateTime::now_utc().date()
    }
}

#[derive(Deserialize)]
pub struct ReminderConfig {
    pub enabled: bool,
}

#[derive(Deserialize, Serialize)]
enum InnerReminder {
    Concrete(Date, String),
    Recurring {
        start: Date,
        interval: RepeatingDate,
        reminder: String,
    },
}

#[derive(Deserialize, Serialize)]
pub struct Reminders {
    stored: Vec<InnerReminder>,
}

impl Reminders {
    #[cfg(test)]
    pub fn new() -> Self {
        Self { stored: Vec::new() }
    }

    #[tracing::instrument(err, name = "Loading reminders from disk")]
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read(path)
            .with_context(|| format!("Could not load reminders from {:?}", path))?;

        let reminders = serde_json::from_slice(&content)
            .map_err(|e| anyhow!(e))
            .context("Could not read structure in file")?;

        tracing::info!("Loaded reminders");
        Ok(reminders)
    }

    #[tracing::instrument(err, name = "Saving reminders to disk", skip(self))]
    pub fn save(&self, path: &Path) -> Result<()> {
        tracing::info!("Saving reminders to {}", path.to_string_lossy());
        let mut reminders_file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .context("Opening reminders file to write")?;

        let _ = serde_json::to_writer_pretty(&mut reminders_file, &self).map_err(|e| anyhow!(e))?;
        tracing::info!("Saved reminders");
        Ok(())
    }

    pub fn on_date<S: Into<String>>(&mut self, date: Date, reminder: S) {
        self.stored
            .push(InnerReminder::Concrete(date, reminder.into()));
    }

    pub fn every(&mut self, clock: &impl Clock, interval: &RepeatingDate, reminder: &str) {
        let start = clock.today();
        self.stored.push(InnerReminder::Recurring {
            start,
            interval: interval.clone(),
            reminder: reminder.to_string(),
        });
    }

    #[tracing::instrument(name = "Loading todays reminders", skip(self, clock))]
    pub fn for_today(&self, clock: &impl Clock) -> Vec<String> {
        let today = clock.today();

        let mut reminders = Vec::new();

        for reminder in &self.stored {
            match reminder {
                InnerReminder::Concrete(date, reminder) => {
                    if today == *date {
                        reminders.push(reminder.clone());
                    }
                }
                InnerReminder::Recurring {
                    start,
                    interval,
                    reminder,
                } => match interval {
                    RepeatingDate::Weekday(weekday) => {
                        if today.weekday() == *weekday {
                            reminders.push(reminder.clone());
                        }
                    }
                    RepeatingDate::Periodic { amount, period } => {
                        let interval_in_days = amount * period;
                        let difference = today.to_julian_day() - start.to_julian_day();

                        if difference % interval_in_days == 0 {
                            reminders.push(reminder.clone());
                        }
                    }
                },
            }
        }

        reminders
    }

    pub fn all(&self) -> Vec<Reminder> {
        let mut nr = 1;
        let mut result = Vec::new();
        for reminder in &self.stored {
            match reminder {
                InnerReminder::Concrete(date, reminder) => {
                    result.push(Reminder {
                        nr,
                        date: DateRepr::Exact(*date),
                        reminder: reminder.to_string(),
                    });
                }
                InnerReminder::Recurring {
                    interval, reminder, ..
                } => {
                    result.push(Reminder {
                        nr,
                        date: DateRepr::Repeating(interval.clone()),
                        reminder: reminder.to_string(),
                    });
                }
            }
            nr += 1;
        }

        result
    }

    #[tracing::instrument(skip(self))]
    pub fn delete(&mut self, nr: u32) -> Result<()> {
        let nr = (nr - 1) as usize;
        if nr < self.stored.len() {
            self.stored.remove(nr);
            Ok(())
        } else {
            bail!("There is no reminder '{}'", (nr + 1));
        }
    }
}

pub enum DateRepr {
    Exact(Date),
    Repeating(RepeatingDate),
}

impl Display for DateRepr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DateRepr::Exact(date) => {
                let format = format_description::parse("[year]-[month]-[day]").unwrap();
                write!(f, "{}", date.format(&format).unwrap())
            }
            DateRepr::Repeating(repeating) => repeating.fmt(f),
        }
    }
}

pub struct Reminder {
    pub nr: usize,
    pub date: DateRepr,
    pub reminder: String,
}

#[derive(Debug, Eq, PartialEq)]
pub enum SpecificDate {
    Next(Weekday),
    OnDate(Date),
    OnDayMonth(u8, Month),
}

impl SpecificDate {
    pub fn next_date(self, current: Date) -> Date {
        match self {
            Self::OnDate(date) => date,
            Self::OnDayMonth(day, month) => Date::from_calendar_date(current.year(), month, day)
                .expect("Day should have existed"),
            Self::Next(weekday) => current.next(weekday),
        }
    }
}

impl FromStr for SpecificDate {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let components: Vec<&str> = s.split('.').collect();

        match &components[..] {
            [day, month, year] => {
                let day: u8 = str::parse(day).map_err(|e: ParseIntError| e.to_string())?;
                let month = parse_month(month)?;
                let year: i32 = str::parse(year).map_err(|e: ParseIntError| e.to_string())?;
                Ok(SpecificDate::OnDate(
                    Date::from_calendar_date(year, month, day).map_err(|e| e.to_string())?,
                ))
            }
            [day, month] => {
                let day: u8 = str::parse(day).map_err(|e: ParseIntError| e.to_string())?;
                let month = parse_month(month)?;
                Ok(SpecificDate::OnDayMonth(day, month))
            }
            [weekday] => {
                let weekday = parse_weekday(weekday)?;
                Ok(SpecificDate::Next(weekday))
            }
            _ => Err(
                "No matching date format found. Use day.month or day.monty.year or weekday."
                    .to_string(),
            ),
        }
    }
}

#[rustfmt::skip]
fn parse_weekday(s: &str) -> Result<Weekday, String> {
    match s {
        "Monday"    | "monday"   => Ok(Weekday::Monday),
        "Tuesday"   | "tuesday"  => Ok(Weekday::Tuesday),
        "Wednesday" | "wedneday" => Ok(Weekday::Wednesday),
        "Thursday"  | "thursday" => Ok(Weekday::Thursday),
        "Friday"    | "friday"   => Ok(Weekday::Friday),
        "Saturday"  | "saturday" => Ok(Weekday::Saturday),
        "Sunday"    | "sunday"   => Ok(Weekday::Sunday),
        _ => Err(format!("No matching day of the week: {}", s)),
    }
}

#[rustfmt::skip]
fn parse_month(month: &str) -> Result<Month, String> {
    match month {
        "January"   | "Jan" | "january"   | "jan" => Ok(Month::January),
        "February"  | "Feb" | "february"  | "feb" => Ok(Month::February),
        "March"     | "Mar" | "march"     | "mar" => Ok(Month::March),
        "April"     | "Apr" | "april"     | "apr" => Ok(Month::April),
        "May"                             | "may" => Ok(Month::May),
        "June"      | "Jun" | "june"      | "jun" => Ok(Month::June),
        "July"      | "Jul" | "july"      | "jul" => Ok(Month::July),
        "August"    | "Aug" | "august"    | "aug" => Ok(Month::August),
        "September" | "Sep" | "september" | "sep" => Ok(Month::September),
        "October"   | "Oct" | "october"   | "oct" => Ok(Month::October),
        "November"  | "Nov" | "november"  | "nov" => Ok(Month::November),
        "December"  | "Dec" | "december"  | "dec" => Ok(Month::December),
        _ => Err(format!("No matching month name: {}", month)),
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RepeatingDate {
    Weekday(Weekday),
    Periodic { amount: usize, period: Period },
}

impl Display for RepeatingDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepeatingDate::Weekday(weekday) => write!(f, "{}", weekday),
            RepeatingDate::Periodic { amount, period } => {
                write!(f, "every {} {:?}", amount, period)
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Period {
    Days,
    Weeks,
}

impl Mul<&Period> for &usize {
    type Output = i32;

    fn mul(self, rhs: &Period) -> Self::Output {
        let rhs = match rhs {
            Period::Days => 1,
            Period::Weeks => 7,
        };

        (*self as i32) * rhs
    }
}

impl FromStr for RepeatingDate {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed = parse_weekday(s).map(RepeatingDate::Weekday);
        if parsed.is_ok() {
            return parsed;
        }

        if let Some((digits, period)) = s.split_once('.') {
            let amount = str::parse(digits).map_err(|e: ParseIntError| e.to_string())?;
            let period = match period {
                "days" => Period::Days,
                "weeks" => Period::Weeks,
                _ => return Err(format!("unknown period: {}", period)),
            };

            return Ok(RepeatingDate::Periodic { amount, period });
        }

        Err(format!("Unrecognized format for repeating date: {}", s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ops::Add;

    use anyhow::Result;
    use assert_fs::{prelude::*, TempDir};
    use time::{ext::NumericalDuration, macros::date, Date, Duration, Month, Month::*};

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

        pub(crate) fn advance_to(&mut self, weekday: Weekday) {
            self.current_date = self.current_date.next(weekday);
        }
    }

    // the names had to be different to not clash with time-rs
    trait PeriodicExt {
        fn daily(self) -> RepeatingDate;
        fn weekly(self) -> RepeatingDate;
    }

    impl PeriodicExt for usize {
        fn daily(self) -> RepeatingDate {
            RepeatingDate::Periodic {
                amount: self,
                period: Period::Days,
            }
        }

        fn weekly(self) -> RepeatingDate {
            RepeatingDate::Periodic {
                amount: self,
                period: Period::Weeks,
            }
        }
    }

    #[test]
    fn large_in_memory_test() -> Result<()> {
        let mut clock = ControlledClock::new(2021, July, 15)?;
        let mut reminders = Reminders::new();

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

    #[test]
    fn repeating_reminders() -> Result<()> {
        use time::Weekday::*;
        let mut clock = ControlledClock::new(2021, July, 15)?;
        let mut reminders = Reminders::new();

        clock.advance_to(Monday);
        reminders.every(&clock, &RepeatingDate::Weekday(Wednesday), "Email someone");

        clock.advance_to(Wednesday);
        let todays_reminders = reminders.for_today(&clock);
        assert_eq!(todays_reminders, vec!["Email someone".to_string()]);

        clock.advance_by(1.days()); // Thursday
        reminders.every(&clock, &2.weekly(), "Second task");

        clock.advance_by(1.weeks()); // next Thursday
        let todays_reminders = reminders.for_today(&clock);
        assert!(todays_reminders.is_empty());

        clock.advance_by(1.weeks()); // Thursday after that...
        let todays_reminders = reminders.for_today(&clock);
        assert_eq!(todays_reminders, vec!["Second task".to_string()]);

        Ok(())
    }

    #[test]
    fn adding_multiple_reminders_on_filesystem() -> Result<()> {
        let mut clock = ControlledClock::new(2021, July, 15)?;

        let dir = TempDir::new().unwrap();
        dir.child("reminders.json")
            .write_str(r#"{"stored": [] }"#)
            .unwrap();

        let mut reminders = Reminders::load(&dir.path().join("reminders.json"))?;

        reminders.on_date(clock.after(3.days()), "First task");
        reminders.on_date(clock.after(4.days()), "Second task");
        reminders.on_date(clock.after(4.days()), "Third task");

        let todays_reminders = reminders.for_today(&clock);
        assert!(todays_reminders.is_empty());

        clock.advance_by(3.days());

        let todays_reminders = reminders.for_today(&clock);
        assert_eq!(todays_reminders, vec!["First task".to_string()]);

        clock.advance_by(1.days());
        let todays_reminders = reminders.for_today(&clock);
        assert_eq!(
            todays_reminders,
            vec!["Second task".to_string(), "Third task".to_string()]
        );

        clock.advance_by(1.days());
        let todays_reminders = reminders.for_today(&clock);
        assert!(todays_reminders.is_empty());

        Ok(())
    }

    #[test]
    fn lists_all_currently_tracked_reminders() -> Result<()> {
        // ..event past ones!
        use time::Weekday::*;
        let mut clock = ControlledClock::new(2021, July, 15)?;
        let mut reminders = Reminders::new();

        clock.advance_to(Monday);
        reminders.every(&clock, &RepeatingDate::Weekday(Wednesday), "One");
        reminders.every(&clock, &2.weekly(), "Two");
        reminders.on_date(clock.after(3.days()), "Three");
        reminders.on_date(clock.after(4.days()), "Four");
        reminders.on_date(clock.after(4.days()), "Five");

        assert_eq!(reminders.all().len(), 5);

        Ok(())
    }

    #[test]
    fn can_delete_reminders() -> Result<()> {
        use time::Weekday::*;
        let mut clock = ControlledClock::new(2021, July, 15)?;
        let mut reminders = Reminders::new();

        clock.advance_to(Monday);
        reminders.every(&clock, &RepeatingDate::Weekday(Wednesday), "One");
        reminders.every(&clock, &2.weekly(), "Two");
        reminders.on_date(clock.after(3.days()), "Three");
        reminders.on_date(clock.after(4.days()), "Four");
        reminders.on_date(clock.after(4.days()), "Five");

        assert_eq!(reminders.all().len(), 5);

        reminders.delete(3)?; // should be the "Three"
        assert_eq!(reminders.all().len(), 4);

        let exissting_remindests = reminders
            .all()
            .into_iter()
            .map(|reminders| reminders.reminder)
            .collect::<Vec<_>>();

        assert_eq!(
            exissting_remindests,
            &["One", "Two", /* deleted: Three */ "Four", "Five"]
        );

        Ok(())
    }

    #[test]
    fn reports_when_the_number_to_delete_is_out_of_range() -> Result<()> {
        let clock = ControlledClock::new(2021, July, 15)?;
        let mut reminders = Reminders::new();
        reminders.on_date(clock.today(), "Awesome");
        let result = reminders.delete(3);

        let err = result.unwrap_err();
        assert_eq!(err.to_string(), "There is no reminder '3'");
        Ok(())
    }

    mod parsing_specific_date {
        use super::*;
        use data_test::data_test;
        use std::str::FromStr;
        use time::{macros::date, Weekday};

        data_test! {

            fn parses_date(input, expected) => {
                use super::*;

                assert_eq!(SpecificDate::from_str(input).unwrap(), expected);
            }
            - day_month ("12.Feb",           super::SpecificDate::OnDayMonth(12, time::Month::February))
            - day_month_long ("12.February", super::SpecificDate::OnDayMonth(12, time::Month::February))
            - short_day_month ("2.Feb",      super::SpecificDate::OnDayMonth(2, time::Month::February))
            - day_month_year ("15.Jan.2022", super::SpecificDate::OnDate(super::date! (2022 - 01 - 15)))
            - weekday ("Wednesday",          super::SpecificDate::Next(super::Weekday::Wednesday))
        }
    }

    mod parsing_repeating_date {
        use super::*;
        use data_test::data_test;
        use std::str::FromStr;
        use time::Weekday;

        data_test! {

            fn parses_date(input, expected) => {
                use super::*;

                assert_eq!(RepeatingDate::from_str(input), expected);
            }
            - weekday ("Wednesday", Ok(super::RepeatingDate::Weekday(super::Weekday::Wednesday)))
            - n_days ("2.days", Ok(super::RepeatingDate::Periodic{amount: 2, period: super::Period::Days}))
            - n_weeks ("7.weeks", Ok(super::RepeatingDate::Periodic{amount: 7, period: super::Period::Weeks}))
            - negative_amount ("-1.months", Err("invalid digit found in string".into()))
            - unknown_period ("1.fortnights", Err("unknown period: fortnights".into()))
            - missing_separator ("quaselgoop", Err("Unrecognized format for repeating date: quaselgoop".into()))
        }
    }

    mod config {
        use super::*;

        #[test]
        fn parse_config() -> Result<()> {
            let input = r#"enabled: true"#;

            let config: ReminderConfig = serde_yaml::from_str(&input).unwrap();

            assert!(config.enabled);

            Ok(())
        }
    }

    mod specific_date {
        use super::*;

        #[test]
        fn specifics_dates_are_their_own_next_date() {
            let jan_15_2022 = date!(2022 - 01 - 15);
            let specific_date = SpecificDate::OnDate(jan_15_2022.clone());

            let next_date = specific_date.next_date(date!(2022 - 01 - 10));

            assert_eq!(jan_15_2022, next_date);
        }

        #[test]
        fn day_month_dates_use_year_of_item_if_possible() {
            let specific_date = SpecificDate::OnDayMonth(9, Month::December);

            let dez_7_2021 = date!(2021 - 12 - 07);
            let next_date = specific_date.next_date(dez_7_2021);

            assert_eq!(date!(2021 - 12 - 09), next_date);
        }

        #[test]
        fn weekday_picks_next_available_weekday() {
            let specific_date = SpecificDate::Next(Weekday::Wednesday);

            let dez_7_2021 = date!(2021 - 12 - 07);
            let next_date = specific_date.next_date(dez_7_2021);

            assert_eq!(date!(2021 - 12 - 08), next_date);
        }
    }
}
