use sha2::{Digest, Sha256};
use std::io::Read;

pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const RELEASE_BASE: &str = "https://github.com/chiploom/zed-wit/releases/download";
pub const MAX_BINARY_BYTES: u64 = 128 * 1024 * 1024;

pub fn asset_name(target: &str) -> Result<String, String> {
    let suffix = match target {
        "aarch64-apple-darwin"
        | "x86_64-apple-darwin"
        | "aarch64-unknown-linux-gnu"
        | "x86_64-unknown-linux-gnu" => "",
        "x86_64-pc-windows-msvc" => ".exe",
        _ => return Err(format!("No WIT language server release for {target}")),
    };
    Ok(format!("wit-language-server-{target}{suffix}"))
}

pub fn release_url(asset: &str) -> String {
    format!("{RELEASE_BASE}/v{SERVER_VERSION}/{asset}")
}

pub fn parse_checksum(text: &str, asset: &str) -> Result<[u8; 32], String> {
    let mut fields = text.split_whitespace();
    let hash = fields.next().ok_or("Empty release checksum")?;
    let name = fields.next().ok_or("Missing release checksum filename")?;
    if fields.next().is_some() || name.strip_prefix('*').unwrap_or(name) != asset {
        return Err("Release checksum does not identify the expected server asset".into());
    }
    if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Release checksum must be a 64-digit SHA-256 digest".into());
    }
    let mut result = [0; 32];
    for (index, value) in result.iter_mut().enumerate() {
        *value = u8::from_str_radix(&hash[index * 2..index * 2 + 2], 16)
            .map_err(|e| format!("Invalid SHA-256 checksum: {e}"))?;
    }
    Ok(result)
}

pub fn verify_binary(mut reader: impl Read, expected: &[u8; 32]) -> Result<(), String> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    let mut total = 0_u64;
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|e| format!("Read server: {e}"))?;
        if count == 0 {
            break;
        }
        total += count as u64;
        if total > MAX_BINARY_BYTES {
            return Err("Server exceeds the 128 MiB release size limit".into());
        }
        hash.update(&buffer[..count]);
    }
    if total == 0 || hash.finalize().as_slice() != expected {
        return Err(
            "WIT server SHA-256 verification failed".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_names_are_fixed_and_versioned() {
        for target in [
            "aarch64-apple-darwin",
            "x86_64-apple-darwin",
            "aarch64-unknown-linux-gnu",
            "x86_64-unknown-linux-gnu",
            "x86_64-pc-windows-msvc",
        ] {
            let asset = asset_name(target).unwrap();
            assert!(
                release_url(&asset).contains(&format!("/v{SERVER_VERSION}/wit-language-server-"))
            );
            assert_eq!(asset.ends_with(".exe"), target.contains("windows"));
        }
        assert!(asset_name("../../other").is_err());
    }

    #[test]
    fn verifies_bytes_and_rejects_corruption() {
        let asset = "server";
        let digest: String = Sha256::digest(b"server bytes")
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let expected = parse_checksum(&format!("{digest}  {asset}\n"), asset).unwrap();
        verify_binary(&b"server bytes"[..], &expected).unwrap();
        assert!(verify_binary(&b"changed"[..], &expected).is_err());
        assert!(verify_binary(&b""[..], &expected).is_err());
        for text in [
            "",
            "abc server",
            &format!("{digest} other"),
            &format!("{digest} server\n{digest} server"),
            &format!("{} server", "é".repeat(32)),
        ] {
            assert!(parse_checksum(text, asset).is_err());
        }
    }

    #[test]
    fn propagates_read_errors() {
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("disk error"))
            }
        }
        assert!(
            verify_binary(Broken, &[0; 32])
                .unwrap_err()
                .contains("disk error")
        );
    }
}
