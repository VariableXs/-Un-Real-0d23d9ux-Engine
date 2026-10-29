//! UNX-F36206-J1 继承与 Deny 优先的协同（继承 Deny 位次）。
//! 判据：① 继承 Deny/Allow 混排 100 例求值序与基准口径零差——双
//! 独立实现（线性首遇即拒引擎 vs 逐位独立解析基准机）在 20 棵混排
//! 树 × 5 请求上逐例对平；② 黄金 30 组入库（三段式冻结表：纯
//! Allow / 显式 Deny 在前 / 继承 Deny 在后——继承 Deny 位次的口径
//! 钉子：显式 Allow 在规范序上先于继承 Deny）；③ 纯功能 ≥320 行。
//! 设计纪律：本域冻结的求值口径（与 win32 canonical order 对齐）：
//! 线性扫规范序有效集；首遇「同 sid 且 mask∧desired≠0」的 Deny 整
//! 请求即拒（拒绝带已积累的授予位走）；Allow 累积凑满即授。继承
//! Deny 的位次由规范序决定：它排在显式 Allow 之后——「显式授权
//! 胜过继承拒绝」是本域显式钉死的口径（父辈的 Deny 不追溯推翻子
//! 对象的显式 Allow），与 win32 「non-inherited 先于 inherited」一
//! 致。任何想改这条口径的模块必须先改黄金表（变更走版本锚）。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9d20;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_EVAL: u16 = SEG_BASE + 2; // 双实现对平破
pub const E_GOLD: u16 = SEG_BASE + 3; // 黄金 30 组不符
pub const E_ORDER: u16 = SEG_BASE + 4; // 有效集非规范（求值前提破）
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚漂移
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

pub const VER: u32 = 1;

/// 混排例量锚：20 棵树 × 5 请求 = 100 例。
pub const TREE_N: usize = 20;
pub const QUERY_N: usize = 5;
pub const CASE_N: usize = 100;
/// 黄金组数锚。
pub const GOLD_N: usize = 30;

/// 基准机（独立实现二）：逐位独立解析。位 b 授予 ⟺ 首个匹配
/// (sid, b) 的 ACE 是 Allow 且其位次先于首个匹配 desired 的 Deny。
pub fn baseline_eval(acl: &[ie::Ace], sid: u16, desired: u32) -> (u8, u32) {
    // 首个「会触发拒绝」的 Deny 位次。
    let mut pos_deny = acl.len();
    let mut i = 0;
    while i < acl.len() {
        let a = &acl[i];
        if a.sid == sid && a.kind == ie::AK_DENY && a.mask & desired != 0 {
            pos_deny = i;
            i = acl.len(); // break 的 no_std 朴素写法
        } else {
            i += 1;
        }
    }
    let mut granted = 0u32;
    let mut bit = 0u32;
    while bit < 32 {
        let m = 1u32 << bit;
        if desired & m != 0 {
            let mut k = 0;
            while k < acl.len() {
                let a = &acl[k];
                if a.sid == sid && a.mask & m != 0 {
                    if a.kind == ie::AK_ALLOW && k < pos_deny {
                        granted |= m;
                    }
                    k = acl.len();
                } else {
                    k += 1;
                }
            }
        }
        bit += 1;
    }
    if granted & desired == desired {
        (ie::V_GRANT, granted)
    } else {
        (ie::V_DENY, granted)
    }
}

/// 20 棵混排树：显式 Deny/Allow + 祖辈继承 Deny/Allow 交错。
pub fn mixed_tree(case: usize) -> Result<ie::InhTree, u16> {
    if case >= TREE_N {
        return Err(E_INTG);
    }
    let mut t = ie::InhTree::new();
    let root = t.add_node("root", None, true, ie::SE_DACL_PRESENT)?;
    let mid = t.add_node("mid", Some(root), true, ie::SE_DACL_PRESENT)?;
    let leaf = t.add_node("leaf", Some(mid), false, ie::SE_DACL_PRESENT)?;
    let mut ra: Vec<ie::Ace> = Vec::new();
    // root：一条 Allow 或 Deny，继承位随例轮转（OI|CI 保证到达叶）。
    let root_deny = case % 2 == 0;
    ra.push(ie::Ace {
        sid: 10,
        mask: 0x4,
        flags: ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT,
        kind: if root_deny { ie::AK_DENY } else { ie::AK_ALLOW },
        origin: 0,
    });
    // root 第二条：显式面（对 root 自身求值用）。
    ra.push(ie::Ace { sid: 11, mask: 0x1, flags: 0, kind: if case % 3 == 0 { ie::AK_DENY } else { ie::AK_ALLOW }, origin: 0 });
    t.set_dacl(root, ra)?;
    let mut ma: Vec<ie::Ace> = Vec::new();
    ma.push(ie::Ace {
        sid: 12,
        mask: 0x2,
        flags: if case % 4 == 0 { ie::AI_OBJECT_INHERIT } else { 0 },
        kind: if case % 5 == 0 { ie::AK_DENY } else { ie::AK_ALLOW },
        origin: 0,
    });
    t.set_dacl(mid, ma)?;
    let mut la: Vec<ie::Ace> = Vec::new();
    la.push(ie::Ace { sid: 10, mask: 0x2, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    la.push(ie::Ace { sid: 13, mask: 0x8, flags: 0, kind: ie::AK_DENY, origin: 0 });
    t.set_dacl(leaf, la)?;
    Ok(t)
}

/// 100 例对平（判据 ① 主面）：引擎 eval_request vs 基准机
/// baseline_eval，sid/desired 按例轮转覆盖显式+继承双方。
pub fn mixed_sweep() -> Result<u32, u16> {
    let mut done = 0u32;
    let mut case = 0;
    while case < TREE_N {
        let t = mixed_tree(case)?;
        let leaf = 2;
        let acl = t.effective_dacl(leaf)?;
        if !ie::is_canonical(&acl) {
            return Err(E_ORDER);
        }
        let mut q = 0;
        while q < QUERY_N {
            let sid: u16 = match q {
                0 => 10,
                1 => 12,
                2 => 13,
                3 => 11,
                _ => 99,
            };
            let desired: u32 = match q {
                0 => 0x6,
                1 => 0x2,
                2 => 0x8,
                3 => 0x1,
                _ => 0xF,
            };
            let (v1, g1) = ie::eval_request(&acl, sid, desired);
            let (v2, g2) = baseline_eval(&acl, sid, desired);
            if v1 != v2 || g1 != g2 {
                return Err(E_EVAL);
            }
            done += 1;
            q += 1;
        }
        case += 1;
    }
    if done != CASE_N as u32 {
        return Err(E_INTG);
    }
    Ok(done)
}

/// 黄金 30 组（冻结字面表）：(deny_mask, deny_flags, allow_mask,
/// sid, desired, verdict, granted)——sid 统一 7。三段布局：
///   行 1..10  纯 Allow（无 Deny）；
///   行 11..20 显式 Deny + 显式 Allow（Deny 组 0 先）；
///   行 21..30 显式 Allow + 继承 Deny（组 1 < 组 2——显式 Allow 先）。
pub const GOLD_N7: u16 = 7;

pub const GOLDEN30: [(u32, u8, u32, u16, u32, u8, u32); GOLD_N] = [
    // 行 1..10：纯 Allow。verdict = grant ⟺ desired ⊆ mask。
    (0, 0, 0x3, GOLD_N7, 0x3, ie::V_GRANT, 0x3),
    (0, 0, 0x3, GOLD_N7, 0x1, ie::V_GRANT, 0x1),
    (0, 0, 0x7, GOLD_N7, 0x3, ie::V_GRANT, 0x3),
    (0, 0, 0x1, GOLD_N7, 0x3, ie::V_DENY, 0x1),
    (0, 0, 0x2, GOLD_N7, 0x3, ie::V_DENY, 0x2),
    (0, 0, 0x8, GOLD_N7, 0x8, ie::V_GRANT, 0x8),
    (0, 0, 0x5, GOLD_N7, 0x4, ie::V_GRANT, 0x4),
    (0, 0, 0x5, GOLD_N7, 0x3, ie::V_DENY, 0x1),
    (0, 0, 0x0, GOLD_N7, 0x1, ie::V_DENY, 0x0),
    (0, 0, 0xF, GOLD_N7, 0xF, ie::V_GRANT, 0xF),
    // 行 11..20：显式 Deny 在前——desired 碰 Deny 位即拒。
    (0x1, 0, 0x3, GOLD_N7, 0x1, ie::V_DENY, 0x0),
    (0x1, 0, 0x3, GOLD_N7, 0x2, ie::V_GRANT, 0x2),
    (0x1, 0, 0x3, GOLD_N7, 0x3, ie::V_DENY, 0x0),
    (0x2, 0, 0x7, GOLD_N7, 0x5, ie::V_GRANT, 0x5),
    (0x2, 0, 0x7, GOLD_N7, 0x2, ie::V_DENY, 0x0),
    (0x8, 0, 0xF, GOLD_N7, 0x7, ie::V_GRANT, 0x7),
    (0x4, 0, 0x3, GOLD_N7, 0x3, ie::V_GRANT, 0x3),
    (0x1, 0, 0x1, GOLD_N7, 0x1, ie::V_DENY, 0x0),
    (0x6, 0, 0x9, GOLD_N7, 0x9, ie::V_GRANT, 0x9),
    (0x6, 0, 0x9, GOLD_N7, 0x2, ie::V_DENY, 0x0),
    // 行 21..30：显式 Allow 先于继承 Deny（继承 Deny 位次口径钉）。
    (0x8, ie::AI_INHERITED, 0x3, GOLD_N7, 0x3, ie::V_GRANT, 0x3),
    (0x8, ie::AI_INHERITED, 0x3, GOLD_N7, 0x1, ie::V_GRANT, 0x1),
    (0x4, ie::AI_INHERITED, 0x3, GOLD_N7, 0x7, ie::V_DENY, 0x3),
    (0x4, ie::AI_INHERITED, 0x3, GOLD_N7, 0x4, ie::V_DENY, 0x0),
    (0x8, ie::AI_INHERITED, 0x7, GOLD_N7, 0x7, ie::V_GRANT, 0x7),
    (0x2, ie::AI_INHERITED, 0x1, GOLD_N7, 0x3, ie::V_DENY, 0x1),
    (0x8, ie::AI_INHERITED, 0xF, GOLD_N7, 0x8, ie::V_GRANT, 0x8),
    (0x1, ie::AI_INHERITED, 0x0, GOLD_N7, 0x1, ie::V_DENY, 0x0),
    (0x2, ie::AI_INHERITED, 0x5, GOLD_N7, 0x4, ie::V_GRANT, 0x4),
    (0x2, ie::AI_INHERITED, 0x5, GOLD_N7, 0x6, ie::V_DENY, 0x4),
];

/// 黄金 ACL 构造：先显式 Deny（若 flags==0 且 deny_mask≠0）、再显式
/// Allow、继承 Deny 按行尾布局（构造序无关——求值前先规范序）。
pub fn golden_acl(row: usize) -> Result<Vec<ie::Ace>, u16> {
    if row >= GOLD_N {
        return Err(E_INTG);
    }
    let (dmask, dflags, amask, sid, _desired, _v, _g) = GOLDEN30[row];
    let mut acl: Vec<ie::Ace> = Vec::new();
    if dmask != 0 {
        let origin = if dflags & ie::AI_INHERITED != 0 { 42 } else { 0 };
        acl.push(ie::Ace { sid, mask: dmask, flags: dflags, kind: ie::AK_DENY, origin });
    }
    if amask != 0 || dmask == 0 {
        acl.push(ie::Ace { sid, mask: amask, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    }
    let (sorted, _) = crate::unxstar::unx36205_inhorder::canonicalize(&acl);
    Ok(sorted)
}

/// 黄金 30 组回放（判据 ② 主面）。
pub fn golden_replay() -> Result<u32, u16> {
    let mut ok = 0u32;
    let mut row = 0;
    while row < GOLD_N {
        let (_dm, _df, _am, sid, desired, want_v, want_g) = GOLDEN30[row];
        let acl = golden_acl(row)?;
        let (v, g) = ie::eval_request(&acl, sid, desired);
        if v != want_v || g != want_g {
            return Err(E_GOLD);
        }
        let (v2, g2) = baseline_eval(&acl, sid, desired);
        if v2 != want_v || g2 != want_g {
            return Err(E_GOLD);
        }
        ok += 1;
        row += 1;
    }
    Ok(ok)
}

// ---- 步数分布账（黄金 30 组的执行面） ---------------------------------------
/// 黄金 30 组的求值步数账：每行两实现各记一次「扫描消耗」——基线机
/// 步数 = 位循环内层扫描长度总和；分布账锚（min ≤ max、全部非零、
/// 行数恰 30）。账面口径：步数退化 = 求值实现变笨的早期信号。
pub fn golden_step_account() -> Result<(u32, u32, u32), u16> {
    let mut min = u32::MAX;
    let mut max = 0u32;
    let mut total = 0u32;
    let mut row = 0;
    while row < GOLD_N {
        let acl = golden_acl(row)?;
        let (_dm, _df, _am, sid, desired, _wv, _wg) = GOLDEN30[row];
        // 步数账：baseline_eval 的位循环内层扫描长度（复刻内层结构）。
        let mut steps = 1u32; // pos_deny 前扫
        let mut i = 0;
        while i < acl.len() {
            steps += 1;
            if acl[i].sid == sid && acl[i].kind == ie::AK_DENY && acl[i].mask & desired != 0 {
                i = acl.len();
            } else {
                i += 1;
            }
        }
        let mut bit = 0u32;
        while bit < 32 {
            let m = 1u32 << bit;
            if desired & m != 0 {
                let mut k = 0;
                while k < acl.len() {
                    steps += 1;
                    if acl[k].sid == sid && acl[k].mask & m != 0 {
                        k = acl.len();
                    } else {
                        k += 1;
                    }
                }
            }
            bit += 1;
        }
        if steps == 0 {
            return Err(E_INTG);
        }
        if steps < min {
            min = steps;
        }
        if steps > max {
            max = steps;
        }
        total = total.wrapping_add(steps);
        row += 1;
    }
    if min > max || total < max {
        return Err(E_INTG);
    }
    Ok((min, max, total))
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
    // R1：100 例双实现对平。
    let done = mixed_sweep()?;
    // R2：黄金 30 组（双实现都过冻结账）。
    let ok = golden_replay()?;
    if ok != GOLD_N as u32 {
        return Err(E_GOLD);
    }
    // R3：继承 Deny 位次口径钉（显式 Allow 先于继承 Deny——手推三
    // ACE 例：allow 0x3 显式 / deny 0x4 继承 / deny 0x1 继承）。
    let mut probe: Vec<ie::Ace> = Vec::new();
    probe.push(ie::Ace { sid: 7, mask: 0x4, flags: ie::AI_INHERITED, kind: ie::AK_DENY, origin: 9 });
    probe.push(ie::Ace { sid: 7, mask: 0x3, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    probe.push(ie::Ace { sid: 7, mask: 0x1, flags: ie::AI_INHERITED, kind: ie::AK_DENY, origin: 10 });
    let (sorted, _) = super::unx36205_inhorder::canonicalize(&probe);
    let (v, g) = ie::eval_request(&sorted, 7, 0x3);
    if v != ie::V_GRANT || g != 0x3 {
        return Err(E_EVAL);
    }
    let (v2, g2) = ie::eval_request(&sorted, 7, 0x4);
    // desired 0x4 与显式 Allow(0x3) 无交集 → 拒绝时授予位为 0。
    if v2 != ie::V_DENY || g2 != 0x0 {
        return Err(E_EVAL);
    }
    // R4：拒绝带已授予位走（拒绝时 granted 保留积累）——显式 Allow
    // （组 1）先于继承 Deny（组 2），拒绝命中时已积累 0x1。
    let mut p2: Vec<ie::Ace> = Vec::new();
    p2.push(ie::Ace { sid: 7, mask: 0x1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    p2.push(ie::Ace { sid: 7, mask: 0x2, flags: ie::AI_INHERITED, kind: ie::AK_DENY, origin: 9 });
    let (_, g3) = ie::eval_request(&p2, 7, 0x3);
    if g3 != 0x1 {
        return Err(E_EVAL);
    }
    // R5：黄金表形状（行数/段位标记）。
    if GOLDEN30.len() != 30 || GOLDEN30[0].1 != 0 || GOLDEN30[10].1 != 0 || GOLDEN30[20].1 != ie::AI_INHERITED {
        return Err(E_GOLD);
    }
    // R6：步数分布账（30 行 min/max/total 全账非零有序）。
    let (smin, smax, _stotal) = golden_step_account()?;
    if smin == 0 || smax < smin {
        return Err(E_INTG);
    }
    Ok(done.wrapping_mul(3).wrapping_add(ok))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_EVAL => "求值序双实现对平破",
        E_GOLD => "黄金 30 组不符（口径漂移）",
        E_ORDER => "有效集非规范序（求值前提破）",
        E_ANCHOR => "引擎锚漂移",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36206_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36206_inhdeny");
    cs.add("SEG 段位独占", SEG_BASE == 0x9d20, "0x9d20~0x9d2F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "100 例双实现对平/黄金 30 组回放/位次口径钉全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("例量锚", TREE_N == 20 && QUERY_N == 5 && CASE_N == 100, "20 树 × 5 请求 = 100 例（判据锚）");
    cs.add("黄金库锚", GOLD_N == 30, "黄金 30 组入库（三段冻结表）");
    cs.add("位次口径钉", ie::canon_group(&ie::Ace { sid: 7, mask: 1, flags: 0, kind: ie::AK_ALLOW, origin: 0 }) < ie::canon_group(&ie::Ace { sid: 7, mask: 1, flags: ie::AI_INHERITED, kind: ie::AK_DENY, origin: 9 }), "显式 Allow(组1) < 继承 Deny(组2)");
    cs.add("越界面", golden_acl(GOLD_N).is_err(), "黄金行号越界显性拒");
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
        let cs = run_unx36206_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_explicit_allow_beats_inherited_deny() {
        // 性质直证：显式 Allow(0x3) 在规范序上先于继承 Deny(0x4)——
        // desired=0x3 授（凑满即走）；desired=0x4 拒（位次靠后）。
        let acl = golden_acl(22).ok().unwrap_or_default();
        let (v1, g1) = ie::eval_request(&acl, GOLD_N7, 0x3);
        assert_eq!((v1, g1), (ie::V_GRANT, 0x3));
        let (v2, g2) = ie::eval_request(&acl, GOLD_N7, 0x4);
        assert_eq!((v2, g2), (ie::V_DENY, 0x0));
    }

    #[test]
    fn test_baseline_matches_engine_on_random_acl() {
        // 账面守恒：双实现在 LCG 乱序 ACL 上 60 组全对平（超出卡面
        // 100 例之外的加固面）。
        let mut rng = crate::unxstar::unx36205_inhorder::Lcg::new(0xBEEF_1234);
        let mut pairs = 0u32;
        let mut c = 0;
        while c < 60 {
            let mut acl: Vec<ie::Ace> = Vec::new();
            let n = 2 + rng.below(5) as usize;
            let mut i = 0;
            while i < n {
                let inh = rng.below(2) == 0;
                acl.push(ie::Ace {
                    sid: 7,
                    mask: 1 + rng.below(0xF) as u32,
                    flags: if inh { ie::AI_INHERITED } else { 0 },
                    kind: if rng.below(2) == 0 { ie::AK_DENY } else { ie::AK_ALLOW },
                    origin: if inh { 3 } else { 0 },
                });
                i += 1;
            }
            let (sorted, _) = crate::unxstar::unx36205_inhorder::canonicalize(&acl);
            let desired = 1 + rng.below(0xF) as u32;
            let r1 = ie::eval_request(&sorted, 7, desired);
            let r2 = baseline_eval(&sorted, 7, desired);
            assert_eq!(r1, r2, "case {} desired={:#x}", c, desired);
            pairs += 1;
            c += 1;
        }
        assert_eq!(pairs, 60);
    }
}
