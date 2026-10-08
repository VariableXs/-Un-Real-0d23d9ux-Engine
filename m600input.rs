//! m600input — VARIX-M600 AI-08 输入手感域 (F176~F200)
//!
//! 输入管线端到端/手感调音台/触控板手势谱/压感笔工坊/键盘节奏引擎/
//! 中文输入深度优化/输入法皮肤 SDK/快捷键谱系学/手势冲突仲裁庭/滚动物理学/
//! 惯性曲线库/触屏 palm 优雅/多点触控乐谱/手写公式引擎/语音听写管线/
//! 眼动输入实验/脑机接口预留位/输入回放调试/手感金样本/键位迁移向导/
//! 游戏直通模式/输入隐私保护/无障碍输入套装/手感回归走廊/输入年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。
//! 登记类接口自带去重或容量上限拒绝；自检断言遵守"末态读取"禁令。

use crate::checks::CheckSet;

// ===========================================================================
// F176 — 输入管线端到端：五个级联段必须按序到齐且总时延在预算内
// ===========================================================================

pub const PIPELINE_STAGE_COUNT: usize = 5;
pub const PIPELINE_BUDGET_US: u32 = 8000;

/// 一段管线跳：段号 + 该段耗时（微秒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PipelineHop {
    pub stage: u8,
    pub latency_us: u32,
}

/// 端到端成立当且仅当：恰好 5 段、段号严格为 0..5、总时延不超预算。
pub fn pipeline_complete(hops: &[PipelineHop]) -> bool {
    if hops.len() != PIPELINE_STAGE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < hops.len() {
        if hops[i].stage as usize != i {
            return false;
        }
        i += 1;
    }
    let mut total = 0u32;
    let mut j = 0usize;
    while j < hops.len() {
        total += hops[j].latency_us;
        j += 1;
    }
    total <= PIPELINE_BUDGET_US
}

// ===========================================================================
// F177 — 手感调音台：4 路滑杆，越界写入钳到 1000‰
// ===========================================================================

pub const FEEL_SLIDER_COUNT: usize = 4;
pub const FEEL_SLIDER_MAX: u32 = 1000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeelConsole {
    pub sliders: [u32; FEEL_SLIDER_COUNT],
}

impl FeelConsole {
    pub const fn new() -> FeelConsole {
        FeelConsole { sliders: [0; FEEL_SLIDER_COUNT] }
    }

    /// 设置滑杆：段位非法返回 false；数值超上限静默钳位。
    pub fn set_slider(&mut self, index: usize, value: u32) -> bool {
        if index >= FEEL_SLIDER_COUNT {
            return false;
        }
        self.sliders[index] = if value > FEEL_SLIDER_MAX { FEEL_SLIDER_MAX } else { value };
        true
    }

    /// 四路平均值（permille）。
    pub fn average_permille(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < FEEL_SLIDER_COUNT {
            sum += self.sliders[i];
            i += 1;
        }
        sum / FEEL_SLIDER_COUNT as u32
    }
}

// ===========================================================================
// F178 — 触控板手势谱：按手指数 + 缩放量分类
// ===========================================================================

pub const PINCH_SCALE_STEP: u32 = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchpadGesture {
    Move,
    Scroll,
    Pinch,
}

/// ≥2 指且缩放量达步长 → Pinch；≥2 指未达步长 → Scroll；否则 Move。
pub fn gesture_classify(fingers: u8, dscale_permille: u32) -> TouchpadGesture {
    if fingers >= 2 && dscale_permille >= PINCH_SCALE_STEP {
        TouchpadGesture::Pinch
    } else if fingers >= 2 {
        TouchpadGesture::Scroll
    } else {
        TouchpadGesture::Move
    }
}

// ===========================================================================
// F179 — 压感笔工坊：压感映射透明度，倾斜按比例吃掉压感
// ===========================================================================

pub const PEN_PALM_PRESSURE: u32 = 50;
pub const PEN_TILT_PENALTY_DIV: u32 = 4;

/// 压力低于掌压阈值视为手掌 → 0；否则压感减去 tilt/4 的惩罚。
pub fn pen_opacity(pressure_permille: u32, tilt_permille: u32) -> u32 {
    if pressure_permille < PEN_PALM_PRESSURE {
        0
    } else {
        let penalty = tilt_permille / PEN_TILT_PENALTY_DIV;
        pressure_permille.saturating_sub(penalty)
    }
}

pub fn pen_is_palm(pressure_permille: u32) -> bool {
    pressure_permille < PEN_PALM_PRESSURE
}

// ===========================================================================
// F180 — 键盘节奏引擎：击键数 → WPM → 爆发判定
// ===========================================================================

pub const RHYTHM_CHARS_PER_WORD: u32 = 5;
pub const RHYTHM_BURST_WPM: u32 = 80;

/// WPM = hits / (window_ms / 60000) / 5，全整数。
pub fn rhythm_wpm(hits: u32, window_ms: u32) -> u32 {
    if window_ms == 0 {
        0
    } else {
        hits * 60_000 / window_ms / RHYTHM_CHARS_PER_WORD
    }
}

pub fn rhythm_burst(hits: u32, window_ms: u32) -> bool {
    rhythm_wpm(hits, window_ms) >= RHYTHM_BURST_WPM
}

// ===========================================================================
// F181 — 中文输入深度优化：音节合法性 + 候选池去重
// ===========================================================================

pub const PINYIN_SYLL_MAX: usize = 6;
pub const PINYIN_POOL_CAP: usize = 16;

/// 音节合法：非空、≤6 字节、全小写 ASCII。
pub fn pinyin_syllable_ok(s: &[u8]) -> bool {
    if s.is_empty() || s.len() > PINYIN_SYLL_MAX {
        return false;
    }
    s.iter().all(|&b| b.is_ascii_lowercase())
}

/// 候选池：同分候选去重，容量上限拒绝。
#[derive(Clone, Copy, Debug)]
pub struct PinyinPool {
    pub scores: [u32; PINYIN_POOL_CAP],
    pub count: usize,
}

impl PinyinPool {
    pub const fn new() -> PinyinPool {
        PinyinPool { scores: [0; PINYIN_POOL_CAP], count: 0 }
    }

    pub fn add(&mut self, score: u32) -> bool {
        if self.count >= PINYIN_POOL_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.scores[i] == score {
                return false;
            }
            i += 1;
        }
        self.scores[self.count] = score;
        self.count += 1;
        true
    }
}

// ===========================================================================
// F182 — 输入法皮肤 SDK：清单校验 + 槽位认领（一次占坑）
// ===========================================================================

pub const SKIN_COLOR_MIN: u8 = 2;
pub const SKIN_COLOR_MAX: u8 = 16;
pub const SKIN_SLOT_COUNT: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkinManifest {
    pub name_hash: u64,
    pub colors: u8,
}

pub fn skin_manifest_ok(m: SkinManifest) -> bool {
    m.name_hash != 0 && m.colors >= SKIN_COLOR_MIN && m.colors <= SKIN_COLOR_MAX
}

/// 槽位板：认领过的槽不可重复认领（去重）。
#[derive(Clone, Copy, Debug)]
pub struct SkinSlotBoard {
    pub taken: [bool; SKIN_SLOT_COUNT],
    pub count: usize,
}

impl SkinSlotBoard {
    pub const fn new() -> SkinSlotBoard {
        SkinSlotBoard { taken: [false; SKIN_SLOT_COUNT], count: 0 }
    }

    pub fn claim(&mut self, index: usize) -> bool {
        if index >= SKIN_SLOT_COUNT || self.taken[index] {
            return false;
        }
        self.taken[index] = true;
        self.count += 1;
        true
    }
}

// ===========================================================================
// F183 — 快捷键谱系学：chord = mods<<8 | keycode，重复注册即冲突
// ===========================================================================

pub const CHORD_LEDGER_CAP: usize = 32;

pub fn chord_make(mods: u32, keycode: u8) -> u32 {
    (mods << 8) | keycode as u32
}

pub fn chord_mods(chord: u32) -> u32 {
    chord >> 8
}

pub fn chord_key(chord: u32) -> u8 {
    (chord & 0xFF) as u8
}

/// 和弦登记簿：同一 chord 只登记一次（冲突拒绝），容量上限拒绝。
#[derive(Clone, Copy, Debug)]
pub struct ChordLedger {
    pub chords: [u32; CHORD_LEDGER_CAP],
    pub count: usize,
}

impl ChordLedger {
    pub const fn new() -> ChordLedger {
        ChordLedger { chords: [0; CHORD_LEDGER_CAP], count: 0 }
    }

    pub fn register(&mut self, chord: u32) -> bool {
        if self.count >= CHORD_LEDGER_CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.chords[i] == chord {
                return false;
            }
            i += 1;
        }
        self.chords[self.count] = chord;
        self.count += 1;
        true
    }
}

// ===========================================================================
// F184 — 手势冲突仲裁庭：优先级高者胜，平级小 id 胜
// ===========================================================================

/// 返回获胜者的手势 id。
pub fn arbiter_pick(id_a: u16, prio_a: u8, id_b: u16, prio_b: u8) -> u16 {
    if prio_a > prio_b {
        id_a
    } else if prio_b > prio_a {
        id_b
    } else if id_a <= id_b {
        id_a
    } else {
        id_b
    }
}

// ===========================================================================
// F185 — 滚动物理学：摩擦衰减 + 甩动停止计时
// ===========================================================================

pub const SCROLL_FRICTION_DEFAULT: u32 = 120;
pub const SCROLL_STOP_VELOCITY: u32 = 10;

/// 一帧衰减：v' = v * (1000 - friction) / 1000。
pub fn scroll_decay(vel: u32, friction_permille: u32) -> u32 {
    vel * (1000 - friction_permille) / 1000
}

/// 甩动到停所需帧数（上限 10000 防死循环）。
pub fn fling_ticks(mut vel: u32, friction_permille: u32) -> u32 {
    let mut ticks = 0u32;
    while vel >= SCROLL_STOP_VELOCITY && ticks < 10_000 {
        vel = scroll_decay(vel, friction_permille);
        ticks += 1;
    }
    ticks
}

// ===========================================================================
// F186 — 惯性曲线库：线性 / 缓入 / 缓出（permille 定点）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InertiaCurve {
    Linear,
    EaseIn,
    EaseOut,
}

pub fn inertia_sample(curve: InertiaCurve, t_permille: u32) -> u32 {
    let t = if t_permille > 1000 { 1000 } else { t_permille };
    match curve {
        InertiaCurve::Linear => t,
        InertiaCurve::EaseIn => t * t / 1000,
        InertiaCurve::EaseOut => t * (2000 - t) / 1000,
    }
}

// ===========================================================================
// F187 — 触屏 palm 优雅：大面积低压判掌，掌触不计入手指数
// ===========================================================================

pub const PALM_AREA_MIN: u32 = 700;
pub const PALM_PRESSURE_MAX: u32 = 300;

pub fn touch_is_palm(area_permille: u32, pressure_permille: u32) -> bool {
    area_permille >= PALM_AREA_MIN && pressure_permille <= PALM_PRESSURE_MAX
}

/// 过滤后真实手指数。
pub fn palm_filter(touch_count: u32, palm_count: u32) -> u32 {
    touch_count.saturating_sub(palm_count)
}

// ===========================================================================
// F188 — 多点触控乐谱：10 槽位按触点 id 进出场
// ===========================================================================

pub const MT_SLOT_COUNT: usize = 10;

#[derive(Clone, Copy, Debug)]
pub struct MultiTouchBoard {
    pub ids: [Option<u16>; MT_SLOT_COUNT],
}

impl MultiTouchBoard {
    pub const fn new() -> MultiTouchBoard {
        MultiTouchBoard { ids: [None; MT_SLOT_COUNT] }
    }

    /// 触点落下：返回占用的槽位；板满返回 None。
    pub fn down(&mut self, id: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < MT_SLOT_COUNT {
            if self.ids[i].is_none() {
                self.ids[i] = Some(id);
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 触点抬起：按 id 找槽并清空。
    pub fn up(&mut self, id: u16) -> bool {
        let mut i = 0usize;
        while i < MT_SLOT_COUNT {
            if self.ids[i] == Some(id) {
                self.ids[i] = None;
                return true;
            }
            i += 1;
        }
        false
    }

    pub fn active(&self) -> usize {
        self.ids.iter().filter(|s| s.is_some()).count()
    }
}

// ===========================================================================
// F189 — 手写公式引擎：笔画数 + 交叉 + 对齐 → 符号分类
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandwriteGlyph {
    Minus,
    Plus,
    Equals,
    Unknown,
}

/// 1 笔对齐 → Minus；2 笔交叉 → Plus；2 笔不交叉 → Equals；其余 Unknown。
pub fn glyph_classify(strokes: u8, cross: bool, aligned: bool) -> HandwriteGlyph {
    if !aligned {
        HandwriteGlyph::Unknown
    } else if strokes == 1 {
        HandwriteGlyph::Minus
    } else if strokes == 2 {
        if cross {
            HandwriteGlyph::Plus
        } else {
            HandwriteGlyph::Equals
        }
    } else {
        HandwriteGlyph::Unknown
    }
}

// ===========================================================================
// F190 — 语音听写管线：置信度过闸记账
// ===========================================================================

pub const DICTATION_MIN_CONF: u32 = 700;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DictationTally {
    pub accepted: u32,
    pub rejected: u32,
}

impl DictationTally {
    pub fn feed(&mut self, conf_permille: u32) -> bool {
        if conf_permille >= DICTATION_MIN_CONF {
            self.accepted += 1;
            true
        } else {
            self.rejected += 1;
            false
        }
    }

    pub fn acceptance_permille(&self) -> u32 {
        let total = self.accepted + self.rejected;
        if total == 0 {
            0
        } else {
            self.accepted * 1000 / total
        }
    }
}

// ===========================================================================
// F191 — 眼动输入实验：驻留时长 + 抖动幅度双重门槛
// ===========================================================================

pub const GAZE_DWELL_MS: u32 = 600;
pub const GAZE_JITTER_MAX: u32 = 50;

pub fn gaze_select(dwell_ms: u32, jitter_permille: u32) -> bool {
    dwell_ms >= GAZE_DWELL_MS && jitter_permille <= GAZE_JITTER_MAX
}

/// 驻留进度 permille，封顶 1000。
pub fn gaze_progress(dwell_ms: u32, need_ms: u32) -> u32 {
    if need_ms == 0 {
        return 1000;
    }
    let p = dwell_ms * 1000 / need_ms;
    if p > 1000 {
        1000
    } else {
        p
    }
}

// ===========================================================================
// F192 — 脑机接口预留位：4 通道认领（同人去重）+ 信号门槛
// ===========================================================================

pub const BCI_SLOT_CAP: usize = 4;
pub const BCI_MIN_CONF: u32 = 700;

#[derive(Clone, Copy, Debug)]
pub struct BciLedger {
    pub channels: [Option<u32>; BCI_SLOT_CAP],
    pub count: usize,
}

impl BciLedger {
    pub const fn new() -> BciLedger {
        BciLedger { channels: [None; BCI_SLOT_CAP], count: 0 }
    }

    /// 认领通道：同一 owner 不得重复认领；满员拒绝。
    pub fn claim(&mut self, owner: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < BCI_SLOT_CAP {
            if self.channels[i] == Some(owner) {
                return None;
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < BCI_SLOT_CAP {
            if self.channels[j].is_none() {
                self.channels[j] = Some(owner);
                self.count += 1;
                return Some(j);
            }
            j += 1;
        }
        None
    }
}

pub fn bci_signal_ok(conf_permille: u32) -> bool {
    conf_permille >= BCI_MIN_CONF
}

// ===========================================================================
// F193 — 输入回放调试：序列严格递增记录 + 末态比对
// ===========================================================================

pub const REPLAY_LOG_CAP: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct ReplayLedger {
    pub seqs: [u64; REPLAY_LOG_CAP],
    pub count: usize,
}

impl ReplayLedger {
    pub const fn new() -> ReplayLedger {
        ReplayLedger { seqs: [0; REPLAY_LOG_CAP], count: 0 }
    }

    /// 记录事件号：必须严格递增；容量上限拒绝。
    pub fn record(&mut self, seq: u64) -> bool {
        if self.count >= REPLAY_LOG_CAP {
            return false;
        }
        if self.count > 0 && seq <= self.seqs[self.count - 1] {
            return false;
        }
        self.seqs[self.count] = seq;
        self.count += 1;
        true
    }
}

/// 回放末态与期望剧本逐项一致。
pub fn replay_log_matches(log: &ReplayLedger, expected: &[u64]) -> bool {
    if log.count != expected.len() {
        return false;
    }
    let mut i = 0usize;
    while i < expected.len() {
        if log.seqs[i] != expected[i] {
            return false;
        }
        i += 1;
    }
    true
}

// ===========================================================================
// F194 — 手感金样本：固定剧本 → 固定末态
// ===========================================================================

pub const GOLDEN_KEYS: u32 = 10;
pub const GOLDEN_IN_RHYTHM: u32 = 8;
pub const GOLDEN_AVG_LATENCY_US: u32 = 450;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeelGolden {
    pub keys: u32,
    pub in_rhythm: u32,
    pub total_latency_us: u32,
}

/// 金样剧本：10 键、8 键在节奏内、总时延 4500us。
pub fn feel_golden_sample() -> FeelGolden {
    FeelGolden { keys: GOLDEN_KEYS, in_rhythm: GOLDEN_IN_RHYTHM, total_latency_us: 4500 }
}

/// 期望末态：平均时延 450us，节奏命中率 800‰。
pub fn feel_golden_matches(s: &FeelGolden) -> bool {
    s.keys == GOLDEN_KEYS
        && s.in_rhythm == GOLDEN_IN_RHYTHM
        && s.total_latency_us / s.keys == GOLDEN_AVG_LATENCY_US
        && s.in_rhythm * 1000 / s.keys == 800
}

// ===========================================================================
// F195 — 键位迁移向导：每源一键、每目标一源，进度按槽位计
// ===========================================================================

pub const MIGRATOR_SLOTS: usize = 8;

#[derive(Clone, Copy, Debug)]
pub struct KeymapMigrator {
    pub map: [Option<u8>; MIGRATOR_SLOTS],
}

impl KeymapMigrator {
    pub const fn new() -> KeymapMigrator {
        KeymapMigrator { map: [None; MIGRATOR_SLOTS] }
    }

    /// 源位已迁移（去重）或目标键已被别的源占用 → 拒绝。
    pub fn remap(&mut self, src: usize, dst: u8) -> bool {
        if src >= MIGRATOR_SLOTS || self.map[src].is_some() {
            return false;
        }
        let mut i = 0usize;
        while i < MIGRATOR_SLOTS {
            if self.map[i] == Some(dst) {
                return false;
            }
            i += 1;
        }
        self.map[src] = Some(dst);
        true
    }

    pub fn progress_permille(&self) -> u32 {
        let mapped = self.map.iter().filter(|m| m.is_some()).count() as u32;
        mapped * 1000 / MIGRATOR_SLOTS as u32
    }
}

// ===========================================================================
// F196 — 游戏直通模式：直通生效时预算收紧到 2ms
// ===========================================================================

pub const FEEL_NORMAL_BUDGET_US: u32 = 8000;
pub const GAME_BUDGET_US: u32 = 2000;

pub fn passthrough_active(gaming: bool, exclusive_ok: bool) -> bool {
    gaming && exclusive_ok
}

pub fn passthrough_budget_us(gaming: bool, exclusive_ok: bool) -> u32 {
    if passthrough_active(gaming, exclusive_ok) {
        GAME_BUDGET_US
    } else {
        FEEL_NORMAL_BUDGET_US
    }
}

// ===========================================================================
// F197 — 输入隐私保护：密码域按键即焚 + 保护域标记去重
// ===========================================================================

pub const PRIVACY_FIELD_CAP: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldPrivacy {
    Normal,
    Password,
}

/// 密码域击键不留痕（返回 None），普通域原样放行。
pub fn redact_keystroke(kind: FieldPrivacy, ch: u8) -> Option<u8> {
    match kind {
        FieldPrivacy::Password => None,
        FieldPrivacy::Normal => Some(ch),
    }
}

/// 保护域登记板：同一域只标记一次。
#[derive(Clone, Copy, Debug)]
pub struct PrivacyFieldBoard {
    pub marked: [bool; PRIVACY_FIELD_CAP],
    pub count: usize,
}

impl PrivacyFieldBoard {
    pub const fn new() -> PrivacyFieldBoard {
        PrivacyFieldBoard { marked: [false; PRIVACY_FIELD_CAP], count: 0 }
    }

    pub fn mark(&mut self, index: usize) -> bool {
        if index >= PRIVACY_FIELD_CAP || self.marked[index] {
            return false;
        }
        self.marked[index] = true;
        self.count += 1;
        true
    }
}

// ===========================================================================
// F198 — 无障碍输入套装：粘滞键锁存 + 慢速键放行
// ===========================================================================

pub const A11Y_STICKY_MS: u32 = 1000;
pub const A11Y_SLOW_MS: u32 = 300;

/// 粘滞键：功能开启且按住时长达标才锁存。
pub fn sticky_latch(hold_ms: u32, enabled: bool) -> bool {
    enabled && hold_ms >= A11Y_STICKY_MS
}

/// 慢速键：功能关闭直接放行；开启则须按住满慢速阈值。
pub fn slow_key_accept(hold_ms: u32, enabled: bool) -> bool {
    !enabled || hold_ms >= A11Y_SLOW_MS
}

// ===========================================================================
// F199 — 手感回归走廊：采样值落在金样 ±100‰ 容差内
// ===========================================================================

pub const CORRIDOR_TOLERANCE: u32 = 100;

pub fn corridor_within(sample: u32, golden: u32) -> bool {
    let diff = if sample > golden { sample - golden } else { golden - sample };
    diff * 1000 <= golden * CORRIDOR_TOLERANCE
}

// ===========================================================================
// F200 — 输入年报：章节完备性
// ===========================================================================

pub const M600_INPUT_REPORT_SECTIONS: [&str; 5] =
    ["pipeline", "feel", "gestures", "privacy", "a11y"];

pub fn m600_input_report_complete(filled: u32) -> bool {
    filled >= M600_INPUT_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m600input_checks() -> CheckSet {
    let mut set = CheckSet::new("m600input");

    // F176 输入管线端到端
    let good = [
        PipelineHop { stage: 0, latency_us: 1000 },
        PipelineHop { stage: 1, latency_us: 1500 },
        PipelineHop { stage: 2, latency_us: 2000 },
        PipelineHop { stage: 3, latency_us: 1200 },
        PipelineHop { stage: 4, latency_us: 1300 },
    ];
    let mut slow = good;
    slow[4].latency_us = 3000;
    set.add(
        "F176 pipeline end to end",
        pipeline_complete(&good) && !pipeline_complete(&slow),
        "ordered hops within 8ms",
    );
    set.add("F176 pipeline stage count", !pipeline_complete(&good[..4]), "missing stage rejected");

    // F177 手感调音台
    let mut console = FeelConsole::new();
    console.set_slider(0, 700);
    console.set_slider(1, 1500);
    let s0 = console.sliders[0];
    let s1 = console.sliders[1];
    set.add("F177 feel console clamp", s0 == 700 && s1 == 1000, "over-max clamped");
    let bad_index = console.set_slider(9, 100);
    console.set_slider(2, 200);
    console.set_slider(3, 100);
    let avg = console.average_permille();
    set.add("F177 feel console range", !bad_index && avg == 500, "(700+1000+200+100)/4");

    // F178 触控板手势谱
    set.add(
        "F178 gesture classes",
        gesture_classify(1, 0) == TouchpadGesture::Move
            && gesture_classify(2, 30) == TouchpadGesture::Scroll
            && gesture_classify(2, 80) == TouchpadGesture::Pinch,
        "fingers + scale decide",
    );
    set.add("F178 gesture pinch step", gesture_classify(3, PINCH_SCALE_STEP) == TouchpadGesture::Pinch, "boundary inclusive");

    // F179 压感笔工坊
    set.add(
        "F179 pen opacity",
        pen_opacity(800, 0) == 800 && pen_opacity(800, 400) == 700 && pen_opacity(30, 0) == 0,
        "tilt penalty + palm zero",
    );
    set.add("F179 pen palm", pen_is_palm(30) && !pen_is_palm(800), "palm below 50");

    // F180 键盘节奏引擎
    set.add(
        "F180 rhythm wpm",
        rhythm_wpm(100, 60_000) == 20 && rhythm_wpm(300, 60_000) == 60 && rhythm_wpm(50, 10_000) == 60,
        "hits over window / 5",
    );
    set.add(
        "F180 rhythm burst",
        !rhythm_burst(50, 10_000) && rhythm_burst(70, 10_000),
        "60 wpm calm, 84 wpm burst",
    );

    // F181 中文输入深度优化
    set.add(
        "F181 pinyin syllable",
        pinyin_syllable_ok(b"nihao") && !pinyin_syllable_ok(b"nihao!")
            && !pinyin_syllable_ok(b"NIHAO") && !pinyin_syllable_ok(b"") && !pinyin_syllable_ok(b"aaaaaaa"),
        "lowercase only, len 1..=6",
    );
    let mut pool = PinyinPool::new();
    let pa = pool.add(10);
    let pb = pool.add(10);
    let pc = pool.add(20);
    let pcount = pool.count;
    set.add("F181 pinyin pool dedup", pa && !pb && pc && pcount == 2, "same score rejected");

    // F182 输入法皮肤 SDK
    set.add(
        "F182 skin manifest",
        skin_manifest_ok(SkinManifest { name_hash: 1, colors: 8 })
            && !skin_manifest_ok(SkinManifest { name_hash: 0, colors: 8 })
            && !skin_manifest_ok(SkinManifest { name_hash: 1, colors: 1 })
            && !skin_manifest_ok(SkinManifest { name_hash: 1, colors: 17 }),
        "hash nonzero, colors 2..=16",
    );
    let mut slots = SkinSlotBoard::new();
    let c1 = slots.claim(3);
    let c2 = slots.claim(3);
    let scount = slots.count;
    set.add("F182 skin slot once", c1 && !c2 && scount == 1, "claimed slot stays taken");

    // F183 快捷键谱系学
    let chord_c = chord_make(1, 5);
    set.add("F183 chord encode", chord_c == 261 && chord_mods(chord_c) == 1 && chord_key(chord_c) == 5, "mods<<8|key");
    let mut ledger = ChordLedger::new();
    let r1 = ledger.register(chord_c);
    let r2 = ledger.register(chord_c);
    let r3 = ledger.register(chord_make(2, 5));
    let lcount = ledger.count;
    set.add("F183 chord conflict", r1 && !r2 && r3 && lcount == 2, "duplicate chord rejected");

    // F184 手势冲突仲裁庭
    set.add(
        "F184 arbiter priority",
        arbiter_pick(10, 3, 20, 5) == 20 && arbiter_pick(10, 5, 20, 3) == 10,
        "higher prio wins",
    );
    set.add("F184 arbiter tie", arbiter_pick(30, 4, 20, 4) == 20, "lower id wins tie");

    // F185 滚动物理学
    set.add(
        "F185 scroll decay",
        scroll_decay(1000, 250) == 750 && scroll_decay(100, 1000) == 0,
        "friction applied",
    );
    set.add(
        "F185 fling ticks",
        fling_ticks(0, 120) == 0 && fling_ticks(1000, 400) < fling_ticks(1000, 120),
        "stronger friction stops sooner",
    );

    // F186 惯性曲线库
    set.add(
        "F186 inertia curves",
        inertia_sample(InertiaCurve::Linear, 500) == 500
            && inertia_sample(InertiaCurve::EaseIn, 500) == 250
            && inertia_sample(InertiaCurve::EaseOut, 500) == 750,
        "midpoint splits 250/500/750",
    );
    set.add(
        "F186 inertia endpoints",
        inertia_sample(InertiaCurve::EaseIn, 1000) == 1000 && inertia_sample(InertiaCurve::EaseOut, 0) == 0,
        "curves anchored",
    );

    // F187 触屏 palm 优雅
    set.add(
        "F187 palm gate",
        touch_is_palm(800, 100) && !touch_is_palm(500, 100) && !touch_is_palm(800, 500),
        "big area + low pressure",
    );
    set.add("F187 palm filter", palm_filter(5, 2) == 3, "palms not counted");

    // F188 多点触控乐谱
    let mut board = MultiTouchBoard::new();
    let d1 = board.down(1);
    let d2 = board.down(2);
    let active_two = board.active();
    set.add("F188 touch bind", d1 == Some(0) && d2 == Some(1) && active_two == 2, "slots in order");
    let ghost = board.up(99);
    let released = board.up(1);
    let active_after = board.active();
    set.add("F188 touch release", !ghost && released && active_after == 1, "id keyed release");
    let mut full = MultiTouchBoard::new();
    let mut tid = 1u16;
    let mut filled = 0usize;
    while tid <= 10 {
        if full.down(tid).is_some() {
            filled += 1;
        }
        tid += 1;
    }
    let overflow = full.down(11);
    set.add("F188 touch board full", filled == 10 && overflow.is_none(), "10 slots cap");

    // F189 手写公式引擎
    set.add(
        "F189 glyph minus plus",
        glyph_classify(1, false, true) == HandwriteGlyph::Minus
            && glyph_classify(2, true, true) == HandwriteGlyph::Plus,
        "strokes and cross",
    );
    set.add(
        "F189 glyph equals unknown",
        glyph_classify(2, false, true) == HandwriteGlyph::Equals
            && glyph_classify(3, false, true) == HandwriteGlyph::Unknown
            && glyph_classify(2, true, false) == HandwriteGlyph::Unknown,
        "unaligned always unknown",
    );

    // F190 语音听写管线
    let mut session = DictationTally::default();
    let f1 = session.feed(800);
    let f2 = session.feed(600);
    let f3 = session.feed(700);
    let acc = session.accepted;
    let rej = session.rejected;
    set.add("F190 dictation gate", f1 && !f2 && f3 && acc == 2 && rej == 1, "conf floor 700");
    let rate = session.acceptance_permille();
    set.add("F190 dictation rate", rate == 666, "2/3 permille");

    // F191 眼动输入实验
    set.add(
        "F191 gaze select",
        gaze_select(600, 50) && !gaze_select(599, 10) && !gaze_select(1000, 60),
        "dwell + jitter gate",
    );
    set.add(
        "F191 gaze progress",
        gaze_progress(300, 600) == 500 && gaze_progress(1200, 600) == 1000,
        "progress capped",
    );

    // F192 脑机接口预留位
    let mut bci = BciLedger::new();
    let b1 = bci.claim(1);
    let b2 = bci.claim(1);
    let b3 = bci.claim(2);
    set.add(
        "F192 bci claim dedup",
        b1 == Some(0) && b2.is_none() && b3 == Some(1),
        "one channel per owner",
    );
    let mut bfull = BciLedger::new();
    let mut owner = 1u32;
    let mut got = 0usize;
    while owner <= 4 {
        if bfull.claim(owner).is_some() {
            got += 1;
        }
        owner += 1;
    }
    let boverflow = bfull.claim(9);
    set.add("F192 bci capacity", got == 4 && boverflow.is_none(), "4 reserved slots");
    set.add("F192 bci signal", bci_signal_ok(800) && !bci_signal_ok(600), "conf floor 700");

    // F193 输入回放调试
    let mut log = ReplayLedger::new();
    let l1 = log.record(1);
    let l2 = log.record(2);
    let l3 = log.record(2);
    let l4 = log.record(3);
    let lcount = log.count;
    set.add("F193 replay record", l1 && l2 && !l3 && l4 && lcount == 3, "strictly increasing");
    set.add(
        "F193 replay match",
        replay_log_matches(&log, &[1, 2, 3]) && !replay_log_matches(&log, &[1, 2, 4]),
        "end state compared",
    );

    // F194 手感金样本
    let golden = feel_golden_sample();
    set.add("F194 golden feel", feel_golden_matches(&golden), "scripted end state");
    let mut drifted = feel_golden_sample();
    drifted.in_rhythm = 7;
    set.add("F194 golden catches drift", !feel_golden_matches(&drifted), "mutation detected");

    // F195 键位迁移向导
    let mut migrator = KeymapMigrator::new();
    let m1 = migrator.remap(0, 5);
    let m2 = migrator.remap(0, 3);
    let m3 = migrator.remap(1, 5);
    let m4 = migrator.remap(1, 3);
    let prog = migrator.progress_permille();
    set.add(
        "F195 keymap migrate",
        m1 && !m2 && !m3 && m4 && prog == 250,
        "src once, dst unique, 2/8 done",
    );

    // F196 游戏直通模式
    set.add(
        "F196 passthrough budget",
        passthrough_budget_us(true, true) == 2000 && passthrough_budget_us(true, false) == 8000
            && passthrough_budget_us(false, true) == 8000,
        "game mode tightens to 2ms",
    );
    set.add("F196 passthrough active", passthrough_active(true, true) && !passthrough_active(false, true), "gate");

    // F197 输入隐私保护
    set.add(
        "F197 keystroke redact",
        redact_keystroke(FieldPrivacy::Password, b'a').is_none()
            && redact_keystroke(FieldPrivacy::Normal, b'a') == Some(b'a'),
        "password never logged",
    );
    let mut privacy = PrivacyFieldBoard::new();
    let p1 = privacy.mark(2);
    let p2 = privacy.mark(2);
    let pcount = privacy.count;
    set.add("F197 privacy field dedup", p1 && !p2 && pcount == 1, "field marked once");

    // F198 无障碍输入套装
    set.add(
        "F198 sticky keys",
        sticky_latch(1200, true) && !sticky_latch(800, true) && !sticky_latch(1200, false),
        "hold 1s while enabled",
    );
    set.add(
        "F198 slow keys",
        slow_key_accept(400, true) && !slow_key_accept(200, true) && slow_key_accept(200, false),
        "disabled passes through",
    );

    // F199 手感回归走廊
    set.add(
        "F199 corridor pass",
        corridor_within(1040, 1000) && corridor_within(910, 1000),
        "within +/-10%",
    );
    set.add(
        "F199 corridor reject",
        !corridor_within(1150, 1000) && !corridor_within(890, 1000),
        "outside tolerance",
    );

    // F200 输入年报
    set.add(
        "F200 input report",
        M600_INPUT_REPORT_SECTIONS.len() == 5 && m600_input_report_complete(5) && !m600_input_report_complete(4),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f176_pipeline_hops() {
        let hops = [
            PipelineHop { stage: 0, latency_us: 500 },
            PipelineHop { stage: 1, latency_us: 500 },
            PipelineHop { stage: 2, latency_us: 500 },
            PipelineHop { stage: 3, latency_us: 500 },
            PipelineHop { stage: 4, latency_us: 500 },
        ];
        assert!(pipeline_complete(&hops));
        // 总时延 2500us 远低于 8000us 预算
        let mut over = hops;
        over[0].latency_us = 7000;
        assert!(!pipeline_complete(&over));
    }

    #[test]
    fn f179_pen_pressure_mapping() {
        assert_eq!(pen_opacity(1000, 0), 1000);
        assert_eq!(pen_opacity(1000, 800), 800); // tilt/4 = 200 惩罚
        assert_eq!(pen_opacity(49, 0), 0);
        assert!(pen_is_palm(10));
        assert!(!pen_is_palm(100));
    }

    #[test]
    fn f180_rhythm_boundaries() {
        assert_eq!(rhythm_wpm(0, 60_000), 0);
        assert_eq!(rhythm_wpm(500, 0), 0); // 零窗口不除零
        assert_eq!(rhythm_wpm(80, 12_000), 80); // 80*60000/12000=400, /5=80
        assert!(rhythm_burst(80, 12_000));
    }

    #[test]
    fn f186_inertia_midpoints() {
        assert_eq!(inertia_sample(InertiaCurve::EaseIn, 1000), 1000);
        assert_eq!(inertia_sample(InertiaCurve::EaseOut, 1000), 1000);
        assert_eq!(inertia_sample(InertiaCurve::EaseIn, 250), 62); // 250*250/1000
        assert_eq!(inertia_sample(InertiaCurve::EaseOut, 250), 437); // 250*1750/1000
    }

    #[test]
    fn f191_gaze_dwell_progress() {
        assert!(!gaze_select(0, 0));
        assert!(gaze_select(2000, 0));
        assert_eq!(gaze_progress(0, 600), 0);
        assert_eq!(gaze_progress(600, 600), 1000);
    }

    #[test]
    fn f195_migrator_progress() {
        let mut m = KeymapMigrator::new();
        assert!(m.remap(7, 7));
        assert_eq!(m.progress_permille(), 125);
        assert!(!m.remap(7, 7)); // 源位去重
        assert!(!m.remap(3, 7)); // 目标冲突
    }

    #[test]
    fn m600input_selfcheck_all_pass() {
        let set = run_m600input_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
