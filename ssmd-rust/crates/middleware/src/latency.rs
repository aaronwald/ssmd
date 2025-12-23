//! Low-latency primitives: TSC clock and string interning
//!
//! These avoid syscalls on the hot path.

use lasso::{Spur, ThreadedRodeo};
use once_cell::sync::Lazy;
use quanta::Clock;

/// Global TSC clock - zero syscall timestamp reads
pub static CLOCK: Lazy<Clock> = Lazy::new(Clock::new);

/// Global string interner - lock-free reads after interning
pub static INTERNER: Lazy<ThreadedRodeo> = Lazy::new(ThreadedRodeo::new);

/// Get current TSC timestamp (zero syscalls)
#[inline]
pub fn now_tsc() -> u64 {
    CLOCK.raw()
}

/// Intern a string, returning a Spur handle
#[inline]
pub fn intern(s: &str) -> Spur {
    INTERNER.get_or_intern(s)
}

/// Resolve a Spur back to &str
#[inline]
pub fn resolve(spur: Spur) -> &'static str {
    // SAFETY: INTERNER is static, so resolved strings live forever
    INTERNER.resolve(&spur)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_now_tsc_returns_increasing_values() {
        let t1 = now_tsc();
        let t2 = now_tsc();
        assert!(t2 >= t1, "TSC should be monotonic");
    }

    #[test]
    fn test_intern_and_resolve() {
        let spur = intern("BTCUSD");
        let resolved = resolve(spur);
        assert_eq!(resolved, "BTCUSD");
    }

    #[test]
    fn test_intern_same_string_returns_same_spur() {
        let spur1 = intern("ETHUSD");
        let spur2 = intern("ETHUSD");
        assert_eq!(spur1, spur2);
    }

    #[test]
    fn test_intern_different_strings_return_different_spurs() {
        let spur1 = intern("AAPL");
        let spur2 = intern("GOOGL");
        assert_ne!(spur1, spur2);
    }
}
