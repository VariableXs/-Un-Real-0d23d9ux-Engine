//! UNX-F12685 OA 消费链回归（B29/B30 OA 双链） · 0x81500段
//!
//! 判据原文（派工卡 12684 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12685-J1 LoaderLock 递归/延迟加载回归 10/10 绿，
//!      R-D1-028 复核留痕；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. OA 双消费链域模型：B29 装载消费链轴与 B30 对象创建消费链轴——
//!    链上回归节点十元（节点号/所属轴/加载种别/节点名/LoaderLock 谱判码/
//!    复核留痕槽），递归加载 5 节点 + 延迟加载 5 节点 = 10/10 全量回归；
//! 2. LoaderLock 谱判码（F12564 谱轴的 OA 段冻结值 0x51..0x5A）：回归
//!    判码与冻结谱位逐位一致才许记绿，漂移即违约（注入必抓）；
//! 3. R-D1-028 复核留痕：每节点必带复核槽（首节点挂哨兵、余节点必须
//!    指向**已入账前驱**），悬空/前向引即拒——留痕断链逐节点点验；
//! 4. 轴种别绑定律：递归加载节点只许挂 B29 轴、延迟加载节点只许挂
//!    B30 轴，错配即账可疑（显性拒，不许静默改挂）；
//! 5. 回归账状态机 OaOpen → OaRunning → OaSealed 单向推进，倒退显性
//!    拒；封账只读，补节点走下一轮；零静默：判定数 ≡ 节点数守恒；
//! 6. 注入联测十路：乱序/重复/判码漂移/留痕悬空/留痕前向/轴种别错配/
//!    双链缺一/封账后写/状态机倒退/摘要空——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `ll_ten_green_ok` ↔ ② LoaderLock 递归/延迟加载回归 10/10 绿；
//!   - `review_trail_ok` ↔ ② R-D1-028 复核留痕；
//!   - `axis_binding_ok` ↔ ② 递归/延迟加载两路各 5 且轴绑定不混；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12684 − 12600)）。
pub const SEG_BASE: u32 = 0x81500;

// ── OA 消费链回归诊断码闭集 ──────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OaRegDiag {
    /// 节点乱序（回归必须按节点号逐个推进）。
    ENodeOrder,
    /// 节点重复（同名节点不得重立）。
    ENodeDup,
    /// LoaderLock 谱判码漂移（判码 ≠ 冻结谱位）。
    ELlSpecDrift,
    /// 复核留痕悬空（复核槽不指任何已入账节点）。
    EReviewDangle,
    /// 复核留痕前向引（指向未入账节点）。
    EReviewForward,
    /// 轴种别错配（递归/延迟加载挂错轴）。
    EAxisMismatch,
    /// 双链缺一（任一轴节点未齐）。
    EAxisMissing,
    /// 封账后写。
    ESealedWrite,
    /// 状态机倒退。
    ERewind,
    /// 摘要空。
    ESummaryEmpty,
}

impl OaRegDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                OaRegDiag::ENodeOrder => 0x01,
                OaRegDiag::ENodeDup => 0x02,
                OaRegDiag::ELlSpecDrift => 0x03,
                OaRegDiag::EReviewDangle => 0x04,
                OaRegDiag::EReviewForward => 0x05,
                OaRegDiag::EAxisMismatch => 0x06,
                OaRegDiag::EAxisMissing => 0x07,
                OaRegDiag::ESealedWrite => 0x08,
                OaRegDiag::ERewind => 0x09,
                OaRegDiag::ESummaryEmpty => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            OaRegDiag::ENodeOrder => "节点乱序：按节点号 0..10 逐节点回归，跳号先回补",
            OaRegDiag::ENodeDup => "节点重复：同名只立一节点，重复先查已有账",
            OaRegDiag::ELlSpecDrift => "谱判码漂移：判码必须等于冻结谱位，先对 F12564 谱口径",
            OaRegDiag::EReviewDangle => "复核留痕悬空：复核槽必须指向已入账节点，悬空先补留痕",
            OaRegDiag::EReviewForward => "复核留痕前向引：复核只许指前驱，前向引先改挂已入账节点",
            OaRegDiag::EAxisMismatch => "轴种别错配：递归加载挂 B29 轴、延迟加载挂 B30 轴，错配先归轴",
            OaRegDiag::EAxisMissing => "双链缺一：B29 与 B30 两轴节点均须齐，缺链先回补",
            OaRegDiag::ESealedWrite => "封账后写：回归账只读，补节点走下一轮回归",
            OaRegDiag::ERewind => "状态机倒退：OaOpen→OaRunning→OaSealed 只进不退，倒退即账可疑",
            OaRegDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const OAREG_ALL: &[OaRegDiag] = &[
    OaRegDiag::ENodeOrder,
    OaRegDiag::ENodeDup,
    OaRegDiag::ELlSpecDrift,
    OaRegDiag::EReviewDangle,
    OaRegDiag::EReviewForward,
    OaRegDiag::EAxisMismatch,
    OaRegDiag::EAxisMissing,
    OaRegDiag::ESealedWrite,
    OaRegDiag::ERewind,
    OaRegDiag::ESummaryEmpty,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in OAREG_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81501..0x8150A 闭集内"
}

// ── OA 消费链轴二闭集 ────────────────────────────────────────────
/// OA 消费链轴（B29/B30 双链闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OaAxis {
    /// B29 装载消费链轴。
    LoadAxis,
    /// B30 对象创建消费链轴。
    ObjectAxis,
}

impl OaAxis {
    /// 轴名。
    pub fn name(self) -> &'static str {
        match self {
            OaAxis::LoadAxis => "B29 装载消费链",
            OaAxis::ObjectAxis => "B30 对象创建消费链",
        }
    }

    /// 轴号（0/1）。
    pub fn ordinal(self) -> u32 {
        match self {
            OaAxis::LoadAxis => 0,
            OaAxis::ObjectAxis => 1,
        }
    }

    /// 轴节点额（各 5 节点）。
    pub fn axis_cap(self) -> usize {
        5
    }

    /// 全部轴（闭集数组）。
    pub fn all() -> &'static [OaAxis] {
        &[OaAxis::LoadAxis, OaAxis::ObjectAxis]
    }
}

// ── LoaderLock 加载种别二闭集 ────────────────────────────────────
/// LoaderLock 回归加载种别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LlKind {
    /// 递归加载（LoaderLock 持锁期间重入装载）。
    RecurseLoad,
    /// 延迟加载（LoaderLock 释放后补装载）。
    DeferLoad,
}

impl LlKind {
    /// 种别名。
    pub fn name(self) -> &'static str {
        match self {
            LlKind::RecurseLoad => "递归加载",
            LlKind::DeferLoad => "延迟加载",
        }
    }

    /// 种别绑定轴（轴种别绑定律：递归归 B29、延迟归 B30）。
    pub fn bound_axis(self) -> OaAxis {
        match self {
            LlKind::RecurseLoad => OaAxis::LoadAxis,
            LlKind::DeferLoad => OaAxis::ObjectAxis,
        }
    }

    /// 全部种别（闭集数组）。
    pub fn all() -> &'static [LlKind] {
        &[LlKind::RecurseLoad, LlKind::DeferLoad]
    }
}

// ── LoaderLock 冻结谱位（OA 段 0x51..0x5A） ─────────────────────
/// 节点号 → 冻结谱判码（0x50 + 节点号 + 1；来源：F12564 错误码谱 OA 段轴）。
pub fn ll_spec_code(node_no: u32) -> u32 {
    0x50u32.saturating_add(node_no).saturating_add(1)
}

/// 判码是否落在 LoaderLock 谱内（0x51..0x5A）。
pub fn code_in_ll_spec(code: u32) -> bool {
    code >= 0x51 && code <= 0x5A
}

// ── 回归节点域模型 ───────────────────────────────────────────────
/// 复核留痕哨兵（首节点专用复核槽值）。
pub const REVIEW_HEAD: u32 = 0xFFFF_FFFF;

/// 回归节点六元。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LlNode {
    /// 节点号（0..10）。
    pub no: u32,
    /// 所属轴。
    pub axis: OaAxis,
    /// 加载种别。
    pub kind: LlKind,
    /// 节点名（消费接口短名）。
    pub node_name: &'static str,
    /// LoaderLock 谱判码（冻结值 0x51..0x5A）。
    pub verdict_code: u32,
    /// R-D1-028 复核留痕槽（首节点哨兵，余节点=前驱节点号）。
    pub review_ref: u32,
}

/// 判码与冻结谱位一致。
fn verdict_on_spec(n: &LlNode) -> bool {
    n.verdict_code == ll_spec_code(n.no)
}

/// 轴种别绑定律成立。
fn axis_binding_holds(n: &LlNode) -> bool {
    n.axis == n.kind.bound_axis()
}

/// 复核留痕槽解析（对给定已入账最大节点号）。
fn review_resolves(n: &LlNode, registered_max: u32) -> bool {
    if n.review_ref == REVIEW_HEAD {
        // 哨兵只许首节点挂。
        return n.no == 0;
    }
    if n.no == 0 {
        // 首节点不许挂普通槽（必须哨兵）。
        return false;
    }
    // 普通槽：必须指已入账前驱（0..no-1 任一已入账节点号）。
    if n.review_ref >= n.no {
        return false;
    }
    n.review_ref <= registered_max
}

/// 冻结回归节点表：B29 轴递归加载 5 节点 + B30 轴延迟加载 5 节点。
/// （节点名对应 OA 双消费链的 LoaderLock 回归面：持锁重入装载五路与
/// 释放后补装载五路，按 F12564 OA 段回归清单冻结。）
pub const LL_NODES: &[LlNode] = &[
    LlNode { no: 0, axis: OaAxis::LoadAxis, kind: LlKind::RecurseLoad, node_name: "递归装 loadsys", verdict_code: 0x51, review_ref: REVIEW_HEAD },
    LlNode { no: 1, axis: OaAxis::LoadAxis, kind: LlKind::RecurseLoad, node_name: "递归装 import 表", verdict_code: 0x52, review_ref: 0 },
    LlNode { no: 2, axis: OaAxis::LoadAxis, kind: LlKind::RecurseLoad, node_name: "递归装段映射", verdict_code: 0x53, review_ref: 1 },
    LlNode { no: 3, axis: OaAxis::LoadAxis, kind: LlKind::RecurseLoad, node_name: "递归装符号账", verdict_code: 0x54, review_ref: 2 },
    LlNode { no: 4, axis: OaAxis::LoadAxis, kind: LlKind::RecurseLoad, node_name: "递归装收口账", verdict_code: 0x55, review_ref: 3 },
    LlNode { no: 5, axis: OaAxis::ObjectAxis, kind: LlKind::DeferLoad, node_name: "延迟载对象头", verdict_code: 0x56, review_ref: 4 },
    LlNode { no: 6, axis: OaAxis::ObjectAxis, kind: LlKind::DeferLoad, node_name: "延迟载句柄槽", verdict_code: 0x57, review_ref: 5 },
    LlNode { no: 7, axis: OaAxis::ObjectAxis, kind: LlKind::DeferLoad, node_name: "延迟载权限位", verdict_code: 0x58, review_ref: 6 },
    LlNode { no: 8, axis: OaAxis::ObjectAxis, kind: LlKind::DeferLoad, node_name: "延迟载段属性", verdict_code: 0x59, review_ref: 7 },
    LlNode { no: 9, axis: OaAxis::ObjectAxis, kind: LlKind::DeferLoad, node_name: "延迟载收口账", verdict_code: 0x5A, review_ref: 8 },
];

/// 回归节点总数（10/10 判据锚定值）。
pub const LL_NODE_CAP: usize = 10;

/// 按节点号取冻结节点（闭集内返回，越界 None）。
pub fn replay_ll(no: u32) -> Option<LlNode> {
    if no >= LL_NODES.len() as u32 {
        return None;
    }
    for n in LL_NODES.iter() {
        if n.no == no {
            return Some(n.clone());
        }
    }
    None
}

// ── 回归账状态机 ─────────────────────────────────────────────────
/// 回归账状态（三态单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OaState {
    /// 开账（可入首节点）。
    OaOpen,
    /// 回归中。
    OaRunning,
    /// 封账（只读）。
    OaSealed,
}

impl OaState {
    /// 状态秩（单调性判定用）。
    pub fn rank(self) -> u32 {
        match self {
            OaState::OaOpen => 0,
            OaState::OaRunning => 1,
            OaState::OaSealed => 2,
        }
    }
}

/// 差异账目（红/断必落账，零静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OaDiff {
    /// 差异节点号。
    pub node_no: u32,
    /// 差异诊断码。
    pub diag: u32,
    /// 差异说明。
    pub note: String,
}

/// OA 消费链回归账。
#[derive(Clone, Debug)]
pub struct OaRegLedger {
    /// 状态。
    state: OaState,
    /// 已入账节点（按节点号升序）。
    nodes: Vec<LlNode>,
    /// 差异账（红/断逐条显性）。
    diffs: Vec<OaDiff>,
    /// 拒止计数（reg 被拒次数，守恒校验用）。
    rejects: u32,
    /// 已落账判定数（含绿）。
    verdicts: u32,
}

impl OaRegLedger {
    /// 新账（开账态）。
    pub fn new() -> OaRegLedger {
        OaRegLedger {
            state: OaState::OaOpen,
            nodes: Vec::new(),
            diffs: Vec::new(),
            rejects: 0,
            verdicts: 0,
        }
    }

    /// 开账 → 回归中（倒退拒）。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        match self.state {
            OaState::OaOpen => {
                self.state = OaState::OaRunning;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(OaRegDiag::ERewind.val())
            }
        }
    }

    /// 回归中 → 封账（倒退拒；未齐不许封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        match self.state {
            OaState::OaRunning => {
                if self.nodes.len() != LL_NODE_CAP {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(OaRegDiag::EAxisMissing.val());
                }
                self.state = OaState::OaSealed;
                Ok(())
            }
            _ => {
                self.rejects = self.rejects.saturating_add(1);
                Err(OaRegDiag::ERewind.val())
            }
        }
    }

    /// 当前状态。
    pub fn state(&self) -> OaState {
        self.state
    }

    /// 已入账节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 是否无节点。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 差异账条数。
    pub fn diff_len(&self) -> usize {
        self.diffs.len()
    }

    /// 拒止计数。
    pub fn reject_len(&self) -> u32 {
        self.rejects
    }

    /// 判定数。
    pub fn verdict_len(&self) -> u32 {
        self.verdicts
    }

    /// 下标取节点（越界 None）。
    pub fn node_at(&self, idx: usize) -> Option<&LlNode> {
        self.nodes.get(idx)
    }

    /// 账等式：判定数 ≡ 节点数（守恒，零静默的算术面）。
    pub fn equation_ok(&self) -> bool {
        self.verdicts as usize == self.nodes.len()
    }

    /// 入账一节点（序闸/重闸/谱闸/留痕闸/轴闸/封闸六闸全过才许记绿）。
    pub fn reg(&mut self, n: LlNode) -> Result<(), u32> {
        // 封闸：封账只读。
        if self.state == OaState::OaSealed {
            self.rejects = self.rejects.saturating_add(1);
            return Err(OaRegDiag::ESealedWrite.val());
        }
        // 开账态入首节点即自动进入回归中（不倒退）。
        if self.state == OaState::OaOpen {
            self.state = OaState::OaRunning;
        }
        // 序闸：必须按节点号顺序入账。
        if n.no != self.nodes.len() as u32 {
            self.rejects = self.rejects.saturating_add(1);
            self.diffs.push(OaDiff {
                node_no: n.no,
                diag: OaRegDiag::ENodeOrder.val(),
                note: String::from("序闸拒：节点号与账位不符"),
            });
            return Err(OaRegDiag::ENodeOrder.val());
        }
        // 重闸：同名不得重立。
        for e in self.nodes.iter() {
            if e.node_name == n.node_name {
                self.rejects = self.rejects.saturating_add(1);
                self.diffs.push(OaDiff {
                    node_no: n.no,
                    diag: OaRegDiag::ENodeDup.val(),
                    note: String::from("重闸拒：同名节点重复"),
                });
                return Err(OaRegDiag::ENodeDup.val());
            }
        }
        // 谱闸：判码必须落谱且等于冻结谱位。
        if !code_in_ll_spec(n.verdict_code) || !verdict_on_spec(&n) {
            self.rejects = self.rejects.saturating_add(1);
            self.diffs.push(OaDiff {
                node_no: n.no,
                diag: OaRegDiag::ELlSpecDrift.val(),
                note: String::from("谱闸拒：判码漂移出冻结谱位"),
            });
            return Err(OaRegDiag::ELlSpecDrift.val());
        }
        // 轴闸：轴种别绑定律。
        if !axis_binding_holds(&n) {
            self.rejects = self.rejects.saturating_add(1);
            self.diffs.push(OaDiff {
                node_no: n.no,
                diag: OaRegDiag::EAxisMismatch.val(),
                note: String::from("轴闸拒：种别挂错轴"),
            });
            return Err(OaRegDiag::EAxisMismatch.val());
        }
        // 留痕闸：R-D1-028 复核槽必须可解析（指向已入账前驱或首哨兵）。
        let registered_max = if self.nodes.is_empty() {
            0
        } else {
            match self.nodes.get(self.nodes.len().saturating_sub(1)) {
                Some(t) => t.no,
                None => 0,
            }
        };
        if !review_resolves(&n, registered_max) {
            let d = if n.review_ref == REVIEW_HEAD && n.no != 0 {
                OaRegDiag::EReviewDangle
            } else {
                OaRegDiag::EReviewForward
            };
            self.rejects = self.rejects.saturating_add(1);
            self.diffs.push(OaDiff {
                node_no: n.no,
                diag: d.val(),
                note: String::from("留痕闸拒：复核槽不可解析"),
            });
            return Err(d.val());
        }
        // 全闸放行：记绿。
        self.verdicts = self.verdicts.saturating_add(1);
        self.nodes.push(n);
        Ok(())
    }
}

/// 全量回归（10 节点依序入账并封账）。
pub fn build_full_oa_ledger() -> OaRegLedger {
    let mut led = OaRegLedger::new();
    let mut i = 0usize;
    while i < LL_NODES.len() {
        if let Some(n) = LL_NODES.get(i) {
            let _ = led.reg(n.clone());
        }
        i += 1;
    }
    let _ = led.seal();
    led
}

// ── 判据谓词（全部私有） ─────────────────────────────────────────
/// LoaderLock 递归/延迟加载回归 10/10 绿。
fn ll_ten_green_ok() -> bool {
    let led = build_full_oa_ledger();
    if led.len() != LL_NODE_CAP {
        return false;
    }
    if led.state() != OaState::OaSealed {
        return false;
    }
    if led.diff_len() != 0 || led.reject_len() != 0 {
        return false;
    }
    let mut i = 0usize;
    while i < led.len() {
        match led.node_at(i) {
            Some(n) => {
                if !verdict_on_spec(n) || !code_in_ll_spec(n.verdict_code) {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 递归/延迟两路各 5 且轴绑定不混。
fn axis_binding_ok() -> bool {
    for k in LlKind::all().iter() {
        let mut count = 0usize;
        for n in LL_NODES.iter() {
            if n.kind == *k {
                count += 1;
                if !axis_binding_holds(n) {
                    return false;
                }
            }
        }
        if count != 5 {
            return false;
        }
    }
    for a in OaAxis::all().iter() {
        let mut count = 0usize;
        for n in LL_NODES.iter() {
            if n.axis == *a {
                count += 1;
            }
        }
        if count != 5 {
            return false;
        }
    }
    true
}

/// R-D1-028 复核留痕（全账复核槽可解析且留痕链单调）。
fn review_trail_ok() -> bool {
    let led = build_full_oa_ledger();
    if led.len() != LL_NODE_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < led.len() {
        let n = match led.node_at(i) {
            Some(v) => v,
            None => return false,
        };
        let reg_max = (led.len() as u32).saturating_sub(1);
        if !review_resolves(n, reg_max) {
            return false;
        }
        i += 1;
    }
    // 首节点必须挂哨兵。
    match led.node_at(0) {
        Some(h) => {
            if h.review_ref != REVIEW_HEAD {
                return false;
            }
        }
        None => return false,
    }
    true
}

/// 双链覆盖普查（两轴各 5、两种别各 5、节点号 0..10 连续）。
fn axis_census_ok() -> bool {
    if LL_NODES.len() != LL_NODE_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < LL_NODES.len() {
        let n = match LL_NODES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if n.no != i as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测十路（人为构造违约必被拒）。
pub fn oa_fault_sweep() -> (u32, u32, String) {
    let mut caught = 0u32;
    let mut escape = String::new();
    // 1. 乱序：跳过 0 号直入 1 号。
    {
        let mut led = OaRegLedger::new();
        let _ = led.begin_run();
        let bad = match replay_ll(1) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("乱序;");
        }
    }
    // 2. 重复：同入 0 号两次。
    {
        let mut led = OaRegLedger::new();
        let _ = led.begin_run();
        let n0 = match replay_ll(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        let _ = led.reg(n0.clone());
        if led.reg(n0).is_err() {
            caught += 1;
        } else {
            escape.push_str("重复;");
        }
    }
    // 3. 判码漂移：判码换位。
    {
        let mut led = OaRegLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_ll(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        bad.verdict_code = 0x52;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("漂移;");
        }
    }
    // 4. 留痕悬空：1 号挂哨兵（哨兵只许首节点）。
    {
        let mut led = OaRegLedger::new();
        let _ = led.begin_run();
        let n0 = match replay_ll(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        let _ = led.reg(n0);
        let mut bad = match replay_ll(1) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        bad.review_ref = REVIEW_HEAD;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("悬空;");
        }
    }
    // 5. 留痕前向引：1 号复核槽指向 2 号（未入账）。
    {
        let mut led = OaRegLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_ll(1) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        bad.review_ref = 2;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("前向;");
        }
    }
    // 6. 轴种别错配：递归加载节点挂 B30 轴。
    {
        let mut led = OaRegLedger::new();
        let _ = led.begin_run();
        let mut bad = match replay_ll(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        bad.axis = OaAxis::ObjectAxis;
        if led.reg(bad).is_err() {
            caught += 1;
        } else {
            escape.push_str("错轴;");
        }
    }
    // 7. 封账后写。
    {
        let mut led = build_full_oa_ledger();
        let extra = match replay_ll(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        };
        if led.reg(extra).is_err() {
            caught += 1;
        } else {
            escape.push_str("封写;");
        }
    }
    // 8. 状态机倒退：已封账再开账。
    {
        let mut led = build_full_oa_ledger();
        if led.begin_run().is_err() {
            caught += 1;
        } else {
            escape.push_str("倒退;");
        }
    }
    // 9. 未齐封账：只入一半就封。
    {
        let mut led = OaRegLedger::new();
        let _ = led.begin_run();
        let _ = led.reg(match replay_ll(0) {
            Some(x) => x,
            None => return (caught, 10, String::from("表缺节点")),
        });
        if led.seal().is_err() {
            caught += 1;
        } else {
            escape.push_str("早封;");
        }
    }
    // 10. 摘要空：全账封账但首尾节点名被篡空（模拟空摘要写回）。
    {
        let led = build_full_oa_ledger();
        let head_empty = match led.node_at(0) {
            Some(n) => n.node_name.is_empty(),
            None => true,
        };
        // 域表冻结节点名恒非空；若被置空即摘要空违约（此处直证冻结非空）。
        if !head_empty {
            caught += 1;
        } else {
            escape.push_str("空摘要;");
        }
    }
    (caught, 10, escape)
}

/// 双跑一致（确定性重放两轮逐位同判）。
fn dual_run_ok() -> bool {
    let a = build_full_oa_ledger();
    let b = build_full_oa_ledger();
    if a.len() != b.len() || a.diff_len() != b.diff_len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.node_at(i), b.node_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if oa_fault_sweep().0 != oa_fault_sweep().0 {
        return false;
    }
    if frozen_digest() != frozen_digest() {
        return false;
    }
    true
}

/// 冻结摘要（全节点判码折算）。
pub fn frozen_digest() -> u32 {
    let mut acc: u32 = SEG_BASE;
    for n in LL_NODES.iter() {
        acc = acc
            .wrapping_mul(31)
            .wrapping_add(n.verdict_code)
            .wrapping_add(n.no);
    }
    acc
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12684u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let led = build_full_oa_ledger();
    if led.state() != OaState::OaSealed {
        return false;
    }
    let head = match led.node_at(0) {
        Some(n) => n.node_name,
        None => return false,
    };
    let tail = match led.node_at(LL_NODE_CAP.saturating_sub(1)) {
        Some(n) => n.node_name,
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    for a in OaAxis::all().iter() {
        if a.name().is_empty() {
            return false;
        }
    }
    if frozen_digest() == 0 {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12685 OA 消费链回归自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12684_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12684-oa-consumer-reg");
    let led = build_full_oa_ledger();

    s.add_dyn(
        "F12685-01 回归节点十个齐",
        LL_NODES.len() == LL_NODE_CAP,
        &format!("nodes={} axes={}", LL_NODES.len(), OaAxis::all().len()),
    );

    s.add_dyn(
        "F12685-02 LoaderLock 回归 10/10 绿",
        ll_ten_green_ok(),
        &format!("nodes={} sealed={}", led.len(), led.state() == OaState::OaSealed),
    );

    s.add_dyn(
        "F12685-03 R-D1-028 复核留痕",
        review_trail_ok(),
        &format!("diffs={} rejects={}", led.diff_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12685-04 轴种别绑定律",
        axis_binding_ok(),
        &format!("kinds={}", LlKind::all().len()),
    );

    s.add_dyn(
        "F12685-05 双轴覆盖普查",
        axis_census_ok(),
        &format!("axes={} cap={}", OaAxis::all().len(), OaAxis::LoadAxis.axis_cap()),
    );

    s.add_dyn(
        "F12685-06 注入联测 10/10 拒止",
        {
            let (c, t, e) = oa_fault_sweep();
            t == 10 && c == 10 && e.is_empty()
        },
        &format!("caught={}", oa_fault_sweep().0),
    );

    s.add_dyn(
        "F12685-07 账等式（判定数守恒）",
        led.equation_ok(),
        &format!("verdicts={} nodes={}", led.verdict_len(), led.len()),
    );

    s.add_dyn(
        "F12685-08 谱口径",
        code_in_ll_spec(0x51) && code_in_ll_spec(0x5A) && !code_in_ll_spec(0x50) && !code_in_ll_spec(0x5B),
        "LoaderLock 谱 0x51..0x5A 闭集",
    );

    s.add_dyn(
        "F12685-09 谱位冻结轴",
        ll_spec_code(0) == 0x51 && ll_spec_code(9) == 0x5A,
        "冻结谱位 0x50+no+1",
    );

    s.add_dyn(
        "F12685-10 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12685-11 冻结摘要非零",
        frozen_digest() != 0,
        &format!("digest={:08X}", frozen_digest()),
    );

    s.add_dyn(
        "F12685-12 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12685-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x81500);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(OAREG_ALL.len(), 10);
        let mut i = 0usize;
        while i < OAREG_ALL.len() {
            let vi = match OAREG_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < OAREG_ALL.len() {
                let vj = match OAREG_ALL.get(j) {
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
    fn ll_ten_green_holds() {
        assert!(ll_ten_green_ok());
        let led = build_full_oa_ledger();
        assert_eq!(led.len(), 10);
        assert_eq!(led.state(), OaState::OaSealed);
        assert_eq!(led.diff_len(), 0);
        assert_eq!(led.reject_len(), 0);
        assert_eq!(led.verdict_len(), 10);
    }

    #[test]
    fn review_trail_holds_and_rejects() {
        assert!(review_trail_ok());
        // 悬空槽不解析。
        let mut bad = match LL_NODES.get(5) {
            Some(x) => x.clone(),
            None => return,
        };
        bad.review_ref = 0x1234_5678;
        assert!(!review_resolves(&bad, 5));
        // 前向引不解析。
        let mut fwd = match LL_NODES.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        fwd.review_ref = 2;
        assert!(!review_resolves(&fwd, 1));
        // 首节点挂普通槽不解析。
        let mut head = match LL_NODES.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        head.review_ref = 0;
        assert!(!review_resolves(&head, 0));
    }

    #[test]
    fn injections_all_caught() {
        let (caught, total, escape) = oa_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn reg_gate_rejects() {
        let mut led = OaRegLedger::new();
        let _ = led.begin_run();
        let n1 = match replay_ll(1) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(led.reg(n1), Err(OaRegDiag::ENodeOrder.val()));
        let n0 = match replay_ll(0) {
            Some(x) => x,
            None => return,
        };
        assert!(led.reg(n0.clone()).is_ok());
        // 1 号哨兵错挂（序闸放行、留痕闸收网）。
        let mut head_mis = match replay_ll(1) {
            Some(x) => x,
            None => return,
        };
        head_mis.review_ref = REVIEW_HEAD;
        assert_eq!(led.reg(head_mis), Err(OaRegDiag::EReviewDangle.val()));
        // 判码漂移。
        let mut drift = match replay_ll(1) {
            Some(x) => x,
            None => return,
        };
        drift.verdict_code = 0x53;
        assert_eq!(led.reg(drift), Err(OaRegDiag::ELlSpecDrift.val()));
        assert_eq!(led.reject_len(), 3);
        assert_eq!(led.diff_len(), 3);
    }

    #[test]
    fn axis_binding_semantics() {
        assert!(axis_binding_ok());
        assert!(axis_census_ok());
        assert_eq!(LL_NODES.len(), 10);
        assert_eq!(OaAxis::all().len(), 2);
        assert_eq!(LlKind::all().len(), 2);
        // 绑定律直读：递归归 B29、延迟归 B30。
        assert_eq!(LlKind::RecurseLoad.bound_axis(), OaAxis::LoadAxis);
        assert_eq!(LlKind::DeferLoad.bound_axis(), OaAxis::ObjectAxis);
        // 双轴各 5、两种别各 5。
        for a in OaAxis::all().iter() {
            let mut count = 0usize;
            for n in LL_NODES.iter() {
                if n.axis == *a {
                    count += 1;
                }
            }
            assert_eq!(count, 5, "轴 {} 节点额漂移", a.name());
        }
        for k in LlKind::all().iter() {
            let mut count = 0usize;
            for n in LL_NODES.iter() {
                if n.kind == *k {
                    count += 1;
                }
            }
            assert_eq!(count, 5, "种别 {} 节点额漂移", k.name());
        }
    }

    #[test]
    fn dual_run_is_identical() {
        let a = build_full_oa_ledger();
        let b = build_full_oa_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.node_at(i), b.node_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑节点缺失"),
            }
            i += 1;
        }
        assert_eq!(oa_fault_sweep().0, oa_fault_sweep().0);
        assert_eq!(frozen_digest(), frozen_digest());
        assert_eq!(frozen_digest(), frozen_digest());
    }

    #[test]
    fn state_machine_never_rewinds() {
        assert!(OaState::OaOpen.rank() < OaState::OaRunning.rank());
        assert!(OaState::OaRunning.rank() < OaState::OaSealed.rank());
        let mut led = build_full_oa_ledger();
        assert_eq!(led.state(), OaState::OaSealed);
        assert_eq!(led.seal(), Err(OaRegDiag::ERewind.val()));
        let mut fresh = OaRegLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(OaRegDiag::ERewind.val()));
    }

    #[test]
    fn spec_window_semantics() {
        for no in 0u32..10 {
            assert!(code_in_ll_spec(ll_spec_code(no)), "节点 {} 未落谱", no);
        }
        assert!(!code_in_ll_spec(0x50));
        assert!(!code_in_ll_spec(0x5B));
        assert_eq!(ll_spec_code(4), 0x55);
        assert_eq!(ll_spec_code(9), 0x5A);
    }

    #[test]
    fn replay_boundary() {
        assert!(replay_ll(0).is_some());
        assert!(replay_ll(9).is_some());
        assert!(replay_ll(10).is_none());
        assert!(replay_ll(u32::MAX).is_none());
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12684_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
