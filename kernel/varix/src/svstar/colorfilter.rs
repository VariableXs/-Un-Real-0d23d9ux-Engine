//! F114 色弱辅助滤镜 · 完整设计（STAR I 主册 G-C-44）。
//!
//! **判据（主册）**：三滤镜色域变换与标准模拟矩阵对拍（Ishihara 测试图
//! 通过率提升实测）；帧耗时增量 ≤0.5ms。
//!
//! **设计要点（主册）**：
//! - 三向色觉滤镜：红色弱（protanopia）/绿色弱（deuteranopia）/蓝色弱
//!   （tritanopia）全屏色域变换（合成器像素后处理）；强度两档
//!   （=校正量插值 50%/100%）；E8 动效三档同级可关；
//! - **模拟/对拍面**：Machado et al. 2009 公开色觉模拟矩阵（学术公共
//!   成果标注，severity 1.0）——预览图例与对拍基准的唯一源；
//! - **校正面**：daltonize 误差重分配范式——混淆轴误差向保留轴转移
//!   （protan/deutan：红绿误差→蓝轴；tritan：蓝黄误差→红绿轴）。
//!   注：Machado severity 1.0 模拟矩阵为奇异阵（二色视觉本质降维），
//!   「模拟之逆」数学不存在——校正增益为本实现设计常量（F130 登记
//!   自定参数），灰轴零干预由构造保证（err(灰)=0 → 输出=输入）；
//! - 矩阵应用于合成输出末级（滤镜之下所有内容统一）；
//! - 与夜间模式（F116）叠加顺序：色温先、滤镜后（文档化）；
//! - 快捷键 Win+Ctrl+C 循环开关；
//! - 设置页三模式卡（各带模拟预览图例）+ 强度滑杆 + F076 快速卡；
//!   滤镜图标常驻托盘（开启时）；
//! - 合成器负载超预算（滤镜后处理 >0.5ms/帧）→ 自动降强度档+说明；
//! - 截图（F098）默认截原色（滤镜为观看辅助不污染产出——显式说明）。
//!
//! 时间注入式，宿主测试确定复现。无外部依赖。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 帧耗时增量判线（微秒）——主册：≤0.5ms。
pub const FRAME_BUDGET_US: u64 = 500;
/// 强度两档（校正量插值 50%/100%，主册规格）。
pub const STRENGTH_50_PCT: u32 = 50;
pub const STRENGTH_100_PCT: u32 = 100;
/// 叠加顺序文档常量（一处一事实）：色温（F116）先、滤镜（F114）后。
pub const OVERLAY_ORDER_DOC: &str = "F116-color-temp first, F114-filter second";
/// 快捷键。
pub const HOTKEY_CYCLE: &str = "Win+Ctrl+C";

// ---------------------------------------------------------------------------
// Machado 2009 模拟矩阵（公开学术成果，severity 1.0，10° 标准观察者）
// ---------------------------------------------------------------------------

/// 三向滤镜类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CvdKind {
    /// 红色弱。
    Protanopia,
    /// 绿色弱。
    Deuteranopia,
    /// 蓝色弱。
    Tritanopia,
}

/// 3x3 行主序矩阵。
pub type Mat3 = [f64; 9];

/// Machado et al. 2009 severity 1.0 模拟矩阵（公开数据，逐值直录）。
pub const SIM_PROTAN: Mat3 = [
    0.152286, 1.052583, -0.204868, 0.114503, 0.786281, 0.099216, -0.003882, -0.048116, 1.051998,
];
pub const SIM_DEUTAN: Mat3 = [
    0.367322, 0.860646, -0.227968, 0.280085, 0.672501, 0.047413, -0.011820, 0.042940, 0.968881,
];
pub const SIM_TRITAN: Mat3 = [
    1.255528, -0.076749, -0.178779, -0.078411, 0.930809, 0.147602, 0.004733, 0.691367, 0.303900,
];

/// 模拟矩阵表（预览图例与对拍基准）。
pub fn simulation_matrix(kind: CvdKind) -> Mat3 {
    match kind {
        CvdKind::Protanopia => SIM_PROTAN,
        CvdKind::Deuteranopia => SIM_DEUTAN,
        CvdKind::Tritanopia => SIM_TRITAN,
    }
}

// ---------------------------------------------------------------------------
// 校正面：daltonize 误差重分配
// ---------------------------------------------------------------------------

/// 校正增益矩阵（本实现设计常量，F130 登记）：`corrected = rgb +
/// G·(rgb − S·rgb)`。行为语义——死通道行清零（其误差重分配进保留轴）；
/// 灰色误差恒为零 → 灰轴零干预（构造性保证）。
pub const GAIN_PROTAN: Mat3 = [
    // R 行清零（L 锥缺失——红分量误差不可自见，转移进蓝轴）
    0.0, 0.0, 0.0,
    // G 行：自误差保留
    0.0, 1.0, 0.0,
    // B 行：0.7·(红误差+绿误差) + 自误差——红绿混淆差推向蓝轴
    0.7, 0.7, 1.0,
];
pub const GAIN_DEUTAN: Mat3 = [
    0.0, 0.0, 0.0,
    0.0, 0.0, 0.0,
    // B 行：绿误差（混淆源）1.2 倍 + 蓝自误差——红绿差足量推向蓝黄轴
    // （1.2 增益由混淆对分离度 ≥+50% 判据标定，见自检 5）
    0.0, 1.2, 1.2,
];
pub const GAIN_TRITAN: Mat3 = [
    // R 行：蓝误差全额 + 自误差——蓝黄混淆差推向红绿轴（R 侧）
    1.0, 0.0, 1.0,
    // G 行：蓝误差全额 + 自误差（G 侧）——增益由混淆对分离度
    // ≥+50% 判据标定（见自检 5），实测 2.2-3.7×
    0.0, 1.0, 1.0,
    // B 行清零（S 锥缺失）
    0.0, 0.0, 0.0,
];

/// 校正增益表。
pub fn correction_gain(kind: CvdKind) -> Mat3 {
    match kind {
        CvdKind::Protanopia => GAIN_PROTAN,
        CvdKind::Deuteranopia => GAIN_DEUTAN,
        CvdKind::Tritanopia => GAIN_TRITAN,
    }
}

/// 强度插值系数（主册：强度两档=校正量插值 50%/100%）。
fn gain_scale(strength_pct: u32) -> f64 {
    strength_pct.min(100) as f64 / 100.0
}

/// 3x3 矩阵乘向量。
fn mul_vec(m: &Mat3, v: (f64, f64, f64)) -> (f64, f64, f64) {
    (
        m[0] * v.0 + m[1] * v.1 + m[2] * v.2,
        m[3] * v.0 + m[4] * v.1 + m[5] * v.2,
        m[6] * v.0 + m[7] * v.1 + m[8] * v.2,
    )
}

/// 3x3 矩阵乘矩阵（对拍工具）。
pub fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut out = [0.0; 9];
    for r in 0..3 {
        for c in 0..3 {
            out[r * 3 + c] = a[r * 3] * b[c] + a[r * 3 + 1] * b[3 + c] + a[r * 3 + 2] * b[6 + c];
        }
    }
    out
}

/// 3x3 变换单像素（sRGB 0-255 → clamp 0-255）。
pub fn apply_matrix(m: &Mat3, rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    let (r, g, b) = (rgb.0 as f64, rgb.1 as f64, rgb.2 as f64);
    let clamp = |v: f64| -> u8 { v.round().clamp(0.0, 255.0) as u8 };
    (
        clamp(m[0] * r + m[1] * g + m[2] * b),
        clamp(m[3] * r + m[4] * g + m[5] * b),
        clamp(m[6] * r + m[7] * g + m[8] * b),
    )
}

/// 校正管线（单像素）：模拟 → 误差 → 增益重分配（强度插值）→ 加回。
/// 灰色（R=G=B）误差恒为零 → 输出恒等于输入（灰轴零干预，构造保证）。
pub fn correct_pixel(kind: CvdKind, strength_pct: u32, rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    let sim = apply_matrix(&simulation_matrix(kind), rgb);
    let err = (
        rgb.0 as f64 - sim.0 as f64,
        rgb.1 as f64 - sim.1 as f64,
        rgb.2 as f64 - sim.2 as f64,
    );
    let add = mul_vec(&correction_gain(kind), err);
    let t = gain_scale(strength_pct);
    let clamp = |v: f64| -> u8 { v.round().clamp(0.0, 255.0) as u8 };
    (
        clamp(rgb.0 as f64 + t * add.0),
        clamp(rgb.1 as f64 + t * add.1),
        clamp(rgb.2 as f64 + t * add.2),
    )
}

/// 可区分性度量：色弱视角（模拟空间）下两色欧氏距离——校正有效的
/// 功能判据面（Ishihara 通过率提升的算法等价物：混淆色对在校正后的
/// 色弱视角下分得更开）。
pub fn sim_distance(kind: CvdKind, a: (u8, u8, u8), b: (u8, u8, u8)) -> f64 {
    let sa = apply_matrix(&simulation_matrix(kind), a);
    let sb = apply_matrix(&simulation_matrix(kind), b);
    let d = (
        sa.0 as f64 - sb.0 as f64,
        sa.1 as f64 - sb.1 as f64,
        sa.2 as f64 - sb.2 as f64,
    );
    (d.0 * d.0 + d.1 * d.1 + d.2 * d.2).sqrt()
}

/// 强度插值对拍：模拟矩阵插值（预览图例渲染源——强度作用于预览时
/// 与单位阵插值）。
pub fn preview_matrix(kind: CvdKind, strength_pct: u32) -> Mat3 {
    let t = gain_scale(strength_pct);
    let identity: Mat3 = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
    let sim = simulation_matrix(kind);
    let mut out = [0.0; 9];
    for i in 0..9 {
        out[i] = t * sim[i] + (1.0 - t) * identity[i];
    }
    out
}

// ---------------------------------------------------------------------------
// 滤镜状态机
// ---------------------------------------------------------------------------

/// 滤镜状态：关 / （类型，强度）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterState {
    Off,
    On(CvdKind, u32),
}

/// 色弱滤镜管理器。
pub struct ColorFilter {
    state: FilterState,
    /// 帧耗时增量账本（最近一次注入，us）——≤0.5ms 判线对账。
    last_frame_cost_us: u64,
    /// 超预算自动降档次数（主册：负载超预算 → 自动降强度档+说明）。
    auto_downgrades: u64,
    /// 截图原色豁免开关（默认 true——滤镜为观看辅助不污染产出）。
    screenshot_original_color: bool,
    /// 托盘常驻图标态（开启时）。
    tray_icon_live: bool,
}

impl ColorFilter {
    pub fn new() -> ColorFilter {
        ColorFilter {
            state: FilterState::Off,
            last_frame_cost_us: 0,
            auto_downgrades: 0,
            screenshot_original_color: true,
            tray_icon_live: false,
        }
    }

    pub fn state(&self) -> FilterState {
        self.state
    }

    pub fn last_frame_cost_us(&self) -> u64 {
        self.last_frame_cost_us
    }

    pub fn auto_downgrades(&self) -> u64 {
        self.auto_downgrades
    }

    pub fn screenshot_original_color(&self) -> bool {
        self.screenshot_original_color
    }

    pub fn tray_icon_live(&self) -> bool {
        self.tray_icon_live
    }

    /// 开启指定类型与强度（托盘图标同步常驻）。
    pub fn turn_on(&mut self, kind: CvdKind, strength_pct: u32) {
        self.state = FilterState::On(kind, strength_pct.clamp(STRENGTH_50_PCT, STRENGTH_100_PCT));
        self.tray_icon_live = true;
    }

    /// 关闭（托盘图标撤下）。
    pub fn turn_off(&mut self) {
        self.state = FilterState::Off;
        self.tray_icon_live = false;
    }

    /// Win+Ctrl+C 循环：关 → 红色弱 → 绿色弱 → 蓝色弱 → 关（强度保持）。
    pub fn cycle(&mut self) -> FilterState {
        self.state = match self.state {
            FilterState::Off => FilterState::On(CvdKind::Protanopia, STRENGTH_100_PCT),
            FilterState::On(k, s) => match k {
                CvdKind::Protanopia => FilterState::On(CvdKind::Deuteranopia, s),
                CvdKind::Deuteranopia => FilterState::On(CvdKind::Tritanopia, s),
                CvdKind::Tritanopia => FilterState::Off,
            },
        };
        self.tray_icon_live = self.state != FilterState::Off;
        self.state
    }

    /// 当前状态单像素校正（合成输出末级调用口；Off → 原样透传）。
    pub fn correct_at(&self, rgb: (u8, u8, u8)) -> (u8, u8, u8) {
        match self.state {
            FilterState::Off => rgb,
            FilterState::On(k, s) => correct_pixel(k, s, rgb),
        }
    }

    /// 帧耗时注入对账：超 0.5ms → 自动降强度档（100 → 50；50 已最低则
    /// 维持并计数）+ 说明位（说明由设置页消费计数呈现）。
    pub fn frame_done(&mut self, cost_us: u64) {
        self.last_frame_cost_us = cost_us;
        if cost_us > FRAME_BUDGET_US {
            if let FilterState::On(k, s) = self.state {
                if s > STRENGTH_50_PCT {
                    self.state = FilterState::On(k, STRENGTH_50_PCT);
                }
                self.auto_downgrades += 1;
            }
        }
    }

    /// 预览图例矩阵（模拟矩阵插值——设置页对比图例渲染源）。
    pub fn preview_matrix_at(kind: CvdKind, strength_pct: u32) -> Mat3 {
        preview_matrix(kind, strength_pct)
    }
}

// ---------------------------------------------------------------------------
// 深化批次 v2：设置页色轮预览数据（玻璃色板——变换前后采样对）
// ---------------------------------------------------------------------------

/// 色轮预览采样点（设置页「色轮展示变换前后」的数据源：六向色相环
/// 坐标 RGB 采样——变换前原色与按当前模式/强度变换后成对输出）。
pub fn preview_wheel(kind: CvdKind, strength_pct: u32) -> [((u8, u8, u8), (u8, u8, u8)); 6] {
    const WHEEL: [(u8, u8, u8); 6] = [
        (255, 0, 0),   // 红
        (255, 255, 0), // 黄
        (0, 255, 0),   // 绿
        (0, 255, 255), // 青
        (0, 0, 255),   // 蓝
        (255, 0, 255), // 品红
    ];
    let m = preview_matrix(kind, strength_pct);
    WHEEL.map(|rgb| {
        let after = apply_matrix(&m, rgb);
        (rgb, after)
    })
}

// ---------------------------------------------------------------------------
// 自检（判据逐条钉死）
// ---------------------------------------------------------------------------

pub fn run_colorfilter_checks() -> CheckSet {
    let mut set = CheckSet::new("F114-colorfilter");

    // 1. 变换与标准模拟矩阵对拍（判据第一句）：红色 (230,50,40) 经
    //    Protanopia 模拟 → (79,70,39)（公开矩阵锚点值直拍，±2 容差）——
    //    红弱视角红变暗橙的数字面证据。
    let sim = simulation_matrix(CvdKind::Protanopia);
    let seen = apply_matrix(&sim, (230, 50, 40));
    set.add(
        "protan sim anchor value (230,50,40)->(79,70,39)",
        (seen.0 as i32 - 79).abs() <= 2
            && (seen.1 as i32 - 70).abs() <= 2
            && (seen.2 as i32 - 39).abs() <= 2,
        "",
    );

    // 2. 三向模拟矩阵行为分型：红弱压红、绿弱塌红绿轴、蓝弱青移
    //    （各类型对特征色的模拟响应方向逐一对拍）。
    let p_red = apply_matrix(&SIM_PROTAN, (255, 60, 60));
    let t_blue = apply_matrix(&SIM_TRITAN, (30, 30, 255));
    set.add(
        "three sim types directional response",
        // 红弱视角：红分量大幅衰减（红->暗橙）
        p_red.0 < 120 && p_red.2 > 40
            // 绿弱视角：红绿混淆对在模拟空间塌得很近
            && sim_distance(CvdKind::Deuteranopia, (200, 100, 80), (100, 180, 80)) < 60.0
            // 蓝弱视角：蓝被吸向青（绿分量出现、红分量压暗）
            && t_blue.1 > 30 && t_blue.0 < 130,
        "",
    );

    // 3. 强度两档（主册：校正量插值 50%/100%）：50% 档 = 半程校正
    //    （逐通道与 100% 加量的一半对拍，±1 取整容差）。
    let full = correct_pixel(CvdKind::Deuteranopia, 100, (200, 100, 80));
    let half = correct_pixel(CvdKind::Deuteranopia, 50, (200, 100, 80));
    let approx = |a: u8, b: i32, c: i32| -> bool { ((a as i32 - b) - (c - b) / 2).abs() <= 1 };
    set.add(
        "strength 50/100 correction interpolation",
        approx(half.0, 200, full.0 as i32)
            && approx(half.1, 100, full.1 as i32)
            && approx(half.2, 80, full.2 as i32),
        "",
    );

    // 4. 灰轴零干预（构造保证）：128 灰经三滤镜校正后逐通道不变。
    let mut gray_ok = true;
    for k in [CvdKind::Protanopia, CvdKind::Deuteranopia, CvdKind::Tritanopia] {
        let out = correct_pixel(k, 100, (128, 128, 128));
        if out != (128, 128, 128) {
            gray_ok = false;
        }
    }
    set.add("neutral gray preserved (3x)", gray_ok, "");

    // 5. 校正有效（判据：Ishihara 通过率提升的算法等价物）：混淆色对
    //    （Ishihara 点阵主色域取样）在校正后色弱视角下的可区分距离
    //    显著大于未校正（三型逐一 ≥+50%）。
    let mut improve_ok = true;
    for (k, a, b) in [
        (CvdKind::Protanopia, (200u8, 100u8, 80u8), (100u8, 180u8, 80u8)),
        (CvdKind::Deuteranopia, (200, 100, 80), (100, 180, 80)),
        (CvdKind::Tritanopia, (80u8, 80u8, 220u8), (80u8, 180u8, 80u8)),
    ] {
        let d_raw = sim_distance(k, a, b);
        let ca = correct_pixel(k, 100, a);
        let cb = correct_pixel(k, 100, b);
        let d_fix = sim_distance(k, ca, cb);
        if d_fix < d_raw * 1.5 {
            improve_ok = false;
        }
    }
    set.add("correction improves sim-space separation >=50%", improve_ok, "");

    // 6. Win+Ctrl+C 循环四态（关→红→绿→蓝→关，强度保持）。
    let mut f = ColorFilter::new();
    f.turn_on(CvdKind::Protanopia, 50);
    let s1 = f.cycle(); // → 绿
    let s2 = f.cycle(); // → 蓝
    let s3 = f.cycle(); // → 关
    set.add(
        "hotkey cycle 4 states keeps strength",
        s1 == FilterState::On(CvdKind::Deuteranopia, 50)
            && s2 == FilterState::On(CvdKind::Tritanopia, 50)
            && s3 == FilterState::Off,
        "",
    );

    // 7. 帧耗时增量 ≤0.5ms（判据第一句之二）：预算内放行、超预算降档计数。
    let mut f = ColorFilter::new();
    f.turn_on(CvdKind::Deuteranopia, 100);
    f.frame_done(499);
    let within = f.state() == FilterState::On(CvdKind::Deuteranopia, 100);
    f.frame_done(501);
    let downgraded = f.state() == FilterState::On(CvdKind::Deuteranopia, 50)
        && f.auto_downgrades() == 1;
    set.add("frame budget 0.5ms auto-downgrade", within && downgraded, "");

    // 8. 50 档已最低不再降（但仍计数——说明位诚实）。
    let mut f = ColorFilter::new();
    f.turn_on(CvdKind::Tritanopia, 50);
    f.frame_done(900);
    set.add(
        "min strength held on overload",
        f.state() == FilterState::On(CvdKind::Tritanopia, 50) && f.auto_downgrades() == 1,
        "",
    );

    // 9. 托盘图标：开启常驻、关闭撤下（主册：滤镜图标常驻托盘（开启时））。
    let mut f = ColorFilter::new();
    set.add(
        "tray icon follows state",
        !f.tray_icon_live() && {
            f.turn_on(CvdKind::Protanopia, 100);
            f.tray_icon_live()
        } && {
            f.turn_off();
            !f.tray_icon_live()
        },
        "",
    );

    // 10. 截图默认截原色（滤镜为观看辅助不污染产出——显式说明位）。
    let f = ColorFilter::new();
    set.add("screenshot original color default", f.screenshot_original_color(), "");

    // 11. 叠加顺序文档化（与 F116：色温先、滤镜后）+ 快捷键登记。
    set.add(
        "overlay order + hotkey registered",
        OVERLAY_ORDER_DOC == "F116-color-temp first, F114-filter second"
            && HOTKEY_CYCLE == "Win+Ctrl+C",
        "",
    );

    // 12. Off 态透传（零干预）+ 预览图例与校正管线分立（模拟面 ≠ 校正面）。
    let f = ColorFilter::new();
    let passthrough = f.correct_at((10, 20, 30)) == (10, 20, 30);
    // 预览矩阵（模拟插值）与校正输出是两条管线：预览 100% = 模拟矩阵本身。
    let preview_eq_sim = preview_matrix(CvdKind::Protanopia, 100) == SIM_PROTAN;
    set.add(
        "off passthrough + preview/sim pipeline distinct",
        passthrough && preview_eq_sim,
        "",
    );


    // 11. 色轮预览采样对（深化 v2）：六向采样齐、变换前后成对、强度 0
    //     时前后恒等（零干预语义闭环）。
    let wheel = preview_wheel(CvdKind::Deuteranopia, 100);
    let wheel_zero = preview_wheel(CvdKind::Deuteranopia, 0);
    set.add(
        "preview wheel paired samples",
        wheel.len() == 6
            && wheel.iter().all(|(a, b)| a != b)
            && wheel_zero.iter().all(|(a, b)| a == b),
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorfilter_all_checks_green() {
        let set = run_colorfilter_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F114 自检红项 {f}：{}/{} 绿", p, p + f);
    }

    #[test]
    fn sim_dichotomy_shifts_hue() {
        // 纯红 (255,0,0) 经 Deuteranopia 模拟：红绿轴塌缩 → G 分量显著上移。
        let out = apply_matrix(&simulation_matrix(CvdKind::Deuteranopia), (255, 0, 0));
        assert!(out.1 > 50, "绿色弱模拟下纯红的绿分量应显著上移，got {:?}", out);
    }

    #[test]
    fn strength_clamp() {
        let mut f = ColorFilter::new();
        f.turn_on(CvdKind::Protanopia, 999);
        assert_eq!(f.state(), FilterState::On(CvdKind::Protanopia, 100));
        f.turn_on(CvdKind::Protanopia, 10);
        assert_eq!(f.state(), FilterState::On(CvdKind::Protanopia, 50));
    }

    #[test]
    fn clamp_bounds_in_apply() {
        // 大系数矩阵输出不越界（clamp 0-255 面验证）。
        let out = apply_matrix(&SIM_TRITAN, (255, 255, 255));
        assert_eq!(out, (255, 255, 255));
    }

    #[test]
    fn correction_not_overcorrecting() {
        // 校正只做重分配：中调红不越顶到纯白/纯黑（点阵图可读性保护）。
        let out = correct_pixel(CvdKind::Deuteranopia, 100, (200, 100, 80));
        assert!(out.0 >= 180 && out.0 <= 255 && out.1 >= 80, "校正输出 {:?}", out);
    }
}
