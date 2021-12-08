use std::collections::BTreeMap;
use std::num::ParseIntError;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use time::{format_description, Date, Month, OffsetDateTime, Weekday};

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
}

impl Reminders {
    pub fn new() -> Self {
        Self {
            dated: BTreeMap::new(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read(path)
            .with_context(|| format!("Could not load reminders from {:?}", path))?;
        serde_json::from_slice(&content).map_err(|e| anyhow!(e))
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

        if let Some(reminders) = self.dated.get(&today) {
            reminders.to_vec()
        } else {
            Vec::new()
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
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
        if let Ok(result) = parse_day_month_year(s) {
            return Ok(result);
        }

        if let Ok(result) = parse_day_month(s) {
            return Ok(result);
        }

        parse_weekday(s).map(|day| SpecificDate::Next(day))
    }
}

fn parse_day_month_year(s: &str) -> Result<SpecificDate, String> {
    let format = format_description::parse("[day].[month repr:short].[year]").unwrap();

    let date = Date::parse(s, &format).map_err(|e| e.to_string())?;

    Ok(SpecificDate::OnDate(date))
}

fn parse_weekday(s: &str) -> Result<Weekday, String> {
    let day = match s {
        "Monday" | "monday" => Weekday::Monday,
        "Tuesday" | "tuesday" => Weekday::Tuesday,
        "Wednesday" | "wedneday" => Weekday::Wednesday,
        "Thursday" | "thursday" => Weekday::Thursday,
        "Friday" | "friday" => Weekday::Friday,
        "Saturday" | "saturday" => Weekday::Saturday,
        "Sunday" | "sunday" => Weekday::Sunday,
        _ => return Err(format!("No matching day of the week: {}", s)),
    };

    Ok(day)
}

fn parse_day_month(s: &str) -> Result<SpecificDate, String> {
    use time::format_description::modifier::{Day, Month, MonthRepr, Padding};
    use time::format_description::Component;
    use time::format_description::FormatItem::*;

    let mut month = Month::default();
    month.repr = MonthRepr::Short;
    month.padding = Padding::None;
    month.case_sensitive = false;

    let mut day = Day::default();
    day.padding = Padding::None;

    let mut parsed = time::parsing::Parsed::new();
    let structure_to_parse = vec![
        Component(Component::Day(day)),
        Literal(&[b'.']),
        Component(Component::Month(month)),
    ];

    parsed
        .parse_items(s.as_bytes(), &structure_to_parse)
        .map_err(|e| e.to_string())?;

    let day = parsed
        .day()
        .ok_or_else(|| format!("Could get month component from '{}'", &s))?;
    let month = parsed
        .month()
        .ok_or_else(|| format!("Could get date from '{}'", &s))?;

    Ok(SpecificDate::OnDayMonth(day.get(), month))
}

#[derive(Debug, PartialEq, Eq)]
pub enum RepeatingDate {
    Weekday(Weekday),
    Periodic(usize, Period),
}

#[derive(Debug, PartialEq, Eq)]
pub enum Period {
    Days,
    Weeks,
    Months,
}

impl FromStr for RepeatingDate {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed = parse_weekday(s).map(|day| RepeatingDate::Weekday(day));
        if parsed.is_ok() {
            return parsed;
        }

        if let Some((digits, period)) = s.split_once('.') {
            let amount = str::parse(&digits).map_err(|e: ParseIntError| e.to_string())?;
            let period = match period {
                "days" => Period::Days,
                "weeks" => Period::Weeks,
                "months" => Period::Months,
                _ => return Err(format!("unknown period: {}", period)),
            };

            return Ok(RepeatingDate::Periodic(amount, period));
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
    fn adding_multiple_reminders_on_filesystem() -> Result<()> {
        let mut clock = ControlledClock::new(2021, July, 15)?;

        let dir = TempDir::new().unwrap();
        dir.child("reminders.json")
            .write_str(r#"{"dated": {}}"#)
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
        use time::{macros::date, Weekday};

        data_test! {

            fn parses_date(input, expected) => {
                use super::*;

                assert_eq!(RepeatingDate::from_str(input), expected);
            }
            - weekday ("Wednesday", Ok(super::RepeatingDate::Weekday(super::Weekday::Wednesday)))
            - n_days ("2.days", Ok(super::RepeatingDate::Periodic(2, super::Period::Days)))
            - n_weeks ("7.weeks", Ok(super::RepeatingDate::Periodic(7, super::Period::Weeks)))
            - n_months ("3.months", Ok(super::RepeatingDate::Periodic(3, super::Period::Months)))
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
