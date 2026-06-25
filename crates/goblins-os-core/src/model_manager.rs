use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    env,
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, SystemTime},
};
use sysinfo::{Disks, System};

use crate::http_error::error_response;

const GIB: u64 = 1024 * 1024 * 1024;
const MAX_MODEL_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;
/// Free space the model store must keep beyond the download itself so the OS
/// never runs the model partition to exhaustion mid-install.
const MODEL_STORE_HEADROOM_BYTES: u64 = 2 * GIB;

#[derive(Serialize)]
#[serde(rename_all = "kebab-case")]
#[derive(PartialEq, Eq)]
pub enum LocalModelState {
    Installable,
    Waiting,
    Blocked,
}

#[derive(Serialize)]
pub struct HardwareReport {
    ram_gb: u64,
    gpu_vram_gb: Option<u64>,
    model_dir: String,
    model_dir_available_gb: Option<u64>,
    runtime: RuntimeReport,
}

#[derive(Serialize)]
pub struct RuntimeReport {
    selected: Option<String>,
    ollama: bool,
    vllm: bool,
    lm_studio: bool,
}

#[derive(Serialize)]
pub struct LocalModelOption {
    id: &'static str,
    name: &'static str,
    role: &'static str,
    source: &'static str,
    weights_in_os_image: bool,
    download_required: bool,
    minimum_ram_gb: u64,
    minimum_gpu_vram_gb: Option<u64>,
    minimum_free_storage_gb: u64,
    disk_requirement: &'static str,
    state: LocalModelState,
    reasons: Vec<String>,
    install: LocalModelInstall,
}

#[derive(Serialize)]
pub struct LocalModelCatalog {
    install_policy: &'static str,
    hardware: HardwareReport,
    models: Vec<LocalModelOption>,
}

#[derive(Serialize)]
pub struct LocalModelInstall {
    state: LocalModelInstallState,
    consent_required: bool,
    consent_recorded: bool,
    manifest_required: bool,
    verification_required: bool,
    resumable: bool,
    state_path: String,
    target_dir: String,
    manifest_path: String,
    detail: String,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum LocalModelInstallState {
    NotRequested,
    WaitingForConsent,
    WaitingForManifest,
    Queued,
    Downloading,
    Installed,
    Blocked,
    Failed,
}

#[derive(Deserialize)]
pub struct LocalModelInstallRequest {
    model_id: String,
    consent: bool,
}

#[derive(Serialize)]
struct LocalModelInstallResponse {
    ok: bool,
    model_id: String,
    state: LocalModelInstallState,
    state_path: String,
    target_dir: String,
    manifest_path: String,
    detail: String,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredModelInstall {
    model_id: String,
    source: String,
    state: LocalModelInstallState,
    consent_recorded: bool,
    requested_at: String,
    updated_at: String,
    target_dir: String,
    manifest_path: String,
    detail: String,
    files: Vec<StoredModelFile>,
}

#[derive(Serialize, Deserialize, Clone)]
struct StoredModelFile {
    relative_path: String,
    expected_sha256: String,
    expected_bytes: u64,
    downloaded_bytes: u64,
    verified: bool,
}

#[derive(Deserialize)]
struct ModelManifest {
    model_id: String,
    source: String,
    files: Vec<ModelManifestFile>,
}

#[derive(Deserialize)]
struct ModelManifestFile {
    url: String,
    path: String,
    sha256: String,
    bytes: u64,
}

pub async fn local_model_catalog() -> Json<LocalModelCatalog> {
    Json(build_local_model_catalog())
}

pub async fn install_local_model(Json(request): Json<LocalModelInstallRequest>) -> Response {
    match start_model_install(request) {
        Ok(response) => Json(response).into_response(),
        Err((status, text)) => error_response(status, text),
    }
}

pub fn build_local_model_catalog() -> LocalModelCatalog {
    let hardware = inspect_hardware();
    let runtime_ready = hardware.runtime.ollama
        || hardware.runtime.vllm
        || hardware.runtime.lm_studio
        || hardware.runtime.selected.is_some();

    LocalModelCatalog {
        install_policy:
            "Model weights are never bundled in the OS image; the installer downloads them only after compatibility and user consent checks pass.",
        models: vec![
            evaluate_model(ModelSpec {
                id: "gpt-oss-20b",
                name: "gpt-oss-20b",
                role: "Local/private reasoning for capable desktops and laptops",
                source: "openai/gpt-oss-20b",
                minimum_ram_gb: 16,
                minimum_gpu_vram_gb: None,
                minimum_free_storage_gb: 24,
                disk_requirement:
                    "About 16GB of verified weights; exact size confirmed from the provider manifest before download starts.",
            }, &hardware, runtime_ready),
            evaluate_model(ModelSpec {
                id: "gpt-oss-120b",
                name: "gpt-oss-120b",
                role: "High-end local reasoning for workstation or datacenter-class systems",
                source: "openai/gpt-oss-120b",
                minimum_ram_gb: 16,
                minimum_gpu_vram_gb: Some(80),
                minimum_free_storage_gb: 96,
                disk_requirement:
                    "About 65GB of verified weights; exact size confirmed from the provider manifest before download starts.",
            }, &hardware, runtime_ready),
        ],
        hardware,
    }
}

impl LocalModelCatalog {
    pub fn installable_model_count(&self) -> usize {
        self.models
            .iter()
            .filter(|model| model.state == LocalModelState::Installable)
            .count()
    }

    pub fn blocked_model_count(&self) -> usize {
        self.models
            .iter()
            .filter(|model| model.state == LocalModelState::Blocked)
            .count()
    }

    pub fn hardware(&self) -> &HardwareReport {
        &self.hardware
    }
}

impl HardwareReport {
    pub fn ram_gb(&self) -> u64 {
        self.ram_gb
    }

    pub fn gpu_vram_gb(&self) -> Option<u64> {
        self.gpu_vram_gb
    }

    pub fn model_dir_available_gb(&self) -> Option<u64> {
        self.model_dir_available_gb
    }

    pub fn runtime_ready(&self) -> bool {
        self.runtime.ollama
            || self.runtime.vllm
            || self.runtime.lm_studio
            || self.runtime.selected.is_some()
    }
}

struct ModelSpec {
    id: &'static str,
    name: &'static str,
    role: &'static str,
    source: &'static str,
    minimum_ram_gb: u64,
    minimum_gpu_vram_gb: Option<u64>,
    minimum_free_storage_gb: u64,
    disk_requirement: &'static str,
}

enum StorageVerdict {
    Sufficient,
    Insufficient { available_gb: u64 },
    Unmeasured,
}

fn storage_verdict(available_gb: Option<u64>, minimum_gb: u64) -> StorageVerdict {
    match available_gb {
        Some(available) if available >= minimum_gb => StorageVerdict::Sufficient,
        Some(available) => StorageVerdict::Insufficient {
            available_gb: available,
        },
        None => StorageVerdict::Unmeasured,
    }
}

fn evaluate_model(
    spec: ModelSpec,
    hardware: &HardwareReport,
    runtime_ready: bool,
) -> LocalModelOption {
    let mut reasons = Vec::new();
    let mut blocked = false;

    if hardware.ram_gb < spec.minimum_ram_gb {
        blocked = true;
        reasons.push(format!(
            "Requires at least {}GB RAM; detected {}GB.",
            spec.minimum_ram_gb, hardware.ram_gb
        ));
    }

    if let Some(required_vram) = spec.minimum_gpu_vram_gb {
        match hardware.gpu_vram_gb {
            Some(detected_vram) if detected_vram >= required_vram => {}
            Some(detected_vram) => {
                blocked = true;
                reasons.push(format!(
                    "Requires an {required_vram}GB GPU class system; detected {detected_vram}GB VRAM."
                ));
            }
            None => {
                blocked = true;
                reasons.push(format!(
                    "Requires an {required_vram}GB GPU class system; no compatible GPU VRAM was detected."
                ));
            }
        }
    }

    if !runtime_ready {
        reasons.push(
            "Waiting for a local inference runtime such as Ollama, vLLM, LM Studio, or an OS-selected runtime."
                .to_string(),
        );
    }

    match storage_verdict(
        hardware.model_dir_available_gb,
        spec.minimum_free_storage_gb,
    ) {
        StorageVerdict::Sufficient => {}
        StorageVerdict::Insufficient { available_gb } => {
            blocked = true;
            reasons.push(format!(
                "Requires about {}GB free in the model store; only {}GB is available.",
                spec.minimum_free_storage_gb, available_gb
            ));
        }
        StorageVerdict::Unmeasured => {
            reasons.push(
                "Storage availability for the model directory could not be measured.".to_string(),
            );
        }
    }

    let state = if blocked {
        LocalModelState::Blocked
    } else if runtime_ready && hardware.model_dir_available_gb.is_some() {
        LocalModelState::Installable
    } else {
        LocalModelState::Waiting
    };

    let install = install_state_for(spec.id);

    LocalModelOption {
        id: spec.id,
        name: spec.name,
        role: spec.role,
        source: spec.source,
        weights_in_os_image: false,
        download_required: true,
        minimum_ram_gb: spec.minimum_ram_gb,
        minimum_gpu_vram_gb: spec.minimum_gpu_vram_gb,
        minimum_free_storage_gb: spec.minimum_free_storage_gb,
        disk_requirement: spec.disk_requirement,
        state,
        reasons,
        install,
    }
}

fn start_model_install(
    request: LocalModelInstallRequest,
) -> Result<LocalModelInstallResponse, (StatusCode, &'static str)> {
    let catalog = build_local_model_catalog();
    let Some(model) = catalog
        .models
        .iter()
        .find(|model| model.id == request.model_id)
    else {
        return Err((StatusCode::NOT_FOUND, "Unknown local model."));
    };

    if !request.consent {
        return Err((
            StatusCode::PRECONDITION_REQUIRED,
            "Local model downloads require explicit user consent.",
        ));
    }
    if model.state != LocalModelState::Installable {
        return Err((
            StatusCode::CONFLICT,
            "Local model is not installable on the current hardware and runtime state.",
        ));
    }

    // Reject a second download request while one is already queued or running so
    // overlapping threads cannot append to the same .partial file and corrupt it.
    if let Ok(existing) = read_install_state(model.id) {
        if matches!(
            existing.state,
            LocalModelInstallState::Queued | LocalModelInstallState::Downloading
        ) {
            return Err((
                StatusCode::CONFLICT,
                "A download for this model is already in progress.",
            ));
        }
    }

    let manifest_path = manifest_path(model.id);
    let target_dir = model_target_dir(model.id);
    let source = model.source.to_string();
    let stored = match read_manifest(&manifest_path) {
        Ok(manifest) => {
            validate_manifest(model.id, &manifest)?;
            ensure_model_store_capacity(&target_dir, &manifest)?;
            // Atomically claim this model so two concurrent requests cannot both
            // pass the persisted-state check above and spawn racing downloads.
            if !claim_download(model.id) {
                return Err((
                    StatusCode::CONFLICT,
                    "A download for this model is already in progress.",
                ));
            }
            let stored = StoredModelInstall {
                model_id: model.id.to_string(),
                source,
                state: LocalModelInstallState::Queued,
                consent_recorded: true,
                requested_at: format!("{:?}", SystemTime::now()),
                updated_at: format!("{:?}", SystemTime::now()),
                target_dir: target_dir.display().to_string(),
                manifest_path: manifest_path.display().to_string(),
                detail: "Download queued; each file resumes from its partial file and verifies SHA-256 before becoming active.".to_string(),
                files: manifest
                    .files
                    .iter()
                    .map(|file| StoredModelFile {
                        relative_path: file.path.clone(),
                        expected_sha256: file.sha256.clone(),
                        expected_bytes: file.bytes,
                        downloaded_bytes: partial_file_len(&target_dir, &file.path),
                        verified: false,
                    })
                    .collect(),
            };
            persist_install_state(&stored).map_err(|_| {
                release_download(model.id);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Model install state could not be written.",
                )
            })?;
            spawn_manifest_download(model.id.to_string(), target_dir.clone(), manifest);
            stored
        }
        Err(ManifestReadError::Missing) => StoredModelInstall {
            model_id: model.id.to_string(),
            source,
            state: LocalModelInstallState::WaitingForManifest,
            consent_recorded: true,
            requested_at: format!("{:?}", SystemTime::now()),
            updated_at: format!("{:?}", SystemTime::now()),
            target_dir: target_dir.display().to_string(),
            manifest_path: manifest_path.display().to_string(),
            detail: "Consent recorded. Waiting for a provider manifest with HTTPS URLs, byte counts, and SHA-256 digests before any weight download starts.".to_string(),
            files: Vec::new(),
        },
        Err(ManifestReadError::Invalid) => StoredModelInstall {
            model_id: model.id.to_string(),
            source,
            state: LocalModelInstallState::Failed,
            consent_recorded: true,
            requested_at: format!("{:?}", SystemTime::now()),
            updated_at: format!("{:?}", SystemTime::now()),
            target_dir: target_dir.display().to_string(),
            manifest_path: manifest_path.display().to_string(),
            detail: "Provider manifest is invalid; refusing to download unverifiable model weights."
                .to_string(),
            files: Vec::new(),
        },
    };

    if stored.state != LocalModelInstallState::Queued {
        persist_install_state(&stored).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Model install state could not be written.",
            )
        })?;
    }

    Ok(LocalModelInstallResponse {
        ok: true,
        model_id: stored.model_id,
        state: stored.state,
        state_path: install_state_path(model.id).display().to_string(),
        target_dir: stored.target_dir,
        manifest_path: stored.manifest_path,
        detail: stored.detail,
    })
}

fn install_state_for(model_id: &str) -> LocalModelInstall {
    let state_path = install_state_path(model_id);
    let manifest_path = manifest_path(model_id);
    let target_dir = model_target_dir(model_id);
    let stored = fs::read(&state_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<StoredModelInstall>(&bytes).ok());

    match stored {
        Some(stored) => LocalModelInstall {
            state: stored.state,
            consent_required: false,
            consent_recorded: stored.consent_recorded,
            manifest_required: !manifest_path.is_file(),
            verification_required: true,
            resumable: true,
            state_path: state_path.display().to_string(),
            target_dir: target_dir.display().to_string(),
            manifest_path: manifest_path.display().to_string(),
            detail: stored.detail,
        },
        None => LocalModelInstall {
            state: LocalModelInstallState::NotRequested,
            consent_required: true,
            consent_recorded: false,
            manifest_required: !manifest_path.is_file(),
            verification_required: true,
            resumable: true,
            state_path: state_path.display().to_string(),
            target_dir: target_dir.display().to_string(),
            manifest_path: manifest_path.display().to_string(),
            detail: "No download has been requested. The installer must record consent before Goblins OS touches model weights.".to_string(),
        },
    }
}

#[derive(Debug)]
enum ManifestReadError {
    Missing,
    Invalid,
}

fn read_manifest(path: &Path) -> Result<ModelManifest, ManifestReadError> {
    let file = fs::File::open(path).map_err(|_| ManifestReadError::Missing)?;
    let metadata = file.metadata().map_err(|_| ManifestReadError::Invalid)?;
    if metadata.len() > MAX_MODEL_MANIFEST_BYTES {
        return Err(ManifestReadError::Invalid);
    }

    serde_json::from_reader(file).map_err(|_| ManifestReadError::Invalid)
}

fn validate_manifest(
    model_id: &str,
    manifest: &ModelManifest,
) -> Result<(), (StatusCode, &'static str)> {
    if manifest.model_id != model_id {
        return Err((StatusCode::BAD_REQUEST, "Model manifest id does not match."));
    }
    if manifest.source.trim().is_empty() || manifest.files.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "Model manifest is incomplete."));
    }
    for file in &manifest.files {
        if !file.url.starts_with("https://") {
            return Err((StatusCode::BAD_REQUEST, "Model file URL must use HTTPS."));
        }
        if file.path.starts_with('/') || file.path.contains("..") || file.path.trim().is_empty() {
            return Err((StatusCode::BAD_REQUEST, "Model file path is not safe."));
        }
        if file.sha256.len() != 64 || !file.sha256.chars().all(|char| char.is_ascii_hexdigit()) {
            return Err((
                StatusCode::BAD_REQUEST,
                "Model file SHA-256 digest is required.",
            ));
        }
        if file.bytes == 0 {
            return Err((StatusCode::BAD_REQUEST, "Model file byte size is required."));
        }
    }

    Ok(())
}

fn spawn_manifest_download(model_id: String, target_dir: PathBuf, manifest: ModelManifest) {
    thread::spawn(move || {
        if let Err(error) = download_manifest_files(&model_id, &target_dir, &manifest) {
            let _ = mark_install_failed(&model_id, error);
        }
        // Release the in-process claim whether the download succeeded or failed,
        // so a later retry (e.g. to resume) is allowed.
        release_download(&model_id);
    });
}

/// Process-wide set of model ids with an in-flight download, guarding against
/// concurrent install requests racing on the same partial files.
fn in_flight_downloads() -> &'static Mutex<HashSet<String>> {
    static IN_FLIGHT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}

fn claim_download(model_id: &str) -> bool {
    let mut guard = match in_flight_downloads().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.insert(model_id.to_string())
}

fn release_download(model_id: &str) {
    let mut guard = match in_flight_downloads().lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    guard.remove(model_id);
}

fn download_manifest_files(
    model_id: &str,
    target_dir: &Path,
    manifest: &ModelManifest,
) -> Result<(), String> {
    update_install_state(
        model_id,
        LocalModelInstallState::Downloading,
        "Download started.",
    )?;

    for file in &manifest.files {
        download_file(target_dir, file)?;
        update_file_progress(model_id, target_dir, file, false)?;
        verify_file(target_dir, file)?;
        update_file_progress(model_id, target_dir, file, true)?;
    }

    update_install_state(
        model_id,
        LocalModelInstallState::Installed,
        "Model weights are installed and verified outside the immutable OS image.",
    )
}

/// HTTP agent for weight downloads: bounded connect/read/write timeouts so a
/// stalled or slow-loris provider cannot hang the install thread forever, and
/// `redirects(0)` so the manifest's verified HTTPS URL cannot be silently
/// downgraded to HTTP via a 3xx response.
fn download_agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(120))
        .timeout_write(Duration::from_secs(30))
        .redirects(0)
        .build()
}

fn download_file(target_dir: &Path, file: &ModelManifestFile) -> Result<(), String> {
    let final_path = safe_join(target_dir, &file.path)?;
    let partial_path = final_path.with_extension("partial");
    if let Some(parent) = partial_path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let existing = partial_path
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if existing > file.bytes {
        fs::remove_file(&partial_path).map_err(|error| error.to_string())?;
    }

    let mut request = download_agent().get(&file.url);
    let resume_from = partial_path
        .metadata()
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    if resume_from > 0 {
        request = request.set("Range", &format!("bytes={resume_from}-"));
    }

    let response = request.call().map_err(|error| error.to_string())?;
    if (300..=399).contains(&response.status()) {
        return Err(
            "Provider attempted a redirect; refusing to follow it to avoid an HTTPS downgrade."
                .to_string(),
        );
    }
    if resume_from > 0 && response.status() != 206 {
        fs::remove_file(&partial_path).map_err(|error| error.to_string())?;
        return Err(
            "Provider did not honor the resume range; partial file was removed.".to_string(),
        );
    }
    if resume_from == 0 && !(200..=299).contains(&response.status()) {
        return Err(format!("Provider returned HTTP {}.", response.status()));
    }

    let mut reader = response.into_reader();
    let mut writer = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&partial_path)
        .map_err(|error| error.to_string())?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        writer
            .write_all(&buffer[..read])
            .map_err(|error| error.to_string())?;
    }
    writer.sync_all().map_err(|error| error.to_string())?;

    let downloaded = partial_path
        .metadata()
        .map_err(|error| error.to_string())?
        .len();
    if downloaded != file.bytes {
        return Err(format!(
            "Downloaded byte count does not match manifest for {}.",
            file.path
        ));
    }

    Ok(())
}

fn verify_file(target_dir: &Path, file: &ModelManifestFile) -> Result<(), String> {
    let final_path = safe_join(target_dir, &file.path)?;
    let partial_path = final_path.with_extension("partial");
    let mut hasher = Sha256::new();
    let mut reader = fs::File::open(&partial_path).map_err(|error| error.to_string())?;
    let mut buffer = [0_u8; 64 * 1024];

    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }

    let digest = format!("{:x}", hasher.finalize());
    if !digest.eq_ignore_ascii_case(&file.sha256) {
        return Err(format!("SHA-256 verification failed for {}.", file.path));
    }

    fs::rename(partial_path, final_path).map_err(|error| error.to_string())
}

fn update_file_progress(
    model_id: &str,
    target_dir: &Path,
    manifest_file: &ModelManifestFile,
    verified: bool,
) -> Result<(), String> {
    let mut stored = read_install_state(model_id)?;
    for file in &mut stored.files {
        if file.relative_path == manifest_file.path {
            file.downloaded_bytes = if verified {
                manifest_file.bytes
            } else {
                partial_file_len(target_dir, &manifest_file.path)
            };
            file.verified = verified;
        }
    }
    stored.updated_at = format!("{:?}", SystemTime::now());
    persist_install_state(&stored).map_err(|error| error.to_string())
}

fn update_install_state(
    model_id: &str,
    state: LocalModelInstallState,
    detail: &str,
) -> Result<(), String> {
    let mut stored = read_install_state(model_id)?;
    stored.state = state;
    stored.detail = detail.to_string();
    stored.updated_at = format!("{:?}", SystemTime::now());
    persist_install_state(&stored).map_err(|error| error.to_string())
}

fn mark_install_failed(model_id: &str, detail: String) -> Result<(), String> {
    update_install_state(model_id, LocalModelInstallState::Failed, &detail)
}

fn read_install_state(model_id: &str) -> Result<StoredModelInstall, String> {
    let bytes = fs::read(install_state_path(model_id)).map_err(|error| error.to_string())?;
    serde_json::from_slice(&bytes).map_err(|error| error.to_string())
}

fn persist_install_state(state: &StoredModelInstall) -> std::io::Result<()> {
    let path = install_state_path(&state.model_id);
    let Some(parent) = path.parent() else {
        return Err(std::io::Error::other(
            "model install state path has no parent",
        ));
    };
    fs::create_dir_all(parent)?;
    fs::write(path, serde_json::to_vec_pretty(state)?)
}

fn partial_file_len(target_dir: &Path, relative_path: &str) -> u64 {
    safe_join(target_dir, relative_path)
        .ok()
        .map(|path| path.with_extension("partial"))
        .and_then(|path| path.metadata().ok())
        .map(|metadata| metadata.len())
        .unwrap_or(0)
}

fn safe_join(base: &Path, relative_path: &str) -> Result<PathBuf, String> {
    if relative_path.starts_with('/') || relative_path.contains("..") || relative_path.is_empty() {
        return Err("Model file path is outside the model directory.".to_string());
    }

    Ok(base.join(relative_path))
}

fn install_state_path(model_id: &str) -> PathBuf {
    model_install_state_dir().join(format!("{model_id}.json"))
}

fn manifest_path(model_id: &str) -> PathBuf {
    model_manifest_dir().join(format!("{model_id}.json"))
}

fn model_target_dir(model_id: &str) -> PathBuf {
    model_dir().join(model_id)
}

pub(crate) fn model_dir() -> PathBuf {
    env::var("GOBLINS_OS_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new("/var/lib/goblins-os/models").to_path_buf())
}

fn model_install_state_dir() -> PathBuf {
    env::var("GOBLINS_OS_MODEL_INSTALL_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new("/var/lib/goblins-os/models/install-state").to_path_buf())
}

fn model_manifest_dir() -> PathBuf {
    env::var("GOBLINS_OS_MODEL_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| Path::new("/etc/goblins-os/model-manifests").to_path_buf())
}

fn inspect_hardware() -> HardwareReport {
    let mut system = System::new();
    system.refresh_memory();

    let model_dir = model_dir();

    HardwareReport {
        // An explicit operator override wins, for VMs/containers/cgroups where
        // total-RAM auto-detection is wrong (mirrors GOBLINS_OS_GPU_VRAM_GB).
        ram_gb: parse_positive_gb(env::var("GOBLINS_OS_RAM_GB").ok().as_deref())
            .unwrap_or_else(|| bytes_to_gib(system.total_memory())),
        gpu_vram_gb: crate::accelerators::best_vram_gb(),
        model_dir: model_dir.display().to_string(),
        model_dir_available_gb: available_space_gb(&model_dir),
        runtime: detect_runtimes(),
    }
}

fn parse_positive_gb(value: Option<&str>) -> Option<u64> {
    value
        .map(str::trim)
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|gb| *gb > 0)
}

/// Detect the local inference runtimes Goblins OS can hand local models to.
/// Shared with the system hardware view so both surfaces report the same state.
pub(crate) fn detect_runtimes() -> RuntimeReport {
    RuntimeReport {
        selected: env::var("GOBLINS_OS_LOCAL_MODEL_RUNTIME").ok(),
        ollama: executable_exists("ollama"),
        vllm: executable_exists("vllm"),
        lm_studio: executable_exists("lms"),
    }
}

fn bytes_to_gib(bytes: u64) -> u64 {
    bytes.div_ceil(GIB)
}

pub(crate) fn available_space_gb(path: &Path) -> Option<u64> {
    available_space_bytes(path).map(bytes_to_gib)
}

fn available_space_bytes(path: &Path) -> Option<u64> {
    let disks = Disks::new_with_refreshed_list();
    disks
        .iter()
        .filter(|disk| path.starts_with(disk.mount_point()))
        .max_by_key(|disk| disk.mount_point().to_string_lossy().len())
        .map(|disk| disk.available_space())
}

/// Refuse to queue a download unless the verified weights, minus any resumable
/// progress, fit in the model store with headroom to spare.
fn ensure_model_store_capacity(
    target_dir: &Path,
    manifest: &ModelManifest,
) -> Result<(), (StatusCode, &'static str)> {
    let remaining = remaining_download_bytes(target_dir, manifest);
    match available_space_bytes(target_dir) {
        Some(available)
            if has_sufficient_storage(remaining, available, MODEL_STORE_HEADROOM_BYTES) =>
        {
            Ok(())
        }
        Some(_) => Err((
            StatusCode::INSUFFICIENT_STORAGE,
            "Model store does not have enough free space for the verified download plus safety headroom.",
        )),
        None => Err((
            StatusCode::INSUFFICIENT_STORAGE,
            "Model store free space could not be measured; refusing to start an unbounded download.",
        )),
    }
}

fn remaining_download_bytes(target_dir: &Path, manifest: &ModelManifest) -> u64 {
    manifest.files.iter().fold(0_u64, |total, file| {
        let already = partial_file_len(target_dir, &file.path).min(file.bytes);
        total.saturating_add(file.bytes.saturating_sub(already))
    })
}

fn has_sufficient_storage(required: u64, available: u64, headroom: u64) -> bool {
    available >= required.saturating_add(headroom)
}

fn executable_exists(binary: &str) -> bool {
    let Some(paths) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&paths).any(|path| path.join(binary).is_file())
}

#[cfg(test)]
mod tests {
    use super::{
        claim_download, download_file, has_sufficient_storage, parse_positive_gb, release_download,
        remaining_download_bytes, safe_join, storage_verdict, validate_manifest, verify_file,
        LocalModelInstallRequest, ModelManifest, ModelManifestFile, StorageVerdict,
        MODEL_STORE_HEADROOM_BYTES,
    };
    use axum::http::StatusCode;
    use sha2::{Digest, Sha256};
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::path::{Path, PathBuf};

    #[test]
    fn download_claim_is_exclusive_until_released() {
        let id = "goblins-os-test-claim-model";
        release_download(id);
        assert!(claim_download(id), "first claim should succeed");
        assert!(
            !claim_download(id),
            "a second claim must be rejected while the download is in flight"
        );
        release_download(id);
        assert!(
            claim_download(id),
            "after release a retry (e.g. resume) should be allowed"
        );
        release_download(id);
    }

    /// Serve `payload` once over a real localhost socket, honoring a `Range:
    /// bytes=N-` request with a 206 when `support_range` is set. Returns the URL.
    fn serve_payload(payload: Vec<u8>, support_range: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{addr}/model.bin");
        std::thread::spawn(move || {
            if let Some(Ok(mut stream)) = listener.incoming().next() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                let _ = reader.read_line(&mut line); // request line
                let mut range_start = 0usize;
                let mut has_range = false;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 {
                        break;
                    }
                    if header == "\r\n" || header == "\n" {
                        break;
                    }
                    if let Some(value) = header.to_ascii_lowercase().strip_prefix("range:") {
                        if let Some(spec) = value.trim().strip_prefix("bytes=") {
                            range_start = spec
                                .split('-')
                                .next()
                                .unwrap_or("0")
                                .trim()
                                .parse()
                                .unwrap_or(0);
                            has_range = true;
                        }
                    }
                }
                let serve_range = has_range && support_range && range_start <= payload.len();
                let body: &[u8] = if serve_range {
                    &payload[range_start..]
                } else {
                    &payload[..]
                };
                let status = if serve_range {
                    "206 Partial Content"
                } else {
                    "200 OK"
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
                let _ = stream.flush();
            }
        });
        url
    }

    fn unique_tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn download_and_verify_real_bytes_end_to_end() {
        let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let digest = format!("{:x}", Sha256::digest(&payload));
        let url = serve_payload(payload.clone(), false);
        let dir = unique_tmp("goblins-os-dl-basic");
        let file = ModelManifestFile {
            url,
            path: "model.bin".to_string(),
            sha256: digest,
            bytes: payload.len() as u64,
        };

        download_file(&dir, &file).expect("real download of real bytes should succeed");
        verify_file(&dir, &file).expect("SHA-256 verification should pass and activate the file");

        assert_eq!(std::fs::read(dir.join("model.bin")).unwrap(), payload);
        assert!(
            !dir.join("model.partial").exists(),
            "the partial is consumed once verified"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn download_resumes_from_a_partial_file() {
        let payload: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        let digest = format!("{:x}", Sha256::digest(&payload));
        let dir = unique_tmp("goblins-os-dl-resume");
        std::fs::create_dir_all(&dir).unwrap();
        // Simulate an interrupted prior run: 80_000 of 200_000 bytes already on disk.
        std::fs::write(dir.join("model.partial"), &payload[..80_000]).unwrap();

        let url = serve_payload(payload.clone(), true);
        let file = ModelManifestFile {
            url,
            path: "model.bin".to_string(),
            sha256: digest,
            bytes: payload.len() as u64,
        };

        download_file(&dir, &file).expect("download should resume from the partial via Range");
        verify_file(&dir, &file).expect("resumed bytes verify against the manifest digest");

        assert_eq!(std::fs::read(dir.join("model.bin")).unwrap(), payload);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_rejects_corrupted_bytes() {
        let payload: Vec<u8> = (0..50_000u32).map(|i| (i % 97) as u8).collect();
        let wrong_digest = format!("{:x}", Sha256::digest(b"a different artifact"));
        let url = serve_payload(payload.clone(), false);
        let dir = unique_tmp("goblins-os-dl-corrupt");
        let file = ModelManifestFile {
            url,
            path: "model.bin".to_string(),
            sha256: wrong_digest,
            bytes: payload.len() as u64,
        };

        download_file(&dir, &file).expect("bytes download");
        assert!(
            verify_file(&dir, &file).is_err(),
            "a digest mismatch must refuse to activate the file"
        );
        assert!(
            !dir.join("model.bin").exists(),
            "unverified bytes never become the active model file"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn safe_join_rejects_paths_outside_model_directory() {
        assert!(safe_join(Path::new("/models"), "../secret").is_err());
        assert!(safe_join(Path::new("/models"), "/secret").is_err());
        assert_eq!(
            safe_join(Path::new("/models"), "weights/model.safetensors").unwrap(),
            Path::new("/models/weights/model.safetensors")
        );
    }

    #[test]
    fn model_install_request_requires_explicit_consent_field() {
        let request = serde_json::from_slice::<LocalModelInstallRequest>(
            br#"{"model_id":"gpt-oss-20b","consent":true}"#,
        )
        .unwrap();

        assert_eq!(request.model_id, "gpt-oss-20b");
        assert!(request.consent);
    }

    #[test]
    fn manifest_requires_https_urls_and_sha256() {
        let manifest = ModelManifest {
            model_id: "gpt-oss-20b".to_string(),
            source: "openai/gpt-oss-20b".to_string(),
            files: vec![ModelManifestFile {
                url: "http://example.invalid/model.safetensors".to_string(),
                path: "model.safetensors".to_string(),
                sha256: "0".repeat(64),
                bytes: 1,
            }],
        };

        assert_eq!(
            validate_manifest("gpt-oss-20b", &manifest),
            Err((StatusCode::BAD_REQUEST, "Model file URL must use HTTPS."))
        );
    }

    #[test]
    fn storage_guard_reserves_headroom_beyond_the_download() {
        assert!(has_sufficient_storage(
            10,
            12 + MODEL_STORE_HEADROOM_BYTES,
            MODEL_STORE_HEADROOM_BYTES
        ));
        assert!(!has_sufficient_storage(
            10,
            10 + MODEL_STORE_HEADROOM_BYTES - 1,
            MODEL_STORE_HEADROOM_BYTES
        ));
        // Saturating arithmetic keeps a hostile manifest from wrapping required+headroom
        // back to zero and slipping past the check.
        assert!(!has_sufficient_storage(u64::MAX, u64::MAX - 1, 1));
    }

    #[test]
    fn ram_override_parses_only_positive_values() {
        assert_eq!(parse_positive_gb(Some("32")), Some(32));
        assert_eq!(parse_positive_gb(Some("  16  ")), Some(16));
        assert_eq!(parse_positive_gb(Some("0")), None);
        assert_eq!(parse_positive_gb(Some("not-a-number")), None);
        assert_eq!(parse_positive_gb(None), None);
    }

    #[test]
    fn storage_verdict_greys_out_models_that_will_not_fit() {
        assert!(matches!(
            storage_verdict(Some(120), 96),
            StorageVerdict::Sufficient
        ));
        assert!(matches!(
            storage_verdict(Some(96), 96),
            StorageVerdict::Sufficient
        ));
        assert!(matches!(
            storage_verdict(Some(50), 96),
            StorageVerdict::Insufficient { available_gb: 50 }
        ));
        assert!(matches!(
            storage_verdict(None, 96),
            StorageVerdict::Unmeasured
        ));
    }

    #[test]
    fn remaining_bytes_counts_the_full_download_when_no_partials_exist() {
        let manifest = ModelManifest {
            model_id: "gpt-oss-20b".to_string(),
            source: "openai/gpt-oss-20b".to_string(),
            files: vec![
                ModelManifestFile {
                    url: "https://example.test/a.safetensors".to_string(),
                    path: "a.safetensors".to_string(),
                    sha256: "0".repeat(64),
                    bytes: 4_000,
                },
                ModelManifestFile {
                    url: "https://example.test/b.safetensors".to_string(),
                    path: "b.safetensors".to_string(),
                    sha256: "0".repeat(64),
                    bytes: 6_000,
                },
            ],
        };

        // A directory with no partial files means the whole manifest is still pending.
        assert_eq!(
            remaining_download_bytes(Path::new("/nonexistent/goblins-os-test"), &manifest),
            10_000
        );
    }
}
