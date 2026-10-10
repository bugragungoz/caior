//! Dotted numeric versions as Windows installers report them ("12.0.40664.0", "8.0.50727.6195").

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// A version made of up to four numeric parts. Missing parts compare as zero,
/// so "14.40" == "14.40.0.0".
#[derive(Debug, Clone, Default, Eq)]
pub struct Version {
    parts: Vec<u64>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("invalid version {0:?}: expected dotted numbers like 12.0.40664.0")]
pub struct ParseVersionError(pub String);

impl Version {
    pub fn parts(&self) -> &[u64] {
        &self.parts
    }

    fn part(&self, i: usize) -> u64 {
        self.parts.get(i).copied().unwrap_or(0)
    }
}

impl FromStr for Version {
    type Err = ParseVersionError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim().trim_start_matches(['v', 'V']);
        if trimmed.is_empty() {
            return Err(ParseVersionError(s.to_string()));
        }
        let parts = trimmed
            .split('.')
            .map(|p| p.parse::<u64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ParseVersionError(s.to_string()))?;
        if parts.len() > 4 {
            return Err(ParseVersionError(s.to_string()));
        }
        Ok(Version { parts })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let len = self.parts.len().max(other.parts.len());
        (0..len)
            .map(|i| self.part(i).cmp(&other.part(i)))
            .find(|o| o.is_ne())
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text: Vec<String> = self.parts.iter().map(u64::to_string).collect();
        f.write_str(&text.join("."))
    }
}

impl serde::Serialize for Version {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for Version {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Version {
    /// Lenient parse for versions read from the system: keeps the leading numeric
    /// dotted part ("8.0.11-servicing" -> 8.0.11, "v14.44.35211.00" -> 14.44.35211.0).
    pub fn parse_lenient(s: &str) -> Option<Version> {
        let t = s.trim().trim_start_matches(['v', 'V']);
        let end = t.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(t.len());
        let head = t[..end].trim_end_matches('.');
        let parts: Vec<&str> = head.split('.').take(4).collect();
        if parts.is_empty() || parts.iter().any(|p| p.is_empty()) {
            return None;
        }
        parts.join(".").parse().ok()
    }

    /// Compares only the first `n` parts (used for inclusive upper bounds such as
    /// os_max = "10.0.18363" against "10.0.18363.1556").
    pub fn truncated(&self, n: usize) -> Version {
        Version { parts: (0..n).map(|i| self.part(i)).collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    #[test]
    fn missing_parts_are_zero() {
        assert_eq!(v("14.40"), v("14.40.0.0"));
        assert!(v("14.40.33816") > v("14.40"));
    }

    #[test]
    fn numeric_not_lexical() {
        assert!(v("12.0.40664.0") > v("12.0.9999.0"));
        assert!(v("10.0.40219.325") > v("10.0.40219.1"));
    }

    #[test]
    fn leading_v_is_accepted() {
        assert_eq!(v("v8.0.11"), v("8.0.11"));
    }

    #[test]
    fn lenient_parse_of_system_strings() {
        assert_eq!(Version::parse_lenient("v14.44.35211.00"), Some(v("14.44.35211.0")));
        assert_eq!(Version::parse_lenient("8.0.11-servicing.24517.7"), Some(v("8.0.11")));
        assert_eq!(Version::parse_lenient("1.2.3.4.5"), Some(v("1.2.3.4")));
        assert_eq!(Version::parse_lenient("Evergreen"), None);
        assert_eq!(Version::parse_lenient(""), None);
    }

    #[test]
    fn truncated_for_upper_bounds() {
        assert!(v("10.0.18363.1556").truncated(3) <= v("10.0.18363"));
        assert!(v("10.0.19041.1").truncated(3) > v("10.0.18363"));
    }

    #[test]
    fn rejects_garbage() {
        assert!("8.x".parse::<Version>().is_err());
        assert!("".parse::<Version>().is_err());
        assert!("1.2.3.4.5".parse::<Version>().is_err());
    }
}
