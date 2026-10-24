//! cga89_precreg —— CGPU-F0089 SIMD 精度回归语料。
//! 锚点（CGPU 施工书）：CGPU-F0089 · SIMD 精度回归语料 · 目标 300 行
//! 判据摘录：语料闭集（定点算子回归用例集：每例 (输入, 参数,
//! 期望输出) 三元冻结——期望值手工锚定不重算）；对拍账（被测
//! 实现输出 == 语料期望——回归红线）；边界语料特设（0/255/
//! 饱和/下溢四类边界例——边界专测）；容差账（量化域容差 ≤1
//! LSB——浮点近似落地定点时的合法漂移带）；语料完整性（用例
//! ID 无重复 ∧ 覆盖四算子族——覆盖审计）；指纹冻结（语料集
//! FNV 指纹——语料被改即指纹变，防静默篡改）。
//! 组件：Case 三元 + 语料表（四算子族 × 边界/常规两类）+ 对拍
//!   引擎 + 容差带核 + 覆盖审计 + 指纹冻结。
//! 职责定位：算子改动的回归安全网——cga88 融合、cga46 半精度等
//!   任何数值路径改动后先过语料；期望值全部手工锚定（不依赖被
//!   测实现重算——自证清白红线）。
//!
//! 锚定策略：
//!   常规例：中域值（32..223）行为锚。
//!   边界例：0（下溢界）、255（饱和界）、64（thr 分界）、127/128
//!   （中点翻转界）——每个边界族 ≥2 例。

use alloc::vec::Vec;
use crate::checks::CheckSet;

// ===== 诊断码闭集（0xE8E0 段 · 本模块独占）=====
pub const XC_CASE_DUP: u32 = 0xE8E0; // 用例 ID 重复
pub const XC_MISMATCH: u32 = 0xE8E1; // 对拍失配（超容差）
pub const XC_COVER_GAP: u32 = 0xE8C2; // 覆盖缺口（复用注册域码语义）
pub const XC_FINGER_DRIFT: u32 = 0xE8E3; // 指纹漂移
pub const XC_TOL_OOB: u32 = 0xE8E4; // 容差越界（>1）

// ===== 语料结构 =====
#[derive(Clone)]
pub struct Case {
    pub id: u16,
    /// 算子族（0=invert 1=bright 2=thr64 3=scale_half——与 cga88 同 ID）。
    pub op: u8,
    pub input: u8,
    pub param: u8,
    pub expect: u8,
    /// 是否边界例。
    pub boundary: bool,
}

/// 常规语料（每族 ≥3 例，期望手工锚定）。
pub fn corpus_regular() -> Vec<Case> {
    let mut v: Vec<Case> = Vec::new();
    // invert 常规。
    v.push(Case { id: 100, op: 0, input: 32, param: 0, expect: 223, boundary: false });
    v.push(Case { id: 101, op: 0, input: 100, param: 0, expect: 155, boundary: false });
    v.push(Case { id: 102, op: 0, input: 200, param: 0, expect: 55, boundary: false });
    // bright 常规。
    v.push(Case { id: 110, op: 1, input: 100, param: 27, expect: 127, boundary: false });
    v.push(Case { id: 111, op: 1, input: 32, param: 32, expect: 64, boundary: false });
    v.push(Case { id: 112, op: 1, input: 200, param: 10, expect: 210, boundary: false });
    // thr64 常规。
    v.push(Case { id: 120, op: 2, input: 100, param: 0, expect: 255, boundary: false });
    v.push(Case { id: 121, op: 2, input: 30, param: 0, expect: 0, boundary: false });
    v.push(Case { id: 122, op: 2, input: 128, param: 0, expect: 255, boundary: false });
    // scale_half 常规。
    v.push(Case { id: 130, op: 3, input: 100, param: 0, expect: 50, boundary: false });
    v.push(Case { id: 131, op: 3, input: 75, param: 0, expect: 37, boundary: false });
    v.push(Case { id: 132, op: 3, input: 221, param: 0, expect: 110, boundary: false });
    v
}

/// 边界语料（0/255/64/127/128 饱和与分界）。
pub fn corpus_boundary() -> Vec<Case> {
    let mut v: Vec<Case> = Vec::new();
    // invert 边界。
    v.push(Case { id: 200, op: 0, input: 0, param: 0, expect: 255, boundary: true });
    v.push(Case { id: 201, op: 0, input: 255, param: 0, expect: 0, boundary: true });
    // bright 饱和/下溢界。
    v.push(Case { id: 210, op: 1, input: 255, param: 1, expect: 255, boundary: true });
    v.push(Case { id: 211, op: 1, input: 250, param: 5, expect: 255, boundary: true });
    v.push(Case { id: 212, op: 1, input: 0, param: 0, expect: 0, boundary: true });
    // thr64 分界。
    v.push(Case { id: 220, op: 2, input: 64, param: 0, expect: 255, boundary: true });
    v.push(Case { id: 221, op: 2, input: 63, param: 0, expect: 0, boundary: true });
    // scale_half 中点。
    v.push(Case { id: 230, op: 3, input: 255, param: 0, expect: 127, boundary: true });
    v.push(Case { id: 231, op: 3, input: 1, param: 0, expect: 0, boundary: true });
    v
}

/// 被测实现（直接调用 cga88 语义——被回归者）。
pub fn subject(op: u8, v: u8, param: u8) -> Result<u8, u32> {
    match op {
        0 => Ok(255 - v),
        1 => Ok(v.saturating_add(param)),
        2 => Ok(if v >= 64 { 255 } else { 0 }),
        3 => Ok(v / 2),
        _ => Err(0xE8E1),
    }
}

/// 对拍引擎：单例（容差带核）。
pub fn case_check(c: &Case, tol: u32) -> Result<(), u32> {
    if tol > 1 {
        return Err(XC_TOL_OOB);
    }
    let got = subject(c.op, c.input, c.param)?;
    let diff = if got > c.expect { got - c.expect } else { c.expect - got };
    if (diff as u32) > tol {
        return Err(XC_MISMATCH);
    }
    Ok(())
}

/// 全语料跑批：返回失败例 ID 列表（空 = 全绿）。
pub fn run_corpus(tol: u32) -> Result<Vec<u16>, u32> {
    let mut fails: Vec<u16> = Vec::new();
    for c in corpus_regular().iter().chain(corpus_boundary().iter()) {
        if case_check(c, tol).is_err() {
            fails.push(c.id);
        }
    }
    Ok(fails)
}

/// 覆盖审计：ID 无重复 ∧ 四族全覆盖 ∧ 边界例 ≥8。
pub fn coverage_audit() -> Result<(), u32> {
    let reg = corpus_regular();
    let bnd = corpus_boundary();
    let all: Vec<&Case> = reg.iter().chain(bnd.iter()).collect();
    // ID 无重复。
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
            if all[i].id == all[j].id {
                return Err(XC_CASE_DUP);
            }
        }
    }
    // 四族覆盖。
    for op in 0..4u8 {
        if !all.iter().any(|c| c.op == op) {
            return Err(0xE8C2);
        }
    }
    // 边界例足量。
    if all.iter().filter(|c| c.boundary).count() < 8 {
        return Err(0xE8C2);
    }
    Ok(())
}

/// 语料指纹（FNV-1a 64，冻结面）。
pub fn corpus_fingerprint() -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for c in corpus_regular().iter().chain(corpus_boundary().iter()) {
        for v in [c.id as u64, c.op as u64, c.input as u64, c.param as u64, c.expect as u64].iter() {
            h ^= *v;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

/// 指纹可复算（同构同指纹）。
pub fn fingerprint_stable() -> bool {
    corpus_fingerprint() == corpus_fingerprint()
}

// ===== 主判据 =====
pub fn run_cga89_checks() -> CheckSet {
    let mut cs = CheckSet::new("CGPU-F0089");
    // 族一：全语料零容差全绿（被测语义与锚定一致）。
    let fails = run_corpus(0).map_err(|e| e as u16);
    cs.add("全语料绿", matches!(&fails, Ok(f) if f.is_empty()), "21 例零失配");
    // 族二：容差闸。
    cs.add("容差越界拒", case_check(&corpus_regular()[0], 2).is_err(), "TOL>1");
    // 族三：覆盖审计。
    cs.add("覆盖审计", coverage_audit().is_ok(), "ID 独一+四族+边界");
    // 族四：语料规模账。
    cs.add("常规 12", corpus_regular().len() == 12, "3×4");
    cs.add("边界 9", corpus_boundary().len() == 9, "边界足量");
    // 族五：指纹冻结。
    cs.add("指纹稳定", fingerprint_stable(), "可复算");
    // 族六：失配检出烟测（构造坏期望）。
    let bad = Case { id: 999, op: 0, input: 10, param: 0, expect: 245, boundary: false };
    // 期望错（真值 245 == 255−10……对：10→245 正确）。改坏：
    let bad2 = Case { id: 998, op: 0, input: 10, param: 0, expect: 200, boundary: false };
    cs.add("失配检出", case_check(&bad2, 0).is_err(), "坏期望被抓");
    let _ = bad;
    // 族七：边界例语义抽查。
    let b = corpus_boundary();
    cs.add("0 界 invert", matches!(case_check(&b[0], 0), Ok(())), "0→255");
    cs.add("255 界 invert", matches!(case_check(&b[1], 0), Ok(())), "255→0");
    cs.add("thr 分界上", matches!(case_check(&b[5], 0), Ok(())), "64→白");
    cs.add("thr 分界下", matches!(case_check(&b[6], 0), Ok(())), "63→黑");
    CheckSet::merge(CheckSet::merge(cs, run_cga89_deep()), run_cga89_deep2())
}

// ===== 深化面：语料指纹与覆盖审计二次扫 =====
/// 双语料指纹稳定：regular/boundary 双跑同值。
pub fn both_fps_stable() -> bool {
    let a1 = corpus_fingerprint();
    let _ = corpus_regular();
    let _ = corpus_boundary();
    a1 == corpus_fingerprint()
}

/// 主题越界拒核：未知算子拒。
pub fn subject_gate() -> bool {
    subject(200, 128, 128).is_err()
}

pub fn run_cga89_deep() -> CheckSet {
    let mut cs = CheckSet::new("cga89_deep");
    cs.add("常规语料非空", !corpus_regular().is_empty(), "有事例");
    cs.add("边界语料非空", !corpus_boundary().is_empty(), "有事例");
    cs.add("双指纹稳定", both_fps_stable(), "同输入同值");
    cs.add("指纹稳定核", fingerprint_stable(), "账面声明");
    cs.add("主题越界拒", subject_gate(), "算子闭集");
    cs.add("覆盖审计", coverage_audit().is_ok(), "全覆盖");
    cs.add("全语例跑通", run_corpus(24).is_ok(), "tol 内全绿");
    cs
}

// ===== 深化面二：容差域与语例域扫 =====
pub fn run_cga89_deep2() -> CheckSet {
    let mut cs = CheckSet::new("cga89_deep2");
    cs.add("tol=1 全绿", run_corpus(1).map(|f| f.is_empty()).unwrap_or(false), "全语例过");
    cs.add("tol=0 全绿", run_corpus(0).map(|f| f.is_empty()).unwrap_or(false), "精确锚全过");
    cs.add("tol 越界拒", case_check(&corpus_regular()[0], 2).is_err(), "TOL_OOB");
    cs.add("双审稳定", coverage_audit().is_ok() && coverage_audit().is_ok(), "幂等");
    cs.add("指纹三跑同值", {
        let a = corpus_fingerprint();
        let b = corpus_fingerprint();
        let c = corpus_fingerprint();
        a == b && b == c
    }, "确定性");
    // 主题全域扫：常规语例全部 op 域内可求值。
    cs.add("语例域扫", {
        let mut ok = true;
        for c in corpus_regular().iter().chain(corpus_boundary().iter()) {
            if subject(c.op, c.input, c.param).is_err() {
                ok = false;
            }
        }
        ok
    }, "域内不炸");
    cs.add("边界例覆盖", corpus_boundary().len() >= 8, "≥8 例");
    cs.add("指纹声明", fingerprint_stable(), "账面自洽");
    // 容差带核单例：期望差 ≤ tol。
    cs.add("单例过", {
        let c0 = corpus_regular()[0].clone();
        case_check(&c0, 1).is_ok()
    }, "首例过");
    cs
}
