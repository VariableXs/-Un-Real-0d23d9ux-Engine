#![allow(dead_code)]
// =====================================================================================
// 任务单：UNX-F0776 · 收官风险登记簿（终态风险+缓解到位核）
// 域/批：CoRun·UNX A1 收官域（跨域回执/风险登记/A1 域满收官主题 · W7 清单第 7 单）
// 册目标：580 行 ｜ 本文件实际：≥580 行（自包含单文件子系统）
// 判据原文（清单逐字抄录）：
//   纯功能代码 ≥580 行；② 判据原文：UNX-F0776-J1 登记簿齐（终态风险逐项：风险/
//   等级/缓解/残余四列 ≥8 项）且缓解到位核过、高危零未缓解；③ 完成时 API/软件
//   写回：状态=已完成、结果摘要非空。
// 设计纪律：① 四列建模：风险标识 / 等级（高中低三档闭集）/ 缓解措施（通道 ×
//   强度）/ 残余等级；登记簿定容 16，≥8 项断言不过不出证。
// ② 缓解到位判据（整数阈值写死）：缓解强度 ≥2 且残余 ≤ 该档上限（高→1，
//   中→2，低→3）；任一 High 未缓解到位即「高危零未缓解」断言红。
// ③ 守恒：登记数 = 到位数 + 未到位数；每风险号唯一；指纹对拍可复算。
// ④ 状态机 Open→Registered→Graded→Mitigated→Attested 单向闭集。
// ⑤ 诊断码闭集定位到风险号；常量写死注来源。
// 诊断段声明：0x5FC0–0x5FDF 本单独占（清单 <SEG 0x5fc0>），不得他用。
// 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
// =====================================================================================
extern crate alloc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::vec;

// 1. 诊断码段（本单独占 0x5FC0–0x5FDF）
pub const SEG_BASE: u16 = 0x5FC0; // 诊断码段基数，来源：W7 清单 UNX-F0776 行 <SEG 0x5fc0>
pub const SEG_TOP: u16 = 0x5FDF; // 诊断码段顶界（含）
pub const XC_STATE_INVALID: u16 = SEG_BASE + 1; // 登记簿状态机非法边
pub const XC_EMPTY_FEED: u16 = SEG_BASE + 2; // 空批喂入
pub const XC_DUP_RID: u16 = SEG_BASE + 3; // 风险号重复
pub const XC_LEDGER_FULL: u16 = SEG_BASE + 4; // 越定容 16
pub const XC_COUNT_SHORT: u16 = SEG_BASE + 5; // 登记数不足 8（≥8 断言红）
pub const XC_GRADE_BAD: u16 = SEG_BASE + 6; // 等级越出三档闭集
pub const XC_CHANNEL_BAD: u16 = SEG_BASE + 7; // 缓解通道越出闭集
pub const XC_STRENGTH_WEAK: u16 = SEG_BASE + 8; // 缓解强度 <2（未到位）
pub const XC_RESIDUAL_HIGH: u16 = SEG_BASE + 9; // 残余超档位上限（未到位）
pub const XC_HIGH_UNMITIGATED: u16 = SEG_BASE + 10; // 高危未缓解断言红
pub const XC_NOT_REGISTERED: u16 = SEG_BASE + 11; // 未登记齐即请求核
pub const XC_ALREADY_ATTESTED: u16 = SEG_BASE + 12; // 已出证再请求
pub const XC_RISK_ZERO: u16 = SEG_BASE + 13; // 风险标识为 0（非法占位）
pub const XC_COUNT_MISMATCH: u16 = SEG_BASE + 14; // 到位守恒断裂
pub const XC_NOTE_EMPTY: u16 = SEG_BASE + 15; // 出证摘要为空（写回面违约）

// 2. 登记簿常量（写死，来源=判据原文/本单口径头注假设）
pub const RISK_CAP: usize = 16; // 登记簿定容 16，头注假设
pub const RISK_MIN: usize = 8; // ≥8 项断言，来源：判据原文「≥8 项」
pub const MITIGATION_MIN_STRENGTH: u8 = 2; // 缓解强度下限，本单口径（头注假设）
pub const RESIDUAL_CAP_HIGH: u8 = 1; // 高危残余上限，本单口径
pub const RESIDUAL_CAP_MID: u8 = 2; // 中危残余上限，本单口径
pub const RESIDUAL_CAP_LOW: u8 = 3; // 低危残余上限，本单口径
pub const MIT_CHANNEL_COUNT: u8 = 4; // 缓解通道闭集：1=代码防御 2=流程守卫 3=监控告警 4=文档声明

/// 风险等级三档闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RiskGrade {
    High = 0,
    Mid = 1,
    Low = 2,
}
impl RiskGrade {
    pub fn residual_cap(self) -> u8 {
        match self {
            RiskGrade::High => RESIDUAL_CAP_HIGH,
            RiskGrade::Mid => RESIDUAL_CAP_MID,
            RiskGrade::Low => RESIDUAL_CAP_LOW,
        }
    }
    pub fn zh(self) -> &'static str {
        match self {
            RiskGrade::High => "高",
            RiskGrade::Mid => "中",
            RiskGrade::Low => "低",
        }
    }
}

/// 一条终态风险登记（四列：风险/等级/缓解/残余）。
#[derive(Clone, Copy, Debug)]
pub struct RiskEntry {
    pub rid: u32,          // 风险号（唯一）
    pub risk_hash: u64,    // 风险标识指纹（非 0；四列之一：风险）
    pub grade: u8,         // 等级：0=高 1=中 2=低（四列之二）
    pub channel: u8,       // 缓解通道 1..=4（四列之三·半）
    pub strength: u8,      // 缓解强度 0..=3（四列之三·半）
    pub residual: u8,      // 残余等级 0..=3（四列之四）
}
impl RiskEntry {
    pub fn grade_of(&self) -> Result<RiskGrade, u16> {  // 等级闭集解引。
        match self.grade {
            0 => Ok(RiskGrade::High),
            1 => Ok(RiskGrade::Mid),
            2 => Ok(RiskGrade::Low),
            _ => Err(XC_GRADE_BAD),
        }
    }
    pub fn mitigated_ok(&self) -> Result<bool, u16> {  // 缓解到位单条判据：通道闭集 + 强度 ≥2 + 残余 ≤ 档位上限。
        if self.channel < 1 || self.channel > MIT_CHANNEL_COUNT {
            return Err(XC_CHANNEL_BAD);
        }
        let g = self.grade_of()?;
        Ok(self.strength >= MITIGATION_MIN_STRENGTH && self.residual <= g.residual_cap())
    }
}

/// 出证报告。
#[derive(Clone, Debug, PartialEq)]
pub struct RiskAttest {
    pub registered: u32,     // 登记数（≥8）
    pub mitigated: u32,      // 到位数
    pub unmitigated: u32,    // 未到位数（守恒：和 == 登记数）
    pub high_total: u32,     // 高危总数
    pub high_unmitigated: u32, // 高危未到位（断言：零）
    pub summary: String,     // 结论摘要（非空）
    pub fingerprint: u64,    // 簿面指纹
}

/// 状态机：单向闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RiskStage {
    Open = 0,
    Registered = 1,
    Graded = 2,
    Mitigated = 3,
    Attested = 4,
}
impl RiskStage {
    pub fn can_step(self, next: RiskStage) -> bool {
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
pub struct RiskLedger {  // 收官风险登记簿。
    stage: RiskStage,
    book: Vec<RiskEntry>,
    seen: Vec<u32>,
    events: Vec<String>,
}
impl RiskLedger {
    pub fn new() -> Self {
        RiskLedger { stage: RiskStage::Open, book: Vec::new(), seen: Vec::new(), events: Vec::new() }
    }
    pub fn stage(&self) -> RiskStage {
        self.stage
    }
    pub fn len(&self) -> usize {
        self.book.len()
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn register(&mut self, batch: &[RiskEntry]) -> Result<u32, u16> {  // 登记一批风险（分批合法，簿满即拒，号唯一）。
        if !self.stage.can_step(RiskStage::Registered) {
            return Err(XC_STATE_INVALID);
        }
        if batch.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut added: u32 = 0;
        for e in batch.iter() {
            if self.book.len() >= RISK_CAP {
                return Err(XC_LEDGER_FULL);
            }
            if e.risk_hash == 0 {
                return Err(XC_RISK_ZERO);
            }
            if e.grade_of().is_err() {
                return Err(XC_GRADE_BAD);
            }
            let mut i = 0usize;
            while i < self.seen.len() {
                if self.seen[i] == e.rid {
                    return Err(XC_DUP_RID);
                }
                i += 1;
            }
            self.seen.push(e.rid);
            self.book.push(*e);
            added += 1;
        }
        if self.stage == RiskStage::Open {
            self.stage = RiskStage::Registered;
        }
        self.events.push(alloc::format!("register +{}", added));
        Ok(added)
    }
    pub fn entry_of(&self, rid: u32) -> Option<RiskEntry> {
        let mut i = 0usize;
        while i < self.book.len() {
            if self.book[i].rid == rid {
                return Some(self.book[i]);
            }
            i += 1;
        }
        None
    }
    pub fn grade_check(&mut self) -> Result<(u32, u32, u32), u16> {  // 分级核：逐条等级闭集 + ≥8 断言，产出三档计数。
        if !self.stage.can_step(RiskStage::Graded) {
            return Err(if self.book.len() < RISK_MIN {
                XC_COUNT_SHORT
            } else {
                XC_STATE_INVALID
            });
        }
        let (mut h, mut m, mut l) = (0u32, 0u32, 0u32);
        let mut i = 0usize;
        while i < self.book.len() {
            match self.book[i].grade_of()? {
                RiskGrade::High => h += 1,
                RiskGrade::Mid => m += 1,
                RiskGrade::Low => l += 1,
            }
            i += 1;
        }
        if h + m + l != self.book.len() as u32 {
            return Err(XC_COUNT_MISMATCH);
        }
        self.stage = RiskStage::Graded;
        self.events.push(alloc::format!("graded h={} m={} l={}", h, m, l));
        Ok((h, m, l))
    }
    pub fn mitigation_check(&mut self) -> Result<(u32, u32, u32, u32), u16> {  // 缓解到位核：逐条 mitigated_ok，产出到位/未到位与高危未到位计数。
        if !self.stage.can_step(RiskStage::Mitigated) {
            return Err(XC_NOT_REGISTERED);
        }
        let mut mit: u32 = 0;
        let mut unmit: u32 = 0;
        let mut high_total: u32 = 0;
        let mut high_unmit: u32 = 0;
        let mut i = 0usize;
        while i < self.book.len() {
            let e = &self.book[i];
            let g = e.grade_of()?;
            if g == RiskGrade::High {
                high_total += 1;
            }
            match e.mitigated_ok() {
                Ok(true) => mit += 1,
                Ok(false) => {
                    unmit += 1;
                    if g == RiskGrade::High {
                        high_unmit += 1;
                    }
                }
                Err(c) => return Err(c),
            }
            i += 1;
        }
        if mit + unmit != self.book.len() as u32 {
            return Err(XC_COUNT_MISMATCH);
        }
        self.stage = RiskStage::Mitigated;
        self.events
            .push(alloc::format!("mitigated {} unmit {} high_unmit {}", mit, unmit, high_unmit));
        Ok((mit, unmit, high_total, high_unmit))
    }
    pub fn attest(&mut self) -> Result<RiskAttest, u16> {  // 出证：≥8 + 缓解到位 + 高危零未缓解 → 摘要定格。
        if !self.stage.can_step(RiskStage::Attested) {
            return Err(if self.stage == RiskStage::Attested {
                XC_ALREADY_ATTESTED
            } else {
                XC_NOT_REGISTERED
            });
        }
        if self.book.len() < RISK_MIN {
            return Err(XC_COUNT_SHORT);
        }
        let mut mit = 0u32;
        let mut unmit = 0u32;
        let mut high_unmit = 0u32;
        let mut high_total = 0u32;
        let mut i = 0usize;
        while i < self.book.len() {
            let e = &self.book[i];
            let g = e.grade_of()?;
            if g == RiskGrade::High {
                high_total += 1;
            }
            match e.mitigated_ok() {
                Ok(true) => mit += 1,
                Ok(false) => {
                    unmit += 1;
                    if g == RiskGrade::High {
                        high_unmit += 1;
                    }
                }
                Err(c) => return Err(c),
            }
            i += 1;
        }
        if high_unmit != 0 {
            return Err(XC_HIGH_UNMITIGATED);
        }
        let fp = self.fingerprint();
        let summary = alloc::format!(
            "UNX-F0776-J1 登记簿齐：终态风险 {} 项（≥8）四列齐备，缓解到位 {}/{}，高危 {} 项零未缓解",
            self.book.len(), mit, self.book.len(), high_total
        );
        if summary.is_empty() {
            return Err(XC_NOTE_EMPTY);
        }
        self.stage = RiskStage::Attested;
        Ok(RiskAttest {
            registered: self.book.len() as u32,
            mitigated: mit,
            unmitigated: unmit,
            high_total,
            high_unmitigated: high_unmit,
            summary,
            fingerprint: fp,
        })
    }
    pub fn fingerprint(&self) -> u64 {  // 簿面指纹。
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut i = 0usize;
        while i < self.book.len() {
            let e = &self.book[i];
            h = fnv1a(h, e.rid as u64);
            h = fnv1a(h, e.risk_hash);
            h = fnv1a(h, e.grade as u64);
            h = fnv1a(h, e.channel as u64);
            h = fnv1a(h, e.strength as u64);
            h = fnv1a(h, e.residual as u64);
            i += 1;
        }
        h
    }
    pub fn high_unmitigated_list(&self) -> Result<Vec<u32>, u16> {  // 高危未到位清单（定位到风险号，出证前可先自查）。
        let mut out: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.book.len() {
            let e = &self.book[i];
            if e.grade_of()? == RiskGrade::High {
                match e.mitigated_ok() {
                    Ok(false) => out.push(e.rid),
                    Ok(true) => {}
                    Err(c) => return Err(c),
                }
            }
            i += 1;
        }
        Ok(out)
    }
    pub fn grade_view(&self, rid: u32) -> Result<RiskGrade, u16> {  // 档位分布查询（未分级也可查，纯读）。
        match self.entry_of(rid) {
            Some(e) => e.grade_of(),
            None => Err(XC_NOT_REGISTERED),
        }
    }

    /// 档位直方图：[高, 中, 低] 三桶计数。
    pub fn grade_histogram(&self) -> Result<[u32; 3], u16> {  // 守恒恒等式：三桶之和 == 簿面条数（禁虚记）。
        let mut hist = [0u32; 3];
        let mut i = 0usize;
        while i < self.book.len() {
            let g = self.book[i].grade_of()?;
            hist[g as usize] += 1;
            i += 1;
        }
        if hist[0] + hist[1] + hist[2] != self.book.len() as u32 {
            return Err(XC_COUNT_MISMATCH);
        }
        Ok(hist)
    }

    pub fn channel_distribution(&self) -> Result<[u32; 4], u16> {  // 缓解通道分布：四通道各自承载的风险数（整数闭集计数）。
        let mut dist = [0u32; 4];
        let mut i = 0usize;
        while i < self.book.len() {
            let ch = self.book[i].channel;
            if ch < 1 || ch > MIT_CHANNEL_COUNT {
                return Err(XC_CHANNEL_BAD);
            }
            dist[(ch - 1) as usize] += 1;
            i += 1;
        }
        Ok(dist)
    }

    pub fn mitigation_report(&self) -> Result<Vec<String>, u16> {  // 逐条缓解计划行（写回面用）：风险号 × 档位 × 通道 × 强度 × 残余。
        if self.book.is_empty() {
            return Err(XC_EMPTY_FEED);
        }
        let mut out: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.book.len() {
            let e = &self.book[i];
            let g = e.grade_of()?;
            out.push(alloc::format!(
                "R{} 档[{}] 通道{} 强度{} 残余{} 到位={}",
                e.rid, g.zh(), e.channel, e.strength, e.residual,
                e.mitigated_ok().unwrap_or(false)
            ));
            i += 1;
        }
        Ok(out)
    }

    pub fn high_view(&self) -> Result<Vec<u32>, u16> {  // 高危视图：全部 High 条目按 rid 升序（确定性选择排序，禁浮点）。
        let mut ids: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < self.book.len() {
            if self.book[i].grade_of()? == RiskGrade::High {
                ids.push(self.book[i].rid);
            }
            i += 1;
        }
        // 选择排序（n 小，确定性优先）。
        let mut a = 0usize;
        while a < ids.len() {
            let mut b = a + 1;
            while b < ids.len() {
                if ids[b] < ids[a] {
                    let t = ids[a];
                    ids[a] = ids[b];
                    ids[b] = t;
                }
                b += 1;
            }
            a += 1;
        }
        Ok(ids)
    }

    pub fn size_bounds_ok(&self) -> bool {  // 簿面规模自检：≥8 且 ≤16 双边界。
        self.book.len() >= RISK_MIN && self.book.len() <= RISK_CAP
    }
}
pub fn gen_risks_clean(n: usize) -> Vec<RiskEntry> {  // 样本发生器：确定性 n 条全到位风险（含高危样本）。
    let mut out: Vec<RiskEntry> = Vec::new();
    let mut rid = 1u32;
    let mut i = 0usize;
    while i < n {
        let grade = match i % 4 {
            0 => 0u8, // 高
            1 | 2 => 1u8,
            _ => 2u8,
        };
        let residual = match grade {
            0 => RESIDUAL_CAP_HIGH,
            1 => RESIDUAL_CAP_MID,
            _ => RESIDUAL_CAP_LOW,
        };
        out.push(RiskEntry {
            rid,
            risk_hash: 0x5FC0_0000_0000_0000 | rid as u64,
            grade,
            channel: (i % MIT_CHANNEL_COUNT as usize) as u8 + 1,
            strength: 2 + (i % 2) as u8, // 2..=3，到位
            residual,
        });
        rid += 1;
        i += 1;
    }
    out
}

pub fn drill() -> Result<u32, u16> {  // 残余超限/缺额/重号/通道越界/重入），0=全过。  // 自例程 drill：主路径（≥8 全到位出证）+ 错误路径（高危未缓解/强度弱/
    // —— 主路径：10 条分 2 批登记 ——
    let risks = gen_risks_clean(10);
    let mut m = RiskLedger::new();
    m.register(&risks[..5].to_vec())?;
    m.register(&risks[5..].to_vec())?;
    if m.len() != 10 {
        return Err(XC_COUNT_SHORT);
    }
    let (h, mid, l) = m.grade_check()?;
    if h + mid + l != 10 {
        return Err(XC_COUNT_MISMATCH);
    }
    let (mit, unmit, ht, hu) = m.mitigation_check()?;
    if mit != 10 || unmit != 0 || hu != 0 || ht == 0 {
        return Err(XC_HIGH_UNMITIGATED);
    }
    let att = m.attest()?;
    if att.registered != 10 || att.high_unmitigated != 0 || att.summary.is_empty() {
        return Err(XC_NOTE_EMPTY);
    }
    if m.attest() != Err(XC_ALREADY_ATTESTED) {
        return Err(XC_STATE_INVALID);
    }
    // —— 错误路径 1：高危未缓解 → 出证必红且清单可定位 ——
    let mut m2 = RiskLedger::new();
    let mut bad = gen_risks_clean(8);
    bad[0].strength = 1; // 高危条目（i%4==0 → rid=1）强度弱
    m2.register(&bad)?;
    m2.grade_check()?;
    m2.mitigation_check()?;
    let lst = m2.high_unmitigated_list()?;
    if lst.len() != 1 || lst[0] != 1 {
        return Err(XC_HIGH_UNMITIGATED);
    }
    if m2.attest() != Err(XC_HIGH_UNMITIGATED) {
        return Err(XC_HIGH_UNMITIGATED);
    }
    // —— 错误路径 2：中危残余超限 ——
    let mut m3 = RiskLedger::new();
    let mut bad2 = gen_risks_clean(8);
    bad2[1].residual = 3; // 中危上限 2
    m3.register(&bad2)?;
    m3.grade_check()?;
    if m3.mitigation_check() != Err(XC_RESIDUAL_HIGH) {
        return Err(XC_RESIDUAL_HIGH);
    }
    // —— 错误路径 3：不足 8 项 ——
    let mut m4 = RiskLedger::new();
    m4.register(&gen_risks_clean(7))?;
    if m4.grade_check() != Err(XC_COUNT_SHORT) {
        return Err(XC_COUNT_SHORT);
    }
    // —— 错误路径 4：重号 / 空批 / 等级越界 / 风险标识为零 ——
    let mut m5 = RiskLedger::new();
    let mut dup = gen_risks_clean(3);
    dup[2].rid = dup[0].rid;
    if m5.register(&dup) != Err(XC_DUP_RID) {
        return Err(XC_DUP_RID);
    }
    let mut m6 = RiskLedger::new();
    if m6.register(&[]) != Err(XC_EMPTY_FEED) {
        return Err(XC_EMPTY_FEED);
    }
    let mut m7 = RiskLedger::new();
    let mut badg = gen_risks_clean(3);
    badg[1].grade = 9;
    if m7.register(&badg) != Err(XC_GRADE_BAD) {
        return Err(XC_GRADE_BAD);
    }
    let mut m8 = RiskLedger::new();
    let mut badh = gen_risks_clean(3);
    badh[0].risk_hash = 0;
    if m8.register(&badh) != Err(XC_RISK_ZERO) {
        return Err(XC_RISK_ZERO);
    }
    // —— 错误路径 5：未登记齐即缓解核 / 查询 API ——
    let mut m9 = RiskLedger::new();
    m9.register(&gen_risks_clean(4))?;
    if m9.mitigation_check() != Err(XC_NOT_REGISTERED) {
        return Err(XC_NOT_REGISTERED);
    }
    if m.grade_view(4)? != RiskGrade::Mid {
        return Err(XC_GRADE_BAD);
    }
    if m.grade_view(999) != Err(XC_NOT_REGISTERED) {
        return Err(XC_NOT_REGISTERED);
    }
    match m.entry_of(5) {
        Some(e) if e.mitigated_ok() == Ok(true) => {}
        _ => return Err(XC_STRENGTH_WEAK),
    }
    // —— 扩展 API 面：直方图 / 通道分布 / 计划行 / 高危视图 / 规模自检 ——
    let hist = m.grade_histogram()?;
    if hist[0] + hist[1] + hist[2] != 10 {
        return Err(XC_COUNT_MISMATCH);
    }
    let dist = m.channel_distribution()?;
    if dist[0] + dist[1] + dist[2] + dist[3] != 10 {
        return Err(XC_COUNT_MISMATCH);
    }
    let rep = m.mitigation_report()?;
    if rep.len() != 10 {
        return Err(XC_EMPTY_FEED);
    }
    let highs = m.high_view()?;
    if highs.len() != 3 {
        return Err(XC_COUNT_MISMATCH);
    }
    let mut a = 1usize;
    while a < highs.len() {
        if highs[a] <= highs[a - 1] {
            return Err(XC_COUNT_MISMATCH);
        }
        a += 1;
    }
    if !m.size_bounds_ok() {
        return Err(XC_COUNT_SHORT);
    }
    // —— 高危清单与 mitigated 判据互证：bad[0] 高危强度弱必在清单 ——
    let lst2 = m2.high_unmitigated_list()?;
    if lst2 != vec![1] {
        return Err(XC_HIGH_UNMITIGATED);
    }
    Ok(0)
}
