//! Lighting Menu (developer only, `DSOR_STUDIO_PREVIEW=1`): live edits of the
//! level's ambience, named presets and the global F2 multipliers.
//!
//! Kept out of crate::rendering::lighting, which only resolves each bubble
//! through the working overrides (presets::resolve_bubble) before fading to it.

pub(crate) mod presets;

use bevy::prelude::*;

/// The menu's switch, read once.
pub fn enabled() -> bool {
    std::env::var("DSOR_STUDIO_PREVIEW").is_ok_and(|v| v == "1")
}

pub struct LightingMenuPlugin;

impl Plugin for LightingMenuPlugin {
    fn build(&self, app: &mut App) {
        if !enabled() {
            return;
        }
        app.insert_resource(presets::LightingMenuStore::load());
    }
}
