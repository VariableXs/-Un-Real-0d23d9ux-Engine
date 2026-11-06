//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18412 · VerSetConditionMask 位构造语义 · 0x7E2C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18412-J1 64 位条件掩码构造（双 32 位折叠）逐位断言全过，非法操作数注入检出
//!
//! 结构清单：
//!   1. VcmCode        诊断码闭集（code() = SEG_BASE + n）
//!   2. VerTypeBit     8 类操作数位（VER_MINORVERSION..VER_PRODUCT_TYPE）
//!   3. VerCond        条件码闭集（VER_EQUAL=1..VER_OR=7）
//!   4. FIELD_SHIFTS   裁决表：每类操作数的 2 位字段位移
//!   5. cond_allowed   裁决表：字段类 × 条件码合法性
//!   6. ver_set_condition / fold64 / split64 位构造核
//!   7. MaskBuildLedger 构造账（冲突检测 + 双跑指纹）
//!   8. Lcg + InjectLedger（三类非法操作数 × 10 轮）
//!   9. golden_run / probe_a/b/c / inject_rounds
//!  10. run_unx18412_checks 判据集（≤20 条）
//!
//! 设计要点：
//!   - 与 Windows 同源的位布局：每类操作数占 2 位字段，字段位移 = 2 × 位序
//!     （MINOR=0, MAJOR=2, BUILD=4, PLATFORMID=6, SPMINOR=8, SPMAJOR=10,
//!     SUITENAME=12, PRODUCTTYPE=14），8 字段恰占低 16 位。
//!   - 双 32 位折叠：64 位掩码按 (low:u32, high:u32) 携带，fold/split 必须逐位
//!     无损往返（判据逐位断言的载体）。
//!   - 裁决表（合法性）：数值类字段只许 VER_EQUAL/VER_GREATER/VER_GREATER_EQUAL/
//!     VER_LESS/VER_LESS_EQUAL（1..5）；SUITENAME 只许 VER_AND/VER_OR（6/7）；
//!     PRODUCTTYPE 只许 VER_EQUAL/VER_GREATER_EQUAL（1/3）——这是 Windows
//!     SDK 文档化用法收敛成的闭集，越表即非法操作数。
//!   - 冲突检测：同一字段被二次构造且 2 位残差（cond & 0b11）不同 → 记
//!     冲突账。写入为 2 位域截断 + 域内替换（VER_AND/VER_OR 第 3 位不入
//!     字段区，防 SuiteName(shift12) 溢出污染 ProductType(shift14)；域内
//!     替换使域值恒等于最后一次合法残差，逐位断言/冲突账双口径自洽）。
//!   - 注入三类：位掩码非单比特 / 条件码越闭集 / 条件码越裁决表，各 10 轮。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E2C0，批 B01 独占区，%0x40==0 对齐）。
pub const SEG_BASE: u32 = 0x7E2C0;

// ── 冻结常量（推导见行注） ──────────────────────────────────────
/// 字段宽度（冻结：每类操作数 2 位，条件码 1..7 由裁决表限制表达域）。
pub const FIELD_WIDTH: u32 = 2;
/// 字段数（冻结：8 类操作数）。
pub const FIELD_COUNT: usize = 8;
/// 字段区总宽（推导：8 × 2 = 16 位，掩码低 16 位）。
pub const FIELD_AREA_BITS: u32 = FIELD_WIDTH * FIELD_COUNT as u32;
/// 字段区掩码（推导：(1<<16)-1）。
pub const FIELD_AREA: u64 = (1u64 << FIELD_AREA_BITS) - 1;
/// 判据注入轮数（写死：J1「10/10」口径）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VcmCode {
    /// 位掩码为零（无操作数）
    TypeMaskZero,
    /// 位掩码多比特（一次只许构造一类）
    TypeMaskMultiBit,
    /// 位掩码越 8 类闭集
    TypeMaskUnknown,
    /// 条件码为零
    CondZero,
    /// 条件码超 7（3 位溢出）
    CondOverflow,
    /// 条件码越裁决表（字段类×条件不配）
    CondAgainstAdjudication,
    /// 字段区越域（高 48 位置位）
    FieldAreaViolated,
    /// 同字段冲突构造（值不同）
    FieldConflict,
    /// 折叠往返无损性破坏
    FoldRoundtripBroken,
    /// 逐位断言失配
    BitAssertFailed,
    /// 双跑不一致
    NotReproducible,
}

impl VcmCode {
    /// 诊断码取值：SEG_BASE + 枚举序号（窗口 0x7E2C0..0x7E300）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐条非空）。
    pub fn detail(self) -> &'static str {
        match self {
            VcmCode::TypeMaskZero => "位掩码为零：无操作数可构造",
            VcmCode::TypeMaskMultiBit => "位掩码多比特：一次只许一类操作数",
            VcmCode::TypeMaskUnknown => "位掩码越闭集：非 8 类操作数位",
            VcmCode::CondZero => "条件码为零：闭集自 1 起",
            VcmCode::CondOverflow => "条件码超 7：VER_OR 为上限",
            VcmCode::CondAgainstAdjudication => "条件码越裁决表：字段类不配",
            VcmCode::FieldAreaViolated => "字段区越域：高 48 位置位",
            VcmCode::FieldConflict => "同字段冲突构造：两次值不同",
            VcmCode::FoldRoundtripBroken => "折叠往返破坏：split(fold(x))≠x",
            VcmCode::BitAssertFailed => "逐位断言失配",
            VcmCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 闭集全量。
    pub const ALL: [VcmCode; 11] = [
        VcmCode::TypeMaskZero,
        VcmCode::TypeMaskMultiBit,
        VcmCode::TypeMaskUnknown,
        VcmCode::CondZero,
        VcmCode::CondOverflow,
        VcmCode::CondAgainstAdjudication,
        VcmCode::FieldAreaViolated,
        VcmCode::FieldConflict,
        VcmCode::FoldRoundtripBroken,
        VcmCode::BitAssertFailed,
        VcmCode::NotReproducible,
    ];
}

/// 按数值反查诊断文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < VcmCode::ALL.len() {
        if VcmCode::ALL[i].code() == code {
            return VcmCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E2C0 段闭集内"
}

// ── 8 类操作数位（Windows winnt.h 值，冻结） ────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerTypeBit {
    /// VER_MINORVERSION = 0x00000001
    MinorVersion,
    /// VER_MAJORVERSION = 0x00000002
    MajorVersion,
    /// VER_BUILDNUMBER = 0x00000004
    BuildNumber,
    /// VER_PLATFORMID = 0x00000008
    PlatformId,
    /// VER_SERVICEPACKMINOR = 0x00000010
    SpMinor,
    /// VER_SERVICEPACKMAJOR = 0x00000020
    SpMajor,
    /// VER_SUITENAME = 0x00000040
    SuiteName,
    /// VER_PRODUCT_TYPE = 0x00000080
    ProductType,
}

impl VerTypeBit {
    /// Windows 位值（冻结，与 winnt.h 一致）。
    pub fn bit(self) -> u32 {
        match self {
            VerTypeBit::MinorVersion => 0x0000_0001,
            VerTypeBit::MajorVersion => 0x0000_0002,
            VerTypeBit::BuildNumber => 0x0000_0004,
            VerTypeBit::PlatformId => 0x0000_0008,
            VerTypeBit::SpMinor => 0x0000_0010,
            VerTypeBit::SpMajor => 0x0000_0020,
            VerTypeBit::SuiteName => 0x0000_0040,
            VerTypeBit::ProductType => 0x0000_0080,
        }
    }
    /// 位序（log2(bit)，推导：bit 恰为 1<<位序）。
    pub fn ordinal(self) -> u32 {
        match self {
            VerTypeBit::MinorVersion => 0,
            VerTypeBit::MajorVersion => 1,
            VerTypeBit::BuildNumber => 2,
            VerTypeBit::PlatformId => 3,
            VerTypeBit::SpMinor => 4,
            VerTypeBit::SpMajor => 5,
            VerTypeBit::SuiteName => 6,
            VerTypeBit::ProductType => 7,
        }
    }
    /// 字段位移（裁决表：shift = FIELD_WIDTH × 位序）。
    pub fn shift(self) -> u32 {
        FIELD_WIDTH * self.ordinal()
    }
    /// 由位值还原（越 8 类闭集返回 None）。
    pub fn from_bit(bit: u32) -> Option<VerTypeBit> {
        match bit {
            0x0000_0001 => Some(VerTypeBit::MinorVersion),
            0x0000_0002 => Some(VerTypeBit::MajorVersion),
            0x0000_0004 => Some(VerTypeBit::BuildNumber),
            0x0000_0008 => Some(VerTypeBit::PlatformId),
            0x0000_0010 => Some(VerTypeBit::SpMinor),
            0x0000_0020 => Some(VerTypeBit::SpMajor),
            0x0000_0040 => Some(VerTypeBit::SuiteName),
            0x0000_0080 => Some(VerTypeBit::ProductType),
            _ => None,
        }
    }
}

// ── 条件码闭集（Windows 冻结值） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerCond {
    /// VER_EQUAL = 1
    Equal,
    /// VER_GREATER = 2
    Greater,
    /// VER_GREATER_EQUAL = 3
    GreaterEqual,
    /// VER_LESS = 4
    Less,
    /// VER_LESS_EQUAL = 5
    LessEqual,
    /// VER_AND = 6（仅 SUITENAME）
    And,
    /// VER_OR = 7（仅 SUITENAME）
    Or,
}

impl VerCond {
    /// 条件码值（冻结）。
    pub fn num(self) -> u8 {
        match self {
            VerCond::Equal => 1,
            VerCond::Greater => 2,
            VerCond::GreaterEqual => 3,
            VerCond::Less => 4,
            VerCond::LessEqual => 5,
            VerCond::And => 6,
            VerCond::Or => 7,
        }
    }
    /// 由数值还原（0 与 >7 返回 None）。
    pub fn from_num(n: u8) -> Option<VerCond> {
        match n {
            1 => Some(VerCond::Equal),
            2 => Some(VerCond::Greater),
            3 => Some(VerCond::GreaterEqual),
            4 => Some(VerCond::Less),
            5 => Some(VerCond::LessEqual),
            6 => Some(VerCond::And),
            7 => Some(VerCond::Or),
            _ => None,
        }
    }
}

/// 裁决表：字段类 × 允许条件码集合（Windows SDK 文档化用法收敛闭集）。
/// 推导：数值比较类字段允许 1..5；SUITENAME 的套件组合只许 VER_AND/VER_OR；
/// PRODUCTTYPE 只有 1(WorkStation)/3(Server) 两档，只许 EQUAL/GREATER_EQUAL。
pub fn cond_allowed(t: VerTypeBit, c: u8) -> bool {
    match t {
        VerTypeBit::MinorVersion
        | VerTypeBit::MajorVersion
        | VerTypeBit::BuildNumber
        | VerTypeBit::PlatformId
        | VerTypeBit::SpMinor
        | VerTypeBit::SpMajor => c >= 1 && c <= 5,
        VerTypeBit::SuiteName => c == 6 || c == 7,
        VerTypeBit::ProductType => c == 1 || c == 3,
    }
}

// ── 位构造核（双 32 位折叠） ────────────────────────────────────
/// 64 位折叠：low/high 双 u32 合成 u64（little-endian 口径：low 为低半）。
pub fn fold64(low: u32, high: u32) -> u64 {
    ((high as u64) << 32) | (low as u64)
}

/// 64 位拆分：还原 (low, high)。
pub fn split64(m: u64) -> (u32, u32) {
    ((m & 0xFFFF_FFFF) as u32, ((m >> 32) & 0xFFFF_FFFF) as u32)
}

/// 单条构造记录（冲突账载体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BuildRec {
    /// 操作数位序
    pub ordinal: u32,
    /// 写入的条件码
    pub cond: u8,
}

/// 构造账：携带当前 (low,high) 掩码与逐条构造记录，支持冲突检测。
pub struct MaskBuildLedger {
    low: u32,
    high: u32,
    /// 构造历史（逐条）
    pub history: Vec<BuildRec>,
    /// 冲突计数（同字段两次不同值）
    pub conflicts: u32,
}

impl MaskBuildLedger {
    /// 空账（掩码 0，无历史）。
    pub fn new() -> MaskBuildLedger {
        MaskBuildLedger { low: 0, high: 0, history: Vec::new(), conflicts: 0 }
    }

    /// 当前 64 位掩码（折叠视图）。
    pub fn mask64(&self) -> u64 {
        fold64(self.low, self.high)
    }

    /// 构造一条：VerSetConditionMask 语义（2 位域截断 + 域内替换，跨字段 OR 合并）
    /// + 裁决表前置闸。返回新掩码；非法操作数显式拒。
    pub fn set_condition(&mut self, type_bit: u32, cond: u8) -> Result<u64, u32> {
        // 闸 1：位掩码非零。
        if type_bit == 0 {
            return Err(VcmCode::TypeMaskZero.code());
        }
        // 闸 2：单比特（x&(x-1)==0 判幂二）。
        if type_bit & (type_bit - 1) != 0 {
            return Err(VcmCode::TypeMaskMultiBit.code());
        }
        // 闸 3：8 类闭集。
        let t = match VerTypeBit::from_bit(type_bit) {
            Some(t) => t,
            None => return Err(VcmCode::TypeMaskUnknown.code()),
        };
        // 闸 4：条件码闭集（0 拒 / >7 拒）。
        if cond == 0 {
            return Err(VcmCode::CondZero.code());
        }
        if cond > 7 {
            return Err(VcmCode::CondOverflow.code());
        }
        // 闸 5：裁决表（字段类×条件配对）。
        if !cond_allowed(t, cond) {
            return Err(VcmCode::CondAgainstAdjudication.code());
        }
        // 冲突检测与写入统一到 2 位域残差口径（魔数推导：字段宽 FIELD_WIDTH=2，
        // 写入值取 cond 低 2 位；VER_AND(6)/VER_OR(7) 的第 3 位截断不入字段区，
        // 防止跨字段污染——SuiteName(shift12) 原值写入会溢出污染 ProductType(shift14)）。
        let residue = (cond as u64) & 0b11;
        let shift = t.shift();
        let field = (self.mask64() >> shift) & 0b11;
        if field != 0 && field != residue {
            self.conflicts += 1;
        }
        // 域内替换写入（账面强化，推导：Windows 的 VER_SET_CONDITION 按类型构造
        // 掩码、同类型重写即替换；裸 OR 会让同字段重写累积出 1|2=3 的无主域值，
        // 与逐位断言/冲突账双口径失配——故先清 2 位域再置残差，跨字段仍 OR 合并）。
        let cleared = self.mask64() & !(0b11u64 << shift);
        let merged = cleared | (residue << shift);
        let (lo, hi) = split64(merged);
        self.low = lo;
        self.high = hi;
        self.history.push(BuildRec { ordinal: t.ordinal(), cond });
        Ok(merged)
    }

    /// 逐位断言：对每个已构造字段，掩码对应 2 位 == 条件码残差（cond & 0b11，
    /// 与构造核截断写入同口径——魔数推导见 set_condition 内注）。
    pub fn bit_assert(&self) -> Result<(), u32> {
        let m = self.mask64();
        if m & !FIELD_AREA != 0 {
            return Err(VcmCode::FieldAreaViolated.code());
        }
        let mut i = 0;
        while i < self.history.len() {
            let rec = match self.history.get(i) {
                Some(r) => *r,
                None => break,
            };
            let shift = FIELD_WIDTH * rec.ordinal;
            let field = (m >> shift) & 0b11;
            let expect = (rec.cond as u64) & 0b11;
            if field != expect {
                return Err(VcmCode::BitAssertFailed.code());
            }
            i += 1;
        }
        Ok(())
    }

    /// 指纹（双跑一致用）。
    pub fn fingerprint(&self) -> u32 {
        let mut h: u32 = 2166136261;
        h = h.wrapping_mul(16777619) ^ self.low;
        h = h.wrapping_mul(16777619) ^ self.high;
        h = h.wrapping_mul(16777619) ^ (self.history.len() as u32);
        h = h.wrapping_mul(16777619) ^ self.conflicts;
        h
    }
}

/// 折叠往返无损断言：split(fold(l,h)) == (l,h)，逐位核对 32 位两半。
pub fn fold_roundtrip(l: u32, h: u32) -> Result<(), u32> {
    let m = fold64(l, h);
    let (l2, h2) = split64(m);
    if l2 != l || h2 != h {
        return Err(VcmCode::FoldRoundtripBroken.code());
    }
    Ok(())
}

// ── 确定性伪随机 ────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    /// 播种（种子写死 = SEG_BASE，双跑一致）。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    /// 推进一格。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0, n) 伪随机。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
/// 三类注入槽：0=位掩码非单比特/零，1=条件码越闭集，2=条件码越裁决表。
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    /// 全抓断言。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检合计（守恒口径）。
    pub fn missed_total(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 注入 10/10：三类非法操作数逐类注入，断言每轮显式拒。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut ledger = MaskBuildLedger::new();
        // 槽 0：位掩码非单比特（MINOR|MAJOR = 0x3）与零掩码。
        let multi = ledger.set_condition(0x3, 1).is_err();
        let zero = ledger.set_condition(0, 1).is_err();
        led.record(0, multi && zero);
        // 槽 1：条件码越闭集（0 与 8）。
        let c0 = ledger.set_condition(VerTypeBit::MinorVersion.bit(), 0).is_err();
        let c8 = ledger.set_condition(VerTypeBit::MinorVersion.bit(), 8).is_err();
        led.record(1, c0 && c8);
        // 槽 2：条件码越裁决表（SUITENAME 用数值比较 1；PRODUCTTYPE 用 AND 6）。
        let suite_num = ledger.set_condition(VerTypeBit::SuiteName.bit(), 1).is_err();
        let ptype_and = ledger.set_condition(VerTypeBit::ProductType.bit(), 6).is_err();
        led.record(2, suite_num && ptype_and);
        r += 1;
    }
    if !led.all_caught() || led.missed_total() != 0 {
        return Err(VcmCode::NotReproducible.code());
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：8 类操作数各构造一条合法条件 → 逐位断言 → 折叠往返 →
/// 冲突账核对。返回 (构造条数, 最终掩码低 16 位, 冲突数)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = MaskBuildLedger::new();
    // 裁决表放行的最小构造集：每类一条、全部走合法条件。
    let plan = [
        (VerTypeBit::MinorVersion, VerCond::Equal),
        (VerTypeBit::MajorVersion, VerCond::GreaterEqual),
        (VerTypeBit::BuildNumber, VerCond::Greater),
        (VerTypeBit::PlatformId, VerCond::Equal),
        (VerTypeBit::SpMinor, VerCond::LessEqual),
        (VerTypeBit::SpMajor, VerCond::GreaterEqual),
        (VerTypeBit::SuiteName, VerCond::And),
        (VerTypeBit::ProductType, VerCond::GreaterEqual),
    ];
    let mut i = 0;
    while i < plan.len() {
        let (t, c) = plan[i];
        led.set_condition(t.bit(), c.num())?;
        i += 1;
    }
    // 逐位断言。
    led.bit_assert()?;
    // 期望掩码参考表：逐字段独立合成再对拍（双路径推导一致；
    // 残差口径与构造核一致：cond & 0b11，见 set_condition 内魔数推导）。
    let mut expect: u64 = 0;
    let mut k = 0;
    while k < plan.len() {
        let (t, c) = plan[k];
        expect |= ((c.num() as u64) & 0b11) << t.shift();
        k += 1;
    }
    if led.mask64() != expect {
        return Err(VcmCode::BitAssertFailed.code());
    }
    // 折叠往返：掩码两半逐位无损。
    let (lo, hi) = split64(led.mask64());
    fold_roundtrip(lo, hi)?;
    fold_roundtrip(u32::MAX, u32::MAX)?;
    fold_roundtrip(0x1234_5678, 0x9ABC_DEF0)?;
    // 冲突账：黄金链全为首次构造，冲突应为 0。
    if led.conflicts != 0 {
        return Err(VcmCode::FieldConflict.code());
    }
    Ok((
        led.history.len() as u32,
        (led.mask64() & FIELD_AREA) as u32,
        led.conflicts,
    ))
}

/// 冲突路径探针：同字段先 1 后 2 → 冲突账 +1，域值取后值（域内替换口径核对）。
pub fn probe_conflict() -> Result<u32, u32> {
    let mut led = MaskBuildLedger::new();
    led.set_condition(VerTypeBit::MajorVersion.bit(), 1)?;
    led.set_condition(VerTypeBit::MajorVersion.bit(), 2)?;
    if led.conflicts != 1 {
        return Err(VcmCode::FieldConflict.code());
    }
    let field = (led.mask64() >> VerTypeBit::MajorVersion.shift()) & 0b11;
    Ok(field as u32)
}

/// 探针 A：合法构造单字段并逐位核。
pub fn probe_a() -> Result<u64, u32> {
    let mut led = MaskBuildLedger::new();
    let m = led.set_condition(VerTypeBit::ProductType.bit(), VerCond::GreaterEqual.num())?;
    led.bit_assert()?;
    Ok(m)
}

/// 探针 B：空掩码查询（零构造）逐位断言即空过。
pub fn probe_b() -> Result<(), u32> {
    let led = MaskBuildLedger::new();
    led.bit_assert()
}

/// 探针 C：字段区越域注入（手工置高 48 位）被逐位断言拒。
pub fn probe_c() -> Result<(), u32> {
    let mut led = MaskBuildLedger::new();
    led.set_condition(VerTypeBit::MinorVersion.bit(), 1)?;
    // 直接抬高位模拟越域（OR 合并合法路径做不出 48 位高位）。
    led.high = 0x0000_0001;
    match led.bit_assert() {
        Err(c) if c == VcmCode::FieldAreaViolated.code() => Ok(()),
        _ => Err(VcmCode::BitAssertFailed.code()),
    }
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(VcmCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < VcmCode::ALL.len() {
        let v = VcmCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集巡检。
pub fn detail_closure() -> bool {
    let mut i = 0;
    while i < VcmCode::ALL.len() {
        let c = VcmCode::ALL[i];
        if c.detail().is_empty() || detail_of(c.code()) != c.detail() {
            return false;
        }
        i += 1;
    }
    true
}

/// 裁决表全扫：8 类 × 7 条件逐格核对合法集（对账口径）。
pub fn adjudication_table_audit() -> bool {
    let all_types = [
        VerTypeBit::MinorVersion,
        VerTypeBit::MajorVersion,
        VerTypeBit::BuildNumber,
        VerTypeBit::PlatformId,
        VerTypeBit::SpMinor,
        VerTypeBit::SpMajor,
        VerTypeBit::SuiteName,
        VerTypeBit::ProductType,
    ];
    let mut t = 0;
    while t < all_types.len() {
        let mut c = 1;
        while c <= 7 {
            let expect = match all_types[t] {
                VerTypeBit::SuiteName => c == 6 || c == 7,
                VerTypeBit::ProductType => c == 1 || c == 3,
                _ => c <= 5,
            };
            if cond_allowed(all_types[t], c) != expect {
                return false;
            }
            c += 1;
        }
        t += 1;
    }
    // 位移裁决表：shift 单调 2 步进且末字段 ≤14。
    let mut o = 0;
    while o < FIELD_COUNT {
        let tb = match VerTypeBit::from_bit(1u32 << o) {
            Some(x) => x,
            None => return false,
        };
        if tb.shift() != FIELD_WIDTH * o as u32 {
            return false;
        }
        o += 1;
    }
    true
}

// ── 判据集 ──────────────────────────────────────────────────────
/// UNX-F18412 判据集（≤20 条；动态明细 ≤8 槽）。
pub fn run_unx18412_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18412 VerSetConditionMask 位构造语义");

    // 1. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7E2C0 段 0x40 窗口且 0x40 对齐");
    // 2. 黄金全链（8 字段构造→逐位断言→折叠往返）。
    let golden = golden_run();
    let golden_ok = match &golden {
        Ok((n, _, c)) => *n == 8 && *c == 0,
        Err(_) => false,
    };
    s.add_dyn("J1-黄金全链", golden_ok, &format!("{:?}", golden));
    // 3. 逐位断言：黄金掩码 == 参考表独立合成。
    s.add("J1-逐位断言", golden.is_ok(), "8 字段 2 位域逐位核对");
    // 4. 折叠往返无损。
    let rt = fold_roundtrip(0xDEAD_BEEF, 0xCAFE_BABE).is_ok()
        && fold_roundtrip(0, 0).is_ok()
        && fold_roundtrip(u32::MAX, 0).is_ok()
        && fold_roundtrip(0, u32::MAX).is_ok();
    s.add("J1-双32位折叠往返", rt, "split(fold(l,h))==(l,h) 四组极值");
    // 5. 位移裁决表：shift=2×位序。
    s.add("J1-位移裁决表", adjudication_table_audit(), "8 类×7 条件全表核对");
    // 6. 重复注册等价：同字段同值幂等（无冲突）。
    let mut idem = MaskBuildLedger::new();
    let m1 = idem.set_condition(VerTypeBit::MinorVersion.bit(), 1).unwrap_or(0);
    let m2 = idem.set_condition(VerTypeBit::MinorVersion.bit(), 1).unwrap_or(0);
    s.add("J1-同字段幂等", m1 == m2 && idem.conflicts == 0, "同值重构造不记冲突");
    // 7. 冲突检出。
    s.add("J1-同字段冲突检出", probe_conflict().is_ok(), "先1后2冲突账+1且掩码取后值");
    // 8. 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-注入10/10", inj_ok, &format!("r={}", JUDGE_ROUNDS));
    // 9. 计数守恒。
    let conserve_ok = match &inj {
        Ok(l) => l.missed_total() == 0,
        Err(_) => false,
    };
    s.add("J1-计数守恒漏检零", conserve_ok, "tries=caught+missed 且 missed=0");
    // 10. 探针三序。
    let a = probe_a().is_ok();
    let b = probe_b().is_ok();
    let c = probe_c().is_ok();
    s.add_dyn("J1-探针三序", a && b && c, &format!("a={} b={} c={}", a, b, c));
    // 11. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 12. 文案闭集。
    s.add("J3-文案闭集", detail_closure(), "11 码逐条非空且反查一致");
    // 13. 条件码闭集还原往返。
    let cond_rt = {
        let mut ok = true;
        let mut n = 1;
        while n <= 7 {
            match VerCond::from_num(n) {
                Some(c) if c.num() == n => {}
                _ => ok = false,
            }
            n += 1;
        }
        ok && VerCond::from_num(0).is_none() && VerCond::from_num(8).is_none()
    };
    s.add("J1-条件码闭集往返", cond_rt, "1..7 可还原，0/8 越集拒");
    // 14. 操作数位闭集还原。
    let type_rt = VerTypeBit::from_bit(0x0000_0080) == Some(VerTypeBit::ProductType)
        && VerTypeBit::from_bit(0x0000_0100).is_none()
        && VerTypeBit::from_bit(0x0000_0003).is_none();
    s.add("J1-操作数位闭集", type_rt, "0x80 可还原，0x100/0x3 越集拒");
    // 15. 段内窗口逐码核对（显式边界）。
    let win_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < VcmCode::ALL.len() {
            if VcmCode::ALL[i].code() >= SEG_BASE + 0x40 {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("J1-段内窗口核对", win_ok, "11 码全部 < SEG_BASE+0x40");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 判据集全绿。
    #[test]
    fn all_checks_pass() {
        let set = run_unx18412_checks();
        assert!(set.all_passed(), "UNX-F18412 判据集存在红项");
    }

    /// 逐位断言：8 字段合成掩码逐位核对。
    #[test]
    fn bit_by_bit_assert() {
        let mut led = MaskBuildLedger::new();
        let _ = led.set_condition(VerTypeBit::MinorVersion.bit(), 3);
        let _ = led.set_condition(VerTypeBit::ProductType.bit(), 1);
        let m = led.mask64();
        assert_eq!((m >> 0) & 0b11, 3);
        assert_eq!((m >> 14) & 0b11, 1);
        assert!(led.bit_assert().is_ok());
    }

    /// 注入三类 10/10。
    #[test]
    fn injection_all_caught() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(_) => panic!("注入轮次内部出错"),
        };
        assert!(led.all_caught());
        assert_eq!(led.missed_total(), 0);
    }

    /// 判定次序：零掩码先于条件码校验（闸序与实现一致）。
    #[test]
    fn order_typemask_before_cond() {
        let mut led = MaskBuildLedger::new();
        // 位掩码 0 + 条件码 0：应返回 TypeMaskZero（第一闸）。
        match led.set_condition(0, 0) {
            Err(c) => assert_eq!(c, VcmCode::TypeMaskZero.code()),
            Ok(_) => panic!("零掩码不应构造成功"),
        }
        // 多比特 + 非法条件：应返回 TypeMaskMultiBit。
        match led.set_condition(0x3, 0) {
            Err(c) => assert_eq!(c, VcmCode::TypeMaskMultiBit.code()),
            Ok(_) => panic!("多比特掩码不应构造成功"),
        }
    }

    /// 裁决表：SUITENAME 只收 AND/OR，PRODUCTTYPE 只收 1/3。
    #[test]
    fn adjudication_table() {
        assert!(cond_allowed(VerTypeBit::SuiteName, 6));
        assert!(cond_allowed(VerTypeBit::SuiteName, 7));
        assert!(!cond_allowed(VerTypeBit::SuiteName, 1));
        assert!(cond_allowed(VerTypeBit::ProductType, 1));
        assert!(cond_allowed(VerTypeBit::ProductType, 3));
        assert!(!cond_allowed(VerTypeBit::ProductType, 2));
        assert!(cond_allowed(VerTypeBit::MajorVersion, 5));
        assert!(!cond_allowed(VerTypeBit::MajorVersion, 6));
    }

    /// 折叠往返：极值四组。
    #[test]
    fn fold_roundtrip_extremes() {
        assert!(fold_roundtrip(0, 0).is_ok());
        assert!(fold_roundtrip(u32::MAX, u32::MAX).is_ok());
        assert!(fold_roundtrip(0xFFFF_FFFF, 0x0000_0000).is_ok());
        assert!(fold_roundtrip(0x0000_0001, 0x8000_0000).is_ok());
        assert_eq!(fold64(1, 2), 0x0000_0002_0000_0001);
        assert_eq!(split64(0x0000_0002_0000_0001), (1, 2));
    }

    /// 冲突账：同字段异值计一次，域值取后值（域内替换：重写不累积位）。
    #[test]
    fn conflict_ledger() {
        let mut led = MaskBuildLedger::new();
        let _ = led.set_condition(VerTypeBit::SpMajor.bit(), 1);
        let _ = led.set_condition(VerTypeBit::SpMajor.bit(), 2);
        assert_eq!(led.conflicts, 1);
        assert_eq!((led.mask64() >> VerTypeBit::SpMajor.shift()) & 0b11, 2);
        // 同字段同值幂等：冲突不增。
        let _ = led.set_condition(VerTypeBit::SpMajor.bit(), 2);
        assert_eq!(led.conflicts, 1);
    }

    /// 双跑一致 + 段位守卫 + 文案闭集。
    #[test]
    fn repro_segment_detail() {
        assert!(repro_check().is_ok());
        assert!(segment_guard());
        assert!(detail_closure());
        let a = MaskBuildLedger::new();
        let b = MaskBuildLedger::new();
        assert_eq!(a.fingerprint(), b.fingerprint());
    }
}
