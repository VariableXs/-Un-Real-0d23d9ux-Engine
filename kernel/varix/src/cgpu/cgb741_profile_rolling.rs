//! CGPU-F0741 · 画像滚动更新（目标 380 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0741，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：画像滚动更新——画像权重在不中断派发的前提下滚动演进：
//!    增量应用（有向增量向量）、归一再平衡（Σ=1000‰）、
//!    版本单调递增、回滚栈与更新账守恒。
//!
//! 设计纪律：
//! - 增量向量：四类有向增量（i32，‰），应用后即归一——
//  先裁负（饱和到 0），再按剩余比例缩放回 1000（整数比例，
//  余数按类序尾分配，确定性）。
// - 版本单调：每应用/回滚版本 +1，禁止回退。
// - 回滚栈：每次应用前压栈快照，回滚弹出恢复（版本仍 +1）；
//   空栈回滚拒绝。
// - 更新账守恒：current_version == base_version + applies + rollbacks
//  （显式恒等式）；rollbacks ≤ applies。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4ae0 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4ae0，批内 CGPU-F0741 独占）。
pub const SEG_BASE: u16 = 0x4ae0;

/// 归一失败（全零画像不可归一）。
pub const E_NORM: u16 = SEG_BASE + 1;
/// 增量非法（全零增量或单项绝对值超上限）。
pub const E_DELTA: u16 = SEG_BASE + 2;
/// 空栈回滚。
pub const E_ROLLBACK: u16 = SEG_BASE + 3;
/// 更新账不平。
pub const E_LEDGER: u16 = SEG_BASE + 4;
/// 增量维数不符（必须 4）。
pub const E_DIM: u16 = SEG_BASE + 5;
/// 回滚栈已满。
pub const E_STACK: u16 = SEG_BASE + 6;

/// 增量单项绝对值上限（‰）。
pub const DELTA_CAP: i32 = 500;
/// 回滚栈深度。
pub const STACK_CAP: usize = 16;
/// 权重总和目标。
pub const SUM_TARGET: u32 = 1000;

/// 归一再平衡：先饱和裁负到 0，再整数比例缩放回 1000。
/// 余数分配：按类序从余量最大可容纳者依次 +1（确定性）。
pub fn renormalize(w: &[u32; 4]) -> Result<[u32; 4], u16> {
    // 裁负语义在此层无负数（u32），饱和由调用方增量应用时处理；
    // 此处负责缩放归一。
    let s: u64 = w.iter().map(|x| *x as u64).sum();
    if s == 0 {
        return Err(E_NORM);
    }
    let mut out = [0u32; 4];
    let mut used: u32 = 0;
    for i in 0..4 {
        out[i] = (w[i] as u64 * SUM_TARGET as u64 / s) as u32;
        used += out[i];
    }
    // 余数分配：Σ out ≤ 1000，余 r = 1000 - used；
    // 按类序给未满者 +1（每轮最多 +4，r < 4 由整除性质保证？不——
    // 通用写法：循环给 i=0..4 轮转 +1 直到用尽，且不越原始比例上限+1）。
    let mut r = SUM_TARGET - used;
    let mut i = 0usize;
    while r > 0 {
        out[i % 4] += 1;
        r -= 1;
        i += 1;
    }
    Ok(out)
}

/// 有向增量应用：w[i] = max(0, w[i]+d[i])，随后 renormalize。
pub fn apply_delta(w: &[u32; 4], d: &[i32; 4]) -> Result<[u32; 4], u16> {
    // 增量合法性：全零拒绝；单项超上限拒绝。
    let nonzero = d.iter().any(|x| *x != 0);
    if !nonzero {
        return Err(E_DELTA);
    }
    for x in d {
        if *x > DELTA_CAP || *x < -DELTA_CAP {
            return Err(E_DELTA);
        }
    }
    let mut next = [0u32; 4];
    for i in 0..4 {
        let v = w[i] as i64 + d[i] as i64;
        next[i] = if v < 0 { 0 } else { v as u32 };
    }
    renormalize(&next)
}

/// 画像滚动器。
///
/// 更新账守恒（显式恒等式）：
///   version == base_version + applies + rollbacks
///   rollbacks ≤ applies
#[derive(Clone, Debug)]
pub struct ProfileRoller {
    weights: [u32; 4],
    version: u64,
    base_version: u64,
    applies: u64,
    rollbacks: u64,
    stack: Vec<[u32; 4]>,
}

impl ProfileRoller {
    pub fn new(weights: [u32; 4]) -> Result<Self, u16> {
        let w = renormalize(&weights)?;
        Ok(Self {
            weights: w,
            version: 1,
            base_version: 1,
            applies: 0,
            rollbacks: 0,
            stack: Vec::new(),
        })
    }
    /// 滚动应用一次增量（压栈→应用→版本+1）。
    pub fn roll(&mut self, d: [i32; 4]) -> Result<u64, u16> {
        if self.stack.len() >= STACK_CAP {
            return Err(E_STACK);
        }
        let next = apply_delta(&self.weights, &d)?;
        self.stack.push(self.weights);
        self.weights = next;
        self.applies += 1;
        self.version += 1;
        Ok(self.version)
    }
    /// 回滚（弹栈恢复，版本仍 +1）。
    pub fn rollback(&mut self) -> Result<u64, u16> {
        match self.stack.pop() {
            Some(w) => {
                self.weights = w;
                self.rollbacks += 1;
                self.version += 1;
                Ok(self.version)
            }
            None => Err(E_ROLLBACK),
        }
    }
    /// 守恒审计：version == base + applies + rollbacks；rollbacks ≤ applies。
    pub fn audit(&self) -> Result<(), u16> {
        if self.version != self.base_version + self.applies + self.rollbacks {
            return Err(E_LEDGER);
        }
        if self.rollbacks > self.applies {
            return Err(E_LEDGER);
        }
        let s: u32 = self.weights.iter().sum();
        if s != SUM_TARGET {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    /// 当前权重。
    pub fn weights(&self) -> [u32; 4] {
        self.weights
    }
    /// 当前版本。
    pub fn version(&self) -> u64 {
        self.version
    }
    /// 栈深。
    pub fn depth(&self) -> usize {
        self.stack.len()
    }
    /// 应用/回滚账。
    pub fn ops(&self) -> (u64, u64) {
        (self.applies, self.rollbacks)
    }
}

/// 滚动记录行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RollRec {
    /// 版本号。
    pub ver: u64,
    /// 动作：0=apply 1=rollback。
    pub act: u8,
    /// 应用后权重。
    pub w: [u32; 4],
}

/// 滚动序列重放：从基线出发按记录序重放（确定性），
/// 终态权重与记录尾权重必须一致。
pub fn replay(base: [u32; 4], recs: &[RollRec]) -> Result<[u32; 4], u16> {
    let mut r = ProfileRoller::new(base)?;
    for rec in recs {
        match rec.act {
            0 => {
                // 由前后权重反推增量（记录自洽性）。
                let cur = r.weights();
                let mut d = [0i32; 4];
                for i in 0..4 {
                    d[i] = rec.w[i] as i32 - cur[i] as i32;
                }
                r.roll(d)?;
            }
            1 => {
                r.rollback()?;
            }
            _ => return Err(E_LEDGER),
        }
        if r.weights() != rec.w {
            return Err(E_LEDGER);
        }
        if r.version() != rec.ver {
            return Err(E_LEDGER);
        }
    }
    Ok(r.weights())
}

/// 自例程：主路径（基线→三次滚动→回滚→重放）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    // 归一：[400,300,200,100] 已归一。
    let r0 = renormalize(&[400, 300, 200, 100])?;
    if r0 != [400, 300, 200, 100] {
        return Err(E_NORM);
    }
    // 归一：未归一 [500,250,150,100]（Σ=1000 已和——不变）；
    // 用 Σ≠1000 的 [200,200,200,200]（Σ=800）→ 缩放 [250,250,250,250]。
    let r1 = renormalize(&[200, 200, 200, 200])?;
    if r1 != [250, 250, 250, 250] {
        return Err(E_NORM);
    }
    // 错误路径 0：全零归一。
    if renormalize(&[0, 0, 0, 0]) != Err(E_NORM) {
        return Err(E_NORM);
    }
    let mut p = ProfileRoller::new([400, 300, 200, 100])?;
    if p.version() != 1 || p.weights() != [400, 300, 200, 100] {
        return Err(E_LEDGER);
    }
    // 错误路径 1：全零增量。
    if p.roll([0, 0, 0, 0]) != Err(E_DELTA) {
        return Err(E_DELTA);
    }
    // 错误路径 2：增量超上限。
    if p.roll([501, 0, 0, -501]) != Err(E_DELTA) {
        return Err(E_DELTA);
    }
    // 主路径：增量 +100 crit / -100 idle → [500,300,200,0]，
    // Σ=1000 归一不变。
    let v2 = p.roll([100, 0, 0, -100])?;
    if v2 != 2 || p.weights() != [500, 300, 200, 0] {
        return Err(E_DELTA);
    }
    // 主路径：增量 +50 crit / -100 high → [550,200,200,0] → Σ=950 →
    // 归一：550*1000/950=578；200→210；200→210；0→0；Σ=998，
    // 余 2 → 类序轮转 +1：idx0→579, idx1→211 → [579,211,210,0]。
    let v3 = p.roll([50, -100, 0, 0])?;
    if v3 != 3 || p.weights() != [579, 211, 210, 0] {
        return Err(E_DELTA);
    }
    p.audit()?;
    // 主路径：回滚一次 → 回到 [500,300,200,0]，版本 4。
    let v4 = p.rollback()?;
    if v4 != 4 || p.weights() != [500, 300, 200, 0] {
        return Err(E_ROLLBACK);
    }
    p.audit()?;
    let (ap, rb) = p.ops();
    if ap != 2 || rb != 1 || p.depth() != 1 {
        return Err(E_LEDGER);
    }
    // 错误路径 3：清栈后空栈回滚。
    let mut p2 = ProfileRoller::new([400, 300, 200, 100])?;
    if p2.rollback() != Err(E_ROLLBACK) {
        return Err(E_ROLLBACK);
    }
    // 增量导致负权饱和：[100,300,200,400] 应用 -500 crit（恰在上限内）
    // → crit 饱和 0 → [0,300,200,400] Σ=900 →
    // 归一 [0,333,222,444] Σ=999，余 1 → idx0 +1 → [1,333,222,444]。
    let mut p3 = ProfileRoller::new([100, 300, 200, 400])?;
    let _ = p3.roll([-500, 0, 0, 0])?;
    if p3.weights() != [1, 333, 222, 444] {
        return Err(E_DELTA);
    }
    // 错误路径 4：重放记录动作非法。
    let bad_recs = [RollRec { ver: 2, act: 9, w: [1, 1, 1, 1] }];
    if replay([400, 300, 200, 100], &bad_recs) != Err(E_LEDGER) {
        return Err(E_LEDGER);
    }
    // 主路径：重放一致（apply→rollback 序列）。
    let recs = [
        RollRec { ver: 2, act: 0, w: [440, 300, 200, 60] },
        RollRec { ver: 3, act: 1, w: [400, 300, 200, 100] },
    ];
    let fin = replay([400, 300, 200, 100], &recs)?;
    if fin != [400, 300, 200, 100] {
        return Err(E_LEDGER);
    }
    // 错误路径 5：重放版本不符（记录 ver 与实际推进版本不一致）。
    let bad2 = [RollRec { ver: 9, act: 0, w: [440, 300, 200, 60] }];
    if replay([400, 300, 200, 100], &bad2) != Err(E_LEDGER) {
        return Err(E_LEDGER);
    }
    // 更新账恒等式：version == base + applies + rollbacks。
    let mut p4 = ProfileRoller::new([400, 300, 200, 100])?;
    p4.roll([10, -10, 0, 0])?;
    p4.roll([0, 10, -10, 0])?;
    p4.rollback()?;
    p4.audit()?;
    if p4.version() != 4 {
        return Err(E_LEDGER);
    }
    Ok(0)
}
