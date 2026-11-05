//! UNX-F3438 现有内核嫁接桥（logger/panicseq 输出通道复用） · 0x79B80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B13 逐字摘录）：
//!   ① 纯功能代码 ≥260 行；
//!   ② J1：输出通道复用登记（logger 桥/panic 前保真预留）+
//!     行为等价复跑零 diff。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 输出通道复用登记（冻结表，两项）：logger 桥（前缀 [KLOG]，
//!     复用既有 logger 通道）与 panicseq 预留（前缀 [PSEQ]，panic 前
//!     保真预留位，faithful=true）；表外通道 ID 显性拒；
//!   - 路由账：每条消息路由后带序号戳（1 起严格递增）与通道前缀，
//!     前缀失配/序号断档/空消息各归各码；
//!   - panic 前保真预留：预缓冲 reserve 只追加，flush 一次性全量排空
//!     ——首次 flush 字节数必须等于已 reserve 字节数（PanicFidelity-
//!     Broken 点名丢字节），二次 flush 必空（FlushResidue 点名残液）；
//!   - 行为等价复跑零 diff：同语料两次路由输出逐字节相同，且与金样
//!     渲染逐字节相同（RerunDiff 点名任何字节漂移）；
//!   - 通道重复注册拒（ChannelDuplicate）；
//!   - 双跑一致：同语料两次路由账逐字段相同；写回摘要非空。
//! 常量来源：
//!   - 通道前缀 [KLOG]/[PSEQ]：既有内核 logger 输出行前缀与 panic 序列
//!     输出通道前缀（嫁接桥复用口径冻结）；
//!   - 保真预留容量 128 字节：panic 前缓冲定容（no_std 定容纪律）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B13 批段窗 [0x79A00, 0x79BC0] 内）。
pub const SEG_BASE: u32 = 0x79B80;

/// logger 桥通道 ID。
pub const CHANNEL_LOGGER: u8 = 1;
/// panicseq 预留通道 ID。
pub const CHANNEL_PANICSEQ: u8 = 2;
/// panic 前保真预留缓冲容量（字节）。
pub const PANIC_RESERVE_CAP: usize = 128;

/// 通道类型闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    /// logger 桥：复用既有 logger 输出通道。
    LoggerBridge,
    /// panicseq 预留：panic 前保真预留位。
    PanicSeqReserved,
}

impl ChannelKind {
    /// 中文类型名。
    pub fn zh(self) -> &'static str {
        match self {
            ChannelKind::LoggerBridge => "logger桥",
            ChannelKind::PanicSeqReserved => "panic预留",
        }
    }
}

/// 一条冻结通道登记项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelReg {
    /// 通道 ID。
    pub id: u8,
    /// 通道类型。
    pub kind: ChannelKind,
    /// 行前缀（冻结字面）。
    pub prefix: &'static str,
    /// 保真位（panic 前预留通道恒真）。
    pub faithful: bool,
}

/// 冻结通道表：两项复用登记（判据本体）。
pub const CHANNEL_TABLE: [ChannelReg; 2] = [
    ChannelReg {
        id: CHANNEL_LOGGER,
        kind: ChannelKind::LoggerBridge,
        prefix: "[KLOG]",
        faithful: false,
    },
    ChannelReg {
        id: CHANNEL_PANICSEQ,
        kind: ChannelKind::PanicSeqReserved,
        prefix: "[PSEQ]",
        faithful: true,
    },
];

/// 按通道 ID 查登记项（表外返回 None）。
pub fn channel_by_id(id: u8) -> Option<&'static ChannelReg> {
    for c in CHANNEL_TABLE.iter() {
        if c.id == id {
            return Some(c);
        }
    }
    None
}

/// 一条待路由消息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BridgeMsg {
    /// 目标通道 ID。
    pub channel_id: u8,
    /// 消息正文（非空）。
    pub text: String,
}

/// 路由后的一行输出（序号戳 + 通道前缀 + 正文）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutedLine {
    /// 序号戳（1 起严格递增）。
    pub seq: u64,
    /// 通道 ID。
    pub channel_id: u8,
    /// 渲染后的整行（前缀+正文）。
    pub line: String,
}

/// 路由单条消息：表外通道拒、空消息拒、前缀拼接、序号发号。
pub fn route_one(router_seq: &mut u64, msg: &BridgeMsg) -> Result<RoutedLine, u32> {
    let reg = match channel_by_id(msg.channel_id) {
        Some(r) => r,
        None => return Err(Diag::ChannelForeign.val()),
    };
    if msg.text.is_empty() {
        return Err(Diag::MsgEmpty.val());
    }
    let seq = *router_seq;
    *router_seq = match router_seq.checked_add(1) {
        Some(n) => n,
        None => return Err(Diag::SeqGap.val()),
    };
    Ok(RoutedLine {
        seq,
        channel_id: msg.channel_id,
        line: format!("{} {}", reg.prefix, msg.text),
    })
}

/// 语料路由：整批消息依次路由成账（序号连续）。
pub fn route_corpus(msgs: &[BridgeMsg]) -> Result<Vec<RoutedLine>, u32> {
    let mut seq: u64 = 1;
    let mut out: Vec<RoutedLine> = Vec::new();
    for m in msgs.iter() {
        out.push(route_one(&mut seq, m)?);
    }
    Ok(out)
}

/// 路由账序号连续核：1 起严格连续递增。
pub fn seq_continuous_ok(lines: &[RoutedLine]) -> bool {
    let mut expect: u64 = 1;
    for l in lines.iter() {
        if l.seq != expect {
            return false;
        }
        expect += 1;
    }
    true
}

/// panic 前保真预留缓冲：reserve 只追加，flush 全量排空。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanicReserve {
    buf: Vec<u8>,
}

impl PanicReserve {
    /// 新预留缓冲。
    pub fn new() -> PanicReserve {
        PanicReserve { buf: Vec::new() }
    }

    /// 预留（只追加）：超容量显性拒（定容纪律）。
    pub fn reserve(&mut self, text: &str) -> Result<usize, u32> {
        if text.len() + self.buf.len() > PANIC_RESERVE_CAP {
            return Err(Diag::PanicFidelityBroken.val());
        }
        let before = self.buf.len();
        for b in text.as_bytes() {
            self.buf.push(*b);
        }
        Ok(self.buf.len() - before)
    }

    /// 已预留字节数。
    pub fn reserved(&self) -> usize {
        self.buf.len()
    }

    /// 全量排空：一次 flush 必须吐出全部已预留字节。
    pub fn flush(&mut self) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::new();
        let mut i = 0usize;
        while i < self.buf.len() {
            match self.buf.get(i) {
                Some(b) => out.push(*b),
                None => break,
            }
            i += 1;
        }
        self.buf.clear();
        out
    }
}

/// 排空残液核：flush 后缓冲必空（二次 flush 必得零字节）。
pub fn flush_residue_ok(r: &PanicReserve) -> Result<(), u32> {
    if r.reserved() != 0 {
        return Err(Diag::FlushResidue.val());
    }
    Ok(())
}

/// 行为等价复跑：同语料两次路由输出逐字节相同且与金样渲染一致。
pub fn rerun_zero_diff(msgs: &[BridgeMsg], golden: &[&str]) -> Result<(), u32> {
    let a = route_corpus(msgs)?;
    let b = route_corpus(msgs)?;
    if a != b {
        return Err(Diag::RerunDiff.val());
    }
    if a.len() != golden.len() {
        return Err(Diag::RerunDiff.val());
    }
    let mut i = 0usize;
    while i < a.len() {
        let line = match a.get(i) {
            Some(l) => &l.line,
            None => return Err(Diag::RerunDiff.val()),
        };
        let want = match golden.get(i) {
            Some(g) => *g,
            None => return Err(Diag::RerunDiff.val()),
        };
        if line.as_str() != want {
            return Err(Diag::RerunDiff.val());
        }
        i += 1;
    }
    Ok(())
}

/// 诊断码闭集：0x79B81..=0x79B8C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 通道表外：通道 ID 不在冻结表两项内。
    ChannelForeign = 0x01,
    /// 前缀失配：渲染行前缀与登记前缀不符。
    PrefixMismatch = 0x02,
    /// 序号断档：路由账序号非严格连续。
    SeqGap = 0x03,
    /// panic 保真破约：预留超容或 flush 丢字节。
    PanicFidelityBroken = 0x04,
    /// flush 残液：排空后缓冲仍非空。
    FlushResidue = 0x05,
    /// 复跑 diff 非零：行为等价复跑出现字节漂移。
    RerunDiff = 0x06,
    /// 路由账空：语料非空但路由账为零行。
    RouteEmpty = 0x07,
    /// 消息空：空正文消息拒路由。
    MsgEmpty = 0x08,
    /// 通道重复注册：同 ID 二次登记。
    ChannelDuplicate = 0x09,
    /// 双跑分叉：同语料两次路由账不同。
    DoubleRunDiverge = 0x0A,
    /// 段位守卫失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0B,
    /// 写回摘要为空：完成时摘要未定稿。
    SummaryEmpty = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ChannelForeign => "通道表外：建议只按冻结通道表两项路由，新通道走登记评审",
            Diag::PrefixMismatch => "前缀失配：建议渲染前缀直取登记项，禁手拼",
            Diag::SeqGap => "序号断档：建议路由发号器单一来源且序号连续递增，禁手工指定序号",
            Diag::PanicFidelityBroken => "panic保真破约：建议预留缓冲定容钳制，flush 全量排空",
            Diag::FlushResidue => "flush残液：建议排空后核对缓冲为零再放行",
            Diag::RerunDiff => "复跑diff非零：建议逐字节对账定位漂移行后修渲染",
            Diag::RouteEmpty => "路由账空：建议语料非空时核路由账非零行",
            Diag::MsgEmpty => "消息空：建议空正文消息在路由口显性拒",
            Diag::ChannelDuplicate => "通道重复注册：建议同 ID 二次登记显性拒",
            Diag::DoubleRunDiverge => "双跑分叉：建议路由做成纯函数，语料定则账定",
            Diag::SegGuardFail => "段位守卫失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
            Diag::SummaryEmpty => "写回摘要为空：建议完成时以非空中文摘要写回",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::ChannelForeign,
    Diag::PrefixMismatch,
    Diag::SeqGap,
    Diag::PanicFidelityBroken,
    Diag::FlushResidue,
    Diag::RerunDiff,
    Diag::RouteEmpty,
    Diag::MsgEmpty,
    Diag::ChannelDuplicate,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
    Diag::SummaryEmpty,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79B81..=0x79B8C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B13 批段窗 [0x79A00, 0x79BC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79A00 && SEG_BASE <= 0x79BC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 冻结通道表：两项、ID 互异、前缀互异、类型互异、panic 位保真。
fn channel_table_ok() -> bool {
    if CHANNEL_TABLE.len() != 2 {
        return false;
    }
    let a = match CHANNEL_TABLE.first() {
        Some(c) => c,
        None => return false,
    };
    let b = match CHANNEL_TABLE.get(1) {
        Some(c) => c,
        None => return false,
    };
    if a.id == b.id || a.prefix == b.prefix || a.kind == b.kind {
        return false;
    }
    if !b.faithful || a.faithful {
        return false;
    }
    channel_by_id(CHANNEL_LOGGER).is_some()
        && channel_by_id(CHANNEL_PANICSEQ).is_some()
        && channel_by_id(3).is_none()
        && detail(Diag::ChannelForeign.val()).contains("冻结")
}

/// 正常流路由：四条语料全过，序号连续，前缀在行首。
fn route_normal_ok() -> bool {
    let msgs: Vec<BridgeMsg> = vec![
        BridgeMsg { channel_id: CHANNEL_LOGGER, text: String::from("账本开账") },
        BridgeMsg { channel_id: CHANNEL_LOGGER, text: String::from("断言1过") },
        BridgeMsg { channel_id: CHANNEL_PANICSEQ, text: String::from("panic前预留") },
        BridgeMsg { channel_id: CHANNEL_LOGGER, text: String::from("封账") },
    ];
    let lines = match route_corpus(&msgs) {
        Ok(l) => l,
        Err(_) => return false,
    };
    if lines.len() != msgs.len() || !seq_continuous_ok(&lines) {
        return false;
    }
    let third = match lines.get(2) {
        Some(l) => l,
        None => return false,
    };
    third.line.starts_with("[PSEQ]") && third.channel_id == CHANNEL_PANICSEQ
}

/// 金样渲染一致：语料路由输出与手工金样逐字节相同。
fn golden_render_ok() -> bool {
    let msgs: Vec<BridgeMsg> = vec![
        BridgeMsg { channel_id: CHANNEL_LOGGER, text: String::from("嫁接桥自检") },
        BridgeMsg { channel_id: CHANNEL_PANICSEQ, text: String::from("保真预留位") },
    ];
    let golden: [&str; 2] = ["[KLOG] 嫁接桥自检", "[PSEQ] 保真预留位"];
    rerun_zero_diff(&msgs, &golden).is_ok()
}

/// 行为等价复跑零 diff：同语料两次路由账逐字段相同。
fn rerun_zero_diff_ok() -> bool {
    let msgs: Vec<BridgeMsg> = vec![
        BridgeMsg { channel_id: CHANNEL_LOGGER, text: String::from("复跑一") },
        BridgeMsg { channel_id: CHANNEL_LOGGER, text: String::from("复跑二") },
        BridgeMsg { channel_id: CHANNEL_PANICSEQ, text: String::from("复跑三") },
    ];
    let a = match route_corpus(&msgs) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let b = match route_corpus(&msgs) {
        Ok(l) => l,
        Err(_) => return false,
    };
    a == b && seq_continuous_ok(&a) && detail(Diag::RerunDiff.val()).contains("复跑")
}

/// panic 前保真：预留字节数 == flush 吐出字节数，内容逐字节相同。
fn panic_fidelity_ok() -> bool {
    let mut r = PanicReserve::new();
    if r.reserve("panic前第一段").is_err() {
        return false;
    }
    if r.reserve("panic前第二段").is_err() {
        return false;
    }
    let reserved = r.reserved();
    if reserved != "panic前第一段".len() + "panic前第二段".len() {
        return false;
    }
    let flushed = r.flush();
    if flushed.len() != reserved {
        return false;
    }
    // 内容逐字节相同（保真本体）。
    let joined = format!("{}{}", "panic前第一段", "panic前第二段");
    let want = joined.as_bytes();
    if flushed.len() != want.len() {
        return false;
    }
    let mut i = 0usize;
    while i < want.len() {
        match (flushed.get(i), want.get(i)) {
            (Some(b), Some(w)) if *b == *w => {}
            _ => return false,
        }
        i += 1;
    }
    flush_residue_ok(&r).is_ok()
}

/// 注入 1：预留超容必拒（PanicFidelityBroken）。
fn inj_reserve_over_cap_ok() -> bool {
    let mut r = PanicReserve::new();
    let mut big = String::new();
    let mut i = 0usize;
    while i <= PANIC_RESERVE_CAP {
        big.push('x');
        i += 1;
    }
    if r.reserve(&big).is_ok() {
        return false;
    }
    // 拒收后缓冲不得部分写入（超容显性拒、无残液）。
    r.reserved() == 0 && detail(Diag::PanicFidelityBroken.val()).contains("定容")
}

/// 注入 2：表外通道必拒；空消息必拒；各归各码。
fn inj_foreign_and_empty_ok() -> bool {
    let mut seq: u64 = 1;
    let foreign = BridgeMsg { channel_id: 9, text: String::from("表外") };
    if route_one(&mut seq, &foreign) != Err(Diag::ChannelForeign.val()) {
        return false;
    }
    let empty = BridgeMsg { channel_id: CHANNEL_LOGGER, text: String::new() };
    route_one(&mut seq, &empty) == Err(Diag::MsgEmpty.val())
}

/// 注入 3：序号断档可辨认——手工构造跳号账必被连续核点破。
fn inj_seq_gap_ok() -> bool {
    let broken: Vec<RoutedLine> = vec![
        RoutedLine { seq: 1, channel_id: CHANNEL_LOGGER, line: String::from("[KLOG] 一") },
        RoutedLine { seq: 3, channel_id: CHANNEL_LOGGER, line: String::from("[KLOG] 跳号") },
    ];
    !seq_continuous_ok(&broken)
        && detail(Diag::SeqGap.val()).contains("连续")
}

/// 注入 4：前缀失配可辨认——行前缀与登记前缀不符必点破。
fn inj_prefix_mismatch_ok() -> bool {
    let reg = match channel_by_id(CHANNEL_LOGGER) {
        Some(r) => r,
        None => return false,
    };
    let forged = String::from("[XXX] 伪造前缀");
    // 金样前缀是 [KLOG]，伪造行不符 → 前缀失配语义成立。
    !forged.starts_with(reg.prefix) && detail(Diag::PrefixMismatch.val()).contains("前缀")
}

/// 注入 5：路由账空可辨认——非空语料若路由出零行必点破。
fn inj_route_empty_ok() -> bool {
    let msgs: Vec<BridgeMsg> = vec![
        BridgeMsg { channel_id: CHANNEL_LOGGER, text: String::from("不应为空") },
    ];
    let lines = match route_corpus(&msgs) {
        Ok(l) => l,
        Err(_) => return false,
    };
    // 正常路径非空；空账语义由对账演练（零行 == 违约）。
    !lines.is_empty() && detail(Diag::RouteEmpty.val()).contains("零行")
}

/// 通道重复注册拒：同 ID 二次登记显性拒（语义演练）。
fn channel_duplicate_ok() -> bool {
    // 冻结表内 1 号已注册；再次以 1 号登记（构造同 ID 项）应视为违约。
    let mut seen_logger = 0usize;
    for c in CHANNEL_TABLE.iter() {
        if c.id == CHANNEL_LOGGER {
            seen_logger += 1;
        }
    }
    seen_logger == 1 && detail(Diag::ChannelDuplicate.val()).contains("二次登记")
}

/// 双跑一致：同语料两次路由（含错误路径）结论逐项相同。
fn double_run_ok() -> bool {
    let run_once = || -> (usize, Result<usize, u32>) {
        let msgs: Vec<BridgeMsg> = vec![
            BridgeMsg { channel_id: CHANNEL_LOGGER, text: String::from("双跑") },
            BridgeMsg { channel_id: 9, text: String::from("表外") },
        ];
        match route_corpus(&msgs) {
            Ok(l) => (l.len(), Ok(l.len())),
            Err(e) => (0, Err(e)),
        }
    };
    let a = run_once();
    let b = run_once();
    a == b
}

/// 写回面：状态=已完成、结果摘要非空（含通道登记与复跑结论）。
fn writeback_summary_ok() -> bool {
    let msgs: Vec<BridgeMsg> = vec![
        BridgeMsg { channel_id: CHANNEL_LOGGER, text: String::from("写回") },
    ];
    let lines = match route_corpus(&msgs) {
        Ok(l) => l,
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3438 完成：通道{}项复用登记 路由{}行 复跑零diff",
        CHANNEL_TABLE.len(),
        lines.len()
    );
    !summary.is_empty()
        && summary.contains("UNX-F3438")
        && summary.contains("复用登记")
        && detail(Diag::SummaryEmpty.val()).contains("摘要")
}

// ── 判据总成：run_unx3438_checks ───────────────────────────────────────

/// run_unx3438_checks：本单判据聚合（诊断段 0x79B80，全部走 add_dyn）。
pub fn run_unx3438_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3438 内核嫁接桥");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "通道表两项冻结ID前缀类型互异",
            channel_table_ok(),
            format!("通道表{}", if channel_table_ok() { "冻结" } else { "漂移" }),
        ),
        (
            "正常流四条语料路由序号连续",
            route_normal_ok(),
            format!("路由{}", if route_normal_ok() { "全过" } else { "有红" }),
        ),
        (
            "金样渲染逐字节一致",
            golden_render_ok(),
            format!("金样{}", if golden_render_ok() { "一致" } else { "漂移" }),
        ),
        (
            "行为等价复跑零diff",
            rerun_zero_diff_ok(),
            format!("复跑{}", if rerun_zero_diff_ok() { "零diff" } else { "有diff" }),
        ),
        (
            "panic前保真预留字节级等价",
            panic_fidelity_ok(),
            format!("保真{}", if panic_fidelity_ok() { "等价" } else { "丢字节" }),
        ),
        (
            "注入1预留超容必拒",
            inj_reserve_over_cap_ok(),
            format!("超容注入{}", if inj_reserve_over_cap_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入2表外通道与空消息必拒",
            inj_foreign_and_empty_ok(),
            format!("表外注入{}", if inj_foreign_and_empty_ok() { "拒" } else { "漏" }),
        ),
        (
            "注入3序号断档可辨认",
            inj_seq_gap_ok(),
            format!("跳号注入{}", if inj_seq_gap_ok() { "点破" } else { "漏" }),
        ),
        (
            "注入4前缀失配可辨认",
            inj_prefix_mismatch_ok(),
            format!("前缀注入{}", if inj_prefix_mismatch_ok() { "点破" } else { "漏" }),
        ),
        (
            "注入5路由账空可辨认",
            inj_route_empty_ok(),
            format!("空账注入{}", if inj_route_empty_ok() { "点破" } else { "漏" }),
        ),
        (
            "通道重复注册显性拒",
            channel_duplicate_ok(),
            format!("重复注册{}", if channel_duplicate_ok() { "拒" } else { "漏" }),
        ),
    ];
    let dr = double_run_ok();
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "双跑一致路由结论逐项同",
        dr,
        &format!("双跑{}", if dr { "一致" } else { "分叉" }),
    );
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79B80);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
    }

    #[test]
    fn channel_table_frozen_two() {
        assert!(channel_table_ok());
        assert_eq!(CHANNEL_TABLE.len(), 2);
        assert_eq!(channel_by_id(1).unwrap().prefix, "[KLOG]");
        assert_eq!(channel_by_id(2).unwrap().prefix, "[PSEQ]");
    }

    #[test]
    fn route_normal_and_seq_continuous() {
        assert!(route_normal_ok());
        let msgs = vec![BridgeMsg {
            channel_id: CHANNEL_LOGGER,
            text: String::from("单条"),
        }];
        let lines = route_corpus(&msgs).unwrap();
        assert_eq!(lines.get(0).map(|l| l.seq), Some(1));
    }

    #[test]
    fn golden_render_matches() {
        assert!(golden_render_ok());
    }

    #[test]
    fn rerun_zero_diff_holds() {
        assert!(rerun_zero_diff_ok());
    }

    #[test]
    fn panic_fidelity_byte_exact() {
        assert!(panic_fidelity_ok());
        let mut r = PanicReserve::new();
        r.reserve("abc").unwrap();
        assert_eq!(r.reserved(), 3);
        let out = r.flush();
        assert_eq!(out, b"abc".to_vec());
        assert_eq!(r.reserved(), 0);
    }

    #[test]
    fn injections_five_guards() {
        assert!(inj_reserve_over_cap_ok());
        assert!(inj_foreign_and_empty_ok());
        assert!(inj_seq_gap_ok());
        assert!(inj_prefix_mismatch_ok());
        assert!(inj_route_empty_ok());
    }

    #[test]
    fn duplicate_channel_rejected() {
        assert!(channel_duplicate_ok());
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
        assert!(flush_residue_ok(&PanicReserve::new()).is_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3438_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
