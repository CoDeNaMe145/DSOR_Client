//! Lighting Menu (developer only, `DSOR_STUDIO_PREVIEW=1`): live edits of the
//! level's ambience, named presets and the global F2 multipliers.
//!
//! Kept out of crate::rendering::lighting, which only resolves each bubble
//! through the working overrides (presets::resolve_bubble) before fading to it.
//! With the menu on, F2 opens it instead of the lighting panel.

mod global;
pub(crate) mod presets;
mod window;

use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};

/// The menu's switch, read once.
pub fn enabled() -> bool {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ENABLED.get_or_init(|| std::env::var("DSOR_STUDIO_PREVIEW").is_ok_and(|v| v == "1"))
}

pub struct LightingMenuPlugin;

impl Plugin for LightingMenuPlugin {
    fn build(&self, app: &mut App) {
        if !enabled() {
            return;
        }
        app.insert_resource(presets::LightingMenuStore::load())
            .add_plugins((window::WindowPlugin, global::GlobalControlsPlugin))
            .add_systems(Update, (toggle, keep_cursor).chain());
    }
}

fn toggle(keys: Res<ButtonInput<KeyCode>>, mut roots: Query<&mut Visibility, With<window::MenuRoot>>) {
    if keys.just_pressed(KeyCode::F2) {
        for mut v in &mut roots {
            *v = if *v == Visibility::Hidden { Visibility::Inherited } else { Visibility::Hidden };
        }
    }
}

/// The cursor stays visible while the menu is open (the free camera hides it).
fn keep_cursor(roots: Query<&Visibility, With<window::MenuRoot>>, mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>) {
    if roots.iter().any(|v| *v != Visibility::Hidden) {
        for mut cursor in &mut cursors {
            if !cursor.visible {
                cursor.visible = true;
            }
        }
    }
}
