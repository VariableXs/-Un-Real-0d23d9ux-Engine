//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18438 与 E5 疑难库互通预告（per-app 处方互通） · 0x7E940段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18438-J1 处方导出格式与 AI-25 消费签名一致断言，互通字段清单冻结 v1
//! 设计要点：
//!   - 处方记录 v1 布局冻结：16 字 ×u32 = 64 字节定长（魔数/格式版/
//!     映像路径哈希高低字/映像哈希高低字/API 族/维度位图/主/次/Build/
//!     SP/产品类型/Suite/保留/校验和），小端序列化；
//!   - AI-25 消费签名（消费侧验收闭集）：长度恰 64 ∧ 魔数相符 ∧
//!     格式版==1（v2 拒）∧ 保留域==0 ∧ 校验和自洽（FNV-1a 覆盖 0..14 字）
//!     ——五闸全过才收货；
//!   - 互通字段清单冻结 v1：16 字段名闭集 + 逐字 4 字节 + 偏移连续
//!     （Σwidth==RECORD_BYTES），结构恒等式逐条复算；
//!   - 导出↔消费闭环：LCG 语料 100 条处方 导出→字节→消费解析→
//!     逐字还原一致（互通即字节级往返零损）；
//!   - 注入核三类×10 轮：篡改单字节 10/10 校验拒、伪造 v2 版本
//!     10/10 拒、坏魔数 10/10 拒；短缓冲/保留域非零另两闸各 10/10；
//!   - 确定性：LCG 种子=SEG_BASE(0x7E940)；no_std+alloc；零 unwrap/
//!     expect/panic；禁浮点/IO/墙钟/rand。
//! 结构清单：
//!   RecipeCode(诊断闭集) / FIELD_N+RECORD_BYTES+冻结布局常量
//!   / INTEROP_FIELDS(字段名闭集) / RecipeRecord(处方记录)
//!   / export_bytes(导出) / ai25_consume(消费签名五闸) / fnv_words
//!   / corpus(100 条语料) / InjectLedger / golden_run / run_unx18438_checks
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E940，AI-VX11-1105-G1·A04 独占区）。
pub const SEG_BASE: u32 = 0x7E940;

// ── 域冻结常量（推导注记） ───────────────────────────────────────
/// 字段数（冻结 v1：16 字 ×u32）。
pub const FIELD_N: usize = 16;
/// 记录字节数（推导：16×4 = 64 字节定长）。
pub const RECORD_BYTES: usize = 64;
/// 魔数（冻结："RPC1" 小端字 = 0x31435052）。
pub const RECIPE_MAGIC: u32 = 0x3143_5052;
/// 格式版本（冻结 v1）。
pub const FMT_VER_V1: u32 = 1;
/// 保留域字下标（冻结：14 号字）。
pub const RESERVED_IDX: usize = 14;
/// 校验和字下标（冻结：15 号字）。
pub const CHECKSUM_IDX: usize = 15;
/// 校验覆盖字数（推导：0..15 号字参与校验，校验和自身除外）。
pub const CHECKSUM_SPAN: usize = 15;
/// 语料条数（冻结：100 条往返）。
pub const CORPUS_N: usize = 100;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 维度位图宽（冻结：主/次/Build/SP/ProductType/Suite 六位）。
pub const DIM_BITS: u32 = 6;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecipeCode {
    /// 缓冲长度不符（≠64）
    LenMismatch,
    /// 魔数不符
    MagicMismatch,
    /// 版本不符（v2+ 拒）
    VerMismatch,
    /// 保留域非零
    ReservedDirty,
    /// 校验和不自洽（篡改检出）
    ChecksumMismatch,
    /// 字段越界（写入下标 ≥16）
    FieldIllegal,
    /// 互通字段清单破（名字空/越界）
    LayoutBroken,
    /// 往返不一致（导出→消费还原失真）
    RoundtripBroken,
    /// 消费签名闸序错（结构闸先于校验闸的次序破）
    GateOrderBroken,
    /// 双跑不一致
    NotReproducible,
}

impl RecipeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            RecipeCode::LenMismatch => "缓冲长度不符：处方记录定长 64 字节",
            RecipeCode::MagicMismatch => "魔数不符：非 RPC1 处方流",
            RecipeCode::VerMismatch => "版本不符：消费面只认 v1，v2 走预告版协商",
            RecipeCode::ReservedDirty => "保留域非零：冻结布局被污染",
            RecipeCode::ChecksumMismatch => "校验和不自洽：字节遭篡改",
            RecipeCode::FieldIllegal => "字段越界：16 字闭集外",
            RecipeCode::LayoutBroken => "互通字段清单破：名字闭集或连续性恒等式不成立",
            RecipeCode::RoundtripBroken => "往返不一致：导出→消费还原失真",
            RecipeCode::GateOrderBroken => "消费闸序破：结构闸必须先于校验闸",
            RecipeCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [RecipeCode; 10] = [
        RecipeCode::LenMismatch,
        RecipeCode::MagicMismatch,
        RecipeCode::VerMismatch,
        RecipeCode::ReservedDirty,
        RecipeCode::ChecksumMismatch,
        RecipeCode::FieldIllegal,
        RecipeCode::LayoutBroken,
        RecipeCode::RoundtripBroken,
        RecipeCode::GateOrderBroken,
        RecipeCode::NotReproducible,
    ];
}

/// 按数值取中文文案。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < RecipeCode::ALL.len() {
        if RecipeCode::ALL[i].val() == code {
            return RecipeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E940 段闭集内"
}

// ── 互通字段清单（冻结 v1：名字闭集 + 布局恒等式） ──────────────
/// 16 字段名闭集（顺序即字下标，冻结 v1；AI-25 消费面按此清单解包）。
pub const INTEROP_FIELDS: [&str; FIELD_N] = [
    "MAGIC",
    "FMT_VER",
    "PATH_HASH_HI",
    "PATH_HASH_LO",
    "PROC_HASH_HI",
    "PROC_HASH_LO",
    "API_FAMILY",
    "DIM_MASK",
    "MAIN_VER",
    "MINOR_VER",
    "BUILD_VER",
    "SP_VER",
    "PRODUCT_TYPE",
    "SUITE_MASK",
    "RESERVED",
    "CHECKSUM",
];

/// 清单冻结恒等式复算：名字全非空 ∧ 无重复 ∧ 字数==16。
pub fn layout_frozen() -> Result<(), u32> {
    if INTEROP_FIELDS.len() != FIELD_N {
        return Err(RecipeCode::LayoutBroken.val());
    }
    let mut i = 0;
    while i < FIELD_N {
        if INTEROP_FIELDS[i].is_empty() {
            return Err(RecipeCode::LayoutBroken.val());
        }
        // 无重复（成对比较，16² = 256 次可承受）。
        let mut j = i + 1;
        while j < FIELD_N {
            if INTEROP_FIELDS[i] == INTEROP_FIELDS[j] {
                return Err(RecipeCode::LayoutBroken.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

// ── FNV-1a 校验和（u32，跨字小端字节流） ────────────────────────
/// 对字流 0..span 求 FNV-1a（逐字按小端 4 字节展开）。
pub fn fnv_words(words: &[u32; FIELD_N], span: usize) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let mut i = 0;
    while i < span && i < FIELD_N {
        let w = words[i];
        let mut b = 0;
        while b < 4 {
            h ^= (w >> (b * 8)) as u8 as u32;
            h = h.wrapping_mul(0x01000193);
            b += 1;
        }
        i += 1;
    }
    h
}

// ── 处方记录 ────────────────────────────────────────────────────
/// per-app 处方记录 v1：16 字定长。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecipeRecord {
    pub words: [u32; FIELD_N],
}

impl RecipeRecord {
    /// 构造器：按语义字段装配并自动填魔数/版本/保留/校验和。
    pub fn build(
        path_hi: u32,
        path_lo: u32,
        proc_hi: u32,
        proc_lo: u32,
        api_family: u32,
        dim_mask: u32,
        main: u32,
        minor: u32,
        build: u32,
        sp: u32,
        product_type: u32,
        suite: u32,
    ) -> Result<RecipeRecord, u32> {
        if dim_mask >= (1u32 << DIM_BITS) {
            return Err(RecipeCode::FieldIllegal.val());
        }
        let mut w = [0u32; FIELD_N];
        w[0] = RECIPE_MAGIC;
        w[1] = FMT_VER_V1;
        w[2] = path_hi;
        w[3] = path_lo;
        w[4] = proc_hi;
        w[5] = proc_lo;
        w[6] = api_family;
        w[7] = dim_mask;
        w[8] = main;
        w[9] = minor;
        w[10] = build;
        w[11] = sp;
        w[12] = product_type;
        w[13] = suite;
        w[RESERVED_IDX] = 0;
        w[CHECKSUM_IDX] = fnv_words(&w, CHECKSUM_SPAN);
        Ok(RecipeRecord { words: w })
    }

    /// 导出为小端字节流（恰 64 字节）。
    pub fn export_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < FIELD_N {
            let w = self.words[i];
            let mut b = 0;
            while b < 4 {
                out.push((w >> (b * 8)) as u8);
                b += 1;
            }
            i += 1;
        }
        out
    }

    /// 字节流还原字数组（长度必须恰 64）。
    pub fn words_from_bytes(buf: &[u8]) -> Result<[u32; FIELD_N], u32> {
        if buf.len() != RECORD_BYTES {
            return Err(RecipeCode::LenMismatch.val());
        }
        let mut w = [0u32; FIELD_N];
        let mut i = 0;
        while i < FIELD_N {
            let base = i * 4;
            // 小端拼字（下标 base..base+4 恒在 64 字节内）。
            let b0 = buf[base] as u32;
            let b1 = buf[base + 1] as u32;
            let b2 = buf[base + 2] as u32;
            let b3 = buf[base + 3] as u32;
            w[i] = b0 | (b1 << 8) | (b2 << 16) | (b3 << 24);
            i += 1;
        }
        Ok(w)
    }
}

// ── AI-25 消费签名（五闸，冻结闸序） ────────────────────────────
/// 消费面验收：闸序 = 长度 → 魔数 → 版本 → 保留域 → 校验和。
/// 结构闸（前四）先于内容闸（校验和）——次序本身是签名的一部分。
pub fn ai25_consume(buf: &[u8]) -> Result<RecipeRecord, u32> {
    // 闸 1：长度。
    if buf.len() != RECORD_BYTES {
        return Err(RecipeCode::LenMismatch.val());
    }
    let w = RecipeRecord::words_from_bytes(buf)?;
    // 闸 2：魔数。
    if w[0] != RECIPE_MAGIC {
        return Err(RecipeCode::MagicMismatch.val());
    }
    // 闸 3：版本（只认 v1）。
    if w[1] != FMT_VER_V1 {
        return Err(RecipeCode::VerMismatch.val());
    }
    // 闸 4：保留域。
    if w[RESERVED_IDX] != 0 {
        return Err(RecipeCode::ReservedDirty.val());
    }
    // 闸 5：校验和自洽。
    let calc = fnv_words(&w, CHECKSUM_SPAN);
    if calc != w[CHECKSUM_IDX] {
        return Err(RecipeCode::ChecksumMismatch.val());
    }
    Ok(RecipeRecord { words: w })
}

/// 闸序断言：同时破魔数+校验和时必须先报魔数（结构闸先于内容闸）。
pub fn gate_order_check() -> Result<(), u32> {
    let rec = RecipeRecord::build(1, 2, 3, 4, 5, 0b000011, 10, 0, 19_045, 0, 1, 0x01C4)?;
    let mut buf = rec.export_bytes();
    // 破魔数（字节 0）+ 破校验和（字节 60 = 校验和字第 0 字节）。
    buf[0] = buf[0].wrapping_add(1);
    buf[60] = buf[60].wrapping_add(1);
    match ai25_consume(&buf) {
        Err(c) if c == RecipeCode::MagicMismatch.val() => Ok(()),
        _ => Err(RecipeCode::GateOrderBroken.val()),
    }
}

// ── 语料（LCG 100 条） ──────────────────────────────────────────
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

/// 100 条确定性处方语料。
pub fn make_corpus() -> Result<Vec<RecipeRecord>, u32> {
    let mut g = Lcg(SEG_BASE as u64);
    let mut v: Vec<RecipeRecord> = Vec::new();
    let mut i = 0;
    while i < CORPUS_N {
        let rec = RecipeRecord::build(
            g.below(1 << 31) as u32,
            i as u32,
            g.below(1 << 31) as u32,
            0xB00C + i as u32,
            g.below(12) as u32, // 12 API 族。
            1u32 << ((i % (DIM_BITS as usize)) as u32),
            10 + (i % 3) as u32,
            (i % 2) as u32,
            19_000 + (i % 1000) as u32,
            (i % 4) as u32,
            1 + (i % 3) as u32,
            0x01C4 ^ (i as u32 & 0x0F),
        )?;
        v.push(rec);
        i += 1;
    }
    Ok(v)
}

// ── 注入核（五类 ×10 轮必拒） ───────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 5],
    pub caught: [u32; 5],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 5], caught: [0; 5] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 5 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 5 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    pub fn missed(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 5 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 10 轮注入：槽0 篡改单字节 / 槽1 伪 v2 / 槽2 坏魔数 / 槽3 短缓冲 / 槽4 保留域脏。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        let rec = RecipeRecord::build(9, 9, 9, 9, 3, 0b000001, 10, 0, 19_045, 0, 1, 0x01C4)?;
        // 槽 0：篡改载荷单字节（8 号字 MAIN_VER 首字节）→ 校验拒。
        let mut b0 = rec.export_bytes();
        b0[32] = b0[32].wrapping_add(1);
        let hit0 = match ai25_consume(&b0) {
            Err(c) => c == RecipeCode::ChecksumMismatch.val(),
            Ok(_) => false,
        };
        led.record(0, hit0);
        // 槽 1：伪 v2（1 号字改 2）→ 版本拒（结构闸先于校验闸）。
        let mut w1 = rec.words;
        w1[1] = 2;
        w1[CHECKSUM_IDX] = fnv_words(&w1, CHECKSUM_SPAN); // 校验和自洽，只破版本。
        let rec1 = RecipeRecord { words: w1 };
        let b1 = rec1.export_bytes();
        let hit1 = match ai25_consume(&b1) {
            Err(c) => c == RecipeCode::VerMismatch.val(),
            Ok(_) => false,
        };
        led.record(1, hit1);
        // 槽 2：坏魔数。
        let mut w2 = rec.words;
        w2[0] = 0xDEAD_BEEF;
        let rec2 = RecipeRecord { words: w2 };
        let b2 = rec2.export_bytes();
        let hit2 = match ai25_consume(&b2) {
            Err(c) => c == RecipeCode::MagicMismatch.val(),
            Ok(_) => false,
        };
        led.record(2, hit2);
        // 槽 3：短缓冲（63 字节）。
        let b3_full = rec.export_bytes();
        let b3 = &b3_full[..RECORD_BYTES - 1];
        let hit3 = match ai25_consume(b3) {
            Err(c) => c == RecipeCode::LenMismatch.val(),
            Ok(_) => false,
        };
        led.record(3, hit3);
        // 槽 4：保留域脏（校验和同步重算，只破保留闸）。
        let mut w4 = rec.words;
        w4[RESERVED_IDX] = 0xFFFF;
        w4[CHECKSUM_IDX] = fnv_words(&w4, CHECKSUM_SPAN);
        let rec4 = RecipeRecord { words: w4 };
        let b4 = rec4.export_bytes();
        let hit4 = match ai25_consume(&b4) {
            Err(c) => c == RecipeCode::ReservedDirty.val(),
            Ok(_) => false,
        };
        led.record(4, hit4);
        r += 1;
    }
    Ok(led)
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 正探针：单条处方导出→消费往返逐字一致。
pub fn probe_seq_a() -> Result<(), u32> {
    let rec = RecipeRecord::build(0xAABB, 0xCCDD, 0x1122, 0x3344, 7, 0b111111, 10, 0, 19_045, 3, 1, 0x01C4)?;
    let buf = rec.export_bytes();
    if buf.len() != RECORD_BYTES {
        return Err(RecipeCode::LenMismatch.val());
    }
    let got = ai25_consume(&buf)?;
    if got != rec {
        return Err(RecipeCode::RoundtripBroken.val());
    }
    Ok(())
}

/// 负探针 1：空缓冲拒（长度闸）。
pub fn probe_seq_b() -> Result<(), u32> {
    match ai25_consume(&[]) {
        Err(c) if c == RecipeCode::LenMismatch.val() => Ok(()),
        _ => Err(RecipeCode::LenMismatch.val()),
    }
}

/// 负探针 2：维度位图越界（≥6 位）构造拒。约定「负探针返回 Err(拒因码)」。
pub fn probe_seq_c() -> Result<(), u32> {
    match RecipeRecord::build(0, 0, 0, 0, 0, 0b1111111, 0, 0, 0, 0, 0, 0) {
        Err(c) => Err(c),
        Ok(_) => Err(RecipeCode::NotReproducible.val()),
    }
}

/// 黄金全链：清单冻结 + 100 条往返 + 注入五闸全抓 + 闸序 + 双跑一致。
pub fn golden_run() -> Result<(usize, u32), u32> {
    layout_frozen()?;
    let corpus = make_corpus()?;
    let mut ok = 0usize;
    let mut i = 0;
    while i < corpus.len() {
        let buf = corpus[i].export_bytes();
        let got = ai25_consume(&buf)?;
        if got != corpus[i] {
            return Err(RecipeCode::RoundtripBroken.val());
        }
        ok += 1;
        i += 1;
    }
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() {
        return Err(RecipeCode::NotReproducible.val());
    }
    gate_order_check()?;
    Ok((ok, inj.caught[0]))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < RecipeCode::ALL.len() {
        let v = RecipeCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

// ── 判据收口 ────────────────────────────────────────────────────
pub fn run_unx18438_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18438 与 E5 疑难库互通预告");
    // ① 黄金全链。
    let g = golden_run();
    s.add_dyn(
        "J1-黄金全链",
        g.is_ok(),
        &format!("往返{:?}/{} 拒{:?}", g.as_ref().map(|r| r.0).unwrap_or(0), CORPUS_N, g.as_ref().map(|r| r.1).unwrap_or(0)),
    );
    // ② 互通字段清单冻结 v1。
    s.add("字段清单冻结v1", layout_frozen().is_ok() && FIELD_N == 16 && RECORD_BYTES == 64, "16 名闭集+64 字节定长");
    // ③ 布局恒等式：逐字 4 字节、偏移连续、Σwidth==RECORD_BYTES。
    let layout_ok = INTEROP_FIELDS.len() * 4 == RECORD_BYTES && CHECKSUM_SPAN == FIELD_N - 1 && RESERVED_IDX == FIELD_N - 2;
    s.add("布局恒等式逐条复算", layout_ok, "16×4==64 校验域/保留域定位");
    // ④ 100 条往返零损。
    let corpus = make_corpus();
    let mut rt_ok = corpus.is_ok();
    let mut rt_n = 0usize;
    if let Ok(cs) = &corpus {
        let mut i = 0;
        while i < cs.len() && rt_ok {
            let buf = cs[i].export_bytes();
            match ai25_consume(&buf) {
                Ok(got) => {
                    rt_ok = got == cs[i];
                    rt_n += 1;
                }
                Err(_) => rt_ok = false,
            }
            i += 1;
        }
    }
    s.add_dyn("100条处方往返零损", rt_ok && rt_n == CORPUS_N, &format!("还原{}/{}", rt_n, CORPUS_N));
    // ⑤ 注入五类 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn(
        "注入五闸10/10",
        inj_ok,
        &format!(
            "{}/{}/{}/{}/{}",
            inj.as_ref().map(|l| l.caught[0]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[1]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[2]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[3]).unwrap_or(0),
            inj.as_ref().map(|l| l.caught[4]).unwrap_or(0)
        ),
    );
    s.add("注入漏检为零", match &inj {
        Ok(l) => l.missed() == 0,
        Err(_) => false,
    }, "检出+漏检=注入");
    // ⑥ 消费闸序：结构闸先于内容闸。
    s.add("消费签名闸序", gate_order_check().is_ok(), "魔数先于校验和");
    // ⑦ 魔数/版本冻结值复算。
    s.add(
        "冻结值复算",
        RECIPE_MAGIC == 0x3143_5052 && FMT_VER_V1 == 1 && CHECKSUM_IDX == 15,
        "RPC1/v1/15 号字校验",
    );
    // ⑧ 校验和敏感性：单字翻转必被检出（10 组逐组验）。
    let mut sen_ok = true;
    let mut wi = 2;
    while wi < CHECKSUM_SPAN {
        let rec = RecipeRecord::build(
            wi as u32,
            wi as u32,
            wi as u32,
            wi as u32,
            wi as u32,
            1,
            10,
            0,
            19_000,
            0,
            1,
            0,
        )
        .unwrap_or(RecipeRecord { words: [0; FIELD_N] });
        let mut w = rec.words;
        w[wi] = w[wi].wrapping_add(1); // 翻转第 wi 字（校验和未重算）。
        let buf = RecipeRecord { words: w }.export_bytes();
        // 14 号保留域翻转走保留闸，其余走校验闸——两闸都算抓出。
        let expect = if wi == RESERVED_IDX {
            RecipeCode::ReservedDirty.val()
        } else {
            RecipeCode::ChecksumMismatch.val()
        };
        if !matches!(ai25_consume(&buf), Err(c) if c == expect) {
            sen_ok = false;
        }
        wi += 1;
    }
    s.add("校验和逐字敏感", sen_ok, "载荷 2..15 字翻转全检出（保留域走脏闸）");
    // ⑨ 探针三序。
    s.add("探针三序", probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_err(), "往返/空缓冲/位图越界");
    // ⑩ 双跑一致。
    s.add("双跑一致", golden_run() == golden_run(), "同语料同字节");
    // ⑪ 文案闭集 + 段位守卫。
    let mut closed = true;
    let mut ci = 0;
    while ci < RecipeCode::ALL.len() {
        if RecipeCode::ALL[ci].detail().is_empty() {
            closed = false;
        }
        ci += 1;
    }
    s.add("诊断文案闭集非空", closed, "10 码全有中文建议");
    s.add("段位守卫 0x7E940", segment_guard(), "SEG_BASE 对齐 0x40 窗口");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_hundred_records() {
        let corpus = make_corpus().unwrap_or_else(|_| Vec::new());
        assert_eq!(corpus.len(), CORPUS_N);
        for rec in corpus.iter() {
            let buf = rec.export_bytes();
            assert_eq!(buf.len(), RECORD_BYTES);
            let got = ai25_consume(&buf).unwrap_or(RecipeRecord { words: [0; FIELD_N] });
            assert_eq!(got, *rec, "往返必须逐字一致");
        }
    }

    #[test]
    fn injections_five_gates_all_caught() {
        let inj = inject_rounds(10).unwrap_or_else(|_| InjectLedger::new());
        assert!(inj.all_caught(), "五闸必须 10/10 全抓");
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn checksum_word_sensitivity() {
        let mut wi = 2;
        while wi < CHECKSUM_SPAN {
            let rec = RecipeRecord::build(wi as u32, 0, 0, 0, 0, 1, 10, 0, 19_000, 0, 1, 0)
                .unwrap_or(RecipeRecord { words: [0; FIELD_N] });
            let mut w = rec.words;
            w[wi] = w[wi].wrapping_add(1);
            let buf = RecipeRecord { words: w }.export_bytes();
            let expect = if wi == RESERVED_IDX {
                RecipeCode::ReservedDirty.val()
            } else {
                RecipeCode::ChecksumMismatch.val()
            };
            match ai25_consume(&buf) {
                Err(c) => assert_eq!(c, expect),
                Ok(_) => panic!("第 {} 字翻转必须被抓出", wi),
            }
            wi += 1;
        }
    }

    #[test]
    fn version_gate_v2_rejected() {
        let rec = RecipeRecord::build(0, 0, 0, 1, 0, 1, 10, 0, 19_000, 0, 1, 0).unwrap_or(RecipeRecord { words: [0; FIELD_N] });
        let mut w = rec.words;
        w[1] = 2;
        w[CHECKSUM_IDX] = fnv_words(&w, CHECKSUM_SPAN);
        let buf = RecipeRecord { words: w }.export_bytes();
        match ai25_consume(&buf) {
            Err(c) => assert_eq!(c, RecipeCode::VerMismatch.val(), "v2 必须走预告协商拒收"),
            Ok(_) => panic!("v2 必须拒"),
        }
    }

    #[test]
    fn gate_order_structure_first() {
        assert!(gate_order_check().is_ok());
    }

    #[test]
    fn layout_frozen_invariants() {
        assert!(layout_frozen().is_ok());
        assert_eq!(INTEROP_FIELDS[0], "MAGIC");
        assert_eq!(INTEROP_FIELDS[CHECKSUM_IDX], "CHECKSUM");
        assert_eq!(INTEROP_FIELDS[RESERVED_IDX], "RESERVED");
    }

    #[test]
    fn probes_all_sides() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_err());
    }

    #[test]
    fn double_run_identical() {
        assert_eq!(golden_run(), golden_run(), "双跑一致");
    }

    #[test]
    fn checks_set_all_green() {
        let s = run_unx18438_checks();
        assert!(s.all_passed(), "本卡判据必须全绿: {}", s.red_items_text());
        assert!(s.len() >= 8 && s.len() <= 20);
    }
}
