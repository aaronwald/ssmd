use ssmd_cdc::config::Config;

// Tests that modify env vars must run serially to avoid race conditions
// Combine into single test function to ensure sequential execution
#[test]
fn test_config_from_env() {
    // Test 1: Full config
    std::env::set_var("DATABASE_URL", "postgres://user:pass@localhost/db");
    std::env::set_var("NATS_URL", "nats://localhost:4222");

    let config = Config::from_env().unwrap();

    assert_eq!(config.database_url, "postgres://user:pass@localhost/db");
    assert_eq!(config.nats_url, "nats://localhost:4222");
    assert_eq!(config.slot_name, "ssmd_cdc"); // default

    // Test 2: Tables default (reusing DATABASE_URL)
    assert_eq!(config.tables, vec!["events", "markets", "series_fees"]);

    // Cleanup
    std::env::remove_var("DATABASE_URL");
    std::env::remove_var("NATS_URL");
}
