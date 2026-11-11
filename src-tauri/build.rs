fn main() {
    // The window and program icons are compiled in; rebuild when they change (Cargo does not notice otherwise).
    println!("cargo:rerun-if-changed=icons");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    #[cfg(feature = "desktop")]
    tauri_build::build()
}
