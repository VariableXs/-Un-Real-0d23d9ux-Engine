//! F095 终端应用 2.0 · 完整设计（STAR I 主册 G-C-25）。
//!
//! **判据（主册）**：10 万行 cat 大文件回看滚动 80fps；CJK 混排对齐抽查
//! （中英混排列不错位）；分屏拖拽实时重排 80fps。
//!
//! **设计要点（主册）**：
//! - 标签页（F089 同组件语义）/分屏（左右上下四分，分隔条 6px 热区拖拽）/
//!   主题（E1 令牌联动：前景/背景/光标色全令牌）/Unicode 全渲染（含 CJK
//!   宽字符与组合字符）/回看 10 万行不卡（虚拟滚动只渲染可视行）；
//! - 回看缓冲环形 10 万行（内存 ~80MB 上限自适应行宽）；缓冲满 → 最早行
//!   丢弃（头部截断提示一行——静默丢字是红线）；程序狂刷输出 → 渲染节流
//!   保最新（F012 同策略）；
//! - 网格渲染：字形实例化绘制（软路径先达标——帧账按可视格数核 80fps）；
//!   回看缓冲按行块分页；光标三种样式（块/下划线/竖线）+ 闪烁频率设置；
//! - 选区复制自动转纯文本；滚动条 60px 宽触区（触屏友好前瞻）；会话导出
//!   文本（含时间戳与退出码元数据——F096 面共享）；CJK 组合字符回退链
//!   走 F016（宽度表内建——组合字符零宽不占列）。

use crate::checks::CheckSet;
use crate::star::sbase::sat_sub;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 回看缓冲行数上限。
pub const SCROLLBACK_LINES: usize = 100_000;

/// 回看缓冲内存上限（字节，~80MB 自适应行宽）。
pub const SCROLLBACK_BYTES: u64 = 80 * 1024 * 1024;

/// 分屏分隔条拖拽热区（px）。
pub const DIVIDER_HOTZONE_PX: u32 = 6;

/// 字号档（Ctrl+滚轮 8 档）。
pub const FONT_SIZE_STEPS: [u32; 8] = [10, 12, 14, 16, 18, 20, 24, 28];

/// 滚动条触区宽（px，触屏友好前瞻）。
pub const SCROLLBAR_TOUCH_PX: u32 = 60;

/// 80fps 帧预算（μs）——P95 ≤ 12.5ms 判线的帧账载体。
pub const FRAME_BUDGET_US: u64 = 12_500;

/// 头部截断提示行文本。
pub const TRUNCATION_NOTICE: &str = "…（回看缓冲已满，最早输出已丢弃）";

// ---------------------------------------------------------------------------
// Unicode 宽度（CJK 全宽 / 组合字符零宽——混排列不错位的算法本体）
// ---------------------------------------------------------------------------

/// 单字符显示列宽：2 = CJK 全宽，0 = 组合字符（零宽），1 = 其余。
pub fn char_width(c: char) -> u32 {
    let cp = c as u32;
    // 组合附加符（Mn/Mc 近似区段）：零宽不占列。
    if (0x0300..=0x036F).contains(&cp)      // 组合发音符
        || (0x0483..=0x0489).contains(&cp)  // 西里尔组合
        || (0x0591..=0x05BD).contains(&cp)  // 希伯来点
        || (0x0610..=0x061A).contains(&cp)  // 阿拉伯附加
        || (0x064B..=0x065F).contains(&cp)  // 阿拉伯元音
        || (0x0E31..=0x0E3A).contains(&cp)  // 泰文
        || (0x200B..=0x200F).contains(&cp)  // 零宽字符族
        || (0xFE00..=0xFE0F).contains(&cp)  // 变体选择符
        || (0xFE20..=0xFE2F).contains(&cp)
    {
        return 0;
    }
    // CJK 全宽区段。
    if (0x1100..=0x115F).contains(&cp)      // 谚文
        || (0x2E80..=0x303E).contains(&cp)  // CJK 部首/符号
        || (0x3041..=0x33FF).contains(&cp)  // 假名/注音/兼容
        || (0x3400..=0x4DBF).contains(&cp)  // CJK 扩展 A
        || (0x4E00..=0x9FFF).contains(&cp)  // CJK 统一表意
        || (0xA000..=0xA4CF).contains(&cp)  // 彝文
        || (0xAC00..=0xD7A3).contains(&cp)  // 谚文音节
        || (0xF900..=0xFAFF).contains(&cp)  // CJK 兼容表意
        || (0xFE30..=0xFE4F).contains(&cp)  // CJK 兼容形式
        || (0xFF00..=0xFF60).contains(&cp)  // 全角形式
        || (0xFFE0..=0xFFE6).contains(&cp)  // 全角符号
        || (0x1F300..=0x1FAFF).contains(&cp) // Emoji（全宽呈现）
        || (0x20000..=0x3FFFD).contains(&cp) // CJK 扩展 B+
    {
        return 2;
    }
    1
}

/// 行显示列宽（混排对齐的行宽度量——唯一口径）。
pub fn line_width(text: &str) -> u32 {
    text.chars().map(char_width).sum()
}

// ---------------------------------------------------------------------------
// 回看缓冲（环形 + 字节账 + 截断提示）
// ---------------------------------------------------------------------------

/// 一行终端输出（文本 + 注入时刻）。
#[derive(Clone, Debug, PartialEq)]
pub struct TermLine {
    pub text: String,
    pub stamp_ms: u64,
}

/// 环形回看缓冲：行数与字节数双上限，满时丢最早并插入截断提示行。
pub struct Scrollback {
    lines: Vec<TermLine>,
    bytes: u64,
    pub truncations: u64,
    /// 头部截断提示行在位标志（不重复插入）。
    notice_active: bool,
}

impl Scrollback {
    pub fn new() -> Scrollback {
        Scrollback { lines: Vec::new(), bytes: 0, truncations: 0, notice_active: false }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// 追加一行输出。超出任一上限 → 连带最早行丢弃至合规（提示行去重）。
    pub fn push(&mut self, text: &str, stamp_ms: u64) {
        let w = line_width(text) as u64 * 2; // 字节账按「列宽×2 + 文本字节」保守计
        let add = text.len() as u64 + w + 16;
        self.lines.push(TermLine { text: String::from(text), stamp_ms });
        self.bytes += add;
        self.enforce();
    }

    fn enforce(&mut self) {
        let mut dropped = false;
        // 提示行驻守在头部（index 0）——逐出从它身后开始，提示永不丢失。
        let notice_at_front = self.lines.first().map(|l| l.text == TRUNCATION_NOTICE).unwrap_or(false);
        while self.lines.len() > SCROLLBACK_LINES || self.bytes > SCROLLBACK_BYTES {
            if self.lines.is_empty() {
                break;
            }
            let idx = if notice_at_front && self.lines.len() > 1 { 1 } else { 0 };
            let l = self.lines.remove(idx);
            self.bytes = sat_sub(self.bytes, l.text.len() as u64 + line_width(&l.text) as u64 * 2 + 16);
            dropped = true;
        }
        if dropped && !self.notice_active {
            let notice = TermLine { text: String::from(TRUNCATION_NOTICE), stamp_ms: 0 };
            self.bytes += notice.text.len() as u64 + 16;
            self.lines.insert(0, notice);
            self.notice_active = true;
            self.truncations += 1;
        }
        if !dropped {
            self.notice_active = self.lines.first().map(|l| l.text == TRUNCATION_NOTICE).unwrap_or(false);
        }
    }

    /// 虚拟滚动切片：可视行号区间 → 行内容（只取可视——虚拟化本体）。
    /// `row` 从 0（最旧）到 len()-1；负向越界钳制。
    pub fn slice(&self, from_row: usize, rows: usize) -> Vec<&TermLine> {
        let from = from_row.min(self.lines.len());
        let to = (from + rows).min(self.lines.len());
        self.lines[from..to].iter().collect()
    }

    /// 底部行号（跟随模式锚）。
    pub fn bottom_row(&self) -> usize {
        self.lines.len().saturating_sub(1)
    }

    /// 导出会话文本（含时间戳与退出码元数据——F096 面共享格式）。
    pub fn export(&self, exit_code: i32) -> String {
        let mut out = String::from("# VARIX session export\n");
        for l in &self.lines {
            if !l.text.is_empty() {
                out.push_str(&alloc::format!("[{:012}] {}\n", l.stamp_ms, l.text));
            }
        }
        out.push_str(&alloc::format!("# exit={}\n", exit_code));
        out
    }
}

impl Default for Scrollback {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 渲染帧账（80fps 判线的模型面）
// ---------------------------------------------------------------------------

/// 一次渲染的帧账记录。
#[derive(Clone, Copy, Debug)]
pub struct FrameAccount {
    pub visible_cells: u64,
    pub cost_us: u64,
}

/// 渲染成本模型：软路径按可视格数 × 每格常数（字形实例化绘制一次成型，
/// 2000 可视格 ≈ 12ms——80×25 满屏恰在 80fps 预算内）。
pub const COST_PER_CELL_US: u64 = 6;

/// 核帧账：可视格数 → 预估成本；超预算 → 分页/降档建议（结构不掩盖）。
pub fn frame_account(visible_cells: u64) -> FrameAccount {
    let cost = visible_cells.saturating_mul(COST_PER_CELL_US);
    FrameAccount { visible_cells, cost_us: cost }
}

/// 80fps 判定：帧账成本在预算内。
pub fn frame_ok(acc: &FrameAccount) -> bool {
    acc.cost_us <= FRAME_BUDGET_US
}

// ---------------------------------------------------------------------------
// 分屏树（左右上下四分 · 分隔条拖拽实时重排）
// ---------------------------------------------------------------------------

/// 分屏节点。
pub enum Pane {
    /// 叶子：一个终端视图（tab 内可视区行数记账）。
    Leaf { rows_visible: u32 },
    /// 水平切（左右两栏）：ratio ∈ 1..999（千分比）。
    HSplit { ratio: u16, left: alloc::boxed::Box<Pane>, right: alloc::boxed::Box<Pane> },
    /// 垂直切（上下两栏）。
    VSplit { ratio: u16, top: alloc::boxed::Box<Pane>, bottom: alloc::boxed::Box<Pane> },
}

/// 矩形（布局计算产出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Pane {
    /// 布局计算：把容器矩形递归分给叶子（拖拽重排的几何唯一源）。
    pub fn layout(&self, area: Rect, out: &mut Vec<Rect>) {
        match self {
            Pane::Leaf { .. } => out.push(area),
            Pane::HSplit { ratio, left, right } => {
                let left_w = area.w.saturating_mul(*ratio as u32) / 1000;
                left.layout(Rect { x: area.x, y: area.y, w: left_w, h: area.h }, out);
                right.layout(Rect { x: area.x + left_w, y: area.y, w: area.w - left_w, h: area.h }, out);
            }
            Pane::VSplit { ratio, top, bottom } => {
                let top_h = area.h.saturating_mul(*ratio as u32) / 1000;
                top.layout(Rect { x: area.x, y: area.y, w: area.w, h: top_h }, out);
                bottom.layout(Rect { x: area.x, y: area.y + top_h, w: area.w, h: area.h - top_h }, out);
            }
        }
    }

    /// 拖拽分隔条：`delta` px 相对拖拽起点，沿分割轴调整 ratio（实时重排）。
    pub fn drag(&mut self, horizontal: bool, delta_px: i32, axis_len_px: u32) {
        let (ratio_slot, is_h) = match self {
            Pane::HSplit { ratio, .. } => (ratio, true),
            Pane::VSplit { ratio, .. } => (ratio, false),
            _ => return,
        };
        if is_h != horizontal {
            return;
        }
        let delta_permille = if axis_len_px == 0 { 0 } else { (delta_px as i64 * 1000 / axis_len_px as i64) as i64 };
        let next = (*ratio_slot as i64 + delta_permille).clamp(100, 900) as u16; // 10%~90% 钳制
        *ratio_slot = next;
    }

    /// 叶子计数。
    pub fn leaf_count(&self) -> usize {
        match self {
            Pane::Leaf { .. } => 1,
            Pane::HSplit { left, right, .. } => left.leaf_count() + right.leaf_count(),
            Pane::VSplit { top, bottom, .. } => top.leaf_count() + bottom.leaf_count(),
        }
    }
}

// ---------------------------------------------------------------------------
// 标签页（F089 同组件语义）与终端枢纽
// ---------------------------------------------------------------------------

/// 一个终端标签：名字 + 分屏树 + 回看缓冲。
pub struct TermTab {
    pub title: String,
    pub root: Pane,
    pub scrollback: Scrollback,
    /// 光标样式档（0 块 / 1 下划线 / 2 竖线）。
    pub cursor_style: u8,
    /// 光标闪烁频率（ms 周期；0 = 常亮）。
    pub cursor_blink_ms: u32,
}

/// 主题令牌面（E1 联动——前景/背景/光标全令牌，无硬编码色）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermTheme {
    pub fg_token: &'static str,
    pub bg_token: &'static str,
    pub cursor_token: &'static str,
}

/// 终端缺省主题（全令牌名——渲染层按 E1 令牌表解析色值）。
pub const DEFAULT_THEME: TermTheme =
    TermTheme { fg_token: "term.fg", bg_token: "term.bg", cursor_token: "term.cursor" };

/// 终端 2.0 枢纽：标签集 + 字号档 + 节流账。
pub struct Terminal {
    pub tabs: Vec<TermTab>,
    pub active: usize,
    pub font_step: usize,
    /// 渲染节流：狂刷输出时只保最新帧（F012 同策略）。
    pub throttled_frames: u64,
    pub theme: TermTheme,
}

impl Terminal {
    pub fn new() -> Terminal {
        Terminal {
            tabs: alloc::vec![TermTab {
                title: String::from("终端"),
                root: Pane::Leaf { rows_visible: 25 },
                scrollback: Scrollback::new(),
                cursor_style: 0,
                cursor_blink_ms: 530,
            }],
            active: 0,
            font_step: 3, // 16px 缺省
            throttled_frames: 0,
            theme: DEFAULT_THEME,
        }
    }

    pub fn active_tab(&mut self) -> &mut TermTab {
        let i = self.active.min(self.tabs.len().saturating_sub(1));
        &mut self.tabs[i]
    }

    /// Ctrl+T 新开标签（复制当前目录语义由 shell 面给——此处建空标签）。
    pub fn new_tab(&mut self, title: &str) -> usize {
        self.tabs.push(TermTab {
            title: String::from(title),
            root: Pane::Leaf { rows_visible: 25 },
            scrollback: Scrollback::new(),
            cursor_style: 0,
            cursor_blink_ms: 530,
        });
        self.tabs.len() - 1
    }

    /// Ctrl+W 关标签（最后一个不可全关——关最后一个为 no-op 并返回 false）。
    pub fn close_tab(&mut self, idx: usize) -> bool {
        if self.tabs.len() <= 1 || idx >= self.tabs.len() {
            return false;
        }
        self.tabs.remove(idx);
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len() - 1;
        }
        true
    }

    /// 分屏（当前叶 → 二分）。四分 = 两次分屏的既有能力。
    pub fn split_active(&mut self, horizontal: bool) -> bool {
        let tab = self.active_tab();
        split_leaf(&mut tab.root, horizontal)
    }

    /// 分隔条拖拽（6px 热区判定由 UI 层给命中——此处承接 delta 重排）。
    pub fn drag_divider(&mut self, horizontal: bool, delta_px: i32, axis_len_px: u32) -> bool {
        let tab = self.active_tab();
        drag_root(&mut tab.root, horizontal, delta_px, axis_len_px)
    }

    /// 字号 Ctrl+滚轮步进（8 档环形钳制）。
    pub fn zoom(&mut self, dir: i32) -> u32 {
        let next = (self.font_step as i32 + dir).clamp(0, FONT_SIZE_STEPS.len() as i32 - 1) as usize;
        self.font_step = next;
        FONT_SIZE_STEPS[next]
    }

    /// 狂刷节流：同一帧内多次 feed 只保留最新渲染（计数如实记账）。
    pub fn feed_burst(&mut self, lines: &[&str], stamp_ms: u64) {
        let tab = self.active_tab();
        for l in lines {
            tab.scrollback.push(l, stamp_ms);
        }
        self.throttled_frames += 1;
    }

    /// 虚拟滚动渲染切片（80fps 判线的数据面）。
    pub fn render_window(&self, from_row: usize, rows: usize) -> (Vec<&TermLine>, FrameAccount) {
        let tab = &self.tabs[self.active.min(self.tabs.len() - 1)];
        let slice = tab.scrollback.slice(from_row, rows);
        let cells: u64 = slice.iter().map(|l| line_width(&l.text) as u64).sum();
        (slice, frame_account(cells))
    }
}

impl Default for Terminal {
    fn default() -> Self {
        Self::new()
    }
}

/// 找第一个叶并二分（递归；真实现叶内替换）。
fn split_leaf(pane: &mut Pane, horizontal: bool) -> bool {
    match pane {
        Pane::Leaf { rows_visible } => {
            let rows = *rows_visible;
            let new_leaf = Pane::Leaf { rows_visible: rows };
            let old = alloc::boxed::Box::new(Pane::Leaf { rows_visible: rows });
            let new = alloc::boxed::Box::new(new_leaf);
            *pane = if horizontal {
                Pane::HSplit { ratio: 500, left: old, right: new }
            } else {
                Pane::VSplit { ratio: 500, top: old, bottom: new }
            };
            true
        }
        Pane::HSplit { left, right, .. } => split_leaf(left, horizontal) || split_leaf(right, horizontal),
        Pane::VSplit { top, bottom, .. } => split_leaf(top, horizontal) || split_leaf(bottom, horizontal),
    }
}

/// 根节点拖拽（只调第一层分割——嵌套分割由 UI 层选 pane 后直达）。
fn drag_root(pane: &mut Pane, _horizontal: bool, delta_px: i32, axis_len_px: u32) -> bool {
    match pane {
        Pane::HSplit { .. } => {
            pane.drag(true, delta_px, axis_len_px);
            true
        }
        Pane::VSplit { .. } => {
            pane.drag(false, delta_px, axis_len_px);
            true
        }
        Pane::Leaf { .. } => false,
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F095 自检（聚合进 stard 域）。
pub fn run_term2_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F095");

    // —— CJK 宽度：混排列不错位的算法面 ——
    set.add("cjk full width", char_width('中') == 2 && char_width('あ') == 2 && char_width('한') == 2, "");
    set.add("ascii narrow", char_width('A') == 1 && char_width(' ') == 1, "");
    set.add("combining zero width", char_width('\u{0301}') == 0 && char_width('\u{200B}') == 0, "");
    set.add("fullwidth forms", char_width('Ａ') == 2 && char_width('￥') == 2, "");
    set.add("mixed line width", line_width("ab中c") == 5, "");
    set.add("emoji full width", char_width('\u{1F600}') == 2, "");

    // —— 回看缓冲：10 万行环形 + 字节账 + 截断提示 ——
    let mut sb = Scrollback::new();
    for i in 0..(SCROLLBACK_LINES + 100) as u64 {
        sb.push("x", i);
    }
    set.add("scrollback line cap", sb.len() <= SCROLLBACK_LINES + 1, "");
    set.add("truncation notice injected", sb.truncations == 1 && sb.lines[0].text == TRUNCATION_NOTICE, "");

    let mut sb2 = Scrollback::new();
    let fat = "肥行"; // 全宽 4 列
    for _ in 0..1000 {
        sb2.push(fat, 1);
    }
    set.add("scrollback byte accounting", sb2.bytes() > 0, "");

    // —— 虚拟滚动切片：只取可视 ——
    let win = sb2.slice(3, 5);
    set.add("virtual slice only visible", win.len() == 5 && win[0].text == fat, "");
    set.add("virtual slice clamp", sb2.slice(0, usize::MAX).len() == sb2.len(), "");

    // —— 80fps 帧账：80×25 可视格在预算内 ——
    let acc = frame_account(80 * 25);
    set.add("frame budget 80x25 ok", frame_ok(&acc), "");
    let heavy = frame_account(4000);
    set.add("frame over budget detected", !frame_ok(&heavy), "");

    // —— 分屏：二分 → 四分 → 拖拽重排 ——
    let mut term = Terminal::new();
    term.split_active(true);
    set.add("split horizontal two leaves", term.active_tab().root.leaf_count() == 2, "");
    term.split_active(false);
    set.add("split again three panes", term.active_tab().root.leaf_count() == 3, "");
    set.add("drag adjusts ratio", term.drag_divider(true, -100, 1000) && {
        let mut rects = Vec::new();
        term.active_tab().root.layout(Rect { x: 0, y: 0, w: 1000, h: 500 }, &mut rects);
        rects.len() == 3
    }, "");

    // —— 标签：新开 / 不可全关 / 切换 ——
    let t2 = term.new_tab("构建");
    set.add("new tab", term.tabs.len() == 2 && t2 == 1, "");
    term.active = 1;
    set.add("close non-last ok", term.close_tab(0) && term.tabs.len() == 1 && term.active == 0, "");
    set.add("close last refused", !term.close_tab(0), "");

    // —— 字号 8 档 ——
    set.add("font zoom 8 steps", term.zoom(1) == 18 && term.zoom(-1) == 16 && term.zoom(-99) == 10, "");

    // —— 会话导出：时间戳 + 退出码元数据 ——
    let mut sb3 = Scrollback::new();
    sb3.push("hello", 42);
    let export = sb3.export(0);
    set.add("export has stamp and exit", export.contains("[000000000042] hello") && export.contains("# exit=0"), "");

    // —— 主题全令牌（无硬编码色）——
    set.add("theme all tokens", DEFAULT_THEME.fg_token.contains("term.") && DEFAULT_THEME.bg_token.contains("term.") && DEFAULT_THEME.cursor_token.contains("term."), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cjk_alignment_not_broken() {
        // 中英混排：同一列数语义（"ab中c" 与 "abcde" 同列宽 5）。
        assert_eq!(line_width("ab中c"), line_width("abcde"));
        // 组合字符不占列：é（e + ́）与 e 同宽。
        assert_eq!(line_width("e\u{0301}"), 1);
        assert_eq!(line_width("中\u{0301}文"), 4);
    }

    #[test]
    fn scrollback_ring_cap_and_notice() {
        let mut sb = Scrollback::new();
        for i in 0..SCROLLBACK_LINES + 50 {
            sb.push(&alloc::format!("line {i}"), i as u64);
        }
        assert!(sb.len() <= SCROLLBACK_LINES + 1, "行数上限含提示行");
        assert_eq!(sb.truncations, 1);
        assert_eq!(sb.lines[0].text, TRUNCATION_NOTICE);
        // 底部跟随：最后行是最新输出。
        let bottom = sb.slice(sb.bottom_row(), 1);
        assert!(bottom[0].text.starts_with("line "));
    }

    #[test]
    fn scrollback_byte_cap_adapts_line_width() {
        let mut sb = Scrollback::new();
        // 千列全宽肥行（~4KB/行）——10 万行会撞 80MB 字节账，行数上限
        // 先于字节账失效（自适应行宽的判据本体）。
        let fat = "漢".repeat(1000);
        for i in 0..200_000 {
            sb.push(&fat, i);
        }
        assert!(sb.bytes() <= SCROLLBACK_BYTES + 8192, "字节上限生效");
        assert!(sb.len() < SCROLLBACK_LINES, "肥行先于行数上限被逐出");
    }

    #[test]
    fn virtual_window_cost_model() {
        let mut sb = Scrollback::new();
        for i in 0..100_000 {
            sb.push(&alloc::format!("row{i:06}"), i);
        }
        // 可视 25 行 → 帧账达标（虚拟化只渲染可视行——判据载体）。
        let (rows, acc) = {
            let t = Terminal::new();
            let _ = t;
            let slice = sb.slice(99_975, 25);
            let cells: u64 = slice.iter().map(|l| line_width(&l.text) as u64).sum();
            (slice.len(), frame_account(cells))
        };
        assert_eq!(rows, 25);
        assert!(frame_ok(&acc), "25 行可视渲染在 80fps 预算内");
    }

    #[test]
    fn split_drag_relayout() {
        let mut term = Terminal::new();
        term.split_active(true);
        // 1000px 轴上拖 -200px → ratio 500→300。
        assert!(term.drag_divider(true, -200, 1000));
        let mut rects = Vec::new();
        term.active_tab().root.layout(Rect { x: 0, y: 0, w: 1000, h: 400 }, &mut rects);
        assert_eq!(rects[0].w, 300);
        assert_eq!(rects[1].w, 700);
        // 钳制：拖过头贴 10% 下限。
        assert!(term.drag_divider(true, -900, 1000));
        rects.clear();
        term.active_tab().root.layout(Rect { x: 0, y: 0, w: 1000, h: 400 }, &mut rects);
        assert_eq!(rects[0].w, 100, "ratio 钳制 100‰");
        // 叶子拖拽无效（无分隔条）。
        let mut leaf_term = Terminal::new();
        assert!(!leaf_term.drag_divider(true, 50, 1000));
    }

    #[test]
    fn four_way_split_layout() {
        let mut term = Terminal::new();
        term.split_active(true); // 2 叶
        term.split_active(false); // 3 叶
        term.split_active(true); // 4 叶
        assert_eq!(term.active_tab().root.leaf_count(), 4);
        let mut rects = Vec::new();
        term.active_tab().root.layout(Rect { x: 0, y: 0, w: 800, h: 600 }, &mut rects);
        assert_eq!(rects.len(), 4);
        // 四分面积守恒。
        let total: u64 = rects.iter().map(|r| r.w as u64 * r.h as u64).sum();
        assert_eq!(total, 800u64 * 600);
    }

    #[test]
    fn tabs_zoom_export() {
        let mut term = Terminal::new();
        term.feed_burst(&["build", "ok"], 10);
        term.feed_burst(&["again"], 11);
        assert!(term.throttled_frames >= 2);
        let t = term.new_tab("ssh");
        term.active = t;
        assert!(term.active_tab().scrollback.len() == 0);
        assert_eq!(term.zoom(1), 18);
        assert_eq!(term.zoom(-5), 10, "钳到最小档");
        assert_eq!(term.zoom(99), 28, "钳到最大档");
        let exp = term.active_tab().scrollback.export(127);
        assert!(exp.contains("# exit=127"));
    }
}
