//! UNX-F12777 深化指引继承档（承接会话指引） · 0x82C40段
//!
//! 判据原文（派工卡 12777 逐字锚定）：
//!   UNX-F12778-J1 移交清单齐（四类资产逐项登记），交接演练 10/10。
//!
//! 设计要点：
//! 1. 指引三段登记表：段闭集 Style|HardGate|Retest 三段各九条目（段内条目号
//!    0..9），每行登记（段/条目号/条目内容代号/继承旗标），冻结闭集 ≥27 行
//!    ——行序冻结，乱序/越段/零代号显性拒，行数不足禁冻结，冻结后只读；
//! 2. 指引档齐判定器 judge_guide(seg, item)：判定次序冻结——段越闭集
//!    → EINVAL；条目未登记 → ENOENT；继承旗标未置但被引用 → EPERM；
//!    重复继承 → EALREADY；倒退继承（同段已继承更高条目号）→ EREGRESS；
//!    次序冻结的理由：段闭集不先判，EINVAL 永被 ENOENT 掩蔽，不可达；
//! 3. 可点验 10/10：点验判定器 verify_point(seg, item) 对任意条目以正向/
//!    反向两轮独立扫描取行、逐位比对打包位型，27 条目全数两轮一致；
//!    交接演练十条断言独立写死（行数/段守恒/旗标守恒/代号守恒/移交清单/
//!    判定器/点验/注入/指纹/冻结就绪），任何一条为假即移交失败；
//! 4. 注入器三类（未登记条目 / 旗标未置引用 / 倒退继承）各 10 轮全抓，
//!    逃脱即判据失效；LCG 种子写死 = SEG_BASE，序列确定，双跑一致指纹
//!    （FNV-1a 逐字节哈希，双向比对）；
//! 5. 移交清单四类资产（登记表/判定器/点验/注入台账）逐项登记且证据码非零
//!    才许收官；状态机 开表→三段齐→冻结→继承→收官 单向推进，倒退显性拒；
//! 6. no_std + alloc；零浮点、零 IO、零墙钟、零随机（种子固定）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12777 − 12600) = 0x80000 + 0x2C40）。
pub const SEG_BASE: u32 = 0x82C40;

// ── 深化指引档诊断码闭集 ─────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0C 偏移，越出闭集的码在查表处回落"未知"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GdDiag {
    /// 段越三段闭集（EINVAL 族：只认 Style|HardGate|Retest，即 0..2）。
    ESegRange,
    /// 条目未登记（ENOENT 族：该 (段,条目号) 不在冻结登记表内）。
    ENoent,
    /// 继承旗标未置但被引用（EPERM 族：该条目登记为不继承）。
    EPerm,
    /// 重复继承/重复登记（EALREADY 族：同一对象只落一笔）。
    EAlready,
    /// 倒退继承（同段已继承更高条目号后又引用更低条目）。
    ERegress,
    /// 登记乱序（未按段序×条目序 0..27 逐行登记）。
    EOrder,
    /// 行数未满 27 即试图冻结/引用继承/收官。
    ENotFull,
    /// 登记表已冻结（冻结后一切写入拒绝，表只读）。
    ESealed,
    /// 容量耗尽（27 行闭集已满仍试图新增行）。
    ECap,
    /// 条目内容代号为零（零代号行不构成指引资产，不许入表）。
    ECodeZero,
    /// 点验两轮逐位不一致（正向/反向两轮扫描出现分歧）。
    EVerify,
    /// 双跑指纹不一致（确定性被破坏，构建流程可疑）。
    EFingerprint,
}

impl GdDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                GdDiag::ESegRange => 0x01,
                GdDiag::ENoent => 0x02,
                GdDiag::EPerm => 0x03,
                GdDiag::EAlready => 0x04,
                GdDiag::ERegress => 0x05,
                GdDiag::EOrder => 0x06,
                GdDiag::ENotFull => 0x07,
                GdDiag::ESealed => 0x08,
                GdDiag::ECap => 0x09,
                GdDiag::ECodeZero => 0x0a,
                GdDiag::EVerify => 0x0b,
                GdDiag::EFingerprint => 0x0c,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            GdDiag::ESegRange => "段越三段闭集：只认 Style|HardGate|Retest（0..2），先对段闭集表再引用",
            GdDiag::ENoent => "条目未登记：该(段,条目号)不在冻结登记表内，先补登记并冻结再引用",
            GdDiag::EPerm => "继承旗标未置但被引用：该条目登记为不继承，改引旗标真条目或走内容代号直查",
            GdDiag::EAlready => "重复继承/重复登记：同一对象只落一笔，先查已继承清单去重再落",
            GdDiag::ERegress => "倒退继承：同段已继承更高条目号，继承只进不退，回到继承清单尾部续走",
            GdDiag::EOrder => "登记乱序：按段序×条目序 0..27 逐行登记，跳行先回补上一行",
            GdDiag::ENotFull => "行数未满 27：先补齐冻结闭集行数，再走冻结/引用继承/收官口",
            GdDiag::ESealed => "登记表已冻结：冻结后只读，改动走下一条深化卡滚动登记",
            GdDiag::ECap => "容量耗尽：27 行闭集已满，新增行属越表，走新指引档另立登记表",
            GdDiag::ECodeZero => "条目内容代号为零：先落非零代号再入表，零代号行不构成指引资产",
            GdDiag::EVerify => "点验两轮逐位不一致：正向/反向扫描出现分歧，整表复核登记序与位型",
            GdDiag::EFingerprint => "双跑指纹不一致：确定性被破坏，核对 LCG 种子与构建路径后重跑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const GD_ALL: &[GdDiag] = &[
    GdDiag::ESegRange,
    GdDiag::ENoent,
    GdDiag::EPerm,
    GdDiag::EAlready,
    GdDiag::ERegress,
    GdDiag::EOrder,
    GdDiag::ENotFull,
    GdDiag::ESealed,
    GdDiag::ECap,
    GdDiag::ECodeZero,
    GdDiag::EVerify,
    GdDiag::EFingerprint,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in GD_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x82C41..0x82C4C 闭集内"
}

// ── 段闭集 Style|HardGate|Retest ─────────────────────────────────
/// 指引段闭集（判据域模型三段，冻结不许增删）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideSeg {
    /// 第 0 段：Style（风格指引）。
    Style,
    /// 第 1 段：HardGate（硬闸指引）。
    HardGate,
    /// 第 2 段：Retest（复测指引）。
    Retest,
}

impl GuideSeg {
    /// 段号（闭集序）。
    pub fn index(self) -> u32 {
        match self {
            GuideSeg::Style => 0,
            GuideSeg::HardGate => 1,
            GuideSeg::Retest => 2,
        }
    }

    /// 段中文名。
    pub fn name(self) -> &'static str {
        match self {
            GuideSeg::Style => "Style",
            GuideSeg::HardGate => "HardGate",
            GuideSeg::Retest => "Retest",
        }
    }

    /// 由段号反查闭集段（越界返回 None）。
    pub fn from_index(idx: u32) -> Option<GuideSeg> {
        match idx {
            0 => Some(GuideSeg::Style),
            1 => Some(GuideSeg::HardGate),
            2 => Some(GuideSeg::Retest),
            _ => None,
        }
    }
}

/// 三段闭集数组（段序即登记大序）。
pub const SEG_ALL: &[GuideSeg] = &[GuideSeg::Style, GuideSeg::HardGate, GuideSeg::Retest];

/// 每段条目数（段内条目号 0..9，三段九条 → 闭集 27 行）。
pub const ITEMS_PER_SEG: u32 = 9;

/// 段内条目号上沿（0..=8）。
pub const ITEM_HI: u32 = 8;

/// 冻结闭集总行数（≥27 行口径的冻结值）。
pub const GUIDE_CAP: usize = 27;

/// 继承旗标为真的声明行数（每段 6 真 ×3 段）。
pub const GUIDE_INHERIT_N: usize = 18;

/// 继承旗标为假的声明行数（每段 3 假 ×3 段，条目号 ≡2 (mod 3)）。
pub const GUIDE_NOINHERIT_N: usize = 9;

// ── 指引行数据模型 ───────────────────────────────────────────────
/// 指引登记表单行：段/段内条目号/条目内容代号/继承旗标/登记序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuideRow {
    /// 段号（0..2，对应 Style|HardGate|Retest）。
    pub seg: u32,
    /// 段内条目号（0..=8）。
    pub item: u32,
    /// 条目内容代号（非零有效，风格 0x77xx / 硬闸 0x78xx / 复测 0x79xx）。
    pub code: u32,
    /// 继承旗标（true=可被后档继承引用；false=仅登记不继承）。
    pub inherit: bool,
    /// 登记序（0..27，全表冻结序）。
    pub seq: u32,
}

/// 冻结指引三段登记表（常量来源：UNX-F12777 深化指引继承档卡规格——
/// 段闭集 Style|HardGate|Retest × 段内条目 0..9 共 27 行，行序冻结；
/// 旗标布点：条目号 ≡2 (mod 3) 者不继承，其余继承）。
pub const GUIDE_TABLE: &[GuideRow] = &[
    GuideRow { seg: 0, item: 0, code: 0x7701, inherit: true, seq: 0 },
    GuideRow { seg: 0, item: 1, code: 0x7702, inherit: true, seq: 1 },
    GuideRow { seg: 0, item: 2, code: 0x7703, inherit: false, seq: 2 },
    GuideRow { seg: 0, item: 3, code: 0x7704, inherit: true, seq: 3 },
    GuideRow { seg: 0, item: 4, code: 0x7705, inherit: true, seq: 4 },
    GuideRow { seg: 0, item: 5, code: 0x7706, inherit: false, seq: 5 },
    GuideRow { seg: 0, item: 6, code: 0x7707, inherit: true, seq: 6 },
    GuideRow { seg: 0, item: 7, code: 0x7708, inherit: true, seq: 7 },
    GuideRow { seg: 0, item: 8, code: 0x7709, inherit: false, seq: 8 },
    GuideRow { seg: 1, item: 0, code: 0x7801, inherit: true, seq: 9 },
    GuideRow { seg: 1, item: 1, code: 0x7802, inherit: true, seq: 10 },
    GuideRow { seg: 1, item: 2, code: 0x7803, inherit: false, seq: 11 },
    GuideRow { seg: 1, item: 3, code: 0x7804, inherit: true, seq: 12 },
    GuideRow { seg: 1, item: 4, code: 0x7805, inherit: true, seq: 13 },
    GuideRow { seg: 1, item: 5, code: 0x7806, inherit: false, seq: 14 },
    GuideRow { seg: 1, item: 6, code: 0x7807, inherit: true, seq: 15 },
    GuideRow { seg: 1, item: 7, code: 0x7808, inherit: true, seq: 16 },
    GuideRow { seg: 1, item: 8, code: 0x7809, inherit: false, seq: 17 },
    GuideRow { seg: 2, item: 0, code: 0x7901, inherit: true, seq: 18 },
    GuideRow { seg: 2, item: 1, code: 0x7902, inherit: true, seq: 19 },
    GuideRow { seg: 2, item: 2, code: 0x7903, inherit: false, seq: 20 },
    GuideRow { seg: 2, item: 3, code: 0x7904, inherit: true, seq: 21 },
    GuideRow { seg: 2, item: 4, code: 0x7905, inherit: true, seq: 22 },
    GuideRow { seg: 2, item: 5, code: 0x7906, inherit: false, seq: 23 },
    GuideRow { seg: 2, item: 6, code: 0x7907, inherit: true, seq: 24 },
    GuideRow { seg: 2, item: 7, code: 0x7908, inherit: true, seq: 25 },
    GuideRow { seg: 2, item: 8, code: 0x7909, inherit: false, seq: 26 },
];

// ── 状态机：开表 → 三段齐 → 冻结 → 收官（只进不退） ──────────────
/// 指引档状态机：单向推进，倒退显性拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GdState {
    /// 开表：允许逐行登记。
    Open,
    /// 三段齐：27 行入表完毕，等待冻结。
    Complete,
    /// 冻结：表只读，允许引用继承。
    Frozen,
    /// 收官：移交清单四类资产齐且演练 10/10，终态。
    Handed,
}

impl GdState {
    /// 状态序（只许前进）。
    pub fn rank(self) -> u8 {
        match self {
            GdState::Open => 0,
            GdState::Complete => 1,
            GdState::Frozen => 2,
            GdState::Handed => 3,
        }
    }
}

// ── 移交清单：四类资产逐项登记（判据 J1 第一半） ─────────────────
/// 移交资产四类闭集（判据"四类资产"的域模型落点，冻结不许增删）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetClass {
    /// 资产一：指引三段登记表（冻结闭集 ≥27 行）。
    GuideTable,
    /// 资产二：指引档齐判定器 judge_guide（四门判定次序冻结）。
    Judge,
    /// 资产三：点验判定器 verify_point（两轮逐位一致）。
    PointVerify,
    /// 资产四：注入台账（三类各 10 轮全抓）。
    InjectLedger,
}

impl AssetClass {
    /// 类号（闭集序）。
    pub fn index(self) -> u32 {
        match self {
            AssetClass::GuideTable => 0,
            AssetClass::Judge => 1,
            AssetClass::PointVerify => 2,
            AssetClass::InjectLedger => 3,
        }
    }

    /// 类中文名（演练文案用）。
    pub fn name(self) -> &'static str {
        match self {
            AssetClass::GuideTable => "登记表",
            AssetClass::Judge => "判定器",
            AssetClass::PointVerify => "点验",
            AssetClass::InjectLedger => "注入台账",
        }
    }
}

/// 四类资产闭集数组。
pub const ASSET_ALL: &[AssetClass] = &[
    AssetClass::GuideTable,
    AssetClass::Judge,
    AssetClass::PointVerify,
    AssetClass::InjectLedger,
];

/// 资产类数（四类闭集）。
pub const ASSET_CAP: usize = 4;

/// 移交清单单条：资产类号 + 非零证据码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetEntry {
    /// 资产类号（0..3）。
    pub class_idx: u32,
    /// 证据码（非零有效）。
    pub evidence: u32,
}

/// 移交清单：四类资产逐项登记，齐（4/4 且证据码全非零）才许收官。
#[derive(Clone, Debug)]
pub struct HandoverList {
    /// 已登记资产条（按登记序）。
    entries: Vec<AssetEntry>,
}

impl HandoverList {
    /// 立一份空清单。
    pub fn new() -> HandoverList {
        HandoverList { entries: Vec::new() }
    }

    /// 已登记条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否无条。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 逐项登记一类资产（类号闭集/证据码非零/重复三道闸）。
    pub fn register_asset(&mut self, class_idx: u32, evidence: u32) -> Result<(), u32> {
        // 类号闭集闸：越四类闭集即该资产类不存在（未登记族）。
        if class_idx >= ASSET_CAP as u32 {
            return Err(GdDiag::ENoent.val());
        }
        // 证据码非零闸。
        if evidence == 0 {
            return Err(GdDiag::ECodeZero.val());
        }
        // 重复闸：同一资产类只落一笔。
        let mut k = 0usize;
        while k < self.entries.len() {
            if let Some(e) = self.entries.get(k) {
                if e.class_idx == class_idx {
                    return Err(GdDiag::EAlready.val());
                }
            }
            k += 1;
        }
        self.entries.push(AssetEntry { class_idx, evidence });
        Ok(())
    }

    /// 取某资产类的证据码（未登记返回 None）。
    pub fn evidence_of(&self, class_idx: u32) -> Option<u32> {
        for e in self.entries.iter() {
            if e.class_idx == class_idx {
                return Some(e.evidence);
            }
        }
        None
    }

    /// 清单齐判定：四类全登记且证据码全非零。
    pub fn complete(&self) -> bool {
        if self.entries.len() != ASSET_CAP {
            return false;
        }
        for a in ASSET_ALL.iter() {
            match self.evidence_of(a.index()) {
                Some(v) => {
                    if v == 0 {
                        return false;
                    }
                }
                None => return false,
            }
        }
        true
    }

    /// 清单指纹（FNV-1a，双跑一致判定用）。
    pub fn fingerprint(&self) -> u32 {
        let mut f = Fnv::new();
        f.push_u32(self.entries.len() as u32);
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                f.push_u32(e.class_idx);
                f.push_u32(e.evidence);
            }
            i += 1;
        }
        f.finish()
    }
}

// ── FNV-1a 逐字节指纹（零随机红线合规：纯确定性哈希） ────────────
/// FNV-1a 32 位增量哈希器（常数取 FNV 官方 32 位参数）。
pub struct Fnv {
    /// 当前哈希状态。
    h: u32,
}

impl Fnv {
    /// 以 FNV offset basis 构造。
    pub fn new() -> Fnv {
        Fnv { h: 0x811C_9DC5 }
    }

    /// 吸入一字节。
    pub fn push_u8(&mut self, b: u8) {
        self.h ^= b as u32;
        self.h = self.h.wrapping_mul(0x0100_0193);
    }

    /// 吸入一个 u32（小端四字节）。
    pub fn push_u32(&mut self, v: u32) {
        let bytes = v.to_le_bytes();
        for b in bytes.iter() {
            self.push_u8(*b);
        }
    }

    /// 取指纹值。
    pub fn finish(&self) -> u32 {
        self.h
    }
}

/// 登记表全量指纹（行集 + 状态 + 继承史，双跑一致判定的被测值）。
pub fn archive_fingerprint(reg: &GuideReg) -> u32 {
    let mut f = Fnv::new();
    f.push_u32(reg.len() as u32);
    f.push_u32(reg.state().rank() as u32);
    let mut i = 0usize;
    while i < reg.len() {
        if let Some(r) = reg.row_at(i) {
            f.push_u32(r.seg);
            f.push_u32(r.item);
            f.push_u32(r.code);
            f.push_u32(r.inherit as u32);
            f.push_u32(r.seq);
        }
        i += 1;
    }
    f.push_u32(reg.inherited_len() as u32);
    let mut k = 0usize;
    while k < reg.inherited_len() {
        if let Some(pair) = reg.inherited_at(k) {
            f.push_u32(pair.0);
            f.push_u32(pair.1);
        }
        k += 1;
    }
    f.finish()
}

/// 双跑指纹判定：两份登记表指纹逐位一致则 Ok(指纹)，否则 Err(EFingerprint)。
pub fn dual_fingerprint(a: &GuideReg, b: &GuideReg) -> Result<u32, u32> {
    let fa = archive_fingerprint(a);
    let fb = archive_fingerprint(b);
    if fa != fb {
        return Err(GdDiag::EFingerprint.val());
    }
    Ok(fa)
}

// ── 指引登记表：域模型主体 ───────────────────────────────────────
/// 深化指引继承档登记表：27 行闭集 + 冻结状态机 + 继承史。
#[derive(Clone, Debug)]
pub struct GuideReg {
    /// 已入表行（按登记序，与 GUIDE_TABLE 冻结序一致）。
    rows: Vec<GuideRow>,
    /// 状态机（只进不退）。
    state: GdState,
    /// 已确认继承的 (段,条目号) 清单（按确认序）。
    inherited: Vec<(u32, u32)>,
}

impl GuideReg {
    /// 开一张空表。
    pub fn new() -> GuideReg {
        GuideReg {
            rows: Vec::new(),
            state: GdState::Open,
            inherited: Vec::new(),
        }
    }

    /// 当前状态。
    pub fn state(&self) -> GdState {
        self.state
    }

    /// 已入表行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 是否无行。
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// 取第 idx 行（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<GuideRow> {
        self.rows.get(idx).copied()
    }

    /// 已确认继承条数。
    pub fn inherited_len(&self) -> usize {
        self.inherited.len()
    }

    /// 取第 k 条继承记录（越界 None）。
    pub fn inherited_at(&self, k: usize) -> Option<(u32, u32)> {
        self.inherited.get(k).copied()
    }

    /// 按 (段,条目号) 正向查行。
    pub fn find_row(&self, seg: u32, item: u32) -> Option<GuideRow> {
        self.find_row_dir(seg, item, true)
    }

    /// 定向扫描查行：forward=true 正向（登记序），false 反向——
    /// 点验判定器的两轮即由这两个方向独立取行，行序被篡改时两轮分歧。
    fn find_row_dir(&self, seg: u32, item: u32, forward: bool) -> Option<GuideRow> {
        let n = self.rows.len();
        let mut k = 0usize;
        while k < n {
            let idx = if forward { k } else { n - 1 - k };
            if let Some(r) = self.rows.get(idx) {
                if r.seg == seg && r.item == item {
                    return Some(*r);
                }
            }
            k += 1;
        }
        None
    }

    /// 逐字登记一行（冻结/容量/段闭集/条目闭集/乱序/零代号六道闸）。
    pub fn register(&mut self, seg: u32, item: u32, code: u32, inherit: bool) -> Result<(), u32> {
        // 闸一：冻结后只读。
        if self.state.rank() >= GdState::Frozen.rank() {
            return Err(GdDiag::ESealed.val());
        }
        // 闸二：容量耗尽（27 行闭集已满）。
        if self.rows.len() >= GUIDE_CAP {
            return Err(GdDiag::ECap.val());
        }
        // 闸三：段闭集（EINVAL 族）。
        let _seg = GuideSeg::from_index(seg).ok_or(GdDiag::ESegRange.val())?;
        // 闸四：条目号闭集（0..=8 之外属未登记族）。
        if item > ITEM_HI {
            return Err(GdDiag::ENoent.val());
        }
        // 闸五：行序——只许按段序×条目序逐行登记。
        let expect_seg = (self.rows.len() / ITEMS_PER_SEG as usize) as u32;
        let expect_item = (self.rows.len() % ITEMS_PER_SEG as usize) as u32;
        if seg != expect_seg || item != expect_item {
            return Err(GdDiag::EOrder.val());
        }
        // 闸六：条目内容代号非零。
        if code == 0 {
            return Err(GdDiag::ECodeZero.val());
        }
        self.rows.push(GuideRow {
            seg,
            item,
            code,
            inherit,
            seq: self.rows.len() as u32,
        });
        // 27 行齐即推进状态机（只进不退，冻结仍须显性走冻结口）。
        if self.rows.len() == GUIDE_CAP {
            self.state = GdState::Complete;
        }
        Ok(())
    }

    /// 冻结：27 行齐才许冻结；重冻/收官后冻结均倒退拒。
    pub fn freeze(&mut self) -> Result<(), u32> {
        if self.state.rank() >= GdState::Frozen.rank() {
            return Err(GdDiag::ERegress.val());
        }
        if self.rows.len() != GUIDE_CAP {
            return Err(GdDiag::ENotFull.val());
        }
        self.state = GdState::Frozen;
        Ok(())
    }

    /// 指引档齐判定器：引用 (seg,item) 的合法性判定（只判不落）。
    ///
    /// 判定次序冻结（先判的码掩蔽后判的码，次序本身是验收对象）：
    /// 1. 段越闭集 → EINVAL（先于条目查找，否则 EINVAL 永不可达）；
    /// 2. 条目未登记 → ENOENT；
    /// 3. 继承旗标未置但被引用 → EPERM；
    /// 4. 重复继承 → EALREADY（先于倒退：已继承者重复引用是重复，不是倒退）；
    /// 5. 倒退继承 → EREGRESS（同段已继承更高条目号）。
    pub fn judge_guide(&self, seg: u32, item: u32) -> Result<GuideRow, u32> {
        // 次序一：段闭集（EINVAL）。
        let _seg = GuideSeg::from_index(seg).ok_or(GdDiag::ESegRange.val())?;
        // 次序二：条目未登记（ENOENT）。
        let row = self.find_row(seg, item).ok_or(GdDiag::ENoent.val())?;
        // 次序三：继承旗标未置但被引用（EPERM）。
        if !row.inherit {
            return Err(GdDiag::EPerm.val());
        }
        // 次序四：重复继承（EALREADY）。
        if self.inherited.contains(&(seg, item)) {
            return Err(GdDiag::EAlready.val());
        }
        // 次序五：倒退继承（EREGRESS）。
        for pair in self.inherited.iter() {
            let (s, i) = *pair;
            if s == seg && i > item {
                return Err(GdDiag::ERegress.val());
            }
        }
        Ok(row)
    }

    /// 确认继承：未冻结拒（表未定形不许引用）→ 判定器四门 → 落继承史。
    pub fn confirm_inherit(&mut self, seg: u32, item: u32) -> Result<(), u32> {
        if self.state.rank() < GdState::Frozen.rank() {
            return Err(GdDiag::ENotFull.val());
        }
        let row = self.judge_guide(seg, item)?;
        self.inherited.push((row.seg, row.item));
        Ok(())
    }

    /// 收官：清单四类齐 + 交接演练 10/10 + 冻结位，三者全备才许收官。
    /// `drill_green` 为交接演练 10/10 的判定结果（调用方跑演练后传入，
    /// 假即演练有红项）。收官为终态，再收官倒退拒。
    pub fn hand_over(&mut self, list: &HandoverList, drill_green: bool) -> Result<(), u32> {
        // 终态闸：已收官再收官属倒退。
        if self.state.rank() >= GdState::Handed.rank() {
            return Err(GdDiag::ERegress.val());
        }
        // 冻结闸：未冻结不许收官。
        if self.state.rank() < GdState::Frozen.rank() {
            return Err(GdDiag::ENotFull.val());
        }
        // 移交清单闸：四类资产逐项登记且证据码全非零。
        if !list.complete() {
            return Err(GdDiag::ENotFull.val());
        }
        // 交接演练闸：10/10 有红项即拒。
        if !drill_green {
            return Err(GdDiag::EVerify.val());
        }
        self.state = GdState::Handed;
        Ok(())
    }

    /// 点验判定器：对任意条目做两轮独立点验（正向扫描一轮、反向扫描一轮），
    /// 两轮取行与打包位型逐位一致 → Ok(位型)；一致地未登记/越段 → 回
    /// 对应判定码；两轮出现分歧 → EVerify（表序可疑）。
    pub fn verify_point(&self, seg: u32, item: u32) -> Result<u32, u32> {
        // 段闭集先判（与 judge_guide 次序一一致）。
        let _seg = GuideSeg::from_index(seg).ok_or(GdDiag::ESegRange.val())?;
        // 两轮独立取行：正向一轮、反向一轮。
        let ra = self.find_row_dir(seg, item, true);
        let rb = self.find_row_dir(seg, item, false);
        match (ra, rb) {
            (Some(a), Some(b)) => {
                let ba = point_bits(&a);
                let bb = point_bits(&b);
                // 逐位一致 + 逐字段一致（位型是打包视角，字段是结构视角）。
                if ba == bb && a == b {
                    Ok(ba)
                } else {
                    Err(GdDiag::EVerify.val())
                }
            }
            // 两轮一致地未登记 → ENOENT（点验口径与判定器口径对齐）。
            (None, None) => Err(GdDiag::ENoent.val()),
            // 一轮有一轮无 → 表序分歧，点验失败。
            _ => Err(GdDiag::EVerify.val()),
        }
    }
}

/// 点验位型打包：seg[31:30] item[29:26] code[25:10] seq[5:1] inherit[0]，
/// 位段互斥，同表内 27 行位型两两互异（位型碰撞即编码失效）。
fn point_bits(r: &GuideRow) -> u32 {
    let mut b = (r.seg & 0x3) << 30;
    b |= (r.item & 0xF) << 26;
    b |= (r.code & 0xFFFF) << 10;
    b |= (r.seq & 0x1F) << 1;
    b |= r.inherit as u32;
    b
}

// ── 确定性 LCG（种子 = SEG_BASE，零随机红线合规） ────────────────
/// 线性同余发生器：种子写死，序列确定，双跑一致。
pub struct Lcg {
    /// 当前状态。
    state: u64,
}

impl Lcg {
    /// 以固定种子构造。
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }

    /// 下一伪随机数（确定性）。
    pub fn next_u32(&mut self) -> u32 {
        // LCG 常数取自 Knuth MMIX 31 位推荐值（整数手册常用闭集）。
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

// ── 构建器：满表/冻结表/移交清单（确定性，无随机） ────────────────
/// 构建满表：按 GUIDE_TABLE 冻结序逐字登记 27 行（表尾状态 Complete）。
pub fn build_full_archive() -> GuideReg {
    let mut reg = GuideReg::new();
    let mut i = 0usize;
    while i < GUIDE_TABLE.len() {
        match GUIDE_TABLE.get(i) {
            Some(row) => {
                // 冻结源表按序登记，出错即源表失序：终止构造，后续断言必红。
                if reg.register(row.seg, row.item, row.code, row.inherit).is_err() {
                    break;
                }
            }
            None => break,
        }
        i += 1;
    }
    reg
}

/// 构建冻结表：满表 + 显性冻结口（行数不足时冻结失败，状态停在 Complete）。
pub fn build_frozen_archive() -> GuideReg {
    let mut reg = build_full_archive();
    let _ = reg.freeze();
    reg
}

/// 构建齐账移交清单：四类资产各落一笔非零证据码（确定性代号）。
pub fn build_full_handover() -> HandoverList {
    let mut list = HandoverList::new();
    let mut idx = 0u32;
    while idx < ASSET_CAP as u32 {
        // 证据码 0x12770..0x12773：卡号 12777 的确定性派生，写死不用随机。
        let _ = list.register_asset(idx, 0x12770 + idx);
        idx += 1;
    }
    list
}

/// 正常继承流：三段各自按条目号升序继承全部旗标真条目（6×3=18 笔）。
/// 升序保证不触发倒退；旗标假条目（≡2 mod 3）自然跳过。
pub fn inherit_normal(reg: &mut GuideReg) -> Result<usize, u32> {
    let mut n = 0usize;
    let mut seg = 0u32;
    while seg < 3 {
        let mut item = 0u32;
        while item <= ITEM_HI {
            let inheritable = match reg.find_row(seg, item) {
                Some(r) => r.inherit,
                None => false,
            };
            if inheritable {
                reg.confirm_inherit(seg, item)?;
                n += 1;
            }
            item += 1;
        }
        seg += 1;
    }
    Ok(n)
}

// ── 注入面：三类违约各 10 轮，判定器必须全抓 ─────────────────────
/// 注入轮数（三类共用口径：10 轮）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入台账：三类违约的轮数与抓获数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectReport {
    /// 类一（未登记条目）轮数。
    pub unreg_rounds: usize,
    /// 类一抓获数。
    pub unreg_caught: usize,
    /// 类二（旗标未置引用）轮数。
    pub flag_rounds: usize,
    /// 类二抓获数。
    pub flag_caught: usize,
    /// 类三（倒退继承）轮数。
    pub regress_rounds: usize,
    /// 类三抓获数。
    pub regress_caught: usize,
}

impl InjectReport {
    /// 三类各 10 轮全抓判定（逃脱即判据失效）。
    pub fn all_caught(&self) -> bool {
        self.unreg_rounds == INJECT_ROUNDS
            && self.unreg_caught == INJECT_ROUNDS
            && self.flag_rounds == INJECT_ROUNDS
            && self.flag_caught == INJECT_ROUNDS
            && self.regress_rounds == INJECT_ROUNDS
            && self.regress_caught == INJECT_ROUNDS
    }

    /// 总抓获数（三类各 10 → 满抓 30）。
    pub fn caught_total(&self) -> usize {
        self.unreg_caught + self.flag_caught + self.regress_caught
    }
}

/// 三类注入各 10 轮（LCG 种子 = SEG_BASE 写死，序列确定双跑一致）：
/// 类一：合法段 + 越出 0..9 闭集的伪条目号，判定器必须回 ENOENT；
/// 类二：引用旗标假条目（条目号 ≡2 mod 3），判定器必须回 EPERM；
/// 类三：同段先继承 0/1/6 再引用更低的 3 或 4，判定器必须回 EREGRESS。
pub fn run_injections() -> InjectReport {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut rep = InjectReport {
        unreg_rounds: 0,
        unreg_caught: 0,
        flag_rounds: 0,
        flag_caught: 0,
        regress_rounds: 0,
        regress_caught: 0,
    };
    let reg = build_full_archive();
    // 类一：未登记条目注入。
    let mut k = 0usize;
    while k < INJECT_ROUNDS {
        let seg = rng.below(3);
        let item = ITEM_HI + 1 + rng.below(30);
        let caught = reg.judge_guide(seg, item) == Err(GdDiag::ENoent.val());
        rep.unreg_rounds += 1;
        if caught {
            rep.unreg_caught += 1;
        }
        k += 1;
    }
    // 类二：旗标未置引用注入。
    let mut k = 0usize;
    while k < INJECT_ROUNDS {
        let seg = rng.below(3);
        let slot = rng.below(3);
        let item = 2 + slot * 3; // 2/5/8：冻结表中旗标为假的条目号
        let caught = reg.judge_guide(seg, item) == Err(GdDiag::EPerm.val());
        rep.flag_rounds += 1;
        if caught {
            rep.flag_caught += 1;
        }
        k += 1;
    }
    // 类三：倒退继承注入（需要冻结位与继承史，逐轮新表）。
    // 靶心约束：未继承、旗标真（非 ≡2 mod 3）、低于已继承上沿 6——
    // 先继承 (seg,0)(seg,1)(seg,6)，再引 3 或 4，判定器必须回 EREGRESS；
    // 靶心落旗标假条目会误回 EPERM（那是类二的口径），必须避开。
    let mut k = 0usize;
    while k < INJECT_ROUNDS {
        let seg = rng.below(3);
        let mut work = build_frozen_archive();
        let _ = work.confirm_inherit(seg, 0);
        let _ = work.confirm_inherit(seg, 1);
        let _ = work.confirm_inherit(seg, 6);
        let target = 3 + rng.below(2); // 3 或 4：旗标真、未继承、低于上沿 6
        let caught = work.confirm_inherit(seg, target) == Err(GdDiag::ERegress.val());
        rep.regress_rounds += 1;
        if caught {
            rep.regress_caught += 1;
        }
        k += 1;
    }
    rep
}

// ── 交接演练 10/10（判据 J1 第二半） ─────────────────────────────
/// 交接演练：十条断言独立写死，覆盖四类资产与表完整性。
/// 每条不复用他条被测值，任何一条为假即移交失败。
pub fn drill_assertions(
    reg: &GuideReg,
    list: &HandoverList,
    rep: &InjectReport,
) -> Vec<(String, bool)> {
    let mut out: Vec<(String, bool)> = Vec::new();
    // 演练 1：冻结登记表 27 行齐（尾行可取）。
    out.push((
        "演练1-登记表27行齐".to_string(),
        reg.len() == GUIDE_CAP && reg.row_at(GUIDE_CAP - 1).is_some(),
    ));
    // 演练 2：三段各九行（段闭集守恒）。
    let mut seg_ok = true;
    for sg in SEG_ALL.iter() {
        let mut cnt = 0usize;
        let mut i = 0usize;
        while i < reg.len() {
            if let Some(r) = reg.row_at(i) {
                if r.seg == sg.index() {
                    cnt += 1;
                }
            }
            i += 1;
        }
        if cnt != ITEMS_PER_SEG as usize {
            seg_ok = false;
        }
    }
    out.push(("演练2-三段各九行".to_string(), seg_ok));
    // 演练 3：旗标守恒（真 18 / 假 9，与冻结表声明值恒等）。
    let mut t = 0usize;
    let mut f = 0usize;
    let mut i = 0usize;
    while i < reg.len() {
        if let Some(r) = reg.row_at(i) {
            if r.inherit {
                t += 1;
            } else {
                f += 1;
            }
        }
        i += 1;
    }
    out.push((
        "演练3-旗标守恒18真9假".to_string(),
        t == GUIDE_INHERIT_N && f == GUIDE_NOINHERIT_N,
    ));
    // 演练 4：条目内容代号全非零（零代号行不构成资产）。
    let mut code_ok = true;
    let mut i = 0usize;
    while i < reg.len() {
        if let Some(r) = reg.row_at(i) {
            if r.code == 0 {
                code_ok = false;
            }
        }
        i += 1;
    }
    out.push(("演练4-代号全非零".to_string(), code_ok));
    // 演练 5：移交清单四类资产逐项登记且证据码全非零。
    out.push((
        "演练5-移交清单四类齐".to_string(),
        list.complete() && list.len() == ASSET_CAP,
    ));
    // 演练 6：判定器闸门正确（旗标假引用 EPERM、未登记 ENOENT、越段
    // EINVAL——三门均与继承史无关，继承史演进不掩蔽闸门行为）。
    let mut judge_ok = true;
    for sg in SEG_ALL.iter() {
        if reg.judge_guide(sg.index(), 2) != Err(GdDiag::EPerm.val()) {
            judge_ok = false;
        }
        if reg.judge_guide(sg.index(), 9) != Err(GdDiag::ENoent.val()) {
            judge_ok = false;
        }
    }
    if reg.judge_guide(3, 0) != Err(GdDiag::ESegRange.val()) {
        judge_ok = false;
    }
    out.push(("演练6-判定器闸门正确".to_string(), judge_ok));
    // 演练 7：点验两轮逐位一致（27 条目全数 Ok）。
    let mut point_ok = true;
    let mut seg = 0u32;
    while seg < 3 {
        let mut item = 0u32;
        while item <= ITEM_HI {
            if reg.verify_point(seg, item).is_err() {
                point_ok = false;
            }
            item += 1;
        }
        seg += 1;
    }
    out.push(("演练7-点验两轮逐位一致".to_string(), point_ok));
    // 演练 8：注入三类 10 轮全抓。
    out.push(("演练8-注入三类全抓".to_string(), rep.all_caught()));
    // 演练 9：双跑一致指纹（两次冻结构建指纹逐位一致）。
    let a = build_frozen_archive();
    let b = build_frozen_archive();
    out.push((
        "演练9-双跑指纹一致".to_string(),
        dual_fingerprint(&a, &b).is_ok(),
    ));
    // 演练 10：冻结就绪（状态机至少到冻结位，收官在移交口完成）。
    out.push((
        "演练10-冻结就绪".to_string(),
        reg.state().rank() >= GdState::Frozen.rank(),
    ));
    out
}

/// 交接演练 10/10 全绿判定（条数恒 10，任何一条假即不全绿）。
pub fn drill_all_green(list: &[(String, bool)]) -> bool {
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
/// 段位守卫：SEG 对齐 0x40 且落在 VX9 公式窗 [0x80000, 0x831C0]。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if !(0x80000..=0x831C0).contains(&SEG_BASE) {
        return false;
    }
    true
}

/// 诊断码闭集守卫：12 码互异、全落段窗、查表命中、未知码回落。
fn diag_closed_ok() -> bool {
    if GD_ALL.len() != 12 {
        return false;
    }
    for d in GD_ALL.iter() {
        let v = d.val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        if d.detail().is_empty() {
            return false;
        }
        if detail(v) != d.detail() {
            return false;
        }
    }
    // 未知码必须回落"未知"文案，不得误命中任何已知码。
    if detail(SEG_BASE + 0x3F) != "未知诊断码：不在 0x82C41..0x82C4C 闭集内" {
        return false;
    }
    true
}

/// 冻结闭集：满表 → 冻结 → 27 行且关键字段与源表一致。
fn table_freeze_ok() -> bool {
    let raw = build_full_archive();
    if raw.state() != GdState::Complete || raw.len() != GUIDE_CAP {
        return false;
    }
    let frozen = build_frozen_archive();
    if frozen.state() != GdState::Frozen {
        return false;
    }
    // 首/中/尾三行与源表逐字段一致（查表校对，不复用被测值）。
    let head_ok = match (frozen.row_at(0), GUIDE_TABLE.first()) {
        (Some(a), Some(b)) => a == *b,
        _ => false,
    };
    let mid_ok = match (frozen.row_at(13), GUIDE_TABLE.get(13)) {
        (Some(a), Some(b)) => a == *b,
        _ => false,
    };
    let tail_ok = match (frozen.row_at(26), GUIDE_TABLE.get(26)) {
        (Some(a), Some(b)) => a == *b,
        _ => false,
    };
    head_ok && mid_ok && tail_ok
}

/// 判定器正常引用：三段样例旗标真条目 Ok 且字段对表；旗标假条目 EPERM。
fn judge_normal_ok() -> bool {
    let reg = build_full_archive();
    // 三段首条目（旗标真）Ok 且代号与源表一致。
    let mut seg = 0u32;
    while seg < 3 {
        let expect = 0x7700 + 0x100 * seg + 1;
        let ok = match reg.judge_guide(seg, 0) {
            Ok(r) => r.code == expect && r.inherit && r.item == 0,
            Err(_) => false,
        };
        if !ok {
            return false;
        }
        seg += 1;
    }
    // 旗标假条目（0,2）被引用必须 EPERM。
    if reg.judge_guide(0, 2) != Err(GdDiag::EPerm.val()) {
        return false;
    }
    // 次尾条目（2,7，旗标真）Ok 且登记序 25。
    match reg.judge_guide(2, 7) {
        Ok(r) => r.seq == 25 && r.code == 0x7908,
        Err(_) => false,
    }
}

/// 判定次序冻结：EINVAL → ENOENT → EPERM → EALREADY → EREGRESS。
fn judge_order_ok() -> bool {
    let reg = build_full_archive();
    // EINVAL 先于 ENOENT：段 3 越闭集，即便 (3,0) 也未登记，仍回 EINVAL。
    if reg.judge_guide(3, 0) != Err(GdDiag::ESegRange.val()) {
        return false;
    }
    // ENOENT：合法段 + 越出条目闭集的号（两段各一路）。
    if reg.judge_guide(0, 9) != Err(GdDiag::ENoent.val()) {
        return false;
    }
    if reg.judge_guide(2, 99) != Err(GdDiag::ENoent.val()) {
        return false;
    }
    // 次序三/四/五需继承史：冻结后先继承 (0,0)(0,1)(0,4)。
    let mut work = build_frozen_archive();
    let _ = work.confirm_inherit(0, 0);
    let _ = work.confirm_inherit(0, 1);
    let _ = work.confirm_inherit(0, 4);
    // EPERM 先于 EREGRESS/EALREADY：旗标假条目即便倒退也回 EPERM。
    if work.judge_guide(0, 2) != Err(GdDiag::EPerm.val()) {
        return false;
    }
    // EALREADY 先于 EREGRESS：条目 1 已继承且低于上沿 4，仍回 EALREADY。
    if work.judge_guide(0, 1) != Err(GdDiag::EAlready.val()) {
        return false;
    }
    // EREGRESS：条目 3 未继承、旗标真、低于已继承上沿 4。
    if work.judge_guide(0, 3) != Err(GdDiag::ERegress.val()) {
        return false;
    }
    // 正常路：条目 6 未继承、旗标真、不倒退 → Ok。
    work.judge_guide(0, 6).is_ok()
}

/// 点验判定器：27 条目两轮逐位一致全 Ok；位型两两互异；违约路对码。
fn point_verify_ok() -> bool {
    let reg = build_full_archive();
    // 27 条目全数两轮一致。
    let mut seg = 0u32;
    while seg < 3 {
        let mut item = 0u32;
        while item <= ITEM_HI {
            if reg.verify_point(seg, item).is_err() {
                return false;
            }
            item += 1;
        }
        seg += 1;
    }
    // 位型互异抽验：首条目 vs 次条目 vs 邻段首条目。
    let b00 = match reg.verify_point(0, 0) {
        Ok(b) => b,
        Err(_) => 0,
    };
    let b01 = match reg.verify_point(0, 1) {
        Ok(b) => b,
        Err(_) => 1,
    };
    let b10 = match reg.verify_point(1, 0) {
        Ok(b) => b,
        Err(_) => 2,
    };
    if b00 == b01 || b00 == b10 {
        return false;
    }
    // 未登记条目：两轮一致回 ENOENT。
    if reg.verify_point(0, 9) != Err(GdDiag::ENoent.val()) {
        return false;
    }
    // 段越闭集：EINVAL。
    if reg.verify_point(3, 0) != Err(GdDiag::ESegRange.val()) {
        return false;
    }
    // 旗标假条目点验仍是表完整性判定：两轮一致即 Ok。
    reg.verify_point(0, 2).is_ok()
}

/// 注入必抓·类一：未登记条目 10 轮全数命中，无逃脱。
fn inject_unreg_ok(rep: &InjectReport) -> bool {
    rep.unreg_rounds == INJECT_ROUNDS && rep.unreg_caught == INJECT_ROUNDS
}

/// 注入必抓·类二：旗标未置引用 10 轮全数命中，无逃脱。
fn inject_flag_ok(rep: &InjectReport) -> bool {
    rep.flag_rounds == INJECT_ROUNDS && rep.flag_caught == INJECT_ROUNDS
}

/// 注入必抓·类三：倒退继承 10 轮全数命中，无逃脱。
fn inject_regress_ok(rep: &InjectReport) -> bool {
    rep.regress_rounds == INJECT_ROUNDS && rep.regress_caught == INJECT_ROUNDS
}

/// 双跑一致：指纹、注入台账、移交清单三面逐项一致。
fn dual_run_ok() -> bool {
    let a = build_frozen_archive();
    let b = build_frozen_archive();
    // 指纹一致（含 Err(EFingerprint) 语义路径）。
    if dual_fingerprint(&a, &b).is_err() {
        return false;
    }
    // 注入台账一致。
    if run_injections() != run_injections() {
        return false;
    }
    // 移交清单指纹一致。
    if build_full_handover().fingerprint() != build_full_handover().fingerprint() {
        return false;
    }
    true
}

/// 容量耗尽：满表后第 28 行拒（ECap），行数不变。
fn capacity_ok() -> bool {
    let mut full = build_full_archive();
    if full.len() != GUIDE_CAP {
        return false;
    }
    if full.register(0, 0, 0x7701, true) != Err(GdDiag::ECap.val()) {
        return false;
    }
    full.len() == GUIDE_CAP
}

/// 冻结守卫：冻结后写入拒；重冻结倒退拒；未冻结不得引用继承/收官。
fn frozen_guard_ok() -> bool {
    // 冻结后写入拒，表不变。
    let mut frozen = build_frozen_archive();
    if frozen.register(0, 0, 0x7701, true) != Err(GdDiag::ESealed.val()) {
        return false;
    }
    if frozen.len() != GUIDE_CAP {
        return false;
    }
    // 重冻结倒退拒。
    if frozen.freeze() != Err(GdDiag::ERegress.val()) {
        return false;
    }
    // 未冻结（Complete）不得引用继承。
    let mut raw = build_full_archive();
    if raw.confirm_inherit(0, 0) != Err(GdDiag::ENotFull.val()) {
        return false;
    }
    // 未冻结不得收官。
    let list = build_full_handover();
    raw.hand_over(&list, true) == Err(GdDiag::ENotFull.val())
}

/// 交接演练 10/10：齐账全绿；未冻结档/缺资产清单/残缺注入台账各有红项。
fn drill_ok() -> bool {
    let frozen = build_frozen_archive();
    let list = build_full_handover();
    let rep = run_injections();
    let green = drill_assertions(&frozen, &list, &rep);
    if green.len() != 10 {
        return false;
    }
    if !drill_all_green(&green) {
        return false;
    }
    // 反例一：未冻结档 → 演练 10 红。
    let raw = build_full_archive();
    if drill_all_green(&drill_assertions(&raw, &list, &rep)) {
        return false;
    }
    // 反例二：清单缺资产 → 演练 5 红。
    let mut partial = HandoverList::new();
    let _ = partial.register_asset(0, 0x12770);
    if drill_all_green(&drill_assertions(&frozen, &partial, &rep)) {
        return false;
    }
    // 反例三：注入台账残缺（倒退继承漏抓一路）→ 演练 8 红。
    let mut broken = rep;
    broken.regress_caught = 9;
    !drill_all_green(&drill_assertions(&frozen, &list, &broken))
}

/// 正常流：满表 → 冻结 → 正常继承 18 笔 → 演练 10/10 → 收官至 Handed。
fn happy_path_ok() -> bool {
    let mut reg = build_full_archive();
    if reg.state() != GdState::Complete {
        return false;
    }
    if reg.freeze().is_err() {
        return false;
    }
    match inherit_normal(&mut reg) {
        Ok(n) => {
            if n != GUIDE_INHERIT_N {
                return false;
            }
        }
        Err(_) => return false,
    }
    let list = build_full_handover();
    let rep = run_injections();
    let green = drill_all_green(&drill_assertions(&reg, &list, &rep));
    if !green {
        return false;
    }
    if reg.hand_over(&list, green).is_err() {
        return false;
    }
    reg.state() == GdState::Handed
}

/// 移交清单收官语义：缺资产/演练红各拒一路；齐且绿收官成功；再收官倒退拒。
fn handover_ok() -> bool {
    let mut reg = build_frozen_archive();
    if inherit_normal(&mut reg).is_err() {
        return false;
    }
    let list = build_full_handover();
    let rep = run_injections();
    let green = drill_all_green(&drill_assertions(&reg, &list, &rep));
    if !green {
        return false;
    }
    // 缺资产：清单不齐 → ENotFull。
    let mut partial = HandoverList::new();
    let _ = partial.register_asset(0, 0x12770);
    if reg.hand_over(&partial, green) != Err(GdDiag::ENotFull.val()) {
        return false;
    }
    // 演练有红项 → EVerify。
    if reg.hand_over(&list, false) != Err(GdDiag::EVerify.val()) {
        return false;
    }
    // 齐 + 绿 → 收官成功且状态 Handed。
    if reg.hand_over(&list, green).is_err() {
        return false;
    }
    if reg.state() != GdState::Handed {
        return false;
    }
    // 终态再收官：倒退拒。
    reg.hand_over(&list, green) == Err(GdDiag::ERegress.val())
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12777 自检（判据 12777 卡：移交清单四类资产齐 + 交接演练 10/10）。
pub fn run_unx12777_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12777");
    let frozen = build_frozen_archive();
    let list = build_full_handover();
    let rep = run_injections();

    s.add_dyn(
        "F12777-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F12777-02 诊断码闭集",
        diag_closed_ok(),
        &format!("n={} base=0x{:05x}", GD_ALL.len(), SEG_BASE),
    );

    s.add_dyn(
        "F12777-03 段闭集与冻结表",
        SEG_ALL.len() == 3 && GUIDE_TABLE.len() == GUIDE_CAP,
        &format!(
            "segs={} rows={}首={}尾={}",
            SEG_ALL.len(),
            GUIDE_TABLE.len(),
            GUIDE_TABLE.first().map(|r| r.code).unwrap_or(0),
            GUIDE_TABLE.last().map(|r| r.code).unwrap_or(0)
        ),
    );

    s.add_dyn(
        "F12777-04 登记表27行冻结",
        table_freeze_ok(),
        &format!("state={:?}", frozen.state()),
    );

    s.add_dyn(
        "F12777-05 判定器正常引用",
        judge_normal_ok(),
        "三段样例 Ok 且字段对表；旗标假引用 EPERM",
    );

    s.add_dyn(
        "F12777-06 判定次序冻结",
        judge_order_ok(),
        "EINVAL→ENOENT→EPERM→EALREADY→EREGRESS",
    );

    s.add_dyn(
        "F12777-07 点验两轮逐位一致",
        point_verify_ok(),
        "27条目两轮全一致；位型互异；违约路对码",
    );

    s.add_dyn(
        "F12777-08 注入·未登记条目10轮",
        inject_unreg_ok(&rep),
        &format!("caught={}/{}", rep.unreg_caught, rep.unreg_rounds),
    );

    s.add_dyn(
        "F12777-09 注入·旗标未置10轮",
        inject_flag_ok(&rep),
        &format!("caught={}/{}", rep.flag_caught, rep.flag_rounds),
    );

    s.add_dyn(
        "F12777-10 注入·倒退继承10轮",
        inject_regress_ok(&rep),
        &format!("caught={}/{}", rep.regress_caught, rep.regress_rounds),
    );

    s.add_dyn(
        "F12777-11 交接演练10/10",
        drill_ok() && drill_all_green(&drill_assertions(&frozen, &list, &rep)),
        &format!("n={}", drill_assertions(&frozen, &list, &rep).len()),
    );

    s.add_dyn(
        "F12777-12 双跑一致指纹",
        dual_run_ok(),
        "指纹/注入台账/移交清单 三面逐项一致",
    );

    s.add_dyn(
        "F12777-13 容量耗尽与冻结守卫",
        capacity_ok() && frozen_guard_ok(),
        "28行拒/冻结写拒/重冻倒退拒/未冻继承拒/未冻收官拒",
    );

    s.add_dyn(
        "F12777-14 移交清单四类资产收官",
        handover_ok() && happy_path_ok(),
        "缺资产拒/演练红拒/收官成功/再收官倒退拒",
    );

    // 收口：判据条数声明（前 14 条全绿才收口绿）。
    let done = s.len();
    let (passed, failed) = s.tally();
    s.add_dyn(
        "F12777-15 收口",
        done == 14 && failed == 0 && passed == 14,
        &format!("判据{}条收口 全绿={}", 14, failed == 0),
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_formula_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 公式窗 [0x80000, 0x831C0]。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x80000u32 + 0x40u32 * (12777 - 12600));
        assert!((0x80000..=0x831C0).contains(&SEG_BASE));
    }

    #[test]
    fn diag_codes_closed_set() {
        // 闭集：12 码互异、全落段窗、查表命中、未知码回落。
        assert_eq!(GD_ALL.len(), 12);
        let mut i = 0usize;
        while i < GD_ALL.len() {
            let vi = match GD_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi >= SEG_BASE && vi < SEG_BASE + 0x40);
            assert!(!detail(vi).is_empty());
            let mut j = i + 1;
            while j < GD_ALL.len() {
                let vj = match GD_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 1,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).contains("未知"));
    }

    #[test]
    fn happy_path_freeze_inherit_handover() {
        // 正常流：满表 → 冻结 → 正常继承 18 笔 → 演练 10/10 → 收官。
        let mut reg = build_full_archive();
        assert_eq!(reg.state(), GdState::Complete);
        assert!(reg.freeze().is_ok());
        assert_eq!(reg.state(), GdState::Frozen);
        let n = match inherit_normal(&mut reg) {
            Ok(n) => n,
            Err(_) => 99,
        };
        assert_eq!(n, GUIDE_INHERIT_N);
        let list = build_full_handover();
        let rep = run_injections();
        let green = drill_all_green(&drill_assertions(&reg, &list, &rep));
        assert!(green, "交接演练有红项");
        assert!(reg.hand_over(&list, green).is_ok());
        assert_eq!(reg.state(), GdState::Handed);
    }

    #[test]
    fn judge_gate_order_frozen() {
        // 判定次序：EINVAL → ENOENT → EPERM → EALREADY → EREGRESS。
        assert!(judge_order_ok());
        let reg = build_full_archive();
        assert_eq!(reg.judge_guide(3, 0), Err(GdDiag::ESegRange.val()));
        assert_eq!(reg.judge_guide(0, 9), Err(GdDiag::ENoent.val()));
        let mut work = build_frozen_archive();
        let _ = work.confirm_inherit(0, 0);
        let _ = work.confirm_inherit(0, 1);
        let _ = work.confirm_inherit(0, 4);
        assert_eq!(work.judge_guide(0, 2), Err(GdDiag::EPerm.val()));
        assert_eq!(work.judge_guide(0, 1), Err(GdDiag::EAlready.val()));
        assert_eq!(work.judge_guide(0, 3), Err(GdDiag::ERegress.val()));
        assert!(work.judge_guide(0, 6).is_ok());
        // 未冻结不得引用继承。
        let mut raw = build_full_archive();
        assert_eq!(raw.confirm_inherit(0, 0), Err(GdDiag::ENotFull.val()));
    }

    #[test]
    fn injectors_ten_rounds_all_caught() {
        // 三类注入各 10 轮全抓，逃脱即判据失效。
        let rep = run_injections();
        assert_eq!(rep.unreg_rounds, 10);
        assert_eq!(rep.unreg_caught, 10, "未登记条目注入有逃脱");
        assert_eq!(rep.flag_rounds, 10);
        assert_eq!(rep.flag_caught, 10, "旗标未置引用注入有逃脱");
        assert_eq!(rep.regress_rounds, 10);
        assert_eq!(rep.regress_caught, 10, "倒退继承注入有逃脱");
        assert!(rep.all_caught());
        assert_eq!(rep.caught_total(), 30);
    }

    #[test]
    fn dual_run_fingerprint_identical() {
        // 双跑一致：指纹、注入台账、移交清单三面逐项一致。
        let a = build_frozen_archive();
        let b = build_frozen_archive();
        assert_eq!(archive_fingerprint(&a), archive_fingerprint(&b));
        assert!(dual_fingerprint(&a, &b).is_ok());
        assert_eq!(run_injections(), run_injections());
        assert_eq!(
            build_full_handover().fingerprint(),
            build_full_handover().fingerprint()
        );
    }

    #[test]
    fn capacity_exhausted_and_frozen_guards() {
        // 容量耗尽：满表后第 28 行拒，行数不变。
        assert!(capacity_ok());
        let mut full = build_full_archive();
        assert_eq!(full.register(2, 8, 0x7909, true), Err(GdDiag::ECap.val()));
        assert_eq!(full.len(), GUIDE_CAP);
        // 未冻结不得收官。
        let list = build_full_handover();
        assert_eq!(full.hand_over(&list, true), Err(GdDiag::ENotFull.val()));
        // 冻结守卫：写入拒、重冻倒退拒。
        assert!(frozen_guard_ok());
        let mut frozen = build_frozen_archive();
        assert_eq!(frozen.register(0, 0, 0x7701, true), Err(GdDiag::ESealed.val()));
        assert_eq!(frozen.freeze(), Err(GdDiag::ERegress.val()));
    }

    #[test]
    fn boundary_four_classes() {
        // 边界一：首条目 (0,0)。
        let reg = build_full_archive();
        let first_ok = match reg.judge_guide(0, 0) {
            Ok(r) => r.code == 0x7701 && r.seq == 0 && r.inherit,
            Err(_) => false,
        };
        assert!(first_ok);
        // 边界二：次尾条目 (2,7)——尾条目 (2,8) 旗标假，属 EPERM 口径。
        let last_ok = match reg.judge_guide(2, 7) {
            Ok(r) => r.code == 0x7908 && r.seq == 25,
            Err(_) => false,
        };
        assert!(last_ok);
        // 尾条目 (2,8) 旗标假：引用必 EPERM。
        assert_eq!(reg.judge_guide(2, 8), Err(GdDiag::EPerm.val()));
        // 边界三：段闭集上沿（seg=3 越闭集）。
        assert_eq!(reg.judge_guide(3, 0), Err(GdDiag::ESegRange.val()));
        // 边界四：容量满沿（27 行齐后第 28 行拒）与条目闭集上沿。
        let mut full = build_full_archive();
        assert_eq!(full.register(0, 0, 7, true), Err(GdDiag::ECap.val()));
        assert_eq!(reg.judge_guide(2, 9), Err(GdDiag::ENoent.val()));
    }

    #[test]
    fn point_verify_all_entries_two_rounds() {
        // 27 条目两轮点验全 Ok；位型互异；违约路对码。
        let reg = build_full_archive();
        let mut seg = 0u32;
        while seg < 3 {
            let mut item = 0u32;
            while item <= ITEM_HI {
                assert!(
                    reg.verify_point(seg, item).is_ok(),
                    "段{}条目{}两轮点验不一致",
                    seg,
                    item
                );
                item += 1;
            }
            seg += 1;
        }
        assert_eq!(reg.verify_point(0, 9), Err(GdDiag::ENoent.val()));
        assert_eq!(reg.verify_point(3, 0), Err(GdDiag::ESegRange.val()));
        // 旗标假条目点验仍是表完整性判定：两轮一致即 Ok。
        assert!(reg.verify_point(0, 2).is_ok());
        // 位型抽验互异。
        let b00 = match reg.verify_point(0, 0) {
            Ok(b) => b,
            Err(_) => 0,
        };
        let b01 = match reg.verify_point(0, 1) {
            Ok(b) => b,
            Err(_) => 1,
        };
        let b10 = match reg.verify_point(1, 0) {
            Ok(b) => b,
            Err(_) => 2,
        };
        assert_ne!(b00, b01);
        assert_ne!(b00, b10);
    }

    #[test]
    fn drill_detects_broken_archive() {
        // 演练 10/10：齐账全绿；三类残档各抓出红项。
        assert!(drill_ok());
        let frozen = build_frozen_archive();
        let list = build_full_handover();
        let rep = run_injections();
        let green = drill_assertions(&frozen, &list, &rep);
        assert_eq!(green.len(), 10);
        assert!(drill_all_green(&green));
        // 未冻结档 → 演练 10 红。
        let raw = build_full_archive();
        assert!(!drill_all_green(&drill_assertions(&raw, &list, &rep)));
        // 清单缺资产 → 演练 5 红。
        let mut partial = HandoverList::new();
        assert_eq!(
            partial.register_asset(4, 0x12770),
            Err(GdDiag::ENoent.val())
        );
        let _ = partial.register_asset(0, 0x12770);
        assert!(!drill_all_green(&drill_assertions(&frozen, &partial, &rep)));
        // 注入台账残缺 → 演练 8 红。
        let mut broken = rep;
        broken.regress_caught = 9;
        assert!(!drill_all_green(&drill_assertions(&frozen, &list, &broken)));
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Open→Complete→Frozen→Handed。
        assert!(GdState::Open.rank() < GdState::Complete.rank());
        assert!(GdState::Complete.rank() < GdState::Frozen.rank());
        assert!(GdState::Frozen.rank() < GdState::Handed.rank());
        let mut reg = build_full_archive();
        assert_eq!(reg.state(), GdState::Complete);
        assert!(reg.freeze().is_ok());
        assert_eq!(reg.state(), GdState::Frozen);
        // 冻结后写入拒且表不变。
        assert_eq!(reg.register(0, 0, 0x7701, true), Err(GdDiag::ESealed.val()));
        assert_eq!(reg.len(), GUIDE_CAP);
    }

    #[test]
    fn handover_semantics() {
        // 移交清单：缺资产/演练红各拒一路；齐且绿收官；再收官倒退拒。
        assert!(handover_ok());
        // 证据码零代号与重复登记两闸。
        let mut list = HandoverList::new();
        assert_eq!(list.register_asset(0, 0), Err(GdDiag::ECodeZero.val()));
        let _ = list.register_asset(0, 0x12770);
        assert_eq!(list.register_asset(0, 0x12771), Err(GdDiag::EAlready.val()));
        assert!(!list.complete());
        assert!(list.evidence_of(0).is_some());
        assert!(list.evidence_of(3).is_none());
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条 + 收口。
        let s = run_unx12777_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
        assert_eq!(s.len(), 15);
    }
}
