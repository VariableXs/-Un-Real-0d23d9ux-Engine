//! UNX-F12776 B40 预告与开门件（域收官清单） · 0x82C00段
//!
//! 判据原文（派工卡 12776 逐字锚定）：
//!   UNX-F12777-J1 深化指引档齐（体例/硬闸/复测三段），可点验 10/10。
//!
//! 设计要点：
//! 1. B40 开门件域模型——域收官清单登记表：每行四元组（清单项号/清单段/
//!    衔接目标号/衔接旗标），清单段闭集 Regression|Sealed|Handoff 三段，
//!    登记表冻结 26 行（≥24 行下限），项号唯一递增、行序冻结不许重排；
//! 2. 开门判定器 judge_open(item_no)：未登记项 → ENOENT；清单段越闭集 →
//!    EINVAL；衔接目标悬空 → ESRCH；重复开门 → EALREADY。四码判定次序
//!    冻结（ENOENT 先于 EINVAL 先于 ESRCH 先于 EALREADY），次序本身
//!    可点验——构造双违约样本必须返回序在前的码；
//! 3. 衔接零悬空：衔接目标号对号面闭窗全扫描（scan_dangling），登记表
//!    内任一行目标号必须对得上表内其他项号（0 = 无衔接豁免），悬空目标
//!    必被抓 10/10——注入器把合法目标改写成表外野号，全扫描逐轮命中；
//! 4. 开门状态机 Closed→Armed→Opened 单调只进不退，倒退显性拒
//!    （已开门再 arm、再 open_all、已武装再 arm 均回 ERewind）；
//! 5. 注入器三类（悬空衔接/清单段越界/倒退开门）各 10 轮，固定 LCG
//!    选点，每轮违约必须被对应闸口抓住，逃脱即判据失效；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子 = SEG_BASE）；
//!    取值一律 match/if let/.get()/checked_*/saturating_*，无
//!    unwrap/expect/panic/裸下标；双跑一致指纹（FNV 折叠）全程确定。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12776 − 12600) = 0x82C00）。
pub const SEG_BASE: u32 = 0x82C00;

// ── 开门件诊断码闭集 ─────────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateDiag {
    /// 未登记项：项号不在域收官清单登记表内。
    ENoent,
    /// 清单段越闭集：登记行的清单段字节不是 Regression|Sealed|Handoff。
    EInval,
    /// 衔接目标悬空：衔接目标号对不上表内任何项号。
    Esrch,
    /// 重复开门：该项已在本次开门流程中开过。
    EAlready,
    /// 状态机倒退：Closed→Armed→Opened 只进不退，倒退/重复推进被拒。
    ERewind,
    /// 判定次序违约：未武装先开门，或开门流程未按次序推进。
    EOrder,
    /// 扫描不净：衔接零悬空全扫描仍有悬空行，禁止武装/开门。
    EScan,
    /// 容量耗尽：开门登记簿已达容量上限，超开被拒。
    ECap,
    /// 双跑指纹不一致：两次全流程指纹不等，确定面失效。
    EFingerprint,
    /// 段位守卫：段基址对齐或窗口越界。
    ESeg,
}

impl GateDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                GateDiag::ENoent => 0x01,
                GateDiag::EInval => 0x02,
                GateDiag::Esrch => 0x03,
                GateDiag::EAlready => 0x04,
                GateDiag::ERewind => 0x05,
                GateDiag::EOrder => 0x06,
                GateDiag::EScan => 0x07,
                GateDiag::ECap => 0x08,
                GateDiag::EFingerprint => 0x09,
                GateDiag::ESeg => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            GateDiag::ENoent => "未登记项：项号不在域收官清单登记表内，先对表补登记再开门",
            GateDiag::EInval => "清单段越闭集：清单段字节只认 0x01 Regression/0x02 Sealed/0x03 Handoff，回修登记行",
            GateDiag::Esrch => "衔接目标悬空：衔接目标号对不上表内项号，先补目标行或清零衔接",
            GateDiag::EAlready => "重复开门：该项已开过，去重后再走开门口，勿重复登记",
            GateDiag::ERewind => "状态机倒退：Closed→Armed→Opened 只进不退，倒退即开门流程可疑，回放全流程",
            GateDiag::EOrder => "判定次序违约：先 arm 武装再开门，Closed 态直接判项属次序颠倒",
            GateDiag::EScan => "扫描不净：衔接零悬空全扫描仍有悬空行，先修悬空目标再武装",
            GateDiag::ECap => "容量耗尽：开门登记簿已满，超开拒绝，扩容或分批收官",
            GateDiag::EFingerprint => "双跑指纹不一致：两次全流程指纹不等，查非确定源后复测",
            GateDiag::ESeg => "段位守卫：段基址须对齐 0x40 且落在 0x80000..=0x831C0 窗口",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const GATE_ALL: &[GateDiag] = &[
    GateDiag::ENoent,
    GateDiag::EInval,
    GateDiag::Esrch,
    GateDiag::EAlready,
    GateDiag::ERewind,
    GateDiag::EOrder,
    GateDiag::EScan,
    GateDiag::ECap,
    GateDiag::EFingerprint,
    GateDiag::ESeg,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in GATE_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82C01..0x82C0A 闭集内"
}

// ── 清单段闭集 ───────────────────────────────────────────────────
/// 清单段闭集：Regression（复测清单）/ Sealed（封账清单）/ Handoff（交接清单）。
/// 判据锚定"体例/硬闸/复测三段"——三段闭集即域收官清单的段位制度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecklistKind {
    /// 复测清单段：判据复测与注入必抓条目。
    Regression,
    /// 封账清单段：已封账不再改动的收官条目。
    Sealed,
    /// 交接清单段：跨域交接与目标衔接条目。
    Handoff,
}

impl ChecklistKind {
    /// 闭集字节（0x01..0x03，其余一律越闭集）。
    pub fn byte(self) -> u8 {
        match self {
            ChecklistKind::Regression => 0x01,
            ChecklistKind::Sealed => 0x02,
            ChecklistKind::Handoff => 0x03,
        }
    }

    /// 由字节反查闭集段（越界返回 None，供 EINVAL 判定）。
    pub fn from_byte(b: u8) -> Option<ChecklistKind> {
        match b {
            0x01 => Some(ChecklistKind::Regression),
            0x02 => Some(ChecklistKind::Sealed),
            0x03 => Some(ChecklistKind::Handoff),
            _ => None,
        }
    }

    /// 段中文名。
    pub fn name(self) -> &'static str {
        match self {
            ChecklistKind::Regression => "复测",
            ChecklistKind::Sealed => "封账",
            ChecklistKind::Handoff => "交接",
        }
    }
}

/// 清单段闭集数组。
pub const KIND_ALL: &[ChecklistKind] = &[
    ChecklistKind::Regression,
    ChecklistKind::Sealed,
    ChecklistKind::Handoff,
];

// ── 衔接旗标位闭集 ───────────────────────────────────────────────
/// 旗标 bit0：单向指向衔接。
pub const LINK_PT: u8 = 0b001;
/// 旗标 bit1：双向衔接（两端互认）。
pub const LINK_BIDI: u8 = 0b010;
/// 旗标 bit2：已核验衔接（人工点验留痕）。
pub const LINK_VERIFIED: u8 = 0b100;
/// 旗标闭集掩码：越出三位的一律是坏旗标。
pub const FLAG_CLOSED_MASK: u8 = 0b111;
/// 注入用野号：必不在登记表项号闭集内的悬空目标。
pub const DANGLING_ID: u32 = 0x7F_0001;
/// 注入用坏段字节：必越清单段闭集。
pub const KIND_BAD: u8 = 0x7F;

// ── 域收官清单登记表（冻结 ≥24 行） ─────────────────────────────
/// 登记行四元组：清单项号/清单段（裸字节，判定时验闭集）/衔接目标号/衔接旗标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateRow {
    /// 清单项号（表内唯一递增）。
    pub item_no: u32,
    /// 清单段裸字节（0x01..0x03 闭集，越闭集行在判定时吃 EINVAL）。
    pub kind_raw: u8,
    /// 衔接目标号（0 = 无衔接；非零必须对上表内其他项号）。
    pub target: u32,
    /// 衔接旗标（LINK_* 位闭集；目标号为零时旗标必须为零）。
    pub flag: u8,
}

/// 域收官清单登记表：26 行冻结（≥24 行下限），项号 12701..12726 唯一递增。
///
/// 结构：复测段 10 行（12701..12710）逐行衔接到封账段；封账段 8 行
/// （12711..12718）逐行衔接到交接段；交接段 8 行（12719..12726）为域内
/// 收官行——除 12722 域内互衔 12726 外一律无外衔接（目标号为零、旗标为零）。
/// 全表衔接零悬空：任一非零目标号均可对上表内项号。
pub const REG_ALL: &[GateRow] = &[
    // 复测段（Regression）：判据复测与注入必抓条目。
    GateRow { item_no: 12701, kind_raw: 0x01, target: 12711, flag: LINK_PT | LINK_VERIFIED },
    GateRow { item_no: 12702, kind_raw: 0x01, target: 12712, flag: LINK_PT | LINK_VERIFIED },
    GateRow { item_no: 12703, kind_raw: 0x01, target: 12713, flag: LINK_PT | LINK_VERIFIED },
    GateRow { item_no: 12704, kind_raw: 0x01, target: 12714, flag: LINK_PT | LINK_VERIFIED },
    GateRow { item_no: 12705, kind_raw: 0x01, target: 12715, flag: LINK_PT | LINK_VERIFIED },
    GateRow { item_no: 12706, kind_raw: 0x01, target: 12711, flag: LINK_PT },
    GateRow { item_no: 12707, kind_raw: 0x01, target: 12716, flag: LINK_PT },
    GateRow { item_no: 12708, kind_raw: 0x01, target: 12717, flag: LINK_PT },
    GateRow { item_no: 12709, kind_raw: 0x01, target: 12718, flag: LINK_PT },
    GateRow { item_no: 12710, kind_raw: 0x01, target: 12715, flag: LINK_PT | LINK_BIDI },
    // 封账段（Sealed）：已封账收官条目，逐行交接给交接段。
    GateRow { item_no: 12711, kind_raw: 0x02, target: 12719, flag: LINK_PT | LINK_VERIFIED },
    GateRow { item_no: 12712, kind_raw: 0x02, target: 12720, flag: LINK_PT | LINK_VERIFIED },
    GateRow { item_no: 12713, kind_raw: 0x02, target: 12721, flag: LINK_PT | LINK_VERIFIED },
    GateRow { item_no: 12714, kind_raw: 0x02, target: 12722, flag: LINK_PT | LINK_VERIFIED },
    GateRow { item_no: 12715, kind_raw: 0x02, target: 12723, flag: LINK_PT | LINK_VERIFIED },
    GateRow { item_no: 12716, kind_raw: 0x02, target: 12724, flag: LINK_PT },
    GateRow { item_no: 12717, kind_raw: 0x02, target: 12725, flag: LINK_PT },
    GateRow { item_no: 12718, kind_raw: 0x02, target: 12726, flag: LINK_PT },
    // 交接段（Handoff）：域内收官行，无外衔接为主，12722 域内互衔。
    GateRow { item_no: 12719, kind_raw: 0x03, target: 0, flag: 0 },
    GateRow { item_no: 12720, kind_raw: 0x03, target: 0, flag: 0 },
    GateRow { item_no: 12721, kind_raw: 0x03, target: 0, flag: 0 },
    GateRow { item_no: 12722, kind_raw: 0x03, target: 12726, flag: LINK_PT | LINK_BIDI },
    GateRow { item_no: 12723, kind_raw: 0x03, target: 0, flag: 0 },
    GateRow { item_no: 12724, kind_raw: 0x03, target: 0, flag: 0 },
    GateRow { item_no: 12725, kind_raw: 0x03, target: 0, flag: 0 },
    GateRow { item_no: 12726, kind_raw: 0x03, target: 0, flag: 0 },
];

/// 登记表行数下限（判据冻结闭集 ≥24 行）。
pub const REG_MIN_ROWS: usize = 24;
/// 开门登记簿默认容量：与登记表行数一致，全表开门恰好装满。
pub const OPEN_CAP: usize = REG_ALL.len();

// ── 开门状态机：Closed→Armed→Opened 单调 ────────────────────────
/// 开门状态机三态：闭门 → 武装 → 开门，rank 只许前进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateState {
    /// 闭门：仅允许武装（arm），判项属次序违约。
    Closed,
    /// 武装：衔接零悬空扫描通过，允许逐项开门。
    Armed,
    /// 开门：全流程收官，只读。
    Opened,
}

impl GateState {
    /// 状态序（只许前进，倒退显性拒）。
    pub fn rank(self) -> u8 {
        match self {
            GateState::Closed => 0,
            GateState::Armed => 1,
            GateState::Opened => 2,
        }
    }
}

// ── 开门件账板 ───────────────────────────────────────────────────
/// 域收官清单开门账板：登记行副本 + 开门登记簿 + 状态机。
#[derive(Clone, Debug)]
pub struct GateBoard {
    /// 登记行（自冻结表复制的可注入副本，注入器在此构造违约样本）。
    rows: Vec<GateRow>,
    /// 开门登记簿（已开门项号，按开门序）。
    opened: Vec<u32>,
    /// 开门状态机。
    state: GateState,
    /// 开门登记簿容量。
    cap: usize,
}

impl GateBoard {
    /// 按冻结登记表开一块默认账板（容量 = 登记表行数）。
    pub fn new() -> GateBoard {
        GateBoard {
            rows: REG_ALL.to_vec(),
            opened: Vec::new(),
            state: GateState::Closed,
            cap: OPEN_CAP,
        }
    }

    /// 指定容量的账板（容量耗尽注入与边界用）。
    pub fn with_cap(cap: usize) -> GateBoard {
        GateBoard {
            rows: REG_ALL.to_vec(),
            opened: Vec::new(),
            state: GateState::Closed,
            cap,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> GateState {
        self.state
    }

    /// 登记行数。
    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    /// 已开门项数。
    pub fn opened_len(&self) -> usize {
        self.opened.len()
    }

    /// 开门登记簿容量。
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 取开门簿第 idx 项（越界 None）。
    pub fn opened_at(&self, idx: usize) -> Option<u32> {
        self.opened.get(idx).copied()
    }

    /// 按项号查登记行。
    pub fn row_of(&self, item_no: u32) -> Option<GateRow> {
        for r in self.rows.iter() {
            if r.item_no == item_no {
                return Some(*r);
            }
        }
        None
    }

    /// 该项是否已开门。
    pub fn is_opened(&self, item_no: u32) -> bool {
        self.opened.contains(&item_no)
    }

    /// 衔接零悬空全扫描：逐行对号，返回悬空行的项号表（空表 = 零悬空）。
    pub fn scan_dangling(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for r in self.rows.iter() {
            if r.target != 0 && self.row_of(r.target).is_none() && !out.contains(&r.item_no) {
                out.push(r.item_no);
            }
        }
        out
    }

    /// 武装：闭门态 + 衔接零悬空扫描通过才许武装；已武装/已开门再 arm 即倒退拒。
    pub fn arm(&mut self) -> Result<(), u32> {
        if self.state != GateState::Closed {
            return Err(GateDiag::ERewind.val());
        }
        if !self.scan_dangling().is_empty() {
            return Err(GateDiag::EScan.val());
        }
        if self.rows_len() < REG_MIN_ROWS {
            return Err(GateDiag::EScan.val());
        }
        self.state = GateState::Armed;
        Ok(())
    }

    /// 开门判定器：四码次序冻结——
    /// 未武装 EOrder → 未登记 ENoent → 段越闭集 EINVAL → 目标悬空 ESRCH
    /// → 重复开门 EALREADY → 容量耗尽 ECap。
    pub fn judge_open(&mut self, item_no: u32) -> Result<GateRow, u32> {
        // 次序 0：闭门态判项属次序违约（先 arm 再开门）。
        if self.state == GateState::Closed {
            return Err(GateDiag::EOrder.val());
        }
        // 次序 1：未登记项 → ENOENT。
        let row = match self.row_of(item_no) {
            Some(r) => r,
            None => return Err(GateDiag::ENoent.val()),
        };
        // 次序 2：清单段越闭集 → EINVAL。
        if ChecklistKind::from_byte(row.kind_raw).is_none() {
            return Err(GateDiag::EInval.val());
        }
        // 次序 3：衔接目标悬空 → ESRCH（目标号为零 = 无衔接，豁免）。
        if row.target != 0 && self.row_of(row.target).is_none() {
            return Err(GateDiag::Esrch.val());
        }
        // 次序 4：重复开门 → EALREADY。
        if self.is_opened(item_no) {
            return Err(GateDiag::EAlready.val());
        }
        // 容量闸：开门登记簿满即拒。
        if self.opened.len() >= self.cap {
            return Err(GateDiag::ECap.val());
        }
        self.opened.push(item_no);
        Ok(row)
    }

    /// 全表开门：武装态 + 零悬空 + 逐行判定全过才许收官。
    pub fn open_all(&mut self) -> Result<usize, u32> {
        // 已开门再 open_all 即倒退拒。
        if self.state.rank() >= GateState::Opened.rank() {
            return Err(GateDiag::ERewind.val());
        }
        // 闭门态 open_all 属次序违约。
        if self.state != GateState::Armed {
            return Err(GateDiag::EOrder.val());
        }
        // 衔接零悬空前置闸。
        if !self.scan_dangling().is_empty() {
            return Err(GateDiag::EScan.val());
        }
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.rows.len() {
            let item = match self.rows.get(i) {
                Some(r) => r.item_no,
                None => break,
            };
            if !self.is_opened(item) {
                match self.judge_open(item) {
                    Ok(_) => n += 1,
                    Err(e) => return Err(e),
                }
            }
            i += 1;
        }
        self.state = GateState::Opened;
        Ok(n)
    }

    // ── 注入原语（仅注入器与测试面使用，正常流不得调用） ────────
    /// 注入悬空衔接：把该项的衔接目标改写为表外野号，返回是否命中行。
    pub fn corrupt_target(&mut self, item_no: u32, new_target: u32) -> bool {
        let mut hit = false;
        for r in self.rows.iter_mut() {
            if r.item_no == item_no {
                r.target = new_target;
                hit = true;
            }
        }
        hit
    }

    /// 注入清单段越界：把该项的清单段字节改写为坏字节，返回是否命中行。
    pub fn corrupt_kind(&mut self, item_no: u32, new_kind: u8) -> bool {
        let mut hit = false;
        for r in self.rows.iter_mut() {
            if r.item_no == item_no {
                r.kind_raw = new_kind;
                hit = true;
            }
        }
        hit
    }
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
        // LCG 常数取 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
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

// ── 双跑一致指纹（FNV 折叠，全程确定） ───────────────────────────
/// 账板指纹：登记行四元组 + 开门簿 + 状态序，逐项折叠成 32 位指纹。
pub fn board_fingerprint(b: &GateBoard) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    // 折叠登记行。
    let mut i = 0usize;
    while i < b.rows_len() {
        if let Some(r) = b.row_of_by_idx(i) {
            h = h.wrapping_mul(0x0100_0193) ^ r.item_no;
            h = h.wrapping_mul(0x0100_0193) ^ (r.kind_raw as u32);
            h = h.wrapping_mul(0x0100_0193) ^ r.target;
            h = h.wrapping_mul(0x0100_0193) ^ (r.flag as u32);
        }
        i += 1;
    }
    // 折叠开门簿。
    let mut j = 0usize;
    while j < b.opened_len() {
        if let Some(v) = b.opened_at(j) {
            h = h.wrapping_mul(0x0100_0193) ^ v;
        }
        j += 1;
    }
    // 折叠状态序。
    h = h.wrapping_mul(0x0100_0193) ^ (b.state().rank() as u32);
    h
}

impl GateBoard {
    /// 按行位取登记行（越界 None；供指纹折叠，不暴露裸下标）。
    pub fn row_of_by_idx(&self, idx: usize) -> Option<GateRow> {
        self.rows.get(idx).copied()
    }
}

// ── 正常流：全流程收官 ───────────────────────────────────────────
/// 全流程：闭门 → 武装 → 全表开门 → Opened，返回收官账板。
pub fn run_pipeline() -> GateBoard {
    let mut b = GateBoard::new();
    match b.arm() {
        Ok(()) => {}
        Err(_) => return b,
    }
    let _ = b.open_all();
    b
}

// ── 注入器三类（各 10 轮，固定 LCG 选点） ────────────────────────
/// 注入器一：悬空衔接注入 10 轮——把 LCG 选中的行目标改写为表外野号，
/// 全扫描必须报悬空且判项必须吃 ESRCH，逃脱即判据失效。
/// 返回 (抓到轮数, 总轮数, 首个逃脱轮描述)。
pub fn inject_dangling_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x11) as u64);
    let mut k = 0usize;
    while k < 10 {
        // LCG 选行：项号从冻结表按位取（越位回落首行）。
        let pick = rng.below(REG_ALL.len() as u32) as usize;
        let item = match REG_ALL.get(pick) {
            Some(r) => r.item_no,
            None => match REG_ALL.get(0) {
                Some(r0) => r0.item_no,
                None => break,
            },
        };
        let mut b = GateBoard::new();
        // 先武装再注入：悬空行本就过不了武装闸（EScan），故抓法走
        // 武装后注入——注入后全扫描必须报悬空，判项必须吃 ESRCH。
        let armed = b.arm().is_ok();
        let hit = b.corrupt_target(item, DANGLING_ID);
        let scan_hit = !b.scan_dangling().is_empty();
        let judge_hit = b.judge_open(item) == Err(GateDiag::Esrch.val());
        total += 1;
        if armed && hit && scan_hit && judge_hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("悬空衔接注入逃脱：轮{} 项{}", k, item);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入器二：清单段越界注入 10 轮——把 LCG 选中行的段字节改写为坏字节，
/// 判项必须吃 EINVAL（旗标面与扫描面不得放过坏段行）。
pub fn inject_inval_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x22) as u64);
    let mut k = 0usize;
    while k < 10 {
        let pick = rng.below(REG_ALL.len() as u32) as usize;
        let item = match REG_ALL.get(pick) {
            Some(r) => r.item_no,
            None => match REG_ALL.get(0) {
                Some(r0) => r0.item_no,
                None => break,
            },
        };
        let mut b = GateBoard::new();
        let hit = b.corrupt_kind(item, KIND_BAD);
        let mut judge_hit = false;
        if b.arm().is_ok() {
            judge_hit = b.judge_open(item) == Err(GateDiag::EInval.val());
        }
        total += 1;
        if hit && judge_hit {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("清单段越界注入逃脱：轮{} 项{}", k, item);
        }
        k += 1;
    }
    (caught, total, escape)
}

/// 注入器三：倒退开门注入 10 轮——收官后按 LCG 选路回撞状态机
/// （已开门再 arm / 已开门再 open_all / 已武装再 arm），每路必须吃 ERewind。
pub fn inject_rewind_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    let mut rng = Lcg::new(SEG_BASE.wrapping_add(0x33) as u64);
    let mut k = 0usize;
    while k < 10 {
        let path = rng.below(3);
        let mut b = run_pipeline();
        let rewind_caught = if b.state() != GateState::Opened {
            false
        } else if path == 0 {
            // 路一：已开门再 arm → 倒退拒。
            b.arm() == Err(GateDiag::ERewind.val())
        } else if path == 1 {
            // 路二：已开门再 open_all → 倒退拒。
            b.open_all() == Err(GateDiag::ERewind.val())
        } else {
            // 路三：先 arm 至武装态再 arm → 重复推进倒退拒。
            // 收官板不可再 arm（已是倒退态），改为构造武装板重入。
            let mut b2 = GateBoard::new();
            let _ = b2.arm();
            b2.arm() == Err(GateDiag::ERewind.val())
        };
        total += 1;
        if rewind_caught {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("倒退开门注入逃脱：轮{} 路{}", k, path);
        }
        k += 1;
    }
    (caught, total, escape)
}

// ── 深化指引档：体例/硬闸/复测三段点验 10/10 ─────────────────────
/// 深化指引档点验表：体例 3 条 + 硬闸 4 码 + 复测 3 条 = 10 条，
/// 每条独立复算，不复用被测值；判据 UNX-F12777-J1 的"10/10"即此表。
pub fn guide_checks() -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    // ── 体例段（3 条）────────────────────────────────────────────
    // 体例-1：登记表行数达冻结下限（≥24 行）。
    out.push(("体例-1 登记表≥24行".to_string(), REG_ALL.len() >= REG_MIN_ROWS));
    // 体例-2：项号唯一且逐行递增（行序冻结）。
    let mut ordered = REG_ALL.len() >= 1;
    let mut i = 1usize;
    while i < REG_ALL.len() {
        let prev = match REG_ALL.get(i - 1) {
            Some(r) => r.item_no,
            None => break,
        };
        let cur = match REG_ALL.get(i) {
            Some(r) => r.item_no,
            None => break,
        };
        if cur <= prev {
            ordered = false;
        }
        i += 1;
    }
    out.push(("体例-2 项号唯一递增".to_string(), ordered));
    // 体例-3：清单段三段闭集各非空且全表段字节合法。
    let mut seg_reg = false;
    let mut seg_sealed = false;
    let mut seg_hand = false;
    let mut kind_ok = true;
    for r in REG_ALL.iter() {
        match ChecklistKind::from_byte(r.kind_raw) {
            Some(ChecklistKind::Regression) => seg_reg = true,
            Some(ChecklistKind::Sealed) => seg_sealed = true,
            Some(ChecklistKind::Handoff) => seg_hand = true,
            None => kind_ok = false,
        }
    }
    out.push((
        "体例-3 三段闭集齐".to_string(),
        seg_reg && seg_sealed && seg_hand && kind_ok && KIND_ALL.len() == 3,
    ));
    // ── 硬闸段（4 码）────────────────────────────────────────────
    // 硬闸-1：未登记项 → ENOENT。
    let mut b1 = GateBoard::new();
    let _ = b1.arm();
    let h1 = b1.judge_open(0x7F_7F7F) == Err(GateDiag::ENoent.val());
    out.push(("硬闸-1 ENOENT".to_string(), h1));
    // 硬闸-2：清单段越闭集 → EINVAL。
    let mut b2 = GateBoard::new();
    let _ = b2.corrupt_kind(12701, KIND_BAD);
    let _ = b2.arm();
    let h2 = b2.judge_open(12701) == Err(GateDiag::EInval.val());
    out.push(("硬闸-2 EINVAL".to_string(), h2));
    // 硬闸-3：衔接目标悬空 → ESRCH（先武装再注入：悬空行过不了武装闸）。
    let mut b3 = GateBoard::new();
    let _ = b3.arm();
    let _ = b3.corrupt_target(12702, DANGLING_ID);
    let h3 = b3.judge_open(12702) == Err(GateDiag::Esrch.val());
    out.push(("硬闸-3 ESRCH".to_string(), h3));
    // 硬闸-4：重复开门 → EALREADY。
    let mut b4 = GateBoard::new();
    let _ = b4.arm();
    let first = b4.judge_open(12701).is_ok();
    let h4 = first && b4.judge_open(12701) == Err(GateDiag::EAlready.val());
    out.push(("硬闸-4 EALREADY".to_string(), h4));
    // ── 复测段（3 条）────────────────────────────────────────────
    // 复测-1：三类注入各 10 轮全抓，零逃脱。
    out.push(("复测-1 注入30轮全抓".to_string(), inject_sweeps_ok()));
    // 复测-2：双跑指纹一致。
    out.push(("复测-2 双跑指纹一致".to_string(), dual_run_ok()));
    // 复测-3：状态机单调全绿（Closed→Armed→Opened 无倒退）。
    out.push(("复测-3 状态机单调".to_string(), state_mono_ok()));
    out
}

/// 深化指引档 10/10 全绿判定。
pub fn guide_all_green() -> bool {
    let list = guide_checks();
    if list.len() != 10 {
        return false;
    }
    for item in list.iter() {
        let _name = &item.0;
        if !item.1 {
            return false;
        }
    }
    true
}

// ── 判据谓词（私有，断言方向：cond 为违约条件） ─────────────────
/// 段位守卫：SEG 对齐 0x40 且落在 0x80000..=0x831C0 窗口。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：全部落在 [SEG_BASE, SEG_BASE+0x40) 窗口且查表可命中。
fn diag_closed_ok() -> bool {
    for d in GATE_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    // 已知码必须命中文案，未知码必须回落。
    if detail(GateDiag::ENoent.val()) != GateDiag::ENoent.detail() {
        return false;
    }
    if detail(SEG_BASE + 0x3F) == GateDiag::ENoent.detail() {
        return false;
    }
    true
}

/// 登记表闭集守卫：行数下限 + 项号唯一递增 + 段字节合法 + 旗标位闭集
/// + 衔接零悬空（非零目标必须对上表内项号；零目标必须零旗标）。
fn registry_closed_ok() -> bool {
    if REG_ALL.len() < REG_MIN_ROWS {
        return false;
    }
    // 项号唯一递增。
    let mut i = 1usize;
    while i < REG_ALL.len() {
        let prev = match REG_ALL.get(i - 1) {
            Some(r) => r.item_no,
            None => return false,
        };
        let cur = match REG_ALL.get(i) {
            Some(r) => r.item_no,
            None => return false,
        };
        if cur <= prev {
            return false;
        }
        i += 1;
    }
    // 逐行：段字节合法、旗标位闭集、衔接语义自洽。
    for r in REG_ALL.iter() {
        if ChecklistKind::from_byte(r.kind_raw).is_none() {
            return false;
        }
        if (r.flag & !FLAG_CLOSED_MASK) != 0 {
            return false;
        }
        if r.target == 0 && r.flag != 0 {
            return false;
        }
        if r.target != 0 {
            if r.flag == 0 {
                return false;
            }
            // 对号面：目标必须对上表内其他项号。
            let mut found = false;
            for t in REG_ALL.iter() {
                if t.item_no == r.target {
                    found = true;
                }
            }
            if !found {
                return false;
            }
        }
    }
    // 账板副本扫描同样零悬空。
    let b = GateBoard::new();
    b.scan_dangling().is_empty()
}

/// 正常流：武装 → 全表开门 → Opened，26 行全开。
fn happy_path_ok() -> bool {
    let b = run_pipeline();
    if b.state() != GateState::Opened {
        return false;
    }
    if b.opened_len() != REG_ALL.len() {
        return false;
    }
    // 首行与尾行都必须在开门簿内。
    let first = match REG_ALL.get(0) {
        Some(r) => r.item_no,
        None => return false,
    };
    let last = match REG_ALL.get(REG_ALL.len() - 1) {
        Some(r) => r.item_no,
        None => return false,
    };
    b.is_opened(first) && b.is_opened(last)
}

/// 判定次序：四码次序冻结，双违约样本必须返回序在前的码。
fn judge_order_ok() -> bool {
    // 未武装判项 → EOrder（次序 0 最先，压过一切码）。
    let mut b0 = GateBoard::new();
    if b0.judge_open(12701) != Err(GateDiag::EOrder.val()) {
        return false;
    }
    // 未登记项压过段越闭集：野号在坏段板上仍吃 ENOENT。
    let mut b1 = GateBoard::new();
    let _ = b1.corrupt_kind(12701, KIND_BAD);
    let _ = b1.arm();
    if b1.judge_open(0x7F_7F7F) != Err(GateDiag::ENoent.val()) {
        return false;
    }
    // 段越闭集压过重复开门：坏段行已开过再判仍吃 EINVAL。
    let mut b2 = GateBoard::new();
    let _ = b2.arm();
    let _ = b2.judge_open(12701);
    let _ = b2.corrupt_kind(12701, KIND_BAD);
    if b2.judge_open(12701) != Err(GateDiag::EInval.val()) {
        return false;
    }
    // 目标悬空压过重复开门：悬空行已开过再判仍吃 ESRCH。
    let mut b3 = GateBoard::new();
    let _ = b3.arm();
    let _ = b3.judge_open(12701);
    let _ = b3.corrupt_target(12701, DANGLING_ID);
    if b3.judge_open(12701) != Err(GateDiag::Esrch.val()) {
        return false;
    }
    // 单违约样本各归其码。
    let mut b4 = GateBoard::new();
    let _ = b4.arm();
    let _ = b4.judge_open(12701);
    if b4.judge_open(12701) != Err(GateDiag::EAlready.val()) {
        return false;
    }
    true
}

/// 状态机单调：三态 rank 递增，倒退路径全数拒绝。
fn state_mono_ok() -> bool {
    if !(GateState::Closed.rank() < GateState::Armed.rank()
        && GateState::Armed.rank() < GateState::Opened.rank())
    {
        return false;
    }
    // 已开门再 arm → ERewind。
    let mut b = run_pipeline();
    if b.arm() != Err(GateDiag::ERewind.val()) {
        return false;
    }
    // 已开门再 open_all → ERewind。
    if b.open_all() != Err(GateDiag::ERewind.val()) {
        return false;
    }
    // 已武装再 arm → ERewind（重复推进）。
    let mut b2 = GateBoard::new();
    let _ = b2.arm();
    if b2.arm() != Err(GateDiag::ERewind.val()) {
        return false;
    }
    // 闭门态 open_all → EOrder（次序违约）。
    let mut b3 = GateBoard::new();
    b3.open_all() == Err(GateDiag::EOrder.val())
}

/// 注入必抓：三类注入各 10 轮全抓，零逃脱。
fn inject_sweeps_ok() -> bool {
    let (c1, t1, e1) = inject_dangling_sweep();
    if t1 != 10 || c1 != 10 || !e1.is_empty() {
        return false;
    }
    let (c2, t2, e2) = inject_inval_sweep();
    if t2 != 10 || c2 != 10 || !e2.is_empty() {
        return false;
    }
    let (c3, t3, e3) = inject_rewind_sweep();
    if t3 != 10 || c3 != 10 || !e3.is_empty() {
        return false;
    }
    true
}

/// 双跑一致：两次全流程指纹一致，且注入器双跑逐项一致。
fn dual_run_ok() -> bool {
    let b1 = run_pipeline();
    let b2 = run_pipeline();
    if board_fingerprint(&b1) != board_fingerprint(&b2) {
        return false;
    }
    if b1.state() != b2.state() || b1.opened_len() != b2.opened_len() {
        return false;
    }
    let s1 = inject_dangling_sweep();
    let s2 = inject_dangling_sweep();
    if s1 != s2 {
        return false;
    }
    let s3 = inject_inval_sweep();
    let s4 = inject_inval_sweep();
    if s3 != s4 {
        return false;
    }
    let s5 = inject_rewind_sweep();
    let s6 = inject_rewind_sweep();
    s5 == s6
}

/// 容量耗尽：开门簿满后超开必拒且账面不变。
fn capacity_ok() -> bool {
    // 小容量板：开满两格后第三格必吃 ECap，账面停在 2。
    let mut b = GateBoard::with_cap(2);
    let _ = b.arm();
    if b.judge_open(12701).is_err() {
        return false;
    }
    if b.judge_open(12702).is_err() {
        return false;
    }
    if b.judge_open(12703) != Err(GateDiag::ECap.val()) {
        return false;
    }
    if b.opened_len() != 2 || b.cap() != 2 {
        return false;
    }
    // 默认板：容量与登记表行数一致，全表开门恰好装满。
    let mut full = GateBoard::new();
    if full.cap() != REG_ALL.len() {
        return false;
    }
    let _ = full.arm();
    match full.open_all() {
        Ok(n) => n == REG_ALL.len() && full.opened_len() == OPEN_CAP,
        Err(_) => false,
    }
}

/// 边界：首尾行、零号/极大野号、零衔接行、双向旗标语义。
fn boundary_ok() -> bool {
    // 零号与 u32 极大值都是野号 → ENOENT。
    let mut b = GateBoard::new();
    let _ = b.arm();
    if b.judge_open(0) != Err(GateDiag::ENoent.val()) {
        return false;
    }
    if b.judge_open(u32::MAX) != Err(GateDiag::ENoent.val()) {
        return false;
    }
    // 首行开门成功；尾行（无衔接交接行）开门成功。
    if b.judge_open(12701).is_err() {
        return false;
    }
    if b.judge_open(12726).is_err() {
        return false;
    }
    // 双向旗标行语义：12710 必持 BIDI 位，且旗标不出三位闭集。
    let r10 = match GateBoard::new().row_of(12710) {
        Some(r) => r,
        None => return false,
    };
    if (r10.flag & LINK_BIDI) == 0 || (r10.flag & !FLAG_CLOSED_MASK) != 0 {
        return false;
    }
    // 零衔接行语义：12719 目标为零旗标为零，开门不受衔接闸影响。
    let r19 = match GateBoard::new().row_of(12719) {
        Some(r) => r,
        None => return false,
    };
    r19.target == 0 && r19.flag == 0
}

/// 完成写回语义：收官态指纹非零且开门簿满，摘要非空。
fn writeback_ok() -> bool {
    let b = run_pipeline();
    if b.state() != GateState::Opened {
        return false;
    }
    let fp = board_fingerprint(&b);
    if fp == 0 {
        return false;
    }
    let summary = format!("opened={} fp=0x{:08x}", b.opened_len(), fp);
    !summary.is_empty()
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12776 自检（判据 12776 卡：深化指引档齐，体例/硬闸/复测三段 10/10）。
pub fn run_unx12776_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12776");
    let board = run_pipeline();

    s.add_dyn(
        "F12777-01 段位守卫",
        seg_guard_ok(),
        &format!(
            "seg=0x{:05x} align40={} win=[0x80000,0x831C0]",
            SEG_BASE,
            SEG_BASE % 0x40 == 0
        ),
    );

    s.add_dyn(
        "F12777-02 诊断码闭集",
        diag_closed_ok(),
        &format!(
            "n={} first=0x{:05x} last=0x{:05x}",
            GATE_ALL.len(),
            GATE_ALL.first().map(|d| d.val()).unwrap_or(0),
            GATE_ALL.last().map(|d| d.val()).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12777-03 登记表冻结闭集",
        registry_closed_ok(),
        &format!(
            "rows={} min={} 首项={} 尾项={}",
            REG_ALL.len(),
            REG_MIN_ROWS,
            REG_ALL.first().map(|r| r.item_no).unwrap_or(0),
            REG_ALL.last().map(|r| r.item_no).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12777-04 衔接零悬空全扫描",
        GateBoard::new().scan_dangling().is_empty(),
        &format!(
            "dangling={} targets_nonzero={}",
            GateBoard::new().scan_dangling().len(),
            REG_ALL.iter().filter(|r| r.target != 0).count()
        ),
    );

    s.add_dyn(
        "F12777-05 正常流全表开门",
        happy_path_ok(),
        &format!(
            "opened={} state={:?}",
            board.opened_len(),
            board.state()
        ),
    );

    s.add_dyn(
        "F12777-06 判定次序四码冻结",
        judge_order_ok(),
        "EOrder→ENOENT→EINVAL→ESRCH→EALREADY 双违约样本逐对点验",
    );

    s.add_dyn(
        "F12777-07 状态机单调倒退拒",
        state_mono_ok(),
        "Closed→Armed→Opened rank 递增，arm/open_all 倒退三路全拒",
    );

    let (c1, t1, e1) = inject_dangling_sweep();
    s.add_dyn(
        "F12777-08 注入-悬空衔接 10 轮全抓",
        c1 == 10 && t1 == 10 && e1.is_empty(),
        &format!("caught={}/{} {}", c1, t1, e1),
    );

    let (c2, t2, e2) = inject_inval_sweep();
    s.add_dyn(
        "F12777-09 注入-清单段越界 10 轮全抓",
        c2 == 10 && t2 == 10 && e2.is_empty(),
        &format!("caught={}/{} {}", c2, t2, e2),
    );

    let (c3, t3, e3) = inject_rewind_sweep();
    s.add_dyn(
        "F12777-10 注入-倒退开门 10 轮全抓",
        c3 == 10 && t3 == 10 && e3.is_empty(),
        &format!("caught={}/{} {}", c3, t3, e3),
    );

    s.add_dyn(
        "F12777-11 双跑一致指纹",
        dual_run_ok(),
        &format!(
            "fp=0x{:08x} 两次全流程与三注入器逐项一致",
            board_fingerprint(&board)
        ),
    );

    s.add_dyn(
        "F12777-12 容量耗尽",
        capacity_ok(),
        &format!("cap={} full_opened={}", OPEN_CAP, board.opened_len()),
    );

    s.add_dyn(
        "F12777-13 边界首尾与野号",
        boundary_ok(),
        "零号/极大野号 ENOENT、首尾行开门、双向旗标、零衔接行 四点",
    );

    // 深化指引档 10/10：判据 UNX-F12777-J1 的可点验面。
    s.add_dyn(
        "F12777-14 深化指引档 10/10",
        guide_all_green(),
        &format!(
            "green={}/{} 体例3+硬闸4+复测3",
            guide_checks().iter().filter(|x| x.1).count(),
            guide_checks().len()
        ),
    );

    s.add_dyn(
        "F12777-15 完成写回语义",
        writeback_ok(),
        &format!(
            "opened={} fp=0x{:08x} state={:?}",
            board.opened_len(),
            board_fingerprint(&board),
            board.state()
        ),
    );

    // 收口：判据条数声明（前 15 条齐、零红才放行）。
    let n = s.len();
    s.add_dyn(
        "F12777-16 收口 判据条数声明",
        n == 15 && s.all_passed(),
        &format!("判据条数={} 预期=15 红={}", n, s.tally().1),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 0x80000..=0x831C0 窗口。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
        assert_eq!(SEG_BASE, 0x82C00);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：10 个码互异、全部落窗口、查表可命中。
        assert_eq!(GATE_ALL.len(), 10);
        let mut i = 0usize;
        while i < GATE_ALL.len() {
            let vi = GATE_ALL.get(i).map(|d| d.val()).unwrap_or(0);
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < GATE_ALL.len() {
                let vj = GATE_ALL.get(j).map(|d| d.val()).unwrap_or(0);
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        // 未知码回落。
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn registry_frozen_table_invariants() {
        // 冻结登记表：≥24 行、项号唯一递增、段闭集、零悬空、旗标闭集。
        assert!(REG_ALL.len() >= 24);
        assert!(registry_closed_ok());
        let b = GateBoard::new();
        assert_eq!(b.rows_len(), REG_ALL.len());
        assert!(b.scan_dangling().is_empty());
        // 三段各非空。
        let mut reg = 0usize;
        let mut sealed = 0usize;
        let mut hand = 0usize;
        for r in REG_ALL.iter() {
            match ChecklistKind::from_byte(r.kind_raw) {
                Some(ChecklistKind::Regression) => reg += 1,
                Some(ChecklistKind::Sealed) => sealed += 1,
                Some(ChecklistKind::Handoff) => hand += 1,
                None => panic!("登记表混入越闭集段字节"),
            }
        }
        assert!(reg > 0 && sealed > 0 && hand > 0);
    }

    #[test]
    fn happy_path_and_state_machine_mono() {
        // 正常流：武装 → 全表开门 → Opened。
        let mut b = GateBoard::new();
        assert_eq!(b.state(), GateState::Closed);
        assert!(b.arm().is_ok());
        assert_eq!(b.state(), GateState::Armed);
        assert_eq!(b.open_all(), Ok(REG_ALL.len()));
        assert_eq!(b.state(), GateState::Opened);
        assert_eq!(b.opened_len(), REG_ALL.len());
        assert!(b.is_opened(12701) && b.is_opened(12726));
        // 倒退拒绝：已开门再 arm / 再 open_all 均 ERewind。
        assert_eq!(b.arm(), Err(GateDiag::ERewind.val()));
        assert_eq!(b.open_all(), Err(GateDiag::ERewind.val()));
        // rank 单调。
        assert!(GateState::Closed.rank() < GateState::Armed.rank());
        assert!(GateState::Armed.rank() < GateState::Opened.rank());
    }

    #[test]
    fn judge_order_precedence_frozen() {
        // 次序 0：闭门态判项 → EOrder。
        let mut b0 = GateBoard::new();
        assert_eq!(b0.judge_open(12701), Err(GateDiag::EOrder.val()));
        // 次序 1：未登记项压过坏段。
        let mut b1 = GateBoard::new();
        let _ = b1.corrupt_kind(12701, KIND_BAD);
        let _ = b1.arm();
        assert_eq!(b1.judge_open(0x7F_7F7F), Err(GateDiag::ENoent.val()));
        // 次序 2：坏段压过重复开门。
        let mut b2 = GateBoard::new();
        let _ = b2.arm();
        let _ = b2.judge_open(12701);
        let _ = b2.corrupt_kind(12701, KIND_BAD);
        assert_eq!(b2.judge_open(12701), Err(GateDiag::EInval.val()));
        // 次序 3：悬空压过重复开门。
        let mut b3 = GateBoard::new();
        let _ = b3.arm();
        let _ = b3.judge_open(12701);
        let _ = b3.corrupt_target(12701, DANGLING_ID);
        assert_eq!(b3.judge_open(12701), Err(GateDiag::Esrch.val()));
        // 次序 4：纯重复开门 → EALREADY。
        let mut b4 = GateBoard::new();
        let _ = b4.arm();
        assert!(b4.judge_open(12701).is_ok());
        assert_eq!(b4.judge_open(12701), Err(GateDiag::EAlready.val()));
        // 判据谓词同口径全绿。
        assert!(judge_order_ok());
    }

    #[test]
    fn injections_all_caught() {
        // 三类注入各 10 轮全抓，零逃脱。
        let (c1, t1, e1) = inject_dangling_sweep();
        assert_eq!(t1, 10);
        assert_eq!(c1, 10, "悬空衔接逃脱：{}", e1);
        let (c2, t2, e2) = inject_inval_sweep();
        assert_eq!(t2, 10);
        assert_eq!(c2, 10, "清单段越界逃脱：{}", e2);
        let (c3, t3, e3) = inject_rewind_sweep();
        assert_eq!(t3, 10);
        assert_eq!(c3, 10, "倒退开门逃脱：{}", e3);
        assert!(inject_sweeps_ok());
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：两次全流程指纹相同，注入器双跑逐项相同。
        assert!(dual_run_ok());
        let a = run_pipeline();
        let b = run_pipeline();
        assert_eq!(board_fingerprint(&a), board_fingerprint(&b));
        assert_eq!(a.state(), b.state());
        assert_eq!(a.opened_len(), b.opened_len());
        assert_eq!(inject_dangling_sweep(), inject_dangling_sweep());
        assert_eq!(inject_inval_sweep(), inject_inval_sweep());
        assert_eq!(inject_rewind_sweep(), inject_rewind_sweep());
    }

    #[test]
    fn capacity_exhaustion_rejected() {
        // 容量耗尽：开满后超开必拒，账面停格。
        let mut b = GateBoard::with_cap(2);
        let _ = b.arm();
        assert!(b.judge_open(12701).is_ok());
        assert!(b.judge_open(12702).is_ok());
        assert_eq!(b.judge_open(12703), Err(GateDiag::ECap.val()));
        assert_eq!(b.opened_len(), 2);
        // 默认板容量与登记表行数一致，全表开门恰好装满。
        let mut full = GateBoard::new();
        assert_eq!(full.cap(), REG_ALL.len());
        let _ = full.arm();
        assert_eq!(full.open_all(), Ok(REG_ALL.len()));
        assert_eq!(full.opened_len(), OPEN_CAP);
        assert!(capacity_ok());
    }

    #[test]
    fn boundary_edges_hold() {
        // 野号：零号与极大值均 ENOENT。
        let mut b = GateBoard::new();
        let _ = b.arm();
        assert_eq!(b.judge_open(0), Err(GateDiag::ENoent.val()));
        assert_eq!(b.judge_open(u32::MAX), Err(GateDiag::ENoent.val()));
        // 首尾行开门成功；零衔接行不受衔接闸影响；双向旗标行语义成立。
        assert!(b.judge_open(12701).is_ok());
        assert!(b.judge_open(12726).is_ok());
        assert!(boundary_ok());
        // 注入原语命中面：改到不存在的项号返回 false。
        let mut b2 = GateBoard::new();
        assert!(!b2.corrupt_target(99999, DANGLING_ID));
        assert!(!b2.corrupt_kind(99999, KIND_BAD));
        assert!(b2.corrupt_target(12705, DANGLING_ID));
        assert_eq!(b2.scan_dangling(), alloc::vec![12705]);
    }

    #[test]
    fn guide_ledger_ten_of_ten() {
        // 深化指引档：恰 10 条、全绿。
        let list = guide_checks();
        assert_eq!(list.len(), 10);
        assert!(guide_all_green());
        // 三段各就位：体例 3 + 硬闸 4 + 复测 3。
        assert!(list.get(0).map(|x| x.0.contains("体例")).unwrap_or(false));
        assert!(list.get(3).map(|x| x.0.contains("硬闸")).unwrap_or(false));
        assert!(list.get(7).map(|x| x.0.contains("复测")).unwrap_or(false));
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条，红项为零。
        let s = run_unx12776_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
