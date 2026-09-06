#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShutdownSignal {
    Sigint,
    Sigterm,
}

impl ShutdownSignal {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sigint => "sigint",
            Self::Sigterm => "sigterm",
        }
    }
}

#[cfg(unix)]
pub async fn shutdown_signal() -> ShutdownSignal {
    use tokio::signal::unix::{SignalKind, signal};

    let mut sigterm =
        signal(SignalKind::terminate()).expect("failed to listen for the SIGTERM shutdown signal");

    tokio::select! {
        result = tokio::signal::ctrl_c() => {
            result.expect("failed to listen for the SIGINT shutdown signal");
            ShutdownSignal::Sigint
        }
        _ = sigterm.recv() => ShutdownSignal::Sigterm,
    }
}

#[cfg(not(unix))]
pub async fn shutdown_signal() -> ShutdownSignal {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to listen for the shutdown signal");

    ShutdownSignal::Sigint
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_names_match_structured_logging_contract() {
        assert_eq!(ShutdownSignal::Sigint.as_str(), "sigint");
        assert_eq!(ShutdownSignal::Sigterm.as_str(), "sigterm");
    }
}
