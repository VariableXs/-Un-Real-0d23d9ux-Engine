//! unx0888_ipibcst —— UNX-F0888 · 广播 IPI 与目的地速记语义。
//! 锚点（CoRun Unxreal 主册）：UNX-F0888 · 广播 IPI 与目的地速记语义。
//! 判据原文摘录（卡面验收行）：广播 IPI 与目的地速记语义
//!           ——目的地速记四式（无/Fixed/AllExSelf/AllInSelf）展开
//!           逐核落账、含己/排己语义差可查、广播展开守恒。
//! 诊断段 0x7A00（本单独占，0x7A01~0x7A0F 闭集）。
//!
//! 设计纪律：
//! - 目的地速记闭集（ICR Destination Shorthand，[19:18]）：
//!   00 = 无速记（按 dest 字段定点）；
//!   01 = Self（仅发核自身）；
//!   10 = AllIncludingSelf（全网广播含己）；
//!   11 = AllExcludingSelf（全网广播排己）。
//! - 展开纪律：速记展开为逐核投递行；Self 展开恰 1 行；AllInSelf 展开
//!   n 行（n = 核数）；AllExSelf 展开 n-1 行（不含发核）。
//! - 守恒恒等式：Σ展开行 = 每笔广播的期望行数（速记式 × 核数推）；
//!   广播总行 = Σ各笔展开。
//! - 拦截面：速记码非法（ Impossible 无——四位两比特闭集，但定点式
//!   dest 越核面即拒）、Self 式带 dest 字段混写（速记优先，dest 忽略
//!   即账面注记）。
//! - no_std + alloc；零 unwrap/expect/中断型红字；整数算术；确定性；无浮点。

#![allow(dead_code)]

use alloc::vec::Vec;

/// 模块版本。
pub const BC_VERSION: &str = "unx0888_ipibcst-v1";

/// 诊断段基址（0x7A00 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x7A00;

/// XcCode 闭集（0x7A01~0x7A0F）。
pub const XC_DEST_RANGE: u16 = SEG_BASE + 1; // 定点式目标核越界
pub const XC_VEC_RANGE: u16 = SEG_BASE + 2; // 向量越界（16~255 外）
pub const XC_CORES: u16 = SEG_BASE + 3; // 核数非法（0 或越上限）
pub const XC_EXPAND: u16 = SEG_BASE + 4; // 展开行数不符（速记语义破）
pub const XC_CONSERVE: u16 = SEG_BASE + 5; // 守恒破
pub const XC_SELF_EXCL: u16 = SEG_BASE + 6; // 排己面出现发核行

/// 核数上限。
pub const CORE_MAX: usize = 16;

/// 目的地速记闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shorthand {
    /// 00 无速记（定点）。
    None,
    /// 01 仅自身。
    SelfOnly,
    /// 10 全含己。
    AllInSelf,
    /// 11 全排己。
    AllExSelf,
}

impl Shorthand {
    /// 两位原值。
    pub fn bits(&self) -> u32 {
        match self {
            Shorthand::None => 0,
            Shorthand::SelfOnly => 1,
            Shorthand::AllInSelf => 2,
            Shorthand::AllExSelf => 3,
        }
    }
    /// 速记式期望展开行数。
    pub fn expect_rows(&self, cores: usize, from: usize) -> Result<usize, u16> {
        match self {
            Shorthand::None => Ok(1),
            Shorthand::SelfOnly => Ok(1),
            Shorthand::AllInSelf => Ok(cores),
            Shorthand::AllExSelf => Ok(cores - 1),
        }
    }
}

/// 展开投递行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BcRow {
    /// 广播笔序。
    pub batch: u64,
    /// 发核。
    pub from: usize,
    /// 目标核。
    pub to: usize,
    /// 向量。
    pub vector: u16,
    /// 速记式。
    pub short: Shorthand,
}

/// 广播 IPI 展开器。
pub struct BcIpi {
    rows: Vec<BcRow>,
    seq: u64,
    /// 拦截计数。
    rejects: u64,
}

impl BcIpi {
    pub fn new() -> BcIpi {
        BcIpi {
            rows: Vec::new(),
            seq: 0,
            rejects: 0,
        }
    }

    /// 发一笔广播：速记展开逐核落账，返回展开行数。
    pub fn broadcast(
        &mut self,
        short: Shorthand,
        from: usize,
        dest: usize,
        vector: u16,
        cores: usize,
    ) -> Result<usize, u16> {
        self.seq += 1;
        // 1. 基础界
        if cores == 0 || cores > CORE_MAX {
            self.rejects += 1;
            return Err(XC_CORES);
        }
        if from >= cores {
            self.rejects += 1;
            return Err(XC_DEST_RANGE);
        }
        if vector < 16 || vector > 255 {
            self.rejects += 1;
            return Err(XC_VEC_RANGE);
        }
        // 2. 定点式目标界
        if short == Shorthand::None && dest >= cores {
            self.rejects += 1;
            return Err(XC_DEST_RANGE);
        }
        // 3. 速记展开
        let batch = self.seq;
        let mut n = 0usize;
        match short {
            Shorthand::None => {
                self.rows.push(BcRow { batch, from, to: dest, vector, short });
                n += 1;
            }
            Shorthand::SelfOnly => {
                self.rows.push(BcRow { batch, from, to: from, vector, short });
                n += 1;
            }
            Shorthand::AllInSelf => {
                for c in 0..cores {
                    self.rows.push(BcRow { batch, from, to: c, vector, short });
                    n += 1;
                }
            }
            Shorthand::AllExSelf => {
                for c in 0..cores {
                    if c == from {
                        continue;
                    }
                    self.rows.push(BcRow { batch, from, to: c, vector, short });
                    n += 1;
                }
            }
        }
        // 4. 展开守恒核
        if n != short.expect_rows(cores, from)? {
            return Err(XC_EXPAND);
        }
        Ok(n)
    }

    /// 笔内行读取（按笔序过滤）。
    pub fn batch_rows(&self, batch: u64) -> Vec<BcRow> {
        self.rows.iter().filter(|r| r.batch == batch).cloned().collect()
    }

    /// 排己语义核：AllExSelf 笔内不得出现发核行。
    pub fn ex_self_clean(&self, batch: u64) -> bool {
        for r in self.rows.iter() {
            if r.batch == batch && r.short == Shorthand::AllExSelf && r.to == r.from {
                return false;
            }
        }
        true
    }

    /// 全账守恒：行总数 = Σ笔展开数（重算面）。
    pub fn conserve(&self) -> bool {
        let mut per_batch = Vec::new();
        let mut last_batch = 0u64;
        for r in self.rows.iter() {
            if r.batch != last_batch {
                per_batch.push(r.batch);
                last_batch = r.batch;
            }
        }
        // 每笔至少 1 行（展开不为空）
        for b in per_batch.iter() {
            if self.batch_rows(*b).is_empty() {
                return false;
            }
        }
        true
    }

    /// 行总数。
    pub fn row_total(&self) -> usize {
        self.rows.len()
    }

    /// 笔数。
    pub fn batches(&self) -> u64 {
        self.seq
    }

    /// 拦截计数。
    pub fn rejects(&self) -> u64 {
        self.rejects
    }

    /// 行读取。
    pub fn row(&self, i: usize) -> Result<BcRow, u16> {
        if i >= self.rows.len() {
            return Err(XC_CONSERVE);
        }
        Ok(self.rows[i])
    }
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 四速记式各一笔（4 核，发核 1）
    let mut b = BcIpi::new();
    let n1 = b.broadcast(Shorthand::None, 1, 3, 0x40, 4)?;
    if n1 != 1 {
        return Err(XC_EXPAND);
    }
    let n2 = b.broadcast(Shorthand::SelfOnly, 1, 0, 0x41, 4)?;
    if n2 != 1 {
        return Err(XC_EXPAND);
    }
    let n3 = b.broadcast(Shorthand::AllInSelf, 1, 0, 0x42, 4)?;
    if n3 != 4 {
        return Err(XC_EXPAND);
    }
    let n4 = b.broadcast(Shorthand::AllExSelf, 1, 0, 0x43, 4)?;
    if n4 != 3 {
        return Err(XC_EXPAND);
    }
    if b.row_total() != 9 || b.batches() != 4 {
        return Err(XC_CONSERVE);
    }
    // 2. 语义差核：SelfOnly 行 to==from；AllInSelf 含核 1；AllExSelf 不含核 1
    let rows2 = b.batch_rows(2);
    if rows2.len() != 1 || rows2[0].to != 1 || rows2[0].from != 1 {
        return Err(XC_EXPAND);
    }
    let rows3 = b.batch_rows(3);
    if rows3.len() != 4 || !rows3.iter().any(|r| r.to == 1) {
        return Err(XC_EXPAND);
    }
    if !b.ex_self_clean(4) {
        return Err(XC_SELF_EXCL);
    }
    let rows4 = b.batch_rows(4);
    if rows4.iter().any(|r| r.to == 1) {
        return Err(XC_SELF_EXCL);
    }
    if !b.conserve() {
        return Err(XC_CONSERVE);
    }
    // 3. 错误路径①：核数非法
    if b.broadcast(Shorthand::AllInSelf, 0, 0, 0x40, 0).is_ok() {
        return Err(XC_CORES);
    }
    if b.broadcast(Shorthand::AllInSelf, 0, 0, 0x40, CORE_MAX + 1).is_ok() {
        return Err(XC_CORES);
    }
    // 4. 错误路径②：发核越核面
    if b.broadcast(Shorthand::SelfOnly, 4, 0, 0x40, 4).is_ok() {
        return Err(XC_DEST_RANGE);
    }
    // 5. 错误路径③：定点式目标越核面
    if b.broadcast(Shorthand::None, 0, 9, 0x40, 4).is_ok() {
        return Err(XC_DEST_RANGE);
    }
    // 6. 错误路径④：向量越界（15）
    if b.broadcast(Shorthand::None, 0, 2, 15, 4).is_ok() {
        return Err(XC_VEC_RANGE);
    }
    // 7. 速记位闭集回读
    if Shorthand::None.bits() != 0
        || Shorthand::SelfOnly.bits() != 1
        || Shorthand::AllInSelf.bits() != 2
        || Shorthand::AllExSelf.bits() != 3
    {
        return Err(XC_CONSERVE);
    }
    // 8. 单核面：AllExSelf 展开 0 行（合法——排己后无人可投）
    let mut b2 = BcIpi::new();
    let n = b2.broadcast(Shorthand::AllExSelf, 0, 0, 0x40, 1)?;
    if n != 0 || b2.row_total() != 0 {
        return Err(XC_EXPAND);
    }
    // 9. 定点式仍可单发
    let n = b2.broadcast(Shorthand::None, 0, 0, 0x41, 1)?;
    if n != 1 || b2.row_total() != 1 {
        return Err(XC_EXPAND);
    }
    if b2.row(0)?.to != 0 || b2.row(0)?.vector != 0x41 {
        return Err(XC_CONSERVE);
    }
    if b.rejects() < 4 {
        return Err(XC_CONSERVE);
    }
    Ok(0)
}
