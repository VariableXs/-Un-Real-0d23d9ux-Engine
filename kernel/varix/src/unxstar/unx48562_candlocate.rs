//! UNX-F48562 · I·三窗候选窗定位联测（正：三窗定位偏差 ≤2px 联测）
//!
//! 判据原文：UNX-F48562-J1 —— 三类窗口（vxwm/Wine/Xwayland）候选窗
//! 定位联测：同 anchor/同候选窗尺寸/同屏尺寸下三窗定位结果两两偏差
//! ≤2px；屏幕边界钳制正确（候选窗不出屏：x+y ≥0 且 x+w ≤sw、y+h ≤sh）；
//! 驱动修正表冻结（三窗修正量与基准差有界）；多 anchor 扫描（含四角
//! 与中心）联测 max 偏差 ≤2px；fuzz 随机 anchor 200 组联测全绿；同种子
//! 重放全等。
//!
//! 设计纪律：no_std（仅 core/alloc）；零 unwrap/expect/panic；fuzz 用
//! 确定性 LCG（wrapping_mul/wrapping_add），同种子同序列。

use crate::alloc::vec;
use crate::alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xF5A0;

pub const E_KIND_OOB: u16 = SEG_BASE + 1;
pub const E_DRIFT: u16 = SEG_BASE + 2;
pub const E_OFFSCREEN: u16 = SEG_BASE + 3;
pub const E_TABLE_DRIFT: u16 = SEG_BASE + 4;
pub const E_NONDET: u16 = SEG_BASE + 5;
pub const E_CLAMP_MISS: u16 = SEG_BASE + 6;

/// 定位容差（px）。
pub const TOLERANCE_PX: i32 = 2;

/// 三类窗口。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocKind {
    Vxwm,
    Wine,
    Xwayland,
}

pub const ALL_KINDS: [LocKind; 3] = [LocKind::Vxwm, LocKind::Wine, LocKind::Xwayland];

impl LocKind {
    pub fn index(&self) -> usize {
        match self {
            LocKind::Vxwm => 0,
            LocKind::Wine => 1,
            LocKind::Xwayland => 2,
        }
    }
    pub fn from_index(i: usize) -> Result<LocKind, u16> {
        match i {
            0 => Ok(LocKind::Vxwm),
            1 => Ok(LocKind::Wine),
            2 => Ok(LocKind::Xwayland),
            _ => Err(E_KIND_OOB),
        }
    }
    /// 驱动修正量（冻结表）：dx, dy。
    pub fn fix(&self) -> (i32, i32) {
        match self {
            LocKind::Vxwm => (0, 0),
            LocKind::Wine => (1, -1),
            LocKind::Xwayland => (-1, 1),
        }
    }
}

/// 定位输入。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    pub ax: i32,
    pub ay: i32,
    pub cw: i32,
    pub ch: i32,
    pub sw: i32,
    pub sh: i32,
}

/// 基准定位：anchor 下方左对齐，越屏则翻转到上方/左方，再钳制。
pub fn base_locate(a: &Anchor) -> (i32, i32) {
    let mut x = a.ax;
    let mut y = a.ay + 20; // 光标高 20。
    // 候选窗超右边界 → 左移。
    if x + a.cw > a.sw {
        x = a.sw - a.cw;
    }
    // 候选窗超下边界 → 翻转到光标上方。
    if y + a.ch > a.sh {
        y = a.ay - a.ch;
    }
    // 最终钳制。
    if x < 0 {
        x = 0;
    }
    if y < 0 {
        y = 0;
    }
    (x, y)
}

/// 窗型定位：基准 + 驱动修正。
pub fn kind_locate(k: LocKind, a: &Anchor) -> (i32, i32) {
    let (bx, by) = base_locate(a);
    let (dx, dy) = k.fix();
    let mut x = bx + dx;
    let mut y = by + dy;
    // 窗型侧同样钳制。
    if x < 0 {
        x = 0;
    }
    if y < 0 {
        y = 0;
    }
    if x + a.cw > a.sw {
        x = a.sw - a.cw;
    }
    if y + a.ch > a.sh {
        y = a.sh - a.ch;
    }
    (x, y)
}

/// 两点偏差（曼哈顿和）。
pub fn dev(a: (i32, i32), b: (i32, i32)) -> i32 {
    let dx = if a.0 > b.0 { a.0 - b.0 } else { b.0 - a.0 };
    let dy = if a.1 > b.1 { a.1 - b.1 } else { b.1 - a.1 };
    dx + dy
}

/// 三窗联测：同 anchor 三窗两两偏差最大值。
pub fn tri_test(a: &Anchor) -> Result<i32, u16> {
    let p0 = kind_locate(LocKind::Vxwm, a);
    let p1 = kind_locate(LocKind::Wine, a);
    let p2 = kind_locate(LocKind::Xwayland, a);
    let d01 = dev(p0, p1);
    let d12 = dev(p1, p2);
    let d02 = dev(p0, p2);
    let mut m = d01;
    if d12 > m {
        m = d12;
    }
    if d02 > m {
        m = d02;
    }
    if m > TOLERANCE_PX {
        return Err(E_DRIFT);
    }
    Ok(m)
}

/// 钳制审计：结果在屏内。
pub fn in_screen(p: (i32, i32), a: &Anchor) -> Result<(), u16> {
    if p.0 < 0 || p.1 < 0 {
        return Err(E_OFFSCREEN);
    }
    if p.0 + a.cw > a.sw || p.1 + a.ch > a.sh {
        return Err(E_OFFSCREEN);
    }
    Ok(())
}

/// 确定性 LCG。
pub struct Lcg {
    st: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { st: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.st = self
            .st
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        self.st
    }
    pub fn below(&mut self, b: u64) -> u64 {
        if b == 0 {
            return 0;
        }
        self.next() % b
    }
}

/// R1 修正表冻结：三窗修正量与基准差 ≤1。
pub fn r1_fix_table_frozen() -> Result<(), u16> {
    for k in ALL_KINDS.iter() {
        let (dx, dy) = k.fix();
        let adx = if dx < 0 { -dx } else { dx };
        let ady = if dy < 0 { -dy } else { dy };
        if adx > 1 || ady > 1 {
            return Err(E_TABLE_DRIFT);
        }
    }
    Ok(())
}

/// R2 中心 anchor 联测：正常位偏差 ≤2。
pub fn r2_center_anchor() -> Result<(), u16> {
    let a = Anchor { ax: 400, ay: 300, cw: 200, ch: 80, sw: 1024, sh: 768 };
    let m = tri_test(&a)?;
    let _ = m;
    // 三窗全部在屏内。
    for k in ALL_KINDS.iter() {
        let p = kind_locate(*k, &a);
        in_screen(p, &a)?;
    }
    Ok(())
}

/// R3 右下角钳制：超屏翻转后仍三窗一致且在屏内。
pub fn r3_corner_clamp() -> Result<(), u16> {
    // 右下角 anchor。
    let a = Anchor { ax: 980, ay: 700, cw: 200, ch: 80, sw: 1024, sh: 768 };
    let m = tri_test(&a)?;
    let _ = m;
    for k in ALL_KINDS.iter() {
        let p = kind_locate(*k, &a);
        in_screen(p, &a)?;
    }
    Ok(())
}

/// R4 左上角：负坐标钳制到 0。
pub fn r4_topleft_clamp() -> Result<(), u16> {
    let a = Anchor { ax: 0, ay: 5, cw: 300, ch: 100, sw: 1024, sh: 768 };
    for k in ALL_KINDS.iter() {
        let p = kind_locate(*k, &a);
        if p.0 < 0 || p.1 < 0 {
            return Err(E_CLAMP_MISS);
        }
        in_screen(p, &a)?;
    }
    Ok(())
}

/// R5 四角扫描联测：四角 + 中心 max 偏差 ≤2。
pub fn r5_five_point_scan() -> Result<(), u16> {
    let pts: [(i32, i32); 5] = [(0, 0), (1020, 0), (0, 760), (1020, 760), (512, 384)];
    for (ax, ay) in pts.iter() {
        let a = Anchor { ax: *ax, ay: *ay, cw: 180, ch: 90, sw: 1024, sh: 768 };
        tri_test(&a)?;
    }
    Ok(())
}

/// R6 屏内不出界：小屏极限（候选窗大于屏）钳制为 0。
pub fn r6_tiny_screen() -> Result<(), u16> {
    let a = Anchor { ax: 10, ay: 10, cw: 500, ch: 400, sw: 300, sh: 200 };
    for k in ALL_KINDS.iter() {
        let p = kind_locate(*k, &a);
        // 候选窗比屏大：钳制到 0。
        if p.0 != 0 || p.1 != 0 {
            return Err(E_CLAMP_MISS);
        }
    }
    Ok(())
}

/// R7 多 anchor 扫描：网格 8×6 = 48 点联测全绿。
pub fn r7_grid_scan() -> Result<(), u16> {
    for gy in 0..6u32 {
        for gx in 0..8u32 {
            let a = Anchor {
                ax: (gx * 120) as i32,
                ay: (gy * 120) as i32,
                cw: 160,
                ch: 70,
                sw: 1024,
                sh: 768,
            };
            tri_test(&a)?;
        }
    }
    Ok(())
}

/// R8 fuzz 随机 anchor 200 组联测全绿。
pub fn r8_fuzz_anchors() -> Result<(), u16> {
    let mut g = Lcg::new(0x48562_0001);
    for _ in 0..200 {
        let a = Anchor {
            ax: g.below(1024) as i32,
            ay: g.below(768) as i32,
            cw: 60 + g.below(240) as i32,
            ch: 30 + g.below(120) as i32,
            sw: 1024,
            sh: 768,
        };
        tri_test(&a)?;
    }
    Ok(())
}

/// R9 fuzz 在屏内审计：随机 anchor 三窗全部在屏。
pub fn r9_fuzz_inscreen() -> Result<(), u16> {
    let mut g = Lcg::new(0x48562_0002);
    for _ in 0..100 {
        let a = Anchor {
            ax: g.below(1024) as i32,
            ay: g.below(768) as i32,
            cw: 40 + g.below(200) as i32,
            ch: 30 + g.below(100) as i32,
            sw: 1024,
            sh: 768,
        };
        for k in ALL_KINDS.iter() {
            let p = kind_locate(*k, &a);
            in_screen(p, &a)?;
        }
    }
    Ok(())
}

/// R10 确定性重放：同 anchor 两轮三窗结果全等。
pub fn r10_deterministic() -> Result<(), u16> {
    let a = Anchor { ax: 333, ay: 222, cw: 150, ch: 60, sw: 1024, sh: 768 };
    let r1: Vec<(i32, i32)> = ALL_KINDS.iter().map(|k| kind_locate(*k, &a)).collect();
    let r2: Vec<(i32, i32)> = ALL_KINDS.iter().map(|k| kind_locate(*k, &a)).collect();
    if r1 != r2 {
        return Err(E_NONDET);
    }
    Ok(())
}

/// 判据主入口：R1..R10，hits 计数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    r1_fix_table_frozen()?;
    hits += 1;
    r2_center_anchor()?;
    hits += 1;
    r3_corner_clamp()?;
    hits += 1;
    r4_topleft_clamp()?;
    hits += 1;
    r5_five_point_scan()?;
    hits += 1;
    r6_tiny_screen()?;
    hits += 1;
    r7_grid_scan()?;
    hits += 1;
    r8_fuzz_anchors()?;
    hits += 1;
    r9_fuzz_inscreen()?;
    hits += 1;
    r10_deterministic()?;
    hits += 1;
    Ok(hits)
}
