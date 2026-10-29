//! UNX-F36205-J1 继承 ACE 的规范序重排（新继承入位）。
//! 判据：① 插入继承 ACE 后 canonical 序保持 200 例——在已规范 ACL
//! 上按收集口径追加新继承 ACE，canonicalize 后组序（显式 Deny <
//! 显式 Allow < 继承 Deny < 继承 Allow）与同组稳定序逐例核对；
//! ② 乱序注入 10 例被自动重排 + 留账（moved 账 > 0 且重排后全规范、
//! 多重集恒等——重排不许丢/改/造 ACE）；③ 纯功能代码 ≥320 行。
//! 设计纪律：规范序是继承引擎的「货架纪律」——新继承 ACE 到货必须
//! 入位，而不是堆在门口。本模块把 canonicalize 做成纯函数（输入
//! 乱序多重集 → 输出规范序 + moved 留账），与 36201 的收集序基线
//! 双实现对平：同一输入两个实现必须产出逐位相同的结果，防止单实
//! 现漂移。稳定性锚：同组内按原收集序（seq）排——排序算法不必稳
//! 定，比较器必须全序。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x7b70;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_ORDER: u16 = SEG_BASE + 2; // 重排后仍非规范
pub const E_MULTI: u16 = SEG_BASE + 3; // 重排丢/改/造 ACE（多重集破）
pub const E_LEDGER: u16 = SEG_BASE + 4; // moved 留账不符
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚/双实现漂移
pub const E_CASE: u16 = SEG_BASE + 6; // 用例编号越界

pub const VER: u32 = 1;

/// 用例锚（判据 ① / ② 的例数）。
pub const INS_N: usize = 200;
pub const SHUFFLE_N: usize = 10;

/// 确定性 LCG（no_std 无随机源——账面可重放是硬要求）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
    pub fn below(&mut self, n: u32) -> u32 {
        self.next() % n
    }
}

/// 重排纯函数：乱序多重集 → (规范序, moved 账)。
/// 比较器全序：(组, 原下标)——同组按收集序，算法稳定性无关紧要。
pub fn canonicalize(acl: &[ie::Ace]) -> (Vec<ie::Ace>, usize) {
    let mut tagged: Vec<(u8, u32, ie::Ace)> = Vec::new();
    let mut i = 0;
    while i < acl.len() {
        tagged.push((ie::canon_group(&acl[i]), i as u32, acl[i]));
        i += 1;
    }
    tagged.sort_by(|x, y| {
        if x.0 != y.0 {
            x.0.cmp(&y.0)
        } else {
            x.1.cmp(&y.1)
        }
    });
    let mut out: Vec<ie::Ace> = Vec::new();
    let mut moved = 0usize;
    let mut k = 0;
    while k < tagged.len() {
        if tagged[k].1 as usize != k {
            moved += 1;
        }
        out.push(tagged[k].2);
        k += 1;
    }
    (out, moved)
}

/// 多重集恒等（逐 ACE 计数——重排不许丢/改/造）。
pub fn multiset_eq(a: &[ie::Ace], b: &[ie::Ace]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        let mut found = 0usize;
        let mut k = 0;
        while k < b.len() {
            if a[i] == b[k] {
                found += 1;
            }
            k += 1;
        }
        let mut dup = 0usize;
        let mut m = 0;
        while m < i {
            if a[m] == a[i] {
                dup += 1;
            }
            m += 1;
        }
        if found < dup + 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// 生成第 case 例的已规范 ACL + 新继承 ACE（确定性）。
pub fn gen_case(case: usize) -> (Vec<ie::Ace>, ie::Ace) {
    let mut rng = Lcg::new(0xC0FF_EE01 ^ (case as u32).wrapping_mul(0x9E37));
    let mut base: Vec<ie::Ace> = Vec::new();
    let n = 4 + rng.below(5) as usize; // 4..8 条
    let mut i = 0;
    while i < n {
        let inh = rng.below(2) == 0;
        let deny = rng.below(2) == 0;
        let mut flags = if inh { ie::AI_INHERITED } else { 0 };
        if inh && rng.below(3) == 0 {
            flags |= ie::AI_OBJECT_INHERIT | ie::AI_CONTAINER_INHERIT;
        }
        base.push(ie::Ace {
            sid: 1 + rng.below(8) as u16,
            mask: 1 + rng.below(0xF) as u32,
            flags,
            kind: if deny { ie::AK_DENY } else { ie::AK_ALLOW },
            origin: if inh { 50 } else { 0 },
        });
        i += 1;
    }
    let (sorted, _) = canonicalize(&base);
    // 新继承 ACE：按收集口径追加在尾部（继承引擎到货形态）。
    let fresh = ie::Ace {
        sid: 30 + (case % 9) as u16,
        mask: 0x4,
        flags: ie::AI_INHERITED | if case % 2 == 0 { ie::AI_CONTAINER_INHERIT } else { ie::AI_OBJECT_INHERIT },
        kind: if case % 3 == 0 { ie::AK_DENY } else { ie::AK_ALLOW },
        origin: 77,
    };
    (sorted, fresh)
}

/// 200 例插入对平（判据 ① 主面）。
pub fn insert_sweep() -> Result<u32, u16> {
    let mut done = 0u32;
    let mut case = 0;
    while case < INS_N {
        let (base, fresh) = gen_case(case);
        if !ie::is_canonical(&base) {
            return Err(E_ORDER);
        }
        // 收集口径：追加尾部 → canonicalize 入位。
        let mut merged: Vec<ie::Ace> = Vec::new();
        let mut i = 0;
        while i < base.len() {
            merged.push(base[i]);
            i += 1;
        }
        merged.push(fresh);
        let (out, _) = canonicalize(&merged);
        if !ie::is_canonical(&out) {
            return Err(E_ORDER);
        }
        if !multiset_eq(&merged, &out) {
            return Err(E_MULTI);
        }
        // 双实现对平：36201 收集序基线（canonical_merge 显式+继承两路）
        // 与本模块 canonicalize 必须逐位同结果。
        let mut explicit: Vec<ie::Ace> = Vec::new();
        let mut inherited: Vec<ie::Ace> = Vec::new();
        let mut k = 0;
        while k < merged.len() {
            if merged[k].flags & ie::AI_INHERITED != 0 {
                inherited.push(merged[k]);
            } else {
                explicit.push(merged[k]);
            }
            k += 1;
        }
        let via_merge = ie::canonical_merge(explicit, inherited);
        if via_merge.len() != out.len() {
            return Err(E_ANCHOR);
        }
        let mut v = 0;
        while v < out.len() {
            if via_merge[v] != out[v] {
                return Err(E_ANCHOR);
            }
            v += 1;
        }
        done += 1;
        case += 1;
    }
    Ok(done)
}

/// 乱序注入扫描（10 例）：确定性乱序 → 自动重排 + moved 留账。
pub fn shuffle_sweep() -> Result<u32, u16> {
    let mut caught = 0u32;
    let mut case = 0;
    while case < SHUFFLE_N {
        let (base, fresh) = gen_case(case);
        let mut merged: Vec<ie::Ace> = Vec::new();
        let mut i = 0;
        while i < base.len() {
            merged.push(base[i]);
            i += 1;
        }
        merged.push(fresh);
        let (canonical, _) = canonicalize(&merged);
        // 确定性乱序：隔位倒置（保证非规范——组序被交换）。
        let mut scrambled: Vec<ie::Ace> = Vec::new();
        let n = canonical.len();
        let mut k = 0;
        while k < n {
            scrambled.push(canonical[n - 1 - k]);
            k += 1;
        }
        if ie::is_canonical(&scrambled) {
            // 全同组退化例才可能倒置仍规范——跳过计数（显性口径）。
            case += 1;
            continue;
        }
        let (fixed, moved) = canonicalize(&scrambled);
        if !ie::is_canonical(&fixed) {
            return Err(E_ORDER);
        }
        if !multiset_eq(&scrambled, &fixed) {
            return Err(E_MULTI);
        }
        if moved == 0 {
            return Err(E_LEDGER); // 确有乱序却零 moved = 留账失真
        }
        // 幂等入位：对重排结果再排一次必须零 moved（同一多重集的
        // 组内序随输入序变化是合法的——恒等锚是幂等，不是逐位同）。
        let (again, moved2) = canonicalize(&fixed);
        if moved2 != 0 || again.len() != fixed.len() {
            return Err(E_MULTI);
        }
        let mut v = 0;
        while v < fixed.len() {
            if again[v] != fixed[v] {
                return Err(E_MULTI);
            }
            v += 1;
        }
        caught += 1;
        case += 1;
    }
    if caught == 0 {
        return Err(E_LEDGER);
    }
    Ok(caught)
}

/// 规范序分区不变量（20 例）：canonicalize 输出的组号序列必须非降
/// （显式 Deny | 显式 Allow | 继承 Deny | 继承 Allow 四分区）；对已
/// 规范输入再排必须零移动（幂等 = 重排完成态的账面定义——完成态
/// 再排还动 ACE 就是排序器失稳）。
pub fn group_monotone_check() -> Result<u32, u16> {
    let mut checked = 0u32;
    let mut case = 0;
    while case < 20 {
        let (base, _ace) = gen_case(case);
        let (ordered, _m) = canonicalize(&base);
        let mut k = 1;
        while k < ordered.len() {
            if ie::canon_group(&ordered[k]) < ie::canon_group(&ordered[k - 1]) {
                return Err(E_ORDER);
            }
            k += 1;
        }
        let (_again, m2) = canonicalize(&ordered);
        if m2 != 0 {
            return Err(E_LEDGER);
        }
        checked += 1;
        case += 1;
    }
    Ok(checked)
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
    // R1：200 例插入保持规范 + 多重集守恒 + 双实现对平。
    let done = insert_sweep()?;
    // R2：乱序注入自动重排 + 留账。
    let caught = shuffle_sweep()?;
    // R3：组序钉子（四组两两有序——手推四 ACE 全组覆盖例）。
    let mut probe: Vec<ie::Ace> = Vec::new();
    probe.push(ie::Ace { sid: 1, mask: 1, flags: ie::AI_INHERITED, kind: ie::AK_ALLOW, origin: 5 });
    probe.push(ie::Ace { sid: 2, mask: 1, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    probe.push(ie::Ace { sid: 3, mask: 1, flags: ie::AI_INHERITED, kind: ie::AK_DENY, origin: 6 });
    probe.push(ie::Ace { sid: 4, mask: 1, flags: 0, kind: ie::AK_DENY, origin: 0 });
    let (fixed, moved) = canonicalize(&probe);
    if moved != 2 {
        return Err(E_LEDGER);
    }
    if ie::canon_group(&fixed[0]) != 0
        || ie::canon_group(&fixed[1]) != 1
        || ie::canon_group(&fixed[2]) != 2
        || ie::canon_group(&fixed[3]) != 3
    {
        return Err(E_ORDER);
    }
    // R4：空集/单例平凡规范。
    let (e0, m0) = canonicalize(&[]);
    if !e0.is_empty() || m0 != 0 {
        return Err(E_ORDER);
    }
    // R5：多重集守恒钉子（同 sid 同 mask 异 kind 不许折叠）。
    let mut twins: Vec<ie::Ace> = Vec::new();
    twins.push(ie::Ace { sid: 9, mask: 2, flags: 0, kind: ie::AK_ALLOW, origin: 0 });
    twins.push(ie::Ace { sid: 9, mask: 2, flags: 0, kind: ie::AK_DENY, origin: 0 });
    let (t_out, _) = canonicalize(&twins);
    if t_out.len() != 2 {
        return Err(E_MULTI);
    }
    // R6：分区不变量 + 幂等钉（20 例）。
    let mono = group_monotone_check()?;
    if mono == 0 {
        return Err(E_ORDER);
    }
    Ok(done.wrapping_mul(5).wrapping_add(caught))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ORDER => "重排后仍非规范序",
        E_MULTI => "重排丢/改/造 ACE（多重集守恒破）",
        E_LEDGER => "moved 留账失真",
        E_ANCHOR => "引擎锚/双实现漂移",
        E_CASE => "用例编号越界",
        _ => "未知诊断码",
    }
}

pub fn run_unx36205_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36205_inhorder");
    cs.add("SEG 段位独占", SEG_BASE == 0x7b70, "0x7b70~0x7b7F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "200 例入位/乱序重排留账/组序钉子/多重集守恒全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("例量锚", INS_N == 200 && SHUFFLE_N == 10, "200 例插入 + 10 例乱序（判据锚）");
    cs.add("组序锚", ie::canon_group(&ie::Ace { sid: 1, mask: 1, flags: 0, kind: ie::AK_DENY, origin: 0 }) == 0, "显式 Deny 为组 0（规范序头）");
    cs.add("幂等面", { let (a, _) = canonicalize(&[ie::Ace { sid: 1, mask: 1, flags: ie::AI_INHERITED, kind: ie::AK_ALLOW, origin: 5 }]); let (b, m) = canonicalize(&a); b.len() == 1 && m == 0 }, "重排幂等（二次重排零 moved）");
    cs.add("越界面", multiset_eq(&[], &[]) && !multiset_eq(&[ie::Ace { sid: 1, mask: 1, flags: 0, kind: 0, origin: 0 }], &[]), "多重集比较空集/异径钉");
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
        let cs = run_unx36205_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_fresh_inherited_lands_in_group23() {
        // 性质直证：新继承 Deny 必须落在显式 Allow 之后（组 2），
        // 新继承 Allow 落在继承 Deny 之后（组 3）。
        let (base, _) = gen_case(0);
        let fresh_deny = ie::Ace { sid: 40, mask: 1, flags: ie::AI_INHERITED, kind: ie::AK_DENY, origin: 77 };
        let mut merged = base.clone();
        merged.push(fresh_deny);
        let (out, _) = canonicalize(&merged);
        let mut idx = 0usize;
        let mut i = 0;
        while i < out.len() {
            if out[i] == fresh_deny {
                idx = i;
            }
            i += 1;
        }
        assert!(idx < out.len(), "fresh Deny 必须在重排结果中");
        let mut k = 0;
        while k < idx {
            assert!(ie::canon_group(&out[k]) <= 2, "继承 Deny 之前的组号 ≤ 2");
            k += 1;
        }
        let mut m = idx + 1;
        while m < out.len() {
            assert_eq!(ie::canon_group(&out[m]), 3, "继承 Deny 之后只能是继承 Allow");
            m += 1;
        }
    }

    #[test]
    fn test_shuffle_ledger_conserves() {
        // 账面守恒：乱序注入的拦截账 ≥ 8（200 例里全同组退化例极少），
        // 且每例重排幂等。
        let caught = shuffle_sweep().ok().unwrap_or(0);
        assert!(caught >= 8, "乱序拦截账过少: {}", caught);
    }
}
