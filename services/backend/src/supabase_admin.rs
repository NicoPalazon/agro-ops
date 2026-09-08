use std::{fmt, time::Duration};

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
                .header("authorization", bearer_value(&self.secret_key)?)
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

    async fn invite(
        &self,
        email: &str,
        full_name: &str,
    ) -> Result<ExternalAuthUser, ExternalIdentityAdminError> {
        let response = self
            .client
            .post(self.invite_endpoint.clone())
            .header("apikey", self.secret_key.clone())
            .header("authorization", bearer_value(&self.secret_key)?)
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

fn bearer_value(key: &HeaderValue) -> Result<HeaderValue, ExternalIdentityAdminError> {
    let value = key
        .to_str()
        .map_err(|_| ExternalIdentityAdminError::Unavailable)?;
    let mut bearer = HeaderValue::from_str(&format!("Bearer {value}"))
        .map_err(|_| ExternalIdentityAdminError::Unavailable)?;
    bearer.set_sensitive(true);
    Ok(bearer)
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppEnvironment;

    #[test]
    fn privileged_credential_is_redacted_from_adapter_debug_output() {
        let secret = "service-role-secret-that-must-never-leak";
        let config = SupabaseAdminConfig::from_values(
            AppEnvironment::Local,
            "http://127.0.0.1:54321",
            secret,
            "http://127.0.0.1:3000/aceptar-invitacion",
        )
        .expect("test configuration must be valid");
        let adapter = SupabaseIdentityAdmin::new(&config).expect("adapter must build");

        assert!(!format!("{adapter:?}").contains(secret));
        assert!(!format!("{:?}", ExternalIdentityAdminError::Unavailable).contains(secret));
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
