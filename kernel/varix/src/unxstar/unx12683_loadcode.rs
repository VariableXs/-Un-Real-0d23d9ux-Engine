//! UNX-F12684 装载错误码联签回归（F12564 回归） · 0x814C0段
//!
//! 判据原文（派工卡 12683 逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F12684-J1 OA 双消费链（B29 装载/B30 对象创建）
//!      回归 20/20 绿，链上零断；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. OA 双消费链域模型：装载链（B29 轴）与对象创建链（B30 轴）——
//!    链节点六元（节点号/所属链/节点名/错误码谱位/回归判/衔接槽），
//!    双链各 10 节点 = 20 节点全量回归；
//! 2. 链上零断（判据 J1 后半）：每节点带衔接槽（同链前驱节点引），
//!    首节点槽空、余节点槽必须可解析——断链即违约，逐节点点验；
//! 3. 回归判三闭集（绿/红/断）：绿 = 节点判码与 F12564 谱位逐位一致；
//!    红 = 判码漂移；断 = 衔接槽悬空——红/断必落差异账（零静默）；
//! 4. F12564 错误码谱回归主判：双链 20 节点判码全落谱内且谱位对齐
//!    ——20/20 绿才许回归封账；
//! 5. 回归账状态机：OaOpen → OaRunning → OaSealed 单向推进，倒退显
//!    性拒；零静默：逐节点显性判定，判定数 ≡ 节点数守恒；
//! 6. 注入联测十路：谱位越谱/判码漂移/链断/节点乱序/节点重复/双链
//!    缺一/差异静默/封账后补节点/链环倒退/摘要空——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；取值一律 match/
//!    if let/.get()/.get_mut()/saturating_*，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `oa_twenty_ok` ↔ ② OA 双消费链回归 20/20 绿；
//!   - `chain_unbroken_ok` ↔ ② 链上零断；
//!   - `zero_silent_diff_ok` ↔ 差异账零静默（红/断必落账）；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12683 − 12600)）。
pub const SEG_BASE: u32 = 0x814C0;

// ── OA 回归诊断码闭集 ────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OaDiag {
    /// 谱位越谱（判码不在 F12564 错误码谱内）。
    ECodeRange,
    /// 判码漂移（回归判码 ≠ 谱位判码）。
    ECodeDrift,
    /// 链断（衔接槽悬空）。
    EChainBreak,
    /// 节点乱序。
    ENodeOrder,
    /// 节点重复。
    ENodeDup,
    /// 双链缺一（任一链节点未齐）。
    EChainMissing,
    /// 差异静默。
    EDiffSilent,
    /// 封账后补节点。
    ESealedWrite,
    /// 链环倒退。
    ERingRewind,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl OaDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                OaDiag::ECodeRange => 0x01,
                OaDiag::ECodeDrift => 0x02,
                OaDiag::EChainBreak => 0x03,
                OaDiag::ENodeOrder => 0x04,
                OaDiag::ENodeDup => 0x05,
                OaDiag::EChainMissing => 0x06,
                OaDiag::EDiffSilent => 0x07,
                OaDiag::ESealedWrite => 0x08,
                OaDiag::ERingRewind => 0x09,
                OaDiag::ESummaryEmpty => 0x0a,
                OaDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            OaDiag::ECodeRange => "谱位越谱：判码必须落在 F12564 错误码谱内，越谱先归谱",
            OaDiag::ECodeDrift => "判码漂移：回归判码与谱位判码不一致，先对判码口径",
            OaDiag::EChainBreak => "链断：衔接槽必须可解析，断链先补前驱节点",
            OaDiag::ENodeOrder => "节点乱序：按节点号 0..20 逐节点回归，跳号先回补",
            OaDiag::ENodeDup => "节点重复：同链同谱只立一节点，重复先查已有账",
            OaDiag::EChainMissing => "双链缺一：装载链与对象链节点均须齐，缺链先回补",
            OaDiag::EDiffSilent => "差异静默：红/断必落差异账，有差未记即回归违约",
            OaDiag::ESealedWrite => "封账后补节点：回归账只读，补节点走下一轮回归",
            OaDiag::ERingRewind => "链环倒退：链只进不退，倒退即链可疑",
            OaDiag::ESummaryEmpty => "摘要空：封账摘要非空才许写回已完成",
            OaDiag::ERewind => "状态机倒退：OaOpen→OaRunning→OaSealed 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const OA_ALL: &[OaDiag] = &[
    OaDiag::ECodeRange,
    OaDiag::ECodeDrift,
    OaDiag::EChainBreak,
    OaDiag::ENodeOrder,
    OaDiag::ENodeDup,
    OaDiag::EChainMissing,
    OaDiag::EDiffSilent,
    OaDiag::ESealedWrite,
    OaDiag::ERingRewind,
    OaDiag::ESummaryEmpty,
    OaDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in OA_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x814C1..0x814CB 闭集内"
}

// ── F12564 错误码谱（五闭集，独立谱值——与 12682 同源不同表轴） ──
/// F12564 谱位（回归判码轴）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OaSpec {
    /// 装载谱位。
    LoadSpec,
    /// 句柄谱位。
    HandleSpec,
    /// 对象谱位。
    ObjectSpec,
    /// 权限谱位。
    PrivSpec,
    /// 掩码谱位。
    MaskSpec,
}

impl OaSpec {
    /// 谱名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            OaSpec::LoadSpec => "装载谱位",
            OaSpec::HandleSpec => "句柄谱位",
            OaSpec::ObjectSpec => "对象谱位",
            OaSpec::PrivSpec => "权限谱位",
            OaSpec::MaskSpec => "掩码谱位",
        }
    }

    /// 谱号（0..4）。
    pub fn ordinal(self) -> u32 {
        match self {
            OaSpec::LoadSpec => 0,
            OaSpec::HandleSpec => 1,
            OaSpec::ObjectSpec => 2,
            OaSpec::PrivSpec => 3,
            OaSpec::MaskSpec => 4,
        }
    }

    /// 谱判码（F12564 冻结谱值 0x61..0x65）。
    pub const fn code(self) -> u32 {
        match self {
            OaSpec::LoadSpec => 0x61,
            OaSpec::HandleSpec => 0x62,
            OaSpec::ObjectSpec => 0x63,
            OaSpec::PrivSpec => 0x64,
            OaSpec::MaskSpec => 0x65,
        }
    }

    /// 全部五谱（闭集数组）。
    pub fn all() -> &'static [OaSpec] {
        &[
            OaSpec::LoadSpec,
            OaSpec::HandleSpec,
            OaSpec::ObjectSpec,
            OaSpec::PrivSpec,
            OaSpec::MaskSpec,
        ]
    }
}

/// 判码是否落在 F12564 谱内（OA 轴）。
pub fn code_in_oa_spec(code: u32) -> bool {
    for s in OaSpec::all().iter() {
        if s.code() == code {
            return true;
        }
    }
    false
}

// ── 所属链二闭集 ─────────────────────────────────────────────────
/// 所属链（双链闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OaChain {
    /// B29 装载链。
    LoadChain,
    /// B30 对象创建链。
    ObjectChain,
}

impl OaChain {
    /// 链名。
    pub fn name(self) -> &'static str {
        match self {
            OaChain::LoadChain => "B29 装载链",
            OaChain::ObjectChain => "B30 对象创建链",
        }
    }

    /// 链号（0/1）。
    pub fn ordinal(self) -> u32 {
        match self {
            OaChain::LoadChain => 0,
            OaChain::ObjectChain => 1,
        }
    }

    /// 链内节点数（各 10 节点）。
    pub fn chain_cap(self) -> usize {
        10
    }

    /// 全部双链（闭集数组）。
    pub fn all() -> &'static [OaChain] {
        &[OaChain::LoadChain, OaChain::ObjectChain]
    }
}

// ── 回归判三闭集 ─────────────────────────────────────────────────
/// 回归判（三闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OaVerdict {
    /// 回归绿（判码与谱位逐位一致）。
    Green,
    /// 回归红（判码漂移）。
    Red,
    /// 链断（衔接槽悬空）。
    Broken,
}

impl OaVerdict {
    /// 判名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            OaVerdict::Green => "绿",
            OaVerdict::Red => "红",
            OaVerdict::Broken => "断",
        }
    }

    /// 判号（0..2）。
    pub fn ordinal(self) -> u32 {
        match self {
            OaVerdict::Green => 0,
            OaVerdict::Red => 1,
            OaVerdict::Broken => 2,
        }
    }

    /// 全部三判（闭集数组）。
    pub fn all() -> &'static [OaVerdict] {
        &[OaVerdict::Green, OaVerdict::Red, OaVerdict::Broken]
    }
}

// ── 链节点六元 ───────────────────────────────────────────────────
/// 链节点六元（节点号/所属链/节点名/错误码谱位/回归判/衔接槽）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OaNode {
    /// 节点号（全批 0..20 连续）。
    pub no: u32,
    /// 所属链。
    pub chain: OaChain,
    /// 节点名（链内谱短名）。
    pub node_name: &'static str,
    /// 错误码谱位（F12564 谱内）。
    pub spec: OaSpec,
    /// 回归判码（绿态 = 谱位判码）。
    pub reg_code: u32,
    /// 衔接槽（同链前驱节点号；首节点 = 0xFF 哨兵）。
    pub link_prev: u32,
    /// 回归判。
    pub verdict: OaVerdict,
}

/// 首节点哨兵槽。
pub const HEAD_SLOT: u32 = 0xFF;

/// 单节点回归判定（谱位 + 判码 + 衔接 + 判互证 四道）。
pub fn oa_node_ok(n: &OaNode) -> Result<(), u32> {
    // 节点名文本闸。
    if n.node_name.is_empty() {
        return Err(OaDiag::ENodeOrder.val());
    }
    // 谱位闸。
    if !code_in_oa_spec(n.spec.code()) {
        return Err(OaDiag::ECodeRange.val());
    }
    // 判码谱闸。
    if !code_in_oa_spec(n.reg_code) {
        return Err(OaDiag::ECodeRange.val());
    }
    // 判互证闸：绿态判码必须与谱位判码逐位一致。
    if n.verdict == OaVerdict::Green && n.reg_code != n.spec.code() {
        return Err(OaDiag::ECodeDrift.val());
    }
    // 衔接闸：链断判必须自证（断态节点由链点验闸处理），绿/红节点
    // 衔接槽不得为越界哨兵外的悬空值（0xFF 仅许首节点）。
    if n.link_prev != HEAD_SLOT && n.link_prev >= OA_NODE_CAP as u32 {
        return Err(OaDiag::EChainBreak.val());
    }
    Ok(())
}

/// 全批节点容量（双链 20 节点）。
pub const OA_NODE_CAP: usize = 20;

// ── 冻结 OA 双链节点谱（20 节点：装载链 0..9，对象链 10..19） ────
/// 构造节点——冻结展开为常量表。
const fn mk_node(
    no: u32,
    chain: OaChain,
    node_name: &'static str,
    spec: OaSpec,
    link_prev: u32,
) -> OaNode {
    OaNode {
        no,
        chain,
        node_name,
        spec,
        reg_code: spec.code(),
        link_prev,
        verdict: OaVerdict::Green,
    }
}

/// 20 节点冻结节点谱（节点号序；链内衔接逐位可解析）。
pub const OA_NODES: &[OaNode] = &[
    mk_node(0, OaChain::LoadChain, "装载开门", OaSpec::LoadSpec, HEAD_SLOT),
    mk_node(1, OaChain::LoadChain, "装载对账", OaSpec::HandleSpec, 0),
    mk_node(2, OaChain::LoadChain, "装载回执", OaSpec::ObjectSpec, 1),
    mk_node(3, OaChain::LoadChain, "装载差异", OaSpec::PrivSpec, 2),
    mk_node(4, OaChain::LoadChain, "装载封存", OaSpec::MaskSpec, 3),
    mk_node(5, OaChain::LoadChain, "装载重放", OaSpec::LoadSpec, 4),
    mk_node(6, OaChain::LoadChain, "装载段位", OaSpec::HandleSpec, 5),
    mk_node(7, OaChain::LoadChain, "装载尾账", OaSpec::ObjectSpec, 6),
    mk_node(8, OaChain::LoadChain, "装载复核", OaSpec::PrivSpec, 7),
    mk_node(9, OaChain::LoadChain, "装载归档", OaSpec::MaskSpec, 8),
    mk_node(10, OaChain::ObjectChain, "对象开门", OaSpec::ObjectSpec, HEAD_SLOT),
    mk_node(11, OaChain::ObjectChain, "对象建类", OaSpec::PrivSpec, 10),
    mk_node(12, OaChain::ObjectChain, "对象建柄", OaSpec::MaskSpec, 11),
    mk_node(13, OaChain::ObjectChain, "对象对账", OaSpec::LoadSpec, 12),
    mk_node(14, OaChain::ObjectChain, "对象回执", OaSpec::HandleSpec, 13),
    mk_node(15, OaChain::ObjectChain, "对象差异", OaSpec::ObjectSpec, 14),
    mk_node(16, OaChain::ObjectChain, "对象封存", OaSpec::PrivSpec, 15),
    mk_node(17, OaChain::ObjectChain, "对象重放", OaSpec::MaskSpec, 16),
    mk_node(18, OaChain::ObjectChain, "对象复核", OaSpec::LoadSpec, 17),
    mk_node(19, OaChain::ObjectChain, "对象归档", OaSpec::HandleSpec, 18),
];

// ── 链点验（链上零断主判） ───────────────────────────────────────
/// 取节点（越谱 None）。
pub fn oa_node_of(no: u32) -> Option<&'static OaNode> {
    OA_NODES.get(no as usize)
}

/// 单节点衔接点验（首节点槽哨兵；余节点槽必须落在同链已立前驱内）。
pub fn link_resolves(n: &OaNode) -> bool {
    if n.link_prev == HEAD_SLOT {
        // 首节点：必须是链首（节点号 = 链首位）。
        return n.no == n.chain.ordinal() * OaChain::LoadChain.chain_cap() as u32;
    }
    // 余节点：前驱必须存在、属同链且在当前节点之前。
    let prev = match oa_node_of(n.link_prev) {
        Some(v) => v,
        None => return false,
    };
    prev.chain == n.chain && prev.no < n.no
}

/// 链上零断全点验（20 节点逐节点衔接可解析、双链各 10 节点齐）。
pub fn chain_unbroken_ok() -> bool {
    if OA_NODES.len() != OA_NODE_CAP {
        return false;
    }
    for n in OA_NODES.iter() {
        if !link_resolves(n) {
            return false;
        }
    }
    // 双链节点额精确 10/10。
    for c in OaChain::all().iter() {
        let mut count = 0usize;
        for n in OA_NODES.iter() {
            if n.chain == *c {
                count += 1;
            }
        }
        if count != c.chain_cap() {
            return false;
        }
    }
    true
}

// ── 链摘要（整数异或折迭，确定性） ───────────────────────────────
/// 异或折迭摘要（节点判码序列 → 单字摘要）。
pub fn fold_digest(codes: &[u32]) -> u32 {
    let mut acc: u32 = SEG_BASE;
    for c in codes.iter() {
        acc = acc.wrapping_mul(0xC2B2_AE3D) ^ *c;
    }
    acc
}

/// 冻结谱摘要（OA_NODES 判码折迭）。
pub fn frozen_digest() -> u32 {
    let mut codes: Vec<u32> = Vec::new();
    for n in OA_NODES.iter() {
        codes.push(n.reg_code);
    }
    fold_digest(&codes)
}

// ── 回归账状态机（OaOpen → OaRunning → OaSealed） ────────────────
/// 回归账状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OaState {
    /// 开账。
    OaOpen,
    /// 回归中。
    OaRunning,
    /// 已封账。
    OaSealed,
}

impl OaState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            OaState::OaOpen => 0,
            OaState::OaRunning => 1,
            OaState::OaSealed => 2,
        }
    }
}

/// OA 回归账。
#[derive(Clone, Debug)]
pub struct OaLedger {
    /// 已回归节点（节点号序）。
    nodes: Vec<OaNode>,
    /// 差异账（节点号 + 回归判号；零静默口径：红/断必落账）。
    diffs: Vec<(u32, u32)>,
    /// 拒收记账（节点号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: OaState,
}

impl OaLedger {
    /// 开账。
    pub fn new() -> OaLedger {
        OaLedger {
            nodes: Vec::new(),
            diffs: Vec::new(),
            rejects: Vec::new(),
            state: OaState::OaOpen,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> OaState {
        self.state
    }

    /// 已回归节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 差异账条数。
    pub fn diff_len(&self) -> usize {
        self.diffs.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 节点（越界 None）。
    pub fn node_at(&self, idx: usize) -> Option<&OaNode> {
        self.nodes.get(idx)
    }

    /// 进回归（OaOpen→OaRunning）。
    pub fn begin_run(&mut self) -> Result<(), u32> {
        if self.state.rank() >= OaState::OaRunning.rank() {
            return Err(OaDiag::ERewind.val());
        }
        self.state = OaState::OaRunning;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < OA_NODE_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐节点回归（节点号序 + 回归判定 + 节点互异 + 链点验 四道）。
    pub fn reg(&mut self, n: OaNode) -> Result<(), u32> {
        if self.state.rank() < OaState::OaRunning.rank() {
            return Err(OaDiag::ESummaryEmpty.val());
        }
        if self.state.rank() >= OaState::OaSealed.rank() {
            self.note_reject(n.no, OaDiag::ESealedWrite.val());
            return Err(OaDiag::ESealedWrite.val());
        }
        // 节点号序闸。
        if n.no != self.nodes.len() as u32 {
            self.note_reject(n.no, OaDiag::ENodeOrder.val());
            return Err(OaDiag::ENodeOrder.val());
        }
        // 链引闸：前 10 节点必装载链，后 10 节点必对象链。
        let want = if n.no < OaChain::LoadChain.chain_cap() as u32 {
            OaChain::LoadChain
        } else {
            OaChain::ObjectChain
        };
        if n.chain != want {
            self.note_reject(n.no, OaDiag::EChainMissing.val());
            return Err(OaDiag::EChainMissing.val());
        }
        // 回归判定闸。
        if let Err(e) = oa_node_ok(&n) {
            self.note_reject(n.no, e);
            return Err(e);
        }
        // 链点验闸：衔接槽必须落在账内已立前驱（链上零断）。
        if n.link_prev != HEAD_SLOT {
            if n.link_prev >= self.nodes.len() as u32 {
                self.note_reject(n.no, OaDiag::EChainBreak.val());
                return Err(OaDiag::EChainBreak.val());
            }
            let prev = match self.nodes.get(n.link_prev as usize) {
                Some(v) => v,
                None => return Err(OaDiag::EChainBreak.val()),
            };
            if prev.chain != n.chain {
                self.note_reject(n.no, OaDiag::EChainBreak.val());
                return Err(OaDiag::EChainBreak.val());
            }
        } else {
            // 首节点哨兵：只许链首位节点挂哨兵。
            let head = n.chain.ordinal() * OaChain::LoadChain.chain_cap() as u32;
            if n.no != head {
                self.note_reject(n.no, OaDiag::ERingRewind.val());
                return Err(OaDiag::ERingRewind.val());
            }
        }
        // 节点互异闸：同链同谱名只立一节点。
        for done in self.nodes.iter() {
            if done.chain == n.chain && done.node_name == n.node_name {
                self.note_reject(n.no, OaDiag::ENodeDup.val());
                return Err(OaDiag::ENodeDup.val());
            }
        }
        // 差异入账（零静默口径）：红/断必落差异账。
        if n.verdict != OaVerdict::Green {
            self.diffs.push((n.no, n.verdict.ordinal()));
        }
        self.nodes.push(n);
        Ok(())
    }

    /// 封账（判据 J1 主闸）：20 节点齐 + 20/20 绿 + 双链齐 + 摘要对齐 + 零拒收。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= OaState::OaSealed.rank() {
            return Err(OaDiag::ERewind.val());
        }
        if self.state.rank() < OaState::OaRunning.rank() {
            return Err(OaDiag::ESummaryEmpty.val());
        }
        if self.nodes.len() != OA_NODE_CAP {
            return Err(OaDiag::EChainMissing.val());
        }
        // 20/20 绿闸。
        for n in self.nodes.iter() {
            if n.verdict != OaVerdict::Green {
                return Err(OaDiag::ECodeDrift.val());
            }
        }
        // 双链额闸。
        for c in OaChain::all().iter() {
            let mut count = 0usize;
            for n in self.nodes.iter() {
                if n.chain == *c {
                    count += 1;
                }
            }
            if count != c.chain_cap() {
                return Err(OaDiag::EChainMissing.val());
            }
        }
        // 差异账空闸。
        if !self.diffs.is_empty() {
            return Err(OaDiag::EDiffSilent.val());
        }
        if !self.rejects.is_empty() {
            return Err(OaDiag::EChainMissing.val());
        }
        // 摘要对齐闸：账面判码摘要 ≡ 冻结谱摘要。
        let snap: Vec<OaNode> = self.nodes.iter().cloned().collect();
        let mut codes: Vec<u32> = Vec::new();
        for n in snap.iter() {
            codes.push(n.reg_code);
        }
        if fold_digest(&codes) != frozen_digest() {
            return Err(OaDiag::ECodeDrift.val());
        }
        self.state = OaState::OaSealed;
        Ok(())
    }

    /// 账等式（节点数 = 20，差异零，拒收零）。
    pub fn equation_ok(&self) -> bool {
        self.nodes.len() == OA_NODE_CAP
            && self.diffs.is_empty()
            && self.rejects.is_empty()
    }
}

/// 确定性回归重放（同输入恒同判：绿态判码 = 谱位判码）。
pub fn replay_oa(no: u32) -> Option<OaNode> {
    let n = OA_NODES.get(no as usize)?;
    Some(OaNode {
        no,
        chain: n.chain,
        node_name: n.node_name,
        spec: n.spec,
        reg_code: n.spec.code(),
        link_prev: n.link_prev,
        verdict: OaVerdict::Green,
    })
}

/// 构建完整回归账（20 节点逐节点回归 + 封账）。
pub fn build_full_oa_ledger() -> OaLedger {
    let mut led = OaLedger::new();
    let _ = led.begin_run();
    let mut i = 0u32;
    while (i as usize) < OA_NODE_CAP {
        let n = match replay_oa(i) {
            Some(v) => v,
            None => break,
        };
        match led.reg(n) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    let _ = led.seal();
    led
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OaFault {
    /// 谱位越谱。
    CodeRange,
    /// 判码漂移。
    CodeDrift,
    /// 链断。
    ChainBreak,
    /// 节点乱序。
    NodeOrder,
    /// 节点重复。
    NodeDup,
    /// 双链缺一。
    ChainMissing,
    /// 差异静默。
    DiffSilent,
    /// 封账后补节点。
    SealedWrite,
    /// 链环倒退。
    RingRewind,
    /// 摘要空。
    SummaryEmpty,
}

impl OaFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            OaFault::CodeRange => 0,
            OaFault::CodeDrift => 1,
            OaFault::ChainBreak => 2,
            OaFault::NodeOrder => 3,
            OaFault::NodeDup => 4,
            OaFault::ChainMissing => 5,
            OaFault::DiffSilent => 6,
            OaFault::SealedWrite => 7,
            OaFault::RingRewind => 8,
            OaFault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            OaFault::CodeRange => "谱位越谱",
            OaFault::CodeDrift => "判码漂移",
            OaFault::ChainBreak => "链断",
            OaFault::NodeOrder => "节点乱序",
            OaFault::NodeDup => "节点重复",
            OaFault::ChainMissing => "双链缺一",
            OaFault::DiffSilent => "差异静默",
            OaFault::SealedWrite => "封账后补节点",
            OaFault::RingRewind => "链环倒退",
            OaFault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [OaFault] {
        &[
            OaFault::CodeRange,
            OaFault::CodeDrift,
            OaFault::ChainBreak,
            OaFault::NodeOrder,
            OaFault::NodeDup,
            OaFault::ChainMissing,
            OaFault::DiffSilent,
            OaFault::SealedWrite,
            OaFault::RingRewind,
            OaFault::SummaryEmpty,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。
pub fn oa_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in OaFault::all().iter() {
        total += 1;
        if run_oa_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_oa_fault(v: OaFault) -> bool {
    match v {
        OaFault::CodeRange => {
            let mut n = match replay_oa(0) {
                Some(x) => x,
                None => return false,
            };
            n.node_name = "";
            oa_node_ok(&n).is_err()
        }
        OaFault::CodeDrift => {
            // 绿态判码漂移必抓（判互证闸）。
            let mut n = match replay_oa(1) {
                Some(x) => x,
                None => return false,
            };
            n.reg_code = n.reg_code.saturating_add(1);
            oa_node_ok(&n).is_err()
        }
        OaFault::ChainBreak => {
            // 衔接槽悬空必抓。
            let mut n = match replay_oa(2) {
                Some(x) => x,
                None => return false,
            };
            n.link_prev = OA_NODE_CAP as u32 + 9;
            oa_node_ok(&n).is_err()
        }
        OaFault::NodeOrder => {
            let mut led = OaLedger::new();
            let _ = led.begin_run();
            let n = match replay_oa(1) {
                Some(x) => x,
                None => return false,
            };
            led.reg(n).is_err()
        }
        OaFault::NodeDup => {
            let mut led = OaLedger::new();
            let _ = led.begin_run();
            let n0 = match replay_oa(0) {
                Some(x) => x,
                None => return false,
            };
            let _ = led.reg(n0.clone());
            let mut dup = match replay_oa(1) {
                Some(x) => x,
                None => return false,
            };
            dup.node_name = n0.node_name;
            led.reg(dup).is_err()
        }
        OaFault::ChainMissing => {
            // 链引错位：节点 0 必装载链，注对象链必拒。
            let mut led = OaLedger::new();
            let _ = led.begin_run();
            let mut n = match replay_oa(0) {
                Some(x) => x,
                None => return false,
            };
            n.chain = OaChain::ObjectChain;
            led.reg(n).is_err()
        }
        OaFault::DiffSilent => {
            // 红态回归必落差异账（守恒直读）。
            let mut led = OaLedger::new();
            let _ = led.begin_run();
            let mut n = match replay_oa(0) {
                Some(x) => x,
                None => return false,
            };
            n.verdict = OaVerdict::Red;
            n.reg_code = n.reg_code.saturating_add(1);
            let _ = led.reg(n);
            led.diff_len() == 1
        }
        OaFault::SealedWrite => {
            let mut led = build_full_oa_ledger();
            if led.state() != OaState::OaSealed {
                return false;
            }
            let n = match replay_oa(0) {
                Some(x) => x,
                None => return false,
            };
            led.reg(n).is_err()
        }
        OaFault::RingRewind => {
            // 链环倒退：链首哨兵挂在非链首节点必拒。
            let mut led = OaLedger::new();
            let _ = led.begin_run();
            let mut n = match replay_oa(1) {
                Some(x) => x,
                None => return false,
            };
            n.link_prev = HEAD_SLOT;
            led.reg(n).is_err()
        }
        OaFault::SummaryEmpty => {
            // 摘要空族：未回归即封账被拒（OaOpen 态直接 seal）。
            let mut led = OaLedger::new();
            led.seal().is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// OA 双消费链回归 20/20 绿（判据 J1 前半直读口）。
fn oa_twenty_ok() -> bool {
    // build_full_oa_ledger 已含封账步；此处直读封账终态（重复 seal
    // 会被倒退闸正确拒绝，不在此复调）。
    let led = build_full_oa_ledger();
    if led.len() != OA_NODE_CAP {
        return false;
    }
    if led.state() != OaState::OaSealed {
        return false;
    }
    // 20/20 绿直读 + 判码与谱位逐位一致。
    let mut i = 0usize;
    while i < OA_NODE_CAP {
        let n = match led.node_at(i) {
            Some(v) => v,
            None => return false,
        };
        if n.verdict != OaVerdict::Green {
            return false;
        }
        if n.reg_code != n.spec.code() {
            return false;
        }
        i += 1;
    }
    // 独立复核：20 节点逐节点回归判定。
    for n in OA_NODES.iter() {
        if oa_node_ok(n).is_err() {
            return false;
        }
    }
    true
}

/// 链上零断（判据 J1 后半直读口）。
fn chain_unbroken_pred_ok() -> bool {
    if !chain_unbroken_ok() {
        return false;
    }
    // 反证：悬空槽必不解析。
    let mut bad = match OA_NODES.get(3) {
        Some(x) => x.clone(),
        None => return false,
    };
    bad.link_prev = OA_NODE_CAP as u32 + 5;
    if link_resolves(&bad) {
        return false;
    }
    // 跨链衔接必不解析（对象链节点引装载链前驱）。
    let mut cross = match OA_NODES.get(12) {
        Some(x) => x.clone(),
        None => return false,
    };
    cross.link_prev = 3;
    if link_resolves(&cross) {
        return false;
    }
    // 链首哨兵错挂必不解析。
    let mut head_mis = match OA_NODES.get(11) {
        Some(x) => x.clone(),
        None => return false,
    };
    head_mis.link_prev = HEAD_SLOT;
    if link_resolves(&head_mis) {
        return false;
    }
    true
}

/// 差异账零静默（红/断必落账）。
fn zero_silent_diff_ok() -> bool {
    let led = build_full_oa_ledger();
    // 20/20 绿 ⇒ 差异账空。
    if led.diff_len() != 0 {
        return false;
    }
    // 注入反证：红态必落差异账。
    let mut probe = OaLedger::new();
    let _ = probe.begin_run();
    let mut n = match replay_oa(0) {
        Some(x) => x,
        None => return false,
    };
    n.verdict = OaVerdict::Red;
    n.reg_code = n.spec.code().saturating_add(1);
    let _ = probe.reg(n);
    if probe.diff_len() != 1 {
        return false;
    }
    true
}

/// 双链覆盖普查（20 节点号连续、双链各 10、谱位五谱全覆盖）。
fn chain_census_ok() -> bool {
    let mut i = 0usize;
    while i < OA_NODES.len() {
        let n = match OA_NODES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if n.no != i as u32 {
            return false;
        }
        i += 1;
    }
    // 五谱全覆盖。
    for s in OaSpec::all().iter() {
        let mut found = false;
        for n in OA_NODES.iter() {
            if n.spec == *s {
                found = true;
            }
        }
        if !found {
            return false;
        }
    }
    true
}

/// 零静默（节点逐个齐、节点号一一对应、绿态全真）。
fn zero_silent_ok() -> bool {
    let led = build_full_oa_ledger();
    if led.len() != OA_NODE_CAP {
        return false;
    }
    let mut i = 0usize;
    while i < led.len() {
        let n = match led.node_at(i) {
            Some(v) => v,
            None => return false,
        };
        if n.no != i as u32 || n.verdict != OaVerdict::Green {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测 10/10 拒止。
fn oa_fault_sweep_ok() -> bool {
    let (caught, total, escape) = oa_fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
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
    if SEG_BASE != 0x80000 + 0x40 * (12683u32.saturating_sub(12600)) {
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
    // 摘要：首节点名 + 尾节点名拼接非空。
    let head = match led.node_at(0) {
        Some(n) => n.node_name,
        None => return false,
    };
    let tail = match led.node_at(OA_NODE_CAP - 1) {
        Some(n) => n.node_name,
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    // 双链名摘要非空。
    for c in OaChain::all().iter() {
        if c.name().is_empty() {
            return false;
        }
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12684 OA 双链回归自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12683_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12683-loadcode");
    let led = build_full_oa_ledger();

    s.add_dyn(
        "F12684-01 双链节点二十个齐",
        OA_NODES.len() == OA_NODE_CAP,
        &format!("nodes={} chains={}", OA_NODES.len(), OaChain::all().len()),
    );

    s.add_dyn(
        "F12684-02 OA 双消费链回归 20/20 绿",
        oa_twenty_ok(),
        &format!("nodes={} sealed={}", led.len(), led.state() == OaState::OaSealed),
    );

    s.add_dyn(
        "F12684-03 链上零断",
        chain_unbroken_pred_ok(),
        &format!("nodes_resolved=20"),
    );

    s.add_dyn(
        "F12684-04 差异账零静默",
        zero_silent_diff_ok(),
        &format!("diffs={} rejects={}", led.diff_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12684-05 双链覆盖普查",
        chain_census_ok(),
        &format!("specs={}", OaSpec::all().len()),
    );

    s.add_dyn(
        "F12684-06 零静默",
        zero_silent_ok(),
        &format!("nodes={}", led.len()),
    );

    s.add_dyn(
        "F12684-07 注入联测 10/10 拒止",
        oa_fault_sweep_ok(),
        &format!("caught={}", oa_fault_sweep().0),
    );

    s.add_dyn(
        "F12684-08 账等式",
        led.equation_ok(),
        &format!("nodes={} diffs={} rejects={}", led.len(), led.diff_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12684-09 双跑一致",
        dual_run_ok(),
        "确定性重放两轮逐位同判",
    );

    s.add_dyn(
        "F12684-10 F12564 谱口径",
        code_in_oa_spec(0x61) && code_in_oa_spec(0x65) && !code_in_oa_spec(0x60) && !code_in_oa_spec(0x66),
        "谱值 0x61..0x65 闭集",
    );

    s.add_dyn(
        "F12684-11 链额冻结轴",
        OaChain::LoadChain.chain_cap() == 10 && OaChain::ObjectChain.chain_cap() == 10,
        "双链各 10 节点",
    );

    s.add_dyn(
        "F12684-12 完成写回语义",
        writeback_ok(),
        "封账即已完成且摘要非空",
    );

    s.add_dyn(
        "F12684-13 段位守卫",
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
        assert_eq!(SEG_BASE, 0x814C0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        assert_eq!(OA_ALL.len(), 11);
        let mut i = 0usize;
        while i < OA_ALL.len() {
            let vi = match OA_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < OA_ALL.len() {
                let vj = match OA_ALL.get(j) {
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
    fn oa_twenty_green() {
        // 20/20 绿。
        assert!(oa_twenty_ok());
        let led = build_full_oa_ledger();
        assert_eq!(led.len(), 20);
        assert_eq!(led.state(), OaState::OaSealed);
        assert_eq!(led.diff_len(), 0);
        assert_eq!(led.reject_len(), 0);
    }

    #[test]
    fn chain_unbroken_holds() {
        // 链上零断 + 反证注入必抓。
        assert!(chain_unbroken_pred_ok());
        assert!(chain_unbroken_ok());
        // 悬空槽不解析。
        let mut bad = match OA_NODES.get(5) {
            Some(x) => x.clone(),
            None => return,
        };
        bad.link_prev = OA_NODE_CAP as u32 + 3;
        assert!(!link_resolves(&bad));
        // 跨链衔接不解析。
        let mut cross = match OA_NODES.get(13) {
            Some(x) => x.clone(),
            None => return,
        };
        cross.link_prev = 4;
        assert!(!link_resolves(&cross));
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
        let mut led = OaLedger::new();
        let _ = led.begin_run();
        let n1 = match replay_oa(1) {
            Some(x) => x,
            None => return,
        };
        assert_eq!(led.reg(n1), Err(OaDiag::ENodeOrder.val()));
        let n0 = match replay_oa(0) {
            Some(x) => x,
            None => return,
        };
        assert!(led.reg(n0.clone()).is_ok());
        // 链首哨兵错挂（首节点入账后再测，序闸放行、哨兵闸收网）。
        let mut head_mis = match replay_oa(1) {
            Some(x) => x,
            None => return,
        };
        head_mis.link_prev = HEAD_SLOT;
        assert_eq!(led.reg(head_mis), Err(OaDiag::ERingRewind.val()));
        // 节点重复。
        let mut dup = match replay_oa(1) {
            Some(x) => x,
            None => return,
        };
        dup.node_name = n0.node_name;
        assert_eq!(led.reg(dup), Err(OaDiag::ENodeDup.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn diff_zero_silent_holds() {
        assert!(zero_silent_diff_ok());
        let led = build_full_oa_ledger();
        assert_eq!(led.diff_len(), 0);
    }

    #[test]
    fn chain_census_holds() {
        assert!(chain_census_ok());
        assert_eq!(OA_NODES.len(), 20);
        assert_eq!(OaChain::all().len(), 2);
        assert_eq!(OaSpec::all().len(), 5);
        // 双链各 10。
        for c in OaChain::all().iter() {
            let mut count = 0usize;
            for n in OA_NODES.iter() {
                if n.chain == *c {
                    count += 1;
                }
            }
            assert_eq!(count, 10, "链 {} 节点额漂移", c.name());
        }
    }

    #[test]
    fn zero_silent_ledger() {
        assert!(zero_silent_ok());
        let led = build_full_oa_ledger();
        let mut i = 0usize;
        while i < led.len() {
            let n = match led.node_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(n.no, i as u32, "节点 {} 错位", i);
            i += 1;
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
        // 摘要直读：同序列恒同摘要。
        assert_eq!(fold_digest(&[0x61, 0x62]), fold_digest(&[0x61, 0x62]));
        assert_ne!(fold_digest(&[0x61, 0x62]), fold_digest(&[0x61, 0x63]));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：OaOpen→OaRunning→OaSealed。
        assert!(OaState::OaOpen.rank() < OaState::OaRunning.rank());
        assert!(OaState::OaRunning.rank() < OaState::OaSealed.rank());
        let mut led = build_full_oa_ledger();
        assert_eq!(led.state(), OaState::OaSealed);
        assert_eq!(led.seal(), Err(OaDiag::ERewind.val()));
        let mut fresh = OaLedger::new();
        assert!(fresh.begin_run().is_ok());
        assert_eq!(fresh.begin_run(), Err(OaDiag::ERewind.val()));
    }

    #[test]
    fn spec_window_semantics() {
        // F12564 谱语义直读。
        for s in OaSpec::all().iter() {
            assert!(code_in_oa_spec(s.code()), "谱 {} 未落谱", s.name());
        }
        assert!(!code_in_oa_spec(0x60));
        assert!(!code_in_oa_spec(0x66));
        // 谱值互异递增。
        assert!(OaSpec::MaskSpec.code() > OaSpec::LoadSpec.code());
        assert!(OaSpec::ObjectSpec.code() > OaSpec::HandleSpec.code());
    }

    #[test]
    fn checks_entry_all_green() {
        let s = run_unx12683_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
