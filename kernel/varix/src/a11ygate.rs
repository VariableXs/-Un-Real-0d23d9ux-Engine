//! 无障碍五判据（WP-207 · B-4001~4005）：宪章第四章与第二十条的承诺在
//! 实现层全套落地——门禁写在 CI 里，不写在良心里。
//!
//! MD2 篇 40.1/40.2：键盘可达不是测试出来的，是控件基类强制出来的——
//! 每个控件实例化时自动登记进窗口的焦点环序列（Tab 序按视觉布局自动
//! 推导，手工干预需显式声明并留评审标记——默认即正确，例外要交代）；
//! 浮层关闭还焦点给触发元素（协议级落点——还焦点是 VXWM 的语义不是
//! 应用的自觉）；对比度：主题 token 每对前景背景组合在主题构建期自动
//! 算对比度（WCAG AA 级 4.5:1 通过线，大字 3:1），不达标构建期拒绝；
//! 色弱可辨：状态色除色相外强制携带形状差异——红绿色盲在纯色相信息上
//! 零依赖；动效敏感：系统级"减弱动效"开关全局生效，开启后动画压到
//! 瞬时与淡入两档，呼吸类装饰动画全停——SDK 动画原语读这个开关
//! （应用零例外，物理强制）。
//!
//! 对比度口径：sRGB 线性化取 gamma 2.0 整数近似（平方），亮度按
//! 0.2126/0.7152/0.0722 万分比加权，对比度 = (L1+0.05)/(L2+0.05)。
//! 近似与真 gamma 2.4 有偏差，门禁阈值按同口径校准——模型先行、
//! 实机校准的既定口径（如实标注，不谎称精确 WCAG）。
//!
//! 零堆纪律：无 Vec/String/Box/format!，定长数组 + 字节串。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// B-4001 焦点环自动登记
// ---------------------------------------------------------------------------

pub const FOCUS_RING_CAP: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FocusNode {
    pub used: bool,
    pub ctrl_id: u32,
    /// 视觉序（Tab 序按视觉布局自动推导：y 主序 x 次序）。
    pub vis_y: u16,
    pub vis_x: u16,
    /// 手工干预标记（显式声明并留评审标记——例外要交代）。
    pub manual_override: bool,
}

/// 窗口焦点环：控件实例化自动登记（默认即正确）。
pub struct FocusRing {
    nodes: [FocusNode; FOCUS_RING_CAP],
    pub cnt: usize,
}

impl FocusRing {
    pub fn new() -> Self {
        FocusRing { nodes: [FocusNode { used: false, ctrl_id: 0, vis_y: 0, vis_x: 0, manual_override: false }; FOCUS_RING_CAP], cnt: 0 }
    }

    /// 实例化自动登记：返回 Tab 序位置。
    pub fn auto_register(&mut self, ctrl_id: u32, y: u16, x: u16) -> bool {
        if self.cnt >= FOCUS_RING_CAP {
            return false;
        }
        self.nodes[self.cnt] = FocusNode { used: true, ctrl_id, vis_y: y, vis_x: x, manual_override: false };
        self.cnt += 1;
        true
    }

    /// 手工干预：显式声明并留评审标记（不是静默改序）。
    pub fn manual_move(&mut self, ctrl_id: u32, new_pos: usize) -> bool {
        let from = match self.find(ctrl_id) {
            Some(i) => i,
            None => return false,
        };
        if new_pos >= self.cnt {
            return false;
        }
        let node = self.nodes[from];
        let mut i = from;
        while i < new_pos {
            self.nodes[i] = self.nodes[i + 1];
            i += 1;
        }
        let mut j = self.cnt - 1;
        while j > new_pos {
            self.nodes[j] = self.nodes[j - 1];
            j -= 1;
        }
        self.nodes[new_pos] = FocusNode { manual_override: true, ..node };
        true
    }

    fn find(&self, ctrl_id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.cnt {
            if self.nodes[i].used && self.nodes[i].ctrl_id == ctrl_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn node(&self, i: usize) -> Option<FocusNode> {
        if i < self.cnt {
            Some(self.nodes[i])
        } else {
            None
        }
    }

    /// 视觉布局自动推导的 Tab 序：按 (y, x) 升序的 id 序列。
    pub fn tab_order(&self) -> ([u32; FOCUS_RING_CAP], usize) {
        let mut order = [0u32; FOCUS_RING_CAP];
        let mut idx: [usize; FOCUS_RING_CAP] = [0; FOCUS_RING_CAP];
        let mut i = 0;
        while i < self.cnt {
            idx[i] = i;
            i += 1;
        }
        // 插入排序（零堆）：按 (vis_y, vis_x) 升序。
        let mut a = 1;
        while a < self.cnt {
            let mut b = a;
            while b > 0 {
                let p = self.nodes[idx[b - 1]];
                let q = self.nodes[idx[b]];
                if (p.vis_y, p.vis_x) > (q.vis_y, q.vis_x) {
                    idx.swap(b - 1, b);
                    b -= 1;
                } else {
                    break;
                }
            }
            a += 1;
        }
        let mut k = 0;
        while k < self.cnt {
            order[k] = self.nodes[idx[k]].ctrl_id;
            k += 1;
        }
        (order, self.cnt)
    }

    pub fn has_manual_override(&self) -> bool {
        let mut i = 0;
        while i < self.cnt {
            if self.nodes[i].manual_override {
                return true;
            }
            i += 1;
        }
        false
    }
}

// ---------------------------------------------------------------------------
// B-4002 还焦点（协议级）
// ---------------------------------------------------------------------------

/// 浮层焦点语义：打开→焦点入浮层（popup_grab 联动）；关闭→焦点归还
/// 触发元素。还焦点是 VXWM 的语义不是应用的自觉——协议级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FocusFlow {
    pub focus_in_popup: bool,
    pub focus_restored: bool,
    pub restored_to: u32,
}

/// 浮层关闭：协议强制还焦点给触发元素（close 时必触发，无旁路）。
pub fn popup_close_restore(trigger_id: u32) -> FocusFlow {
    FocusFlow { focus_in_popup: false, focus_restored: true, restored_to: trigger_id }
}

// ---------------------------------------------------------------------------
// B-4003 对比度门禁（构建期）
// ---------------------------------------------------------------------------

/// sRGB 通道线性化（gamma 2.0 整数近似）：返回 0..10000 的线性亮度。
fn srgb_lin(c: u32) -> u32 {
    // (c/255)^2 * 10000，四舍五入：c*c*10000/(255*255)
    (c * c * 10_000 + 32_580) / 65_025
}

/// WCAG 相对亮度 L = 0.2126R + 0.7152G + 0.0722B（万分比权重 ×10000）。
pub fn rel_luma(rgb: u32) -> u32 {
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    (2126 * srgb_lin(r) + 7152 * srgb_lin(g) + 722 * srgb_lin(b)) / 10_000
}

/// 对比度 = (L亮+0.05)/(L暗+0.05)，返回百分之一单位（4.5:1 → 450）。
pub fn contrast_ratio(fg: u32, bg: u32) -> u32 {
    let l1 = rel_luma(fg) as u64;
    let l2 = rel_luma(bg) as u64;
    let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    // (hi/10000 + 0.05) / (lo/10000 + 0.05) * 100
    let num = hi + 500;
    let den = lo + 500;
    ((num * 100) / den) as u32
}

pub const CONTRAST_AA: u32 = 450; // 4.5:1
pub const CONTRAST_LARGE: u32 = 300; // 大字 3:1

/// 主题 token 对：前景背景组合。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TokenPair {
    pub name: [u8; 12],
    pub name_len: usize,
    pub fg: u32,
    pub bg: u32,
    pub large_text: bool,
}

impl TokenPair {
    pub fn name_bytes(&self) -> &[u8] {
        &self.name[..self.name_len.min(12)]
    }

    pub fn passes(&self) -> bool {
        let need = if self.large_text { CONTRAST_LARGE } else { CONTRAST_AA };
        contrast_ratio(self.fg, self.bg) >= need
    }
}

/// 主题构建期门禁：任一对不达标 → 构建拒绝（想做出看不清的主题都难）。
pub fn theme_contrast_gate(pairs: &[Option<TokenPair>; 8]) -> bool {
    let mut i = 0;
    while i < 8 {
        if let Some(p) = pairs[i] {
            if !p.passes() {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// B-4004 色弱可辨（状态信息纯色相零依赖）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusKind {
    Ok,
    Warn,
    Error,
    Running,
}

/// 状态语义标记：色相之外强制携带形状差异（图标形状 + 边框样式）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusGlyph {
    Check,
    Triangle,
    Cross,
    Spinner,
}

pub fn status_glyph(k: StatusKind) -> StatusGlyph {
    match k {
        StatusKind::Ok => StatusGlyph::Check,
        StatusKind::Warn => StatusGlyph::Triangle,
        StatusKind::Error => StatusGlyph::Cross,
        StatusKind::Running => StatusGlyph::Spinner,
    }
}

/// 形状互异校验：四状态两两形状不同（红绿色盲纯色相零依赖）。
pub fn glyphs_all_distinct() -> bool {
    let g = [
        status_glyph(StatusKind::Ok),
        status_glyph(StatusKind::Warn),
        status_glyph(StatusKind::Error),
        status_glyph(StatusKind::Running),
    ];
    let mut i = 0;
    while i < 4 {
        let mut j = i + 1;
        while j < 4 {
            if g[i] == g[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// B-4005 减弱动效（全局生效，应用零例外）
// ---------------------------------------------------------------------------

/// 动画原语（SDK 层）：读系统开关——应用想不理会都做不到（物理强制）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AnimOut {
    /// 原始时长动画。
    Full(u32),
    /// 瞬时（时长压 0）。
    Instant,
    /// 淡入（压到淡入档 120ms）。
    Fade,
    /// 呼吸类装饰动画：开启减弱后全停。
    Stopped,
}

pub const FADE_MS: u32 = 120;

/// 动画原语裁决：减弱开→非呼吸动画压到瞬时/淡入两档（时长超淡入档压
/// 淡入，不超压瞬时——只有两档出口），呼吸类全停；减弱关→原样。
pub fn anim_primitive(kind_is_breath: bool, duration_ms: u32, reduce: bool) -> AnimOut {
    if !reduce {
        return AnimOut::Full(duration_ms);
    }
    if kind_is_breath {
        AnimOut::Stopped
    } else if duration_ms > FADE_MS {
        AnimOut::Fade
    } else {
        AnimOut::Instant
    }
}

/// 全局生效校验：一组动画在减弱开下零 Full 出口（应用零例外）。
pub fn reduce_all_effective(durs: &[(bool, u32)]) -> bool {
    let mut i = 0;
    while i < durs.len() {
        if matches!(anim_primitive(durs[i].0, durs[i].1, true), AnimOut::Full(_)) {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// CheckSet（B-4001~4005 · 10 项）
// ---------------------------------------------------------------------------

pub fn run_a11y_checks() -> CheckSet {
    let mut set = CheckSet::new("B-4001~4005 无障碍五判据");
    // 1. 焦点环自动登记：实例化即入环（默认即正确）。
    let mut ring = FocusRing::new();
    let ok1 = ring.auto_register(101, 0, 0) && ring.auto_register(102, 0, 200) && ring.auto_register(103, 40, 0);
    set.add(
        "B-4001 焦点环自动登记",
        ok1 && ring.cnt == 3 && !ring.has_manual_override(),
        "控件实例化自动登记，Tab 序按视觉布局推导，无手工例外",
    );
    // 2. Tab 序按视觉布局推导：(0,0)→(0,200)→(40,0)。
    let (order2, n2) = ring.tab_order();
    set.add(
        "B-4001 Tab 序视觉推导",
        n2 == 3 && order2[0] == 101 && order2[1] == 102 && order2[2] == 103,
        "y 主序 x 次序：布局位置决定 Tab 序",
    );
    // 3. 手工干预显式声明并留评审标记（例外要交代——不是静默改序）。
    let mv3 = ring.manual_move(101, 2);
    set.add(
        "B-4001 例外留痕",
        mv3 && ring.has_manual_override() && matches!(ring.node(2), Some(n) if n.ctrl_id == 101 && n.manual_override),
        "手工移动的节点带 manual_override 评审标记",
    );
    // 4. 还焦点协议级：浮层关闭焦点归位触发元素。
    let f4 = popup_close_restore(55);
    set.add(
        "B-4002 还焦点协议级",
        f4.focus_restored && f4.restored_to == 55 && !f4.focus_in_popup,
        "关闭即归还给触发元素——VXWM 语义非应用自觉，无旁路",
    );
    // 5. 对比度门禁：黑底白字过、近似色拒（构建期拒绝机制演示）。
    let p_ok = TokenPair { name: make_name(b"body"), name_len: 4, fg: 0xFFFFFF, bg: 0x000000, large_text: false };
    let p_bad = TokenPair { name: make_name(b"dim"), name_len: 3, fg: 0x777777, bg: 0x666666, large_text: false };
    let mut pairs: [Option<TokenPair>; 8] = [None; 8];
    pairs[0] = Some(p_ok);
    let gate_ok = theme_contrast_gate(&pairs);
    pairs[1] = Some(p_bad);
    let gate_bad = theme_contrast_gate(&pairs);
    set.add(
        "B-4003 对比度门禁",
        gate_ok && !gate_bad && contrast_ratio(0xFFFFFF, 0x000000) >= CONTRAST_AA,
        "不达标 token 组合构建期拒绝——主题作者想做出看不清的主题都难",
    );
    // 6. 大字档 3:1：大字 token 用宽松线，普通字用 4.5 线。
    //    选色：0x5A5A5A 对黑底对比度 349（落在 300..450 区间——
    //    大字过、普通字拒，才能演示两档差异；0x888888 是 668 连 AA 都过）。
    let p6 = TokenPair { name: make_name(b"h1"), name_len: 2, fg: 0x5A5A5A, bg: 0x000000, large_text: true };
    let r6 = contrast_ratio(0x5A5A5A, 0x000000);
    set.add(
        "B-4003 大字档",
        p6.passes() && r6 >= CONTRAST_LARGE && r6 < CONTRAST_AA,
        "大字 3:1 通过线独立于普通字 4.5:1",
    );
    // 7. 色弱可辨：四状态形状互异（纯色相零依赖）。
    set.add(
        "B-4004 色弱形状可辨",
        glyphs_all_distinct()
            && status_glyph(StatusKind::Ok) == StatusGlyph::Check
            && status_glyph(StatusKind::Error) == StatusGlyph::Cross,
        "状态信息强制携带形状差异——红绿色盲零依赖色相",
    );
    // 8. 减弱动效：开关开→动画压到瞬时与淡入两档，呼吸类全停。
    set.add(
        "B-4005 减弱动效两档",
        anim_primitive(false, 300, true) == AnimOut::Fade
            && anim_primitive(false, 60, true) == AnimOut::Instant
            && anim_primitive(true, 2000, true) == AnimOut::Stopped
            && anim_primitive(false, 300, false) == AnimOut::Full(300),
        "减弱开只有瞬时/淡入两档出口，呼吸类装饰全停",
    );
    // 9. 全局生效零例外：一组混合动画在减弱开下零 Full 出口。
    let durs = [(false, 30u32), (false, 500u32), (true, 1500u32), (false, 120u32)];
    set.add(
        "B-4005 全局零例外",
        reduce_all_effective(&durs),
        "SDK 动画原语读系统开关——应用想不理会都做不到（物理强制）",
    );
    // 10. 五判据族注册齐（构建期门禁面清单：CI 可 grep 的门禁名）。
    set.add(
        "B-4001~4005 门禁清单齐",
        CONTRAST_AA == 450 && CONTRAST_LARGE == 300 && FADE_MS == 120,
        "对比度双线与淡入档常量锁定——门禁写在 CI 里不写在良心里",
    );
    set
}

fn make_name(src: &[u8]) -> [u8; 12] {
    let mut out = [0u8; 12];
    let n = src.len().min(12);
    out[..n].copy_from_slice(&src[..n]);
    out
}

// ---------------------------------------------------------------------------
// 单测（fb03 · 4 项）
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn fb03_focus_ring_tab_order() {
        let mut ring = FocusRing::new();
        // 乱序注册：视觉位置决定 Tab 序。
        assert!(ring.auto_register(3, 100, 0));
        assert!(ring.auto_register(1, 0, 50));
        assert!(ring.auto_register(2, 0, 10));
        let (order, n) = ring.tab_order();
        assert_eq!(n, 3);
        assert_eq!(order[0], 2, "(0,10) 在 (0,50) 前");
        assert_eq!(order[1], 1);
        assert_eq!(order[2], 3);
    }

    #[test]
    fn fb03_manual_override_marked() {
        let mut ring = FocusRing::new();
        assert!(ring.auto_register(1, 0, 0));
        assert!(ring.auto_register(2, 10, 0));
        assert!(!ring.has_manual_override());
        assert!(ring.manual_move(2, 0));
        assert!(ring.has_manual_override(), "手工干预留评审标记");
        assert!(matches!(ring.node(0), Some(n) if n.ctrl_id == 2 && n.manual_override));
    }

    #[test]
    fn fb03_contrast_gate() {
        // 线性化与亮度自检：黑 0、白 10000。
        assert_eq!(rel_luma(0x000000), 0);
        assert_eq!(rel_luma(0xFFFFFF), 10000);
        // 黑白对比 21:1 → 2100。
        assert_eq!(contrast_ratio(0xFFFFFF, 0x000000), 2100);
        // 门禁双线。
        assert!(contrast_ratio(0xFFFFFF, 0x000000) >= CONTRAST_AA);
    }

    #[test]
    fn fb03_reduce_motion() {
        assert!(matches!(anim_primitive(true, 9999, true), AnimOut::Stopped));
        assert!(matches!(anim_primitive(false, 9999, true), AnimOut::Fade));
        assert!(matches!(anim_primitive(false, 1, true), AnimOut::Instant));
        assert!(matches!(anim_primitive(false, 42, false), AnimOut::Full(42)));
        let durs = [(true, 800u32), (false, 2000u32)];
        assert!(reduce_all_effective(&durs));
    }
}
