fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    // Darwin's Rust std link line includes iconv, but headroom uses none of
    // its symbols or initializers. Keep only dylibs the final binary uses.
    println!("cargo::rustc-link-arg-bin=headroom=-Wl,-dead_strip_dylibs");
}
