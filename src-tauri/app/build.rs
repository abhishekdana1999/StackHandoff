// The default config path: a `tauri.conf.json` beside this `Cargo.toml`.
//
// An explicit `config_path` used to point one level up. That is what made
// `tauri dev` fail with "No package info in the config file": the Tauri CLI
// requires the config's directory to have a sibling `Cargo.toml` with a
// `[package]` section, and the directory above this one holds a virtual
// workspace manifest with no package of its own. The two disagreed about where
// the config lived -- the build script found it, the CLI did not -- so the app
// built but could not be run in development.
fn main() {
    tauri_build::build()
}
