//! VE-F0775 九宫格 fuzz
//! 职责：随机内边距/尺寸 fuzz、三不变量（覆盖恒等/角区固定/非负区尺寸）、
//!       随机气泡锚点不变量（避让）、种子复现。
//! 契约：任意轮不 panic；不变量违反可复现。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec335_cfuzz::Rng;
use super::vec361_ninecore::{Cell, Insets, Slice, layout};

/// 随机切片
pub fn gen_slice(rng: &mut Rng) -> Slice {
    let tex = 32.0 + ((rng.next() % 8) * 8) as f64;
    let l = 2.0 + ((rng.next() % 6) * 2) as f64;
    let t = 2.0 + ((rng.next() % 6) * 2) as f64;
    let r = 2.0 + ((rng.next() % 6) * 2) as f64;
    let b = 2.0 + ((rng.next() % 6) * 2) as f64;
    let ins = Insets { left: l, top: t, right: r, bottom: b };
    // 保证 inset 和 < tex
    let tex_w = tex.max(ins.sum_h() + 4.0);
    let tex_h = tex.max(ins.sum_v() + 4.0);
    Slice::build(tex_w, tex_h, ins).expect("gen slice")
}

/// 随机绘制尺寸（最小值以上）
pub fn gen_target(rng: &mut Rng, s: &Slice) -> (f64, f64) {
    let min_w = s.inset.sum_h();
    let min_h = s.inset.sum_v();
    let w = min_w + (rng.next() % 64) as f64;
    let h = min_h + (rng.next() % 64) as f64;
    (w, h)
}

/// 三不变量
pub fn invariants(s: &Slice, cells: &[Cell; 9], w: f64, h: f64) -> bool {
    // 1) 覆盖恒等
    let area: f64 = cells.iter().map(|c| c.w * c.h).sum();
    if (area - w * h).abs() > 1e-9 {
        return false;
    }
    // 2) 角区固定
    let (l, t, r, b) = (s.inset.left, s.inset.top, s.inset.right, s.inset.bottom);
    if cells[0].w != l || cells[0].h != t || cells[2].w != r || cells[2].h != t
        || cells[6].w != l || cells[6].h != b || cells[8].w != r || cells[8].h != b
    {
        return false;
    }
    // 3) 非负
    cells.iter().all(|c| c.w >= 0.0 && c.h >= 0.0)
}

/// fuzz 主循环
pub fn fuzz_run(seed: u64, rounds: usize) -> usize {
    let mut rng = Rng::new(seed);
    let mut violations = 0;
    for _ in 0..rounds {
        let s = gen_slice(&mut rng);
        let (w, h) = gen_target(&mut rng, &s);
        if let Some(cells) = layout(&s, 0.0, 0.0, w, h) {
            if !invariants(&s, &cells, w, h) {
                violations += 1;
            }
        }
    }
    violations
}

/// 随机气泡锚点 + 避让检查
pub fn fuzz_bubble(seed: u64, rounds: usize) -> usize {
    let mut rng = Rng::new(seed);
    let mut fails = 0;
    for _ in 0..rounds {
        let w = 100.0 + (rng.next() % 200) as f64;
        let h = 60.0 + (rng.next() % 120) as f64;
        let bub = super::vec372_ninebubble::Bubble::build(
            Insets::uniform(16.0),
            super::vec372_ninebubble::ArrowDir::Down,
            rng.unit(),
            8.0,
            rng.unit() * 4.0,
        );
        if let Some(b) = bub {
            // 若锚点在边区，必须避让角区
            if b.anchor_in_edge(w, h) && !b.clears_corners(w, h) {
                fails += 1;
            }
        }
    }
    fails
}

/// 种子复现
pub fn reproducible(seed: u64, rounds: usize) -> bool {
    fuzz_run(seed, rounds) == fuzz_run(seed, rounds)
}

pub fn run_vec375_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec375");
    // 族一：随机切片生成
    let mut rg = Rng::new(42);
    let sl = gen_slice(&mut rg);
    s.add("生成-切片合法", sl.tex_w > 0.0 && sl.inset.sum_h() < sl.tex_w, "valid slice");
    let sl2 = gen_slice(&mut Rng::new(42));
    s.add("生成-同种同形", sl.tex_w == sl2.tex_w && sl.inset == sl2.inset, "reproducible gen");
    // 族二：随机目标
    let (w, h) = gen_target(&mut rg, &sl);
    s.add("生成-目标合法", w >= sl.inset.sum_h() && h >= sl.inset.sum_v(), "≥ min");
    // 族三：不变量在样本上成立
    let cells = layout(&sl, 0.0, 0.0, w, h).expect("cells");
    s.add("不变量-样本", invariants(&sl, &cells, w, h), "sample clean");
    // 手工构造违反（篡改角区）
    let mut bad = cells;
    bad[0].w += 1.0;
    s.add("不变量-篡改检出", !invariants(&sl, &bad, w, h), "tampered corner");
    // 篡改面积
    let mut bad2 = cells;
    bad2[4].w = 0.0;
    s.add("不变量-面积检出", !invariants(&sl, &bad2, w, h), "area broken");
    // 族四：fuzz 主循环
    let v = fuzz_run(2024, 64);
    s.add("fuzz-64轮", v == 0, "64 rounds clean");
    let v2 = fuzz_run(777_777, 128);
    s.add("fuzz-128轮", v2 == 0, "128 rounds clean");
    // 族五：气泡避让 fuzz
    let bf = fuzz_bubble(999, 64);
    s.add("气泡-避让不违反", bf == 0, "bubble invariants");
    // 族六：种子复现
    s.add("复现-同种", reproducible(31415, 32), "reproducible");
    // 族七：负样本注入（保证判据不是恒绿）
    let mut negative_detected = false;
    {
        let s_neg = gen_slice(&mut Rng::new(7));
        let (wn, hn) = gen_target(&mut Rng::new(7), &s_neg);
        if let Some(mut cells_n) = layout(&s_neg, 0.0, 0.0, wn, hn) {
            cells_n[8].h += 0.5; // 破坏面积
            negative_detected = !invariants(&s_neg, &cells_n, wn, hn);
        }
    }
    s.add("判据-非恒绿", negative_detected, "negative sample caught");
    s
}
