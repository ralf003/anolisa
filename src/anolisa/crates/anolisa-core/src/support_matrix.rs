//! Component support matrix matching.
//!
//! A component manifest may declare one or more `[[support_matrix]]`
//! entries describing the (os, os_version, arch, kernel) tuples on which
//! the component is supported, together with a tier and optional notes.
//!
//! [`match_support`] takes an [`EnvFacts`] snapshot together with the
//! matrix entries and returns the first entry that matches the host, or
//! an [`UnsupportedError`] describing what was offered when nothing
//! matched. Entries are evaluated in declaration order, so callers
//! should list lower-tier (more preferred) rows first.

use serde::Deserialize;

use anolisa_env::EnvFacts;

/// One row of a component manifest's `[[support_matrix]]` table.
#[derive(Debug, Clone, Deserialize)]
pub struct SupportEntry {
    /// `ID` from `/etc/os-release`, e.g. `"alinux"`, `"ubuntu"`,
    /// `"debian"`, `"anolis"`.
    pub os: String,
    /// Either a single `VERSION_ID` (e.g. `"4"`) or a list of accepted
    /// values (e.g. `["24.04", "26.04"]`).
    pub os_version: OsVersionSpec,
    /// Accepted CPU architectures, e.g. `["x86_64", "aarch64"]`.
    pub arch: Vec<String>,
    /// Optional kernel range, e.g. `">=6.6"`. `None` means any kernel
    /// is acceptable.
    pub kernel: Option<String>,
    /// Support tier: `"p0"`, `"p1"`, `"p2"`. Lower tier means higher
    /// priority, but ordering is enforced by the manifest author —
    /// matching simply returns the first hit.
    pub tier: String,
    /// Free-form note shown alongside the matched entry.
    pub notes: Option<String>,
}

/// `os_version` accepts either a single string or a list of strings.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum OsVersionSpec {
    /// A single accepted version, e.g. `"4"` or `"24.04"`.
    Single(String),
    /// A list of accepted versions, e.g. `["24.04", "26.04"]`.
    Multiple(Vec<String>),
}

/// Returned by [`match_support`] when no row in the matrix accepts the
/// current host. The `Display` implementation produces a single-line
/// message suitable for CLI surfaces.
#[derive(Debug, thiserror::Error)]
#[error(
    "unsupported: {component} not supported on {os} {os_version} ({arch}); supported: {supported_list}"
)]
pub struct UnsupportedError {
    /// Component name being matched.
    pub component: String,
    /// Resolved `EnvFacts.os_id` (or `"unknown"`).
    pub os: String,
    /// Resolved `EnvFacts.os_version` (or `"unknown"`).
    pub os_version: String,
    /// Resolved `EnvFacts.arch`.
    pub arch: String,
    /// Comma-separated rendering of every entry in the matrix, in
    /// declaration order, of the form `os os_version (arch)`.
    pub supported_list: String,
}

/// Match `env` against `matrix` and return the first accepting entry,
/// or an [`UnsupportedError`] describing what `matrix` advertised.
///
/// Matching rules (all must hold for an entry to match):
/// 1. `entry.os == env.os_id`
/// 2. `entry.os_version` accepts `env.os_version` (see
///    [`os_version_matches`])
/// 3. `entry.arch` contains `env.arch`
/// 4. If `entry.kernel` is `Some(spec)`, `env.kernel` is parsable and
///    satisfies the range (see [`kernel_version_matches`])
pub fn match_support(
    env: &EnvFacts,
    component_name: &str,
    matrix: &[SupportEntry],
) -> Result<SupportEntry, UnsupportedError> {
    let os_id = env.os_id.as_deref().unwrap_or("");
    let os_version = env.os_version.as_deref().unwrap_or("");
    let arch = env.arch.as_str();
    let kernel = env.kernel.as_deref().unwrap_or("");

    for entry in matrix {
        if entry.os != os_id {
            continue;
        }
        if !os_version_matches(&entry.os_version, os_version) {
            continue;
        }
        if !entry.arch.iter().any(|a| a == arch) {
            continue;
        }
        if let Some(spec) = entry.kernel.as_deref() {
            if !kernel_version_matches(spec, kernel) {
                continue;
            }
        }
        return Ok(entry.clone());
    }

    Err(UnsupportedError {
        component: component_name.to_string(),
        os: if os_id.is_empty() {
            "unknown".to_string()
        } else {
            os_id.to_string()
        },
        os_version: if os_version.is_empty() {
            "unknown".to_string()
        } else {
            os_version.to_string()
        },
        arch: arch.to_string(),
        supported_list: render_supported_list(matrix),
    })
}

/// Render every matrix row as `os version (arch)` joined by `", "`.
fn render_supported_list(matrix: &[SupportEntry]) -> String {
    matrix
        .iter()
        .map(|e| {
            let v = match &e.os_version {
                OsVersionSpec::Single(s) => s.clone(),
                OsVersionSpec::Multiple(vs) => vs.join("|"),
            };
            format!("{} {} ({})", e.os, v, e.arch.join("|"))
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Check whether `actual` (e.g. `"24.04"`) is accepted by `spec`.
///
/// A match is exact equality, or `actual` starts with `spec` followed
/// by a `.` so that `"4"` accepts `"4.0"` / `"4.1"` but not `"40"`.
pub fn os_version_matches(spec: &OsVersionSpec, actual: &str) -> bool {
    fn one(v: &str, actual: &str) -> bool {
        actual == v || actual.starts_with(&format!("{}.", v))
    }
    match spec {
        OsVersionSpec::Single(v) => one(v, actual),
        OsVersionSpec::Multiple(vs) => vs.iter().any(|v| one(v, actual)),
    }
}

/// Check whether `actual` kernel release (e.g. `"6.6.30-anolisa.1"`)
/// satisfies `spec` (e.g. `">=6.6"`).
///
/// Comparison is performed on the `(major, minor)` pair extracted from
/// each side; patch / suffix on `actual` is ignored. Supported
/// operators: `>=`, `>`, `<=`, `<`, `=` / `==`. An unparsable spec or
/// actual returns `false`.
pub fn kernel_version_matches(spec: &str, actual: &str) -> bool {
    let spec = spec.trim();
    let (op, version) = if let Some(rest) = spec.strip_prefix(">=") {
        (Op::Ge, rest.trim())
    } else if let Some(rest) = spec.strip_prefix("<=") {
        (Op::Le, rest.trim())
    } else if let Some(rest) = spec.strip_prefix("==") {
        (Op::Eq, rest.trim())
    } else if let Some(rest) = spec.strip_prefix('>') {
        (Op::Gt, rest.trim())
    } else if let Some(rest) = spec.strip_prefix('<') {
        (Op::Lt, rest.trim())
    } else if let Some(rest) = spec.strip_prefix('=') {
        (Op::Eq, rest.trim())
    } else {
        // Bare version means exact major.minor match.
        (Op::Eq, spec)
    };

    let Some(want) = parse_major_minor(version) else {
        return false;
    };
    let Some(have) = parse_major_minor(actual) else {
        return false;
    };

    match op {
        Op::Ge => have >= want,
        Op::Gt => have > want,
        Op::Le => have <= want,
        Op::Lt => have < want,
        Op::Eq => have == want,
    }
}

#[derive(Copy, Clone)]
enum Op {
    Ge,
    Gt,
    Le,
    Lt,
    Eq,
}

/// Parse the leading `major.minor` from `s`, ignoring anything after
/// the second numeric component (patch, `-suffix`, etc.). Missing
/// minor defaults to `0`.
fn parse_major_minor(s: &str) -> Option<(u32, u32)> {
    let head: String = s
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = head.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts
        .next()
        .and_then(|m| m.parse::<u32>().ok())
        .unwrap_or(0);
    Some((major, minor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn env(os_id: &str, os_version: &str, arch: &str, kernel: &str) -> EnvFacts {
        EnvFacts {
            os: "linux".to_string(),
            arch: arch.to_string(),
            libc: None,
            kernel: Some(kernel.to_string()),
            pkg_base: None,
            os_id: Some(os_id.to_string()),
            os_version: Some(os_version.to_string()),
            btf: None,
            cap_bpf: None,
            container: None,
            user: "tester".to_string(),
            uid: 1000,
            home: PathBuf::from("/home/tester"),
        }
    }

    fn entry(
        os: &str,
        ver: OsVersionSpec,
        arch: &[&str],
        kernel: Option<&str>,
        tier: &str,
    ) -> SupportEntry {
        SupportEntry {
            os: os.to_string(),
            os_version: ver,
            arch: arch.iter().map(|s| s.to_string()).collect(),
            kernel: kernel.map(|s| s.to_string()),
            tier: tier.to_string(),
            notes: None,
        }
    }

    #[test]
    fn test_exact_os_version_match() {
        let e = env("alinux", "4", "x86_64", "6.6.30-anolisa.1");
        let matrix = vec![entry(
            "alinux",
            OsVersionSpec::Single("4".to_string()),
            &["x86_64", "aarch64"],
            Some(">=6.6"),
            "p0",
        )];
        let got = match_support(&e, "demo", &matrix).expect("should match");
        assert_eq!(got.tier, "p0");
        assert_eq!(got.os, "alinux");
    }

    #[test]
    fn test_list_os_version_match() {
        let e = env("ubuntu", "24.04", "x86_64", "6.8.0-generic");
        let matrix = vec![entry(
            "ubuntu",
            OsVersionSpec::Multiple(vec!["24.04".to_string(), "26.04".to_string()]),
            &["x86_64"],
            Some(">=6.6"),
            "p1",
        )];
        let got = match_support(&e, "demo", &matrix).expect("should match");
        assert_eq!(got.tier, "p1");
    }

    #[test]
    fn test_arch_mismatch() {
        let e = env("alinux", "4", "aarch64", "6.6.30-anolisa.1");
        let matrix = vec![entry(
            "alinux",
            OsVersionSpec::Single("4".to_string()),
            &["x86_64"],
            None,
            "p0",
        )];
        let err = match_support(&e, "demo", &matrix).expect_err("should not match");
        assert_eq!(err.arch, "aarch64");
        assert!(err.supported_list.contains("x86_64"));
    }

    #[test]
    fn test_kernel_version_gte() {
        assert!(kernel_version_matches(">=6.6", "6.6.30-anolisa.1"));
        assert!(kernel_version_matches(">=6.6", "6.10.0"));
        assert!(!kernel_version_matches(">=6.6", "5.10.1-xxx"));
        assert!(!kernel_version_matches(">=6.6", "6.5.0"));
        assert!(kernel_version_matches(">=5.10", "5.10.0-1-amd64"));
    }

    #[test]
    fn test_no_match_returns_error() {
        let e = env("debian", "12", "x86_64", "6.1.0-xxx");
        let matrix = vec![
            entry(
                "alinux",
                OsVersionSpec::Single("4".to_string()),
                &["x86_64"],
                Some(">=6.6"),
                "p0",
            ),
            entry(
                "ubuntu",
                OsVersionSpec::Multiple(vec!["24.04".to_string(), "26.04".to_string()]),
                &["x86_64", "aarch64"],
                Some(">=6.6"),
                "p1",
            ),
        ];
        let err = match_support(&e, "demo", &matrix).expect_err("should not match");
        assert_eq!(err.component, "demo");
        assert_eq!(err.os, "debian");
        assert_eq!(err.os_version, "12");
        assert_eq!(err.arch, "x86_64");
        assert!(
            err.supported_list.contains("alinux 4"),
            "supported_list missing alinux entry: {}",
            err.supported_list
        );
        assert!(
            err.supported_list.contains("ubuntu 24.04|26.04"),
            "supported_list missing ubuntu entry: {}",
            err.supported_list
        );
    }

    #[test]
    fn test_first_match_wins() {
        let e = env("alinux", "4", "x86_64", "6.6.30-anolisa.1");
        let matrix = vec![
            entry(
                "alinux",
                OsVersionSpec::Single("4".to_string()),
                &["x86_64"],
                Some(">=6.6"),
                "p0",
            ),
            entry(
                "alinux",
                OsVersionSpec::Single("4".to_string()),
                &["x86_64"],
                None,
                "p2",
            ),
        ];
        let got = match_support(&e, "demo", &matrix).expect("should match");
        assert_eq!(got.tier, "p0");
    }

    #[test]
    fn test_os_version_starts_with_dot() {
        // "4" should accept "4.0" but not "40"
        let spec = OsVersionSpec::Single("4".to_string());
        assert!(os_version_matches(&spec, "4"));
        assert!(os_version_matches(&spec, "4.0"));
        assert!(!os_version_matches(&spec, "40"));
    }

    #[test]
    fn test_kernel_no_requirement() {
        let e = env("alinux", "4", "x86_64", "");
        let matrix = vec![entry(
            "alinux",
            OsVersionSpec::Single("4".to_string()),
            &["x86_64"],
            None,
            "p0",
        )];
        assert!(match_support(&e, "demo", &matrix).is_ok());
    }
}
