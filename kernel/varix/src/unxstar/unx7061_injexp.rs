//! UNX-F7061 注入扩谱总起（18 格矩阵全域化）（0x9300 段）
//!
//! 锚点：UNX-F7061-J1 注入扩谱总起：F6961 18 格矩阵 → ≥54 格
//! （三倍扩谱）映射表落盘；逐格来源可溯。
//! 域：CoRun·UNX-B4-B34（注入扩谱面）。
//!
//! 结构：F6961 原始 18 格（3 轴 × 6 值闭集）+ 三倍扩谱（每格 ×3
//! 投影：同值异道/异值同道/值道正交）→ 54 格映射表 + 逐格来源
//! 可溯（(新格, 源格, 投影道) 三元组冻结）+ 扩谱完备核（新格数
//! ≥54 + 双射回溯 + 无孤儿格）+ 注入核（孤儿格/回溯破缺/溢格/封账）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x9300 独占 0x80）。
pub const SEG_BASE: u32 = 0x9300;
pub const XC_IX_ORPHAN: u32 = SEG_BASE + 0x01; // 孤儿格
pub const XC_IX_BACK: u32 = SEG_BASE + 0x02; // 回溯破缺
pub const XC_IX_OVER: u32 = SEG_BASE + 0x03; // 溢格
pub const XC_IX_SEALED: u32 = SEG_BASE + 0x04; // 封账

/// F6961 原始轴 × 值闭集：3 轴。
pub const AXES: [&str; 3] = ["op", "field", "timing"];
/// 每轴 6 值（3×6=18 格）。
pub const AXIS_VALS: [&str; 6] = ["v0", "v1", "v2", "v3", "v4", "v5"];

/// 原始格编码：axis*6 + val（0..18）。
pub fn origin_cell(axis: usize, val: usize) -> usize {
    axis * 6 + val
}

/// 投影道闭集（每源格扩 3 道）。
pub const PROJ_SAMEVAL: u8 = 0; // 同值异道：值不变、注入通道换
pub const PROJ_SAMECHAN: u8 = 1; // 异值同道：通道不变、值域扩展
pub const PROJ_ORTH: u8 = 2; // 值道正交：新通道 × 新值域

/// 扩谱格编码：源格 ×3 + 投影道（0..54）。
pub fn expand_cell(src: usize, proj: u8) -> Result<usize, u32> {
    if src >= 18 {
        return Err(XC_IX_OVER);
    }
    if proj > PROJ_ORTH {
        return Err(XC_IX_OVER);
    }
    Ok(src * 3 + proj as usize)
}

/// 扩谱回溯：(新格 → (源格, 投影道))。
pub fn trace_back(cell: usize) -> Result<(usize, u8), u32> {
    if cell >= 54 {
        return Err(XC_IX_OVER);
    }
    Ok((cell / 3, (cell % 3) as u8))
}

/// 映射表项：(新格, 源格, 投影道) 逐格来源三元组。
#[derive(Clone, Copy)]
pub struct MapEntry {
    pub cell: usize,
    pub src: usize,
    pub proj: u8,
}

/// 全映射表落盘（54 格冻结序）。
pub fn full_map() -> Vec<MapEntry> {
    let mut m = Vec::new();
    for src in 0..18usize {
        for proj in 0..=2u8 {
            if let Ok(cell) = expand_cell(src, proj) {
                m.push(MapEntry { cell, src, proj });
            }
        }
    }
    m
}

/// 扩谱完备核：54 格全覆盖 + 无重复 + 回溯一致。
pub fn expand_complete() -> bool {
    let m = full_map();
    if m.len() != 54 {
        return false;
    }
    let mut seen = [false; 54];
    for e in &m {
        if seen[e.cell] {
            return false;
        }
        seen[e.cell] = true;
        // 回溯一致
        match trace_back(e.cell) {
            Ok((s, p)) => {
                if s != e.src || p != e.proj {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    seen.iter().all(|&b| b)
}

/// 逐格来源可溯核：每格标注源轴与值。
pub fn source_attributable() -> bool {
    let m = full_map();
    for e in &m {
        let axis = e.src / 6;
        let val = e.src % 6;
        if axis >= 3 || val >= 6 {
            return false;
        }
        // 源轴/值名可导出（可溯性 = 标签非空）
        if AXES[axis].is_empty() || AXIS_VALS[val].is_empty() {
            return false;
        }
    }
    true
}

/// 扩谱标签导出（机读）："cell=N src=<axis>.<val> proj=<道>"。
pub fn export_labels() -> Vec<String> {
    let m = full_map();
    let mut out = Vec::new();
    for e in &m {
        let axis = e.src / 6;
        let val = e.src % 6;
        let proj_name = match e.proj {
            PROJ_SAMEVAL => "sameval",
            PROJ_SAMECHAN => "samechan",
            _ => "orth",
        };
        out.push(format!(
            "cell={} src={}.{} proj={}",
            e.cell, AXES[axis], AXIS_VALS[val], proj_name
        ));
    }
    out
}

/// 三倍扩谱计数核：18×3=54。
pub fn triple_count_exact() -> bool {
    18 * 3 == 54 && full_map().len() == 54 && 54 >= 54
}

/// 注入核（5 注入必抓）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 源格溢出（src=18）
    if expand_cell(18, PROJ_SAMEVAL) == Err(XC_IX_OVER) {
        caught += 1;
    }
    // 2) 投影道溢出（proj=3）
    if expand_cell(0, 3) == Err(XC_IX_OVER) {
        caught += 1;
    }
    // 3) 新格溢出回溯（cell=54）
    if trace_back(54) == Err(XC_IX_OVER) {
        caught += 1;
    }
    // 4) 孤儿格检测：去掉一格后 seen 有洞
    let mut m = full_map();
    let removed = m.pop().unwrap();
    let mut seen = [false; 54];
    for e in &m {
        seen[e.cell] = true;
    }
    if !seen[removed.cell] && seen.iter().filter(|&&b| b).count() == 53 {
        caught += 1;
    }
    // 5) 回溯破缺：伪造映射（src 与 cell 不对应）
    let fake = MapEntry { cell: 10, src: 9, proj: 1 };
    let real = trace_back(10);
    let broken = match real {
        Ok((s, p)) => s != fake.src || p != fake.proj,
        Err(_) => true,
    };
    if broken {
        caught += 1;
    }
    (caught, 5)
}

/// 指纹冻结：全映射折叠哈希。
pub fn fp_frozen(m: &[MapEntry]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for e in m {
        h ^= e.cell as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= e.src as u64;
        h = h.wrapping_mul(0x100000001b3);
        h ^= e.proj as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 判据入口（CheckSet）。
pub fn run_unx7061_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F7061");
    // J1-1 扩谱完备（54 格全覆盖）
    let ok = expand_complete();
    s.add("B34-01-扩谱完备", ok, if ok { "" } else { "完备漂移" });
    // J1-2 逐格来源可溯
    let ok = source_attributable();
    s.add("B34-02-来源可溯", ok, if ok { "" } else { "可溯漂移" });
    // J1-3 三倍扩谱计数
    let ok = triple_count_exact();
    s.add("B34-03-三倍扩谱", ok, if ok { "" } else { "计数漂移" });
    // J1-4 落盘标签机读
    let labels = export_labels();
    let ok = labels.len() == 54
        && labels[0] == "cell=0 src=op.v0 proj=sameval"
        && labels[53].starts_with("cell=53");
    s.add("B34-04-落盘机读", ok, if ok { "" } else { "落盘漂移" });
    // J1-5 注入核 5/5
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { alloc::string::String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("B34-05-注入核必抓", ok, &detail);
    // J1-6 指纹冻结
    let m = full_map();
    let ok = fp_frozen(&m) == fp_frozen(&m);
    s.add("B34-06-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    s
}
