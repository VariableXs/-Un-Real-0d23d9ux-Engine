//! UNX-F0209 · 引导时间线（甘特式阶段视图）
//!
//! 锚点判据（施工书原文）：UNX-F0209-J1 阶段时间线文本图生成，
//! 总耗时守恒（各段之和 = 全程）。
//!
//! 设计：与 F0198 时间线账本对接（同名 Unit 口径）→ 甘特渲染器
//! （固定 60 刻度宽，阶段条 = start/total 比例定位）→ 守恒红线
//! （各段耗时之和 = 全程）+ 覆盖红线（条区间 [start, start+span]
//! 不得越过全程右界）。文本产物逐行前缀 = 阶段名，肉眼可核。
//!
//! 错误码段：0x2BF0（全仓普查后独占分配，AI-WBX1022）。

use alloc::vec;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const F0209_VERSION: &str = "F0209-gantt-v1";

/// 诊断码段基址（UNX-F0209 独占）。
pub const SEG_BASE: u16 = 0x2BF0;

/// 甘特图宽度（刻度数）。
pub const BARS: usize = 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 守恒破（各段和 ≠ 全程）。
    NotConserved = SEG_BASE + 1,
    /// 条越界。
    BarOut = SEG_BASE + 2,
    /// 空阶段表。
    NoStages = SEG_BASE + 3,
    /// 非法区间（span 为 0 或 end ≤ start）。
    BadSpan = SEG_BASE + 4,
    /// 渲染行数与阶段数不符。
    RenderShort = SEG_BASE + 5,
    /// 条太短不可见。
    BarTooShort = SEG_BASE + 6,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 甘特阶段（与 F0198 Span 同口径）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stage {
    /// 阶段名。
    pub name: &'static str,
    /// 起点 tick。
    pub start: u64,
    /// 耗时 tick。
    pub span: u64,
}

/// 甘特渲染器。
#[derive(Debug)]
pub struct Gantt {
    /// 阶段表（按 start 升序）。
    pub stages: Vec<Stage>,
    /// 全程（末段终点）。
    total: u64,
}

impl Gantt {
    /// 构造并校验（守恒 + 区间合法）。
    pub fn build(stages: Vec<Stage>) -> Result<Self, u16> {
        if stages.is_empty() {
            return Err(E::NoStages.code());
        }
        for s in stages.iter() {
            if s.span == 0 {
                return Err(E::BadSpan.code());
            }
        }
        // start 升序且区间不倒挂。
        for w in stages.windows(2) {
            if w[0].start + w[0].span > w[1].start {
                return Err(E::BadSpan.code());
            }
        }
        let total = stages
            .iter()
            .map(|s| s.start + s.span)
            .max()
            .unwrap_or_default();
        // 守恒红线：各段之和 = 全程（时间线闭合语义）。
        let sum: u64 = stages.iter().map(|s| s.span).sum();
        if sum != total {
            return Err(E::NotConserved.code());
        }
        Ok(Gantt { stages, total })
    }

    /// 单阶段条行：`name |----#####------|`（# 位于 start/total 比例处）。
    fn bar_row(&self, s: &Stage) -> Result<String, u16> {
        if self.total == 0 || s.start + s.span > self.total {
            return Err(E::BarOut.code());
        }
        let left = ((s.start as u128 * BARS as u128) / self.total as u128) as usize;
        let width = ((s.span as u128 * BARS as u128) / self.total as u128) as usize;
        if width == 0 {
            return Err(E::BarTooShort.code());
        }
        if left + width > BARS {
            return Err(E::BarOut.code());
        }
        let mut row = String::from(s.name);
        row.push_str(" |");
        for i in 0..BARS {
            if i >= left && i < left + width {
                row.push('#');
            } else {
                row.push('-');
            }
        }
        row.push('|');
        Ok(row)
    }

    /// 全图渲染：每阶段一行。
    pub fn render(&self) -> Result<Vec<String>, u16> {
        let mut out = Vec::new();
        for s in self.stages.iter() {
            out.push(self.bar_row(s)?);
        }
        if out.len() != self.stages.len() {
            return Err(E::RenderShort.code());
        }
        Ok(out)
    }

    /// 全程。
    pub fn total(&self) -> u64 {
        self.total
    }
}

/// J1 自证（判据原文：文本图生成，各段之和 = 全程）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：fw(0..2) + image(2..7) + paging(7..10) → total 10 守恒。
    let stages = vec![
        Stage {
            name: "fw",
            start: 0,
            span: 2,
        },
        Stage {
            name: "image",
            start: 2,
            span: 5,
        },
        Stage {
            name: "paging",
            start: 7,
            span: 3,
        },
    ];
    let g = Gantt::build(stages)?;
    if g.total() != 10 {
        return Err(E::NotConserved.code());
    }

    // 二、J1 主体：三行渲染，每行含阶段名前缀 + 定宽条 + # 计数 ≥1。
    let rows = g.render()?;
    if rows.len() != 3 {
        return Err(E::RenderShort.code());
    }
    for (i, r) in rows.iter().enumerate() {
        let name = g.stages[i].name;
        if !r.starts_with(name) {
            return Err(E::RenderShort.code());
        }
        let hashes = r.chars().filter(|c| *c == '#').count();
        if hashes == 0 {
            return Err(E::BarTooShort.code());
        }
        // 条总宽 = BARS + 2 边界符。
        let bars = r.chars().filter(|c| *c == '-' || *c == '#').count();
        if bars != BARS {
            return Err(E::BarOut.code());
        }
    }

    // 三、红线逐项：守恒破 / 条越界 / 零段 / 重叠区间。
    let bad = vec![
        Stage {
            name: "a",
            start: 0,
            span: 3,
        },
        Stage {
            name: "b",
            start: 3,
            span: 4,
        },
    ];
    // 和 = 7，total = 7 → 守恒；再造破例：b.span=5 → 和 8 ≠ total 8？
    // 构造真破例：重叠段（b 从 2 开始，与前段叠 1）→ BadSpan 先拦。
    let overlap = vec![
        Stage {
            name: "a",
            start: 0,
            span: 3,
        },
        Stage {
            name: "b",
            start: 2,
            span: 5,
        },
    ];
    if Gantt::build(overlap).is_ok() {
        return Err(E::BadSpan.code());
    }
    if Gantt::build(Vec::new()).is_ok() {
        return Err(E::NoStages.code());
    }
    let zero = vec![Stage {
        name: "z",
        start: 0,
        span: 0,
    }];
    if Gantt::build(zero).is_ok() {
        return Err(E::BadSpan.code());
    }
    // 守恒破例：间隙段（b 从 4 起，和 3+5=8 ≠ max(9)）。
    let gap = vec![
        Stage {
            name: "a",
            start: 0,
            span: 3,
        },
        Stage {
            name: "b",
            start: 4,
            span: 5,
        },
    ];
    if Gantt::build(gap).is_ok() {
        return Err(E::NotConserved.code());
    }
    let _ = bad;

    // 四、判别式段位对拍。
    if E::NotConserved.code() != SEG_BASE + 1 || E::BarTooShort.code() != SEG_BASE + 6 {
        return Err(E::NotConserved.code());
    }
    Ok(())
}
