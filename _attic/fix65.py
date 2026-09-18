import io

# --- ksha256.rs ---
p = 'kernel/varix/src/security/ksha256.rs'
s = io.open(p, encoding='utf-8').read()
old = "    let mut inner = [0u8; 64 + data.len().max(0)];"
new = "    let mut inner = alloc::vec![0u8; 64 + data.len()];"
assert old in s, 'ksha256 inner not found'
s = s.replace(old, new, 1)
io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('ksha256 OK')

# --- kvault.rs ---
p = 'kernel/varix/src/security/kvault.rs'
s = io.open(p, encoding='utf-8').read()

old = "crate::time::boot_ms"
cnt = s.count(old)
assert cnt == 2, 'boot_ms count=%d' % cnt
s = s.replace("let t0 = crate::time::boot_ms();", "let t0 = crate::cpu::clock::read_tsc();", 1)
s = s.replace("let t1 = crate::time::boot_ms();", "let t1 = crate::cpu::clock::read_tsc();", 1)
s = s.replace('init\u8017\u65f6={}ms\uff08pbkdf2', 'init\u8017\u65f6={}ticks\uff08pbkdf2', 1)

old = """            VaultError::Kv(e) => e.as_str(),
            VaultError::Block(e) => e.as_str(),"""
new = """            VaultError::Kv(e) => match e {
                KvError::Full => "kv-full",
                KvError::TooLarge => "kv-too-large",
                KvError::BadNamespace => "kv-bad-ns",
                KvError::BadKey => "kv-bad-key",
                KvError::Io(b) => b.as_str(),
            },
            VaultError::Block(e) => e.as_str(),"""
assert old in s, 'kv as_str not found'
s = s.replace(old, new, 1)

old = "            let mut seed = n;\n"
new = "            let mut seed = [0u8; 32];\n            seed[..NONCE_LEN].copy_from_slice(&n);\n"
assert old in s, 'seed type not found'
s = s.replace(old, new, 1)

old = 'KvStore::open(&mut dev, VLT_JOURNAL_BASE, VLT_DATA_BASE).expect("open").0'
new = 'KvStore::open(dev, VLT_JOURNAL_BASE, VLT_DATA_BASE).expect("open").0'
assert old in s, 'fresh_store not found'
s = s.replace(old, new, 1)

old = "use crate::kaesgcm::{Aes256Gcm, GcmError, KEY_LEN, NONCE_LEN, TAG_LEN};"
new = "use crate::kaesgcm::{Aes256Gcm, KEY_LEN, NONCE_LEN, TAG_LEN};"
assert old in s, 'gcm import not found'
s = s.replace(old, new, 1)

io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('kvault OK')

# --- kaesgcm.rs ---
p = 'kernel/varix/src/security/kaesgcm.rs'
s = io.open(p, encoding='utf-8').read()

old = "    /// `ct` \u4e0e `pt` \u7b49\u957f\uff08\u53ef\u540c\u4e00\u7f13\u51b2\u539f\u5730\u52a0\u5bc6\u2014\u2014CTR \u6309\u5757\u8bfb\u540e\u5199\uff09\u3002"
new = "    /// `ct` \u4e0e `pt` \u7b49\u957f\uff08**\u4e0d\u540c\u7f13\u51b2**\u2014\u2014\u501f\u7528\u68c0\u67e5\u7981\u6b62\u540c\u7f13\u51b2\u539f\u5730\u8c03\u7528\uff09\u3002"
assert old in s, 'seal doc not found'
s = s.replace(old, new, 1)

i = s.find("    /// 原地加密（ct 与 pt 同缓冲）往返一致。")
assert i >= 0, 'in_place test not found'
j = s.find("    #[test]", i)
j2 = s.find("\n}\n", j)  # test 函数结束后
# 找到该 test 的闭合：从 i 到下一个 "\n    }\n"（test 函数结束）
end = s.find("\n    }\n", j) + len("\n    }\n")
old_block = s[i:end]
new_block = """    /// 长度 100（跨 7 块、尾块 4B）往返一致 + 密钥流偏移正确性。
    #[test]
    fn non_multiple_length_roundtrip() {
        let k = [0x42u8; 32];
        let iv = [0x24u8; 12];
        let g = Aes256Gcm::new(&k);
        let plain = alloc::vec![0x11u8; 100];
        let mut ct = alloc::vec![0u8; 100];
        let mut tag = [0u8; 16];
        g.seal(&iv, b"x", &plain, &mut ct, &mut tag).unwrap();
        assert_ne!(ct, plain);
        let mut out = alloc::vec![0u8; 100];
        g.open(&iv, b"x", &ct, &tag, &mut out).unwrap();
        assert_eq!(out, plain);
        // 两段相同明文用同一 nonce 加密 → 密文一致（确定性冒烟）。
        let mut ct2 = alloc::vec![0u8; 100];
        let mut tag2 = [0u8; 16];
        g.seal(&iv, b"x", &plain, &mut ct2, &mut tag2).unwrap();
        assert_eq!(ct, ct2);
        assert_eq!(tag, tag2);
    }

    /// 缓冲长度不匹配 → BufMismatch。
    #[test]
    fn buf_mismatch_rejected() {
        let k = [1u8; 32];
        let iv = [2u8; 12];
        let g = Aes256Gcm::new(&k);
        let mut ct = alloc::vec![0u8; 3];
        let mut tag = [0u8; 16];
        assert_eq!(g.seal(&iv, b"", b"abcd", &mut ct, &mut tag), Err(GcmError::BufMismatch));
    }
"""
s = s[:i] + new_block + s[end:]
io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('kaesgcm OK')
