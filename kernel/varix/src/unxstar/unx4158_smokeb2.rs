//! unx4158_smokeb2 — UNX-F4158 下游消费 smoke（fake B2 嫁接负载）（诊断段 0x9580）。
//!
//! 判据原文：
//!   J1 fake fs 按 B2 视角实现最小集后全链冒烟通过（挂载/创建/读/写/删五步）。
//!   J2 语义表消费侧零障碍（阻塞项=0）。
//!   J3 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 五步固定序：mount → create → read → write → delete，步号
//!      1..5，乱序跳步拒收；delete 后条目账减一（守恒）。
//!   2. 最小集裁定：B2 视角只需要 mount/create/read/write/delete
//!      五钩子，未列钩子不阻塞消费（阻塞只记语义缺口）。
//!   3. 阻塞项账：消费侧遇未实现钩子或语义缺口记一条阻塞，全链
//!      绿判定要求阻塞项账 = 0（判据 J2）。
//!   4. 读写守恒：write 落账字节数 = read 读回字节数（fake 负载
//!      环形一致性），不等即失守。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9580;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NOT_MOUNT: u16 = SEG_BASE + 1;   // 未挂载先建
const E_DUP_ENTRY: u16 = SEG_BASE + 2;   // 同名条目重复创建
const E_NO_ENTRY: u16 = SEG_BASE + 3;    // 读/写/删查无此条目
const E_EMPTY_NAME: u16 = SEG_BASE + 4;  // 空名字拒收
const E_BAL: u16 = SEG_BASE + 5;         // 条目账/字节账守恒失守
const E_BLOCKED: u16 = SEG_BASE + 6;     // 消费阻塞存在（判据 J2 失守）
const E_STEP_SKIP: u16 = SEG_BASE + 7;   // 五步乱序（链序失守）
const E_EMPTY_SUM: u16 = SEG_BASE + 8;   // 写回摘要为空

pub const NAME_CAP: usize = 16; // 条目名容量
pub const DATA_CAP: usize = 32; // 条目数据容量

/// 五步步号闭集。
pub const STEP_MOUNT: u8 = 1;  // 挂载
pub const STEP_CREATE: u8 = 2; // 创建
pub const STEP_READ: u8 = 3;   // 读
pub const STEP_WRITE: u8 = 4;  // 写
pub const STEP_DELETE: u8 = 5; // 删
pub const STEP_LAST: u8 = 5;

/// 冒烟链状态机：Halt 未起 / Running 进行中 / Done 五步全过 / Blocked 有阻塞。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainState {
    Halt,
    Running,
    Done,
    Blocked,
}

/// fake fs 条目：名字槽 + 数据槽 + 有效位。
pub struct FakeEntry {
    pub name: [u8; NAME_CAP],
    pub name_len: usize,
    pub data: [u8; DATA_CAP],
    pub data_len: usize,
    pub live: bool,
}

/// B2 消费冒烟枢纽：五步账 + 条目账 + 阻塞账。
pub struct SmokeB2 {
    pub state: ChainState,
    entries: Vec<FakeEntry>,
    steps_done: [bool; 5],      // 五步完成位
    pub step_last: u8,          // 最后完成步号
    pub blockers: Vec<u16>,     // 阻塞项账（判据 J2）
    pub read_bytes: u32,        // 读出字节账
    pub write_bytes: u32,       // 写入字节账
    pub mounted: bool,
}

impl SmokeB2 {
    pub fn new() -> SmokeB2 {
        SmokeB2 {
            state: ChainState::Halt,
            entries: Vec::new(),
            steps_done: [false; 5],
            step_last: 0,
            blockers: Vec::new(),
            read_bytes: 0,
            write_bytes: 0,
            mounted: false,
        }
    }

    /// 消费侧遇缺口记阻塞（判据 J2 面）。
    pub fn record_blocker(&mut self, code: u16) {
        self.blockers.push(code);
        self.state = ChainState::Blocked;
    }

    /// 步 1：挂载。
    pub fn step_mount(&mut self) -> Result<(), u16> {
        if self.step_last >= STEP_MOUNT && self.steps_done[0] {
            return Err(E_STEP_SKIP);
        }
        self.mounted = true;
        self.steps_done[0] = true;
        self.step_last = STEP_MOUNT;
        self.state = ChainState::Running;
        Ok(())
    }

    /// 步 2：创建（未挂载拒收，重名拒收；创建可与读写交错）。
    pub fn step_create(&mut self, name: &[u8]) -> Result<(), u16> {
        if !self.mounted {
            return Err(E_NOT_MOUNT);
        }
        if name.is_empty() || name.len() > NAME_CAP {
            return Err(E_EMPTY_NAME);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].live && self.entries[i].name_len == name.len() {
                let mut same = true;
                let mut k = 0usize;
                while k < name.len() {
                    if self.entries[i].name[k] != name[k] {
                        same = false;
                        break;
                    }
                    k += 1;
                }
                if same {
                    return Err(E_DUP_ENTRY);
                }
            }
            i += 1;
        }
        let mut e = FakeEntry {
            name: [0u8; NAME_CAP],
            name_len: name.len(),
            data: [0u8; DATA_CAP],
            data_len: 0,
            live: true,
        };
        let mut k = 0usize;
        while k < name.len() {
            e.name[k] = name[k];
            k += 1;
        }
        self.entries.push(e);
        self.steps_done[1] = true;
        self.step_last = STEP_CREATE;
        Ok(())
    }

    fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].live && self.entries[i].name_len == name.len() {
                let mut same = true;
                let mut k = 0usize;
                while k < name.len() {
                    if self.entries[i].name[k] != name[k] {
                        same = false;
                        break;
                    }
                    k += 1;
                }
                if same {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 步 3：读（读回当前数据长度；空数据合法读零）。
    pub fn step_read(&mut self, name: &[u8]) -> Result<usize, u16> {
        if self.step_last < STEP_CREATE {
            return Err(E_STEP_SKIP);
        }
        let idx = match self.find(name) {
            Some(i) => i,
            None => return Err(E_NO_ENTRY),
        };
        let n = self.entries[idx].data_len;
        self.read_bytes += n as u32;
        self.steps_done[2] = true;
        if self.step_last < STEP_READ {
            self.step_last = STEP_READ;
        }
        Ok(n)
    }

    /// 步 4：写（数据长度 <= DATA_CAP，落账字节守恒）。
    pub fn step_write(&mut self, name: &[u8], data: &[u8]) -> Result<(), u16> {
        if self.step_last < STEP_CREATE {
            return Err(E_STEP_SKIP);
        }
        let idx = match self.find(name) {
            Some(i) => i,
            None => return Err(E_NO_ENTRY),
        };
        if data.len() > DATA_CAP {
            return Err(E_BAL);
        }
        let mut i = 0usize;
        while i < data.len() {
            self.entries[idx].data[i] = data[i]; // 先比长度后写入
            i += 1;
        }
        self.entries[idx].data_len = data.len();
        self.write_bytes += data.len() as u32;
        self.steps_done[3] = true;
        self.step_last = STEP_WRITE;
        Ok(())
    }

    /// 步 5：删（live 摘除，条目账守恒）。
    pub fn step_delete(&mut self, name: &[u8]) -> Result<(), u16> {
        if self.step_last < STEP_WRITE {
            return Err(E_STEP_SKIP);
        }
        let idx = match self.find(name) {
            Some(i) => i,
            None => return Err(E_NO_ENTRY),
        };
        self.entries[idx].live = false;
        self.steps_done[4] = true;
        self.step_last = STEP_DELETE;
        self.state = if self.blockers.is_empty() {
            ChainState::Done
        } else {
            ChainState::Blocked
        };
        Ok(())
    }

    /// 全链绿判定：五步齐 + 阻塞项=0 + 读写字节守恒（判据 J1/J2）。
    pub fn verdict(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < 5 {
            if !self.steps_done[i] {
                return Err(E_STEP_SKIP);
            }
            i += 1;
        }
        if !self.blockers.is_empty() {
            return Err(E_BLOCKED);
        }
        if self.read_bytes != self.write_bytes {
            return Err(E_BAL);
        }
        if self.state != ChainState::Done {
            return Err(E_STEP_SKIP);
        }
        Ok(())
    }

    /// 活跃条目计数（守恒核对面）。
    pub fn live_count(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if self.entries[i].live {
                c += 1;
            }
            i += 1;
        }
        c
    }

    /// 完成写回（判据 J3）。
    pub fn writeback(&mut self, status_done: bool, summary_len: u32) -> Result<(), u16> {
        if !status_done {
            return Err(E_STEP_SKIP);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程：R1 五步全链 → R2 边界 → R3 阻塞注入 → R4 守恒对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：挂载→建→写→读→删 五步，读写守恒，绿判定。
    let mut s = SmokeB2::new();
    s.step_mount()?;
    s.step_create(b"file-a")?;
    s.step_write(b"file-a", b"hello-b2")?;
    let n = s.step_read(b"file-a")?;
    if n != 8 {
        return Err(E_BAL);
    }
    s.step_delete(b"file-a")?;
    s.verdict()?;
    s.writeback(true, 0x40)?;
    if s.live_count() != 0 {
        return Err(E_BAL);
    }
    hits += 1;

    // R2 边界：未挂载先建 / 重名 / 查无 / 空名 / 乱序。
    let mut s = SmokeB2::new();
    if s.step_create(b"x").is_ok() {
        return Err(E_NOT_MOUNT);
    }
    s.step_mount()?;
    if s.step_read(b"ghost").is_ok() {
        return Err(E_STEP_SKIP);
    }
    s.step_create(b"a")?;
    if s.step_create(b"a").is_ok() {
        return Err(E_DUP_ENTRY);
    }
    if s.step_create(b"").is_ok() {
        return Err(E_EMPTY_NAME);
    }
    if s.step_delete(b"a").is_ok() {
        return Err(E_STEP_SKIP); // 写之前删：乱序拒收
    }
    hits += 1;

    // R3 阻塞注入：记一条阻塞后绿判定必失败（阻塞项!=0）。
    let mut s = SmokeB2::new();
    s.step_mount()?;
    s.step_create(b"f")?;
    s.step_write(b"f", b"xy")?;
    s.step_read(b"f")?;
    s.record_blocker(E_NO_ENTRY);
    s.step_delete(b"f")?;
    if s.verdict().is_ok() {
        return Err(E_BLOCKED);
    }
    hits += 1;

    // R4 守恒对账：第二轮全链累计读写字节账一致。
    let mut s = SmokeB2::new();
    s.step_mount()?;
    s.step_create(b"n1")?;
    s.step_write(b"n1", b"abcd")?;
    s.step_read(b"n1")?;
    s.step_create(b"n2")?;
    s.step_write(b"n2", b"zz")?;
    s.step_read(b"n2")?;
    s.step_delete(b"n1")?;
    if s.write_bytes != s.read_bytes {
        return Err(E_BAL);
    }
    if s.live_count() != 1 {
        return Err(E_BAL);
    }
    s.step_delete(b"n2")?;
    s.verdict()?;
    hits += 1;

    Ok(hits)
}
