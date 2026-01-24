//! PostgreSQL LSN (Log Sequence Number) utilities
//!
//! LSN format: "segment/offset" where both are hex numbers (e.g., "0/16B3748")
//! Used for CDC synchronization to filter events before a snapshot point.

/// PostgreSQL Log Sequence Number
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Lsn {
    segment: u64,
    offset: u64,
}

impl Lsn {
    /// Parse LSN from PostgreSQL format "segment/offset"
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() != 2 {
            return None;
        }
        Some(Self {
            segment: u64::from_str_radix(parts[0], 16).ok()?,
            offset: u64::from_str_radix(parts[1], 16).ok()?,
        })
    }

    /// Check if this LSN is greater than or equal to another
    pub fn gte(&self, other: &Lsn) -> bool {
        (self.segment, self.offset) >= (other.segment, other.offset)
    }
}

/// Compare two LSN strings, returns true if lsn >= threshold
///
/// # Examples
/// ```
/// use ssmd_middleware::lsn::lsn_gte;
///
/// assert!(lsn_gte("0/16B3748", "0/16B3748"));  // Equal
/// assert!(lsn_gte("0/16B3749", "0/16B3748"));  // Greater offset
/// assert!(lsn_gte("1/0", "0/FFFFFFF"));        // Greater segment
/// assert!(!lsn_gte("0/16B3747", "0/16B3748")); // Less than
/// assert!(lsn_gte("0/10", "0/9"));             // Hex: 16 > 9
/// ```
pub fn lsn_gte(lsn: &str, threshold: &str) -> bool {
    match (Lsn::parse(lsn), Lsn::parse(threshold)) {
        (Some(a), Some(b)) => a.gte(&b),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lsn_parse() {
        let lsn = Lsn::parse("0/16B3748").unwrap();
        assert_eq!(lsn.segment, 0);
        assert_eq!(lsn.offset, 0x16B3748);

        let lsn = Lsn::parse("1/ABCDEF").unwrap();
        assert_eq!(lsn.segment, 1);
        assert_eq!(lsn.offset, 0xABCDEF);

        assert!(Lsn::parse("invalid").is_none());
        assert!(Lsn::parse("0/GGG").is_none()); // Invalid hex
    }

    #[test]
    fn test_lsn_comparison_equal() {
        assert!(lsn_gte("0/16B3748", "0/16B3748"));
    }

    #[test]
    fn test_lsn_comparison_greater_offset() {
        assert!(lsn_gte("0/16B3749", "0/16B3748"));
        assert!(!lsn_gte("0/16B3747", "0/16B3748"));
    }

    #[test]
    fn test_lsn_comparison_greater_segment() {
        assert!(lsn_gte("1/0", "0/FFFFFFF"));
        assert!(!lsn_gte("0/FFFFFFF", "1/0"));
    }

    #[test]
    fn test_lsn_comparison_hex_ordering() {
        // This is where string comparison fails: "10" < "9" as strings
        // but 0x10 (16) > 0x9 (9) numerically
        assert!(lsn_gte("0/10", "0/9"));
        assert!(lsn_gte("0/A", "0/9"));
        assert!(lsn_gte("0/FF", "0/FE"));
    }

    #[test]
    fn test_lsn_comparison_invalid() {
        assert!(!lsn_gte("invalid", "0/0"));
        assert!(!lsn_gte("0/0", "invalid"));
    }
}
