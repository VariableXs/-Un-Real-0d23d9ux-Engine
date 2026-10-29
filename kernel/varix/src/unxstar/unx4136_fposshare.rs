//! unx4136_fposshare — UNX-F4136 f_pos 跨进程共享回归（fork 类语义预锚）（诊断段 0x92E0）。
//!
//! 判据原文：UNX-F4136-J1 共享 file（fake fork 复制）双端偏移联动：一端写 10 字节
//!           另一端读位置前移 10（共享语义），独立 open 双端互不影响（隔离语义）
//!           ——对照断言全过。
//!   J2 独立 open 双端偏移互不影响，也不串到共享对。
//!   J3 撤销共享不清偏移，余下单把手仍可推进。
//!   J4 偏移推进量恒等于本次字节数。
//! 设计纪律：
//!   1. 偏移归属 file 而非 fd：fd 只是「指向 file 的把手」。dup/fork 复制把手不复制
//!      file，故两把手共享同一 f_pos；独立 open 建新 file，故偏移各自独立。本模块
//!      的全部断言都由此一条结构事实推出，不做特例。
//!   2. 共享深度用 file.refs 记账：refs>1 即 Shared，refs==1 即 Exclusive；close 只
//!      减 refs，refs 归零才回收 file。
//!   3. 偏移只由 read/write 推进，且推进量恒等于本次字节数（不做对齐、不做预读外扩）；
//!      两端联动量因此可精确对账为「字节数之和」。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x92E0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x92E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BADF: u16 = SEG_BASE + 1; // fd 非法（EBADF 9）
const E_CLOSED: u16 = SEG_BASE + 2; // 已关闭 fd 再操作
const E_NOFILE: u16 = SEG_BASE + 3; // file 槽失效
const E_ACCT: u16 = SEG_BASE + 4; // 偏移账与字节账不平
const E_MIX: u16 = SEG_BASE + 5; // 共享/隔离语义串账（判据反向失守）
const E_OVF: u16 = SEG_BASE + 6; // 偏移上溢（u64 触顶）
const E_DUP: u16 = SEG_BASE + 7; // fd 号重复占用
const E_SLOT: u16 = SEG_BASE + 8; // 表满

pub const EBADF_ERRNO: u16 = 9;

/// fd 表容量与 file 表容量（file 数可多于 fd 数，独立 open 各自建 file）。
pub const FD_CAP: usize = 16;
pub const FILE_CAP: usize = 16;

/// file 共享状态机：Exclusive（单把手）↔ Shared（多把手）；refs 归零落 Closed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ShareState {
    Exclusive,
    Shared,
    Closed,
}

/// file 对象：偏移与共享计数的唯一归属地。
pub struct FileObj {
    /// 目标 inode
    pub ino: u32,
    /// f_pos 当前偏移
    pub pos: u64,
    /// 指向本 file 的把手数
    pub refs: u32,
    /// 共享状态
    pub state: ShareState,
    /// 本 file 累计字节（读+写）
    pub bytes: u64,
}

impl FileObj {
    pub fn new(ino: u32) -> Result<FileObj, u16> {
        if ino == 0 {
            return Err(E_NOFILE);
        }
        Ok(FileObj {
            ino,
            pos: 0,
            refs: 1,
            state: ShareState::Exclusive,
            bytes: 0,
        })
    }

    /// 加一个把手：Exclusive → Shared。
    fn acquire(&mut self) {
        self.refs += 1;
        self.state = ShareState::Shared;
    }

    /// 减一个把手：归零落 Closed。
    fn release(&mut self) {
        if self.refs > 0 {
            self.refs -= 1;
        }
        if self.refs == 0 {
            self.state = ShareState::Closed;
        } else if self.refs == 1 {
            self.state = ShareState::Exclusive;
        }
    }
}

/// fd 把手：fd 号 + 指向的 file 槽 + 是否有效。
#[derive(Clone, Copy)]
pub struct FdSlot {
    /// fd 号
    pub fd: u32,
    /// file 槽下标
    pub file_idx: usize,
    /// 是否有效
    pub live: bool,
}

/// 偏移命名空间：fd 表 + file 表 + 读写账。
pub struct PosSpace {
    /// fd 表
    pub fds: Vec<FdSlot>,
    /// file 表
    pub files: Vec<FileObj>,
    /// 读调用次数
    pub reads: u32,
    /// 写调用次数
    pub writes: u32,
    /// 读推进字节
    pub read_bytes: u64,
    /// 写推进字节
    pub write_bytes: u64,
    /// 共享联动观测次数（同一 file 被两个不同 fd 先后推进）
    pub linkage_events: u32,
}

impl PosSpace {
    pub fn new() -> PosSpace {
        let mut fds = Vec::new();
        let mut files = Vec::new();
        let mut i = 0usize;
        while i < FD_CAP {
            fds.push(FdSlot {
                fd: 0,
                file_idx: 0,
                live: false,
            });
            i += 1;
        }
        let mut j = 0usize;
        while j < FILE_CAP {
            files.push(FileObj {
                ino: 0,
                pos: 0,
                refs: 0,
                state: ShareState::Closed,
                bytes: 0,
            });
            j += 1;
        }
        PosSpace {
            fds,
            files,
            reads: 0,
            writes: 0,
            read_bytes: 0,
            write_bytes: 0,
            linkage_events: 0,
        }
    }

    fn fd_idx(&self, fd: u32) -> Result<usize, u16> {
        let mut i = 0usize;
        while i < self.fds.len() {
            if self.fds[i].live && self.fds[i].fd == fd {
                return Ok(i);
            }
            i += 1;
        }
        Err(E_BADF)
    }

    fn alloc_fd(&mut self, file_idx: usize) -> Result<u32, u16> {
        if file_idx >= self.files.len() {
            return Err(E_NOFILE);
        }
        let mut i = 0usize;
        while i < self.fds.len() {
            if !self.fds[i].live {
                let fd = (i as u32) + 3; // fd 从 3 起（0/1/2 预留给 std 流）
                if self.fd_idx(fd).is_ok() {
                    return Err(E_DUP);
                }
                self.fds[i] = FdSlot {
                    fd,
                    file_idx,
                    live: true,
                };
                return Ok(fd);
            }
            i += 1;
        }
        Err(E_SLOT)
    }

    /// 独立 open：新 file（pos=0、refs=1），新 fd。
    pub fn open(&mut self, ino: u32) -> Result<u32, u16> {
        let mut slot = self.files.len();
        let mut i = 0usize;
        while i < self.files.len() {
            if self.files[i].ino == 0 {
                slot = i;
                break;
            }
            i += 1;
        }
        if slot >= self.files.len() {
            return Err(E_SLOT);
        }
        self.files[slot] = FileObj::new(ino)?;
        self.alloc_fd(slot)
    }

    /// dup/fork：同 file 再加一个把手（偏移共享）。
    pub fn share(&mut self, fd: u32) -> Result<u32, u16> {
        let i = self.fd_idx(fd)?;
        let fi = self.fds[i].file_idx;
        if fi >= self.files.len() {
            return Err(E_NOFILE);
        }
        if self.files[fi].state == ShareState::Closed {
            return Err(E_CLOSED);
        }
        self.files[fi].acquire();
        self.alloc_fd(fi)
    }

    /// 关闭把手：refs 减一，file 归零才失效。
    pub fn close(&mut self, fd: u32) -> Result<(), u16> {
        let i = self.fd_idx(fd)?;
        let fi = self.fds[i].file_idx;
        self.fds[i].live = false;
        if fi < self.files.len() {
            self.files[fi].release();
            if self.files[fi].state == ShareState::Closed {
                self.files[fi].ino = 0;
            }
        }
        Ok(())
    }

    /// 推进偏移：delta 为本次字节数，pos 与 bytes 同步累加。
    fn advance(&mut self, fd: u32, delta: u64, is_write: bool) -> Result<u64, u16> {
        let i = self.fd_idx(fd)?;
        let fi = self.fds[i].file_idx;
        if fi >= self.files.len() {
            return Err(E_NOFILE);
        }
        if self.files[fi].state == ShareState::Closed {
            return Err(E_CLOSED);
        }
        let f = &mut self.files[fi];
        if f.pos > u64::MAX - delta {
            return Err(E_OVF);
        }
        let shared = f.refs > 1;
        f.pos += delta;
        f.bytes += delta;
        let newpos = f.pos;
        if is_write {
            self.writes += 1;
            self.write_bytes += delta;
        } else {
            self.reads += 1;
            self.read_bytes += delta;
        }
        if shared {
            self.linkage_events += 1;
        }
        Ok(newpos)
    }

    /// 读 n 字节：偏移前移 n。
    pub fn read(&mut self, fd: u32, n: u64) -> Result<u64, u16> {
        self.advance(fd, n, false)
    }

    /// 写 n 字节：偏移前移 n（共享时对端可见）。
    pub fn write(&mut self, fd: u32, n: u64) -> Result<u64, u16> {
        self.advance(fd, n, true)
    }

    /// 读某 fd 的当前偏移（不推进）。
    pub fn tell(&self, fd: u32) -> Result<u64, u16> {
        let i = self.fd_idx(fd)?;
        let fi = self.fds[i].file_idx;
        match self.files.get(fi) {
            Some(f) => Ok(f.pos),
            None => Err(E_NOFILE),
        }
    }

    /// 两 fd 是否指向同一 file（共享语义的判定式）。
    pub fn same_file(&self, a: u32, b: u32) -> Result<bool, u16> {
        let ia = self.fd_idx(a)?;
        let ib = self.fd_idx(b)?;
        Ok(self.fds[ia].file_idx == self.fds[ib].file_idx)
    }

    /// 偏移账对账：每 file 的 pos == bytes；总写读字节 == Σ file.bytes。
    pub fn audit(&self) -> bool {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < self.files.len() {
            let f = &self.files[i];
            if f.ino != 0 {
                if f.pos != f.bytes {
                    return false; // 本项目里 pos 与 bytes 同源同增
                }
                if f.refs == 0 {
                    return false; // 有对象却无把手：状态漏迁
                }
                sum += f.bytes;
            }
            i += 1;
        }
        sum == self.read_bytes + self.write_bytes
    }

    /// 活把手数：Σ file.refs。
    pub fn handles(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.files.len() {
            if self.files[i].ino != 0 {
                n += self.files[i].refs;
            }
            i += 1;
        }
        n
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_BADF => "fd 非法（EBADF 9）",
        E_CLOSED => "已关闭 fd 再操作",
        E_NOFILE => "file 槽失效",
        E_ACCT => "偏移账与字节账不平",
        E_MIX => "共享/隔离语义串账",
        E_OVF => "偏移上溢（u64 触顶）",
        E_DUP => "fd 号重复占用",
        E_SLOT => "fd/file 表满",
        _ => "未知诊断码",
    }
}

/// 确定性演练：共享联动 → 隔离对照 → 撤销共享 → 对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：共享语义——A 写 10 字节，B 的读位置从 0 前移到 10（判据主断言）。
    let mut sp = PosSpace::new();
    let a = sp.open(0x3001)?;
    let b = sp.share(a)?;
    if !sp.same_file(a, b)? {
        return Err(E_MIX);
    }
    if sp.tell(b)? != 0 {
        return Err(E_ACCT);
    }
    let pos_a = sp.write(a, 10)?;
    if pos_a != 10 {
        return Err(E_ACCT);
    }
    if sp.tell(b)? != 10 {
        return Err(E_MIX); // 共享 file：对端偏移必须联动
    }
    if sp.linkage_events != 1 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R2：共享读——B 读 4 字节，A 偏移到 14（双向联动）。
    if sp.read(b, 4)? != 14 {
        return Err(E_ACCT);
    }
    if sp.tell(a)? != 14 {
        return Err(E_MIX);
    }
    if !sp.same_file(a, b)? {
        return Err(E_MIX);
    }
    hits += 1;

    // R3：隔离语义——同一 inode 独立 open 两次，双端偏移互不影响。
    let c = sp.open(0x3001)?;
    let d = sp.open(0x3001)?;
    if sp.same_file(c, d)? {
        return Err(E_MIX); // 独立 open 必须指向不同 file
    }
    sp.write(c, 10)?;
    if sp.tell(c)? != 10 || sp.tell(d)? != 0 {
        return Err(E_MIX);
    }
    if sp.tell(a)? != 14 {
        return Err(E_MIX); // 也不得串到共享对
    }
    hits += 1;

    // R4：撤销共享——关掉 B，A 仍是唯一把手，偏移保持 14（关闭不清偏移）。
    sp.close(b)?;
    if sp.tell(a)? != 14 {
        return Err(E_ACCT);
    }
    if sp.same_file(a, c)? {
        return Err(E_MIX);
    }
    if sp.write(a, 1)? != 15 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R5：千轮联动——共享对每轮写 1 字节，双端偏移始终相等。
    let e = sp.open(0x3002)?;
    let f = sp.share(e)?;
    let mut i = 0u32;
    while i < 1_000 {
        sp.write(e, 1)?;
        if sp.tell(f)? != sp.tell(e)? {
            return Err(E_MIX);
        }
        i += 1;
    }
    if sp.tell(f)? != 1_000 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：关闭后操作 → E_BADF（把手已失效，不是 E_CLOSED：fd 号不复用）。
    sp.close(f)?;
    if sp.tell(f) != Err(E_BADF) {
        return Err(E_BADF);
    }
    if sp.write(f, 1) != Err(E_BADF) {
        return Err(E_BADF);
    }
    // 共享对余下 e 仍可用（refs 从 2 减到 1）
    if sp.write(e, 2)? != 1_002 {
        return Err(E_ACCT);
    }
    hits += 1;

    // I2：非法 fd → E_BADF；share 非法 fd → E_BADF。
    if sp.share(0xFFFF) != Err(E_BADF) {
        return Err(E_BADF);
    }
    if sp.read(1, 4) != Err(E_BADF) {
        return Err(E_BADF);
    }
    hits += 1;

    // I3：ino=0 的 open → E_NOFILE。
    if sp.open(0).is_ok() {
        return Err(E_NOFILE);
    }
    hits += 1;

    // I4：偏移上溢注入 → E_OVF（先比差值，后加法，绝不回绕）。
    let g = sp.open(0x3003)?;
    let big = u64::MAX - 5;
    sp.write(g, big)?;
    if sp.write(g, 10) != Err(E_OVF) {
        return Err(E_OVF);
    }
    if sp.tell(g)? != big {
        return Err(E_ACCT); // 拒收后偏移不得变化
    }
    hits += 1;

    // C1：对账——每个活 file 的 pos==bytes；总字节 == 读写账之和。
    if !sp.audit() {
        return Err(E_ACCT);
    }
    // 写：a 10 + c 10 + a 1 + 千轮 1000 + e 2 + big；读：4
    if sp.write_bytes != 10 + 10 + 1 + 1_000 + 2 + big {
        return Err(E_ACCT);
    }
    if sp.read_bytes != 4 {
        return Err(E_ACCT);
    }
    // 活把手：a（1）、c（1）、d（1）、e（1）、g（1）= 5
    if sp.handles() != 5 {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
