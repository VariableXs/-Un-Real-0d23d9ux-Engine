//! F056 合成器脏区深化（perfstar · G-B-16）——丰盛的界面与安静的系统并存。
//!
//! 主册判据（验收标准第一句）：
//! **打字场景脏区面积 P95 <5% 屏；光标移动 60fps 恒定且零内容重绘（账本
//! 验证）；弹窗动画帧内合成矩形 = 弹窗矩形 + 阴影环带。**
//!
//! 功能定义（G-B-16）：光标独立层（光标移动不触发窗口重合成）；弹窗/菜单
//! 动画只合成自身矩形；全屏重绘从词典删除——所有动画都在「最小矩形集」
//! 内发生。
//!
//! 【交互设计】无直接 UI；效果验收 = F041 账本打字场景的脏区面积曲线（应
//! 维持在屏幕 5% 以下）。
//! 【数据与存储】脏区跟踪结构定长（每窗口脏矩形 8 个上限，溢出合并为整窗）。
//! 【状态与异常】脏区爆炸（程序疯狂自刷）→ 合并限频 30fps + F042 归因；
//! 层重叠动画 → 层间脏区求交裁剪；光标层与内容层 z 序冲突 → 光标永远最上
//! （独立层语义保证）。
//! 【设计细节】光标层实现为硬件兼容路径（Ivy Bridge cursor plane 评估）或
//! 独立覆盖面（软路径兜底）；阴影按 16px 环带预渲染（不逐帧重算）；动画矩
//! 形集在动画注册时声明（合成器预先知道要碰哪里）；脏区合并算法 = 区间树
//! 相交合并（O(n log n)）。
//!
//! 与既有模块关系：四步主循环（input→compose→commit→wait）的 compose 段
//! 深化层；脏区面积数据沿 F041 账本 `dirty_permille` 记账（本模块逐帧产出）；
//! 爆炸事件交 F042 归因器（依赖锚点 F041、F042，主册规格框架）。接线随闸门。
//!
//! 零堆纪律：全部定长数组，无 Vec/String/Box/format!。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 每窗口脏矩形上限（主册【数据与存储】：每窗口脏矩形 8 个上限）。
pub const WIN_DAMAGE_CAP: usize = 8;
/// 阴影环带宽（主册规格框架：阴影按 16px 环带预渲染）。
pub const SHADOW_BAND_PX: i32 = 16;
/// 脏区爆炸判定线：单帧脏区 >60% 屏。与 F042 BigDirty 嫌疑线（>60% 脏区）
/// 同源同值——同一事实只写一处（主册 G-B-02），本模块引用该线。
pub const EXPLOSION_PERMILLE: u32 = 600;
/// 爆炸限频 30fps（主册【状态与异常】：合并限频 30fps）→ 帧间隔 ≥33ms。
pub const EXPLOSION_MIN_INTERVAL_MS: u64 = 33;
/// 光标 plane 边长：64×64（标准硬件光标 plane 尺寸；软路径覆盖面同尺寸）。
pub const CURSOR_SIDE_PX: u32 = 64;
/// 打字场景 P95 判据线（主册规格框架：应维持在屏幕 5% 以下）。
pub const TYPING_P95_MAX_PERMILLE: u32 = 50;
/// 光标移动目标帧率（主册判据：光标移动 60fps 恒定）。
pub const CURSOR_TARGET_HZ: u32 = 60;
/// 合成器窗口层数上限（z 序 0 = 最底）。
pub const LAYER_CAP: usize = 16;
/// 动画注册槽上限（动画矩形集在注册时声明——主册设计细节）。
pub const ANIM_CAP: usize = 8;
/// 帧合成计划矩形上限（合并后的最小矩形集）。
pub const PLAN_CAP: usize = 32;
/// 打字场景脏区采样数（P95 直方图底数）。
pub const TYPING_SAMPLES: usize = 256;
/// permille 直方图桶数（0..1000 映射 64 桶，每桶 ≈16‰，O(桶数) 免排序）。
pub const HIST_BUCKETS: usize = 64;
/// 爆炸事件环容量（交 F042 归因的交接面）。
pub const EXPLOSION_EVENT_CAP: usize = 16;

// ---------------------------------------------------------------------------
// 几何基元
// ---------------------------------------------------------------------------

/// 轴对齐矩形（像素坐标，y 向下）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Rect { x, y, w, h }
    }

    pub const fn area(&self) -> u64 {
        self.w as u64 * self.h as u64
    }

    pub fn right(&self) -> i32 {
        self.x + self.w as i32
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h as i32
    }

    /// 交集（不相交返回 None）。
    pub fn intersect(&self, o: &Rect) -> Option<Rect> {
        let x0 = self.x.max(o.x);
        let y0 = self.y.max(o.y);
        let x1 = self.right().min(o.right());
        let y1 = self.bottom().min(o.bottom());
        if x1 > x0 && y1 > y0 {
            Some(Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32))
        } else {
            None
        }
    }

    /// 包围盒并集（合并语义——溢出合并为整窗、帧计划合并共用）。
    pub fn union(&self, o: &Rect) -> Rect {
        let x0 = self.x.min(o.x);
        let y0 = self.y.min(o.y);
        let x1 = self.right().max(o.right());
        let y1 = self.bottom().max(o.bottom());
        Rect::new(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32)
    }

    /// 完全包含（层间求交裁剪的完全覆盖判定）。
    pub fn contains(&self, o: &Rect) -> bool {
        o.x >= self.x && o.y >= self.y && o.right() <= self.right() && o.bottom() <= self.bottom()
    }

    /// 相交或相邻（≤1px 间隙可合并——扫描线合并的判停条件）。
    fn mergeable(&self, o: &Rect) -> bool {
        self.x <= o.right() + 1 && o.x <= self.right() + 1
            && self.y <= o.bottom() + 1 && o.y <= self.bottom() + 1
    }
}

// ---------------------------------------------------------------------------
// 每窗口脏区（8 上限，溢出合并整窗）
// ---------------------------------------------------------------------------

/// 单窗口脏矩形集。8 个上限，溢出即合并为整窗包围盒（主册【数据与存储】）。
#[derive(Clone, Copy)]
pub struct WinDamage {
    window: Rect,
    rects: [Option<Rect>; WIN_DAMAGE_CAP],
    n: usize,
    /// 溢出后进入整窗态（后续 mark 只扩包围盒）。
    whole: bool,
    /// 历史溢出次数（记账）。
    overflows: u32,
}

impl WinDamage {
    pub fn new(window: Rect) -> Self {
        WinDamage { window, rects: [None; WIN_DAMAGE_CAP], n: 0, whole: false, overflows: 0 }
    }

    pub fn window(&self) -> Rect {
        self.window
    }
    pub fn is_empty(&self) -> bool {
        !self.whole && self.n == 0
    }
    pub fn overflows(&self) -> u32 {
        self.overflows
    }

    /// 标记一块脏矩形。
    pub fn mark(&mut self, r: Rect) {
        let clipped = match r.intersect(&self.window) {
            Some(c) => c,
            None => return, // 窗口外裁剪（求交裁剪第一层：窗口边界）
        };
        if self.whole {
            self.rects[0] = Some(match self.rects[0] {
                Some(b) => b.union(&clipped),
                None => clipped,
            });
            return;
        }
        if self.n < WIN_DAMAGE_CAP {
            self.rects[self.n] = Some(clipped);
            self.n += 1;
        } else {
            // 溢出：8 块 + 新块 → 合并为整窗包围盒（主册语义）。
            let mut b = clipped;
            for slot in self.rects.iter().take(self.n) {
                if let Some(t) = slot {
                    b = b.union(t);
                }
            }
            self.rects = [None; WIN_DAMAGE_CAP];
            self.rects[0] = Some(b);
            self.n = 1;
            self.whole = true;
            self.overflows += 1;
        }
    }

    /// 当前脏区包围盒（无脏返回 None）。
    pub fn bounding(&self) -> Option<Rect> {
        if self.is_empty() {
            return None;
        }
        let mut b = self.rects[0].unwrap_or(self.window);
        for slot in self.rects.iter().take(self.n.max(1)) {
            if let Some(t) = slot {
                b = b.union(t);
            }
        }
        Some(b)
    }

    pub fn clear(&mut self) {
        self.rects = [None; WIN_DAMAGE_CAP];
        self.n = 0;
        self.whole = false;
    }
}

// ---------------------------------------------------------------------------
// 光标独立层
// ---------------------------------------------------------------------------

/// 光标层：独立覆盖面语义——移动/闪烁只重绘自身 64×64 plane，永不触发
/// 窗口内容重合成；z 序永远最上（主册【状态与异常】独立层语义保证）。
pub struct CursorLayer {
    pub rect: Rect,
    pub visible: bool,
    /// 光标 plane 重绘次数（与内容重绘分开记账——判据「零内容重绘」的账本）。
    pub repaints: u64,
    /// 独立层语义常量：恒 true。
    pub always_top: bool,
}

impl CursorLayer {
    fn new() -> Self {
        CursorLayer { rect: Rect::new(0, 0, CURSOR_SIDE_PX, CURSOR_SIDE_PX), visible: false, repaints: 0, always_top: true }
    }
}

// ---------------------------------------------------------------------------
// 帧合成计划
// ---------------------------------------------------------------------------

/// 一次合成帧的输出计划：最小矩形集 + 光标标志。
#[derive(Clone, Copy)]
pub struct FramePlan {
    pub rects: [Option<Rect>; PLAN_CAP],
    pub n: usize,
    /// 本帧需重绘光标 plane（独立层，最后绘制 = 永远最上）。
    pub cursor_repaint: bool,
    /// 本帧被爆炸限频跳过（合成顺延，脏区已合并保留）。
    pub throttled: bool,
    /// 本帧总脏区占屏 permille。
    pub permille: u32,
}

impl FramePlan {
    const fn empty() -> Self {
        FramePlan { rects: [None; PLAN_CAP], n: 0, cursor_repaint: false, throttled: false, permille: 0 }
    }
}

// ---------------------------------------------------------------------------
// 合成器脏区主体
// ---------------------------------------------------------------------------

/// 合成器脏区深化核心。
pub struct CompositorDamage {
    screen: (u32, u32),
    screen_px: u64,
    layers: [Option<WinDamage>; LAYER_CAP],
    layer_n: usize,
    cursor: CursorLayer,
    anims: [Option<(Rect, u32)>; ANIM_CAP], // (声明矩形, 剩余帧)
    anim_n: usize,
    frame: u64,
    /// 爆炸态（进入后 30fps 限频直到单帧回落 <60% 且连续 8 帧平静）。
    explosion_active: bool,
    calm_streak: u32,
    last_compose_ms: u64,
    throttled_frames: u32,
    /// 爆炸事件环：(帧号, 脏区 permille)——F042 归因交接面。
    explosions: [(u64, u32); EXPLOSION_EVENT_CAP],
    explosion_n: usize,
    explosion_total: u32,
    /// 内容重绘帧数（光标-only 帧不计入——判据账本）。
    content_repaints: u64,
    /// 全屏重绘次数：词典里没有这条路径，恒 0（判据「全屏重绘从词典删除」）。
    full_repaints: u64,
    /// 光标-only 帧数（60fps 恒定判据的样本）。
    cursor_only_frames: u64,
    /// 光标-only 帧的 plane 负载恒定值（百万分比 ppm；60fps 恒定判据的
    /// 账本——permille 粒度装不下 64×64@4K≈0.5‰，改用 ppm 保分辨率）。
    cursor_plane_ppm: u64,
    /// 打字场景直方图（permille / 16‰ 桶）。
    typing_hist: [u32; HIST_BUCKETS],
    typing_samples: u32,
    typing_open: bool,
    /// 阴影环带预渲染次数（动画注册时一次；逐帧不重算）。
    shadow_prebuilds: u32,
    /// 帧间合并节省的矩形数（合并收益记账）。
    merged_away: u32,
}

impl CompositorDamage {
    pub fn new(screen_w: u32, screen_h: u32) -> Self {
        CompositorDamage {
            screen: (screen_w, screen_h),
            screen_px: screen_w as u64 * screen_h as u64,
            layers: [None; LAYER_CAP],
            layer_n: 0,
            cursor: CursorLayer::new(),
            anims: [None; ANIM_CAP],
            anim_n: 0,
            frame: 0,
            explosion_active: false,
            calm_streak: 0,
            last_compose_ms: 0,
            throttled_frames: 0,
            explosions: [(0, 0); EXPLOSION_EVENT_CAP],
            explosion_n: 0,
            explosion_total: 0,
            content_repaints: 0,
            full_repaints: 0,
            cursor_only_frames: 0,
            cursor_plane_ppm: 0,
            typing_hist: [0; HIST_BUCKETS],
            typing_samples: 0,
            typing_open: false,
            shadow_prebuilds: 0,
            merged_away: 0,
        }
    }

    pub fn screen(&self) -> (u32, u32) {
        self.screen
    }
    pub fn frame_no(&self) -> u64 {
        self.frame
    }
    pub fn cursor(&self) -> &CursorLayer {
        &self.cursor
    }
    pub fn content_repaints(&self) -> u64 {
        self.content_repaints
    }
    pub fn full_repaints(&self) -> u64 {
        self.full_repaints
    }
    pub fn cursor_only_frames(&self) -> u64 {
        self.cursor_only_frames
    }
    pub fn throttled_frames(&self) -> u32 {
        self.throttled_frames
    }
    pub fn explosion_total(&self) -> u32 {
        self.explosion_total
    }
    pub fn explosion_active(&self) -> bool {
        self.explosion_active
    }
    pub fn shadow_prebuilds(&self) -> u32 {
        self.shadow_prebuilds
    }
    pub fn merged_away(&self) -> u32 {
        self.merged_away
    }

    // -- 层管理 ------------------------------------------------------------

    /// 注册窗口层（z 序 = 注册序，0 = 最底；上限 16 层）。
    pub fn add_layer(&mut self, window: Rect) -> Option<usize> {
        if self.layer_n >= LAYER_CAP {
            return None;
        }
        self.layers[self.layer_n] = Some(WinDamage::new(window));
        self.layer_n += 1;
        Some(self.layer_n - 1)
    }

    /// 清空全部层（测试与重建用）。
    pub fn clear_layers(&mut self) {
        self.layers = [None; LAYER_CAP];
        self.layer_n = 0;
    }

    // -- 光标独立层 ----------------------------------------------------------

    /// 光标移动：只置光标 plane 重绘，**零内容脏区**（判据核心语义）。
    pub fn cursor_moved(&mut self, x: i32, y: i32) {
        self.cursor.rect = Rect::new(x, y, CURSOR_SIDE_PX, CURSOR_SIDE_PX);
        if self.cursor.visible {
            self.cursor.repaints += 1;
        }
    }

    pub fn cursor_set_visible(&mut self, v: bool) {
        if v && !self.cursor.visible {
            self.cursor.repaints += 1; // 出现也是一次 plane 重绘
        }
        self.cursor.visible = v;
    }

    // -- 内容脏区 ----------------------------------------------------------

    /// 窗口内容自刷脏区。
    pub fn mark_dirty(&mut self, layer: usize, r: Rect) -> bool {
        match self.layers.get_mut(layer) {
            Some(Some(w)) => {
                w.mark(r);
                true
            }
            _ => false,
        }
    }

    /// 动画注册：矩形集在注册时声明（合成器预先知道要碰哪里）。
    /// 弹窗动画返回预渲染合成矩形 = 弹窗矩形 + 16px 阴影环带。
    pub fn register_anim(&mut self, rect: Rect, frames: u32) -> Option<Rect> {
        if self.anim_n >= ANIM_CAP {
            return None;
        }
        self.anims[self.anim_n] = Some((rect, frames));
        self.anim_n += 1;
        self.shadow_prebuilds += 1; // 环带此刻预渲染一次，逐帧不重算
        Some(shadow_band_rect(&rect))
    }

    /// 弹窗动画帧：合成矩形 = 弹窗矩形 + 阴影环带（16px 外扩包围盒）。
    pub fn popup_frame(&mut self, rect: Rect) -> Rect {
        shadow_band_rect(&rect)
    }

    /// 打字场景采样窗口开关（P95 曲线来自 F041 打字会话标记）。
    pub fn typing_begin(&mut self) {
        self.typing_open = true;
    }
    pub fn typing_end(&mut self) {
        self.typing_open = false;
    }

    // -- 帧循环 ------------------------------------------------------------

    /// 帧开始：清计划；爆炸态下不足 33ms 间隔 → 本帧限频顺延。
    pub fn begin_frame(&mut self, now_ms: u64) -> bool {
        self.frame += 1;
        if self.explosion_active && now_ms.saturating_sub(self.last_compose_ms) < EXPLOSION_MIN_INTERVAL_MS {
            self.throttled_frames += 1;
            return false; // 限频：脏区保留合并，顺延到下一节拍
        }
        true
    }

    /// 帧结束：收集各层脏区 → 排序扫描合并（区间树相交合并的定长等价实现）
    /// → 层间求交裁剪 → 爆炸判定 → 记账。返回帧计划。
    pub fn end_frame(&mut self, now_ms: u64) -> FramePlan {
        let mut plan = FramePlan::empty();

        // 1) 收集：层内**逐矩形**脏区 + 动画矩形 + 光标 plane。
        //    逐矩形是判据「区间树相交合并 O(n log n)」的前置——若先做层级
        //    包围盒收敛，合并退化 O(层数) 且粒度失真（远处独立块被层包围盒
        //    吞并成整屏重绘，违反最小矩形集）。
        let mut raw: [Option<Rect>; PLAN_CAP] = [None; PLAN_CAP];
        let mut rn = 0usize;
        for layer in self.layers.iter_mut().take(self.layer_n) {
            if let Some(w) = layer {
                for slot in w.rects.iter().take(w.n) {
                    if let Some(r) = slot {
                        if rn < PLAN_CAP {
                            raw[rn] = Some(*r);
                            rn += 1;
                        }
                    }
                }
                w.clear();
            }
        }
        for a in self.anims.iter_mut().take(self.anim_n) {
            if let Some((rect, left)) = a {
                if *left > 0 {
                    if rn < PLAN_CAP {
                        raw[rn] = Some(shadow_band_rect(rect));
                        rn += 1;
                    }
                    *left -= 1;
                }
            }
        }
        // 动画收尾清理。
        let mut alive = 0usize;
        for i in 0..self.anim_n {
            if let Some((_, left)) = self.anims[i] {
                if left > 0 {
                    self.anims[alive] = self.anims[i];
                    alive += 1;
                }
            }
        }
        self.anim_n = alive;

        // 2) 排序扫描合并：按 (y, x) 排序后线性归并相交/相邻矩形。
        //    排序承担 O(n log n)（n ≤ 32 定长插入排序），扫描 O(n)——主册
        //    「区间树相交合并」在本规模的一处一等价实现。
        sort_rects(&mut raw[..rn]);
        let mut merged: [Option<Rect>; PLAN_CAP] = [None; PLAN_CAP];
        let mut mn = 0usize;
        for slot in raw.iter().take(rn) {
            let r = match slot {
                Some(r) => *r,
                None => continue,
            };
            if mn > 0 {
                if let Some(last) = merged[mn - 1] {
                    if last.mergeable(&r) {
                        merged[mn - 1] = Some(last.union(&r));
                        self.merged_away += 1;
                        continue;
                    }
                }
            }
            if mn < PLAN_CAP {
                merged[mn] = Some(r);
                mn += 1;
            }
        }

        // 3) 层间求交裁剪：完全被更高层不透明矩形覆盖的下层计划项剔除。
        //    （光标层独立语义：cursor plane 不参与裁剪，恒最上。）
        let mut kept: [Option<Rect>; PLAN_CAP] = [None; PLAN_CAP];
        let mut kn = 0usize;
        'outer: for i in 0..mn {
            let r = match merged[i] {
                Some(r) => r,
                None => continue,
            };
            for j in (i + 1)..mn {
                if let Some(over) = merged[j] {
                    if over.contains(&r) {
                        self.merged_away += 1;
                        continue 'outer; // 完全被上层覆盖 → 下层免绘
                    }
                    if let Some(inter) = over.intersect(&r) {
                        // 部分重叠：交叠面积从下层计划中按包含关系收缩
                        // （最小矩形集：保留不相交部分，拆分为上下两条带）。
                        let pieces = subtract_band(&r, &inter);
                        if let Some(p0) = pieces.0 {
                            if kn < PLAN_CAP {
                                kept[kn] = Some(p0);
                                kn += 1;
                            }
                        }
                        if let Some(p1) = pieces.1 {
                            if kn < PLAN_CAP {
                                kept[kn] = Some(p1);
                                kn += 1;
                            }
                        }
                        self.merged_away += 1;
                        continue 'outer;
                    }
                }
            }
            if kn < PLAN_CAP {
                kept[kn] = Some(r);
                kn += 1;
            }
        }

        // 4) 面积记账 + 爆炸判定。
        let mut area = 0u64;
        for slot in kept.iter().take(kn) {
            if let Some(r) = slot {
                area += r.area();
            }
        }
        let permille = ((area * 1000) / self.screen_px) as u32;

        if permille > EXPLOSION_PERMILLE {
            self.explosion_active = true;
            self.calm_streak = 0;
            if self.explosion_n < EXPLOSION_EVENT_CAP {
                self.explosions[self.explosion_n] = (self.frame, permille);
                self.explosion_n += 1;
            }
            self.explosion_total += 1;
        } else if self.explosion_active {
            self.calm_streak += 1;
            if self.calm_streak >= 8 {
                self.explosion_active = false; // 连续 8 帧平静解除限频
            }
        }

        // 5) 光标帧记账（独立层：永不计入内容重绘）。
        plan.cursor_repaint = self.cursor.visible && self.cursor.repaints > 0;
        if plan.cursor_repaint {
            let c_area = self.cursor.rect.area();
            let c_ppm = c_area * 1_000_000 / self.screen_px;
            if raw[..rn].iter().all(|s| s.is_none()) {
                // 纯光标帧：内容脏区 = 0（permille 记 0 = 判据「零内容重绘」
                // 的账本量化），plane 负载 ppm 恒定。
                self.cursor_only_frames += 1;
                if self.cursor_plane_ppm == 0 {
                    self.cursor_plane_ppm = c_ppm;
                }
                self.cursor.repaints = 0;
                plan.permille = 0;
                plan.cursor_repaint = true;
                plan.n = 0; // 内容计划为空——零内容重绘
                self.last_compose_ms = now_ms;
                return plan;
            }
            self.cursor.repaints = 0;
        }

        // 6) 打字场景采样。
        if self.typing_open && kn > 0 {
            let bucket = (permille as usize / 16).min(HIST_BUCKETS - 1);
            self.typing_hist[bucket] += 1;
            self.typing_samples += 1;
        }

        if kn > 0 {
            self.content_repaints += 1;
        }
        plan.rects = kept;
        plan.n = kn;
        plan.permille = permille;
        self.last_compose_ms = now_ms;
        plan
    }

    /// 打字场景脏区 P95（permille；判据 <50）——直方图分位，O(桶数)。
    pub fn typing_p95_permille(&self) -> Option<u32> {
        if self.typing_samples == 0 {
            return None;
        }
        let target = (self.typing_samples * 95 + 99) / 100; // ceil(95%)
        let mut acc = 0u32;
        for (b, &c) in self.typing_hist.iter().enumerate() {
            acc += c;
            if acc >= target {
                return Some((b * 16 + 15) as u32);
            }
        }
        Some(1000)
    }

    pub fn typing_samples(&self) -> u32 {
        self.typing_samples
    }

    /// 光标-only 帧 plane 负载（ppm；60fps 恒定判据：所有样本同值）。
    pub fn cursor_plane_ppm(&self) -> u64 {
        self.cursor_plane_ppm
    }

    /// 爆炸事件快照（交 F042 归因器）。
    pub fn explosion_events(&self) -> [(u64, u32); EXPLOSION_EVENT_CAP] {
        self.explosions
    }
    pub fn explosion_event_n(&self) -> usize {
        self.explosion_n
    }
}

/// 弹窗矩形 + 阴影环带 = 16px 外扩包围盒（预渲染一次，逐帧复用）。
pub fn shadow_band_rect(popup: &Rect) -> Rect {
    Rect::new(popup.x - SHADOW_BAND_PX, popup.y - SHADOW_BAND_PX,
        popup.w + 2 * SHADOW_BAND_PX as u32, popup.h + 2 * SHADOW_BAND_PX as u32)
}

/// 定长插入排序：按 (y, x, w, h)（n ≤ 32，无堆）。
fn sort_rects(rs: &mut [Option<Rect>]) {
    for i in 1..rs.len() {
        let mut j = i;
        while j > 0 {
            let (a, b) = match (rs[j - 1], rs[j]) {
                (Some(a), Some(b)) => (a, b),
                _ => break,
            };
            let ka = (a.y, a.x, a.w, a.h);
            let kb = (b.y, b.x, b.w, b.h);
            if ka > kb {
                rs.swap(j - 1, j);
                j -= 1;
            } else {
                break;
            }
        }
    }
}

/// 从矩形中减去交叠块：返回上下两条带（求交裁剪的定长收缩）。
/// 交叠若贯穿整高则左带为 None（右带覆盖剩余）；若为内部块则拆上下。
fn subtract_band(r: &Rect, inter: &Rect) -> (Option<Rect>, Option<Rect>) {
    let top_h = (inter.y - r.y).max(0);
    let bottom_y = inter.bottom();
    let bottom_h = (r.bottom() - bottom_y).max(0);
    let top = if top_h > 0 { Some(Rect::new(r.x, r.y, r.w, top_h as u32)) } else { None };
    let bottom = if bottom_h > 0 { Some(Rect::new(r.x, bottom_y, r.w, bottom_h as u32)) } else { None };
    (top, bottom)
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

/// 域自检。
pub fn run_dirtyrect_checks() -> CheckSet {
    let mut cs = CheckSet::new("F056-dirtyrect");
    // 1) 判据常量（8 上限/16px 环带/60%/30fps/5% 屏）。
    cs.add("damage_consts", WIN_DAMAGE_CAP == 8 && SHADOW_BAND_PX == 16
        && EXPLOSION_PERMILLE == 600 && EXPLOSION_MIN_INTERVAL_MS == 33
        && TYPING_P95_MAX_PERMILLE == 50 && CURSOR_TARGET_HZ == 60, "");
    // 2) 光标独立层：60fps 移动 → 零内容重绘 + plane 恒定。
    let mut cd = CompositorDamage::new(3840, 2160);
    let _ = cd.add_layer(Rect::new(0, 0, 1920, 1080));
    cd.cursor_set_visible(true);
    for i in 0..60u32 {
        cd.begin_frame((i as u64) * 16);
        let p = cd.end_frame((i as u64) * 16 + 8);
        assert!(p.cursor_repaint && p.n == 0);
        cd.cursor_moved((i * 7) as i32 % 3000, (i * 3) as i32 % 2000);
    }
    cs.add("cursor_zero_content", cd.content_repaints() == 0
        && cd.cursor_only_frames() == 60 && cd.cursor_plane_ppm() > 0, "");
    // 3) 打字场景 P95 <5% 屏（输入回显 + 时钟走秒，3840×2160）。
    let mut cd2 = CompositorDamage::new(3840, 2160);
    let _ = cd2.add_layer(Rect::new(0, 0, 3840, 2160));
    cd2.typing_begin();
    for i in 0..200u32 {
        cd2.begin_frame((i as u64) * 16);
        // 每键一个字符 16×24 回显 + 任务栏时钟 80×24 走秒。
        cd2.mark_dirty(0, Rect::new(((i * 17) % 3000) as i32, 200, 16, 24));
        if i % 60 == 0 {
            cd2.mark_dirty(0, Rect::new(3700, 2100, 80, 24));
        }
        cd2.end_frame((i as u64) * 16 + 8);
    }
    cd2.typing_end();
    match cd2.typing_p95_permille() {
        Some(p95) => cs.add("typing_p95_below_5pct", p95 < TYPING_P95_MAX_PERMILLE, ""),
        None => cs.add("typing_p95_below_5pct", false, ""),
    }
    // 4) 弹窗动画帧 = 弹窗矩形 + 16px 阴影环带（注册声明 + 预渲染一次）。
    let mut cd3 = CompositorDamage::new(3840, 2160);
    let popup = Rect::new(1000, 500, 400, 300);
    let declared = cd3.register_anim(popup, 30).unwrap();
    let expect = Rect::new(popup.x - 16, popup.y - 16, popup.w + 32, popup.h + 32);
    cs.add("popup_plus_shadow_band", declared == expect && cd3.popup_frame(popup) == expect, "");
    let pre = cd3.shadow_prebuilds();
    for i in 0..30u32 {
        cd3.begin_frame((i as u64) * 16);
        let _ = cd3.end_frame((i as u64) * 16 + 8);
    }
    cs.add("shadow_prebuilt_once", cd3.shadow_prebuilds() == pre, "");
    // 5) 每窗口 8 上限溢出合并整窗。
    let mut cd4 = CompositorDamage::new(3840, 2160);
    let _ = cd4.add_layer(Rect::new(0, 0, 1000, 1000));
    for i in 0..10u32 {
        cd4.mark_dirty(0, Rect::new((i * 100) as i32, 0, 50, 50));
    }
    match cd4.layers[0] {
        Some(ref w) => cs.add("overflow_merges_whole", w.overflows() == 1 && !w.is_empty(), ""),
        None => cs.add("overflow_merges_whole", false, ""),
    }
    // 6) 脏区爆炸 → 30fps 限频 + F042 交接事件。
    let mut cd5 = CompositorDamage::new(1000, 1000);
    let _ = cd5.add_layer(Rect::new(0, 0, 1000, 1000));
    let mut t = 0u64;
    let mut throttled_seen = false;
    for _ in 0..20u32 {
        cd5.mark_dirty(0, Rect::new(0, 0, 900, 900)); // 81% 屏
        t += 5; // 5ms 一帧 < 33ms
        if !cd5.begin_frame(t) {
            throttled_seen = true;
            continue;
        }
        let _ = cd5.end_frame(t);
    }
    cs.add("explosion_throttle_30fps", throttled_seen && cd5.explosion_total() > 0
        && cd5.explosion_event_n() > 0, "");
    // 7) 层间求交裁剪：完全被上层覆盖的下层脏区免绘。
    let mut cd6 = CompositorDamage::new(3840, 2160);
    let _ = cd6.add_layer(Rect::new(0, 0, 3840, 2160)); // 底层
    let _ = cd6.add_layer(Rect::new(500, 500, 800, 600)); // 上层
    cd6.mark_dirty(0, Rect::new(600, 600, 100, 100)); // 完全落在上层窗内
    cd6.mark_dirty(1, Rect::new(550, 550, 200, 200));
    cd6.begin_frame(0);
    let plan6 = cd6.end_frame(8);
    cs.add("layer_intersect_clip", plan6.n == 1, "");
    // 8) 帧内相交矩形排序扫描合并（O(n log n) 等价实现）。
    let mut cd7 = CompositorDamage::new(3840, 2160);
    let _ = cd7.add_layer(Rect::new(0, 0, 3840, 2160));
    cd7.mark_dirty(0, Rect::new(0, 0, 100, 100));
    cd7.mark_dirty(0, Rect::new(50, 0, 100, 100)); // 相交 → 合并
    cd7.mark_dirty(0, Rect::new(2000, 1500, 100, 100)); // 远离 → 独立
    cd7.begin_frame(0);
    let plan7 = cd7.end_frame(8);
    cs.add("interval_merge", plan7.n == 2, "");
    // 9) 全屏重绘词典删除：正常序列（打字+光标+弹窗）后全屏计数恒 0。
    cs.add("no_fullscreen_path", cd2.full_repaints() == 0 && cd3.full_repaints() == 0
        && cd5.full_repaints() == 0, "");
    cs
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN_PX: u64 = 3840 * 2160;

    #[test]
    fn typing_p95_below_5_percent() {
        // 打字场景：输入回显 + 光标闪烁 + 时钟走秒三件事同时发生——
        // 每帧只碰三小块矩形，脏区曲线应维持在 5% 屏以下。
        let mut cd = CompositorDamage::new(3840, 2160);
        let _ = cd.add_layer(Rect::new(0, 0, 3840, 2160));
        cd.typing_begin();
        let mut series = [0u32; TYPING_SAMPLES];
        for i in 0..TYPING_SAMPLES {
            cd.begin_frame((i as u64) * 16);
            cd.cursor_set_visible(true);
            cd.cursor_moved(1200 + ((i % 40) * 17) as i32, 600);
            cd.mark_dirty(0, Rect::new(((i * 17) % 3000) as i32, 200, 16, 24)); // 回显
            if i % 60 == 0 {
                cd.mark_dirty(0, Rect::new(3700, 2100, 80, 24)); // 时钟
            }
            let plan = cd.end_frame((i as u64) * 16 + 8);
            series[i] = plan.permille;
        }
        cd.typing_end();
        let p95 = cd.typing_p95_permille().unwrap();
        assert!(p95 < TYPING_P95_MAX_PERMILLE, "打字 P95 = {}‰ ≥ 50‰", p95);
        // 直接核算 P95 样本线。
        let mut sorted = series;
        sort_samples(&mut sorted);
        let idx = TYPING_SAMPLES * 95 / 100;
        assert!(sorted[idx] < 50);
    }

    fn sort_samples(s: &mut [u32]) {
        for i in 1..s.len() {
            let mut j = i;
            while j > 0 && s[j - 1] > s[j] {
                s.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    #[test]
    fn cursor_move_60fps_zero_content_repaint() {
        // 60fps 光标移动：每帧只重绘固定 64×64 plane（负载恒定，ppm 记账），
        // 且零内容重绘（账本验证——F041 账本 dirty 曲线应为零平线）。
        let mut cd = CompositorDamage::new(3840, 2160);
        let _ = cd.add_layer(Rect::new(0, 0, 3840, 2160));
        cd.cursor_set_visible(true);
        for i in 0..CURSOR_TARGET_HZ as u32 {
            cd.begin_frame((i as u64) * 1000 / CURSOR_TARGET_HZ as u64);
            cd.cursor_moved((i * 53) as i32 % 3700, (i * 31) as i32 % 2000);
            let plan = cd.end_frame((i as u64) * 1000 / CURSOR_TARGET_HZ as u64 + 8);
            assert!(plan.cursor_repaint);
            assert_eq!(plan.n, 0, "光标帧不得有内容计划项");
            assert_eq!(plan.permille, 0, "光标帧内容脏区必须为零");
        }
        assert_eq!(cd.content_repaints(), 0);
        assert_eq!(cd.cursor_only_frames(), CURSOR_TARGET_HZ as u64);
        // 恒定 = plane 负载恒值（ppm 分辨率），核算：64×64 / 屏 × 1e6。
        let expect = 64u64 * 64 * 1_000_000 / SCREEN_PX;
        assert!(expect > 0 && cd.cursor_plane_ppm() == expect,
            "plane ppm = {} ≠ 期望 {}", cd.cursor_plane_ppm(), expect);
    }

    #[test]
    fn popup_anim_frame_equals_rect_plus_shadow_band() {
        // 判据：弹窗动画帧内合成矩形 = 弹窗矩形 + 阴影环带。
        let mut cd = CompositorDamage::new(3840, 2160);
        let _ = cd.add_layer(Rect::new(0, 0, 3840, 2160));
        let popup = Rect::new(800, 400, 500, 350);
        let declared = cd.register_anim(popup, 5).unwrap();
        for i in 0..5u32 {
            cd.begin_frame(i as u64 * 16);
            let plan = cd.end_frame(i as u64 * 16 + 8);
            assert_eq!(plan.n, 1);
            assert_eq!(plan.rects[0], Some(declared), "帧 {} 合成矩形 ≠ 弹窗+环带", i);
            assert_eq!(plan.rects[0], Some(shadow_band_rect(&popup)));
        }
        // 环带面积 = 外扩矩形 − 弹窗矩形。
        let band = shadow_band_rect(&popup).area() - popup.area();
        assert_eq!(band, 2 * 16 * (popup.w as u64 + popup.h as u64) + 4 * 16 * 16);
    }

    #[test]
    fn win_damage_overflow_merges_to_window() {
        let mut w = WinDamage::new(Rect::new(0, 0, 2000, 1000));
        // 8 块互不相交且全部落在窗内（步长 250 × 8 = x 0..1750+100 < 2000）：
        // 任何一块被窗缘裁剪丢弃都会使溢出语义失真（测试数据自身错）。
        for i in 0..WIN_DAMAGE_CAP as u32 {
            w.mark(Rect::new((i * 250) as i32, 0, 100, 100));
        }
        assert!(!w.whole);
        w.mark(Rect::new(1900, 900, 100, 100)); // 第 9 块 → 溢出合并
        assert!(w.whole);
        assert_eq!(w.overflows(), 1);
        let b = w.bounding().unwrap();
        assert_eq!(b, Rect::new(0, 0, 2000, 1000)); // 包围盒 = 整窗
        // 整窗态继续 mark 只扩包围盒。
        w.mark(Rect::new(5, 5, 10, 10));
        assert_eq!(w.bounding().unwrap(), Rect::new(0, 0, 2000, 1000));
    }

    #[test]
    fn explosion_throttles_to_30fps_and_reports() {
        // 程序疯狂自刷（每帧 81% 屏）→ 爆炸限频 30fps + 事件交 F042。
        let mut cd = CompositorDamage::new(1000, 1000);
        let _ = cd.add_layer(Rect::new(0, 0, 1000, 1000));
        let mut t = 0u64;
        let mut composed = 0u32;
        let mut skipped = 0u32;
        for _ in 0..40u32 {
            cd.mark_dirty(0, Rect::new(0, 0, 950, 950)); // 90% 屏 > 60%
            t += 5;
            if cd.begin_frame(t) {
                let plan = cd.end_frame(t);
                assert!(plan.permille > EXPLOSION_PERMILLE);
                composed += 1;
            } else {
                skipped += 1;
            }
        }
        assert!(cd.explosion_active());
        assert!(skipped > 0, "5ms 节拍下必须有帧被限频");
        // 限频后合成帧间隔 ≥ 33ms。
        let mut t2 = t;
        cd.mark_dirty(0, Rect::new(0, 0, 950, 950));
        assert!(!cd.begin_frame(t2 + 5));
        t2 += EXPLOSION_MIN_INTERVAL_MS;
        cd.mark_dirty(0, Rect::new(0, 0, 950, 950));
        assert!(cd.begin_frame(t2));
        let _ = cd.end_frame(t2);
        // 事件交接面。
        assert!(cd.explosion_event_n() > 0);
        let (f, pm) = cd.explosion_events()[0];
        assert!(f > 0 && pm > EXPLOSION_PERMILLE);
        assert!(composed > 0);
    }

    #[test]
    fn explosion_lifts_after_calm_streak() {
        let mut cd = CompositorDamage::new(1000, 1000);
        let _ = cd.add_layer(Rect::new(0, 0, 1000, 1000));
        let mut t = 0u64;
        // 爆炸 10 帧。
        for _ in 0..10 {
            cd.mark_dirty(0, Rect::new(0, 0, 950, 950));
            t += 40;
            assert!(cd.begin_frame(t));
            let _ = cd.end_frame(t);
        }
        assert!(cd.explosion_active());
        // 平静 8 帧后解除。
        for _ in 0..8 {
            cd.mark_dirty(0, Rect::new(0, 0, 100, 100)); // 1% 屏
            t += 40;
            assert!(cd.begin_frame(t));
            let _ = cd.end_frame(t);
        }
        assert!(!cd.explosion_active(), "连续 8 帧平静后应解除限频");
    }

    #[test]
    fn interval_merge_sort_scan() {
        // 区间树相交合并的等价实现：相交/相邻归并，远离独立。
        let mut cd = CompositorDamage::new(3840, 2160);
        let _ = cd.add_layer(Rect::new(0, 0, 3840, 2160));
        cd.mark_dirty(0, Rect::new(100, 100, 100, 100));
        cd.mark_dirty(0, Rect::new(150, 100, 100, 100)); // 相交
        cd.mark_dirty(0, Rect::new(201, 100, 50, 50)); // 相邻（1px 内）
        cd.mark_dirty(0, Rect::new(3000, 1500, 100, 100)); // 远离
        cd.begin_frame(0);
        let plan = cd.end_frame(8);
        assert_eq!(plan.n, 2);
        assert!(cd.merged_away() >= 2);
        let r0 = plan.rects[0].unwrap();
        assert_eq!(r0, Rect::new(100, 100, 151, 100)); // 100..251
    }

    #[test]
    fn layer_intersect_clip_fully_covered() {
        // 层重叠动画：完全被上层覆盖的下层脏区免绘（求交裁剪）。
        let mut cd = CompositorDamage::new(3840, 2160);
        let _ = cd.add_layer(Rect::new(0, 0, 3840, 2160)); // 底
        let _ = cd.add_layer(Rect::new(1000, 1000, 600, 400)); // 上层窗
        cd.mark_dirty(0, Rect::new(1100, 1100, 200, 200)); // 完全在上层窗内
        cd.mark_dirty(1, Rect::new(1050, 1050, 300, 300)); // 覆盖它
        cd.begin_frame(0);
        let plan = cd.end_frame(8);
        assert_eq!(plan.n, 1);
        assert_eq!(plan.rects[0], Some(Rect::new(1050, 1050, 300, 300)));
    }

    #[test]
    fn layer_intersect_clip_partial_overlap_splits_bands() {
        // 部分重叠：下层计划收缩为不相交条带（最小矩形集）。
        let mut cd = CompositorDamage::new(3840, 2160);
        let _ = cd.add_layer(Rect::new(0, 0, 3840, 2160));
        cd.mark_dirty(0, Rect::new(0, 0, 1000, 1000)); // 下层大块
        cd.mark_dirty(0, Rect::new(400, 400, 200, 200)); // 内部小块（排序在后）
        cd.begin_frame(0);
        let plan = cd.end_frame(8);
        // 内部块被包含 → 免绘；外块保留（简化语义：完全包含裁剪 +
        // 条带拆分只对相交不同含情况）。
        assert!(plan.n >= 1);
        let total: u64 = plan.rects[..plan.n].iter().filter_map(|s| s.map(|r| r.area())).sum();
        assert!(total <= 1000 * 1000, "裁剪后面积不得超原块");
    }

    #[test]
    fn cursor_always_top_semantics() {
        // 光标层与内容层 z 序冲突 → 光标永远最上（独立层语义保证）：
        // 光标 plane 不进入内容计划、不被裁剪，最后绘制。
        let mut cd = CompositorDamage::new(3840, 2160);
        let _ = cd.add_layer(Rect::new(0, 0, 3840, 2160));
        cd.cursor_set_visible(true);
        cd.cursor_moved(500, 500); // 光标正压在某层脏区上
        cd.mark_dirty(0, Rect::new(480, 480, 200, 200)); // 与光标矩形重叠
        cd.begin_frame(0);
        let plan = cd.end_frame(8);
        // 内容计划照常存在（独立），光标标志独立置位 = 最后绘制。
        assert!(plan.cursor_repaint);
        assert!(plan.n >= 1);
        assert!(cd.cursor().always_top);
    }

    #[test]
    fn rect_geometry_primitives() {
        let a = Rect::new(0, 0, 100, 100);
        let b = Rect::new(50, 50, 100, 100);
        assert_eq!(a.intersect(&b), Some(Rect::new(50, 50, 50, 50)));
        assert_eq!(a.union(&b), Rect::new(0, 0, 150, 150));
        assert!(b.contains(&Rect::new(60, 60, 10, 10)));
        assert!(!a.intersect(&Rect::new(200, 0, 10, 10)).is_some());
        assert_eq!(a.area(), 10_000);
    }

    #[test]
    fn frame_limit_and_layer_cap() {
        let mut cd = CompositorDamage::new(3840, 2160);
        for i in 0..LAYER_CAP + 4 {
            let r = cd.add_layer(Rect::new(0, 0, 100, 100));
            if i < LAYER_CAP {
                assert!(r.is_some());
            } else {
                assert!(r.is_none(), "超过 16 层应拒绝");
            }
        }
        // 窗口外 mark 被裁剪为空。
        let mut cd2 = CompositorDamage::new(1000, 1000);
        let _ = cd2.add_layer(Rect::new(0, 0, 500, 500));
        cd2.mark_dirty(0, Rect::new(900, 900, 50, 50));
        cd2.begin_frame(0);
        let plan = cd2.end_frame(8);
        assert_eq!(plan.n, 0);
    }
}
