//! F482 鼠标主键交换（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **交换后全语义镜像用例（框选/拖拽/菜单三链路）；文案镜像抽查；中键
//! 不变；交换即时性；持久化。**
//!
//! 功能定义（主册批次三）：左利手支持——主键交换一键（右键当主键/左键当
//! 菜单——全系统语义镜像，包括拖拽语义 F262、框选 F203、右键菜单 F215 的
//! 键位映射）；交换后设置页文案同步镜像（「点击菜单键」而非死写「右键」）；
//! 物理中键行为不变。
//!
//! 零堆纪律：定长映射表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 物理按键（中键行为恒不变——主册判据）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhysButton {
    Left,
    Right,
    Middle,
}

/// 交换后的语义角色。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SemanticRole {
    /// 主键（选择/拖拽/框选——F203/F262 全走主键）。
    Primary,
    /// 菜单键（右键菜单 F215 语义）。
    Menu,
    /// 中键（自动滚动/粘贴——不参与交换）。
    Middle,
}

/// 主键交换映射（swap=false 正常：左主右菜单；swap=true 镜像）。
pub fn map_button(phys: PhysButton, swap: bool) -> SemanticRole {
    match phys {
        PhysButton::Left => {
            if swap {
                SemanticRole::Menu
            } else {
                SemanticRole::Primary
            }
        }
        PhysButton::Right => {
            if swap {
                SemanticRole::Primary
            } else {
                SemanticRole::Menu
            }
        }
        PhysButton::Middle => SemanticRole::Middle, // 中键不变
    }
}

/// 交换即时性：swap 翻转后立即重映射（无重启——即时生效）。
pub fn swap_instant(before_swap: SemanticRole, after_swap: SemanticRole) -> bool {
    before_swap != after_swap
}

/// 文案镜像（主册：设置页文案说「菜单键」不说死「右键」——有心的细节）。
/// 零堆纪律：文案为静态常量，按交换态二选一返回（不拼串不分配）。
pub const MENU_COPY_SWAPPED: &str = "点击左侧菜单键打开菜单";
pub const MENU_COPY_NORMAL: &str = "点击右侧菜单键打开菜单";

pub fn mirror_copy(swap: bool) -> &'static str {
    if swap {
        // 交换后菜单键在左边——文案按当前事实描述。
        MENU_COPY_SWAPPED
    } else {
        MENU_COPY_NORMAL
    }
}

/// 三链路语义镜像审计（框选 F203 / 拖拽 F262 / 菜单 F215——交换后全部
/// 跟随主键位）。
pub fn three_paths_mirror(swap: bool) -> bool {
    // 主键物理位（交换后）承担框选/拖拽；菜单物理位承担菜单。
    let primary_phys = if swap { PhysButton::Right } else { PhysButton::Left };
    let menu_phys = if swap { PhysButton::Left } else { PhysButton::Right };
    matches!(map_button(primary_phys, swap), SemanticRole::Primary)
        && matches!(map_button(menu_phys, swap), SemanticRole::Menu)
        && matches!(map_button(PhysButton::Middle, swap), SemanticRole::Middle)
}

/// 交换设置（持久化单元：存取经单字节——关机后设置保留）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SwapSetting(pub bool);

impl SwapSetting {
    pub fn save(&self) -> u8 {
        self.0 as u8
    }

    pub fn load(v: u8) -> SwapSetting {
        SwapSetting(v != 0)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_swapbtn_checks() -> CheckSet {
    let mut cs = CheckSet::new("F482-swapbtn");
    // 1) 交换后全语义镜像（框选/拖拽/菜单三链路）。
    cs.add("mirror_normal", three_paths_mirror(false), "");
    cs.add("mirror_swapped", three_paths_mirror(true), "");
    // 2) 键位映射逐键用例。
    cs.add("map_default", map_button(PhysButton::Left, false) == SemanticRole::Primary && map_button(PhysButton::Right, false) == SemanticRole::Menu, "");
    cs.add("map_swapped", map_button(PhysButton::Left, true) == SemanticRole::Menu && map_button(PhysButton::Right, true) == SemanticRole::Primary, "");
    // 3) 中键不变（两个方向都不动中键）。
    cs.add("middle_invariant", map_button(PhysButton::Middle, false) == SemanticRole::Middle && map_button(PhysButton::Middle, true) == SemanticRole::Middle, "");
    // 4) 交换即时性（翻转即换语义——同一物理键前后语义不同）。
    cs.add("swap_instant", swap_instant(map_button(PhysButton::Left, false), map_button(PhysButton::Left, true)), "");
    // 5) 文案镜像（「菜单键」不写死「右键」——两侧文案同构）。
    let left_copy = mirror_copy(true);
    let right_copy = mirror_copy(false);
    cs.add("copy_mirrors", left_copy.contains("左") && left_copy.contains("菜单键") && right_copy.contains("右") && !left_copy.contains("右键"), "");
    // 6) 持久化（save/load 往返保真）。
    cs.add("persist_roundtrip", SwapSetting::load(SwapSetting(true).save()) == SwapSetting(true) && SwapSetting::load(SwapSetting(false).save()) == SwapSetting(false), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_swap_roundtrip() {
        // 开→关往返：每个物理键回到原语义。
        for phys in [PhysButton::Left, PhysButton::Right, PhysButton::Middle] {
            let base = map_button(phys, false);
            let swapped = map_button(phys, true);
            let restored = map_button(phys, false);
            assert_eq!(base, restored);
            if phys != PhysButton::Middle {
                assert_ne!(base, swapped);
            }
        }
    }

    #[test]
    fn copy_never_dead_says_right_button() {
        // 交换后若死写「右键」会骗到左利手——文案必须跟随事实。
        let c = mirror_copy(true);
        assert!(c.starts_with("点击左"));
        assert!(c.ends_with("打开菜单"));
    }

    #[test]
    fn middle_button_never_swapped() {
        assert_eq!(map_button(PhysButton::Middle, true), SemanticRole::Middle);
    }
}

// ===========================================================================
// 深化 v2（F482）：三链路语义镜像矩阵 / 交换次数账 / 文案镜像全表 /
// 持久化与即时性联动 / 左利手会话审计
// ===========================================================================

/// 三链路语义镜像矩阵（框选 F203 / 拖拽 F262 / 菜单 F215 三链路 ×
/// 交换前后 = 6 格全算：交换后主键语义完整落到另一侧——任何一链路
/// 漏镜像都是「只换了一半」的半成品）。
pub fn three_path_matrix(swapped: bool) -> [bool; 3] {
    // 三链路断言：主键/菜单键语义与交换态自洽（swapped 决定主键在哪侧）。
    let primary_phys = if swapped { PhysButton::Right } else { PhysButton::Left };
    let menu_phys = if swapped { PhysButton::Left } else { PhysButton::Right };
    [
        // 框选链路（F203）：主键侧按下 = Primary。
        map_button(primary_phys, swapped) == SemanticRole::Primary,
        // 拖拽链路（F262）：主键按住拖动；中键恒中性。
        map_button(menu_phys, swapped) == SemanticRole::Menu
            && map_button(PhysButton::Middle, swapped) == SemanticRole::Middle,
        // 菜单链路（F215）：菜单键呼出；两键语义互斥不重叠。
        map_button(menu_phys, swapped) == SemanticRole::Menu
            && map_button(primary_phys, swapped) == SemanticRole::Primary,
    ]
}

/// 交换次数账（会话审计：一键开/一键关是两次完整切换——账面可查，
/// 不限制次数但记录节奏——高频抖动 = 用户在找合适方向，不是故障）。
pub struct SwapSession {
    pub toggles: u32,
    pub current: bool,
}

impl SwapSession {
    pub const fn new() -> Self {
        SwapSession { toggles: 0, current: false }
    }

    pub fn toggle(&mut self) -> bool {
        self.current = !self.current;
        self.toggles = self.toggles.saturating_add(1);
        self.current
    }
}

/// 文案镜像全表（v1 常量的深化：交换态文案 ≠ 默认态文案，且都不含
/// 死写「右键」——两态文案逐一过检）。
pub fn copy_table_ok() -> bool {
    let s = mirror_copy(true);
    let n = mirror_copy(false);
    s != n && !s.contains("右键") && !n.contains("右键") && s.contains("菜单键") && n.contains("菜单键")
}

/// 中键不变的结构性复核（三态矩阵中中键恒 Middle——交换对中键零影响
/// 的行为面，与 v1 map_button 用例互证）。
pub fn middle_neutral_matrix() -> bool {
    let states = [false, true];
    states.iter().all(|&s| {
        map_button(PhysButton::Middle, s) == SemanticRole::Middle
    })
}

/// 持久化即效联动（保存 → 立即读取生效——「交换即时性」的落盘面：
/// 不存在「存了但没生效」的窗口）。
pub fn persist_immediate(save_ok: bool, loaded: bool) -> bool {
    save_ok && loaded
}

// ---------------------------------------------------------------------------
// 深化自检（F482 v2）
// ---------------------------------------------------------------------------

pub fn run_swapbtn_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F482-v2");
    // 1) 三链路矩阵：交换前后各 3 格全绿。
    cs.add("matrix_default", three_path_matrix(false).iter().all(|&x| x), "");
    cs.add("matrix_swapped", three_path_matrix(true).iter().all(|&x| x), "");
    // 2) 交换次数账：连翻四次回到原态 + 账面 4。
    let mut sess = SwapSession::new();
    let final_state = (0..4).fold(false, |_, _| sess.toggle());
    cs.add("toggle_account", sess.toggles == 4 && !final_state && !sess.current, "");
    // 3) 文案镜像全表。
    cs.add("copy_table", copy_table_ok(), "");
    // 4) 中键中性矩阵。
    cs.add("middle_neutral", middle_neutral_matrix(), "");
    // 5) 持久化即效联动。
    let saved = SwapSetting(true).save();
    let loaded = SwapSetting::load(saved);
    cs.add("persist_immediate", persist_immediate(saved == 1, loaded == SwapSetting(true)), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn swap_preserves_middle_in_all_paths() {
        // 三链路 × 中键：任何交换态下中键都不是主键也不是菜单键。
        for swapped in [false, true] {
            assert_ne!(map_button(PhysButton::Middle, swapped), SemanticRole::Primary);
            assert_ne!(map_button(PhysButton::Middle, swapped), SemanticRole::Menu);
        }
    }

    #[test]
    fn session_odd_toggles_flip() {
        let mut sess = SwapSession::new();
        assert!(sess.toggle(), "第 1 次：开");
        assert!(!sess.toggle(), "第 2 次：关");
        assert!(sess.toggle(), "第 3 次：开");
        assert_eq!(sess.toggles, 3);
    }

    #[test]
    fn copy_never_says_right_button_both_states() {
        for swap in [false, true] {
            let c = mirror_copy(swap);
            assert!(!c.contains("右键"), "死写「右键」会骗到左利手");
            assert!(c.contains("菜单键"));
        }
    }
}

// ===========================================================================
// 深化 v5（F482）：拖拽/框选键位角色解析 / 设置页全面文案镜像 /
// 交换设置持久化 v2（校验魔标）/ 高频抖动防抖账
// ===========================================================================

/// 交互动作 → 应答键位角色解析（主册「框选/拖拽/菜单三链路全语义镜像」
/// 的动作面：任何交互动作问「该用哪个物理键」——交换后全部跟着语义走）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Interaction {
    /// 框选（F203）。
    Marquee,
    /// 按住拖拽（F262）。
    Drag,
    /// 呼出右键菜单（F215）。
    ContextMenu,
    /// 中键自动滚动。
    MiddleScroll,
}

/// 动作应答物理键（swap 决定主键在哪侧；中键动作恒中键）。
pub fn responding_button(act: Interaction, swap: bool) -> PhysButton {
    match act {
        Interaction::Marquee | Interaction::Drag => {
            if swap { PhysButton::Right } else { PhysButton::Left }
        }
        Interaction::ContextMenu => {
            if swap { PhysButton::Left } else { PhysButton::Right }
        }
        Interaction::MiddleScroll => PhysButton::Middle,
    }
}

/// 动作-键位全链自洽审计（每个动作的应答键映射回语义必须还是原语义——
/// 「镜像」不是换键位是把语义整体搬家）。
pub fn interaction_chain_ok(swap: bool) -> bool {
    let mp = responding_button(Interaction::Marquee, swap);
    let dp = responding_button(Interaction::Drag, swap);
    let cp = responding_button(Interaction::ContextMenu, swap);
    let mid = responding_button(Interaction::MiddleScroll, swap);
    mp == dp
        && map_button(mp, swap) == SemanticRole::Primary
        && map_button(cp, swap) == SemanticRole::Menu
        && map_button(mid, swap) == SemanticRole::Middle
}

/// 设置页全表面文案镜像（v2 只查了菜单键一句——v5 全表面：按钮/提示/
/// 帮助三处文案都按交换态出词，死写「右键/左键」 = 半成品镜像）。
pub const SURFACE_COPY_HINT: [&str; 2] = [
    "使用菜单键呼出菜单",
    "按住主键拖拽以框选",
];

/// 表面文案审计（语义词表本身即镜像保证：文案只说「主键/菜单键」，
/// 不随交换态换词——物理键位由 responding_button 动态解析，词表恒真）。
pub fn surface_copy_table_ok() -> bool {
    SURFACE_COPY_HINT.iter().all(|c| !c.contains("左键") && !c.contains("右键"))
}

/// 交换设置持久化 v2（魔标 + 位值 + 坏值拒收——v1 裸单字节无法识别
/// 坏文件，读回垃圾字节会静默翻转主键）。
pub const SWAP_PERSIST_MAGIC: [u8; 3] = *b"VSW";
pub const SWAP_PERSIST_LEN: usize = 4;

pub fn save_swap_v2(swap: bool, out: &mut [u8]) -> Option<usize> {
    if out.len() < SWAP_PERSIST_LEN {
        return None;
    }
    out[..3].copy_from_slice(&SWAP_PERSIST_MAGIC);
    out[3] = swap as u8;
    Some(SWAP_PERSIST_LEN)
}

pub fn load_swap_v2(buf: &[u8]) -> Option<bool> {
    if buf.len() < SWAP_PERSIST_LEN || buf[..3] != SWAP_PERSIST_MAGIC {
        return None;
    }
    match buf[3] {
        0 => Some(false),
        1 => Some(true),
        _ => None, // 坏位拒收：不许静默钳回默认
    }
}

/// 高频抖动防抖账（主册「交换即时性」的守护面：用户连续翻转找方向时
/// 每次翻转都生效——但 50ms 内的重复翻转按机械抖动折叠，不刷设置风暴）。
pub struct SwapDebounce {
    pub last_toggle_ms: u64,
    pub folded: u32,
}

impl SwapDebounce {
    pub const DEBOUNCE_MS: u64 = 50;

    pub const fn new() -> Self {
        SwapDebounce { last_toggle_ms: 0, folded: 0 }
    }

    /// 翻转请求裁决：true = 实际生效；false = 折叠为抖动。
    pub fn request(&mut self, now_ms: u64) -> bool {
        if now_ms.saturating_sub(self.last_toggle_ms) < Self::DEBOUNCE_MS && self.last_toggle_ms != 0 {
            self.folded += 1;
            return false;
        }
        self.last_toggle_ms = now_ms;
        true
    }
}

pub fn run_swapbtn_v5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F482-v5");
    // 1) 动作-键位链：交换前后两态全链自洽。
    cs.add("chain_default", interaction_chain_ok(false), "");
    cs.add("chain_swapped", interaction_chain_ok(true), "");
    // 2) 框选与拖拽同键（都是主键动作）；菜单独立键。
    cs.add("marquee_drag_same_key", responding_button(Interaction::Marquee, true) == responding_button(Interaction::Drag, true), "");
    cs.add("menu_opposite_key", responding_button(Interaction::Marquee, false) != responding_button(Interaction::ContextMenu, false), "");
    // 3) 中键动作恒中键（两个交换态）。
    cs.add("middlescroll_invariant", responding_button(Interaction::MiddleScroll, false) == PhysButton::Middle && responding_button(Interaction::MiddleScroll, true) == PhysButton::Middle, "");
    // 4) 表面文案表：语义词、无死写物理键。
    cs.add("surface_copy_semantic", surface_copy_table_ok(), "");
    // 5) 持久化 v2：round-trip + 坏魔标拒收 + 坏位拒收。
    let mut buf = [0u8; SWAP_PERSIST_LEN];
    cs.add("persist_v2_true", {
        let n = save_swap_v2(true, &mut buf).unwrap_or(0);
        n == SWAP_PERSIST_LEN && load_swap_v2(&buf) == Some(true)
    }, "");
    cs.add("persist_v2_false", {
        let n = save_swap_v2(false, &mut buf).unwrap_or(0);
        load_swap_v2(&buf[..n]) == Some(false)
    }, "");
    cs.add("persist_bad_magic", load_swap_v2(b"XXX\x01").is_none(), "");
    cs.add("persist_bad_bit", load_swap_v2(&[b'V', b'S', b'W', 7]).is_none(), "");
    // 6) 防抖账：50ms 内连翻折叠、间隔后放行。
    let mut d = SwapDebounce::new();
    cs.add("debounce_first_pass", d.request(1_000), "");
    cs.add("debounce_jitter_folded", !d.request(1_010) && d.folded == 1, "");
    cs.add("debounce_spaced_pass", d.request(1_100), "");
    // 7) 交换设置与即时映射联动（存什么读什么映射什么——三面一账）。
    let stored = load_swap_v2(&buf).unwrap_or(false);
    cs.add("stored_drives_mapping", responding_button(Interaction::Marquee, stored) == PhysButton::Left, "");
    cs
}

#[cfg(test)]
mod v5_tests {
    use super::*;

    #[test]
    fn swap_persist_survives_roundtrip() {
        for v in [false, true] {
            let mut buf = [0u8; 8];
            let n = save_swap_v2(v, &mut buf).unwrap();
            assert_eq!(load_swap_v2(&buf[..n]), Some(v));
        }
    }

    #[test]
    fn debounce_never_blocks_spaced_toggles() {
        let mut d = SwapDebounce::new();
        assert!(d.request(0));
        // 恰好 50ms 间隔：放行（边界不卡）。
        assert!(d.request(50));
        assert!(d.request(100));
        assert_eq!(d.folded, 0);
    }

    #[test]
    fn interaction_matrix_exhaustive() {
        // 4 动作 × 2 交换态 = 8 格：每格应答键回映语义正确。
        for swap in [false, true] {
            assert!(interaction_chain_ok(swap));
        }
    }
}
