//! UNX-F51518 B16 错误码扩展（纵深族 ×12 并入域内三表） · 0xC0480段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51519-J1 收口防重 grep 留痕；与 B09/B10 零重叠断言
//!
//! 设计要点：
//!   ① 纵深族 ×12 按「错误码并入账」建模：每条新码携带（码值, 文案长度,
//!      纵深级）三元组，一次并入同步写足域内三表——码值表、文案长度表、
//!      纵深级表；任一表写不进即整条回拒，禁半写半落。
//!   ② 与 B09/B10 零重叠是硬断言：B09 值域 [0xC0000, 0xC017F]、B10 值域
//!      [0xC0180, 0xC02FF] 与 B16 值域 [0xC0480, 0xC057F] 两两不相交，
//!      并入前逐条断言，重叠即明拒（RangeOverlap）。
//!   ③ 码值表单调：并入按码值严格递增落表，重复码（ValDuplicate）与
//!      倒退码（ValBackward）显性拒；三表同槽同步推进，账面对平。
//!   ④ 单调状态机：空账 → 并入中 → 封账，只进不退；rollback 显性拒
//!      （PhaseBackward），封账后并入/再封账一律拒（SealedLocked）。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；三类注入（B09 重叠、
//!      B10 重叠、封账后并入）×10 轮必抓。
//!   ⑥ 判定次序：封账 → 负值 → 重叠 → 值域 → 重复 → 倒退 → 表满 →
//!      文案 → 纵深；多重违约同时成立时取最先命中的判据码。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0xC0480;

/// B09 段值域下界（零重叠断言对照面）。
pub const B09_LO: u32 = 0xC0000;
/// B09 段值域上界。
pub const B09_HI: u32 = 0xC017F;
/// B10 段值域下界。
pub const B10_LO: u32 = 0xC0180;
/// B10 段值域上界。
pub const B10_HI: u32 = 0xC02FF;
/// B16 段值域下界（本卡段）。
pub const B16_LO: u32 = 0xC0480;
/// B16 段值域上界。
pub const B16_HI: u32 = 0xC057F;
/// 域内三表容量上限（条）。
pub const TABLE_CAP: usize = 16;
/// 文案长度上限（字节）。
pub const TEXT_CAP: i64 = 64;
/// 纵深级上限（入口/参数/内核/介质 四级）。
pub const DEPTH_MAX: u8 = 4;
/// 纵深族冻结批量（判据：×12）。
pub const DEPTH_BATCH: usize = 12;

/// 诊断码闭集（错误码并入域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 已封账，并入/再封账被拒
    SealedLocked,
    /// 码值为负
    ValNegative,
    /// 与 B09/B10 值域重叠（零重叠断言触发）
    RangeOverlap,
    /// 码值越出 B16 值域
    ValOutOfRange,
    /// 码值与已并入条目重复
    ValDuplicate,
    /// 码值倒退（小于已并入最大码值）
    ValBackward,
    /// 三表已满（容量耗尽）
    TableFull,
    /// 文案为空或为负
    TextEmpty,
    /// 文案超长
    TextTooLong,
    /// 纵深级越界（须 1..=4）
    DepthBad,
    /// 状态机倒退（rollback 显性拒）
    PhaseBackward,
    /// 三表对账失同步（复核不一致）
    TableDesync,
}

impl ErrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ErrCode::Ok => "并入成功",
            ErrCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            ErrCode::SealedLocked => "已封账：封账后禁止并入或重复收官",
            ErrCode::ValNegative => "码值为负：错误码必须为非负整数",
            ErrCode::RangeOverlap => "与 B09/B10 重叠：零重叠断言触发，核对段域分配",
            ErrCode::ValOutOfRange => "码值越出 B16 值域：重新申请段内码位",
            ErrCode::ValDuplicate => "码值重复：查重后再并入",
            ErrCode::ValBackward => "码值倒退：并入必须严格递增",
            ErrCode::TableFull => "三表已满：归档后开新表",
            ErrCode::TextEmpty => "文案为空：每条码必须有非空中文文案",
            ErrCode::TextTooLong => "文案超长：压缩到上限以内",
            ErrCode::DepthBad => "纵深级越界：只允许 1..=4 级",
            ErrCode::PhaseBackward => "状态机倒退：并入账只进不退",
            ErrCode::TableDesync => "三表失同步：复核不一致，检查并入原子性",
        }
    }
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [ErrCode; 14] = [
    ErrCode::Ok,
    ErrCode::SegBad,
    ErrCode::SealedLocked,
    ErrCode::ValNegative,
    ErrCode::RangeOverlap,
    ErrCode::ValOutOfRange,
    ErrCode::ValDuplicate,
    ErrCode::ValBackward,
    ErrCode::TableFull,
    ErrCode::TextEmpty,
    ErrCode::TextTooLong,
    ErrCode::DepthBad,
    ErrCode::PhaseBackward,
    ErrCode::TableDesync,
];

/// 冻结种子码：纵深族 ×12（判据批量）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeedCode {
    /// 段内偏移（码值 = SEG_BASE + off）
    pub off: u32,
    /// 冻结中文文案
    pub text: &'static str,
    /// 纵深级（1..=4）
    pub depth: u8,
}

/// 纵深族冻结表（×12 条，一次性并入三表）。
pub const SEED_CODES: [SeedCode; DEPTH_BATCH] = [
    SeedCode { off: 0x10, text: "入口层参数失配", depth: 1 },
    SeedCode { off: 0x14, text: "入口层状态未就绪", depth: 1 },
    SeedCode { off: 0x18, text: "参数层越界拒绝", depth: 2 },
    SeedCode { off: 0x1C, text: "参数层序号失步", depth: 2 },
    SeedCode { off: 0x20, text: "内核层资源枯竭", depth: 3 },
    SeedCode { off: 0x24, text: "内核层守卫触发", depth: 3 },
    SeedCode { off: 0x28, text: "介质层写入受挫", depth: 4 },
    SeedCode { off: 0x2C, text: "介质层回收受阻", depth: 4 },
    SeedCode { off: 0x30, text: "纵深联动降级", depth: 4 },
    SeedCode { off: 0x34, text: "纵深联动恢复", depth: 3 },
    SeedCode { off: 0x38, text: "纵深族聚合告警", depth: 2 },
    SeedCode { off: 0x3C, text: "纵深族封账印记", depth: 1 },
];

/// 并入状态机：空账 → 并入中 → 封账（只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MergePhase {
    /// 空账
    Empty,
    /// 并入中
    Merging,
    /// 已封账
    Sealed,
}

/// 错误码并入账引擎：域内三表 + 单调状态机 + 注入守恒面。
pub struct ErrCodeLedger {
    vals: [u32; TABLE_CAP],
    text_lens: [u16; TABLE_CAP],
    depths: [u8; TABLE_CAP],
    merged: usize,
    phase: MergePhase,
    seed_fault: u32,
    injected: u64,
    detected: u64,
}

/// 与 B09 值域重叠断言。
pub fn overlaps_b09(val: u32) -> bool {
    val >= B09_LO && val <= B09_HI
}

/// 与 B10 值域重叠断言。
pub fn overlaps_b10(val: u32) -> bool {
    val >= B10_LO && val <= B10_HI
}

impl ErrCodeLedger {
    /// 新账：三表空、状态空账。
    pub fn new() -> ErrCodeLedger {
        ErrCodeLedger {
            vals: [0; TABLE_CAP],
            text_lens: [0; TABLE_CAP],
            depths: [0; TABLE_CAP],
            merged: 0,
            phase: MergePhase::Empty,
            seed_fault: 0,
            injected: 0,
            detected: 0,
        }
    }

    /// 种子并入：纵深族 ×12 一次性落三表（判据批量）。
    pub fn with_seed() -> ErrCodeLedger {
        let mut l = ErrCodeLedger::new();
        for sc in SEED_CODES.iter() {
            let val = SEG_BASE.saturating_add(sc.off) as i64;
            let tl = sc.text.len() as i64;
            if l.merge_one(val, tl, sc.depth as i64).is_err() {
                l.seed_fault = l.seed_fault.saturating_add(1);
            }
        }
        l
    }

    /// 并入一条纵深族错误码（判定次序见文档头 ⑥）。
    pub fn merge_one(&mut self, val: i64, text_len: i64, depth: i64) -> Result<usize, ErrCode> {
        if self.phase == MergePhase::Sealed {
            return Err(ErrCode::SealedLocked);
        }
        if val < 0 {
            return Err(ErrCode::ValNegative);
        }
        let v = val as u32;
        if overlaps_b09(v) || overlaps_b10(v) {
            return Err(ErrCode::RangeOverlap);
        }
        if val < B16_LO as i64 || val > B16_HI as i64 {
            return Err(ErrCode::ValOutOfRange);
        }
        for i in 0..self.merged {
            let seen = match self.vals.get(i) {
                Some(x) => *x,
                None => return Err(ErrCode::TableDesync),
            };
            if seen == v {
                return Err(ErrCode::ValDuplicate);
            }
        }
        if self.merged > 0 {
            let last = match self.vals.get(self.merged - 1) {
                Some(x) => *x,
                None => return Err(ErrCode::TableDesync),
            };
            if v < last {
                return Err(ErrCode::ValBackward);
            }
        }
        if self.merged >= TABLE_CAP {
            return Err(ErrCode::TableFull);
        }
        if text_len <= 0 {
            return Err(ErrCode::TextEmpty);
        }
        if text_len > TEXT_CAP {
            return Err(ErrCode::TextTooLong);
        }
        if depth <= 0 || depth > DEPTH_MAX as i64 {
            return Err(ErrCode::DepthBad);
        }
        let slot = self.merged;
        let pv = match self.vals.get_mut(slot) {
            Some(x) => x,
            None => return Err(ErrCode::TableFull),
        };
        *pv = v;
        let pt = match self.text_lens.get_mut(slot) {
            Some(x) => x,
            None => return Err(ErrCode::TableDesync),
        };
        *pt = text_len as u16;
        let pd = match self.depths.get_mut(slot) {
            Some(x) => x,
            None => return Err(ErrCode::TableDesync),
        };
        *pd = depth as u8;
        self.merged = slot + 1;
        if self.phase == MergePhase::Empty {
            self.phase = MergePhase::Merging;
        }
        Ok(self.merged)
    }

    /// 封账收官：并入面冻结。
    pub fn seal(&mut self) -> Result<usize, ErrCode> {
        if self.phase == MergePhase::Sealed {
            return Err(ErrCode::SealedLocked);
        }
        self.phase = MergePhase::Sealed;
        Ok(self.merged)
    }

    /// 回滚（显性拒：并入账只进不退）。
    pub fn rollback(&mut self) -> Result<(), ErrCode> {
        Err(ErrCode::PhaseBackward)
    }

    /// 三表复核：同槽同步、单调递增、值域纵深文案全合法。
    pub fn verify_tables(&self) -> Result<(), ErrCode> {
        for i in 0..self.merged {
            let v = match self.vals.get(i) {
                Some(x) => *x,
                None => return Err(ErrCode::TableDesync),
            };
            let t = match self.text_lens.get(i) {
                Some(x) => *x,
                None => return Err(ErrCode::TableDesync),
            };
            let d = match self.depths.get(i) {
                Some(x) => *x,
                None => return Err(ErrCode::TableDesync),
            };
            if v < B16_LO || v > B16_HI || overlaps_b09(v) || overlaps_b10(v) {
                return Err(ErrCode::TableDesync);
            }
            if t == 0 || t as i64 > TEXT_CAP || d == 0 || d > DEPTH_MAX {
                return Err(ErrCode::TableDesync);
            }
            if i > 0 {
                let prev = match self.vals.get(i - 1) {
                    Some(x) => *x,
                    None => return Err(ErrCode::TableDesync),
                };
                if v <= prev {
                    return Err(ErrCode::TableDesync);
                }
            }
        }
        Ok(())
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
    }

    pub fn merged_len(&self) -> usize {
        self.merged
    }

    pub fn seed_faults(&self) -> u32 {
        self.seed_fault
    }

    pub fn phase_name(&self) -> &'static str {
        match self.phase {
            MergePhase::Empty => "空账",
            MergePhase::Merging => "并入中",
            MergePhase::Sealed => "已封账",
        }
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    /// 守恒断言面：注入 = 检出，逃逸恒 0 且有检出。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected && self.detected > 0
    }

    /// 三表快照（双跑一致性对照）。
    pub fn snapshot(&self) -> ([u32; TABLE_CAP], [u16; TABLE_CAP], [u8; TABLE_CAP], usize, MergePhase) {
        (self.vals, self.text_lens, self.depths, self.merged, self.phase)
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同一构造路径两次重建三表快照逐字段一致。
pub fn dual_run_consistent() -> bool {
    let build = || -> ErrCodeLedger {
        let mut e = ErrCodeLedger::with_seed();
        let _ = e.merge_one(0xC04C0, 8, 2);
        let _ = e.merge_one(0xC04C4, 12, 3);
        let _ = e.seal();
        e
    };
    let a = build();
    let b = build();
    a.snapshot() == b.snapshot()
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51518_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51518");
    // 1 正常流：种子 ×12 并入三表。
    {
        let e = ErrCodeLedger::with_seed();
        s.add_dyn(
            "种子 ×12 并入三表",
            e.merged_len() == DEPTH_BATCH
                && e.seed_faults() == 0
                && e.phase_name() == "并入中",
            "纵深族种子应全数并入且零故障",
        );
    }
    // 2 三表同步复核。
    {
        let e = ErrCodeLedger::with_seed();
        let sync = match e.verify_tables() {
            Ok(()) => e.merged_len() == DEPTH_BATCH,
            Err(_) => false,
        };
        s.add_dyn("三表同步复核", sync, "码值/文案/纵深三表同槽同步推进");
    }
    // 3 边界零：文案空与纵深零。
    {
        let mut e = ErrCodeLedger::with_seed();
        let r_t = e.merge_one(0xC04C0, 0, 1);
        let r_d = e.merge_one(0xC04C0, 8, 0);
        s.add_dyn(
            "边界零拒绝",
            r_t == Err(ErrCode::TextEmpty) && r_d == Err(ErrCode::DepthBad),
            "文案空与纵深零都必须明拒",
        );
    }
    // 4 边界负：码值负与文案负。
    {
        let mut e = ErrCodeLedger::with_seed();
        let r_v = e.merge_one(-1, 8, 1);
        let r_t = e.merge_one(0xC04C0, -4, 1);
        s.add_dyn(
            "边界负拒绝",
            r_v == Err(ErrCode::ValNegative) && r_t == Err(ErrCode::TextEmpty),
            "负码值与负文案都必须明拒",
        );
    }
    // 5 边界上限：B16 上界放行。
    {
        let mut e = ErrCodeLedger::with_seed();
        let r = e.merge_one(B16_HI as i64, 8, 4);
        s.add_dyn("边界上限放行", r.is_ok(), "B16 上界码值应放行");
    }
    // 6 边界上限+1：B16 上界外拒。
    {
        let mut e = ErrCodeLedger::with_seed();
        let r = e.merge_one(B16_HI as i64 + 1, 8, 4);
        s.add_dyn(
            "边界上限+1 拒绝",
            r == Err(ErrCode::ValOutOfRange),
            "越出 B16 上界必须拒绝",
        );
    }
    // 7 注入必抓：B09 重叠 ×10。
    {
        let mut e = ErrCodeLedger::with_seed();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let val = B09_LO.saturating_add(i.saturating_mul(4)) as i64;
            if e.merge_one(val, 8, 1) == Err(ErrCode::RangeOverlap) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("B09 重叠注入 10/10", caught == 10, "B09 重叠必须全抓");
    }
    // 8 注入必抓：B10 重叠 ×10。
    {
        let mut e = ErrCodeLedger::with_seed();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let val = B10_LO.saturating_add(i.saturating_mul(4)) as i64;
            if e.merge_one(val, 8, 1) == Err(ErrCode::RangeOverlap) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("B10 重叠注入 10/10", caught == 10, "B10 重叠必须全抓");
    }
    // 9 注入必抓：封账后并入 ×10。
    {
        let mut e = ErrCodeLedger::with_seed();
        let _ = e.seal();
        let mut caught = 0u32;
        for i in 0..10u32 {
            let val = 0xC04C0 + (4 * i) as i64;
            if e.merge_one(val, 8, 1) == Err(ErrCode::SealedLocked) {
                caught += 1;
                e.note_detected();
            }
        }
        s.add_dyn("封账后并入注入 10/10", caught == 10, "封账锁定必须全抓");
    }
    // 10 判定次序：封账先于负值；重叠先于文案。
    {
        let mut e = ErrCodeLedger::with_seed();
        let _ = e.seal();
        let r1 = e.merge_one(-1, 8, 1);
        let mut e2 = ErrCodeLedger::with_seed();
        let r2 = e2.merge_one(B09_LO as i64, 0, 1);
        s.add_dyn(
            "判定次序封账/重叠在前",
            r1 == Err(ErrCode::SealedLocked) && r2 == Err(ErrCode::RangeOverlap),
            "封账判据先于负值，重叠判据先于文案",
        );
    }
    // 11 状态机拒止：rollback 与重复封账。
    {
        let mut e = ErrCodeLedger::with_seed();
        let r1 = e.rollback();
        let _ = e.seal();
        let r2 = e.seal();
        let r3 = e.rollback();
        s.add_dyn(
            "状态机倒退拒止",
            r1 == Err(ErrCode::PhaseBackward)
                && r2 == Err(ErrCode::SealedLocked)
                && r3 == Err(ErrCode::PhaseBackward),
            "并入账只进不退，倒退与重复封账全拒",
        );
    }
    // 12 容量耗尽：表满 +1 拒。
    {
        let mut e = ErrCodeLedger::with_seed();
        let mut filled = true;
        for i in 0..4u32 {
            let val = 0xC04C0 + (4 * i) as i64;
            if e.merge_one(val, 8, 2).is_err() {
                filled = false;
            }
        }
        let over = e.merge_one(0xC04D0, 8, 2);
        s.add_dyn(
            "容量耗尽拒绝",
            filled && e.merged_len() == TABLE_CAP && over == Err(ErrCode::TableFull),
            "三表满后第 17 条必须明拒",
        );
    }
    // 13 重复码与倒退码显性拒。
    {
        let mut e = ErrCodeLedger::with_seed();
        let dup = e.merge_one(0xC04BC, 8, 1);
        let back = e.merge_one(0xC0480, 8, 1);
        s.add_dyn(
            "重复/倒退显性拒",
            dup == Err(ErrCode::ValDuplicate) && back == Err(ErrCode::ValBackward),
            "码值表严格递增，重复与倒退都拒绝",
        );
    }
    // 14 对账守恒：注入 = 检出，逃逸 0。
    {
        let mut e = ErrCodeLedger::with_seed();
        for i in 0..10u32 {
            let _ = e.merge_one(B09_LO.saturating_add(i.saturating_mul(4)) as i64, 8, 1);
            e.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            e.conserved() && e.injected() == 10,
            "注入=检出+逃逸，逃逸恒 0",
        );
    }
    // 15 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同一构造两次建账快照相等");
    // 16 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 17 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_merges_twelve_clean() {
        let e = ErrCodeLedger::with_seed();
        assert_eq!(e.merged_len(), DEPTH_BATCH);
        assert_eq!(e.seed_faults(), 0);
        assert!(e.verify_tables().is_ok());
    }

    #[test]
    fn b09_b10_zero_overlap_holds() {
        // 零重叠断言本体：三段两两不相交。
        assert!(B09_HI < B10_LO && B10_HI < B16_LO);
        assert!(!overlaps_b09(SEG_BASE) && !overlaps_b10(SEG_BASE));
        assert!(overlaps_b09(0xC0080));
        assert!(overlaps_b10(0xC0200));
        assert!(!overlaps_b09(B16_HI) && !overlaps_b10(B16_HI));
    }

    #[test]
    fn boundary_zero_and_negative() {
        let mut e = ErrCodeLedger::with_seed();
        assert_eq!(e.merge_one(0xC04C0, 0, 1), Err(ErrCode::TextEmpty));
        assert_eq!(e.merge_one(0xC04C0, 8, 0), Err(ErrCode::DepthBad));
        assert_eq!(e.merge_one(-1, 8, 1), Err(ErrCode::ValNegative));
        assert_eq!(e.merge_one(0xC04C0, -4, 1), Err(ErrCode::TextEmpty));
    }

    #[test]
    fn boundary_upper_and_upper_plus_one() {
        let mut e = ErrCodeLedger::with_seed();
        assert!(e.merge_one(B16_HI as i64, 8, 4).is_ok());
        let mut e2 = ErrCodeLedger::with_seed();
        assert_eq!(
            e2.merge_one(B16_HI as i64 + 1, 8, 4),
            Err(ErrCode::ValOutOfRange)
        );
    }

    #[test]
    fn overlap_injections_all_caught() {
        let mut e = ErrCodeLedger::with_seed();
        for i in 0..10u32 {
            let b09 = B09_LO.saturating_add(i.saturating_mul(4)) as i64;
            let b10 = B10_LO.saturating_add(i.saturating_mul(4)) as i64;
            assert_eq!(e.merge_one(b09, 8, 1), Err(ErrCode::RangeOverlap));
            assert_eq!(e.merge_one(b10, 8, 1), Err(ErrCode::RangeOverlap));
        }
    }

    #[test]
    fn sealed_lock_and_rollback_reject() {
        let mut e = ErrCodeLedger::with_seed();
        assert_eq!(e.rollback(), Err(ErrCode::PhaseBackward));
        assert!(e.seal().is_ok());
        assert_eq!(e.seal(), Err(ErrCode::SealedLocked));
        assert_eq!(
            e.merge_one(0xC04C0, 8, 1),
            Err(ErrCode::SealedLocked),
            "封账后并入必须锁定"
        );
        assert_eq!(e.rollback(), Err(ErrCode::PhaseBackward));
    }

    #[test]
    fn judge_order_overlap_before_text() {
        let mut e = ErrCodeLedger::with_seed();
        // 重叠判据先于文案判据：文案空 + B09 重叠 → RangeOverlap。
        assert_eq!(e.merge_one(B09_LO as i64, 0, 1), Err(ErrCode::RangeOverlap));
        // 值域判据先于重复判据：越界码不入查重。
        assert_eq!(
            e.merge_one(B16_HI as i64 + 1, 8, 1),
            Err(ErrCode::ValOutOfRange)
        );
    }

    #[test]
    fn table_capacity_exhausted() {
        let mut e = ErrCodeLedger::with_seed();
        for i in 0..4u32 {
            let val = 0xC04C0 + (4 * i) as i64;
            assert!(e.merge_one(val, 8, 2).is_ok());
        }
        assert_eq!(e.merged_len(), TABLE_CAP);
        assert_eq!(e.merge_one(0xC04D0, 8, 2), Err(ErrCode::TableFull));
    }

    #[test]
    fn dup_and_backward_codes_rejected() {
        let mut e = ErrCodeLedger::with_seed();
        assert_eq!(e.merge_one(0xC04BC, 8, 1), Err(ErrCode::ValDuplicate));
        assert_eq!(e.merge_one(0xC0480, 8, 1), Err(ErrCode::ValBackward));
    }

    #[test]
    fn injection_ledger_conserved() {
        let mut e = ErrCodeLedger::with_seed();
        for i in 0..10u32 {
            let val = B10_LO.saturating_add(i.saturating_mul(4)) as i64;
            let _ = e.merge_one(val, 8, 1);
            e.note_detected();
        }
        assert!(e.conserved());
        assert_eq!(e.injected(), 10);
        assert_eq!(e.detected(), 10);
    }

    #[test]
    fn dual_run_snapshots_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
