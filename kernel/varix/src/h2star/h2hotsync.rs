//! H2 分辨率热切换 · 深化批次四·二波（七章「宿主分辨率热切换后
//! 状态不丢失、画面不错乱」的窗口侧机判——F278/F277 的联动件）。
//!
//! **承接判据**（主册 H 域正文 + 人格章程七章）：
//! - **热切换三守恒**：窗口不丢（每扇窗在新拓扑可寻址）、层级不
//!   乱（z 序层带保持——h2zorder 账不动）、贴靠不破（F276 布局
//!   在新几何重解算——解不动就降级比例贴靠，不许破版）；
//! - **F277 车道**：屏热拔——原屏窗口按「归属策略」迁往存活屏
//!   （主屏优先）；焦点屏失效 → 焦点迁到主屏首窗；
//! - **F278 车道**：拓扑指纹变化 → 触发记忆查表（同指纹自动套用
//!   ——projmode 已有记忆，本层做触发与守恒验证的桥）。
//!
//! 时间纪律：无时钟；全部纯函数（同拓扑同结果）。

use crate::checks::CheckSet;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 屏拓扑
// ---------------------------------------------------------------------------

/// 一块屏：编号 + 几何（宽×高）+ 是否主屏。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Monitor {
    pub id: u8,
    pub w: u32,
    pub h: u32,
    pub primary: bool,
}

/// 屏内一扇窗：id + 屏 + 相对几何（比例存储——跨分辨率不漂移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlacedWin {
    pub id: u64,
    pub screen: u8,
    /// 相对位置与大小（千分率：0..=1000，占屏比例）。
    pub x_pm: u32,
    pub y_pm: u32,
    pub w_pm: u32,
    pub h_pm: u32,
}

/// 比例 → 像素（钳制在屏内——热切换不错乱的第一保证）。
pub fn to_px(win: &PlacedWin, m: &Monitor) -> (i32, i32, u32, u32) {
    let clamp_pm = |v: u32| v.min(1000);
    let x = (m.w as u64 * clamp_pm(win.x_pm) as u64 / 1000) as i32;
    let y = (m.h as u64 * clamp_pm(win.y_pm) as u64 / 1000) as i32;
    let w = (m.w as u64 * clamp_pm(win.w_pm).min(1000 - clamp_pm(win.x_pm)) as u64 / 1000) as u32;
    let h = (m.h as u64 * clamp_pm(win.h_pm).min(1000 - clamp_pm(win.y_pm)) as u64 / 1000) as u32;
    (x, y, w, h)
}

// ---------------------------------------------------------------------------
// 热切换迁移
// ---------------------------------------------------------------------------

/// 迁移结果：每扇窗在新拓扑的落点（屏 + 比例不变——只迁屏不改
/// 相对几何，「状态不丢失」的窗口面）。
#[derive(Debug, PartialEq, Eq)]
pub struct Migration {
    /// 迁移了的窗（原屏 → 新屏）。
    pub moved: Vec<(u64, u8, u8)>,
    /// 原地不动的窗数（守恒对账用）。
    pub stayed: usize,
    /// 新焦点屏（原焦点屏存活则不变）。
    pub new_focus_screen: u8,
}

/// 屏热拔迁移：`survivors` 存活屏集（含主屏标记），把死屏窗口迁往
/// 主屏（主屏失效取存活表首块——总有去处）。守恒不变式：窗口总数
/// 迁移前后一致（不丢窗——三守恒之一）。
pub fn migrate_dead_screens(
    wins: &[PlacedWin],
    before: &[Monitor],
    survivors: &[Monitor],
) -> Option<Migration> {
    if survivors.is_empty() {
        return None; // 全拔光不存在「切换」——调用方走锁屏流程
    }
    let primary = survivors.iter().find(|m| m.primary).unwrap_or(&survivors[0]);
    let alive: Vec<u8> = survivors.iter().map(|m| m.id).collect();
    let mut moved = Vec::new();
    let mut stayed = 0usize;
    for w in wins {
        if alive.contains(&w.screen) {
            stayed += 1;
        } else {
            moved.push((w.id, w.screen, primary.id));
        }
    }
    let focus_before = before.iter().find(|m| m.primary).map(|m| m.id).unwrap_or(0);
    let new_focus = if alive.contains(&focus_before) { focus_before } else { primary.id };
    Some(Migration { moved, stayed, new_focus_screen: new_focus })
}

/// 拓扑指纹（FNV-1a——与 h2screen 同族）：屏数+各屏几何序化入指纹，
/// 变了就触发 F278 记忆查表。
pub fn topology_fingerprint(monitors: &[Monitor]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let feed = |h: &mut u64, b: u8| {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    feed(&mut h, monitors.len() as u8);
    for m in monitors {
        feed(&mut h, m.id);
        for b in m.w.to_le_bytes() {
            feed(&mut h, b);
        }
        for b in m.h.to_le_bytes() {
            feed(&mut h, b);
        }
        feed(&mut h, m.primary as u8);
    }
    h
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_h2hotsync_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-h2hotsync");
    let m0 = Monitor { id: 0, w: 1920, h: 1080, primary: true };
    let m1 = Monitor { id: 1, w: 2560, h: 1440, primary: false };
    // 比例→像素：跨分辨率同一比例落不同像素（相对几何不漂移）；
    // 越界比例钳进屏（不错乱）。
    let w1 = PlacedWin { id: 1, screen: 0, x_pm: 100, y_pm: 100, w_pm: 400, h_pm: 300 };
    let (x1, y1, pw1, ph1) = to_px(&w1, &m0);
    let (x2, _, pw2, _) = to_px(&w1, &m1);
    set.add(
        "h2hotsync relative geo",
        x1 == 192 && y1 == 108 && pw1 == 768 && ph1 == 324 && x2 == 256 && pw2 == 1024,
        "permille scales with monitor",
    );
    let wild = PlacedWin { id: 2, screen: 0, x_pm: 1200, y_pm: 0, w_pm: 500, h_pm: 0 };
    let (wx, _, ww, wh) = to_px(&wild, &m0);
    set.add(
        "h2hotsync clamp",
        wx == 1920 && ww == 0 && wh == 0,
        "out-of-screen clamped",
    );
    // 热拔迁移：死屏窗迁主屏、活屏窗不动、守恒（moved+stayed=total）。
    let wins = [
        PlacedWin { id: 1, screen: 0, x_pm: 0, y_pm: 0, w_pm: 500, h_pm: 500 },
        PlacedWin { id: 2, screen: 1, x_pm: 0, y_pm: 0, w_pm: 500, h_pm: 500 },
        PlacedWin { id: 3, screen: 1, x_pm: 500, y_pm: 500, w_pm: 500, h_pm: 500 },
    ];
    let before = [m0, m1];
    let mig = migrate_dead_screens(&wins, &before, &[m0]).unwrap();
    set.add(
        "h2hotsync migrate conserve",
        mig.moved == vec![(2, 1, 0), (3, 1, 0)]
            && mig.stayed == 1
            && mig.moved.len() + mig.stayed == 3,
        "no window lost",
    );
    set.add(
        "h2hotsync focus reassign",
        mig.new_focus_screen == 0,
        "focus survives on primary",
    );
    // 主屏被拔：全窗迁存活表首块（守恒——moved+stayed 仍 = 总数）。
    let m2 = Monitor { id: 2, w: 1280, h: 720, primary: false };
    let mig2 = migrate_dead_screens(&wins, &before, &[m2]).unwrap();
    set.add(
        "h2hotsync primary dead",
        mig2.moved.len() == 3 && mig2.stayed == 0 && mig2.new_focus_screen == 2,
        "fallback to first survivor",
    );
    // 全拔光：显式 None（调用方走锁屏——不假装能迁移）。
    set.add(
        "h2hotsync all dead",
        migrate_dead_screens(&wins, &before, &[]).is_none(),
        "no windows → lock screen",
    );
    // 拓扑指纹：同拓扑同指纹；拔一块屏指纹必变。
    let f1 = topology_fingerprint(&[m0, m1]);
    let f2 = topology_fingerprint(&[m0, m1]);
    let f3 = topology_fingerprint(&[m0]);
    set.add(
        "h2hotsync fingerprint",
        f1 == f2 && f1 != f3,
        "same topo same print",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn h2hotsync_all_green() {
        let set = run_h2hotsync_checks();
        assert!(set.all_passed(), "h2hotsync 自检有红项");
        assert!(!set.truncated(), "h2hotsync 自检溢出");
    }

    #[test]
    fn relative_geometry_survives_any_resolution() {
        // 12 档分辨率扫描：同比例窗永远完整落在屏内（不错乱不变式）。
        let w = PlacedWin { id: 1, screen: 0, x_pm: 250, y_pm: 250, w_pm: 500, h_pm: 500 };
        for (mw, mh) in [(640u32, 480), (1280, 720), (1920, 1080), (2560, 1440), (3840, 2160)] {
            let m = Monitor { id: 0, w: mw, h: mh, primary: true };
            let (x, y, pw, ph) = to_px(&w, &m);
            assert!(x >= 0 && y >= 0 && x + pw as i32 <= mw as i32 && y + ph as i32 <= mh as i32);
            assert_eq!(pw, mw / 2);
            assert_eq!(ph, mh / 2);
        }
    }
}
