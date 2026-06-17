use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsStr,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use async_trait::async_trait;
use harness_contracts::ManifestValidationFailure as EventManifestValidationFailure;
use ring::digest;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    DiscoverySource, ManifestLoadReport, ManifestLoaderError, ManifestOrigin, ManifestRecord,
    ManifestSigner, ManifestValidationFailure, Plugin, PluginActivationContext,
    PluginActivationResult, PluginError, PluginManifest, PluginManifestLoader, PluginRuntimeLoader,
    RuntimeLoaderError,
};

const DEFAULT_METADATA_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_RUNTIME_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
pub struct CargoExtensionManifestLoader {
    search_paths: Option<Vec<PathBuf>>,
    timeout: Duration,
}

impl Default for CargoExtensionManifestLoader {
    fn default() -> Self {
        Self {
            search_paths: None,
            timeout: DEFAULT_METADATA_TIMEOUT,
        }
    }
}

impl CargoExtensionManifestLoader {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_search_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.search_paths = Some(paths);
        self
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    fn paths(&self) -> Vec<PathBuf> {
        self.search_paths.clone().unwrap_or_else(|| {
            env::var_os("PATH")
                .map(|path| env::split_paths(&path).collect())
                .unwrap_or_default()
        })
    }
}

#[async_trait]
impl PluginManifestLoader for CargoExtensionManifestLoader {
    async fn enumerate(
        &self,
        source: &DiscoverySource,
    ) -> Result<Vec<ManifestRecord>, ManifestLoaderError> {
        self.load_report(source).await.map(|report| report.records)
    }

    async fn load_report(
        &self,
        source: &DiscoverySource,
    ) -> Result<ManifestLoadReport, ManifestLoaderError> {
        if !matches!(source, DiscoverySource::CargoExtension) {
            return Ok(ManifestLoadReport::default());
        }

        let mut report = ManifestLoadReport::default();
        for binary in discover_cargo_extension_binaries(&self.paths())? {
            let output =
                run_extension_command(&binary, &["--harness-manifest"], None, self.timeout).await;
            match output {
                Ok(output) if output.status_success => {
                    match decode_manifest_metadata(&binary, &output.stdout) {
                        Ok(record) => report.records.push(record),
                        Err(failure) => report.failures.push(failure),
                    }
                }
                Ok(output) => report.failures.push(cargo_extension_failure(
                    binary,
                    output.stdout,
                    format!("metadata command exited with status {}", output.status_code),
                    None,
                    None,
                )),
                Err(details) => report.failures.push(cargo_extension_failure(
                    binary,
                    Vec::new(),
                    details,
                    None,
                    None,
                )),
            }
        }

        Ok(report)
    }
}

#[derive(Debug, Clone)]
pub struct CargoExtensionRuntimeLoader {
    timeout: Duration,
}

impl Default for CargoExtensionRuntimeLoader {
    fn default() -> Self {
        Self {
            timeout: DEFAULT_RUNTIME_TIMEOUT,
        }
    }
}

impl CargoExtensionRuntimeLoader {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

#[async_trait]
impl PluginRuntimeLoader for CargoExtensionRuntimeLoader {
    fn can_load(&self, _manifest: &PluginManifest, origin: &ManifestOrigin) -> bool {
        matches!(origin, ManifestOrigin::CargoExtension { .. })
    }

    async fn load(
        &self,
        manifest: &PluginManifest,
        origin: &ManifestOrigin,
    ) -> Result<Arc<dyn Plugin>, RuntimeLoaderError> {
        let ManifestOrigin::CargoExtension { binary, .. } = origin else {
            return Err(RuntimeLoaderError::UnsupportedOrigin(origin.to_string()));
        };

        Ok(Arc::new(CargoExtensionPlugin {
            manifest: manifest.clone(),
            binary: binary.clone(),
            timeout: self.timeout,
        }))
    }
}

struct CargoExtensionPlugin {
    manifest: PluginManifest,
    binary: PathBuf,
    timeout: Duration,
}

#[async_trait]
impl Plugin for CargoExtensionPlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    async fn activate(
        &self,
        ctx: PluginActivationContext,
    ) -> Result<PluginActivationResult, PluginError> {
        let result = self
            .call_runtime(
                "activate",
                json!({
                    "trust_level": ctx.trust_level,
                    "plugin_id": ctx.plugin_id,
                    "config": ctx.config,
                    "workspace_root": ctx.workspace_root,
                }),
            )
            .await
            .map_err(PluginError::ActivateFailed)?;
        if result.is_null() {
            return Ok(PluginActivationResult::default());
        }
        serde_json::from_value(result)
            .map_err(|error| PluginError::ActivateFailed(error.to_string()))
    }

    async fn deactivate(&self) -> Result<(), PluginError> {
        self.call_runtime("deactivate", Value::Null)
            .await
            .map(|_| ())
            .map_err(PluginError::DeactivateFailed)
    }
}

impl CargoExtensionPlugin {
    async fn call_runtime(&self, method: &str, params: Value) -> Result<Value, String> {
        let request = CargoExtensionRpcRequest {
            jsonrpc: "2.0",
            id: 1,
            method,
            params,
        };
        let input = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        let output = run_extension_command(
            &self.binary,
            &["--harness-runtime"],
            Some(input),
            self.timeout,
        )
        .await?;
        if !output.status_success {
            return Err(format!("runtime exited with status {}", output.status_code));
        }
        let response: CargoExtensionRpcResponse =
            serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
        if let Some(error) = response.error {
            return Err(error.message);
        }
        Ok(response.result.unwrap_or(Value::Null))
    }
}

#[derive(Serialize)]
struct CargoExtensionRpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: Value,
}

#[derive(Deserialize)]
struct CargoExtensionRpcResponse {
    result: Option<Value>,
    error: Option<CargoExtensionRpcError>,
}

#[derive(Deserialize)]
struct CargoExtensionRpcError {
    message: String,
}

struct CommandOutput {
    stdout: Vec<u8>,
    status_success: bool,
    status_code: String,
}

async fn run_extension_command(
    binary: &Path,
    args: &[&str],
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    let binary = binary.to_path_buf();
    let args = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    tokio::task::spawn_blocking(move || {
        run_extension_command_blocking(&binary, &args, input, timeout)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn run_extension_command_blocking(
    binary: &Path,
    args: &[String],
    input: Option<Vec<u8>>,
    timeout: Duration,
) -> Result<CommandOutput, String> {
    let mut child = Command::new(binary)
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("spawn failed: {error}"))?;

    if let Some(input) = input {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "runtime stdin unavailable".to_owned())?;
        stdin
            .write_all(&input)
            .map_err(|error| format!("runtime stdin write failed: {error}"))?;
    }

    let started = Instant::now();
    loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("wait failed: {error}"))?
        {
            let output = child
                .wait_with_output()
                .map_err(|error| format!("read output failed: {error}"))?;
            return Ok(CommandOutput {
                stdout: output.stdout,
                status_success: status.success(),
                status_code: status
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| "signal".to_owned()),
            });
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("timed out after {} ms", timeout.as_millis()));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn discover_cargo_extension_binaries(
    paths: &[PathBuf],
) -> Result<Vec<PathBuf>, ManifestLoaderError> {
    let mut binaries = BTreeSet::new();
    for path in paths {
        let entries = match fs::read_dir(path) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(ManifestLoaderError::Io(error.to_string())),
        };
        for entry in entries {
            let entry = entry.map_err(|error| ManifestLoaderError::Io(error.to_string()))?;
            let file_name = entry.file_name();
            if is_cargo_extension_name(&file_name) && is_executable(&entry.path()) {
                binaries.insert(entry.path());
            }
        }
    }
    Ok(binaries.into_iter().collect())
}

fn is_cargo_extension_name(name: &OsStr) -> bool {
    name.to_string_lossy().starts_with("jyowo-plugin-")
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .map(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.is_file())
        .unwrap_or(false)
}

fn decode_manifest_metadata(
    binary: &Path,
    bytes: &[u8],
) -> Result<ManifestRecord, ManifestValidationFailure> {
    let raw_hash = sha256(bytes);
    let value = serde_json::from_slice::<Value>(bytes).map_err(|error| {
        cargo_extension_failure(
            binary.to_path_buf(),
            bytes.to_vec(),
            format!("metadata json parse failed: {error}"),
            None,
            None,
        )
    })?;
    let package_metadata = value
        .get("package_metadata")
        .and_then(Value::as_object)
        .map(|metadata| {
            metadata
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let manifest_value = value.get("manifest").cloned().unwrap_or(value);
    let manifest = serde_json::from_value::<PluginManifest>(manifest_value).map_err(|error| {
        cargo_extension_failure(
            binary.to_path_buf(),
            bytes.to_vec(),
            format!("metadata manifest decode failed: {error}"),
            None,
            None,
        )
    })?;
    let origin = ManifestOrigin::CargoExtension {
        binary: binary.to_path_buf(),
        package_metadata,
    };
    let canonical = ManifestSigner::canonical_payload(&manifest).map_err(|error| {
        cargo_extension_failure(
            binary.to_path_buf(),
            bytes.to_vec(),
            format!("metadata manifest canonicalization failed: {error}"),
            Some(manifest.name.to_string()),
            Some(manifest.version.to_string()),
        )
    })?;
    ManifestRecord::new(manifest.clone(), origin, sha256(&canonical))
        .map_err(|error| {
            cargo_extension_failure(
                binary.to_path_buf(),
                bytes.to_vec(),
                format!("metadata manifest validation failed: {error}"),
                Some(manifest.name.to_string()),
                Some(manifest.version.to_string()),
            )
        })
        .map_err(|mut failure| {
            failure.raw_bytes_hash = raw_hash;
            failure
        })
}

fn cargo_extension_failure(
    binary: PathBuf,
    bytes: Vec<u8>,
    details: String,
    partial_name: Option<String>,
    partial_version: Option<String>,
) -> ManifestValidationFailure {
    ManifestValidationFailure {
        origin: Some(ManifestOrigin::CargoExtension {
            binary,
            package_metadata: BTreeMap::new(),
        }),
        partial_name,
        partial_version,
        raw_bytes_hash: sha256(&bytes),
        failure: EventManifestValidationFailure::CargoExtensionMetadataMalformed {
            details: details.clone(),
        },
        details,
    }
}

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let digest = digest::digest(&digest::SHA256, bytes);
    let mut output = [0_u8; 32];
    output.copy_from_slice(digest.as_ref());
    output
}
