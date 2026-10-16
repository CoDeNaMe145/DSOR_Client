//! Opt-in render wall-time diagnostics. Intervals around system sets may include
//! runnable parallel work and waits; they are not exclusive CPU timings and must
//! not be added together. Extraction wrappers measure the original callbacks,
//! without replacing their work or changing the renderer's thread handoff.

use super::super::cpu;
use bevy::prelude::*;
use bevy::render::{
    Extract, ExtractSchedule, Render, RenderApp, RenderSystems,
    renderer::{RenderGraph, RenderGraphSystems, render_system},
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

/// Diagnostic executor comparisons retain the separate render thread. Main-world
/// systems and the compute/asset thread pools keep their existing configuration.
pub(super) fn install_executor_modes(render: &mut SubApp) {
    if crate::flag("singlerender") {
        single_threaded(render, Render);
    }
    if crate::flag("singlegraph") {
        single_threaded(render, RenderGraph);
        single_threaded(render, bevy::core_pipeline::schedule::Core3d);
        single_threaded(render, bevy::core_pipeline::schedule::Core2d);
    }
}

fn single_threaded(render: &mut SubApp, label: impl bevy::ecs::schedule::ScheduleLabel) {
    let label = label.intern();
    if let Some(schedule) = render.get_schedule_mut(label) {
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        info!(
            "render executor profile: {label:?} uses SingleThreadedExecutor (render pipelining unchanged)"
        );
    }
}

#[derive(Default)]
struct Samples {
    sum: f64,
    maximum: f64,
    count: u32,
    reported: f64,
}

impl Samples {
    fn sample(&mut self, name: &str, ms: f64) {
        self.sum += ms;
        self.maximum = self.maximum.max(ms);
        self.count += 1;
        let now = cpu::now();
        if now - self.reported >= 5000.0 {
            info!(
                "render detailed wall interval: {name} mean {:.3} ms, max {:.3} ms, samples {} (includes scheduling/waits; intervals may overlap; not exclusive CPU or GPU time)",
                self.sum / self.count as f64,
                self.maximum,
                self.count
            );
            self.sum = 0.0;
            self.maximum = 0.0;
            self.count = 0;
            self.reported = now;
        }
    }
}

pub(super) fn install_extraction(app: &mut App) {
    app.add_systems(Startup, || {
        info!(
            "task pool profile: compute_threads {:?}, async_compute_threads {:?}, io_threads {:?}",
            bevy::tasks::ComputeTaskPool::try_get().map(|pool| pool.thread_num()),
            bevy::tasks::AsyncComputeTaskPool::try_get().map(|pool| pool.thread_num()),
            bevy::tasks::IoTaskPool::try_get().map(|pool| pool.thread_num())
        );
    });
    let measured_extract = Arc::new(AtomicU64::new(f64::NAN.to_bits()));
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        wrap_extraction(render, measured_extract.clone());
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(handoff) = app.get_sub_app_mut(bevy::render::pipelined_rendering::RenderExtractApp)
    {
        let Some(mut original) = handoff.take_extract() else {
            return;
        };
        let mut total = Samples::default();
        let mut waiting = Samples::default();
        handoff.set_extract(move |main, render| {
            measured_extract.store(f64::NAN.to_bits(), Ordering::Relaxed);
            let start = cpu::now();
            original(main, render);
            let elapsed = cpu::now() - start;
            let extraction = f64::from_bits(measured_extract.load(Ordering::Relaxed));
            total.sample("main_render_handoff_with_extract", elapsed);
            if extraction.is_finite() {
                // Bevy services main-thread work while awaiting the render
                // world. This residual is not pure blocking or idle time.
                waiting.sample(
                    "main_wait_for_renderer_and_main_thread_tasks",
                    (elapsed - extraction).max(0.0),
                );
            }
        });
    }
}

fn wrap_extraction(render: &mut SubApp, measured: Arc<AtomicU64>) {
    let Some(mut original) = render.take_extract() else {
        return;
    };
    let mut samples = Samples::default();
    render.set_extract(move |main, render| {
        let start = cpu::now();
        original(main, render);
        let elapsed = cpu::now() - start;
        measured.store(elapsed.to_bits(), Ordering::Relaxed);
        samples.sample("entity_sync_and_extract_callback", elapsed);
    });
}

#[derive(Resource, Default)]
struct Interval<const INDEX: usize> {
    start: Option<f64>,
    samples: Samples,
}

const NAMES: [&str; 26] = [
    "apply_extracted_commands",
    "prepare_assets",
    "prepare_meshes",
    "create_views",
    "specialize",
    "prepare_views",
    "queue",
    "phase_sort",
    "prepare_resources",
    "batch_phases",
    "write_phase_buffers",
    "collect_phase_buffers",
    "flush_resources",
    "prepare_bind_groups",
    "render_set",
    "cleanup",
    "post_cleanup",
    "acquire_swapchain_interval",
    "extract_skins_interval",
    "upload_skins_interval",
    "render_graph_begin",
    "render_graph_views",
    "render_graph_submit",
    "render_graph_finish",
    "readback_submit_present_tail",
    "render_graph_total",
];

fn begin<const INDEX: usize>(mut clock: ResMut<Interval<INDEX>>) {
    clock.start = Some(cpu::now());
}

fn end<const INDEX: usize>(mut clock: ResMut<Interval<INDEX>>) {
    let Some(start) = clock.start.take() else {
        return;
    };
    clock.samples.sample(NAMES[INDEX], cpu::now() - start);
}

pub(super) fn install_stages(render: &mut SubApp) {
    super::batches::install_census(render);
    macro_rules! render_interval {
        ($index:literal, $previous:expr, $set:expr, $next:expr) => {
            render.init_resource::<Interval<$index>>().add_systems(
                Render,
                (
                    begin::<$index>.after($previous).before($set),
                    end::<$index>.after($set).before($next),
                ),
            );
        };
    }
    // Anchor both ends to neighboring existing phases, so a queue timer cannot
    // start early in asset preparation or finish after the renderer presents.
    render.init_resource::<Interval<0>>().add_systems(
        Render,
        (
            begin::<0>.before(RenderSystems::ExtractCommands),
            end::<0>
                .after(RenderSystems::ExtractCommands)
                .before(RenderSystems::PrepareAssets),
        ),
    );
    render_interval!(
        1,
        RenderSystems::ExtractCommands,
        RenderSystems::PrepareAssets,
        RenderSystems::PrepareMeshes
    );
    render_interval!(
        2,
        RenderSystems::PrepareAssets,
        RenderSystems::PrepareMeshes,
        RenderSystems::CreateViews
    );
    render_interval!(
        3,
        RenderSystems::PrepareMeshes,
        RenderSystems::CreateViews,
        RenderSystems::Specialize
    );
    render_interval!(
        4,
        RenderSystems::CreateViews,
        RenderSystems::Specialize,
        RenderSystems::PrepareViews
    );
    render_interval!(
        5,
        RenderSystems::Specialize,
        RenderSystems::PrepareViews,
        RenderSystems::Queue
    );
    render_interval!(
        6,
        RenderSystems::PrepareViews,
        RenderSystems::Queue,
        RenderSystems::PhaseSort
    );
    render_interval!(
        7,
        RenderSystems::Queue,
        RenderSystems::PhaseSort,
        RenderSystems::Prepare
    );
    render_interval!(
        8,
        RenderSystems::PhaseSort,
        RenderSystems::PrepareResources,
        RenderSystems::PrepareResourcesBatchPhases
    );
    render_interval!(
        9,
        RenderSystems::PrepareResources,
        RenderSystems::PrepareResourcesBatchPhases,
        RenderSystems::PrepareResourcesWritePhaseBuffers
    );
    render_interval!(
        10,
        RenderSystems::PrepareResourcesBatchPhases,
        RenderSystems::PrepareResourcesWritePhaseBuffers,
        RenderSystems::PrepareResourcesCollectPhaseBuffers
    );
    render_interval!(
        11,
        RenderSystems::PrepareResourcesWritePhaseBuffers,
        RenderSystems::PrepareResourcesCollectPhaseBuffers,
        RenderSystems::PrepareResourcesFlush
    );
    render_interval!(
        12,
        RenderSystems::PrepareResourcesCollectPhaseBuffers,
        RenderSystems::PrepareResourcesFlush,
        RenderSystems::PrepareBindGroups
    );
    render_interval!(
        13,
        RenderSystems::PrepareResourcesFlush,
        RenderSystems::PrepareBindGroups,
        RenderSystems::Render
    );
    render_interval!(
        14,
        RenderSystems::Prepare,
        RenderSystems::Render,
        RenderSystems::Cleanup
    );
    render_interval!(
        15,
        RenderSystems::Render,
        RenderSystems::Cleanup,
        RenderSystems::PostCleanup
    );
    render.init_resource::<Interval<16>>().add_systems(
        Render,
        (
            begin::<16>
                .after(RenderSystems::Cleanup)
                .before(RenderSystems::PostCleanup),
            end::<16>.after(RenderSystems::PostCleanup),
        ),
    );
    render.init_resource::<Interval<17>>().add_systems(
        Render,
        (
            begin::<17>.before(bevy::render::view::window::prepare_windows),
            end::<17>.after(bevy::render::view::window::prepare_windows),
        )
            .in_set(RenderSystems::PrepareViews),
    );
    render.init_resource::<Interval<18>>().add_systems(
        ExtractSchedule,
        (
            begin::<18>.before(bevy::pbr::extract_skins),
            end::<18>.after(bevy::pbr::extract_skins),
            skin_buffers.after(bevy::pbr::extract_skins),
        ),
    );
    render.init_resource::<Interval<19>>().add_systems(
        Render,
        (
            begin::<19>.before(bevy::pbr::prepare_skins),
            end::<19>.after(bevy::pbr::prepare_skins),
        )
            .in_set(RenderSystems::PrepareResources),
    );

    render.init_resource::<Interval<20>>().add_systems(
        RenderGraph,
        (
            begin::<20>.before(RenderGraphSystems::Begin),
            end::<20>
                .after(RenderGraphSystems::Begin)
                .before(RenderGraphSystems::Render),
        ),
    );
    render.init_resource::<Interval<21>>().add_systems(
        RenderGraph,
        (
            begin::<21>
                .after(RenderGraphSystems::Begin)
                .before(RenderGraphSystems::Render),
            end::<21>
                .after(RenderGraphSystems::Render)
                .before(RenderGraphSystems::Submit),
        ),
    );
    render.init_resource::<Interval<22>>().add_systems(
        RenderGraph,
        (
            begin::<22>
                .after(RenderGraphSystems::Render)
                .before(RenderGraphSystems::Submit),
            end::<22>
                .after(RenderGraphSystems::Submit)
                .before(RenderGraphSystems::Finish),
        ),
    );
    render.init_resource::<Interval<23>>().add_systems(
        RenderGraph,
        (
            begin::<23>
                .after(RenderGraphSystems::Submit)
                .before(RenderGraphSystems::Finish),
            end::<23>.after(RenderGraphSystems::Finish),
        ),
    );
    render
        .init_resource::<Interval<24>>()
        .init_resource::<Interval<25>>()
        .add_systems(RenderGraph, begin::<25>.before(RenderGraphSystems::Begin))
        .add_systems(
            RenderGraph,
            (end::<25>, begin::<24>)
                .chain()
                .after(RenderGraphSystems::Finish),
        )
        .add_systems(
            Render,
            end::<24>.after(render_system).in_set(RenderSystems::Render),
        );
}

fn skin_buffers(
    skins: Option<Res<bevy::pbr::SkinUniforms>>,
    meshes: Extract<Query<(&bevy::mesh::skinning::SkinnedMesh, &ViewVisibility)>>,
    mut last: Local<f64>,
) {
    let now = cpu::now();
    if now - *last < 5000.0 {
        return;
    }
    let Some(skins) = skins else { return };
    *last = now;
    let (mut allocated, mut palettes, mut visible, mut visible_palettes) = (0, 0, 0, 0);
    for entity in skins.all_skins() {
        allocated += 1;
        if let Ok((skin, _)) = meshes.get(**entity) {
            palettes += skin.joints.len();
        }
    }
    for (skin, visibility) in &meshes {
        if visibility.get() {
            visible += 1;
            visible_palettes += skin.joints.len();
        }
    }
    let high_water = skins.current_staging_buffer.len();
    info!(
        "skin buffer profile: allocated_skin_records {allocated}, allocated_palette_entries {palettes}, visible_meshes {visible}, visible_palette_entries {visible_palettes}, staging_high_water_entries {high_water}, unused_or_padding_entries {}, upload_bytes_per_frame {}, buffer_capacity_bytes {}",
        high_water.saturating_sub(palettes),
        high_water * std::mem::size_of::<Mat4>(),
        skins.current_buffer.size()
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Resource, Default)]
    struct ExtractCalls(u32);

    #[test]
    fn extraction_wrapper_preserves_original_callback_and_worlds() {
        let mut main = World::new();
        main.insert_resource(ExtractCalls(3));
        let mut render = SubApp::new();
        render.set_extract(|main, render| {
            main.resource_mut::<ExtractCalls>().0 += 1;
            render.insert_resource(ExtractCalls(main.resource::<ExtractCalls>().0));
        });
        let measured = Arc::new(AtomicU64::new(f64::NAN.to_bits()));
        wrap_extraction(&mut render, measured.clone());
        render.extract(&mut main);
        assert_eq!(main.resource::<ExtractCalls>().0, 4);
        assert_eq!(render.world().resource::<ExtractCalls>().0, 4);
        assert!(f64::from_bits(measured.load(Ordering::Relaxed)).is_finite());
    }

    #[test]
    fn detailed_boundaries_fit_bevy_render_and_graph_schedules() {
        let mut render = SubApp::new();
        // Extract parameters initialize against the temporary main-world
        // resource which Bevy normally inserts before running this schedule.
        render.init_resource::<bevy::render::MainWorld>();
        render.add_schedule(Render::base_schedule());
        render.add_schedule(RenderGraph::base_schedule());
        render.init_schedule(ExtractSchedule);
        // Real systems supply their normal nested-set membership; they are not
        // run here, so this validates ordering without a window or GPU device.
        render.add_systems(
            Render,
            (
                bevy::render::view::window::prepare_windows.in_set(RenderSystems::PrepareViews),
                bevy::pbr::prepare_skins.in_set(RenderSystems::PrepareResources),
                render_system.in_set(RenderSystems::Render),
            ),
        );
        render.add_systems(ExtractSchedule, bevy::pbr::extract_skins);
        install_stages(&mut render);
        render
            .world_mut()
            .schedule_scope(Render, |world, schedule| {
                schedule.initialize(world).unwrap()
            });
        render
            .world_mut()
            .schedule_scope(RenderGraph, |world, schedule| {
                schedule.initialize(world).unwrap()
            });
        render
            .world_mut()
            .schedule_scope(ExtractSchedule, |world, schedule| {
                schedule.initialize(world).unwrap()
            });
    }
}
