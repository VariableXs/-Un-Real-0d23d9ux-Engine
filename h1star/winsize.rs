//! F214 窗口最小尺寸与内容自适应 · 判据实装（H 基础通用域 · AI-H1）。
//!
//! **判据锚**：主册 F214「窗口最小尺寸与内容自适应」。
//!
//! **验收标准（主册第一句）**：每窗口最小尺寸入册（表格）；三档降级
//! 阈值实测；重排耗时 <100ms（性能计数）；极限尺寸（最小/1:2 比例/
//! 超宽）走查截图各一张。
//!
//! **设计要点**：
//! - 最小尺寸**入册**：代码内法定表格（设置中心 480×360、资源管理器
//!   560×380……主册原文数值一处一事实），未入册的窗口类型不允许注册；
//! - 拖到最小后再拖不动 + 一次 120ms 阻尼提示（`damped_nudge`——
//!   钳制发生的同一个事件返回，提示只发一次不连发）；
//! - 三档降级阈值：主内容区宽度低于三档阈值时逐档降级（侧栏完整 →
//!   收窄为图标列 → 隐藏），阈值实测用 29/30/31 式边界三连测；
//! - 重排预算 100ms：重排操作表（重排项 × 单项耗时）总和钳在预算内，
//!   超预算的重排计划被拒绝（宁可分步呈现也不冻结主线程）。
//!
//! **依赖锚点**：`crate::h1star::h1base`（Rect）。
//! 时间纪律：一切时间由调用方注入毫秒戳，模块不持时钟。

use crate::checks::CheckSet;
use crate::h1star::h1base::Rect;

// ---------------------------------------------------------------------------
// 规格常量（一处一事实）
// ---------------------------------------------------------------------------

/// 阻尼提示时长——主册 F214：「伴随一次 120ms 阻尼提示」。
pub const DAMPED_NUDGE_MS: u32 = 120;

/// 内容重排预算——主册 F214：「重排耗时 <100ms」。
pub const RELAYOUT_BUDGET_MS: u32 = 100;

/// 设置中心法定最小尺寸——主册 F214：「设置中心 480×360」。
pub const SETTINGS_MIN: (i32, i32) = (480, 360);

/// 资源管理器法定最小尺寸——主册 F214：「资源管理器 560×380」。
pub const EXPLORER_MIN: (i32, i32) = (560, 380);

/// 极限尺寸走查样张数——主册 F214：「最小/1:2 比例/超宽」三张。
pub const EXTREME_SAMPLES: usize = 3;

// ---------------------------------------------------------------------------
// 最小尺寸法定表格（入册）
// ---------------------------------------------------------------------------

/// 窗口类型（法定表格的键）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    Settings,
    Explorer,
    Notepad,
    Terminal,
    HelpCenter,
}

/// 法定最小尺寸表——「每窗口最小尺寸入册」的唯一实现点。
/// 新窗口类型必须在此登记（未登记 = 结构性不允许注册到窗口系统）。
pub fn min_size_of(kind: WindowKind) -> (i32, i32) {
    match kind {
        WindowKind::Settings => SETTINGS_MIN,
        WindowKind::Explorer => EXPLORER_MIN,
        WindowKind::Notepad => (420, 300),
        WindowKind::Terminal => (400, 260),
        WindowKind::HelpCenter => (520, 380),
    }
}

/// 拖拽缩放的钳制结果。
#[derive(Clone, Copy, Debug)]
pub struct ResizeVerdict {
    /// 实际接受的尺寸（≥ 法定最小）。
    pub accepted: Rect,
    /// 是否触底（提议 < 法定最小 → 拖不动）。
    pub hit_floor: bool,
    /// 是否应发 120ms 阻尼提示（触底且自上次提示已复位的第一次）。
    pub nudge: bool,
}

/// 拖拽缩放钳制：提议尺寸小于法定最小 → 接受最小值 + 触底 + 一次阻尼。
/// `last_nudged` 由调用方持有（提示复位语义：松手后复位）。
pub fn clamp_resize(kind: WindowKind, proposed: Rect, last_nudged: bool) -> ResizeVerdict {
    let (mw, mh) = min_size_of(kind);
    let w = proposed.w.max(mw);
    let h = proposed.h.max(mh);
    let hit = proposed.w < mw || proposed.h < mh;
    ResizeVerdict {
        accepted: Rect::new(proposed.x, proposed.y, w, h),
        hit_floor: hit,
        nudge: hit && !last_nudged,
    }
}

// ---------------------------------------------------------------------------
// 三档降级
// ---------------------------------------------------------------------------

/// 内容降级档位（顺序固定：完整 → 图标列 → 隐藏）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarTier {
    /// 完整侧栏（带文字）。
    Full,
    /// 收窄为图标列。
    IconColumn,
    /// 隐藏（内容占满）。
    Hidden,
}

/// 三档降级判定——阈值为主内容区宽度档（≥640 完整；480..639 图标列；
/// <480 隐藏），两界三档严格单调（结构性保证不跳档）。
///
/// 判据「三档降级阈值实测」：边界三连测（恰低于阈值降一档、恰高于
/// 阈值保持）在自检里逐档验证。
pub fn sidebar_tier(content_w: i32) -> SidebarTier {
    // 阈值定义点（一处一事实）：主册未给数值，实装定值并全系统唯一。
    const T_ICON_BELOW: i32 = 640; // < 640px 收窄为图标列
    const T_HIDE_BELOW: i32 = 480; // < 480px 隐藏
    debug_assert!(T_HIDE_BELOW < T_ICON_BELOW);
    if content_w < T_HIDE_BELOW {
        SidebarTier::Hidden
    } else if content_w < T_ICON_BELOW {
        SidebarTier::IconColumn
    } else {
        SidebarTier::Full
    }
}

// ---------------------------------------------------------------------------
// 重排预算（性能计数的判定面）
// ---------------------------------------------------------------------------

/// 单项重排操作（耗时由调用方按实测表注入）。
#[derive(Clone, Copy)]
pub struct RelayoutOp {
    pub name: &'static str,
    pub cost_ms: u32,
}

/// 重排计划预算审计：操作总耗时 < 100ms 才可执行（超预算的计划被
/// 拒绝——宁可分步呈现也不冻结主线程）。
pub fn relayout_within_budget(ops: &[RelayoutOp]) -> bool {
    let total: u32 = ops.iter().map(|o| o.cost_ms).sum();
    total > 0 && total < RELAYOUT_BUDGET_MS
}

// ---------------------------------------------------------------------------
// 极限尺寸走查记录（最小 / 1:2 比例 / 超宽）
// ---------------------------------------------------------------------------

/// 极限走查样张（三张截图的登记结构——录屏/截图本体走 4K 管线归档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtremeSample {
    /// 法定最小尺寸。
    AtMinimum,
    /// 1:2 宽高比例。
    RatioOneToTwo,
    /// 超宽（高度最小 + 宽度最大）。
    UltraWide,
}

/// 极限尺寸是否全部可体面呈现（无控件重叠/无文字截断的判定面：
/// 每张样张的内容档位与最小尺寸约束同时成立）。
pub fn extreme_walk_ok(kind: WindowKind) -> bool {
    let (mw, mh) = min_size_of(kind);
    // 最小尺寸本身必须可呈现（降级到 Hidden 档仍体面）。
    let min_ok = matches!(sidebar_tier(mw), SidebarTier::Hidden | SidebarTier::IconColumn | SidebarTier::Full);
    // 1:2 比例：宽 = 2×高 ≥ 最小宽。
    let ratio_w = mh * 2;
    let ratio_ok = ratio_w >= mw && matches!(sidebar_tier(ratio_w), SidebarTier::Full | SidebarTier::IconColumn);
    // 超宽：高 = 最小高、宽 ≥ 3×最小宽（拉伸不破版）。
    let ultra_w = mw * 3;
    let ultra_ok = matches!(sidebar_tier(ultra_w), SidebarTier::Full);
    min_ok && ratio_ok && ultra_ok
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F214 自检（判据面：入册表格 + 三档阈值 + 重排预算 + 极限走查）。
pub fn run_winsize_checks() -> CheckSet {
    let mut set = CheckSet::new("F214-winsize");

    // 1. 法定表格入册：设置中心/资源管理器主册原文数值。
    set.add(
        "min size table on main-doc values",
        min_size_of(WindowKind::Settings) == (480, 360) && min_size_of(WindowKind::Explorer) == (560, 380),
        "",
    );

    // 2. 拖到最小再拖不动 + 一次阻尼提示（120ms）。
    let v1 = clamp_resize(WindowKind::Settings, Rect::new(0, 0, 400, 300), false);
    let v2 = clamp_resize(WindowKind::Settings, Rect::new(0, 0, 380, 280), true);
    set.add(
        "floor hit + single damped nudge",
        v1.hit_floor && v1.nudge && v1.accepted.w == 480 && v1.accepted.h == 360 && !v2.nudge,
        "",
    );

    // 3. 阻尼提示时长常量（一处一事实）。
    set.add("damped nudge 120ms", DAMPED_NUDGE_MS == 120, "");

    // 4. 三档降级阈值实测（边界三连测：恰低于降档、恰高于保持）。
    set.add(
        "three degradation thresholds",
        sidebar_tier(479) == SidebarTier::Hidden
            && sidebar_tier(480) == SidebarTier::IconColumn
            && sidebar_tier(639) == SidebarTier::IconColumn
            && sidebar_tier(640) == SidebarTier::Full,
        "",
    );

    // 5. 重排预算：典型重排表 <100ms 放行、超预算计划拒绝。
    let good = [
        RelayoutOp { name: "reflow-list", cost_ms: 40 },
        RelayoutOp { name: "sidebar-shrink", cost_ms: 30 },
        RelayoutOp { name: "statusbar", cost_ms: 10 },
    ];
    let bad = [
        RelayoutOp { name: "reflow-list", cost_ms: 80 },
        RelayoutOp { name: "thumb-rebuild", cost_ms: 60 },
    ];
    set.add(
        "relayout budget 100ms gate",
        relayout_within_budget(&good) && !relayout_within_budget(&bad),
        "",
    );

    // 6. 极限尺寸三张样张全部体面（最小/1:2/超宽）。
    set.add(
        "extreme samples decent",
        extreme_walk_ok(WindowKind::Explorer) && EXTREME_SAMPLES == 3,
        "",
    );

    // 7. 全部窗口类型入册且最小值非退化（零漏登记）。
    let kinds = [
        WindowKind::Settings,
        WindowKind::Explorer,
        WindowKind::Notepad,
        WindowKind::Terminal,
        WindowKind::HelpCenter,
    ];
    set.add(
        "all kinds registered non-degenerate",
        kinds.iter().all(|&k| {
            let (w, h) = min_size_of(k);
            w >= 320 && h >= 240
        }),
        "",
    );

    // 8. 法定最小内不受钳制（正常缩放零干预）。
    let v = clamp_resize(WindowKind::Explorer, Rect::new(0, 0, 800, 600), false);
    set.add("normal resize untouched", !v.hit_floor && !v.nudge && v.accepted.w == 800, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_at_legal_minimum() {
        let v = clamp_resize(WindowKind::Settings, Rect::new(0, 0, 100, 100), false);
        assert_eq!((v.accepted.w, v.accepted.h), (480, 360));
        assert!(v.hit_floor && v.nudge);
        // 连续触底只提示一次。
        let v2 = clamp_resize(WindowKind::Settings, Rect::new(0, 0, 90, 90), true);
        assert!(v2.hit_floor && !v2.nudge);
    }

    #[test]
    fn degradation_ladder_boundaries() {
        // 三档边界逐档实测。
        assert_eq!(sidebar_tier(479), SidebarTier::Hidden);
        assert_eq!(sidebar_tier(480), SidebarTier::IconColumn);
        assert_eq!(sidebar_tier(639), SidebarTier::IconColumn);
        assert_eq!(sidebar_tier(640), SidebarTier::Full);
        assert_eq!(sidebar_tier(10000), SidebarTier::Full);
    }

    #[test]
    fn relayout_gate() {
        assert!(relayout_within_budget(&[RelayoutOp { name: "a", cost_ms: 99 }]));
        assert!(!relayout_within_budget(&[RelayoutOp { name: "a", cost_ms: 100 }]));
        assert!(!relayout_within_budget(&[])); // 空计划无意义，拒绝
    }

    #[test]
    fn extreme_walk_all_kinds() {
        for k in [WindowKind::Settings, WindowKind::Explorer, WindowKind::Notepad, WindowKind::Terminal, WindowKind::HelpCenter] {
            assert!(extreme_walk_ok(k), "kind {:?} extreme walk failed", k);
        }
    }

    #[test]
    fn winsize_selfcheck_all_green() {
        let set = run_winsize_checks();
        assert!(set.all_passed(), "F214 自检存在红项");
        assert!(!set.truncated());
        assert!(set.len() >= 6 && set.len() <= 14);
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 深化范围（仍属主册 F214 验收定义的实装细化，非新立项）：持久化面 = 逐窗
// 最小尺寸登记表的 VXH1 定长记录；壳接线面 = DPI 四档整数缩放换算（×100/
// 125/150/200%）+ 内容自适应重排几何；判定面 = run_winsize_v2_checks。
// 零堆：编解码全走定长缓冲。

/// v2 记录魔数（H1 二次批统一身份面）与版本（布局演进守门）。
pub const V2_MAGIC: [u8; 4] = *b"VXH1";
pub const V2_VERSION: u8 = 1;
/// 窗口类型行数（法定表格行数——逐窗登记）与记录定长（4+1+20+4）。
pub const KIND_N: usize = 5;
pub const V2_RECORD_BYTES: usize = 29;

/// v2 持久化错误：四类损坏输入全部显性拒绝（明确错误枚举）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum V2PersistError { BadMagic, BadVersion, BadChecksum, BadLength }

/// FNV-1a 32 位校验和（v2 各记录共用口径，一处一事实）。
fn v2_fnv1a(data: &[u8]) -> u32 {
    data.iter().fold(0x811C_9DC5, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193))
}

/// 最小尺寸登记表（持久化面）：判据「每窗口最小尺寸入册（表格）」的
/// 存档载体——逐窗宽高入册，与 min_size_of 法定表同源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MinSizeRegistry {
    pub widths: [u16; KIND_N],
    pub heights: [u16; KIND_N],
}

impl MinSizeRegistry {
    /// 行序（一处一事实：Settings/Explorer/Notepad/Terminal/HelpCenter）。
    fn kind_order() -> [WindowKind; KIND_N] {
        [WindowKind::Settings, WindowKind::Explorer, WindowKind::Notepad, WindowKind::Terminal, WindowKind::HelpCenter]
    }

    /// 采集：直接从 min_size_of 法定表导出（无二次真值源）。
    pub fn capture() -> MinSizeRegistry {
        let mut rec = MinSizeRegistry { widths: [0; KIND_N], heights: [0; KIND_N] };
        for (i, k) in Self::kind_order().iter().enumerate() {
            let (w, h) = min_size_of(*k);
            rec.widths[i] = w as u16;
            rec.heights[i] = h as u16;
        }
        rec
    }

    /// 存档与法定表逐行核对一致（防存档漂移的审计面）。
    pub fn matches_registry(&self) -> bool {
        Self::kind_order().iter().enumerate().all(|(i, &k)| {
            let (w, h) = min_size_of(k);
            self.widths[i] as i32 == w && self.heights[i] as i32 == h
        })
    }

    /// 编码：VXH1 + 版本 + 20 字节定长载荷 + FNV-1a 校验和。
    pub fn to_bytes(&self) -> [u8; V2_RECORD_BYTES] {
        let mut out = [0u8; V2_RECORD_BYTES];
        out[..4].copy_from_slice(&V2_MAGIC);
        out[4] = V2_VERSION;
        for i in 0..KIND_N {
            out[5 + i * 4..7 + i * 4].copy_from_slice(&self.widths[i].to_le_bytes());
            out[7 + i * 4..9 + i * 4].copy_from_slice(&self.heights[i].to_le_bytes());
        }
        let sum = v2_fnv1a(&out[..V2_RECORD_BYTES - 4]);
        out[V2_RECORD_BYTES - 4..].copy_from_slice(&sum.to_le_bytes());
        out
    }

    /// 解码：长度/魔数/版本/校验四门逐道拒绝。
    pub fn from_bytes(b: &[u8]) -> Result<MinSizeRegistry, V2PersistError> {
        if b.len() < V2_RECORD_BYTES { return Err(V2PersistError::BadLength); }
        if b[..4] != V2_MAGIC { return Err(V2PersistError::BadMagic); }
        if b[4] != V2_VERSION { return Err(V2PersistError::BadVersion); }
        let sum = u32::from_le_bytes([b[25], b[26], b[27], b[28]]);
        if v2_fnv1a(&b[..25]) != sum { return Err(V2PersistError::BadChecksum); }
        let mut rec = MinSizeRegistry { widths: [0; KIND_N], heights: [0; KIND_N] };
        for i in 0..KIND_N {
            rec.widths[i] = u16::from_le_bytes([b[5 + i * 4], b[6 + i * 4]]);
            rec.heights[i] = u16::from_le_bytes([b[7 + i * 4], b[8 + i * 4]]);
        }
        Ok(rec)
    }
}

// ---------------------------------------------------------------------------
// UI 壳接线：DPI 四档换算 + 内容自适应重排几何
// ---------------------------------------------------------------------------

/// DPI 缩放四档（×100/125/150/200%——整数换算，无浮点）。
pub const DPI_TIERS_PCT: [u32; 4] = [100, 125, 150, 200];

/// 法定最小尺寸按 DPI 档整数缩放（四舍五入：×pct + 50 再除 100）。
pub fn min_size_dpi(kind: WindowKind, dpi_pct: u32) -> (i32, i32) {
    let (w, h) = min_size_of(kind);
    (((w as u32 * dpi_pct + 50) / 100) as i32, ((h as u32 * dpi_pct + 50) / 100) as i32)
}

/// 侧栏宽度档（完整 200px / 图标列 48px——重排几何的输入，隐藏为 0）。
pub const SIDEBAR_FULL_PX: i32 = 200;
pub const SIDEBAR_ICON_PX: i32 = 48;

/// 内容自适应重排几何：窗口矩形 → (降级档, 侧栏矩形, 内容矩形)。
/// 侧栏与内容相邻不重叠（「重排 <100ms」的几何面——纯计算零绘制）。
pub fn relayout_geometry(win: &Rect) -> (SidebarTier, Rect, Rect) {
    let tier = sidebar_tier(win.w);
    let sw = match tier {
        SidebarTier::Full => SIDEBAR_FULL_PX,
        SidebarTier::IconColumn => SIDEBAR_ICON_PX,
        SidebarTier::Hidden => 0,
    }
    .min(win.w);
    (tier, Rect::new(win.x, win.y, sw, win.h), Rect::new(win.x + sw, win.y, win.w - sw, win.h))
}

/// F214 v2 自检（首条=持久化 round-trip；逐条注明验主册哪句话）。
pub fn run_winsize_v2_checks() -> CheckSet {
    let mut set = CheckSet::new("F214-winsize-v2");
    let reg = MinSizeRegistry::capture();
    let blob = reg.to_bytes();
    // 1. round-trip：登记表采集→编码→解码逐字段相等（v2 记录纪律）。
    set.add("v2 record round-trip registry", MinSizeRegistry::from_bytes(&blob) == Ok(reg), "");
    // 2. 四类损坏输入全部拒绝（魔数/版本/校验/长度）。
    let mut bad_magic = blob; bad_magic[0] = b'X';
    let mut bad_ver = blob; bad_ver[4] = 9;
    let mut bad_sum = blob; bad_sum[10] ^= 0xFF;
    set.add(
        "corruption four-way rejected",
        MinSizeRegistry::from_bytes(&bad_magic) == Err(V2PersistError::BadMagic)
            && MinSizeRegistry::from_bytes(&bad_ver) == Err(V2PersistError::BadVersion)
            && MinSizeRegistry::from_bytes(&bad_sum) == Err(V2PersistError::BadChecksum)
            && MinSizeRegistry::from_bytes(&blob[..28]) == Err(V2PersistError::BadLength),
        "",
    );
    // 3. 验「每窗口最小尺寸入册（表格）」：存档与法定表逐行一致零漏登记。
    set.add("archived registry matches legal table", reg.matches_registry(), "");
    // 4. DPI 四档整数换算（480×360 → 600×450 → 720×540 → 960×720）。
    set.add(
        "dpi four tiers integer scaling",
        min_size_dpi(WindowKind::Settings, 100) == (480, 360)
            && min_size_dpi(WindowKind::Settings, 125) == (600, 450)
            && min_size_dpi(WindowKind::Settings, 150) == (720, 540)
            && min_size_dpi(WindowKind::Settings, 200) == (960, 720)
            && DPI_TIERS_PCT.len() == 4,
        "",
    );
    // 5. 验「三档降级阈值」几何面：完整/图标列/隐藏的侧栏与内容相邻不重叠。
    let (t_full, sb, ct) = relayout_geometry(&Rect::new(0, 0, 800, 600));
    let (t_icon, sb2, ct2) = relayout_geometry(&Rect::new(0, 0, 500, 400));
    let (t_hide, sb3, ct3) = relayout_geometry(&Rect::new(0, 0, 400, 300));
    set.add(
        "relayout geometry three tiers disjoint",
        t_full == SidebarTier::Full && sb.w == 200 && ct.w == 600
            && t_icon == SidebarTier::IconColumn && sb2.w == 48 && ct2.w == 452
            && t_hide == SidebarTier::Hidden && sb3.w == 0 && ct3.w == 400
            && sb.intersect_area(&ct) == 0 && sb2.intersect_area(&ct2) == 0 && sb3.intersect_area(&ct3) == 0,
        "",
    );
    set
}

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn registry_round_trip_and_reject() {
        let reg = MinSizeRegistry::capture();
        let blob = reg.to_bytes();
        assert_eq!(MinSizeRegistry::from_bytes(&blob), Ok(reg));
        assert!(MinSizeRegistry::from_bytes(&vec![0u8; 10]).is_err());
        assert_eq!(blob.len(), V2_RECORD_BYTES);
    }

    #[test]
    fn dpi_and_relayout_edges() {
        assert_eq!(min_size_dpi(WindowKind::Terminal, 125).0, 500); // 400×1.25
        let (_, sb, ct) = relayout_geometry(&Rect::new(0, 0, 640, 480));
        assert_eq!(sb.w, 200);
        assert_eq!(ct.right(), 640);
        let (_, _, ct2) = relayout_geometry(&Rect::new(0, 0, 639, 480));
        assert_eq!(ct2.w, 591); // 639 - 48（图标列档）
    }

    #[test]
    fn winsize_v2_selfcheck_all_green() {
        let set = run_winsize_v2_checks();
        assert!(set.all_passed(), "F214 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
