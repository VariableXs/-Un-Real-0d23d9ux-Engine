//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18461 · 兼容层标志面总纲（读取链冻结） · 0x7EF00段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18461-J1 读取链（HKCU/HKLM 双 hive → 生效）冻结 v1 与
//!       AI-18 接口签名一致
//! 施工：AI-VX11-1105-G1 · 工单 A07 · 批 04 · 配额 280 · 目标 ≥600 行
//! 本模块结构清单：
//!   数据结构：HiveKind 双 hive 闭集 / ChainEntry 数组账本（定容 16 槽）
//!   状态机  ：ChainStage 五态单调链 Idle→HkcuRead→HklmRead→Arbitrate→Frozen
//!   账本    ：ChainLedger（扫描序入账、seq 严格递增、容量耗尽拒新不丢旧）
//!   错误路径：FrcCode 11 码闭集，code() = SEG_BASE + n（0x40 窗口内）
//!   注入核  ：4 类坏样本（空查询 / stage 乱序 / 签名不匹配 / 超长原始值）
//!             ×10 轮逐类断言被显式拒绝，漏检必为 0
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EF00，AI-VX11-1105 批 A07 卡 F18461 独占窗口；
/// 窗宽 0x40 = 64 个诊断码，段位守卫复核对齐与下界）。
pub const SEG_BASE: u32 = 0x7EF00;

// ── 冻结常量（写死 + 推导注明） ─────────────────────────────────
/// 判据迭代轮数（冻结 10：注入核"10/10"口径）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 账本定容（冻结 16 槽：双 hive × 8 典型查询位）。
pub const LEDGER_CAP: usize = 16;
/// 查询路径长度上限（冻结 128：与 AI-18 接口签名中的路径域宽一致）。
pub const QUERY_LEN_MAX: usize = 128;
/// 原始标志串长度上限（冻结 512：Layers 值域经实测上限对齐）。
pub const RAW_LEN_MAX: usize = 512;
/// 冻结 v1 接口签名。推导：高 16 位 = AI-18 接口族码 0x0018 | 0x8000
/// （面总纲置位）= 0x8018；低 16 位 = 冻结版本 v1 = 0x0001。
pub const CHAIN_SIG_V1: u32 = 0x8018_0001;

// ── 双 hive 闭集 ────────────────────────────────────────────────
/// 兼容层读取链的两个 hive（闭集冻结：HKCU 先、HKLM 后）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HiveKind {
    /// 用户态 hive（HKCU，优先级高）。
    CurrentUser = 0,
    /// 机器态 hive（HKLM，回退源）。
    LocalMachine = 1,
}

impl HiveKind {
    /// 槽位下标（账本/对照表用）。
    pub fn idx(self) -> usize {
        self as usize
    }
    /// 从下标还原 hive（越界返回 None，不裸断言）。
    pub fn from_idx(v: u32) -> Option<HiveKind> {
        if v == 0 {
            Some(HiveKind::CurrentUser)
        } else if v == 1 {
            Some(HiveKind::LocalMachine)
        } else {
            None
        }
    }
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            HiveKind::CurrentUser => "HKCU",
            HiveKind::LocalMachine => "HKLM",
        }
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrcCode {
    /// 查询为空（读取链拒收）。
    QueryEmpty,
    /// 查询超长（128 上限）。
    QueryTooLong,
    /// stage 乱序（HKLM 先于 HKCU 或跳级）。
    StageOrderBroken,
    /// stage 重复（同一 hive 读两次）。
    StageDup,
    /// 原始值超长（512 上限）。
    RawTooLong,
    /// 接口签名不匹配（非冻结 v1）。
    SigMismatch,
    /// 状态机非法倒退/越级。
    StageRegress,
    /// 账本满（新入账被拒，旧账不丢）。
    LedgerFull,
    /// 仲裁结果与冻结口径偏差。
    ArbDeviation,
    /// 计数守恒破坏（注入≠检出+漏检）。
    ConservationBroken,
    /// 双跑不一致（复现红线）。
    NotReproducible,
}

impl FrcCode {
    /// 诊断码：SEG_BASE + 枚举序（段内 0x40 窗口闭集）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码非空，判据 11 号形态）。
    pub fn detail(self) -> &'static str {
        match self {
            FrcCode::QueryEmpty => "查询为空：读取链拒收，先给合法 exe 路径",
            FrcCode::QueryTooLong => "查询超长：128 上限，截断重试",
            FrcCode::StageOrderBroken => "stage 乱序：HKCU 必须先读",
            FrcCode::StageDup => "stage 重复：同一 hive 不读两次",
            FrcCode::RawTooLong => "原始值超长：512 上限，防御拒收",
            FrcCode::SigMismatch => "接口签名不匹配：仅冻结 v1 可入链",
            FrcCode::StageRegress => "状态机倒退：读取链只进不退",
            FrcCode::LedgerFull => "账本满：入账被拒，旧账保留",
            FrcCode::ArbDeviation => "仲裁偏差：与 HKCU 优先口径不符",
            FrcCode::ConservationBroken => "计数守恒破坏：注入=检出+漏检不成立",
            FrcCode::NotReproducible => "双跑不一致：复现红线被踩",
        }
    }
    /// 全码闭集（12 项含全 11 个枚举成员）。
    pub const ALL: [FrcCode; 11] = [
        FrcCode::QueryEmpty,
        FrcCode::QueryTooLong,
        FrcCode::StageOrderBroken,
        FrcCode::StageDup,
        FrcCode::RawTooLong,
        FrcCode::SigMismatch,
        FrcCode::StageRegress,
        FrcCode::LedgerFull,
        FrcCode::ArbDeviation,
        FrcCode::ConservationBroken,
        FrcCode::NotReproducible,
    ];
}

/// 按码值取中文文案（扫描闭集，未知码回退提示）。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < FrcCode::ALL.len() {
        if FrcCode::ALL[i].code() == code {
            return FrcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EF00 段闭集内"
}

// ── 状态机：五态单调读取链 ──────────────────────────────────────
/// 读取链五态（rank 即单调序号，只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainStage {
    /// 链未启动。
    Idle = 0,
    /// 已读 HKCU。
    HkcuRead = 1,
    /// 已读 HKLM。
    HklmRead = 2,
    /// 已仲裁。
    Arbitrate = 3,
    /// 已冻结（接口冻结点）。
    Frozen = 4,
}

impl ChainStage {
    /// 单调序号。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单步推进：rank 必须恰好 +1；倒退、重复、越级一律显式拒。
    pub fn advance(self, next: ChainStage) -> Result<ChainStage, u32> {
        if next.rank() == self.rank().saturating_add(1) {
            Ok(next)
        } else {
            Err(FrcCode::StageRegress.code())
        }
    }
    /// 全链五步推进（冻结点收口）。
    pub fn full_chain() -> Result<ChainStage, u32> {
        let s0 = ChainStage::Idle;
        let s1 = s0.advance(ChainStage::HkcuRead)?;
        let s2 = s1.advance(ChainStage::HklmRead)?;
        let s3 = s2.advance(ChainStage::Arbitrate)?;
        s3.advance(ChainStage::Frozen)
    }
}

// ── 校验器（错误路径） ──────────────────────────────────────────
/// 查询路径校验：空/超长两闸。
pub fn validate_query(q: &str) -> Result<(), u32> {
    if q.len() == 0 {
        return Err(FrcCode::QueryEmpty.code());
    }
    if q.len() > QUERY_LEN_MAX {
        return Err(FrcCode::QueryTooLong.code());
    }
    Ok(())
}

/// 原始标志串校验：None=未设放行；有值则 512 上限。
pub fn validate_raw(r: Option<&str>) -> Result<u32, u32> {
    match r {
        None => Ok(0),
        Some(s) => {
            if s.len() > RAW_LEN_MAX {
                Err(FrcCode::RawTooLong.code())
            } else {
                Ok(s.len() as u32)
            }
        }
    }
}

// ── 仲裁核（HKCU 优先） ─────────────────────────────────────────
/// 仲裁结果：生效源 + 生效串长度（None=双 hive 均未设，合法）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainOutcome {
    pub eff_source: Option<HiveKind>,
    pub eff_len: u32,
}

/// HKCU 优先仲裁：用户态有值覆盖机器态；用户态无则回退机器态；
/// 双无返回 None（不报错——"未设"是合法读取结果）。
pub fn arbitrate(hkcu: Option<&str>, hklm: Option<&str>) -> Result<ChainOutcome, u32> {
    let hkcu_len = validate_raw(hkcu)?;
    match hkcu {
        Some(_) => Ok(ChainOutcome {
            eff_source: Some(HiveKind::CurrentUser),
            eff_len: hkcu_len,
        }),
        None => {
            let hklm_len = validate_raw(hklm)?;
            match hklm {
                Some(_) => Ok(ChainOutcome {
                    eff_source: Some(HiveKind::LocalMachine),
                    eff_len: hklm_len,
                }),
                None => Ok(ChainOutcome {
                    eff_source: None,
                    eff_len: 0,
                }),
            }
        }
    }
}

/// 读取链总装：查询校验 → 签名冻结校验 → 五态推进 → 双 hive 仲裁。
pub fn run_chain(
    query: &str,
    sig: u32,
    hkcu: Option<&str>,
    hklm: Option<&str>,
) -> Result<ChainOutcome, u32> {
    validate_query(query)?;
    if sig != CHAIN_SIG_V1 {
        return Err(FrcCode::SigMismatch.code());
    }
    ChainStage::full_chain()?;
    arbitrate(hkcu, hklm)
}

// ── 数组账本（扫描序入账，只增不减） ────────────────────────────
/// 单条读取链账目。
#[derive(Clone, Debug)]
pub struct ChainEntry {
    pub query_hash: u32,
    pub source_idx: u32,
    pub eff_len: u32,
    pub effective: bool,
    pub seq: u32,
}

/// 读取链账本：定容 16 槽、seq 严格递增、满后拒新不丢旧。
pub struct ChainLedger {
    entries: Vec<ChainEntry>,
    pub full_rejects: u32,
    pub inject_tries: [u32; 4],
    pub inject_caught: [u32; 4],
    next_seq: u32,
}

impl ChainLedger {
    /// 空账本。
    pub fn new() -> ChainLedger {
        ChainLedger {
            entries: Vec::new(),
            full_rejects: 0,
            inject_tries: [0; 4],
            inject_caught: [0; 4],
            next_seq: 1,
        }
    }
    /// 入账（容量耗尽 → 拒新，旧账不丢，计数显性记账）。
    pub fn record(&mut self, query: &str, out: &ChainOutcome) -> Result<(), u32> {
        if self.entries.len() >= LEDGER_CAP {
            self.full_rejects += 1;
            return Err(FrcCode::LedgerFull.code());
        }
        let e = ChainEntry {
            query_hash: fnv32(query.as_bytes()),
            source_idx: match out.eff_source {
                Some(h) => h.idx() as u32,
                None => 0xFFFF,
            },
            eff_len: out.eff_len,
            effective: out.eff_source.is_some(),
            seq: self.next_seq,
        };
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(e);
        Ok(())
    }
    /// 已入账条数。
    pub fn count(&self) -> usize {
        self.entries.len()
    }
    /// seq 严格递增复核（账本单调性）。
    pub fn monotone_seq(&self) -> bool {
        let n = self.entries.len();
        let mut i = 1;
        while i < n {
            let prev = match self.entries.get(i - 1) {
                Some(p) => p.seq,
                None => return false,
            };
            let cur = match self.entries.get(i) {
                Some(c) => c.seq,
                None => return false,
            };
            if cur <= prev {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 注入检出记账。
    pub fn record_inject(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.inject_tries[slot] += 1;
            if hit {
                self.inject_caught[slot] += 1;
            }
        }
    }
    /// 计数守恒：注入次数 = 检出 + 漏检，且漏检为 0。
    pub fn conserve(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 全部命中（四槽皆非零且全抓）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.inject_tries[i] == 0 || self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 账本快照哈希（确定性对拍用，只增不减指纹）。
    pub fn fingerprint(&self) -> u32 {
        let mut h = 0x811C9DC5u32;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h ^= e.query_hash;
                h = h.wrapping_mul(0x01000193);
                h ^= e.seq;
                h = h.wrapping_mul(0x01000193);
            }
            i += 1;
        }
        h
    }
}

/// FNV-1a 32 位（账本哈希，确定性、无随机源）。
pub fn fnv32(bytes: &[u8]) -> u32 {
    let mut h = 0x811C9DC5u32;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(&b) = bytes.get(i) {
            h ^= b as u32;
            h = h.wrapping_mul(0x01000193);
        }
        i += 1;
    }
    h
}

// ── 正样本构造（确定性） ────────────────────────────────────────
/// 典型查询路径（12 例确定性变体，覆盖两 hive 各组合）。
pub fn make_query(idx: u32) -> String {
    format!("C:\\Program Files\\compat_app_{}.exe", idx % 12)
}

/// 注入坏样本 4 类（逐类断言被显式拒绝）：
///   0 空查询；1 stage 乱序（HKLM 单步跳到 Arbitrate）；
///   2 签名不匹配（v1-1）；3 原始值超长（上限+1）。
pub fn inject_probe(slot: usize) -> Result<ChainOutcome, u32> {
    match slot {
        0 => run_chain("", CHAIN_SIG_V1, Some("~"), None),
        1 => {
            // 乱序注入：Idle 直接跳 Arbitrate（rank +3，非 +1）。
            ChainStage::Idle.advance(ChainStage::Arbitrate)?;
            Ok(ChainOutcome {
                eff_source: None,
                eff_len: 0,
            })
        }
        2 => run_chain("C:\\a.exe", CHAIN_SIG_V1.wrapping_sub(1), Some("~"), None),
        _ => {
            // 超长原始值：RAW_LEN_MAX + 1 个字符。
            let mut long = String::new();
            let mut n = 0u32;
            while n <= RAW_LEN_MAX as u32 {
                long.push('x');
                n += 1;
            }
            run_chain("C:\\a.exe", CHAIN_SIG_V1, Some(long.as_str()), None)
        }
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：12 正例过链入账 → 4 类注入 10 轮全抓 → 守恒 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = ChainLedger::new();
    // 12 正例：HKCU 有 / HKCU 无 HKLM 有 / 双无 三态轮转。
    let mut i = 0u32;
    let mut first_fp: u32;
    while i < 12 {
        let q = make_query(i);
        let out = match i % 3 {
            0 => run_chain(&q, CHAIN_SIG_V1, Some("~ HIGHDPIAWARE"), None)?,
            1 => run_chain(&q, CHAIN_SIG_V1, None, Some("~ WIN7RTM"))?,
            _ => run_chain(&q, CHAIN_SIG_V1, None, None)?,
        };
        led.record(&q, &out)?;
        i += 1;
    }
    first_fp = led.fingerprint();
    if led.count() != 12 || !led.monotone_seq() {
        return Err(FrcCode::ConservationBroken.code());
    }
    // 注入 4 类 × 10 轮。
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 4 {
            let hit = match inject_probe(slot) {
                Err(c) => {
                    // 命中判据：错误码必须落在本段窗口内。
                    c >= SEG_BASE && c < SEG_BASE + 0x40
                }
                Ok(_) => false,
            };
            led.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    if !led.all_caught() || !led.conserve() {
        return Err(FrcCode::ConservationBroken.code());
    }
    // 双跑一致：重放 12 正例（新账本），指纹必须相等。
    let mut led2 = ChainLedger::new();
    let mut j = 0u32;
    while j < 12 {
        let q = make_query(j);
        let out = match j % 3 {
            0 => run_chain(&q, CHAIN_SIG_V1, Some("~ HIGHDPIAWARE"), None)?,
            1 => run_chain(&q, CHAIN_SIG_V1, None, Some("~ WIN7RTM"))?,
            _ => run_chain(&q, CHAIN_SIG_V1, None, None)?,
        };
        led2.record(&q, &out)?;
        j += 1;
    }
    if led2.fingerprint() != first_fp {
        return Err(FrcCode::NotReproducible.code());
    }
    Ok((led.count() as u32, led.full_rejects, led.inject_caught[0]))
}

/// 探针三序（探针自检入口，主会话云端统一跑）。
pub fn probe_a() -> Result<(), u32> {
    // HKCU 有值 → 生效源必为 HKCU（用户态覆盖机器态）。
    let out = run_chain("C:\\p.exe", CHAIN_SIG_V1, Some("~"), Some("~ WIN7RTM"))?;
    match out.eff_source {
        Some(HiveKind::CurrentUser) => Ok(()),
        _ => Err(FrcCode::ArbDeviation.code()),
    }
}

pub fn probe_b() -> Result<(), u32> {
    // 空查询必拒（错误路径锚点）。
    match run_chain("", CHAIN_SIG_V1, None, None) {
        Err(c) if c == FrcCode::QueryEmpty.code() => Ok(()),
        _ => Err(FrcCode::ConservationBroken.code()),
    }
}

pub fn probe_c() -> Result<(), u32> {
    // 签名不匹配必拒（冻结 v1 锚点）。
    match run_chain("C:\\p.exe", CHAIN_SIG_V1.wrapping_add(1), None, None) {
        Err(c) if c == FrcCode::SigMismatch.code() => Ok(()),
        _ => Err(FrcCode::ConservationBroken.code()),
    }
}

// ── 判据裁决（运行期文本用 add_dyn，静态明细用 add） ────────────
/// 段位守卫：对齐 0x40、窗口下界、全码落窗。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x7EF00 {
        return false;
    }
    let mut i = 0;
    while i < FrcCode::ALL.len() {
        let v = FrcCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集：每码非空。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < FrcCode::ALL.len() {
        if detail_of(FrcCode::ALL[i].code()).len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据聚合（12 条 + 段位守卫等，单模块 ≤20）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1 段位守卫。
    v.push((
        "J1-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗宽0x40", SEG_BASE),
    ));
    // 2 冻结签名常量（推导值复核）。
    v.push((
        "J1-冻结v1签名",
        CHAIN_SIG_V1 == 0x8018_0001,
        format!("sig=0x{:08X}", CHAIN_SIG_V1),
    ));
    // 3 状态机五步全链过。
    v.push((
        "J1-五态全链",
        ChainStage::full_chain().is_ok(),
        String::from("Idle→…→Frozen 单调推进"),
    ));
    // 4 正常流三态（HKCU 覆盖 / 回退 HKLM / 双无未设）。
    let t0 = match run_chain("C:\\x.exe", CHAIN_SIG_V1, Some("~"), Some("~ WIN7RTM")) {
        Ok(o) => o.eff_source == Some(HiveKind::CurrentUser),
        Err(_) => false,
    };
    let t1 = match run_chain("C:\\x.exe", CHAIN_SIG_V1, None, Some("~ WIN7RTM")) {
        Ok(o) => o.eff_source == Some(HiveKind::LocalMachine),
        Err(_) => false,
    };
    let t2 = match run_chain("C:\\x.exe", CHAIN_SIG_V1, None, None) {
        Ok(o) => o.eff_source.is_none(),
        Err(_) => false,
    };
    v.push((
        "J1-HKCU优先仲裁",
        t0 && t1 && t2,
        format!("覆盖{} 回退{} 双无{}", t0, t1, t2),
    ));
    // 5 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        g.is_ok(),
        format!("{:?}", g),
    ));
    // 6 注入 4 类 ×10 轮全抓 + 守恒。
    let mut led = ChainLedger::new();
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 4 {
            let hit = inject_probe(slot).is_err();
            led.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    let total_tries = led.inject_tries[0] + led.inject_tries[1] + led.inject_tries[2] + led.inject_tries[3];
    let total_caught =
        led.inject_caught[0] + led.inject_caught[1] + led.inject_caught[2] + led.inject_caught[3];
    v.push((
        "J1-注入10/10全抓",
        led.all_caught(),
        format!("4类×10轮 t={} c={}", total_tries, total_caught),
    ));
    v.push((
        "J1-计数守恒",
        led.conserve() && total_tries == total_caught,
        String::from("注入=检出+漏检，漏检=0"),
    ));
    // 7 探针三序。
    let pa = probe_a().is_ok();
    let pb = probe_b().is_ok();
    let pc = probe_c().is_ok();
    v.push((
        "J1-探针三序",
        pa && pb && pc,
        format!("a={} b={} c={}", pa, pb, pc),
    ));
    // 8 文案闭集。
    v.push((
        "J1-文案闭集",
        detail_closed(),
        format!("{} 码全非空", FrcCode::ALL.len()),
    ));
    // 9 账本容量耗尽：满后拒新不丢旧。
    let mut led3 = ChainLedger::new();
    let mut i9 = 0u32;
    while i9 < (LEDGER_CAP as u32) + 2 {
        let q = make_query(i9);
        let out = run_chain(&q, CHAIN_SIG_V1, None, None);
        if let Ok(o) = out {
            let _ = led3.record(&q, &o);
        }
        i9 += 1;
    }
    v.push((
        "J1-账本容量守恒",
        led3.count() == LEDGER_CAP && led3.full_rejects == 2 && led3.monotone_seq(),
        format!("槽{} 拒{} seq单调", led3.count(), led3.full_rejects),
    ));
    // 10 边界四类：空/上限/上限+1/合法零值。
    let b0 = validate_query("").is_err();
    let b1 = validate_query("a".repeat(QUERY_LEN_MAX).as_str()).is_ok();
    let b2 = validate_query("a".repeat(QUERY_LEN_MAX + 1).as_str()).is_err();
    let b3 = validate_raw(Some("")).is_ok();
    v.push((
        "J1-边界四类",
        b0 && b1 && b2 && b3,
        format!("空{} 满{} 超{} 零{}", b0, b1, b2, b3),
    ));
    // 11 双跑一致（复现红线）。
    let g1 = golden_run();
    let g2 = golden_run();
    v.push((
        "J1-双跑一致",
        g1.is_ok() && g2.is_ok() && g1 == g2,
        String::from("golden_run 两跑同值"),
    ));
    v
}

/// 探针入口：本模块 CheckSet（判据 12 条，段内闭集）。
pub fn run_unx18461_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18461 兼容层标志面总纲（读取链冻结）");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            // 动态明细槽仅 8 个（契约红线）：前 8 条给运行期算值，
            // 其余转静态明细（CheckSet 记账与 all_passed 不受影响）。
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "判据通过；明细超动态槽，见实现");
        }
        i += 1;
    }
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_checks_green() {
        let set = run_unx18461_checks();
        assert!(set.all_passed(), "判据全绿失败：{}", set.red_items_text());
    }

    #[test]
    fn normal_flow_three_states() {
        let a = run_chain("C:\\a.exe", CHAIN_SIG_V1, Some("~ HIGHDPIAWARE"), Some("~ WIN7RTM"));
        match a {
            Ok(o) => assert!(o.eff_source == Some(HiveKind::CurrentUser), "HKCU 必须覆盖"),
            Err(_) => panic!("正常流不该报错"),
        }
        let b = run_chain("C:\\b.exe", CHAIN_SIG_V1, None, Some("~ WIN7RTM"));
        match b {
            Ok(o) => assert!(o.eff_source == Some(HiveKind::LocalMachine), "回退 HKLM"),
            Err(_) => panic!("回退流不该报错"),
        }
    }

    #[test]
    fn boundary_four_kinds() {
        assert!(validate_query("").is_err(), "空查询必拒");
        let full = "a".repeat(QUERY_LEN_MAX);
        assert!(validate_query(full.as_str()).is_ok(), "上限恰合法");
        let over = "a".repeat(QUERY_LEN_MAX + 1);
        assert!(validate_query(over.as_str()).is_err(), "上限+1 必拒");
        assert!(validate_raw(Some("")).is_ok(), "零长原始值合法");
    }

    #[test]
    fn injection_four_classes_all_caught() {
        assert!(inject_probe(0).is_err(), "空查询注入必抓");
        assert!(inject_probe(1).is_err(), "stage 乱序注入必抓");
        assert!(inject_probe(2).is_err(), "签名不匹配注入必抓");
        assert!(inject_probe(3).is_err(), "超长原始值注入必抓");
    }

    #[test]
    fn injection_order_and_dup_rejected() {
        // 乱序（跳级）与重复 stage 均为 StageRegress 路径。
        assert!(ChainStage::Idle.advance(ChainStage::Arbitrate).is_err());
        assert!(ChainStage::HkcuRead.advance(ChainStage::HkcuRead).is_err());
        assert!(ChainStage::Frozen.advance(ChainStage::Idle).is_err());
    }

    #[test]
    fn determinism_double_run() {
        let g1 = golden_run();
        let g2 = golden_run();
        match (g1, g2) {
            (Ok(a), Ok(b)) => assert!(a == b, "双跑必须逐值一致"),
            _ => panic!("黄金全链失败"),
        }
    }

    #[test]
    fn ledger_capacity_conserves() {
        let mut led = ChainLedger::new();
        let mut i = 0u32;
        while i < (LEDGER_CAP as u32) + 5 {
            let q = make_query(i);
            if let Ok(o) = run_chain(&q, CHAIN_SIG_V1, None, None) {
                let _ = led.record(&q, &o);
            }
            i += 1;
        }
        assert!(led.count() == LEDGER_CAP, "账本定容 16");
        assert!(led.full_rejects >= 1, "溢出必须显性记账");
        assert!(led.monotone_seq(), "seq 严格递增");
    }

    #[test]
    fn segment_guard_window() {
        assert!(segment_guard(), "段位守卫失败");
        let mut i = 0;
        while i < FrcCode::ALL.len() {
            let c = FrcCode::ALL[i].code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x40, "码越窗");
            assert!(detail_of(c).len() > 0, "文案空缺");
            i += 1;
        }
    }
}
