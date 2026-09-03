use std::{fmt, time::Duration};

const DEFAULT_PORT: u16 = 8080;
const DEFAULT_WORKER_HEARTBEAT_INTERVAL_SECONDS: u64 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppEnvironment {
    Local,
    Staging,
    Production,
}

impl AppEnvironment {
    fn parse(value: &str) -> Result<Self, ConfigError> {
        match value {
            "local" => Ok(Self::Local),
            "staging" => Ok(Self::Staging),
            "production" => Ok(Self::Production),
            _ => Err(ConfigError::UnsupportedAppEnvironment(value.to_owned())),
        }
    }
}

impl fmt::Display for AppEnvironment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Local => "local",
            Self::Staging => "staging",
            Self::Production => "production",
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    UnsupportedAppEnvironment(String),
    MissingDatabaseUrl,
    InvalidPort,
    InvalidWorkerHeartbeatInterval,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedAppEnvironment(value) => {
                write!(formatter, "invalid configuration: unsupported APP_ENV '{value}'")
            }
            Self::MissingDatabaseUrl => {
                formatter.write_str("invalid configuration: DATABASE_URL is required")
            }
            Self::InvalidPort => {
                formatter.write_str("invalid configuration: PORT must be a valid TCP port")
            }
            Self::InvalidWorkerHeartbeatInterval => formatter.write_str(
                "invalid configuration: WORKER_HEARTBEAT_INTERVAL_SECONDS must be a positive integer",
            ),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Clone)]
pub struct RuntimeConfig {
    app_environment: AppEnvironment,
    database_url: String,
    port: u16,
    worker_heartbeat_interval: Duration,
}

impl RuntimeConfig {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_lookup(|key| std::env::var(key).ok())
    }

    pub fn app_environment(&self) -> AppEnvironment {
        self.app_environment
    }

    pub fn database_url(&self) -> &str {
        &self.database_url
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn worker_heartbeat_interval(&self) -> Duration {
        self.worker_heartbeat_interval
    }

    fn from_lookup<F>(lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let app_environment = match lookup("APP_ENV") {
            Some(value) => AppEnvironment::parse(&value)?,
            None => AppEnvironment::Local,
        };
        let database_url = lookup("DATABASE_URL")
            .filter(|value| !value.trim().is_empty())
            .ok_or(ConfigError::MissingDatabaseUrl)?;
        let port = match lookup("PORT") {
            Some(value) => value
                .parse::<u16>()
                .ok()
                .filter(|port| *port > 0)
                .ok_or(ConfigError::InvalidPort)?,
            None => DEFAULT_PORT,
        };
        let worker_heartbeat_interval = match lookup("WORKER_HEARTBEAT_INTERVAL_SECONDS") {
            Some(value) => value
                .parse::<u64>()
                .ok()
                .filter(|seconds| *seconds > 0)
                .map(Duration::from_secs)
                .ok_or(ConfigError::InvalidWorkerHeartbeatInterval)?,
            None => Duration::from_secs(DEFAULT_WORKER_HEARTBEAT_INTERVAL_SECONDS),
        };

        Ok(Self {
            app_environment,
            database_url,
            port,
            worker_heartbeat_interval,
        })
    }
}

impl fmt::Debug for RuntimeConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeConfig")
            .field("app_environment", &self.app_environment)
            .field("database_url", &"[REDACTED]")
            .field("port", &self.port)
            .field("worker_heartbeat_interval", &self.worker_heartbeat_interval)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DATABASE_URL: &str = "postgres://user:super-secret-password@localhost/agro_ops";

    fn config(values: &[(&str, &str)]) -> Result<RuntimeConfig, ConfigError> {
        RuntimeConfig::from_lookup(|key| {
            values
                .iter()
                .find_map(|(candidate, value)| (*candidate == key).then(|| (*value).to_owned()))
        })
    }

    #[test]
    fn parses_valid_local_environment_with_default_port() {
        let config = config(&[("APP_ENV", "local"), ("DATABASE_URL", DATABASE_URL)])
            .expect("local configuration must be valid");

        assert_eq!(config.app_environment(), AppEnvironment::Local);
        assert_eq!(config.port(), DEFAULT_PORT);
        assert_eq!(
            config.worker_heartbeat_interval(),
            Duration::from_secs(DEFAULT_WORKER_HEARTBEAT_INTERVAL_SECONDS)
        );
    }

    #[test]
    fn defaults_missing_app_environment_to_local() {
        let config = config(&[("DATABASE_URL", DATABASE_URL)])
            .expect("configuration without APP_ENV must be valid");

        assert_eq!(config.app_environment(), AppEnvironment::Local);
    }

    #[test]
    fn parses_valid_staging_environment() {
        let config = config(&[("APP_ENV", "staging"), ("DATABASE_URL", DATABASE_URL)])
            .expect("staging configuration must be valid");

        assert_eq!(config.app_environment(), AppEnvironment::Staging);
    }

    #[test]
    fn parses_valid_production_environment() {
        let config = config(&[("APP_ENV", "production"), ("DATABASE_URL", DATABASE_URL)])
            .expect("production configuration must be valid");

        assert_eq!(config.app_environment(), AppEnvironment::Production);
    }

    #[test]
    fn rejects_invalid_app_environment_without_exposing_database_url() {
        let error = config(&[("APP_ENV", "preview"), ("DATABASE_URL", DATABASE_URL)])
            .expect_err("unsupported environment must fail");

        assert_eq!(
            error.to_string(),
            "invalid configuration: unsupported APP_ENV 'preview'"
        );
        assert!(!format!("{error:?}").contains(DATABASE_URL));
    }

    #[test]
    fn parses_valid_port() {
        let config = config(&[
            ("DATABASE_URL", DATABASE_URL),
            ("PORT", "9090"),
            ("WORKER_HEARTBEAT_INTERVAL_SECONDS", "10"),
        ])
        .expect("port configuration must be valid");

        assert_eq!(config.port(), 9090);
        assert_eq!(config.worker_heartbeat_interval(), Duration::from_secs(10));
    }

    #[test]
    fn rejects_invalid_port_without_exposing_database_url() {
        let error = config(&[("DATABASE_URL", DATABASE_URL), ("PORT", "not-a-port")])
            .expect_err("invalid port must fail");

        assert_eq!(
            error.to_string(),
            "invalid configuration: PORT must be a valid TCP port"
        );
        assert!(!format!("{error:?}").contains(DATABASE_URL));
    }

    #[test]
    fn rejects_zero_worker_heartbeat_interval_without_exposing_database_url() {
        let error = config(&[
            ("DATABASE_URL", DATABASE_URL),
            ("WORKER_HEARTBEAT_INTERVAL_SECONDS", "0"),
        ])
        .expect_err("zero worker heartbeat interval must fail");

        assert_eq!(error, ConfigError::InvalidWorkerHeartbeatInterval);
        assert_eq!(
            error.to_string(),
            "invalid configuration: WORKER_HEARTBEAT_INTERVAL_SECONDS must be a positive integer"
        );
        assert!(!format!("{error:?}").contains(DATABASE_URL));
    }

    #[test]
    fn rejects_non_integer_worker_heartbeat_interval_without_exposing_database_url() {
        let error = config(&[
            ("DATABASE_URL", DATABASE_URL),
            ("WORKER_HEARTBEAT_INTERVAL_SECONDS", "abc"),
        ])
        .expect_err("non-integer worker heartbeat interval must fail");

        assert_eq!(error, ConfigError::InvalidWorkerHeartbeatInterval);
        assert_eq!(
            error.to_string(),
            "invalid configuration: WORKER_HEARTBEAT_INTERVAL_SECONDS must be a positive integer"
        );
        assert!(!format!("{error:?}").contains(DATABASE_URL));
    }

    #[test]
    fn rejects_missing_database_url() {
        let error = config(&[("APP_ENV", "local")]).expect_err("missing database URL must fail");

        assert_eq!(
            error.to_string(),
            "invalid configuration: DATABASE_URL is required"
        );
    }

    #[test]
    fn redacts_database_url_from_debug_output() {
        let config =
            config(&[("DATABASE_URL", DATABASE_URL)]).expect("configuration must be valid");

        assert!(!format!("{config:?}").contains(DATABASE_URL));
    }
}
