//! Login, character selection, and shared loading-screen state.

pub(crate) mod loading;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) mod login;
pub(crate) mod selection;
