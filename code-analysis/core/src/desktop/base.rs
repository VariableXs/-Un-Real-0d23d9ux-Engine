//! UNREAL-X：AI-11 / AI-12 桌面域公共底座（族0109~0112 · X02701~X02800）。
//!
//! 落点 `code-analysis/core/src/desktop/`。零 AI：全部确定性算法，无外部依赖。
//! 底座只提供「五层 × 五档」模板里跨族复用的钳制 / 叙事 / 令牌 / 快照 / 守卫件，
//! 各族在自己的文件里实现本族真实算法，不靠底座凑项。

/// 档位矩阵档数（「≥5 档独立可交付」口径）。
pub const LEVELS: u8 = 5;

/// 默认帧预算 16ms（60fps）；极端输入一律回落到它。
pub const DEFAULT_FRAME_MS: u32 = 16;
/// 帧预算上限 10s，超出视为非法输入。
pub const MAX_FRAME_MS: u32 = 10_000;

pub fn clamp_level(level: u8) -> u8 {
    if level >= LEVELS {
        LEVELS - 1
    } else {
        level
    }
}

/// 非法/极端输入钳制：0 或超上限回默认，不崩溃。
pub fn clamp_ms(ms: u32) -> u32 {
    if ms == 0 || ms > MAX_FRAME_MS {
        DEFAULT_FRAME_MS
    } else {
        ms
    }
}

/// 越界回默认 + 给出可读原因（L2 档1 口径）。
pub fn clamp_ms_reason(ms: u32) -> (u32, bool) {
    let ok = ms != 0 && ms <= MAX_FRAME_MS;
    (clamp_ms(ms), ok)
}

// ---------------------------------------------------------------------------
// 失败叙事与错误码（禁裸报错：每种失败都有下一步建议）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeskError {
    Ok = 0,
    OutOfRange = 1,
    Disabled = 2,
    NoMemory = 3,
    Busy = 4,
    Corrupt = 5,
    NotFound = 6,
    Denied = 7,
}

impl DeskError {
    pub fn code(self) -> u32 {
        self as u32
    }

    pub fn advice(self) -> &'static str {
        match self {
            DeskError::Ok => "无需处理",
            DeskError::OutOfRange => "参数超出可用范围，已回落默认档，可在设置页重新选择",
            DeskError::Disabled => "服务未启用，请在设置页打开开关后重试",
            DeskError::NoMemory => "内存紧张，请关闭部分桌面特效或提高缓存配额",
            DeskError::Busy => "上一批任务仍在执行，可等待进度结束或取消后重试",
            DeskError::Corrupt => "配置数据损坏，已载入最近一次快照，可在快照页回滚",
            DeskError::NotFound => "目标不存在，可能已被卸载，请刷新列表",
            DeskError::Denied => "权限不足，请授予桌面服务权限后重试",
        }
    }

    pub fn ok(self) -> bool {
        self == DeskError::Ok
    }
}

/// `E07 · 权限不足，请授予…` 形态的叙事串。
pub fn error_narrative(e: DeskError) -> String {
    format!("E{:02} · {}", e.code(), e.advice())
}

// ---------------------------------------------------------------------------
// 动效令牌 / 三态 / 键盘 / 微文案 / 无障碍（L3 手感与细节）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Motion {
    pub curve: u8,
    pub dur_ms: u16,
    pub scale_milli: u16,
}

pub const MOTION_STD: Motion = Motion {
    curve: 2,
    dur_ms: 220,
    scale_milli: 1000,
};

/// reduce-motion 下统一降级为纯淡入淡出（curve 0）。
pub fn motion_for(level: u8, reduce: bool) -> Motion {
    let l = clamp_level(level);
    if reduce {
        return Motion {
            curve: 0,
            dur_ms: 120,
            scale_milli: 1000,
        };
    }
    Motion {
        curve: l,
        dur_ms: 120 + 40 * l as u16,
        scale_milli: 1000 + 20 * l as u16,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeskState {
    Idle,
    Hover,
    Press,
    Disabled,
}

/// 三态 + 焦点环：hover 抬升、press 下压、disabled 无环。
pub fn focus_ring(state: DeskState) -> u8 {
    match state {
        DeskState::Idle => 0,
        DeskState::Hover => 1,
        DeskState::Press => 2,
        DeskState::Disabled => 0,
    }
}

pub fn elevation(state: DeskState) -> u8 {
    match state {
        DeskState::Idle => 1,
        DeskState::Hover => 2,
        DeskState::Press => 0,
        DeskState::Disabled => 0,
    }
}

/// 快捷键冲突检测：忽略大小写与空格（`Ctrl+Shift+K` == `ctrl + shift + k`）。
pub fn hotkey_conflict(a: &str, b: &str) -> bool {
    let norm = |s: &str| -> String {
        s.split('+')
            .map(|p| p.trim().to_ascii_lowercase())
            .filter(|p| !p.is_empty())
            .collect::<Vec<String>>()
            .join("+")
    };
    norm(a) == norm(b)
}

/// 微文案过检：中文语境自然（不吐英文原始错误词）、长度克制（≤24 字）。
pub fn microcopy_ok(s: &str) -> bool {
    let n = s.chars().count();
    n > 0 && n <= 24 && !s.starts_with("Error") && !s.contains("undefined") && !s.contains("null")
}

/// 简化 WCAG 对比度 ×100：亮度传入 0..1000 千分比。
pub fn contrast_x100(l1: u32, l2: u32) -> u32 {
    let (a, b) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    ((a + 50) * 100) / (b + 50)
}

/// 无障碍 HC 红线：7:1。
pub fn hc_redline(l1: u32, l2: u32) -> bool {
    contrast_x100(l1, l2) >= 700
}

// ---------------------------------------------------------------------------
// 快照 / 迁移 / 降级链（L1 档4 + L4 档4）
// ---------------------------------------------------------------------------

pub const SNAP_VER: u16 = 2;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snap {
    pub ver: u16,
    pub payload: [u8; 8],
}

fn hex(c: u8) -> char {
    if c < 10 {
        (b'0' + c) as char
    } else {
        (b'a' + c - 10) as char
    }
}

fn unhex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

pub fn export_snap(s: Snap) -> String {
    let mut out = format!("VS{}:", s.ver);
    for b in s.payload.iter() {
        out.push(hex(b >> 4));
        out.push(hex(b & 0xf));
    }
    out
}

pub fn import_snap(text: &str) -> Option<Snap> {
    let body = text.strip_prefix("VS")?;
    let mut it = body.splitn(2, ':');
    let ver: u16 = it.next()?.parse().ok()?;
    let digits = it.next()?;
    if digits.len() != 16 {
        return None;
    }
    let raw = digits.as_bytes();
    let mut payload = [0u8; 8];
    for (i, slot) in payload.iter_mut().enumerate() {
        let hi = unhex(raw[i * 2])?;
        let lo = unhex(raw[i * 2 + 1])?;
        *slot = (hi << 4) | lo;
    }
    Some(Snap { ver, payload })
}

/// 跨版本携带：只升不降，降版迁移直接失败。
pub fn migrate(text: &str, to: u16) -> Option<Snap> {
    let mut s = import_snap(text)?;
    if s.ver > to {
        return None;
    }
    s.ver = to;
    Some(s)
}

/// 低配设备自动降级链：返回（材质档, 动效档, 精度档），压力越大递降越狠。
pub fn degrade_chain(level: u8, pressure: u8) -> (u8, u8, u8) {
    let l = clamp_level(level);
    if pressure >= 200 {
        (0, 0, 0)
    } else if pressure >= 120 {
        (l / 2, 0, l / 2)
    } else if pressure >= 60 {
        let m = if l == 0 { 0 } else { l - 1 };
        (m, m, l)
    } else {
        (l, l, l)
    }
}

/// CPU/内存/电量紧张时的资源降级策略：返回是否触发守护。
pub fn resource_guard(cpu: u8, mem: u8, battery: u8) -> (bool, DeskError) {
    if cpu >= 220 || mem >= 220 {
        (true, DeskError::NoMemory)
    } else if battery <= 5 {
        (true, DeskError::Busy)
    } else {
        (false, DeskError::Ok)
    }
}

// ---------------------------------------------------------------------------
// 守卫注册表 / 扩展点 / 彩蛋（L4 档5 + L5 档4/档5，均自带去重）
// ---------------------------------------------------------------------------

/// 防劣化回归守卫：断言只增不删。
pub struct Guard {
    pub rules: Vec<String>,
}

impl Guard {
    pub fn new() -> Self {
        Guard { rules: Vec::new() }
    }
    pub fn guard(&mut self, rule: &str) -> bool {
        if self.rules.iter().any(|r| r == rule) {
            false
        } else {
            self.rules.push(rule.to_string());
            true
        }
    }
    pub fn count(&self) -> usize {
        self.rules.len()
    }
    pub fn has(&self, rule: &str) -> bool {
        self.rules.iter().any(|r| r == rule)
    }
}

/// 面向创作者/开发者的扩展点：注册去重，卸载净身。
pub struct Plugins {
    pub ids: Vec<String>,
}

impl Plugins {
    pub fn new() -> Self {
        Plugins { ids: Vec::new() }
    }
    pub fn register(&mut self, id: &str) -> bool {
        if self.ids.iter().any(|x| x == id) {
            false
        } else {
            self.ids.push(id.to_string());
            true
        }
    }
    pub fn unregister(&mut self, id: &str) -> bool {
        let before = self.ids.len();
        self.ids.retain(|x| x != id);
        self.ids.len() != before
    }
    pub fn count(&self) -> usize {
        self.ids.len()
    }
}

/// 彩蛋层：可关闭、不损主线体验。
pub struct Eggs {
    pub armed: Vec<String>,
    pub master_off: bool,
}

impl Eggs {
    pub fn new() -> Self {
        Eggs {
            armed: Vec::new(),
            master_off: false,
        }
    }
    pub fn arm(&mut self, id: &str) -> bool {
        if self.master_off || self.armed.iter().any(|x| x == id) {
            false
        } else {
            self.armed.push(id.to_string());
            true
        }
    }
    pub fn is_armed(&self, id: &str) -> bool {
        !self.master_off && self.armed.iter().any(|x| x == id)
    }
    pub fn disable_all(&mut self) {
        self.master_off = true;
    }
    pub fn count(&self) -> usize {
        if self.master_off {
            0
        } else {
            self.armed.len()
        }
    }
}

// ---------------------------------------------------------------------------
// 批量队列 / 本地启发式建议（L5 档1/档2）
// ---------------------------------------------------------------------------

pub struct Batch {
    pub total: usize,
    pub done: usize,
}

impl Batch {
    pub fn new(total: usize) -> Self {
        Batch { total, done: 0 }
    }
    pub fn step(&mut self) -> bool {
        if self.done >= self.total {
            false
        } else {
            self.done += 1;
            true
        }
    }
    /// 进度可观测：0~100。
    pub fn progress(&self) -> u32 {
        if self.total == 0 {
            100
        } else {
            ((self.done * 100) / self.total) as u32
        }
    }
}

pub struct Advisor {
    pub advices: Vec<(String, String, bool)>,
}

impl Advisor {
    pub fn new() -> Self {
        Advisor {
            advices: Vec::new(),
        }
    }
    /// 建议去重：同 id 不重复登记。
    pub fn suggest(&mut self, id: &str, reason: &str) -> bool {
        if self.advices.iter().any(|(i, _, _)| i == id) {
            false
        } else {
            self.advices.push((id.to_string(), reason.to_string(), false));
            true
        }
    }
    /// 可解释。
    pub fn explain(&self, id: &str) -> Option<&str> {
        self.advices
            .iter()
            .find(|(i, _, _)| i == id)
            .map(|(_, r, _)| r.as_str())
    }
    /// 一键拒绝。
    pub fn reject(&mut self, id: &str) -> bool {
        match self.advices.iter_mut().find(|(i, _, _)| i == id) {
            Some(a) => {
                a.2 = true;
                true
            }
            None => false,
        }
    }
    pub fn rejected(&self, id: &str) -> bool {
        self.advices
            .iter()
            .find(|(i, _, _)| i == id)
            .map(|(_, _, r)| *r)
            .unwrap_or(false)
    }
    pub fn count(&self) -> usize {
        self.advices.len()
    }
}

/// 三线跨域联动：返回（内核, Variable 系统, 代码分析）参与标记。
pub fn link_matrix(kind: u8) -> (bool, bool, bool) {
    match kind % 5 {
        0 => (true, false, false),
        1 => (false, true, false),
        2 => (false, false, true),
        3 => (true, true, false),
        _ => (true, true, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_clamp_and_narrative() {
        assert_eq!(clamp_level(9), 4);
        assert_eq!(clamp_ms(0), DEFAULT_FRAME_MS);
        assert!(!clamp_ms_reason(99_999).1);
        assert!(error_narrative(DeskError::Denied).starts_with("E07 · "));
        assert!(DeskError::Ok.ok());
    }

    #[test]
    fn base_snapshot_roundtrip() {
        let s = Snap {
            ver: 1,
            payload: [1, 2, 3, 4, 5, 6, 7, 8],
        };
        let text = export_snap(s);
        let back = import_snap(&text).unwrap();
        assert_eq!(back, s);
        assert_eq!(migrate(&text, SNAP_VER).unwrap().ver, SNAP_VER);
        assert!(migrate(&export_snap(Snap { ver: 3, payload: s.payload }), 2).is_none());
        assert!(import_snap("garbage").is_none());
    }

    #[test]
    fn base_registries_dedupe() {
        let mut g = Guard::new();
        assert!(g.guard("r1"));
        assert!(!g.guard("r1"));
        let mut p = Plugins::new();
        assert!(p.register("x"));
        assert!(!p.register("x"));
        assert!(p.unregister("x"));
        let mut e = Eggs::new();
        assert!(e.arm("egg"));
        assert!(!e.arm("egg"));
        e.disable_all();
        assert_eq!(e.count(), 0);
    }
}
