//! Artwork providers, in the order the service asks them: local sources first, online ones only when the user
//! turned online artwork on.

pub mod steam_cdn;
pub mod steam_local;
pub mod steamgriddb;

pub use steam_cdn::SteamCdnProvider;
pub use steam_local::SteamLocalProvider;
pub use steamgriddb::SteamGridDbProvider;
