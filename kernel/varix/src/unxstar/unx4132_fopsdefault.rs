//! unx4132_fopsdefault — UNX-F4132 fops 空实现裁断（未实现函数返回矩阵）（诊断段 0x9260）。
//!
//! 判据原文：UNX-F4132-J1 未实现函数调用返回矩阵（读/写→ENOSYS、ioctl→ENOTTY、
//!           mmap→ENODEV 等 8 格）全过，零空指针崩——为 B08 裁断供给对照面。
//!   J2 缺省矩阵 8 格行序与函数号一致且不随实现多少而变。
//!   J3 三格无需出错的缺省（poll/fsync/release）不得写成错误。
//!   J4 派发三账 hits 加 defaults 恒等于 calls。
//! 设计纪律：
//!   1. 「未实现」不是「随便返回个错」：每格错误码按 Linux 缺省实现取定，且必须
//!      与调用方语义相称——读写走 ENOSYS(38)（功能性缺失），ioctl 走 ENOTTY(25)
//!      （设备不支持该命令），mmap 走 ENODEV(19)（无映射能力）。
//!   2. 三格是「无需出错」的缺省实现：poll 返回 DEFAULT_POLLMASK(0x5)、release
//!      返回 0、fsync 返回 0——把这三格错写成 ENOSYS 会让应用层误判文件系统坏掉。
//!   3. 零空指针：本域以「能力位 + 缺省裁决表」代替函数指针，任何位缺失都走裁决，
//!      不存在解引用空指针的路径（结构性免疫，而非靠断言兜底）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9260 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9260;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOTREG: u16 = SEG_BASE + 1; // 表未注册即派发
const E_DUPBIND: u16 = SEG_BASE + 2; // 同一位重复绑定（注册冲突）
const E_OPID: u16 = SEG_BASE + 3; // 函数号越界（闭集外）
const E_CELL: u16 = SEG_BASE + 4; // 8 格矩阵有缺格
const E_ACCT: u16 = SEG_BASE + 5; // 派发账与格数不平
const E_BADVERDICT: u16 = SEG_BASE + 6; // 裁决码不在格表闭集内
const E_RESET: u16 = SEG_BASE + 7; // 已派发后回退注册态
const E_EMPTY: u16 = SEG_BASE + 8; // 空表（一位都没绑）

/// ENOSYS 的 errno 原值（POSIX errno.h）：功能未实现。
pub const ENOSYS_ERRNO: u16 = 38;
/// ENOTTY：设备不支持该 ioctl。
pub const ENOTTY_ERRNO: u16 = 25;
/// ENODEV：无此设备/无该能力。
pub const ENODEV_ERRNO: u16 = 19;
/// ESPIPE：不可寻址（no_llseek 的返回值）。
pub const ESPIPE_ERRNO: u16 = 29;
/// DEFAULT_POLLMASK（Linux include/linux/poll.h）：EPOLLIN|EPOLLOUT。
pub const DEFAULT_POLLMASK: u16 = 0x0005;

/// 8 个 f_op 成员的闭集编号（B08 冻结清单的前 8 项）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FopId {
    Read = 0,
    Write = 1,
    Llseek = 2,
    Ioctl = 3,
    Mmap = 4,
    Poll = 5,
    Fsync = 6,
    Release = 7,
}

pub const FOP_COUNT: usize = 8;

impl FopId {
    pub fn from_idx(i: u8) -> Option<FopId> {
        match i {
            0 => Some(FopId::Read),
            1 => Some(FopId::Write),
            2 => Some(FopId::Llseek),
            3 => Some(FopId::Ioctl),
            4 => Some(FopId::Mmap),
            5 => Some(FopId::Poll),
            6 => Some(FopId::Fsync),
            7 => Some(FopId::Release),
            _ => None,
        }
    }

    /// 能力位（每位独立，便于「只实现一部分」的表表达）。
    pub fn bit(self) -> u32 {
        1u32 << (self as u32)
    }

    pub fn name(self) -> &'static str {
        match self {
            FopId::Read => "read",
            FopId::Write => "write",
            FopId::Llseek => "llseek",
            FopId::Ioctl => "ioctl",
            FopId::Mmap => "mmap",
            FopId::Poll => "poll",
            FopId::Fsync => "fsync",
            FopId::Release => "release",
        }
    }

    /// 缺省裁决：返回 (返回码, 是否视为错误)。
    /// 码值来源见头注纪律 1/2，逐格写死不做推导。
    pub fn default_verdict(self) -> (u16, bool) {
        match self {
            FopId::Read => (ENOSYS_ERRNO, true),
            FopId::Write => (ENOSYS_ERRNO, true),
            FopId::Llseek => (ESPIPE_ERRNO, true),
            FopId::Ioctl => (ENOTTY_ERRNO, true),
            FopId::Mmap => (ENODEV_ERRNO, true),
            FopId::Poll => (DEFAULT_POLLMASK, false),
            FopId::Fsync => (0, false),
            FopId::Release => (0, false),
        }
    }
}

/// 注册状态机：Unregistered → Registered → Dispatched；不可回退。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TableState {
    Unregistered,
    Registered,
    Dispatched,
}

/// 被裁决方的能力表：只有位，没有指针。
#[derive(Clone, Copy)]
pub struct FopsTable {
    /// 已实现成员的能力位
    pub present: u32,
    /// 注册状态
    pub state: TableState,
}

impl FopsTable {
    /// 空表：一位未实现（合法但须显式注册）。
    pub fn empty() -> FopsTable {
        FopsTable {
            present: 0,
            state: TableState::Unregistered,
        }
    }

    /// 逐位绑定。重复绑定位即注册冲突。
    pub fn bind(&mut self, id: FopId) -> Result<(), u16> {
        if self.state == TableState::Dispatched {
            return Err(E_RESET);
        }
        if (self.present & id.bit()) != 0 {
            return Err(E_DUPBIND);
        }
        self.present |= id.bit();
        self.state = TableState::Registered;
        Ok(())
    }

    /// 按位图整表绑定（等价于批量 bind，位图外位一律拒）。
    pub fn bind_mask(&mut self, mask: u32) -> Result<(), u16> {
        if (mask & !((1u32 << FOP_COUNT) - 1)) != 0 {
            return Err(E_OPID);
        }
        let mut i = 0u8;
        while (i as usize) < FOP_COUNT {
            if let Some(id) = FopId::from_idx(i) {
                if (mask & id.bit()) != 0 {
                    self.bind(id)?;
                }
            }
            i += 1;
        }
        if self.present == 0 {
            return Err(E_EMPTY);
        }
        Ok(())
    }

    pub fn has(&self, id: FopId) -> bool {
        (self.present & id.bit()) != 0
    }

    /// 注册完成 → Dispatched（此后不可再 bind）。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.state != TableState::Registered {
            return Err(E_NOTREG);
        }
        self.state = TableState::Dispatched;
        Ok(())
    }
}

/// 单格裁决结果。
#[derive(Clone, Copy)]
pub struct Verdict {
    /// 返回码（errno 或 poll 掩码）
    pub code: u16,
    /// 是否走了缺省实现
    pub defaulted: bool,
    /// 是否属错误返回
    pub is_err: bool,
}

/// 派发核：8 格矩阵 + 派发三账。
pub struct FopsDispatch {
    /// 能力表
    pub table: FopsTable,
    /// 总派发次数
    pub calls: u32,
    /// 走缺省实现的次数
    pub defaults: u32,
    /// 命中已实现成员的次数
    pub hits: u32,
    /// 缺省格导出缓存（注册时生成一次）
    pub matrix: Vec<(u8, u16, u8)>,
}

impl FopsDispatch {
    pub fn new(table: FopsTable) -> Result<FopsDispatch, u16> {
        if table.state != TableState::Dispatched {
            return Err(E_NOTREG);
        }
        // 注册时即冻结 8 格缺省矩阵（此后不再变）
        let mut matrix = Vec::new();
        let mut i = 0u8;
        while (i as usize) < FOP_COUNT {
            let id = match FopId::from_idx(i) {
                Some(x) => x,
                None => return Err(E_CELL),
            };
            let (code, is_err) = id.default_verdict();
            matrix.push((i, code, if is_err { 1u8 } else { 0u8 }));
            i += 1;
        }
        if matrix.len() != FOP_COUNT {
            return Err(E_CELL);
        }
        Ok(FopsDispatch {
            table,
            calls: 0,
            defaults: 0,
            hits: 0,
            matrix,
        })
    }

    /// 核心派发：有实现则「成功 0 + 非缺省」，无实现则查 8 格裁决表。
    pub fn call(&mut self, id: FopId) -> Result<Verdict, u16> {
        self.calls += 1;
        if self.table.has(id) {
            self.hits += 1;
            return Ok(Verdict {
                code: 0,
                defaulted: false,
                is_err: false,
            });
        }
        let idx = id as usize;
        let row = match self.matrix.get(idx) {
            Some(&r) => r,
            None => return Err(E_CELL),
        };
        if row.0 as usize != idx {
            return Err(E_CELL); // 行序必须与闭集编号一致
        }
        self.defaults += 1;
        Ok(Verdict {
            code: row.1,
            defaulted: true,
            is_err: row.2 != 0,
        })
    }

    /// 三账守恒：hits + defaults == calls。
    pub fn conserved(&self) -> bool {
        self.hits + self.defaults == self.calls
    }

    /// 8 格矩阵自检：行序 0..8 连续、错误位与码语义一致。
    pub fn matrix_ok(&self) -> bool {
        if self.matrix.len() != FOP_COUNT {
            return false;
        }
        let mut i = 0usize;
        while i < FOP_COUNT {
            let row = self.matrix[i];
            if row.0 as usize != i {
                return false;
            }
            let want = match FopId::from_idx(i as u8) {
                Some(id) => id.default_verdict(),
                None => return false,
            };
            if row.1 != want.0 || (row.2 != 0) != want.1 {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 导出给 B08 裁断的对照面：(编号, 缺省码, 错误位)。
    pub fn export(&self) -> Vec<(u8, u16, u8)> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.matrix.len() {
            out.push(self.matrix[i]);
            i += 1;
        }
        out
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOTREG => "表未注册（未 seal）即派发",
        E_DUPBIND => "同一能力位重复绑定",
        E_OPID => "函数号越界（8 项闭集外）",
        E_CELL => "8 格裁决矩阵缺格或行序错",
        E_ACCT => "派发三账与格数不平",
        E_BADVERDICT => "裁决码不在格表闭集内",
        E_RESET => "已派发后回退注册态",
        E_EMPTY => "空表（一位都未实现）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：8 格逐格裁决 → 部分实现 → 注册冲突 → 三账对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：最小表（仅 release 一项可缺省掉）注册 + seal，8 格缺省矩阵冻结。
    let mut t = FopsTable::empty();
    // 空表 seal 需要先置 Registered：先绑一位再解不了，故用最小表（release）。
    t.bind(FopId::Release)?;
    t.seal()?;
    let mut d = FopsDispatch::new(t)?;
    if !d.matrix_ok() {
        return Err(E_CELL);
    }
    hits += 1;

    // R2：逐格核对 8 个缺省码（含 read/write→ENOSYS、ioctl→ENOTTY、mmap→ENODEV）。
    let mut i = 0u8;
    while (i as usize) < FOP_COUNT {
        let id = match FopId::from_idx(i) {
            Some(x) => x,
            None => return Err(E_OPID),
        };
        let v = d.call(id)?;
        if id == FopId::Release {
            // 唯一已实现成员：成功、非缺省、非错误
            if v.defaulted || v.is_err || v.code != 0 {
                return Err(E_ACCT);
            }
        } else {
            let (code, is_err) = id.default_verdict();
            if !v.defaulted || v.code != code || v.is_err != is_err {
                return Err(E_BADVERDICT);
            }
        }
        i += 1;
    }
    if d.calls != 8 || d.hits != 1 || d.defaults != 7 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：三格「无需出错」的缺省实现不得被写成错误（poll/fsync/release）。
    let v = d.call(FopId::Poll)?;
    if v.is_err || v.code != DEFAULT_POLLMASK {
        return Err(E_BADVERDICT);
    }
    let v = d.call(FopId::Fsync)?;
    if v.is_err || v.code != 0 {
        return Err(E_BADVERDICT);
    }
    if !d.conserved() {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：半实现表（read+write+llseek）——三格命中、五格缺省，矩阵不变。
    let mut t2 = FopsTable::empty();
    t2.bind_mask(FopId::Read.bit() | FopId::Write.bit() | FopId::Llseek.bit())?;
    t2.seal()?;
    let mut d2 = FopsDispatch::new(t2)?;
    if d2.call(FopId::Read)?.code != 0 {
        return Err(E_ACCT);
    }
    if d2.call(FopId::Write)?.defaulted {
        return Err(E_ACCT);
    }
    if d2.call(FopId::Llseek)?.defaulted {
        return Err(E_ACCT);
    }
    // ioctl 未实现：ENOTTY 且缺省
    let vi = d2.call(FopId::Ioctl)?;
    if !vi.defaulted || vi.code != ENOTTY_ERRNO {
        return Err(E_BADVERDICT);
    }
    // mmap 未实现：ENODEV 且缺省
    let vm = d2.call(FopId::Mmap)?;
    if !vm.defaulted || vm.code != ENODEV_ERRNO {
        return Err(E_BADVERDICT);
    }
    if d2.hits != 3 || d2.defaults != 2 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R5：全实现表——8 格全部命中，零缺省（对照面顶端）。
    let mut t3 = FopsTable::empty();
    let mut all = 0u32;
    let mut k = 0u8;
    while (k as usize) < FOP_COUNT {
        if let Some(id) = FopId::from_idx(k) {
            all |= id.bit();
        }
        k += 1;
    }
    t3.bind_mask(all)?;
    t3.seal()?;
    let mut d3 = FopsDispatch::new(t3)?;
    let mut m = 0u8;
    while (m as usize) < FOP_COUNT {
        if let Some(id) = FopId::from_idx(m) {
            let v = d3.call(id)?;
            if v.defaulted || v.code != 0 {
                return Err(E_ACCT);
            }
        }
        m += 1;
    }
    if d3.defaults != 0 || d3.hits != 8 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：未 seal 即派发 → E_NOTREG。
    let mut t4 = FopsTable::empty();
    t4.bind(FopId::Read)?;
    if FopsDispatch::new(t4).is_ok() {
        return Err(E_NOTREG);
    }
    hits += 1;

    // I2：重复绑定位 → E_DUPBIND；seal 后回退 → E_RESET。
    let mut t5 = FopsTable::empty();
    t5.bind(FopId::Read)?;
    if t5.bind(FopId::Read) != Err(E_DUPBIND) {
        return Err(E_DUPBIND);
    }
    t5.seal()?;
    if t5.bind(FopId::Write) != Err(E_RESET) {
        return Err(E_RESET);
    }
    hits += 1;

    // I3：位图含闭集外位 → E_OPID；空位图 → E_EMPTY。
    let mut t6 = FopsTable::empty();
    if t6.bind_mask(1 << (FOP_COUNT as u32)) != Err(E_OPID) {
        return Err(E_OPID);
    }
    if t6.bind_mask(0) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    hits += 1;

    // I4：函数号越界 → 闭集直拒（不给近似实现）。
    if FopId::from_idx(FOP_COUNT as u8).is_some() || FopId::from_idx(200).is_some() {
        return Err(E_OPID);
    }
    hits += 1;

    // C1：三账守恒 + 8 格导出一致。
    if !d.conserved() || !d2.conserved() || !d3.conserved() {
        return Err(E_ACCT);
    }
    let e = d3.export();
    if e.len() != FOP_COUNT {
        return Err(E_CELL);
    }
    // 缺省矩阵不随实现多少而变（B08 对照面的稳定性）
    if d.matrix != d3.matrix {
        return Err(E_CELL);
    }
    hits += 1;

    Ok(hits)
}
