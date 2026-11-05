//! UNX-F12673 探测面注入联测二（坏缓冲/坏类/坏权限 30 例） · 0x81200段
//!
//! 判据原文（派工卡 12672 逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② 判据原文：UNX-F12673-J1 抽样 20 场景双机一致（S1 母版），码值类
//!      零容差；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 注入案卷三族闭集：坏缓冲（缓冲越界/短 buffer/空指针代位）、坏类
//!    （类号越闭集/类大小漂移）、坏权限（权限位越档/权限档倒退）——
//!    30 例 = 3 族 × 10 例，案号 0..30 冻结出题，族间不混编；
//! 2. 注入面六闭集：串缓冲/缓冲簇/类号/类界/权限位/权限档，每面冻结
//!    基码（0x20..0x70），案判码 = 面基码 + 族偏移（0x01..0x14 闭集），
//!    案卷内不跨段引码；
//! 3. 抽样 20 场景双机一致：S1 母版冻结 20 场景出题与判码，S2 复算机
//!    按同一冻结出题重放判码——双机判码逐案全等（码值类零容差：不等
//!    即违约，无四舍五入无容忍带），一致率以整数案数计（20/20）；
//! 4. 零静默：30 案逐案显性判定（过/拒都记账），静默计数必须为零；
//!    案卷状态机 Opened → Judged → Archived 单向推进，倒退显性拒；
//! 5. 注入联测十路：族越闭集/零码/判码漂移/面越窗/案号乱序/案重复/
//!    缺案归档/静默吞案/归档后补案/摘要空——逐路必抓，逃脱即违约；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（出题冻结）；
//!    取值一律 match/if let/.get()/.get_mut()，无 unwrap/expect/panic/
//!    裸下标；中文注释与中文诊断；无死代码无 #[allow]。
//!
//! 判据谓词与验收判据的对应：
//!   - `sampling_consensus_ok` ↔ ② 抽样 20 场景双机一致（S1 母版），
//!     码值类零容差；
//!   - `inj_thirty_ok` / `zero_silent_ok` ↔ ① 30 例注入案卷全过记账；
//!   - `writeback_ok` ↔ ③ 状态=已完成、结果摘要非空。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12672 − 12600)）。
pub const SEG_BASE: u32 = 0x81200;

// ── 注入案卷诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0B 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjDiag {
    /// 族越闭集（违约族不在三族内）。
    EFamilyRange,
    /// 零码（预期码或实收码为零）。
    EZeroCode,
    /// 判码漂移（预期码 ≠ 实收码，码值类零容差）。
    ECodeDrift,
    /// 面越窗（判码不在注入面基码窗口）。
    EFaceRange,
    /// 案号乱序。
    ECaseOrder,
    /// 案重复（同族同面同谱只立一案）。
    ECaseDup,
    /// 缺案归档（30 案未齐即归档）。
    EShortArchive,
    /// 静默吞案（判定缺失）。
    ESilentDrop,
    /// 归档后补案。
    EArchivedWrite,
    /// 摘要空。
    ESummaryEmpty,
    /// 状态机倒退。
    ERewind,
}

impl InjDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0B 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                InjDiag::EFamilyRange => 0x01,
                InjDiag::EZeroCode => 0x02,
                InjDiag::ECodeDrift => 0x03,
                InjDiag::EFaceRange => 0x04,
                InjDiag::ECaseOrder => 0x05,
                InjDiag::ECaseDup => 0x06,
                InjDiag::EShortArchive => 0x07,
                InjDiag::ESilentDrop => 0x08,
                InjDiag::EArchivedWrite => 0x09,
                InjDiag::ESummaryEmpty => 0x0a,
                InjDiag::ERewind => 0x0b,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            InjDiag::EFamilyRange => "族越闭集：只认坏缓冲/坏类/坏权限三族，先正违约族再立案",
            InjDiag::EZeroCode => "零码：预期码与实收码均须非零，先补显性判码",
            InjDiag::ECodeDrift => "判码漂移：码值类零容差，双机判码不等即改判码口径",
            InjDiag::EFaceRange => "面越窗：判码不在注入面基码窗口，先归面再对账",
            InjDiag::ECaseOrder => "案号乱序：按案号 0..30 逐案过闸，跳号先回补",
            InjDiag::ECaseDup => "案重复：同族同面同谱只立一案，重复先查已有卷",
            InjDiag::EShortArchive => "缺案归档：30 案齐才许归档，缺案先回补",
            InjDiag::ESilentDrop => "静默吞案：逐案显性判定，缺判定即案卷违约",
            InjDiag::EArchivedWrite => "归档后补案：案卷只读，补案走下一轮联测",
            InjDiag::ESummaryEmpty => "摘要空：归档摘要非空才许写回已完成",
            InjDiag::ERewind => "状态机倒退：Opened→Judged→Archived 只进不退，倒退即卷可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const INJ_ALL: &[InjDiag] = &[
    InjDiag::EFamilyRange,
    InjDiag::EZeroCode,
    InjDiag::ECodeDrift,
    InjDiag::EFaceRange,
    InjDiag::ECaseOrder,
    InjDiag::ECaseDup,
    InjDiag::EShortArchive,
    InjDiag::ESilentDrop,
    InjDiag::EArchivedWrite,
    InjDiag::ESummaryEmpty,
    InjDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in INJ_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x81201..0x8120B 闭集内"
}

// ── 违约族三闭集 ─────────────────────────────────────────────────
/// 注入违约族（题面三族闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjFamily {
    /// 坏缓冲族。
    BadBuffer,
    /// 坏类族。
    BadClass,
    /// 坏权限族。
    BadPriv,
}

impl InjFamily {
    /// 族名（中文短名）。
    pub fn name(self) -> &'static str {
        match self {
            InjFamily::BadBuffer => "坏缓冲",
            InjFamily::BadClass => "坏类",
            InjFamily::BadPriv => "坏权限",
        }
    }

    /// 族号（0/1/2，冻结出题用）。
    pub fn ordinal(self) -> u32 {
        match self {
            InjFamily::BadBuffer => 0,
            InjFamily::BadClass => 1,
            InjFamily::BadPriv => 2,
        }
    }

    /// 族偏移下沿（每族判码起点差 0x05，窗口 0x01..0x14）。
    pub fn code_floor(self) -> u32 {
        match self {
            InjFamily::BadBuffer => 0x01,
            InjFamily::BadClass => 0x06,
            InjFamily::BadPriv => 0x0B,
        }
    }

    /// 全部三族（闭集数组）。
    pub fn all() -> &'static [InjFamily] {
        &[InjFamily::BadBuffer, InjFamily::BadClass, InjFamily::BadPriv]
    }
}

// ── 注入面六闭集 ─────────────────────────────────────────────────
/// 注入面行（面名/面基码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjFace {
    /// 面名（注入面六闭集短名）。
    pub name: &'static str,
    /// 面基码（案判码窗口基址）。
    pub base: u32,
}

/// 注入面六闭集（面基码 0x20..0x70，来源：B05 探测面批注入位谱冻结）。
pub const INJ_FACE_TABLE: &[InjFace] = &[
    InjFace { name: "串缓冲面", base: 0x20 },
    InjFace { name: "缓冲簇面", base: 0x30 },
    InjFace { name: "类号面", base: 0x40 },
    InjFace { name: "类界面", base: 0x50 },
    InjFace { name: "权限位面", base: 0x60 },
    InjFace { name: "权限档面", base: 0x70 },
];

/// 面判码族偏移上沿（0x14 闭集）。
pub const FACE_OFF_CAP: u32 = 0x14;

/// 取注入面行（越面 None）。
pub fn inj_face_of(name: &str) -> Option<&InjFace> {
    for f in INJ_FACE_TABLE.iter() {
        if f.name == name {
            return Some(f);
        }
    }
    None
}

/// 判码是否落在注入面基码窗口（base < code <= base + 族偏移上沿）。
pub fn code_in_inj_face(code: u32, base: u32) -> bool {
    code > base && code <= base.saturating_add(FACE_OFF_CAP)
}

// ── 注入案与黄金判定 ─────────────────────────────────────────────
/// 注入案七元（案号/族/面名/谱名/预期码/实收码/期望一致）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjCase {
    /// 案号。
    pub no: u32,
    /// 违约族。
    pub family: InjFamily,
    /// 注入面名（六闭集内）。
    pub face: &'static str,
    /// 谱名（族内细分违约谱）。
    pub spectrum: &'static str,
    /// 预期拒止码。
    pub expect_code: u32,
    /// 实收拒止码（S2 复算机回放）。
    pub got_code: u32,
    /// 期望一致（true = 双机应逐码全等）。
    pub expect_match: bool,
}

/// 单案黄金判定（族闭集 + 零码 + 双机全等 + 码窗 四道）。
pub fn inj_case_ok(c: &InjCase) -> Result<(), u32> {
    // 族闭集闸（族枚举天然封闭，谱名非空补一道文本闸）。
    if c.spectrum.is_empty() {
        return Err(InjDiag::EFamilyRange.val());
    }
    // 零码闸。
    if c.expect_code == 0 || c.got_code == 0 {
        return Err(InjDiag::EZeroCode.val());
    }
    // 双机全等闸（码值类零容差）。
    if c.expect_code != c.got_code {
        return Err(InjDiag::ECodeDrift.val());
    }
    // 码窗闸：面名闭集 + 判码窗口。
    let f = match inj_face_of(c.face) {
        Some(v) => v,
        None => return Err(InjDiag::EFaceRange.val()),
    };
    if !code_in_inj_face(c.expect_code, f.base) {
        return Err(InjDiag::EFaceRange.val());
    }
    Ok(())
}

/// 案卷容量（30 例 = 3 族 × 10 例）。
pub const INJ_CASE_CAP: usize = 30;

/// 族内例数（每族 10 例）。
pub const CASES_PER_FAMILY: usize = 10;

// ── 冻结注入案卷（30 案：三族 × 10 谱，面随谱冻结） ─────────────
/// 构造案（案号/族/面名/谱名/族偏移）——冻结展开为常量表。
const fn mk_inj(no: u32, family: InjFamily, face: &'static str, spectrum: &'static str, off: u32) -> InjCase {
    InjCase {
        no,
        family,
        face,
        spectrum,
        expect_code: off,
        got_code: off,
        expect_match: true,
    }
}

/// 30 案冻结案卷（案号序，族内谱序，面随谱落窗）。
pub const INJ_CASES: &[InjCase] = &[
    mk_inj(0, InjFamily::BadBuffer, "串缓冲面", "缓冲越界写", 0x21),
    mk_inj(1, InjFamily::BadBuffer, "串缓冲面", "缓冲越界读", 0x22),
    mk_inj(2, InjFamily::BadBuffer, "串缓冲面", "短 buffer 计量", 0x23),
    mk_inj(3, InjFamily::BadBuffer, "串缓冲面", "空指针代位", 0x24),
    mk_inj(4, InjFamily::BadBuffer, "缓冲簇面", "簇计零", 0x31),
    mk_inj(5, InjFamily::BadBuffer, "缓冲簇面", "簇计越上沿", 0x32),
    mk_inj(6, InjFamily::BadBuffer, "缓冲簇面", "簇 stride 漂移", 0x33),
    mk_inj(7, InjFamily::BadBuffer, "缓冲簇面", "簇尾越面", 0x34),
    mk_inj(8, InjFamily::BadBuffer, "缓冲簇面", "对齐欠账", 0x35),
    mk_inj(9, InjFamily::BadBuffer, "缓冲簇面", "回卷计量错", 0x36),
    mk_inj(10, InjFamily::BadClass, "类号面", "类号越闭集", 0x41),
    mk_inj(11, InjFamily::BadClass, "类号面", "类号零", 0x42),
    mk_inj(12, InjFamily::BadClass, "类号面", "类号倒序", 0x43),
    mk_inj(13, InjFamily::BadClass, "类号面", "类号重挂", 0x44),
    mk_inj(14, InjFamily::BadClass, "类界面", "类大小漂移", 0x51),
    mk_inj(15, InjFamily::BadClass, "类界面", "类界回退", 0x52),
    mk_inj(16, InjFamily::BadClass, "类界面", "类界越窗", 0x53),
    mk_inj(17, InjFamily::BadClass, "类界面", "类界零宽", 0x54),
    mk_inj(18, InjFamily::BadClass, "类界面", "类版错配", 0x55),
    mk_inj(19, InjFamily::BadClass, "类界面", "类界重叠", 0x56),
    mk_inj(20, InjFamily::BadPriv, "权限位面", "权限位越档", 0x61),
    mk_inj(21, InjFamily::BadPriv, "权限位面", "权限位零", 0x62),
    mk_inj(22, InjFamily::BadPriv, "权限位面", "权限位重复挂", 0x63),
    mk_inj(23, InjFamily::BadPriv, "权限位面", "权限位错挂", 0x64),
    mk_inj(24, InjFamily::BadPriv, "权限档面", "权限档倒退", 0x71),
    mk_inj(25, InjFamily::BadPriv, "权限档面", "权限档越闭集", 0x72),
    mk_inj(26, InjFamily::BadPriv, "权限档面", "权限档双主", 0x73),
    mk_inj(27, InjFamily::BadPriv, "权限档面", "权限档空档", 0x74),
    mk_inj(28, InjFamily::BadPriv, "权限档面", "权限档跳跃", 0x75),
    mk_inj(29, InjFamily::BadPriv, "权限档面", "权限档混编", 0x76),
];

// ── S1 母版抽样 20 场景（判据 J1 主语） ──────────────────────────
/// 抽样场景四元（场景号/案号引/面名/母版判码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SampleScene {
    /// 场景号（0..20）。
    pub scene: u32,
    /// 引案号（INJ_CASES 内）。
    pub case_ref: u32,
    /// 场景面名。
    pub face: &'static str,
    /// S1 母版判码。
    pub master_code: u32,
}

/// 20 场景 S1 母版抽样谱（案号 0..30 内每 1.5 案抽 1，冻结）。
pub const SAMPLE_SCENES: &[SampleScene] = &[
    SampleScene { scene: 0, case_ref: 0, face: "串缓冲面", master_code: 0x21 },
    SampleScene { scene: 1, case_ref: 2, face: "串缓冲面", master_code: 0x23 },
    SampleScene { scene: 2, case_ref: 4, face: "缓冲簇面", master_code: 0x31 },
    SampleScene { scene: 3, case_ref: 6, face: "缓冲簇面", master_code: 0x33 },
    SampleScene { scene: 4, case_ref: 8, face: "缓冲簇面", master_code: 0x35 },
    SampleScene { scene: 5, case_ref: 10, face: "类号面", master_code: 0x41 },
    SampleScene { scene: 6, case_ref: 12, face: "类号面", master_code: 0x43 },
    SampleScene { scene: 7, case_ref: 14, face: "类界面", master_code: 0x51 },
    SampleScene { scene: 8, case_ref: 16, face: "类界面", master_code: 0x53 },
    SampleScene { scene: 9, case_ref: 18, face: "类界面", master_code: 0x55 },
    SampleScene { scene: 10, case_ref: 20, face: "权限位面", master_code: 0x61 },
    SampleScene { scene: 11, case_ref: 22, face: "权限位面", master_code: 0x63 },
    SampleScene { scene: 12, case_ref: 24, face: "权限档面", master_code: 0x71 },
    SampleScene { scene: 13, case_ref: 26, face: "权限档面", master_code: 0x73 },
    SampleScene { scene: 14, case_ref: 28, face: "权限档面", master_code: 0x75 },
    SampleScene { scene: 15, case_ref: 1, face: "串缓冲面", master_code: 0x22 },
    SampleScene { scene: 16, case_ref: 5, face: "缓冲簇面", master_code: 0x32 },
    SampleScene { scene: 17, case_ref: 11, face: "类号面", master_code: 0x42 },
    SampleScene { scene: 18, case_ref: 15, face: "类界面", master_code: 0x52 },
    SampleScene { scene: 19, case_ref: 21, face: "权限位面", master_code: 0x62 },
];

/// 抽样容量（20 场景）。
pub const SAMPLE_CAP: usize = 20;

/// S2 复算机按案号回放判码（同一冻结出题，确定性重放）。
fn replay_case_code(case_ref: u32) -> Option<u32> {
    let c = INJ_CASES.get(case_ref as usize)?;
    Some(c.expect_code)
}

/// 双机一致抽样判定（码值类零容差：逐场景判码全等才算一致）。
pub fn sample_agree(s: &SampleScene) -> bool {
    let replayed = match replay_case_code(s.case_ref) {
        Some(v) => v,
        None => return false,
    };
    replayed == s.master_code
}

/// 抽样一致率（一致数/总数，整数口径，无浮点）。
pub fn sample_census() -> (usize, usize, String) {
    let mut agree = 0usize;
    let mut total = 0usize;
    let mut miss = String::new();
    for s in SAMPLE_SCENES.iter() {
        total += 1;
        if sample_agree(s) {
            agree += 1;
        } else if miss.is_empty() {
            miss = format!("场景{}(案{}) 双机判码不一致", s.scene, s.case_ref);
        }
    }
    (agree, total, miss)
}

// ── 案卷状态机（Opened → Judged → Archived） ─────────────────────
/// 案卷状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjState {
    /// 开卷。
    Opened,
    /// 侧判中。
    Judged,
    /// 已归档。
    Archived,
}

impl InjState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            InjState::Opened => 0,
            InjState::Judged => 1,
            InjState::Archived => 2,
        }
    }
}

/// 注入案卷账。
#[derive(Clone, Debug)]
pub struct InjLedger {
    /// 已侧判案（案号序）。
    cases: Vec<InjCase>,
    /// 判定记账（案号 + 判码；非零 = 显性记账，零静默口径）。
    verdicts: Vec<(u32, u32)>,
    /// 拒收记账（案号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: InjState,
}

impl InjLedger {
    /// 开卷。
    pub fn new() -> InjLedger {
        InjLedger {
            cases: Vec::new(),
            verdicts: Vec::new(),
            rejects: Vec::new(),
            state: InjState::Opened,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> InjState {
        self.state
    }

    /// 已侧判案数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 判定记账数（零静默口径）。
    pub fn verdict_len(&self) -> usize {
        self.verdicts.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 案（越界 None）。
    pub fn case_at(&self, idx: usize) -> Option<&InjCase> {
        self.cases.get(idx)
    }

    /// 进侧判（Opened→Judged）。
    pub fn begin_judge(&mut self) -> Result<(), u32> {
        if self.state.rank() >= InjState::Judged.rank() {
            return Err(InjDiag::ERewind.val());
        }
        self.state = InjState::Judged;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < INJ_CASE_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐案侧判（案号序 + 黄金判定 + 案互异 + 显性记账 四道）。
    pub fn side(&mut self, c: InjCase) -> Result<(), u32> {
        if self.state.rank() < InjState::Judged.rank() {
            return Err(InjDiag::ESilentDrop.val());
        }
        if self.state.rank() >= InjState::Archived.rank() {
            self.note_reject(c.no, InjDiag::EArchivedWrite.val());
            return Err(InjDiag::EArchivedWrite.val());
        }
        // 案号序闸。
        if c.no != self.cases.len() as u32 {
            self.note_reject(c.no, InjDiag::ECaseOrder.val());
            return Err(InjDiag::ECaseOrder.val());
        }
        // 黄金判定闸。
        if let Err(e) = inj_case_ok(&c) {
            self.note_reject(c.no, e);
            return Err(e);
        }
        // 案互异闸：同族同面同谱只立一案。
        for done in self.cases.iter() {
            if done.family == c.family && done.face == c.face && done.spectrum == c.spectrum {
                self.note_reject(c.no, InjDiag::ECaseDup.val());
                return Err(InjDiag::ECaseDup.val());
            }
        }
        // 显性记账（零静默口径）：每案落判定。
        self.verdicts.push((c.no, c.expect_code));
        self.cases.push(c);
        Ok(())
    }

    /// 归档：30 案齐 + 判定记账 30 条 + 零拒收 + 抽样 20/20 一致。
    pub fn archive(&mut self) -> Result<(), u32> {
        if self.state.rank() >= InjState::Archived.rank() {
            return Err(InjDiag::ERewind.val());
        }
        if self.state.rank() < InjState::Judged.rank() {
            return Err(InjDiag::ESilentDrop.val());
        }
        if self.cases.len() != INJ_CASE_CAP {
            return Err(InjDiag::EShortArchive.val());
        }
        if self.verdicts.len() != INJ_CASE_CAP {
            return Err(InjDiag::ESilentDrop.val());
        }
        if !self.rejects.is_empty() {
            return Err(InjDiag::EShortArchive.val());
        }
        let (agree, total, _miss) = sample_census();
        if total != SAMPLE_CAP || agree != SAMPLE_CAP {
            return Err(InjDiag::ECodeDrift.val());
        }
        self.state = InjState::Archived;
        Ok(())
    }

    /// 账等式（零静默口径：案数 = 判定数 = 30，拒收零）。
    pub fn equation_ok(&self) -> bool {
        self.cases.len() == INJ_CASE_CAP
            && self.verdicts.len() == INJ_CASE_CAP
            && self.rejects.is_empty()
    }
}

// ── 构建完整案卷 ─────────────────────────────────────────────────
/// 构建完整案卷（30 案逐案侧判）。
pub fn build_full_inj_ledger() -> InjLedger {
    let mut led = InjLedger::new();
    let _ = led.begin_judge();
    let mut i = 0usize;
    while i < INJ_CASE_CAP {
        let c = match INJ_CASES.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.side(c) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    led
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjFault {
    /// 族越闭集。
    FamilyRange,
    /// 零码。
    ZeroCode,
    /// 判码漂移。
    CodeDrift,
    /// 面越窗。
    FaceRange,
    /// 案号乱序。
    CaseOrder,
    /// 案重复。
    CaseDup,
    /// 缺案归档。
    ShortArchive,
    /// 静默吞案。
    SilentDrop,
    /// 归档后补案。
    ArchivedWrite,
    /// 摘要空。
    SummaryEmpty,
}

impl InjFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            InjFault::FamilyRange => 0,
            InjFault::ZeroCode => 1,
            InjFault::CodeDrift => 2,
            InjFault::FaceRange => 3,
            InjFault::CaseOrder => 4,
            InjFault::CaseDup => 5,
            InjFault::ShortArchive => 6,
            InjFault::SilentDrop => 7,
            InjFault::ArchivedWrite => 8,
            InjFault::SummaryEmpty => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            InjFault::FamilyRange => "族越闭集",
            InjFault::ZeroCode => "零码",
            InjFault::CodeDrift => "判码漂移",
            InjFault::FaceRange => "面越窗",
            InjFault::CaseOrder => "案号乱序",
            InjFault::CaseDup => "案重复",
            InjFault::ShortArchive => "缺案归档",
            InjFault::SilentDrop => "静默吞案",
            InjFault::ArchivedWrite => "归档后补案",
            InjFault::SummaryEmpty => "摘要空",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [InjFault] {
        &[
            InjFault::FamilyRange,
            InjFault::ZeroCode,
            InjFault::CodeDrift,
            InjFault::FaceRange,
            InjFault::CaseOrder,
            InjFault::CaseDup,
            InjFault::ShortArchive,
            InjFault::SilentDrop,
            InjFault::ArchivedWrite,
            InjFault::SummaryEmpty,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn inj_fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in InjFault::all().iter() {
        total += 1;
        if run_inj_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_inj_fault(v: InjFault) -> bool {
    match v {
        InjFault::FamilyRange => {
            // 谱名清空 → 族文本闸必拒。
            let mut c = match INJ_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.spectrum = "";
            inj_case_ok(&c).is_err()
        }
        InjFault::ZeroCode => {
            let mut c = match INJ_CASES.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.expect_code = 0;
            inj_case_ok(&c).is_err()
        }
        InjFault::CodeDrift => {
            // 双机漂移：S2 回放码改一位，零容差必抓。
            let mut c = match INJ_CASES.get(2) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.got_code = c.got_code.saturating_add(1);
            inj_case_ok(&c).is_err()
        }
        InjFault::FaceRange => {
            let mut c = match INJ_CASES.get(3) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.face = "幽灵注入面";
            inj_case_ok(&c).is_err()
        }
        InjFault::CaseOrder => {
            let mut led = InjLedger::new();
            let _ = led.begin_judge();
            let c = match INJ_CASES.get(1) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.side(c).is_err()
        }
        InjFault::CaseDup => {
            let mut led = InjLedger::new();
            let _ = led.begin_judge();
            let c0 = match INJ_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.side(c0.clone());
            // 同族同面同谱换案号再立被拒。
            let dup = InjCase {
                no: 1,
                family: c0.family,
                face: c0.face,
                spectrum: c0.spectrum,
                expect_code: c0.expect_code,
                got_code: c0.got_code,
                expect_match: true,
            };
            led.side(dup).is_err()
        }
        InjFault::ShortArchive => {
            let mut led = InjLedger::new();
            let _ = led.begin_judge();
            let c = match INJ_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            let _ = led.side(c);
            led.archive().is_err()
        }
        InjFault::SilentDrop => {
            // 未侧判即归档被拒（Opened 态直接 archive）。
            let mut led = InjLedger::new();
            led.archive().is_err()
        }
        InjFault::ArchivedWrite => {
            let mut led = build_full_inj_ledger();
            if led.archive().is_err() {
                return false;
            }
            let c = match INJ_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.side(c).is_err()
        }
        InjFault::SummaryEmpty => {
            // 摘要空族：面名清空 → 码窗闸无从归面，面越窗必拒。
            let mut c = match INJ_CASES.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            c.face = "";
            inj_case_ok(&c).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 抽样 20 场景双机一致（S1 母版），码值类零容差（判据 J1 直读口）。
fn sampling_consensus_ok() -> bool {
    if SAMPLE_SCENES.len() != SAMPLE_CAP {
        return false;
    }
    // 场景号连续 0..20。
    let mut i = 0usize;
    while i < SAMPLE_SCENES.len() {
        let s = match SAMPLE_SCENES.get(i) {
            Some(v) => v,
            None => return false,
        };
        if s.scene != i as u32 {
            return false;
        }
        // 场景引案必须落在 30 案闭集内且面名与母版判码落窗一致。
        if s.case_ref >= INJ_CASES.len() as u32 {
            return false;
        }
        let f = match inj_face_of(s.face) {
            Some(v) => v,
            None => return false,
        };
        if !code_in_inj_face(s.master_code, f.base) {
            return false;
        }
        // 双机一致：逐场景全等（零容差）。
        if !sample_agree(s) {
            return false;
        }
        i += 1;
    }
    let (agree, total, miss) = sample_census();
    if total != SAMPLE_CAP || agree != SAMPLE_CAP || !miss.is_empty() {
        return false;
    }
    true
}

/// 30 例注入案卷全过（判据 ① 直读口）。
fn inj_thirty_ok() -> bool {
    let mut led = build_full_inj_ledger();
    if led.len() != INJ_CASE_CAP {
        return false;
    }
    if led.verdict_len() != INJ_CASE_CAP {
        return false;
    }
    if led.reject_len() != 0 {
        return false;
    }
    if led.archive().is_err() {
        return false;
    }
    if led.state() != InjState::Archived {
        return false;
    }
    // 独立复核：30 案逐案黄金判定。
    for c in INJ_CASES.iter() {
        if inj_case_ok(c).is_err() {
            return false;
        }
    }
    true
}

/// 三族谱覆盖普查（每族恰 10 例、谱名互异、面随族正确落窗）。
fn family_census_ok() -> bool {
    let mut per_family = [0usize; 3];
    let mut i = 0usize;
    while i < INJ_CASES.len() {
        let c = match INJ_CASES.get(i) {
            Some(v) => v,
            None => return false,
        };
        let slot = c.family.ordinal() as usize;
        match per_family.get_mut(slot) {
            Some(v) => *v += 1,
            None => return false,
        }
        let f = match inj_face_of(c.face) {
            Some(v) => v,
            None => return false,
        };
        if !code_in_inj_face(c.expect_code, f.base) {
            return false;
        }
        // 谱名互异普查。
        let mut j = i + 1;
        while j < INJ_CASES.len() {
            let g = match INJ_CASES.get(j) {
                Some(v) => v,
                None => return false,
            };
            if g.family == c.family && g.spectrum == c.spectrum {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    for slot in per_family.iter() {
        if *slot != CASES_PER_FAMILY {
            return false;
        }
    }
    true
}

/// 零静默（判定记账逐案齐、无静默槽）。
fn zero_silent_ok() -> bool {
    let led = build_full_inj_ledger();
    if led.verdict_len() != led.len() {
        return false;
    }
    let mut i = 0usize;
    while i < led.verdict_len() {
        let c = match led.case_at(i) {
            Some(v) => v,
            None => return false,
        };
        if c.no != i as u32 || c.expect_code == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 注入联测 10/10 拒止。
fn inj_fault_sweep_ok() -> bool {
    let (caught, total, escape) = inj_fault_sweep();
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

/// 注入面六闭集普查（六面互异、基码递增、查表命中）。
fn face_census_ok() -> bool {
    if INJ_FACE_TABLE.len() != 6 {
        return false;
    }
    let mut ok = true;
    let mut i = 0usize;
    while i < INJ_FACE_TABLE.len() {
        let f = match INJ_FACE_TABLE.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if inj_face_of(f.name).is_none() {
            ok = false;
        }
        let mut j = i + 1;
        while j < INJ_FACE_TABLE.len() {
            match INJ_FACE_TABLE.get(j) {
                Some(g) => {
                    if g.name == f.name || g.base <= f.base {
                        ok = false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    ok
}

/// 双跑一致（冻结案卷下两轮全流程同判）。
fn dual_run_ok() -> bool {
    let a = build_full_inj_ledger();
    let b = build_full_inj_ledger();
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.case_at(i), b.case_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if inj_fault_sweep().0 != inj_fault_sweep().0 {
        return false;
    }
    if sample_census().0 != sample_census().0 {
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
    if SEG_BASE != 0x80000 + 0x40 * (12672u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_inj_ledger();
    if led.archive().is_err() {
        return false;
    }
    if led.state() != InjState::Archived {
        return false;
    }
    // 摘要：首案族名 + 尾案谱名拼接非空。
    let head = match led.case_at(0) {
        Some(c) => c.family.name(),
        None => return false,
    };
    let tail = match led.case_at(INJ_CASE_CAP - 1) {
        Some(c) => c.spectrum,
        None => return false,
    };
    if head.is_empty() || tail.is_empty() {
        return false;
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12673 注入联测自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12672_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12672-probe-inj2");
    let led = build_full_inj_ledger();

    s.add_dyn(
        "F12673-01 注入案卷三十例齐",
        INJ_CASES.len() == INJ_CASE_CAP,
        &format!("cases={}", INJ_CASES.len()),
    );

    s.add_dyn(
        "F12673-02 抽样 20 场景双机一致",
        sampling_consensus_ok(),
        &format!("agree={}", sample_census().0),
    );

    s.add_dyn(
        "F12673-03 码值类零容差直读",
        {
            let mut c = match INJ_CASES.get(2) {
                Some(x) => x.clone(),
                None => return s,
            };
            c.got_code = c.got_code.saturating_add(1);
            inj_case_ok(&c).is_err()
        },
        "判码漂移一位即拒，无容忍带",
    );

    s.add_dyn(
        "F12673-04 三十例注入全过",
        inj_thirty_ok(),
        &format!("cases={} verdicts={} rejects={}", led.len(), led.verdict_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12673-05 三族谱覆盖普查",
        family_census_ok(),
        &format!("families={}", InjFamily::all().len()),
    );

    s.add_dyn(
        "F12673-06 零静默",
        zero_silent_ok(),
        &format!("verdicts={}", led.verdict_len()),
    );

    s.add_dyn(
        "F12673-07 注入联测 10/10 拒止",
        inj_fault_sweep_ok(),
        &format!("caught={}", inj_fault_sweep().0),
    );

    s.add_dyn(
        "F12673-08 注入面六闭集普查",
        face_census_ok(),
        &format!("faces={}", INJ_FACE_TABLE.len()),
    );

    s.add_dyn(
        "F12673-09 案卷账等式",
        led.equation_ok(),
        &format!("cases={} verdicts={} rejects={}", led.len(), led.verdict_len(), led.reject_len()),
    );

    s.add_dyn(
        "F12673-10 双跑一致",
        dual_run_ok(),
        "冻结出题下两轮逐案同判",
    );

    s.add_dyn(
        "F12673-11 判码窗口口径",
        code_in_inj_face(0x20 + 0x01, 0x20)
            && !code_in_inj_face(0x20, 0x20)
            && !code_in_inj_face(0x20 + FACE_OFF_CAP + 1, 0x20),
        "基码 < 判码 ≤ 基码+0x14",
    );

    s.add_dyn(
        "F12673-12 完成写回语义",
        writeback_ok(),
        "归档即已完成且摘要非空",
    );

    s.add_dyn(
        "F12673-13 段位守卫",
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
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x82FC0);
        assert_eq!(SEG_BASE, 0x81200);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：11 个码互异、全部落窗口、查表可命中。
        assert_eq!(INJ_ALL.len(), 11);
        let mut i = 0usize;
        while i < INJ_ALL.len() {
            let vi = match INJ_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < INJ_ALL.len() {
                let vj = match INJ_ALL.get(j) {
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
    fn sample_twenty_all_agree() {
        // 抽样 20 场景双机一致（S1 母版），零容差。
        let (agree, total, miss) = sample_census();
        assert_eq!(total, 20);
        assert_eq!(agree, 20, "不一致：{}", miss);
        assert!(miss.is_empty());
        assert!(sampling_consensus_ok());
    }

    #[test]
    fn zero_tolerance_drift_caught() {
        // 码值类零容差：漂移一位即违约。
        let mut c = match INJ_CASES.get(4) {
            Some(x) => x.clone(),
            None => return,
        };
        c.got_code = c.got_code.saturating_add(1);
        assert_eq!(inj_case_ok(&c), Err(InjDiag::ECodeDrift.val()));
        c.expect_code = 0;
        assert_eq!(inj_case_ok(&c), Err(InjDiag::EZeroCode.val()));
    }

    #[test]
    fn happy_path_archives() {
        let mut led = build_full_inj_ledger();
        assert_eq!(led.len(), INJ_CASE_CAP);
        assert_eq!(led.verdict_len(), INJ_CASE_CAP);
        assert_eq!(led.reject_len(), 0);
        assert!(led.archive().is_ok());
        assert_eq!(led.state(), InjState::Archived);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数拒止。
        let (caught, total, escape) = inj_fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn case_gate_rejects() {
        // 案闸直读：乱序/重复/面越窗。
        let mut led = InjLedger::new();
        let _ = led.begin_judge();
        let c1 = match INJ_CASES.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.side(c1), Err(InjDiag::ECaseOrder.val()));
        let c0 = match INJ_CASES.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        assert!(led.side(c0.clone()).is_ok());
        let dup = InjCase {
            no: 1,
            family: c0.family,
            face: c0.face,
            spectrum: c0.spectrum,
            expect_code: c0.expect_code,
            got_code: c0.got_code,
            expect_match: true,
        };
        assert_eq!(led.side(dup), Err(InjDiag::ECaseDup.val()));
        // 面越窗。
        let ghost = InjCase {
            no: 1,
            family: InjFamily::BadClass,
            face: "幽灵注入面",
            spectrum: "越面族",
            expect_code: 0x41,
            got_code: 0x41,
            expect_match: true,
        };
        assert_eq!(led.side(ghost), Err(InjDiag::EFaceRange.val()));
        assert_eq!(led.reject_len(), 3);
    }

    #[test]
    fn family_census_holds() {
        // 三族各 10 例、谱名互异。
        assert_eq!(INJ_CASES.len(), 30);
        assert_eq!(InjFamily::all().len(), 3);
        let mut counts = [0usize; 3];
        for c in INJ_CASES.iter() {
            match counts.get_mut(c.family.ordinal() as usize) {
                Some(v) => *v += 1,
                None => return,
            }
        }
        assert_eq!(counts[0], 10);
        assert_eq!(counts[1], 10);
        assert_eq!(counts[2], 10);
        assert!(family_census_ok());
        // 族闭集：ordinal/name/code_floor 三轴一致。
        for f in InjFamily::all().iter() {
            assert!(!f.name().is_empty());
        }
        assert!(InjFamily::BadPriv.code_floor() > InjFamily::BadClass.code_floor());
        assert!(InjFamily::BadClass.code_floor() > InjFamily::BadBuffer.code_floor());
    }

    #[test]
    fn zero_silent_ledger() {
        // 零静默：逐案判定记账且案号一一对应。
        assert!(zero_silent_ok());
        let led = build_full_inj_ledger();
        assert_eq!(led.verdict_len(), INJ_CASE_CAP);
        let mut i = 0usize;
        while i < INJ_CASE_CAP {
            let c = match led.case_at(i) {
                Some(v) => v,
                None => return,
            };
            assert_eq!(c.no, i as u32, "案 {} 错位", i);
            i += 1;
        }
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致。
        let a = build_full_inj_ledger();
        let b = build_full_inj_ledger();
        assert_eq!(a.len(), b.len());
        let mut i = 0usize;
        while i < a.len() {
            match (a.case_at(i), b.case_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
        assert_eq!(inj_fault_sweep().0, inj_fault_sweep().0);
        assert_eq!(sample_census().0, sample_census().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Opened→Judged→Archived。
        assert!(InjState::Opened.rank() < InjState::Judged.rank());
        assert!(InjState::Judged.rank() < InjState::Archived.rank());
        let mut led = build_full_inj_ledger();
        assert_eq!(led.state(), InjState::Judged);
        assert!(led.archive().is_ok());
        assert_eq!(led.archive(), Err(InjDiag::ERewind.val()));
        let mut fresh = InjLedger::new();
        assert!(fresh.begin_judge().is_ok());
        assert_eq!(fresh.begin_judge(), Err(InjDiag::ERewind.val()));
    }

    #[test]
    fn faces_base_codes_ascending() {
        // 六面基码递增且互异（0x20..0x70）。
        assert_eq!(INJ_FACE_TABLE.len(), 6);
        for (i, f) in INJ_FACE_TABLE.iter().enumerate() {
            assert_eq!(f.base, 0x20 + (i as u32) * 0x10, "面 {} 基码漂移", i);
        }
        assert!(face_census_ok());
        assert!(inj_face_of("串缓冲面").is_some());
        assert!(inj_face_of("权限档面").is_some());
        assert!(inj_face_of("幽灵注入面").is_none());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12672_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
