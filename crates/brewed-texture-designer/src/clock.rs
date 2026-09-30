//! When things were, as lists say it: on the calendar and the clock of the system where the
//! program runs (its time zone, and summer time), as the system tells them; or how long ago.

use std::time::{SystemTime, UNIX_EPOCH};

/// A moment on the calendar and the clock of the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Local {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// When `ms` (milliseconds since 1970) was, to the second: "Today, 14:32:05", "Yesterday,
/// 09:05:40", "28 Sep, 18:40:12", "28 Sep 2025, 18:40:12"; how long ago, where the system does
/// not tell the time of day.
pub fn said(ms: u64) -> String {
    said_with(ms, true)
}

/// [`said`], to the minute: "Today, 14:32".
pub fn said_minutes(ms: u64) -> String {
    said_with(ms, false)
}

fn said_with(ms: u64, seconds: bool) -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let at = (ms / 1000) as i64;
    let (Some(then), Some(today)) = (local(at), local(now)) else {
        return ago(UNIX_EPOCH + std::time::Duration::from_millis(ms));
    };
    let clock = if seconds { format!("{:02}:{:02}:{:02}", then.hour, then.minute, then.second) } else { format!("{:02}:{:02}", then.hour, then.minute) };
    let day = |l: &Local| (l.year, l.month, l.day);
    let month = MONTHS[(then.month.clamp(1, 12) - 1) as usize];
    if day(&then) == day(&today) {
        format!("Today, {clock}")
    } else if local(now - 86_400).is_some_and(|y| day(&y) == day(&then)) {
        format!("Yesterday, {clock}")
    } else if then.year == today.year {
        format!("{} {month}, {clock}", then.day)
    } else {
        format!("{} {month} {}, {clock}", then.day, then.year)
    }
}

/// How long ago `at` was, in words.
pub fn ago(at: SystemTime) -> String {
    let Ok(since) = SystemTime::now().duration_since(at) else { return "just now".into() };
    let minutes = since.as_secs() / 60;
    match minutes {
        0 => "just now".into(),
        1 => "a minute ago".into(),
        2..=59 => format!("{minutes} minutes ago"),
        60..=119 => "an hour ago".into(),
        120..=1439 => format!("{} hours ago", minutes / 60),
        1440..=2879 => "yesterday".into(),
        _ => format!("{} days ago", minutes / 1440),
    }
}

/// `seconds` since 1970 where the program runs; none when the system cannot say.
#[cfg(windows)]
fn local(seconds: i64) -> Option<Local> {
    use windows_sys::Win32::Foundation::{FILETIME, SYSTEMTIME};
    use windows_sys::Win32::System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime};
    // Hundreds of nanoseconds since 1601.
    let ticks = u64::try_from(seconds.checked_add(11_644_473_600)?).ok()?.checked_mul(10_000_000)?;
    let file = FILETIME { dwLowDateTime: ticks as u32, dwHighDateTime: (ticks >> 32) as u32 };
    // SAFETY: plain data, which the functions write; the pointers are to values alive for the
    // calls, of the types the functions take (none: the system's time zone).
    let (mut utc, mut here): (SYSTEMTIME, SYSTEMTIME) = unsafe { (std::mem::zeroed(), std::mem::zeroed()) };
    let told = unsafe { FileTimeToSystemTime(&file, &mut utc) != 0 && SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut here) != 0 };
    told.then(|| Local {
        year: i32::from(here.wYear),
        month: u32::from(here.wMonth),
        day: u32::from(here.wDay),
        hour: u32::from(here.wHour),
        minute: u32::from(here.wMinute),
        second: u32::from(here.wSecond),
    })
}

/// `seconds` since 1970 where the program runs; none when the system cannot say.
#[cfg(any(free_unix, target_os = "macos"))]
fn local(seconds: i64) -> Option<Local> {
    let time = seconds as libc::time_t;
    // SAFETY: plain data, which localtime_r writes (and says whether it did); the pointers are
    // to values alive for the call.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let told = unsafe { libc::localtime_r(&time, &mut tm) };
    (!told.is_null()).then(|| Local {
        year: tm.tm_year + 1900,
        month: (tm.tm_mon + 1) as u32,
        day: tm.tm_mday as u32,
        hour: tm.tm_hour as u32,
        minute: tm.tm_min as u32,
        second: tm.tm_sec as u32,
    })
}

#[cfg(not(any(windows, free_unix, target_os = "macos")))]
fn local(_: i64) -> Option<Local> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Now is today, a day ago yesterday, on the clock of the system; the system says the time
    /// of day on every system the program is made for.
    #[test]
    fn moments_are_said_by_the_day() {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        assert!(local(0).is_some(), "the system tells the time of day");
        assert!(said(now).starts_with("Today, "), "{}", said(now));
        assert!(said(now - 86_400_000).starts_with("Yesterday, "), "{}", said(now - 86_400_000));
        let long_ago = said(now - 400 * 86_400_000);
        assert!(long_ago.chars().filter(char::is_ascii_digit).count() >= 8, "a year of its own: {long_ago}");
        let clock = said(now).trim_start_matches("Today, ").to_string();
        assert!(clock.len() == 8 && clock.as_bytes()[2] == b':' && clock.as_bytes()[5] == b':', "{clock}");
    }
}
