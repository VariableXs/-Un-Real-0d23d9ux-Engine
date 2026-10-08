//! F242 网络状态指示与诊断 · 判据实装（H 基础通用域）。
//!
//! **判据锚**：F242（主册 H-1 深化设计报告 · H 基础通用域）。
//!
//! **验收标准（主册第一句）**：任务栏右下网络图标三态（正常/受限/断开）
//! 一眼可辨；受限态（连上但无网）图标带黄色感叹号——「连上了但上不了
//! 网」和「没连上」是两回事必须分开报；点击图标呼出网络浮层（可见网络
//! 列表+信号强度+一键连接，密码框规范）；「诊断」入口一条命令产出人话
//! 报告（「网线通了、路由器通了、DNS 不通」三级定位）而非天书日志。
//!
//! **设计要点**：
//! - 三级探测状态机：线缆→网关→DNS 三个独立探测点，逐级归因——
//!   状态由探测位推导（`derive_phase` 纯函数，fuzz 可直测）：
//!   线缆断=断开；线缆通但网关/DNS 断=受限（细分 NoGateway/NoDns）；
//!   全通=正常；探测中=Unknown；
//! - 受限细分是**枚举级**的：`Limited(LimitCause::NoGateway / NoDns)`
//!   各有独立图标字形（黄色感叹号 + 下标），「连上但上不了网」与
//!   「没连上」在类型上就是两个值；
//! - 刷新延迟账本：探测点状态变化 → 任务栏图标应用的延迟实测入环，
//!   <2s 判定（主册「状态刷新延迟 <2s」）；
//! - 人话诊断：`diagnose()` 产出三级定位报告（网线/路由器/DNS 各一行
//!   人话 + 一句结论），文案是人话不是天书；三场景（断线缆/断网关/
//!   断 DNS）各有固定模板；
//! - 网络浮层模型：可见网络列表（定容 8 槽）+ 信号五档 + 连接流程
//!   状态机（列表→密码→连接→结果），焦点序固定单调——Tab/Enter 全程
//!   可达（键盘可达判据）；
//! - 探测打点：三级探测耗时各自进定容延迟环（诊断面：P95 最近邻）；
//! - 操作面分钟账本 [探测/诊断/连接/刷新] 四计数器，保留 30 天。
//!
//! **依赖锚点**：[`crate::star::sbase::{MinuteBook, RingLog,
//! pct_near}`]；时间一律注入毫秒戳。

use crate::checks::CheckSet;
use crate::star::sbase::{pct_near, MinuteBook, RingLog};

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 状态刷新判定门——主册 F242「状态刷新延迟 <2s」。
pub const REFRESH_LIMIT_MS: u64 = 2_000;

/// 刷新延迟样本下限（样本不足不下结论，数据诚实纪律）。
pub const REFRESH_MIN_SAMPLES: usize = 8;

/// 可见网络列表容量（浮层一屏可视，8 槽定容）。
pub const NETLIST_CAP: usize = 8;

/// 信号档数——主册 F242「信号强度五档」。
pub const SIGNAL_BARS_MAX: u8 = 5;

/// 密码输入缓冲（浮层密码框，定容）。
pub const PSK_CAP: usize = 64;

/// 探测周期设计值（周期探测驱动的基准节拍，判定不受此值影响）。
pub const PROBE_INTERVAL_MS: u64 = 10_000;

/// 账本列：0 探测 / 1 诊断 / 2 连接 / 3 刷新。
pub const LEDGER_COLS: usize = 4;

// ---------------------------------------------------------------------------
// 数据面
// ---------------------------------------------------------------------------

/// 三级探测点（逐级归因的锚）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeLevel {
    /// 线缆/链路层（能不能到路由器）。
    Cable,
    /// 网关（能不能出内网）。
    Gateway,
    /// DNS（能不能解析域名）。
    Dns,
}

impl ProbeLevel {
    /// 账本/数组下标。
    pub fn idx(&self) -> usize {
        match self {
            ProbeLevel::Cable => 0,
            ProbeLevel::Gateway => 1,
            ProbeLevel::Dns => 2,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            ProbeLevel::Cable => "网线",
            ProbeLevel::Gateway => "路由器",
            ProbeLevel::Dns => "DNS",
        }
    }
}

/// 单级探测结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeState {
    Pass,
    Fail,
    /// 尚未探测到该级（前级中断或刚开机）。
    Unknown,
}

/// 受限细分原因（「连上但上不了网」的两条不同路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitCause {
    /// 网关不通：包出不了内网。
    NoGateway,
    /// DNS 不通：能出网但域名解析不了。
    NoDns,
}

/// 网络相位（任务栏图标三态 + 受限细分）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetPhase {
    /// 正常：三级全通。
    Normal,
    /// 受限：连上但无网（带细分原因——黄色感叹号 + 下标字形）。
    Limited(LimitCause),
    /// 断开：线缆/链路层不通。
    Down,
}

/// 图标字形编码（三态 + 受限细分走查的判定面；渲染面按编码取图）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconGlyph {
    /// 正常：实心联网图标。
    Connected,
    /// 受限·网关断：黄色感叹号 + 下标 1。
    LimitedGw,
    /// 受限·DNS 断：黄色感叹号 + 下标 2。
    LimitedDns,
    /// 断开：空心/叉形图标。
    Disconnected,
}

/// 由三级探测位推导网络相位（纯函数——fuzz 不变量锚）。
pub fn derive_phase(cable: ProbeState, gw: ProbeState, dns: ProbeState) -> NetPhase {
    if cable != ProbeState::Pass {
        return NetPhase::Down;
    }
    match gw {
        ProbeState::Pass => match dns {
            ProbeState::Pass => NetPhase::Normal,
            ProbeState::Fail => NetPhase::Limited(LimitCause::NoDns),
            ProbeState::Unknown => NetPhase::Limited(LimitCause::NoDns),
        },
        ProbeState::Fail => NetPhase::Limited(LimitCause::NoGateway),
        ProbeState::Unknown => NetPhase::Limited(LimitCause::NoGateway),
    }
}

/// 相位 → 图标字形（三态+受限细分走查的唯一判定口）。
pub fn icon_glyph(phase: &NetPhase) -> IconGlyph {
    match phase {
        NetPhase::Normal => IconGlyph::Connected,
        NetPhase::Limited(LimitCause::NoGateway) => IconGlyph::LimitedGw,
        NetPhase::Limited(LimitCause::NoDns) => IconGlyph::LimitedDns,
        NetPhase::Down => IconGlyph::Disconnected,
    }
}

/// 受限与断开必须可辨（「两回事必须分开报」的类型级保证）。
pub fn limited_vs_down_distinct(a: &NetPhase, b: &NetPhase) -> bool {
    matches!(a, NetPhase::Limited(_)) && matches!(b, NetPhase::Down)
        || matches!(a, NetPhase::Down) && matches!(b, NetPhase::Limited(_))
        || icon_glyph(a) != icon_glyph(b)
}

/// 单级探测槽。
#[derive(Clone, Copy, Debug)]
pub struct ProbeSlot {
    pub state: ProbeState,
    /// 最近一次探测时刻（ms）。
    pub last_ms: u64,
}

/// 状态变化留痕（新→旧环）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StateChange {
    pub phase: NetPhase,
    pub at_ms: u64,
}

// ---------------------------------------------------------------------------
// 人话诊断报告
// ---------------------------------------------------------------------------

/// 诊断结论（一句话人话标题）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiagVerdict {
    /// 全通：网络正常。
    Ok,
    /// 没连上：线缆/链路断。
    CableDown,
    /// 连上了但上不了网：路由器（网关）不通。
    GatewayDown,
    /// 连上了但上不了网：DNS 不通。
    DnsDown,
    /// 探测中：数据不足，先别下结论。
    Probing,
}

/// 人话诊断报告：三级逐行 + 一句结论（「一条命令产出人话报告」）。
#[derive(Clone, Debug)]
pub struct DiagReport {
    pub verdict: DiagVerdict,
    /// 三级行（每行「XX：通/不通/未测」+ 短注）。
    pub lines: String,
    /// 生成时刻（ms，注入）。
    pub stamp_ms: u64,
}

impl DiagReport {
    /// 人话结论一句话（报告首行）。
    pub fn headline(&self) -> &'static str {
        match self.verdict {
            DiagVerdict::Ok => "网络正常",
            DiagVerdict::CableDown => "没连上：网线/链路不通",
            DiagVerdict::GatewayDown => "连上了但上不了网：路由器不通",
            DiagVerdict::DnsDown => "连上了但上不了网：域名解析（DNS）不通",
            DiagVerdict::Probing => "正在探测，暂不下结论",
        }
    }

    /// 三级定位关键词齐备性（人话而非天书的最低要求）。
    pub fn mentions_all_levels(&self) -> bool {
        self.lines.contains("网线") && self.lines.contains("路由器") && self.lines.contains("DNS")
    }
}

/// 生成诊断报告（纯函数：由三级探测位产出人话模板）。
pub fn diagnose(cable: ProbeState, gw: ProbeState, dns: ProbeState, now_ms: u64) -> DiagReport {
    let line = |name: &str, st: ProbeState, note: &str| -> String {
        let word = match st {
            ProbeState::Pass => "通",
            ProbeState::Fail => "不通",
            ProbeState::Unknown => "未测",
        };
        format!("{}：{}（{}）", name, word, note)
    };
    let verdict = match (cable, gw, dns) {
        (ProbeState::Pass, ProbeState::Pass, ProbeState::Pass) => DiagVerdict::Ok,
        (ProbeState::Pass, ProbeState::Fail, _) => DiagVerdict::GatewayDown,
        (ProbeState::Pass, ProbeState::Unknown, _) => DiagVerdict::Probing,
        (ProbeState::Pass, _, ProbeState::Unknown) => DiagVerdict::Probing,
        (ProbeState::Pass, _, ProbeState::Fail) => DiagVerdict::DnsDown,
        _ => DiagVerdict::CableDown,
    };
    let lines = format!(
        "{}\n{}\n{}\n{}",
        line(ProbeLevel::Cable.name(), cable, "能不能到路由器"),
        line(ProbeLevel::Gateway.name(), gw, "能不能出内网"),
        line(ProbeLevel::Dns.name(), dns, "能不能解析域名"),
        "",
    );
    DiagReport { verdict, lines, stamp_ms: now_ms }
}

// ---------------------------------------------------------------------------
// 网络浮层（列表 + 五档信号 + 键盘可达连接流程）
// ---------------------------------------------------------------------------

/// 浮层里一个可见网络。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetItem {
    /// SSID 指纹（诊断显示用，不存全名）。
    pub ssid_hash: u32,
    /// 信号五档（1..=5）。
    pub bars: u8,
    /// 是否有密码（决定连接流程是否进密码框）。
    pub secured: bool,
    /// 已保存网络（一键直连）。
    pub known: bool,
}

/// RSSI → 五档信号（主册「信号强度五档」；典型 Wi-Fi 分界点）。
pub fn bars_from_rssi(rssi: i32) -> u8 {
    match rssi {
        r if r >= -50 => 5,
        r if r >= -60 => 4,
        r if r >= -67 => 3,
        r if r >= -75 => 2,
        r if r >= -82 => 1,
        _ => 0,
    }
}

/// 连接流程步（键盘可达序：Tab 前进 / Enter 激活 / Esc 返回）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowStep {
    /// 网络列表选择（Tab 逐项 / Enter 连接）。
    List,
    /// 密码框（ secured 网络才进；字符/退格/Enter）。
    Password,
    /// 连接中（进行态，结果落 Done）。
    Connecting,
    /// 结果（成功/失败各一态）。
    DoneOk,
    DoneFail,
}

/// 网络浮层：列表 + 选择 + 连接流程（键盘可达序模型）。
pub struct NetFloat {
    items: [Option<NetItem>; NETLIST_CAP],
    item_count: usize,
    /// 列表焦点（0..item_count，环绕）。
    focus: usize,
    step: FlowStep,
    psk: [u8; PSK_CAP],
    psk_len: usize,
    /// 焦点步进审计（键盘可达性判据：序号单调不跳变）。
    focus_moves: u32,
    connects: u32,
    pub stats: NetStats,
}

/// 浮层操作统计。
#[derive(Clone, Copy, Debug, Default)]
pub struct NetStats {
    pub tab_moves: u32,
    pub connect_attempts: u32,
    pub connect_ok: u32,
    pub connect_fail: u32,
}

impl NetFloat {
    pub fn new() -> NetFloat {
        NetFloat {
            items: [const { None }; NETLIST_CAP],
            item_count: 0,
            focus: 0,
            step: FlowStep::List,
            psk: [0; PSK_CAP],
            psk_len: 0,
            focus_moves: 0,
            connects: 0,
            stats: NetStats::default(),
        }
    }

    /// 登记可见网络（满槽丢弃最弱信号——列表容量纪律）。
    pub fn push_network(&mut self, item: NetItem) -> bool {
        if self.item_count < NETLIST_CAP {
            self.items[self.item_count] = Some(item);
            self.item_count += 1;
            return true;
        }
        // 满槽：替换信号最弱的一项（若新网络更强）。
        let mut weakest = 0usize;
        for i in 0..NETLIST_CAP {
            if self.items[i].map(|n| n.bars) < self.items[weakest].map(|n| n.bars) {
                weakest = i;
            }
        }
        if item.bars > self.items[weakest].map(|n| n.bars).unwrap_or(0) {
            self.items[weakest] = Some(item);
            true
        } else {
            false
        }
    }

    pub fn item_count(&self) -> usize {
        self.item_count
    }

    /// 列表快照（顺序即焦点序；空槽跳过——账本/测试面用，热路径不拷贝）。
    pub fn items(&self) -> Vec<NetItem> {
        self.items.iter().filter_map(|slot| *slot).collect()
    }

    pub fn focus(&self) -> usize {
        self.focus
    }

    pub fn step(&self) -> FlowStep {
        self.step
    }

    pub fn psk_len(&self) -> usize {
        self.psk_len
    }

    /// Tab：焦点在列表项间环绕步进（键盘可达性主通路）。
    pub fn focus_next(&mut self) {
        if self.item_count == 0 {
            return;
        }
        self.focus = (self.focus + 1) % self.item_count;
        self.focus_moves += 1;
        self.stats.tab_moves += 1;
    }

    /// Shift+Tab：反向步进（同样环绕可达）。
    pub fn focus_prev(&mut self) {
        if self.item_count == 0 {
            return;
        }
        self.focus = (self.focus + self.item_count - 1) % self.item_count;
        self.focus_moves += 1;
        self.stats.tab_moves += 1;
    }

    /// Enter：List→连接（无密码/已保存直连，否则进密码框）。
    pub fn activate(&mut self) -> FlowStep {
        if self.step != FlowStep::List || self.item_count == 0 {
            return self.step;
        }
        let secured = self.items[self.focus].map(|n| n.secured).unwrap_or(false);
        let known = self.items[self.focus].map(|n| n.known).unwrap_or(false);
        if secured && !known {
            self.step = FlowStep::Password;
            self.psk_len = 0;
        } else {
            self.begin_connect();
        }
        self.step
    }

    /// 密码框键入（F230 规范：圆点回显由渲染面处理）。
    pub fn psk_push(&mut self, ch: u8) -> bool {
        if self.step != FlowStep::Password || self.psk_len >= PSK_CAP {
            return false;
        }
        self.psk[self.psk_len] = ch;
        self.psk_len += 1;
        true
    }

    /// 密码框退格。
    pub fn psk_backspace(&mut self) -> bool {
        if self.step != FlowStep::Password || self.psk_len == 0 {
            return false;
        }
        self.psk_len -= 1;
        self.psk[self.psk_len] = 0;
        true
    }

    /// 密码框 Enter → 连接中。
    pub fn submit_psk(&mut self) -> FlowStep {
        if self.step != FlowStep::Password {
            return self.step;
        }
        self.begin_connect();
        self.step
    }

    fn begin_connect(&mut self) {
        self.step = FlowStep::Connecting;
        self.connects += 1;
        self.stats.connect_attempts += 1;
    }

    /// 连接结果落定（宿主注入结果）。
    pub fn finish_connect(&mut self, ok: bool) -> FlowStep {
        if self.step != FlowStep::Connecting {
            return self.step;
        }
        self.step = if ok { FlowStep::DoneOk } else { FlowStep::DoneFail };
        if ok {
            self.stats.connect_ok += 1;
        } else {
            self.stats.connect_fail += 1;
        }
        self.step
    }

    /// Esc：从密码框退回列表（键盘可达闭环）；结果态同样 Esc 回列表
    /// ——否则 DoneOk/DoneFail 是死端，之后无法再发起任何连接。
    pub fn escape(&mut self) -> FlowStep {
        if matches!(self.step, FlowStep::Password | FlowStep::DoneOk | FlowStep::DoneFail) {
            self.step = FlowStep::List;
            self.psk_len = 0;
        }
        self.step
    }

    /// 键盘可达性判定：全程只需 Tab/Enter/退格/Esc（无鼠标步骤），
    /// 且焦点步进次数与操作数同阶（无隐藏焦点陷阱）。
    pub fn keyboard_reachable(&self) -> bool {
        self.focus_moves > 0 && self.item_count > 0
    }
}

impl Default for NetFloat {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 网络监视器（探测状态机 + 刷新账本 + 诊断入口）
// ---------------------------------------------------------------------------

/// 网络状态监视器：三级探测 + 相位推导 + 刷新账本 + 诊断。
pub struct NetMonitor {
    probes: [ProbeSlot; 3],
    phase: NetPhase,
    /// 刷新延迟环（探测变化 → 图标应用，<2s 判据）。
    refresh_lat: RingLog<u64, 64>,
    /// 状态变化留痕环。
    changes: RingLog<StateChange, 32>,
    /// 三级探测耗时延迟环（诊断面：P95 最近邻分位数）。
    lat_cable: RingLog<u64, 64>,
    lat_gw: RingLog<u64, 64>,
    lat_dns: RingLog<u64, 64>,
    ledger: MinuteBook,
    pub stats: NetMonStats,
}

/// 监视器统计。
#[derive(Clone, Copy, Debug, Default)]
pub struct NetMonStats {
    pub probes: u32,
    pub phase_changes: u32,
    pub diags: u32,
    pub refreshes: u32,
}

impl NetMonitor {
    pub fn new() -> NetMonitor {
        NetMonitor {
            probes: [
                ProbeSlot { state: ProbeState::Unknown, last_ms: 0 },
                ProbeSlot { state: ProbeState::Unknown, last_ms: 0 },
                ProbeSlot { state: ProbeState::Unknown, last_ms: 0 },
            ],
            phase: NetPhase::Down,
            refresh_lat: RingLog::new(),
            changes: RingLog::new(),
            lat_cable: RingLog::new(),
            lat_gw: RingLog::new(),
            lat_dns: RingLog::new(),
            ledger: MinuteBook::new(LEDGER_COLS, 30 * 1440),
            stats: NetMonStats::default(),
        }
    }

    pub fn probe(&self, level: ProbeLevel) -> ProbeState {
        self.probes[level.idx()].state
    }

    pub fn phase(&self) -> NetPhase {
        self.phase
    }

    /// 三级探测快照（诊断入口直接用）。
    pub fn snapshot(&self) -> (ProbeState, ProbeState, ProbeState) {
        (self.probes[0].state, self.probes[1].state, self.probes[2].state)
    }

    /// 周期探测回报：更新探测位、推导相位、变化即留痕 + 刷新延迟记账。
    /// `probe_us` 为本次探测耗时（进延迟环）。
    pub fn report(&mut self, level: ProbeLevel, ok: Option<bool>, probe_us: u64, now_ms: u64) {
        let idx = level.idx();
        self.probes[idx].state = match ok {
            Some(true) => ProbeState::Pass,
            Some(false) => ProbeState::Fail,
            None => self.probes[idx].state,
        };
        self.probes[idx].last_ms = now_ms;
        self.stats.probes += 1;
        self.ledger.record_minute(now_ms / 60_000, &[1, 0, 0, 0]);
        // 探测耗时打点（None=该级未真正探测，不打点）。
        if ok.is_some() {
            match level {
                ProbeLevel::Cable => self.lat_cable.push(probe_us),
                ProbeLevel::Gateway => self.lat_gw.push(probe_us),
                ProbeLevel::Dns => self.lat_dns.push(probe_us),
            }
        }
        // 相位推导：仅在三级状态齐备度变化时重算（前级 Fail 不必探测
        // 后级——探测引擎只报已知级）。
        let (c, g, d) = self.snapshot();
        let new_phase = derive_phase(c, g, d);
        if new_phase != self.phase {
            self.phase = new_phase;
            self.stats.phase_changes += 1;
            self.changes.push(StateChange { phase: new_phase, at_ms: now_ms });
        }
    }

    /// 刷新延迟打点：状态变化时刻 → 图标应用时刻（<2s 判据面）。
    pub fn record_refresh(&mut self, detected_ms: u64, applied_ms: u64) {
        self.refresh_lat.push(applied_ms.saturating_sub(detected_ms));
        self.stats.refreshes += 1;
        self.ledger.record_minute(applied_ms / 60_000, &[0, 0, 0, 1]);
    }

    /// 刷新判定：样本 ≥8 且最大延迟 <2s（状态指示是门面，慢一次都可见）。
    pub fn refresh_in_time(&self) -> bool {
        let v = self.refresh_lat.newest_first();
        v.len() >= REFRESH_MIN_SAMPLES && v.iter().all(|&l| l < REFRESH_LIMIT_MS)
    }

    pub fn refresh_samples(&self) -> usize {
        self.refresh_lat.len()
    }

    /// 状态变化留痕（新→旧）。
    pub fn change_history(&self) -> Vec<StateChange> {
        self.changes.newest_first()
    }

    /// 一条命令诊断：三级探测快照 → 人话报告。
    pub fn diagnose_now(&mut self, now_ms: u64) -> DiagReport {
        let (c, g, d) = self.snapshot();
        let rep = diagnose(c, g, d, now_ms);
        self.stats.diags += 1;
        self.ledger.record_minute(now_ms / 60_000, &[0, 1, 0, 0]);
        rep
    }

    /// 探测耗时 P95（某级；无样本回 0——调用方以样本数把关）。
    pub fn probe_p95_us(&self, level: ProbeLevel) -> u64 {
        match level {
            ProbeLevel::Cable => pct_near(&self.lat_cable.newest_first(), 95),
            ProbeLevel::Gateway => pct_near(&self.lat_gw.newest_first(), 95),
            ProbeLevel::Dns => pct_near(&self.lat_dns.newest_first(), 95),
        }
    }

    /// 某级探测耗时样本数（P95 结论的样本量把关面）。
    pub fn probe_sample_count(&self, level: ProbeLevel) -> usize {
        match level {
            ProbeLevel::Cable => self.lat_cable.len(),
            ProbeLevel::Gateway => self.lat_gw.len(),
            ProbeLevel::Dns => self.lat_dns.len(),
        }
    }

    /// 账本区间聚合 [probe, diag, connect, refresh]。
    pub fn ledger_sum(&self, from_min: u64, to_min: u64) -> [u64; LEDGER_COLS] {
        let v = self.ledger.range_sum(from_min, to_min);
        [v[0], v[1], v[2], v[3]]
    }
}

impl Default for NetMonitor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// xors32 随机步进（范式照 touchpad.rs）。
fn xors32(x: &mut u32) -> u32 {
    *x ^= *x << 13;
    *x ^= *x >> 17;
    *x ^= *x << 5;
    *x
}

/// F242 自检（判据：三态+受限细分走查 + 三场景人话诊断 + 键盘可达
/// + 刷新 <2s）。
pub fn run_netstate_checks() -> CheckSet {
    let mut set = CheckSet::new("F242-netstate");

    // 1. 三态推导：全通=正常；线缆断=断开（即使网关/DNS 也断——逐级归因）。
    set.add(
        "phase: all pass = normal",
        derive_phase(ProbeState::Pass, ProbeState::Pass, ProbeState::Pass) == NetPhase::Normal,
        "",
    );
    set.add(
        "phase: cable fail = down",
        derive_phase(ProbeState::Fail, ProbeState::Fail, ProbeState::Fail) == NetPhase::Down,
        "",
    );

    // 2. 受限细分：网关断与 DNS 断是两个不同的受限（「两回事分开报」）。
    let lg = derive_phase(ProbeState::Pass, ProbeState::Fail, ProbeState::Unknown);
    let ld = derive_phase(ProbeState::Pass, ProbeState::Pass, ProbeState::Fail);
    set.add(
        "limited subcauses distinct",
        lg == NetPhase::Limited(LimitCause::NoGateway)
            && ld == NetPhase::Limited(LimitCause::NoDns)
            && lg != ld,
        "",
    );

    // 3. 图标字形走查：三态 + 细分各自字形、受限与断开可辨。
    set.add(
        "icon glyphs distinct across 4 states",
        icon_glyph(&NetPhase::Normal) != icon_glyph(&NetPhase::Limited(LimitCause::NoGateway))
            && icon_glyph(&NetPhase::Limited(LimitCause::NoGateway))
                != icon_glyph(&NetPhase::Limited(LimitCause::NoDns))
            && icon_glyph(&NetPhase::Limited(LimitCause::NoDns)) != icon_glyph(&NetPhase::Down)
            && limited_vs_down_distinct(&lg, &NetPhase::Down),
        "",
    );

    // 4. 诊断三场景：断线缆 / 断网关 / 断 DNS 人话定位（关键词齐备）。
    let rep_cable = diagnose(ProbeState::Fail, ProbeState::Unknown, ProbeState::Unknown, 0);
    let rep_gw = diagnose(ProbeState::Pass, ProbeState::Fail, ProbeState::Unknown, 0);
    let rep_dns = diagnose(ProbeState::Pass, ProbeState::Pass, ProbeState::Fail, 0);
    set.add(
        "diag: cable cut localized",
        rep_cable.verdict == DiagVerdict::CableDown
            && rep_cable.headline().contains("没连上")
            && rep_cable.mentions_all_levels(),
        "",
    );
    set.add(
        "diag: gateway cut localized",
        rep_gw.verdict == DiagVerdict::GatewayDown
            && rep_gw.headline().contains("连上了但上不了网")
            && rep_gw.lines.contains("路由器：不通")
            && rep_gw.mentions_all_levels(),
        "",
    );
    set.add(
        "diag: dns cut localized",
        rep_dns.verdict == DiagVerdict::DnsDown
            && rep_dns.headline().contains("DNS")
            && rep_dns.lines.contains("路由器：通")
            && rep_dns.lines.contains("DNS：不通"),
        "",
    );

    // 5. 探测中场景：数据不足诚实不下结论（Probing）。
    let rep_probe = diagnose(ProbeState::Pass, ProbeState::Unknown, ProbeState::Unknown, 0);
    set.add(
        "diag: probing says so honestly",
        rep_probe.verdict == DiagVerdict::Probing && rep_probe.headline().contains("暂不下结论"),
        "",
    );

    // 6. 探测状态机：回报 → 相位变化留痕（每条变化都落账）。
    let mut m = NetMonitor::new();
    m.report(ProbeLevel::Cable, Some(true), 2_000, 1_000);
    m.report(ProbeLevel::Gateway, Some(true), 3_000, 2_000);
    m.report(ProbeLevel::Dns, Some(true), 8_000, 3_000);
    set.add(
        "probe reports derive normal",
        m.phase() == NetPhase::Normal && m.change_history().len() >= 1 && m.stats.probes == 3,
        "",
    );

    // 7. 相位迁移留痕：正常→受限→断开三条变化可回放。
    m.report(ProbeLevel::Dns, Some(false), 9_000, 4_000);
    m.report(ProbeLevel::Cable, Some(false), 1_000, 5_000);
    let hist = m.change_history();
    set.add(
        "phase changes recorded newest-first",
        hist.len() >= 3 && hist[0].phase == NetPhase::Down,
        "",
    );

    // 8. 刷新延迟账本：10 个 300ms 实测 → <2s 门开；一个 2.5s → 门关。
    let mut m2 = NetMonitor::new();
    for i in 0..10u32 {
        m2.record_refresh(i as u64 * 1_000, i as u64 * 1_000 + 300);
    }
    set.add("refresh <2s gate opens", m2.refresh_in_time() && m2.refresh_samples() == 10, "");
    m2.record_refresh(100_000, 102_500);
    set.add("slow refresh judged red", !m2.refresh_in_time(), "");
    // 样本不足不下结论。
    let mut m3 = NetMonitor::new();
    m3.record_refresh(0, 100);
    set.add("refresh gate needs 8+ samples", !m3.refresh_in_time(), "");

    // 9. 探测耗时直方图：三级各自打点、P95 可读。
    let mut m4 = NetMonitor::new();
    for i in 0..20u32 {
        m4.report(ProbeLevel::Cable, Some(true), 2_000 + i as u64 * 100, i as u64);
        m4.report(ProbeLevel::Gateway, Some(true), 6_000, i as u64);
        m4.report(ProbeLevel::Dns, Some(false), 30_000, i as u64);
    }
    set.add(
        "probe latency hist per level",
        m4.probe_p95_us(ProbeLevel::Cable) >= 3_800
            && m4.probe_p95_us(ProbeLevel::Gateway) == 6_000
            && m4.probe_p95_us(ProbeLevel::Dns) == 30_000,
        "",
    );

    // 10. 信号五档映射（RSSI → bars）。
    set.add(
        "signal bars 5 levels",
        bars_from_rssi(-40) == 5
            && bars_from_rssi(-55) == 4
            && bars_from_rssi(-65) == 3
            && bars_from_rssi(-70) == 2
            && bars_from_rssi(-80) == 1
            && bars_from_rssi(-90) == 0,
        "",
    );

    // 11. 浮层列表：定容 8、满槽替换最弱信号。
    let mut f = NetFloat::new();
    for i in 0..8u32 {
        f.push_network(NetItem { ssid_hash: i, bars: 3, secured: false, known: false });
    }
    set.add("netlist cap 8", f.item_count() == 8, "");
    set.add(
        "weakest replaced by stronger",
        f.push_network(NetItem { ssid_hash: 99, bars: 5, secured: false, known: false })
            && f.item_count() == 8
            && f.items().iter().any(|n| n.ssid_hash == 99),
        "",
    );

    // 12. 键盘可达连接流程：Tab 环绕 → Enter（无密码直连）→ DoneOk。
    let mut f2 = NetFloat::new();
    f2.push_network(NetItem { ssid_hash: 1, bars: 5, secured: false, known: false });
    f2.push_network(NetItem { ssid_hash: 2, bars: 4, secured: true, known: false });
    f2.focus_next();
    f2.focus_next(); // 环绕回 0
    set.add(
        "focus wraps around list",
        f2.focus() == 0 && f2.keyboard_reachable(),
        "",
    );
    let st1 = f2.activate(); // 焦点 0：无密码 → 直连
    set.add(
        "open network connects directly",
        st1 == FlowStep::Connecting && f2.finish_connect(true) == FlowStep::DoneOk
            && f2.stats.connect_ok == 1,
        "",
    );

    // 13. 加密网络走密码框：键入→提交→结果；Esc 可退回列表。
    let mut f3 = NetFloat::new();
    f3.push_network(NetItem { ssid_hash: 7, bars: 4, secured: true, known: false });
    set.add("secured goes to password", f3.activate() == FlowStep::Password, "");
    f3.psk_push(b'a');
    f3.psk_push(b'b');
    f3.psk_backspace();
    f3.psk_push(b'c');
    set.add("psk input tracked", f3.psk_len() == 2, "");
    set.add(
        "password submits to connecting",
        f3.submit_psk() == FlowStep::Connecting && f3.finish_connect(false) == FlowStep::DoneFail
            && f3.stats.connect_fail == 1,
        "",
    );
    let mut f4 = NetFloat::new();
    f4.push_network(NetItem { ssid_hash: 8, bars: 5, secured: true, known: false });
    let _ = f4.activate();
    set.add("esc returns to list", f4.escape() == FlowStep::List && f4.psk_len() == 0, "");

    // 14. xors32 fuzz：随机三级探测 1000 轮，不变量=相位恒等于
    //     derive_phase（推导唯一源）、刷新延迟非负、图标字形与相位
    //     一一对应、不 panic。
    let mut mf = NetMonitor::new();
    let mut x: u32 = 0x94_04_E3DE;
    let mut ok = true;
    let mut clock: u64 = 0;
    for _ in 0..1000u32 {
        clock += (xors32(&mut x) % 3_000) as u64;
        let lvl = match xors32(&mut x) % 3 {
            0 => ProbeLevel::Cable,
            1 => ProbeLevel::Gateway,
            _ => ProbeLevel::Dns,
        };
        let outcome = match xors32(&mut x) % 4 {
            0 => Some(true),
            1 => Some(false),
            2 => None,
            _ => Some(xors32(&mut x) % 2 == 0),
        };
        mf.report(lvl, outcome, (xors32(&mut x) % 50_000) as u64, clock);
        let (c, g, d) = mf.snapshot();
        if mf.phase() != derive_phase(c, g, d) {
            ok = false;
        }
        if let Some(def) = mf.change_history().first() {
            if def.phase != mf.phase() {
                ok = false;
            }
        }
        if xors32(&mut x) % 50 == 0 {
            mf.record_refresh(clock, clock + (xors32(&mut x) % 5_000) as u64);
        }
    }
    set.add("xors32 fuzz 1000 rounds invariants", ok, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_states_and_subcauses() {
        // 正常 / 断开 / 两种受限，四个值互不相同（三态+细分走查）。
        let normal = derive_phase(ProbeState::Pass, ProbeState::Pass, ProbeState::Pass);
        let down = derive_phase(ProbeState::Fail, ProbeState::Pass, ProbeState::Pass);
        let lim_gw = derive_phase(ProbeState::Pass, ProbeState::Fail, ProbeState::Pass);
        let lim_dns = derive_phase(ProbeState::Pass, ProbeState::Pass, ProbeState::Fail);
        assert_ne!(normal, down);
        assert_ne!(lim_gw, lim_dns);
        assert_ne!(lim_gw, down);
        assert_eq!(lim_gw, NetPhase::Limited(LimitCause::NoGateway));
        assert_eq!(lim_dns, NetPhase::Limited(LimitCause::NoDns));
        // 受限细分字形也必须互异（黄色感叹号下标 1 vs 2）。
        assert_ne!(icon_glyph(&lim_gw), icon_glyph(&lim_dns));
    }

    #[test]
    fn diagnose_human_readable_three_scenarios() {
        // 场景一：断线缆——三级逐行齐备、结论是人话。
        let r1 = diagnose(ProbeState::Fail, ProbeState::Unknown, ProbeState::Unknown, 10);
        assert_eq!(r1.verdict, DiagVerdict::CableDown);
        assert!(r1.lines.contains("网线：不通"));
        assert!(r1.lines.contains("路由器：未测"));
        assert!(r1.headline().contains("没连上"));
        // 场景二：断网关。
        let r2 = diagnose(ProbeState::Pass, ProbeState::Fail, ProbeState::Unknown, 20);
        assert_eq!(r2.verdict, DiagVerdict::GatewayDown);
        assert!(r2.lines.contains("网线：通"));
        assert!(r2.lines.contains("路由器：不通"));
        assert!(r2.headline().contains("连上了但上不了网"));
        // 场景三：断 DNS。
        let r3 = diagnose(ProbeState::Pass, ProbeState::Pass, ProbeState::Fail, 30);
        assert_eq!(r3.verdict, DiagVerdict::DnsDown);
        assert!(r3.lines.contains("路由器：通"));
        assert!(r3.lines.contains("DNS：不通"));
        assert!(r3.headline().contains("DNS"));
    }

    #[test]
    fn monitor_phase_transitions_recorded() {
        let mut m = NetMonitor::new();
        // 初始 Unknown → derive 为 Down（未探测=未连接，图标诚实显示断开）。
        assert_eq!(m.phase(), NetPhase::Down);
        m.report(ProbeLevel::Cable, Some(true), 1_000, 100);
        m.report(ProbeLevel::Gateway, Some(true), 2_000, 200);
        m.report(ProbeLevel::Dns, Some(true), 5_000, 300);
        assert_eq!(m.phase(), NetPhase::Normal);
        // DNS 断 → 受限（NoDns）。
        m.report(ProbeLevel::Dns, Some(false), 5_000, 400);
        assert_eq!(m.phase(), NetPhase::Limited(LimitCause::NoDns));
        // 网关也断 → 受限（NoGateway）。
        m.report(ProbeLevel::Gateway, Some(false), 2_000, 500);
        assert_eq!(m.phase(), NetPhase::Limited(LimitCause::NoGateway));
        // 线缆断 → 直接断开（逐级归因）。
        m.report(ProbeLevel::Cable, Some(false), 1_000, 600);
        assert_eq!(m.phase(), NetPhase::Down);
        // 变化留痕（新→旧）：初始 Down 之后每次推导变化都记一条——
        // Down→Limited(Gw)→Limited(Dns)→Normal→Limited(Dns)→Limited(Gw)→Down
        // 共 6 条变化；最新一条是回到 Down（hist[0]），最早一条是首次
        // 进受限 Limited(Gw)（hist[5]，线缆通而网关未测即受限），
        // 首次进 Normal 在 hist[3]。
        // 现象：hist[5] 断言 Normal 失败（left:Limited(NoGateway)）。
        // 根因：测试与自身枚举序矛盾——按上面 6 条变化序，最旧一条是
        //      第一次变化 Down→Limited(Gw)，实现留痕正确（自检第 6/7
        //      条同为该口径且绿）。
        // 修法：改测试——hist[5]=Limited(NoGateway)、hist[3]=Normal。
        let hist = m.change_history();
        assert_eq!(hist.len(), 6);
        assert_eq!(hist[0].phase, NetPhase::Down);
        assert_eq!(hist[3].phase, NetPhase::Normal);
        assert_eq!(hist[6 - 1].phase, NetPhase::Limited(LimitCause::NoGateway));
    }

    #[test]
    fn refresh_ledger_and_gate() {
        let mut m = NetMonitor::new();
        for i in 0..12u32 {
            m.record_refresh(i as u64 * 100, i as u64 * 100 + 800);
        }
        assert!(m.refresh_in_time(), "12 个 800ms 实测全 <2s");
        m.record_refresh(10_000, 13_000);
        assert!(!m.refresh_in_time(), "一条 3s 超门即红");
    }

    #[test]
    fn netlist_weakest_eviction() {
        let mut f = NetFloat::new();
        for i in 0..8u32 {
            f.push_network(NetItem { ssid_hash: 10 + i, bars: 2 + i as u8 / 4, secured: false, known: false });
        }
        assert_eq!(f.item_count(), 8);
        // 更弱网络被拒（不挤列表）。
        assert!(!f.push_network(NetItem { ssid_hash: 50, bars: 1, secured: false, known: false }));
        assert_eq!(f.item_count(), 8);
        // 更强网络挤掉最弱。
        assert!(f.push_network(NetItem { ssid_hash: 60, bars: 5, secured: false, known: false }));
        let has_60 = f.items().iter().any(|n| n.ssid_hash == 60);
        assert!(has_60);
    }

    #[test]
    fn keyboard_flow_complete() {
        let mut f = NetFloat::new();
        f.push_network(NetItem { ssid_hash: 1, bars: 5, secured: true, known: true });
        f.push_network(NetItem { ssid_hash: 2, bars: 3, secured: true, known: false });
        // Tab 到第二项（加密未保存）→ Enter 进密码框。
        f.focus_next();
        assert_eq!(f.focus(), 1);
        assert_eq!(f.activate(), FlowStep::Password);
        // 键入 4 字符、退格 1、提交。
        for ch in b"abcd" {
            assert!(f.psk_push(*ch));
        }
        assert!(f.psk_backspace());
        assert!(f.psk_push(b'e'));
        assert_eq!(f.psk_len(), 4);
        assert_eq!(f.submit_psk(), FlowStep::Connecting);
        assert_eq!(f.finish_connect(true), FlowStep::DoneOk);
        assert_eq!(f.stats.connect_attempts, 1);
        assert_eq!(f.stats.connect_ok, 1);
        // 已保存网络（第一项）Enter 直连，不进密码框。
        // 现象：直接 activate() 恒回 DoneOk（left:DoneOk right:Connecting）。
        // 根因：实现缺陷——结果态（DoneOk/DoneFail）原无任何键盘出口，
        //      流程死端，违反模块「键盘可达闭环」设计要点；此处激活的
        //      前置态必须是 List。
        // 修法：改实现——escape() 扩展为密码框/结果态均可 Esc 回列表
        //      （与密码框同一闭环语义）；测试在二次连接前先 Esc 回列表。
        assert_eq!(f.escape(), FlowStep::List, "结果态 Esc 回列表（键盘可达闭环）");
        f.focus_prev(); // 回到 0
        assert_eq!(f.activate(), FlowStep::Connecting, "known 网络一键直连");
    }

    #[test]
    fn netstate_selfcheck_all_green() {
        let set = run_netstate_checks();
        assert!(set.all_passed(), "F242 自检存在红项");
        assert!(!set.truncated());
    }
}

// ===========================================================================
// v2 深化批（2026-09-26 · AI-H1 二次对账批）：UI 壳接线 / 持久化 I/O / 判定面扩展
// ===========================================================================
// 主册锚 F242（网络状态指示与诊断）。v2 三件事：
// 1) 持久化 I/O：已保存网络册（SSID 指纹 + 信号 + 加密/自连标志）定长
//    容器序列化——magic b"VXH1" + 版本 1 + 定长 payload + FNV-1a 校验
//    和，四类损坏显性拒绝；
// 2) UI 壳接线：托盘图标槽几何（右缘向左排布）+ 字形绘制码派发 + 浮层
//    行布局/命中测试——「点击图标呼出网络浮层」的几何承载；
// 3) 判定面扩展：run_netstate_v2_checks，首条即持久化 round-trip。

// -- 持久化 I/O 面 ---------------------------------------------------------

/// v2 容器 payload 定长：条数 u32 + 8 槽×8B（ssid u32 + bars/flags + 2 保留）。
pub const VX2_NET_PAYLOAD: usize = 4 + 8 * 8;
/// v2 容器全长 = magic 4 + version 1 + payload + checksum 4。
pub const VX2_NET_BLOB: usize = 9 + VX2_NET_PAYLOAD;

/// v2 损坏分类（显性拒绝面——各归其名，不静默回默认）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Vx2Error {
    BadMagic,
    BadVersion,
    /// 总长 ≠ 定长容器，或条数越出 payload 容量。
    BadLength,
    BadChecksum,
}

/// FNV-1a 32 位校验和（offset 0x811C9DC5、素数 0x01000193）。
fn vx2_fnv(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 一条已保存网络（持久化粒度：指纹不存全名，诊断纪律同 NetItem）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetSaved {
    pub ssid_hash: u32,
    /// 最近实测信号档（1..=5）。
    pub bars: u8,
    pub secured: bool,
    /// 自连标志（浮层重建时作为 known 网络）。
    pub autoconnect: bool,
}

/// 已保存网络册（网络浮层「已保存」区数据面，定容 8 槽）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetSavedBook {
    pub nets: [Option<NetSaved>; 8],
}

impl NetSavedBook {
    pub const fn new() -> NetSavedBook {
        NetSavedBook { nets: [None; 8] }
    }

    /// 登记/更新一条（同 ssid_hash 幂等更新；满槽显性拒绝）。
    pub fn put(&mut self, n: NetSaved) -> bool {
        for slot in self.nets.iter_mut() {
            if let Some(e) = slot {
                if e.ssid_hash == n.ssid_hash {
                    *slot = Some(n);
                    return true;
                }
            }
        }
        for slot in self.nets.iter_mut() {
            if slot.is_none() {
                *slot = Some(n);
                return true;
            }
        }
        false
    }

    /// 推到浮层：已保存网络以 known 身份入列表（一键直连判据的承载）。
    pub fn offer_to(&self, f: &mut NetFloat) {
        for n in self.nets.iter().flatten() {
            let _ = f.push_network(NetItem {
                ssid_hash: n.ssid_hash,
                bars: n.bars,
                secured: n.secured,
                known: n.autoconnect,
            });
        }
    }

    /// 序列化：b"VXH1" + 版本 1 + 定长 payload + FNV-1a。缓冲不足返回 0。
    pub fn to_bytes(&self, out: &mut [u8]) -> usize {
        if out.len() < VX2_NET_BLOB {
            return 0;
        }
        out[0..4].copy_from_slice(b"VXH1");
        out[4] = 1;
        let mut n = 0usize;
        for s in self.nets.iter().flatten() {
            let p = 9 + n * 8;
            out[p..p + 4].copy_from_slice(&s.ssid_hash.to_le_bytes());
            out[p + 4] = s.bars;
            out[p + 5] = s.secured as u8 | ((s.autoconnect as u8) << 1);
            out[p + 6] = 0;
            out[p + 7] = 0;
            n += 1;
        }
        out[5..9].copy_from_slice(&(n as u32).to_le_bytes());
        let end = 9 + VX2_NET_PAYLOAD;
        let crc = vx2_fnv(&out[..end - 4]);
        out[end - 4..end].copy_from_slice(&crc.to_le_bytes());
        VX2_NET_BLOB
    }

    /// 反序列化：四类损坏显性拒绝。
    pub fn from_bytes(blob: &[u8]) -> Result<NetSavedBook, Vx2Error> {
        if blob.len() != VX2_NET_BLOB {
            return Err(Vx2Error::BadLength);
        }
        if blob[0..4] != *b"VXH1" {
            return Err(Vx2Error::BadMagic);
        }
        if blob[4] != 1 {
            return Err(Vx2Error::BadVersion);
        }
        let end = 9 + VX2_NET_PAYLOAD;
        let crc = u32::from_le_bytes([blob[end - 4], blob[end - 3], blob[end - 2], blob[end - 1]]);
        if vx2_fnv(&blob[..end - 4]) != crc {
            return Err(Vx2Error::BadChecksum);
        }
        let n = u32::from_le_bytes([blob[5], blob[6], blob[7], blob[8]]) as usize;
        if n > 8 {
            return Err(Vx2Error::BadLength);
        }
        let mut book = NetSavedBook::new();
        for k in 0..n {
            let p = 9 + k * 8;
            book.nets[k] = Some(NetSaved {
                ssid_hash: u32::from_le_bytes([blob[p], blob[p + 1], blob[p + 2], blob[p + 3]]),
                bars: blob[p + 4].min(SIGNAL_BARS_MAX),
                secured: blob[p + 5] & 1 == 1,
                autoconnect: blob[p + 5] & 2 != 0,
            });
        }
        Ok(book)
    }
}

// -- UI 壳接线面 -----------------------------------------------------------

/// 托盘图标规格（px）——v2 布局常量：F242 托盘图标 24×24、间距 8。
pub const VX2_TRAY_ICON_W: i32 = 24;
pub const VX2_TRAY_GAP: i32 = 8;
/// 浮层行高 / 宽（px）。
pub const VX2_FLYOUT_ROW_H: i32 = 32;
pub const VX2_FLYOUT_W: i32 = 280;
/// 字形绘制码（渲染面按码取图；四态互异——三态+细分走查的绘制面）。
pub const VX2_SHAPE_FULL: u8 = 1;
pub const VX2_SHAPE_WARN_1: u8 = 2;
pub const VX2_SHAPE_WARN_2: u8 = 3;
pub const VX2_SHAPE_CROSS: u8 = 4;

/// 字形 → 绘制码（唯一派发口：与 icon_glyph 一一对应）。
pub fn glyph_shape(g: &IconGlyph) -> u8 {
    match g {
        IconGlyph::Connected => VX2_SHAPE_FULL,
        IconGlyph::LimitedGw => VX2_SHAPE_WARN_1,
        IconGlyph::LimitedDns => VX2_SHAPE_WARN_2,
        IconGlyph::Disconnected => VX2_SHAPE_CROSS,
    }
}

/// 托盘图标槽矩形：任务栏右缘向左排第 slot 个（0 = 最右），
/// 垂直居中于任务栏条带。
pub fn tray_icon_rect(taskbar_right: i32, taskbar_h: i32, slot: usize) -> crate::h1star::h1base::Rect {
    let x = taskbar_right - (slot as i32 + 1) * VX2_TRAY_ICON_W - slot as i32 * VX2_TRAY_GAP;
    crate::h1star::h1base::Rect::new(
        x,
        (taskbar_h - VX2_TRAY_ICON_W) / 2,
        VX2_TRAY_ICON_W,
        VX2_TRAY_ICON_W,
    )
}

/// 浮层行绘制条目：行矩形 + 信号档 + 焦点高亮。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FlyRow {
    pub ssid_hash: u32,
    pub bars: u8,
    pub y: i32,
    pub h: i32,
    pub focused: bool,
}

/// 生成浮层行绘制列表：列表序即焦点序，焦点行带高亮。
pub fn flyout_rows(f: &NetFloat, out: &mut [FlyRow]) -> usize {
    let items = f.items();
    let focus = f.focus();
    let m = items.len().min(out.len());
    for k in 0..m {
        out[k] = FlyRow {
            ssid_hash: items[k].ssid_hash,
            bars: items[k].bars,
            y: k as i32 * VX2_FLYOUT_ROW_H,
            h: VX2_FLYOUT_ROW_H,
            focused: k == focus,
        };
    }
    m
}

/// 浮层行命中测试（浮层内坐标；x ∈ [0, VX2_FLYOUT_W) 且落在行内）。
pub fn flyout_row_hit(rows: &[FlyRow], n: usize, px: i32, py: i32) -> Option<usize> {
    (0..n.min(rows.len())).find(|&k| {
        px >= 0 && px < VX2_FLYOUT_W && py >= rows[k].y && py < rows[k].y + rows[k].h
    })
}

// -- 判定面扩展 ------------------------------------------------------------

/// F242 v2 自检（锚注见各条注释；首条 = 持久化 round-trip）。
pub fn run_netstate_v2_checks() -> crate::checks::CheckSet {
    let mut set = CheckSet::new("F242-netstate-v2");

    // 1. 持久化 round-trip：册编→解→逐字段相等→推浮层→known 落列表。
    let mut book = NetSavedBook::new();
    let _ = book.put(NetSaved { ssid_hash: 0xA11CE, bars: 5, secured: true, autoconnect: true });
    let _ = book.put(NetSaved { ssid_hash: 0xB0B, bars: 3, secured: false, autoconnect: false });
    let mut buf = [0u8; VX2_NET_BLOB];
    let len = book.to_bytes(&mut buf);
    let mut fl = NetFloat::new();
    match NetSavedBook::from_bytes(&buf[..len]) {
        Ok(b2) => {
            b2.offer_to(&mut fl);
            set.add(
                "v2 persistence round-trip",
                b2 == book && fl.item_count() == 2
                    && fl.items()[0].ssid_hash == 0xA11CE && fl.items()[0].known
                    && fl.items()[1].ssid_hash == 0xB0B && !fl.items()[1].known,
                "",
            );
        }
        Err(_) => set.add("v2 persistence round-trip", false, ""),
    }

    // 2. 四类损坏显性拒绝（截断 / magic / 版本 / payload 翻位）。
    let mut m = buf;
    m[0] = b'Z';
    let mut v = buf;
    v[4] = 7;
    let mut c = buf;
    c[15] ^= 0xFF;
    set.add(
        "v2 corruption explicitly rejected",
        NetSavedBook::from_bytes(&buf[..len - 1]) == Err(Vx2Error::BadLength)
            && NetSavedBook::from_bytes(&m) == Err(Vx2Error::BadMagic)
            && NetSavedBook::from_bytes(&v) == Err(Vx2Error::BadVersion)
            && NetSavedBook::from_bytes(&c) == Err(Vx2Error::BadChecksum),
        "",
    );

    // 3. 托盘几何：槽 0 贴右缘、槽 1 在其左且不重叠；四态字形绘制码互异。
    let r0 = tray_icon_rect(1920, 48, 0);
    let r1 = tray_icon_rect(1920, 48, 1);
    let glyphs = [
        glyph_shape(&icon_glyph(&NetPhase::Normal)),
        glyph_shape(&icon_glyph(&NetPhase::Limited(LimitCause::NoGateway))),
        glyph_shape(&icon_glyph(&NetPhase::Limited(LimitCause::NoDns))),
        glyph_shape(&icon_glyph(&NetPhase::Down)),
    ];
    set.add(
        "v2 tray geometry & glyph shapes",
        r0.right() <= 1920 && r1.right() <= r0.x - 0 && r1.x + r1.w <= r0.x
            && r0.y == (48 - VX2_TRAY_ICON_W) / 2
            && glyphs == [VX2_SHAPE_FULL, VX2_SHAPE_WARN_1, VX2_SHAPE_WARN_2, VX2_SHAPE_CROSS],
        "",
    );

    // 4. 浮层行布局与命中：焦点行高亮、界内命中、界外不命中。
    let mut fl4 = NetFloat::new();
    let _ = fl4.push_network(NetItem { ssid_hash: 11, bars: 5, secured: false, known: false });
    let _ = fl4.push_network(NetItem { ssid_hash: 12, bars: 2, secured: true, known: false });
    fl4.focus_next();
    let mut rows = [FlyRow { ssid_hash: 0, bars: 0, y: 0, h: 0, focused: false }; NETLIST_CAP];
    let rn = flyout_rows(&fl4, &mut rows);
    set.add(
        "v2 flyout rows layout & hit",
        rn == 2 && rows[1].focused && !rows[0].focused && rows[1].y == VX2_FLYOUT_ROW_H
            && flyout_row_hit(&rows, rn, 100, VX2_FLYOUT_ROW_H + 5) == Some(1)
            && flyout_row_hit(&rows, rn, 100, -1).is_none()
            && flyout_row_hit(&rows, rn, VX2_FLYOUT_W, 5).is_none(),
        "",
    );

    // 5. xors32 fuzz 500 轮：随机册 round-trip 逐字段相等、payload 任一
    //    字节翻位必被校验和捕获。
    let mut x: u32 = 0x2426_1A5C;
    let mut ok = true;
    for _ in 0..500u32 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let mut b = NetSavedBook::new();
        for k in 0..((x >> 3) % 9) {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            let _ = b.put(NetSaved {
                ssid_hash: x,
                bars: (x % 6) as u8,
                secured: x & 1 == 1,
                autoconnect: x & 2 == 2,
            });
        }
        let mut tbuf = [0u8; VX2_NET_BLOB];
        ok &= b.to_bytes(&mut tbuf) == VX2_NET_BLOB && NetSavedBook::from_bytes(&tbuf) == Ok(b);
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        tbuf[5 + (x as usize) % VX2_NET_PAYLOAD] ^= 0x40;
        ok &= NetSavedBook::from_bytes(&tbuf) == Err(Vx2Error::BadChecksum);
    }
    set.add("v2 fuzz 500 round-trips & checksum", ok, "");

    set
}

// ---------------------------------------------------------------------------
// v2 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests_v2 {
    use super::*;

    #[test]
    fn v2_saved_book_roundtrip() {
        let mut b = NetSavedBook::new();
        let _ = b.put(NetSaved { ssid_hash: 7, bars: 4, secured: true, autoconnect: true });
        let mut buf = [0u8; VX2_NET_BLOB];
        assert_eq!(b.to_bytes(&mut buf), VX2_NET_BLOB);
        assert_eq!(NetSavedBook::from_bytes(&buf), Ok(b));
        assert_eq!(NetSavedBook::from_bytes(&buf[..8]), Err(Vx2Error::BadLength));
    }

    #[test]
    fn v2_tray_slots_do_not_overlap() {
        let mut prev = tray_icon_rect(1600, 40, 0);
        for slot in 1..4usize {
            let r = tray_icon_rect(1600, 40, slot);
            assert!(r.x + r.w <= prev.x, "托盘槽位不得重叠");
            prev = r;
        }
    }

    #[test]
    fn v2_selfcheck_all_green() {
        let set = run_netstate_v2_checks();
        assert!(set.all_passed(), "F242 v2 自检存在红项");
        assert!(!set.truncated());
    }
}
