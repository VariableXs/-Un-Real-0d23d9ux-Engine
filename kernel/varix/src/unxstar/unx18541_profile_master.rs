//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18541 · 伪装档案总纲（bottle 级结构冻结）
//! · 0x80300 段 · 批 08 · 配额 300
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18541-J1 档案结构（bottle 默认层+per-app 层）冻结 v1，
//!       与 E1 接口签名一致。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 结构冻结 v1：档案字段表六字段（版本占 3 槽 + SP/PT/Suite/DPI/
//!     提权各占 1 槽）→ 槽宽表 [3,1,1,1,1,1]，总槽 8；偏移表由槽宽
//!     累加推导（0/3/4/5/6/7），冻结后任一字段可按 (id→offset,width)
//!     寻址，构成 E1 接口签名的数据面；
//!   - 双层结构：layer 0 = bottle 默认层（全应用兜底），layer 1 =
//!     per-app 层（覆盖优先）；解析序 per-app 先于 bottle 默认——
//!     同字段双层皆有值时 layer 1 胜出，layer 1 空则回退 layer 0；
//!   - 冻结状态机三态单向：Draft → Frozen → Sealed；封层序闸：
//!     必须先封 layer 0 再封 layer 1（跨层乱序拒），重复封层拒，
//!     相位倒退拒；
//!   - 注入四类 × 10 轮：结构截断（字段数 <6）/ 槽位重叠（offset
//!     落入前一字段宽内）/ 层越界（layer ≥2）/ 字段 id 重复登记
//!     ——逐类逐轮必抓，漏检即红；
//!   - 边界：空档案（valid=false，不参与解析）、槽全满、越界 id、
//!     重复 id、乱序封层逐项显性拒绝；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机（指纹折叠为确定性纯函数）。
//! 本模块结构清单：
//!   [错误路径] PmCode 闭集（12 码）+ ALL + detail() + detail(u32)
//!   [裁决表]   FIELD_WIDTHS / field_offset / field_width / field_name
//!   [数据结构] ProfileRaw（8 槽定容）+ 双层 ProfilePair
//!   [状态机]   FreezePhase 三态单向 + MasterFreezer（跨层序闸）
//!   [账本]     MasterLedger（冻结账 + 解析账守恒）
//!   [注入核]   四类变异 × 10 轮（截断/重叠/层越界/id 重复）
//!   [收口]     golden_run + run_unx18541_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80300，AI-VX11-1105 G1 批08 独占区）。
pub const SEG_BASE: u32 = 0x80300;

// ── 域冻结常量（结构冻结 v1） ────────────────────────────────────
/// 档案字段数（冻结：版本/SP/PT/Suite/DPI/提权 = 6）。
pub const FIELD_COUNT: usize = 6;
/// 槽宽表（冻结：版本占 3 槽 major/minor/build，其余各 1 槽）。
/// 推导：六字段中仅版本是三元组，故 [3,1,1,1,1,1]。
pub const FIELD_WIDTHS: [u16; FIELD_COUNT] = [3, 1, 1, 1, 1, 1];
/// 总槽数（推导：3+1+1+1+1+1 = 8）。
pub const PROFILE_UNITS: usize = 8;
/// 层数（冻结：0=bottle 默认层，1=per-app 层）。
pub const LAYER_COUNT: usize = 2;
/// 判据注入轮数（10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 版本主版本下界（冻结：≥3，覆盖 XP=5/Vista=6/7=6/10=10 一族）。
pub const VER_MAJOR_MIN: u16 = 3;

/// 字段偏移表（推导：槽宽前缀和）。
pub fn field_offset(id: usize) -> Option<u16> {
    match id {
        0 => Some(0),
        1 => Some(3),
        2 => Some(4),
        3 => Some(5),
        4 => Some(6),
        5 => Some(7),
        _ => None,
    }
}

/// 字段槽宽（id 越界 None）。
pub fn field_width(id: usize) -> Option<u16> {
    if id >= FIELD_COUNT {
        return None;
    }
    Some(FIELD_WIDTHS[id])
}

/// 字段名（闭集，E1 接口签名的文本面）。
pub fn field_name(id: usize) -> Option<&'static str> {
    match id {
        0 => Some("version"),
        1 => Some("sp"),
        2 => Some("product_type"),
        3 => Some("suite_mask"),
        4 => Some("dpi"),
        5 => Some("elevate"),
        _ => None,
    }
}

/// E1 接口签名核对：任一 id 的 (名, 偏移, 宽) 三元组与冻结表一致。
pub fn e1_signature_ok() -> bool {
    let mut id = 0;
    let mut off = 0u16;
    while id < FIELD_COUNT {
        // 名在闭集。
        match field_name(id) {
            Some(_) => {}
            None => return false,
        }
        // 偏移 = 前缀和；宽 = 槽宽表。
        match (field_offset(id), field_width(id)) {
            (Some(o), Some(w)) => {
                if o != off {
                    return false;
                }
                off = off.saturating_add(w);
            }
            _ => return false,
        }
        id += 1;
    }
    // 末字段收尾：前缀和终值 = 总槽。
    off == PROFILE_UNITS as u16
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PmCode {
    /// 字段 id 越界（≥6）。
    FieldIdOutOfRange,
    /// 结构截断（字段数 <6）。
    LayoutTruncated,
    /// 槽位重叠（offset 落入前字段宽内）。
    LayoutOverlap,
    /// 槽位越顶（offset+width > 总槽）。
    LayoutOverrun,
    /// 层越界（layer ≥2）。
    LayerOutOfRange,
    /// 重复封层（同层二次冻结）。
    LayerDuplicate,
    /// 跨层乱序（layer 1 先于 layer 0）。
    LayerOrderBroken,
    /// 相位倒退（冻结机非法回退）。
    PhaseRegression,
    /// 档案值域越界（版本主段为 0 等）。
    ProfileInvalid,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 注入漏检。
    InjectionMissed,
    /// 双跑不一致。
    NotReproducible,
}

impl PmCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PmCode::FieldIdOutOfRange => "字段 id 越界：闭集仅 0..=5",
            PmCode::LayoutTruncated => "结构截断：字段数不足 6",
            PmCode::LayoutOverlap => "槽位重叠：偏移落入前字段宽内",
            PmCode::LayoutOverrun => "槽位越顶：offset+width 超总槽",
            PmCode::LayerOutOfRange => "层越界：仅 layer 0 与 layer 1",
            PmCode::LayerDuplicate => "重复封层：同层只许冻结一次",
            PmCode::LayerOrderBroken => "跨层乱序：须先封 bottle 默认层",
            PmCode::PhaseRegression => "相位倒退：冻结机单向禁回退",
            PmCode::ProfileInvalid => "档案值域越界：版本主段不得为 0",
            PmCode::LedgerBroken => "账本守恒破坏：尝试≠过+拒",
            PmCode::InjectionMissed => "注入漏检：变异必被抓",
            PmCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [PmCode; 12] = [
        PmCode::FieldIdOutOfRange,
        PmCode::LayoutTruncated,
        PmCode::LayoutOverlap,
        PmCode::LayoutOverrun,
        PmCode::LayerOutOfRange,
        PmCode::LayerDuplicate,
        PmCode::LayerOrderBroken,
        PmCode::PhaseRegression,
        PmCode::ProfileInvalid,
        PmCode::LedgerBroken,
        PmCode::InjectionMissed,
        PmCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PmCode::ALL.len() {
        if PmCode::ALL[i].code() == code {
            return PmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80300 段闭集内"
}

// ── 档案原始结构（8 槽定容） ─────────────────────────────────────
/// 单层档案原始槽像：槽 0..3 = 版本 major/minor/build，
/// 槽 4 = SP，槽 5 = PT，槽 6 = Suite，槽 7 = DPI/提权共用待定槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileRaw {
    pub units: [u16; PROFILE_UNITS],
    /// 空档案标记（false = 未绑定，解析时回退下一层）。
    pub valid: bool,
}

impl ProfileRaw {
    pub const fn empty() -> ProfileRaw {
        ProfileRaw { units: [0; PROFILE_UNITS], valid: false }
    }
    /// 槽读取（越界 None，禁裸下标）。
    pub fn unit(&self, idx: usize) -> Option<u16> {
        if idx >= PROFILE_UNITS {
            return None;
        }
        Some(self.units[idx])
    }
    /// 值域守卫：生效档案版本主段不得为 0（伪装到不存在的系统无意义）。
    pub fn value_ok(&self) -> bool {
        if !self.valid {
            return false;
        }
        match self.unit(0) {
            Some(m) => m >= VER_MAJOR_MIN,
            None => false,
        }
    }
    /// 结构指纹：31 折叠对槽值敏感（空档案也有指纹，双跑对拍用）。
    pub fn fingerprint(&self) -> u32 {
        let mut acc: u32 = 0x18541;
        acc = acc.wrapping_mul(31).wrapping_add(if self.valid { 7 } else { 3 });
        let mut i = 0;
        while i < PROFILE_UNITS {
            acc = acc.wrapping_mul(31).wrapping_add(self.units[i] as u32);
            i += 1;
        }
        acc
    }
}

/// 双层档案对（结构冻结的语义载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfilePair {
    /// layer 0：bottle 默认层。
    pub bottle: ProfileRaw,
    /// layer 1：per-app 层。
    pub app: ProfileRaw,
}

impl ProfilePair {
    /// 双层解析：per-app 生效则取 per-app，否则回退 bottle；
    /// 双层皆空 → 空档案（调用方落系统默认，属 F18542 域）。
    pub fn resolve(&self) -> (ProfileRaw, u8) {
        if self.app.valid {
            return (self.app, 1);
        }
        (self.bottle, 0)
    }
}

// ── 结构校验器（冻结闸） ────────────────────────────────────────
/// 结构候造体：注入核用它伪造违约布局。
#[derive(Clone, Copy, Debug)]
pub struct LayoutCand {
    pub widths: [u16; FIELD_COUNT],
    pub offsets: [u16; FIELD_COUNT],
    /// 登记字段数（正常 = 6；注入类 1 伪造 <6）。
    pub count: usize,
}

impl LayoutCand {
    /// 冻结 v1 正宗布局（widths/offsets 皆从冻结表抄）。
    pub fn frozen() -> LayoutCand {
        let mut widths = [0u16; FIELD_COUNT];
        let mut offsets = [0u16; FIELD_COUNT];
        let mut i = 0;
        while i < FIELD_COUNT {
            widths[i] = FIELD_WIDTHS[i];
            offsets[i] = match field_offset(i) {
                Some(o) => o,
                None => 0,
            };
            i += 1;
        }
        LayoutCand { widths, offsets, count: FIELD_COUNT }
    }
}

/// 结构校验：字段数足额 + 偏移配宽无重叠无越顶 + 前缀和闭合。
/// 判定次序：截断 → 越顶 → 重叠（先量级后位置，与总纲语义一致）。
pub fn validate_layout(c: &LayoutCand) -> Result<(), u32> {
    if c.count < FIELD_COUNT {
        return Err(PmCode::LayoutTruncated.code());
    }
    let mut i = 0;
    while i < FIELD_COUNT {
        let off = c.offsets[i] as usize;
        let w = c.widths[i] as usize;
        // 越顶：offset+width 超总槽。
        if off.saturating_add(w) > PROFILE_UNITS {
            return Err(PmCode::LayoutOverrun.code());
        }
        // 重叠：与前面每个字段比较区间交叠。
        let mut j = 0;
        while j < i {
            let pj = c.offsets[j] as usize;
            let wj = c.widths[j] as usize;
            let j_end = pj.saturating_add(wj);
            if off < j_end && pj < off.saturating_add(w) {
                return Err(PmCode::LayoutOverlap.code());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ── 冻结状态机（三态单向 + 跨层序闸） ────────────────────────────
/// 冻结相位（单向：Draft→Frozen→Sealed）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FreezePhase {
    /// 起草中。
    Draft,
    /// 双层齐冻结。
    Frozen,
    /// 已封印（结构 v1 落死）。
    Sealed,
}

/// 封层事件（序闸审计用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreezeRec {
    pub layer: u8,
    pub fingerprint: u32,
}

/// 冻结事件账（定容 4：双层各一 + 2 余量）。
pub struct FreezeJournal {
    pub recs: [Option<FreezeRec>; 4],
    pub len: usize,
    pub full_rejects: u32,
}

impl FreezeJournal {
    pub fn new() -> FreezeJournal {
        FreezeJournal { recs: [None; 4], len: 0, full_rejects: 0 }
    }
    /// 入账（满显性拒，旧账不丢）。
    pub fn push(&mut self, r: FreezeRec) -> Result<(), u32> {
        if self.len >= 4 {
            self.full_rejects = self.full_rejects.saturating_add(1);
            return Err(PmCode::LayerDuplicate.code());
        }
        self.recs[self.len] = Some(r);
        self.len += 1;
        Ok(())
    }
    /// 按下标取事件。
    pub fn rec_at(&self, idx: usize) -> Option<FreezeRec> {
        if idx >= self.len {
            return None;
        }
        self.recs[idx]
    }
}

/// 冻结账（守恒：封层尝试 = 过 + 拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MasterLedger {
    pub tries: u32,
    pub passed: u32,
    pub rejected: u32,
}

impl MasterLedger {
    pub const fn new() -> MasterLedger {
        MasterLedger { tries: 0, passed: 0, rejected: 0 }
    }
    pub fn conserve(&self) -> bool {
        self.tries == self.passed + self.rejected
    }
}

/// 总纲冻结机：跨层序闸 + 相位单向 + 事件账。
pub struct MasterFreezer {
    pub phase: FreezePhase,
    /// 各层冻结位（bitplane：bit0=layer0，bit1=layer1）。
    pub frozen_mask: u8,
    pub journal: FreezeJournal,
    pub ledger: MasterLedger,
}

impl MasterFreezer {
    pub fn new() -> MasterFreezer {
        MasterFreezer {
            phase: FreezePhase::Draft,
            frozen_mask: 0,
            journal: FreezeJournal::new(),
            ledger: MasterLedger::new(),
        }
    }

    /// 封层：序闸（先 0 后 1）+ 重复拒 + 相位推进。
    pub fn freeze_layer(&mut self, layer: usize, fp: u32) -> Result<(), u32> {
        self.ledger.tries = self.ledger.tries.saturating_add(1);
        if layer >= LAYER_COUNT {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(PmCode::LayerOutOfRange.code());
        }
        let bit = 1u8 << layer;
        if self.frozen_mask & bit != 0 {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(PmCode::LayerDuplicate.code());
        }
        // 跨层序闸：layer 1 必须等 layer 0 冻结。
        if layer == 1 && self.frozen_mask & 1 == 0 {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(PmCode::LayerOrderBroken.code());
        }
        // Sealed 相位禁再封（单向）。
        if self.phase == FreezePhase::Sealed {
            self.ledger.rejected = self.ledger.rejected.saturating_add(1);
            return Err(PmCode::PhaseRegression.code());
        }
        self.frozen_mask |= bit;
        self.journal.push(FreezeRec { layer: layer as u8, fingerprint: fp })?;
        self.ledger.passed = self.ledger.passed.saturating_add(1);
        if self.frozen_mask == 0b11 {
            self.phase = FreezePhase::Frozen;
        }
        Ok(())
    }

    /// 封印：仅 Frozen 相位可 seal；封后结构 v1 落死。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase == FreezePhase::Sealed {
            return Err(PmCode::PhaseRegression.code());
        }
        if self.phase != FreezePhase::Frozen {
            return Err(PmCode::LayerOrderBroken.code());
        }
        if !self.ledger.conserve() {
            return Err(PmCode::LedgerBroken.code());
        }
        self.phase = FreezePhase::Sealed;
        Ok(())
    }

    /// 事件账序核：封层顺序必须 0→1。
    pub fn journal_order_ok(&self) -> bool {
        if self.journal.len < 2 {
            return true;
        }
        let a = match self.journal.rec_at(0) {
            Some(r) => r,
            None => return false,
        };
        let b = match self.journal.rec_at(1) {
            Some(r) => r,
            None => return false,
        };
        a.layer == 0 && b.layer == 1
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金链输出（事件数 / 冻结账 / 双层指纹）。
pub type GoldenOut = (usize, MasterLedger, u32, u32);

/// 黄金全链：E1 签名核对 → 冻结布局校验 → 双层按序冻结 → 封印 →
/// 双层解析（per-app 胜出/回退各验一例）。
pub fn golden_run() -> Result<GoldenOut, u32> {
    if !e1_signature_ok() {
        return Err(PmCode::NotReproducible.code());
    }
    let frozen = LayoutCand::frozen();
    validate_layout(&frozen)?;
    let mut fz = MasterFreezer::new();
    fz.freeze_layer(0, frozen.fingerprint_of(0))?;
    fz.freeze_layer(1, frozen.fingerprint_of(1))?;
    fz.seal()?;
    if !fz.journal_order_ok() {
        return Err(PmCode::LayerOrderBroken.code());
    }
    if !fz.ledger.conserve() {
        return Err(PmCode::LedgerBroken.code());
    }
    // 解析语义两例：per-app 生效胜出；per-app 空回退 bottle。
    let mut bottle = ProfileRaw::empty();
    bottle.units[0] = 6;
    bottle.units[1] = 1;
    bottle.units[2] = 7601;
    bottle.valid = true;
    let mut app = ProfileRaw::empty();
    app.units[0] = 10;
    app.units[1] = 0;
    app.units[2] = 19045;
    app.valid = true;
    let pair = ProfilePair { bottle, app };
    let (got1, src1) = pair.resolve();
    if src1 != 1 || !got1.value_ok() {
        return Err(PmCode::ProfileInvalid.code());
    }
    let empty_app = ProfilePair { bottle, app: ProfileRaw::empty() };
    let (got2, src2) = empty_app.resolve();
    if src2 != 0 || got2 != bottle {
        return Err(PmCode::ProfileInvalid.code());
    }
    Ok((fz.journal.len, fz.ledger, pair.fingerprint_pair(), frozen.fingerprint_of(0)))
}

impl LayoutCand {
    /// 层指纹：折叠宽度+偏移表与层号（双跑对拍锚）。
    pub fn fingerprint_of(&self, layer: usize) -> u32 {
        let mut acc: u32 = 0x18540 + layer as u32;
        let mut i = 0;
        while i < FIELD_COUNT {
            acc = acc.wrapping_mul(31).wrapping_add(self.widths[i] as u32);
            acc = acc.wrapping_mul(31).wrapping_add(self.offsets[i] as u32);
            i += 1;
        }
        acc
    }
}

impl ProfilePair {
    /// 双层联合指纹（逐槽折叠 + 层有效性）。
    pub fn fingerprint_pair(&self) -> u32 {
        self.bottle.fingerprint().wrapping_mul(31).wrapping_add(self.app.fingerprint() % 31)
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PmCode::NotReproducible.code());
    }
    Ok(())
}

// ── 注入核（四类变异 × 10 轮） ──────────────────────────────────
/// 注入账（逐类捕获计数）。
#[derive(Clone, Copy, Debug, Default)]
pub struct InjectLedger {
    /// 类1 结构截断。
    pub truncate_caught: u32,
    /// 类2 槽位重叠。
    pub overlap_caught: u32,
    /// 类3 层越界。
    pub layer_oob_caught: u32,
    /// 类4 字段 id 重复（经冻结机重复封层路径）。
    pub dup_caught: u32,
}

/// 各类面额（实现推导，判据魔数）：
/// 类1：count=5 的截断布局必被 LayoutTruncated 抓 → 每轮 1；
/// 类2：offsets=[0,2,..] 与 widths[0]=3 相交必被抓 → 每轮 1；
/// 类3：layer=2 越界必被抓 → 每轮 1；
/// 类4：layer 0 二次封层必被抓 → 每轮 1。
pub const FACE_TRUNCATE: u32 = 1;
pub const FACE_OVERLAP: u32 = 1;
pub const FACE_LAYER_OOB: u32 = 1;
pub const FACE_DUP: u32 = 1;

/// 四类注入 rounds 轮；每类每轮必抓，漏检即 Err。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::default();
    let mut r = 0;
    while r < rounds {
        // 类1 结构截断：字段数 5。
        let mut cand = LayoutCand::frozen();
        cand.count = 5;
        match validate_layout(&cand) {
            Err(c) if c == PmCode::LayoutTruncated.code() => {
                led.truncate_caught = led.truncate_caught.saturating_add(FACE_TRUNCATE);
            }
            _ => return Err(PmCode::InjectionMissed.code()),
        }
        // 类2 槽位重叠：字段 1 偏移 2（落入字段 0 宽 3 内）。
        let mut cand2 = LayoutCand::frozen();
        cand2.offsets[1] = 2;
        match validate_layout(&cand2) {
            Err(c) if c == PmCode::LayoutOverlap.code() => {
                led.overlap_caught = led.overlap_caught.saturating_add(FACE_OVERLAP);
            }
            _ => return Err(PmCode::InjectionMissed.code()),
        }
        // 类3 层越界：layer 2。
        let mut fz = MasterFreezer::new();
        match fz.freeze_layer(2, 0) {
            Err(c) if c == PmCode::LayerOutOfRange.code() => {
                led.layer_oob_caught = led.layer_oob_caught.saturating_add(FACE_LAYER_OOB);
            }
            _ => return Err(PmCode::InjectionMissed.code()),
        }
        // 类4 重复封层：layer 0 连封两次。
        let mut fz2 = MasterFreezer::new();
        if fz2.freeze_layer(0, 1).is_err() {
            return Err(PmCode::InjectionMissed.code());
        }
        match fz2.freeze_layer(0, 1) {
            Err(c) if c == PmCode::LayerDuplicate.code() => {
                led.dup_caught = led.dup_caught.saturating_add(FACE_DUP);
            }
            _ => return Err(PmCode::InjectionMissed.code()),
        }
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：黄金链全过。
pub fn probe_seq_a() -> Result<(), u32> {
    golden_run().map(|_| ())
}

/// 探针 B：跨层乱序（layer 1 先封）→ 拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut fz = MasterFreezer::new();
    fz.freeze_layer(1, 0)
}

/// 探针 C：相位倒退（Draft 未齐即 seal）→ 拒。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut fz = MasterFreezer::new();
    fz.freeze_layer(0, 0)?;
    fz.seal()
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < PmCode::ALL.len() {
        let v = PmCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（8–20 条；动态明细 ≤8） ────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. E1 接口签名：名/偏移/宽三元组闭集一致（J1 主体）。
    v.push(("J1-E1签名一致", e1_signature_ok(), String::from("6字段 前缀和闭合=8")));

    // 2. 黄金链：按序冻结+封印+双层解析全过。
    let g = golden_run();
    let g_ok = match &g {
        Ok((n, led, _, _)) => *n == 2 && led.conserve(),
        Err(_) => false,
    };
    v.push(("J1-黄金全链", g_ok, format!("{:?}", g)));

    // 3. 槽宽/偏移冻结表逐格核对。
    let tbl_ok = FIELD_WIDTHS == [3, 1, 1, 1, 1, 1]
        && field_offset(0) == Some(0)
        && field_offset(1) == Some(3)
        && field_offset(5) == Some(7)
        && field_offset(6).is_none();
    v.push(("J1-槽宽偏移表冻结", tbl_ok, String::from("[3,1,1,1,1,1] @0/3/4/5/6/7")));

    // 4. 双层优先：per-app 胜出 + 空回退 bottle。
    let mut bottle = ProfileRaw::empty();
    bottle.units[0] = 6;
    bottle.valid = true;
    let mut app = ProfileRaw::empty();
    app.units[0] = 10;
    app.valid = true;
    let r1 = ProfilePair { bottle, app }.resolve();
    let r2 = ProfilePair { bottle, app: ProfileRaw::empty() }.resolve();
    let prec_ok = r1.1 == 1 && r1.0.units[0] == 10 && r2.1 == 0 && r2.0.units[0] == 6;
    v.push(("J1-双层优先序", prec_ok, format!("l1胜={:?} 回退={:?}", r1.1, r2.1)));

    // 5. 空档案显性：valid=false 拒绝值域断言。
    let empty_ok = !ProfileRaw::empty().value_ok() && ProfileRaw::empty().unit(0) == Some(0);
    v.push(("J1-空档案显性", empty_ok, String::from("valid=false 不生效")));

    // 6. 类1 截断注入（1 × 10 轮）。
    let led = inject_rounds(JUDGE_ROUNDS);
    let t1 = led.as_ref().map(|l| l.truncate_caught).unwrap_or(0);
    v.push(("J1-截断注入全抓", t1 == FACE_TRUNCATE * JUDGE_ROUNDS, format!("{}/{}", t1, FACE_TRUNCATE * JUDGE_ROUNDS)));

    // 7. 类2 重叠注入。
    let t2 = led.as_ref().map(|l| l.overlap_caught).unwrap_or(0);
    v.push(("J1-重叠注入全抓", t2 == FACE_OVERLAP * JUDGE_ROUNDS, format!("{}/{}", t2, FACE_OVERLAP * JUDGE_ROUNDS)));

    // 8. 类3 层越界注入。
    let t3 = led.as_ref().map(|l| l.layer_oob_caught).unwrap_or(0);
    v.push(("J1-层越界注入全抓", t3 == FACE_LAYER_OOB * JUDGE_ROUNDS, format!("{}/{}", t3, FACE_LAYER_OOB * JUDGE_ROUNDS)));

    // 9. 类4 重复封层注入。
    let t4 = led.as_ref().map(|l| l.dup_caught).unwrap_or(0);
    v.push(("J1-重复封层全抓", t4 == FACE_DUP * JUDGE_ROUNDS, format!("{}/{}", t4, FACE_DUP * JUDGE_ROUNDS)));

    // 10. 注入账守恒。
    let cons = match led.as_ref() {
        Ok(l) => {
            let sum = l.truncate_caught
                .saturating_add(l.overlap_caught)
                .saturating_add(l.layer_oob_caught)
                .saturating_add(l.dup_caught);
            sum == (FACE_TRUNCATE + FACE_OVERLAP + FACE_LAYER_OOB + FACE_DUP) * JUDGE_ROUNDS
        }
        Err(_) => false,
    };
    v.push(("J1-注入账守恒", cons, String::from("4类面额×10轮")));

    // 11. 跨层乱序拒（探针 B）。
    v.push(("J1-跨层乱序拒", probe_seq_b().is_err(), String::from("layer1 先封必拒")));

    // 12. 相位倒退拒（探针 C）。
    v.push(("J1-相位倒退拒", probe_seq_c().is_err(), String::from("Draft 即 seal 必拒")));

    // 13. 封层事件账序核 + 容量。
    let jok = {
        let mut fz = MasterFreezer::new();
        fz.freeze_layer(0, 11).is_ok() && fz.freeze_layer(1, 12).is_ok() && fz.journal_order_ok()
    };
    v.push(("J1-事件账序核", jok, String::from("0→1 有序")));

    // 14. 版本值域：主段 0 拒 / 主段下界过。
    let val_ok = {
        let mut bad = ProfileRaw::empty();
        bad.units[0] = 0;
        bad.valid = true;
        let mut good = ProfileRaw::empty();
        good.units[0] = VER_MAJOR_MIN;
        good.valid = true;
        !bad.value_ok() && good.value_ok()
    };
    v.push(("J1-版本值域守卫", val_ok, format!("主段≥{}", VER_MAJOR_MIN)));

    // 15. 结构指纹双跑一致。
    let c1 = LayoutCand::frozen().fingerprint_of(0);
    let c2 = LayoutCand::frozen().fingerprint_of(0);
    v.push(("J2-结构指纹双跑", c1 == c2, format!("{:08X}", c1)));

    // 16. 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));

    // 17. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 18. 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = PmCode::ALL.len() == 12;
        let mut i = 0;
        while i < PmCode::ALL.len() {
            if PmCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集12码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 18 条；前 8 条动态明细）。
pub fn run_unx18541_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18541 伪装档案总纲");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18541_checks();
        assert!(set.all_passed(), "UNX-F18541 判据集应全绿");
    }

    /// E1 签名：偏移表前缀和闭合。
    #[test]
    fn test_e1_signature() {
        assert!(e1_signature_ok());
        assert_eq!(PROFILE_UNITS, 8);
        assert_eq!(field_width(0), Some(3));
    }

    /// 黄金链：双层按序冻结+解析语义。
    #[test]
    fn test_golden_run() {
        match golden_run() {
            Ok((n, led, _, _)) => {
                assert_eq!(n, 2);
                assert!(led.conserve());
                assert_eq!(led.passed, 2);
                assert_eq!(led.rejected, 0);
            }
            Err(e) => assert!(false, "黄金链不应失败: {}", detail(e)),
        }
    }

    /// 双层优先：per-app 胜出 / 空回退。
    #[test]
    fn test_layer_precedence() {
        let mut b = ProfileRaw::empty();
        b.units[0] = 6;
        b.valid = true;
        let mut a = ProfileRaw::empty();
        a.units[0] = 10;
        a.valid = true;
        assert_eq!(ProfilePair { bottle: b, app: a }.resolve().1, 1);
        assert_eq!(ProfilePair { bottle: b, app: ProfileRaw::empty() }.resolve().1, 0);
    }

    /// 注入四类全抓（10 轮）。
    #[test]
    fn test_injections_all_caught() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => {
                assert_eq!(l.truncate_caught, 10);
                assert_eq!(l.overlap_caught, 10);
                assert_eq!(l.layer_oob_caught, 10);
                assert_eq!(l.dup_caught, 10);
            }
            Err(e) => assert!(false, "注入全抓不应报错: {}", detail(e)),
        }
    }

    /// 结构校验：截断/重叠/越顶三闸。
    #[test]
    fn test_layout_gates() {
        let mut cand = LayoutCand::frozen();
        cand.count = 0;
        match validate_layout(&cand) {
            Err(c) => assert_eq!(c, PmCode::LayoutTruncated.code()),
            Ok(()) => assert!(false, "截断必须拒"),
        }
        let mut cand2 = LayoutCand::frozen();
        cand2.offsets[5] = 8;
        match validate_layout(&cand2) {
            Err(c) => assert_eq!(c, PmCode::LayoutOverrun.code()),
            Ok(()) => assert!(false, "越顶必须拒"),
        }
    }

    /// 冻结机闸位：乱序 / 重复 / 倒退 / 空账。
    #[test]
    fn test_freezer_gates() {
        assert!(probe_seq_b().is_err());
        assert!(probe_seq_c().is_err());
        let mut fz = MasterFreezer::new();
        match fz.freeze_layer(7, 0) {
            Err(c) => assert_eq!(c, PmCode::LayerOutOfRange.code()),
            Ok(()) => assert!(false, "层越界必须拒"),
        }
        assert!(fz.freeze_layer(0, 1).is_ok());
        match fz.freeze_layer(0, 1) {
            Err(c) => assert_eq!(c, PmCode::LayerDuplicate.code()),
            Ok(()) => assert!(false, "重复封层必须拒"),
        }
    }

    /// 事件账容量：4 槽满显性拒。
    #[test]
    fn test_journal_capacity() {
        let mut j = FreezeJournal::new();
        let mut k = 0;
        while k < 4 {
            assert!(j.push(FreezeRec { layer: k as u8, fingerprint: k as u32 }).is_ok());
            k += 1;
        }
        match j.push(FreezeRec { layer: 9, fingerprint: 0 }) {
            Err(c) => assert_eq!(c, PmCode::LayerDuplicate.code()),
            Ok(()) => assert!(false, "满账必须拒"),
        }
        assert_eq!(j.len, 4);
    }

    /// 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
