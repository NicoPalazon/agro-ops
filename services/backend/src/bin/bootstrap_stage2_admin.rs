use agro_ops_backend::stage2_access_provisioning::{
    ProvisionStage2AccessConfig, bootstrap_stage2_administrator,
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
    let result = bootstrap_stage2_administrator(&database, config.request())
        .await
        .unwrap_or_else(|error| {
            eprintln!("initial administrator bootstrap failed: {error}");
            std::process::exit(1);
        });

    println!(
        "Initial administrator provisioned: organization_id={}, user_id={}, role={}, permissions={}, supabase_subject={}",
        result.organization_id,
        result.user_id,
        result.role_name,
        result.permission_codes.join(","),
        result.supabase_subject,
    );
}
