//! Small shared helpers: identifier rules, semver, hashing, Python-compatible formatting.

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

/// Render a list the way Python's `repr(list_of_str)` does: `['a', 'b']`.
pub fn py_list<S: AsRef<str>>(items: &[S]) -> String {
    let inner: Vec<String> = items.iter().map(|s| format!("'{}'", s.as_ref())).collect();
    format!("[{}]", inner.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn python_list_repr() {
        assert_eq!(py_list(&["a", "b"]), "['a', 'b']");
        assert_eq!(py_list::<&str>(&[]), "[]");
    }
}
