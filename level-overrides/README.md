# Authored level overrides

The level editor saves one versioned `<map>.json` file here. These files are the
source of truth for edits and can be committed independently of game assets.
Do not put converted models, original DB4 databases, or metadata snapshots here.

An operation identifies a geometry placement or DB4 record and records an add,
field update, or deletion. Updates retain their original record so rebuilds can
detect overlapping upstream changes. Database identities use primary keys;
geometry identities use model references and original placement content, never
the generated model-table index. Unresolved operations stay in the override.

See [the level editor guide](../docs/level-editor.md) for editing, offline use,
conflict resolution, and rebuilding client data and derived DB4 files.
