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
