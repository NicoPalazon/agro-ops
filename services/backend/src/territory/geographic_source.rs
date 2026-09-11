//! Persisted territorial geographic-source evidence, separate from canonical
//! establishment geometry.

use std::fmt;

use serde_json::json;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    external_references::ExternalId,
    territory::{
        geojson::{GEOJSON_INPUT_VERSION, NormalizedGeoJsonMultiPolygon},
        senasa::NormalizedPolygon4326,
    },
};

pub const SENASA_RENSPA_SOURCE_TYPE: &str = "senasa_renspa";
pub const SENASA_PARSER_VERSION: &str = "senasa_pasted_polygon_v1";
pub const MANUAL_SOURCE_TYPE: &str = "manual";
pub const IMPORTED_SOURCE_TYPE: &str = "importada";
pub const CORRECTION_PARSER_VERSION: &str = "territory_correction_geojson_v1";
const MAX_EXTERNAL_NAME_LENGTH: usize = 255;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeographicSourceType {
    SenasaRenspa,
    Manual,
    Importada,
}

impl GeographicSourceType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SenasaRenspa => SENASA_RENSPA_SOURCE_TYPE,
            Self::Manual => MANUAL_SOURCE_TYPE,
            Self::Importada => IMPORTED_SOURCE_TYPE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalSourceName(String);

impl ExternalSourceName {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidExternalSourceName> {
        let value = value.into();
        let valid =
            !value.is_empty() && value.len() <= MAX_EXTERNAL_NAME_LENGTH && value.trim() == value;
        valid
            .then_some(Self(value))
            .ok_or(InvalidExternalSourceName)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidExternalSourceName;

impl fmt::Display for InvalidExternalSourceName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("external source name must be bounded, trimmed, and nonblank")
    }
}

impl std::error::Error for InvalidExternalSourceName {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeographicSourceFingerprint([u8; 32]);

impl GeographicSourceFingerprint {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// The fingerprint deliberately excludes raw pasted text. It includes the
    /// establishment, opaque external identity, external name, source type,
    /// parser version and normalized coordinate sequence, so whitespace-only
    /// changes replay while distinct RENSPA identities remain distinct.
    pub fn for_senasa(
        establishment_id: Uuid,
        external_id: &ExternalId,
        external_name: Option<&ExternalSourceName>,
        polygon: &NormalizedPolygon4326,
    ) -> Self {
        let canonical = json!({
            "establecimiento_id": establishment_id.to_string(),
            "external_id": external_id.as_str(),
            "nombre_externo": external_name.map(ExternalSourceName::as_str),
            "tipo_origen": SENASA_RENSPA_SOURCE_TYPE,
            "version_parser": SENASA_PARSER_VERSION,
            "anillo_normalizado": polygon.ring(),
        });
        let bytes = serde_json::to_vec(&canonical)
            .expect("canonical geographic-source fingerprint serialization must succeed");
        Self(Sha256::digest(bytes).into())
    }

    pub fn for_alternative(
        establishment_code: &str,
        source_type: GeographicSourceType,
        geometry: &NormalizedGeoJsonMultiPolygon,
    ) -> Self {
        let canonical = json!({
            "establecimiento_codigo": establishment_code,
            "tipo_origen": source_type.as_str(),
            "version_parser": GEOJSON_INPUT_VERSION,
            "geometria_normalizada": geometry.as_geojson(),
        });
        let bytes = serde_json::to_vec(&canonical)
            .expect("canonical geographic-source fingerprint serialization must succeed");
        Self(Sha256::digest(bytes).into())
    }

    pub fn for_correction(
        replaced_source_id: Uuid,
        geometry: &NormalizedGeoJsonMultiPolygon,
    ) -> Self {
        let canonical = json!({
            "reemplaza_fuente_geografica_id": replaced_source_id,
            "version_parser": CORRECTION_PARSER_VERSION,
            "geometria_normalizada": geometry.as_geojson(),
        });
        let bytes = serde_json::to_vec(&canonical)
            .expect("canonical geographic correction fingerprint serialization must succeed");
        Self(Sha256::digest(bytes).into())
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        external_references::ExternalId,
        territory::{
            geographic_source::{ExternalSourceName, GeographicSourceFingerprint},
            senasa::parse_polygon,
        },
    };
    use uuid::Uuid;

    #[test]
    fn fingerprint_uses_normalized_semantics_not_source_whitespace_but_keeps_external_identity() {
        let establishment_id = Uuid::nil();
        let compact =
            parse_polygon("-33, -59\n-34, -59\n-34, -58\n-33, -59").expect("polygon must parse");
        let spaced = parse_polygon("  -33, -59  \n\n-34 , -59\n-34, -58\n -33, -59 ")
            .expect("polygon must parse");
        let name = ExternalSourceName::new("Campo externo").expect("name must be valid");
        let first = GeographicSourceFingerprint::for_senasa(
            establishment_id,
            &ExternalId::new("RENSPA-1").expect("external id must be valid"),
            Some(&name),
            &compact,
        );
        let same_semantics = GeographicSourceFingerprint::for_senasa(
            establishment_id,
            &ExternalId::new("RENSPA-1").expect("external id must be valid"),
            Some(&name),
            &spaced,
        );
        let other_reference = GeographicSourceFingerprint::for_senasa(
            establishment_id,
            &ExternalId::new("RENSPA-2").expect("external id must be valid"),
            Some(&name),
            &compact,
        );

        assert_eq!(first, same_semantics);
        assert_ne!(first, other_reference);
    }
}
