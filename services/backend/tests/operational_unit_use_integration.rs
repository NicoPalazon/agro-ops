use std::sync::OnceLock;

use sqlx::{PgPool, postgres::PgPoolOptions};
use uuid::Uuid;

const CHECK: &str = "23514";
const EXCLUSION: &str = "23P01";
const RAISE: &str = "P0001";
const FIELD: &str = "POLYGON((-60 -34, -60 -34.1, -59.9 -34.1, -59.9 -34, -60 -34))";
const BASE_A: &str = "POLYGON((-60 -34, -60 -34.1, -59.95 -34.1, -59.95 -34, -60 -34))";
const BASE_B: &str = "POLYGON((-59.95 -34, -59.95 -34.1, -59.9 -34.1, -59.9 -34, -59.95 -34))";

#[derive(Clone, Copy)]
struct Fixture {
    organization_id: Uuid,
    user_id: Uuid,
    establishment_id: Uuid,
    base_plot_a_id: Uuid,
    base_plot_b_id: Uuid,
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

async fn create_fixture(db: &PgPool) -> Fixture {
    let organization_id: Uuid =
        sqlx::query_scalar("INSERT INTO public.organizaciones (nombre) VALUES ($1) RETURNING id")
            .bind(format!("Organización usos {}", Uuid::new_v4()))
            .fetch_one(db)
            .await
            .expect("organization must insert");
    let user_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.usuarios (organizacion_id, nombre_completo) VALUES ($1, $2) RETURNING id",
    )
    .bind(organization_id)
    .bind(format!("Usuario usos {}", Uuid::new_v4()))
    .fetch_one(db)
    .await
    .expect("user must insert");
    let establishment_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.establecimientos (organizacion_id, codigo, nombre, geometria, origen_geometria, creado_por) VALUES ($1, 'CAMPO', 'Campo de prueba', ST_Multi(ST_GeomFromText($2, 4326)), 'manual', $3) RETURNING id",
    )
    .bind(organization_id)
    .bind(FIELD)
    .bind(user_id)
    .fetch_one(db)
    .await
    .expect("establishment must insert");
    let base_plot_a_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'A', 'Lote A', ST_Multi(ST_GeomFromText($3, 4326)), $4) RETURNING id",
    )
    .bind(organization_id)
    .bind(establishment_id)
    .bind(BASE_A)
    .bind(user_id)
    .fetch_one(db)
    .await
    .expect("base plot A must insert");
    let base_plot_b_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.lotes_base (organizacion_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, 'B', 'Lote B', ST_Multi(ST_GeomFromText($3, 4326)), $4) RETURNING id",
    )
    .bind(organization_id)
    .bind(establishment_id)
    .bind(BASE_B)
    .bind(user_id)
    .fetch_one(db)
    .await
    .expect("base plot B must insert");

    Fixture {
        organization_id,
        user_id,
        establishment_id,
        base_plot_a_id,
        base_plot_b_id,
    }
}

async fn campaign(db: &PgPool, fixture: Fixture, code: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO public.campanas (organizacion_id, codigo, nombre, fecha_inicio, fecha_fin, creado_por) VALUES ($1, $2, $3, DATE '2026-07-01', DATE '2027-06-30', $4) RETURNING id",
    )
    .bind(fixture.organization_id)
    .bind(code)
    .bind(format!("Campaña {code}"))
    .bind(fixture.user_id)
    .fetch_one(db)
    .await
    .expect("campaign must insert")
}

async fn create_uop(
    db: &PgPool,
    fixture: Fixture,
    campaign_id: Uuid,
    code: &str,
    geometry: &str,
    base_plot_id: Uuid,
) -> Uuid {
    let mut transaction = db.begin().await.expect("transaction must begin");
    let uop_id: Uuid = sqlx::query_scalar(
        "INSERT INTO public.unidades_operativas (organizacion_id, campana_id, establecimiento_id, codigo, nombre, geometria, creado_por) VALUES ($1, $2, $3, $4, $5, ST_Multi(ST_GeomFromText($6, 4326)), $7) RETURNING id",
    )
    .bind(fixture.organization_id)
    .bind(campaign_id)
    .bind(fixture.establishment_id)
    .bind(code)
    .bind(format!("UOP {code}"))
    .bind(geometry)
    .bind(fixture.user_id)
    .fetch_one(&mut *transaction)
    .await
    .expect("UOP must insert before deferred validation");
    sqlx::query(
        "INSERT INTO public.unidades_operativas_lotes_base (organizacion_id, establecimiento_id, unidad_operativa_id, lote_base_id, creado_por) VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(fixture.organization_id)
    .bind(fixture.establishment_id)
    .bind(uop_id)
    .bind(base_plot_id)
    .bind(fixture.user_id)
    .execute(&mut *transaction)
    .await
    .expect("base plot link must insert");
    transaction.commit().await.expect("valid UOP must commit");
    uop_id
}

async fn territorial_use(db: &PgPool, fixture: Fixture, code: &str) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO public.usos_territoriales (organizacion_id, codigo, nombre, creado_por) VALUES ($1, $2, $3, $4) RETURNING id",
    )
    .bind(fixture.organization_id)
    .bind(code)
    .bind(format!("Uso {code}"))
    .bind(fixture.user_id)
    .fetch_one(db)
    .await
    .expect("territorial use must insert")
}

async fn assign_use(
    db: &PgPool,
    fixture: Fixture,
    uop_id: Uuid,
    use_id: Uuid,
    start: &str,
    end: &str,
) -> Uuid {
    sqlx::query_scalar(
        "INSERT INTO public.unidades_operativas_usos (unidad_operativa_id, uso_territorial_id, fecha_inicio, fecha_fin, creado_por) VALUES ($1, $2, $3::date, $4::date, $5) RETURNING id",
    )
    .bind(uop_id)
    .bind(use_id)
    .bind(start)
    .bind(end)
    .bind(fixture.user_id)
    .fetch_one(db)
    .await
    .expect("valid territorial use assignment must insert")
}

fn assert_database_error(error: sqlx::Error, expected_sqlstate: &str, message: Option<&str>) {
    let database_error = error
        .as_database_error()
        .expect("expected PostgreSQL database error");
    assert_eq!(database_error.code().as_deref(), Some(expected_sqlstate));
    if let Some(message) = message {
        assert_eq!(database_error.message(), message);
    }
}

#[tokio::test]
async fn territorial_use_vocabulary_is_organization_scoped_and_lifecycle_preserves_history() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let fixture = create_fixture(&db).await;
    let trigo_id = territorial_use(&db, fixture, "TRIGO").await;

    let persisted: (Uuid, String, String, bool) = sqlx::query_as(
        "SELECT organizacion_id, codigo, nombre, activo FROM public.usos_territoriales WHERE id = $1",
    )
    .bind(trigo_id)
    .fetch_one(&db)
    .await
    .expect("created territorial use must persist");
    assert_eq!(persisted.0, fixture.organization_id);
    assert_eq!(persisted.1, "TRIGO");
    assert!(persisted.3);

    let duplicate_error = sqlx::query(
        "INSERT INTO public.usos_territoriales (organizacion_id, codigo, nombre, creado_por) VALUES ($1, 'TRIGO', 'Duplicado', $2)",
    )
    .bind(fixture.organization_id)
    .bind(fixture.user_id)
    .execute(&db)
    .await
    .expect_err("a territorial code must be unique within its organization");
    assert_database_error(duplicate_error, "23505", None);

    let other_fixture = create_fixture(&db).await;
    let other_trigo_id = territorial_use(&db, other_fixture, "TRIGO").await;
    assert_ne!(other_trigo_id, trigo_id);

    let campaign_id = campaign(&db, fixture, "CICLO_USO").await;
    let uop_id = create_uop(
        &db,
        fixture,
        campaign_id,
        "A1",
        BASE_A,
        fixture.base_plot_a_id,
    )
    .await;
    let assignment_id =
        assign_use(&db, fixture, uop_id, trigo_id, "2026-07-01", "2026-12-01").await;
    sqlx::query("UPDATE public.usos_territoriales SET activo = FALSE WHERE id = $1")
        .bind(trigo_id)
        .execute(&db)
        .await
        .expect("a territorial use may be deactivated");
    let history_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.unidades_operativas_usos WHERE id = $1 AND uso_territorial_id = $2",
    )
    .bind(assignment_id)
    .bind(trigo_id)
    .fetch_one(&db)
    .await
    .expect("historical assignment must remain queryable after deactivation");
    assert_eq!(history_count, 1);
}

#[tokio::test]
async fn operational_unit_uses_allow_unassigned_units_sequential_boundaries_and_gaps() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let fixture = create_fixture(&db).await;
    let campaign_id = campaign(&db, fixture, "SECUENCIAS").await;
    let left_uop = create_uop(
        &db,
        fixture,
        campaign_id,
        "A1",
        BASE_A,
        fixture.base_plot_a_id,
    )
    .await;
    let right_uop = create_uop(
        &db,
        fixture,
        campaign_id,
        "B1",
        BASE_B,
        fixture.base_plot_b_id,
    )
    .await;
    let zero_assignments: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.unidades_operativas_usos WHERE unidad_operativa_id = $1",
    )
    .bind(left_uop)
    .fetch_one(&db)
    .await
    .expect("unassigned UOP state must be queryable");
    assert_eq!(zero_assignments, 0);

    let trigo_id = territorial_use(&db, fixture, "TRIGO").await;
    let soja_id = territorial_use(&db, fixture, "SOJA_SEGUNDA").await;
    assign_use(&db, fixture, left_uop, trigo_id, "2026-07-01", "2026-12-01").await;
    assign_use(&db, fixture, left_uop, soja_id, "2026-12-01", "2027-03-01").await;
    assign_use(
        &db,
        fixture,
        right_uop,
        trigo_id,
        "2026-07-01",
        "2026-11-01",
    )
    .await;
    assign_use(&db, fixture, right_uop, soja_id, "2027-02-01", "2027-05-01").await;

    let assignments: i64 = sqlx::query_scalar(
        "SELECT COUNT(*)::bigint FROM public.unidades_operativas_usos WHERE unidad_operativa_id IN ($1, $2)",
    )
    .bind(left_uop)
    .bind(right_uop)
    .fetch_one(&db)
    .await
    .expect("sequential, boundary-touching, and gapped uses must persist");
    assert_eq!(assignments, 4);
}

#[tokio::test]
async fn database_rejects_overlapping_cross_organization_and_outside_campaign_use_assignments() {
    let _guard = database_test_lock().lock().await;
    let db = test_pool().await;
    let fixture = create_fixture(&db).await;
    let campaign_id = campaign(&db, fixture, "INTEGRIDAD_USOS").await;
    let uop_id = create_uop(
        &db,
        fixture,
        campaign_id,
        "A1",
        BASE_A,
        fixture.base_plot_a_id,
    )
    .await;
    let trigo_id = territorial_use(&db, fixture, "TRIGO").await;
    let soja_id = territorial_use(&db, fixture, "SOJA").await;
    assign_use(&db, fixture, uop_id, trigo_id, "2026-09-01", "2026-12-01").await;

    for (start, end) in [
        ("2026-10-01", "2027-01-01"),
        ("2026-09-01", "2026-12-01"),
        ("2026-10-01", "2026-11-01"),
    ] {
        let error = sqlx::query(
            "INSERT INTO public.unidades_operativas_usos (unidad_operativa_id, uso_territorial_id, fecha_inicio, fecha_fin, creado_por) VALUES ($1, $2, $3::date, $4::date, $5)",
        )
        .bind(uop_id)
        .bind(soja_id)
        .bind(start)
        .bind(end)
        .bind(fixture.user_id)
        .execute(&db)
        .await
        .expect_err("positive-duration intersecting use intervals must fail");
        assert_database_error(error, EXCLUSION, None);
    }

    let empty_interval_error = sqlx::query(
        "INSERT INTO public.unidades_operativas_usos (unidad_operativa_id, uso_territorial_id, fecha_inicio, fecha_fin, creado_por) VALUES ($1, $2, DATE '2027-01-01', DATE '2027-01-01', $3)",
    )
    .bind(uop_id)
    .bind(soja_id)
    .bind(fixture.user_id)
    .execute(&db)
    .await
    .expect_err("empty use interval must fail");
    assert_database_error(empty_interval_error, CHECK, None);

    for (start, end) in [("2026-06-30", "2026-08-01"), ("2027-06-30", "2027-07-02")] {
        let error = sqlx::query(
            "INSERT INTO public.unidades_operativas_usos (unidad_operativa_id, uso_territorial_id, fecha_inicio, fecha_fin, creado_por) VALUES ($1, $2, $3::date, $4::date, $5)",
        )
        .bind(uop_id)
        .bind(soja_id)
        .bind(start)
        .bind(end)
        .bind(fixture.user_id)
        .execute(&db)
        .await
        .expect_err("a direct SQL assignment outside its campaign must fail");
        assert_database_error(error, RAISE, Some("agro_ops_uop_uso_fuera_campana"));
    }

    let other_fixture = create_fixture(&db).await;
    let other_use_id = territorial_use(&db, other_fixture, "PASTURA").await;
    let cross_organization_error = sqlx::query(
        "INSERT INTO public.unidades_operativas_usos (unidad_operativa_id, uso_territorial_id, fecha_inicio, fecha_fin, creado_por) VALUES ($1, $2, DATE '2027-03-01', DATE '2027-05-01', $3)",
    )
    .bind(uop_id)
    .bind(other_use_id)
    .bind(fixture.user_id)
    .execute(&db)
    .await
    .expect_err("a direct SQL cross-organization assignment must fail");
    assert_database_error(
        cross_organization_error,
        RAISE,
        Some("agro_ops_uop_uso_organizacion_invalida"),
    );

    let update_error = sqlx::query(
        "UPDATE public.unidades_operativas_usos SET fecha_fin = DATE '2026-11-01' WHERE unidad_operativa_id = $1",
    )
    .bind(uop_id)
    .execute(&db)
    .await
    .expect_err("historical use assignments must not be overwritten");
    assert_database_error(update_error, RAISE, Some("agro_ops_uop_uso_inmutable"));
}
