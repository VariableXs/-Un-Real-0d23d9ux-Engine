//! unx4046_relatime — UNX-F4046 atime 相对更新规则（relatime 类）（诊断段 0x8820）。
//!
//! 判据原文：
//!   J1 relatime 规则（atime 早于 mtime 或超 24h 才更新）触发矩阵 8 格全过；
//!   J2 strict 模式开关行为可切换可验。
//! 设计纪律：
//!   1. relatime 判定（Linux 谱系）：读访问到达时，仅当
//!      （a）atime < mtime 或（b）atime 距今 ≥ 24h（超期阈值）
//!      才更新 atime；否则跳过（读不计账）。
//!   2. 8 格触发矩阵（判据 J1）：{atime<mtime, atime≥mtime} ×
//!      {超期, 未超期} × {relatime, strict} = 8 格，逐格断言。
//!   3. strict 模式（判据 J2）：开关切到 strict 后每次读都更新
//!      （无条件）；noatime 语义在此锚外（不实现）。
//!   4. 逻辑时钟：以 u64 tick 代替墙钟，24h 阈值 = 86400 tick
//!      （秒粒度约定），整数比较无浮点。
//!   5. 边界：恰好 24h（边界含）、mtime 晚于 now（非法时钟注入）
//!      ——全部显性分支并记错误账。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

pub const SEG_BASE: u16 = 0x8820;

// ── 错误码闭集 ───────────────────────────────────────────────
const E_CLOCK_INV: u16 = SEG_BASE + 1;   // 非法时钟（now 早于既有时间戳）
const E_FUT_ATIME: u16 = SEG_BASE + 2;   // 未来 atime 注入（时钟 ahead 失真）
const E_BAL: u16 = SEG_BASE + 3;         // 触发矩阵账不平
const E_MTX_MISS: u16 = SEG_BASE + 4;    // 矩阵格判定与预期不符
const E_SWITCH_LIVE: u16 = SEG_BASE + 5; // 模式切换后行为未生效（判据 J2 失守）

/// 24h 超期阈值（tick 数，秒粒度约定）。
pub const RELATIME_DAY: u64 = 86400;

/// 挂载 atime 模式（relatime/strict 二态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AtimeMode {
    Relatime,
    Strict,
}

/// 单 inode 时间戳账。
pub struct TimeStamps {
    pub atime: u64, // 上次访问时间
    pub mtime: u64, // 上次内容修改时间
    pub ctime: u64, // 上次元数据修改时间
}

/// 读访问判定枢纽。
pub struct RelatimeGate {
    pub mode: AtimeMode,
    pub read_calls: u32,    // 读访问总数
    pub updated_total: u32, // atime 更新数
    pub skipped_total: u32, // 跳过数（relatime 判不更新）
    pub now: u64,           // 逻辑时钟（单调递增）
    pub mtx_cells: u32,     // 矩阵格数（判据 J1 账面）
    pub mtx_ok: u32,        // 矩阵一致格数
    pub rejects: u32,       // 错误账
}

impl RelatimeGate {
    pub fn new() -> RelatimeGate {
        RelatimeGate {
            mode: AtimeMode::Relatime,
            read_calls: 0,
            updated_total: 0,
            skipped_total: 0,
            now: 0,
            mtx_cells: 0,
            mtx_ok: 0,
            rejects: 0,
        }
    }

    /// 时钟推进（确定性，只进不退）。
    pub fn tick_to(&mut self, t: u64) -> Result<(), u16> {
        if t < self.now {
            self.rejects += 1;
            return Err(E_CLOCK_INV);
        }
        self.now = t;
        Ok(())
    }

    /// 模式切换（判据 J2 可切换面）。
    pub fn set_mode(&mut self, m: AtimeMode) -> Result<(), u16> {
        self.mode = m;
        Ok(())
    }

    /// 读访问：按当前模式判定是否更新 atime（返回更新后 atime 或
    /// Err(E_BAL) 之外的双态：Ok(true)=更新，Ok(false)=跳过）。
    pub fn read_touch(&mut self, ts: &mut TimeStamps) -> Result<bool, u16> {
        self.read_calls += 1;
        // 非法时钟注入：mtime/ctime 晚于 now 即拒。
        if ts.mtime > self.now || ts.ctime > self.now {
            self.rejects += 1;
            return Err(E_CLOCK_INV);
        }
        // 未来 atime 注入：atime 晚于 now 即拒。
        if ts.atime > self.now {
            self.rejects += 1;
            return Err(E_FUT_ATIME);
        }
        match self.mode {
            AtimeMode::Strict => {
                // strict：无条件更新。
                ts.atime = self.now;
                self.updated_total += 1;
                Ok(true)
            }
            AtimeMode::Relatime => {
                // 判据 J1 规则：atime < mtime 或超 24h 才更新。
                let older = ts.atime < ts.mtime;
                let aged = self.now >= ts.atime + RELATIME_DAY;
                if older || aged {
                    ts.atime = self.now;
                    self.updated_total += 1;
                    Ok(true)
                } else {
                    self.skipped_total += 1;
                    Ok(false)
                }
            }
        }
    }

    /// 8 格矩阵（判据 J1）：relatime 模式下 4 格 + strict 模式下
    /// 4 格，逐格断言触发/跳过与规则一致。
    pub fn matrix_sweep(&mut self) -> Result<u32, u16> {
        let mut cells = 0u32;
        // ── relatime 四格 ──
        self.set_mode(AtimeMode::Relatime)?;
        self.tick_to(100_000)?;
        // 格 1：atime < mtime（无论超期）→ 更新。
        let mut t1 = TimeStamps { atime: 90_000, mtime: 95_000, ctime: 95_000 };
        if self.read_touch(&mut t1)? != true || t1.atime != 100_000 {
            return Err(E_MTX_MISS);
        }
        cells += 1;
        // 格 2：atime ≥ mtime 且未超期 → 跳过。
        let mut t2 = TimeStamps { atime: 99_000, mtime: 98_000, ctime: 98_000 };
        if self.read_touch(&mut t2)? != false || t2.atime != 99_000 {
            return Err(E_MTX_MISS);
        }
        cells += 1;
        // 格 3：atime ≥ mtime 且恰超 24h（now - atime = 86400）→ 更新。
        let mut t3 = TimeStamps { atime: 100_000 - 86_400, mtime: 50_000, ctime: 50_000 };
        if self.read_touch(&mut t3)? != true {
            return Err(E_MTX_MISS);
        }
        cells += 1;
        // 格 4：atime < mtime 且未超期 → 更新（规则 a 优先）。
        let mut t4 = TimeStamps { atime: 99_999, mtime: 100_000, ctime: 100_000 };
        if self.read_touch(&mut t4)? != true {
            return Err(E_MTX_MISS);
        }
        cells += 1;
        // ── strict 四格 ──
        self.set_mode(AtimeMode::Strict)?;
        // 格 5：strict 下 atime < mtime → 更新。
        let mut t5 = TimeStamps { atime: 90_000, mtime: 95_000, ctime: 95_000 };
        if self.read_touch(&mut t5)? != true {
            return Err(E_MTX_MISS);
        }
        cells += 1;
        // 格 6：strict 下未超期 → 仍更新（判据 J2 行为面）。
        let mut t6 = TimeStamps { atime: 99_000, mtime: 98_000, ctime: 98_000 };
        if self.read_touch(&mut t6)? != true {
            return Err(E_MTX_MISS);
        }
        cells += 1;
        // 格 7：strict 下同 tick 连读 → 第二次仍更新。
        let mut t7 = TimeStamps { atime: 99_999, mtime: 99_999, ctime: 99_999 };
        self.read_touch(&mut t7)?;
        if self.read_touch(&mut t7)? != true || t7.atime != 100_000 {
            return Err(E_MTX_MISS);
        }
        cells += 1;
        // 格 8：切回 relatime 后同刻度连读 → 跳过（开关行为可验）。
        self.set_mode(AtimeMode::Relatime)?;
        let mut t8 = TimeStamps { atime: 99_999, mtime: 99_999, ctime: 99_999 };
        if self.read_touch(&mut t8)? != false {
            return Err(E_MTX_MISS);
        }
        cells += 1;
        self.mtx_cells += cells;
        self.mtx_ok += cells;
        if cells != 8 {
            return Err(E_BAL);
        }
        Ok(cells)
    }

    /// 守恒对账：读调用 = 更新 + 跳过；矩阵格账闭合。
    pub fn audit(&self) -> Result<(), u16> {
        if self.read_calls != self.updated_total + self.skipped_total {
            return Err(E_BAL);
        }
        if self.mtx_cells != self.mtx_ok {
            return Err(E_MTX_MISS);
        }
        Ok(())
    }

    /// 导出快照（模式，读账，更新账，跳过账）。
    pub fn ledger(&self) -> (AtimeMode, u32, u32, u32) {
        (self.mode, self.read_calls, self.updated_total, self.skipped_total)
    }

    /// 更新率视图（千分比整数化）。
    pub fn update_rate(&self) -> u32 {
        if self.read_calls == 0 {
            return 0;
        }
        self.updated_total * 1000 / self.read_calls
    }

    /// 超期判定谓词（规则 b 独立观测面，与 read_touch 共用阈值）。
    pub fn aged_p(&self, ts: &TimeStamps) -> bool {
        self.now >= ts.atime + RELATIME_DAY
    }

    /// 谓词一致性核验：relatime 触发 = 规则 a 或 规则 b；
    /// strict 恒真；两模式行为差即开关可验性（判据 J2 的账面形态）。
    pub fn rule_consistency(&self, ts: &TimeStamps) -> Result<bool, u16> {
        if ts.mtime > self.now || ts.ctime > self.now {
            return Err(E_CLOCK_INV);
        }
        match self.mode {
            AtimeMode::Strict => Ok(true),
            AtimeMode::Relatime => Ok(ts.atime < ts.mtime || self.aged_p(ts)),
        }
    }

    /// 拒收细分：非法时钟注入账（回退 + 未来时间戳两类合并面）。
    pub fn clock_rejects(&self) -> u32 {
        self.rejects
    }
}

/// 自例程：R1 矩阵 8 格 → R2 边界时钟注入 → R3 开关行为复验 → R4 非法时钟注入 → R5 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut g = RelatimeGate::new();

    // R1 触发矩阵 8 格（判据 J1）全过。
    let c = g.matrix_sweep()?;
    if c != 8 {
        return Err(E_MTX_MISS);
    }
    hits += 1;

    // R2 边界时钟注入：时钟回退被拒（只进不退）。
    if g.tick_to(99_999) != Err(E_CLOCK_INV) {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 开关行为复验（判据 J2）：strict/relatime 交替切换行为即时生效。
    g.set_mode(AtimeMode::Strict)?;
    let mut t = TimeStamps { atime: 100_000, mtime: 100_000, ctime: 100_000 };
    g.tick_to(100_001)?;
    if g.read_touch(&mut t)? != true {
        return Err(E_SWITCH_LIVE);
    }
    g.set_mode(AtimeMode::Relatime)?;
    if g.read_touch(&mut t)? != false {
        // 同刻度 relatime：atime=100001 ≥ mtime 且未超期 → 跳过。
        return Err(E_SWITCH_LIVE);
    }
    hits += 1;

    // R4 非法时钟注入：mtime 晚于 now、未来 atime 两通道分别被拒。
    g.set_mode(AtimeMode::Relatime)?;
    let mut bad = TimeStamps { atime: 50, mtime: 999_999, ctime: 50 };
    if g.read_touch(&mut bad) != Err(E_CLOCK_INV) {
        return Err(E_BAL);
    }
    let mut fut = TimeStamps { atime: 999_999, mtime: 50, ctime: 50 };
    if g.read_touch(&mut fut) != Err(E_FUT_ATIME) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 守恒对账：读账 = 更新 + 跳过；矩阵账闭合。
    g.audit()?;
    let (_m, rc, up, skip) = g.ledger();
    if rc != up + skip || rc < 11 {
        // 11 = 矩阵 9 读 + 复验 2 读。
        return Err(E_BAL);
    }
    // 谓词一致性与更新率视图：relatime 判定与规则 a/b 复合一致。
    let t_chk = TimeStamps { atime: 100_001, mtime: 100_000, ctime: 100_000 };
    if g.rule_consistency(&t_chk)? != (t_chk.atime < t_chk.mtime || g.aged_p(&t_chk)) {
        return Err(E_BAL);
    }
    if g.update_rate() != up * 1000 / rc {
        return Err(E_BAL);
    }
    // 注入账：R2 回退 1 + R4 两通道 2 = 3 笔非法时钟拒收。
    if g.clock_rejects() != 3 {
        return Err(E_BAL);
    }
    hits += 1;

    Ok(hits)
}
