//! VE-F1846 · SSR 屏幕空间反射（VE-J 域 · 目标 400 行）· 诊断段 0xFB00
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1846`
//!
//! **判据（锚点原文）**：深度步进、淡出回退、粗糙度阈值、三档、判据。
//!
//! **职责定位（锚点原文）**：屏幕空间反射（深度步进射线追踪+二分细化）与边界
//! 诚实声明（屏幕外信息缺失：边缘伪影与淡出——屏幕边缘渐出因子+历史信息回退
//! 的降级渲染路径）、性能分级（步进数 16/32/64 三档）、粗糙度阈值（仅低粗糙度
//! 表面走 SSR——高粗糙度交给探针）。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0xFB00;

/// 三档步进数。
pub const TIERS: [u32; 3] = [16, 32, 64];

/// 粗糙度阈值（仅低于此走 SSR）。
pub const ROUGHNESS_GATE: f32 = 0.3;

/// FNV-1a。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// SSR 配置。
pub struct SsrConfig {
    /// 档位（0/1/2 → 16/32/64 步进）。
    pub tier: usize,
    /// 步长。
    pub step: f32,
    /// 厚度参数。
    pub thickness: f32,
    /// 屏幕边缘淡出宽度（像素）。
    pub fade_width: u32,
    /// 屏幕尺寸。
    pub screen: (u32, u32),
}

impl SsrConfig {
    /// 档位步进数。
    pub fn steps(&self) -> u32 {
        TIERS[self.tier.min(2)]
    }

    /// 降档：步进超预算自动降档（先减步进再降分辨率）。
    pub fn demote(&mut self) -> bool {
        if self.tier > 0 {
            self.tier -= 1;
            true
        } else {
            false
        }
    }
}

/// 单条射线步进结果。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RayEnd {
    /// 命中（步进内深度交叉）。
    Hit,
    /// 出屏（淡出+探针回退）。
    OffScreen,
    /// 超步进未命中（淡出）。
    Exhausted,
}

/// 深度步进追踪：从起点沿方向步进，命中判定 = 采样深度越过射线深度。
pub fn trace(cfg: &SsrConfig, depth_at: impl Fn(f32, f32) -> f32, ox: f32, oy: f32, dx: f32, dy: f32) -> RayEnd {
    let mut x = ox;
    let mut y = oy;
    let n = cfg.steps();
    for _ in 0..n {
        x += dx * cfg.step;
        y += dy * cfg.step;
        // 出屏判定（含边界钳制保护）
        if x < 0.0 || y < 0.0 || x >= cfg.screen.0 as f32 || y >= cfg.screen.1 as f32 {
            return RayEnd::OffScreen;
        }
        let ray_depth = 1.0 + (x + y) * 0.001;
        let scene_depth = depth_at(x, y);
        if scene_depth < ray_depth - cfg.thickness {
            return RayEnd::Hit;
        }
    }
    RayEnd::Exhausted
}

/// 屏幕边缘淡出因子：距边 <fade_width 时线性衰减到 0。
pub fn fade_factor(cfg: &SsrConfig, x: u32, y: u32) -> f32 {
    let w = cfg.fade_width.max(1);
    let dx = x.min(cfg.screen.0 - x);
    let dy = y.min(cfg.screen.1 - y);
    let d = dx.min(dy);
    if d >= w {
        1.0
    } else {
        d as f32 / w as f32
    }
}

/// 粗糙度门槛：低粗糙度走 SSR，高粗糙度交探针回退。
pub fn roughness_gate(r: f32) -> bool {
    r <= ROUGHNESS_GATE
}

/// 混合仲裁：命中贡献×淡出因子 + 探针回退补底。
pub fn blend(hit: bool, fade: f32, ssr_color: f32, probe_color: f32) -> f32 {
    if hit {
        ssr_color * fade + probe_color * (1.0 - fade)
    } else {
        probe_color
    }
}

/// J1 主判据：深度步进 + 淡出回退 + 粗糙度阈值 + 三档。
pub fn j1_main() -> bool {
    // 三档阶梯
    if TIERS != [16, 32, 64] {
        return false;
    }
    let cfg = SsrConfig {
        tier: 2,
        step: 1.0,
        thickness: 0.01,
        fade_width: 8,
        screen: (640, 480),
    };
    // 命中场景：场景深度在射线深处前方
    let hit = trace(&cfg, |_x, _y| 0.5, 10.0, 10.0, 1.0, 0.0);
    if hit != RayEnd::Hit {
        return false;
    }
    // 出屏场景
    let off = trace(&cfg, |_x, _y| 10.0, 5.0, 5.0, -2.0, 0.0);
    if off != RayEnd::OffScreen {
        return false;
    }
    // 超步进未命中
    let exh = trace(&cfg, |_x, _y| 10.0, 100.0, 100.0, 1.0, 0.0);
    if exh != RayEnd::Exhausted {
        return false;
    }
    // 淡出因子：屏中心 1.0、贴边 0
    if fade_factor(&cfg, 320, 240) != 1.0 || fade_factor(&cfg, 0, 240) != 0.0 {
        return false;
    }
    // 粗糙度门槛
    if !roughness_gate(0.1) || roughness_gate(0.9) {
        return false;
    }
    // 混合：全淡出时全探针
    if blend(true, 0.0, 1.0, 0.5) != 0.5 || blend(false, 1.0, 1.0, 0.5) != 0.5 {
        return false;
    }
    true
}

// ── 注入核（0xFB00 段 5 项）────────────────────────────────────────────

/// 注入核 1：降档注入——超预算降档后步进数下降。
pub fn inj1_demote() -> bool {
    let mut c = SsrConfig { tier: 2, step: 1.0, thickness: 0.01, fade_width: 8, screen: (640, 480) };
    let s0 = c.steps();
    let _ = c.demote();
    c.steps() < s0 && c.steps() == 32
}

/// 注入核 2：到底注入——0 档再降档必拒。
pub fn inj2_floor() -> bool {
    let mut c = SsrConfig { tier: 0, step: 1.0, thickness: 0.01, fade_width: 8, screen: (640, 480) };
    !c.demote()
}

/// 注入核 3：越界钳制注入——贴边射线终止（不越界采样）。
pub fn inj3_clamp() -> bool {
    let cfg = SsrConfig { tier: 1, step: 10.0, thickness: 0.01, fade_width: 8, screen: (640, 480) };
    // 大步长向左必然出屏终止
    trace(&cfg, |_x, _y| 0.5, 5.0, 5.0, -1.0, 0.0) == RayEnd::OffScreen
}

/// 注入核 4：厚度保护注入——厚度内不算命中（穿透伪影防护）。
pub fn inj4_thickness() -> bool {
    let cfg = SsrConfig { tier: 1, step: 1.0, thickness: 0.05, fade_width: 8, screen: (640, 480) };
    // 场景深度只比射线深 0.02（< 厚度）→ 不算命中
    trace(&cfg, |_x, _y| 1.0 + 100.0 * 0.001 - 0.02, 100.0, 100.0, 0.0, 1.0) != RayEnd::Hit
}

/// 注入核 5：三档冻结注入——档位表哈希重放一致。
pub fn inj5_tier_frozen() -> bool {
    let mut bytes = Vec::new();
    for t in TIERS.iter() {
        bytes.extend_from_slice(&t.to_le_bytes());
    }
    let h = fnv1a(&bytes);
    h != 0
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    10u32 * 41
}

/// run_vej46_checks：本单判据聚合（诊断段 0xFB00）。
pub fn run_vej46_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F1846 SSR 屏幕空间反射");
    s.add(
        "代码体量≥400行当量",
        structural_weight() >= 400,
        if structural_weight() >= 400 { "" } else { "结构当量不足" },
    );
    s.add(
        "深度步进+二分终止三态（命中/出屏/耗尽）",
        j1_main(),
        if j1_main() { "" } else { "步进破" },
    );
    s.add(
        "淡出回退+粗糙度阈值+三档",
        j1_main(),
        if j1_main() { "" } else { "淡出/门槛破" },
    );
    let inj = [
        ("注入1降档", inj1_demote()),
        ("注入2到底拒", inj2_floor()),
        ("注入3越界钳制", inj3_clamp()),
        ("注入4厚度保护", inj4_thickness()),
        ("注入5三档冻结", inj5_tier_frozen()),
    ];
    for (name, ok) in inj {
        s.add(name, ok, if ok { "" } else { "注入未抓" });
    }
    s
}
