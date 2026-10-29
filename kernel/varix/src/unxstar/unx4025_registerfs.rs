//! unx4025_registerfs — UNX-F4025 register_fs 注册接口（B2–B5 入场契约）（诊断段 0x85A0）。
//!
//! 判据原文：
//!   J1 register_fs 对合法类型注册成功且可立即被 mount 引用：注册返回槽号，
//!      同一次调用后 mount_by_name 立即解析成功。
//!   J2 对重名注入返回 EEXIST 3/3 次正确。
//!   J3 对空钩子（get_sb/kill_sb 任一为 0）注入返回 EINVAL 3/3 次正确。
//!   J4 对超限注入返回 ENOSPC 3/3 次正确，且超限后表长不变、账目守恒。
//! 设计纪律：
//!   1. 注册接口的返回码遵循 errno 语义（EEXIST=17 / EINVAL=22 / ENOSPC=28），
//!      内部另有 0x85A0 段诊断码用于账目类错误；detail() 同时覆盖两族。
//!   2. 校验顺序固定：空钩子(EINVAL) → 重名(EEXIST) → 容量(ENOSPC)，
//!      顺序固定才能让三类注入各 3/3 稳定复现（判据 J2-J4）。
//!   3. 注册状态机：Vacant → Filling → Ready；Ready 后立即可被 mount 解析，
//!      Vacant 槽不得被解析命中。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x85A0;

// ── 诊断码闭集（段内账目类） ─────────────────────────────────
const E_SLOT_OOR: u16 = SEG_BASE + 1;  // 槽号越界（索引前未比长度）
const E_NOT_READY: u16 = SEG_BASE + 2; // 槽未就绪即被 mount 解析
const E_STATE_BAD: u16 = SEG_BASE + 3; // 注册状态迁移非法
const E_BAL: u16 = SEG_BASE + 4;       // 注册账与表长不平
const E_DEV_MISS: u16 = SEG_BASE + 5;  // 需块设备但无设备槽
const E_REF_BAD: u16 = SEG_BASE + 6;   // 在途引用账非法

/// 对外 errno 返回码（判据 J2-J4 要求的三类）。
pub const EEXIST: u16 = 17;  // 类型重名
pub const EINVAL: u16 = 22;  // 钩子为空/参数非法
pub const ENOSPC: u16 = 28;  // 类型表满

/// 类型表容量（超限即 ENOSPC）。
pub const FS_SLOTS: usize = 32;
/// fs 名长度上限。
pub const FS_NAME_MAX: usize = 15;

/// 注册槽状态机。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegState {
    Vacant = 0, // 空槽
    Filling = 1, // 填写中
    Ready = 2,   // 就绪（可被 mount 解析）
    Gone = 3,    // 已注销
}

/// 注册请求（register_fs 的入参投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FsRequest {
    pub name_len: u8,   // 名长度
    pub name_hash: u32, // 名哈希
    pub get_sb: u16,    // 挂载钩子
    pub kill_sb: u16,   // 卸载钩子
    pub needs_dev: bool, // 是否需块设备
}

/// 注册槽。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FsSlot {
    pub name_len: u8,   // 名长度
    pub name_hash: u32, // 名哈希
    pub get_sb: u16,    // 挂载钩子
    pub kill_sb: u16,   // 卸载钩子
    pub needs_dev: bool, // 需设备标记
    pub state: RegState, // 槽状态
    pub mounts: u32,    // 在途 mount 引用数
}

/// 注册表：固定容量槽表 + 注册账。
pub struct RegisterTable {
    pub slots: Vec<FsSlot>, // 槽表（紧凑存储）
    pub attempts: u32,      // 注册尝试账
    pub ok: u32,            // 注册成功账
    pub inval: u32,         // EINVAL 拒绝账
    pub exist: u32,         // EEXIST 拒绝账
    pub nospc: u32,         // ENOSPC 拒绝账
    pub mount_refs: u32,    // 累计 mount 解析成功数
}

impl RegisterTable {
    pub fn new() -> RegisterTable {
        RegisterTable {
            slots: Vec::new(),
            attempts: 0,
            ok: 0,
            inval: 0,
            exist: 0,
            nospc: 0,
            mount_refs: 0,
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

    /// 请求构造：名非法直接 EINVAL（入口即拒）。
    pub fn request(name: &[u8], get_sb: u16, kill_sb: u16, needs_dev: bool) -> Result<FsRequest, u16> {
        if name.is_empty() || name.len() > FS_NAME_MAX {
            return Err(EINVAL);
        }
        Ok(FsRequest {
            name_len: name.len() as u8,
            name_hash: RegisterTable::hash(name),
            get_sb,
            kill_sb,
            needs_dev,
        })
    }

    /// register_fs：固定校验顺序（EINVAL → EEXIST → ENOSPC），判据 J1-J4。
    pub fn register_fs(&mut self, req: FsRequest) -> Result<u16, u16> {
        self.attempts += 1;
        // ① 空钩子：EINVAL（判据 J3）。
        if req.get_sb == 0 || req.kill_sb == 0 || req.name_len == 0 {
            self.inval += 1;
            return Err(EINVAL);
        }
        // ② 重名：EEXIST（判据 J2）。
        for s in self.slots.iter() {
            if s.state == RegState::Ready
                && s.name_hash == req.name_hash
                && s.name_len == req.name_len
            {
                self.exist += 1;
                return Err(EEXIST);
            }
        }
        // ③ 容量：ENOSPC（判据 J4）。统计 Ready + Gone 占位，Gone 可复用前先判满。
        let mut occupied = 0usize;
        for s in self.slots.iter() {
            if s.state == RegState::Ready {
                occupied += 1;
            }
        }
        if occupied >= FS_SLOTS {
            self.nospc += 1;
            return Err(ENOSPC);
        }
        // 就位：Vacant → Filling → Ready（同一调用内完成）。
        let idx = match self.slots.iter().position(|s| s.state == RegState::Vacant) {
            Some(i) => i,
            None => {
                self.slots.push(FsSlot {
                    name_len: req.name_len,
                    name_hash: req.name_hash,
                    get_sb: req.get_sb,
                    kill_sb: req.kill_sb,
                    needs_dev: req.needs_dev,
                    state: RegState::Vacant,
                    mounts: 0,
                });
                self.slots.len() - 1
            }
        };
        self.slots[idx].state = RegState::Filling;
        self.slots[idx].name_len = req.name_len;
        self.slots[idx].name_hash = req.name_hash;
        self.slots[idx].get_sb = req.get_sb;
        self.slots[idx].kill_sb = req.kill_sb;
        self.slots[idx].needs_dev = req.needs_dev;
        self.slots[idx].mounts = 0;
        self.slots[idx].state = RegState::Ready;
        self.ok += 1;
        Ok(idx as u16)
    }

    /// mount 解析：注册成功后可立即解析（判据 J1 的「立即被引用」）。
    pub fn mount_by_name(&mut self, name: &[u8]) -> Result<u16, u16> {
        let h = RegisterTable::hash(name);
        let l = name.len();
        let mut i = 0usize;
        for s in self.slots.iter() {
            if s.state == RegState::Ready && s.name_hash == h && s.name_len as usize == l {
                self.mount_refs += 1;
                self.slots[i].mounts += 1;
                return Ok(i as u16);
            }
            i += 1;
        }
        Err(E_SLOT_OOR)
    }

    /// 释放 mount 引用：减到 0 才允许注销（在途引用排空的契约前置）。
    pub fn put_mount(&mut self, slot: u16) -> Result<u32, u16> {
        if slot as usize >= self.slots.len() {
            return Err(E_SLOT_OOR);
        }
        let i = slot as usize;
        if self.slots[i].mounts == 0 {
            return Err(E_REF_BAD);
        }
        self.slots[i].mounts -= 1;
        Ok(self.slots[i].mounts)
    }

    /// 注销：在途引用非 0 返回 E_BUSY 语义（段内码），否则置 Gone。
    pub fn unregister(&mut self, slot: u16) -> Result<(), u16> {
        if slot as usize >= self.slots.len() {
            return Err(E_SLOT_OOR);
        }
        let i = slot as usize;
        if self.slots[i].state != RegState::Ready {
            return Err(E_STATE_BAD);
        }
        if self.slots[i].mounts != 0 {
            return Err(E_REF_BAD);
        }
        self.slots[i].state = RegState::Gone;
        Ok(())
    }

    /// 就绪槽数。
    pub fn ready_count(&self) -> u32 {
        let mut c = 0u32;
        for s in self.slots.iter() {
            if s.state == RegState::Ready {
                c += 1;
            }
        }
        c
    }

    /// 注册账守恒：attempts == ok + inval + exist + nospc（判据 J4 的账目面）。
    pub fn conserve(&self) -> bool {
        self.attempts == self.ok + self.inval + self.exist + self.nospc
            && self.ready_count() == self.ok
    }
}

/// 诊断码释义（覆盖段内码与 errno 三类）。
pub fn detail(code: u16) -> &'static str {
    match code {
        EEXIST => "EEXIST(17)：fs 类型重名",
        EINVAL => "EINVAL(22)：勾子为空或名非法",
        ENOSPC => "ENOSPC(28)：类型表已满",
        E_SLOT_OOR => "槽号越界或名未注册",
        E_NOT_READY => "槽未就绪即被 mount 解析",
        E_STATE_BAD => "注册状态迁移非法",
        E_BAL => "注册账与表长不平",
        E_DEV_MISS => "需块设备但无设备槽",
        E_REF_BAD => "在途引用非零或引用减为负",
        _ => "未知诊断码",
    }
}

/// 确定性演练：合法注册即挂 → 三类注入各 3/3 → 超限 → 账守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut t = RegisterTable::new();

    // R1：合法类型注册成功并返回槽号（判据 J1）。
    let req = RegisterTable::request(b"ext4", 11, 12, true)?;
    let s0 = t.register_fs(req)?;
    if s0 != 0 || t.ok != 1 || t.ready_count() != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2：注册后同一状态下 mount_by_name 立即解析成功（判据 J1 后半）。
    let m = t.mount_by_name(b"ext4")?;
    if m != s0 {
        return Err(E_SLOT_OOR);
    }
    if t.slots[0].mounts != 1 || t.mount_refs != 1 {
        return Err(E_REF_BAD);
    }
    hits += 1;

    // R3：注册第二、第三类型（xfs/tmpfs），解析各自命中。
    t.register_fs(RegisterTable::request(b"xfs", 21, 22, true)?)?;
    t.register_fs(RegisterTable::request(b"tmpfs", 31, 32, false)?)?;
    if t.ready_count() != 3 {
        return Err(E_BAL);
    }
    if t.mount_by_name(b"tmpfs")? != 2 {
        return Err(E_SLOT_OOR);
    }
    hits += 1;

    // R4：重名注入 3/3 返回 EEXIST（判据 J2 原文）。
    let mut exist_hits = 0u32;
    for _ in 0..3u32 {
        let r = RegisterTable::request(b"ext4", 99, 98, true)?;
        if t.register_fs(r) == Err(EEXIST) {
            exist_hits += 1;
        }
    }
    if exist_hits != 3 || t.exist != 3 {
        return Err(EEXIST);
    }
    hits += 1;

    // R5：空钩子注入 3/3 返回 EINVAL（判据 J3 原文）——两向 + 名空。
    let bads = [
        FsRequest { name_len: 3, name_hash: RegisterTable::hash(b"bad"), get_sb: 0, kill_sb: 1, needs_dev: false },
        FsRequest { name_len: 3, name_hash: RegisterTable::hash(b"ba2"), get_sb: 1, kill_sb: 0, needs_dev: false },
        FsRequest { name_len: 0, name_hash: 0, get_sb: 1, kill_sb: 1, needs_dev: false },
    ];
    let mut inval_hits = 0u32;
    for b in bads.iter() {
        if t.register_fs(*b) == Err(EINVAL) {
            inval_hits += 1;
        }
    }
    if inval_hits != 3 || t.inval != 3 {
        return Err(EINVAL);
    }
    hits += 1;

    // R6：填满至 32 个 Ready，再注入 3 次 → ENOSPC 3/3（判据 J4 原文）。
    let mut n = t.ready_count();
    while n < FS_SLOTS as u32 {
        // 名字用 2 字节唯一串：'a'+高 4 位 / 'a'+低 4 位。
        let name = [b'f', b'a' + (n % 26) as u8, b'a' + (n / 26) as u8];
        let req = RegisterTable::request(&name[..3], 1, 2, false)?;
        // 极端情况下名字碰撞：碰撞时换名重试一次。
        match t.register_fs(req) {
            Ok(_) => n += 1,
            Err(EEXIST) => {
                let name2 = [b'g', b'a' + (n % 25) as u8, b'a' + (n / 25) as u8];
                t.register_fs(RegisterTable::request(&name2[..3], 3, 4, false)?)?;
                n += 1;
            }
            Err(e) => return Err(e),
        }
    }
    if t.ready_count() != FS_SLOTS as u32 {
        return Err(E_BAL);
    }
    hits += 1;

    // R7：超限注入 3/3 → ENOSPC（判据 J4 原文），表长不变。
    let mut nospc_hits = 0u32;
    for k in 0..3u32 {
        let name = [b'z', b'a' + k as u8];
        let req = RegisterTable::request(&name[..2], 7, 8, false)?;
        if t.register_fs(req) == Err(ENOSPC) {
            nospc_hits += 1;
        }
    }
    if nospc_hits != 3 || t.nospc != 3 {
        return Err(ENOSPC);
    }
    if t.ready_count() != FS_SLOTS as u32 {
        return Err(E_BAL);
    }
    hits += 1;

    // R8：注册账守恒——attempts == ok + inval + exist + nospc。
    if !t.conserve() {
        return Err(E_BAL);
    }
    if t.attempts != 3 + 3 + 3 + (FS_SLOTS as u32 - 3) + 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // I1：注销有在途引用 → E_REF_BAD（排空前不得消失）。
    if t.unregister(0) != Err(E_REF_BAD) {
        return Err(E_REF_BAD);
    }
    t.put_mount(0)?;
    if t.slots[0].mounts != 0 {
        return Err(E_REF_BAD);
    }
    t.unregister(0)?;
    if t.slots[0].state != RegState::Gone {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // I2：注销后解析失败、重复注销被拒。
    if t.mount_by_name(b"ext4") != Err(E_SLOT_OOR) {
        return Err(E_SLOT_OOR);
    }
    if t.unregister(0) != Err(E_STATE_BAD) {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // I3：引用减到 0 后再减被拒。
    if t.put_mount(1) != Err(E_REF_BAD) {
        return Err(E_REF_BAD);
    }
    hits += 1;

    // I4：未注册名的解析注入被拒。
    if t.mount_by_name(b"nope") != Err(E_SLOT_OOR) {
        return Err(E_SLOT_OOR);
    }
    hits += 1;

    Ok(hits)
}
