fn main() {
    // The icons are built into the exe here: a new icon has to run this again.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
