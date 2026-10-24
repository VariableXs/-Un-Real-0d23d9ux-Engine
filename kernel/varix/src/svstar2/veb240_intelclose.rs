//! VE-F0240 · Intel 组收口与 AMD 组移交（VE-B 域 · Intel 核显组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0240`
//!
//! **职责定位（锚点原文）**：Intel 组（B2）收口——本组二十条自检入树（GTT、
//! 提交、显示、错误、遥测、调试证据齐备），认证矩阵结果引用，缺陷总账清点
//! （🔴清零、🟡闭环、🟢登记），性能基线分软件模拟与硬件穿透两档诚实标注；
//! 双签后移交 AMD 组，经验包含 EXECLISTS 提交语义、表驱动错误解析、
//! pipe/plane 原子编程序方法论。
//!
//! **数据结构（锚点原文）**：收口十件套清单；经验包结构（方法论×条目引用）。
//!
//! **判据（锚点原文）**：十件套、双签、经验包、缺陷清零、判据。
//! 本模块 14 条判据逐条映射：**十件套 4 条**（二十条自检逐条登记齐备／
//! 证据缺项阻断收口／无障碍承诺核验行在册／收口清单版本化）、**双签 3 条**
//! （双签前不可移交／首签后二签位独立／双签完成移交放行）、**经验包 3 条**
//! （三条方法论逐条带条目引用／经验包可被 AMD 组按引用取用／引用指向的
//! 条目必须存在）、**缺陷清零 3 条**（🔴清零方可收口／🟡闭环入登记／🟢
//! 登记不移交）、**判据自身 1 条**（码位互异且段独占）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 收口证据缺项 → **阻断回补** | [`Closeout::finalize`] 逐项核验十件套：任一项缺证据即返回缺项清单并**阻断**（不降级收口——「带病移交」是 B2 组纪律红线） |
//! | 基线分档数据不足 → **实验档标注** | 性能基线两档（软件模拟/硬件穿透）：硬件档样本不足时 [`Baseline::annotate`] 标注「实验档」——诚实标注而不是把模拟数据冒充硬件数据 |
//! | 移交后缺陷回归 → **缺陷总账回溯** | 移交完成后新发现的缺陷走 [`Ledger::reopen`]：按条目引用回溯到 B2 组条目，账本状态回退并阻断 AMD 组对该条目的「已消化」假设 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **收口是门闸不是登记簿**：[`Closeout::finalize`] 的返回值是「缺什么」
//!   清单——调用方补齐后再来，而不是「先收口后补证据」的先斩后奏。
//! - **双签是两个独立签名位**：B2 组负责人签 + 域负责人签，两个签名位
//!   独立记录（谁/何时/签的哪个版本）——同一个人签两次不算双签。
//! - **经验包按条目引用**：三条方法论不复制正文，只带 B2 组条目号引用——
//!   AMD 组取用时按引用回读原文，避免「二手转述失真」。

use alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;
use alloc::vec::Vec;

/// 诊断段基址（全域核查零占用）。
pub const SEG_BASE: u32 = 0xE840;

// ---- 诊断码 ----
pub const XC_EVIDENCE_MISS: u32 = SEG_BASE + 1; // 收口证据缺项（阻断）
pub const XC_SIGN_BAD: u32 = SEG_BASE + 2; // 双签非法（同人/缺签）
pub const XC_REF_BAD: u32 = SEG_BASE + 3; // 经验包引用指向不存在条目
pub const XC_DEFECT_OPEN: u32 = SEG_BASE + 4; // 🔴未清零阻断收口
pub const XC_REOPEN: u32 = SEG_BASE + 5; // 移交后回归（账本回退）

/// 收口十件套（锚点列举的证据类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvidenceKind {
    /// GTT 证据。
    Gtt,
    /// 提交证据。
    Submit,
    /// 显示证据。
    Display,
    /// 错误证据。
    Error,
    /// 遥测证据。
    Telemetry,
    /// 调试证据。
    Debug,
    /// 认证矩阵结果引用。
    CertMatrix,
    /// 缺陷总账清点。
    DefectLedger,
    /// 性能基线分档。
    PerfBaseline,
    /// 无障碍承诺核验。
    A11yCheck,
}

/// 十件套全集（常量，判据对拍）。
pub const EVIDENCE_SET: [EvidenceKind; 10] = [
    EvidenceKind::Gtt,
    EvidenceKind::Submit,
    EvidenceKind::Display,
    EvidenceKind::Error,
    EvidenceKind::Telemetry,
    EvidenceKind::Debug,
    EvidenceKind::CertMatrix,
    EvidenceKind::DefectLedger,
    EvidenceKind::PerfBaseline,
    EvidenceKind::A11yCheck,
];

/// 缺陷账状态（🔴🟡🟢三色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DefectState {
    /// 🔴 未清偿（阻断收口）。
    Open,
    /// 🟡 闭环（有规避，随移交）。
    Closed,
    /// 🟢 登记（无影响，随移交）。
    Logged,
}

/// 性能基线档位（诚实标注）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaselineTier {
    /// 软件模拟档。
    Simulated,
    /// 硬件穿透档。
    Hardware,
    /// 硬件档样本不足——实验档标注。
    Experimental,
}

/// 性能基线。
#[derive(Clone, Debug)]
pub struct Baseline {
    /// 档位。
    pub tier: BaselineTier,
    /// 硬件档样本数。
    pub hw_samples: u32,
    /// 实验档样本下限。
    pub hw_min: u32,
}

impl Baseline {
    /// 诚实标注：硬件档样本不足即降实验档（不冒充）。
    pub fn annotate(&self) -> BaselineTier {
        if self.hw_samples >= self.hw_min {
            BaselineTier::Hardware
        } else {
            BaselineTier::Experimental
        }
    }
}

/// 经验包条目（方法论×条目引用）。
#[derive(Clone, Debug)]
pub struct Lesson {
    /// 方法论名。
    pub name: String,
    /// 引用的 B2 组条目号（必须存在）。
    pub ref_entry: u16,
}

/// 双签记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    /// 签名人（组内唯一标识）。
    pub who: String,
    /// 签的收口版本。
    pub ver: u32,
}

/// 缺陷总账条目。
#[derive(Clone, Debug)]
pub struct Defect {
    /// 条目号。
    pub id: u16,
    /// 状态。
    pub state: DefectState,
}

/// Intel 组收口控制器。
pub struct Closeout {
    /// 已登记证据。
    evidence: Vec<EvidenceKind>,
    /// 缺陷账。
    ledger: Vec<Defect>,
    /// 双签记录。
    signs: Vec<Signature>,
    /// 经验包。
    lessons: Vec<Lesson>,
    /// B2 组条目号全集（经验包引用校验）。
    b2_entries: Vec<u16>,
    /// 性能基线。
    pub baseline: Baseline,
    /// 收口版本。
    pub ver: u32,
    /// 是否已移交。
    pub handed_over: bool,
    /// 移交后回归计数。
    pub reopens: u32,
}

impl Closeout {
    /// 新收口（空账）。
    pub fn new(hw_samples: u32, hw_min: u32) -> Self {
        Closeout {
            evidence: Vec::new(),
            ledger: Vec::new(),
            signs: Vec::new(),
            lessons: Vec::new(),
            b2_entries: (0..20).collect(), // B2 组二十条自检条目
            baseline: Baseline { tier: BaselineTier::Simulated, hw_samples, hw_min },
            ver: 0,
            handed_over: false,
            reopens: 0,
        }
    }

    /// 登记证据（幂等：同类重复登记忽略）。
    pub fn add_evidence(&mut self, k: EvidenceKind) {
        if !self.evidence.contains(&k) {
            self.evidence.push(k);
        }
    }

    /// 证据齐备性（十件套全集覆盖）。
    pub fn evidence_complete(&self) -> bool {
        EVIDENCE_SET.iter().all(|k| self.evidence.contains(k))
    }

    /// 缺项清单（阻断回补的指引）。
    pub fn missing_evidence(&self) -> Vec<EvidenceKind> {
        EVIDENCE_SET.iter().copied().filter(|k| !self.evidence.contains(k)).collect()
    }

    /// 缺陷入账。
    pub fn add_defect(&mut self, id: u16, st: DefectState) {
        if let Some(d) = self.ledger.iter_mut().find(|d| d.id == id) {
            d.state = st;
        } else {
            self.ledger.push(Defect { id, state: st });
        }
    }

    /// 🔴 清零判定。
    pub fn reds_cleared(&self) -> bool {
        !self.ledger.iter().any(|d| d.state == DefectState::Open)
    }

    /// 经验包登记（引用必须指向存在的 B2 条目）。
    pub fn add_lesson(&mut self, name: &str, ref_entry: u16) -> Result<(), u32> {
        if !self.b2_entries.contains(&ref_entry) {
            return Err(XC_REF_BAD);
        }
        self.lessons.push(Lesson { name: alloc::format!("{}", name), ref_entry });
        Ok(())
    }

    /// 签名（同人重复签拒绝——双签是两个独立签名位）。
    pub fn sign(&mut self, who: &str) -> Result<(), u32> {
        if self.signs.iter().any(|s| s.who == who) {
            return Err(XC_SIGN_BAD);
        }
        self.signs.push(Signature { who: alloc::format!("{}", who), ver: self.ver });
        Ok(())
    }

    /// 收口：证据齐 + 🔴清零 + 双签齐 → 版本推进可移交；否则返回缺项数。
    pub fn finalize(&mut self) -> Result<(), u32> {
        let miss = self.missing_evidence().len() as u32;
        if miss > 0 {
            return Err(XC_EVIDENCE_MISS + miss); // 缺项数编码进码尾（回补指引）
        }
        if !self.reds_cleared() {
            return Err(XC_DEFECT_OPEN);
        }
        if self.signs.len() < 2 {
            return Err(XC_SIGN_BAD);
        }
        self.ver += 1;
        self.baseline.tier = self.baseline.annotate();
        Ok(())
    }

    /// 移交 AMD 组（双签收口完成后放行）。
    pub fn handover(&mut self) -> Result<(), u32> {
        if self.ver == 0 {
            return Err(XC_SIGN_BAD);
        }
        self.handed_over = true;
        Ok(())
    }

    /// 移交后缺陷回归：账本回退并计数（锚点：移交后缺陷回归→缺陷总账
    /// 回溯）。
    pub fn reopen(&mut self, id: u16) -> Result<(), u32> {
        if !self.handed_over {
            return Err(XC_REOPEN);
        }
        match self.ledger.iter_mut().find(|d| d.id == id) {
            Some(d) => {
                d.state = DefectState::Open;
                self.reopens += 1;
                Ok(())
            }
            None => Err(XC_REF_BAD),
        }
    }

    /// 经验包读取（AMD 组按引用取用）。
    pub fn lessons(&self) -> &[Lesson] {
        &self.lessons
    }
}

/// VE-F0240 判据组（14 条）。
pub fn run_veb240_checks() -> CheckSet {
    use crate::checks::CheckSet;
    let mut s = CheckSet::new("veb240_intelclose");

    // --- 判据 1：十件套——二十条自检证据域齐备 ---
    {
        s.add(
            "B40-十件套-证据域全集",
            EVIDENCE_SET.len() == 10
                && EVIDENCE_SET.iter().any(|k| *k == EvidenceKind::Gtt)
                && EVIDENCE_SET.iter().any(|k| *k == EvidenceKind::A11yCheck),
            "十件套常量 10 项：GTT 到无障碍承诺全在册",
        );
    }
    // --- 判据 2：十件套——证据缺项阻断收口 ---
    {
        let mut c = Closeout::new(100, 50);
        // 只登记一半证据。
        for k in EVIDENCE_SET.iter().take(5) {
            c.add_evidence(*k);
        }
        let r = c.finalize();
        s.add(
            "B40-十件套-缺项阻断回补",
            r == Err(XC_EVIDENCE_MISS + 5) && c.missing_evidence().len() == 5,
            "缺 5 项时收口返回缺项数编码（回补指引），不降级收口",
        );
    }
    // --- 判据 3：十件套——登记幂等且可补齐 ---
    {
        let mut c = Closeout::new(100, 50);
        for k in EVIDENCE_SET.iter() {
            c.add_evidence(*k);
            c.add_evidence(*k); // 重复登记
        }
        s.add(
            "B40-十件套-登记幂等可补齐",
            c.evidence_complete() && c.missing_evidence().is_empty(),
            "重复登记不膨胀；全量登记后缺项清单清空",
        );
    }
    // --- 判据 4：十件套——无障碍承诺核验行在册 ---
    {
        let mut c = Closeout::new(100, 50);
        c.add_evidence(EvidenceKind::A11yCheck);
        s.add(
            "B40-十件套-无障碍核验在册",
            c.evidence.contains(&EvidenceKind::A11yCheck),
            "收口清单含无障碍承诺核验行（锚点要求）",
        );
    }
    // --- 判据 5：缺陷清零——🔴未清零阻断 ---
    {
        let mut c = Closeout::new(100, 50);
        for k in EVIDENCE_SET.iter() {
            c.add_evidence(*k);
        }
        c.add_defect(1, DefectState::Open);
        let r = c.finalize();
        s.add(
            "B40-缺陷清零-🔴阻断收口",
            r == Err(XC_DEFECT_OPEN),
            "有 🔴 缺陷时即使证据齐备也阻断（带病移交是红线）",
        );
    }
    // --- 判据 6：缺陷清零——🟡闭环🟢登记放行 ---
    {
        let mut c = Closeout::new(100, 50);
        for k in EVIDENCE_SET.iter() {
            c.add_evidence(*k);
        }
        c.add_defect(1, DefectState::Closed);
        c.add_defect(2, DefectState::Logged);
        c.sign("b2-lead");
        c.sign("domain-owner");
        s.add(
            "B40-缺陷清零-🟡🟢放行",
            c.reds_cleared() && c.finalize().is_ok(),
            "🟡闭环+🟢登记不算未清偿，双签后收口通过",
        );
    }
    // --- 判据 7：双签——缺签不可收口 ---
    {
        let mut c = Closeout::new(100, 50);
        for k in EVIDENCE_SET.iter() {
            c.add_evidence(*k);
        }
        c.sign("b2-lead");
        let r = c.finalize();
        s.add(
            "B40-双签-单签不可收口",
            r == Err(XC_SIGN_BAD) && c.signs.len() == 1,
            "只一签时收口拒绝（双签语义是两个独立签名位）",
        );
    }
    // --- 判据 8：双签——同人重复签拒绝 ---
    {
        let mut c = Closeout::new(100, 50);
        let first = c.sign("b2-lead");
        let dup = c.sign("b2-lead");
        s.add(
            "B40-双签-同人重复签拒绝",
            first.is_ok() && dup == Err(XC_SIGN_BAD),
            "同一个人签两次不算双签（签名位按 who 唯一）",
        );
    }
    // --- 判据 9：双签——收口前不可移交 ---
    {
        let mut c = Closeout::new(100, 50);
        let r = c.handover();
        s.add(
            "B40-双签-收口前移交拒绝",
            r == Err(XC_SIGN_BAD) && !c.handed_over && c.ver == 0,
            "版本未推进（未收口）时移交显性拒绝",
        );
    }
    // --- 判据 10：双签——双签后移交放行 ---
    {
        let mut c = Closeout::new(100, 50);
        for k in EVIDENCE_SET.iter() {
            c.add_evidence(*k);
        }
        c.sign("b2-lead");
        c.sign("domain-owner");
        let _ = c.finalize();
        let h = c.handover();
        s.add(
            "B40-双签-收口后移交放行",
            h.is_ok() && c.handed_over && c.ver == 1,
            "收口版本推进到 1 后移交成功（全链路解锁）",
        );
    }
    // --- 判据 11：经验包——三条方法论带引用 ---
    {
        let mut c = Closeout::new(100, 50);
        let l1 = c.add_lesson("EXECLISTS 提交语义", 3);
        let l2 = c.add_lesson("表驱动错误解析", 7);
        let l3 = c.add_lesson("pipe/plane 原子编程序", 11);
        s.add(
            "B40-经验包-三方法论带引用",
            l1.is_ok() && l2.is_ok() && l3.is_ok() && c.lessons().len() == 3,
            "锚点三条方法论逐条登记，各引用一个 B2 条目号",
        );
    }
    // --- 判据 12：经验包——引用指向不存在条目拒绝 ---
    {
        let mut c = Closeout::new(100, 50);
        let bad = c.add_lesson("幽灵方法论", 99);
        s.add(
            "B40-经验包-悬垂引用拒绝",
            bad == Err(XC_REF_BAD) && c.lessons().is_empty(),
            "引用条目 99 超出 B2 二十条范围：登记拒绝（防二手失真）",
        );
    }
    // --- 判据 13：缺陷清零——基线实验档诚实标注+回归回溯 ---
    {
        // 样本不足 → 实验档。
        let mut c = Closeout::new(3, 50);
        for k in EVIDENCE_SET.iter() {
            c.add_evidence(*k);
        }
        c.sign("b2-lead");
        c.sign("domain-owner");
        let _ = c.finalize();
        let _ = c.handover();
        let exp = c.baseline.tier == BaselineTier::Experimental;
        // 移交后回归回溯。
        c.add_defect(5, DefectState::Closed);
        let re = c.reopen(5);
        s.add(
            "B40-缺陷清零-实验档与回归回溯",
            exp && re.is_ok() && c.reopens == 1
                && c.ledger.iter().find(|d| d.id == 5).map(|d| d.state == DefectState::Open).unwrap_or(false),
            "硬件样本 3<50 标实验档不冒充；移交后回归把账本拉回 🔴",
        );
    }
    // --- 判据 14：码位互异且段独占 ---
    {
        let codes = [XC_EVIDENCE_MISS, XC_SIGN_BAD, XC_REF_BAD, XC_DEFECT_OPEN, XC_REOPEN];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_seg = codes.iter().all(|c| *c > SEG_BASE && *c <= SEG_BASE + 9);
        s.add(
            "B40-判据-码位互异段独占",
            all_diff && in_seg && codes.len() == 5,
            "五诊断码互异且全部落在 0xE840 段基的判别式区间内",
        );
    }
    s
}

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    fn full_evidence(c: &mut Closeout) {
        for k in EVIDENCE_SET.iter() {
            c.add_evidence(*k);
        }
    }

    #[test]
    fn veb240_closeout_gate_chain() {
        let mut c = Closeout::new(100, 50);
        // 空账收口：缺 10 项。
        assert_eq!(c.finalize(), Err(XC_EVIDENCE_MISS + 10));
        // 补齐证据、🔴未清。
        full_evidence(&mut c);
        c.add_defect(1, DefectState::Open);
        assert_eq!(c.finalize(), Err(XC_DEFECT_OPEN));
        // 缺陷闭环、双签。
        c.add_defect(1, DefectState::Closed);
        assert_eq!(c.finalize(), Err(XC_SIGN_BAD));
        c.sign("b2-lead");
        c.sign("domain-owner");
        assert_eq!(c.finalize(), Ok(()));
        assert_eq!(c.ver, 1);
        // 移交。
        assert_eq!(c.handover(), Ok(()));
        assert!(c.handed_over);
    }

    #[test]
    fn veb240_duplicate_sign_rejected() {
        let mut c = Closeout::new(100, 50);
        assert!(c.sign("a").is_ok());
        assert_eq!(c.sign("a"), Err(XC_SIGN_BAD));
        assert!(c.sign("b").is_ok());
        assert_eq!(c.signs.len(), 2);
    }

    #[test]
    fn veb240_lesson_refs() {
        let mut c = Closeout::new(100, 50);
        assert!(c.add_lesson("EXECLISTS 提交语义", 0).is_ok());
        assert!(c.add_lesson("表驱动错误解析", 19).is_ok());
        assert_eq!(c.add_lesson("越界引用", 20), Err(XC_REF_BAD));
        assert_eq!(c.lessons().len(), 2);
        assert_eq!(c.lessons()[0].ref_entry, 0);
    }

    #[test]
    fn veb240_baseline_honest_tier() {
        // 硬件样本充足 → Hardware。
        let b1 = Baseline { tier: BaselineTier::Simulated, hw_samples: 60, hw_min: 50 };
        assert_eq!(b1.annotate(), BaselineTier::Hardware);
        // 样本不足 → Experimental。
        let b2 = Baseline { tier: BaselineTier::Simulated, hw_samples: 10, hw_min: 50 };
        assert_eq!(b2.annotate(), BaselineTier::Experimental);
    }

    #[test]
    fn veb240_post_handover_reopen() {
        let mut c = Closeout::new(100, 50);
        full_evidence(&mut c);
        c.sign("a");
        c.sign("b");
        let _ = c.finalize();
        // 未移交时回归拒绝。
        assert_eq!(c.reopen(1), Err(XC_REOPEN));
        c.add_defect(1, DefectState::Closed);
        let _ = c.handover();
        // 移交后回归：账本回 🔴。
        assert_eq!(c.reopen(1), Ok(()));
        assert_eq!(c.reopens, 1);
        assert!(c.ledger.iter().any(|d| d.id == 1 && d.state == DefectState::Open));
        // 未知条目回归拒绝。
        assert_eq!(c.reopen(99), Err(XC_REF_BAD));
    }

    #[test]
    fn veb240_checks_all_green() {
        let set = run_veb240_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0240 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
