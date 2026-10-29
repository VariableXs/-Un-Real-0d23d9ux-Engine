//! unx4088_drename — UNX-F4088 重命名时的 dentry 迁移语义锚（诊断段 0x8D20）。
//!
//! 判据原文：UNX-F4088-J1 同名同父改名后哈希表项原子迁移（迁移期间并发查找 1,000 次
//!           零丢失零重复），跨父迁移更新双向链账零差。
//! 可验性质（本模块自证面）：
//!   J1 迁移三相位（LookupOld → InsertNew → UnlinkOld）逐步推进，任一步骤乱序调用
//!      必须拒（E_PHASE）；整个过程表中条目总数恒不变（无丢失）；
//!   J2 迁移窗口内（InsertNew 之后、UnlinkOld 之前）查找 1,000 次：新名必须全部
//!      命中同一个 dentry，旧名也必须仍可命中（同一 dentry），且无同名重复项
//!      （零丢失、零重复）；
//!   J3 迁移完成后旧名彻底消失、新名唯一命中，代次（gen）按序递增；
//!   J4 跨父迁移：旧父的 nchild -1、新父的 nchild +1，两向链账零差
//!      （父 → 子表、子 → 父指针同时更新），父未变时两侧账不动；
//!   J5 非法迁移拒：源项不存在（E_NO_ENTRY）、目标名已存在（E_TARGET_EXISTS）、
//!      跨父到不存在的父（E_NO_DIR）、新名空（E_NAME_EMPTY），拒收不改任何账。
//! 设计纪律：
//!   1. 迁移必须"先立后破"：先插入新名，再摘旧名——反过来会出现"两个名字都不命中"
//!      的窗口，这就是判据里"零丢失"要防的事。
//!   2. 相位机是显式状态，不是布尔：Begin 只做校验与建上下文，不改表；InsertNew
//!      改表；UnlinkOld 改表并收尾。每步都检查当前相位，乱序即拒。
//!   3. 目录子表与 dentry 表的双账必须同步：本模块用 DirRec.nchild 作计数器账，
//!      与表中的实际条目数在 commit 后逐父核对，差值非零即 E_BAL。
//!   4. 目标名已存在时不允许覆盖（rename 覆盖语义属于 link/unlink 面，本模块只管
//!      dentry 迁移），故一律 E_TARGET_EXISTS，由上层决定先 unlink。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//!
//! 诊断段 0x8D20 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8D20;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NO_ENTRY: u16 = SEG_BASE + 1; // 源目录项不存在
const E_TARGET_EXISTS: u16 = SEG_BASE + 2; // 目标名已存在（不覆盖）
const E_NO_DIR: u16 = SEG_BASE + 3; // 目标父目录不存在
const E_NAME_EMPTY: u16 = SEG_BASE + 4; // 新名/旧名为空
const E_PHASE: u16 = SEG_BASE + 5; // 迁移相位乱序
const E_BAL: u16 = SEG_BASE + 6; // 父链账不平（nchild 与实条目不符）
const E_DUP: u16 = SEG_BASE + 7; // 迁移后出现重复键
const E_NAME_LONG: u16 = SEG_BASE + 8; // 名字超长（NAME_MAX=255）

/// 名字长度上限（Linux NAME_MAX）。
pub const NAME_MAX: usize = 255;

/// 迁移相位机：Idle → LookupOld → InsertNew → UnlinkOld → Done。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenamePhase {
    Idle = 0,      // 空闲（可开新迁移）
    LookupOld = 1, // 已定位源项，尚未改表
    InsertNew = 2, // 已插入新名（旧名仍在：双名窗口）
    UnlinkOld = 3, // 已摘旧名，待收尾
    Done = 4,      // 完成
}

/// 迁移步骤（drill 用于乱序注入）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenameStep {
    Lookup = 0,  // 定位源项
    Insert = 1,  // 插入新名
    Unlink = 2,  // 摘除旧名
    Commit = 3,  // 收尾对账
}

/// 目录项表项：父 + 名 + dentry 号 + 代次。
pub struct DentPair {
    pub parent: u64,   // 父目录 ino
    pub name: Vec<u8>, // 名字字节
    pub dentry: u64,   // 指向的 dentry 标识
    pub gen: u32,      // 该目录项代次（迁移后 +1）
}

/// 目录计数器：父 → 子项数（双向链账的账本侧）。
pub struct DirRec {
    pub id: u64,      // 目录 ino
    pub nchild: u32,  // 账面子项数（必须等于表中实条目数）
}

/// 迁移上下文：一次迁移的全部参数与相位。
pub struct RenameCtx {
    pub old_parent: u64, // 源父
    pub new_parent: u64, // 目标父
    pub old_name: Vec<u8>, // 旧名
    pub new_name: Vec<u8>, // 新名
    pub dentry: u64,     // 被迁移的 dentry
    pub phase: RenamePhase, // 当前相位
    pub gen_out: u32,    // 迁移后应写回的代次
}

/// 迁移枢纽：表 + 目录账 + 统计。
pub struct RenameMachine {
    pub table: Vec<DentPair>, // 目录项表
    pub dirs: Vec<DirRec>,    // 目录计数账
    pub ctx: Option<RenameCtx>, // 在途迁移（同一时刻只允许一个）
    pub migrated: u32,        // 完成迁移数
    pub lookups_during: u32,  // 窗口期查找次数
    pub phase_rejects: u32,   // 相位乱序拒收
    pub dup_seen: u32,        // 检测到的重复键
}

impl RenameMachine {
    pub fn new() -> RenameMachine {
        RenameMachine {
            table: Vec::new(),
            dirs: Vec::new(),
            ctx: None,
            migrated: 0,
            lookups_during: 0,
            phase_rejects: 0,
            dup_seen: 0,
        }
    }

    /// 建目录：id 唯一。
    pub fn add_dir(&mut self, id: u64) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.dirs.len() {
            if self.dirs[i].id == id { return Err(E_NO_DIR); }
            i += 1;
        }
        self.dirs.push(DirRec { id, nchild: 0 });
        Ok(())
    }

    /// 目录下标。
    fn dir_idx(&self, id: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.dirs.len() {
            if self.dirs[i].id == id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 表内定位（父, 名）。
    pub fn find(&self, parent: u64, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.table.len() {
            if self.table[i].parent == parent && bytes_eq(&self.table[i].name, name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 建条目：名字非空/非超长、父必须已建、同键唯一。
    pub fn add_entry(
        &mut self,
        parent: u64,
        name: &[u8],
        dentry: u64,
    ) -> Result<(), u16> {
        if name.is_empty() { return Err(E_NAME_EMPTY); }
        if name.len() > NAME_MAX { return Err(E_NAME_LONG); }
        let di = self.dir_idx(parent).ok_or(E_NO_DIR)?;
        if self.find(parent, name).is_some() { return Err(E_TARGET_EXISTS); }
        self.table.push(DentPair {
            parent,
            name: copy(name),
            dentry,
            gen: 0,
        });
        self.dirs[di].nchild += 1;
        Ok(())
    }

    /// 开迁移：只做校验与建上下文，不改表（相位 Idle → LookupOld）。
    pub fn begin(
        &mut self,
        old_parent: u64,
        old_name: &[u8],
        new_parent: u64,
        new_name: &[u8],
    ) -> Result<(), u16> {
        if self.ctx.is_some() {
            self.phase_rejects += 1;
            return Err(E_PHASE); // 已有一个在途迁移
        }
        if old_name.is_empty() || new_name.is_empty() { return Err(E_NAME_EMPTY); }
        if new_name.len() > NAME_MAX { return Err(E_NAME_LONG); }
        let si = self.find(old_parent, old_name).ok_or(E_NO_ENTRY)?;
        if self.find(new_parent, new_name).is_some() {
            return Err(E_TARGET_EXISTS); // 不覆盖
        }
        if self.dir_idx(new_parent).is_none() { return Err(E_NO_DIR); }
        let dentry = self.table[si].dentry;
        let gen_out = self.table[si].gen + 1;
        self.ctx = Some(RenameCtx {
            old_parent,
            new_parent,
            old_name: copy(old_name),
            new_name: copy(new_name),
            dentry,
            phase: RenamePhase::LookupOld,
            gen_out,
        });
        Ok(())
    }

    /// 步骤 1：插入新名（先立）。相位必须 LookupOld。
    pub fn step_insert(&mut self) -> Result<(), u16> {
        let ph = match &self.ctx {
            Some(c) => c.phase,
            None => {
                self.phase_rejects += 1;
                return Err(E_PHASE);
            }
        };
        if ph != RenamePhase::LookupOld {
            self.phase_rejects += 1;
            return Err(E_PHASE);
        }
        let (np, nn, d, g) = {
            let c = match &self.ctx {
                Some(x) => x,
                None => return Err(E_PHASE),
            };
            (c.new_parent, copy(&c.new_name), c.dentry, c.gen_out)
        };
        let di = self.dir_idx(np).ok_or(E_NO_DIR)?;
        if self.find(np, &nn).is_some() { return Err(E_TARGET_EXISTS); }
        self.table.push(DentPair {
            parent: np,
            name: nn,
            dentry: d,
            gen: g,
        });
        self.dirs[di].nchild += 1;
        if let Some(c) = &mut self.ctx {
            c.phase = RenamePhase::InsertNew;
        }
        Ok(())
    }

    /// 步骤 2：摘旧名（后破）。相位必须 InsertNew。摘后旧父 nchild -1。
    pub fn step_unlink(&mut self) -> Result<(), u16> {
        let ph = match &self.ctx {
            Some(c) => c.phase,
            None => {
                self.phase_rejects += 1;
                return Err(E_PHASE);
            }
        };
        if ph != RenamePhase::InsertNew {
            self.phase_rejects += 1;
            return Err(E_PHASE);
        }
        let (op, on) = {
            let c = match &self.ctx {
                Some(x) => x,
                None => return Err(E_PHASE),
            };
            (c.old_parent, copy(&c.old_name))
        };
        let si = self.find(op, &on).ok_or(E_NO_ENTRY)?;
        let di = self.dir_idx(op).ok_or(E_NO_DIR)?;
        self.table.remove(si);
        if self.dirs[di].nchild > 0 {
            self.dirs[di].nchild -= 1;
        } else {
            return Err(E_BAL);
        }
        if let Some(c) = &mut self.ctx {
            c.phase = RenamePhase::UnlinkOld;
        }
        Ok(())
    }

    /// 步骤 3：收尾对账。相位必须 UnlinkOld；核对目标唯一、旧名消失、父账相符。
    pub fn commit(&mut self) -> Result<u32, u16> {
        let ph = match &self.ctx {
            Some(c) => c.phase,
            None => {
                self.phase_rejects += 1;
                return Err(E_PHASE);
            }
        };
        if ph != RenamePhase::UnlinkOld {
            self.phase_rejects += 1;
            return Err(E_PHASE);
        }
        let (np, nn, op, on) = {
            let c = match &self.ctx {
                Some(x) => x,
                None => return Err(E_PHASE),
            };
            (c.new_parent, copy(&c.new_name), c.old_parent, copy(&c.old_name))
        };
        // 新名必须恰好命中一条，旧名必须彻底消失
        if self.find(np, &nn).is_none() { return Err(E_BAL); }
        if self.find(op, &on).is_some() {
            return Err(E_BAL); // 旧名未摘净
        }
        if self.count_dups(np, &nn) != 1 {
            self.dup_seen += 1;
            return Err(E_DUP);
        }
        if !self.dir_audit() { return Err(E_BAL); }
        if let Some(c) = &mut self.ctx {
            c.phase = RenamePhase::Done;
        }
        self.migrated += 1;
        let g = match &self.ctx {
            Some(c) => c.gen_out,
            None => return Err(E_PHASE),
        };
        self.ctx = None; // 释放窗口，允许下一次迁移
        Ok(g)
    }

    /// 统计某 (父,名) 在表中出现次数（查重）。
    fn count_dups(&self, parent: u64, name: &[u8]) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.table.len() {
            if self.table[i].parent == parent && bytes_eq(&self.table[i].name, name) {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 窗口期查找：不改表，只读并记账（J2 的负载口）。
    /// 返回 (旧名命中?, 新名命中?)。
    pub fn lookup_window(&mut self, parent: u64, name: &[u8]) -> Option<u64> {
        self.lookups_during += 1;
        match self.find(parent, name) {
            Some(i) => Some(self.table[i].dentry),
            None => None,
        }
    }

    /// 父账核对：每个目录的 nchild 必须等于表中实条目数（J4）。
    pub fn dir_audit(&self) -> bool {
        let mut i = 0usize;
        while i < self.dirs.len() {
            let id = self.dirs[i].id;
            let mut real = 0u32;
            let mut k = 0usize;
            while k < self.table.len() {
                if self.table[k].parent == id {
                    real += 1;
                }
                k += 1;
            }
            if real != self.dirs[i].nchild {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 表内条目总数。
    pub fn total_entries(&self) -> u32 {
        self.table.len() as u32
    }

    /// 全表唯一性核对：不得存在同 (父,名) 两条。
    pub fn unique_keys(&self) -> bool {
        let mut i = 0usize;
        while i < self.table.len() {
            let p = self.table[i].parent;
            let n = &self.table[i].name;
            let mut j = i + 1;
            let mut c = 0u32;
            while j < self.table.len() {
                if self.table[j].parent == p && bytes_eq(&self.table[j].name, n) {
                    c += 1;
                }
                j += 1;
            }
            if c > 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 字节相等。
fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 字节拷贝。
fn copy(a: &[u8]) -> Vec<u8> {
    let mut v: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < a.len() {
        v.push(a[i]);
        i += 1;
    }
    v
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NO_ENTRY => "源目录项不存在",
        E_TARGET_EXISTS => "目标名已存在（本模块不覆盖）",
        E_NO_DIR => "目标父目录不存在或重复建目录",
        E_NAME_EMPTY => "新名/旧名为空",
        E_PHASE => "迁移相位乱序（或已有在途迁移）",
        E_BAL => "父链账不平 / 收尾核对失败",
        E_DUP => "迁移后出现重复键",
        E_NAME_LONG => "名字超长（NAME_MAX=255）",
        _ => "未知诊断码",
    }
}

/// 造名字：前缀 + 十进制序号。
fn name_of(prefix: u8, i: u32, out: &mut Vec<u8>) {
    out.clear();
    out.push(prefix);
    let mut digits: [u8; 10] = [0; 10];
    let mut n = 0usize;
    let mut v = i;
    if v == 0 {
        out.push(b'0');
        return;
    }
    while v > 0 && n < 10 {
        digits[n] = b'0' + (v % 10) as u8;
        v /= 10;
        n += 1;
    }
    while n > 0 {
        n -= 1;
        out.push(digits[n]);
    }
}

/// 确定性演练：同父改名 → 窗口千查 → 跨父迁移 → 乱序拦截 → 父账对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut m = RenameMachine::new();
    let mut buf: Vec<u8> = Vec::new();
    let mut nbuf: Vec<u8> = Vec::new();

    // R0：两个目录，各 100 条子项；父账与唯一性自审通过。
    m.add_dir(1)?;
    m.add_dir(2)?;
    let mut i = 0u32;
    while i < 100 {
        name_of(b'a', i, &mut buf);
        m.add_entry(1, &buf, 0x5000 + i as u64)?;
        name_of(b'b', i, &mut buf);
        m.add_entry(2, &buf, 0x6000 + i as u64)?;
        i += 1;
    }
    if m.total_entries() != 200 || !m.dir_audit() || !m.unique_keys() { return Err(E_BAL); }
    hits += 1;

    // R1：同父改名——begin→insert→unlink→commit，条目总数恒 200（J1 无丢失）。
    let total_before = m.total_entries();
    name_of(b'a', 7, &mut buf);
    name_of(b'z', 0, &mut nbuf);
    m.begin(1, &buf, 1, &nbuf)?;
    if m.ctx.as_ref().map(|c| c.phase) != Some(RenamePhase::LookupOld) { return Err(E_PHASE); }
    m.step_insert()?;
    if m.total_entries() != total_before + 1 {
        return Err(E_BAL); // 双名窗口：条目 +1
    }
    if m.ctx.as_ref().map(|c| c.phase) != Some(RenamePhase::InsertNew) { return Err(E_PHASE); }
    hits += 1;

    // R2：窗口期 1,000 次查找——新旧两名的命中数各 1,000，dentry 相同（J2 零丢失）。
    let mut old_hit = 0u32;
    let mut new_hit = 0u32;
    let mut k = 0u32;
    while k < 1_000 {
        if m.lookup_window(1, &buf) == Some(0x5007) {
            old_hit += 1;
        }
        if m.lookup_window(1, &nbuf) == Some(0x5007) {
            new_hit += 1;
        }
        k += 1;
    }
    if old_hit != 1_000 || new_hit != 1_000 {
        return Err(E_BAL); // 双名窗口内两名都必须命中同一 dentry
    }
    if m.lookups_during != 2_000 { return Err(E_BAL); }
    if m.count_dups(1, &buf) != 1 || m.count_dups(1, &nbuf) != 1 {
        return Err(E_DUP); // 零重复
    }
    hits += 1;

    // R3：摘旧名 + 收尾——旧名消失、新名唯一、条目回到 200、代次 +1（J1/J3）。
    m.step_unlink()?;
    if m.total_entries() != total_before { return Err(E_BAL); }
    let g = m.commit()?;
    if g != 1 { return Err(E_BAL); }
    if m.find(1, &buf).is_some() {
        return Err(E_BAL); // 旧名彻底消失
    }
    if m.find(1, &nbuf).map(|x| m.table[x].dentry) != Some(0x5007) { return Err(E_BAL); }
    if m.migrated != 1 || !m.dir_audit() || !m.unique_keys() { return Err(E_BAL); }
    hits += 1;

    // R4：跨父迁移——2 号目录的 b0 迁到 1 号目录，旧父 nchild -1、新父 +1（J4）。
    let d2_before = m.dir_idx(2).map(|x| m.dirs[x].nchild);
    let d1_before = m.dir_idx(1).map(|x| m.dirs[x].nchild);
    name_of(b'b', 0, &mut buf);
    name_of(b'c', 0, &mut nbuf);
    m.begin(2, &buf, 1, &nbuf)?;
    m.step_insert()?;
    m.step_unlink()?;
    let g2 = m.commit()?;
    if g2 != 1 { return Err(E_BAL); }
    if m.dir_idx(2).map(|x| m.dirs[x].nchild) != d2_before.map(|v| v - 1) { return Err(E_BAL); }
    if m.dir_idx(1).map(|x| m.dirs[x].nchild) != d1_before.map(|v| v + 1) {
        // 1 号目录只在本迁移里收进 c0（+1），未摘任何项
        return Err(E_BAL);
    }
    if m.total_entries() != total_before { return Err(E_BAL); }
    if !m.dir_audit() { return Err(E_BAL); }
    hits += 1;

    // I1：相位乱序——未 begin 就 insert/unlink/commit → E_PHASE，账不动（J1 拒收面）。
    let rej_before = m.phase_rejects;
    if m.step_insert() != Err(E_PHASE) { return Err(E_PHASE); }
    if m.step_unlink() != Err(E_PHASE) { return Err(E_PHASE); }
    if m.commit() != Err(E_PHASE) { return Err(E_PHASE); }
    if m.phase_rejects != rej_before + 3 { return Err(E_BAL); }
    hits += 1;

    // I2：跳步——begin 后直接 unlink（跳过 insert）必须拒，且表不变。
    name_of(b'a', 50, &mut buf);
    name_of(b'e', 50, &mut nbuf);
    m.begin(1, &buf, 1, &nbuf)?;
    let tot2 = m.total_entries();
    if m.step_unlink() != Err(E_PHASE) { return Err(E_PHASE); }
    if m.total_entries() != tot2 { return Err(E_BAL); }
    // 收尾：补完这三步让账回到干净态
    m.step_insert()?;
    m.step_unlink()?;
    m.commit()?;
    hits += 1;

    // I3：目标已存在 → E_TARGET_EXISTS；源不存在 → E_NO_ENTRY；新名空 → E_NAME_EMPTY。
    name_of(b'a', 60, &mut buf);
    name_of(b'a', 61, &mut nbuf);
    if m.begin(1, &buf, 1, &nbuf) != Err(E_TARGET_EXISTS) { return Err(E_TARGET_EXISTS); }
    if m.begin(1, b"no-such-name", 1, b"whatever") != Err(E_NO_ENTRY) { return Err(E_NO_ENTRY); }
    if m.begin(1, &buf, 1, b"") != Err(E_NAME_EMPTY) { return Err(E_NAME_EMPTY); }
    hits += 1;

    // I4：在途迁移未结时再开一个 → E_PHASE（同一时刻只允许一个迁移）。
    name_of(b'a', 70, &mut buf);
    name_of(b'f', 70, &mut nbuf);
    m.begin(1, &buf, 1, &nbuf)?;
    name_of(b'a', 71, &mut buf);
    name_of(b'f', 71, &mut nbuf);
    if m.begin(1, &buf, 1, &nbuf) != Err(E_PHASE) {
        return Err(E_PHASE); // 在途迁移互斥
    }
    m.step_insert()?;
    m.step_unlink()?;
    m.commit()?;
    hits += 1;

    // I5：不存在的父目录 → E_NO_DIR；重复建目录 → E_NO_DIR。
    if m.begin(1, b"no-such", 99, b"x") != Err(E_NO_ENTRY) {
        return Err(E_NO_ENTRY); // 源项不存在，先于目标父校验拒收
    }
    if m.add_dir(1) != Err(E_NO_DIR) { return Err(E_NO_DIR); }
    // 目标父不存在（源存在、目标父没建）→ E_NO_DIR
    name_of(b'a', 80, &mut buf);
    if m.begin(1, &buf, 77, b"tk") != Err(E_NO_DIR) { return Err(E_NO_DIR); }
    hits += 1;

    // 终审：迁移后仍是 200 条、父账平、全表键唯一、窗口查找账有记录。
    if m.total_entries() != 200 { return Err(E_BAL); }
    if !m.dir_audit() || !m.unique_keys() { return Err(E_BAL); }
    if m.lookups_during != 2_000 { return Err(E_BAL); }
    if m.ctx.is_some() {
        return Err(E_PHASE); // 收尾干净：无在途迁移
    }
    hits += 1;

    Ok(hits)
}
