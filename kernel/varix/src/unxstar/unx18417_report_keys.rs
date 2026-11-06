//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18417 · 版本谎报总开关数据结构（version_report_matrix 预留） · 0x7E400段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18417-J1 三元组（进程,API,维度）键结构冻结 v1 与 B02 消费签名一致
//!
//! 结构清单：
//!   1. RmkCode        诊断码闭集（code() = SEG_BASE + n）
//!   2. MatrixApi      API 闭集（6 类，序号冻结）
//!   3. ReportDim      报告维度闭集（8 维，序号冻结）
//!   4. ReportKey      三元组键（冻结 v1 布局：u64 复合）
//!   5. KeyCodec       键编解码核（pack/unpack + 布局版本）
//!   6. B02Consumer    B02 消费签名（字段序/位宽/口径三冻结）
//!   7. MatrixTable    定容矩阵账（32 槽：插入/查询/冻结/去重）
//!   8. Lcg + InjectLedger（三类违约 × 10 轮）
//!   9. golden_run / probe_a/b/c / inject_rounds
//!  10. run_unx18417_checks 判据集（≤20 条）
//!
//! 设计要点：
//!   - 三元组（进程,API,维度）键冻结 v1：u64 复合布局 = pid[32] | api[8] |
//!     dim[8] | 保留[16]，字段序固定 little-endian 语义（低位在前）；
//!     KEY_LAYOUT_V1 = 1 写死，任何布局演进必须升版（v1 账只读）。
//!   - B02 消费签名三冻结：字段序 (pid,api,dim)、位宽 (32,8,8)、闭集域
//!     （api 1..=6 / dim 1..=8）——与 B02 消费端描述符表逐项一致。
//!   - 矩阵账：32 槽定容，键唯一（重复拒）、冻结槽只读、越域键拒；
//!     查询走 pack 后指纹定位（与消费端同路径，保证口径一致）。
//!   - 注入三类：越域键（api/dim 越闭集）/ 重复键 / 冻结槽改写，各 10 轮。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E400，批 B01 独占区，%0x40==0 对齐）。
pub const SEG_BASE: u32 = 0x7E400;

// ── 冻结常量（推导见行注） ──────────────────────────────────────
/// 键布局版本（冻结 v1：pid|api|dim|保留 复合布局）。
pub const KEY_LAYOUT_V1: u32 = 1;
/// 矩阵容量（冻结：32 = 6 API × 8 维 ÷ 1.5 余量取整）。
pub const MATRIX_CAP: usize = 32;
/// pid 位宽（冻结 v1）。
pub const PID_BITS: u32 = 32;
/// api 位宽（冻结 v1）。
pub const API_BITS: u32 = 8;
/// dim 位宽（冻结 v1）。
pub const DIM_BITS: u32 = 8;
/// api 字段位移（推导：紧跟 pid 低位之后）。
pub const API_SHIFT: u32 = PID_BITS;
/// dim 字段位移（推导：api 之后）。
pub const DIM_SHIFT: u32 = API_SHIFT + API_BITS;
/// 判据注入轮数（写死：J1「10/10」口径）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RmkCode {
    /// 空矩阵查询
    EmptyMatrix,
    /// 矩阵满
    MatrixFull,
    /// api 越闭集（0 或 >6）
    ApiOutOfRange,
    /// dim 越闭集（0 或 >8）
    DimOutOfRange,
    /// 重复键（同三元组已入账）
    DuplicateKey,
    /// 冻结槽改写（v1 只读红线）
    FrozenSlotRewrite,
    /// 键布局版本不符（非 v1）
    LayoutVersionMismatch,
    /// pack/unpack 往返失配
    CodecRoundtripBroken,
    /// B02 消费签名不一致
    B02SignatureMismatch,
    /// 键序号越界（矩阵槽探测越界）
    SlotOutOfRange,
    /// 双跑不一致
    NotReproducible,
}

impl RmkCode {
    /// 诊断码取值：SEG_BASE + 枚举序号（窗口 0x7E400..0x7E440）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐条非空）。
    pub fn detail(self) -> &'static str {
        match self {
            RmkCode::EmptyMatrix => "空矩阵：无任何三元组入账",
            RmkCode::MatrixFull => "矩阵满：32 槽占尽",
            RmkCode::ApiOutOfRange => "api 越闭集：合法域 1..6",
            RmkCode::DimOutOfRange => "dim 越闭集：合法域 1..8",
            RmkCode::DuplicateKey => "重复键：同三元组已入账",
            RmkCode::FrozenSlotRewrite => "冻结槽改写：v1 只读红线",
            RmkCode::LayoutVersionMismatch => "键布局版本不符：须 v1",
            RmkCode::CodecRoundtripBroken => "pack/unpack 往返失配",
            RmkCode::B02SignatureMismatch => "B02 消费签名不一致",
            RmkCode::SlotOutOfRange => "矩阵槽探测越界",
            RmkCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 闭集全量。
    pub const ALL: [RmkCode; 10] = [
        RmkCode::EmptyMatrix,
        RmkCode::MatrixFull,
        RmkCode::ApiOutOfRange,
        RmkCode::DimOutOfRange,
        RmkCode::DuplicateKey,
        RmkCode::FrozenSlotRewrite,
        RmkCode::LayoutVersionMismatch,
        RmkCode::CodecRoundtripBroken,
        RmkCode::B02SignatureMismatch,
        RmkCode::NotReproducible,
    ];
}

/// 按数值反查诊断文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < RmkCode::ALL.len() {
        if RmkCode::ALL[i].code() == code {
            return RmkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E400 段闭集内"
}

// ── API 闭集（序号冻结，B02 同源） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MatrixApi {
    /// VerifyVersionInfoW = 1
    VerifyVersionInfoW,
    /// VerifyVersionInfoA = 2
    VerifyVersionInfoA,
    /// VerSetConditionMask = 3
    VerSetConditionMask,
    /// GetVersionExW = 4
    GetVersionExW,
    /// GetVersionExA = 5
    GetVersionExA,
    /// RtlGetVersion = 6
    RtlGetVersion,
}

impl MatrixApi {
    /// API 序号（冻结）。
    pub fn num(self) -> u32 {
        match self {
            MatrixApi::VerifyVersionInfoW => 1,
            MatrixApi::VerifyVersionInfoA => 2,
            MatrixApi::VerSetConditionMask => 3,
            MatrixApi::GetVersionExW => 4,
            MatrixApi::GetVersionExA => 5,
            MatrixApi::RtlGetVersion => 6,
        }
    }
    /// 由序号还原（越闭集 None）。
    pub fn from_num(n: u32) -> Option<MatrixApi> {
        match n {
            1 => Some(MatrixApi::VerifyVersionInfoW),
            2 => Some(MatrixApi::VerifyVersionInfoA),
            3 => Some(MatrixApi::VerSetConditionMask),
            4 => Some(MatrixApi::GetVersionExW),
            5 => Some(MatrixApi::GetVersionExA),
            6 => Some(MatrixApi::RtlGetVersion),
            _ => None,
        }
    }
}

// ── 维度闭集（序号冻结） ────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReportDim {
    /// 主版本 = 1
    MajorVersion,
    /// 次版本 = 2
    MinorVersion,
    /// 构建号 = 3
    BuildNumber,
    /// 平台 id = 4
    PlatformId,
    /// SP 主号 = 5
    SpMajor,
    /// SP 次号 = 6
    SpMinor,
    /// 套件掩码 = 7
    SuiteMask,
    /// 产品类型 = 8
    ProductType,
}

impl ReportDim {
    /// 维度序号（冻结）。
    pub fn num(self) -> u32 {
        match self {
            ReportDim::MajorVersion => 1,
            ReportDim::MinorVersion => 2,
            ReportDim::BuildNumber => 3,
            ReportDim::PlatformId => 4,
            ReportDim::SpMajor => 5,
            ReportDim::SpMinor => 6,
            ReportDim::SuiteMask => 7,
            ReportDim::ProductType => 8,
        }
    }
    /// 由序号还原（越闭集 None）。
    pub fn from_num(n: u32) -> Option<ReportDim> {
        match n {
            1 => Some(ReportDim::MajorVersion),
            2 => Some(ReportDim::MinorVersion),
            3 => Some(ReportDim::BuildNumber),
            4 => Some(ReportDim::PlatformId),
            5 => Some(ReportDim::SpMajor),
            6 => Some(ReportDim::SpMinor),
            7 => Some(ReportDim::SuiteMask),
            8 => Some(ReportDim::ProductType),
            _ => None,
        }
    }
}

// ── 三元组键 + 编解码核 ────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReportKey {
    /// 进程 id（任意 32 位，0 合法=内核自身）
    pub pid: u32,
    /// API 序号
    pub api: u32,
    /// 维度序号
    pub dim: u32,
}

/// 键域守卫：api 1..=6 且 dim 1..=8（B02 消费闭集同源）。
pub fn key_legal(k: &ReportKey) -> bool {
    MatrixApi::from_num(k.api).is_some() && ReportDim::from_num(k.dim).is_some()
}

/// pack：三元组 → u64 复合（v1 布局）。非法 api/dim 显式拒。
pub fn key_pack(k: &ReportKey) -> Result<u64, u32> {
    if !key_legal(k) {
        // 区分越域侧：api 优先（字段序裁决）。
        if MatrixApi::from_num(k.api).is_none() {
            return Err(RmkCode::ApiOutOfRange.code());
        }
        return Err(RmkCode::DimOutOfRange.code());
    }
    let mut v: u64 = k.pid as u64;
    v |= (k.api as u64) << API_SHIFT;
    v |= (k.dim as u64) << DIM_SHIFT;
    Ok(v)
}

/// unpack：u64 复合 → 三元组（保留位必须为零，v1 布局守卫）。
pub fn key_unpack(v: u64) -> Result<ReportKey, u32> {
    // 保留位域：dim 字段之上高 16 位（bit48..63）必须为零（v1 布局冻结）。
    let reserved_mask: u64 = !((1u64 << (DIM_SHIFT + DIM_BITS)) - 1);
    if v & reserved_mask != 0 {
        return Err(RmkCode::LayoutVersionMismatch.code());
    }
    let pid = (v & 0xFFFF_FFFF) as u32;
    let api = ((v >> API_SHIFT) & 0xFF) as u32;
    let dim = ((v >> DIM_SHIFT) & 0xFF) as u32;
    let k = ReportKey { pid, api, dim };
    if !key_legal(&k) {
        if MatrixApi::from_num(api).is_none() {
            return Err(RmkCode::ApiOutOfRange.code());
        }
        return Err(RmkCode::DimOutOfRange.code());
    }
    Ok(k)
}

/// 往返断言：unpack(pack(k)) == k。
pub fn key_roundtrip(k: &ReportKey) -> Result<(), u32> {
    let v = key_pack(k)?;
    let k2 = key_unpack(v)?;
    if k2.pid != k.pid || k2.api != k.api || k2.dim != k.dim {
        return Err(RmkCode::CodecRoundtripBroken.code());
    }
    Ok(())
}

/// B02 消费描述符（三冻结：字段序/位宽/闭集域）。
pub struct B02Consumer {
    /// 字段序（冻结：pid → api → dim）
    pub field_order: [u8; 3],
    /// 位宽（冻结：32/8/8）
    pub widths: [u32; 3],
    /// 布局版本
    pub layout: u32,
    /// api 闭集上限
    pub api_max: u32,
    /// dim 闭集上限
    pub dim_max: u32,
}

/// B02 消费签名一致性核：实现侧常量与消费端描述符逐项对表。
/// 推导：B02 端按 (pid,api,dim) 序低位在前消费，api 域 1..=6、dim 域
/// 1..=8、布局 v1——任何一项漂移即签名不一致。
pub fn b02_signature_check(c: &B02Consumer) -> Result<(), u32> {
    if c.field_order != [0, 1, 2] {
        return Err(RmkCode::B02SignatureMismatch.code());
    }
    if c.widths != [PID_BITS, API_BITS, DIM_BITS] {
        return Err(RmkCode::B02SignatureMismatch.code());
    }
    if c.layout != KEY_LAYOUT_V1 {
        return Err(RmkCode::LayoutVersionMismatch.code());
    }
    if c.api_max != 6 || c.dim_max != 8 {
        return Err(RmkCode::B02SignatureMismatch.code());
    }
    Ok(())
}

/// 实现侧默认描述符（B02 同源口径）。
pub fn b02_default() -> B02Consumer {
    B02Consumer {
        field_order: [0, 1, 2],
        widths: [PID_BITS, API_BITS, DIM_BITS],
        layout: KEY_LAYOUT_V1,
        api_max: 6,
        dim_max: 8,
    }
}

// ── 矩阵账 ──────────────────────────────────────────────────────
/// 单槽账目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MatrixSlot {
    /// 复合键
    pub packed: u64,
    /// 谎报总开关（0=诚实透传 1=按 matrix 谎报）
    pub switch_on: bool,
    /// 冻结标志（置位后只读）
    pub frozen: bool,
}

/// version_report_matrix 预留结构：32 槽定容账。
pub struct MatrixTable {
    slots: [Option<MatrixSlot>; MATRIX_CAP],
    /// 累计插入数
    pub insert_total: u32,
    /// 累计拒绝数
    pub rejected_total: u32,
    /// 冻结槽数
    pub frozen_count: u32,
}

impl MatrixTable {
    /// 新矩阵（全空）。
    pub fn new() -> MatrixTable {
        MatrixTable {
            slots: [None; MATRIX_CAP],
            insert_total: 0,
            rejected_total: 0,
            frozen_count: 0,
        }
    }

    /// 键定位：线性扫（32 槽规模无需哈希表）。
    fn find(&self, packed: u64) -> Option<usize> {
        let mut i = 0;
        while i < MATRIX_CAP {
            if let Some(s) = self.slots[i] {
                if s.packed == packed {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 插入：域闸 → 重复闸 → 空槽分配。开关与冻结位随账写入。
    pub fn insert(&mut self, k: &ReportKey, switch_on: bool) -> Result<u64, u32> {
        let packed = key_pack(k)?;
        if self.find(packed).is_some() {
            self.rejected_total += 1;
            return Err(RmkCode::DuplicateKey.code());
        }
        let mut slot: Option<usize> = None;
        let mut i = 0;
        while i < MATRIX_CAP {
            if self.slots[i].is_none() {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        let idx = match slot {
            Some(s) => s,
            None => {
                self.rejected_total += 1;
                return Err(RmkCode::MatrixFull.code());
            }
        };
        self.slots[idx] = Some(MatrixSlot { packed, switch_on, frozen: false });
        self.insert_total += 1;
        Ok(packed)
    }

    /// 查询：返回开关态（消费端 B02 的读路径）。
    pub fn query(&self, k: &ReportKey) -> Result<bool, u32> {
        let packed = key_pack(k)?;
        match self.find(packed) {
            Some(idx) => match self.slots[idx] {
                Some(s) => Ok(s.switch_on),
                None => Err(RmkCode::EmptyMatrix.code()),
            },
            None => Err(RmkCode::EmptyMatrix.code()),
        }
    }

    /// 冻结：槽只读化（单调，倒退拒）。
    pub fn freeze(&mut self, k: &ReportKey) -> Result<(), u32> {
        let packed = key_pack(k)?;
        match self.find(packed) {
            Some(idx) => {
                let already = match self.slots[idx] {
                    Some(s) => s.frozen,
                    None => false,
                };
                if !already {
                    if let Some(s) = self.slots[idx] {
                        self.slots[idx] = Some(MatrixSlot { frozen: true, ..s });
                    }
                    self.frozen_count += 1;
                }
                Ok(())
            }
            None => {
                self.rejected_total += 1;
                Err(RmkCode::EmptyMatrix.code())
            }
        }
    }

    /// 冻结槽改写探测：冻结后改开关必拒（v1 只读红线）。
    pub fn try_rewrite_frozen(&mut self, k: &ReportKey, _new_switch: bool) -> Result<(), u32> {
        let packed = key_pack(k)?;
        match self.find(packed) {
            Some(idx) => {
                let frozen = match self.slots[idx] {
                    Some(s) => s.frozen,
                    None => false,
                };
                if frozen {
                    return Err(RmkCode::FrozenSlotRewrite.code());
                }
                Ok(())
            }
            None => Err(RmkCode::EmptyMatrix.code()),
        }
    }

    /// 账导出：(复合键, 开关, 冻结) 逐槽清单（对账可导出）。
    pub fn export(&self) -> Vec<(u64, bool, bool)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < MATRIX_CAP {
            if let Some(s) = self.slots[i] {
                out.push((s.packed, s.switch_on, s.frozen));
            }
            i += 1;
        }
        out
    }

    /// 守恒：导出数 == 累计插入数。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.export().len() as u32 != self.insert_total {
            return Err(RmkCode::B02SignatureMismatch.code());
        }
        Ok(())
    }
}

// ── 确定性伪随机 ────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    /// 播种（种子 = SEG_BASE）。
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
/// 三类注入槽：0=越域键，1=重复键，2=冻结槽改写。
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
    /// 漏检合计。
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

/// 注入 10/10：三类违约逐类注入，断言每轮被显式检出。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut mx = MatrixTable::new();
        // 槽 0：越域键——api=0/7、dim=0/9 四向 pack 拒。
        let bad_api0 = key_pack(&ReportKey { pid: 1, api: 0, dim: 1 }).is_err();
        let bad_api7 = key_pack(&ReportKey { pid: 1, api: 7, dim: 1 }).is_err();
        let bad_dim0 = key_pack(&ReportKey { pid: 1, api: 1, dim: 0 }).is_err();
        let bad_dim9 = key_pack(&ReportKey { pid: 1, api: 1, dim: 9 }).is_err();
        led.record(0, bad_api0 && bad_api7 && bad_dim0 && bad_dim9);
        // 槽 1：重复键——同三元组二次插入必拒。
        let k = ReportKey { pid: 42, api: 1, dim: 3 };
        let first = mx.insert(&k, true).is_ok();
        let second = mx.insert(&k, false).is_err();
        led.record(1, first && second);
        // 槽 2：冻结槽改写——冻结后 try_rewrite 必拒。
        let k2 = ReportKey { pid: 43, api: 4, dim: 8 };
        let _ = mx.insert(&k2, false);
        let _ = mx.freeze(&k2);
        let rewrite = mx.try_rewrite_frozen(&k2, true).is_err();
        led.record(2, rewrite);
        r += 1;
    }
    if !led.all_caught() || led.missed_total() != 0 {
        return Err(RmkCode::NotReproducible.code());
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：6 API × 8 维 = 48 键超容量 → 取 32 键填满矩阵 → 32 槽
/// 查询逐键一致 → 全冻结 → 改写全拒 → 守恒导出。返回 (插入数, 冻结数)。
pub fn golden_run() -> Result<(u32, u32), u32> {
    // B02 签名先核对。
    b02_signature_check(&b02_default())?;
    let mut mx = MatrixTable::new();
    // 32 键填充：pid 1..8 × api 1..2 × dim 1..2 取前 32 组合。
    let mut n = 0;
    let mut pid = 1;
    while pid <= 8 && n < MATRIX_CAP {
        let mut api = 1;
        while api <= 2 && n < MATRIX_CAP {
            let mut dim = 1;
            while dim <= 2 && n < MATRIX_CAP {
                let k = ReportKey { pid, api, dim };
                let want_switch = (n % 2) == 0;
                mx.insert(&k, want_switch)?;
                n += 1;
                dim += 1;
            }
            api += 1;
        }
        pid += 1;
    }
    if mx.insert_total != MATRIX_CAP as u32 {
        return Err(RmkCode::MatrixFull.code());
    }
    // 表满后第 33 键必拒。
    let overflow = mx.insert(&ReportKey { pid: 99, api: 6, dim: 8 }, false).is_err();
    if !overflow {
        return Err(RmkCode::MatrixFull.code());
    }
    // 查询逐键一致（双路径：query 走 pack 定位）。
    let mut pid2 = 1;
    while pid2 <= 8 {
        let mut api2 = 1;
        while api2 <= 2 {
            let mut dim2 = 1;
            while dim2 <= 2 {
                let k = ReportKey { pid: pid2, api: api2, dim: dim2 };
                let got = mx.query(&k)?;
                // 开关态按插入序复算：n 序号 = (pid-1)*4 + (api-1)*2 + (dim-1)。
                let n_seq = ((pid2 - 1) * 4 + (api2 - 1) * 2 + (dim2 - 1)) as u32;
                if got != (n_seq % 2 == 0) {
                    return Err(RmkCode::CodecRoundtripBroken.code());
                }
                dim2 += 1;
            }
            api2 += 1;
        }
        pid2 += 1;
    }
    // 全冻结 → 改写全拒。
    let mut pid3 = 1;
    while pid3 <= 8 {
        let mut api3 = 1;
        while api3 <= 2 {
            let mut dim3 = 1;
            while dim3 <= 2 {
                let k = ReportKey { pid: pid3, api: api3, dim: dim3 };
                mx.freeze(&k)?;
                if mx.try_rewrite_frozen(&k, true).is_ok() {
                    return Err(RmkCode::FrozenSlotRewrite.code());
                }
                dim3 += 1;
            }
            api3 += 1;
        }
        pid3 += 1;
    }
    if mx.frozen_count != MATRIX_CAP as u32 {
        return Err(RmkCode::FrozenSlotRewrite.code());
    }
    mx.conserve()?;
    Ok((mx.insert_total, mx.frozen_count))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：键编解码往返（边界 pid 极值 + 全闭集扫描）。
pub fn probe_a() -> Result<u32, u32> {
    let mut ok = 0;
    // pid 极值。
    key_roundtrip(&ReportKey { pid: 0, api: 1, dim: 1 })?;
    key_roundtrip(&ReportKey { pid: u32::MAX, api: 6, dim: 8 })?;
    ok += 2;
    // 全闭集扫描：6×8=48 组合。
    let mut api = 1;
    while api <= 6 {
        let mut dim = 1;
        while dim <= 8 {
            key_roundtrip(&ReportKey { pid: 7, api, dim })?;
            ok += 1;
            dim += 1;
        }
        api += 1;
    }
    Ok(ok)
}

/// 探针 B：空矩阵查询/冻结必拒。
pub fn probe_b() -> Result<(), u32> {
    let mx = MatrixTable::new();
    let k = ReportKey { pid: 1, api: 1, dim: 1 };
    if mx.query(&k).is_err() {
        // 空矩阵 freeze 也拒（走同一 rejected 账）。
        let mut mx2 = MatrixTable::new();
        if mx2.freeze(&k).is_err() {
            return Ok(());
        }
    }
    Err(RmkCode::NotReproducible.code())
}

/// 探针 C：布局版本守卫——保留位置位 unpack 拒。
pub fn probe_c() -> Result<(), u32> {
    let v = key_pack(&ReportKey { pid: 1, api: 1, dim: 1 })?;
    let tampered = v | (1u64 << 63); // 高位置位 = 非 v1 布局
    match key_unpack(tampered) {
        Err(c) if c == RmkCode::LayoutVersionMismatch.code() => Ok(()),
        _ => Err(RmkCode::NotReproducible.code()),
    }
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RmkCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RmkCode::ALL.len() {
        let v = RmkCode::ALL[i].code();
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
    while i < RmkCode::ALL.len() {
        let c = RmkCode::ALL[i];
        if c.detail().is_empty() || detail_of(c.code()) != c.detail() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据集 ──────────────────────────────────────────────────────
/// UNX-F18417 判据集（≤20 条；动态明细 ≤8 槽）。
pub fn run_unx18417_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18417 版本谎报总开关数据结构");

    // 1. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7E400 段 0x40 窗口且 0x40 对齐");
    // 2. 黄金全链。
    let golden = golden_run();
    let golden_ok = match &golden {
        Ok((ins, frz)) => *ins == 32 && *frz == 32,
        Err(_) => false,
    };
    s.add_dyn("J1-黄金全链", golden_ok, &format!("{:?}", golden));
    // 3. 三元组键布局 v1 冻结（位移推导核对）。
    let layout_ok = API_SHIFT == 32 && DIM_SHIFT == 40 && KEY_LAYOUT_V1 == 1;
    s.add("J1-键布局v1冻结", layout_ok, "pid[32]|api[8]|dim[8]|保留[16]");
    // 4. B02 消费签名一致（J1 主判据）。
    let sig_ok = b02_signature_check(&b02_default()).is_ok();
    s.add("J1-B02消费签名一致", sig_ok, "字段序/位宽/闭集三冻结对表");
    // 5. B02 签名漂移检出（位宽改 16 必拒）。
    let drift = B02Consumer { field_order: [0, 1, 2], widths: [32, 16, 8], layout: 1, api_max: 6, dim_max: 8 };
    s.add("J1-签名漂移检出", b02_signature_check(&drift).is_err(), "api 位宽 16≠8 即不一致");
    // 6. 编解码往返全闭集。
    let rt = probe_a();
    let rt_ok = match &rt {
        Ok(n) => *n == 50,
        Err(_) => false,
    };
    s.add_dyn("J1-编解码往返", rt_ok, &format!("{:?}", rt));
    // 7. 越域键四向拒。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-注入10/10", inj_ok, &format!("r={}", JUDGE_ROUNDS));
    // 8. 计数守恒。
    let conserve_ok = match &inj {
        Ok(l) => l.missed_total() == 0,
        Err(_) => false,
    };
    s.add("J1-计数守恒漏检零", conserve_ok, "tries=caught+missed 且 missed=0");
    // 9. api/dim 闭集往返。
    let api_rt = MatrixApi::from_num(1) == Some(MatrixApi::VerifyVersionInfoW)
        && MatrixApi::from_num(6) == Some(MatrixApi::RtlGetVersion)
        && MatrixApi::from_num(0).is_none()
        && MatrixApi::from_num(7).is_none();
    let dim_rt = ReportDim::from_num(1) == Some(ReportDim::MajorVersion)
        && ReportDim::from_num(8) == Some(ReportDim::ProductType)
        && ReportDim::from_num(0).is_none()
        && ReportDim::from_num(9).is_none();
    s.add("J1-api/dim闭集往返", api_rt && dim_rt, "1..6 / 1..8 可还原，0 与越界拒");
    // 10. 探针三序。
    let a = probe_b().is_ok();
    let b = probe_c().is_ok();
    let c = probe_a().is_ok();
    s.add_dyn("J1-探针三序", a && b && c, &format!("a={} b={} c={}", a, b, c));
    // 11. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 12. 表满边界（第 33 键拒且账不丢）。
    let mut full = MatrixTable::new();
    let mut n = 0;
    let mut pid = 1;
    while pid <= 8 && n < MATRIX_CAP {
        let mut api = 1;
        while api <= 2 && n < MATRIX_CAP {
            let mut dim = 1;
            while dim <= 2 && n < MATRIX_CAP {
                let _ = full.insert(&ReportKey { pid, api, dim }, false);
                n += 1;
                dim += 1;
            }
            api += 1;
        }
        pid += 1;
    }
    s.add(
        "J1-表满边界",
        full.insert_total == 32 && full.insert(&ReportKey { pid: 99, api: 6, dim: 8 }, true).is_err(),
        "32 槽占尽第 33 键必拒",
    );
    // 13. 重复键边界。
    let mut dup = MatrixTable::new();
    let k = ReportKey { pid: 1, api: 1, dim: 1 };
    let dup_ok = dup.insert(&k, true).is_ok() && dup.insert(&k, false).is_err();
    s.add("J1-重复键拒绝", dup_ok, "同三元组二次插入必拒");
    // 14. 文案闭集。
    s.add("J3-文案闭集", detail_closure(), "10 码逐条非空且反查一致");
    // 15. 段内窗口逐码核对。
    let win_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < RmkCode::ALL.len() {
            if RmkCode::ALL[i].code() >= SEG_BASE + 0x40 {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("J1-段内窗口核对", win_ok, "10 码全部 < SEG_BASE+0x40");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 判据集全绿。
    #[test]
    fn all_checks_pass() {
        let set = run_unx18417_checks();
        assert!(set.all_passed(), "UNX-F18417 判据集存在红项");
    }

    /// 正常流：插入→查询→冻结→改写拒。
    #[test]
    fn normal_flow_insert_query_freeze() {
        let mut mx = MatrixTable::new();
        let k = ReportKey { pid: 5, api: 3, dim: 2 };
        mx.insert(&k, true).unwrap_or_default();
        assert_eq!(mx.query(&k), Ok(true));
        mx.freeze(&k).unwrap_or_default();
        assert!(mx.try_rewrite_frozen(&k, false).is_err());
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

    /// 判定次序：api 越域先于 dim 越域（字段序裁决）。
    #[test]
    fn order_api_before_dim() {
        match key_pack(&ReportKey { pid: 1, api: 0, dim: 0 }) {
            Err(c) => assert_eq!(c, RmkCode::ApiOutOfRange.code()),
            Ok(_) => panic!("api=0 不应打包成功"),
        }
    }

    /// 编解码边界：pid 极值与保留位。
    #[test]
    fn codec_boundaries() {
        assert!(key_roundtrip(&ReportKey { pid: 0, api: 1, dim: 1 }).is_ok());
        assert!(key_roundtrip(&ReportKey { pid: u32::MAX, api: 6, dim: 8 }).is_ok());
        // 复合值直接核对位布局。
        let v = key_pack(&ReportKey { pid: 1, api: 2, dim: 3 }).unwrap_or(0);
        assert_eq!(v, 1 | (2 << 32) | (3 << 40));
    }

    /// 四类边界：空/满/重复/越界。
    #[test]
    fn boundary_edges() {
        let mut mx = MatrixTable::new();
        let k = ReportKey { pid: 1, api: 1, dim: 1 };
        assert!(mx.query(&k).is_err()); // 空
        let mut i = 0;
        let mut pid = 1;
        while i < MATRIX_CAP {
            let _ = mx.insert(&ReportKey { pid, api: 1, dim: 1 }, false);
            pid += 1;
            i += 1;
        }
        assert!(mx.insert(&ReportKey { pid, api: 1, dim: 1 }, false).is_err()); // 满
        assert!(mx.insert(&ReportKey { pid: 1, api: 1, dim: 1 }, false).is_err()); // 重复
        assert!(key_pack(&ReportKey { pid: 1, api: 9, dim: 1 }).is_err()); // 越界
    }

    /// 冻结单调：重复冻结幂等且计数不涨。
    #[test]
    fn freeze_idempotent() {
        let mut mx = MatrixTable::new();
        let k = ReportKey { pid: 2, api: 2, dim: 2 };
        let _ = mx.insert(&k, false);
        mx.freeze(&k).unwrap_or_default();
        let c1 = mx.frozen_count;
        mx.freeze(&k).unwrap_or_default();
        assert_eq!(mx.frozen_count, c1);
    }

    /// 双跑一致。
    #[test]
    fn reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫 + 文案闭集。
    #[test]
    fn segment_and_detail() {
        assert!(segment_guard());
        assert!(detail_closure());
        assert!(detail_of(SEG_BASE + 64).starts_with("未知"));
    }
}
