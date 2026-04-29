use std::env;
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=frontend/package.json");
    println!("cargo:rerun-if-changed=frontend/vite.config.ts");
    println!("cargo:rerun-if-changed=frontend/index.html");
    println!("cargo:rerun-if-changed=frontend/src");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let frontend = Path::new(&manifest_dir).join("frontend");
    let skip = env::var("MINICI_SKIP_FRONTEND_BUILD").unwrap_or_default() == "1";

    let dist = frontend.join("dist");

    if skip {
        eprintln!("mini-ci: MINICI_SKIP_FRONTEND_BUILD=1 — skipping frontend build");
        write_placeholder_dist(&dist);
        return;
    }

    if !frontend.join("package.json").exists() {
        eprintln!("mini-ci: frontend/package.json missing — placeholder dist");
        write_placeholder_dist(&dist);
        return;
    }

    let status = Command::new("npm")
        .current_dir(&frontend)
        .args(["ci"])
        .status();

    if status.map(|s| !s.success()).unwrap_or(true) {
        let _ = Command::new("npm")
            .current_dir(&frontend)
            .args(["install"])
            .status()
            .expect("npm install failed");
    }

    let ok = Command::new("npm")
        .current_dir(&frontend)
        .args(["run", "build"])
        .status()
        .expect("npm run build failed")
        .success();

    assert!(ok, "frontend build failed");
}

fn write_placeholder_dist(dist: &std::path::Path) {
    std::fs::create_dir_all(dist).ok();
    std::fs::write(
        dist.join("index.html"),
        "<!DOCTYPE html><html><body><p>mini-ci UI not built</p></body></html>",
    )
    .expect("write placeholder dist");
}
