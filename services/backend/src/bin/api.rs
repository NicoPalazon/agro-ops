use std::net::SocketAddr;

use agro_ops_backend::{
    AppState, app,
    auth::SupabaseAuthVerifier,
    config::{RuntimeConfig, SupabaseAdminConfig, SupabaseAuthConfig},
    shutdown::shutdown_signal,
    supabase_admin::SupabaseIdentityAdmin,
    telemetry::init_tracing,
};
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info};

#[tokio::main]
async fn main() {
    init_tracing();

    let config = RuntimeConfig::from_env().unwrap_or_else(|error| {
        error!(service = "api", %error, "startup configuration invalid");
        std::process::exit(1);
    });

    let auth_config =
        SupabaseAuthConfig::from_env(config.app_environment()).unwrap_or_else(|error| {
            error!(service = "api", %error, "startup authentication configuration invalid");
            std::process::exit(1);
        });
    let auth = SupabaseAuthVerifier::new(&auth_config).unwrap_or_else(|_| {
        error!(
            service = "api",
            "startup authentication configuration invalid"
        );
        std::process::exit(1);
    });
    let admin_config =
        SupabaseAdminConfig::from_env(config.app_environment()).unwrap_or_else(|error| {
            error!(service = "api", %error, "startup access-administration configuration invalid");
            std::process::exit(1);
        });
    let external_identity_admin = SupabaseIdentityAdmin::new(&admin_config).unwrap_or_else(|_| {
        error!(
            service = "api",
            "startup access-administration configuration invalid"
        );
        std::process::exit(1);
    });
    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect_lazy(config.database_url())
        .unwrap_or_else(|_| {
            error!(
                service = "api",
                "invalid configuration: DATABASE_URL must be a valid PostgreSQL connection URL"
            );
            std::process::exit(1);
        });
    let state = AppState {
        db,
        auth: Arc::new(auth),
        external_identity_admin: Arc::new(external_identity_admin),
    };

    let address = SocketAddr::from(([0, 0, 0, 0], config.port()));

    let listener = TcpListener::bind(address).await.unwrap_or_else(|error| {
        error!(service = "api", %error, "failed to bind API listener");
        std::process::exit(1);
    });

    info!(
        service = "api",
        version = env!("CARGO_PKG_VERSION"),
        environment = %config.app_environment(),
        %address,
        "Agro Ops API started"
    );

    axum::serve(listener, app(state))
        .with_graceful_shutdown(async {
            let signal = shutdown_signal().await;
            info!(
                service = "api",
                signal = signal.as_str(),
                "shutdown signal received"
            );
        })
        .await
        .expect("API server failed");

    info!(service = "api", "Agro Ops API stopped");
}
