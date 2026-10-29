//! UNX-F56110 IO 负载脚本冻结库（0x60DA0 段）
//!
//! 锚点：UNX-F56110-J1 冻结库全量（IO 负载脚本逐份落库：脚本键 × 签名 ×
//! 冻结波次三元组）；J2 冻结语义（同键异签篡改报警、幂等重入放行、签名
//! 格式闭集）；J3 库审计链（脚本指纹链、封账只读、五档覆盖）三判据全绿。
//! 域：CoRun·UNX-F2-B40（IO 域·脚本冻结面）。
//!
//! 结构：脚本签名登记册（脚本键 → 签名 × 冻结波次逐份落库）+ 幂等重入
//! 核 + 篡改报警核（同键异签必拒）+ 签名格式核 + 指纹链 + 注入核（空
//! 脚本键/坏签名格式/篡改落库/同波重复/封账改写/空库比对/五档覆盖缺
//! 失必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60DA0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60DA0;
pub const IS_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 脚本键为空
pub const IS_SIG_BAD: u32 = SEG_BASE + 0x02; // 签名格式非法
pub const IS_TAMPER: u32 = SEG_BASE + 0x03; // 同键异签篡改
pub const IS_DUP: u32 = SEG_BASE + 0x04; // 同波次重复落库
pub const IS_SEALED: u32 = SEG_BASE + 0x05; // 冻结库已封账拒写
pub const IS_EMPTY: u32 = SEG_BASE + 0x06; // 空库比对
pub const IS_COVER_MISS: u32 = SEG_BASE + 0x07; // 五档覆盖缺失

/// 签名长度。
pub const SIG_LEN: usize = 16;

/// 签名格式核。
pub fn signature_ok(sig: &str) -> bool {
    let b = sig.as_bytes();
    b.len() == SIG_LEN && b.iter().all(|c| c.is_ascii_hexdigit())
}

/// 脚本签名（正向 FNV × 逆向 FNV 拼接 16 位十六进制）。
pub fn script_signature(script: &[u8]) -> String {
    let mut h1: u32 = 0x811C_9DC5;
    for b in script.iter() {
        h1 ^= *b as u32;
        h1 = h1.wrapping_mul(0x0100_0193);
    }
    let mut h2: u32 = 0x811C_9DC5;
    for b in script.iter().rev() {
        h2 ^= *b as u32;
        h2 = h2.wrapping_mul(0x0100_0193);
    }
    format!("{:08x}{:08x}", h1, h2)
}

/// 五档脚本键闭集（与 F56101 同源）。
pub const TIER_KEYS: [&str; 5] =
    ["load|4k_randread", "load|4k_randwrite", "load|1m_seqread", "load|1m_seqwrite", "load|mix_70_30"];

/// 脚本账目：脚本键 × 签名 × 冻结波次 × 指纹 × 链。
#[derive(Clone)]
pub struct IoScript {
    pub key: String,
    pub sig: String,
    pub wave: u32,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 冻结总库。
pub struct IoScriptVault {
    pub scripts: Vec<IoScript>,
    pub sealed: bool,
}

impl IoScriptVault {
    pub fn new() -> Self {
        IoScriptVault { scripts: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.scripts.len() {
            if self.scripts[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 落库冻结：空键拒 + 签名格式 + 重复/篡改闸三闸。
    pub fn freeze(&mut self, key: &str, sig: &str, wave: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(IS_SEALED);
        }
        if key.is_empty() {
            return Err(IS_KEY_EMPTY);
        }
        if !signature_ok(sig) {
            return Err(IS_SIG_BAD);
        }
        if wave == 0 {
            return Err(IS_DUP);
        }
        if let Some(i) = self.find(key) {
            if self.scripts[i].sig == sig {
                if self.scripts[i].wave == wave {
                    return Err(IS_DUP);
                }
                return Ok(());
            }
            return Err(IS_TAMPER);
        }
        let fp = {
            let mut h: u32 = 0x811C_9DC5;
            let line = format!("{}|{}|w{}", key, sig, wave);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            h
        };
        let chain = match self.scripts.last() {
            None => CHAIN_SEED ^ fp,
            Some(prev) => prev.fp ^ fp,
        };
        self.scripts.push(IoScript {
            key: key.to_string(),
            sig: sig.to_string(),
            wave,
            fp,
            chain,
        });
        Ok(())
    }

    /// 脚本比对（空库显性拒绝）。
    pub fn verify_script(&self, key: &str, script: &[u8]) -> Result<bool, u32> {
        let idx = match self.find(key) {
            Some(i) => i,
            None => return Err(IS_EMPTY),
        };
        Ok(script_signature(script) == self.scripts[idx].sig)
    }

    /// J1：五档覆盖核（闭集键逐个在库）。
    pub fn covered(&self) -> bool {
        for k in TIER_KEYS.iter() {
            if self.find(k).is_none() {
                return false;
            }
        }
        true
    }

    /// J1：库全量（逐份签名合法）。
    pub fn vault_ok(&self) -> bool {
        !self.scripts.is_empty()
            && self.scripts.iter().all(|s| !s.key.is_empty() && signature_ok(&s.sig))
    }

    /// J2：键唯一核。
    pub fn keys_unique(&self) -> bool {
        for i in 0..self.scripts.len() {
            for j in (i + 1)..self.scripts.len() {
                if self.scripts[i].key == self.scripts[j].key {
                    return false;
                }
            }
        }
        true
    }

    /// J3：脚本指纹链复核。
    pub fn chain_ok(&self) -> bool {
        if self.scripts.is_empty() {
            return false;
        }
        for i in 0..self.scripts.len() {
            let expect = if i == 0 {
                CHAIN_SEED ^ self.scripts[0].fp
            } else {
                self.scripts[i - 1].fp ^ self.scripts[i].fp
            };
            if self.scripts[i].chain != expect {
                return false;
            }
        }
        true
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    pub fn chain_tail(&self) -> u32 {
        match self.scripts.last() {
            None => CHAIN_SEED,
            Some(s) => s.chain,
        }
    }
}

/// 标准夹具：五档脚本全冻结（波次 6）。
pub fn standard_vault() -> IoScriptVault {
    let mut v = IoScriptVault::new();
    let bodies: [&[u8]; 5] = [
        b"fio --rw=randread --bs=4k --direct=1",
        b"fio --rw=randwrite --bs=4k --direct=1",
        b"fio --rw=read --bs=1m --direct=1",
        b"fio --rw=write --bs=1m --direct=1",
        b"fio --rw=randrw --rwmixread=70 --bs=4k",
    ];
    for i in 0..TIER_KEYS.len() {
        let sig = script_signature(bodies[i]);
        if v.freeze(TIER_KEYS[i], &sig, 6).is_err() {
            panic!("standard fixture freeze must succeed");
        }
    }
    v
}

/// 注入核：7 注入必抓（空脚本键/坏签名/篡改落库/同波重复/封账改写/空库比对/覆盖缺失）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空脚本键
    let mut v = IoScriptVault::new();
    if v.freeze("", "0123456789abcdef", 1) == Err(IS_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 坏签名
    if v.freeze("k", "0123456789abcde", 1) == Err(IS_SIG_BAD) {
        caught += 1;
    }
    // 3) 篡改落库
    if v.freeze("k", "0123456789abcdef", 1).is_ok()
        && v.freeze("k", "ffffffffffffffff", 2) == Err(IS_TAMPER)
    {
        caught += 1;
    }
    // 4) 同波重复
    if v.freeze("k2", "0123456789abcdef", 1).is_ok()
        && v.freeze("k2", "0123456789abcdef", 1) == Err(IS_DUP)
    {
        caught += 1;
    }
    // 5) 空库比对
    if IoScriptVault::new().verify_script("k", b"x").err() == Some(IS_EMPTY) {
        caught += 1;
    }
    // 6) 覆盖缺失（单脚本库不覆盖五档）
    let mut v2 = IoScriptVault::new();
    let _ = v2.freeze("k", "0123456789abcdef", 1);
    if !v2.covered() {
        caught += 1;
    }
    // 7) 封账改写拒
    let mut sb = standard_vault();
    sb.seal();
    if sb.freeze("late", "0123456789abcdef", 7) == Err(IS_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 冻结库渲染导出。
pub fn render_vault(v: &IoScriptVault) -> String {
    let mut out = String::new();
    for s in v.scripts.iter() {
        out.push_str(&format!("{}|{}|w{}\n", s.key, s.sig, s.wave));
    }
    out
}

/// 渲染完整性核：行数齐 + 逐行三段 + 签名 16 位。
pub fn render_complete(text: &str, expect_rows: usize) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != expect_rows {
        return false;
    }
    lines.iter().all(|ln| {
        let parts: Vec<&str> = ln.split('|').collect();
        parts.len() == 3 && signature_ok(parts[1])
    })
}

/// 判据入口（CheckSet）。
pub fn run_unx56110_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56110");
    // J1-1 冻结库全量（五份逐份合法）
    let v = standard_vault();
    let ok = v.vault_ok() && v.scripts.len() == 5;
    s.add("56110-J1-冻结库全量", ok, if ok { "" } else { "脚本残缺" });
    // J1-2 五档覆盖命中
    let ok = v.covered();
    s.add("56110-J1-五档覆盖命中", ok, if ok { "" } else { "覆盖缺档" });
    // J1-3 签名格式闭集
    let ok = signature_ok("0123456789abcdef") && !signature_ok("0123456789abcde");
    s.add("56110-J1-签名格式闭集", ok, if ok { "" } else { "签名漂移" });
    // J2-1 脚本比对命中（字节流复签一致）
    let ok = v.verify_script("load|4k_randread", b"fio --rw=randread --bs=4k --direct=1") == Ok(true);
    s.add("56110-J2-脚本比对命中", ok, if ok { "" } else { "比对漂移" });
    // J2-2 脚本比对报警（字节流被改）
    let ok = v.verify_script("load|4k_randread", b"fio --rw=randread --bs=8k --direct=1") == Ok(false);
    s.add("56110-J2-脚本比对报警", ok, if ok { "" } else { "比对钝感" });
    // J2-3 篡改报警（同键异签拒）
    let mut v2 = standard_vault();
    let ok = v2.freeze("load|4k_randread", "ffffffffffffffff", 7) == Err(IS_TAMPER);
    s.add("56110-J2-篡改报警", ok, if ok { "" } else { "篡改穿透" });
    // J2-4 幂等重入放行（同签异波）
    let sig = script_signature(b"fio --rw=randread --bs=4k --direct=1");
    let ok = v2.freeze("load|4k_randread", &sig, 7).is_ok();
    s.add("56110-J2-幂等重入", ok, if ok { "" } else { "幂等漂移" });
    // J3-1 脚本指纹链复核
    let ok = v.chain_ok();
    s.add("56110-J3-脚本指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_vault().chain_tail() == standard_vault().chain_tail();
    s.add("56110-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（脚本改一字节链尾变）
    let mut v3 = IoScriptVault::new();
    let sig = script_signature(b"fio --rw=randread --bs=4k --direct=2");
    let _ = v3.freeze("load|4k_randread", &sig, 6);
    let ok = v3.chain_tail() != {
        let mut t = IoScriptVault::new();
        let sig = script_signature(b"fio --rw=randread --bs=4k --direct=1");
        let _ = t.freeze("load|4k_randread", &sig, 6);
        t.chain_tail()
    };
    s.add("56110-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut v4 = standard_vault();
    v4.seal();
    let ok = v4.freeze("late", "0123456789abcdef", 7) == Err(IS_SEALED);
    s.add("56110-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出五行齐
    let text = render_vault(&standard_vault());
    let ok = render_complete(&text, 5) && text.starts_with("load|4k_randread|");
    s.add("56110-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56110-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56110_all_checks_green() {
        let set = run_unx56110_checks();
        assert!(set.all_passed(), "UNX-F56110 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56110_chain_stable() {
        assert_eq!(standard_vault().chain_tail(), standard_vault().chain_tail());
    }
}
