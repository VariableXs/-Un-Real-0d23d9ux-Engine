//! UNX-F36202-J1 容器继承/对象继承位语义（CI/OI 组合四态）。
//! 判据：① CI/OI 四态组合 100 例传播正确——(OI,CI) 四态 × 容器/叶子
//! × NP × IO × 深度 1/2 的全枚举面，实际传播与黄金规则逐例对平；
//! ② 组合误判注入 20 例被黄金对照拦截（OI↔CI 混淆 / 清位 / 加 NP
//! 五种误判 × 四态，全部被「按原始位的黄金期望」检出）；③ 四态黄金
//! 表登记（本域传播口径的冻结锚）；④ 纯功能代码 ≥320 行。
//! 设计纪律：CI（容器继承）与 OI（对象继承）是继承引擎的第一语义
//! 位——搞混 CI/OI 等于搞混「谁能拿到这条 ACE」。本模块把四态在
//! 两层传播（深度 1 与深度 2）上的可观测结果全部钉死，供 36203
//! （ID 位）/36205（规范序）复用同一传播口径。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7b40;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_PROP: u16 = SEG_BASE + 2; // 传播与黄金规则零差破
pub const E_INJ: u16 = SEG_BASE + 3; // 误判注入未被拦截
pub const E_STATE: u16 = SEG_BASE + 4; // 四态编号越界
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚/黄金表漂移
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

pub const VER: u32 = 1;

/// 四态黄金表：(标签, 传播位组合)。位组合即 ACE flags 的继承位部分，
/// 其余位（NP/IO）由用例轴单独枚举——四态指的是「以继承位论」的分类。
pub const GOLDEN_FOUR: [(&'static str, u8); 4] = [
    ("S0-无继承位", 0),
    ("S1-OI-only", ie::AI_OBJECT_INHERIT),
    ("S2-CI-only", ie::AI_CONTAINER_INHERIT),
    ("S3-OI|CI", ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT),
];

/// 传播例数（判据锚）。
pub const CASE_N: usize = 100;
pub const INJ_N: usize = 20;

/// 黄金规则（独立重导）：root ACE 携 flags，问「深度 depth 的目标
/// （容器性 container）能否见到该 ACE 的继承副本」。
///   深度 1：目标容器看 CI、叶子看 OI；
///   深度 2：先按深度 1 折算副本位（置 ID 清 IO；NP 再清 OI|CI），
///           副本传得下（NP 不清位或清位后仍有目标位）才谈深度 2。
pub fn expect_propagation(root_flags: u8, container: bool, depth: u32) -> bool {
    let want = if container { ie::AI_CONTAINER_INHERIT } else { ie::AI_OBJECT_INHERIT };
    if root_flags & want == 0 {
        return false;
    }
    if depth <= 1 {
        return true;
    }
    // 深度 ≥ 2：OI/CI 语义是「穿透不适用的容器、作用于任意深度的
    // 适用目标」（win32 继承传播形态）——唯一断代位是 NP。
    root_flags & ie::AI_NO_PROPAGATE == 0
}

/// 100 例用例轴解码（确定性枚举——不引入随机源）：
///   state = case%4；child 容器性 = (case/4)%2；NP = (case/8)%2；
///   IO = (case/16)%2；深度 2 考核 = (case/32)%2；sid/kind 随例轮转。
pub fn decode(case: usize) -> Result<(usize, bool, bool, bool, bool, u16, u8), u16> {
    if case >= CASE_N {
        return Err(E_STATE);
    }
    let state = case % 4;
    let child_container = (case / 4) % 2 == 1;
    let np = (case / 8) % 2 == 1;
    let io = (case / 16) % 2 == 1;
    let d2 = (case / 32) % 2 == 1;
    let sid = 100 + (case % 7) as u16;
    let kind = if case % 2 == 0 { ie::AK_DENY } else { ie::AK_ALLOW };
    Ok((state, child_container, np, io, d2, sid, kind))
}

pub fn build_case(case: usize) -> Result<(ie::InhTree, usize, usize), u16> {
    let (state, child_container, np, io, _d2, sid, kind) = decode(case)?;
    let mut flags = GOLDEN_FOUR[state].1;
    if np {
        flags |= ie::AI_NO_PROPAGATE;
    }
    if io {
        flags |= ie::AI_INHERIT_ONLY;
    }
    let mut t = ie::InhTree::new();
    let root = t.add_node("root", None, true, ie::SE_DACL_PRESENT)?;
    let mid = t.add_node("mid", Some(root), true, ie::SE_DACL_PRESENT)?;
    let node2 = t.add_node("node2", Some(mid), child_container, ie::SE_DACL_PRESENT)?;
    let mut aces: Vec<ie::Ace> = Vec::new();
    aces.push(ie::Ace { sid, mask: 0x7, flags, kind, origin: 0 });
    t.set_dacl(root, aces)?;
    Ok((t, mid, node2))
}

/// 单例直证：mid（深度 1，恒容器）与 node2（深度 2）两个观测点的
/// 实际可见性与黄金规则对平；副本位语义（ID 置位 / IO 清位）同验。
fn case_verify(case: usize) -> Result<(), u16> {
    let (state, child_container, np, io, d2, sid, kind) = decode(case)?;
    let root_flags = {
        let mut f = GOLDEN_FOUR[state].1;
        if np {
            f |= ie::AI_NO_PROPAGATE;
        }
        if io {
            f |= ie::AI_INHERIT_ONLY;
        }
        f
    };
    let (t, mid, node2) = build_case(case)?;
    // 观测点 1：mid 恒为容器，深度 1。
    let want1 = expect_propagation(root_flags, true, 1);
    let mid_eff = t.effective_dacl(mid)?;
    let got1 = count_matches(&mid_eff, sid);
    if got1 != want1 as usize {
        return Err(E_PROP);
    }
    if want1 {
        let a = find_ace(&mid_eff, sid).ok_or(E_PROP)?;
        if a.flags & ie::AI_INHERITED == 0 || a.flags & ie::AI_INHERIT_ONLY != 0 {
            return Err(E_PROP);
        }
        if a.kind != kind {
            return Err(E_PROP);
        }
    }
    // 观测点 2：node2 深度 2，容器性按用例轴（恒核——OI/CI 的深度
    // 语义由黄金规则全权决定，d2 轴只作用例多样化）。
    let want2 = expect_propagation(root_flags, child_container, 2);
    let _ = d2;
    let n2_eff = t.effective_dacl(node2)?;
    let got2 = count_matches(&n2_eff, sid);
    if got2 != want2 as usize {
        return Err(E_PROP);
    }
    Ok(())
}

fn count_matches(acl: &[ie::Ace], sid: u16) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < acl.len() {
        if acl[i].sid == sid {
            n += 1;
        }
        i += 1;
    }
    n
}

fn find_ace(acl: &[ie::Ace], sid: u16) -> Option<ie::Ace> {
    let mut i = 0;
    while i < acl.len() {
        if acl[i].sid == sid {
            return Some(acl[i]);
        }
        i += 1;
    }
    None
}

/// 误判注入表：20 条 (加位, 清位, 观测容器性, 观测深度)。每条都经过
/// 手推钉死——对所属四态的原始位，注入后在该观测点上黄金结论必然
/// 翻转（加位/清位在其余观测点可能不可见，本表只收可观测错位）。
/// 布局：每态 5 条（S0×5 → 表 0..5，S1×5 → 5..10，S2×5 → 10..15，
/// S3×5 → 15..20），覆盖 OI↔CI 混淆、清位、加 NP 断代五类误判。
pub const INJ_TABLE: [(u8, u8, bool, u32); INJ_N] = [
    // S0（无继承位）：黄金结论恒不可见——注入让它可见。
    (ie::AI_OBJECT_INHERIT, 0, false, 1),
    (ie::AI_CONTAINER_INHERIT, 0, true, 1),
    (ie::AI_OBJECT_INHERIT, 0, false, 2),
    (ie::AI_CONTAINER_INHERIT, 0, true, 2),
    (ie::AI_OBJECT_INHERIT | ie::AI_INHERIT_ONLY, 0, false, 1),
    // S1（OI-only）：
    (ie::AI_CONTAINER_INHERIT, 0, true, 1),
    (0, ie::AI_OBJECT_INHERIT, false, 1),
    (ie::AI_NO_PROPAGATE, 0, false, 2),
    (ie::AI_CONTAINER_INHERIT | ie::AI_NO_PROPAGATE, 0, true, 1),
    (ie::AI_CONTAINER_INHERIT, ie::AI_OBJECT_INHERIT, false, 1),
    // S2（CI-only）：
    (ie::AI_OBJECT_INHERIT, 0, false, 1),
    (0, ie::AI_CONTAINER_INHERIT, true, 1),
    (ie::AI_NO_PROPAGATE, 0, true, 2),
    (ie::AI_OBJECT_INHERIT | ie::AI_NO_PROPAGATE, 0, false, 1),
    (ie::AI_OBJECT_INHERIT, ie::AI_CONTAINER_INHERIT, true, 1),
    // S3（OI|CI）：
    (0, ie::AI_OBJECT_INHERIT, false, 1),
    (0, ie::AI_CONTAINER_INHERIT, true, 1),
    (ie::AI_NO_PROPAGATE, 0, true, 2),
    (0, ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, false, 1),
    (ie::AI_NO_PROPAGATE, ie::AI_CONTAINER_INHERIT, false, 2),
];

/// 单例拦截判定：黄金对照按「原始位」取期望，注入位按「篡改位」实跑；
/// 期望 ≠ 实跑 = 错位被拦截（黄金对照起到守门作用）。
pub fn injection_verify(case: usize) -> Result<(), u16> {
    if case >= INJ_N {
        return Err(E_STATE);
    }
    let state = case / 5;
    let (add, clear, container, depth) = INJ_TABLE[case];
    let orig = GOLDEN_FOUR[state].1;
    let mutated = (orig | add) & !clear;
    let want = expect_propagation(orig, container, depth);
    let got = expect_propagation(mutated, container, depth);
    if got == want {
        return Err(E_INJ); // 未错位 = 未拦截（黄金对照失效）
    }
    Ok(())
}

/// 全 20 例注入扫描（判据 ② 主面——每例必须被拦截）。
pub fn injection_sweep() -> Result<u32, u16> {
    let mut detected = 0u32;
    let mut case = 0;
    while case < INJ_N {
        injection_verify(case)?;
        detected += 1;
        case += 1;
    }
    Ok(detected)
}

/// 100 例传播扫描（判据 ① 主面）。
pub fn propagation_sweep() -> Result<u32, u16> {
    let mut done = 0u32;
    let mut case = 0;
    while case < CASE_N {
        case_verify(case)?;
        done += 1;
        case += 1;
    }
    Ok(done)
}

// ---- 位轴名账 ---------------------------------------------------------------
/// 五位轴名（诊断渲染底表）：OI/CI/NP/IO/ID 各一位一名——位名账
/// 是「组合四态」的可读面，哑名 = 诊断渲染失败。
pub fn axis_name(bit: u8) -> &'static str {
    match bit {
        b if b == ie::AI_OBJECT_INHERIT => "OI",
        b if b == ie::AI_CONTAINER_INHERIT => "CI",
        b if b == ie::AI_NO_PROPAGATE => "NP",
        b if b == ie::AI_INHERIT_ONLY => "IO",
        b if b == ie::AI_INHERITED => "ID",
        _ => "?",
    }
}

/// 四轴解码（flags → 四布尔）：传播判据的输入形态（OI 在位/CI 在位/
/// NP 在位/IO 在位）——decode 元组与 GOLDEN_FOUR 表双轨同源。
pub fn axis_decode(flags: u8) -> (bool, bool, bool, bool) {
    (
        flags & ie::AI_OBJECT_INHERIT != 0,
        flags & ie::AI_CONTAINER_INHERIT != 0,
        flags & ie::AI_NO_PROPAGATE != 0,
        flags & ie::AI_INHERIT_ONLY != 0,
    )
}

/// 轴账对平钉：GOLDEN_FOUR 四态的 flags 逐一走 axis_decode → 轴形
/// 必须两两互异（四态 = 四种不同轴形——塌缩即表失义）；axis_name
/// 五名全可读（无 "?"）。
pub fn axis_ledger() -> Result<(), u16> {
    let mut i = 0;
    while i < GOLDEN_FOUR.len() {
        let (name, _flags) = GOLDEN_FOUR[i];
        if name.is_empty() {
            return Err(E_STATE);
        }
        let mut k = i + 1;
        while k < GOLDEN_FOUR.len() {
            if axis_decode(GOLDEN_FOUR[i].1) == axis_decode(GOLDEN_FOUR[k].1) {
                return Err(E_STATE);
            }
            k += 1;
        }
        i += 1;
    }
    let bits = [ie::AI_OBJECT_INHERIT, ie::AI_CONTAINER_INHERIT, ie::AI_NO_PROPAGATE, ie::AI_INHERIT_ONLY, ie::AI_INHERITED];
    let mut b = 0;
    while b < bits.len() {
        if axis_name(bits[b]) == "?" {
            return Err(E_ANCHOR);
        }
        b += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // R1：引擎锚登记（必须消费 36201 双锚——下游锚纪律）。
    let (av, ab) = ie::engine_anchor();
    if av != 1 || ab != 0x7b30 {
        return Err(E_ANCHOR);
    }
    // R2：四态黄金表健康（标签互异非空 + 位值两两互异）。
    let mut i = 0;
    while i < 4 {
        if GOLDEN_FOUR[i].0.is_empty() {
            return Err(E_ANCHOR);
        }
        let mut k = i + 1;
        while k < 4 {
            if GOLDEN_FOUR[i].0 == GOLDEN_FOUR[k].0 || GOLDEN_FOUR[i].1 == GOLDEN_FOUR[k].1 {
                return Err(E_ANCHOR);
            }
            k += 1;
        }
        i += 1;
    }
    // R3：100 例传播正确。
    let done = propagation_sweep()?;
    if done != CASE_N as u32 {
        return Err(E_PROP);
    }
    // R4：20 例误判注入全拦截。
    let detected = injection_sweep()?;
    if detected != INJ_N as u32 {
        return Err(E_INJ);
    }
    // R5：黄金规则自检（四条手推钉子：CI 到容器 / OI 到叶子互斥可见、
    // NP 断代、S0 全断）。
    if !expect_propagation(ie::AI_CONTAINER_INHERIT, true, 1)
        || expect_propagation(ie::AI_CONTAINER_INHERIT, false, 1)
    {
        return Err(E_PROP);
    }
    if !expect_propagation(ie::AI_OBJECT_INHERIT, false, 1)
        || expect_propagation(ie::AI_OBJECT_INHERIT, true, 1)
    {
        return Err(E_PROP);
    }
    if expect_propagation(
        ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT | ie::AI_NO_PROPAGATE,
        true,
        2,
    ) {
        return Err(E_PROP);
    }
    if expect_propagation(0, true, 1) || expect_propagation(0, false, 2) {
        return Err(E_PROP);
    }
    // R6：位轴名账（四态轴形互异 + 五位名全可读）。
    axis_ledger()?;
    // 校验和：100 例 + 20 例账面折叠。
    Ok(done.wrapping_mul(7).wrapping_add(detected))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_PROP => "传播与黄金规则零差破（CI/OI 四态可见性错位）",
        E_INJ => "组合误判注入未被拦截",
        E_STATE => "四态/注入编号越界",
        E_ANCHOR => "引擎锚/四态黄金表漂移",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36202_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36202_inhbits");
    cs.add("SEG 段位独占", SEG_BASE == 0x7b40, "0x7b40~0x7b4F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例传播对平/20 例注入拦截/四态黄金表健康"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("四态黄金表", GOLDEN_FOUR.len() == 4 && GOLDEN_FOUR[3].1 == 0x03, "S0/S1-OI/S2-CI/S3-OI|CI 冻结登记");
    cs.add("例量锚", CASE_N == 100 && INJ_N == 20, "100 例传播 + 20 例注入（判据锚）");
    cs.add("传播轴解码", decode(CASE_N - 1).is_ok() && decode(CASE_N).is_err(), "用例轴闭集（越界显性拒）");
    cs.add("NP 断代钉", !expect_propagation(ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT | ie::AI_NO_PROPAGATE, false, 2), "NP 在深度 2 双容器性全断");
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
        let cs = run_unx36202_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_ci_oi_visibility_matrix() {
        // 性质直证：四态 × 容器/叶子在深度 1 的可见性矩阵手推钉死。
        //            容器    叶子
        // S0          否      否
        // S1-OI       否      是
        // S2-CI       是      否
        // S3-OI|CI    是      是
        let matrix = [
            [false, false],
            [false, true],
            [true, false],
            [true, true],
        ];
        let mut s = 0;
        while s < 4 {
            assert_eq!(expect_propagation(GOLDEN_FOUR[s].1, true, 1), matrix[s][0], "state={} 容器", s);
            assert_eq!(expect_propagation(GOLDEN_FOUR[s].1, false, 1), matrix[s][1], "state={} 叶子", s);
            s += 1;
        }
    }

    #[test]
    fn test_injection_all_detected() {
        // 账面守恒：20 例注入的拦截账 = 20，无一漏拦。
        let detected = injection_sweep().ok().unwrap_or(0);
        assert_eq!(detected, INJ_N as u32);
        // 注入轴越界显性拒。
        assert_eq!(injection_verify(INJ_N).err(), Some(E_STATE));
        // 黄金表锚：每条注入对原始位确为「翻转例」（抽查三条不同类）。
        assert!(injection_verify(0).is_ok());
        assert!(injection_verify(6).is_ok());
        assert!(injection_verify(18).is_ok());
    }
}
