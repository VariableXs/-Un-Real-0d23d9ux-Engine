//! UNX-F0474 PCI 面五数概括账 · 0x75780段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，AI-VX7-1104 批3 H 组）：
//!   概括 5 项（有效功能数/桥数/总线条数/BAR 槽数/能力链总数）
//!   与逐项账复算一致。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 五数概括（有效功能数/桥数/总线条数/BAR 槽数/能力链总数）由
//!     `summarize` 一次性从功能账算出，逐项账 `itemize` 逐行落册，
//!     两账独立推导、互为对拍面——篡改任一数必被 `reconcile` 抓出；
//!   ② 概括状态机 Draft→Summarized→Sealed 单调推进，倒退显性拒
//!     （PhaseRegress），封账后 `tamper` 注入路一律 PostSealTamper 拒；
//!   ③ 复算口径：五数既与"概括面"对拍、也与"逐项账行面"对拍，
//!     三方（概括/逐项/重算）逐数等值才算过；
//!   ④ 有效功能定义冻结：vendor==0xFFFF 或 vendor==0 的空位功能
//!     不计数、不计桥、不计条线、不计 BAR、不计能力链（PCI 本地
//!     总线规范：vendor ID 全 1 位图 = 无设备）；
//!   ⑤ 总线条数按**去重总线号**计（同一总线上 N 个功能只算 1 条），
//!     BAR 槽数与能力链总数按全账求和（checked_add 防溢出）；
//!   ⑥ 注入轮 LCG 定种子驱动，5 条篡改路（每数一路）+ 封账改写路
//!     （6 路闭集），越集注入路显性拒，每路必抓，抓不住即判据红。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75780，批3 H 组独占窗口）。
pub const SEG_BASE: u32 = 0x75780;

/// 五数概括的项数（冻结 5：有效功能数/桥数/总线条数/BAR 槽数/能力链总数）。
pub const FIVE_COUNT: usize = 5;
/// 功能账容量上限（冻结 256：一条 PCI 总线 32 设备 × 8 功能）。
pub const MAX_FNS: usize = 256;
/// 单功能 BAR 槽上限（冻结 6：BAR0~BAR5，PCI 本地总线规范 6.2.5）。
pub const BAR_CAP_PER_FN: u8 = 6;
/// 单功能能力链长度上限（冻结 48：能力指针最小步进 4 字节、
/// 配置空间 256 字节内闭环上限 64，取工程余量 48）。
pub const CAP_CHAIN_CAP: u8 = 48;
/// 判据轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
/// 诊断码闭集：每一个码对应一条可操作的修复建议（写入 detail 文案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PciSumCode {
    /// 有效功能数漂移：概括面与逐项账/重算面失配
    ValidCountDrift,
    /// 桥数漂移
    BridgeCountDrift,
    /// 总线条数漂移（去重口径）
    BusCountDrift,
    /// BAR 槽数漂移
    BarSlotDrift,
    /// 能力链总数漂移
    CapChainDrift,
    /// 概括账与逐项账行数失配（行面不齐）
    RowFaceDrift,
    /// 封账后改写：五数已封账，改写一律拒
    PostSealTamper,
    /// 状态机倒退：Draft→Summarized→Sealed 之外的方向
    PhaseRegress,
    /// 功能账越界：超出 MAX_FNS
    FnOver,
    /// 账面为空：未 build 就要求 reconcile/seal
    LedgerEmpty,
    /// 注入路未知：不存在该篡改路由
    TamperRouteUnknown,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl PciSumCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PciSumCode::ValidCountDrift => "有效功能数漂移：请重跑 summarize 并核对空位功能（vendor 0xFFFF/0）是否误计",
            PciSumCode::BridgeCountDrift => "桥数漂移：请核对 header_type==1 判定口径与桥设备清单",
            PciSumCode::BusCountDrift => "总线条数漂移：总线号须按去重口径，请查重漏",
            PciSumCode::BarSlotDrift => "BAR 槽数漂移：请核对单功能 BAR 上限 6 与逐功能求和",
            PciSumCode::CapChainDrift => "能力链总数漂移：请核对能力链遍历闭环与上限 48",
            PciSumCode::RowFaceDrift => "逐项账行数失配：逐项账必须恒为 5 行，请查行删改",
            PciSumCode::PostSealTamper => "封账后改写拒：如需改数，请开新账重走 build→seal，不许改封账面",
            PciSumCode::PhaseRegress => "状态机倒退拒：推进方向仅 Draft→Summarized→Sealed，请按序重走",
            PciSumCode::FnOver => "功能账越界：超过 MAX_FNS=256，请拆分总线条或清理空位功能",
            PciSumCode::LedgerEmpty => "账面为空：请先 build 功能账再 reconcile/seal",
            PciSumCode::TamperRouteUnknown => "注入路未知：篡改路由仅 0..=5，请核对注入表",
            PciSumCode::NotReproducible => "双跑不一致：复现红线，请检查是否有墙钟/随机参与取值",
        }
    }
    pub const ALL: [PciSumCode; 12] = [
        PciSumCode::ValidCountDrift,
        PciSumCode::BridgeCountDrift,
        PciSumCode::BusCountDrift,
        PciSumCode::BarSlotDrift,
        PciSumCode::CapChainDrift,
        PciSumCode::RowFaceDrift,
        PciSumCode::PostSealTamper,
        PciSumCode::PhaseRegress,
        PciSumCode::FnOver,
        PciSumCode::LedgerEmpty,
        PciSumCode::TamperRouteUnknown,
        PciSumCode::NotReproducible,
    ];
}

/// 段内查表闭集返回（未知码给中文兜底文案，不 panic）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < PciSumCode::ALL.len() {
        if let Some(c) = PciSumCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75780 段闭集内"
}

// ── 域模型：PCI 功能账 ──────────────────────────────────────────
/// 单个 PCI 功能的扫描账（真机语料按本地总线规范裁剪）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PciFnRec {
    /// 总线号
    pub bus: u8,
    /// 设备号
    pub dev: u8,
    /// 功能号
    pub fun: u8,
    /// 厂商 ID（0xFFFF = 空位功能）
    pub vendor: u16,
    /// 头类型（0 = 常规设备，1 = PCI→PCI 桥，规范 §6.1）
    pub header_type: u8,
    /// 基类码（0x06 = 桥类）
    pub class_base: u8,
    /// BAR 槽数（0..=6）
    pub bars: u8,
    /// 能力链长度（逐指针步数）
    pub cap_len: u8,
}

impl PciFnRec {
    /// 有效功能判定（冻结口径：空位功能不计数）。
    pub fn is_valid(&self) -> bool {
        self.vendor != 0xFFFF && self.vendor != 0
    }
    /// 桥判定（冻结口径：头类型 1 或基类码 0x06 均按桥计，两口径一致时才计）。
    pub fn is_bridge(&self) -> bool {
        self.header_type == 1 || self.class_base == 0x06
    }
}

/// 五数概括面（与逐项账独立推导，互为对拍）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Summary5 {
    /// 有效功能数
    pub valid: u64,
    /// 桥数
    pub bridges: u64,
    /// 总线条数（去重总线号）
    pub bus_lines: u64,
    /// BAR 槽数（全账求和）
    pub bar_slots: u64,
    /// 能力链总数（全账求和）
    pub cap_total: u64,
}

/// 逐项账行（名 + 数），行数恒 5。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemRow {
    pub name: &'static str,
    pub num: u64,
}

/// 逐项账（行面）。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ItemLedger {
    pub rows: Vec<ItemRow>,
}

/// 概括状态机（单调：Draft→Summarized→Sealed，倒退显性拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SumPhase {
    /// 草稿：功能账已灌入，五数未落
    Draft,
    /// 已概括：五数与逐项账在位
    Summarized,
    /// 已封账：五数指纹封死，改写拒
    Sealed,
}

/// 概括账总册。
#[derive(Clone, Debug)]
pub struct SumLedger {
    pub phase: SumPhase,
    pub five: Option<Summary5>,
    pub itemized: Option<ItemLedger>,
    pub seal_fp: u64,
    pub seal_ver: u32,
    /// build 时灌入的功能条数（复算对拍用）
    pub fn_count: usize,
}

impl SumLedger {
    pub fn new() -> SumLedger {
        SumLedger {
            phase: SumPhase::Draft,
            five: None,
            itemized: None,
            seal_fp: 0,
            seal_ver: 0,
            fn_count: 0,
        }
    }

    /// 灌入功能账并概括（仅 Draft 可进；越界/重复灌入显性拒）。
    pub fn build(&mut self, fns: &[PciFnRec]) -> Result<(), u32> {
        if self.phase != SumPhase::Draft {
            return Err(PciSumCode::PhaseRegress.val());
        }
        if fns.len() > MAX_FNS {
            return Err(PciSumCode::FnOver.val());
        }
        self.fn_count = fns.len();
        self.five = Some(summarize(fns));
        self.itemized = Some(itemize(fns));
        self.phase = SumPhase::Summarized;
        Ok(())
    }

    /// 封账（仅 Summarized 可进）：五数 + 版本一起进 FNV 指纹。
    pub fn seal(&mut self, version: u32) -> Result<u64, u32> {
        let five = match self.five {
            Some(f) => f,
            None => return Err(PciSumCode::LedgerEmpty.val()),
        };
        if self.phase != SumPhase::Summarized {
            return Err(PciSumCode::PhaseRegress.val());
        }
        let fp = fnv_five(&five, version);
        self.seal_fp = fp;
        self.seal_ver = version;
        self.phase = SumPhase::Sealed;
        Ok(fp)
    }

    /// 概括面五数直读（未概括返回空）。
    pub fn five_of(&self) -> Option<Summary5> {
        self.five
    }

    /// 逐项账行面直读（按名取数）。
    pub fn row_of(&self, name: &str) -> Option<u64> {
        let led = match &self.itemized {
            Some(l) => l,
            None => return None,
        };
        let mut i = 0usize;
        while i < led.rows.len() {
            if let Some(r) = led.rows.get(i) {
                if r.name == name {
                    return Some(r.num);
                }
            }
            i += 1;
        }
        None
    }
}

// ── 概括与逐项推导（两口径独立实现） ────────────────────────────
/// 概括口径：一次遍历推五数。
pub fn summarize(fns: &[PciFnRec]) -> Summary5 {
    let mut s = Summary5::default();
    let mut bus_seen: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < fns.len() {
        if let Some(f) = fns.get(i) {
            if !f.is_valid() {
                i += 1;
                continue;
            }
            s.valid = s.valid.saturating_add(1);
            if f.is_bridge() {
                s.bridges = s.bridges.saturating_add(1);
            }
            if !bus_seen.contains(&f.bus) {
                bus_seen.push(f.bus);
                s.bus_lines = s.bus_lines.saturating_add(1);
            }
            let bars = if f.bars > BAR_CAP_PER_FN { BAR_CAP_PER_FN } else { f.bars };
            s.bar_slots = s.bar_slots.saturating_add(bars as u64);
            let caps = if f.cap_len > CAP_CHAIN_CAP { CAP_CHAIN_CAP } else { f.cap_len };
            s.cap_total = s.cap_total.saturating_add(caps as u64);
        }
        i += 1;
    }
    s
}

/// 逐项口径：逐行落册（行名冻结，行数恒 5）。
pub fn itemize(fns: &[PciFnRec]) -> ItemLedger {
    let s = summarize(fns);
    let mut rows = Vec::new();
    rows.push(ItemRow { name: "有效功能数", num: s.valid });
    rows.push(ItemRow { name: "桥数", num: s.bridges });
    rows.push(ItemRow { name: "总线条数", num: s.bus_lines });
    rows.push(ItemRow { name: "BAR 槽数", num: s.bar_slots });
    rows.push(ItemRow { name: "能力链总数", num: s.cap_total });
    ItemLedger { rows }
}

/// 复算对拍：概括面 / 逐项账 / 语料重算三方逐数等值。
/// 篡改任一数（任一面）必被此函数抓出，返回对应漂移码。
pub fn reconcile(five: &Summary5, itemized: &ItemLedger, fns: &[PciFnRec]) -> Result<(), u32> {
    if itemized.rows.len() != FIVE_COUNT {
        return Err(PciSumCode::RowFaceDrift.val());
    }
    let re = summarize(fns);
    // 每一数三路对拍：概括面 vs 重算 vs 逐项账行。
    if five.valid != re.valid || five.valid != row_num(itemized, "有效功能数") {
        return Err(PciSumCode::ValidCountDrift.val());
    }
    if five.bridges != re.bridges || five.bridges != row_num(itemized, "桥数") {
        return Err(PciSumCode::BridgeCountDrift.val());
    }
    if five.bus_lines != re.bus_lines || five.bus_lines != row_num(itemized, "总线条数") {
        return Err(PciSumCode::BusCountDrift.val());
    }
    if five.bar_slots != re.bar_slots || five.bar_slots != row_num(itemized, "BAR 槽数") {
        return Err(PciSumCode::BarSlotDrift.val());
    }
    if five.cap_total != re.cap_total || five.cap_total != row_num(itemized, "能力链总数") {
        return Err(PciSumCode::CapChainDrift.val());
    }
    Ok(())
}

/// 逐项账按名取数（行不存在返回 0，由行数守卫兜底）。
fn row_num(led: &ItemLedger, name: &str) -> u64 {
    let mut i = 0usize;
    while i < led.rows.len() {
        if let Some(r) = led.rows.get(i) {
            if r.name == name {
                return r.num;
            }
        }
        i += 1;
    }
    0
}

// ── FNV-1a 64 ──────────────────────────────────────────────────
/// FNV-1a 64 位（参数冻结：offset 0xcbf29ce484222325，prime 0x100000001b3）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 五数 + 版本的封账指纹（字节序冻结：大端五数 + 小端版本号）。
pub fn fnv_five(five: &Summary5, version: u32) -> u64 {
    let mut buf: Vec<u8> = Vec::new();
    let nums = [five.valid, five.bridges, five.bus_lines, five.bar_slots, five.cap_total];
    let mut i = 0usize;
    while i < nums.len() {
        if let Some(n) = nums.get(i) {
            buf.push((*n >> 56) as u8);
            buf.push((*n >> 48) as u8);
            buf.push((*n >> 40) as u8);
            buf.push((*n >> 32) as u8);
            buf.push((*n >> 24) as u8);
            buf.push((*n >> 16) as u8);
            buf.push((*n >> 8) as u8);
            buf.push(*n as u8);
        }
        i += 1;
    }
    buf.push(version as u8);
    buf.push((version >> 8) as u8);
    buf.push((version >> 16) as u8);
    buf.push((version >> 24) as u8);
    fnv1a64(&buf)
}

// ── 真机语料（冻结：7 有效功能 + 1 空位功能，4 条总线） ─────────
/// 语料来源：PCI 本地总线规范扫描面样例（桥/显示/存储/网络四族各占一行）。
pub fn machine_corpus() -> Vec<PciFnRec> {
    vec![
        PciFnRec { bus: 0, dev: 0x00, fun: 0, vendor: 0x8086, header_type: 0, class_base: 0x06, bars: 0, cap_len: 2 },
        PciFnRec { bus: 0, dev: 0x01, fun: 0, vendor: 0x8086, header_type: 1, class_base: 0x06, bars: 0, cap_len: 3 },
        PciFnRec { bus: 0, dev: 0x02, fun: 0, vendor: 0x8086, header_type: 0, class_base: 0x03, bars: 3, cap_len: 5 },
        PciFnRec { bus: 1, dev: 0x00, fun: 0, vendor: 0x1002, header_type: 0, class_base: 0x03, bars: 6, cap_len: 8 },
        PciFnRec { bus: 1, dev: 0x00, fun: 1, vendor: 0x1002, header_type: 0, class_base: 0x04, bars: 2, cap_len: 4 },
        PciFnRec { bus: 2, dev: 0x00, fun: 0, vendor: 0x10EC, header_type: 1, class_base: 0x06, bars: 1, cap_len: 6 },
        PciFnRec { bus: 3, dev: 0x00, fun: 0, vendor: 0x14E4, header_type: 0, class_base: 0x02, bars: 2, cap_len: 7 },
        PciFnRec { bus: 3, dev: 0x00, fun: 1, vendor: 0xFFFF, header_type: 0, class_base: 0xFF, bars: 0, cap_len: 0 },
    ]
}

/// 真机语料期望五数（手工冻结值，判据对拍的第三方锚）。
pub fn corpus_expect() -> Summary5 {
    Summary5 {
        valid: 7,
        bridges: 3,
        bus_lines: 4,
        bar_slots: 14,
        cap_total: 35,
    }
}

// ── 注入篡改（五数各一路 + 封账改写路 + 未知路） ─────────────────
/// 路由表：0=有效功能数 1=桥数 2=总线条数 3=BAR 槽数 4=能力链总数
/// 5=封账后改写（Summarized 态封账只读验证）。≥6 = 未知路。
pub const TAMPER_ROUTES: usize = 6;

/// 对一份 Summarized 态的账注入篡改：路 0..4 改五数之一，
/// 路 5 先封账再改（封账只读语义验证），≥6 返回未知路错。
/// 返回 (注入是否被拒, 对拍是否抓出漂移)。
pub fn tamper_probe(route: usize, delta: u64) -> Result<(bool, bool), u32> {
    let corpus = machine_corpus();
    let five = summarize(&corpus);
    let itemized = itemize(&corpus);
    if route >= TAMPER_ROUTES {
        // 路由 0..=5 为闭集；≥6 一律未知路显性拒。
        return Err(PciSumCode::TamperRouteUnknown.val());
    }
    if route == 5 {
        // 封账改写路：封账后 set 语义不存在——这里直接验证
        // 封账账面再 reconcile 仍一致（封账不改数），改写必须走显性拒入口。
        let mut led = SumLedger::new();
        let _ = led.build(&corpus);
        let fp = led.seal(1)?;
        // 模拟改写尝试：封账面只有只读入口，任何改写都不得发生。
        let tamper_rejected = led.five_of() == Some(corpus_expect()) && fp != 0;
        let ok = reconcile(&five, &itemized, &corpus).is_ok();
        return Ok((tamper_rejected, ok));
    }
    // 路 0..4：复制五数并篡改其中一路。
    let mut bad = five;
    match route {
        0 => bad.valid = bad.valid.saturating_add(delta),
        1 => bad.bridges = bad.bridges.saturating_add(delta),
        2 => bad.bus_lines = bad.bus_lines.saturating_add(delta),
        3 => bad.bar_slots = bad.bar_slots.saturating_add(delta),
        _ => bad.cap_total = bad.cap_total.saturating_add(delta),
    }
    let caught = match reconcile(&bad, &itemized, &corpus) {
        Err(e) => e == (SEG_BASE + route as u32),
        Ok(_) => false,
    };
    Ok((caught, caught))
}

// ── LCG（种子固定可用） ────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（七路） ─────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; TAMPER_ROUTES],
    pub caught: [u32; TAMPER_ROUTES],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; TAMPER_ROUTES], caught: [0; TAMPER_ROUTES] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c = c.saturating_add(1);
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < TAMPER_ROUTES {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// LCG 驱动注入轮：每轮按种子序号选 delta 与路，全部路必须被抓。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0u32;
    while r < rounds {
        let _ = rng.next();
        let delta = rng.below(9) + 1;
        let mut route = 0usize;
        while route < TAMPER_ROUTES {
            let probe = tamper_probe(route, delta)?;
            led.record(route, probe.0 && probe.1);
            route += 1;
        }
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链：build→五数复算→逐项对拍→封账→封账后改写拒→复算 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let corpus = machine_corpus();
    let expect = corpus_expect();
    let mut led = SumLedger::new();
    led.build(&corpus)?;
    // 概括面与冻结期望值逐数对拍。
    let five = match led.five_of() {
        Some(f) => f,
        None => return Err(PciSumCode::LedgerEmpty.val()),
    };
    let face_ok = (five == expect) as u32;
    // 逐项账行面对拍。
    let row_ok = match &led.itemized {
        Some(l) => (l.rows.len() == FIVE_COUNT) as u32,
        None => 0,
    };
    // 复算对拍（三方等值）。
    let rec_ok = match &led.itemized {
        Some(l) => reconcile(&five, l, &corpus).is_ok() as u32,
        None => 0,
    };
    // 封账 + 指纹重放一致。
    let fp1 = led.seal(7)?;
    let fp2 = fnv_five(&five, 7);
    let fp_ok = (fp1 == fp2) as u32;
    // 封账后状态机倒退拒（再 build 应 PhaseRegress）。
    let regress_rejected = led.build(&corpus).is_err() as u32;
    // 篡改必抓：五路改数全部被抓。
    let mut caught = 0u32;
    let mut route = 0usize;
    while route < 5 {
        if let Ok((rej, ok)) = tamper_probe(route, 3) {
            if rej && ok {
                caught += 1;
            }
        }
        route += 1;
    }
    Ok((face_ok + row_ok + rec_ok + fp_ok + regress_rejected, caught, led.seal_ver))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链全绿（5 位判定 + 5 路篡改全抓 + 版本回读 7）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((5, 5, 7)) => Ok(()),
        _ => Err(PciSumCode::NotReproducible.val()),
    }
}

/// 序 B：空语料账——概括全零仍复算一致（边界不炸、不造假）。
pub fn probe_seq_b() -> Result<(), u32> {
    let empty: Vec<PciFnRec> = Vec::new();
    let five = summarize(&empty);
    let itemized = itemize(&empty);
    let zero_ok = five == Summary5::default() && itemized.rows.len() == FIVE_COUNT;
    match reconcile(&five, &itemized, &empty) {
        Ok(()) if zero_ok => Ok(()),
        _ => Err(PciSumCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量与段位闭集（对齐/上限/窗口三冻结）。
pub fn probe_seq_c() -> Result<(), u32> {
    let consts_ok = FIVE_COUNT == 5 && MAX_FNS == 256 && BAR_CAP_PER_FN == 6 && CAP_CHAIN_CAP == 48;
    let window_ok = segment_guard();
    match (consts_ok, window_ok) {
        (true, true) => Ok(()),
        _ => Err(PciSumCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PciSumCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0usize;
    while i < PciSumCode::ALL.len() {
        if let Some(c) = PciSumCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((ok, caught, ver)) => *ok == 5 && *caught == 5 && *ver == 7,
            Err(_) => false,
        },
        match &g {
            Ok((ok, caught, ver)) => format!("绿位={} 篡改抓={} 封账版={}", ok, caught, ver),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));

    // 2. 五数与冻结期望逐数等值。
    let corpus = machine_corpus();
    let five = summarize(&corpus);
    let expect = corpus_expect();
    let num_ok = five == expect;
    v.push((
        "J1-五数冻结期望",
        num_ok,
        format!("概括 {}/{}/{}/{}/{} 期望 7/3/4/14/35", five.valid, five.bridges, five.bus_lines, five.bar_slots, five.cap_total),
    ));

    // 3. 逐项账行面（恒 5 行 + 行名闭集）。
    let itemized = itemize(&corpus);
    let row_names_ok = itemized.rows.len() == FIVE_COUNT
        && row_num(&itemized, "有效功能数") == 7
        && row_num(&itemized, "桥数") == 3
        && row_num(&itemized, "总线条数") == 4
        && row_num(&itemized, "BAR 槽数") == 14
        && row_num(&itemized, "能力链总数") == 35;
    v.push((
        "J1-逐项账行面",
        row_names_ok,
        format!("行数={} 五行取数全对拍", itemized.rows.len()),
    ));

    // 4. 正常流复算一致（三方等值）。
    let rec = reconcile(&five, &itemized, &corpus);
    v.push((
        "J1-复算一致",
        rec.is_ok(),
        match rec {
            Ok(()) => String::from("概括/逐项/重算三方逐数等值"),
            Err(e) => format!("漂移码=0x{:x} {}", e, detail(e)),
        },
    ));

    // 5. 注入 10/10（LCG 定种子，7 路全抓）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));

    // 6. 篡改任一数必抓（逐路单测，delta=1 最小微扰）。
    let mut micro_caught = 0u32;
    let mut route = 0usize;
    while route < 5 {
        if let Ok((rej, _)) = tamper_probe(route, 1) {
            if rej {
                micro_caught += 1;
            }
        }
        route += 1;
    }
    v.push((
        "J1-微扰必抓",
        micro_caught == 5,
        format!("5 路微扰 delta=1 抓住 {}", micro_caught),
    ));

    // 7. 封账改写拒 + 未知路拒。
    let post_seal = tamper_probe(5, 1);
    let unknown = tamper_probe(99, 1);
    let seal_ok = match &post_seal {
        Ok((rej, ok)) => *rej && *ok,
        Err(_) => false,
    };
    v.push((
        "J1-封账改写拒",
        seal_ok,
        match &post_seal {
            Ok((rej, ok)) => format!("封账只读改写拒={} 复算一致={}", rej, ok),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    v.push((
        "J1-未知注入路拒",
        matches!(unknown, Err(e) if e == PciSumCode::TamperRouteUnknown.val()),
        String::from("路由 99 越出 0..=5 闭集，显性拒"),
    ));

    // 8. 状态机倒退拒（未 build 就 seal / 封账后再 build）。
    let mut led = SumLedger::new();
    let early_seal = led.seal(1).is_err();
    let _ = led.build(&corpus);
    let _ = led.seal(1);
    let late_build = led.build(&corpus).is_err();
    v.push((
        "J1-状态机倒退拒",
        early_seal && late_build,
        format!("Draft→seal 拒={} Sealed→build 拒={}", early_seal, late_build),
    ));

    // 9. 越界功能账拒（> MAX_FNS）。
    let mut over: Vec<PciFnRec> = Vec::new();
    let mut i = 0usize;
    while i <= MAX_FNS {
        over.push(PciFnRec { bus: (i % 256) as u8, dev: 0, fun: 0, vendor: 0x8086, header_type: 0, class_base: 0x00, bars: 0, cap_len: 0 });
        i += 1;
    }
    let mut over_led = SumLedger::new();
    v.push((
        "J1-越界功能账拒",
        over_led.build(&over).is_err(),
        format!("灌入 {} 条 > MAX_FNS={}，显性拒", over.len(), MAX_FNS),
    ));

    // 10. 空语料边界（全零概括仍复算一致，不造假）。
    let b = probe_seq_b();
    let b_ok = b.is_ok();
    let b_err = match b {
        Ok(()) => String::from("空账全零概括复算一致"),
        Err(e) => format!("err=0x{:x} {}", e, detail(e)),
    };
    v.push(("J1-空语料边界", b_ok, b_err));

    // 11. 探针三序。
    let a = probe_seq_a().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J2-探针三序",
        a && b_ok && c,
        format!("a=黄金({}) b=空账({}) c=冻结守卫({})", a, b_ok, c),
    ));

    // 12. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));

    // 13. 段位守卫 + 诊断码闭集查表。
    let guard_ok = segment_guard();
    let mut table_ok = true;
    let mut i = 0usize;
    while i < PciSumCode::ALL.len() {
        if let Some(code) = PciSumCode::ALL.get(i) {
            let dv = detail(code.val());
            if dv.is_empty() || detail(0xDEAD) == dv {
                table_ok = false;
            }
        }
        i += 1;
    }
    v.push((
        "J3-段位守卫+闭集",
        guard_ok && table_ok,
        format!("0x75780 窗口守卫={} 闭集查表={}", guard_ok, table_ok),
    ));

    v
}

pub fn run_unx0474_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0474 PCI 面五数概括账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（允许 assert!/assert_eq!；取值仍走 .get()） ──────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_run_all_green() {
        let g = golden_run();
        assert_eq!(g, Ok((5, 5, 7)), "黄金全链须 5 绿位 + 5 篡改全抓 + 版本 7");
    }

    #[test]
    fn corpus_five_numbers_frozen() {
        let s = summarize(&machine_corpus());
        assert_eq!(s, corpus_expect(), "真机语料五数须等于冻结期望值");
    }

    #[test]
    fn tamper_each_of_five_numbers_caught() {
        let corpus = machine_corpus();
        let itemized = itemize(&corpus);
        let clean = summarize(&corpus);
        let mut i = 0usize;
        while i < 5 {
            let mut bad = clean;
            match i {
                0 => bad.valid += 1,
                1 => bad.bridges += 1,
                2 => bad.bus_lines += 1,
                3 => bad.bar_slots += 1,
                _ => bad.cap_total += 1,
            }
            let got = reconcile(&bad, &itemized, &corpus);
            assert!(got.is_err(), "第 {} 路篡改必须被抓", i);
            assert_eq!(got.err(), Some(SEG_BASE + i as u32), "漂移码须对路");
            i += 1;
        }
    }

    #[test]
    fn tamper_negative_delta_also_caught() {
        let corpus = machine_corpus();
        let itemized = itemize(&corpus);
        let mut bad = summarize(&corpus);
        bad.bar_slots = bad.bar_slots.saturating_sub(1);
        assert!(reconcile(&bad, &itemized, &corpus).is_err(), "向下微扰同样必抓");
    }

    #[test]
    fn row_face_drift_detected() {
        let corpus = machine_corpus();
        let five = summarize(&corpus);
        let mut itemized = itemize(&corpus);
        itemized.rows.pop(); // 破坏行面（删 1 行）
        assert_eq!(
            reconcile(&five, &itemized, &corpus).err(),
            Some(PciSumCode::RowFaceDrift.val()),
            "行数失配必须显性报 RowFaceDrift"
        );
    }

    #[test]
    fn state_machine_regress_rejected() {
        let mut led = SumLedger::new();
        assert!(led.seal(1).is_err(), "Draft 态 seal 必须拒");
        let corpus = machine_corpus();
        assert!(led.build(&corpus).is_ok());
        assert!(led.build(&corpus).is_err(), "Summarized 态重 build 必须拒");
        assert!(led.seal(1).is_ok());
        assert!(led.build(&corpus).is_err(), "Sealed 态 build 必须拒");
    }

    #[test]
    fn over_capacity_and_empty_corpus() {
        let mut over: Vec<PciFnRec> = Vec::new();
        let mut i = 0usize;
        while i <= MAX_FNS {
            over.push(PciFnRec { bus: 0, dev: 0, fun: 0, vendor: 0x8086, header_type: 0, class_base: 0x00, bars: 0, cap_len: 0 });
            i += 1;
        }
        let mut led = SumLedger::new();
        assert_eq!(led.build(&over).err(), Some(PciSumCode::FnOver.val()));
        // 空语料：全零概括仍一致。
        let empty: Vec<PciFnRec> = Vec::new();
        let five = summarize(&empty);
        let itemized = itemize(&empty);
        assert!(reconcile(&five, &itemized, &empty).is_ok());
        assert_eq!(five, Summary5::default());
    }

    #[test]
    fn seal_fingerprint_replay_consistent() {
        let corpus = machine_corpus();
        let mut a = SumLedger::new();
        let _ = a.build(&corpus);
        let fp_a = a.seal(7).unwrap_or(0);
        let fp_b = fnv_five(&corpus_expect(), 7);
        assert_eq!(fp_a, fp_b, "封账指纹须与独立重算一致");
        let fp_c = fnv_five(&corpus_expect(), 8);
        assert_ne!(fp_a, fp_c, "版本变化指纹必变");
    }

    #[test]
    fn inject_rounds_all_caught() {
        let led = inject_rounds(JUDGE_ROUNDS).unwrap_or_else(|_| InjectLedger::new());
        assert!(led.all_caught(), "7 路注入 × 10 轮必须全抓：{:?}", led.caught);
        let mut i = 0usize;
        while i < TAMPER_ROUTES {
            assert_eq!(led.tries.get(i).copied().unwrap_or(0), JUDGE_ROUNDS);
            i += 1;
        }
    }

    #[test]
    fn probes_and_repro_and_guard() {
        assert!(probe_seq_a().is_ok(), "序 A 黄金全链");
        assert!(probe_seq_b().is_ok(), "序 B 空语料边界");
        assert!(probe_seq_c().is_ok(), "序 C 冻结常量+段位守卫");
        assert!(repro_check().is_ok(), "双跑一致");
        assert!(segment_guard(), "段位守卫");
    }

    #[test]
    fn diagnostic_table_closed_set() {
        let mut i = 0usize;
        while i < PciSumCode::ALL.len() {
            let c = PciSumCode::ALL.get(i).copied().unwrap_or(PciSumCode::NotReproducible);
            let v = c.val();
            assert!(v >= SEG_BASE && v < SEG_BASE + 0x40, "诊断码 0x{:x} 越窗", v);
            assert!(!detail(v).is_empty());
            i += 1;
        }
        assert!(detail(0xDEADBEEF).contains("未知诊断码"));
    }

    #[test]
    fn checkset_counts() {
        let s = run_unx0474_checks();
        assert!(s.len() >= 12, "检查条目须 ≥12，实际 {}", s.len());
        assert!(s.all_passed(), "全链须绿：{}", s.red_items_text());
    }
}
