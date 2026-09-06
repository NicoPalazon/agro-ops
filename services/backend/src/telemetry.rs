use tracing_subscriber::EnvFilter;

/// Installs the process-wide JSON tracing subscriber used by backend binaries.
pub fn init_tracing() {
    tracing_subscriber::fmt()
        .json()
        .with_current_span(true)
        .with_span_list(true)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();
}
