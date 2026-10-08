//! F029 深化批次四 · 拓扑变更检测面（compatstar2/deep3 · G-A-29）。
//!
//! 批次一~三覆盖枚举结构/接口冻结/单屏模式档等程序可见面；本批补齐
//! 主册【功能定义】「全语义对齐」的序列化/账本/容错面：拓扑快照对比
//! （4 屏快照 old/new：增/减/主屏变更/重排四类事件分类）、热插拔事件
//! 分类器（单屏接入/拔出/主副切换，未知组合显性记账）、显示配置持久化
//! 匹配（按拓扑指纹查表，定长 8 历史，未命中走默认并记账）、回退链
//! （精确匹配→同分辨率匹配→默认三段降级，逐段记账）。判据对账：主册
//! G-A-29【设计细节】枚举结构预留显示器数组 + MS EnumDisplayMonitors/
//! ChangeDisplaySettings 文档语义对拍（MONITORINFO 字段面同构；指纹为
//! 拓扑确定性摘要——域内口径）。零堆纪律：定长 4 屏快照 + 定长 8 历史，
//! 无 Vec/String/Box/format!，错误一律 Err 或计数账面，零静默。

use crate::checks::CheckSet;

/// 快照容量（4 屏预留——多屏期扩容不动契约）。
pub const MAX_MONITORS: usize = 4;
/// 显示配置历史长度（定长 8）。
pub const HISTORY_LEN: usize = 8;
/// 默认配置号（回退链终点）。
pub const DEFAULT_CONFIG: u32 = 0;
/// FNV-1a 32 位偏移基/素数（拓扑指纹确定性摘要——域内口径）。
pub const FNV_OFFSET_BASIS: u32 = 0x811C_9DC5;
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 一台显示器的快照（MS MONITORINFO 字段面同构：present/primary/几何）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MonInfo {
    pub present: bool,
    pub primary: bool,
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl MonInfo {
    /// 缺席位。
    pub const fn absent() -> Self {
        MonInfo { present: false, primary: false, x: 0, y: 0, w: 0, h: 0 }
    }

    /// 在席位。
    pub const fn on(x: i32, y: i32, w: u32, h: u32, primary: bool) -> Self {
        MonInfo { present: true, primary, x, y, w, h }
    }
}

/// 一次拓扑快照（4 屏定长）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Snapshot {
    pub mons: [MonInfo; MAX_MONITORS],
}

impl Snapshot {
    pub const fn empty() -> Self {
        Snapshot { mons: [MonInfo::absent(); MAX_MONITORS] }
    }

    /// 拓扑指纹：FNV-1a 逐屏压入 present/primary/几何（确定性、无堆）。
    pub fn fingerprint(&self) -> u32 {
        let mut h = FNV_OFFSET_BASIS;
        for m in self.mons.iter() {
            for v in [m.present as u32, m.primary as u32, m.x as u32, m.y as u32, m.w, m.h] {
                for byte in v.to_le_bytes() {
                    h ^= byte as u32;
                    h = h.wrapping_mul(FNV_PRIME);
                }
            }
        }        h
    }
}

// 快照对比与热插拔分类 -------------------------------------------------------

/// 拓扑事件四类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TopoEvent {
    Added,
    Removed,
    PrimaryChanged,
    Rearranged,
}

/// 对比日志（每屏至多一事件；同一屏双变更按主屏变更归类——上层语义优先）。
pub struct DiffLog {
    pub events: [Option<TopoEvent>; MAX_MONITORS],
    pub n: usize,
}

/// 快照对比：old/new 逐屏分类 增/减/主屏变更/重排；无变化零事件。
pub fn diff_topology(old: &Snapshot, new: &Snapshot) -> DiffLog {
    let mut out = DiffLog { events: [None; MAX_MONITORS], n: 0 };
    for i in 0..MAX_MONITORS {
        let (o, n) = (old.mons[i], new.mons[i]);
        let ev = match (o.present, n.present) {
            (false, true) => Some(TopoEvent::Added),
            (true, false) => Some(TopoEvent::Removed),
            (true, true) if o.primary != n.primary => Some(TopoEvent::PrimaryChanged),
            (true, true) if o.x != n.x || o.y != n.y || o.w != n.w || o.h != n.h => Some(TopoEvent::Rearranged),
            _ => None,
        };
        if let Some(e) = ev {
            out.events[i] = Some(e);
            out.n += 1;
        }
    }
    out
}

/// 热插拔分类四态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HotplugKind {
    SingleConnect,
    SingleDisconnect,
    PrimarySwap,
    Unknown,
}

/// 热插拔事件分类器：单接入/单拔出/主副切换；未知组合显性记账。
#[derive(Clone, Copy)]
pub struct HotplugLedger {
    pub connect: u32,
    pub disconnect: u32,
    pub primary_swap: u32,
    pub unknown: u32,
}

impl HotplugLedger {
    pub const fn new() -> Self {
        HotplugLedger { connect: 0, disconnect: 0, primary_swap: 0, unknown: 0 }
    }

    /// 分类一笔对比日志并记账。主副切换 = 恰两屏 primary 互换（一失一得）。
    pub fn classify(&mut self, d: &DiffLog) -> HotplugKind {
        let all = |k: TopoEvent| d.events.iter().take(d.n).all(|e| *e == Some(k));
        if d.n == 1 && all(TopoEvent::Added) {
            self.connect += 1;
            return HotplugKind::SingleConnect;
        }
        if d.n == 1 && all(TopoEvent::Removed) {
            self.disconnect += 1;
            return HotplugKind::SingleDisconnect;
        }
        if d.n == 2 && all(TopoEvent::PrimaryChanged) {
            self.primary_swap += 1;
            return HotplugKind::PrimarySwap;
        }
        self.unknown += 1;
        HotplugKind::Unknown
    }
}

// 显示配置持久化与回退链 ------------------------------------------------------

/// 一条历史项（拓扑指纹 + 主屏分辨率 + 配置号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HistEntry {
    pub fp: u32,
    pub w: u32,
    pub h: u32,
    pub cfg: u32,
}

/// 定长 8 历史：满则淘汰最旧（左移——账面自洽）。
pub struct ConfigHistory {
    pub entries: [Option<HistEntry>; HISTORY_LEN],
    pub count: usize,
}

impl ConfigHistory {
    pub const fn new() -> Self {
        ConfigHistory { entries: [None; HISTORY_LEN], count: 0 }
    }

    /// 记住一笔：同指纹覆盖（配置更新语义）；满则左移淘汰最旧。
    pub fn remember(&mut self, fp: u32, w: u32, h: u32, cfg: u32) {
        for slot in self.entries.iter_mut() {
            if let Some(e) = slot {
                if e.fp == fp { *e = HistEntry { fp, w, h, cfg }; return; }
            }
        }
        if self.count == HISTORY_LEN {
            for i in 1..HISTORY_LEN { self.entries[i - 1] = self.entries[i]; }
            self.count = HISTORY_LEN - 1;
        }
        self.entries[self.count] = Some(HistEntry { fp, w, h, cfg });
        self.count += 1;
    }

    /// 回退链第一段：精确指纹匹配。
    pub fn exact(&self, fp: u32) -> Option<u32> {
        self.entries.iter().flatten().find(|e| e.fp == fp).map(|e| e.cfg)
    }

    /// 回退链第二段：主屏同分辨率匹配。
    pub fn same_resolution(&self, w: u32, h: u32) -> Option<u32> {
        self.entries.iter().flatten().find(|e| e.w == w && e.h == h).map(|e| e.cfg)
    }
}

/// 回退链逐段记账。
#[derive(Clone, Copy, Default)]
pub struct ResolveLedger {
    pub exact_hits: u32,
    pub resolution_hits: u32,
    pub default_falls: u32,
}

/// 一次解析结果（阶段名 + 配置号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResolveOutcome {
    pub stage: &'static str,
    pub cfg: u32,
}

/// 回退链：精确匹配 → 同分辨率匹配（新快照主屏分辨率）→ 默认，逐段记账。
pub fn resolve(hist: &ConfigHistory, led: &mut ResolveLedger, snap: &Snapshot) -> ResolveOutcome {
    let fp = snap.fingerprint();
    if let Some(cfg) = hist.exact(fp) {
        led.exact_hits += 1;
        return ResolveOutcome { stage: "exact", cfg };
    }
    let primary = snap.mons.iter().find(|m| m.present && m.primary);
    if let Some(cfg) = primary.and_then(|m| hist.same_resolution(m.w, m.h)) {
        led.resolution_hits += 1;
        return ResolveOutcome { stage: "resolution", cfg };
    }
    led.default_falls += 1;
    ResolveOutcome { stage: "default", cfg: DEFAULT_CONFIG }
}

/// 域自检（F029 深化批次四 · 快照对比/分类/持久化/回退链）。
pub fn run_f029f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F029-topology-d4");
    // 1) 增事件：空 → 单屏在席。
    let old = Snapshot::empty();
    let mut new = Snapshot::empty();
    new.mons[0] = MonInfo::on(0, 0, 1920, 1080, true);
    let d = diff_topology(&old, &new);
    cs.add("diff_add_detected", d.n == 1 && d.events[0] == Some(TopoEvent::Added), "");
    // 2) 减事件：单屏在席 → 空。
    let d = diff_topology(&new, &old);
    cs.add("diff_remove_detected", d.n == 1 && d.events[0] == Some(TopoEvent::Removed), "");
    // 3) 主屏变更：几何不变、primary 翻转（两屏互换）。
    let mut a = Snapshot::empty();
    let mut b = Snapshot::empty();
    a.mons[0] = MonInfo::on(0, 0, 1920, 1080, true);
    a.mons[1] = MonInfo::on(1920, 0, 1920, 1080, false);
    b.mons[0] = MonInfo::on(0, 0, 1920, 1080, false);
    b.mons[1] = MonInfo::on(1920, 0, 1920, 1080, true);
    let d = diff_topology(&a, &b);
    cs.add("diff_primary_changed", d.n == 2 && d.events[0] == Some(TopoEvent::PrimaryChanged), "");
    // 4) 重排：在席屏位移。
    let mut moved = Snapshot::empty();
    moved.mons[0] = MonInfo::on(0, 0, 1920, 1080, true);
    let mut shifted = Snapshot::empty();
    shifted.mons[0] = MonInfo::on(-1920, 0, 1920, 1080, true);
    let d = diff_topology(&moved, &shifted);
    cs.add("diff_rearranged", d.n == 1 && d.events[0] == Some(TopoEvent::Rearranged), "");
    // 5) 无变化零事件（零事件是合法态，与未知组合区分）。
    let d = diff_topology(&moved, &moved);
    cs.add("diff_no_change_zero_events", d.n == 0, "");
    // 6) 热插拔单接入/单拔出分类并记账。
    let mut led = HotplugLedger::new();
    let k1 = led.classify(&diff_topology(&old, &new));
    let k2 = led.classify(&diff_topology(&new, &old));
    cs.add("hotplug_connect_disconnect", k1 == HotplugKind::SingleConnect && k2 == HotplugKind::SingleDisconnect && led.connect == 1 && led.disconnect == 1, "");
    // 7) 主副切换：两屏 primary 互换 → PrimarySwap。
    let mut led2 = HotplugLedger::new();
    let k = led2.classify(&diff_topology(&a, &b));
    cs.add("hotplug_primary_swap", k == HotplugKind::PrimarySwap && led2.primary_swap == 1, "");
    // 8) 未知组合显性记账：另一屏分辨率重排（非纯接入/拔出/互换）。
    let mut mixed = Snapshot::empty();
    mixed.mons[0] = MonInfo::on(0, 0, 1920, 1080, true);
    mixed.mons[1] = MonInfo::on(1920, 0, 1920, 1080, false);
    let mut mixed2 = Snapshot::empty();
    mixed2.mons[0] = MonInfo::on(0, 0, 1920, 1080, true);
    mixed2.mons[1] = MonInfo::on(3840, 0, 2560, 1440, false);
    let k = led2.classify(&diff_topology(&mixed, &mixed2));
    cs.add("hotplug_unknown_ledger", k == HotplugKind::Unknown && led2.unknown == 1, "");
    // 9) 持久化精确匹配：记住指纹 → resolve 走 exact 段。
    let mut hist = ConfigHistory::new();
    let mut led3 = ResolveLedger::default();
    hist.remember(new.fingerprint(), 1920, 1080, 7);
    let out = resolve(&hist, &mut led3, &new);
    cs.add("history_exact_hit", out == ResolveOutcome { stage: "exact", cfg: 7 } && led3.exact_hits == 1, "");
    // 10) 回退第二段：指纹未命中、主屏同分辨率 → resolution 段。
    let mut other = Snapshot::empty();
    other.mons[0] = MonInfo::on(100, 50, 1920, 1080, true);
    let out = resolve(&hist, &mut led3, &other);
    cs.add("fallback_resolution_stage", out == ResolveOutcome { stage: "resolution", cfg: 7 } && led3.resolution_hits == 1, "");
    // 11) 回退终点：全未命中 → 默认配置并记账。
    let mut odd = Snapshot::empty();
    odd.mons[0] = MonInfo::on(0, 0, 1366, 768, true);
    let out = resolve(&hist, &mut led3, &odd);
    cs.add("fallback_default_ledger", out == ResolveOutcome { stage: "default", cfg: DEFAULT_CONFIG } && led3.default_falls == 1, "");
    // 12) 历史定长 8 环形淘汰：第 9 笔挤掉最旧。
    let mut ring = ConfigHistory::new();
    for i in 0..9u32 {
        ring.remember(0xA000 + i, 1920, 1080, i);
    }
    cs.add("history_ring_eviction", ring.count == HISTORY_LEN && ring.exact(0xA000).is_none() && ring.exact(0xA008) == Some(8), "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_changes_with_geometry() {
        // 指纹确定性：同拓扑同指纹；位移即变。
        let mut s1 = Snapshot::empty();
        s1.mons[0] = MonInfo::on(0, 0, 1920, 1080, true);
        let s2 = s1;
        assert_eq!(s1.fingerprint(), s2.fingerprint());
        let mut s3 = s1;
        s3.mons[0].x = -1920;
        assert_ne!(s1.fingerprint(), s3.fingerprint());
    }

    #[test]
    fn diff_primary_takes_priority_over_rearrange() {
        // 同屏 primary 翻转 + 位移同时发生 → 归主屏变更（上层语义优先）。
        let mut a = Snapshot::empty();
        let mut b = Snapshot::empty();
        a.mons[0] = MonInfo::on(0, 0, 1920, 1080, true);
        b.mons[0] = MonInfo::on(-1920, 0, 1920, 1080, false);
        let d = diff_topology(&a, &b);
        assert_eq!(d.n, 1);
        assert_eq!(d.events[0], Some(TopoEvent::PrimaryChanged));
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f029f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
