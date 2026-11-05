use crate::errors::{technical, Explain};
use crate::model_catalog::{find_model, ModelPreset, ModelRuntime};
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
    pub(crate) preset: Option<&'static ModelPreset>,
    pub(crate) device: String,
}

impl ModelConfig {
    pub(crate) fn select(
        selection: Option<String>,
        directory: Option<PathBuf>,
        package: Option<PathBuf>,
        device: String,
    ) -> Result<Self> {
        let mut config = match selection {
            None => Self::resolve(None, directory, package),
            Some(selection)
                if selection.starts_with("https://") || selection.starts_with("http://") =>
            {
                Self::resolve(Some(selection), directory, package)
            }
            Some(selection) => {
                let preset = find_model(&selection)
                    .with_context(|| crate::model_catalog::unknown_model_message(&selection))?;
                if preset.runtime == ModelRuntime::Onnx {
                    let mut config =
                        Self::resolve(Some(preset.url.to_string()), directory, package);
                    config.preset = Some(preset);
                    config
                } else {
                    if package.is_some() {
                        bail!("--model-package works only with the default model or --model-url; {} downloads its own files. Remove --model-package", preset.name);
                    }
                    Self {
                        directory: directory.unwrap_or_default(),
                        package: PathBuf::new(),
                        url: preset.url.to_string(),
                        preset: Some(preset),
                        device: "auto".to_string(),
                    }
                }
            }
        };
        let runtime = config
            .preset
            .map_or(ModelRuntime::Onnx, |model| model.runtime);
        let allowed = match runtime {
            ModelRuntime::Onnx => &["auto", "cpu"][..],
            ModelRuntime::Photon => &["auto", "cpu", "mps", "cuda"][..],
            ModelRuntime::Phonon => &["auto", "cpu", "mlx"][..],
        };
        if !allowed.contains(&device.as_str()) {
            bail!(
                "device {device:?} is not supported by the selected model; choose {}",
                allowed.join(", ")
            );
        }
        config.device = device;
        Ok(config)
    }

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
            preset: None,
            device: "auto".to_string(),
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

    let failed = || format!("could not install the speech model into {}", root.display());
    for entry in fs::read_dir(root).explain(failed, MODEL_DIR_HINT)? {
        let entry = entry.explain(failed, MODEL_DIR_HINT)?;
        let path = entry.path();
        let file_type = entry.file_type().explain(failed, MODEL_DIR_HINT)?;

        if file_type.is_dir() {
            remove_appledouble_files(&path)?;
            continue;
        }

        let is_appledouble = path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.starts_with("._"));
        if is_appledouble {
            fs::remove_file(&path).explain(failed, MODEL_DIR_HINT)?;
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
    let failed = || {
        format!(
            "could not install the speech model into {}",
            into_dir.display()
        )
    };
    fs::create_dir_all(into_dir).explain(failed, MODEL_DIR_HINT)?;

    for file_name in crate::REQUIRED_MODEL_FILES {
        fs::copy(from_dir.join(file_name), into_dir.join(file_name))
            .explain(failed, MODEL_DIR_HINT)?;
    }

    Ok(())
}

const MODEL_DIR_HINT: &str = "Check that the folder is writable and has free space, or set FSCRIPT_MODEL_DIR and FSCRIPT_MODEL_PACKAGE to another folder";
const NETWORK_HINT: &str = "Check your internet connection and try again";

/// The default bundle is "the speech model"; a custom link is named so it can be checked.
fn model_source(model_url: &str) -> String {
    if model_url == crate::DEFAULT_MODEL_URL {
        "the speech model".to_string()
    } else {
        format!("the model from {model_url}")
    }
}

fn is_storage_problem(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::StorageFull
            | io::ErrorKind::PermissionDenied
            | io::ErrorKind::ReadOnlyFilesystem
            | io::ErrorKind::QuotaExceeded
    )
}

fn download_model_package(model_url: &str, package_path: &Path) -> Result<()> {
    let source = model_source(model_url);
    let parent = package_path.parent().with_context(|| {
        format!(
            "cannot save {source} to {}; set FSCRIPT_MODEL_PACKAGE to a file path",
            package_path.display()
        )
    })?;
    let save_failed = || format!("could not save {source} to {}", parent.display());
    fs::create_dir_all(parent).explain(save_failed, MODEL_DIR_HINT)?;

    if model_url == crate::DEFAULT_MODEL_URL {
        eprintln!("downloading the speech model (first run only)...");
    } else {
        eprintln!("downloading the model from {model_url}...");
    }
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(60 * 60))
        .build()
        .map_err(technical)
        .explain(
            || format!("could not start downloading {source}"),
            NETWORK_HINT,
        )?;
    let response = client
        .get(model_url)
        .send()
        .map_err(technical)
        .explain(|| format!("could not download {source}"), NETWORK_HINT)?;
    let status = response.status();
    if !status.is_success() {
        if model_url == crate::DEFAULT_MODEL_URL {
            bail!("could not download the speech model (the server answered {status}); try again later");
        }
        bail!("could not download {source} (the server answered {status}); check the model link");
    }
    let mut response = response;

    let tmp_path = package_path.with_extension("tar.gz.partial");
    let mut file = File::create(&tmp_path).explain(save_failed, MODEL_DIR_HINT)?;
    if let Err(error) = io::copy(&mut response, &mut file) {
        drop(file);
        let _ = fs::remove_file(&tmp_path);
        if is_storage_problem(&error) {
            return Err(error).explain(save_failed, MODEL_DIR_HINT);
        }
        return Err(technical(error)).explain(
            || format!("the download of {source} was interrupted"),
            "Check your internet connection and run fscript again",
        );
    }
    fs::rename(&tmp_path, package_path).explain(save_failed, MODEL_DIR_HINT)?;
    Ok(())
}

fn extract_model_package(model_dir: &Path, package_path: &Path) -> Result<()> {
    let destination_root = model_dir.parent().with_context(|| {
        format!(
            "cannot install the speech model into {}; set FSCRIPT_MODEL_DIR to a folder path",
            model_dir.display()
        )
    })?;
    let install_failed = || {
        format!(
            "could not install the speech model into {}",
            destination_root.display()
        )
    };
    fs::create_dir_all(destination_root).explain(install_failed, MODEL_DIR_HINT)?;

    let staging_dir =
        tempfile::tempdir_in(destination_root).explain(install_failed, MODEL_DIR_HINT)?;
    eprintln!("installing the speech model...");
    let archive_file = File::open(package_path).explain(
        || {
            format!(
                "could not open the speech model package {}",
                package_path.display()
            )
        },
        MODEL_DIR_HINT,
    )?;
    let decoder = GzDecoder::new(archive_file);
    let mut archive = Archive::new(decoder);
    if let Err(error) = archive.unpack(staging_dir.path()) {
        if is_storage_problem(&error) {
            return Err(error).explain(install_failed, MODEL_DIR_HINT);
        }
        return Err(technical(error)).with_context(|| damaged_package_message(package_path));
    }
    remove_appledouble_files(staging_dir.path())?;

    let extracted_default_dir = staging_dir.path().join(crate::DEFAULT_MODEL_BASENAME);
    if has_required_model_files(&extracted_default_dir) {
        if model_dir.exists() {
            install_required_model_files(&extracted_default_dir, model_dir)?;
        } else {
            fs::rename(&extracted_default_dir, model_dir)
                .explain(install_failed, MODEL_DIR_HINT)?;
        }
    }
    Ok(())
}

fn damaged_package_message(package_path: &Path) -> String {
    format!(
        "the speech model package {} is damaged or incomplete; delete that file and run fscript again to download it fresh",
        package_path.display()
    )
}

/// Whether fscript chose this folder itself, so deleting it only removes a cache.
pub(crate) fn is_managed_model_dir(model_dir: &Path, model_url: &str) -> bool {
    if model_url == crate::DEFAULT_MODEL_URL {
        model_dir == default_model_dir()
    } else {
        model_dir == model_cache_paths(model_url).0
    }
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
    }

    extract_model_package(model_dir, package_path)?;
    if !has_required_model_files(model_dir) {
        let mut message = damaged_package_message(package_path);
        if model_url != crate::DEFAULT_MODEL_URL {
            message.push_str(
                ". If you set a model link, check that it points to a compatible model bundle",
            );
        }
        bail!("{message}");
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
    fn damaged_cached_package_says_how_to_recover() {
        let dir = tempdir().unwrap();
        let package = dir.path().join("cache").join("bundle.tar.gz");
        fs::create_dir_all(package.parent().unwrap()).unwrap();
        fs::write(&package, "not a tarball").unwrap();
        let error = ensure_model_dir(
            &dir.path().join("models").join("bundle"),
            &package,
            crate::DEFAULT_MODEL_URL,
        )
        .unwrap_err();
        let message = crate::errors::render(&error);
        assert!(
            message.contains("is damaged or incomplete; delete that file and run fscript again"),
            "{message}"
        );
        assert!(!message.contains("gzip"), "{message}");
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

    #[test]
    fn ensuring_selected_bundle_preserves_other_model_directories() {
        let dir = tempdir().unwrap();
        let models = dir.path().join("models");
        let default_model = models.join(crate::DEFAULT_MODEL_BASENAME);
        let selected_model = models.join("parakeet-fr");
        let package_path = dir.path().join("cache").join("selected.tar.gz");
        fs::create_dir_all(&default_model).unwrap();
        for file_name in crate::REQUIRED_MODEL_FILES {
            fs::write(
                default_model.join(file_name),
                format!("default:{file_name}"),
            )
            .unwrap();
        }
        write_model_package(&package_path, "selected");

        ensure_model_dir(
            &selected_model,
            &package_path,
            "https://example.test/selected.tar.gz",
        )
        .unwrap();

        for file_name in crate::REQUIRED_MODEL_FILES {
            assert_eq!(
                fs::read_to_string(default_model.join(file_name)).unwrap(),
                format!("default:{file_name}")
            );
            assert_eq!(
                fs::read_to_string(selected_model.join(file_name)).unwrap(),
                format!("selected:{file_name}")
            );
        }
    }
}
