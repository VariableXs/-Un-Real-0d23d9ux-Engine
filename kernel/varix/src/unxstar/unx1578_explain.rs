//! UNX-F1578 移交面人话化（explain 对齐体例） · 0x78F80段
//!
//! 判据原文（UNX-F1578-J1 摘录）：「移交面人话化覆盖 100%（联签/包/演练/
//! 时点四面全部状态可解释），一致性 1000 次逐字一致，快照校验通过」；完成时
//! API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 四面闭集：联签/包/演练/时点四面，每面状态闭集（联签四态/包三态/
//!    演练三态/时点三态），态出面即拒；
//! 2. 覆盖 100%：每个态必须有且只有一条人话模板（模板表冻结），缺模板即
//!    ENoTpl，重复模板即 EDupTpl——可解释不是「有话就说」而是「逐态有话」；
//! 3. 体例对齐：模板句式逐字以「面名·态名：人话」前缀起句，尾带可操作建议，
//!    句式不对齐记 EStyle 拒出；
//! 4. 一致性 1000 次：explain 为纯函数，千次调用逐字比对与首次一致，任何
//!    一次漂移即不一致；
//! 5. 快照校验：模板表整体折叠成快照哈希，冻结后重放复算逐字一致；状态机
//!    拟册→已覆盖→快照已冻结 单向；诊断码闭集 0x78F81..0x78F8C。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡给定值；B07 批窗口 0x78E00..=0x78FC0）。
pub const SEG_BASE: u32 = 0x78F80;

/// 一致性比对次数（判据钦定 1000 次）。
pub const CONSIST_RUNS: u32 = 1000;

/// 体例前缀（逐字钉死）。
pub const STYLE_PREFIX: &str = "面·态：";

/// 移交面人话化诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 面取值非法。
    EFace,
    /// 态取值非法。
    EState,
    /// 无对应人话模板。
    ENoTpl,
    /// 模板为空。
    EEmpty,
    /// 模板重复（同态双模板）。
    EDupTpl,
    /// 句式不对齐（缺体例前缀或建议尾）。
    EStyle,
    /// 覆盖面有缺（有态无模板）。
    EUncovered,
    /// 快照缺失。
    ENoSnap,
    /// 快照校验不符。
    EReplay,
    /// 册已冻结。
    EFrozen,
    /// 重复冻结。
    EDupSeal,
    /// 相位倒退。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EFace => 0x01,
                Diag::EState => 0x02,
                Diag::ENoTpl => 0x03,
                Diag::EEmpty => 0x04,
                Diag::EDupTpl => 0x05,
                Diag::EStyle => 0x06,
                Diag::EUncovered => 0x07,
                Diag::ENoSnap => 0x08,
                Diag::EReplay => 0x09,
                Diag::EFrozen => 0x0a,
                Diag::EDupSeal => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EFace => "面取值非法：只认联签/包/演练/时点四面",
            Diag::EState => "态取值非法：态必须落在该面闭集内",
            Diag::ENoTpl => "无对应人话模板：逐态补模板，缺一态不可出册",
            Diag::EEmpty => "模板为空：空模板视为未写，补人话句",
            Diag::EDupTpl => "模板重复：同态只留一条，删重复项",
            Diag::EStyle => "句式不对齐：按「面·态：人话+建议」体例重写",
            Diag::EUncovered => "覆盖面有缺：逐面逐态核对模板表，缺即补",
            Diag::ENoSnap => "快照缺失：冻结前必须落快照哈希",
            Diag::EReplay => "快照校验不符：模板表被改动，冻结册可疑",
            Diag::EFrozen => "册已冻结：冻结后写入拒绝，改动走下一批",
            Diag::EDupSeal => "重复冻结：冻结口唯一，重复冻结拒绝",
            Diag::EPhase => "相位倒退：拟册→已覆盖→快照已冻结 单向，倒退即拒",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EFace,
    Diag::EState,
    Diag::ENoTpl,
    Diag::EEmpty,
    Diag::EDupTpl,
    Diag::EStyle,
    Diag::EUncovered,
    Diag::ENoSnap,
    Diag::EReplay,
    Diag::EFrozen,
    Diag::EDupSeal,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x78F81..0x78F8C 闭集内"
}

/// 四面闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    /// 联签面。
    Cossign,
    /// 包面。
    Pack,
    /// 演练面。
    Drill,
    /// 时点面。
    Tick,
}

impl Face {
    /// 面名。
    pub fn name(self) -> &'static str {
        match self {
            Face::Cossign => "联签",
            Face::Pack => "包",
            Face::Drill => "演练",
            Face::Tick => "时点",
        }
    }
}

/// 四面闭集数组。
pub const FACE_ALL: &[Face] = &[Face::Cossign, Face::Pack, Face::Drill, Face::Tick];

/// 可解释态闭集（四面全部状态收敛到一个枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XState {
    /// 联签·待签。
    CosWait,
    /// 联签·已签。
    CosDone,
    /// 联签·有保留。
    CosReserve,
    /// 联签·否决。
    CosVeto,
    /// 包·拟包。
    PackDraft,
    /// 包·已审查。
    PackAudited,
    /// 包·已封包。
    PackSealed,
    /// 演练·未演练。
    DrillNone,
    /// 演练·演练中。
    DrillRun,
    /// 演练·演练过。
    DrillDone,
    /// 时点·拟稿。
    TickDraft,
    /// 时点·已声明。
    TickDeclared,
    /// 时点·已冻结。
    TickFrozen,
}

impl XState {
    /// 所属面。
    pub fn face(self) -> Face {
        match self {
            XState::CosWait | XState::CosDone | XState::CosReserve | XState::CosVeto => Face::Cossign,
            XState::PackDraft | XState::PackAudited | XState::PackSealed => Face::Pack,
            XState::DrillNone | XState::DrillRun | XState::DrillDone => Face::Drill,
            XState::TickDraft | XState::TickDeclared | XState::TickFrozen => Face::Tick,
        }
    }

    /// 态名。
    pub fn name(self) -> &'static str {
        match self {
            XState::CosWait => "待签",
            XState::CosDone => "已签",
            XState::CosReserve => "有保留",
            XState::CosVeto => "否决",
            XState::PackDraft => "拟包",
            XState::PackAudited => "已审查",
            XState::PackSealed => "已封包",
            XState::DrillNone => "未演练",
            XState::DrillRun => "演练中",
            XState::DrillDone => "演练过",
            XState::TickDraft => "拟稿",
            XState::TickDeclared => "已声明",
            XState::TickFrozen => "已冻结",
        }
    }
}

/// 可解释态闭集数组（四面 4+3+3+3 = 13 态）。
pub const XSTATE_ALL: &[XState] = &[
    XState::CosWait,
    XState::CosDone,
    XState::CosReserve,
    XState::CosVeto,
    XState::PackDraft,
    XState::PackAudited,
    XState::PackSealed,
    XState::DrillNone,
    XState::DrillRun,
    XState::DrillDone,
    XState::TickDraft,
    XState::TickDeclared,
    XState::TickFrozen,
];

/// 单条人话模板：态 + 句身（句式 = 前缀 + 面名·态名 + 人话 + 建议）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tpl {
    /// 态。
    pub state: XState,
    /// 人话句身（不含体例前缀与态名）。
    pub body: &'static str,
}

impl Tpl {
    /// 拼整句（体例对齐：以「面名·态名：」起句）。
    pub fn line(&self) -> String {
        format!("{}{}·{}：{}", STYLE_PREFIX, self.state.face().name(), self.state.name(), self.body)
    }
}

/// 冻结模板表（四面 13 态逐态一条，句身带可操作建议）。
pub const TPL_ALL: &[Tpl] = &[
    Tpl { state: XState::CosWait, body: "四方还没签齐，先催缺签方，别提前下结论" },
    Tpl { state: XState::CosDone, body: "四方签齐结论通过，可进入终验排期" },
    Tpl { state: XState::CosReserve, body: "签了但带保留项，先读附注清保留再推进" },
    Tpl { state: XState::CosVeto, body: "有方否决，先看否决理由单，整改后重签" },
    Tpl { state: XState::PackDraft, body: "包还在拟，五件没齐前不要外发" },
    Tpl { state: XState::PackAudited, body: "安检已过零命中，可以安排封包" },
    Tpl { state: XState::PackSealed, body: "包已封档，只读不改，改动走下一批" },
    Tpl { state: XState::DrillNone, body: "还没演练，移交前必须补一次全流程演练" },
    Tpl { state: XState::DrillRun, body: "演练进行中，先等演练结果再谈移交" },
    Tpl { state: XState::DrillDone, body: "演练通过留痕在册，可按窗口执行移交" },
    Tpl { state: XState::TickDraft, body: "时点声明还在拟稿，窗口和切换点没定" },
    Tpl { state: XState::TickDeclared, body: "三声明已生效，按切换点判责任归属" },
    Tpl { state: XState::TickFrozen, body: "声明已冻结，归属判定只读，改动走下一批" },
];

/// 册相位（拟册→已覆盖→快照已冻结 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 拟册：模板表登记中。
    Draft,
    /// 已覆盖：四面逐态覆盖核过。
    Covered,
    /// 快照已冻结：哈希落档、只读。
    Sealed,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Draft => 0,
            Phase::Covered => 1,
            Phase::Sealed => 2,
        }
    }
}

/// FNV-1a 折叠（整数哈希）。
fn fold_word(mut h: u64, w: u64) -> u64 {
    let mut k = w;
    let mut i = 0;
    while i < 8 {
        let b = (k & 0xFF) as u8;
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001B3);
        k >>= 8;
        i += 1;
    }
    h
}

fn fold_str(h: u64, s: &str) -> u64 {
    let mut acc = h;
    for b in s.as_bytes() {
        acc ^= *b as u64;
        acc = acc.wrapping_mul(0x100000001B3);
    }
    acc
}

/// 人话化册：模板表 + 覆盖核 + 快照冻结。
#[derive(Clone, Debug)]
pub struct ExplainBook {
    phase: Phase,
    /// 模板表（登记序，可含重复态——覆盖核时点出）。
    tpls: Vec<Tpl>,
    /// 快照哈希。
    snap: Option<u64>,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl ExplainBook {
    /// 以冻结模板表开册。
    pub fn new() -> ExplainBook {
        ExplainBook {
            phase: Phase::Draft,
            tpls: TPL_ALL.to_vec(),
            snap: None,
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 模板条数。
    pub fn tpl_count(&self) -> usize {
        self.tpls.len()
    }

    /// 查某态的模板（命中零条或多余一条都算异常）。
    pub fn tpl_of(&self, st: XState) -> Result<&Tpl, u32> {
        let hits: Vec<&Tpl> = self.tpls.iter().filter(|t| t.state == st).collect();
        if hits.is_empty() {
            return Err(Diag::ENoTpl.val());
        }
        if hits.len() > 1 {
            return Err(Diag::EDupTpl.val());
        }
        let t = match hits.get(0) {
            Some(x) => *x,
            None => return Err(Diag::ENoTpl.val()),
        };
        if t.body.is_empty() {
            return Err(Diag::EEmpty.val());
        }
        Ok(t)
    }

    /// explain：逐态人话输出（纯函数，千次一致）。
    pub fn explain(&self, st: XState) -> Result<String, u32> {
        let t = self.tpl_of(st)?;
        let line = t.line();
        // 体例校验：必须以前缀起句且含「面·态：」三段。
        let head = format!("{}{}·{}：", STYLE_PREFIX, st.face().name(), st.name());
        if !line.starts_with(&head) || line.len() <= head.len() {
            return Err(Diag::EStyle.val());
        }
        Ok(line)
    }

    /// 覆盖核：闭集内逐态可解释且句式对齐，返回缺口清单。
    pub fn cover(&mut self) -> Result<Vec<XState>, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::EFrozen.val());
        }
        if self.phase == Phase::Covered {
            return Err(Diag::EPhase.val());
        }
        let mut gaps = Vec::new();
        for st in XSTATE_ALL {
            if self.explain(*st).is_err() {
                gaps.push(*st);
            }
        }
        if gaps.is_empty() {
            self.phase = Phase::Covered;
        } else {
            self.rejects += 1;
            return Err(Diag::EUncovered.val());
        }
        Ok(gaps)
    }

    /// 快照哈希（模板表整体折叠）。
    pub fn snapshot_hash(&self) -> u64 {
        let mut h: u64 = 0xCBF29CE484222325;
        for t in &self.tpls {
            h = fold_word(h, t.state as u32 as u64);
            h = fold_str(h, t.body);
        }
        h
    }

    /// 冻结：已覆盖才可冻结，快照落档。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.phase == Phase::Sealed {
            return Err(Diag::EDupSeal.val());
        }
        if self.phase != Phase::Covered {
            return Err(Diag::EPhase.val());
        }
        let h = self.snapshot_hash();
        if h == 0 {
            return Err(Diag::ENoSnap.val());
        }
        self.snap = Some(h);
        self.phase = Phase::Sealed;
        Ok(h)
    }

    /// 快照哈希。
    pub fn snap(&self) -> Option<u64> {
        self.snap
    }

    /// 冻结后重放：快照复算逐字一致。
    pub fn replay(&self) -> Result<u32, u32> {
        if self.phase != Phase::Sealed {
            return Err(Diag::EPhase.val());
        }
        let s = match self.snap {
            Some(x) => x,
            None => return Err(Diag::ENoSnap.val()),
        };
        if self.snapshot_hash() != s {
            return Err(Diag::EReplay.val());
        }
        Ok(self.tpl_count() as u32)
    }

    /// 覆盖面摘要（落档面：四面逐面态数）。
    pub fn cover_text(&self) -> String {
        let mut out = String::new();
        for f in FACE_ALL {
            let mut n = 0usize;
            for st in XSTATE_ALL {
                if st.face() == *f {
                    n += 1;
                }
            }
            let seg = format!("面{} 态{}条", f.name(), n);
            out.push_str(&seg);
            out.push('\n');
        }
        out
    }
}

/// 一致性 1000 次：explain 千次逐字与首次一致。返回 (一致, 首句)。
pub fn consistency_1000(b: &ExplainBook, st: XState) -> (bool, String) {
    let first = match b.explain(st) {
        Ok(s) => s,
        Err(_) => return (false, String::new()),
    };
    let mut i = 1u32;
    while i < CONSIST_RUNS {
        match b.explain(st) {
            Ok(s) => {
                if s != first {
                    return (false, first);
                }
            }
            Err(_) => return (false, first),
        }
        i += 1;
    }
    (true, first)
}

/// 覆盖注入 10/10：十路缺模板样本（每路从基线册抽走一个态的模板）
/// 必须全数被覆盖核点出。返回十布尔。
pub fn gap_matrix() -> [bool; 10] {
    let mut out = [false; 10];
    let mut i = 0usize;
    while i < 10 {
        let victim = match XSTATE_ALL.get(i) {
            Some(x) => *x,
            None => break,
        };
        // 构造缺模板册：基线抽走 victim 态的模板。
        let mut probe = ExplainBook {
            phase: Phase::Draft,
            tpls: TPL_ALL.iter().filter(|t| t.state != victim).map(|t| *t).collect(),
            snap: None,
            rejects: 0,
        };
        // explain(victim) 必须显性拒。
        let no_tpl = probe.explain(victim) == Err(Diag::ENoTpl.val());
        // 覆盖核必须报缺口。
        let cover_blocked = probe.cover() == Err(Diag::EUncovered.val());
        // 缺口册不可冻结。
        let seal_blocked = probe.seal() == Err(Diag::EPhase.val());
        let cell = match out.get_mut(i) {
            Some(x) => x,
            None => break,
        };
        *cell = no_tpl && cover_blocked && seal_blocked;
        i += 1;
    }
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B07 派工卡 0x78E00..=0x78FC0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x78E00 || SEG_BASE > 0x78FC0 {
        return false;
    }
    SEG_BASE == 0x78F80
}

/// 四面闭集。
fn face_closed_ok() -> bool {
    if FACE_ALL.len() != 4 {
        return false;
    }
    let mut i = 0usize;
    while i < FACE_ALL.len() {
        let a = match FACE_ALL.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let mut j = i + 1;
        while j < FACE_ALL.len() {
            let b = match FACE_ALL.get(j) {
                Some(x) => *x,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 态-面归属闭合（13 态逐态 face 归属四面之一且面名非空）。
fn state_face_ok() -> bool {
    if XSTATE_ALL.len() != 13 {
        return false;
    }
    for st in XSTATE_ALL {
        let f = st.face();
        if FACE_ALL.iter().all(|x| *x != f) || f.name().is_empty() || st.name().is_empty() {
            return false;
        }
    }
    true
}

/// 覆盖 100%（基线册逐态可解释）。
fn cover_full_ok(b: &ExplainBook) -> bool {
    b.phase() == Phase::Covered && b.tpl_count() == XSTATE_ALL.len()
}

/// 一致性 1000 次逐字一致（全态扫）。
fn consist_ok(b: &ExplainBook) -> bool {
    for st in XSTATE_ALL {
        let (ok, first) = consistency_1000(b, *st);
        if !ok || first.is_empty() {
            return false;
        }
    }
    true
}

/// 缺口注入 10/10。
fn gap_ok() -> bool {
    gap_matrix() == [true; 10]
}

/// 双跑一致。
fn dual_run_ok() -> bool {
    let mut a = ExplainBook::new();
    let mut b = ExplainBook::new();
    let ca = a.cover();
    let cb = b.cover();
    match (ca, cb) {
        (Ok(x), Ok(y)) => x == y && a.snapshot_hash() == b.snapshot_hash() && a.snapshot_hash() != 0,
        _ => false,
    }
}

/// 十三项判据：UNX-F1578 移交面人话化。
pub fn run_unx1578_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1578");
    let mut b = ExplainBook::new();

    s.add_dyn(
        "F1578-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1578-02 四面闭集",
        face_closed_ok(),
        &format!(
            "n={} faces={}/{}/{}/{}",
            FACE_ALL.len(),
            Face::Cossign.name(),
            Face::Pack.name(),
            Face::Drill.name(),
            Face::Tick.name()
        ),
    );

    s.add_dyn(
        "F1578-03 十三态-面归属闭合",
        state_face_ok(),
        &format!("states={} tpls={}", XSTATE_ALL.len(), b.tpl_count()),
    );

    // 基线册逐态可解释且体例对齐。
    let mut all_ok = true;
    for st in XSTATE_ALL {
        match b.explain(*st) {
            Ok(line) => {
                if !line.starts_with(STYLE_PREFIX) || !line.contains(st.name()) {
                    all_ok = false;
                }
            }
            Err(_) => all_ok = false,
        }
    }
    s.add_dyn(
        "F1578-04 基线册逐态可解释体例齐",
        all_ok && b.tpl_count() == XSTATE_ALL.len(),
        &format!("tpls={}", b.tpl_count()),
    );

    // 覆盖核：100% 覆盖、相位推进、重复覆盖拒绝。
    let cov = b.cover();
    let cov_ok = match &cov {
        Ok(gaps) => gaps.is_empty(),
        Err(_) => false,
    };
    let dup = b.cover() == Err(Diag::EPhase.val());
    s.add_dyn(
        "F1578-05 覆盖核百分百单向",
        cov_ok && dup && b.phase() == Phase::Covered,
        &format!("phase_ord={}", b.phase().ord()),
    );

    // 一致性 1000 次逐字一致。
    s.add_dyn(
        "F1578-06 一致性一千次逐字一致",
        consist_ok(&b),
        &format!("runs={}", CONSIST_RUNS),
    );

    // 快照冻结：哈希落档、重复冻结拒绝、冻结后覆盖口拒绝。
    let sl = b.seal();
    let sl_ok = match sl {
        Ok(h) => h != 0,
        Err(_) => false,
    };
    let dup_seal = b.seal() == Err(Diag::EDupSeal.val());
    let cover_after = b.cover() == Err(Diag::EFrozen.val());
    s.add_dyn(
        "F1578-07 快照冻结闸门三向",
        sl_ok && dup_seal && cover_after && b.phase() == Phase::Sealed,
        &format!("snap={:016x}", b.snap().unwrap_or(0)),
    );

    // 冻结重放：快照复算逐字一致。
    s.add_dyn(
        "F1578-08 快照校验重放一致",
        b.replay() == Ok(XSTATE_ALL.len() as u32),
        &format!("replay={:?}", b.replay()),
    );

    // 缺口注入 10/10。
    s.add_dyn(
        "F1578-09 缺模板十路全点出",
        gap_ok(),
        &format!("matrix={:?}", gap_matrix()),
    );

    // 重复模板显性拒：双模板同态 explain 必须报 EDupTpl。
    let mut dup_book = ExplainBook {
        phase: Phase::Draft,
        tpls: {
            let mut v: Vec<Tpl> = TPL_ALL.to_vec();
            v.push(Tpl { state: XState::CosWait, body: "重复模板句" });
            v
        },
        snap: None,
        rejects: 0,
    };
    let dup_tpl = dup_book.explain(XState::CosWait) == Err(Diag::EDupTpl.val());
    s.add_dyn(
        "F1578-10 重复模板显性拒",
        dup_tpl,
        &format!("dup_tpl={}", dup_tpl),
    );

    // 覆盖面摘要落档：四面逐面态数齐（4+3+3+3）。
    let text = b.cover_text();
    s.add_dyn(
        "F1578-11 覆盖面摘要落档",
        text.contains("面联签 态4条")
            && text.contains("面包 态3条")
            && text.contains("面演练 态3条")
            && text.contains("面时点 态3条"),
        &format!("bytes={}", text.len()),
    );

    // 相位倒退显性拒（冻结册覆盖口与冻结口均拒）。
    let back = b.cover() == Err(Diag::EFrozen.val()) && b.seal() == Err(Diag::EDupSeal.val());
    s.add_dyn(
        "F1578-12 相位倒退显性拒",
        back && b.phase().ord() == 2,
        &format!("phase_ord={}", b.phase().ord()),
    );

    s.add_dyn(
        "F1578-13 双跑一致与体例前缀守卫",
        dual_run_ok() && STYLE_PREFIX == "面·态：" && ALL.len() == 12,
        &format!("prefix={} all={}", STYLE_PREFIX, ALL.len()),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faces_and_states_closed() {
        assert_eq!(FACE_ALL.len(), 4);
        assert_eq!(XSTATE_ALL.len(), 13);
        for st in XSTATE_ALL {
            assert!(FACE_ALL.iter().any(|f| *f == st.face()));
        }
    }

    #[test]
    fn tpl_table_complete_no_dup() {
        assert_eq!(TPL_ALL.len(), XSTATE_ALL.len());
        for st in XSTATE_ALL {
            let hits = TPL_ALL.iter().filter(|t| t.state == *st).count();
            assert_eq!(hits, 1, "态 {:?} 模板数应为 1", st);
        }
    }

    #[test]
    fn explain_style_aligned() {
        let b = ExplainBook::new();
        let line = b.explain(XState::CosDone).expect("已签可解释");
        assert!(line.starts_with(STYLE_PREFIX));
        assert!(line.contains("联签·已签："));
        assert!(line.len() > STYLE_PREFIX.len() + 8);
    }

    #[test]
    fn missing_tpl_explicit() {
        let mut probe = ExplainBook {
            phase: Phase::Draft,
            tpls: TPL_ALL.iter().filter(|t| t.state != XState::DrillRun).map(|t| *t).collect(),
            snap: None,
            rejects: 0,
        };
        assert_eq!(probe.explain(XState::DrillRun), Err(Diag::ENoTpl.val()));
        assert_eq!(probe.cover(), Err(Diag::EUncovered.val()));
        assert_eq!(probe.seal(), Err(Diag::EPhase.val()));
    }

    #[test]
    fn dup_tpl_explicit() {
        let mut v: Vec<Tpl> = TPL_ALL.to_vec();
        v.push(Tpl { state: XState::TickFrozen, body: "又一条" });
        let b = ExplainBook { phase: Phase::Draft, tpls: v, snap: None, rejects: 0 };
        assert_eq!(b.explain(XState::TickFrozen), Err(Diag::EDupTpl.val()));
    }

    #[test]
    fn consistency_thousand_runs() {
        let b = ExplainBook::new();
        for st in XSTATE_ALL {
            let (ok, first) = consistency_1000(&b, *st);
            assert!(ok, "态 {:?} 千次不一致", st);
            assert!(!first.is_empty());
        }
    }

    #[test]
    fn cover_seal_replay() {
        let mut b = ExplainBook::new();
        assert!(b.cover().is_ok());
        assert_eq!(b.cover(), Err(Diag::EPhase.val()), "重复覆盖拒绝");
        let h = b.seal().expect("冻结");
        assert_ne!(h, 0);
        assert_eq!(b.seal(), Err(Diag::EDupSeal.val()));
        assert_eq!(b.replay(), Ok(XSTATE_ALL.len() as u32));
    }

    #[test]
    fn gap_ten_of_ten() {
        let m = gap_matrix();
        assert_eq!(m, [true; 10]);
        assert!(gap_ok());
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn cover_text_four_faces() {
        let b = ExplainBook::new();
        let t = b.cover_text();
        assert!(t.contains("面联签 态4条"));
        assert!(t.contains("面时点 态3条"));
    }

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EFace.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert_eq!(detail(SEG_BASE + 0x80), "未知诊断码：不在 0x78F81..0x78F8C 闭集内");
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1578_checks();
        assert_eq!(s.len(), 13);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
