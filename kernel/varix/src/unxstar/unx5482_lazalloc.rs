//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F5482 · 延迟分配回归账 · 0x72F80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B2-B35）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F5482-J1 B09–B10 延迟分配断言重放 20 用例全过，
//!     重放清单与深化册判据对照一致率 100%，可导出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：预留 reserve 页、首触才提交（越界触碰显性拒、重复幂等）；
//!   双路对照=重放状态机 vs 深化册闭式算法（过滤+插入排序+去重）逐用例
//!   全等；20 用例由 id 确定性派生；重放账 @VXL1 可导出、五重闸核验、
//!   失真注入 10 路 10/10 必抓；重放序严格单调（跳例/回例一律拒）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72F80，VX3-1103 延迟分配窗口独占）。
pub const SEG_BASE: u32 = 0x72F80;
/// 重放用例数（判据原文 20 用例，冻结）。
pub const CASE_N: usize = 20;
pub const PAGE_BYTES: u64 = 4096;
/// 预留基础页数（用例 k 预留 8+(k*3)%5*4 页，冻结）。
pub const RESERVE_BASE: u32 = 8;
/// 失真注入轮数（判据 10 路语义，冻结）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 开销占比上限（≤2% = 200 bps，冻结）。
pub const OVERHEAD_CAP_BPS: u64 = 200;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LazyCode {
    /// 页越界：触碰页号 ≥ 预留页数（延迟分配只在预留界内提交）
    PageRange,
    /// 用例倒退：重放序只进不退，跳例、回例、封盘后再重放一律拒
    CursorBack,
    /// 重放账破损：头字段缺失、数值域不合法或校验和对不上
    LedgerBroken,
    /// 对照失真：重放路径与深化册路径逐字段不一致
    CrossMiss,
    /// 预备不足：提交页数 > 预留页数（延迟分配守恒红线被破）
    ReserveLow,
    /// 空用例：触碰序为空的用例拒绝重放（先备触碰序再重放）
    CaseEmpty,
    /// 开销超限：簿记占比 >2%（200 bps）
    OverheadHigh,
    /// 双跑不一致：复现红线，golden_run 两次应同值
    NotReproducible,
}

impl LazyCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            LazyCode::PageRange => "页越界：触碰页号必须 < 预留页数，越界触碰按预期拒记账",
            LazyCode::CursorBack => "用例倒退：重放序只进不退，跳例/回例/封盘后再重放一律拒",
            LazyCode::LedgerBroken => "重放账破损：字段缺失或校验和对不上，重出重放账",
            LazyCode::CrossMiss => "对照失真：重放路径与深化册路径不一致，逐字段定位差异用例",
            LazyCode::ReserveLow => "预备不足：提交页数超预留，延迟分配守恒红线被破",
            LazyCode::CaseEmpty => "空用例：触碰序为空拒绝重放，先备触碰序再重放",
            LazyCode::OverheadHigh => "开销超限：簿记占比 >2%，查重放账头尺寸与游标簿记",
            LazyCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [LazyCode; 8] = [
        LazyCode::PageRange,
        LazyCode::CursorBack,
        LazyCode::LedgerBroken,
        LazyCode::CrossMiss,
        LazyCode::ReserveLow,
        LazyCode::CaseEmpty,
        LazyCode::OverheadHigh,
        LazyCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for i in 0..(LazyCode::ALL.len()) {
        if LazyCode::ALL[i].val() == code {
            return LazyCode::ALL[i].detail();
        }
    }
    "未知诊断码：不在 0x72F80 段闭集内"
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegionState {
    /// 已预留未触碰：零页提交。
    Reserved,
    /// 激活：至少一页已提交，未满。
    Active,
    /// 已填满：提交页数 = 预留页数。
    Filled,
}

impl RegionState {
    /// 单调序值：Reserved=0 < Active=1 < Filled=2。
    pub fn rank(self) -> u8 {
        match self {
            RegionState::Reserved => 0,
            RegionState::Active => 1,
            RegionState::Filled => 2,
        }
    }
}

/// 用例规格：id 确定性派生预留页数与触碰序。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseSpec {
    pub id: u64,
    /// 预留页数。
    pub reserve: u32,
    /// 触碰页号序（越界触碰按预期拒）。
    pub touches: Vec<u32>,
}

/// 用例结果：重放/深化册两路各自算出后互证。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseResult {
    /// 提交页数（去重后的有效触碰页数）。
    pub committed: u32,
    /// 越界拒次数。
    pub rejects: u32,
    /// 幂等触碰次数（已提交页的重复触碰）。
    pub idempotent: u32,
    pub state: RegionState,
    /// 升序已提交页单。
    pub pages: Vec<u32>,
}

/// 用例派生：reserve=8+(id*3)%5*4 页，触碰序=(t*t+id*5) mod (reserve+3)。
pub fn derive_case(id: u64) -> CaseSpec {
    let reserve = RESERVE_BASE + (((id * 3) % 5) as u32) * 4;
    let t_count = 6usize + ((id % 7) as usize);
    let touches: Vec<u32> = (0..t_count)
        .map(|t| ((t as u64).wrapping_mul(t as u64).wrapping_add(id.wrapping_mul(5)) % ((reserve as u64) + 3)) as u32)
        .collect();
    CaseSpec { id, reserve, touches }
}

pub fn derive_cases() -> Vec<CaseSpec> {
    (1..=(CASE_N as u64)).map(derive_case).collect()
}

/// 区段：预留 + 提交位图（u32 位图，预留 ≤ 28 页够用）。
#[derive(Clone, Debug)]
pub struct Region {
    pub reserve: u32,
    pub mask: u32,
    pub state: RegionState,
    pub rejects: u32,
    pub idempotent: u32,
}

impl Region {
    pub fn new(reserve: u32) -> Region {
        Region { reserve, mask: 0, state: RegionState::Reserved, rejects: 0, idempotent: 0 }
    }

    /// 触碰一页：越界拒（PageRange）；有效页首次提交、重复幂等。
    pub fn touch(&mut self, page: u32) -> Result<bool, u32> {
        if page >= self.reserve {
            self.rejects += 1;
            return Err(LazyCode::PageRange.val());
        }
        let bit = 1u32.checked_shl(page).unwrap_or(0);
        if self.mask & bit != 0 {
            self.idempotent += 1;
            return Ok(false);
        }
        self.mask |= bit;
        if self.state == RegionState::Reserved {
            self.state = RegionState::Active;
        }
        if self.committed() == self.reserve {
            self.state = RegionState::Filled;
        }
        Ok(true)
    }

    pub fn committed(&self) -> u32 {
        self.mask.count_ones()
    }

    pub fn conservation_ok(&self) -> bool {
        self.committed() <= self.reserve
    }

    pub fn pages(&self) -> Vec<u32> {
        (0..self.reserve)
            .filter(|p| self.mask & 1u32.checked_shl(*p).unwrap_or(0) != 0)
            .collect()
    }
}

/// 重放一用例（重放路径）：状态机逐步施加触碰。
pub fn replay_case(spec: &CaseSpec) -> Result<CaseResult, u32> {
    if spec.touches.is_empty() {
        return Err(LazyCode::CaseEmpty.val());
    }
    let mut rg = Region::new(spec.reserve);
    for p in spec.touches.iter() {
        let _ = rg.touch(*p);
    }
    if !rg.conservation_ok() {
        return Err(LazyCode::ReserveLow.val());
    }
    Ok(CaseResult {
        committed: rg.committed(),
        rejects: rg.rejects,
        idempotent: rg.idempotent,
        state: rg.state,
        pages: rg.pages(),
    })
}

/// 深化册对照计算：不用状态机，直接闭式推导。
pub fn canonical_case(spec: &CaseSpec) -> Result<CaseResult, u32> {
    if spec.touches.is_empty() {
        return Err(LazyCode::CaseEmpty.val());
    }
    let mut valid: Vec<u32> = Vec::new();
    let mut rejects = 0u32;
    for p in spec.touches.iter() {
        if *p >= spec.reserve {
            rejects += 1;
        } else {
            valid.push(*p);
        }
    }
    for i in 1..(valid.len()) {
        let mut j = i;
        while j > 0 {
            let ab = match (valid.get(j - 1), valid.get(j)) {
                (Some(a), Some(b)) => (*a, *b),
                _ => break,
            };
            if ab.0 <= ab.1 {
                break;
            }
            valid[j - 1] = ab.1;
            valid[j] = ab.0;
            j -= 1;
        }
    }
    let mut pages: Vec<u32> = Vec::new();
    for p in valid.iter() {
        let dup = match pages.last() {
            Some(last) => *last == *p,
            None => false,
        };
        if !dup {
            pages.push(*p);
        }
    }
    let idempotent = (valid.len().saturating_sub(pages.len())) as u32;
    let committed = pages.len() as u32;
    let state = if committed == 0 {
        RegionState::Reserved
    } else if committed >= spec.reserve {
        RegionState::Filled
    } else {
        RegionState::Active
    };
    if committed > spec.reserve {
        return Err(LazyCode::ReserveLow.val());
    }
    Ok(CaseResult { committed, rejects, idempotent, state, pages })
}

pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for i in 0..(bytes.len()) {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
    }
    h
}

/// 用例行渲染（导出路径）：case=01|reserve=..|..|pages=0,1,2|state=Filled。
pub fn render_case_line(spec: &CaseSpec, r: &CaseResult) -> String {
    let pages = r.pages.iter().map(|p| p.to_string()).collect::<Vec<String>>().join(",");
    format!(
        "case={:02}|reserve={}|touches={}|committed={}|rejects={}|pages={}|state={:?}\n",
        spec.id, spec.reserve, spec.touches.len(), r.committed, r.rejects, pages, r.state
    )
}

/// 快照头：@VXL1|cases|committed|rejects|sum;
pub fn header_laz(cases: u64, committed: u64, rejects: u64, sum: u32) -> String {
    format!("@VXL1|cases={}|committed={}|rejects={}|sum={:08X};\n", cases, committed, rejects, sum)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LazHeader {
    pub cases: u64,
    pub committed: u64,
    pub rejects: u64,
    pub sum: u32,
}

pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    let mut i = from;
    while i < h.len() {
        if h[i] == needle {
            return Some(i);
        }
        i += 1;
    }
    None
}

pub fn find_sub(h: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    if pat.is_empty() || h.len() < pat.len() {
        return None;
    }
    let mut i = from;
    while i + pat.len() <= h.len() {
        let mut k = 0usize;
        while k < pat.len() && h[i + k] == pat[k] {
            k += 1;
        }
        if k == pat.len() {
            return Some(i);
        }
        i += 1;
    }
    None
}

pub fn parse_dec(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    s.bytes().try_fold(0u64, |v, c| {
        if c.is_ascii_digit() {
            Some(v.checked_mul(10)?.checked_add((c - b'0') as u64)?)
        } else {
            None
        }
    })
}

pub fn parse_hex8(s: &str) -> Option<u32> {
    if s.len() != 8 {
        return None;
    }
    s.bytes().try_fold(0u32, |v, c| match c {
        c @ b'0'..=b'9' => Some((v << 4) | (c - b'0') as u32),
        c @ b'A'..=b'F' => Some((v << 4) | ((c - b'A') as u32 + 10)),
        _ => None,
    })
}

/// 重放账头解析：逐 token 扫描，未知字段即破损。
pub fn parse_header(hdr: &str) -> Option<LazHeader> {
    let mut cases: Option<u64> = None;
    let mut committed: Option<u64> = None;
    let mut rejects: Option<u64> = None;
    let mut sum: Option<u32> = None;
    let bts = hdr.as_bytes();
    let mut i = 0usize;
    while i <= bts.len() {
        let end = match find_byte(bts, i, b'|') {
            Some(p) => p,
            None => bts.len(),
        };
        let tok = &hdr[i..end];
        if !tok.is_empty() {
            let eq = find_byte(tok.as_bytes(), 0, b'=')?;
            let (k, v) = (&tok[..eq], &tok[eq + 1..]);
            match k {
                "cases" => cases = Some(parse_dec(v)?),
                "committed" => committed = Some(parse_dec(v)?),
                "rejects" => rejects = Some(parse_dec(v)?),
                "sum" => sum = Some(parse_hex8(v)?),
                _ => return None,
            }
        }
        if end >= bts.len() {
            break;
        }
        i = end + 1;
    }
    Some(LazHeader { cases: cases?, committed: committed?, rejects: rejects?, sum: sum? })
}

/// 用例重放器：重放序严格单调（只许按 1→20 顺序重放）。
#[derive(Clone, Debug)]
pub struct Replayer {
    /// 下一待重放用例 id（1 起，严格递增）。
    pub cursor: u64,
    pub state: ReplayState,
    pub admin_ops: u64,
    pub payload_ops: u64,
    pub replays: u32,
    /// 对照失真累计。
    pub mismatches: u32,
}

/// 重放器状态机（Idle→Replaying→CrossChecked→Sealed，单调推进）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReplayState {
    /// 空闲：一例未重放。
    Idle,
    /// 重放中：至少一例已重放，尚未完成对照。
    Replaying,
    /// 已对照：20 例重放/深化册全等，账面定案。
    CrossChecked,
    /// 已封盘：账面冻结，任何再重放按倒退拒。
    Sealed,
}

impl ReplayState {
    /// 单调序值：Idle=0 < Replaying=1 < CrossChecked=2 < Sealed=3。
    pub fn rank(self) -> u8 {
        match self {
            ReplayState::Idle => 0,
            ReplayState::Replaying => 1,
            ReplayState::CrossChecked => 2,
            ReplayState::Sealed => 3,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: ReplayState) -> bool {
        to.rank() > self.rank()
    }
}

impl Replayer {
    pub fn new() -> Replayer {
        Replayer { cursor: 1, state: ReplayState::Idle, admin_ops: 0, payload_ops: 0, replays: 0, mismatches: 0 }
    }

    /// 状态推进公开口：倒退/平移显性拒。
    pub fn advance_state(&mut self, to: ReplayState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            return Err(LazyCode::CursorBack.val());
        }
        self.state = to;
        Ok(())
    }

    /// 封盘：账面冻结。
    pub fn seal(&mut self) -> Result<(), u32> { self.advance_state(ReplayState::Sealed) }

    /// 重放一例：id 必须等于游标（跳例/回例一律倒退拒）。
    pub fn replay_one(&mut self, id: u64) -> Result<(CaseSpec, CaseResult), u32> {
        if self.state == ReplayState::Sealed || id != self.cursor {
            return Err(LazyCode::CursorBack.val());
        }
        let spec = derive_case(id);
        let r = replay_case(&spec)?;
        let line = render_case_line(&spec, &r);
        self.admin_ops += 2;
        self.payload_ops += line.len() as u64 * 2 + spec.reserve as u64;
        if self.state == ReplayState::Idle {
            self.state = ReplayState::Replaying;
        }
        self.replays += 1;
        self.cursor = id.checked_add(1).unwrap_or(self.cursor);
        Ok((spec, r))
    }

    /// 双路对照：逐用例重放路径 vs 深化册路径全等。
    pub fn cross_check(&mut self, specs: &[CaseSpec]) -> Result<usize, u32> {
        if self.state == ReplayState::Sealed {
            return Err(LazyCode::CursorBack.val());
        }
        let mut crossed = 0usize;
        for spec in specs.iter().take(CASE_N) {
            let a = replay_case(spec)?;
            let b = canonical_case(spec)?;
            if a != b {
                self.mismatches += 1;
                return Err(LazyCode::CrossMiss.val());
            }
            crossed += 1;
        }
        self.advance_state(ReplayState::CrossChecked)?;
        Ok(crossed)
    }

    /// 开销占比（bps）：簿记 / 载荷；无载荷视为超限。
    pub fn overhead_bps(&self) -> u64 {
        if self.payload_ops == 0 {
            return u64::MAX;
        }
        self.admin_ops.saturating_mul(10_000) / self.payload_ops
    }
}

/// 导出重放账：快照头 + 20 用例行（要求 20 例全过且两路全等）。
pub fn export_ledger(specs: &[CaseSpec]) -> Result<String, u32> {
    if specs.len() < CASE_N {
        return Err(LazyCode::CaseEmpty.val());
    }
    let mut body = String::new();
    let mut sum_committed = 0u64;
    let mut sum_rejects = 0u64;
    for spec in specs.iter().take(CASE_N) {
        let r = replay_case(spec)?;
        if r != canonical_case(spec)? {
            return Err(LazyCode::CrossMiss.val());
        }
        body.push_str(&render_case_line(spec, &r));
        sum_committed += r.committed as u64;
        sum_rejects += r.rejects as u64;
    }
    let hdr = header_laz(CASE_N as u64, sum_committed, sum_rejects, fnv1a(body.as_bytes()));
    let mut out = hdr;
    out.push_str(&body);
    Ok(out)
}

fn body_control_scan(body: &str) -> Result<(), u32> {
    if body.bytes().any(|c| c < 0x20 && c != b'\n') {
        return Err(LazyCode::LedgerBroken.val());
    }
    Ok(())
}

/// 正文行数（'\n' 计数）。
pub fn count_lines(s: &str) -> usize {
    s.bytes().filter(|c| *c == b'\n').count()
}

/// 重放账核验：头字段 + 行数锚 + 正文逐字节对拍 + 校验和 + 全过锚。
pub fn verify_ledger(specs: &[CaseSpec], out: &str) -> Result<(), u32> {
    let ob = out.as_bytes();
    if out.len() < 7 || !out.starts_with("@VXL1|") {
        return Err(LazyCode::LedgerBroken.val());
    }
    let semi = match find_byte(ob, 0, b';') {
        Some(p) => p,
        None => return Err(LazyCode::LedgerBroken.val()),
    };
    let h = match parse_header(&out[6..semi]) {
        Some(h) => h,
        None => return Err(LazyCode::LedgerBroken.val()),
    };
    if h.cases != CASE_N as u64 {
        return Err(LazyCode::LedgerBroken.val());
    }
    let mut sum_committed = 0u64;
    let mut sum_rejects = 0u64;
    let mut expect = String::new();
    for spec in specs.iter().take(CASE_N) {
        let r = replay_case(spec)?;
        sum_committed += r.committed as u64;
        sum_rejects += r.rejects as u64;
        expect.push_str(&render_case_line(spec, &r));
    }
    if h.committed != sum_committed || h.rejects != sum_rejects {
        return Err(LazyCode::CrossMiss.val());
    }
    let mut start = semi + 1;
    if start < out.len() && ob[start] == b'\n' {
        start += 1;
    }
    let body = &out[start..];
    body_control_scan(body)?;
    if body != expect {
        return Err(LazyCode::CrossMiss.val());
    }
    if count_lines(body) != CASE_N {
        return Err(LazyCode::LedgerBroken.val());
    }
    if h.sum != fnv1a(body.as_bytes()) {
        return Err(LazyCode::LedgerBroken.val());
    }
    Ok(())
}

fn push_range(b: &[u8], lo: usize, hi: usize, v: &mut Vec<u8>) {
    let mut k = lo;
    while k <= hi && k < b.len() {
        v.push(b[k]);
        k += 1;
    }
}

fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    let tgt = match find_sub(b, 0, pat) {
        Some(pp) => pp + pat.len(),
        None => usize::MAX,
    };
    for i in 0..(b.len()) {
        if i == tgt {
            let c = b[i];
            v.push(if c == newc { alt } else { newc });
        } else {
            v.push(b[i]);
        }
    }
}

fn header_line_end(b: &[u8]) -> usize {
    match find_byte(b, 0, b';').and_then(|sp| find_byte(b, sp, b'\n')) {
        Some(x) => x,
        None => b.len(),
    }
}

/// 对黄金重放账注入第 kind 路失真，返回坏件（内容全 ASCII，安全）。
pub fn distort(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let he = header_line_end(b);
    match kind % 10 {
        0 => {
            let mut flipped = false;
            for i in 0..(b.len()) {
                if i > he && b[i] != b'\n' && !flipped {
                    v.push(b[i] ^ 0x01);
                    flipped = true;
                } else {
                    v.push(b[i]);
                }
            }
        }
        1 => patch_after(b, b"committed=", b'9', b'1', &mut v),
        2 => patch_after(b, b"sum=", b'0', b'F', &mut v),
        5 => patch_after(b, b"rejects=", b'9', b'1', &mut v),
        7 => patch_after(b, b"cases=", b'0', b'1', &mut v),
        3 => {
            let le = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, le + 1, b.len(), &mut v);
        }
        4 => push_range(b, 0, b.len().saturating_sub(7), &mut v), // 截尾：去末 6 字节
        6 => {
            let l1 = find_byte(b, he + 1, b'\n').unwrap_or(b.len());
            let l2 = find_byte(b, l1 + 1, b'\n').unwrap_or(b.len());
            push_range(b, 0, he, &mut v);
            push_range(b, l1 + 1, l2, &mut v);
            push_range(b, he + 1, l1, &mut v);
            push_range(b, l2 + 1, b.len(), &mut v);
        }
        8 => {
            push_range(b, 0, he, &mut v);
            v.push(0x01);
            push_range(b, he + 1, b.len(), &mut v);
        }
        _ => {
            push_range(b, 0, b.len(), &mut v);
            let tail: &[u8] = b"case=99|reserve=8|touches=1|committed=0|rejects=0|pages=|state=Reserved\n";
            for k in 0..(tail.len()) {
                v.push(tail[k]);
            }
        }
    }
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}

pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 注入轮：每轮对新鲜黄金重放账注入一路失真，核验器必须抓到
///（黄金件本身必须核验通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    for r in 0..rounds {
        let specs = derive_cases();
        let out = export_ledger(&specs)?;
        verify_ledger(&specs, &out)?;
        let kind = (r % 10) as usize;
        led.record(kind, verify_ledger(&specs, &distort(&out, kind)).is_err());
    }
    Ok(led)
}

/// 越界触碰记账探针：越界触碰必须计入 rejects 且页位图不动。
pub fn probe_range_reject() -> bool {
    let mut rg = Region::new(4);
    let r1 = rg.touch(0).is_ok();
    let r2 = rg.touch(4).is_err(); // 越界：页号=预留。
    let r3 = rg.touch(9).is_err(); // 越界：远界。
    let ok = r1 && r2 && r3 && rg.rejects == 2 && rg.committed() == 1;
    let _ = rg.touch(0);
    ok && rg.committed() == 1 && rg.idempotent == 1
}

/// 守恒探针：提交页数恒 ≤ 预留页数（位图与深化册双路互证）。
pub fn probe_conservation() -> bool {
    for k in 1..=(CASE_N as u64) {
        let spec = derive_case(k);
        let (r, c) = match (replay_case(&spec), canonical_case(&spec)) {
            (Ok(r), Ok(c)) => (r, c),
            _ => return false,
        };
        if r.committed > spec.reserve || r != c {
            return false;
        }
    }
    true
}

pub fn header_roundtrip() -> bool {
    let sum = fnv1a(b"probe");
    let hl = header_laz(20, 123, 45, sum);
    let a = match parse_header(&hl[6..hl.len() - 2]) {
        Some(h) => h.cases == 20 && h.committed == 123 && h.rejects == 45 && h.sum == sum,
        None => false,
    };
    let bad = parse_header("cases=20|ghost=1|sum=00000000").is_none()
        && parse_header("cases=x|sum=00000000").is_none()
        && parse_header("cases=20|sum=00").is_none();
    a && bad
}

/// 全链：20 例顺序重放 → 双路对照 → 导出+核验 → 七拒探 → 开销闸。
/// 返回 (正旗数, 拒旗数, 载荷账, 对照例数)。
pub fn golden_run() -> Result<(u32, u32, u64, usize), u32> {
    let specs = derive_cases();
    let mut rp = Replayer::new();
    let rej_skip = rp.replay_one(2).is_err();
    let mut last = CaseResult { committed: 0, rejects: 0, idempotent: 0, state: RegionState::Reserved, pages: Vec::new() };
    for id in 1..=(CASE_N as u64) {
        let (_, r) = rp.replay_one(id)?;
        last = r;
    }
    let crossed = rp.cross_check(&specs)?;
    let rej_back = rp.replay_one(1).is_err();
    let empty_spec = CaseSpec { id: 1, reserve: 8, touches: Vec::new() };
    let rej_empty = replay_case(&empty_spec).is_err();
    rp.seal()?;
    let rej_sealed = rp.replay_one(1).is_err();
    let mut rg = Region::new(2);
    let rej_range = rg.touch(5).is_err();
    let mut rg2 = Region::new(2);
    rg2.mask = 0b111; // 人工注入越界位图：3 页提交 > 2 页预留。
    let rej_reserve = !rg2.conservation_ok();
    let last_canon = canonical_case(specs.last().ok_or(LazyCode::CaseEmpty.val())?)?;
    if last != last_canon {
        return Err(LazyCode::CrossMiss.val());
    }
    let out = export_ledger(&specs)?;
    let rej_tamper = verify_ledger(&specs, &distort(&out, 2)).is_err();
    if rp.overhead_bps() > OVERHEAD_CAP_BPS {
        return Err(LazyCode::OverheadHigh.val());
    }
    let flags = (crossed == CASE_N) as u32 + (rp.mismatches == 0) as u32
        + (verify_ledger(&specs, &out).is_ok()) as u32 + probe_conservation() as u32;
    let rejects = rej_skip as u32 + rej_back as u32 + rej_empty as u32
        + rej_sealed as u32 + rej_range as u32 + rej_reserve as u32 + rej_tamper as u32;
    Ok((flags, rejects, rp.payload_ops, crossed))
}

/// 序 A：黄金全链四正七拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 7, _, _)) => Ok(()),
        _ => Err(LazyCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(LazyCode::CrossMiss.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72F80 && LazyCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = CASE_N == 20 && PAGE_BYTES == 4096 && RESERVE_BASE == 8
        && JUDGE_ROUNDS == 10 && OVERHEAD_CAP_BPS == 200 && in_window;
    if ok { Ok(()) } else { Err(LazyCode::NotReproducible.val()) }
}

pub fn repro_check() -> Result<(), u32> {
    match (golden_run(), golden_run()) {
        (a, b) if a == b => Ok(()),
        _ => Err(LazyCode::NotReproducible.val()),
    }
}

pub fn segment_guard() -> bool {
    for i in 0..(LazyCode::ALL.len()) {
        let v = LazyCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
    }
    true
}

pub fn writeback_summary() -> Result<String, u32> {
    let specs = derive_cases();
    let mut rp = Replayer::new();
    for id in 1..=(CASE_N as u64) {
        let _ = rp.replay_one(id)?;
    }
    let crossed = rp.cross_check(&specs)?;
    let out = export_ledger(&specs)?;
    Ok(format!("状态=已完成 cases={} crossed={} lines={} overhead={}bps replays={}",
        CASE_N, crossed, count_lines(&out), rp.overhead_bps(), rp.replays))
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, p, c)) => (*f == 4 && *r == 7, format!("flags={} rejects={} payload={} crossed={}", f, r, p, c)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-20用例重放全过", gp, gd));
    let specs = derive_cases();
    let mut rp = Replayer::new();
    let cr = rp.cross_check(&specs);
    let cp = matches!(&cr, Ok(n) if *n == CASE_N) && rp.mismatches == 0;
    v.push(("J1-深化册对照100%", cp, format!("crossed={:?} mismatches={}", cr, rp.mismatches)));
    let out = export_ledger(&specs);
    let vp = match &out {
        Ok(o) => verify_ledger(&specs, o).is_ok(),
        Err(_) => false,
    };
    v.push(("J1-重放账可导出", vp, String::from("@VXL1 头+20 用例行，五重闸全过")));
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    let mut rp4 = Replayer::new();
    for k4 in 1..=(CASE_N as u64) {
        let _ = rp4.replay_one(k4);
    }
    let bps = rp4.overhead_bps();
    v.push(("J1-开销占比≤2%", bps <= OVERHEAD_CAP_BPS,
        format!("admin={} payload={} bps={} cap={}", rp4.admin_ops, rp4.payload_ops, bps, OVERHEAD_CAP_BPS)));
    v.push(("J1-越界拒与守恒", probe_range_reject() && probe_conservation(),
        String::from("越界触碰计入 rejects 且位图不动；提交≤预留 20 例全守恒")));
    let hd = String::from("建头→解析逐域对账；未知字段/坏数字/短校验和必败");
    v.push(("J1-快照头校验", header_roundtrip(), hd));
    let mut rp2 = Replayer::new();
    let r1 = rp2.replay_one(2).is_err();
    let _ = rp2.replay_one(1);
    let r2 = rp2.replay_one(1).is_err();
    let _ = rp2.seal();
    let r3 = rp2.replay_one(2).is_err();
    let mut rp3 = Replayer::new();
    let m1 = rp3.advance_state(ReplayState::Replaying).is_ok();
    let m2 = rp3.advance_state(ReplayState::Idle).is_err();
    let m3 = rp3.advance_state(ReplayState::Replaying).is_err();
    let m4 = rp3.advance_state(ReplayState::CrossChecked).is_ok();
    let m5 = rp3.advance_state(ReplayState::Sealed).is_ok();
    v.push(("J2-单调守卫", r1 && r2 && r3 && m1 && m2 && m3 && m4 && m5,
        format!("跳{}回{}封{} 状态机进拒平进封={}/{}/{}/{}/{}", r1, r2, r3, m1, m2, m3, m4, m5)));
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72F80 窗口")));
    v
}

pub fn run_unx5482_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F5482 延迟分配回归账");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
