//! Opt-in timestamps for Bevy 0.19's shadow passes, which do not record GPU
//! diagnostics themselves. The normal renderer is untouched unless gpuprofile
//! is enabled. Keep the pass bodies and ordering aligned with bevy_pbr::PbrPlugin.

use bevy::core_pipeline::{
    mip_generation::experimental::depth::early_downsample_depth,
    schedule::{Core3d, Core3dSystems},
};
use bevy::ecs::schedule::ScheduleCleanupPolicy;
use bevy::pbr::{
    EARLY_SHADOW_PASS, LATE_SHADOW_PASS, Shadow, ShadowView, ViewLightEntities,
    early_prepass_build_indirect_parameters, late_prepass_build_indirect_parameters,
    main_build_indirect_parameters, per_view_shadow_pass, shared_shadow_pass,
};
use bevy::prelude::*;
use bevy::render::{
    RenderApp,
    diagnostic::RecordDiagnostics,
    occlusion_culling::OcclusionCulling,
    render_phase::ViewBinnedRenderPhases,
    render_resource::{RenderPassDescriptor, StoreOp},
    renderer::{RenderContext, ViewQuery},
    view::ExtractedView,
};

pub(super) struct ShadowProfilePlugin;

impl Plugin for ShadowProfilePlugin {
    fn build(&self, _app: &mut App) {}

    fn finish(&self, app: &mut App) {
        let Some(render) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        // At finish, the renderer and its schedules have been installed.
        // Preserve transitive dependencies while removing the stock systems;
        // then reproduce their ordering explicitly with the replacements.
        // Bevy forbids adding another function to a function's system-type set.
        for removed in [
            render.remove_systems_in_set(
                Core3d,
                per_view_shadow_pass::<EARLY_SHADOW_PASS>,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            ),
            render.remove_systems_in_set(
                Core3d,
                per_view_shadow_pass::<LATE_SHADOW_PASS>,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            ),
            render.remove_systems_in_set(
                Core3d,
                shared_shadow_pass::<EARLY_SHADOW_PASS>,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            ),
            render.remove_systems_in_set(
                Core3d,
                shared_shadow_pass::<LATE_SHADOW_PASS>,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            ),
        ] {
            assert_eq!(
                removed.expect("shadow profiler could not replace Bevy's shadow pass"),
                1,
                "shadow profiler expected exactly one stock shadow pass"
            );
        }
        render.add_systems(
            Core3d,
            (
                profiled_directional_shadows::<EARLY_SHADOW_PASS>
                    .after(early_prepass_build_indirect_parameters)
                    .before(early_downsample_depth)
                    .before(profiled_directional_shadows::<LATE_SHADOW_PASS>),
                profiled_directional_shadows::<LATE_SHADOW_PASS>
                    .after(late_prepass_build_indirect_parameters)
                    .before(main_build_indirect_parameters)
                    .before(Core3dSystems::MainPass),
                profiled_shared_shadows::<EARLY_SHADOW_PASS>
                    .after(early_prepass_build_indirect_parameters)
                    .before(early_downsample_depth)
                    .before(profiled_shared_shadows::<LATE_SHADOW_PASS>),
                profiled_shared_shadows::<LATE_SHADOW_PASS>
                    .after(late_prepass_build_indirect_parameters)
                    .before(main_build_indirect_parameters)
                    .before(Core3dSystems::MainPass),
            ),
        );
    }
}

// These are Bevy 0.19.1's shared_shadow_pass/per_view_shadow_pass/view_shadow_pass
// with a diagnostic pass span around the existing shadow phase. In particular,
// late passes still run only with occlusion culling; attachment loads/stores and
// phase rendering are unchanged.
fn profiled_shared_shadows<const IS_LATE: bool>(
    world: &World,
    view_light_query: ViewQuery<(Entity, &ShadowView, &ExtractedView, Has<OcclusionCulling>)>,
    shadow_render_phases: Res<ViewBinnedRenderPhases<Shadow>>,
    mut ctx: RenderContext,
) {
    let (entity, light, view, occlusion) = view_light_query.into_inner();
    draw_shadow::<IS_LATE>(
        entity,
        light,
        view,
        occlusion,
        world,
        &shadow_render_phases,
        &mut ctx,
    );
}

fn profiled_directional_shadows<const IS_LATE: bool>(
    world: &World,
    view: ViewQuery<&ViewLightEntities>,
    view_light_query: Query<(&ShadowView, &ExtractedView, Has<OcclusionCulling>)>,
    shadow_render_phases: Res<ViewBinnedRenderPhases<Shadow>>,
    mut ctx: RenderContext,
) {
    for entity in view.into_inner().lights.iter().copied() {
        if let Ok((light, view, occlusion)) = view_light_query.get(entity) {
            draw_shadow::<IS_LATE>(
                entity,
                light,
                view,
                occlusion,
                world,
                &shadow_render_phases,
                &mut ctx,
            );
        }
    }
}

fn draw_shadow<const IS_LATE: bool>(
    entity: Entity,
    light: &ShadowView,
    view: &ExtractedView,
    occlusion_culling: bool,
    world: &World,
    phases: &ViewBinnedRenderPhases<Shadow>,
    ctx: &mut RenderContext,
) {
    if IS_LATE && !occlusion_culling {
        return;
    }
    let Some(phase) = phases.get(&view.retained_view_entity) else {
        return;
    };
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let depth_stencil_attachment = Some(light.depth_attachment.get_attachment(StoreOp::Store));
    let mut pass = ctx.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some(&light.pass_name),
        color_attachments: &[],
        depth_stencil_attachment,
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
    });
    let span = diagnostics.pass_span(
        &mut pass,
        format!(
            "{}_{}",
            light.pass_name,
            if IS_LATE { "late" } else { "early" }
        ),
    );
    if let Err(error) = phase.render(&mut pass, world, entity) {
        error!("Error encountered while rendering the shadow phase {error:?}");
    }
    span.end(&mut pass);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_schedule_installs_without_a_gpu_and_keeps_early_before_late() {
        let mut app = App::new();
        let mut render = SubApp::new();
        // These are real rendering systems. Initializing their schedule does
        // not require a GPU; only running the passes does. This exercises the
        // plugin's actual finish path, including Bevy's system-set validation.
        render.add_systems(
            Core3d,
            (
                per_view_shadow_pass::<EARLY_SHADOW_PASS>
                    .after(early_prepass_build_indirect_parameters)
                    .before(early_downsample_depth)
                    .before(per_view_shadow_pass::<LATE_SHADOW_PASS>),
                per_view_shadow_pass::<LATE_SHADOW_PASS>
                    .after(late_prepass_build_indirect_parameters)
                    .before(main_build_indirect_parameters)
                    .before(Core3dSystems::MainPass),
                shared_shadow_pass::<EARLY_SHADOW_PASS>
                    .after(early_prepass_build_indirect_parameters)
                    .before(early_downsample_depth)
                    .before(shared_shadow_pass::<LATE_SHADOW_PASS>),
                shared_shadow_pass::<LATE_SHADOW_PASS>
                    .after(late_prepass_build_indirect_parameters)
                    .before(main_build_indirect_parameters)
                    .before(Core3dSystems::MainPass),
            ),
        );
        app.insert_sub_app(RenderApp, render);
        ShadowProfilePlugin.finish(&mut app);
        let render = app.get_sub_app_mut(RenderApp).unwrap();
        render
            .world_mut()
            .schedule_scope(Core3d, |world, schedule| {
                schedule.initialize(world).unwrap();
                let systems: Vec<_> = schedule
                    .systems()
                    .unwrap()
                    .map(|(_, system)| system.name().to_string())
                    .collect();
                for pass in ["profiled_directional_shadows", "profiled_shared_shadows"] {
                    let early = systems
                        .iter()
                        .position(|name| name.contains(&format!("{pass}<false>")))
                        .unwrap();
                    let late = systems
                        .iter()
                        .position(|name| name.contains(&format!("{pass}<true>")))
                        .unwrap();
                    assert!(
                        early < late,
                        "the early shadow pass must precede its late pass"
                    );
                }
                assert!(
                    !systems
                        .iter()
                        .any(|name| name.contains("::per_view_shadow_pass")
                            || name.contains("::shared_shadow_pass")),
                    "stock passes must not render a second time"
                );
            });
    }
}
