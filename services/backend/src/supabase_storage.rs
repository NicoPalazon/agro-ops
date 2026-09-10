use std::{fmt, time::Duration};

use async_trait::async_trait;
use reqwest::{Client, Url, header::HeaderValue};
use serde::{Deserialize, Serialize};

use crate::{
    config::SupabaseAdminConfig,
    documents::{DocumentStorage, DocumentStorageError, PrivateDownloadAccess},
};

const STORAGE_REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_ACCESS_SECONDS: u16 = 60;

pub struct SupabaseDocumentStorage {
    client: Client,
    storage_endpoint: Url,
    object_endpoint: Url,
    sign_endpoint: Url,
    secret_key: HeaderValue,
    authorization: HeaderValue,
}

impl SupabaseDocumentStorage {
    pub fn new(config: &SupabaseAdminConfig) -> Result<Self, DocumentStorageError> {
        let client = Client::builder()
            .timeout(STORAGE_REQUEST_TIMEOUT)
            .build()
            .map_err(|_| DocumentStorageError::Unavailable)?;
        Self::with_client(config, client)
    }

    fn with_client(
        config: &SupabaseAdminConfig,
        client: Client,
    ) -> Result<Self, DocumentStorageError> {
        let mut base_url =
            Url::parse(config.url()).map_err(|_| DocumentStorageError::Unavailable)?;
        if !base_url.path().ends_with('/') {
            base_url.set_path(&format!("{}/", base_url.path()));
        }
        let storage_endpoint = base_url
            .join("storage/v1/")
            .map_err(|_| DocumentStorageError::Unavailable)?;
        let object_endpoint = storage_endpoint
            .join("object/")
            .map_err(|_| DocumentStorageError::Unavailable)?;
        let sign_endpoint = storage_endpoint
            .join("object/sign/")
            .map_err(|_| DocumentStorageError::Unavailable)?;
        let mut secret_key = HeaderValue::from_str(config.secret_key())
            .map_err(|_| DocumentStorageError::Unavailable)?;
        secret_key.set_sensitive(true);
        let mut authorization = HeaderValue::from_str(&format!("Bearer {}", config.secret_key()))
            .map_err(|_| DocumentStorageError::Unavailable)?;
        authorization.set_sensitive(true);
        Ok(Self {
            client,
            storage_endpoint,
            object_endpoint,
            sign_endpoint,
            secret_key,
            authorization,
        })
    }

    fn object_url(&self, bucket: &str, key: &str) -> Result<Url, DocumentStorageError> {
        self.object_endpoint
            .join(&format!("{bucket}/{key}"))
            .map_err(|_| DocumentStorageError::Unavailable)
    }

    fn sign_url(&self, bucket: &str, key: &str) -> Result<Url, DocumentStorageError> {
        self.sign_endpoint
            .join(&format!("{bucket}/{key}"))
            .map_err(|_| DocumentStorageError::Unavailable)
    }
}

impl fmt::Debug for SupabaseDocumentStorage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SupabaseDocumentStorage")
            .field("storage_endpoint", &self.storage_endpoint)
            .field("object_endpoint", &self.object_endpoint)
            .field("sign_endpoint", &self.sign_endpoint)
            .field("secret_key", &"[REDACTED]")
            .field("authorization", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

#[async_trait]
impl DocumentStorage for SupabaseDocumentStorage {
    async fn put(
        &self,
        bucket: &str,
        key: &str,
        mime_type: &str,
        content: &[u8],
    ) -> Result<(), DocumentStorageError> {
        let response = self
            .client
            .post(self.object_url(bucket, key)?)
            .header("apikey", self.secret_key.clone())
            .header("authorization", self.authorization.clone())
            .header("x-upsert", "false")
            .header("content-type", mime_type)
            .body(content.to_vec())
            .send()
            .await
            .map_err(|_| DocumentStorageError::Unavailable)?;
        response
            .status()
            .is_success()
            .then_some(())
            .ok_or(DocumentStorageError::Unavailable)
    }

    async fn create_download_access(
        &self,
        bucket: &str,
        key: &str,
    ) -> Result<PrivateDownloadAccess, DocumentStorageError> {
        let response = self
            .client
            .post(self.sign_url(bucket, key)?)
            .header("apikey", self.secret_key.clone())
            .header("authorization", self.authorization.clone())
            .json(&SignedUrlRequest {
                expires_in: DOWNLOAD_ACCESS_SECONDS,
            })
            .send()
            .await
            .map_err(|_| DocumentStorageError::Unavailable)?;
        if !response.status().is_success() {
            return Err(DocumentStorageError::Unavailable);
        }
        let signed = response
            .json::<SignedUrlResponse>()
            .await
            .map_err(|_| DocumentStorageError::Unavailable)?;
        let url = self
            .storage_endpoint
            .join(signed.signed_url.trim_start_matches('/'))
            .map_err(|_| DocumentStorageError::Unavailable)?;
        Ok(PrivateDownloadAccess {
            url: url.to_string(),
            expires_in_seconds: DOWNLOAD_ACCESS_SECONDS,
        })
    }

    async fn delete(&self, bucket: &str, key: &str) -> Result<(), DocumentStorageError> {
        let response = self
            .client
            .delete(self.object_url(bucket, key)?)
            .header("apikey", self.secret_key.clone())
            .header("authorization", self.authorization.clone())
            .send()
            .await
            .map_err(|_| DocumentStorageError::Unavailable)?;
        response
            .status()
            .is_success()
            .then_some(())
            .ok_or(DocumentStorageError::Unavailable)
    }
}

#[derive(Serialize)]
struct SignedUrlRequest {
    #[serde(rename = "expiresIn")]
    expires_in: u16,
}

#[derive(Deserialize)]
struct SignedUrlResponse {
    #[serde(rename = "signedURL")]
    signed_url: String,
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::{
        Json, Router,
        body::Bytes,
        extract::{OriginalUri, State},
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use tokio::{net::TcpListener, task::JoinHandle};

    use super::*;
    use crate::config::AppEnvironment;

    const SECRET: &str = "sb_secret_storage_adapter_test";

    type CapturedRequest = (String, HeaderMap, Vec<u8>);

    #[derive(Clone)]
    struct MockStorage {
        status: StatusCode,
        requests: Arc<Mutex<Vec<CapturedRequest>>>,
    }

    async fn capture(
        State(mock): State<MockStorage>,
        headers: HeaderMap,
        OriginalUri(uri): OriginalUri,
        body: Bytes,
    ) -> (StatusCode, Json<serde_json::Value>) {
        mock.requests.lock().expect("capture lock must work").push((
            uri.path().to_owned(),
            headers,
            body.to_vec(),
        ));
        (
            mock.status,
            Json(serde_json::json!({
                "signedURL": "/object/sign/documentos_privados/organizaciones/x/documentos/y/contenido?token=temporary"
            })),
        )
    }

    async fn mock_server(status: StatusCode) -> (String, MockStorage, JoinHandle<()>) {
        let mock = MockStorage {
            status,
            requests: Arc::new(Mutex::new(Vec::new())),
        };
        let app = Router::new()
            .route("/storage/v1/object/{*key}", post(capture).delete(capture))
            .route("/storage/v1/object/sign/{*key}", post(capture))
            .with_state(mock.clone());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("mock listener must bind");
        let address = listener.local_addr().expect("mock address must resolve");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("mock server must run");
        });
        (format!("http://{address}"), mock, task)
    }

    fn storage(url: String) -> SupabaseDocumentStorage {
        let config = SupabaseAdminConfig::from_values(
            AppEnvironment::Local,
            url,
            SECRET,
            "http://localhost/aceptar-invitacion",
        )
        .expect("test configuration must be valid");
        SupabaseDocumentStorage::with_client(&config, Client::new())
            .expect("storage adapter must construct")
    }

    #[tokio::test]
    async fn uploads_deletes_and_creates_short_lived_private_access_without_exposing_secrets() {
        let (url, mock, task) = mock_server(StatusCode::OK).await;
        let storage = storage(url);
        let key = "organizaciones/00000000-0000-0000-0000-000000000001/documentos/00000000-0000-0000-0000-000000000002/contenido";
        storage
            .put("documentos_privados", key, "application/pdf", b"content")
            .await
            .expect("put must succeed");
        let download = storage
            .create_download_access("documentos_privados", key)
            .await
            .expect("signed access must succeed");
        storage
            .delete("documentos_privados", key)
            .await
            .expect("delete must succeed");

        assert_eq!(download.expires_in_seconds, DOWNLOAD_ACCESS_SECONDS);
        assert!(download.url.contains("/storage/v1/object/sign/"));
        assert!(!format!("{storage:?}").contains(SECRET));
        let requests = mock.requests.lock().expect("capture lock must work");
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[0].2, b"content");
        for (_, headers, _) in requests.iter() {
            assert_eq!(headers["apikey"], SECRET);
            assert_eq!(headers["authorization"], format!("Bearer {SECRET}"));
        }
        task.abort();
    }

    #[tokio::test]
    async fn storage_failures_are_sanitized_for_put_sign_and_cleanup_delete() {
        let (url, _, task) = mock_server(StatusCode::INTERNAL_SERVER_ERROR).await;
        let storage = storage(url);
        let key = "organizaciones/00000000-0000-0000-0000-000000000001/documentos/00000000-0000-0000-0000-000000000002/contenido";
        assert_eq!(
            storage
                .put("documentos_privados", key, "application/pdf", b"content")
                .await,
            Err(DocumentStorageError::Unavailable)
        );
        assert_eq!(
            storage
                .create_download_access("documentos_privados", key)
                .await,
            Err(DocumentStorageError::Unavailable)
        );
        assert_eq!(
            storage.delete("documentos_privados", key).await,
            Err(DocumentStorageError::Unavailable)
        );
        task.abort();
    }
}
