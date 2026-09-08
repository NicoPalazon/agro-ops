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
    MissingSupabaseUrl,
    InvalidSupabaseUrl,
    InsecureSupabaseUrl,
    MissingSupabasePublishableKey,
    MissingSupabaseSecretKey,
    MissingSupabaseInviteRedirectUrl,
    InvalidSupabaseInviteRedirectUrl,
    InsecureSupabaseInviteRedirectUrl,
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
            Self::MissingSupabaseUrl => {
                formatter.write_str("invalid configuration: SUPABASE_URL is required for API authentication")
            }
            Self::InvalidSupabaseUrl => formatter.write_str(
                "invalid configuration: SUPABASE_URL must be an absolute HTTP(S) URL",
            ),
            Self::InsecureSupabaseUrl => formatter.write_str(
                "invalid configuration: SUPABASE_URL must use HTTPS outside local development",
            ),
            Self::MissingSupabasePublishableKey => formatter.write_str(
                "invalid configuration: SUPABASE_PUBLISHABLE_KEY is required for API authentication",
            ),
            Self::MissingSupabaseSecretKey => formatter.write_str(
                "invalid configuration: SUPABASE_SECRET_KEY is required for access administration",
            ),
            Self::MissingSupabaseInviteRedirectUrl => formatter.write_str(
                "invalid configuration: SUPABASE_INVITE_REDIRECT_URL is required for access administration",
            ),
            Self::InvalidSupabaseInviteRedirectUrl => formatter.write_str(
                "invalid configuration: SUPABASE_INVITE_REDIRECT_URL must be an absolute HTTP(S) URL",
            ),
            Self::InsecureSupabaseInviteRedirectUrl => formatter.write_str(
                "invalid configuration: SUPABASE_INVITE_REDIRECT_URL must use HTTPS outside local development",
            ),
        }
    }
}

#[derive(Clone)]
pub struct SupabaseAuthConfig {
    url: String,
    publishable_key: String,
}

impl SupabaseAuthConfig {
    pub fn from_env(app_environment: AppEnvironment) -> Result<Self, ConfigError> {
        Self::from_lookup(app_environment, |key| std::env::var(key).ok())
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn publishable_key(&self) -> &str {
        &self.publishable_key
    }

    #[cfg(test)]
    pub(crate) fn from_values(
        app_environment: AppEnvironment,
        url: impl Into<String>,
        publishable_key: impl Into<String>,
    ) -> Result<Self, ConfigError> {
        let url = url.into();
        let publishable_key = publishable_key.into();

        Self::from_lookup(app_environment, |key| match key {
            "SUPABASE_URL" => Some(url.clone()),
            "SUPABASE_PUBLISHABLE_KEY" => Some(publishable_key.clone()),
            _ => None,
        })
    }

    fn from_lookup<F>(app_environment: AppEnvironment, lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let url = lookup("SUPABASE_URL")
            .filter(|value| !value.trim().is_empty())
            .ok_or(ConfigError::MissingSupabaseUrl)?;
        let parsed_url = reqwest::Url::parse(&url).map_err(|_| ConfigError::InvalidSupabaseUrl)?;
        if !matches!(parsed_url.scheme(), "http" | "https") || parsed_url.host().is_none() {
            return Err(ConfigError::InvalidSupabaseUrl);
        }
        if app_environment != AppEnvironment::Local && parsed_url.scheme() != "https" {
            return Err(ConfigError::InsecureSupabaseUrl);
        }
        let publishable_key = lookup("SUPABASE_PUBLISHABLE_KEY")
            .filter(|value| !value.trim().is_empty())
            .ok_or(ConfigError::MissingSupabasePublishableKey)?;

        Ok(Self {
            url,
            publishable_key,
        })
    }
}

impl fmt::Debug for SupabaseAuthConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SupabaseAuthConfig")
            .field("url", &self.url)
            .field("publishable_key", &"[REDACTED]")
            .finish()
    }
}

#[derive(Clone)]
pub struct SupabaseAdminConfig {
    url: String,
    secret_key: String,
    invite_redirect_url: String,
}

impl SupabaseAdminConfig {
    pub fn from_env(app_environment: AppEnvironment) -> Result<Self, ConfigError> {
        Self::from_lookup(app_environment, |key| std::env::var(key).ok())
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn secret_key(&self) -> &str {
        &self.secret_key
    }

    pub fn invite_redirect_url(&self) -> &str {
        &self.invite_redirect_url
    }

    #[cfg(test)]
    pub(crate) fn from_values(
        app_environment: AppEnvironment,
        url: impl Into<String>,
        secret_key: impl Into<String>,
        invite_redirect_url: impl Into<String>,
    ) -> Result<Self, ConfigError> {
        let url = url.into();
        let secret_key = secret_key.into();
        let invite_redirect_url = invite_redirect_url.into();
        Self::from_lookup(app_environment, |key| match key {
            "SUPABASE_URL" => Some(url.clone()),
            "SUPABASE_SECRET_KEY" => Some(secret_key.clone()),
            "SUPABASE_INVITE_REDIRECT_URL" => Some(invite_redirect_url.clone()),
            _ => None,
        })
    }

    fn from_lookup<F>(app_environment: AppEnvironment, lookup: F) -> Result<Self, ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        let url = lookup("SUPABASE_URL")
            .filter(|value| !value.trim().is_empty())
            .ok_or(ConfigError::MissingSupabaseUrl)?;
        let parsed_url = reqwest::Url::parse(&url).map_err(|_| ConfigError::InvalidSupabaseUrl)?;
        if !matches!(parsed_url.scheme(), "http" | "https") || parsed_url.host().is_none() {
            return Err(ConfigError::InvalidSupabaseUrl);
        }
        if app_environment != AppEnvironment::Local && parsed_url.scheme() != "https" {
            return Err(ConfigError::InsecureSupabaseUrl);
        }
        let secret_key = lookup("SUPABASE_SECRET_KEY")
            .filter(|value| !value.trim().is_empty())
            .ok_or(ConfigError::MissingSupabaseSecretKey)?;
        let invite_redirect_url = lookup("SUPABASE_INVITE_REDIRECT_URL")
            .filter(|value| !value.trim().is_empty())
            .ok_or(ConfigError::MissingSupabaseInviteRedirectUrl)?;
        let parsed_redirect_url = reqwest::Url::parse(&invite_redirect_url)
            .map_err(|_| ConfigError::InvalidSupabaseInviteRedirectUrl)?;
        if !matches!(parsed_redirect_url.scheme(), "http" | "https")
            || parsed_redirect_url.host().is_none()
        {
            return Err(ConfigError::InvalidSupabaseInviteRedirectUrl);
        }
        if app_environment != AppEnvironment::Local && parsed_redirect_url.scheme() != "https" {
            return Err(ConfigError::InsecureSupabaseInviteRedirectUrl);
        }

        Ok(Self {
            url,
            secret_key,
            invite_redirect_url,
        })
    }
}

impl fmt::Debug for SupabaseAdminConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SupabaseAdminConfig")
            .field("url", &self.url)
            .field("secret_key", &"[REDACTED]")
            .field("invite_redirect_url", &self.invite_redirect_url)
            .finish()
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

    #[test]
    fn requires_supabase_auth_configuration() {
        let error = SupabaseAuthConfig::from_lookup(AppEnvironment::Local, |_| None)
            .expect_err("authentication configuration must be required by the API");

        assert_eq!(error, ConfigError::MissingSupabaseUrl);
    }

    #[test]
    fn reads_and_redacts_supabase_auth_configuration() {
        let config = SupabaseAuthConfig::from_lookup(AppEnvironment::Local, |key| match key {
            "SUPABASE_URL" => Some("https://project.supabase.co".to_owned()),
            "SUPABASE_PUBLISHABLE_KEY" => Some("sb_publishable_test".to_owned()),
            _ => None,
        })
        .expect("authentication configuration must be valid");

        assert_eq!(config.url(), "https://project.supabase.co");
        assert_eq!(config.publishable_key(), "sb_publishable_test");
        assert!(!format!("{config:?}").contains("sb_publishable_test"));
    }

    #[test]
    fn rejects_an_invalid_supabase_url() {
        let error = SupabaseAuthConfig::from_lookup(AppEnvironment::Local, |key| match key {
            "SUPABASE_URL" => Some("not a URL".to_owned()),
            "SUPABASE_PUBLISHABLE_KEY" => Some("sb_publishable_test".to_owned()),
            _ => None,
        })
        .expect_err("an invalid Supabase URL must fail startup validation");

        assert_eq!(error, ConfigError::InvalidSupabaseUrl);
    }

    #[test]
    fn allows_http_supabase_urls_only_in_local_development() {
        let local = SupabaseAuthConfig::from_values(
            AppEnvironment::Local,
            "http://127.0.0.1:54321",
            "sb_publishable_test",
        )
        .expect("local Supabase development may use HTTP");
        assert_eq!(local.url(), "http://127.0.0.1:54321");

        for environment in [AppEnvironment::Staging, AppEnvironment::Production] {
            let error = SupabaseAuthConfig::from_values(
                environment,
                "http://project.supabase.co",
                "sb_publishable_test",
            )
            .expect_err("non-local Supabase authentication must use HTTPS");
            assert_eq!(error, ConfigError::InsecureSupabaseUrl);
        }
    }

    #[test]
    fn reads_and_redacts_supabase_admin_configuration() {
        let secret = "service-role-secret-value";
        let config = SupabaseAdminConfig::from_values(
            AppEnvironment::Staging,
            "https://project.supabase.co",
            secret,
            "https://web.example.com/aceptar-invitacion",
        )
        .expect("Supabase Admin configuration must be valid");

        assert_eq!(config.url(), "https://project.supabase.co");
        assert_eq!(config.secret_key(), secret);
        assert_eq!(
            config.invite_redirect_url(),
            "https://web.example.com/aceptar-invitacion"
        );
        assert!(!format!("{config:?}").contains(secret));
    }

    #[test]
    fn requires_supabase_secret_key_for_access_administration() {
        let error = SupabaseAdminConfig::from_lookup(AppEnvironment::Local, |key| match key {
            "SUPABASE_URL" => Some("http://127.0.0.1:54321".to_owned()),
            "SUPABASE_INVITE_REDIRECT_URL" => {
                Some("http://127.0.0.1:3000/aceptar-invitacion".to_owned())
            }
            _ => None,
        })
        .expect_err("Supabase Admin credentials must be required by the API");

        assert_eq!(error, ConfigError::MissingSupabaseSecretKey);
    }

    #[test]
    fn requires_and_validates_the_invitation_redirect_url() {
        let missing = SupabaseAdminConfig::from_lookup(AppEnvironment::Local, |key| match key {
            "SUPABASE_URL" => Some("http://127.0.0.1:54321".to_owned()),
            "SUPABASE_SECRET_KEY" => Some("secret".to_owned()),
            _ => None,
        })
        .expect_err("invitation redirect must be explicit");
        assert_eq!(missing, ConfigError::MissingSupabaseInviteRedirectUrl);

        let insecure = SupabaseAdminConfig::from_values(
            AppEnvironment::Staging,
            "https://project.supabase.co",
            "secret",
            "http://web.example.com/aceptar-invitacion",
        )
        .expect_err("staging invitation redirect must use HTTPS");
        assert_eq!(insecure, ConfigError::InsecureSupabaseInviteRedirectUrl);
    }
}
