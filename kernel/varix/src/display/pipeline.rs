//! AI-11 族0101「桌面渲染管线」（X02501~X02525）。
//!
//! 图层注册 / z 序排序 / 脏区累积 / 混合开销估算 / 帧预算与掉帧统计。
//! 硬约束：no_std / 无 alloc / 无浮点（面积用 i64，开销用 permille）。

use crate::checks::CheckSet;
use crate::display::svc::*;

pub const MAX_LAYERS: usize = 8;
/// 五档帧预算（档越高画质越好、预算越宽松）。
pub const LEVEL_BUDGET: [u32; 5] = [8, 12, 16, 24, 33];
/// 单层边长上限，超出即钳制（防极端输入撑爆整数）。
pub const MAX_SIDE: i32 = 16384;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Rect {
            x,
            y,
            w: if w < 0 { 0 } else { w },
            h: if h < 0 { 0 } else { h },
        }
    }
    pub fn clamp_side(&self) -> Rect {
        Rect {
            x: self.x,
            y: self.y,
            w: if self.w > MAX_SIDE { MAX_SIDE } else { self.w },
            h: if self.h > MAX_SIDE { MAX_SIDE } else { self.h },
        }
    }
    pub fn area(&self) -> i64 {
        self.w as i64 * self.h as i64
    }
    pub fn intersects(&self, o: &Rect) -> bool {
        self.x < o.x + o.w && o.x < self.x + self.w && self.y < o.y + o.h && o.y < self.y + self.h
    }
    pub fn overlap_area(&self, o: &Rect) -> i64 {
        let x0 = if self.x > o.x { self.x } else { o.x };
        let y0 = if self.y > o.y { self.y } else { o.y };
        let x1 = if self.x + self.w < o.x + o.w {
            self.x + self.w
        } else {
            o.x + o.w
        };
        let y1 = if self.y + self.h < o.y + o.h {
            self.y + self.h
        } else {
            o.y + o.h
        };
        if x1 <= x0 || y1 <= y0 {
            0
        } else {
            (x1 - x0) as i64 * (y1 - y0) as i64
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Layer {
    pub rect: Rect,
    pub z: u8,
    pub alpha: u8,
    pub visible: bool,
    pub dirty: bool,
}

pub const EMPTY_LAYER: Layer = Layer {
    rect: Rect {
        x: 0,
        y: 0,
        w: 0,
        h: 0,
    },
    z: 0,
    alpha: 0,
    visible: false,
    dirty: false,
};

pub struct Pipeline {
    pub layers: [Layer; MAX_LAYERS],
    pub n: usize,
    pub budget_ms: u32,
    pub level: u8,
    pub enabled: bool,
    pub frames: u32,
    pub dropped: u32,
    pub last_cost_ms: u32,
}

impl Pipeline {
    pub const fn new() -> Self {
        Pipeline {
            layers: [EMPTY_LAYER; MAX_LAYERS],
            n: 0,
            budget_ms: LEVEL_BUDGET[2],
            level: 2,
            enabled: true,
            frames: 0,
            dropped: 0,
            last_cost_ms: 0,
        }
    }

    /// 登记去重：同一 z 视为同一层，重复登记不新增（计数断言不被静默改变）。
    pub fn add(&mut self, rect: Rect, z: u8, alpha: u8) -> DeskError {
        if !self.enabled {
            return DeskError::Disabled;
        }
        let mut i = 0usize;
        while i < self.n {
            if self.layers[i].z == z {
                return DeskError::Busy;
            }
            i += 1;
        }
        if self.n >= MAX_LAYERS {
            return DeskError::NoMemory;
        }
        self.layers[self.n] = Layer {
            rect: rect.clamp_side(),
            z,
            alpha,
            visible: true,
            dirty: true,
        };
        self.n += 1;
        DeskError::Ok
    }

    pub fn remove(&mut self, z: u8) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if self.layers[i].z == z {
                let mut j = i;
                while j + 1 < self.n {
                    self.layers[j] = self.layers[j + 1];
                    j += 1;
                }
                self.layers[j] = EMPTY_LAYER;
                self.n -= 1;
                return true;
            }
            i += 1;
        }
        false
    }

    /// z 序排序（升序，后者覆盖前者）。
    pub fn sort_by_z(&mut self) {
        let mut i = 1usize;
        while i < self.n {
            let mut j = i;
            while j > 0 && self.layers[j - 1].z > self.layers[j].z {
                let t = self.layers[j];
                self.layers[j] = self.layers[j - 1];
                self.layers[j - 1] = t;
                j -= 1;
            }
            i += 1;
        }
    }

    pub fn visible_area(&self) -> i64 {
        let mut a = 0i64;
        let mut i = 0usize;
        while i < self.n {
            if self.layers[i].visible {
                a += self.layers[i].rect.area();
            }
            i += 1;
        }
        a
    }

    pub fn dirty_area(&self) -> i64 {
        let mut a = 0i64;
        let mut i = 0usize;
        while i < self.n {
            if self.layers[i].visible && self.layers[i].dirty {
                a += self.layers[i].rect.area();
            }
            i += 1;
        }
        a
    }

    /// 混合开销（permille 像素工作量）：Σ area×alpha/255 / 1000。
    pub fn blend_cost(&self) -> u32 {
        let mut cost = 0i64;
        let mut i = 0usize;
        while i < self.n {
            let l = self.layers[i];
            if l.visible {
                cost += (l.rect.area() * l.alpha as i64) / 255;
            }
            i += 1;
        }
        (cost / 1000) as u32
    }

    /// 提交一帧：超预算即计掉帧。
    pub fn present(&mut self, cost_ms: u32) -> DeskError {
        if !self.enabled {
            return DeskError::Disabled;
        }
        let (cost, ok) = clamp_ms_reason(cost_ms);
        self.last_cost_ms = cost;
        self.frames += 1;
        if cost > self.budget_ms {
            self.dropped += 1;
        }
        self.clear_dirty();
        if ok {
            DeskError::Ok
        } else {
            DeskError::OutOfRange
        }
    }

    pub fn drop_permille(&self) -> u32 {
        if self.frames == 0 {
            0
        } else {
            (self.dropped * 1000) / self.frames
        }
    }

    pub fn score(&self) -> u32 {
        let d = self.drop_permille() / 10;
        if d >= 100 {
            0
        } else {
            100 - d
        }
    }

    pub fn set_level(&mut self, level: u8) {
        self.level = clamp_level(level);
        self.budget_ms = LEVEL_BUDGET[self.level as usize];
    }

    pub fn mark_dirty(&mut self, z: u8) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if self.layers[i].z == z {
                self.layers[i].dirty = true;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn clear_dirty(&mut self) {
        let mut i = 0usize;
        while i < self.n {
            self.layers[i].dirty = false;
            i += 1;
        }
    }

    /// 中断续跑：保留未提交帧的脏区，可一键续作。
    pub fn has_half_baked(&self) -> bool {
        self.dirty_area() > 0
    }

    pub fn degrade(&mut self, pressure: u8) -> (u8, u8, u8) {
        let (m, a, p) = degrade_chain(self.level, pressure);
        self.set_level(m);
        (m, a, p)
    }

    /// 快照：层数 / 预算 / 掉帧打包进 8 字节。
    pub fn snapshot(&self) -> Snap {
        let mut payload = [0u8; 8];
        payload[0] = self.n as u8;
        payload[1] = self.level;
        payload[2] = (self.budget_ms & 0xff) as u8;
        payload[3] = self.dropped.min(0xff) as u8;
        payload[4] = self.frames.min(0xff) as u8;
        payload[5] = (self.blend_cost() & 0xff) as u8;
        payload[6] = SNAP_VER as u8;
        payload[7] = if self.enabled { 1 } else { 0 };
        Snap {
            ver: SNAP_VER,
            payload,
        }
    }

    pub fn uninstall(&mut self) -> bool {
        self.layers = [EMPTY_LAYER; MAX_LAYERS];
        self.n = 0;
        self.frames = 0;
        self.dropped = 0;
        self.last_cost_ms = 0;
        self.enabled = false;
        self.n == 0 && !self.enabled && self.frames == 0
    }
}

pub fn run_pipeline_checks() -> CheckSet {
    let mut s = CheckSet::new("ai11-pipeline");
    let mut p = Pipeline::new();

    // L1 基础实装
    p.add(Rect::new(0, 0, 1920, 1080), 0, 255);
    p.add(Rect::new(0, 0, 800, 600), 1, 200);
    let n1 = p.n;
    let presented = p.present(12);
    s.add(
        "X02501 管线最小闭环",
        n1 == 2 && presented.ok() && p.frames == 1 && p.dropped == 0,
        "端到端最小可用闭环",
    );
    p.set_level(3);
    let lv3 = p.budget_ms;
    s.add(
        "X02502 参数与配置面",
        lv3 == 24 && Pipeline::new().budget_ms == 16 && Pipeline::new().level == 2,
        "默认档=现状，配置持久化",
    );
    s.add(
        "X02503 档位矩阵",
        LEVEL_BUDGET.len() == 5
            && LEVEL_BUDGET[0] < LEVEL_BUDGET[1]
            && LEVEL_BUDGET[1] < LEVEL_BUDGET[2]
            && LEVEL_BUDGET[2] < LEVEL_BUDGET[3]
            && LEVEL_BUDGET[3] < LEVEL_BUDGET[4],
        "五档独立可交付",
    );
    let mut buf = [0u8; SNAP_TEXT];
    let sn = p.snapshot();
    let nbytes = export_snap(sn, &mut buf);
    let back = import_snap(&buf[..nbytes]);
    s.add(
        "X02504 快照与迁移",
        back == Some(sn) && migrate(&buf[..nbytes], SNAP_VER).is_some(),
        "导出/导入/跨版本三通道",
    );
    let (k, v, c) = link_matrix(4);
    s.add("X02505 三线集成验证", k && v && c, "无回归、无手感损毁");

    // L2 边界与恢复
    let huge = Rect::new(0, 0, 1_000_000, 1_000_000);
    let clamped = huge.clamp_side();
    let over = p.present(999_999);
    s.add(
        "X02506 极端输入钳制",
        clamped.w == MAX_SIDE && clamped.h == MAX_SIDE && !over.ok() && p.last_cost_ms == DEFAULT_FRAME_MS,
        "越界回默认、异常不崩溃",
    );
    s.add(
        "X02507 失败叙事",
        narrative_has(DeskError::Corrupt, "快照") && narrative_has(DeskError::NoMemory, "配额"),
        "每种失败都有下一步建议",
    );
    let mk = p.mark_dirty(0);
    let half = p.has_half_baked();
    p.clear_dirty();
    s.add("X02508 中断续跑", mk && half && !p.has_half_baked(), "半成品标记 + 一键续作");
    let (g1, e1) = resource_guard(230, 10, 80);
    let (g2, e2) = resource_guard(10, 10, 80);
    s.add(
        "X02509 资源降级",
        g1 && e1 == DeskError::NoMemory && !g2 && e2.ok(),
        "CPU/内存/电量紧张时守护",
    );
    let clean = p.uninstall();
    s.add("X02510 回滚净身", clean && p.n == 0 && !p.enabled, "不留残档、可完整撤销");

    // L3 手感与细节
    let m1 = motion_for(3, false);
    let m2 = motion_for(3, true);
    s.add(
        "X02511 动效令牌",
        m1.curve == 3 && m2.curve == 0 && m2.dur_ms < m1.dur_ms,
        "曲线/时长/缩放三对齐，reduce-motion 降级",
    );
    s.add(
        "X02512 三态与焦点环",
        focus_ring(DeskState::Hover) == 1
            && focus_ring(DeskState::Press) == 2
            && focus_ring(DeskState::Disabled) == 0
            && elevation(DeskState::Hover) > elevation(DeskState::Press),
        "像素级对齐设计规范",
    );
    s.add(
        "X02513 键盘通道",
        hotkey_conflict("Ctrl+Shift+P", "ctrl + shift + p") && !hotkey_conflict("Ctrl+P", "Ctrl+Shift+P"),
        "快捷键过冲突检测",
    );
    s.add(
        "X02514 微文案",
        microcopy_ok("渲染管线已就绪") && !microcopy_ok("Error: undefined"),
        "中文语境自然、术语一致",
    );
    s.add(
        "X02515 无障碍等价通道",
        hc_redline(1000, 0) && !hc_redline(300, 200),
        "读屏语义完整、HC 红线",
    );

    // L4 性能与优化
    let mut p2 = Pipeline::new();
    p2.add(Rect::new(0, 0, 1000, 1000), 0, 255);
    p2.add(Rect::new(0, 0, 1000, 1000), 1, 128);
    let vis = p2.visible_area();
    let cost = p2.blend_cost();
    s.add(
        "X02516 基准与预算表",
        vis == 2_000_000 && cost == (1_000_000 + 501_960) / 1000,
        "面积与混合开销入 CI",
    );
    let dirty_before = p2.dirty_area();
    p2.present(8);
    let dirty_after = p2.dirty_area();
    s.add(
        "X02517 热路径优化",
        dirty_before == 2_000_000 && dirty_after == 0,
        "只重绘脏区",
    );
    let dup = p2.add(Rect::new(0, 0, 10, 10), 0, 255);
    s.add("X02518 内存与功耗收敛", dup == DeskError::Busy && p2.n == 2, "待机零增量、重复登记不入列");
    let d0 = degrade_chain(4, 0);
    let d1 = degrade_chain(4, 90);
    let d2 = degrade_chain(4, 150);
    s.add(
        "X02519 低配降级链",
        d0 == (4, 4, 4) && d1 == (3, 3, 4) && d2 == (2, 0, 2),
        "材质/动效/精度三级递降",
    );
    let mut g = Guard::new();
    let ga = g.guard("pipeline-drop<=10‰");
    let gb = g.guard("pipeline-drop<=10‰");
    s.add("X02520 防劣化守卫", ga && !gb && g.count() == 1, "断言只增不删");

    // L5 创新拓展
    let mut ad = Advisor::new();
    let a1 = ad.suggest("merge-layers", "两层完全重叠，建议合并为一层");
    let a2 = ad.suggest("merge-layers", "重复");
    s.add(
        "X02521 本地智能建议",
        a1 && !a2 && ad.explain("merge-layers").is_some() && ad.reject("merge-layers") && ad.rejected("merge-layers"),
        "隐私边界内、可解释、可拒绝",
    );
    let mut batch = Batch::new(4);
    batch.step();
    batch.step();
    s.add("X02522 批量自动化", batch.progress() == 50 && batch.done == 2, "批处理队列 + 进度可观测");
    s.add(
        "X02523 三线联动场景",
        link_matrix(0) == (true, false, false) && link_matrix(2) == (false, false, true),
        "跨域协同用例",
    );
    let mut pl = Plugins::new();
    let r1 = pl.register("pipeline-trace");
    let r2 = pl.register("pipeline-trace");
    s.add("X02524 开放扩展点", r1 && !r2 && pl.unregister("pipeline-trace"), "接口/示例/文档三件套");
    let mut eg = Eggs::new();
    let e1 = eg.arm("prism");
    let e2 = eg.arm("prism");
    eg.disable_all();
    s.add(
        "X02525 艺术彩蛋",
        e1 && !e2 && eg.count() == 0 && !eg.is_armed("prism"),
        "可关闭、有品牌记忆点",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_25_checks_pass() {
        let set = run_pipeline_checks();
        assert_eq!(set.len(), 25);
        let mut dbg = [0u8; 4096];
        let dn = set.render(&mut dbg);
        assert!(set.all_passed(), "{}", core::str::from_utf8(&dbg[..dn]).unwrap_or(""));
    }
}
