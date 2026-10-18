//! The local character seen through the map where walls hide it, as the 2018/261
//! client shows it: a glow in the highlight colour over the hidden parts
//! (shaders/hidden_pixels.wgsl), drawn after the scene against its depth. With
//! the visibility groups fading (crate::rendering::occlusion), this replaces the
//! dithered hole around the player (now DSOR_FLAGS=playerhole).
//!
//! Each mesh under the local player gets a child drawing the same mesh (and
//! skin) with this material: Bevy keeps one material per entity.
//! EVIDENCE: export_win32/frame/default.xml `HiddenCharacterColor` (p_hiddenpixels,
//!   Batch type HiddenPixels) into the highlight buffer, blurred and added by
//!   pe_compose; dro_client64.exe Highlight::RenderHiddenPixels (SetEntity,
//!   SetColor).
//! UNVERIFIED: the colour and strength (set at runtime by RenderHiddenPixels,
//!   not in the data); the original also blurs the glow (pe_highlight*).

use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::skinning::SkinnedMesh;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::pbr::{MaterialPipeline, MaterialPipelineKey};
use bevy::picking::Pickable;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, CompareFunction, RenderPipelineDescriptor, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;

/// A soft blue-white, at 0.4 (UNVERIFIED; the original also blurs it, softer).
const COLOR: Vec4 = Vec4::new(0.45, 0.7, 1.0, 0.4);

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct HiddenPixelsMaterial {
    #[uniform(0)]
    color: Vec4,
}

impl Material for HiddenPixelsMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://dsor_client/rendering/shaders/hidden_pixels.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Add
    }

    fn enable_prepass() -> bool {
        false
    }

    fn enable_shadows() -> bool {
        false
    }

    fn specialize(
        _: &MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _: &MeshVertexBufferLayoutRef,
        _: MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // Every fragment reaches the shader, which keeps the hidden ones.
        if let Some(depth) = descriptor.depth_stencil.as_mut() {
            depth.depth_compare = Some(CompareFunction::Always);
            depth.depth_write_enabled = Some(false);
        }
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

/// The child drawing a player mesh's hidden parts.
#[derive(Component)]
struct HiddenPixelsGhost;

#[derive(Resource)]
struct GhostMaterial(Handle<HiddenPixelsMaterial>);

pub struct HiddenPixelsPlugin;

impl Plugin for HiddenPixelsPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "shaders/hidden_pixels.wgsl");
        app.add_plugins(MaterialPlugin::<HiddenPixelsMaterial>::default())
            .add_systems(Startup, |mut commands: Commands, mut materials: ResMut<Assets<HiddenPixelsMaterial>>| {
                commands.insert_resource(GhostMaterial(materials.add(HiddenPixelsMaterial { color: COLOR })));
            })
            .add_systems(PostUpdate, add_ghosts);
    }
}

#[allow(clippy::type_complexity)]
fn add_ghosts(
    mut commands: Commands,
    material: Option<Res<GhostMaterial>>,
    meshes: Query<(Entity, &Mesh3d, Option<&SkinnedMesh>), (Added<Mesh3d>, Without<HiddenPixelsGhost>)>,
    parents: Query<&ChildOf>,
    players: Query<(), With<crate::network::net::LocalPlayer>>,
) {
    let Some(material) = material else { return };
    if crate::flag("playerhole") || crate::flag("nohiddenpixels") {
        return;
    }
    for (entity, mesh, skin) in &meshes {
        if !parents.iter_ancestors(entity).any(|a| players.contains(a)) {
            continue;
        }
        let mut ghost = commands.spawn((
            HiddenPixelsGhost,
            Mesh3d(mesh.0.clone()),
            MeshMaterial3d(material.0.clone()),
            Transform::IDENTITY,
            NoFrustumCulling,
            Pickable::IGNORE,
            bevy::light::NotShadowCaster,
            ChildOf(entity),
        ));
        if let Some(skin) = skin {
            ghost.insert(skin.clone());
        }
    }
}
