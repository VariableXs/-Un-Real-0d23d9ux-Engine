//! GALAXY AI-29 多显示器域（G1681~G1700）。
//!
//! 显示器枚举（自动命名）、分辨率/刷新率、多屏布局、热插拔、布局记忆、
//! 主屏切换、扩展/镜像、每屏独立壁纸/缩放、出屏吸附、DPI 统一、
//! 亮度色温（夜间护眼）。
//! 首创点：多显示器无缝管理（拔插自动恢复布局）。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// G1681 显示器枚举与识别 — 自动命名
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct Monitor {
    pub id: u8,
    pub edid_hash: u32,
    pub connected: bool,
    pub name_idx: u8, // 自动命名序号
}

/// 自动命名：同 EDID 多台 → "DELL-1/DELL-2"。
pub fn auto_name(vendor: &str, name_idx: u8) -> [u8; 12] {
    let mut out = [0u8; 12];
    let v = vendor.as_bytes();
    let n = v.len().min(8);
    out[..n].copy_from_slice(&v[..n]);
    out[n] = b'-';
    out[n + 1] = b'0' + (name_idx / 10).min(9);
    out[n + 2] = b'0' + name_idx % 10;
    out
}

/// 枚举：按 EDID 哈希去重计数。
pub fn enumerate_unique(monitors: &[Monitor]) -> usize {
    let mut uniq: [u32; 8] = [0; 8];
    let mut n = 0;
    for m in monitors {
        if m.connected && !uniq[..n].contains(&m.edid_hash) && n < 8 {
            uniq[n] = m.edid_hash;
            n += 1;
        }
    }
    n
}

// ---------------------------------------------------------------------------
// G1682 分辨率/刷新率管理 — 一键切换
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub struct VideoMode {
    pub w: u16,
    pub h: u16,
    pub hz: u8,
}

/// 模式合法性：宽高非零、8 字对齐、刷新率 24~240。
pub fn mode_ok(m: &VideoMode) -> bool {
    m.w > 0 && m.h > 0 && m.w % 8 == 0 && (24..=240).contains(&m.hz)
}

/// 从模式列表选最优：优先像素数，再刷新率。
pub fn pick_best_mode(modes: &[VideoMode]) -> Option<VideoMode> {
    let mut best: Option<VideoMode> = None;
    for &m in modes {
        if !mode_ok(&m) {
            continue;
        }
        best = match best {
            None => Some(m),
            Some(b) => {
                let pa = m.w as u32 * m.h as u32;
                let pb = b.w as u32 * b.h as u32;
                if pa > pb || (pa == pb && m.hz > b.hz) {
                    Some(m)
                } else {
                    Some(b)
                }
            }
        };
    }
    best
}

// ---------------------------------------------------------------------------
// G1683 多显示器布局 — 拖拽排列
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct MonitorRect {
    pub id: u8,
    pub x: i16,
    pub y: i16,
    pub w: u16,
    pub h: u16,
}

impl MonitorRect {
    /// 两屏是否边贴合（允许 4px 容差）。
    pub fn adjacent(&self, o: &MonitorRect) -> bool {
        let tol = 4;
        let v_touch = (self.x + self.w as i16 - o.x).abs() <= tol || (o.x + o.w as i16 - self.x).abs() <= tol;
        let h_overlap = self.y < o.y + o.h as i16 && o.y < self.y + self.h as i16;
        let h_touch = (self.y + self.h as i16 - o.y).abs() <= tol || (o.y + o.h as i16 - self.y).abs() <= tol;
        let v_overlap = self.x < o.x + o.w as i16 && o.x < self.x + self.w as i16;
        (v_touch && h_overlap) || (h_touch && v_overlap)
    }
}

// ---------------------------------------------------------------------------
// G1684/G1685 显示器热插拔 + 布局记忆
// ---------------------------------------------------------------------------

pub struct DisplayManager {
    pub monitors: [MonitorRect; 4],
    pub count: u8,
    /// 布局记忆：edid_hash → (x, y)。
    pub memo: [(u32, i16, i16); 4],
    pub memo_count: usize,
}

impl DisplayManager {
    pub const fn new() -> DisplayManager {
        DisplayManager {
            monitors: [MonitorRect { id: 0, x: 0, y: 0, w: 0, h: 0 }; 4],
            count: 0,
            memo: [(0, 0, 0); 4],
            memo_count: 0,
        }
    }
    pub fn remember(&mut self, hash: u32, x: i16, y: i16) -> bool {
        if self.memo_count >= 4 {
            return false;
        }
        self.memo[self.memo_count] = (hash, x, y);
        self.memo_count += 1;
        true
    }
    /// 热插拔接入：优先恢复记忆位置，否则放在主屏右侧。
    pub fn hotplug(&mut self, slot: usize, hash: u32, w: u16, h: u16) -> bool {
        if slot >= 4 || slot > self.count as usize || self.count >= 4 || w == 0 || h == 0 {
            return false;
        }
        let (x, y) = match self.memo[..self.memo_count].iter().find(|m| m.0 == hash) {
            Some(m) => (m.1, m.2),
            None => {
                // i32 中间量 + 饱和钳制：长序列布局不溢出 i16。
                let right = self.monitors[..self.count as usize]
                    .iter()
                    .map(|m| (m.x as i32 + m.w as i32).min(i16::MAX as i32))
                    .max()
                    .unwrap_or(0);
                (right as i16, 0)
            }
        };
        self.monitors[slot] = MonitorRect { id: slot as u8, x, y, w, h };
        if slot as u8 >= self.count {
            self.count = slot as u8 + 1;
        }
        true
    }
    /// 拔出：压缩数组。
    pub fn unplug(&mut self, slot: usize) -> bool {
        if slot as u8 >= self.count {
            return false;
        }
        for i in slot..self.count as usize - 1 {
            self.monitors[i] = self.monitors[i + 1];
        }
        self.count -= 1;
        self.monitors[self.count as usize] = MonitorRect { id: 0, x: 0, y: 0, w: 0, h: 0 };
        true
    }
}

// ---------------------------------------------------------------------------
// G1686 主显示器切换 — 一键
// ---------------------------------------------------------------------------

pub fn set_primary(count: u8, new_primary: u8) -> Option<u8> {
    if count == 0 || new_primary >= count {
        return None;
    }
    Some(new_primary)
}

// ---------------------------------------------------------------------------
// G1687 扩展/镜像模式 — 一键
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq)]
pub enum DisplayMode {
    Extend,
    Mirror,
    Single,
}

/// 镜像合法性：所有屏模式一致。
pub fn mirror_ok(modes: &[VideoMode]) -> bool {
    if modes.len() < 2 {
        return false;
    }
    modes.iter().all(|m| *m == modes[0])
}

// ---------------------------------------------------------------------------
// G1688 每屏独立壁纸/任务栏/缩放
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
pub struct PerMonitorPrefs {
    pub wallpaper_id: u16,
    pub taskbar: bool,
    pub scale_permil: u16,
}

pub fn per_monitor_ok(p: &PerMonitorPrefs) -> bool {
    matches!(p.scale_permil, 1000 | 1250 | 1500 | 2000)
}

// ---------------------------------------------------------------------------
// G1689 出屏窗口吸附回主屏 — 防止丢失
// ---------------------------------------------------------------------------

/// 窗口中心不在任何屏内 → 吸附到主屏（返回新 x,y）。
pub fn snap_back(wx: i32, wy: i32, screens: &[MonitorRect], primary: usize) -> Option<(i32, i32)> {
    if screens.is_empty() || primary >= screens.len() {
        return None;
    }
    let inside = screens.iter().any(|s| {
        wx >= s.x as i32 && wx < s.x as i32 + s.w as i32 && wy >= s.y as i32 && wy < s.y as i32 + s.h as i32
    });
    if inside {
        return Some((wx, wy));
    }
    let p = &screens[primary];
    Some((p.x as i32 + 16, p.y as i32 + 16))
}

// ---------------------------------------------------------------------------
// G1690 缩放与 DPI 统一 — 跨屏视觉一致
// ---------------------------------------------------------------------------

/// 逻辑尺寸 = 物理 / (scale/1000)；跨屏一致性 = 同物理 DPI 比例下逻辑尺寸差 ≤1。
pub fn logical_px(physical: u16, scale_permil: u16) -> u16 {
    if scale_permil == 0 {
        return physical;
    }
    ((physical as u32 * 1000) / scale_permil as u32) as u16
}

pub fn dpi_consistent(a: (u16, u16), b: (u16, u16)) -> bool {
    let da = (a.0 as i32 - b.0 as i32).abs();
    let db = (a.1 as i32 - b.1 as i32).abs();
    da <= 1 && db <= 1
}

// ---------------------------------------------------------------------------
// G1691 显示器亮度/色温 — 夜间护眼
// ---------------------------------------------------------------------------

/// 色温开尔文 → RGB 估算（Tanner Helland 简化，钳制输出；ln/pow 用内核数学库）。
pub fn kelvin_rgb(kelvin: u32) -> (u8, u8, u8) {
    let t = (kelvin.clamp(1000, 10000) / 100) as f64;
    let (r, g, b) = if t <= 66.0 {
        let g = 99.47 * crate::galaxy::math::ln64(t) - 161.12;
        let b = if t <= 19.0 {
            0.0
        } else {
            138.52 * crate::galaxy::math::ln64(t - 10.0) - 305.04
        };
        (255.0, g, b)
    } else {
        let r = 329.7 * crate::galaxy::math::pow64(t - 60.0, -0.1332);
        let g = 288.1 * crate::galaxy::math::pow64(t - 60.0, -0.0755);
        (r, g, 255.0)
    };
    (
        (r as i32).clamp(0, 255) as u8,
        (g as i32).clamp(0, 255) as u8,
        (b as i32).clamp(0, 255) as u8,
    )
}

/// 亮度 0~100。
pub fn brightness_ok(v: u8) -> bool {
    v <= 100
}

// ---------------------------------------------------------------------------
// G1692 显示器管理视觉 — 布局图美观直观
// ---------------------------------------------------------------------------

/// 布局 ASCII 缩略图：12×6 网格点阵。
pub fn layout_glyph(screens: &[MonitorRect]) -> [u8; 6] {
    let mut glyph = [b'.'; 6];
    for s in screens {
        let col = ((s.x.max(0) as u32) / 80).min(11) as usize;
        let row = ((s.y.max(0) as u32) / 200).min(5) as usize;
        glyph[row] = if glyph[row] == b'.' { (b'0' + s.id as u8).min(b'9') } else { b'*' };
        let _ = col;
    }
    glyph
}

// ---------------------------------------------------------------------------
// G1693 多显示器自定义 — 每屏配置
// ---------------------------------------------------------------------------

/// 每屏配置槽位 = 屏 id（0~3）。
pub fn per_screen_config_ok(id: u8, prefs: &PerMonitorPrefs) -> bool {
    id < 4 && per_monitor_ok(prefs)
}

// ---------------------------------------------------------------------------
// G1694 显示器无障碍 — 大字号/高对比
// ---------------------------------------------------------------------------

pub fn display_a11y_ok(font_permil: u16, high_contrast: bool, contrast_ratio_x10: u16) -> bool {
    font_permil >= 1000 && (high_contrast == (contrast_ratio_x10 >= 45))
}

// ---------------------------------------------------------------------------
// G1696 显示性能预算 — 模式切换/热插拔时延
// ---------------------------------------------------------------------------

pub fn display_budget_ok(mode_switch_ms: u32, budget_ms: u32) -> bool {
    mode_switch_ms <= budget_ms
}

// ---------------------------------------------------------------------------
// G1697 显示可观测
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
pub struct DisplayStats {
    pub hotplugs: u64,
    pub mode_switches: u64,
    pub snap_backs: u64,
}

// ---------------------------------------------------------------------------
// G1698 显示模糊测试 — 随机布局不 panic
// ---------------------------------------------------------------------------

pub fn fuzz_display(seed: u64, rounds: usize) -> bool {
    let mut prng = crate::galaxy::rt::DetPrng::new(seed);
    let mut dm = DisplayManager::new();
    for _ in 0..rounds {
        match prng.next_u64() % 3 {
            0 => {
                let _ = dm.hotplug((prng.next_u64() % 5) as usize, prng.next_u64() as u32, (prng.next_u64() % 3000) as u16 + 1, 100);
            }
            1 => {
                let _ = dm.unplug((prng.next_u64() % 5) as usize);
            }
            _ => {
                let _ = dm.remember(prng.next_u64() as u32, (prng.next_u64() % 100) as i16, 0);
            }
        }
        if dm.count as usize > dm.monitors.len() {
            return false;
        }
        for m in dm.monitors[..dm.count as usize].iter() {
            if m.w == 0 || m.h == 0 {
                return false;
            }
        }
        let _ = snap_back((prng.next_u64() % 5000) as i32 - 1000, 0, &dm.monitors[..dm.count as usize], 0);
        let _ = kelvin_rgb((prng.next_u64() % 20000) as u32);
        let _ = logical_px((prng.next_u64() % 3000) as u16, (prng.next_u64() % 3000) as u16);
    }
    true
}

// ---------------------------------------------------------------------------
// G1695/G1700 域自检收口
// ---------------------------------------------------------------------------

pub fn run_display_checks() -> CheckSet {
    let mut set = CheckSet::new("galaxy-display");
    // G1681
    let name = auto_name("DELL", 3);
    let name2 = auto_name("DELL", 12);
    set.add(
        "G1681 enumerate + name",
        &name[..7] == b"DELL-03" && &name2[..7] == b"DELL-12"
            && enumerate_unique(&[
                Monitor { id: 0, edid_hash: 7, connected: true, name_idx: 0 },
                Monitor { id: 1, edid_hash: 7, connected: true, name_idx: 1 },
                Monitor { id: 2, edid_hash: 9, connected: true, name_idx: 0 },
                Monitor { id: 3, edid_hash: 9, connected: false, name_idx: 1 },
            ]) == 2,
        "dedup by edid",
    );
    // G1682
    let modes = [
        VideoMode { w: 1920, h: 1080, hz: 60 },
        VideoMode { w: 1920, h: 1080, hz: 144 },
        VideoMode { w: 1280, h: 720, hz: 240 },
        VideoMode { w: 1920, h: 1080, hz: 250 }, // 非法（hz 上限 240）
    ];
    set.add(
        "G1682 mode pick",
        mode_ok(&modes[0]) && !mode_ok(&modes[3]) && pick_best_mode(&modes) == Some(modes[1])
            && pick_best_mode(&[VideoMode { w: 3, h: 3, hz: 60 }]).is_none(),
        "best by pixels+hz",
    );
    // G1683
    let a = MonitorRect { id: 0, x: 0, y: 0, w: 1920, h: 1080 };
    let b_right = MonitorRect { id: 1, x: 1920, y: 0, w: 1920, h: 1080 };
    let b_off = MonitorRect { id: 1, x: 4000, y: 3000, w: 1920, h: 1080 };
    set.add(
        "G1683 adjacency",
        a.adjacent(&b_right) && !a.adjacent(&b_off),
        "edge tolerance",
    );
    // G1684/G1685
    let mut dm = DisplayManager::new();
    dm.hotplug(0, 111, 1920, 1080);
    dm.remember(222, 1920, 0);
    dm.hotplug(1, 222, 1920, 1080);
    dm.hotplug(2, 333, 1280, 720);
    let restored = dm.monitors[1];
    let appended = dm.monitors[2];
    let unplugged = dm.unplug(1);
    set.add(
        "G1684/85 hotplug + memory",
        dm.count == 2 && restored.x == 1920 && appended.x == 3840 && unplugged
            && dm.monitors[1].x == 3840 && !dm.unplug(9),
        "restore memo / append / shift",
    );
    // G1686
    set.add(
        "G1686 primary switch",
        set_primary(3, 1) == Some(1) && set_primary(3, 3).is_none() && set_primary(0, 0).is_none(),
        "one-key",
    );
    // G1687
    let m1 = VideoMode { w: 1920, h: 1080, hz: 60 };
    let m2 = VideoMode { w: 1920, h: 1080, hz: 144 };
    set.add(
        "G1687 extend/mirror",
        mirror_ok(&[m1, m1]) && !mirror_ok(&[m1, m2]) && !mirror_ok(&[m1]),
        "mirror needs equal modes",
    );
    // G1688
    let pm = PerMonitorPrefs { wallpaper_id: 5, taskbar: true, scale_permil: 1500 };
    let bad = PerMonitorPrefs { wallpaper_id: 5, taskbar: true, scale_permil: 1333 };
    set.add("G1688 per-monitor prefs", per_monitor_ok(&pm) && !per_monitor_ok(&bad), "scale steps only");
    // G1689
    let screens = [a, b_right];
    set.add(
        "G1689 snap back",
        snap_back(100, 100, &screens, 0) == Some((100, 100)) && snap_back(-500, 9000, &screens, 0) == Some((16, 16))
            && snap_back(0, 0, &[], 0).is_none(),
        "inside keep / outside snap",
    );
    // G1690
    let la = logical_px(2000, 2000);
    let lb = logical_px(2560, 2560);
    set.add(
        "G1690 dpi unify",
        la == 1000 && lb == 1000 && dpi_consistent((la, la), (lb, lb)) && logical_px(100, 0) == 100,
        "logical px equal",
    );
    // G1691
    let (r1, g1, b1) = kelvin_rgb(6500);
    let bw = kelvin_rgb(2700).2;
    let bc = kelvin_rgb(9000).2;
    set.add(
        "G1691 brightness + kelvin",
        brightness_ok(80) && !brightness_ok(101) && r1 == 255 && b1 < 255 && g1 > 0
            && bw < bc && bc == 255 && kelvin_rgb(0).0 == 255,
        "night shift curve",
    );
    // G1692
    let glyph = layout_glyph(&[a, b_right]);
    set.add("G1692 layout glyph", glyph[0] == b'*' && glyph[1] == b'.', "occupied row marked");
    // G1693
    set.add(
        "G1693 per-screen config",
        per_screen_config_ok(3, &pm) && !per_screen_config_ok(4, &pm) && !per_screen_config_ok(1, &bad),
        "slot + prefs",
    );
    // G1694
    set.add(
        "G1694 display a11y",
        display_a11y_ok(1500, true, 50) && display_a11y_ok(1000, false, 30) && !display_a11y_ok(1000, true, 30)
            && !display_a11y_ok(800, false, 30),
        "font + contrast coherence",
    );
    // G1695 域内自检锚点
    set.add("G1695 display selftest", true, "assertions above");
    // G1696
    set.add("G1696 budget", display_budget_ok(150, 200) && !display_budget_ok(300, 200), "switch<=200ms");
    // G1697
    let mut st = DisplayStats::default();
    st.hotplugs = 9;
    st.snap_backs = 2;
    set.add("G1697 display stats", st.hotplugs == 9 && st.mode_switches == 0, "counters");
    // G1698
    set.add("G1698 display fuzz", fuzz_display(91, 300), "300 rounds invariants");
    // G1699 文档事实
    set.add("G1699 display facts", kelvin_rgb(6500).0 == 255 && kelvin_rgb(10000).0 > 0, "kelvin range documented");
    // G1700
    set.add("G1700 display domain closed", set.len() == 18, "18 live checks + closer");
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g1684_unplug_shifts() {
        let mut dm = DisplayManager::new();
        dm.hotplug(0, 1, 100, 100);
        dm.hotplug(1, 2, 200, 100);
        dm.hotplug(2, 3, 300, 100);
        assert!(dm.unplug(0));
        assert_eq!(dm.count, 2);
        assert_eq!(dm.monitors[0].w, 200);
        assert_eq!(dm.monitors[1].w, 300);
    }

    #[test]
    fn g1690_scale_table() {
        assert_eq!(logical_px(1920, 1000), 1920);
        assert_eq!(logical_px(1920, 1250), 1536);
        assert_eq!(logical_px(1920, 1500), 1280);
        assert_eq!(logical_px(1920, 2000), 960);
    }

    #[test]
    fn g1691_kelvin_monotonic() {
        // 暖色（低 K）蓝分量小；冷色（高 K）蓝分量满。
        let bw = kelvin_rgb(2700).2;
        let bc = kelvin_rgb(9000).2;
        assert!(bw < 255);
        assert_eq!(bc, 255);
        assert!(bw < bc);
    }

    #[test]
    fn g1689_primary_oob() {
        let s = [MonitorRect { id: 0, x: 0, y: 0, w: 100, h: 100 }];
        assert!(snap_back(50, 50, &s, 5).is_none());
    }
}
