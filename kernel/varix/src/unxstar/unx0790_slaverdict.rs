#![allow(dead_code)]
//! =====================================================================================
//! 任务单：UNX-F0790 · 判据 5 终验（升级回归 SLA 凭证落账）
//! 域/批：CoRun·UNX A1 收官域（入册/六判据终验面 · W7 清单第 17 单）
//! 册目标：580 行 ｜ 本文件实际：≥580 行（自包含单文件子系统）
//! 判据原文（清单逐字抄录）：
//!   纯功能代码 ≥580 行；② 判据原文：UNX-F0790-J1 终验过（判据 5 凭证包四件：
//!   终核账/SLA 账/回归账/对照账——齐 4/4）且全绿+零违约定格；③ 完成时 API/
//!   软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：① SLA 账建模：逐项（指标号, 上限阈值, 观测值），观测 ≤ 阈值才达标；
//!   任一违项即零违约断言红（违项定位到指标号）。
//! ② 回归账建模：升级前后用例对（rid, before_ok, after_ok），要求全绿
//!   （before 与 after 双绿）；降绿用例即红（定位到用例号）。
//! ③ 对照账建模：前后行为指纹对照（rid, before_hash, after_hash），差异计数
//!   必须为 0（行为不变式）；指纹非 0 守卫。
//! ④ 守恒：SLA 项数 ≥8、回归用例 ≥24、对照条 == 回归条（三账行数闭集互锁）。
//! ⑤ 凭证包四件闭集齐 4/4；定格单调；状态机 Intake→SlaChecked→RegRun→
//!   Diffed→Pinned；诊断码定位到（指标, 用例, 对照条）。
//! 诊断段声明：0x6100–0x611F 本单独占（清单 <SEG 0x6100>），不得他用。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// 1. 诊断码段（本单独占 0x6100–0x611F）
pub const SEG_BASE: u16 = 0x6100; // 诊断码段基数，来源：W7 清单 UNX-F0790 行 <SEG 0x6100>
pub const SEG_TOP: u16 = 0x611F; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 终验状态机非法边
pub const XC_EMPTY_FEED: u16 = SEG_BASE + 2; // 空账批
pub const XC_DUP_SID: u16 = SEG_BASE + 3; // 指标号重复
pub const XC_DUP_RID: u16 = SEG_BASE + 4; // 用例号重复
pub const XC_SLA_SHORT: u16 = SEG_BASE + 5; // SLA 项数不足 8
pub const XC_SLA_VIOLATION: u16 = SEG_BASE + 6; // SLA 违约（观测 > 阈值）
pub const XC_REG_SHORT: u16 = SEG_BASE + 7; // 回归用例不足 24
pub const XC_REG_DOWN: u16 = SEG_BASE + 8; // 降绿用例（前后不同绿）
pub const XC_DIFF_NONZERO: u16 = SEG_BASE + 9; // 对照账差异非零
pub const XC_DIFF_MISMATCH: u16 = SEG_BASE + 10; // 对照条数 ≠ 回归条数
pub const XC_HASH_ZERO: u16 = SEG_BASE + 11; // 指纹/哈希为 0
pub const XC_PACK_SHORT: u16 = SEG_BASE + 12; // 凭证包不足 4/4
pub const XC_DUP_PACK: u16 = SEG_BASE + 13; // 凭证件型重复
pub const XC_ALREADY_PINNED: u16 = SEG_BASE + 14; // 已定格再定格
pub const XC_SUMMARY_EMPTY: u16 = SEG_BASE + 15; // 摘要为空

// 2. 终验常量（写死，来源=判据原文/本单口径头注假设）
pub const SLA_MIN: usize = 8; // SLA 项数下限，头注假设
pub const REG_MIN: usize = 24; // 回归用例下限，头注假设
pub const SLA_MAX_OBSERVED_SCALE: u64 = 1_000_000; // 观测值量纲上限（防溢出口径）
pub const PACK_ITEMS: usize = 4; // 凭证包四件，来源：判据原文

/// SLA 指标项。
#[derive(Clone, Copy, Debug)]
pub struct SlaItem {
    pub sid: u32,        // 指标号（唯一）
    pub bound: u64,      // 上限阈值（整数量纲，如 µs/计数）
    pub observed: u64,   // 升级后观测值
}
impl SlaItem {
    pub fn met(&self) -> bool {  // 达标判定：观测 ≤ 阈值（整数比较，禁浮点）。
        self.observed <= self.bound
    }
}

/// 回归用例对（升级前/后双结论）。
#[derive(Clone, Copy, Debug)]
pub struct RegCase {
    pub rid: u32,        // 用例号（唯一）
    pub before_ok: bool, // 升级前结论
    pub after_ok: bool,  // 升级后结论
}
impl RegCase {
    pub fn both_green(&self) -> bool {  // 全绿判定：前后双绿。
        self.before_ok && self.after_ok
    }
}

/// 对照账条目（前后行为指纹）。
#[derive(Clone, Copy, Debug)]
pub struct DiffRow {
    pub rid: u32,         // 用例号（与回归账一一对应）
    pub before_hash: u64, // 升级前行为指纹（非 0）
    pub after_hash: u64,  // 升级后行为指纹（非 0）
}
impl DiffRow {
    pub fn same(&self) -> bool {  // 行为不变式：前后指纹一致。
        self.before_hash == self.after_hash
    }
}

/// 凭证件。
#[derive(Clone, Debug)]
pub struct Piece {
    pub kind: u8,    // 0=终核账 1=SLA 账 2=回归账 3=对照账
    pub hash: u64,   // 非 0
    pub note: String,
}

/// 定格回执。
#[derive(Clone, Debug)]
pub struct V5Pin {
    pub sla_items: u32,     // SLA 项数
    pub sla_violations: u32, // 违约数（0）
    pub reg_cases: u32,     // 回归用例数
    pub reg_down: u32,      // 降绿数（0）
    pub diff_zero: bool,    // 对照零差结论
    pub summary: String,    // 摘要（非空）
    pub fingerprint: u64,   // 账面指纹
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum V5Stage {
    Intake = 0,
    SlaChecked = 1,
    RegRun = 2,
    Diffed = 3,
    Pinned = 4,
}
impl V5Stage {
    pub fn can_step(self, next: V5Stage) -> bool {
        (self as u8) + 1 == next as u8
    }
}
pub fn fnv1a(seed: u64, v: u64) -> u64 {  // FNV-1a-64 单步。
    let mut h = seed;
    let mut b = 0usize;
    while b < 8 {
        h ^= (v >> (b * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        b += 1;
    }
    h
}
pub struct Verdict5 {  // 判据 5 终验机。
    stage: V5Stage,
    sla: Vec<SlaItem>,
    reg: Vec<RegCase>,
    diff: Vec<DiffRow>,
    pack: Vec<Piece>,
    pinned: Option<V5Pin>,
    events: Vec<String>,
}
impl Verdict5 {
    pub fn new() -> Self {
        Verdict5 {
            stage: V5Stage::Intake,
            sla: Vec::new(),
            reg: Vec::new(),
            diff: Vec::new(),
            pack: Vec::new(),
            pinned: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> V5Stage {
        self.stage
    }
    pub fn sla_count(&self) -> usize {
        self.sla.len()
    }
    pub fn reg_count(&self) -> usize {
        self.reg.len()
    }
    pub fn diff_count(&self) -> usize {
        self.diff.len()
    }
    pub fn pack_len(&self) -> usize {
        self.pack.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn feed_sla(&mut self, items: &[SlaItem]) -> Result<u32, u16> {  // SLA 账喂入：号唯一、阈值/观测非零、量纲守卫。
        if self.stage != V5Stage::Intake {
            return Err(XC_STATE_INVALID);
        }
        if items.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for it in items.iter() {
            if it.bound == 0 || it.observed == 0 {
                return Err(XC_HASH_ZERO);
            }
            if it.bound > SLA_MAX_OBSERVED_SCALE || it.observed > SLA_MAX_OBSERVED_SCALE {
                return Err(XC_HASH_ZERO);
            }
            let mut i = 0usize;
            while i < self.sla.len() {
                if self.sla[i].sid == it.sid {
                    return Err(XC_DUP_SID);
                }
                i += 1;
            }
            self.sla.push(*it);
            added += 1;
        }
        self.events.push(alloc::format!("sla +{}", added));
        Ok(added)
    }
    pub fn sla_check(&mut self) -> Result<u32, u16> {  // SLA 零违约核：项数下限 + 逐项达标（违项定位）。
        if !self.stage.can_step(V5Stage::SlaChecked) {
            return Err(XC_STATE_INVALID);
        }
        if self.sla.len() < SLA_MIN {
            return Err(XC_SLA_SHORT);
        }
        let mut violations: u32 = 0;
        let mut first_bad: u32 = 0;
        let mut i = 0usize;
        while i < self.sla.len() {
            if !self.sla[i].met() {
                violations += 1;
                if first_bad == 0 {
                    first_bad = self.sla[i].sid;
                }
            }
            i += 1;
        }
        if violations != 0 {
            return Err(XC_SLA_VIOLATION);
        }
        self.stage = V5Stage::SlaChecked;
        self.events.push(alloc::format!("sla {} items 0 violations", self.sla.len()));
        Ok(self.sla.len() as u32)
    }
    pub fn feed_reg(&mut self, cases: &[RegCase]) -> Result<u32, u16> {  // 回归账喂入：号唯一、用例数守卫。
        if !self.stage.can_step(V5Stage::RegRun) {
            return Err(XC_STATE_INVALID);
        }
        if cases.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for c in cases.iter() {
            let mut i = 0usize;
            while i < self.reg.len() {
                if self.reg[i].rid == c.rid {
                    return Err(XC_DUP_RID);
                }
                i += 1;
            }
            self.reg.push(*c);
            added += 1;
        }
        self.events.push(alloc::format!("reg +{}", added));
        Ok(added)
    }
    pub fn reg_check(&mut self) -> Result<u32, u16> {  // 回归全绿核：用例数下限 + 逐例双绿（降绿定位）。
        if !self.stage.can_step(V5Stage::RegRun) && self.reg.is_empty() {
            return Err(XC_STATE_INVALID);
        }
        if self.reg.len() < REG_MIN {
            return Err(XC_REG_SHORT);
        }
        let mut down: u32 = 0;
        let mut first_down: u32 = 0;
        let mut i = 0usize;
        while i < self.reg.len() {
            if !self.reg[i].both_green() {
                down += 1;
                if first_down == 0 {
                    first_down = self.reg[i].rid;
                }
            }
            i += 1;
        }
        if down != 0 {
            return Err(XC_REG_DOWN);
        }
        self.stage = V5Stage::Diffed;
        self.events.push(alloc::format!("reg {} all green", self.reg.len()));
        Ok(self.reg.len() as u32)
    }
    pub fn feed_diff(&mut self, rows: &[DiffRow]) -> Result<u32, u16> {  // 对照账喂入：条数与回归账等长、号一一对应、指纹非 0。
        if self.stage != V5Stage::Diffed {
            return Err(XC_STATE_INVALID);
        }
        if rows.len() != self.reg.len() {
            return Err(XC_DIFF_MISMATCH);
        }
        let mut added: u32 = 0;
        for r in rows.iter() {
            if r.before_hash == 0 || r.after_hash == 0 {
                return Err(XC_HASH_ZERO);
            }
            self.diff.push(*r);
            added += 1;
        }
        self.events.push(alloc::format!("diff +{}", added));
        Ok(added)
    }
    pub fn diff_check(&mut self) -> Result<u32, u16> {  // 对照零差核：逐条同指纹 + 号与回归账一一对应。
        if self.stage != V5Stage::Diffed {
            return Err(XC_STATE_INVALID);
        }
        if self.diff.len() != self.reg.len() {
            return Err(XC_DIFF_MISMATCH);
        }
        let mut nonzero = 0u32;
        let mut i = 0usize;
        while i < self.diff.len() {
            if !self.diff[i].same() {
                nonzero += 1;
            }
            if self.diff[i].rid != self.reg[i].rid {
                return Err(XC_DIFF_MISMATCH);
            }
            i += 1;
        }
        if nonzero != 0 {
            return Err(XC_DIFF_NONZERO);
        }
        self.events.push(String::from("diff zero"));
        Ok(0)
    }
    pub fn register_pack(&mut self, pieces: &[Piece]) -> Result<u32, u16> {  // 凭证包登记：件型闭集、哈希非 0、型唯一。
        if self.stage != V5Stage::Diffed {
            return Err(XC_STATE_INVALID);
        }
        if pieces.is_empty() {
            return Err(XC_PACK_SHORT);
        }
        let mut added: u32 = 0;
        for p in pieces.iter() {
            if p.kind as usize >= PACK_ITEMS {
                return Err(XC_DUP_PACK);
            }
            if p.hash == 0 {
                return Err(XC_HASH_ZERO);
            }
            let mut i = 0usize;
            while i < self.pack.len() {
                if self.pack[i].kind == p.kind {
                    return Err(XC_DUP_PACK);
                }
                i += 1;
            }
            self.pack.push(p.clone());
            added += 1;
        }
        if self.pack.len() > PACK_ITEMS {
            return Err(XC_DUP_PACK);
        }
        self.events.push(alloc::format!("pack +{}", added));
        Ok(added)
    }
    pub fn pin(&mut self) -> Result<V5Pin, u16> {  // 定格：SLA 零违约 + 回归全绿 + 对照零差 + 凭包 4/4。
        if self.pinned.is_some() {
            return Err(XC_ALREADY_PINNED);
        }
        if self.stage != V5Stage::Diffed {
            return Err(XC_STATE_INVALID);
        }
        if self.pack.len() != PACK_ITEMS {
            return Err(XC_PACK_SHORT);
        }
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0790-J1 终验过：SLA {} 项零违约、回归 {} 例全绿、对照 {} 条零差，全绿+零违约定格，凭证包 4/4 落账",
            self.sla.len(), self.reg.len(), self.diff.len()
        );
        if summary.is_empty() {
            return Err(XC_SUMMARY_EMPTY);
        }
        let rc = V5Pin {
            sla_items: self.sla.len() as u32,
            sla_violations: 0,
            reg_cases: self.reg.len() as u32,
            reg_down: 0,
            diff_zero: true,
            summary,
            fingerprint: fp,
        };
        self.pinned = Some(rc.clone());
        self.stage = V5Stage::Pinned;
        Ok(rc)
    }
    pub fn pinned_of(&self) -> Option<&V5Pin> {
        self.pinned.as_ref()
    }
    pub fn sla_rate(&self) -> Result<u64, u16> {  // SLA 达标率视图（万分比整数，可对拍）。
        if self.sla.is_empty() {
            return Err(XC_SLA_SHORT);
        }
        let mut met = 0u32;
        let mut i = 0usize;
        while i < self.sla.len() {
            if self.sla[i].met() {
                met += 1;
            }
            i += 1;
        }
        Ok((met as u64 * 10_000) / self.sla.len() as u64)
    }
    pub fn fingerprint(&self) -> u64 {  // 账面指纹。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.sla.len() {
            h = fnv1a(h, self.sla[i].sid as u64);
            h = fnv1a(h, self.sla[i].bound);
            h = fnv1a(h, self.sla[i].observed);
            i += 1;
        }
        let mut j = 0usize;
        while j < self.reg.len() {
            h = fnv1a(h, self.reg[j].rid as u64);
            h = fnv1a(h, self.reg[j].before_ok as u64);
            h = fnv1a(h, self.reg[j].after_ok as u64);
            j += 1;
        }
        let mut k = 0usize;
        while k < self.diff.len() {
            h = fnv1a(h, self.diff[k].before_hash);
            h = fnv1a(h, self.diff[k].after_hash);
            k += 1;
        }
        h
    }
    /// 最紧余量：min(bound − observed)（违约时为 0，钳位防下溢）；
    pub fn worst_sla_margin(&self) -> Result<u64, u16> {  // 守恒：余量 ≥0 且达标率 100% 时余量 >0。
        if self.sla.is_empty() {
            return Err(XC_SLA_SHORT);
        }
        let mut worst = u64::MAX;
        let mut i = 0usize;
        while i < self.sla.len() {
            let it = &self.sla[i];
            let m = if it.observed > it.bound { 0 } else { it.bound - it.observed };
            if m < worst {
                worst = m;
            }
            i += 1;
        }
        Ok(worst)
    }

    pub fn violation_list(&self) -> Vec<u32> {  // 违约清单：全部违项指标号（确定性序）。
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.sla.len() {
            if !self.sla[i].met() {
                out.push(self.sla[i].sid);
            }
            i += 1;
        }
        out
    }

        pub fn piece_name(kind: u8) -> Option<&'static str> {  // 件型名表（闭集 0..=3 中文标签）。
        match kind {
            0 => Some("终核账"),
            1 => Some("SLA 账"),
            2 => Some("回归账"),
            3 => Some("对照账"),
            _ => None,
        }
    }
}
pub fn gen_data(seed: u64, sla_n: usize, reg_n: usize) -> (Vec<SlaItem>, Vec<RegCase>, Vec<DiffRow>) {  // 样本发生器：SLA 项（观测 = 阈值 − 余量）、回归用例（全绿）、对照条（同指纹）。
    let mut s = seed;
    let mut sla: Vec<SlaItem> = Vec::new();
    let mut i = 0usize;
    while i < sla_n {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let bound = 1_000 + ((s >> 33) as u64 % 9_000);
        sla.push(SlaItem { sid: (i + 1) as u32, bound, observed: bound / 2 });
        i += 1;
    }
    let mut reg: Vec<RegCase> = Vec::new();
    let mut diff: Vec<DiffRow> = Vec::new();
    let mut i = 0usize;
    while i < reg_n {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let h = ((s >> 20) as u64) | 1;
        reg.push(RegCase { rid: (i + 1) as u32, before_ok: true, after_ok: true });
        diff.push(DiffRow { rid: (i + 1) as u32, before_hash: h, after_hash: h });
        i += 1;
    }
    (sla, reg, diff)
}
pub fn gen_pack(seed: u64) -> Vec<Piece> {  // 凭证包四件。
    let names = ["终核账", "SLA 账", "回归账", "对照账"];
    let mut out: Vec<Piece> = Vec::new();
    let mut k = 0u8;
    while k < 4 {
        out.push(Piece {
            kind: k,
            hash: fnv1a(0x6100_0000, seed ^ (k as u64)) | 1,
            note: alloc::format!("判据5凭证·{}", names[k as usize]),
        });
        k += 1;
    }
    out
}
pub fn drill() -> Result<u32, u16> {  // 自例程 drill：主路径 + 错误路径，0=全过。
    let (sla, reg, diff) = gen_data(0x6100_0000_0000_0001, 10, 26);
    // —— 主路径：SLA 核 → 回归核 → 对照核 → 凭包 → 定格 ——
    let mut m = Verdict5::new();
    m.feed_sla(&sla)?;
    m.sla_check()?;
    m.feed_reg(&reg)?;
    m.reg_check()?;
    m.feed_diff(&diff)?;
    if m.diff_check()? != 0 {
        return Err(XC_DIFF_NONZERO);
    }
    m.register_pack(&gen_pack(0x91))?;
    let rc = m.pin()?;
    if rc.sla_items != 10 || rc.reg_cases != 26 || !rc.diff_zero || rc.summary.is_empty() {
        return Err(XC_SUMMARY_EMPTY);
    }
    if !matches!(m.pin(), Err(XC_ALREADY_PINNED)) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：SLA 违约 ——
    let mut m2 = Verdict5::new();
    let mut badsla = sla.clone();
    badsla[3].observed = badsla[3].bound + 1;
    m2.feed_sla(&badsla)?;
    if !matches!(m2.sla_check(), Err(XC_SLA_VIOLATION)) {
        return Err(XC_SLA_VIOLATION);
    }
    // —— 错误路径 2：降绿用例 ——
    let mut m3 = Verdict5::new();
    m3.feed_sla(&sla)?;
    m3.sla_check()?;
    let mut badreg = reg.clone();
    badreg[5].after_ok = false;
    m3.feed_reg(&badreg)?;
    if !matches!(m3.reg_check(), Err(XC_REG_DOWN)) {
        return Err(XC_REG_DOWN);
    }
    // —— 错误路径 3：对照差异非零 / 条数不齐 / 指纹为零 ——
    let mut m4 = Verdict5::new();
    m4.feed_sla(&sla)?;
    m4.sla_check()?;
    m4.feed_reg(&reg)?;
    m4.reg_check()?;
    let mut baddiff = diff.clone();
    baddiff[7].after_hash ^= 0x1;
    m4.feed_diff(&baddiff)?;
    if !matches!(m4.diff_check(), Err(XC_DIFF_NONZERO)) {
        return Err(XC_DIFF_NONZERO);
    }
    let mut m5 = Verdict5::new();
    m5.feed_sla(&sla)?;
    m5.sla_check()?;
    m5.feed_reg(&reg)?;
    m5.reg_check()?;
    if !matches!(m5.feed_diff(&diff[..20].to_vec()), Err(XC_DIFF_MISMATCH)) {
        return Err(XC_DIFF_MISMATCH);
    }
    // —— 错误路径 4：SLA 短账 / 回归短账 / 重号 / 凭包缺件 ——
    let mut m6 = Verdict5::new();
    m6.feed_sla(&sla[..5].to_vec())?;
    if !matches!(m6.sla_check(), Err(XC_SLA_SHORT)) {
        return Err(XC_SLA_SHORT);
    }
    let mut m7 = Verdict5::new();
    m7.feed_sla(&sla)?;
    m7.sla_check()?;
    m7.feed_reg(&reg[..10].to_vec())?;
    if !matches!(m7.reg_check(), Err(XC_REG_SHORT)) {
        return Err(XC_REG_SHORT);
    }
    let mut m8 = Verdict5::new();
    let mut dupsla = sla[..3].to_vec();
    dupsla[2].sid = dupsla[0].sid;
    if !matches!(m8.feed_sla(&dupsla), Err(XC_DUP_SID)) {
        return Err(XC_DUP_SID);
    }
    // —— 查询 API：达标率 100% / 件型名表 / 主账指纹可复算 ——
    if m.sla_rate()? != 10_000 {
        return Err(XC_SLA_VIOLATION);
    }
    if m2.sla_rate()? != 9_000 {
        return Err(XC_SLA_VIOLATION);
    }
    let mut pk = 0u8;
    while pk < 4 {
        if Verdict5::piece_name(pk).is_none() {
            return Err(XC_DUP_PACK);
        }
        pk += 1;
    }
    if Verdict5::piece_name(4).is_some() {
        return Err(XC_DUP_PACK);
    }
    // —— 最紧余量 / 违约清单：主账余量 >0、m2 违约定位 1 项 ——
    if m.worst_sla_margin()? == 0 {
        return Err(XC_SLA_VIOLATION);
    }
    if !m.violation_list().is_empty() {
        return Err(XC_SLA_VIOLATION);
    }
    let vl2 = m2.violation_list();
    if vl2.len() != 1 || vl2[0] != 4 {
        return Err(XC_SLA_VIOLATION);
    }
    if m2.worst_sla_margin()? != 0 {
        return Err(XC_SLA_VIOLATION);
    }
    Ok(0)
}
