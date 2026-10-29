//! unx4006_tstamp4 — UNX-F4006 时间戳四件套（atime/mtime/ctime）语义锚（诊断段 0x8380）。
//!
//! 判据原文：
//!   J1 三时间戳纳秒级分辨率编码落账：秒 + 纳秒双字段承载，纳秒域
//!      0..=999_999_999 越界拒收。
//!   J2 mtime/ctime 触发矩阵（读/写/截断/属性改四操作）按 POSIX 行为表
//!      逐格断言通过：读只动 atime；写动 mtime+ctime；截断动 mtime+ctime；
//!      属性改动只动 ctime。
//!   J3 时间单调性：同一 inode 的 ctime 不得回退，注入回退序列被拒。
//!   J4 触发账：四操作 × 三时间戳共十二格，每次操作后逐格比对零差。
//! 设计纪律：
//!   1. 时间值为纯逻辑时钟 (sec: u64, nsec: u32)，由外部注入的 tick 递增，
//!      不读墙钟（禁区）；纳秒进位 1e9 即秒进一。
//!   2. 四件套语义锚：atime 访问时间、mtime 数据变更时间、ctime 元数据
//!      变更时间；Linux 语义下 atime 受 relatime 约束，本模块以策略位表达。
//!   3. 触发矩阵为常量表 4×3，操作执行后按表逐格比对；表与实现不一致
//!      即判据失守（E_MATRIX_MISS）。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x8380;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_NSEC_OVER: u16 = SEG_BASE + 1;  // 纳秒域越界（≥1e9）
const E_CTIME_BACK: u16 = SEG_BASE + 2; // ctime 回退（单调性失守）
const E_MATRIX_MISS: u16 = SEG_BASE + 3; // 触发矩阵与实现不符
const E_BAL: u16 = SEG_BASE + 4;        // 触发账与操作账不平
const E_OP_UNKNOWN: u16 = SEG_BASE + 5; // 未知操作类型
const E_ATTR_EMPTY: u16 = SEG_BASE + 6; // 属性改动未带任何属性（空改）

/// 纳秒域上界（1 秒 = 1_000_000_000 ns）。
pub const NSEC_LIMIT: u32 = 1_000_000_000;

/// 逻辑时间值：秒 + 纳秒。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TsVal {
    pub sec: u64,  // 秒部分（逻辑时钟，不读墙钟）
    pub nsec: u32, // 纳秒部分 0..=999_999_999
}

impl TsVal {
    /// 构造并校验纳秒域（判据 J1）。
    pub fn new(sec: u64, nsec: u32) -> Result<TsVal, u16> {
        if nsec >= NSEC_LIMIT {
            return Err(E_NSEC_OVER);
        }
        Ok(TsVal { sec, nsec })
    }

    /// 零值锚（inode 创建初值）。
    pub fn zero() -> TsVal {
        TsVal { sec: 0, nsec: 0 }
    }

    /// 严格新于判定：先比秒再比纳秒。
    pub fn newer_than(&self, o: &TsVal) -> bool {
        self.sec > o.sec || (self.sec == o.sec && self.nsec > o.nsec)
    }

    /// 全序比较：-1 小、0 等、1 大（整数三元组，不引入 Ordering 依赖）。
    pub fn cmp_val(&self, o: &TsVal) -> i8 {
        if self.sec < o.sec {
            return -1;
        }
        if self.sec > o.sec {
            return 1;
        }
        if self.nsec < o.nsec {
            return -1;
        }
        if self.nsec > o.nsec {
            return 1;
        }
        0
    }
}

/// 四件套：atime/mtime/ctime（btime 由 sb 持有，此处不建模）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ts4 {
    pub atime: TsVal, // 访问时间
    pub mtime: TsVal, // 数据修改时间
    pub ctime: TsVal, // 元数据变更时间
}

impl Ts4 {
    pub fn zero() -> Ts4 {
        Ts4 { atime: TsVal::zero(), mtime: TsVal::zero(), ctime: TsVal::zero() }
    }

    /// 单调性校验：ctime 不得回退（判据 J3）。
    pub fn ctime_monotonic(&self, next: &TsVal) -> Result<(), u16> {
        if next.newer_than(&self.ctime) || next.cmp_val(&self.ctime) == 0 {
            Ok(())
        } else {
            Err(E_CTIME_BACK)
        }
    }
}

/// 四类操作（触发矩阵行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileOp {
    Read = 0,     // 读：命中 atime
    Write = 1,    // 写：命中 mtime + ctime
    Truncate = 2, // 截断：命中 mtime + ctime
    SetAttr = 3,  // 属性改动：命中 ctime
}

pub const OP_COUNT: usize = 4;
pub const TS_COUNT: usize = 3;

/// 触发矩阵常量表：行 = 操作，列 = [atime, mtime, ctime]，1 表示更新。
/// 来源：POSIX.1-2017 §4.9 时间戳更新规则 + Linux inode_setattr 行为。
pub const TRIGGER_TAB: [[u8; TS_COUNT]; OP_COUNT] = [
    [1, 0, 0], // Read     → atime
    [0, 1, 1], // Write    → mtime + ctime
    [0, 1, 1], // Truncate → mtime + ctime
    [0, 0, 1], // SetAttr  → ctime
];

/// 时间戳账状态机：Fresh → Read → Written（最近一次操作性质）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TsPhase {
    Fresh = 0,   // 尚无操作
    Read = 1,    // 最近一次为读（只动 atime）
    Written = 2, // 最近一次为写/截断/属性改（动 mtime 或 ctime）
}

/// 时间戳基座：四件套 + 逻辑时钟 + 触发账。
pub struct TsBase {
    pub ts: Ts4,            // 当前三时间戳
    pub tick: u64,          // 逻辑时钟（每次更新前自增，确定性）
    pub ops: [u32; OP_COUNT], // 四操作各计次账
    pub cells: [u32; OP_COUNT * TS_COUNT], // 十二格触发账
    pub total_ops: u32,     // 总操作账
    pub back_rej: u32,      // ctime 回退拒绝账
    pub phase: TsPhase,     // 账状态机
}

impl TsBase {
    pub fn new() -> TsBase {
        TsBase {
            ts: Ts4::zero(),
            tick: 0,
            ops: [0; OP_COUNT],
            cells: [0; OP_COUNT * TS_COUNT],
            total_ops: 0,
            back_rej: 0,
            phase: TsPhase::Fresh,
        }
    }

    /// 逻辑时钟推进一格并转成 TsVal（纳秒按 1000 对齐，模拟粗粒度计时）。
    fn next_tick(&mut self) -> Result<TsVal, u16> {
        self.tick += 1;
        // 每 tick 记 1 微秒：sec = tick / 1_000_000，nsec = (tick % 1e6) * 1000。
        let sec = self.tick / 1_000_000u64;
        let nsec = ((self.tick % 1_000_000u64) as u32) * 1000u32;
        TsVal::new(sec, nsec)
    }

    /// 执行一次操作：按 TRIGGER_TAB 更新对应时间戳并逐格落账（判据 J2/J4）。
    pub fn apply(&mut self, op: FileOp) -> Result<Ts4, u16> {
        let row = op as usize;
        if row >= OP_COUNT {
            return Err(E_OP_UNKNOWN);
        }
        let now = self.next_tick()?;
        // ctime 先行单调校验：任何操作都会带来时间前进，回退即拒。
        self.ts.ctime_monotonic(&now)?;
        let flags = TRIGGER_TAB[row];
        if flags[0] == 1 {
            self.ts.atime = now;
        }
        if flags[1] == 1 {
            self.ts.mtime = now;
        }
        if flags[2] == 1 {
            self.ts.ctime = now;
        }
        // 十二格账：本行命中的格子 +1。
        for c in 0..TS_COUNT {
            if flags[c] == 1 {
                self.cells[row * TS_COUNT + c] += 1;
            }
        }
        self.ops[row] += 1;
        self.total_ops += 1;
        self.phase = if op == FileOp::Read { TsPhase::Read } else { TsPhase::Written };
        Ok(self.ts)
    }

    /// 属性改动空改拦截：无属性提交视作空改。
    pub fn setattr(&mut self, attrs: u32) -> Result<Ts4, u16> {
        if attrs == 0 {
            return Err(E_ATTR_EMPTY);
        }
        self.apply(FileOp::SetAttr)
    }

    /// 矩阵对账：十二格逐格与「操作次数 × 矩阵位」比对（判据 J4）。
    pub fn matrix_matches(&self) -> bool {
        for r in 0..OP_COUNT {
            for c in 0..TS_COUNT {
                let expect = if TRIGGER_TAB[r][c] == 1 { self.ops[r] } else { 0 };
                if self.cells[r * TS_COUNT + c] != expect {
                    return false;
                }
            }
        }
        true
    }

    /// 守恒：十二格总数 == 各操作命中位之和（每次操作命中 1~2 格）。
    pub fn conserve(&self) -> bool {
        let mut cell_sum = 0u32;
        for c in self.cells.iter() {
            cell_sum += *c;
        }
        let mut expect = 0u32;
        for r in 0..OP_COUNT {
            let mut w = 0u32;
            for c in 0..TS_COUNT {
                w += TRIGGER_TAB[r][c] as u32;
            }
            expect += w * self.ops[r];
        }
        cell_sum == expect
    }

    /// 回退注入：直接把 ctime 改成更早值，必须被拒并入账（判据 J3）。
    pub fn inject_back(&mut self, older: TsVal) -> Result<(), u16> {
        match self.ts.ctime_monotonic(&older) {
            Ok(_) => Err(E_CTIME_BACK), // 竟被接受——判据失守
            Err(_) => {
                self.back_rej += 1;
                Ok(())
            }
        }
    }
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_NSEC_OVER => "纳秒域越界（≥1e9）",
        E_CTIME_BACK => "ctime 回退（单调性失守）",
        E_MATRIX_MISS => "触发矩阵与实现不符",
        E_BAL => "触发账与操作账不平",
        E_OP_UNKNOWN => "未知操作类型",
        E_ATTR_EMPTY => "属性改动未带任何属性（空改）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：纳秒编码 → 触发矩阵逐格 → 单调性 → 注入拦截 → 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut base = TsBase::new();

    // R1：纳秒分辨率编码落账——999_999_999 合法、1e9 越界（判据 J1）。
    let t = TsVal::new(7, 999_999_999)?;
    if t.sec != 7 || t.nsec != 999_999_999 {
        return Err(E_NSEC_OVER);
    }
    if TsVal::new(7, 1_000_000_000) != Err(E_NSEC_OVER) {
        return Err(E_NSEC_OVER);
    }
    hits += 1;

    // R2：读操作只动 atime——mtime/ctime 保持零（判据 J2 第一格）。
    base.apply(FileOp::Read)?;
    let a1 = base.ts;
    if a1.atime.cmp_val(&TsVal::zero()) == 0 {
        return Err(E_MATRIX_MISS);
    }
    if a1.mtime.cmp_val(&TsVal::zero()) != 0 || a1.ctime.cmp_val(&TsVal::zero()) != 0 {
        return Err(E_MATRIX_MISS);
    }
    if base.phase != TsPhase::Read {
        return Err(E_MATRIX_MISS);
    }
    hits += 1;

    // R3：写操作动 mtime+ctime、不动 atime（第二格）。
    let at_before = base.ts.atime;
    base.apply(FileOp::Write)?;
    if base.phase != TsPhase::Written {
        return Err(E_MATRIX_MISS);
    }
    if base.ts.mtime.cmp_val(&TsVal::zero()) == 0 || base.ts.ctime.cmp_val(&TsVal::zero()) == 0 {
        return Err(E_MATRIX_MISS);
    }
    if base.ts.atime != at_before {
        return Err(E_MATRIX_MISS);
    }
    hits += 1;

    // R4：截断动 mtime+ctime（第三格），且 mtime 前进。
    let mt_before = base.ts.mtime;
    base.apply(FileOp::Truncate)?;
    if !base.ts.mtime.newer_than(&mt_before) {
        return Err(E_MATRIX_MISS);
    }
    hits += 1;

    // R5：属性改只动 ctime（第四格），atime/mtime 不动。
    let at_b = base.ts.atime;
    let mt_b = base.ts.mtime;
    base.setattr(0b1011)?;
    if base.ts.atime != at_b || base.ts.mtime != mt_b {
        return Err(E_MATRIX_MISS);
    }
    if base.ts.ctime.cmp_val(&TsVal::zero()) == 0 {
        return Err(E_MATRIX_MISS);
    }
    hits += 1;

    // R6：十二格矩阵对账——4 操作各 1 次，命中 1+2+2+1 = 6 格次（判据 J4）。
    if !base.matrix_matches() || !base.conserve() {
        return Err(E_MATRIX_MISS);
    }
    if base.total_ops != 4 {
        return Err(E_BAL);
    }
    hits += 1;

    // R7：单调性——ctime 在新操作后严格前进。
    let c_before = base.ts.ctime;
    base.apply(FileOp::Write)?;
    if !base.ts.ctime.newer_than(&c_before) {
        return Err(E_CTIME_BACK);
    }
    hits += 1;

    // I1：ctime 回退注入被拒并入账（判据 J3）。
    base.inject_back(TsVal::zero())?;
    if base.back_rej != 1 {
        return Err(E_BAL);
    }
    hits += 1;

    // I2：纳秒越界注入——构造 1e9 直接拒。
    if TsVal::new(0, NSEC_LIMIT) != Err(E_NSEC_OVER) {
        return Err(E_NSEC_OVER);
    }
    hits += 1;

    // I3：空属性改注入被拒，不落操作账。
    let ops_before = base.total_ops;
    if base.setattr(0) != Err(E_ATTR_EMPTY) {
        return Err(E_ATTR_EMPTY);
    }
    if base.total_ops != ops_before {
        return Err(E_BAL);
    }
    hits += 1;

    // R8：末态守恒复核（注入不破坏十二格账）。
    if !base.matrix_matches() || !base.conserve() {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
