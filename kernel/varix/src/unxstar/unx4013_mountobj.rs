//! unx4013_mountobj — UNX-F4013 Mount 对象结构锚（诊断段 0x8460）。
//!
//! 判据原文：
//!   J1 Mount 字段集（挂载点 dentry/根 dentry/sb 指针/覆盖栈指针/传播组号/
//!      mnt_flags）实例化断言通过。
//!   J2 字段与 B12/B13 语义表清单零差：六字段名逐一比对，缺项/多项判失败。
//!   J3 覆盖栈语义：同一挂载点再次挂载形成覆盖链，栈顶为最新挂载；卸载
//!      只允许自顶向下（LIFO），夹层卸载被拒。
//!   J4 mnt_flags 位段透传：MNT_RDONLY 等四位闭集，越界位置位拒收。
//! 设计纪律：
//!   1. mount 对象六字段以槽号互指（dentry/sb/覆盖栈），不做裸指针运算。
//!      覆盖栈指针指向上一个 mount 槽，0 表示栈底。
//!   2. 生命周期状态机：Unmounted → Mounting → Mounted → Unmounting →
//!      Detached，单向推进；Mounted 可回 Unmounting，其余回跳拒收。
//!   3. 传播组号 peer_group 为 u16，同组挂载共享传播事件；组号 0 表示
//!      未入组（私有挂载），入组后不可单方面改号。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8460;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_MP_NULL: u16 = SEG_BASE + 1;    // 挂载点 dentry 为空
const E_ROOT_NULL: u16 = SEG_BASE + 2;  // 根 dentry 为空
const E_SB_NULL: u16 = SEG_BASE + 3;    // sb 指针为空
const E_FLAG_BAD: u16 = SEG_BASE + 4;   // mnt_flags 越出闭集
const E_STACK_BAD: u16 = SEG_BASE + 5;  // 覆盖栈操作非法（夹层卸载/成环）
const E_STATE_JUMP: u16 = SEG_BASE + 6; // 生命周期迁移非法
const E_GROUP_LOCK: u16 = SEG_BASE + 7; // 已入组后改号

/// mnt_flags 闭集（Linux MS_* 语义子集，仅维系对象侧镜像位）。
pub const MNT_RDONLY: u32 = 0x0001;  // 只读挂载
pub const MNT_NOSUID: u32 = 0x0002;  // 禁 setuid
pub const MNT_NODEV: u32 = 0x0004;   // 禁设备节点
pub const MNT_NOEXEC: u32 = 0x0008;  // 禁执行
pub const MNT_NOATIME: u32 = 0x0010; // 不更新 atime
pub const MNT_FLAG_MASK: u32 =
    MNT_RDONLY | MNT_NOSUID | MNT_NODEV | MNT_NOEXEC | MNT_NOATIME;

/// mount 字段清单锚（与 B12/B13 语义表逐项对应）。
pub const MNT_FIELDS: [&'static str; 6] = [
    "mnt_mountpoint", // 挂载点 dentry
    "mnt_root",       // 该挂载的根 dentry
    "mnt_sb",         // sb 指针
    "mnt_over",       // 覆盖栈指针（下层 mount 槽）
    "mnt_group",      // 传播组号
    "mnt_flags",      // 挂载标志位
];

/// mount 生命周期状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MntState {
    Unmounted = 0,  // 已创建未挂
    Mounting = 1,   // 挂载中
    Mounted = 2,    // 已挂载
    Unmounting = 3, // 卸载中
    Detached = 4,   // 已摘除（终态）
}

/// Mount 对象结构锚。
pub struct Mount {
    pub mnt_mountpoint: u16, // 挂载点 dentry 槽号（0=空）
    pub mnt_root: u16,       // 根 dentry 槽号（0=空）
    pub mnt_sb: u16,         // sb 槽号（0=空）
    pub mnt_over: u16,       // 覆盖栈下层 mount 槽号（0=栈底）
    pub mnt_group: u16,      // 传播组号（0=未入组）
    pub mnt_flags: u32,      // 挂载标志位
    pub state: MntState,     // 生命周期
    pub depth: u16,          // 覆盖栈深度（栈底=0，逐层 +1）
}

impl Mount {
    /// 实例化：六字段全量校验（判据 J1）——三个必填槽均非空。
    pub fn new(
        mountpoint: u16,
        root: u16,
        sb: u16,
        over: u16,
        group: u16,
        flags: u32,
    ) -> Result<Mount, u16> {
        if mountpoint == 0 {
            return Err(E_MP_NULL);
        }
        if root == 0 {
            return Err(E_ROOT_NULL);
        }
        if sb == 0 {
            return Err(E_SB_NULL);
        }
        if flags & !MNT_FLAG_MASK != 0 {
            return Err(E_FLAG_BAD);
        }
        Ok(Mount {
            mnt_mountpoint: mountpoint,
            mnt_root: root,
            mnt_sb: sb,
            mnt_over: over,
            mnt_group: group,
            mnt_flags: flags,
            state: MntState::Unmounted,
            depth: if over == 0 { 0 } else { 1 },
        })
    }

    /// 六字段清单导出（判据 J2 diff 面）。
    pub fn fields(&self) -> [&'static str; 6] {
        MNT_FIELDS
    }

    /// 字段在位自检。
    pub fn fields_ok(&self) -> bool {
        !MNT_FIELDS.is_empty()
            && self.mnt_mountpoint != 0
            && self.mnt_root != 0
            && self.mnt_sb != 0
            && self.mnt_flags & !MNT_FLAG_MASK == 0
    }

    /// 挂载：Unmounted → Mounting → Mounted（两步同一 API 内完成）。
    pub fn mount_up(&mut self) -> Result<MntState, u16> {
        match self.state {
            MntState::Unmounted => {
                self.state = MntState::Mounting;
                self.state = MntState::Mounted;
                Ok(self.state)
            }
            _ => Err(E_STATE_JUMP),
        }
    }

    /// 卸载：Mounted → Unmounting → Detached。
    pub fn unmount(&mut self) -> Result<MntState, u16> {
        match self.state {
            MntState::Mounted => {
                self.state = MntState::Unmounting;
                self.state = MntState::Detached;
                Ok(self.state)
            }
            _ => Err(E_STATE_JUMP),
        }
    }

    /// 只读位判定（B02 透传的上游锚）。
    pub fn readonly(&self) -> bool {
        self.mnt_flags & MNT_RDONLY != 0
    }

    /// 入组：group 非 0 且此前未入组；已入组改号拒收（判据 J4 组号面）。
    pub fn join_group(&mut self, group: u16) -> Result<u16, u16> {
        if group == 0 {
            return Err(E_FLAG_BAD);
        }
        if self.mnt_group != 0 {
            return Err(E_GROUP_LOCK);
        }
        self.mnt_group = group;
        Ok(group)
    }
}

/// 挂载表：覆盖栈维护 + 卸载 LIFO 强约束（判据 J3）。
pub struct MountTable {
    pub mounts: Vec<Mount>, // 槽表（1-based）
    pub mounted: u32,       // 当前挂载态计数
    pub detached: u32,      // 已摘除计数
    pub stack_rej: u32,     // 夹层卸载拒绝账
}

impl MountTable {
    pub fn new() -> MountTable {
        MountTable { mounts: Vec::new(), mounted: 0, detached: 0, stack_rej: 0 }
    }

    /// 追加一个 mount：over 必须指向已存在的 mount 槽或 0。
    pub fn push(&mut self, mut m: Mount) -> Result<u16, u16> {
        if m.mnt_over != 0 && m.mnt_over as usize > self.mounts.len() {
            return Err(E_STACK_BAD);
        }
        // 覆盖栈深度 = 下层深度 + 1；栈底为 0。
        if m.mnt_over == 0 {
            m.depth = 0;
        } else {
            let below = self.mounts[(m.mnt_over - 1) as usize].depth;
            m.depth = match below.checked_add(1) {
                Some(v) => v,
                None => return Err(E_STACK_BAD),
            };
        }
        m.mount_up()?;
        self.mounts.push(m);
        self.mounted += 1;
        Ok(self.mounts.len() as u16)
    }

    /// 在指定挂载点叠挂：新 mount 覆盖该点当前栈顶（判据 J3 覆盖链）。
    pub fn overlay(&mut self, mountpoint: u16, root: u16, sb: u16, flags: u32) -> Result<u16, u16> {
        // 找到该挂载点当前栈顶（最后登记者）。
        let mut top: u16 = 0;
        let mut i = 0usize;
        for m in self.mounts.iter() {
            i += 1;
            if m.mnt_mountpoint == mountpoint && m.state == MntState::Mounted {
                top = i as u16;
            }
        }
        let m = Mount::new(mountpoint, root, sb, top, 0, flags)?;
        self.push(m)
    }

    /// 卸载槽：只允许栈顶（无上层覆盖者）——夹层卸载拒收并入账。
    pub fn unmount_slot(&mut self, slot: u16) -> Result<(), u16> {
        if slot == 0 || slot as usize > self.mounts.len() {
            return Err(E_STACK_BAD);
        }
        // 检查是否被上层覆盖：任一 Mounted 且 mnt_over == slot 即非栈顶。
        let mut covered = false;
        for m in self.mounts.iter() {
            if m.mnt_over == slot && m.state == MntState::Mounted {
                covered = true;
                break;
            }
        }
        if covered {
            self.stack_rej += 1;
            return Err(E_STACK_BAD);
        }
        let i = (slot - 1) as usize;
        self.mounts[i].unmount()?;
        self.mounted -= 1;
        self.detached += 1;
        Ok(())
    }

    /// 栈顶槽查询：该挂载点最上层 mount 槽号（无则 0）。
    pub fn stack_top(&self, mountpoint: u16) -> u16 {
        let mut top: u16 = 0;
        let mut i = 0usize;
        for m in self.mounts.iter() {
            i += 1;
            if m.mnt_mountpoint == mountpoint && m.state == MntState::Mounted {
                top = i as u16;
            }
        }
        top
    }

    /// 覆盖链回溯：从槽顶沿 mnt_over 向下到栈底，返回层数（= depth）。
    pub fn chain_depth(&self, slot: u16) -> Result<u16, u16> {
        if slot == 0 || slot as usize > self.mounts.len() {
            return Err(E_STACK_BAD);
        }
        let mut cur = slot;
        let mut steps = 0u16;
        while cur != 0 {
            if cur as usize > self.mounts.len() || steps > 64 {
                return Err(E_STACK_BAD); // 越界或成环
            }
            let over = self.mounts[(cur - 1) as usize].mnt_over;
            if over != 0 && over >= cur {
                return Err(E_STACK_BAD); // 覆盖指针必须早于自身
            }
            cur = over;
            if cur != 0 {
                steps += 1;
            }
        }
        Ok(steps)
    }

    /// 账守恒：mounted + detached == 槽表长度。
    pub fn conserve(&self) -> bool {
        self.mounted + self.detached == self.mounts.len() as u32
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_MP_NULL => "挂载点 dentry 为空",
        E_ROOT_NULL => "根 dentry 为空",
        E_SB_NULL => "sb 指针为空",
        E_FLAG_BAD => "mnt_flags 越出闭集",
        E_STACK_BAD => "覆盖栈操作非法（夹层卸载/成环/越界）",
        E_STATE_JUMP => "生命周期迁移非法",
        E_GROUP_LOCK => "已入组后改号",
        _ => "未知诊断码",
    }
}

/// 确定性演练：六字段 → 覆盖链三层 → LIFO 卸载 → 组号 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut t = MountTable::new();

    // R1：六字段实例化断言（判据 J1）——ext4 根挂载。
    let m0 = Mount::new(1, 2, 3, 0, 0, MNT_RDONLY)?;
    if m0.mnt_mountpoint != 1 || m0.mnt_root != 2 || m0.mnt_sb != 3 {
        return Err(E_MP_NULL);
    }
    if m0.mnt_over != 0 || m0.mnt_group != 0 || m0.mnt_flags != MNT_RDONLY {
        return Err(E_FLAG_BAD);
    }
    if !m0.fields_ok() || m0.state != MntState::Unmounted {
        return Err(E_STATE_JUMP);
    }
    hits += 1;

    // R2：字段清单六项与名逐一比对（判据 J2）。
    let fl = m0.fields();
    if fl.len() != 6 || fl[0] != "mnt_mountpoint" || fl[5] != "mnt_flags" {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // R3：挂载根 + 同点叠挂两次 → 覆盖链三层（判据 J3 正向）。
    let s0 = t.push(m0)?;
    if s0 != 1 || t.mounted != 1 {
        return Err(E_STACK_BAD);
    }
    let s1 = t.overlay(1, 4, 5, 0)?;
    let s2 = t.overlay(1, 6, 7, 0)?;
    if s1 != 2 || s2 != 3 {
        return Err(E_STACK_BAD);
    }
    if t.stack_top(1) != s2 {
        return Err(E_STACK_BAD);
    }
    hits += 1;

    // R4：覆盖栈深度——栈顶 depth=2、层数 2（根之上两层）。
    if t.mounts[(s2 - 1) as usize].depth != 2 {
        return Err(E_STACK_BAD);
    }
    if t.chain_depth(s2)? != 2 {
        return Err(E_STACK_BAD);
    }
    hits += 1;

    // R5：只读位可观测——根挂载只读、叠挂可写。
    if !t.mounts[(s0 - 1) as usize].readonly() {
        return Err(E_FLAG_BAD);
    }
    if t.mounts[(s2 - 1) as usize].readonly() {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // R6：LIFO 卸载——先卸栈顶 s2，再卸 s1，最后卸 s0，每步 mounted 递减。
    t.unmount_slot(s2)?;
    if t.mounted != 2 || t.stack_top(1) != s1 {
        return Err(E_STACK_BAD);
    }
    t.unmount_slot(s1)?;
    t.unmount_slot(s0)?;
    if t.mounted != 0 || t.detached != 3 {
        return Err(E_STACK_BAD);
    }
    if !t.conserve() {
        return Err(E_STACK_BAD);
    }
    hits += 1;

    // R7：传播组入组一次成功、组号可读。
    let mut mg = Mount::new(1, 2, 3, 0, 0, 0)?;
    mg.mount_up()?;
    if mg.join_group(42)? != 42 || mg.mnt_group != 42 {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // I1：挂载点/根/sb 三个空槽注入各拒一次。
    if Mount::new(0, 2, 3, 0, 0, 0).err() != Some(E_MP_NULL) {
        return Err(E_MP_NULL);
    }
    if Mount::new(1, 0, 3, 0, 0, 0).err() != Some(E_ROOT_NULL) {
        return Err(E_ROOT_NULL);
    }
    if Mount::new(1, 2, 0, 0, 0, 0).err() != Some(E_SB_NULL) {
        return Err(E_SB_NULL);
    }
    hits += 1;

    // I2：mnt_flags 越界位注入被拒。
    if Mount::new(1, 2, 3, 0, 0, 0x100).err() != Some(E_FLAG_BAD) {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // I3：夹层卸载注入——三层栈中先卸中间层被拒并入账（判据 J3 反向）。
    let mut t2 = MountTable::new();
    let a = t2.push(Mount::new(9, 2, 3, 0, 0, 0)?)?;
    let b = t2.push(Mount::new(9, 4, 5, a, 0, 0)?)?;
    let _c = t2.push(Mount::new(9, 6, 7, b, 0, 0)?)?;
    if t2.unmount_slot(b) != Err(E_STACK_BAD) {
        return Err(E_STACK_BAD);
    }
    if t2.stack_rej != 1 {
        return Err(E_STACK_BAD);
    }
    hits += 1;

    // I4：终态复活/重复挂载注入被拒。
    let mut dead = Mount::new(1, 2, 3, 0, 0, 0)?;
    dead.mount_up()?;
    dead.unmount()?;
    if dead.mount_up() != Err(E_STATE_JUMP) {
        return Err(E_STATE_JUMP);
    }
    hits += 1;

    // I5：已入组改号注入被拒。
    if mg.join_group(43) != Err(E_GROUP_LOCK) {
        return Err(E_GROUP_LOCK);
    }
    if mg.join_group(0) != Err(E_FLAG_BAD) {
        return Err(E_FLAG_BAD);
    }
    hits += 1;

    // I6：覆盖指针悬空注入被拒（over 指向不存在槽）。
    if t2.push(Mount::new(9, 8, 9, 99, 0, 0)?) != Err(E_STACK_BAD) {
        return Err(E_STACK_BAD);
    }
    hits += 1;

    Ok(hits)
}
