use std::collections::BTreeMap;
use std::fmt::Display;
use std::num::ParseIntError;
use std::ops::Mul;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use time::{Date, Month, OffsetDateTime, Weekday};

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
    pub location: PathBuf,
}

#[derive(Deserialize, Serialize)]
pub struct Reminders {
    dated: BTreeMap<Date, Vec<String>>,
    intervals: Vec<RepeatingReminder>,
}

#[derive(Deserialize, Serialize)]
struct RepeatingReminder {
    start: Date,
    interval: RepeatingDate,
    reminder: String,
}

impl Reminders {
    pub fn new() -> Self {
        Self {
            dated: BTreeMap::new(),
            intervals: Vec::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read(path)
            .with_context(|| format!("Could not load reminders from {:?}", path))?;
        serde_json::from_slice(&content).map_err(|e| anyhow!(e)).context("Could not read structure in file")
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        tracing::info!("Saving reminders to {}", path.to_string_lossy());
        let mut reminders_file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(path)
            .context("Opening reminders file to write")?;
        serde_json::to_writer_pretty(&mut reminders_file, &self).map_err(|e| anyhow!(e))
    }

    pub fn on_date<S: Into<String>>(&mut self, date: Date, reminder: S) {
        self.dated.entry(date).or_default().push(reminder.into());
    }

    pub fn for_today(&self, clock: &impl Clock) -> Vec<String> {
        let today = clock.today();

        let mut reminders = Vec::new();

        if let Some(dated_reminders) = self.dated.get(&today) {
            reminders.extend_from_slice(dated_reminders)
        }

        for repeating_reminder in &self.intervals {
            match &repeating_reminder.interval {
                RepeatingDate::Weekday(weekday) => {
                    if today.weekday() == *weekday {
                        reminders.push(repeating_reminder.reminder.clone());
                    }
                }
                RepeatingDate::Periodic { amount, period } => {
                    let interval_in_days = amount * period;
                    let difference =
                        today.to_julian_day() - repeating_reminder.start.to_julian_day();

                    if difference % interval_in_days == 0 {
                        reminders.push(repeating_reminder.reminder.clone());
                    }
                }
            }
        }

        reminders
    }

    pub fn every(&mut self, clock: &impl Clock, interval: &RepeatingDate, reminder: &str) {
        let start = clock.today();
        self.intervals.push(RepeatingReminder {
            start,
            interval: interval.clone(),
            reminder: reminder.to_string(),
        });
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum SpecificDate {
    Next(Weekday),
    OnDate(Date),
    OnDayMonth(u8, Month),
}

impl SpecificDate {
    pub fn next_date(self, base: Date) -> Date {
        match self {
            Self::OnDate(date) => date,
            Self::OnDayMonth(day, month) => {
                Date::from_calendar_date(base.year(), month, day).expect("Day should have existed")
            }
            Self::Next(weekday) => {
                let mut next = base;
                loop {
                    if next.weekday() == weekday {
                        return next;
                    }

                    next = next.next_day().unwrap();
                }
            }
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
                "No matching date format found. Use day.month or day.monty.year or weekday.".to_string(),
            ),
        }
    }
}

#[rustfmt::skip]
fn parse_weekday(s: &str) -> Result<Weekday, String> {
    let day = match s {
        "Monday"    | "monday"   => Weekday::Monday,
        "Tuesday"   | "tuesday"  => Weekday::Tuesday,
        "Wednesday" | "wedneday" => Weekday::Wednesday,
        "Thursday"  | "thursday" => Weekday::Thursday,
        "Friday"    | "friday"   => Weekday::Friday,
        "Saturday"  | "saturday" => Weekday::Saturday,
        "Sunday"    | "sunday"   => Weekday::Sunday,
        _ => return Err(format!("No matching day of the week: {}", s)),
    };

    Ok(day)
}

#[rustfmt::skip]
fn parse_month(month: &str) -> Result<Month, String> {
    let month = match month {
        "January"   | "Jan" | "january"   | "jan" => Month::January,
        "February"  | "Feb" | "february"  | "feb" => Month::February,
        "March"     | "Mar" | "march"     | "mar" => Month::March,
        "April"     | "Apr" | "april"     | "apr" => Month::April,
        "May"                             | "may" => Month::May,
        "June"      | "Jun" | "june"      | "jun" => Month::June,
        "July"      | "Jul" | "july"      | "jul" => Month::July,
        "August"    | "Aug" | "august"    | "aug" => Month::August,
        "September" | "Sep" | "september" | "sep" => Month::September,
        "October"   | "Oct" | "october"   | "oct" => Month::October,
        "November"  | "Nov" | "november"  | "nov" => Month::November,
        "December"  | "Dec" | "december"  | "dec" => Month::December,
        _ => return Err(format!("No matching month name: {}", month)),
    };

    Ok(month)
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
            RepeatingDate::Periodic { amount, period } => write!(f, "{} {:?}", amount, period),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum Period {
    Days,
    Weeks,
    Months,
}

impl Mul<&Period> for &usize {
    type Output = i32;

    fn mul(self, rhs: &Period) -> Self::Output {
        let rhs = match rhs {
            Period::Days => 1,
            Period::Weeks => 7,
            Period::Months => 30, // TODO: indicator that is clearly not OK with months...
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
                "months" => Period::Months,
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
    use indoc::indoc;
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
            loop {
                if self.current_date.weekday() == weekday {
                    break;
                }
                self.current_date = self.current_date.next_day().unwrap();
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
        reminders.every(
            &clock,
            &RepeatingDate::Periodic {
                amount: 2,
                period: Period::Weeks,
            },
            "Second task",
        );

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
            .write_str(r#"{"dated": {}, "intervals": [] }"#)
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
            - n_months ("3.months", Ok(super::RepeatingDate::Periodic{amount: 3, period: super::Period::Months}))
            - negative_amount ("-1.months", Err("invalid digit found in string".into()))
            - unknown_period ("1.fortnights", Err("unknown period: fortnights".into()))
            - missing_separator ("quaselgoop", Err("Unrecognized format for repeating date: quaselgoop".into()))
        }
    }

    mod config {
        use super::*;

        #[test]
        fn parse_config() -> Result<()> {
            let input = indoc! { r#"
            location: path/to/dir
            "#
            };

            let config: ReminderConfig = serde_yaml::from_str(&input).unwrap();

            assert_eq!(config.location, PathBuf::from("path/to/dir"));

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
