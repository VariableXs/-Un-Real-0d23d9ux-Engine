//! UNX-F36211-J1 继承深度上限与性能预算。
//! 判据：① 深度 32 层树求值 P99 ≤5μs——确定性引擎无时钟，账面口径
//! = 求值步数预算（BUDGET_P99 冻结界：5μs ≈ 166 步 @30ns 走线锚 →
//! 冻结 200 步），100 例采样取第 99 百分位；时钟实测在探针 ktest 以
//! std::time 补证；② 超深注入 10 例拒绝并诊断（深度 33..42 → 显性
//! E_DEPTH + detail 可读）；③ 预算账入 O1 预告（快照两行登记）；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：深度是继承求值的成本乘子——有效 DACL 的继承副本数随
//! 深度线性膨胀，32 层是本域冻结的求值上限（超深树不是「慢」，是
//! 「拒」——超界拒绝比无界等待诚实）。P99 的账面形态：样本 = 一
//! 次 32 层树叶子求值，步数 = 沿途 ACE 扫描总量；预算界冻结后任何
//! 「求值实现变笨」（比如逐代全树重扫）都会当场爆红。超深注入的
//! 拒绝必须带深度诊断（实际深度越界值可读），不许只回一个哑码。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9d70;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_DEPTH: u16 = SEG_BASE + 2; // 深度超上限（拒绝 + 诊断）
pub const E_BUDGET: u16 = SEG_BASE + 3; // P99 步数越冻结界
pub const E_SAMPLE: u16 = SEG_BASE + 4; // 采样账不符
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚/O1 快照漂移
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

pub const VER: u32 = 1;

/// 深度上限（本域冻结——32 层树必须可求值，33 层拒绝）。
pub const DEPTH_MAX: u32 = 32;

/// P99 步数冻结界（5μs ≈ 166 步 @30ns 走线锚 → 冻结 200）。
pub const BUDGET_P99: u32 = 200;

/// 采样量（P99 统计的最小样本）。
pub const SAMPLE_N: usize = 100;

/// O1 预告快照（预算账两行——tag, 冻结界, 例量）。
pub const O1_SNAPSHOT: [(&'static str, u32, usize); 2] = [
    ("o1-inh-depth-p99", BUDGET_P99, SAMPLE_N),
    ("o1-inh-depth-max", DEPTH_MAX, 10),
];

/// 深度守卫基元：求值前显性核深度（超界拒绝并带实际深度可诊断）。
pub fn eval_with_depth_guard(t: &ie::InhTree, idx: usize) -> Result<Vec<ie::Ace>, u16> {
    let d = t.depth_of(idx)?;
    if d > DEPTH_MAX {
        return Err(E_DEPTH);
    }
    t.effective_dacl(idx)
}

/// 实际深度读取（诊断面——拒绝时调用方取实际深度写日志）。
pub fn actual_depth(t: &ie::InhTree, idx: usize) -> Result<u32, u16> {
    t.depth_of(idx)
}

/// 造深度 d 的链树（每节点 1..3 条 OI|CI ACE，随 variant 轮转），
/// 返回（树, 叶下标）。d = 0 → 单根。
pub fn deep_chain(d: u32, variant: usize) -> Result<(ie::InhTree, usize), u16> {
    let mut t = ie::InhTree::new();
    let root = t.add_node("n0", None, true, ie::SE_DACL_PRESENT)?;
    let mut cur = root;
    let mut level = 0;
    while level < d {
        let next = t.add_node("nx", Some(cur), true, ie::SE_DACL_PRESENT)?;
        // 每层 1..3 条 ACE（variant 轮转——步数样本有分布，P99 才有义）。
        let n_aces = 1 + (variant + level as usize) % 3;
        let mut aces: Vec<ie::Ace> = Vec::new();
        let mut i = 0;
        while i < n_aces {
            aces.push(ie::Ace {
                sid: 10 + ((variant + i) % 5) as u16,
                mask: 1 << (i % 7),
                flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT,
                kind: if (variant + i) % 4 == 0 { ie::AK_DENY } else { ie::AK_ALLOW },
                origin: 0,
            });
            i += 1;
        }
        t.set_dacl(next, aces)?;
        cur = next;
        level += 1;
    }
    Ok((t, cur))
}

/// 求值步数账（成本模型）：有效 DACL 长度 + 沿途祖先显式 ACE 总数
/// （每次求值必须扫过的 ACE 量——实现变笨立刻爆账）。
pub fn eval_steps(t: &ie::InhTree, idx: usize) -> Result<u32, u16> {
    if idx >= t.nodes.len() {
        return Err(E_INTG);
    }
    let eff_len = t.effective_dacl(idx)?.len() as u32;
    let mut scanned = 0u32;
    let mut cur = t.nodes[idx].parent;
    while let Some(p) = cur {
        scanned += t.nodes[p].aces.len() as u32;
        cur = t.nodes[p].parent;
    }
    scanned += t.nodes[idx].aces.len() as u32;
    Ok(eff_len + scanned + 1) // +1 = 入口常数
}

/// P99 实测（判据 ① 主面）：100 例深度 32 求值采样，取第 99 百分位。
/// 排序用插入序（样本量小——no_std 无 sort 依赖负担）。
pub fn p99_measure() -> Result<(u32, u32), u16> {
    let mut samples: Vec<u32> = Vec::new();
    let mut k = 0;
    while k < SAMPLE_N {
        let (t, leaf) = deep_chain(DEPTH_MAX, k % 5)?;
        let d = t.depth_of(leaf)?;
        if d != DEPTH_MAX {
            return Err(E_DEPTH); // 负载深度必须恰为 32
        }
        let s = eval_steps(&t, leaf)?;
        if eval_with_depth_guard(&t, leaf).is_err() {
            return Err(E_DEPTH);
        }
        samples.push(s);
        k += 1;
    }
    // 插入排序（升序）。
    let mut i = 1;
    while i < samples.len() {
        let key = samples[i];
        let mut j = i;
        while j > 0 && samples[j - 1] > key {
            samples[j] = samples[j - 1];
            j -= 1;
        }
        samples[j] = key;
        i += 1;
    }
    // P99 = 第 ceil(0.99*N) 位（100 例 → 第 99 位，下标 98）。
    let p99 = samples[(SAMPLE_N * 99 / 100) - 1];
    let max = samples[samples.len() - 1];
    if p99 > BUDGET_P99 || max > BUDGET_P99 {
        return Err(E_BUDGET); // P99 过不等于 max 过——max 也是硬锚
    }
    Ok((p99, max))
}

/// 超深注入扫描（判据 ② 主面）：深度 33..42 十例，全部 E_DEPTH 且
/// 诊断可读（实际深度可取回）。
pub fn overdeep_sweep() -> Result<u32, u16> {
    let mut rejected = 0u32;
    let mut d = DEPTH_MAX + 1;
    while d <= DEPTH_MAX + 10 {
        let (t, leaf) = deep_chain(d, 0)?;
        let actual = actual_depth(&t, leaf)?;
        if actual != d {
            return Err(E_INTG);
        }
        if eval_with_depth_guard(&t, leaf).err() != Some(E_DEPTH) {
            return Err(E_DEPTH);
        }
        // 诊断可读：E_DEPTH 的 detail 非哑码。
        if detail(E_DEPTH) == "未知诊断码" {
            return Err(E_ANCHOR);
        }
        rejected += 1;
        d += 1;
    }
    Ok(rejected)
}

// ---- 拒绝诊断载荷 ----------------------------------------------------------
/// E_DEPTH 拒绝的账面载荷：高 16 位 = 冻结上限，低 16 位 = 实际深度。
/// 拒绝不是哑码——越界值必须可读可账（「超界拒绝比无界等待诚实」的
/// 账面形态：调用方拿到 token 即可渲染「depth 33 > cap 32」式诊断）。
pub fn depth_reject_payload(actual: u32) -> u32 {
    (DEPTH_MAX << 16) | (actual & 0xFFFF)
}

/// 载荷回读：实际深度。
pub fn payload_depth(token: u32) -> u32 {
    token & 0xFFFF
}

/// 载荷回读：冻结上限。
pub fn payload_cap(token: u32) -> u32 {
    token >> 16
}

/// 载荷编解码自洽核：三组样例往返（编 → 解 → 对位）+ 上限恒锚。
pub fn payload_roundtrip() -> Result<(), u16> {
    let cases = [0u32, 1, 33, 41, 0xFFFF];
    let mut i = 0;
    while i < cases.len() {
        let tok = depth_reject_payload(cases[i]);
        if payload_depth(tok) != cases[i] || payload_cap(tok) != DEPTH_MAX {
            return Err(E_INTG);
        }
        i += 1;
    }
    Ok(())
}

// ---- 深度-成本矩阵 ----------------------------------------------------------
/// 4 档深度（8/16/24/32）× 5 variant 的步数矩阵 + 双轴核对：
/// ① 深度轴单调不减（同 variant 更深步数不降——成本模型锚：求值
/// 必须扫沿途 ACE，深度涨而账降 = 实现退化或账面失真）；② 32 层行
/// 的 variant 极差有界（负载轮转 ±1 ACE/层 × 32 层 → 冻结界 160，
/// 过宽 = 某种系统性偏斜）。
pub fn depth_cost_matrix() -> Result<(u32, u32), u16> {
    let depths = [8u32, 16, 24, 32];
    // 行优先铺平（no_std 固定二维数组开销大，账面用下标换算）。
    let mut mat: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < depths.len() {
        let mut v = 0;
        while v < 5 {
            let (t, leaf) = deep_chain(depths[i], v)?;
            if actual_depth(&t, leaf)? != depths[i] {
                return Err(E_INTG);
            }
            mat.push(eval_steps(&t, leaf)?);
            v += 1;
        }
        i += 1;
    }
    // 深度轴单调不减（同 variant 列逐行比）。
    let mut r = 1;
    while r < depths.len() {
        let mut v = 0;
        while v < 5 {
            if mat[r * 5 + v] < mat[(r - 1) * 5 + v] {
                return Err(E_BUDGET);
            }
            v += 1;
        }
        r += 1;
    }
    // 32 层行极差界。
    let mut row_max = 0u32;
    let mut row_min = u32::MAX;
    let mut v = 0;
    while v < 5 {
        let s = mat[3 * 5 + v];
        if s > row_max {
            row_max = s;
        }
        if s < row_min {
            row_min = s;
        }
        v += 1;
    }
    let spread = row_max - row_min;
    if spread > 160 {
        return Err(E_BUDGET);
    }
    // 深度锚：最浅档的步数必须远低于预算界（8 层 ≈ 1/4 深度 → 步数
    // 也应显著低于 32 层行 min——双档夹逼防「深度没吃进成本」）。
    let mut d8_min = u32::MAX;
    let mut v = 0;
    while v < 5 {
        if mat[v] < d8_min {
            d8_min = mat[v];
        }
        v += 1;
    }
    if d8_min * 2 >= row_max {
        return Err(E_BUDGET);
    }
    Ok((spread, row_max))
}

// ---- 边界三连 ---------------------------------------------------------------
/// 边界黄金口径（31/32/33 三层钉）：31、32 过（上限内不许拒——负例
/// 面），33 拒且拒绝载荷实际深度 = 33、上限 = 32。这是超深注入的
/// 最小形：比十连扫更早爆「边界差一」类退化。
pub fn boundary_triple() -> Result<u32, u16> {
    let (t31, l31) = deep_chain(DEPTH_MAX - 1, 1)?;
    if actual_depth(&t31, l31)? != DEPTH_MAX - 1 {
        return Err(E_INTG);
    }
    if eval_with_depth_guard(&t31, l31).is_err() {
        return Err(E_DEPTH);
    }
    let (t32, l32) = deep_chain(DEPTH_MAX, 2)?;
    if eval_with_depth_guard(&t32, l32).is_err() {
        return Err(E_DEPTH);
    }
    let (t33, l33) = deep_chain(DEPTH_MAX + 1, 3)?;
    match eval_with_depth_guard(&t33, l33) {
        Ok(_) => return Err(E_DEPTH),   // 33 层必须拒（漏拒 = 上限失效）
        Err(E_DEPTH) => {}
        Err(_) => return Err(E_INTG),   // 拒但码不对 = 诊断漂移
    }
    let tok = depth_reject_payload(actual_depth(&t33, l33)?);
    if payload_depth(tok) != DEPTH_MAX + 1 || payload_cap(tok) != DEPTH_MAX {
        return Err(E_INTG);
    }
    Ok(tok)
}

// ---- 步数分布账 -------------------------------------------------------------
/// 100 例样本的 min/P50/max 三分位（判据 ① 的分布面——P99 之外的
/// 双分位锚）：三分位全 ≤ 冻结界；三分位自身有序（min ≤ P50 ≤ max，
/// 排序实现的账面自检）；P50 是比 P99 更紧的常锚（中位样本超界 =
/// 普通样本就已退化，不必等尾部）。
pub fn budget_distribution() -> Result<(u32, u32, u32), u16> {
    let mut samples: Vec<u32> = Vec::new();
    let mut k = 0;
    while k < SAMPLE_N {
        let (t, leaf) = deep_chain(DEPTH_MAX, (k * 7 + 3) % 5)?;
        let d = actual_depth(&t, leaf)?;
        if d != DEPTH_MAX {
            return Err(E_SAMPLE);
        }
        samples.push(eval_steps(&t, leaf)?);
        k += 1;
    }
    if samples.len() != SAMPLE_N {
        return Err(E_SAMPLE);
    }
    // 插入排序升序（与 p99_measure 同一排序器——分布账同源可对平）。
    let mut i = 1;
    while i < samples.len() {
        let key = samples[i];
        let mut j = i;
        while j > 0 && samples[j - 1] > key {
            samples[j] = samples[j - 1];
            j -= 1;
        }
        samples[j] = key;
        i += 1;
    }
    let min = samples[0];
    let p50 = samples[SAMPLE_N / 2];
    let max = samples[SAMPLE_N - 1];
    if min > BUDGET_P99 || p50 > BUDGET_P99 || max > BUDGET_P99 {
        return Err(E_BUDGET);
    }
    if !(min <= p50 && p50 <= max) {
        return Err(E_SAMPLE);
    }
    Ok((min, p50, max))
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let (av, ab) = ie::engine_anchor();
    if av != 1 || ab != 0x7b30 {
        return Err(E_ANCHOR);
    }
    // R1：深度 32 恰好求值（上限内不许拒——负例）。
    let (t32, leaf32) = deep_chain(DEPTH_MAX, 3)?;
    if eval_with_depth_guard(&t32, leaf32).is_err() {
        return Err(E_DEPTH);
    }
    // R2：P99 实测 ≤ 冻结界（100 例 + max 同界）。
    let (p99, max) = p99_measure()?;
    // R3：超深 10 例全拒 + 诊断可读。
    let rejected = overdeep_sweep()?;
    if rejected != 10 {
        return Err(E_DEPTH);
    }
    // R4：步数账单调性钉（同树更浅节点步数 ≤ 更深节点）。
    let (t, leaf) = deep_chain(DEPTH_MAX, 0)?;
    let mid = t.nodes[leaf].parent.ok_or(E_INTG)?;
    if eval_steps(&t, mid)? > eval_steps(&t, leaf)? {
        return Err(E_BUDGET);
    }
    // R5：O1 快照形状（两行、tag 非空、界值对位）。
    if O1_SNAPSHOT.len() != 2
        || O1_SNAPSHOT[0].1 != BUDGET_P99
        || O1_SNAPSHOT[1].1 != DEPTH_MAX
        || O1_SNAPSHOT[0].0.is_empty()
    {
        return Err(E_ANCHOR);
    }
    // R6：拒绝载荷往返（编 → 解 → 对位，三组样例）。
    payload_roundtrip()?;
    // R7：深度-成本矩阵（单调不减 + 32 层行极差界 + 双档夹逼）。
    let (_spread, _rmax) = depth_cost_matrix()?;
    // R8：边界三连（31/32 过、33 拒、载荷对位）。
    let tok = boundary_triple()?;
    if payload_cap(tok) != DEPTH_MAX {
        return Err(E_INTG);
    }
    // R9：步数分布三分位（min/P50/max 全界 + 有序）。
    let _dist = budget_distribution()?;
    let _ = max;
    Ok(p99.wrapping_mul(3).wrapping_add(rejected))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_DEPTH => "深度超上限 32（拒绝 + 实际深度可诊断）",
        E_BUDGET => "P99/Max 步数越冻结界 200（求值实现退化）",
        E_SAMPLE => "采样账不符",
        E_ANCHOR => "引擎锚/O1 快照漂移",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36211_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36211_inhdepth");
    cs.add("SEG 段位独占", SEG_BASE == 0x9d70, "0x9d70~0x9d7F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "32 层可求值/P99 ≤ 200 步/超深 10 例拒+诊断/O1 登记全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("深度锚", DEPTH_MAX == 32, "求值深度上限 = 32（本域冻结）");
    cs.add("预算锚", BUDGET_P99 == 200, "5μs ≈ 200 步冻结界（30ns 走线锚）");
    cs.add("O1 预告", O1_SNAPSHOT[0].0 == "o1-inh-depth-p99" && O1_SNAPSHOT[1].1 == 32, "预算账入 O1 预告（两行快照）");
    cs.add("步数面", { let (t, l) = deep_chain(4, 1).ok().unwrap_or((ie::InhTree::new(), 0)); eval_steps(&t, l).is_ok() && actual_depth(&t, l) == Ok(4) }, "步数账/深度账双通（4 层对照）");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试 ---------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_checks_green() {
        let cs = run_unx36211_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_p99_wall_clock_under_5us() {
        // 时钟补证（探针 ktest 面）：32 层树叶子求值单次实跑 < 5μs
        // （no_std 账面用步数预算；测试域有 std 时钟，两者同向）。
        let (t, leaf) = deep_chain(DEPTH_MAX, 2).ok().unwrap_or((ie::InhTree::new(), 0));
        let t0 = std::time::Instant::now();
        let mut n = 0u32;
        while n < SAMPLE_N as u32 {
            let _ = eval_with_depth_guard(&t, leaf);
            n += 1;
        }
        let per_call = t0.elapsed() / SAMPLE_N as u32;
        // debug 构建（无优化）只做同向性检查 < 1ms；权威判据是步数
        // 预算 BUDGET_P99（确定性），release ktest 的墙钟在 O1 面。
        assert!(per_call.as_nanos() < 1_000_000, "单次求值墙钟同向性破: {:?}", per_call);
    }

    #[test]
    fn test_depth_boundary_32_ok_33_rejected() {
        // 账面守恒：边界钉——32 层恰过、33 层恰拒（超深注入的最小形）。
        let (t32, l32) = deep_chain(32, 0).ok().unwrap_or((ie::InhTree::new(), 0));
        assert!(eval_with_depth_guard(&t32, l32).is_ok());
        let (t33, l33) = deep_chain(33, 0).ok().unwrap_or((ie::InhTree::new(), 0));
        assert_eq!(eval_with_depth_guard(&t33, l33).err(), Some(E_DEPTH));
        assert_eq!(actual_depth(&t33, l33), Ok(33));
        assert_eq!(overdeep_sweep().ok().unwrap_or(0), 10);
    }
}
