//! UNX-F8741 · 冷启动计时协议终版账 · 诊断段 0xB740
//! 验收锚点：UNX-F8741-J1 冷启动计时协议终版——阶段链（boot→mm→
//! sched→usr）时间戳单调不减、阶段间隙上限约束、总时长预算约束、
//! 协议版本冻结字段，违序/超窗/超预算三路全拒，双判据缺一即批内红灯。

use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};

pub const SEG_BASE: u16 = 0xB740;

// ── 诊断段独占 · 计时协议错误域 ─────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_ORDER: u16 = SEG_ERR + 1;   // 时间戳违序
const E_WINDOW: u16 = SEG_ERR + 2;  // 阶段间隙超窗
const E_BUDGET: u16 = SEG_ERR + 3;  // 总时长超预算
const E_STAGE: u16 = SEG_ERR + 4;   // 阶段缺失或重复
const E_VER: u16 = SEG_ERR + 5;     // 协议版本不符

/// 协议版本（终版冻结）。
pub const PROTO_VER: u32 = 0x4354_4601; // "CTF1"
pub const STAGE_WINDOW_US: u64 = 500_000;   // 单阶段间隙上限 500ms
pub const BUDGET_US: u64 = 2_000_000;       // 冷启动总预算 2s

// ── 阶段链 ───────────────────────────────────────────────────
/// 四阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColdStage {
    Boot,   // 引导
    Mm,     // 内存管理就绪
    Sched,  // 调度就绪
    Usr,    // 用户态可入
}

impl ColdStage {
    pub fn all() -> [ColdStage; 4] {
        [ColdStage::Boot, ColdStage::Mm, ColdStage::Sched, ColdStage::Usr]
    }
    pub fn as_str(self) -> &'static str {
        match self {
            ColdStage::Boot => "boot",
            ColdStage::Mm => "mm",
            ColdStage::Sched => "sched",
            ColdStage::Usr => "usr",
        }
    }
    pub fn idx(self) -> usize {
        match self {
            ColdStage::Boot => 0,
            ColdStage::Mm => 1,
            ColdStage::Sched => 2,
            ColdStage::Usr => 3,
        }
    }
}

/// 计时记录：阶段 → 时间戳（微秒）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeMark {
    pub stage: ColdStage,
    pub t_us: u64,
}

/// 冷启动时间线：记录集 + 协议版本。
pub struct ColdTimeline {
    pub marks: Vec<TimeMark>,
    pub proto_ver: u32,
}

impl ColdTimeline {
    pub fn new() -> Self {
        ColdTimeline { marks: Vec::new(), proto_ver: PROTO_VER }
    }
    pub fn mark(&mut self, st: ColdStage, t: u64) {
        self.marks.push(TimeMark { stage: st, t_us: t });
    }
    /// 四阶段齐且不重。
    pub fn stages_complete(&self) -> bool {
        for st in ColdStage::all() {
            if self.marks.iter().filter(|m| m.stage == st).count() != 1 {
                return false;
            }
        }
        true
    }
    /// 协议校验：版本 + 齐全 + 单调 + 间隙窗 + 总预算。
    pub fn validate(&self) -> Result<(u64, u64), u16> {
        if self.proto_ver != PROTO_VER {
            return Err(E_VER);
        }
        if !self.stages_complete() {
            return Err(E_STAGE);
        }
        // 按阶段序取时间戳
        let mut prev: u64 = 0;
        let mut total_span: u64 = 0;
        for (i, st) in ColdStage::all().iter().enumerate() {
            let m = self.marks.iter().find(|m| m.stage == *st).unwrap();
            if m.t_us < prev {
                return Err(E_ORDER);
            }
            if i > 0 && m.t_us - prev > STAGE_WINDOW_US {
                return Err(E_WINDOW);
            }
            prev = m.t_us;
            if i == 3 {
                total_span = m.t_us - self.marks.iter().find(|m| m.stage == ColdStage::Boot).unwrap().t_us;
            }
        }
        if total_span > BUDGET_US {
            return Err(E_BUDGET);
        }
        Ok((total_span, STAGE_WINDOW_US))
    }
    /// 总时长（usr − boot）。
    pub fn span_us(&self) -> Option<u64> {
        let b = self.marks.iter().find(|m| m.stage == ColdStage::Boot)?.t_us;
        let u = self.marks.iter().find(|m| m.stage == ColdStage::Usr)?.t_us;
        Some(u.saturating_sub(b))
    }
}

/// 时间线指纹（阶段×时间戳折叠）。
pub fn timeline_fp(tl: &ColdTimeline) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for m in &tl.marks {
        h ^= m.stage.idx() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        for b in m.t_us.to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
    for b in tl.proto_ver.to_le_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

// ── 标准夹具：合规时间线（四阶段预算内）────────────────────
pub fn standard_timeline() -> ColdTimeline {
    let mut tl = ColdTimeline::new();
    tl.mark(ColdStage::Boot, 0);
    tl.mark(ColdStage::Mm, 120_000);
    tl.mark(ColdStage::Sched, 300_000);
    tl.mark(ColdStage::Usr, 640_000);
    tl
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx8741_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8741 冷启动计时协议终版");
    let tl = standard_timeline();

    // 1. 合规时间线全过
    let v = tl.validate();
    s.add("COLD-合规全过", v.is_ok(), if v.is_ok() { "" } else { "合规线被拒" });

    // 2. 四阶段齐且不重
    s.add("COLD-阶段齐全", tl.stages_complete(), if tl.stages_complete() { "" } else { "阶段缺失" });

    // 3. 总时长在预算内
    let span = tl.span_us().unwrap_or(u64::MAX);
    s.add("COLD-预算内", span <= BUDGET_US, if span <= BUDGET_US { "" } else { "超预算" });

    // 4. 违序检出：sched 早于 mm
    let mut t2 = standard_timeline();
    t2.marks[2].t_us = 100_000; // sched < mm
    s.add("COLD-违序拒", t2.validate() == Err(E_ORDER),
        if t2.validate() == Err(E_ORDER) { "" } else { "违序放行" });

    // 5. 间隙超窗拒：boot→mm 600ms
    let mut t3 = standard_timeline();
    t3.marks[1].t_us = 600_000;
    s.add("COLD-超窗拒", t3.validate() == Err(E_WINDOW),
        if t3.validate() == Err(E_WINDOW) { "" } else { "超窗放行" });

    // 6. 总预算超拒：各阶段合法但总时长 2.1s
    let mut t4 = standard_timeline();
    t4.marks[1].t_us = 400_000;
    t4.marks[2].t_us = 800_000;
    t4.marks[3].t_us = 2_100_000; // 间隙各 ≤500ms，总超 2s
    s.add("COLD-超预算拒", t4.validate() == Err(E_BUDGET),
        if t4.validate() == Err(E_BUDGET) { "" } else { "超预算放行" });

    // 7. 阶段缺失拒
    let mut t5 = standard_timeline();
    t5.marks.remove(1); // 缺 mm
    s.add("COLD-缺阶段拒", t5.validate() == Err(E_STAGE),
        if t5.validate() == Err(E_STAGE) { "" } else { "缺阶段放行" });

    // 8. 阶段重复拒
    let mut t6 = standard_timeline();
    t6.marks.push(TimeMark { stage: ColdStage::Mm, t_us: 150_000 });
    s.add("COLD-重复阶段拒", t6.validate() == Err(E_STAGE),
        if t6.validate() == Err(E_STAGE) { "" } else { "重复阶段放行" });

    // 9. 协议版本不符拒
    let mut t7 = standard_timeline();
    t7.proto_ver = 0x4354_4600;
    s.add("COLD-版本拒", t7.validate() == Err(E_VER),
        if t7.validate() == Err(E_VER) { "" } else { "版本漂移放行" });

    // 10. 时间线指纹稳定且敏感
    let f1 = timeline_fp(&tl);
    let f2 = timeline_fp(&tl);
    let mut t8 = standard_timeline();
    t8.marks[3].t_us += 1;
    let f3 = timeline_fp(&t8);
    s.add("COLD-指纹敏感", f1 == f2 && f1 != f3,
        if f1 == f2 && f1 != f3 { "" } else { "指纹碰撞或漂移" });

    // 11. 恰贴窗界通过（间隙恰 500ms）
    let mut t9 = ColdTimeline::new();
    t9.mark(ColdStage::Boot, 0);
    t9.mark(ColdStage::Mm, 500_000);
    t9.mark(ColdStage::Sched, 1_000_000);
    t9.mark(ColdStage::Usr, 1_500_000);
    s.add("COLD-贴窗界过", t9.validate().is_ok(), if t9.validate().is_ok() { "" } else { "贴界误拒" });

    // 12. 恰贴预算界通过（总恰 2s）
    let mut t10 = ColdTimeline::new();
    t10.mark(ColdStage::Boot, 0);
    t10.mark(ColdStage::Mm, 500_000);
    t10.mark(ColdStage::Sched, 1_000_000);
    t10.mark(ColdStage::Usr, 2_000_000);
    s.add("COLD-贴预算界过", t10.validate().is_ok(),
        if t10.validate().is_ok() { "" } else { "贴预算界误拒" });

    // 13. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB740 && SEG_BASE < 0xB760;
    s.add("COLD-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
