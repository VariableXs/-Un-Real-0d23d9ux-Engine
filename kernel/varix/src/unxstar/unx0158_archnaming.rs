//! UNX-F0158 · 年度镜像命名与归档规范 · 诊断段 0x2690
//!
//! 判据原文（UNX-F0158-J1）：镜像名闭集语法 `varix-YYYY.M.P[-tag].img`
//! （年 4 位、版本 0~999、标签闭集 stable|beta|dbg），非法名全拒；
//! 归档账按年分组守恒（Σ年组 = Σ镜像名）；重名归档即红。
//!
//! 设计要点：命名解析器（闭集语法）+ 归档册（年 → 名集）+
//! 分组守恒。零浮点、零 unwrap/expect。

use alloc::vec;
use crate::alloc::{format, string::String, vec::Vec};

pub const SEG_BASE: u16 = 0x26A0;

const E_FMT: u16 = SEG_BASE + 1; // 名语法非法
const E_YEAR: u16 = SEG_BASE + 2; // 年份非法（非 4 位或超 2999）
const E_TAG: u16 = SEG_BASE + 3; // 标签越纲
const E_VER: u16 = SEG_BASE + 4; // 版本字段越纲
const E_DUP: u16 = SEG_BASE + 5; // 归档重名
const E_BAL: u16 = SEG_BASE + 6; // 分组守恒被破坏

/// 标签闭集。
pub const TAGS: [&str; 3] = ["stable", "beta", "dbg"];

/// 归档镜像名分量。
#[derive(Clone, PartialEq, Eq)]
pub struct ImgName {
    pub year: u16,
    pub major: u16,
    pub minor: u16,
    pub tag: Option<&'static str>,
}

fn parse_field(s: &str) -> Result<u16, u16> {
    if s.is_empty() || s.len() > 3 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(E_VER);
    }
    let v: u32 = s.bytes().fold(0, |acc, b| acc * 10 + (b - b'0') as u32);
    if v > 999 {
        return Err(E_VER);
    }
    Ok(v as u16)
}

/// 命名解析：`varix-YYYY.M.P[-tag].img`。
pub fn parse_name(s: &str) -> Result<ImgName, u16> {
    let body = s.strip_prefix("varix-").ok_or(E_FMT)?;
    let body = body.strip_suffix(".img").ok_or(E_FMT)?;
    let (ymv, tag) = match body.split_once('-') {
        Some((a, t)) => (a, Some(t)),
        None => (body, None),
    };
    // 年段 4 位。
    let (ys, rest) = ymv.split_once('.').ok_or(E_FMT)?;
    if ys.len() != 4 || !ys.bytes().all(|b| b.is_ascii_digit()) {
        return Err(E_YEAR);
    }
    let year: u32 = ys.bytes().fold(0, |acc, b| acc * 10 + (b - b'0') as u32);
    if year > 2999 {
        return Err(E_YEAR);
    }
    // 版本段 M.P。
    let (ms, ps) = rest.split_once('.').ok_or(E_FMT)?;
    let major = parse_field(ms)?;
    let minor = parse_field(ps)?;
    // 标签闭集。
    let tag = match tag {
        None => None,
        Some(t) => {
            let found = TAGS.iter().find(|x| **x == t);
            match found {
                Some(x) => Some(*x),
                None => return Err(E_TAG),
            }
        }
    };
    Ok(ImgName {
        year: year as u16,
        major,
        minor,
        tag,
    })
}

/// 归档册。
pub struct ArchiveBook {
    pub names: Vec<ImgName>,
    pub sealed: bool,
}

impl ArchiveBook {
    pub fn new() -> Result<Self, u16> {
        Ok(ArchiveBook { names: Vec::new(), sealed: false })
    }

    /// 归档一个镜像名（解析 + 重名拒）。
    pub fn archive(&mut self, s: &str) -> Result<(), u16> {
        if self.sealed {
            return Err(E_BAL);
        }
        let n = parse_name(s)?;
        if self.names.contains(&n) {
            return Err(E_DUP);
        }
        self.names.push(n);
        Ok(())
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }

    /// 按年分组：返回 (年, 该年镜像数) 升序表。
    pub fn by_year(&self) -> Vec<(u16, usize)> {
        let mut years: Vec<u16> = Vec::new();
        for n in &self.names {
            if !years.contains(&n.year) {
                years.push(n.year);
            }
        }
        years.sort_unstable();
        years
            .into_iter()
            .map(|y| (y, self.names.iter().filter(|n| n.year == y).count()))
            .collect()
    }

    /// 分组守恒：Σ年组 == 总数；组内年唯一。
    pub fn audit(&self) -> Result<(), u16> {
        let groups = self.by_year();
        let sum: usize = groups.iter().map(|(_, c)| *c).sum();
        if sum != self.names.len() {
            return Err(E_BAL);
        }
        for w in groups.windows(2) {
            if w[0].0 >= w[1].0 {
                return Err(E_BAL);
            }
        }
        Ok(())
    }
}

/// 规范名渲染（往返口径）。
pub fn render_name(n: &ImgName) -> String {
    let tag = match n.tag {
        Some(t) => format!("-{}", t),
        None => String::new(),
    };
    format!("varix-{:04}.{}.{}{}.img", n.year, n.major, n.minor, tag)
}

// ── 确定性演练：合法 4 例 + 注入拦截 5 例 ─────────────────────────

/// 演练入口（ktest 挂点）。
pub fn drill() -> Result<(), u16> {
    let mut hits = 0u32;

    // L1：三种标签 + 无标签名全解析。
    for s in [
        "varix-2026.1.0.img",
        "varix-2026.1.1-stable.img",
        "varix-2026.2.0-beta.img",
        "varix-2025.12.31-dbg.img",
    ] {
        parse_name(s)?;
    }
    hits += 1;

    // L2：归档 2026 三 + 2025 一 → 分组 (2025,1),(2026,3) 守恒。
    let mut b = ArchiveBook::new()?;
    b.archive("varix-2026.1.0.img")?;
    b.archive("varix-2026.1.1-stable.img")?;
    b.archive("varix-2026.2.0-beta.img")?;
    b.archive("varix-2025.9.9-dbg.img")?;
    b.audit()?;
    let g = b.by_year();
    if g != vec![(2025, 1), (2026, 3)] {
        return Err(E_BAL);
    }
    hits += 1;

    // L3：渲染往返——解析再渲染等于原串。
    let n = parse_name("varix-2026.1.1-stable.img")?;
    if render_name(&n) != "varix-2026.1.1-stable.img" {
        return Err(E_BAL);
    }
    let n2 = parse_name("varix-2026.1.0.img")?;
    if render_name(&n2) != "varix-2026.1.0.img" {
        return Err(E_BAL);
    }
    hits += 1;

    // L4：标签闭集边界三值全收。
    for t in TAGS {
        let s = format!("varix-2026.0.0-{}.img", t);
        parse_name(&s)?;
    }
    hits += 1;

    // I1：前缀/后缀缺失拒。
    if !matches!(parse_name("vx-2026.1.0.img"), Err(E_FMT)) {
        return Err(E_FMT);
    }
    if !matches!(parse_name("varix-2026.1.0"), Err(E_FMT)) {
        return Err(E_FMT);
    }
    hits += 1;

    // I2：年份非法——3 位 / 5 位 / 3000。
    if !matches!(parse_name("varix-226.1.0.img"), Err(E_YEAR)) {
        return Err(E_YEAR);
    }
    if !matches!(parse_name("varix-20261.1.0.img"), Err(E_YEAR)) {
        return Err(E_YEAR);
    }
    if !matches!(parse_name("varix-3000.1.0.img"), Err(E_YEAR)) {
        return Err(E_YEAR);
    }
    hits += 1;

    // I3：版本越纲与语法断裂。
    if !matches!(parse_name("varix-2026.1000.0.img"), Err(E_VER)) {
        return Err(E_VER);
    }
    if !matches!(parse_name("varix-2026.1.img"), Err(E_FMT)) {
        return Err(E_FMT);
    }
    hits += 1;

    // I4：标签越纲拒。
    if !matches!(parse_name("varix-2026.1.0-nightly.img"), Err(E_TAG)) {
        return Err(E_TAG);
    }
    hits += 1;

    // I5：归档重名拒 + 封册拒记 + 分组注入。
    let mut i5 = ArchiveBook::new()?;
    i5.archive("varix-2026.1.0.img")?;
    if i5.archive("varix-2026.1.0.img") != Err(E_DUP) {
        return Err(E_DUP);
    }
    i5.seal();
    if i5.archive("varix-2026.2.0.img") != Err(E_BAL) {
        return Err(E_BAL);
    }
    hits += 1;

    let _ = hits;
    Ok(())
}
