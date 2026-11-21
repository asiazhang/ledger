#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unreachable
    )
)]

mod command;
mod crud;
mod model;
mod validation;

#[cfg(test)]
mod tests;

pub use command::replay_command;
pub use command::{PhysicalAssetCommand, ValuationCommandRow};

pub use crud::{
    create_physical_asset, delete_physical_asset, dispose_physical_asset, get_physical_asset,
    list_physical_assets, update_physical_asset, update_physical_asset_valuation,
};
pub use model::{
    PhysicalAsset, PhysicalAssetDisposeInput, PhysicalAssetInput, PhysicalAssetList,
    PhysicalAssetStatus, PhysicalAssetUpdateInput, PhysicalAssetValuationInput,
};
