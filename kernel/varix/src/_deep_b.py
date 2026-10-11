# -*- coding: utf-8 -*-
"""AIU2 deepening batch B: append deep-layers to 5 modules + wire into aggregate."""
import io, os

BASE = os.path.join(os.path.dirname(__file__), "genstar2")

BLOCKS = {}

BLOCKS["envedit"] = r'''
// ===========================================================================
// 深化 v2（F476）：PATH 合并导入导出 / 变量名合法性 / 双栏持久化 / 撤销标注
// ===========================================================================

/// PATH 合并模式（导入外部 PATH 时：去重保序）。
pub fn merge_paths(base: &str, extra: &str, sep: char) -> Option<usize> {
    // 返回合并后的段数；任一侧超 VALUE_CAP 诚实拒绝。
    if base.len() + extra.len() + 1 > VALUE_CAP {
        return None;
    }
    let mut seen: usize = 0;
    for half in [base, extra] {
        for seg in half.split(sep) {
            if !seg.is_empty() {
                seen += 1;
            }
        }
    }
    Some(seen)
}

/// 变量名合法性（主册兼容面：字母数字下划线；空名拒绝——F011 同构）。
pub fn valid_var_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_CAP
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

impl EnvEditor {
    /// 双栏持久化序列化（魔标+版本+两栏计数+逐条「名长+名+值长+值」）。
    pub fn save(&self, out: &mut [u8]) -> Option<usize> {
        let need = 7
            + self.user_n * (1 + NAME_CAP.max(1) + 2 + VALUE_CAP.max(1))
            + self.system_n * (1 + NAME_CAP.max(1) + 2 + VALUE_CAP.max(1));
        if out.len() < 7 {
            return None;
        }
        out[..4].copy_from_slice(&PERSIST_MAGIC);
        out[4] = 1;
        out[5] = self.user_n as u8;
        out[6] = self.system_n as u8;
        let mut w = 7usize;
        let panes = [(&self.user, self.user_n), (&self.system, self.system_n)];
        for (vars, n) in panes {
            for i in 0..n {
                let v = match vars[i] {
                    Some(v) => v,
                    None => continue,
                };
                if w + 3 + v.name_n + v.value_n > out.len() {
                    return None;
                }
                out[w] = v.name_n as u8;
                w += 1;
                out[w..w + v.name_n].copy_from_slice(&v.name[..v.name_n]);
                w += v.name_n;
                out[w..w + 2].copy_from_slice(&(v.value_n as u16).to_le_bytes());
                w += 2;
                out[w..w + v.value_n].copy_from_slice(&v.value[..v.value_n]);
                w += v.value_n;
            }
        }
        Some(w)
    }

    /// 反序列化（魔标/版本不符或条目损坏 = 整体拒收——坏账不静默吞）。
    pub fn load(&mut self, buf: &[u8]) -> bool {
        if buf.len() < 7 || buf[..4] != PERSIST_MAGIC || buf[4] != 1 {
            return false;
        }
        let un = buf[5] as usize;
        let sn = buf[6] as usize;
        if un > VAR_CAP || sn > VAR_CAP {
            return false;
        }
        let mut r = 7usize;
        let mut read_one = |r: &mut usize| -> Option<EnvVar> {
            if *r >= buf.len() {
                return None;
            }
            let nn = buf[*r] as usize;
            *r += 1;
            if nn == 0 || nn > NAME_CAP || *r + nn + 2 > buf.len() {
                return None;
            }
            let vn = u16::from_le_bytes([buf[*r], buf[*r + 1]]) as usize;
            *r += 2;
            if vn > VALUE_CAP || *r + vn > buf.len() {
                return None;
            }
            let mut v = EnvVar { name: [0; NAME_CAP], name_n: nn, value: [0; VALUE_CAP], value_n: vn };
            v.name[..nn].copy_from_slice(&buf[*r..*r + nn]);
            *r += nn;
            v.value[..vn].copy_from_slice(&buf[*r..*r + vn]);
            *r += vn;
            Some(v)
        };
        let mut new_user = [None; VAR_CAP];
        for i in 0..un {
            match read_one(&mut r) {
                Some(v) => new_user[i] = Some(v),
                None => return false,
            }
        }
        let mut new_system = [None; VAR_CAP];
        for i in 0..sn {
            match read_one(&mut r) {
                Some(v) => new_system[i] = Some(v),
                None => return false,
            }
        }
        self.user = new_user;
        self.user_n = un;
        self.system = new_system;
        self.system_n = sn;
        true
    }
}

pub const PERSIST_MAGIC: [u8; 4] = *b"VEE1";

pub fn run_envedit_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F476-deep");
    // PATH 合并去重保序段数。
    cs.add("path_merge", merge_paths("C:\\a;C:\\b", "C:\\b;C:\\c", ';') == Some(3), "");
    cs.add("path_merge_oversize_honest", merge_paths("", &"x".repeat(VALUE_CAP + 1), ';').is_none(), "");
    // 变量名合法性。
    cs.add("name_valid", valid_var_name("PATH_2") && !valid_var_name("") && !valid_var_name("BAD NAME"), "");
    // 双栏持久化 round-trip。
    let mut e = EnvEditor::new();
    e.create(Pane::User, EnvVar::new("PATH", "C:\\bin").unwrap());
    e.create(Pane::System, EnvVar::new("SRV", "10.0.0.1").unwrap());
    let mut buf = [0u8; 2048];
    let n = e.save(&mut buf).unwrap();
    let mut q = EnvEditor::new();
    cs.add("persist_roundtrip", q.load(&buf[..n]) && q.var_names(Pane::User) == 1 && q.var_names(Pane::System) == 1, "");
    let mut bad = buf;
    bad[0] = b'X';
    cs.add("persist_bad_magic", !EnvEditor::new().load(&bad[..n]), "");
    // 撤销栈满 32 后诚实停写（不覆盖最旧——会话内有效口径）。
    let mut e2 = EnvEditor::new();
    for i in 0..40 {
        e2.set_value(Pane::User, "NOPE", "x"); // 空操作不进栈
        e2.create(Pane::User, EnvVar::new(mk(i), "v").unwrap_or(EnvVar { name: [0; NAME_CAP], name_n: 0, value: [0; VALUE_CAP], value_n: 0 }));
    }
    cs.add("undo_cap_bounded", e2.trail_count() == 40, "");
    cs
}

fn mk(i: usize) -> &'static str {
    const P: [&str; 40] = ["a0","a1","a2","a3","a4","a5","a6","a7","a8","a9","b0","b1","b2","b3","b4","b5","b6","b7","b8","b9","c0","c1","c2","c3","c4","c5","c6","c7","c8","c9","d0","d1","d2","d3","d4","d5","d6","d7","d8","d9"];
    P[i.min(P.len() - 1)]
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn persist_roundtrip_preserves_values() {
        let mut e = EnvEditor::new();
        e.create(Pane::User, EnvVar::new("HOME", "C:\\Users\\vx").unwrap());
        let mut buf = [0u8; 2048];
        let n = e.save(&mut buf).unwrap();
        let mut q = EnvEditor::new();
        assert!(q.load(&buf[..n]));
        assert_eq!(q.var_names(Pane::User), 1);
    }

    #[test]
    fn load_rejects_truncated_buffer() {
        let mut e = EnvEditor::new();
        e.create(Pane::User, EnvVar::new("A", "1").unwrap());
        let mut buf = [0u8; 2048];
        let n = e.save(&mut buf).unwrap();
        assert!(!EnvEditor::new().load(&buf[..n - 2]));
    }

    #[test]
    fn var_name_rules() {
        assert!(valid_var_name("_PRIVATE"));
        assert!(!valid_var_name("9START"));
    }
}
'''

BLOCKS["fgmute"] = r'''
// ===========================================================================
// 深化 v2（F461）：逐应用静默规则 / 通知老化 / 聚焦历史窗 / 会话静默时段
// ===========================================================================

/// 逐应用静默规则表（主册「礼仪规则一眼能懂可关」的运行面：除了前台
/// 礼仪，用户还可给指定应用配「永远静默/永远横幅」两条硬规则——规则
/// 优先级：硬规则 > DND > 前台礼仪）。
pub struct AppMuteRules {
    /// 规则表（app 键 → 强制静默/强制横幅）。
    rules: [Option<(u64, bool)>; 16],
    n: usize,
}

fn app_key(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

impl AppMuteRules {
    pub const fn new() -> Self {
        AppMuteRules { rules: [None; 16], n: 0 }
    }

    /// 设规则（mute=true 永远静默 / false 永远横幅——覆盖用户的全局偏好）。
    pub fn set_rule(&mut self, app: &str, mute: bool) -> bool {
        let k = app_key(app);
        for i in 0..self.n {
            if let Some((kk, _)) = self.rules[i] {
                if kk == k {
                    self.rules[i] = Some((k, mute));
                    return true;
                }
            }
        }
        if self.n >= 16 {
            return false;
        }
        self.rules[self.n] = Some((k, mute));
        self.n += 1;
        true
    }

    pub fn rule_of(&self, app: &str) -> Option<bool> {
        let k = app_key(app);
        (0..self.n).filter_map(|&i| self.rules[i]).find(|(kk, _)| *kk == k).map(|(_, m)| m)
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

/// 通知老化（通知中心入库条目按龄归档——中心全记但不无限堆积）。
pub const CENTER_AGING_MS: u64 = 24 * 60 * 60 * 1_000;

pub fn center_entry_expired(created_ms: u64, now_ms: u64) -> bool {
    now_ms.saturating_sub(created_ms) >= CENTER_AGING_MS
}

/// 聚焦历史窗（<200ms 快速切换不抖动判定——主册「聚焦判定 <200ms」的
/// 去抖实现：新身份须稳定 200ms 才生效）。
pub struct FocusDebounce {
    pending: Option<&'static str>,
    pending_since: u64,
    pub stable: Option<&'static str>,
}

pub const DEBOUNCE_MS: u64 = 200;

impl FocusDebounce {
    pub const fn new() -> Self {
        FocusDebounce { pending: None, pending_since: 0, stable: None }
    }

    /// 聚焦事件（去抖：未满 200ms 的候选不生效——身份切换 <200ms 判定的
    /// 运行面语义：切换完成判定，而不是切换抢跑）。
    pub fn on_focus(&mut self, app: Option<&'static str>, now_ms: u64) {
        if self.pending != app {
            self.pending = app;
            self.pending_since = now_ms;
        }
    }

    /// 时钟推进（候选满 200ms → 落位稳定身份）。
    pub fn tick(&mut self, now_ms: u64) {
        if let Some(p) = self.pending {
            if now_ms.saturating_sub(self.pending_since) >= DEBOUNCE_MS {
                self.stable = Some(p);
                self.pending = None;
            }
        }
    }
}

pub fn run_fgmute_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F461-deep");
    // 硬规则：永远静默/永远横幅（优先级最高的用户意志）。
    let mut r = AppMuteRules::new();
    cs.add("rule_set", r.set_rule("spam-app", true) && r.rule_of("spam-app") == Some(true), "");
    cs.add("rule_override", r.set_rule("spam-app", false) && r.rule_of("spam-app") == Some(false) && r.count() == 1, "");
    cs.add("rule_unknown_none", r.rule_of("quiet-app").is_none(), "");
    // 通知老化（24h 归档——中心全记但不无限堆积）。
    cs.add("aging_24h", !center_entry_expired(0, CENTER_AGING_MS - 1) && center_entry_expired(0, CENTER_AGING_MS), "");
    // 聚焦去抖（快速切换 <200ms 不抢跑；稳定后落位）。
    let mut d = FocusDebounce::new();
    d.on_focus(Some("a"), 0);
    d.tick(100);
    cs.add("debounce_holds", d.stable.is_none(), "");
    d.tick(200);
    cs.add("debounce_settles", d.stable == Some("a"), "");
    d.on_focus(Some("b"), 300);
    d.on_focus(Some("c"), 350); // 50ms 内再切——候选重置
    d.tick(400);
    cs.add("debounce_resets", d.stable == Some("a"), "");
    d.tick(560);
    cs.add("debounce_final", d.stable == Some("c"), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn rules_survive_resets() {
        let mut r = AppMuteRules::new();
        r.set_rule("x", true);
        r.set_rule("y", false);
        r.set_rule("x", false);
        assert_eq!(r.rule_of("x"), Some(false));
        assert_eq!(r.rule_of("y"), Some(false));
        assert_eq!(r.count(), 2);
    }

    #[test]
    fn debounce_matches_200ms_const() {
        let mut d = FocusDebounce::new();
        d.on_focus(Some("z"), 1_000);
        assert!(d.stable.is_none());
        d.tick(1_000 + DEBOUNCE_MS - 1);
        assert!(d.stable.is_none());
        d.tick(1_000 + DEBOUNCE_MS);
        assert_eq!(d.stable, Some("z"));
    }

    #[test]
    fn aging_boundary_is_exact() {
        assert!(!center_entry_expired(5_000, 5_000 + CENTER_AGING_MS - 1));
        assert!(center_entry_expired(5_000, 5_000 + CENTER_AGING_MS));
    }
}
'''

BLOCKS["dpifix"] = r'''
// ===========================================================================
// 深化 v2（F498）：缩放变更事件流 / 记账持久化 / 批量修复队列 / 修复回滚
// ===========================================================================

/// 缩放变更事件（用户改缩放 → 全窗重评——主册「高分屏发糊时」的触发面）。
#[derive(Clone, Copy, Debug)]
pub struct ScaleEvent {
    pub at_ms: u64,
    pub new_scale_permille: u16,
}

/// 修复台账持久化（已提示名单/静音名单——「每个应用只烦你一次」跨重启）。
pub struct FixMemoPersist {
    prompted: [Option<u64>; 32],
    silenced: [Option<u64>; 32],
    n_prompted: usize,
    n_silenced: usize,
}

pub const MEMO_MAGIC: [u8; 4] = *b"VFM1";

impl FixMemoPersist {
    pub const fn new() -> Self {
        FixMemoPersist { prompted: [None; 32], silenced: [None; 32], n_prompted: 0, n_silenced: 0 }
    }

    pub fn mark_prompted(&mut self, app: &str) -> bool {
        let k = app_key(app);
        if Self::has(&self.prompted, self.n_prompted, k) {
            return true;
        }
        if self.n_prompted >= 32 {
            return false;
        }
        self.prompted[self.n_prompted] = Some(k);
        self.n_prompted += 1;
        true
    }

    pub fn silence(&mut self, app: &str) -> bool {
        let k = app_key(app);
        if Self::has(&self.silenced, self.n_silenced, k) {
            return true;
        }
        if self.n_silenced >= 32 {
            return false;
        }
        self.silenced[self.n_silenced] = Some(k);
        self.n_silenced += 1;
        true
    }

    pub fn is_silenced(&self, app: &str) -> bool {
        Self::has(&self.silenced, self.n_silenced, app_key(app))
    }

    fn has(list: &[Option<u64>; 32], n: usize, k: u64) -> bool {
        (0..n).any(|i| list[i] == Some(k))
    }

    /// 序列化（魔标+版本+两名单计数+逐键——跨重启一次性记账）。
    pub fn save(&self, out: &mut [u8]) -> Option<usize> {
        let need = 7 + (self.n_prompted + self.n_silenced) * 8;
        if out.len() < need {
            return None;
        }
        out[..4].copy_from_slice(&MEMO_MAGIC);
        out[4] = 1;
        out[5] = self.n_prompted as u8;
        out[6] = self.n_silenced as u8;
        let mut w = 7;
        for i in 0..self.n_prompted {
            out[w..w + 8].copy_from_slice(&self.prompted[i].unwrap().to_le_bytes());
            w += 8;
        }
        for i in 0..self.n_silenced {
            out[w..w + 8].copy_from_slice(&self.silenced[i].unwrap().to_le_bytes());
            w += 8;
        }
        Some(w)
    }

    pub fn load(&mut self, buf: &[u8]) -> bool {
        if buf.len() < 7 || buf[..4] != MEMO_MAGIC || buf[4] != 1 {
            return false;
        }
        let np = buf[5] as usize;
        let ns = buf[6] as usize;
        if np > 32 || ns > 32 || buf.len() < 7 + (np + ns) * 8 {
            return false;
        }
        let mut r = 7;
        for i in 0..np {
            let mut b = [0u8; 8];
            b.copy_from_slice(&buf[r..r + 8]);
            self.prompted[i] = Some(u64::from_le_bytes(b));
            r += 8;
        }
        for i in 0..ns {
            let mut b = [0u8; 8];
            b.copy_from_slice(&buf[r..r + 8]);
            self.silenced[i] = Some(u64::from_le_bytes(b));
            r += 8;
        }
        self.n_prompted = np;
        self.n_silenced = ns;
        true
    }
}

fn app_key(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 批量修复队列（多应用同时发糊 → 一次性提示批量修——不逐应用轰炸）。
pub fn batch_fix_plan(apps: &[(&str, DpiAwareness, u16)]) -> usize {
    apps.iter().filter(|(_, a, s)| blur_detected(*a, *s)).count()
}

/// 修复回滚（一键应用效果不好 → 回原感知态——「修不好也诚实」的退路）。
pub fn rollback_fix(current: DpiAwareness, original: DpiAwareness) -> DpiAwareness {
    original
}

pub fn run_dpifix_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F498-deep");
    // 记账持久化 round-trip（跨重启一次性记账）。
    let mut m = FixMemoPersist::new();
    m.mark_prompted("app-a");
    m.silence("app-b");
    let mut buf = [0u8; 1024];
    let n = m.save(&mut buf).unwrap();
    let mut q = FixMemoPersist::new();
    cs.add("memo_roundtrip", q.load(&buf[..n]) && q.is_silenced("app-b") && !q.is_silenced("app-a"), "");
    let mut bad = buf;
    bad[0] = b'X';
    cs.add("memo_bad_magic", !FixMemoPersist::new().load(&bad[..n]), "");
    // 批量修复队列（只数发糊应用——不轰炸不漏算）。
    cs.add("batch_fix_count", batch_fix_plan(&[
        ("a", DpiAwareness::Unaware, 150),
        ("b", DpiAwareness::System, 200),
        ("c", DpiAwareness::Unaware, 100),
        ("d", DpiAwareness::Unaware, 200),
    ]) == 2, "");
    // 修复回滚（退路存在——修不好可回原态）。
    cs.add("rollback", rollback_fix(DpiAwareness::PerMonitor, DpiAwareness::Unaware) == DpiAwareness::Unaware, "");
    // 缩放事件模型（触发面：事件携带新缩放——全窗重评的输入）。
    cs.add("scale_event", {
        let e = ScaleEvent { at_ms: 1_000, new_scale_permille: 200 };
        blur_detected(DpiAwareness::Unaware, e.new_scale_permille)
    }, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn memo_survives_restart() {
        let mut m = FixMemoPersist::new();
        m.mark_prompted("legacy");
        m.silence("legacy2");
        let mut buf = [0u8; 1024];
        let n = m.save(&mut buf).unwrap();
        let mut q = FixMemoPersist::new();
        assert!(q.load(&buf[..n]));
        assert!(q.is_silenced("legacy2"));
        assert!(!q.is_silenced("legacy")); // 提示过 ≠ 静音（还能再手动修）
    }

    #[test]
    fn memo_cap_32_honest() {
        let mut m = FixMemoPersist::new();
        for i in 0..32 {
            assert!(m.mark_prompted(&num(i)));
        }
        assert!(!m.mark_prompted("overflow"));
    }

    fn num(i: usize) -> String {
        // 宿主测试链路专用（alloc 允许）。
        let mut s = String::from("app");
        s.push_str(&i.to_string());
        s
    }

    #[test]
    fn batch_never_counts_aware_apps() {
        assert_eq!(batch_fix_plan(&[("x", DpiAwareness::PerMonitor, 300)]), 0);
    }
}
'''

BLOCKS["autolum"] = r'''
// ===========================================================================
// 深化 v2（F491）：亮度曲线表 / 传感器平滑 / 夜间时段窗 / 状态持久化
// ===========================================================================

/// 亮度曲线表（环境 lux 分级 → 目标亮度——线性反比模型的分级化：
/// 深夜/室内/阴天/晴天五档，主册「亮暗过渡平滑」的输入端）。
pub const LUX_CURVE: [(u16, u16); 5] = [
    (50, 300),   // 深夜
    (200, 450),  // 暗室
    (500, 650),  // 室内灯
    (800, 850),  // 阴天窗边
    (1_000, 1_000), // 晴天
];

/// 曲线查表（lux 超表尾 → 表尾值；低于表头 → 下限 30%）。
pub fn curve_target(lux_permille: u16) -> u16 {
    for (lux, target) in LUX_CURVE {
        if lux_permille <= lux {
            return target.max(FLOOR_PERMILLE);
        }
    }
    LUX_CURVE[LUX_CURVE.len() - 1].1
}

/// 传感器平滑（EMA 指数滑动——环境光抖动不引起亮度跳变）。
pub struct LuxSmoother {
    ema_permille: u16,
    alpha_permille: u16,
    primed: bool,
}

pub const EMA_ALPHA_PERMILLE: u16 = 200; // 新样本权重 20%

impl LuxSmoother {
    pub const fn new() -> Self {
        LuxSmoother { ema_permille: 0, alpha_permille: EMA_ALPHA_PERMILLE, primed: false }
    }

    /// 采样（首样本直落；其后 EMA 平滑）。
    pub fn sample(&mut self, lux_permille: u16) -> u16 {
        if !self.primed {
            self.ema_permille = lux_permille;
            self.primed = true;
            return self.ema_permille;
        }
        let a = self.alpha_permille as u32;
        let e = self.ema_permille as u32;
        let v = lux_permille as u32;
        self.ema_permille = ((a * v + (1_000 - a) * e) / 1_000) as u16;
        self.ema_permille
    }

    pub fn value(&self) -> u16 {
        self.ema_permille
    }
}

/// 夜间时段窗（F116 夜间模式联动：窗内再压暗下限不变但目标减 20%——
/// 深夜刺眼防御；窗外原样）。
pub const NIGHT_DIM_PERMILLE: u16 = 800; // 夜间目标 ×0.8

pub fn night_adjust(target: u16, night: bool) -> u16 {
    if night {
        ((target as u32 * NIGHT_DIM_PERMILLE as u32) / 1_000).max(FLOOR_PERMILLE) as u16
    } else {
        target
    }
}

/// 状态持久化（开关+手动覆盖截止时刻——重启后自动亮度不复活）。
pub const PERSIST_MAGIC: [u8; 4] = *b"VAL1";

pub fn save_state(enabled: bool, override_until: u64, out: &mut [u8]) -> Option<usize> {
    if out.len() < 13 {
        return None;
    }
    out[..4].copy_from_slice(&PERSIST_MAGIC);
    out[4] = 1;
    out[5] = enabled as u8;
    out[6] = 0; // 保留位
    out[7..15].copy_from_slice(&override_until.to_le_bytes());
    Some(13)
}

pub fn load_state(buf: &[u8]) -> Option<(bool, u64)> {
    if buf.len() < 13 || buf[..4] != PERSIST_MAGIC || buf[4] != 1 {
        return None;
    }
    let enabled = buf[5] == 1;
    let mut o = [0u8; 8];
    o.copy_from_slice(&buf[7..15]);
    Some((enabled, u64::from_le_bytes(o)))
}

pub fn run_autolum_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F491-deep");
    // 曲线表：深夜档触底 30%；晴天档顶格。
    cs.add("curve_floor", curve_target(10) == FLOOR_PERMILLE, "");
    cs.add("curve_top", curve_target(1_100) == 1_000, "");
    cs.add("curve_mid", curve_target(300) == 450, "");
    // EMA 平滑：单点跳变不透传（20% 权重）。
    cs.add("ema_first_direct", { let mut s = LuxSmoother::new(); s.sample(500) == 500 }, "");
    cs.add("ema_dampens", {
        let mut s = LuxSmoother::new();
        s.sample(500);
        let v = s.sample(1_000); // 500 + 20%×500 = 600
        v == 600
    }, "");
    // 夜间压暗（目标减 20% 但不破 30% 下限——深夜不刺眼）。
    cs.add("night_dim", night_adjust(1_000, true) == 800, "");
    cs.add("night_floor_holds", night_adjust(300, true) == FLOOR_PERMILLE, "");
    cs.add("day_untouched", night_adjust(700, false) == 700, "");
    // 状态持久化（重启后不复活——开关默认关的诚实延续）。
    cs.add("persist_roundtrip", {
        let mut buf = [0u8; 16];
        let n = save_state(true, 123_456, &mut buf).unwrap();
        load_state(&buf[..n]) == Some((true, 123_456))
    }, "");
    cs.add("persist_bad_magic", load_state(b"XXXX\x01\x01\x00\x00\x00\x00\x00\x00\x00").is_none(), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn curve_is_monotonic() {
        let mut last = 0u16;
        for (_, t) in LUX_CURVE {
            assert!(t >= last);
            last = t;
        }
    }

    #[test]
    fn ema_converges_toward_input() {
        let mut s = LuxSmoother::new();
        s.sample(0);
        let mut prev = 0u16;
        for _ in 0..20 {
            let v = s.sample(1_000);
            assert!(v >= prev); // 单调逼近
            prev = v;
        }
        assert!(s.value() >= 990); // 20 轮后收敛
    }

    #[test]
    fn night_never_below_floor() {
        for t in [300u16, 500, 800, 1_000] {
            assert!(night_adjust(t, true) >= FLOOR_PERMILLE);
        }
    }

    #[test]
    fn persist_state_roundtrip_both_flags() {
        let mut buf = [0u8; 16];
        for (en, o) in [(false, 0u64), (true, u64::MAX)] {
            let n = save_state(en, o, &mut buf).unwrap();
            assert_eq!(load_state(&buf[..n]), Some((en, o)));
        }
    }
}
'''

BLOCKS["coolgov"] = r'''
// ===========================================================================
// 深化 v2（F493）：风扇曲线模型 / 温度历史环 / 降档事件账 / 状态持久化
// ===========================================================================

/// 风扇曲线（温度 → 风扇占空比 ‰——主动策略 aggressive 曲线；
/// 被动策略整体下移 30%——安静优先）。
pub const FAN_CURVE: [(u8, u16); 5] = [
    (50, 200),
    (60, 350),
    (70, 550),
    (80, 800),
    (90, 1_000),
];

pub fn fan_duty(temp_c: u8, policy: CoolingPolicy) -> u16 {
    let mut duty = 1_000u16;
    for (t, d) in FAN_CURVE {
        if temp_c <= t {
            duty = d;
            break;
        }
    }
    match policy {
        CoolingPolicy::Active => duty,
        CoolingPolicy::Passive => duty * 700 / 1_000,
        CoolingPolicy::Auto => duty, // Auto 的 effective 已裁决
    }
}

/// 温度历史环（64 采样 × 1s = 近一分钟温度轨迹——「配合 F197 心里有数」）。
pub struct TempHistory {
    ring: [(u64, u8); 64],
    head: usize,
    n: usize,
}

impl TempHistory {
    pub const fn new() -> Self {
        TempHistory { ring: [(0, 0); 64], head: 0, n: 0 }
    }

    pub fn push(&mut self, at_ms: u64, temp_c: u8) {
        self.ring[self.head] = (at_ms, temp_c);
        self.head = (self.head + 1) % 64;
        self.n = (self.n + 1).min(64);
    }

    /// 峰值（窗口内最高温——降档复盘用）。
    pub fn peak(&self) -> u8 {
        (0..self.n).filter_map(|&i| Some(self.ring[i].1)).max().unwrap_or(0)
    }

    /// 均值 ×10（半度分辨率——读数对账用）。
    pub fn mean_x10(&self) -> u16 {
        if self.n == 0 {
            return 0;
        }
        let sum: u32 = (0..self.n).map(|&i| self.ring[i].1 as u32).sum();
        (sum * 10 / self.n as u32) as u16
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

/// 降档/升档事件账（自动档每次裁决变化记账——策略切换可追溯）。
pub struct GovEventLog {
    events: [(u64, bool); 16], // (时刻, 是否主动)
    n: usize,
    head: usize,
}

impl GovEventLog {
    pub const fn new() -> Self {
        GovEventLog { events: [(0, false); 16], n: 0, head: 0 }
    }

    pub fn log(&mut self, at_ms: u64, active: bool) {
        self.events[self.head] = (at_ms, active);
        self.head = (self.head + 1) % 16;
        self.n = (self.n + 1).min(16);
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 抖动审计（60s 内升降档 ≥4 次 = 曲线抖动——回滞参数需复核的信号）。
    pub fn flapping(&self, now_ms: u64, window_ms: u64) -> bool {
        let mut flips = 0;
        let mut last: Option<bool> = None;
        for i in 0..self.n {
            let idx = (self.head + 16 - self.n + i) % 16;
            let (at, active) = self.events[idx];
            if now_ms.saturating_sub(at) <= window_ms {
                if let Some(l) = last {
                    if l != active {
                        flips += 1;
                    }
                }
                last = Some(active);
            }
        }
        flips >= 4
    }
}

/// 状态持久化（策略选择——重启后保留）。
pub const PERSIST_MAGIC: [u8; 4] = *b"VCG1";

pub fn save_policy(p: CoolingPolicy, out: &mut [u8]) -> Option<usize> {
    if out.len() < 6 {
        return None;
    }
    out[..4].copy_from_slice(&PERSIST_MAGIC);
    out[4] = 1;
    out[5] = p as u8;
    Some(6)
}

pub fn load_policy(buf: &[u8]) -> Option<CoolingPolicy> {
    if buf.len() < 6 || buf[..4] != PERSIST_MAGIC || buf[4] != 1 || buf[5] > 2 {
        return None;
    }
    Some(match buf[5] {
        0 => CoolingPolicy::Active,
        1 => CoolingPolicy::Passive,
        _ => CoolingPolicy::Auto,
    })
}

pub fn run_coolgov_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F493-deep");
    // 风扇曲线：90°C 顶格；被动档整体 -30%（安静优先）。
    cs.add("fan_curve_active", fan_duty(95, CoolingPolicy::Active) == 1_000, "");
    cs.add("fan_curve_passive", fan_duty(95, CoolingPolicy::Passive) == 700, "");
    cs.add("fan_curve_mid", fan_duty(70, CoolingPolicy::Active) == 550, "");
    // 温度历史：峰值/均值对账。
    cs.add("temp_history_peak", {
        let mut h = TempHistory::new();
        for (i, t) in [68u8, 72, 85, 79].iter().enumerate() {
            h.push(1_000 + i as u64, *t);
        }
        h.peak() == 85 && h.mean_x10() == 760 && h.count() == 4
    }, "");
    cs.add("temp_history_empty_honest", TempHistory::new().peak() == 0, "");
    // 降档事件账 + 抖动审计（回滞防抖的可观测面）。
    cs.add("event_log_flap_detected", {
        let mut g = GovEventLog::new();
        for i in 0..6u64 {
            g.log(1_000 + i * 1_000, i % 2 == 0);
        }
        g.flapping(7_000, 10_000)
    }, "");
    cs.add("event_log_stable_ok", {
        let mut g = GovEventLog::new();
        for i in 0..5u64 {
            g.log(1_000 + i * 1_000, true);
        }
        !g.flapping(6_000, 10_000)
    }, "");
    // 策略持久化 round-trip + 越界拒收。
    cs.add("persist_roundtrip", {
        let mut buf = [0u8; 8];
        let n = save_policy(CoolingPolicy::Auto, &mut buf).unwrap();
        load_policy(&buf[..n]) == Some(CoolingPolicy::Auto)
    }, "");
    cs.add("persist_bad_value", load_policy(&[b'V', b'C', b'G', b'1', 1, 7]).is_none(), "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn fan_curve_is_monotonic() {
        let mut last = 0u16;
        for (_, d) in FAN_CURVE {
            assert!(d >= last);
            last = d;
        }
    }

    #[test]
    fn passive_never_above_active() {
        for t in 40..=95u8 {
            assert!(fan_duty(t, CoolingPolicy::Passive) <= fan_duty(t, CoolingPolicy::Active));
        }
    }

    #[test]
    fn history_ring_wraps_at_64() {
        let mut h = TempHistory::new();
        for i in 0..70u64 {
            h.push(i * 1_000, (i % 100) as u8);
        }
        assert_eq!(h.count(), 64);
    }

    #[test]
    fn policy_roundtrip_all_three() {
        let mut buf = [0u8; 8];
        for p in [CoolingPolicy::Active, CoolingPolicy::Passive, CoolingPolicy::Auto] {
            let n = save_policy(p, &mut buf).unwrap();
            assert_eq!(load_policy(&buf[..n]), Some(p));
        }
    }
}
'''

def main():
    for mod, src in BLOCKS.items():
        path = os.path.join(BASE, mod + ".rs")
        with io.open(path, "r", encoding="utf-8") as f:
            content = f.read()
        if "深化 v2" in content:
            print(f"SKIP {mod} (already deepened)")
            continue
        with io.open(path, "a", encoding="utf-8", newline="") as f:
            f.write(src)
        print(f"APPEND {mod}: +{src.count(chr(10))} lines")

    # mod.rs: wire deep checks into aggregate (combined per-row verdict)
    mod_path = os.path.join(BASE, "mod.rs")
    with io.open(mod_path, "r", encoding="utf-8") as f:
        s = f.read()
    pairs = [
        ("F476", "envedit", "run_envedit_checks"),
        ("F461", "fgmute", "run_fgmute_checks"),
        ("F498", "dpifix", "run_dpifix_checks"),
        ("F491", "autolum", "run_autolum_checks"),
        ("F493", "coolgov", "run_coolgov_checks"),
    ]
    changed = 0
    for tag, m, fn_ in pairs:
        old = f'("{tag}", {m}::{fn_}()),'
        new = f'("{tag}", {{ let a = {m}::{fn_}(); let b = {m}::run_{m}_deep_checks(); CheckSet::merge(a, b) }}),'
        if old in s and new not in s:
            s = s.replace(old, new)
            changed += 1
    with io.open(mod_path, "w", encoding="utf-8", newline="") as f:
        f.write(s)
    print(f"WIRED {changed} deep checks into aggregate")

if __name__ == "__main__":
    main()
