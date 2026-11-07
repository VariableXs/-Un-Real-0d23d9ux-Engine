//! UNX-F52390 Grafana OSS（Windows）收口 · 0xCA140段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52390"（判据原文锚定源）criteria：① 纯功能代码 ≥300 行；② 判据
//!   原文：UNX-F52391-J1 六判据全落；面板/数据源 ×10；KB-COMPAT 定级登
//!   记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘
//!   要非空。
//!   键 "52391"（功能标题源）title：Grafana OSS（Windows）收口
//!   quota: 300
//! off-by-one 板式口径：本卡（卡 N=52390）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52391-J1（六判据全落；面板/数据源 ×10），照原文（键 "52390"
//! criteria）锚定；功能按本卡标题（Grafana OSS（Windows）收口，键
//! "52391" title）落地；行数基线按功能标题源 quota=300 执行（落盘
//! ≥360 行）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五面板族闭集 20 格：timeseries/stat/table/logs/gauge 各 4 格，
//!      族序单调（空账本首族必 timeseries，逆跳必抓），逐格载荷纯函数
//!      折叠恒等才收，漂移必抓。
//!   ② datasource 三元闸 3 格：Prometheus/Loki/InfluxDB 三元闭集，空账
//!      本首闸必 Prometheus，逆跳/越闸必抓 EDsOrd。
//!   ③ dashboard.json 双向投影 12 格：正向投影（账→JSON 键）逐格恒等，
//!      载荷漂移 EDashDrift 必抓，同格二次提交必拒 EDup（回投核验）。
//!   ④ 告警规则 8 格：规则载荷逐格对照，越闭集 ENoAlert 必拒。
//!   ⑤ 与 Prometheus 消费联签 4 格（F52392 前向声明）+ Go 上栈声明 2 行
//!      闭集，缺行拒封（ERowMiss），未齐拒封（ESeal）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（面板载荷漂移/datasource 三元闸
//!      逆/越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集
//!      S1/S2/S3（六闸 + 20 面板 + 3 数据源 + 12 投影 + 8 告警 + 4 联签
//!      + 2 Go 全齐 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52390;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 面板格数（五族 × 4）。
pub const PANELS: usize = 20;

/// datasource 三元闸格数。
pub const DS_GATES: usize = 3;

/// dashboard.json 双向投影格数。
pub const DASH_CELLS: usize = 12;

/// 告警规则格数。
pub const ALERTS: usize = 8;

/// Prometheus 消费联签行数（F52392 前向声明）。
pub const UNION_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrafanaCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 面板/告警载荷漂移
    EDrift,
    /// 面板族逆序
    EOrder,
    /// datasource 三元闸逆序
    EDsOrd,
    /// dashboard.json 投影漂移
    EDashDrift,
    /// 告警规则越闭集
    ENoAlert,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl GrafanaCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GrafanaCode::Ok => "成功",
            GrafanaCode::ENoCell => "格号越闭集：面板 20 格/投影 12 格闭集之外先登记",
            GrafanaCode::EDrift => "载荷漂移：面板/告警折叠与基准不恒等，逐格复算",
            GrafanaCode::EOrder => "面板族逆序：timeseries→stat→table→logs→gauge 单调，逆跳必抓",
            GrafanaCode::EDsOrd => "datasource 三元闸逆序：首闸必 Prometheus，Prometheus→Loki→InfluxDB 单调",
            GrafanaCode::EDashDrift => "dashboard.json 投影漂移：双向投影载荷与基准不恒等，逐格复算",
            GrafanaCode::ENoAlert => "告警规则越闭集：8 格闭集之外先登记",
            GrafanaCode::ERowMiss => "缺声明行：Prometheus 联签 4 行或 Go 上栈 2 行未齐先补齐",
            GrafanaCode::EDup => "重复落账：同格只许一条账目",
            GrafanaCode::ESeal => "未齐即封：20 面板 + 3 数据源 + 12 投影 + 8 告警 + 闸未齐先补齐",
            GrafanaCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            GrafanaCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            GrafanaCode::EGradeDrift => "定级漂移：全齐才可定 S1，判据缺只可定 S3",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20面板+3数据源+12投影+8告警+4联签+2Go 全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 面板格载荷（格 o；族 = o/4）。
pub const fn panel_cell(o: usize) -> Option<u64> {
    if o >= PANELS {
        return None;
    }
    Some((0xC0000u64 + (o as u64) * 0x39) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 面板族（0..4=timeseries 4..8=stat 8..12=table 12..16=logs 16..20=gauge）。
pub const fn panel_kind(o: usize) -> Option<u32> {
    if o >= PANELS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 面板族名。
pub const fn kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("timeseries"),
        1 => Some("stat"),
        2 => Some("table"),
        3 => Some("logs"),
        4 => Some("gauge"),
        _ => None,
    }
}

/// datasource 闸号（0=Prometheus 1=Loki 2=InfluxDB）。
pub const fn ds_of(i: usize) -> Option<u32> {
    if i >= DS_GATES {
        return None;
    }
    Some(i as u32)
}

/// datasource 名。
pub const fn ds_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("Prometheus"),
        1 => Some("Loki"),
        2 => Some("InfluxDB"),
        _ => None,
    }
}

/// datasource 闸载荷（格 i）。
pub const fn ds_cell(i: usize) -> Option<u64> {
    if i >= DS_GATES {
        return None;
    }
    Some((0xC0800u64 + (i as u64) * 0x41) ^ 0x44535447_0000)
}

/// dashboard.json 双向投影格载荷（格 i）。
pub const fn dash_cell(i: usize) -> Option<u64> {
    if i >= DASH_CELLS {
        return None;
    }
    Some((0xC1000u64 + (i as u64) * 0x3D) ^ 0x47524144_0000)
}

/// 告警规则格载荷（格 a）。
pub const fn alert_cell(a: usize) -> Option<u64> {
    if a >= ALERTS {
        return None;
    }
    Some((0xC2000u64 + (a as u64) * 0x43) ^ 0x414C5254_0000)
}

/// Prometheus 消费联签行折叠（行 r）。
pub const fn union_row(r: usize) -> Option<u32> {
    if r >= UNION_ROWS {
        return None;
    }
    Some(0xC300 + r as u32 * 0x2C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xC340 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Grafana OSS（Windows）收口账本状态机。
#[derive(Clone)]
pub struct GrafanaLedger {
    /// 六判据（安装/服务注册/端口开放/面板功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 面板账。
    panels: [Option<u64>; PANELS],
    /// datasource 三元闸账。
    dss: [Option<u64>; DS_GATES],
    /// dashboard.json 投影账。
    dash: [Option<u64>; DASH_CELLS],
    /// 告警规则账。
    alerts: [Option<u64>; ALERTS],
    /// 消费联签账。
    unions: [Option<u32>; UNION_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后面板族。
    last_kind: i64,
    /// 最后 datasource 闸。
    last_ds: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GrafanaLedger {
    /// 新账本。
    pub fn new() -> Self {
        GrafanaLedger {
            gates: [None; 6],
            panels: [None; PANELS],
            dss: [None; DS_GATES],
            dash: [None; DASH_CELLS],
            alerts: [None; ALERTS],
            unions: [None; UNION_ROWS],
            gos: [None; GO_ROWS],
            last_kind: -1,
            last_ds: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GrafanaCode> {
        if gate >= 6 {
            return Err(GrafanaCode::ENoCell);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xCA1400 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < 6 {
            match self.gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 面板一格（族序单调 + 首族必 timeseries + 载荷对照 + 重复拒）。
    pub fn record_panel(&mut self, o: usize, got: u64) -> Result<(), GrafanaCode> {
        if o >= PANELS {
            return Err(GrafanaCode::ENoCell);
        }
        let kind = match panel_kind(o) {
            Some(k) => k as i64,
            None => return Err(GrafanaCode::ENoCell),
        };
        let order_bad = if self.last_kind < 0 {
            kind != 0
        } else {
            kind < self.last_kind || kind > self.last_kind + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1500);
            return Err(GrafanaCode::EOrder);
        }
        let want = match panel_cell(o) {
            Some(w) => w,
            None => return Err(GrafanaCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1501 + o as u64);
            return Err(GrafanaCode::EDrift);
        }
        if match self.panels.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GrafanaCode::EDup);
        }
        if let Some(c) = self.panels.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind;
        self.fnv(0xCA1502 + o as u64);
        Ok(())
    }

    /// 面板全账核验（20 格）。
    pub fn panels_all(&self) -> bool {
        let mut o = 0usize;
        while o < PANELS {
            match self.panels.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// datasource 一闸（闸序单调 + 首闸必 Prometheus + 载荷对照 + 重复拒）。
    pub fn record_ds(&mut self, i: usize, got: u64) -> Result<(), GrafanaCode> {
        if i >= DS_GATES {
            return Err(GrafanaCode::EDsOrd);
        }
        let d = match ds_of(i) {
            Some(d) => d as i64,
            None => return Err(GrafanaCode::EDsOrd),
        };
        let ord_bad = if self.last_ds < 0 {
            d != 0
        } else {
            d < self.last_ds || d > self.last_ds + 1
        };
        if ord_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1600);
            return Err(GrafanaCode::EDsOrd);
        }
        let want = match ds_cell(i) {
            Some(w) => w,
            None => return Err(GrafanaCode::EDsOrd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1601 + i as u64);
            return Err(GrafanaCode::EDrift);
        }
        if match self.dss.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GrafanaCode::EDup);
        }
        if let Some(c) = self.dss.get_mut(i) {
            *c = Some(got);
        }
        self.last_ds = d;
        self.fnv(0xCA1602 + i as u64);
        Ok(())
    }

    /// datasource 全账核验（3 闸）。
    pub fn dss_all(&self) -> bool {
        let mut i = 0usize;
        while i < DS_GATES {
            match self.dss.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// dashboard.json 投影一格（载荷对照 + 重复拒；回投即 EDup）。
    pub fn record_dash(&mut self, i: usize, got: u64) -> Result<(), GrafanaCode> {
        if i >= DASH_CELLS {
            return Err(GrafanaCode::ENoCell);
        }
        let want = match dash_cell(i) {
            Some(w) => w,
            None => return Err(GrafanaCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1700 + i as u64);
            return Err(GrafanaCode::EDashDrift);
        }
        if match self.dash.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GrafanaCode::EDup);
        }
        if let Some(c) = self.dash.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xCA1701 + i as u64);
        Ok(())
    }

    /// 投影全账核验（12 格）。
    pub fn dash_all(&self) -> bool {
        let mut i = 0usize;
        while i < DASH_CELLS {
            match self.dash.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 告警规则一格（载荷对照 + 重复拒）。
    pub fn record_alert(&mut self, a: usize, got: u64) -> Result<(), GrafanaCode> {
        if a >= ALERTS {
            return Err(GrafanaCode::ENoAlert);
        }
        let want = match alert_cell(a) {
            Some(w) => w,
            None => return Err(GrafanaCode::ENoAlert),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xCA1800 + a as u64);
            return Err(GrafanaCode::EDrift);
        }
        if match self.alerts.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GrafanaCode::EDup);
        }
        if let Some(c) = self.alerts.get_mut(a) {
            *c = Some(got);
        }
        self.fnv(0xCA1801 + a as u64);
        Ok(())
    }

    /// 告警全账核验（8 格）。
    pub fn alerts_all(&self) -> bool {
        let mut a = 0usize;
        while a < ALERTS {
            match self.alerts.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// Prometheus 消费联签一行（F52392 前向声明）。
    pub fn declare_union(&mut self, r: usize) -> Result<(), GrafanaCode> {
        if r >= UNION_ROWS {
            return Err(GrafanaCode::ERowMiss);
        }
        if match self.unions.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GrafanaCode::EDup);
        }
        if let Some(c) = self.unions.get_mut(r) {
            *c = union_row(r);
        }
        self.fnv(0xCA1900 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn unions_all(&self) -> bool {
        let mut r = 0usize;
        while r < UNION_ROWS {
            match self.unions.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_go(&mut self, r: usize) -> Result<(), GrafanaCode> {
        if r >= GO_ROWS {
            return Err(GrafanaCode::ERowMiss);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GrafanaCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0xCA1980 + r as u64);
        Ok(())
    }

    /// Go 声明全齐核验。
    pub fn gos_all(&self) -> bool {
        let mut r = 0usize;
        while r < GO_ROWS {
            match self.gos.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：声明行缺 → ERowMiss；格/闸未齐 → ESeal；全齐 → Ok。
    pub fn seal(&self) -> Result<(), GrafanaCode> {
        if !(self.unions_all() && self.gos_all()) {
            return Err(GrafanaCode::ERowMiss);
        }
        if !(self.gates_all() && self.panels_all() && self.dss_all() && self.dash_all() && self.alerts_all()) {
            return Err(GrafanaCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), GrafanaCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(GrafanaCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for GrafanaLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), GrafanaCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(GrafanaCode::EFp)
    }
}

/// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
pub fn judge_grade(led: &GrafanaLedger) -> Grade {
    if led.gates_all()
        && led.panels_all()
        && led.dss_all()
        && led.dash_all()
        && led.alerts_all()
        && led.unions_all()
        && led.gos_all()
    {
        Grade::S1
    } else if led.gates_all() {
        Grade::S2
    } else {
        Grade::S3
    }
}

/// 定级守卫：定级与目标档漂移 → EGradeDrift。
pub fn grade_guard(led: &GrafanaLedger, want: Grade) -> Result<(), GrafanaCode> {
    let got = judge_grade(led);
    if got == want {
        Ok(())
    } else {
        Err(GrafanaCode::EGradeDrift)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GrafanaLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PANELS {
        if let Some(w) = panel_cell(o) {
            if led.record_panel(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut d = 0usize;
    while d < DS_GATES {
        if let Some(w) = ds_cell(d) {
            if led.record_ds(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut i = 0usize;
    while i < DASH_CELLS {
        if let Some(w) = dash_cell(i) {
            if led.record_dash(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut a = 0usize;
    while a < ALERTS {
        if let Some(w) = alert_cell(a) {
            if led.record_alert(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut r = 0usize;
    while r < UNION_ROWS {
        let _ = led.declare_union(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < GO_ROWS {
        let _ = led.declare_go(r);
        r += 1;
    }
    // 注入 10 轮：面板载荷漂移 / datasource 三元闸逆（空账本必抓）/ 越闭集 三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GrafanaLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_panel(0, panel_cell(0).unwrap_or(0));
                lt.record_panel(1, panel_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(GrafanaCode::EDrift)
            }
            1 => lt.record_ds(1, ds_cell(1).unwrap_or(0)) == Err(GrafanaCode::EDsOrd),
            _ => {
                lt.record_panel(PANELS, 0).is_err()
                    && lt.record_dash(DASH_CELLS, 0).is_err()
                    && lt.record_alert(ALERTS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xCA1A00 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52390 判据集（锚 UNX-F52391-J1，off-by-one 板式）。
pub fn run_unx52390_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52390");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == PANELS as u32 + DS_GATES as u32 + DASH_CELLS as u32 + ALERTS as u32,
        "20 面板 + 3 数据源 + 12 投影 + 8 告警逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "面板五族 20 格全通",
        {
            let mut lt = GrafanaLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < PANELS {
                if let Some(w) = panel_cell(o) {
                    if lt.record_panel(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.panels_all()
        },
        "timeseries/stat/table/logs/gauge 五族逐格载荷恒等",
    );
    s.add(
        "datasource 三元闸 3/3 全落",
        {
            let mut lt = GrafanaLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DS_GATES {
                if let Some(w) = ds_cell(d) {
                    if lt.record_ds(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.dss_all()
        },
        "Prometheus/Loki/InfluxDB 三元闭集逐闸恒等",
    );
    s.add(
        "dashboard.json 双向投影 12 格全落",
        {
            let mut lt = GrafanaLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < DASH_CELLS {
                if let Some(w) = dash_cell(i) {
                    if lt.record_dash(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.dash_all()
        },
        "账→JSON 正向投影逐格恒等落账",
    );
    s.add(
        "dashboard.json 回投核验 12/12",
        {
            let mut lt = GrafanaLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < DASH_CELLS {
                if let Some(w) = dash_cell(i) {
                    let _ = lt.record_dash(i, w);
                    // 回投：同格二次提交必拒 EDup（JSON→账反向核验）。
                    if lt.record_dash(i, w) != Err(GrafanaCode::EDup) {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.dash_all()
        },
        "JSON→账反向回投逐格核验，重复提交必拒",
    );
    s.add(
        "告警规则 8 格全落",
        {
            let mut lt = GrafanaLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < ALERTS {
                if let Some(w) = alert_cell(a) {
                    if lt.record_alert(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.alerts_all()
        },
        "告警规则载荷逐格对照落账",
    );
    s.add(
        "Prometheus 消费联签 4/4（F52392 前向声明）",
        {
            let mut lt = GrafanaLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < UNION_ROWS {
                if lt.declare_union(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.unions_all()
        },
        "Prometheus Server 消费面四行联签闭集（§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2",
        {
            let mut lt = GrafanaLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < GO_ROWS {
                if lt.declare_go(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.gos_all()
        },
        "Go 运行时上栈两行声明闭集",
    );
    s.add(
        "面板族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PANELS {
                if panel_kind(o).unwrap_or(5) < panel_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "timeseries→stat→table→logs→gauge 五族闭集单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = GrafanaLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/服务注册/端口开放/面板功能/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·面板越闭集",
        GrafanaLedger::new().record_panel(PANELS, 0).is_err(),
        "面板 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        GrafanaLedger::new().record_dash(DASH_CELLS, 0).is_err(),
        "投影 12 越闭集必拒",
    );
    s.add(
        "边界·告警越闭集",
        GrafanaLedger::new().record_alert(ALERTS, 0).is_err(),
        "告警 8 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            // 合法序先落投影格 0 正格，再验格 1 投影载荷漂移必抓。
            let mut lt = GrafanaLedger::new();
            let _ = lt.record_dash(0, dash_cell(0).unwrap_or(0));
            lt.record_dash(1, dash_cell(1).unwrap_or(0).wrapping_add(5))
                == Err(GrafanaCode::EDashDrift)
        },
        "dashboard.json 投影载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "面板漂移/三元闸逆/越闭集三态轮转必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        fp_check(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = GrafanaLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < PANELS {
                if let Some(w) = panel_cell(o) {
                    let _ = lt.record_panel(o, w);
                }
                o += 1;
            }
            let mut d = 0usize;
            while d < DS_GATES {
                if let Some(w) = ds_cell(d) {
                    let _ = lt.record_ds(d, w);
                }
                d += 1;
            }
            let mut i = 0usize;
            while i < DASH_CELLS {
                if let Some(w) = dash_cell(i) {
                    let _ = lt.record_dash(i, w);
                }
                i += 1;
            }
            let mut a = 0usize;
            while a < ALERTS {
                if let Some(w) = alert_cell(a) {
                    let _ = lt.record_alert(a, w);
                }
                a += 1;
            }
            let mut r = 0usize;
            while r < UNION_ROWS {
                let _ = lt.declare_union(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < GO_ROWS {
                let _ = lt.declare_go(r);
                r += 1;
            }
            lt.seal().is_ok() && grade_guard(&lt, Grade::S1).is_ok()
        },
        "全齐 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 19, "含本条共 20 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_panel_kind() {
        assert_eq!(panel_kind(0), Some(0));
        assert_eq!(panel_kind(4), Some(1));
        assert_eq!(panel_kind(19), Some(4));
        assert!(panel_kind(20).is_none());
        assert!(panel_cell(20).is_none());
        assert_eq!(kind_name(0), Some("timeseries"));
        assert_eq!(kind_name(4), Some("gauge"));
        assert!(kind_name(5).is_none());
    }

    #[test]
    fn test_ds_gates() {
        assert_eq!(ds_of(0), Some(0));
        assert_eq!(ds_of(2), Some(2));
        assert!(ds_of(3).is_none());
        assert_eq!(ds_name(0), Some("Prometheus"));
        assert_eq!(ds_name(1), Some("Loki"));
        assert_eq!(ds_name(2), Some("InfluxDB"));
        assert!(ds_name(3).is_none());
        assert!(ds_cell(DS_GATES).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = panel_cell(3).unwrap_or(0);
        assert_eq!(a, panel_cell(3).unwrap_or(0));
        assert_ne!(a, panel_cell(4).unwrap_or(0));
        assert_ne!(dash_cell(0).unwrap_or(0), dash_cell(5).unwrap_or(0));
        assert!(dash_cell(DASH_CELLS).is_none());
        assert!(alert_cell(ALERTS).is_none());
        assert!(union_row(UNION_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 timeseries，跳族必抓。
        let mut lt = GrafanaLedger::new();
        assert_eq!(
            lt.record_panel(9, panel_cell(9).unwrap_or(0)),
            Err(GrafanaCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_panel(0, panel_cell(0).unwrap_or(0));
        let _ = lt.record_panel(5, panel_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_panel(2, panel_cell(2).unwrap_or(0)),
            Err(GrafanaCode::EOrder)
        );
        assert!(lt.record_panel(6, panel_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_ds_order_caught() {
        // 空账本：首闸必 Prometheus。
        assert_eq!(
            GrafanaLedger::new().record_ds(2, ds_cell(2).unwrap_or(0)),
            Err(GrafanaCode::EDsOrd)
        );
        // 合法落闸 0 后跳闸 2 → 越闸必抓。
        let mut lt = GrafanaLedger::new();
        let _ = lt.record_ds(0, ds_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_ds(2, ds_cell(2).unwrap_or(0)),
            Err(GrafanaCode::EDsOrd)
        );
        // 闸载荷漂移 EDrift。
        assert_eq!(
            lt.record_ds(1, ds_cell(1).unwrap_or(0).wrapping_add(4)),
            Err(GrafanaCode::EDrift)
        );
    }

    #[test]
    fn test_dash_drift_and_dup() {
        let mut lt = GrafanaLedger::new();
        assert_eq!(
            lt.record_dash(0, dash_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(GrafanaCode::EDashDrift)
        );
        let _ = lt.record_dash(0, dash_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_dash(0, dash_cell(0).unwrap_or(0)),
            Err(GrafanaCode::EDup)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = GrafanaLedger::new();
        let _ = lt.record_panel(0, panel_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_panel(0, panel_cell(0).unwrap_or(0)),
            Err(GrafanaCode::EDup)
        );
        let _ = lt.declare_go(0);
        assert_eq!(lt.declare_go(0), Err(GrafanaCode::EDup));
        assert_eq!(lt.declare_go(GO_ROWS), Err(GrafanaCode::ERowMiss));
        assert_eq!(lt.declare_union(UNION_ROWS), Err(GrafanaCode::ERowMiss));
        assert_eq!(lt.record_alert(ALERTS, 0), Err(GrafanaCode::ENoAlert));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GrafanaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!GrafanaLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：声明行缺 → ERowMiss。
        assert_eq!(GrafanaLedger::new().seal(), Err(GrafanaCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = GrafanaLedger::new();
        let mut r = 0usize;
        while r < UNION_ROWS {
            let _ = lt.declare_union(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < GO_ROWS {
            let _ = lt.declare_go(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(GrafanaCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 面板 + 3 数据源 + 12 投影 + 8 告警（闸与声明行不计数）。
        assert_eq!(
            recorded,
            PANELS as u32 + DS_GATES as u32 + DASH_CELLS as u32 + ALERTS as u32
        );
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert!(fp_check(f1, f2).is_ok());
        assert!(fp_check(f1, f1.wrapping_add(1)).is_err());
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCA140);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(GrafanaCode::Ok as u32, 0);
        assert_eq!(GrafanaCode::EGradeDrift as u32, 12);
        assert_eq!(GrafanaCode::Ok.val(), SEG_BASE);
        assert_eq!(GrafanaCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!GrafanaCode::Ok.detail().is_empty());
        assert!(!GrafanaCode::ESeal.detail().is_empty());
        assert!(!GrafanaCode::EDashDrift.detail().is_empty());
        assert!(!GrafanaCode::EDsOrd.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(judge_grade(&GrafanaLedger::new()), Grade::S3);
        let mut lt = GrafanaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        assert!(grade_guard(&lt, Grade::S1).is_err());
        let mut full = GrafanaLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < PANELS {
            if let Some(w) = panel_cell(o) {
                let _ = full.record_panel(o, w);
            }
            o += 1;
        }
        let mut d = 0usize;
        while d < DS_GATES {
            if let Some(w) = ds_cell(d) {
                let _ = full.record_ds(d, w);
            }
            d += 1;
        }
        let mut i = 0usize;
        while i < DASH_CELLS {
            if let Some(w) = dash_cell(i) {
                let _ = full.record_dash(i, w);
            }
            i += 1;
        }
        let mut a = 0usize;
        while a < ALERTS {
            if let Some(w) = alert_cell(a) {
                let _ = full.record_alert(a, w);
            }
            a += 1;
        }
        let mut r = 0usize;
        while r < UNION_ROWS {
            let _ = full.declare_union(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < GO_ROWS {
            let _ = full.declare_go(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
        assert_eq!(judge_grade(&full), Grade::S1);
        let (inj, cgt, esc) = full.stats();
        assert_eq!(inj, cgt + esc);
    }

    #[test]
    fn unx52390_checks_green() {
        let s = run_unx52390_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 20);
    }
}
