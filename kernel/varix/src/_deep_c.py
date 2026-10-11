# -*- coding: utf-8 -*-
"""AIU2 deepening batch C: termfont/termclip/termdir/unitconv/chantest."""
import io, os

BASE = os.path.join(os.path.dirname(__file__), "genstar2")
BLOCKS = {}

BLOCKS["termfont"] = r'''
// ===========================================================================
// 深化 v2（F467）：行高联动 / 提示文案 / 档位持久化 / 缩放网格对齐
// ===========================================================================

/// 行高随档位联动（字号 × 1.5 向上取整——终端网格不糊的配对参数）。
pub fn line_height_px(tier: usize) -> Option<u16> {
    if tier >= FONT_TIERS_PX.len() {
        return None;
    }
    Some((FONT_TIERS_PX[tier] * 3 + 2) / 2 * 1 + (FONT_TIERS_PX[tier] % 2))
        .map(|base: u16| (FONT_TIERS_PX[tier] * 3).div_ceil(2))
        .or(None)
}

/// 提示文案（主册：「16px」微提示——档位可见即文案）。
pub fn hint_text(tier: usize) -> Option<&'static str> {
    const HINTS: [&str; 5] = ["12px", "14px", "16px", "20px", "24px"];
    HINTS.get(tier).copied()
}

/// 档位持久化（用户默认档——重启后保持；魔标+版本+档位）。
pub const PERSIST_MAGIC: [u8; 4] = *b"VTF1";

pub fn save_default_tier(tier: Option<usize>, out: &mut [u8]) -> Option<usize> {
    if out.len() < 7 {
        return None;
    }
    out[..4].copy_from_slice(&PERSIST_MAGIC);
    out[4] = 1;
    out[5] = match tier {
        Some(t) if t < FONT_TIERS_PX.len() => t as u8,
        Some(_) => return None,
        None => 0xFF, // 未改哨兵
    };
    out[6] = 0;
    Some(7)
}

pub fn load_default_tier(buf: &[u8]) -> Option<Option<usize>> {
    if buf.len() < 7 || buf[..4] != PERSIST_MAGIC || buf[4] != 1 {
        return None;
    }
    match buf[5] {
        0xFF => Some(None),
        t if (t as usize) < FONT_TIERS_PX.len() => Some(Some(t as usize)),
        _ => None,
    }
}

impl TermFont {
    /// 会话内滚轮事件聚合（快速连滚合并为单步——高分辨率滚轮不飞档）。
    pub fn wheel_ticks(&mut self, ticks: i32, up: bool, now_ms: u64) -> usize {
        let n = ticks.unsigned_abs().min(4) as usize;
        let mut applied = 0;
        for _ in 0..n {
            if self.step(up, now_ms) {
                applied += 1;
            }
        }
        applied
    }
}

pub fn run_termfont_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F467-deep");
    // 行高联动（1.5 倍行高——12px→18px、16px→24px）。
    cs.add("line_height_pairs", line_height_px(0) == Some(18) && line_height_px(2) == Some(24), "");
    cs.add("line_height_oob", line_height_px(5).is_none(), "");
    // 提示文案五档齐备。
    cs.add("hint_texts", (0..5).all(|t| hint_text(t).is_some()) && hint_text(2) == Some("16px"), "");
    // 档位持久化（Some/None 两态 round-trip + 坏档拒收）。
    cs.add("persist_some", {
        let mut buf = [0u8; 8];
        let n = save_default_tier(Some(3), &mut buf).unwrap();
        load_default_tier(&buf[..n]) == Some(Some(3))
    }, "");
    cs.add("persist_none", {
        let mut buf = [0u8; 8];
        let n = save_default_tier(None, &mut buf).unwrap();
        load_default_tier(&buf[..n]) == Some(None)
    }, "");
    cs.add("persist_bad_tier", save_default_tier(Some(9), &mut [0u8; 8]).is_none(), "");
    cs.add("persist_bad_magic", load_default_tier(b"XXXX\x01\x03\x00").is_none(), "");
    // 滚轮聚合（连滚 5 tick 只走 4 档上限内——边界诚实）。
    cs.add("wheel_aggregate", {
        let mut f = TermFont::new();
        let applied = f.wheel_ticks(5, true, 0);
        applied == 2 && f.px() == 12 // 从 16px 上滚两档到 12px 到底
    }, "");
    cs.add("wheel_down_clamped", {
        let mut f = TermFont::new();
        let applied = f.wheel_ticks(9, false, 0);
        applied == 2 && f.px() == 24
    }, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn line_height_tracks_every_tier() {
        for t in 0..FONT_TIERS_PX.len() {
            let lh = line_height_px(t).unwrap();
            assert!(lh >= FONT_TIERS_PX[t], "行高不小于字号");
        }
    }

    #[test]
    fn persist_roundtrip_all_tiers() {
        let mut buf = [0u8; 8];
        for t in 0..FONT_TIERS_PX.len() {
            let n = save_default_tier(Some(t), &mut buf).unwrap();
            assert_eq!(load_default_tier(&buf[..n]), Some(Some(t)));
        }
    }

    #[test]
    fn wheel_aggregation_never_overshoots() {
        let mut f = TermFont::new();
        f.wheel_ticks(3, true, 0);
        assert_eq!(f.px(), 12); // 顶档停住
        assert!(f.wheel_ticks(3, true, 1) == 0);
    }
}
'''

BLOCKS["termclip"] = r'''
// ===========================================================================
// 深化 v2（F466）：粘贴缓冲超时 / 选择区快照 / 引号规范化矩阵 / 审计账
// ===========================================================================

/// 确认缓冲超时（挂起 30s 未确认 → 自动作废——安全带不解到明天）。
pub const BUFFER_TIMEOUT_MS: u64 = 30_000;

/// 粘贴缓冲计时器（armed 起算；超时作废——浮层出路纪律同源）。
pub struct BufferTimer {
    armed_at: Option<u64>,
}

impl BufferTimer {
    pub const fn new() -> Self {
        BufferTimer { armed_at: None }
    }

    pub fn arm(&mut self, now_ms: u64) {
        self.armed_at = Some(now_ms);
    }

    pub fn expired(&mut self, now_ms: u64) -> bool {
        match self.armed_at {
            Some(t) if now_ms.saturating_sub(t) >= BUFFER_TIMEOUT_MS => {
                self.armed_at = None;
                true
            }
            _ => false,
        }
    }

    pub fn disarm(&mut self) {
        self.armed_at = None;
    }
}

/// 选择区快照（选择即复制的一致性凭证：快照带序号——重复选择不重写同内容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectionSnapshot {
    pub seq: u64,
    pub len: usize,
    pub select_copies_enabled: bool,
}

pub fn selection_copies(prev: Option<SelectionSnapshot>, len: usize, enabled: bool, seq: u64) -> Option<SelectionSnapshot> {
    if !enabled || len == 0 {
        return None;
    }
    // 内容长度相同且序号相同 = 未变（不重复入剪贴板账）。
    if let Some(p) = prev {
        if p.len == len && p.seq == seq {
            return None;
        }
    }
    Some(SelectionSnapshot { seq, len, select_copies_enabled: enabled })
}

/// 引号规范化矩阵（三形态输入 → 规范路径——F336 互通的穷举面）。
pub fn quote_normalized_ok(raw: &str, expect: &str) -> bool {
    strip_quotes(raw) == expect
}

/// 粘贴审计账（最近 8 次：来源/行数/裁决——粘贴炸弹防御可回溯）。
pub struct PasteAudit {
    ring: [(u64, u8, bool); 8], // (时刻, 行数, 是否需确认)
    head: usize,
    n: usize,
}

impl PasteAudit {
    pub const fn new() -> Self {
        PasteAudit { ring: [(0, 0, false); 8], head: 0, n: 0 }
    }

    pub fn log(&mut self, at_ms: u64, lines: u8, needed_confirm: bool) {
        self.ring[self.head] = (at_ms, lines, needed_confirm);
        self.head = (self.head + 1) % 8;
        self.n = (self.n + 1).min(8);
    }

    /// 粘贴炸弹特征（60s 内 ≥3 次多行粘贴——告警信号）。
    pub fn bomb_pattern(&self, now_ms: u64, window_ms: u64) -> bool {
        let mut multi = 0;
        for i in 0..self.n {
            let idx = (self.head + 8 - self.n + i) % 8;
            let (at, lines, _) = self.ring[idx];
            if now_ms.saturating_sub(at) <= window_ms && lines > 1 {
                multi += 1;
            }
        }
        multi >= 3
    }
}

pub fn run_termclip_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F466-deep");
    // 确认缓冲超时（30s 挂起自动作废——出路完整）。
    cs.add("buffer_timeout", {
        let mut t = BufferTimer::new();
        t.arm(1_000);
        !t.expired(1_000 + BUFFER_TIMEOUT_MS - 1) && t.expired(1_000 + BUFFER_TIMEOUT_MS)
    }, "");
    cs.add("buffer_disarm", {
        let mut t = BufferTimer::new();
        t.arm(0);
        t.disarm();
        !t.expired(BUFFER_TIMEOUT_MS + 1)
    }, "");
    // 选择即复制的一致性（未变不重写；关闭不出账；空选择不出账）。
    cs.add("selection_dedup", {
        let s1 = selection_copies(None, 42, true, 1);
        let s2 = selection_copies(s1, 42, true, 1);
        s1.is_some() && s2.is_none()
    }, "");
    cs.add("selection_disabled_none", selection_copies(None, 42, false, 1).is_none(), "");
    cs.add("selection_empty_none", selection_copies(None, 0, true, 1).is_none(), "");
    // 引号规范化矩阵（单双引号/无引号/混合——F336 互通穷举）。
    cs.add("quote_matrix", quote_normalized_ok("\"C:\\a b\"", "C:\\a b")
        && quote_normalized_ok("'C:\\a b'", "C:\\a b")
        && quote_normalized_ok("C:\\plain", "C:\\plain")
        && !quote_normalized_ok("\"mismatch'", "mismatch'"), "");
    // 粘贴审计 + 炸弹特征（60s 三次多行 → 告警）。
    cs.add("bomb_pattern_detected", {
        let mut a = PasteAudit::new();
        a.log(1_000, 5, true);
        a.log(2_000, 9, true);
        a.log(3_000, 2, true);
        a.bomb_pattern(4_000, 60_000)
    }, "");
    cs.add("single_paste_ok", {
        let mut a = PasteAudit::new();
        a.log(1_000, 5, true);
        !a.bomb_pattern(2_000, 60_000)
    }, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn buffer_timeout_exact_boundary() {
        let mut t = BufferTimer::new();
        t.arm(0);
        assert!(!t.expired(BUFFER_TIMEOUT_MS - 1));
        assert!(t.expired(BUFFER_TIMEOUT_MS));
    }

    #[test]
    fn selection_changes_when_content_changes() {
        let s1 = selection_copies(None, 10, true, 1);
        let s2 = selection_copies(s1, 20, true, 2);
        assert!(s2.is_some());
        assert_eq!(s2.unwrap().len, 20);
    }

    #[test]
    fn audit_ring_wraps() {
        let mut a = PasteAudit::new();
        for i in 0..12u64 {
            a.log(i * 1_000, 1, false);
        }
        assert_eq!(a.n, 8);
        assert!(!a.bomb_pattern(13_000, 60_000));
    }
}
'''

BLOCKS["termdir"] = r'''
// ===========================================================================
// 深化 v2（F470）：目录合法性校验 / 标签目录账持久化 / 路径归一化
// ===========================================================================

/// 路径归一化（尾分隔符统一 + 重复分隔符合并——「开终端就在对的目录」
/// 的前置卫生；零分配：返回归一化后的字节长，原位写回缓冲）。
pub fn normalize_path(buf: &mut [u8], n: &mut usize) {
    // 合并重复分隔符。
    let mut w = 0;
    for r in 0..*n {
        let c = buf[r];
        if c == b'\\' && w > 0 && buf[w - 1] == b'\\' {
            continue;
        }
        buf[w] = c;
        w += 1;
    }
    *n = w;
    // 尾分隔符保留单个（目录语义）。
    if *n > 1 && buf[*n - 1] == b'\\' && buf[*n - 2] == b'\\' {
        *n -= 1;
    }
}

/// 目录可达性校验（存在 + 非系统保留名——终端不 cd 进不该进的地方）。
pub fn dir_entry_ok(path: &str, exists: bool) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("目录不能为空");
    }
    if path.len() > PATH_CAP {
        return Err("路径过长");
    }
    const RESERVED: [&str; 6] = ["CON", "PRN", "AUX", "NUL", "COM1", "LPT1"];
    let last = path.rsplit(['\\', '/']).next().unwrap_or("");
    for r in RESERVED {
        if last.eq_ignore_ascii_case(r) {
            return Err("系统保留名不可作目录");
        }
    }
    if !exists {
        return Err("目录不存在");
    }
    Ok(())
}

/// 标签目录账持久化（多标签各目录跨重启恢复——主册「多标签独立」的
/// 持久化面；魔标+版本+逐标签路径）。
pub const TABS_PERSIST_MAGIC: [u8; 4] = *b"VTD1";

pub fn save_tabs(tabs: &[Option<DirPath>; TAB_CAP], tab_n: usize, out: &mut [u8]) -> Option<usize> {
    if out.len() < 6 + tab_n * (1 + PATH_CAP) {
        return None;
    }
    out[..4].copy_from_slice(&TABS_PERSIST_MAGIC);
    out[4] = 1;
    out[5] = tab_n as u8;
    let mut w = 6;
    for i in 0..tab_n {
        match &tabs[i] {
            Some(d) => {
                out[w] = d.n as u8;
                out[w + 1..w + 1 + d.n].copy_from_slice(&d.buf[..d.n]);
            }
            None => out[w] = 0,
        }
        w += 1 + PATH_CAP;
    }
    Some(w)
}

pub fn load_tabs(buf: &[u8]) -> Option<([Option<DirPath>; TAB_CAP], usize)> {
    if buf.len() < 6 || buf[..4] != TABS_PERSIST_MAGIC || buf[4] != 1 {
        return None;
    }
    let tn = buf[5] as usize;
    if tn > TAB_CAP || buf.len() < 6 + tn * (1 + PATH_CAP) {
        return None;
    }
    let mut tabs = [None; TAB_CAP];
    let mut r = 6;
    for i in 0..tn {
        let len = buf[r] as usize;
        if len > PATH_CAP {
            return None;
        }
        if len > 0 {
            let mut d = DirPath { buf: [0; PATH_CAP], n: len };
            d.buf[..len].copy_from_slice(&buf[r + 1..r + 1 + len]);
            tabs[i] = Some(d);
        }
        r += 1 + PATH_CAP;
    }
    Some((tabs, tn))
}

pub fn run_termdir_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F470-deep");
    // 路径归一化（重复分隔符合并——零分配原位写回）。
    cs.add("normalize_dedup", {
        let mut b = *b"C:\\\\work\\\\sub\\\\";
        let mut n = 14;
        normalize_path(&mut b, &mut n);
        core::str::from_utf8(&b[..n]) == Ok("C:\\work\\sub\\")
    }, "");
    // 目录可达性校验（保留名拒绝——终端不 cd 进 CON）。
    cs.add("reserved_name_rejected", dir_entry_ok("C:\\CON", true).is_err() && dir_entry_ok("C:\\nul", true).is_err(), "");
    cs.add("missing_dir_rejected", dir_entry_ok("C:\\ghost", false).is_err(), "");
    cs.add("valid_dir_ok", dir_entry_ok("C:\\work", true).is_ok(), "");
    // 标签账持久化 round-trip（三标签两空——重启恢复各目录）。
    cs.add("tabs_persist_roundtrip", {
        let mut t = TermDirs::new();
        t.new_tab(DirPath::new("C:\\a").unwrap());
        t.new_tab(DirPath::new("D:\\b").unwrap());
        t.new_tab(DirPath::new("E:\\c").unwrap());
        let mut buf = [0u8; 1024];
        let n = save_tabs(&t.tabs, t.tab_n, &mut buf).unwrap();
        match load_tabs(&buf[..n]) {
            Some((tabs, tn)) => {
                tn == 3
                    && tabs[0].as_ref().unwrap().as_str() == "C:\\a"
                    && tabs[2].as_ref().unwrap().as_str() == "E:\\c"
            }
            None => false,
        }
    }, "");
    cs.add("tabs_persist_bad_magic", load_tabs(b"XXXX\x01\x00").is_none(), "");
    // 超长路径拒绝（目录合法性 ——PATH_CAP 红线）。
    cs.add("oversize_rejected", dir_entry_ok(&oversize_helper(), true).is_err(), "");
    cs
}

fn oversize_helper() -> String {
    let mut s = String::from("C:\\");
    for _ in 0..PATH_CAP {
        s.push('x');
    }
    s
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn normalize_keeps_single_trailing() {
        let mut b = *b"C:\\root\\";
        let mut n = 8;
        normalize_path(&mut b, &mut n);
        assert_eq!(&b[..n], b"C:\\root\\");
    }

    #[test]
    fn normalize_shortens_double_trailing() {
        let mut b = *b"C:\\root\\\\";
        let mut n = 9;
        normalize_path(&mut b, &mut n);
        assert_eq!(&b[..n], b"C:\\root\\");
    }

    #[test]
    fn reserved_names_case_insensitive() {
        assert!(dir_entry_ok("C:\\con", true).is_err());
        assert!(dir_entry_ok("C:\\Aux", true).is_err());
        assert!(dir_entry_ok("C:\\console-app", true).is_ok()); // 非整名不误伤
    }

    #[test]
    fn tabs_persist_empty_slot_survives() {
        let mut t = TermDirs::new();
        t.new_tab(DirPath::new("C:\\one").unwrap());
        let mut buf = [0u8; 1024];
        let n = save_tabs(&t.tabs, t.tab_n, &mut buf).unwrap();
        let (tabs, tn) = load_tabs(&buf[..n]).unwrap();
        assert_eq!(tn, 1);
        assert!(tabs[0].is_some());
        assert!(tabs[1].is_none());
    }
}
'''

BLOCKS["unitconv"] = r'''
// ===========================================================================
// 深化 v2（F458）：复合表达式换算 / 双向换算自检 / 单位别名扩充 / 持久化
// ===========================================================================

/// 复合换算（链式：「5km in m in mi」类两跳——按序复合）。
pub fn convert_chain(v: f64, hops: &[&str]) -> Option<f64> {
    let mut val = v;
    let mut i = 0;
    while i + 1 < hops.len() {
        val = convert(val, hops[i], hops[i + 1])?;
        i += 1;
    }
    Some(val)
}

/// 双向换算自检（round-trip 精度守护：a→b→a 偏差 ≤1e-6 相对值）。
pub fn roundtrip_ok(v: f64, a: &str, b: &str) -> bool {
    match (convert(v, a, b), convert(v, a, b).and_then(|x| convert(x, b, a))) {
        (Some(_), Some(back)) => (back - v).abs() <= v.abs() * 1e-6 + 1e-9,
        _ => false,
    }
}

/// 单位别名扩充（口语别名 → 册内单位名——「180 磅多重」的口语面）。
pub fn alias_resolve(word: &str) -> Option<&'static str> {
    const ALIASES: [(&str, &str); 10] = [
        ("斤", "kg"), ("公斤", "kg"), ("千米", "km"), ("英里", "mi"),
        ("厘米", "cm2fake"), ("加仑", "gal"), ("迈", "kmh"), ("码", "kg2fake"),
        ("摄氏", "c"), ("华氏", "f"),
    ];
    // 仅映射册内单位（fake 后缀 = 口语存在但册内未收——诚实返回 None）。
    ALIASES.iter().find(|(w, _)| *w == word).and_then(|(_, u)| {
        if UNITS.iter().any(|unit| unit.names.contains(u)) {
            Some(UNITS.iter().find(|unit| unit.names.contains(u)).unwrap().names[0])
        } else {
            None
        }
    })
}

/// 换算历史（最近 8 次查询——搜索框回看「刚才算过什么」）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConvHistoryEntry {
    pub value: f64,
    pub from: &'static str,
    pub to: &'static str,
}

pub struct ConvHistory {
    ring: [Option<ConvHistoryEntry>; 8],
    head: usize,
    n: usize,
}

impl ConvHistory {
    pub const fn new() -> Self {
        ConvHistory { ring: [None; 8], head: 0, n: 0 }
    }

    pub fn push(&mut self, e: ConvHistoryEntry) {
        self.ring[self.head] = Some(e);
        self.head = (self.head + 1) % 8;
        self.n = (self.n + 1).min(8);
    }

    pub fn latest(&self) -> Option<ConvHistoryEntry> {
        if self.n == 0 {
            return None;
        }
        let idx = (self.head + 8 - 1) % 8;
        self.ring[idx]
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

/// 区域默认持久化（用户改过区域制式——跨重启记住）。
pub const PERSIST_MAGIC: [u8; 4] = *b"VUC1";

pub fn save_region(region: &RegionDefaults, out: &mut [u8]) -> Option<usize> {
    if out.len() < 4 + 3 * 8 {
        return None;
    }
    out[..4].copy_from_slice(&PERSIST_MAGIC);
    let mut w = 4;
    for (slot, name) in [(0, region.length), (1, region.weight), (2, region.temperature)] {
        out[w..w + 8].copy_from_slice(persist_name(name));
        w += 8;
    }
    let _ = slot;
    Some(w)
}

fn persist_name(name: &str) -> [u8; 8] {
    let mut b = [0u8; 8];
    for (i, c) in name.bytes().take(8).enumerate() {
        b[i] = c;
    }
    b
}

fn load_name(b: &[u8]) -> Option<&'static str> {
    let end = b.iter().position(|&c| c == 0).unwrap_or(8);
    let s = core::str::from_utf8(&b[..end]).ok()?;
    UNITS.iter().find(|u| u.names.contains(&s)).map(|u| u.names[0])
}

pub fn load_region(buf: &[u8]) -> Option<RegionDefaults> {
    if buf.len() < 28 || buf[..4] != PERSIST_MAGIC {
        return None;
    }
    Some(RegionDefaults {
        length: load_name(&buf[4..12])?,
        weight: load_name(&buf[12..20])?,
        temperature: load_name(&buf[20..28])?,
    })
}

pub fn run_unitconv_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F458-deep");
    // 复合链式换算（km→m→mi 两跳与直连一致——复合不漂移）。
    cs.add("chain_two_hop", (convert_chain(5.0, &["km", "m", "mi"]).unwrap() - convert(5.0, "km", "mi").unwrap()).abs() < 1e-9, "");
    cs.add("chain_bad_hop_honest", convert_chain(5.0, &["km", "kg", "mi"]).is_none(), "");
    // 双向 round-trip（六族各验一对——精度守护）。
    cs.add("roundtrip_all_families", ["km", "lb", "c", "acre", "gal", "mph"].iter().all(|&u| roundtrip_ok(42.0, u, base_of(u))), "");
    // 口语别名（册内映射、册外诚实 None）。
    cs.add("alias_known", alias_resolve("公斤") == Some("kg"), "");
    cs.add("alias_unknown_honest", alias_resolve("码").is_none(), "");
    // 换算历史（最近 8 条、latest 命中）。
    cs.add("history_latest", {
        let mut h = ConvHistory::new();
        h.push(ConvHistoryEntry { value: 1.0, from: "km", to: "mi" });
        h.push(ConvHistoryEntry { value: 2.0, from: "lb", to: "kg" });
        h.latest() == Some(ConvHistoryEntry { value: 2.0, from: "lb", to: "kg" }) && h.count() == 2
    }, "");
    cs.add("history_empty_honest", ConvHistory::new().latest().is_none(), "");
    // 区域默认持久化 round-trip + 未知单位拒收。
    cs.add("region_persist", {
        let mut buf = [0u8; 32];
        let n = save_region(&REGION_ZH, &mut buf).unwrap();
        load_region(&buf[..n]) == Some(REGION_ZH)
    }, "");
    cs.add("region_persist_bad_unit", load_region(&[b'V', b'U', b'C', b'1', b'x', 0,0,0,0,0,0,0, b'k', b'g', 0,0,0,0,0,0, b'c', 0,0,0,0,0,0,0]).is_none(), "");
    cs
}

fn base_of(unit: &str) -> &'static str {
    let u = find_unit(unit).unwrap();
    let family = u.family;
    UNITS.iter().find(|x| x.family == family && x.is_base_target).unwrap().names[0]
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn roundtrip_temperature_exact() {
        assert!(roundtrip_ok(36.6, "c", "f"));
        assert!(roundtrip_ok(0.0, "c", "k"));
    }

    #[test]
    fn chain_of_one_is_identity() {
        assert_eq!(convert_chain(7.5, &["km"]), Some(7.5));
    }

    #[test]
    fn history_ring_wraps() {
        let mut h = ConvHistory::new();
        for i in 0..12 {
            h.push(ConvHistoryEntry { value: i as f64, from: "km", to: "mi" });
        }
        assert_eq!(h.count(), 8);
        assert_eq!(h.latest().unwrap().value, 11.0);
    }

    #[test]
    fn region_roundtrip_all_known_units() {
        let mut buf = [0u8; 32];
        let n = save_region(&REGION_ZH, &mut buf).unwrap();
        let r = load_region(&buf[..n]).unwrap();
        assert!(find_unit(r.length).is_some() && find_unit(r.weight).is_some() && find_unit(r.temperature).is_some());
    }
}
'''

BLOCKS["chantest"] = r'''
// ===========================================================================
// 深化 v2（F480）：测试音序列发生器 / 声道路由账 / 无声判定窗 / 结果报告
// ===========================================================================

/// 测试音配置（440Hz 标准音 + 500ms 时长——左右独立发声的声学参数）。
pub const TEST_TONE_HZ: u16 = 440;
pub const TEST_TONE_MS: u64 = 500;

/// 音序列发生器（左右交替节拍——双声道同时测试的时序面）。
pub struct ToneSeq {
    step: u8,
    steps_left: u8,
}

impl ToneSeq {
    pub const fn new(steps: u8) -> Self {
        ToneSeq { step: 0, steps_left: steps }
    }

    /// 下一发声声道（Left→Right→Both 循环；步尽 → None——测试有终点）。
    pub fn next_channel(&mut self) -> Option<Channel> {
        if self.steps_left == 0 {
            return None;
        }
        self.steps_left -= 1;
        let ch = match self.step % 3 {
            0 => Channel::Left,
            1 => Channel::Right,
            _ => Channel::Both,
        };
        self.step += 1;
        Some(ch)
    }

    pub fn remaining(&self) -> u8 {
        self.steps_left
    }
}

/// 声道路由账（每声道一次验证记录——「测试的就是当前在用的那个设备」
/// 的可审计面）。
pub struct RouteLog {
    entries: [(u8, u32, bool); 8], // (声道 id, 设备 ack, 是否匹配)
    n: usize,
}

impl RouteLog {
    pub const fn new() -> Self {
        RouteLog { entries: [(0, 0, false); 8], n: 0 }
    }

    pub fn record(&mut self, ch: Channel, ack_device: Option<u32>, expect_device: u32) {
        if self.n >= 8 {
            return;
        }
        let ch_id = match ch {
            Channel::Left => 0,
            Channel::Right => 1,
            Channel::Both => 2,
        };
        self.entries[self.n] = (ch_id, ack_device.unwrap_or(0), ack_device == Some(expect_device));
        self.n += 1;
    }

    pub fn all_matched(&self) -> bool {
        self.n >= 3 && (0..self.n).all(|i| self.entries[i].2)
    }

    pub fn count(&self) -> usize {
        self.n
    }
}

/// 无声判定窗（发声后 500ms 内无 ack = 无声——诊断提示的触发时钟）。
pub const SILENCE_WINDOW_MS: u64 = 500;

pub fn is_silent(played_at_ms: u64, ack_at_ms: Option<u64>) -> bool {
    match ack_at_ms {
        Some(t) => t.saturating_sub(played_at_ms) > SILENCE_WINDOW_MS,
        None => true,
    }
}

/// 结果报告结构（测试完成后的可读结论——三声道各一行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TestReport {
    pub left_ok: bool,
    pub right_ok: bool,
    pub both_ok: bool,
    pub volume_permille: u16,
}

impl TestReport {
    pub fn all_pass(&self) -> bool {
        self.left_ok && self.right_ok && self.both_ok
    }

    /// 人话结论（全过/哪边无声——三要素口径）。
    pub fn conclusion(&self) -> &'static str {
        if self.all_pass() {
            "左右声道测试通过"
        } else if !self.left_ok {
            ChannelTest::silent_diagnosis(Channel::Left)
        } else if !self.right_ok {
            ChannelTest::silent_diagnosis(Channel::Right)
        } else {
            ChannelTest::silent_diagnosis(Channel::Both)
        }
    }
}

pub fn run_chantest_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new("F480-deep");
    // 音序列（左右双循环——步尽诚实终止）。
    cs.add("tone_seq_cycle", {
        let mut s = ToneSeq::new(6);
        let seq = [s.next_channel(), s.next_channel(), s.next_channel(), s.next_channel(), s.next_channel(), s.next_channel()];
        seq == [Some(Channel::Left), Some(Channel::Right), Some(Channel::Both), Some(Channel::Left), Some(Channel::Right), Some(Channel::Both)]
            && s.next_channel().is_none()
            && s.remaining() == 0
    }, "");
    // 音参数（440Hz/500ms 在册——测试音不炸耳的声学锚）。
    cs.add("tone_params", TEST_TONE_HZ == 440 && TEST_TONE_MS == 500, "");
    // 路由账（三声道全匹配才算过——单边通不算通）。
    cs.add("route_log_all_pass", {
        let mut r = RouteLog::new();
        r.record(Channel::Left, Some(7), 7);
        r.record(Channel::Right, Some(7), 7);
        r.record(Channel::Both, Some(7), 7);
        r.all_matched()
    }, "");
    cs.add("route_log_one_fail", {
        let mut r = RouteLog::new();
        r.record(Channel::Left, Some(7), 7);
        r.record(Channel::Right, Some(9), 7); // ack 设备不对
        r.record(Channel::Both, Some(7), 7);
        !r.all_matched()
    }, "");
    // 无声判定窗（500ms 无 ack = 无声——诊断触发时钟）。
    cs.add("silence_window", is_silent(0, None) && !is_silent(0, Some(499)) && is_silent(0, Some(501)), "");
    // 结果报告（全过人话/左无声人话——三要素结论）。
    cs.add("report_all_pass", {
        TestReport { left_ok: true, right_ok: true, both_ok: true, volume_permille: 400 }.conclusion() == "左右声道测试通过"
    }, "");
    cs.add("report_left_silent", {
        TestReport { left_ok: false, right_ok: true, both_ok: true, volume_permille: 400 }.conclusion() == "左声道无声——检查接口或换设备"
    }, "");
    cs
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn seq_partial_run_stops_cleanly() {
        let mut s = ToneSeq::new(2);
        assert_eq!(s.next_channel(), Some(Channel::Left));
        assert_eq!(s.next_channel(), Some(Channel::Right));
        assert_eq!(s.next_channel(), None);
        assert_eq!(s.remaining(), 0);
    }

    #[test]
    fn route_log_caps_at_8() {
        let mut r = RouteLog::new();
        for _ in 0..10 {
            r.record(Channel::Left, Some(1), 1);
        }
        assert_eq!(r.count(), 8);
    }

    #[test]
    fn silence_boundary_exact() {
        assert!(!is_silent(1_000, Some(1_000 + SILENCE_WINDOW_MS)));
        assert!(is_silent(1_000, Some(1_000 + SILENCE_WINDOW_MS + 1)));
    }

    #[test]
    fn report_both_silent_falls_to_both_message() {
        let r = TestReport { left_ok: true, right_ok: true, both_ok: false, volume_permille: 400 };
        assert_eq!(r.conclusion(), "双声道无声——检查音量、接口或输出设备");
    }
}
'''

def main():
    for mod, src in BLOCKS.items():
        path = os.path.join(BASE, mod + ".rs")
        with io.open(path, "r", encoding="utf-8") as f:
            content = f.read()
        if "深化 v2" in content:
            print(f"SKIP {mod}")
            continue
        with io.open(path, "a", encoding="utf-8", newline="") as f:
            f.write(src)
        print(f"APPEND {mod}: +{src.count(chr(10))} lines")

    mod_path = os.path.join(BASE, "mod.rs")
    with io.open(mod_path, "r", encoding="utf-8") as f:
        s = f.read()
    pairs = [
        ("F467", "termfont", "run_termfont_checks"),
        ("F466", "termclip", "run_termclip_checks"),
        ("F470", "termdir", "run_termdir_checks"),
        ("F458", "unitconv", "run_unitconv_checks"),
        ("F480", "chantest", "run_chantest_checks"),
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
    print(f"WIRED {changed}")

if __name__ == "__main__":
    main()
