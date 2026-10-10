//! CGPU-F3525 · 文档版本与代码同步（CGPU-W 域 · SDK 文档域 · W05 组 · 目标 300 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F3525`
//!
//! **判据（锚点原文）**：对齐、两组、判据。
//!
//! **职责定位（锚点原文）**：文档版本与代码同步（版本对齐/漂移检测——
//! 同步实现）。
//!
//! ## 一、版本对齐（判据一）
//!
//! 文档版本与代码版本的对齐模型（[`SyncPair`]）：代码语义版本
//! （major.minor.patch）× 文档覆盖版本（声称描述到哪个版本）× 同步状态
//! 三态（[`SyncState`]：对齐/落后/超前）。判定规则写死：文档覆盖版本
//! == 代码版本 → 对齐；文档 < 代码 → 落后（新能力无文档——SDK 用户
//! 看不到）；文档 > 代码 → 超前（文档描述了不存在的能力——比落后更
//! 危险，因为它承诺谎言）。对齐判定是纯比较，O(1)。
//!
//! ## 二、漂移检测（判据二）
//!
//! 漂移两类（[`DriftKind`]）：签名漂移（文档记录的 API 签名指纹 vs
//! 代码签名指纹不一致——参数数/返回形态变了文档没跟）；语义漂移（文档
//! 声明的行为契约词 vs 代码契约词不一致——行为变了文档没跟）。指纹是
//! **注入的**（上游签名提取器产指纹，本模块是比对与记账面——零解析
//! 零 IO）。漂移台账（[`DriftLedger`]）逐条留痕：哪对、哪类、两侧指纹，
//! 修复后闭环销账（留痕不删——历史可审计）。
//!
//! ## 三、同步纪律（两组判据的合并面）
//!
//! 对齐组：三态判定 + 批量对账（一个文档族对全部登记对）；漂移组：
//! 两类检测 + 台账 + 销账。两组合并出同步结论（[`sync_verdict`]）：
//! 全对齐且零未销漂移 → 可发布；否则列清单（发布闸——文档与代码不同步
//! 的 SDK 不出 W 域发布门，呼应 vcw01 定位条款「文档即契约」）。
//!
//! **对接**：F3521（W 域开工——本模块是其同步面）；F3522/F3523/F3524
//! （信息架构/生成管线/API 参考——指纹的上游提取面）；V 域（验收判据
//! ——同步结论进发布判据）。零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const DOC_SYNC_VERSION: &str = "W05-docsync-v1";

// ---------------------------------------------------------------------------
// 二、诊断码（0x34 段独占：W05 同步面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x34 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 对账对不存在（查了没登记的文档键）。
    PairMissing,
    /// 指纹为空（空指纹 = 上游提取未闭合，拒绝入账）。
    EmptyFingerprint,
    /// 销账的漂移条目不存在（销账键打错）。
    DriftMissing,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::PairMissing => "对账对不存在：先 register_pair 登记文档↔代码对应关系再对账",
            XcCode::EmptyFingerprint => "空指纹：上游提取未闭合不是数据，拒绝入账并回查 F3523 生成管线",
            XcCode::DriftMissing => "销账键不存在：漂移条目按 id 销账，建议先查台账拿真实 id",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、版本对齐（判据一）
// ---------------------------------------------------------------------------

/// 语义版本三元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SemVer {
    /// major（语义断层）。
    pub major: u32,
    /// minor（向后兼容增量）。
    pub minor: u32,
    /// patch（修复）。
    pub patch: u32,
}

/// 同步状态三态（判定规则写死）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncState {
    /// 文档覆盖版本 == 代码版本。
    Aligned,
    /// 文档 < 代码（新能力无文档）。
    Behind,
    /// 文档 > 代码（文档承诺谎言——比落后更危险）。
    Ahead,
}

impl SyncState {
    /// 状态名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            SyncState::Aligned => "对齐",
            SyncState::Behind => "落后",
            SyncState::Ahead => "超前",
        }
    }
}

/// 对齐判定（O(1) 纯比较；手工三级比较——元组 PartialOrd 非 const 稳定，
/// 判定规则与语义版本序一致：major → minor → patch）。
pub const fn align_of(doc: SemVer, code: SemVer) -> SyncState {
    if doc.major != code.major {
        if doc.major < code.major {
            return SyncState::Behind;
        }
        return SyncState::Ahead;
    }
    if doc.minor != code.minor {
        if doc.minor < code.minor {
            return SyncState::Behind;
        }
        return SyncState::Ahead;
    }
    if doc.patch == code.patch {
        return SyncState::Aligned;
    }
    if doc.patch < code.patch {
        SyncState::Behind
    } else {
        SyncState::Ahead
    }
}

/// 对账对（一个文档单元 ↔ 一个代码单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncPair {
    /// 文档键（如 "api/render.submit"）。
    pub doc_key: String,
    /// 代码键（如 "vgpu::render::submit"）。
    pub code_key: String,
    /// 文档覆盖版本。
    pub doc_ver: SemVer,
    /// 代码版本。
    pub code_ver: SemVer,
    /// 文档记录的签名指纹（上游注入；空=未提取）。
    pub doc_fp: u64,
    /// 代码签名指纹（上游注入）。
    pub code_fp: u64,
    /// 文档声明的行为契约词（语义面对照锚）。
    pub doc_contract: String,
    /// 代码契约词（同上）。
    pub code_contract: String,
}

impl SyncPair {
    /// 对齐态判定。
    pub const fn state(&self) -> SyncState {
        align_of(self.doc_ver, self.code_ver)
    }
}

// ---------------------------------------------------------------------------
// 四、漂移检测（判据二）
// ---------------------------------------------------------------------------

/// 漂移两类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriftKind {
    /// 签名漂移（指纹不一致）。
    Signature,
    /// 语义漂移（契约词不一致）。
    Semantic,
}

impl DriftKind {
    /// 类名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            DriftKind::Signature => "签名漂移",
            DriftKind::Semantic => "语义漂移",
        }
    }
}

/// 漂移台账条目（留痕不删——修复销账是状态位不是删除）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriftEntry {
    /// 条目 id（单调分配）。
    pub id: u64,
    /// 对账对键（文档键——一对一条目）。
    pub doc_key: String,
    /// 漂移类。
    pub kind: DriftKind,
    /// 检出时两侧指纹（审计对照）。
    pub doc_fp: u64,
    pub code_fp: u64,
    /// 已销账（修复闭环标记；留痕不删）。
    pub resolved: bool,
}

/// 漂移检测器（登记对 + 检测 + 台账 + 销账）。
#[derive(Clone, Debug, Default)]
pub struct DriftLedger {
    pairs: Vec<SyncPair>,
    entries: Vec<DriftEntry>,
    next_id: u64,
}

impl DriftLedger {
    /// 新台账。
    pub const fn new() -> DriftLedger {
        DriftLedger {
            pairs: Vec::new(),
            entries: Vec::new(),
            next_id: 1,
        }
    }

    /// 登记对账对（同文档键重复登记拒绝——一对一约束）。
    pub fn register_pair(&mut self, pair: SyncPair) -> Result<(), XcCode> {
        if pair.doc_fp == 0 || pair.code_fp == 0 {
            return Err(XcCode::EmptyFingerprint);
        }
        let mut i = 0usize;
        while i < self.pairs.len() {
            if let Some(p) = self.pairs.get(i) {
                if p.doc_key == pair.doc_key {
                    return Err(XcCode::PairMissing); // 语义：键冲突按不存在新对拒
                }
            }
            i += 1;
        }
        self.pairs.push(pair);
        Ok(())
    }

    /// 全量对账（判据一组：三态判定 + 判据二组：两类漂移检测；返回
    /// (落后数, 超前数, 新检出漂移数)——一次遍历双组全出）。
    pub fn audit_all(&mut self) -> (usize, usize, usize) {
        let mut behind = 0usize;
        let mut ahead = 0usize;
        let mut new_drifts = 0usize;
        let mut i = 0usize;
        while i < self.pairs.len() {
            let (state, doc_key, dfp, cfp, dcontract, ccontract) = match self.pairs.get(i) {
                Some(p) => (
                    p.state(),
                    p.doc_key.clone(),
                    p.doc_fp,
                    p.code_fp,
                    p.doc_contract.clone(),
                    p.code_contract.clone(),
                ),
                None => break,
            };
            match state {
                SyncState::Behind => behind += 1,
                SyncState::Ahead => ahead += 1,
                SyncState::Aligned => {}
            }
            // 指纹漂移（签名面）与契约词漂移（语义面）分别立案。
            if dfp != cfp && self.open_entry(&doc_key, DriftKind::Signature, dfp, cfp) {
                new_drifts += 1;
            }
            if dcontract != ccontract
                && self.open_entry(&doc_key, DriftKind::Semantic, dfp, cfp)
            {
                new_drifts += 1;
            }
            i += 1;
        }
        (behind, ahead, new_drifts)
    }

    /// 立案（同键同类未销条目存在则不重复立案——幂等；返回是否新立案）。
    fn open_entry(&mut self, doc_key: &str, kind: DriftKind, doc_fp: u64, code_fp: u64) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.doc_key == doc_key && e.kind == kind && !e.resolved {
                    return false;
                }
            }
            i += 1;
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.entries.push(DriftEntry {
            id,
            doc_key: doc_key.to_string(),
            kind,
            doc_fp,
            code_fp,
            resolved: false,
        });
        true
    }

    /// 销账（修复闭环；条目不存在显性拒）。
    pub fn resolve(&mut self, id: u64) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get_mut(i) {
                if e.id == id {
                    e.resolved = true;
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(XcCode::DriftMissing)
    }

    /// 未销漂移数。
    pub fn open_len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if !e.resolved {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 对账对数。
    pub const fn pair_len(&self) -> usize {
        self.pairs.len()
    }
}

// ---------------------------------------------------------------------------
// 五、同步结论（发布闸：两组判据的合并面）
// ---------------------------------------------------------------------------

/// 同步结论（可发布性 + 人读清单）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncVerdict {
    /// 可发布（全对齐且零未销漂移）。
    pub publishable: bool,
    /// 结论清单（人读；不可发布时点名每处问题）。
    pub detail: String,
}

/// 同步结论（W 域发布闸的同步判据面）。
pub fn sync_verdict(l: &DriftLedger) -> SyncVerdict {
    let mut behind = 0usize;
    let mut ahead = 0usize;
    let mut i = 0usize;
    while i < l.pairs.len() {
        if let Some(p) = l.pairs.get(i) {
            match p.state() {
                SyncState::Behind => behind += 1,
                SyncState::Ahead => ahead += 1,
                SyncState::Aligned => {}
            }
        }
        i += 1;
    }
    let open = l.open_len();
    let publishable = behind == 0 && ahead == 0 && open == 0;
    let mut detail = String::from("同步结论：落后 ");
    detail.push_str(&behind.to_string());
    detail.push_str("，超前 ");
    detail.push_str(&ahead.to_string());
    detail.push_str("，未销漂移 ");
    detail.push_str(&open.to_string());
    if publishable {
        detail.push_str("——可发布");
    } else {
        detail.push_str("——文档与代码不同步，压住发布门");
    }
    SyncVerdict {
        publishable,
        detail,
    }
}
