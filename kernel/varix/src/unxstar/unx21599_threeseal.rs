//! UNX-F21599 · 满账三印宣示（文印/账印/链印）
//!
//! 诊断段 0x91091~0x91098（F2-B40 域收官段 19/100）。
//!
//! 判据原文（施工书 UNX-F21599-J1）：三印：文印（文档一致）/账印（账平）/链印（git 链闭合）全绿。
//!
//! 设计要点（为什么是「三印并列」而不是「一个综合分」）：
//! 三印描述的是三件互不蕴含的事实：文档条目对不对、账本平不平、提交链闭不闭合。任何
//! 合成打分都会允许「两项满分掩盖一项缺失」，而验收恰恰只关心最弱那一项。所以本模型
//! 里三印各自是独立校验器，各产出一个局部摘要，宣示只是「三绿之后的一次合并」，合并
//! 本身不具任何豁免权——少一印就整单拒收，且必须点名缺的是哪一印。
//!
//! 缺印与印不符分成两个码（0x91092 vs 0x91093 等）：前者是「没做」，后者是「做了但对不上」。
//! 处置完全不同——补做 vs 复核；合成一个码，值班人只能重头查一遍。
//!
//! 合成指纹取三印摘要的顺序咬合，而不是把三印的原始字段拼起来再散列：原始字段里含
//! 可变长度与可空项，拼法一变指纹就变；摘要已经是定长整数，咬合方式可以写死在规范里。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 文印条目数（四十批册，每批一册文档）。
pub const DOC_ENTRIES: u16 = 40;
/// 账印总数（40 批 × 6000 行，来源：施工书域账累计栏）。
pub const LEDGER_TOTAL: u32 = 240_000;
/// 链印长度（四十次收口提交）。
pub const CHAIN_LEN: u16 = 40;
/// 终条编号（宣示落款用）。
pub const TAIL_ID: u32 = 21_600;

/// 三印诊断码闭集（0x91091~0x91098 八码）。
///
/// 显式判别式而非隐式递增：码值是跨模块日志的对账键，插入新变体会整体漂移。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThreeSealCode {
    /// 0x91091 三印全绿，宣示成立。
    AllSealed = 0x91091,
    /// 0x91092 文印缺失。
    DocMissing = 0x91092,
    /// 0x91093 文印与文档册不符。
    DocMismatch = 0x91093,
    /// 0x91094 账印缺失。
    LedgerMissing = 0x91094,
    /// 0x91095 账印不平（双路不一致或总数不符）。
    LedgerDiverge = 0x91095,
    /// 0x91096 链印缺失。
    ChainMissing = 0x91096,
    /// 0x91097 链印断裂（长度不足 / 首尾不闭合 / 父链不连续）。
    ChainBroken = 0x91097,
    /// 0x91098 已宣示仍再次宣示。
    AlreadyProclaimed = 0x91098,
}

impl ThreeSealCode {
    /// 现象。
    pub const fn what(self) -> &'static str {
        match self {
            ThreeSealCode::AllSealed => "三印全绿，宣示成立",
            ThreeSealCode::DocMissing => "文印缺失",
            ThreeSealCode::DocMismatch => "文印与文档册不符",
            ThreeSealCode::LedgerMissing => "账印缺失",
            ThreeSealCode::LedgerDiverge => "账印不平",
            ThreeSealCode::ChainMissing => "链印缺失",
            ThreeSealCode::ChainBroken => "链印断裂",
            ThreeSealCode::AlreadyProclaimed => "已宣示仍再次宣示",
        }
    }
    /// 原因。
    pub const fn why(self) -> &'static str {
        match self {
            ThreeSealCode::AllSealed => "文印条目齐、账印双路平、链印首尾闭合",
            ThreeSealCode::DocMissing => "文档册未产出或未提交文印",
            ThreeSealCode::DocMismatch => "条目数或文档指纹与册面记录不一致",
            ThreeSealCode::LedgerMissing => "账本未结或未提交账印",
            ThreeSealCode::LedgerDiverge => "双路记账不一致，或总数不等于配额",
            ThreeSealCode::ChainMissing => "提交链未导出或未提交链印",
            ThreeSealCode::ChainBroken => "提交次数不足、首尾哈希不同或父链断开",
            ThreeSealCode::AlreadyProclaimed => "宣示是终态操作，重复执行会覆盖回执",
        }
    }
    /// 建议。
    pub const fn advice(self) -> &'static str {
        match self {
            ThreeSealCode::AllSealed => "归档回执，转入域闭环聚合",
            ThreeSealCode::DocMissing => "补齐四十册文档后重出文印",
            ThreeSealCode::DocMismatch => "以册面为准重算文印指纹，不要改册面",
            ThreeSealCode::LedgerMissing => "先结账再出账印",
            ThreeSealCode::LedgerDiverge => "逐批回放双路记账，定位分叉批",
            ThreeSealCode::ChainMissing => "导出提交链后重出链印",
            ThreeSealCode::ChainBroken => "补提或修正父链后重出链印",
            ThreeSealCode::AlreadyProclaimed => "读取既有回执，不要重复宣示",
        }
    }
    /// 数值形式（u32：五位数码段，`as u16` 会截断成 0x1091）。
    pub const fn code(self) -> u32 {
        self as u32
    }
}

/// 印种（定位到「哪一印」，与码互为经纬）。
///
/// `Whole` 是给「与具体某印无关」的故障（如重复宣示）留的落点：不设它就得让
/// `kind` 变成 `Option`，而 `Option` 在导出文本里又要多一个「空」分支，
/// 反而比一个显式的整体项更难读。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealKind {
    /// 文印。
    Doc,
    /// 账印。
    Ledger,
    /// 链印。
    Chain,
    /// 整体（不指向单一印）。
    Whole,
}

impl SealKind {
    /// 短标签（导出与日志用，稳定不可改）。
    pub const fn label(self) -> &'static str {
        match self {
            SealKind::Doc => "doc",
            SealKind::Ledger => "ledger",
            SealKind::Chain => "chain",
            SealKind::Whole => "whole",
        }
    }
    /// 该印缺失时该报的码。
    pub const fn missing_code(self) -> u32 {
        match self {
            SealKind::Doc => ThreeSealCode::DocMissing.code(),
            SealKind::Ledger => ThreeSealCode::LedgerMissing.code(),
            SealKind::Chain => ThreeSealCode::ChainMissing.code(),
            SealKind::Whole => ThreeSealCode::AllSealed.code(),
        }
    }
}

/// 文印（文档一致）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DocSeal {
    /// 文档条目数。
    pub entries: u16,
    /// 文档册指纹。
    pub fingerprint: u64,
}

impl DocSeal {
    /// 局部摘要（定长，供合成指纹咬合）。
    pub const fn digest(&self) -> u64 {
        let mut h = fnv_u64(0xcbf2_9ce4_8422_2325, self.entries as u64);
        h = fnv_u64(h, self.fingerprint);
        h
    }
}

/// 账印（账平）。
///
/// 双路两字段并存是刻意的：只留一个 `total` 就等于让账本自证，双路校验失去意义。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerSeal {
    /// 账面总数。
    pub total: u32,
    /// 路径 A（逐批实计）。
    pub route_a: u32,
    /// 路径 B（配额基数 + 偏差回放）。
    pub route_b: u32,
}

impl LedgerSeal {
    /// 是否账平（双路一致且等于总数）。
    pub const fn is_balanced(&self) -> bool {
        self.route_a == self.route_b && self.route_a == self.total
    }
    /// 局部摘要。
    pub const fn digest(&self) -> u64 {
        let mut h = fnv_u64(0xcbf2_9ce4_8422_2325, self.total as u64);
        h = fnv_u64(h, self.route_a as u64);
        h = fnv_u64(h, self.route_b as u64);
        h
    }
}

/// 链印（git 链闭合）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChainSeal {
    /// 提交链长度。
    pub length: u16,
    /// 链首哈希。
    pub head: u64,
    /// 链尾哈希。
    pub tail: u64,
    /// 父链是否逐提交连续（导出时逐条核对的结果）。
    pub contiguous: bool,
}

impl ChainSeal {
    /// 是否闭合（长度够、首尾同、父链连续、哈希非零）。
    pub const fn is_closed(&self) -> bool {
        self.length == CHAIN_LEN
            && self.head != 0
            && self.head == self.tail
            && self.contiguous
    }
    /// 局部摘要。
    pub const fn digest(&self) -> u64 {
        let mut h = fnv_u64(0xcbf2_9ce4_8422_2325, self.length as u64);
        h = fnv_u64(h, self.head);
        h = fnv_u64(h, self.tail);
        h = fnv_u64(h, if self.contiguous { 1 } else { 0 });
        h
    }
}

/// 三印故障（码 + 印种 + 短注，三者共同定位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StampFault {
    /// 诊断码（u32）。
    pub code: u32,
    /// 印种。
    pub kind: SealKind,
    /// 短注（不符项的人话说明，静态串避免为报错而分配）。
    pub note: &'static str,
}

impl StampFault {
    const fn new(code: ThreeSealCode, kind: SealKind, note: &'static str) -> StampFault {
        StampFault { code: code.code(), kind, note }
    }
    /// 缺印故障（码由印种推导，避免「缺链印却报文印码」这类错配）。
    const fn missing(kind: SealKind) -> StampFault {
        StampFault {
            code: kind.missing_code(),
            kind,
            note: "该印未提交",
        }
    }
    /// 码是否落在本单码段内。
    pub const fn is_known(self) -> bool {
        self.code >= 0x91091 && self.code <= 0x91098
    }
    /// 三要素 + 印种 + 短注拼一行（运维面板直读）。
    pub fn text(&self) -> String {
        let c = match self.code {
            0x91091 => ThreeSealCode::AllSealed,
            0x91092 => ThreeSealCode::DocMissing,
            0x91093 => ThreeSealCode::DocMismatch,
            0x91094 => ThreeSealCode::LedgerMissing,
            0x91095 => ThreeSealCode::LedgerDiverge,
            0x91096 => ThreeSealCode::ChainMissing,
            0x91097 => ThreeSealCode::ChainBroken,
            _ => ThreeSealCode::AlreadyProclaimed,
        };
        let mut out = String::with_capacity(192);
        out.push_str(c.what());
        out.push_str(" | ");
        out.push_str(c.why());
        out.push_str(" | ");
        out.push_str(c.advice());
        out.push_str(" | seal=");
        out.push_str(self.kind.label());
        out.push_str(" note=");
        out.push_str(self.note);
        out
    }
}

/// 宣示回执（三印摘要 + 合成指纹）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Proclamation {
    /// 文印条目数。
    pub entries: u16,
    /// 账印总数。
    pub ledger_total: u32,
    /// 链印长度。
    pub chain_len: u16,
    /// 文印摘要。
    pub doc_digest: u64,
    /// 账印摘要。
    pub ledger_digest: u64,
    /// 链印摘要。
    pub chain_digest: u64,
    /// 合成指纹（三摘要顺序咬合）。
    pub composite: u64,
    /// 落款终条编号。
    pub tail_id: u32,
}

impl Proclamation {
    /// 回执是否完整（三摘要全非零 + 三个计数命中 + 落款在位）。
    ///
    /// 摘要非零单独作为一条：FNV 偏移基非零，算出 0 只可能是「印是空壳」或
    /// 「摘要函数被替换」，这两种都不该被当成合格回执。
    pub const fn is_complete(&self) -> bool {
        self.entries == DOC_ENTRIES
            && self.ledger_total == LEDGER_TOTAL
            && self.chain_len == CHAIN_LEN
            && self.doc_digest != 0
            && self.ledger_digest != 0
            && self.chain_digest != 0
            && self.composite != 0
            && self.tail_id == TAIL_ID
    }

    /// 三摘要元组（比对用；合成指纹对不上时先看是哪一印变了）。
    pub const fn digests(&self) -> [u64; 3] {
        [self.doc_digest, self.ledger_digest, self.chain_digest]
    }

    /// 回执一行文本（字段序固定，便于逐行 diff）。
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(192);
        out.push_str("three-seal doc=");
        out.push_str(&self.entries.to_string());
        out.push_str(" ledger=");
        out.push_str(&self.ledger_total.to_string());
        out.push_str(" chain=");
        out.push_str(&self.chain_len.to_string());
        out.push_str(" composite=");
        out.push_str(&hex64(self.composite));
        out.push_str(" tail=");
        out.push_str(&self.tail_id.to_string());
        out
    }
}

/// 三印册（三印 + 宣示状态）。
///
/// 三印用 `Option` 而不是「字段 + 存在位」：缺印是最常见的状态（产出有先后），
/// 用 Option 可以让「缺印」在类型层面就不可被误读成「印的值是 0」。
pub struct SealBook {
    doc: Option<DocSeal>,
    ledger: Option<LedgerSeal>,
    chain: Option<ChainSeal>,
    proclaimed: bool,
    /// 成功宣示次数（幂等观测点：成功一次后恒为 1）。
    proclaim_count: u16,
    /// 被拒宣示次数（缺印 / 重复宣示都要留痕）。
    rejects: u16,
    /// 宣示后的合成指纹（0 表示尚未宣示）。
    composite: u64,
}

impl SealBook {
    /// 空册（三印全缺）。
    pub const fn new() -> SealBook {
        SealBook {
            doc: None,
            ledger: None,
            chain: None,
            proclaimed: false,
            proclaim_count: 0,
            rejects: 0,
            composite: 0,
        }
    }

    /// 由三印构造（任一可为 None，用于构造缺印样本）。
    pub const fn from_seals(
        doc: Option<DocSeal>,
        ledger: Option<LedgerSeal>,
        chain: Option<ChainSeal>,
    ) -> SealBook {
        SealBook {
            doc,
            ledger,
            chain,
            proclaimed: false,
            proclaim_count: 0,
            rejects: 0,
            composite: 0,
        }
    }

    /// 文印校验器（独立，不依赖账印与链印）。
    ///
    /// 「缺」与「不符」在这里就分开：缺是 `missing`（码由印种推），不符是 `new` 指定码。
    /// 若统一返回同一个码，撤回「没做」与「做错」的处置差异就丢了。
    pub fn verify_doc(&self) -> Result<u64, StampFault> {
        let d = match self.doc {
            Some(d) => d,
            None => return Err(StampFault::missing(SealKind::Doc)),
        };
        if d.entries != DOC_ENTRIES {
            return Err(StampFault::new(ThreeSealCode::DocMismatch, SealKind::Doc, "条目数与四十册不符"));
        }
        if d.fingerprint == 0 {
            return Err(StampFault::new(ThreeSealCode::DocMismatch, SealKind::Doc, "文档指纹为零"));
        }
        Ok(d.digest())
    }

    /// 账印校验器。
    pub fn verify_ledger(&self) -> Result<u64, StampFault> {
        let l = match self.ledger {
            Some(l) => l,
            None => return Err(StampFault::missing(SealKind::Ledger)),
        };
        if l.route_a != l.route_b {
            return Err(StampFault::new(ThreeSealCode::LedgerDiverge, SealKind::Ledger, "双路记账分叉"));
        }
        if l.total != LEDGER_TOTAL || l.route_a != l.total {
            return Err(StampFault::new(ThreeSealCode::LedgerDiverge, SealKind::Ledger, "总数与配额或双路结果不符"));
        }
        Ok(l.digest())
    }

    /// 链印校验器。
    pub fn verify_chain(&self) -> Result<u64, StampFault> {
        let c = match self.chain {
            Some(c) => c,
            None => return Err(StampFault::missing(SealKind::Chain)),
        };
        if c.length != CHAIN_LEN {
            return Err(StampFault::new(ThreeSealCode::ChainBroken, SealKind::Chain, "提交次数不足"));
        }
        if !c.contiguous {
            return Err(StampFault::new(ThreeSealCode::ChainBroken, SealKind::Chain, "父链不连续"));
        }
        if c.head == 0 || c.head != c.tail {
            return Err(StampFault::new(ThreeSealCode::ChainBroken, SealKind::Chain, "首尾哈希不闭合"));
        }
        Ok(c.digest())
    }

    /// 缺印清单（按文→账→链固定序；用于一次性告诉对方「缺几印、缺哪几印」）。
    pub fn missing(&self) -> Vec<SealKind> {
        let mut out = Vec::with_capacity(3);
        if self.doc.is_none() {
            out.push(SealKind::Doc);
        }
        if self.ledger.is_none() {
            out.push(SealKind::Ledger);
        }
        if self.chain.is_none() {
            out.push(SealKind::Chain);
        }
        out
    }

    /// 宣示：三印全绿才成立。
    ///
    /// 顺序固定「文 → 账 → 链」，且只报第一因：三印同时坏时先修哪一印是有先后道理的
    /// （文档是账的依据），一次报三条会让人误以为要并行修三处。
    ///
    /// 计数放在外层而不是各分支：三个校验器都用 `?` 提前返回，若在每个分支里各加一次，
    /// 漏一处就少记一次；包一层「先跑、失败就记」才能保证「拒绝必有留痕」是结构性的。
    pub fn proclaim(&mut self) -> Result<Proclamation, StampFault> {
        let outcome = self.stamp();
        if outcome.is_err() {
            self.rejects = self.rejects.saturating_add(1);
        }
        outcome
    }

    /// 宣示本体（不含留痕计数）。
    fn stamp(&mut self) -> Result<Proclamation, StampFault> {
        if self.proclaimed {
            return Err(StampFault::new(
                ThreeSealCode::AlreadyProclaimed,
                SealKind::Whole,
                "宣示已成立",
            ));
        }
        let doc_digest = self.verify_doc()?;
        let ledger_digest = self.verify_ledger()?;
        let chain_digest = self.verify_chain()?;
        let mut composite = fnv_u64(0xcbf2_9ce4_8422_2325, doc_digest);
        composite = fnv_u64(composite, ledger_digest);
        composite = fnv_u64(composite, chain_digest);
        self.proclaimed = true;
        self.proclaim_count = self.proclaim_count.saturating_add(1);
        self.composite = composite;
        let (entries, ledger_total, chain_len) = match (self.doc, self.ledger, self.chain) {
            (Some(d), Some(l), Some(c)) => (d.entries, l.total, c.length),
            _ => (0, 0, 0),
        };
        Ok(Proclamation {
            entries,
            ledger_total,
            chain_len,
            doc_digest,
            ledger_digest,
            chain_digest,
            composite,
            tail_id: TAIL_ID,
        })
    }

    /// 是否已宣示。
    pub const fn is_proclaimed(&self) -> bool {
        self.proclaimed
    }
    /// 成功宣示次数。
    pub const fn proclaim_count(&self) -> u16 {
        self.proclaim_count
    }
    /// 被拒宣示次数。
    pub const fn rejects(&self) -> u16 {
        self.rejects
    }
    /// 宣示后的合成指纹（未宣示为 0）。
    pub const fn composite(&self) -> u64 {
        self.composite
    }

    /// 三印状态导出（缺印也导，字段位永远在，便于 diff 出「昨天还在、今天没了」）。
    pub fn export(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str("threeseal doc=");
        out.push_str(present(self.doc.is_some()));
        out.push_str(" ledger=");
        out.push_str(present(self.ledger.is_some()));
        out.push_str(" chain=");
        out.push_str(present(self.chain.is_some()));
        out.push_str(" missing=");
        out.push_str(&self.missing().len().to_string());
        out.push_str(" proclaimed=");
        out.push_str(if self.proclaimed { "1" } else { "0" });
        out.push_str(" count=");
        out.push_str(&self.proclaim_count.to_string());
        out.push_str(" rejects=");
        out.push_str(&self.rejects.to_string());
        out.push_str(" composite=");
        let mark = if self.composite == 0 { "-".to_string() } else { hex64(self.composite) };
        out.push_str(mark.as_str());
        out
    }
}

/// 存在位转文本（缺印写 `absent` 而不是留空，留空会被 diff 当成「字段被删」）。
const fn present(has: bool) -> &'static str {
    if has { "present" } else { "absent" }
}

/// FNV-1a 64 单步（质数与偏移基来源：FNV 规范 1a 变体）。
const fn fnv_u64(mut h: u64, v: u64) -> u64 {
    let mut i = 0u32;
    while i < 8 {
        let byte = ((v >> (i * 8)) & 0xFF) as u8;
        h ^= byte as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

/// 64 位十六进制（no_std 手写，避免格式化器的分配）。
fn hex64(v: u64) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(16);
    let mut shift = 60i32;
    while shift >= 0 {
        let nib = ((v >> shift) & 0xF) as usize;
        s.push(HEX[nib] as char);
        shift -= 4;
    }
    s
}

/// 标准三印（判据夹具）：文印 40 册、账印 240000 双路平、链印 40 提交首尾闭合。
///
/// 返回 `Result`：夹具本身要能逐印通过校验，否则后面十六条判据都会在「三印不绿」
/// 这个假前提上跑，红绿都不再有意义。
pub fn standard_book() -> Result<SealBook, StampFault> {
    let doc = DocSeal { entries: DOC_ENTRIES, fingerprint: 0x9f21_0a11_5c3d_7e01 };
    let ledger = LedgerSeal { total: LEDGER_TOTAL, route_a: LEDGER_TOTAL, route_b: LEDGER_TOTAL };
    let chain = ChainSeal { length: CHAIN_LEN, head: 0x51ad_2c07_9b3e_0041, tail: 0x51ad_2c07_9b3e_0041, contiguous: true };
    let book = SealBook::from_seals(Some(doc), Some(ledger), Some(chain));
    book.verify_doc()?;
    book.verify_ledger()?;
    book.verify_chain()?;
    Ok(book)
}

/// 宣示并取故障：被接受时回一个码 0 的占位故障，让位置断言必然落空。
///
/// 判据需要「拿故障码」，而 `proclaim` 成功时没有故障可拿。此处不用 panic（本批禁 panic），
/// 改用「码 0」这个不可能出现在闭集里的值——任何 `== Some(0x9109x)` 的断言都会失败，
/// 失败信息仍能指出「本该被拒却被接受」，不会静默当成通过。
fn refusal(book: &mut SealBook) -> StampFault {
    match book.proclaim() {
        Ok(_) => StampFault { code: 0, kind: SealKind::Whole, note: "被接受（不应发生）" },
        Err(f) => f,
    }
}

/// 判据入口（UNX-F21599-J1 及其配套十五条）。
///
/// 判据常量独立写死（40 / 240000 / 0x91092…0x91098），不复用被测常量名：若断言写成
/// `entries == DOC_ENTRIES`，把常量改成 41 时判据会跟着变，等于自证。
pub fn run_unx21599_threeseal_checks() -> crate::checks::CheckSet {
    let mut cs = crate::checks::CheckSet::new("unx-f21599");

    // 1) 夹具可构造（先证夹具，再证被测逻辑）。
    let standard = standard_book();
    cs.add("f21599.fixture_buildable", standard.is_ok(), "标准三印构造失败");

    // 2) 三印各自独立校验通过，且局部摘要互不相同（同摘要意味着咬合把印抹平了）。
    let mut ok = false;
    if let Ok(book) = &standard {
        let d = book.verify_doc();
        let l = book.verify_ledger();
        let c = book.verify_chain();
        ok = match (d, l, c) {
            (Ok(dd), Ok(ld), Ok(cd)) => {
                dd != 0 && ld != 0 && cd != 0 && dd != ld && ld != cd && dd != cd
            }
            _ => false,
        };
    }
    cs.add("f21599.seals_independent", ok, "三印未全部通过或局部摘要碰撞");

    // 3) 三印全绿 → 宣示成立，回执完整且落款在位。
    let mut ok = false;
    if let Ok(mut book) = standard_book() {
        if let Ok(p) = book.proclaim() {
            let ds = p.digests();
            ok = p.is_complete()
                && p.entries == 40
                && p.ledger_total == 240_000
                && p.chain_len == 40
                && p.tail_id == 21_600
                && ds[0] != ds[1]
                && book.is_proclaimed()
                && book.composite() == p.composite
                && p.text().contains("composite=")
                && p.text().contains("tail=21600");
        }
    }
    cs.add("f21599.proclaim_ok", ok, "三印全绿却宣示失败或回执不完整");

    // 4) 反向：文印缺失 → 0x91092 且点名 doc。
    let mut ok = false;
    if let Ok(book) = standard_book() {
        let mut broken = SealBook::from_seals(None, book.ledger, book.chain);
        let e = refusal(&mut broken);
        ok = e.code == 0x91092 && e.kind == SealKind::Doc && e.is_known();
    }
    cs.add("f21599.doc_missing", ok, "缺文印未被拒或码/印种不符");

    // 5) 反向：文印条目 39（做了但对不上）。
    let mut ok = false;
    if let Ok(book) = standard_book() {
        let bad = DocSeal { entries: 39, fingerprint: 0x9f21_0a11_5c3d_7e01 };
        let mut broken = SealBook::from_seals(Some(bad), book.ledger, book.chain);
        let e = refusal(&mut broken);
        ok = e.code == 0x91093 && e.kind == SealKind::Doc;
    }
    cs.add("f21599.doc_mismatch", ok, "文印条目 39 未被拒或码不符");

    // 6) 反向：账印缺失 → 0x91094。
    let mut ok = false;
    if let Ok(book) = standard_book() {
        let mut broken = SealBook::from_seals(book.doc, None, book.chain);
        let e = refusal(&mut broken);
        ok = e.code == 0x91094 && e.kind == SealKind::Ledger;
    }
    cs.add("f21599.ledger_missing", ok, "缺账印未被拒或码/印种不符");

    // 7) 反向：账印双路差 1 → 0x91095（账不平）。
    let mut ok = false;
    if let Ok(book) = standard_book() {
        let bad = LedgerSeal { total: LEDGER_TOTAL, route_a: LEDGER_TOTAL, route_b: LEDGER_TOTAL - 1 };
        let mut broken = SealBook::from_seals(book.doc, Some(bad), book.chain);
        let e = refusal(&mut broken);
        ok = e.code == 0x91095 && e.kind == SealKind::Ledger && !bad.is_balanced();
    }
    cs.add("f21599.ledger_diverge", ok, "账印双路差 1 未被拒或码不符");

    // 8) 反向：链印缺失 → 0x91096。
    let mut ok = false;
    if let Ok(book) = standard_book() {
        let mut broken = SealBook::from_seals(book.doc, book.ledger, None);
        let e = refusal(&mut broken);
        ok = e.code == 0x91096 && e.kind == SealKind::Chain;
    }
    cs.add("f21599.chain_missing", ok, "缺链印未被拒或码/印种不符");

    // 9) 反向：首尾哈希不同（链没闭合）→ 0x91097。
    let mut ok = false;
    if let Ok(book) = standard_book() {
        let bad = ChainSeal { length: CHAIN_LEN, head: 0x51ad_2c07_9b3e_0041, tail: 0x51ad_2c07_9b3e_0042, contiguous: true };
        let mut broken = SealBook::from_seals(book.doc, book.ledger, Some(bad));
        let e = refusal(&mut broken);
        ok = e.code == 0x91097 && e.kind == SealKind::Chain && !bad.is_closed();
    }
    cs.add("f21599.chain_not_closed", ok, "首尾不闭合未被拒或码不符");

    // 10) 反向：链长 39 → 0x91097（链断的另一种成因，与首尾不同）。
    let mut ok = false;
    if let Ok(book) = standard_book() {
        let bad = ChainSeal { length: 39, head: 0x51ad_2c07_9b3e_0041, tail: 0x51ad_2c07_9b3e_0041, contiguous: true };
        let mut broken = SealBook::from_seals(book.doc, book.ledger, Some(bad));
        let e = refusal(&mut broken);
        ok = e.code == 0x91097 && e.kind == SealKind::Chain;
    }
    cs.add("f21599.chain_short", ok, "链长 39 未被拒或码不符");

    // 11) 缺印清单：去掉文印与链印 → 恰报 [doc, chain]，顺序固定。
    let mut ok = false;
    if let Ok(book) = standard_book() {
        let broken = SealBook::from_seals(None, book.ledger, None);
        let m = broken.missing();
        ok = m.len() == 2
            && m[0] == SealKind::Doc
            && m[1] == SealKind::Chain
            && m[0].label() == "doc"
            && m[1].label() == "chain";
    }
    cs.add("f21599.missing_lists", ok, "缺印清单条数/顺序/标签不符");

    // 12) 边界：空册（三印全缺）→ 首因报文印，缺印清单三条齐。
    let ok = {
        let mut empty = SealBook::new();
        let e = refusal(&mut empty);
        e.code == 0x91092
            && empty.missing().len() == 3
            && empty.export().contains("doc=absent")
            && empty.export().contains("ledger=absent")
            && empty.export().contains("chain=absent")
            && empty.export().contains("missing=3")
            && empty.export().contains("composite=-")
    };
    cs.add("f21599.empty_book", ok, "空册未被拒或导出未标全缺");

    // 13) 拒绝必须留痕且不可宣示（缺印时 proclaimed 不能变 true）。
    let mut ok = false;
    if let Ok(book) = standard_book() {
        let mut broken = SealBook::from_seals(book.doc, None, book.chain);
        let e = refusal(&mut broken);
        ok = e.code == 0x91094
            && broken.rejects() == 1
            && broken.proclaim_count() == 0
            && !broken.is_proclaimed()
            && broken.composite() == 0
            && e.text().contains("账印缺失")
            && e.text().contains("seal=ledger");
    }
    cs.add("f21599.reject_traced", ok, "被拒后状态/计数/三要素文本不符");

    // 14) 幂等：重复宣示被拒，回执与合成指纹不变，导出文本可重放。
    let mut ok = false;
    if let Ok(mut book) = standard_book() {
        if book.proclaim().is_ok() {
            let before = book.composite();
            let again = book.proclaim();
            let text_a = book.export();
            let text_b = book.export();
            ok = again.as_ref().err().map(|f| f.code) == Some(0x91098)
                && again.as_ref().err().map(|f| f.kind) == Some(SealKind::Whole)
                && book.proclaim_count() == 1
                && book.rejects() == 1
                && book.composite() == before
                && before != 0
                && text_a == text_b
                && text_a.contains("proclaimed=1")
                && text_a.contains("count=1")
                && text_a.contains("rejects=1");
        }
    }
    cs.add(
        "f21599.proclaim_idempotent",
        ok,
        "重复宣示未被拒，或计数/合成指纹/导出文本不稳定",
    );

    // 15) 合成指纹对任一中途篡改敏感：改文印指纹，合成必变，账印与链印摘要不动。
    let mut ok = false;
    if let Ok(book) = standard_book() {
        let alt_doc = DocSeal { entries: DOC_ENTRIES, fingerprint: 0x9f21_0a11_5c3d_7e02 };
        let mut base = SealBook::from_seals(book.doc, book.ledger, book.chain);
        let mut alt = SealBook::from_seals(Some(alt_doc), book.ledger, book.chain);
        if let (Ok(p1), Ok(p2)) = (base.proclaim(), alt.proclaim()) {
            ok = p1.composite != p2.composite
                && p1.doc_digest != p2.doc_digest
                && p1.ledger_digest == p2.ledger_digest
                && p1.chain_digest == p2.chain_digest;
        }
    }
    cs.add("f21599.composite_sensitive", ok, "改文印后合成指纹未变或局部摘要串扰");

    // 16) 三印互不干扰：只缺链印时，文印与账印的校验器仍各自放行。
    let mut ok = false;
    if let Ok(book) = standard_book() {
        let broken = SealBook::from_seals(book.doc, book.ledger, None);
        ok = broken.verify_doc().is_ok()
            && broken.verify_ledger().is_ok()
            && broken.verify_chain().is_err();
    }
    cs.add("f21599.verifiers_isolated", ok, "缺链印干扰了文印/账印的独立校验");

    cs
}
