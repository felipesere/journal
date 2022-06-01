use std::fmt::Display;
use std::num::ParseIntError;
use std::ops::Mul;
use std::path::Path;
use std::str::FromStr;

use anyhow::{anyhow, bail, Context, Result};
use clap::StructOpt;
use serde::{Deserialize, Serialize};
use tabled::object::Segment;
use time::format_description::FormatItem;
use time::{format_description, Date, Month, OffsetDateTime, Weekday};

use handlebars::Handlebars;
use tabled::{Alignment, Modify, Style, Table, Tabled};

use crate::config::Section;
use crate::{storage::Journal, Config};

const YEAR_MONTH_DAY: &[FormatItem] = time::macros::format_description!("[year]-[month]-[day]");

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

pub trait Clock: Sync {
    fn today(&self) -> Date;
}

pub struct WallClock;

impl Clock for WallClock {
    fn today(&self) -> Date {
        OffsetDateTime::now_utc().date()
    }
}

const REMIDNERS: &str = r#"
## Your reminders for today:
{{#each reminders as | reminder | }}
* [ ] {{ reminder }}
{{/each }}

"#;

#[derive(Deserialize, Serialize, Clone)]
pub struct ReminderConfig {
    #[serde(default = "default_reminders_template")]
    pub template: String,
}

fn default_reminders_template() -> String {
    REMIDNERS.to_string()
}

impl Default for ReminderConfig {
    fn default() -> Self {
        Self {
            template: default_reminders_template(),
        }
    }
}

#[async_trait::async_trait]
impl Section for ReminderConfig {
    async fn render(&self, journal: &Journal, clock: &dyn Clock) -> Result<String> {
        let location = journal.child_file("reminders.json");
        let reminders = Reminders::load(&location)?;

        let todays_reminders = reminders.for_today(clock);

        #[derive(Serialize)]
        struct C {
            reminders: Vec<String>,
        }

        let mut tt = Handlebars::new();
        tt.register_template_string("reminders", self.template.to_string())?;
        tt.register_escape_fn(handlebars::no_escape);
        tt.render(
            "reminders",
            &C {
                reminders: todays_reminders,
            },
        )
        .map_err(|e| e.into())
    }
}

#[derive(Debug, StructOpt)]
#[clap(alias = "reminders")]
pub enum ReminderCmd {
    /// Add a new reminder, either on a specific date or recurring.
    New {
        #[clap(long = "on", group = "date_selection")]
        on_date: Option<SpecificDate>,

        #[clap(long = "every", group = "date_selection")]
        every: Option<RepeatingDate>,

        #[clap(takes_value(true))]
        reminder: String,
    },
    /// List all existing reminders
    List,
    /// Delete a reminder
    Delete {
        /// The number to delete
        nr: u32,
    },
}

impl ReminderCmd {
    pub(crate) fn execute(self, config: &Config, clock: &impl Clock) -> Result<()> {
        let location = config.dir.join("reminders.json");
        let mut reminders_storage = Reminders::load(&location)?;

        match self {
            ReminderCmd::Delete { nr } => {
                tracing::info!("intention to delete reminder");

                reminders_storage.delete(nr)?;

                println!("Deleted {}", nr,);
            }
            ReminderCmd::List => {
                tracing::info!("intention to list reminders");

                let data = reminders_storage.all();
                let table = Table::new(&data)
                    .with(Style::modern())
                    .with(Modify::new(Segment::all()).with(Alignment::left()));

                println!("{}", table);
            }
            ReminderCmd::New {
                on_date: specific_date_spec,
                every: interval_spec,
                reminder,
            } => {
                tracing::info!("intention to create a new reminder");

                if let Some(date_spec) = specific_date_spec {
                    let next = date_spec.next_date(clock.today());

                    reminders_storage.on_date(next, reminder.clone());

                    println!(
                        "Added a reminder for '{}' on '{}'",
                        reminder,
                        next.format(YEAR_MONTH_DAY)?
                    );
                }

                if let Some(interval_spec) = interval_spec {
                    reminders_storage.every(clock, &interval_spec, &reminder);

                    println!(
                        "Added a reminder for '{}' every '{}'",
                        reminder, interval_spec,
                    );
                }
            }
        }

        reminders_storage
            .save(&location)
            .context("Failed to save reminders")?;

        tracing::info!("Saved reminders");

        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
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

        serde_json::to_writer_pretty(&mut reminders_file, &self).map_err(|e| anyhow!(e))?;
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
    pub fn for_today(&self, clock: &dyn Clock) -> Vec<String> {
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
                    let format = format_description::parse("[year]-[month]-[day]").unwrap();
                    result.push(Reminder {
                        nr,
                        date: date.format(&format).unwrap(),
                        reminder: reminder.to_string(),
                    });
                }
                InnerReminder::Recurring {
                    interval, reminder, ..
                } => {
                    result.push(Reminder {
                        nr,
                        date: interval.to_string(),
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

#[derive(Tabled)]
pub struct Reminder {
    pub nr: usize,
    pub date: String,
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
        "Monday"    | "Mon" | "monday"   | "mon" => Ok(Weekday::Monday),
        "Tuesday"   | "Tue" | "tuesday"  | "tue" => Ok(Weekday::Tuesday),
        "Wednesday" | "Wed" | "wedneday" | "wed" => Ok(Weekday::Wednesday),
        "Thursday"  | "Thu" | "thursday" | "thu" => Ok(Weekday::Thursday),
        "Friday"    | "Fri" | "friday"   | "fri" => Ok(Weekday::Friday),
        "Saturday"  | "Sat" | "saturday" | "sat" => Ok(Weekday::Saturday),
        "Sunday"    | "Sun" | "sunday"   | "sun" => Ok(Weekday::Sunday),
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
#[serde(rename_all = "lowercase")]
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
#[path = "controlled_clock.rs"]
mod controlled_clock;

#[cfg(test)]
mod tests {
    use super::controlled_clock::ControlledClock;
    use super::*;

    use anyhow::Result;
    use assert_fs::{prelude::*, TempDir};
    use time::{ext::NumericalDuration, macros::date, Month, Month::*};

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

    fn reminders() -> (TempDir, Reminders) {
        let dir = TempDir::new().unwrap();
        dir.child("reminders.json")
            .write_str(r#"{"stored": [] }"#)
            .unwrap();

        let reminders = Reminders::load(&dir.path().join("reminders.json")).unwrap();

        (dir, reminders)
    }

    #[test]
    fn repeating_reminders() -> Result<()> {
        use time::Weekday::*;
        let mut clock = ControlledClock::new(2021, July, 15)?;
        let (_dir, mut reminders) = reminders();

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

        let (_dir, mut reminders) = reminders();

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
        let (_dir, mut reminders) = reminders();

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
        let (_dir, mut reminders) = reminders();

        clock.advance_to(Monday);
        reminders.every(&clock, &RepeatingDate::Weekday(Wednesday), "One");
        reminders.every(&clock, &2.weekly(), "Two");
        reminders.on_date(clock.after(3.days()), "Three");
        reminders.on_date(clock.after(4.days()), "Four");
        reminders.on_date(clock.after(4.days()), "Five");

        assert_eq!(reminders.all().len(), 5);

        reminders.delete(3)?; // should be the "Three"
        assert_eq!(reminders.all().len(), 4);

        let existing_reminders = reminders
            .all()
            .into_iter()
            .map(|reminders| reminders.reminder)
            .collect::<Vec<_>>();

        assert_eq!(
            existing_reminders,
            &["One", "Two", /* deleted: Three */ "Four", "Five"]
        );

        Ok(())
    }

    #[test]
    fn reports_when_the_number_to_delete_is_out_of_range() -> Result<()> {
        let clock = ControlledClock::new(2021, July, 15)?;
        let (_dir, mut reminders) = reminders();

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

    mod specific_date {
        use super::*;

        #[test]
        fn specifics_dates_are_their_own_next_date() {
            let jan_15_2022 = date!(2022 - 01 - 15);
            let specific_date = SpecificDate::OnDate(jan_15_2022);

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
