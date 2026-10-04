//! 容器加密金库（B-14）——XChaCha20-Poly1305 + Argon2id 口令派生。
//!
//! 约束（蓝图 756/758 行硬性条款）：
//! - XChaCha20 为流密码，**每条记录独立随机 24B nonce**，绝不复用；
//! - 密钥由口令 Argon2id 派生（m=19MiB, t=2, p=1），盐随机 16B 随容器持久化；
//! - 验证器 = key 的 BLAKE3 前 16B（非明文口令、非完整 key），解锁时恒时比较；
//! - 开启金库后 chunk 载荷、索引 blob、journal payload 一律为 `[nonce24][密文+tag]`。

use chacha20poly1305::aead::{Aead, KeyInit, Payload};

use crate::{CmdResult, ContainerError};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::RngCore;

pub const NONCE_LEN: usize = 24;
pub const SALT_LEN: usize = 16;
pub const VERIFIER_LEN: usize = 16;
/// Argon2id 参数（蓝图谱系默认档）。
const ARGON_M_KIB: u32 = 19456;
const ARGON_T: u32 = 2;
const ARGON_P: u32 = 1;

#[derive(Clone)]
pub struct VaultKey(Key);

#[derive(Clone)]
pub struct Vault {
    key: VaultKey,
    verifier: [u8; VERIFIER_LEN],
}

fn derive(passphrase: &[u8], salt: &[u8; SALT_LEN]) -> VaultKey {
    use argon2::Argon2;
    let a = Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(ARGON_M_KIB, ARGON_T, ARGON_P, None).expect("参数合法"),
    );
    let mut out = [0u8; 32];
    a.hash_password_into(passphrase, salt, &mut out)
        .expect("Argon2id 派生");
    VaultKey(Key::from(out))
}

fn verifier_of(key: &VaultKey) -> [u8; VERIFIER_LEN] {
    let h = blake3::hash(key.0.as_slice());
    h.as_bytes()[..VERIFIER_LEN].try_into().expect("定长")
}

impl Vault {
    /// 新建：随机盐 + 验证器（写入容器 SuperBlock 持久化）。
    pub fn create(passphrase: &[u8]) -> (Vault, [u8; SALT_LEN], [u8; VERIFIER_LEN]) {
        let mut salt = [0u8; SALT_LEN];
        rand::thread_rng().fill_bytes(&mut salt);
        let key = derive(passphrase, &salt);
        let verifier = verifier_of(&key);
        let v = Vault { key, verifier };
        (v, salt, verifier)
    }

    /// 解锁：按持久化的盐重派生并恒时比对验证器。
    pub fn unlock(passphrase: &[u8], salt: &[u8; SALT_LEN], expect: &[u8; VERIFIER_LEN]) -> CmdResult<Vault> {
        let key = derive(passphrase, salt);
        let got = verifier_of(&key);
        // 恒时比较
        let mut diff = 0u8;
        for (a, b) in got.iter().zip(expect.iter()) {
            diff |= a ^ b;
        }
        if diff != 0 {
            return Err(ContainerError::Auth("口令不符（解锁失败）".into()));
        }
        Ok(Vault { key, verifier: got })
    }

    pub fn verifier(&self) -> &[u8; VERIFIER_LEN] {
        &self.verifier
    }

    /// 加密：返回 [nonce24][密文+tag]。
    pub fn seal_bytes(&self, plain: &[u8]) -> Vec<u8> {
        let mut nonce = [0u8; NONCE_LEN];
        rand::thread_rng().fill_bytes(&mut nonce);
        let ct = XChaCha20Poly1305::new(&self.key.0)
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload { msg: plain, aad: b"uxv-b14" },
            )
            .expect("AEAD 加密（nonce 唯一）");
        let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
        out.extend_from_slice(&nonce);
        out.extend_from_slice(&ct);
        out
    }

    /// 解密：输入 [nonce24][密文+tag]；篡改即认证失败。
    pub fn open_bytes(&self, blob: &[u8]) -> CmdResult<Vec<u8>> {
        if blob.len() < NONCE_LEN + 16 {
            return Err(ContainerError::Corrupted("密文过短".into()));
        }
        XChaCha20Poly1305::new(&self.key.0)
            .decrypt(
                XNonce::from_slice(&blob[..NONCE_LEN]),
                Payload { msg: &blob[NONCE_LEN..], aad: b"uxv-b14" },
            )
            .map_err(|_| ContainerError::Corrupted("AEAD 认证失败（数据被篡改或密钥不符）".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seal_open_roundtrip() {
        let (v, salt, verifier) = Vault::create(b"correct horse battery staple");
        assert_eq!(v.verifier(), &verifier);
        let ct = v.seal_bytes(b"secret-metadata");
        assert_ne!(&ct[..], b"secret-metadata");
        assert_eq!(v.open_bytes(&ct).unwrap(), b"secret-metadata");
        // 同明文两次加密密文不同（随机 nonce）
        let ct2 = v.seal_bytes(b"secret-metadata");
        assert_ne!(ct, ct2, "nonce 复用即灾难：必须随机");
        let _ = salt;
    }

    #[test]
    fn wrong_passphrase_is_rejected() {
        let (v1, salt, verifier) = Vault::create(b"right");
        let ct = v1.seal_bytes(b"payload");
        assert!(Vault::unlock(b"wrong", &salt, &verifier).is_err());
        let v2 = Vault::unlock(b"right", &salt, &verifier).unwrap();
        assert_eq!(v2.open_bytes(&ct).unwrap(), b"payload");
    }

    #[test]
    fn tampered_ciphertext_fails_auth() {
        let (v, _, _) = Vault::create(b"k");
        let mut ct = v.seal_bytes(b"important");
        let n = ct.len();
        ct[n - 1] ^= 0xFF;
        assert!(matches!(v.open_bytes(&ct), Err(ContainerError::Corrupted(_))));
    }

    #[test]
    fn argon2id_params_are_persistent_format_compatible() {
        // 盐不同 → key 不同（同口令）
        let (v1, s1, _) = Vault::create(b"same-pass");
        let (v2, s2, _) = Vault::create(b"same-pass");
        assert_ne!(s1, s2);
        assert_ne!(v1.verifier(), v2.verifier());
    }
}
