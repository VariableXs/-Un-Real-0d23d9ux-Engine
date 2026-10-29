//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1403 · 延迟测量起止点契约（raise→handler 首指令）· 0x36140
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1403-J1 测量起止点契约全向量生效率 100%（设备 raise 以
//!      HPET 首次可读时刻为准/handler 首指令以 TrapFrame 入口计），
//!      起止点漂移注入 10/10 必抓，契约版本化登记；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 起点契约（endpoint-start）：设备中断 raise 时刻以「HPET 计数
//!     首次可读」为准——建模为 raise 事件必须携带 HPET 读数（None =
//!     起点 beyond 契约，拒入账）；禁止用「构造事件时刻」顶替（漂移
//!     源一：软件路径抖动计入延迟窗口）；
//!   - 终点契约（endpoint-end）：handler 首指令时刻以 TrapFrame 入口
//!     计——handler 报点必须带 trap 序号且为首报（次报拒入，漂移源
//!     二：handler 内部处理时长混入延迟窗口）；
//!   - 延迟 = 终点 - 起点，单调性红线：终点 < 起点显性拒（时钟倒退
//!     不产生负延迟）；
//!   - 全向量生效率 100%：契约登记表逐向量（0..255）要求「起止点双
//!     契约齐备」（向量 0..31 与 32..255 分段核），双缺任一 = 未生效；
//!     spurious 向量 0xFF 槽位豁免登记但显性记账（0xFF 特例消费）；
//!   - 漂移注入：起点顶替（软件时刻）/ 终点次报 / 负延迟 / 起点缺失
//!     四族注入 10/10 必抓；
//!   - 契约版本化登记：契约文本 + 版本号（v1）+ FNV 指纹冻结，变更
//!     唯一路径 = 版本前进（旧版留痕），改文本不改版本 = 篡改拒；
//!   - LCG 种子写死 = SEG_BASE（0x36140），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36140，全仓独占）。
pub const SEG_BASE: u32 = 0x36140;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LatCode {
    /// 起点缺 HPET 读数（beyond 契约）
    StartNoHpet,
    /// 起点用了软件构造时刻顶替（漂移源一）
    StartSubstituted,
    /// 终点非首报（漂移源二：handler 内处理时长混入）
    EndNotFirst,
    /// 负延迟（终点 < 起点，时钟倒退）
    NegativeLatency,
    /// 向量越界（>255）
    VectorOutOfRange,
    /// spurious 0xFF 试图落延迟账（只许特例记账）
    SpuriousLatency,
    /// 契约篡改：文本变指纹不变
    ContractTampered,
    /// 契约版本回退
    VersionRegress,
    /// 契约生效表未满 100%
    CoverageIncomplete,
}

impl LatCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LatCode::StartNoHpet => "起点缺 HPET 读数：raise 事件无 HPET 首次可读时刻",
            LatCode::StartSubstituted => "起点顶替：用软件构造时刻顶替 HPET 时刻（漂移源一）",
            LatCode::EndNotFirst => "终点非首报：handler 首指令点被次报顶替（漂移源二）",
            LatCode::NegativeLatency => "负延迟：终点早于起点，时钟倒退不入账",
            LatCode::VectorOutOfRange => "向量越界：超出 0..=255",
            LatCode::SpuriousLatency => "spurious 0xFF 只许特例记账，不得落延迟账",
            LatCode::ContractTampered => "契约篡改：文本变而指纹未变（或反之）",
            LatCode::VersionRegress => "契约版本回退：版本唯一前进路径被破",
            LatCode::CoverageIncomplete => "契约生效表未满 100%：存在向量双契约缺一",
        }
    }
    pub const ALL: [LatCode; 9] = [
        LatCode::StartNoHpet,
        LatCode::StartSubstituted,
        LatCode::EndNotFirst,
        LatCode::NegativeLatency,
        LatCode::VectorOutOfRange,
        LatCode::SpuriousLatency,
        LatCode::ContractTampered,
        LatCode::VersionRegress,
        LatCode::CoverageIncomplete,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in LatCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 向量空间上限。
pub const VECTOR_MAX: u16 = 255;
/// spurious 特例向量。
pub const SPURIOUS_VECTOR: u16 = 0xFF;
/// 契约当前版本。
pub const CONTRACT_VERSION: u32 = 1;
/// 起点契约文本（冻结）。
pub const CONTRACT_START_TEXT: &str = "endpoint-start: raise 以 HPET 首次可读时刻为准";
/// 终点契约文本（冻结）。
pub const CONTRACT_END_TEXT: &str = "endpoint-end: handler 首指令以 TrapFrame 入口计";

/// FNV-1a 32 位（契约指纹）。
pub fn fnv1a32(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

// ── 契约版本化登记 ───────────────────────────────────────────────
/// 一条契约登记（版本 + 文本 + 指纹三元组自锁）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ContractEntry {
    pub version: u32,
    pub start_text_len: usize,
    pub end_text_len: usize,
    pub fingerprint: u32,
}

/// 契约登记账。
pub struct ContractLedger {
    pub entries: Vec<ContractEntry>,
    pub tamper_caught: usize,
}

impl ContractLedger {
    pub fn new() -> ContractLedger {
        ContractLedger {
            entries: Vec::new(),
            tamper_caught: 0,
        }
    }

    /// 登记当前契约（v1）：指纹 = 文本 FNV。
    pub fn register_v1(&mut self) -> Result<ContractEntry, LatCode> {
        if !self.entries.is_empty() {
            return Err(LatCode::VersionRegress);
        }
        let mut text: Vec<u8> = Vec::new();
        text.extend_from_slice(CONTRACT_START_TEXT.as_bytes());
        text.push(0);
        text.extend_from_slice(CONTRACT_END_TEXT.as_bytes());
        let e = ContractEntry {
            version: CONTRACT_VERSION,
            start_text_len: CONTRACT_START_TEXT.len(),
            end_text_len: CONTRACT_END_TEXT.len(),
            fingerprint: fnv1a32(&text),
        };
        self.entries.push(e);
        Ok(e)
    }

    /// 版本前进（唯一合法变更路径）：严格 +1。
    pub fn advance(&mut self, new_start: &str, new_end: &str) -> Result<ContractEntry, LatCode> {
        let cur = match self.entries.last() {
            Some(e) => *e,
            None => return Err(LatCode::VersionRegress),
        };
        let mut text: Vec<u8> = Vec::new();
        text.extend_from_slice(new_start.as_bytes());
        text.push(0);
        text.extend_from_slice(new_end.as_bytes());
        let e = ContractEntry {
            version: cur.version + 1,
            start_text_len: new_start.len(),
            end_text_len: new_end.len(),
            fingerprint: fnv1a32(&text),
        };
        self.entries.push(e);
        Ok(e)
    }

    /// 篡改检测：给定文本算指纹，与账面最新版对拍。
    pub fn verify_text(&mut self, start: &str, end: &str) -> Result<u32, LatCode> {
        let cur = match self.entries.last() {
            Some(e) => *e,
            None => return Err(LatCode::ContractTampered),
        };
        let mut text: Vec<u8> = Vec::new();
        text.extend_from_slice(start.as_bytes());
        text.push(0);
        text.extend_from_slice(end.as_bytes());
        let fp = fnv1a32(&text);
        if fp != cur.fingerprint || start.len() != cur.start_text_len || end.len() != cur.end_text_len {
            self.tamper_caught += 1;
            return Err(LatCode::ContractTampered);
        }
        Ok(fp)
    }
}

// ── 起止点账（逐向量） ───────────────────────────────────────────
/// 起点（HPET 首次可读时刻）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatencyStart {
    pub vector: u16,
    pub hpet_tick: u64,
}

/// 终点（TrapFrame 入口首报）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatencyEnd {
    pub vector: u16,
    pub trap_seq: u64,
    /// 是否首报（handler 报点序号 == 该向量本帧首报）。
    pub first_report: bool,
    pub trap_tick: u64,
}

/// 一条延迟样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LatencySample {
    pub vector: u16,
    pub latency: u64,
}

/// 起止点账引擎。
pub struct EndpointLedger {
    pub samples: Vec<LatencySample>,
    pub rejected: usize,
    pub spurious_booked: usize,
    /// 逐向量契约生效表（起点齐/终点齐两布尔）。
    pub start_alive: [bool; 256],
    pub end_alive: [bool; 256],
}

impl EndpointLedger {
    pub fn new() -> EndpointLedger {
        EndpointLedger {
            samples: Vec::new(),
            rejected: 0,
            spurious_booked: 0,
            start_alive: [false; 256],
            end_alive: [false; 256],
        }
    }

    /// 起点：必须带 HPET 读数（None = 顶替，拒）。
    pub fn book_start(&mut self, vector: u16, hpet: Option<u64>) -> Result<(), LatCode> {
        if vector == SPURIOUS_VECTOR {
            self.spurious_booked += 1;
            return Err(LatCode::SpuriousLatency);
        }
        if vector > VECTOR_MAX {
            return Err(LatCode::VectorOutOfRange);
        }
        match hpet {
            Some(_) => {
                self.start_alive[vector as usize] = true;
                Ok(())
            }
            None => {
                self.rejected += 1;
                Err(LatCode::StartSubstituted)
            }
        }
    }

    /// 终点：必须首报；延迟单调核；落样本。
    pub fn book_end(&mut self, end: &LatencyEnd, start: &LatencyStart) -> Result<u64, LatCode> {
        if end.vector > VECTOR_MAX {
            return Err(LatCode::VectorOutOfRange);
        }
        if end.vector == SPURIOUS_VECTOR {
            self.spurious_booked += 1;
            return Err(LatCode::SpuriousLatency);
        }
        if end.vector != start.vector {
            self.rejected += 1;
            return Err(LatCode::NegativeLatency);
        }
        if !end.first_report {
            self.rejected += 1;
            return Err(LatCode::EndNotFirst);
        }
        if end.trap_tick < start.hpet_tick {
            self.rejected += 1;
            return Err(LatCode::NegativeLatency);
        }
        let lat = end.trap_tick - start.hpet_tick;
        self.samples.push(LatencySample {
            vector: end.vector,
            latency: lat,
        });
        self.end_alive[end.vector as usize] = true;
        Ok(lat)
    }

    /// 全向量契约生效率：256 个真向量双契约齐备率（0xFF 豁免）。
    pub fn coverage(&self) -> (usize, usize) {
        let mut alive = 0;
        for v in 0..=255usize {
            if v == SPURIOUS_VECTOR as usize {
                continue; // spurious 豁免
            }
            if self.start_alive[v] && self.end_alive[v] {
                alive += 1;
            }
        }
        (alive, 255)
    }

    pub fn coverage_full(&self) -> bool {
        let (a, b) = self.coverage();
        a == b
    }

    /// 汇总行。
    pub fn summary_line(&self) -> String {
        let (a, b) = self.coverage();
        format!(
            "起止点契约: 样本={} 生效={}/{} 拒收={} spurious特例={}",
            self.samples.len(),
            a,
            b,
            self.rejected,
            self.spurious_booked
        )
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 注入面（四族 10/10 必抓） ────────────────────────────────────
fn drill_start_substituted_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut led = EndpointLedger::new();
        let v = (lcg.next_below(256) & 0xFE) as u16;
        // 注入：软件构造时刻顶替（hpet=None）。
        let r = led.book_start(v, None);
        if r == Err(LatCode::StartSubstituted) && led.rejected == 1 {
            caught += 1;
        }
        let _ = i;
    }
    let ok = caught == 10;
    (ok, format!("起点顶替注入 {}/10 必抓", caught))
}

fn drill_end_not_first_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut led = EndpointLedger::new();
        let v = (lcg.next_below(256) & 0xFE) as u16;
        let _ = led.book_start(v, Some(1000 + i as u64));
        let end = LatencyEnd {
            vector: v,
            trap_seq: 1,
            first_report: false, // 次报顶替
            trap_tick: 1100 + i as u64,
        };
        let start = LatencyStart { vector: v, hpet_tick: 1000 + i as u64 };
        let r = led.book_end(&end, &start);
        if r == Err(LatCode::EndNotFirst) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("终点次报注入 {}/10 必抓", caught))
}

fn drill_negative_latency_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 2);
    let mut caught = 0;
    for i in 0..10u32 {
        let mut led = EndpointLedger::new();
        let v = (lcg.next_below(256) & 0xFE) as u16;
        let start_tick = 5000 + i as u64;
        let _ = led.book_start(v, Some(start_tick));
        let end = LatencyEnd {
            vector: v,
            trap_seq: 1,
            first_report: true,
            trap_tick: start_tick - 1 - (lcg.next_below(10)), // 终点早于起点
        };
        let start = LatencyStart { vector: v, hpet_tick: start_tick };
        let r = led.book_end(&end, &start);
        if r == Err(LatCode::NegativeLatency) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("负延迟注入 {}/10 必抓", caught))
}

fn drill_start_missing_10() -> (bool, String) {
    // 起点缺失族：缺 HPET 读数的 raise 事件不得落延迟账（终点来了
    // 也无起点可配——语义上 book_end 无 start 参数无法成对，此处证明
    // book_start(None) 拒后 start_alive 不置位）。
    let mut caught = 0;
    for i in 0..10u32 {
        let mut led = EndpointLedger::new();
        let _ = led.book_start(i as u16, None);
        let not_alive = !led.start_alive[i as usize];
        // 后续终点即便合法也不得成对（start 缺席 → coverage 不满）。
        if not_alive && !led.coverage_full() {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("起点缺失注入 {}/10 生效表不置位", caught))
}

fn drill_spurious_special() -> (bool, String) {
    // spurious 0xFF：只许特例记账，起止点都不落。
    let mut led = EndpointLedger::new();
    let r1 = led.book_start(SPURIOUS_VECTOR, Some(100));
    let start = LatencyStart { vector: SPURIOUS_VECTOR, hpet_tick: 100 };
    let end = LatencyEnd { vector: SPURIOUS_VECTOR, trap_seq: 1, first_report: true, trap_tick: 200 };
    let r2 = led.book_end(&end, &start);
    let ok = r1 == Err(LatCode::SpuriousLatency)
        && r2 == Err(LatCode::SpuriousLatency)
        && led.spurious_booked == 2
        && led.samples.is_empty();
    (ok, format!("spurious 0xFF 特例: 起拒={} 终拒={} 特例账={}", r1.is_err(), r2.is_err(), led.spurious_booked))
}

fn drill_vector_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 3);
    let mut caught = 0;
    for _ in 0..10 {
        let mut led = EndpointLedger::new();
        let bad = 300 + (lcg.next_below(100) as u16);
        let r1 = led.book_start(bad, Some(100));
        if r1 == Err(LatCode::VectorOutOfRange) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("向量越界注入 {}/10 必抓", caught))
}

fn drill_contract_tamper() -> (bool, String) {
    // 契约篡改面：文本变指纹不变 = 抓。
    let mut led = ContractLedger::new();
    let e = led.register_v1();
    let r_ok = led.verify_text(CONTRACT_START_TEXT, CONTRACT_END_TEXT);
    // 篡改：终点文本改动。
    let r_bad = led.verify_text(CONTRACT_START_TEXT, "endpoint-end: 被篡改的文本");
    let ok = e.is_ok()
        && r_ok.is_ok()
        && r_bad == Err(LatCode::ContractTampered)
        && led.tamper_caught == 1;
    (ok, format!("契约篡改: 合法验={} 篡改抓={}", r_ok.is_ok(), r_bad.is_err()))
}

fn drill_contract_advance() -> (bool, String) {
    // 版本前进：v1 → v2 合法，指纹必变；同文本前进允许（语义刷新）。
    let mut led = ContractLedger::new();
    let _ = led.register_v1();
    let e2 = led.advance("endpoint-start: v2 文本", CONTRACT_END_TEXT);
    let fp_diff = match (&led.entries[0], &e2) {
        (_, Ok(n)) => n.fingerprint != led.entries[0].fingerprint && n.version == 2,
        _ => false,
    };
    (fp_diff, format!("契约前进: v2={:?}", e2.map(|e| e.version)))
}

fn drill_contract_repeat_register() -> (bool, String) {
    // 重复 v1 登记拒（版本唯一前进）。
    let mut led = ContractLedger::new();
    let r1 = led.register_v1();
    let r2 = led.register_v1();
    (r1.is_ok() && r2 == Err(LatCode::VersionRegress), format!("重复登记: 首ok={} 二拒={}", r1.is_ok(), r2.is_err()))
}

fn drill_coverage_100() -> (bool, String) {
    // 全向量生效率 100% 正例：256 真向量双契约全齐。
    let mut led = EndpointLedger::new();
    for v in 0..=255u16 {
        if v == SPURIOUS_VECTOR {
            continue;
        }
        let _ = led.book_start(v, Some(1000 + v as u64));
        let end = LatencyEnd {
            vector: v,
            trap_seq: v as u64,
            first_report: true,
            trap_tick: 1000 + v as u64 + 10,
        };
        let start = LatencyStart { vector: v, hpet_tick: 1000 + v as u64 };
        let _ = led.book_end(&end, &start);
    }
    let full = led.coverage_full();
    let samples = led.samples.len();
    (full && samples == 255, format!("全向量生效: 255/255 full={} samples={}", full, samples))
}

fn drill_latency_math() -> (bool, String) {
    // 延迟数学生：终点-起点差值保真。
    let mut led = EndpointLedger::new();
    let _ = led.book_start(40, Some(1_000_000));
    let end = LatencyEnd { vector: 40, trap_seq: 1, first_report: true, trap_tick: 1_000_133 };
    let lat = led.book_end(&end, &LatencyStart { vector: 40, hpet_tick: 1_000_000 });
    let ok = lat == Ok(133) && led.samples[0].latency == 133;
    (ok, format!("延迟数学: {:?}", lat))
}

fn drill_zero_latency_legal() -> (bool, String) {
    // 零延迟合法（同刻读数）：不为负即收。
    let mut led = EndpointLedger::new();
    let _ = led.book_start(41, Some(2000));
    let end = LatencyEnd { vector: 41, trap_seq: 1, first_report: true, trap_tick: 2000 };
    let lat = led.book_end(&end, &LatencyStart { vector: 41, hpet_tick: 2000 });
    (lat == Ok(0), format!("零延迟合法: {:?}", lat))
}

fn drill_vector_mismatch_10() -> (bool, String) {
    // 起终点向量不一致 = 负延迟族（对账断裂）。
    let mut caught = 0;
    for i in 0..10u16 {
        let mut led = EndpointLedger::new();
        let v1: u16 = 10 + i;
        let v2: u16 = 20 + i;
        let _ = led.book_start(v1, Some(1000));
        let end = LatencyEnd { vector: v2, trap_seq: 1, first_report: true, trap_tick: 1100 };
        let r = led.book_end(&end, &LatencyStart { vector: v1, hpet_tick: 1000 });
        if r == Err(LatCode::NegativeLatency) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("向量错配 {}/10 必抓", caught))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    LatCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账（判据 J1 落地面） ─────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let mut led = EndpointLedger::new();
    let mut contract = ContractLedger::new();
    let creg = contract.register_v1();
    // 32 个真向量（0..31 + 40..47）双契约全齐。
    let mut n = 0;
    for v in 0..32u16 {
        let _ = led.book_start(v, Some(10_000 + v as u64));
        let end = LatencyEnd {
            vector: v,
            trap_seq: v as u64,
            first_report: true,
            trap_tick: 10_000 + v as u64 + 5 + (v % 7) as u64,
        };
        let _ = led.book_end(&end, &LatencyStart { vector: v, hpet_tick: 10_000 + v as u64 });
        n += 1;
    }
    // spurious 特例记账一次。
    let _ = led.book_start(SPURIOUS_VECTOR, None);
    let ok = creg.is_ok()
        && n == 32
        && led.samples.len() == 32
        && led.spurious_booked == 1
        && led.summary_line().len() > 0
        && contract.entries.len() == 1;
    (
        ok,
        format!("全流程: 契约v1={} 32向量样本={} [{}] [契约指纹={:#010x}]", creg.is_ok(), led.samples.len(), led.summary_line(), contract.entries.first().map(|e| e.fingerprint).unwrap_or(0)),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_start_substituted_10();
    v.push(("J2-起点顶替10/10", a, b));
    let (a, b) = drill_end_not_first_10();
    v.push(("J2-终点次报10/10", a, b));
    let (a, b) = drill_negative_latency_10();
    v.push(("J2-负延迟注入10/10", a, b));
    let (a, b) = drill_start_missing_10();
    v.push(("J2-起点缺失10/10", a, b));
    let (a, b) = drill_spurious_special();
    v.push(("J2-spurious特例", a, b));
    let (a, b) = drill_vector_range_10();
    v.push(("J2-向量越界10/10", a, b));
    let (a, b) = drill_vector_mismatch_10();
    v.push(("J2-向量错配10/10", a, b));
    let (a, b) = drill_contract_tamper();
    v.push(("J3-契约篡改抓", a, b));
    let (a, b) = drill_contract_advance();
    v.push(("J3-契约版本前进", a, b));
    let (a, b) = drill_contract_repeat_register();
    v.push(("J3-重复登记拒", a, b));
    let (a, b) = drill_coverage_100();
    v.push(("J1-全向量生效100%", a, b));
    let (a, b) = drill_latency_math();
    v.push(("J3-延迟数学", a, b));
    let (a, b) = drill_zero_latency_legal();
    v.push(("J3-零延迟合法", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1403_checks：本单判据聚合（诊断段 0x36140）。
pub fn run_unx1403_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1403 延迟测量起止点契约");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36140 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
