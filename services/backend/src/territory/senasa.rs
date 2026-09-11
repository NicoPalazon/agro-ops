//! External SENASA polygon text boundary.
//!
//! The portal presents one source pair per line as `latitud, longitud`.
//! This module is the only place where that external ordering is interpreted;
//! callers receive standard geospatial `[longitud, latitud]` coordinates.

use serde_json::{Value, json};

pub const MAX_SENASA_TEXT_BYTES: usize = 65_536;
pub const MAX_SENASA_COORDINATE_PAIRS: usize = 5_000;

/// Canonical pasted-text representation used when copying a SENASA polygon:
/// one `latitud, longitud` pair per line. Blank lines and surrounding
/// whitespace are ignored; no other punctuation or alternate ordering is
/// accepted.
pub const SENASA_PASTED_TEXT_FORMAT: &str = "latitud, longitud\\nlatitud, longitud\\n...";

#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedPolygon4326 {
    ring: Vec<[f64; 2]>,
}

impl NormalizedPolygon4326 {
    pub fn source_coordinate_pair_count(&self) -> usize {
        self.ring.len()
    }

    pub fn ring(&self) -> &[[f64; 2]] {
        &self.ring
    }

    pub fn as_geojson_polygon(&self) -> Value {
        json!({ "type": "Polygon", "coordinates": [self.ring] })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SenasaPolygonParseErrorKind {
    InputTooLong,
    CoordinateLimitExceeded,
    MalformedCoordinate,
    MissingCoordinate,
    NonNumericCoordinate,
    NonFiniteCoordinate,
    LatitudeOutOfRange,
    LongitudeOutOfRange,
    InsufficientVertices,
    OpenRing,
}

impl SenasaPolygonParseErrorKind {
    pub fn code(self) -> &'static str {
        match self {
            Self::InputTooLong => "entrada_demasiado_larga",
            Self::CoordinateLimitExceeded => "limite_de_coordenadas_excedido",
            Self::MalformedCoordinate => "coordenada_malformada",
            Self::MissingCoordinate => "coordenada_faltante",
            Self::NonNumericCoordinate => "coordenada_no_numerica",
            Self::NonFiniteCoordinate => "coordenada_no_finita",
            Self::LatitudeOutOfRange => "latitud_fuera_de_rango",
            Self::LongitudeOutOfRange => "longitud_fuera_de_rango",
            Self::InsufficientVertices => "vertices_insuficientes",
            Self::OpenRing => "anillo_abierto",
        }
    }

    pub fn message(self) -> &'static str {
        match self {
            Self::InputTooLong => "El texto del polígono supera el límite permitido.",
            Self::CoordinateLimitExceeded => {
                "El polígono supera el límite de pares de coordenadas permitido."
            }
            Self::MalformedCoordinate => "Cada línea debe contener un par latitud, longitud.",
            Self::MissingCoordinate => "Falta una latitud o una longitud en un par de coordenadas.",
            Self::NonNumericCoordinate => "Las coordenadas deben ser números válidos.",
            Self::NonFiniteCoordinate => "Las coordenadas deben ser números finitos.",
            Self::LatitudeOutOfRange => "La latitud debe estar entre -90 y 90.",
            Self::LongitudeOutOfRange => "La longitud debe estar entre -180 y 180.",
            Self::InsufficientVertices => {
                "Se requieren al menos cuatro pares de coordenadas para un polígono cerrado."
            }
            Self::OpenRing => {
                "El anillo del polígono debe cerrarse explícitamente repitiendo el primer par al final."
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SenasaPolygonParseError {
    pub kind: SenasaPolygonParseErrorKind,
    pub line: Option<usize>,
}

impl SenasaPolygonParseError {
    fn new(kind: SenasaPolygonParseErrorKind, line: Option<usize>) -> Self {
        Self { kind, line }
    }
}

/// Parses the confirmed SENASA source ordering (`latitud, longitud`) into a
/// normalized polygon whose points are `[longitud, latitud]` in SRID 4326.
pub fn parse_polygon(source_text: &str) -> Result<NormalizedPolygon4326, SenasaPolygonParseError> {
    if source_text.len() > MAX_SENASA_TEXT_BYTES {
        return Err(SenasaPolygonParseError::new(
            SenasaPolygonParseErrorKind::InputTooLong,
            None,
        ));
    }

    let source_lines: Vec<(usize, &str)> = source_text
        .lines()
        .enumerate()
        .filter_map(|(index, line)| (!line.trim().is_empty()).then_some((index + 1, line.trim())))
        .collect();
    if source_lines.len() > MAX_SENASA_COORDINATE_PAIRS {
        return Err(SenasaPolygonParseError::new(
            SenasaPolygonParseErrorKind::CoordinateLimitExceeded,
            None,
        ));
    }

    let mut ring = Vec::with_capacity(source_lines.len());
    for (line_number, source_pair) in source_lines {
        let mut values = source_pair.split(',');
        let latitude = values
            .next()
            .expect("split always yields a first segment")
            .trim();
        let longitude = values.next().map(str::trim);
        if values.next().is_some() {
            return Err(SenasaPolygonParseError::new(
                SenasaPolygonParseErrorKind::MalformedCoordinate,
                Some(line_number),
            ));
        }
        let Some(longitude) = longitude else {
            return Err(SenasaPolygonParseError::new(
                SenasaPolygonParseErrorKind::MalformedCoordinate,
                Some(line_number),
            ));
        };
        if latitude.is_empty() || longitude.is_empty() {
            return Err(SenasaPolygonParseError::new(
                SenasaPolygonParseErrorKind::MissingCoordinate,
                Some(line_number),
            ));
        }

        let latitude = parse_coordinate(latitude, line_number)?;
        let longitude = parse_coordinate(longitude, line_number)?;
        if !(-90.0..=90.0).contains(&latitude) {
            return Err(SenasaPolygonParseError::new(
                SenasaPolygonParseErrorKind::LatitudeOutOfRange,
                Some(line_number),
            ));
        }
        if !(-180.0..=180.0).contains(&longitude) {
            return Err(SenasaPolygonParseError::new(
                SenasaPolygonParseErrorKind::LongitudeOutOfRange,
                Some(line_number),
            ));
        }

        // The source order is known and must never be inferred or swapped.
        ring.push([longitude, latitude]);
    }

    if ring.len() < 4 {
        return Err(SenasaPolygonParseError::new(
            SenasaPolygonParseErrorKind::InsufficientVertices,
            None,
        ));
    }
    if ring.first() != ring.last() {
        return Err(SenasaPolygonParseError::new(
            SenasaPolygonParseErrorKind::OpenRing,
            None,
        ));
    }

    Ok(NormalizedPolygon4326 { ring })
}

fn parse_coordinate(value: &str, line: usize) -> Result<f64, SenasaPolygonParseError> {
    let coordinate = value.parse::<f64>().map_err(|_| {
        SenasaPolygonParseError::new(
            SenasaPolygonParseErrorKind::NonNumericCoordinate,
            Some(line),
        )
    })?;
    coordinate.is_finite().then_some(coordinate).ok_or_else(|| {
        SenasaPolygonParseError::new(SenasaPolygonParseErrorKind::NonFiniteCoordinate, Some(line))
    })
}

#[cfg(test)]
mod tests {
    use super::{MAX_SENASA_COORDINATE_PAIRS, SenasaPolygonParseErrorKind, parse_polygon};

    const VALID_POLYGON: &str =
        "-33.7601, -59.81266\n-33.76887, -59.79991\n-33.78036, -59.81339\n-33.7601, -59.81266";

    #[test]
    fn parses_valid_senasa_pairs_and_normalizes_latitude_longitude_to_longitude_latitude() {
        let polygon = parse_polygon(VALID_POLYGON).expect("fixture polygon must parse");

        assert_eq!(polygon.source_coordinate_pair_count(), 4);
        assert_eq!(polygon.ring()[0], [-59.81266, -33.7601]);
        assert_eq!(
            polygon.as_geojson_polygon()["coordinates"][0][0],
            serde_json::json!([-59.81266, -33.7601])
        );
    }

    #[test]
    fn accepts_harmless_line_and_surrounding_whitespace() {
        let polygon = parse_polygon(
            "  -33.7601, -59.81266  \r\n\n\t-33.76887 , -59.79991\n -33.78036, -59.81339\n-33.7601, -59.81266  ",
        )
        .expect("whitespace variation must parse");

        assert_eq!(polygon.ring()[1], [-59.79991, -33.76887]);
    }

    #[test]
    fn rejects_malformed_missing_and_non_numeric_pairs() {
        for (input, expected) in [
            (
                "-33.7, -59.8, 1",
                SenasaPolygonParseErrorKind::MalformedCoordinate,
            ),
            ("-33.7,", SenasaPolygonParseErrorKind::MissingCoordinate),
            (
                "sur, -59.8",
                SenasaPolygonParseErrorKind::NonNumericCoordinate,
            ),
        ] {
            assert_eq!(parse_polygon(input).unwrap_err().kind, expected);
        }
    }

    #[test]
    fn rejects_non_finite_and_out_of_range_coordinates() {
        for (input, expected) in [
            (
                "NaN, -59\n0, 0\n1, 1\nNaN, -59",
                SenasaPolygonParseErrorKind::NonFiniteCoordinate,
            ),
            (
                "91, -59\n0, 0\n1, 1\n91, -59",
                SenasaPolygonParseErrorKind::LatitudeOutOfRange,
            ),
            (
                "0, 181\n0, 0\n1, 1\n0, 181",
                SenasaPolygonParseErrorKind::LongitudeOutOfRange,
            ),
        ] {
            assert_eq!(parse_polygon(input).unwrap_err().kind, expected);
        }
    }

    #[test]
    fn rejects_insufficient_and_unclosed_rings_without_repairing_them() {
        assert_eq!(
            parse_polygon("0, 0\n0, 1\n0, 0").unwrap_err().kind,
            SenasaPolygonParseErrorKind::InsufficientVertices
        );
        assert_eq!(
            parse_polygon("0, 0\n0, 1\n1, 1\n1, 0").unwrap_err().kind,
            SenasaPolygonParseErrorKind::OpenRing
        );
    }

    #[test]
    fn rejects_oversized_coordinate_sequences() {
        let source = "0, 0\n".repeat(MAX_SENASA_COORDINATE_PAIRS + 1);
        assert_eq!(
            parse_polygon(&source).unwrap_err().kind,
            SenasaPolygonParseErrorKind::CoordinateLimitExceeded
        );
    }

    #[test]
    fn never_guesses_or_swaps_source_coordinate_order() {
        let input = "-59, -33\n-59, -34\n-58, -34\n-59, -33";
        let polygon = parse_polygon(input).expect("values remain valid in source order");

        assert_eq!(polygon.ring()[0], [-33.0, -59.0]);
    }
}
