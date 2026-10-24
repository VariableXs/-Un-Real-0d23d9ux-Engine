//! VE-F3808 · 无障碍纹理与图形（VE-T 域 · 无障碍渲染 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3808`
//!
//! **判据（锚点原文）**：图形 3:1 门、纹理替代、图标语义、对比红线、判据。
//!
//! **本条是什么**：纹理与图形的无障碍保障——图形对比度 3:1 门
//! （非文本对比红线）；纹理有图案替代（不靠颜色单独传信息）；图标语义
//! 标注强制；纹理噪声对读屏/光敏的防护。
//!
//! 诊断码段：独占 0xC1xx。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ─────────────────────────────────────────────────────────────
// 诊断码（0xC1xx）
// ─────────────────────────────────────────────────────────────

/// F3808 诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C1Code(pub u16);

impl C1Code {
    /// 图形对比 <3:1——P1。
    pub const CONTRAST_LOW: C1Code = C1Code(0xC101);
    /// 仅颜色传信息——红线。
    pub const COLOR_ONLY: C1Code = C1Code(0xC102);
    /// 图标无语义——补标注。
    pub const ICON_NO_SEM: C1Code = C1Code(0xC103);
    /// 纹理无替代——补。
    pub const TEXTURE_NO_ALT: C1Code = C1Code(0xC104);
    /// 图形缺登记。
    pub const GRAPHIC_MISS: C1Code = C1Code(0xC105);
    /// 替代缺位。
    pub const ALT_MISS: C1Code = C1Code(0xC106);

    /// 诊断说明。
    pub fn explain(self) -> &'static str {
        match self.0 {
            0xC101 => "图形对比度低于 3:1，P1。",
            0xC102 => "仅靠颜色传递信息，无障碍红线。",
            0xC103 => "图标缺语义标注。",
            0xC104 => "纹理缺图案替代。",
            0xC105 => "图形项缺登记。",
            0xC106 => "替代通道缺位。",
            _ => "未知诊断码。",
        }
    }

    /// 数值。
    pub fn raw(self) -> u16 {
        self.0
    }
}

// ─────────────────────────────────────────────────────────────
// 图形 3:1 门
// ─────────────────────────────────────────────────────────────

/// 图形元素。
#[derive(Clone, Copy, Debug)]
pub struct Graphic {
    /// 对比度千分比（3000 = 3:1）。
    pub contrast_bp: u16,
    /// 有图案替代。
    pub has_pattern: bool,
    /// 有语义标注。
    pub has_semantic: bool,
    /// 是否仅靠颜色传信息。
    pub color_only: bool,
}

impl Graphic {
    /// 构造。
    pub const fn new(contrast_bp: u16) -> Self {
        Graphic {
            contrast_bp,
            has_pattern: false,
            has_semantic: false,
            color_only: false,
        }
    }

    /// 过 3:1 门。
    pub fn pass_3to1(self) -> bool {
        self.contrast_bp >= 3_000
    }

    /// 无障碍合规（3:1 + 非仅色 + 语义）。
    pub fn a11y_ok(self) -> bool {
        self.pass_3to1() && !self.color_only && self.has_semantic
    }
}

/// 图形清单。
#[derive(Clone, Debug)]
pub struct GraphicList {
    items: Vec<Graphic>,
}

impl GraphicList {
    /// 新建。
    pub fn new() -> Self {
        GraphicList { items: Vec::new() }
    }

    /// 收一条。
    pub fn push(&mut self, g: Graphic) -> usize {
        self.items.push(g);
        self.items.len() - 1
    }

    /// 可变取。
    pub fn get_mut(&mut self, i: usize) -> Option<&mut Graphic> {
        self.items.get_mut(i)
    }

    /// 对比不足数。
    pub fn low_contrast(&self) -> usize {
        self.items.iter().filter(|g| !g.pass_3to1()).count()
    }

    /// 仅色传信数。
    pub fn color_only_count(&self) -> usize {
        self.items.iter().filter(|g| g.color_only).count()
    }

    /// 无语义数。
    pub fn no_semantic(&self) -> usize {
        self.items.iter().filter(|g| !g.has_semantic).count()
    }

    /// 总数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

impl Default for GraphicList {
    fn default() -> Self {
        GraphicList::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 纹理替代 + 图标语义
// ─────────────────────────────────────────────────────────────

/// 纹理项。
#[derive(Clone, Copy, Debug)]
pub struct Texture {
    /// 图案替代 id（0=无）。
    pub pattern_alt: u16,
    /// 噪声强度（0..100）。
    pub noise: u8,
    /// 光敏安全（噪声 ≤ 阈值）。
    pub photo_safe: bool,
}

impl Texture {
    /// 构造。
    pub const fn new(noise: u8) -> Self {
        Texture {
            pattern_alt: 0,
            noise,
            photo_safe: noise <= 30,
        }
    }

    /// 有替代。
    pub fn has_alt(self) -> bool {
        self.pattern_alt != 0
    }

    /// 合规（有替代 + 光敏安全）。
    pub fn ok(self) -> bool {
        self.has_alt() && self.photo_safe
    }
}

/// 图标语义表。
#[derive(Clone, Debug)]
pub struct IconSem {
    /// 图标 id → 语义标注 id（0=缺）。
    marks: Vec<(u16, u16)>,
}

impl IconSem {
    /// 新建。
    pub fn new() -> Self {
        IconSem { marks: Vec::new() }
    }

    /// 标注。
    pub fn mark(&mut self, icon: u16, sem: u16) -> bool {
        if sem == 0 {
            return false;
        }
        for e in self.marks.iter_mut() {
            if e.0 == icon {
                e.1 = sem;
                return true;
            }
        }
        self.marks.push((icon, sem));
        true
    }

    /// 缺标注数。
    pub fn unmarked(&self, total: usize) -> usize {
        let marked = self.marks.iter().filter(|e| e.1 != 0).count();
        total.saturating_sub(marked)
    }

    /// 全标注。
    pub fn complete(&self, total: usize) -> bool {
        self.unmarked(total) == 0
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.marks.len()
    }

    /// 空。
    pub fn is_empty(&self) -> bool {
        self.marks.is_empty()
    }
}

impl Default for IconSem {
    fn default() -> Self {
        IconSem::new()
    }
}

// ─────────────────────────────────────────────────────────────
// 判据集
// ─────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

/// F3808 判据集。
pub fn run_f3808_checks() -> CheckSet {
    let mut set = CheckSet::new("VE-F3808");

    // 0xC101 3:1 门
    {
        let g = Graphic::new(2999);
        set.add("0xC101-2999不过", !g.pass_3to1(), "");
        let g2 = Graphic::new(3000);
        set.add("0xC101-3000过", g2.pass_3to1(), "");
    }

    // 0xC102 仅色红线
    {
        let mut g = Graphic::new(4000);
        g.color_only = true;
        g.has_semantic = true;
        set.add("0xC102-仅色不合规", !g.a11y_ok(), "");
        g.color_only = false;
        set.add("0xC102-修后合规", g.a11y_ok(), "");
    }

    // 0xC103 图标语义
    {
        let mut g = Graphic::new(4000);
        set.add("0xC103-无语义不合规", !g.a11y_ok(), "");
        g.has_semantic = true;
        set.add("0xC103-有语义合规", g.a11y_ok(), "");
    }

    // 清单统计
    {
        let mut l = GraphicList::new();
        l.push(Graphic::new(2000));
        l.push(Graphic::new(5000));
        set.add("0xC101-低对比计数", l.low_contrast() == 1, "");
        set.add("0xC105-总数", l.len() == 2, "");
    }

    // 0xC104 纹理替代
    {
        let mut t = Texture::new(20);
        set.add("0xC104-无替代不合规", !t.ok() && !t.has_alt(), "");
        t.pattern_alt = 7;
        set.add("0xC104-有替代合规", t.ok(), "");
        let t2 = Texture::new(50);
        set.add("0xC104-高噪声不安全", !t2.photo_safe, "");
    }

    // 0xC106/0xC103 图标语义表
    {
        let mut s = IconSem::new();
        set.add("0xC106-空表", s.is_empty(), "");
        s.mark(1, 10);
        s.mark(2, 20);
        set.add("0xC103-全标注", s.complete(2), "");
        set.add("0xC103-缺一", !s.complete(3) && s.unmarked(3) == 1, "");
        set.add("0xC103-零语义拒", !s.mark(3, 0), "");
    }

    // 清单空态
    {
        let l = GraphicList::new();
        set.add("0xC105-空清单", l.is_empty() && l.low_contrast() == 0, "");
    }

    set
}
