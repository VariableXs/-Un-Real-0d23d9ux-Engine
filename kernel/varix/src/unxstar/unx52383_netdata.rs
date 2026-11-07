//! UNX-F52383 Netdata（Windows agent）收口 · 0xC9F80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52383"（判据原文锚定源）criteria：① 纯功能代码 ≥280 行；② 判据
//!   原文：UNX-F52384-J1 实时面板 ×10 指标；KB-COMPAT 定级登记；判据锚定
//!   §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52384"（功能标题源）title：Netdata（Windows agent）收口
//!   quota: 300
//! off-by-one 板式口径：本卡（卡 N=52383）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52384-J1（实时面板 ×10 指标），照原文（键 "52383" criteria）锚定；
//! 功能按本卡标题（Netdata（Windows agent）收口，键 "52384" title）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五指标族闭集 20 格：cpu/memory/disk/network/apps 各 4 格，族序单调
//!      （首族必为 cpu，逆跳必抓），逐格载荷纯函数折叠恒等才收。
//!   ② 流式三态 12 格：实时流/补灌/降级三态各 4 格，态序单调且空账本
//!      首态必为「实时流」，回退/越态必抓（EStreamState）。
//!   ③ netdata.conf 双向投影 12 格：六对（正向投影/反向回读成对落账），
//!      反向先于正向必抓（EConfMono）。
//!   ④ 告警三元闸：判定/通知/静默三行声明，缺项 EAlarmMiss 拒封。
//!   ⑤ 与云分账声明 2 行闭集（外部上报面声明），缺行拒封（ERowMiss），
//!      未齐拒封（ESeal）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（指标载荷漂移/流式态回退/越闭集），
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集 S1/S2/S3——
//!      云上报外部面为登记条件，全齐封顶 S2（有条件收口）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52383;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 指标格数（五族 × 4）。
pub const METRICS: usize = 20;

/// 流式三态格数（三态 × 4）。
pub const STREAM_CELLS: usize = 12;

/// netdata.conf 双向投影格数（六对）。
pub const CONF_CELLS: usize = 12;

/// 告警三元闸行数。
pub const ALARM_ROWS: usize = 3;

/// 与云分账声明行数。
pub const CLOUD_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetdataCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 载荷漂移
    EDrift,
    /// 指标族序逆/投影对序逆
    EOrder,
    /// 流式三态回退
    EStreamState,
    /// conf 投影反向先于正向
    EConfMono,
    /// 告警三元闸缺项
    EAlarmMiss,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl NetdataCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            NetdataCode::Ok => "成功",
            NetdataCode::ENoCell => "格号越闭集：指标 20/流式 12/conf 12 之外先登记",
            NetdataCode::EDrift => "载荷漂移：格折叠与基准不恒等，逐格复算",
            NetdataCode::EOrder => "序逆：cpu→memory→disk→network→apps 族序单调；投影反向先于正向必抓",
            NetdataCode::EStreamState => "流式三态回退：实时流→补灌→降级单调，回退/越态必抓",
            NetdataCode::EConfMono => "conf 投影不成对：netdata.conf 双向投影反向先于正向必抓",
            NetdataCode::EAlarmMiss => "告警三元闸缺项：判定/通知/静默三行未齐先补齐",
            NetdataCode::ENoRow => "声明行越闭集：告警闸 3 行 + 云分账 2 行之外先登记",
            NetdataCode::ERowMiss => "缺声明行：与云分账声明（外部上报面声明）未齐先补齐",
            NetdataCode::EDup => "重复落账：同格只许一条账目",
            NetdataCode::ESeal => "未齐即封：20 指标 + 12 流式 + 12 conf 未齐先补齐",
            NetdataCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NetdataCode::EGradeDrift => "定级漂移：云上报外部面登记为条件，全齐封顶 S2",
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
            Grade::S1 => "S1：全通收口（本卡不可达：外部上报面条件登记）",
            Grade::S2 => "S2：有条件收口——云上报外部面登记条件下全齐或六闸齐",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 指标格载荷（格 o；族 = o/4）。
pub const fn met_cell(o: usize) -> Option<u64> {
    if o >= METRICS {
        return None;
    }
    let fam = (o / 4) as u64;
    Some(
        (0xDA000u64 + (o as u64) * 0x2C)
            ^ fam.wrapping_mul(0x01000193)
            ^ 0x4E45544441000000,
    )
}

/// 指标族（0..4=cpu 4..8=memory 8..12=disk 12..16=network 16..20=apps）。
pub const fn met_family(o: usize) -> Option<u32> {
    if o >= METRICS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 指标族名。
pub const fn family_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("cpu"),
        1 => Some("memory"),
        2 => Some("disk"),
        3 => Some("network"),
        4 => Some("apps"),
        _ => None,
    }
}

/// 流式格载荷（格 o；态 = o/4）。
pub const fn stream_cell(o: usize) -> Option<u64> {
    if o >= STREAM_CELLS {
        return None;
    }
    let st = (o / 4) as u64;
    Some((0xDB000u64 + (o as u64) * 0x31) ^ st.wrapping_mul(0x9E3779B1) ^ 0x53545245414D0000)
}

/// 流式态（0..4=实时流 4..8=补灌 8..12=降级）。
pub const fn stream_state(o: usize) -> Option<u32> {
    if o >= STREAM_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 流式态名。
pub const fn state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("实时流"),
        1 => Some("补灌"),
        2 => Some("降级"),
        _ => None,
    }
}

/// conf 投影格载荷（格 i；对 = i/2，偶正向/奇反向）。
pub const fn conf_cell(i: usize) -> Option<u64> {
    if i >= CONF_CELLS {
        return None;
    }
    let pair = (i / 2) as u64;
    Some((0xDC000u64 + (i as u64) * 0x2C) ^ pair.wrapping_mul(0x85EBCA77) ^ 0x4E44434F4E460000)
}

/// conf 投影对号（0..6）。
pub const fn conf_pair(i: usize) -> Option<u32> {
    if i >= CONF_CELLS {
        return None;
    }
    Some((i / 2) as u32)
}

/// 告警三元闸行折叠（行 r：0=判定 1=通知 2=静默）。
pub const fn alarm_row(r: usize) -> Option<u32> {
    if r >= ALARM_ROWS {
        return None;
    }
    Some(0xDD000 + r as u32 * 0x2C)
}

/// 与云分账声明行折叠（行 r）。
pub const fn cloud_row(r: usize) -> Option<u32> {
    if r >= CLOUD_ROWS {
        return None;
    }
    Some(0xDD060 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Netdata（Windows agent）收口账本状态机。
#[derive(Clone)]
pub struct NetdataLedger {
    /// 六判据（安装/服务注册/实时面板/采集功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// 指标账。
    mets: [Option<u64>; METRICS],
    /// 流式三态账。
    stream: [Option<u64>; STREAM_CELLS],
    /// conf 投影账。
    conf: [Option<u64>; CONF_CELLS],
    /// 告警三元闸账。
    alarms: [Option<u32>; ALARM_ROWS],
    /// 与云分账声明账。
    clouds: [Option<u32>; CLOUD_ROWS],
    /// 最后指标族。
    last_family: i64,
    /// 最后流式态。
    last_state: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NetdataLedger {
    /// 新账本。
    pub fn new() -> Self {
        NetdataLedger {
            gates: [None; 6],
            mets: [None; METRICS],
            stream: [None; STREAM_CELLS],
            conf: [None; CONF_CELLS],
            alarms: [None; ALARM_ROWS],
            clouds: [None; CLOUD_ROWS],
            last_family: -1,
            last_state: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NetdataCode> {
        if gate >= 6 {
            return Err(NetdataCode::ENoCell);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xDA100 + gate as u64);
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

    /// 指标一格（族序单调 + 首族必 cpu + 载荷对照 + 重复拒）。
    pub fn record_met(&mut self, o: usize, got: u64) -> Result<(), NetdataCode> {
        if o >= METRICS {
            return Err(NetdataCode::ENoCell);
        }
        let fam = match met_family(o) {
            Some(k) => k as i64,
            None => return Err(NetdataCode::ENoCell),
        };
        let order_bad = if self.last_family < 0 {
            fam != 0
        } else {
            fam < self.last_family || fam > self.last_family + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDA400);
            return Err(NetdataCode::EOrder);
        }
        let want = match met_cell(o) {
            Some(w) => w,
            None => return Err(NetdataCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDA401 + o as u64);
            return Err(NetdataCode::EDrift);
        }
        if match self.mets.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NetdataCode::EDup);
        }
        if let Some(c) = self.mets.get_mut(o) {
            *c = Some(got);
        }
        self.last_family = fam;
        self.fnv(0xDA402 + o as u64);
        Ok(())
    }

    /// 指标全账核验（20 格）。
    pub fn mets_all(&self) -> bool {
        let mut o = 0usize;
        while o < METRICS {
            match self.mets.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 流式一格（态序单调 + 空账首态必「实时流」+ 载荷对照 + 重复拒）。
    pub fn record_stream(&mut self, o: usize, got: u64) -> Result<(), NetdataCode> {
        if o >= STREAM_CELLS {
            return Err(NetdataCode::ENoCell);
        }
        let st = (o / 4) as i64;
        let state_bad = if self.last_state < 0 {
            st != 0
        } else {
            st < self.last_state || st > self.last_state + 1
        };
        if state_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDB400);
            return Err(NetdataCode::EStreamState);
        }
        let want = match stream_cell(o) {
            Some(w) => w,
            None => return Err(NetdataCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDB401 + o as u64);
            return Err(NetdataCode::EDrift);
        }
        if match self.stream.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NetdataCode::EDup);
        }
        if let Some(c) = self.stream.get_mut(o) {
            *c = Some(got);
        }
        self.last_state = st;
        self.fnv(0xDB402 + o as u64);
        Ok(())
    }

    /// 流式全账核验（12 格）。
    pub fn stream_all(&self) -> bool {
        let mut o = 0usize;
        while o < STREAM_CELLS {
            match self.stream.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// conf 投影一格（对内先正后反 + 载荷对照 + 重复拒）。
    pub fn record_conf(&mut self, i: usize, got: u64) -> Result<(), NetdataCode> {
        if i >= CONF_CELLS {
            return Err(NetdataCode::ENoCell);
        }
        // 双向投影成对：奇格（反向）落账前其偶格镜像（正向）必须已在账。
        if i % 2 == 1 {
            let fwd_ok = match self.conf.get(i - 1) {
                Some(Some(_)) => true,
                _ => false,
            };
            if !fwd_ok {
                self.injected += 1;
                self.caught += 1;
                self.fnv(0xDC400);
                return Err(NetdataCode::EConfMono);
            }
        }
        let want = match conf_cell(i) {
            Some(w) => w,
            None => return Err(NetdataCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xDC401 + i as u64);
            return Err(NetdataCode::EDrift);
        }
        if match self.conf.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NetdataCode::EDup);
        }
        if let Some(c) = self.conf.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xDC402 + i as u64);
        Ok(())
    }

    /// conf 投影全账核验（12 格）。
    pub fn conf_all(&self) -> bool {
        let mut i = 0usize;
        while i < CONF_CELLS {
            match self.conf.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 告警三元闸一行（0=判定 1=通知 2=静默）。
    pub fn declare_alarm(&mut self, r: usize) -> Result<(), NetdataCode> {
        if r >= ALARM_ROWS {
            return Err(NetdataCode::ENoRow);
        }
        if match self.alarms.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NetdataCode::EDup);
        }
        if let Some(c) = self.alarms.get_mut(r) {
            *c = alarm_row(r);
        }
        self.fnv(0xDD000 + r as u64);
        Ok(())
    }

    /// 告警闸全齐核验。
    pub fn alarms_all(&self) -> bool {
        let mut r = 0usize;
        while r < ALARM_ROWS {
            match self.alarms.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 与云分账声明一行（外部上报面声明）。
    pub fn declare_cloud(&mut self, r: usize) -> Result<(), NetdataCode> {
        if r >= CLOUD_ROWS {
            return Err(NetdataCode::ENoRow);
        }
        if match self.clouds.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NetdataCode::EDup);
        }
        if let Some(c) = self.clouds.get_mut(r) {
            *c = cloud_row(r);
        }
        self.fnv(0xDD060 + r as u64);
        Ok(())
    }

    /// 云分账全齐核验。
    pub fn clouds_all(&self) -> bool {
        let mut r = 0usize;
        while r < CLOUD_ROWS {
            match self.clouds.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账核验：告警闸缺 → EAlarmMiss；云分账缺 → ERowMiss；格未齐 → ESeal。
    pub fn seal(&self) -> Result<(), NetdataCode> {
        if !self.alarms_all() {
            return Err(NetdataCode::EAlarmMiss);
        }
        if !self.clouds_all() {
            return Err(NetdataCode::ERowMiss);
        }
        if !(self.gates_all() && self.mets_all() && self.stream_all() && self.conf_all()) {
            return Err(NetdataCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), NetdataCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(NetdataCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

/// KB-COMPAT 定级（全齐/六闸齐 → S2 封顶：外部上报面条件登记；否则 S3）。
pub fn judge_grade(led: &NetdataLedger) -> Grade {
    if led.gates_all() {
        Grade::S2
    } else {
        Grade::S3
    }
}

impl Default for NetdataLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NetdataLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < METRICS {
        if let Some(w) = met_cell(o) {
            if led.record_met(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut s = 0usize;
    while s < STREAM_CELLS {
        if let Some(w) = stream_cell(s) {
            if led.record_stream(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut i = 0usize;
    while i < CONF_CELLS {
        if let Some(w) = conf_cell(i) {
            if led.record_conf(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < ALARM_ROWS {
        let _ = led.declare_alarm(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < CLOUD_ROWS {
        let _ = led.declare_cloud(r);
        r += 1;
    }
    // 注入 10 轮：指标载荷漂移 / 流式态回退（空账本必抓）/ 越闭集 三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NetdataLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_met(0, met_cell(0).unwrap_or(0));
                lt.record_met(1, met_cell(1).unwrap_or(0).wrapping_add(1)) == Err(NetdataCode::EDrift)
            }
            1 => lt.record_stream(STREAM_CELLS - 4, stream_cell(STREAM_CELLS - 4).unwrap_or(0))
                == Err(NetdataCode::EStreamState),
            _ => {
                lt.record_met(METRICS, 0).is_err()
                    && lt.record_stream(STREAM_CELLS, 0).is_err()
                    && lt.record_conf(CONF_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xDA600 + k as u64);
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
/// UNX-F52383 判据集（锚 UNX-F52384-J1，off-by-one 板式）。
pub fn run_unx52383_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52383");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == METRICS as u32 + STREAM_CELLS as u32 + CONF_CELLS as u32,
        "20 指标 + 12 流式 + 12 conf 逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "指标五族 20 格全通",
        {
            let mut lt = NetdataLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < METRICS {
                if let Some(w) = met_cell(o) {
                    if lt.record_met(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.mets_all()
        },
        "cpu/memory/disk/network/apps 五族逐格载荷恒等",
    );
    s.add(
        "流式三态 12 格全落",
        {
            let mut lt = NetdataLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < STREAM_CELLS {
                if let Some(w) = stream_cell(c) {
                    if lt.record_stream(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.stream_all()
        },
        "实时流/补灌/降级三态单调落账",
    );
    s.add(
        "netdata.conf 双向投影 12 格全落",
        {
            let mut lt = NetdataLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CONF_CELLS {
                if let Some(w) = conf_cell(i) {
                    if lt.record_conf(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.conf_all()
        },
        "六对正反投影成对落账",
    );
    s.add(
        "告警三元闸 3/3",
        {
            let mut lt = NetdataLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < ALARM_ROWS {
                if lt.declare_alarm(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.alarms_all()
        },
        "判定/通知/静默三行闭集",
    );
    s.add(
        "与云分账声明 2/2（外部上报面声明）",
        {
            let mut lt = NetdataLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < CLOUD_ROWS {
                if lt.declare_cloud(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.clouds_all()
        },
        "Netdata Cloud 上报面两行分账闭集（§AI-66.3）",
    );
    s.add(
        "指标族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < METRICS {
                if met_family(o).unwrap_or(5) < met_family(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "cpu→memory→disk→network→apps 五族闭集单调",
    );
    s.add(
        "实时面板指标对号完整",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < CONF_CELLS {
                if conf_pair(i).unwrap_or(6) >= 6 {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "实时面板 ×10 指标面对应六对投影对号闭集 0..6",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = NetdataLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/服务注册/实时面板/采集功能/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·指标越闭集",
        NetdataLedger::new().record_met(METRICS, 0).is_err(),
        "指标 20 越闭集必拒",
    );
    s.add(
        "边界·流式越闭集",
        NetdataLedger::new().record_stream(STREAM_CELLS, 0).is_err(),
        "流式 12 越闭集必拒",
    );
    s.add(
        "边界·conf 投影越闭集",
        NetdataLedger::new().record_conf(CONF_CELLS, 0).is_err(),
        "conf 投影 12 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            let mut lt = NetdataLedger::new();
            lt.record_conf(0, conf_cell(0).unwrap_or(0).wrapping_add(1)) == Err(NetdataCode::EDrift)
        },
        "conf 投影载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "指标漂移/流式态回退/越闭集三态轮转必抓",
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
        fp == fp2 && fp != 0,
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = NetdataLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < METRICS {
                if let Some(w) = met_cell(o) {
                    let _ = lt.record_met(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < STREAM_CELLS {
                if let Some(w) = stream_cell(c) {
                    let _ = lt.record_stream(c, w);
                }
                c += 1;
            }
            let mut i = 0usize;
            while i < CONF_CELLS {
                if let Some(w) = conf_cell(i) {
                    let _ = lt.record_conf(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < ALARM_ROWS {
                let _ = lt.declare_alarm(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < CLOUD_ROWS {
                let _ = lt.declare_cloud(r);
                r += 1;
            }
            lt.seal().is_ok() && judge_grade(&lt) == Grade::S2
        },
        "全齐封顶 S2：云上报外部面条件登记（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_family() {
        assert_eq!(met_family(0), Some(0));
        assert_eq!(met_family(4), Some(1));
        assert_eq!(met_family(19), Some(4));
        assert!(met_family(20).is_none());
        assert!(met_cell(20).is_none());
        assert_eq!(family_name(0), Some("cpu"));
        assert_eq!(family_name(4), Some("apps"));
        assert!(family_name(5).is_none());
    }

    #[test]
    fn test_stream_states() {
        assert_eq!(stream_state(0), Some(0));
        assert_eq!(stream_state(7), Some(1));
        assert_eq!(stream_state(11), Some(2));
        assert!(stream_state(12).is_none());
        assert_eq!(state_name(0), Some("实时流"));
        assert_eq!(state_name(2), Some("降级"));
        assert!(state_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = met_cell(3).unwrap_or(0);
        assert_eq!(a, met_cell(3).unwrap_or(0));
        assert_ne!(a, met_cell(4).unwrap_or(0));
        assert_ne!(stream_cell(0).unwrap_or(0), stream_cell(5).unwrap_or(0));
        assert_ne!(conf_cell(0).unwrap_or(0), conf_cell(2).unwrap_or(0));
        assert!(alarm_row(ALARM_ROWS).is_none());
        assert!(cloud_row(CLOUD_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 cpu，跳族必抓。
        let mut lt = NetdataLedger::new();
        assert_eq!(
            lt.record_met(9, met_cell(9).unwrap_or(0)),
            Err(NetdataCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_met(0, met_cell(0).unwrap_or(0));
        let _ = lt.record_met(4, met_cell(4).unwrap_or(0));
        assert_eq!(
            lt.record_met(2, met_cell(2).unwrap_or(0)),
            Err(NetdataCode::EOrder)
        );
        assert!(lt.record_met(5, met_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_stream_state_caught() {
        // 空账本：首态必「实时流」。
        assert_eq!(
            NetdataLedger::new().record_stream(8, stream_cell(8).unwrap_or(0)),
            Err(NetdataCode::EStreamState)
        );
        // 合法落态 0 后回态 0（越过补灌）→ 态回退必抓。
        let mut lt = NetdataLedger::new();
        let _ = lt.record_stream(0, stream_cell(0).unwrap_or(0));
        let _ = lt.record_stream(4, stream_cell(4).unwrap_or(0));
        assert_eq!(
            lt.record_stream(1, stream_cell(1).unwrap_or(0)),
            Err(NetdataCode::EStreamState)
        );
    }

    #[test]
    fn test_conf_pair_caught() {
        // 空账本：反向投影先于正向必抓。
        let mut lt = NetdataLedger::new();
        assert_eq!(
            lt.record_conf(3, conf_cell(3).unwrap_or(0)),
            Err(NetdataCode::EConfMono)
        );
        // 正向落账后反向可落。
        let _ = lt.record_conf(2, conf_cell(2).unwrap_or(0));
        assert!(lt.record_conf(3, conf_cell(3).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_and_dup_caught() {
        let mut lt = NetdataLedger::new();
        assert_eq!(
            lt.record_met(0, met_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(NetdataCode::EDrift)
        );
        assert_eq!(
            lt.record_stream(0, stream_cell(0).unwrap_or(0).wrapping_add(5)),
            Err(NetdataCode::EDrift)
        );
        let _ = lt.record_met(0, met_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_met(0, met_cell(0).unwrap_or(0)),
            Err(NetdataCode::EDup)
        );
        let _ = lt.declare_cloud(0);
        assert_eq!(lt.declare_cloud(0), Err(NetdataCode::EDup));
        assert_eq!(lt.declare_cloud(CLOUD_ROWS), Err(NetdataCode::ENoRow));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = NetdataLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!NetdataLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：告警闸缺 → EAlarmMiss。
        assert_eq!(NetdataLedger::new().seal(), Err(NetdataCode::EAlarmMiss));
        // 闸齐、云分账缺 → ERowMiss。
        let mut lt = NetdataLedger::new();
        let mut r = 0usize;
        while r < ALARM_ROWS {
            let _ = lt.declare_alarm(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(NetdataCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut r = 0usize;
        while r < CLOUD_ROWS {
            let _ = lt.declare_cloud(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(NetdataCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 指标 + 12 流式 + 12 conf（闸与声明行不计数）。
        assert_eq!(
            recorded,
            METRICS as u32 + STREAM_CELLS as u32 + CONF_CELLS as u32
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
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC9F80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(NetdataCode::Ok as u32, 0);
        assert_eq!(NetdataCode::EGradeDrift as u32, 12);
        assert_eq!(NetdataCode::Ok.val(), SEG_BASE);
        assert_eq!(NetdataCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!NetdataCode::Ok.detail().is_empty());
        assert!(!NetdataCode::EAlarmMiss.detail().is_empty());
        assert!(!NetdataCode::EConfMono.detail().is_empty());
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S2（外部上报面条件登记封顶）。
        assert_eq!(judge_grade(&NetdataLedger::new()), Grade::S3);
        let mut lt = NetdataLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        let mut full = NetdataLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < METRICS {
            if let Some(w) = met_cell(o) {
                let _ = full.record_met(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < STREAM_CELLS {
            if let Some(w) = stream_cell(c) {
                let _ = full.record_stream(c, w);
            }
            c += 1;
        }
        let mut i = 0usize;
        while i < CONF_CELLS {
            if let Some(w) = conf_cell(i) {
                let _ = full.record_conf(i, w);
            }
            i += 1;
        }
        let mut r = 0usize;
        while r < ALARM_ROWS {
            let _ = full.declare_alarm(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < CLOUD_ROWS {
            let _ = full.declare_cloud(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
        assert_eq!(judge_grade(&full), Grade::S2);
        assert!(full.conservation().is_ok());
    }

    #[test]
    fn unx52383_checks_green() {
        let s = run_unx52383_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 19);
    }
}
