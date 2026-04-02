use std::sync::OnceLock;

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Weekday};
use regex::Regex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateCandidate {
    pub month: u32,
    pub day: u32,
    pub year: Option<i32>,
    pub matched_text: String,
    match_start: usize,
}

pub fn assistant_local_now() -> DateTime<Local> {
    Local::now()
}

pub fn assistant_local_today() -> NaiveDate {
    assistant_local_now().date_naive()
}

pub fn assistant_local_year() -> i32 {
    assistant_local_now().year()
}

pub fn extract_first_date_candidate(message: &str, today: NaiveDate) -> Option<DateCandidate> {
    [
        extract_iso_date(message),
        extract_month_name_date(message),
        extract_day_first_month_name_date(message),
        extract_relative_date(message, today),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|candidate| candidate.match_start)
}

pub fn extract_single_calendar_date(
    message: &str,
    today: NaiveDate,
) -> Option<(NaiveDate, String)> {
    let candidate = extract_first_date_candidate(message, today)?;
    let resolved = resolve_event_date(&candidate, today).ok()?;
    Some((resolved, candidate.matched_text))
}

pub fn resolve_event_date(
    candidate: &DateCandidate,
    today: NaiveDate,
) -> Result<NaiveDate, String> {
    if let Some(year) = candidate.year {
        return NaiveDate::from_ymd_opt(year, candidate.month, candidate.day).ok_or_else(|| {
            "I couldn't parse that calendar date. Use YYYY-MM-DD or a date like June 9, 2026."
                .to_string()
        });
    }

    let current_year = today.year();
    let mut date = NaiveDate::from_ymd_opt(current_year, candidate.month, candidate.day)
        .ok_or_else(|| {
            "I couldn't parse that calendar date. Use YYYY-MM-DD or a date like June 9, 2026."
                .to_string()
        })?;
    if date < today {
        date = NaiveDate::from_ymd_opt(current_year + 1, candidate.month, candidate.day)
            .ok_or_else(|| {
                "I couldn't parse that calendar date. Use YYYY-MM-DD or a date like June 9, 2026."
                    .to_string()
            })?;
    }
    Ok(date)
}

fn extract_iso_date(message: &str) -> Option<DateCandidate> {
    let regex = iso_date_regex();
    let matched = regex.find(message)?;
    let raw = matched.as_str();
    let date = NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()?;
    Some(DateCandidate {
        month: date.month(),
        day: date.day(),
        year: Some(date.year()),
        matched_text: raw.to_string(),
        match_start: matched.start(),
    })
}

fn extract_month_name_date(message: &str) -> Option<DateCandidate> {
    let regex = month_name_date_regex();
    let captures = regex.captures(message)?;
    let matched = captures.get(0)?;
    let month = parse_month_name(captures.get(1)?.as_str())?;
    let day = parse_day_number(captures.get(2)?.as_str())?;
    let year = captures
        .get(3)
        .and_then(|value| value.as_str().parse::<i32>().ok());

    Some(DateCandidate {
        month,
        day,
        year,
        matched_text: matched.as_str().trim().to_string(),
        match_start: matched.start(),
    })
}

fn extract_day_first_month_name_date(message: &str) -> Option<DateCandidate> {
    let regex = day_first_month_name_date_regex();
    let captures = regex.captures(message)?;
    let matched = captures.get(0)?;
    let day = parse_day_number(captures.get(1)?.as_str())?;
    let month = parse_month_name(captures.get(2)?.as_str())?;
    let year = captures
        .get(3)
        .and_then(|value| value.as_str().parse::<i32>().ok());

    Some(DateCandidate {
        month,
        day,
        year,
        matched_text: matched.as_str().trim().to_string(),
        match_start: matched.start(),
    })
}

fn extract_relative_date(message: &str, today: NaiveDate) -> Option<DateCandidate> {
    [
        extract_named_relative_day(message, "day after tomorrow", today + Duration::days(2)),
        extract_named_relative_day(message, "tomorrow", today + Duration::days(1)),
        extract_named_relative_day(message, "today", today),
        extract_qualified_weekday(message, today),
        extract_prefixed_weekday(message, today),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|candidate| candidate.match_start)
}

fn extract_named_relative_day(
    message: &str,
    phrase: &str,
    resolved: NaiveDate,
) -> Option<DateCandidate> {
    let pattern = format!(r"(?i)\b{}\b", regex::escape(phrase));
    let regex = Regex::new(&pattern).ok()?;
    let matched = regex.find(message)?;
    Some(DateCandidate {
        month: resolved.month(),
        day: resolved.day(),
        year: Some(resolved.year()),
        matched_text: matched.as_str().to_string(),
        match_start: matched.start(),
    })
}

fn extract_qualified_weekday(message: &str, today: NaiveDate) -> Option<DateCandidate> {
    let regex = qualified_weekday_regex();
    let captures = regex.captures(message)?;
    let matched = captures.get(0)?;
    let qualifier = captures.get(1)?.as_str().trim().to_ascii_lowercase();
    let weekday = parse_weekday(captures.get(2)?.as_str())?;
    let resolved = resolve_qualified_weekday(today, weekday, &qualifier);
    Some(DateCandidate {
        month: resolved.month(),
        day: resolved.day(),
        year: Some(resolved.year()),
        matched_text: matched.as_str().trim().to_string(),
        match_start: matched.start(),
    })
}

fn extract_prefixed_weekday(message: &str, today: NaiveDate) -> Option<DateCandidate> {
    let regex = prefixed_weekday_regex();
    let captures = regex.captures(message)?;
    let matched = captures.get(0)?;
    let weekday = parse_weekday(captures.get(1)?.as_str())?;
    let resolved = resolve_next_or_same_weekday(today, weekday);
    Some(DateCandidate {
        month: resolved.month(),
        day: resolved.day(),
        year: Some(resolved.year()),
        matched_text: matched.as_str().trim().to_string(),
        match_start: matched.start(),
    })
}

fn parse_day_number(raw: &str) -> Option<u32> {
    raw.parse::<u32>().ok().filter(|day| (1..=31).contains(day))
}

fn parse_month_name(raw: &str) -> Option<u32> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "jan" | "january" => Some(1),
        "feb" | "february" => Some(2),
        "mar" | "march" => Some(3),
        "apr" | "april" => Some(4),
        "may" => Some(5),
        "jun" | "june" => Some(6),
        "jul" | "july" => Some(7),
        "aug" | "august" => Some(8),
        "sep" | "sept" | "september" => Some(9),
        "oct" | "october" => Some(10),
        "nov" | "november" => Some(11),
        "dec" | "december" => Some(12),
        _ => None,
    }
}

fn parse_weekday(raw: &str) -> Option<Weekday> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "monday" => Some(Weekday::Mon),
        "tuesday" => Some(Weekday::Tue),
        "wednesday" => Some(Weekday::Wed),
        "thursday" => Some(Weekday::Thu),
        "friday" => Some(Weekday::Fri),
        "saturday" => Some(Weekday::Sat),
        "sunday" => Some(Weekday::Sun),
        _ => None,
    }
}

fn resolve_qualified_weekday(today: NaiveDate, target: Weekday, qualifier: &str) -> NaiveDate {
    match qualifier {
        "next" => {
            let days_until = days_until_weekday(today.weekday(), target);
            today + Duration::days(if days_until == 0 { 7 } else { days_until })
        }
        "this" => today + Duration::days(days_until_weekday(today.weekday(), target)),
        _ => resolve_next_or_same_weekday(today, target),
    }
}

fn resolve_next_or_same_weekday(today: NaiveDate, target: Weekday) -> NaiveDate {
    today + Duration::days(days_until_weekday(today.weekday(), target))
}

fn days_until_weekday(current: Weekday, target: Weekday) -> i64 {
    let current_days = current.num_days_from_monday() as i64;
    let target_days = target.num_days_from_monday() as i64;
    (target_days - current_days).rem_euclid(7)
}

fn iso_date_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"\b\d{4}-\d{2}-\d{2}\b").expect("iso date regex should compile")
    })
}

fn month_name_date_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)\b(jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|jun(?:e)?|jul(?:y)?|aug(?:ust)?|sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)\s+(\d{1,2})(?:st|nd|rd|th)?(?:,\s*|\s+)?(\d{4})?\b",
        )
        .expect("month name date regex should compile")
    })
}

fn day_first_month_name_date_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:the\s+)?(\d{1,2})(?:st|nd|rd|th)?(?:\s+of)?\s+(jan(?:uary)?|feb(?:ruary)?|mar(?:ch)?|apr(?:il)?|may|jun(?:e)?|jul(?:y)?|aug(?:ust)?|sep(?:t(?:ember)?)?|oct(?:ober)?|nov(?:ember)?|dec(?:ember)?)(?:,\s*|\s+)?(\d{4})?\b",
        )
        .expect("day-first month name regex should compile")
    })
}

fn qualified_weekday_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)\b(next|this)\s+(monday|tuesday|wednesday|thursday|friday|saturday|sunday)\b",
        )
        .expect("qualified weekday regex should compile")
    })
}

fn prefixed_weekday_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r"(?i)\b(?:on|for)\s+(monday|tuesday|wednesday|thursday|friday|saturday|sunday)\b",
        )
        .expect("prefixed weekday regex should compile")
    })
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::{extract_first_date_candidate, extract_single_calendar_date, resolve_event_date};

    #[test]
    fn extracts_day_first_month_name_dates() {
        let today = NaiveDate::from_ymd_opt(2026, 4, 2).unwrap();
        let candidate = extract_first_date_candidate("Add test event for 7th of April", today)
            .expect("date candidate should parse");

        assert_eq!(candidate.month, 4);
        assert_eq!(candidate.day, 7);
        assert_eq!(candidate.year, None);
        assert_eq!(candidate.matched_text, "7th of April");
    }

    #[test]
    fn extracts_next_weekday_relative_to_today() {
        let today = NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
        let (date, matched_text) =
            extract_single_calendar_date("Make an event for next Tuesday", today)
                .expect("relative weekday should resolve");

        assert_eq!(matched_text.to_ascii_lowercase(), "next tuesday");
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 4, 7).unwrap());
    }

    #[test]
    fn resolves_month_day_without_year_into_next_future_date() {
        let today = NaiveDate::from_ymd_opt(2026, 10, 2).unwrap();
        let candidate = extract_first_date_candidate("Schedule on April 7", today)
            .expect("month-day date should parse");
        let date = resolve_event_date(&candidate, today).expect("date should resolve");

        assert_eq!(date, NaiveDate::from_ymd_opt(2027, 4, 7).unwrap());
    }

    #[test]
    fn extracts_prefixed_weekday_without_next_keyword() {
        let today = NaiveDate::from_ymd_opt(2026, 4, 2).unwrap();
        let (date, matched_text) =
            extract_single_calendar_date("Schedule a reminder for Tuesday", today)
                .expect("weekday should resolve");

        assert_eq!(matched_text.to_ascii_lowercase(), "for tuesday");
        assert_eq!(date, NaiveDate::from_ymd_opt(2026, 4, 7).unwrap());
    }
}
