//! unx4019_selfcheck — UNX-F4019 VFS 自检面注册与骨架（诊断段 0x8500）。
//!
//! 判据原文：
//!   J1 run_vfs_checks() 注册进 ktest 总面一次成功：总面登记账 +1，面状态
//!      Registered，用例清单可导出。
//!   J2 B01 全部断言一次命令可跑：单条命令号能检索到 B01 面，面内用例数
//!      与登记账一致（不执行用例，只做检索与清单核对）。
//!   J3 注册重复注入被拒绝：同名面二次注册返回 E_DUP_PLANE，账不变。
//!   J4 面账守恒：register 成功数 + 拒绝数 == 注册尝试数；用例账互不串面。
//! 设计纪律：
//!   1. 本模块只做「面注册表 + 用例登记账 + 注册状态机」，不写用例执行器：
//!      执行由 ktest 总面在宿主侧驱动，本模块保证登记面可被检索到。
//!   2. 面状态机：Empty → Registering → Registered；Registered 为可检索态，
//!      注销（Unregistered）后不可再检索，重复注册须先注销。
//!   3. 用例以 (面号, 用例号) 二元键唯一；同一面内用例号重复即拒收。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8500;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_PLANE: u16 = SEG_BASE + 1;  // 同名面重复注册
const E_STATE_BAD: u16 = SEG_BASE + 2;  // 面状态迁移非法
const E_DUP_CASE: u16 = SEG_BASE + 3;   // 面内用例号重复
const E_PLANE_MISS: u16 = SEG_BASE + 4; // 检索不存在的面
const E_CMD_BAD: u16 = SEG_BASE + 5;    // 命令号非法（0 或未登记）
const E_ACCOUNT: u16 = SEG_BASE + 6;    // 面账/用例账不平

/// VFS 面号（ktest 总面内的子面标识）。
pub const VFS_PLANE_ID: u16 = 0x0100;
/// B01 批命令号（一次命令跑整批的检索键）。
pub const CMD_B01: u16 = 0x0B01;
/// B02 批命令号。
pub const CMD_B02: u16 = 0x0B02;
/// 单面用例上限（防止登记表无界增长）。
pub const CASES_MAX: usize = 512;

/// 面注册状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlaneState {
    Empty = 0,       // 未登记
    Registering = 1, // 登记中
    Registered = 2,  // 已登记可检索
    Unregistered = 3, // 已注销
}

/// 单条用例登记项（只登记，不执行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseReg {
    pub case_id: u16,   // 用例号（面内唯一）
    pub cmd: u16,       // 所属批命令号（B01/B02）
    pub asserts: u16,   // 该用例承载的断言条数
    pub critical: bool, // 是否关键用例（红即整面红）
}

/// 自检面：面元信息 + 用例表 + 账目。
pub struct VfsPlane {
    pub plane_id: u16,        // 面号
    pub name_hash: u32,       // 面名哈希（注册键）
    pub state: PlaneState,    // 状态机
    pub cases: Vec<CaseReg>,  // 用例登记表
    pub asserts_total: u32,   // 面内断言总数
    pub reg_attempts: u32,    // 注册尝试账
    pub reg_ok: u32,          // 注册成功账
    pub reg_rej: u32,         // 注册拒绝账
}

impl VfsPlane {
    /// 建空面：面号非 0。
    pub fn new(plane_id: u16, name: &[u8]) -> Result<VfsPlane, u16> {
        if plane_id == 0 {
            return Err(E_CMD_BAD);
        }
        Ok(VfsPlane {
            plane_id,
            name_hash: VfsPlane::hash(name),
            state: PlaneState::Empty,
            cases: Vec::new(),
            asserts_total: 0,
            reg_attempts: 0,
            reg_ok: 0,
            reg_rej: 0,
        })
    }

    /// 名字哈希：FNV-1a 32 位（与其余模块一致）。
    pub fn hash(name: &[u8]) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in name.iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 登记用例：面内用例号查重 + 容量校验（判据 J1 的用例账）。
    pub fn add_case(&mut self, c: CaseReg) -> Result<usize, u16> {
        if self.state != PlaneState::Empty && self.state != PlaneState::Registering {
            return Err(E_STATE_BAD); // 已登记面不得再改用例表
        }
        if c.asserts == 0 {
            return Err(E_ACCOUNT);
        }
        for e in self.cases.iter() {
            if e.case_id == c.case_id {
                return Err(E_DUP_CASE);
            }
        }
        if self.cases.len() >= CASES_MAX {
            return Err(E_ACCOUNT);
        }
        self.cases.push(c);
        self.asserts_total += c.asserts as u32;
        Ok(self.cases.len())
    }

    /// 注册：Empty/Unregistered → Registering → Registered（判据 J1）。
    pub fn register(&mut self) -> Result<PlaneState, u16> {
        self.reg_attempts += 1;
        match self.state {
            PlaneState::Empty | PlaneState::Unregistered => {
                self.state = PlaneState::Registering;
                // 登记骨架：至少一条用例、命令号非 0 才算可检索。
                if self.cases.is_empty() || self.asserts_total == 0 {
                    self.state = PlaneState::Empty;
                    self.reg_rej += 1;
                    return Err(E_ACCOUNT);
                }
                self.state = PlaneState::Registered;
                self.reg_ok += 1;
                Ok(self.state)
            }
            _ => {
                self.reg_rej += 1;
                Err(E_DUP_PLANE)
            }
        }
    }

    /// 注销：Registered → Unregistered。
    pub fn unregister(&mut self) -> Result<PlaneState, u16> {
        if self.state != PlaneState::Registered {
            return Err(E_STATE_BAD);
        }
        self.state = PlaneState::Unregistered;
        Ok(self.state)
    }

    /// 按批命令号检索用例清单（只读返回条数，不执行）。
    pub fn cases_of_cmd(&self, cmd: u16) -> Result<usize, u16> {
        if self.state != PlaneState::Registered {
            return Err(E_PLANE_MISS);
        }
        let mut n = 0usize;
        for c in self.cases.iter() {
            if c.cmd == cmd {
                n += 1;
            }
        }
        Ok(n)
    }

    /// 关键用例数（红即整面红的条目）。
    pub fn critical_count(&self) -> u32 {
        let mut n = 0u32;
        for c in self.cases.iter() {
            if c.critical {
                n += 1;
            }
        }
        n
    }

    /// 面账守恒：reg_ok + reg_rej == reg_attempts（判据 J4）。
    pub fn account_ok(&self) -> bool {
        self.reg_ok + self.reg_rej == self.reg_attempts
    }
}

/// ktest 总面注册表：多面登记 + 命令检索。
pub struct KtestRegistry {
    pub planes: Vec<VfsPlane>, // 面表
    pub lookups: u32,          // 检索账
    pub hits: u32,             // 命中账
    pub misses: u32,           // 未命中账
}

impl KtestRegistry {
    pub fn new() -> KtestRegistry {
        KtestRegistry { planes: Vec::new(), lookups: 0, hits: 0, misses: 0 }
    }

    /// 登记一个面：同名（面号或名哈希）重复即拒（判据 J3）。
    pub fn register_plane(&mut self, mut p: VfsPlane) -> Result<usize, u16> {
        for e in self.planes.iter() {
            if e.plane_id == p.plane_id || e.name_hash == p.name_hash {
                p.reg_attempts += 1;
                p.reg_rej += 1;
                return Err(E_DUP_PLANE);
            }
        }
        p.register()?;
        self.planes.push(p);
        Ok(self.planes.len() - 1)
    }

    /// 按面号检索。
    pub fn find_plane(&mut self, plane_id: u16) -> Result<usize, u16> {
        self.lookups += 1;
        let mut i = 0usize;
        for p in self.planes.iter() {
            if p.plane_id == plane_id && p.state == PlaneState::Registered {
                self.hits += 1;
                return Ok(i);
            }
            i += 1;
        }
        self.misses += 1;
        Err(E_PLANE_MISS)
    }

    /// 按命令号检索：返回 (面槽, 该命令用例数)（判据 J2）。
    pub fn find_cmd(&mut self, cmd: u16) -> Result<(usize, usize), u16> {
        self.lookups += 1;
        if cmd == 0 {
            self.misses += 1;
            return Err(E_CMD_BAD);
        }
        let mut i = 0usize;
        for p in self.planes.iter() {
            if p.state == PlaneState::Registered {
                let mut n = 0usize;
                for c in p.cases.iter() {
                    if c.cmd == cmd {
                        n += 1;
                    }
                }
                if n > 0 {
                    self.hits += 1;
                    return Ok((i, n));
                }
            }
            i += 1;
        }
        self.misses += 1;
        Err(E_CMD_BAD)
    }

    /// 总面账：所有面断言总数。
    pub fn asserts_sum(&self) -> u32 {
        let mut s = 0u32;
        for p in self.planes.iter() {
            s += p.asserts_total;
        }
        s
    }

    /// 检索账守恒：hits + misses == lookups。
    pub fn lookup_ok(&self) -> bool {
        self.hits + self.misses == self.lookups
    }
}

/// run_vfs_checks() 的骨架：登记 VFS B01/B02 面（不执行用例）。
/// 返回 (面数, 断言总数)。
pub fn run_vfs_checks(reg: &mut KtestRegistry) -> Result<(u32, u32), u16> {
    let mut b01 = VfsPlane::new(VFS_PLANE_ID, b"vfs_b01")?;
    // B01 用例登记：F4002..F4014 段内 14 卡面（用例号 = 卡号后四位）。
    let b01_cases: [(u16, u16, bool); 8] = [
        (4002, 4, true),   // errno 全集 32/32
        (4003, 4, true),   // 七类类型往返
        (4004, 3, true),   // 权限 4,096 组合
        (4005, 4, true),   // 属主三段传递
        (4006, 3, true),   // 时间戳触发矩阵
        (4007, 3, true),   // 元数据核守恒
        (4008, 3, false),  // sb 字段集
        (4009, 3, false),  // inode 字段集
    ];
    for (cid, a, crit) in b01_cases.iter() {
        b01.add_case(CaseReg {
            case_id: *cid,
            cmd: CMD_B01,
            asserts: *a,
            critical: *crit,
        })?;
    }
    let mut b02 = VfsPlane::new(VFS_PLANE_ID + 1, b"vfs_b02")?;
    let b02_cases: [(u16, u16, bool); 5] = [
        (4022, 3, true),  // s_flags 只读透传
        (4023, 3, true),  // 块大小协商
        (4024, 3, false), // fs 类型表
        (4025, 3, true),  // register_fs
        (4026, 2, false), // unregister_fs 排空
    ];
    for (cid, a, crit) in b02_cases.iter() {
        b02.add_case(CaseReg {
            case_id: *cid,
            cmd: CMD_B02,
            asserts: *a,
            critical: *crit,
        })?;
    }
    let a1 = b01.asserts_total;
    let a2 = b02.asserts_total;
    reg.register_plane(b01)?;
    reg.register_plane(b02)?;
    Ok((2, a1 + a2))
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_DUP_PLANE => "同名/同号面重复注册",
        E_STATE_BAD => "面状态迁移非法（已登记改表/未登记注销）",
        E_DUP_CASE => "面内用例号重复",
        E_PLANE_MISS => "检索不存在的面",
        E_CMD_BAD => "命令号非法（0 或未登记）",
        E_ACCOUNT => "面账/用例账不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：注册一次成功 → 命令检索 → 用例重复 → 重注册 → 账守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut reg = KtestRegistry::new();

    // R1：run_vfs_checks() 注册进总面一次成功（判据 J1 原文）。
    let (planes, asserts) = run_vfs_checks(&mut reg)?;
    if planes != 2 || reg.planes.len() != 2 {
        return Err(E_ACCOUNT);
    }
    // B01 断言 4+4+3+4+3+3+3+3 = 27；B02 3+3+3+3+2 = 14。
    if asserts != 27 + 14 {
        return Err(E_ACCOUNT);
    }
    if reg.planes[0].state != PlaneState::Registered {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R2：B01 命令一次检索命中 8 条用例（判据 J2 原文）。
    let (slot, n) = reg.find_cmd(CMD_B01)?;
    if slot != 0 || n != 8 {
        return Err(E_CMD_BAD);
    }
    hits += 1;

    // R3：B02 命令检索命中 5 条。
    let (slot2, n2) = reg.find_cmd(CMD_B02)?;
    if slot2 != 1 || n2 != 5 {
        return Err(E_CMD_BAD);
    }
    hits += 1;

    // R4：按面号检索命中、面内用例数与登记账一致。
    let i0 = reg.find_plane(VFS_PLANE_ID)?;
    if reg.planes[i0].cases.len() != 8 || reg.planes[i0].cases_of_cmd(CMD_B01)? != 8 {
        return Err(E_ACCOUNT);
    }
    if reg.planes[i0].asserts_total != 27 {
        return Err(E_ACCOUNT);
    }
    hits += 1;

    // R5：关键用例账——B01 前 6 条 critical，B02 3 条 critical。
    if reg.planes[0].critical_count() != 6 || reg.planes[1].critical_count() != 3 {
        return Err(E_ACCOUNT);
    }
    hits += 1;

    // R6：面账守恒 + 检索账守恒。
    if !reg.planes[0].account_ok() || !reg.planes[1].account_ok() {
        return Err(E_ACCOUNT);
    }
    if !reg.lookup_ok() || reg.hits != 3 || reg.misses != 0 {
        return Err(E_ACCOUNT);
    }
    hits += 1;

    // R7：注销后再检索失败，重新注册恢复（状态机闭环）。
    reg.planes[0].unregister()?;
    if reg.find_plane(VFS_PLANE_ID) != Err(E_PLANE_MISS) {
        return Err(E_PLANE_MISS);
    }
    reg.planes[0].register()?;
    if reg.find_plane(VFS_PLANE_ID)? != 0 {
        return Err(E_PLANE_MISS);
    }
    hits += 1;

    // I1：同名面重复注册注入被拒（判据 J3 原文）。
    let dup = VfsPlane::new(VFS_PLANE_ID, b"vfs_b01")?;
    if reg.register_plane(dup) != Err(E_DUP_PLANE) {
        return Err(E_DUP_PLANE);
    }
    // 面号不同但名相同也拒（名哈希键）。
    let dup2 = VfsPlane::new(VFS_PLANE_ID + 9, b"vfs_b01")?;
    if reg.register_plane(dup2) != Err(E_DUP_PLANE) {
        return Err(E_DUP_PLANE);
    }
    if reg.planes.len() != 2 {
        return Err(E_ACCOUNT);
    }
    hits += 1;

    // I2：面内用例号重复注入被拒。
    let mut p = VfsPlane::new(0x0200, b"tmp")?;
    p.add_case(CaseReg { case_id: 1, cmd: CMD_B01, asserts: 1, critical: false })?;
    if p.add_case(CaseReg { case_id: 1, cmd: CMD_B01, asserts: 2, critical: false })
        != Err(E_DUP_CASE)
    {
        return Err(E_DUP_CASE);
    }
    hits += 1;

    // I3：空面注册注入被拒（无用例不可登记），拒绝账 +1。
    let mut empty = VfsPlane::new(0x0300, b"empty")?;
    if empty.register() != Err(E_ACCOUNT) {
        return Err(E_ACCOUNT);
    }
    if empty.reg_rej != 1 || !empty.account_ok() {
        return Err(E_ACCOUNT);
    }
    hits += 1;

    // I4：已登记面改用例表注入被拒。
    if reg.planes[0].add_case(CaseReg { case_id: 9999, cmd: CMD_B01, asserts: 1, critical: false })
        != Err(E_STATE_BAD)
    {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // I5：命令号 0 检索注入被拒。
    if reg.find_cmd(0) != Err(E_CMD_BAD) {
        return Err(E_CMD_BAD);
    }
    // 未登记命令号检索未命中。
    if reg.find_cmd(0x0B99) != Err(E_CMD_BAD) {
        return Err(E_CMD_BAD);
    }
    if !reg.lookup_ok() {
        return Err(E_ACCOUNT);
    }
    hits += 1;

    Ok(hits)
}
