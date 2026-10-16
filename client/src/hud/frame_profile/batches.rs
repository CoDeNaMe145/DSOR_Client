//! Opt-in GPU batch fragmentation census.

use super::super::cpu;
use bevy::prelude::*;
use bevy::render::{
    Render, RenderSystems,
    batching::gpu_preprocessing::BinUnpackingBuffers,
    view::{ExtractedView, NoIndirectDrawing},
};
use std::any::TypeId;

pub(super) fn install_census(render: &mut SubApp) {
    render.add_systems(
        Render,
        batch_census
            .after(RenderSystems::PrepareResourcesWritePhaseBuffers)
            .before(RenderSystems::PrepareResourcesCollectPhaseBuffers),
    );
}

fn phase_name(phase: TypeId) -> &'static str {
    use bevy::core_pipeline::{
        core_3d::{AlphaMask3d, Opaque3d},
        prepass::{AlphaMask3dPrepass, Opaque3dPrepass},
    };
    if phase == TypeId::of::<Opaque3d>() {
        "opaque"
    } else if phase == TypeId::of::<AlphaMask3d>() {
        "alpha_mask"
    } else if phase == TypeId::of::<Opaque3dPrepass>() {
        "opaque_prepass"
    } else if phase == TypeId::of::<AlphaMask3dPrepass>() {
        "alpha_mask_prepass"
    } else if phase == TypeId::of::<bevy::pbr::Shadow>() {
        "shadow"
    } else {
        "other"
    }
}

fn batch_census(
    buffers: Option<Res<BinUnpackingBuffers>>,
    views: Query<(
        &ExtractedView,
        Has<NoIndirectDrawing>,
        Has<bevy::pbr::ShadowView>,
        Has<Camera3d>,
    )>,
    mut last: Local<f64>,
) {
    let now = cpu::now();
    if now - *last < 5000.0 {
        return;
    }
    *last = now;
    let Some(buffers) = buffers else {
        info!("batch profile: GPU bin unpacking resources absent");
        return;
    };
    let (mut view_count, mut direct_views, mut shadow_views, mut direct_shadows) = (0, 0, 0, 0);
    for (_, direct, shadow, camera_3d) in &views {
        if shadow || camera_3d {
            view_count += 1;
            direct_views += usize::from(direct);
        }
        if shadow {
            shadow_views += 1;
            direct_shadows += usize::from(direct);
        }
    }
    let (mut total_jobs, mut total_instances, mut singleton_jobs) = (0, 0, 0);
    let (mut instance_capacity, mut index_capacity) = (0_u64, 0_u64);
    for (key, phase) in &buffers.view_phase_buffers {
        let jobs = phase
            .indexed_unpacking_jobs
            .iter()
            .chain(&phase.non_indexed_unpacking_jobs);
        let (mut count, mut instances, mut singletons, mut largest) = (0, 0, 0, 0);
        for job in jobs {
            count += 1;
            instances += u64::from(job.mesh_instance_count);
            singletons += usize::from(job.mesh_instance_count == 1);
            largest = largest.max(job.mesh_instance_count);
            instance_capacity += job.render_binned_mesh_instance_buffer.size();
            index_capacity += job.bin_index_to_indirect_parameters_offset_buffer.size();
        }
        total_jobs += count;
        total_instances += instances;
        singleton_jobs += singletons;
        if count != 0 {
            info!(
                "batch profile: view {:?}, phase {} ({:?}), jobs {count}, instances {instances}, singleton_jobs {singletons}, largest_job_instances {largest}",
                key.view,
                phase_name(key.phase),
                key.phase
            );
        }
    }
    info!(
        "batch profile total: camera3d_and_shadow_views {view_count}, direct_views {direct_views}, shadow_views {shadow_views}, direct_shadow_views {direct_shadows}, view_phase_records {}, jobs {total_jobs}, instances_across_phases {total_instances}, singleton_jobs {singleton_jobs}, metadata_entries {}, instance_buffer_capacity_bytes {instance_capacity}, bin_index_buffer_capacity_bytes {index_capacity} (capacities are not uploaded byte counts; jobs are per view/phase, not unique scene meshes)",
        buffers.view_phase_buffers.len(),
        buffers.bin_unpacking_metadata.len()
    );
}
