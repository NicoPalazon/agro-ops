//! User supplied GeoJSON geometry boundary.
//!
//! This deliberately accepts only the small V1 contract used by territorial
//! capture: a Polygon, MultiPolygon, or one Feature containing either.  It is
//! not a general GeoJSON import framework.

use serde_json::{Value, json};

pub const GEOJSON_INPUT_VERSION: &str = "geojson_polygon_multipolygon_v1";
pub const MAX_GEOJSON_BYTES: usize = 1_000_000;
pub const MAX_GEOJSON_POSITIONS: usize = 20_000;

#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedGeoJsonMultiPolygon {
    coordinates: Vec<Vec<Vec<Vec<f64>>>>,
}

impl NormalizedGeoJsonMultiPolygon {
    pub fn coordinates(&self) -> &Vec<Vec<Vec<Vec<f64>>>> {
        &self.coordinates
    }

    pub fn as_geojson(&self) -> Value {
        json!({ "type": "MultiPolygon", "coordinates": self.coordinates })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeoJsonGeometryErrorKind {
    InputTooLong,
    InvalidDocument,
    UnsupportedGeometry,
    UnexpectedSrid,
    MissingGeometry,
    EmptyGeometry,
    MalformedCoordinates,
    NonFiniteCoordinate,
    LongitudeOutOfRange,
    LatitudeOutOfRange,
    CoordinateLimitExceeded,
    InsufficientVertices,
    OpenRing,
}

impl GeoJsonGeometryErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::InputTooLong => "geojson_demasiado_largo",
            Self::InvalidDocument => "geojson_invalido",
            Self::UnsupportedGeometry => "tipo_geometria_no_admitido",
            Self::UnexpectedSrid => "srid_invalido",
            Self::MissingGeometry => "geometria_faltante",
            Self::EmptyGeometry => "geometria_vacia",
            Self::MalformedCoordinates => "coordenadas_malformadas",
            Self::NonFiniteCoordinate => "coordenada_no_finita",
            Self::LongitudeOutOfRange => "longitud_fuera_de_rango",
            Self::LatitudeOutOfRange => "latitud_fuera_de_rango",
            Self::CoordinateLimitExceeded => "limite_de_coordenadas_excedido",
            Self::InsufficientVertices => "vertices_insuficientes",
            Self::OpenRing => "anillo_abierto",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::InputTooLong => "El GeoJSON supera el límite permitido.",
            Self::InvalidDocument => "El GeoJSON no tiene una estructura válida.",
            Self::UnsupportedGeometry => "Solo se admite una geometría Polygon o MultiPolygon.",
            Self::UnexpectedSrid => {
                "El GeoJSON debe usar coordenadas WGS84 (SRID 4326) y no declarar otro sistema de referencia."
            }
            Self::MissingGeometry => "Falta la geometría GeoJSON.",
            Self::EmptyGeometry => "La geometría no puede estar vacía.",
            Self::MalformedCoordinates => "Las coordenadas GeoJSON no son válidas.",
            Self::NonFiniteCoordinate => "Las coordenadas deben ser números finitos.",
            Self::LongitudeOutOfRange => "La longitud debe estar entre -180 y 180.",
            Self::LatitudeOutOfRange => "La latitud debe estar entre -90 y 90.",
            Self::CoordinateLimitExceeded => {
                "La geometría supera el límite de coordenadas permitido."
            }
            Self::InsufficientVertices => "Cada anillo requiere al menos cuatro posiciones.",
            Self::OpenRing => "Cada anillo debe cerrarse repitiendo la primera posición al final.",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeoJsonGeometryError {
    pub kind: GeoJsonGeometryErrorKind,
}

/// Parses the standard GeoJSON longitude/latitude contract without repairing
/// rings or topology.  PostGIS performs the authoritative topology check.
pub fn normalize_geometry(
    value: Value,
) -> Result<NormalizedGeoJsonMultiPolygon, GeoJsonGeometryError> {
    let size = serde_json::to_vec(&value)
        .map_err(|_| error(GeoJsonGeometryErrorKind::InvalidDocument))?
        .len();
    if size > MAX_GEOJSON_BYTES {
        return Err(error(GeoJsonGeometryErrorKind::InputTooLong));
    }
    if value.get("crs").is_some() {
        return Err(error(GeoJsonGeometryErrorKind::UnexpectedSrid));
    }
    let geometry = if value.get("type").and_then(Value::as_str) == Some("Feature") {
        value
            .get("geometry")
            .cloned()
            .ok_or_else(|| error(GeoJsonGeometryErrorKind::MissingGeometry))?
    } else {
        value
    };
    let kind = geometry
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| error(GeoJsonGeometryErrorKind::InvalidDocument))?;
    if geometry.get("crs").is_some() {
        return Err(error(GeoJsonGeometryErrorKind::UnexpectedSrid));
    }
    let coordinates = geometry
        .get("coordinates")
        .ok_or_else(|| error(GeoJsonGeometryErrorKind::MissingGeometry))?;
    let polygons = match kind {
        "Polygon" => vec![parse_polygon(coordinates)?],
        "MultiPolygon" => coordinates
            .as_array()
            .ok_or_else(|| error(GeoJsonGeometryErrorKind::MalformedCoordinates))?
            .iter()
            .map(parse_polygon)
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(error(GeoJsonGeometryErrorKind::UnsupportedGeometry)),
    };
    if polygons.is_empty() {
        return Err(error(GeoJsonGeometryErrorKind::EmptyGeometry));
    }
    let mut position_count = 0usize;
    for polygon in &polygons {
        if polygon.is_empty() {
            return Err(error(GeoJsonGeometryErrorKind::EmptyGeometry));
        }
        for ring in polygon {
            position_count += ring.len();
            if position_count > MAX_GEOJSON_POSITIONS {
                return Err(error(GeoJsonGeometryErrorKind::CoordinateLimitExceeded));
            }
            if ring.len() < 4 {
                return Err(error(GeoJsonGeometryErrorKind::InsufficientVertices));
            }
            if ring.first() != ring.last() {
                return Err(error(GeoJsonGeometryErrorKind::OpenRing));
            }
        }
    }
    Ok(NormalizedGeoJsonMultiPolygon {
        coordinates: polygons,
    })
}

fn parse_polygon(value: &Value) -> Result<Vec<Vec<Vec<f64>>>, GeoJsonGeometryError> {
    let rings = value
        .as_array()
        .ok_or_else(|| error(GeoJsonGeometryErrorKind::MalformedCoordinates))?;
    rings.iter().map(parse_ring).collect()
}

fn parse_ring(value: &Value) -> Result<Vec<Vec<f64>>, GeoJsonGeometryError> {
    value
        .as_array()
        .ok_or_else(|| error(GeoJsonGeometryErrorKind::MalformedCoordinates))?
        .iter()
        .map(parse_position)
        .collect()
}

fn parse_position(value: &Value) -> Result<Vec<f64>, GeoJsonGeometryError> {
    let position = value
        .as_array()
        .ok_or_else(|| error(GeoJsonGeometryErrorKind::MalformedCoordinates))?;
    if position.len() != 2 {
        return Err(error(GeoJsonGeometryErrorKind::MalformedCoordinates));
    }
    let longitude = position[0]
        .as_f64()
        .ok_or_else(|| error(GeoJsonGeometryErrorKind::MalformedCoordinates))?;
    let latitude = position[1]
        .as_f64()
        .ok_or_else(|| error(GeoJsonGeometryErrorKind::MalformedCoordinates))?;
    if !longitude.is_finite() || !latitude.is_finite() {
        return Err(error(GeoJsonGeometryErrorKind::NonFiniteCoordinate));
    }
    if !(-180.0..=180.0).contains(&longitude) {
        return Err(error(GeoJsonGeometryErrorKind::LongitudeOutOfRange));
    }
    if !(-90.0..=90.0).contains(&latitude) {
        return Err(error(GeoJsonGeometryErrorKind::LatitudeOutOfRange));
    }
    Ok(vec![longitude, latitude])
}

fn error(kind: GeoJsonGeometryErrorKind) -> GeoJsonGeometryError {
    GeoJsonGeometryError { kind }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const RING: [[f64; 2]; 5] = [
        [-59.0, -34.0],
        [-58.0, -34.0],
        [-58.0, -33.0],
        [-59.0, -34.0],
        [-59.0, -34.0],
    ];

    #[test]
    fn polygon_normalizes_to_multipolygon() {
        let normalized =
            normalize_geometry(json!({"type":"Polygon", "coordinates":[RING]})).unwrap();
        assert_eq!(normalized.as_geojson()["type"], "MultiPolygon");
        assert_eq!(normalized.coordinates().len(), 1);
    }

    #[test]
    fn feature_and_multipolygon_are_preserved_semantically() {
        let normalized = normalize_geometry(json!({"type":"Feature", "properties":{"x":1}, "geometry":{"type":"MultiPolygon", "coordinates":[[RING]]}})).unwrap();
        assert_eq!(normalized.coordinates().len(), 1);
    }

    #[test]
    fn rejects_wrong_type_malformed_and_open_geometry() {
        assert_eq!(
            normalize_geometry(json!({"type":"Point", "coordinates":[0,0]}))
                .unwrap_err()
                .kind,
            GeoJsonGeometryErrorKind::UnsupportedGeometry
        );
        assert_eq!(
            normalize_geometry(json!({"type":"Polygon", "coordinates":"no"}))
                .unwrap_err()
                .kind,
            GeoJsonGeometryErrorKind::MalformedCoordinates
        );
        assert_eq!(normalize_geometry(json!({"type":"Polygon", "coordinates":[[[-59.0,-34.0],[-58.0,-34.0],[-58.0,-33.0],[-59.0,-33.0]]]})).unwrap_err().kind, GeoJsonGeometryErrorKind::OpenRing);
    }
}
