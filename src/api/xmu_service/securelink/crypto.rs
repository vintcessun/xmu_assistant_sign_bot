//! SecureLink 的 sheeta 加密/签名原语，移植自 xmu_secure_link 的 `crypto.rs`。
//!
//! - 包体用随机 AES-128-CBC 密钥加密（IV 固定 `securelink666666`）；
//! - AES 密钥用服务端 RSA 公钥 PKCS#1 v1.5 加密后放进 `secret` 头；
//! - 请求签名 `sheetaSign = base64(upper(hex(sha1("certId-ts-nonce-body"))))`。

use aes::Aes128;
use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose};
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use rand_compat::{RngCore, rngs::OsRng};
use rsa::{Pkcs1v15Encrypt, RsaPublicKey, pkcs8::DecodePublicKey};
use sha1::{Digest, Sha1};

type Aes128CbcEnc = cbc::Encryptor<Aes128>;
type Aes128CbcDec = cbc::Decryptor<Aes128>;

pub const AUTH_RSA_PUBLIC_KEY_PEM: &str = "-----BEGIN PUBLIC KEY-----\n\
MIGfMA0GCSqGSIb3DQEBAQUAA4GNADCBiQKBgQC2nJO4nAbiAvMzRjKm9nq80A7P\n\
zFjY8fT0kaN+Cz5u0Tk8zhzX2TXMOf+YL16rNOtuKbNwzBQ2Swk53jy8ZKVQ1yYH\n\
O9T4YMkAS3Sz7mVJtZONGORdLISHyS9dqPDVfSuoU4ItiLki+X1aJ/JCWAF7nuPb\n\
UwKFC5Aepns2wGzk/wIDAQAB\n\
-----END PUBLIC KEY-----";

pub const BODY_AES_IV: &[u8; 16] = b"securelink666666";

pub fn random_aes_key() -> [u8; 16] {
    let mut key = [0u8; 16];
    OsRng.fill_bytes(&mut key);
    key
}

pub fn aes_128_cbc_encrypt(key: &[u8; 16], iv: &[u8; 16], plaintext: &[u8]) -> Vec<u8> {
    Aes128CbcEnc::new(key.into(), iv.into()).encrypt_padded_vec_mut::<Pkcs7>(plaintext)
}

pub fn aes_128_cbc_decrypt(key: &[u8; 16], iv: &[u8; 16], ciphertext: &[u8]) -> Result<Vec<u8>> {
    Aes128CbcDec::new(key.into(), iv.into())
        .decrypt_padded_vec_mut::<Pkcs7>(ciphertext)
        .map_err(|_| anyhow::anyhow!("AES-CBC decrypt failed"))
}

pub fn compute_sheeta_sign(cert_id: &str, req_timestamp: &str, nonce: &str, body: &str) -> String {
    let source = format!("{cert_id}-{req_timestamp}-{nonce}-{body}");
    let hex_digest = format!("{:x}", Sha1::digest(source.as_bytes()));
    general_purpose::STANDARD.encode(hex_digest).to_uppercase()
}

pub fn rsa_encrypt_base64(plaintext: &[u8]) -> Result<String> {
    let key = RsaPublicKey::from_public_key_pem(AUTH_RSA_PUBLIC_KEY_PEM)
        .context("parse SecureLink RSA public key")?;
    let encrypted = key
        .encrypt(&mut OsRng, Pkcs1v15Encrypt, plaintext)
        .context("RSA encrypt")?;
    Ok(general_purpose::STANDARD.encode(encrypted))
}

/// 返回 (base64 包体, 本次 AES 密钥, RSA 加密后的密钥 base64)。
pub fn encrypt_body(plain_json: &str) -> Result<(String, [u8; 16], String)> {
    let key = random_aes_key();
    let ciphertext = aes_128_cbc_encrypt(&key, BODY_AES_IV, plain_json.as_bytes());
    let body = general_purpose::STANDARD.encode(ciphertext);
    let secret = rsa_encrypt_base64(&key)?;
    Ok((body, key, secret))
}

pub fn decode_jwt_payload(token: &str) -> Result<serde_json::Value> {
    let payload = token.split('.').nth(1).context("token is not a JWT")?;
    let bytes = general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| general_purpose::URL_SAFE.decode(payload))?;
    Ok(serde_json::from_slice(&bytes)?)
}
