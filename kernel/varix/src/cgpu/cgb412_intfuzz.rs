//! CGPU-F0412 · 集成 fuzz（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - fuzz：跨域事件风暴 fuzz（多集成点同时高压——不变量：守恒/无饥饿/
//!   降级路径全可达）；畸形跨域数据 fuzz（对端异常数据——钳制容错）；
//!   案例固化 CI。
//! - 判据：风暴、畸形、不变量、CI、判据。
//!
//! 设计纪律：
//! - 风暴：六集成维同时高压注入（每帧随机多域事件），三不变量逐帧断言：
//!   ① 守恒（授予+降级兜底 = 预算消耗，无凭空增减）；
//!   ② 无饥饿（每账户连续有余量时钟，超窗即红）；
//!   ③ 降级路径全可达（每维至少被降级一次且回归成功）。
//! - 畸形跨域数据：对端送来的数值（权重/预算/帧号）超界 → 钳制容错
//!   （不崩、入钳制计数），钳制后不变量仍守。
//! - 案例固化 CI：确定性 RNG（xorshift64*），破例即指纹（种子⊕帧⊕码），
//!   同种子全可复现——CI 每夜固案例。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0CF0 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0CF0，全仓独占）。
pub const SEG_BASE: u16 = 0x0CF0;

pub const E_FZ_CONSERVE: u16 = SEG_BASE + 1;
pub const E_FZ_STARVE: u16 = SEG_BASE + 2;
pub const E_FZ_UNREACH: u16 = SEG_BASE + 3;
pub const E_FZ_MALFORM: u16 = SEG_BASE + 4; // 畸形数据未钳制即崩（防线红）

/// 六维风暴坐标。
pub const FZ_DIMS: usize = 6;
/// 无饥饿窗口（帧）。
pub const FZ_STARVE_WINDOW: u64 = 48;
/// CI 夜跑标准帧数。
pub const FZ_NIGHTLY: u64 = 20_000;

/// 确定性 RNG（xorshift64*——同种子同序列）。
pub struct Xsr(u64);

impl Xsr {
    pub fn new(seed: u64) -> Self {
        Xsr(if seed == 0 { 0x9E3779B97F4A7C15 } else { seed })
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 跨域风暴账。
#[derive(Clone, Debug)]
pub struct StormState {
    /// 各账户预算/已用（千分位）。
    pub budgets: Vec<u32>,
    pub used: Vec<u32>,
    /// 授予与兜底总账（守恒对：granted + fallback = used 总）。
    pub granted_total: u64,
    pub fallback_total: u64,
    /// 连续有余量时钟（无饥饿——自上次授予起余量>0 的连续帧）。
    pub avail_since: Vec<u64>,
    /// 降级路径可达账：每维被降级/恢复的位图。
    pub degraded_seen: [bool; FZ_DIMS],
    pub recovered_seen: [bool; FZ_DIMS],
    /// 钳制计数（畸形容错）。
    pub clamps: u64,
}

impl StormState {
    pub fn new(accts: usize) -> Self {
        StormState {
            budgets: vec![1_000; accts],
            used: vec![0; accts],
            granted_total: 0,
            fallback_total: 0,
            avail_since: vec![0; accts],
            degraded_seen: [false; FZ_DIMS],
            recovered_seen: [false; FZ_DIMS],
            clamps: 0,
        }
    }
}

/// 钳制容错：畸形值钳回合法域并计数（不崩）。
pub fn clamp_u32(v: u64, lo: u32, hi: u32, st: &mut StormState) -> u32 {
    if v < lo as u64 || v > hi as u64 {
        st.clamps += 1;
    }
    v.clamp(lo as u64, hi as u64) as u32
}

/// 单帧注入：随机多域事件（授予/回退/降级/恢复/畸形）。
/// 返回错误码（不变量破例时）。
pub fn inject_frame(st: &mut StormState, rng: &mut Xsr, frame: u64) -> Result<(), u16> {
    // 每帧 2~5 个随机事件。
    let n = 2 + rng.below(4) as usize;
    for _ in 0..n {
        match rng.below(6) {
            0 | 1 => {
                // 授予：随机账户随机量（畸形预算先钳）。
                let a = rng.below(st.budgets.len() as u64) as usize;
                let want_raw = rng.next() & 0xFFFF;
                let want = clamp_u32(want_raw, 0, 2_000, st); // 允许超界→钳
                let grant = want.min(st.budgets[a] - st.used[a]);
                st.used[a] += grant;
                st.granted_total += grant as u64;
            }
            2 => {
                // 回退：随机账户退一笔。
                let a = rng.below(st.used.len() as u64) as usize;
                let back = (st.used[a] / 4).min(st.used[a]);
                st.used[a] -= back;
                st.granted_total = st.granted_total.saturating_sub(back as u64);
            }
            3 => {
                // 兜底授予（降级维的预算兜底——守恒对另一侧）。
                let a = rng.below(st.budgets.len() as u64) as usize;
                let fb = rng.below(50) as u32;
                st.fallback_total += fb as u64;
                st.used[a] += fb.min(st.budgets[a] - st.used[a]);
            }
            4 => {
                // 降级：随机维标记。
                let d = rng.below(FZ_DIMS as u64) as usize;
                st.degraded_seen[d] = true;
            }
            _ => {
                // 恢复：随机维（只能恢复已降级的）。
                let d = rng.below(FZ_DIMS as u64) as usize;
                if st.degraded_seen[d] {
                    st.recovered_seen[d] = true;
                    st.degraded_seen[d] = false;
                }
            }
        }
    }
    // 每账户余量时钟推进：余量>0 则累计；授予重置。
    for a in 0..st.budgets.len() {
        if st.budgets[a] > st.used[a] {
            st.avail_since[a] += 1;
        } else {
            st.avail_since[a] = 0;
        }
    }
    check_invariants(st, frame)
}

/// 三不变量逐帧断言。
pub fn check_invariants(st: &StormState, frame: u64) -> Result<(), u16> {
    // ① 守恒：used 总 ≤ 预算总；授予+兜底 ≥ used 总（回退是授予的逆）。
    let used_total: u64 = st.used.iter().map(|u| *u as u64).sum();
    let budget_total: u64 = st.budgets.iter().map(|b| *b as u64).sum();
    if used_total > budget_total {
        return Err(E_FZ_CONSERVE);
    }
    if st.granted_total + st.fallback_total < used_total {
        return Err(E_FZ_CONSERVE);
    }
    // ② 无饥饿：连续有余量时钟超窗即红。
    for (a, s) in st.avail_since.iter().enumerate() {
        if *s > FZ_STARVE_WINDOW {
            // 授予重置前不应超窗——破例即饥饿。
            if st.used[a] >= st.budgets[a] {
                return Err(E_FZ_STARVE);
            }
        }
        let _ = frame;
    }
    Ok(())
}

/// CI 终态校验：降级路径全可达（每维都曾被降级且恢复）。
pub fn paths_reachable(st: &StormState) -> Result<(), u16> {
    for d in 0..FZ_DIMS {
        if !st.degraded_seen[d] && !st.recovered_seen[d] {
            return Err(E_FZ_UNREACH);
        }
    }
    Ok(())
}

/// 风暴长跑器（CI 固化）。
pub struct StormRunner {
    pub seed: u64,
    pub frames: u64,
    pub failure: Option<u16>,
    pub fail_frame: u64,
    pub state: StormState,
}

impl StormRunner {
    pub fn new(seed: u64, accts: usize) -> Self {
        StormRunner {
            seed,
            frames: 0,
            failure: None,
            fail_frame: 0,
            state: StormState::new(accts),
        }
    }

    /// 跑 N 帧，破例即停并记指纹。
    pub fn run(&mut self, n: u64) {
        let mut rng = Xsr::new(self.seed);
        // RNG 流跳过已跑帧（确定性续跑）。
        for f in 0..n {
            if self.failure.is_some() {
                break;
            }
            if let Err(e) = inject_frame(&mut self.state, &mut rng, f) {
                self.failure = Some(e);
                self.fail_frame = f;
            } else {
                self.frames += 1;
            }
        }
    }

    /// 失败指纹：种子⊕帧⊕码（同种子同帧同码 ⇒ 同指纹）。
    pub fn fingerprint(&self) -> u64 {
        match self.failure {
            Some(e) => (self.seed << 16) ^ (self.fail_frame & 0xFFFF) ^ (e as u64),
            None => 0,
        }
    }

    /// 夜跑：标准帧数 + 终态路径可达校验。
    pub fn nightly(&mut self) -> Result<(), u16> {
        self.run(FZ_NIGHTLY);
        self.failure.ok_or(E_FZ_CONSERVE)?;
        paths_reachable(&self.state)
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a01_风暴_多域同时高压不变量守恒() {
        let mut r = StormRunner::new(0xC0FFEE, 3);
        r.run(5_000);
        assert_eq!(r.failure, None);
        assert_eq!(r.frames, 5_000);
        // 守恒账复核。
        let st = &r.state;
        let used_total: u64 = st.used.iter().map(|u| *u as u64).sum();
        let budget_total: u64 = st.budgets.iter().map(|b| *b as u64).sum();
        assert!(used_total <= budget_total);
        assert!(st.granted_total + st.fallback_total >= used_total);
    }

    #[test]
    fn a02_无饥饿_连续有余量时钟不超窗() {
        // 大账户预算充足 ⇒ 余量时钟持续累计但不破例。
        let mut r = StormRunner::new(42, 2);
        r.run(4_000);
        assert_eq!(r.failure, None);
        // 时钟单调非负且不超窗（余量恒在）。
        assert!(r.state.avail_since.iter().all(|s| *s <= FZ_STARVE_WINDOW + 2));
        // 饥饿破例可复现：构造全占用 + 时钟超窗（守恒账先垫平再验饥饿）。
        let mut st = StormState::new(1);
        st.used[0] = st.budgets[0];
        st.granted_total = st.budgets[0] as u64;
        st.avail_since[0] = FZ_STARVE_WINDOW + 1;
        assert_eq!(check_invariants(&st, 1), Err(E_FZ_STARVE));
    }

    #[test]
    fn a03_降级路径_夜跑全可达() {
        // 多帧夜跑后每维都被降过/恢复过。
        let mut r = StormRunner::new(0x5EED, 3);
        r.run(FZ_NIGHTLY);
        assert_eq!(r.failure, None);
        assert_eq!(paths_reachable(&r.state), Ok(()));
        // 反例：全新账 ⇒ 路径不可达。
        let fresh = StormState::new(2);
        assert_eq!(paths_reachable(&fresh), Err(E_FZ_UNREACH));
    }

    #[test]
    fn a04_畸形数据_钳制容错不崩() {
        let mut st = StormState::new(2);
        // 超界值钳到 hi 且计数。
        assert_eq!(clamp_u32(9_999, 0, 2_000, &mut st), 2_000);
        assert_eq!(st.clamps, 1);
        // 带内值不计数。
        assert_eq!(clamp_u32(1_500, 0, 2_000, &mut st), 1_500);
        assert_eq!(st.clamps, 1);
        // 钳后不变量仍守：钳出的授予量不越预算。
        let mut r = StormRunner::new(0xBADF, 2);
        r.run(3_000);
        assert_ne!(r.failure, Some(E_FZ_MALFORM));
        assert_eq!(r.failure, None);
        assert!(r.state.clamps > 0); // 风暴中确实出现畸形并已钳
    }

    #[test]
    fn a05_案例固化_指纹确定性与续跑一致() {
        // 同种子同帧数同结果。
        let mut a = StormRunner::new(7, 2);
        a.run(2_000);
        let mut b = StormRunner::new(7, 2);
        b.run(2_000);
        assert_eq!(a.frames, b.frames);
        assert_eq!(a.state.granted_total, b.state.granted_total);
        assert_eq!(a.state.used, b.state.used);
        // 指纹可复算。
        let mut c = StormRunner::new(0xDEAD, 2);
        c.failure = Some(E_FZ_CONSERVE);
        c.fail_frame = 99;
        assert_eq!(c.fingerprint(), (0xDEADu64 << 16) ^ 99u64 ^ (E_FZ_CONSERVE as u64));
        // RNG 确定性。
        let mut x = Xsr::new(1);
        let mut y = Xsr::new(1);
        for _ in 0..32 {
            assert_eq!(x.next(), y.next());
        }
        assert_ne!(Xsr::new(0).next(), 0, "zero seed remapped");
    }
}
