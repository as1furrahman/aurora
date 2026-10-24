//! Small shared helpers: identifier rules, semver, hashing, legacy-compatible formatting.

use sha2::{Digest, Sha256};

/// `^[a-z0-9]+(-[a-z0-9]+)*$`, matched against the whole string (no trailing newline).
pub fn is_kebab_id(s: &str) -> bool {
    !s.is_empty()
        && s.split('-')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
}

/// `^[A-Za-z0-9][A-Za-z0-9._-]*$`: a plain file name (no separators, not hidden).
pub fn is_plain_file_name(s: &str) -> bool {
    let mut bytes = s.bytes();
    matches!(bytes.next(), Some(b) if b.is_ascii_alphanumeric())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Strict Semantic Versioning 2.0.0.
pub fn is_semver(s: &str) -> bool {
    semver::Version::parse(s).is_ok()
}

/// Lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Render a quoted list in the legacy CLI diagnostic format: `['a', 'b']`.
pub fn list_repr<S: AsRef<str>>(items: &[S]) -> String {
    let inner: Vec<String> = items.iter().map(|s| format!("'{}'", s.as_ref())).collect();
    format!("[{}]", inner.join(", "))
}

/// Format Unix seconds as `YYYY-MM-DDTHH:MM:SSZ` (UTC), the run-record timestamp format.
/// Civil-from-days conversion (Howard Hinnant's algorithm); no time-zone database needed.
pub fn format_utc(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let secs = unix_secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z", secs / 3_600, secs % 3_600 / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_formatting() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_792_756_800), "2026-10-23T12:00:00Z");
        assert_eq!(format_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(format_utc(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn kebab_ids() {
        for ok in ["a", "plan", "final-verify", "a1-b2"] {
            assert!(is_kebab_id(ok), "{ok}");
        }
        for bad in ["", "-a", "a-", "a--b", "A", "a_b", "a\n", "a b"] {
            assert!(!is_kebab_id(bad), "{bad:?}");
        }
    }

    #[test]
    fn plain_file_names() {
        for ok in ["plan.md", "a", "x_y-z.1"] {
            assert!(is_plain_file_name(ok), "{ok}");
        }
        for bad in ["", ".hidden", "a/b", "../x", "a\n", "-x"] {
            assert!(!is_plain_file_name(bad), "{bad:?}");
        }
    }

    #[test]
    fn semver_rules() {
        assert!(is_semver("1.0.0") && is_semver("1.2.3-rc.1+build.5"));
        assert!(!is_semver("1.0") && !is_semver("01.0.0") && !is_semver("1.0.0\n"));
    }

    #[test]
    fn list_repr_uses_quoted_items() {
        assert_eq!(list_repr(&["a", "b"]), "['a', 'b']");
        assert_eq!(list_repr::<&str>(&[]), "[]");
    }
}
