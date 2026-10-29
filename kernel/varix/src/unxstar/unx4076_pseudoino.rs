//! unx4076_pseudoino — UNX-F4076 伪 inode（proc 类动态生成）锚（诊断段 0x8BC0）。
//!
//! 判据原文：UNX-F4076-J1 动态生成 inode 不占盘上号空间（与固定号隔离账），
//!           生成-销毁万次零泄漏（残差=0）。
//! 可验性质（本模块自证面）：
//!   J1 伪号分配器吐出的每个号都必须落在 [PSEUDO_BASE, PSEUDO_TOP) 且 >= 盘上号
//!      上界 DISK_TOP，隔离账（verify_isolated）逐号核对零违例；
//!   J2 万次 allocate→release 后 created == destroyed == 10000、live == 0，
//!      残差 created - destroyed - live == 0（零泄漏）；
//!   J3 伪号单调不复用：next_off 只增不减，销毁后的旧号再 allocate 拿不到
//!      （proc 类伪 inode 号发出去就不能回收，防 ABA）；
//!   J4 销毁路径分账：重复销毁记 E_DOUBLE_FREE、销毁不存在的号记 E_NOT_LIVE，
//!      两者都不许动 destroyed 计数（错误路径不入账）；
//!   J5 生成类闭集 PseudoKind 只能取三类之一，类号随节点登记，读回不得串类。
//! 设计纪律：
//!   1. 号段切分写死：盘上号 [1, DISK_TOP)，伪号 [PSEUDO_BASE, PSEUDO_TOP)。
//!      PSEUDO_BASE = 1<<40，DISK_TOP = 1<<32（ext4 i_ino 为 32 位），两区之间
//!      留 2^8 倍空档，越界即拒（E_SPACE_MIX / E_DISK_OOR）。
//!   2. 分配器只增加 never-reuse：proc/sysfs 类伪 inode 的正身是 dentry 缓存条目，
//!      号复用会让旧 dentry 指向新对象（ABA），所以销毁只退 live 账不退 off 账。
//!   3. 销毁槽位另记 recycled 表（供 B08 侧观察"应回收实例数"），但该表不参与
//!      号分配，两账分离，禁止拿 recycled 长度当可分配余量。
//!   4. 全表线性查找（伪 inode 在世量级千条），查前比 len，未命中走 Option。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8BC0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8BC0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_SPACE_MIX: u16 = SEG_BASE + 1; // 伪号落入盘上号空间（隔离账失守）
const E_DOUBLE_FREE: u16 = SEG_BASE + 2; // 重复销毁同号
const E_NOT_LIVE: u16 = SEG_BASE + 3; // 销毁不存在的号
const E_EXHAUST: u16 = SEG_BASE + 4; // 伪号空间耗尽
const E_DISK_OOR: u16 = SEG_BASE + 5; // 盘上号越界登记
const E_KIND_MIX: u16 = SEG_BASE + 6; // 生成类与登记类不符
const E_BAL: u16 = SEG_BASE + 7; // 生成-销毁-live 三账不平

/// 盘上号上限：ext4 i_ino 为 32 位，故 1<<32 为硬上界（号 0 另作保留）。
pub const DISK_TOP: u64 = 1 << 32;

/// 伪号起点：1<<40，与 DISK_TOP 相隔 256 倍，绝无交叠。
pub const PSEUDO_BASE: u64 = 1 << 40;

/// 伪号上限：1<<48，单机 proc 类伪 inode 不可能触到（余量 2^40 个号）。
pub const PSEUDO_TOP: u64 = 1 << 48;

/// 伪 inode 生成类闭集（proc/sysfs/devtmpfs 三族）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PseudoKind {
    Proc = 0,   // /proc 下的进程/内核视图伪文件
    Sysfs = 1,  // /sys 下的 kobject 视图伪文件
    DevTmp = 2, // devtmpfs 动态设备节点
}

/// 单节点生命周期态：Free 只出现在预留槽，Live 是在世，Retired 是已销毁。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PseudoState {
    Free = 0,    // 槽位已建未发号
    Live = 1,    // 号已发出，dentry 可引用
    Retired = 2, // 已销毁，号永不复用
}

/// 号空间切分描述：把"隔离"这件事变成可核对的常量对。
pub struct InoSpace {
    pub disk_top: u64,   // 盘上号上界（不含）
    pub pseudo_base: u64, // 伪号下界（含）
    pub pseudo_top: u64,  // 伪号上界（不含）
}

impl InoSpace {
    /// 默认切分：盘上 32 位、伪号 40 位起。
    pub fn new() -> InoSpace {
        InoSpace {
            disk_top: DISK_TOP,
            pseudo_base: PSEUDO_BASE,
            pseudo_top: PSEUDO_TOP,
        }
    }

    /// 定制切分（drill 的耗尽注入用小空间，其余约束同上）。
    pub fn with_bounds(disk_top: u64, base: u64, top: u64) -> Result<InoSpace, u16> {
        if base <= disk_top || top <= base {
            return Err(E_SPACE_MIX); // 切分本身就不隔离，构造即拒
        }
        Ok(InoSpace {
            disk_top,
            pseudo_base: base,
            pseudo_top: top,
        })
    }

    /// 号归属判定：true = 伪号区，false = 盘上号区（含非法 0）。
    pub fn is_pseudo(&self, ino: u64) -> bool {
        ino >= self.pseudo_base && ino < self.pseudo_top
    }

    /// 盘上号合法性：1..disk_top（0 号保留）。
    pub fn is_disk(&self, ino: u64) -> bool {
        ino != 0 && ino < self.disk_top
    }
}

/// 伪 inode 节点：号 + 类 + 态 + 代次 + 槽位。
pub struct PseudoNode {
    pub ino: u64,          // 伪 inode 号（PSEUDO_BASE + off）
    pub kind: PseudoKind,  // 生成类
    pub state: PseudoState, // 生命周期态
    pub gen: u32,          // 同槽再生日次（本模块不复用号，仅作观察账）
    pub slot: u32,         // 分配槽位号（recycled 表引用它）
}

/// 伪号分配器：单调发号 + 在世表 + 回收槽表 + 三本账。
pub struct PseudoAlloc {
    pub space: InoSpace,       // 号空间切分
    pub nodes: Vec<PseudoNode>, // 全量节点表（含 Retired，保留观察）
    pub recycled: Vec<u32>,    // 已回收槽位表（不参与发号）
    pub next_off: u64,         // 下一个号偏移（只增）
    pub created: u32,          // 生成计数
    pub destroyed: u32,        // 销毁计数
    pub live: u32,             // 在世计数
    pub high_water: u32,       // 在世峰值（判据"峰值有账"面）
}

impl PseudoAlloc {
    pub fn new() -> PseudoAlloc {
        PseudoAlloc {
            space: InoSpace::new(),
            nodes: Vec::new(),
            recycled: Vec::new(),
            next_off: 0,
            created: 0,
            destroyed: 0,
            live: 0,
            high_water: 0,
        }
    }

    /// 定制空间构造（耗尽注入用）。
    pub fn with_space(space: InoSpace) -> PseudoAlloc {
        PseudoAlloc {
            space,
            nodes: Vec::new(),
            recycled: Vec::new(),
            next_off: 0,
            created: 0,
            destroyed: 0,
            live: 0,
            high_water: 0,
        }
    }

    /// 动态生成一枚伪 inode：号 = pseudo_base + next_off。
    /// 生成前先判耗尽（next_off 不得越出 pseudo_top - pseudo_base）。
    pub fn allocate(&mut self, kind: PseudoKind) -> Result<u64, u16> {
        let span = self.space.pseudo_top - self.space.pseudo_base;
        if self.next_off >= span {
            return Err(E_EXHAUST);
        }
        let ino = self.space.pseudo_base + self.next_off;
        // 隔离自检：发号瞬间再核一次，防切分被外部改坏（判据 J1 的硬闸）
        if !self.space.is_pseudo(ino) || ino < self.space.disk_top {
            return Err(E_SPACE_MIX);
        }
        let slot = self.nodes.len() as u32;
        self.nodes.push(PseudoNode {
            ino,
            kind,
            state: PseudoState::Live,
            gen: 1,
            slot,
        });
        self.next_off += 1;
        self.created += 1;
        self.live += 1;
        if self.live > self.high_water {
            self.high_water = self.live;
        }
        Ok(ino)
    }

    /// 线性定位节点下标（查前比 len，未命中 None）。
    pub fn find(&self, ino: u64) -> Option<usize> {
        if !self.space.is_pseudo(ino) {
            return None; // 盘上号根本不在本表，先按号段拒，避免无谓扫描
        }
        let mut i = 0usize;
        while i < self.nodes.len() {
            if self.nodes[i].ino == ino {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 销毁：只退 live 账，号与 off 账永不动（J3 单调不复用）。
    pub fn release(&mut self, ino: u64) -> Result<(), u16> {
        let idx = match self.find(ino) {
            Some(i) => i,
            None => return Err(E_NOT_LIVE),
        };
        match self.nodes[idx].state {
            PseudoState::Live => {}
            PseudoState::Retired => return Err(E_DOUBLE_FREE),
            PseudoState::Free => return Err(E_NOT_LIVE),
        }
        self.nodes[idx].state = PseudoState::Retired;
        let slot = self.nodes[idx].slot;
        self.recycled.push(slot);
        self.destroyed += 1;
        if self.live > 0 {
            self.live -= 1;
        } else {
            return Err(E_BAL); // 在世账已为 0 还要减，说明账早就不平
        }
        Ok(())
    }

    /// 读类：已销毁节点不再报类（防上层沿旧号取类做出错误语义）。
    pub fn kind_of(&self, ino: u64) -> Result<PseudoKind, u16> {
        let idx = match self.find(ino) {
            Some(i) => i,
            None => return Err(E_NOT_LIVE),
        };
        if self.nodes[idx].state != PseudoState::Live {
            return Err(E_NOT_LIVE);
        }
        Ok(self.nodes[idx].kind)
    }

    /// 盘上号登记口：任何走这条路的号必须落在盘上号区，否则 E_SPACE_MIX。
    pub fn register_disk_ino(&self, ino: u64) -> Result<(), u16> {
        if self.space.is_pseudo(ino) {
            return Err(E_SPACE_MIX); // 拿伪号冒充盘上号，隔离账直接击穿
        }
        if !self.space.is_disk(ino) {
            return Err(E_DISK_OOR);
        }
        Ok(())
    }

    /// 类一致性复核：登记的类必须与请求类相符（drill 用于串类注入）。
    pub fn require_kind(&self, ino: u64, want: PseudoKind) -> Result<(), u16> {
        let got = self.kind_of(ino)?;
        if got != want {
            return Err(E_KIND_MIX);
        }
        Ok(())
    }

    /// 隔离终审：全表逐号必须落在伪号区（J1）。
    pub fn verify_isolated(&self) -> bool {
        let mut i = 0usize;
        while i < self.nodes.len() {
            let ino = self.nodes[i].ino;
            if !self.space.is_pseudo(ino) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 三账守恒：created == destroyed + live，且 destroyed 与回收槽表等长。
    pub fn conserved(&self) -> bool {
        self.created == self.destroyed + self.live && self.destroyed as usize == self.recycled.len()
    }

    /// 残差（判据 J2 的直读面）：created - destroyed - live，正常恒 0。
    pub fn residual(&self) -> u32 {
        let base = self.destroyed.saturating_add(self.live);
        self.created.saturating_sub(base)
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_SPACE_MIX => "伪号与盘上号空间交叠（隔离账失守）",
        E_DOUBLE_FREE => "同一伪 inode 重复销毁",
        E_NOT_LIVE => "销毁/读取不存在的伪 inode 号",
        E_EXHAUST => "伪号空间耗尽（next_off 触顶）",
        E_DISK_OOR => "盘上号越界（0 或 >= DISK_TOP）",
        E_KIND_MIX => "请求类与登记类不符（串类）",
        E_BAL => "生成-销毁-在世三账不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：正常发号 → 万次零泄漏 → 边界 → 注入拦截 → 三账对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三类各发 4 枚，号严格递增且全部落在伪号区（J1 头一段）。
    let mut pa = PseudoAlloc::new();
    let mut first: u64 = 0;
    let mut last: u64 = 0;
    let mut n = 0u32;
    while n < 4 {
        let a = pa.allocate(PseudoKind::Proc)?;
        pa.allocate(PseudoKind::Sysfs)?; // 二号位顺手占一个，验证三型号段互不重叠
        let c = pa.allocate(PseudoKind::DevTmp)?;
        if n == 0 {
            first = a;
        }
        last = c;
        n += 1;
    }
    if first != PSEUDO_BASE {
        return Err(E_SPACE_MIX); // 首号必须正好是下界
    }
    if last <= first {
        return Err(E_BAL);
    }
    if !pa.verify_isolated() || pa.live != 12 || pa.created != 12 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：类读回不串——第 2 枚（Sysfs）查类必须是 Sysfs（J5）。
    if pa.kind_of(PSEUDO_BASE + 1)? != PseudoKind::Sysfs {
        return Err(E_KIND_MIX);
    }
    if pa.require_kind(PSEUDO_BASE + 2, PseudoKind::Proc) != Err(E_KIND_MIX) {
        return Err(E_KIND_MIX);
    }
    hits += 1;

    // R3：万次生成-销毁零泄漏（J2）——每轮发一枚立刻销毁，残差必须回到 0。
    let mut cyc = PseudoAlloc::new();
    let mut k = 0u32;
    let mut prev_ino = 0u64;
    while k < 10_000 {
        let ino = cyc.allocate(PseudoKind::Proc)?;
        if ino <= prev_ino && k > 0 {
            return Err(E_SPACE_MIX); // 号必须严格单调（J3）
        }
        prev_ino = ino;
        if cyc.live != 1 {
            return Err(E_BAL);
        }
        cyc.release(ino)?;
        if cyc.live != 0 {
            return Err(E_BAL);
        }
        if cyc.residual() != 0 {
            return Err(E_BAL);
        }
        k += 1;
    }
    if cyc.created != 10_000 || cyc.destroyed != 10_000 || cyc.live != 0 {
        return Err(E_BAL);
    }
    if cyc.high_water != 1 {
        return Err(E_BAL); // 逐发逐销，峰值只能是 1
    }
    if !cyc.conserved() || cyc.residual() != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：号不复用——万轮后新号必须比首号大 10000 个偏移（J3 终审）。
    let after = cyc.allocate(PseudoKind::DevTmp)?;
    if after != PSEUDO_BASE + 10_000 {
        return Err(E_SPACE_MIX);
    }
    if cyc.find(prev_ino).map(|i| cyc.nodes[i].state) != Some(PseudoState::Retired) {
        return Err(E_BAL); // 旧号仍在表内，但态必须是 Retired
    }
    hits += 1;

    // R5：盘上号登记口——合法盘上号通过，伪号冒充盘上号被隔离账击穿（J1）。
    if pa.register_disk_ino(1) != Ok(()) {
        return Err(E_SPACE_MIX);
    }
    if pa.register_disk_ino(PSEUDO_BASE) != Err(E_SPACE_MIX) {
        return Err(E_SPACE_MIX);
    }
    if pa.register_disk_ino(DISK_TOP) != Err(E_DISK_OOR) {
        return Err(E_DISK_OOR); // 恰好等于上界也不许
    }
    hits += 1;

    // I1：重复销毁 → E_DOUBLE_FREE，且 destroyed 账不动（J4）。
    let ino = pa.allocate(PseudoKind::Proc)?;
    pa.release(ino)?;
    let d_before = pa.destroyed;
    if pa.release(ino) != Err(E_DOUBLE_FREE) {
        return Err(E_DOUBLE_FREE);
    }
    if pa.destroyed != d_before {
        return Err(E_BAL); // 错误路径不得入销毁账
    }
    hits += 1;

    // I2：销毁不存在号 → E_NOT_LIVE（伪号区未发号 + 盘上号双点）。
    if pa.release(PSEUDO_BASE + 0xFFFF) != Err(E_NOT_LIVE) {
        return Err(E_NOT_LIVE);
    }
    if pa.release(0x1234) != Err(E_NOT_LIVE) {
        return Err(E_NOT_LIVE);
    }
    hits += 1;

    // I3：读已销毁节点的类 → E_NOT_LIVE（不得沿旧号返回陈旧类）。
    if pa.kind_of(ino) != Err(E_NOT_LIVE) {
        return Err(E_NOT_LIVE);
    }
    hits += 1;

    // I4：耗尽注入——切出只有 2 个号的小空间，第 3 次发号必须 E_EXHAUST。
    let small = InoSpace::with_bounds(1 << 16, 1 << 20, (1 << 20) + 2)?;
    let mut tiny = PseudoAlloc::with_space(small);
    tiny.allocate(PseudoKind::Proc)?;
    tiny.allocate(PseudoKind::Proc)?;
    if tiny.allocate(PseudoKind::Proc) != Err(E_EXHAUST) {
        return Err(E_EXHAUST);
    }
    if tiny.created != 2 || tiny.live != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // I5：切分本身不隔离（base <= disk_top）→ 构造即拒。
    match InoSpace::with_bounds(1 << 40, 1 << 40, 1 << 48) {
        Err(E_SPACE_MIX) => {}
        _ => return Err(E_SPACE_MIX),
    }
    match InoSpace::with_bounds(1 << 40, 1 << 48, 1 << 40) {
        Err(E_SPACE_MIX) => {}
        _ => return Err(E_SPACE_MIX),
    }
    hits += 1;

    // R6：终审——大盘三账守恒 + 隔离全绿 + 残差 0。
    if !pa.conserved() || !pa.verify_isolated() || pa.residual() != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
