//! CGPU-F2083 域自检（CGPU-N 域重建原则判据层）
//!
//! 判据侧**独立写死**期望（三原则次序、两级口径 980/2 与 950/5、0x9C2x 码段
//! 与 vcn01/vcn02 十码合并互异）。聚合防自调：族内判据只调另一族 standalone
//! + 进行中 set 自身 tally，守恒断言归 CI 探针层。

use crate::checks::CheckSet;

use super::vcn03_principles::*;

use alloc::string::String;

// ---------------------------------------------------------------------------
// 判据侧独立写死的期望
// ---------------------------------------------------------------------------

const EXPECT_VERSION: &str = "CN03-principles-v1";
const EXPECT_PIXEL_SSIM: u32 = 980;
const EXPECT_PIXEL_LAT: u32 = 2;
const EXPECT_PERC_SSIM: u32 = 950;
const EXPECT_PERC_LAT: u32 = 5;

/// 单遍词法剥除（F2807 教训：字符串内 `//` 不得被行注释剥离误伤）。
fn strip_lexical_noise(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
            let mut depth = 1usize;
            i += 2;
            while i < b.len() && depth > 0 {
                if i + 1 < b.len() && b[i] == b'/' && b[i + 1] == b'*' {
                    depth += 1;
                    i += 2;
                } else if i + 1 < b.len() && b[i] == b'*' && b[i + 1] == b'/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            continue;
        }
        if b[i] == b'"' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'"';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if b[i] == b'\'' {
            i += 1;
            while i < b.len() {
                if b[i] == b'\\' {
                    i += 2;
                    continue;
                }
                let closed = b[i] == b'\'';
                i += 1;
                if closed {
                    break;
                }
            }
            continue;
        }
        if let Some(c) = src.get(i..i + 1) {
            out.push_str(c);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// 一、规格（三原则 / 分级验收 / 诚实闸）
// ---------------------------------------------------------------------------

fn chk_spec_principles(set: &mut CheckSet) {
    // 规格-01：三原则封闭 + 次序秩（首名「等价优先」判据侧写死）。
    let ok = PRINCIPLES.len() == 3
        && PRINCIPLES[0] == "等价优先"
        && PRINCIPLES[1] == "性能保底"
        && PRINCIPLES[2] == "降级诚实"
        && PRINCIPLE_DOCS.len() == 3;
    if ok {
        set.ok("EN083-规格-01-三原则封闭次序");
    } else {
        set.fail("EN083-规格-01-三原则封闭次序", "原则漂移或次序失守");
    }
    // 规格-02：分级枚举往返 + 越界 None。
    let mut rt = true;
    let mut i = 0usize;
    while i < 2 {
        match EquivalenceTier::of_ordinal(i) {
            Some(t) => {
                if t.ordinal() != i {
                    rt = false;
                }
            }
            None => rt = false,
        }
        i += 1;
    }
    let oor = EquivalenceTier::of_ordinal(2).is_none();
    if rt && oor && EquivalenceTier::ALL.len() == 2 {
        set.ok("EN083-规格-02-分级封闭往返");
    } else {
        set.fail("EN083-规格-02-分级封闭往返", "分级漂移或越界未拒");
    }
    // 规格-03：分级口径秩序（等价优先的可测面——像素级 SSIM 更高延迟更紧，
    // 判据侧独立写死 980/950 与 2/5 双向断言）。
    let ok = tiers_ordered()
        && PIXEL_SSIM_FLOOR == EXPECT_PIXEL_SSIM
        && PIXEL_LATENCY_CAP_MS == EXPECT_PIXEL_LAT
        && PERCEPTUAL_SSIM_FLOOR == EXPECT_PERC_SSIM
        && PERCEPTUAL_LATENCY_CAP_MS == EXPECT_PERC_LAT
        && SSIM_SCALE == 1000;
    if ok {
        set.ok("EN083-规格-03-分级口径秩序");
    } else {
        set.fail("EN083-规格-03-分级口径秩序", "阈值漂移或口径失序");
    }
}

fn chk_spec_verify(set: &mut CheckSet) {
    // 规格-04：像素级恰边界——SSIM 979 拒/980 过、延迟 2 过/3 拒（含等两侧）。
    let ssim_edge = verify(EquivalenceTier::PixelExact, 979, 2);
    let pass = verify(EquivalenceTier::PixelExact, 980, 2);
    let lat_edge = verify(EquivalenceTier::PixelExact, 980, 3);
    let ok = ssim_edge == Err(E_N083_PIXEL_SSIM)
        && pass == Ok(Acceptance::PixelVerified)
        && lat_edge == Err(E_N083_PIXEL_LATENCY);
    if ok {
        set.ok("EN083-规格-04-像素级恰边界");
    } else {
        set.fail("EN083-规格-04-像素级恰边界", "像素级闸未两侧钉死");
    }
    // 规格-05：感知级恰边界 + 分级分离可测（949/6 双低：感知拒而
    // 950/5 恰过；970/4 语料像素级拒而感知级过——两口径可分辨非恒等）。
    let perc_edge = verify(EquivalenceTier::Perceptual, 949, 5);
    let perc_pass = verify(EquivalenceTier::Perceptual, 950, 5);
    let perc_lat = verify(EquivalenceTier::Perceptual, 950, 6);
    let split = verify(EquivalenceTier::PixelExact, 970, 4);
    let split_perc = verify(EquivalenceTier::Perceptual, 970, 4);
    let ok = perc_edge == Err(E_N083_PERCEPTUAL_SHORT)
        && perc_pass == Ok(Acceptance::PerceptualVerified)
        && perc_lat == Err(E_N083_PERCEPTUAL_SHORT)
        && split == Err(E_N083_PIXEL_SSIM)
        && split_perc == Ok(Acceptance::PerceptualVerified);
    if ok {
        set.ok("EN083-规格-05-感知级恰边界分级分离");
    } else {
        set.fail("EN083-规格-05-感知级恰边界分级分离", "感知级闸失守或两口径恒等");
    }
    // 规格-06：降级诚实闸——声明与验收同级才过；拿感知级冒充像素级拒。
    let honest = claim_honest(EquivalenceTier::PixelExact, Acceptance::PixelVerified);
    let honest_p = claim_honest(EquivalenceTier::Perceptual, Acceptance::PerceptualVerified);
    let liar = claim_honest(EquivalenceTier::PixelExact, Acceptance::PerceptualVerified);
    let ok = honest.is_ok() && honest_p.is_ok() && liar == Err(E_N083_CLAIM_MISMATCH);
    if ok {
        set.ok("EN083-规格-06-降级诚实声明同级");
    } else {
        set.fail("EN083-规格-06-降级诚实声明同级", "冒充像素级未被拒");
    }
}

// ---------------------------------------------------------------------------
// 二、判据承载力（零 panic / 码段独立复核 / 防自调）
// ---------------------------------------------------------------------------

fn chk_criterion_zero_panic(set: &mut CheckSet) {
    // 判据-01：判据面零 panic（自扫本文件）。
    let src = include_str!("vcn03_principles_checks.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EN083-判据-01-判据面零 panic");
    } else {
        set.fail("EN083-判据-01-判据面零 panic", "判据面含 panic 面");
    }
    // 规格-07：生产面零 panic（扫实现文件）。
    let src = include_str!("vcn03_principles.rs");
    let clean = strip_lexical_noise(src);
    let mut hits = 0usize;
    for pat in [".unwrap()", ".expect(", "panic!", "unwrap_or_else(||"].iter() {
        if clean.contains(pat) {
            hits += 1;
        }
    }
    if hits == 0 {
        set.ok("EN083-规格-07-生产面零 panic");
    } else {
        set.fail("EN083-规格-07-生产面零 panic", "生产面含 panic 面");
    }
}

fn chk_criterion_codes_independent(set: &mut CheckSet) {
    // 判据-02：码段独占独立复核——本域四码皆 0x9C2x；与 vcn01（0x9C0x）/
    // vcn02（0x9C1x）十码合并互异（同段跨文件防撞码）。
    let got = [E_N083_PIXEL_SSIM, E_N083_PIXEL_LATENCY, E_N083_PERCEPTUAL_SHORT, E_N083_CLAIM_MISMATCH];
    let prior = [0x9C00u16, 0x9C01, 0x9C02, 0x9C03, 0x9C04, 0x9C05, 0x9C10, 0x9C11, 0x9C12];
    let mut ok = true;
    let mut i = 0usize;
    while i < got.len() {
        if got[i] & 0xFF00 != 0x9C00 || got[i] & 0x00F0 != 0x0020 {
            ok = false;
        }
        let mut j = 0usize;
        while j < prior.len() {
            if got[i] == prior[j] {
                ok = false;
            }
            j += 1;
        }
        let mut k = i + 1;
        while k < got.len() {
            if got[i] == got[k] {
                ok = false;
            }
            k += 1;
        }
        i += 1;
    }
    if ok {
        set.ok("EN083-判据-02-码段独占独立复核");
    } else {
        set.fail("EN083-判据-02-码段独占独立复核", "码漂移或与前批撞码");
    }
}

fn chk_criterion_not_truncated(set: &mut CheckSet) {
    // 判据-03：聚合防自调——只调不递归的 A 族 + 自身进行中 tally；
    // 条数期望判据侧写死（A 族 6 条；判据-03 登记前 B 族进行中 3 条）。
    let a = run_vcn03_checks_a_standalone();
    let (ap, af) = a.tally();
    let (sp, sf) = set.tally();
    let a_ok = ap + af == 6;
    let self_ok = sp + sf == 3;
    let no_trunc =
        !a.truncated() && !set.truncated() && a.dropped() == 0 && set.dropped() == 0;
    if a_ok && self_ok && no_trunc {
        set.ok("EN083-判据-03-聚合守恒防自调");
    } else {
        set.fail("EN083-判据-03-聚合守恒防自调", "族条数漂移或有截断/丢弃");
    }
}

// ---------------------------------------------------------------------------
// 入口（a=规格 / b=判据承载力；合并入口供聚合器）
// ---------------------------------------------------------------------------

/// 判据族 a：规格。
pub fn run_vcn03_checks_a_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcn03/a");
    chk_spec_principles(&mut s);
    chk_spec_verify(&mut s);
    s
}

/// 判据族 b：判据承载力。
pub fn run_vcn03_checks_b_standalone() -> CheckSet {
    let mut s = CheckSet::new("svstar2/vcn03/b");
    chk_criterion_zero_panic(&mut s);
    chk_criterion_codes_independent(&mut s);
    chk_criterion_not_truncated(&mut s);
    s
}

/// 全域判据入口（聚合器调用这个）。
pub fn run_vcn03_checks() -> CheckSet {
    CheckSet::merge(run_vcn03_checks_a_standalone(), run_vcn03_checks_b_standalone())
}
