//! The data UIs render and the commands they send. Plain data: no behavior,
//! no domain state of its own. Each facade (UniFFI, cxx-qt) exposes these
//! types as they are.

pub mod items;
pub mod pages;
pub mod playlists;
pub mod plugins;
pub mod search;
pub mod session;
pub mod sign_in;
