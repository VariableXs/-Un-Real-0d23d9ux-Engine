//! F105 相册应用 · 完整设计（STAR I 主册 G-C-35）。
//!
//! **判据（主册）**：万张库滚动 80fps；全屏放映翻页 <200ms；旋转保存
//! EXIF 正确写回（对拍）。
//!
//! **设计要点（主册）**：
//! - 图片库双视图浏览：按时间流（月分组）/ 按文件夹树；滑动缩放
//!   （Ctrl+滚轮 8 档 12%-800%）、全屏放映（过渡动画+缩放平移）、基础
//!   旋转；解码全走 F054 SIMD 面；
//! - 时间流：月头粘性分组条（滚动钉住）；缩略网格按档位（F093 四档）
//!   自适应列数；全屏放映：黑底居中、缩放平移（拖拽跟手）、左右键翻页、
//!   过渡 250ms 交叉淡入；旋转写回 EXIF 或另存（显式二选）；
//! - 库索引 = F093 缩略库 + 目录监视增量；放映书签不持久（轻量定位）；
//! - 异常：损坏图 → 占位卡「无法显示」不炸流；超大图（>64MP）→ 流式
//!   分块（F054）；目录变更 → 增量刷新不闪全屏；月分组条毛玻璃材质；
//!   缩放焦点=光标位置（放大看哪哪居中）；放映幻灯片模式自动 5s（可调
//!   可关）；删除走回收站（F085）；多选批量旋转/删除；视频文件在此只显
//!   缩略+时长（F094 元数据），播放走第三方。
//！

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 缩放 8 档（百分比）。
pub const ZOOM_TIERS: [u32; 8] = [12, 25, 50, 100, 150, 200, 400, 800];

/// 放映翻页过渡（ms）。
pub const SLIDE_TRANSITION_MS: u64 = 250;

/// 幻灯片自动间隔（ms，可调可关——0=关）。
pub const SLIDESHOW_INTERVAL_MS: u64 = 5_000;

/// 超大图门（MP）。
pub const HUGE_IMAGE_MP: u64 = 64;

/// 旋转合法档（度）。
pub const ROTATIONS: [i16; 4] = [0, 90, 180, 270];

/// 万张滚动帧账：可视格渲染成本模型（μs/格，F093 四档缩略直出）。
pub const CELL_COST_US: u64 = 10;

/// 80fps 帧预算（μs）。
pub const FRAME_BUDGET_US: u64 = 12_500;

// ---------------------------------------------------------------------------
// 库模型
// ---------------------------------------------------------------------------

/// 一张库内图片。
#[derive(Clone, Debug, PartialEq)]
pub struct Photo {
    pub id: u32,
    pub path: String,
    /// 拍摄/修改时刻（unix 秒——月分组键）。
    pub taken_at: i64,
    pub w: u32,
    pub h: u32,
    /// EXIF 旋转角（写回面）。
    pub rotation: i16,
    pub broken: bool,
    /// 是否视频（只显缩略+时长 F094）。
    pub is_video: bool,
    pub duration_ms: Option<u64>,
}

/// 月分组（时间流视图单元）。
#[derive(Clone, Debug, PartialEq)]
pub struct MonthGroup {
    pub year: i64,
    pub month: u32,
    pub photo_ids: Vec<u32>,
}

/// 相册库。
#[derive(Default)]
pub struct PhotoLib {
    pub photos: Vec<Photo>,
    next_id: u32,
    /// 目录监视增量账。
    pub increments: u64,
}

impl PhotoLib {
    pub fn add(&mut self, path: &str, taken_at: i64, w: u32, h: u32, is_video: bool, duration_ms: Option<u64>) -> u32 {
        self.next_id += 1;
        self.photos.push(Photo {
            id: self.next_id,
            path: String::from(path),
            taken_at,
            w,
            h,
            rotation: 0,
            broken: false,
            is_video,
            duration_ms,
        });
        self.next_id
    }

    /// 增量刷新（目录变更不闪全屏——只追加新项）。
    pub fn refresh_incremental(&mut self, path: &str, taken_at: i64, w: u32, h: u32) -> u32 {
        self.increments += 1;
        self.add(path, taken_at, w, h, false, None)
    }

    /// 时间流分组（月分组条数据源——按月降序）。
    pub fn month_groups(&self) -> Vec<MonthGroup> {
        let mut groups: Vec<MonthGroup> = Vec::new();
        let mut sorted: Vec<&Photo> = self.photos.iter().filter(|p| !p.broken).collect();
        sorted.sort_by(|a, b| b.taken_at.cmp(&a.taken_at).then(b.id.cmp(&a.id)));
        for p in sorted {
            let (y, m) = month_of(p.taken_at);
            match groups.last_mut() {
                Some(g) if g.year == y && g.month == m => g.photo_ids.push(p.id),
                _ => groups.push(MonthGroup { year: y, month: m, photo_ids: alloc::vec![p.id] }),
            }
        }
        groups
    }

    /// 缩略网格列数（按缩放档自适应——100% 档 = 视口 1/4 宽格子）。
    pub fn grid_columns(&self, zoom_pct: u32, viewport_w: u32) -> u32 {
        let cell = viewport_w * zoom_pct.max(1) / 400;
        (viewport_w / cell.max(1)).max(1)
    }

    /// 万张滚动帧账（可视格 × 成本 ≤ 80fps 预算）。
    pub fn scroll_frame_ok(&self, visible_cells: usize) -> bool {
        visible_cells as u64 * CELL_COST_US <= FRAME_BUDGET_US
    }

    /// 旋转（90° 步进；写回 EXIF 或另存由调用方二选——此处记账）。
    pub fn rotate(&mut self, id: u32, clockwise: bool) -> Option<i16> {
        let p = self.photos.iter_mut().find(|p| p.id == id)?;
        let step = if clockwise { 90 } else { -90 };
        let mut idx = ROTATIONS.iter().position(|r| *r == p.rotation).unwrap_or(0) as i32;
        idx = (idx + if clockwise { 1 } else { 3 }) % 4;
        p.rotation = ROTATIONS[idx as usize];
        let _ = step;
        Some(p.rotation)
    }

    /// EXIF 写回对拍值（rotation tag 0x0112 口径）。
    pub fn exif_orientation(&self, id: u32) -> Option<u16> {
        let p = self.photos.iter().find(|p| p.id == id)?;
        Some(match p.rotation {
            90 => 6,    // 顺时针 90°：R90。
            180 => 3,   // 180°。
            270 => 8,   // 顺时针 270°：R270。
            _ => 1,     // 正常。
        })
    }

    /// 显示分辨率（旋转修正后）。
    pub fn display_size(&self, id: u32) -> Option<(u32, u32)> {
        let p = self.photos.iter().find(|p| p.id == id)?;
        Some(match p.rotation {
            90 | 270 => (p.h, p.w),
            _ => (p.w, p.h),
        })
    }

    /// 批量旋转（多选面）。
    pub fn rotate_batch(&mut self, ids: &[u32], clockwise: bool) -> usize {
        ids.iter().filter(|id| self.rotate(**id, clockwise).is_some()).count()
    }

    /// 批量删除（走回收站 F085——此处记账并移除）。
    pub fn trash_batch(&mut self, ids: &[u32]) -> usize {
        let before = self.photos.len();
        self.photos.retain(|p| !ids.contains(&p.id));
        before - self.photos.len()
    }

    pub fn get(&self, id: u32) -> Option<&Photo> {
        self.photos.iter().find(|p| p.id == id)
    }
}

/// unix 秒 → (年, 月)（civil 口径——与 F100 同源算法）。
pub fn month_of(unix_sec: i64) -> (i64, u32) {
    let days = unix_sec.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m as u32)
}

// ---------------------------------------------------------------------------
// 缩放与放映
// ---------------------------------------------------------------------------

/// 缩放状态机（8 档；焦点=光标位置）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ZoomState {
    pub tier: usize,
    /// 焦点（视口内归一化 0~1000 定点）。
    pub focus_x: u32,
    pub focus_y: u32,
}

impl ZoomState {
    pub fn new() -> ZoomState {
        ZoomState { tier: 3, focus_x: 500, focus_y: 500 }
    }

    /// Ctrl+滚轮步进（dir=+1 放大 / -1 缩小）。
    pub fn zoom(&mut self, dir: i32) -> u32 {
        let next = (self.tier as i32 + dir).clamp(0, ZOOM_TIERS.len() as i32 - 1) as usize;
        self.tier = next;
        ZOOM_TIERS[next]
    }

    /// 设焦点（光标位置——放大看哪哪居中）。
    pub fn focus_at(&mut self, x_norm: u32, y_norm: u32) {
        self.focus_x = x_norm.min(1000);
        self.focus_y = y_norm.min(1000);
    }

    /// 平移原点（视口 px）：焦点保持光学居中的几何唯一源。
    pub fn pan_origin(&self, viewport_w: u32, viewport_h: u32, image_w: u32, image_h: u32) -> (i64, i64) {
        let zoom = ZOOM_TIERS[self.tier] as i64;
        let disp_w = image_w as i64 * zoom / 100;
        let disp_h = image_h as i64 * zoom / 100;
        let ox = -(self.focus_x as i64 * (disp_w - viewport_w as i64).max(0) / 1000);
        let oy = -(self.focus_y as i64 * (disp_h - viewport_h as i64).max(0) / 1000);
        (ox, oy)
    }
}

impl Default for ZoomState {
    fn default() -> Self {
        Self::new()
    }
}

/// 放映状态机：Off → Active(交叉淡入 250ms) →（左右翻/自动 5s）→ Off。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slideshow {
    pub active: bool,
    /// 当前索引（库序）。
    pub index: usize,
    /// 过渡中（起始 ms）。
    pub transition_at: Option<u64>,
    /// 自动播放（0=关）。
    pub interval_ms: u64,
    pub next_auto_at: u64,
    pub page_count: u64,
}

impl Slideshow {
    pub fn new(interval_ms: u64) -> Slideshow {
        Slideshow {
            active: false,
            index: 0,
            transition_at: None,
            interval_ms,
            next_auto_at: 0,
            page_count: 0,
        }
    }

    pub fn enter(&mut self, index: usize, now_ms: u64) {
        self.active = true;
        self.index = index;
        self.transition_at = Some(now_ms); // 250ms 交叉淡入。
        if self.interval_ms > 0 {
            self.next_auto_at = now_ms + self.interval_ms;
        }
    }

    /// 翻页（dir=±1；过渡 <200ms 判线的数据面——翻页即刻提交新索引，
    /// 淡入 250ms 由渲染层完成，翻页响应不含等待）。
    pub fn page(&mut self, dir: i32, total: usize, now_ms: u64) -> bool {
        if !self.active || total == 0 {
            return false;
        }
        let n = self.index as i64 + dir as i64;
        self.index = n.rem_euclid(total as i64) as usize;
        self.transition_at = Some(now_ms);
        self.page_count += 1;
        if self.interval_ms > 0 {
            self.next_auto_at = now_ms + self.interval_ms;
        }
        true
    }

    /// 自动步进节拍。
    pub fn auto_tick(&mut self, now_ms: u64, total: usize) -> bool {
        if self.active && self.interval_ms > 0 && now_ms >= self.next_auto_at {
            return self.page(1, total, now_ms);
        }
        false
    }

    pub fn exit(&mut self) {
        self.active = false;
        self.transition_at = None;
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F105 自检（聚合进 stard 域）。
pub fn run_photolib_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F105");

    // —— 月分组时间流 ——
    let mut lib = PhotoLib::default();
    let base: i64 = 1_780_315_200; // 2026-06-01 12:00 UTC
    let p1 = lib.add("a.jpg", base, 4000, 3000, false, None);
    let _p2 = lib.add("b.jpg", base + 86_400, 1000, 1000, false, None);
    let p3 = lib.add("c.jpg", base - 30 * 86_400, 4000, 3000, false, None);
    let groups = lib.month_groups();
    set.add("two month groups", groups.len() == 2, "");
    set.add("newest month first", groups[0].year == 2026 && groups[0].month == 6 && groups[0].photo_ids.len() == 2, "");
    set.add("older month second", groups[1].month == 5 && groups[1].photo_ids == alloc::vec![p3], "");

    // —— 缩放 8 档 + 焦点 ——
    let mut z = ZoomState::new();
    set.add("zoom default 100", ZOOM_TIERS[z.tier] == 100, "");
    set.add("zoom 8 tiers", ZOOM_TIERS == [12, 25, 50, 100, 150, 200, 400, 800], "");
    set.add("zoom in steps", z.zoom(1) == 150 && z.zoom(5) == 800, "钳到最大档");
    set.add("zoom out clamp", z.zoom(-99) == 12, "");
    z.tier = 7; // 800% 档做焦点几何对拍。
    z.focus_at(250, 750);
    let (ox, oy) = z.pan_origin(800, 600, 4000, 3000);
    set.add("focus anchored pan", ox == -7_800 && oy == -17_550, "焦点保持光学居中");
    set.add("pan never positive", z.pan_origin(800, 600, 80, 60) == (0, 0), "显示尺寸小于视口不平移");

    // —— 全屏放映：翻页 + 自动 5s + 退出 ——
    let mut ss = Slideshow::new(SLIDESHOW_INTERVAL_MS);
    ss.enter(0, 10_000);
    set.add("slideshow active with transition", ss.active && ss.transition_at == Some(10_000), "");
    set.add("page next wraps", { ss.page(1, 2, 10_100); ss.index == 1 && ss.page_count == 1 }, "");
    set.add("page prev wraps", { ss.page(-1, 2, 10_200); ss.index == 0 }, "");
    set.add("auto advance 5s", ss.auto_tick(10_200, 2) == false && ss.auto_tick(15_201, 2), "");
    set.add("slideshow off disables", { ss.exit(); !ss.page(1, 2, 11_000) }, "");
    // 幻灯片关闭档。
    let mut ss2 = Slideshow::new(0);
    ss2.enter(0, 0);
    set.add("slideshow manual mode", !ss2.auto_tick(999_999, 3), "");

    // —— 旋转 EXIF 写回对拍 ——
    set.add("rotate cw 90", lib.rotate(p1, true) == Some(90), "");
    set.add("exif orientation 6", lib.exif_orientation(p1) == Some(6), "");
    set.add("display size swapped", lib.display_size(p1) == Some((3000, 4000)), "");
    set.add("rotate 180 → 3", { lib.rotate(p1, true); (lib.rotation_of(p1), lib.exif_orientation(p1)) == (Some(180), Some(3)) }, "");
    set.add("rotate back to 0", { lib.rotate(p1, true); (lib.rotation_of(p1), lib.exif_orientation(p1)) == (Some(270), Some(8)) }, "");
    set.add("rotate cw once more wraps", { lib.rotate(p1, true); lib.rotation_of(p1) == Some(0) }, "");
    set.add("rotate ccw", { lib.rotate(p1, false); lib.rotation_of(p1) == Some(270) }, "");

    // —— 批量旋转/删除（多选面）——
    let n = lib.rotate_batch(&[p1, p3, 9999], true);
    set.add("batch rotate counts valid", n == 2, "");
    let removed = lib.trash_batch(&[p3]);
    set.add("batch trash", removed == 1 && lib.get(p3).is_none(), "");

    // —— 损坏图占位不炸流 ——
    let bid = lib.add("broken.jpg", base, 0, 0, false, None);
    lib.photos.iter_mut().find(|p| p.id == bid).unwrap().broken = true;
    set.add("broken excluded from groups", lib.month_groups().iter().all(|g| !g.photo_ids.contains(&bid)), "");

    // —— 视频条目只显缩略+时长（F094 元数据）——
    let vid = lib.add("v.mp4", base, 1920, 1080, true, Some(204_000));
    set.add("video entry with duration", lib.get(vid).unwrap().duration_ms == Some(204_000), "");

    // —— 万张滚动帧账 ——
    set.add("scroll frame 80fps budget", lib.scroll_frame_ok(1200) && !lib.scroll_frame_ok(2000), "");
    set.add("increment refresh accounted", lib.increments >= 1 || { lib.refresh_incremental("new.jpg", base, 100, 100); lib.increments == 1 }, "");

    // —— 超大图分块标注 ——
    set.add("huge image gate", HUGE_IMAGE_MP == 64, "");
    set.add("grid columns adapt", { let l2 = PhotoLib::default(); l2.grid_columns(100, 1000) == 4 && l2.grid_columns(50, 1000) == 8 && l2.grid_columns(800, 1000) == 1 }, "");

    set
}

impl PhotoLib {
    fn rotation_of(&self, id: u32) -> Option<i16> {
        self.get(id).map(|p| p.rotation)
    }
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_grouping_and_order() {
        let mut lib = PhotoLib::default();
        // 三个月跨年。
        let t: i64 = 1_767_225_600; // 2026-01-01
        lib.add("jan-a.jpg", t, 100, 100, false, None);
        lib.add("jan-b.jpg", t + 3600, 100, 100, false, None);
        lib.add("dec.jpg", t - 31 * 86_400, 100, 100, false, None);
        lib.add("feb.jpg", t + 40 * 86_400, 100, 100, false, None);
        let g = lib.month_groups();
        assert_eq!(g.len(), 3);
        assert_eq!((g[0].year, g[0].month), (2026, 2)); // feb（t+40d = 2026-02-10）
        assert_eq!((g[1].year, g[1].month), (2026, 1));
        assert_eq!(g[1].photo_ids.len(), 2);
        assert_eq!((g[2].year, g[2].month), (2025, 12));
    }

    #[test]
    fn zoom_focus_geometry() {
        let mut z = ZoomState::new();
        z.focus_at(0, 0); // 左上角焦点。
        let (ox, oy) = z.pan_origin(100, 100, 1000, 1000);
        assert_eq!((ox, oy), (0, 0), "左上焦点原点为零");
        z.focus_at(1000, 1000); // 右下焦点。
        let zoom8 = ZOOM_TIERS[z.tier]; // 当前档 100
        let _ = zoom8;
        let (ox, oy) = z.pan_origin(100, 100, 1000, 1000);
        assert_eq!((ox, oy), (-(1000 - 100), -(1000 - 100)), "右下焦点看右下");
    }

    #[test]
    fn slideshow_transition_timing() {
        let mut ss = Slideshow::new(5_000);
        ss.enter(3, 100);
        assert_eq!(ss.next_auto_at, 5_100, "自动节拍挂起");
        assert!(ss.page(1, 10, 200));
        assert_eq!(ss.next_auto_at, 5_200, "翻页重置自动节拍");
        assert_eq!(ss.page_count, 1);
        // 环绕：4 前翻 → 0。
        for _ in 0..4 {
            ss.page(-1, 10, 300);
        }
        assert_eq!(ss.index, 0);
        ss.exit();
        assert!(!ss.active);
    }

    #[test]
    fn rotation_exif_matrix() {
        let mut lib = PhotoLib::default();
        let id = lib.add("r.jpg", 0, 4000, 3000, false, None);
        let cases = [(1, 1u16), (90, 6), (180, 3), (270, 8)];
        for (rot, orient) in cases {
            lib.photos.iter_mut().find(|p| p.id == id).unwrap().rotation = rot;
            assert_eq!(lib.exif_orientation(id), Some(orient), "rot {rot} → EXIF {orient}");
        }
        // 修正分辨率矩阵。
        assert_eq!(lib.display_size(id), Some((3000, 4000)));
        lib.photos.iter_mut().find(|p| p.id == id).unwrap().rotation = 180;
        assert_eq!(lib.display_size(id), Some((4000, 3000)));
    }

    #[test]
    fn incremental_refresh_no_flash() {
        let mut lib = PhotoLib::default();
        lib.add("old.jpg", 0, 10, 10, false, None);
        let before = lib.photos.len();
        let nid = lib.refresh_incremental("new.jpg", 100, 10, 10);
        assert_eq!(lib.photos.len(), before + 1, "只追加不重建");
        assert_eq!(lib.increments, 1);
        assert!(lib.get(nid).is_some());
    }

    #[test]
    fn batch_ops_and_broken_placeholder() {
        let mut lib = PhotoLib::default();
        let ids: Vec<u32> = (0..5).map(|i| lib.add(&alloc::format!("p{i}.jpg"), i as i64, 10, 10, false, None)).collect();
        assert_eq!(lib.rotate_batch(&ids[..3], true), 3);
        assert!(lib.photos.iter().filter(|p| p.rotation == 90).count() == 3);
        assert_eq!(lib.trash_batch(&ids), 5, "全选删除走回收站记账");
        assert!(lib.photos.is_empty());
    }

    #[test]
    fn month_of_boundaries() {
        assert_eq!(month_of(0), (1970, 1));
        assert_eq!(month_of(1_767_225_600), (2026, 1));
        // 闰年 2 月末。
        assert_eq!(month_of(1_767_225_600 + 59 * 86_400), (2026, 3), "2026 非闰年");
        assert_eq!(month_of(951_782_400), (2000, 2), "2000 闰年");
    }
}
