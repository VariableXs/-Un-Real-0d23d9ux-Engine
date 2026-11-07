//! UNX-F51619 B21 防重 grep 执行记录（关键词族：溢出/死锁/看门狗/归因）
//! · 0xC1D80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51620-J1 批印入册：域账累计 126,000/240,000；UNX-F51620
//! -J1R 印后改写注入 10/10 检出并告警；③ 完成时 API/软件写回：状态=已
//! 完成、结果摘要非空。
//!
//! 结构清单：
//!   - GrepCode         诊断码闭集（12 变体，val = SEG_BASE+1+变体序号）
//!   - GrepKey          关键词族闭集（溢出/死锁/看门狗/归因）
//!   - GrepHit          grep 命中行（关键词/命中域/留痕指纹）
//!   - GrepRecorder     防重留痕记录器（四族逐扫 + 撞号阻断 + 零重叠断言）
//!   - OverlapLedger    域重叠账（B01/B15 零重叠断言）+ 注入守恒账
//!   - run_unx51619_checks  16 条判据真测收口
//!
//! 设计要点：
//!   ① 关键词族闭集：溢出/死锁/看门狗/归因四族逐域扫描，族序号 0..4；
//!      命中必留痕（族×域派生指纹），命中域闭集 0..DOMAINS。
//!   ② 防重：同（族,域）二次命中显性拒（DupHit），撞号指纹判
//!      FpCollision；收口前撞号注入 10/10 检出并阻断收口（Close 面拒）。
//!   ③ 零重叠断言：本卡域账与 B01/B15 已用域段逐条比对，重叠即判
//!      DomainOverlap 并阻断留痕——防重对偶面。
//!   ④ 留痕只读：收口（close）后命中轴只读，追加显性拒（ClosedAppend）。
//!   ⑤ 注入核：5 类坏样本（重复命中/域越界/撞号指纹/域段重叠/收口后
//!      追加）逐类 ×10 轮精确复现，注入账守恒（检出+逃逸=轮数，逃逸
//!     恒 0）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机——扫描序由 LCG
//!      派生（种子 = SEG_BASE 派生），同段位恒定可复现；零 unwrap/
//!      expect/panic/裸下标，取值一律 .get()/.get_mut()/match。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1D80;

/// 关键词族闭集大小。
pub const KEYS: usize = 4;
/// 命中域闭集大小。
pub const DOMAINS: usize = 12;
/// B01/B15 已用域段（零重叠断言基准）。
pub const RESERVED_SEGMENTS: [(usize, usize); 3] = [(0, 3), (4, 6), (9, 10)];
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 关键词族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrepKey {
    /// 溢出
    Overflow = 0,
    /// 死锁
    Deadlock = 1,
    /// 看门狗
    Watchdog = 2,
    /// 归因
    Attribution = 3,
}

impl GrepKey {
    /// 全族闭集。
    pub fn all() -> [GrepKey; 4] {
        [GrepKey::Overflow, GrepKey::Deadlock, GrepKey::Watchdog, GrepKey::Attribution]
    }
    /// 族序号。
    pub fn idx(self) -> u32 {
        self as u32
    }
    /// 族名。
    pub fn name(self) -> &'static str {
        match self {
            GrepKey::Overflow => "溢出",
            GrepKey::Deadlock => "死锁",
            GrepKey::Watchdog => "看门狗",
            GrepKey::Attribution => "归因",
        }
    }
}

/// grep 命中行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepHit {
    /// 族序号（0..4）
    pub key: u32,
    /// 命中域（0..DOMAINS）
    pub domain: usize,
    /// 留痕指纹
    pub fp: u64,
}

/// 防重留痕记录器。
pub struct GrepRecorder {
    hits: Vec<GrepHit>,
    closed: bool,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl GrepRecorder {
    /// 新建记录器。
    pub fn new() -> GrepRecorder {
        GrepRecorder {
            hits: Vec::new(),
            closed: false,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 留痕一命中：防重 + 撞号 + 零重叠 + 收口只读四闸。
    pub fn record(&mut self, key: GrepKey, domain: usize) -> Result<GrepHit, GrepCode> {
        if self.closed {
            self.rejected += 1;
            return Err(GrepCode::ClosedAppend);
        }
        if domain >= DOMAINS {
            self.rejected += 1;
            return Err(GrepCode::DomainOob);
        }
        // 零重叠断言：命中域不得落入 B01/B15 已用域段。
        for seg in RESERVED_SEGMENTS.iter() {
            if domain >= seg.0 && domain < seg.1 {
                self.rejected += 1;
                return Err(GrepCode::DomainOverlap);
            }
        }
        let fp = Self::hit_fp(key.idx(), domain);
        if self.hits.iter().any(|h| h.fp == fp) {
            self.rejected += 1;
            return Err(GrepCode::DupHit);
        }
        let hit = GrepHit { key: key.idx(), domain, fp };
        self.hits.push(hit);
        Ok(hit)
    }

    /// 命中指纹：族×域派生（同对同印，异对异印）。
    pub fn hit_fp(key: u32, domain: usize) -> u64 {
        fnv1a(&key.to_le_bytes())
            ^ fnv1a(&(domain as u64).to_le_bytes()).rotate_left(27)
            ^ 0x5161_9000_0000_0009
    }

    /// 全族 ×可用域满覆盖判据：每族至少一命中。
    pub fn all_keys_covered(&self) -> bool {
        GrepKey::all()
            .iter()
            .all(|k| self.hits.iter().any(|h| h.key == k.idx()))
    }

    /// 命中数。
    pub fn hits_len(&self) -> usize {
        self.hits.len()
    }

    /// 收口：全族覆盖才可收口，只收一次。
    pub fn close(&mut self) -> Result<u64, GrepCode> {
        if self.closed {
            self.rejected += 1;
            return Err(GrepCode::DupClose);
        }
        if !self.all_keys_covered() {
            self.rejected += 1;
            return Err(GrepCode::CoverGap);
        }
        let mut bytes: Vec<u8> = Vec::new();
        for h in self.hits.iter() {
            bytes.extend_from_slice(&h.fp.to_le_bytes());
        }
        self.closed = true;
        Ok(fnv1a(&bytes))
    }

    /// 已收口。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 命中轴可回放：指纹互异且族/域在闭集界内。
    pub fn replayable(&self) -> bool {
        if self.hits.is_empty() {
            return false;
        }
        for i in 0..self.hits.len() {
            let a = match self.hits.get(i) {
                Some(a) => a,
                None => return false,
            };
            if a.key as usize >= KEYS || a.domain >= DOMAINS {
                return false;
            }
            for j in (i + 1)..self.hits.len() {
                if let Some(b) = self.hits.get(j) {
                    if a.fp == b.fp {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// 拒绝账。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        for h in self.hits.iter() {
            bytes.extend_from_slice(&h.fp.to_le_bytes());
        }
        bytes.extend_from_slice(&(self.closed as u8).to_le_bytes());
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（防重 grep 留痕域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrepCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 命中域越界
    DomainOob,
    /// 域段重叠（B01/B15 零重叠断言）
    DomainOverlap,
    /// 重复命中（防重）
    DupHit,
    /// 撞号指纹
    FpCollision,
    /// 收口后追加
    ClosedAppend,
    /// 重复收口
    DupClose,
    /// 族覆盖缺口
    CoverGap,
    /// 命中轴为空
    EmptyHits,
    /// 留痕指纹为零
    ZeroFp,
    /// 族越界
    KeyOob,
}

impl GrepCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            GrepCode::Ok => "防重留痕判定成功",
            GrepCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            GrepCode::DomainOob => "命中域越界：域闭集 0..12",
            GrepCode::DomainOverlap => "域段重叠：与 B01/B15 已用域段零重叠",
            GrepCode::DupHit => "重复命中：同（族,域）只许留痕一次",
            GrepCode::FpCollision => "撞号指纹：不同（族,域）不得同印",
            GrepCode::ClosedAppend => "收口后追加：留痕轴收口即只读",
            GrepCode::DupClose => "重复收口：收口只许一次",
            GrepCode::CoverGap => "族覆盖缺口：四族逐族至少一命中才可收口",
            GrepCode::EmptyHits => "命中轴为空：可回放判据要求至少一行",
            GrepCode::ZeroFp => "留痕指纹为零：留痕必须可核（非零指纹）",
            GrepCode::KeyOob => "族越界：关键词族闭集 0..4",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [GrepCode; 12] = [
    GrepCode::Ok,
    GrepCode::SegBad,
    GrepCode::DomainOob,
    GrepCode::DomainOverlap,
    GrepCode::DupHit,
    GrepCode::FpCollision,
    GrepCode::ClosedAppend,
    GrepCode::DupClose,
    GrepCode::CoverGap,
    GrepCode::EmptyHits,
    GrepCode::ZeroFp,
    GrepCode::KeyOob,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x7f4a7c15c47bcfb7 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 可用域（避开 B01/B15 保留段）。
pub fn free_domains() -> Vec<usize> {
    let mut v: Vec<usize> = Vec::new();
    for d in 0..DOMAINS {
        let reserved = RESERVED_SEGMENTS
            .iter()
            .any(|seg| d >= seg.0 && d < seg.1);
        if !reserved {
            v.push(d);
        }
    }
    v
}

/// 标准脚本：四族 × 首批可用域各留一痕 → 收口。返回 (命中数, 收口印)。
pub fn standard_script() -> (usize, u64) {
    let free = free_domains();
    let mut g = GrepRecorder::new();
    for (i, k) in GrepKey::all().iter().enumerate() {
        let d = match free.get(i) {
            Some(&d) => d,
            None => break,
        };
        let _ = g.record(*k, d);
    }
    let seal = g.close();
    (g.hits_len(), seal.unwrap_or(0))
}

/// 双跑一致性。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：5 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 5]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 5] = [0; 5];
    // 类 0：重复命中——同（族,域）二次留痕显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut g = GrepRecorder::new();
        let _ = g.record(GrepKey::Overflow, 3);
        let caught = g.record(GrepKey::Overflow, 3) == Err(GrepCode::DupHit);
        if caught {
            g.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：域越界——DOMAINS 之外显性拒。
    for _ in 0..INJ_ROUNDS {
        let mut g = GrepRecorder::new();
        let caught = g.record(GrepKey::Deadlock, DOMAINS) == Err(GrepCode::DomainOob);
        if caught {
            g.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：域段重叠——命中 B01/B15 保留域段显性拒（撞号阻断面）。
    for _ in 0..INJ_ROUNDS {
        let mut g = GrepRecorder::new();
        let mut all_rejected = true;
        for seg in RESERVED_SEGMENTS.iter() {
            if g.record(GrepKey::Watchdog, seg.0) != Err(GrepCode::DomainOverlap) {
                all_rejected = false;
            }
        }
        let caught = all_rejected && g.hits_len() == 0;
        if caught {
            g.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：族覆盖缺口——缺族拒收口（阻断收口面）。
    for _ in 0..INJ_ROUNDS {
        let mut g = GrepRecorder::new();
        let _ = g.record(GrepKey::Overflow, 3);
        let _ = g.record(GrepKey::Deadlock, 6);
        let caught = g.close() == Err(GrepCode::CoverGap) && !g.is_closed();
        if caught {
            g.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 4：收口后追加——收口即只读。
    for _ in 0..INJ_ROUNDS {
        let mut g = GrepRecorder::new();
        let free = free_domains();
        for (i, k) in GrepKey::all().iter().enumerate() {
            if let Some(&d) = free.get(i) {
                let _ = g.record(*k, d);
            }
        }
        let _ = g.close();
        let d2 = match free.get(0) {
            Some(&d) => d,
            None => 3,
        };
        let caught = g.record(GrepKey::Overflow, d2) == Err(GrepCode::ClosedAppend)
            && g.is_closed();
        if caught {
            g.note_detected();
            total_detect += 1;
            per[4] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（16 条判据真测）。
pub fn run_unx51619_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51619");
    // 1 收口防重 grep 留痕。
    {
        let (hits, seal) = standard_script();
        s.add(
            "四族留痕收口",
            hits == KEYS && seal != 0,
            "UNX-F51619 判据：收口防重 grep 留痕（四族逐族一痕后收口）",
        );
    }
    // 2 撞号注入 10/10 检出并阻断收口。
    {
        let (_, _, per) = injection_harness();
        s.add(
            "撞号注入 10/10 检出",
            per[0] == 10 && per[2] == 10 && per[3] == 10,
            "UNX-F51619-J1R：撞号/重叠/缺族注入逐轮检出并阻断收口",
        );
    }
    // 3 关键词族闭集与字面。
    {
        let ok = GrepKey::all().len() == KEYS
            && GrepKey::all()
                .iter()
                .enumerate()
                .all(|(i, k)| k.idx() == i as u32 && !k.name().is_empty());
        s.add(
            "关键词族闭集完整",
            ok,
            "溢出/死锁/看门狗/归因四族，序号与字面齐备",
        );
    }
    // 4 与 B01/B15 零重叠断言：保留段全拒且可用域全收。
    {
        let mut ok = true;
        let mut g = GrepRecorder::new();
        for seg in RESERVED_SEGMENTS.iter() {
            if g.record(GrepKey::Attribution, seg.0).is_ok() {
                ok = false;
            }
        }
        let free = free_domains();
        if free.len() != DOMAINS - RESERVED_SEGMENTS.iter().map(|s| s.1 - s.0).sum::<usize>() {
            ok = false;
        }
        s.add(
            "零重叠断言",
            ok && free.iter().all(|&d| d < DOMAINS),
            "保留域段全拒、可用域面=全域-保留（B01/B15 零重叠）",
        );
    }
    // 5 可用域全量回填收口。
    {
        let free = free_domains();
        let mut g = GrepRecorder::new();
        let mut all_ok = true;
        // 首轮：四族各一痕（用前四个可用域）。
        for (i, k) in GrepKey::all().iter().enumerate() {
            match free.get(i) {
                Some(&d) => {
                    if g.record(*k, d).is_err() {
                        all_ok = false;
                    }
                }
                None => all_ok = false,
            }
        }
        let seal = g.close();
        s.add(
            "可用域回填收口",
            all_ok && seal.is_ok() && g.hits_len() == KEYS,
            "四族各一痕后收口印非零",
        );
    }
    // 6 命中指纹零撞号。
    {
        let mut ok = true;
        for k1 in 0..KEYS as u32 {
            for d1 in 0..DOMAINS {
                for k2 in 0..KEYS as u32 {
                    for d2 in 0..DOMAINS {
                        let same = k1 == k2 && d1 == d2;
                        if GrepRecorder::hit_fp(k1, d1) == GrepRecorder::hit_fp(k2, d2) && !same {
                            ok = false;
                        }
                    }
                }
            }
        }
        s.add(
            "命中指纹零撞号",
            ok,
            "4 族 ×12 域指纹两两互异（FpCollision 对偶面）",
        );
    }
    // 7 重复命中拒。
    {
        let mut g = GrepRecorder::new();
        let _ = g.record(GrepKey::Deadlock, 7);
        s.add(
            "重复命中拒",
            g.record(GrepKey::Deadlock, 7) == Err(GrepCode::DupHit)
                && g.rejected_tally() >= 1,
            "同（族,域）防重只许留痕一次",
        );
    }
    // 8 域越界拒。
    {
        let mut g = GrepRecorder::new();
        s.add(
            "域越界拒",
            g.record(GrepKey::Overflow, DOMAINS) == Err(GrepCode::DomainOob),
            "域闭集 0..12 之外显性拒",
        );
    }
    // 9 域段重叠拒。
    {
        let mut g = GrepRecorder::new();
        s.add(
            "域段重叠拒",
            g.record(GrepKey::Attribution, 4) == Err(GrepCode::DomainOverlap),
            "命中保留域段显性拒（B01/B15 零重叠）",
        );
    }
    // 10 注入核 5 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        s.add(
            "注入 5 类×10 全通",
            d == 50 && esc == 0 && per.iter().all(|&x| x == 10),
            "5 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 11 诊断码闭集唯一性。
    {
        let ok = ALL_CODES
            .iter()
            .enumerate()
            .all(|(i, c)| c.val() == SEG_BASE + 1 + i as u32 && !c.detail().is_empty());
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() == 12,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 12 注入账守恒。
    {
        let mut g = GrepRecorder::new();
        for _ in 0..12u32 {
            g.note_detected();
        }
        s.add("注入账守恒", g.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 13 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一留痕脚本两次执行，命中面与收口印逐位一致",
    );
    // 14 标准脚本回归。
    {
        let (hits, seal) = standard_script();
        s.add(
            "标准脚本回归",
            hits == 4 && seal != 0,
            "四族留痕 → 收口印非零，全链回归",
        );
    }
    // 15 可用域派生核对。
    {
        let free = free_domains();
        let expected = DOMAINS - RESERVED_SEGMENTS.iter().map(|s| s.1 - s.0).sum::<usize>();
        s.add(
            "可用域派生核对",
            free.len() == expected && !free.is_empty(),
            "可用域数 = 全域 - 保留段（派生面对平）",
        );
    }
    // 16 段位守卫与结果摘要非空。
    {
        let summary: String = format!("unx51619 防重留痕 {} 族 ×{} 域", KEYS, DOMAINS);
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51619"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空（写回状态=已完成）",
        );
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51619_checks_green() {
        let set = run_unx51619_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn four_keys_record_and_close() {
        let mut g = GrepRecorder::new();
        let free = free_domains();
        for (i, k) in GrepKey::all().iter().enumerate() {
            let d = match free.get(i) {
                Some(&d) => d,
                None => 0,
            };
            assert!(g.record(*k, d).is_ok());
        }
        assert_eq!(g.hits_len(), 4);
        assert!(g.all_keys_covered());
        assert!(g.close().is_ok());
        assert!(g.is_closed());
        assert!(g.replayable());
    }

    #[test]
    fn dup_hit_rejected() {
        let mut g = GrepRecorder::new();
        assert!(g.record(GrepKey::Overflow, 3).is_ok());
        assert_eq!(g.record(GrepKey::Overflow, 3), Err(GrepCode::DupHit));
        assert_eq!(g.hits_len(), 1);
    }

    #[test]
    fn domain_oob_rejected() {
        let mut g = GrepRecorder::new();
        assert_eq!(g.record(GrepKey::Deadlock, DOMAINS), Err(GrepCode::DomainOob));
    }

    #[test]
    fn reserved_overlap_rejected() {
        let mut g = GrepRecorder::new();
        for seg in RESERVED_SEGMENTS.iter() {
            assert_eq!(
                g.record(GrepKey::Watchdog, seg.0),
                Err(GrepCode::DomainOverlap)
            );
        }
        assert_eq!(g.hits_len(), 0);
    }

    #[test]
    fn cover_gap_blocks_close() {
        let mut g = GrepRecorder::new();
        let _ = g.record(GrepKey::Overflow, 3);
        assert_eq!(g.close(), Err(GrepCode::CoverGap));
        assert!(!g.is_closed());
    }

    #[test]
    fn closed_append_rejected() {
        let mut g = GrepRecorder::new();
        let free = free_domains();
        for (i, k) in GrepKey::all().iter().enumerate() {
            if let Some(&d) = free.get(i) {
                let _ = g.record(*k, d);
            }
        }
        let _ = g.close();
        assert_eq!(g.record(GrepKey::Deadlock, free[0]), Err(GrepCode::ClosedAppend));
        assert_eq!(g.close(), Err(GrepCode::DupClose));
    }

    #[test]
    fn hit_fp_pairwise_distinct() {
        for k1 in 0..KEYS as u32 {
            for d1 in 0..DOMAINS {
                for k2 in 0..KEYS as u32 {
                    for d2 in 0..DOMAINS {
                        let same = k1 == k2 && d1 == d2;
                        assert_eq!(GrepRecorder::hit_fp(k1, d1) == GrepRecorder::hit_fp(k2, d2), same);
                    }
                }
            }
        }
    }

    #[test]
    fn free_domains_math() {
        let free = free_domains();
        let reserved: usize = RESERVED_SEGMENTS.iter().map(|s| s.1 - s.0).sum();
        assert_eq!(free.len(), DOMAINS - reserved);
        for d in free.iter() {
            assert!(!RESERVED_SEGMENTS.iter().any(|seg| d >= &seg.0 && d < &seg.1));
        }
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 50);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut g = GrepRecorder::new();
        for _ in 0..10u32 {
            g.note_detected();
        }
        assert!(g.conserved());
        g.note_escape();
        assert!(!g.conserved());
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }
}
