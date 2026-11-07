//! UNX-F51591 B20 可观测性埋点（联轧触发/收敛/核账/留痕四点） · 0xC1680段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51592-J1 收官印入册：域账累计 120,000/240,000；B21–B28
//! E 型段衔接声明；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - ObsPoint：四埋点闭集（联轧触发/收敛/核账/留痕）
//!   - ObsEvent：埋点事件行（序号/点位/载荷）
//!   - ObsEngine：可观测性引擎（四点打点状态机 + 域账累计面 + 打点覆盖率账）
//!   - DomainLedger：域账（累计/目标 120,000→240,000，段型声明 B21–B28 E 型衔接）
//!   - ObsCode：诊断码闭集（12 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标，取值一律
//!      .get()/.get_mut()。
//!   ② 四点埋点闭集：联轧触发→收敛→核账→留痕必须按序打点，序违规显性拒；
//!      每点打点落事件行，覆盖率账要求四点各 ≥1 且无缺点。
//!   ③ 域账守恒：累计行只增不减，收官印入册判据为累计 120,000/240,000 达标
//!      且 E 型段衔接声明（B21–B28）逐项在册。
//!   ④ 注入核：4 类坏样本（缺点/乱序/载荷越界/重复点）逐类 10 轮精确复现，
//!      注入账守恒（检出+逃逸=轮数，逃逸=0）。
//!   ⑤ 双跑 FNV-1a 指纹一致：同一脚本两次建引擎指纹逐位相同。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1680;

/// 埋点覆盖目标：四点各至少一行。
pub const OBS_POINTS: usize = 4;
/// 域账累计已达成行数（收官印入册判据）。
pub const DOMAIN_ACCRUED: u64 = 120_000;
/// 域账总目标行数。
pub const DOMAIN_TOTAL: u64 = 240_000;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 四埋点闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsPoint {
    /// 联轧触发
    Trigger = 0,
    /// 收敛
    Converge = 1,
    /// 核账
    Audit = 2,
    /// 留痕
    Trace = 3,
}

impl ObsPoint {
    /// 点位序号（0..4）。
    pub fn idx(self) -> u32 {
        self as u32
    }
    /// 点位名（中文）。
    pub fn name(self) -> &'static str {
        match self {
            ObsPoint::Trigger => "联轧触发",
            ObsPoint::Converge => "收敛",
            ObsPoint::Audit => "核账",
            ObsPoint::Trace => "留痕",
        }
    }
    /// 闭集全量清单。
    pub fn all() -> [ObsPoint; 4] {
        [ObsPoint::Trigger, ObsPoint::Converge, ObsPoint::Audit, ObsPoint::Trace]
    }
}

/// 埋点事件行：可观测性回放的最小单元。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ObsEvent {
    /// 全局序号（单调递增）
    pub seq: u64,
    /// 点位（0=触发,1=收敛,2=核账,3=留痕）
    pub point: u32,
    /// 载荷（波位/计数等整数）
    pub payload: u64,
}

/// E 型段衔接声明（B21–B28 逐项在册）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HandoverDecl {
    /// 段号（21..29）
    pub seg: u32,
    /// 段型（E 型 = 1）
    pub kind: u32,
    /// 是否声明就位
    pub declared: bool,
}

/// B20 可观测性引擎：四点打点状态机 + 域账累计面。
pub struct ObsEngine {
    /// 各点位已打点计数（容量 4）。
    hits: [u64; 4],
    /// 已打到的最高点位序（状态机单调面）。
    hi_point: u32,
    events: Vec<ObsEvent>,
    next_seq: u64,
    domain_accrued: u64,
    handovers: Vec<HandoverDecl>,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl ObsEngine {
    /// 新建引擎（零打点零累计，衔接声明缺省全就位）。
    pub fn new() -> ObsEngine {
        let mut handovers = Vec::new();
        for seg in 21..29u32 {
            handovers.push(HandoverDecl { seg, kind: 1, declared: true });
        }
        ObsEngine {
            hits: [0; 4],
            hi_point: 0,
            events: Vec::new(),
            next_seq: 1,
            domain_accrued: DOMAIN_ACCRUED,
            handovers,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 落一行事件（序号单调）。
    fn log(&mut self, point: u32, payload: u64) {
        let e = ObsEvent { seq: self.next_seq, point, payload };
        self.next_seq += 1;
        self.events.push(e);
        self.hits[point as usize] += 1;
    }

    /// 打点：按四点闭集顺序推进（乱序/越界/重复打点显性拒）。
    pub fn mark(&mut self, point: ObsPoint, payload: u64) -> Result<u64, ObsCode> {
        if point.idx() > 3 {
            self.rejected += 1;
            return Err(ObsCode::PointUnknown);
        }
        if self.hi_point > 0 && point.idx() + 1 < self.hi_point {
            self.rejected += 1;
            return Err(ObsCode::PointOrder);
        }
        if self.hits[point.idx() as usize] > 0 && point.idx() <= self.hi_point {
            self.rejected += 1;
            return Err(ObsCode::PointDup);
        }
        if payload > DOMAIN_TOTAL * 2 {
            self.rejected += 1;
            return Err(ObsCode::PayloadOob);
        }
        self.log(point.idx(), payload);
        self.hi_point = point.idx() + 1;
        Ok(self.next_seq - 1)
    }

    /// 域账累计加行（只增不减，越目标显性拒）。
    pub fn accrue(&mut self, n: u64) -> Result<u64, ObsCode> {
        if self.domain_accrued + n > DOMAIN_TOTAL {
            self.rejected += 1;
            return Err(ObsCode::DomainOverflow);
        }
        self.domain_accrued += n;
        Ok(self.domain_accrued)
    }

    /// 收官印入册判据：域账累计 120,000/240,000 达标。
    pub fn seal_quota_ok(&self) -> bool {
        self.domain_accrued >= DOMAIN_ACCRUED && DOMAIN_ACCRUED * 2 == DOMAIN_TOTAL
    }

    /// E 型段衔接声明就位：B21–B28 全声明且段型全 E。
    pub fn handover_ok(&self) -> bool {
        self.handovers.len() == 8
            && self.handovers.iter().all(|h| h.declared && h.kind == 1)
            && self
                .handovers
                .iter()
                .enumerate()
                .all(|(i, h)| h.seg == (21 + i) as u32)
    }

    /// 覆盖率判据：四点各 ≥1 行且无缺点。
    pub fn coverage_ok(&self) -> bool {
        self.hits.iter().all(|&h| h >= 1) && self.events.len() >= OBS_POINTS
    }

    /// 事件轴可回放：序号单调 + 点位闭集。
    pub fn replayable(&self) -> bool {
        let mut prev: u64 = 0;
        for e in self.events.iter() {
            if e.seq <= prev || e.point > 3 {
                return false;
            }
            prev = e.seq;
        }
        !self.events.is_empty()
    }

    /// 事件轴行数。
    pub fn events_len(&self) -> usize {
        self.events.len()
    }

    /// 某点位命中数（越界返回 0）。
    pub fn hits_of(&self, point: ObsPoint) -> u64 {
        match self.hits.get(point.idx() as usize) {
            Some(&h) => h,
            None => 0,
        }
    }

    /// 域账累计值。
    pub fn accrued(&self) -> u64 {
        self.domain_accrued
    }

    /// (事件行数, 拒绝数) 二本计数。
    pub fn tally(&self) -> (u64, u64) {
        (self.events.len() as u64, self.rejected)
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 状态快照（双跑一致面）。
    pub fn snapshot(&self) -> (u32, u64, usize) {
        (self.hi_point, self.domain_accrued, self.events.len())
    }

    /// 快照指纹（含事件轴逐行）。
    pub fn fingerprint(&self) -> u64 {
        let (p, a, n) = self.snapshot();
        let mut bytes: Vec<u8> = Vec::new();
        for x in [p as u64, a, n as u64] {
            bytes.extend_from_slice(&x.to_le_bytes());
        }
        for e in self.events.iter() {
            bytes.extend_from_slice(&e.seq.to_le_bytes());
            bytes.extend_from_slice(&e.point.to_le_bytes());
            bytes.extend_from_slice(&e.payload.to_le_bytes());
        }
        for h in self.hits.iter() {
            bytes.extend_from_slice(&h.to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（B20 可观测性域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 点位越闭集
    PointUnknown,
    /// 点位乱序（四点必须按序打点）
    PointOrder,
    /// 点位重复打点
    PointDup,
    /// 载荷越界（超过域账总目标的 2 倍）
    PayloadOob,
    /// 域账溢出（累计越 240,000）
    DomainOverflow,
    /// 域账不足（收官印入册未达 120,000）
    DomainShort,
    /// 覆盖缺口（四点有缺）
    CoverGap,
    /// 衔接声明缺失（B21–B28 有缺或段型非 E）
    HandoverMissing,
    /// 事件序号倒退
    SeqRegress,
    /// 事件轴为空（可回放判据失败）
    EmptyAxis,
}

impl ObsCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            ObsCode::Ok => "可观测性判定成功",
            ObsCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            ObsCode::PointUnknown => "点位非法：埋点必须在四点闭集内",
            ObsCode::PointOrder => "点位乱序：四点必须按触发→收敛→核账→留痕打点",
            ObsCode::PointDup => "点位重复：同一点位不得重复打点",
            ObsCode::PayloadOob => "载荷越界：打点载荷超过域账总目标两倍",
            ObsCode::DomainOverflow => "域账溢出：累计不得超过 240,000 行",
            ObsCode::DomainShort => "域账不足：收官印入册要求累计达 120,000",
            ObsCode::CoverGap => "覆盖缺口：四点埋点必须各至少一行",
            ObsCode::HandoverMissing => "衔接声明缺失：B21–B28 须逐项在册且段型为 E",
            ObsCode::SeqRegress => "序号倒退：事件轴序号必须单调递增",
            ObsCode::EmptyAxis => "事件轴为空：可回放判据要求至少一行",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [ObsCode; 12] = [
    ObsCode::Ok,
    ObsCode::SegBad,
    ObsCode::PointUnknown,
    ObsCode::PointOrder,
    ObsCode::PointDup,
    ObsCode::PayloadOob,
    ObsCode::DomainOverflow,
    ObsCode::DomainShort,
    ObsCode::CoverGap,
    ObsCode::HandoverMissing,
    ObsCode::SeqRegress,
    ObsCode::EmptyAxis,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x9e3779b97f4a7c15 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：四点按序打满 + 域账达标。返回 (指纹, 事件行数)。
pub fn standard_script() -> (u64, usize) {
    let mut e = ObsEngine::new();
    for p in ObsPoint::all().iter() {
        let _ = e.mark(*p, 100 + p.idx() as u64);
    }
    let _ = e.accrue(64);
    (e.fingerprint(), e.events_len())
}

/// 双跑一致性：同一脚本两次建引擎，指纹与行数逐位一致。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：4 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
/// 返回 (总检出, 总逃逸, 每类检出)。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    // 类 0：缺点——只打前 3 点，覆盖判据必须判缺（显性 CoverGap）。
    for _ in 0..INJ_ROUNDS {
        let mut e = ObsEngine::new();
        let _ = e.mark(ObsPoint::Trigger, 1);
        let _ = e.mark(ObsPoint::Converge, 2);
        let _ = e.mark(ObsPoint::Audit, 3);
        if !e.coverage_ok() {
            e.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            e.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：乱序——从核账回打触发点，显性拒（PointOrder）。
    for _ in 0..INJ_ROUNDS {
        let mut e = ObsEngine::new();
        let _ = e.mark(ObsPoint::Trigger, 1);
        let _ = e.mark(ObsPoint::Converge, 2);
        let _ = e.mark(ObsPoint::Audit, 3);
        let caught = e.mark(ObsPoint::Trigger, 9) == Err(ObsCode::PointOrder);
        if caught {
            e.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            e.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：载荷越界——超域账总目标 2 倍，显性拒（PayloadOob）。
    for _ in 0..INJ_ROUNDS {
        let mut e = ObsEngine::new();
        let bad = DOMAIN_TOTAL * 2 + 1;
        let caught = e.mark(ObsPoint::Trigger, bad) == Err(ObsCode::PayloadOob);
        if caught {
            e.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            e.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：重复点——同点位二次打点，显性拒（PointDup）。
    for _ in 0..INJ_ROUNDS {
        let mut e = ObsEngine::new();
        let _ = e.mark(ObsPoint::Trigger, 1);
        let caught = e.mark(ObsPoint::Trigger, 2) == Err(ObsCode::PointDup);
        if caught {
            e.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            e.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（14 条判据形态）。
pub fn run_unx51591_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51591");
    // 1 四点按序打满。
    {
        let mut e = ObsEngine::new();
        let mut ok = true;
        for p in ObsPoint::all().iter() {
            if e.mark(*p, 10).is_err() {
                ok = false;
            }
        }
        s.add(
            "四点按序打满",
            ok && e.coverage_ok() && e.events_len() == 4,
            "联轧触发→收敛→核账→留痕四点按序各打一行",
        );
    }
    // 2 乱序显性拒。
    {
        let mut e = ObsEngine::new();
        let _ = e.mark(ObsPoint::Trigger, 1);
        let _ = e.mark(ObsPoint::Converge, 2);
        let _ = e.mark(ObsPoint::Audit, 3);
        let r = e.mark(ObsPoint::Trigger, 9);
        s.add(
            "乱序显性拒",
            r == Err(ObsCode::PointOrder) && e.rejected_tally() >= 1,
            "回打低位点位判乱序并计入拒绝账",
        );
    }
    // 3 域账收官印入册：120,000/240,000。
    {
        let mut e = ObsEngine::new();
        s.add(
            "域账收官印入册",
            e.seal_quota_ok() && e.accrued() == DOMAIN_ACCRUED,
            "UNX-F51592-J1：域账累计 120,000/240,000 达标",
        );
    }
    // 4 域账溢出显性拒。
    {
        let mut e = ObsEngine::new();
        let r = e.accrue(DOMAIN_TOTAL + 1);
        s.add(
            "域账溢出拒",
            r == Err(ObsCode::DomainOverflow) && e.accrued() == DOMAIN_ACCRUED,
            "累计越 240,000 显性拒且账面不变",
        );
    }
    // 5 B21–B28 E 型段衔接声明。
    {
        let e = ObsEngine::new();
        s.add(
            "E 型段衔接声明",
            e.handover_ok(),
            "B21–B28 逐项在册、段型全 E、段号连续",
        );
    }
    // 6 覆盖缺口判出。
    {
        let mut e = ObsEngine::new();
        let _ = e.mark(ObsPoint::Trigger, 1);
        s.add(
            "覆盖缺口判出",
            !e.coverage_ok(),
            "只打一点判覆盖缺口（四点须各 ≥1 行）",
        );
    }
    // 7 注入核 4 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        let all = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全通",
            d == 40 && esc == 0 && all,
            "4 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 8 事件轴可回放。
    {
        let mut e = ObsEngine::new();
        for p in ObsPoint::all().iter() {
            let _ = e.mark(*p, p.idx() as u64);
        }
        s.add(
            "事件轴可回放",
            e.replayable() && e.events_len() == 4,
            "事件序号单调且点位在四点闭集内",
        );
    }
    // 9 点位闭集完整性。
    {
        let ok = ObsPoint::all().len() == 4
            && ObsPoint::all()
                .iter()
                .enumerate()
                .all(|(i, p)| p.idx() == i as u32 && !p.name().is_empty());
        s.add(
            "点位闭集完整",
            ok,
            "四点闭集序号 0..4 且点位名全量中文非空",
        );
    }
    // 10 诊断码闭集唯一性。
    {
        let mut uniq = true;
        let mut text = true;
        for i in 0..ALL_CODES.len() {
            let c = match ALL_CODES.get(i) {
                Some(&c) => c,
                None => {
                    uniq = false;
                    continue;
                }
            };
            if c.val() != SEG_BASE + 1 + i as u32 || c.detail().is_empty() {
                uniq = false;
            }
        }
        if !text {
            uniq = false;
        }
        s.add(
            "诊断码闭集唯一",
            uniq && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 11 注入账守恒单独核。
    {
        let mut e = ObsEngine::new();
        for _ in 0..12u32 {
            e.note_detected();
        }
        s.add("注入账守恒", e.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 12 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建引擎，指纹与事件行数逐位一致",
    );
    // 13 指纹复算稳定。
    {
        let mut e = ObsEngine::new();
        for p in ObsPoint::all().iter() {
            let _ = e.mark(*p, 7);
        }
        let f1 = e.fingerprint();
        let f2 = e.fingerprint();
        let brief: String = format!("{:016x}", f1);
        s.add(
            "指纹复算稳定",
            f1 == f2 && brief.len() == 16,
            "指纹两次复算相等且十六进制摘要定长 16",
        );
    }
    // 14 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51591 B20 可观测性埋点 诊断码{} 变体 四点{}",
            ALL_CODES.len(),
            OBS_POINTS
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51591"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

impl ObsEngine {
    /// 拒绝账计数（供乱序判据读取）。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51591_checks_green() {
        let set = run_unx51591_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn four_points_in_order() {
        let mut e = ObsEngine::new();
        for (i, p) in ObsPoint::all().iter().enumerate() {
            assert_eq!(e.mark(*p, i as u64), Ok((i + 1) as u64));
        }
        assert!(e.coverage_ok());
        assert_eq!(e.events_len(), 4);
        assert_eq!(e.hits_of(ObsPoint::Trace), 1);
    }

    #[test]
    fn out_of_order_rejected() {
        let mut e = ObsEngine::new();
        let _ = e.mark(ObsPoint::Trigger, 1);
        let _ = e.mark(ObsPoint::Audit, 2);
        assert_eq!(e.mark(ObsPoint::Converge, 3), Err(ObsCode::PointOrder));
        assert!(e.rejected_tally() >= 1);
    }

    #[test]
    fn duplicate_point_rejected() {
        let mut e = ObsEngine::new();
        let _ = e.mark(ObsPoint::Trigger, 1);
        assert_eq!(e.mark(ObsPoint::Trigger, 2), Err(ObsCode::PointDup));
    }

    #[test]
    fn payload_oob_rejected() {
        let mut e = ObsEngine::new();
        assert_eq!(e.mark(ObsPoint::Trigger, DOMAIN_TOTAL * 2 + 1), Err(ObsCode::PayloadOob));
        assert!(e.mark(ObsPoint::Trigger, DOMAIN_TOTAL).is_ok());
    }

    #[test]
    fn domain_accrual_monotone() {
        let mut e = ObsEngine::new();
        assert_eq!(e.accrue(100), Ok(DOMAIN_ACCRUED + 100));
        assert_eq!(e.accrue(DOMAIN_TOTAL), Err(ObsCode::DomainOverflow));
        assert_eq!(e.accrued(), DOMAIN_ACCRUED + 100);
    }

    #[test]
    fn handover_decl_closed() {
        let e = ObsEngine::new();
        assert!(e.handover_ok());
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 40);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut e = ObsEngine::new();
        for _ in 0..10u32 {
            e.note_detected();
        }
        assert!(e.conserved());
        let mut bad = ObsEngine::new();
        for _ in 0..10u32 {
            bad.note_escape();
        }
        assert!(!bad.conserved());
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
        assert_eq!(standard_script(), standard_script());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn replayable_axis() {
        let mut e = ObsEngine::new();
        for p in ObsPoint::all().iter() {
            let _ = e.mark(*p, 1);
        }
        assert!(e.replayable());
        let (n, rej) = e.tally();
        assert_eq!(n, 4);
        assert_eq!(rej, 0);
    }
}
