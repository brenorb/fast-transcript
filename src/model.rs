use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use std::fs;
use std::fs::File;
use std::io;
use std::path::Path;
use std::time::Duration;
use tar::Archive;

pub(crate) fn remove_appledouble_files(root: &Path) -> Result<()> {
    if !root.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(root).with_context(|| format!("failed to read {}", root.display()))? {
        let entry = entry.with_context(|| format!("failed to inspect {}", root.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect {}", path.display()))?;

        if file_type.is_dir() {
            remove_appledouble_files(&path)?;
            continue;
        }

        let is_appledouble = path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("._"));
        if is_appledouble {
            fs::remove_file(&path)
                .with_context(|| format!("failed to remove {}", path.display()))?;
        }
    }

    Ok(())
}

fn has_required_model_files(model_dir: &Path) -> bool {
    crate::REQUIRED_MODEL_FILES
        .iter()
        .all(|file_name| model_dir.join(file_name).is_file())
}

fn download_model_package(model_url: &str, package_path: &Path) -> Result<()> {
    let parent = package_path
        .parent()
        .with_context(|| format!("package path {} has no parent", package_path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;

    eprintln!(
        "model missing; downloading {} to {}",
        model_url,
        package_path.display()
    );
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60 * 60))
        .build()
        .context("failed to build HTTP client for model download")?;
    let mut response = client
        .get(model_url)
        .send()
        .with_context(|| format!("failed to request {model_url}"))?
        .error_for_status()
        .with_context(|| format!("download failed for {model_url}"))?;

    let tmp_path = package_path.with_extension("tar.gz.partial");
    let mut file = File::create(&tmp_path)
        .with_context(|| format!("failed to create {}", tmp_path.display()))?;
    io::copy(&mut response, &mut file)
        .with_context(|| format!("failed to write {}", tmp_path.display()))?;
    fs::rename(&tmp_path, package_path).with_context(|| {
        format!(
            "failed to move downloaded model package into {}",
            package_path.display()
        )
    })?;
    Ok(())
}

fn extract_model_package(model_dir: &Path, package_path: &Path) -> Result<()> {
    let destination_root = model_dir
        .parent()
        .with_context(|| format!("model dir {} has no parent", model_dir.display()))?;
    fs::create_dir_all(destination_root)
        .with_context(|| format!("failed to create {}", destination_root.display()))?;

    eprintln!(
        "extracting {} into {}",
        package_path.display(),
        destination_root.display()
    );
    let archive_file = File::open(package_path)
        .with_context(|| format!("failed to open {}", package_path.display()))?;
    let decoder = GzDecoder::new(archive_file);
    let mut archive = Archive::new(decoder);
    archive
        .unpack(destination_root)
        .with_context(|| format!("failed to unpack {}", package_path.display()))?;
    remove_appledouble_files(destination_root)?;

    let extracted_default_dir = destination_root.join(crate::DEFAULT_MODEL_BASENAME);
    if extracted_default_dir != model_dir
        && has_required_model_files(&extracted_default_dir)
        && !model_dir.exists()
    {
        fs::rename(&extracted_default_dir, model_dir).with_context(|| {
            format!(
                "failed to move extracted model from {} to {}",
                extracted_default_dir.display(),
                model_dir.display()
            )
        })?;
    }
    Ok(())
}

pub(crate) fn ensure_model_dir(
    model_dir: &Path,
    package_path: &Path,
    model_url: &str,
) -> Result<()> {
    if has_required_model_files(model_dir) {
        return Ok(());
    }

    if !package_path.exists() {
        download_model_package(model_url, package_path)?;
    } else {
        eprintln!(
            "model missing; reusing cached package {}",
            package_path.display()
        );
    }

    extract_model_package(model_dir, package_path)?;
    if !has_required_model_files(model_dir) {
        bail!(
            "model directory {} is still incomplete after extraction",
            model_dir.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::remove_appledouble_files;
    use tempfile::tempdir;

    #[test]
    fn remove_appledouble_files_cleans_resource_forks_only() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        std::fs::create_dir_all(&nested).unwrap();
        let keep = nested.join("encoder-model.int8.onnx");
        let remove = nested.join("._encoder-model.int8.onnx");
        std::fs::write(&keep, "ok").unwrap();
        std::fs::write(&remove, "junk").unwrap();

        remove_appledouble_files(dir.path()).unwrap();

        assert!(keep.exists());
        assert!(!remove.exists());
    }
}
