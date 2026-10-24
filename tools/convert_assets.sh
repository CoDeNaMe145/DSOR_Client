#!/bin/sh
# Convert an unpacked 2018 Drakensang Online client into the assets this client
# loads (glTF models + PNG textures + maps + prepared audio), using DSO_Godot.
#
#   tools/convert_assets.sh <export_win32> <out_dir> [<DSO_Godot>]
#
#   <export_win32>  the client's unpacked data tree (models/, meshes/, maps/,
#                   textures/, anims/, ...). If you only have the packed client,
#                   run DSO_Godot's unpack.py first.
#   <out_dir>       where the converted assets go (~10 GB). Keep it OUTSIDE any
#                   git-tracked path: the game data is proprietary.
#   <DSO_Godot>     a checkout of https://github.com/DSO-Reconstruction/DSO_Godot
#                   (cloned into <out_dir>/.DSO_Godot when omitted).
#
# Afterwards point the client at it:  ln -sfn <out_dir> <workspace>/assets
#
# Then puts back what glTF cannot carry (step 5, this repository's tools), in
# this order: Nebula render states, emitters, animators / render states / cull
# modes, cube maps and second layers, decal tiling, character colours, the
# client's tables (skills, NPCs, exits, ambience) and the interface windows;
# last, the models' textures as KTX2 (step 6). The navigation meshes need the
# server repository: DSOR_SERVER=<DSOR_Server> (or EXPERIMENTAL) enables them.
#
# Needs python3 with Pillow and numpy, a C++ compiler and curl (the .crn
# transcoder), git (only when cloning DSO_Godot), and vgmstream-cli for audio
# (on PATH or DSOR_VGMSTREAM=/path/to/vgmstream-cli). Logs go to <out_dir>/logs/.
set -eu

if [ $# -lt 2 ]; then
    sed -n '2,18p' "$0"; exit 2
fi
root=$(cd "$1" && pwd)
mkdir -p "$2"
out=$(cd "$2" && pwd)
godot=${3:-$out/.DSO_Godot}
# Resolved before the cd into DSO_Godot below: a relative $0 broke step 5.
tools=$(cd "$(dirname "$0")" && pwd)
logs=$out/logs
mkdir -p "$logs"

[ -d "$root/models" ] && [ -d "$root/maps" ] || {
    echo "error: $root does not look like an unpacked export_win32 tree" >&2; exit 1; }

if [ ! -f "$godot/export_model.py" ]; then
    git clone --depth 1 https://github.com/DSO-Reconstruction/DSO_Godot "$godot"
fi
python3 -c "import PIL, numpy" || {
    echo "error: python3 needs Pillow and numpy (pip install -r $godot/requirements.txt)" >&2; exit 1; }

# Prepare audio first so a missing decoder fails before the model conversion.
echo "[convert] audio (log: $logs/export_audio.log)"
python3 "$tools/export_audio.py" "$root" "$out" > "$logs/export_audio.log" 2>&1 || {
    cat "$logs/export_audio.log" >&2; exit 1; }
tail -n 3 "$logs/export_audio.log"

# 1. The Crunch (.crn) texture transcoder; without it ~2,600 textures are skipped.
[ -x "$godot/build/crn2dds" ] || sh "$godot/tools/build_crn2dds.sh"

cd "$godot"

# 2. Every model -> <out>/<models subpath>.glb, textures -> <out>/textures/**.png.
#    The two player-character skeletons are split separately (step 3).
echo "[convert] models (log: $logs/export_models.log)"
python3 batch_export.py --root "$root" --out "$out" \
    --skip uniskel uniskel_dwarf --report "$logs/export_report.json" \
    > "$logs/export_models.log" 2>&1
tail -n 12 "$logs/export_models.log"

# 3. Player characters: skeleton + clips, one .glb per part, outfits.json.
for c in uniskel uniskel_dwarf; do
    if [ -f "$root/models/characters/$c.n3" ]; then
        echo "[convert] character $c (log: $logs/split_$c.log)"
        python3 split_character.py "$root/models/characters/$c.n3" \
            --root "$root" --export-root "$out" > "$logs/split_$c.log" 2>&1
        tail -n 4 "$logs/split_$c.log"
    fi
done

# 4. Maps -> <out>/maps/<map>.map.json placement manifests.
echo "[convert] maps (log: $logs/export_maps.log)"
python3 export_maps.py --root "$root" --out "$out/maps" > "$logs/export_maps.log" 2>&1
tail -n 6 "$logs/export_maps.log"
sh "$tools/level-tool.sh" events "$root/maps" "$out/maps"

# 5. What glTF cannot carry, and the client's own tables.
echo "[convert] Nebula states and tables (log: $logs/post.log)"
{
    # The skin fragments DSO_Godot leaves out (big skeletons: most bosses).
    python3 "$tools/restore_skin_fragments.py" "$root" "$out" "$godot"
    python3 "$tools/fix_materials.py" "$out"
    python3 "$tools/embed_emitters.py" "$out"
    python3 "$tools/embed_animators.py" "$root/models" "$out"
    python3 "$tools/character_render_states.py" "$out"
    python3 "$tools/embed_textures.py" "$root" "$out" "$godot"
    python3 "$tools/fix_materials.py" "$out" --n3 "$root/models"
    python3 "$tools/fix_materials.py" "$out" --character-colors
    # The dye masks of character and monster surfaces (equipment colours).
    python3 "$tools/embed_character_masks.py" "$root" "$out" "$godot"
    python3 "$tools/close_loops.py" "$out"
    python3 "$tools/fx_clip_starts.py" "$root" "$out" "$godot"
    python3 "$tools/restore_pivots.py" "$root/models" "$out" "$godot"
    python3 "$tools/character_tables.py" "$root" "$out"
    # Monsters (templates, animation sets, the hits on players), picking
    # shapes, combat texts, character statistics, the selection screen.
    python3 "$tools/export_monsters.py" "$root" "$out"
    python3 "$tools/export_picking.py" "$root" "$out"
    python3 "$tools/export_combat_text.py" "$root" "$out" "$godot"
    python3 "$tools/export_stats.py" "$root" "$out"
    python3 "$tools/export_character_selection.py" "$root/db/static.db4" "$out"
    # The systems windows' data (achievements, events, dailies, the album, recipes...).
    python3 "$tools/systems_catalog.py" "$root/db/static.db4" "$root" "$godot" "$out"
    python3 "$tools/export_skills.py" "$root" "$out"
    # Pets (after the monsters' anim sets and the sequences it checks).
    python3 "$tools/export_pets.py" "$root" "$out"
    python3 "$tools/export_npcs.py" "$root" "$out"
    python3 "$tools/export_exits.py" "$root" "$out"
    python3 "$tools/export_ambience.py" "$root" "$out"
    python3 "$tools/export_deco.py" "$root" "$out"
    python3 "$tools/export_audio_runtime.py" "$root" "$out"
    # Last: the VFX editor's edits (vfx-overrides/) over the fresh conversion.
    python3 "$tools/vfx_apply.py" "$out"
    python3 "$tools/export_ui.py" "$root" "$out" "$godot"
    # The player trade window, composed from exported windows; the debug menu's index.
    python3 "$tools/compose_trade_ui.py" "$out"
    python3 "$tools/export_debug.py" "$out"
    # The walkable ground (maps/<map>.nav.bin): player heights and collisions, and
    # where skills may land. It needs the server's PathEngine reader:
    # DSOR_SERVER=<DSOR_Server checkout> (or the legacy EXPERIMENTAL reader).
    if [ -n "${DSOR_SERVER:-}" ]; then python3 "$tools/export_navmesh.py" "$root" "$out" --server "$DSOR_SERVER" && python3 "$tools/quest_catalog.py" "$DSOR_SERVER/data/quests.xml" "$out" --database "$root/db/static.db4"
    elif [ -n "${EXPERIMENTAL:-}" ]; then python3 "$tools/export_navmesh.py" "$root" "$out"
    else echo "warning: no DSOR_SERVER: no navigation meshes (players walk through walls, skills land anywhere)" >&2; fi
} > "$logs/post.log" 2>&1
tail -n 14 "$logs/post.log"

# Authored edits live outside the generated tree. Recompose client sidecars and
# derived DB4 files from one effective map; conflicts must fail the conversion.
sh "$tools/level-tool.sh" apply --assets "$out" --source-root "$root"
sh "$tools/level-tool.sh" props --assets "$out" --source-root "$root" --static-db "$root/db/static.db4"

# 6. The models' textures as KTX2 beside their PNGs (the game's DXT blocks and
#    mips, or BC7): what a GPU that samples BC loads (crates/texture-pack).
echo "[convert] KTX2 textures (log: $logs/ktx2.log)"
cargo run --quiet --locked --release --manifest-path "$tools/../Cargo.toml" -p dsor-texture-pack --bin dsor-ktx2 -- \
    "$root" "$out" > "$logs/ktx2.log" 2>&1
tail -n 1 "$logs/ktx2.log"

du -sh "$out"
echo "[convert] done -> $out"
