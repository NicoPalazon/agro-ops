use agro_ops_backend::stage2_access_provisioning::{
    ProvisionStage2AccessConfig, provision_stage2_access,
};
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() {
    let config = ProvisionStage2AccessConfig::from_env().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(1);
    });

    let database = PgPoolOptions::new()
        .max_connections(1)
        .connect(config.database_url())
        .await
        .unwrap_or_else(|_| {
            eprintln!("unable to connect to PostgreSQL using DATABASE_URL");
            std::process::exit(1);
        });

    let result = provision_stage2_access(&database, config.request())
        .await
        .unwrap_or_else(|error| {
            eprintln!("stage 2 access provisioning failed: {error}");
            std::process::exit(1);
        });

    println!(
        "Stage 2 access provisioned: organization_id={}, user_id={}, role={}, permission={}, supabase_subject={}",
        result.organization_id,
        result.user_id,
        result.role_name,
        result.permission_code,
        result.supabase_subject,
    );
}
