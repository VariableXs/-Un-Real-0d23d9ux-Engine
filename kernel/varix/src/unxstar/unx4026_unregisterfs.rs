//! unx4026_unregisterfs — UNX-F4026 unregister_fs 与在途引用排空（诊断段 0x85C0）。
//!
//! 判据原文：
//!   J1 有在途 sb 实例时 unregister_fs 返回 EBUSY（=16），实例计数账等于
//!      在途数（每个实例一账，不多不少）。
//!   J2 全部实例 kill 后注销成功，类型表项消失（再查找不可得）。
//!   J3 排空是幂等前置：重复 kill 同一实例被拒，kill 未知实例被拒。
//!   J4 引用账守恒：类型项持有 instances 计数 == 存活实例数 == 表内出现次数。
//! 设计纪律：
//!   1. 类型项与 sb 实例分离：类型项持 instances 计数与 live 位图（以实例
//!      槽号集合表达），unregister_fs 只在 live 集合为空时摘除类型项。
//!   2. 实例生命周期：Born → Live → Killed（终态）；kill 后计数 -1，
//!      重复 kill 视为非法迁移（E_INST_DEAD）。
//!   3. EBUSY=16 与 errno 基座一致；段内另有 0x85C0 诊断码覆盖账目类错误。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x85C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_TYPE_MISS: u16 = SEG_BASE + 1;  // 类型项不存在（未注册/已摘除）
const E_INST_DEAD: u16 = SEG_BASE + 2;  // 重复 kill / 已死实例
const E_INST_OOR: u16 = SEG_BASE + 3;   // 实例槽号越界
const E_STATE_BAD: u16 = SEG_BASE + 4;  // 类型状态迁移非法
const E_BAL: u16 = SEG_BASE + 5;        // 实例账与表账不平
const E_NAME_BAD: u16 = SEG_BASE + 6;   // 名非法（空/超长）

/// EBUSY（判据 J1 要求的返回码）。
pub const EBUSY: u16 = 16;

/// fs 名长度上限。
pub const FS_NAME_MAX: usize = 15;
/// 实例表容量。
pub const INST_MAX: usize = 128;

/// 类型项状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeLife {
    Live = 0,    // 已注册，可挂载
    Draining = 1, // 正在排空（已发起注销，等待实例归零）
    Gone = 2,    // 已摘除（终态）
}

/// sb 实例状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstState {
    Born = 0,   // 刚创建（挂载中）
    Live = 1,   // 已激活（可读写）
    Killed = 2, // 已杀死（终态）
}

/// sb 实例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SbInstance {
    pub type_id: u16, // 所属类型槽号
    pub state: InstState, // 生命周期
    pub sb_magic: u32, // 该实例的 sb 魔数（唯一性留痕）
}

/// 类型项：名 + 实例集合 + 计数账。
pub struct FsTypeEntry {
    pub name_len: u8,     // 名长度
    pub name_hash: u32,   // 名哈希
    pub life: TypeLife,   // 类型生命周期
    pub instances: Vec<usize>, // 存活实例槽号集合
    pub unreg_attempts: u32,   // 注销尝试账
    pub busy_rej: u32,         // EBUSY 拒绝账
}

impl FsTypeEntry {
    /// 在途实例数（判据 J1 的「实例计数账」）。
    pub fn live_count(&self) -> u32 {
        self.instances.len() as u32
    }
}

/// 注销管理器：类型表 + 实例表 + 账。
pub struct UnregManager {
    pub types: Vec<FsTypeEntry>, // 类型表
    pub insts: Vec<SbInstance>,  // 实例表
    pub created_insts: u32,      // 实例创建账
    pub killed_insts: u32,       // 实例杀死账
    pub unreg_ok: u32,           // 注销成功账
    pub unreg_busy: u32,         // 注销 EBUSY 账
    pub gone: u32,               // 已摘除类型数
}

impl UnregManager {
    pub fn new() -> UnregManager {
        UnregManager {
            types: Vec::new(),
            insts: Vec::new(),
            created_insts: 0,
            killed_insts: 0,
            unreg_ok: 0,
            unreg_busy: 0,
            gone: 0,
        }
    }

    /// 名哈希：FNV-1a 32 位。
    pub fn hash(name: &[u8]) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for b in name.iter() {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    /// 注册类型（前置，便于本模块独立成链）。
    pub fn register_type(&mut self, name: &[u8]) -> Result<u16, u16> {
        if name.is_empty() || name.len() > FS_NAME_MAX {
            return Err(E_NAME_BAD);
        }
        let h = UnregManager::hash(name);
        for t in self.types.iter() {
            if t.name_hash == h && t.name_len as usize == name.len() && t.life != TypeLife::Gone {
                return Err(E_STATE_BAD); // 重名视作状态冲突（注册由 F4025 面承担）
            }
        }
        self.types.push(FsTypeEntry {
            name_len: name.len() as u8,
            name_hash: h,
            life: TypeLife::Live,
            instances: Vec::new(),
            unreg_attempts: 0,
            busy_rej: 0,
        });
        Ok((self.types.len() - 1) as u16)
    }

    /// 查找：按名定位活类型槽。
    pub fn find_type(&self, name: &[u8]) -> Option<usize> {
        let h = UnregManager::hash(name);
        let mut i = 0usize;
        for t in self.types.iter() {
            if t.life != TypeLife::Gone && t.name_hash == h && t.name_len as usize == name.len() {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 创建 sb 实例：类型须 Live，实例计数 +1（判据 J4 正向）。
    pub fn new_instance(&mut self, type_slot: u16, magic: u32) -> Result<u16, u16> {
        if type_slot as usize >= self.types.len() {
            return Err(E_TYPE_MISS);
        }
        let ti = type_slot as usize;
        if self.types[ti].life != TypeLife::Live {
            return Err(E_STATE_BAD);
        }
        if self.insts.len() >= INST_MAX {
            return Err(E_BAL);
        }
        self.insts.push(SbInstance {
            type_id: type_slot,
            state: InstState::Live, // 创建即激活（挂载期内）
            sb_magic: magic,
        });
        self.created_insts += 1;
        let ii = self.insts.len() - 1;
        self.types[ti].instances.push(ii);
        Ok(ii as u16)
    }

    /// 杀死实例：重复 kill 拒，计数 -1，从类型项实例集合移除（判据 J3）。
    pub fn kill_instance(&mut self, inst: u16) -> Result<u32, u16> {
        if inst as usize >= self.insts.len() {
            return Err(E_INST_OOR);
        }
        let ii = inst as usize;
        if self.insts[ii].state == InstState::Killed {
            return Err(E_INST_DEAD);
        }
        let ti = self.insts[ii].type_id as usize;
        if ti >= self.types.len() {
            return Err(E_TYPE_MISS);
        }
        self.insts[ii].state = InstState::Killed;
        self.killed_insts += 1;
        // 从类型项集合中移除（按值查）。
        let mut pos: Option<usize> = None;
        let mut k = 0usize;
        for s in self.types[ti].instances.iter() {
            if *s == ii {
                pos = Some(k);
                break;
            }
            k += 1;
        }
        match pos {
            Some(p) => {
                self.types[ti].instances.remove(p);
            }
            None => return Err(E_BAL),
        }
        Ok(self.types[ti].live_count())
    }

    /// unregister_fs：在途实例非空返回 EBUSY（判据 J1）；归零后摘除（判据 J2）。
    pub fn unregister_fs(&mut self, type_slot: u16) -> Result<(), u16> {
        if type_slot as usize >= self.types.len() {
            return Err(E_TYPE_MISS);
        }
        let ti = type_slot as usize;
        if self.types[ti].life == TypeLife::Gone {
            return Err(E_TYPE_MISS);
        }
        self.types[ti].unreg_attempts += 1;
        let live = self.types[ti].live_count();
        if live != 0 {
            // 有在途：进入 Draining 并返回 EBUSY，计数账即返回给调用方。
            self.types[ti].life = TypeLife::Draining;
            self.types[ti].busy_rej += 1;
            self.unreg_busy += 1;
            return Err(EBUSY);
        }
        // 排空后：置 Gone 并清空集合。
        self.types[ti].life = TypeLife::Gone;
        self.types[ti].instances.clear();
        self.unreg_ok += 1;
        self.gone += 1;
        Ok(())
    }

    /// 在途计数账导出：某类型的存活实例数（判据 J1 的读面）。
    pub fn in_flight(&self, type_slot: u16) -> Result<u32, u16> {
        if type_slot as usize >= self.types.len() {
            return Err(E_TYPE_MISS);
        }
        Ok(self.types[type_slot as usize].live_count())
    }

    /// 账守恒：created - killed == Σ live_count（判据 J4 全文）。
    pub fn conserve(&self) -> bool {
        let mut sum = 0u32;
        for t in self.types.iter() {
            sum += t.live_count();
        }
        if self.created_insts as i64 - self.killed_insts as i64 != sum as i64 {
            return false;
        }
        // 每个实例槽在类型集合中出现至多一次。
        for (i, inst) in self.insts.iter().enumerate() {
            if inst.state == InstState::Killed {
                continue;
            }
            let ti = inst.type_id as usize;
            if ti >= self.types.len() {
                return false;
            }
            let mut seen = 0u32;
            for s in self.types[ti].instances.iter() {
                if *s == i {
                    seen += 1;
                }
            }
            if seen != 1 {
                return false;
            }
        }
        true
    }
}

/// 诊断码释义（覆盖段内码与 EBUSY）。
pub fn detail(code: u16) -> &'static str {
    match code {
        EBUSY => "EBUSY(16)：有在途 sb 实例，拒绝注销",
        E_TYPE_MISS => "类型项不存在（未注册/已摘除）",
        E_INST_DEAD => "重复 kill 或对已死实例操作",
        E_INST_OOR => "实例槽号越界",
        E_STATE_BAD => "类型状态迁移非法（重名/Draining 外挂载）",
        E_BAL => "实例账与表账不平",
        E_NAME_BAD => "名非法（空/超 15 字节）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：建类型 → 建实例 → EBUSY → 逐个 kill → 摘除 → 注入拦截。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut m = UnregManager::new();

    // R1：注册类型 ext4 并建 3 个 sb 实例，在途数 3（判据 J4 正向）。
    let t0 = m.register_type(b"ext4")?;
    let i0 = m.new_instance(t0, 0xEF53)?;
    let i1 = m.new_instance(t0, 0xEF53)?;
    let i2 = m.new_instance(t0, 0xEF53)?;
    if m.in_flight(t0)? != 3 || m.created_insts != 3 {
        return Err(E_BAL);
    }
    if !m.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：有在途实例时 unregister_fs 返回 EBUSY（判据 J1 原文）。
    if m.unregister_fs(t0) != Err(EBUSY) {
        return Err(EBUSY);
    }
    if m.unreg_busy != 1 || m.types[t0 as usize].busy_rej != 1 {
        return Err(E_BAL);
    }
    if m.types[t0 as usize].life != TypeLife::Draining {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R3：EBUSY 账 = 在途数（3）——重复尝试三次，每次都应 EBUSY 且计数不变。
    if m.unregister_fs(t0) != Err(EBUSY) {
        return Err(EBUSY);
    }
    if m.unregister_fs(t0) != Err(EBUSY) {
        return Err(EBUSY);
    }
    if m.in_flight(t0)? != 3 || m.unreg_busy != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：kill 一个实例 → 在途 2，账仍守恒（判据 J3 正向）。
    if m.kill_instance(i0)? != 2 {
        return Err(E_BAL);
    }
    if m.in_flight(t0)? != 2 || m.killed_insts != 1 {
        return Err(E_BAL);
    }
    if !m.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：仍有在途 → 仍 EBUSY（排空未完成不得摘除）。
    if m.unregister_fs(t0) != Err(EBUSY) {
        return Err(EBUSY);
    }
    hits += 1;

    // R6：kill 余下两实例 → 在途 0，注销成功（判据 J2 原文）。
    m.kill_instance(i1)?;
    m.kill_instance(i2)?;
    if m.in_flight(t0)? != 0 {
        return Err(E_BAL);
    }
    m.unregister_fs(t0)?;
    if m.types[t0 as usize].life != TypeLife::Gone {
        return Err(E_STATE_BAD);
    }
    if m.unreg_ok != 1 || m.gone != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R7：类型表项消失——按名查找不可得（判据 J2 后半）。
    if m.find_type(b"ext4").is_some() {
        return Err(E_TYPE_MISS);
    }
    if m.conserve() {
        // 全清后 created 3 == killed 3，Σlive 0，守恒应成立。
    } else {
        return Err(E_BAL);
    }
    hits += 1;

    // R8：另一类型 xfs 独立成链——注销不影响别的类型账。
    let t1 = m.register_type(b"xfs")?;
    let j0 = m.new_instance(t1, 0x5846_5342)?;
    if m.unregister_fs(t1) != Err(EBUSY) {
        return Err(EBUSY);
    }
    m.kill_instance(j0)?;
    m.unregister_fs(t1)?;
    if m.find_type(b"xfs").is_some() || m.gone != 2 {
        return Err(E_TYPE_MISS);
    }
    if !m.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：重复 kill 注入被拒（判据 J3 反向）。
    if m.kill_instance(i0) != Err(E_INST_DEAD) {
        return Err(E_INST_DEAD);
    }
    hits += 1;

    // I2：kill 未知实例注入被拒。
    if m.kill_instance(99) != Err(E_INST_OOR) {
        return Err(E_INST_OOR);
    }
    hits += 1;

    // I3：注销已摘除类型注入被拒。
    if m.unregister_fs(t0) != Err(E_TYPE_MISS) {
        return Err(E_TYPE_MISS);
    }
    hits += 1;

    // I4：对已摘除类型建实例注入被拒。
    if m.new_instance(t0, 1) != Err(E_STATE_BAD) {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // I5：空名注册注入被拒、超长名注入被拒。
    if m.register_type(b"") != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    let long = [b'q'; 16];
    if m.register_type(&long) != Err(E_NAME_BAD) {
        return Err(E_NAME_BAD);
    }
    hits += 1;

    Ok(hits)
}
