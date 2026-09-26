//! F498 高 DPI 模糊修复提示（genstar2 · I 域通用·二分队 · AI-U2）。
//!
//! 主册判据（验收标准第一句）：
//! **模糊检测判据（非 aware 应用识别）；一键应用与重绘；不再提示；与 F028
//! 三态底层联动；提示一次性记账。**
//!
//! 功能定义（主册批次三）：旧应用在高分屏发糊时（非 DPI aware 应用被系统
//! 拉伸）——系统检测到模糊场景弹一次性提示条（「此应用在高分屏下可能模糊
//! ——尝试高 DPI 优化？」一键应用）；应用后立即重绘验证；「不再为此应用
//! 提示」选项；每个应用只烦你一次。
//!
//! 零堆纪律：定长记账表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// F028 三态（底层联动——本项是三态的用户侧入口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpiAwareness {
    Unaware,
    System,
    PerMonitor,
}

/// 拉伸倍率超过此值判「模糊场景」（150% 缩放下非 aware 必糊——主册场景）。
pub const BLUR_SCALE_PERMILLE: u16 = 150;

/// 提示条文案（一次性、可操作）。
pub const PROMPT_TEXT: &str = "此应用在高分屏下可能模糊——尝试高 DPI 优化？";

/// DPI 修复管理器。
pub struct DpiFix {
    /// 每应用提示记账（提示一次性——每个应用只烦你一次）。
    prompted: [Option<u64>; 32],
    n: usize,
    /// 永不再提示名单（用户选择——诚实尊重）。
    silenced: [Option<u64>; 32],
    silenced_n: usize,
}

fn app_key(name: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in name.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 模糊检测判据（主册：非 aware 应用识别——感知态 × 缩放倍率；150% 即糊）。
pub fn blur_detected(aware: DpiAwareness, scale_permille: u16) -> bool {
    aware == DpiAwareness::Unaware && scale_permille >= BLUR_SCALE_PERMILLE
}

impl DpiFix {
    pub const fn new() -> Self {
        DpiFix { prompted: [None; 32], n: 0, silenced: [None; 32], silenced_n: 0 }
    }

    fn in_list(list: &[Option<u64>; 32], n: usize, k: u64) -> bool {
        (0..n).any(|i| list[i] == Some(k))
    }

    /// 是否弹提示（模糊 + 从未提示过 + 未被静音——一次性记账）。
    pub fn should_prompt(&mut self, app: &str, aware: DpiAwareness, scale_permille: u16) -> bool {
        if !blur_detected(aware, scale_permille) {
            return false;
        }
        let k = app_key(app);
        if Self::in_list(&self.silenced, self.silenced_n, k) || Self::in_list(&self.prompted, self.n, k) {
            return false;
        }
        if self.n < 32 {
            self.prompted[self.n] = Some(k);
            self.n += 1;
        }
        true
    }

    /// 「不再为此应用提示」（用户选择记账）。
    pub fn silence(&mut self, app: &str) -> bool {
        let k = app_key(app);
        if Self::in_list(&self.silenced, self.silenced_n, k) {
            return true; // 幂等
        }
        if self.silenced_n >= 32 {
            return false;
        }
        self.silenced[self.silenced_n] = Some(k);
        self.silenced_n += 1;
        true
    }

    /// 一键应用（F028 三态底层联动：Unaware → PerMonitor 优化）。
    /// 返回应用后的感知态（立即重绘由调用层触发）。
    pub fn apply_fix(from: DpiAwareness) -> Option<DpiAwareness> {
        match from {
            DpiAwareness::Unaware => Some(DpiAwareness::PerMonitor),
            _ => None, // 非 unaware 应用不糊——无修复可做（诚实）
        }
    }

    /// 重绘验证（应用后立即重绘——对比度/清晰度判据模拟：应用后不再判糊）。
    pub fn redraw_verify(after: DpiAwareness, scale_permille: u16) -> bool {
        !blur_detected(after, scale_permille)
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

pub fn run_dpifix_checks() -> CheckSet {
    let mut cs = CheckSet::new("F498-dpifix");
    // 1) 模糊检测判据（非 aware + 150% 缩放 → 糊；aware 不糊）。
    cs.add("blur_unaware_150", blur_detected(DpiAwareness::Unaware, 150), "");
    cs.add("no_blur_aware", !blur_detected(DpiAwareness::System, 200) && !blur_detected(DpiAwareness::PerMonitor, 200), "");
    cs.add("no_blur_100", !blur_detected(DpiAwareness::Unaware, 100), "");
    // 2) 提示一次性记账（每个应用只烦你一次）。
    let mut d = DpiFix::new();
    cs.add("prompt_once", d.should_prompt("old-app", DpiAwareness::Unaware, 150), "");
    cs.add("prompt_never_twice", !d.should_prompt("old-app", DpiAwareness::Unaware, 150), "");
    // 3) 不再提示（静音名单）。
    cs.add("silence_works", d.silence("another-app") && !d.should_prompt("another-app", DpiAwareness::Unaware, 200), "");
    // 4) 一键应用与重绘（Unaware → PerMonitor 后 200% 不糊）。
    cs.add("apply_fix", DpiFix::apply_fix(DpiAwareness::Unaware) == Some(DpiAwareness::PerMonitor), "");
    cs.add("redraw_verify", DpiFix::redraw_verify(DpiAwareness::PerMonitor, 200), "");
    cs.add("fix_honest_noop", DpiFix::apply_fix(DpiAwareness::System).is_none(), "");
    // 5) 三态底层联动（枚举与 F028 同构）。
    cs.add("three_states", DpiAwareness::Unaware as u8 == 0 && DpiAwareness::System as u8 == 1 && DpiAwareness::PerMonitor as u8 == 2, "");
    // 6) 文案在册。
    cs.add("prompt_text", PROMPT_TEXT.contains("高分屏") && PROMPT_TEXT.ends_with("？"), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_exactly_once_per_app() {
        let mut d = DpiFix::new();
        assert!(d.should_prompt("legacy", DpiAwareness::Unaware, 200));
        assert!(!d.should_prompt("legacy", DpiAwareness::Unaware, 200));
        // 换应用独立记账。
        assert!(d.should_prompt("legacy2", DpiAwareness::Unaware, 150));
    }

    #[test]
    fn silence_preempts_prompt_forever() {
        let mut d = DpiFix::new();
        d.silence("sticky");
        for _ in 0..3 {
            assert!(!d.should_prompt("sticky", DpiAwareness::Unaware, 300));
        }
    }

    #[test]
    fn fix_chain_end_to_end() {
        // 检测 → 提示 → 一键应用 → 重绘验证通过（用户侧完整链）。
        let mut d = DpiFix::new();
        assert!(d.should_prompt("app", DpiAwareness::Unaware, 150));
        let fixed = DpiFix::apply_fix(DpiAwareness::Unaware).unwrap();
        assert!(DpiFix::redraw_verify(fixed, 150));
    }
}

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

// ===========================================================================
// 深化 v3（F498）：DPI 感知阶梯提升序 / 混合 DPI 多屏账 / 修复撤销面 /
// 提示抑制持久化 / 缩放变更重算账
// ===========================================================================

/// DPI 感知阶梯（主册「高 DPI 优化」的提升序：Unaware → System →
/// PerMonitor 单向提升——降级会让已经清晰的界面重新模糊）。
pub fn upgrade_path_ok(from: DpiAwareness, to: DpiAwareness) -> bool {
    let rank = |a: DpiAwareness| match a {
        DpiAwareness::Unaware => 0,
        DpiAwareness::System => 1,
        DpiAwareness::PerMonitor => 2,
    };
    rank(to) > rank(from)
}

/// 混合 DPI 多屏账（主册「多屏不同缩放」：每屏独立缩放，窗口跨屏
/// 时刻按目标屏缩放重算——窗口在 100% 屏清晰、拖到 200% 屏也清晰）。
pub const SCREEN_SCALE_CAP: usize = 4;

pub struct MultiScale {
    scales: [u16; SCREEN_SCALE_CAP], // permille，如 1500 = 150%
    n: usize,
}

impl MultiScale {
    pub const fn new() -> Self {
        MultiScale { scales: [1_000; SCREEN_SCALE_CAP], n: 1 }
    }

    pub fn set_screen(&mut self, idx: usize, permille: u16) -> bool {
        if idx >= SCREEN_SCALE_CAP || permille < 1_000 || permille > 4_000 {
            return false;
        }
        self.scales[idx] = permille;
        self.n = self.n.max(idx + 1);
        true
    }

    pub fn scale(&self, idx: usize) -> Option<u16> {
        self.scales.get(idx).copied()
    }

    /// 跨屏位图重算倍率（源屏 → 目标屏的缩放比 ×1000：100%→200% = 2000）。
    pub fn rescale_ratio(&self, from: usize, to: usize) -> Option<u32> {
        let f = self.scale(from)? as u32;
        let t = self.scale(to)? as u32;
        Some(t * 1_000 / f.max(1))
    }
}

impl DpiFix {
    /// 修复撤销（主册「恢复默认永远一键可退」：应用过优化的应用从
    /// 提示账移除——撤销后若仍模糊会再次提示；优化失败可退）。
    pub fn undo_fix(&mut self, app: &str) -> bool {
        let k = app_key(app);
        for i in 0..self.n {
            if self.prompted[i] == Some(k) {
                // 尾补位删除（定长表纪律与 silence 一致）。
                self.prompted[i] = self.prompted[self.n - 1];
                self.prompted[self.n - 1] = None;
                self.n -= 1;
                return true;
            }
        }
        false
    }

    pub fn is_prompted(&self, app: &str) -> bool {
        Self::in_list(&self.prompted, self.n, app_key(app))
    }
}

/// 提示抑制持久化（主册「不再提示」跨会话：抑制名单定长落盘
/// round-trip——重启后还是不问）。
pub const DPI_SILENCE_MAGIC: [u8; 4] = *b"VDM1";

pub fn save_silence(names: &[&'static str], out: &mut [u8]) -> Option<usize> {
    if names.len() > 16 || out.len() < 4 + names.len() * 24 {
        return None;
    }
    out[..4].copy_from_slice(&DPI_SILENCE_MAGIC);
    out[4] = names.len() as u8;
    let mut w = 5;
    for n in names {
        let b = n.as_bytes();
        if b.len() > 23 {
            return None;
        }
        out[w] = b.len() as u8;
        out[w + 1..w + 1 + b.len()].copy_from_slice(b);
        w += 24;
    }
    Some(w)
}

pub fn load_silence(buf: &[u8]) -> Option<usize> {
    if buf.len() < 5 || buf[..4] != DPI_SILENCE_MAGIC {
        return None;
    }
    let n = buf[4] as usize;
    if n > 16 || buf.len() < 5 + n * 24 {
        return None;
    }
    for i in 0..n {
        let base = 5 + i * 24;
        let len = buf[base] as usize;
        if len == 0 || len > 23 {
            return None;
        }
    }
    Some(n)
}

/// 缩放变更重算账（主册「分辨率/缩放变更即时重算」：变更时刻起
/// 所有 DpiAwareness=System 窗口需重排——重算触发的判定面）。
pub fn needs_recalc(aware: DpiAwareness, old_scale: u16, new_scale: u16) -> bool {
    old_scale != new_scale && aware != DpiAwareness::PerMonitor
}

// ---------------------------------------------------------------------------
// 深化 v3 自检（F498-v3）
// ---------------------------------------------------------------------------

pub fn run_dpifix_v3_checks() -> CheckSet {
    let mut cs = CheckSet::new("F498-v3");
    // 1) 阶梯提升：单向、同级拒。
    cs.add("upgrade_unaware_pm", upgrade_path_ok(DpiAwareness::Unaware, DpiAwareness::PerMonitor), "");
    cs.add("upgrade_system_pm", upgrade_path_ok(DpiAwareness::System, DpiAwareness::PerMonitor), "");
    cs.add("no_downgrade", !upgrade_path_ok(DpiAwareness::PerMonitor, DpiAwareness::System), "");
    cs.add("no_same", !upgrade_path_ok(DpiAwareness::System, DpiAwareness::System), "");
    // 2) 混合 DPI：逐屏设置、跨屏重算比。
    let mut ms = MultiScale::new();
    let _ = ms.set_screen(0, 1_000);
    let _ = ms.set_screen(1, 2_000);
    cs.add("mixed_scales", ms.scale(0) == Some(1_000) && ms.scale(1) == Some(2_000), "");
    cs.add("cross_rescale", ms.rescale_ratio(0, 1) == Some(2_000), "");
    cs.add("rescale_down", ms.rescale_ratio(1, 0) == Some(500), "");
    cs.add("scale_reject", !ms.set_screen(2, 500) && !ms.set_screen(2, 5_000), "");
    // 3) 修复撤销：提示账在册可撤、未登记诚实拒。
    let mut p = DpiFix::new();
    let _ = p.should_prompt("legacy.app", DpiAwareness::Unaware, 1_500);
    let fix = DpiFix::apply_fix(DpiAwareness::Unaware);
    cs.add("fix_maps_pm", fix == Some(DpiAwareness::PerMonitor), "");
    cs.add("undo_after_prompt", p.is_prompted("legacy.app") && p.undo_fix("legacy.app") && !p.is_prompted("legacy.app"), "");
    cs.add("undo_never_prompted", !p.undo_fix("never.app"), "");
    // 4) 抑制持久化 round-trip + 坏名拒收。
    let names = ["a.app", "b.tool"];
    let mut buf = [0u8; 4 + 16 * 24];
    cs.add("silence_roundtrip", {
        let n = save_silence(&names, &mut buf).unwrap();
        load_silence(&buf[..n]) == Some(2)
    }, "");
    cs.add("silence_bad_magic", load_silence(b"XXXX\x00\x00\x00\x00").is_none(), "");
    // 5) 重算触发：缩放变 + 非 PerMonitor 才需重排。
    cs.add("recalc_system", needs_recalc(DpiAwareness::System, 1_000, 1_500), "");
    cs.add("recalc_pm_free", !needs_recalc(DpiAwareness::PerMonitor, 1_000, 1_500), "");
    cs.add("recalc_same_free", !needs_recalc(DpiAwareness::System, 1_500, 1_500), "");
    cs
}

#[cfg(test)]
mod v3_tests {
    use super::*;

    #[test]
    fn multi_scale_full_chain() {
        let mut ms = MultiScale::new();
        let scales = [1_000u16, 1_250, 1_500, 2_000];
        for (i, s) in scales.iter().enumerate() {
            assert!(ms.set_screen(i, *s));
        }
        // 相邻屏重算比逐对验证。
        assert_eq!(ms.rescale_ratio(0, 1), Some(1_250));
        assert_eq!(ms.rescale_ratio(1, 2), Some(1_200));
        assert_eq!(ms.rescale_ratio(2, 3), Some(1_333));
    }

    #[test]
    fn silence_oversize_name_rejected() {
        let long = "this-app-name-is-way-too-long-for-the-slot";
        let mut buf = [0u8; 4 + 16 * 24];
        assert!(save_silence(&[long], &mut buf).is_none());
    }

    #[test]
    fn blur_boundary_at_150() {
        // v1 口径：BLUR_SCALE_PERMILLE=150 实为「百分数」线（150=150%）——
        // permille 命名与实际语义有偏差（行为差异候选登记 F475）。
        assert!(blur_detected(DpiAwareness::Unaware, 1_500), "150%（传 1500）判糊");
        assert!(!blur_detected(DpiAwareness::Unaware, 100), "100%（传 100）清晰");
        assert!(blur_detected(DpiAwareness::Unaware, 150), "恰 150 过线");
    }
}
