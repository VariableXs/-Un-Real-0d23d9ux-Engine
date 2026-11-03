//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1924 · PCID 位分配与刷新策略 · 0x72740
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B17）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F1924-J1 位轮转分配/回收/批刷新窗断言全过（位耗尽
//!     即全 flush 回退），分配账守恒（发=收+在用），翻转竞态检出 10/10；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 位空间 12 位（4096 槽），轮转窗 256 槽内循环分配；窗内位耗尽 →
//!     全 flush 回退（flush_all+1、世代+1、位图清零、窗基址推进一窗）；
//!   · 分配账守恒：发（issued）= 收（reclaimed）+ 在用（位图置位数），
//!     逐轮复算零差；回收只收在用位，双回收/野回收/世代错配按竞态拒；
//!   · 批刷新窗：脏位队列 64 槽，满窗即批刷，禁止积压超发；
//!   · 翻转竞态注入 10 路（双回收/野回收/越界回收/世代错配/刷中双收/
//!     重复分配/窗基址倒退/脏队列超发/封账分配/封账回收）10/10 必抓；
//!   · 位图整数实现：4096 槽 = 64 个 u64 字，get/set/clr 全 checked。
//! ═══════════════════════════════════════════════════════════════════════
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
/// 诊断段基址（0x72740，VX3-1103 PCID 窗口独占）。
pub const SEG_BASE: u32 = 0x72740;
/// PCID 位宽（冻结 12 位）。
pub const PCID_BITS: u32 = 12;
/// PCID 位空间（2^12 = 4096 槽）。
pub const PCID_SPACE: usize = 1 << PCID_BITS;
/// 轮转窗容量（冻结 256 槽，窗内循环分配）。
pub const ROT_WINDOW: usize = 256;
/// 批刷新窗容量（冻结 64 槽脏位队列）。
pub const BATCH_WIN: usize = 64;
/// 位图字数（4096 / 64 = 64 字）。
pub const BITMAP_WORDS: usize = PCID_SPACE / 64;
/// 账面纪元（冻结 5）。
pub const EPOCH: u64 = 5;
/// 黄金压测轮数（冻结 512 轮分配-回收混合）。
pub const GOLDEN_ROUNDS: usize = 512;
/// 竞态注入路数（判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PcidCode {
    /// 位倒退：窗基址倒退 / 世代倒退 / 回收序倒退
    CursorBack,
    /// 位耗尽未回退：窗内无位可分时未走全 flush 回退路径
    ExhaustNoFlush,
    /// 分配账不守恒：发 ≠ 收 + 在用
    ConsrvMiss,
    /// 竞态漏检：翻转竞态注入未被抓
    RaceMiss,
    /// 位越界：PCID ≥ 4096 或不在合法位空间
    PcidRange,
    /// 重复分配：同世代同位重复发出
    AllocDup,
    /// 批刷新窗超发：脏队列超过 64 槽仍在积压不入刷
    FlushLate,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}
impl PcidCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PcidCode::CursorBack => "位倒退：窗基址/世代/回收序只进不退，回退一律拒",
            PcidCode::ExhaustNoFlush => "位耗尽未回退：窗内无位必须走全 flush 回退，禁止死等",
            PcidCode::ConsrvMiss => "分配账不守恒：发=收+在用零差被破坏，查回收路径",
            PcidCode::RaceMiss => "竞态漏检：翻转竞态注入未抓，查竞态检出器闭集",
            PcidCode::PcidRange => "位越界：PCID 必须落在 0..4096，越界一律拒",
            PcidCode::AllocDup => "重复分配：同世代同位不得二次发出，先回收再分配",
            PcidCode::FlushLate => "批刷新窗超发：脏位队列满窗必须立即批刷，禁止积压",
            PcidCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [PcidCode; 8] = [
        PcidCode::CursorBack, PcidCode::ExhaustNoFlush, PcidCode::ConsrvMiss, PcidCode::RaceMiss,
        PcidCode::PcidRange, PcidCode::AllocDup, PcidCode::FlushLate, PcidCode::NotReproducible,
    ];
}
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PcidCode::ALL.len() {
        if PcidCode::ALL[i].val() == code { return PcidCode::ALL[i].detail(); }
        i += 1;
    }
    "未知诊断码：不在 0x72740 段闭集内"
}
// ── 位图（整数实现，4096 槽 = 64 字） ────────────────────────────
/// 位图：word = idx / 64，bit = idx % 64，全 checked 访问。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BitMap {
    pub words: Vec<u64>,
    pub set_count: usize,
}
impl BitMap {
    pub fn new() -> BitMap {
        let mut words: Vec<u64> = Vec::new();
        let mut i = 0usize;
        while i < BITMAP_WORDS {
            words.push(0u64);
            i += 1;
        }
        BitMap { words, set_count: 0 }
    }
    pub fn set(&mut self, idx: usize) -> bool {
        match self.cell(idx) {
            Some((cell, bit)) => {
                if *cell & (1u64 << bit) == 0 {
                    *cell |= 1u64 << bit;
                    self.set_count += 1;
                }
                true
            }
            None => false,
        }
    }
    pub fn clr(&mut self, idx: usize) -> bool {
        match self.cell(idx) {
            Some((cell, bit)) => {
                if *cell & (1u64 << bit) != 0 {
                    *cell &= !(1u64 << bit);
                    self.set_count -= 1;
                }
                true
            }
            None => false,
        }
    }
    pub fn get(&self, idx: usize) -> Option<bool> {
        if idx >= PCID_SPACE { return None; }
        match self.words.get(idx / 64) {
            Some(cell) => Some((*cell & (1u64 << ((idx % 64) as u32))) != 0),
            None => None,
        }
    }
    /// 越界守卫的可变字访问（set/clr 共用）：返回 (字, 位) 或 None。
    fn cell(&mut self, idx: usize) -> Option<(&mut u64, u32)> {
        if idx >= PCID_SPACE { return None; }
        let w = idx / 64;
        let bit = (idx % 64) as u32;
        match self.words.get_mut(w) {
            Some(c) => Some((c, bit)),
            None => None,
        }
    }
    pub fn clear_all(&mut self) {
        let mut i = 0usize;
        while i < self.words.len() {
            if let Some(c) = self.words.get_mut(i) {
                *c = 0;
            }
            i += 1;
        }
        self.set_count = 0;
    }
}
// ── 分配器状态机（Idle→Rotating→FlushBackoff→Sealed，单调） ─────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PcidState {
    /// 空闲：一 PCID 未发。
    Idle,
    /// 轮转中：窗内循环分配，脏位批刷随行。
    Rotating,
    /// 回退中：位耗尽触发全 flush，窗基址推进后回到轮转。
    FlushBackoff,
    /// 已封账：任何再分配/回收按倒退拒。
    Sealed,
}
impl PcidState {
    pub fn rank(self) -> u8 {
        match self {
            PcidState::Idle => 0,
            PcidState::Rotating => 1,
            PcidState::FlushBackoff => 2,
            PcidState::Sealed => 3,
        }
    }
    pub fn can_advance(self, to: PcidState) -> bool {
        to.rank() > self.rank()
    }
}
// ── 分配账行（逐轮账：发/收/在用三域守恒） ──────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocRow {
    pub round: usize,
    pub generation: u64,
    pub issued: u64,
    pub reclaimed: u64,
    pub in_use: u64,
    pub flush_all: u64,
    pub flush_batch: u64,
}
impl AllocRow {
    pub fn conserv_ok(&self) -> bool {
        self.issued == self.reclaimed.saturating_add(self.in_use)
    }
    pub fn render(&self, sum: u32) -> String {
        format!(
            "@PCD1|round={}|gen={}|iss={}|rec={}|use={}|fa={}|fb={}|sum={:08X};\n",
            self.round, self.generation, self.issued, self.reclaimed, self.in_use, self.flush_all, self.flush_batch, sum
        )
    }
}
// ── 校验和（FNV-1a 32 位，整数实现） ────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        h = (h ^ (bytes[i] as u32)).wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}
// ── PCID 分配器 ──────────────────────────────────────────────────
/// 分配器：位图（同世代语义，全 flush 清零）+ 窗基址/游标/世代三只进
/// 不退游标 + 全刷/批刷计数 + 发/收三计数控 + 脏位批刷新窗。
#[derive(Clone, Debug)]
pub struct PcidAllocator {
    pub state: PcidState,
    pub epoch: u64,
    pub bm: BitMap,
    pub window_base: usize,
    pub cursor: usize,
    pub generation: u64,
    pub flush_all: u64,
    pub flush_batch: u64,
    pub issued: u64,
    pub reclaimed: u64,
    pub dirty: Vec<usize>,
    pub rejects: u32,
    pub races_caught: u32,
}
impl PcidAllocator {
    pub fn new(epoch: u64) -> PcidAllocator {
        PcidAllocator {
            state: PcidState::Idle, epoch, bm: BitMap::new(), window_base: 0, cursor: 0,
            generation: 0, flush_all: 0, flush_batch: 0, issued: 0, reclaimed: 0,
            dirty: Vec::new(), rejects: 0, races_caught: 0,
        }
    }
    pub fn advance_state(&mut self, to: PcidState) -> Result<(), u32> {
        if !self.state.can_advance(to) {
            self.rejects += 1;
            return Err(PcidCode::CursorBack.val());
        }
        self.state = to;
        Ok(())
    }
    pub fn conserv_check(&self) -> Result<(), u32> {
        if self.issued != self.reclaimed.saturating_add(self.bm.set_count as u64) { return Err(PcidCode::ConsrvMiss.val()); }
        Ok(())
    }
    fn scan_free_in_window(&self) -> Option<usize> {
        let mut step = 0usize;
        while step < ROT_WINDOW {
            let idx = self.window_base.saturating_add((self.cursor + step) % ROT_WINDOW);
            match self.bm.get(idx) {
                Some(false) => return Some(idx),
                Some(true) => {}
                None => return None,
            }
            step += 1;
        }
        None
    }
    pub fn alloc(&mut self) -> Result<usize, u32> {
        if self.state == PcidState::Sealed {
            self.rejects += 1;
            return Err(PcidCode::CursorBack.val());
        }
        let idx = match self.scan_free_in_window() {
            Some(idx) => idx,
            None => {
                // 位耗尽：全 flush 回退（判据原文显性路径）。
                self.flush_all_force()?;
                match self.scan_free_in_window() {
                    Some(i2) => i2,
                    None => return Err(PcidCode::ExhaustNoFlush.val()),
                }
            }
        };
        if !self.bm.set(idx) { return Err(PcidCode::PcidRange.val()); }
        self.issued += 1;
        self.conserv_check()?;
        self.mark_dirty(idx)?;
        if self.state == PcidState::Idle {
            self.state = PcidState::Rotating;
        }
        self.cursor = (self.cursor + 1) % ROT_WINDOW;
        Ok(idx)
    }
    pub fn reclaim(&mut self, idx: usize) -> Result<(), u32> {
        if self.state == PcidState::Sealed {
            self.rejects += 1;
            return Err(PcidCode::CursorBack.val());
        }
        match self.bm.get(idx) {
            None => {
                self.races_caught += 1;
                return Err(PcidCode::PcidRange.val());
            }
            Some(false) => {
                // 野回收（未分配位）或双回收：竞态检出。
                self.races_caught += 1;
                return Err(PcidCode::RaceMiss.val());
            }
            Some(true) => {}
        }
        if !self.bm.clr(idx) { return Err(PcidCode::PcidRange.val()); }
        self.reclaimed += 1;
        self.conserv_check()
    }
    fn mark_dirty(&mut self, idx: usize) -> Result<(), u32> {
        if self.dirty.len() >= BATCH_WIN {
            // 满窗必须先批刷，禁止积压超发。
            self.batch_flush()?;
        }
        self.dirty.push(idx);
        if self.dirty.len() > BATCH_WIN { return Err(PcidCode::FlushLate.val()); }
        Ok(())
    }
    pub fn batch_flush(&mut self) -> Result<usize, u32> {
        let n = self.dirty.len();
        if n > BATCH_WIN { return Err(PcidCode::FlushLate.val()); }
        self.dirty.clear();
        self.flush_batch += 1;
        Ok(n)
    }
    pub fn flush_all_force(&mut self) -> Result<usize, u32> {
        if self.state == PcidState::Sealed {
            self.rejects += 1;
            return Err(PcidCode::CursorBack.val());
        }
        let retired = self.bm.set_count;
        self.bm.clear_all();
        self.dirty.clear();
        self.flush_all += 1;
        self.generation += 1;
        // 窗基址推进一窗（环绕位空间）。
        self.window_base = (self.window_base + ROT_WINDOW) % PCID_SPACE;
        self.cursor = 0;
        self.state = PcidState::FlushBackoff;
        self.reclaimed += retired as u64;
        self.conserv_check()?;
        Ok(retired)
    }
    pub fn guard_back(&mut self, base: usize, gen: u64) -> Result<(), u32> {
        if base < self.window_base || gen < self.generation {
            self.rejects += 1;
            return Err(PcidCode::CursorBack.val());
        }
        Ok(())
    }
    pub fn snap_row(&self, round: usize) -> AllocRow {
        AllocRow {
            round, generation: self.generation, issued: self.issued, reclaimed: self.reclaimed,
            in_use: self.bm.set_count as u64, flush_all: self.flush_all, flush_batch: self.flush_batch,
        }
    }
    pub fn seal(&mut self) -> Result<(), u32> {
        self.advance_state(PcidState::Sealed)
    }
}
// ── 确定性压测剧本 ──────────────────────────────────────────────
/// 轮号 → 动作码（确定性，可复算）：0=分配 1=批刷 2=回收；
/// 7 拍周期（5 分配 / 1 批刷 / 1 回收），净增速 +4/7 轮，
/// 512 轮内在用位必然突破 256 窗 → 触发位耗尽回退。
pub fn act_of(round: usize) -> usize {
    match round % 7 {
        0 | 1 | 2 | 3 | 4 => 0,
        5 => 1,
        _ => 2,
    }
}
pub fn golden_press(epoch: u64) -> Result<(AllocRow, usize, u64, u64), u32> {
    let mut a = PcidAllocator::new(epoch);
    // 压测回放表：已分配未回收位号队列（FIFO 回收序）。
    let mut outstanding: Vec<usize> = Vec::new();
    let mut allocs = 0usize;
    let mut r = 0usize;
    while r < GOLDEN_ROUNDS {
        let prev_gen = a.generation;
        match act_of(r) {
            0 => {
                let idx = a.alloc()?;
                outstanding.push(idx);
                allocs += 1;
            }
            1 => {
                a.batch_flush()?;
            }
            _ => {
                // FIFO 回收：取最早未收位。
                if !outstanding.is_empty() {
                    let idx = outstanding.remove(0);
                    a.reclaim(idx)?;
                }
            }
        }
        // 世代翻即全 flush 发生：旧世代未收位已按守恒口径并入回收账，
        // 回放表必须同步清空（旧位已失效，禁止再收）。
        if a.generation != prev_gen {
            outstanding.clear();
        }
        a.conserv_check()?;
        r += 1;
    }
    let row = a.snap_row(GOLDEN_ROUNDS);
    if !row.conserv_ok() { return Err(PcidCode::ConsrvMiss.val()); }
    Ok((row, allocs, a.flush_all, a.flush_batch))
}
// ── 翻转竞态注入（10 路，检出器 10/10 必抓） ────────────────────
pub fn race_probe(kind: usize) -> bool {
    let mut a = PcidAllocator::new(EPOCH);
    match kind % 10 {
        // 0 双回收 · 1 野回收 · 2 越界回收 · 3 世代错配 · 4 刷中双收
        // · 5 重复分配 · 6 窗基址倒退 · 7 脏队列超发 · 8/9 封账后写。
        0 => {
            let idx = match a.alloc() { Ok(i) => i, Err(_) => return false };
            if a.reclaim(idx).is_err() { return false; }
            let before = a.races_caught;
            a.reclaim(idx).is_err() && a.races_caught == before + 1
        }
        1 => {
            let before = a.races_caught;
            a.reclaim(3).is_err() && a.races_caught == before + 1
        }
        2 => a.reclaim(PCID_SPACE + 1).is_err(),
        3 => {
            let idx = match a.alloc() { Ok(i) => i, Err(_) => return false };
            if a.flush_all_force().is_err() { return false; }
            let before = a.races_caught;
            a.reclaim(idx).is_err() && a.races_caught == before + 1
        }
        4 => {
            let idx = match a.alloc() { Ok(i) => i, Err(_) => return false };
            if a.batch_flush().is_err() || a.reclaim(idx).is_err() { return false; }
            let before = a.races_caught;
            a.reclaim(idx).is_err() && a.races_caught == before + 1
        }
        5 => {
            // 重复分配：位图挡住二次发出，再扫窗必不得再次命中同位。
            let idx = match a.alloc() { Ok(i) => i, Err(_) => return false };
            match a.bm.get(idx) {
                Some(true) => match a.scan_free_in_window() {
                    Some(i2) => i2 != idx || ROT_WINDOW == 1,
                    None => true,
                },
                _ => false,
            }
        }
        6 => {
            let base0 = a.window_base;
            let _ = a.flush_all_force();
            a.guard_back(base0, a.generation).is_err()
        }
        7 => {
            // 手工塞满脏窗后分配必先批刷（刷后队列恰 1 条，不超发）。
            let mut i = 0usize;
            while i < BATCH_WIN {
                a.dirty.push(i);
                i += 1;
            }
            a.dirty.len() == BATCH_WIN
                && match a.alloc() { Ok(_) => a.dirty.len() == 1, Err(_) => false }
        }
        8 => {
            if a.seal().is_err() { return false; }
            a.alloc().is_err() && a.rejects >= 1
        }
        _ => {
            let idx = match a.alloc() { Ok(i) => i, Err(_) => return false };
            if a.seal().is_err() { return false; }
            a.reclaim(idx).is_err()
        }
    }
}
pub fn race_sweep() -> Result<u32, u32> {
    let mut caught = 0u32;
    let mut k = 0u32;
    while k < JUDGE_ROUNDS {
        if !race_probe(k as usize) { return Err(PcidCode::RaceMiss.val()); }
        caught += 1;
        k += 1;
    }
    Ok(caught)
}
// ── 字节查找与解析（账行核验用） ────────────────────────────────
pub fn find_byte(h: &[u8], from: usize, needle: u8) -> Option<usize> {
    let mut i = from;
    while i < h.len() {
        if h[i] == needle { return Some(i); }
        i += 1;
    }
    None
}
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
pub fn parse_line_fields(line: &str) -> Option<([u64; 7], u32)> {
    if !line.starts_with("@PCD1|") { return None; }
    // 行尾 ';' 终结符先行剥离（终结符不属于任何域）。
    let line = match line.strip_suffix(';') {
        Some(l) => l,
        None => line,
    };
    let bts = line.as_bytes();
    let want_keys: [&str; 7] = ["round", "gen", "iss", "rec", "use", "fa", "fb"];
    let mut vals: [u64; 7] = [0; 7];
    let mut kidx = 0usize;
    let mut sumv: Option<u32> = None;
    let mut i = 6usize;
    while i <= bts.len() {
        let end = match find_byte(bts, i, b'|') {
            Some(p) => p,
            None => bts.len(),
        };
        let tok = &line[i..end];
        if !tok.is_empty() {
            let eq = match find_byte(tok.as_bytes(), 0, b'=') {
                Some(e) => e,
                None => return None,
            };
            let (k, v) = (&tok[..eq], &tok[eq + 1..]);
            if k == "sum" {
                sumv = parse_hex8(v);
            } else {
                if kidx >= want_keys.len() || k != want_keys[kidx] { return None; }
                vals[kidx] = parse_dec(v)?;
                kidx += 1;
            }
        }
        if end >= bts.len() {
            break;
        }
        i = end + 1;
    }
    if kidx != want_keys.len() { return None; }
    Some((vals, sumv?))
}
pub fn verify_line_matches(r: &AllocRow, line: &str) -> bool {
    let body = &line[..line.len().saturating_sub(1)];
    match parse_line_fields(body) {
        Some((v, s)) => {
            v[0] == r.round as u64 && v[1] == r.generation && v[2] == r.issued
                && v[3] == r.reclaimed && v[4] == r.in_use && v[5] == r.flush_all
                && v[6] == r.flush_batch && s == fnv1a(r.render(0).as_bytes())
        }
        None => false,
    }
}
pub fn parse_row_roundtrip(r: &AllocRow, sum: u32) -> bool {
    verify_line_matches(r, &r.render(sum))
}
// ── 失真注入（10 路逐字失真，账行文本核） ────────────────────────
/// 域单字节改写：定位 pat 首现处，把其后首个字节改写（保证与原值不同）。
fn patch_after(b: &[u8], pat: &[u8], newc: u8, alt: u8, v: &mut Vec<u8>) {
    // 子串查找内联（pat 首现 → 其后一字节为目标；找不到不动）。
    let mut tgt = usize::MAX;
    if !pat.is_empty() && b.len() >= pat.len() {
        let mut i = 0usize;
        while i + pat.len() <= b.len() && tgt == usize::MAX {
            let mut k = 0usize;
            while k < pat.len() && b[i + k] == pat[k] {
                k += 1;
            }
            if k == pat.len() {
                tgt = i + pat.len();
            }
            i += 1;
        }
    }
    let mut i = 0usize;
    while i < b.len() {
        if i == tgt {
            let c = b[i];
            v.push(if c == newc { alt } else { newc });
        } else {
            v.push(b[i]);
        }
        i += 1;
    }
}
pub fn distort(out: &str, kind: usize) -> String {
    let b = out.as_bytes();
    let mut v: Vec<u8> = Vec::new();
    let le = match find_byte(b, 0, b'\n') {
        Some(p) => p,
        None => b.len(),
    };
    match kind % 10 {
        // 七域各一路 + sum + 截尾 + 控制符。
        0 => patch_after(b, b"round=", b'9', b'1', &mut v),
        1 => patch_after(b, b"gen=", b'9', b'1', &mut v),
        2 => patch_after(b, b"iss=", b'9', b'1', &mut v),
        3 => patch_after(b, b"rec=", b'0', b'9', &mut v),
        4 => patch_after(b, b"use=", b'0', b'7', &mut v),
        5 => patch_after(b, b"fa=", b'0', b'3', &mut v),
        6 => patch_after(b, b"fb=", b'0', b'4', &mut v),
        7 => patch_after(b, b"sum=", b'0', b'F', &mut v),
        8 => {
            // 截尾：去末 6 字节。
            let mut k = 0usize;
            while k + 7 < b.len() {
                v.push(b[k]);
                k += 1;
            }
        }
        _ => {
            // 注控制符：行中插裸 0x01。
            let mut k = 0usize;
            while k < b.len() {
                if k == le / 2 {
                    v.push(0x01);
                }
                v.push(b[k]);
                k += 1;
            }
        }
    }
    match String::from_utf8(v) {
        Ok(s) => s,
        Err(_) => String::new(),
    }
}
pub fn verify_row_text(r: &AllocRow) -> Result<(), u32> {
    let sum = fnv1a(r.render(0).as_bytes());
    let line = r.render(sum);
    if !parse_row_roundtrip(r, sum) { return Err(PcidCode::RaceMiss.val()); }
    if line.is_empty() || !line.starts_with("@PCD1|") { return Err(PcidCode::CursorBack.val()); }
    Ok(())
}
// ── 注入检出账 ──────────────────────────────────────────────────
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
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let (row, _, _, _) = golden_press(EPOCH + r as u64)?;
        verify_row_text(&row)?;
        let kind = (r % 10) as usize;
        let gold_sum = fnv1a(row.render(0).as_bytes());
        let gold = row.render(gold_sum);
        let bad = distort(&gold, kind);
        // 检出口：坏行解析或逐域对账必须失败（黄金行必过，防污染基准）。
        let hit = bad.is_empty() || !verify_line_matches(&row, &bad);
        led.record(kind, hit);
        r += 1;
    }
    Ok(led)
}
// ── 黄金全链 ────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let (row, allocs, fa, fb) = golden_press(EPOCH)?;
    verify_row_text(&row)?;
    let races = race_sweep()?;
    // 四拒探：越界回收 / 状态平移倒退闭集 / 封账后写 / 注入检出。
    let mut a = PcidAllocator::new(EPOCH);
    let range_rej = a.reclaim(PCID_SPACE).is_err();
    let mono = a.advance_state(PcidState::Rotating).is_ok()
        && a.advance_state(PcidState::Idle).is_err()
        && a.advance_state(PcidState::Rotating).is_err()
        && a.advance_state(PcidState::FlushBackoff).is_ok()
        && a.advance_state(PcidState::Rotating).is_err()
        && a.advance_state(PcidState::Sealed).is_ok();
    let sealed_rej = a.alloc().is_err();
    // 位耗尽回退必须发生（512 轮压测 256 窗必然翻窗）。
    let exhaust_ok = row.flush_all >= 1 && row.generation >= 1;
    let flags = (row.conserv_ok() && allocs > 0) as u32 + (races == JUDGE_ROUNDS) as u32
        + exhaust_ok as u32 + (fb > 0 && row.flush_batch == fb) as u32;
    let rejects = range_rej as u32 + mono as u32 + sealed_rej as u32 + inject_rounds(1).is_ok() as u32;
    Ok((flags, rejects, races, fa))
}
// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(PcidCode::NotReproducible.val()),
    }
}
pub fn probe_seq_b() -> Result<(), u32> {
    match race_sweep() {
        Ok(n) if n == JUDGE_ROUNDS => Ok(()),
        _ => Err(PcidCode::RaceMiss.val()),
    }
}
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72740 && PcidCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = PCID_BITS == 12 && PCID_SPACE == 4096 && ROT_WINDOW == 256 && BATCH_WIN == 64
        && BITMAP_WORDS == 64 && EPOCH == 5 && GOLDEN_ROUNDS == 512 && JUDGE_ROUNDS == 10 && in_window;
    if ok {
        Ok(())
    } else {
        Err(PcidCode::NotReproducible.val())
    }
}
// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b { return Err(PcidCode::NotReproducible.val()); }
    Ok(())
}
// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PcidCode::ALL.len() {
        let v = PcidCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 { return false; }
        i += 1;
    }
    true
}
// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let (row, allocs, fa, fb) = golden_press(EPOCH)?;
    verify_row_text(&row)?;
    let races = race_sweep()?;
    Ok(format!(
        "状态=已完成 rounds={} allocs={} gen={} fa={} fb={} races={}/{}",
        row.round, allocs, row.generation, fa, fb, races, JUDGE_ROUNDS
    ))
}
// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, races, fa)) => (*f == 4 && *r == 4, format!("flags={} rejects={} races={} fa={}", f, r, races, fa)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链四正四拒", gp, gd));
    // J1 位轮转分配/回收/批刷新窗断言全过 + 位耗尽全 flush 回退。
    let press = golden_press(EPOCH);
    let (pp, pd) = match &press {
        Ok((row, allocs, fa, fb)) => (
            row.conserv_ok() && *allocs > 0 && *fa >= 1 && *fb >= 1 && row.generation >= 1,
            format!("allocs={} fa={} fb={} gen={}", allocs, fa, fb, row.generation),
        ),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-轮转分配/回收/批刷新窗", pp, pd));
    v.push(("J1-位耗尽全flush回退", matches!(&press, Ok((row, _, _, _)) if row.flush_all >= 1),
        String::from("512 轮压测必翻窗≥1 次")));
    // J1 分配账守恒：发=收+在用（三纪元逐轮复算）。
    let mut conserv_ok = true;
    let mut ep = 0u64;
    while ep < 3 {
        if let Ok((row, _, _, _)) = golden_press(EPOCH + ep) {
            conserv_ok = conserv_ok && row.conserv_ok();
        } else {
            conserv_ok = false;
        }
        ep += 1;
    }
    v.push(("J1-分配账守恒", conserv_ok, String::from("发=收+在用 三纪元零差")));
    // J1 翻转竞态检出 10/10。
    let races = race_sweep();
    let (rp, rd) = match &races {
        Ok(n) => (*n == JUDGE_ROUNDS, format!("{}/{}", n, JUDGE_ROUNDS)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-翻转竞态检出10/10", rp, rd));
    // J1 位图三操作闭集：越界 get/set/clr 全拒 + 计数守恒 + 全清归零。
    let mut bm = BitMap::new();
    let b1 = bm.set(0) && bm.set(4095) && !bm.set(4096) && !bm.set(usize::MAX);
    let b2 = bm.get(0) == Some(true) && bm.get(4095) == Some(true) && bm.get(4096).is_none();
    let b3 = bm.set_count == 2 && {
        let _ = bm.clr(0);
        bm.set_count == 1 && bm.get(0) == Some(false)
    } && {
        // clr 未置位不扣计数。
        let c0 = bm.set_count;
        let _ = bm.clr(0);
        bm.set_count == c0
    };
    let b4 = {
        bm.clear_all();
        bm.set_count == 0 && bm.get(4095) == Some(false)
    };
    v.push(("J1-位图闭集", b1 && b2 && b3 && b4, String::from("越界拒/计数守恒/全清归零")));
    // J1 账行解析往返 + 失真注入 10/10。
    let rt_ok = match golden_press(EPOCH) {
        Ok((row0, _, _, _)) => parse_row_roundtrip(&row0, fnv1a(row0.render(0).as_bytes())),
        Err(_) => false,
    };
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-账行解析往返", rt_ok, String::from("建行→解析逐域对账")));
    v.push(("J1-失真注入10/10", ip, id));
    // J2 单调守卫：状态机四态进拒平封 + 封账写拒 + 窗基址/世代倒退拒。
    let mut a2 = PcidAllocator::new(EPOCH);
    let mono = a2.advance_state(PcidState::Rotating).is_ok()
        && a2.advance_state(PcidState::Idle).is_err()
        && a2.advance_state(PcidState::Rotating).is_err()
        && a2.advance_state(PcidState::FlushBackoff).is_ok()
        && a2.advance_state(PcidState::Rotating).is_err()
        && a2.advance_state(PcidState::FlushBackoff).is_err()
        && a2.advance_state(PcidState::Sealed).is_ok()
        && a2.alloc().is_err() && a2.reclaim(0).is_err() && a2.flush_all_force().is_err();
    // 窗基址倒退拒：先真 flush 推进窗基址，再传旧基址必拒。
    let mut a3 = PcidAllocator::new(EPOCH);
    let _ = a3.flush_all_force();
    let s9 = a3.guard_back(0, a3.generation).is_err();
    v.push(("J2-单调守卫", mono && s9, format!("状态进拒平封+封账写拒={} 倒退拒={}", mono, s9)));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72740 窗口")));
    v
}
pub fn run_unx1924_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1924 PCID 位分配与刷新策略");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}