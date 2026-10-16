//! Keep directional-shadow visibility in sync with the views the renderer uses.
//!
//! Bevy 0.19 builds cascades for every Camera, including orthographic UI cameras.
//! Its light-visibility system uses those cascades to mark Mesh3d entities visible,
//! but prepare_lights only renders active Camera3d views whose layers match the
//! light. A UI camera's enormous unused cascade otherwise keeps nearby NPCs
//! animating and uploading skin matrices despite contributing no rendered shadow.

use bevy::camera::visibility::RenderLayers;
use bevy::light::{
    Cascades, cascade::build_directional_light_cascades, update_directional_light_frusta,
};
use bevy::prelude::*;

pub struct ShadowViewsPlugin;

impl Plugin for ShadowViewsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            retain_rendered_shadow_views
                .after(build_directional_light_cascades)
                .before(update_directional_light_frusta)
                // Diagnostic A/B only: retain the engine's unused UI cascades.
                .run_if(|| !crate::flag("legacyshadowviews")),
        );
    }
}

fn retain_rendered_shadow_views(
    cameras: Query<(&Camera, Option<&RenderLayers>), With<Camera3d>>,
    mut lights: Query<(&mut Cascades, Option<&RenderLayers>), With<DirectionalLight>>,
) {
    for (mut cascades, light_layers) in &mut lights {
        let light_layers = light_layers.cloned().unwrap_or_default();
        cascades.cascades.retain(|camera, _| {
            cameras.get(*camera).is_ok_and(|(camera, layers)| {
                camera.is_active && light_layers.intersects(&layers.cloned().unwrap_or_default())
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::camera::{primitives::Aabb, visibility::CascadesVisibleEntities};
    use bevy::ecs::system::RunSystemOnce;
    use bevy::light::{
        CascadeShadowConfigBuilder, DirectionalLightShadowMap, check_dir_light_mesh_visibility,
    };

    #[test]
    fn unused_ui_cascade_does_not_keep_offscreen_meshes_visible() {
        bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
        let mut world = World::new();
        world.init_resource::<DirectionalLightShadowMap>();
        let game = world
            .spawn((
                Camera3d::default(),
                Projection::Perspective(PerspectiveProjection {
                    aspect_ratio: 16.0 / 9.0,
                    ..default()
                }),
                GlobalTransform::IDENTITY,
            ))
            .id();
        let ui = world
            .spawn((
                Camera2d,
                Projection::Orthographic(OrthographicProjection {
                    area: Rect::new(-640.0, -360.0, 640.0, 360.0),
                    ..OrthographicProjection::default_2d()
                }),
                GlobalTransform::IDENTITY,
                RenderLayers::layer(7),
            ))
            .id();
        let light = world
            .spawn((
                DirectionalLight {
                    shadow_maps_enabled: true,
                    ..default()
                },
                CascadeShadowConfigBuilder {
                    num_cascades: 1,
                    maximum_distance: 60.0,
                    ..default()
                }
                .build(),
                GlobalTransform::IDENTITY,
                ViewVisibility::VISIBLE,
            ))
            .id();
        let caster = |world: &mut World, x| {
            world
                .spawn((
                    Mesh3d::default(),
                    Aabb::from_min_max(Vec3::splat(-0.5), Vec3::splat(0.5)),
                    GlobalTransform::from_xyz(x, 0.0, -20.0),
                    InheritedVisibility::VISIBLE,
                    ViewVisibility::HIDDEN,
                ))
                .id()
        };
        // At z=-20, x=30 is outside the perspective camera's image but inside
        // its real directional shadow cascade: this caster must stay active.
        let real_shadow_caster = caster(&mut world, 30.0);
        let ui_only_caster = caster(&mut world, 200.0);

        world
            .run_system_once(build_directional_light_cascades)
            .unwrap();
        world
            .run_system_once(update_directional_light_frusta)
            .unwrap();
        world
            .run_system_once(check_dir_light_mesh_visibility)
            .unwrap();
        assert!(
            world.get::<ViewVisibility>(ui_only_caster).unwrap().get(),
            "reproduce the unused UI cascade marking a faraway mesh visible"
        );
        assert!(
            world
                .get::<ViewVisibility>(real_shadow_caster)
                .unwrap()
                .get()
        );
        assert!(
            world
                .get::<Cascades>(light)
                .unwrap()
                .cascades
                .contains_key(&ui)
        );

        for entity in [ui_only_caster, real_shadow_caster] {
            world.entity_mut(entity).insert(ViewVisibility::HIDDEN);
        }
        world.run_system_once(retain_rendered_shadow_views).unwrap();
        world
            .run_system_once(update_directional_light_frusta)
            .unwrap();
        world
            .run_system_once(check_dir_light_mesh_visibility)
            .unwrap();
        let views = &world
            .get::<CascadesVisibleEntities>(light)
            .unwrap()
            .entities;
        assert!(views.contains_key(&game));
        assert!(!views.contains_key(&ui));
        assert!(!world.get::<ViewVisibility>(ui_only_caster).unwrap().get());
        assert!(
            world
                .get::<ViewVisibility>(real_shadow_caster)
                .unwrap()
                .get(),
            "legitimate offscreen shadow casters still need animation and skinning"
        );
    }

    #[test]
    fn three_dimensional_views_follow_camera_activity_and_light_layers() {
        let mut world = World::new();
        let game = world.spawn(Camera3d::default()).id();
        let portrait = world
            .spawn((Camera3d::default(), RenderLayers::layer(3)))
            .id();
        let highlight = world
            .spawn((Camera3d::default(), RenderLayers::layer(8)))
            .id();
        let inactive = world
            .spawn((
                Camera3d::default(),
                Camera {
                    is_active: false,
                    ..default()
                },
            ))
            .id();
        let ui = world.spawn(Camera2d).id();
        let all = [game, portrait, highlight, inactive, ui];
        let mut cascades = Cascades::default();
        for entity in all {
            cascades.cascades.insert(entity, Vec::new());
        }
        let sun = world
            .spawn((DirectionalLight::default(), cascades.clone()))
            .id();
        let portrait_light = world
            .spawn((
                DirectionalLight::default(),
                cascades.clone(),
                RenderLayers::layer(3),
            ))
            .id();
        let shared_light = world
            .spawn((
                DirectionalLight::default(),
                cascades,
                RenderLayers::from_layers(&[0, 3, 8]),
            ))
            .id();
        world.run_system_once(retain_rendered_shadow_views).unwrap();
        for (light, expected) in [
            (sun, vec![game]),
            (portrait_light, vec![portrait]),
            (shared_light, vec![game, portrait, highlight]),
        ] {
            let views = &world.get::<Cascades>(light).unwrap().cascades;
            assert_eq!(views.len(), expected.len());
            for entity in expected {
                assert!(views.contains_key(&entity));
            }
        }
    }
}
