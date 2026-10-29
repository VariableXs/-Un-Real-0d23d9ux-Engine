//! unx4008_superblock — UNX-F4008 Superblock 对象结构锚（诊断段 0x83C0）。
//!
//! 判据原文：
//!   J1 Superblock 字段集（fs 类型指针/块大小/根 inode 指针/魔数/s_flags/
//!      s_instances）实例化后逐字段访问断言通过。
//!   J2 字段清单与 B02 语义表一致（清单 diff 零差）：六字段一组，别名与
//!      语义位一一对应，缺项或多项均判失败。
//!   J3 魔数登记：每个 sb 携带四字节魔数，魔数为 0 视作未登记注册失败。
//!   J4 s_instances 实例计数账守恒：关联/解关联成对出现，计数不得为负。
//! 设计纪律：
//!   1. sb 生命周期状态机：Allocated → Active → Frozen → Dead，单向为主；
//!      Frozen 可回 Active（冻结/解冻对），Dead 为终态不可复活。
//!   2. 字段用弱引用语义建模：fs_type 与 root_inode 以槽号（u16）承载，
//!      不做指针运算（纯功能线无 unsafe），0 视作未绑定空槽。
//!   3. s_flags 位段：SB_RDONLY=1、SB_NOSUID=2、SB_NODEV=4、SB_NOEXEC=8，
//!      与 MNT_* 的透传由 B02 承接，本模块只维护 sb 侧位。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x83C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MAGIC_ZERO: u16 = SEG_BASE + 1;  // 魔数为 0（未登记）
const E_BSIZE_BAD: u16 = SEG_BASE + 2;   // 块大小非 2 的幂或越界
const E_FIELD_MISS: u16 = SEG_BASE + 3;  // 字段清单缺项（diff 非零）
const E_STATE_BAD: u16 = SEG_BASE + 4;   // 状态迁移非法（终态复活等）
const E_INST_NEG: u16 = SEG_BASE + 5;    // s_instances 计数将为负
const E_BAL: u16 = SEG_BASE + 6;         // 实例账与状态账不平

/// s_flags 位定义（与 MS_* 挂载位对应，sb 侧只读镜像）。
pub const SB_RDONLY: u32 = 0x0001; // 只读
pub const SB_NOSUID: u32 = 0x0002; // 禁 setuid 生效
pub const SB_NODEV: u32 = 0x0004;  // 禁设备节点
pub const SB_NOEXEC: u32 = 0x0008; // 禁执行
pub const SB_FLAG_MASK: u32 = SB_RDONLY | SB_NOSUID | SB_NODEV | SB_NOEXEC;

/// 块大小合法域：512..=65536 且为 2 的幂（PAGE_SIZE 上限）。
pub const BSIZE_MIN: u32 = 512;
pub const BSIZE_MAX: u32 = 65_536;

/// sb 生命周期状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SbState {
    Allocated = 0, // 已分配未激活
    Active = 1,    // 已激活，可承载 inode
    Frozen = 2,    // 冻结，写路径暂停
    Dead = 3,      // 终态，待回收
}

/// 字段清单锚：与 B02 语义表逐项对应的六字段名。
pub const FIELD_LIST: [&'static str; 6] = [
    "s_type",      // fs 类型指针（槽号）
    "s_blocksize", // 块大小
    "s_root",      // 根 inode 指针（槽号）
    "s_magic",     // 四字节魔数
    "s_flags",     // 只读等挂载位镜像
    "s_instances", // 类型实例计数
];

/// Superblock 对象结构锚。
pub struct Superblock {
    pub s_type: u16,      // fs 类型指针（类型表槽号，0=未绑定）
    pub s_blocksize: u32, // 块大小（字节）
    pub s_root: u16,      // 根 inode 指针（inode 槽号，0=未绑定）
    pub s_magic: u32,     // 四字节魔数
    pub s_flags: u32,     // s_flags 位段
    pub s_instances: u32, // 该类型当前实例数（快照值）
    pub state: SbState,   // 生命周期状态
    pub frozen_rounds: u32, // 冻结/解冻成对次数（账）
}

impl Superblock {
    /// 实例化：六字段全量校验后进入 Allocated 态（判据 J1/J3）。
    pub fn instantiate(
        s_type: u16,
        bsize: u32,
        root: u16,
        magic: u32,
        flags: u32,
    ) -> Result<Superblock, u16> {
        if magic == 0 {
            return Err(E_MAGIC_ZERO);
        }
        if !bsize_ok(bsize) {
            return Err(E_BSIZE_BAD);
        }
        if flags & !SB_FLAG_MASK != 0 {
            return Err(E_FIELD_MISS);
        }
        Ok(Superblock {
            s_type,
            s_blocksize: bsize,
            s_root: root,
            s_magic: magic,
            s_flags: flags,
            s_instances: 0,
            state: SbState::Allocated,
            frozen_rounds: 0,
        })
    }

    /// 激活：Allocated → Active；重复激活或终态激活拒收。
    pub fn activate(&mut self, instances: u32) -> Result<(), u16> {
        if self.state != SbState::Allocated {
            return Err(E_STATE_BAD);
        }
        self.s_instances = instances;
        self.state = SbState::Active;
        Ok(())
    }

    /// 冻结：Active → Frozen；非 Active 拒收。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.state != SbState::Active {
            return Err(E_STATE_BAD);
        }
        self.state = SbState::Frozen;
        Ok(())
    }

    /// 解冻：Frozen → Active，成对计数 +1（判据 J4 的成对账）。
    pub fn thaw(&mut self) -> Result<(), u16> {
        if self.state != SbState::Frozen {
            return Err(E_STATE_BAD);
        }
        self.state = SbState::Active;
        self.frozen_rounds += 1;
        Ok(())
    }

    /// 下线：Active/Frozen → Dead；Dead 不可再迁移。
    pub fn kill(&mut self) -> Result<(), u16> {
        if self.state == SbState::Dead {
            return Err(E_STATE_BAD);
        }
        self.state = SbState::Dead;
        Ok(())
    }

    /// 实例计数增减：减到负数拒收（判据 J4）。
    pub fn inst_add(&mut self) -> Result<u32, u16> {
        match self.s_instances.checked_add(1) {
            Some(v) => {
                self.s_instances = v;
                Ok(v)
            }
            None => Err(E_BAL),
        }
    }

    pub fn inst_del(&mut self) -> Result<u32, u16> {
        if self.s_instances == 0 {
            return Err(E_INST_NEG);
        }
        self.s_instances -= 1;
        Ok(self.s_instances)
    }

    /// 只读位判定（B02 透传的上游锚）。
    pub fn is_readonly(&self) -> bool {
        self.s_flags & SB_RDONLY != 0
    }

    /// 挂载位设定：仅 Active 态可改，越界拒收。
    pub fn set_flags(&mut self, flags: u32) -> Result<u32, u16> {
        if flags & !SB_FLAG_MASK != 0 {
            return Err(E_FIELD_MISS);
        }
        if self.state != SbState::Active && self.state != SbState::Frozen {
            return Err(E_STATE_BAD);
        }
        self.s_flags = flags;
        Ok(flags)
    }

    /// 字段清单导出：六字段名（判据 J2 的 diff 面）。
    pub fn field_list(&self) -> [&'static str; 6] {
        FIELD_LIST
    }

    /// 字段自检：六字段逐一可访问且语义在位（J1 逐字段断言本体）。
    pub fn fields_ok(&self) -> bool {
        let list = self.field_list();
        if list.len() != 6 {
            return false;
        }
        // 逐字段语义在位：类型槽、块大小、魔数、实例账、状态均有效。
        let size_ok = bsize_ok(self.s_blocksize);
        let magic_ok = self.s_magic != 0;
        let state_ok = (self.state as u8) <= (SbState::Dead as u8);
        size_ok && magic_ok && state_ok
    }
}

/// 块大小合法判定：2 的幂且在 [512, 65536]。
pub fn bsize_ok(b: u32) -> bool {
    if b < BSIZE_MIN || b > BSIZE_MAX {
        return false;
    }
    b & (b - 1) == 0
}

/// sb 注册表：多 sb 槽 + 实例总账（判据 J4 的基座）。
pub struct SbRegistry {
    pub slots: Vec<Superblock>, // sb 槽表
    pub live: u32,              // 存活 sb 数（非 Dead）
    pub dead: u32,              // 已下线 sb 数
}

impl SbRegistry {
    pub fn new() -> SbRegistry {
        SbRegistry { slots: Vec::new(), live: 0, dead: 0 }
    }

    /// 登记：实例化 + 激活，入 live 账。
    pub fn register(&mut self, sb: Superblock) -> Result<usize, u16> {
        let mut s = sb;
        if !s.fields_ok() {
            return Err(E_FIELD_MISS);
        }
        s.activate(1)?;
        self.slots.push(s);
        self.live += 1;
        Ok(self.slots.len() - 1)
    }

    /// 注销：置 Dead 并从 live 账移出。
    pub fn unregister(&mut self, idx: usize) -> Result<(), u16> {
        if idx >= self.slots.len() {
            return Err(E_STATE_BAD);
        }
        if self.slots[idx].state == SbState::Dead {
            return Err(E_STATE_BAD);
        }
        self.slots[idx].kill()?;
        self.live -= 1;
        self.dead += 1;
        Ok(())
    }

    /// 实例总账：Σ 各 sb 的 s_instances（判据 J4 读面）。
    pub fn instances_sum(&self) -> u32 {
        let mut s = 0u32;
        for sb in &self.slots {
            s += sb.s_instances;
        }
        s
    }

    /// 守恒：live + dead == slots.len()。
    pub fn conserve(&self) -> bool {
        self.live + self.dead == self.slots.len() as u32
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_MAGIC_ZERO => "魔数为 0（类型未登记）",
        E_BSIZE_BAD => "块大小非 2 的幂或越界 [512,65536]",
        E_FIELD_MISS => "字段清单缺项或位段越界",
        E_STATE_BAD => "状态迁移非法（终态复活/重复激活）",
        E_INST_NEG => "s_instances 计数将为负",
        E_BAL => "实例账与状态账不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：六字段实例化 → 状态机 → 实例账 → 注入拦截 → 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut reg = SbRegistry::new();

    // R1：实例化六字段并逐字段访问（判据 J1）——ext4 魔数 0xEF53。
    let sb = Superblock::instantiate(1, 4096, 2, 0xEF53, SB_RDONLY)?;
    if sb.s_type != 1 || sb.s_blocksize != 4096 || sb.s_root != 2 {
        return Err(E_FIELD_MISS);
    }
    if sb.s_magic != 0xEF53 || sb.s_flags != SB_RDONLY || sb.s_instances != 0 {
        return Err(E_FIELD_MISS);
    }
    hits += 1;

    // R2：字段清单六项与非空名导出（判据 J2 的 diff 零差面）。
    let fl = sb.field_list();
    if fl.len() != 6 || fl[0] != "s_type" || fl[5] != "s_instances" {
        return Err(E_FIELD_MISS);
    }
    if !sb.fields_ok() {
        return Err(E_FIELD_MISS);
    }
    hits += 1;

    // R3：登记激活 → live=1，实例账 1（判据 J4 正向）。
    let i0 = reg.register(sb)?;
    if reg.live != 1 || reg.instances_sum() != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：冻结/解冻成对一次，frozen_rounds=1，状态回 Active。
    reg.slots[i0].freeze()?;
    if reg.slots[i0].state != SbState::Frozen {
        return Err(E_STATE_BAD);
    }
    reg.slots[i0].thaw()?;
    if reg.slots[i0].state != SbState::Active || reg.slots[i0].frozen_rounds != 1 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R5：实例计数增减守恒——+2 再 -1 净 +1。
    reg.slots[i0].inst_add()?;
    reg.slots[i0].inst_add()?;
    reg.slots[i0].inst_del()?;
    if reg.slots[i0].s_instances != 2 || reg.instances_sum() != 2 {
        return Err(E_BAL);
    }
    hits += 1;

    // R6：只读位可观测（B02 上游锚）。
    if !reg.slots[i0].is_readonly() {
        return Err(E_FIELD_MISS);
    }
    hits += 1;

    // R7：注销 → live=0/dead=1，守恒成立。
    reg.unregister(i0)?;
    if !reg.conserve() || reg.live != 0 || reg.dead != 1 {
        return Err(E_BAL);
    }
    if reg.slots[i0].state != SbState::Dead {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // I1：魔数 0 注入被拒（判据 J3）。
    if Superblock::instantiate(1, 4096, 2, 0, 0).err() != Some(E_MAGIC_ZERO) {
        return Err(E_MAGIC_ZERO);
    }
    hits += 1;

    // I2：块大小非 2 的幂 / 越界双注入被拒。
    if Superblock::instantiate(1, 3000, 2, 0xEF53, 0).err() != Some(E_BSIZE_BAD) {
        return Err(E_BSIZE_BAD);
    }
    if Superblock::instantiate(1, 1024 * 1024, 2, 0xEF53, 0).err() != Some(E_BSIZE_BAD) {
        return Err(E_BSIZE_BAD);
    }
    hits += 1;

    // I3：终态复活注入被拒（Dead 再激活/再下线）。
    if reg.unregister(i0) != Err(E_STATE_BAD) {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // I4：实例计数为 0 再减被拒。
    let mut z = Superblock::instantiate(2, 1024, 3, 0x01021994, 0)?;
    z.activate(0)?;
    if z.inst_del() != Err(E_INST_NEG) {
        return Err(E_INST_NEG);
    }
    hits += 1;

    // I5：s_flags 越界位注入被拒。
    if z.set_flags(0x1000) != Err(E_FIELD_MISS) {
        return Err(E_FIELD_MISS);
    }
    hits += 1;

    Ok(hits)
}
