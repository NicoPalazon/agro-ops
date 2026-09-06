use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, StatusCode, Url, header::HeaderValue};
use serde::Deserialize;

use crate::config::SupabaseAuthConfig;

pub const SUPABASE_AUTH_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthenticatedUser {
    pub id: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum VerifyAccessTokenError {
    Invalid,
    Unavailable,
}

#[async_trait]
pub trait AccessTokenVerifier: Send + Sync {
    async fn verify(&self, access_token: &str)
    -> Result<AuthenticatedUser, VerifyAccessTokenError>;
}

/// Verifies access tokens with Supabase Auth itself. This deliberately uses the
/// publishable API key, not a service-role key, so it follows the project's
/// active Supabase JWT signing-key configuration (including asymmetric keys).
pub struct SupabaseAuthVerifier {
    client: Client,
    user_endpoint: Url,
    publishable_key: HeaderValue,
}

impl SupabaseAuthVerifier {
    pub fn new(config: &SupabaseAuthConfig) -> Result<Self, VerifyAccessTokenError> {
        let client = Client::builder()
            .timeout(SUPABASE_AUTH_REQUEST_TIMEOUT)
            .build()
            .map_err(|_| VerifyAccessTokenError::Unavailable)?;

        Self::with_client(config, client)
    }

    fn with_client(
        config: &SupabaseAuthConfig,
        client: Client,
    ) -> Result<Self, VerifyAccessTokenError> {
        let mut base_url = Url::parse(config.url()).map_err(|_| VerifyAccessTokenError::Invalid)?;
        if !base_url.path().ends_with('/') {
            let path = format!("{}/", base_url.path());
            base_url.set_path(&path);
        }
        let user_endpoint = base_url
            .join("auth/v1/user")
            .map_err(|_| VerifyAccessTokenError::Invalid)?;
        let publishable_key = HeaderValue::from_str(config.publishable_key())
            .map_err(|_| VerifyAccessTokenError::Invalid)?;

        Ok(Self {
            client,
            user_endpoint,
            publishable_key,
        })
    }
}

#[derive(Deserialize)]
struct SupabaseUserResponse {
    id: String,
}

#[async_trait]
impl AccessTokenVerifier for SupabaseAuthVerifier {
    async fn verify(
        &self,
        access_token: &str,
    ) -> Result<AuthenticatedUser, VerifyAccessTokenError> {
        let response = self
            .client
            .get(self.user_endpoint.clone())
            .header("apikey", self.publishable_key.clone())
            .bearer_auth(access_token)
            .send()
            .await
            .map_err(|_| VerifyAccessTokenError::Unavailable)?;

        if response.status() == StatusCode::UNAUTHORIZED
            || response.status() == StatusCode::FORBIDDEN
        {
            return Err(VerifyAccessTokenError::Invalid);
        }
        if !response.status().is_success() {
            return Err(VerifyAccessTokenError::Unavailable);
        }

        let user = response
            .json::<SupabaseUserResponse>()
            .await
            .map_err(|_| VerifyAccessTokenError::Invalid)?;

        (!user.id.trim().is_empty())
            .then_some(AuthenticatedUser { id: user.id })
            .ok_or(VerifyAccessTokenError::Invalid)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Router,
        extract::State,
        http::{HeaderMap, StatusCode, header},
        response::IntoResponse,
        routing::get,
    };
    use tokio::{net::TcpListener, task::JoinHandle};

    use super::*;
    use crate::config::AppEnvironment;

    const PUBLISHABLE_KEY: &str = "sb_publishable_test";
    const ACCESS_TOKEN: &str = "test-user-access-token";

    #[derive(Clone)]
    struct MockUserResponse {
        status: StatusCode,
        body: &'static str,
        never_respond: bool,
    }

    #[derive(Clone)]
    struct MockSupabaseState {
        response: MockUserResponse,
        headers: Arc<Mutex<Option<HeaderMap>>>,
    }

    struct MockSupabase {
        verifier: SupabaseAuthVerifier,
        headers: Arc<Mutex<Option<HeaderMap>>>,
        task: JoinHandle<()>,
    }

    impl Drop for MockSupabase {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn user(State(state): State<MockSupabaseState>, headers: HeaderMap) -> impl IntoResponse {
        *state
            .headers
            .lock()
            .expect("mock request lock must be available") = Some(headers);

        if state.response.never_respond {
            std::future::pending::<()>().await;
        }

        (
            state.response.status,
            [(header::CONTENT_TYPE, "application/json")],
            state.response.body,
        )
    }

    async fn mock_supabase(response: MockUserResponse, timeout: Duration) -> MockSupabase {
        let headers = Arc::new(Mutex::new(None));
        let state = MockSupabaseState {
            response,
            headers: Arc::clone(&headers),
        };
        let app = Router::new()
            .route("/auth/v1/user", get(user))
            .with_state(state);
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock Supabase listener must bind");
        let address = listener
            .local_addr()
            .expect("mock Supabase listener must expose its address");
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let config = SupabaseAuthConfig::from_values(
            AppEnvironment::Local,
            format!("http://{address}"),
            PUBLISHABLE_KEY,
        )
        .expect("mock Supabase configuration must be valid");
        let client = Client::builder()
            .timeout(timeout)
            .build()
            .expect("mock verifier client must build");

        MockSupabase {
            verifier: SupabaseAuthVerifier::with_client(&config, client)
                .expect("mock verifier must build"),
            headers,
            task,
        }
    }

    fn response(status: StatusCode, body: &'static str) -> MockUserResponse {
        MockUserResponse {
            status,
            body,
            never_respond: false,
        }
    }

    #[tokio::test]
    async fn verifies_a_supabase_identity_with_bearer_and_publishable_key() {
        let mock = mock_supabase(
            response(StatusCode::OK, r#"{"id":"user-id"}"#),
            Duration::from_millis(100),
        )
        .await;

        let identity = mock
            .verifier
            .verify(ACCESS_TOKEN)
            .await
            .expect("valid Supabase user must authenticate");

        assert_eq!(identity.id, "user-id");
        let headers = mock
            .headers
            .lock()
            .expect("mock request lock must be available");
        let headers = headers.as_ref().expect("verification must call Supabase");
        assert_eq!(
            headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok()),
            Some("Bearer test-user-access-token")
        );
        assert_eq!(
            headers.get("apikey").and_then(|value| value.to_str().ok()),
            Some(PUBLISHABLE_KEY)
        );
    }

    #[tokio::test]
    async fn rejects_supabase_unauthorized_and_forbidden_responses() {
        for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
            let mock = mock_supabase(response(status, r#"{}"#), Duration::from_millis(100)).await;

            assert_eq!(
                mock.verifier.verify(ACCESS_TOKEN).await,
                Err(VerifyAccessTokenError::Invalid)
            );
        }
    }

    #[tokio::test]
    async fn rejects_malformed_successful_identity_payloads() {
        let mock = mock_supabase(
            response(StatusCode::OK, r#"{"id":false}"#),
            Duration::from_millis(100),
        )
        .await;

        assert_eq!(
            mock.verifier.verify(ACCESS_TOKEN).await,
            Err(VerifyAccessTokenError::Invalid)
        );
    }

    #[tokio::test]
    async fn fails_closed_when_supabase_returns_a_server_error() {
        let mock = mock_supabase(
            response(StatusCode::INTERNAL_SERVER_ERROR, r#"{}"#),
            Duration::from_millis(100),
        )
        .await;

        assert_eq!(
            mock.verifier.verify(ACCESS_TOKEN).await,
            Err(VerifyAccessTokenError::Unavailable)
        );
    }

    #[tokio::test]
    async fn fails_closed_when_supabase_verification_times_out() {
        let mock = mock_supabase(
            MockUserResponse {
                status: StatusCode::OK,
                body: r#"{"id":"user-id"}"#,
                never_respond: true,
            },
            Duration::from_millis(20),
        )
        .await;

        assert_eq!(
            mock.verifier.verify(ACCESS_TOKEN).await,
            Err(VerifyAccessTokenError::Unavailable)
        );
    }
}
