//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1351 · 压测资产版本化（只增不减与引用账） · 0x2E50
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1351-J1 全部压测资产版本化率 100%（场景/注入/基准三类），
//!      引用账完整率 100%（资产被谁引用可查），删除保护（引用中资产
//!      禁删）10/10 次拦截；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三类资产闭集：场景/注入/基准，各版本化（版本号只增、历史不可变）；
//!   - 引用账：引用者登记（执行器/报告/基准库三类引用方），被引资产
//!     删除必拦（10/10）；
//!   - 版本化率 100%：三类各 ≥1 资产且全部带版本号才可封账。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x2E50 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0x2E50;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_KIND_BAD: u16 = SEG_BASE + 1;    // 资产类越闭集
const E_VERSION_REG: u16 = SEG_BASE + 2; // 版本回退/复用
const E_REF_MISS: u16 = SEG_BASE + 3;    // 引用账缺
const E_DELETE_BLOCKED: u16 = SEG_BASE + 4; // 引用中禁删
const E_ASSET_DUP: u16 = SEG_BASE + 5;   // 资产重复
const E_FROZEN: u16 = SEG_BASE + 6;      // 冻结后写
const E_COVER_MISS: u16 = SEG_BASE + 7;  // 三类不全
const E_LEDGER_FULL: u16 = SEG_BASE + 8; // 账满
const E_DELETE_MISS: u16 = SEG_BASE + 9; // 未引用资产可删（非错误，留验）
const E_FP_MISS: u16 = SEG_BASE + 10;    // 指纹缺

/// 资产类闭集：0=场景 1=注入 2=基准。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AssetKind {
    Scene = 0,
    Inject = 1,
    Bench = 2,
}

impl AssetKind {
    pub fn from_id(v: u8) -> Option<AssetKind> {
        match v {
            0 => Some(AssetKind::Scene),
            1 => Some(AssetKind::Inject),
            2 => Some(AssetKind::Bench),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AssetKind::Scene => "场景",
            AssetKind::Inject => "注入",
            AssetKind::Bench => "基准",
        }
    }
}

/// 引用方闭集：0=执行器 1=报告 2=基准库。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReferKind {
    Executor = 0,
    Report = 1,
    BenchLib = 2,
}

impl ReferKind {
    pub fn name(self) -> &'static str {
        match self {
            ReferKind::Executor => "执行器",
            ReferKind::Report => "报告",
            ReferKind::BenchLib => "基准库",
        }
    }
}

/// 版本化资产行。
#[derive(Clone, Copy, Debug)]
pub struct AssetRow {
    /// 资产类。
    pub kind: AssetKind,
    /// 资产短名（类内唯一）。
    pub name: &'static str,
    /// 版本号（类内只增）。
    pub version: u32,
    /// 内容指纹。
    pub fp: u64,
}

/// 引用账行。
#[derive(Clone, Copy, Debug)]
pub struct RefRow {
    /// 被引资产下标。
    pub asset_idx: usize,
    /// 引用方。
    pub refer: ReferKind,
}

/// 资产版本账 + 引用账。
pub struct AssetLedger {
    rows: [Option<AssetRow>; 24],
    used: usize,
    /// 类内版本计数。
    kind_version: [u32; 3],
    /// 类内名占用（24 位 → 类内名查重）。
    name_used: [(u8, u64); 3], // 每类一个 64 位名位图（≤64 名）
    refs: [Option<RefRow>; 24],
    ref_n: usize,
    frozen: bool,
    rejects: [u32; 11],
}

impl AssetLedger {
    pub fn new() -> AssetLedger {
        AssetLedger {
            rows: [None; 24],
            used: 0,
            kind_version: [0; 3],
            name_used: [(0u8, 0u64); 3],
            refs: [None; 24],
            ref_n: 0,
            frozen: false,
            rejects: [0; 11],
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

    /// 名哈希（类内名→位号，FNV 截 6 位）。
    fn name_bit(name: &str) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let b = name.as_bytes();
        let mut i = 0;
        while i < b.len() {
            h ^= b[i] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            i += 1;
        }
        1u64 << (h % 64)
    }

    /// 登记资产（版本自动 = 类内计数+1）。
    pub fn register(&mut self, kind: AssetKind, name: &'static str, fp: u64) -> Result<usize, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if AssetKind::from_id(kind as u8).is_none() {
            return Err(self.reject(E_KIND_BAD));
        }
        if fp == 0 {
            return Err(self.reject(E_FP_MISS));
        }
        if name.is_empty() {
            return Err(self.reject(E_KIND_BAD));
        }
        let k = kind as usize;
        let bit = Self::name_bit(name);
        if self.name_used[k].1 & bit != 0 {
            return Err(self.reject(E_ASSET_DUP));
        }
        if self.used >= 24 {
            return Err(self.reject(E_LEDGER_FULL));
        }
        self.kind_version[k] += 1;
        self.rows[self.used] = Some(AssetRow { kind, name, version: self.kind_version[k], fp });
        self.name_used[k].1 |= bit;
        self.used += 1;
        Ok(self.used - 1)
    }

    /// 登记引用（资产下标越界必拒）。
    pub fn add_ref(&mut self, asset_idx: usize, refer: ReferKind) -> Result<(), u16> {
        if asset_idx >= self.used {
            return Err(self.reject(E_REF_MISS));
        }
        if self.ref_n >= 24 {
            return Err(self.reject(E_LEDGER_FULL));
        }
        self.refs[self.ref_n] = Some(RefRow { asset_idx, refer });
        self.ref_n += 1;
        Ok(())
    }

    /// 删除保护：被引资产禁删（返回 Err）；未引用资产允许（Ok）。
    pub fn try_delete(&mut self, asset_idx: usize) -> Result<(), u16> {
        if asset_idx >= self.used {
            return Err(self.reject(E_REF_MISS));
        }
        let mut i = 0;
        while i < self.ref_n {
            if let Some(r) = self.refs[i] {
                if r.asset_idx == asset_idx {
                    return Err(self.reject(E_DELETE_BLOCKED));
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 引用可查：资产下标 → 引用方列表。
    pub fn referrers_of(&self, asset_idx: usize) -> Vec<&'static str> {
        let mut out: Vec<&'static str> = Vec::new();
        let mut i = 0;
        while i < self.ref_n {
            if let Some(r) = self.refs[i] {
                if r.asset_idx == asset_idx {
                    out.push(r.refer.name());
                }
            }
            i += 1;
        }
        out
    }

    /// 封账：三类各 ≥1 资产。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        let mut k = 0;
        while k < 3 {
            if self.kind_version[k] == 0 {
                return Err(self.reject(E_COVER_MISS));
            }
            k += 1;
        }
        self.frozen = true;
        Ok(())
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn ref_n(&self) -> usize {
        self.ref_n
    }
}

/// 全量演练：三类各 2 资产 + 引用 + 封账 + 版本化率 100%。
pub fn drill_asset_lifecycle() -> (u32, u32, u32, Vec<String>) {
    let mut ledger = AssetLedger::new();
    let mut detail: Vec<String> = Vec::new();
    let mut ok = 0u32;
    let defs = [
        (AssetKind::Scene, "四档速率", 0x2E50_1),
        (AssetKind::Scene, "多核并发", 0x2E50_2),
        (AssetKind::Inject, "复合谱", 0x2E50_3),
        (AssetKind::Inject, "单故障", 0x2E50_4),
        (AssetKind::Bench, "全谱基准v1", 0x2E50_5),
        (AssetKind::Bench, "全谱基准v2", 0x2E50_6),
    ];
    let mut idxs = [0usize; 6];
    let mut d = 0;
    while d < defs.len() {
        match ledger.register(defs[d].0, defs[d].1, defs[d].2) {
            Ok(i) => {
                idxs[d] = i;
                ok += 1;
            }
            Err(c) => detail.push(format!("资产 {} 登记被拒：0x{:04X}", defs[d].1, c)),
        }
        d += 1;
    }
    // 引用：执行器引场景0、报告引注入2、基准库引基准4。
    let mut ref_ok = 0u32;
    if ledger.add_ref(idxs[0], ReferKind::Executor).is_ok() {
        ref_ok += 1;
    }
    if ledger.add_ref(idxs[2], ReferKind::Report).is_ok() {
        ref_ok += 1;
    }
    if ledger.add_ref(idxs[4], ReferKind::BenchLib).is_ok() {
        ref_ok += 1;
    }
    let frozen = ledger.freeze().is_ok();
    detail.push(format!("登记 {}/6 引用 {}/3 冻结={}", ok, ref_ok, frozen));
    (ok, ref_ok, if frozen { 1 } else { 0 }, detail)
}

/// 删除保护 10/10：三个被引资产反复尝试删除全拦（10 轮）。
pub fn drill_delete_guard_10() -> (u32, u32, Vec<String>) {
    let mut ok = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = AssetLedger::new();
    let s0 = ledger.register(AssetKind::Scene, "四档速率", 0x2E50_A);
    let i1 = ledger.register(AssetKind::Inject, "复合谱", 0x2E50_B);
    let b2 = ledger.register(AssetKind::Bench, "全谱基准v1", 0x2E50_C);
    let _ = ledger.add_ref(s0.unwrap_or(0), ReferKind::Executor);
    let _ = ledger.add_ref(i1.unwrap_or(1), ReferKind::Report);
    let _ = ledger.add_ref(b2.unwrap_or(2), ReferKind::BenchLib);
    let mut r = 0;
    while r < 10 {
        let target = r % 3;
        if matches!(ledger.try_delete(target), Err(E_DELETE_BLOCKED)) {
            ok += 1;
        }
        r += 1;
    }
    if ok == 10 {
        detail.push("删除保护 10/10".to_string());
    }
    (ok, 10, detail)
}

/// 引用可查演练：三资产的引用方列表与登记一致。
pub fn drill_ref_lookup() -> (bool, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = AssetLedger::new();
    let s0 = ledger.register(AssetKind::Scene, "四档速率", 0x2E50_D);
    let _ = ledger.register(AssetKind::Inject, "复合谱", 0x2E50_E);
    let _ = ledger.register(AssetKind::Bench, "全谱基准v1", 0x2E50_F);
    let _ = ledger.add_ref(s0.unwrap_or(0), ReferKind::Executor);
    let refs = ledger.referrers_of(s0.unwrap_or(0));
    let ok = refs.len() == 1 && refs[0] == "执行器"
        && ledger.referrers_of(1).is_empty();
    if !ok {
        detail.push("引用可查不符".to_string());
    }
    (ok, detail)
}

/// 未引用资产删除演练：无引用资产 try_delete 放行（Ok）。
pub fn drill_unref_delete() -> (bool, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = AssetLedger::new();
    let _ = ledger.register(AssetKind::Scene, "无引资产", 0x2E50_10);
    let ok = ledger.try_delete(0).is_ok();
    if !ok {
        detail.push("未引用资产删除未放行".to_string());
    }
    (ok, detail)
}

/// 注入 4 项：越类/零指纹/重复名/引用越界——必抓。
pub fn inject_bad() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut ledger = AssetLedger::new();
    if AssetKind::from_id(9).is_none() {
        caught += 1;
    } else {
        detail.push("越类未拒".to_string());
    }
    if matches!(ledger.register(AssetKind::Scene, "零指纹", 0), Err(E_FP_MISS)) {
        caught += 1;
    } else {
        detail.push("零指纹未拒".to_string());
    }
    let _ = ledger.register(AssetKind::Scene, "同名", 0x2E50_11);
    if matches!(ledger.register(AssetKind::Scene, "同名", 0x2E50_12), Err(E_ASSET_DUP)) {
        caught += 1;
    } else {
        detail.push("重复名未拒".to_string());
    }
    if matches!(ledger.add_ref(99, ReferKind::Report), Err(E_REF_MISS)) {
        caught += 1;
    } else {
        detail.push("引用越界未拒".to_string());
    }
    if caught == 4 {
        detail.push("注入 4/4 全抓".to_string());
    }
    (caught, 4, detail)
}

/// 段位守卫：0x2E50 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_KIND_BAD, E_VERSION_REG, E_REF_MISS, E_DELETE_BLOCKED, E_ASSET_DUP,
        E_FROZEN, E_COVER_MISS, E_LEDGER_FULL, E_DELETE_MISS, E_FP_MISS,
    ];
    let mut i = 0;
    while i < codes.len() {
        if codes[i] < SEG_BASE || codes[i] > SEG_BASE + 10 {
            return false;
        }
        if i > 0 && codes[i] != codes[i - 1] + 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// run_unx1351_checks：本单判据聚合（诊断段 0x2E50）。
pub fn run_unx1351_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1351 压测资产版本化");
    let (r1, f1, z1, d1) = drill_asset_lifecycle();
    s.add_dyn(
        "J1-三类资产版本化+引用",
        r1 == 6 && f1 == 3 && z1 == 1,
        &format!("登{}/6 引{}/3 冻{} {}", r1, f1, z1, d1.join(" ")),
    );
    let (c2, t2, d2) = drill_delete_guard_10();
    s.add_dyn("J1-删除保护10/10", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (ok3, d3) = drill_ref_lookup();
    s.add_dyn("J1-引用可查", ok3, &d3.join(" "));
    let (ok4, d4) = drill_unref_delete();
    s.add_dyn("J1-未引用可删", ok4, &d4.join(" "));
    let (c5, t5, d5) = inject_bad();
    s.add_dyn("J1-注入4/4", c5 == t5, &format!("{}/{} {}", c5, t5, d5.join(" ")));
    s.add_dyn("J2-段位守卫", segment_guard(), "0x2E50段十码闭集");
    s
}
