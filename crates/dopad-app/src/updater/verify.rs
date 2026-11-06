use super::asset::SignedManifest;
use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};

pub(crate) fn embedded_key() -> anyhow::Result<VerifyingKey> {
    let encoded = option_env!("DOPAD_UPDATE_PUBLIC_KEY")
        .ok_or_else(|| anyhow::anyhow!("update signing key is not configured in this build"))?;
    let bytes = decode_hex(encoded)?;
    let key_bytes: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("embedded update public key must be 32 bytes"))?;
    let key = VerifyingKey::from_bytes(&key_bytes)?;
    anyhow::ensure!(!key.is_weak(), "embedded update public key is invalid");
    Ok(key)
}

pub(crate) fn verify_manifest(
    key: &VerifyingKey,
    manifest_bytes: &[u8],
    signature_base64: &str,
) -> anyhow::Result<SignedManifest> {
    anyhow::ensure!(
        manifest_bytes.len() <= 64 * 1024,
        "update manifest exceeds the size limit"
    );
    let signature_bytes = base64::engine::general_purpose::STANDARD
        .decode(signature_base64.trim())
        .map_err(|error| anyhow::anyhow!("update manifest signature is malformed: {error}"))?;
    let signature = Signature::from_slice(&signature_bytes)?;
    key.verify_strict(manifest_bytes, &signature)
        .map_err(|_| anyhow::anyhow!("update manifest signature could not be verified"))?;
    let manifest: SignedManifest = serde_json::from_slice(manifest_bytes)?;
    anyhow::ensure!(
        manifest.sha256.len() == 64,
        "manifest SHA-256 digest is malformed"
    );
    anyhow::ensure!(
        manifest.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()),
        "manifest SHA-256 digest is malformed"
    );
    Ok(manifest)
}

fn decode_hex(value: &str) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        value.len() == 64,
        "update public key must be 64 hexadecimal characters"
    );
    (0..value.len())
        .step_by(2)
        .map(|index| {
            u8::from_str_radix(&value[index..index + 2], 16)
                .map_err(|error| anyhow::anyhow!("update public key is malformed: {error}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    #[test]
    fn verifies_ed25519_manifest_before_deserializing_it() {
        let signing_key = SigningKey::from_bytes(&[7; 32]);
        let manifest = br#"{"version":"1.2.3","target":"windows-x86_64","asset_name":"dopad-v1.2.3-windows-x86_64-setup.exe","size":42,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#;
        let signature = signing_key.sign(manifest);
        let encoded = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());

        assert!(verify_manifest(&signing_key.verifying_key(), manifest, &encoded).is_ok());
        assert!(verify_manifest(&signing_key.verifying_key(), manifest, "bad signature").is_err());
    }
}
