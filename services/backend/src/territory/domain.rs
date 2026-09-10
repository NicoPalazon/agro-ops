use std::fmt;

use time::OffsetDateTime;
use uuid::Uuid;

use crate::external_references::ExternalId;

pub const TERRITORIAL_SRID: i32 = 4326;
const MAX_CODE_LENGTH: usize = 64;
const MAX_NAME_LENGTH: usize = 255;
const MAX_WKT_LENGTH: usize = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CanonicalTerritorialCode(String);

impl CanonicalTerritorialCode {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidTerritorialCode> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= MAX_CODE_LENGTH
            && value.as_bytes()[0].is_ascii_uppercase()
            && value.bytes().all(|byte| {
                byte.is_ascii_uppercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-')
            });
        valid.then_some(Self(value)).ok_or(InvalidTerritorialCode)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FunctionalName(String);

impl FunctionalName {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidTerritorialName> {
        let value = value.into();
        let valid = !value.is_empty() && value.len() <= MAX_NAME_LENGTH && value.trim() == value;
        valid.then_some(Self(value)).ok_or(InvalidTerritorialName)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// WKT is accepted only at the application boundary. PostgreSQL/PostGIS is the
/// authoritative representation and validates it before persistence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeometryWkt(String);

impl GeometryWkt {
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidTerritorialGeometry> {
        let value = value.into();
        let valid = !value.is_empty() && value.len() <= MAX_WKT_LENGTH && value.trim() == value;
        valid
            .then_some(Self(value))
            .ok_or(InvalidTerritorialGeometry)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewEstablecimiento {
    pub codigo: CanonicalTerritorialCode,
    pub nombre: FunctionalName,
    pub geometria_wkt: GeometryWkt,
    pub origen_geometria: GeometryProvenance,
    /// Opaque RENSPA identity registered through the transversal external
    /// reference primitive; it is never the Agro Ops establishment identity.
    pub renspa: ExternalId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewLoteBase {
    pub establecimiento_id: Uuid,
    pub codigo: CanonicalTerritorialCode,
    pub nombre: FunctionalName,
    pub geometria_wkt: GeometryWkt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Establecimiento {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub codigo: CanonicalTerritorialCode,
    pub nombre: FunctionalName,
    /// Complete physical field/property perimeter, returned as EWKT by PostGIS.
    pub geometria_ewkt: String,
    pub origen_geometria: GeometryProvenance,
    pub activo: bool,
    pub creado_por: Uuid,
    pub creado_en: OffsetDateTime,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoteBase {
    pub id: Uuid,
    pub organization_id: Uuid,
    pub establecimiento_id: Uuid,
    pub codigo: CanonicalTerritorialCode,
    pub nombre: FunctionalName,
    /// Stable internal subdivision of its parent establishment, returned as
    /// EWKT by PostGIS. It is never a RENSPA/SENASA property perimeter.
    pub geometria_ewkt: String,
    pub activo: bool,
    pub creado_por: Uuid,
    pub creado_en: OffsetDateTime,
}

/// Source context of an establishment's authoritative perimeter. It describes
/// provenance only; geometry remains owned by Agro Ops/PostGIS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeometryProvenance {
    SenasaRenspa,
    Manual,
    Importada,
}

impl GeometryProvenance {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SenasaRenspa => "senasa_renspa",
            Self::Manual => "manual",
            Self::Importada => "importada",
        }
    }
}

impl TryFrom<&str> for GeometryProvenance {
    type Error = InvalidGeometryProvenance;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "senasa_renspa" => Ok(Self::SenasaRenspa),
            "manual" => Ok(Self::Manual),
            "importada" => Ok(Self::Importada),
            _ => Err(InvalidGeometryProvenance),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidTerritorialCode;

impl fmt::Display for InvalidTerritorialCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("territorial code must be uppercase canonical text")
    }
}

impl std::error::Error for InvalidTerritorialCode {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidTerritorialName;

impl fmt::Display for InvalidTerritorialName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("territorial name must be bounded, trimmed, and nonblank")
    }
}

impl std::error::Error for InvalidTerritorialName {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidTerritorialGeometry;

impl fmt::Display for InvalidTerritorialGeometry {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("territorial geometry WKT must be bounded, trimmed, and nonblank")
    }
}

impl std::error::Error for InvalidTerritorialGeometry {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidGeometryProvenance;

impl fmt::Display for InvalidGeometryProvenance {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("territorial geometry provenance is not recognized")
    }
}

impl std::error::Error for InvalidGeometryProvenance {}
