//! Stable territorial identity owned by Agro Ops.
//!
//! An establishment is the complete physical field/property and its perimeter.
//! A base plot is a stable internal subdivision of that establishment; it is not
//! a RENSPA/SENASA perimeter. RENSPA is an external reference to the internal
//! establishment UUID. Campaign-specific UOPs are deliberately not implemented.

pub mod api;
pub mod application;
pub mod domain;
pub mod geographic_source;
pub mod infrastructure;
pub mod senasa;
