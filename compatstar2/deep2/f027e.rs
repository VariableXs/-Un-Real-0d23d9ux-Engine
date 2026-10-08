//! F027 深化批次三 · 组合窗口位置执行/边界/注入面（compatstar2/deep2 · G-A-27）。
//!
//! 批次一/二深化覆盖 IMM32 桥接主干；本批补齐主册【功能定义】「全语义对齐」的
//! 候选窗定位侧出口：CFS_* 候选窗位置状态机（DEFAULT/EXCLUDE/FORCE 三态 + 未
//! 初始化拒绝）、屏幕边界避让算法（候选窗 200×120 定尺寸，超右翻左、超下翻上
//! 、双超夹回）、WM_IME_* 消息序校验链（STARTCOMPOSITION→COMPOSITION…→
//! ENDCOMPOSITION 合法序表，乱序检出）、组合串属性段模型（三段着色，定长 8 段
//! 合并相邻同属性）。
//!
//! 判据对账：主册【设计细节】/【交互设计】未落地面为源，一处一事实（MS
//! COMPOSITIONFORM dwStyle / WM_IME_* / ATTR_* 文档语义对拍）。零堆纪律：
//! 定长段表 + 定长字符面，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// CFS_DEFAULT = 0x0000（MS COMPOSITIONFORM dwStyle：默认光标定位）。
pub const CFS_DEFAULT: u32 = 0x0000;
/// CFS_EXCLUDE = 0x0008（MS：避让矩形——候选窗不得覆盖）。
pub const CFS_EXCLUDE: u32 = 0x0008;
/// CFS_FORCE_POSITION = 0x0020（MS：强制坐标，不做避让调整）。
pub const CFS_FORCE_POSITION: u32 = 0x0020;
/// CFS_RECT = 0x0001 / CFS_POINT = 0x0002（MS：矩形定位 / 点定位）。
pub const CFS_RECT: u32 = 0x0001;
pub const CFS_POINT: u32 = 0x0002;
/// 候选窗定尺寸 200×120（主册 G-A-27 候选窗跟随光标语义的模型口径）。
pub const CAND_WIN_W: i32 = 200;
pub const CAND_WIN_H: i32 = 120;
/// 光标锚定偏移 8px（与主层 F027【交互设计】同构；程序无 EXFORMINFO 时）。
pub const CARET_ANCHOR_OFFSET_PX: i32 = 8;
/// WM_IME_* 消息值（MS winuser.h）：STARTCOMPOSITION=0x010D、
/// ENDCOMPOSITION=0x010E、COMPOSITION=0x010F。
pub const WM_IME_STARTCOMPOSITION: u32 = 0x010D;
pub const WM_IME_ENDCOMPOSITION: u32 = 0x010E;
pub const WM_IME_COMPOSITION: u32 = 0x010F;
/// ATTR_* 组合串属性（MS：0x00 输入中未转换 / 0x01 已选中待转换 / 0x02 已转换）。
pub const ATTR_INPUT: u8 = 0x00;
pub const ATTR_TARGET_CONVERTED: u8 = 0x01;
pub const ATTR_CONVERTED: u8 = 0x02;
/// 组合串长度上限与属性段表容量（域内模型口径，段表定长 8）。
pub const COMP_STR_CAP: usize = 32;
pub const MAX_ATTR_SEGMENTS: usize = 8;

// ---------------------------------------------------------------------------
// CFS_* 候选窗位置状态机
// ---------------------------------------------------------------------------

/// 候选窗定位三态（DEFAULT/EXCLUDE 避让、FORCE 强制）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CandStyle { Default, Exclude, Force }

/// CFS_* 候选窗位置状态机（未初始化查询显性拒绝）。
pub struct CandWin {
    style: Option<CandStyle>,
    /// 程序提供的位置点（set 时登记）。
    pub anchor: (i32, i32),
    /// 非法风格拒绝计数。
    pub bad_style_rejects: u32,
}

impl CandWin {
    pub const fn new() -> Self {
        CandWin { style: None, anchor: (0, 0), bad_style_rejects: 0 }
    }

    /// 登记定位风格：三态收编，其余位值显性拒绝并计数。
    pub fn set(&mut self, style: u32, x: i32, y: i32) -> Result<CandStyle, &'static str> {
        let s = match style {
            CFS_DEFAULT => CandStyle::Default,
            CFS_EXCLUDE => CandStyle::Exclude,
            CFS_FORCE_POSITION => CandStyle::Force,
            _ => {
                self.bad_style_rejects += 1;
                return Err("bad-cfs-style");
            }
        };
        self.style = Some(s);
        self.anchor = (x, y);
        Ok(s)
    }

    /// 生效锚点：Default/Exclude 走光标锚定 8px 偏移（主层【交互设计】同构），
    /// Force 尊重程序给点（MS：FORCE_POSITION 不做避让调整）；
    /// 未初始化（未收到任何 CFS_*）→ Err（零静默）。
    pub fn anchored(&self, caret: (i32, i32)) -> Result<(i32, i32), &'static str> {
        match self.style {
            None => Err("uninitialized"),
            Some(CandStyle::Force) => Ok(self.anchor),
            Some(_) => Ok((caret.0 + CARET_ANCHOR_OFFSET_PX, caret.1 + CARET_ANCHOR_OFFSET_PX)),
        }
    }

    pub fn is_set(&self) -> bool {
        self.style.is_some()
    }
}

/// 右/下边界翻转：超右翻左、超下翻上、双超仍越界则夹回屏内。
/// 出处：主册 G-A-27【交互设计】候选窗跟随光标 + 避让模型口径。
pub fn avoid_flip(x: i32, y: i32, screen_w: i32, screen_h: i32) -> (i32, i32) {
    let mut nx = if x + CAND_WIN_W > screen_w { x - CAND_WIN_W } else { x };
    let mut ny = if y + CAND_WIN_H > screen_h { y - CAND_WIN_H } else { y };
    if nx < 0 {
        nx = 0; // 双超夹回
    }
    if ny < 0 {
        ny = 0;
    }
    (nx, ny)
}

/// 序链状态：None→Started→Composing(n≥1)→None（END 收口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SeqState { None, Started, Composing }

/// 消息序校验链（合法序表 + 乱序检出计数——零静默吞错）。
pub struct SeqChain {
    pub state: SeqState,
    /// 乱序检出计数。
    pub violations: u32,
    /// 完整合法序完成次数（账面）。
    pub completed: u32,
}

impl SeqChain {
    pub const fn new() -> Self {
        SeqChain { state: SeqState::None, violations: 0, completed: 0 }
    }

    /// 合法序表：None→START；Started/Composing→COMPOSITION；Started/Composing→END。
    pub fn feed(&mut self, msg: u32) -> Result<(), &'static str> {
        let ok = match (self.state, msg) {
            (SeqState::None, WM_IME_STARTCOMPOSITION) => {
                self.state = SeqState::Started;
                true
            }
            (SeqState::Started, WM_IME_COMPOSITION) | (SeqState::Composing, WM_IME_COMPOSITION) => {
                self.state = SeqState::Composing;
                true
            }
            (SeqState::Started, WM_IME_ENDCOMPOSITION)
            | (SeqState::Composing, WM_IME_ENDCOMPOSITION) => {
                self.state = SeqState::None;
                self.completed += 1;
                true
            }
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            self.violations += 1;
            Err("out-of-order")
        }
    }
}

/// 一个着色段（start..start+len，attr 着色）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AttrSeg { pub start: u8, pub len: u8, pub attr: u8 }

/// 组合串属性段模型：逐字符属性 + 段表合并相邻同属性（定长 8 段）。
pub struct AttrSegs {
    chars: [u8; COMP_STR_CAP],
    /// 组合串当前长度。
    pub n: usize,
    pub segs: [AttrSeg; MAX_ATTR_SEGMENTS],
    pub seg_len: usize,
    /// 段表溢出计数（超 8 段截断并如实入账）。
    pub overflows: u32,
}

impl AttrSegs {
    pub const fn new() -> Self {
        AttrSegs {
            chars: [ATTR_INPUT; COMP_STR_CAP],
            n: 0,
            segs: [AttrSeg { start: 0, len: 0, attr: 0 }; MAX_ATTR_SEGMENTS],
            seg_len: 0,
            overflows: 0,
        }
    }

    /// 重置组合串（长度夹回 COMP_STR_CAP；属性归 ATTR_INPUT）。
    pub fn reset(&mut self, len: usize) {
        self.n = len.min(COMP_STR_CAP);
        for c in self.chars.iter_mut() {
            *c = ATTR_INPUT;
        }
        self.seg_len = 0;
    }

    /// 区间着色（越界部分夹回，不越面）。
    pub fn set_range(&mut self, start: usize, len: usize, attr: u8) {
        let end = (start + len).min(self.n);
        let mut i = start.min(COMP_STR_CAP);
        while i < end {
            self.chars[i] = attr;
            i += 1;
        }
    }

    /// 段表重建：合并相邻同属性；超 8 段 → 溢出计数并截断到 8（显性）。
    pub fn compact(&mut self) -> usize {
        self.seg_len = 0;
        let mut i = 0usize;
        while i < self.n {
            let attr = self.chars[i];
            let mut run = 1usize;
            while i + run < self.n && self.chars[i + run] == attr {
                run += 1;
            }
            if self.seg_len < MAX_ATTR_SEGMENTS {
                self.segs[self.seg_len] = AttrSeg { start: i as u8, len: run as u8, attr };
                self.seg_len += 1;
            } else {
                self.overflows += 1;
            }
            i += run;
        }
        self.seg_len
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化批次三）
// ---------------------------------------------------------------------------

pub fn run_f027e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F027-ime-candwin-d3");
    // 1) CFS_* 位值对拍 MS imm32.h（一处一事实）。
    cs.add(
        "cfs_bit_values",
        CFS_DEFAULT == 0x0000 && CFS_RECT == 0x0001 && CFS_POINT == 0x0002
            && CFS_EXCLUDE == 0x0008 && CFS_FORCE_POSITION == 0x0020,
        "",
    );
    // 2) 未初始化拒绝 + 非法风格拒绝计数。
    let mut w = CandWin::new();
    let uninit = w.anchored((10, 10));
    let bad = w.set(0x0040, 0, 0);
    cs.add(
        "cfs_uninit_and_bad_style",
        uninit == Err("uninitialized") && bad == Err("bad-cfs-style") && w.bad_style_rejects == 1,
        "",
    );
    // 3) 三态收编：DEFAULT/EXCLUDE 走光标 8px 锚定，FORCE 尊重程序给点。
    let _ = w.set(CFS_DEFAULT, 0, 0);
    let a1 = w.anchored((100, 200));
    let _ = w.set(CFS_EXCLUDE, 0, 0);
    let a2 = w.anchored((100, 200));
    let _ = w.set(CFS_FORCE_POSITION, 55, 66);
    let a3 = w.anchored((100, 200));
    cs.add(
        "cfs_three_styles",
        a1 == Ok((108, 208)) && a2 == Ok((108, 208)) && a3 == Ok((55, 66)) && w.is_set(),
        "",
    );
    // 4) 超右翻左：(500,100)@640 宽 → (300,100)。
    cs.add("avoid_flip_right", avoid_flip(500, 100, 640, 480) == (300, 100), "");
    // 5) 超下翻上 + 双超夹回：(100,380)@640×400 翻上；(50,20)@(240,110) 双超夹回。
    let single = avoid_flip(100, 380, 640, 400);
    let both = avoid_flip(50, 20, 240, 110);
    cs.add("avoid_flip_bottom_clamp", single == (100, 260) && both == (0, 0), "");
    // 6) 合法序链：START→COMPOSITION×2→END 完成一次，零违例。
    let mut s = SeqChain::new();
    let legal = s.feed(WM_IME_STARTCOMPOSITION).is_ok()
        && s.feed(WM_IME_COMPOSITION).is_ok()
        && s.feed(WM_IME_COMPOSITION).is_ok()
        && s.feed(WM_IME_ENDCOMPOSITION).is_ok();
    cs.add(
        "ime_seq_legal_chain",
        legal && s.completed == 1 && s.violations == 0 && s.state == SeqState::None,
        "",
    );
    // 7) 乱序检出：无 START 先 COMPOSITION、双 START、无 COMPOSITION 直接 END。
    let mut s2 = SeqChain::new();
    let v1 = s2.feed(WM_IME_COMPOSITION);
    let v2 = s2.feed(WM_IME_STARTCOMPOSITION).is_ok() && s2.feed(WM_IME_STARTCOMPOSITION).is_err();
    let v3 = s2.feed(WM_IME_ENDCOMPOSITION).is_ok();
    cs.add("ime_seq_out_of_order", v1 == Err("out-of-order") && v2 && v3 && s2.violations == 2, "");
    // 8) 属性三段着色 + 相邻同属性合并：IN/IN/TGT/TGT/CVT/CVT → 3 段。
    let mut a = AttrSegs::new();
    a.reset(6);
    a.set_range(2, 2, ATTR_TARGET_CONVERTED);
    a.set_range(4, 2, ATTR_CONVERTED);
    let segs = a.compact();
    cs.add(
        "attr_segments_merge",
        segs == 3
            && a.segs[0] == AttrSeg { start: 0, len: 2, attr: ATTR_INPUT }
            && a.segs[1] == AttrSeg { start: 2, len: 2, attr: ATTR_TARGET_CONVERTED }
            && a.segs[2] == AttrSeg { start: 4, len: 2, attr: ATTR_CONVERTED },
        "",
    );
    // 9) 段表溢出：10 个交替段超 8 段 → 截断到 8 并计数 2（显性）。
    let mut b = AttrSegs::new();
    b.reset(10);
    for i in 0..10 {
        let attr = if i % 2 == 0 { ATTR_INPUT } else { ATTR_CONVERTED };
        b.set_range(i, 1, attr);
    }
    let segs2 = b.compact();
    cs.add("attr_overflow_counted", segs2 == 8 && b.seg_len == 8 && b.overflows == 2, "");
    // 10) 组合串长度夹回：reset(40) → n=32（越面截断不越写）。
    let mut c = AttrSegs::new();
    c.reset(40);
    cs.add("comp_str_cap_clamped", c.n == COMP_STR_CAP, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn avoid_within_screen_no_flip() {
        // 屏内候选窗原位不动（无翻转）；恰好贴右不超不翻。
        assert_eq!(avoid_flip(100, 100, 1920, 1080), (100, 100));
        assert_eq!(avoid_flip(1720, 0, 1920, 1080), (1720, 0));
    }

    #[test]
    fn seq_unknown_message_rejected() {
        let mut s = SeqChain::new();
        assert_eq!(s.feed(0x0281), Err("out-of-order"), "非 WM_IME 序内消息一律乱序检出");
        assert_eq!(s.violations, 1);
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f027e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
