//! The commit the client is built from, for the bug dump (crate::diagnostics::dump).
//!
//! Read from `.git` directly (no git executable needed), and the build reruns
//! only when the checked-out commit changes: watching `.git/index`, which every
//! `git status` (and GitHub Desktop, continuously) rewrites, recompiled the
//! whole client on nearly every launch.
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let git = Path::new("../.git");
    // Only files that exist may be watched: cargo treats a missing one as
    // always changed (a rebuild on every launch again).
    let watch = |path: &Path| {
        if path.is_file() { println!("cargo:rerun-if-changed={}", path.display()); }
    };
    let commit = (|| {
        let head = git.join("HEAD");
        watch(&head);
        let head = std::fs::read_to_string(head).ok()?;
        let head = head.trim();
        let Some(reference) = head.strip_prefix("ref: ") else { return Some(head.to_owned()) };
        // The branch's commit: its loose ref, else the packed one.
        let loose = git.join(reference);
        let packed = git.join("packed-refs");
        watch(&loose);
        watch(&packed);
        std::fs::read_to_string(&loose).ok().or_else(|| {
            std::fs::read_to_string(&packed).ok()?.lines()
                .find(|line| line.ends_with(reference))
                .and_then(|line| line.split_whitespace().next().map(str::to_owned))
        })
    })()
    .map(|hash| hash.trim().chars().take(7).collect::<String>())
    .filter(|hash| !hash.is_empty())
    .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=DSOR_GIT_COMMIT={commit}");
}
