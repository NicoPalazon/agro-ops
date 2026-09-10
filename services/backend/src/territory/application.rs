use async_trait::async_trait;
use uuid::Uuid;

use crate::{
    authorization::{AuthorizationContext, PermissionDenied, permission_codes::TERRITORIO_CREAR},
    territory::domain::{Establecimiento, LoteBase, NewEstablecimiento, NewLoteBase},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerritoryStoreError {
    Conflict,
    Unavailable,
}

#[async_trait]
pub trait TerritoryStore: Send + Sync {
    async fn create_establecimiento(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        new_establecimiento: NewEstablecimiento,
    ) -> Result<Establecimiento, TerritoryStoreError>;

    async fn create_lote_base(
        &self,
        organization_id: Uuid,
        actor_id: Uuid,
        new_lote_base: NewLoteBase,
    ) -> Result<LoteBase, TerritoryStoreError>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerritoryApplicationError {
    PermissionDenied,
    Conflict,
    Unavailable,
}

pub async fn create_establecimiento(
    store: &dyn TerritoryStore,
    context: &AuthorizationContext,
    new_establecimiento: NewEstablecimiento,
) -> Result<Establecimiento, TerritoryApplicationError> {
    context
        .require_permission(TERRITORIO_CREAR)
        .map_err(|PermissionDenied| TerritoryApplicationError::PermissionDenied)?;
    store
        .create_establecimiento(
            context.organization_id,
            context.user_id,
            new_establecimiento,
        )
        .await
        .map_err(map_store_error)
}

pub async fn create_lote_base(
    store: &dyn TerritoryStore,
    context: &AuthorizationContext,
    new_lote_base: NewLoteBase,
) -> Result<LoteBase, TerritoryApplicationError> {
    context
        .require_permission(TERRITORIO_CREAR)
        .map_err(|PermissionDenied| TerritoryApplicationError::PermissionDenied)?;
    store
        .create_lote_base(context.organization_id, context.user_id, new_lote_base)
        .await
        .map_err(map_store_error)
}

fn map_store_error(error: TerritoryStoreError) -> TerritoryApplicationError {
    match error {
        TerritoryStoreError::Conflict => TerritoryApplicationError::Conflict,
        TerritoryStoreError::Unavailable => TerritoryApplicationError::Unavailable,
    }
}
