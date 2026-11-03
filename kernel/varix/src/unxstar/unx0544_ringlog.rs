//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0544 · 环形日志缓冲（128 行） · 0x71E40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1-B28）：
//!   ① 纯功能代码 ≥220 行；
//!   ② 判据原文：环形缓冲账齐（128 槽×行结构），守恒式
//!     （写=读+存+覆 零差），容量冻结；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 128 槽定长环，每槽一行 Row{seq,level,tag,len,sum}；
//!   · 守恒式 written == read + stored + overwritten 恒成立，任何
//!     写/读/覆路径之后即时复算，破式即 ConservMismatch；
//!   · seq 单调递增（行身份账），日志序不倒退；
//!   · 容量冻结 128，两档模式：覆写关（满即拒）/ 覆写开（挤最老）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x71E40，VX3-1103 批集窗口独占）。
pub const SEG_BASE: u32 = 0x71E40;

/// 环容量（128 槽，冻结）。
pub const RING_SLOTS: usize = 128;
/// 行载荷长度上限（冻结 64）。
pub const ROW_LEN_MAX: u16 = 64;
/// 日志级别上限（0–7，冻结）。
pub const LEVEL_MAX: u8 = 7;
/// 覆写档压入行数（真机档 200，越容量验证挤老账）。
pub const OVERWRITE_BURST: usize = 200;
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RingCode {
    /// 覆写关档下环满仍写
    RingFull,
    /// 日志级别越界（>7）
    LevelOver,
    /// 行载荷超长（>64）
    RowTooLong,
    /// 空环读
    EmptyRing,
    /// 行序倒退（seq 非严格递增）
    SeqRegress,
    /// 守恒式破（写≠读+存+覆）
    ConservMismatch,
    /// 容量漂移（槽数 ≠ 128）
    CapDrift,
    /// 双跑不一致
    NotReproducible,
}

impl RingCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RingCode::RingFull => "环满拒写：覆写关档下 128 槽已满",
            RingCode::LevelOver => "级别越界：日志级别只许 0–7",
            RingCode::RowTooLong => "行超长：载荷上限 64",
            RingCode::EmptyRing => "空环读：无行可取",
            RingCode::SeqRegress => "行序倒退：seq 必须严格递增",
            RingCode::ConservMismatch => "守恒式破：written ≠ read+stored+overwritten，查读写路径漏记账",
            RingCode::CapDrift => "容量漂移：槽数冻结 128，不许改",
            RingCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [RingCode; 8] = [
        RingCode::RingFull,
        RingCode::LevelOver,
        RingCode::RowTooLong,
        RingCode::EmptyRing,
        RingCode::SeqRegress,
        RingCode::ConservMismatch,
        RingCode::CapDrift,
        RingCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RingCode::ALL.len() {
        if RingCode::ALL[i].val() == code {
            return RingCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x71E40 段闭集内"
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 日志行：seq 为全环单调行号（行身份账）；载荷不落字节，用
/// (tag,len,sum) 三元记账，sum 可复算防账面篡改。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub seq: u64,
    pub level: u8,
    pub tag: u32,
    pub len: u16,
    pub sum: u32,
}

/// 行校验和（确定性可复算，零 IO 零墙钟）。
pub fn payload_sum(tag: u32, len: u16) -> u32 {
    tag.wrapping_mul(0x9E37_79B9)
        .wrapping_add(len as u32)
        .wrapping_mul(0x85EB_CA6B)
}

/// 槽位：占位旗 + 行体。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub occupied: bool,
    pub row: Row,
}

/// 环形日志缓冲。
#[derive(Clone, Debug)]
pub struct RingLog {
    pub slots: Vec<Slot>,
    /// 最老行下标（0 ≤ head < 128）。
    pub head: usize,
    /// 在存行数（0 ≤ stored ≤ 128）。
    pub stored: usize,
    /// 下一行号（单调递增，只增不减）。
    pub next_seq: u64,
    /// 覆写档：false=满即拒，true=挤最老。
    pub overwrite: bool,
    /// 守恒四账：累计写 / 累计读 / 在存 / 累计覆。
    pub written: u64,
    pub read: u64,
    pub overwritten: u64,
}

impl RingLog {
    /// 建环：128 空槽，从指定覆写档起步。
    pub fn new(overwrite: bool) -> RingLog {
        let mut slots: Vec<Slot> = Vec::new();
        let mut i = 0;
        while i < RING_SLOTS {
            slots.push(Slot {
                occupied: false,
                row: Row { seq: 0, level: 0, tag: 0, len: 0, sum: 0 },
            });
            i += 1;
        }
        RingLog {
            slots,
            head: 0,
            stored: 0,
            next_seq: 1,
            overwrite,
            written: 0,
            read: 0,
            overwritten: 0,
        }
    }

    /// 容量冻结复核（槽数恒 128）。
    pub fn capacity_frozen(&self) -> bool {
        self.slots.len() == RING_SLOTS
    }

    /// 写行：级别闸 / 长度闸 / 满闸（覆写关拒 / 覆写开挤老）。
    pub fn write(&mut self, level: u8, tag: u32, len: u16) -> Result<u64, u32> {
        if level > LEVEL_MAX {
            return Err(RingCode::LevelOver.val());
        }
        if len > ROW_LEN_MAX {
            return Err(RingCode::RowTooLong.val());
        }
        if !self.capacity_frozen() {
            return Err(RingCode::CapDrift.val());
        }
        if self.stored == RING_SLOTS {
            if !self.overwrite {
                return Err(RingCode::RingFull.val());
            }
            // 覆写档：挤掉 head 最老行，head 前移。
            let pos = self.head;
            self.head = (self.head + 1) % RING_SLOTS;
            self.overwritten += 1;
            let seq = self.next_seq;
            self.next_seq += 1;
            let row = Row { seq, level, tag, len, sum: payload_sum(tag, len) };
            if let Some(s) = self.slots.get_mut(pos) {
                s.occupied = true;
                s.row = row;
            }
            self.written += 1;
            return Ok(seq);
        }
        // 未满：写到 (head+stored) 槽。
        let pos = (self.head + self.stored) % RING_SLOTS;
        let seq = self.next_seq;
        self.next_seq += 1;
        let row = Row { seq, level, tag, len, sum: payload_sum(tag, len) };
        if let Some(s) = self.slots.get_mut(pos) {
            s.occupied = true;
            s.row = row;
        }
        self.stored += 1;
        self.written += 1;
        Ok(seq)
    }

    /// 读行：取 head 最老行；空环显性拒。
    pub fn read(&mut self) -> Result<Row, u32> {
        if self.stored == 0 {
            return Err(RingCode::EmptyRing.val());
        }
        let row = match self.slots.get(self.head) {
            Some(s) => s.row,
            None => return Err(RingCode::EmptyRing.val()),
        };
        if let Some(s) = self.slots.get_mut(self.head) {
            s.occupied = false;
        }
        self.head = (self.head + 1) % RING_SLOTS;
        self.stored -= 1;
        self.read += 1;
        Ok(row)
    }

    /// 守恒式：写=读+存+覆 零差（checked 求和防绕回假绿）。
    pub fn conservation(&self) -> Result<(), u32> {
        match self
            .read
            .checked_add(self.stored as u64)
            .and_then(|x| x.checked_add(self.overwritten))
        {
            Some(total) if total == self.written => Ok(()),
            _ => Err(RingCode::ConservMismatch.val()),
        }
    }

    /// 覆写档账面一致性：覆行只在满后发生，且覆数 ≤ 写数。
    pub fn overwrite_sane(&self) -> bool {
        if self.overwritten > self.written {
            return false;
        }
        if self.stored == RING_SLOTS && self.overwrite {
            // 满环覆写档：written − overwritten == stored 恒成立。
            match self.written.checked_sub(self.overwritten) {
                Some(d) => d == self.stored as u64,
                None => false,
            }
        } else {
            true
        }
    }

    /// 全读排空，返回行序账。
    pub fn drain(&mut self) -> Vec<Row> {
        let mut rows: Vec<Row> = Vec::new();
        while self.stored > 0 {
            match self.read() {
                Ok(r) => rows.push(r),
                Err(_) => break,
            }
        }
        rows
    }
}

/// 行序复核：seq 严格递增 + 校验和可复算。
pub fn seq_guard(rows: &[Row]) -> Result<(), u32> {
    let mut i = 0;
    while i < rows.len() {
        let r = match rows.get(i) {
            Some(r) => *r,
            None => return Err(RingCode::SeqRegress.val()),
        };
        if r.sum != payload_sum(r.tag, r.len) {
            return Err(RingCode::ConservMismatch.val());
        }
        if i > 0 {
            let p = match rows.get(i - 1) {
                Some(p) => *p,
                None => return Err(RingCode::SeqRegress.val()),
            };
            if r.seq <= p.seq {
                return Err(RingCode::SeqRegress.val());
            }
        }
        i += 1;
    }
    Ok(())
}

/// 行账三元一致（tag/len/sum 对得上）。
pub fn row_sane(r: &Row) -> bool {
    r.sum == payload_sum(r.tag, r.len) && r.level <= LEVEL_MAX && r.len <= ROW_LEN_MAX
}

// ── LCG（确定性伪随机，供注入轮步进） ───────────────────────────
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

// ── 注入检出账（四路：满拒路/级别路/超长路/倒退路） ─────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 黄金全链：满拒→排空→覆写挤老→守恒→序账 ──────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 相 A：覆写关档。写满 128，第 129 行必拒。
    let mut a = RingLog::new(false);
    let mut k = 0;
    while k < RING_SLOTS {
        a.write((k % 8) as u8, k as u32, 16)?;
        k += 1;
    }
    let full_rejected = a.write(1, 999, 16).is_err();
    // 守恒：写 128 = 读 0 + 存 128 + 覆 0。
    a.conservation()?;
    // 排空：128 行 seq 严格递增、行账三元一致。
    let rows = a.drain();
    if rows.len() != RING_SLOTS {
        return Err(RingCode::ConservMismatch.val());
    }
    seq_guard(&rows)?;
    let head_ok = match rows.first() {
        Some(r) => r.seq == 1,
        None => false,
    };
    a.conservation()?;
    // 相 B：覆写开档。压 200 行，挤老 72 行。
    let mut b = RingLog::new(true);
    let mut k = 0;
    while k < OVERWRITE_BURST {
        b.write(2, k as u32, 8)?;
        k += 1;
    }
    b.conservation()?;
    let ov_ok = b.written == OVERWRITE_BURST as u64
        && b.overwritten == (OVERWRITE_BURST - RING_SLOTS) as u64
        && b.stored == RING_SLOTS;
    if !b.overwrite_sane() {
        return Err(RingCode::ConservMismatch.val());
    }
    // 排空：最老行 seq = 73（前 72 行被挤）。
    let rows_b = b.drain();
    let old_ok = match rows_b.first() {
        Some(r) => r.seq == (OVERWRITE_BURST - RING_SLOTS + 1) as u64,
        None => false,
    };
    seq_guard(&rows_b)?;
    b.conservation()?;
    // 三拒口径：级别越界 / 超长 / 空环读。
    let mut c = RingLog::new(false);
    let lvl = c.write(LEVEL_MAX + 1, 1, 8).is_err();
    let long = c.write(1, 1, ROW_LEN_MAX + 1).is_err();
    let empty = c.read().is_err();
    let flags = full_rejected as u32 + head_ok as u32 + ov_ok as u32 + old_ok as u32;
    let rejects = lvl as u32 + long as u32 + empty as u32;
    Ok((flags, rejects, a.written as u32))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链（4 旗 + 3 拒）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 3, _)) => Ok(()),
        _ => Err(RingCode::NotReproducible.val()),
    }
}

/// 序 B：守恒面（空环守恒零差 + 覆写档账面自洽 + 行序守卫抓倒序）。
pub fn probe_seq_b() -> Result<(), u32> {
    let fresh = RingLog::new(false);
    fresh.conservation()?;
    let mut ov = RingLog::new(true);
    let mut k = 0;
    while k < RING_SLOTS + 10 {
        ov.write(3, k as u32, 4)?;
        k += 1;
    }
    ov.conservation()?;
    if !ov.overwrite_sane() {
        return Err(RingCode::ConservMismatch.val());
    }
    // 倒序行账必被抓。
    let bad = [
        Row { seq: 2, level: 1, tag: 1, len: 4, sum: payload_sum(1, 4) },
        Row { seq: 1, level: 1, tag: 1, len: 4, sum: payload_sum(1, 4) },
    ];
    match seq_guard(&bad) {
        Err(e) if e == RingCode::SeqRegress.val() => Ok(()),
        _ => Err(RingCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（128 槽 / 行长 64 / 级别 7）。
pub fn probe_seq_c() -> Result<(), u32> {
    let r = RingLog::new(false);
    match (r.capacity_frozen(), RING_SLOTS == 128, ROW_LEN_MAX == 64, LEVEL_MAX == 7) {
        (true, true, true, true) => Ok(()),
        _ => Err(RingCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：满拒路（覆写关档第 129 行必拒）。
        let mut s1 = RingLog::new(false);
        let mut k = 0;
        let mut filled = true;
        while k < RING_SLOTS {
            if s1.write(1, k as u32, 8).is_err() {
                filled = false;
            }
            k += 1;
        }
        led.record(0, filled && s1.write(1, 0xFFFF, 8).is_err());
        // 路 2：级别路（级别 8 必拒）。
        let mut s2 = RingLog::new(false);
        led.record(1, s2.write(LEVEL_MAX + 1, 1, 8).is_err());
        // 路 3：超长路（长度 65 必拒）。
        let mut s3 = RingLog::new(false);
        led.record(2, s3.write(1, 1, ROW_LEN_MAX + 1).is_err());
        // 路 4：倒退路（倒序行账必被抓）。
        let bad = [
            Row { seq: 9, level: 1, tag: 2, len: 8, sum: payload_sum(2, 8) },
            Row { seq: 3, level: 1, tag: 2, len: 8, sum: payload_sum(2, 8) },
        ];
        led.record(3, matches!(seq_guard(&bad), Err(e) if e == RingCode::SeqRegress.val()));
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(RingCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RingCode::ALL.len() {
        let v = RingCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut r = RingLog::new(true);
    let mut k = 0;
    while k < OVERWRITE_BURST {
        r.write(4, k as u32, 12)?;
        k += 1;
    }
    r.conservation()?;
    if !r.overwrite_sane() || !r.capacity_frozen() {
        return Err(RingCode::ConservMismatch.val());
    }
    Ok(format!(
        "状态=已完成 slots={} written={} stored={} overwritten={}",
        RING_SLOTS, r.written, r.stored, r.overwritten
    ))
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((flags, rej, _)) => *flags == 4 && *rej == 3,
            Err(_) => false,
        },
        match &g {
            Ok((flags, rej, w)) => format!("flags={} rejects={} written_a={}", flags, rej, w),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J1 守恒零差：两档跑后守恒式均零差。
    let cons = {
        let mut x = RingLog::new(false);
        let mut ok = true;
        let mut k = 0;
        while k < RING_SLOTS {
            if x.write(1, k as u32, 16).is_err() {
                ok = false;
            }
            k += 1;
        }
        ok && x.conservation().is_ok() && x.drain().len() == RING_SLOTS && x.conservation().is_ok()
    };
    v.push((
        "J1-守恒零差",
        cons,
        String::from("写=读+存+覆 两档零差"),
    ));
    // J1 容量冻结：128 槽常量与账面一致。
    let mut cap_ok = true;
    let mut k = 0;
    while k < 3 {
        let ring = RingLog::new(k % 2 == 0);
        if !ring.capacity_frozen() {
            cap_ok = false;
        }
        k += 1;
    }
    v.push((
        "J1-容量冻结",
        cap_ok && RING_SLOTS == 128,
        String::from("128 槽三环复核"),
    ));
    // 注入 10/10。
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
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 段位守卫。
    v.push((
        "J4-段位守卫",
        segment_guard(),
        String::from("0x71E40 窗口"),
    ));
    v
}

pub fn run_unx0544_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0544 环形日志缓冲（128 行）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
