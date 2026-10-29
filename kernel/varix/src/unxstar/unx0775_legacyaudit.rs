#![allow(dead_code)]
// =====================================================================================
// 任务单：UNX-F0775 · 遗留账面清点（零未决/零红账/零半态）
// 域/批：CoRun·UNX A1 收官域（跨域回执/风险登记/A1 域满收官主题 · W7 清单第 6 单）
// 册目标：570 行 ｜ 本文件实际：≥570 行（自包含单文件子系统）
// 判据原文（清单逐字抄录）：
//   纯功能代码 ≥570 行；② 判据原文：UNX-F0775-J1 清点过（未决违约零/红账残留
//   零/半态零三类清点断言过）且遗留清单（如有）逐项有处置路径；③ 完成时 API/
//   软件写回：状态=已完成、结果摘要非空。
// 设计纪律：① 三类红账分账建模：未决违约（violation 未闭环）、红账残留（debt
//   未赎回）、半态（状态机滞留过渡态），清点断言 = 三类计数全零才准清账。
// ② 遗留清单独立建模：合法遗留项必须逐项带处置通道（四通道闭集），无通道
//   遗留即红；遗留项不参与三类红账计数（记录性挂账）。
// ③ 守恒：总条目 == 净账 + 三类红账 + 合法遗留；清账前后账面指纹可对拍。
// ④ 状态机 Open→Swept→Classified→Disposed→Cleared 单向闭集，回头边拒绝。
// ⑤ 诊断码闭集定位到（账类, 条目号）；常量写死注来源。
// 诊断段声明：0x5FA0–0x5FBF 本单独占（清单 <SEG 0x5fa0>），不得他用。
// 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
// =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

// 1. 诊断码段（本单独占 0x5FA0–0x5FBF）
pub const SEG_BASE: u16 = 0x5FA0; // 诊断码段基数，来源：W7 清单 UNX-F0775 行 <SEG 0x5fa0>
pub const SEG_TOP: u16 = 0x5FBF; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 清点状态机非法边
pub const XC_EMPTY_FEED: u16 = SEG_BASE + 2; // 空账喂入
pub const XC_DUP_ID: u16 = SEG_BASE + 3; // 条目号重复（跨三类全域唯一）
pub const XC_OPEN_VIOLATION: u16 = SEG_BASE + 4; // 未决违约零断言红
pub const XC_RED_RESIDUE: u16 = SEG_BASE + 5; // 红账残留零断言红
pub const XC_HALF_STATE: u16 = SEG_BASE + 6; // 半态零断言红
pub const XC_LEFTOVER_NO_PATH: u16 = SEG_BASE + 7; // 遗留项无处置路径
pub const XC_LEFTOVER_BAD_CHANNEL: u16 = SEG_BASE + 8; // 处置通道越出四通道闭集
pub const XC_CLASS_MISMATCH: u16 = SEG_BASE + 9; // 守恒分类断裂
pub const XC_NOT_SWEPT: u16 = SEG_BASE + 10; // 未扫核即请求分类/清账
pub const XC_ALREADY_CLEARED: u16 = SEG_BASE + 11; // 已清账再请求
pub const XC_LEFTOVER_SHORT: u16 = SEG_BASE + 12; // 遗留清单越定容上限
pub const XC_SUM_OVERFLOW: u16 = SEG_BASE + 13; // 计数溢出
pub const XC_EVIDENCE_ZERO: u16 = SEG_BASE + 14; // 闭环条目证据为 0（虚闭环）
pub const XC_CLEAR_BLOCKED: u16 = SEG_BASE + 15; // 清账被红案阻止

// 2. 清点常量（写死，来源=判据原文/本单口径头注假设）
pub const CLASS_COUNT: usize = 3; // 三类红账：未决违约/红账残留/半态，来源：判据原文
pub const CHANNEL_COUNT: usize = 4; // 处置通道闭集数（勘误/重验/移交/挂观察），头注假设
pub const LEDGER_CAP: usize = 64; // 三类账定容上限（每类），头注假设
pub const LEFTOVER_CAP: usize = 16; // 遗留清单定容上限，头注假设

/// 账类闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LedgerClass {
    Violation = 0, // 违约账
    RedDebt = 1,   // 红账（待赎）
    HalfState = 2, // 半态账（滞留过渡态）
}
impl LedgerClass {
    pub fn zh(self) -> &'static str {
        match self {
            LedgerClass::Violation => "未决违约",
            LedgerClass::RedDebt => "红账残留",
            LedgerClass::HalfState => "半态",
        }
    }
}

/// 处置通道闭集：1=勘误回写 2=重验复走 3=移交下游 4=挂观察豁免（有据豁免）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DispositionChannel {
    Errata = 1,
    Reverify = 2,
    Handover = 3,
    WatchWaive = 4,
}
impl DispositionChannel {
    pub fn from_u8(v: u8) -> Option<DispositionChannel> {
        match v {
            1 => Some(DispositionChannel::Errata),
            2 => Some(DispositionChannel::Reverify),
            3 => Some(DispositionChannel::Handover),
            4 => Some(DispositionChannel::WatchWaive),
            _ => None,
        }
    }
    pub fn zh(self) -> &'static str {
        match self {
            DispositionChannel::Errata => "勘误回写",
            DispositionChannel::Reverify => "重验复走",
            DispositionChannel::Handover => "移交下游",
            DispositionChannel::WatchWaive => "挂观察豁免",
        }
    }
}

/// 一条三类账条目：closed==true 表示已闭环（须带证据），否则计红。
#[derive(Clone, Copy, Debug)]
pub struct ScrubEntry {
    pub eid: u32,          // 条目号（全域唯一）
    pub class: LedgerClass, // 账类
    pub closed: bool,      // 闭环标记
    pub evidence: u64,     // 闭环证据指纹（closed 时非 0）
}

/// 一条遗留清单项：合法遗留必须带处置通道与依据。
#[derive(Clone, Copy, Debug)]
pub struct LeftoverItem {
    pub lid: u32,                       // 遗留项号
    pub channel: u8,                    // 处置通道 1..=4（闭集外拒收）
    pub reason_hash: u64,               // 遗留依据哈希（非 0）
    pub owner_domain: u8,               // 责任域 1..=5
}

/// 三类清点报告。
#[derive(Clone, Debug)]
pub struct ScrubReport {
    pub open_violations: u32,  // 未决违约计数（断言：零）
    pub red_residues: u32,     // 红账残留计数（断言：零）
    pub half_states: u32,      // 半态计数（断言：零）
    pub clean: u32,            // 净账条目数
    pub first_red: Option<(LedgerClass, u32)>, // 首红定位（类, 条目号）
}

/// 清账终判。
#[derive(Clone, Debug, PartialEq)]
pub struct ClearVerdict {
    pub three_zero_ok: bool,   // 三类零断言结论
    pub leftover_ok: bool,     // 遗留逐项有处置结论
    pub total_entries: u32,    // 总条目守恒回执
    pub leftover_count: u32,   // 合法遗留数
    pub summary: String,       // 结论摘要（非空）
    pub fingerprint: u64,      // 账面指纹
}

/// 清点状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrubStage {
    Open = 0,
    Swept = 1,
    Classified = 2,
    Disposed = 3,
    Cleared = 4,
}
impl ScrubStage {
    pub fn can_step(self, next: ScrubStage) -> bool {
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
pub struct LegacyScrubber {  // 遗留账面清点机。
    stage: ScrubStage,
    entries: Vec<ScrubEntry>,   // 三类账（每类 ≤ LEDGER_CAP）
    leftover: Vec<LeftoverItem>, // 遗留清单（≤ LEFTOVER_CAP）
    seen_ids: Vec<u32>,          // 条目号定位账（与 entries 等长）
    report: Option<ScrubReport>,
    events: Vec<String>,
}
impl LegacyScrubber {
    pub fn new() -> Self {
        LegacyScrubber {
            stage: ScrubStage::Open,
            entries: Vec::new(),
            leftover: Vec::new(),
            seen_ids: Vec::new(),
            report: None,
            events: Vec::new(),
        }
    }
    pub fn stage(&self) -> ScrubStage {
        self.stage
    }
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
    pub fn leftover_count(&self) -> usize {
        self.leftover.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn feed_entries(&mut self, batch: &[ScrubEntry]) -> Result<u32, u16> {  // 喂入三类账条目批：号唯一、分类定容守卫。
        if self.stage != ScrubStage::Open {
            return Err(XC_STATE_INVALID);
        }
        if batch.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for e in batch.iter() {
            let mut per_class = 0usize;
            let mut i = 0usize;
            while i < self.entries.len() {
                if self.entries[i].class == e.class {
                    per_class += 1;
                }
                if self.seen_ids[i] == e.eid {
                    return Err(XC_DUP_ID);
                }
                i += 1;
            }
            if per_class >= LEDGER_CAP {
                return Err(XC_SUM_OVERFLOW);
            }
            self.seen_ids.push(e.eid);
            self.entries.push(*e);
            added += 1;
        }
        self.events.push(alloc::format!("feed +{}", added));
        Ok(added)
    }
    pub fn register_leftover(&mut self, items: &[LeftoverItem]) -> Result<u32, u16> {  // 登记遗留清单项：通道闭集、依据非零、责任域闭集、定容守卫。
        if self.stage != ScrubStage::Open {
            return Err(XC_STATE_INVALID);
        }
        if items.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for it in items.iter() {
            if self.leftover.len() >= LEFTOVER_CAP {
                return Err(XC_LEFTOVER_SHORT);
            }
            if DispositionChannel::from_u8(it.channel).is_none() {
                return Err(XC_LEFTOVER_BAD_CHANNEL);
            }
            if it.reason_hash == 0 {
                return Err(XC_LEFTOVER_NO_PATH);
            }
            if it.owner_domain < 1 || it.owner_domain > 5 {
                return Err(XC_LEFTOVER_BAD_CHANNEL);
            }
            self.leftover.push(*it);
            added += 1;
        }
        self.events.push(alloc::format!("leftover +{}", added));
        Ok(added)
    }
    pub fn sweep(&mut self) -> Result<ScrubReport, u16> {  // 三类清点扫核：逐条分类计数，闭环必须带证据（禁虚闭环）。
        if !self.stage.can_step(ScrubStage::Swept) {
            return Err(XC_STATE_INVALID);
        }
        let mut rep = ScrubReport {
            open_violations: 0,
            red_residues: 0,
            half_states: 0,
            clean: 0,
            first_red: None,
        };
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if e.closed {
                if e.evidence == 0 {
                    // 虚闭环：按未决计红。
                    if rep.first_red.is_none() {
                        rep.first_red = Some((e.class, e.eid));
                    }
                    match e.class {
                        LedgerClass::Violation => rep.open_violations += 1,
                        LedgerClass::RedDebt => rep.red_residues += 1,
                        LedgerClass::HalfState => rep.half_states += 1,
                    }
                } else {
                    rep.clean += 1;
                }
            } else {
                if rep.first_red.is_none() {
                    rep.first_red = Some((e.class, e.eid));
                }
                match e.class {
                    LedgerClass::Violation => rep.open_violations += 1,
                    LedgerClass::RedDebt => rep.red_residues += 1,
                    LedgerClass::HalfState => rep.half_states += 1,
                }
            }
            i += 1;
        }
        // 守恒：净账 + 三类红 == 总条目。
        let reds = rep.open_violations + rep.red_residues + rep.half_states;
        if rep.clean + reds != self.entries.len() as u32 {
            return Err(XC_CLASS_MISMATCH);
        }
        self.stage = ScrubStage::Swept;
        self.report = Some(rep.clone());
        self.events
            .push(alloc::format!("sweep v={} r={} h={}", rep.open_violations, rep.red_residues, rep.half_states));
        Ok(rep)
    }
    pub fn classify(&mut self) -> Result<u32, u16> {  // 遗留清单分类：逐项通道解引合法（三零断言 + 遗留处置双查前的分类面）。
        if !self.stage.can_step(ScrubStage::Classified) {
            return Err(XC_NOT_SWEPT);
        }
        let mut ok: u32 = 0;
        let mut i = 0usize;
        while i < self.leftover.len() {
            if DispositionChannel::from_u8(self.leftover[i].channel).is_none() {
                return Err(XC_LEFTOVER_BAD_CHANNEL);
            }
            ok += 1;
            i += 1;
        }
        self.stage = ScrubStage::Classified;
        self.events.push(alloc::format!("classify {}/{}", ok, self.leftover.len()));
        Ok(ok)
    }
    pub fn clear(&mut self) -> Result<ClearVerdict, u16> {  // 清账断言：三类全零 + 遗留逐项有处置 → 出终判。
        if !self.stage.can_step(ScrubStage::Disposed) {
            return Err(if self.stage == ScrubStage::Cleared {
                XC_ALREADY_CLEARED
            } else {
                XC_NOT_SWEPT
            });
        }
        // 遗留处置面：无遗留也合法（判据「如有」口径），有则逐项须有通道。
        let mut i = 0usize;
        while i < self.leftover.len() {
            if DispositionChannel::from_u8(self.leftover[i].channel).is_none() {
                return Err(XC_LEFTOVER_NO_PATH);
            }
            i += 1;
        }
        self.stage = ScrubStage::Disposed;
        let rep = match &self.report {
            Some(r) => r.clone(),
            None => return Err(XC_NOT_SWEPT),
        };
        let three_zero_ok =
            rep.open_violations == 0 && rep.red_residues == 0 && rep.half_states == 0;
        if !three_zero_ok {
            return Err(match rep.first_red {
                Some((LedgerClass::Violation, _)) => XC_OPEN_VIOLATION,
                Some((LedgerClass::RedDebt, _)) => XC_RED_RESIDUE,
                Some((LedgerClass::HalfState, _)) => XC_HALF_STATE,
                None => XC_CLEAR_BLOCKED,
            });
        }
        let summary = alloc::format!(
            "UNX-F0775-J1 清点过：未决违约零/红账残留零/半态零三类断言过，净账 {} 条，合法遗留 {} 条逐项有处置",
            rep.clean,
            self.leftover.len()
        );
        let fp = self.fingerprint();
        self.stage = ScrubStage::Cleared;
        Ok(ClearVerdict {
            three_zero_ok,
            leftover_ok: true,
            total_entries: self.entries.len() as u32,
            leftover_count: self.leftover.len() as u32,
            summary,
            fingerprint: fp,
        })
    }
    pub fn fingerprint(&self) -> u64 {  // 账面指纹：全部（eid, class, closed, evidence）规范序。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            h = fnv1a(h, e.eid as u64);
            h = fnv1a(h, e.class as u64);
            h = fnv1a(h, e.closed as u64);
            h = fnv1a(h, e.evidence);
            i += 1;
        }
        h
    }
    pub fn entry_of(&self, eid: u32) -> Option<ScrubEntry> {  // 按条目号查询（未登记 None）。
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].eid == eid {
                return Some(self.entries[i]);
            }
            i += 1;
        }
        None
    }

    pub fn class_count(&self, class: LedgerClass) -> Result<u32, u16> {  // 分类计数查询（扫核前调用返回 Err）。
        let rep = match &self.report {
            Some(r) => r,
            None => return Err(XC_NOT_SWEPT),
        };
        Ok(match class {
            LedgerClass::Violation => rep.open_violations,
            LedgerClass::RedDebt => rep.red_residues,
            LedgerClass::HalfState => rep.half_states,
        })
    }

    pub fn red_entries(&self) -> Vec<(LedgerClass, u32)> {  // 红案清单：全部未闭环/虚闭环条目的（类, 号）列表（确定性序）。
        let mut out: Vec<(LedgerClass, u32)> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            let e = &self.entries[i];
            if !e.closed || (e.closed && e.evidence == 0) {
                out.push((e.class, e.eid));
            }
            i += 1;
        }
        out
    }

    pub fn disposition_plan(&self) -> Result<Vec<String>, u16> {  // 遗留处置计划：逐项生成「通道 × 责任域 × 依据」计划行（写回面用）。
        if self.leftover.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut plan: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.leftover.len() {
            let it = &self.leftover[i];
            let ch = match DispositionChannel::from_u8(it.channel) {
                Some(c) => c,
                None => return Err(XC_LEFTOVER_BAD_CHANNEL),
            };
            plan.push(alloc::format!(
                "L{} 走[{}] 责任域{} 依据{:#x}",
                it.lid, ch.zh(), it.owner_domain, it.reason_hash
            ));
            i += 1;
        }
        Ok(plan)
    }

    /// 豁免审计：通道 4（挂观察豁免）责任域必须 ∈ 4..=5（核心域 1..=3 禁豁免，
    pub fn waive_audit(&self) -> Result<u32, u16> {  // 本单口径，头注假设）。
        let mut waived: u32 = 0;
        let mut i = 0usize;
        while i < self.leftover.len() {
            let it = &self.leftover[i];
            if it.channel == 4 {
                if it.owner_domain <= 3 {
                    return Err(XC_LEFTOVER_BAD_CHANNEL);
                }
                waived += 1;
            }
            i += 1;
        }
        Ok(waived)
    }
}
pub fn gen_clean(seed: u64, n: usize) -> (Vec<ScrubEntry>, Vec<LeftoverItem>) {  // 样本发生器：全净账 + 合法遗留（确定性）。
    let mut s = seed;
    let mut entries: Vec<ScrubEntry> = Vec::new();
    let mut eid = 1u32;
    let mut i = 0usize;
    while i < n {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        let class = match (s >> 33) % 3 {
            0 => LedgerClass::Violation,
            1 => LedgerClass::RedDebt,
            _ => LedgerClass::HalfState,
        };
        entries.push(ScrubEntry {
            eid,
            class,
            closed: true,
            evidence: 0xE5A0_0000_0000_0000 | eid as u64,
        });
        eid += 1;
        i += 1;
    }
    let leftover = vec![
        LeftoverItem { lid: 9001, channel: 1, reason_hash: 0x5FA0_0001, owner_domain: 1 },
        LeftoverItem { lid: 9002, channel: 2, reason_hash: 0x5FA0_0002, owner_domain: 2 },
    ];
    (entries, leftover)
}

pub fn drill() -> Result<u32, u16> {  // 虚闭环、遗留无通道、重复号、清账重入），0=全过。  // 自例程 drill：主路径（全净账清账）+ 错误路径（三类红逐项检出、
    // —— 主路径 ——
    let (entries, leftover) = gen_clean(0x5FA0_0000_5CAB_0001, 30);
    let mut m = LegacyScrubber::new();
    m.feed_entries(&entries)?;
    m.register_leftover(&leftover)?;
    let rep = m.sweep()?;
    if rep.open_violations != 0 || rep.red_residues != 0 || rep.half_states != 0 || rep.clean != 30 {
        return Err(XC_CLASS_MISMATCH);
    }
    if m.classify()? != 2 {
        return Err(XC_LEFTOVER_BAD_CHANNEL);
    }
    let v = m.clear()?;
    if !v.three_zero_ok || !v.leftover_ok || v.summary.is_empty() || v.total_entries != 30 {
        return Err(XC_CLEAR_BLOCKED);
    }
    if m.clear() != Err(XC_ALREADY_CLEARED) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：未决违约红 ——
    let (mut e2, l2) = gen_clean(0x5FA0_0000_5CAB_0002, 10);
    e2[3].closed = false;
    let mut m2 = LegacyScrubber::new();
    m2.feed_entries(&e2)?;
    m2.register_leftover(&l2)?;
    m2.sweep()?;
    m2.classify()?;
    if m2.clear() != Err(XC_OPEN_VIOLATION) {
        return Err(XC_OPEN_VIOLATION);
    }
    // —— 错误路径 2：红账残留红 ——
    let (mut e3, l3) = gen_clean(0x5FA0_0000_5CAB_0003, 10);
    e3[5].evidence = 0; // 虚闭环 → 红账残留
    let mut m3 = LegacyScrubber::new();
    m3.feed_entries(&e3)?;
    m3.register_leftover(&l3)?;
    m3.sweep()?;
    m3.classify()?;
    if m3.clear() != Err(XC_RED_RESIDUE) {
        return Err(XC_RED_RESIDUE);
    }
    // —— 错误路径 3：半态红 ——
    let (mut e4, l4) = gen_clean(0x5FA0_0000_5CAB_0004, 10);
    e4[7].closed = false;
    e4[7].class = LedgerClass::HalfState;
    let mut m4 = LegacyScrubber::new();
    m4.feed_entries(&e4)?;
    m4.register_leftover(&l4)?;
    m4.sweep()?;
    m4.classify()?;
    if m4.clear() != Err(XC_HALF_STATE) {
        return Err(XC_HALF_STATE);
    }
    // —— 错误路径 5：遗留通道越界 / 依据为零 ——
    let mut m5 = LegacyScrubber::new();
    let bad_ch = vec![LeftoverItem { lid: 1, channel: 9, reason_hash: 1, owner_domain: 1 }];
    if m5.register_leftover(&bad_ch) != Err(XC_LEFTOVER_BAD_CHANNEL) {
        return Err(XC_LEFTOVER_BAD_CHANNEL);
    }
    let mut m6 = LegacyScrubber::new();
    let no_reason = vec![LeftoverItem { lid: 2, channel: 2, reason_hash: 0, owner_domain: 1 }];
    if m6.register_leftover(&no_reason) != Err(XC_LEFTOVER_NO_PATH) {
        return Err(XC_LEFTOVER_NO_PATH);
    }
    // —— 错误路径 6：条目号重复 ——
    let mut m7 = LegacyScrubber::new();
    let dup = vec![entries[0], entries[0]];
    if m7.feed_entries(&dup) != Err(XC_DUP_ID) {
        return Err(XC_DUP_ID);
    }
    // —— 错误路径 7：未扫核即清账 / 空喂入 ——
    let mut m8 = LegacyScrubber::new();
    m8.feed_entries(&entries[..3].to_vec())?;
    if m8.clear() != Err(XC_NOT_SWEPT) {
        return Err(XC_NOT_SWEPT);
    }
    if m8.feed_entries(&[]) != Err(XC_EMPTY_FEED) {
        return Err(XC_EMPTY_FEED);
    }
    // —— 查询 API：主账按号可解引 ——
    match m.entry_of(1) {
        Some(e) if e.evidence != 0 => {}
        _ => return Err(XC_EVIDENCE_ZERO),
    }
    if m.entry_of(9999).is_some() {
        return Err(XC_DUP_ID);
    }
    // —— 扩展 API 面：分类计数 / 红案清单 / 处置计划 / 豁免审计 ——
    if m.class_count(LedgerClass::Violation)? != 0
        || m.class_count(LedgerClass::RedDebt)? != 0
        || m.class_count(LedgerClass::HalfState)? != 0
    {
        return Err(XC_CLASS_MISMATCH);
    }
    if !m.red_entries().is_empty() {
        return Err(XC_OPEN_VIOLATION);
    }
    let plan = m.disposition_plan()?;
    if plan.len() != 2 {
        return Err(XC_LEFTOVER_SHORT);
    }
    if m.waive_audit()? != 0 {
        return Err(XC_LEFTOVER_BAD_CHANNEL);
    }
    // —— 豁免审计红案：核心域 2 走豁免通道必拒 ——
    let mut m9 = LegacyScrubber::new();
    let bad_waive = vec![LeftoverItem { lid: 3, channel: 4, reason_hash: 7, owner_domain: 2 }];
    m9.register_leftover(&bad_waive)?;
    if m9.waive_audit() != Err(XC_LEFTOVER_BAD_CHANNEL) {
        return Err(XC_LEFTOVER_BAD_CHANNEL);
    }
    // —— 红案清单可定位：m2 的未决违约条目号 4 必在清单 ——
    let reds2 = m2.red_entries();
    if reds2.len() != 1 || reds2[0].1 != 4 || reds2[0].0 != LedgerClass::Violation {
        return Err(XC_OPEN_VIOLATION);
    }
    Ok(0)
}
