//! unx4129_openrename — UNX-F4129 open 文件与 rename/unlink 并存语义（诊断段 0x9200）。
//!
//! 判据原文：UNX-F4129-J1 打开后 rename 持续可读写（旧名消失新名可达，账目可验）；
//!           unlink 后打开句柄读写到底（POSIX「打开即活」语义断言），路径消失后
//!           open 返回 ENOENT。
//!   J2 rename 只搬目录项（links/refs 不动），同名 rename 为无操作。
//!   J3 无句柄时 unlink 立即回收，有句柄落 Orphan。
//! 设计纪律：
//!   1. 名字与对象解耦：目录项（DirEntry）只持有 ino，打开文件（Handle）也只见 ino。
//!      rename 只搬目录项、unlink 只摘目录项，二者都不触碰 inode 内容与句柄偏移，
//!      这是「打开即活」的结构性保证，而不是靠特判维持。
//!   2. inode 回收的唯一条件是 links==0 && refs==0：unlink 后 links 归零，但只要
//!      还有打开句柄（refs>0）就停在 Orphan 态，读写在 Orphan 态完全放行。
//!   3. 路径查不到一律 ENOENT(2)：rename 后的旧名、unlink 后的原名、从未存在的名，
//!      三种「查不到」同码同账，不靠 errno 细分泄漏历史。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x9200 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9200;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOENT: u16 = SEG_BASE + 1; // 对应 ENOENT(2)：路径不存在
const E_EXIST: u16 = SEG_BASE + 2; // 对应 EEXIST(17)：目标名已占用
const E_NOINODE: u16 = SEG_BASE + 3; // inode 表满或号非法
const E_BADHANDLE: u16 = SEG_BASE + 4; // 句柄不属于本命名空间或已回收
const E_ORPHAN_RW: u16 = SEG_BASE + 5; // Orphan 态被误拒（判据反向失守）
const E_ACCT: u16 = SEG_BASE + 6; // 目录项数/links/refs 三账不平
const E_NAME: u16 = SEG_BASE + 7; // 名字号越界
const E_RECLAIM: u16 = SEG_BASE + 8; // 回收条件不满足却回收

/// ENOENT 的 errno 原值（POSIX errno.h），判据按此值断言。
pub const ENOENT_ERRNO: u16 = 2;

/// 目录容量（单目录定长表，容量写死便于内存账）。
pub const DIR_CAP: usize = 8;
/// inode 容量（同目录上限，1:1 便于对账）。
pub const INO_CAP: usize = 8;
/// 名字号合法区间：1..=0x00FF（0 表空项）。
pub const NAME_MAX: u16 = 0x00FF;

/// 对象生命周期状态机：Alive（有名）→ Orphan（无名有句柄）→ Reclaimed（终态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DentryState {
    Alive,
    Orphan,
    Reclaimed,
}

/// 目录项：名字号 → ino。ino==0 表空槽（墓碑）。
#[derive(Clone, Copy)]
pub struct DirEntry {
    /// 名字号（1..=NAME_MAX）
    pub name: u16,
    /// 目标 inode 号（0 表空）
    pub ino: u32,
}

/// inode 记录：链接数与句柄引用数分离，是「打开即活」的两个独立账。
pub struct InodeRec {
    /// inode 号（0 表空槽）
    pub ino: u32,
    /// 目录项链接数（rename 不变、unlink 减一）
    pub links: u16,
    /// 打开句柄数（open 加一、close 减一）
    pub refs: u16,
    /// 逻辑字节数（写入累加）
    pub bytes: u32,
    /// 内容摘要（FNV-1a 滚动，用于读写到底的可验性）
    pub digest: u32,
    /// 生命周期状态
    pub state: DentryState,
    /// 累计写入次数
    pub writes: u32,
}

/// 打开句柄：只记 ino 与偏移，不记名字——名字变了句柄不受影响。
#[derive(Clone, Copy)]
pub struct Handle {
    /// 目标 inode 号
    pub ino: u32,
    /// 当前偏移
    pub pos: u32,
    /// 是否仍有效（close 后置 false）
    pub live: bool,
}

/// 单目录命名空间：目录项表 + inode 表 + 四路操作账。
pub struct Namespace {
    /// 目录项表（定长）
    pub entries: Vec<DirEntry>,
    /// inode 表（定长）
    pub inodes: Vec<InodeRec>,
    /// 创建次数
    pub created: u32,
    /// rename 次数
    pub renamed: u32,
    /// unlink 次数
    pub unlinked: u32,
    /// 回收次数
    pub reclaimed: u32,
    /// 打开次数
    pub opened: u32,
    /// 关闭次数
    pub closed: u32,
}

fn fnv_step(h: u32, b: u8) -> u32 {
    let mut x = h ^ (b as u32);
    x = x.wrapping_mul(0x0100_0193);
    x
}

impl Namespace {
    pub fn new() -> Namespace {
        let mut entries = Vec::new();
        let mut inodes = Vec::new();
        let mut i = 0usize;
        while i < DIR_CAP {
            entries.push(DirEntry { name: 0, ino: 0 });
            i += 1;
        }
        let mut j = 0usize;
        while j < INO_CAP {
            inodes.push(InodeRec {
                ino: 0,
                links: 0,
                refs: 0,
                bytes: 0,
                digest: 0x811C_9DC5,
                state: DentryState::Reclaimed,
                writes: 0,
            });
            j += 1;
        }
        Namespace {
            entries,
            inodes,
            created: 0,
            renamed: 0,
            unlinked: 0,
            reclaimed: 0,
            opened: 0,
            closed: 0,
        }
    }

    // ── 内部查表 ────────────────────────────────────────────
    fn entry_idx(&self, name: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].name == name && self.entries[i].ino != 0 {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    fn ino_idx(&self, ino: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.inodes.len() {
            if self.inodes[i].ino == ino && ino != 0 {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 路径解析：名字 → ino。查不到即 ENOENT（不区分「曾存在」与「从未存在」）。
    pub fn lookup(&self, name: u16) -> Result<u32, u16> {
        if name == 0 || name > NAME_MAX {
            return Err(E_NAME);
        }
        match self.entry_idx(name) {
            Some(i) => Ok(self.entries[i].ino),
            None => Err(E_NOENT),
        }
    }

    /// 创建：占空目录项 + 占空 inode，links=1、refs=0、状态 Alive。
    pub fn create(&mut self, name: u16) -> Result<u32, u16> {
        if name == 0 || name > NAME_MAX {
            return Err(E_NAME);
        }
        if self.entry_idx(name).is_some() {
            return Err(E_EXIST);
        }
        let mut slot = self.entries.len();
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].ino == 0 {
                slot = i;
                break;
            }
            i += 1;
        }
        if slot >= self.entries.len() {
            return Err(E_NOINODE);
        }
        let mut islot = self.inodes.len();
        let mut j = 0usize;
        while j < self.inodes.len() {
            if self.inodes[j].ino == 0 {
                islot = j;
                break;
            }
            j += 1;
        }
        if islot >= self.inodes.len() {
            return Err(E_NOINODE);
        }
        let ino = (islot as u32) + 1; // inode 号从 1 起，0 保留为空
        self.inodes[islot] = InodeRec {
            ino,
            links: 1,
            refs: 0,
            bytes: 0,
            digest: 0x811C_9DC5,
            state: DentryState::Alive,
            writes: 0,
        };
        self.entries[slot] = DirEntry { name, ino };
        self.created += 1;
        Ok(ino)
    }

    /// 打开：refs 加一，返回句柄（pos=0）。
    pub fn open(&mut self, name: u16) -> Result<Handle, u16> {
        let ino = self.lookup(name)?;
        let idx = match self.ino_idx(ino) {
            Some(i) => i,
            None => return Err(E_NOINODE),
        };
        if self.inodes[idx].state == DentryState::Reclaimed {
            return Err(E_NOENT);
        }
        self.inodes[idx].refs += 1;
        self.opened += 1;
        Ok(Handle {
            ino,
            pos: 0,
            live: true,
        })
    }

    /// 关闭：refs 减一；若 links==0 且 refs==0 则回收（Orphan → Reclaimed）。
    pub fn close(&mut self, h: &mut Handle) -> Result<(), u16> {
        if !h.live {
            return Err(E_BADHANDLE);
        }
        let idx = match self.ino_idx(h.ino) {
            Some(i) => i,
            None => return Err(E_BADHANDLE),
        };
        if self.inodes[idx].refs == 0 {
            return Err(E_ACCT); // 引用账穿底
        }
        self.inodes[idx].refs -= 1;
        h.live = false;
        self.closed += 1;
        if self.inodes[idx].links == 0 && self.inodes[idx].refs == 0 {
            self.inodes[idx].state = DentryState::Reclaimed;
            self.inodes[idx].ino = 0; // 槽位归还（先归零 ino，再计回收）
            self.reclaimed += 1;
        }
        Ok(())
    }

    /// 写：只认句柄，不看名字。Orphan 态照常放行（判据核心）。
    pub fn write(&mut self, h: &mut Handle, data: &[u8]) -> Result<u32, u16> {
        if !h.live {
            return Err(E_BADHANDLE);
        }
        let idx = match self.ino_idx(h.ino) {
            Some(i) => i,
            None => return Err(E_BADHANDLE),
        };
        if self.inodes[idx].state == DentryState::Reclaimed {
            return Err(E_RECLAIM);
        }
        if data.is_empty() {
            return Ok(0); // 空写合法但无账
        }
        let mut d = self.inodes[idx].digest;
        let mut k = 0usize;
        while k < data.len() {
            d = fnv_step(d, data[k]);
            k += 1;
        }
        self.inodes[idx].digest = d;
        self.inodes[idx].bytes += data.len() as u32;
        self.inodes[idx].writes += 1;
        h.pos += data.len() as u32;
        Ok(data.len() as u32)
    }

    /// 读：返回逻辑字节数与摘要快照（不搬数据，只给可对账的两个量）。
    pub fn read_meta(&self, h: &Handle) -> Result<(u32, u32), u16> {
        if !h.live {
            return Err(E_BADHANDLE);
        }
        let idx = match self.ino_idx(h.ino) {
            Some(i) => i,
            None => return Err(E_BADHANDLE),
        };
        Ok((self.inodes[idx].bytes, self.inodes[idx].digest))
    }

    /// rename：搬目录项，旧名摘除、新名挂上；inode 的 links/refs 一律不动。
    pub fn rename(&mut self, old: u16, new: u16) -> Result<(), u16> {
        if new == 0 || new > NAME_MAX {
            return Err(E_NAME);
        }
        if old == new {
            let _ = self.lookup(old)?;
            return Ok(()); // POSIX：同名 rename 为无操作
        }
        let oi = match self.entry_idx(old) {
            Some(i) => i,
            None => return Err(E_NOENT),
        };
        // 目标名若已被占用：POSIX 允许覆盖，本域取「拒收」保守语义
        if self.entry_idx(new).is_some() {
            return Err(E_EXIST);
        }
        let ino = self.entries[oi].ino;
        self.entries[oi] = DirEntry { name: 0, ino: 0 };
        // 找空槽挂新名
        let mut slot = self.entries.len();
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].ino == 0 {
                slot = i;
                break;
            }
            i += 1;
        }
        if slot >= self.entries.len() {
            // 回滚：把旧名挂回去，保持目录项账不灭失
            self.entries[oi] = DirEntry { name: old, ino };
            return Err(E_NOINODE);
        }
        self.entries[slot] = DirEntry { name: new, ino };
        self.renamed += 1;
        Ok(())
    }

    /// unlink：摘目录项，links 减一；有打开句柄则落 Orphan（不回收）。
    pub fn unlink(&mut self, name: u16) -> Result<(), u16> {
        let ei = match self.entry_idx(name) {
            Some(i) => i,
            None => return Err(E_NOENT),
        };
        let ino = self.entries[ei].ino;
        let idx = match self.ino_idx(ino) {
            Some(i) => i,
            None => return Err(E_NOINODE),
        };
        if self.inodes[idx].links == 0 {
            return Err(E_ACCT);
        }
        self.entries[ei] = DirEntry { name: 0, ino: 0 };
        self.inodes[idx].links -= 1;
        self.unlinked += 1;
        if self.inodes[idx].links == 0 {
            if self.inodes[idx].refs == 0 {
                self.inodes[idx].state = DentryState::Reclaimed;
                self.inodes[idx].ino = 0;
                self.reclaimed += 1;
            } else {
                self.inodes[idx].state = DentryState::Orphan;
            }
        }
        Ok(())
    }

    /// 三账对账：目录项非空数 == Σlinks；打开句柄数 == Σrefs。
    pub fn audit(&self) -> bool {
        let mut live_entries = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].ino != 0 {
                live_entries += 1;
            }
            i += 1;
        }
        let mut links = 0u32;
        let mut refs = 0u32;
        let mut j = 0usize;
        while j < self.inodes.len() {
            let r = &self.inodes[j];
            if r.ino != 0 {
                links += r.links as u32;
                refs += r.refs as u32;
                if r.state == DentryState::Reclaimed {
                    return false; // 有号却已回收：状态与号不一致
                }
                if r.links == 0 && r.state == DentryState::Alive {
                    return false; // 无名却 Alive：状态漏迁
                }
                if r.links > 0 && r.state != DentryState::Alive {
                    return false;
                }
            }
            j += 1;
        }
        live_entries == links && self.opened - self.closed == refs
    }

    /// 孤儿清点：links==0 且 refs>0 的对象数（unlink 后仍被持有的存量）。
    pub fn orphan_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.inodes.len() {
            if self.inodes[i].ino != 0 && self.inodes[i].state == DentryState::Orphan {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOENT => "路径不存在（ENOENT 2）",
        E_EXIST => "目标名已被占用（EEXIST 17）",
        E_NOINODE => "inode 表满或号非法",
        E_BADHANDLE => "句柄非法或已关闭",
        E_ORPHAN_RW => "Orphan 态读写被误拒",
        E_ACCT => "目录项/links/refs 三账不平",
        E_NAME => "名字号越界（0 或 >0xFF）",
        E_RECLAIM => "回收条件不满足却回收",
        _ => "未知诊断码",
    }
}

/// 确定性演练：rename 并存 → unlink 并存 → 旧名 ENOENT → 三账对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：创建 a、打开、写 5 字节。
    let mut ns = Namespace::new();
    let ino = ns.create(0x61)?; // 'a'
    let mut h = ns.open(0x61)?;
    if ns.write(&mut h, &[1u8, 2, 3, 4, 5])? != 5 {
        return Err(E_ACCT);
    }
    if ns.read_meta(&h)?.0 != 5 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R2：rename a→b：旧名消失、新名可达、句柄仍旧可读写（判据主断言）。
    let digest_before = ns.read_meta(&h)?.1;
    ns.rename(0x61, 0x62)?; // 'b'
    if ns.lookup(0x61) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if ns.lookup(0x62)? != ino {
        return Err(E_ACCT);
    }
    if ns.read_meta(&h)? != (5, digest_before) {
        return Err(E_ACCT); // 内容与摘要不因改名而变
    }
    if ns.write(&mut h, &[6u8, 7])? != 2 {
        return Err(E_ACCT); // 改名后可继续写
    }
    if ns.read_meta(&h)?.0 != 7 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：旧名 open → ENOENT；新名 open → 成功（第二个句柄 refs=2）。
    match ns.open(0x61) {
        Err(E_NOENT) => {}
        _ => return Err(E_NOENT),
    }
    let mut h2 = ns.open(0x62)?;
    if ns.inodes[0].refs != 2 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：unlink b：目录项摘除、links 归零、状态 Orphan，句柄读写到底。
    ns.unlink(0x62)?;
    if ns.lookup(0x62) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    if ns.orphan_count() != 1 {
        return Err(E_ACCT);
    }
    {
        let r = match ns.inodes.get(0) {
            Some(x) => x,
            None => return Err(E_NOINODE),
        };
        if r.state != DentryState::Orphan || r.links != 0 || r.refs != 2 {
            return Err(E_ACCT);
        }
    }
    // 两个句柄都还能写、还能读元（POSIX「打开即活」）
    if ns.write(&mut h, &[8u8])? != 1 || ns.write(&mut h2, &[9u8, 10])? != 2 {
        return Err(E_ORPHAN_RW);
    }
    if ns.read_meta(&h)?.0 != 10 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R5：路径消失后 open 返回 ENOENT（不是 EBADF，也不是 EACCES）。
    match ns.open(0x62) {
        Err(E_NOENT) => {}
        _ => return Err(E_NOENT),
    }
    hits += 1;

    // R6：close 两个句柄 → refs 归零 → 立即回收，槽位归还。
    ns.close(&mut h)?;
    if ns.reclaimed != 0 {
        return Err(E_RECLAIM); // 还有一个句柄，不得回收
    }
    ns.close(&mut h2)?;
    if ns.reclaimed != 1 {
        return Err(E_RECLAIM);
    }
    if ns.orphan_count() != 0 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：use-after-close → E_BADHANDLE（句柄不复活）。
    if ns.write(&mut h, &[1u8]) != Err(E_BADHANDLE) {
        return Err(E_BADHANDLE);
    }
    if ns.read_meta(&h) != Err(E_BADHANDLE) {
        return Err(E_BADHANDLE);
    }
    hits += 1;

    // I2：重名创建 → E_EXIST；名字越界 → E_NAME。
    ns.create(0x63)?;
    if ns.create(0x63) != Err(E_EXIST) {
        return Err(E_EXIST);
    }
    if ns.create(0) != Err(E_NAME) || ns.lookup(NAME_MAX + 1) != Err(E_NAME) {
        return Err(E_NAME);
    }
    hits += 1;

    // I3：rename 源不存在 → ENOENT；目标已占用 → E_EXIST（保守拒收）。
    if ns.rename(0x7A, 0x7B) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    ns.create(0x64)?;
    if ns.rename(0x63, 0x64) != Err(E_EXIST) {
        return Err(E_EXIST);
    }
    hits += 1;

    // I4：unlink 不存在名 → ENOENT；重复 unlink 同码（历史不泄漏）。
    if ns.unlink(0x7A) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    ns.unlink(0x63)?;
    if ns.unlink(0x63) != Err(E_NOENT) {
        return Err(E_NOENT);
    }
    hits += 1;

    // I5：无句柄时 unlink 立即回收（links==0 && refs==0）。
    let before = ns.reclaimed;
    ns.unlink(0x64)?;
    if ns.reclaimed != before + 1 {
        return Err(E_RECLAIM);
    }
    hits += 1;

    // C1：三账对账 + 四路计次（create 3、rename 1、unlink 3、reclaim 2）。
    if !ns.audit() {
        return Err(E_ACCT);
    }
    if ns.created != 3 || ns.renamed != 1 || ns.unlinked != 3 {
        return Err(E_ACCT);
    }
    if ns.reclaimed != 2 {
        return Err(E_ACCT);
    }
    if ns.opened != 2 || ns.closed != 2 {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
