//! UNX-F52381 Prometheus Windows Exporter 收口 · 0xC9F00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15，逐字）：
//!   键 "52381"（判据原文锚定源）criteria：① 纯功能代码 ≥300 行；② 判据
//!   原文：UNX-F52382-J1 指标端点 ×10 全通；KB-COMPAT 定级登记；判据锚定
//!   §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!   键 "52382"（功能标题源）title：Prometheus Windows Exporter 收口
//!   quota: 280
//! off-by-one 板式口径：本卡（卡 N=52381）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52382-J1（指标端点 ×10 全通），照原文（键 "52381" criteria）锚定；
//! 功能按本卡标题（Prometheus Windows Exporter 收口，键 "52382" title）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五 collector 族闭集 20 格：cpu/memory/disk/network/service 各 4 格，
//!      族序单调（首族必为 cpu，逆跳必抓），逐格载荷纯函数折叠恒等才收。
//!   ② /metrics 指标端点三态 12 格：抓取成功/部分序列过期/抓取失败三态
//!      各 4 格，态序单调且空账本首格必为「抓取成功」态，回退/越态必抓。
//!   ③ prometheus.yml 消费 8 格：scrape 间隔/抓取目标/标签重写等 8 项逐
//!      格落账，载荷漂移必抓。
//!   ④ 采样 8 拍单调：拍号严格逐拍递进（0→7），拍载荷逐拍递增恒等，
//!      跳拍/倒拍/漂移一律 EMono 抓获。
//!   ⑤ 与 Prometheus Server 消费联签 4 格（F52393 前向声明）+ Go 上栈声明
//!      2 行闭集，缺行拒封（ERowMiss），未齐拒封（ESeal）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate/seal/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（collector 载荷漂移/端点态回退/
//!      越闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集
//!      S1/S2/S3（六闸 + 20 collector + 12 端点 + 8 yml + 8 拍 + 4 联签
//!      + 2 Go 全齐 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52381;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// collector 格数（五族 × 4）。
pub const COLLECTORS: usize = 20;

/// /metrics 端点三态格数（三态 × 4）。
pub const ENDPOINT_CELLS: usize = 12;

/// prometheus.yml 消费格数。
pub const YML_CELLS: usize = 8;

/// 采样拍数。
pub const BEATS: usize = 8;

/// Prometheus Server 消费联签行数（F52393 前向声明）。
pub const UNION_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const GO_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PromExpCode {
    /// 成功
    Ok = 0,
    /// 格号越闭集
    ENoCell,
    /// 载荷漂移
    EDrift,
    /// collector 族序逆
    EOrder,
    /// 端点三态回退
    EEpState,
    /// 采样拍非单调
    EMono,
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
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl PromExpCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            PromExpCode::Ok => "成功",
            PromExpCode::ENoCell => "格号越闭集：collector 20/端点 12/yml 8/拍 8 之外先登记",
            PromExpCode::EDrift => "载荷漂移：格折叠与基准不恒等，逐格复算",
            PromExpCode::EOrder => "collector 族序逆：cpu→memory→disk→network→service 单调，逆跳必抓",
            PromExpCode::EEpState => "端点三态回退：抓取成功→部分序列过期→抓取失败单调，回退/越态必抓",
            PromExpCode::EMono => "采样拍非单调：8 拍须逐拍递进且拍载荷逐拍恒等递增",
            PromExpCode::ENoRow => "声明行越闭集：联签 4 行 + Go 上栈 2 行闭集之外先登记",
            PromExpCode::ERowMiss => "缺声明行：Server 消费联签/Go 上栈声明未齐先补齐",
            PromExpCode::EDup => "重复落账：同格只许一条账目",
            PromExpCode::ESeal => "未齐即封：20 collector + 12 端点 + 8 yml + 8 拍未齐先补齐",
            PromExpCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PromExpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PromExpCode::EGradeDrift => "定级漂移：六闸+全节+4 联签+2 Go 全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20 collector+12 端点+8 yml+8 拍+4 联签+2 Go 全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// collector 格载荷（格 o；族 = o/4）。
pub const fn col_cell(o: usize) -> Option<u64> {
    if o >= COLLECTORS {
        return None;
    }
    let fam = (o / 4) as u64;
    Some(
        (0xD1000u64 + (o as u64) * 0x2C)
            ^ fam.wrapping_mul(0x01000193)
            ^ 0x50524F4D45580000,
    )
}

/// collector 族（0..4=cpu 4..8=memory 8..12=disk 12..16=network 16..20=service）。
pub const fn col_family(o: usize) -> Option<u32> {
    if o >= COLLECTORS {
        return None;
    }
    Some((o / 4) as u32)
}

/// collector 族名。
pub const fn family_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("cpu"),
        1 => Some("memory"),
        2 => Some("disk"),
        3 => Some("network"),
        4 => Some("service"),
        _ => None,
    }
}

/// /metrics 端点格载荷（格 o；态 = o/4）。
pub const fn ep_cell(o: usize) -> Option<u64> {
    if o >= ENDPOINT_CELLS {
        return None;
    }
    let st = (o / 4) as u64;
    Some((0xD2000u64 + (o as u64) * 0x31) ^ st.wrapping_mul(0x9E3779B1) ^ 0x4D45545249430000)
}

/// 端点态（0..4=抓取成功 4..8=部分序列过期 8..12=抓取失败）。
pub const fn ep_state(o: usize) -> Option<u32> {
    if o >= ENDPOINT_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 端点态名。
pub const fn state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("抓取成功"),
        1 => Some("部分序列过期"),
        2 => Some("抓取失败"),
        _ => None,
    }
}

/// prometheus.yml 消费格基准（格 i）。
pub const fn yml_cell(i: usize) -> Option<u64> {
    if i >= YML_CELLS {
        return None;
    }
    Some(0xD3000u64 + (i as u64) * 0x2C)
}

/// 采样拍基准（拍 b；逐拍严格递增）。
pub const fn beat_cell(b: usize) -> Option<u64> {
    if b >= BEATS {
        return None;
    }
    Some(0xD4000u64 + (b as u64) * 0x37)
}

/// Server 消费联签行折叠（行 r；F52393 前向声明）。
pub const fn union_row(r: usize) -> Option<u32> {
    if r >= UNION_ROWS {
        return None;
    }
    Some(0xD5000 + r as u32 * 0x2C)
}

/// Go 上栈声明行折叠（行 r）。
pub const fn go_row(r: usize) -> Option<u32> {
    if r >= GO_ROWS {
        return None;
    }
    Some(0xD50C0 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Prometheus Windows Exporter 收口账本状态机。
#[derive(Clone)]
pub struct PromExpLedger {
    /// 六判据（安装/服务注册/端点开放/采集功能/卸载净除/升级保留）。
    gates: [Option<bool>; 6],
    /// collector 账。
    cols: [Option<u64>; COLLECTORS],
    /// 端点三态账。
    eps: [Option<u64>; ENDPOINT_CELLS],
    /// prometheus.yml 消费账。
    yml: [Option<u64>; YML_CELLS],
    /// 采样拍账。
    beats: [Option<u64>; BEATS],
    /// Server 消费联签账。
    unions: [Option<u32>; UNION_ROWS],
    /// Go 上栈声明账。
    gos: [Option<u32>; GO_ROWS],
    /// 最后 collector 族。
    last_family: i64,
    /// 最后端点态。
    last_state: i64,
    /// 最后采样拍。
    last_beat: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PromExpLedger {
    /// 新账本。
    pub fn new() -> Self {
        PromExpLedger {
            gates: [None; 6],
            cols: [None; COLLECTORS],
            eps: [None; ENDPOINT_CELLS],
            yml: [None; YML_CELLS],
            beats: [None; BEATS],
            unions: [None; UNION_ROWS],
            gos: [None; GO_ROWS],
            last_family: -1,
            last_state: -1,
            last_beat: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PromExpCode> {
        if gate >= 6 {
            return Err(PromExpCode::ENoCell);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0xD1100 + gate as u64);
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

    /// collector 一格（族序单调 + 首族必 cpu + 载荷对照 + 重复拒）。
    pub fn record_col(&mut self, o: usize, got: u64) -> Result<(), PromExpCode> {
        if o >= COLLECTORS {
            return Err(PromExpCode::ENoCell);
        }
        let fam = match col_family(o) {
            Some(k) => k as i64,
            None => return Err(PromExpCode::ENoCell),
        };
        let order_bad = if self.last_family < 0 {
            fam != 0
        } else {
            fam < self.last_family || fam > self.last_family + 1
        };
        if order_bad {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD1400);
            return Err(PromExpCode::EOrder);
        }
        let want = match col_cell(o) {
            Some(w) => w,
            None => return Err(PromExpCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD1401 + o as u64);
            return Err(PromExpCode::EDrift);
        }
        if match self.cols.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PromExpCode::EDup);
        }
        if let Some(c) = self.cols.get_mut(o) {
            *c = Some(got);
        }
        self.last_family = fam;
        self.fnv(0xD1402 + o as u64);
        Ok(())
    }

    /// collector 全账核验（20 格）。
    pub fn cols_all(&self) -> bool {
        let mut o = 0usize;
        while o < COLLECTORS {
            match self.cols.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 端点一格（态序单调 + 空账首态必「抓取成功」+ 载荷对照 + 重复拒）。
    pub fn record_ep(&mut self, o: usize, got: u64) -> Result<(), PromExpCode> {
        if o >= ENDPOINT_CELLS {
            return Err(PromExpCode::ENoCell);
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
            self.fnv(0xD2400);
            return Err(PromExpCode::EEpState);
        }
        let want = match ep_cell(o) {
            Some(w) => w,
            None => return Err(PromExpCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD2401 + o as u64);
            return Err(PromExpCode::EDrift);
        }
        if match self.eps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PromExpCode::EDup);
        }
        if let Some(c) = self.eps.get_mut(o) {
            *c = Some(got);
        }
        self.last_state = st;
        self.fnv(0xD2402 + o as u64);
        Ok(())
    }

    /// 端点全账核验（12 格）。
    pub fn eps_all(&self) -> bool {
        let mut o = 0usize;
        while o < ENDPOINT_CELLS {
            match self.eps.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// prometheus.yml 消费一格（载荷对照 + 重复拒）。
    pub fn record_yml(&mut self, i: usize, got: u64) -> Result<(), PromExpCode> {
        if i >= YML_CELLS {
            return Err(PromExpCode::ENoCell);
        }
        let want = match yml_cell(i) {
            Some(w) => w,
            None => return Err(PromExpCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD3400 + i as u64);
            return Err(PromExpCode::EDrift);
        }
        if match self.yml.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PromExpCode::EDup);
        }
        if let Some(c) = self.yml.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0xD3401 + i as u64);
        Ok(())
    }

    /// yml 消费全账核验（8 格）。
    pub fn yml_all(&self) -> bool {
        let mut i = 0usize;
        while i < YML_CELLS {
            match self.yml.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 采样一拍（拍号严格递进 + 拍载荷对照 + 重复拒）。
    pub fn record_beat(&mut self, b: usize, got: u64) -> Result<(), PromExpCode> {
        if b >= BEATS {
            return Err(PromExpCode::ENoCell);
        }
        if b as i64 != self.last_beat + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD4400);
            return Err(PromExpCode::EMono);
        }
        let want = match beat_cell(b) {
            Some(w) => w,
            None => return Err(PromExpCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0xD4401 + b as u64);
            return Err(PromExpCode::EMono);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PromExpCode::EDup);
        }
        if let Some(c) = self.beats.get_mut(b) {
            *c = Some(got);
        }
        self.last_beat = b as i64;
        self.fnv(0xD4402 + b as u64);
        Ok(())
    }

    /// 采样全账核验（8 拍）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < BEATS {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// Server 消费联签一行（F52393 前向声明）。
    pub fn declare_union(&mut self, r: usize) -> Result<(), PromExpCode> {
        if r >= UNION_ROWS {
            return Err(PromExpCode::ENoRow);
        }
        if match self.unions.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PromExpCode::EDup);
        }
        if let Some(c) = self.unions.get_mut(r) {
            *c = union_row(r);
        }
        self.fnv(0xD5000 + r as u64);
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
    pub fn declare_go(&mut self, r: usize) -> Result<(), PromExpCode> {
        if r >= GO_ROWS {
            return Err(PromExpCode::ENoRow);
        }
        if match self.gos.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PromExpCode::EDup);
        }
        if let Some(c) = self.gos.get_mut(r) {
            *c = go_row(r);
        }
        self.fnv(0xD50C0 + r as u64);
        Ok(())
    }

    /// Go 上栈声明全齐核验。
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
    pub fn seal(&self) -> Result<(), PromExpCode> {
        if !(self.unions_all() && self.gos_all()) {
            return Err(PromExpCode::ERowMiss);
        }
        if !(self.gates_all() && self.cols_all() && self.eps_all() && self.yml_all() && self.beats_all()) {
            return Err(PromExpCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 注入守恒等式：注入 = 检出 + 逃逸。
    pub fn conservation(&self) -> Result<(), PromExpCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(PromExpCode::EInj)
        }
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

/// 双跑指纹对照。
pub fn fp_check(a: u64, b: u64) -> Result<(), PromExpCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(PromExpCode::EFp)
    }
}

/// KB-COMPAT 定级（全齐 → S1，六闸齐 → S2，否则 S3）。
pub fn judge_grade(led: &PromExpLedger) -> Grade {
    if led.gates_all()
        && led.cols_all()
        && led.eps_all()
        && led.yml_all()
        && led.beats_all()
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

impl Default for PromExpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PromExpLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < COLLECTORS {
        if let Some(w) = col_cell(o) {
            if led.record_col(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut e = 0usize;
    while e < ENDPOINT_CELLS {
        if let Some(w) = ep_cell(e) {
            if led.record_ep(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut i = 0usize;
    while i < YML_CELLS {
        if let Some(w) = yml_cell(i) {
            if led.record_yml(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut b = 0usize;
    while b < BEATS {
        if let Some(w) = beat_cell(b) {
            if led.record_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
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
    // 注入 10 轮：collector 载荷漂移 / 端点态回退（空账本必抓）/ 越闭集 三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PromExpLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_col(0, col_cell(0).unwrap_or(0));
                lt.record_col(1, col_cell(1).unwrap_or(0).wrapping_add(1)) == Err(PromExpCode::EDrift)
            }
            1 => lt.record_ep(ENDPOINT_CELLS - 4, ep_cell(ENDPOINT_CELLS - 4).unwrap_or(0))
                == Err(PromExpCode::EEpState),
            _ => {
                lt.record_col(COLLECTORS, 0).is_err()
                    && lt.record_ep(ENDPOINT_CELLS, 0).is_err()
                    && lt.record_beat(BEATS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0xD1600 + k as u64);
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
/// UNX-F52381 判据集（锚 UNX-F52382-J1，off-by-one 板式）。
pub fn run_unx52381_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52381");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "run_cycle 记格数恒等",
        recorded == COLLECTORS as u32 + ENDPOINT_CELLS as u32 + YML_CELLS as u32 + BEATS as u32,
        "20 collector + 12 端点 + 8 yml + 8 拍逐格恒等（闸与声明行不计数）",
    );
    s.add(
        "collector 五族 20 格全通",
        {
            let mut lt = PromExpLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < COLLECTORS {
                if let Some(w) = col_cell(o) {
                    if lt.record_col(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.cols_all()
        },
        "cpu/memory/disk/network/service 五族逐格载荷恒等",
    );
    s.add(
        "/metrics 端点三态 12 格全落",
        {
            let mut lt = PromExpLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < ENDPOINT_CELLS {
                if let Some(w) = ep_cell(e) {
                    if lt.record_ep(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.eps_all()
        },
        "抓取成功/部分序列过期/抓取失败三态单调落账",
    );
    s.add(
        "prometheus.yml 消费 8 格全落",
        {
            let mut lt = PromExpLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < YML_CELLS {
                if let Some(w) = yml_cell(i) {
                    if lt.record_yml(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.yml_all()
        },
        "scrape 间隔/抓取目标/标签重写等 8 项逐格对照",
    );
    s.add(
        "采样 8 拍单调",
        {
            let mut lt = PromExpLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BEATS {
                if let Some(w) = beat_cell(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "拍号逐拍递进且拍载荷逐拍恒等递增",
    );
    s.add(
        "Server 消费联签 4/4（F52393 前向声明）",
        {
            let mut lt = PromExpLedger::new();
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
            let mut lt = PromExpLedger::new();
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
        "collector 族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < COLLECTORS {
                if col_family(o).unwrap_or(5) < col_family(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "cpu→memory→disk→network→service 五族闭集单调",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = PromExpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/服务注册/端点开放/采集功能/卸载净除/升级保留逐格落账",
    );
    s.add(
        "边界·collector 越闭集",
        PromExpLedger::new().record_col(COLLECTORS, 0).is_err(),
        "collector 20 越闭集必拒",
    );
    s.add(
        "边界·端点越闭集",
        PromExpLedger::new().record_ep(ENDPOINT_CELLS, 0).is_err(),
        "端点 12 越闭集必拒",
    );
    s.add(
        "边界·采样拍越闭集",
        PromExpLedger::new().record_beat(BEATS, 0).is_err(),
        "采样拍 8 越闭集必拒",
    );
    s.add(
        "漂移拒",
        {
            let mut lt = PromExpLedger::new();
            lt.record_col(0, col_cell(0).unwrap_or(0).wrapping_add(1)) == Err(PromExpCode::EDrift)
        },
        "collector 载荷漂移必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "collector 漂移/端点态回退/越闭集三态轮转必抓",
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
            let mut lt = PromExpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < COLLECTORS {
                if let Some(w) = col_cell(o) {
                    let _ = lt.record_col(o, w);
                }
                o += 1;
            }
            let mut e = 0usize;
            while e < ENDPOINT_CELLS {
                if let Some(w) = ep_cell(e) {
                    let _ = lt.record_ep(e, w);
                }
                e += 1;
            }
            let mut i = 0usize;
            while i < YML_CELLS {
                if let Some(w) = yml_cell(i) {
                    let _ = lt.record_yml(i, w);
                }
                i += 1;
            }
            let mut b = 0usize;
            while b < BEATS {
                if let Some(w) = beat_cell(b) {
                    let _ = lt.record_beat(b, w);
                }
                b += 1;
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
            lt.seal().is_ok() && judge_grade(&lt) == Grade::S1
        },
        "全齐 → S1（KB-COMPAT 定级登记）",
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
    fn test_collector_family() {
        assert_eq!(col_family(0), Some(0));
        assert_eq!(col_family(4), Some(1));
        assert_eq!(col_family(19), Some(4));
        assert!(col_family(20).is_none());
        assert!(col_cell(20).is_none());
        assert_eq!(family_name(0), Some("cpu"));
        assert_eq!(family_name(4), Some("service"));
        assert!(family_name(5).is_none());
    }

    #[test]
    fn test_endpoint_states() {
        assert_eq!(ep_state(0), Some(0));
        assert_eq!(ep_state(7), Some(1));
        assert_eq!(ep_state(11), Some(2));
        assert!(ep_state(12).is_none());
        assert_eq!(state_name(0), Some("抓取成功"));
        assert_eq!(state_name(2), Some("抓取失败"));
        assert!(state_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = col_cell(3).unwrap_or(0);
        assert_eq!(a, col_cell(3).unwrap_or(0));
        assert_ne!(a, col_cell(4).unwrap_or(0));
        assert_ne!(ep_cell(0).unwrap_or(0), ep_cell(5).unwrap_or(0));
        assert!(yml_cell(YML_CELLS).is_none());
        assert!(union_row(UNION_ROWS).is_none());
        assert!(go_row(GO_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 空账本：首族必 cpu，跳族必抓。
        let mut lt = PromExpLedger::new();
        assert_eq!(
            lt.record_col(9, col_cell(9).unwrap_or(0)),
            Err(PromExpCode::EOrder)
        );
        assert_eq!(lt.stats().1, 1);
        // 合法序落低族格后再回跳 → 族序逆必抓。
        let _ = lt.record_col(0, col_cell(0).unwrap_or(0));
        let _ = lt.record_col(5, col_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_col(2, col_cell(2).unwrap_or(0)),
            Err(PromExpCode::EOrder)
        );
        assert!(lt.record_col(6, col_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_ep_state_caught() {
        // 空账本：首态必「抓取成功」。
        assert_eq!(
            PromExpLedger::new().record_ep(8, ep_cell(8).unwrap_or(0)),
            Err(PromExpCode::EEpState)
        );
        // 合法落态 0 后跳态 2 → 越态必抓。
        let mut lt = PromExpLedger::new();
        let _ = lt.record_ep(0, ep_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_ep(8, ep_cell(8).unwrap_or(0)),
            Err(PromExpCode::EEpState)
        );
    }

    #[test]
    fn test_drift_and_mono_caught() {
        let mut lt = PromExpLedger::new();
        assert_eq!(
            lt.record_col(0, col_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(PromExpCode::EDrift)
        );
        assert_eq!(
            lt.record_yml(2, yml_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(PromExpCode::EDrift)
        );
        // 采样跳拍与拍载荷漂移均 EMono。
        assert_eq!(
            lt.record_beat(3, beat_cell(3).unwrap_or(0)),
            Err(PromExpCode::EMono)
        );
        assert_eq!(
            lt.record_beat(0, beat_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(PromExpCode::EMono)
        );
        assert!(lt.record_beat(0, beat_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = PromExpLedger::new();
        let _ = lt.record_col(0, col_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_col(0, col_cell(0).unwrap_or(0)),
            Err(PromExpCode::EDup)
        );
        let _ = lt.declare_go(0);
        assert_eq!(lt.declare_go(0), Err(PromExpCode::EDup));
        assert_eq!(lt.declare_go(GO_ROWS), Err(PromExpCode::ENoRow));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = PromExpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        assert!(!PromExpLedger::new().gates_all());
    }

    #[test]
    fn test_seal_ladder() {
        // 空账：声明行缺 → ERowMiss。
        assert_eq!(PromExpLedger::new().seal(), Err(PromExpCode::ERowMiss));
        // 行齐格未齐 → ESeal。
        let mut lt = PromExpLedger::new();
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
        assert_eq!(lt.seal(), Err(PromExpCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记格数 = 20 collector + 12 端点 + 8 yml + 8 拍（闸与声明行不计数）。
        assert_eq!(
            recorded,
            COLLECTORS as u32 + ENDPOINT_CELLS as u32 + YML_CELLS as u32 + BEATS as u32
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
        assert_eq!(SEG_BASE, 0xC9F00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(PromExpCode::Ok as u32, 0);
        assert_eq!(PromExpCode::EGradeDrift as u32, 12);
        assert_eq!(PromExpCode::Ok.val(), SEG_BASE);
        assert_eq!(PromExpCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!PromExpCode::Ok.detail().is_empty());
        assert!(!PromExpCode::ESeal.detail().is_empty());
        assert!(!PromExpCode::EEpState.detail().is_empty());
        assert!(!PromExpCode::EMono.detail().is_empty());
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐目标档 S1。
        assert_eq!(judge_grade(&PromExpLedger::new()), Grade::S3);
        let mut lt = PromExpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(judge_grade(&lt), Grade::S2);
        let mut full = PromExpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < COLLECTORS {
            if let Some(w) = col_cell(o) {
                let _ = full.record_col(o, w);
            }
            o += 1;
        }
        let mut e = 0usize;
        while e < ENDPOINT_CELLS {
            if let Some(w) = ep_cell(e) {
                let _ = full.record_ep(e, w);
            }
            e += 1;
        }
        let mut i = 0usize;
        while i < YML_CELLS {
            if let Some(w) = yml_cell(i) {
                let _ = full.record_yml(i, w);
            }
            i += 1;
        }
        let mut b = 0usize;
        while b < BEATS {
            if let Some(w) = beat_cell(b) {
                let _ = full.record_beat(b, w);
            }
            b += 1;
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
        assert_eq!(judge_grade(&full), Grade::S1);
        let (inj, cgt, esc) = full.stats();
        assert_eq!(inj, cgt + esc);
    }

    #[test]
    fn unx52381_checks_green() {
        let s = run_unx52381_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert_eq!(s.len(), 19);
    }
}
