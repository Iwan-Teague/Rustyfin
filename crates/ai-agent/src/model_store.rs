use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};

use async_stream::stream;
use futures::{Stream, TryStreamExt};
use llama_cpp_2::model::LlamaModel;
use llama_cpp_2::model::params::LlamaModelParams;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::io::StreamReader;
use tracing::warn;

use crate::backend::shared_backend;
use crate::error::AiError;
use crate::types::{ModelInfo, PullChunk};

const CATALOG: &[(&str, &str)] = &[
    (
        "qwen2.5:1.5b",
        "https://huggingface.co/bartowski/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/Qwen2.5-1.5B-Instruct-Q4_K_M.gguf",
    ),
    (
        "llama3.2:3b",
        "https://huggingface.co/bartowski/Llama-3.2-3B-Instruct-GGUF/resolve/main/Llama-3.2-3B-Instruct-Q4_K_M.gguf",
    ),
    (
        "llama3.1:8b",
        "https://huggingface.co/bartowski/Meta-Llama-3.1-8B-Instruct-GGUF/resolve/main/Meta-Llama-3.1-8B-Instruct-Q4_K_M.gguf",
    ),
    (
        "qwen2.5:7b",
        "https://huggingface.co/bartowski/Qwen2.5-7B-Instruct-GGUF/resolve/main/Qwen2.5-7B-Instruct-Q4_K_M.gguf",
    ),
    (
        "deepseek-r1:8b",
        "https://huggingface.co/bartowski/DeepSeek-R1-Distill-Llama-8B-GGUF/resolve/main/DeepSeek-R1-Distill-Llama-8B-Q4_K_M.gguf",
    ),
    (
        "mistral:7b",
        "https://huggingface.co/bartowski/Mistral-7B-Instruct-v0.3-GGUF/resolve/main/Mistral-7B-Instruct-v0.3-Q4_K_M.gguf",
    ),
];

pub struct ModelStore;

impl ModelStore {
    pub fn discover(model_dir: &Path) -> Result<Vec<ModelInfo>, AiError> {
        if !model_dir.exists() {
            return Ok(Vec::new());
        }
        if !model_dir.is_dir() {
            return Err(AiError::ModelDirError(format!(
                "{} is not a directory",
                model_dir.display()
            )));
        }

        let backend = shared_backend()?;
        let mut out = Vec::new();
        let read_dir = std::fs::read_dir(model_dir).map_err(|error| {
            AiError::ModelDirError(format!(
                "failed to read model dir {}: {error}",
                model_dir.display()
            ))
        })?;

        for entry in read_dir {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warn!(error = %error, "failed to read model directory entry");
                    continue;
                }
            };

            let path = entry.path();
            if path.extension() != Some(OsStr::new("gguf")) {
                continue;
            }

            let metadata = match entry.metadata() {
                Ok(metadata) if metadata.is_file() => metadata,
                _ => continue,
            };

            let file = path
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or_default()
                .to_string();
            let name = path
                .file_stem()
                .and_then(OsStr::to_str)
                .unwrap_or_default()
                .to_string();

            let mut info = ModelInfo {
                name: name.clone(),
                file: file.clone(),
                size_gb: bytes_to_gb(metadata.len()),
                parameter_size: None,
                quantization: quantization_from_filename(&file),
                architecture: None,
                context_length: None,
            };

            let model_params = LlamaModelParams::default().with_vocab_only(true);
            match LlamaModel::load_from_file(backend, &path, &model_params) {
                Ok(model) => {
                    let meta = read_metadata(&model);
                    info.architecture = meta.get("general.architecture").cloned();
                    info.parameter_size = parameter_size_from_meta(&meta);
                    info.quantization = meta
                        .get("general.quantization")
                        .cloned()
                        .or(info.quantization);
                    info.context_length =
                        context_length_from_meta(&meta, info.architecture.as_deref());
                }
                Err(error) => {
                    warn!(
                        model = %path.display(),
                        error = %error,
                        "failed to load gguf metadata, using filename fallback"
                    );
                }
            }

            out.push(info);
        }

        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    pub fn download(
        model_or_url: String,
        model_dir: PathBuf,
        http_client: reqwest::Client,
    ) -> impl Stream<Item = PullChunk> + Send + 'static + use<> {
        stream! {
            let input = model_or_url.trim().to_string();
            let (url, stem) = match resolve_download_target(&input) {
                Ok(target) => target,
                Err(error) => {
                    yield PullChunk::Error(error.to_string());
                    return;
                }
            };

            if let Err(error) = tokio::fs::create_dir_all(&model_dir).await {
                yield PullChunk::Error(format!("failed to create model directory: {error}"));
                return;
            }

            let final_path = model_dir.join(format!("{stem}.gguf"));
            let part_path = model_dir.join(format!("{stem}.gguf.part"));

            yield PullChunk::Progress {
                status: "starting".to_string(),
                bytes_done: 0,
                bytes_total: None,
                percent: 0,
            };

            let response = match http_client.get(&url).send().await {
                Ok(response) => response,
                Err(error) => {
                    yield PullChunk::Error(format!("failed to start download: {error}"));
                    return;
                }
            };

            if !response.status().is_success() {
                yield PullChunk::Error(format!("download failed with status {}", response.status()));
                return;
            }

            let bytes_total = response.content_length();
            let stream = response
                .bytes_stream()
                .map_err(|error| std::io::Error::other(error.to_string()));
            let mut reader = StreamReader::new(stream);

            let mut file = match tokio::fs::File::create(&part_path).await {
                Ok(file) => file,
                Err(error) => {
                    yield PullChunk::Error(format!("failed to create partial model file: {error}"));
                    return;
                }
            };

            let mut bytes_done: u64 = 0;
            let mut buffer = vec![0u8; 64 * 1024];

            loop {
                let read = match reader.read(&mut buffer).await {
                    Ok(read) => read,
                    Err(error) => {
                        let _ = tokio::fs::remove_file(&part_path).await;
                        yield PullChunk::Error(format!("download stream read error: {error}"));
                        return;
                    }
                };

                if read == 0 {
                    break;
                }

                if let Err(error) = file.write_all(&buffer[..read]).await {
                    let _ = tokio::fs::remove_file(&part_path).await;
                    yield PullChunk::Error(format!("failed writing model file: {error}"));
                    return;
                }

                bytes_done = bytes_done.saturating_add(read as u64);
                let percent = percent(bytes_done, bytes_total);

                yield PullChunk::Progress {
                    status: "downloading".to_string(),
                    bytes_done,
                    bytes_total,
                    percent,
                };
            }

            if let Err(error) = file.flush().await {
                let _ = tokio::fs::remove_file(&part_path).await;
                yield PullChunk::Error(format!("failed to flush model file: {error}"));
                return;
            }

            if let Err(error) = tokio::fs::rename(&part_path, &final_path).await {
                let _ = tokio::fs::remove_file(&part_path).await;
                yield PullChunk::Error(format!(
                    "failed to finalize model file {}: {error}",
                    final_path.display()
                ));
                return;
            }

            yield PullChunk::Progress {
                status: "success".to_string(),
                bytes_done,
                bytes_total,
                percent: 100,
            };
            yield PullChunk::Done;
        }
    }

    pub fn delete(name: &str, model_dir: &Path) -> Result<(), AiError> {
        let direct = model_dir.join(format!("{name}.gguf"));
        if direct.exists() {
            std::fs::remove_file(&direct)?;
            return Ok(());
        }

        let read_dir = std::fs::read_dir(model_dir).map_err(|error| {
            AiError::ModelDirError(format!(
                "failed to read model dir {}: {error}",
                model_dir.display()
            ))
        })?;

        for entry in read_dir {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    warn!(error = %error, "failed to read model entry while deleting");
                    continue;
                }
            };

            let path = entry.path();
            if path.extension() != Some(OsStr::new("gguf")) {
                continue;
            }
            let stem = match path.file_stem().and_then(OsStr::to_str) {
                Some(stem) => stem,
                None => continue,
            };
            if stem == name {
                std::fs::remove_file(path)?;
                return Ok(());
            }
        }

        Err(AiError::ModelNotFound(name.to_string()))
    }

    pub fn checksum(model_path: &Path) -> Result<String, AiError> {
        let mut file = std::fs::File::open(model_path).map_err(|error| {
            AiError::ModelDirError(format!(
                "failed to open model file {}: {error}",
                model_path.display()
            ))
        })?;
        let mut hasher = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file.read(&mut buffer).map_err(|error| {
                AiError::ModelDirError(format!(
                    "failed to read model file {}: {error}",
                    model_path.display()
                ))
            })?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok(format!("{:x}", hasher.finalize()))
    }
}

fn read_metadata(model: &LlamaModel) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    let count = model.meta_count();

    for index in 0..count {
        let key = match model.meta_key_by_index(index) {
            Ok(key) => key,
            Err(_) => continue,
        };
        let value = match model.meta_val_str_by_index(index) {
            Ok(value) => value,
            Err(_) => continue,
        };
        metadata.insert(key, value);
    }

    metadata
}

fn parameter_size_from_meta(meta: &HashMap<String, String>) -> Option<String> {
    let raw = meta.get("general.parameter_count")?;
    let parsed = raw.trim().parse::<f64>().ok()?;

    if parsed >= 1_000_000_000.0 {
        return Some(format!("{:.0}B", parsed / 1_000_000_000.0));
    }
    if parsed >= 1_000_000.0 {
        return Some(format!("{:.0}M", parsed / 1_000_000.0));
    }
    if parsed >= 1_000.0 {
        return Some(format!("{:.0}K", parsed / 1_000.0));
    }
    None
}

fn context_length_from_meta(
    meta: &HashMap<String, String>,
    architecture: Option<&str>,
) -> Option<u32> {
    if let Some(value) = meta
        .get("llama.context_length")
        .and_then(|v| v.parse::<u32>().ok())
    {
        return Some(value);
    }

    let key = architecture.map(|arch| format!("{arch}.context_length"))?;
    meta.get(&key).and_then(|value| value.parse::<u32>().ok())
}

fn quantization_from_filename(file: &str) -> Option<String> {
    let stem = file.strip_suffix(".gguf").unwrap_or(file);
    let segment = stem.rsplit('-').next()?;
    let upper = segment.to_ascii_uppercase();

    if upper.starts_with('Q') {
        return Some(segment.to_string());
    }
    None
}

fn resolve_download_target(input: &str) -> Result<(String, String), AiError> {
    if input.starts_with("https://") {
        let stem = sanitize_stem(stem_from_url(input));
        return Ok((input.to_string(), stem));
    }

    if let Some((name, url)) = CATALOG.iter().find(|(name, _)| *name == input) {
        let stem = sanitize_stem(name.replace(':', "-"));
        return Ok(((*url).to_string(), stem));
    }

    Err(AiError::DownloadError(format!(
        "unknown model '{input}' (expected curated model name or https URL)"
    )))
}

fn stem_from_url(url: &str) -> String {
    let file = url
        .rsplit('/')
        .next()
        .unwrap_or("model.gguf")
        .split('?')
        .next()
        .unwrap_or("model.gguf");
    let filename = if file.ends_with(".gguf") {
        file.to_string()
    } else {
        format!("{file}.gguf")
    };
    Path::new(&filename)
        .file_stem()
        .and_then(OsStr::to_str)
        .unwrap_or("model")
        .to_string()
}

fn sanitize_stem(stem: String) -> String {
    let sanitized: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();

    if sanitized.is_empty() {
        "model".to_string()
    } else {
        sanitized
    }
}

fn bytes_to_gb(bytes: u64) -> f64 {
    bytes as f64 / (1024.0 * 1024.0 * 1024.0)
}

fn percent(bytes_done: u64, bytes_total: Option<u64>) -> u8 {
    match bytes_total {
        Some(total) if total > 0 => {
            let value = (bytes_done as f64 / total as f64) * 100.0;
            value.clamp(0.0, 100.0).round() as u8
        }
        _ => 0,
    }
}
