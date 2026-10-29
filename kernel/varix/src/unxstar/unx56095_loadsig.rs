//! UNX-F56095 负载脚本签名冻结总库（0x60BC0 段）
//!
//! 锚点：UNX-F56095-J1 冻结总库全量（负载脚本逐份落库：脚本键 × 签名 ×
//! 冻结波次三元组）；J2 冻结语义（入库后签名不可变，重入库必须同签名
//! 幂等、异签名即篡改报警）；J3 总库审计链（脚本指纹链、封账只读、签名
//! 格式闭集）三判据全绿。
//! 域：CoRun·UNX-F2-B40（采集管线域·脚本冻结面）。
//!
//! 结构：脚本签名登记册（脚本键 → 签名 × 冻结波次逐份落库）+ 幂等重入
//! 核（同键同签名放行、同键异签名报警）+ 签名格式核（16 位十六进制）+
//! 指纹链 + 封账语义 + 注入核（空脚本键/坏签名格式/异签名篡改/重复幂
//! 等乱序/封账改写/空库比对/波次零必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60BC0 独占 0x20）。
pub const SEG_BASE: u32 = 0x60BC0;
pub const FS_KEY_EMPTY: u32 = SEG_BASE + 0x01; // 脚本键为空
pub const FS_SIG_BAD: u32 = SEG_BASE + 0x02; // 签名格式非法
pub const FS_TAMPER: u32 = SEG_BASE + 0x03; // 异签名篡改（同键异签）
pub const FS_WAVE_ZERO: u32 = SEG_BASE + 0x04; // 冻结波次零
pub const FS_SEALED: u32 = SEG_BASE + 0x05; // 总库已封账拒写
pub const FS_EMPTY: u32 = SEG_BASE + 0x06; // 空库比对
pub const FS_DUP: u32 = SEG_BASE + 0x07; // 同波次重复落库

/// 签名长度（16 位十六进制）。
pub const SIG_LEN: usize = 16;

/// 签名格式核。
pub fn signature_ok(sig: &str) -> bool {
    let b = sig.as_bytes();
    b.len() == SIG_LEN && b.iter().all(|c| c.is_ascii_hexdigit())
}

/// 脚本签名（对脚本字节流 FNV-1a 32 位 ×2 拼接为 16 位十六进制）。
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

/// 脚本账目：脚本键 × 签名 × 冻结波次 × 指纹 × 链。
#[derive(Clone)]
pub struct FrozenScript {
    pub key: String,
    pub sig: String,
    pub wave: u32,
    pub fp: u32,
    pub chain: u32,
}

pub const CHAIN_SEED: u32 = 0x811C_9DC5;

/// 冻结总库。
pub struct ScriptVault {
    pub scripts: Vec<FrozenScript>,
    pub sealed: bool,
}

impl ScriptVault {
    pub fn new() -> Self {
        ScriptVault { scripts: Vec::new(), sealed: false }
    }

    fn find(&self, key: &str) -> Option<usize> {
        for i in 0..self.scripts.len() {
            if self.scripts[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// 入库冻结：空键拒 + 签名格式 + 波次 ≥1 + 同波次重复拒四闸。
    pub fn freeze(&mut self, key: &str, sig: &str, wave: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(FS_SEALED);
        }
        if key.is_empty() {
            return Err(FS_KEY_EMPTY);
        }
        if !signature_ok(sig) {
            return Err(FS_SIG_BAD);
        }
        if wave == 0 {
            return Err(FS_WAVE_ZERO);
        }
        if let Some(i) = self.find(key) {
            if self.scripts[i].sig == sig {
                // 幂等重入：同签名放行但同波次重复落库拒。
                if self.scripts[i].wave == wave {
                    return Err(FS_DUP);
                }
                return Ok(());
            }
            return Err(FS_TAMPER);
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
        self.scripts.push(FrozenScript { key: key.to_string(), sig: sig.to_string(), wave, fp, chain });
        Ok(())
    }

    /// 比对核：给定脚本字节流与在库签名比对（空库显性拒绝）。
    pub fn verify_script(&self, key: &str, script: &[u8]) -> Result<bool, u32> {
        let idx = match self.find(key) {
            Some(i) => i,
            None => return Err(FS_EMPTY),
        };
        Ok(script_signature(script) == self.scripts[idx].sig)
    }

    /// J1：总库全量（逐份签名合法）。
    pub fn vault_ok(&self) -> bool {
        !self.scripts.is_empty()
            && self.scripts.iter().all(|s| !s.key.is_empty() && signature_ok(&s.sig))
    }

    /// J2：冻结不变核（同键异签必须报警——由 freeze 闸保证，此处静态复核签名唯一性）。
    pub fn sig_unique(&self) -> bool {
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

/// 标准夹具：3 份负载脚本冻结（波次 6）。
pub fn standard_vault() -> ScriptVault {
    let mut v = ScriptVault::new();
    let plans: [(&str, &[u8]); 3] = [
        ("load|4k|randread", b"fio --rw=randread --bs=4k"),
        ("load|4k|randwrite", b"fio --rw=randwrite --bs=4k"),
        ("load|1m|seqread", b"fio --rw=read --bs=1m"),
    ];
    for p in plans.iter() {
        let sig = script_signature(p.1);
        if v.freeze(p.0, &sig, 6).is_err() {
            panic!("standard fixture freeze must succeed");
        }
    }
    v
}

/// 注入核：7 注入必抓（空脚本键/坏签名格式/异签名篡改/同波次重复/封账改写/空库比对/波次零）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 空脚本键
    let mut v = ScriptVault::new();
    if v.freeze("", "0123456789abcdef", 1) == Err(FS_KEY_EMPTY) {
        caught += 1;
    }
    // 2) 坏签名（长度 15）
    if v.freeze("k", "0123456789abcde", 1) == Err(FS_SIG_BAD) {
        caught += 1;
    }
    // 3) 波次零
    if v.freeze("k", "0123456789abcdef", 0) == Err(FS_WAVE_ZERO) {
        caught += 1;
    }
    // 4) 同波次重复落库
    if v.freeze("k", "0123456789abcdef", 1).is_ok()
        && v.freeze("k", "0123456789abcdef", 1) == Err(FS_DUP)
    {
        caught += 1;
    }
    // 5) 异签名篡改报警
    if v.freeze("k", "ffffffffffffffff", 2) == Err(FS_TAMPER) {
        caught += 1;
    }
    // 6) 空库比对
    if ScriptVault::new().verify_script("k", b"x").err() == Some(FS_EMPTY) {
        caught += 1;
    }
    // 7) 封账改写拒
    v.seal();
    if v.freeze("late", "0123456789abcdef", 3) == Err(FS_SEALED) {
        caught += 1;
    }
    (caught, 7)
}

/// 总库渲染导出（逐份一行）。
pub fn render_vault(v: &ScriptVault) -> String {
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
pub fn run_unx56095_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56095");
    // J1-1 冻结总库全量（三份逐份合法）
    let v = standard_vault();
    let ok = v.vault_ok() && v.scripts.len() == 3;
    s.add("56095-J1-冻结总库全量", ok, if ok { "" } else { "脚本残缺" });
    // J1-2 签名格式闭集
    let ok = signature_ok("0123456789abcdef") && !signature_ok("0123456789abcde")
        && !signature_ok("g123456789abcdef");
    s.add("56095-J1-签名格式闭集", ok, if ok { "" } else { "签名格式漂移" });
    // J1-3 脚本签名确定性（同脚本同签、异脚本异签）
    let ok = script_signature(b"abc") == script_signature(b"abc")
        && script_signature(b"abc") != script_signature(b"abd");
    s.add("56095-J1-签名确定性", ok, if ok { "" } else { "签名碰撞" });
    // J2-1 幂等重入（同键同签异波放行）
    let mut v2 = standard_vault();
    let sig = script_signature(b"fio --rw=randread --bs=4k");
    let ok = v2.freeze("load|4k|randread", &sig, 7).is_ok();
    s.add("56095-J2-幂等重入", ok, if ok { "" } else { "幂等语义漂移" });
    // J2-2 异签名篡改报警
    let ok = v2.freeze("load|4k|randread", "ffffffffffffffff", 8) == Err(FS_TAMPER);
    s.add("56095-J2-篡改报警", ok, if ok { "" } else { "篡改穿透" });
    // J2-3 脚本比对命中（字节流复签一致）
    let ok = v.verify_script("load|4k|randread", b"fio --rw=randread --bs=4k") == Ok(true);
    s.add("56095-J2-脚本比对命中", ok, if ok { "" } else { "比对漂移" });
    // J2-4 脚本比对报警（字节流被改）
    let ok = v.verify_script("load|4k|randread", b"fio --rw=randread --bs=2k") == Ok(false);
    s.add("56095-J2-脚本比对报警", ok, if ok { "" } else { "比对钝感" });
    // J2-5 签名键唯一
    let ok = v.sig_unique();
    s.add("56095-J2-签名键唯一", ok, if ok { "" } else { "键重复" });
    // J3-1 脚本指纹链复核
    let ok = v.chain_ok();
    s.add("56095-J3-脚本指纹链", ok, if ok { "" } else { "指纹链断裂" });
    // J3-2 链尾稳定
    let ok = standard_vault().chain_tail() == standard_vault().chain_tail();
    s.add("56095-J3-链尾稳定", ok, if ok { "" } else { "链尾漂移" });
    // J3-3 链尾敏感（脚本改一字节链尾变）
    let mut v3 = ScriptVault::new();
    let sig = script_signature(b"fio --rw=randread --bs=4K");
    let _ = v3.freeze("load|4k|randread", &sig, 6);
    let mut v4 = ScriptVault::new();
    let sig = script_signature(b"fio --rw=randread --bs=4k");
    let _ = v4.freeze("load|4k|randread", &sig, 6);
    let ok = v3.chain_tail() != v4.chain_tail();
    s.add("56095-J3-链尾敏感", ok, if ok { "" } else { "链尾钝感" });
    // J3-4 封账拒写
    let mut v5 = standard_vault();
    v5.seal();
    let sig = script_signature(b"late");
    let ok = v5.freeze("late", &sig, 7) == Err(FS_SEALED);
    s.add("56095-J3-封账拒写", ok, if ok { "" } else { "封账穿透" });
    // J1-4 渲染导出三行齐
    let text = render_vault(&standard_vault());
    let ok = render_complete(&text, 3) && text.starts_with("load|4k|randread|");
    s.add("56095-J1-渲染导出齐", ok, if ok { "" } else { "渲染面漂移" });
    // 注入核 7/7
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56095-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56095_all_checks_green() {
        let set = run_unx56095_checks();
        assert!(set.all_passed(), "UNX-F56095 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56095_chain_stable() {
        assert_eq!(standard_vault().chain_tail(), standard_vault().chain_tail());
    }
}
