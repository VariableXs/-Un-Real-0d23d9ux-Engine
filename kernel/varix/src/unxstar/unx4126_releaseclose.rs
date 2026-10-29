//! unx4126_releaseclose — UNX-F4126 release/close 路径与排空（诊断段 0x91A0）。
//!
//! 判据原文：UNX-F4126-J1 close 触发 f_op.release 恰好一次（重复 close 注入零二次
//!           调用），在途 IO 排空先于释放（排空完成标志先行断言）。
//!   J2 排空标志置位时在途账必须已归零。
//!   J3 排空期新提交一律拒收并计入 rejected 账。
//!   J4 批级 release_total 恒等于成功关闭数。
//! 设计纪律：
//!   1. 关闭是四段式单向流程：Open → Draining → Drained → Released → Closed。
//!      每段只认前一段的到达，跳段即拒（E_ORDER），不做事后补救。
//!   2. release 只允许在 Drained 之后调用；Drained 的判据是「提交数 == 完成数」
//!      而不是「调用过 drain」——标志先行的含义是：drained 置位那一刻，在途账
//!      必须已归零，否则该标志本身就是伪造。
//!   3. release 调用次数账（release_calls）与 close 次数账（close_calls）分离：
//!      重复 close 允许返回成功语义以外的「已关闭」错误，但 release_calls 恒为 1。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x91A0 本单独占。

pub const SEG_BASE: u16 = 0x91A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_BUSY: u16 = SEG_BASE + 1; // 在途 IO 未排空即试图推进（EBUSY 面）
const E_ORDER: u16 = SEG_BASE + 2; // 阶段跳变（未排空先 release 等）
const E_ALREADY: u16 = SEG_BASE + 3; // 重复 close（不得二次 release）
const E_IO_OVER: u16 = SEG_BASE + 4; // 完成数超过提交数（账目被伪造）
const E_NODRAIN: u16 = SEG_BASE + 5; // drained 标志与在途账不一致
const E_ACCT: u16 = SEG_BASE + 6; // 提交/完成/拒收三账不平
const E_NOFILE: u16 = SEG_BASE + 7; // 无对应 file 对象
const E_NEG: u16 = SEG_BASE + 8; // 计数下溢

/// EBUSY 的 errno 原值（POSIX errno.h）。
pub const EBUSY_ERRNO: u16 = 16;

/// 关闭流程相位机（单向，不可回退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClosePhase {
    Open,
    Draining,
    Drained,
    Released,
    Closed,
}

impl ClosePhase {
    /// 相位序号，用于「只许前进一格」的判定。
    pub fn rank(self) -> u8 {
        match self {
            ClosePhase::Open => 0,
            ClosePhase::Draining => 1,
            ClosePhase::Drained => 2,
            ClosePhase::Released => 3,
            ClosePhase::Closed => 4,
        }
    }
}

/// 在途 IO 账：提交、完成、拒收三路。
pub struct IoLedger {
    /// 已提交（进入块层/驱动）的 IO 笔数
    pub submitted: u32,
    /// 已完成（回调归还）的 IO 笔数
    pub completed: u32,
    /// 因排空期拒收而未提交的笔数
    pub rejected: u32,
}

impl IoLedger {
    pub fn new() -> IoLedger {
        IoLedger {
            submitted: 0,
            completed: 0,
            rejected: 0,
        }
    }

    /// 在途笔数（提交减完成），恒非负由 submit/complete 的判定保证。
    pub fn pending(&self) -> u32 {
        if self.completed > self.submitted {
            0
        } else {
            self.submitted - self.completed
        }
    }

    pub fn quiesced(&self) -> bool {
        self.submitted == self.completed
    }
}

/// 关闭上下文：相位 + 在途账 + 两个计次账 + 排空标志。
pub struct ReleaseCtx {
    /// 关联 inode 号（0 表无对象）
    pub ino: u32,
    /// 相位
    pub phase: ClosePhase,
    /// 在途 IO 账
    pub io: IoLedger,
    /// release 实际调用次数（判据核心：恒 ≤1）
    pub release_calls: u32,
    /// close 调用次数（含被拒的重复 close）
    pub close_calls: u32,
    /// 排空完成标志（置位时在途必须为 0）
    pub drained: bool,
    /// 拒收账：所有被 Err 挡下的动作计数
    pub rejected_ops: u32,
}

impl ReleaseCtx {
    pub fn open(ino: u32) -> Result<ReleaseCtx, u16> {
        if ino == 0 {
            return Err(E_NOFILE);
        }
        Ok(ReleaseCtx {
            ino,
            phase: ClosePhase::Open,
            io: IoLedger::new(),
            release_calls: 0,
            close_calls: 0,
            drained: false,
            rejected_ops: 0,
        })
    }

    /// 拒收记账：只累加拒收账并回传错误码，由调用方按各自返回类型包 Err。
    fn reject(&mut self, e: u16) -> u16 {
        self.rejected_ops += 1;
        e
    }

    /// 提交 IO：Draining 之后一律拒收（排空期冻结新提交）。
    pub fn submit_io(&mut self, n: u32) -> Result<(), u16> {
        if self.phase != ClosePhase::Open {
            self.io.rejected += n;
            return Err(self.reject(E_BUSY));
        }
        if n == 0 {
            return Err(self.reject(E_ACCT));
        }
        self.io.submitted += n;
        Ok(())
    }

    /// 完成 IO：完成数不得越过提交数。
    pub fn complete_io(&mut self, n: u32) -> Result<(), u16> {
        if n == 0 {
            return Err(self.reject(E_ACCT));
        }
        if self.io.completed + n > self.io.submitted {
            return Err(self.reject(E_IO_OVER));
        }
        self.io.completed += n;
        Ok(())
    }

    /// 开始排空：Open → Draining。若此刻已无在途，可直接推进到 Drained。
    pub fn begin_drain(&mut self) -> Result<(), u16> {
        if self.phase != ClosePhase::Open {
            return Err(self.reject(E_ORDER));
        }
        self.phase = ClosePhase::Draining;
        if self.io.quiesced() {
            self.mark_drained()?;
        }
        Ok(())
    }

    /// 排空完成标志置位：只有「提交==完成」时才允许置位（标志先行断言点）。
    fn mark_drained(&mut self) -> Result<(), u16> {
        if !self.io.quiesced() {
            return Err(self.reject(E_NODRAIN));
        }
        self.drained = true;
        self.phase = ClosePhase::Drained;
        Ok(())
    }

    /// 排空轮询：Draining 期每次调用复检在途账，归零即置 drained。
    pub fn poll_drain(&mut self) -> Result<bool, u16> {
        match self.phase {
            ClosePhase::Drained | ClosePhase::Released | ClosePhase::Closed => Ok(true),
            ClosePhase::Draining => {
                if self.io.quiesced() {
                    self.mark_drained()?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            ClosePhase::Open => Err(self.reject(E_ORDER)),
        }
    }

    /// release：只许 Drained → Released，且只许一次。返回本次是否真实调用。
    pub fn release(&mut self) -> Result<u32, u16> {
        if self.phase != ClosePhase::Drained {
            return Err(self.reject(E_ORDER));
        }
        if self.release_calls != 0 {
            return Err(self.reject(E_ALREADY));
        }
        self.release_calls += 1;
        self.phase = ClosePhase::Released;
        Ok(self.release_calls)
    }

    /// close：Released → Closed。重复 close 拒收但计次。
    pub fn close(&mut self) -> Result<(), u16> {
        self.close_calls += 1;
        if self.phase == ClosePhase::Closed {
            return Err(self.reject(E_ALREADY));
        }
        if self.phase != ClosePhase::Released {
            return Err(self.reject(E_ORDER));
        }
        self.phase = ClosePhase::Closed;
        Ok(())
    }

    /// 原子化一站式关闭：drain → release → close，任一失败即返回。
    pub fn close_shutdown(&mut self) -> Result<(), u16> {
        if self.phase == ClosePhase::Open {
            self.begin_drain()?;
        }
        // 排空多轮轮询（有界轮次，避免不确定等待）
        let mut rounds = 0u32;
        while self.phase == ClosePhase::Draining {
            if rounds >= 1024 {
                return Err(self.reject(E_BUSY));
            }
            self.poll_drain()?;
            rounds += 1;
            if !self.io.quiesced() {
                return Err(self.reject(E_BUSY));
            }
        }
        self.release()?;
        self.close()
    }

    /// 守恒：提交 == 完成 + 在途；且 release_calls ≤ close_calls。
    pub fn conserved(&self) -> bool {
        self.io.completed + self.io.pending() == self.io.submitted
            && self.release_calls <= self.close_calls
            && self.release_calls <= 1
    }
}

/// 批量关闭账：多 file 同批走 close_shutdown 的汇总（关闭风暴面）。
pub struct CloseBatch {
    /// 关闭成功笔数
    pub closed: u32,
    /// 因在途未排空而失败笔数
    pub busy: u32,
    /// 因相位错乱而失败笔数
    pub bad_order: u32,
    /// 累计 release 调用次数（判据只看这个数是否等于成功数）
    pub release_total: u32,
}

impl CloseBatch {
    pub fn new() -> CloseBatch {
        CloseBatch {
            closed: 0,
            busy: 0,
            bad_order: 0,
            release_total: 0,
        }
    }

    /// 入账式批量关闭。
    pub fn run(&mut self, ctx: &mut ReleaseCtx) -> Result<(), u16> {
        match ctx.close_shutdown() {
            Ok(()) => {
                self.closed += 1;
                self.release_total += ctx.release_calls;
                Ok(())
            }
            Err(E_BUSY) => {
                self.busy += 1;
                Err(E_BUSY)
            }
            Err(E_ORDER) => {
                self.bad_order += 1;
                Err(E_ORDER)
            }
            Err(e) => Err(e),
        }
    }

    /// 每次成功关闭恰好一次 release（判据「恰好一次」的批级形式）。
    pub fn release_once_per_close(&self) -> bool {
        self.closed == self.release_total
    }
}

/// 诊断码解释。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_BUSY => "在途 IO 未排空（EBUSY 16）",
        E_ORDER => "关闭相位跳变（未排空先 release 等）",
        E_ALREADY => "重复 close，不得触发二次 release",
        E_IO_OVER => "完成数超过提交数（账目伪造）",
        E_NODRAIN => "drained 标志与在途账不一致",
        E_ACCT => "提交/完成/拒收三账不平",
        E_NOFILE => "无对应 file 对象",
        E_NEG => "计数下溢",
        _ => "未知诊断码",
    }
}

/// 确定性演练：正常关闭 → 排空序 → 重复 close → 批级对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：正常路径——3 笔在途、排空、release 一次、close 一次。
    let mut c = ReleaseCtx::open(0x1100)?;
    c.submit_io(3)?;
    c.begin_drain()?;
    if c.phase != ClosePhase::Draining {
        return Err(E_ORDER); // 有在途时不得直接 Drained
    }
    if c.drained {
        return Err(E_NODRAIN);
    }
    c.complete_io(3)?;
    if c.io.pending() != 0 {
        return Err(E_ACCT);
    }
    if !c.poll_drain()? {
        return Err(E_NODRAIN);
    }
    if !c.drained || c.phase != ClosePhase::Drained {
        return Err(E_NODRAIN);
    }
    c.release()?;
    c.close()?;
    if c.release_calls != 1 || c.phase != ClosePhase::Closed {
        return Err(E_ACCT);
    }
    hits += 1;

    // R2：重复 close 注入 100 次——release_calls 恒为 1（判据「零二次调用」）。
    let mut i = 0;
    while i < 100 {
        if c.close() != Err(E_ALREADY) {
            return Err(E_ALREADY);
        }
        if c.release_calls != 1 {
            return Err(E_ALREADY);
        }
        i += 1;
    }
    if c.close_calls != 1 + 100 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R3：排空未完成即 release → E_ORDER（相位门先于计次门）。
    let mut c2 = ReleaseCtx::open(0x1200)?;
    c2.submit_io(5)?;
    if c2.release() != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    if c2.release_calls != 0 {
        return Err(E_ACCT);
    }
    hits += 1;

    // R4：排空期新提交一律拒收并记入 rejected 账。
    c2.begin_drain()?;
    if c2.submit_io(2) != Err(E_BUSY) {
        return Err(E_BUSY);
    }
    if c2.io.rejected != 2 {
        return Err(E_ACCT);
    }
    if c2.io.submitted != 5 {
        return Err(E_ACCT);
    }
    c2.complete_io(5)?;
    if !c2.poll_drain()? {
        return Err(E_NODRAIN);
    }
    c2.release()?;
    c2.close()?;
    hits += 1;

    // R5：批量 8 个 file 同批关闭，release_total == closed。
    let mut batch = CloseBatch::new();
    let mut n = 0u32;
    while n < 8 {
        let mut ctx = ReleaseCtx::open(0x1300 + n)?;
        ctx.submit_io(1)?;
        ctx.complete_io(1)?;
        batch.run(&mut ctx)?;
        n += 1;
    }
    if batch.closed != 8 || !batch.release_once_per_close() {
        return Err(E_ACCT);
    }
    hits += 1;

    // I1：完成数越过提交数 → E_IO_OVER。
    let mut c3 = ReleaseCtx::open(0x1400)?;
    c3.submit_io(2)?;
    if c3.complete_io(3) != Err(E_IO_OVER) {
        return Err(E_IO_OVER);
    }
    hits += 1;

    // I2：零笔提交/完成 → E_ACCT（空动作不得混入账）。
    if c3.submit_io(0) != Err(E_ACCT) || c3.complete_io(0) != Err(E_ACCT) {
        return Err(E_ACCT);
    }
    hits += 1;

    // I3：ino 为 0 → E_NOFILE。
    if ReleaseCtx::open(0).is_ok() {
        return Err(E_NOFILE);
    }
    hits += 1;

    // I4：Open 相位直接 poll_drain → E_ORDER（不得跳过排空起点）。
    let mut c4 = ReleaseCtx::open(0x1500)?;
    if c4.poll_drain() != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    // 未 close 就再 release：Drained 未达 → E_ORDER，且仍零次。
    if c4.release() != Err(E_ORDER) {
        return Err(E_ORDER);
    }
    hits += 1;

    // C1：三账守恒终审——c 的提交 3、完成 3、拒收 0。
    if !c.conserved() {
        return Err(E_ACCT);
    }
    if c.io.submitted != 3 || c.io.completed != 3 || c.io.rejected != 0 {
        return Err(E_ACCT);
    }
    if c.rejected_ops != 100 {
        return Err(E_ACCT); // 100 次重复 close 全部被拒
    }
    hits += 1;

    // C2：批量关闭账守恒——8 成功 + 0 忙 + 0 错序。
    if batch.busy != 0 || batch.bad_order != 0 {
        return Err(E_ACCT);
    }
    if batch.release_total != 8 {
        return Err(E_ACCT);
    }
    hits += 1;

    Ok(hits)
}
