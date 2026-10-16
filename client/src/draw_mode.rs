//! Choose draw mode once, before cameras and shadow views are created.
//!
//! Native scenes use direct draws to avoid per-frame preparation of many small
//! indirect batches. CPU visibility and shadow-caster checks remain enabled.

use bevy::prelude::*;
use bevy::render::{
    RenderApp,
    batching::gpu_preprocessing::{GpuPreprocessingMode, GpuPreprocessingSupport},
    view::NoIndirectDrawing,
};

pub struct DrawModePlugin {
    direct: bool,
}

impl Default for DrawModePlugin {
    fn default() -> Self {
        Self {
            direct: use_direct_draws(
                cfg!(not(target_arch = "wasm32")),
                crate::flag("directdraw"),
                crate::flag("indirectdraw"),
            ),
        }
    }
}

fn use_direct_draws(native: bool, direct: bool, indirect: bool) -> bool {
    !indirect && (native || direct)
}

impl Plugin for DrawModePlugin {
    fn build(&self, app: &mut App) {
        if self.direct {
            // Bevy requires this component from camera creation: toggling it on
            // a live view is unsupported. This covers later cameras too.
            app.register_required_components::<Camera3d, NoIndirectDrawing>();
        }
    }

    fn finish(&self, app: &mut App) {
        if !self.direct {
            info!("draw mode: platform default (indirect draws when supported)");
            return;
        }
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        // MeshRenderPlugin initializes this during its earlier finish hook.
        // Directional shadows inherit their camera's setting. Point/spot shadow
        // views use this global maximum instead, so cap it before any are made.
        // PreprocessingOnly retains GPU instance preprocessing and skinning,
        // while selecting direct draws and disabling GPU culling/bin unpacking.
        let mut support = render.world_mut().resource_mut::<GpuPreprocessingSupport>();
        cap_to_direct_draws(&mut support);
        info!(
            "draw mode: direct Camera3d and directional/point/spot shadows; CPU visibility retained; GPU instance preprocessing retained when available; GPU culling disabled"
        );
    }
}

fn cap_to_direct_draws(support: &mut GpuPreprocessingSupport) {
    support.max_supported_mode = support.min(GpuPreprocessingMode::PreprocessingOnly);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_defaults_to_direct_and_explicit_indirect_wins_on_every_platform() {
        for (native, direct, indirect, expected) in [
            (true, false, false, true),
            (true, true, false, true),
            (true, false, true, false),
            (true, true, true, false),
            (false, false, false, false),
            (false, true, false, true),
            (false, false, true, false),
            (false, true, true, false),
        ] {
            assert_eq!(use_direct_draws(native, direct, indirect), expected);
        }
    }

    #[test]
    fn direct_draw_is_present_from_spawn_for_initial_and_later_3d_cameras() {
        let mut app = App::new();
        app.add_plugins(DrawModePlugin { direct: true });
        let initial = app.world_mut().spawn(Camera3d::default()).id();
        let ui = app.world_mut().spawn(Camera2d).id();
        app.update();
        let later = app.world_mut().spawn(Camera3d::default()).id();
        assert!(app.world().get::<NoIndirectDrawing>(initial).is_some());
        assert!(app.world().get::<NoIndirectDrawing>(later).is_some());
        assert!(app.world().get::<NoIndirectDrawing>(ui).is_none());
    }

    #[test]
    fn fallback_leaves_camera_and_shadow_support_unchanged() {
        let mut app = App::new();
        let mut render = SubApp::new();
        render.insert_resource(GpuPreprocessingSupport {
            max_supported_mode: GpuPreprocessingMode::Culling,
        });
        app.insert_sub_app(RenderApp, render);
        app.add_plugins(DrawModePlugin { direct: false });
        let camera = app.world_mut().spawn(Camera3d::default()).id();
        app.finish();
        assert!(app.world().get::<NoIndirectDrawing>(camera).is_none());
        assert!(app.sub_app(RenderApp).world().resource::<GpuPreprocessingSupport>().is_culling_supported());
    }

    #[test]
    fn shadow_support_cap_retains_preprocessing_without_enabling_unsupported_features() {
        for original in [
            GpuPreprocessingMode::None,
            GpuPreprocessingMode::PreprocessingOnly,
            GpuPreprocessingMode::Culling,
        ] {
            let mut support = GpuPreprocessingSupport {
                max_supported_mode: original,
            };
            cap_to_direct_draws(&mut support);
            assert!(support.max_supported_mode != GpuPreprocessingMode::Culling);
            assert!(support.is_available() == (original != GpuPreprocessingMode::None));
        }
    }
}
