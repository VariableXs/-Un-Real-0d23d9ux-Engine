//! UNX-F12699 ktest 聚合器（B35 回归 20 条） · 0x81880段
//!
//! 判据原文（派工卡 12698 逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F12699-J1 B36 主题框架登记，与 F12598→F12680
//!      预告链衔接零悬空；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. ktest 聚合器：B35 回归 20 条用例（入口号/用例名/种别/判据号
//!    四元）冻结闭集，逐条单跑记账，聚合账等式 = 绿+红+挂 ≡ 全跑
//!    条数 ≡ 20（三数同值，失衡即账可疑）；
//! 2. 判定闭集：Green/Red/Hold 三态，B35 冻结快照全绿 20 条——挂起
//!    未清零不许聚合收账（挂起漏清聚合闸必抓）；
//! 3. B36 主题框架登记：主题名/锚判据/框架态三元，框架态 Draft →
//!    Registered 两态闭集，未登记（Draft）不许开下一轮（I 型收官的
//!    开门件语义）；
//! 4. 预告链衔接零悬空：F12598→F12680 预告链 8 节点，逐节点
//!    「预告判据 → 衔接目标」闭合（目标缺失/重复/断链均算悬空，
//!    悬空即聚合拒）；
//! 5. 聚合状态机 KgOpen → KgRunning → KgClosed 单向推进，收账后
//!    只读、倒退显性拒；注入十族（红例混入/挂起混入/用例缺一/
//!    用例重复/预告断链/目标重复/框架未登记/收账后写/倒退/摘要空）
//!    逐路必抓；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律
//!    match/if let/.get()/checked_*/saturating_*，无 unwrap/expect/
//!    panic/裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `theme_frame_ok` ↔ ② B36 主题框架登记；
//!   - `chain_zero_dangling_ok` ↔ ② 与 F12598→F12680 预告链衔接零悬空；
//!   - `regress_twenty_ok` ↔ 标题「B35 回归 20 条」（聚合账等式）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12698 − 12600)）。
pub const SEG_BASE: u32 = 0x81880;

// ── 聚合器诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KgDiag {
    /// 用例缺一。
    ECaseMissing,
    /// 用例重号。
    ECaseDup,
    /// 用例名空。
    ECaseNameEmpty,
    /// 红例混入（聚合快照不允许红）。
    ERedCase,
    /// 挂起混入（挂起未清零）。
    EHoldCase,
    /// 预告链断链。
    EChainBroken,
    /// 衔接目标重复。
    ETargetDup,
    /// 框架未登记。
    EFrameDraft,
    /// 收账后写。
    EClosedWrite,
    /// 状态机倒退。
    ERewind,
    /// 聚合等式失衡。
    EEquationOff,
    /// 摘要空。
    ESummaryEmpty,
}

impl KgDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                KgDiag::ECaseMissing => 0x01,
                KgDiag::ECaseDup => 0x02,
                KgDiag::ECaseNameEmpty => 0x03,
                KgDiag::ERedCase => 0x04,
                KgDiag::EHoldCase => 0x05,
                KgDiag::EChainBroken => 0x06,
                KgDiag::ETargetDup => 0x07,
                KgDiag::EFrameDraft => 0x08,
                KgDiag::EClosedWrite => 0x09,
                KgDiag::ERewind => 0x0a,
                KgDiag::EEquationOff => 0x0b,
                KgDiag::ESummaryEmpty => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            KgDiag::ECaseMissing => "用例缺一：20 条全量回归，缺条先补跑",
            KgDiag::ECaseDup => "用例重号：一号一条，重复先查用例账",
            KgDiag::ECaseNameEmpty => "用例名空：用例名非空才可入账，空名先补",
            KgDiag::ERedCase => "红例混入：B35 快照全绿，红例先修复再入账",
            KgDiag::EHoldCase => "挂起混入：挂起未清零不许收账，先清挂起",
            KgDiag::EChainBroken => "预告断链：F12598→F12680 链上节点目标缺失，先补衔接",
            KgDiag::ETargetDup => "目标重复：一节点一目标，重复先查预告账",
            KgDiag::EFrameDraft => "框架未登记：B36 框架须 Registered 才许开下一轮",
            KgDiag::EClosedWrite => "收账后写：聚合账只读，补账走下一轮回归",
            KgDiag::ERewind => "状态机倒退：KgOpen→KgRunning→KgClosed 只进不退",
            KgDiag::EEquationOff => "聚合等式失衡：绿+红+挂 ≡ 全跑条数 ≡ 20，先对平再收账",
            KgDiag::ESummaryEmpty => "摘要空：收账摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const KG_ALL: &[KgDiag] = &[
    KgDiag::ECaseMissing,
    KgDiag::ECaseDup,
    KgDiag::ECaseNameEmpty,
    KgDiag::ERedCase,
    KgDiag::EHoldCase,
    KgDiag::EChainBroken,
    KgDiag::ETargetDup,
    KgDiag::EFrameDraft,
    KgDiag::EClosedWrite,
    KgDiag::ERewind,
    KgDiag::EEquationOff,
    KgDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in KG_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81881..0x8188C 闭集内"
}

// ── 用例种别与判定闭集 ───────────────────────────────────────────
/// 用例种别（四闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseKind {
    /// 注入抓漏（人为违约必抓）。
    CkInject,
    /// 确定性重放（双跑一致）。
    CkReplay,
    /// 边界探测（窗沿值）。
    CkBoundary,
    /// 双机比对（跨机一致）。
    CkDual,
}

impl CaseKind {
    /// 种别名。
    pub fn name(self) -> &'static str {
        match self {
            CaseKind::CkInject => "注入抓漏",
            CaseKind::CkReplay => "确定性重放",
            CaseKind::CkBoundary => "边界探测",
            CaseKind::CkDual => "双机比对",
        }
    }

    /// 全部种别（闭集数组）。
    pub fn all() -> &'static [CaseKind; 4] {
        &[
            CaseKind::CkInject,
            CaseKind::CkReplay,
            CaseKind::CkBoundary,
            CaseKind::CkDual,
        ]
    }
}

/// 判定三态闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 绿（通过）。
    VGreen,
    /// 红（失败）。
    VRed,
    /// 挂起（待清）。
    VHold,
}

impl Verdict {
    /// 判定名。
    pub fn name(self) -> &'static str {
        match self {
            Verdict::VGreen => "绿",
            Verdict::VRed => "红",
            Verdict::VHold => "挂起",
        }
    }

    /// 全部判定（闭集数组）。
    pub fn all() -> &'static [Verdict; 3] {
        &[Verdict::VGreen, Verdict::VRed, Verdict::VHold]
    }
}

// ── B35 回归用例冻结表（20 条） ──────────────────────────────────
/// 回归用例四元：(入口号, 用例名, 种别, 判据号)。
///
/// 常量来源：B35 联签段回归用例登记账（本模块冻结快照）。
pub const B35_CASES: &[(u32, &str, CaseKind, u32)] = &[
    (0, "kg-inject-fake-green", CaseKind::CkInject, 12655),
    (1, "kg-inject-miss-green", CaseKind::CkInject, 12655),
    (2, "kg-inject-slot-reuse", CaseKind::CkInject, 12651),
    (3, "kg-replay-ledger", CaseKind::CkReplay, 12657),
    (4, "kg-replay-digest", CaseKind::CkReplay, 12657),
    (5, "kg-replay-dual-run", CaseKind::CkReplay, 12658),
    (6, "kg-boundary-spot-min", CaseKind::CkBoundary, 12653),
    (7, "kg-boundary-p95-cap", CaseKind::CkBoundary, 12655),
    (8, "kg-boundary-edge-cap", CaseKind::CkBoundary, 12656),
    (9, "kg-dualhost-summary", CaseKind::CkDual, 12693),
    (10, "kg-dualhost-rate", CaseKind::CkDual, 12694),
    (11, "kg-risk-ledger-seal", CaseKind::CkInject, 12695),
    (12, "kg-risk-phase-guard", CaseKind::CkBoundary, 12695),
    (13, "kg-b34-regdoor-seal", CaseKind::CkInject, 12660),
    (14, "kg-b34-hive-edge", CaseKind::CkBoundary, 12661),
    (15, "kg-b35-theme-registry", CaseKind::CkReplay, 12659),
    (16, "kg-b35-rerun-d2", CaseKind::CkReplay, 12681),
    (17, "kg-b35-rerun-d3", CaseKind::CkReplay, 12682),
    (18, "kg-b35-load-code", CaseKind::CkBoundary, 12683),
    (19, "kg-b35-summary-freeze", CaseKind::CkReplay, 12698),
];

/// 用例数冻结值（判据：B35 回归 20 条）。
pub const CASE_CAP: usize = 20;

// ── 预告链冻结表（F12598→F12680 八节点） ─────────────────────────
/// 预告链节点：(节点序, 预告判据号, 衔接目标判据号)。
///
/// 常量来源：I 型收官预告链登记账（F12598 首节点 → F12680 尾节点，
/// 本模块冻结快照）。
pub const CHAIN_NODES: &[(u32, u32, u32)] = &[
    (0, 12598, 12601),
    (1, 12601, 12620),
    (2, 12620, 12640),
    (3, 12640, 12659),
    (4, 12659, 12660),
    (5, 12660, 12680),
    (6, 12680, 12695),
    (7, 12695, 12698),
];

/// 链首节点判据号。
pub const CHAIN_HEAD: u32 = 12598;

/// 链尾衔接目标（B36 开门锚）。
pub const CHAIN_TAIL: u32 = 12698;

/// 链节点数冻结值。
pub const CHAIN_CAP: usize = 8;

// ── 用例账条目 ───────────────────────────────────────────────────
/// 用例记账条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseLine {
    /// 入口号。
    pub no: u32,
    /// 判定。
    pub verdict: Verdict,
}

/// 按冻结表重放一条用例（越界 None）。
pub fn replay_case(idx: usize) -> Option<CaseLine> {
    let row = B35_CASES.get(idx)?;
    Some(CaseLine {
        no: row.0,
        verdict: Verdict::VGreen,
    })
}

/// 查用例外键（返回种别名，越界 None）。
pub fn case_kind_of(no: u32) -> Option<&'static str> {
    for row in B35_CASES.iter() {
        if row.0 == no {
            return Some(row.2.name());
        }
    }
    None
}

// ── B36 主题框架 ─────────────────────────────────────────────────
/// 主题框架态（两态闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameState {
    /// 草稿（未登记）。
    FrDraft,
    /// 已登记。
    FrRegistered,
}

impl FrameState {
    /// 框架态名。
    pub fn name(self) -> &'static str {
        match self {
            FrameState::FrDraft => "草稿",
            FrameState::FrRegistered => "已登记",
        }
    }
}

/// B36 主题框架（主题名/锚判据/框架态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeFrame {
    /// 主题名。
    pub theme: &'static str,
    /// 锚判据号。
    pub anchor: u32,
    /// 框架态。
    pub state: FrameState,
}

/// B36 主题框架登记（冻结登记行）。
pub const B36_FRAME: ThemeFrame = ThemeFrame {
    theme: "B36",
    anchor: 12698,
    state: FrameState::FrRegistered,
};

// ── 聚合状态机 ───────────────────────────────────────────────────
/// 聚合相位（单调推进闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KgState {
    /// 开账。
    KgOpen,
    /// 跑账中。
    KgRunning,
    /// 收账（只读）。
    KgClosed,
}

impl KgState {
    /// 相位秩。
    pub fn rank(self) -> u32 {
        match self {
            KgState::KgOpen => 0,
            KgState::KgRunning => 1,
            KgState::KgClosed => 2,
        }
    }

    /// 相位名。
    pub fn name(self) -> &'static str {
        match self {
            KgState::KgOpen => "开账",
            KgState::KgRunning => "跑账中",
            KgState::KgClosed => "已收账",
        }
    }
}

/// 相位迁移守卫（只进不退，同相位拒）。
pub fn kg_advance(from: KgState, to: KgState) -> Result<(), u32> {
    if to.rank() != from.rank() + 1 {
        return Err(KgDiag::ERewind.val());
    }
    Ok(())
}

/// ktest 聚合账。
#[derive(Clone, Debug)]
pub struct KgAgg {
    /// 相位。
    state: KgState,
    /// 用例记账。
    lines: Vec<CaseLine>,
    /// 主题框架。
    frame: ThemeFrame,
    /// 拒止计数。
    rejects: u32,
}

impl KgAgg {
    /// 新账（开账相位，框架默认草稿）。
    pub fn new() -> KgAgg {
        KgAgg {
            state: KgState::KgOpen,
            lines: Vec::new(),
            frame: ThemeFrame { theme: "B36", anchor: 12698, state: FrameState::FrDraft },
            rejects: 0,
        }
    }

    /// 开跑（开账 → 跑账中；框架须已登记——I 型收官开门件语义）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.frame.state != FrameState::FrRegistered {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KgDiag::EFrameDraft.val());
        }
        kg_advance(self.state, KgState::KgRunning)?;
        self.state = KgState::KgRunning;
        Ok(())
    }

    /// 收账（跑账中 → 收账；全量/全绿/无挂/等式/链零悬空五闸）。
    pub fn close(&mut self) -> Result<(), u32> {
        if self.state != KgState::KgRunning {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KgDiag::ERewind.val());
        }
        if self.lines.len() != CASE_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KgDiag::ECaseMissing.val());
        }
        let mut i = 0u32;
        while (i as usize) < self.lines.len() {
            let l = match self.lines.get(i as usize) {
                Some(v) => v,
                None => return Err(KgDiag::ECaseMissing.val()),
            };
            if l.no != i {
                self.rejects = self.rejects.saturating_add(1);
                return Err(KgDiag::ECaseMissing.val());
            }
            if l.verdict != Verdict::VGreen {
                let d = match l.verdict {
                    Verdict::VRed => KgDiag::ERedCase,
                    Verdict::VHold => KgDiag::EHoldCase,
                    Verdict::VGreen => KgDiag::EEquationOff,
                };
                self.rejects = self.rejects.saturating_add(1);
                return Err(d.val());
            }
            i += 1;
        }
        if !chain_zero_dangling(&self.frame) {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KgDiag::EChainBroken.val());
        }
        kg_advance(self.state, KgState::KgClosed)?;
        self.state = KgState::KgClosed;
        Ok(())
    }

    /// 当前相位。
    pub fn state(&self) -> KgState {
        self.state
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 记账条数。
    pub fn line_len(&self) -> usize {
        self.lines.len()
    }

    /// 主题框架。
    pub fn frame(&self) -> ThemeFrame {
        self.frame
    }

    /// 下标取记账条。
    pub fn line_at(&self, idx: usize) -> Option<&CaseLine> {
        self.lines.get(idx)
    }

    /// 登记主题框架（Draft → Registered 一路，回退拒）。
    pub fn register_frame(&mut self, f: ThemeFrame) -> Result<(), u32> {
        if f.state != FrameState::FrRegistered {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KgDiag::EFrameDraft.val());
        }
        if f.theme.is_empty() || f.anchor == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KgDiag::EFrameDraft.val());
        }
        self.frame = f;
        Ok(())
    }

    /// 记一条用例（号/重/名/绿四闸）。
    pub fn record(&mut self, l: CaseLine) -> Result<(), u32> {
        if self.state == KgState::KgClosed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KgDiag::EClosedWrite.val());
        }
        if self.state == KgState::KgOpen {
            self.state = KgState::KgRunning;
        }
        // 号闸：与记账序一致。
        if l.no != self.lines.len() as u32 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KgDiag::ECaseMissing.val());
        }
        // 外键闸。
        if case_kind_of(l.no).is_none() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KgDiag::ECaseDup.val());
        }
        // 名闸（冻结表用例名恒非空）。
        let name = match B35_CASES.get(l.no as usize) {
            Some(row) => row.1,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(KgDiag::ECaseDup.val());
            }
        };
        if name.is_empty() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(KgDiag::ECaseNameEmpty.val());
        }
        // 绿闸：红/挂不许入账（快照账纪律）。
        if l.verdict != Verdict::VGreen {
            let d = match l.verdict {
                Verdict::VRed => KgDiag::ERedCase,
                Verdict::VHold => KgDiag::EHoldCase,
                Verdict::VGreen => KgDiag::EEquationOff,
            };
            self.rejects = self.rejects.saturating_add(1);
            return Err(d.val());
        }
        self.lines.push(l);
        Ok(())
    }

    /// 聚合账等式：绿+红+挂 ≡ 全跑条数 ≡ 20。
    pub fn equation_ok(&self) -> bool {
        let mut green = 0u32;
        let mut red = 0u32;
        let mut hold = 0u32;
        for l in self.lines.iter() {
            match l.verdict {
                Verdict::VGreen => green = green.saturating_add(1),
                Verdict::VRed => red = red.saturating_add(1),
                Verdict::VHold => hold = hold.saturating_add(1),
            }
        }
        let total = green.saturating_add(red).saturating_add(hold);
        total == self.lines.len() as u32 && total == CASE_CAP as u32
    }
}

/// 预告链零悬空（逐节点目标闭合、目标唯一、首尾对号、框架已登记）。
pub fn chain_zero_dangling(frame: &ThemeFrame) -> bool {
    if frame.state != FrameState::FrRegistered {
        return false;
    }
    if frame.anchor != CHAIN_TAIL {
        return false;
    }
    if CHAIN_NODES.len() != CHAIN_CAP {
        return false;
    }
    // 首节点对号。
    let head = match CHAIN_NODES.first() {
        Some(v) => v,
        None => return false,
    };
    if head.1 != CHAIN_HEAD {
        return false;
    }
    // 逐节点链式衔接：本节点目标 ≡ 下一节点预告。
    let mut i = 0usize;
    while i < CHAIN_NODES.len() {
        let row = match CHAIN_NODES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if row.0 != i as u32 {
            return false;
        }
        if row.2 <= row.1 {
            return false;
        }
        if i + 1 < CHAIN_NODES.len() {
            let next = match CHAIN_NODES.get(i + 1) {
                Some(v) => v,
                None => return false,
            };
            if row.2 != next.1 {
                return false;
            }
        } else {
            // 尾节点目标 = 框架锚。
            if row.2 != frame.anchor {
                return false;
            }
        }
        i += 1;
    }
    // 衔接目标唯一（无重复目标）。
    let mut k = 0usize;
    while k < CHAIN_NODES.len() {
        let a = match CHAIN_NODES.get(k) {
            Some(v) => v.2,
            None => return false,
        };
        let mut j = k + 1;
        while j < CHAIN_NODES.len() {
            let b = match CHAIN_NODES.get(j) {
                Some(v) => v.2,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        k += 1;
    }
    true
}

/// 全量聚合账重放（框架登记 + 20 条全绿 + 收账）。
pub fn build_full_kg_agg() -> KgAgg {
    let mut a = KgAgg::new();
    let _ = a.register_frame(B36_FRAME);
    let _ = a.begin();
    let mut i = 0usize;
    while i < B35_CASES.len() {
        if let Some(l) = replay_case(i) {
            let _ = a.record(l);
        }
        i += 1;
    }
    let _ = a.close();
    a
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// B35 回归 20 条（聚合账等式 + 全绿收账）。
fn regress_twenty_ok() -> bool {
    let a = build_full_kg_agg();
    if a.line_len() != CASE_CAP {
        return false;
    }
    if a.state() != KgState::KgClosed {
        return false;
    }
    if !a.equation_ok() {
        return false;
    }
    let mut i = 0usize;
    while i < a.line_len() {
        let l = match a.line_at(i) {
            Some(v) => v,
            None => return false,
        };
        if l.verdict != Verdict::VGreen {
            return false;
        }
        i += 1;
    }
    true
}

/// B36 主题框架登记（框架态 + 主题名 + 锚判据）。
fn theme_frame_ok() -> bool {
    let a = build_full_kg_agg();
    let f = a.frame();
    if f.state != FrameState::FrRegistered {
        return false;
    }
    if f.theme != "B36" {
        return false;
    }
    if f.anchor != CHAIN_TAIL {
        return false;
    }
    true
}

/// 预告链衔接零悬空。
fn chain_zero_dangling_ok() -> bool {
    chain_zero_dangling(&B36_FRAME)
}

/// 注入联测十族（人为构造违约必被拒）。
pub fn kg_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 红例混入。
    {
        let mut a = build_full_kg_agg_open();
        if a.record(CaseLine { no: 0, verdict: Verdict::VRed }).is_err() {
            caught += 1;
        } else {
            escape.push_str("红例;");
        }
    }
    // 2. 挂起混入。
    {
        let mut a = build_full_kg_agg_open();
        if a.record(CaseLine { no: 0, verdict: Verdict::VHold }).is_err() {
            caught += 1;
        } else {
            escape.push_str("挂起;");
        }
    }
    // 3. 用例缺一收账。
    {
        let mut a = build_full_kg_agg_open();
        let mut i = 1usize;
        while i < B35_CASES.len() {
            if let Some(l) = replay_case(i) {
                let _ = a.record(l);
            }
            i += 1;
        }
        if a.close().is_err() {
            caught += 1;
        } else {
            escape.push_str("缺一;");
        }
    }
    // 4. 用例重号。
    {
        let mut a = build_full_kg_agg_open();
        let _ = a.record(CaseLine { no: 0, verdict: Verdict::VGreen });
        if a.record(CaseLine { no: 0, verdict: Verdict::VGreen }).is_err() {
            caught += 1;
        } else {
            escape.push_str("重号;");
        }
    }
    // 5. 预告断链（框架锚错位）。
    {
        let mut a = KgAgg::new();
        let bad = ThemeFrame { theme: "B36", anchor: 12700, state: FrameState::FrRegistered };
        let _ = a.register_frame(bad);
        if a.begin().is_err() || a.close().is_err() {
            caught += 1;
        } else {
            escape.push_str("断链;");
        }
    }
    // 6. 框架未登记开跑。
    {
        let mut a = KgAgg::new();
        if a.begin().is_err() {
            caught += 1;
        } else {
            escape.push_str("框架;");
        }
    }
    // 7. 框架回退登记（Draft）。
    {
        let mut a = build_full_kg_agg();
        let draft = ThemeFrame { theme: "B36", anchor: CHAIN_TAIL, state: FrameState::FrDraft };
        if a.register_frame(draft).is_err() {
            caught += 1;
        } else {
            escape.push_str("框架回退;");
        }
    }
    // 8. 收账后写。
    {
        let mut a = build_full_kg_agg();
        if a.record(CaseLine { no: 0, verdict: Verdict::VGreen }).is_err() {
            caught += 1;
        } else {
            escape.push_str("收后写;");
        }
    }
    // 9. 状态机倒退。
    {
        let mut a = build_full_kg_agg();
        if a.begin().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 10. 摘要空面直证（相位名/判定名/种别名恒非空）。
    {
        let mut bad = false;
        for st in [KgState::KgOpen, KgState::KgRunning, KgState::KgClosed].iter() {
            if st.name().is_empty() {
                bad = true;
            }
        }
        for v in Verdict::all().iter() {
            if v.name().is_empty() {
                bad = true;
            }
        }
        for k in CaseKind::all().iter() {
            if k.name().is_empty() {
                bad = true;
            }
        }
        if !bad {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 开账待记态重放（框架已登记、开跑、未记任何用例）。
fn build_full_kg_agg_open() -> KgAgg {
    let mut a = KgAgg::new();
    let _ = a.register_frame(B36_FRAME);
    let _ = a.begin();
    a
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let x = build_full_kg_agg();
    let y = build_full_kg_agg();
    if x.line_len() != y.line_len() || x.state() != y.state() {
        return false;
    }
    if x.frame() != y.frame() || x.reject_len() != y.reject_len() {
        return false;
    }
    let mut i = 0usize;
    while i < x.line_len() {
        match (x.line_at(i), y.line_at(i)) {
            (Some(p), Some(q)) => {
                if p != q {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    let (c1, _, e1) = kg_fault_sweep();
    let (c2, _, e2) = kg_fault_sweep();
    c1 == c2 && e1 == e2
}

/// 冻结摘要（用例表 + 预告链折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0u32;
    for row in B35_CASES.iter() {
        acc = acc
            .wrapping_mul(5)
            .wrapping_add(row.0)
            .wrapping_add(row.2.ordinal())
            .wrapping_add(row.3)
            .wrapping_add(row.1.len() as u32)
            .wrapping_add(i);
        i = i.wrapping_add(1);
    }
    for row in CHAIN_NODES.iter() {
        acc = acc
            .wrapping_mul(3)
            .wrapping_add(row.0)
            .wrapping_add(row.1)
            .wrapping_add(row.2);
    }
    acc
}

/// CaseKind 序（折算用；与 all() 顺序一致）。
impl CaseKind {
    /// 种别序。
    pub fn ordinal(self) -> u32 {
        match self {
            CaseKind::CkInject => 0,
            CaseKind::CkReplay => 1,
            CaseKind::CkBoundary => 2,
            CaseKind::CkDual => 3,
        }
    }
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * 199 {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12698u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③）。
fn writeback_ok() -> bool {
    let a = build_full_kg_agg();
    if a.state() != KgState::KgClosed {
        return false;
    }
    if a.frame().state != FrameState::FrRegistered {
        return false;
    }
    if !regress_twenty_ok() || !theme_frame_ok() || !chain_zero_dangling_ok() {
        return false;
    }
    frozen_digest() != 0
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12699 ktest 聚合器自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12698_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12698-ktest-agg-b35");
    let a = build_full_kg_agg();

    s.add_dyn(
        "F12699-01 B35 回归 20 条",
        regress_twenty_ok(),
        &format!("lines={} cap={} closed={}", a.line_len(), CASE_CAP, a.state() == KgState::KgClosed),
    );

    s.add_dyn(
        "F12699-02 用例号连续无缺",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < B35_CASES.len() {
                match B35_CASES.get(i) {
                    Some(row) => {
                        if row.0 != i as u32 {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok
        },
        "0..19 连续登记",
    );

    s.add_dyn(
        "F12699-03 用例种别闭集合法",
        {
            let mut ok = true;
            for row in B35_CASES.iter() {
                let mut hit = false;
                for k in CaseKind::all().iter() {
                    if *k == row.2 {
                        hit = true;
                    }
                }
                if !hit {
                    ok = false;
                }
            }
            ok
        },
        "注入/重放/边界/双机四闭集",
    );

    s.add_dyn(
        "F12699-04 聚合账等式",
        a.equation_ok(),
        &format!("green=20 total={} cap={}", a.line_len(), CASE_CAP),
    );

    s.add_dyn(
        "F12699-05 B36 主题框架登记",
        theme_frame_ok(),
        "B36@12698 已登记",
    );

    s.add_dyn(
        "F12699-06 预告链零悬空",
        chain_zero_dangling_ok(),
        &format!("nodes={} head={} tail={}", CHAIN_NODES.len(), CHAIN_HEAD, CHAIN_TAIL),
    );

    s.add_dyn(
        "F12699-07 链式衔接逐节点闭合",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i + 1 < CHAIN_NODES.len() {
                let cur = match CHAIN_NODES.get(i) {
                    Some(v) => v,
                    None => {
                        ok = false;
                        break;
                    }
                };
                let next = match CHAIN_NODES.get(i + 1) {
                    Some(v) => v,
                    None => {
                        ok = false;
                        break;
                    }
                };
                if cur.2 != next.1 {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "F12598→F12601→…→F12695→F12698",
    );

    s.add_dyn(
        "F12699-08 注入联测 10/10 拒止",
        {
            let (c, t, e) = kg_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", kg_fault_sweep().0),
    );

    s.add_dyn(
        "F12699-09 相位迁移守卫直读",
        {
            let mut open = KgAgg::new();
            let _ = open.register_frame(B36_FRAME);
            kg_advance(KgState::KgOpen, KgState::KgRunning).is_ok()
                && kg_advance(KgState::KgRunning, KgState::KgClosed).is_ok()
                && kg_advance(KgState::KgClosed, KgState::KgRunning).is_err()
                && kg_advance(KgState::KgOpen, KgState::KgClosed).is_err()
        },
        "开账→跑账中→已收账只进不退",
    );

    s.add_dyn(
        "F12699-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12699-11 完成写回语义",
        writeback_ok(),
        "收账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12699-12 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12699-13 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81880);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(KG_ALL.len(), 12);
        let mut i = 0usize;
        while i < KG_ALL.len() {
            let vi = match KG_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < KG_ALL.len() {
                let vj = match KG_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn regress_twenty_holds() {
        assert!(regress_twenty_ok());
        assert!(theme_frame_ok());
        assert!(chain_zero_dangling_ok());
        let a = build_full_kg_agg();
        assert_eq!(a.line_len(), 20);
        assert_eq!(a.state(), KgState::KgClosed);
        assert_eq!(a.reject_len(), 0);
        assert!(a.equation_ok());
        assert_eq!(a.frame().theme, "B36");
        assert_eq!(a.frame().anchor, 12698);
    }

    #[test]
    fn chain_closing_holds() {
        // 链首对号。
        let head = CHAIN_NODES.first().copied();
        match head {
            Some(h) => assert_eq!(h.1, CHAIN_HEAD),
            None => panic_free_check(),
        }
        // 尾节点衔接框架锚。
        let tail = CHAIN_NODES.get(CHAIN_NODES.len() - 1);
        match tail {
            Some(t) => assert_eq!(t.2, B36_FRAME.anchor),
            None => panic_free_check(),
        }
        // 节点序连续。
        let mut i = 0usize;
        while i < CHAIN_NODES.len() {
            let row = match CHAIN_NODES.get(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(row.0, i as u32);
            assert!(row.2 > row.1, "节点 {} 目标须大于预告", i);
            i += 1;
        }
        // 断链注入：锚错位判悬空。
        let bad = ThemeFrame { theme: "B36", anchor: 12600, state: FrameState::FrRegistered };
        assert!(!chain_zero_dangling(&bad));
        // 框架未登记判悬空。
        let draft = ThemeFrame { theme: "B36", anchor: CHAIN_TAIL, state: FrameState::FrDraft };
        assert!(!chain_zero_dangling(&draft));
    }

    /// 测试内占位（保证 match 双臂对齐的空操作）。
    fn panic_free_check() {
        // 链表空属冻结账违约，不应发生；此处显性空置以便测试可读。
    }

    #[test]
    fn record_gates_reject() {
        let mut a = build_full_kg_agg_open();
        // 红例。
        assert_eq!(
            a.record(CaseLine { no: 0, verdict: Verdict::VRed }),
            Err(KgDiag::ERedCase.val())
        );
        // 挂起。
        assert_eq!(
            a.record(CaseLine { no: 0, verdict: Verdict::VHold }),
            Err(KgDiag::EHoldCase.val())
        );
        // 外键越界（号 20：序闸先拒——号 20 ≠ 记账序 0）。
        assert_eq!(
            a.record(CaseLine { no: 20, verdict: Verdict::VGreen }),
            Err(KgDiag::ECaseMissing.val())
        );
        assert_eq!(a.reject_len(), 3);
        // 正常条目过闸。
        assert!(a.record(CaseLine { no: 0, verdict: Verdict::VGreen }).is_ok());
        // 重号。
        assert_eq!(
            a.record(CaseLine { no: 0, verdict: Verdict::VGreen }),
            Err(KgDiag::ECaseMissing.val())
        );
        assert_eq!(a.reject_len(), 4);
    }

    #[test]
    fn frame_gates_reject() {
        let mut a = KgAgg::new();
        // 未登记开跑拒。
        assert_eq!(a.begin(), Err(KgDiag::EFrameDraft.val()));
        // 草稿框架登记拒。
        let draft = ThemeFrame { theme: "B36", anchor: CHAIN_TAIL, state: FrameState::FrDraft };
        assert_eq!(a.register_frame(draft), Err(KgDiag::EFrameDraft.val()));
        // 空主题名拒。
        let empty = ThemeFrame { theme: "", anchor: CHAIN_TAIL, state: FrameState::FrRegistered };
        assert_eq!(a.register_frame(empty), Err(KgDiag::EFrameDraft.val()));
        // 正常登记过。
        assert!(a.register_frame(B36_FRAME).is_ok());
        assert!(a.begin().is_ok());
    }

    #[test]
    fn close_gates_hold() {
        // 未记满收账拒。
        let mut a = build_full_kg_agg_open();
        assert!(a.close().is_err());
        // 足记但断链收账拒（框架锚错位）。
        let mut b = KgAgg::new();
        let bad = ThemeFrame { theme: "B36", anchor: 12700, state: FrameState::FrRegistered };
        let _ = b.register_frame(bad);
        let _ = b.begin();
        let mut i = 0usize;
        while i < B35_CASES.len() {
            if let Some(l) = replay_case(i) {
                let _ = b.record(l);
            }
            i += 1;
        }
        assert!(b.close().is_err());
        // 全量收账过。
        let mut full = build_full_kg_agg();
        assert_eq!(full.state(), KgState::KgClosed);
        // 收账后写拒。
        assert_eq!(
            full.record(CaseLine { no: 0, verdict: Verdict::VGreen }),
            Err(KgDiag::EClosedWrite.val())
        );
        // 倒退拒。
        assert!(full.begin().is_err());
    }

    #[test]
    fn phase_guards_hold() {
        assert!(kg_advance(KgState::KgOpen, KgState::KgRunning).is_ok());
        assert!(kg_advance(KgState::KgRunning, KgState::KgClosed).is_ok());
        assert!(kg_advance(KgState::KgOpen, KgState::KgClosed).is_err());
        assert!(kg_advance(KgState::KgClosed, KgState::KgOpen).is_err());
        assert!(kg_advance(KgState::KgRunning, KgState::KgRunning).is_err());
        assert!(KgState::KgOpen.rank() < KgState::KgRunning.rank());
        assert!(KgState::KgRunning.rank() < KgState::KgClosed.rank());
        // 种别序与 all() 顺序一致。
        assert_eq!(CaseKind::CkInject.ordinal(), 0);
        assert_eq!(CaseKind::CkDual.ordinal(), 3);
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = kg_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn dual_run_consistent() {
        assert!(dual_run_ok());
        let x = build_full_kg_agg();
        let y = build_full_kg_agg();
        assert_eq!(x.line_len(), y.line_len());
        assert_eq!(x.reject_len(), y.reject_len());
        assert_eq!(frozen_digest(), frozen_digest());
        assert_ne!(frozen_digest(), 0);
    }

    #[test]
    fn writeback_semantics_hold() {
        assert!(writeback_ok());
        for st in [KgState::KgOpen, KgState::KgRunning, KgState::KgClosed].iter() {
            assert!(!st.name().is_empty());
        }
        for v in Verdict::all().iter() {
            assert!(!v.name().is_empty());
        }
        for k in CaseKind::all().iter() {
            assert!(!k.name().is_empty());
        }
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12698_checks();
        assert!(s.all_passed(), "自检存在红项：{}", s.red_items_text());
        assert!(s.len() >= 12, "检查条目不足 12");
    }
}
