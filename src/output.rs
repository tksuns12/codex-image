use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use chrono::Utc;
use serde::Serialize;

use crate::diagnostics::CliError;

const DEFAULT_IMAGE_FORMAT: &str = "png";
const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedImageSource {
    pub path: PathBuf,
    pub not_before: Option<SystemTime>,
}

impl GeneratedImageSource {
    pub fn unchecked(path: PathBuf) -> Self {
        Self {
            path,
            not_before: None,
        }
    }

    pub fn trusted_after(path: PathBuf, not_before: SystemTime) -> Self {
        Self {
            path,
            not_before: Some(not_before),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GenerationManifest {
    pub prompt: String,
    pub model: String,
    pub manifest_path: String,
    pub images: Vec<GeneratedImageArtifact>,
    pub response: GenerationResponseMetadata,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BatchGenerationManifest {
    pub mode: &'static str,
    pub prompt_file: String,
    pub model: String,
    pub manifest_path: String,
    pub item_count: usize,
    pub items: Vec<BatchGenerationItemManifest>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BatchGenerationItemManifest {
    pub index: usize,
    pub prompt: String,
    pub out_dir: String,
    pub manifest_path: String,
    pub images: Vec<GeneratedImageArtifact>,
    pub response: GenerationResponseMetadata,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GeneratedImageArtifact {
    pub index: usize,
    pub path: String,
    pub format: String,
    pub byte_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GenerationResponseMetadata {
    pub created: i64,
    pub usage: UsageMetadata,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct UsageMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
}

pub fn write_generation_output_from_files(
    prompt: &str,
    model: &str,
    out_dir: &Path,
    source_paths: &[PathBuf],
) -> Result<GenerationManifest, CliError> {
    let sources: Vec<GeneratedImageSource> = source_paths
        .iter()
        .cloned()
        .map(GeneratedImageSource::unchecked)
        .collect();

    write_generation_output_from_sources(prompt, model, out_dir, &sources)
}

pub fn write_generation_output_from_sources(
    prompt: &str,
    model: &str,
    out_dir: &Path,
    source_paths: &[GeneratedImageSource],
) -> Result<GenerationManifest, CliError> {
    if source_paths.is_empty() {
        return Err(CliError::ImageGenerationResponseContract {
            source_message: "Codex image generation response missing image path".to_string(),
        });
    }

    fs::create_dir_all(out_dir).map_err(|_| CliError::OutputWriteFailed)?;

    let mut images = Vec::with_capacity(source_paths.len());

    for (idx, source) in source_paths.iter().enumerate() {
        let source_path = &source.path;
        let metadata =
            source_path
                .metadata()
                .map_err(|_| CliError::ImageGenerationResponseContract {
                    source_message: "Codex image generation source path missing".to_string(),
                })?;

        if !metadata.is_file() {
            return Err(CliError::ImageGenerationResponseContract {
                source_message: "Codex image generation source path missing".to_string(),
            });
        }

        if let Some(not_before) = source.not_before {
            validate_source_freshness(&metadata, not_before)?;
        }

        let format = source_path
            .extension()
            .and_then(|extension| extension.to_str())
            .map(str::trim)
            .filter(|extension| !extension.is_empty())
            .unwrap_or(DEFAULT_IMAGE_FORMAT)
            .to_ascii_lowercase();

        if !is_safe_format(&format) {
            return Err(CliError::ImageGenerationResponseContract {
                source_message: "Codex image generation source path has invalid format".to_string(),
            });
        }

        let image_bytes = fs::read(source_path).map_err(|_| CliError::OutputWriteFailed)?;
        let image_name = format!(
            "image-{index:04}.{format}",
            index = idx + 1,
            format = format
        );
        let image_path = out_dir.join(image_name);
        atomic_write_bytes(&image_path, &image_bytes).map_err(|_| CliError::OutputWriteFailed)?;

        if !image_path.is_file() {
            return Err(CliError::OutputVerificationFailed);
        }

        images.push(GeneratedImageArtifact {
            index: idx + 1,
            path: path_to_string(&image_path),
            format,
            byte_count: image_bytes.len(),
        });
    }

    let manifest_path = out_dir.join(MANIFEST_FILE);
    let manifest = GenerationManifest {
        prompt: prompt.to_string(),
        model: model.to_string(),
        manifest_path: path_to_string(&manifest_path),
        images,
        response: GenerationResponseMetadata {
            created: Utc::now().timestamp(),
            usage: UsageMetadata::default(),
        },
    };

    let manifest_json =
        serde_json::to_vec_pretty(&manifest).map_err(|_| CliError::OutputWriteFailed)?;
    atomic_write_bytes(&manifest_path, &manifest_json).map_err(|_| CliError::OutputWriteFailed)?;

    if !manifest_path.is_file() {
        return Err(CliError::OutputVerificationFailed);
    }

    Ok(manifest)
}

pub fn write_batch_generation_manifest(
    prompt_file: &Path,
    model: &str,
    out_dir: &Path,
    item_manifests: &[GenerationManifest],
) -> Result<BatchGenerationManifest, CliError> {
    if item_manifests.is_empty() {
        return Err(CliError::ImageGenerationResponseContract {
            source_message: "Codex batch generation response missing item manifests".to_string(),
        });
    }

    fs::create_dir_all(out_dir).map_err(|_| CliError::OutputWriteFailed)?;

    let mut items = Vec::with_capacity(item_manifests.len());
    for (idx, item_manifest) in item_manifests.iter().enumerate() {
        let item_manifest_path = PathBuf::from(&item_manifest.manifest_path);
        let item_out_dir = item_manifest_path
            .parent()
            .ok_or(CliError::OutputVerificationFailed)?;

        if !item_manifest_path.is_file() || !item_out_dir.is_dir() {
            return Err(CliError::OutputVerificationFailed);
        }

        items.push(BatchGenerationItemManifest {
            index: idx + 1,
            prompt: item_manifest.prompt.clone(),
            out_dir: path_to_string(item_out_dir),
            manifest_path: item_manifest.manifest_path.clone(),
            images: item_manifest.images.clone(),
            response: item_manifest.response.clone(),
        });
    }

    let manifest_path = out_dir.join(MANIFEST_FILE);
    let manifest = BatchGenerationManifest {
        mode: "batch",
        prompt_file: path_to_string(prompt_file),
        model: model.to_string(),
        manifest_path: path_to_string(&manifest_path),
        item_count: items.len(),
        items,
    };

    let manifest_json =
        serde_json::to_vec_pretty(&manifest).map_err(|_| CliError::OutputWriteFailed)?;
    atomic_write_bytes(&manifest_path, &manifest_json).map_err(|_| CliError::OutputWriteFailed)?;

    if !manifest_path.is_file() {
        return Err(CliError::OutputVerificationFailed);
    }

    Ok(manifest)
}

pub fn remove_batch_manifest_if_exists(out_dir: &Path) -> Result<(), CliError> {
    let manifest_path = out_dir.join(MANIFEST_FILE);

    match fs::remove_file(manifest_path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(CliError::OutputWriteFailed),
    }
}

fn validate_source_freshness(
    metadata: &fs::Metadata,
    not_before: SystemTime,
) -> Result<(), CliError> {
    let modified = metadata
        .modified()
        .map_err(|_| CliError::ImageGenerationResponseContract {
            source_message: "Codex image generation source path metadata unavailable".to_string(),
        })?;

    if modified < not_before {
        return Err(CliError::ImageGenerationResponseContract {
            source_message: "Codex image generation source path predates current invocation"
                .to_string(),
        });
    }

    Ok(())
}

fn atomic_write_bytes(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "output path does not have a parent directory",
        )
    })?;

    let mut attempt = 0_u32;
    let pid = std::process::id();

    loop {
        let tmp_path = parent.join(format!(
            ".{}.tmp-{}-{}",
            path.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("output"),
            pid,
            attempt
        ));

        match fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp_path)
        {
            Ok(mut file) => {
                file.write_all(bytes)?;
                file.sync_all()?;
                drop(file);

                fs::rename(&tmp_path, path)?;
                return Ok(());
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                attempt += 1;
                if attempt > 100 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        "failed to allocate temporary output file",
                    ));
                }
            }
            Err(err) => return Err(err),
        }
    }
}

fn is_safe_format(format: &str) -> bool {
    !format.is_empty() && format.chars().all(|c| c.is_ascii_alphanumeric())
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}
