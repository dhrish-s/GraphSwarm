/// Initialise env_logger with the given level filter.
pub fn setup_logging(level: &str) {
    let _ = env_logger::Builder::from_default_env()
        .filter_level(level.parse().unwrap_or(log::LevelFilter::Info))
        .format_timestamp_millis()
        .try_init();
}
