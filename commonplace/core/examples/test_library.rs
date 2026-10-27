// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Builds Personal and Work sample libraries for trying the apps by hand:
//!
//! ```sh
//! cargo run --example test_library -- DIRECTORY [--date YYYY-MM-DD]
//! ```
//!
//! Without `--date`, dates count from the day it runs. With it, samples use noon UTC on the
//! selected date, including note timestamps, action dates, and journal entries.

use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use commonplace_core::{Date, Spaces, samples};

const USAGE: &str = "usage: test_library DIRECTORY [--date YYYY-MM-DD]";

fn arguments(args: impl IntoIterator<Item = OsString>) -> Result<(PathBuf, Option<Date>), String> {
    let mut args = args.into_iter();
    let dir = args.next().ok_or(USAGE)?;
    let date = match (args.next(), args.next(), args.next()) {
        (None, None, None) => None,
        (Some(flag), Some(date), None) if flag == "--date" => {
            let text = date.to_str().ok_or("the date must use YYYY-MM-DD")?;
            let date = text.parse::<Date>().map_err(|error| error.to_string())?;
            if date.year() == 0 {
                return Err("the date's year must be between 0001 and 9999".into());
            }
            Some(date)
        }
        _ => return Err(USAGE.into()),
    };
    Ok((dir.into(), date))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (dir, date) = match arguments(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(error) => {
            eprintln!("test_library: {error}");
            std::process::exit(2);
        }
    };
    if dir.exists() {
        eprintln!("test_library: {} already exists", dir.display());
        std::process::exit(1);
    }
    let mut spaces = Spaces::open(&dir)?;
    let names = if let Some(date) = date {
        samples::populate_on(&mut spaces, date)?
    } else {
        let seconds = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        let today =
            Date::new(1970, 1, 1).expect("the epoch is a date").add_days((seconds / 86_400) as i32);
        samples::populate(&mut spaces, today)?
    };
    let settings =
        names.into_iter().map(|(key, name)| format!("{key}={name}\n")).collect::<String>();
    fs::write(dir.join("device.ini"), format!("[names]\n{settings}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<(PathBuf, Option<Date>), String> {
        arguments(args.iter().map(OsString::from))
    }

    #[test]
    fn accepts_the_current_day_or_a_chosen_calendar_date() {
        assert_eq!(parse(&["with spaces"]).unwrap(), ("with spaces".into(), None));
        assert_eq!(
            parse(&["library", "--date", "2024-02-29"]).unwrap(),
            ("library".into(), Some(Date::new(2024, 2, 29).unwrap()))
        );
    }

    #[test]
    fn rejects_invalid_dates_and_unrecognized_arguments() {
        for args in [
            vec![],
            vec!["library", "--date"],
            vec!["library", "--date", "0000-01-01"],
            vec!["library", "--date", "2025-02-29"],
            vec!["library", "--date", "2026-10-27", "extra"],
            vec!["library", "--date", "2026-2-01"],
            vec!["library", "--other", "2026-10-27"],
        ] {
            assert!(parse(&args).is_err(), "accepted {args:?}");
        }
    }
}
