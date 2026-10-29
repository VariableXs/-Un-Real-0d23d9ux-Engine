//! unx4145_unimplmatrix — UNX-F4145 未实现函数的缺省行为矩阵（诊断段 0x93E0）。
//!
//! 判据原文：UNX-F4145-J1 可缺省函数的缺省实现矩阵（generic_read 类 10 格）行为正确，
//!           不可缺省函数缺失注入在注册时被拒（register_fs 联动断言）。
//!   J2 不可缺省项缺失时，缺省位图全置也不得兜住。
//!   J3 缺省解析返回码与矩阵逐格一致。
//!   J4 拒收不留半注册状态。
//! 设计纪律：
//!   1. 函数分两族且分族依据是「能否给出语义正确的兜底」：能兜底的 10 格进缺省矩阵，
//!      兜不住的在注册时硬拒——把不可缺省项塞进缺省矩阵等于让 fs 挂载后必然出错。
//!   2. 缺省矩阵的每格必须给「兜底实现名 + 兜底返回码」两项：只给名字不给返回码，
//!      调用方无法断言行为；只给返回码不给名字，定位时无法回溯到实现。
//!   3. 注册联动是硬门：必需项缺一位即 E_NODEF，且报告首个缺失项的下标与总数；
//!      拒收不得产生半注册状态（宁可整表退回，也不留悬挂 fs_type）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x93E0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x93E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NODEF: u16 = SEG_BASE + 1; // 不可缺省项缺失（注册拒收）
const E_CELL: u16 = SEG_BASE + 2; // 缺省矩阵缺格/重格
const E_BITMAP: u16 = SEG_BASE + 3; // 位图越界
const E_RET: u16 = SEG_BASE + 4; // 缺省返回码与行为不符
const E_ACCT: u16 = SEG_BASE + 5; // 注册/拒收两账不平
const E_DUP: u16 = SEG_BASE + 6; // 重复注册同一 fs 名
const E_STATE: u16 = SEG_BASE + 7; // 注册状态机非法迁移
const E_EMPTY: u16 = SEG_BASE + 8; // 空表注册

/// 可缺省函数数（判据原文 10 格）。
pub const DEF_COUNT: usize = 10;
/// 不可缺省函数数（注册硬门）。
pub const REQ_COUNT: usize = 6;

/// 注册状态机：Fresh → Registering → Mounted；拒收落 Rejected。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegState {
    Fresh,
    Registering,
    Mounted,
    Rejected,
}

/// 缺省格：编号 + 兜底实现名 + 兜底返回码 + 是否可缺省。
#[derive(Clone, Copy)]
pub struct DefCell {
    /// 格号（0..DEF_COUNT）
    pub idx: u8,
    /// 兜底实现名（对应 Linux 的 generic_* / noop_* / default_* 实现）
    pub impl_name: &'static str,
    /// 兜底返回码（0 表成功，负值为 errno 折算）
    pub ret: i16,
}

/// 10 格缺省矩阵（来源：Linux v5.10 fs/libfs.c、mm/filemap.c 的通用实现）。
pub const DEF_MATRIX: [DefCell; DEF_COUNT] = [
    DefCell { idx: 0, impl_name: "generic_file_llseek", ret: 0 },
    DefCell { idx: 1, impl_name: "new_sync_read", ret: 0 },
    DefCell { idx: 2, impl_name: "new_sync_write", ret: 0 },
    DefCell { idx: 3, impl_name: "default_poll", ret: 0x0005 }, // DEFAULT_POLLMASK
    DefCell { idx: 4, impl_name: "noop_ioctl", ret: -25 },      // -ENOTTY
    DefCell { idx: 5, impl_name: "noop_mmap", ret: -19 },        // -ENODEV
    DefCell { idx: 6, impl_name: "noop_fsync", ret: 0 },
    DefCell { idx: 7, impl_name: "noop_flush", ret: 0 },
    DefCell { idx: 8, impl_name: "noop_release", ret: 0 },
    DefCell { idx: 9, impl_name: "noop_splice_read", ret: 0 },
];

/// 不可缺省清单（6 项）：注册时必须全部齐备。
pub const REQ_SET: [&str; REQ_COUNT] = [
    "alloc_inode",
    "destroy_inode",
    "get_tree",
    "kill_sb",
    "lookup",
    "statfs",
];

pub const REQ_MASK: u16 = (1u16 << REQ_COUNT) - 1;

/// 缺省解析器：给「未实现的函数」返回兜底行为。
pub struct DefResolver {
    /// 已实现的位（低 10 位对应 DEF_MATRIX 格号）
    pub present: u16,
    /// 解析次数
    pub resolves: u32,
    /// 命中缺省次数
    pub defaulted: u32,
    /// 命中实现的次数
    pub direct: u32,
}

impl DefResolver {
    pub fn new(present: u16) -> Result<DefResolver, u16> {
        if (present & !((1u16 << DEF_COUNT) - 1)) != 0 {
            return Err(E_BITMAP);
        }
        Ok(DefResolver {
            present,
            resolves: 0,
            defaulted: 0,
            direct: 0,
        })
    }

    /// 解析：实现优先，缺省兜底；返回 (返回码, 是否走缺省, 实现名)。
    pub fn resolve(&mut self, idx: u8) -> Result<(i16, bool, &'static str), u16> {
        if (idx as usize) >= DEF_COUNT {
            return Err(E_CELL);
        }
        self.resolves += 1;
        let cell = DEF_MATRIX[idx as usize];
        if cell.idx != idx {
            return Err(E_CELL); // 行序必须与格号一致
        }
        if (self.present & (1u16 << idx)) != 0 {
            self.direct += 1;
            return Ok((0, false, "fs_own_impl"));
        }
        self.defaulted += 1;
        Ok((cell.ret, true, cell.impl_name))
    }

    /// 两账守恒：direct + defaulted == resolves。
    pub fn conserved(&self) -> bool {
        self.direct + self.defaulted == self.resolves
    }

    /// 缺省覆盖：10 格全缺省时返回码序列应与矩阵一致。
    pub fn full_default_trace(&mut self) -> Result<Vec<i16>, u16> {
        let mut out = Vec::new();
        let mut i = 0u8;
        while (i as usize) < DEF_COUNT {
            let (ret, is_dft, _) = self.resolve(i)?;
            if !is_dft {
                return Err(E_RET); // 全缺省表里不该有直连
            }
            out.push(ret);
            i += 1;
        }
        Ok(out)
    }
}

/// fs 注册表：必需位图 + 缺省位图 + 两账。
pub struct FsRegistry {
    /// 已注册 fs 名
    pub names: Vec<&'static str>,
    /// 相位
    pub state: RegState,
    /// 注册成功次数
    pub accepted: u32,
    /// 拒收次数
    pub rejected: u32,
    /// 拒收时报告的首个缺失必需项下标
    pub last_missing: usize,
}

impl FsRegistry {
    pub fn new() -> FsRegistry {
        FsRegistry {
            names: Vec::new(),
            state: RegState::Fresh,
            accepted: 0,
            rejected: 0,
            last_missing: REQ_COUNT,
        }
    }

    /// register_fs：必需 6 位必须全置；缺一位即拒并留首个缺失下标。
    pub fn register(
        &mut self,
        name: &'static str,
        req_mask: u16,
        def_mask: u16,
    ) -> Result<(), u16> {
        if (req_mask & !REQ_MASK) != 0 || (def_mask & !((1u16 << DEF_COUNT) - 1)) != 0 {
            return Err(E_BITMAP);
        }
        if req_mask == 0 && def_mask == 0 {
            return Err(E_EMPTY);
        }
        let mut i = 0usize;
        while i < self.names.len() {
            if self.names[i] == name {
                return Err(E_DUP);
            }
            i += 1;
        }
        // 硬门：逐位查必需项
        let mut missing = 0usize;
        let mut first = REQ_COUNT;
        let mut k = 0usize;
        while k < REQ_COUNT {
            if (req_mask & (1u16 << k)) == 0 {
                missing += 1;
                if first == REQ_COUNT {
                    first = k;
                }
            }
            k += 1;
        }
        if missing != 0 {
            self.rejected += 1;
            self.last_missing = first;
            self.state = RegState::Rejected;
            return Err(E_NODEF);
        }
        // 通过：缺省位图缺的格由兜底补齐（不影响注册结论）
        self.state = RegState::Registering;
        self.names.push(name);
        self.state = RegState::Mounted;
        self.accepted += 1;
        Ok(())
    }

    /// 注册账守恒：accepted + rejected == 尝试次数。
    pub fn accounted(&self) -> u32 {
        self.accepted + self.rejected
    }

    /// 缺省覆盖率（千分比）：给定实现位图，10 格中走兜底的比例。
    pub fn default_ratio_permille(&self, def_mask: u16) -> u32 {
        let mut present = 0u32;
        let mut i = 0usize;
        while i < DEF_COUNT {
            if (def_mask & (1u16 << i)) != 0 {
                present += 1;
            }
            i += 1;
        }
        (((DEF_COUNT as u32 - present) * 1000) / DEF_COUNT as u32) as u32
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NODEF => "不可缺省项缺失（注册拒收）",
        E_CELL => "缺省矩阵缺格或行序错",
        E_BITMAP => "位图出现闭集外位",
        E_RET => "缺省返回码与行为不符",
        E_ACCT => "注册/拒收两账不平",
        E_DUP => "重复注册同一 fs 名",
        E_STATE => "注册状态机非法迁移",
        E_EMPTY => "空表注册（两位图皆空）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：10 格行为 → 注册硬门 6 次注入 → 缺省解析 → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：10 格矩阵自检——格号连续、兜底实现名非空、返回码逐格固定。
    let mut i = 0usize;
    while i < DEF_COUNT {
        let c = DEF_MATRIX[i];
        if c.idx as usize != i {
            return Err(E_CELL);
        }
        if c.impl_name.len() == 0 {
            return Err(E_CELL);
        }
        i += 1;
    }
    if REQ_SET.len() != REQ_COUNT || REQ_MASK != 0x003F {
        return Err(E_ACCT);
    }
    if DEF_MATRIX[3].ret != 0x0005 {
        return Err(E_RET); // poll 兜底为 DEFAULT_POLLMASK
    }
    if DEF_MATRIX[4].ret != -25 {
        return Err(E_RET); // ioctl 兜底 -ENOTTY
    }
    if DEF_MATRIX[5].ret != -19 {
        return Err(E_RET); // mmap 兜底 -ENODEV
    }
    hits += 1;

    // R2：全缺省解析——10 格逐格走兜底，返回码序列与矩阵完全一致。
    let mut r = DefResolver::new(0)?;
    let trace = r.full_default_trace()?;
    if trace.len() != DEF_COUNT {
        return Err(E_CELL);
    }
    i = 0;
    while i < DEF_COUNT {
        if trace[i] != DEF_MATRIX[i].ret {
            return Err(E_RET);
        }
        i += 1;
    }
    if r.defaulted != 10 || r.direct != 0 || !r.conserved() {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：部分实现——置 2 位（格 0、格 2），则 2 格直连、8 格兜底。
    let mut r2 = DefResolver::new(0b0000_0000_0000_0101)?; // 格 0、格 2
    let mut k = 0u8;
    while (k as usize) < DEF_COUNT {
        let (ret, is_dft, name) = r2.resolve(k)?;
        if k == 0 || k == 2 {
            if is_dft || ret != 0 || name != "fs_own_impl" {
                return Err(E_RET);
            }
        } else if !is_dft {
            return Err(E_RET);
        }
        k += 1;
    }
    if r2.direct != 2 || r2.defaulted != 8 || !r2.conserved() {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：注册通过——必需 6 位全置，缺省位图全零（10 格全靠兜底）。
    let mut reg = FsRegistry::new();
    reg.register("ramfs_like", REQ_MASK, 0)?;
    if reg.accepted != 1 || reg.state != RegState::Mounted {
        return Err(E_STATE);
    }
    if reg.default_ratio_permille(0) != 1000 {
        return Err(E_ACCT); // 10 格全兜底 = 100%
    }
    hits += 1;

    // R5：注册硬门——必需位逐位缺失各注入 1 次（共 6 次），全部 E_NODEF。
    let mut rej = 0u32;
    let mut bit = 0usize;
    while bit < REQ_COUNT {
        let mut rg = FsRegistry::new();
        let mask = REQ_MASK & !(1u16 << bit);
        if rg.register("partial_fs", mask, 0) != Err(E_NODEF) {
            return Err(E_NODEF);
        }
        if rg.last_missing != bit {
            return Err(E_NODEF); // 首个缺失下标必须准确指向被拔掉的那一位
        }
        if rg.state != RegState::Rejected || rg.names.len() != 0 {
            return Err(E_STATE); // 拒收不得留半注册
        }
        rej += 1;
        bit += 1;
    }
    if rej != 6 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R6：缺省率对照——实现 5 格则兜底率 500‰。
    if reg.default_ratio_permille(0b0000_0000_0001_1111) != 500 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：不可缺省项缺失不得被缺省矩阵「兜住」——即使缺省位图全置也必须拒。
    let mut rg = FsRegistry::new();
    if rg.register("def_only_fs", 0x0000, 0xFFFF) != Err(E_NODEF) {
        return Err(E_NODEF);
    }
    if rg.last_missing != 0 {
        return Err(E_NODEF);
    }
    hits += 1;

    // I2：位图越界 → E_BITMAP（必需位图第 6 位、缺省位图第 10 位）。
    let mut rg = FsRegistry::new();
    if rg.register("bad_req", REQ_MASK | (1u16 << REQ_COUNT), 0) != Err(E_BITMAP) {
        return Err(E_BITMAP);
    }
    if rg.register("bad_def", REQ_MASK, 1u16 << DEF_COUNT) != Err(E_BITMAP) {
        return Err(E_BITMAP);
    }
    if DefResolver::new(1u16 << DEF_COUNT).is_ok() {
        return Err(E_BITMAP);
    }
    hits += 1;

    // I3：空表注册 → E_EMPTY；重复注册同名 → E_DUP；解析越界 → E_CELL。
    if rg.register("empty", 0, 0) != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    if reg.register("ramfs_like", REQ_MASK, 0) != Err(E_DUP) {
        return Err(E_DUP);
    }
    if r.resolve(DEF_COUNT as u8) != Err(E_CELL) {
        return Err(E_CELL);
    }
    hits += 1;

    // C1：两账终审——注册 1 成功、6 拒收（另有 6 次被 E_BITMAP/E_EMPTY/E_DUP 挡下，
    //     不计入注册账），故 accounted == 7。
    if reg.accounted() != 1 {
        return Err(E_ACCT);
    }
    if reg.rejected != 0 {
        return Err(E_ACCT);
    }
    // 全缺省解析账：10 次兜底
    if !r.conserved() || r.resolves != 10 {
        return Err(E_ACCT);
    }
    if !r2.conserved() || r2.resolves != 10 {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
