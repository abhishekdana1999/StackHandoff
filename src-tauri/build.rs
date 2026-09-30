use std::env;
use std::path::PathBuf;

fn main() {
    // Tell cargo to rerun if migrations change
    println!("cargo:rerun-if-changed=migrations/");
    
    // Set the database URL for SQLx compile-time checking
    let db_path = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("stackhandoff")
        .join("stackhandoff.db");
    
    let db_url = format!("sqlite://{}", db_path.display());
    env::set_var("DATABASE_URL", &db_url);
    
    // Ensure migrations directory exists
    let migrations_dir = PathBuf::from("migrations");
    if !migrations_dir.exists() {
        std::fs::create_dir_all(&migrations_dir).ok();
    }
    
    tauri_build::build()
}