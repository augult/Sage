fn main() {
    // The UI is embedded when this crate compiles, so a change to it has to trigger a rebuild.
    println!("cargo:rerun-if-changed=../ui");
    tauri_build::build()
}
