//! AI-11 内核桌面服务公共底座（族0101~0110 · X02501~X02750）。
//!
//! 硬约束：`no_std` / 无 alloc / 无浮点（全部 permille 或定点整数）/ 纯逻辑。
//! 底座只提供「五层 × 五档」模板里跨族复用的钳制 / 错误叙事 / 动效令牌 /
//! 三态 / 键盘 / 微文案 / 对比度 / 快照 / 降级链 / 守卫 / 扩展点 / 彩蛋件，
//! 各族在自己的文件里实现本族真实算法。

/// 档位矩阵档数（「≥5 档独立可交付」口径）。
pub const LEVELS: u8 = 5;
/// 默认帧预算 16ms（60fps）。
pub const DEFAULT_FRAME_MS: u32 = 16;
/// 帧预算上限 10s。
pub const MAX_FRAME_MS: u32 = 10_000;

pub fn clamp_level(level: u8) -> u8 {
    if level >= LEVELS {
        LEVELS - 1
    } else {
        level
    }
}

pub fn clamp_ms(ms: u32) -> u32 {
    if ms == 0 || ms > MAX_FRAME_MS {
        DEFAULT_FRAME_MS
    } else {
        ms
    }
}

/// 越界回默认并给出是否合法的判定（L2 档1）。
pub fn clamp_ms_reason(ms: u32) -> (u32, bool) {
    (clamp_ms(ms), ms != 0 && ms <= MAX_FRAME_MS)
}

/// 百分比钳制（permille 之外的高频小比例也走这里）。
pub fn clamp_pct(p: u32) -> u32 {
    if p > 100 {
        100
    } else {
        p
    }
}

// ---------------------------------------------------------------------------
// 错误码与失败叙事（禁裸报错：每种失败都有下一步建议）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
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
    pub fn ok(self) -> bool {
        self == DeskError::Ok
    }
    pub fn advice(self) -> &'static str {
        match self {
            DeskError::Ok => "无需处理",
            DeskError::OutOfRange => "参数超出可用范围，已回落默认档，可在设置页重选",
            DeskError::Disabled => "服务未启用，请在设置页打开开关后重试",
            DeskError::NoMemory => "内存紧张，请关闭部分桌面特效或提高配额",
            DeskError::Busy => "上一批任务仍在执行，可等待结束或取消后重试",
            DeskError::Corrupt => "配置数据损坏，已载入最近一次快照，可回滚",
            DeskError::NotFound => "目标不存在，可能已被卸载，请刷新列表",
            DeskError::Denied => "权限不足，请授予桌面服务权限后重试",
        }
    }
}

/// 叙事串缓冲长度（UTF-8 字节）。
pub const NARR_LEN: usize = 64;

/// 生成 `E07 · 权限不足…` 形态的叙事串，返回写入字节数。
pub fn narrative(e: DeskError, buf: &mut [u8; NARR_LEN]) -> usize {
    let code = e.code();
    let mut n = 0usize;
    let put = |buf: &mut [u8; NARR_LEN], n: &mut usize, b: u8| {
        if *n < NARR_LEN {
            buf[*n] = b;
            *n += 1;
        }
    };
    put(buf, &mut n, b'E');
    put(buf, &mut n, b'0' + ((code / 10) % 10) as u8);
    put(buf, &mut n, b'0' + (code % 10) as u8);
    for b in " · ".as_bytes() {
        put(buf, &mut n, *b);
    }
    for b in e.advice().as_bytes() {
        put(buf, &mut n, *b);
    }
    n
}

/// 叙事串是否以指定前缀开头（避免测试里重建字符串）。
pub fn narrative_has(e: DeskError, needle: &str) -> bool {
    let mut buf = [0u8; NARR_LEN];
    let n = narrative(e, &mut buf);
    contains_bytes(&buf[..n], needle.as_bytes())
}

pub fn contains_bytes(hay: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    let mut i = 0usize;
    while i + needle.len() <= hay.len() {
        if &hay[i..i + needle.len()] == needle {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 动效令牌 / 三态 / 键盘 / 微文案 / 对比度（L3 手感与细节）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DeskState {
    Idle,
    Hover,
    Press,
    Disabled,
}

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

/// 快捷键归一化缓冲。
pub const HOTKEY_LEN: usize = 24;

pub fn norm_hotkey(src: &str, out: &mut [u8; HOTKEY_LEN]) -> usize {
    let mut n = 0usize;
    let mut pending = false;
    for b in src.as_bytes() {
        let c = *b;
        if c == b' ' {
            continue;
        }
        if c == b'+' {
            if n < HOTKEY_LEN {
                out[n] = b'+';
                n += 1;
            }
            pending = false;
            continue;
        }
        if pending && n < HOTKEY_LEN {
            out[n] = b'+';
            n += 1;
        }
        if n < HOTKEY_LEN {
            out[n] = if c >= b'A' && c <= b'Z' { c + 32 } else { c };
            n += 1;
        }
        pending = true;
    }
    n
}

pub fn hotkey_conflict(a: &str, b: &str) -> bool {
    let mut x = [0u8; HOTKEY_LEN];
    let mut y = [0u8; HOTKEY_LEN];
    let nx = norm_hotkey(a, &mut x);
    let ny = norm_hotkey(b, &mut y);
    nx == ny && nx > 0 && x[..nx] == y[..ny]
}

/// 微文案过检：不吐英文原始错误词、长度克制（≤24 字节）。
pub fn microcopy_ok(s: &str) -> bool {
    let b = s.as_bytes();
    let n = b.len();
    n > 0
        && n <= 24
        && !contains_bytes(b, "Error".as_bytes())
        && !contains_bytes(b, "undefined".as_bytes())
        && !contains_bytes(b, "null".as_bytes())
}

/// 简化 WCAG 对比度 ×100（亮度 0..1000 千分比）。
pub fn contrast_x100(l1: u32, l2: u32) -> u32 {
    let (a, b) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    ((a + 50) * 100) / (b + 50)
}

/// 无障碍 HC 红线 7:1。
pub fn hc_redline(l1: u32, l2: u32) -> bool {
    contrast_x100(l1, l2) >= 700
}

// ---------------------------------------------------------------------------
// 快照 / 迁移 / 降级链（L1 档4 + L4 档4）
// ---------------------------------------------------------------------------

pub const SNAP_VER: u16 = 2;
pub const SNAP_TEXT: usize = 24;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Snap {
    pub ver: u16,
    pub payload: [u8; 8],
}

fn hex(c: u8) -> u8 {
    if c < 10 {
        b'0' + c
    } else {
        b'a' + c - 10
    }
}

fn unhex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    }
}

/// 导出为 `VS2:xxxxxxxxxxxxxxxx` 形态。
pub fn export_snap(s: Snap, out: &mut [u8; SNAP_TEXT]) -> usize {
    let mut n = 0usize;
    let put = |out: &mut [u8; SNAP_TEXT], n: &mut usize, b: u8| {
        if *n < SNAP_TEXT {
            out[*n] = b;
            *n += 1;
        }
    };
    put(out, &mut n, b'V');
    put(out, &mut n, b'S');
    let v = s.ver;
    if v >= 10 {
        put(out, &mut n, b'0' + ((v / 10) % 10) as u8);
    }
    put(out, &mut n, b'0' + (v % 10) as u8);
    put(out, &mut n, b':');
    for b in s.payload.iter() {
        put(out, &mut n, hex(b >> 4));
        put(out, &mut n, hex(b & 0xf));
    }
    n
}

pub fn import_snap(text: &[u8]) -> Option<Snap> {
    if text.len() < 5 || text[0] != b'V' || text[1] != b'S' {
        return None;
    }
    let mut i = 2usize;
    let mut ver: u16 = 0;
    while i < text.len() && text[i] != b':' {
        let d = unhex(text[i])?;
        ver = ver * 10 + d as u16;
        i += 1;
    }
    if i >= text.len() || text[i] != b':' {
        return None;
    }
    i += 1;
    if text.len() - i != 16 {
        return None;
    }
    let mut payload = [0u8; 8];
    for (k, slot) in payload.iter_mut().enumerate() {
        let hi = unhex(text[i + k * 2])?;
        let lo = unhex(text[i + k * 2 + 1])?;
        *slot = (hi << 4) | lo;
    }
    Some(Snap { ver, payload })
}

/// 跨版本携带：只升不降。
pub fn migrate(text: &[u8], to: u16) -> Option<Snap> {
    let mut s = import_snap(text)?;
    if s.ver > to {
        return None;
    }
    s.ver = to;
    Some(s)
}

/// 低配降级链：返回（材质档, 动效档, 精度档）。
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

/// CPU/内存/电量紧张时的资源守护。
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
// 守卫 / 扩展点 / 彩蛋 / 建议 / 批量（L4 档5 + L5）
// ---------------------------------------------------------------------------

pub const REG_CAP: usize = 16;

/// 防劣化回归守卫：断言只增不删。
pub struct Guard {
    rules: [Option<&'static str>; REG_CAP],
    n: usize,
}

impl Guard {
    pub const fn new() -> Self {
        Guard {
            rules: [None; REG_CAP],
            n: 0,
        }
    }
    pub fn guard(&mut self, rule: &'static str) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if self.rules[i] == Some(rule) {
                return false;
            }
            i += 1;
        }
        if self.n >= REG_CAP {
            return false;
        }
        self.rules[self.n] = Some(rule);
        self.n += 1;
        true
    }
    pub fn has(&self, rule: &str) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if self.rules[i] == Some(rule) {
                return true;
            }
            i += 1;
        }
        false
    }
    pub fn count(&self) -> usize {
        self.n
    }
}

/// 开放扩展点：注册去重，卸载净身。
pub struct Plugins {
    ids: [Option<&'static str>; REG_CAP],
    n: usize,
}

impl Plugins {
    pub const fn new() -> Self {
        Plugins {
            ids: [None; REG_CAP],
            n: 0,
        }
    }
    pub fn register(&mut self, id: &'static str) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if self.ids[i] == Some(id) {
                return false;
            }
            i += 1;
        }
        if self.n >= REG_CAP {
            return false;
        }
        self.ids[self.n] = Some(id);
        self.n += 1;
        true
    }
    pub fn unregister(&mut self, id: &str) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if self.ids[i] == Some(id) {
                let mut j = i;
                while j + 1 < self.n {
                    self.ids[j] = self.ids[j + 1];
                    j += 1;
                }
                self.ids[j] = None;
                self.n -= 1;
                return true;
            }
            i += 1;
        }
        false
    }
    pub fn has(&self, id: &str) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if self.ids[i] == Some(id) {
                return true;
            }
            i += 1;
        }
        false
    }
    pub fn count(&self) -> usize {
        self.n
    }
}

/// 彩蛋层：可关闭、不损主线。
pub struct Eggs {
    armed: [Option<&'static str>; REG_CAP],
    n: usize,
    pub master_off: bool,
}

impl Eggs {
    pub const fn new() -> Self {
        Eggs {
            armed: [None; REG_CAP],
            n: 0,
            master_off: false,
        }
    }
    pub fn arm(&mut self, id: &'static str) -> bool {
        if self.master_off || self.is_armed(id) {
            return false;
        }
        if self.n >= REG_CAP {
            return false;
        }
        self.armed[self.n] = Some(id);
        self.n += 1;
        true
    }
    pub fn is_armed(&self, id: &str) -> bool {
        if self.master_off {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            if self.armed[i] == Some(id) {
                return true;
            }
            i += 1;
        }
        false
    }
    pub fn disable_all(&mut self) {
        self.master_off = true;
    }
    pub fn count(&self) -> usize {
        if self.master_off {
            0
        } else {
            self.n
        }
    }
}

/// 本地启发式建议：可解释、可一键拒绝、登记去重。
pub struct Advisor {
    items: [Option<(&'static str, &'static str, bool)>; REG_CAP],
    n: usize,
}

impl Advisor {
    pub const fn new() -> Self {
        Advisor {
            items: [None; REG_CAP],
            n: 0,
        }
    }
    pub fn suggest(&mut self, id: &'static str, reason: &'static str) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some((k, _, _)) = self.items[i] {
                if k == id {
                    return false;
                }
            }
            i += 1;
        }
        if self.n >= REG_CAP {
            return false;
        }
        self.items[self.n] = Some((id, reason, false));
        self.n += 1;
        true
    }
    pub fn explain(&self, id: &str) -> Option<&'static str> {
        let mut i = 0usize;
        while i < self.n {
            if let Some((k, r, _)) = self.items[i] {
                if k == id {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }
    pub fn reject(&mut self, id: &str) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some((k, _, _)) = self.items[i] {
                if k == id {
                    self.items[i] = Some((k, self.items[i].unwrap().1, true));
                    return true;
                }
            }
            i += 1;
        }
        false
    }
    pub fn rejected(&self, id: &str) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some((k, _, r)) = self.items[i] {
                if k == id {
                    return r;
                }
            }
            i += 1;
        }
        false
    }
    pub fn count(&self) -> usize {
        self.n
    }
}

/// 批量/自动化队列：进度可观测。
pub struct Batch {
    pub total: u32,
    pub done: u32,
}

impl Batch {
    pub const fn new(total: u32) -> Self {
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
    pub fn progress(&self) -> u32 {
        if self.total == 0 {
            100
        } else {
            (self.done * 100) / self.total
        }
    }
}

/// 三线跨域联动：（内核, Variable 系统, 代码分析）。
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
    fn svc_clamp_and_narrative() {
        assert_eq!(clamp_level(9), 4);
        assert_eq!(clamp_ms(0), DEFAULT_FRAME_MS);
        assert!(!clamp_ms_reason(99_999).1);
        assert!(narrative_has(DeskError::Denied, "权限不足"));
        assert!(DeskError::Ok.ok());
    }

    #[test]
    fn svc_snapshot_roundtrip() {
        let s = Snap {
            ver: 1,
            payload: [1, 2, 3, 4, 5, 6, 7, 8],
        };
        let mut buf = [0u8; SNAP_TEXT];
        let n = export_snap(s, &mut buf);
        let back = import_snap(&buf[..n]).unwrap();
        assert_eq!(back.ver, 1);
        assert_eq!(back.payload, s.payload);
        assert_eq!(migrate(&buf[..n], SNAP_VER).unwrap().ver, SNAP_VER);
        assert!(import_snap("nope".as_bytes()).is_none());
    }

    #[test]
    fn svc_registries_dedupe() {
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

    #[test]
    fn svc_hotkey_and_microcopy() {
        assert!(hotkey_conflict("Ctrl+Shift+K", "ctrl + shift + k"));
        assert!(!hotkey_conflict("Ctrl+K", "Ctrl+Shift+K"));
        assert!(microcopy_ok("渲染管线已就绪"));
        assert!(!microcopy_ok("Error: undefined"));
    }
}
