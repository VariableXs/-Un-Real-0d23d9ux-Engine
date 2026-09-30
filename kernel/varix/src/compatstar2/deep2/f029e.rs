//! F029 深化批次三 · 显示模式协商执行/边界/注入面（compatstar2/deep2 · G-A-29）。
//!
//! 批次一/二深化覆盖单屏枚举契约主干；本批补齐主册【功能定义】「全语义对齐」
//! 的模式协商侧出口：枚举模式排序模型（分辨率主序降序×刷新率次序降序，定长
//! 16 模式表，冒泡定序）、刷新率白名单窗口判定（24-240Hz 之外如实拒绝并计数
//! ）、DEVMODE 字段掩码变更检测（仅掩码声明字段参与变更比较，未声明字段忽略
//! ——六字段真实位值）、虚拟桌面坐标几何（多显示器原点/偏移/包围盒 union，
//! 矩形相交与并集，定长 4 屏）。
//!
//! 判据对账：主册【设计细节】/【状态与异常】未落地面为源，一处一事实（MS
//! EnumDisplaySettings / DEVMODE dmFields 文档语义对拍；接口冻结 ADR-PR-001
//! 不变）。零堆纪律：定长模式表 + 定长屏表，无 alloc。
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 模式表容量（定长 16 模式）。
pub const MAX_MODES: usize = 16;
/// 刷新率白名单窗口（主册 G-A-29 模式协商口径：24Hz 下界 / 240Hz 上界，边界含）。
pub const MIN_REFRESH_HZ: u32 = 24;
pub const MAX_REFRESH_HZ: u32 = 240;
/// DM_* 位值对拍 MS DEVMODE dmFields：DISPLAYORIENTATION=0x80、
/// BITSPERPEL=0x40000、PELSWIDTH=0x80000、PELSHEIGHT=0x100000、
/// DISPLAYFLAGS=0x200000、DISPLAYFREQUENCY=0x400000（六字段一处一事实）。
pub const DM_DISPLAYORIENTATION: u32 = 0x0000_0080;
pub const DM_BITSPERPEL: u32 = 0x0004_0000;
pub const DM_PELSWIDTH: u32 = 0x0008_0000;
pub const DM_PELSHEIGHT: u32 = 0x0010_0000;
pub const DM_DISPLAYFLAGS: u32 = 0x0020_0000;
pub const DM_DISPLAYFREQUENCY: u32 = 0x0040_0000;
/// 虚拟桌面屏表容量（定长 4 屏——主册【设计细节】枚举结构预留）。
pub const MAX_SCREENS: usize = 4;

// ---------------------------------------------------------------------------
// 枚举模式排序模型
// ---------------------------------------------------------------------------

/// 一个显示模式（宽×高×刷新率）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DispMode { pub width: u32, pub height: u32, pub refresh: u32 }

/// 定长 16 模式表（满后显性拒绝并计数——不静默覆盖）。
pub struct ModeTable {
    pub modes: [DispMode; MAX_MODES],
    pub len: usize,
    pub overflow_rejects: u32,
}

impl ModeTable {
    pub const fn new() -> Self {
        ModeTable {
            modes: [DispMode { width: 0, height: 0, refresh: 0 }; MAX_MODES],
            len: 0,
            overflow_rejects: 0,
        }
    }

    pub fn push(&mut self, m: DispMode) -> Result<(), &'static str> {
        if self.len >= MAX_MODES {
            self.overflow_rejects += 1;
            return Err("mode-table-full");
        }
        self.modes[self.len] = m;
        self.len += 1;
        Ok(())
    }

    /// 冒泡定序：分辨率主序降序 × 刷新率次序降序（稳定排序，枚举面契约）。
    pub fn sort_desc(&mut self) {
        if self.len < 2 {
            return;
        }
        for i in 0..self.len - 1 {
            for j in 0..self.len - 1 - i {
                let a = self.modes[j];
                let b = self.modes[j + 1];
                if (a.width, a.height, a.refresh) < (b.width, b.height, b.refresh) {
                    self.modes.swap(j, j + 1);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 刷新率白名单窗口判定 + DEVMODE 字段掩码变更检测
// ---------------------------------------------------------------------------

/// 刷新率白名单闸门（24-240Hz 之外如实拒绝并计数）。
pub struct RefreshGate {
    pub accepts: u32,
    pub rejects: u32,
}

impl RefreshGate {
    pub const fn new() -> Self { RefreshGate { accepts: 0, rejects: 0 } }

    /// 边界值含（24 与 240 合法）；越窗 Err 并计数（零静默吞错）。
    pub fn admit(&mut self, hz: u32) -> Result<(), &'static str> {
        if hz >= MIN_REFRESH_HZ && hz <= MAX_REFRESH_HZ {
            self.accepts += 1;
            Ok(())
        } else {
            self.rejects += 1;
            Err("refresh-out-of-window")
        }
    }
}

/// DEVMODE 六字段真实位面（与 DM_* 位值一一对应）。
#[derive(Clone, Copy)]
pub struct DevMode {
    pub bits_per_pel: u32,
    pub pel_width: u32,
    pub pel_height: u32,
    pub display_flags: u32,
    pub display_frequency: u32,
    pub display_orientation: u32,
}

/// 变更检测：仅掩码声明字段参与比较，未声明字段忽略；
/// 返回值 = 发生变更的 DM_* 位集合（ChangeDisplaySettingsEx 语义对拍）。
pub fn changed_fields(before: &DevMode, after: &DevMode, mask: u32) -> u32 {
    let mut diff = 0u32;
    if mask & DM_BITSPERPEL != 0 && before.bits_per_pel != after.bits_per_pel {
        diff |= DM_BITSPERPEL;
    }
    if mask & DM_PELSWIDTH != 0 && before.pel_width != after.pel_width {
        diff |= DM_PELSWIDTH;
    }
    if mask & DM_PELSHEIGHT != 0 && before.pel_height != after.pel_height {
        diff |= DM_PELSHEIGHT;
    }
    if mask & DM_DISPLAYFLAGS != 0 && before.display_flags != after.display_flags {
        diff |= DM_DISPLAYFLAGS;
    }
    if mask & DM_DISPLAYFREQUENCY != 0 && before.display_frequency != after.display_frequency {
        diff |= DM_DISPLAYFREQUENCY;
    }
    if mask & DM_DISPLAYORIENTATION != 0 && before.display_orientation != after.display_orientation {
        diff |= DM_DISPLAYORIENTATION;
    }
    diff
}

/// 屏幕矩形（副屏可负原点——多屏几何同构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect { pub left: i32, pub top: i32, pub right: i32, pub bottom: i32 }

/// 矩形相交（空交 → None——不虚造退化矩形）。
pub fn rect_intersect(a: Rect, b: Rect) -> Option<Rect> {
    let l = a.left.max(b.left);
    let t = a.top.max(b.top);
    let r = a.right.min(b.right);
    let bot = a.bottom.min(b.bottom);
    if l < r && t < bot { Some(Rect { left: l, top: t, right: r, bottom: bot }) } else { None }
}

/// 矩形并集包围盒（union 的 bounding box 口径）。
pub fn rect_union_bbox(a: Rect, b: Rect) -> Rect {
    Rect {
        left: a.left.min(b.left), top: a.top.min(b.top),
        right: a.right.max(b.right), bottom: a.bottom.max(b.bottom),
    }
}

/// 虚拟桌面（定长 4 屏：原点/偏移登记 + 包围盒 union）。
pub struct VDesktop {
    pub screens: [Option<Rect>; MAX_SCREENS],
    pub len: usize,
    pub overflow_rejects: u32,
}

impl VDesktop {
    pub const fn new() -> Self {
        VDesktop { screens: [None; MAX_SCREENS], len: 0, overflow_rejects: 0 }
    }
    /// 挂载一屏；表满显性拒绝并计数。
    pub fn attach(&mut self, r: Rect) -> Result<(), &'static str> {
        if self.len >= MAX_SCREENS {
            self.overflow_rejects += 1;
            return Err("screen-table-full");
        }
        self.screens[self.len] = Some(r);
        self.len += 1;
        Ok(())
    }

    /// 包围盒 union：全屏原点/偏移合并（空桌 → None）。
    pub fn bounding_box(&self) -> Option<Rect> {
        let mut acc: Option<Rect> = None;
        for &r in self.screens.iter().flatten() {
            acc = Some(match acc { None => r, Some(a) => rect_union_bbox(a, r) });
        }
        acc
    }
}

// ---------------------------------------------------------------------------
// 域自检（深化批次三）
// ---------------------------------------------------------------------------

pub fn run_f029e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F029-dispmode-d3");
    // 1) 排序：分辨率主序降序 × 刷新率次序降序（乱序表冒泡定序）。
    let mut t = ModeTable::new();
    for m in [
        DispMode { width: 1920, height: 1080, refresh: 60 },
        DispMode { width: 2560, height: 1440, refresh: 144 },
        DispMode { width: 1920, height: 1080, refresh: 144 },
        DispMode { width: 1280, height: 720, refresh: 240 },
        DispMode { width: 2560, height: 1440, refresh: 60 },
    ] {
        let _ = t.push(m);
    }
    t.sort_desc();
    let ordered = (t.modes[0].width, t.modes[0].refresh) == (2560, 144)
        && (t.modes[1].width, t.modes[1].refresh) == (2560, 60)
        && (t.modes[2].width, t.modes[2].refresh) == (1920, 144)
        && (t.modes[3].width, t.modes[3].refresh) == (1920, 60)
        && t.modes[4].width == 1280;
    cs.add("mode_sort_desc_res_x_refresh", ordered && t.len == 5, "");
    // 2) 模式表满显性拒绝：17 个模式 → 第 17 个 Err 并计数。
    let mut full = ModeTable::new();
    let mut last_push = Ok(());
    for i in 0..17 {
        last_push = full.push(DispMode { width: 640, height: 480, refresh: 60 + i });
    }
    cs.add(
        "mode_table_full_reject",
        last_push == Err("mode-table-full") && full.overflow_rejects == 1 && full.len == MAX_MODES,
        "",
    );
    // 3) 刷新率白名单：24/240 边界含合法；23/241 越窗拒绝并计数。
    let mut g = RefreshGate::new();
    let lo = g.admit(24);
    let hi = g.admit(240);
    let bad_lo = g.admit(23);
    let bad_hi = g.admit(241);
    cs.add(
        "refresh_whitelist_window",
        lo.is_ok() && hi.is_ok() && bad_lo == Err("refresh-out-of-window")
            && bad_hi.is_err() && g.rejects == 2 && g.accepts == 2,
        "",
    );
    // 4) DM_* 位值对拍 MS DEVMODE（六字段一处一事实）。
    cs.add(
        "dm_bit_values",
        DM_DISPLAYORIENTATION == 0x0000_0080 && DM_BITSPERPEL == 0x0004_0000
            && DM_PELSWIDTH == 0x0008_0000 && DM_PELSHEIGHT == 0x0010_0000
            && DM_DISPLAYFLAGS == 0x0020_0000 && DM_DISPLAYFREQUENCY == 0x0040_0000,
        "",
    );
    // 5) 掩码变更检测：宽+频率双变，掩码仅声明宽 → 只报宽位。
    let before = DevMode {
        bits_per_pel: 32,
        pel_width: 1920,
        pel_height: 1080,
        display_flags: 1,
        display_frequency: 60,
        display_orientation: 0,
    };
    let mut after = before;
    after.pel_width = 2560;
    after.display_frequency = 144;
    cs.add("dm_mask_change_detect", changed_fields(&before, &after, DM_PELSWIDTH) == DM_PELSWIDTH, "");
    // 6) 未声明字段忽略：掩码不含频率 → 频率变更不报；掩码含 → 报。
    let ignored = changed_fields(&before, &after, DM_PELSHEIGHT);
    let reported = changed_fields(&before, &after, DM_DISPLAYFREQUENCY | DM_PELSHEIGHT);
    cs.add("dm_unmasked_ignored", ignored == 0 && reported == DM_DISPLAYFREQUENCY, "");
    // 7) 虚拟桌面负原点包围盒：副屏在左 → union 含负 x。
    let mut vd = VDesktop::new();
    let _ = vd.attach(Rect { left: 0, top: 0, right: 1920, bottom: 1080 });
    let _ = vd.attach(Rect { left: -1920, top: 0, right: 0, bottom: 1080 });
    cs.add(
        "vdesktop_negative_origin_bbox",
        vd.bounding_box() == Some(Rect { left: -1920, top: 0, right: 1920, bottom: 1080 }),
        "",
    );
    // 8) 矩形相交：重叠得交矩形；分离/贴边 → None。
    let r1 = Rect { left: 0, top: 0, right: 100, bottom: 100 };
    let r2 = Rect { left: 50, top: 50, right: 150, bottom: 150 };
    let r3 = Rect { left: 100, top: 0, right: 200, bottom: 100 };
    cs.add(
        "rect_intersect",
        rect_intersect(r1, r2) == Some(Rect { left: 50, top: 50, right: 100, bottom: 100 })
            && rect_intersect(r1, r3) == None,
        "",
    );
    // 9) 矩形并集包围盒 + 屏表满拒绝（第 5 屏 Err 计数）。
    let uni = rect_union_bbox(r1, r2);
    let mut vd2 = VDesktop::new();
    let mut fifth = Ok(());
    for _ in 0..5 {
        fifth = vd2.attach(r1);
    }
    cs.add(
        "rect_union_and_screen_cap",
        uni == Rect { left: 0, top: 0, right: 150, bottom: 150 }
            && fifth == Err("screen-table-full") && vd2.overflow_rejects == 1,
        "",
    );
    // 10) 空桌包围盒显性 None（不虚造退化矩形）。
    cs.add("vdesktop_empty_none", VDesktop::new().bounding_box() == None, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_mode_table_no_sort_panic() {
        let mut t = ModeTable::new();
        let _ = t.push(DispMode { width: 800, height: 600, refresh: 60 });
        t.sort_desc(); // len<2 早退，不 panic 不越界
        assert_eq!(t.modes[0].width, 800);
        assert_eq!(t.len, 1);
    }

    #[test]
    fn gate_boundary_counts_honest() {
        let mut g = RefreshGate::new();
        assert!(g.admit(60).is_ok());
        assert!(g.admit(120).is_ok());
        assert_eq!(g.accepts, 2);
        assert_eq!(g.rejects, 0);
        assert_eq!(g.admit(0), Err("refresh-out-of-window"));
        assert_eq!(g.rejects, 1, "0Hz 如实拒绝——零静默");
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f029e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
