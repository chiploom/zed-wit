mod distribution;

use distribution::{SERVER_VERSION, asset_name, parse_checksum, release_url, verify_binary};
use std::{fs, io::Read, path::Path};
use zed_extension_api::{self as zed, LanguageServerId, settings::LspSettings};

struct WitExtension;

fn platform_target(os: zed::Os, arch: zed::Architecture) -> Result<&'static str, String> {
    match (os, arch) {
        (zed::Os::Mac, zed::Architecture::Aarch64) => Ok("aarch64-apple-darwin"),
        (zed::Os::Mac, zed::Architecture::X8664) => Ok("x86_64-apple-darwin"),
        (zed::Os::Linux, zed::Architecture::Aarch64) => Ok("aarch64-unknown-linux-gnu"),
        (zed::Os::Linux, zed::Architecture::X8664) => Ok("x86_64-unknown-linux-gnu"),
        (zed::Os::Windows, zed::Architecture::X8664) => Ok("x86_64-pc-windows-msvc"),
        _ => Err("WIT server supports macOS/Linux ARM64 and x86_64, and Windows x86_64. Configure lsp.wit-language-server.binary.path for another platform.".into()),
    }
}

fn read_checksum(path: &Path, asset: &str) -> Result<[u8; 32], String> {
    let file = fs::File::open(path).map_err(|e| format!("Open {}: {e}", path.display()))?;
    let mut text = String::new();
    file.take(1025)
        .read_to_string(&mut text)
        .map_err(|e| format!("Read checksum: {e}"))?;
    if text.len() > 1024 {
        return Err("Release checksum is unexpectedly large".into());
    }
    parse_checksum(&text, asset)
}

fn verify_cached_server(binary_path: &Path, checksum_path: &Path, asset: &str) -> Result<(), String> {
    let expected = read_checksum(checksum_path, asset)?;
    verify_binary(
        fs::File::open(binary_path)
            .map_err(|e| format!("Open cached server {}: {e}", binary_path.display()))?,
        &expected,
    )
}

fn cache_is_valid(binary_path: &Path, checksum_path: &Path, asset: &str) -> bool {
    binary_path.is_file()
        && checksum_path.is_file()
        && verify_cached_server(binary_path, checksum_path, asset).is_ok()
}

fn remove_file_if_exists(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("Remove {}: {error}", path.display())),
    }
}

impl WitExtension {
    fn install_server(&self, id: &LanguageServerId) -> zed::Result<String> {
        let (os, arch) = zed::current_platform();
        let target = platform_target(os, arch)?;
        let asset = asset_name(target)?;
        let directory = format!("wit-language-server-{SERVER_VERSION}-{target}");
        let binary_path = Path::new(&directory).join(&asset);
        let checksum_path = Path::new(&directory).join(format!("{asset}.sha256"));
        let binary_staging = Path::new(&directory).join(format!("{asset}.download"));
        let checksum_staging = Path::new(&directory).join(format!("{asset}.sha256.download"));
        fs::create_dir_all(&directory).map_err(|e| format!("Create server cache: {e}"))?;

        if !cache_is_valid(&binary_path, &checksum_path, &asset) {
            zed::set_language_server_installation_status(
                id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );
            let download = || -> zed::Result<()> {
                for path in [
                    &binary_staging,
                    &checksum_staging,
                    &binary_path,
                    &checksum_path,
                ] {
                    remove_file_if_exists(path)?;
                }

                zed::download_file(
                    &release_url(&format!("{asset}.sha256")),
                    &checksum_staging.to_string_lossy(),
                    zed::DownloadedFileType::Uncompressed,
                )?;
                // Validate metadata before accepting a potentially expensive binary download.
                let expected = read_checksum(&checksum_staging, &asset)?;
                zed::download_file(
                    &release_url(&asset),
                    &binary_staging.to_string_lossy(),
                    zed::DownloadedFileType::Uncompressed,
                )?;
                verify_binary(
                    fs::File::open(&binary_staging)
                        .map_err(|e| format!("Open downloaded server: {e}"))?,
                    &expected,
                )?;

                fs::rename(&checksum_staging, &checksum_path)
                    .map_err(|e| format!("Install verified checksum: {e}"))?;
                fs::rename(&binary_staging, &binary_path)
                    .map_err(|e| format!("Install verified server: {e}"))?;
                Ok(())
            };
            if let Err(error) = download() {
                let _ = remove_file_if_exists(&binary_staging);
                let _ = remove_file_if_exists(&checksum_staging);
                let message = format!(
                    "Install WIT server v{SERVER_VERSION} for {target}: {error}. Before this version is released, build the server locally and set lsp.wit-language-server.binary.path (see README)."
                );
                zed::set_language_server_installation_status(
                    id,
                    &zed::LanguageServerInstallationStatus::Failed(message.clone()),
                );
                return Err(message);
            }
        }
        verify_cached_server(&binary_path, &checksum_path, &asset)?;
        zed::make_file_executable(&binary_path.to_string_lossy())?;
        zed::set_language_server_installation_status(
            id,
            &zed::LanguageServerInstallationStatus::None,
        );
        Ok(binary_path.to_string_lossy().into_owned())
    }
}

impl zed::Extension for WitExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        if id.as_ref() != "wit-language-server" {
            return Err(format!("Unknown WIT language server: {}", id.as_ref()));
        }
        let settings = LspSettings::for_worktree(id.as_ref(), worktree)?;
        let binary = settings.binary;
        let command = if let Some(path) = binary.as_ref().and_then(|b| b.path.as_ref()) {
            if path.is_empty() {
                return Err("lsp.wit-language-server.binary.path must not be empty".into());
            }
            path.clone()
        } else if let Some(path) = worktree.which("wit-language-server") {
            path
        } else {
            self.install_server(id)?
        };
        Ok(zed::Command {
            command,
            args: binary
                .as_ref()
                .and_then(|b| b.arguments.clone())
                .unwrap_or_default(),
            env: binary
                .and_then(|b| b.env)
                .map(|env| env.into_iter().collect())
                .unwrap_or_default(),
        })
    }
}

zed::register_extension!(WitExtension);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_mapping() {
        for (os, arch, target) in [
            (
                zed::Os::Mac,
                zed::Architecture::Aarch64,
                "aarch64-apple-darwin",
            ),
            (
                zed::Os::Mac,
                zed::Architecture::X8664,
                "x86_64-apple-darwin",
            ),
            (
                zed::Os::Linux,
                zed::Architecture::Aarch64,
                "aarch64-unknown-linux-gnu",
            ),
            (
                zed::Os::Linux,
                zed::Architecture::X8664,
                "x86_64-unknown-linux-gnu",
            ),
            (
                zed::Os::Windows,
                zed::Architecture::X8664,
                "x86_64-pc-windows-msvc",
            ),
        ] {
            assert_eq!(platform_target(os, arch).unwrap(), target);
        }
        assert!(platform_target(zed::Os::Windows, zed::Architecture::Aarch64).is_err());
        assert!(platform_target(zed::Os::Linux, zed::Architecture::X86).is_err());
    }

    #[test]
    fn corrupted_cache_is_treated_as_missing() {
        use sha2::{Digest, Sha256};
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!(
            "zed-wit-cache-test-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();

        let asset = "wit-language-server-test";
        let binary_path = directory.join(asset);
        let checksum_path = directory.join(format!("{asset}.sha256"));
        let bytes = b"verified server";
        let digest: String = Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();

        fs::write(&binary_path, bytes).unwrap();
        fs::write(&checksum_path, format!("{digest}  {asset}\n")).unwrap();
        assert!(cache_is_valid(&binary_path, &checksum_path, asset));

        fs::write(&binary_path, b"corrupted server").unwrap();
        assert!(!cache_is_valid(&binary_path, &checksum_path, asset));

        fs::write(&binary_path, bytes).unwrap();
        fs::write(&checksum_path, format!("{digest}  other\n")).unwrap();
        assert!(!cache_is_valid(&binary_path, &checksum_path, asset));

        fs::remove_file(&checksum_path).unwrap();
        assert!(!cache_is_valid(&binary_path, &checksum_path, asset));

        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn manifests_agree_on_distribution_identity() {
        let manifest: toml::Value = toml::from_str(include_str!("../extension.toml")).unwrap();
        assert_eq!(manifest["version"].as_str(), Some(SERVER_VERSION));
        assert_eq!(manifest["schema_version"].as_integer(), Some(1));
        assert_eq!(
            manifest["grammars"]["wit"]["rev"].as_str(),
            Some("cdf07263b136054b413cab449ac7a1d059c27542")
        );
        let native: toml::Value =
            toml::from_str(include_str!("../crates/wit-language-server/Cargo.toml")).unwrap();
        assert_eq!(native["package"]["version"].as_str(), Some(SERVER_VERSION));
    }
}
