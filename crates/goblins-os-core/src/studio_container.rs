//! Deterministic OCI packaging for generated static web projects.
//!
//! Containerization never executes project files and never pulls from a
//! registry. The immutable OS supplies one reviewed, statically linked BusyBox
//! binary; the builder places that binary, its license, and the already-reviewed
//! project snapshot into a normalized OCI image-layout archive that Podman,
//! Docker-compatible tooling, and Skopeo can import offline.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Read},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};

use serde_json::json;
use sha2::{Digest, Sha256};

#[cfg(target_os = "linux")]
const BUSYBOX: &str = "/usr/bin/busybox.musl.static";
#[cfg(not(target_os = "linux"))]
const BUSYBOX: &str = "/nonexistent/goblins-os-busybox.musl.static";
#[cfg(target_os = "linux")]
const BUSYBOX_LICENSE: &str = "/usr/share/doc/busybox/LICENSE";
#[cfg(not(target_os = "linux"))]
const BUSYBOX_LICENSE: &str = "/nonexistent/goblins-os-busybox-license";
#[cfg(target_os = "linux")]
const BUSYBOX_SOURCE_DIR: &str = "/usr/share/goblins-os/studio-container";
#[cfg(not(target_os = "linux"))]
const BUSYBOX_SOURCE_DIR: &str = "/nonexistent/goblins-os-studio-container";

const MAX_RUNTIME_BYTES: usize = 8 * 1024 * 1024;
const MAX_LICENSE_BYTES: usize = 256 * 1024;
const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
const MAX_OCI_ARCHIVE_BYTES: usize = 20 * 1024 * 1024;
const SOURCE_NOTICE: &str = "BusyBox is supplied by the Fedora busybox package. The exact corresponding signed Fedora source RPM is included beside this notice as busybox.src.rpm. Fedora packaging history: https://src.fedoraproject.org/rpms/busybox\n";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct StudioContainerStatus {
    pub(crate) available: bool,
    pub(crate) detail: String,
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct StudioContainerArchive {
    pub(crate) bytes: Vec<u8>,
    pub(crate) sha256: String,
    pub(crate) image_ref: String,
}

struct ContainerRuntimeAssets<'a> {
    binary: &'a [u8],
    license: &'a [u8],
    source: &'a [u8],
    architecture: &'a str,
}

pub(crate) fn container_status(files: &[String]) -> StudioContainerStatus {
    if crate::studio_runtime::web_entrypoint(files).is_none() {
        return StudioContainerStatus {
            available: false,
            detail: "Container packaging currently supports static web projects with index.html or public/index.html. Python projects still run in the local sandbox and export as source."
                .to_string(),
        };
    }
    let source_ready = busybox_source_rpm().is_some_and(|path| {
        path.to_str()
            .is_some_and(|path| trusted_runtime_asset_ready(path, false, MAX_SOURCE_BYTES))
    });
    if !trusted_runtime_asset_ready(BUSYBOX, true, MAX_RUNTIME_BYTES)
        || !trusted_runtime_asset_ready(BUSYBOX_LICENSE, false, MAX_LICENSE_BYTES)
        || !source_ready
    {
        return StudioContainerStatus {
            available: false,
            detail: "This image is missing the verified offline Studio container runtime. Update Goblins OS before packaging this static project."
                .to_string(),
        };
    }
    StudioContainerStatus {
        available: true,
        detail: "Builds a deterministic, networkless OCI image archive from the reviewed project snapshot and saves it to Downloads. No registry pull or project code execution occurs."
            .to_string(),
    }
}

pub(crate) fn build_container(
    id: &str,
    workspace_sha256: &str,
    files: BTreeMap<String, Vec<u8>>,
) -> Result<StudioContainerArchive, String> {
    let file_paths = files.keys().cloned().collect::<Vec<_>>();
    let entrypoint = crate::studio_runtime::web_entrypoint(&file_paths).ok_or_else(|| {
        "Container packaging requires index.html or public/index.html.".to_string()
    })?;
    let busybox = read_trusted_runtime_asset(BUSYBOX, true, MAX_RUNTIME_BYTES)
        .map_err(|_| "The verified offline Studio container runtime is unavailable.".to_string())?;
    let license = read_trusted_runtime_asset(BUSYBOX_LICENSE, false, MAX_LICENSE_BYTES)
        .map_err(|_| "The Studio container runtime license could not be included.".to_string())?;
    let source_path = busybox_source_rpm().ok_or_else(|| {
        "The Studio container runtime corresponding source is unavailable.".to_string()
    })?;
    let source = read_trusted_runtime_asset(
        source_path
            .to_str()
            .ok_or_else(|| "The Studio container runtime source path is invalid.".to_string())?,
        false,
        MAX_SOURCE_BYTES,
    )
    .map_err(|_| "The Studio container runtime corresponding source is unavailable.".to_string())?;
    build_container_from_parts(
        id,
        workspace_sha256,
        entrypoint,
        files,
        ContainerRuntimeAssets {
            binary: &busybox,
            license: &license,
            source: &source,
            architecture: oci_architecture(),
        },
    )
}

fn build_container_from_parts(
    id: &str,
    workspace_sha256: &str,
    entrypoint: &str,
    files: BTreeMap<String, Vec<u8>>,
    runtime: ContainerRuntimeAssets<'_>,
) -> Result<StudioContainerArchive, String> {
    if runtime.binary.is_empty()
        || runtime.license.is_empty()
        || runtime.source.is_empty()
        || files.is_empty()
    {
        return Err("The Studio container inputs are incomplete.".to_string());
    }
    if workspace_sha256.len() != 64
        || !workspace_sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("The reviewed workspace digest is invalid.".to_string());
    }
    let site_root = match entrypoint {
        "index.html" => "/workspace",
        "public/index.html" => "/workspace/public",
        _ => return Err("That static web entrypoint is not supported.".to_string()),
    };
    let image_ref = format!("localhost/goblins-studio-{id}:latest");

    let mut layer_files = BTreeMap::new();
    layer_files.insert("bin/busybox".to_string(), (runtime.binary.to_vec(), 0o755));
    layer_files.insert(
        "usr/share/licenses/busybox/LICENSE".to_string(),
        (runtime.license.to_vec(), 0o644),
    );
    layer_files.insert(
        "usr/share/licenses/busybox/SOURCE".to_string(),
        (SOURCE_NOTICE.as_bytes().to_vec(), 0o644),
    );
    layer_files.insert(
        "usr/share/source/busybox/busybox.src.rpm".to_string(),
        (runtime.source.to_vec(), 0o444),
    );
    for (path, content) in files {
        layer_files.insert(format!("workspace/{path}"), (content, 0o644));
    }
    let layer = deterministic_layer(&layer_files)?;
    let layer_digest = digest_hex(&layer);

    let config = serde_json::to_vec(&json!({
        "architecture": runtime.architecture,
        "os": "linux",
        "config": {
            "User": "65532:65532",
            "ExposedPorts": { "8080/tcp": {} },
            "Env": ["PATH=/bin"],
            "Entrypoint": ["/bin/busybox", "httpd", "-f", "-p", "8080", "-h", site_root],
            "WorkingDir": site_root,
            "Labels": {
                "org.opencontainers.image.title": id,
                "org.opencontainers.image.description": "Static web app built in Goblins OS Build Studio",
                "io.goblins-os.studio.workspace-sha256": workspace_sha256,
                "io.goblins-os.studio.network-build": "none"
            },
            "StopSignal": "SIGTERM"
        },
        "rootfs": {
            "type": "layers",
            "diff_ids": [format!("sha256:{layer_digest}")]
        },
        "history": [{
            "created_by": "Goblins OS Build Studio deterministic offline packager"
        }]
    }))
    .map_err(|_| "The OCI image configuration could not be encoded.".to_string())?;
    let config_digest = digest_hex(&config);

    let manifest = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.manifest.v1+json",
        "config": {
            "mediaType": "application/vnd.oci.image.config.v1+json",
            "digest": format!("sha256:{config_digest}"),
            "size": config.len()
        },
        "layers": [{
            "mediaType": "application/vnd.oci.image.layer.v1.tar",
            "digest": format!("sha256:{layer_digest}"),
            "size": layer.len()
        }]
    }))
    .map_err(|_| "The OCI image manifest could not be encoded.".to_string())?;
    let manifest_digest = digest_hex(&manifest);

    let index = serde_json::to_vec(&json!({
        "schemaVersion": 2,
        "mediaType": "application/vnd.oci.image.index.v1+json",
        "manifests": [{
            "mediaType": "application/vnd.oci.image.manifest.v1+json",
            "digest": format!("sha256:{manifest_digest}"),
            "size": manifest.len(),
            "platform": { "architecture": runtime.architecture, "os": "linux" },
            "annotations": { "org.opencontainers.image.ref.name": image_ref }
        }]
    }))
    .map_err(|_| "The OCI image index could not be encoded.".to_string())?;

    let mut archive_files = BTreeMap::new();
    archive_files.insert(
        "oci-layout".to_string(),
        br#"{"imageLayoutVersion":"1.0.0"}"#.to_vec(),
    );
    archive_files.insert("index.json".to_string(), index);
    archive_files.insert(format!("blobs/sha256/{config_digest}"), config);
    archive_files.insert(format!("blobs/sha256/{layer_digest}"), layer);
    archive_files.insert(format!("blobs/sha256/{manifest_digest}"), manifest);
    let bytes = deterministic_tar(&archive_files)?;
    if bytes.len() > MAX_OCI_ARCHIVE_BYTES {
        return Err("The OCI image exceeds the bounded 20 MiB Studio export path.".to_string());
    }
    Ok(StudioContainerArchive {
        sha256: digest_hex(&bytes),
        bytes,
        image_ref,
    })
}

fn deterministic_layer(files: &BTreeMap<String, (Vec<u8>, u32)>) -> Result<Vec<u8>, String> {
    let output = Vec::new();
    let mut archive = tar::Builder::new(output);
    archive.mode(tar::HeaderMode::Deterministic);
    let mut directories = BTreeSet::new();
    for path in files.keys() {
        let mut parent = Path::new(path).parent();
        while let Some(directory) = parent {
            if directory.as_os_str().is_empty() {
                break;
            }
            directories.insert(directory.to_string_lossy().into_owned());
            parent = directory.parent();
        }
    }
    for directory in directories {
        append_tar_directory(&mut archive, &directory)?;
    }
    for (path, (content, mode)) in files {
        append_tar_file(&mut archive, path, content, *mode)?;
    }
    finish_tar(archive)
}

fn deterministic_tar(files: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let output = Vec::new();
    let mut archive = tar::Builder::new(output);
    archive.mode(tar::HeaderMode::Deterministic);
    append_tar_directory(&mut archive, "blobs")?;
    append_tar_directory(&mut archive, "blobs/sha256")?;
    for (path, content) in files {
        append_tar_file(&mut archive, path, content, 0o644)?;
    }
    finish_tar(archive)
}

fn append_tar_directory(archive: &mut tar::Builder<Vec<u8>>, path: &str) -> Result<(), String> {
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Directory);
    header.set_size(0);
    header.set_mode(0o755);
    normalized_header(&mut header);
    archive
        .append_data(&mut header, format!("{path}/"), Cursor::new([]))
        .map_err(|_| "The deterministic OCI directory could not be written.".to_string())
}

fn append_tar_file(
    archive: &mut tar::Builder<Vec<u8>>,
    path: &str,
    content: &[u8],
    mode: u32,
) -> Result<(), String> {
    let mut header = tar::Header::new_gnu();
    header.set_size(content.len() as u64);
    header.set_mode(mode);
    normalized_header(&mut header);
    archive
        .append_data(&mut header, path, Cursor::new(content))
        .map_err(|_| "The deterministic OCI file could not be written.".to_string())
}

fn normalized_header(header: &mut tar::Header) {
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
}

fn finish_tar(mut archive: tar::Builder<Vec<u8>>) -> Result<Vec<u8>, String> {
    archive
        .finish()
        .map_err(|_| "The deterministic OCI archive could not be finalized.".to_string())?;
    archive
        .into_inner()
        .map_err(|_| "The deterministic OCI archive could not be finalized.".to_string())
}

fn trusted_runtime_asset_ready(path: &str, executable: bool, max_bytes: usize) -> bool {
    trusted_runtime_metadata(path, executable, max_bytes).is_some()
}

fn busybox_source_rpm() -> Option<PathBuf> {
    let directory = fs::read_dir(BUSYBOX_SOURCE_DIR).ok()?;
    let mut source = None;
    for entry in directory {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let name = name.to_str()?;
        if !name.starts_with("busybox-") || !name.ends_with(".src.rpm") {
            return None;
        }
        if source.replace(entry.path()).is_some() {
            return None;
        }
    }
    source
}

fn trusted_runtime_metadata(
    path: &str,
    executable: bool,
    max_bytes: usize,
) -> Option<fs::Metadata> {
    let metadata = fs::symlink_metadata(path).ok()?;
    if !metadata.is_file()
        || metadata.uid() != 0
        || metadata.permissions().mode() & 0o022 != 0
        || metadata.len() == 0
        || metadata.len() > max_bytes as u64
        || (executable && metadata.permissions().mode() & 0o111 == 0)
    {
        return None;
    }
    Some(metadata)
}

fn read_trusted_runtime_asset(
    path: &str,
    executable: bool,
    max_bytes: usize,
) -> Result<Vec<u8>, ()> {
    let before = trusted_runtime_metadata(path, executable, max_bytes).ok_or(())?;
    let mut file = fs::File::open(path).map_err(|_| ())?;
    let opened = file.metadata().map_err(|_| ())?;
    if opened.dev() != before.dev()
        || opened.ino() != before.ino()
        || opened.len() != before.len()
        || opened.uid() != 0
    {
        return Err(());
    }
    let mut bytes = Vec::with_capacity(opened.len() as usize);
    file.by_ref()
        .take((max_bytes + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    let after = file.metadata().map_err(|_| ())?;
    if bytes.len() != opened.len() as usize
        || bytes.len() > max_bytes
        || after.dev() != opened.dev()
        || after.ino() != opened.ino()
        || after.len() != opened.len()
    {
        return Err(());
    }
    Ok(bytes)
}

fn digest_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;

    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        let _ = std::write!(encoded, "{byte:02x}");
    }
    encoded
}

fn oci_architecture() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "amd64",
        architecture => architecture,
    }
}

#[cfg(test)]
mod tests {
    use super::{build_container_from_parts, digest_hex, ContainerRuntimeAssets};
    use std::{collections::BTreeMap, io::Read};

    #[test]
    fn oci_static_container_is_deterministic_and_digest_linked() {
        let mut files = BTreeMap::new();
        files.insert(
            "assets/app.js".to_string(),
            b"document.body.textContent='ok';\n".to_vec(),
        );
        files.insert(
            "index.html".to_string(),
            b"<script src=\"assets/app.js\"></script>\n".to_vec(),
        );
        let workspace_digest = digest_hex(b"workspace");
        let first = build_container_from_parts(
            "demo",
            &workspace_digest,
            "index.html",
            files.clone(),
            ContainerRuntimeAssets {
                binary: b"fixed-busybox",
                license: b"GPL license",
                source: b"signed source rpm",
                architecture: "arm64",
            },
        )
        .expect("first OCI archive");
        let second = build_container_from_parts(
            "demo",
            &workspace_digest,
            "index.html",
            files,
            ContainerRuntimeAssets {
                binary: b"fixed-busybox",
                license: b"GPL license",
                source: b"signed source rpm",
                architecture: "arm64",
            },
        )
        .expect("second OCI archive");
        assert_eq!(first, second);
        assert_eq!(first.sha256, digest_hex(&first.bytes));
        assert_eq!(first.image_ref, "localhost/goblins-studio-demo:latest");

        let mut outer = tar::Archive::new(first.bytes.as_slice());
        let mut names = Vec::new();
        let mut blobs = BTreeMap::new();
        for entry in outer.entries().expect("outer entries") {
            let mut entry = entry.expect("outer entry");
            let name = entry
                .path()
                .expect("outer path")
                .to_string_lossy()
                .into_owned();
            names.push(name.clone());
            if entry.header().entry_type().is_file() && name.starts_with("blobs/sha256/") {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).expect("blob bytes");
                assert_eq!(name.rsplit('/').next(), Some(digest_hex(&bytes).as_str()));
                blobs.insert(name, bytes);
            }
            assert_eq!(entry.header().mtime().expect("mtime"), 0);
            assert_eq!(entry.header().uid().expect("uid"), 0);
            assert_eq!(entry.header().gid().expect("gid"), 0);
        }
        assert!(names.contains(&"oci-layout".to_string()));
        assert!(names.contains(&"index.json".to_string()));
        assert!(blobs.len() >= 3);

        let outer_files = tar_files(&first.bytes);
        let index: serde_json::Value =
            serde_json::from_slice(&outer_files["index.json"]).expect("OCI index JSON");
        let manifest_digest = index["manifests"][0]["digest"]
            .as_str()
            .expect("manifest digest")
            .strip_prefix("sha256:")
            .expect("manifest SHA-256");
        let manifest: serde_json::Value =
            serde_json::from_slice(&outer_files[&format!("blobs/sha256/{manifest_digest}")])
                .expect("OCI manifest JSON");
        let config_digest = manifest["config"]["digest"]
            .as_str()
            .expect("config digest")
            .strip_prefix("sha256:")
            .expect("config SHA-256");
        let config: serde_json::Value =
            serde_json::from_slice(&outer_files[&format!("blobs/sha256/{config_digest}")])
                .expect("OCI config JSON");
        let labels = config["config"]["Labels"].as_object().expect("OCI labels");
        assert_eq!(
            labels["io.goblins-os.studio.workspace-sha256"],
            workspace_digest
        );
        assert!(
            !labels.contains_key("org.opencontainers.image.licenses"),
            "the packager must not claim a license for user project files"
        );
        let layer_digest = manifest["layers"][0]["digest"]
            .as_str()
            .expect("layer digest")
            .strip_prefix("sha256:")
            .expect("layer SHA-256");
        let layer_files = tar_files(&outer_files[&format!("blobs/sha256/{layer_digest}")]);
        for required in [
            "bin/busybox",
            "usr/share/licenses/busybox/LICENSE",
            "usr/share/licenses/busybox/SOURCE",
            "usr/share/source/busybox/busybox.src.rpm",
            "workspace/index.html",
            "workspace/assets/app.js",
        ] {
            assert!(layer_files.contains_key(required), "missing {required}");
        }
    }

    #[test]
    fn oci_container_rejects_non_static_and_unbound_inputs() {
        let digest = digest_hex(b"workspace");
        let files = BTreeMap::from([("main.py".to_string(), b"print('no')\n".to_vec())]);
        assert!(build_container_from_parts(
            "demo",
            &digest,
            "main.py",
            files,
            ContainerRuntimeAssets {
                binary: b"busybox",
                license: b"license",
                source: b"source",
                architecture: "arm64",
            },
        )
        .is_err());
        assert!(build_container_from_parts(
            "demo",
            "not-a-digest",
            "index.html",
            BTreeMap::from([("index.html".to_string(), b"ok".to_vec())]),
            ContainerRuntimeAssets {
                binary: b"busybox",
                license: b"license",
                source: b"source",
                architecture: "arm64",
            },
        )
        .is_err());
    }

    fn tar_files(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
        let mut files = BTreeMap::new();
        let mut archive = tar::Archive::new(bytes);
        for entry in archive.entries().expect("tar entries") {
            let mut entry = entry.expect("tar entry");
            if !entry.header().entry_type().is_file() {
                continue;
            }
            let path = entry
                .path()
                .expect("tar path")
                .to_string_lossy()
                .into_owned();
            let mut content = Vec::new();
            entry.read_to_end(&mut content).expect("tar content");
            files.insert(path, content);
        }
        files
    }
}
