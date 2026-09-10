use std::sync::{Arc, OnceLock};

use agro_ops_backend::{
    AppState, app,
    auth::{AccessTokenVerifier, AuthenticatedUser, VerifyAccessTokenError},
    documents::{DocumentSettings, UnavailableDocumentStorage},
    supabase_admin::UnavailableExternalIdentityAdmin,
};
use async_trait::async_trait;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::{PgPool, postgres::PgPoolOptions};
use tower::ServiceExt;
use uuid::Uuid;

const FIELD: &str = "MULTIPOLYGON(((-60 -34, -60 -34.1, -59.9 -34.1, -59.9 -34, -60 -34)),((-59.8 -34, -59.8 -34.1, -59.7 -34.1, -59.7 -34, -59.8 -34)))";
const BASE_A: &str = "POLYGON((-60 -34, -60 -34.1, -59.95 -34.1, -59.95 -34, -60 -34))";
const BASE_B: &str = "POLYGON((-59.8 -34, -59.8 -34.1, -59.75 -34.1, -59.75 -34, -59.8 -34))";

#[derive(Clone, Copy)]
struct Principal {
    subject: Uuid,
    organization_id: Uuid,
    user_id: Uuid,
}

#[derive(Clone, Copy)]
struct TerritoryFixture {
    establishment_id: Uuid,
    base_plot_a_id: Uuid,
    campaign_id: Uuid,
    operational_unit_id: Uuid,
}

struct TestVerifier;

#[async_trait]
impl AccessTokenVerifier for TestVerifier {
    async fn verify(
        &self,
        access_token: &str,
    ) -> Result<AuthenticatedUser, VerifyAccessTokenError> {
        access_token
            .strip_prefix("test:")
            .filter(|subject| Uuid::parse_str(subject).is_ok())
            .map(|id| AuthenticatedUser { id: id.to_owned() })
            .ok_or(VerifyAccessTokenError::Invalid)
    }
}

fn database_test_lock() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn test_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(6)
        .connect(&std::env::var("DATABASE_URL").expect("DATABASE_URL must be set"))
        .await
        .expect("PostgreSQL/PostGIS with migrations must be available")
}

fn state(db: PgPool) -> AppState {
    AppState {
        db,
        auth: Arc::new(TestVerifier),
        external_identity_admin: Arc::new(UnavailableExternalIdentityAdmin),
        document_storage: Arc::new(UnavailableDocumentStorage),
        document_settings: DocumentSettings::new("documentos_privados", 1024),
    }
}

async fn principal(db: &PgPool, grant_territory_read: bool) -> Principal {
    let organization_id: Uuid =
        sqlx::query_scalar("INSERT INTO public.organizaciones (nombre) VALUES ($1) RETURNING id")
            .bind(format!("Organización territorial API {}", Uuid::new_v4()))
            .fetch_one(db)
            .await
            .expect("organization must insert");
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usuarios (organizacion_id, nombre_completo) VALUES ($1, $2) RETURNING id",
    )
    .bind(organization_id)
    .bind(format!("Usuario territorial API {}", Uuid::new_v4()))
    .fetch_one(db)
    .await
    .expect("user must insert");
    let subject = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO public.identidades_autenticacion_externas (usuario_id, proveedor, sujeto_proveedor) VALUES ($1, 'supabase', $2)",
    )
    .bind(user_id)
    .bind(subject)
    .execute(db)
    .await
    .expect("identity must insert");

    let role_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.roles (organizacion_id, nombre) VALUES ($1, $2) RETURNING id",
    )
    .bind(organization_id)
    .bind(format!("Rol territorial API {}", Uuid::new_v4()))
    .fetch_one(db)
    .await
    .expect("role must insert");
    sqlx::query("INSERT INTO public.usuarios_roles (usuario_id, rol_id) VALUES ($1, $2)")
        .bind(user_id)
        .bind(role_id)
        .execute(db)
        .await
        .expect("current user role must insert");
    if grant_territory_read {
        let permission_id: Uuid =
            sqlx::query_scalar("SELECT id FROM public.permisos WHERE codigo = 'territorio:ver'")
                .fetch_one(db)
                .await
                .expect("territorio:ver migration must be applied");
        sqlx::query("INSERT INTO public.roles_permisos (rol_id, permiso_id) VALUES ($1, $2)")
            .bind(role_id)
            .bind(permission_id)
            .execute(db)
            .await
            .expect("territory read permission must grant");
    }

    Principal {
        subject,
        organization_id,
        user_id,
    }
}

async fn create_territory(db: &PgPool, principal: Principal) -> TerritoryFixture {
    let establishment_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.establecimientos (organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por) VALUES ($1, 'CAMPO', 'Campo API', ST_GeomFromText($2, 4326), 'manual', $3) RETURNING id",
    )
    .bind(principal.organization_id)
    .bind(FIELD)
    .bind(principal.user_id)
    .fetch_one(db)
    .await
    .expect("disconnected multipolygon establishment must insert");
    let base_plot_a_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'A', 'Lote A API', ST_Multi(ST_GeomFromText($3, 4326)), $4) RETURNING id",
    )
    .bind(principal.organization_id)
    .bind(establishment_id)
    .bind(BASE_A)
    .bind(principal.user_id)
    .fetch_one(db)
    .await
    .expect("base plot A must insert");
    sqlx::query(
        "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'B', 'Lote B API', ST_Multi(ST_GeomFromText($3, 4326)), $4)",
    )
    .bind(principal.organization_id)
    .bind(establishment_id)
    .bind(BASE_B)
    .bind(principal.user_id)
    .execute(db)
    .await
    .expect("base plot B must insert");
    for external_id in ["RENSPA-001", "RENSPA-002"] {
        sqlx::query(
            "INSERT INTO public.external_references (organizacion_id, sistema_externo, external_id, entidad_tipo, entidad_id) VALUES ($1, 'senasa', $2, 'establecimiento', $3)",
        )
        .bind(principal.organization_id)
        .bind(external_id)
        .bind(establishment_id)
        .execute(db)
        .await
        .expect("SENASA external reference must insert");
    }
    sqlx::query(
        "INSERT INTO public.establecimientos (organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por) VALUES ($1, 'Z_CAMPO', 'Campo posterior', ST_GeomFromText($2, 4326), 'manual', $3)",
    )
    .bind(principal.organization_id)
    .bind(FIELD)
    .bind(principal.user_id)
    .execute(db)
    .await
    .expect("second establishment must insert");

    let campaign_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.campanas (organizacion_id, codigo, nombre, fecha_inicio, fecha_fin, creado_por) VALUES ($1, 'CAMP_2026', 'Campaña API', DATE '2026-07-01', DATE '2027-06-30', $2) RETURNING id",
    )
    .bind(principal.organization_id)
    .bind(principal.user_id)
    .fetch_one(db)
    .await
    .expect("campaign must insert");

    let mut transaction = db.begin().await.expect("UOP transaction must begin");
    let operational_unit_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.unidades_operativas (organizacion_id, campana_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, $3, 'UOP_A', 'UOP API', ST_Multi(ST_GeomFromText($4, 4326)), $5) RETURNING id",
    )
    .bind(principal.organization_id)
    .bind(campaign_id)
    .bind(establishment_id)
    .bind(BASE_A)
    .bind(principal.user_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("UOP must insert");
    sqlx::query(
        "INSERT INTO public.unidades_operativas_lotes_base (organizacion_id, establecimiento_id, unidad_operativa_id, lote_base_id, creado_por) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(principal.organization_id)
    .bind(establishment_id)
    .bind(operational_unit_id)
    .bind(base_plot_a_id)
    .bind(principal.user_id)
    .execute(&mut *transaction)
    .await
    .expect("UOP base plot link must insert");
    transaction.commit().await.expect("valid UOP must commit");

    let pasture_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usos_territoriales (organizacion_id, codigo, nombre, creado_por) VALUES ($1, 'PASTURA', 'Pastura', $2) RETURNING id",
    )
    .bind(principal.organization_id)
    .bind(principal.user_id)
    .fetch_one(db)
    .await
    .expect("first use must insert");
    let soja_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usos_territoriales (organizacion_id, codigo, nombre, creado_por) VALUES ($1, 'SOJA', 'Soja', $2) RETURNING id",
    )
    .bind(principal.organization_id)
    .bind(principal.user_id)
    .fetch_one(db)
    .await
    .expect("second use must insert");
    for (use_id, start, end) in [
        (pasture_id, "2026-07-01", "2026-10-01"),
        (soja_id, "2027-01-01", "2027-07-01"),
    ] {
        sqlx::query(
            "INSERT INTO public.unidades_operativas_usos (unidad_operativa_id, uso_territorial_id, fecha_inicio, fecha_fin, creado_por) VALUES ($1, $2, $3::date, $4::date, $5)",
        )
        .bind(operational_unit_id)
        .bind(use_id)
        .bind(start)
        .bind(end)
        .bind(principal.user_id)
        .execute(db)
        .await
        .expect("immutable territorial use assignment must insert");
    }

    TerritoryFixture {
        establishment_id,
        base_plot_a_id,
        campaign_id,
        operational_unit_id,
    }
}

async fn response(state: AppState, path: &str, subject: Option<Uuid>) -> axum::response::Response {
    let mut request = Request::builder().method("GET").uri(path);
    if let Some(subject) = subject {
        request = request.header("authorization", format!("Bearer test:{subject}"));
    }
    app(state)
        .oneshot(request.body(Body::empty()).expect("request must build"))
        .await
        .expect("request must complete")
}

async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body must read"),
    )
    .expect("response must be JSON")
}

#[tokio::test]
async fn territorial_read_api_enforces_scope_and_preserves_canonical_geography_and_history() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let authorized = principal(&db, true).await;
    let fixture = create_territory(&db, authorized).await;
    let unauthorized = principal(&db, false).await;
    let other = principal(&db, true).await;
    let other_territory = create_territory(&db, other).await;

    assert_eq!(
        response(state(db.clone()), "/territorio/establecimientos", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        response(
            state(db.clone()),
            "/territorio/establecimientos",
            Some(unauthorized.subject)
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );

    let establishment = response(
        state(db.clone()),
        &format!("/territorio/establecimientos/{}", fixture.establishment_id),
        Some(authorized.subject),
    )
    .await;
    assert_eq!(establishment.status(), StatusCode::OK);
    let establishment = json_body(establishment).await;
    assert!(establishment.get("renspa").is_none());
    assert_eq!(
        establishment["referencias_externas"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        establishment["referencias_externas"][0]["external_id"],
        "RENSPA-001"
    );
    assert_eq!(
        establishment["referencias_externas"][1]["external_id"],
        "RENSPA-002"
    );
    assert_eq!(establishment["geometria"]["type"], "MultiPolygon");
    assert_eq!(
        establishment["geometria"]["coordinates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        establishment["geometria"]["coordinates"][0][0][0],
        serde_json::json!([-60.0, -34.0])
    );
    assert_eq!(establishment["lotes_base"].as_array().unwrap().len(), 2);
    assert_eq!(establishment["lotes_base"][0]["codigo"], "A");
    assert_eq!(
        establishment["lotes_base"][0]["geometria"]["type"],
        "MultiPolygon"
    );

    let establishments = response(
        state(db.clone()),
        "/territorio/establecimientos",
        Some(authorized.subject),
    )
    .await;
    assert_eq!(establishments.status(), StatusCode::OK);
    let establishments = json_body(establishments).await;
    assert_eq!(establishments[0]["codigo"], "CAMPO");
    assert_eq!(establishments[1]["codigo"], "Z_CAMPO");

    assert_eq!(
        response(
            state(db.clone()),
            &format!(
                "/territorio/establecimientos/{}",
                other_territory.establishment_id
            ),
            Some(authorized.subject),
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    let campaign = response(
        state(db.clone()),
        &format!("/territorio/campanas/{}", fixture.campaign_id),
        Some(authorized.subject),
    )
    .await;
    assert_eq!(campaign.status(), StatusCode::OK);
    let campaign = json_body(campaign).await;
    assert_eq!(campaign["fecha_inicio"], "2026-07-01");
    assert_eq!(campaign["fecha_fin"], "2027-06-30");

    let uops = response(
        state(db.clone()),
        &format!(
            "/territorio/campanas/{}/establecimientos/{}/unidades-operativas",
            fixture.campaign_id, fixture.establishment_id
        ),
        Some(authorized.subject),
    )
    .await;
    assert_eq!(uops.status(), StatusCode::OK);
    let uops = json_body(uops).await;
    assert_eq!(uops[0]["campana_id"], fixture.campaign_id.to_string());
    assert_eq!(
        uops[0]["establecimiento_id"],
        fixture.establishment_id.to_string()
    );
    assert_eq!(uops[0]["geometria"]["type"], "MultiPolygon");
    assert_eq!(
        uops[0]["lote_base_ids"],
        serde_json::json!([fixture.base_plot_a_id])
    );

    let uses = response(
        state(db.clone()),
        &format!(
            "/territorio/unidades-operativas/{}/usos",
            fixture.operational_unit_id
        ),
        Some(authorized.subject),
    )
    .await;
    assert_eq!(uses.status(), StatusCode::OK);
    let uses = json_body(uses).await;
    assert_eq!(uses.as_array().unwrap().len(), 2);
    assert_eq!(uses[0]["uso_codigo"], "PASTURA");
    assert_eq!(uses[0]["fecha_inicio"], "2026-07-01");
    assert_eq!(uses[0]["fecha_fin"], "2026-10-01");
    assert_eq!(uses[1]["uso_codigo"], "SOJA");
    assert_eq!(uses[1]["fecha_inicio"], "2027-01-01");
    assert_eq!(uses[1]["fecha_fin"], "2027-07-01");

    let openapi = response(state(db), "/openapi.json", Some(authorized.subject)).await;
    assert_eq!(openapi.status(), StatusCode::OK);
    let openapi = json_body(openapi).await;
    assert!(
        openapi["paths"]
            .get("/territorio/unidades-operativas/{unidad_operativa_id}/usos")
            .is_some()
    );
}
