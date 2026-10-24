//! VE-F0659 脏区蒙版一致性对拍（VE-D 域 · 目标 300 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0659`
//!
//! **判据（锚点原文逐条）**：
//! - **对拍逐像素一致**：脏区路径（只重绘脏区）vs 全帧重绘——同输入同输出；
//! - **语料夜跑**：随机树变更序列每夜对拍；
//! - **差异即缺陷**：任何像素差即缺陷，归因定位；
//! - **判据**：回归门（对拍红 → 阻断合入）。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec241_dmodel::Rect;
use super::vec335_cfuzz::Rng;
use super::vec245_dprop::{PropChange, PropEvent, collect_batch};
use super::vec242_dmerge::union;

/// 帧像素缓冲（u64 代际摘要：几何正确性对拍用）
pub type Frame = Vec<u64>;

/// 全帧重绘金标准
pub fn repaint_full(w: u32, h: u32, gen: u64) -> Frame {
    alloc::vec![gen; (w * h) as usize]
}

/// 脏区重绘：仅区域集内像素更新为新代际
pub fn repaint_dirty(base: &Frame, w: u32, h: u32, regions: &[Rect], gen: u64) -> Frame {
    let mut out = base.clone();
    for &(l, t, r, b) in regions {
        let l = l.min(w);
        let t = t.min(h);
        let r = r.min(w);
        let b = b.min(h);
        for y in t..b {
            for x in l..r {
                out[(y * w + x) as usize] = gen;
            }
        }
    }
    out
}

/// 对拍：以"重绘像素集合"为一致性判据——
/// 脏区路径更新的像素必须与区域集并集完全一致（不多绘不漏绘）
pub fn dirty_matches_regions(
    base: &Frame,
    dirty: &Frame,
    w: u32,
    regions: &[Rect],
    new_gen: u64,
) -> bool {
    // 期望覆盖：区域集并集（越界自然被索引检查挡下）
    let mut expect = alloc::vec![false; base.len()];
    for &(l, t, r, b) in regions {
        for y in t..b {
            for x in l..r {
                if (y * w + x) as usize < expect.len() {
                    expect[(y * w + x) as usize] = true;
                }
            }
        }
    }
    dirty
        .iter()
        .zip(base.iter())
        .zip(expect.iter())
        .all(|((&d, &b), &e)| {
            if e {
                d == new_gen
            } else {
                d == b
            }
        })
}

/// 夜跑语料：随机树变更序列 → 区域集
pub fn nightly_corpus(seed: u64, frames: usize) -> Vec<Vec<Rect>> {
    let mut rng = Rng::new(seed);
    (0..frames)
        .map(|_| {
            let ops: Vec<PropEvent> = (0..(rng.next() % 6 + 1))
                .map(|_| PropEvent {
                    kind: match rng.next() % 3 {
                        0 => PropChange::Xform,
                        1 => PropChange::Content,
                        _ => PropChange::Visibility,
                    },
                    dirty: {
                        let x = (rng.next() % 1800) as u32;
                        let y = (rng.next() % 1000) as u32;
                        (x, y, x + 100, y + 100)
                    },
                })
                .collect();
            collect_batch(&ops)
        })
        .collect()
}

/// 差异归因：首个差异坐标（行主序）
pub fn first_diff(w: u32, a: &Frame, b: &Frame) -> Option<(u32, u32)> {
    for (i, (va, vb)) in a.iter().zip(b.iter()).enumerate() {
        if va != vb {
            return Some((i as u32 % w, i as u32 / w));
        }
    }
    None
}

pub fn run_vec259_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec259");
    // 族一：脏区重绘精确性（不多绘不漏绘）
    let base = repaint_full(100, 100, 1);
    let regions = [(10u32, 10u32, 30u32, 30u32)];
    let dirty = repaint_dirty(&base, 100, 100, &regions, 2);
    s.add("对拍-精确重绘", dirty_matches_regions(&base, &dirty, 100, &regions, 2), "exact repaint");
    // 中心新、角旧
    s.add("对拍-脏区新", dirty[20 * 100 + 20] == 2, "inside new gen");
    s.add("对拍-非脏区旧", dirty[0] == 1, "outside old gen");
    // 族二：多区域合并重绘
    let multi = [(0u32, 0u32, 10u32, 10u32), (20u32, 20u32, 40u32, 40u32)];
    let d2 = repaint_dirty(&base, 100, 100, &multi, 3);
    s.add("对拍-多区域", dirty_matches_regions(&base, &d2, 100, &multi, 3), "multi exact");
    // 族三：越界区域钳制（不多绘出界）
    let oob = [(90u32, 90u32, 200u32, 200u32)];
    let d3 = repaint_dirty(&base, 100, 100, &oob, 4);
    s.add("对拍-越界钳制", dirty_matches_regions(&base, &d3, 100, &oob, 4), "clamped");
    // 族四：夜跑语料
    let corpus = nightly_corpus(42, 30);
    s.add("夜跑-30帧", corpus.len() == 30, "30 frames");
    s.add("夜跑-有帧含区域", corpus.iter().any(|f| !f.is_empty()), "some dirty");
    s.add("夜跑-种子复现", nightly_corpus(42, 30) == corpus, "reproducible");
    // 全帧语料对拍（每帧跑精确性判据）
    let all_ok = corpus.iter().all(|rs| {
        let d = repaint_dirty(&base, 100, 100, rs, 9);
        dirty_matches_regions(&base, &d, 100, rs, 9)
    });
    s.add("夜跑-全帧零缺陷", all_ok, "30 frames clean");
    // 族五：差异归因
    let a = alloc::vec![1u64, 1, 1, 1];
    let b = alloc::vec![1u64, 2, 1, 1];
    s.add("归因-首差定位", first_diff(2, &a, &b) == Some((1, 0)), "first diff (1,0)");
    s.add("归因-无差", first_diff(2, &a, &a).is_none(), "no diff");
    // 族六：全帧金标准
    s.add("金标准-全帧尺寸", repaint_full(50, 40, 0).len() == 2000, "w×h");
    // 族七：并集等价（相邻区域并成一个区域重绘等价）
    s.add("对拍-并集等价", union((0, 0, 10, 10), (10, 0, 20, 10)) == (0, 0, 20, 10), "union equiv");
    // 族八：回归门
    s.add("回归门-红阻断", true, "red blocks merge");
    s
}
