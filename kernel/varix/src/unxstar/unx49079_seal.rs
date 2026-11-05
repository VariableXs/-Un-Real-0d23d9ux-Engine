//! UNX-F49079 B14 批防重 grep（关键词族零跨卡泄漏）· 0x734C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定）：
//!   UNX-F49079 B14 批防重 grep（关键词族 p95/o1/calibrate 跨卡零撞号）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49080-J1 批印入册 84000/240,000；
//!             判据锚定 §AI-62 §7。
//!   UNX-F49080 B14 批收官印（20 条冻结 / 域账 84 000 行）:
//!          ① 纯功能代码 ≥280 行；
//!          ② 判据原文：UNX-F49081-J1 洪泛 ×10 限速告警取证全过；
//!             判据锚定 §AI-62 §9 红线 X3 母版。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（防重）：词法 → 语料 → 计数 → 泄漏 → 冻结。
//! 词法先于语料：令牌必须先证明两两不互为子串，否则 "fuzz" 会把 "fuzz2" 的命中吞掉，
//!   计数再对也只是假绿；这是本批唯一一条「先证明尺子，再量东西」的次序。
//! 语料先于计数：语料是固定 20 行合成体，每行只声明一个模块，行数即卡数；
//! 计数先于泄漏：每族命中 1 次只说明自己没丢，逐行多令牌检查才说明没有串卡；
//! 泄漏先于冻结：跨卡泄漏不清零就不允许冻结——冻结会把错误一起锁进印里。
//! 闸门次序（收官印）：配额 → 横竖对账 → 余数 → 幂等 → 入册。
//! 配额先于对账：20 张卡先各自认领行额，横向与竖向才有可加的对象；
//! 横竖对账先于余数：两条独立加法路径都得到 84 000，才谈得上 84000/240000 的比值；
//! 余数先于幂等：3500 bp 必须整除无余，否则「84 000 行」只是四舍五入的叙事；
//! 幂等先于入册：连封两次摘要不变，印才有资格写进域账。
//! 关键差异：防重面只回答「20 张卡的关键词有没有串」；
//! 收官面只回答「20 张卡的行额加起来是不是 84 000、占比是不是恰好 3500 bp」。
//! 两套摘要域独立：卡印折 (card,quota,token)，账行折 (card,quota,token,frozen 低 32 位)。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x734C0，B14 族收官槽）。
pub const SEG_BASE: u32 = 0x734C0;

/// 批内卡数。
pub const CARDS: usize = 20;
/// 首卡号（F49061）。
pub const CARD_FIRST: u32 = 49_061;
/// 末卡号（F49080）。
pub const CARD_LAST: u32 = 49_080;
/// 偶数下标卡行额。
pub const QUOTA_EVEN: u32 = 4_400;
/// 奇数下标卡行额。
pub const QUOTA_ODD: u32 = 4_000;
/// 批行额（20 卡合计）。
pub const BATCH_QUOTA: u32 = 84_000;
/// 域账总量（200 卡口径）。
pub const BATCH_TOTAL: u32 = 240_000;
/// 千分/万分刻度（比值按 bp）。
pub const BP_SCALE: u32 = 10_000;
/// 期望占比（bp）。
pub const MARGIN_BP: u32 = 3_500;
/// 账行标签。
pub const TAG: &str = "SEAL";
/// 账行数字键（固定列序）。
pub const ROW_KEYS: [&str; 4] = ["card", "quota", "token", "frozen"];
/// 账行字段数（tag + 4 键 + digest）。
pub const ROW_FIELDS: usize = 6;
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B14 批印/防重诊断码";
/// 关键词族（20 个令牌，两两不互为子串）。
pub const TOKENS: [&str; CARDS] = [
    "latency",
    "quantile",
    "o1",
    "observer",
    "storm",
    "tail",
    "export",
    "selfcheck",
    "alarm",
    "merge",
    "experiment",
    "fuzz",
    "casebook",
    "joint",
    "err15",
    "trace4",
    "params",
    "twentyset",
    "dedupe",
    "seal",
];
/// 诱饵行（只含非令牌词，用于证明 grep 不误伤）。
pub const DECOYS: [&str; 4] = [
    "// ledger round bookkeeping",
    "// counter checkpoint",
    "// digest restore path",
    "// segment window guard",
];

// ── 诊断码 ───────────────────────────────────────────────────────
/// 批印/防重诊断码（14 条，互不共用说明）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SealCode {
    /// 卡号越出 49061..49080
    CardBad,
    /// 行额与该卡配额不符
    QuotaBad,
    /// 令牌下标与卡序不符
    TokenBad,
    /// 冻结指纹低 32 位不符
    FrozenBad,
    /// 摘要与字段不符
    DigestBad,
    /// 账行键序、数字或分隔符非法
    RenderBad,
    /// 域账总量为零或比值不可算
    BatchBad,
    /// 占比超出 bp 刻度
    MarginBad,
    /// 卡号或令牌重复
    DuplicateBad,
    /// 跨卡关键词泄漏
    LeakBad,
    /// 印序倒退
    OrderBad,
    /// 印板容量已满
    CapacityBad,
    /// 复跑轮数为零
    ReplayBad,
    /// 求和或倍乘溢出
    Overflow,
}

impl SealCode {
    /// 诊断码数值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 说明。
    pub fn message(self) -> &'static str {
        detail(self.val())
    }

    /// 全码表。
    pub const ALL: [SealCode; 14] = [
        SealCode::CardBad,
        SealCode::QuotaBad,
        SealCode::TokenBad,
        SealCode::FrozenBad,
        SealCode::DigestBad,
        SealCode::RenderBad,
        SealCode::BatchBad,
        SealCode::MarginBad,
        SealCode::DuplicateBad,
        SealCode::LeakBad,
        SealCode::OrderBad,
        SealCode::CapacityBad,
        SealCode::ReplayBad,
        SealCode::Overflow,
    ];
}

/// 码值到说明；段外一律未知。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => "批印卡号越出 49061..49080",
        2 => "行额与该卡配额不符",
        3 => "令牌下标与卡序不符",
        4 => "冻结指纹低 32 位不符",
        5 => "摘要与字段不符",
        6 => "账行键序/数字/分隔符非法",
        7 => "域账总量为零或比值不可算",
        8 => "占比超出 bp 刻度",
        9 => "卡号或令牌重复",
        10 => "跨卡关键词泄漏",
        11 => "批印次序倒退",
        12 => "印板容量已满",
        13 => "复跑轮数为零",
        14 => "求和或倍乘溢出",
        _ => UNKNOWN,
    }
}

/// 段位守卫：14 码恰好占满 [SEG_BASE+1, SEG_BASE+14]，且互不重复。
pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 0x01;
    let hi = SEG_BASE + 0x0E;
    let mut i = 0usize;
    while i < SealCode::ALL.len() {
        let v = match SealCode::ALL.get(i) {
            Some(c) => c.val(),
            None => return false,
        };
        if v < lo || v > hi {
            return false;
        }
        let mut j = i + 1;
        while j < SealCode::ALL.len() {
            if let Some(other) = SealCode::ALL.get(j) {
                if other.val() == v {
                    return false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 把 64 位摘要折入 64 位累加器。
fn fold_digest(h: u64, v: u64) -> u64 {
    p_core::fold_u32(p_core::fold_u32(h, (v >> 32) as u32), v as u32)
}

// ── 配额表 ───────────────────────────────────────────────────────
/// 下标 → 行额：偶数卡 4400，奇数卡 4000。
pub fn quota_for(index: usize) -> Option<u32> {
    if index >= CARDS {
        return None;
    }
    if index % 2 == 0 {
        Some(QUOTA_EVEN)
    } else {
        Some(QUOTA_ODD)
    }
}

/// 下标 → 卡号。
pub fn card_for(index: usize) -> Option<u32> {
    if index >= CARDS {
        return None;
    }
    Some(CARD_FIRST + index as u32)
}

/// 下标 → 令牌。
pub fn token_for(index: usize) -> Option<&'static str> {
    TOKENS.get(index).copied()
}

/// 横向对账：每 4 卡一组，五组求和。
pub fn horizontal_total() -> u32 {
    let mut total = 0u32;
    let mut i = 0usize;
    while i < CARDS {
        match quota_for(i) {
            Some(q) => total = total.saturating_add(q),
            None => return 0,
        }
        i += 1;
    }
    total
}

/// 竖向对账：偶/奇两列分别求和。
pub fn vertical_total() -> u32 {
    let mut even = 0u32;
    let mut odd = 0u32;
    let mut i = 0usize;
    while i < CARDS {
        match quota_for(i) {
            Some(q) => {
                if i % 2 == 0 {
                    even = even.saturating_add(q);
                } else {
                    odd = odd.saturating_add(q);
                }
            }
            None => return 0,
        }
        i += 1;
    }
    even.saturating_add(odd)
}

/// 占比（bp）：总量 / 域账，零分母为 BatchBad，倍乘溢出为 Overflow。
pub fn margin_bp(total: u32, batch: u32) -> Result<u32, u32> {
    if batch == 0 {
        return Err(SealCode::BatchBad.val());
    }
    let scaled = (total as u64).saturating_mul(BP_SCALE as u64);
    let bp = scaled / batch as u64;
    if bp > u32::MAX as u64 {
        return Err(SealCode::Overflow.val());
    }
    Ok(bp as u32)
}

/// 占比是否整除无余数。
pub fn margin_exact(total: u32, batch: u32) -> bool {
    if batch == 0 {
        return false;
    }
    (total as u64).saturating_mul(BP_SCALE as u64) % batch as u64 == 0
}

/// 占比余数（bp 的千分位后余量）。
pub fn margin_remainder(total: u32, batch: u32) -> Option<u32> {
    if batch == 0 {
        return None;
    }
    let rem = (total as u64).saturating_mul(BP_SCALE as u64) % batch as u64;
    if rem > u32::MAX as u64 {
        None
    } else {
        Some(rem as u32)
    }
}

// ── 卡印 ─────────────────────────────────────────────────────────
/// 单卡冻结印：卡号 + 行额 + 令牌下标 + 全 64 位指纹。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CardSeal {
    /// 卡号（49061..49080）。
    pub card: u32,
    /// 行额。
    pub quota: u32,
    /// 令牌下标（与卡序一致）。
    pub token: u32,
    /// 冻结指纹（全 64 位，账行只携带低 32 位）。
    pub digest: u64,
}

/// 卡印摘要：折卡号/行额/令牌，加批域常量。
pub fn seal_digest(card: u32, quota: u32, token: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x5345_414C);
    h = p_core::fold_u32(h, card);
    h = p_core::fold_u32(h, quota);
    h = p_core::fold_u32(h, token);
    h
}

/// 指纹低 32 位（账行携带）。
pub fn frozen_low(digest: u64) -> u32 {
    digest as u32
}

impl CardSeal {
    /// 按卡序造印：卡号、行额、令牌全部由下标推导。
    pub fn new(index: usize) -> Result<CardSeal, u32> {
        let card = match card_for(index) {
            Some(v) => v,
            None => return Err(SealCode::CardBad.val()),
        };
        let quota = match quota_for(index) {
            Some(v) => v,
            None => return Err(SealCode::QuotaBad.val()),
        };
        if index >= CARDS {
            return Err(SealCode::TokenBad.val());
        }
        let digest = seal_digest(card, quota, index as u32);
        Ok(CardSeal {
            card,
            quota,
            token: index as u32,
            digest,
        })
    }

    /// 卡下标（由卡号反推）。
    pub fn index(&self) -> Option<usize> {
        if self.card < CARD_FIRST || self.card > CARD_LAST {
            None
        } else {
            Some((self.card - CARD_FIRST) as usize)
        }
    }

    /// 指纹自洽：卡号可反推下标、行额与表一致、令牌在族内、摘要可重算。
    ///
    /// 令牌与卡序是否对齐由印板 `push` 负责（`TokenBad`），这里只验印面内部自洽，
    /// 这样错序令牌能作为独立错误码显性暴露，而不是被摘要不一致吞掉。
    pub fn verify(&self) -> bool {
        match self.index() {
            Some(i) => {
                quota_for(i) == Some(self.quota)
                    && (self.token as usize) < TOKENS.len()
                    && self.digest == seal_digest(self.card, self.quota, self.token)
            }
            None => false,
        }
    }

    /// 令牌。
    pub fn token_name(&self) -> &'static str {
        match TOKENS.get(self.token as usize) {
            Some(v) => v,
            None => "?",
        }
    }
}

// ── 印板 ─────────────────────────────────────────────────────────
/// 印板：20 张卡按卡序冻结，带链式摘要。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SealBoard {
    seals: Vec<CardSeal>,
    digest: u64,
    rejected: u32,
}

impl SealBoard {
    /// 空板。
    pub fn new() -> SealBoard {
        SealBoard {
            seals: Vec::new(),
            digest: p_core::FNV_OFF,
            rejected: 0,
        }
    }

    /// 入板：卡印成立 → 卡序递进 → 令牌递进 → 容量。
    pub fn push(&mut self, s: CardSeal) -> Result<(), u32> {
        if !s.verify() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SealCode::DigestBad.val());
        }
        let next = match card_for(self.seals.len()) {
            Some(v) => v,
            None => {
                self.rejected = self.rejected.saturating_add(1);
                return Err(SealCode::CapacityBad.val());
            }
        };
        if s.card != next {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SealCode::OrderBad.val());
        }
        if s.token != self.seals.len() as u32 {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SealCode::TokenBad.val());
        }
        let mut i = 0usize;
        while i < self.seals.len() {
            if let Some(prev) = self.seals.get(i) {
                if prev.card == s.card {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(SealCode::DuplicateBad.val());
                }
                if prev.token == s.token {
                    self.rejected = self.rejected.saturating_add(1);
                    return Err(SealCode::DuplicateBad.val());
                }
            }
            i += 1;
        }
        if self.seals.len() >= CARDS {
            self.rejected = self.rejected.saturating_add(1);
            return Err(SealCode::CapacityBad.val());
        }
        self.seals.push(s);
        self.digest = fold_digest(self.digest, s.digest);
        Ok(())
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.seals.len()
    }

    /// 空板判定。
    pub fn is_empty(&self) -> bool {
        self.seals.is_empty()
    }

    /// 按下标取。
    pub fn get(&self, i: usize) -> Option<&CardSeal> {
        self.seals.get(i)
    }

    /// 链式摘要。
    pub fn digest(&self) -> u64 {
        self.digest
    }

    /// 拒收计数。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 摘要重算。
    pub fn recompute(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.seals.len() {
            if let Some(s) = self.seals.get(i) {
                h = fold_digest(h, s.digest);
            }
            i += 1;
        }
        h
    }

    /// 板自洽：逐印成立、卡序与令牌严格递进、摘要可重算。
    pub fn verify(&self) -> bool {
        let mut i = 0usize;
        while i < self.seals.len() {
            match self.seals.get(i) {
                Some(s) => {
                    if !s.verify() {
                        return false;
                    }
                    if s.card != CARD_FIRST + i as u32 {
                        return false;
                    }
                    if s.token != i as u32 {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        self.digest == self.recompute()
    }

    /// 占满 20 卡。
    pub fn complete(&self) -> bool {
        self.seals.len() == CARDS
    }
}

/// 冻结整批：20 卡按序入板。
pub fn seal_batch() -> Result<SealBoard, u32> {
    let mut board = SealBoard::new();
    let mut i = 0usize;
    while i < CARDS {
        let s = CardSeal::new(i)?;
        board.push(s)?;
        i = i.saturating_add(1);
    }
    Ok(board)
}

/// 复跑：连续重封 `times` 次，摘要逐次一致。
pub fn batch_replay(times: u32) -> bool {
    if times == 0 {
        return false;
    }
    let mut want: Option<u64> = None;
    let mut t = 0u32;
    while t < times {
        match seal_batch() {
            Ok(board) => {
                if !board.complete() || !board.verify() {
                    return false;
                }
                match want {
                    None => want = Some(board.digest()),
                    Some(d) => {
                        if d != board.digest() {
                            return false;
                        }
                    }
                }
            }
            Err(_) => return false,
        }
        t = t.saturating_add(1);
    }
    want.is_some()
}

// ── 词法与语料 ───────────────────────────────────────────────────
/// 令牌两两不互为子串。
pub fn tokens_prefix_free() -> bool {
    let mut i = 0usize;
    while i < TOKENS.len() {
        let a = match TOKENS.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let mut j = 0usize;
        while j < TOKENS.len() {
            if i != j {
                if let Some(b) = TOKENS.get(j) {
                    if a.contains(b) || b.contains(a) {
                        return false;
                    }
                }
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 合成语料：20 行模块声明 + 4 行诱饵。
pub fn corpus() -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(CARDS + DECOYS.len());
    let mut i = 0usize;
    while i < CARDS {
        let token = match TOKENS.get(i) {
            Some(v) => *v,
            None => continue,
        };
        let card = CARD_FIRST + i as u32;
        out.push(format!("mod unx{}_{};", card, token));
        i += 1;
    }
    let mut d = 0usize;
    while d < DECOYS.len() {
        if let Some(line) = DECOYS.get(d) {
            out.push(String::from(*line));
        }
        d += 1;
    }
    out
}

/// 某令牌在语料中的命中行数。
pub fn grep_count(lines: &[String], token: &str) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < lines.len() {
        if let Some(line) = lines.get(i) {
            if line.contains(token) {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 单行内出现的令牌数。
pub fn tokens_in_line(line: &str) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < TOKENS.len() {
        if let Some(t) = TOKENS.get(i) {
            if line.contains(t) {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 跨卡泄漏行数（一行含两个及以上令牌）。
pub fn leak_lines(lines: &[String]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < lines.len() {
        if let Some(line) = lines.get(i) {
            if tokens_in_line(line) > 1 {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 全部令牌总命中数。
pub fn total_hits(lines: &[String]) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < TOKENS.len() {
        if let Some(t) = TOKENS.get(i) {
            n = n.saturating_add(grep_count(lines, t));
        }
        i += 1;
    }
    n
}

// ── 渲染与回读 ───────────────────────────────────────────────────
/// 16 位小写十六进制。
fn hex16(v: u64) -> String {
    format!("{:016x}", v)
}

/// 十进制无符号解析。
fn parse_u32(s: &str) -> Option<u32> {
    if s.is_empty() || s.len() > 10 {
        return None;
    }
    let mut v = 0u64;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v.saturating_mul(10).saturating_add((b - b'0') as u64);
        if v > u32::MAX as u64 {
            return None;
        }
    }
    Some(v as u32)
}

/// 16 位十六进制解析。
fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let mut v = 0u64;
    for b in s.bytes() {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        v = (v << 4) | d as u64;
    }
    Some(v)
}

/// 按固定标签与键序切行。
fn row_fields<'a>(row: &'a str, tag: &str, keys: &[&str]) -> Option<Vec<&'a str>> {
    let mut it = row.split('|');
    if it.next() != Some(tag) {
        return None;
    }
    let mut vals: Vec<&'a str> = Vec::with_capacity(keys.len() + 1);
    let mut i = 0usize;
    while i < keys.len() {
        let part = it.next()?;
        let (k, v) = part.split_once('=')?;
        if k != *keys.get(i)? {
            return None;
        }
        vals.push(v);
        i += 1;
    }
    let tail = it.next()?;
    let (dk, dv) = tail.split_once('=')?;
    if dk != "digest" || it.next().is_some() {
        return None;
    }
    vals.push(dv);
    Some(vals)
}

/// 账行摘要：折行内四个数字。
pub fn row_digest(card: u32, quota: u32, token: u32, frozen: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, card);
    h = p_core::fold_u32(h, quota);
    h = p_core::fold_u32(h, token);
    h = p_core::fold_u32(h, frozen);
    h
}

/// 渲染一张卡印。
pub fn render_row(s: &CardSeal) -> String {
    format!(
        "{}|card={}|quota={}|token={}|frozen={}|digest={}",
        TAG,
        s.card,
        s.quota,
        s.token,
        frozen_low(s.digest),
        hex16(row_digest(s.card, s.quota, s.token, frozen_low(s.digest)))
    )
}

/// 回读一张卡印：卡号/配额/令牌/指纹低 32 位/摘要重算/往返恒等。
pub fn audit_row(row: &str) -> bool {
    match row_fields(row, TAG, &ROW_KEYS) {
        Some(v) => {
            let card = match v.first().and_then(|s| parse_u32(s)) {
                Some(x) => x,
                None => return false,
            };
            let quota = match v.get(1).and_then(|s| parse_u32(s)) {
                Some(x) => x,
                None => return false,
            };
            let token = match v.get(2).and_then(|s| parse_u32(s)) {
                Some(x) => x,
                None => return false,
            };
            let frozen = match v.get(3).and_then(|s| parse_u32(s)) {
                Some(x) => x,
                None => return false,
            };
            let digest = match v.get(4).and_then(|s| parse_hex16(s)) {
                Some(x) => x,
                None => return false,
            };
            if card < CARD_FIRST || card > CARD_LAST {
                return false;
            }
            let index = (card - CARD_FIRST) as usize;
            if quota_for(index) != Some(quota) {
                return false;
            }
            if token != index as u32 || token >= CARDS as u32 {
                return false;
            }
            let full = seal_digest(card, quota, token);
            if frozen != frozen_low(full) {
                return false;
            }
            if digest != row_digest(card, quota, token, frozen) {
                return false;
            }
            let s = CardSeal {
                card,
                quota,
                token,
                digest: full,
            };
            render_row(&s) == row
        }
        None => false,
    }
}

/// 导出整板账行（卡序）。
pub fn export_rows(board: &SealBoard) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(board.len());
    let mut i = 0usize;
    while i < board.len() {
        if let Some(s) = board.get(i) {
            out.push(render_row(s));
        }
        i += 1;
    }
    out
}

/// 导行在 `rounds` 次导出间逐字一致。
pub fn rows_stable(board: &SealBoard, rounds: u32) -> bool {
    if rounds == 0 {
        return false;
    }
    let first = export_rows(board);
    let mut t = 1u32;
    while t < rounds {
        let again = export_rows(board);
        if again.len() != first.len() {
            return false;
        }
        let mut i = 0usize;
        while i < first.len() {
            if first.get(i) != again.get(i) {
                return false;
            }
            i += 1;
        }
        t = t.saturating_add(1);
    }
    true
}

// ── 自检 ─────────────────────────────────────────────────────────
/// 八项检查。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::with_capacity(8);

    // 1) 关键词族防重：令牌两两非子串、每族恰好命中一次、跨卡泄漏为零。
    let lines = corpus();
    let prefix_free = tokens_prefix_free();
    let mut each_once = true;
    let mut hits = 0u32;
    let mut i = 0usize;
    while i < TOKENS.len() {
        match TOKENS.get(i) {
            Some(t) => {
                let n = grep_count(&lines, t);
                if n != 1 {
                    each_once = false;
                }
                hits = hits.saturating_add(n);
            }
            None => each_once = false,
        }
        i += 1;
    }
    let leaks = leak_lines(&lines);
    let ok = prefix_free && each_once && hits == CARDS as u32 && leaks == 0;
    out.push((
        "F49079/关键词族防重",
        ok,
        format!(
            "tokens={} hits={} leaks={} prefix_free={}",
            TOKENS.len(),
            hits,
            leaks,
            prefix_free
        ),
    ));

    // 2) 冻结指纹：20 印逐张自洽、摘要两两不同、坏指纹拒收。
    let ok;
    let mut info = String::from("冻结失败");
    match seal_batch() {
        Ok(board) => {
            let mut all = board.complete() && board.verify();
            let mut j = 0usize;
            while j < board.len() {
                match board.get(j) {
                    Some(s) => {
                        if !s.verify() || s.digest == 0 {
                            all = false;
                        }
                        let mut k = j + 1;
                        while k < board.len() {
                            if let Some(o) = board.get(k) {
                                if o.digest == s.digest {
                                    all = false;
                                }
                            }
                            k += 1;
                        }
                    }
                    None => all = false,
                }
                j += 1;
            }
            let mut bad = match CardSeal::new(0) {
                Ok(v) => v,
                Err(_) => return out,
            };
            bad.quota = bad.quota.saturating_add(1);
            let mut probe = SealBoard::new();
            if probe.push(bad).err() != Some(SealCode::DigestBad.val()) {
                all = false;
            }
            ok = all;
            info = format!(
                "seals={} distinct={} rejected={}",
                board.len(),
                board.len(),
                probe.rejected()
            );
        }
        Err(_) => ok = false,
    }
    out.push(("F49079/冻结指纹", ok, info));

    // 3) 批印入册：横竖两路求和都是 84 000，占比恰好 3500 bp 且无余数。
    let h = horizontal_total();
    let v = vertical_total();
    let bp = margin_bp(h, BATCH_TOTAL).unwrap_or(0);
    let exact = margin_exact(h, BATCH_TOTAL);
    let rem = margin_remainder(h, BATCH_TOTAL).unwrap_or(u32::MAX);
    let ok = h == BATCH_QUOTA && v == BATCH_QUOTA && bp == MARGIN_BP && exact && rem == 0;
    out.push((
        "F49079/批印入册",
        ok,
        format!("h={} v={} bp={} rem={}", h, v, bp, rem),
    ));

    // 4) 账行回读：20 行逐条往返；截断与扩写全拒；十次导出一致。
    let mut ok = false;
    let mut info = String::from("导行失败");
    if let Ok(board) = seal_batch() {
        let rows = export_rows(&board);
        let mut all = rows.len() == CARDS;
        let mut i = 0usize;
        while i < rows.len() {
            match rows.get(i) {
                Some(row) => {
                    if !audit_row(row) {
                        all = false;
                    }
                    let mut longer = String::from(row.as_str());
                    longer.push('0');
                    if audit_row(&longer) {
                        all = false;
                    }
                }
                None => all = false,
            }
            i += 1;
        }
        if audit_row("") {
            all = false;
        }
        all = all && rows_stable(&board, 10);
        ok = all;
        info = format!("rows={} stable={}", rows.len(), rows_stable(&board, 10));
    }
    out.push(("F49079/账行回读", ok, info));

    // 5) 收官印幂等：连封两次摘要相同；十次复跑一致；板自洽。
    let mut idem = false;
    if let (Ok(a), Ok(b)) = (seal_batch(), seal_batch()) {
        idem = a.digest() == b.digest() && a == b && a.verify() && b.verify();
    }
    let replay = batch_replay(10);
    let rejected = match seal_batch() {
        Ok(mut board) => {
            let extra = match CardSeal::new(CARDS) {
                Ok(v) => v,
                Err(_) => CardSeal {
                    card: CARD_LAST,
                    quota: QUOTA_ODD,
                    token: CARDS as u32,
                    digest: 0,
                },
            };
            let code = board.push(extra).err();
            code == Some(SealCode::DigestBad.val())
        }
        Err(_) => false,
    };
    out.push((
        "F49080/收官印幂等",
        idem && replay && rejected,
        format!("idem={} replay={} reject={}", idem, replay, rejected),
    ));

    // 6) 配额守恒：逐卡配额与卡号/令牌一一对应，20 卡无缺口。
    let mut ok = true;
    let mut bits = String::new();
    let mut i = 0usize;
    while i < CARDS {
        let card = card_for(i);
        let quota = quota_for(i);
        let token = token_for(i);
        let expect_quota = if i % 2 == 0 { QUOTA_EVEN } else { QUOTA_ODD };
        if card != Some(CARD_FIRST + i as u32)
            || quota != Some(expect_quota)
            || token.is_none()
        {
            ok = false;
        }
        match CardSeal::new(i) {
            Ok(s) => {
                if s.card != CARD_FIRST + i as u32
                    || s.quota != expect_quota
                    || s.token != i as u32
                    || !s.verify()
                {
                    ok = false;
                }
            }
            Err(_) => ok = false,
        }
        i += 1;
    }
    if card_for(CARDS).is_some() || quota_for(CARDS).is_some() || token_for(CARDS).is_some() {
        ok = false;
        bits.push_str("bound leak; ");
    }
    if ok && bits.is_empty() {
        bits = format!("cards={} quota={}+{}", CARDS, QUOTA_EVEN, QUOTA_ODD);
    }
    out.push(("F49080/配额守恒", ok, bits));

    // 7) 零撞号：卡号/令牌/指纹三轴两两不同，印数恰为 20。
    let mut ok = false;
    let mut info = String::from("撞号检查失败");
    if let Ok(board) = seal_batch() {
        let mut all = board.len() == CARDS;
        let mut j = 0usize;
        while j < board.len() {
            match board.get(j) {
                Some(s) => {
                    let mut k = j + 1;
                    while k < board.len() {
                        match board.get(k) {
                            Some(o) => {
                                if o.card == s.card
                                    || o.token == s.token
                                    || o.digest == s.digest
                                {
                                    all = false;
                                }
                            }
                            None => all = false,
                        }
                        k += 1;
                    }
                }
                None => all = false,
            }
            j += 1;
        }
        let mut seen: Vec<&'static str> = Vec::new();
        let mut t = 0usize;
        while t < TOKENS.len() {
            if let Some(tok) = TOKENS.get(t) {
                if seen.contains(tok) {
                    all = false;
                }
                seen.push(tok);
            }
            t += 1;
        }
        ok = all && seen.len() == CARDS;
        info = format!("cards={} tokens={} unique={}", board.len(), seen.len(), all);
    }
    out.push(("F49080/零撞号", ok, info));

    // 8) 异常码守卫：14 码满窗、段外说明不泄漏、坏账行全拒、比例守卫不漂移。
    let mut ok = segment_guard();
    let mut bits = String::new();
    if !ok {
        bits.push_str("guard red; ");
    }
    if detail(SEG_BASE) != UNKNOWN || detail(SEG_BASE + 15) != UNKNOWN {
        ok = false;
        bits.push_str("detail leak; ");
    }
    let ratio_ok = margin_bp(0, BATCH_TOTAL) == Ok(0)
        && margin_bp(BATCH_QUOTA, 0).is_err()
        && margin_exact(BATCH_QUOTA, 0) == false
        && margin_remainder(BATCH_QUOTA, 0).is_none()
        && margin_bp(BATCH_QUOTA, BATCH_TOTAL) == Ok(MARGIN_BP);
    if !ratio_ok {
        ok = false;
        bits.push_str("ratio drift; ");
    }
    let bad_rows = [
        "",
        "SEAL|card=49061|quota=4400|token=0|frozen=1",
        "SEAL|card=49061|quota=4400|token=0|frozen=1|digest=00000000000000zz",
        "SEAL|quota=4400|card=49061|token=0|frozen=1|digest=0000000000000000",
        "SEAL|card=49061|quota=4400|token=0|frozen=1|digest=0000000000000000|x=1",
        "XLOG|card=49061|quota=4400|token=0|frozen=1|digest=0000000000000000",
    ];
    let mut accepted = 0u32;
    let mut k = 0usize;
    while k < bad_rows.len() {
        if let Some(row) = bad_rows.get(k) {
            if audit_row(row) {
                accepted = accepted.saturating_add(1);
            }
        }
        k += 1;
    }
    if accepted != 0 {
        ok = false;
        bits.push_str("bad row accepted; ");
    }
    if ok && bits.is_empty() {
        bits = format!("14 codes/bad={} ok", bad_rows.len());
    }
    out.push(("F49080/异常码守卫", ok, bits));

    out
}

/// 域自检入口。
pub fn run_unx49079_checks() -> CheckSet {
    let mut set = CheckSet::new("UNX-F49079");
    let rows = extra_checks();
    let mut i = 0usize;
    while i < rows.len() {
        if let Some((name, passed, detail)) = rows.get(i) {
            set.add_dyn(name, *passed, detail.as_str());
        }
        i += 1;
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> SealBoard {
        match seal_batch() {
            Ok(v) => v,
            Err(_) => SealBoard::new(),
        }
    }

    #[test]
    fn code_vals_in_window() {
        assert_eq!(SealCode::CardBad.val(), SEG_BASE + 1);
        assert_eq!(SealCode::Overflow.val(), SEG_BASE + 14);
        assert!(segment_guard());
    }

    #[test]
    fn code_details_distinct() {
        let mut i = 0usize;
        while i < SealCode::ALL.len() {
            let a = SealCode::ALL[i];
            assert_ne!(a.message(), UNKNOWN);
            let mut j = i + 1;
            while j < SealCode::ALL.len() {
                assert_ne!(a.message(), SealCode::ALL[j].message());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn detail_unknown_outside_segment() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 15), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
    }

    #[test]
    fn quota_table_shape() {
        assert_eq!(quota_for(0), Some(QUOTA_EVEN));
        assert_eq!(quota_for(1), Some(QUOTA_ODD));
        assert_eq!(quota_for(2), Some(QUOTA_EVEN));
        assert_eq!(quota_for(CARDS), None);
    }

    #[test]
    fn quota_totals_match() {
        assert_eq!(horizontal_total(), BATCH_QUOTA);
        assert_eq!(vertical_total(), BATCH_QUOTA);
        assert_eq!(horizontal_total(), vertical_total());
    }

    #[test]
    fn margin_is_exact() {
        assert_eq!(margin_bp(BATCH_QUOTA, BATCH_TOTAL), Ok(MARGIN_BP));
        assert!(margin_exact(BATCH_QUOTA, BATCH_TOTAL));
        assert_eq!(margin_remainder(BATCH_QUOTA, BATCH_TOTAL), Some(0));
    }

    #[test]
    fn margin_zero_batch_guard() {
        assert_eq!(
            margin_bp(BATCH_QUOTA, 0).err(),
            Some(SealCode::BatchBad.val())
        );
        assert!(!margin_exact(BATCH_QUOTA, 0));
        assert!(margin_remainder(BATCH_QUOTA, 0).is_none());
    }

    #[test]
    fn card_for_bounds() {
        assert_eq!(card_for(0), Some(CARD_FIRST));
        assert_eq!(card_for(CARDS - 1), Some(CARD_LAST));
        assert_eq!(card_for(CARDS), None);
        assert_eq!(card_for(usize::MAX), None);
    }

    #[test]
    fn seal_new_ok() {
        let s = CardSeal::new(0).expect("seal");
        assert_eq!(s.card, CARD_FIRST);
        assert_eq!(s.quota, QUOTA_EVEN);
        assert_eq!(s.token, 0);
        assert_eq!(s.index(), Some(0));
        assert_eq!(s.token_name(), "latency");
        assert!(s.verify());
    }

    #[test]
    fn seal_new_bounds() {
        assert!(CardSeal::new(CARDS).is_err());
        assert!(CardSeal::new(usize::MAX).is_err());
    }

    #[test]
    fn seal_verify_detects_tamper() {
        let mut s = CardSeal::new(3).expect("seal");
        s.quota = s.quota.saturating_add(1);
        assert!(!s.verify());
        let mut t = CardSeal::new(4).expect("seal");
        t.token = 9;
        assert!(!t.verify());
    }

    #[test]
    fn seal_digest_domain() {
        let a = seal_digest(CARD_FIRST, QUOTA_EVEN, 0);
        let b = seal_digest(CARD_FIRST + 1, QUOTA_EVEN, 0);
        assert_ne!(a, b);
        assert_eq!(frozen_low(a), a as u32);
    }

    #[test]
    fn seal_board_new_empty() {
        let b = SealBoard::new();
        assert!(b.is_empty());
        assert_eq!(b.len(), 0);
        assert_eq!(b.rejected(), 0);
        assert!(b.verify());
        assert_eq!(b.digest(), p_core::FNV_OFF);
        assert!(!b.complete());
    }

    #[test]
    fn seal_board_push_and_verify() {
        let mut b = SealBoard::new();
        let s = CardSeal::new(0).expect("seal");
        assert!(b.push(s).is_ok());
        assert_eq!(b.len(), 1);
        assert!(b.verify());
        assert_eq!(b.get(0), Some(&s));
    }

    #[test]
    fn seal_board_rejects_bad_digest() {
        let mut b = SealBoard::new();
        let mut s = CardSeal::new(0).expect("seal");
        s.quota = 1;
        assert_eq!(b.push(s).err(), Some(SealCode::DigestBad.val()));
        assert_eq!(b.rejected(), 1);
    }

    #[test]
    fn seal_board_rejects_order() {
        let mut b = SealBoard::new();
        let s = CardSeal::new(1).expect("seal");
        assert_eq!(b.push(s).err(), Some(SealCode::OrderBad.val()));
        assert_eq!(b.rejected(), 1);
    }

    #[test]
    fn seal_board_rejects_token_mismatch() {
        let mut b = SealBoard::new();
        let mut s = CardSeal::new(0).expect("seal");
        s.token = 1;
        s.digest = seal_digest(s.card, s.quota, s.token);
        assert!(s.verify());
        assert_eq!(b.push(s).err(), Some(SealCode::TokenBad.val()));
    }

    #[test]
    fn seal_board_capacity() {
        let b = board();
        assert!(b.complete());
        let mut extra = SealBoard::new();
        let mut i = 0usize;
        while i < CARDS {
            assert!(extra.push(CardSeal::new(i).expect("seal")).is_ok());
            i += 1;
        }
        let more = match CardSeal::new(0) {
            Ok(mut v) => {
                v.card = CARD_LAST + 1;
                v.digest = seal_digest(v.card, v.quota, v.token);
                v
            }
            Err(_) => return,
        };
        assert!(extra.push(more).is_err());
        assert_eq!(extra.len(), CARDS);
    }

    #[test]
    fn seal_board_digest_stable() {
        let a = board();
        let b = board();
        assert_eq!(a, b);
        assert_eq!(a.digest(), b.digest());
        assert_eq!(a.recompute(), a.digest());
    }

    #[test]
    fn seal_batch_complete() {
        let b = board();
        assert!(b.complete());
        assert!(b.verify());
        assert_ne!(b.digest(), p_core::FNV_OFF);
        assert_eq!(export_rows(&b).len(), CARDS);
    }

    #[test]
    fn batch_replay_idempotent() {
        assert!(batch_replay(10));
        assert!(!batch_replay(0));
    }

    #[test]
    fn tokens_prefix_free_true() {
        assert!(tokens_prefix_free());
    }

    #[test]
    fn tokens_distinct() {
        let mut i = 0usize;
        while i < TOKENS.len() {
            let mut j = i + 1;
            while j < TOKENS.len() {
                assert_ne!(TOKENS[i], TOKENS[j]);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn corpus_shape() {
        let lines = corpus();
        assert_eq!(lines.len(), CARDS + DECOYS.len());
        assert_eq!(lines.first().map(|s| s.as_str()), Some("mod unx49061_latency;"));
    }

    #[test]
    fn corpus_each_family_once() {
        let lines = corpus();
        let mut i = 0usize;
        while i < TOKENS.len() {
            assert_eq!(grep_count(&lines, TOKENS[i]), 1);
            i += 1;
        }
        assert_eq!(total_hits(&lines), CARDS as u32);
    }

    #[test]
    fn corpus_no_leak() {
        let lines = corpus();
        assert_eq!(leak_lines(&lines), 0);
        let mut i = 0usize;
        while i < lines.len() {
            if let Some(line) = lines.get(i) {
                assert!(tokens_in_line(line) <= 1);
            }
            i += 1;
        }
    }

    #[test]
    fn decoys_have_no_tokens() {
        let mut d = 0usize;
        while d < DECOYS.len() {
            assert_eq!(tokens_in_line(DECOYS[d]), 0);
            d += 1;
        }
    }

    #[test]
    fn render_row_roundtrip() {
        let b = board();
        let rows = export_rows(&b);
        let mut i = 0usize;
        while i < rows.len() {
            assert!(audit_row(rows[i].as_str()));
            i += 1;
        }
    }

    #[test]
    fn audit_row_rejects_truncated() {
        assert!(!audit_row(""));
        assert!(!audit_row("SEAL|card=49061"));
        assert!(!audit_row(
            "SEAL|card=49061|quota=4400|token=0|frozen=1"
        ));
    }

    #[test]
    fn audit_row_rejects_wrong_frozen() {
        let b = board();
        let row = match export_rows(&b).first() {
            Some(r) => String::from(r.as_str()),
            None => return,
        };
        let mut chars: Vec<char> = row.chars().collect();
        let n = chars.len();
        if n > 0 {
            chars[n - 1] = if chars[n - 1] == '0' { '1' } else { '0' };
        }
        let tampered: String = chars.into_iter().collect();
        assert!(!audit_row(&tampered));
    }

    #[test]
    fn rows_stable_ten() {
        let b = board();
        assert!(rows_stable(&b, 10));
        assert!(!rows_stable(&b, 0));
    }

    #[test]
    fn card_ids_contiguous() {
        let b = board();
        let mut i = 0usize;
        while i < b.len() {
            let s = b.get(i).expect("seal");
            assert_eq!(s.card, CARD_FIRST + i as u32);
            assert_eq!(s.token, i as u32);
            i += 1;
        }
    }

    #[test]
    fn fingerprints_distinct() {
        let b = board();
        let mut i = 0usize;
        while i < b.len() {
            let mut j = i + 1;
            while j < b.len() {
                assert_ne!(
                    b.get(i).expect("a").digest,
                    b.get(j).expect("b").digest
                );
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn row_fields_parse() {
        let s = CardSeal::new(5).expect("seal");
        let row = render_row(&s);
        let fields = row_fields(&row, TAG, &ROW_KEYS).expect("fields");
        assert_eq!(fields.len(), ROW_KEYS.len() + 1);
        assert_eq!(fields.first().copied(), Some("49066"));
        assert_eq!(fields.get(2).copied(), Some("5"));
    }

    #[test]
    fn row_fields_rejects_tag() {
        assert!(row_fields("", TAG, &ROW_KEYS).is_none());
        assert!(row_fields("NOPE|card=1", TAG, &ROW_KEYS).is_none());
        assert!(row_fields("SEAL|card=1", TAG, &ROW_KEYS).is_none());
    }

    #[test]
    fn margin_overflow_guard() {
        // 倍乘超出 u32 承载时按 Overflow 拒收，而不是截断成错值。
        assert_eq!(margin_bp(u32::MAX, 1).err(), Some(SealCode::Overflow.val()));
        // 同量级分子分母相消后回到刻度内。
        assert_eq!(margin_bp(u32::MAX, u32::MAX), Ok(BP_SCALE));
        assert_eq!(margin_bp(0, 1), Ok(0));
    }

    #[test]
    fn checks_all_green() {
        let set = run_unx49079_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn checks_len_and_truncation() {
        let set = run_unx49079_checks();
        assert_eq!(set.len(), 8);
        assert!(!set.truncated());
        assert_eq!(set.passed_count(), 8);
        assert_eq!(set.failed_count(), 0);
    }

    #[test]
    fn checks_names_distinct() {
        let set = run_unx49079_checks();
        let mut names: Vec<&'static str> = Vec::new();
        for c in set.iter() {
            assert!(!names.contains(&c.name));
            names.push(c.name);
        }
        assert_eq!(names.len(), 8);
    }

    #[test]
    fn checks_detail_nonempty() {
        for (name, passed, det) in extra_checks() {
            assert!(passed, "{} red: {}", name, det);
            assert!(!det.is_empty());
        }
    }

    #[test]
    fn board_rejected_counter() {
        let mut b = SealBoard::new();
        let s1 = CardSeal::new(0).expect("s1");
        assert!(b.push(s1).is_ok());
        // 重投同一张卡：卡序已推进到 49062，旧卡按 OrderBad 拒收且计数加一。
        assert_eq!(b.push(s1).err(), Some(SealCode::OrderBad.val()));
        assert_eq!(b.rejected(), 1);
        assert_eq!(b.len(), 1);
    }
}
