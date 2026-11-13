//! Small shared helpers. No domain logic lives here.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// FNV-1a, 64-bit. Used for change detection only, never for security.
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Proleptic Gregorian date for a unix timestamp (UTC). Returns (year, month, day).
pub fn civil_date(unix_secs: u64) -> (i64, u32, u32) {
    let z = (unix_secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d)
}

/// Day of week for a unix timestamp (UTC): 0 = Monday … 6 = Sunday.
pub fn weekday(unix_secs: u64) -> u32 {
    ((unix_secs / 86_400 + 3) % 7) as u32
}

pub fn date_string(unix_secs: u64) -> String {
    let (y, m, d) = civil_date(unix_secs);
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn rand_hex(n_bytes: usize) -> String {
    let mut buf = vec![0u8; n_bytes];
    if getrandom::fill(&mut buf).is_err() {
        // Extremely unlikely; fall back to time-derived bytes so ids stay unique enough.
        let seed = fnv64(&now_ms().to_le_bytes()).to_le_bytes();
        for (i, b) in buf.iter_mut().enumerate() {
            *b = seed[i % seed.len()] ^ (i as u8);
        }
    }
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// `std::fs::canonicalize` without the verbatim prefix Windows answers with: `\\?\C:\work` comes
/// back as `C:\work`. `cmd.exe` cannot start in the verbatim form, and nobody writes it.
pub fn canonical(path: impl AsRef<Path>) -> std::io::Result<PathBuf> {
    let real = std::fs::canonicalize(path)?;
    #[cfg(windows)]
    let real = real.to_str().map(|text| PathBuf::from(plain_path(text))).unwrap_or(real);
    Ok(real)
}

/// A Windows path without its verbatim prefix: `\\?\C:\work` as `C:\work`, `\\?\UNC\host\share`
/// as `\\host\share`. Anything else comes back as it is.
pub fn plain_path(path: &str) -> String {
    if let Some(share) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{share}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\").filter(|rest| rest.as_bytes().get(1) == Some(&b':')) {
        rest.to_string()
    } else {
        path.to_string()
    }
}

/// Makes `dir`, with its parents, a folder only its owner can enter. What a session stores is not
/// for the machine's other users: the conversation, and the shell's environment with every key in it.
pub fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(dir, std::os::unix::fs::PermissionsExt::from_mode(0o700))?;
    Ok(())
}

/// Cut `s` to at most `max` chars, marking how much was dropped.
pub fn truncate_chars(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        None => s.to_string(),
        Some((idx, _)) => {
            let dropped = s[idx..].chars().count();
            format!("{}…[+{dropped} chars]", &s[..idx])
        }
    }
}

/// First line of `s`, cut to `max` chars, for one-line summaries.
pub fn one_line(s: &str, max: usize) -> String {
    let first = s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    match first.char_indices().nth(max) {
        None => first.to_string(),
        Some((idx, _)) => format!("{}…", &first[..idx]),
    }
}

pub fn fmt_duration(ms: u64) -> String {
    if ms < 10_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else if ms < 60_000 {
        format!("{}s", ms / 1000)
    } else {
        format!("{}m{:02}s", ms / 60_000, (ms % 60_000) / 1000)
    }
}

pub fn fmt_bytes(n: u64) -> String {
    if n < 1024 {
        format!("{n} B")
    } else if n < 1024 * 1024 {
        format!("{:.1} KB", n as f64 / 1024.0)
    } else {
        format!("{:.1} MB", n as f64 / (1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_date_known_values() {
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(951_782_400), (2000, 2, 29));
        assert_eq!(date_string(1_794_339_533), "2026-11-10");
    }

    #[test]
    fn weekday_known_values() {
        assert_eq!(weekday(0), 3); // 1970-01-01 was a Thursday
        assert_eq!(weekday(1_794_339_533), 1); // 2026-11-10 is a Tuesday
    }

    #[test]
    fn truncate_respects_char_boundaries() {
        assert_eq!(truncate_chars("héllo", 10), "héllo");
        assert_eq!(truncate_chars("héllo wörld", 5), "héllo…[+6 chars]");
        assert_eq!(one_line("\n  first line  \nsecond", 5), "first…");
    }

    #[test]
    fn verbatim_windows_paths_are_written_plainly() {
        assert_eq!(plain_path(r"\\?\C:\Users\me\Desktop\Novel"), r"C:\Users\me\Desktop\Novel");
        assert_eq!(plain_path(r"\\?\UNC\host\share\dir"), r"\\host\share\dir");
        assert_eq!(plain_path(r"C:\work"), r"C:\work");
        assert_eq!(plain_path("/home/me/work"), "/home/me/work");
        // A device path is not a drive path: it stays as it is.
        assert_eq!(plain_path(r"\\?\Volume{1}\x"), r"\\?\Volume{1}\x");
    }
}
