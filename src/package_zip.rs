use anyhow::{Context, Result};
use std::fs::File;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

pub fn zip_directory(src_dir: &Path, out_zip: &Path) -> Result<u64> {
    std::fs::create_dir_all(
        out_zip
            .parent()
            .unwrap_or_else(|| Path::new(".")),
    )
    .context("create artifact dir")?;

    let file = File::create(out_zip).context("create zip")?;
    let mut zip = ZipWriter::new(file);
    let src_dir = src_dir.canonicalize().context("canonicalize src")?;
    let options = SimpleFileOptions::default();

    for entry in WalkDir::new(&src_dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let rel = path.strip_prefix(&src_dir).context("strip prefix")?;
        let name = rel.to_string_lossy().replace('\\', "/");
        if name.is_empty() {
            continue;
        }
        let mut f = std::fs::File::open(path).context("open file")?;
        zip.start_file(&name, options)
            .context("zip start_file")?;
        std::io::copy(&mut f, &mut zip).context("zip copy")?;
    }

    zip.finish().context("zip finish")?;
    let len = std::fs::metadata(out_zip)?.len();
    Ok(len)
}

pub fn artifact_paths(data_root: &Path, project_id: &str, filename: &str) -> PathBuf {
    data_root
        .join("artifacts")
        .join(project_id)
        .join(filename)
}
