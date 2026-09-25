//! HMAC-SHA256, random secrets and the per-install admin secret (`Data/temp/admin.secret`).

use once_cell::sync::Lazy;
use rand::distributions::Alphanumeric;
use rand::rngs::OsRng;
use rand::{Rng, RngCore};
use sha2::{Digest, Sha256};

/// File holding the random key used for gate cookies and session fingerprints.
/// Created once; deleting it invalidates every gate cookie.
pub const SECRET_PATH: &str = "Data/temp/admin.secret";

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let inner = Sha256::new().chain_update(ipad).chain_update(msg).finalize();
    Sha256::new().chain_update(opad).chain_update(inner).finalize().into()
}

/// Constant-time byte comparison (length leak only).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `[A-Za-z0-9]{len}` from the OS RNG.
pub fn random_alnum(len: usize) -> String {
    OsRng.sample_iter(&Alphanumeric).take(len).map(char::from).collect()
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    OsRng.fill_bytes(&mut b);
    b
}

/// Read the secret file or create it (0600). Falls back to a per-process key when the file
/// cannot be written; gate cookies then reset on restart.
fn load_or_create_secret(path: &str) -> Vec<u8> {
    if let Ok(text) = std::fs::read_to_string(path) {
        if let Ok(b) = hex::decode(text.trim()) {
            if b.len() >= 32 {
                return b;
            }
        }
    }
    let fresh = random_bytes::<32>().to_vec();
    let write = || -> std::io::Result<()> {
        if let Some(dir) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, hex::encode(&fresh))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    };
    if let Err(e) = write() {
        crab_core::log::warn("admin", format!("{path}: {e}; admin cookies will reset on restart"));
    }
    fresh
}

static SECRET: Lazy<Vec<u8>> = Lazy::new(|| {
    if cfg!(test) {
        b"crabindex-test-admin-secret-0123456789".to_vec()
    } else {
        load_or_create_secret(SECRET_PATH)
    }
});

pub fn secret() -> &'static [u8] {
    &SECRET
}

/// Gate cookie value for an admin path + entry token.
pub fn gate_value(path: &str, token: &str) -> String {
    hex::encode(hmac_sha256(secret(), format!("gate\n{path}\n{token}").as_bytes()))
}

/// Keyed fingerprint of the devkey (sessions die when the devkey changes).
pub fn devkey_fingerprint(devkey: &str) -> [u8; 32] {
    hmac_sha256(secret(), format!("devkey\n{devkey}").as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4231_vectors() {
        assert_eq!(
            hex::encode(hmac_sha256(b"Jefe", b"what do ya want for nothing?")),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        // Test case 6: key longer than the block size.
        assert_eq!(
            hex::encode(hmac_sha256(&[0xaa; 131], b"Test Using Larger Than Block-Size Key - Hash Key First")),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
    }

    #[test]
    fn random_strings() {
        let t = random_alnum(18);
        assert_eq!(t.len(), 18);
        assert!(t.bytes().all(|b| b.is_ascii_alphanumeric()));
        assert_ne!(random_alnum(32), random_alnum(32));
    }

    #[test]
    fn secret_file_is_created_once() {
        let dir = std::env::temp_dir().join(format!("crab-admin-secret-{}", std::process::id()));
        let p = dir.join("temp/admin.secret");
        let p = p.to_str().unwrap();
        let a = load_or_create_secret(p);
        let b = load_or_create_secret(p);
        assert_eq!(a, b);
        assert_eq!(a.len(), 32);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn gate_depends_on_path_and_token() {
        assert_eq!(gate_value("/admin", "t"), gate_value("/admin", "t"));
        assert_ne!(gate_value("/admin", "t"), gate_value("/admin", "u"));
        assert_ne!(gate_value("/admin", "t"), gate_value("/panel", "t"));
        assert!(ct_eq(b"ab", b"ab") && !ct_eq(b"ab", b"ac") && !ct_eq(b"ab", b"abc"));
    }
}
