//! Media Dates: dates that keep the precision the Provider gave them.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Why a [`MediaDate`] is invalid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidMediaDate {
    /// The year is outside 1..=9999, the years a four-digit date can
    /// name.
    #[error("year {0} is outside 1..=9999")]
    Year(u16),
    /// The month is outside 1..=12.
    #[error("month {0} does not exist")]
    Month(u8),
    /// The day does not exist in its month.
    #[error("day {day} does not exist in {year}-{month:02}")]
    Day {
        /// The year.
        year: u16,
        /// The month.
        month: u8,
        /// The day that does not exist.
        day: u8,
    },
    /// A day was given without its month.
    #[error("a day needs a month")]
    DayWithoutMonth,
    /// The text is not `YYYY`, `YYYY-MM` or `YYYY-MM-DD`.
    #[error("not a Media Date: {0:?}")]
    Syntax(String),
}

/// How much of a [`MediaDate`] is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DatePrecision {
    /// Only the year.
    Year,
    /// Year and month.
    Month,
    /// The full date.
    Day,
}

/// A release, upload, playlist creation or playlist update date with its
/// original precision preserved. A release known only by year stays a year;
/// Ristle never invents a month or day.
///
/// The fields are public for reading. Build values with [`MediaDate::new`] or
/// parse them, which reject dates that do not exist; deserialization checks
/// the same rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "MediaDateRepr", into = "MediaDateRepr")]
pub struct MediaDate {
    /// The year, 1 to 9999.
    pub year: u16,
    /// The month, 1 to 12, when known.
    pub month: Option<u8>,
    /// The day of the month, when known. Only set together with `month`.
    pub day: Option<u8>,
}

impl MediaDate {
    /// A checked Media Date.
    pub fn new(year: u16, month: Option<u8>, day: Option<u8>) -> Result<Self, InvalidMediaDate> {
        let date = Self { year, month, day };
        date.check()?;
        Ok(date)
    }

    /// Whether the date exists, for values built field by field.
    pub fn check(&self) -> Result<(), InvalidMediaDate> {
        if !(1..=9999).contains(&self.year) {
            return Err(InvalidMediaDate::Year(self.year));
        }
        match (self.month, self.day) {
            (None, None) => Ok(()),
            (None, Some(_)) => Err(InvalidMediaDate::DayWithoutMonth),
            (Some(month), _) if !(1..=12).contains(&month) => Err(InvalidMediaDate::Month(month)),
            (Some(_), None) => Ok(()),
            (Some(month), Some(day)) => {
                if day >= 1 && day <= days_in_month(self.year, month) {
                    Ok(())
                } else {
                    Err(InvalidMediaDate::Day {
                        year: self.year,
                        month,
                        day,
                    })
                }
            }
        }
    }

    /// How much of the date is known.
    pub fn precision(&self) -> DatePrecision {
        match (self.month, self.day) {
            (Some(_), Some(_)) => DatePrecision::Day,
            (Some(_), None) => DatePrecision::Month,
            _ => DatePrecision::Year,
        }
    }
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, as precise as the date is.
impl fmt::Display for MediaDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}", self.year)?;
        if let Some(month) = self.month {
            write!(f, "-{month:02}")?;
            if let Some(day) = self.day {
                write!(f, "-{day:02}")?;
            }
        }
        Ok(())
    }
}

/// Parses `YYYY`, `YYYY-MM` or `YYYY-MM-DD`, keeping the precision of the
/// text.
impl FromStr for MediaDate {
    type Err = InvalidMediaDate;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let syntax = || InvalidMediaDate::Syntax(text.to_owned());
        let mut parts = text.split('-');
        let year = parts
            .next()
            .filter(|part| part.len() == 4)
            .ok_or_else(syntax)?;
        let month = parts.next().map(|part| two_digits(part).ok_or_else(syntax));
        let day = parts.next().map(|part| two_digits(part).ok_or_else(syntax));
        if parts.next().is_some() || !year.bytes().all(|b| b.is_ascii_digit()) {
            return Err(syntax());
        }
        let year = year.parse().map_err(|_| syntax())?;
        Self::new(year, month.transpose()?, day.transpose()?)
    }
}

fn two_digits(part: &str) -> Option<u8> {
    if part.len() == 2 && part.bytes().all(|b| b.is_ascii_digit()) {
        part.parse().ok()
    } else {
        None
    }
}

#[derive(Serialize, Deserialize)]
struct MediaDateRepr {
    year: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    month: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    day: Option<u8>,
}

impl TryFrom<MediaDateRepr> for MediaDate {
    type Error = InvalidMediaDate;

    fn try_from(repr: MediaDateRepr) -> Result<Self, Self::Error> {
        Self::new(repr.year, repr.month, repr.day)
    }
}

impl From<MediaDate> for MediaDateRepr {
    fn from(date: MediaDate) -> Self {
        Self {
            year: date.year,
            month: date.month,
            day: date.day,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_precision_it_was_given() {
        let year = MediaDate::new(1999, None, None).unwrap();
        let month = MediaDate::new(1999, Some(7), None).unwrap();
        let day = MediaDate::new(1999, Some(7), Some(4)).unwrap();

        assert_eq!(year.precision(), DatePrecision::Year);
        assert_eq!(month.precision(), DatePrecision::Month);
        assert_eq!(day.precision(), DatePrecision::Day);
        assert_eq!(year.to_string(), "1999");
        assert_eq!(month.to_string(), "1999-07");
        assert_eq!(day.to_string(), "1999-07-04");
        for date in [year, month, day] {
            assert_eq!(date.to_string().parse::<MediaDate>().unwrap(), date);
        }
    }

    #[test]
    fn rejects_dates_that_do_not_exist() {
        assert_eq!(
            MediaDate::new(0, None, None),
            Err(InvalidMediaDate::Year(0))
        );
        assert_eq!(
            MediaDate::new(10_000, None, None),
            Err(InvalidMediaDate::Year(10_000))
        );
        assert!(MediaDate::new(9999, Some(12), Some(31)).is_ok());
        assert_eq!(
            MediaDate::new(2020, Some(13), None),
            Err(InvalidMediaDate::Month(13))
        );
        assert_eq!(
            MediaDate::new(2020, Some(0), None),
            Err(InvalidMediaDate::Month(0))
        );
        assert_eq!(
            MediaDate::new(2020, None, Some(1)),
            Err(InvalidMediaDate::DayWithoutMonth)
        );
        assert!(MediaDate::new(2020, Some(2), Some(29)).is_ok());
        assert!(MediaDate::new(2000, Some(2), Some(29)).is_ok());
        assert!(MediaDate::new(1900, Some(2), Some(29)).is_err());
        assert!(MediaDate::new(2021, Some(4), Some(31)).is_err());
        assert!(MediaDate::new(2021, Some(1), Some(0)).is_err());
    }

    #[test]
    fn rejects_malformed_text() {
        for text in [
            "",
            "99",
            "1999-7",
            "1999-07-4",
            "1999-07-04-01",
            "1999/07",
            "+999",
            "1999-",
            "0000",
            "1999-13",
        ] {
            assert!(text.parse::<MediaDate>().is_err(), "{text:?} parsed");
        }
    }

    #[test]
    fn deserialization_checks_the_date() {
        let json = serde_json::to_string(&MediaDate::new(2019, Some(2), None).unwrap()).unwrap();
        assert_eq!(json, r#"{"year":2019,"month":2}"#);
        assert!(serde_json::from_str::<MediaDate>(r#"{"year":2019,"month":2,"day":30}"#).is_err());
        assert!(serde_json::from_str::<MediaDate>(r#"{"year":2019,"day":3}"#).is_err());
    }
}
