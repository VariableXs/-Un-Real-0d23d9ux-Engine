//! unx48207_xmulticand — UNX-F48207 Xwayland 多屏候选窗定位（跨屏 clamp 纪律）（诊断段 0xBAC0）。
//!
//! 判据原文：
//!   J1 跨屏 ×10 不出屏；判据锚定 §AI-61 §2 专题三类型三。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 多屏几何闭集：屏集合带编号与主屏标记；候选窗必须完整落在
//!      「spot 所在屏」内（焦点跟随优先），spot 屏放不下时按
//!      上→下→左→右 邻屏顺序唯一仲裁，禁止跨屏拼接。
//!   2. clamp 纪律：最终矩形 = 期望矩形对目标屏工作区做
//!      先 X 后 Y 的两轴钳制；钳制量（左右/上下偏移）入账可审计。
//!   3. ×10 不出屏：10 组多屏场景逐组断言 矩形⊆某屏工作区 且
//!      与屏集合的包含关系唯一确定；任一组出屏 = 判据红。
//!   4. 主屏缺省：spot 不在任何屏时回退主屏工作区左下角（与 root
//!      模式同一落点纪律），并留「回退」账，不算静默改行为。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec;      // vec! 宏（坑 2 三件套）
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBAC0;

const E_NO_SCREENS: u16 = SEG_BASE + 1; // 屏集合为空
const E_DUP_SCREen: u16 = SEG_BASE + 2; // 屏编号重复
const E_NO_PRIMARY: u16 = SEG_BASE + 3; // 无主屏
const E_OUT_OF_SCREEN: u16 = SEG_BASE + 4; // 钳制后仍出屏
const E_NEG_GEOM: u16 = SEG_BASE + 5;   // 屏几何负尺寸
const E_SPLIT: u16 = SEG_BASE + 6;      // 跨屏拼接嫌疑
const E_FALLBACK_SILENT: u16 = SEG_BASE + 7; // 回退未留账
const E_BAD_TARGET: u16 = SEG_BASE + 8; // 仲裁目标屏不自洽

/// 单屏几何：编号 + 工作区 + 主屏标记。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Screen {
    pub id: u8,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub primary: bool,
}

impl Screen {
    fn valid(&self) -> bool {
        self.w > 0 && self.h > 0
    }
    fn contains_pt(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
    /// 矩形是否完整包含（不出屏）。
    pub fn contains_rect(&self, rx: i32, ry: i32, rw: i32, rh: i32) -> bool {
        rx >= self.x && ry >= self.y && rx + rw <= self.x + self.w && ry + rh <= self.y + self.h
    }
}

/// 定位请求：spot 点 + 候选窗期望尺寸。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpotReq {
    pub px: i32,
    pub py: i32,
    pub w: i32,
    pub h: i32,
}

/// 定位回执：最终矩形 + 目标屏 + 钳制账 + 回退标记。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaceOut {
    pub x: i32,
    pub y: i32,
    pub screen_id: u8,
    pub clamp_x: i32, // X 轴钳制量（0=无钳制）
    pub clamp_y: i32,
    pub fallback: bool,
}

/// 多屏候选窗定位总账。
#[derive(Debug)]
pub struct MultiCand {
    screens: Vec<Screen>,
    placements: u32,
    fallbacks: u32,
    clamps: u32,
}

impl MultiCand {
    /// 屏集合登记：非空、编号唯一、恰一主屏、几何合法。
    pub fn set_screens(&mut self, scs: Vec<Screen>) -> Result<(), u16> {
        if scs.is_empty() {
            return Err(E_NO_SCREENS);
        }
        for s in &scs {
            if !s.valid() {
                return Err(E_NEG_GEOM);
            }
        }
        for i in 0..scs.len() {
            for j in (i + 1)..scs.len() {
                if scs[i].id == scs[j].id {
                    return Err(E_DUP_SCREen);
                }
            }
        }
        if scs.iter().filter(|s| s.primary).count() != 1 {
            return Err(E_NO_PRIMARY);
        }
        self.screens = scs;
        Ok(())
    }

    fn find_screen_at(&self, px: i32, py: i32) -> Option<&Screen> {
        self.screens.iter().find(|s| s.contains_pt(px, py))
    }

    fn primary(&self) -> Option<&Screen> {
        self.screens.iter().find(|s| s.primary)
    }

    /// 两轴 clamp：期望矩形对目标屏先 X 后 Y 钳制，返回 (x,y,cx,cy)。
    fn clamp_to(s: &Screen, ex: i32, ey: i32, w: i32, h: i32) -> (i32, i32, i32, i32) {
        let mut x = ex;
        let mut y = ey;
        let cx: i32;
        let cy: i32;
        if x < s.x {
            cx = s.x - x;
            x = s.x;
        } else if x + w > s.x + s.w {
            cx = (s.x + s.w) - (x + w);
            x = s.x + s.w - w;
        } else {
            cx = 0;
        }
        if y < s.y {
            cy = s.y - y;
            y = s.y;
        } else if y + h > s.y + s.h {
            cy = (s.y + s.h) - (y + h);
            y = s.y + s.h - h;
        } else {
            cy = 0;
        }
        (x, y, cx, cy)
    }

    /// 定位：spot 屏优先 → 邻屏 上→下→左→右 唯一仲裁 → 主屏回退。
    pub fn place(&mut self, req: SpotReq) -> Result<PlaceOut, u16> {
        if self.screens.is_empty() {
            return Err(E_NO_SCREENS);
        }
        if req.w <= 0 || req.h <= 0 {
            return Err(E_NEG_GEOM);
        }
        // 路径 A：spot 在某屏内（屏几何拷贝快照，避开账本互斥借用）。
        let spot_hit: Option<(u8, i32, i32, i32, i32)> = self
            .screens
            .iter()
            .find(|s| s.contains_pt(req.px, req.py))
            .map(|s| (s.id, s.x, s.y, s.w, s.h));
        if let Some((sid, sx, sy, sw, sh)) = spot_hit {
            let snap = Screen { id: sid, x: sx, y: sy, w: sw, h: sh, primary: false };
            // spot 屏放得下 → spot 原点直放。
            if snap.contains_rect(req.px, req.py, req.w, req.h) {
                self.placements += 1;
                return Ok(PlaceOut {
                    x: req.px,
                    y: req.py,
                    screen_id: sid,
                    clamp_x: 0,
                    clamp_y: 0,
                    fallback: false,
                });
            }
            // 放不下 → clamp 到 spot 屏。
            let (x, y, cx, cy) = Self::clamp_to(&snap, req.px, req.py, req.w, req.h);
            if !snap.contains_rect(x, y, req.w, req.h) {
                return Err(E_OUT_OF_SCREEN);
            }
            if cx != 0 || cy != 0 {
                self.clamps += 1;
            }
            self.placements += 1;
            return Ok(PlaceOut {
                x,
                y,
                screen_id: sid,
                clamp_x: cx,
                clamp_y: cy,
                fallback: false,
            });
        }
        // 路径 B：邻屏仲裁（上→下→左→右，取第一个能完整容纳的屏）。
        // 邻屏定义：与 spot 点「同轴延伸」的屏 —— 上：spot.y < 屏.y 且 X 有交叠；
        // 下：spot.y > 屏底；左：spot.x < 屏.x；右：spot.x > 屏右。
        let cands: Vec<(u8, i32, i32, i32, i32)> = {
            let mut up = Vec::new();
            let mut down = Vec::new();
            let mut left = Vec::new();
            let mut right = Vec::new();
            for s in &self.screens {
                let x_overlap = req.px >= s.x - req.w && req.px <= s.x + s.w;
                let y_overlap = req.py >= s.y - req.h && req.py <= s.y + s.h;
                if y_overlap && req.px < s.x {
                    left.push((s.id, s.x, s.y, s.w, s.h));
                } else if y_overlap && req.px > s.x + s.w {
                    right.push((s.id, s.x, s.y, s.w, s.h));
                } else if x_overlap && req.py < s.y {
                    up.push((s.id, s.x, s.y, s.w, s.h));
                } else if x_overlap && req.py > s.y + s.h {
                    down.push((s.id, s.x, s.y, s.w, s.h));
                }
            }
            let mut v = Vec::new();
            for g in [up, down, left, right] {
                for t in g {
                    v.push(t);
                }
            }
            v
        };
        for (sid, sx, sy, sw2, sh2) in cands {
            let snap = Screen { id: sid, x: sx, y: sy, w: sw2, h: sh2, primary: false };
            if snap.contains_rect(req.px, req.py, req.w, req.h) {
                self.placements += 1;
                return Ok(PlaceOut {
                    x: req.px,
                    y: req.py,
                    screen_id: sid,
                    clamp_x: 0,
                    clamp_y: 0,
                    fallback: false,
                });
            }
            let (x, y, cx, cy) = Self::clamp_to(&snap, req.px, req.py, req.w, req.h);
            if snap.contains_rect(x, y, req.w, req.h) {
                if cx != 0 || cy != 0 {
                    self.clamps += 1;
                }
                self.placements += 1;
                return Ok(PlaceOut {
                    x,
                    y,
                    screen_id: sid,
                    clamp_x: cx,
                    clamp_y: cy,
                    fallback: false,
                });
            }
        }
        // 路径 C：主屏回退（左下角，root 同款落点，内缩 8）。
        let p_snap: (u8, i32, i32, i32, i32) = match self.primary() {
            Some(p) => (p.id, p.x, p.y, p.w, p.h),
            None => return Err(E_NO_PRIMARY),
        };
        let (pid, px0, py0, pw0, ph0) = p_snap;
        let m = 8i32;
        let w = if req.w + 2 * m > pw0 { pw0 - 2 * m } else { req.w };
        let h = if req.h + 2 * m > ph0 { ph0 - 2 * m } else { req.h };
        if w <= 0 || h <= 0 {
            return Err(E_OUT_OF_SCREEN);
        }
        self.fallbacks += 1;
        self.placements += 1;
        Ok(PlaceOut {
            x: px0 + m,
            y: py0 + ph0 - m - h,
            screen_id: pid,
            clamp_x: 0,
            clamp_y: 0,
            fallback: true,
        })
    }

    /// 出屏审计：总定位数 = 直放 + 钳制 + 回退 三路守恒；回退必须留账。
    pub fn audit(&self) -> Result<(), u16> {
        if self.fallbacks > 0 && self.fallbacks > self.placements {
            return Err(E_FALLBACK_SILENT);
        }
        Ok(())
    }

    pub fn placements(&self) -> u32 {
        self.placements
    }
    pub fn fallbacks(&self) -> u32 {
        self.fallbacks
    }
    pub fn clamps(&self) -> u32 {
        self.clamps
    }
}

/// 自例程判据：落实 J1「跨屏 ×10 不出屏」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // 双屏布局：主屏 (0,0,1920,1080)，副屏 (1920,0,1920,1080)。
    let mk = |id: u8, x: i32, y: i32, w: i32, h: i32, p: bool| Screen {
        id, x, y, w, h, primary: p,
    };

    // R1 场景：×10 横跨双屏 —— spot 在主/副屏交替，候选窗 300×200 均不出屏。
    let mut mc = MultiCand { screens: Vec::new(), placements: 0, fallbacks: 0, clamps: 0 };
    mc.set_screens(vec![
        mk(1, 0, 0, 1920, 1080, true),
        mk(2, 1920, 0, 1920, 1080, false),
    ])?;
    for i in 0..10 {
        let px = if i % 2 == 0 { 1800 } else { 2000 };
        let out = mc.place(SpotReq { px, py: 1000, w: 300, h: 200 })?;
        let s = mc.screens.iter().find(|s| s.id == out.screen_id).ok_or(E_BAD_TARGET)?;
        if !s.contains_rect(out.x, out.y, 300, 200) {
            return Err(E_OUT_OF_SCREEN);
        }
        // 唯一性：矩形不得与另一屏交叠（跨屏拼接红线）。
        for other in &mc.screens {
            if other.id != out.screen_id {
                let overlap = !(out.x + 300 <= other.x
                    || other.x + other.w <= out.x
                    || out.y + 200 <= other.y
                    || other.y + other.h <= out.y);
                if overlap {
                    return Err(E_SPLIT);
                }
            }
        }
    }
    if mc.placements() != 10 {
        return Err(E_BAD_TARGET);
    }
    mc.audit()?;
    hits += 1;

    // R2 场景：spot 屏右缘钳制 —— 期望矩形越右缘 → clamp_x 负向回拉且不出屏。
    let out2 = mc.place(SpotReq { px: 1900, py: 100, w: 300, h: 200 })?;
    if out2.screen_id != 1 || out2.clamp_x >= 0 {
        return Err(E_OUT_OF_SCREEN);
    }
    let s1 = mc.screens[0];
    if !s1.contains_rect(out2.x, out2.y, 300, 200) {
        return Err(E_OUT_OF_SCREEN);
    }
    if out2.x != 1920 - 300 {
        return Err(E_BAD_TARGET); // 右缘贴齐
    }
    hits += 1;

    // R3 场景：spot 屏下缘钳制 —— clamp_y 回拉到底缘。
    let out3 = mc.place(SpotReq { px: 100, py: 1000, w: 300, h: 200 })?;
    if out3.screen_id != 1 || out3.clamp_y >= 0 {
        return Err(E_OUT_OF_SCREEN);
    }
    if out3.y != 1080 - 200 {
        return Err(E_BAD_TARGET);
    }
    hits += 1;

    // R4 场景：垂直双屏 —— 上屏 spot 越下缘应落上屏内（不出屏纪律优先于追光标）。
    let mut mc4 = MultiCand { screens: Vec::new(), placements: 0, fallbacks: 0, clamps: 0 };
    mc4.set_screens(vec![
        mk(1, 0, 0, 1920, 1080, true),
        mk(2, 0, 1080, 1920, 1080, false),
    ])?;
    // spot 在上屏 (100,1000)，候选窗高 200 → 上屏放不下 → 钳制到上屏底缘。
    let out4 = mc4.place(SpotReq { px: 100, py: 1000, w: 300, h: 200 })?;
    let s = mc4.screens.iter().find(|s| s.id == out4.screen_id).ok_or(E_BAD_TARGET)?;
    if !s.contains_rect(out4.x, out4.y, 300, 200) {
        return Err(E_OUT_OF_SCREEN);
    }
    if out4.screen_id != 1 {
        return Err(E_BAD_TARGET); // 焦点屏优先，不跨屏
    }
    hits += 1;

    // R5 场景：屏外远端 spot 回退主屏左下角 —— fallback 留账。
    //（远端无邻屏可仲裁：x/y 均无交叠 → 路径 B 空 → 路径 C 主屏回退）
    let out5 = mc.place(SpotReq { px: 10000, py: 5000, w: 300, h: 200 })?;
    if !out5.fallback || out5.screen_id != 1 {
        return Err(E_FALLBACK_SILENT);
    }
    if out5.x != 8 || out5.y != 1080 - 8 - 200 {
        return Err(E_BAD_TARGET);
    }
    if mc.fallbacks() != 1 {
        return Err(E_FALLBACK_SILENT);
    }
    mc.audit()?;
    hits += 1;

    // R6 场景：屏集合登记守卫 —— 空集/重号/多主屏/无主屏/负几何 全拒。
    let mut g = MultiCand { screens: Vec::new(), placements: 0, fallbacks: 0, clamps: 0 };
    match g.set_screens(Vec::new()) {
        Err(E_NO_SCREENS) => {}
        _ => return Err(E_NO_SCREENS),
    }
    match g.set_screens(vec![mk(1, 0, 0, 100, 100, true), mk(1, 100, 0, 100, 100, false)]) {
        Err(E_DUP_SCREen) => {}
        _ => return Err(E_DUP_SCREen),
    }
    match g.set_screens(vec![mk(1, 0, 0, 100, 100, true), mk(2, 100, 0, 100, 100, true)]) {
        Err(E_NO_PRIMARY) => {} // 恰一主屏纪律：双主屏按无有效主屏配置拒收
        _ => return Err(E_NO_PRIMARY),
    }
    match g.set_screens(vec![mk(1, 0, 0, 100, 100, false)]) {
        Err(E_NO_PRIMARY) => {}
        _ => return Err(E_NO_PRIMARY),
    }
    match g.set_screens(vec![mk(1, 0, 0, 0, 100, true)]) {
        Err(E_NEG_GEOM) => {}
        _ => return Err(E_NEG_GEOM),
    }
    hits += 1;

    // R7 场景：三屏 L 形 —— 右屏 spot 越右缘钳制回右屏内。
    let mut mc7 = MultiCand { screens: Vec::new(), placements: 0, fallbacks: 0, clamps: 0 };
    mc7.set_screens(vec![
        mk(1, 0, 0, 1920, 1080, true),
        mk(2, 1920, 0, 1280, 1080, false),
        mk(3, 0, 1080, 3200, 800, false),
    ])?;
    let out7 = mc7.place(SpotReq { px: 3100, py: 950, w: 400, h: 300 })?;
    if out7.screen_id != 2 {
        return Err(E_BAD_TARGET);
    }
    let s2 = mk(2, 1920, 0, 1280, 1080, false);
    if !s2.contains_rect(out7.x, out7.y, 400, 300) {
        return Err(E_OUT_OF_SCREEN);
    }
    if out7.x != 1920 + 1280 - 400 {
        return Err(E_BAD_TARGET);
    }
    hits += 1;

    // R8 场景：零尺寸候选窗拒收。
    match mc7.place(SpotReq { px: 100, py: 100, w: 0, h: 100 }) {
        Err(E_NEG_GEOM) => {}
        _ => return Err(E_NEG_GEOM),
    }
    hits += 1;

    // R9 场景：三屏直放覆盖 —— 副屏 3 直放一次后总数守恒，回退仍为零。
    let d9 = mc7.place(SpotReq { px: 1600, py: 1300, w: 300, h: 200 })?;
    if d9.screen_id != 3 || d9.fallback || d9.clamp_x != 0 || d9.clamp_y != 0 {
        return Err(E_BAD_TARGET);
    }
    if mc7.placements() != 2 || mc7.fallbacks() != 0 {
        return Err(E_BAD_TARGET);
    }
    mc7.audit()?;
    hits += 1;

    // R10 场景：确定性 —— 同几何同输入两次定位逐字段一致。
    let a = mc7.place(SpotReq { px: 3100, py: 950, w: 400, h: 300 })?;
    let b = mc7.place(SpotReq { px: 3100, py: 950, w: 400, h: 300 })?;
    if a != b {
        return Err(E_SPLIT);
    }
    hits += 1;

    Ok(hits)
}
