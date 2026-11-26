//! Fernet decryption of `get_font_access` payloads (see docs/server-api.md).

use base64::Engine;
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::{Error, Result};

/// Decode urlsafe base64 the way Python's `urlsafe_b64decode` does for these payloads:
/// the server appends junk after the `=` padding, so cut at the first `=` (a base64 data
/// terminator) and decode up to it. Without a `=`, decode unpadded.
fn b64_url(s: &str) -> Option<Vec<u8>> {
    let t = s.trim();
    match t.find('=') {
        Some(i) => {
            // Keep the whole padding run (one or two '='); junk follows it.
            let b = t.as_bytes();
            let mut j = i;
            while b.get(j) == Some(&b'=') {
                j += 1;
            }
            base64::engine::general_purpose::URL_SAFE.decode(&t[..j]).ok()
        }
        None => base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(t).ok(),
    }
}

/// Byte candidates for a font blob. The server appends junk after the token: after the `=`
/// padding when there is one, and directly after the last data character when the token needs no
/// padding (then the length is not a multiple of 4). Without `=` the cut point is ambiguous, so
/// every cut that leaves a multiple of 4 characters (up to 16 junk chars) is offered; the HMAC
/// check picks the right one.
fn token_candidates(s: &str) -> Vec<Vec<u8>> {
    let t = s.trim();
    if t.contains('=') {
        return b64_url(t).into_iter().collect();
    }
    let n = t.len();
    (0..=16)
        .filter(|k| *k < n && (n - k).is_multiple_of(4))
        .filter_map(|k| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&t[..n - k]).ok())
        .collect()
}

/// Decrypt a Fernet token manually: version → HMAC-SHA256 verify → AES-128-CBC/PKCS7.
/// Timestamp is intentionally ignored (the server clock may run ahead of this machine).
/// Stage-specific errors make failures diagnosable.
pub fn fernet_decrypt(token: &str, key: &str) -> Result<Vec<u8>> {
    let kb = b64_url(key).ok_or_else(|| Error::Font("key: bad base64".into()))?;
    if kb.len() != 32 {
        return Err(Error::Font(format!("key: {} bytes, need 32", kb.len())));
    }
    let candidates = token_candidates(token);
    if candidates.is_empty() {
        return Err(Error::Font("token: bad base64".into()));
    }
    let mut last = Error::Font("token: bad base64".into());
    for tb in candidates {
        match fernet_open(&tb, &kb) {
            Ok(pt) => return Ok(pt),
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// Verify and decrypt one decoded Fernet token with a decoded 32-byte key.
fn fernet_open(tb: &[u8], kb: &[u8]) -> Result<Vec<u8>> {
    if tb.len() < 1 + 8 + 16 + 32 {
        return Err(Error::Font(format!("token: too short ({} bytes)", tb.len())));
    }
    if tb[0] != 0x80 {
        return Err(Error::Font(format!("token: version {:#x}, need 0x80", tb[0])));
    }
    let (sign_key, enc_key) = kb.split_at(16);
    let hmac_at = tb.len() - 32;

    let mut mac = Hmac::<Sha256>::new_from_slice(sign_key).expect("hmac any key len");
    mac.update(&tb[..hmac_at]);
    mac.verify_slice(&tb[hmac_at..])
        .map_err(|_| Error::Font("hmac mismatch (wrong key)".into()))?;

    let iv: [u8; 16] = tb[9..25].try_into().unwrap();
    let mut buf = tb[25..hmac_at].to_vec();
    if buf.is_empty() || !buf.len().is_multiple_of(16) {
        return Err(Error::Font(format!("ciphertext not block-aligned ({})", buf.len())));
    }
    type Dec = cbc::Decryptor<aes::Aes128>;
    let pt = Dec::new(enc_key.into(), (&iv).into())
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| Error::Font("aes/pkcs7 unpad failed".into()))?;
    Ok(pt.to_vec())
}

/// Non-secret description of a key token and font blob for decrypt-failure reports: lengths,
/// where `=` sits, and which characters fall outside the urlsafe-base64 alphabet. Never the
/// characters themselves.
pub fn shape(backup_token: &str, blob: &str) -> String {
    let eq = |s: &str| s.char_indices().filter(|(_, c)| *c == '=').map(|(i, _)| i).collect::<Vec<_>>();
    let odd = |s: &str| {
        let mut v: Vec<char> = s.chars().filter(|c| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '='))).collect();
        v.sort_unstable();
        v.dedup();
        v.into_iter().map(|c| format!("{:?}", c)).collect::<Vec<_>>().join(" ")
    };
    let tail_eq: Vec<usize> = eq(blob).into_iter().filter(|i| *i + 8 >= blob.len()).map(|i| blob.len() - i).collect();
    format!(
        "backup_token: {} chars, '=' at {:?}, non-b64 [{}]; blob: {} chars (mod 4 = {}), starts {:?}, '=' from end {:?}, non-b64 [{}]",
        backup_token.chars().count(),
        eq(backup_token),
        odd(backup_token),
        blob.len(),
        blob.len() % 4,
        blob.get(..4).unwrap_or(""),
        tail_eq,
        odd(blob),
    )
}
