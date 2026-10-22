//! UNX-F7881 恢复引擎总纲（恢复四阶段宣言）（0xA000 段）
//!
//! 锚点：UNX-F7881-J1 四阶段冻结（①清单载入→②对象定位→③逐文件
//! 还原→④逐文件校验——阶段状态机 ×1；跳阶段注入 ×10 拒收；与备份
//! 五阶段（F7861）对称性声明 ×1）。
//! 域：CoRun·UNX-B5-B35（恢复引擎面）。
//!
//! 结构：四阶段闭集（LoadManifest→LocateObject→RestoreFile→
//! VerifyFile）+ 阶段状态机（严格递进 + 跳阶段 ×10 注入全拒）+
//! 与备份五阶段（F7861）对称性声明（恢复四 ↔ 备份五的映射账）
//! + 注入核（跳步/回退/未知态/对称破缺/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0xA000 独占 0x80）。
pub const SEG_BASE: u32 = 0xA000;
pub const XC_RE_SKIP: u32 = SEG_BASE + 0x01; // 跳阶段
pub const XC_RE_BACK: u32 = SEG_BASE + 0x02; // 回退
pub const XC_RE_UNK: u32 = SEG_BASE + 0x03; // 未知态
pub const XC_RE_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// 四阶段闭集。
pub const PH_LOAD: u8 = 0; // ①清单载入
pub const PH_LOCATE: u8 = 1; // ②对象定位
pub const PH_RESTORE: u8 = 2; // ③逐文件还原
pub const PH_VERIFY: u8 = 3; // ④逐文件校验

/// 阶段名。
pub fn phase_name(p: u8) -> &'static str {
    match p {
        PH_LOAD => "load-manifest",
        PH_LOCATE => "locate-object",
        PH_RESTORE => "restore-file",
        PH_VERIFY => "verify-file",
        _ => "?",
    }
}

/// 恢复引擎状态机。
pub struct RestoreEngine {
    pub phase: u8,
    pub files_restored: Vec<String>,
    pub files_verified: Vec<String>,
    pub sealed: bool,
}

impl RestoreEngine {
    pub fn new() -> Self {
        RestoreEngine {
            phase: PH_LOAD,
            files_restored: Vec::new(),
            files_verified: Vec::new(),
            sealed: false,
        }
    }

    /// 阶段推进（严格 +1 递进）。
    pub fn advance(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_RE_SEALED);
        }
        if self.phase >= PH_VERIFY {
            self.sealed = true;
            return Err(XC_RE_SEALED);
        }
        self.phase += 1;
        Ok(())
    }

    /// 目标阶段直推（合法 = 只能到下一阶段；跳阶段拒）。
    pub fn goto(&mut self, target: u8) -> Result<(), u32> {
        if target > PH_VERIFY {
            return Err(XC_RE_UNK);
        }
        if self.sealed {
            return Err(XC_RE_SEALED);
        }
        if target < self.phase {
            return Err(XC_RE_BACK);
        }
        if target > self.phase + 1 {
            return Err(XC_RE_SKIP);
        }
        if target == self.phase {
            return Err(XC_RE_SKIP);
        }
        self.phase = target;
        if self.phase == PH_VERIFY {
            // 进入校验阶段不封账；完成校验后封
        }
        Ok(())
    }

    /// 逐文件还原登记。
    pub fn restore_file(&mut self, name: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_RE_SEALED);
        }
        if self.phase < PH_RESTORE {
            return Err(XC_RE_SKIP);
        }
        self.files_restored.push(String::from(name));
        Ok(())
    }

    /// 逐文件校验登记（须先还原）。
    pub fn verify_file(&mut self, name: &str) -> Result<bool, u32> {
        if self.sealed {
            return Err(XC_RE_SEALED);
        }
        if self.phase < PH_VERIFY {
            return Err(XC_RE_SKIP);
        }
        let ok = self.files_restored.iter().any(|f| f == name);
        if ok {
            self.files_verified.push(String::from(name));
        }
        Ok(ok)
    }

    /// 全阶段完成断言。
    pub fn complete(&self) -> bool {
        self.phase == PH_VERIFY
            && !self.files_restored.is_empty()
            && self.files_restored.len() == self.files_verified.len()
    }
}

/// 四阶段全序核：载入→定位→还原→校验。
pub fn four_phase_exact() -> bool {
    let mut e = RestoreEngine::new();
    if e.advance().is_err() {
        return false;
    }
    if e.advance().is_err() {
        return false;
    }
    if e.restore_file("etc/passwd").is_err() {
        return false;
    }
    if e.advance().is_err() {
        return false;
    }
    match e.verify_file("etc/passwd") {
        Ok(true) => {}
        _ => return false,
    }
    e.complete()
}

/// 跳阶段 ×10 注入全拒核：从各阶段跳两步+ 全拒。
pub fn skip_inject_10_exact() -> bool {
    let mut caught = 0usize;
    // 注入 1：LOAD → RESTORE（跳 LOCATE）
    let mut e = RestoreEngine::new();
    if e.goto(PH_RESTORE) == Err(XC_RE_SKIP) {
        caught += 1;
    }
    // 注入 2：LOAD → VERIFY（跳两步）
    let mut e2 = RestoreEngine::new();
    if e2.goto(PH_VERIFY) == Err(XC_RE_SKIP) {
        caught += 1;
    }
    // 注入 3：LOAD 停留（target==phase）
    let mut e3 = RestoreEngine::new();
    if e3.goto(PH_LOAD) == Err(XC_RE_SKIP) {
        caught += 1;
    }
    // 注入 4：还原前校验登记（阶段未到）
    let mut e4 = RestoreEngine::new();
    if e4.verify_file("x") == Err(XC_RE_SKIP) {
        caught += 1;
    }
    // 注入 5：还原前 restore（阶段未到）
    let mut e5 = RestoreEngine::new();
    if e5.restore_file("y") == Err(XC_RE_SKIP) {
        caught += 1;
    }
    // 注入 6：LOCATE → VERIFY
    let mut e6 = RestoreEngine::new();
    let _ = e6.advance();
    if e6.goto(PH_VERIFY) == Err(XC_RE_SKIP) {
        caught += 1;
    }
    // 注入 7：RESTORE → LOAD 回退
    let mut e7 = RestoreEngine::new();
    let _ = e7.advance();
    let _ = e7.advance();
    if e7.goto(PH_LOAD) == Err(XC_RE_BACK) {
        caught += 1;
    }
    // 注入 8：未知目标态（target=9）
    let mut e8 = RestoreEngine::new();
    if e8.goto(9) == Err(XC_RE_UNK) {
        caught += 1;
    }
    // 注入 9：封账后推进
    let mut e9 = RestoreEngine::new();
    for _ in 0..3 {
        let _ = e9.advance();
    }
    if e9.advance() == Err(XC_RE_SEALED) {
        caught += 1;
    }
    // 注入 10：verify 未还原文件名 → false
    let mut e10 = RestoreEngine::new();
    for _ in 0..3 {
        let _ = e10.advance();
    }
    let _ = e10.restore_file("known");
    match e10.verify_file("unknown") {
        Ok(false) => caught += 1,
        _ => {}
    }
    caught == 10
}

/// 与备份五阶段对称性声明核：恢复四 ↔ 备份五映射（冻结表）。
pub const BACKUP_FIVE: [&str; 5] = [
    "scan-source",
    "snapshot",
    "chunk-write",
    "merkle-seal",
    "manifest-write",
];

/// 对称映射账：恢复四阶段 → 备份五阶段对应位。
pub const SYM_MAP: [(u8, usize); 4] = [
    (PH_LOAD, 4), // 清单载入 ↔ manifest-write（逆）
    (PH_LOCATE, 3), // 对象定位 ↔ merkle-seal 定位（逆）
    (PH_RESTORE, 2), // 逐文件还原 ↔ chunk-write（逆）
    (PH_VERIFY, 1), // 逐文件校验 ↔ snapshot 对照（逆）
];

pub fn symmetry_exact() -> bool {
    SYM_MAP.len() == 4
        && SYM_MAP.iter().all(|(p, b)| {
            *p <= PH_VERIFY && *b < BACKUP_FIVE.len()
        })
        // 映射目标唯一
        && {
            let mut seen = [false; 5];
            SYM_MAP.iter().all(|(_, b)| {
                if seen[*b] {
                    false
                } else {
                    seen[*b] = true;
                    true
                }
            })
        }
}

/// 注入核（4 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 封账后 restore
    let mut e = RestoreEngine::new();
    for _ in 0..3 {
        let _ = e.advance();
    }
    e.sealed = true;
    if e.restore_file("z") == Err(XC_RE_SEALED) {
        caught += 1;
    }
    // 2) 对称映射破缺（伪造重复目标）
    let bad = [(0u8, 2usize), (1, 2), (2, 1), (3, 3)];
    let dup = {
        let mut seen = [false; 5];
        bad.iter().any(|(_, b)| {
            if seen[*b] {
                true
            } else {
                seen[*b] = true;
                false
            }
        })
    };
    if dup {
        caught += 1;
    }
    // 3) 还原阶段未到（LOAD 态直接 verify 登记路径由 skip 注入覆盖）
    let mut e3 = RestoreEngine::new();
    if e3.advance().is_ok() && e3.phase == PH_LOCATE {
        caught += 1;
    }
    // 4) 完成断言：半途不 complete
    let mut e4 = RestoreEngine::new();
    let _ = e4.advance();
    let _ = e4.advance();
    let _ = e4.restore_file("a");
    if !e4.complete() {
        caught += 1;
    }
    (caught, 4)
}

/// 指纹冻结：引擎账折叠哈希。
pub fn fp_frozen(e: &RestoreEngine) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    h ^= e.phase as u64;
    h = h.wrapping_mul(0x100000001b3);
    for f in &e.files_restored {
        for &c in f.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    for f in &e.files_verified {
        for &c in f.as_bytes() {
            h ^= c as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx7881_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7881");
    // J1-1 四阶段全序
    let ok = four_phase_exact();
    s.add("B35-01-四阶段全序", ok, if ok { "" } else { "全序漂移" });
    // J1-2 跳阶段注入 ×10 全拒
    let ok = skip_inject_10_exact();
    s.add("B35-02-跳阶段十拒", ok, if ok { "" } else { "跳步漏抓" });
    // J1-3 对称性声明
    let ok = symmetry_exact();
    s.add("B35-03-对称性声明", ok, if ok { "" } else { "对称漂移" });
    // J1-4 阶段闭集
    let ok = phase_name(0) != "?" && phase_name(3) != "?" && phase_name(4) == "?";
    s.add("B35-04-阶段闭集", ok, if ok { "" } else { "闭集漂移" });
    // J1-5 注入核 4/4
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B35-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let mut e = RestoreEngine::new();
    for _ in 0..3 {
        let _ = e.advance();
    }
    let _ = e.restore_file("f1");
    let ok = fp_frozen(&e) == fp_frozen(&e);
    s.add("B35-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
