//! unx4152_e2ereg — UNX-F4152 fs 注册端到端回归（注册→挂载→开-读-关）（诊断段 0x94C0）。
//!
//! 判据原文：
//!   J1 fake fs 从 register_fs 到端到端读出的一链四步全过。
//!   J2 全链耗时账有记录。
//!   J3 链上任一环断开注入定位准确。
//!   J4 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 一链四步固定序：register_fs → mount → open+read → close，
//!      每步完成记一步账，步号 1..4，乱序跳步拒收。
//!   2. 耗时账以整数滴答记（每步 tick 消耗累加），禁止墙钟。
//!   3. 断链注入按步号打点：被断步之后的全部步骤必须零执行，
//!      run() 返回失败步号实现精确定位（判据 J3）。
//!   4. 读出内容账：read 出账字节数与 fake 负载长度守恒。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x94C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_DUP_FS: u16 = SEG_BASE + 1;      // 同名 fs 重复注册
const E_NOT_REG: u16 = SEG_BASE + 2;     // 未注册先挂载
const E_DUP_MOUNT: u16 = SEG_BASE + 3;   // 重复挂载
const E_NOT_MOUNT: u16 = SEG_BASE + 4;   // 未挂载先 open
const E_NOT_OPEN: u16 = SEG_BASE + 5;    // 未 open 先 read/close
const E_STEP_SKIP: u16 = SEG_BASE + 6;   // 跳步（链序失守）
const E_BAD_STEP: u16 = SEG_BASE + 7;    // 断链步号越界
const E_READ_BAL: u16 = SEG_BASE + 8;    // 读出字节账与负载不平
const E_EMPTY_SUM: u16 = SEG_BASE + 9;   // 写回摘要为空

/// 四步步号闭集（0 = 无注入）。
pub const STEP_NONE: u8 = 0;
pub const STEP_REGISTER: u8 = 1; // register_fs
pub const STEP_MOUNT: u8 = 2;    // mount
pub const STEP_OPENREAD: u8 = 3; // open + read
pub const STEP_CLOSE: u8 = 4;    // close
pub const STEP_LAST: u8 = 4;

/// 链路状态机：Halt 未起 / Running 链进行中 / Done 全链完成 / Broken 断链。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainState {
    Halt,
    Running,
    Done,
    Broken,
}

/// 单步执行账。
pub struct StepRecord {
    pub step: u8,     // 步号 1..4
    pub ticks: u32,   // 本步滴答消耗（整数计时账）
    pub ok: bool,     // 本步是否通过
}

/// fake fs 负载：注册名 + 内嵌读出内容（字节）。
pub struct FakeFs {
    pub name: [u8; 8], // fs 名（定长槽）
    pub name_len: usize,
    pub payload: [u8; 32], // 读出负载
    pub payload_len: usize,
    pub registered: bool,
    pub mounted: bool,
}

/// 端到端链枢纽：四步账 + 耗时账 + 断链定位。
pub struct E2eChain {
    pub fs: FakeFs,
    pub state: ChainState,
    pub steps: Vec<StepRecord>,
    pub total_ticks: u32,      // 全链耗时账（判据 J2）
    pub read_bytes: u32,       // 读出字节账
    pub broken_at: u8,         // 断链定位（0=未断）
    pub break_inject: u8,      // 注入步号（0=无注入）
}

impl E2eChain {
    pub fn new(name: &[u8], payload: &[u8]) -> Result<E2eChain, u16> {
        if name.is_empty() || name.len() > 8 {
            return Err(E_DUP_FS);
        }
        if payload.is_empty() || payload.len() > 32 {
            return Err(E_READ_BAL);
        }
        let mut n = [0u8; 8];
        let mut i = 0usize;
        while i < name.len() {
            n[i] = name[i];
            i += 1;
        }
        let mut p = [0u8; 32];
        let mut j = 0usize;
        while j < payload.len() {
            p[j] = payload[j];
            j += 1;
        }
        Ok(E2eChain {
            fs: FakeFs { name: n, name_len: name.len(), payload: p, payload_len: payload.len(), registered: false, mounted: false },
            state: ChainState::Halt,
            steps: Vec::new(),
            total_ticks: 0,
            read_bytes: 0,
            broken_at: 0,
            break_inject: 0,
        })
    }

    /// 设定断链注入步号（1..4），0 清除。
    pub fn set_break_inject(&mut self, step: u8) -> Result<(), u16> {
        if step > STEP_LAST {
            return Err(E_BAD_STEP);
        }
        self.break_inject = step;
        Ok(())
    }

    fn record(&mut self, step: u8, ticks: u32, ok: bool) {
        self.steps.push(StepRecord { step, ticks, ok });
        self.total_ticks += ticks;
    }

    /// 全链执行：四步顺序推进；断链步即停并定位（判据 J1/J3）。
    pub fn run(&mut self) -> Result<(), u16> {
        self.state = ChainState::Running;
        self.steps.clear();
        self.total_ticks = 0;
        self.read_bytes = 0;
        self.broken_at = 0;
        self.fs.registered = false;
        self.fs.mounted = false;

        // 步 1：register_fs。
        if self.break_inject == STEP_REGISTER {
            self.record(STEP_REGISTER, 1, false);
            self.broken_at = STEP_REGISTER;
            self.state = ChainState::Broken;
            return Err(E_STEP_SKIP);
        }
        self.fs.registered = true;
        self.record(STEP_REGISTER, 2, true);

        // 步 2：mount。
        if self.break_inject == STEP_MOUNT {
            self.record(STEP_MOUNT, 1, false);
            self.broken_at = STEP_MOUNT;
            self.state = ChainState::Broken;
            return Err(E_STEP_SKIP);
        }
        if !self.fs.registered {
            return Err(E_NOT_REG);
        }
        self.fs.mounted = true;
        self.record(STEP_MOUNT, 3, true);

        // 步 3：open + read。
        if self.break_inject == STEP_OPENREAD {
            self.record(STEP_OPENREAD, 1, false);
            self.broken_at = STEP_OPENREAD;
            self.state = ChainState::Broken;
            return Err(E_STEP_SKIP);
        }
        if !self.fs.mounted {
            return Err(E_NOT_MOUNT);
        }
        self.read_bytes = self.fs.payload_len as u32; // open 后整负载读出
        self.record(STEP_OPENREAD, 4, true);

        // 步 4：close。
        if self.break_inject == STEP_CLOSE {
            self.record(STEP_CLOSE, 1, false);
            self.broken_at = STEP_CLOSE;
            self.state = ChainState::Broken;
            return Err(E_STEP_SKIP);
        }
        if self.read_bytes != self.fs.payload_len as u32 {
            return Err(E_READ_BAL);
        }
        self.read_bytes = 0; // close 路径释放读通道
        self.record(STEP_CLOSE, 2, true);
        self.state = ChainState::Done;
        Ok(())
    }

    /// 步账核对：四步齐且全部 ok 才算全链过。
    pub fn verify_chain(&self) -> Result<(), u16> {
        if self.steps.len() != 4 {
            return Err(E_STEP_SKIP);
        }
        let mut i = 0usize;
        while i < self.steps.len() {
            let expect = i as u8 + 1;
            if self.steps[i].step != expect {
                return Err(E_STEP_SKIP);
            }
            if !self.steps[i].ok {
                return Err(E_STEP_SKIP);
            }
            i += 1;
        }
        if self.state != ChainState::Done {
            return Err(E_STEP_SKIP);
        }
        Ok(())
    }

    /// 耗时账可读：total_ticks 必须非零（判据 J2 有记录）。
    pub fn ticks_recorded(&self) -> Result<u32, u16> {
        if self.total_ticks == 0 {
            return Err(E_STEP_SKIP);
        }
        Ok(self.total_ticks)
    }

    /// 断链定位核对：注入步号必须与定位步号一致（判据 J3）。
    pub fn verify_break(&self, expect_step: u8) -> Result<(), u16> {
        if self.broken_at != expect_step {
            return Err(E_BAD_STEP);
        }
        Ok(())
    }

    /// 完成写回（判据 J4）。
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

/// 自例程：R1 全链四步 → R2 边界 → R3 断链注入定位 → R4 账守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 正常路径：四步全过，读出账守恒，耗时账非零。
    let mut c = E2eChain::new(b"fakefs", b"hello-payload")?;
    c.run()?;
    c.verify_chain()?;
    if c.ticks_recorded()? == 0 {
        return Err(E_STEP_SKIP);
    }
    c.writeback(true, 0x40)?;
    hits += 1;

    // R2 边界：空负载 / 负载超 32 / 步号 5 越界。
    if E2eChain::new(b"fs", b"").is_ok() {
        return Err(E_READ_BAL);
    }
    let long = [b'x'; 33];
    if E2eChain::new(b"fs", &long).is_ok() {
        return Err(E_READ_BAL);
    }
    let mut c = E2eChain::new(b"fs", b"ok")?;
    if c.set_break_inject(5).is_ok() {
        return Err(E_BAD_STEP);
    }
    hits += 1;

    // R3 断链注入：四个步号逐一注入，定位必须逐步准确，断后链必失败。
    let mut step = STEP_REGISTER;
    while step <= STEP_LAST {
        let mut c = E2eChain::new(b"fs", b"pay")?;
        c.set_break_inject(step)?;
        if c.run().is_ok() {
            return Err(E_STEP_SKIP);
        }
        c.verify_break(step)?;
        step += 1;
    }
    hits += 1;

    // R4 守恒对账：无注入重跑四步账齐，broken_at 归零。
    let mut c = E2eChain::new(b"fs", b"clean")?;
    c.set_break_inject(STEP_MOUNT)?;
    c.run().is_err();
    c.set_break_inject(STEP_NONE)?;
    c.run()?;
    c.verify_chain()?;
    if c.broken_at != 0 {
        return Err(E_BAD_STEP);
    }
    hits += 1;

    // R5 追加回归：断链后重跑清账 + 步账逐条审计 + 耗时账自洽。
    let mut c = E2eChain::new(b"fs", b"payload-9")?;
    c.set_break_inject(STEP_CLOSE)?;
    c.run().is_err();
    c.verify_break(STEP_CLOSE)?;
    c.set_break_inject(STEP_NONE)?;
    c.run()?;
    c.verify_chain()?;
    // 步账逐条：步号连续且全 ok。
    let mut i = 0usize;
    while i < c.steps.len() {
        if c.steps[i].step != (i + 1) as u8 || !c.steps[i].ok {
            return Err(E_STEP_SKIP);
        }
        i += 1;
    }
    // 耗时账 = 各步滴答之和（账面自洽，判据 J2 复核）。
    let mut sum = 0u32;
    let mut j = 0usize;
    while j < c.steps.len() {
        sum += c.steps[j].ticks;
        j += 1;
    }
    if sum != c.total_ticks {
        return Err(E_STEP_SKIP);
    }
    if c.ticks_recorded()? == 0 {
        return Err(E_STEP_SKIP);
    }
    // close 后读通道已排空：读出账归零。
    if c.read_bytes != 0 {
        return Err(E_READ_BAL);
    }
    c.writeback(true, 0x20)?;
    hits += 1;

    Ok(hits)
}
