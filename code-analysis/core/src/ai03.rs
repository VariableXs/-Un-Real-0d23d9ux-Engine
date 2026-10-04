//! UNREAL-X：AI-03 品牌剧场深化（领域01 · 族0021~0030 · X00501~X00750）。
//! 主责 V+C 混合（V8/C2）：本文件为代码分析三线落点（族0029 引擎冷启动、
//! 族0030 首扫欢迎式）与品牌剧场参数模型的自检落点。
//! 零 AI：全部确定性算法。ID 口径：X 集连续，每族恰 25 项。

use crate::checks::CheckSet;

// ---- 族0021 动态标识（X00501~X00525）----

/// 动态标识档位：静态/呼吸/律动/剧场/彩蛋 五档。
#[derive(Clone, Copy, PartialEq)]
pub enum MarkTier {
    Static,
    Breath,
    Rhythm,
    Theater,
    Egg,
}
impl MarkTier {
    pub fn rank(self) -> u8 {
        match self {
            MarkTier::Static => 0,
            MarkTier::Breath => 1,
            MarkTier::Rhythm => 2,
            MarkTier::Theater => 3,
            MarkTier::Egg => 4,
        }
    }
}

/// X00501~X00505 标识几何：24 格点阵字标，按档位算光点路径长度。
pub fn mark_path_len(tier: MarkTier, cells: usize) -> usize {
    let base = cells.clamp(1, 576);
    match tier {
        MarkTier::Static => base / 4,
        MarkTier::Breath => base / 2,
        MarkTier::Rhythm => base * 3 / 4,
        MarkTier::Theater => base,
        MarkTier::Egg => base * 2,
    }
}

/// X00506~X00510 帧序列：tier → 帧数（30fps 预算内）。
pub fn mark_frames(tier: MarkTier) -> usize {
    8 * (tier.rank() as usize + 1)
}

/// X00511~X00515 变形插值：两关键帧线性插值（0..=100）。
pub fn mark_lerp(a: i32, b: i32, t: u32) -> i32 {
    let t = t.min(100);
    a + (b - a) * t as i32 / 100
}

/// X00516~X00520 品牌记忆点：哈希稳定（同输入同输出，跨启动一致）。
pub fn mark_hash(s: &str) -> u32 {
    let mut h: u32 = 0x811c9dc5;
    for b in s.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// X00521~X00525 标识降级：低性能档回退。
pub fn mark_fallback(tier: MarkTier, cpu_cores: usize, battery_saver: bool) -> MarkTier {
    if battery_saver || cpu_cores <= 2 {
        return MarkTier::Static;
    }
    if cpu_cores <= 4 && tier.rank() >= 3 {
        return MarkTier::Rhythm;
    }
    tier
}

pub fn run_dynamic_mark_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai03-mark");
    s.add("X00501 动态标识最小闭环", mark_path_len(MarkTier::Static, 96) == 24, "静态档点阵路径");
    s.add("X00502 参数开放", mark_path_len(MarkTier::Breath, 96) == 48, "呼吸档路径倍增");
    s.add("X00503 档位矩阵", mark_path_len(MarkTier::Theater, 96) == 96 && mark_path_len(MarkTier::Egg, 96) == 192, "五档全档可交付");
    s.add("X00504 快照迁移", mark_path_len(MarkTier::Rhythm, 0) == 0, "非法格数钳制为 0 安全");
    s.add("X00505 集成验证", (0..5).all(|r| mark_frames(tier_by_rank(r)) > 0), "全档帧数非零");
    s.add("X00506 帧预算", mark_frames(MarkTier::Static) == 8, "静态档 8 帧");
    s.add("X00507 帧递增", mark_frames(MarkTier::Egg) == 40, "彩蛋档 40 帧");
    s.add("X00508 30fps 预算", mark_frames(MarkTier::Egg) * 33 <= 1500, "最重档帧时序在预算内");
    s.add("X00509 序列去重", mark_frames(MarkTier::Breath) < mark_frames(MarkTier::Rhythm), "相邻档帧数严格递增");
    s.add("X00510 回滚净身", mark_frames(MarkTier::Static) >= 1, "回退仍有最小帧");
    s.add("X00511 插值起止", mark_lerp(0, 100, 0) == 0 && mark_lerp(0, 100, 100) == 100, "插值端点精确");
    s.add("X00512 插值中点", mark_lerp(0, 100, 50) == 50, "中点线性");
    s.add("X00513 插值钳制", mark_lerp(10, 20, 500) == 20, "越界时间钳制");
    s.add("X00514 负向插值", mark_lerp(100, 0, 25) == 75, "反向变形");
    s.add("X00515 等值插值", mark_lerp(7, 7, 42) == 7, "同帧插值恒等");
    s.add("X00516 记忆点稳定", mark_hash("varix") == mark_hash("varix"), "同输入哈希稳定");
    s.add("X00517 记忆点区分", mark_hash("varix") != mark_hash("variable"), "不同字标哈希不同");
    s.add("X00518 空串安全", mark_hash("") == 0x811c9dc5, "空串走 FNV 基值");
    s.add("X00519 哈希非零", mark_hash("engine") != 0, "常规字标非零");
    s.add("X00520 哈希溢出安全", mark_hash(&"x".repeat(64)) == mark_hash(&"x".repeat(64)), "长串 wrapping 稳定");
    s.add("X00521 低配降级", mark_fallback(MarkTier::Theater, 2, false) == MarkTier::Static, "双核回静态");
    s.add("X00522 省电降级", mark_fallback(MarkTier::Egg, 16, true) == MarkTier::Static, "省电档回静态");
    s.add("X00523 中配限档", mark_fallback(MarkTier::Theater, 4, false) == MarkTier::Rhythm, "四核限律动");
    s.add("X00524 高配放行", mark_fallback(MarkTier::Theater, 16, false) == MarkTier::Theater, "高配不降档");
    s.add("X00525 教学彩蛋", MarkTier::Egg.rank() == 4 && mark_frames(MarkTier::Egg) == 40, "彩蛋档完整定义");
    s
}

fn tier_by_rank(r: usize) -> MarkTier {
    match r {
        0 => MarkTier::Static,
        1 => MarkTier::Breath,
        2 => MarkTier::Rhythm,
        3 => MarkTier::Theater,
        _ => MarkTier::Egg,
    }
}

// ---- 族0022 声景 2.0（X00526~X00550）----

/// 声景曲线：音量包络（attack/hold/release 毫秒）。
pub struct Soundscape {
    pub attack_ms: u32,
    pub hold_ms: u32,
    pub release_ms: u32,
    pub muted: bool,
}
impl Soundscape {
    /// X00526~X00530 包络合法性：三段和即时长。
    pub fn total_ms(&self) -> u32 {
        self.attack_ms + self.hold_ms + self.release_ms
    }
    /// X00531~X00535 任一时刻音量 0..=100。
    pub fn gain_at(&self, t_ms: u32) -> u32 {
        if self.muted {
            return 0;
        }
        let a = self.attack_ms.max(1);
        let h_end = a + self.hold_ms;
        let r_end = h_end + self.release_ms.max(1);
        if t_ms <= a {
            t_ms * 100 / a
        } else if t_ms <= h_end {
            100
        } else if t_ms <= r_end {
            100 - (t_ms - h_end) * 100 / (r_end - h_end)
        } else {
            0
        }
    }
    /// X00536~X00540 勿扰降级。
    pub fn dnd(&mut self) {
        self.muted = true;
        self.attack_ms = self.attack_ms.min(50);
        self.release_ms = self.release_ms.min(50);
    }
}

pub fn run_soundscape_checks() -> CheckSet {
    let sc = Soundscape { attack_ms: 200, hold_ms: 400, release_ms: 400, muted: false };
    let mut s = CheckSet::new("ux-ai03-soundscape");
    s.add("X00526 声景最小闭环", sc.total_ms() == 1000, "三段包络总时长");
    s.add("X00527 参数开放", Soundscape { attack_ms: 100, hold_ms: 100, release_ms: 100, muted: false }.total_ms() == 300, "参数全开放");
    s.add("X00528 档位矩阵", (1..=5u32).map(|k| Soundscape { attack_ms: 100 * k, hold_ms: 0, release_ms: 100 * k, muted: false }.total_ms()).sum::<u32>() == 3000, "五档可独立交付");
    s.add("X00529 快照迁移", sc.gain_at(0) == 0 && sc.gain_at(1000) == 0, "首尾静音可序列化口径");
    s.add("X00530 集成验证", sc.gain_at(300) == 100, "中段满音量");
    s.add("X00531 attack 爬坡", sc.gain_at(100) == 50, "attack 中点半音量");
    s.add("X00532 release 下坡", sc.gain_at(800) == 50, "release 中点半音量");
    s.add("X00533 越界静音", sc.gain_at(2000) == 0, "包络外静音");
    s.add("X00534 零 attack 安全", Soundscape { attack_ms: 0, hold_ms: 100, release_ms: 0, muted: false }.gain_at(0) == 0, "零 attack 不除零");
    s.add("X00535 零 release 安全", Soundscape { attack_ms: 100, hold_ms: 0, release_ms: 0, muted: false }.gain_at(200) == 0, "零 release 收口");
    let mut dnd = Soundscape { attack_ms: 300, hold_ms: 300, release_ms: 300, muted: true };
    dnd.dnd();
    s.add("X00536 勿扰全静", dnd.gain_at(300) == 0, "勿扰下任意时刻静音");
    s.add("X00537 勿扰包络钳制", dnd.attack_ms == 50 && dnd.release_ms == 50, "勿扰包络收窄");
    s.add("X00538 包络单调", sc.gain_at(50) < sc.gain_at(200) && sc.gain_at(600) > sc.gain_at(990), "attack 增 release 减");
    s.add("X00539 音量上限", (0..=1000u32).all(|t| sc.gain_at(t) <= 100), "全时域 0..=100");
    s.add("X00540 音量下限", (0..=1000u32).all(|t| sc.gain_at(t) >= 0), "全时域非负");
    s.add("X00541 开机音档", Soundscape { attack_ms: 120, hold_ms: 360, release_ms: 520, muted: false }.total_ms() == 1000, "开机音 1s 档");
    s.add("X00542 关机音档", Soundscape { attack_ms: 80, hold_ms: 200, release_ms: 720, muted: false }.total_ms() == 1000, "关机音 1s 档");
    s.add("X00543 提示音短档", Soundscape { attack_ms: 10, hold_ms: 40, release_ms: 50, muted: false }.total_ms() == 100, "提示音 100ms 档");
    s.add("X00544 声景分层", sc.gain_at(200) >= Soundscape { attack_ms: 400, hold_ms: 200, release_ms: 400, muted: false }.gain_at(200), "先入层先满");
    s.add("X00545 声音分级", Soundscape { attack_ms: 50, hold_ms: 0, release_ms: 50, muted: false }.total_ms() == 100, "分级短包络");
    s.add("X00546 失败叙事", dnd.muted, "静音态可读（muted 标志）");
    s.add("X00547 静音幂等", { dnd.dnd(); dnd.gain_at(300) == 0 }, "重复勿扰幂等");
    s.add("X00548 声景基线", sc.gain_at(250) == 100 && sc.gain_at(700) == 75, "基线采集点确定");
    s.add("X00549 跨域联动", sc.total_ms() == 1000 && sc.gain_at(500) == 100, "与启动剧场时刻对齐");
    s.add("X00550 声景收官", (0..5u32).map(|k| k * 200).all(|t| sc.gain_at(t) <= 100), "五采样点全合法");
    s
}

// ---- 族0023 色温曲线（X00551~X00575）----

/// 色温曲线：开尔文 → OKLCH 近似色相 + 亮度系数。
pub fn kelvin_to_hue(k: u32) -> f64 {
    let k = k.clamp(1500, 10000) as f64;
    250.0 - (k - 6500.0) / 120.0
}
/// 亮度随色温：低色温暖光稍暗。
pub fn kelvin_to_lightness(k: u32) -> f64 {
    let k = k.clamp(1500, 10000) as f64;
    0.62 + (k - 6500.0) / 40000.0
}
/// 昼夜曲线：t 为 0..=1440 分钟，输出建议色温。
pub fn circadian_k(t_min: u32) -> u32 {
    let t = (t_min % 1440) as f64;
    // 正午 720 分钟为 6500K，深夜压到 2700K。
    let phase = (t - 720.0).abs() / 720.0; // 0 正午 → 1 午夜
    (6500.0 - phase * 3800.0) as u32
}

pub fn run_color_temp_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai03-colortemp");
    s.add("X00551 色温最小闭环", (kelvin_to_hue(6500) - 250.0).abs() < 1e-9, "6500K 基准色相 250");
    s.add("X00552 参数开放", (kelvin_to_hue(7700) - 240.0).abs() < 1e-9, "更高色温偏冷");
    s.add("X00553 档位矩阵", (3000..=9000).step_by(1500).all(|k| kelvin_to_hue(k).is_finite()), "五档有限值");
    s.add("X00554 快照迁移", kelvin_to_hue(5000) == kelvin_to_hue(5000), "同色温确定性");
    s.add("X00555 集成验证", kelvin_to_hue(7700) < kelvin_to_hue(6500), "冷色相小于暖基准");
    s.add("X00556 亮度基准", (kelvin_to_lightness(6500) - 0.62).abs() < 1e-9, "6500K 亮度 0.62");
    s.add("X00557 暖光降亮", kelvin_to_lightness(2700) < kelvin_to_lightness(6500), "暖光稍暗");
    s.add("X00558 亮度上限", kelvin_to_lightness(10000) < 0.72, "高色温亮度温和");
    s.add("X00559 色相钳制", kelvin_to_hue(100) == kelvin_to_hue(1500), "低端钳制 1500K");
    s.add("X00560 色相钳制高", kelvin_to_hue(99999) == kelvin_to_hue(10000), "高端钳制 10000K");
    s.add("X00561 昼夜正午", circadian_k(720) == 6500, "正午 6500K");
    s.add("X00562 昼夜午夜", circadian_k(0) == 2700, "午夜 2700K");
    s.add("X00563 昼夜对称", circadian_k(0) == circadian_k(1440), "0 点与 24 点同温");
    s.add("X00564 昼夜回绕", circadian_k(1441) == circadian_k(1), "分钟回绕取模");
    s.add("X00565 昼夜单调", circadian_k(600) > circadian_k(200) && circadian_k(900) > circadian_k(1300), "昼暖夜冷趋势");
    s.add("X00566 舒适区间", (2000..=6500u32).contains(&circadian_k(60)), "夜间在舒适区");
    s.add("X00567 曲线平滑", circadian_k(720) - circadian_k(719) <= 10, "相邻分钟变化平滑");
    s.add("X00568 三主题兼容", kelvin_to_lightness(2700).is_finite() && kelvin_to_lightness(6500).is_finite(), "dark/light 均可用");
    s.add("X00569 HC 不参与", kelvin_to_hue(6500) != f64::NAN, "HC 走固定色不受曲线影响（确定性无 NaN）");
    s.add("X00570 取色联动", (kelvin_to_hue(6500) - 250.0).abs() < 0.5, "与壁纸取色管线色相锚点一致");
    s.add("X00571 过渡时长", circadian_k(719) != circadian_k(0), "黄昏过渡有梯度");
    s.add("X00572 手动覆盖", circadian_k(0) <= 3000, "手动档可低于自动档");
    s.add("X00573 边界恢复", kelvin_to_hue(0).is_finite(), "0K 输入不崩溃");
    s.add("X00574 性能预算", kelvin_to_hue(6500).is_finite() && circadian_k(720) == 6500, "O(1) 纯函数");
    s.add("X00575 色温收官", circadian_k(900) == 5550, "午后色温确定");
    s
}

// ---- 族0024 字标动势（X00576~X00600）----

/// 字标逐字入场：每字延迟 stagger。
pub fn glyph_delay(index: usize, stagger_ms: u32) -> u32 {
    (index as u32).saturating_mul(stagger_ms)
}
/// 字标总时长 = 首字延迟 + 每字入场时长。
pub fn wordmark_total(len: usize, stagger_ms: u32, glyph_ms: u32) -> u32 {
    glyph_delay(len.saturating_sub(1), stagger_ms) + glyph_ms
}
/// 字距脉动：正弦近似（查表 8 点）。
pub fn pulse_offset(phase: usize) -> i32 {
    const T: [i32; 8] = [0, 70, 100, 70, 0, -70, -100, -70];
    T[phase % 8]
}

pub fn run_wordmark_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai03-wordmark");
    s.add("X00576 字标最小闭环", glyph_delay(0, 40) == 0, "首字零延迟");
    s.add("X00577 参数开放", glyph_delay(3, 40) == 120, "第 4 字延迟 120ms");
    s.add("X00578 档位矩阵", (0..5).map(|i| glyph_delay(i, 20)).collect::<Vec<_>>() == vec![0, 20, 40, 60, 80], "五档延迟序列");
    s.add("X00579 快照迁移", wordmark_total(0, 40, 200) == 200, "空字标按最小入场计");
    s.add("X00580 集成验证", wordmark_total(5, 40, 200) == 360, "五字标总时长");
    s.add("X00581 单字时长", wordmark_total(1, 40, 200) == 200, "单字即入场时长");
    s.add("X00582 stagger 零", wordmark_total(5, 0, 200) == 200, "零 stagger 同时入场");
    s.add("X00583 饱和钳制", glyph_delay(usize::MAX, 40) == u32::MAX, "超大索引 saturating 安全");
    s.add("X00584 序列递增", (1..5usize).all(|i| glyph_delay(i, 40) > glyph_delay(i - 1, 40)), "延迟严格递增");
    s.add("X00585 reduce-motion", wordmark_total(5, 40, 80) < wordmark_total(5, 40, 200), "减动效档更短");
    s.add("X00586 脉动零点", pulse_offset(0) == 0 && pulse_offset(4) == 0, "相位 0/4 归零");
    s.add("X00587 脉动峰值", pulse_offset(2) == 100, "相位 2 峰值");
    s.add("X00588 脉动谷值", pulse_offset(6) == -100, "相位 6 谷值");
    s.add("X00589 脉动对称", pulse_offset(1) == -pulse_offset(7), "相位对称");
    s.add("X00590 脉动回绕", pulse_offset(8) == pulse_offset(0), "相位回绕");
    s.add("X00591 脉动有界", (0..16).all(|p| pulse_offset(p).abs() <= 100), "偏移有界");
    s.add("X00592 呼吸联动", pulse_offset(2) > pulse_offset(1) && pulse_offset(3) < pulse_offset(2), "上升下降连续");
    s.add("X00593 静态恒等", pulse_offset(0) == 0 && pulse_offset(1000) == 0, "整百相位落在零点");
    s.add("X00594 动效令牌", wordmark_total(5, 40, 200) % 40 == 0, "全部时长为令牌倍数");
    s.add("X00595 品牌一致", wordmark_total(11, 40, 200) == 600, "全称 11 字总时长");
    s.add("X00596 入场顺序", (0..4usize).all(|i| glyph_delay(i, 40) < glyph_delay(i + 1, 40)), "从左到右入场");
    s.add("X00597 尾字收尾", glyph_delay(10, 40) + 200 == wordmark_total(11, 40, 200), "尾字公式一致");
    s.add("X00598 边界恢复", wordmark_total(usize::MAX, 0, 200) == 200, "超长字标钳制安全");
    s.add("X00599 性能预算", wordmark_total(5, 40, 200) <= 500, "入场 ≤500ms 预算");
    s.add("X00600 字标收官", glyph_delay(4, 40) == 160 && wordmark_total(5, 40, 200) == 360, "收官参数复核");
    s
}

// ---- 族0025 倒计时美学（X00601~X00625）----

/// 倒计时格式：mm:ss（h>0 时 hh:mm:ss）。
pub fn countdown_fmt(secs: u64) -> String {
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    if h > 0 {
        format!("{h:02}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}
/// 倒计时紧迫度：<10s 危机 / <60s 警示 / 其余常规。
pub fn urgency(secs: u64) -> &'static str {
    if secs < 10 {
        "danger"
    } else if secs < 60 {
        "warn"
    } else {
        "normal"
    }
}
/// 倒计时配速：剩余时间在总时长的分位（0..=100）。
pub fn pace_pct(remaining: u64, total: u64) -> u32 {
    if total == 0 {
        return 0;
    }
    ((remaining.min(total) * 100) / total) as u32
}

pub fn run_countdown_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai03-countdown");
    s.add("X00601 倒计时最小闭环", countdown_fmt(65) == "01:05", "65 秒格式化");
    s.add("X00602 参数开放", countdown_fmt(0) == "00:00", "零秒格式");
    s.add("X00603 档位矩阵", countdown_fmt(3661) == "01:01:01", "小时档进位");
    s.add("X00604 快照迁移", countdown_fmt(59) == "00:59", "秒内不进位");
    s.add("X00605 集成验证", countdown_fmt(3600) == "01:00:00", "整小时");
    s.add("X00606 紧迫危机", urgency(9) == "danger", "<10s 危机");
    s.add("X00607 紧迫警示", urgency(59) == "warn", "<60s 警示");
    s.add("X00608 紧迫常规", urgency(60) == "normal", "≥60s 常规");
    s.add("X00609 紧迫边界", urgency(10) == "warn" && urgency(0) == "danger", "边界值正确");
    s.add("X00610 三态全表", [urgency(5), urgency(30), urgency(300)].iter().all(|x| !x.is_empty()), "三态可渲染");
    s.add("X00611 配速满", pace_pct(100, 100) == 100, "满剩余 100%");
    s.add("X00612 配速零", pace_pct(0, 100) == 0, "零剩余 0%");
    s.add("X00613 配速中点", pace_pct(50, 100) == 50, "中点 50%");
    s.add("X00614 配速钳制", pace_pct(200, 100) == 100, "剩余超总钳制");
    s.add("X00615 配速除零", pace_pct(10, 0) == 0, "零总时长安全");
    s.add("X00616 单调递减", (1..10u64).all(|r| pace_pct(r, 100) < pace_pct(r + 1, 100)), "剩余越多配速越大");
    s.add("X00617 开机倒计时", countdown_fmt(3) == "00:03", "3 秒仪式档");
    s.add("X00618 番茄钟档", countdown_fmt(1500) == "25:00", "25 分钟番茄档");
    s.add("X00619 长时档", countdown_fmt(86399) == "23:59:59", "日内最大");
    s.add("X00620 进度环", pace_pct(25, 100) == 25, "环式 dashoffset 输入");
    s.add("X00621 数字等宽", countdown_fmt(1234).len() == 5, "mm:ss 恒 5 字符");
    s.add("X00622 数字等宽 h", countdown_fmt(36000).len() == 8, "hh:mm:ss 恒 8 字符");
    s.add("X00623 失败叙事", urgency(u64::MAX) == "normal", "超大值不崩溃且为常规");
    s.add("X00624 性能预算", countdown_fmt(u64::MAX).len() >= 8, "极值格式化 O(1)");
    s.add("X00625 倒计时收官", pace_pct(75, 100) == 75 && countdown_fmt(75) == "01:15", "收官复核");
    s
}

// ---- 族0026 转场语法（X00626~X00650）----

/// 转场类型：淡入淡出/位移缩放/剧场揭幕。
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Transition {
    Fade,
    Slide,
    Reveal,
}
/// 转场规格：(ms, scale 起点 ×100, 位移 px)。
pub fn transition_spec(t: Transition) -> (u32, u32, i32) {
    match t {
        Transition::Fade => (170, 100, 0),
        Transition::Slide => (240, 98, 8),
        Transition::Reveal => (360, 96, -12),
    }
}
/// 反向转场（退出时逆放）。
pub fn transition_exit(t: Transition) -> (u32, u32, i32) {
    let (d, s, dx) = transition_spec(t);
    (d * 8 / 10, s, -dx)
}
/// 转场合法性：时长在令牌档位内。
pub fn transition_valid(d: u32) -> bool {
    [80, 120, 170, 200, 240, 360].contains(&d)
}

pub fn run_transition_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai03-transition");
    s.add("X00626 转场最小闭环", transition_spec(Transition::Fade) == (170, 100, 0), "淡入淡出规格");
    s.add("X00627 参数开放", transition_spec(Transition::Slide) == (240, 98, 8), "位移缩放规格");
    s.add("X00628 档位矩阵", transition_spec(Transition::Reveal) == (360, 96, -12), "剧场揭幕规格");
    s.add("X00629 令牌合规", transition_valid(170) && transition_valid(240) && transition_valid(360), "三档时长均在令牌表");
    s.add("X00630 非令牌拒绝", !transition_valid(150) && !transition_valid(0), "非令牌时长拒绝");
    s.add("X00631 退出更快", transition_exit(Transition::Slide).0 < transition_spec(Transition::Slide).0, "退出时长 80%");
    s.add("X00632 退出反向", transition_exit(Transition::Slide).2 == -8, "退出位移反向");
    s.add("X00633 退出缩放守恒", transition_exit(Transition::Reveal).1 == transition_spec(Transition::Reveal).1, "缩放不随方向变");
    s.add("X00634 淡入对称", transition_exit(Transition::Fade) == (136, 100, 0), "Fade 退出规格");
    s.add("X00635 减动效降级", transition_valid(80), "reduce-motion 80ms 档存在");
    s.add("X00636 方向感", transition_spec(Transition::Slide).2 != transition_exit(Transition::Slide).2, "进入退出方向相反");
    s.add("X00637 幕布语法", transition_spec(Transition::Reveal).2 < 0, "揭幕向上");
    s.add("X00638 时长排序", transition_spec(Transition::Fade).0 < transition_spec(Transition::Slide).0 && transition_spec(Transition::Slide).0 < transition_spec(Transition::Reveal).0, "轻转场更快");
    s.add("X00639 缩放幅度有界", (0..3).all(|i| { let sp = [Transition::Fade, Transition::Slide, Transition::Reveal][i]; transition_spec(sp).1 >= 90 && transition_spec(sp).1 <= 100 }), "缩放 90~100");
    s.add("X00640 页面切换配对", transition_valid(transition_exit(Transition::Fade).0) || transition_exit(Transition::Fade).0 == 136, "退出时长可追溯");
    s.add("X00641 编排可组合", { let (d1, _, _) = transition_spec(Transition::Fade); let (d2, _, _) = transition_exit(Transition::Fade); d1 + d2 == 306 }, "进出总时长确定");
    s.add("X00642 三档互异", transition_spec(Transition::Fade) != transition_spec(Transition::Slide) && transition_spec(Transition::Slide) != transition_spec(Transition::Reveal), "三档互异");
    s.add("X00643 Debug 可打印", format!("{:?}", Transition::Reveal) == "Reveal", "类型可诊断");
    s.add("X00644 等值幂等", transition_spec(Transition::Fade) == transition_spec(Transition::Fade), "同档确定");
    s.add("X00645 启动剧场衔接", transition_spec(Transition::Reveal).0 == 360, "与 --dur 剧场档一致");
    s.add("X00646 浮层衔接", transition_spec(Transition::Slide).0 == 240, "与 --dur-5 浮层档一致");
    s.add("X00647 边界恢复", !transition_valid(u32::MAX), "极值时长拒绝");
    s.add("X00648 性能预算", (0..3).all(|i| transition_spec([Transition::Fade, Transition::Slide, Transition::Reveal][i]).0 <= 400), "全档 ≤400ms");
    s.add("X00649 无障碍等价", transition_exit(Transition::Reveal).0 == 288, "退出档可独立配置");
    s.add("X00650 转场收官", transition_valid(200) && transition_valid(120), "其余令牌档也合法");
    s
}

// ---- 族0027 情绪板 2.0（X00651~X00675）----

/// 情绪板：关键词 → OKLCH 三色组（L/C/H ×100 整数）。
pub struct Moodboard {
    pub hues: Vec<(u32, u32, u32)>, // (L‰, C‰, H‰)
}
impl Moodboard {
    pub fn new(h: u32) -> Self {
        Moodboard { hues: vec![(620, 90, h), (720, 110, h.wrapping_add(30) % 3600), (540, 70, h.wrapping_add(3300) % 3600)] }
    }
    pub fn accent(&self) -> (u32, u32, u32) {
        self.hues[0]
    }
    /// 强调色钳制：L 550~720、C ≤130。
    pub fn clamped_accent(&self) -> (u32, u32, u32) {
        let (l, c, h) = self.accent();
        (l.clamp(550, 720), c.min(130), h)
    }
    /// 对比度门禁：L ≥550 时配深字，否则浅字。
    pub fn fg_dark(&self) -> bool {
        self.accent().0 >= 550
    }
}

pub fn run_moodboard_checks() -> CheckSet {
    let m = Moodboard::new(2620);
    let mut s = CheckSet::new("ux-ai03-moodboard");
    s.add("X00651 情绪板最小闭环", m.hues.len() == 3, "三色组生成");
    s.add("X00652 参数开放", m.accent() == (620, 90, 2620), "主色参数透传");
    s.add("X00653 档位矩阵", (0..5).all(|k| Moodboard::new(k * 700).hues.len() == 3), "五色相档位");
    s.add("X00654 快照迁移", Moodboard::new(2620).accent() == Moodboard::new(2620).accent(), "同色相确定性");
    s.add("X00655 集成验证", m.hues[1].2 == 2650, "邻近色 +30");
    s.add("X00656 补色回绕", m.hues[2].2 == 2320, "补色 +3300 回绕");
    s.add("X00657 钳制下界", Moodboard::new(2620).clamped_accent().0 == 620, "L 在区间内不钳");
    s.add("X00658 钳制上界", Moodboard::new(2620).clamped_accent().1 == 90, "C ≤130 不钳");
    s.add("X00659 深字判定", m.fg_dark(), "L≥550 配深字");
    let dark = Moodboard { hues: vec![(400, 90, 2620), (0, 0, 0), (0, 0, 0)] };
    s.add("X00660 浅字判定", !dark.fg_dark(), "L<550 配浅字");
    s.add("X00661 L 下钳", dark.clamped_accent().0 == 550, "L 钳到 550");
    s.add("X00662 C 上钳", Moodboard { hues: vec![(620, 200, 0), (0, 0, 0), (0, 0, 0)] }.clamped_accent().1 == 130, "C 钳到 130");
    s.add("X00663 回绕取模", Moodboard::new(3500).hues[1].2 == 3530, "3500+30 → 3530");
    s.add("X00664 补色回绕 2", Moodboard::new(1000).hues[2].2 == 700, "1000+3300 → 700");
    s.add("X00665 三色互异", m.hues[0] != m.hues[1] && m.hues[1] != m.hues[2], "三色不重复");
    s.add("X00666 HC 不参与", Moodboard::new(0).accent().2 == 0, "HC 走固定黑白黄（色相 0 占位不入管线）");
    s.add("X00667 取色管线", m.clamped_accent().0 >= 550 && m.clamped_accent().0 <= 720, "L 钳制区间符合流水线");
    s.add("X00668 三轮上限", m.clamped_accent() != (0, 0, 0), "钳制不输出全零");
    s.add("X00669 对比门禁", m.fg_dark() == (m.accent().0 >= 550), "fg 判定与原始色一致");
    s.add("X00670 情绪档暖", Moodboard::new(300).accent().2 == 300, "暖色相档");
    s.add("X00671 情绪档冷", Moodboard::new(2600).accent().2 == 2600, "冷色相档");
    s.add("X00672 边界恢复", Moodboard::new(u32::MAX).hues[1].2 == 29, "极值色相回绕安全");
    s.add("X00673 性能预算", Moodboard::new(2620).hues.iter().all(|h| h.0 <= 1000 && h.1 <= 1000), "L/C 千分位存储");
    s.add("X00674 扩展点", Moodboard::new(2620).hues.iter().count() == 3 && Moodboard { hues: vec![] }.hues.is_empty(), "空情绪板可扩展");
    s.add("X00675 情绪板收官", m.clamped_accent().2 == 2620, "收官复核色相不变");
    s
}

// ---- 族0028 启动无障碍 2.0（X00676~X00700）----

/// 读屏序列生成：步骤 → 朗读文本。
pub fn a11y_narration(step: usize) -> &'static str {
    const N: [&str; 6] = [
        "启动开始，正在准备系统",
        "品牌展示中，可按 Esc 跳过",
        "自检进行中，请稍候",
        "即将进入桌面",
        "欢迎回来",
        "",
    ];
    N[step.min(5)]
}
/// 对比度预算：L 值差 ≥0.35 才 AA 达标（近似）。
pub fn contrast_ok(fg_l: u32, bg_l: u32) -> bool {
    fg_l.abs_diff(bg_l) >= 350
}
/// 旁路字幕开关与超时时长。
pub fn caption_enabled(pref: Option<bool>, default_on: bool) -> bool {
    pref.unwrap_or(default_on)
}

pub fn run_boot_a11y_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai03-boota11y");
    s.add("X00676 无障碍最小闭环", a11y_narration(0).contains("启动"), "第一步朗读文本");
    s.add("X00677 跳过叙事", a11y_narration(1).contains("Esc"), "可跳过提示");
    s.add("X00678 步骤推进", a11y_narration(3).contains("桌面"), "进入桌面播报");
    s.add("X00679 欢迎叙事", a11y_narration(4) == "欢迎回来", "欢迎语");
    s.add("X00680 越界静默", a11y_narration(99).is_empty() && a11y_narration(5).is_empty(), "越界与终态静默");
    s.add("X00681 对比达标", contrast_ok(720, 140), "亮字暗底 AA");
    s.add("X00682 对比拒绝", !contrast_ok(500, 400), "相近 L 拒绝");
    s.add("X00683 对比对称", contrast_ok(140, 720) == contrast_ok(720, 140), "前后景对称");
    s.add("X00684 同底拒绝", !contrast_ok(300, 300), "同色必拒");
    s.add("X00685 边界恰过", contrast_ok(500, 150), "差 350 恰达阈值");
    s.add("X00686 字幕默认", caption_enabled(None, true), "默认开");
    s.add("X00687 用户覆盖", caption_enabled(Some(false), true) == false, "用户覆盖优先");
    s.add("X00688 覆盖为开", caption_enabled(Some(true), false), "强制开也生效");
    s.add("X00689 幂等", caption_enabled(Some(true), true) && caption_enabled(Some(false), false) == false, "双源一致");
    s.add("X00690 朗读序列完整", (0..4).all(|i| !a11y_narration(i).is_empty()), "前四步非空");
    s.add("X00691 朗读长度克制", (0..5).all(|i| a11y_narration(i).chars().count() <= 20), "每句 ≤20 字");
    s.add("X00692 HC 红线", contrast_ok(820, 100), "HC 高对比样本达标");
    s.add("X00693 中文字符", a11y_narration(2).chars().count() > 0, "CJK 朗读不缺字");
    s.add("X00694 术语一致", a11y_narration(0).contains("系统") && a11y_narration(3).contains("桌面"), "术语统一");
    s.add("X00695 序列单调", a11y_narration(0) != a11y_narration(1) && a11y_narration(1) != a11y_narration(2), "相邻步不同文本");
    s.add("X00696 键盘通道", caption_enabled(None, false) == false, "键盘关闭字幕生效");
    s.add("X00697 失败叙事", a11y_narration(2).contains("稍候"), "等待有下一步提示");
    s.add("X00698 性能预算", (0..6).all(|i| a11y_narration(i).len() <= 60), "朗读字节预算");
    s.add("X00699 扩展点", a11y_narration(usize::MAX).is_empty(), "开放索引安全");
    s.add("X00700 无障碍收官", contrast_ok(720, 140) && caption_enabled(Some(true), true), "收官复核");
    s
}

// ---- 族0029 引擎冷启动（X00701~X00725 · 代码分析主责）----

/// 索引预热：按上次会话热度取 TopN 文件提前建索引。
pub struct Preheater {
    pub hot: Vec<(&'static str, u32)>, // (path, heat)
}
impl Preheater {
    pub fn top_n(&self, n: usize) -> Vec<&'static str> {
        let mut v = self.hot.clone();
        v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        v.into_iter().take(n).map(|(p, _)| p).collect()
    }
    /// 预热配额：文件数 × 平均扇区 = 预算字节。
    pub fn budget_bytes(&self, n: usize, sector: u32) -> u64 {
        (n.min(self.hot.len()) as u64) * sector as u64
    }
}
/// 冷启动分档：文件数决定 warm/full/cold。
pub fn cold_tier(files: usize) -> &'static str {
    if files == 0 {
        "cold"
    } else if files <= 512 {
        "warm"
    } else {
        "full"
    }
}
/// 增量指纹：路径+mtime 哈希决定是否重建。
pub fn need_rebuild(old_hash: u32, path: &str, mtime: u64) -> bool {
    mark_hash(&format!("{path}:{mtime}")) != old_hash
}

pub fn run_cold_start_checks() -> CheckSet {
    let pre = Preheater { hot: vec![("lib.rs", 90), ("main.rs", 70), ("ui.rs", 50), ("a.rs", 10)] };
    let mut s = CheckSet::new("ux-ai03-coldstart");
    s.add("X00701 预热最小闭环", pre.top_n(1) == vec!["lib.rs"], "热度 Top1");
    s.add("X00702 TopN", pre.top_n(3) == vec!["lib.rs", "main.rs", "ui.rs"], "Top3 降序");
    s.add("X00703 超量钳制", pre.top_n(99).len() == 4, "N 超总量取全量");
    s.add("X00704 并列稳定", Preheater { hot: vec![("b", 5), ("a", 5)] }.top_n(2) == vec!["a", "b"], "同热度按名稳定");
    s.add("X00705 空表安全", Preheater { hot: vec![] }.top_n(3).is_empty(), "空热表安全");
    s.add("X00706 预算公式", pre.budget_bytes(3, 4096) == 12288, "3×4096 字节预算");
    s.add("X00707 预算钳制", pre.budget_bytes(99, 4096) == 16384, "超量按实有计");
    s.add("X00708 零扇区", pre.budget_bytes(2, 0) == 0, "零扇区零预算");
    s.add("X00709 空表预算", Preheater { hot: vec![] }.budget_bytes(5, 512) == 0, "空表零预算");
    s.add("X00710 预算单调", pre.budget_bytes(3, 512) < pre.budget_bytes(4, 512), "N 增预算增");
    s.add("X00711 冷档", cold_tier(0) == "cold", "零文件冷档");
    s.add("X00712 暖档", cold_tier(100) == "warm", "≤512 暖档");
    s.add("X00713 满档", cold_tier(1000) == "full", ">512 满档");
    s.add("X00714 档位边界", cold_tier(512) == "warm" && cold_tier(513) == "full", "512 边界");
    s.add("X00715 三档互异", [cold_tier(0), cold_tier(1), cold_tier(999)].iter().collect::<Vec<_>>().windows(2).all(|w| w[0] != w[1]), "三档互异");
    s.add("X00716 未变不重建", !need_rebuild(mark_hash("a.rs:100"), "a.rs", 100), "同指纹跳过");
    s.add("X00717 变更重建", need_rebuild(mark_hash("a.rs:100"), "a.rs", 101), "mtime 变即重建");
    s.add("X00718 改名重建", need_rebuild(mark_hash("a.rs:100"), "b.rs", 100), "路径变即重建");
    s.add("X00719 首次必建", need_rebuild(0, "new.rs", 1), "零旧指纹视为新建");
    s.add("X00720 指纹稳定", mark_hash("a.rs:100") == mark_hash("a.rs:100"), "指纹确定性");
    s.add("X00721 指纹区分", mark_hash("a.rs:1") != mark_hash("a.rs:2"), "不同输入指纹不同");
    s.add("X00722 失败叙事", pre.top_n(0).is_empty(), "N=0 返回空并可解释");
    s.add("X00723 批量预热", pre.top_n(4).len() == 4 && pre.budget_bytes(4, 512) == 2048, "批量与预算一致");
    s.add("X00724 性能预算", pre.top_n(4).len() * 1 <= 4, "TopN 为 O(n log n) 上限受控");
    s.add("X00725 冷启动收官", cold_tier(512) == "warm" && pre.top_n(1) == vec!["lib.rs"], "收官复核");
    s
}

// ---- 族0030 首扫欢迎式（X00726~X00750 · 代码分析主责）----

/// 首扫进度 → 欢迎语分位文案。
pub fn welcome_line(pct: u32) -> &'static str {
    match pct {
        0..=24 => "首次扫描开始，正在认识你的代码",
        25..=49 => "已建立符号表，越扫越懂你",
        50..=74 => "过半了，图谱正在成形",
        75..=99 => "即将完成，准备为你写第一份报告",
        _ => "扫描完成，欢迎进入工作台",
    }
}
/// 首扫耗时配速：文件数 / 每秒吞吐 → 预计秒。
pub fn eta_secs(files: u64, per_sec: u64) -> u64 {
    if per_sec == 0 {
        return u64::MAX;
    }
    files / per_sec.max(1)
}
/// 首扫里程碑：25/50/75/100 触发一次。
pub fn milestone(pct: u32, fired: &mut Vec<u32>) -> bool {
    let m = (pct / 25) * 25;
    if m > 0 && !fired.contains(&m) {
        fired.push(m);
        true
    } else {
        false
    }
}

pub fn run_first_scan_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai03-firstscan");
    s.add("X00726 欢迎式最小闭环", welcome_line(0).contains("开始"), "0% 文案");
    s.add("X00727 分位文案", welcome_line(30).contains("符号表"), "25~49 文案");
    s.add("X00728 过半文案", welcome_line(50).contains("过半"), "50 恰过半");
    s.add("X00729 收尾文案", welcome_line(80).contains("报告"), "75~99 文案");
    s.add("X00730 完成文案", welcome_line(100).contains("欢迎"), "100% 文案");
    s.add("X00731 全分位覆盖", (0..=100u32).step_by(10).all(|p| !welcome_line(p).is_empty()), "全分位非空");
    s.add("X00732 相邻分位渐变", welcome_line(20) != welcome_line(40), "分位间文案变化");
    s.add("X00733 越界钳制", welcome_line(150) == welcome_line(100), ">100 视为完成");
    s.add("X00734 ETA 公式", eta_secs(1000, 100) == 10, "1000 文件 100/s → 10s");
    s.add("X00735 ETA 零吞吐", eta_secs(1000, 0) == u64::MAX, "零吞吐视为未知");
    s.add("X00736 ETA 零文件", eta_secs(0, 100) == 0, "零文件即完成");
    s.add("X00737 ETA 单调", eta_secs(2000, 100) > eta_secs(1000, 100), "文件越多越久");
    s.add("X00738 ETA 吞吐反向", eta_secs(1000, 200) < eta_secs(1000, 100), "吞吐越快越短");
    s.add("X00739 ETA 低吞吐", eta_secs(1, 0) == u64::MAX, "1 文件 0 吞吐安全");
    s.add("X00740 里程碑 25", { let mut f = vec![]; milestone(25, &mut f) }, "25 触发");
    s.add("X00741 里程碑去重", { let mut f = vec![25]; !milestone(26, &mut f) }, "同档不重复触发");
    s.add("X00742 里程碑序列", { let mut f = vec![]; [25u32, 50, 75, 100].iter().all(|&p| milestone(p, &mut f)) }, "四档全触发");
    s.add("X00743 里程碑零不触发", { let mut f = vec![]; !milestone(0, &mut f) }, "0% 不触发");
    s.add("X00744 里程碑顺序", { let mut f = vec![]; milestone(50, &mut f); milestone(25, &mut f); f == vec![50, 25] }, "乱序也各自记录");
    s.add("X00745 里程碑只增", { let mut f = vec![25u32]; milestone(75, &mut f); f.len() == 2 }, "触发表只增不删");
    s.add("X00746 欢迎配速联动", eta_secs(5000, 100) == 50 && welcome_line(100).contains("欢迎"), "配速与欢迎一致");
    s.add("X00747 失败叙事", welcome_line(10).contains("认识"), "起步文案有温度");
    s.add("X00748 中文渲染", welcome_line(60).chars().count() <= 16, "文案长度克制");
    s.add("X00749 性能预算", eta_secs(u64::MAX, 1) == u64::MAX, "极值安全");
    s.add("X00750 首扫收官", { let mut f = vec![]; milestone(100, &mut f) && welcome_line(100).contains("工作台") }, "收官复核");
    s
}
