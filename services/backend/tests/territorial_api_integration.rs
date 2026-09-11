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
const SOURCE_A: &str = "-34, -60\n-34.01, -60\n-34.01, -59.99\n-34, -59.99\n-34, -60";
const SOURCE_B: &str = "-34, -59.98\n-34.01, -59.98\n-34.01, -59.97\n-34, -59.97\n-34, -59.98";
const SOURCE_OVERLAP: &str =
    "-34, -59.995\n-34.01, -59.995\n-34.01, -59.985\n-34, -59.985\n-34, -59.995";

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

type GeographicSourceEvidenceSnapshot = (Uuid, Vec<u8>, String, Vec<u8>, Uuid);

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

async fn principal(db: &PgPool, permission_codes: &[&str]) -> Principal {
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
    if !permission_codes.is_empty() {
        let permission_ids: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM public.permisos WHERE codigo = ANY($1) ORDER BY codigo",
        )
        .bind(permission_codes)
        .fetch_all(db)
        .await
        .expect("territorial permissions must be present");
        assert_eq!(permission_ids.len(), permission_codes.len());
        for permission_id in permission_ids {
            sqlx::query("INSERT INTO public.roles_permisos (rol_id, permiso_id) VALUES ($1, $2)")
                .bind(role_id)
                .bind(permission_id)
                .execute(db)
                .await
                .expect("territorial permission must grant");
        }
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

async fn post_json(
    state: AppState,
    path: &str,
    subject: Option<Uuid>,
    body: Value,
) -> axum::response::Response {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if let Some(subject) = subject {
        request = request.header("authorization", format!("Bearer test:{subject}"));
    }
    app(state)
        .oneshot(
            request
                .body(Body::from(body.to_string()))
                .expect("request must build"),
        )
        .await
        .expect("request must complete")
}

async fn post_json_with_idempotency(
    state: AppState,
    path: &str,
    subject: Option<Uuid>,
    body: Value,
    idempotency_key: &str,
) -> axum::response::Response {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json")
        .header("idempotency-key", idempotency_key);
    if let Some(subject) = subject {
        request = request.header("authorization", format!("Bearer test:{subject}"));
    }
    app(state)
        .oneshot(
            request
                .body(Body::from(body.to_string()))
                .expect("request must build"),
        )
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

async fn create_grouping_establishment(db: &PgPool, principal: Principal, code: &str) -> Uuid {
    sqlx::query_scalar(
        r#"
        INSERT INTO public.establecimientos (
            organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por
        )
        VALUES (
            $1, $2, 'Campo para agrupación',
            ST_Multi(ST_GeomFromText('POLYGON((-60 -34, -60 -34.01, -59.99 -34.01, -59.99 -34, -60 -34))', 4326)),
            'manual', $3
        )
        RETURNING id
        "#,
    )
    .bind(principal.organization_id)
    .bind(code)
    .bind(principal.user_id)
    .fetch_one(db)
    .await
    .expect("grouping establishment must insert")
}

async fn import_senasa_source(
    db: &PgPool,
    principal: Principal,
    establecimiento_id: Uuid,
    external_id: &str,
    source_text: &str,
    idempotency_key: &str,
) -> Uuid {
    let imported = post_json_with_idempotency(
        state(db.clone()),
        &format!("/territorio/establecimientos/{establecimiento_id}/fuentes-geograficas/senasa"),
        Some(principal.subject),
        serde_json::json!({
            "external_id": external_id,
            "texto_poligono": source_text,
        }),
        idempotency_key,
    )
    .await;
    assert_eq!(imported.status(), StatusCode::CREATED);
    Uuid::parse_str(json_body(imported).await["fuente"]["id"].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn territorial_read_api_enforces_scope_and_preserves_canonical_geography_and_history() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let authorized = principal(&db, &["territorio:ver"]).await;
    let fixture = create_territory(&db, authorized).await;
    let unauthorized = principal(&db, &[]).await;
    let other = principal(&db, &["territorio:ver"]).await;
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

#[tokio::test]
async fn senasa_polygon_preview_api_normalizes_validates_and_never_persists_territorial_state() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let creator = principal(&db, &["territorio:crear"]).await;
    let reader = principal(&db, &["territorio:ver"]).await;
    let source = serde_json::json!({
        "texto_poligono": "-33.7601, -59.81266\n-33.76887, -59.79991\n-33.78036, -59.81339\n-33.7601, -59.81266"
    });
    let before: (i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*)::bigint FROM public.establecimientos WHERE organizacion_id = $1),
            (SELECT COUNT(*)::bigint FROM public.lotes_base WHERE organizacion_id = $1),
            (SELECT COUNT(*)::bigint FROM public.external_references WHERE organizacion_id = $1),
            (SELECT COUNT(*)::bigint FROM public.audit_events WHERE organizacion_id = $1)
        "#,
    )
    .bind(creator.organization_id)
    .fetch_one(&db)
    .await
    .expect("pre-preview state must be queryable");

    assert_eq!(
        post_json(
            state(db.clone()),
            "/territorio/previsualizaciones/senasa-poligono",
            None,
            source.clone(),
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        post_json(
            state(db.clone()),
            "/territorio/previsualizaciones/senasa-poligono",
            Some(reader.subject),
            source.clone(),
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );

    let preview = post_json(
        state(db.clone()),
        "/territorio/previsualizaciones/senasa-poligono",
        Some(creator.subject),
        source,
    )
    .await;
    assert_eq!(preview.status(), StatusCode::OK);
    let preview = json_body(preview).await;
    assert_eq!(preview["cantidad_pares_coordenadas_fuente"], 4);
    assert_eq!(preview["tipo_geometria"], "MultiPolygon");
    assert_eq!(preview["srid"], 4326);
    assert_eq!(preview["valida"], true);
    assert_eq!(preview["geometria"]["type"], "MultiPolygon");
    assert_eq!(
        preview["geometria"]["coordinates"][0][0][0],
        serde_json::json!([-59.81266, -33.7601])
    );

    let bow_tie = post_json(
        state(db.clone()),
        "/territorio/previsualizaciones/senasa-poligono",
        Some(creator.subject),
        serde_json::json!({ "texto_poligono": "0, 0\n1, 1\n0, 1\n1, 0\n0, 0" }),
    )
    .await;
    assert_eq!(bow_tie.status(), StatusCode::BAD_REQUEST);
    let bow_tie = json_body(bow_tie).await;
    assert_eq!(bow_tie["codigo"], "topologia_invalida");
    assert!(
        bow_tie["detalle"]
            .as_str()
            .unwrap()
            .contains("Self-intersection")
    );

    let malformed = post_json(
        state(db.clone()),
        "/territorio/previsualizaciones/senasa-poligono",
        Some(creator.subject),
        serde_json::json!({ "texto_poligono": "-33.7," }),
    )
    .await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(malformed).await["codigo"], "coordenada_faltante");

    let after: (i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*)::bigint FROM public.establecimientos WHERE organizacion_id = $1),
            (SELECT COUNT(*)::bigint FROM public.lotes_base WHERE organizacion_id = $1),
            (SELECT COUNT(*)::bigint FROM public.external_references WHERE organizacion_id = $1),
            (SELECT COUNT(*)::bigint FROM public.audit_events WHERE organizacion_id = $1)
        "#,
    )
    .bind(creator.organization_id)
    .fetch_one(&db)
    .await
    .expect("post-preview state must be queryable");
    assert_eq!(after, before);
}

#[tokio::test]
async fn senasa_geographic_sources_are_immutable_provenance_and_never_replace_canonical_geometry() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let creator = principal(&db, &["territorio:crear", "territorio:ver"]).await;
    let fixture = create_territory(&db, creator).await;
    let path = format!(
        "/territorio/establecimientos/{}/fuentes-geograficas/senasa",
        fixture.establishment_id
    );
    let source_text =
        "-33.7601, -59.81266\n-33.76887, -59.79991\n-33.78036, -59.81339\n-33.7601, -59.81266";
    let request = serde_json::json!({
        "external_id": "RENSPA-FUENTE-1",
        "nombre_externo": "Campo fuente",
        "texto_poligono": source_text,
    });
    let canonical_before: Vec<u8> = sqlx::query_scalar(
        "SELECT ST_AsEWKB(geometria) FROM public.establecimientos WHERE id = $1",
    )
    .bind(fixture.establishment_id)
    .fetch_one(&db)
    .await
    .expect("canonical establishment geometry must be readable");

    assert_eq!(
        post_json_with_idempotency(
            state(db.clone()),
            &path,
            None,
            request.clone(),
            "senasa-source-unauthenticated",
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let denied = principal(&db, &[]).await;
    assert_eq!(
        post_json_with_idempotency(
            state(db.clone()),
            &path,
            Some(denied.subject),
            request.clone(),
            "senasa-source-denied",
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );

    let created = post_json_with_idempotency(
        state(db.clone()),
        &path,
        Some(creator.subject),
        request.clone(),
        "senasa-source-create-1",
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let created = json_body(created).await;
    assert_eq!(created["creada"], true);
    assert_eq!(created["fuente"]["external_id"], "RENSPA-FUENTE-1");
    assert_eq!(created["fuente"]["texto_fuente_original"], source_text);
    assert_eq!(created["fuente"]["tipo_origen"], "senasa_renspa");
    assert_eq!(
        created["fuente"]["version_parser"],
        "senasa_pasted_polygon_v1"
    );
    assert_eq!(created["fuente"]["geometria"]["type"], "MultiPolygon");
    assert_eq!(
        created["fuente"]["geometria"]["coordinates"][0][0][0],
        serde_json::json!([-59.81266, -33.7601])
    );
    assert_eq!(
        created["fuente"]["huella_sha256"].as_str().unwrap().len(),
        64
    );
    let source_id = created["fuente"]["id"].as_str().unwrap().to_owned();
    let external_reference_id = created["fuente"]["external_reference_id"]
        .as_str()
        .unwrap()
        .to_owned();

    let source_row: (String, i32, bool, bool, i32, String, Uuid, Uuid) = sqlx::query_as(
        r#"
        SELECT GeometryType(geometria), ST_SRID(geometria), ST_IsValid(geometria),
               ST_IsEmpty(geometria), octet_length(huella_sha256), version_parser,
               creado_por, external_reference_id
        FROM public.fuentes_geograficas WHERE id = $1
        "#,
    )
    .bind(Uuid::parse_str(&source_id).unwrap())
    .fetch_one(&db)
    .await
    .expect("source evidence must persist with normalized geometry");
    assert_eq!(
        source_row,
        (
            "MULTIPOLYGON".to_owned(),
            4326,
            true,
            false,
            32,
            "senasa_pasted_polygon_v1".to_owned(),
            creator.user_id,
            Uuid::parse_str(&external_reference_id).unwrap(),
        )
    );

    let whitespace_retry = serde_json::json!({
        "external_id": "RENSPA-FUENTE-1",
        "nombre_externo": "Campo fuente",
        "texto_poligono": "  -33.7601, -59.81266  \n\n-33.76887 , -59.79991\n -33.78036, -59.81339\n-33.7601, -59.81266 ",
    });
    let duplicate = post_json_with_idempotency(
        state(db.clone()),
        &path,
        Some(creator.subject),
        whitespace_retry,
        "senasa-source-whitespace-retry",
    )
    .await;
    assert_eq!(duplicate.status(), StatusCode::OK);
    let duplicate = json_body(duplicate).await;
    assert_eq!(duplicate["creada"], false);
    assert_eq!(duplicate["fuente"]["id"], source_id);

    let replay = post_json_with_idempotency(
        state(db.clone()),
        &path,
        Some(creator.subject),
        request.clone(),
        "senasa-source-create-1",
    )
    .await;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(json_body(replay).await["fuente"]["id"], source_id);

    let same_geometry_other_renspa = post_json_with_idempotency(
        state(db.clone()),
        &path,
        Some(creator.subject),
        serde_json::json!({
            "external_id": "RENSPA-FUENTE-2",
            "nombre_externo": "Campo fuente",
            "texto_poligono": source_text,
        }),
        "senasa-source-other-reference",
    )
    .await;
    assert_eq!(same_geometry_other_renspa.status(), StatusCode::CREATED);
    let same_geometry_other_renspa = json_body(same_geometry_other_renspa).await;
    assert_eq!(same_geometry_other_renspa["creada"], true);
    let other_source_id = same_geometry_other_renspa["fuente"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let concurrent_request = serde_json::json!({
        "external_id": "RENSPA-CONCURRENTE",
        "texto_poligono": source_text,
    });
    let (concurrent_first, concurrent_second) = tokio::join!(
        post_json_with_idempotency(
            state(db.clone()),
            &path,
            Some(creator.subject),
            concurrent_request.clone(),
            "senasa-source-concurrent-a",
        ),
        post_json_with_idempotency(
            state(db.clone()),
            &path,
            Some(creator.subject),
            concurrent_request,
            "senasa-source-concurrent-b",
        )
    );
    let concurrent_statuses = [concurrent_first.status(), concurrent_second.status()];
    assert!(concurrent_statuses.contains(&StatusCode::CREATED));
    assert!(concurrent_statuses.contains(&StatusCode::OK));
    let concurrent_first = json_body(concurrent_first).await;
    let concurrent_second = json_body(concurrent_second).await;
    assert_eq!(
        concurrent_first["fuente"]["id"],
        concurrent_second["fuente"]["id"]
    );
    let concurrent_source_id = concurrent_first["fuente"]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let invalid = post_json_with_idempotency(
        state(db.clone()),
        &path,
        Some(creator.subject),
        serde_json::json!({
            "external_id": "RENSPA-BOWTIE",
            "texto_poligono": "0, 0\n1, 1\n0, 1\n1, 0\n0, 0",
        }),
        "senasa-source-invalid-topology",
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(json_body(invalid).await["codigo"], "topologia_invalida");

    let canonical_after: Vec<u8> = sqlx::query_scalar(
        "SELECT ST_AsEWKB(geometria) FROM public.establecimientos WHERE id = $1",
    )
    .bind(fixture.establishment_id)
    .fetch_one(&db)
    .await
    .expect("canonical establishment geometry must remain readable");
    assert_eq!(canonical_after, canonical_before);
    let source_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.fuentes_geograficas WHERE organizacion_id = $1",
    )
    .bind(creator.organization_id)
    .fetch_one(&db)
    .await
    .expect("source count must be queryable");
    assert_eq!(source_count, 3);
    let imported_reference_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.external_references WHERE organizacion_id = $1 AND external_id = ANY($2)",
    )
    .bind(creator.organization_id)
    .bind(["RENSPA-FUENTE-1", "RENSPA-FUENTE-2", "RENSPA-CONCURRENTE"])
    .fetch_one(&db)
    .await
    .expect("imported external reference count must be queryable");
    assert_eq!(imported_reference_count, 3);
    let audit_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.audit_events WHERE organizacion_id = $1 AND accion = 'fuente_geografica.senasa_confirmada' AND actor_usuario_id = $2",
    )
    .bind(creator.organization_id)
    .bind(creator.user_id)
    .fetch_one(&db)
    .await
    .expect("source audit count must be queryable");
    assert_eq!(audit_count, 3);

    let direct_invalid_geometry = sqlx::query(
        r#"
        INSERT INTO public.fuentes_geograficas (
            organizacion_id, establecimiento_id, external_reference_id, tipo_origen,
            nombre_externo, texto_fuente_original, geometria, huella_sha256,
            version_parser, creado_por
        )
        VALUES ($1, $2, $3, 'senasa_renspa', NULL, '0, 0',
                ST_Multi(ST_GeomFromText('POLYGON((0 0, 1 1, 0 1, 1 0, 0 0))', 4326)),
                decode(repeat('ab', 32), 'hex'), 'senasa_pasted_polygon_v1', $4)
        "#,
    )
    .bind(creator.organization_id)
    .bind(fixture.establishment_id)
    .bind(Uuid::parse_str(&external_reference_id).unwrap())
    .bind(creator.user_id)
    .execute(&db)
    .await
    .expect_err("database constraints must reject invalid source geometry");
    assert_eq!(
        direct_invalid_geometry
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("23514"),
    );

    let list = response(
        state(db.clone()),
        &format!(
            "/territorio/establecimientos/{}/fuentes-geograficas",
            fixture.establishment_id
        ),
        Some(creator.subject),
    )
    .await;
    assert_eq!(list.status(), StatusCode::OK);
    let listed_sources = json_body(list).await;
    let listed_sources = listed_sources.as_array().unwrap();
    let mut listed_identities = listed_sources
        .iter()
        .map(|source| {
            (
                source["external_id"].as_str().unwrap().to_owned(),
                source["id"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    listed_identities.sort_unstable();
    let mut expected_identities = vec![
        ("RENSPA-FUENTE-1".to_owned(), source_id.clone()),
        ("RENSPA-FUENTE-2".to_owned(), other_source_id),
        ("RENSPA-CONCURRENTE".to_owned(), concurrent_source_id),
    ];
    expected_identities.sort_unstable();
    assert_eq!(listed_identities, expected_identities);

    let second_establishment_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM public.establecimientos WHERE organizacion_id = $1 AND codigo = 'Z_CAMPO'",
    )
    .bind(creator.organization_id)
    .fetch_one(&db)
    .await
    .expect("second establishment must exist");
    let conflict = post_json_with_idempotency(
        state(db.clone()),
        &format!(
            "/territorio/establecimientos/{second_establishment_id}/fuentes-geograficas/senasa"
        ),
        Some(creator.subject),
        request,
        "senasa-source-reference-conflict",
    )
    .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);

    let other = principal(&db, &["territorio:crear"]).await;
    let cross_organization = post_json_with_idempotency(
        state(db.clone()),
        &path,
        Some(other.subject),
        serde_json::json!({
            "external_id": "RENSPA-OTHER-ORGANIZATION",
            "texto_poligono": source_text,
        }),
        "senasa-source-cross-organization",
    )
    .await;
    assert_eq!(cross_organization.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn confirmed_sources_define_exact_canonical_union_with_atomic_replacement_and_scope() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let creator = principal(&db, &["territorio:crear", "territorio:ver"]).await;
    let establishment_id = create_grouping_establishment(&db, creator, "AGRUPACION").await;
    let grouping_path =
        format!("/territorio/establecimientos/{establishment_id}/fuentes-geograficas-confirmadas");
    let source_a = import_senasa_source(
        &db,
        creator,
        establishment_id,
        "RENSPA-AGRUPACION-A",
        SOURCE_A,
        "grouping-import-a",
    )
    .await;
    let source_b = import_senasa_source(
        &db,
        creator,
        establishment_id,
        "RENSPA-AGRUPACION-B",
        SOURCE_B,
        "grouping-import-b",
    )
    .await;
    let source_overlap = import_senasa_source(
        &db,
        creator,
        establishment_id,
        "RENSPA-AGRUPACION-SUPERPUESTA",
        SOURCE_OVERLAP,
        "grouping-import-overlap",
    )
    .await;
    let evidence_before: Vec<GeographicSourceEvidenceSnapshot> = sqlx::query_as(
        r#"
        SELECT id, ST_AsEWKB(geometria), texto_fuente_original, huella_sha256,
               external_reference_id
        FROM public.fuentes_geograficas
        WHERE id = ANY($1::uuid[])
        ORDER BY id
        "#,
    )
    .bind([source_a, source_b, source_overlap])
    .fetch_all(&db)
    .await
    .expect("source evidence snapshot must be readable");
    let references_before: Vec<(Uuid, String, Uuid)> = sqlx::query_as(
        r#"
        SELECT id, external_id, entidad_id
        FROM public.external_references
        WHERE organizacion_id = $1
          AND external_id = ANY($2::text[])
        ORDER BY id
        "#,
    )
    .bind(creator.organization_id)
    .bind([
        "RENSPA-AGRUPACION-A",
        "RENSPA-AGRUPACION-B",
        "RENSPA-AGRUPACION-SUPERPUESTA",
    ])
    .fetch_all(&db)
    .await
    .expect("external reference snapshot must be readable");

    let one_source_body = serde_json::json!({ "fuente_geografica_ids": [source_a] });
    assert_eq!(
        post_json_with_idempotency(
            state(db.clone()),
            &grouping_path,
            None,
            one_source_body.clone(),
            "grouping-unauthenticated",
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let denied = principal(&db, &[]).await;
    assert_eq!(
        post_json_with_idempotency(
            state(db.clone()),
            &grouping_path,
            Some(denied.subject),
            one_source_body.clone(),
            "grouping-forbidden",
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let duplicate_selection = post_json_with_idempotency(
        state(db.clone()),
        &grouping_path,
        Some(creator.subject),
        serde_json::json!({ "fuente_geografica_ids": [source_a, source_a] }),
        "grouping-duplicate-selection",
    )
    .await;
    assert_eq!(duplicate_selection.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(duplicate_selection).await["codigo"],
        "fuente_confirmada_duplicada"
    );
    let empty_selection = post_json_with_idempotency(
        state(db.clone()),
        &grouping_path,
        Some(creator.subject),
        serde_json::json!({ "fuente_geografica_ids": [] }),
        "grouping-empty-selection",
    )
    .await;
    assert_eq!(empty_selection.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(empty_selection).await["codigo"],
        "fuentes_confirmadas_requeridas"
    );

    let one_source = post_json_with_idempotency(
        state(db.clone()),
        &grouping_path,
        Some(creator.subject),
        one_source_body,
        "grouping-one-source",
    )
    .await;
    assert_eq!(one_source.status(), StatusCode::OK);
    let one_source = json_body(one_source).await;
    assert_eq!(one_source["actualizada"], true);
    assert_eq!(one_source["superposicion_detectada"], false);
    assert_eq!(one_source["geometria_canonica"]["type"], "MultiPolygon");
    let canonical_one: (bool, String, i32) = sqlx::query_as(
        r#"
        SELECT ST_Equals(establecimiento.geometria, fuente.geometria),
               GeometryType(establecimiento.geometria), ST_SRID(establecimiento.geometria)
        FROM public.establecimientos AS establecimiento
        JOIN public.fuentes_geograficas AS fuente ON fuente.id = $2
        WHERE establecimiento.id = $1
        "#,
    )
    .bind(establishment_id)
    .bind(source_a)
    .fetch_one(&db)
    .await
    .expect("single-source canonical geometry must be readable");
    assert_eq!(canonical_one, (true, "MULTIPOLYGON".to_owned(), 4326));

    let establishment_read = response(
        state(db.clone()),
        &format!("/territorio/establecimientos/{establishment_id}"),
        Some(creator.subject),
    )
    .await;
    assert_eq!(establishment_read.status(), StatusCode::OK);
    assert_eq!(
        json_body(establishment_read).await["geometria"],
        one_source["geometria_canonica"]
    );

    let disjoint = post_json_with_idempotency(
        state(db.clone()),
        &grouping_path,
        Some(creator.subject),
        serde_json::json!({ "fuente_geografica_ids": [source_a, source_b] }),
        "grouping-disjoint",
    )
    .await;
    assert_eq!(disjoint.status(), StatusCode::OK);
    let disjoint = json_body(disjoint).await;
    assert_eq!(disjoint["actualizada"], true);
    assert_eq!(disjoint["superposicion_detectada"], false);
    let disjoint_geometry: (bool, String, i32, i32, bool, bool) = sqlx::query_as(
        r#"
        SELECT ST_Equals(
                   establecimiento.geometria,
                   (SELECT ST_Multi(ST_UnaryUnion(ST_Collect(geometria)))
                    FROM public.fuentes_geograficas WHERE id = ANY($2::uuid[]))
               ),
               GeometryType(establecimiento.geometria),
               ST_SRID(establecimiento.geometria),
               ST_NumGeometries(establecimiento.geometria),
               NOT ST_Covers(
                   establecimiento.geometria,
                   ST_SetSRID(ST_Point(-59.985, -34.005), 4326)
               ),
               ST_Area(ST_ConvexHull(establecimiento.geometria)::geography)
                   > ST_Area(establecimiento.geometria::geography)
        FROM public.establecimientos AS establecimiento
        WHERE id = $1
        "#,
    )
    .bind(establishment_id)
    .bind([source_a, source_b])
    .fetch_one(&db)
    .await
    .expect("disconnected canonical geometry must be readable");
    assert_eq!(
        disjoint_geometry,
        (true, "MULTIPOLYGON".to_owned(), 4326, 2, true, true)
    );
    let excludes_unselected_overlap: bool = sqlx::query_scalar(
        r#"
        SELECT NOT ST_Covers(
            geometria,
            ST_SetSRID(ST_Point(-59.987, -34.005), 4326)
        )
        FROM public.establecimientos WHERE id = $1
        "#,
    )
    .bind(establishment_id)
    .fetch_one(&db)
    .await
    .expect("explicit contributor geometry must be queryable");
    assert!(excludes_unselected_overlap);

    let disjoint_same_key_replay = post_json_with_idempotency(
        state(db.clone()),
        &grouping_path,
        Some(creator.subject),
        serde_json::json!({ "fuente_geografica_ids": [source_b, source_a] }),
        "grouping-disjoint",
    )
    .await;
    assert_eq!(disjoint_same_key_replay.status(), StatusCode::OK);
    assert_eq!(
        json_body(disjoint_same_key_replay).await["actualizada"],
        true
    );
    let disjoint_retry = post_json_with_idempotency(
        state(db.clone()),
        &grouping_path,
        Some(creator.subject),
        serde_json::json!({ "fuente_geografica_ids": [source_b, source_a] }),
        "grouping-disjoint-semantic-retry",
    )
    .await;
    assert_eq!(disjoint_retry.status(), StatusCode::OK);
    assert_eq!(json_body(disjoint_retry).await["actualizada"], false);
    let audit_after_disjoint: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.audit_events WHERE organizacion_id = $1 AND accion = 'establecimiento.fuentes_geograficas_confirmadas'",
    )
    .bind(creator.organization_id)
    .fetch_one(&db)
    .await
    .expect("grouping audit count must be readable");
    assert_eq!(audit_after_disjoint, 2);

    let overlapping = post_json_with_idempotency(
        state(db.clone()),
        &grouping_path,
        Some(creator.subject),
        serde_json::json!({ "fuente_geografica_ids": [source_a, source_overlap] }),
        "grouping-overlap",
    )
    .await;
    assert_eq!(overlapping.status(), StatusCode::OK);
    let overlapping = json_body(overlapping).await;
    assert_eq!(overlapping["superposicion_detectada"], true);
    assert!(overlapping["area_superpuesta_m2"].as_f64().unwrap() > 0.0);
    assert!(
        overlapping["area_fuentes_m2"].as_f64().unwrap()
            > overlapping["area_canonica_m2"].as_f64().unwrap()
    );
    let exact_overlap_union: bool = sqlx::query_scalar(
        r#"
        SELECT ST_Equals(
            geometria,
            (SELECT ST_Multi(ST_UnaryUnion(ST_Collect(geometria)))
             FROM public.fuentes_geograficas WHERE id = ANY($2::uuid[]))
        )
        FROM public.establecimientos WHERE id = $1
        "#,
    )
    .bind(establishment_id)
    .bind([source_a, source_overlap])
    .fetch_one(&db)
    .await
    .expect("overlap union must be queryable");
    assert!(exact_overlap_union);

    let replace_with_b = post_json_with_idempotency(
        state(db.clone()),
        &grouping_path,
        Some(creator.subject),
        serde_json::json!({ "fuente_geografica_ids": [source_b] }),
        "grouping-replace-with-b",
    )
    .await;
    assert_eq!(replace_with_b.status(), StatusCode::OK);
    assert_eq!(json_body(replace_with_b).await["actualizada"], true);
    let replacement_state: (i64, bool) = sqlx::query_as(
        r#"
        SELECT COUNT(*)::bigint, bool_and(
            contribucion.fuente_geografica_id = $2
            AND ST_Equals(establecimiento.geometria, fuente.geometria)
        )
        FROM public.fuentes_geograficas_contribuciones_canonicas AS contribucion
        JOIN public.establecimientos AS establecimiento
          ON establecimiento.id = contribucion.establecimiento_id
        JOIN public.fuentes_geograficas AS fuente
          ON fuente.id = contribucion.fuente_geografica_id
        WHERE contribucion.establecimiento_id = $1
          AND contribucion.vigente_hasta IS NULL
        "#,
    )
    .bind(establishment_id)
    .bind(source_b)
    .fetch_one(&db)
    .await
    .expect("replacement contributor state must be readable");
    assert_eq!(replacement_state, (1, true));
    let audit_before_replays: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.audit_events WHERE organizacion_id = $1 AND accion = 'establecimiento.fuentes_geograficas_confirmadas'",
    )
    .bind(creator.organization_id)
    .fetch_one(&db)
    .await
    .expect("audit count before replay must be readable");
    let same_key_replay = post_json_with_idempotency(
        state(db.clone()),
        &grouping_path,
        Some(creator.subject),
        serde_json::json!({ "fuente_geografica_ids": [source_b] }),
        "grouping-replace-with-b",
    )
    .await;
    assert_eq!(same_key_replay.status(), StatusCode::OK);
    assert_eq!(json_body(same_key_replay).await["actualizada"], true);
    let semantic_replay = post_json_with_idempotency(
        state(db.clone()),
        &grouping_path,
        Some(creator.subject),
        serde_json::json!({ "fuente_geografica_ids": [source_b] }),
        "grouping-replace-with-b-semantic-retry",
    )
    .await;
    assert_eq!(semantic_replay.status(), StatusCode::OK);
    assert_eq!(json_body(semantic_replay).await["actualizada"], false);
    let audit_after_replays: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.audit_events WHERE organizacion_id = $1 AND accion = 'establecimiento.fuentes_geograficas_confirmadas'",
    )
    .bind(creator.organization_id)
    .fetch_one(&db)
    .await
    .expect("audit count after replay must be readable");
    assert_eq!(audit_after_replays, audit_before_replays);

    let provenance = response(
        state(db.clone()),
        &format!("/territorio/establecimientos/{establishment_id}/fuentes-geograficas"),
        Some(creator.subject),
    )
    .await;
    assert_eq!(provenance.status(), StatusCode::OK);
    let provenance = json_body(provenance).await;
    let confirmed = provenance
        .as_array()
        .unwrap()
        .iter()
        .filter(|source| source["confirmada_para_geometria_canonica"] == true)
        .collect::<Vec<_>>();
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0]["id"], source_b.to_string());
    assert_eq!(confirmed[0]["confirmada_por"], creator.user_id.to_string());

    let evidence_after: Vec<GeographicSourceEvidenceSnapshot> = sqlx::query_as(
        r#"
        SELECT id, ST_AsEWKB(geometria), texto_fuente_original, huella_sha256,
               external_reference_id
        FROM public.fuentes_geograficas
        WHERE id = ANY($1::uuid[])
        ORDER BY id
        "#,
    )
    .bind([source_a, source_b, source_overlap])
    .fetch_all(&db)
    .await
    .expect("source evidence must remain readable");
    assert_eq!(evidence_after, evidence_before);
    let references_after: Vec<(Uuid, String, Uuid)> = sqlx::query_as(
        r#"
        SELECT id, external_id, entidad_id
        FROM public.external_references
        WHERE organizacion_id = $1
          AND external_id = ANY($2::text[])
        ORDER BY id
        "#,
    )
    .bind(creator.organization_id)
    .bind([
        "RENSPA-AGRUPACION-A",
        "RENSPA-AGRUPACION-B",
        "RENSPA-AGRUPACION-SUPERPUESTA",
    ])
    .fetch_all(&db)
    .await
    .expect("external references must remain readable");
    assert_eq!(references_after, references_before);

    let same_organization_other_establishment =
        create_grouping_establishment(&db, creator, "OTRO_CAMPO").await;
    let other_establishment_source = import_senasa_source(
        &db,
        creator,
        same_organization_other_establishment,
        "RENSPA-OTRO-CAMPO",
        SOURCE_A,
        "grouping-import-other-establishment",
    )
    .await;
    let wrong_establishment = post_json_with_idempotency(
        state(db.clone()),
        &grouping_path,
        Some(creator.subject),
        serde_json::json!({ "fuente_geografica_ids": [other_establishment_source] }),
        "grouping-wrong-establishment",
    )
    .await;
    assert_eq!(wrong_establishment.status(), StatusCode::NOT_FOUND);

    let other_organization = principal(&db, &["territorio:crear"]).await;
    let other_organization_establishment =
        create_grouping_establishment(&db, other_organization, "AGRUPACION").await;
    let other_organization_source = import_senasa_source(
        &db,
        other_organization,
        other_organization_establishment,
        "RENSPA-OTRA-ORGANIZACION",
        SOURCE_A,
        "grouping-import-other-organization",
    )
    .await;
    let cross_organization = post_json_with_idempotency(
        state(db.clone()),
        &grouping_path,
        Some(creator.subject),
        serde_json::json!({ "fuente_geografica_ids": [other_organization_source] }),
        "grouping-cross-organization",
    )
    .await;
    assert_eq!(cross_organization.status(), StatusCode::NOT_FOUND);

    let inconsistent_direct_link = sqlx::query(
        r#"
        INSERT INTO public.fuentes_geograficas_contribuciones_canonicas (
            organizacion_id, establecimiento_id, fuente_geografica_id, confirmado_por
        ) VALUES ($1, $2, $3, $4)
        "#,
    )
    .bind(creator.organization_id)
    .bind(establishment_id)
    .bind(other_establishment_source)
    .bind(creator.user_id)
    .execute(&db)
    .await
    .expect_err("database must reject a direct membership that skips canonical versioning");
    assert_eq!(
        inconsistent_direct_link
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("P0001")
    );

    let (concurrent_a, concurrent_overlap) = tokio::join!(
        post_json_with_idempotency(
            state(db.clone()),
            &grouping_path,
            Some(creator.subject),
            serde_json::json!({ "fuente_geografica_ids": [source_a] }),
            "grouping-concurrent-a",
        ),
        post_json_with_idempotency(
            state(db.clone()),
            &grouping_path,
            Some(creator.subject),
            serde_json::json!({ "fuente_geografica_ids": [source_overlap] }),
            "grouping-concurrent-overlap",
        )
    );
    assert_eq!(concurrent_a.status(), StatusCode::OK);
    assert_eq!(concurrent_overlap.status(), StatusCode::OK);
    let final_state: (i64, bool) = sqlx::query_as(
        r#"
        SELECT COUNT(*)::bigint,
               bool_and(ST_Equals(establecimiento.geometria, fuente.geometria))
        FROM public.fuentes_geograficas_contribuciones_canonicas AS contribucion
        JOIN public.establecimientos AS establecimiento
          ON establecimiento.id = contribucion.establecimiento_id
        JOIN public.fuentes_geograficas AS fuente
          ON fuente.id = contribucion.fuente_geografica_id
        WHERE contribucion.establecimiento_id = $1
          AND contribucion.vigente_hasta IS NULL
        "#,
    )
    .bind(establishment_id)
    .fetch_one(&db)
    .await
    .expect("final concurrent grouping state must be readable");
    assert_eq!(final_state, (1, true));
}

#[tokio::test]
async fn geographic_correction_versions_evidence_membership_perimeters_and_uop_history_atomically()
{
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let manager = principal(
        &db,
        &["territorio:crear", "territorio:gestionar", "territorio:ver"],
    )
    .await;
    let denied = principal(&db, &["territorio:crear", "territorio:ver"]).await;
    let origin = create_grouping_establishment(&db, manager, "CORRECCION_ORIGEN").await;
    let target = create_grouping_establishment(&db, manager, "CORRECCION_DESTINO").await;
    let keep = import_senasa_source(
        &db,
        manager,
        origin,
        "RENSPA-CORRECCION-KEEP",
        SOURCE_A,
        "correction-import-keep",
    )
    .await;
    let moved = import_senasa_source(
        &db,
        manager,
        origin,
        "RENSPA-CORRECCION-MOVED",
        SOURCE_B,
        "correction-import-moved",
    )
    .await;
    let target_source = import_senasa_source(
        &db,
        manager,
        target,
        "RENSPA-CORRECCION-TARGET",
        SOURCE_OVERLAP,
        "correction-import-target",
    )
    .await;
    for (establishment_id, source_ids, key) in [
        (origin, vec![keep, moved], "correction-group-origin"),
        (target, vec![target_source], "correction-group-target"),
    ] {
        let grouped = post_json_with_idempotency(
            state(db.clone()),
            &format!(
                "/territorio/establecimientos/{establishment_id}/fuentes-geograficas-confirmadas"
            ),
            Some(manager.subject),
            serde_json::json!({ "fuente_geografica_ids": source_ids }),
            key,
        )
        .await;
        assert_eq!(grouped.status(), StatusCode::OK);
    }

    let base_plot_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO public.lotes_base (
            organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por
        ) VALUES (
            $1, $2, 'HIST', 'Lote histórico',
            ST_Multi(ST_GeomFromText(
                'POLYGON((-59.999 -34.001, -59.999 -34.009, -59.991 -34.009, -59.991 -34.001, -59.999 -34.001))',
                4326
            )), $3
        ) RETURNING id
        "#,
    )
    .bind(manager.organization_id)
    .bind(origin)
    .bind(manager.user_id)
    .fetch_one(&db)
    .await
    .expect("base plot inside retained source must insert");
    let campaign_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.campanas (organizacion_id, codigo, nombre, fecha_inicio, fecha_fin, creado_por) VALUES ($1, 'CORR_2026', 'Campaña histórica', DATE '2026-07-01', DATE '2027-06-30', $2) RETURNING id",
    )
    .bind(manager.organization_id)
    .bind(manager.user_id)
    .fetch_one(&db)
    .await
    .expect("campaign must insert");
    let mut uop_transaction = db.begin().await.expect("UOP transaction must begin");
    let uop_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO public.unidades_operativas (
            organizacion_id, campana_id, establecimiento_id, codigo, nombre, geometria, creado_por
        ) VALUES (
            $1, $2, $3, 'UOP_HIST', 'UOP histórica',
            ST_Multi(ST_GeomFromText(
                'POLYGON((-59.999 -34.001, -59.999 -34.009, -59.991 -34.009, -59.991 -34.001, -59.999 -34.001))',
                4326
            )), $4
        ) RETURNING id
        "#,
    )
    .bind(manager.organization_id)
    .bind(campaign_id)
    .bind(origin)
    .bind(manager.user_id)
    .fetch_one(&mut *uop_transaction)
    .await
    .expect("historical UOP must insert");
    sqlx::query(
        "INSERT INTO public.unidades_operativas_lotes_base (organizacion_id, establecimiento_id, unidad_operativa_id, lote_base_id, creado_por) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(manager.organization_id)
    .bind(origin)
    .bind(uop_id)
    .bind(base_plot_id)
    .bind(manager.user_id)
    .execute(&mut *uop_transaction)
    .await
    .expect("historical UOP link must insert");
    uop_transaction.commit().await.expect("UOP must commit");
    let uop_before: (Uuid, Vec<u8>, Vec<u8>) = sqlx::query_as(
        r#"
        SELECT unidad.establecimiento_geometria_version_id,
               ST_AsEWKB(unidad.geometria), ST_AsEWKB(version.geometria)
        FROM public.unidades_operativas AS unidad
        JOIN public.establecimientos_geometrias_versiones AS version
          ON version.id = unidad.establecimiento_geometria_version_id
        WHERE unidad.id = $1
        "#,
    )
    .bind(uop_id)
    .fetch_one(&db)
    .await
    .expect("historical UOP perimeter reference must be readable");
    let moved_before: (Vec<u8>, String, Vec<u8>, Uuid, Option<String>) = sqlx::query_as(
        "SELECT ST_AsEWKB(geometria), texto_fuente_original, huella_sha256, creado_por, motivo_correccion FROM public.fuentes_geograficas WHERE id = $1",
    )
    .bind(moved)
    .fetch_one(&db)
    .await
    .expect("original evidence must be readable");

    let corrected_geometry = serde_json::json!({
        "type": "MultiPolygon",
        "coordinates": [
            [[[-59.96, -34.0], [-59.96, -34.01], [-59.95, -34.01], [-59.95, -34.0], [-59.96, -34.0]]],
            [[[-59.93, -34.0], [-59.93, -34.01], [-59.92, -34.01], [-59.92, -34.0], [-59.93, -34.0]]]
        ]
    });
    let correction_body = serde_json::json!({
        "establecimiento_destino_id": target,
        "geometry": corrected_geometry,
        "motivo": "Corrección de coordenadas y reasignación confirmada por negocio"
    });
    let correction_path = format!("/territorio/fuentes-geograficas/{moved}/correcciones");
    let preview = post_json(
        state(db.clone()),
        &format!("{correction_path}/previsualizacion"),
        Some(manager.subject),
        correction_body.clone(),
    )
    .await;
    assert_eq!(preview.status(), StatusCode::OK);
    let preview = json_body(preview).await;
    assert_eq!(preview["crea_revision"], true);
    assert_eq!(preview["conflictos"], serde_json::json!([]));
    assert_eq!(preview["impactos"].as_array().unwrap().len(), 2);

    let forbidden = post_json_with_idempotency(
        state(db.clone()),
        &correction_path,
        Some(denied.subject),
        correction_body.clone(),
        "correction-forbidden",
    )
    .await;
    assert_eq!(forbidden.status(), StatusCode::FORBIDDEN);

    let corrected = post_json_with_idempotency(
        state(db.clone()),
        &correction_path,
        Some(manager.subject),
        correction_body.clone(),
        "correction-confirm",
    )
    .await;
    assert_eq!(corrected.status(), StatusCode::OK);
    let corrected = json_body(corrected).await;
    let replacement = Uuid::parse_str(corrected["fuente_activa_id"].as_str().unwrap()).unwrap();
    assert_ne!(replacement, moved);
    assert_eq!(corrected["revision_creada"], true);
    assert_eq!(corrected["aplicada"], true);
    assert_eq!(
        corrected["version_geometria_ids"].as_array().unwrap().len(),
        2
    );

    let moved_after: (Vec<u8>, String, Vec<u8>, Uuid, Option<String>) = sqlx::query_as(
        "SELECT ST_AsEWKB(geometria), texto_fuente_original, huella_sha256, creado_por, motivo_correccion FROM public.fuentes_geograficas WHERE id = $1",
    )
    .bind(moved)
    .fetch_one(&db)
    .await
    .expect("original evidence must remain readable");
    assert_eq!(moved_after, moved_before);
    let lineage: (Uuid, String, String, Option<String>, bool) = sqlx::query_as(
        r#"
        SELECT nueva.reemplaza_fuente_geografica_id, nueva.motivo_correccion, nueva.version_parser,
               referencia.external_id, ST_Equals(nueva.geometria, anterior.geometria)
        FROM public.fuentes_geograficas AS nueva
        JOIN public.fuentes_geograficas AS anterior
          ON anterior.id = nueva.reemplaza_fuente_geografica_id
        LEFT JOIN public.external_references AS referencia
          ON referencia.id = nueva.external_reference_id
        WHERE nueva.id = $1
        "#,
    )
    .bind(replacement)
    .fetch_one(&db)
    .await
    .expect("correction lineage must be queryable");
    assert_eq!(lineage.0, moved);
    assert_eq!(
        lineage.1,
        "Corrección de coordenadas y reasignación confirmada por negocio"
    );
    assert_eq!(lineage.2, "territory_correction_geojson_v1");
    assert_eq!(lineage.3.as_deref(), Some("RENSPA-CORRECCION-MOVED"));
    assert!(!lineage.4);

    let memberships: Vec<(Uuid, Uuid, Option<time::OffsetDateTime>, Option<Uuid>)> =
        sqlx::query_as(
            r#"
            SELECT fuente_geografica_id, establecimiento_id, vigente_hasta,
                   reemplazada_por_contribucion_id
            FROM public.fuentes_geograficas_contribuciones_canonicas
            WHERE fuente_geografica_id = ANY($1::uuid[])
            ORDER BY confirmado_en, id
            "#,
        )
        .bind([moved, replacement])
        .fetch_all(&db)
        .await
        .expect("membership history must be queryable");
    assert_eq!(memberships.len(), 2);
    assert_eq!(memberships[0].0, moved);
    assert_eq!(memberships[0].1, origin);
    assert!(memberships[0].2.is_some());
    assert!(memberships[0].3.is_some());
    assert_eq!(memberships[1].0, replacement);
    assert_eq!(memberships[1].1, target);
    assert!(memberships[1].2.is_none());

    let canonical: (bool, bool, String, i32, bool) = sqlx::query_as(
        r#"
        SELECT
            ST_Equals(origen.geometria, fuente_conservada.geometria),
            ST_Equals(
                destino.geometria,
                (SELECT ST_Multi(ST_UnaryUnion(ST_Collect(fuente.geometria)))
                 FROM public.fuentes_geograficas_contribuciones_canonicas AS contribucion
                 JOIN public.fuentes_geograficas AS fuente
                   ON fuente.id = contribucion.fuente_geografica_id
                 WHERE contribucion.establecimiento_id = $2
                   AND contribucion.vigente_hasta IS NULL)
            ),
            GeometryType(destino.geometria),
            ST_NumGeometries(destino.geometria),
            NOT ST_Covers(destino.geometria, ST_SetSRID(ST_Point(-59.94, -34.005), 4326))
        FROM public.establecimientos AS origen
        JOIN public.establecimientos AS destino ON destino.id = $2
        JOIN public.fuentes_geograficas AS fuente_conservada ON fuente_conservada.id = $3
        WHERE origen.id = $1
        "#,
    )
    .bind(origin)
    .bind(target)
    .bind(keep)
    .fetch_one(&db)
    .await
    .expect("both affected canonical geometries must be exact");
    assert!(canonical.0);
    assert!(canonical.1);
    assert_eq!(canonical.2, "MULTIPOLYGON");
    assert!(canonical.3 >= 2);
    assert!(
        canonical.4,
        "exact union must not fill the gap between components"
    );

    let uop_after: (Uuid, Vec<u8>, Vec<u8>) = sqlx::query_as(
        r#"
        SELECT unidad.establecimiento_geometria_version_id,
               ST_AsEWKB(unidad.geometria), ST_AsEWKB(version.geometria)
        FROM public.unidades_operativas AS unidad
        JOIN public.establecimientos_geometrias_versiones AS version
          ON version.id = unidad.establecimiento_geometria_version_id
        WHERE unidad.id = $1
        "#,
    )
    .bind(uop_id)
    .fetch_one(&db)
    .await
    .expect("historical UOP version must remain readable");
    assert_eq!(uop_after, uop_before);

    let counts_before_replay: (i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*) FROM public.fuentes_geograficas WHERE reemplaza_fuente_geografica_id = $1),
            (SELECT COUNT(*) FROM public.fuentes_geograficas_contribuciones_canonicas WHERE fuente_geografica_id = ANY($2::uuid[])),
            (SELECT COUNT(*) FROM public.establecimientos_geometrias_versiones WHERE id = ANY($3::uuid[])),
            (SELECT COUNT(*) FROM public.audit_events WHERE accion = 'fuente_geografica.corregida_reasignada' AND entidad_id = $4)
        "#,
    )
    .bind(moved)
    .bind([moved, replacement])
    .bind(
        corrected["version_geometria_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| Uuid::parse_str(id.as_str().unwrap()).unwrap())
            .collect::<Vec<_>>(),
    )
    .bind(replacement)
    .fetch_one(&db)
    .await
    .expect("correction effects must be countable");
    assert_eq!(counts_before_replay, (1, 2, 2, 1));
    let replay = post_json_with_idempotency(
        state(db.clone()),
        &correction_path,
        Some(manager.subject),
        correction_body,
        "correction-confirm",
    )
    .await;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(json_body(replay).await["aplicada"], false);
    let counts_after_replay: (i64, i64, i64, i64) = sqlx::query_as(
        r#"
        SELECT
            (SELECT COUNT(*) FROM public.fuentes_geograficas WHERE reemplaza_fuente_geografica_id = $1),
            (SELECT COUNT(*) FROM public.fuentes_geograficas_contribuciones_canonicas WHERE fuente_geografica_id = ANY($2::uuid[])),
            (SELECT COUNT(*) FROM public.establecimientos_geometrias_versiones WHERE id = ANY($3::uuid[])),
            (SELECT COUNT(*) FROM public.audit_events WHERE accion = 'fuente_geografica.corregida_reasignada' AND entidad_id = $4)
        "#,
    )
    .bind(moved)
    .bind([moved, replacement])
    .bind(
        corrected["version_geometria_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| Uuid::parse_str(id.as_str().unwrap()).unwrap())
            .collect::<Vec<_>>(),
    )
    .bind(replacement)
    .fetch_one(&db)
    .await
    .expect("replayed effects must be countable");
    assert_eq!(counts_after_replay, counts_before_replay);
    let audit_actor: Uuid = sqlx::query_scalar(
        "SELECT actor_usuario_id FROM public.audit_events WHERE accion = 'fuente_geografica.corregida_reasignada' AND entidad_id = $1",
    )
    .bind(replacement)
    .fetch_one(&db)
    .await
    .expect("correction audit must identify actor");
    assert_eq!(audit_actor, manager.user_id);

    let origin_state_before_conflicts: (Vec<u8>, i64, i64) = sqlx::query_as(
        r#"
        SELECT ST_AsEWKB(geometria),
               (SELECT COUNT(*) FROM public.fuentes_geograficas),
               (SELECT COUNT(*) FROM public.fuentes_geograficas_contribuciones_canonicas)
        FROM public.establecimientos WHERE id = $1
        "#,
    )
    .bind(origin)
    .fetch_one(&db)
    .await
    .expect("origin state must be readable");
    let move_last = post_json_with_idempotency(
        state(db.clone()),
        &format!("/territorio/fuentes-geograficas/{keep}/correcciones"),
        Some(manager.subject),
        serde_json::json!({
            "establecimiento_destino_id": target,
            "motivo": "Intento de mover la última contribución"
        }),
        "correction-last-source",
    )
    .await;
    assert_eq!(move_last.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(move_last).await["codigo"],
        "establecimiento_sin_geometria_confirmada"
    );
    let excludes_lote = post_json_with_idempotency(
        state(db.clone()),
        &format!("/territorio/fuentes-geograficas/{keep}/correcciones"),
        Some(manager.subject),
        serde_json::json!({
            "establecimiento_destino_id": origin,
            "geometry": {
                "type": "Polygon",
                "coordinates": [[[-59.5, -34.0], [-59.5, -34.01], [-59.49, -34.01], [-59.49, -34.0], [-59.5, -34.0]]]
            },
            "motivo": "Intento que excluiría el lote actual"
        }),
        "correction-excludes-base-plot",
    )
    .await;
    assert_eq!(excludes_lote.status(), StatusCode::CONFLICT);
    let excludes_lote = json_body(excludes_lote).await;
    assert_eq!(
        excludes_lote["codigo"],
        "lotes_base_fuera_geometria_canonica"
    );
    assert!(
        excludes_lote["detalle"]
            .as_str()
            .unwrap()
            .contains(&base_plot_id.to_string())
    );
    let origin_state_after_conflicts: (Vec<u8>, i64, i64) = sqlx::query_as(
        r#"
        SELECT ST_AsEWKB(geometria),
               (SELECT COUNT(*) FROM public.fuentes_geograficas),
               (SELECT COUNT(*) FROM public.fuentes_geograficas_contribuciones_canonicas)
        FROM public.establecimientos WHERE id = $1
        "#,
    )
    .bind(origin)
    .fetch_one(&db)
    .await
    .expect("origin state after rejected corrections must be readable");
    assert_eq!(origin_state_after_conflicts, origin_state_before_conflicts);

    let direct_source_overwrite = sqlx::query(
        "UPDATE public.fuentes_geograficas SET geometria = ST_Multi(ST_GeomFromText('POLYGON((-59 -34, -59 -34.01, -58.99 -34.01, -58.99 -34, -59 -34))', 4326)) WHERE id = $1",
    )
    .bind(moved)
    .execute(&db)
    .await
    .expect_err("immutable source evidence must reject direct overwrite");
    assert_eq!(
        direct_source_overwrite
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("P0001")
    );
    let direct_history_delete = sqlx::query(
        "DELETE FROM public.fuentes_geograficas_contribuciones_canonicas WHERE fuente_geografica_id = $1",
    )
    .bind(moved)
    .execute(&db)
    .await
    .expect_err("historical membership must reject direct deletion");
    assert_eq!(
        direct_history_delete
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("P0001")
    );
}
