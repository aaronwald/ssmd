use ssmd_cdc::config::Config;

#[test]
fn test_config_from_env() {
    std::env::set_var("DATABASE_URL", "postgres://user:pass@localhost/db");
    std::env::set_var("NATS_URL", "nats://localhost:4222");

    let config = Config::from_env().unwrap();

    assert_eq!(config.database_url, "postgres://user:pass@localhost/db");
    assert_eq!(config.nats_url, "nats://localhost:4222");
    assert_eq!(config.slot_name, "ssmd_cdc"); // default

    std::env::remove_var("DATABASE_URL");
    std::env::remove_var("NATS_URL");
}

#[test]
fn test_config_tables_default() {
    std::env::set_var("DATABASE_URL", "postgres://localhost/db");

    let config = Config::from_env().unwrap();

    assert_eq!(config.tables, vec!["events", "markets", "series_fees"]);

    std::env::remove_var("DATABASE_URL");
}
