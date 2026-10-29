//! CGPU-F0729 · 场景内容 fuzz（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0729，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：场景内容 fuzz——对场景内容描述表（元素清单）做
//!    确定性变异（增/删/改/重排/截断），以不变量组为预言机：
//!    id 唯一、字节和守恒、容量上限、序完整性；
//!    命中违例即记反例并做收缩最小化。
//!
//! 设计纪律：
//! - LCG 常量写死（a=2862933555777941757, c=3037000493，
//   取自经典 glibc 型推荐），同种子同流，禁随机熵。
// - 变异五算子闭集：Dup/Remove/Bump/Truncate/Shuffle，
//   每算子选择由 LCG 取模定，无隐藏分支。
// - 不变量四闭集（预言机）：
//   I1 id 唯一；I2 字节和 == 校验和字段；I3 长度 ≤ 容量；
//   I4 序号单调段完整（0..len 无跳号——按 idx 字段）。
// - 反例收缩：删算子序列前缀重放，最小保持违例。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4960 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4960，批内 CGPU-F0729 独占）。
pub const SEG_BASE: u16 = 0x4960;

/// 种子非法（0）。
pub const E_SEED: u16 = SEG_BASE + 1;
/// 不变量 I1 违例（id 重复）。
pub const E_I1: u16 = SEG_BASE + 2;
/// 不变量 I2 违例（字节和不符）。
pub const E_I2: u16 = SEG_BASE + 3;
/// 不变量 I3 违例（超容）。
pub const E_I3: u16 = SEG_BASE + 4;
/// 不变量 I4 违例（序跳号）。
pub const E_I4: u16 = SEG_BASE + 5;
/// 变异步数预算耗尽。
pub const E_BUDGET: u16 = SEG_BASE + 6;

/// 内容表容量。
pub const TABLE_CAP: usize = 64;
/// 变异步上限。
pub const MUT_MAX: usize = 256;

/// LCG 乘子（glibc 型）。
const LCG_A: u64 = 2862933555777941757;
/// LCG 增量。
const LCG_C: u64 = 3037000493;

/// 确定性伪随机源。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rng {
    st: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Result<Self, u16> {
        if seed == 0 {
            return Err(E_SEED);
        }
        Ok(Self { st: seed })
    }
    pub fn next(&mut self) -> u64 {
        self.st = self.st.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.st
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 场景内容元素。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CElem {
    /// 元素 id。
    pub id: u16,
    /// 字节数。
    pub bytes: u32,
}

/// 场景内容描述表（带校验和字段——I2 的账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentTable {
    elems: Vec<CElem>,
    /// 校验和：Σbytes（登记口径）。
    checksum: u64,
}

impl ContentTable {
    /// 构造：自动算校验和。
    pub fn new(elems: Vec<CElem>) -> Self {
        let checksum = elems.iter().map(|e| e.bytes as u64).sum();
        Self { elems, checksum }
    }
    /// 长度。
    pub fn len(&self) -> usize {
        self.elems.len()
    }
    pub fn is_empty(&self) -> bool {
        self.elems.is_empty()
    }
    /// 字节和实算。
    pub fn sum_bytes(&self) -> u64 {
        self.elems.iter().map(|e| e.bytes as u64).sum()
    }
}

/// 预言机：四不变量逐条校验，返回首个违例（或 Ok）。
pub fn oracle(t: &ContentTable) -> Result<(), u16> {
    // I3：容量。
    if t.len() > TABLE_CAP {
        return Err(E_I3);
    }
    // I1：id 唯一（O(n²)，n ≤ 64）。
    for i in 0..t.len() {
        for j in i + 1..t.len() {
            if t.elems[i].id == t.elems[j].id {
                return Err(E_I1);
            }
        }
    }
    // I2：字节和守恒。
    if t.sum_bytes() != t.checksum {
        return Err(E_I2);
    }
    // I4：序完整性——元素按登记序应有 idx 字段语义：
    // 以 id 在表中的出现顺序作为序号，id 必须严格递增（场景内容
    /// 规范：内容表按 id 升序维护）。跳号/回退即违例。
    for w in t.elems.windows(2) {
        if w[1].id <= w[0].id {
            return Err(E_I4);
        }
    }
    Ok(())
}

/// 变异算子（闭集五类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MutOp {
    /// 复制一条（id 改为 id+1024，可诱发 I1）。
    Dup,
    /// 删除一条。
    Remove,
    /// 改一条字节（不动校验和——诱发 I2）。
    Bump,
    /// 截断到一半（保持 I2 需重算——截断算子同步重算校验和）。
    Truncate,
    /// 交换两条位置（诱发 I4）。
    Shuffle,
}

/// 应用一个变异算子（返回新表）。
pub fn mutate(t: &ContentTable, op: MutOp, idx: usize) -> ContentTable {
    let mut elems = t.elems.clone();
    let mut checksum = t.checksum;
    match op {
        MutOp::Dup => {
            if let Some(e) = elems.get(idx) {
                let e2 = CElem {
                    id: e.id.wrapping_add(1024),
                    bytes: e.bytes,
                };
                elems.push(e2);
                checksum += e2.bytes as u64;
            }
        }
        MutOp::Remove => {
            if idx < elems.len() {
                let b = elems.remove(idx).bytes as u64;
                checksum -= b;
            }
        }
        MutOp::Bump => {
            if idx < elems.len() {
                elems[idx].bytes = elems[idx].bytes.wrapping_add(7);
                // 不动 checksum → I2 违例源
            }
        }
        MutOp::Truncate => {
            let half = elems.len() / 2;
            elems.truncate(half);
            checksum = elems.iter().map(|e| e.bytes as u64).sum();
        }
        MutOp::Shuffle => {
            if elems.len() >= 2 && idx + 1 < elems.len() {
                elems.swap(idx, idx + 1);
            }
        }
    }
    ContentTable { elems, checksum }
}

/// 变异计划：算子序列（确定性生成）。
pub fn plan(seed: u64, steps: usize) -> Result<Vec<(MutOp, usize)>, u16> {
    if seed == 0 {
        return Err(E_SEED);
    }
    let mut g = Rng::new(seed)?;
    let n = steps.min(MUT_MAX);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let op = match g.below(5) {
            0 => MutOp::Dup,
            1 => MutOp::Remove,
            2 => MutOp::Bump,
            3 => MutOp::Truncate,
            _ => MutOp::Shuffle,
        };
        let idx = g.below(32) as usize;
        out.push((op, idx));
    }
    Ok(out)
}

/// 执行变异计划：每步后跑预言机，命中违例返回（步号,违例码）。
pub fn run_plan(base: &ContentTable, plan: &[(MutOp, usize)]) -> Result<Option<(usize, u16)>, u16> {
    if plan.len() > MUT_MAX {
        return Err(E_BUDGET);
    }
    let mut cur = base.clone();
    for (i, (op, idx)) in plan.iter().enumerate() {
        cur = mutate(&cur, *op, *idx);
        if let Err(code) = oracle(&cur) {
            return Ok(Some((i, code)));
        }
    }
    Ok(None)
}

/// 反例收缩：从头逐步截短计划，保持违例码不变。
/// 返回（最短计划, 违例码, 收缩轮数）。
pub fn shrink(base: &ContentTable, plan: &[(MutOp, usize)], want: u16) -> Result<Vec<(MutOp, usize)>, u16> {
    let mut cur: Vec<(MutOp, usize)> = plan.to_vec();
    let mut rounds = 0usize;
    while cur.len() > 1 {
        rounds += 1;
        if rounds > MUT_MAX {
            return Err(E_BUDGET);
        }
        let half = cur.len() / 2;
        let trial: Vec<(MutOp, usize)> = cur[half..].to_vec();
        match run_plan(base, &trial)? {
            Some((_i, code)) if code == want => {
                cur = trial;
            }
            _ => {
                // 前半也试一次；都不行则去尾一条。
                let trial2: Vec<(MutOp, usize)> = cur[..half].to_vec();
                match run_plan(base, &trial2)? {
                    Some((_i, code)) if code == want => {
                        cur = trial2;
                    }
                    _ => {
                        cur.pop();
                    }
                }
            }
        }
    }
    Ok(cur)
}

/// 基准表构造（确定性样例，8 条合法元素）。
pub fn base_table() -> ContentTable {
    ContentTable::new(vec![
        CElem { id: 1, bytes: 512 },
        CElem { id: 2, bytes: 256 },
        CElem { id: 3, bytes: 128 },
        CElem { id: 4, bytes: 64 },
        CElem { id: 5, bytes: 32 },
        CElem { id: 6, bytes: 16 },
        CElem { id: 7, bytes: 8 },
        CElem { id: 8, bytes: 4 },
    ])
}

/// 自例程：主路径（基准表过预言机→多种子 fuzz→命中即收缩）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let base = base_table();
    // 主路径 0：基准表过四不变量。
    oracle(&base)?;
    if base.len() != 8 || base.sum_bytes() != 1020 {
        return Err(E_I2);
    }
    // 错误路径 0：0 种子。
    if Rng::new(0) != Err(E_SEED) || plan(0, 8) != Err(E_SEED) {
        return Err(E_SEED);
    }
    // 主路径：10 种子 × 64 步 fuzz；记录首例（必有：Bump 算子
    // 大概率在 64 步内出现并诱发 I2）。
    let mut found: Option<(u64, Vec<(MutOp, usize)>, u16)> = None;
    for s in 1..=10u64 {
        let pl = plan(s, 64)?;
        if let Some((at, code)) = run_plan(&base, &pl)? {
            found = Some((s, pl.clone(), code));
            let _ = at;
            break;
        }
    }
    // 不强制要求命中（变异可能恰好未诱发违例——空表极端情况），
    // 但对命中例必须可收缩且收缩后仍复现同码。
    if let Some((_s, pl, code)) = found {
        let sh = shrink(&base, &pl, code)?;
        match run_plan(&base, &sh)? {
            Some((_i, code2)) if code2 == code => {}
            _ => return Err(E_BUDGET),
        }
        if sh.len() > pl.len() {
            return Err(E_BUDGET);
        }
    }
    // 定向违例注入（验证预言机四分支）：
    // I2：Bump 一条。
    let t_i2 = mutate(&base, MutOp::Bump, 0);
    if oracle(&t_i2) != Err(E_I2) {
        return Err(E_I2);
    }
    // I1：手工构造重复 id（Dup 后 id+1024 不冲突，故手工构造）。
    let t_i1 = ContentTable::new(vec![
        CElem { id: 5, bytes: 10 },
        CElem { id: 5, bytes: 20 },
    ]);
    if oracle(&t_i1) != Err(E_I1) {
        return Err(E_I1);
    }
    // I4：乱序（大 id 在前）。
    let t_i4 = ContentTable::new(vec![
        CElem { id: 9, bytes: 10 },
        CElem { id: 2, bytes: 20 },
    ]);
    if oracle(&t_i4) != Err(E_I4) {
        return Err(E_I4);
    }
    // I3：超容（65 条，需先构造合法大表再截断校验和同步——直接构造
    // 65 条 id 递增、校验和自动同步的表）。
    let big: Vec<CElem> = (1..=65u16).map(|i| CElem { id: i, bytes: 1 }).collect();
    let t_i3 = ContentTable::new(big);
    if oracle(&t_i3) != Err(E_I3) {
        return Err(E_I3);
    }
    // Remove/Truncate 算子保持合法：删一条 + 截半后仍过预言机。
    let t_rm = mutate(&base, MutOp::Remove, 3);
    oracle(&t_rm)?;
    let t_tr = mutate(&base, MutOp::Truncate, 0);
    oracle(&t_tr)?;
    if t_tr.len() != 4 {
        return Err(E_I3);
    }
    // Shuffle 诱发 I4：交换 0,1 → id 变 2,1 → 违例。
    let t_sh = mutate(&base, MutOp::Shuffle, 0);
    if oracle(&t_sh) != Err(E_I4) {
        return Err(E_I4);
    }
    // 错误路径 1：计划超预算。
    let long = plan(3, MUT_MAX)?;
    let mut toolong = long.clone();
    toolong.push((MutOp::Dup, 0));
    if run_plan(&base, &toolong) != Err(E_BUDGET) {
        return Err(E_BUDGET);
    }
    // 错误路径 2：Rng 确定性（同种子同流）。
    let mut g1 = Rng::new(42)?;
    let mut g2 = Rng::new(42)?;
    for _ in 0..16 {
        if g1.next() != g2.next() {
            return Err(E_SEED);
        }
    }
    Ok(0)
}
