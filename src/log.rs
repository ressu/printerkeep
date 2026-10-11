//! The log file, `printer_data/logs/printerkeep.log`. Fluidd and Mainsail show the logs folder.

use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// When the log is larger than this, it moves to `printerkeep.log.1` and a new log starts.
const MAX_SIZE: u64 = 1024 * 1024;

/// Adds lines to the log, each with the time.
pub fn append(path: &Path, lines: &[String]) -> std::io::Result<()> {
    if let Some(folder) = path.parent() {
        std::fs::create_dir_all(folder)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_SIZE) {
        let mut old = path.as_os_str().to_owned();
        old.push(".1");
        std::fs::rename(path, old)?;
    }
    let time = timestamp(SystemTime::now());
    let mut text = String::new();
    for line in lines {
        text.push_str(&format!("{time} {line}\n"));
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(text.as_bytes())
}

/// The time in UTC, for example `2026-10-11 12:00:00 UTC`.
pub fn timestamp(time: SystemTime) -> String {
    let seconds = time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Days since 1970-01-01 to a date. Algorithm by Howard Hinnant ("chrono-compatible low-level date
/// algorithms").
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn timestamps() {
        assert_eq!(timestamp(UNIX_EPOCH), "1970-01-01 00:00:00 UTC");
        // 2026-10-11 12:34:56 UTC
        let t = UNIX_EPOCH + Duration::from_secs(1_791_722_096);
        assert_eq!(timestamp(t), "2026-10-11 12:34:56 UTC");
        // A leap day.
        let t = UNIX_EPOCH + Duration::from_secs(951_782_400);
        assert_eq!(timestamp(t), "2000-02-29 00:00:00 UTC");
    }

    #[test]
    fn append_rotates_large_logs() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("logs/printerkeep.log");
        append(&log, &["one".into()]).unwrap();
        assert!(
            std::fs::read_to_string(&log)
                .unwrap()
                .ends_with(" UTC one\n")
        );

        std::fs::write(&log, vec![b'x'; MAX_SIZE as usize + 1]).unwrap();
        append(&log, &["two".into()]).unwrap();
        assert!(dir.path().join("logs/printerkeep.log.1").exists());
        assert!(
            std::fs::read_to_string(&log)
                .unwrap()
                .ends_with(" UTC two\n")
        );
    }
}
