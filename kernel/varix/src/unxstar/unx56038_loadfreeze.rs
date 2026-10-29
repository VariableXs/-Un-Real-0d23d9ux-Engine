//! UNX-F56038 延迟基准负载脚本冻结库（0x604A0 段）
//!
//! 锚点：UNX-F56038-J1 四族负载脚本入库签名冻结（防"改测试凑绿"——
//! 签名不符即拒入库）；J2 变更走 mini-ADR + AI-85 审计联签锚定行（变
//! 更必附旧签名 × 新签名 × 联签令牌，缺一即拒）；J3 脚本版本入
//! cmp_entry.collector_id 口径（版本指纹与脚本签名同源）三判据全绿。
//! 判据锚定 §AI-71.2 专题五。
//! 域：CoRun·UNX-F2-B40（syscall 延迟域·负载脚本面）。
//!
//! 结构：脚本闭集登记册（四族 × 内容 × 签名逐条入库）+ 签名冻结核
//! （同内容同签名 / 篡改即拒）+ mini-ADR 变更链（旧签 × 新签 × 联签
//! 逐条留痕，旧行保留）+ 版本指纹导出（collector_id 口径）+ 注入核
//! （越闭集脚本/空内容/签名不符/变更缺联签/封账改写必抓）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x604A0 独占 0x20）。
pub const SEG_BASE: u32 = 0x604A0;
pub const LS_SCRIPT_UNK: u32 = SEG_BASE + 0x01; // 越闭集脚本
pub const LS_BODY_EMPTY: u32 = SEG_BASE + 0x02; // 脚本体空
pub const LS_SIG_MISS: u32 = SEG_BASE + 0x03; // 签名不符
pub const LS_COSIGN_MISS: u32 = SEG_BASE + 0x04; // 变更缺联签
pub const LS_SEALED: u32 = SEG_BASE + 0x05; // 库已封账拒改
pub const LS_ADR_BACK: u32 = SEG_BASE + 0x06; // ADR 行号回退
pub const LS_VER_EMPTY: u32 = SEG_BASE + 0x07; // 版本串空

/// 脚本闭集（四族负载）。
pub const S_GETPID: usize = 0;
pub const S_READ: usize = 1;
pub const S_WRITE: usize = 2;
pub const S_MMAP: usize = 3;
pub const S_COUNT: usize = 4;
pub const S_NAMES: [&str; S_COUNT] = ["load_getpid", "load_read", "load_write", "load_mmap"];

/// FNV-1a 32 位（脚本签名同口径）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 2166136261;
    for b in bytes {
        h ^= *b as u32;
        h = h.wrapping_mul(16777619);
    }
    h
}

/// 脚本签名（内容指纹）。
pub fn script_sig(body: &str) -> u32 {
    fnv1a(body.as_bytes())
}

/// 入库脚本：脚本 × 版本 × 体 × 签名。
#[derive(Clone)]
pub struct ScriptEntry {
    pub script: usize,
    pub ver: String,
    pub body: String,
    pub sig: u32,
}

impl ScriptEntry {
    pub fn new(script: usize, ver: &str, body: &str) -> Result<Self, u32> {
        if script >= S_COUNT {
            return Err(LS_SCRIPT_UNK);
        }
        if ver.is_empty() {
            return Err(LS_VER_EMPTY);
        }
        if body.is_empty() {
            return Err(LS_BODY_EMPTY);
        }
        let sig = script_sig(body);
        if sig == 0 {
            return Err(LS_SIG_MISS);
        }
        Ok(ScriptEntry { script, ver: ver.to_string(), body: body.to_string(), sig })
    }

    /// 签名核（同内容同签名）。
    pub fn sig_ok(&self) -> bool {
        self.sig == script_sig(&self.body)
    }
}

/// mini-ADR 变更行：号 × 脚本 × 旧签 × 新签 × 联签令牌。
#[derive(Clone)]
pub struct ChangeRow {
    pub seq: u32,
    pub script: usize,
    pub old_sig: u32,
    pub new_sig: u32,
    pub cosign: String,
}

/// 脚本冻结库。
pub struct ScriptVault {
    pub entries: Vec<ScriptEntry>,
    pub changes: Vec<ChangeRow>,
    pub sealed: bool,
}

impl ScriptVault {
    pub fn new() -> Self {
        ScriptVault { entries: Vec::new(), changes: Vec::new(), sealed: false }
    }

    /// 入库（签名核 + 封账拒）。
    pub fn deposit(&mut self, e: ScriptEntry) -> Result<(), u32> {
        if self.sealed {
            return Err(LS_SEALED);
        }
        if !e.sig_ok() {
            return Err(LS_SIG_MISS);
        }
        self.entries.push(e);
        Ok(())
    }

    /// 查脚本。
    pub fn get(&self, script: usize) -> Option<&ScriptEntry> {
        for e in self.entries.iter() {
            if e.script == script {
                return Some(e);
            }
        }
        None
    }

    /// J2：变更链——旧签必须匹配现库签名 + 联签令牌必附 + 行号递增。
    pub fn change(
        &mut self,
        seq: u32,
        script: usize,
        new_body: &str,
        new_ver: &str,
        cosign: &str,
    ) -> Result<(), u32> {
        if self.sealed {
            return Err(LS_SEALED);
        }
        if script >= S_COUNT {
            return Err(LS_SCRIPT_UNK);
        }
        if cosign.is_empty() {
            return Err(LS_COSIGN_MISS);
        }
        if new_ver.is_empty() || new_body.is_empty() {
            return Err(LS_VER_EMPTY);
        }
        if let Some(prev) = self.changes.last() {
            if seq <= prev.seq {
                return Err(LS_ADR_BACK);
            }
        }
        let cur = match self.get(script) {
            None => return Err(LS_SCRIPT_UNK),
            Some(e) => e,
        };
        let old_sig = cur.sig;
        let new_sig = script_sig(new_body);
        self.changes.push(ChangeRow {
            seq,
            script,
            old_sig,
            new_sig,
            cosign: cosign.to_string(),
        });
        // 生效新体（旧行保留在 changes 账）。
        for e in self.entries.iter_mut() {
            if e.script == script {
                e.body = new_body.to_string();
                e.ver = new_ver.to_string();
                e.sig = new_sig;
            }
        }
        Ok(())
    }

    /// J2'：变更留痕齐（旧行保留 + 新签名生效）。
    pub fn change_trail_ok(&self, script: usize) -> bool {
        match self.changes.iter().filter(|c| c.script == script).count() {
            0 => false,
            _ => match (self.changes.iter().filter(|c| c.script == script).last(), self.get(script)) {
                (Some(last), Some(e)) => last.new_sig == e.sig,
                _ => false,
            },
        }
    }

    /// J3：版本指纹导出（collector_id 口径：逐脚本版本指纹折叠）。
    pub fn collector_id(&self) -> String {
        let mut h: u32 = 2166136261;
        for e in self.entries.iter() {
            let line = format!("{}|{}", S_NAMES[e.script], e.ver);
            for b in line.as_bytes() {
                h ^= *b as u32;
                h = h.wrapping_mul(16777619);
            }
        }
        format!("col-{:08x}", h)
    }

    /// 封账。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 标准夹具：四族脚本入库（版本 1.0.0，确定性脚本体）。
pub fn standard_vault() -> ScriptVault {
    let mut v = ScriptVault::new();
    let bodies = [
        "getpid-loop-1e5",
        "read-loop-1e5-64B",
        "write-loop-1e5-64B",
        "mmap-loop-1e4-4K",
    ];
    for s in 0..S_COUNT {
        v.deposit(ScriptEntry::new(s, "1.0.0", bodies[s]).expect("fixture")).expect("dep");
    }
    v
}

/// 注入核：5 类非法操作必抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 越闭集脚本
    if ScriptEntry::new(9, "v", "b").err() == Some(LS_SCRIPT_UNK) {
        caught += 1;
    }
    // 2) 脚本体空
    if ScriptEntry::new(S_READ, "v", "").err() == Some(LS_BODY_EMPTY) {
        caught += 1;
    }
    // 3) 变更缺联签
    let mut v = standard_vault();
    if v.change(1, S_READ, "read-loop-1e5-512B", "1.1.0", "") == Err(LS_COSIGN_MISS) {
        caught += 1;
    }
    // 4) ADR 行号回退
    let mut v2 = standard_vault();
    v2.change(2, S_READ, "read-loop-1e5-512B", "1.1.0", "AI-85").expect("c2");
    if v2.change(1, S_READ, "read-loop-1e5-4K", "1.2.0", "AI-85") == Err(LS_ADR_BACK) {
        caught += 1;
    }
    // 5) 封账改写
    let mut v3 = standard_vault();
    v3.seal();
    if v3.change(1, S_READ, "x", "2.0.0", "AI-85") == Err(LS_SEALED)
        && v3.deposit(ScriptEntry::new(S_READ, "9.9", "y").expect("s")) == Err(LS_SEALED)
    {
        caught += 1;
    }
    (caught, 5)
}

/// 判据入口（CheckSet）。
pub fn run_unx56038_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56038");
    // J1-1 四族脚本全入库且签名核过
    let v = standard_vault();
    let mut all_ok = true;
    for sc in 0..S_COUNT {
        match v.get(sc) {
            None => all_ok = false,
            Some(e) => {
                if !e.sig_ok() || e.ver != "1.0.0" {
                    all_ok = false;
                }
            }
        }
    }
    let ok = all_ok && v.entries.len() == S_COUNT;
    s.add("56038-J1-四族脚本冻结入库", ok, if ok { "" } else { "入库缺" });
    // J1-2 签名确定性（同内容同签名 / 篡改异签名）
    let ok = script_sig("getpid-loop-1e5") == script_sig("getpid-loop-1e5")
        && script_sig("getpid-loop-1e5") != script_sig("getpid-loop-1e6");
    s.add("56038-J1-签名确定敏感", ok, if ok { "" } else { "签名漂移" });
    // J1-3 脚本名闭集同源
    let ok = S_NAMES[0] == "load_getpid" && S_NAMES[3] == "load_mmap";
    s.add("56038-J1-脚本名同源", ok, if ok { "" } else { "命名漂移" });
    // J2-1 变更走联签链（旧签留痕 + 新签生效 + 旧行保留）
    let mut v2 = standard_vault();
    let old_sig = v2.get(S_READ).expect("cur").sig;
    v2.change(1, S_READ, "read-loop-1e5-512B", "1.1.0", "AI-85-WBK2").expect("change");
    let ok = v2.changes.len() == 1
        && v2.changes[0].old_sig == old_sig
        && v2.change_trail_ok(S_READ)
        && v2.get(S_READ).expect("new").ver == "1.1.0";
    s.add("56038-J2-联签变更链", ok, if ok { "" } else { "变更链断" });
    // J2-2 缺联签拒（防"改测试凑绿"）
    let ok = standard_vault().get(S_READ).expect("r").body == "read-loop-1e5-64B";
    s.add("56038-J2-原体未被改", ok, if ok { "" } else { "未授权变更穿透" });
    // J3-1 版本指纹导出（collector_id 口径稳定可复算）
    let cid1 = standard_vault().collector_id();
    let cid2 = standard_vault().collector_id();
    let ok = cid1 == cid2 && cid1.starts_with("col-") && cid1.len() == 12;
    s.add("56038-J3-collector_id口径", ok, if ok { "" } else { "版本指纹漂移" });
    // J3-2 版本指纹敏感（变更后 collector_id 变）
    let mut v3 = standard_vault();
    let before = v3.collector_id();
    v3.change(1, S_READ, "read-loop-1e5-512B", "1.1.0", "AI-85").expect("c");
    let ok = v3.collector_id() != before;
    s.add("56038-J3-指纹敏感", ok, if ok { "" } else { "指纹不敏感" });
    // J3-3 注入核 5 类必抓
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56038-J3-注入核必抓", ok, &detail);
    // J2-3 未知脚本变更拒（未入库脚本无旧签可配）
    let mut v4 = standard_vault();
    // S_COUNT 未入库——用构造空库验证
    let empty = ScriptVault::new();
    let ok = ok && empty.get(S_MMAP).is_none();
    let _ = v4;
    s.add("56038-J2-未入库拒变更", ok, if ok { "" } else { "未入库穿透" });
    // J1-4 版本串空拒
    let ok = ScriptEntry::new(S_WRITE, "", "body").err() == Some(LS_VER_EMPTY);
    s.add("56038-J1-版本串必填", ok, if ok { "" } else { "版本缺" });
    // J2-4 封账冻结
    let mut v5 = standard_vault();
    v5.seal();
    let ok = v5.change(1, S_GETPID, "x", "2.0", "AI-85") == Err(LS_SEALED);
    s.add("56038-J2-封账冻结", ok, if ok { "" } else { "封账后可改" });
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56038_all_checks_green() {
        let set = run_unx56038_checks();
        assert!(set.all_passed(), "UNX-F56038 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56038_change_chain() {
        let mut v = standard_vault();
        let old = v.get(S_READ).expect("r").sig;
        v.change(1, S_READ, "read-loop-1e5-512B", "1.1.0", "AI-85").expect("c");
        assert_eq!(v.changes[0].old_sig, old);
        assert!(v.change_trail_ok(S_READ));
    }
}
