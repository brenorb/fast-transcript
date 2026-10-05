use anyhow::{bail, Context, Result};
use directories::ProjectDirs;
use flate2::read::GzDecoder;
use reqwest::blocking::Client;
use std::env;
use std::fs;
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tar::Archive;

#[derive(Debug)]
pub(crate) struct ModelConfig {
    pub(crate) directory: PathBuf,
    pub(crate) package: PathBuf,
    pub(crate) url: String,
}

impl ModelConfig {
    pub(crate) fn resolve(
        url: Option<String>,
        directory: Option<PathBuf>,
        package: Option<PathBuf>,
    ) -> Self {
        let url = url.unwrap_or_else(default_model_url);
        let (default_directory, default_package) = if url == crate::DEFAULT_MODEL_URL {
            (default_model_dir(), default_model_package())
        } else {
            model_cache_paths(&url)
        };

        Self {
            directory: directory.unwrap_or(default_directory),
            package: package.unwrap_or(default_package),
            url,
        }
    }
}

fn default_app_data_dir() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("", "", "fast-transcript") {
        return project_dirs.data_local_dir().to_path_buf();
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".local").join("share").join("fast-transcript"))
        .unwrap_or_else(|| PathBuf::from(crate::DEFAULT_DATA_DIR_FALLBACK))
}

fn default_app_cache_dir() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("", "", "fast-transcript") {
        return project_dirs.cache_dir().to_path_buf();
    }

    env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".cache").join("fast-transcript"))
        .unwrap_or_else(|| PathBuf::from(crate::DEFAULT_CACHE_DIR_FALLBACK))
}

pub(crate) fn default_model_dir() -> PathBuf {
    env::var_os("FSCRIPT_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            default_app_data_dir()
                .join(crate::DEFAULT_MODEL_SUBDIR)
                .join(crate::DEFAULT_MODEL_BASENAME)
        })
}

pub(crate) fn default_model_package() -> PathBuf {
    env::var_os("FSCRIPT_MODEL_PACKAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|| default_app_cache_dir().join(crate::DEFAULT_MODEL_PACKAGE_NAME))
}

fn model_cache_paths(model_url: &str) -> (PathBuf, PathBuf) {
    let location = model_url.split(['?', '#']).next().unwrap_or(model_url);
    let filename = location.rsplit('/').next().unwrap_or_default();
    let stem = filename
        .strip_suffix(".tar.gz")
        .or_else(|| filename.strip_suffix(".tgz"))
        .unwrap_or(filename);
    let slug = stem
        .chars()
        .filter_map(|character| {
            if character.is_ascii_alphanumeric() {
                Some(character.to_ascii_lowercase())
            } else if character == '-' || character == '_' {
                Some('-')
            } else {
                None
            }
        })
        .take(48)
        .collect::<String>();
    let slug = if slug.is_empty() { "model" } else { &slug };
    let hash = model_url
        .bytes()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
        });
    let model_id = format!("{slug}-{hash:016x}");

    (
        default_app_data_dir()
            .join(crate::DEFAULT_MODEL_SUBDIR)
            .join(&model_id),
        default_app_cache_dir().join(format!("{model_id}.tar.gz")),
    )
}

fn default_model_url() -> String {
    env::var("FSCRIPT_MODEL_URL").unwrap_or_else(|_| crate::DEFAULT_MODEL_URL.to_string())
}

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

fn install_required_model_files(from_dir: &Path, into_dir: &Path) -> Result<()> {
    fs::create_dir_all(into_dir)
        .with_context(|| format!("failed to create {}", into_dir.display()))?;

    for file_name in crate::REQUIRED_MODEL_FILES {
        fs::copy(from_dir.join(file_name), into_dir.join(file_name)).with_context(|| {
            format!(
                "failed to install {} into {}",
                file_name,
                into_dir.display()
            )
        })?;
    }

    Ok(())
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
    if extracted_default_dir != model_dir && has_required_model_files(&extracted_default_dir) {
        if model_dir.exists() {
            install_required_model_files(&extracted_default_dir, model_dir)?;
            fs::remove_dir_all(&extracted_default_dir).with_context(|| {
                format!(
                    "failed to clean extracted model dir {}",
                    extracted_default_dir.display()
                )
            })?;
        } else {
            fs::rename(&extracted_default_dir, model_dir).with_context(|| {
                format!(
                    "failed to move extracted model from {} to {}",
                    extracted_default_dir.display(),
                    model_dir.display()
                )
            })?;
        }
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
    use super::{ensure_model_dir, remove_appledouble_files, ModelConfig};
    use flate2::{write::GzEncoder, Compression};
    use std::fs::{self, File};
    use tar::Builder;
    use tempfile::tempdir;

    #[test]
    fn model_config_keeps_explicit_storage_overrides_for_selected_bundle() {
        let dir = tempdir().unwrap();
        let model_directory = dir.path().join("model");
        let package = dir.path().join("cache/model.tar.gz");
        let url = "https://models.example.test/custom.tar.gz".to_string();

        let config = ModelConfig::resolve(
            Some(url.clone()),
            Some(model_directory.clone()),
            Some(package.clone()),
        );

        assert_eq!(config.url, url);
        assert_eq!(config.directory, model_directory);
        assert_eq!(config.package, package);
    }

    #[test]
    fn model_config_assigns_different_cache_paths_to_different_bundle_urls() {
        let first = ModelConfig::resolve(
            Some("https://models.example.test/parakeet.tar.gz?revision=1".to_string()),
            None,
            None,
        );
        let second = ModelConfig::resolve(
            Some("https://models.example.test/parakeet.tar.gz?revision=2".to_string()),
            None,
            None,
        );

        assert_ne!(first.directory, second.directory);
        assert_ne!(first.package, second.package);
    }

    fn write_model_package(package_path: &std::path::Path, marker: &str) {
        if let Some(parent) = package_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }

        let archive = File::create(package_path).unwrap();
        let encoder = GzEncoder::new(archive, Compression::default());
        let mut builder = Builder::new(encoder);

        for file_name in crate::REQUIRED_MODEL_FILES {
            let contents = format!("{marker}:{file_name}");
            let archive_path = format!("{}/{}", crate::DEFAULT_MODEL_BASENAME, file_name);
            let mut header = tar::Header::new_gnu();
            header.set_size(contents.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, archive_path, contents.as_bytes())
                .unwrap();
        }

        builder.finish().unwrap();
    }

    #[test]
    fn remove_appledouble_files_cleans_resource_forks_only() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("nested");
        fs::create_dir_all(&nested).unwrap();
        let keep = nested.join("encoder-model.int8.onnx");
        let remove = nested.join("._encoder-model.int8.onnx");
        fs::write(&keep, "ok").unwrap();
        fs::write(&remove, "junk").unwrap();

        remove_appledouble_files(dir.path()).unwrap();

        assert!(keep.exists());
        assert!(!remove.exists());
    }

    #[test]
    fn ensure_model_dir_repairs_existing_incomplete_custom_directory() {
        let dir = tempdir().unwrap();
        let model_dir = dir.path().join("custom-model");
        let package_path = dir.path().join("cache").join("model.tar.gz");

        fs::create_dir_all(&model_dir).unwrap();
        fs::write(
            model_dir.join(crate::REQUIRED_MODEL_FILES[0]),
            "stale:encoder-model.int8.onnx",
        )
        .unwrap();
        write_model_package(&package_path, "fresh");

        ensure_model_dir(
            &model_dir,
            &package_path,
            "https://example.test/model.tar.gz",
        )
        .unwrap();

        for file_name in crate::REQUIRED_MODEL_FILES {
            let contents = fs::read_to_string(model_dir.join(file_name)).unwrap();
            assert_eq!(contents, format!("fresh:{file_name}"));
        }
        assert!(!dir.path().join(crate::DEFAULT_MODEL_BASENAME).exists());
    }
}
