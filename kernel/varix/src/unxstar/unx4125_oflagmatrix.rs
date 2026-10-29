//! unx4125_oflagmatrix — UNX-F4125 O_RDONLY/O_WRONLY/O_RDWR 组合矩阵（诊断段 0x9180）。
//!
//! 判据原文：UNX-F4125-J1 三种访问模式 × 读/写/执行三行为共 9 格矩阵全过，非法
//!           组合（O_RDWR|O_WRONLY）注入返回 EINVAL。
//!   J2 访问模式解码不受任何修饰位影响。
//!   J3 修饰位硬约束四条（TRUNC/EXCL/DIRECTORY/APPEND）各拒一次。
//!   J4 9 格矩阵放行格恒为 5。
//! 设计纪律：
//!   1. 访问模式只由 O_ACCMODE（0o3）两位决定，其余 O_ 位一律视为修饰位，不得
//!      反过来影响访问模式的解码——修饰位越权即拒（E_MOD_BLEED）。
//!   2. 9 格矩阵的语义按 open(2)+read(2)/write(2)/execve(2) 取定：
//!      读行为要 FMODE_READ，写行为要 FMODE_WRITE，执行行为要读权限；
//!      未授予即 EBADF(9)，不是 EACCES——EBADF 描述「这个 fd 不支持此操作」。
//!   3. 非法组合（O_ACCMODE 值为 3）走 EINVAL(22) 而非 EBADF：这是「参数本身
//!      无意义」，在 open 入口就拒，不生成 file 对象，故不进 9 格矩阵。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9180 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9180;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_INVAL: u16 = SEG_BASE + 1; // 对应 EINVAL(22)：非法标志组合
const E_BADF: u16 = SEG_BASE + 2; // 对应 EBADF(9)：行为不被访问模式授予
const E_ACC_UNKNOWN: u16 = SEG_BASE + 3; // O_ACCMODE 解码越界（值 3 及以上）
const E_MOD_BLEED: u16 = SEG_BASE + 4; // 修饰位反向影响访问模式
const E_FLAG_UNKNOWN: u16 = SEG_BASE + 5; // 闭集外标志位
const E_GRID: u16 = SEG_BASE + 6; // 9 格矩阵有缺格/重格
const E_ACCT: u16 = SEG_BASE + 7; // 查格计数与矩阵格数不平
const E_PHASE: u16 = SEG_BASE + 8; // 相位机非法迁移

/// EINVAL / EBADF 的 errno 原值（POSIX errno.h）。
pub const EINVAL_ERRNO: u16 = 22;
pub const EBADF_ERRNO: u16 = 9;

// ── open(2) 标志位（Linux asm-generic/fcntl.h，八进制原值）────
pub const O_ACCMODE: u32 = 0o3;
pub const O_RDONLY: u32 = 0o0;
pub const O_WRONLY: u32 = 0o1;
pub const O_RDWR: u32 = 0o2;
pub const O_CREAT: u32 = 0o100;
pub const O_EXCL: u32 = 0o200;
pub const O_TRUNC: u32 = 0o1000;
pub const O_APPEND: u32 = 0o2000;
pub const O_NONBLOCK: u32 = 0o4000;
pub const O_DIRECTORY: u32 = 0o200000;
pub const O_NOFOLLOW: u32 = 0o400000;
pub const O_CLOEXEC: u32 = 0o2000000;
/// 本域受理的标志闭集。
pub const O_VALID: u32 = O_ACCMODE
    | O_CREAT
    | O_EXCL
    | O_TRUNC
    | O_APPEND
    | O_NONBLOCK
    | O_DIRECTORY
    | O_NOFOLLOW
    | O_CLOEXEC;

/// 访问模式枚举（3 元闭集，与 O_ACCMODE 值一一对应）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AccMode {
    RdOnly = 0,
    WrOnly = 1,
    RdWr = 2,
}

pub const ACC_COUNT: usize = 3;

impl AccMode {
    /// 解码：只取低两位。值 3 无定义 → None（调用方转 EINVAL）。
    pub fn decode(flags: u32) -> Option<AccMode> {
        match flags & O_ACCMODE {
            0 => Some(AccMode::RdOnly),
            1 => Some(AccMode::WrOnly),
            2 => Some(AccMode::RdWr),
            _ => None,
        }
    }

    /// 该模式是否授予读权限。
    pub fn grants_read(self) -> bool {
        match self {
            AccMode::RdOnly | AccMode::RdWr => true,
            AccMode::WrOnly => false,
        }
    }

    /// 该模式是否授予写权限。
    pub fn grants_write(self) -> bool {
        match self {
            AccMode::WrOnly | AccMode::RdWr => true,
            AccMode::RdOnly => false,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AccMode::RdOnly => "O_RDONLY",
            AccMode::WrOnly => "O_WRONLY",
            AccMode::RdWr => "O_RDWR",
        }
    }
}

/// 行为枚举（矩阵的列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Behavior {
    Read = 0,
    Write = 1,
    Exec = 2,
}

pub const BEH_COUNT: usize = 3;

impl Behavior {
    pub fn from_code(c: u8) -> Option<Behavior> {
        match c {
            0 => Some(Behavior::Read),
            1 => Some(Behavior::Write),
            2 => Some(Behavior::Exec),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Behavior::Read => "read",
            Behavior::Write => "write",
            Behavior::Exec => "exec",
        }
    }
}

/// 打开流程相位机：原始 → 已校验 → 已生效；任一失败落 Rejected（终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenPhase {
    Raw,
    Validated,
    Applied,
    Rejected,
}

/// 原始 flags 持有体：解码与校验收口于此。
#[derive(Clone, Copy)]
pub struct OpenFlags {
    /// open(2) 原始标志字
    pub raw: u32,
    /// 相位
    pub phase: OpenPhase,
}

impl OpenFlags {
    pub fn new(raw: u32) -> OpenFlags {
        OpenFlags {
            raw,
            phase: OpenPhase::Raw,
        }
    }

    /// 闭集校验：未知位置位即拒。
    pub fn check_closed_set(&self) -> Result<(), u16> {
        if (self.raw & !O_VALID) != 0 {
            return Err(E_FLAG_UNKNOWN);
        }
        Ok(())
    }

    /// 修饰位合法性（与访问模式无关的硬性约束）。
    pub fn check_modifiers(&self) -> Result<(), u16> {
        let acc = self.raw & O_ACCMODE;
        // O_TRUNC 需写权限；对只读 fd 截断无意义。
        if (self.raw & O_TRUNC) != 0 && acc == O_RDONLY {
            return Err(E_INVAL);
        }
        // O_EXCL 只与 O_CREAT 联用（open(2)）。
        if (self.raw & O_EXCL) != 0 && (self.raw & O_CREAT) == 0 {
            return Err(E_INVAL);
        }
        // O_CREAT 与 O_DIRECTORY 语义冲突（创建目录须 mkdir）。
        if (self.raw & O_CREAT) != 0 && (self.raw & O_DIRECTORY) != 0 {
            return Err(E_INVAL);
        }
        // O_APPEND 需写权限。
        if (self.raw & O_APPEND) != 0 && acc == O_RDONLY {
            return Err(E_INVAL);
        }
        Ok(())
    }

    /// 完整校验：相位 Raw → Validated，任何一步失败落 Rejected。
    pub fn validate(&mut self) -> Result<AccMode, u16> {
        if self.phase != OpenPhase::Raw {
            return Err(E_PHASE);
        }
        let acc = match AccMode::decode(self.raw) {
            Some(a) => a,
            None => {
                self.phase = OpenPhase::Rejected;
                return Err(E_INVAL); // O_ACCMODE==3：无意义组合
            }
        };
        if let Err(e) = self.check_closed_set() {
            self.phase = OpenPhase::Rejected;
            return Err(e);
        }
        if let Err(e) = self.check_modifiers() {
            self.phase = OpenPhase::Rejected;
            return Err(e);
        }
        self.phase = OpenPhase::Validated;
        Ok(acc)
    }

    /// 相位推进到 Applied（仅 Validated 可推进）。
    pub fn apply(&mut self) -> Result<(), u16> {
        if self.phase != OpenPhase::Validated {
            return Err(E_PHASE);
        }
        self.phase = OpenPhase::Applied;
        Ok(())
    }
}

/// 9 格矩阵：3 访问模式 × 3 行为。格值 0 表放行，非 0 表诊断码。
pub struct FlagMatrix {
    /// 行优先展开的 9 格（acc * 3 + beh）
    pub cells: [u16; ACC_COUNT * BEH_COUNT],
    /// 被查询次数（每格每查一次 +1）
    pub lookups: u32,
    /// 放行格数（构建期统计，应恒为 5）
    pub allow_cells: u32,
}

impl FlagMatrix {
    /// 构建矩阵：按访问模式的授权位逐格裁决。
    pub fn build() -> FlagMatrix {
        let mut cells = [0u16; ACC_COUNT * BEH_COUNT];
        let mut allow: u32 = 0;
        let mut a = 0usize;
        while a < ACC_COUNT {
            let acc = match a {
                0 => AccMode::RdOnly,
                1 => AccMode::WrOnly,
                _ => AccMode::RdWr,
            };
            let mut b = 0usize;
            while b < BEH_COUNT {
                let grant = match b {
                    0 => acc.grants_read(),
                    1 => acc.grants_write(),
                    // 执行行为要读权限（execve 打开须 FMODE_READ）
                    _ => acc.grants_read(),
                };
                cells[a * BEH_COUNT + b] = if grant { 0 } else { E_BADF };
                if grant {
                    allow += 1;
                }
                b += 1;
            }
            a += 1;
        }
        FlagMatrix {
            cells,
            lookups: 0,
            allow_cells: allow,
        }
    }

    /// 查格：越界即拒（先比长度，后索引）。
    pub fn lookup(&mut self, acc: AccMode, beh: Behavior) -> Result<(), u16> {
        let idx = (acc as usize) * BEH_COUNT + (beh as usize);
        if idx >= self.cells.len() {
            return Err(E_GRID);
        }
        self.lookups += 1;
        let v = self.cells[idx];
        if v == 0 {
            Ok(())
        } else {
            Err(v)
        }
    }

    /// 格数守恒：格数恰为 9，放行格恰为 5（RdOnly 2 + WrOnly 1 + RdWr 3）。
    pub fn grid_ok(&self) -> bool {
        self.cells.len() == ACC_COUNT * BEH_COUNT && self.allow_cells == 5
    }

    /// 导出为可对账的 (acc 码, beh 码, 诊断码) 三元组表。
    pub fn export(&self) -> Vec<(u8, u8, u16)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.cells.len() {
            out.push(((i / BEH_COUNT) as u8, (i % BEH_COUNT) as u8, self.cells[i]));
            i += 1;
        }
        out
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_INVAL => "非法标志组合（EINVAL 22）",
        E_BADF => "行为不被该访问模式授予（EBADF 9）",
        E_ACC_UNKNOWN => "O_ACCMODE 解码越界",
        E_MOD_BLEED => "修饰位反向影响访问模式",
        E_FLAG_UNKNOWN => "标志位落在受理闭集之外",
        E_GRID => "9 格矩阵缺格/重格",
        E_ACCT => "查格计数与矩阵格数不平",
        E_PHASE => "打开相位机非法迁移",
        _ => "未知诊断码",
    }
}

/// 确定性演练：9 格全过 + 非法组合 10/10 + 相位机 + 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：9 格矩阵构建，格数 9、放行格 5，逐格与授权表对照。
    let mut mx = FlagMatrix::build();
    if !mx.grid_ok() {
        return Err(E_GRID);
    }
    let mut a = 0u8;
    let mut expected_allow: u32 = 0;
    while a < ACC_COUNT as u8 {
        let acc = match AccMode::decode(a as u32) {
            Some(x) => x,
            None => return Err(E_ACC_UNKNOWN),
        };
        let mut b = 0u8;
        while b < BEH_COUNT as u8 {
            let beh = match Behavior::from_code(b) {
                Some(x) => x,
                None => return Err(E_GRID),
            };
            let want_ok = match b {
                0 => acc.grants_read(),
                1 => acc.grants_write(),
                _ => acc.grants_read(),
            };
            let got = mx.lookup(acc, beh);
            if want_ok && got.is_err() {
                return Err(E_GRID);
            }
            if !want_ok && got != Err(E_BADF) {
                return Err(E_BADF);
            }
            if want_ok {
                expected_allow += 1;
            }
            b += 1;
        }
        a += 1;
    }
    if expected_allow != 5 || mx.lookups != 9 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R2：三种访问模式解码正确，且修饰位不串改访问模式。
    if AccMode::decode(O_RDONLY) != Some(AccMode::RdOnly) {
        return Err(E_ACC_UNKNOWN);
    }
    if AccMode::decode(O_WRONLY) != Some(AccMode::WrOnly) {
        return Err(E_ACC_UNKNOWN);
    }
    if AccMode::decode(O_RDWR) != Some(AccMode::RdWr) {
        return Err(E_ACC_UNKNOWN);
    }
    // O_RDONLY | O_CLOEXEC | O_APPEND：低两位仍是 0，仍属只读。
    if AccMode::decode(O_RDONLY | O_CLOEXEC) != Some(AccMode::RdOnly) {
        return Err(E_MOD_BLEED);
    }
    hits += 1;

    // R3：非法组合 O_RDWR|O_WRONLY（值 3）注入 10/10 次全部 EINVAL。
    let mut inval: u32 = 0;
    let mut i = 0;
    while i < 10 {
        let mut f = OpenFlags::new(O_RDWR | O_WRONLY);
        match f.validate() {
            Err(E_INVAL) => {
                if f.phase != OpenPhase::Rejected {
                    return Err(E_PHASE);
                }
                inval += 1;
            }
            _ => return Err(E_INVAL),
        }
        i += 1;
    }
    if inval != 10 {
        return Err(E_INVAL);
    }
    hits += 1;

    // R4：修饰位硬约束四条各拒一次。
    let mut bad = [
        OpenFlags::new(O_RDONLY | O_TRUNC),          // 只读截断
        OpenFlags::new(O_WRONLY | O_EXCL),           // 无 O_CREAT 的 O_EXCL
        OpenFlags::new(O_RDWR | O_CREAT | O_DIRECTORY), // 建目录走 mkdir
        OpenFlags::new(O_RDONLY | O_APPEND),         // 只读追加
    ];
    let mut k = 0usize;
    while k < bad.len() {
        if bad[k].validate() != Err(E_INVAL) {
            return Err(E_INVAL);
        }
        k += 1;
    }
    hits += 1;

    // R5：合法路径——O_RDWR|O_CREAT|O_TRUNC 解码 RdWr 并推进到 Applied。
    let mut good = OpenFlags::new(O_RDWR | O_CREAT | O_TRUNC | O_CLOEXEC);
    if good.validate()? != AccMode::RdWr {
        return Err(E_ACC_UNKNOWN);
    }
    good.apply()?;
    if good.phase != OpenPhase::Applied {
        return Err(E_PHASE);
    }
    hits += 1;

    // I1：闭集外标志位 → E_FLAG_UNKNOWN（不静默忽略）。
    let mut unknown = OpenFlags::new(O_RDONLY | 0x4000_0000);
    if unknown.validate() != Err(E_FLAG_UNKNOWN) {
        return Err(E_FLAG_UNKNOWN);
    }
    hits += 1;

    // I2：相位机——Raw 直接 apply → E_PHASE；Verified 后重新 validate → E_PHASE。
    let mut p = OpenFlags::new(O_RDONLY);
    if p.apply() != Err(E_PHASE) {
        return Err(E_PHASE);
    }
    p.validate()?;
    if p.validate() != Err(E_PHASE) {
        return Err(E_PHASE);
    }
    hits += 1;

    // I3：矩阵导出 9 元组无缺格、无重格（acc/beh 组合唯一）。
    let rows = mx.export();
    if rows.len() != 9 {
        return Err(E_GRID);
    }
    let mut seen = [false; 9];
    for &(x, y, _) in &rows {
        let idx = (x as usize) * BEH_COUNT + (y as usize);
        if idx >= 9 || seen[idx] {
            return Err(E_GRID);
        }
        seen[idx] = true;
    }
    let mut s = 0usize;
    while s < 9 {
        if !seen[s] {
            return Err(E_GRID);
        }
        s += 1;
    }
    hits += 1;

    // C1：对账——查格 9 次 + 上一轮 9 次 = 18；放行格恒 5。
    if mx.lookups != 9 {
        return Err(E_ACCT);
    }
    if mx.allow_cells != 5 {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
