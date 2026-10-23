//! Opt-in stall diagnostics: DSOR_FLAGS=frameprofile (web: ?flag=frameprofile).
//! Raw frame intervals include rendering/presentation waits. Main-schedule and
//! individual system times are CPU wall times, not GPU timings.

use bevy::prelude::*;
use super::cpu;

mod render;
mod batches;

// Combat hitches can miss the older 50 ms stall threshold entirely. Keep the
// finer system accounting opt-in: the mutex and report have no normal-play cost.
#[derive(Default)]
struct SystemTiming { count: u64, total: f64, maximum: f64 }
static COMBAT_TIMINGS: std::sync::Mutex<std::collections::BTreeMap<&'static str, SystemTiming>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

pub(super) fn enabled() -> bool { crate::flag("frameprofile") || crate::flag("combatprofile") }

fn report_combat_timings() {
    if !crate::flag("combatprofile") { return; }
    let timings = std::mem::take(&mut *COMBAT_TIMINGS.lock().unwrap());
    for (name, timing) in timings {
        info!("combat system profile: {name} mean {:.3} ms, max {:.3} ms, samples {} (system wall time)",
            timing.total / timing.count as f64, timing.maximum, timing.count);
    }
}

/// Schedule boundaries avoid assigning native render-thread waits to gameplay.
/// Enable separately from frameprofile so normal A/B timings stay inexpensive.
#[derive(bevy::ecs::schedule::ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct ProfileBoundary(u8);

#[derive(Resource, Default)]
struct StageClock {
    previous: f64,
    reported: f64,
    sums: [f64; 6],
    maxima: [f64; 6],
    counts: [u32; 6],
}

pub(super) fn install(app: &mut App) {
    if crate::flag("gpuprofile") {
        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin)
            .add_systems(Last, report_gpu);
    }
    if !crate::flag("stageprofile") { return; }
    render::install_extraction(app);
    app.init_resource::<StageClock>();
    install_post_update_intervals(app);
    {
        let mut order = app.world_mut().resource_mut::<bevy::app::MainScheduleOrder>();
        order.insert_after(First, ProfileBoundary(0));
        order.insert_after(PreUpdate, ProfileBoundary(1));
        order.insert_after(Update, ProfileBoundary(2));
        order.insert_after(bevy::app::SpawnScene, ProfileBoundary(3));
        order.insert_after(PostUpdate, ProfileBoundary(4));
        order.insert_after(Last, ProfileBoundary(5));
    }
    for i in 0..6 {
        app.add_systems(ProfileBoundary(i), move |mut clock: ResMut<StageClock>| {
            let now = cpu::now();
            if clock.previous > 0.0 {
                let ms = now - clock.previous;
                clock.sums[i as usize] += ms;
                clock.maxima[i as usize] = clock.maxima[i as usize].max(ms);
                clock.counts[i as usize] += 1;
            }
            clock.previous = now;
            if i == 5 && now - clock.reported >= 5000.0 {
                for (j, name) in ["between_frames_and_first", "pre_update", "fixed_and_update", "spawn_scene", "post_update", "last"].iter().enumerate() {
                    info!("stage profile: {name} mean {:.3} ms, max {:.3} ms, samples {}",
                        clock.sums[j] / clock.counts[j].max(1) as f64, clock.maxima[j], clock.counts[j]);
                }
                clock.sums = [0.0; 6];
                clock.maxima = [0.0; 6];
                clock.counts = [0; 6];
                clock.reported = now;
            }
        });
    }
}

/// These boundaries measure elapsed wall time around scheduled work. Other
/// runnable systems can execute between either boundary and its measured set;
/// the intervals overlap and must not be summed or treated as exclusive CPU
/// costs. They are only installed with stageprofile, leaving ordinary schedules
/// and their ordering untouched.
#[derive(Resource, Default)]
struct PostIntervalClock<const INDEX: usize> {
    start: Option<f64>,
    reported: f64,
    sum: f64,
    maximum: f64,
    samples: u32,
}

fn install_post_update_intervals(app: &mut App) {
    use bevy::app::AnimationSystems;
    use bevy::transform::TransformSystems;

    app.init_resource::<PostIntervalClock<0>>()
        .init_resource::<PostIntervalClock<1>>()
        .init_resource::<PostIntervalClock<2>>()
        .add_systems(PostUpdate, (
            begin_post_interval::<0>.before(AnimationSystems),
            end_post_interval::<0>.after(AnimationSystems).before(TransformSystems::Propagate),
            begin_post_interval::<1>
                .in_set(AnimationSystems)
                .after(bevy::animation::advance_animations)
                .before(bevy::animation::animate_targets),
            end_post_interval::<1>
                .in_set(AnimationSystems)
                .after(bevy::animation::animate_targets)
                .before(bevy::animation::transition::expire_completed_transitions),
            begin_post_interval::<2>
                .after(end_post_interval::<0>)
                .before(TransformSystems::Propagate),
            end_post_interval::<2>.after(TransformSystems::Propagate),
        ));
}

fn begin_post_interval<const INDEX: usize>(mut clock: ResMut<PostIntervalClock<INDEX>>) {
    clock.start = Some(cpu::now());
}

fn end_post_interval<const INDEX: usize>(mut clock: ResMut<PostIntervalClock<INDEX>>) {
    let now = cpu::now();
    let Some(start) = clock.start.take() else { return };
    let ms = now - start;
    clock.sum += ms;
    clock.maximum = clock.maximum.max(ms);
    clock.samples += 1;
    if now - clock.reported >= 5000.0 {
        let name = ["animation_set", "animate_targets", "transform_propagation_set"][INDEX];
        info!("post-update wall interval: {name} mean {:.3} ms, max {:.3} ms, samples {} (may include parallel work; not exclusive CPU time)",
            clock.sum / clock.samples as f64, clock.maximum, clock.samples);
        clock.reported = now;
        clock.sum = 0.0;
        clock.maximum = 0.0;
        clock.samples = 0;
    }
}

fn report_gpu(diagnostics: Res<bevy::diagnostic::DiagnosticsStore>, mut last: Local<f64>) {
    let now = cpu::now();
    if now - *last < 5000.0 { return; }
    *last = now;
    let mut passes: Vec<_> = diagnostics.iter()
        .filter(|d| d.path().as_str().ends_with("/elapsed_gpu"))
        .filter_map(|d| d.average().map(|ms| (d.path().as_str(), ms)))
        .collect();
    passes.sort_by(|a, b| b.1.total_cmp(&a.1));
    if passes.is_empty() {
        info!("gpu profile: GPU timestamps unavailable or no completed render samples");
    }
    // Spans may nest; never sum them into a purported total GPU frame time.
    for (path, ms) in passes.into_iter().take(12) {
        info!("gpu profile: {path} mean {ms:.3} ms");
    }
    let mut cpu_passes: Vec<_> = diagnostics.iter()
        .filter(|d| d.path().as_str().ends_with("/elapsed_cpu"))
        .filter_map(|d| d.average().map(|ms| (d.path().as_str(), ms)))
        .collect();
    cpu_passes.sort_by(|a, b| b.1.total_cmp(&a.1));
    for (path, ms) in cpu_passes.into_iter().take(8) {
        info!("render pass CPU profile: {path} mean {ms:.3} ms");
    }
}

#[derive(Resource, Default)]
struct RenderClock {
    start: f64,
    submit: f64,
    reported: f64,
    sums: [f64; 2],
    maxima: [f64; 2],
    samples: u32,
}

pub(super) fn install_render(render: &mut SubApp) {
    use bevy::render::{Render, RenderSystems};
    self::render::install_executor_modes(render);
    if crate::flag("stageprofile") { self::render::install_stages(render); }
    render.init_resource::<RenderClock>().add_systems(Render, (
        (|mut clock: ResMut<RenderClock>| clock.start = cpu::now())
            .before(RenderSystems::ExtractCommands),
        (|mut clock: ResMut<RenderClock>| clock.submit = cpu::now())
            .after(RenderSystems::Prepare).before(RenderSystems::Render),
        (|mut clock: ResMut<RenderClock>, cache: Res<bevy::render::render_resource::PipelineCache>| {
            let now = cpu::now();
            let intervals = [clock.submit - clock.start, now - clock.submit];
            if enabled() && now - clock.start >= 20.0 {
                info!("frame profile: render {:.2} ms (prepare {:.2}, submit/present {:.2}), pipelines pending {}",
                    now - clock.start, intervals[0], intervals[1], cache.waiting_pipelines().count());
            }
            if crate::flag("stageprofile") {
                for (i, ms) in intervals.into_iter().enumerate() {
                    clock.sums[i] += ms;
                    clock.maxima[i] = clock.maxima[i].max(ms);
                }
                clock.samples += 1;
                if now - clock.reported >= 5000.0 {
                    for (i, name) in ["prepare", "submit_present"].iter().enumerate() {
                        info!("render CPU wall interval: {name} mean {:.3} ms, max {:.3} ms, samples {} (includes scheduling/presentation waits; not GPU time)",
                            clock.sums[i] / clock.samples as f64, clock.maxima[i], clock.samples);
                    }
                    clock.reported = now;
                    clock.sums = [0.0; 2];
                    clock.maxima = [0.0; 2];
                    clock.samples = 0;
                }
            }
        }).after(RenderSystems::Render).before(RenderSystems::Cleanup),
    ).run_if(|| enabled() || crate::flag("stageprofile")));
}

pub(crate) struct FrameSpan {
    name: &'static str,
    start: Option<f64>,
}

impl FrameSpan {
    pub(crate) fn new(name: &'static str) -> Self {
        Self { name, start: enabled().then(cpu::now) }
    }
}

impl Drop for FrameSpan {
    fn drop(&mut self) {
        if let Some(start) = self.start {
            let ms = cpu::now() - start;
            if crate::flag("combatprofile") {
                let mut timings = COMBAT_TIMINGS.lock().unwrap();
                let timing = timings.entry(self.name).or_default();
                timing.count += 1;
                timing.total += ms;
                timing.maximum = timing.maximum.max(ms);
            }
            if ms >= 5.0 { info!("frame profile: {} took {ms:.2} ms", self.name); }
        }
    }
}

#[derive(Default)]
pub(super) struct Samples {
    previous: Option<f64>,
    reported: f64,
    frames: Vec<f64>,
}

pub(super) fn sample(mut samples: Local<Samples>,
    map: Option<Res<crate::world::map::CurrentMap>>,
    players: Query<&GlobalTransform, With<crate::network::net::LocalPlayer>>,
    loading: Res<crate::screens::loading::LoadingScreen>,
    work: Option<Res<crate::ui::UiWork>>,
    mut reported_work: Local<crate::ui::UiWork>) {
    let now = cpu::now();
    let previous = samples.previous.replace(now);
    let Some(previous) = previous else { return };
    let ms = now - previous;
    samples.frames.push(ms);
    if ms >= if crate::flag("combatprofile") { 1000.0 / 60.0 } else { 50.0 } {
        info!("frame spike: {ms:.2} ms, previous main {:.2} ms, loading {}, map {}, player {:?}",
            cpu::get(&cpu::RAW_MAIN), loading.active(),
            map.as_ref().map_or("-", |m| m.name.as_str()), players.iter().next().map(|p| p.translation()));
    }
    if now - samples.reported >= 5000.0 {
        samples.reported = now;
        samples.frames.sort_unstable_by(f64::total_cmp);
        let frames = &samples.frames;
        let percentile = |p: f64| frames[((frames.len() - 1) as f64 * p) as usize];
        info!("frame profile: {} frames, p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, max {:.2} ms, over50 {}",
            frames.len(), percentile(0.5), percentile(0.95), percentile(0.99), frames[frames.len() - 1],
            frames.iter().filter(|&&ms| ms >= 50.0).count());
        // Idle, these stay at 0 (docs/ui.md, "Per-frame cost"); a count near
        // the frame count means the interface rebuilds something every frame.
        if let Some(work) = work.as_deref() {
            info!("ui work: {} layouts prepared, {} system views, {} quest lists, {} status bars, {} previews in {} frames",
                work.layouts_prepared - reported_work.layouts_prepared, work.system_views - reported_work.system_views,
                work.quest_lists - reported_work.quest_lists, work.status_bars - reported_work.status_bars,
                work.previews - reported_work.previews, frames.len());
            *reported_work = *work;
        }
        samples.frames.clear();
        report_combat_timings();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_update_profile_boundaries_fit_animation_and_transform_schedules() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default(),
            bevy::animation::AnimationPlugin, bevy::transform::TransformPlugin));
        install_post_update_intervals(&mut app);
        // Building and running Bevy's actual schedule catches dependency cycles
        // between the nested animation interval and the surrounding system sets.
        app.update();
        app.update();
        fn completed<const INDEX: usize>(app: &App) {
            let clock = app.world().resource::<PostIntervalClock<INDEX>>();
            assert!(clock.start.is_none());
            assert!(clock.samples > 0);
            assert!(clock.sum >= 0.0);
        }
        completed::<0>(&app);
        completed::<1>(&app);
        completed::<2>(&app);
    }
}
