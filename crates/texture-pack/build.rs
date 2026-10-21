fn main() {
    // intel_tex_2's ISPC kernels call into the C++ runtime; rust-lld (the CI's
    // linker) does not pull libstdc++ in by itself: __gxx_personality_v0.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux") {
        println!("cargo:rustc-link-lib=stdc++");
    }
}
