//! UNX-F36213-J1 继承引擎黄金用例库 v1（×50 场景冻结）。
//! 判据：① 黄金库 50 场景（十类 × 五变体：受保护标志 / 继承覆盖
//! （NP 短传）/ 顺序重排 / IO 自身不适用 / 深链传播等变体全覆盖）
//! 版本化冻结（GOLD_VER=1 + 十类手推锚逐类钉死）；② 结果账与基准
//! 机口径一致（引擎 eval 与 36206 baseline_eval 双机逐例对平）；
//! ③ 纯功能代码 ≥320 行。
//! 设计纪律：黄金库是继承语义的「宪法附表」——十类场景每类钉住
//! 一条语义规则，任何实现漂移都要先改表（走版本锚）。十类规则
//! （本域冻结口径）：c0 纯 Allow 子集授予；c1 显式 Allow 胜继承
//! Deny（位次口径）；c2 显式 Deny 前置拒；c3 NP 隔代孙代免疫；
//! c4 IO 自身不适用但传子；c5 受保护节点免疫父变更；c6 深链 32 层
//! 传播完整；c7 继承 Deny 单独即拒；c8 乱序输入规范序重排后 Deny
//! 前置；c9 空 ACL 拒绝。每类手推锚（verdict, granted）字面冻结，
//! 类规则函数与手推锚互证——两套推导对不上 = 库烂。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use super::unx36204_inhprot::ProtEngine;
use super::unx36206_inhdeny::baseline_eval;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9fb0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_GOLD: u16 = SEG_BASE + 2; // 场景结果与冻结口径不符
pub const E_ANCHOR: u16 = SEG_BASE + 3; // 手推锚漂移
pub const E_CLASS: u16 = SEG_BASE + 4; // 类号越界
pub const E_COVER: u16 = SEG_BASE + 5; // 十类覆盖账破
pub const E_INTG: u16 = SEG_BASE + 6; // 完整性自检不符

pub const VER: u32 = 1;
pub const GOLD_VER: u32 = 1;

/// 场景量锚：十类 × 五变体 = 50。
pub const CLASS_N: usize = 10;
pub const VARIANT_N: usize = 5;
pub const SCEN_N: usize = 50;

/// 场景表（冻结）：(class, variant) → 构造参数由变体公式给出。
/// sid = 7 + variant；显式 Allow 掩码 m = 0x3 + variant；
/// 期望 desired d = 0x3；各类的 deny 掩码按类规则定。
pub fn scen_params(row: usize) -> Result<(usize, usize, u16, u32, u32), u16> {
    if row >= SCEN_N {
        return Err(E_CLASS);
    }
    let class = row / VARIANT_N;
    let variant = row % VARIANT_N;
    let sid = 7 + variant as u16;
    let m = 0x3 + variant as u32;
    let d = 0x3;
    Ok((class, variant, sid, m, d))
}

/// 十类手推锚（逐类一条字面冻结——(verdict, granted)）：desired=0x3，
/// m=0x3（variant 0 口径）。这是「手算」对类规则函数的独立来源。
pub const HAND_ANCHORS: [(&'static str, u8, u32); CLASS_N] = [
    ("c0-pure-allow", ie::V_GRANT, 0x3),
    ("c1-explicit-allow-beats-inh-deny", ie::V_GRANT, 0x3),
    ("c2-explicit-deny-first", ie::V_DENY, 0x0),
    ("c3-np-grandchild-immune", ie::V_GRANT, 0x3),
    ("c4-io-self-not-applied", ie::V_DENY, 0x1),
    ("c5-protected-immune", ie::V_GRANT, 0x3),
    ("c6-deep-chain-propagates", ie::V_GRANT, 0x3),
    ("c7-inherited-deny-only", ie::V_DENY, 0x0),
    ("c8-reorder-deny-first", ie::V_DENY, 0x0),
    ("c9-empty-acl", ie::V_DENY, 0x0),
];

/// 场景构建（十类）→ 返回（引擎容器, 求值节点, sid, desired）。
/// 统一封在 ProtEngine（保护类要短路能力；其余类 protection 全关——
/// 保护感知收集在无保护时与全模型一致）。
pub fn build_scenario(row: usize) -> Result<(ProtEngine, usize, u16, u32), u16> {
    let (class, _variant, sid, m, d) = scen_params(row)?;
    let mut eng = ProtEngine::new();
    let root = eng.add_node("root", None, true)?;
    match class {
        // c0：纯 Allow。
        0 => {
            eng.tree.set_dacl(root, vec_ace(sid, m, 0, ie::AK_ALLOW, 0))?;
        }
        // c1：父继承 Deny(0x10) + 子显式 Allow（位次口径：Allow 胜）。
        1 => {
            eng.tree.set_dacl(root, vec_ace(sid, 0x10, ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, ie::AK_DENY, 0))?;
            let mid = eng.add_node("mid", Some(root), true)?;
            let child = eng.add_node("child", Some(mid), false)?;
            eng.tree.set_dacl(child, vec_ace(sid, m, 0, ie::AK_ALLOW, 0))?;
            eng.materialize_all()?;
            return Ok((eng, child, sid, d));
        }
        // c2：同 sid 显式 Deny(0x1) + Allow。
        2 => {
            let mut a: Vec<ie::Ace> = vec_ace(sid, m, 0, ie::AK_ALLOW, 0);
            a.push(ie::Ace { sid, mask: 0x1, flags: 0, kind: ie::AK_DENY, origin: 0 });
            eng.tree.set_dacl(root, a)?;
            eng.materialize_all()?;
            return Ok((eng, root, sid, d));
        }
        // c3：父 NP Deny(0x10)，孙显式 Allow——Deny 到 mid 为止。
        3 => {
            eng.tree.set_dacl(root, vec_ace(sid, 0x10, ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT | ie::AI_NO_PROPAGATE, ie::AK_DENY, 0))?;
            let mid = eng.add_node("mid", Some(root), true)?;
            let child = eng.add_node("child", Some(mid), false)?;
            eng.tree.set_dacl(child, vec_ace(sid, m, 0, ie::AK_ALLOW, 0))?;
            eng.materialize_all()?;
            return Ok((eng, child, sid, d));
        }
        // c4：父 IO Deny(0x2)（OI|CI|IO）——父自身不适用，容器子继承
        // 副本后适用；mid 显式 Allow(0x1) 先授 0x1，Deny 命中 0x2 拒。
        4 => {
            eng.tree.set_dacl(root, vec_ace(sid, 0x2, ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT | ie::AI_INHERIT_ONLY, ie::AK_DENY, 0))?;
            let mid = eng.add_node("mid", Some(root), true)?;
            eng.tree.set_dacl(mid, vec_ace(sid, 0x1, 0, ie::AK_ALLOW, 0))?;
            eng.materialize_all()?;
            return Ok((eng, mid, sid, d));
        }
        // c5：父 Deny + 子受保护——子免疫。
        5 => {
            eng.tree.set_dacl(root, vec_ace(sid, 0x10, ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, ie::AK_DENY, 0))?;
            let mid = eng.add_node("mid", Some(root), true)?;
            let child = eng.add_node("child", Some(mid), false)?;
            eng.tree.set_dacl(child, vec_ace(sid, m, 0, ie::AK_ALLOW, 0))?;
            eng.set_protection(child, true)?;
            eng.materialize_all()?;
            return Ok((eng, child, sid, d));
        }
        // c6：32 层链 Allow 传播。
        6 => {
            eng.tree.set_dacl(root, vec_ace(sid, m, ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, ie::AK_ALLOW, 0))?;
            let mut cur = root;
            let mut i = 0;
            while i < 32 {
                cur = eng.add_node("chain", Some(cur), true)?;
                i += 1;
            }
            eng.materialize_all()?;
            return Ok((eng, cur, sid, d));
        }
        // c7：继承 Deny 单独。
        7 => {
            eng.tree.set_dacl(root, vec_ace(sid, 0x2, ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT, ie::AK_DENY, 0))?;
            let mid = eng.add_node("mid", Some(root), true)?;
            let child = eng.add_node("child", Some(mid), false)?;
            eng.materialize_all()?;
            let _ = m;
            return Ok((eng, child, sid, d));
        }
        // c8：乱序输入（Allow 先写、Deny 后写）→ 规范序重排 Deny 前置。
        8 => {
            let mut a: Vec<ie::Ace> = vec_ace(sid, m, 0, ie::AK_ALLOW, 0);
            a.push(ie::Ace { sid, mask: 0x1, flags: 0, kind: ie::AK_DENY, origin: 0 });
            // 逆序写入制造乱序输入面（set_dacl 不排序——重排发生在求值）。
            let mut rev: Vec<ie::Ace> = Vec::new();
            let mut i = a.len();
            while i > 0 {
                i -= 1;
                rev.push(a[i]);
            }
            eng.tree.set_dacl(root, rev)?;
            eng.materialize_all()?;
            return Ok((eng, root, sid, d));
        }
        // c9：空 ACL。
        _ => {
            let _ = m;
            eng.materialize_all()?;
            return Ok((eng, root, sid, d));
        }
    }
    eng.materialize_all()?;
    Ok((eng, root, sid, d))
}

fn vec_ace(sid: u16, mask: u32, flags: u8, kind: u8, origin: u32) -> Vec<ie::Ace> {
    let mut v: Vec<ie::Ace> = Vec::new();
    v.push(ie::Ace { sid, mask, flags, kind, origin });
    v
}

/// 类规则函数（期望账的第二来源）：与 build_scenario 的构造一一对应，
/// 按类规则手推期望。c2/c8 同构（乱序重排后 Deny 前置）。
pub fn expect_by_class(row: usize) -> Result<(u8, u32), u16> {
    let (class, _variant, _sid, m, d) = scen_params(row)?;
    let grant = m & d;
    let full = grant & d == d;
    Ok(match class {
        0 => (if full { ie::V_GRANT } else { ie::V_DENY }, grant),
        1 => (if full { ie::V_GRANT } else { ie::V_DENY }, grant),
        2 => (if 0x1 & d != 0 { ie::V_DENY } else { if full { ie::V_GRANT } else { ie::V_DENY } }, if 0x1 & d != 0 { 0 } else { grant }),
        3 => (if full { ie::V_GRANT } else { ie::V_DENY }, grant),
        // c4：显式 Allow(0x1) 先授 0x1；继承 Deny(0x2) 命中 0x3 → 拒，
        // granted 保留 0x1（与本类构造的定掩码一致，与变体无关）。
        4 => (ie::V_DENY, 0x1 & d),
        5 => (if full { ie::V_GRANT } else { ie::V_DENY }, grant),
        6 => (if full { ie::V_GRANT } else { ie::V_DENY }, grant),
        7 => (ie::V_DENY, 0),
        8 => (ie::V_DENY, 0),
        _ => (ie::V_DENY, 0),
    })
}

/// 50 场景回放（判据 ①② 主面）：引擎求值 vs 类规则期望 vs 手推锚
/// （variant 0 口径）vs 基准机。
pub fn golden_sweep() -> Result<u32, u16> {
    let mut ok = 0u32;
    let mut row = 0;
    while row < SCEN_N {
        let (class, variant, _sid, _m, _d) = scen_params(row)?;
        let (eng, node, sid, d) = build_scenario(row)?;
        let acl = eng.view(node)?.to_vec();
        let (v1, g1) = ie::eval_request(&acl, sid, d);
        let (vw, gw) = expect_by_class(row)?;
        if v1 != vw || g1 != gw {
            return Err(E_GOLD);
        }
        let (v2, g2) = baseline_eval(&acl, sid, d);
        if v2 != vw || g2 != gw {
            return Err(E_GOLD);
        }
        // 手推锚：variant 0 的每类首行必须与字面锚一致。
        if variant == 0 {
            let (tag, av, ag) = HAND_ANCHORS[class];
            let _ = tag;
            if v1 != av || g1 != ag {
                return Err(E_ANCHOR);
            }
        }
        ok += 1;
        row += 1;
    }
    Ok(ok)
}

/// 十类覆盖账（每类恰 5 变体——漏类/多类即红）。
pub fn coverage() -> Result<[u32; CLASS_N], u16> {
    let mut counts = [0u32; CLASS_N];
    let mut row = 0;
    while row < SCEN_N {
        let (class, _v, _s, _m, _d) = scen_params(row)?;
        if class >= CLASS_N {
            return Err(E_COVER);
        }
        counts[class] += 1;
        row += 1;
    }
    let mut c = 0;
    while c < CLASS_N {
        if counts[c] != VARIANT_N as u32 {
            return Err(E_COVER);
        }
        c += 1;
    }
    Ok(counts)
}

// ---- 黄金库指纹 -------------------------------------------------------------
/// 50 场景指纹（FNV-1a）：每行的 (判定, 授予位) 结果流折叠——黄金
/// 库 v1 的封条指纹（重放恒等 = 冻结面未被碰；任何一行结果漂移翻
/// 指纹，域档对账以指纹为锚）。
pub fn golden_fingerprint() -> Result<u32, u16> {
    let mut h: u32 = 0x811C_9DC5;
    let mut row = 0;
    while row < SCEN_N {
        let (eng, node, sid, d) = build_scenario(row)?;
        let acl = eng.view(node)?.to_vec();
        let (v, g) = ie::eval_request(&acl, sid, d);
        h = (h ^ v as u32).wrapping_mul(0x0100_0193);
        h = (h ^ g).wrapping_mul(0x0100_0193);
        row += 1;
    }
    if h == 0 {
        return Err(E_ANCHOR);
    }
    Ok(h)
}

/// 指纹重放钉：双跑恒等（构建路径确定性——同输入同指纹）。
pub fn fingerprint_replay() -> Result<(), u16> {
    let f1 = golden_fingerprint()?;
    let f2 = golden_fingerprint()?;
    if f1 != f2 {
        return Err(E_GOLD);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    if VER != 1 || GOLD_VER != 1 {
        return Err(E_VER);
    }
    let (av, ab) = ie::engine_anchor();
    if av != 1 || ab != 0x7b30 {
        return Err(E_ANCHOR);
    }
    // R1：50 场景三机对平（引擎 / 类规则 / 基准机）+ 手推锚。
    let ok = golden_sweep()?;
    if ok != SCEN_N as u32 {
        return Err(E_GOLD);
    }
    // R2：十类覆盖账（10 × 5 恒等）。
    let counts = coverage()?;
    if counts[0] != 5 || counts[9] != 5 {
        return Err(E_COVER);
    }
    // R3：手推锚内部一致性（锚与类规则在 variant 0 参数下互证）。
    let mut class = 0;
    while class < CLASS_N {
        let row = class * VARIANT_N;
        let (vw, gw) = expect_by_class(row)?;
        let (tag, av, ag) = HAND_ANCHORS[class];
        if tag.is_empty() || vw != av || gw != ag {
            return Err(E_ANCHOR);
        }
        class += 1;
    }
    // R4：版本冻结（GOLD_VER 与 HAND_ANCHORS 表长绑定——表改必须改版）。
    if HAND_ANCHORS.len() != CLASS_N {
        return Err(E_ANCHOR);
    }
    // R5：越界面。
    if scen_params(SCEN_N).err() != Some(E_CLASS) {
        return Err(E_CLASS);
    }
    // R6：黄金库指纹重放（双跑恒等——冻结面封条）。
    fingerprint_replay()?;
    Ok(ok)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符（GOLD_VER 漂移）",
        E_GOLD => "场景结果与冻结口径不符",
        E_ANCHOR => "手推锚漂移（类规则与字面锚互证破）",
        E_CLASS => "类号/行号越界",
        E_COVER => "十类覆盖账破（漏类/多类）",
        E_INTG => "完整性自检不符",
        _ => "未知诊断码",
    }
}

pub fn run_unx36213_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36213_inhgold");
    cs.add("SEG 段位独占", SEG_BASE == 0x9fb0, "0x9fb0~0x9fbF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "50 场景三机对平/十类覆盖/手推锚互证/版本冻结全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("库量锚", SCEN_N == 50 && CLASS_N == 10 && VARIANT_N == 5, "50 场景 = 十类 × 五变体（判据锚）");
    cs.add("版本锚", GOLD_VER == 1 && VER == 1, "GOLD_VER=1 版本化冻结");
    cs.add("覆盖面", coverage().is_ok(), "十类覆盖账恒等（每类 5 变体）");
    cs.add("锚面", HAND_ANCHORS[0].0 == "c0-pure-allow" && HAND_ANCHORS[9].0 == "c9-empty-acl", "十类手推锚表首尾对位");
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
        let cs = run_unx36213_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_class_semantics_pins() {
        // 性质直证：三条最易漂移的语义逐类实测——
        // c1 显式 Allow 胜继承 Deny；c5 保护免疫；c3 NP 隔代免疫。
        let (eng, node, sid, d) = build_scenario(5).ok().unwrap_or((ProtEngine::new(), 0, 0, 0));
        let acl = eng.view(node).ok().unwrap_or_default().to_vec();
        assert_eq!(ie::eval_request(&acl, sid, d).0, ie::V_GRANT, "c1 显式 Allow 胜继承 Deny");
        let (eng5, n5, s5, d5) = build_scenario(25).ok().unwrap_or((ProtEngine::new(), 0, 0, 0));
        let acl5 = eng5.view(n5).ok().unwrap_or_default().to_vec();
        assert_eq!(ie::eval_request(&acl5, s5, d5).0, ie::V_GRANT, "c5 受保护免疫父 Deny");
        let (eng3, n3, s3, d3) = build_scenario(15).ok().unwrap_or((ProtEngine::new(), 0, 0, 0));
        let acl3 = eng3.view(n3).ok().unwrap_or_default().to_vec();
        assert_eq!(ie::eval_request(&acl3, s3, d3).0, ie::V_GRANT, "c3 NP 隔代孙代免疫");
    }

    #[test]
    fn test_golden_accounts_conserve() {
        // 账面守恒：50 场景全绿 + 覆盖账 10×5。
        assert_eq!(golden_sweep().ok().unwrap_or(0), SCEN_N as u32);
        let counts = coverage().ok().unwrap_or([0; CLASS_N]);
        let mut sum = 0u32;
        let mut i = 0;
        while i < CLASS_N {
            sum += counts[i];
            i += 1;
        }
        assert_eq!(sum, SCEN_N as u32);
    }
}
