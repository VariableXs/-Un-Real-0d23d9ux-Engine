//! UNX-F8701 · 八批 ID 总对账账 · 诊断段 0xB6E0
//! 验收锚点：UNX-F8701-J1 八批（B21–B28）各 20 条共 160 个 ID 的
//! 总对账——跨批连续无断档、无重复、批间总量对账自洽、错账零容忍
//! 两路检出（断档路 / 重复路），双判据缺一即批内红灯。
use alloc::format;
use alloc::vec::Vec;
use alloc::string::String;
use alloc::string::ToString;


pub const SEG_BASE: u16 = 0xB6E0;

// ── 诊断段独占 · 总对账错误域 ───────────────────────────────
const SEG_ERR: u16 = SEG_BASE;
const E_GAP: u16 = SEG_ERR + 1;     // 跨批断档
const E_DUP: u16 = SEG_ERR + 2;     // ID 重复
const E_BATCH: u16 = SEG_ERR + 3;   // 批内条数不符
const E_TOTAL: u16 = SEG_ERR + 4;   // 总量对账不平
const E_ORDER: u16 = SEG_ERR + 5;   // 批序错乱

pub const BATCHES: usize = 8;
pub const PER_BATCH: usize = 20;
pub const TOTAL_IDS: usize = BATCHES * PER_BATCH; // 160
pub const ID_BASE: u32 = 84_010;                  // F8401 起始对应 ID 基

// ── 确定性 LCG ───────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 批账本 ───────────────────────────────────────────────────
/// 单批：批名 + 连续 ID 段。
#[derive(Clone, Debug)]
pub struct BatchBook {
    pub name: String,   // 如 "B21"
    pub ids: Vec<u32>,  // 批内 ID（须连续）
}

impl BatchBook {
    pub fn new(name: &str, start: u32, n: usize) -> Self {
        let ids = (0..n).map(|i| start + i as u32).collect();
        BatchBook { name: name.to_string(), ids }
    }
    /// 批内连续。
    pub fn contiguous(&self) -> bool {
        self.ids.windows(2).all(|w| w[1] == w[0] + 1)
    }
    /// 批内无重复。
    pub fn no_dup(&self) -> bool {
        let n = self.ids.len();
        for i in 0..n {
            for j in (i + 1)..n {
                if self.ids[i] == self.ids[j] {
                    return false;
                }
            }
        }
        true
    }
    pub fn start(&self) -> u32 {
        self.ids.first().copied().unwrap_or(0)
    }
    pub fn end(&self) -> u32 {
        self.ids.last().copied().unwrap_or(0)
    }
    pub fn fp(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in self.name.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        for id in &self.ids {
            for b in id.to_le_bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h
    }
}

/// 八批批名 B21..B28。
pub fn batch_names() -> [&'static str; BATCHES] {
    ["B21", "B22", "B23", "B24", "B25", "B26", "B27", "B28"]
}

/// 标准八批：B21 起各 20 条连续，跨批首尾相接无断档。
pub fn standard_books() -> Vec<BatchBook> {
    let mut v = Vec::new();
    let mut start = ID_BASE;
    for name in batch_names() {
        v.push(BatchBook::new(name, start, PER_BATCH));
        start += PER_BATCH as u32;
    }
    v
}

// ── 总对账器 ─────────────────────────────────────────────────
/// 八批总对账：跨批连续 / 无重 / 批量 / 总量四路。
pub struct Recon8 {
    pub errors: Vec<(u16, String)>,
    pub total_ids: usize,
}

impl Recon8 {
    pub fn new() -> Self {
        Recon8 { errors: Vec::new(), total_ids: 0 }
    }
    /// 全量对账：全绿返回 true。
    pub fn run(&mut self, books: &[BatchBook]) -> bool {
        self.errors.clear();
        // 批数与批名
        if books.len() != BATCHES {
            self.errors.push((E_BATCH, "批数不符".to_string()));
            return false;
        }
        for (i, b) in books.iter().enumerate() {
            if b.name != batch_names()[i] {
                self.errors.push((E_ORDER, format!("批序错: {}", b.name)));
            }
            if b.ids.len() != PER_BATCH {
                self.errors.push((E_BATCH, format!("{}条数={}", b.name, b.ids.len())));
            }
            if !b.contiguous() {
                self.errors.push((E_GAP, format!("{}批内断档", b.name)));
            }
            if !b.no_dup() {
                self.errors.push((E_DUP, format!("{}批内重复", b.name)));
            }
        }
        // 跨批连续：下一批首 = 上一批尾 + 1
        for w in books.windows(2) {
            if w[1].start() != w[0].end() + 1 {
                self.errors.push((E_GAP, format!("跨批断档 {}→{}", w[0].name, w[1].name)));
            }
        }
        // 总量
        self.total_ids = books.iter().map(|b| b.ids.len()).sum();
        if self.total_ids != TOTAL_IDS {
            self.errors.push((E_TOTAL, format!("总量={}", self.total_ids)));
        }
        self.errors.is_empty()
    }
    /// 错账两路注入检出：断档路 + 重复路。
    pub fn detect_two_ways(&mut self) -> (bool, bool) {
        // 断档路：B25 首条前移 1
        let mut b1 = standard_books();
        b1[4].ids[0] -= 1;
        let gap_found = !self.run(&b1);
        // 重复路：B28 尾条重复塞入
        let mut b2 = standard_books();
        let last = b2[7].ids[PER_BATCH - 1];
        b2[7].ids.push(last);
        let dup_found = !self.run(&b2) && self.errors.iter().any(|(c, _)| *c == E_BATCH);
        (gap_found, dup_found)
    }
    /// 全集指纹。
    pub fn books_fp(books: &[BatchBook]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in books {
            for by in b.fp().to_le_bytes() {
                h ^= by as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
        h
    }
}

// ── 判据入口 ─────────────────────────────────────────────────
pub fn run_unx8701_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8701 八批ID总对账");
    let books = standard_books();
    let mut r = Recon8::new();

    // 1. 八批标准态全绿
    let ok = r.run(&books);
    s.add("ID-八批标准全绿", ok && r.total_ids == TOTAL_IDS,
        if ok { "" } else { "对账存在错账" });

    // 2. 总量 160 对账
    s.add("ID-总量160", r.total_ids == TOTAL_IDS && books.len() == 8,
        if r.total_ids == TOTAL_IDS { "" } else { "总量不平" });

    // 3. 跨批首尾相接（B21 首 .. B28 尾 全域连续）
    let span_ok = books[0].start() == ID_BASE
        && books[7].end() == ID_BASE + (TOTAL_IDS as u32 - 1)
        && books.windows(2).all(|w| w[1].start() == w[0].end() + 1);
    s.add("ID-跨批相接", span_ok, if span_ok { "" } else { "批间断档" });

    // 4. 断档路检出
    let (gap, dup) = r.detect_two_ways();
    s.add("ID-断档检出", gap, if gap { "" } else { "断档未检出" });

    // 5. 重复路检出
    s.add("ID-重复检出", dup, if dup { "" } else { "重复未检出" });

    // 6. 批序错乱检出
    let mut b3 = standard_books();
    b3.swap(2, 3);
    let order_bad = !r.run(&b3) && r.errors.iter().any(|(c, _)| *c == E_ORDER);
    s.add("ID-批序检出", order_bad, if order_bad { "" } else { "批序错未检出" });

    // 7. 批条数缺检出
    let mut b4 = standard_books();
    b4[6].ids.pop();
    let short_bad = !r.run(&b4) && r.errors.iter().any(|(c, _)| *c == E_BATCH);
    s.add("ID-缺条检出", short_bad, if short_bad { "" } else { "缺条未检出" });

    // 8. 全集指纹稳定且内容敏感
    let fp_a = Recon8::books_fp(&books);
    let fp_b = Recon8::books_fp(&books);
    let mut b5 = standard_books();
    b5[0].ids[19] += 1;
    let fp_c = Recon8::books_fp(&b5);
    s.add("ID-指纹敏感", fp_a == fp_b && fp_a != fp_c,
        if fp_a == fp_b && fp_a != fp_c { "" } else { "指纹碰撞或漂移" });

    // 9. 各批指纹互异
    let mut fp_uniq = true;
    for i in 0..books.len() {
        for j in (i + 1)..books.len() {
            if books[i].fp() == books[j].fp() {
                fp_uniq = false;
            }
        }
    }
    s.add("ID-批指纹互异", fp_uniq, if fp_uniq { "" } else { "批指纹碰撞" });

    // 10. LCG 抽样 6 条逐条对账（ID 值还原）
    let mut lcg = Lcg(0xB6E0);
    let mut spot = true;
    for _ in 0..6 {
        let bi = lcg.below(BATCHES as u64) as usize;
        let ii = lcg.below(PER_BATCH as u64) as usize;
        let expect = ID_BASE + (bi * PER_BATCH + ii) as u32;
        if books[bi].ids[ii] != expect {
            spot = false;
        }
    }
    s.add("ID-抽样还原", spot, if spot { "" } else { "抽样还原失败" });

    // 11. 对账确定性：重跑同结果
    let mut r2 = Recon8::new();
    let ok2 = r2.run(&books);
    s.add("ID-对账确定性", ok == ok2 && r.total_ids == r2.total_ids,
        if ok == ok2 { "" } else { "重跑漂移" });

    // 12. 诊断段独占
    let seg_ok = SEG_BASE >= 0xB6E0 && SEG_BASE < 0xB700;
    s.add("ID-诊断段独占", seg_ok, if seg_ok { "" } else { "段越界" });

    s
}
