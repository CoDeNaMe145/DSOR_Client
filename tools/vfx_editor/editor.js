// DSOR VFX editor (tools/vfx_editor/server.py serves this page).
//
// Three things make a skill's look, all editable here:
//   - its sequences (skills/sequences.json): a timeline of tracks at 25 frames a
//     second -- effect models (fx), lights, camera shakes, the caster's animation;
//   - the effect models a fx track draws: their nodes' extras, as the client reads
//     them -- particle emitters (dsor_emitter, client/src/particles.rs), shader
//     values (dsor_shader), their animators (dsor_anim), UV scroll (dsor_vector);
//   - the textures of those models' materials.
// Every edit is sent at once: the server keeps it in vfx-overrides/, applies it to
// the assets and the preview client replays the selection with it.
"use strict";

const $ = (s, r = document) => r.querySelector(s);
const el = (tag, attrs = {}, ...kids) => {
  const e = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") e.className = v;
    else if (k.startsWith("on")) e.addEventListener(k.slice(2), v);
    else if (v === true) e.setAttribute(k, "");
    else if (v !== false && v != null) e.setAttribute(k, v);
  }
  for (const c of kids.flat()) if (c != null) e.append(c.nodeType ? c : document.createTextNode(c));
  return e;
};
const clone = (v) => JSON.parse(JSON.stringify(v));
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const FPS = 25;

async function api(path, body) {
  const r = await fetch(path, body === undefined ? {} : { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const j = await r.json();
  if (!r.ok) throw new Error(j.error || r.statusText);
  return j;
}

const S = {
  catalog: null,
  tab: "skills",
  sel: null,        // {type: "skill"|"seq"|"fx", name}
  doc: null,        // the document being edited (sequence or effect)
  history: [], future: [],
  track: null,      // selected track index (sequence)
  node: null,       // selected node name (effect)
  saveTimer: null,
  playedAt: 0,
};

function setStatus(text, kind = "") {
  const s = $("#status");
  s.textContent = text;
  s.className = "status " + kind;
}

// -- browser -------------------------------------------------------------------
async function loadCatalog() {
  S.catalog = await api("/api/catalog");
  const cls = $("#class"), fc = $("#filter-class");
  cls.innerHTML = "";
  S.catalog.classes.forEach((c, i) => cls.append(el("option", { value: i, selected: i === 1 }, c)));
  fc.innerHTML = '<option value="">All classes</option>';
  for (const c of S.catalog.classes) fc.append(el("option", { value: c }, c));
  fc.append(el("option", { value: "-" }, "monsters / other"));
  renderList();
}

function edited(type, name) {
  const e = S.catalog.edited;
  if (type === "fx") return e.effects.includes(name);
  if (type === "seq") return e.sequences.includes(name);
  if (type === "skill") {
    const s = S.catalog.skills.find((x) => x.id === name);
    return s && s.seqs.some((q) => e.sequences.includes(q));
  }
  return false;
}

function renderList() {
  const q = $("#search").value.trim().toLowerCase();
  const fc = $("#filter-class").value;
  const onlyEdited = $("#only-edited").checked;
  const ul = $("#list");
  ul.innerHTML = "";
  $("#filter-class").hidden = S.tab !== "skills";
  let items = [];
  if (S.tab === "skills") {
    items = S.catalog.skills
      .filter((s) => !fc || (fc === "-" ? !s.class : s.class === fc))
      .map((s) => ({ type: "skill", name: s.id, note: s.class || s.type }));
  } else if (S.tab === "effects") {
    items = S.catalog.effects.map((e) => ({ type: "fx", name: e, note: e.split("/")[0] }));
  } else {
    items = S.catalog.sequence_names.map((n) => ({ type: "seq", name: n }));
  }
  items = items.filter((i) => (!q || i.name.toLowerCase().includes(q)) && (!onlyEdited || edited(i.type, i.name)));
  for (const it of items.slice(0, 1500)) {
    const label = it.type === "fx" ? it.name.split("/").pop() : it.name;
    const li = el("li", { class: S.sel && S.sel.type === it.type && S.sel.name === it.name ? "sel" : "", title: it.name,
      onclick: () => select(it.type, it.name) },
      edited(it.type, it.name) ? el("span", { class: "dot", title: "edited" }) : null, label, it.note ? el("small", {}, it.note) : null);
    ul.append(li);
  }
  if (items.length > 1500) ul.append(el("li", {}, el("small", {}, `${items.length - 1500} more: refine the search`)));
}

// -- selection -----------------------------------------------------------------
async function select(type, name, { play = true } = {}) {
  flushSave();
  S.sel = { type, name };
  S.history = []; S.future = [];
  S.track = null; S.node = null;
  $("#empty").hidden = true;
  $("#skill-view").hidden = type !== "skill";
  $("#seq-view").hidden = type !== "seq";
  $("#fx-view").hidden = type !== "fx";
  if (type === "skill") {
    S.doc = null;
    const s = S.catalog.skills.find((x) => x.id === name);
    $("#skill-title").textContent = `${name} ${s.class ? "· " + s.class : ""} · ${s.type}`;
    const ul = $("#skill-seqs");
    ul.innerHTML = "";
    for (const q of s.seqs) {
      ul.append(el("li", { onclick: () => select("seq", q, { play: false }) },
        edited("seq", q) ? el("span", { class: "dot" }) : null, " ", q));
    }
    if (!s.seqs.length) ul.append(el("li", {}, "no sequence"));
    inspectorEmpty("Select a sequence to edit its timeline.");
    if (play) preview("skill", name);
  } else if (type === "seq") {
    const d = await api("/api/sequence?name=" + encodeURIComponent(name));
    S.doc = { kind: "seq", name, seq: d.seq || { length: 25, repeat: false, tracks: [] }, original: d.original };
    renderSequence();
    inspectorEmpty("Click a track (its name or its bar).");
    // A sequence alone plays on the caster; a skill's own stays previewed as the skill.
    if (play) preview("sequence", name);
  } else {
    const d = await api("/api/effect?path=" + encodeURIComponent(name));
    S.doc = { kind: "fx", path: name, nodes: d.nodes, animations: d.animations };
    renderEffect();
    inspectorEmpty("Click an emitter or a surface.");
    if (play) preview("effect", name);
  }
  renderList();
}

// -- saving, undo --------------------------------------------------------------
function snapshot() {
  return S.doc.kind === "seq" ? clone(S.doc.seq) : clone(S.doc.nodes);
}
function restore(snap) {
  if (S.doc.kind === "seq") S.doc.seq = snap;
  else S.doc.nodes = snap;
}
// Call before changing S.doc: records undo, schedules the save.
function willChange() {
  S.history.push(snapshot());
  if (S.history.length > 200) S.history.shift();
  S.future = [];
}
function changed({ rerender = true } = {}) {
  clearTimeout(S.saveTimer);
  S.saveTimer = setTimeout(save, 350);
  setStatus("editing…");
  if (rerender) rerenderDoc();
}
function rerenderDoc() {
  if (S.doc.kind === "seq") { renderSequence(); inspectTrack(); }
  else { renderEffect(); inspectNode(); }
}
function flushSave() {
  if (S.saveTimer) { clearTimeout(S.saveTimer); save(); }
}
async function save() {
  S.saveTimer = null;
  const d = S.doc;
  if (!d) return;
  try {
    if (d.kind === "seq") {
      await api("/api/sequence", { name: d.name, seq: d.seq });
      markEdited("sequences", d.name, !same(d.seq, d.original));
    } else {
      const nodes = {}, materials = {};
      for (const n of d.nodes) {
        nodes[n.name] = n.extras;
        for (const m of n.materials) if (m.texture) materials[m.name] = m.texture;
      }
      await api("/api/effect", { path: d.path, nodes, materials });
      markEdited("effects", d.path, d.nodes.some((n) => !same(n.extras, n.original) || n.materials.some((m) => m.texture !== m.texture_orig)));
    }
    setStatus("saved · replaying", "ok");
  } catch (e) {
    setStatus("save failed: " + e.message, "bad");
  }
}
function markEdited(list, name, yes) {
  const l = S.catalog.edited[list];
  const i = l.indexOf(name);
  if (yes && i < 0) l.push(name);
  if (!yes && i >= 0) l.splice(i, 1);
  renderList();
}
function undo() {
  if (!S.doc || !S.history.length) return;
  S.future.push(snapshot());
  restore(S.history.pop());
  changed();
}
function redo() {
  if (!S.doc || !S.future.length) return;
  S.history.push(snapshot());
  restore(S.future.pop());
  changed();
}
async function revert() {
  if (!S.doc) return;
  if (!confirm("Drop every edit of this " + (S.doc.kind === "seq" ? "sequence" : "effect") + " and go back to the game's data?")) return;
  clearTimeout(S.saveTimer); S.saveTimer = null;
  if (S.doc.kind === "seq") {
    await api("/api/sequence", { name: S.doc.name, seq: null });
    markEdited("sequences", S.doc.name, false);
  } else {
    await api("/api/effect/revert", { path: S.doc.path });
    markEdited("effects", S.doc.path, false);
  }
  await select(S.sel.type, S.sel.name, { play: false });
  setStatus("reverted", "ok");
}

// -- preview -------------------------------------------------------------------
let lastOrder = null;
async function preview(kind, name) {
  lastOrder = { kind, name };
  S.playedAt = performance.now();
  try {
    await api("/api/preview", { kind, name, every: +$("#every").value || 3 });
  } catch (e) {
    setStatus(e.message, "bad");
  }
}
function play() {
  if (!S.sel) return;
  if (S.sel.type === "skill") preview("skill", S.sel.name);
  else if (S.sel.type === "seq") preview("sequence", S.sel.name);
  else preview("effect", S.sel.name);
}
async function pollStatus() {
  try {
    const s = await api("/api/status");
    if (!s.running) {
      if (!S.saveTimer) setStatus("preview closed: Open preview", "bad");
    } else if (s.error) setStatus("preview: " + s.error, "bad");
    else if (!S.saveTimer && s.rev >= s.sent && s.sent) setStatus("preview up to date", "ok");
  } catch (e) { /* server restarting */ }
}

// -- sequence timeline ---------------------------------------------------------
function trackLabel(t) {
  switch (t.kind) {
    case "fx": return (t.graphics || "?").split("/").pop() + (t.joint ? " @" + t.joint : "");
    case "anim": return t.name;
    case "sound": return (t.name || "").split("/").pop();
    case "light": return `r ${t.range} · i ${t.intensity}`;
    case "shake": return `${t.intensity}`;
    case "phase": return t.event || "";
    case "character": return (t.object || "").split("/").slice(1).join("/");
    default: return "";
  }
}

function renderSequence() {
  const d = S.doc, seq = d.seq;
  $("#seq-title").innerHTML = "";
  $("#seq-title").append(d.name, same(seq, d.original) ? "" : el("span", { class: "dot", title: "edited", style: "margin-left:8px" }));
  $("#seq-length").value = seq.length;
  $("#seq-repeat").checked = !!seq.repeat;
  const tl = $("#timeline");
  tl.innerHTML = "";
  const total = Math.max(seq.length, ...seq.tracks.map((t) => t.end || 0), 1);
  const ruler = el("div", { class: "ruler" });
  const step = total > 200 ? 25 : total > 80 ? 10 : 5;
  for (let f = 0; f <= total; f += step) {
    ruler.append(el("i", { style: `left:${(f / total) * 100}%` }));
    ruler.append(el("span", { style: `left:${(f / total) * 100}%` }, f % 25 === 0 ? `${f / 25}s` : `${f}`));
  }
  tl.append(ruler);
  seq.tracks.forEach((t, i) => {
    const lane = el("div", { class: "lane" });
    const bar = el("div", { class: "bar k-" + t.kind, title: `${t.kind} ${t.start}–${t.end}`,
      style: `left:${(t.start / total) * 100}%; width:${(Math.max(1, t.end - t.start) / total) * 100}%` },
      el("div", { class: "h l" }), el("div", { class: "h r" }));
    bar.addEventListener("pointerdown", (ev) => dragBar(ev, i, lane, total));
    lane.append(bar);
    tl.append(el("div", { class: "track" + (S.track === i ? " sel" : "") },
      el("div", { class: "label", onclick: () => { S.track = i; renderSequence(); inspectTrack(); } },
        el("b", {}, t.kind), trackLabel(t)), lane));
  });
  const head = el("div", { class: "playhead", id: "playhead", style: "left:190px", hidden: true });
  tl.append(head);
  tl.dataset.total = total;
}

function dragBar(ev, i, lane, total) {
  ev.preventDefault();
  S.track = i;
  const t = S.doc.seq.tracks[i];
  const mode = ev.target.classList.contains("l") ? "l" : ev.target.classList.contains("r") ? "r" : "m";
  const w = lane.getBoundingClientRect().width;
  const x0 = ev.clientX, s0 = t.start, e0 = t.end;
  let moved = false;
  const move = (e) => {
    const df = Math.round(((e.clientX - x0) / w) * total);
    if (!moved && df === 0) return;
    if (!moved) { willChange(); moved = true; }
    if (mode === "m") { t.start = Math.max(0, s0 + df); t.end = t.start + (e0 - s0); }
    else if (mode === "l") t.start = Math.min(Math.max(0, s0 + df), t.end - 1);
    else t.end = Math.max(t.start + 1, e0 + df);
    renderSequence();
  };
  const up = () => {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    if (moved) changed();
    else { renderSequence(); inspectTrack(); }
  };
  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
}

function addTrack(kind) {
  if (!S.doc || S.doc.kind !== "seq") return;
  willChange();
  const end = S.doc.seq.length || 25;
  const at = [0, 0, 0, 0, 0, 0, 1, 1, 1];
  const t = kind === "fx" ? { kind, graphics: S.catalog.effects[0] || "", joint: "", at, start: 0, end }
    : kind === "light" ? { kind, color: [1, 0.6, 0.3], intensity: 1, range: 4, at, flicker: false, start: 0, end }
    : { kind, intensity: 0.05, range: 15, delay: 0, start: 0, end: Math.min(end, 10) };
  S.doc.seq.tracks.push(t);
  S.track = S.doc.seq.tracks.length - 1;
  changed();
}

// -- inspector helpers ---------------------------------------------------------
function inspectorEmpty(text) {
  $("#insp-title").textContent = "Inspector";
  $("#insp").innerHTML = "";
  $("#insp").append(el("p", { class: "hint" }, text));
}
function group(title, ...kids) {
  return el("div", { class: "group" }, el("div", { class: "gh" }, title), ...kids);
}
// A number / text / checkbox field bound to obj[key].
function field(label, obj, key, { type = "number", step = "any", orig, onchange, options } = {}) {
  const v = obj[key];
  const isChanged = orig !== undefined && !same(orig, v);
  let input;
  if (options) {
    input = el("select", {}, options.map((o) => el("option", { value: o, selected: o === v }, o)));
  } else if (type === "checkbox") {
    input = el("input", { type: "checkbox", checked: !!v });
  } else {
    const shown = typeof v === "number" && !Number.isInteger(v) ? +v.toPrecision(6) : v;
    input = el("input", { type, step, value: shown ?? "" });
    if (type === "text" && typeof field.datalist === "string") input.setAttribute("list", field.datalist);
  }
  input.addEventListener("change", () => {
    let nv = type === "checkbox" ? (typeof v === "number" ? (input.checked ? 1 : 0) : input.checked)
      : type === "number" ? parseFloat(input.value) : input.value;
    if (type === "number" && !isFinite(nv)) { input.value = v; return; }
    willChange();
    obj[key] = nv;
    if (onchange) onchange(nv);
    changed();
  });
  return el("label", { class: "field" }, el("span", { class: isChanged ? "changed" : "", title: key }, label), input);
}
// [tx ty tz rx ry rz sx sy sz] of a track.
function transformFields(t, orig) {
  const names = ["X", "Y (up)", "Z", "Rot X°", "Rot Y°", "Rot Z°", "Scale X", "Scale Y", "Scale Z"];
  return group("Placement (from the caster / anchor)", ...names.map((n, i) => field(n, t.at, i, { orig: orig && orig.at ? orig.at[i] : undefined })));
}
function jsonBox(label, obj, key, orig) {
  const ta = el("textarea", { spellcheck: "false" });
  ta.value = JSON.stringify(obj[key], null, 1);
  ta.addEventListener("change", () => {
    try {
      const v = JSON.parse(ta.value);
      willChange();
      obj[key] = v;
      changed();
    } catch (e) {
      setStatus(`${label}: invalid JSON (${e.message})`, "bad");
    }
  });
  return el("div", { class: "wide" },
    el("div", { class: "gh" + (orig !== undefined && !same(orig, obj[key]) ? " changed" : "") }, label), ta);
}

function inspectTrack() {
  const i = S.track, seq = S.doc.seq;
  if (i == null || !seq.tracks[i]) return inspectorEmpty("Click a track (its name or its bar).");
  const t = seq.tracks[i];
  const orig = (S.doc.original && S.doc.original.tracks || [])[i];
  const o = (k) => (orig && orig.kind === t.kind ? orig[k] : undefined);
  $("#insp-title").textContent = `Track ${i + 1} · ${t.kind}`;
  const box = $("#insp");
  box.innerHTML = "";
  box.append(group("Timing (frames, 25 / s)", field("Start", t, "start", { step: 1, orig: o("start") }), field("End", t, "end", { step: 1, orig: o("end") })));
  if (t.kind === "fx") {
    field.datalist = "effects-list";
    box.append(group("Effect",
      field("Model", t, "graphics", { type: "text", orig: o("graphics") }),
      field("Joint (empty: at the caster)", t, "joint", { type: "text", orig: o("joint") }),
      el("button", { onclick: () => { flushSave(); S.tab = "effects"; setTab(); select("fx", t.graphics); } }, "Edit this effect model →")));
    field.datalist = null;
    box.append(transformFields(t, orig));
  } else if (t.kind === "light") {
    const color = el("input", { type: "color", value: "#" + t.color.map((c) => Math.round(Math.min(1, c) * 255).toString(16).padStart(2, "0")).join("") });
    color.addEventListener("change", () => {
      willChange();
      t.color = [1, 3, 5].map((k) => parseInt(color.value.slice(k, k + 2), 16) / 255);
      changed();
    });
    box.append(group("Light", el("label", { class: "field" }, el("span", {}, "Colour"), color),
      field("Intensity", t, "intensity", { orig: o("intensity") }), field("Range (m)", t, "range", { orig: o("range") }),
      field("Flicker", t, "flicker", { type: "checkbox", orig: o("flicker") })));
    box.append(transformFields(t, orig));
  } else if (t.kind === "shake") {
    box.append(group("Camera shake", field("Intensity", t, "intensity", { orig: o("intensity") }), field("Range (m)", t, "range", { orig: o("range") }),
      field("Delay (s)", t, "delay", { orig: o("delay") })));
  } else if (t.kind === "anim") {
    box.append(group("Caster animation", field("Animation state", t, "name", { type: "text", orig: o("name") }),
      field("Speed", t, "speed", { orig: o("speed") }), field("Fade in", t, "fadein", { orig: o("fadein") }), field("Fade out", t, "fadeout", { orig: o("fadeout") })));
  }
  const rest = Object.keys(t).filter((k) => !["kind", "start", "end", "graphics", "joint", "at", "color", "intensity", "range", "flicker", "delay", "name", "speed", "fadein", "fadeout"].includes(k));
  for (const k of rest) box.append(jsonBox(k, t, k, o(k)));
  box.append(el("div", { class: "row", style: "margin-top:14px" },
    el("button", { onclick: () => { willChange(); seq.tracks.splice(i + 1, 0, clone(t)); S.track = i + 1; changed(); } }, "Duplicate"),
    el("button", { onclick: () => { willChange(); seq.tracks.splice(i, 1); S.track = null; changed(); } }, "Delete track")));
}

// -- effect models -------------------------------------------------------------
function nodeKind(n) {
  if (n.extras.dsor_emitter) return "emitter";
  if (n.materials.length) return "surface";
  return "node";
}
function renderEffect() {
  const d = S.doc;
  $("#fx-title").innerHTML = "";
  $("#fx-title").append(d.path, edited("fx", d.path) ? el("span", { class: "dot", style: "margin-left:8px" }) : "");
  $("#fx-anims").textContent = d.animations.map((a) => `${a.name} ${a.length.toFixed(2)} s`).join(" · ");
  const ul = $("#fx-nodes");
  ul.innerHTML = "";
  const order = { emitter: 0, surface: 1, node: 2 };
  const nodes = [...d.nodes].filter((n) => nodeKind(n) !== "node" || Object.keys(n.extras).length)
    .sort((a, b) => order[nodeKind(a)] - order[nodeKind(b)]);
  for (const n of nodes) {
    const m = n.materials[0];
    const hidden = m && m.state === "Hidden" && !n.extras.dsor_emitter;
    const changedNode = !same(n.extras, n.original) || n.materials.some((x) => x.texture !== x.texture_orig);
    ul.append(el("li", { class: S.node === n.name ? "sel" : "", onclick: () => { S.node = n.name; renderEffect(); inspectNode(); } },
      m && m.texture ? el("img", { src: "/asset/" + m.texture, loading: "lazy" }) : el("div", { class: "thumb" }),
      el("div", { class: "t" },
        el("div", {}, changedNode ? el("span", { class: "dot" }) : null, " ", n.name),
        el("small", {}, nodeKind(n), m ? ` · ${m.shader.replace("shd:", "")}${m.state ? " · " + m.state : ""}` : "",
          hidden ? " (drawn by the client)" : ""))));
  }
}

// An emitter's envelope: [v0 v1 v2 v3, key1, key2, sine freq, sine amp, sine mode].
function envelope(t) { return t; }
function sample(e, t) {
  t = Math.min(1, Math.max(0, t));
  const k0 = Math.min(1, Math.max(0, e[4])), k1 = Math.min(1, Math.max(0, e[5]));
  const lerp = (a, b, f) => a + (b - a) * Math.min(1, Math.max(0, f));
  let v = t < k0 ? lerp(e[0], e[1], k0 > 0 ? t / k0 : 1) : t < k1 ? lerp(e[1], e[2], (t - k0) / Math.max(1e-6, k1 - k0)) : lerp(e[2], e[3], (t - k1) / Math.max(1e-6, 1 - k1));
  if (e[7]) { const ph = t * e[6] * Math.PI * 2; v += e[7] * (e[8] === 0 ? Math.sin(ph) : Math.cos(ph)); }
  return v;
}
function curveEditor(label, owner, key, orig, xLabel) {
  const e = owner[key];
  const wrap = el("div", { class: "curve" });
  const head = el("div", { class: "ct" + (orig && !same(orig, e) ? " changed" : "") }, el("b", {}, label), el("span", {}, xLabel));
  const canvas = el("canvas", { width: 340, height: 90 });
  const nums = el("div", { class: "nums" });
  const names = ["start", "key 1", "key 2", "end", "key 1 at", "key 2 at", "wave freq", "wave amp"];
  const inputs = names.map((n, i) => {
    const inp = el("input", { type: "number", step: "any", value: +e[i].toFixed(4) });
    inp.addEventListener("change", () => {
      const v = parseFloat(inp.value);
      if (!isFinite(v)) return;
      willChange(); e[i] = v; changed({ rerender: false }); draw();
    });
    nums.append(el("label", {}, n, inp));
    return inp;
  });
  wrap.append(head, canvas, nums);
  let range = [0, 1];
  function points() {
    return [[0, e[0]], [e[4], e[1]], [e[5], e[2]], [1, e[3]]];
  }
  function draw() {
    const c = canvas.getContext("2d"), W = canvas.width, H = canvas.height;
    let lo = Math.min(0, ...e.slice(0, 4)) - Math.abs(e[7]), hi = Math.max(...e.slice(0, 4)) + Math.abs(e[7]);
    if (hi - lo < 1e-6) hi = lo + 1;
    const pad = (hi - lo) * 0.12; lo -= pad; hi += pad;
    range = [lo, hi];
    const X = (t) => 6 + t * (W - 12), Y = (v) => H - 6 - ((v - lo) / (hi - lo)) * (H - 12);
    c.clearRect(0, 0, W, H);
    c.strokeStyle = "#2a2f3a"; c.lineWidth = 1;
    c.beginPath(); c.moveTo(0, Y(0)); c.lineTo(W, Y(0)); c.stroke();
    if (orig) {
      c.strokeStyle = "#555c6b"; c.setLineDash([3, 3]); c.beginPath();
      for (let i = 0; i <= 80; i++) { const t = i / 80; i ? c.lineTo(X(t), Y(sample(orig, t))) : c.moveTo(X(t), Y(sample(orig, t))); }
      c.stroke(); c.setLineDash([]);
    }
    c.strokeStyle = "#e0a040"; c.lineWidth = 2; c.beginPath();
    for (let i = 0; i <= 120; i++) { const t = i / 120; i ? c.lineTo(X(t), Y(sample(e, t))) : c.moveTo(X(t), Y(sample(e, t))); }
    c.stroke();
    c.fillStyle = "#5aa9e6";
    for (const [t, v] of points()) { c.beginPath(); c.arc(X(t), Y(v), 4.5, 0, 7); c.fill(); }
    c.fillStyle = "#9097a6"; c.font = "10px system-ui";
    c.fillText(hi.toPrecision(3), 2, 10); c.fillText(lo.toPrecision(3), 2, H - 2);
    inputs.forEach((inp, i) => { if (document.activeElement !== inp) inp.value = +e[i].toFixed(4); });
  }
  canvas.addEventListener("pointerdown", (ev) => {
    const r = canvas.getBoundingClientRect();
    const sx = canvas.width / r.width, sy = canvas.height / r.height;
    const W = canvas.width, H = canvas.height;
    const [lo, hi] = range;
    const X = (t) => 6 + t * (W - 12), Y = (v) => H - 6 - ((v - lo) / (hi - lo)) * (H - 12);
    const px = (ev.clientX - r.left) * sx, py = (ev.clientY - r.top) * sy;
    let best = -1, bd = 14;
    points().forEach(([t, v], i) => { const d = Math.hypot(X(t) - px, Y(v) - py); if (d < bd) { bd = d; best = i; } });
    if (best < 0) return;
    willChange();
    canvas.setPointerCapture(ev.pointerId);
    const move = (m) => {
      const qx = (m.clientX - r.left) * sx, qy = (m.clientY - r.top) * sy;
      const v = lo + ((H - 6 - qy) / (H - 12)) * (hi - lo);
      e[best] = +v.toPrecision(4);
      if (best === 1) e[4] = Math.min(e[5], Math.max(0, +((qx - 6) / (W - 12)).toFixed(3)));
      if (best === 2) e[5] = Math.max(e[4], Math.min(1, +((qx - 6) / (W - 12)).toFixed(3)));
      draw();
    };
    const up = () => { canvas.removeEventListener("pointermove", move); canvas.removeEventListener("pointerup", up); changed({ rerender: false }); };
    canvas.addEventListener("pointermove", move);
    canvas.addEventListener("pointerup", up);
  });
  draw();
  return wrap;
}

const ENVELOPES = {
  emission_frequency: ["Particles / second", "over the emission"],
  lifetime: ["Particle lifetime (s)", "over the emission"],
  spread_min: ["Spread min (°)", "over the emission"],
  spread_max: ["Spread max (°)", "over the emission"],
  start_velocity: ["Start speed", "over the emission"],
  rotation_velocity: ["Spin (rad / s)", "over a particle's life"],
  particle_size: ["Size", "over a particle's life"],
  particle_mass: ["Mass (gravity, wind)", "over a particle's life"],
  time_manipulator: ["Time scale", "over a particle's life"],
  velocity_factor: ["Speed factor", "over a particle's life"],
  air_resistance: ["Air resistance", "over a particle's life"],
  color_red: ["Red", "over a particle's life"],
  color_green: ["Green", "over a particle's life"],
  color_blue: ["Blue", "over a particle's life"],
  color_alpha: ["Opacity", "over a particle's life"],
};
const EMITTER_SCALARS = {
  emission_duration: "Emission duration (s)", start_delay: "Start delay (s)", looping: "Loops (1/0)", precalc_time: "Pre-simulated (s)",
  gravity: "Gravity", stretch: "Stretch along speed", texture_tile: "Texture frames", billboard: "Faces camera (1/0)",
  render_oldest_first: "Oldest first (1/0)", start_rotation_min: "Start rotation min", start_rotation_max: "Start rotation max",
  randomize_rotation: "Random rotation (1/0)", velocity_randomize: "Speed randomness", rotation_randomize: "Spin randomness",
  size_randomize: "Size randomness", activity_distance: "Active within (m)", local_space: "Local space (1/0)", additive: "Additive",
};

function inspectNode() {
  const n = S.doc && S.doc.nodes.find((x) => x.name === S.node);
  if (!n) return inspectorEmpty("Click an emitter or a surface.");
  $("#insp-title").textContent = n.name;
  const box = $("#insp");
  box.innerHTML = "";
  const ex = n.extras, or = n.original || {};
  for (const m of n.materials) box.append(materialGroup(m));
  if (ex.dsor_emitter) {
    const em = ex.dsor_emitter, oe = or.dsor_emitter || {};
    const scal = Object.keys(em).filter((k) => !Array.isArray(em[k]));
    box.append(group("Emitter", ...scal.map((k) => field(EMITTER_SCALARS[k] || k, em, k,
      { type: typeof em[k] === "boolean" ? "checkbox" : "number", orig: oe[k] }))));
    const curves = Object.keys(em).filter((k) => Array.isArray(em[k]) && em[k].length === 9);
    box.append(group("Curves (drag the points; dashed: the game's)",
      ...curves.map((k) => curveEditor((ENVELOPES[k] || [k])[0], em, k, oe[k], (ENVELOPES[k] || [, ""])[1]))));
  }
  if (ex.dsor_shader) {
    const sh = ex.dsor_shader, os = or.dsor_shader || {};
    box.append(group("Shader values", ...Object.keys(sh).map((k) => field(k, sh, k, { orig: os[k] }))));
  }
  if (ex.dsor_vector) {
    const vec = ex.dsor_vector, ov = or.dsor_vector || {};
    for (const k of Object.keys(vec)) {
      box.append(group(k + (k === "Velocity" ? " (UV scroll / s)" : ""), ...vec[k].map((_, i) => field("xyzw"[i], vec[k], i, { orig: ov[k] ? ov[k][i] : undefined }))));
    }
  }
  if (Array.isArray(ex.dsor_anim)) {
    ex.dsor_anim.forEach((a, ai) => box.append(animGroup(a, (or.dsor_anim || [])[ai])));
  }
  const known = ["dsor_emitter", "dsor_shader", "dsor_vector", "dsor_anim"];
  for (const k of Object.keys(ex).filter((k) => !known.includes(k))) box.append(jsonBox(k, ex, k, or[k]));
}

function materialGroup(m) {
  const g = group(`Material · ${m.shader.replace("shd:", "")}${m.state ? " · " + m.state : ""}`);
  if (!m.texture) { g.append(el("p", { class: "hint" }, "No texture.")); return g; }
  const filter = el("input", { type: "search", placeholder: "Filter textures…", style: "width:100%;margin:4px 0" });
  const grid = el("div", { class: "texgrid" });
  const fill = () => {
    grid.innerHTML = "";
    const q = filter.value.toLowerCase();
    const list = S.catalog.textures.filter((t) => !q || t.toLowerCase().includes(q));
    const top = list.includes(m.texture) ? [m.texture, ...list.filter((t) => t !== m.texture)] : list;
    for (const t of top.slice(0, 240)) {
      grid.append(el("img", { src: "/asset/" + t, loading: "lazy", title: t.split("/").pop(), class: t === m.texture ? "sel" : "",
        onclick: () => { willChange(); m.texture = t; changed(); } }));
    }
  };
  filter.addEventListener("input", fill);
  fill();
  g.append(el("div", { class: "field" }, el("span", { class: m.texture !== m.texture_orig ? "changed" : "" }, "Texture"),
    el("span", { class: "mono", title: m.texture }, m.texture.split("/").pop())));
  if (m.texture !== m.texture_orig) g.append(el("button", { onclick: () => { willChange(); m.texture = m.texture_orig; changed(); } }, "Back to the game's texture"));
  g.append(filter, grid);
  return g;
}

// An animator: {var, loop, keys: [[seconds, value], ...]}.
function animGroup(a, oa) {
  const g = group(`Animated ${a.var} (${a.loop})`);
  const canvas = el("canvas", { width: 340, height: 70, style: "width:100%;background:#121419;border-radius:4px" });
  const draw = () => {
    const c = canvas.getContext("2d"), W = canvas.width, H = canvas.height;
    const ks = a.keys, all = ks.concat(oa ? oa.keys : []);
    const t1 = Math.max(0.01, ...all.map((k) => k[0])), lo = Math.min(0, ...all.map((k) => k[1])), hi = Math.max(1e-3, ...all.map((k) => k[1]));
    const X = (t) => 6 + (t / t1) * (W - 12), Y = (v) => H - 6 - ((v - lo) / (hi - lo || 1)) * (H - 12);
    c.clearRect(0, 0, W, H);
    const line = (keys, color, dash) => {
      c.strokeStyle = color; c.setLineDash(dash); c.lineWidth = 2; c.beginPath();
      keys.forEach(([t, v], i) => (i ? c.lineTo(X(t), Y(v)) : c.moveTo(X(t), Y(v)))); c.stroke(); c.setLineDash([]);
    };
    if (oa) line(oa.keys, "#555c6b", [3, 3]);
    line(ks, "#e0a040", []);
    c.fillStyle = "#9097a6"; c.font = "10px system-ui"; c.fillText(`${t1.toFixed(2)} s`, W - 40, H - 2); c.fillText(hi.toPrecision(3), 2, 10);
  };
  const table = el("table", { class: "keys" });
  const rows = () => {
    table.innerHTML = "";
    table.append(el("tr", {}, el("td", { class: "hint" }, "time (s)"), el("td", { class: "hint" }, "value"), el("td")));
    a.keys.forEach((k, i) => {
      const cell = (j) => {
        const inp = el("input", { type: "number", step: "any", value: k[j] });
        inp.addEventListener("change", () => { const v = parseFloat(inp.value); if (!isFinite(v)) return; willChange(); k[j] = v; changed({ rerender: false }); draw(); });
        return el("td", {}, inp);
      };
      table.append(el("tr", {}, cell(0), cell(1), el("td", {}, el("button", { title: "Remove key",
        onclick: () => { willChange(); a.keys.splice(i, 1); changed(); } }, "×"))));
    });
  };
  rows();
  draw();
  g.append(canvas, table, el("button", { onclick: () => {
    willChange();
    const last = a.keys[a.keys.length - 1] || [0, 0];
    a.keys.push([+(last[0] + 0.2).toFixed(3), last[1]]);
    changed();
  } }, "+ key"));
  return g;
}

// -- playhead (approximate: from the last Play / edit) -------------------------
function tick() {
  const head = $("#playhead");
  if (head && S.doc && S.doc.kind === "seq") {
    const every = (+$("#every").value || 3) * 1000;
    const total = +$("#timeline").dataset.total || 1;
    const f = (((performance.now() - S.playedAt) % every) / 1000) * FPS;
    const lane = $("#timeline .lane");
    if (lane && f <= total) {
      head.hidden = false;
      const r = lane.getBoundingClientRect(), tr = $("#timeline").getBoundingClientRect();
      head.style.left = `${r.left - tr.left + (f / total) * r.width}px`;
    } else head.hidden = true;
  }
  requestAnimationFrame(tick);
}

// -- wiring --------------------------------------------------------------------
function setTab() {
  document.querySelectorAll(".tabs button").forEach((b) => b.classList.toggle("active", b.dataset.tab === S.tab));
  renderList();
}
document.querySelectorAll(".tabs button").forEach((b) => b.addEventListener("click", () => { S.tab = b.dataset.tab; setTab(); }));
$("#search").addEventListener("input", renderList);
$("#filter-class").addEventListener("change", renderList);
$("#only-edited").addEventListener("change", renderList);
$("#play").addEventListener("click", play);
$("#undo").addEventListener("click", undo);
$("#redo").addEventListener("click", redo);
$("#revert").addEventListener("click", revert);
$("#launch").addEventListener("click", async () => {
  try {
    await api("/api/launch", { class: +$("#class").value });
    setStatus("preview starting…", "ok");
    if (S.sel) setTimeout(play, 500);
  } catch (e) { setStatus(e.message, "bad"); }
});
$("#seq-length").addEventListener("change", () => { willChange(); S.doc.seq.length = Math.max(1, parseInt($("#seq-length").value) || 1); changed(); });
$("#seq-repeat").addEventListener("change", () => { willChange(); S.doc.seq.repeat = $("#seq-repeat").checked; changed(); });
$("#add-fx").addEventListener("click", () => addTrack("fx"));
$("#add-light").addEventListener("click", () => addTrack("light"));
$("#add-shake").addEventListener("click", () => addTrack("shake"));
document.addEventListener("keydown", (e) => {
  if (e.target.matches("input, textarea, select")) return;
  if ((e.ctrlKey || e.metaKey) && e.key === "z") { e.preventDefault(); e.shiftKey ? redo() : undo(); }
  if ((e.ctrlKey || e.metaKey) && e.key === "y") { e.preventDefault(); redo(); }
  if (e.key === " ") { e.preventDefault(); play(); }
});
window.addEventListener("beforeunload", flushSave);

loadCatalog().then(() => {
  const dl = el("datalist", { id: "effects-list" }, S.catalog.effects.map((e) => el("option", { value: e })));
  document.body.append(dl);
}).catch((e) => setStatus("cannot read the assets: " + e.message, "bad"));
setInterval(pollStatus, 1000);
requestAnimationFrame(tick);
