//! Per-system wall times: `DSOR_FLAGS=systemprofile` on a client built with
//! `--features system-profile` (bevy's `trace` spans: one per system run, one
//! per schedule). Every five seconds the log lists each module's systems' time
//! a frame, then the costliest systems and schedules by their total time, with
//! their mean and maximum a run:
//!
//! ```text
//! system profile module:   0.262 ms a frame   62 systems  dsor_client::ui
//! system profile:  494.117 ms/s  mean 5.954 ms  max 7.651 ms  runs   415  system: dsor_client::ui::spawn_windows
//! ```
//!
//! `DSOR_SYSTEM_PROFILE=dsor_client::ui` lists every system whose name holds
//! that text instead of the costliest 45. The render world's systems run inside
//! `bevy_render::run_render_schedule`, so bevy_render's line includes theirs.
//!
//! This is how the 2026-10-21 interface regression was found (docs/ui.md,
//! "Per-frame cost"; docs/testing-and-diagnostics.md). Wall times include waits
//! and parallel work: schedules contain their systems and must not be added to
//! them. The spans cost a little themselves, so compare builds that both have
//! the feature.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use bevy::log::tracing::{self, span};
use bevy::log::tracing_subscriber::{layer::Context, Layer, Registry};
use bevy::prelude::*;

/// How many rows a report lists.
const ROWS: usize = 45;

/// The span kinds measured, as bevy names them.
const KINDS: [&str; 3] = ["system", "system_commands", "schedule"];

/// What a measured span is called in the report ("system: path::to::system").
struct SpanName(String);
struct SpanStart(Instant);

/// (runs, total ms, longest ms) by span name, since `since`.
struct Totals {
    since: Instant,
    spans: HashMap<String, (u64, f64, f64)>,
}

static TOTALS: Mutex<Option<Totals>> = Mutex::new(None);

/// The layer for LogPlugin::custom_layer (crate::diagnostics::dump::log_layer).
pub(crate) struct SystemTimer {
    /// DSOR_SYSTEM_PROFILE=<text>: list every span whose name contains it
    /// (dsor_client::ui: the whole interface), not only the costliest.
    only: Option<String>,
}

impl SystemTimer {
    /// The layer when asked for, None otherwise.
    pub(crate) fn wanted() -> Option<Self> {
        if !crate::flag("systemprofile") { return None; }
        if !cfg!(feature = "system-profile") {
            // The logger is not up yet.
            eprintln!("DSOR_FLAGS=systemprofile needs a client built with --features system-profile");
        }
        Some(Self { only: std::env::var("DSOR_SYSTEM_PROFILE").ok().filter(|s| !s.is_empty()) })
    }
}

struct NameField(String);

impl tracing::field::Visit for NameField {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "name" { value.clone_into(&mut self.0); }
    }
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "name" { self.0 = format!("{value:?}"); }
    }
}

impl Layer<Registry> for SystemTimer {
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, Registry>) {
        let kind = attrs.metadata().name();
        if !KINDS.contains(&kind) { return; }
        let mut name = NameField(String::new());
        attrs.record(&mut name);
        if let Some(span) = ctx.span(id) { span.extensions_mut().replace(SpanName(format!("{kind}: {}", name.0))); }
    }

    fn on_enter(&self, id: &span::Id, ctx: Context<'_, Registry>) {
        let Some(span) = ctx.span(id) else { return };
        let mut extensions = span.extensions_mut();
        if extensions.get_mut::<SpanName>().is_some() { extensions.replace(SpanStart(Instant::now())); }
    }

    fn on_exit(&self, id: &span::Id, ctx: Context<'_, Registry>) {
        let Some(span) = ctx.span(id) else { return };
        let mut extensions = span.extensions_mut();
        let Some(SpanStart(start)) = extensions.remove::<SpanStart>() else { return };
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let Some(SpanName(name)) = extensions.get_mut::<SpanName>() else { return };
        let report = {
            let mut totals = TOTALS.lock().unwrap();
            let totals = totals.get_or_insert_with(|| Totals { since: Instant::now(), spans: HashMap::new() });
            // Names are allocated once a system, not once a run.
            if !totals.spans.contains_key(name.as_str()) { totals.spans.insert(name.clone(), Default::default()); }
            let entry = totals.spans.get_mut(name.as_str()).unwrap();
            entry.0 += 1;
            entry.1 += ms;
            entry.2 = entry.2.max(ms);
            let seconds = totals.since.elapsed().as_secs_f64();
            (seconds >= 5.0).then(|| {
                totals.since = Instant::now();
                (seconds, std::mem::take(&mut totals.spans))
            })
        };
        // Log outside the span's extensions and the lock: logging re-enters the layers.
        drop(extensions);
        let Some((seconds, spans)) = report else { return };
        let frames = spans.get("schedule: Main").map_or(0, |s| s.0).max(1) as f64;
        // The systems by module (dsor_client::ui, bevy_render...), a frame: the
        // interface's whole cost in one line (docs/ui.md, "Per-frame cost").
        let mut modules: HashMap<String, (usize, f64)> = HashMap::new();
        for (name, (_, total, _)) in &spans {
            let Some(path) = name.strip_prefix("system: ") else { continue };
            let module = path.split("::").take(if path.starts_with("dsor_client::") { 2 } else { 1 }).collect::<Vec<_>>().join("::");
            let entry = modules.entry(module).or_default();
            entry.0 += 1;
            entry.1 += total;
        }
        let mut modules: Vec<_> = modules.into_iter().collect();
        modules.sort_by(|a, b| b.1.1.total_cmp(&a.1.1));
        for (module, (systems, total)) in modules.into_iter().take(12) {
            info!("system profile module: {:7.3} ms a frame  {systems:3} systems  {module}", total / frames);
        }
        let mut rows: Vec<_> = spans.into_iter().filter(|(name, _)| self.only.as_ref().is_none_or(|only| name.contains(only.as_str()))).collect();
        rows.sort_by(|a, b| b.1.1.total_cmp(&a.1.1));
        for (name, (runs, total, longest)) in rows.into_iter().take(if self.only.is_some() { usize::MAX } else { ROWS }) {
            info!("system profile: {:8.3} ms/s  mean {:.3} ms  max {:.3} ms  runs {runs:5}  {name}",
                total / seconds, total / runs as f64, longest);
        }
    }
}
