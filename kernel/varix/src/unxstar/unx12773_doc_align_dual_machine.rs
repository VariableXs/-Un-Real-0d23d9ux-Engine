//! UNX-F12773 文档对齐双机抽验 · 0x82B40段
//!
//! 判据原文（派工卡 12773 逐字锚定）：
//!   UNX-F12774-J1 20 条单跑入口齐，全跑条数与聚合账等式断言随批归档。
//!
//! 设计要点：
//! 1. 双机抽验域模型：两台虚拟机视图（机 A / 机 B）各自持行为账
//!    （场景号 + 行为结果原始终码），行为结果闭集 Agree|Conflict|Unknown
//!    三值冻结，越出闭集的原始终码在判定器入口显性拒（语义 EINVAL）；
//! 2. 10 场景冻结抽验表：场景号/场景名/文档锚点三元组冻结，表外场景
//!    一律 ENOENT 显性拒；每场景每机只许登记一笔，重复登记显性拒；
//! 3. 一致判定器 judge_agree(scene_no)：状态闸→封账闸→场景闸→重复闸
//!    →机 A 闭集闸→机 B 闭集闸→语义裁决，六道次序闸固定；
//!    A=B→Agree，A≠B→显式 Conflict（冲突必须入账，不许静默吞并），
//!    两边皆 Unknown→ETENTATIVE 语义码（既非一致也非冲突，跑次仍入判定账）；
//! 4. CONFLICT 显式账：冲突行登记（场景号/机 A 码/机 B 码/复验旗标），
//!    冲突覆盖率可从双机行为账独立复算——静默吞并的缺陷判定器复算必不平；
//!    复验旗标未置齐不许封账；
//! 5. 三类注入器（场景未登记/结果越闭集/冲突静默）各 10 轮，人为构造
//!    违约必被对应闸或复算抓住，逃脱即判据失效；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子固定 =
//!    SEG_BASE）；取值一律 match/if let/.get()，无 unwrap/expec/panic 类出口。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12773 − 12600) = 0x80000 + 0x2B40）。
pub const SEG_BASE: u32 = 0x82B40;

// ── 双机抽验诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0D 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmDiag {
    /// 抽验表未冻结（或 10 场景未全跑）就判定/封账。
    ENotFrozen,
    /// 未登记场景：表外场景号（语义 ENOENT）。
    ESceneNoent,
    /// 行为结果越闭集：原始终码不在 Agree|Conflict|Unknown 三值内（语义 EINVAL）。
    EOutcomeRange,
    /// 两边皆未知：语义待定（语义 ETENTATIVE，非硬失败）。
    ETentative,
    /// 冲突静默：判定器吞并冲突未入显式账，被覆盖率复算检出。
    EConflictSilent,
    /// 冲突覆盖率复算不平：显式账与双机行为账重推闭集不等式。
    ECoverage,
    /// 复验旗标未置齐就试图封账。
    ERecheck,
    /// 场景重复登记：同一场景同一机落了第二笔。
    ESceneDup,
    /// 场景重复判定：冻结抽验每场景只判一次。
    ERejudge,
    /// 机视图容量耗尽：登记笔数已达冻结表容量。
    ECapacity,
    /// 抽验账已封账：封账后写入/判定被拒。
    ESealed,
    /// 状态机倒退：重冻结/重封账等回退动作被拒。
    ERewind,
    /// 双跑指纹不平：两次全流程指纹不等，确定性被破坏。
    EFingerprint,
}

impl DmDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0D 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                DmDiag::ENotFrozen => 0x01,
                DmDiag::ESceneNoent => 0x02,
                DmDiag::EOutcomeRange => 0x03,
                DmDiag::ETentative => 0x04,
                DmDiag::EConflictSilent => 0x05,
                DmDiag::ECoverage => 0x06,
                DmDiag::ERecheck => 0x07,
                DmDiag::ESceneDup => 0x08,
                DmDiag::ERejudge => 0x09,
                DmDiag::ECapacity => 0x0a,
                DmDiag::ESealed => 0x0b,
                DmDiag::ERewind => 0x0c,
                DmDiag::EFingerprint => 0x0d,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            DmDiag::ENotFrozen => "抽验未冻结或 10 场景未全跑：先把双机账登记满并 freeze 冻结，跑满 10 场景后再判定/封账",
            DmDiag::ESceneNoent => "未登记场景：场景号不在 10 场景冻结抽验表内（语义 ENOENT），先对表再判定",
            DmDiag::EOutcomeRange => "行为结果越闭集：原始终码不在 Agree|Conflict|Unknown 三值内（语义 EINVAL），回机视图改用 observe 正常面登记",
            DmDiag::ETentative => "两边皆未知：语义待定（语义 ETENTATIVE），不计一致不计冲突，补跑取证后重登抽验批",
            DmDiag::EConflictSilent => "冲突静默：判定器吞并冲突未入显式账，按复算缺口场景补登冲突行并整批复核判定器",
            DmDiag::ECoverage => "冲突覆盖率复算不平：显式账与双机行为账重推闭集不等式，逐场景对账补齐或剔除多登行",
            DmDiag::ERecheck => "复验旗标未置齐：每个冲突行必须复验一次置旗，先跑复验再封账",
            DmDiag::ESceneDup => "场景重复登记：每场景每机只许一笔，重复笔先查已有行号再落账",
            DmDiag::ERejudge => "场景重复判定：冻结抽验每场景只判一次，重判走新抽验批滚动",
            DmDiag::ECapacity => "机视图容量耗尽：登记笔数已达冻结表容量 10，新样本走下一条抽验批",
            DmDiag::ESealed => "抽验账已封账：封账后写入/判定拒绝，改动走新抽验批",
            DmDiag::ERewind => "状态机倒退：收集→冻结→封账只进不退，倒退即抽验账可疑",
            DmDiag::EFingerprint => "双跑指纹不平：两次全流程指纹不等，先查语料与 LCG 种子是否被改动",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const DM_ALL: &[DmDiag] = &[
    DmDiag::ENotFrozen,
    DmDiag::ESceneNoent,
    DmDiag::EOutcomeRange,
    DmDiag::ETentative,
    DmDiag::EConflictSilent,
    DmDiag::ECoverage,
    DmDiag::ERecheck,
    DmDiag::ESceneDup,
    DmDiag::ERejudge,
    DmDiag::ECapacity,
    DmDiag::ESealed,
    DmDiag::ERewind,
    DmDiag::EFingerprint,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in DM_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82B41..0x82B4D 闭集内，先对段基址 0x82B40 核码"
}

// ── 行为结果闭集 ─────────────────────────────────────────────────
/// 行为结果闭集原始终码（冻结：1=一致 2=冲突 3=未知；来源：验收手册§F12773 三值口径）。
pub const OUTCOME_RAW_AGREE: u32 = 1;
/// 冲突原始终码。
pub const OUTCOME_RAW_CONFLICT: u32 = 2;
/// 未知原始终码。
pub const OUTCOME_RAW_UNKNOWN: u32 = 3;

/// 行为结果三值闭集（每台虚拟机对单场景的独立行为归类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// 一致：该机行为与文档锚点判据行相符。
    Agree,
    /// 冲突：该机行为与文档锚点判据行不符。
    Conflict,
    /// 未知：该机行为无法归类（取证不足）。
    Unknown,
}

impl Outcome {
    /// 原始终码。
    pub fn raw(self) -> u32 {
        match self {
            Outcome::Agree => OUTCOME_RAW_AGREE,
            Outcome::Conflict => OUTCOME_RAW_CONFLICT,
            Outcome::Unknown => OUTCOME_RAW_UNKNOWN,
        }
    }

    /// 由原始终码解析（越闭集返回 None）。
    pub fn from_raw(v: u32) -> Option<Outcome> {
        match v {
            OUTCOME_RAW_AGREE => Some(Outcome::Agree),
            OUTCOME_RAW_CONFLICT => Some(Outcome::Conflict),
            OUTCOME_RAW_UNKNOWN => Some(Outcome::Unknown),
            _ => None,
        }
    }

    /// 中文名（诊断文本用）。
    pub fn name(self) -> &'static str {
        match self {
            Outcome::Agree => "一致",
            Outcome::Conflict => "冲突",
            Outcome::Unknown => "未知",
        }
    }
}

/// 行为结果闭集数组（闭集巡检用）。
pub const OUTCOME_ALL: &[Outcome] = &[Outcome::Agree, Outcome::Conflict, Outcome::Unknown];

// ── 10 场景冻结抽验表 ────────────────────────────────────────────
/// 抽验表容量：10 场景齐即满（判据 J1 抽验批冻结口径）。
pub const SCENE_CAP: usize = 10;

/// 场景规格：场景号 + 场景名 + 文档锚点（锚点指向验收手册判据行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SceneSpec {
    /// 场景号（1..=10，冻结不许重排）。
    pub no: u32,
    /// 场景名。
    pub name: &'static str,
    /// 文档锚点（手册章节 + 判据行号）。
    pub anchor: &'static str,
}

/// 10 场景冻结抽验表（表序即判定序，冻结不许重排；来源：验收手册§4 行为抽验章）。
pub const SCENE_TABLE: &[SceneSpec] = &[
    SceneSpec { no: 1, name: "页表切换行为", anchor: "手册§4.1 判据行 J-401" },
    SceneSpec { no: 2, name: "中断应答行为", anchor: "手册§4.2 判据行 J-402" },
    SceneSpec { no: 3, name: "系统调用入口行为", anchor: "手册§4.3 判据行 J-403" },
    SceneSpec { no: 4, name: "调度抢占行为", anchor: "手册§4.4 判据行 J-404" },
    SceneSpec { no: 5, name: "内存映射行为", anchor: "手册§4.5 判据行 J-405" },
    SceneSpec { no: 6, name: "时钟节拍行为", anchor: "手册§4.6 判据行 J-406" },
    SceneSpec { no: 7, name: "设备探针行为", anchor: "手册§4.7 判据行 J-407" },
    SceneSpec { no: 8, name: "缓存回写行为", anchor: "手册§4.8 判据行 J-408" },
    SceneSpec { no: 9, name: "异常分发行为", anchor: "手册§4.9 判据行 J-409" },
    SceneSpec { no: 10, name: "关机序列行为", anchor: "手册§4.10 判据行 J-410" },
];

/// 按场景号查冻结表（表外返回 None）。
pub fn scene_of(no: u32) -> Option<&'static SceneSpec> {
    for sp in SCENE_TABLE.iter() {
        if sp.no == no {
            return Some(sp);
        }
    }
    None
}

// ── 机器标识与单机行为账 ─────────────────────────────────────────
/// 机器标识：机 A / 机 B（两台独立虚拟机视图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineId {
    /// 机 A。
    A,
    /// 机 B。
    B,
}

impl MachineId {
    /// 机名（诊断文本用）。
    pub fn name(self) -> &'static str {
        match self {
            MachineId::A => "机A",
            MachineId::B => "机B",
        }
    }
}

/// 单机单场景行为账行：场景号 + 行为结果原始终码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BehRow {
    /// 场景号（必须落在冻结抽验表内）。
    pub scene_no: u32,
    /// 行为结果原始终码（判定器入口复核闭集）。
    pub raw: u32,
}

/// 单台虚拟机视图：持自己的行为账（最多 SCENE_CAP 笔）。
#[derive(Clone, Debug)]
pub struct MachineView {
    /// 行为账行（按登记序）。
    rows: Vec<BehRow>,
}

impl MachineView {
    /// 开一台空视图。
    pub fn new() -> MachineView {
        MachineView { rows: Vec::new() }
    }

    /// 已登记笔数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 按场景号查行。
    pub fn row_of(&self, scene_no: u32) -> Option<BehRow> {
        for r in self.rows.iter() {
            if r.scene_no == scene_no {
                return Some(*r);
            }
        }
        None
    }

    /// 正常登记面：容量→场景→重复→闭集 四道闸，全过才落账。
    pub fn observe(&mut self, scene_no: u32, raw: u32) -> Result<(), u32> {
        // 闸1 容量闸：登记笔数达冻结表容量即耗尽。
        if self.rows.len() >= SCENE_CAP {
            return Err(DmDiag::ECapacity.val());
        }
        // 闸2 场景闸：表外场景不收。
        if scene_of(scene_no).is_none() {
            return Err(DmDiag::ESceneNoent.val());
        }
        // 闸3 重复闸：每场景每机只许一笔。
        if self.row_of(scene_no).is_some() {
            return Err(DmDiag::ESceneDup.val());
        }
        // 闸4 闭集闸：行为结果必须落在三值闭集内。
        if Outcome::from_raw(raw).is_none() {
            return Err(DmDiag::EOutcomeRange.val());
        }
        self.rows.push(BehRow { scene_no, raw });
        Ok(())
    }

    /// 注入面：绕过容量闸与闭集闸落/改原始终码（仅供注入器构造违约样本）。
    /// 已登记场景为改写语义（把已入账行为结果篡改为越闭集值）。
    pub fn plant_raw(&mut self, scene_no: u32, raw: u32) -> Result<(), u32> {
        if scene_of(scene_no).is_none() {
            return Err(DmDiag::ESceneNoent.val());
        }
        // 已登记场景：改写原始终码（注入改写语义）。
        if let Some(row) = self.rows.iter_mut().find(|r| r.scene_no == scene_no) {
            row.raw = raw;
            return Ok(());
        }
        // 未登记场景：新落一笔（不验闭集，不验容量）。
        self.rows.push(BehRow { scene_no, raw });
        Ok(())
    }
}

// ── 双机抽验账状态机与判定器 ─────────────────────────────────────
/// 抽验账状态机：收集 → 冻结 → 封账，单向推进，倒退显性拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmState {
    /// 收集：双机登记行为账。
    Collecting,
    /// 冻结：双机 10 场景齐且原始终码全在闭集，允许判定。
    Armed,
    /// 封账：只读。
    Sealed,
}

impl DmState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            DmState::Collecting => 0,
            DmState::Armed => 1,
            DmState::Sealed => 2,
        }
    }
}

/// 一致裁决词：Agree（账面一致）/ Conflict（显式冲突）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 一致：双机行为归类相同。
    Agree,
    /// 冲突：双机行为归类不同（必须入显式账）。
    Conflict,
}

impl Verdict {
    /// 裁决词中文名（诊断文本用）。
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Agree => "一致",
            Verdict::Conflict => "冲突",
        }
    }
}

/// 冲突显式账行：场景号 + 机 A 原始终码 + 机 B 原始终码 + 复验旗标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConflictRow {
    /// 冲突场景号。
    pub scene_no: u32,
    /// 机 A 原始终码。
    pub a_raw: u32,
    /// 机 B 原始终码。
    pub b_raw: u32,
    /// 复验旗标：冲突行必须复验一次置旗，未置齐不许封账。
    pub recheck: bool,
}

/// 双机抽验账：两台虚拟机视图 + 状态机 + 冲突显式账 + 判定次序账。
#[derive(Clone, Debug)]
pub struct DualJudge {
    /// 机 A 行为账。
    a: MachineView,
    /// 机 B 行为账。
    b: MachineView,
    /// 抽验账状态。
    state: DmState,
    /// 冲突显式账（判定器显式登记，不许静默吞并）。
    conflicts: Vec<ConflictRow>,
    /// 判定次序账：已判定场景号（每场景至多一笔，含语义待定场景）。
    judged: Vec<u32>,
}

impl DualJudge {
    /// 开一本空抽验账。
    pub fn new() -> DualJudge {
        DualJudge {
            a: MachineView::new(),
            b: MachineView::new(),
            state: DmState::Collecting,
            conflicts: Vec::new(),
            judged: Vec::new(),
        }
    }

    /// 当前状态。
    pub fn state(&self) -> DmState {
        self.state
    }

    /// 指定机视图已登记笔数。
    pub fn view_len(&self, id: MachineId) -> usize {
        match id {
            MachineId::A => self.a.len(),
            MachineId::B => self.b.len(),
        }
    }

    /// 按机标识与场景号查行为账行。
    pub fn row_of(&self, id: MachineId, scene_no: u32) -> Option<BehRow> {
        match id {
            MachineId::A => self.a.row_of(scene_no),
            MachineId::B => self.b.row_of(scene_no),
        }
    }

    /// 机 A 正常登记。
    pub fn observe_a(&mut self, scene_no: u32, raw: u32) -> Result<(), u32> {
        if self.state.rank() >= DmState::Sealed.rank() {
            return Err(DmDiag::ESealed.val());
        }
        self.a.observe(scene_no, raw)
    }

    /// 机 B 正常登记。
    pub fn observe_b(&mut self, scene_no: u32, raw: u32) -> Result<(), u32> {
        if self.state.rank() >= DmState::Sealed.rank() {
            return Err(DmDiag::ESealed.val());
        }
        self.b.observe(scene_no, raw)
    }

    /// 机 A 注入面（绕过容量/闭集闸，仅供注入器）。
    pub fn plant_a(&mut self, scene_no: u32, raw: u32) -> Result<(), u32> {
        if self.state.rank() >= DmState::Sealed.rank() {
            return Err(DmDiag::ESealed.val());
        }
        self.a.plant_raw(scene_no, raw)
    }

    /// 机 B 注入面（绕过容量/闭集闸，仅供注入器）。
    pub fn plant_b(&mut self, scene_no: u32, raw: u32) -> Result<(), u32> {
        if self.state.rank() >= DmState::Sealed.rank() {
            return Err(DmDiag::ESealed.val());
        }
        self.b.plant_raw(scene_no, raw)
    }

    /// 指定机视图是否就绪：10 场景齐且原始终码全在闭集。
    pub fn view_ready(&self, id: MachineId) -> bool {
        for sp in SCENE_TABLE.iter() {
            let r = match self.row_of(id, sp.no) {
                Some(r) => r,
                None => return false,
            };
            if Outcome::from_raw(r.raw).is_none() {
                return false;
            }
        }
        true
    }

    /// 冻结抽验表：双机 10 场景齐才许冻结；重复冻结倒退拒。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.state.rank() >= DmState::Armed.rank() {
            return Err(DmDiag::ERewind.val());
        }
        if !self.view_ready(MachineId::A) || !self.view_ready(MachineId::B) {
            return Err(DmDiag::ENotFrozen.val());
        }
        self.state = DmState::Armed;
        Ok(())
    }

    /// 冲突显式账行数。
    pub fn conflict_len(&self) -> usize {
        self.conflicts.len()
    }

    /// 按场景号查冲突行。
    pub fn conflict_of(&self, scene_no: u32) -> Option<ConflictRow> {
        for r in self.conflicts.iter() {
            if r.scene_no == scene_no {
                return Some(*r);
            }
        }
        None
    }

    /// 按账内序号取冲突行（越界 None）。
    pub fn conflict_at(&self, idx: usize) -> Option<ConflictRow> {
        self.conflicts.get(idx).copied()
    }

    /// 判定次序账笔数。
    pub fn judged_len(&self) -> usize {
        self.judged.len()
    }

    /// 判定次序账是否含该场景。
    pub fn judged_contains(&self, scene_no: u32) -> bool {
        self.judged.contains(&scene_no)
    }

    /// 判定前六道闸（次序固定）：状态→封账→场景→重复→机 A 闭集→机 B 闭集。
    /// 过闸即登记判定次序账（含语义待定场景），返回双机原始终码。
    fn judge_gates(&mut self, scene_no: u32) -> Result<(u32, u32), u32> {
        // 闸1 状态闸：未冻结禁止判定。
        if self.state.rank() < DmState::Armed.rank() {
            return Err(DmDiag::ENotFrozen.val());
        }
        // 闸2 封账闸：封账后只读。
        if self.state.rank() >= DmState::Sealed.rank() {
            return Err(DmDiag::ESealed.val());
        }
        // 闸3 场景闸：表外场景 ENOENT。
        if scene_of(scene_no).is_none() {
            return Err(DmDiag::ESceneNoent.val());
        }
        // 闸4 重复判定闸：冻结抽验每场景只判一次。
        if self.judged_contains(scene_no) {
            return Err(DmDiag::ERejudge.val());
        }
        // 闸5 机 A 闭集闸。
        let ra = match self.a.row_of(scene_no) {
            Some(r) => r.raw,
            None => return Err(DmDiag::ESceneNoent.val()),
        };
        if Outcome::from_raw(ra).is_none() {
            return Err(DmDiag::EOutcomeRange.val());
        }
        // 闸6 机 B 闭集闸。
        let rb = match self.b.row_of(scene_no) {
            Some(r) => r.raw,
            None => return Err(DmDiag::ESceneNoent.val()),
        };
        if Outcome::from_raw(rb).is_none() {
            return Err(DmDiag::EOutcomeRange.val());
        }
        // 过闸即入判定次序账（语义待定也算跑过）。
        self.judged.push(scene_no);
        Ok((ra, rb))
    }

    /// 语义裁决（共用）：两边皆未知→语义待定；相同→一致；不同→显式冲突。
    fn semantic(oa: Outcome, ob: Outcome) -> Result<(Verdict, bool), u32> {
        if oa == Outcome::Unknown && ob == Outcome::Unknown {
            return Err(DmDiag::ETentative.val());
        }
        if oa == ob {
            return Ok((Verdict::Agree, false));
        }
        Ok((Verdict::Conflict, true))
    }

    /// 一致判定器：judge_agree(scene_no)。
    /// A=B→Agree；A≠B→显式 Conflict（冲突行强制入账，不许静默吞并）；
    /// 两边皆 Unknown→Err(ETENTATIVE 语义码)；表外→ENOENT；越闭集→EINVAL。
    pub fn judge_agree(&mut self, scene_no: u32) -> Result<Verdict, u32> {
        let (ra, rb) = self.judge_gates(scene_no)?;
        let oa = match Outcome::from_raw(ra) {
            Some(o) => o,
            None => return Err(DmDiag::EOutcomeRange.val()),
        };
        let ob = match Outcome::from_raw(rb) {
            Some(o) => o,
            None => return Err(DmDiag::EOutcomeRange.val()),
        };
        let (v, need_ledger) = Self::semantic(oa, ob)?;
        if need_ledger {
            // 显式冲突账：冲突必须入账。
            self.conflicts.push(ConflictRow {
                scene_no,
                a_raw: ra,
                b_raw: rb,
                recheck: false,
            });
        }
        Ok(v)
    }

    /// 缺陷判定器（注入面）：与 judge_agree 同闸序，但 A≠B 时静默吞并——
    /// 不把冲突行入显式账。供「冲突静默」注入轮使用，覆盖率复算必须抓出。
    pub fn judge_agree_silent(&mut self, scene_no: u32) -> Result<Verdict, u32> {
        let (ra, rb) = self.judge_gates(scene_no)?;
        let oa = match Outcome::from_raw(ra) {
            Some(o) => o,
            None => return Err(DmDiag::EOutcomeRange.val()),
        };
        let ob = match Outcome::from_raw(rb) {
            Some(o) => o,
            None => return Err(DmDiag::EOutcomeRange.val()),
        };
        let (v, need_ledger) = Self::semantic(oa, ob)?;
        // 缺陷：need_ledger 被丢弃——冲突静默吞并，不入显式账。
        let _ = need_ledger;
        Ok(v)
    }

    /// 冲突覆盖率复算：从双机行为账独立重推"应判冲突"的场景闭集。
    /// 复算口径：双机结果都合法且归类不同（含单边未知）即应判冲突。
    pub fn conflict_expected_set(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for sp in SCENE_TABLE.iter() {
            let oa = match self.a.row_of(sp.no) {
                Some(r) => Outcome::from_raw(r.raw),
                None => None,
            };
            let ob = match self.b.row_of(sp.no) {
                Some(r) => Outcome::from_raw(r.raw),
                None => None,
            };
            if let (Some(x), Some(y)) = (oa, ob) {
                if x != y {
                    out.push(sp.no);
                }
            }
        }
        out
    }

    /// 冲突静默检测：显式账少于复算闭集或缺口场景 → 冲突静默嫌疑。
    /// Ok(n) = 复算平（n 为冲突行数）；Err 为对应诊断码。
    pub fn detect_silent(&self) -> Result<usize, u32> {
        let exp = self.conflict_expected_set();
        if self.conflicts.len() > exp.len() {
            // 账面多于复算：多登行，覆盖率不平。
            return Err(DmDiag::ECoverage.val());
        }
        if self.conflicts.len() < exp.len() {
            // 账面少于复算：判定器吞并冲突。
            return Err(DmDiag::EConflictSilent.val());
        }
        for no in exp.iter() {
            if self.conflict_of(*no).is_none() {
                // 场景错位：复算闭集内有、显式账内无。
                return Err(DmDiag::EConflictSilent.val());
            }
        }
        Ok(exp.len())
    }

    /// 冲突覆盖率复算平衡（detect_silent 的布尔面）。
    pub fn conflict_coverage_ok(&self) -> bool {
        self.detect_silent().is_ok()
    }

    /// 复验旗标是否全部置齐（无冲突行时视为齐）。
    pub fn recheck_all_done(&self) -> bool {
        self.conflicts.iter().all(|r| r.recheck)
    }

    /// 置复验旗标：仅冻结态可置；无冲突行的场景拒绝（复验目标缺行）。
    pub fn mark_rechecked(&mut self, scene_no: u32) -> Result<(), u32> {
        if self.state.rank() >= DmState::Sealed.rank() {
            return Err(DmDiag::ESealed.val());
        }
        if self.state.rank() < DmState::Armed.rank() {
            return Err(DmDiag::ENotFrozen.val());
        }
        let idx = match self.conflicts.iter().position(|r| r.scene_no == scene_no) {
            Some(i) => i,
            None => return Err(DmDiag::ECoverage.val()),
        };
        if let Some(row) = self.conflicts.get_mut(idx) {
            row.recheck = true;
            return Ok(());
        }
        Err(DmDiag::ECoverage.val())
    }

    /// 封账：冻结态 + 10 场景全跑 + 覆盖率复算平 + 复验旗标齐 才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= DmState::Sealed.rank() {
            return Err(DmDiag::ERewind.val());
        }
        if self.state.rank() < DmState::Armed.rank() {
            return Err(DmDiag::ENotFrozen.val());
        }
        // 全跑条数与聚合账等式：判定次序账必须覆盖全部 10 个冻结场景。
        if self.judged.len() != SCENE_CAP {
            return Err(DmDiag::ENotFrozen.val());
        }
        for sp in SCENE_TABLE.iter() {
            if !self.judged_contains(sp.no) {
                return Err(DmDiag::ENotFrozen.val());
            }
        }
        if !self.conflict_coverage_ok() {
            return Err(DmDiag::ECoverage.val());
        }
        if !self.recheck_all_done() {
            return Err(DmDiag::ERecheck.val());
        }
        self.state = DmState::Sealed;
        Ok(())
    }

    /// 双跑一致指纹：状态机 + 双机行为账 + 判定次序账 + 冲突显式账的确定性折叠。
    pub fn fingerprint(&self) -> u32 {
        // 黄金比率常数折叠（整数手册常用混洗初值）。
        let mut h = SEG_BASE.wrapping_mul(0x9E37_79B1);
        h = h.wrapping_add(self.state.rank() as u32);
        for r in self.a.rows.iter() {
            h = h.wrapping_mul(31).wrapping_add(r.scene_no);
            h = h.wrapping_add(r.raw.wrapping_mul(7));
        }
        for r in self.b.rows.iter() {
            h = h.wrapping_mul(37).wrapping_add(r.scene_no);
            h = h.wrapping_add(r.raw.wrapping_mul(11));
        }
        for no in self.judged.iter() {
            h = h.wrapping_mul(41).wrapping_add(*no);
        }
        for c in self.conflicts.iter() {
            h = h.wrapping_mul(43).wrapping_add(c.scene_no);
            h = h.wrapping_add(c.a_raw.wrapping_mul(3));
            h = h.wrapping_add(c.b_raw.wrapping_mul(5));
            h = h.wrapping_add(if c.recheck { 1 } else { 0 });
        }
        h
    }
}

/// 双跑指纹对账：两次全流程指纹相等→Ok(指纹)；不等→EFingerprint。
pub fn fingerprint_equal(a: &DualJudge, b: &DualJudge) -> Result<u32, u32> {
    let fa = a.fingerprint();
    if fa != b.fingerprint() {
        return Err(DmDiag::EFingerprint.val());
    }
    Ok(fa)
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 64 位推荐值（整数手册常用闭集）。
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 为零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

// ── 冻结语料与构建器 ─────────────────────────────────────────────
/// 标准抽验语料（冻结）：每行 (场景号, 机A原始终码, 机B原始终码)。
/// 来源：F12773 双机抽验批冻结语料——场景 3/6/9 为冲突样本，其余为一致样本。
pub const CORPUS_STANDARD: &[(u32, u32, u32)] = &[
    (1, OUTCOME_RAW_AGREE, OUTCOME_RAW_AGREE),
    (2, OUTCOME_RAW_AGREE, OUTCOME_RAW_AGREE),
    (3, OUTCOME_RAW_CONFLICT, OUTCOME_RAW_AGREE),
    (4, OUTCOME_RAW_AGREE, OUTCOME_RAW_AGREE),
    (5, OUTCOME_RAW_AGREE, OUTCOME_RAW_AGREE),
    (6, OUTCOME_RAW_AGREE, OUTCOME_RAW_CONFLICT),
    (7, OUTCOME_RAW_AGREE, OUTCOME_RAW_AGREE),
    (8, OUTCOME_RAW_AGREE, OUTCOME_RAW_AGREE),
    (9, OUTCOME_RAW_CONFLICT, OUTCOME_RAW_AGREE),
    (10, OUTCOME_RAW_AGREE, OUTCOME_RAW_AGREE),
];

/// 标准语料一致场景数（冻结声明，判定后与实跑计数对等式）。
pub const AGREE_EXPECTED: usize = 7;
/// 标准语料冲突场景数（冻结声明，判定后与显式账行数对等式）。
pub const CONFLICT_EXPECTED: usize = 3;
/// 标准语料冲突场景闭集（与 CORPUS_STANDARD 冻结口径一致）。
pub const CONFLICT_SCENES: &[u32] = &[3, 6, 9];

/// 语料变体构建器：把指定场景的双机原始终码替换为给定值，其余沿用标准语料。
/// override_no 不在 1..=10 时等价于标准语料。
pub fn build_variant_judge(override_no: u32, a_raw: u32, b_raw: u32) -> DualJudge {
    let mut j = DualJudge::new();
    for cell in CORPUS_STANDARD.iter() {
        let (ra, rb) = if cell.0 == override_no {
            (a_raw, b_raw)
        } else {
            (cell.1, cell.2)
        };
        if j.observe_a(cell.0, ra).is_err() {
            break;
        }
        if j.observe_b(cell.0, rb).is_err() {
            break;
        }
    }
    let _ = j.freeze();
    j
}

/// 标准抽验账构建器：冻结语料双机登记 + 冻结。
pub fn build_standard_judge() -> DualJudge {
    build_variant_judge(0, OUTCOME_RAW_AGREE, OUTCOME_RAW_AGREE)
}

/// 全跑：按冻结表序判定全部 10 场景。
/// 返回 (一致数, 冲突数, 语义待定数)；遇硬错误即中断返回该错误码。
pub fn judge_all(j: &mut DualJudge) -> Result<(usize, usize, usize), u32> {
    let mut agree = 0usize;
    let mut conflict = 0usize;
    let mut tentative = 0usize;
    for sp in SCENE_TABLE.iter() {
        let r = j.judge_agree(sp.no);
        match r {
            Ok(Verdict::Agree) => agree += 1,
            Ok(Verdict::Conflict) => conflict += 1,
            Err(e) => {
                if e == DmDiag::ETentative.val() {
                    tentative += 1;
                } else {
                    return Err(e);
                }
            }
        }
    }
    Ok((agree, conflict, tentative))
}

/// 全量复验：把显式账内每个冲突行的复验旗标置齐，返回置旗行数。
pub fn recheck_all(j: &mut DualJudge) -> usize {
    let n = j.conflict_len();
    let mut i = 0usize;
    while i < n {
        if let Some(row) = j.conflict_at(i) {
            let _ = j.mark_rechecked(row.scene_no);
        }
        i += 1;
    }
    n
}

// ── 注入面：三类注入器各 10 轮 ───────────────────────────────────
/// 注入器一：场景未登记 ×10 轮。对冻结账判表外场景号，
/// 必须回 ENOENT 且不留判定次序账、不入冲突账；逃脱即判据失效。
pub fn inject_sweep_unregistered() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x11) as u64);
    let mut k = 0usize;
    while k < 10 {
        let mut j = build_standard_judge();
        let fake = 100u32 + rng.below(0x100);
        let r = j.judge_agree(fake);
        total += 1;
        let hit = r == Err(DmDiag::ESceneNoent.val())
            && j.judged_len() == 0
            && j.conflict_len() == 0;
        if hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{} 场景{} 未登记注入逃脱：判定器未回 ENOENT", k, fake);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入器二：结果越闭集 ×10 轮。把已登记场景的行为原始终码篡改为
/// 闭集外值（注入改写面），判定器必须回 EINVAL 且不入任何账。
pub fn inject_sweep_outcome_range() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x22) as u64);
    let mut k = 0usize;
    while k < 10 {
        let mut j = build_standard_judge();
        let no = 1u32 + rng.below(10);
        let bad = 4u32 + rng.below(100);
        let mid = if k % 2 == 0 { MachineId::A } else { MachineId::B };
        let planted = match mid {
            MachineId::A => j.plant_a(no, bad),
            MachineId::B => j.plant_b(no, bad),
        };
        let r = j.judge_agree(no);
        total += 1;
        let hit = planted == Ok(())
            && r == Err(DmDiag::EOutcomeRange.val())
            && j.judged_len() == 0
            && j.conflict_len() == 0;
        if hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!(
                "轮{} 场景{} {}越闭集注入逃脱：判定器未回 EINVAL",
                k,
                no,
                mid.name()
            );
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入器三：冲突静默 ×10 轮。用缺陷判定器（吞并冲突）判真实冲突场景，
/// 显式账必须缺行且覆盖率复算必须检出（EConflictSilent）；漏检即判据失效。
pub fn inject_sweep_conflict_silent() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x33) as u64);
    let mut k = 0usize;
    while k < 10 {
        let mut j = build_standard_judge();
        let idx = rng.below(CONFLICT_SCENES.len() as u32) as usize;
        let no = match CONFLICT_SCENES.get(idx) {
            Some(v) => *v,
            None => 3,
        };
        let r = j.judge_agree_silent(no);
        total += 1;
        let hit = r == Ok(Verdict::Conflict)
            && j.conflict_len() == 0
            && j.detect_silent() == Err(DmDiag::EConflictSilent.val());
        if hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("轮{} 场景{} 冲突静默注入逃脱：复算未检出缺行", k, no);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 判据谓词（私有，断言方向：cond 为违约条件，末行为成立条件） ──
/// 段位守卫：SEG 对齐 0x40 且落在派工卡窗口 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：互异、全落段窗口、每码中文建议非空、查表命中、未知码回落。
fn diag_closed_ok() -> bool {
    if DM_ALL.len() != 13 {
        return false;
    }
    for d in DM_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
    }
    let mut i = 0usize;
    while i < DM_ALL.len() {
        let vi = match DM_ALL.get(i) {
            Some(d) => d.val(),
            None => 0,
        };
        let mut jj = i + 1;
        while jj < DM_ALL.len() {
            let vj = match DM_ALL.get(jj) {
                Some(d) => d.val(),
                None => 0,
            };
            if vi == vj {
                return false;
            }
            jj += 1;
        }
        if detail(vi).is_empty() {
            return false;
        }
        i += 1;
    }
    if detail(DmDiag::ETentative.val()) != DmDiag::ETentative.detail() {
        return false;
    }
    if !detail(SEG_BASE + 0x3F).contains("未知") {
        return false;
    }
    true
}

/// 场景冻结表守卫：10 场景、号 1..=10 严格递增、名称与锚点非空、查表命中与回落。
fn scene_table_ok() -> bool {
    if SCENE_TABLE.len() != SCENE_CAP {
        return false;
    }
    let mut prev = 0u32;
    for sp in SCENE_TABLE.iter() {
        if sp.no != prev + 1 {
            return false;
        }
        if sp.name.is_empty() || sp.anchor.is_empty() {
            return false;
        }
        prev = sp.no;
    }
    if scene_of(1).is_none() || scene_of(10).is_none() {
        return false;
    }
    if scene_of(0).is_some() || scene_of(11).is_some() || scene_of(999).is_some() {
        return false;
    }
    true
}

/// 行为结果闭集守卫：三值往返一致、闭集外一律 None、名称非空。
fn outcome_closed_ok() -> bool {
    if OUTCOME_ALL.len() != 3 {
        return false;
    }
    if Outcome::from_raw(OUTCOME_RAW_AGREE) != Some(Outcome::Agree) {
        return false;
    }
    if Outcome::from_raw(OUTCOME_RAW_CONFLICT) != Some(Outcome::Conflict) {
        return false;
    }
    if Outcome::from_raw(OUTCOME_RAW_UNKNOWN) != Some(Outcome::Unknown) {
        return false;
    }
    if Outcome::from_raw(0).is_some() || Outcome::from_raw(4).is_some() {
        return false;
    }
    if Outcome::from_raw(0xFFFF).is_some() {
        return false;
    }
    for o in OUTCOME_ALL.iter() {
        if o.name().is_empty() {
            return false;
        }
        match Outcome::from_raw(o.raw()) {
            Some(back) => {
                if back != *o {
                    return false;
                }
            }
            None => return false,
        }
    }
    true
}

/// 双机视图登记正常流：表外/越闭集/重复三路显性拒，冻结语料双机各 10 笔。
fn view_register_ok() -> bool {
    let mut j = DualJudge::new();
    // 表外场景：ENOENT（场景 0 与场景 11 两侧）。
    if j.observe_a(0, OUTCOME_RAW_AGREE) != Err(DmDiag::ESceneNoent.val()) {
        return false;
    }
    if j.observe_b(11, OUTCOME_RAW_AGREE) != Err(DmDiag::ESceneNoent.val()) {
        return false;
    }
    // 越闭集原始终码：正常面登记即拒。
    if j.observe_a(1, 4) != Err(DmDiag::EOutcomeRange.val()) {
        return false;
    }
    if j.observe_b(1, 0) != Err(DmDiag::EOutcomeRange.val()) {
        return false;
    }
    // 正常登记成功 + 重复登记拒。
    if j.observe_a(1, OUTCOME_RAW_AGREE).is_err() {
        return false;
    }
    if j.observe_a(1, OUTCOME_RAW_CONFLICT) != Err(DmDiag::ESceneDup.val()) {
        return false;
    }
    // 冻结语料登记：双机各 10 笔且与冻结行逐场景等式。
    let std = build_standard_judge();
    for sp in SCENE_TABLE.iter() {
        let cell = match CORPUS_STANDARD.iter().find(|c| c.0 == sp.no) {
            Some(c) => *c,
            None => return false,
        };
        let ra = match std.row_of(MachineId::A, sp.no) {
            Some(r) => r.raw,
            None => 0,
        };
        let rb = match std.row_of(MachineId::B, sp.no) {
            Some(r) => r.raw,
            None => 0,
        };
        if ra != cell.1 || rb != cell.2 {
            return false;
        }
    }
    true
}

/// 冻结门：缺账拒、单机齐仍拒、双机齐进 Armed、重冻结倒退拒。
fn freeze_gate_ok() -> bool {
    let mut j = DualJudge::new();
    if j.freeze() != Err(DmDiag::ENotFrozen.val()) {
        return false;
    }
    for sp in SCENE_TABLE.iter() {
        let _ = j.observe_a(sp.no, OUTCOME_RAW_AGREE);
    }
    if j.freeze() != Err(DmDiag::ENotFrozen.val()) {
        return false;
    }
    for sp in SCENE_TABLE.iter() {
        let _ = j.observe_b(sp.no, OUTCOME_RAW_AGREE);
    }
    if j.freeze().is_err() {
        return false;
    }
    if j.state() != DmState::Armed {
        return false;
    }
    j.freeze() == Err(DmDiag::ERewind.val())
}

/// 判定次序守卫：六道闸次序逐一锚定——状态→封账→场景→重复→A闭集→B闭集。
fn judge_order_ok() -> bool {
    // ① 状态闸先于场景闸：未冻结时表内表外同回 ENotFrozen。
    let mut j0 = DualJudge::new();
    if j0.judge_agree(1) != Err(DmDiag::ENotFrozen.val()) {
        return false;
    }
    if j0.judge_agree(999) != Err(DmDiag::ENotFrozen.val()) {
        return false;
    }
    // ② 场景闸先于闭集闸：表外场景即使机账被注入越闭集码也回 ENOENT。
    let mut j = build_standard_judge();
    if j.plant_a(1, 4) != Ok(()) {
        return false;
    }
    if j.judge_agree(999) != Err(DmDiag::ESceneNoent.val()) {
        return false;
    }
    // ③ 表内越闭集：回 EINVAL 且不留判定次序账、不入冲突账。
    if j.judge_agree(1) != Err(DmDiag::EOutcomeRange.val()) {
        return false;
    }
    if j.judged_len() != 0 || j.conflict_len() != 0 {
        return false;
    }
    // ④ 判定成功后同场景重判：ERejudge。
    if j.judge_agree(2) != Ok(Verdict::Agree) {
        return false;
    }
    if j.judge_agree(2) != Err(DmDiag::ERejudge.val()) {
        return false;
    }
    true
}

/// 正常流：冻结语料全跑，一致/冲突计数与冻结声明对等式。
fn agree_path_ok() -> bool {
    let mut j = build_standard_judge();
    let counts = match judge_all(&mut j) {
        Ok(c) => c,
        Err(_) => return false,
    };
    counts.0 == AGREE_EXPECTED && counts.1 == CONFLICT_EXPECTED && counts.2 == 0
        && j.conflict_len() == CONFLICT_EXPECTED
}

/// CONFLICT 显式账守卫：冲突行内容逐字段正确、一致场景不入账、覆盖率平。
fn conflict_ledger_ok() -> bool {
    let mut j = build_standard_judge();
    let _ = judge_all(&mut j);
    // 场景 3：机A=冲突(2)、机B=一致(1)，冲突行已显式登记且复验未置。
    let row3 = match j.conflict_of(3) {
        Some(r) => r,
        None => return false,
    };
    if row3.a_raw != OUTCOME_RAW_CONFLICT || row3.b_raw != OUTCOME_RAW_AGREE || row3.recheck {
        return false;
    }
    // 一致场景不得入冲突账。
    if j.conflict_of(2).is_some() || j.conflict_of(10).is_some() {
        return false;
    }
    if j.conflict_len() != CONFLICT_EXPECTED {
        return false;
    }
    j.conflict_coverage_ok()
}

/// 复验旗标与封账守卫：未置齐封账拒、缺行复验拒、置齐后封账成功。
fn recheck_seal_ok() -> bool {
    let mut j = build_standard_judge();
    let _ = judge_all(&mut j);
    // 复验未置齐：封账拒绝。
    if j.seal() != Err(DmDiag::ERecheck.val()) {
        return false;
    }
    // 一致场景无冲突行：复验目标缺行拒绝。
    if j.mark_rechecked(2) != Err(DmDiag::ECoverage.val()) {
        return false;
    }
    let n = recheck_all(&mut j);
    if n != CONFLICT_EXPECTED {
        return false;
    }
    if !j.recheck_all_done() {
        return false;
    }
    j.seal() == Ok(())
}

/// 封账守卫：未冻结/未全跑/重封/封后判定/封后写入/封后冻结 全数拒绝。
fn seal_guard_ok() -> bool {
    // 收集期封账：未冻结拒。
    let mut j0 = DualJudge::new();
    if j0.seal() != Err(DmDiag::ENotFrozen.val()) {
        return false;
    }
    // 冻结但未全跑：封账拒。
    let mut j = build_standard_judge();
    if j.seal() != Err(DmDiag::ENotFrozen.val()) {
        return false;
    }
    // 全跑 + 复验 + 封账成功后：重封倒退、封后判定、封后写入、封后冻结全拒。
    let _ = judge_all(&mut j);
    let _ = recheck_all(&mut j);
    if j.seal().is_err() {
        return false;
    }
    if j.seal() != Err(DmDiag::ERewind.val()) {
        return false;
    }
    if j.judge_agree(1) != Err(DmDiag::ESealed.val()) {
        return false;
    }
    if j.observe_a(1, OUTCOME_RAW_AGREE) != Err(DmDiag::ESealed.val()) {
        return false;
    }
    if j.plant_b(2, 4) != Err(DmDiag::ESealed.val()) {
        return false;
    }
    if j.freeze() != Err(DmDiag::ERewind.val()) {
        return false;
    }
    true
}

/// 容量耗尽守卫：满 10 笔后任何正常面登记（含表内/表外）都拒；
/// 注入面不改容量语义——满表下仍可对已登记场景改写原始终码。
fn capacity_ok() -> bool {
    let mut j = DualJudge::new();
    for sp in SCENE_TABLE.iter() {
        if j.observe_a(sp.no, OUTCOME_RAW_AGREE).is_err() {
            return false;
        }
        if j.observe_b(sp.no, OUTCOME_RAW_AGREE).is_err() {
            return false;
        }
    }
    // 容量耗尽：正常面再登记一律拒（表内场景与表外场景同拒）。
    if j.observe_a(1, OUTCOME_RAW_AGREE) != Err(DmDiag::ECapacity.val()) {
        return false;
    }
    if j.observe_b(99, OUTCOME_RAW_AGREE) != Err(DmDiag::ECapacity.val()) {
        return false;
    }
    // 注入面：满表下改写已登记场景可行（仅供注入器构造越闭集样本）。
    if j.plant_a(2, 4) != Ok(()) {
        return false;
    }
    true
}

/// 注入一必抓：场景未登记 10/10 全数被 ENOENT 闸抓住。
fn inject_unreg_ok() -> bool {
    let (caught, total, escape) = inject_sweep_unregistered();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 注入二必抓：结果越闭集 10/10 全数被 EINVAL 闸抓住。
fn inject_range_ok() -> bool {
    let (caught, total, escape) = inject_sweep_outcome_range();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 注入三必抓：冲突静默 10/10 全数被覆盖率复算抓住。
fn inject_silent_ok() -> bool {
    let (caught, total, escape) = inject_sweep_conflict_silent();
    total == 10 && caught == 10 && escape.is_empty()
}

/// 冲突覆盖率复算守卫：正常账复算平且逐场景等式；静默样本复算必不平。
fn coverage_ok_pred() -> bool {
    let mut j = build_standard_judge();
    let _ = judge_all(&mut j);
    if !j.conflict_coverage_ok() {
        return false;
    }
    let exp = j.conflict_expected_set();
    if exp.len() != CONFLICT_EXPECTED {
        return false;
    }
    for no in exp.iter() {
        if j.conflict_of(*no).is_none() {
            return false;
        }
    }
    // 静默样本：缺陷判定器跑冲突场景后复算必须报静默。
    let mut s = build_standard_judge();
    let _ = s.judge_agree_silent(6);
    if s.conflict_coverage_ok() {
        return false;
    }
    s.detect_silent() == Err(DmDiag::EConflictSilent.val())
}

/// 双跑一致守卫：两次独立完整流程（登记→冻结→全跑→复验→封账）指纹逐项一致；
/// 指纹对语料变体与账面推进敏感（防恒值指纹）；三类注入扫描双跑一致。
fn dual_run_ok() -> bool {
    let mut j1 = build_standard_judge();
    let mut j2 = build_standard_judge();
    let c1 = match judge_all(&mut j1) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let c2 = match judge_all(&mut j2) {
        Ok(c) => c,
        Err(_) => return false,
    };
    if c1 != c2 {
        return false;
    }
    let fp1 = j1.fingerprint();
    if fp1 != j2.fingerprint() {
        return false;
    }
    // 指纹灵敏度：同态语料变体指纹必异。
    let mut j3 = build_variant_judge(2, OUTCOME_RAW_CONFLICT, OUTCOME_RAW_AGREE);
    let _ = judge_all(&mut j3);
    if j3.fingerprint() == fp1 {
        return false;
    }
    // 复验+封账推进：指纹随账面前进变化，双跑仍一致。
    let _ = recheck_all(&mut j1);
    let _ = recheck_all(&mut j2);
    if j1.fingerprint() == fp1 {
        return false;
    }
    if fingerprint_equal(&j1, &j2).is_err() {
        return false;
    }
    if j1.seal().is_err() || j2.seal().is_err() {
        return false;
    }
    if fingerprint_equal(&j1, &j2).is_err() {
        return false;
    }
    // 三类注入扫描双跑一致。
    if inject_sweep_unregistered() != inject_sweep_unregistered() {
        return false;
    }
    if inject_sweep_outcome_range() != inject_sweep_outcome_range() {
        return false;
    }
    if inject_sweep_conflict_silent() != inject_sweep_conflict_silent() {
        return false;
    }
    true
}

/// 边界一：两边皆未知 → ETENTATIVE 语义码；跑次入判定次序账；
/// 其余场景不受影响，待定场景计入全跑账后仍可复验封账。
fn tentative_ok() -> bool {
    let mut j = build_variant_judge(7, OUTCOME_RAW_UNKNOWN, OUTCOME_RAW_UNKNOWN);
    if j.judge_agree(7) != Err(DmDiag::ETentative.val()) {
        return false;
    }
    if j.judged_len() != 1 || !j.judged_contains(7) {
        return false;
    }
    if j.conflict_len() != 0 {
        return false;
    }
    // 其余场景不受影响。
    if j.judge_agree(1) != Ok(Verdict::Agree) {
        return false;
    }
    // 剩余未判场景逐一遍跑（已判场景跳过）。
    for sp in SCENE_TABLE.iter() {
        if !j.judged_contains(sp.no) {
            let _ = j.judge_agree(sp.no);
        }
    }
    let _ = recheck_all(&mut j);
    j.seal().is_ok() && j.state() == DmState::Sealed
}

/// 边界二：单边未知 → A≠B 显式 Conflict，必须入显式账且复算平。
fn mixed_unknown_ok() -> bool {
    // A=Unknown、B=Agree：显式冲突。
    let mut j = build_variant_judge(4, OUTCOME_RAW_UNKNOWN, OUTCOME_RAW_AGREE);
    let c = match judge_all(&mut j) {
        Ok(c) => c,
        Err(_) => return false,
    };
    if c.0 != AGREE_EXPECTED - 1 || c.1 != CONFLICT_EXPECTED + 1 {
        return false;
    }
    match j.conflict_of(4) {
        Some(r) => {
            if r.a_raw != OUTCOME_RAW_UNKNOWN || r.b_raw != OUTCOME_RAW_AGREE || r.recheck {
                return false;
            }
        }
        None => return false,
    }
    // B=Unknown、A=Agree：同样显式冲突。
    let mut j2 = build_variant_judge(5, OUTCOME_RAW_AGREE, OUTCOME_RAW_UNKNOWN);
    let c2 = match judge_all(&mut j2) {
        Ok(c) => c,
        Err(_) => return false,
    };
    if c2.0 != AGREE_EXPECTED - 1 || c2.1 != CONFLICT_EXPECTED + 1 {
        return false;
    }
    if j2.conflict_of(5).is_none() {
        return false;
    }
    if !j2.conflict_coverage_ok() {
        return false;
    }
    let _ = recheck_all(&mut j2);
    j2.seal().is_ok()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// 判据 J1 单跑入口总数（收口对账用）。
pub const CHECK_TOTAL: usize = 20;

/// UNX-F12774 自检（判据 12773 卡：文档对齐双机抽验 20 条单跑入口齐，
/// 全跑条数与聚合账等式断言随批归档）。
pub fn run_unx12773_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12773");
    let mut ran = 0usize;

    // 01 段位守卫。
    s.add_dyn(
        "F12774-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05X} 对齐0x40={} 窗口[0x80000,0x831C0]={}",
            SEG_BASE,
            SEG_BASE % 0x40 == 0,
            (0x80000..=0x831C0).contains(&SEG_BASE)
        ),
    );
    ran += 1;

    // 02 诊断码闭集。
    s.add_dyn(
        "F12774-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "codes={} 窗口=[0x{:05X},0x{:05X})",
            DM_ALL.len(),
            SEG_BASE,
            SEG_BASE + 0x40
        ),
    );
    ran += 1;

    // 03 场景冻结表。
    s.add_dyn(
        "F12774-03 场景冻结表 10 场景",
        scene_table_ok(),
        &format!(
            "scenes={} cap={} 首={} 尾={}",
            SCENE_TABLE.len(),
            SCENE_CAP,
            scene_of(1).map(|sp| sp.no).unwrap_or(0),
            scene_of(10).map(|sp| sp.no).unwrap_or(0)
        ),
    );
    ran += 1;

    // 04 行为结果闭集。
    s.add_dyn(
        "F12774-04 行为结果闭集三值",
        outcome_closed_ok(),
        &format!(
            "值数={} 码1={} 码2={} 码3={}",
            OUTCOME_ALL.len(),
            OUTCOME_RAW_AGREE,
            OUTCOME_RAW_CONFLICT,
            OUTCOME_RAW_UNKNOWN
        ),
    );
    ran += 1;

    // 05 双机视图登记正常流。
    let std = build_standard_judge();
    s.add_dyn(
        "F12774-05 双机视图登记正常流",
        view_register_ok(),
        &format!(
            "机A行={} 机B行={} cap={}",
            std.view_len(MachineId::A),
            std.view_len(MachineId::B),
            SCENE_CAP
        ),
    );
    ran += 1;

    // 06 冻结门与倒退拒。
    s.add_dyn(
        "F12774-06 冻结门与倒退拒",
        freeze_gate_ok(),
        "冻结门：缺账拒/单机齐仍拒/双机齐进Armed/重冻结倒退拒 四点全过",
    );
    ran += 1;

    // 07 判定次序。
    s.add_dyn(
        "F12774-07 判定次序六道闸",
        judge_order_ok(),
        "次序：状态→封账→场景→重复→机A闭集→机B闭集 逐一锚定",
    );
    ran += 1;

    // 08 正常判定与冻结声明等式。
    let mut jrun = build_standard_judge();
    let c = match judge_all(&mut jrun) {
        Ok(c) => c,
        Err(_) => (0usize, 0usize, 0usize),
    };
    s.add_dyn(
        "F12774-08 一致判定与声明等式",
        agree_path_ok(),
        &format!(
            "一致={} 冲突={} 待定={} 声明7/3 词={}",
            c.0,
            c.1,
            c.2,
            Verdict::Agree.name()
        ),
    );
    ran += 1;

    // 09 CONFLICT 显式账。
    let mut jc = build_standard_judge();
    let _ = judge_all(&mut jc);
    s.add_dyn(
        "F12774-09 冲突显式账",
        conflict_ledger_ok(),
        &format!(
            "冲突行={} 行3复验前={} 复算平={}",
            jc.conflict_len(),
            jc.conflict_of(3).map(|r| r.recheck).unwrap_or(true),
            jc.conflict_coverage_ok()
        ),
    );
    ran += 1;

    // 10 复验旗标与封账。
    s.add_dyn(
        "F12774-10 复验旗标与封账",
        recheck_seal_ok(),
        "复验未置齐封账拒；缺行复验拒；置齐后封账成功",
    );
    ran += 1;

    // 11 封账守卫。
    s.add_dyn(
        "F12774-11 封账守卫",
        seal_guard_ok(),
        "未冻结/未全跑/重封/封后判定/封后写入/封后冻结 全拒",
    );
    ran += 1;

    // 12 容量耗尽。
    s.add_dyn(
        "F12774-12 容量耗尽守卫",
        capacity_ok(),
        &format!("cap={} 满后正常面登记=ECapacity 注入面改写=可行", SCENE_CAP),
    );
    ran += 1;

    // 13 注入一：场景未登记。
    let (cu, tu, eu) = inject_sweep_unregistered();
    s.add_dyn(
        "F12774-13 注入·场景未登记 10/10",
        inject_unreg_ok(),
        &format!("caught={}/{} {}", cu, tu, eu),
    );
    ran += 1;

    // 14 注入二：结果越闭集。
    let (cr, tr, er) = inject_sweep_outcome_range();
    s.add_dyn(
        "F12774-14 注入·结果越闭集 10/10",
        inject_range_ok(),
        &format!("caught={}/{} {}", cr, tr, er),
    );
    ran += 1;

    // 15 注入三：冲突静默。
    let (cs, ts, es) = inject_sweep_conflict_silent();
    s.add_dyn(
        "F12774-15 注入·冲突静默 10/10",
        inject_silent_ok(),
        &format!("caught={}/{} {}", cs, ts, es),
    );
    ran += 1;

    // 16 冲突覆盖率复算。
    let mut jcov = build_standard_judge();
    let _ = judge_all(&mut jcov);
    let exp_len = jcov.conflict_expected_set().len();
    let silent_caught = {
        let mut s2 = build_standard_judge();
        let _ = s2.judge_agree_silent(6);
        s2.detect_silent() == Err(DmDiag::EConflictSilent.val())
    };
    s.add_dyn(
        "F12774-16 冲突覆盖率复算",
        coverage_ok_pred(),
        &format!(
            "exp={} 账={} 静默检出={}",
            exp_len,
            jcov.conflict_len(),
            silent_caught
        ),
    );
    ran += 1;

    // 17 双跑一致指纹。
    s.add_dyn(
        "F12774-17 双跑一致指纹",
        dual_run_ok(),
        "双跑指纹相等+变体敏感+账面推进敏感+三类注入扫描双跑一致",
    );
    ran += 1;

    // 18 边界：两边皆未知。
    s.add_dyn(
        "F12774-18 边界·两边皆未知待定",
        tentative_ok(),
        "场景7 双机皆未知→ETENTATIVE；跑次入判定账；不影响其余场景封账",
    );
    ran += 1;

    // 19 边界：单边未知显式冲突。
    s.add_dyn(
        "F12774-19 边界·单边未知显式冲突",
        mixed_unknown_ok(),
        "单边未知→显式Conflict入账，复算平，复验后封账成功",
    );
    ran += 1;

    // 20 收口：判据条数声明（全跑条数与聚合账等式断言）。
    s.add_dyn(
        "F12774-20 收口·判据条数声明",
        ran == CHECK_TOTAL - 1,
        &format!(
            "已跑={} 收口=1 聚合账={} 判据 UNX-F12774-J1 20条单跑入口齐",
            ran,
            CHECK_TOTAL
        ),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_window() {
        // 段位守卫：对齐 0x40 且落在派工卡窗口，公式复核。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(
            0x80000u32.wrapping_add(0x40u32.wrapping_mul(12773 - 12600)),
            SEG_BASE
        );
    }

    #[test]
    fn diag_closed_set_and_detail() {
        // 闭集：13 码互异、全落窗口、语义码定位、每码中文建议非空、未知码回落。
        assert!(diag_closed_ok());
        assert_eq!(DM_ALL.len(), 13);
        assert_eq!(DmDiag::ESceneNoent.val(), SEG_BASE + 0x02);
        assert_eq!(DmDiag::EOutcomeRange.val(), SEG_BASE + 0x03);
        assert_eq!(DmDiag::ETentative.val(), SEG_BASE + 0x04);
        let mut i = 0usize;
        while i < DM_ALL.len() {
            let d = match DM_ALL.get(i) {
                Some(d) => *d,
                None => DmDiag::ERewind,
            };
            assert!(!d.detail().is_empty());
            assert_eq!(detail(d.val()), d.detail());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3E).contains("未知"));
    }

    #[test]
    fn scene_table_and_outcome_frozen() {
        // 冻结表与结果闭集：容量、声明等式、语料逐场景复算归类。
        assert!(scene_table_ok());
        assert!(outcome_closed_ok());
        assert_eq!(SCENE_TABLE.len(), 10);
        assert_eq!(CORPUS_STANDARD.len(), 10);
        assert_eq!(AGREE_EXPECTED, 7);
        assert_eq!(CONFLICT_EXPECTED, 3);
        assert_eq!(CONFLICT_SCENES.len(), 3);
        let mut agree = 0usize;
        let mut conflict = 0usize;
        for cell in CORPUS_STANDARD.iter() {
            let oa = match Outcome::from_raw(cell.1) {
                Some(o) => o,
                None => Outcome::Unknown,
            };
            let ob = match Outcome::from_raw(cell.2) {
                Some(o) => o,
                None => Outcome::Unknown,
            };
            if oa == ob {
                if oa != Outcome::Unknown {
                    agree += 1;
                }
            } else {
                conflict += 1;
            }
        }
        assert_eq!(agree, AGREE_EXPECTED);
        assert_eq!(conflict, CONFLICT_EXPECTED);
    }

    #[test]
    fn happy_path_full_flow() {
        // 正常流：登记→冻结→全跑→显式账→复验→封账，指纹自洽。
        let mut j = build_standard_judge();
        assert_eq!(j.state(), DmState::Armed);
        let c = match judge_all(&mut j) {
            Ok(c) => c,
            Err(_) => (0usize, 0usize, 0usize),
        };
        assert_eq!(c, (AGREE_EXPECTED, CONFLICT_EXPECTED, 0));
        assert_eq!(j.conflict_len(), 3);
        assert!(j.conflict_of(3).is_some());
        assert!(j.conflict_of(6).is_some());
        assert!(j.conflict_of(9).is_some());
        assert_eq!(j.seal(), Err(DmDiag::ERecheck.val()));
        assert_eq!(recheck_all(&mut j), 3);
        assert!(j.recheck_all_done());
        assert!(j.seal().is_ok());
        assert_eq!(j.state(), DmState::Sealed);
        let fp = j.fingerprint();
        assert_eq!(fp, j.fingerprint());
    }

    #[test]
    fn freeze_and_register_flow() {
        // 登记正常流 + 冻结门。
        assert!(view_register_ok());
        assert!(freeze_gate_ok());
    }

    #[test]
    fn judge_order_gates() {
        // 判定次序：状态→封账→场景→重复→A闭集→B闭集。
        assert!(judge_order_ok());
        let mut j0 = DualJudge::new();
        assert_eq!(j0.judge_agree(999), Err(DmDiag::ENotFrozen.val()));
        let mut j = build_standard_judge();
        assert_eq!(j.judge_agree(999), Err(DmDiag::ESceneNoent.val()));
        let _ = j.plant_a(1, 4);
        assert_eq!(j.judge_agree(1), Err(DmDiag::EOutcomeRange.val()));
        assert_eq!(j.judged_len(), 0);
        assert_eq!(j.conflict_len(), 0);
    }

    #[test]
    fn injections_all_caught() {
        // 三类注入各 10 轮全抓、无逃脱。
        let (c1, t1, e1) = inject_sweep_unregistered();
        assert_eq!(t1, 10);
        assert_eq!(c1, 10, "逃脱:{}", e1);
        assert!(e1.is_empty());
        let (c2, t2, e2) = inject_sweep_outcome_range();
        assert_eq!(t2, 10);
        assert_eq!(c2, 10, "逃脱:{}", e2);
        assert!(e2.is_empty());
        let (c3, t3, e3) = inject_sweep_conflict_silent();
        assert_eq!(t3, 10);
        assert_eq!(c3, 10, "逃脱:{}", e3);
        assert!(e3.is_empty());
        // 静默检测语义码直证。
        let mut s = build_standard_judge();
        let _ = s.judge_agree_silent(6);
        assert_eq!(s.detect_silent(), Err(DmDiag::EConflictSilent.val()));
        assert_eq!(s.conflict_len(), 0);
    }

    #[test]
    fn dual_run_fingerprints_identical() {
        // 双跑一致：两次独立全流程指纹相等；变体敏感。
        assert!(dual_run_ok());
        let mut a = build_standard_judge();
        let mut b = build_standard_judge();
        let _ = judge_all(&mut a);
        let _ = judge_all(&mut b);
        assert_eq!(fingerprint_equal(&a, &b), Ok(a.fingerprint()));
        let mut v = build_variant_judge(9, OUTCOME_RAW_AGREE, OUTCOME_RAW_AGREE);
        let _ = judge_all(&mut v);
        assert_ne!(v.fingerprint(), a.fingerprint());
    }

    #[test]
    fn capacity_and_plant_surface() {
        // 容量耗尽：满 10 笔后正常面全拒；注入面改写可用。
        assert!(capacity_ok());
        let mut j = DualJudge::new();
        for sp in SCENE_TABLE.iter() {
            assert!(j.observe_a(sp.no, OUTCOME_RAW_AGREE).is_ok());
        }
        assert_eq!(j.observe_a(1, OUTCOME_RAW_AGREE), Err(DmDiag::ECapacity.val()));
        assert_eq!(j.observe_a(0, OUTCOME_RAW_AGREE), Err(DmDiag::ECapacity.val()));
        assert_eq!(j.plant_a(3, 0x1234), Ok(()));
        assert_eq!(j.row_of(MachineId::A, 3).map(|r| r.raw), Some(0x1234));
    }

    #[test]
    fn tentative_and_mixed_unknown() {
        // 边界：两边皆未知→ETENTATIVE；单边未知→显式冲突入账。
        assert!(tentative_ok());
        assert!(mixed_unknown_ok());
        let mut t = build_variant_judge(7, OUTCOME_RAW_UNKNOWN, OUTCOME_RAW_UNKNOWN);
        assert_eq!(t.judge_agree(7), Err(DmDiag::ETentative.val()));
        assert!(t.judged_contains(7));
        assert_eq!(t.conflict_len(), 0);
        let mut m = build_variant_judge(7, OUTCOME_RAW_UNKNOWN, OUTCOME_RAW_AGREE);
        assert_eq!(m.judge_agree(7), Ok(Verdict::Conflict));
        assert!(m.conflict_of(7).is_some());
    }

    #[test]
    fn boundaries_first_last_and_rewinds() {
        // 边界：首/尾场景判定、重复判定、复验缺行、重冻结倒退、表外两侧。
        let mut j = build_standard_judge();
        assert_eq!(j.judge_agree(1), Ok(Verdict::Agree));
        assert_eq!(j.judge_agree(10), Ok(Verdict::Agree));
        assert!(j.judged_contains(1));
        assert!(j.judged_contains(10));
        assert_eq!(j.judge_agree(1), Err(DmDiag::ERejudge.val()));
        assert_eq!(j.mark_rechecked(2), Err(DmDiag::ECoverage.val()));
        assert_eq!(j.freeze(), Err(DmDiag::ERewind.val()));
        // 全跑未齐（已判 2 场景）→ 封账拒。
        assert_eq!(j.seal(), Err(DmDiag::ENotFrozen.val()));
        // 表头前(0)与表尾后(11)未登记场景两侧都拒。
        assert_eq!(j.judge_agree(0), Err(DmDiag::ESceneNoent.val()));
        assert_eq!(j.judge_agree(11), Err(DmDiag::ESceneNoent.val()));
    }

    #[test]
    fn checks_entry_twenty_green() {
        // 检查入口：20 条单跑入口齐、全绿、红项为零（聚合账等式）。
        let s = run_unx12773_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert_eq!(s.len(), CHECK_TOTAL);
        assert_eq!(s.tally().1, 0);
        assert_eq!(s.tally().0, CHECK_TOTAL);
    }
}
