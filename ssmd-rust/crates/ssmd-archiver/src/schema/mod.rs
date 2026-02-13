// Re-export all schema types from the shared ssmd-schemas crate.
// The archiver consumes these; the future parquet generator will too.
pub use ssmd_schemas::*;

#[cfg(test)]
mod regression_tests;
