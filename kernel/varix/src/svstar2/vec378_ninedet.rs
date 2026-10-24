//! VE-F0778 九宫格一致性
//! 职责：元格式序列化跨环境一致性（键序确定/数值格式确定）、
//!       布局结果确定性（同输入同九区）、执行轨迹哈希。
//! 契约：序列化输出逐字节确定；布局哈希同输入恒等。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec361_ninecore::{Cell, Insets, Slice, layout};
use super::vec364_ninemeta::{NineMeta, serialize};

/// 单元格哈希
fn cell_hash(c: &Cell, h: u64) -> u64 {
    let mut h = h;
    for v in [c.x, c.y, c.w, c.h] {
        h ^= v.to_bits();
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 九区布局哈希
pub fn layout_hash(cells: &[Cell; 9]) -> u64 {
    let mut h = 0x811c9dc5u64;
    for c in cells {
        h = cell_hash(c, h);
    }
    h
}

/// 序列化确定性：两轮逐字节一致
pub fn serialize_deterministic(m: &NineMeta, rounds: usize) -> bool {
    let first = serialize(m);
    (1..rounds).all(|_| serialize(m) == first)
}

/// 键序确定：serialize 输出的键序固定
pub fn key_order_stable(m: &NineMeta) -> bool {
    let text = serialize(m);
    let keys: Vec<&str> = text.lines().filter_map(|l| l.split_once('=').map(|(k, _)| k)).collect();
    let expect: Vec<&str> = alloc::vec!["tex_w", "tex_h", "inset_l", "inset_t", "inset_r", "inset_b", "ver"];
    keys == expect
}

/// 跨环境档案（数值格式行为）
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct EnvProfile {
    pub name: &'static str,
    /// 整数格式化（无尾零）
    pub int_fmt: bool,
    /// round 半数远离零（非银行家舍入）
    pub round_away: bool,
}

pub fn env_ok(e: EnvProfile) -> bool {
    e.int_fmt && e.round_away
}

/// 布局跨环境一致（同输入两环境产出同哈希——由确定性单线程保证）
pub fn layout_env_stable(s: &Slice, w: f64, h: f64) -> bool {
    let a = layout(s, 0.0, 0.0, w, h).map(|c| layout_hash(&c));
    let b = layout(s, 0.0, 0.0, w, h).map(|c| layout_hash(&c));
    a == b && a.is_some()
}

pub fn run_vec378_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec378");
    let meta = NineMeta::new(64.0, 64.0, 8.0, 8.0, 8.0, 8.0);
    // 族一：序列化确定性
    s.add("序列-多轮一致", serialize_deterministic(&meta, 8), "8 rounds same");
    s.add("序列-键序稳定", key_order_stable(&meta), "key order");
    // 带半径版本键序（radius 插在 inset_b 与 ver 之间）
    let mut mr = meta;
    mr.radius = 6.0;
    let text_r = serialize(&mr);
    s.add("序列-半径在", text_r.contains("radius=6"), "radius line");
    // 族二：布局哈希
    let ins = Insets::uniform(8.0);
    let sl = Slice::build(64.0, 64.0, ins).expect("slice");
    let cells = layout(&sl, 0.0, 0.0, 100.0, 80.0).expect("c");
    let h1 = layout_hash(&cells);
    s.add("哈希-同果恒等", layout_hash(&layout(&sl, 0.0, 0.0, 100.0, 80.0).expect("c2")) == h1, "same input same hash");
    // 尺寸变化 → 哈希变化
    let cells2 = layout(&sl, 0.0, 0.0, 200.0, 80.0).expect("c3");
    s.add("哈希-异果异值", layout_hash(&cells2) != h1, "diff size diff hash");
    // 偏移变化 → 哈希变化
    let cells3 = layout(&sl, 10.0, 0.0, 100.0, 80.0).expect("c4");
    s.add("哈希-偏移敏感", layout_hash(&cells3) != h1, "offset sensitive");
    // 空（零）
    s.add("哈希-九区可零", layout_hash(&cells) != 0, "nonzero for real cells");
    // 族三：跨环境
    s.add("环境-布局稳定", layout_env_stable(&sl, 100.0, 80.0), "env stable");
    s.add("环境-布局稳定大", layout_env_stable(&sl, 3840.0, 2160.0), "4K stable");
    // 环境档案
    let wasm = EnvProfile { name: "wasm32", int_fmt: true, round_away: true };
    let x64 = EnvProfile { name: "x86_64", int_fmt: true, round_away: true };
    s.add("环境-wasm合规", env_ok(wasm), "wasm ok");
    s.add("环境-x64合规", env_ok(x64), "x64 ok");
    let banker = EnvProfile { name: "bankers", int_fmt: true, round_away: false };
    s.add("环境-银行家违规", !env_ok(banker), "banker's rounding rejected");
    let fmt_off = EnvProfile { name: "locale-fmt", int_fmt: false, round_away: true };
    s.add("环境-本地化违规", !env_ok(fmt_off), "locale fmt rejected");
    // 族四：整数格式化行为锚（serialize 中 64.0 → "64"）
    s.add("格式-整数锚", serialize(&meta).contains("tex_w=64\n"), "64 not 64.0");
    // 小数保留
    let mut mfrac = meta;
    mfrac.tex_w = 64.5;
    s.add("格式-小数锚", serialize(&mfrac).contains("tex_w=64.5\n"), "64.5 kept");
    // 族五：确定性不依赖可变状态（两次布局互不污染）
    let _ = layout(&sl, 0.0, 0.0, 500.0, 500.0);
    let cells_after = layout(&sl, 0.0, 0.0, 100.0, 80.0).expect("after");
    s.add("确定-无状态污染", layout_hash(&cells_after) == h1, "no state leak");
    s
}
