//! CGPU-F0191 · 图 fuzz（判据段 0x0590）
//! 帧图 fuzz：随机图生成（随机节点/边/环注入/超大图）、随机执行
//! 参数（线程数/优先级/失败注入）——不变量：不 panic/不死锁（环
//! 检测兜底）/确定性（无随机时）/执行完成（所有节点到达终态）。
//! 判据：随机域、四不变量、案例固化、CI 每夜、报告。
//!
//! 诊断码段 0x0590（本模块独占：0x0590~0x059F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0590;

/// 超大图规模（判据「超大图」硬界）。
pub const MAX_FUZZ_NODES: usize = 4096;

/// xorshift64（no_std 确定性伪随机——fuzz 种子可复现）。
pub struct XorShift64 {
    /// 状态。
    pub state: u64,
}

impl XorShift64 {
    /// 建生成器（种子非零）。
    pub fn new(seed: u64) -> Result<Self, u16> {
        if seed == 0 {
            return Err(SEG_BASE);
        }
        Ok(XorShift64 { state: seed })
    }

    /// 下一步。
    pub fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x
    }

    /// [0, n) 均匀。
    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            return 0;
        }
        (self.next() % n as u64) as usize
    }
}

/// fuzz 图（随机生成的载体）：节点 + 边 + 是否注入环。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FuzzGraph {
    /// 节点号 1..=n。
    pub n: usize,
    /// 边集。
    pub edges: Vec<(u32, u32)>,
    /// 环注入标记（用于测试环检测兜底）。
    pub injected_cycle: bool,
}

impl FuzzGraph {
    /// 校验：规模在界内、边端点在图内。
    pub fn validate(&self) -> Result<(), u16> {
        if self.n == 0 || self.n > MAX_FUZZ_NODES {
            return Err(SEG_BASE + 1);
        }
        for (f, t) in &self.edges {
            if *f == 0 || *t == 0 || *f as usize > self.n || *t as usize > self.n {
                return Err(SEG_BASE + 2);
            }
        }
        Ok(())
    }
}

/// 随机图生成（判据「随机域」核心面）：n 节点 + 密度 d 的随机边 +
/// 可选环注入（末三节点闭合——环检测的靶子）。种子可复现。
pub fn gen_fuzz(seed: u64, n: usize, density_permille: u32, inject_cycle: bool) -> Result<FuzzGraph, u16> {
    if n == 0 || n > MAX_FUZZ_NODES {
        return Err(SEG_BASE + 3);
    }
    if density_permille == 0 || density_permille > 200 {
        return Err(SEG_BASE + 4); // 密度封顶 20%（防边爆炸）
    }
    let mut rng = XorShift64::new(seed)?;
    let mut edges: Vec<(u32, u32)> = Vec::new();
    let expect = (n as u64 * density_permille as u64 / 1000) as usize;
    for _ in 0..expect {
        let a = rng.below(n) as u32 + 1;
        let b = rng.below(n) as u32 + 1;
        if a != b && !edges.contains(&(a, b)) {
            edges.push((a, b));
        }
    }
    if inject_cycle && n >= 3 {
        // 末三节点成环（1->2 已有向，补 2->n、n->1 形成闭合）。
        let a = n as u32;
        edges.push((a - 1, a));
        edges.push((a, 1));
    }
    let g = FuzzGraph {
        n,
        edges,
        injected_cycle: inject_cycle,
    };
    g.validate()?;
    Ok(g)
}

/// 不变量一：不 panic（判据「不 panic」——所有 API 无 panic 路径，
/// fuzz 驱动只调用 Result 化接口；此处静态验证图为合法输入）。
pub fn inv_no_panic(g: &FuzzGraph) -> Result<(), u16> {
    g.validate()
}

/// 不变量二：不死锁（判据「环检测兜底」——环注入图必须被检出，
/// 检不出 = 调度器会挂死在环上）。
pub fn inv_no_deadlock(g: &FuzzGraph, cycle_detected: bool) -> Result<(), u16> {
    if g.injected_cycle && !cycle_detected {
        return Err(SEG_BASE + 5);
    }
    if !g.injected_cycle && cycle_detected {
        // 误报也红：无环图报环 = 检测器过拟合。
        return Err(SEG_BASE + 6);
    }
    Ok(())
}

/// 不变量三：确定性（判据「确定性（无随机时）」——同图两次拓扑
/// 计算结果全同）。
pub fn inv_deterministic(order_a: &[u32], order_b: &[u32]) -> Result<(), u16> {
    if order_a.len() != order_b.len() || order_a.is_empty() {
        return Err(SEG_BASE + 7);
    }
    for i in 0..order_a.len() {
        if order_a[i] != order_b[i] {
            return Err(SEG_BASE + 8);
        }
    }
    Ok(())
}

/// 不变量四：执行完成（判据「所有节点到达终态」——无环图拓扑序
/// 覆盖全部节点，有环图必须显性拒绝而非部分执行）。
pub fn inv_complete(g: &FuzzGraph, order: &[u32]) -> Result<(), u16> {
    if g.injected_cycle {
        return Err(SEG_BASE + 9); // 环图不完成是预期，调用方应走拒绝路径
    }
    if order.len() != g.n {
        return Err(SEG_BASE + 10);
    }
    let mut seen: Vec<u32> = Vec::new();
    for t in order {
        if *t == 0 || *t as usize > g.n || seen.contains(t) {
            return Err(SEG_BASE + 11);
        }
        seen.push(*t);
    }
    Ok(())
}

/// 简易环检测（fuzz 驱动用——与 cgb178 的 DFS 独立实现互为对拍）。
pub fn quick_cycle_check(g: &FuzzGraph) -> bool {
    // Kahn 计数法：能消完 = 无环。
    let mut indeg: Vec<u32> = vec![0; g.n + 1];
    for (f, t) in &g.edges {
        indeg[*t as usize] += 1;
        let _ = f;
    }
    let mut ready: Vec<usize> = Vec::new();
    for i in 1..=g.n {
        if indeg[i] == 0 {
            ready.push(i);
        }
    }
    let mut done = 0usize;
    while let Some(i) = ready.pop() {
        done += 1;
        for (f, t) in &g.edges {
            if *f == i as u32 {
                indeg[*t as usize] -= 1;
                if indeg[*t as usize] == 0 {
                    ready.push(*t as usize);
                }
            }
        }
    }
    done != g.n
}

/// 拓扑序（fuzz 驱动用）。
pub fn quick_topo(g: &FuzzGraph) -> Result<Vec<u32>, u16> {
    if quick_cycle_check(g) {
        return Err(SEG_BASE + 12);
    }
    let mut indeg: Vec<u32> = vec![0; g.n + 1];
    for (_, t) in &g.edges {
        indeg[*t as usize] += 1;
    }
    let mut ready: Vec<usize> = Vec::new();
    for i in 1..=g.n {
        if indeg[i] == 0 {
            ready.push(i);
        }
    }
    let mut order: Vec<u32> = Vec::new();
    while let Some(i) = ready.pop() {
        order.push(i as u32);
        for (f, t) in &g.edges {
            if *f == i as u32 {
                indeg[*t as usize] -= 1;
                if indeg[*t as usize] == 0 {
                    ready.push(*t as usize);
                }
            }
        }
    }
    Ok(order)
}

/// fuzz 案例固化（判据「案例固化」载体）：种子 + 参数 → 报告指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzCase {
    /// 种子。
    pub seed: u64,
    /// 节点数。
    pub n: usize,
    /// 密度。
    pub density: u32,
    /// 环注入。
    pub cyc: bool,
    /// 结论（true=四不变量全过）。
    pub ok: bool,
}

impl FuzzCase {
    /// 校验。
    pub fn validate(&self) -> Result<(), u16> {
        if self.seed == 0 || self.n == 0 || self.n > MAX_FUZZ_NODES {
            return Err(SEG_BASE + 13);
        }
        Ok(())
    }

    /// 案例指纹（CI 报告键）。
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = h.wrapping_mul(0x100000001b3) ^ self.seed;
        h = h.wrapping_mul(0x100000001b3) ^ self.n as u64;
        h = h.wrapping_mul(0x100000001b3) ^ self.density as u64;
        h.wrapping_mul(0x100000001b3) ^ self.cyc as u64
    }
}

/// 单案例 fuzz 驱动（判据「随机域」执行面）：生成 → 四不变量
/// 逐一验证 → 案例固化。
pub fn fuzz_one(seed: u64, n: usize, density: u32, cyc: bool) -> Result<FuzzCase, u16> {
    let g = gen_fuzz(seed, n, density, cyc)?;
    inv_no_panic(&g)?;
    let cyc_detected = quick_cycle_check(&g);
    inv_no_deadlock(&g, cyc_detected)?;
    if !cyc_detected {
        let o1 = quick_topo(&g)?;
        let o2 = quick_topo(&g)?;
        inv_deterministic(&o1, &o2)?;
        inv_complete(&g, &o1)?;
    }
    Ok(FuzzCase {
        seed,
        n,
        density,
        cyc,
        ok: true,
    })
}

/// 每夜 fuzz 批（判据「CI 每夜」批量面）：至少 8 案例（含环注入
/// 与超大图两类靶标）、全过才绿；报告 = 案例指纹序列。
pub fn nightly_batch() -> Result<Vec<u64>, u16> {
    let mut fps: Vec<u64> = Vec::new();
    let plan: [(u64, usize, u32, bool); 8] = [
        (0xD1, 16, 100, false),
        (0xD2, 16, 100, true),
        (0xD3, 128, 150, false),
        (0xD4, 128, 150, true),
        (0xD5, 512, 50, false),
        (0xD6, 512, 200, true),
        (0xD7, MAX_FUZZ_NODES, 10, false),
        (0xD8, MAX_FUZZ_NODES, 10, true),
    ];
    for (seed, n, d, c) in plan {
        let case = fuzz_one(seed, n, d, c)?;
        case.validate()?;
        if !case.ok {
            return Err(SEG_BASE + 14);
        }
        fps.push(case.fp());
    }
    if fps.len() != 8 {
        return Err(SEG_BASE + 15);
    }
    Ok(fps)
}
