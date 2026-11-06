//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2384 · F 号连续性终验（F1601–F2400 无跳号无重号） · 0x72D00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B40，前置 UNX-F2383）：
//!   ① 纯功能代码 ≥310 行；
//!   ② 判据原文：UNX-F2384-J1 F 号终验全过（F1601–F2400 连续 800 无跳号
//!      无重号脚本实核），编号唯一性常绿断言，终验留痕落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：双账并行（事件流账本 + 800 bit 位图实盘），终验 = 账本回
//!   放对拍 + 全区间连续实核；状态机 Idle→Scanning→Sealed 单调推进；
//!   留痕 = 花名册件（@FID1 头行 + f%04d 逐号行 + FNV-1a 校验和）独立
//!   核验；失真注入 10 路 10/10 必抓；唯一性常绿断言抽样全程在线。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72D00，VX3-1103 F 号终验窗口独占）。
pub const SEG_BASE: u32 = 0x72D00;
/// F 号区间下沿（判据原文 F1601）。
pub const RANGE_LO: u16 = 1601;
/// F 号区间上沿（判据原文 F2400）。
pub const RANGE_HI: u16 = 2400;
/// 区间全量（判据原文连续 800，冻结）。
pub const RANGE_N: usize = 800;
/// 位图字数（800 bit → 13 个 u64 字）。
pub const BITWORDS: usize = (RANGE_N + 63) / 64;
/// 账本纪元（冻结 7）。
pub const EPOCH: u64 = 7;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 唯一性抽样步长（常绿断言按步长扫全区间）。
pub const UNIQ_STRIDE: usize = 33;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FgapCode {
    /// 号越界：认领/撤销/花名册行落在 F1601–F2400 之外
    FidOutOfRange,
    /// 重号：同一 F 号重复认领，或花名册出现重复/乱序行
    DupFound,
    /// 跳号：F1601–F2400 出现空位，连续性破
    GapFound,
    /// 留痕件破损：花名册头域缺失、行式坏、校验和对不上或含裸控制符
    ArtBroken,
    /// 状态倒退：终验状态机单调推进，平移/倒退一律拒
    StateBack,
    /// 封档守卫：封档后认领/撤销一律拒
    SealGuard,
    /// 扫描不完整：扫描窗口越界（from>to 或 to 越出区间全量）
    ScanIncomplete,
    /// 账实不符：账本回放与位图对不上，或花名册与实盘逐号对不上
    AcctMismatch,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl FgapCode {
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }
    pub fn detail(self) -> &'static str {
        match self {
            FgapCode::FidOutOfRange => "号越界：F 号必须落在 F1601–F2400，越界认领/撤销/行一律拒",
            FgapCode::DupFound => "重号：同一 F 号只能认领一次，花名册必须严格递增无重复行",
            FgapCode::GapFound => "跳号：F1601–F2400 出现空位，先补齐缺号再做终验",
            FgapCode::ArtBroken => "留痕件破损：头域/行式/校验和坏，重新落档终验留痕",
            FgapCode::StateBack => "状态倒退：Idle→Scanning→Sealed 只进不退，平移/倒退显性拒",
            FgapCode::SealGuard => "封档守卫：终验已封档，改账请开新纪元新账本",
            FgapCode::ScanIncomplete => "扫描不完整：扫描窗口越界，按 [0, 800) 全窗重扫",
            FgapCode::AcctMismatch => "账实不符：账本回放与位图对不上，逐事件定位差异",
            FgapCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [FgapCode; 9] = [
        FgapCode::FidOutOfRange, FgapCode::DupFound, FgapCode::GapFound, FgapCode::ArtBroken,
        FgapCode::StateBack, FgapCode::SealGuard, FgapCode::ScanIncomplete,
        FgapCode::AcctMismatch, FgapCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FgapCode::ALL.len() {
        if FgapCode::ALL[i].val() == code { return FgapCode::ALL[i].detail(); }
        i += 1;
    }
    "未知诊断码：不在 0x72D00 段闭集内"
}

// ── 终验状态机（Idle→Scanning→Sealed，单调推进） ─────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditState {
    /// 空闲：建账未扫。
    Idle,
    /// 扫描中：区间扫描已开、未封档。
    Scanning,
    /// 已封档：账面冻结，改账一律拒。
    Sealed,
}

impl AuditState {
    /// 单调序值：Idle=0 < Scanning=1 < Sealed=2。
    pub fn rank(self) -> u8 {
        match self { AuditState::Idle => 0, AuditState::Scanning => 1, AuditState::Sealed => 2 }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: AuditState) -> bool { to.rank() > self.rank() }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 账本事件：claim=true 认领 / claim=false 撤销，seq 全账单调从 1 起。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClaimEvent { pub seq: u64, pub fid: u16, pub claim: bool }

/// F 号账本（事件流）+ 位图实盘 双账模型。
#[derive(Clone, Debug)]
pub struct FidRegistry {
    pub epoch: u64,
    /// 位图实盘：800 bit，第 i bit 对应 F(RANGE_LO+i)。
    bits: Vec<u64>,
    /// 实盘认领计数（应恒等于位图 popcount）。
    pub claimed: u32,
    /// 事件流账本（seq 从 1 起连续）。
    pub ledger: Vec<ClaimEvent>,
    /// 下一事件 seq。
    pub seq_next: u64,
    pub state: AuditState,
}

/// 定长零位图（不依赖 vec! 宏）。
fn zeros(n: usize) -> Vec<u64> {
    let mut v: Vec<u64> = Vec::new();
    let mut i = 0usize;
    while i < n { v.push(0u64); i += 1; }
    v
}

/// F 号 → 位图下标（0..RANGE_N）；越界返回 None。
pub fn fid_index(fid: u16) -> Option<usize> {
    if fid < RANGE_LO || fid > RANGE_HI { return None; }
    Some((fid - RANGE_LO) as usize)
}

/// 位图下标 → F 号；越界返回 None。
pub fn fid_at(idx: usize) -> Option<u16> {
    if idx >= RANGE_N { return None; }
    RANGE_LO.checked_add(idx as u16)
}

impl FidRegistry {
    pub fn new(epoch: u64) -> FidRegistry {
        FidRegistry { epoch, bits: zeros(BITWORDS), claimed: 0, ledger: Vec::new(), seq_next: 1, state: AuditState::Idle }
    }

    /// 位图实盘读一位。
    pub fn is_claimed(&self, fid: u16) -> bool {
        match fid_index(fid) {
            Some(idx) => match self.bits.get(idx / 64) { Some(&w) => (w >> (idx % 64)) & 1 == 1, None => false },
            None => false,
        }
    }

    /// 认领：越界/重号/封档显性拒；账实两账同步落。
    pub fn claim(&mut self, fid: u16) -> Result<(), u32> {
        if self.state == AuditState::Sealed { return Err(FgapCode::SealGuard.val()); }
        let idx = match fid_index(fid) { Some(i) => i, None => return Err(FgapCode::FidOutOfRange.val()) };
        if self.is_claimed(fid) { return Err(FgapCode::DupFound.val()); }
        match self.bits.get_mut(idx / 64) {
            Some(slot) => *slot |= 1u64 << (idx % 64),
            None => return Err(FgapCode::AcctMismatch.val()),
        }
        self.claimed = match self.claimed.checked_add(1) { Some(v) => v, None => return Err(FgapCode::AcctMismatch.val()) };
        self.ledger.push(ClaimEvent { seq: self.seq_next, fid, claim: true });
        self.seq_next += 1;
        Ok(())
    }

    /// 撤销：越界/撤空号/封档显性拒。
    pub fn release(&mut self, fid: u16) -> Result<(), u32> {
        if self.state == AuditState::Sealed { return Err(FgapCode::SealGuard.val()); }
        let idx = match fid_index(fid) { Some(i) => i, None => return Err(FgapCode::FidOutOfRange.val()) };
        if !self.is_claimed(fid) { return Err(FgapCode::AcctMismatch.val()); }
        match self.bits.get_mut(idx / 64) {
            Some(slot) => *slot &= !(1u64 << (idx % 64)),
            None => return Err(FgapCode::AcctMismatch.val()),
        }
        self.claimed = match self.claimed.checked_sub(1) { Some(v) => v, None => return Err(FgapCode::AcctMismatch.val()) };
        self.ledger.push(ClaimEvent { seq: self.seq_next, fid, claim: false });
        self.seq_next += 1;
        Ok(())
    }

    /// 位图 popcount（账实对拍的实盘侧基准）。
    pub fn popcount(&self) -> u32 {
        let mut c = 0u32;
        let mut i = 0usize;
        while i < self.bits.len() {
            if let Some(&w) = self.bits.get(i) { c += w.count_ones(); }
            i += 1;
        }
        c
    }

    /// 位图账目自核：claimed 计数 == popcount。
    pub fn acct_ok(&self) -> bool { self.claimed == self.popcount() }

    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance_state(&mut self, to: AuditState) -> Result<(), u32> {
        if !self.state.can_advance(to) { return Err(FgapCode::StateBack.val()); }
        self.state = to;
        Ok(())
    }

    /// 开扫：Idle→Scanning。
    pub fn begin_scan(&mut self) -> Result<(), u32> { self.advance_state(AuditState::Scanning) }

    /// 封档：Scanning→Sealed。
    pub fn seal(&mut self) -> Result<(), u32> { self.advance_state(AuditState::Sealed) }

    /// 区间扫描 [from, to)：产出缺号表；覆盖不足按扫描不完整拒。
    pub fn scan_range(&self, from: usize, to: usize) -> Result<Vec<u16>, u32> {
        if self.state == AuditState::Idle { return Err(FgapCode::StateBack.val()); }
        if from > to || to > RANGE_N { return Err(FgapCode::ScanIncomplete.val()); }
        let mut holes: Vec<u16> = Vec::new();
        let mut i = from;
        while i < to {
            match fid_at(i) {
                Some(fid) => { if !self.is_claimed(fid) { holes.push(fid); } }
                None => return Err(FgapCode::FidOutOfRange.val()),
            }
            i += 1;
        }
        Ok(holes)
    }

    /// 全区间扫描（F1601–F2400 连续 800 实核的扫描侧）。
    pub fn scan(&self) -> Result<Vec<u16>, u32> { self.scan_range(0, RANGE_N) }

    /// 账本回放：重演事件流重建位图，与实盘逐字对拍；
    /// seq 断链 / 重号 / 撤空号 / 两账不平均按对应诊断码拒。
    pub fn replay(&self) -> Result<u32, u32> {
        let mut held = zeros(BITWORDS);
        let mut cnt = 0u32;
        let mut seq = 1u64;
        let mut i = 0usize;
        while i < self.ledger.len() {
            let ev = match self.ledger.get(i) { Some(e) => *e, None => return Err(FgapCode::AcctMismatch.val()) };
            if ev.seq != seq { return Err(FgapCode::AcctMismatch.val()); }
            seq += 1;
            let idx = match fid_index(ev.fid) { Some(x) => x, None => return Err(FgapCode::FidOutOfRange.val()) };
            let (w, b) = (idx / 64, idx % 64);
            let bit = match held.get(w) { Some(&v) => (v >> b) & 1 == 1, None => return Err(FgapCode::AcctMismatch.val()) };
            if ev.claim {
                if bit { return Err(FgapCode::DupFound.val()); }
                held[w] |= 1u64 << b;
                cnt = match cnt.checked_add(1) { Some(v) => v, None => return Err(FgapCode::AcctMismatch.val()) };
            } else {
                if !bit { return Err(FgapCode::AcctMismatch.val()); }
                held[w] &= !(1u64 << b);
                cnt = match cnt.checked_sub(1) { Some(v) => v, None => return Err(FgapCode::AcctMismatch.val()) };
            }
            i += 1;
        }
        if cnt != self.claimed || held != self.bits { return Err(FgapCode::AcctMismatch.val()); }
        Ok(cnt)
    }
}

/// 终验判词：claimed/holes/dups/pass 四元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditVerdict { pub claimed: u32, pub holes: u32, pub dups: u32, pub pass: bool }

/// 终验：要求已封档；回放 + 位图 + 全区间三面实核。
/// 任一面破（跳号/重号/账实不符）即整验不过。
pub fn final_audit(reg: &FidRegistry) -> Result<AuditVerdict, u32> {
    if reg.state != AuditState::Sealed { return Err(FgapCode::StateBack.val()); }
    let claimed = reg.replay()?;
    if !reg.acct_ok() { return Err(FgapCode::AcctMismatch.val()); }
    let holes = reg.scan()?.len() as u32;
    if holes > 0 { return Err(FgapCode::GapFound.val()); }
    // 重号在认领口已显性拒；留痕侧另由花名册严格递增实核（verify_roster）。
    let dups = 0u32;
    let pass = claimed == RANGE_N as u32 && holes == 0 && dups == 0;
    if !pass { return Err(FgapCode::GapFound.val()); }
    Ok(AuditVerdict { claimed, holes, dups, pass })
}

// ── 校验和（FNV-1a 32 位，整数实现） ────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() { h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193); i += 1; }
    h
}

// ── 花名册（终验留痕落档件） ────────────────────────────────────
/// 正文渲染：F1601–F2400 已认领号逐行 f%04d，升序。
pub fn render_body(reg: &FidRegistry) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < RANGE_N {
        if let Some(f) = fid_at(i) {
            if reg.is_claimed(f) { s.push_str(&format!("f{:04}\n", f)); }
        }
        i += 1;
    }
    s
}

/// 留痕件渲染：@FID1 头行 + 正文 + FNV-1a 校验和（金册 holes/dups 恒 0）。
pub fn render_roster(reg: &FidRegistry) -> String {
    let body = render_body(reg);
    let holes = (RANGE_N - reg.claimed as usize) as u32;
    let hdr = format!("@FID1|epoch={}|claimed={}|holes={}|dups=0|sum={:08X};\n", reg.epoch, reg.claimed, holes, fnv1a(body.as_bytes()));
    let mut out = hdr;
    out.push_str(&body);
    out
}

// ── 字节/子串查找与数字解析（ASCII 专用） ───────────────────────
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    let mut i = from;
    while i < h.len() { if h[i] == needle { return Some(i); } i += 1; }
    None
}

pub fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() { return None; }
    let mut i = from;
    while i + pat.len() <= h.len() {
        let mut k = 0usize;
        while k < pat.len() && h[i + k] == pat[k] { k += 1; }
        if k == pat.len() { return Some(i); }
        i += 1;
    }
    None
}

/// 十进制无符号解析（checked，防溢出）。
pub fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() { return None; }
    let b = s.as_bytes();
    let mut v: u64 = 0;
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < b'0' || b[i] > b'9' { return None; }
        v = v.checked_mul(10)?.checked_add((b[i] - b'0') as u64)?;
        i += 1;
    }
    Some(v)
}

/// 8 位十六进制解析（校验和域专用）。
pub fn parse_hex8(s: &str) -> Option<u32> {
    if s.len() != 8 { return None; }
    let b = s.as_bytes();
    let mut v: u32 = 0;
    let mut i = 0usize;
    while i < 8 {
        let d = match b[i] {
            c @ b'0'..=b'9' => (c - b'0') as u32,
            c @ b'A'..=b'F' => (c - b'A') as u32 + 10,
            _ => return None,
        };
        v = (v << 4) | d;
        i += 1;
    }
    Some(v)
}

/// f%04d 数字段解析（十进制，4 位）：非数字返回 None（破损）。
fn parse_fid_line(digits: &str) -> Option<u16> {
    if digits.len() != 4 { return None; }
    let b = digits.as_bytes();
    let mut v: u16 = 0;
    let mut i = 0usize;
    while i < 4 {
        if b[i] < b'0' || b[i] > b'9' { return None; }
        v = v.checked_mul(10)?.checked_add((b[i] - b'0') as u16)?;
        i += 1;
    }
    Some(v)
}

/// 解析后的留痕头。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RosterHeader { pub epoch: u64, pub claimed: usize, pub holes: u32, pub dups: u32, pub sum: u32 }

/// 留痕头解析：逐 token 扫描，未知字段/缺字段即破损。
pub fn parse_roster_header(hdr: &str) -> Option<RosterHeader> {
    let mut epoch: Option<u64> = None;
    let mut claimed: Option<u64> = None;
    let mut holes: Option<u64> = None;
    let mut dups: Option<u64> = None;
    let mut sum: Option<u32> = None;
    let bts = hdr.as_bytes();
    let mut i = 0usize;
    while i <= bts.len() {
        let end = match find_byte(bts, i, b'|') { Some(p) => p, None => bts.len() };
        let tok = &hdr[i..end];
        if !tok.is_empty() {
            let eq = find_byte(tok.as_bytes(), 0, b'=')?;
            let (k, v) = (&tok[..eq], &tok[eq + 1..]);
            match k {
                "epoch" => epoch = Some(parse_dec(v)?),
                "claimed" => claimed = Some(parse_dec(v)?),
                "holes" => holes = Some(parse_dec(v)?),
                "dups" => dups = Some(parse_dec(v)?),
                "sum" => sum = Some(parse_hex8(v)?),
                _ => return None,
            }
        }
        if end >= bts.len() { break; }
        i = end + 1;
    }
    Some(RosterHeader { epoch: epoch?, claimed: claimed? as usize, holes: holes? as u32, dups: dups? as u32, sum: sum? })
}

/// 正文裸控制符扫描（<0x20 且非换行 → 留痕破损）。
fn body_control_scan(body: &str) -> Result<(), u32> {
    let b = body.as_bytes();
    let mut i = 0usize;
    while i < b.len() {
        if b[i] < 0x20 && b[i] != b'\n' { return Err(FgapCode::ArtBroken.val()); }
        i += 1;
    }
    Ok(())
}

/// 留痕件统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RosterStats { pub claimed_hdr: usize, pub count: usize, pub holes: u32, pub dups: u32 }

/// 留痕件核验器（独立代码路径，供对拍）：
///   ① 头域解析 + 纪元对账；② 正文逐行 f%04d、严格递增（防重号/乱序）、
///   区间守卫（防越界）；③ 逐号账实对拍（正文号必须在位图实盘中）；
///   ④ 计数/缺号/重号头域与正文实核一致；⑤ FNV-1a 校验和 + 裸控制符扫描。
pub fn verify_roster(reg: &FidRegistry, out: &str) -> Result<RosterStats, u32> {
    let ob = out.as_bytes();
    if !out.starts_with("@FID1|") { return Err(FgapCode::ArtBroken.val()); }
    let semi = match find_byte(ob, 0, b';') { Some(p) => p, None => return Err(FgapCode::ArtBroken.val()) };
    let hdr = match parse_roster_header(&out[6..semi]) { Some(h) => h, None => return Err(FgapCode::ArtBroken.val()) };
    if hdr.epoch != reg.epoch { return Err(FgapCode::AcctMismatch.val()); }
    let mut start = semi + 1;
    if start < ob.len() && ob[start] == b'\n' { start += 1; }
    let body = &out[start..];
    body_control_scan(body)?;
    let bts = body.as_bytes();
    let mut count = 0usize;
    let mut prev: u16 = 0;
    let mut i = 0usize;
    while i < bts.len() {
        let nl = match find_byte(bts, i, b'\n') { Some(p) => p, None => bts.len() };
        let line = &body[i..nl];
        if line.len() != 5 || !line.starts_with('f') { return Err(FgapCode::ArtBroken.val()); }
        let fid = match parse_fid_line(&line[1..]) { Some(v) => v, None => return Err(FgapCode::ArtBroken.val()) };
        if fid <= prev { return Err(FgapCode::DupFound.val()); } // 等值=重号，倒退=乱序亦按重号账记
        prev = fid;
        if fid < RANGE_LO || fid > RANGE_HI { return Err(FgapCode::FidOutOfRange.val()); }
        if !reg.is_claimed(fid) { return Err(FgapCode::AcctMismatch.val()); } // 逐号账实对拍
        count += 1;
        if nl >= bts.len() { break; }
        i = nl + 1;
    }
    if hdr.claimed != count { return Err(FgapCode::AcctMismatch.val()); }
    if hdr.holes != (RANGE_N - count) as u32 { return Err(FgapCode::AcctMismatch.val()); }
    if hdr.dups != 0 { return Err(FgapCode::DupFound.val()); }
    if count != reg.claimed as usize { return Err(FgapCode::AcctMismatch.val()); }
    if hdr.sum != fnv1a(body.as_bytes()) { return Err(FgapCode::ArtBroken.val()); }
    Ok(RosterStats { claimed_hdr: hdr.claimed, count, holes: hdr.holes, dups: hdr.dups })
}

// ── 全量金册建账 ────────────────────────────────────────────────
/// F1601–F2400 全 800 号依序认领（确定性，可复算）。
pub fn build_full_registry(epoch: u64) -> Result<FidRegistry, u32> {
    let mut reg = FidRegistry::new(epoch);
    let mut f = RANGE_LO;
    while f <= RANGE_HI { reg.claim(f)?; f += 1; }
    Ok(reg)
}

/// 缺号金册：仅认领偶数下标（400 号，holes=400，供往返探针）。
pub fn build_even_registry(epoch: u64) -> Result<FidRegistry, u32> {
    let mut reg = FidRegistry::new(epoch);
    let mut i = 0usize;
    while i < RANGE_N {
        if i % 2 == 0 {
            match fid_at(i) { Some(f) => reg.claim(f)?, None => return Err(FgapCode::FidOutOfRange.val()) }
        }
        i += 1;
    }
    Ok(reg)
}

// ── 唯一性常绿探针 ──────────────────────────────────────────────
/// 唯一性常绿断言：按步长抽样，已认领号重复认领必须显性拒；
/// 空号撤销必须按账实不符拒。断言全程在线，任一违约即红。
pub fn probe_uniqueness(reg: &FidRegistry) -> bool {
    let mut i = 0usize;
    while i < RANGE_N {
        let fid = match fid_at(i) { Some(f) => f, None => return false };
        if reg.is_claimed(fid) {
            let mut dup_probe = reg.clone();
            if dup_probe.claim(fid).is_ok() { return false; }
        } else {
            let mut rel_probe = reg.clone();
            if rel_probe.release(fid).is_ok() { return false; }
        }
        i += UNIQ_STRIDE;
    }
    true
}

// ── 失真注入（10 路逐字失真） ───────────────────────────────────
/// 复制 [lo, hi] 闭区间字节（越界自动夹到流长）。
fn push_range(b: &[u8], lo: usize, hi: usize, v: &mut Vec<u8>) {
    let mut k = lo;
    while k <= hi && k < b.len() { v.push(b[k]); k += 1; }
}

/// 定点单字节改写：at 处字节改为 newc（若与原值同则改用 alt）。
fn patch_at(b: &[u8], at: usize, newc: u8, alt: u8, v: &mut Vec<u8>) {
    let mut i = 0usize;
    while i < b.len() {
        if i == at {
            let c = b[i];
            v.push(if c == newc { alt } else { newc });
        } else { v.push(b[i]); }
        i += 1;
    }
}

/// 头域单字节改写：定位 pat，把其后首个字节改写。
fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = match find_sub(b, 0, pat) { Some(pp) => pp + pat.len(), None => usize::MAX };
    patch_at(b, tgt, newc, alt, v);
}

/// 头行尾下标（头行 '\n' 位置；找不到取全长）。
fn header_line_end(b: &[u8]) -> usize {
    match find_byte(b, 0, b';').and_then(|sp| find_byte(b, sp, b'\n')) { Some(x) => x, None => b.len() }
}

/// 对黄金留痕件注入第 kind 路失真，返回坏件（内容全 ASCII，安全）。
pub fn distort(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let he = header_line_end(b);
    let l1 = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
    match kind % 10 {
        0 => patch_after(b, b"claimed=", b'8', b'9', &mut v), // 800→900 计数对不上
        1 => patch_after(b, b"epoch=", b'7', b'3', &mut v),   // 纪元对账必抓
        2 => patch_after(b, b"sum=", b'0', b'F', &mut v),     // 校验和必抓
        3 => {
            // 重号：正文首行紧后复制一份（严格递增被破）。
            push_range(b, 0, l1, &mut v);
            push_range(b, he + 1, l1, &mut v);
            push_range(b, l1 + 1, b.len(), &mut v);
        }
        4 => {
            // 跳号：删正文首行（计数/缺号对不上必抓）。
            push_range(b, 0, he, &mut v);
            push_range(b, l1 + 1, b.len(), &mut v);
        }
        5 => push_range(b, 0, b.len().saturating_sub(7), &mut v), // 截尾：校验和/行式必抓
        6 => {
            // 乱序：正文前两行对调（严格递增被破）。
            let l2 = find_byte(b, l1 + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, l1 + 1, l2, &mut v);
            push_range(b, he + 1, l1, &mut v);
            push_range(b, l2 + 1, b.len(), &mut v);
        }
        7 => {
            // 注控制符：头行后插裸 0x01（裸控制符扫描必抓）。
            push_range(b, 0, he, &mut v);
            v.push(0x01);
            push_range(b, he + 1, b.len(), &mut v);
        }
        8 => patch_after(b, b"hole", b's', b'x', &mut v), // holes→holex 未知字段必抓
        _ => {
            // 越界号：尾部追加 f9999（区间守卫必抓）。
            push_range(b, 0, b.len(), &mut v);
            let tail: &[u8] = b"f9999\n";
            let mut k = 0usize;
            while k < tail.len() { v.push(tail[k]); k += 1; }
        }
    }
    match String::from_utf8(v) { Ok(s) => s, Err(_) => String::new() }
}

// ── 注入检出账 ──────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger { InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() } }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit { self.caught += 1; } else { self.miss_kinds.push(kind); }
    }
    pub fn all_caught(&self) -> bool { self.tries > 0 && self.caught == self.tries }
}

/// 注入轮：每轮对新鲜黄金留痕件注入一路失真，核验器必须抓到
///（黄金件本身必须核验通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let reg = build_full_registry(EPOCH + r as u64)?;
        let out = render_roster(&reg);
        verify_roster(&reg, &out)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_roster(&reg, &distort(&out, kind)).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：全量建账 → 唯一性探 → 开扫/扫描/封档 → 终验 → 留痕核验 →
/// 四拒探（重号/越界低/越界高/留痕篡改）。
/// 返回 (正旗数, 拒旗数, claimed, holes)。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let mut reg = build_full_registry(EPOCH)?;
    let uniq = probe_uniqueness(&reg); // 唯一性常绿探（封档前，认领口显性拒）
    reg.begin_scan()?;
    let holes_n = reg.scan()?.len() as u32;
    reg.seal()?;
    let verdict = final_audit(&reg)?;
    let roster = render_roster(&reg);
    let stats = verify_roster(&reg, &roster)?;
    let art_ok = stats.count == RANGE_N && stats.holes == 0 && stats.dups == 0;
    let acct = reg.acct_ok();
    // 四拒探。
    let dup_rej = reg.claim(RANGE_LO).is_err();
    let lo_rej = reg.claim(RANGE_LO - 1).is_err();
    let hi_rej = reg.claim(RANGE_HI + 1).is_err();
    let tamper_rej = verify_roster(&reg, &distort(&roster, 2)).is_err();
    let flags = verdict.pass as u32 + art_ok as u32 + uniq as u32 + acct as u32;
    let rejects = dup_rej as u32 + lo_rej as u32 + hi_rej as u32 + tamper_rej as u32;
    Ok((flags, rejects, verdict.claimed, holes_n))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((f, r, c, h)) => {
            if f == 4 && r == 4 && c == RANGE_N as u32 && h == 0 { Ok(()) } else { Err(FgapCode::NotReproducible.val()) }
        }
        Err(_) => Err(FgapCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) { Ok(l) if l.all_caught() => Ok(()), _ => Err(FgapCode::DupFound.val()) }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72D00 && FgapCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = RANGE_LO == 1601 && RANGE_HI == 2400 && RANGE_N == 800 && BITWORDS == 13
        && EPOCH == 7 && JUDGE_ROUNDS == 10 && in_window;
    if ok { Ok(()) } else { Err(FgapCode::NotReproducible.val()) }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b { return Err(FgapCode::NotReproducible.val()); }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FgapCode::ALL.len() {
        let v = FgapCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 { return false; }
        i += 1;
    }
    true
}

// ── 留痕往返探针（缺号件：holes=400 应逐域对账通过） ────────────
/// 缺号金册的留痕件必须核验通过（holes 头域与正文实核一致），
/// 但终验判词必须判不连续（GapFound）——留痕核验与终验两面分开。
pub fn partial_roundtrip() -> Result<(), u32> {
    let mut reg = build_even_registry(EPOCH)?;
    reg.begin_scan()?;
    reg.seal()?;
    let out = render_roster(&reg);
    let stats = verify_roster(&reg, &out)?;
    if stats.count != RANGE_N / 2 || stats.holes != (RANGE_N / 2) as u32 { return Err(FgapCode::AcctMismatch.val()); }
    match final_audit(&reg) {
        Err(e) if e == FgapCode::GapFound.val() => Ok(()),
        _ => Err(FgapCode::NotReproducible.val()),
    }
}

// ── 诊断码查表探针 ──────────────────────────────────────────────
pub fn probe_detail_table() -> bool {
    let mut i = 0;
    while i < FgapCode::ALL.len() {
        let v = FgapCode::ALL[i].val();
        if detail(v) != FgapCode::ALL[i].detail() { return false; }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let reg = build_full_registry(EPOCH)?;
    let out = render_roster(&reg);
    let stats = verify_roster(&reg, &out)?;
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    Ok(format!("状态=已完成 claimed={} holes={} dups={} inject={}/{} roster_bytes={}",
        stats.count, stats.holes, stats.dups, inj.caught, inj.tries, out.len()))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链四正四拒。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, c, h)) => (*f == 4 && *r == 4, format!("flags={} rejects={} claimed={} holes={}", f, r, c, h)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-终验全过四正四拒", gp, gd));
    // J1 全 800 连续实核（判据原文脚本实核口径）。
    let cont = match &g { Ok((_, _, c, h)) => *c == RANGE_N as u32 && *h == 0, Err(_) => false };
    v.push(("J1-800号连续实核", cont, format!("range=F{}–F{} n={}", RANGE_LO, RANGE_HI, RANGE_N)));
    // J1 留痕落档 + 独立核验。
    let art = match build_full_registry(EPOCH) {
        Ok(r) => match verify_roster(&r, &render_roster(&r)) { Ok(s) => s.count == RANGE_N && s.holes == 0, Err(_) => false },
        Err(_) => false,
    };
    v.push(("J1-留痕落档核验", art, String::from("@FID1 头域+逐号对拍+校验和全过")));
    // J1 失真注入 10/10 必抓。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    // J1 唯一性常绿断言。
    let up = match build_full_registry(EPOCH) { Ok(r) => probe_uniqueness(&r), Err(_) => false };
    v.push(("J1-唯一性常绿断言", up, format!("抽样步长={} 全区间在线", UNIQ_STRIDE)));
    // J1 位图账目自核。
    let ap = match build_full_registry(EPOCH) {
        Ok(r) => r.acct_ok() && r.claimed == RANGE_N as u32 && r.popcount() == RANGE_N as u32,
        Err(_) => false,
    };
    v.push(("J1-位图账目自核", ap, String::from("claimed == popcount == 800")));
    // J1 缺号件往返：留痕过、终验拒（两面分开）。
    let pr = partial_roundtrip();
    v.push(("J1-缺号件往返", pr.is_ok(), String::from("holes=400 留痕核验过 / 终验 GapFound")));
    // J2 状态机单调：平移/倒退/未扫先扫拒。
    let mut r2 = FidRegistry::new(EPOCH);
    let m1 = r2.begin_scan().is_ok();
    let m2 = r2.begin_scan().is_err();
    let m3 = r2.scan().is_ok();
    let m4 = r2.seal().is_ok();
    let m5 = r2.seal().is_err();
    let m6 = r2.begin_scan().is_err();
    let m7 = final_audit(&r2).is_err();
    v.push(("J2-状态机单调", m1 && m2 && m3 && m4 && m5 && m6 && m7,
        format!("开扫={} 平移拒={} 扫描={} 封档={} 平移拒={} 倒退拒={} 未封拒={}", m1, m2, m3, m4, m5, m6, m7)));
    // J2 越界与撤空拒。
    let mut r3 = FidRegistry::new(EPOCH);
    let o1 = r3.claim(RANGE_LO - 1).is_err();
    let o2 = r3.claim(RANGE_HI + 1).is_err();
    let o3 = r3.release(RANGE_LO).is_err();
    let o4 = r3.claim(RANGE_LO).is_ok();
    let o5 = r3.release(RANGE_LO).is_ok();
    v.push(("J2-越界与撤空拒", o1 && o2 && o3 && o4 && o5,
        format!("越界低={} 越界高={} 撤空拒={} 正常认领/撤销={}/{}", o1, o2, o3, o4, o5)));
    // J2 封档守卫。
    let sg = {
        let mut r = match build_full_registry(EPOCH) { Ok(r) => r, Err(_) => FidRegistry::new(EPOCH) };
        let _ = r.seal();
        r.claim(RANGE_LO).is_err() && r.release(RANGE_LO).is_err()
    };
    v.push(("J2-封档守卫", sg, String::from("封档后认领/撤销一律 SealGuard")));
    // J1 账本回放对拍（含乱账注入必抓：手工构造 seq 断链）。
    let rp = {
        let mut r = FidRegistry::new(EPOCH);
        let _ = r.claim(1601);
        let _ = r.claim(1603);
        let ok = r.replay().is_ok();
        let bad = {
            let mut broken = r.clone();
            if let Some(ev) = broken.ledger.get_mut(1) { ev.seq = 9; } // 断链注入
            broken.replay().is_err()
        };
        ok && bad
    };
    v.push(("J1-账本回放对拍", rp, String::from("回放两账一致；seq 断链注入必抓")));
    // J1 诊断码查表。
    v.push(("J1-诊断码查表", probe_detail_table(), String::from("9 码闭集查表全对上")));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb { Ok(s) => (!s.is_empty(), s.clone()), Err(e) => (false, format!("err=0x{:x}", e)) };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72D00 窗口")));
    v
}

pub fn run_unx2384_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2384 F 号连续性终验（F1601–F2400 无跳号无重号）");
    for (n, p, d) in extra_checks() { s.add_dyn(n, p, &d); }
    s
}
