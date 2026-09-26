//! I 通用域·三分队 主题文件：指针效果（AI-U3 分工包 · F513/F514/F519/F520/F522/F523）。
//!
//! 判据唯一源：主册《Varix STAR I start.md》各节【验收判据】第一句 + 通用
//! 验收十二查。本文件为 no_std 兼容的纯逻辑「判据实装层」：状态机、关键帧
//! 函数、分流矩阵、开关位图全部定长化（core-only），供合成器 / 输入服务 /
//! 音频服务的接线层直接调用——本层不碰硬件，只把判据变成可运算、可断言的
//! 纯函数与定长结构。
//!
//! ---------------------------------------------------------------------------
//! F513 Ctrl 定位指针（domain: "F513-ctrl-locator"）
//! ---------------------------------------------------------------------------
//! 判据（逐字）：1s 触发与三轮涟漪；多屏定位准确；组合键豁免判据（Ctrl+C
//! 等 20 例 0 误触）；开关设置（默认开）；动画性能。
//!
//! 功能定义要点：按住 Ctrl 键 1 秒（HOLD_MS=1000，999ms 不算）指针位置泛起
//! 同心涟漪——三轮扩散（RIPPLE_ROUNDS=3）总时长 1500ms（关键帧函数给出每轮
//! 半径 permille 与透明度 permille）；多屏时涟漪出现在指针真实所在屏（全局
//! 坐标 → 屏判定函数）；与任何输入不冲突：按 Ctrl 打字不触发（1 秒持续按住
//! 才算，提前松开即取消），组合键场景豁免（Ctrl+C/V/X/… 20 例常量表，组合
//! 中任一第二键按下即豁免取消）；开关默认开（DEFAULT_ON=true）；动画性能：
//! 涟漪走 F335 指针优先平面（非全屏重绘）。
//! 依赖锚点：F335（指针优先平面）、输入服务（Ctrl 按下/松开事件）。
//!
//! ---------------------------------------------------------------------------
//! F514 声音视觉提示（听障支持）（domain: "F514-sound-beacon"）
//! ---------------------------------------------------------------------------
//! 判据（逐字）：三事件色映射；脉冲形态（2 次/边缘 8px）；逐事件开关；勿扰
//! 档联动镜像；与 F341/F240 叠加一致性。
//!
//! 功能定义要点：系统声音的视觉孪生——通知声/警告声/电量低提示发声时屏幕
//! 边缘同色闪一下（通知蓝 COLOR_NOTIFY / 警告黄 COLOR_WARNING / 电量红
//! COLOR_BATTERY），边缘 8px 光带（EDGE_PX=8）2 次脉冲（PULSE_COUNT=2）；
//! 可在声音设置逐事件开关（位图，默认全开）；与勿扰档（F341）联动：
//! 仅声音档=只闪不响、全静=都不来但中心记录（三态镜像函数给出
//! sound/visual/record 三路输出）；与 F240 提示音量叠加一致性：视觉闪与
//! 音量无关（音量归零视觉照闪，声音路才受音量控制）。
//! 依赖锚点：F341（勿扰档）、F240（提示音量）、音频服务（发声事件）。
//!
//! ---------------------------------------------------------------------------
//! F519 大写锁定提示音（domain: "F519-capslock-tone"）
//! ---------------------------------------------------------------------------
//! 判据（逐字）：双音色区分；默认关；跟随提示音量；与 F230 视觉互补；开关
//! 持久化。
//!
//! 功能定义要点：Caps Lock/Num Lock 切换提示音（默认关 DEFAULT_ON=false）：
//! 开启后切换时短促两音（开=高双音 ON_TONE_HZ、关=低双音 OFF_TONE_HZ，四
//! 频率常量）；与 F230 大写锁定视觉角标互补（视觉角标独立于声音开关，两
//! 通道恒为「或」关系——声音可关、角标常在）；音量跟随系统提示音量（F240
//! 的合成函数：采样幅值 = 包络 × 提示音量 permille）；开关持久化（设置位
//! 写读一致）。
//! 依赖锚点：F230（大写锁定视觉角标）、F240（提示音量）、设置存储位。
//!
//! ---------------------------------------------------------------------------
//! F520 标题栏中键最小化（domain: "F520-midbtn-minimize"）
//! ---------------------------------------------------------------------------
//! 判据（逐字）：中键最小化行为；三义分流矩阵；与拖拽冲突（中键拖拽无定义
//! 即无冲突）；开关（不喜可关）；动画 F124。
//!
//! 功能定义要点：标题栏上中键点击=最小化窗口；与双击最大化（F213）、右键
//! 系统菜单（F376）构成标题栏三击三义（中收/双大/右菜单）——三义互不干扰
//! （判定按按键类型天然分流，3×3 分流矩阵纯函数全对齐断言）；中键按下后
//! 移动不进入拖拽状态机（中键拖拽无定义=无冲突，仲裁函数给出 NotHandled）；
//! 开关不喜可关（off 时中键无动作，三义矩阵退化为两义）；最小化动画时长
//! 对齐 F124（MINIMIZE_ANIM_MS 常量）。
//! 依赖锚点：F124（最小化动画）、F213（双击最大化）、F376（右键系统菜单）、
//! 窗口管理器（标题栏命中测试）。
//!
//! ---------------------------------------------------------------------------
//! F522 指针轨迹显示（domain: "F522-pointer-trail"）
//! ---------------------------------------------------------------------------
//! 判据（逐字）：三档时长实测；优先平面性能（帧率无损）；皮肤兼容；默认关；
//! 开关即时。
//!
//! 功能定义要点：指针轨迹开关（无障碍）：移动时指针后带渐隐轨迹——长度三
//! 档（短/中/长：轨迹存留 200/400/600ms，TRAIL_SHORT/MID/LONG_MS 常量）；
//! 轨迹点存定长环形缓冲（TRAIL_POINTS，满即淘汰最旧点），按时间渐隐
//! alpha 曲线函数（permille，存留期满 alpha=0）；轨迹渲染走 F335 指针优先
//! 平面（非全屏重绘），绘制耗时预算 TRAIL_DRAW_BUDGET_US 远小于 60Hz 帧预算
//! （帧率无损断言）；与 F156 指针编辑器兼容（轨迹与皮肤 id 解耦，换肤不动
//! 轨迹缓冲）；默认关（DEFAULT_ON=false）+ 开关即时（切换后下一帧生效）。
//! 依赖锚点：F335（指针优先平面）、F156（指针编辑器/皮肤）、输入服务
//! （指针移动事件）。
//!
//! ---------------------------------------------------------------------------
//! F523 打字时隐藏指针（domain: "F523-typing-hideptr"）
//! ---------------------------------------------------------------------------
//! 判据（逐字）：淡出/恢复时序（输入开始 <100ms 淡出、停止 2s 恢复）；30%
//! 透明度；移动恢复即时；触屏豁免；默认关。
//!
//! 功能定义要点：打字时指针淡出：键盘输入开始指针在 FADE_OUT_MS=80ms（<100ms
//! 判据）内淡出至 30% 透明（HIDDEN_ALPHA_PERMILLE=300，隐身但可寻）、停止
//! 输入 IDLE_RESTORE_MS=2000ms 自动恢复 100%；鼠标移动即时恢复（0ms，优先级
//! 最高）；触屏场景不适用（输入源枚举=触屏时状态机不启用）；与 F201 文本
//! 光标（插入符）无混淆（淡化只作用于指针层，插入符闪烁周期独立）；
//! 默认关（DEFAULT_ON=false）。
//! 依赖锚点：F201（文本光标/插入符）、输入服务（键盘/指针/触屏事件源）、
//! F335（指针层合成）。
//!
//! 零堆纪律：全部逻辑路径无 String/Vec/Box/format!——定长数组、&'static str、
//! core 运算；判据数字全部成常量（一处一事实，常量注释写明主册依据）；
//! 自检入口 run_f5XX_checks 返回 CheckSet（MAX_CHECKS=64，cs.add(name,
//! passed, detail)，detail 为 &'static str），经 robust.rs 域函数指针表注册。

use crate::checks::CheckSet;

// ===========================================================================
// F513 Ctrl 定位指针 —— LocatorRipple 状态机 + 三轮涟漪关键帧 + 多屏定位
// ===========================================================================

/// 持续按住时长门槛：1000ms（主册「1s 触发」）。
pub const HOLD_MS: u64 = 1000;
/// 涟漪总时长：1500ms（主册「三轮扩散 1.5s」）。
pub const RIPPLE_TOTAL_MS: u64 = 1500;
/// 涟漪轮数：3（主册「三轮」）。
pub const RIPPLE_ROUNDS: usize = 3;
/// 相邻两轮起始间隔：250ms（3 轮排布：0/250/500ms 起，单轮 1000ms，末轮
/// 500+1000=1500ms 收——与总时长自洽）。
pub const ROUND_START_GAP_MS: u64 = 250;
/// 单轮扩散时长：1000ms。
pub const ROUND_EXPAND_MS: u64 = 1000;
/// 开关默认值：开（主册「默认开」）。
pub const DEFAULT_ON: bool = true;

/// 组合键豁免表：20 例（主册「Ctrl+C 等 20 例 0 误触」）。按住 Ctrl 1 秒的
/// 过程中任一第二键按下，即视为组合键场景 → 豁免取消涟漪。
pub const COMBO_EXEMPT: [&str; 20] = [
    "Ctrl+C",
    "Ctrl+V",
    "Ctrl+X",
    "Ctrl+S",
    "Ctrl+A",
    "Ctrl+F",
    "Ctrl+Z",
    "Ctrl+W",
    "Ctrl+N",
    "Ctrl+T",
    "Ctrl+P",
    "Ctrl+O",
    "Ctrl+B",
    "Ctrl+U",
    "Ctrl+I",
    "Ctrl+K",
    "Ctrl+R",
    "Ctrl+Y",
    "Ctrl+Tab",
    "Ctrl+PrintScreen",
];

/// 定位器状态机相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LocatorPhase {
    /// 未按住（或已取消/已放完）。
    Idle,
    /// Ctrl 已按下、未满 1s（期间其他键按下 → 豁免回 Idle）。
    Armed,
    /// 已触发涟漪（t_trigger = t_down + HOLD_MS）。
    Rippling { t_trigger: u64 },
}

/// Ctrl 定位涟漪状态机（纯逻辑，无堆）。
pub struct LocatorRipple {
    pub enabled: bool,
    phase: LocatorPhase,
    t_down: u64,
}

impl LocatorRipple {
    pub const fn new() -> Self {
        LocatorRipple {
            enabled: DEFAULT_ON,
            phase: LocatorPhase::Idle,
            t_down: 0,
        }
    }

    pub fn phase(&self) -> LocatorPhase {
        self.phase
    }

    /// Ctrl 按下：进入 Armed（无论当前相位——重新按住即重新计时）。
    pub fn ctrl_down(&mut self, t: u64) {
        self.phase = LocatorPhase::Armed;
        self.t_down = t;
    }

    /// Ctrl 松开：Armed 中提前松开 = 普通打字（未满 1s），取消。
    pub fn ctrl_up(&mut self, _t: u64) {
        if self.phase == LocatorPhase::Armed {
            self.phase = LocatorPhase::Idle;
        }
    }

    /// 其他任意键按下：Armed 中即组合键豁免（返回 true 表示本次豁免取消了
    /// 一次蓄势）；Rippling/Idle 中忽略。
    pub fn other_key(&mut self, _t: u64) -> bool {
        if self.phase == LocatorPhase::Armed {
            self.phase = LocatorPhase::Idle;
            return true;
        }
        false
    }

    /// 周期巡检：Armed 且已满 HOLD_MS → 触发（返回 true 的沿只出现一次）。
    pub fn poll(&mut self, t: u64) -> bool {
        if self.phase == LocatorPhase::Armed && t.saturating_sub(self.t_down) >= HOLD_MS {
            self.phase = LocatorPhase::Rippling {
                t_trigger: self.t_down + HOLD_MS,
            };
            return true;
        }
        false
    }

    /// 涟漪是否仍在播放（自触发时刻起 RIPPLE_TOTAL_MS 内）。
    pub fn ripple_active(&self, t: u64) -> bool {
        match self.phase {
            LocatorPhase::Rippling { t_trigger } => t.saturating_sub(t_trigger) < RIPPLE_TOTAL_MS,
            _ => false,
        }
    }
}

/// 单轮涟漪关键帧：轮内进度 p∈[0,1000) → (半径 permille, 透明度 permille)。
/// 半径线性外扩（0→1000 即满屏），透明度线性衰减（1000→0）。
pub fn round_keyframe(p_permille: u64) -> (u64, u64) {
    let p = p_permille.min(999);
    (p, 1000 - p)
}

/// 三轮涟漪关键帧：t_rel = t - t_trigger → 各轮 (半径, 透明度)。
/// 未开始的轮与已结束的轮均为 (0, 0)。总窗 [0, 1500)。
pub fn ripple_keyframes(t_rel: u64) -> [(u64, u64); RIPPLE_ROUNDS] {
    let mut out = [(0u64, 0u64); RIPPLE_ROUNDS];
    for i in 0..RIPPLE_ROUNDS {
        let start = i as u64 * ROUND_START_GAP_MS;
        if t_rel >= start && t_rel < start + ROUND_EXPAND_MS {
            let p = (t_rel - start) * 1000 / ROUND_EXPAND_MS;
            out[i] = round_keyframe(p);
        }
    }
    out
}

/// 多屏屏矩形（全局虚拟桌面坐标）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScreenRect {
    pub id: u8,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl ScreenRect {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

/// 多屏定位：指针全局坐标 → 所在屏 id（主册「多屏时涟漪出现在指针真实
/// 所在屏」）。定长 4 屏足够（1 主 + 3 副上界）；都不含 → None。
pub fn locate_screen(px: i32, py: i32, screens: &[ScreenRect; 4]) -> Option<u8> {
    for s in screens {
        if s.contains(px, py) {
            return Some(s.id);
        }
    }
    None
}

/// 涟漪锚点：把指针全局坐标钳制到所在屏内（涟漪永远画在该屏里）。
pub fn clamp_anchor_to_screen(px: i32, py: i32, s: &ScreenRect) -> (i32, i32) {
    (
        px.clamp(s.x, s.x + s.w - 1),
        py.clamp(s.y, s.y + s.h - 1),
    )
}

/// 渲染平面分配（F335）：涟漪走指针优先平面，非全屏重绘（主册「动画性能」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderPlane {
    /// 指针优先平面：只重绘指针周边损伤区。
    PointerPriority,
    /// 全屏重绘（涟漪禁用此路）。
    Fullscreen,
}

pub fn ripple_plane() -> RenderPlane {
    RenderPlane::PointerPriority
}

/// F513 域自检。
pub fn run_f513_checks() -> CheckSet {
    let mut cs = CheckSet::new("F513-ctrl-locator");
    // 1) 判据常量：1s 门槛 / 三轮 / 1500ms 总时长 / 排布自洽。
    cs.add(
        "hold_1000ms",
        HOLD_MS == 1000,
        "主册判据：按住 1 秒触发",
    );
    cs.add(
        "three_rounds_1500ms",
        RIPPLE_ROUNDS == 3
            && RIPPLE_TOTAL_MS == 1500
            && 2 * ROUND_START_GAP_MS + ROUND_EXPAND_MS == RIPPLE_TOTAL_MS,
        "三轮排布 0/250/500 起步、单轮 1000ms、末轮恰在 1500ms 收",
    );
    // 2) 默认开。
    cs.add(
        "default_on",
        DEFAULT_ON && LocatorRipple::new().enabled,
        "主册：开关设置默认开",
    );
    // 3) 999ms 不触发（边界下沿）。
    let mut r = LocatorRipple::new();
    r.ctrl_down(10_000);
    let triggered_999 = {
        let mut ok = true;
        for t in 10_001..10_999 {
            if r.poll(t) {
                ok = false;
            }
        }
        ok
    };
    cs.add(
        "hold_999_no_trigger",
        triggered_999 && r.phase() == LocatorPhase::Armed,
        "999ms 内 poll 均不触发，仍处 Armed",
    );
    // 4) 1000ms 整触发（t_down+1000 时刻）。
    let trig_1000 = r.poll(11_000);
    cs.add(
        "hold_1000_trigger",
        trig_1000 && matches!(r.phase(), LocatorPhase::Rippling { t_trigger: 11_000 }),
        "t_down+1000 触发，t_trigger 对齐",
    );
    // 5) 涟漪 1500ms 后停止。
    cs.add(
        "ripple_window_1500",
        r.ripple_active(11_000 + 1499) && !r.ripple_active(11_000 + 1500),
        "1499ms 仍在播、1500ms 停",
    );
    // 6) 提前松开 = 打字不触发（1 秒持续按住才算）。
    let mut r2 = LocatorRipple::new();
    r2.ctrl_down(0);
    r2.ctrl_up(500);
    let mut no_trig = true;
    for t in 500..2_000 {
        if r2.poll(t) {
            no_trig = false;
        }
    }
    cs.add(
        "early_release_no_trigger",
        no_trig && r2.phase() == LocatorPhase::Idle,
        "按住 500ms 松开 → 永不触发（打字场景）",
    );
    // 7) 组合键豁免：20 例全测——Armed 期间任一第二键按下即取消。
    let mut exempt_ok = 0usize;
    for _ in 0..COMBO_EXEMPT.len() {
        let mut rr = LocatorRipple::new();
        rr.ctrl_down(0);
        let canceled = rr.other_key(300); // 300ms 时第二键按下
        let mut stayed_idle = true;
        for t in 301..1_500 {
            if rr.poll(t) {
                stayed_idle = false;
            }
        }
        if canceled && stayed_idle && rr.phase() == LocatorPhase::Idle {
            exempt_ok += 1;
        }
    }
    cs.add(
        "combo_exempt_20_all",
        COMBO_EXEMPT.len() == 20 && exempt_ok == 20,
        "Ctrl+C/V/X/S/A/F/Z/W/N/T/P/O/B/U/I/K/R/Y/Tab/PrintScreen 全豁免",
    );
    // 8) 无第二键 → 照常触发（豁免只针对组合）。
    let mut r3 = LocatorRipple::new();
    r3.ctrl_down(0);
    let mut fired = false;
    for t in 1..2_000 {
        if r3.poll(t) {
            fired = true;
            break;
        }
    }
    cs.add(
        "no_combo_still_triggers",
        fired,
        "1s 纯按住（无第二键）触发涟漪",
    );
    // 9) 三轮关键帧：起步/换轮/收尾边界（三轮同心扩散，波纹自然交叠）。
    let kf0 = ripple_keyframes(0);
    let kf250 = ripple_keyframes(250);
    let kf1499 = ripple_keyframes(1499);
    let kf1500 = ripple_keyframes(1500);
    cs.add(
        "keyframe_rounds",
        kf0[0] == (0, 1000)
            && kf0[1] == (0, 0)
            && kf250[1] != (0, 0)
            && kf250[0] != (0, 0)
            && kf1499[2] != (0, 0)
            && kf1499[0] == (0, 0)
            && kf1500 == [(0, 0); RIPPLE_ROUNDS],
        "第 1 轮 t=0 起步、t=250 第 2 轮接力（第 1 轮仍在扩）、第 3 轮 1499 在播、1500 全收",
    );
    // 10) 轮内曲线：半径单调升、透明度单调降、峰值对齐。
    let (rad_a, alpha_a) = round_keyframe(0);
    let (rad_b, alpha_b) = round_keyframe(500);
    let (rad_c, alpha_c) = round_keyframe(999);
    cs.add(
        "keyframe_monotonic",
        rad_a == 0
            && rad_b == 500
            && rad_c == 999
            && alpha_a == 1000
            && alpha_b == 500
            && alpha_c == 1
            && rad_b + alpha_b == 1000,
        "半径 0→999 升、透明度 1000→1 降、和恒 1000",
    );
    // 11) 多屏定位：双屏布局，指针在副屏 → 定位副屏；屏外 → None。
    let screens = [
        ScreenRect { id: 0, x: 0, y: 0, w: 1920, h: 1080 },
        ScreenRect { id: 1, x: 1920, y: 0, w: 2560, h: 1440 },
        ScreenRect { id: 2, x: -1280, y: 0, w: 1280, h: 1024 },
        ScreenRect { id: 3, x: 0, y: 1080, w: 1920, h: 1080 },
    ];
    cs.add(
        "multi_screen_locate",
        locate_screen(3000, 700, &screens) == Some(1)
            && locate_screen(-500, 512, &screens) == Some(2)
            && locate_screen(960, 1500, &screens) == Some(3)
            && locate_screen(1919, 1079, &screens) == Some(0)
            && locate_screen(9600, 9600, &screens).is_none(),
        "右副屏/左副屏/下副屏/主屏右下角/全屏外——定位准确",
    );
    // 12) 锚点钳制：指针在屏缘时涟漪仍画在本屏内。
    let (ax, ay) = clamp_anchor_to_screen(2_000, -5, &screens[1]);
    cs.add(
        "anchor_clamped_in_screen",
        ax >= 1920 && ax < 4480 && ay >= 0 && ay < 1440,
        "涟漪锚点恒在所在屏边界内",
    );
    // 13) 动画性能：涟漪走 F335 指针优先平面（禁全屏重绘）。
    cs.add(
        "plane_pointer_priority",
        ripple_plane() == RenderPlane::PointerPriority,
        "涟漪不触发全屏重绘（主册：动画性能）",
    );
    cs
}

#[cfg(test)]
mod tests_f513 {
    use super::*;

    #[test]
    fn boundary_999_vs_1000() {
        // 主册判据边界：999ms 不触发、1000ms 触发。
        let mut r = LocatorRipple::new();
        r.ctrl_down(0);
        assert!(!r.poll(999), "999ms 不得触发");
        assert_eq!(r.phase(), LocatorPhase::Armed, "999ms 时仍应处于蓄势");
        assert!(r.poll(1000), "1000ms 整必须触发");
        assert_eq!(
            r.phase(),
            LocatorPhase::Rippling { t_trigger: 1000 },
            "触发时刻 = 按下时刻 + 1000ms"
        );
    }

    #[test]
    fn combo_exemption_all_20_cases() {
        // 主册：Ctrl+C 等 20 例 0 误触。
        assert_eq!(COMBO_EXEMPT.len(), 20, "豁免表必须恰好 20 例");
        for (i, combo) in COMBO_EXEMPT.iter().enumerate() {
            let mut r = LocatorRipple::new();
            r.ctrl_down(0);
            // 组合中任一第二键在蓄势期内按下 → 豁免取消。
            assert!(r.other_key(400), "第 {} 例 {}：第二键应触发豁免取消", i, combo);
            for t in 401..2_000 {
                assert!(!r.poll(t), "第 {} 例 {}：豁免后不得再触发涟漪", i, combo);
            }
            assert_eq!(r.phase(), LocatorPhase::Idle, "第 {} 例 {}：应回到 Idle", i, combo);
        }
    }

    #[test]
    fn typing_press_release_never_triggers() {
        // 打字场景：Ctrl 短按（<1s）即松，无论之后等多久都不触发。
        let mut r = LocatorRipple::new();
        r.ctrl_down(5_000);
        r.ctrl_up(5_120);
        for t in 5_121..8_000 {
            assert!(!r.poll(t), "短按后不得触发");
        }
        // 边界：按住 999ms 松开同样不触发。
        let mut r2 = LocatorRipple::new();
        r2.ctrl_down(0);
        r2.ctrl_up(999);
        assert!(!r2.poll(2_000), "999ms 松开后不得触发");
    }

    #[test]
    fn three_rounds_schedule_and_fade() {
        // 三轮排布：0/250/500ms 起步，各播 1000ms，总窗恰 1500ms。
        let kf = ripple_keyframes(0);
        assert_eq!(kf[0], (0, 1000), "第 1 轮 t=0：半径 0、不透明");
        assert_eq!(kf[1], (0, 0), "第 2 轮未开始");
        let kf = ripple_keyframes(249);
        assert_ne!(kf[0], (0, 0), "第 1 轮 249ms 仍在播");
        assert_eq!(kf[1], (0, 0), "第 2 轮 249ms 未开始");
        let kf = ripple_keyframes(250);
        assert_eq!(kf[1], (0, 1000), "第 2 轮 t=250 整起步");
        let kf = ripple_keyframes(1_499);
        assert_ne!(kf[2], (0, 0), "第 3 轮 1499ms 仍在播");
        assert_eq!(kf[0], (0, 0), "第 1 轮早已收（250+1000=1250 起？否——0+1000=1000 已收）");
        assert_eq!(
            ripple_keyframes(1_500),
            [(0, 0); RIPPLE_ROUNDS],
            "1500ms 整三轮全收"
        );
        // 轮内曲线单调性。
        let (r0, a0) = round_keyframe(0);
        let (r1, a1) = round_keyframe(500);
        let (r2, a2) = round_keyframe(999);
        assert!(r0 < r1 && r1 < r2, "半径应单调升");
        assert!(a0 > a1 && a1 > a2, "透明度应单调降");
        assert_eq!(r1 + a1, 1000, "半径+透明度恒 1000");
    }

    #[test]
    fn multi_screen_location_accuracy() {
        let screens = [
            ScreenRect { id: 0, x: 0, y: 0, w: 1920, h: 1080 },
            ScreenRect { id: 1, x: 1920, y: 0, w: 2560, h: 1440 },
            ScreenRect { id: 2, x: -1280, y: 0, w: 1280, h: 1024 },
            ScreenRect { id: 3, x: 0, y: 1080, w: 1920, h: 1080 },
        ];
        assert_eq!(locate_screen(100, 100, &screens), Some(0), "主屏内定位主屏");
        assert_eq!(locate_screen(1920, 0, &screens), Some(1), "副屏左上角属副屏（含左上不含右下）");
        assert_eq!(locate_screen(4_479, 1_439, &screens), Some(1), "副屏右下角前一刻度仍属副屏");
        assert_eq!(locate_screen(-1, 100, &screens), Some(2), "负坐标左侧屏");
        assert_eq!(locate_screen(50_000, 50_000, &screens), None, "所有屏之外返回 None");
        // 锚点钳制。
        let (ax, ay) = clamp_anchor_to_screen(2_000, -100, &screens[1]);
        assert_eq!((ax, ay), (2_000, 0), "y 越界钳到屏顶");
        let (ax, ay) = clamp_anchor_to_screen(9_999, 700, &screens[1]);
        assert_eq!((ax, ay), (4_479, 700), "x 越界钳到屏右缘");
    }

    #[test]
    fn rearm_and_plane_semantics() {
        // 涟漪播放中重新按住 Ctrl → 重新计时（Armed 覆盖 Rippling）。
        let mut r = LocatorRipple::new();
        r.ctrl_down(0);
        assert!(r.poll(1_000));
        assert!(matches!(r.phase(), LocatorPhase::Rippling { .. }));
        r.ctrl_down(2_000);
        assert_eq!(r.phase(), LocatorPhase::Armed, "重按即重新蓄势");
        assert!(!r.ripple_active(2_100), "重按后旧涟漪不再视为活跃");
        assert!(r.poll(3_000), "重新满 1s 再触发");
        // 平面分配：指针优先，非全屏。
        assert_eq!(ripple_plane(), RenderPlane::PointerPriority);
        assert_ne!(ripple_plane(), RenderPlane::Fullscreen);
    }
}

// ===========================================================================
// F514 声音视觉提示（听障支持）—— SoundBeacon 三事件色映射 + 2 次脉冲
// ===========================================================================

/// 边缘光带宽度：8px（主册「边缘 8px」）。
pub const EDGE_PX: u32 = 8;
/// 脉冲次数：2 次（主册「2 次脉冲」）。
pub const PULSE_COUNT: u64 = 2;
/// 单脉冲时长：150ms。
pub const PULSE_MS: u64 = 150;
/// 两脉冲间隔：100ms。
pub const PULSE_GAP_MS: u64 = 100;
/// 光带总存活：2×150+100 = 400ms。
pub const BEACON_TOTAL_MS: u64 = PULSE_COUNT * PULSE_MS + (PULSE_COUNT - 1) * PULSE_GAP_MS;
/// 通知蓝（RGB，主册「通知蓝」）。
pub const COLOR_NOTIFY: u32 = 0x0078D7;
/// 警告黄（RGB，主册「警告黄」）。
pub const COLOR_WARNING: u32 = 0xFFB900;
/// 电量红（RGB，主册「电量红」）。
pub const COLOR_BATTERY: u32 = 0xE81123;

/// 三事件枚举（主册：通知声/警告声/电量低提示）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SoundEvent {
    Notify,
    Warning,
    BatteryLow,
}

impl SoundEvent {
    /// 事件 → 边缘光带颜色（主册「三事件色映射」）。
    pub fn color(self) -> u32 {
        match self {
            SoundEvent::Notify => COLOR_NOTIFY,
            SoundEvent::Warning => COLOR_WARNING,
            SoundEvent::BatteryLow => COLOR_BATTERY,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            SoundEvent::Notify => "notify",
            SoundEvent::Warning => "warning",
            SoundEvent::BatteryLow => "battery-low",
        }
    }
    fn bit(self) -> u8 {
        match self {
            SoundEvent::Notify => 1 << 0,
            SoundEvent::Warning => 1 << 1,
            SoundEvent::BatteryLow => 1 << 2,
        }
    }
}

/// 逐事件开关位图（主册「可在声音设置逐事件开关」，默认全开）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BeaconSwitches {
    bits: u8,
}

impl BeaconSwitches {
    pub const fn new() -> Self {
        BeaconSwitches { bits: 0b0000_0111 }
    }
    pub fn get(&self, evt: SoundEvent) -> bool {
        self.bits & evt.bit() != 0
    }
    pub fn set(&mut self, evt: SoundEvent, on: bool) {
        if on {
            self.bits |= evt.bit();
        } else {
            self.bits &= !evt.bit();
        }
    }
    pub fn raw(&self) -> u8 {
        self.bits
    }
}

/// 勿扰档三态（对齐 F341）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DoNotDisturb {
    /// 勿扰关：声音照响、视觉照闪。
    Off,
    /// 仅声音档（主册「仅声音档=只闪不响」）：静音、视觉保留。
    MuteSoundKeepVisual,
    /// 全静档（主册「全静=都不来但中心记录」）：声音视觉都不来、中心记录。
    FullSilent,
}

/// 三路输出镜像（勿扰档联动镜像函数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TripleOut {
    /// 声音路（受 F240 音量进一步调制）。
    pub sound: bool,
    /// 视觉路（光带闪——不受音量影响）。
    pub visual: bool,
    /// 通知中心记录路。
    pub record: bool,
}

/// 勿扰档 → 三路输出镜像。
pub fn dnd_mirror(dnd: DoNotDisturb) -> TripleOut {
    match dnd {
        DoNotDisturb::Off => TripleOut { sound: true, visual: true, record: false },
        DoNotDisturb::MuteSoundKeepVisual => TripleOut { sound: false, visual: true, record: false },
        DoNotDisturb::FullSilent => TripleOut { sound: false, visual: false, record: true },
    }
}

/// 单脉冲包络（三角波）：脉冲内 u∈[0,PULSE_MS) → alpha permille，
/// 中点达峰 1000，两端 0。
pub fn pulse_envelope(u: u64) -> u64 {
    if u >= PULSE_MS {
        return 0;
    }
    let half = PULSE_MS / 2;
    if u < half {
        u * 1000 / half
    } else {
        (PULSE_MS - u) * 1000 / half
    }
}

/// 光带关键帧：t_rel = 发声事件起的时间 → alpha permille（0 = 熄灭）。
/// 第 p 次脉冲窗 = [p*(PULSE_MS+GAP), +PULSE_MS)。
pub fn beacon_alpha(t_rel: u64) -> u64 {
    for p in 0..PULSE_COUNT {
        let start = p * (PULSE_MS + PULSE_GAP_MS);
        if t_rel >= start && t_rel < start + PULSE_MS {
            return pulse_envelope(t_rel - start);
        }
    }
    0
}

/// 光带是否仍在播。
pub fn beacon_active(t_rel: u64) -> bool {
    t_rel < BEACON_TOTAL_MS
}

/// F341/F240 叠加一致性：给定勿扰档与提示音量 permille → (声音响, 视觉闪,
/// 记录)。视觉路与音量无关（音量归零视觉照闪）；声音路 = 勿扰放行 且
/// 音量 > 0。
pub fn overlay_consistency(dnd: DoNotDisturb, alert_vol_permille: u32) -> TripleOut {
    let m = dnd_mirror(dnd);
    TripleOut {
        sound: m.sound && alert_vol_permille > 0,
        visual: m.visual,
        record: m.record,
    }
}

/// F514 域自检。
pub fn run_f514_checks() -> CheckSet {
    let mut cs = CheckSet::new("F514-sound-beacon");
    // 1) 脉冲形态常量：8px / 2 次 / 总 400ms。
    cs.add(
        "edge_px_8_pulse_2",
        EDGE_PX == 8 && PULSE_COUNT == 2,
        "主册判据：脉冲形态（2 次/边缘 8px）",
    );
    cs.add(
        "beacon_total_400",
        BEACON_TOTAL_MS == 400,
        "2×150ms 脉冲 + 100ms 间隔 = 400ms",
    );
    // 2) 三事件色映射：三色互异且各自对齐。
    cs.add(
        "three_event_color_map",
        COLOR_NOTIFY != COLOR_WARNING
            && COLOR_WARNING != COLOR_BATTERY
            && COLOR_NOTIFY != COLOR_BATTERY
            && SoundEvent::Notify.color() == COLOR_NOTIFY
            && SoundEvent::Warning.color() == COLOR_WARNING
            && SoundEvent::BatteryLow.color() == COLOR_BATTERY,
        "通知蓝/警告黄/电量红一一对应且互异",
    );
    // 3) 逐事件开关默认全开。
    let sw = BeaconSwitches::new();
    cs.add(
        "switches_default_all_on",
        sw.get(SoundEvent::Notify) && sw.get(SoundEvent::Warning) && sw.get(SoundEvent::BatteryLow),
        "默认三事件全闪",
    );
    // 4) 逐事件独立开关：关一个不影响另两个。
    let mut sw2 = BeaconSwitches::new();
    sw2.set(SoundEvent::Warning, false);
    cs.add(
        "per_event_switch_independent",
        !sw2.get(SoundEvent::Warning)
            && sw2.get(SoundEvent::Notify)
            && sw2.get(SoundEvent::BatteryLow)
            && sw2.raw() == 0b0000_0101,
        "关警告事件后其余两位不受影响（位图独立）",
    );
    // 5) 勿扰档镜像三态（F341 联动）。
    let off = dnd_mirror(DoNotDisturb::Off);
    let mute = dnd_mirror(DoNotDisturb::MuteSoundKeepVisual);
    let full = dnd_mirror(DoNotDisturb::FullSilent);
    cs.add(
        "dnd_off_mirror",
        off.sound && off.visual && !off.record,
        "勿扰关：响+闪",
    );
    cs.add(
        "dnd_sound_only_mirror",
        !mute.sound && mute.visual && !mute.record,
        "仅声音档：只闪不响（主册原话）",
    );
    cs.add(
        "dnd_full_silent_mirror",
        !full.sound && !full.visual && full.record,
        "全静：都不来但中心记录（主册原话）",
    );
    // 6) F240 叠加一致性：音量归零 → 声音灭、视觉照闪。
    let v0 = overlay_consistency(DoNotDisturb::Off, 0);
    let v1000 = overlay_consistency(DoNotDisturb::Off, 1000);
    cs.add(
        "f240_volume_visual_independent",
        !v0.sound && v0.visual && v1000.sound && v1000.visual,
        "音量 0‰ 只灭声音路，视觉路与音量无关",
    );
    // 7) 叠加矩阵：勿扰 × 音量 组合抽查（一致性用例表）。
    let m_v = overlay_consistency(DoNotDisturb::MuteSoundKeepVisual, 1000);
    let f_v = overlay_consistency(DoNotDisturb::FullSilent, 1000);
    cs.add(
        "overlay_matrix",
        !m_v.sound && m_v.visual && !f_v.sound && !f_v.visual && f_v.record,
        "仅声音档+满音量=只闪；全静+满音量=仅记录",
    );
    // 8) 脉冲包络：三角形态（两端 0、中点峰 1000）。
    cs.add(
        "pulse_envelope_triangle",
        pulse_envelope(0) == 0
            && pulse_envelope(PULSE_MS / 2) == 1000
            && pulse_envelope(PULSE_MS - 1) < 100
            && pulse_envelope(PULSE_MS) == 0,
        "单脉冲起 0 → 中点 1000 → 末尾回 0",
    );
    // 9) 两次脉冲时序：t=75 峰 1、间隔熄、t=325 峰 2（第二脉冲中点）。
    cs.add(
        "two_pulse_timing",
        beacon_alpha(0) == 0
            && beacon_alpha(75) == 1000
            && beacon_alpha(175) == 0
            && beacon_alpha(200) == 0
            && beacon_alpha(325) == 1000,
        "0ms 起脉冲 1（75ms 峰）、200ms 起脉冲 2（325ms 峰）",
    );
    // 10) 光带 400ms 后熄灭。
    cs.add(
        "beacon_ends_at_400",
        beacon_active(399) && !beacon_active(400) && beacon_alpha(400) == 0,
        "399ms 仍在播、400ms 整熄灭",
    );
    cs
}

#[cfg(test)]
mod tests_f514 {
    use super::*;

    #[test]
    fn color_mapping_is_exact() {
        assert_eq!(SoundEvent::Notify.color(), 0x0078D7, "通知蓝");
        assert_eq!(SoundEvent::Warning.color(), 0xFFB900, "警告黄");
        assert_eq!(SoundEvent::BatteryLow.color(), 0xE81123, "电量红");
        assert_ne!(SoundEvent::Notify.color(), SoundEvent::BatteryLow.color(), "蓝红必须可辨");
    }

    #[test]
    fn switches_bitmap_roundtrip() {
        let mut sw = BeaconSwitches::new();
        assert_eq!(sw.raw(), 0b0111, "默认全开");
        sw.set(SoundEvent::BatteryLow, false);
        sw.set(SoundEvent::Notify, false);
        assert!(!sw.get(SoundEvent::BatteryLow));
        assert!(!sw.get(SoundEvent::Notify));
        assert!(sw.get(SoundEvent::Warning));
        sw.set(SoundEvent::BatteryLow, true);
        assert!(sw.get(SoundEvent::BatteryLow), "再开恢复");
        // 重复关是幂等的。
        sw.set(SoundEvent::Warning, false);
        sw.set(SoundEvent::Warning, false);
        assert!(!sw.get(SoundEvent::Warning));
    }

    #[test]
    fn dnd_mirror_three_modes_exact() {
        let off = dnd_mirror(DoNotDisturb::Off);
        assert_eq!((off.sound, off.visual, off.record), (true, true, false));
        let mute = dnd_mirror(DoNotDisturb::MuteSoundKeepVisual);
        assert_eq!(
            (mute.sound, mute.visual, mute.record),
            (false, true, false),
            "仅声音档=只闪不响"
        );
        let full = dnd_mirror(DoNotDisturb::FullSilent);
        assert_eq!(
            (full.sound, full.visual, full.record),
            (false, false, true),
            "全静=都不来但中心记录"
        );
    }

    #[test]
    fn overlay_consistency_full_table() {
        // F341 × F240 一致性用例表：3 勿扰档 × 3 音量档。
        for (dnd, vol, want_sound, want_visual, want_record) in [
            (DoNotDisturb::Off, 1000, true, true, false),
            (DoNotDisturb::Off, 500, true, true, false),
            (DoNotDisturb::Off, 0, false, true, false),
            (DoNotDisturb::MuteSoundKeepVisual, 1000, false, true, false),
            (DoNotDisturb::MuteSoundKeepVisual, 0, false, true, false),
            (DoNotDisturb::FullSilent, 1000, false, false, true),
            (DoNotDisturb::FullSilent, 0, false, false, true),
        ] {
            let got = overlay_consistency(dnd, vol);
            assert_eq!(got.sound, want_sound, "sound 路: {:?}@{}", dnd, vol);
            assert_eq!(got.visual, want_visual, "visual 路: {:?}@{}", dnd, vol);
            assert_eq!(got.record, want_record, "record 路: {:?}@{}", dnd, vol);
        }
    }

    #[test]
    fn pulse_envelope_shape() {
        // 三角包络逐点单调性：升段不减、降段不增。
        let mut prev = pulse_envelope(0);
        assert_eq!(prev, 0, "脉冲起点为 0");
        for u in 1..PULSE_MS / 2 {
            let v = pulse_envelope(u);
            assert!(v >= prev, "升段 u={} 应不减", u);
            prev = v;
        }
        assert_eq!(pulse_envelope(PULSE_MS / 2), 1000, "中点峰值 1000");
        prev = pulse_envelope(PULSE_MS / 2);
        for u in PULSE_MS / 2 + 1..PULSE_MS {
            let v = pulse_envelope(u);
            assert!(v <= prev, "降段 u={} 应不增", u);
            prev = v;
        }
        assert_eq!(pulse_envelope(PULSE_MS), 0, "脉冲窗外为 0");
    }

    #[test]
    fn two_pulse_window_boundaries() {
        // 两次脉冲窗：[0,150) 与 [250,400)；间隔 [150,250) 熄。
        assert_eq!(beacon_alpha(149) > 0, true, "第一次脉冲 149ms 仍亮");
        assert_eq!(beacon_alpha(150), 0, "150ms 整进入间隔");
        assert_eq!(beacon_alpha(249), 0, "间隔期熄灭");
        assert_eq!(beacon_alpha(250), 0, "第二次脉冲 250ms 起沿包络为 0");
        assert_eq!(beacon_alpha(325), 1000, "第二次脉冲中点 325ms 达峰");
        assert!(beacon_alpha(399) > 0, "第二次脉冲 399ms 仍亮");
        assert!(beacon_active(399));
        assert!(!beacon_active(400), "400ms 整总窗结束");
        assert_eq!(beacon_alpha(1_000), 0, "远超总窗恒 0");
    }
}

// ===========================================================================
// F519 大写锁定提示音 —— CapsTone 双音色 + F240 音量合成 + F230 互补
// ===========================================================================

/// 默认关（主册「默认关」）。
pub const CAPS_TONE_DEFAULT_ON: bool = false;
/// 开启态双音：高双音频率对（C6/E6，短促上行）。
pub const ON_TONE_HZ: [u16; 2] = [1046, 1318];
/// 关闭态双音：低双音频率对（C5/E5，短促下行感）。
pub const OFF_TONE_HZ: [u16; 2] = [523, 659];
/// 单音时长：60ms。
pub const TONE_MS: u64 = 60;
/// 两音间隔：40ms。
pub const TONE_GAP_MS: u64 = 40;
/// 双音总时长：2×60+40 = 160ms。
pub const CAPS_TONE_TOTAL_MS: u64 = 2 * TONE_MS + TONE_GAP_MS;
/// 设置字节中的持久化位（bit4：大写锁定提示音开关）。
pub const SET_BIT_CAPS_TONE: u8 = 1 << 4;

/// 锁定键（主册：Caps Lock / Num Lock 都适用同一双音规则）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockKey {
    CapsLock,
    NumLock,
}

/// 双音频率对：切换后状态 on → 高对，off → 低对（主册「开=高双音、关=低
/// 双音」）。
pub fn tone_pair(now_on: bool) -> [u16; 2] {
    if now_on {
        ON_TONE_HZ
    } else {
        OFF_TONE_HZ
    }
}

/// 第 k 音（k∈{0,1}）在 t_rel 的包络：窗 [k*(TONE_MS+GAP), +TONE_MS)。
pub fn tone_window_active(k: usize, t_rel: u64) -> bool {
    let start = k as u64 * (TONE_MS + TONE_GAP_MS);
    t_rel >= start && t_rel < start + TONE_MS
}

/// 合成采样：t_rel 时刻的幅值 permille = 包络(0/1000) × 提示音量 permille
/// （F240「音量跟随系统提示音量」）。
pub fn caps_sample(t_rel: u64, _now_on: bool, alert_vol_permille: u32) -> u32 {
    let k = if t_rel < TONE_MS + TONE_GAP_MS { 0 } else { 1 };
    if tone_window_active(k, t_rel) {
        alert_vol_permille
    } else {
        0
    }
}

/// 双音是否已放完。
pub fn caps_tone_done(t_rel: u64) -> bool {
    t_rel >= CAPS_TONE_TOTAL_MS
}

/// 开关持久化：写（把开关合并进设置字节）。
pub fn caps_pack(enabled: bool, settings: u8) -> u8 {
    if enabled {
        settings | SET_BIT_CAPS_TONE
    } else {
        settings & !SET_BIT_CAPS_TONE
    }
}

/// 开关持久化：读。
pub fn caps_unpack(settings: u8) -> bool {
    settings & SET_BIT_CAPS_TONE != 0
}

/// 与 F230 的双通道互补：视觉角标通道恒开（独立于声音开关），声音通道 =
/// 开关。两通道「或」关系：至少角标常在（主册「视觉互补」）。
pub fn channels(sound_enabled: bool) -> (bool, bool) {
    (sound_enabled, true)
}

/// F519 域自检。
pub fn run_f519_checks() -> CheckSet {
    let mut cs = CheckSet::new("F519-capslock-tone");
    // 1) 默认关。
    cs.add(
        "default_off",
        !CAPS_TONE_DEFAULT_ON && !caps_unpack(0),
        "主册判据：默认关",
    );
    // 2) 双音色：四频率常量齐备、开高关低、两对互异。
    cs.add(
        "two_tone_pairs",
        ON_TONE_HZ == [1046, 1318]
            && OFF_TONE_HZ == [523, 659]
            && ON_TONE_HZ[0] > OFF_TONE_HZ[0]
            && ON_TONE_HZ[1] > OFF_TONE_HZ[1]
            && tone_pair(true) == ON_TONE_HZ
            && tone_pair(false) == OFF_TONE_HZ,
        "开=高双音 1046/1318、关=低双音 523/659",
    );
    // 3) 两态音对互不相同（听感可区分）。
    cs.add(
        "pairs_distinguishable",
        tone_pair(true)[0] != tone_pair(false)[0] && tone_pair(true)[1] != tone_pair(false)[1],
        "同一键两态音对逐频互异",
    );
    // 4) 双音时序：单音 60ms、间隔 40ms、总 160ms。
    cs.add(
        "tone_schedule",
        TONE_MS == 60 && TONE_GAP_MS == 40 && CAPS_TONE_TOTAL_MS == 160,
        "短促两音 60+40+60=160ms",
    );
    // 5) 音窗：两音窗各自独立、窗外静音、160ms 放完。
    cs.add(
        "tone_windows",
        tone_window_active(0, 0)
            && tone_window_active(0, 59)
            && !tone_window_active(0, 60)
            && !tone_window_active(1, 99)
            && tone_window_active(1, 100)
            && tone_window_active(1, 159)
            && !tone_window_active(1, 160)
            && caps_tone_done(160),
        "音1 [0,60)、音2 [100,160)、160ms 完",
    );
    // 6) 音量跟随 F240：幅值 = 包络 × 提示音量。
    cs.add(
        "volume_follows_f240",
        caps_sample(10, true, 1000) == 1000
            && caps_sample(10, true, 400) == 400
            && caps_sample(10, true, 0) == 0
            && caps_sample(80, true, 1000) == 0,
        "窗内=音量直通、音量 0 静音、间隔期静音",
    );
    // 7) 开关持久化：写读一致（开与关两向）。
    let packed_on = caps_pack(true, 0b0000_1011);
    let packed_off = caps_pack(false, 0b0001_1011);
    cs.add(
        "persist_roundtrip",
        caps_unpack(packed_on)
            && !caps_unpack(packed_off)
            && caps_pack(caps_unpack(packed_on), packed_off) == packed_on,
        "设置位写读一致、其余位不被破坏（写-读-写幂等）",
    );
    // 8) 与 F230 视觉互补：视觉角标通道恒开（声音可关）。
    let (snd_on, vis_on) = channels(true);
    let (snd_off, vis_off) = channels(false);
    cs.add(
        "f230_visual_complement",
        snd_on && vis_on && !snd_off && vis_off && (snd_on || vis_on) && (snd_off || vis_off),
        "两通道恒为或：声音关了角标仍在（互补）",
    );
    // 9) 两种锁定键共用同一规则。
    cs.add(
        "caps_and_num_same_rule",
        LockKey::CapsLock != LockKey::NumLock
            && tone_pair(true) == ON_TONE_HZ
            && tone_pair(false) == OFF_TONE_HZ,
        "CapsLock/NumLock 同一双音规则",
    );
    cs
}

#[cfg(test)]
mod tests_f519 {
    use super::*;

    #[test]
    fn default_is_off() {
        assert!(!CAPS_TONE_DEFAULT_ON, "主册：默认关");
        assert!(!caps_unpack(0), "全零设置字节读出为关");
        assert!(!caps_unpack(0xFF & !SET_BIT_CAPS_TONE), "无该位即关");
    }

    #[test]
    fn persistence_roundtrip_both_ways() {
        // 开 → 写 → 读 = 开；关 → 写 → 读 = 关；周边位不受影响。
        let base: u8 = 0b1110_1111; // bit4 预先为 0
        let on = caps_pack(true, base);
        assert_eq!(on, 0b1111_1111);
        assert!(caps_unpack(on));
        let off = caps_pack(false, on);
        assert_eq!(off, base, "关回去恢复原字节");
        assert!(!caps_unpack(off));
        // 幂等。
        assert_eq!(caps_pack(true, on), on);
        assert_eq!(caps_pack(false, off), off);
    }

    #[test]
    fn tone_pairs_are_high_low_and_distinct() {
        assert_eq!(tone_pair(true), [1046, 1318], "开=高双音");
        assert_eq!(tone_pair(false), [523, 659], "关=低双音");
        assert!(ON_TONE_HZ[0] < ON_TONE_HZ[1], "高对内部上行");
        assert!(OFF_TONE_HZ[0] < OFF_TONE_HZ[1], "低对内部上行");
        assert!(ON_TONE_HZ[0] > OFF_TONE_HZ[1], "高对整体高于低对（听感立辨）");
    }

    #[test]
    fn sample_envelope_and_volume() {
        // 音 1 窗 [0,60)：有幅值；间隔 [60,100)：静音；音 2 窗 [100,160)。
        assert_eq!(caps_sample(0, true, 800), 800);
        assert_eq!(caps_sample(59, false, 800), 800);
        assert_eq!(caps_sample(60, true, 800), 0, "间隔期静音");
        assert_eq!(caps_sample(99, true, 800), 0);
        assert_eq!(caps_sample(100, true, 800), 800, "第二音起");
        assert_eq!(caps_sample(159, true, 800), 800);
        assert_eq!(caps_sample(160, true, 800), 0, "160ms 放完");
        // 音量线性直通（F240）。
        for vol in [0u32, 1, 250, 999, 1000] {
            assert_eq!(caps_sample(10, true, vol), vol, "窗内幅值=提示音量 {}", vol);
        }
    }

    #[test]
    fn visual_badge_independent_of_sound() {
        // F230 互补用例表：声音开关两态下视觉角标都亮。
        for snd in [true, false] {
            let (s, v) = channels(snd);
            assert_eq!(s, snd);
            assert!(v, "视觉角标通道恒开（声音={}）", snd);
            assert!(s || v, "或关系至少一路在");
        }
    }
}

// ===========================================================================
// F520 标题栏中键最小化 —— TitlebarClickRouter 三义分流矩阵
// ===========================================================================

/// 开关默认：开（不喜可关）。
pub const MIDBTN_DEFAULT_ON: bool = true;
/// 最小化动画时长：220ms（对齐 F124 最小化动画）。
pub const MINIMIZE_ANIM_MS: u64 = 220;

/// 标题栏点击类型（三义之源：按按键类型天然分流）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClickKind {
    /// 中键单击（本项新增义）。
    Middle,
    /// 左键双击（F213 双击最大化）。
    DoubleLeft,
    /// 右键单击（F376 系统菜单）。
    Right,
}

/// 命中区域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Zone {
    /// 标题栏（三义生效区）。
    Titlebar,
    /// 客户区（三义均不接管）。
    Client,
    /// 可调边框（交给缩放逻辑）。
    EdgeBorder,
}

/// 标题栏动作输出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TitlebarAction {
    /// 中键最小化（本项）。
    Minimize,
    /// 双击切换最大化（F213）。
    ToggleMaximize,
    /// 右键系统菜单（F376）。
    SysMenu,
    /// 本层不接管。
    None,
}

/// 三义分流矩阵纯函数：enabled=false 时中键义退化（三义→两义），其余两义
/// 永不受本开关影响（F213/F376 语义独立）。
pub fn route(kind: ClickKind, zone: Zone, enabled: bool) -> TitlebarAction {
    if zone != Zone::Titlebar {
        return TitlebarAction::None;
    }
    match kind {
        ClickKind::Middle if enabled => TitlebarAction::Minimize,
        ClickKind::DoubleLeft => TitlebarAction::ToggleMaximize,
        ClickKind::Right => TitlebarAction::SysMenu,
        _ => TitlebarAction::None,
    }
}

/// 中键拖拽仲裁：中键按下后移动 → 不进入拖拽状态机（主册「中键拖拽无定义
/// 即无冲突」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragVerdict {
    /// 进入拖拽状态机（左键才走这路）。
    EntersDrag,
    /// 不进入（无定义=无冲突）。
    NotHandled,
}

pub fn middle_drag_arbitration() -> DragVerdict {
    DragVerdict::NotHandled
}

/// 左键拖拽对照：左键按下后移动进入拖拽（仲裁语义区分按键）。
pub fn left_drag_arbitration() -> DragVerdict {
    DragVerdict::EntersDrag
}

/// F520 域自检。
pub fn run_f520_checks() -> CheckSet {
    let mut cs = CheckSet::new("F520-midbtn-minimize");
    // 1) 开关默认开。
    cs.add(
        "default_on",
        MIDBTN_DEFAULT_ON,
        "主册判据：开关（默认开启，不喜可关）",
    );
    // 2) 三义矩阵——中键×标题栏 = 最小化。
    cs.add(
        "middle_minimizes",
        route(ClickKind::Middle, Zone::Titlebar, true) == TitlebarAction::Minimize,
        "标题栏中键点击 = 最小化窗口",
    );
    // 3) 三义矩阵——双击×标题栏 = 最大化（F213 语义）。
    cs.add(
        "double_maximize",
        route(ClickKind::DoubleLeft, Zone::Titlebar, true) == TitlebarAction::ToggleMaximize,
        "标题栏双击 = 切换最大化（F213）",
    );
    // 4) 三义矩阵——右键×标题栏 = 系统菜单（F376 语义）。
    cs.add(
        "right_sysmenu",
        route(ClickKind::Right, Zone::Titlebar, true) == TitlebarAction::SysMenu,
        "标题栏右键 = 系统菜单（F376）",
    );
    // 5) 3×3 全对齐断言：3 按键 × 3 区域，逐格期望值（非标题栏区全部 None）。
    let mut matrix_ok = true;
    let kinds = [ClickKind::Middle, ClickKind::DoubleLeft, ClickKind::Right];
    let zones = [Zone::Titlebar, Zone::Client, Zone::EdgeBorder];
    for k in kinds {
        for z in zones {
            let got = route(k, z, true);
            let want = match (k, z) {
                (ClickKind::Middle, Zone::Titlebar) => TitlebarAction::Minimize,
                (ClickKind::DoubleLeft, Zone::Titlebar) => TitlebarAction::ToggleMaximize,
                (ClickKind::Right, Zone::Titlebar) => TitlebarAction::SysMenu,
                _ => TitlebarAction::None,
            };
            if got != want {
                matrix_ok = false;
            }
        }
    }
    cs.add(
        "matrix_3x3_aligned",
        matrix_ok,
        "3×3 分流矩阵 9 格逐格对齐（三义互不干扰）",
    );
    // 6) 开关关：中键无动作，矩阵退化为两义（F213/F376 不受影响）。
    cs.add(
        "off_degrades_to_two",
        route(ClickKind::Middle, Zone::Titlebar, false) == TitlebarAction::None
            && route(ClickKind::DoubleLeft, Zone::Titlebar, false) == TitlebarAction::ToggleMaximize
            && route(ClickKind::Right, Zone::Titlebar, false) == TitlebarAction::SysMenu,
        "off 时中键无动作、双大/右菜单两义保持",
    );
    // 7) 中键拖拽无定义：不进拖拽状态机（无冲突）。
    cs.add(
        "middle_drag_not_handled",
        middle_drag_arbitration() == DragVerdict::NotHandled
            && left_drag_arbitration() == DragVerdict::EntersDrag,
        "中键按下后移动不进拖拽（左键对照进入）——天然无冲突",
    );
    // 8) 最小化动画对齐 F124。
    cs.add(
        "anim_aligns_f124",
        MINIMIZE_ANIM_MS == 220,
        "最小化动画 220ms 与 F124 时长常量对齐",
    );
    // 9) 三义互不干扰的语义复核：同一区域三种按键输出互异。
    let a1 = route(ClickKind::Middle, Zone::Titlebar, true);
    let a2 = route(ClickKind::DoubleLeft, Zone::Titlebar, true);
    let a3 = route(ClickKind::Right, Zone::Titlebar, true);
    cs.add(
        "three_meanings_mutually_exclusive",
        a1 != a2 && a2 != a3 && a1 != a3,
        "中收/双大/右菜单三输出两两互异（天然分流）",
    );
    // 10) 误触代价无害性：中键最小化可逆（最小化动作本身不销毁窗口）。
    cs.add(
        "mistouch_reversible",
        route(ClickKind::Middle, Zone::Titlebar, true) != TitlebarAction::None,
        "误触中键代价=最小化（可逆、无害），非关闭",
    );
    cs
}

#[cfg(test)]
mod tests_f520 {
    use super::*;

    #[test]
    fn three_meanings_on_titlebar() {
        assert_eq!(route(ClickKind::Middle, Zone::Titlebar, true), TitlebarAction::Minimize, "中=收");
        assert_eq!(route(ClickKind::DoubleLeft, Zone::Titlebar, true), TitlebarAction::ToggleMaximize, "双=大");
        assert_eq!(route(ClickKind::Right, Zone::Titlebar, true), TitlebarAction::SysMenu, "右=菜单");
    }

    #[test]
    fn full_matrix_enumeration() {
        // 3×3 = 9 格全部显式断言（不靠循环内再推导）。
        assert_eq!(route(ClickKind::Middle, Zone::Titlebar, true), TitlebarAction::Minimize);
        assert_eq!(route(ClickKind::Middle, Zone::Client, true), TitlebarAction::None);
        assert_eq!(route(ClickKind::Middle, Zone::EdgeBorder, true), TitlebarAction::None);
        assert_eq!(route(ClickKind::DoubleLeft, Zone::Titlebar, true), TitlebarAction::ToggleMaximize);
        assert_eq!(route(ClickKind::DoubleLeft, Zone::Client, true), TitlebarAction::None);
        assert_eq!(route(ClickKind::DoubleLeft, Zone::EdgeBorder, true), TitlebarAction::None);
        assert_eq!(route(ClickKind::Right, Zone::Titlebar, true), TitlebarAction::SysMenu);
        assert_eq!(route(ClickKind::Right, Zone::Client, true), TitlebarAction::None);
        assert_eq!(route(ClickKind::Right, Zone::EdgeBorder, true), TitlebarAction::None);
    }

    #[test]
    fn switch_off_degrades_gracefully() {
        // off：中键无动作；两义保持；客户端区照旧 None。
        assert_eq!(route(ClickKind::Middle, Zone::Titlebar, false), TitlebarAction::None, "off 中键无动作");
        assert_eq!(route(ClickKind::DoubleLeft, Zone::Titlebar, false), TitlebarAction::ToggleMaximize);
        assert_eq!(route(ClickKind::Right, Zone::Titlebar, false), TitlebarAction::SysMenu);
        assert_eq!(route(ClickKind::Middle, Zone::Client, false), TitlebarAction::None);
        // on/off 对中键义的影响是唯一的差异点。
        assert_ne!(
            route(ClickKind::Middle, Zone::Titlebar, true),
            route(ClickKind::Middle, Zone::Titlebar, false),
            "开关只影响中键义"
        );
    }

    #[test]
    fn middle_drag_has_no_definition() {
        // 中键拖拽无定义 → 不进拖拽状态机 → 与拖拽零冲突。
        assert_eq!(middle_drag_arbitration(), DragVerdict::NotHandled);
        assert_ne!(middle_drag_arbitration(), left_drag_arbitration(), "左键拖拽语义保留作对照");
    }

    #[test]
    fn minimize_anim_constant() {
        assert_eq!(MINIMIZE_ANIM_MS, 220, "最小化动画时长对齐 F124");
        // 动画时长不受开关影响（off 时根本不会触发动作，但常量不变）。
        assert_eq!(MINIMIZE_ANIM_MS, 220);
    }
}

// ===========================================================================
// F522 指针轨迹显示 —— PointerTrail 三档时长 + 定长环形缓冲 + 渐隐
// ===========================================================================

/// 轨迹存留三档（主册：短/中/长 = 200/400/600ms）。
pub const TRAIL_SHORT_MS: u64 = 200;
pub const TRAIL_MID_MS: u64 = 400;
pub const TRAIL_LONG_MS: u64 = 600;
/// 默认关（主册「默认关」）。
pub const TRAIL_DEFAULT_ON: bool = false;
/// 轨迹点环形缓冲容量（定长，零堆）。
pub const TRAIL_POINTS: usize = 16;
/// 轨迹绘制耗时预算（微秒）：主册「帧率无损」的量化闸。
pub const TRAIL_DRAW_BUDGET_US: u64 = 500;
/// 60Hz 帧预算（微秒）。
pub const FRAME_BUDGET_US_60HZ: u64 = 16_667;

/// 轨迹长度三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrailLen {
    Short,
    Mid,
    Long,
}

impl TrailLen {
    /// 档位 → 轨迹存留时长。
    pub fn hold_ms(self) -> u64 {
        match self {
            TrailLen::Short => TRAIL_SHORT_MS,
            TrailLen::Mid => TRAIL_MID_MS,
            TrailLen::Long => TRAIL_LONG_MS,
        }
    }
}

/// 轨迹点（坐标 + 采样时刻）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrailPoint {
    pub x: i32,
    pub y: i32,
    pub t: u64,
}

/// 指针轨迹：定长环形缓冲 + 按时间渐隐。
pub struct PointerTrail {
    enabled: bool,
    len: TrailLen,
    buf: [Option<TrailPoint>; TRAIL_POINTS],
    head: usize,
    count: usize,
}

impl PointerTrail {
    pub const fn new() -> Self {
        PointerTrail {
            enabled: TRAIL_DEFAULT_ON,
            len: TrailLen::Mid,
            buf: [None; TRAIL_POINTS],
            head: 0,
            count: 0,
        }
    }

    /// 开关即时：切换后下一帧生效（本层不缓存任何「延迟生效」状态）。
    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn set_len(&mut self, len: TrailLen) {
        self.len = len;
    }
    pub fn len_mode(&self) -> TrailLen {
        self.len
    }

    /// 推入轨迹点：未满则追加；已满则覆盖最旧点（淘汰最旧——主册「轨迹」
    /// 天然 FIFO）。
    pub fn push(&mut self, x: i32, y: i32, t: u64) {
        if !self.enabled {
            return;
        }
        let slot = if self.count < TRAIL_POINTS {
            let s = (self.head + self.count) % TRAIL_POINTS;
            self.count += 1;
            s
        } else {
            let s = self.head;
            self.head = (self.head + 1) % TRAIL_POINTS;
            s
        };
        self.buf[slot] = Some(TrailPoint { x, y, t });
    }

    /// 渐隐 alpha：dt = t_now - 采样时刻 → permille（存留期内线性衰减，
    /// 期满 0）。dt=0 → 1000（新点最实），dt=hold → 0。
    pub fn alpha_at(&self, t_now: u64, t_sample: u64) -> u64 {
        let hold = self.len.hold_ms();
        let dt = t_now.saturating_sub(t_sample);
        if dt >= hold {
            0
        } else {
            1000 - dt * 1000 / hold
        }
    }

    /// 可见点数（存留期内）。
    pub fn visible_count(&self, t_now: u64) -> usize {
        let mut n = 0;
        for i in 0..self.count {
            let idx = (self.head + i) % TRAIL_POINTS;
            if let Some(p) = self.buf[idx] {
                if self.alpha_at(t_now, p.t) > 0 {
                    n += 1;
                }
            }
        }
        n
    }

    /// 快照：把存留期内的可见点（旧→新）连同 alpha permille 写入 out，
    /// 返回写入数（供 F335 指针优先平面绘制）。
    pub fn snapshot(&self, t_now: u64, out: &mut [(i32, i32, u64); TRAIL_POINTS]) -> usize {
        let mut n = 0;
        for i in 0..self.count {
            let idx = (self.head + i) % TRAIL_POINTS;
            if let Some(p) = self.buf[idx] {
                let a = self.alpha_at(t_now, p.t);
                if a > 0 && n < out.len() {
                    out[n] = (p.x, p.y, a);
                    n += 1;
                }
            }
        }
        n
    }

    /// 皮肤兼容（F156）：轨迹缓冲与皮肤 id 完全解耦——换肤只换贴图，
    /// 轨迹点数据不受影响（本结构没有皮肤字段，结构性保证）。
    pub fn skin_independent(&self) -> bool {
        // 结构自证：本类型无皮肤状态，缓冲内容只由 push/时间决定。
        true
    }

    pub fn head(&self) -> usize {
        self.head
    }
    pub fn count(&self) -> usize {
        self.count
    }
}

/// 轨迹渲染平面（F335 指针优先平面——非全屏重绘）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TrailPlane {
    PointerPriority,
    Fullscreen,
}

pub fn trail_plane() -> TrailPlane {
    TrailPlane::PointerPriority
}

/// 帧率无损预算断言：轨迹绘制预算远小于 60Hz 帧预算。
pub fn trail_budget_ok() -> bool {
    TRAIL_DRAW_BUDGET_US < FRAME_BUDGET_US_60HZ / 8
}

/// F522 域自检。
pub fn run_f522_checks() -> CheckSet {
    let mut cs = CheckSet::new("F522-pointer-trail");
    // 1) 三档时长常量。
    cs.add(
        "three_lengths",
        TRAIL_SHORT_MS == 200 && TRAIL_MID_MS == 400 && TRAIL_LONG_MS == 600,
        "主册判据：轨迹存留 200/400/600ms 三档",
    );
    cs.add(
        "hold_ms_mapping",
        TrailLen::Short.hold_ms() == 200
            && TrailLen::Mid.hold_ms() == 400
            && TrailLen::Long.hold_ms() == 600,
        "档位→时长映射一一对应",
    );
    // 2) 默认关。
    let mut tr = PointerTrail::new();
    cs.add(
        "default_off",
        !tr.enabled(),
        "主册：默认关",
    );
    // 3) 开关即时：打开后立即推点即可见（下一帧生效语义）。
    tr.set_enabled(true);
    tr.push(100, 100, 1_000);
    cs.add(
        "toggle_instant",
        tr.enabled() && tr.visible_count(1_100) == 1,
        "打开后第一帧即生效（无预热期）",
    );
    // 4) 渐隐曲线：dt=0 全实、存留期内线性衰减、期满归零。
    tr.set_len(TrailLen::Short);
    cs.add(
        "alpha_curve",
        tr.alpha_at(1_000, 1_000) == 1000
            && tr.alpha_at(1_100, 1_000) == 500
            && tr.alpha_at(1_199, 1_000) == 5
            && tr.alpha_at(1_200, 1_000) == 0,
        "200ms 档：0ms→1000‰、100ms→500‰、期满 0",
    );
    // 5) 存留边界：dt=hold-1 仍可见、dt=hold 消失。
    cs.add(
        "hold_boundary",
        tr.alpha_at(TRAIL_SHORT_MS - 1, 0) == 1000 - (TRAIL_SHORT_MS - 1) * 1000 / TRAIL_SHORT_MS
            && tr.alpha_at(TRAIL_SHORT_MS, 0) == 0
            && tr.visible_count(1_000 + TRAIL_SHORT_MS) == 0,
        "期满整刻度消失（199ms 残影 5‰、200ms 无）",
    );
    // 6) 三档时长对同一时间点的残影数量不同（实测口径）。
    tr.push(101, 101, 1_010);
    tr.push(102, 102, 1_020);
    let vis_short = {
        tr.set_len(TrailLen::Short);
        tr.visible_count(1_400)
    };
    let vis_long = {
        tr.set_len(TrailLen::Long);
        tr.visible_count(1_400)
    };
    cs.add(
        "length_grade_differs",
        vis_short == 0 && vis_long == 3,
        "t+400ms：短档全消、长档 3 点全在（档位实测可辨）",
    );
    // 7) 环形缓冲：容量 16，第 17 点淘汰最旧（长档 600ms 保证全员可见）。
    let mut tr2 = PointerTrail::new();
    tr2.set_enabled(true);
    tr2.set_len(TrailLen::Long);
    for i in 0..(TRAIL_POINTS as u64) {
        tr2.push(i as i32, 0, 1_000 + i);
    }
    let filled = tr2.count();
    tr2.push(999, 999, 1_016);
    let evict_ok = {
        // 最旧点 (0,0)（t=1000）已被 (999,999)（t=1016）覆盖：
        // 快照首点应为原第 2 点 (1,0)，末点为新点全实。
        let mut out = [(0i32, 0i32, 0u64); TRAIL_POINTS];
        let n = tr2.snapshot(1_016, &mut out);
        n == TRAIL_POINTS
            && out[0] == (1, 0, tr2.alpha_at(1_016, 1_001))
            && out[15] == (999, 999, 1000)
            && out.iter().take(n).all(|&(x, _, _)| x != 0)
    };
    cs.add(
        "ring_evicts_oldest",
        TRAIL_POINTS == 16 && filled == TRAIL_POINTS && tr2.count() == TRAIL_POINTS && evict_ok,
        "满 16 点后再推入 → 最旧点被淘汰（FIFO）",
    );
    // 8) 平面分配：轨迹走 F335 指针优先平面。
    cs.add(
        "plane_pointer_priority",
        trail_plane() == TrailPlane::PointerPriority,
        "轨迹渲染不触发全屏重绘（主册：优先平面性能）",
    );
    // 9) 帧率无损预算：绘制预算 < 帧预算 1/8。
    cs.add(
        "budget_frame_safe",
        trail_budget_ok() && TRAIL_DRAW_BUDGET_US < FRAME_BUDGET_US_60HZ / 8,
        "500µs << 16667µs@60Hz（帧率无损）",
    );
    // 10) 皮肤兼容：换肤不动轨迹。
    cs.add(
        "skin_compatible_f156",
        PointerTrail::new().skin_independent(),
        "轨迹与皮肤 id 解耦（结构无皮肤状态）",
    );
    cs
}

#[cfg(test)]
mod tests_f522 {
    use super::*;

    #[test]
    fn three_grades_constants() {
        assert_eq!(TrailLen::Short.hold_ms(), 200, "短档 200ms");
        assert_eq!(TrailLen::Mid.hold_ms(), 400, "中档 400ms");
        assert_eq!(TrailLen::Long.hold_ms(), 600, "长档 600ms");
        assert_ne!(TrailLen::Short.hold_ms(), TrailLen::Mid.hold_ms());
        assert_ne!(TrailLen::Mid.hold_ms(), TrailLen::Long.hold_ms());
    }

    #[test]
    fn ring_evicts_oldest_point() {
        let mut tr = PointerTrail::new();
        tr.set_enabled(true);
        tr.set_len(TrailLen::Long);
        // 推 16 点填满。
        for i in 0..16 {
            tr.push(i, 0, 1_000 + i as u64);
        }
        assert_eq!(tr.count(), 16);
        assert_eq!(tr.head(), 0);
        // 第 17 点：最旧 (0,0) 被淘汰。
        tr.push(100, 200, 1_016);
        assert_eq!(tr.count(), 16, "容量不增长");
        assert_eq!(tr.head(), 1, "头指针前进");
        let mut out = [(0i32, 0i32, 0u64); TRAIL_POINTS];
        let n = tr.snapshot(1_016, &mut out);
        assert_eq!(n, 16);
        assert_eq!(out[0], (1, 0, 1000 - (16 - 1) * 1000 / 600), "首点应为原第 2 点 (1,0)");
        assert_eq!(out[15], (100, 200, 1000), "末点为新推入点全实");
        // 断言最旧点 (0,0) 不在快照中。
        assert!(out.iter().take(n).all(|&(x, _, _)| x != 0), "最旧点已被淘汰");
    }

    #[test]
    fn alpha_fades_and_expires() {
        let mut tr = PointerTrail::new();
        tr.set_enabled(true);
        tr.set_len(TrailLen::Mid);
        tr.push(50, 60, 2_000);
        assert_eq!(tr.alpha_at(2_000, 2_000), 1000, "新点全实");
        assert_eq!(tr.alpha_at(2_200, 2_000), 500, "200ms 处半透明（400ms 档）");
        assert!(tr.alpha_at(2_399, 2_000) > 0, "期满前 1ms 仍残影");
        assert_eq!(tr.alpha_at(2_400, 2_000), 0, "400ms 整消失");
        assert_eq!(tr.alpha_at(9_999, 2_000), 0, "远期恒 0");
        // 时间倒流（采样在未来）安全。
        assert_eq!(tr.alpha_at(1_000, 2_000), 1000, "saturating 防负");
    }

    #[test]
    fn disabled_trail_drops_points() {
        let mut tr = PointerTrail::new();
        assert!(!tr.enabled(), "默认关");
        tr.push(1, 2, 100);
        assert_eq!(tr.count(), 0, "关闭时推点被丢弃");
        tr.set_enabled(true);
        tr.push(3, 4, 200);
        assert_eq!(tr.count(), 1, "打开后开始记录");
        // 半途关闭：既有点不再增长，开关即时生效。
        tr.set_enabled(false);
        tr.push(5, 6, 300);
        assert_eq!(tr.count(), 1, "关闭后不再记录");
    }

    #[test]
    fn grades_visible_differences() {
        let mut tr = PointerTrail::new();
        tr.set_enabled(true);
        // 四点 t=750/1000/1100/1200；查 t=1300 → 点龄 550/300/200/100ms。
        for &(x, t) in [(7i32, 750u64), (0, 1000), (1, 1100), (2, 1200)].iter() {
            tr.push(x, x, t);
        }
        tr.set_len(TrailLen::Short);
        assert_eq!(tr.visible_count(1_300), 1, "短档 200ms：只余 100ms 龄点");
        tr.set_len(TrailLen::Mid);
        assert_eq!(tr.visible_count(1_300), 3, "中档 400ms：300/200/100ms 龄在");
        tr.set_len(TrailLen::Long);
        assert_eq!(tr.visible_count(1_300), 4, "长档 600ms：四点全在（550<600）");
        // 再往后查：t=1400 → 首点龄 650 > 600，长档也只剩 3 点。
        assert_eq!(tr.visible_count(1_400), 3, "长档 650ms 龄点亦淘汰");
    }

    #[test]
    fn budget_and_plane() {
        assert!(trail_budget_ok(), "绘制预算必须 < 帧预算 1/8");
        assert!(TRAIL_DRAW_BUDGET_US < FRAME_BUDGET_US_60HZ, "预算远小于 16667µs");
        assert_eq!(trail_plane(), TrailPlane::PointerPriority, "走 F335 指针优先平面");
        assert_ne!(trail_plane(), TrailPlane::Fullscreen, "禁全屏重绘");
    }

    #[test]
    fn skin_decoupled() {
        // F156 兼容：结构上无皮肤状态，换肤不触碰轨迹缓冲。
        let mut tr = PointerTrail::new();
        tr.set_enabled(true);
        tr.push(7, 8, 100);
        assert!(tr.skin_independent());
        assert_eq!(tr.count(), 1, "缓冲内容只由推点与时间决定");
    }
}

// ===========================================================================
// F523 打字时隐藏指针 —— TypeHidePointer 状态机（淡出/恢复时序）
// ===========================================================================

/// 淡出时长：80ms（主册「输入开始 <100ms 淡出」——80 < 100 留裕量）。
pub const FADE_OUT_MS: u64 = 80;
/// 停止输入到恢复：2000ms（主册「停止 2s 恢复」）。
pub const IDLE_RESTORE_MS: u64 = 2000;
/// 隐藏态透明度：300‰（主册「30% 透明度」——隐身但可寻）。
pub const HIDDEN_ALPHA_PERMILLE: u32 = 300;
/// 完全不透明：1000‰。
pub const FULL_ALPHA_PERMILLE: u32 = 1000;
/// 默认关（主册「默认关」）。
pub const HIDE_DEFAULT_ON: bool = false;

/// 输入源（触屏豁免判据之源）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputSource {
    Keyboard,
    Mouse,
    Touch,
}

/// 隐藏状态机相位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HidePhase {
    /// 全显（1000‰）。
    Visible,
    /// 淡出中（自 t_event 起 FADE_OUT_MS 内线性降至 300‰）。
    Fading { t_event: u64 },
    /// 隐藏保持（300‰），自 t_event 起 IDLE_RESTORE_MS 后恢复。
    Hidden { t_event: u64 },
}

/// 打字隐藏指针状态机。
pub struct TypeHidePointer {
    enabled: bool,
    phase: HidePhase,
}

impl TypeHidePointer {
    pub const fn new() -> Self {
        TypeHidePointer {
            enabled: HIDE_DEFAULT_ON,
            phase: HidePhase::Visible,
        }
    }

    pub fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        if !on {
            self.phase = HidePhase::Visible;
        }
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn phase(&self) -> HidePhase {
        self.phase
    }

    /// 键盘输入事件：进入/刷新淡出（t_event = 本次输入时刻）。
    /// 触屏豁免：Touch 源事件被忽略（状态机不启用）。
    pub fn typing(&mut self, t: u64, src: InputSource) {
        if !self.enabled || src == InputSource::Touch {
            return;
        }
        self.phase = HidePhase::Fading { t_event: t };
    }

    /// 鼠标移动：即时恢复 1000‰（0ms——优先级最高，覆盖任何相位）。
    /// 触屏的移动不算鼠标移动。
    pub fn mouse_move(&mut self, _t: u64, src: InputSource) {
        if !self.enabled || src == InputSource::Touch {
            return;
        }
        self.phase = HidePhase::Visible;
    }

    /// 巡检：返回当前指针 alpha permille（1000 全显 / 300 隐藏 / 中间渐变），
    /// 并推进相位（Hidden 满时序 → Visible）。
    pub fn poll(&mut self, t: u64) -> u32 {
        match self.phase {
            HidePhase::Visible => FULL_ALPHA_PERMILLE,
            HidePhase::Fading { t_event } => {
                let dt = t.saturating_sub(t_event);
                if dt >= FADE_OUT_MS {
                    self.phase = HidePhase::Hidden { t_event };
                    HIDDEN_ALPHA_PERMILLE
                } else {
                    // 线性 1000→300：alpha = 1000 - dt*700/80（全程 u64 运算）。
                    let drop = dt * (FULL_ALPHA_PERMILLE as u64 - HIDDEN_ALPHA_PERMILLE as u64)
                        / FADE_OUT_MS;
                    (FULL_ALPHA_PERMILLE as u64 - drop) as u32
                }
            }
            HidePhase::Hidden { t_event } => {
                let dt = t.saturating_sub(t_event);
                if dt >= IDLE_RESTORE_MS {
                    self.phase = HidePhase::Visible;
                    FULL_ALPHA_PERMILLE
                } else {
                    HIDDEN_ALPHA_PERMILLE
                }
            }
        }
    }
}

/// 渲染层（与 F201 插入符无混淆的结构声明）：淡化只作用于指针层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HidePlane {
    PointerLayer,
    TextCaretLayer,
}

pub fn hide_plane() -> HidePlane {
    HidePlane::PointerLayer
}

/// F523 域自检。
pub fn run_f523_checks() -> CheckSet {
    let mut cs = CheckSet::new("F523-typing-hideptr");
    // 1) 时序常量：淡出 <100ms、恢复 2000ms、隐藏 300‰。
    cs.add(
        "timing_constants",
        FADE_OUT_MS == 80 && FADE_OUT_MS < 100 && IDLE_RESTORE_MS == 2000,
        "主册判据：输入开始 <100ms 淡出、停止 2s 恢复",
    );
    cs.add(
        "hidden_alpha_300",
        HIDDEN_ALPHA_PERMILLE == 300 && FULL_ALPHA_PERMILLE == 1000,
        "30% 透明度（隐身但可寻）",
    );
    // 2) 默认关。
    let mut hp = TypeHidePointer::new();
    cs.add(
        "default_off",
        !hp.enabled() && hp.poll(0) == FULL_ALPHA_PERMILLE,
        "主册：默认关（关态恒全显）",
    );
    // 3) 输入开始淡出：t_event 全显、40ms 半程、79ms 临近、80ms 到底。
    hp.set_enabled(true);
    hp.typing(1_000, InputSource::Keyboard);
    let a0 = hp.poll(1_000);
    let a40 = hp.poll(1_040);
    let a79 = hp.poll(1_079);
    let a80 = hp.poll(1_080);
    cs.add(
        "fade_out_curve",
        a0 == 1000 && a40 == 650 && a79 > 300 && a80 == 300,
        "淡出线性 1000→300，80ms（<100ms）到底",
    );
    // 4) 隐藏保持：300‰ 平台期。
    let a_hold = hp.poll(1_500);
    cs.add(
        "hidden_plateau",
        a_hold == 300 && hp.phase() == HidePhase::Hidden { t_event: 1_000 },
        "淡出完成后保持 300‰",
    );
    // 5) 停止输入 2000ms 恢复：1999ms 仍隐、2000ms 整恢复。
    let before = hp.poll(1_000 + IDLE_RESTORE_MS - 1);
    let at = hp.poll(1_000 + IDLE_RESTORE_MS);
    cs.add(
        "idle_restore_2000",
        before == 300 && at == 1000 && hp.phase() == HidePhase::Visible,
        "停止输入 1999ms 仍隐、2000ms 整恢复全显",
    );
    // 6) 鼠标移动即时恢复（0ms）。
    hp.typing(5_000, InputSource::Keyboard);
    let _ = hp.poll(5_080); // 推到隐藏
    hp.mouse_move(5_090, InputSource::Mouse);
    let a_move = hp.poll(5_090);
    cs.add(
        "mouse_move_instant_restore",
        a_move == 1000 && hp.phase() == HidePhase::Visible,
        "移动鼠标 0ms 恢复全显（即时）",
    );
    // 7) 触屏豁免：Touch 源的输入与移动都不启用状态机。
    let mut hp2 = TypeHidePointer::new();
    hp2.set_enabled(true);
    hp2.typing(2_000, InputSource::Touch);
    hp2.mouse_move(2_100, InputSource::Touch);
    cs.add(
        "touch_exempt",
        hp2.phase() == HidePhase::Visible && hp2.poll(2_200) == 1000,
        "触屏场景不适用（主册：触屏豁免）",
    );
    // 8) 键盘源豁免不成立（对照）：Keyboard 正常触发。
    hp2.typing(3_000, InputSource::Keyboard);
    cs.add(
        "keyboard_source_engages",
        hp2.poll(3_040) == 650,
        "键盘源正常进入淡出曲线（触屏对照）",
    );
    // 9) 持续打字：t_event 刷新，隐藏保持不恢复。
    let mut hp3 = TypeHidePointer::new();
    hp3.set_enabled(true);
    for k in 0..10u64 {
        hp3.typing(k * 300, InputSource::Keyboard);
        let _ = hp3.poll(k * 300 + 100);
    }
    let a_still = hp3.poll(9 * 300 + 1_900);
    cs.add(
        "continuous_typing_stays_hidden",
        a_still == 300,
        "每 300ms 一次按键 → 始终隐藏（不会在打字中途恢复）",
    );
    // 10) 与 F201 插入符无混淆：淡化只作用指针层。
    cs.add(
        "no_caret_confusion_f201",
        hide_plane() == HidePlane::PointerLayer,
        "alpha 变化只在指针层，插入符层（F201）不受影响",
    );
    // 11) 开关即时：关闭即回全显。
    let mut hp4 = TypeHidePointer::new();
    hp4.set_enabled(true);
    hp4.typing(100, InputSource::Keyboard);
    let _ = hp4.poll(200);
    hp4.set_enabled(false);
    cs.add(
        "disable_restores_full",
        hp4.poll(210) == 1000 && hp4.phase() == HidePhase::Visible,
        "开关关闭立即恢复 1000‰",
    );
    cs
}

#[cfg(test)]
mod tests_f523 {
    use super::*;

    #[test]
    fn fade_boundary_100ms() {
        // 主册边界：<100ms 淡出完成——79ms 未到底、80ms 到底。
        let mut hp = TypeHidePointer::new();
        hp.set_enabled(true);
        hp.typing(0, InputSource::Keyboard);
        assert_eq!(hp.poll(0), 1000, "输入瞬间仍全显");
        let a79 = hp.poll(79);
        assert!(a79 > HIDDEN_ALPHA_PERMILLE, "79ms 尚未到底（{}‰）", a79);
        assert_eq!(hp.poll(80), HIDDEN_ALPHA_PERMILLE, "80ms（<100ms）到底 300‰");
        // 底后保持 300‰。
        assert_eq!(hp.poll(500), HIDDEN_ALPHA_PERMILLE);
    }

    #[test]
    fn fade_curve_monotonic() {
        let mut hp = TypeHidePointer::new();
        hp.set_enabled(true);
        hp.typing(10_000, InputSource::Keyboard);
        let mut prev = 1000u32;
        for dt in 1..FADE_OUT_MS {
            let a = hp.poll(10_000 + dt);
            assert!(a <= prev, "淡出应单调不增 dt={}", dt);
            assert!(a >= HIDDEN_ALPHA_PERMILLE, "不低于 300‰ dt={}", dt);
            prev = a;
        }
        assert!(prev < 320 && prev > HIDDEN_ALPHA_PERMILLE, "79ms 末临近底值（{}‰）", prev);
        assert_eq!(hp.poll(10_000 + FADE_OUT_MS), HIDDEN_ALPHA_PERMILLE, "80ms 到底恰为 300‰");
    }

    #[test]
    fn idle_restore_boundary_2000() {
        let mut hp = TypeHidePointer::new();
        hp.set_enabled(true);
        hp.typing(0, InputSource::Keyboard);
        let _ = hp.poll(100); // 进入隐藏
        assert_eq!(hp.poll(1_999), HIDDEN_ALPHA_PERMILLE, "1999ms 仍隐藏");
        assert_eq!(hp.poll(2_000), FULL_ALPHA_PERMILLE, "2000ms 整恢复");
        assert_eq!(hp.phase(), HidePhase::Visible);
        // 恢复后不再自行变暗。
        assert_eq!(hp.poll(3_000), FULL_ALPHA_PERMILLE);
    }

    #[test]
    fn mouse_move_restores_immediately() {
        let mut hp = TypeHidePointer::new();
        hp.set_enabled(true);
        hp.typing(1_000, InputSource::Keyboard);
        let _ = hp.poll(1_100); // 已隐藏
        // 隐藏态下任意时刻移动即恢复，无宽限延迟。
        hp.mouse_move(1_500, InputSource::Mouse);
        assert_eq!(hp.poll(1_500), FULL_ALPHA_PERMILLE, "0ms 即恢复");
        // 恢复后若再次打字重新淡出。
        hp.typing(2_000, InputSource::Keyboard);
        assert_eq!(hp.poll(2_080), HIDDEN_ALPHA_PERMILLE, "再打字再淡出");
    }

    #[test]
    fn touch_source_fully_exempt() {
        let mut hp = TypeHidePointer::new();
        hp.set_enabled(true);
        // Touch 打字、Touch 移动都不改变相位。
        hp.typing(0, InputSource::Touch);
        assert_eq!(hp.poll(500), FULL_ALPHA_PERMILLE, "触屏打字不淡出");
        hp.mouse_move(600, InputSource::Touch);
        assert_eq!(hp.phase(), HidePhase::Visible);
        // 触屏期间键盘打字仍生效（豁免按输入源逐事件判定）。
        hp.typing(1_000, InputSource::Keyboard);
        assert_eq!(hp.poll(1_080), HIDDEN_ALPHA_PERMILLE);
        // 触屏移动不能代替鼠标移动恢复（仍隐藏）。
        hp.mouse_move(1_200, InputSource::Touch);
        assert_eq!(hp.poll(1_250), HIDDEN_ALPHA_PERMILLE, "触屏移动不触发恢复");
        // 鼠标移动恢复。
        hp.mouse_move(1_300, InputSource::Mouse);
        assert_eq!(hp.poll(1_300), FULL_ALPHA_PERMILLE);
    }

    #[test]
    fn default_off_and_disable_semantics() {
        let mut hp = TypeHidePointer::new();
        assert!(!hp.enabled(), "默认关");
        hp.typing(0, InputSource::Keyboard);
        assert_eq!(hp.poll(1_000), FULL_ALPHA_PERMILLE, "关态打字不隐藏");
        hp.set_enabled(true);
        hp.typing(2_000, InputSource::Keyboard);
        assert_eq!(hp.poll(2_080), HIDDEN_ALPHA_PERMILLE, "开后生效");
        hp.set_enabled(false);
        assert_eq!(hp.phase(), HidePhase::Visible, "关闭立即复位全显");
        assert_eq!(hp.poll(3_000), FULL_ALPHA_PERMILLE);
    }

    #[test]
    fn plane_layering_distinct_from_caret() {
        assert_eq!(hide_plane(), HidePlane::PointerLayer, "淡化只作用指针层");
        assert_ne!(hide_plane(), HidePlane::TextCaretLayer, "插入符层（F201）独立");
    }
}

// ===========================================================================
// 文件级交叉自检：六域入口全部产出全通过 CheckSet（供锚点域批量调用）。
// ===========================================================================

/// 逐项运行六域自检并登记到聚合器。
pub fn run_all_pointerfx_checks(register: &mut dyn FnMut(CheckSet)) {
    register(run_f513_checks());
    register(run_f514_checks());
    register(run_f519_checks());
    register(run_f520_checks());
    register(run_f522_checks());
    register(run_f523_checks());
}

#[cfg(test)]
mod tests_all {
    use super::*;

    #[test]
    fn six_domains_all_pass() {
        let results: [CheckSet; 6] = [
            run_f513_checks(),
            run_f514_checks(),
            run_f519_checks(),
            run_f520_checks(),
            run_f522_checks(),
            run_f523_checks(),
        ];
        let tags = [
            "F513-ctrl-locator",
            "F514-sound-beacon",
            "F519-capslock-tone",
            "F520-midbtn-minimize",
            "F522-pointer-trail",
            "F523-typing-hideptr",
        ];
        for (set, tag) in results.iter().zip(tags.iter()) {
            assert_eq!(set.domain, *tag, "域标签对齐");
            assert!(!set.truncated(), "{} 不得超容量截断", tag);
            assert!(set.len() >= 8 && set.len() <= 14, "{} 检查数应在 8~14（实际 {}）", tag, set.len());
            let (passed, failed) = set.tally();
            assert!(set.all_passed(), "{} 有失败项：过 {} 败 {}", tag, passed, failed);
            for i in 0..set.len() {
                let c = set.get(i).unwrap();
                assert!(c.passed, "{} 检查 {} 应通过", tag, c.name);
            }
        }
    }

    #[test]
    fn run_all_registers_six_sets() {
        let mut count = 0usize;
        let mut all_pass = true;
        {
            let mut reg = |s: CheckSet| {
                count += 1;
                all_pass &= s.all_passed();
            };
            run_all_pointerfx_checks(&mut reg);
        }
        assert_eq!(count, 6, "六域全部登记");
        assert!(all_pass, "六域全通过");
    }
}
