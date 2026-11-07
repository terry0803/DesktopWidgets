//! Calendar arithmetic. Small enough not to need a date crate.

/// Days since 1970-01-01 (Howard Hinnant's civil-date algorithm).
pub fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 0 is Sunday.
pub fn weekday(year: i32, month: u32, day: u32) -> u32 {
    (days_from_civil(year, month, day) + 4).rem_euclid(7) as u32
}

pub fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

pub fn day_of_year(year: i32, month: u32, day: u32) -> u32 {
    (days_from_civil(year, month, day) - days_from_civil(year, 1, 1)) as u32 + 1
}

/// Reads YYYY-MM-DD, also with slashes or dots, or written out as 2027年1月1日.
pub fn parse(text: &str) -> Option<(i32, u32, u32)> {
    let cleaned = text.trim().trim_end_matches('日').replace(['/', '.', '年', '月'], "-");
    let mut parts = cleaned.split('-');
    let year: i32 = parts.next()?.trim().parse().ok()?;
    let month: u32 = parts.next()?.trim().parse().ok()?;
    let day: u32 = parts.next()?.trim().parse().ok()?;
    let valid = parts.next().is_none()
        && (1..=9999).contains(&year)
        && (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day);
    valid.then_some((year, month, day))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(weekday(2026, 11, 6), 5); // A Friday.
        assert_eq!(weekday(2024, 2, 29), 4);
    }

    #[test]
    fn month_lengths() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(day_of_year(2026, 12, 31), 365);
        assert_eq!(day_of_year(2024, 12, 31), 366);
    }

    #[test]
    fn parsing() {
        assert_eq!(parse("2027-01-01"), Some((2027, 1, 1)));
        assert_eq!(parse(" 2027/2/3 "), Some((2027, 2, 3)));
        assert_eq!(parse("2027.1.1"), Some((2027, 1, 1)));
        assert_eq!(parse("2027年1月1日"), Some((2027, 1, 1)));
        assert_eq!(parse("2026-02-29"), None);
        assert_eq!(parse("2026-13-01"), None);
        assert_eq!(parse("tomorrow"), None);
    }
}
