//! CGPU-F0024 · 多线程栅格化负载均衡（cga24_loadbal）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F0024`
//! **判据原文摘录**：预测准确率、窃取自适应、尾延迟改善实测、利用率指标、
//! 确定性保持。
//!
//! 瓦片工作量极不均：天空瓦片几微秒，复杂几何瓦片可能是它的几十倍。均分
//! 瓦片数不等于均分工作量——按「瓦片数/k」切块时，尾延迟由**最重的那块**
//! 决定，与总工作量无关。本模块三段：
//!
//! 1. **工作量预测**：上一帧每瓦片耗时做本帧预测（EWMA，新帧权重可调）。
//!    实测口径是「平均绝对误差」，并与「用全帧均值的常数预测」对比——只有
//!    赢过常数预测，预测才算真的带来了信息。
//! 2. **窃取粒度自适应**：工作量离散度（相对标准差）超阈值就缩小粒度，
//!    让空闲线程能捡到更细的活；均匀负载下保持粗粒度以免调度开销吃掉收益。
//! 3. **尾延迟治理**：最后 5% 瓦片合并成一次等待，避免「最后一个重瓦片
//!    拖住所有线程而每个线程还在逐瓦片等」。
//!
//! 指标：线程利用率标准差 ≤10%（相对均值的千分比 ≤100‰）。判据用 LPT
//! （最长优先）与朴素均分的 makespan 对比来量化「尾延迟改善」，不靠嘴。
//!
//! 确定性：同输入必须产出同分配。LPT 的排序键是 **(成本降序, 序号升序)**，
//! 装填时选**编号最小**的最轻线程——两个打破平局的规则都写死，否则并行
//! 调度器会在两帧之间给出不同分配，让「同场景复现」变成不可能。
//!
//! 错误路径（0xD1 段）：`E_LB_EMPTY` 空负载/零线程 / `E_LB_SHAPE` 长度不符 /
//! `E_LB_RANGE` 参数越界。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0xD100;
/// 空负载或零线程。
pub const E_LB_EMPTY: u16 = SEG_BASE + 1;
/// 长度不符（实际成本与预测长度不等）。
pub const E_LB_SHAPE: u16 = SEG_BASE + 2;
/// 参数越界（权重/百分比/尾比例超范围）。
pub const E_LB_RANGE: u16 = SEG_BASE + 3;

/// 尾比例（百分比）：最后 5% 的瓦片进合并等待。
pub const TAIL_PCT: u32 = 5;
/// 粒度切换的相对标准差阈值（千分比）。
pub const GRAIN_STD_PERMILLE: u32 = 200;
/// 利用率标准差目标（千分比）——锚点 10%。
pub const UTIL_STD_BUDGET: u32 = 100;

// ---------------------------------------------------------------------------
// 整数平方根（no_std 无浮点，统计量必须自己算）
// ---------------------------------------------------------------------------

/// 整数平方根（向下取整）。牛顿迭代，收敛快于逐位法且无需查表。
pub fn isqrt_u64(v: u64) -> u64 {
    if v < 2 {
        return v;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

/// 负载的均值（千分之一的定点，避免整除丢精度后标准差被算小）。
fn mean_milli(loads: &[u64]) -> u64 {
    if loads.is_empty() {
        return 0;
    }
    let mut s = 0u64;
    for &x in loads {
        s += x;
    }
    s * 1000 / loads.len() as u64
}

/// 负载的相对标准差（千分比，相对于均值）。
///
/// 空负载返回 0；均值为 0（全零负载）也返回 0——全零时线程完全一致，
/// 报「离散度无穷大」只会让自适应误判成需要细粒度。
pub fn util_std_permille(loads: &[u64]) -> u32 {
    if loads.is_empty() {
        return 0;
    }
    let m = mean_milli(loads);
    if m == 0 {
        return 0;
    }
    // 方差在「千分比」域算：sum((x*1000 - m)^2)/n，再开方后除以均值。
    let mut acc: u64 = 0;
    for &x in loads {
        let d = (x * 1000) as i64 - m as i64;
        // 差值可能为负，平方后无符号；先取绝对值再乘，避开 i64 溢出风险。
        let ad = if d < 0 { (-d) as u64 } else { d as u64 };
        acc += ad.saturating_mul(ad);
    }
    let var = acc / loads.len() as u64;
    let sd = isqrt_u64(var); // 单位：千分之一
    (sd * 1000 / m) as u32
}

// ---------------------------------------------------------------------------
// 工作量预测
// ---------------------------------------------------------------------------

/// 上一帧 / 上上帧的每瓦片耗时（微秒）。
///
/// 用 EWMA 而不是「照抄上一帧」：瓦片成本随视角变化而漂移，纯照抄在滑动的
/// 场景里会一直滞后一帧；纯均值又会把突发重瓦片抹成平均，让调度把这批瓦片
/// 均匀撒开、最后一起撞在尾部。
pub struct Predictor {
    prev: Vec<u16>,
    prev2: Vec<u16>,
    /// 新帧权重（0..=4）：pred = (w*prev + (4-w)*prev2) / 4。
    w_new: u8,
    frames: u32,
}

impl Predictor {
    /// 新建。`w_new > 4` 拒绝（权重超出分母就没有意义）。
    pub fn new(w_new: u8) -> Result<Predictor, u16> {
        if w_new > 4 {
            return Err(E_LB_RANGE);
        }
        Ok(Predictor {
            prev: Vec::new(),
            prev2: Vec::new(),
            w_new,
            frames: 0,
        })
    }

    /// 观测一帧（长度变化时整体重置——瓦片数变了，历史不再可比）。
    pub fn observe(&mut self, costs: &[u16]) {
        if self.prev.len() != costs.len() {
            self.prev.clear();
            self.prev2.clear();
            self.prev2.extend_from_slice(costs);
            self.prev.extend_from_slice(costs);
        } else {
            self.prev2.clear();
            self.prev2.extend_from_slice(&self.prev);
            self.prev.clear();
            self.prev.extend_from_slice(costs);
        }
        self.frames = self.frames.saturating_add(1);
    }

    /// 观测帧数。
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// 本帧预测（长度不足时以 1µs 兜底，绝不给 0——0 会让 LPT 把它排到最后，
    /// 一批「预测为 0」的重瓦片会同时压到尾部）。
    pub fn predict(&self, n: usize) -> Vec<u16> {
        let w = self.w_new as u32;
        let mut v = Vec::with_capacity(n);
        for i in 0..n {
            let a = *self.prev.get(i).unwrap_or(&1) as u32;
            let b = *self.prev2.get(i).unwrap_or(&a) as u32;
            let p = (w * a + (4 - w) * b) / 4;
            v.push(if p == 0 { 1 } else { p.min(65_535) as u16 });
        }
        v
    }
}

/// 平均绝对误差（µs）。
pub fn predict_error(pred: &[u16], actual: &[u16]) -> Result<u32, u16> {
    if pred.len() != actual.len() {
        return Err(E_LB_SHAPE);
    }
    if actual.is_empty() {
        return Err(E_LB_EMPTY);
    }
    let mut s = 0u64;
    for i in 0..actual.len() {
        let d = pred[i] as i64 - actual[i] as i64;
        s += if d < 0 { (-d) as u64 } else { d as u64 };
    }
    Ok((s / actual.len() as u64) as u32)
}

/// 常数预测（用全帧均值）——预测器的对照组。
pub fn const_predict(actual: &[u16]) -> Result<Vec<u16>, u16> {
    if actual.is_empty() {
        return Err(E_LB_EMPTY);
    }
    let mut s = 0u64;
    for &x in actual {
        s += x as u64;
    }
    let m = (s / actual.len() as u64) as u16;
    Ok(vec![m; actual.len()])
}

// ---------------------------------------------------------------------------
// 分配
// ---------------------------------------------------------------------------

/// 每线程分到的瓦片序号（序号升序，便于回放）。
pub type Assign = Vec<Vec<u32>>;

/// 每线程负载。
pub fn loads_of(costs: &[u16], a: &Assign) -> Vec<u64> {
    let mut v = Vec::with_capacity(a.len());
    for set in a.iter() {
        let mut s = 0u64;
        for &t in set.iter() {
            if let Some(c) = costs.get(t as usize) {
                s += *c as u64;
            }
        }
        v.push(s);
    }
    v
}

/// 朴素均分：按序号连续切块（当前实现的做法，也是被对拍的对象）。
pub fn chunk_assign(costs: &[u16], workers: usize) -> Result<Assign, u16> {
    if costs.is_empty() || workers == 0 {
        return Err(E_LB_EMPTY);
    }
    let w = workers.min(costs.len());
    let mut a: Assign = vec![Vec::new(); w];
    let base = costs.len() / w;
    let rem = costs.len() % w;
    let mut at = 0u32;
    for i in 0..w {
        let n = base + if i < rem { 1 } else { 0 };
        for _ in 0..n {
            a[i].push(at);
            at += 1;
        }
    }
    Ok(a)
}

/// LPT（Longest Processing Time first）：最重的先发，每次塞进当前最轻的线程。
///
/// 平局规则全部写死：排序按 **(成本降序, 序号升序)**，选线程按 **编号最小**
/// 的最轻者。少了任何一条，多线程下的「同输入同分配」就不成立——而不可复现
/// 的调度等于不可调试的尾延迟。
pub fn lpt_assign(costs: &[u16], workers: usize) -> Result<Assign, u16> {
    if costs.is_empty() || workers == 0 {
        return Err(E_LB_EMPTY);
    }
    let w = workers.min(costs.len());
    let mut order: Vec<u32> = Vec::with_capacity(costs.len());
    for i in 0..costs.len() {
        order.push(i as u32);
    }
    // 稳定插入排序：成本降序、序号升序（避免分配排序引入不确定）。
    let mut k = 1usize;
    while k < order.len() {
        let cur = order[k];
        let mut j = k;
        while j > 0 {
            let prev = order[j - 1];
            let swap = costs[prev as usize] < costs[cur as usize];
            if swap {
                order[j] = prev;
                j -= 1;
            } else {
                break;
            }
        }
        order[j] = cur;
        k += 1;
    }
    let mut a: Assign = vec![Vec::new(); w];
    let mut load = vec![0u64; w];
    for &t in order.iter() {
        let mut pick = 0usize;
        let mut best = load[0];
        for i in 1..w {
            if load[i] < best {
                best = load[i];
                pick = i;
            }
        }
        a[pick].push(t);
        load[pick] += costs[t as usize] as u64;
    }
    for set in a.iter_mut() {
        set.sort_unstable();
    }
    Ok(a)
}

/// makespan（最大线程负载）。
pub fn makespan(costs: &[u16], a: &Assign) -> u64 {
    let l = loads_of(costs, a);
    let mut m = 0u64;
    for &x in l.iter() {
        if x > m {
            m = x;
        }
    }
    m
}

/// makespan 下界：`max(总工作量/线程数, 最重瓦片)`。
pub fn makespan_lower_bound(costs: &[u16], workers: usize) -> u64 {
    if costs.is_empty() || workers == 0 {
        return 0;
    }
    let mut total = 0u64;
    let mut heavy = 0u64;
    for &c in costs.iter() {
        total += c as u64;
        if c as u64 > heavy {
            heavy = c as u64;
        }
    }
    let avg = (total + workers as u64 - 1) / workers as u64;
    if avg > heavy {
        avg
    } else {
        heavy
    }
}

// ---------------------------------------------------------------------------
// 窃取粒度自适应
// ---------------------------------------------------------------------------

/// 窃取粒度。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grain {
    /// 粗粒度：整块窃取（均匀负载下调度开销最小）。
    Coarse,
    /// 细粒度：拆到瓦片级（偏斜负载下才值得）。
    Fine,
}

/// 选粒度：工作量相对标准差超阈值就切细。
pub fn pick_grain(costs: &[u16]) -> Result<Grain, u16> {
    if costs.is_empty() {
        return Err(E_LB_EMPTY);
    }
    let mut l: Vec<u64> = Vec::with_capacity(costs.len());
    for &c in costs.iter() {
        l.push(c as u64);
    }
    if util_std_permille(&l) > GRAIN_STD_PERMILLE {
        Ok(Grain::Fine)
    } else {
        Ok(Grain::Coarse)
    }
}

/// 粒度对应的窃取单位（瓦片数）。
pub fn steal_unit(g: Grain, tiles: usize, workers: usize) -> usize {
    let w = workers.max(1);
    match g {
        Grain::Coarse => (tiles + w - 1) / w,
        Grain::Fine => {
            let u = (tiles / (w * 8)).max(1);
            u
        }
    }
}

// ---------------------------------------------------------------------------
// 尾延迟治理
// ---------------------------------------------------------------------------

/// 尾合并计划。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TailPlan {
    /// 头部瓦片数（正常分发）。
    pub head: usize,
    /// 尾部瓦片数（合并等待）。
    pub tail: usize,
    /// 尾部被合成几块分发。
    pub tail_chunks: usize,
    /// 等待操作数（合并后应当只有 1 次整批等待）。
    pub wait_ops: usize,
}

/// 生成尾合并计划：最后 `pct` 百分比的瓦片合成一批，只发一次等待。
///
/// 朴素做法是每瓦片一次等待——线程在尾部要醒来 `tail` 次，每次都可能重新
/// 争锁；合并后只醒一次，且尾块数不超过线程数，保证「没有线程闲着看别人收尾」。
pub fn tail_plan(total: usize, workers: usize, pct: u32) -> Result<TailPlan, u16> {
    if total == 0 || workers == 0 {
        return Err(E_LB_EMPTY);
    }
    if pct == 0 || pct > 100 {
        return Err(E_LB_RANGE);
    }
    let tail = (total * pct as usize + 99) / 100;
    let tail = tail.max(1).min(total);
    let head = total - tail;
    let w = workers.min(total);
    let tail_chunks = tail.min(w).max(1);
    Ok(TailPlan {
        head,
        tail,
        tail_chunks,
        wait_ops: 1,
    })
}

// ---------------------------------------------------------------------------
// 一站式调度
// ---------------------------------------------------------------------------

/// 一帧的调度结论。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Schedule {
    pub assign: Assign,
    pub grain: Grain,
    pub unit: usize,
    pub tail: TailPlan,
    pub makespan: u64,
    pub lower_bound: u64,
    pub util_std_permille: u32,
}

/// 由预测成本生成调度。
pub fn plan(costs: &[u16], workers: usize) -> Result<Schedule, u16> {
    let assign = lpt_assign(costs, workers)?;
    let grain = pick_grain(costs)?;
    let unit = steal_unit(grain, costs.len(), workers);
    let tail = tail_plan(costs.len(), workers, TAIL_PCT)?;
    let ms = makespan(costs, &assign);
    let lb = makespan_lower_bound(costs, workers);
    let loads = loads_of(costs, &assign);
    Ok(Schedule {
        assign,
        grain,
        unit,
        tail,
        makespan: ms,
        lower_bound: lb,
        util_std_permille: util_std_permille(&loads),
    })
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// 一组「天空 + 几何」混合的真实形态成本（µs）。
fn mixed_cost() -> Vec<u16> {
    // 64 瓦片：48 个轻（2~4µs）、12 个中（10µs）、4 个重（60µs）
    let mut v = Vec::with_capacity(64);
    for i in 0..48u16 {
        v.push(2 + (i % 3));
    }
    for _ in 0..12 {
        v.push(10);
    }
    for _ in 0..4 {
        v.push(60);
    }
    v
}

/// run_cga24_checks：CGPU-F0024 判据聚合（0xD1 段）。
pub fn run_cga24_checks() -> CheckSet {
    let mut s = CheckSet::new("CGPU-F0024 负载均衡");

    // 正-01：EWMA 预测在「重复形态」上误差为零（两帧同形 → 预测精确）
    {
        let p0 = mixed_cost();
        let mut p = match Predictor::new(2) {
            Ok(x) => x,
            Err(_) => {
                s.fail("F0024-正-01-重复形态零误差", "预测器构造失败");
                return s;
            }
        };
        p.observe(&p0);
        p.observe(&p0);
        let pred = p.predict(p0.len());
        let err = predict_error(&pred, &p0).unwrap_or(u32::MAX);
        if err == 0 && p.frames() == 2 && pred.len() == p0.len() {
            s.ok("F0024-正-01-重复形态零误差");
        } else {
            s.fail("F0024-正-01-重复形态零误差", "同形两帧后预测仍有误差");
        }
    }

    // 正-02：预测准确率——必须赢过「用全帧均值」的常数预测
    {
        // 形态缓慢漂移：瓦片成本按帧号线性抬升（视角推近）
        let base = mixed_cost();
        let mut actual: Vec<u16> = Vec::with_capacity(base.len());
        for i in 0..base.len() {
            actual.push(base[i].saturating_add((i % 7) as u16));
        }
        let mut p = Predictor::new(3).expect("预测器");
        p.observe(&base);
        p.observe(&base);
        let pred = p.predict(actual.len());
        let e1 = predict_error(&pred, &actual).unwrap_or(u32::MAX);
        let e2 = match const_predict(&actual) {
            Ok(c) => predict_error(&c, &actual).unwrap_or(u32::MAX),
            Err(_) => u32::MAX,
        };
        // 形态里重瓦片（60µs）远高于均值（~9µs），常数预测必然被拖累
        if e1 < e2 && e2 > 0 {
            s.ok("F0024-正-02-预测赢过常数预测");
        } else {
            s.fail("F0024-正-02-预测赢过常数预测", "EWMA 未优於全帧均值");
        }
    }

    // 正-03：长度变化时预测器整体重置（瓦片数变了，历史不可比）
    {
        let mut p = Predictor::new(2).expect("预测器");
        p.observe(&[10, 20, 30]);
        p.observe(&[5, 6]);
        let pred = p.predict(2);
        if pred == vec![5u16, 6] {
            s.ok("F0024-正-03-长度变化重置历史");
        } else {
            s.fail("F0024-正-03-长度变化重置历史", "跨长度混用了旧历史");
        }
    }

    // 正-04：尾延迟改善实测——LPT 的 makespan 必须显著低于朴素均分
    {
        let c = mixed_cost();
        let w = 8usize;
        let a1 = match chunk_assign(&c, w) {
            Ok(x) => x,
            Err(_) => {
                s.fail("F0024-正-04-尾延迟改善", "均分失败");
                return s;
            }
        };
        let a2 = lpt_assign(&c, w).expect("lpt");
        let m1 = makespan(&c, &a1);
        let m2 = makespan(&c, &a2);
        // 下界：总工作量 / 8
        let lb = makespan_lower_bound(&c, w);
        // 朴素均分把 4 个 60µs 全塞进最后一块 → 至少 240µs；LPT 应接近下界
        if m2 < m1 && m2 <= lb + lb / 20 {
            s.ok("F0024-正-04-尾延迟改善");
        } else {
            s.fail("F0024-正-04-尾延迟改善", "LPT makespan 未显著优于均分");
        }
    }

    // 正-05：利用率指标——LPT 后线程负载相对标准差必须在 10% 以内
    {
        let c = mixed_cost();
        let w = 8usize;
        let a = lpt_assign(&c, w).expect("lpt");
        let loads = loads_of(&c, &a);
        let std = util_std_permille(&loads);
        let a1 = chunk_assign(&c, w).expect("chunk");
        let std1 = util_std_permille(&loads_of(&c, &a1));
        if std <= UTIL_STD_BUDGET && std1 > UTIL_STD_BUDGET {
            s.ok("F0024-正-05-利用率标准差达标");
        } else {
            s.fail("F0024-正-05-利用率标准差达标", "LPT 利用率未达标或对照组未超标");
        }
    }

    // 正-06：窃取粒度自适应——均匀负载粗粒度、偏斜负载细粒度
    {
        let uniform = vec![10u16; 64];
        let skew = mixed_cost();
        let g1 = pick_grain(&uniform);
        let g2 = pick_grain(&skew);
        let ok = g1 == Ok(Grain::Coarse) && g2 == Ok(Grain::Fine);
        // 粒度单位：细粒度必须严格小于粗粒度，且至少 1
        let u1 = steal_unit(Grain::Coarse, 64, 8);
        let u2 = steal_unit(Grain::Fine, 64, 8);
        if ok && u2 >= 1 && u2 < u1 {
            s.ok("F0024-正-06-窃取粒度自适应");
        } else {
            s.fail("F0024-正-06-窃取粒度自适应", "粒度未按离散度切换");
        }
    }

    // 正-07：尾合并——覆盖守恒、尾块数不超过线程数、等待只有一次
    {
        let mut ok = true;
        for total in [1usize, 4, 20, 64, 1000] {
            for workers in [1usize, 2, 8, 64] {
                match tail_plan(total, workers, TAIL_PCT) {
                    Ok(t) => {
                        if t.head + t.tail != total {
                            ok = false;
                        }
                        if t.tail_chunks == 0 || t.tail_chunks > workers.min(total) {
                            ok = false;
                        }
                        if t.wait_ops != 1 {
                            ok = false;
                        }
                        // 5% 向上取整，至少 1 片
                        let want = ((total * 5 + 99) / 100).max(1).min(total);
                        if t.tail != want {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
            }
        }
        // 尾部 0% / 101% 必须拒绝
        if tail_plan(10, 4, 0) != Err(E_LB_RANGE) || tail_plan(10, 4, 101) != Err(E_LB_RANGE) {
            ok = false;
        }
        if ok {
            s.ok("F0024-正-07-尾合并计划守恒");
        } else {
            s.fail("F0024-正-07-尾合并计划守恒", "尾合并覆盖或等待数不符");
        }
    }

    // 正-08：确定性——同输入两次调度必须逐字段相同
    {
        let c = mixed_cost();
        let a = plan(&c, 8);
        let b = plan(&c, 8);
        let ok = a.is_ok() && a == b;
        if ok {
            s.ok("F0024-正-08-调度确定性");
        } else {
            s.fail("F0024-正-08-调度确定性", "同输入两次调度不同");
        }
    }

    // 正-09：线程数多于瓦片数时不得产生空线程（空线程会污染利用率统计）
    {
        let c = vec![7u16, 9, 11];
        let a = lpt_assign(&c, 8);
        let b = chunk_assign(&c, 8);
        let ok = match (a, b) {
            (Ok(x), Ok(y)) => {
                x.len() == 3
                    && y.len() == 3
                    && x.iter().all(|v| v.len() == 1)
                    && y.iter().all(|v| v.len() == 1)
            }
            _ => false,
        };
        if ok {
            s.ok("F0024-正-09-线程多于瓦片退化");
        } else {
            s.fail("F0024-正-09-线程多于瓦片退化", "产生了空线程或丢瓦片");
        }
    }

    // 正-10：全分配覆盖守恒——每个瓦片恰好出现在一个线程里
    {
        let c = mixed_cost();
        let mut ok = true;
        for w in [1usize, 3, 8, 16] {
            for a in [chunk_assign(&c, w), lpt_assign(&c, w)] {
                match a {
                    Ok(assign) => {
                        let mut seen = vec![0u8; c.len()];
                        let mut n = 0usize;
                        for set in assign.iter() {
                            for &t in set.iter() {
                                let i = t as usize;
                                if i >= c.len() {
                                    ok = false;
                                } else {
                                    if seen[i] != 0 {
                                        ok = false;
                                    }
                                    seen[i] = 1;
                                    n += 1;
                                }
                            }
                        }
                        if n != c.len() {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
            }
        }
        if ok {
            s.ok("F0024-正-10-分配覆盖守恒");
        } else {
            s.fail("F0024-正-10-分配覆盖守恒", "瓦片重复分发或漏发");
        }
    }

    // 编-01：标准差手算锚（[10,10,10]→0‰；[8,12]→200‰；[0,20]→1000‰）
    {
        let a = util_std_permille(&[10, 10, 10]);
        let b = util_std_permille(&[8, 12]);
        let c = util_std_permille(&[0, 20]);
        let d = util_std_permille(&[]);
        let e = util_std_permille(&[0, 0, 0]);
        if a == 0 && b == 200 && c == 1000 && d == 0 && e == 0 {
            s.ok("F0024-编-01-标准差手算锚");
        } else {
            s.fail("F0024-编-01-标准差手算锚", "标准差口径与手算不符");
        }
    }

    // 编-02：整数开方自洽（k² ≤ isqrt(k²+r) < (k+1)²，k 扫到 8 位）
    {
        let mut ok = true;
        let mut k = 0u64;
        while k < 4096 {
            let sq = k * k;
            if isqrt_u64(sq) != k {
                ok = false;
            }
            if isqrt_u64(sq + 2 * k) != k {
                ok = false;
            }
            if isqrt_u64(sq + 2 * k + 1) != k + 1 {
                ok = false;
            }
            k += 1;
        }
        if ok {
            s.ok("F0024-编-02-整数开方自洽");
        } else {
            s.fail("F0024-编-02-整数开方自洽", "开方在完全平方附近错位");
        }
    }

    // 坏-01：空负载/零线程/权重越界/长度不符的拒绝路径
    {
        let a = lpt_assign(&[], 4);
        let b = lpt_assign(&[1u16], 0);
        let c = Predictor::new(5).is_err();
        let d = predict_error(&[1u16, 2], &[1u16]);
        let e = pick_grain(&[]);
        if a == Err(E_LB_EMPTY)
            && b == Err(E_LB_EMPTY)
            && c
            && d == Err(E_LB_SHAPE)
            && e == Err(E_LB_EMPTY)
        {
            s.ok("F0024-坏-01-拒绝路径码位分离");
        } else {
            s.fail("F0024-坏-01-拒绝路径码位分离", "拒绝码混用或漏检");
        }
    }

    s
}
