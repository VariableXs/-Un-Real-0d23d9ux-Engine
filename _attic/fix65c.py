import io
p = 'kernel/varix/src/security/kvault.rs'
s = io.open(p, encoding='utf-8').read()

# 定位 vault_probe 函数：从 pub fn 到下一个顶层 "\n}" 或 tests 模块前
start = s.find('pub fn vault_probe')
assert start > 0, 'vault_probe not found'
# 函数结束：找 "\n}\n" 从 start 之后第一个顶格 }
end = s.find('\n}\n', start)
assert end > start
old_body = s[start:end + 3]

new_body = '''pub fn vault_probe(mut dev: &mut dyn BlockDevice) {
    /// 打分制断言：失败打 FAIL 行并终止探针（实机不 panic 断探针链）。
    macro_rules! pcheck {
        ($cond:expr, $($msg:tt)*) => {
            if !$cond {
                crate::kwarn!("vault: PROBE FAIL - {}", format_args!($($msg)*));
                return;
            }
        };
    }
    /// 步骤通过即打一行（验收记录内嵌 kinfo）。
    macro_rules! pok {
        ($($msg:tt)*) => {
            crate::kinfo!("vault: ok - {}", format_args!($($msg)*));
        };
    }
    crate::kinfo!("vault: probe begin base={}", VLT_JOURNAL_BASE);
    if KvStore::format(&mut dev, VLT_JOURNAL_BASE).is_err() {
        crate::kwarn!("vault: format failed - probe abort");
        return;
    }
    let mut store = match KvStore::open(&mut dev, VLT_JOURNAL_BASE, VLT_DATA_BASE) {
        Ok((s, _)) => s,
        Err(e) => {
            crate::kwarn!("vault: open failed {:?} - probe abort", e);
            return;
        }
    };
    let t0 = crate::cpu::clock::read_tsc();
    if let Err(e) = init(&mut store, b"probe-password-2026") {
        crate::kwarn!("vault: PROBE FAIL - init: {} (rounds={})", e.as_str(), PBKDF2_ROUNDS);
        return;
    }
    let t1 = crate::cpu::clock::read_tsc();
    pok!("init (pbkdf2 rounds={}, 耗时 {} ticks)", PBKDF2_ROUNDS, t1 - t0);
    pcheck!(key_is_gone(), "init 后锁定态且槽位全零");

    pcheck!(matches!(get(&mut store, b"secret.txt"), Err(VaultError::Locked)), "锁定态取条目被拒");
    pcheck!(matches!(unlock(&mut store, b"wrong-password"), Err(VaultError::AuthFail)), "错误口令被拒");
    pcheck!(key_is_gone(), "错误口令后槽位零化");

    if let Err(e) = unlock(&mut store, b"probe-password-2026") {
        crate::kwarn!("vault: PROBE FAIL - unlock: {}", e.as_str());
        return;
    }
    pok!("正确口令解锁");
    let secret = b"variable-vault-kernel-secret-2026";
    let m = match put(&mut store, b"secret.txt", secret, 12345) {
        Ok(m) => m,
        Err(e) => {
            crate::kwarn!("vault: PROBE FAIL - put: {}", e.as_str());
            return;
        }
    };
    pcheck!(m.size == secret.len() as u64, "条目元数据 size");
    let back = match get(&mut store, b"secret.txt") {
        Ok(v) => v,
        Err(e) => {
            crate::kwarn!("vault: PROBE FAIL - get: {}", e.as_str());
            return;
        }
    };
    pcheck!(back.to_vec() == secret.to_vec(), "put/get 往返一致 ({}B)", back.len());

    // 盘面密文不得含明文（真加密证据）。
    match store.get(NS, b"secret.txt") {
        Ok(Some(raw)) => {
            pcheck!(!raw.windows(secret.len()).any(|w| w == secret), "盘面密文无明文子串");
        }
        _ => {
            crate::kwarn!("vault: PROBE FAIL - raw blob missing");
            return;
        }
    }
    pok!("ciphertext-at-rest 无明文泄露");

    // 第二条 + 焚毁三步 + 恢复尝试失败。
    if let Err(e) = put(&mut store, b"note.bin", b"burn-me", 12346) {
        crate::kwarn!("vault: PROBE FAIL - put2: {}", e.as_str());
        return;
    }
    if let Err(e) = destroy(&mut store, b"note.bin") {
        crate::kwarn!("vault: PROBE FAIL - destroy: {}", e.as_str());
        return;
    }
    pcheck!(matches!(get(&mut store, b"note.bin"), Err(VaultError::AuthFail)), "焚毁后恢复尝试失败");
    match store.get(NS_SHRED, b"note.bin") {
        Ok(None) => pok!("shred ns 无残留"),
        _ => {
            crate::kwarn!("vault: PROBE FAIL - shred ns 残留");
            return;
        }
    }
    match list(&mut store) {
        Ok(items) => pcheck!(items.len() == 1, "焚毁后列表剩 1 条"),
        Err(e) => {
            crate::kwarn!("vault: PROBE FAIL - list: {}", e.as_str());
            return;
        }
    }

    // 落锁即零化 → 全链 PASS。
    lock();
    pcheck!(key_is_gone(), "lock 后槽位全零");
    crate::kinfo!("vault: PROBE PASS (全链: init/unlock/put/get/密文核验/焚毁/零化)");
}
'''
s = s[:start] + new_body + s[end + 3:]
io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('vault_probe rewritten OK')
