// SPDX-License-Identifier: MPL-2.0-no-copyleft-exception

//! Rules for dates that come back, written as iCalendar `RRULE`s of dates, as decision 0006 and the
//! library format spec define them. Recurring actions and journal schedules share them.

use std::collections::VecDeque;
use std::fmt;

use crate::actions::Date;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum Frequency {
    #[default]
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

impl Frequency {
    fn as_str(self) -> &'static str {
        match self {
            Self::Daily => "DAILY",
            Self::Weekly => "WEEKLY",
            Self::Monthly => "MONTHLY",
            Self::Yearly => "YEARLY",
        }
    }
}

/// A day of the week in a rule's `BYDAY`, with an ordinal such as the 2 in `2MO` or the -1 in
/// `-1FR`, or 0 for every such day.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct WeekdayNum {
    pub ordinal: i8,
    /// From 0 for Monday to 6 for Sunday.
    pub weekday: u8,
}

/// A rule that produces dates, from an `RRULE` with no time parts.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct Rule {
    pub frequency: Frequency,
    /// At least 1.
    pub interval: u32,
    pub weekdays: Vec<WeekdayNum>,
    /// Days of the month, from 1, or from -1 for the last day.
    pub month_days: Vec<i8>,
    /// Months, from 1 for January.
    pub months: Vec<u8>,
    /// Which of each period's dates to keep, from 1, or from -1 for the last.
    pub positions: Vec<i16>,
    pub until: Option<Date>,
    pub count: Option<u32>,
    /// The day weeks start on, from 0 for Monday.
    pub week_start: u8,
}

impl Default for Rule {
    fn default() -> Self {
        Self::new(Frequency::Daily)
    }
}

const WEEKDAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];

/// How many periods in a row can produce no date before a rule counts as finished, so that a rule
/// that never matches, such as February 30, ends. The longest real gap, February 29 every year,
/// spans 8 periods.
const MAX_EMPTY_PERIODS: u32 = 4000;

impl Rule {
    pub fn new(frequency: Frequency) -> Self {
        Self {
            frequency,
            interval: 1,
            weekdays: Vec::new(),
            month_days: Vec::new(),
            months: Vec::new(),
            positions: Vec::new(),
            until: None,
            count: None,
            week_start: 0,
        }
    }

    /// Reads an `RRULE`, or nothing if it has parts that the spec doesn't allow.
    pub fn parse(text: &str) -> Option<Self> {
        let mut frequency = None;
        let mut rule = Self::default();
        for part in text.trim().split(';').filter(|part| !part.is_empty()) {
            let (key, value) = part.split_once('=')?;
            let list = || value.split(',');
            match key.to_ascii_uppercase().as_str() {
                "FREQ" => {
                    frequency = Some(match value.to_ascii_uppercase().as_str() {
                        "DAILY" => Frequency::Daily,
                        "WEEKLY" => Frequency::Weekly,
                        "MONTHLY" => Frequency::Monthly,
                        "YEARLY" => Frequency::Yearly,
                        _ => return None,
                    })
                }
                "INTERVAL" => rule.interval = value.parse().ok().filter(|&n| n > 0)?,
                "BYDAY" => {
                    rule.weekdays = list().map(parse_weekday_num).collect::<Option<_>>()?;
                }
                "BYMONTHDAY" => {
                    rule.month_days = list()
                        .map(|day| day.parse().ok().filter(|d: &i8| (1..=31).contains(&d.abs())))
                        .collect::<Option<_>>()?;
                }
                "BYMONTH" => {
                    rule.months = list()
                        .map(|month| month.parse().ok().filter(|m| (1..=12).contains(m)))
                        .collect::<Option<_>>()?;
                }
                "BYSETPOS" => {
                    rule.positions = list()
                        .map(|p| p.parse().ok().filter(|p: &i16| (1..=366).contains(&p.abs())))
                        .collect::<Option<_>>()?;
                }
                "UNTIL" => rule.until = Some(parse_compact_date(value)?),
                "COUNT" => rule.count = Some(value.parse().ok().filter(|&n| n > 0)?),
                "WKST" => rule.week_start = weekday_index(value)?,
                _ => return None,
            }
        }
        rule.frequency = frequency?;
        rule.is_valid().then_some(rule)
    }

    /// Whether the parts fit together as the spec allows.
    pub fn is_valid(&self) -> bool {
        let ordinals = self.weekdays.iter().any(|day| day.ordinal != 0);
        let ordinal_limit = match (self.frequency, self.months.is_empty()) {
            (Frequency::Monthly, _) | (Frequency::Yearly, false) => 5,
            _ => 53,
        };
        self.interval > 0
            && !(self.until.is_some() && self.count.is_some())
            && self.week_start < 7
            && self.weekdays.iter().all(|day| day.weekday < 7 && day.ordinal.abs() <= ordinal_limit)
            && !(ordinals && matches!(self.frequency, Frequency::Daily | Frequency::Weekly))
            && !(ordinals && !self.month_days.is_empty())
            && !(self.frequency == Frequency::Weekly && !self.month_days.is_empty())
    }

    /// The dates the rule produces on or after `from`, counting from `start`, without the days for
    /// which `skip` is true. Skipped days drop out before `positions` picks among a period's dates,
    /// but still count toward `count`, as excluded dates do in iCalendar, so the rule never runs
    /// past the period of its last date, nor picks more dates in that period.
    pub(crate) fn dates<'a>(
        &'a self,
        start: Date,
        from: Date,
        skip: impl Fn(Date) -> bool + 'a,
    ) -> Dates<'a> {
        let end = self.count.and_then(|_| {
            let all: Vec<Date> = self.all_dates(start, start).collect();
            let last = *all.last()?;
            if self.positions.is_empty() {
                return Some(End::Date(last));
            }
            let period = self.period_of(last);
            let picked = all.iter().rev().take_while(|&&d| self.period_of(d) == period).count();
            let index = (period - self.period_of(start)) / i64::from(self.interval);
            Some(End::Period { index, picked })
        });
        self.iterate(start, from, Box::new(skip), end)
    }

    /// The dates the rule produces on or after `from`, counting from `start`, skipping none.
    pub(crate) fn all_dates(&self, start: Date, from: Date) -> Dates<'_> {
        self.iterate(start, from, Box::new(|_| false), None)
    }

    /// The dates as `dates` gives them, ending at `end` if given, or else after `count` dates.
    fn iterate<'a>(
        &'a self,
        start: Date,
        from: Date,
        skip: Box<dyn Fn(Date) -> bool + 'a>,
        end: Option<End>,
    ) -> Dates<'a> {
        let period = if self.count.is_some() || from <= start {
            0
        } else {
            (self.period_of(from) - self.period_of(start)).max(0) / i64::from(self.interval)
        };
        Dates {
            rule: self,
            start,
            from,
            skip,
            end,
            period,
            pending: VecDeque::new(),
            produced: 0,
            empty: 0,
            finished: false,
        }
    }

    /// Whether the rule produces `date`, counting from `start`.
    pub(crate) fn includes(&self, start: Date, date: Date, skip: impl Fn(Date) -> bool) -> bool {
        self.dates(start, date, skip).next() == Some(date)
    }

    /// A number for the period that holds `date`, which grows by one from each period to the next.
    fn period_of(&self, date: Date) -> i64 {
        match self.frequency {
            Frequency::Daily => date.days(),
            Frequency::Weekly => week_start(date, self.week_start).days().div_euclid(7),
            Frequency::Monthly => i64::from(date.year()) * 12 + i64::from(date.month()) - 1,
            Frequency::Yearly => i64::from(date.year()),
        }
    }

    /// The dates in the `period`th period after the one that holds `start`, before `skip`,
    /// `positions`, and the limits that count from `start`.
    fn candidates(&self, start: Date, period: i64) -> Option<Vec<Date>> {
        let step = period * i64::from(self.interval);
        let month_ok = |month: u8| self.months.is_empty() || self.months.contains(&month);
        let weekday_ok = |date: Date| {
            self.weekdays.is_empty() || self.weekdays.iter().any(|d| d.weekday == date.weekday())
        };
        let mut dates = match self.frequency {
            Frequency::Daily => {
                let date = Date::from_days(start.days() + step);
                check_year(date.year())?;
                let day_ok = self.month_days.is_empty()
                    || self
                        .month_days
                        .iter()
                        .any(|&day| month_day(date.year(), date.month(), day) == Some(date.day()));
                if month_ok(date.month()) && day_ok && weekday_ok(date) {
                    vec![date]
                } else {
                    Vec::new()
                }
            }
            Frequency::Weekly => {
                let first =
                    week_start(start, self.week_start).add_days(i32::try_from(step * 7).ok()?);
                check_year(first.year())?;
                let days: Vec<u8> = if self.weekdays.is_empty() {
                    vec![start.weekday()]
                } else {
                    self.weekdays.iter().map(|day| day.weekday).collect()
                };
                days.into_iter()
                    .map(|day| first.add_days(i32::from((day + 7 - self.week_start) % 7)))
                    .filter(|date| month_ok(date.month()))
                    .collect()
            }
            Frequency::Monthly => {
                let months = i64::from(start.year()) * 12 + i64::from(start.month()) - 1 + step;
                let year = check_year(u16::try_from(months.div_euclid(12)).ok()?)?;
                let month = months.rem_euclid(12) as u8 + 1;
                if month_ok(month) { self.days_in_month(start, year, month) } else { Vec::new() }
            }
            Frequency::Yearly => {
                let year = check_year(u16::try_from(i64::from(start.year()) + step).ok()?)?;
                if !self.month_days.is_empty() || !self.months.is_empty() {
                    let months: Vec<u8> = if self.months.is_empty() {
                        (1..=12).collect()
                    } else {
                        self.months.clone()
                    };
                    months.into_iter().flat_map(|m| self.days_in_month(start, year, m)).collect()
                } else if !self.weekdays.is_empty() {
                    let first = Date::new(year, 1, 1)?;
                    nth_weekdays(first, Date::new(year, 12, 31)?, &self.weekdays)
                } else {
                    Date::new(year, start.month(), start.day()).into_iter().collect()
                }
            }
        };
        dates.sort_unstable();
        dates.dedup();
        Some(dates)
    }

    /// The dates in one month of a monthly or yearly rule.
    fn days_in_month(&self, start: Date, year: u16, month: u8) -> Vec<Date> {
        if !self.month_days.is_empty() {
            self.month_days
                .iter()
                .filter_map(|&day| Date::new(year, month, month_day(year, month, day)?))
                .filter(|date| {
                    self.weekdays.is_empty()
                        || self.weekdays.iter().any(|d| d.weekday == date.weekday())
                })
                .collect()
        } else if !self.weekdays.is_empty() {
            let (Some(first), Some(last)) = (
                Date::new(year, month, 1),
                Date::new(year, month, Date::days_in_month(year, month)),
            ) else {
                return Vec::new();
            };
            nth_weekdays(first, last, &self.weekdays)
        } else {
            Date::new(year, month, start.day()).into_iter().collect()
        }
    }
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let join = |items: Vec<String>| items.join(",");
        write!(f, "FREQ={}", self.frequency.as_str())?;
        if self.interval != 1 {
            write!(f, ";INTERVAL={}", self.interval)?;
        }
        if !self.months.is_empty() {
            write!(f, ";BYMONTH={}", join(self.months.iter().map(u8::to_string).collect()))?;
        }
        if !self.month_days.is_empty() {
            write!(f, ";BYMONTHDAY={}", join(self.month_days.iter().map(i8::to_string).collect()))?;
        }
        if !self.weekdays.is_empty() {
            let days = self.weekdays.iter().map(|day| {
                let name = WEEKDAYS[usize::from(day.weekday % 7)];
                if day.ordinal == 0 { name.to_owned() } else { format!("{}{name}", day.ordinal) }
            });
            write!(f, ";BYDAY={}", join(days.collect()))?;
        }
        if !self.positions.is_empty() {
            write!(f, ";BYSETPOS={}", join(self.positions.iter().map(i16::to_string).collect()))?;
        }
        if self.week_start != 0 {
            write!(f, ";WKST={}", WEEKDAYS[usize::from(self.week_start % 7)])?;
        }
        if let Some(until) = self.until {
            write!(f, ";UNTIL={:04}{:02}{:02}", until.year(), until.month(), until.day())?;
        }
        if let Some(count) = self.count {
            write!(f, ";COUNT={count}")?;
        }
        Ok(())
    }
}

/// Where a rule with a `count` ends, worked out without skipping any days.
#[derive(Clone, Copy)]
enum End {
    /// The last date, for a rule that skipped days can only drop dates from.
    Date(Date),
    /// For a rule whose `positions` pick among what skipping leaves, the last period, numbered as
    /// `Dates::period` counts them, and how many dates it picks there.
    Period { index: i64, picked: usize },
}

pub(crate) struct Dates<'a> {
    rule: &'a Rule,
    start: Date,
    from: Date,
    skip: Box<dyn Fn(Date) -> bool + 'a>,
    /// Where the rule ends in place of `count`.
    end: Option<End>,
    period: i64,
    pending: VecDeque<Date>,
    /// How many dates the rule has produced since `start`, for `count`.
    produced: u32,
    /// How many periods in a row have produced no date.
    empty: u32,
    finished: bool,
}

impl Iterator for Dates<'_> {
    type Item = Date;

    fn next(&mut self) -> Option<Date> {
        loop {
            if self.finished {
                return None;
            }
            if let Some(date) = self.pending.pop_front() {
                if self.rule.until.is_some_and(|until| date > until)
                    || matches!(self.end, Some(End::Date(end)) if date > end)
                    || (self.end.is_none() && self.rule.count.is_some_and(|n| self.produced >= n))
                {
                    self.finished = true;
                    return None;
                }
                self.produced += 1;
                if date >= self.from {
                    return Some(date);
                }
                continue;
            }
            if let Some(End::Period { index, .. }) = self.end
                && self.period > index
            {
                self.finished = true;
                return None;
            }
            let Some(mut dates) = self.rule.candidates(self.start, self.period) else {
                self.finished = true;
                return None;
            };
            self.period += 1;
            dates.retain(|&date| !(self.skip)(date));
            if !self.rule.positions.is_empty() {
                let mut picked: Vec<Date> = self
                    .rule
                    .positions
                    .iter()
                    .filter_map(|&position| {
                        let index = if position > 0 {
                            usize::try_from(position - 1).ok()?
                        } else {
                            dates.len().checked_sub(usize::from(position.unsigned_abs()))?
                        };
                        dates.get(index).copied()
                    })
                    .collect();
                picked.sort_unstable();
                picked.dedup();
                dates = picked;
            }
            dates.retain(|&date| date >= self.start);
            if let Some(End::Period { index, picked }) = self.end
                && self.period - 1 == index
            {
                dates.truncate(picked);
            }
            if dates.is_empty() {
                self.empty += 1;
                if self.empty >= MAX_EMPTY_PERIODS {
                    self.finished = true;
                }
                continue;
            }
            self.empty = 0;
            self.pending.extend(dates);
        }
    }
}

fn check_year(year: u16) -> Option<u16> {
    (1..=9998).contains(&year).then_some(year)
}

/// The first day of the week that holds `date`, for weeks that start on `week_start`.
fn week_start(date: Date, week_start: u8) -> Date {
    date.add_days(-i32::from((date.weekday() + 7 - week_start) % 7))
}

/// The day of the month that a `BYMONTHDAY` value names, counting negative values from the end.
fn month_day(year: u16, month: u8, day: i8) -> Option<u8> {
    let length = i16::from(Date::days_in_month(year, month));
    let day = if day > 0 { i16::from(day) } else { length + 1 + i16::from(day) };
    (1..=length).contains(&day).then_some(day as u8)
}

/// The dates from `first` to `last` that `weekdays` names, each ordinal counting within the range.
fn nth_weekdays(first: Date, last: Date, weekdays: &[WeekdayNum]) -> Vec<Date> {
    let mut found = Vec::new();
    for day in weekdays {
        let offset = (day.weekday + 7 - first.weekday()) % 7;
        let matches: Vec<Date> =
            std::iter::successors(Some(first.add_days(i32::from(offset))), |d| {
                Some(d.add_days(7)).filter(|next| *next <= last)
            })
            .filter(|date| *date <= last)
            .collect();
        match day.ordinal {
            0 => found.extend(matches),
            n if n > 0 => found.extend(matches.get(usize::from(n.unsigned_abs()) - 1)),
            n => {
                let index = matches.len().checked_sub(usize::from(n.unsigned_abs()));
                found.extend(index.and_then(|index| matches.get(index)));
            }
        }
    }
    found
}

fn weekday_index(text: &str) -> Option<u8> {
    WEEKDAYS.iter().position(|day| day.eq_ignore_ascii_case(text)).map(|i| i as u8)
}

fn parse_weekday_num(text: &str) -> Option<WeekdayNum> {
    let split = text.len().checked_sub(2)?;
    let (ordinal, day) = text.split_at_checked(split)?;
    let ordinal = match ordinal {
        "" => 0,
        n => n.trim_start_matches('+').parse().ok().filter(|n: &i8| *n != 0)?,
    };
    Some(WeekdayNum { ordinal, weekday: weekday_index(day)? })
}

fn parse_compact_date(text: &str) -> Option<Date> {
    let text = text.get(..8)?;
    text.bytes().all(|b| b.is_ascii_digit()).then_some(())?;
    format!("{}-{}-{}", &text[..4], &text[4..6], &text[6..]).parse().ok()
}

/// The days of the week in the order the apps list them, starting on Sunday, numbered from 0 for
/// Monday.
pub const WEEK: [u8; 7] = [6, 0, 1, 2, 3, 4, 5];
const DAY_NAMES: [&str; 7] =
    ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const SHORT_DAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
pub const MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WORKDAYS: [u8; 5] = [0, 1, 2, 3, 4];

/// What a monthly or yearly rule picks in its month.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum MonthlyOn {
    /// A day of the month, from 1, or from -1 for the last day.
    Day { day: i8 },
    /// The first to fourth, or with -1 the last, of a kind of day.
    Nth { ordinal: i8, kind: DayKind },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum DayKind {
    /// Any day.
    Day,
    /// Monday to Friday.
    Workday,
    /// One day of the week, from 0 for Monday.
    Weekday { weekday: u8 },
}

/// When a rule stops.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Enum))]
pub enum Ends {
    Never,
    On { date: Date },
    After { count: u32 },
}

/// A rule as the repeat editors show it. Both apps offer the same choices, so every rule they make
/// reads back into the same form.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "mobile", derive(uniffi::Record))]
pub struct RepeatForm {
    pub frequency: Frequency,
    pub interval: u32,
    /// For a weekly rule, the days of the week, from 0 for Monday.
    pub weekdays: Vec<u8>,
    /// For a monthly or yearly rule.
    pub on: MonthlyOn,
    /// For a yearly rule, the month, from 1.
    pub month: u8,
    pub ends: Ends,
}

impl RepeatForm {
    /// A weekly form, on the day of the week of `start`, with every other choice ready for when
    /// the editor switches frequency.
    pub fn new(start: Date) -> Self {
        Self {
            frequency: Frequency::Weekly,
            interval: 1,
            weekdays: vec![start.weekday()],
            on: MonthlyOn::Day { day: start.day() as i8 },
            month: start.month(),
            ends: Ends::Never,
        }
    }

    /// The form for `rule`, or nothing if the editors can't show it.
    pub fn from_rule(rule: &Rule, start: Date) -> Option<Self> {
        let mut form = Self::new(start);
        form.frequency = rule.frequency;
        form.interval = rule.interval;
        form.ends = match (rule.until, rule.count) {
            (Some(date), _) => Ends::On { date },
            (_, Some(count)) => Ends::After { count },
            _ => Ends::Never,
        };
        let plain_days = rule.weekdays.iter().all(|day| day.ordinal == 0);
        let days: Vec<u8> = rule.weekdays.iter().map(|day| day.weekday).collect();
        match rule.frequency {
            Frequency::Daily => {
                let simple = rule.weekdays.is_empty()
                    && rule.month_days.is_empty()
                    && rule.months.is_empty()
                    && rule.positions.is_empty();
                simple.then_some(form)
            }
            Frequency::Weekly => {
                // The editors count weeks from Sunday. They can keep a rule that counts from
                // another day only if neither its days nor its start fall where the two week
                // starts put them in different weeks.
                let from_sunday = |day: u8| (day + 7 - WEEK[0]) % 7;
                let same_weeks = rule.interval == 1
                    || days
                        .iter()
                        .chain([start.weekday()].iter())
                        .all(|&day| from_sunday(day) >= from_sunday(rule.week_start));
                let simple =
                    plain_days && rule.months.is_empty() && rule.positions.is_empty() && same_weeks;
                if !days.is_empty() {
                    form.weekdays = days;
                    form.weekdays.sort_unstable();
                    form.weekdays.dedup();
                }
                simple.then_some(form)
            }
            Frequency::Monthly | Frequency::Yearly => {
                if rule.frequency == Frequency::Yearly {
                    match rule.months.as_slice() {
                        [month] => form.month = *month,
                        [] if rule.weekdays.is_empty() && rule.month_days.is_empty() => {}
                        _ => return None,
                    }
                } else if !rule.months.is_empty() {
                    return None;
                }
                form.on = month_pick(rule, start)?;
                Some(form)
            }
        }
    }

    pub fn rule(&self) -> Rule {
        let mut rule = Rule::new(self.frequency);
        rule.interval = self.interval.max(1);
        match self.ends {
            Ends::Never => {}
            Ends::On { date } => rule.until = Some(date),
            Ends::After { count } => rule.count = Some(count.max(1)),
        }
        let plain = |days: &[u8]| -> Vec<WeekdayNum> {
            days.iter().map(|&weekday| WeekdayNum { ordinal: 0, weekday }).collect()
        };
        match self.frequency {
            Frequency::Daily => {}
            Frequency::Weekly => {
                let mut days = self.weekdays.clone();
                days.sort_unstable();
                days.dedup();
                rule.weekdays = plain(&days);
                // Weeks that start on Sunday decide which days share a week when it skips some.
                if rule.interval > 1 {
                    rule.week_start = WEEK[0];
                }
            }
            Frequency::Monthly | Frequency::Yearly => {
                if self.frequency == Frequency::Yearly {
                    rule.months = vec![self.month.clamp(1, 12)];
                }
                match self.on {
                    MonthlyOn::Day { day } => rule.month_days = vec![day],
                    MonthlyOn::Nth { ordinal, kind: DayKind::Weekday { weekday } } => {
                        rule.weekdays = vec![WeekdayNum { ordinal, weekday }];
                    }
                    MonthlyOn::Nth { ordinal, kind: DayKind::Day } => {
                        rule.month_days = vec![ordinal];
                    }
                    MonthlyOn::Nth { ordinal, kind: DayKind::Workday } => {
                        rule.weekdays = plain(&WORKDAYS);
                        rule.positions = vec![i16::from(ordinal)];
                    }
                }
            }
        }
        rule
    }
}

/// What a monthly or yearly rule picks in each month, if the editors can show it.
fn month_pick(rule: &Rule, start: Date) -> Option<MonthlyOn> {
    let days: Vec<u8> = rule.weekdays.iter().map(|day| day.weekday).collect();
    match (rule.month_days.as_slice(), rule.weekdays.as_slice(), rule.positions.as_slice()) {
        ([day], [], []) if *day > 0 || *day == -1 => Some(MonthlyOn::Day { day: *day }),
        ([], [day], []) if day.ordinal != 0 && (day.ordinal == -1 || day.ordinal > 0) => {
            Some(MonthlyOn::Nth {
                ordinal: day.ordinal,
                kind: DayKind::Weekday { weekday: day.weekday },
            })
        }
        ([], weekdays, [position])
            if weekdays.iter().all(|day| day.ordinal == 0)
                && days == WORKDAYS
                && (*position == -1 || (1..=5).contains(position)) =>
        {
            Some(MonthlyOn::Nth { ordinal: *position as i8, kind: DayKind::Workday })
        }
        ([], [], []) => Some(MonthlyOn::Day { day: start.day() as i8 }),
        _ => None,
    }
}

/// The rules the repeat menus offer before Custom, for an action or schedule that starts on
/// `start`: daily, every workday, weekly, monthly, and yearly.
pub fn repeat_presets(start: Date) -> Vec<Rule> {
    let weekly = |days: &[u8]| {
        let mut rule = Rule::new(Frequency::Weekly);
        rule.weekdays = days.iter().map(|&weekday| WeekdayNum { ordinal: 0, weekday }).collect();
        rule
    };
    let mut monthly = Rule::new(Frequency::Monthly);
    monthly.month_days = vec![start.day() as i8];
    let mut yearly = Rule::new(Frequency::Yearly);
    yearly.months = vec![start.month()];
    yearly.month_days = vec![start.day() as i8];
    vec![
        Rule::new(Frequency::Daily),
        weekly(&WORKDAYS),
        weekly(&[start.weekday()]),
        monthly,
        yearly,
    ]
}

/// A short description of a rule in sentence case, such as "Weekly on Monday" or "Every 2 months
/// on the last workday", the same in every app.
pub fn describe_rule(rule: &Rule, start: Date) -> String {
    let every = |unit: &str, plain: &str| match rule.interval {
        1 => plain.to_owned(),
        n => format!("Every {n} {unit}s"),
    };
    let mut text = match RepeatForm::from_rule(rule, start) {
        Some(form) => match form.frequency {
            Frequency::Daily => every("day", "Daily"),
            Frequency::Weekly if form.weekdays == WORKDAYS && rule.interval == 1 => {
                "Every workday".to_owned()
            }
            Frequency::Weekly => {
                let days = if form.weekdays.len() == 1 {
                    DAY_NAMES[usize::from(form.weekdays[0])].to_owned()
                } else if form.weekdays == WORKDAYS {
                    "workdays".to_owned()
                } else {
                    let shown = WEEK.iter().filter(|day| form.weekdays.contains(day));
                    series(shown.map(|&day| SHORT_DAY_NAMES[usize::from(day)].to_owned()))
                };
                format!("{} on {days}", every("week", "Weekly"))
            }
            Frequency::Monthly => {
                format!("{} on {}", every("month", "Monthly"), describe_on(form.on))
            }
            Frequency::Yearly => {
                let month = MONTH_NAMES[usize::from(form.month - 1)];
                let on = match form.on {
                    MonthlyOn::Day { day } if day > 0 => format!("{month} {day}"),
                    on => format!("{} of {month}", describe_on(on)),
                };
                format!("{} on {on}", every("year", "Yearly"))
            }
        },
        None => "Custom repeat".to_owned(),
    };
    match (rule.until, rule.count) {
        (Some(until), _) => text.push_str(&format!(
            ", until {} {}, {}",
            &MONTH_NAMES[usize::from(until.month() - 1)][..3],
            until.day(),
            until.year()
        )),
        (_, Some(1)) => text.push_str(", once"),
        (_, Some(count)) => text.push_str(&format!(", {count} times")),
        _ => {}
    }
    text
}

fn describe_on(on: MonthlyOn) -> String {
    match on {
        MonthlyOn::Day { day } if day > 0 => format!("the {}", ordinal_number(day)),
        MonthlyOn::Day { day } => format!("the {} day", ordinal_word(day)),
        MonthlyOn::Nth { ordinal, kind } => {
            let kind = match kind {
                DayKind::Day => "day".to_owned(),
                DayKind::Workday => "workday".to_owned(),
                DayKind::Weekday { weekday } => DAY_NAMES[usize::from(weekday % 7)].to_owned(),
            };
            format!("the {} {kind}", ordinal_word(ordinal))
        }
    }
}

/// "first" to "fifth", or "last" and "second-to-last" for negative ordinals.
pub fn ordinal_word(ordinal: i8) -> String {
    const WORDS: [&str; 5] = ["first", "second", "third", "fourth", "fifth"];
    match ordinal {
        -1 => "last".to_owned(),
        n if n < 0 => format!("{}-to-last", ordinal_word(-n)),
        n => WORDS
            .get(usize::from(n.unsigned_abs()).wrapping_sub(1))
            .map_or_else(|| ordinal_number(n), |word| (*word).to_owned()),
    }
}

/// "1st", "2nd", "3rd", "4th", and so on.
fn ordinal_number(n: i8) -> String {
    // Whole ordinals, because the spell checker reads the bare suffix of "2nd" as a typo.
    const FIRSTS: [&str; 4] = ["0th", "1st", "2nd", "3rd"];
    let last = match (n % 10, n % 100) {
        (_, 11..=13) => 0,
        (digit @ 1..=3, _) => digit as usize,
        _ => 0,
    };
    format!("{n}{}", &FIRSTS[last][1..])
}

/// Items joined with commas and a final "and."
fn series(items: impl Iterator<Item = String>) -> String {
    let items: Vec<String> = items.collect();
    match items.as_slice() {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        [rest @ .., last] => format!("{}, and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn date(text: &str) -> Date {
        text.parse().unwrap()
    }

    fn first(rule: &str, start: &str, from: &str, n: usize) -> Vec<String> {
        let rule = Rule::parse(rule).unwrap();
        rule.all_dates(date(start), date(from)).take(n).map(|d| d.to_string()).collect()
    }

    #[test]
    fn reads_and_writes_rules() {
        for text in [
            "FREQ=DAILY",
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR",
            "FREQ=MONTHLY;BYDAY=-1FR",
            "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1",
            "FREQ=YEARLY;BYMONTH=11;BYDAY=4TH;COUNT=3",
            "FREQ=MONTHLY;BYMONTHDAY=-1;UNTIL=20271231",
            "FREQ=WEEKLY;WKST=SU",
        ] {
            assert_eq!(Rule::parse(text).unwrap().to_string(), text);
        }
        for bad in [
            "",
            "FREQ=HOURLY",
            "FREQ=DAILY;BYHOUR=9",
            "FREQ=WEEKLY;BYDAY=1MO",
            "FREQ=WEEKLY;BYMONTHDAY=1",
            "FREQ=DAILY;COUNT=2;UNTIL=20270101",
            "FREQ=DAILY;INTERVAL=0",
            "FREQ=MONTHLY;BYMONTHDAY=32",
        ] {
            assert_eq!(Rule::parse(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn produces_calendar_schedules() {
        assert_eq!(
            first("FREQ=WEEKLY;BYDAY=MO,WE,FR", "2026-10-30", "2026-10-30", 4),
            ["2026-10-30", "2026-11-02", "2026-11-04", "2026-11-06"]
        );
        assert_eq!(
            first("FREQ=WEEKLY;INTERVAL=2", "2026-10-29", "2026-11-01", 2),
            ["2026-11-12", "2026-11-26"]
        );
        assert_eq!(
            first("FREQ=MONTHLY;BYDAY=-1FR", "2026-10-01", "2026-10-01", 3),
            ["2026-10-30", "2026-11-27", "2026-12-25"]
        );
        assert_eq!(
            first("FREQ=MONTHLY;BYMONTHDAY=-1", "2026-01-15", "2026-01-15", 3),
            ["2026-01-31", "2026-02-28", "2026-03-31"]
        );
        assert_eq!(
            first("FREQ=MONTHLY", "2026-01-31", "2026-01-31", 3),
            ["2026-01-31", "2026-03-31", "2026-05-31"]
        );
        assert_eq!(
            first("FREQ=YEARLY;BYMONTH=11;BYDAY=4TH", "2026-01-01", "2026-01-01", 2),
            ["2026-11-26", "2027-11-25"]
        );
        assert_eq!(first("FREQ=YEARLY", "2024-02-29", "2024-03-01", 1), ["2028-02-29"]);
        assert_eq!(
            first("FREQ=DAILY;COUNT=3", "2026-10-01", "2026-10-02", 5),
            ["2026-10-02", "2026-10-03"]
        );
        assert_eq!(first("FREQ=DAILY;UNTIL=20261003", "2026-10-01", "2026-10-01", 5).len(), 3);
        assert!(
            first("FREQ=MONTHLY;BYMONTH=2;BYMONTHDAY=30", "2026-01-01", "2026-01-01", 1).is_empty()
        );
    }

    #[test]
    fn skipped_days_drop_out_before_positions_pick() {
        let rule = Rule::parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1").unwrap();
        let holiday = date("2026-12-31");
        let dates: Vec<String> = rule
            .dates(date("2026-12-01"), date("2026-12-01"), |d| d == holiday)
            .take(1)
            .map(|d| d.to_string())
            .collect();
        assert_eq!(dates, ["2026-12-30"]);
    }

    #[test]
    fn forms_round_trip_and_describe() {
        let start = date("2026-11-02");
        for text in [
            "FREQ=DAILY",
            "FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR",
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,FR;WKST=SU",
            "FREQ=MONTHLY;BYMONTHDAY=2",
            "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1",
            "FREQ=YEARLY;BYMONTH=11;BYDAY=4TH",
            "FREQ=YEARLY;BYMONTH=11;BYMONTHDAY=2;COUNT=3",
        ] {
            let rule = Rule::parse(text).unwrap();
            assert_eq!(RepeatForm::from_rule(&rule, start).unwrap().rule(), rule, "{text}");
        }
        let describe = |text: &str| describe_rule(&Rule::parse(text).unwrap(), start);
        assert_eq!(describe("FREQ=WEEKLY;BYDAY=MO"), "Weekly on Monday");
        assert_eq!(describe("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"), "Every workday");
        assert_eq!(describe("FREQ=WEEKLY;BYDAY=MO,SA,SU"), "Weekly on Sun, Mon, and Sat");
        assert_eq!(
            describe("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR"),
            "Every 2 weeks on Mon, Wed, and Fri"
        );
        assert_eq!(describe("FREQ=MONTHLY;BYMONTHDAY=2"), "Monthly on the 2nd");
        assert_eq!(describe("FREQ=MONTHLY;BYMONTHDAY=-1"), "Monthly on the last day");
        assert_eq!(
            describe("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1"),
            "Monthly on the last workday"
        );
        assert_eq!(describe("FREQ=YEARLY;BYMONTH=11;BYMONTHDAY=2"), "Yearly on November 2");
        assert_eq!(
            describe("FREQ=YEARLY;BYMONTH=11;BYDAY=4TH;COUNT=3"),
            "Yearly on the fourth Thursday of November, 3 times"
        );
        assert_eq!(describe("FREQ=MONTHLY;BYMONTHDAY=1,15"), "Custom repeat");
        assert_eq!(describe("FREQ=MONTHLY;BYMONTHDAY=-2"), "Custom repeat");
        let presets: Vec<String> =
            repeat_presets(start).iter().map(|rule| describe_rule(rule, start)).collect();
        assert_eq!(
            presets,
            [
                "Daily",
                "Every workday",
                "Weekly on Monday",
                "Monthly on the 2nd",
                "Yearly on November 2"
            ]
        );
    }

    #[test]
    fn skipped_days_still_count_toward_count() {
        let rule = Rule::parse("FREQ=DAILY;COUNT=3").unwrap();
        let start = date("2026-10-01");
        let dates: Vec<String> = rule
            .dates(start, start, |day| day == date("2026-10-02"))
            .map(|d| d.to_string())
            .collect();
        assert_eq!(dates, ["2026-10-01", "2026-10-03"]);
        let first_workday =
            Rule::parse("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=1;COUNT=2").unwrap();
        let start = date("2026-11-01");
        let dates: Vec<String> = first_workday
            .dates(start, start, |day| day == date("2026-12-01"))
            .map(|d| d.to_string())
            .collect();
        assert_eq!(dates, ["2026-11-02", "2026-12-02"]);
    }

    #[test]
    fn forms_leave_another_week_start_to_custom_rules() {
        let start = date("2026-11-01");
        let monday = Rule::parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,SU").unwrap();
        assert!(RepeatForm::from_rule(&monday, start).is_none());
        let sunday = Rule::parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,SU;WKST=SU").unwrap();
        assert_eq!(RepeatForm::from_rule(&sunday, start).unwrap().rule(), sunday);
        let mondays = Rule::parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO").unwrap();
        assert!(RepeatForm::from_rule(&mondays, start).is_none());
        assert!(RepeatForm::from_rule(&mondays, start.add_days(1)).is_some());
    }
}
