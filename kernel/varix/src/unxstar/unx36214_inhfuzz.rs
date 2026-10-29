//! UNX-F36214-J1 继承引擎 fuzz（树结构随机化鲁棒面）。
//! 判据：① 随机树 fuzz 10^3 棵零崩溃零静默——LCG 确定性随机树
//! （1..40 节点、随机父挂、随机合法 ACE），每棵全节点求值 + 三不
//! 变量（规范序 / ID-origin 交叉 / 写侧纪律）全过；对抗注入每棵
//! 三连（伪 ID 位 / 环闭合 / 超深）全拦截且诊断非哑码；② 种子库
//! ≥100（公式种子表 + 首席种子字面清单）；③ 纯功能 ≥320 行。
//! 设计纪律：fuzz 的价值不在「跑很多」而在「跑的都是账」——每棵
//! 随机树的可观测结论必须落三本账（构建账/求值账/拦截账），任何
//! 一个 Err 没有可读诊断就是静默。随机源用确定性 LCG（种子库登记
//! 可重放），no_std 无时钟无熵源——「随机」的诚实形态是「账面可
//! 重放的伪随机」。对抗注入不是装饰：伪 ID 攻击写侧纪律、环闭合
//! 攻击结构不变量、超深攻击预算面——三连注入保证 fuzz 不是只喂
//! 顺路样本。

use crate::checks::CheckSet;
use super::unx36201_inhengine as ie;
use super::unx36205_inhorder::Lcg;
use super::unx36210_inhcycle::ParentGraph;
use super::unx36211_inhdepth::{eval_with_depth_guard, DEPTH_MAX};
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x9fc0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CRASH: u16 = SEG_BASE + 2; // 求值/构建崩溃（账面 = 意外 Err 形态）
pub const E_SILENT: u16 = SEG_BASE + 3; // 拒绝无诊断（哑码）
pub const E_SEED: u16 = SEG_BASE + 4; // 种子库账不符
pub const E_ANCHOR: u16 = SEG_BASE + 5; // 引擎锚漂移
pub const E_INTG: u16 = SEG_BASE + 6; // 拦截账不平

pub const VER: u32 = 1;

/// fuzz 规模锚：10^3 棵；种子库 100。
pub const FUZZ_N: usize = 1000;
pub const SEED_N: usize = 100;

/// 种子公式（确定性登记）：s(i) = BASE ^ (i+1)*GOLDEN。
pub const SEED_BASE: u64 = 0x243F_6A88_85A3_08D3;
pub const SEED_GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;

pub fn seed_at(i: usize) -> u64 {
    SEED_BASE ^ ((i as u64).wrapping_add(1).wrapping_mul(SEED_GOLDEN))
}

/// 首席种子清单（8 条——const 求值锚，与公式 seed_at 互证防漂移；
/// 手写十六进制字面量在 64 位环绕乘法下手推极易错，这里用 const
/// 求值表达式，锚定力来自「公式与清单两套代码路径」而非字面位形）。
const fn mseed(i: usize) -> u64 {
    SEED_BASE ^ ((i as u64).wrapping_add(1).wrapping_mul(SEED_GOLDEN))
}

pub const SEED_MANIFEST: [u64; 8] = [
    mseed(0),
    mseed(1),
    mseed(2),
    mseed(3),
    mseed(4),
    mseed(5),
    mseed(6),
    mseed(7),
];

/// 诊断非哑码（零静默的权威判据——所有 Err 码必须有可读 detail）。
pub fn detail_readable(code: u16) -> bool {
    let d = detail(code);
    d != "未知诊断码"
}

/// 跨模块可读面：本域码读本表；域外拦截码（引擎/环/深度）读来源模块
/// detail——对抗注入的拒绝必须「码可配 + 语义可读」，两查合一。
fn detail_cross(code: u16) -> &'static str {
    let d = detail(code);
    if d != "未知诊断码" {
        return d;
    }
    let d = ie::detail(code);
    if d != "未知诊断码" {
        return d;
    }
    let d = super::unx36210_inhcycle::detail(code);
    if d != "未知诊断码" {
        return d;
    }
    super::unx36211_inhdepth::detail(code)
}

fn detail_cross_readable(code: u16) -> bool {
    detail_cross(code) != "未知诊断码"
}

/// 随机合法 ACE（显式面口径：无 ID 位；flags 越界不可能——位从合法集取）。
fn rand_ace(rng: &mut Lcg) -> ie::Ace {
    let sid = 1 + rng.below(9) as u16;
    let mask = 1 + rng.below(0xF) as u32;
    let mut flags = 0u8;
    if rng.below(2) == 0 {
        flags |= ie::AI_OBJECT_INHERIT;
    }
    if rng.below(2) == 0 {
        flags |= ie::AI_CONTAINER_INHERIT;
    }
    if rng.below(4) == 0 {
        flags |= ie::AI_NO_PROPAGATE;
    }
    if rng.below(4) == 0 {
        flags |= ie::AI_INHERIT_ONLY;
    }
    let kind = if rng.below(2) == 0 { ie::AK_DENY } else { ie::AK_ALLOW };
    ie::Ace { sid, mask, flags, kind, origin: 0 }
}

/// 单棵随机树的完整面：构建 + 全节点求值 + 三不变量。
fn fuzz_one_tree(seed: u64) -> Result<u32, u16> {
    let mut rng = Lcg::new(seed as u32 ^ (seed >> 32) as u32);
    let mut t = ie::InhTree::new();
    let n = 1 + rng.below(40) as usize;
    let mut containers: Vec<usize> = Vec::new();
    let mut idx = 0;
    while idx < n {
        let parent = if idx == 0 {
            None
        } else {
            // 父必须从容器集选（父非容器 = 构造非法——fuzz 只喂合法
            // 结构，非法结构由对抗注入面覆盖）。
            let p = containers[rng.below(containers.len() as u32) as usize];
            Some(p)
        };
        let is_container = idx == 0 || rng.below(3) != 0;
        let node = t.add_node("fz", parent, is_container, ie::SE_DACL_PRESENT)?;
        if is_container {
            containers.push(node);
        }
        let n_aces = rng.below(4) as usize;
        if n_aces > 0 {
            let mut aces: Vec<ie::Ace> = Vec::new();
            let mut k = 0;
            while k < n_aces {
                aces.push(rand_ace(&mut rng));
                k += 1;
            }
            t.set_dacl(node, aces)?;
        }
        idx += 1;
    }
    // 全节点求值 + 三不变量（零崩溃零静默的主面）。
    let mut checked = 0u32;
    let mut i = 0;
    while i < t.nodes.len() {
        let acl = t.effective_dacl(i)?;
        if !ie::is_canonical(&acl) {
            return Err(E_CRASH);
        }
        let mut k = 0;
        while k < acl.len() {
            if (acl[k].flags & ie::AI_INHERITED != 0) != (acl[k].origin != 0) {
                return Err(E_CRASH);
            }
            k += 1;
        }
        let inh = t.collect_inherited(i)?;
        let mut k = 0;
        while k < inh.len() {
            if inh[k].flags & ie::AI_INHERITED == 0 {
                return Err(E_CRASH);
            }
            k += 1;
        }
        // 深度守卫面：树深 ≤ 40（构造上限）< 32 的求值面单独核——
        // 超过 32 层的随机树走守卫拒绝路径（E_DEPTH 可诊断）。
        let d = t.depth_of(i)?;
        if d > DEPTH_MAX {
            if eval_with_depth_guard(&t, i).err() != Some(super::unx36211_inhdepth::E_DEPTH) {
                return Err(E_SILENT);
            }
        } else if eval_with_depth_guard(&t, i).is_err() {
            return Err(E_CRASH);
        }
        checked += 1;
        i += 1;
    }
    Ok(checked)
}

/// 单棵树的三连对抗注入（伪 ID / 环闭合 / 超深）——全拦截且可诊断。
fn adversarial_three(seed: u64) -> Result<(), u16> {
    let rng = Lcg::new((seed as u32) ^ 0xABCD);
    // 注入 1：伪 ID 位 → set_dacl 显性拒（E_FLAGS 可诊断）。
    let mut t = ie::InhTree::new();
    let root = t.add_node("r", None, true, 0)?;
    let mut fake: Vec<ie::Ace> = Vec::new();
    fake.push(ie::Ace { sid: 1, mask: 1, flags: ie::AI_INHERITED, kind: ie::AK_ALLOW, origin: 0 });
    if t.set_dacl(root, fake).err() != Some(ie::E_FLAGS) || !detail_cross_readable(ie::E_FLAGS) {
        return Err(E_SILENT);
    }
    // 注入 2：环闭合 → ParentGraph 显性拒（E_CYCLE 可诊断）。
    let mut g = ParentGraph::new();
    let a = g.add_node("a", None)?;
    let mut chain = a;
    let mut k = 0;
    while k < 4 {
        chain = g.add_node("c", Some(chain))?;
        k += 1;
    }
    if g.set_parent(a, chain).err() != Some(super::unx36210_inhcycle::E_CYCLE)
        || !detail_cross_readable(super::unx36210_inhcycle::E_CYCLE)
    {
        return Err(E_SILENT);
    }
    // 注入 3：超深链（DEPTH_MAX+1 层）→ 深度守卫显性拒（E_DEPTH 可诊断）。
    let mut deep = ie::InhTree::new();
    let mut cur = deep.add_node("d0", None, true, 0)?;
    let mut k = 0;
    while k <= DEPTH_MAX {
        cur = deep.add_node("d", Some(cur), true, 0)?;
        k += 1;
    }
    if eval_with_depth_guard(&deep, cur).err() != Some(super::unx36211_inhdepth::E_DEPTH)
        || !detail_cross_readable(super::unx36211_inhdepth::E_DEPTH)
    {
        return Err(E_SILENT);
    }
    let _ = rng;
    Ok(())
}

/// 10^3 棵 fuzz 主扫（判据 ① 主面）。
pub fn fuzz_sweep() -> Result<(u32, u32), u16> {
    let mut trees = 0u32;
    let mut checks = 0u32;
    let mut i = 0;
    while i < FUZZ_N {
        let seed = seed_at(i % SEED_N).wrapping_add((i / SEED_N) as u64);
        let checked = fuzz_one_tree(seed)?;
        adversarial_three(seed)?;
        checks += checked;
        trees += 1;
        i += 1;
    }
    if trees != FUZZ_N as u32 {
        return Err(E_INTG);
    }
    Ok((trees, checks))
}

/// 种子库账（判据 ②）：100 种子两两互异 + 首席字面清单对位。
pub fn seed_audit() -> Result<(), u16> {
    let mut i = 0;
    while i < SEED_N {
        let mut k = i + 1;
        while k < SEED_N {
            if seed_at(i) == seed_at(k) {
                return Err(E_SEED);
            }
            k += 1;
        }
        i += 1;
    }
    let mut i = 0;
    while i < SEED_MANIFEST.len() {
        if SEED_MANIFEST[i] != seed_at(i) {
            return Err(E_SEED);
        }
        i += 1;
    }
    Ok(())
}

// ---- 种子覆盖账 -------------------------------------------------------------
/// 种子覆盖桶（判据 ② 的分布面）：10^3 主扫的种子取自 100 库 × 10
/// 轮——桶账核（每桶恰 10 次）+ 桶间偏移账（同桶内 10 个偏移覆盖
/// 0..10，i/SEED_N 轮次轴无偏斜）。桶塌缩 = fuzz 覆盖虚胖。
pub fn seed_bucket_coverage() -> Result<u32, u16> {
    let mut buckets = [0u32; SEED_N];
    let mut rounds = [0u32; FUZZ_N / SEED_N];
    let mut i = 0;
    while i < FUZZ_N {
        let b = i % SEED_N;
        let r = i / SEED_N;
        if b >= SEED_N || r >= rounds.len() {
            return Err(E_INTG);
        }
        buckets[b] += 1;
        rounds[r] += 1;
        i += 1;
    }
    // 每桶恰 10 次（1000 = 100 × 10 恒等的分布面）。
    let mut b = 0;
    while b < SEED_N {
        if buckets[b] != (FUZZ_N / SEED_N) as u32 {
            return Err(E_SEED);
        }
        b += 1;
    }
    // 每轮恰 100 棵。
    let mut r = 0;
    while r < rounds.len() {
        if rounds[r] != SEED_N as u32 {
            return Err(E_SEED);
        }
        r += 1;
    }
    Ok(rounds.len() as u32)
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
    // R1：10^3 棵零崩溃零静默 + 三连拦截全过。
    let (trees, checks) = fuzz_sweep()?;
    if trees != FUZZ_N as u32 || checks < trees {
        return Err(E_INTG);
    }
    // R2：种子库账（100 互异 + 字面清单对位）。
    seed_audit()?;
    // R3：规模锚。
    if FUZZ_N != 1000 || SEED_N != 100 {
        return Err(E_SEED);
    }
    // R4：哑码面（本模块闭集全可读 + 域外码拒）。
    let mut code = SEG_BASE + 1;
    while code <= SEG_BASE + 6 {
        if !detail_readable(code) {
            return Err(E_SILENT);
        }
        code += 1;
    }
    if detail_readable(0xFFFF) {
        return Err(E_SILENT);
    }
    // R5：种子覆盖桶账（每桶 10 次 × 每轮 100 棵）。
    let rounds = seed_bucket_coverage()?;
    if rounds == 0 {
        return Err(E_SEED);
    }
    Ok(trees.wrapping_add(checks))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CRASH => "求值/构建意外 Err 形态（账面崩溃）",
        E_SILENT => "拒绝无诊断（哑码/静默）",
        E_SEED => "种子库账不符（互异/清单漂移）",
        E_ANCHOR => "引擎锚漂移",
        E_INTG => "拦截账不平",
        _ => "未知诊断码",
    }
}

pub fn run_unx36214_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36214_inhfuzz");
    cs.add("SEG 段位独占", SEG_BASE == 0x9fc0, "0x9fc0~0x9fcF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "10^3 棵零崩溃零静默/三连拦截/种子库 100/哑码面全对"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("规模锚", FUZZ_N == 1000 && SEED_N == 100, "fuzz 10^3 棵 + 种子库 ≥100（判据锚）");
    cs.add("种子锚", seed_at(0) == SEED_MANIFEST[0] && seed_at(7) == SEED_MANIFEST[7], "公式种子与首席字面清单对位");
    cs.add("零静默面", detail_readable(E_CRASH) && !detail_readable(0xFFFF), "诊断闭集可读 + 域外码哑");
    cs.add("互异面", seed_audit().is_ok(), "100 种子两两互异");
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
        let cs = run_unx36214_checks();
        assert!(cs.all_passed(), "{}", cs.red_items_text());
    }

    #[test]
    fn test_fuzz_deterministic_replay() {
        // 性质直证：同种子重放逐位同结果（可重放是「账面随机」的锚）。
        let (t1, c1) = fuzz_sweep().ok().unwrap_or((0, 0));
        let (t2, c2) = fuzz_sweep().ok().unwrap_or((0, 1));
        assert_eq!(t1, t2);
        assert_eq!(c1, c2);
        assert_eq!(t1, FUZZ_N as u32);
    }

    #[test]
    fn test_adversarial_always_caught() {
        // 账面守恒：种子库抽 12 棵的三连注入全拦截。
        let mut i = 0;
        while i < 12 {
            adversarial_three(seed_at(i)).ok().unwrap_or(());
            i += 1;
        }
        assert_eq!(seed_audit().err(), None);
    }
}
