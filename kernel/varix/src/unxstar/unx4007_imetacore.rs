//! unx4007_imetacore — UNX-F4007 i_mode/i_size/i_nlink 元数据核（诊断段 0x83A0）。
//!
//! 判据原文：
//!   J1 元数据核三字段（i_mode/i_size/i_nlink）在创建/链接/截断路径的每次
//!      变更落账，账目可导出。
//!   J2 nlink 计数与 link/unlink 操作次数严格守恒（万次操作账零偏差）。
//!   J3 目录 nlink 语义：mkdir 使父目录 nlink +1、子目录初值 2；rmdir 反向。
//!   J4 非法变更（nlink 减到 0 以下、size 越界、mode 域越界）全部拒收。
//! 设计纪律：
//!   1. i_nlink 为 u32，硬链接上限 65_000（EXT4_LINK_MAX=65000），超限
//!      返回 EMLINK；减到 0 即 inode 待回收态，不允许再减。
//!   2. i_size 为 u64 字节数，非负天然成立；截断只能向小，扩充走写路径；
//!      size 上限 2^63-1（MAX_LFS_FILESIZE 保守锚）。
//!   3. 变更账三项：mode 账、size 账、nlink 账；每次变更先记账后改值，
//!      账目与现值双向可核（现值可由初值 + 变更账推导）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x83A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MODE_OVER: u16 = SEG_BASE + 1;  // i_mode 越出 32 位合法域
const E_SIZE_OVER: u16 = SEG_BASE + 2;  // i_size 超过上限 2^63-1
const E_NLINK_LOW: u16 = SEG_BASE + 3;  // nlink 减到 0 以下（重复 unlink）
const E_NLINK_MAX: u16 = SEG_BASE + 4;  // nlink 超硬链接上限
const E_TYPE_LOCK: u16 = SEG_BASE + 5;  // 类型位段被变更操作篡改
const E_BAL: u16 = SEG_BASE + 6;        // 变更账与现值不平

/// 硬链接上限（Linux EXT4_LINK_MAX = 65000，s_link_max 语义锚）。
pub const LINK_MAX: u32 = 65_000;
/// 文件尺寸上限（MAX_LFS_FILESIZE 保守值 2^63-1）。
pub const SIZE_MAX: u64 = 0x7FFF_FFFF_FFFF_FFFF;
/// 类型位段掩码（变更操作不得触碰）。
pub const S_IFMT: u32 = 0o170000;
/// 权限与特殊位掩码。
pub const S_PERM: u32 = 0o7777;

/// 元数据核三字段载体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IMeta {
    pub i_mode: u32,  // 类型 + 权限全量位
    pub i_size: u64,  // 数据字节数
    pub i_nlink: u32, // 硬链接计数
}

impl IMeta {
    /// 创建：nlink 初值 1（普通文件），目录由 mkdir 另行给 2。
    pub fn new(i_mode: u32, size: u64, nlink: u32) -> Result<IMeta, u16> {
        if i_mode & !(S_IFMT | S_PERM) != 0 {
            return Err(E_MODE_OVER);
        }
        if size > SIZE_MAX {
            return Err(E_SIZE_OVER);
        }
        if nlink > LINK_MAX {
            return Err(E_NLINK_MAX);
        }
        Ok(IMeta { i_mode, i_size: size, i_nlink: nlink })
    }

    /// 类型位段读出口。
    pub fn type_bits(&self) -> u32 {
        self.i_mode & S_IFMT
    }
}

/// 变更类别（操作账键）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetaOp {
    Create = 0,  // 创建
    Link = 1,    // 硬链接 +1
    Unlink = 2,  // 解除链接 -1
    Truncate = 3, // 截断（size 变更）
    WriteGrow = 4, // 写扩充
    Chmod = 5,   // 权限变更
}

pub const OP_KINDS: usize = 6;

/// 元数据核状态机：Active（nlink>0）↔ Orphan（nlink==0 待回收）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetaState {
    Active = 0, // 仍有链接，可访问
    Orphan = 1, // nlink 归零，等待回收
}

/// 元数据核：现值 + 变更账 + 操作账。
pub struct MetaCore {
    pub meta: IMeta,             // 现值
    pub init: IMeta,             // 初值（推导校验锚）
    pub op_count: [u32; OP_KINDS], // 六类操作计数
    pub link_delta: i64,         // link 累计增量
    pub unlink_delta: i64,       // unlink 累计增量（正数表示减少量）
    pub size_delta: i64,         // size 净变化（可负）
    pub mode_chg: u32,           // 权限变更次数
    pub state: MetaState,        // 链接状态机
}

impl MetaCore {
    pub fn new(mode: u32) -> Result<MetaCore, u16> {
        let m = IMeta::new(mode, 0, 1)?;
        Ok(MetaCore {
            meta: m,
            init: m,
            op_count: [0; OP_KINDS],
            link_delta: 0,
            unlink_delta: 0,
            size_delta: 0,
            mode_chg: 0,
            state: MetaState::Active,
        })
    }

    /// 状态机读出口。
    pub fn state(&self) -> MetaState {
        self.state
    }

    /// 创建后类型位段锁定校验。
    fn type_locked(&self, mode: u32) -> bool {
        (mode & S_IFMT) == self.init.type_bits()
    }

    /// 硬链接：nlink +1，超上限拒收（判据 J4）。
    pub fn link(&mut self) -> Result<u32, u16> {
        if self.meta.i_nlink >= LINK_MAX {
            return Err(E_NLINK_MAX);
        }
        self.meta.i_nlink += 1;
        self.link_delta += 1;
        self.state = MetaState::Active;
        self.op_count[MetaOp::Link as usize] += 1;
        Ok(self.meta.i_nlink)
    }

    /// 解除链接：nlink -1，减到 0 以下拒收（重复 unlink 注入面）。
    pub fn unlink(&mut self) -> Result<u32, u16> {
        if self.meta.i_nlink == 0 {
            return Err(E_NLINK_LOW);
        }
        self.meta.i_nlink -= 1;
        self.unlink_delta += 1;
        if self.meta.i_nlink == 0 {
            self.state = MetaState::Orphan;
        }
        self.op_count[MetaOp::Unlink as usize] += 1;
        Ok(self.meta.i_nlink)
    }

    /// 截断到指定长度：只能向小或持平，超上限拒收。
    pub fn truncate(&mut self, new_size: u64) -> Result<u64, u16> {
        if new_size > SIZE_MAX {
            return Err(E_SIZE_OVER);
        }
        if new_size > self.meta.i_size {
            return Err(E_SIZE_OVER); // 截断不得扩充，扩充走 write_grow
        }
        let d = self.meta.i_size - new_size;
        self.meta.i_size = new_size;
        self.size_delta -= d as i64;
        self.op_count[MetaOp::Truncate as usize] += 1;
        Ok(self.meta.i_size)
    }

    /// 写扩充：新增字节累加，超上限拒收。
    pub fn write_grow(&mut self, bytes: u64) -> Result<u64, u16> {
        let next = match self.meta.i_size.checked_add(bytes) {
            Some(v) => v,
            None => return Err(E_SIZE_OVER),
        };
        if next > SIZE_MAX {
            return Err(E_SIZE_OVER);
        }
        self.meta.i_size = next;
        self.size_delta += bytes as i64;
        self.op_count[MetaOp::WriteGrow as usize] += 1;
        Ok(next)
    }

    /// 权限变更：类型位段不得被篡改（判据 J4 的 type_lock 面）。
    pub fn chmod(&mut self, perm: u32) -> Result<u32, u16> {
        if perm & !S_PERM != 0 {
            return Err(E_MODE_OVER);
        }
        let mode = (self.meta.i_mode & S_IFMT) | perm;
        if !self.type_locked(mode) {
            return Err(E_TYPE_LOCK);
        }
        self.meta.i_mode = mode;
        self.mode_chg += 1;
        self.op_count[MetaOp::Chmod as usize] += 1;
        Ok(mode)
    }

    /// 目录 nlink 语义：mkdir 使父 +1（子初值 2 由 new 后手工置）。
    pub fn dir_child_added(&mut self) -> Result<u32, u16> {
        self.link()
    }

    /// 目录 nlink 语义：rmdir 使父 -1。
    pub fn dir_child_removed(&mut self) -> Result<u32, u16> {
        self.unlink()
    }

    /// 现值推导校验：nlink == init + link_delta - unlink_delta。
    pub fn nlink_conserved(&self) -> bool {
        let expect = self.init.i_nlink as i64 + self.link_delta - self.unlink_delta;
        expect == self.meta.i_nlink as i64
    }

    /// size 推导校验：size == init + size_delta。
    pub fn size_conserved(&self) -> bool {
        let expect = self.init.i_size as i64 + self.size_delta;
        expect == self.meta.i_size as i64
    }

    /// 总守恒：nlink + size 双推导同时成立，且操作账与增量账一致。
    pub fn conserve(&self) -> bool {
        self.nlink_conserved()
            && self.size_conserved()
            && self.op_count[MetaOp::Link as usize] as i64 == self.link_delta
            && self.op_count[MetaOp::Unlink as usize] as i64 == self.unlink_delta
    }

    /// 操作账导出：某类操作次数。
    pub fn ops_of(&self, op: MetaOp) -> u32 {
        self.op_count[op as usize]
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_MODE_OVER => "i_mode 越出类型+权限合法域",
        E_SIZE_OVER => "i_size 超上限或截断方向非法",
        E_NLINK_LOW => "nlink 减到 0 以下（重复 unlink）",
        E_NLINK_MAX => "nlink 超硬链接上限 65000",
        E_TYPE_LOCK => "类型位段被变更操作篡改",
        E_BAL => "变更账与现值推导不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：创建 → 链接/解除 → 截断/扩充 → 万次守恒 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：创建 REG 0o100644，nlink 初值 1、size 初值 0。
    let mut core = MetaCore::new(0o100644)?;
    if core.meta.i_nlink != 1 || core.meta.i_size != 0 {
        return Err(E_BAL);
    }
    if core.meta.type_bits() != 0o100000 {
        return Err(E_TYPE_LOCK);
    }
    hits += 1;

    // R2：写扩充 4096 → size 4096；截断 1000 → size 1000（判据 J1）。
    core.write_grow(4096)?;
    if core.meta.i_size != 4096 {
        return Err(E_BAL);
    }
    core.truncate(1000)?;
    if core.meta.i_size != 1000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：链接九次 → nlink 10；解除九次 → nlink 1（判据 J2 小样本）。
    for _ in 0..9 {
        core.link()?;
    }
    if core.meta.i_nlink != 10 {
        return Err(E_BAL);
    }
    for _ in 0..9 {
        core.unlink()?;
    }
    if core.meta.i_nlink != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：万次 link/unlink 交替——nlink 恒为 1，账零偏差（判据 J2 原文）。
    for n in 0..10_000u32 {
        if n % 2 == 0 {
            core.link()?;
        } else {
            core.unlink()?;
        }
    }
    if core.meta.i_nlink != 1 {
        return Err(E_BAL);
    }
    if !core.nlink_conserved() || !core.size_conserved() || !core.conserve() {
        return Err(E_BAL);
    }
    if core.ops_of(MetaOp::Link) != 9 + 5000 || core.ops_of(MetaOp::Unlink) != 9 + 5000 {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：目录 nlink 语义——父目录 mkdir/rmdir 各一次净零。
    let mut dir = MetaCore::new(0o040755)?;
    dir.dir_child_added()?;
    if dir.meta.i_nlink != 2 {
        return Err(E_BAL);
    }
    dir.dir_child_removed()?;
    if dir.meta.i_nlink != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R6：chmod 改权限不动类型位段（0o100600 后仍 REG）。
    let m = core.chmod(0o600)?;
    if m & S_IFMT != 0o100000 || m & S_PERM != 0o600 {
        return Err(E_TYPE_LOCK);
    }
    hits += 1;

    // I1：nlink 减到 0 后再减（重复 unlink）被拒。
    let mut one = MetaCore::new(0o100644)?;
    one.unlink()?; // 1 → 0
    if one.state() != MetaState::Orphan {
        return Err(E_BAL);
    }
    if one.unlink() != Err(E_NLINK_LOW) {
        return Err(E_NLINK_LOW);
    }
    hits += 1;

    // I2：截断方向注入（向大）被拒。
    if core.truncate(core.meta.i_size + 1) != Err(E_SIZE_OVER) {
        return Err(E_SIZE_OVER);
    }
    hits += 1;

    // I3：size 越上限注入被拒。
    if core.write_grow(SIZE_MAX) != Err(E_SIZE_OVER) {
        return Err(E_SIZE_OVER);
    }
    hits += 1;

    // I4：mode 域越界与类型篡改注入双拒。
    if core.chmod(0o10000) != Err(E_MODE_OVER) {
        return Err(E_MODE_OVER);
    }
    if IMeta::new(0o200000, 0, 1) != Err(E_MODE_OVER) {
        return Err(E_MODE_OVER);
    }
    hits += 1;

    // R7：末态守恒复核（注入未破坏账）。
    if !core.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
