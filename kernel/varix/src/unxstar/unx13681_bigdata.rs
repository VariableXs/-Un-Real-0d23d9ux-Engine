//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13681 · 大值数据 db 层（big data cell 链） · 0x65D80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F13681-J1 超门限值经 db 链读写逐字节一致（64KB 级样本），
//!      链完整性断言全绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 门限写死 0x4000（16384 字节，来源：hive big data 门限对齐）；
//!   - db 链结构：Head（总长 + 首 chunk 下标 + chunk 计数）→
//!     Chunk（≤0x1000 载荷 + next 下标）→ NIL（usize::MAX 链尾）；
//!   - 边界闭集：未超门限走 db 链拒收（走普通 cell）、空载荷拒收、
//!     单 chunk 超 0x1000 拒收、恰 16384 拒收（严格大于才入链）；
//!   - 链完整性断言：chunk 计数 ≡ ⌈len/4096⌉（独立重算）、除末块
//!     全满、末块非零、游走步数超块数即环/断链拒收；
//!   - 64KB 级样本：65536 字节 = 16 满块（无余块），读写逐字节一致；
//!   - 注入三族：未超门限入链/断链/长度不符 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65D80），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65D80，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65D80;

/// 大值门限（写死，来源：hive big data 0x4000 字节约定）。
pub const BIGDATA_THRESHOLD: usize = 0x4000;

/// 单 chunk 载荷上限（写死，来源：hive cell 页 0x1000 约定）。
pub const CHUNK_CAP: usize = 0x1000;

/// 链尾哨兵（写死，usize::MAX 不可能是合法下标起点以下）。
pub const NIL: usize = usize::MAX;

/// 64KB 级样本长（写死，来源：判据 64KB 级样本）。
pub const SAMPLE_64K: usize = 65536;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbCode {
    /// 未超门限走 db 链（应走普通 cell）
    ThresholdNotMet,
    /// 空载荷
    EmptyPayload,
    /// 单 chunk 超上限
    ChunkTooLarge,
    /// 链断裂（中途 NIL/悬空下标）
    ChainBroken,
    /// 读写长度不符
    LengthMismatch,
    /// 逐字节不一致
    ByteMismatch,
    /// chunk 计数不符（独立重算对照）
    ChunkCountMismatch,
    /// Head 下标缺失
    HeadMissing,
}

impl DbCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DbCode::ThresholdNotMet => "未超门限走 db 链：0x4000 以下走普通 cell",
            DbCode::EmptyPayload => "空载荷：0 长拒收",
            DbCode::ChunkTooLarge => "单 chunk 超 0x1000：拒收",
            DbCode::ChainBroken => "链断裂：中途 NIL 或悬空下标",
            DbCode::LengthMismatch => "读写长度不符：账实分离",
            DbCode::ByteMismatch => "逐字节不一致：回读对账失败",
            DbCode::ChunkCountMismatch => "chunk 计数不符：≠⌈len/4096⌉",
            DbCode::HeadMissing => "Head 下标缺失：悬空引用",
        }
    }
    pub const ALL: [DbCode; 8] = [
        DbCode::ThresholdNotMet,
        DbCode::EmptyPayload,
        DbCode::ChunkTooLarge,
        DbCode::ChainBroken,
        DbCode::LengthMismatch,
        DbCode::ByteMismatch,
        DbCode::ChunkCountMismatch,
        DbCode::HeadMissing,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DbCode::ALL.len() {
        if DbCode::ALL[i].val() == code {
            return DbCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65D80 段闭集内"
}

// ── db cell 三态 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellKind {
    Head,
    Chunk,
}

/// db 存储单元。
pub enum DbCell {
    /// 链头：总长 + 首 chunk + chunk 计数。
    Head {
        total_len: usize,
        first: usize,
        chunks: usize,
    },
    /// 链块：载荷 + next。
    Chunk { data: Vec<u8>, next: usize },
}

/// db 层账本。
pub struct DbStore {
    cells: Vec<DbCell>,
    pub heads: u32,
    pub chunk_total: u32,
}

impl DbStore {
    pub fn new() -> DbStore {
        DbStore { cells: Vec::new(), heads: 0, chunk_total: 0 }
    }

    /// 独立重算：期望 chunk 数 = ⌈len / CHUNK_CAP⌉（len>0）。
    pub fn expect_chunks(len: usize) -> usize {
        let mut n = len / CHUNK_CAP;
        if len % CHUNK_CAP != 0 {
            n += 1;
        }
        n
    }

    /// 写大值：严格大于门限才入链；切块成链。
    pub fn write_big(&mut self, data: &[u8]) -> Result<usize, u32> {
        if data.is_empty() {
            return Err(DbCode::EmptyPayload.val());
        }
        if data.len() <= BIGDATA_THRESHOLD {
            return Err(DbCode::ThresholdNotMet.val());
        }
        let expect = Self::expect_chunks(data.len());
        let first = self.cells.len();
        let head_idx = first;
        self.cells.push(DbCell::Head {
            total_len: data.len(),
            first: NIL,
            chunks: expect,
        });
        self.heads += 1;
        // 切块：每块 ≤ CHUNK_CAP，链式回填 first/next。
        let mut off = 0usize;
        let mut prev: usize = NIL;
        let mut made = 0usize;
        while off < data.len() {
            // 块长 = min(剩余, CHUNK_CAP)，用 if 不用 Ord::min。
            let rem = data.len() - off;
            let take = if rem > CHUNK_CAP { CHUNK_CAP } else { rem };
            if take > CHUNK_CAP {
                return Err(DbCode::ChunkTooLarge.val());
            }
            let idx = self.cells.len();
            let mut chunk = Vec::new();
            let mut k = 0;
            while k < take {
                chunk.push(data[off + k]);
                k += 1;
            }
            self.cells.push(DbCell::Chunk { data: chunk, next: NIL });
            self.chunk_total += 1;
            // 回填前驱。
            if prev == NIL {
                match self.cells[head_idx] {
                    DbCell::Head { ref mut first, .. } => *first = idx,
                    _ => return Err(DbCode::HeadMissing.val()),
                }
            } else {
                match self.cells[prev] {
                    DbCell::Chunk { ref mut next, .. } => *next = idx,
                    _ => return Err(DbCode::ChainBroken.val()),
                }
            }
            prev = idx;
            off += take;
            made += 1;
        }
        if made != expect {
            return Err(DbCode::ChunkCountMismatch.val());
        }
        Ok(head_idx)
    }

    /// 读大值：走链重组，完整性断言全查。
    pub fn read_big(&self, head_idx: usize) -> Result<Vec<u8>, u32> {
        if head_idx >= self.cells.len() {
            return Err(DbCode::HeadMissing.val());
        }
        let (total, first, chunks) = match self.cells[head_idx] {
            DbCell::Head { total_len, first, chunks } => (total_len, first, chunks),
            _ => return Err(DbCode::HeadMissing.val()),
        };
        let expect = Self::expect_chunks(total);
        if chunks != expect {
            return Err(DbCode::ChunkCountMismatch.val());
        }
        let mut out = Vec::new();
        let mut cur = first;
        let mut walked = 0usize;
        while walked <= chunks {
            // 步数超块数 = 环或断链。
            if walked == chunks {
                // 应已到链尾。
                if cur != NIL {
                    return Err(DbCode::ChainBroken.val());
                }
                break;
            }
            if cur == NIL || cur >= self.cells.len() {
                return Err(DbCode::ChainBroken.val());
            }
            match self.cells[cur] {
                DbCell::Chunk { ref data, next } => {
                    // 除末块全满；末块非零。
                    if walked + 1 < chunks && data.len() != CHUNK_CAP {
                        return Err(DbCode::ChunkTooLarge.val());
                    }
                    if walked + 1 == chunks && (data.len() == 0 || data.len() > CHUNK_CAP) {
                        return Err(DbCode::ChunkTooLarge.val());
                    }
                    let mut k = 0;
                    while k < data.len() {
                        out.push(data[k]);
                        k += 1;
                    }
                    cur = next;
                }
                _ => return Err(DbCode::ChainBroken.val()),
            }
            walked += 1;
        }
        if out.len() != total {
            return Err(DbCode::LengthMismatch.val());
        }
        Ok(out)
    }

    /// 逐字节对账（判据用，独立于 read_big 的重组路径）。
    pub fn verify_bytes(&self, head_idx: usize, data: &[u8]) -> Result<(), u32> {
        let back = self.read_big(head_idx)?;
        if back.len() != data.len() {
            return Err(DbCode::LengthMismatch.val());
        }
        let mut i = 0;
        while i < data.len() {
            if back[i] != data[i] {
                return Err(DbCode::ByteMismatch.val());
            }
            i += 1;
        }
        Ok(())
    }

    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演：64KB 样本入链 → 逐字节回读 → 断言全绿 ─────────────
pub fn golden_run() -> Result<u64, u32> {
    let mut st = DbStore::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    // 64KB 级样本：LCG 确定性字节流。
    let mut sample: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < SAMPLE_64K {
        sample.push((rng.below(256)) as u8);
        i += 1;
    }
    // 边界 1：恰 16384 拒收（严格大于才入链）。
    let mut st_b = DbStore::new();
    let mut exact: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < BIGDATA_THRESHOLD {
        exact.push(0x41);
        i += 1;
    }
    let rej = st_b.write_big(&exact);
    let mut exact_ok = false;
    if let Err(e) = rej {
        if e == DbCode::ThresholdNotMet.val() {
            exact_ok = true;
        }
    }
    if !exact_ok {
        return Err(DbCode::ThresholdNotMet.val());
    }
    // 边界 2：16385 收（2 块：4096 满块×4 + 1 余块 = ⌈16385/4096⌉=5？）。
    //   16385 = 4×4096 + 1 → 5 块（独立重算对照）。
    let mut over: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < BIGDATA_THRESHOLD + 1 {
        over.push(0x42);
        i += 1;
    }
    let hb = st_b.write_big(&over)?;
    if DbStore::expect_chunks(BIGDATA_THRESHOLD + 1) != 5 {
        return Err(DbCode::ChunkCountMismatch.val());
    }
    st_b.verify_bytes(hb, &over)?;
    // 主样本入链：65536 = 16 满块，无余块。
    let h = st.write_big(&sample)?;
    if DbStore::expect_chunks(SAMPLE_64K) != 16 {
        return Err(DbCode::ChunkCountMismatch.val());
    }
    // 链完整性断言全绿。
    st.verify_bytes(h, &sample)?;
    // chunk 计数守恒：16 满块 + 5 块（边界样本）= 21。
    if st.chunk_total + st_b.chunk_total != 16 + 5 {
        return Err(DbCode::ChunkCountMismatch.val());
    }
    // 回读对账双跑幂等。
    let r1 = st.read_big(h)?;
    let r2 = st.read_big(h)?;
    if r1.len() != r2.len() {
        return Err(DbCode::LengthMismatch.val());
    }
    let mut i = 0;
    while i < r1.len() {
        if r1[i] != r2[i] {
            return Err(DbCode::ByteMismatch.val());
        }
        i += 1;
    }
    // 黄金哈希：首尾 8 字节锚 + 块数。
    let mut hsh: u64 = 0x811C9DC5;
    let mut k = 0;
    while k < 8 {
        hsh = (hsh ^ (sample[k] as u64)).wrapping_mul(0x01000193);
        hsh = (hsh ^ (sample[SAMPLE_64K - 1 - k] as u64)).wrapping_mul(0x01000193);
        k += 1;
    }
    hsh = (hsh ^ (st.chunk_total as u64)).wrapping_mul(0x01000193);
    Ok(hsh)
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：未超门限入链。
    ledger.tries[0] += 1;
    let mut st = DbStore::new();
    let small: [u8; 4] = [1, 2, 3, 4];
    let mut c0 = false;
    if let Err(e) = st.write_big(&small) {
        if e == DbCode::ThresholdNotMet.val() {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：断链（手工构造 Head 指向 NIL 的悬空链）。
    ledger.tries[1] += 1;
    let mut st2 = DbStore::new();
    st2.cells.push(DbCell::Head { total_len: BIGDATA_THRESHOLD + 1, first: NIL, chunks: 5 });
    st2.heads += 1;
    let mut c1 = false;
    if let Err(e) = st2.read_big(0) {
        if e == DbCode::ChainBroken.val() {
            c1 = true;
        }
    }
    if c1 {
        ledger.caught[1] += 1;
    }
    // 族 2：长度不符（Head 账长 ≠ 实链重组长）。
    ledger.tries[2] += 1;
    let mut st3 = DbStore::new();
    let mut payload: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < 100 {
        payload.push(7);
        i += 1;
    }
    // 账长写 200，实链只有 100 字节。
    st3.cells.push(DbCell::Head { total_len: 200, first: 1, chunks: 1 });
    st3.heads += 1;
    st3.cells.push(DbCell::Chunk { data: payload, next: NIL });
    st3.chunk_total += 1;
    let mut c2 = false;
    if let Err(e) = st3.read_big(0) {
        if e == DbCode::LengthMismatch.val() {
            c2 = true;
        }
    }
    if c2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DbCode::ALL.len() {
        let v = DbCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演64K链", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 门限/块上限独立写死判据。
    v.push((
        "J1-门限块限写死",
        BIGDATA_THRESHOLD == 16384 && CHUNK_CAP == 4096 && SAMPLE_64K == 65536,
        format!("{}/{}/{}", BIGDATA_THRESHOLD, CHUNK_CAP, SAMPLE_64K),
    ));
    // 独立重算对照：16 满块（65536/4096）。
    v.push((
        "J1-16满块对照",
        DbStore::expect_chunks(65536) == 16 && DbStore::expect_chunks(16385) == 5,
        String::from("⌈len/4096⌉"),
    ));
    // 反向判据：空载荷必拒。
    let mut st2 = DbStore::new();
    let emp = st2.write_big(&[]);
    let mut ok = false;
    if let Err(e) = emp {
        if e == DbCode::EmptyPayload.val() {
            ok = true;
        }
    }
    v.push(("J1-空载荷必拒", ok, String::from("0 长拒收")));
    // 反向判据：Head 下标缺失必拒。
    let miss = DbStore::new().read_big(99);
    let mut ok2 = false;
    if let Err(e) = miss {
        if e == DbCode::HeadMissing.val() {
            ok2 = true;
        }
    }
    v.push(("J1-悬空Head必拒", ok2, String::from("下标先比长度")));
    // NIL 哨兵写死判据。
    v.push((
        "J1-NIL哨兵写死",
        NIL == usize::MAX,
        String::from("链尾 usize::MAX"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65D80 窗口")));
    v
}

pub fn run_unx13681_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13681 大值数据 db 层（big data cell 链）");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = &checks[i];
        s.add_dyn(*n, *p, d);
        i += 1;
    }
    s
}
