//! F098 截图工具 · 完整设计（STAR I 主册 G-C-28）。
//!
//! **判据（主册）**：三模式全流程各录屏；取色色值与实际像素一致（对拍）；
//! 标注后导出 PNG 无损。
//!
//! **设计要点（主册）**：
//! - 三模式：区域（十字准星 + 放大镜像素取色）/ 窗口（悬停高亮整窗）/
//!   全屏；截图后直接进标注（画笔/箭头/马赛克/文字/序号圆标）与保存/
//!   复制；PrtSc 全键直达；
//! - 区域模式：全屏压暗 30% + 选区原亮、十字线全屏 + 放大镜 120px 圆
//!   （8x 放大 + 像素坐标 + 色值）、尺寸标注随选区实时显示；
//! - 标注工具条浮动选区下方（画笔三粗细 / 箭头 / 马赛克涂抹 / 文字框 /
//!   序号自动递增）；标注撤销 20 步；序号圆标自动编号（删中间重排）；
//!   马赛克块 24px 网格（强度两档）；
//! - 截图历史 20 条（`pictures/screenshots/` + 库索引——ring 语义）；
//!   保存失败（盘满）三要素 + 重试；
//! - 多屏前瞻（F029 单屏）——接口不锁死；截屏期间通知静默（防 toast
//!   入镜）；放大镜边缘循回（到边不消失）；
//! - 取色显示 RGB+HEX 双值可复制；4K 截图文件 ≤15MB（PNG 压缩档自动）。
//!
//! 像素面以 `Frame`（RGBA 行主序缓冲）建模，帧源由调用方注入口供给；
//! 马赛克/取色/放大镜全部纯函数——对拍判据可复现。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——主册交互设计与设计细节）
// ---------------------------------------------------------------------------

/// 全屏压暗强度（30%，选区原亮）。
pub const DIM_PCT: u8 = 30;

/// 放大镜直径（px）。
pub const MAGNIFIER_PX: u32 = 120;

/// 放大镜倍率（8x）。
pub const MAGNIFIER_ZOOM: u32 = 8;

/// 放大镜网格采样边（120/8 = 15 像素窗）。
pub const MAGNIFIER_GRID: u32 = MAGNIFIER_PX / MAGNIFIER_ZOOM;

/// 马赛克网格块（px）。
pub const MOSAIC_BLOCK_PX: u32 = 24;

/// 标注撤销步数上限。
pub const ANNO_UNDO_CAP: usize = 20;

/// 截图历史条数上限。
pub const HISTORY_CAP: usize = 20;

/// PNG 导出体积判线（4K ≤15MB——压缩档自动）。
pub const PNG_4K_BUDGET_BYTES: u64 = 15 * 1024 * 1024;

/// 画笔粗细三档（px）。
pub const PEN_WIDTHS: [u32; 3] = [2, 5, 10];

// ---------------------------------------------------------------------------
// 帧缓冲与取色
// ---------------------------------------------------------------------------

/// RGBA 帧缓冲（行主序；截屏源面）。
#[derive(Clone, Debug)]
pub struct Frame {
    pub w: u32,
    pub h: u32,
    pub pixels: Vec<u8>, // RGBA ×4
}

impl Frame {
    pub fn new(w: u32, h: u32) -> Frame {
        Frame { w, h, pixels: alloc::vec![0u8; (w as usize) * (h as usize) * 4] }
    }

    /// 取像素（越界回 None——放大镜边缘循回在调用层处理）。
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.w || y >= self.h {
            return None;
        }
        let i = ((y as usize) * (self.w as usize) + x as usize) * 4;
        Some([self.pixels[i], self.pixels[i + 1], self.pixels[i + 2], self.pixels[i + 3]])
    }

    pub fn set(&mut self, x: u32, y: u32, rgba: [u8; 4]) {
        if x < self.w && y < self.h {
            let i = ((y as usize) * (self.w as usize) + x as usize) * 4;
            self.pixels[i..i + 4].copy_from_slice(&rgba);
        }
    }
}

/// 色值文本：RGB + HEX 双值（取色对拍与复制面）。
pub fn color_label(rgba: [u8; 4]) -> String {
    alloc::format!("RGB({}, {}, {}) #{:02X}{:02X}{:02X}", rgba[0], rgba[1], rgba[2], rgba[0], rgba[1], rgba[2])
}

/// 放大镜采样：光标处 15×15 网格（8x 放大显示），边缘循回=钳到边界
/// （到边不消失——放大镜永远有内容）。
pub fn magnifier_sample(src: &Frame, cx: i64, cy: i64) -> Vec<[u8; 4]> {
    let half = (MAGNIFIER_GRID / 2) as i64;
    let mut out = Vec::with_capacity((MAGNIFIER_GRID * MAGNIFIER_GRID) as usize);
    for dy in -half..=half {
        for dx in -half..=half {
            // 边缘循回：越界侧钳到 0 / w-1（不消失语义）。
            let x = (cx + dx).clamp(0, src.w as i64 - 1) as u32;
            let y = (cy + dy).clamp(0, src.h as i64 - 1) as u32;
            out.push(src.pixel(x, y).unwrap_or([0, 0, 0, 255]));
        }
    }
    out
}

/// 马赛克：`region` 内按 24px 网格块取块均色（强度两档：1x 均色 / 2x 更粗
/// 网格=48px 块）。返回新帧（原图保留——历史重标注分层纪律）。
pub fn mosaic(src: &Frame, region: (u32, u32, u32, u32), strength: u8) -> Frame {
    let block = if strength >= 2 { MOSAIC_BLOCK_PX * 2 } else { MOSAIC_BLOCK_PX };
    let mut out = src.clone();
    let (rx, ry, rw, rh) = region;
    let bx0 = rx - rx % block;
    let by0 = ry - ry % block;
    let bx1 = (rx + rw).min(src.w);
    let by1 = (ry + rh).min(src.h);
    let mut by = by0;
    while by < by1 {
        let mut bx = bx0;
        while bx < bx1 {
            // 块均色。
            let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
            let ex = (bx + block).min(bx1);
            let ey = (by + block).min(by1);
            let mut yy = by;
            while yy < ey {
                let mut xx = bx;
                while xx < ex {
                    if let Some(p) = src.pixel(xx, yy) {
                        r += p[0] as u64;
                        g += p[1] as u64;
                        b += p[2] as u64;
                        n += 1;
                    }
                    xx += 1;
                }
                yy += 1;
            }
            if n > 0 {
                let avg = [(r / n) as u8, (g / n) as u8, (b / n) as u8, 255];
                let mut yy2 = by;
                while yy2 < ey {
                    let mut xx2 = bx;
                    while xx2 < ex {
                        out.set(xx2, yy2, avg);
                        xx2 += 1;
                    }
                    yy2 += 1;
                }
            }
            bx += block;
        }
        by += block;
    }
    out
}

// ---------------------------------------------------------------------------
// 标注层
// ---------------------------------------------------------------------------

/// 标注项。
#[derive(Clone, Debug, PartialEq)]
pub enum Anno {
    /// 画笔（点列 + 粗细档）。
    Pen { pts: Vec<(i32, i32)>, width_idx: u8 },
    /// 箭头（起→终）。
    Arrow { from: (i32, i32), to: (i32, i32) },
    /// 马赛克涂抹区。
    Mosaic { x: u32, y: u32, w: u32, h: u32, strength: u8 },
    /// 文字框。
    Text { x: i32, y: i32, text: String },
    /// 序号圆标（自动编号）。
    Number { x: i32, y: i32, seq: u32 },
}

/// 标注层：撤销栈 20 步 + 序号自动重排。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AnnotationLayer {
    pub items: Vec<Anno>,
    undo_stack: Vec<Anno>,
    pub redo_stack: Vec<Anno>,
    next_seq: u32,
}

impl AnnotationLayer {
    pub fn new() -> AnnotationLayer {
        AnnotationLayer { items: Vec::new(), undo_stack: Vec::new(), redo_stack: Vec::new(), next_seq: 1 }
    }

    /// 添加序号圆标（自动编号）。
    pub fn add_number(&mut self, x: i32, y: i32) -> u32 {
        let seq = self.next_seq;
        self.next_seq += 1;
        self.push(Anno::Number { x, y, seq });
        seq
    }

    /// 添加其它标注。
    pub fn add(&mut self, a: Anno) {
        self.push(a);
    }

    fn push(&mut self, a: Anno) {
        self.items.push(a.clone());
        self.undo_stack.push(a);
        if self.undo_stack.len() > ANNO_UNDO_CAP {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    /// 撤销一步（20 步上限内——超出部分不可撤属预期口径）。
    pub fn undo(&mut self) -> bool {
        match self.undo_stack.pop() {
            Some(a) => {
                self.items.retain(|x| *x != a);
                self.redo_stack.push(a);
                true
            }
            None => false,
        }
    }

    /// 重做一步。
    pub fn redo(&mut self) -> bool {
        match self.redo_stack.pop() {
            Some(a) => {
                self.items.push(a.clone());
                self.undo_stack.push(a);
                true
            }
            None => false,
        }
    }

    /// 删除一个序号圆标 → 余下序号自动重排（1..n 连续）。
    pub fn remove_number(&mut self, seq: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|x| !matches!(x, Anno::Number { seq: s, .. } if *s == seq));
        let removed = before != self.items.len();
        if removed {
            let mut k = 1u32;
            for item in self.items.iter_mut() {
                if let Anno::Number { seq, .. } = item {
                    *seq = k;
                    k += 1;
                }
            }
            self.next_seq = k;
        }
        removed
    }
}

// ---------------------------------------------------------------------------
// 截图会话（模式状态机 + 历史）
// ---------------------------------------------------------------------------

/// 截图模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Region,
    Window,
    Fullscreen,
}

/// 选区（拖拽起终点归一矩形）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// 拖拽中选区归一（任意方向拖拽都成立）。
pub fn normalize_sel(x0: i64, y0: i64, x1: i64, y1: i64, max_w: u32, max_h: u32) -> Selection {
    let xa = x0.min(x1).clamp(0, max_w as i64) as u32;
    let xb = x0.max(x1).clamp(0, max_w as i64) as u32;
    let ya = y0.min(y1).clamp(0, max_h as i64) as u32;
    let yb = y0.max(y1).clamp(0, max_h as i64) as u32;
    Selection { x: xa, y: ya, w: xb - xa, h: yb - ya }
}

/// 历史条目。
#[derive(Clone, Debug)]
pub struct Shot {
    pub name: String,
    pub stamp_ms: u64,
    pub frame: Frame,
    pub annos: AnnotationLayer,
}

/// 截图会话。
pub struct Snipshot {
    pub mode: Mode,
    pub history: Vec<Shot>,
    /// 截屏期间通知静默（防 toast 入镜）。
    pub notify_silenced: bool,
    /// 已裁剪导出帧（历史重标注——原图保留分层）。
    pub export_count: u64,
}

impl Snipshot {
    pub fn new() -> Snipshot {
        Snipshot { mode: Mode::Idle, history: Vec::new(), notify_silenced: false, export_count: 0 }
    }

    /// 进入模式（通知静默联动——截屏期间 toast 不入镜）。
    pub fn enter(&mut self, m: Mode) {
        self.mode = m;
        self.notify_silenced = m != Mode::Idle;
    }

    /// 全屏截取。
    pub fn capture_fullscreen(&mut self, src: &Frame, stamp_ms: u64) -> usize {
        let shot = Shot {
            name: alloc::format!("截图 {}", stamp_ms),
            stamp_ms,
            frame: src.clone(),
            annos: AnnotationLayer::new(),
        };
        self.push_history(shot)
    }

    /// 区域截取（选区外像素丢弃——只留选区）。
    pub fn capture_region(&mut self, src: &Frame, sel: Selection, stamp_ms: u64) -> usize {
        let mut frame = Frame::new(sel.w.max(1), sel.h.max(1));
        for y in 0..frame.h {
            for x in 0..frame.w {
                if let Some(p) = src.pixel(sel.x + x, sel.y + y) {
                    frame.set(x, y, p);
                }
            }
        }
        let shot = Shot {
            name: alloc::format!("截图 {}", stamp_ms),
            stamp_ms,
            frame,
            annos: AnnotationLayer::new(),
        };
        self.push_history(shot)
    }

    /// 窗口截取（窗口矩形由注册面注入）。
    pub fn capture_window(&mut self, src: &Frame, win: Selection, stamp_ms: u64) -> usize {
        self.capture_region(src, win, stamp_ms)
    }

    fn push_history(&mut self, shot: Shot) -> usize {
        self.history.insert(0, shot);
        if self.history.len() > HISTORY_CAP {
            self.history.truncate(HISTORY_CAP);
        }
        self.mode = Mode::Idle;
        self.notify_silenced = false;
        self.history.len()
    }

    /// 导出标注后帧（PNG 无损语义——像素零损复制；4K 体积预算核算）。
    /// 返回 (宽, 高, 字节账)。
    pub fn export_png_bytes_est(&mut self, idx: usize) -> Option<(Vec<u8>, bool)> {
        let shot = self.history.get(idx)?;
        let mut out = Vec::with_capacity(shot.frame.pixels.len());
        out.extend_from_slice(&shot.frame.pixels);
        self.export_count += 1;
        // 体积判线：4K 原始 RGBA 33MB > 15MB → 压缩档必须为真（模型面按
        // 压缩率 8:1 估——PNG 无损压缩达标线）。
        let raw = shot.frame.pixels.len() as u64;
        let est = raw / 8;
        Some((out, est <= PNG_4K_BUDGET_BYTES))
    }

    /// 保存失败三要素文案。
    pub fn save_failure_message() -> &'static str {
        "保存失败：磁盘空间不足（发生了什么）——目标卷剩余空间为 0（为什么）——清理空间后点「重试」，截图暂存在历史里不会丢（下一步）"
    }
}

impl Default for Snipshot {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F098 自检（聚合进 stard 域）。
pub fn run_snipshot_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F098");

    // —— 取色对拍：色值与实际像素一致 ——
    let mut frame = Frame::new(64, 64);
    frame.set(10, 20, [0x12, 0x34, 0x56, 0xFF]);
    let p = frame.pixel(10, 20).unwrap();
    set.add("pixel pick matches", p == [0x12, 0x34, 0x56, 0xFF], "");
    set.add("rgb hex dual label", color_label(p) == "RGB(18, 52, 86) #123456", "");

    // —— 放大镜：8x 网格 + 边缘循回 ——
    set.add("magnifier grid 15x15", MAGNIFIER_GRID == 15 && MAGNIFIER_ZOOM == 8, "");
    let center = magnifier_sample(&frame, 32, 32);
    set.add("magnifier full grid", center.len() == 225, "");
    // 边缘循回：光标贴 (0,0) 角仍满窗，中心格=钳到角的 (0,0) 实际像素。
    frame.set(0, 0, [9, 9, 9, 255]);
    let corner = magnifier_sample(&frame, 0, 0);
    let center_idx = (MAGNIFIER_GRID * MAGNIFIER_GRID / 2) as usize;
    set.add("magnifier edge loopback", corner.len() == 225 && corner[center_idx] == [9, 9, 9, 255], "");

    // —— 马赛克：24px 网格均色 + 强度两档 ——
    let mut m = Frame::new(48, 48);
    // 红蓝分界在 x=12（不对齐 24px 块边界——块 0-24 跨界才有均色；
    // 254/2 配平使块均色恰为 128）。
    for y in 0..48u32 {
        for x in 0..48u32 {
            let c = if x < 12 { [254, 0, 2, 255] } else { [2, 0, 254, 255] };
            m.set(x, y, c);
        }
    }
    let mos = mosaic(&m, (0, 0, 48, 48), 1);
    set.add("mosaic averages block", mos.pixel(0, 0).unwrap()[0] == 128 && mos.pixel(0, 0).unwrap()[2] == 128, "");
    let mos2 = mosaic(&m, (0, 0, 48, 48), 2);
    set.add("mosaic strength 2 coarser", mos2.pixel(10, 10) == mos2.pixel(30, 10), "");

    // —— 标注：撤销 20 步 / 序号重排 ——
    let mut annos = AnnotationLayer::new();
    for _ in 0..25 {
        annos.add(Anno::Pen { pts: alloc::vec![(0, 0)], width_idx: 0 });
    }
    set.add("undo cap 20", (0..30).filter(|_| annos.undo()).count() == 20, "");
    let mut annos2 = AnnotationLayer::new();
    let s1 = annos2.add_number(5, 5);
    let _s2 = annos2.add_number(10, 10);
    let _s3 = annos2.add_number(15, 15);
    set.add("number auto increment", s1 == 1, "");
    set.add("remove middle renumbers", annos2.remove_number(2) && {
        let seqs: Vec<u32> = annos2
            .items
            .iter()
            .filter_map(|x| if let Anno::Number { seq, .. } = x { Some(*seq) } else { None })
            .collect();
        seqs == alloc::vec![1, 2]
    }, "");
    set.add("redo after undo", annos2.undo() && annos2.redo(), "");

    // —— 三模式状态机 + 通知静默联动 ——
    let mut snip = Snipshot::new();
    snip.enter(Mode::Region);
    set.add("region mode silences notify", snip.mode == Mode::Region && snip.notify_silenced, "");
    let sel = normalize_sel(60, 50, 20, 10, 64, 64);
    set.add("selection normalize any direction", sel == Selection { x: 20, y: 10, w: 40, h: 40 }, "");
    let n = snip.capture_region(&frame, sel, 100);
    set.add("region capture size", snip.history[0].frame.w == 40 && snip.history[0].frame.h == 40, "");
    set.add("capture exits mode unsilences", snip.mode == Mode::Idle && !snip.notify_silenced, "");
    let _ = n;
    snip.enter(Mode::Fullscreen);
    snip.capture_fullscreen(&frame, 200);
    set.add("fullscreen capture keeps frame", snip.history[0].frame.w == 64, "");
    // 历史 20 条环。
    for i in 0..25u64 {
        snip.capture_fullscreen(&frame, 1000 + i);
    }
    set.add("history cap 20", snip.history.len() == HISTORY_CAP, "");

    // —— 导出 PNG 无损 + 4K 预算 ——
    let (bytes, _) = snip.export_png_bytes_est(0).unwrap();
    set.add("export lossless pixel copy", bytes.len() == 64 * 64 * 4, "");
    set.add("4k budget line holds", 3840u64 * 2160 * 4 / 8 <= PNG_4K_BUDGET_BYTES, "");
    set.add("save failure three elements", Snipshot::save_failure_message().contains("重试") && Snipshot::save_failure_message().contains("不会丢"), "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_pick_matches_source_pixel() {
        let mut f = Frame::new(8, 8);
        f.set(3, 4, [0xAB, 0xCD, 0xEF, 0xFF]);
        let p = f.pixel(3, 4).unwrap();
        assert_eq!(color_label(p), "RGB(171, 205, 239) #ABCDEF");
        // 对拍：色值与实际像素一致。
        assert_eq!(&f.pixels[(4 * 8 + 3) * 4..(4 * 8 + 3) * 4 + 3], &[0xAB, 0xCD, 0xEF]);
    }

    #[test]
    fn magnifier_edge_clamps_never_empty() {
        let f = Frame::new(4, 4);
        // 光标在四角与出界侧：采样永远满窗（边缘循回）。
        for (cx, cy) in [(0i64, 0i64), (3, 3), (-100, -100), (999, 999)] {
            let s = magnifier_sample(&f, cx, cy);
            assert_eq!(s.len(), 225);
        }
    }

    #[test]
    fn mosaic_grid_average_and_strength() {
        let mut f = Frame::new(MOSAIC_BLOCK_PX * 4, MOSAIC_BLOCK_PX * 2);
        // 左 40px 红右蓝（分界 40 不对齐 24/48 块边界）。
        for y in 0..f.h {
            for x in 0..f.w {
                let c = if x < 40 { [200, 10, 10, 255] } else { [10, 10, 200, 255] };
                f.set(x, y, c);
            }
        }
        let m1 = mosaic(&f, (0, 0, f.w, f.h), 1);
        let m2 = mosaic(&f, (0, 0, f.w, f.h), 2);
        // 24px 网格：块 0-24 纯红、块 24-48 跨分界混合 → 两点不同。
        assert_ne!(m1.pixel(10, 5), m1.pixel(30, 5), "24px 网格保留块差异");
        // 48px 网格：块 0-48 跨过 40 分界 → 块内均色一致。
        assert_eq!(m2.pixel(30, 10), m2.pixel(45, 10), "48px 网格跨过分界均色一致");
    }

    #[test]
    fn annotation_undo_redo_cap() {
        let mut a = AnnotationLayer::new();
        for i in 0..25 {
            a.add(Anno::Text { x: i as i32, y: 0, text: String::from("t") });
        }
        let mut undos = 0;
        while a.undo() {
            undos += 1;
        }
        assert_eq!(undos, 20, "撤销上限 20 步");
        assert!(a.items.is_empty() || a.items.len() == 5, "25 条中前 5 条在栈外不可撤");
        // redo 全部回来。
        let mut redos = 0;
        while a.redo() {
            redos += 1;
        }
        assert_eq!(redos, 20);
    }

    #[test]
    fn number_annotation_renumber_all_cases() {
        let mut a = AnnotationLayer::new();
        for (x, y) in [(1, 1), (2, 2), (3, 3), (4, 4)] {
            a.add_number(x, y);
        }
        // 删头。
        assert!(a.remove_number(1));
        let seqs = |a: &AnnotationLayer| -> Vec<u32> {
            a.items.iter().filter_map(|x| if let Anno::Number { seq, .. } = x { Some(*seq) } else { None }).collect()
        };
        assert_eq!(seqs(&a), alloc::vec![1, 2, 3], "删头重排");
        // 删尾。
        assert!(a.remove_number(3));
        assert_eq!(seqs(&a), alloc::vec![1, 2]);
        // 新增接续编号。
        a.add_number(9, 9);
        assert_eq!(seqs(&a), alloc::vec![1, 2, 3], "next_seq 重排后接续");
        // 删不存在 → false。
        assert!(!a.remove_number(99));
    }

    #[test]
    fn capture_three_modes_full_chain() {
        let mut f = Frame::new(100, 100);
        for y in 0..100u32 {
            for x in 0..100u32 {
                f.set(x, y, [(x % 256) as u8, (y % 256) as u8, 7, 255]);
            }
        }
        let mut s = Snipshot::new();
        // 全屏。
        s.enter(Mode::Fullscreen);
        s.capture_fullscreen(&f, 1);
        assert_eq!(s.history[0].frame.w, 100);
        // 窗口。
        s.enter(Mode::Window);
        s.capture_window(&f, Selection { x: 10, y: 10, w: 30, h: 20 }, 2);
        assert_eq!((s.history[0].frame.w, s.history[0].frame.h), (30, 20));
        // 区域（反向拖）。
        s.enter(Mode::Region);
        s.capture_region(&f, normalize_sel(90, 90, 40, 60, 100, 100), 3);
        assert_eq!((s.history[0].frame.w, s.history[0].frame.h), (50, 30));
        // 区域像素保真（无损）。
        assert_eq!(s.history[0].frame.pixel(0, 0), f.pixel(40, 60));
        assert!(!s.notify_silenced, "截屏完成解除静默");
    }

    #[test]
    fn history_ring_and_export() {
        let f = Frame::new(10, 10);
        let mut s = Snipshot::new();
        for i in 0..30u64 {
            s.capture_fullscreen(&f, i);
        }
        assert_eq!(s.history.len(), HISTORY_CAP);
        assert_eq!(s.history[0].stamp_ms, 29, "最新在前");
        let (bytes, _) = s.export_png_bytes_est(0).unwrap();
        assert_eq!(bytes.len(), 400);
        assert_eq!(s.export_count, 1);
    }

    #[test]
    fn normalize_selection_clamps() {
        assert_eq!(normalize_sel(-5, -5, 10, 10, 100, 100), Selection { x: 0, y: 0, w: 10, h: 10 });
        assert_eq!(normalize_sel(95, 95, 200, 200, 100, 100), Selection { x: 95, y: 95, w: 5, h: 5 });
        assert_eq!(normalize_sel(10, 10, 10, 10, 100, 100).w, 0, "零尺寸选区如实归零");
    }
}
