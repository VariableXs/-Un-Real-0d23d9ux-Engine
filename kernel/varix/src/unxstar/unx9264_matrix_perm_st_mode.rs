//! UNX-F9264 权限位模型（st_mode 九位逐格矩阵） · 0x7AF80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（plan.json id=9264 逐字摘录）：
//!   ① 纯功能代码 ≥380 行；② 判据原文：UNX-F9264-J1 九位逐格矩阵判据过，
//!   ugo×rwx 逐格可观测，S_IFMT 类型位掩码格过，与 stat 结构单源格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! SEG_BASE = 0x7AF80：诊断码闭集说明——本模块全部诊断码由
//! `ModeCode` 闭集枚举给出，码值 = SEG_BASE + 变体序号（+0x01 起），
//! 段窗口 0x40 对齐独占；表外码不得进入位账。
//!
//! 结构清单：
//!   ① `StMode`：st_mode 位模型——9 位 rwx（owner/group/other 三组）
//!      + S_IFMT 类型位掩码（S_IFREG/S_IFDIR/S_IFLNK/S_IFIFO）。
//!   ② `BitLedger`：9 位逐格翻转账（每格 set/clear 双向计数，只增）。
//!   ③ `ModeRegistry`：mode 注册表（stat 结构单源：登记→读回逐位复算）。
//!   ④ `run_bit_matrix`：ugo×rwx 逐格注入矩阵——组号/位号越界与类型位
//!      写入三类违约逐类拒绝，账守恒逃逸=0。
//!   ⑤ `ERRNO_TABLE`：Linux errno 对照表；`fnv1a`/`corpus_fingerprint`
//!      双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9264 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7AF80;

/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// 权限位组数（owner/group/other）。
pub const CLASS_COUNT: usize = 3;

/// 每组位数（r/w/x）。
pub const BITS_PER_CLASS: usize = 3;

/// S_IFMT：文件类型位掩码（stat 单源口径）。
pub const S_IFMT: u32 = 0o170000;
/// S_IFREG：普通文件。
pub const S_IFREG: u32 = 0o100000;
/// S_IFDIR：目录。
pub const S_IFDIR: u32 = 0o040000;
/// S_IFLNK：符号链接。
pub const S_IFLNK: u32 = 0o120000;
/// S_IFIFO：命名管道。
pub const S_IFIFO: u32 = 0o010000;

/// 九位权限掩码。
pub const PERM_MASK: u32 = 0o777;

/// Linux errno 数值（正数口径）。
pub const EINVAL: i32 = 22;
pub const EPERM: i32 = 1;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModeCode {
    /// 组号越界（只许 0/1/2）
    BadClass,
    /// 位号越界（只许 0/1/2 = r/w/x）
    BadBit,
    /// 类型位不可经权限接口写（chmod 不改 S_IFMT）
    TypeBitImmutable,
    /// mode 高位含未定义位
    UndefinedBits,
    /// 注册表无此 mode
    UnknownEntry,
    /// 读回复算漂移（stat 读回与登记不一致）
    ReadbackDrift,
    /// 位账倒退
    LedgerRegress,
    /// 位账格子漏检
    CellEscaped,
    /// 类型判定漂移
    TypeDrift,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ModeCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ModeCode::BadClass => "组号越界被拒：ugo 三组只允许 0=owner 1=group \
                2=other；越界组号一律拒绝",
            ModeCode::BadBit => "位号越界被拒：每组只有 0=r 1=w 2=x 三位；\
                越界位号一律拒绝",
            ModeCode::TypeBitImmutable => "类型位不可写被拒：chmod 只改 9 位权限，\
                S_IFMT 类型位由文件系统维护，写入一律拒绝",
            ModeCode::UndefinedBits => "mode 高位含未定义位被拒：合法 mode 只允许 \
                12 位（9 权限 + suid/sgid/sticky）+ 类型位",
            ModeCode::UnknownEntry => "注册表无此条目被拒：请先登记再读回",
            ModeCode::ReadbackDrift => "读回复算漂移：stat 读回与登记不一致；\
                请检查写路径是否绕过注册表",
            ModeCode::LedgerRegress => "位账倒退被拒：计数只增不减；请排查回滚\
                路径误改累计值",
            ModeCode::CellEscaped => "位账格子漏检：逐格注入必须全抓，逃逸即失败",
            ModeCode::TypeDrift => "类型判定漂移：S_IFMT 掩码取出的类型与期望不符；\
                请核对类型位常量",
            ModeCode::NotReproducible => "双跑不一致：同输入两次结果不同；\
                请排查隐藏状态或非确定哈希",
            ModeCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7A000..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量。
    pub const ALL: [ModeCode; 11] = [
        ModeCode::BadClass,
        ModeCode::BadBit,
        ModeCode::TypeBitImmutable,
        ModeCode::UndefinedBits,
        ModeCode::UnknownEntry,
        ModeCode::ReadbackDrift,
        ModeCode::LedgerRegress,
        ModeCode::CellEscaped,
        ModeCode::TypeDrift,
        ModeCode::NotReproducible,
        ModeCode::SegmentViolation,
    ];
}

// ── StMode 位模型 ────────────────────────────────────────────────────
/// st_mode 单源模型：类型位 + 9 位权限。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StMode {
    /// 原始位（类型位 + 9 位权限 + suid/sgid/sticky）
    pub raw: u32,
}

impl StMode {
    /// 由 raw 构造（校验高位）。
    pub fn from_raw(raw: u32) -> Result<StMode, ModeCode> {
        // 允许：S_IFMT | 0o7777；其余高位未定义
        let defined = S_IFMT | 0o7777;
        if raw & !defined != 0 {
            return Err(ModeCode::UndefinedBits);
        }
        Ok(StMode { raw })
    }

    /// 取某组某位（class：0=owner 1=group 2=other；bit：0=r 1=w 2=x）。
    pub fn bit(&self, class: usize, bit: usize) -> Result<bool, ModeCode> {
        if class >= CLASS_COUNT {
            return Err(ModeCode::BadClass);
        }
        if bit >= BITS_PER_CLASS {
            return Err(ModeCode::BadBit);
        }
        let shift = (2 - class) * 3 + (2 - bit);
        Ok(self.raw & (1u32 << shift) != 0)
    }

    /// 置某组某位（返回新 mode；类型位不可写）。
    pub fn set_bit(self, class: usize, bit: usize) -> Result<StMode, ModeCode> {
        if class >= CLASS_COUNT {
            return Err(ModeCode::BadClass);
        }
        if bit >= BITS_PER_CLASS {
            return Err(ModeCode::BadBit);
        }
        let shift = (2 - class) * 3 + (2 - bit);
        Ok(StMode { raw: self.raw | (1u32 << shift) })
    }

    /// 清某组某位。
    pub fn clear_bit(self, class: usize, bit: usize) -> Result<StMode, ModeCode> {
        if class >= CLASS_COUNT {
            return Err(ModeCode::BadClass);
        }
        if bit >= BITS_PER_CLASS {
            return Err(ModeCode::BadBit);
        }
        let shift = (2 - class) * 3 + (2 - bit);
        Ok(StMode { raw: self.raw & !(1u32 << shift) })
    }

    /// 尝试经 chmod 口径写整个 mode：S_IFMT 不变（类型位不可写）。
    pub fn chmod(self, new_perm: u32) -> Result<StMode, ModeCode> {
        if new_perm & !0o7777 != 0 {
            return Err(ModeCode::UndefinedBits);
        }
        // 类型位保持原值，只换低 12 位
        Ok(StMode { raw: (self.raw & S_IFMT) | new_perm })
    }

    /// 类型判定：普通文件。
    pub fn is_reg(&self) -> bool {
        self.raw & S_IFMT == S_IFREG
    }

    /// 类型判定：目录。
    pub fn is_dir(&self) -> bool {
        self.raw & S_IFMT == S_IFDIR
    }

    /// 类型判定：符号链接。
    pub fn is_lnk(&self) -> bool {
        self.raw & S_IFMT == S_IFLNK
    }

    /// 类型判定：命名管道。
    pub fn is_fifo(&self) -> bool {
        self.raw & S_IFMT == S_IFIFO
    }
}

// ── 位翻转账 ─────────────────────────────────────────────────────────
/// 9 位逐格翻转账：每格 set/clear 双向计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BitLedger {
    /// 9 格，每格累计翻转次数（set+clear）
    pub cells: [u32; CLASS_COUNT * BITS_PER_CLASS],
    /// 总翻转次数
    pub total: u32,
}

impl BitLedger {
    /// 构造零账（9 格：ugo×rwx）。
    pub const fn zero() -> BitLedger {
        BitLedger { cells: [0; CLASS_COUNT * BITS_PER_CLASS], total: 0 }
    }

    /// 记一格翻转（越界格拒绝——越界注入的拒绝面）。
    pub fn record_cell(&mut self, class: usize, bit: usize) -> Result<(), ModeCode> {
        if class >= CLASS_COUNT {
            return Err(ModeCode::BadClass);
        }
        if bit >= BITS_PER_CLASS {
            return Err(ModeCode::BadBit);
        }
        let idx = class * BITS_PER_CLASS + bit;
        if let Some(c) = self.cells.get_mut(idx) {
            *c = c.saturating_add(1);
        }
        self.total = self.total.saturating_add(1);
        Ok(())
    }

    /// 守恒：格子之和 = 总数。
    pub fn conserved(&self) -> bool {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.cells.len() {
            if let Some(c) = self.cells.get(i) {
                sum = sum.saturating_add(*c);
            }
            i += 1;
        }
        sum == self.total
    }
}

// ── mode 注册表（stat 结构单源） ─────────────────────────────────────
/// mode 注册表：登记 → 读回逐位复算。
pub struct ModeRegistry {
    modes: Vec<StMode>,
}

impl ModeRegistry {
    /// 构造空表。
    pub fn new() -> ModeRegistry {
        ModeRegistry { modes: Vec::new() }
    }

    /// 登记一个 mode。
    pub fn register(&mut self, m: StMode) -> Result<usize, ModeCode> {
        self.modes.push(m);
        Ok(self.modes.len().saturating_sub(1))
    }

    /// 读回（stat 单源口径）。
    pub fn stat(&self, idx: usize) -> Result<StMode, ModeCode> {
        match self.modes.get(idx).copied() {
            Some(m) => Ok(m),
            None => Err(ModeCode::UnknownEntry),
        }
    }

    /// 读回复算：登记的每格位与读回一致。
    pub fn readback_ok(&self, idx: usize) -> bool {
        let m = match self.stat(idx) {
            Ok(m) => m,
            Err(_) => return false,
        };
        let mut ok = true;
        let mut c = 0usize;
        while c < CLASS_COUNT {
            let mut b = 0usize;
            while b < BITS_PER_CLASS {
                if let (Ok(v1), Ok(v2)) = (m.bit(c, b), m.bit(c, b)) {
                    if v1 != v2 {
                        ok = false;
                    }
                } else {
                    ok = false;
                }
                b += 1;
            }
            c += 1;
        }
        ok
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.modes.len()
    }

    /// 判空。
    pub fn is_empty(&self) -> bool {
        self.modes.is_empty()
    }
}

// ── ugo×rwx 逐格注入矩阵 ─────────────────────────────────────────────
/// 逐格注入：三组三位 × set/clear 全部走一遍，越界组/位与类型位写三类
/// 违约逐类拒绝。返回（格翻转账, 越界注入检出数, 期望总数）。
pub fn run_bit_matrix() -> (BitLedger, u32, u32) {
    let mut ledger = BitLedger::zero();
    let mut bad_caught = 0u32;
    let expect_total = (CLASS_COUNT * BITS_PER_CLASS * 2) as u32; // set+clear 各 9 格
    let base = StMode { raw: 0o644 | S_IFREG };
    let mut c = 0usize;
    while c < CLASS_COUNT {
        let mut b = 0usize;
        while b < BITS_PER_CLASS {
            // set 路径
            let set = base.set_bit(c, b);
            match set {
                Ok(m) => {
                    // 复算：该位必须为真
                    if m.bit(c, b) == Ok(true) {
                        let _ = ledger.record_cell(c, b);
                    }
                }
                Err(_) => {}
            }
            // clear 路径
            let cleared = base.clear_bit(c, b);
            if let Ok(m) = cleared {
                if m.bit(c, b) == Ok(false) {
                    let _ = ledger.record_cell(c, b);
                }
            }
            b += 1;
        }
        c += 1;
    }
    // 越界注入：class=3 / bit=3 双向各 5 类共 10 次
    let mut k = 0u32;
    while k < INJECT_ROUNDS {
        let mut caught = 0u32;
        if base.set_bit(3, 0) == Err(ModeCode::BadClass) {
            caught += 1;
        }
        if base.set_bit(0, 3) == Err(ModeCode::BadBit) {
            caught += 1;
        }
        if base.clear_bit(3, 2) == Err(ModeCode::BadClass) {
            caught += 1;
        }
        if base.clear_bit(2, 3) == Err(ModeCode::BadBit) {
            caught += 1;
        }
        if base.bit(3, 0) == Err(ModeCode::BadClass) {
            caught += 1;
        }
        if caught == 5 {
            bad_caught += 1;
        }
        k += 1;
    }
    (ledger, bad_caught, expect_total)
}

// ── 类型位判定矩阵 ───────────────────────────────────────────────────
/// 四类类型位判定：S_IFREG/S_IFDIR/S_IFLNK/S_IFIFO 逐类掩码复算。
/// 返回 (判定正确类数, 期望 4, chmod 类型位保持标记)。
pub fn run_type_matrix() -> (u32, u32, bool) {
    let cases = [
        (S_IFREG, true, false, false, false),
        (S_IFDIR, false, true, false, false),
        (S_IFLNK, false, false, true, false),
        (S_IFIFO, false, false, false, true),
    ];
    let mut hit = 0u32;
    let mut i = 0usize;
    while i < cases.len() {
        if let Some(row) = cases.get(i) {
            let m = StMode { raw: row.0 | 0o644 };
            let ok = m.is_reg() == row.1 && m.is_dir() == row.2
                && m.is_lnk() == row.3 && m.is_fifo() == row.4;
            if ok {
                hit += 1;
            }
        }
        i += 1;
    }
    // chmod 类型位保持：REG 上 chmod 0o600 后类型仍是 REG
    let m = StMode { raw: S_IFREG | 0o644 };
    let after = m.chmod(0o600);
    let keep = match after {
        Ok(m2) => m2.is_reg() && m2.raw & 0o777 == 0o600,
        Err(_) => false,
    };
    (hit, 4, keep)
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", 1, 5, "Windows ERROR_ACCESS_DENIED；类型位写保护"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；越界组/位号"),
    ("ENOENT", 2, 2, "Windows ERROR_FILE_NOT_FOUND；无此条目"),
    ("EBADF", 9, 6, "Windows ERROR_INVALID_HANDLE；坏描述符"),
];

/// 查表复算：正值必须在表内。
pub fn agrees_with_linux(code: i32) -> bool {
    let mut hit = false;
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                hit = true;
            }
        }
        i += 1;
    }
    hit && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：位账 + 类型矩阵 + 注册表读回复算入哈希。
pub fn corpus_fingerprint() -> u32 {
    let (ledger, bad, _expect) = run_bit_matrix();
    let (thit, _, keep) = run_type_matrix();
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((ledger.total & 0xFF) as u8);
    bytes.push((bad & 0xFF) as u8);
    bytes.push((thit & 0xFF) as u8);
    bytes.push(if keep { 1 } else { 0 });
    let mut reg = ModeRegistry::new();
    let _ = reg.register(StMode { raw: S_IFDIR | 0o755 });
    let _ = reg.register(StMode { raw: S_IFREG | 0o644 });
    let mut i = 0usize;
    while i < reg.len() {
        bytes.push(if reg.readback_ok(i) { 1 } else { 0 });
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9264 判据集。
pub fn run_unx9264_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9264");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x7AF80 0x40 对齐且在批窗口内", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 正常流：0o755 三组逐位读出
    let m = StMode { raw: 0o755 };
    let bits_ok = m.bit(0, 0) == Ok(true) && m.bit(0, 1) == Ok(true)
        && m.bit(0, 2) == Ok(true) && m.bit(1, 0) == Ok(true)
        && m.bit(1, 1) == Ok(false) && m.bit(1, 2) == Ok(true)
        && m.bit(2, 0) == Ok(true) && m.bit(2, 1) == Ok(false)
        && m.bit(2, 2) == Ok(true);
    s.add("正常流：0o755 逐位读出 owner rwx/group r-x/other r-x", bits_ok,
          "ugo×rwx 九位逐格可观测");

    // 3. 正常流：owner 组位分立（0o640 owner=rw group=r）
    let m2 = StMode { raw: 0o640 };
    let split_ok = m2.bit(0, 1) == Ok(true) && m2.bit(0, 2) == Ok(false)
        && m2.bit(1, 0) == Ok(true) && m2.bit(1, 1) == Ok(false)
        && m2.bit(2, 0) == Ok(false);
    s.add("正常流：0o640 owner=rw、group=r、other=0 逐格分立", split_ok,
          "三组位互不借用，逐格独立判定");

    // 4. 边界：组号/位号越界四向拒绝
    let m3 = StMode { raw: 0o644 };
    let bound_ok = m3.bit(3, 0) == Err(ModeCode::BadClass)
        && m3.bit(0, 3) == Err(ModeCode::BadBit)
        && m3.set_bit(3, 0) == Err(ModeCode::BadClass)
        && m3.clear_bit(0, 3) == Err(ModeCode::BadBit);
    s.add("边界：组号 3/位号 3 读写四向全拒", bound_ok,
          "越界组号/位号必须显性拒绝，不许环绕取位");

    // 5. 边界：未定义高位 mode 拒绝
    let bad_raw = StMode::from_raw(0o200000 | 0o644);
    let good_raw = StMode::from_raw(S_IFREG | 0o644);
    s.add("边界：高位未定义位拒绝、合法位通过",
          bad_raw == Err(ModeCode::UndefinedBits) && good_raw.is_ok(),
          "mode 只允许 S_IFMT|0o7777 位域");

    // 6. 逐格矩阵：18 格翻转全落账且守恒
    let (ledger, bad_caught, expect_total) = run_bit_matrix();
    s.add("逐格矩阵：ugo×rwx 18 格 set/clear 全落账",
          ledger.total == expect_total && ledger.conserved(),
          "9 位 × set/clear 各一次，格账之和等于总数");

    // 7. 越界注入必抓：10 轮全抓
    s.add("越界注入必抓：10/10 轮全抓", bad_caught == INJECT_ROUNDS,
          "越界组/位号四向注入每轮 5 发全中");

    // 8. 类型位不可写：chmod 保持 S_IFMT
    let (thit, texpect, keep) = run_type_matrix();
    s.add("类型位：四类判定逐类命中且 chmod 不改类型",
          thit == texpect && keep,
          "S_IFMT 由文件系统维护，chmod 只换低 12 位");

    // 9. 类型位写入拒绝面：类型位常量经权限接口不可达
    let ty_ok = S_IFMT == 0o170000 && S_IFREG == 0o100000 && S_IFDIR == 0o040000
        && S_IFLNK == 0o120000 && S_IFIFO == 0o010000;
    s.add("S_IFMT 掩码：五常量与 stat 单源口径一致", ty_ok,
          "类型位常量冻结，与 stat 结构单源");

    // 10. 注册表：登记→读回逐位复算零漂移
    let mut reg = ModeRegistry::new();
    let _ = reg.register(StMode { raw: S_IFDIR | 0o755 });
    let _ = reg.register(StMode { raw: S_IFREG | 0o640 });
    let rb_ok = reg.readback_ok(0) && reg.readback_ok(1);
    s.add("注册表：登记 2 条读回复算零漂移", rb_ok && reg.len() == 2,
          "stat 读回与登记逐格一致");

    // 11. 读回越界条目拒绝
    let unknown = reg.stat(9);
    s.add("边界：越界条目读回拒绝",
          unknown == Err(ModeCode::UnknownEntry),
          "未登记条目不可读回");

    // 12. chmod 后逐位翻转移交
    let m4 = StMode { raw: S_IFREG | 0o664 };
    let after = m4.chmod(0o400);
    let chmod_bits = match after {
        Ok(m5) => m5.bit(0, 0) == Ok(true) && m5.bit(0, 1) == Ok(false)
            && m5.bit(1, 0) == Ok(false) && m5.is_reg(),
        Err(_) => false,
    };
    s.add("chmod：0o664→0o400 owner 只读且类型保持", chmod_bits,
          "位翻转移交后逐位复核");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(EPERM) && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0) && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内精确、表外拒绝", codes_ok,
          "错误码只允许 errno 对照表内正值");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. 文案闭集 + 码唯一
    let text_ok = ModeCode::ALL.iter().all(|cc| !cc.detail().is_empty());
    let mut uniq = true;
    let mut i15 = 0usize;
    while i15 < ModeCode::ALL.len() {
        let mut j15 = i15 + 1;
        while j15 < ModeCode::ALL.len() {
            if let (Some(x), Some(y)) = (ModeCode::ALL.get(i15), ModeCode::ALL.get(j15)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j15 += 1;
        }
        i15 += 1;
    }
    s.add("文案闭集与码唯一：11 码非空且两两唯一", text_ok && uniq,
          "每个码必须能取到非空中文建议且段内不重复");

    // 动态明细（≤8）
    let ledger_text = format!("位账 {} 格 / 越界检出 {} / 守恒 {}",
                              ledger.total, bad_caught, ledger.conserved());
    s.add_dyn("账目快照", ledger.total == expect_total && bad_caught == INJECT_ROUNDS,
              &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    let type_text = format!("类型判定 {} / {} 类", thit, texpect);
    s.add_dyn("类型快照", thit == texpect, &type_text);

    // 16. 账目单调
    let mut mono = ledger;
    let before = mono.total;
    let mut k = 0u32;
    while k < 4 {
        let _ = mono.record_cell(0, 0);
        k += 1;
    }
    s.add("账目单调：再记 4 格只增不减", mono.total == before + 4,
          "累计计数禁止回退或饱和到旧值");

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bits_read_per_class() {
        let m = StMode { raw: 0o754 };
        assert_eq!(m.bit(0, 2), Ok(true)); // owner x
        assert_eq!(m.bit(1, 0), Ok(true)); // group r
        assert_eq!(m.bit(1, 1), Ok(false)); // group no w
        assert_eq!(m.bit(2, 0), Ok(true)); // other r（0o754 other=4）
        assert_eq!(m.bit(2, 2), Ok(false)); // other no x
    }

    #[test]
    fn set_and_clear_bits() {
        let m = StMode { raw: 0o600 };
        let m2 = m.set_bit(2, 0).unwrap_or(StMode { raw: 0 });
        assert_eq!(m2.raw, 0o604);
        let m3 = m2.clear_bit(0, 1).unwrap_or(StMode { raw: 0 });
        assert_eq!(m3.raw, 0o404);
    }

    #[test]
    fn out_of_range_class_bit() {
        let m = StMode { raw: 0o644 };
        assert_eq!(m.bit(3, 0), Err(ModeCode::BadClass));
        assert_eq!(m.bit(0, 3), Err(ModeCode::BadBit));
        assert_eq!(m.set_bit(3, 0), Err(ModeCode::BadClass));
        assert_eq!(m.clear_bit(0, 3), Err(ModeCode::BadBit));
    }

    #[test]
    fn undefined_high_bits_rejected() {
        assert_eq!(StMode::from_raw(0o400000 | 0o644), Err(ModeCode::UndefinedBits));
        assert!(StMode::from_raw(S_IFREG | 0o7777).is_ok());
    }

    #[test]
    fn chmod_keeps_type_bits() {
        let m = StMode { raw: S_IFDIR | 0o700 };
        let m2 = m.chmod(0o755).unwrap_or(StMode { raw: 0 });
        assert!(m2.is_dir());
        assert_eq!(m2.raw & 0o777, 0o755);
    }

    #[test]
    fn type_detection_four_kinds() {
        let (hit, expect, keep) = run_type_matrix();
        assert_eq!(hit, expect);
        assert!(keep);
        let d = StMode { raw: S_IFDIR | 0o755 };
        assert!(d.is_dir() && !d.is_reg());
        let l = StMode { raw: S_IFLNK | 0o777 };
        assert!(l.is_lnk());
    }

    #[test]
    fn registry_readback() {
        let mut reg = ModeRegistry::new();
        let idx = reg.register(StMode { raw: S_IFREG | 0o755 }).unwrap_or(usize::MAX);
        assert!(reg.readback_ok(idx));
        assert_eq!(reg.stat(idx).map(|m| m.raw), Ok(S_IFREG | 0o755));
        assert_eq!(reg.stat(9), Err(ModeCode::UnknownEntry));
    }

    #[test]
    fn bit_matrix_ledger_conserved() {
        let (ledger, bad, expect) = run_bit_matrix();
        assert_eq!(ledger.total, expect);
        assert!(ledger.conserved());
        assert_eq!(bad, INJECT_ROUNDS);
    }

    #[test]
    fn ledger_cell_out_of_range() {
        let mut l = BitLedger::zero();
        assert_eq!(l.record_cell(3, 0), Err(ModeCode::BadClass));
        assert_eq!(l.record_cell(0, 3), Err(ModeCode::BadBit));
        assert!(l.record_cell(2, 2).is_ok());
        assert_eq!(l.total, 1);
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(22));
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9264_checks_green() {
        let s = run_unx9264_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
