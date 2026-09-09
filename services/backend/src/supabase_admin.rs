use std::{collections::HashMap, fmt, time::Duration};

use async_trait::async_trait;
use reqwest::{Client, StatusCode, Url, header::HeaderValue};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::SupabaseAdminConfig;

const ADMIN_REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const USERS_PER_PAGE: usize = 1000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalAuthUser {
    pub subject: Uuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalIdentityAdminError {
    Rejected,
    AmbiguousIdentity,
    Unavailable,
}

#[async_trait]
pub trait ExternalIdentityAdmin: Send + Sync {
    async fn resolve_or_invite(
        &self,
        email: &str,
        full_name: &str,
    ) -> Result<ExternalAuthUser, ExternalIdentityAdminError>;

    async fn correos_electronicos_por_sujeto(
        &self,
        subjects: &[Uuid],
    ) -> Result<HashMap<Uuid, String>, ExternalIdentityAdminError>;
}

pub struct SupabaseIdentityAdmin {
    client: Client,
    users_endpoint: Url,
    invite_endpoint: Url,
    invite_redirect_url: String,
    secret_key: HeaderValue,
}

impl SupabaseIdentityAdmin {
    pub fn new(config: &SupabaseAdminConfig) -> Result<Self, ExternalIdentityAdminError> {
        let client = Client::builder()
            .timeout(ADMIN_REQUEST_TIMEOUT)
            .build()
            .map_err(|_| ExternalIdentityAdminError::Unavailable)?;
        Self::with_client(config, client)
    }

    fn with_client(
        config: &SupabaseAdminConfig,
        client: Client,
    ) -> Result<Self, ExternalIdentityAdminError> {
        let mut base_url =
            Url::parse(config.url()).map_err(|_| ExternalIdentityAdminError::Unavailable)?;
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }

        let users_endpoint = base_url
            .join("auth/v1/admin/users")
            .map_err(|_| ExternalIdentityAdminError::Unavailable)?;
        let invite_endpoint = base_url
            .join("auth/v1/invite")
            .map_err(|_| ExternalIdentityAdminError::Unavailable)?;
        let mut secret_key = HeaderValue::from_str(config.secret_key())
            .map_err(|_| ExternalIdentityAdminError::Unavailable)?;
        secret_key.set_sensitive(true);

        Ok(Self {
            client,
            users_endpoint,
            invite_endpoint,
            invite_redirect_url: config.invite_redirect_url().to_owned(),
            secret_key,
        })
    }

    async fn find_by_email(
        &self,
        email: &str,
    ) -> Result<Option<ExternalAuthUser>, ExternalIdentityAdminError> {
        let mut page = 1usize;
        let mut found = None;

        loop {
            let response = self
                .client
                .get(self.users_endpoint.clone())
                .header("apikey", self.secret_key.clone())
                .query(&[("page", page), ("per_page", USERS_PER_PAGE)])
                .send()
                .await
                .map_err(|_| ExternalIdentityAdminError::Unavailable)?;

            if !response.status().is_success() {
                return Err(ExternalIdentityAdminError::Unavailable);
            }

            let users = response
                .json::<SupabaseUsersResponse>()
                .await
                .map_err(|_| ExternalIdentityAdminError::Unavailable)?
                .users;
            for user in &users {
                if user
                    .email
                    .as_deref()
                    .is_some_and(|candidate| candidate.eq_ignore_ascii_case(email))
                {
                    let subject = Uuid::parse_str(&user.id)
                        .map_err(|_| ExternalIdentityAdminError::Unavailable)?;
                    if found.replace(ExternalAuthUser { subject }).is_some() {
                        return Err(ExternalIdentityAdminError::AmbiguousIdentity);
                    }
                }
            }

            if users.len() < USERS_PER_PAGE {
                return Ok(found);
            }
            page += 1;
        }
    }

    async fn fetch_correos_electronicos_por_sujeto(
        &self,
        subjects: &[Uuid],
    ) -> Result<HashMap<Uuid, String>, ExternalIdentityAdminError> {
        let subjects: std::collections::HashSet<Uuid> = subjects.iter().copied().collect();
        let mut page = 1usize;
        let mut emails = HashMap::new();

        while emails.len() < subjects.len() {
            let response = self
                .client
                .get(self.users_endpoint.clone())
                .header("apikey", self.secret_key.clone())
                .query(&[("page", page), ("per_page", USERS_PER_PAGE)])
                .send()
                .await
                .map_err(|_| ExternalIdentityAdminError::Unavailable)?;
            if !response.status().is_success() {
                return Err(ExternalIdentityAdminError::Unavailable);
            }
            let users = response
                .json::<SupabaseUsersResponse>()
                .await
                .map_err(|_| ExternalIdentityAdminError::Unavailable)?
                .users;
            for user in &users {
                let Ok(subject) = Uuid::parse_str(&user.id) else {
                    continue;
                };
                if let Some(email) = subjects
                    .contains(&subject)
                    .then_some(user.email.as_deref())
                    .flatten()
                    .filter(|email| !email.trim().is_empty())
                {
                    emails.insert(subject, email.to_owned());
                }
            }
            if users.len() < USERS_PER_PAGE {
                break;
            }
            page += 1;
        }

        Ok(emails)
    }

    async fn invite(
        &self,
        email: &str,
        full_name: &str,
    ) -> Result<ExternalAuthUser, ExternalIdentityAdminError> {
        let response = self
            .client
            .post(self.invite_endpoint.clone())
            .header("apikey", self.secret_key.clone())
            .query(&[("redirect_to", self.invite_redirect_url.as_str())])
            .json(&InviteRequest {
                email,
                data: InviteMetadata {
                    nombre_completo: full_name,
                    full_name,
                },
            })
            .send()
            .await
            .map_err(|_| ExternalIdentityAdminError::Unavailable)?;

        if response.status().is_success() {
            let payload = response
                .json::<SupabaseInviteResponse>()
                .await
                .map_err(|_| ExternalIdentityAdminError::Unavailable)?;
            return payload
                .subject()
                .map(|subject| ExternalAuthUser { subject });
        }

        if matches!(
            response.status(),
            StatusCode::BAD_REQUEST | StatusCode::CONFLICT | StatusCode::UNPROCESSABLE_ENTITY
        ) {
            return Err(ExternalIdentityAdminError::Rejected);
        }

        Err(ExternalIdentityAdminError::Unavailable)
    }
}

impl fmt::Debug for SupabaseIdentityAdmin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SupabaseIdentityAdmin")
            .field("users_endpoint", &self.users_endpoint)
            .field("invite_endpoint", &self.invite_endpoint)
            .field("invite_redirect_url", &self.invite_redirect_url)
            .field("secret_key", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
struct SupabaseUsersResponse {
    users: Vec<SupabaseUser>,
}

#[derive(Deserialize)]
struct SupabaseUser {
    id: String,
    email: Option<String>,
}

#[derive(Deserialize)]
struct SupabaseInviteResponse {
    id: Option<String>,
    user: Option<SupabaseUser>,
}

impl SupabaseInviteResponse {
    fn subject(self) -> Result<Uuid, ExternalIdentityAdminError> {
        self.id
            .or_else(|| self.user.map(|user| user.id))
            .and_then(|id| Uuid::parse_str(&id).ok())
            .ok_or(ExternalIdentityAdminError::Unavailable)
    }
}

#[derive(Serialize)]
struct InviteRequest<'a> {
    email: &'a str,
    data: InviteMetadata<'a>,
}

#[derive(Serialize)]
struct InviteMetadata<'a> {
    nombre_completo: &'a str,
    #[serde(rename = "full_name")]
    full_name: &'a str,
}

#[async_trait]
impl ExternalIdentityAdmin for SupabaseIdentityAdmin {
    async fn resolve_or_invite(
        &self,
        email: &str,
        full_name: &str,
    ) -> Result<ExternalAuthUser, ExternalIdentityAdminError> {
        if let Some(user) = self.find_by_email(email).await? {
            return Ok(user);
        }

        match self.invite(email, full_name).await {
            Ok(user) => Ok(user),
            Err(ExternalIdentityAdminError::Rejected) => self
                .find_by_email(email)
                .await?
                .ok_or(ExternalIdentityAdminError::Rejected),
            Err(error) => Err(error),
        }
    }

    async fn correos_electronicos_por_sujeto(
        &self,
        subjects: &[Uuid],
    ) -> Result<HashMap<Uuid, String>, ExternalIdentityAdminError> {
        self.fetch_correos_electronicos_por_sujeto(subjects).await
    }
}

#[derive(Debug)]
pub struct UnavailableExternalIdentityAdmin;

#[async_trait]
impl ExternalIdentityAdmin for UnavailableExternalIdentityAdmin {
    async fn resolve_or_invite(
        &self,
        _email: &str,
        _full_name: &str,
    ) -> Result<ExternalAuthUser, ExternalIdentityAdminError> {
        Err(ExternalIdentityAdminError::Unavailable)
    }

    async fn correos_electronicos_por_sujeto(
        &self,
        _subjects: &[Uuid],
    ) -> Result<HashMap<Uuid, String>, ExternalIdentityAdminError> {
        Err(ExternalIdentityAdminError::Unavailable)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Router,
        extract::State,
        http::{HeaderMap, StatusCode, header},
        routing::{get, post},
    };
    use tokio::{net::TcpListener, task::JoinHandle};

    use super::*;
    use crate::config::AppEnvironment;

    const SECRET_KEY: &str = "sb_secret_admin_transport_test_3d9a2f";

    #[derive(Debug)]
    struct CapturedAdminRequest {
        endpoint: &'static str,
        api_key_matches_secret: bool,
        has_authorization: bool,
    }

    #[derive(Clone)]
    struct MockSupabaseAdminState {
        secret_key: HeaderValue,
        users_response: String,
        invite_response: String,
        requests: Arc<Mutex<Vec<CapturedAdminRequest>>>,
    }

    struct MockSupabaseAdmin {
        adapter: SupabaseIdentityAdmin,
        requests: Arc<Mutex<Vec<CapturedAdminRequest>>>,
        task: JoinHandle<()>,
    }

    impl Drop for MockSupabaseAdmin {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    fn capture_request(state: &MockSupabaseAdminState, endpoint: &'static str, headers: HeaderMap) {
        state
            .requests
            .lock()
            .expect("mock request lock must be available")
            .push(CapturedAdminRequest {
                endpoint,
                api_key_matches_secret: headers
                    .get("apikey")
                    .is_some_and(|value| value == state.secret_key),
                has_authorization: headers.contains_key(header::AUTHORIZATION),
            });
    }

    async fn users(
        State(state): State<MockSupabaseAdminState>,
        headers: HeaderMap,
    ) -> (StatusCode, [(header::HeaderName, &'static str); 1], String) {
        capture_request(&state, "users", headers);
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            state.users_response,
        )
    }

    async fn invite(
        State(state): State<MockSupabaseAdminState>,
        headers: HeaderMap,
    ) -> (StatusCode, [(header::HeaderName, &'static str); 1], String) {
        capture_request(&state, "invite", headers);
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            state.invite_response,
        )
    }

    async fn mock_supabase_admin(
        users_response: String,
        invite_response: String,
    ) -> MockSupabaseAdmin {
        let secret_key = HeaderValue::from_static(SECRET_KEY);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let state = MockSupabaseAdminState {
            secret_key,
            users_response,
            invite_response,
            requests: Arc::clone(&requests),
        };
        let app = Router::new()
            .route("/auth/v1/admin/users", get(users))
            .route("/auth/v1/invite", post(invite))
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
        let config = SupabaseAdminConfig::from_values(
            AppEnvironment::Local,
            format!("http://{address}"),
            SECRET_KEY,
            "http://127.0.0.1:3000/aceptar-invitacion",
        )
        .expect("mock Supabase configuration must be valid");
        let client = Client::builder()
            .timeout(Duration::from_millis(100))
            .build()
            .expect("mock admin client must build");

        MockSupabaseAdmin {
            adapter: SupabaseIdentityAdmin::with_client(&config, client)
                .expect("mock admin adapter must build"),
            requests,
            task,
        }
    }

    #[test]
    fn privileged_credential_is_redacted_from_adapter_debug_output() {
        let config = SupabaseAdminConfig::from_values(
            AppEnvironment::Local,
            "http://127.0.0.1:54321",
            SECRET_KEY,
            "http://127.0.0.1:3000/aceptar-invitacion",
        )
        .expect("test configuration must be valid");
        let adapter = SupabaseIdentityAdmin::new(&config).expect("adapter must build");

        assert!(!format!("{adapter:?}").contains(SECRET_KEY));
        assert!(!format!("{:?}", ExternalIdentityAdminError::Unavailable).contains(SECRET_KEY));
    }

    #[tokio::test]
    async fn lookup_sends_opaque_admin_secret_only_in_apikey_header() {
        let subject = Uuid::new_v4();
        let mock = mock_supabase_admin(
            format!(r#"{{"users":[{{"id":"{subject}","email":"user@example.com"}}]}}"#),
            r#"{"id":"unused"}"#.to_owned(),
        )
        .await;

        assert_eq!(
            mock.adapter
                .resolve_or_invite("user@example.com", "User Example")
                .await,
            Ok(ExternalAuthUser { subject })
        );

        let requests = mock
            .requests
            .lock()
            .expect("mock request lock must be available");
        assert_eq!(requests.len(), 1);
        let lookup = &requests[0];
        assert_eq!(lookup.endpoint, "users");
        assert!(lookup.api_key_matches_secret);
        assert!(!lookup.has_authorization);
        assert!(!format!("{requests:?}").contains(SECRET_KEY));
    }

    #[tokio::test]
    async fn invite_sends_opaque_admin_secret_only_in_apikey_header() {
        let subject = Uuid::new_v4();
        let mock = mock_supabase_admin(
            r#"{"users":[]}"#.to_owned(),
            format!(r#"{{"id":"{subject}"}}"#),
        )
        .await;

        assert_eq!(
            mock.adapter
                .resolve_or_invite("new-user@example.com", "New User")
                .await,
            Ok(ExternalAuthUser { subject })
        );

        let requests = mock
            .requests
            .lock()
            .expect("mock request lock must be available");
        let invite = requests
            .iter()
            .find(|request| request.endpoint == "invite")
            .expect("invite request must be captured");
        assert!(invite.api_key_matches_secret);
        assert!(!invite.has_authorization);
        assert!(!format!("{requests:?}").contains(SECRET_KEY));
    }

    #[tokio::test]
    async fn returned_admin_errors_do_not_expose_the_opaque_secret() {
        let mock = mock_supabase_admin(
            r#"{"users":[{"id":"not-a-uuid","email":"user@example.com"}]}"#.to_owned(),
            r#"{"id":"unused"}"#.to_owned(),
        )
        .await;

        let error = mock
            .adapter
            .resolve_or_invite("user@example.com", "User Example")
            .await
            .expect_err("an invalid Supabase subject must be unavailable");

        assert_eq!(error, ExternalIdentityAdminError::Unavailable);
        assert!(!format!("{error:?}").contains(SECRET_KEY));
    }

    #[tokio::test]
    async fn email_enrichment_returns_resolved_matches_when_results_are_incomplete() {
        let resolved_subject = Uuid::new_v4();
        let empty_email_subject = Uuid::new_v4();
        let deleted_subject = Uuid::new_v4();
        let mock = mock_supabase_admin(
            format!(
                r#"{{"users":[{{"id":"{resolved_subject}","email":"resolved@example.com"}},{{"id":"{empty_email_subject}","email":"   "}},{{"id":"not-a-uuid","email":"ignored@example.com"}}]}}"#
            ),
            r#"{"id":"unused"}"#.to_owned(),
        )
        .await;

        let emails = mock
            .adapter
            .correos_electronicos_por_sujeto(&[
                resolved_subject,
                empty_email_subject,
                deleted_subject,
            ])
            .await
            .expect("incomplete Supabase results must remain usable for display enrichment");

        assert_eq!(emails.len(), 1);
        assert_eq!(
            emails.get(&resolved_subject).map(String::as_str),
            Some("resolved@example.com")
        );
        assert!(!emails.contains_key(&empty_email_subject));
        assert!(!emails.contains_key(&deleted_subject));
    }

    #[test]
    fn accepts_both_supported_supabase_invitation_response_shapes() {
        let subject = Uuid::new_v4();
        for payload in [
            format!(r#"{{"id":"{subject}"}}"#),
            format!(r#"{{"user":{{"id":"{subject}"}}}}"#),
        ] {
            let response: SupabaseInviteResponse =
                serde_json::from_str(&payload).expect("Supabase payload must deserialize");
            assert_eq!(response.subject(), Ok(subject));
        }
    }
}
