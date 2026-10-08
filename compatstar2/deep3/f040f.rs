//! F040 深化批次四 · 账本开放导出面（compatstar2/deep3 · G-A-40）。
//!
//! 批次一~三覆盖 JSONL 序列化/季报/季度差分/攻坚名单/三源表决/快照链；
//! 本批补齐【功能定义】账本「开放数据」全语义对齐的导出/容错面：
//! canonical 序列化器（固定字段序 + 定长缓冲，同数据两次序列化逐字节
//! 一致——确定性对拍）、导出解析 round-trip（序列化 → 解析 → 再序列化
//! 字节一致——开放格式自证无损）、字段白名单过滤（未列入白名单字段
//! 剔除并记账——最小披露纪律）、节尺寸预算与校验尾标（各节字节数账 +
//! FNV-1a 尾标验证——导出文件损坏如实报错）。
//!
//! 判据对账：主册 G-A-40【设计细节】「账本数据文件版本化（季度快照
//! 不可变，历史可溯）」+【状态与异常】损坏检出显性化；canonical 序列
//! 为开放数据通行惯例（域内量化口径，无 MS 面——如实注明）。
//! 零堆纪律：定长导出缓冲 + 定长条目表，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）

/// 导出条目容量（单批 8 条——域内模型口径）。
pub const MAX_ENTRIES: usize = 8;
/// 导出缓冲定长。
pub const EXPORT_CAP: usize = 512;
/// 条目节预算 256B（节尺寸预算纪律——超支显性化）。
pub const SECTION_BUDGET: usize = 256;
/// 帧头字节数：magic(2)+ver(1)+mask(1)。
pub const HEADER_BYTES: usize = 4;
/// 条目计数字节数（u16 LE）。
pub const COUNT_BYTES: usize = 2;
/// 帧尾标字节数（FNV-1a 32 位）。
pub const TAIL_BYTES: usize = 4;
/// 字段标签：id（canonical 序第一位）。
pub const TAG_ID: u8 = 1;
/// 字段标签：cat（第二位）。
pub const TAG_CAT: u8 = 2;
/// 字段标签：ts（第三位）。
pub const TAG_TS: u8 = 3;
/// 字段标签：score（第四位）。
pub const TAG_SCORE: u8 = 4;
/// 白名单槽位数（四字段定长）。
pub const FIELDS: usize = 4;
/// 导出格式版本。
pub const EXPORT_VER: u8 = 1;
/// FNV-1a 32 位偏移基（公开参考参数）。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
/// FNV-1a 32 位素数（公开参考参数）。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// FNV-1a 32 位（帧尾标校验公共底座）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// 白名单（下标 0..4 依次对应 id/cat/ts/score）。
pub type Whitelist = [bool; FIELDS];

/// 全字段白名单（开放导出缺省口径）。
pub const fn all_fields() -> Whitelist {
    [true; FIELDS]
}

/// 白名单掩码还原（帧头 mask 位 → 白名单；解析侧对拍用）。
pub fn unmask(m: u8) -> Whitelist {
    [(m & 1) != 0, (m & 2) != 0, (m & 4) != 0, (m & 8) != 0]
}

fn mask_of(wl: &Whitelist) -> u8 {
    (wl[0] as u8) | ((wl[1] as u8) << 1) | ((wl[2] as u8) << 2) | ((wl[3] as u8) << 3)
}

/// 账本条目（四字段：id/cat/ts/score）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerEntry { pub id: u16, pub cat: u8, pub ts: u32, pub score: u8 }

/// 单条目序列化体长（长度前缀除外；白名单决定哪些 TLV 入帧）。
fn entry_body_size(wl: &Whitelist) -> usize {
    let mut s = 0usize;
    if wl[0] { s += 2 + 2; }
    if wl[1] { s += 2 + 1; }
    if wl[2] { s += 2 + 4; }
    if wl[3] { s += 2 + 1; }
    s
}

// ---------------------------------------------------------------------------
// canonical 序列化器

/// canonical 序列化器：固定字段序（id→cat→ts→score，TLV 逐字段）+
/// 定长缓冲——同数据两次序列化逐字节一致（确定性对拍）。
pub struct Exporter {
    buf: [u8; EXPORT_CAP],
    /// 有效字节长（含尾标）。
    pub len: usize,
    /// 白名单外被剔除的字段计数（记账——不静默丢字段）。
    pub filtered: u32,
}

impl Exporter {
    pub const fn new() -> Self {
        Exporter { buf: [0; EXPORT_CAP], len: 0, filtered: 0 }
    }

    /// 有效字节切片（round-trip 对拍用）。
    pub fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    fn put_tlv(&mut self, n: usize, tag: u8, val: u32, width: usize) -> Result<usize, &'static str> {
        if n + 2 + width > EXPORT_CAP - TAIL_BYTES {
            return Err("export-over");
        }        self.buf[n] = tag;
        self.buf[n + 1] = width as u8;
        for k in 0..width {
            self.buf[n + 2 + k] = (val >> (8 * k)) as u8;
        }
        Ok(n + 2 + width)
    }

    /// 序列化条目集：头(魔数/版本/白名单掩码) + 计数 + 条目节(长度
    /// 前缀 + TLV 流) + 尾标。返回总长；白名单外字段剔除并 filtered
    /// 记账（最小披露——零静默）。
    pub fn serialize(
        &mut self,
        entries: &[Option<LedgerEntry>; MAX_ENTRIES],
        wl: &Whitelist,
    ) -> Result<usize, &'static str> {
        self.filtered = 0;
        self.buf[0] = b'L';
        self.buf[1] = b'E';
        self.buf[2] = EXPORT_VER;
        self.buf[3] = mask_of(wl);
        let mut n = HEADER_BYTES;
        let mut count = 0usize;
        for slot in entries.iter() {
            if slot.is_some() { count += 1; }
        }
        self.buf[n] = count as u8;
        self.buf[n + 1] = 0;
        n += COUNT_BYTES;
        for slot in entries.iter() {
            let e = match slot { Some(x) => *x, None => continue };
            self.buf[n] = entry_body_size(wl) as u8;
            n += 1;
            // canonical 字段序：id → cat → ts → score（不随输入顺序变化）。
            let fields = [
                (TAG_ID, e.id as u32, 2usize),
                (TAG_CAT, e.cat as u32, 1usize),
                (TAG_TS, e.ts, 4usize),
                (TAG_SCORE, e.score as u32, 1usize),
            ];
            for (k, (tag, val, width)) in fields.iter().enumerate() {
                if wl[k] {
                    n = self.put_tlv(n, *tag, *val, *width)?;
                } else {
                    self.filtered += 1;
                }
            }
        }
        let tail = fnv1a(&self.buf[..n]).to_le_bytes();
        self.buf[n..n + TAIL_BYTES].copy_from_slice(&tail);
        n += TAIL_BYTES;
        self.len = n;
        Ok(n)
    }

    /// 解析导出帧 →（条目集, 条数）：魔数/版本/尾标全验——损坏显性
    /// 报错（不静默）。被白名单剔除的字段按 0 值回读（缺省语义）。
    pub fn parse(&self) -> Result<([Option<LedgerEntry>; MAX_ENTRIES], usize), &'static str> {
        if self.len < HEADER_BYTES + COUNT_BYTES + TAIL_BYTES { return Err("frame-short"); }
        if self.buf[0] != b'L' || self.buf[1] != b'E' { return Err("bad-magic"); }
        if self.buf[2] != EXPORT_VER { return Err("bad-version"); }
        let tail_off = self.len - TAIL_BYTES;
        let expect = fnv1a(&self.buf[..tail_off]);
        let got = u32::from_le_bytes([self.buf[tail_off], self.buf[tail_off + 1], self.buf[tail_off + 2], self.buf[tail_off + 3]]);
        if expect != got { return Err("tail-mismatch"); }
        let _wl = unmask(self.buf[3]);
        let count = self.buf[HEADER_BYTES] as usize;
        if count > MAX_ENTRIES { return Err("bad-count"); }
        let mut out: [Option<LedgerEntry>; MAX_ENTRIES] = [None; MAX_ENTRIES];
        let mut n = HEADER_BYTES + COUNT_BYTES;
        for i in 0..count {
            let blen = self.buf[n] as usize;
            let mut p = n + 1;
            let end = p + blen;
            let mut e = LedgerEntry { id: 0, cat: 0, ts: 0, score: 0 };
            while p + 2 <= end {
                let tag = self.buf[p];
                let width = self.buf[p + 1] as usize;
                let mut val = 0u32;
                for k in 0..width {
                    val |= (self.buf[p + 2 + k] as u32) << (8 * k);
                }
                match tag {
                    TAG_ID => e.id = val as u16,
                    TAG_CAT => e.cat = val as u8,
                    TAG_TS => e.ts = val,
                    TAG_SCORE => e.score = val as u8,
                    _ => return Err("bad-tag"),
                }
                p += 2 + width;
            }
            out[i] = Some(e);
            n = end;
        }
        Ok((out, count))
    }
}

// ---------------------------------------------------------------------------
// 节尺寸预算账

/// 节尺寸账：头/计数/条目/尾标四节字节数 + 条目节超支判定
/// （条目节 > 256B 预算即超支——明细随节数如实列出）。
pub fn section_ledger(count: usize, body_per_entry: usize) -> ([usize; 4], usize, bool) {
    let entries_bytes = count.saturating_mul(body_per_entry + 1); // +1 长度前缀
    let secs = [HEADER_BYTES, COUNT_BYTES, entries_bytes, TAIL_BYTES];
    let total: usize = secs.iter().sum();
    (secs, total, entries_bytes > SECTION_BUDGET)
}

// ---------------------------------------------------------------------------
// 域自检（深化批次四）

/// 域自检（深化批次四）。
pub fn run_f040f_checks() -> CheckSet {
    let mut cs = CheckSet::new("F040-ledgerexp-d4");
    let mut entries: [Option<LedgerEntry>; MAX_ENTRIES] = [None; MAX_ENTRIES];
    entries[0] = Some(LedgerEntry { id: 1, cat: 3, ts: 1_700_000_001, score: 77 });
    entries[1] = Some(LedgerEntry { id: 2, cat: 5, ts: 1_700_000_002, score: 40 });
    // 1) canonical 确定性：同数据两次序列化逐字节一致。
    let mut e1 = Exporter::new();
    let mut e2 = Exporter::new();
    let n1 = e1.serialize(&entries, &all_fields()).unwrap();
    let n2 = e2.serialize(&entries, &all_fields()).unwrap();
    cs.add("canonical_deterministic", n1 == n2 && e1.bytes() == e2.bytes(), "");
    // 2) 固定字段序：头 4B + 计数 2B 后首 TLV tag == TAG_ID。
    cs.add(
        "canonical_field_order",
        e1.bytes()[0] == b'L' && e1.bytes()[1] == b'E'
            && e1.bytes()[HEADER_BYTES + COUNT_BYTES + 1] == TAG_ID,
        "",
    );
    // 3) 导出解析 round-trip：序列化 → 解析 → 再序列化字节一致。
    let (back, count) = e1.parse().unwrap();
    let mut e3 = Exporter::new();
    let n3 = e3.serialize(&back, &all_fields()).unwrap();
    cs.add(
        "roundtrip_bytes_equal",
        count == 2 && n3 == n1 && e3.bytes() == e1.bytes() && back[0] == entries[0],
        "",
    );
    // 4) 字段白名单过滤：score 不列入 → 回读缺省 0（原值 77 不外泄）。
    let mut wl = all_fields();
    wl[3] = false;
    let mut e4 = Exporter::new();
    let _ = e4.serialize(&entries, &wl).unwrap();
    let (back4, _) = e4.parse().unwrap();
    cs.add(
        "whitelist_filter",
        back4[0].unwrap().score == 0
            && back4[0].unwrap().id == 1
            && back4[0].unwrap().ts == 1_700_000_001,
        "",
    );
    // 5) 过滤记账：2 条 × 剔除 1 字段 = filtered == 2（零静默）。
    cs.add("filtered_ledger", e4.filtered == 2, "");
    // 6) 校验尾标：翻转条目节一个字节 → parse Err("tail-mismatch")。
    let mut corrupt = Exporter::new();
    corrupt.buf = e1.buf;
    corrupt.len = e1.len;
    corrupt.buf[10] ^= 0x01;
    cs.add(
        "tail_verify",
        matches!(corrupt.parse(), Err("tail-mismatch")) && matches!(e1.parse(), Ok(_)),
        "",
    );
    // 7) 魔数错显性报错。
    let mut e6 = Exporter::new();
    let _ = e6.serialize(&entries, &all_fields()).unwrap();
    e6.buf[0] = b'X';
    cs.add("bad_magic_explicit", matches!(e6.parse(), Err("bad-magic")), "");
    // 8) 节尺寸账：2 条全字段 → 条目节 34B ≤ 256 预算，总长 4+2+34+4 = 44。
    let body = entry_body_size(&all_fields());
    let (secs, total, over) = section_ledger(2, body);
    cs.add("section_budget_ok", secs[2] == 34 && total == 44 && !over && body == 16, "");
    // 9) 节预算超支：20 条 × 10B 体 → 条目节 260 > 256，超支显性。
    let (_, _, over2) = section_ledger(20, entry_body_size(&all_fields()));
    cs.add("section_over_budget", over2, "");
    // 10) FNV-1a 已知向量：空串 = 偏移基（尾标底座可复算）。
    cs.add("fnv_known_vector", fnv1a(&[]) == FNV_OFFSET && FNV_PRIME == 0x0100_0193, "");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_all_fields() {
        let mut entries: [Option<LedgerEntry>; MAX_ENTRIES] = [None; MAX_ENTRIES];
        entries[0] = Some(LedgerEntry { id: 0xBEEF, cat: 9, ts: 0xDEAD_BEEF, score: 255 });
        entries[1] = Some(LedgerEntry { id: 7, cat: 1, ts: 42, score: 3 });
        let mut e = Exporter::new();
        e.serialize(&entries, &all_fields()).expect("全字段必成");
        let (back, count) = e.parse().expect("自产帧必解析");
        assert_eq!(count, 2);
        assert_eq!(back[0], entries[0], "逐字段无损（含边界值 0xBEEF/0xDEADBEEF/255）");
        assert_eq!(back[1], entries[1]);
    }

    #[test]
    fn whitelist_excludes_score_but_keeps_rest() {
        let mut entries: [Option<LedgerEntry>; MAX_ENTRIES] = [None; MAX_ENTRIES];
        entries[0] = Some(LedgerEntry { id: 5, cat: 2, ts: 1000, score: 200 });
        let mut wl = all_fields();
        wl[3] = false;
        let mut e = Exporter::new();
        let n = e.serialize(&entries, &wl).expect("过滤序列化必成");
        // 单条剔除 score 后体长 13+1 前缀，总长 4+2+14+4 = 24 < 全字段 44。
        assert_eq!(n, 24);
        assert_eq!(e.filtered, 1, "剔除字段必须记账");
        let (back, _) = e.parse().unwrap();
        let b = back[0].unwrap();
        assert_eq!((b.id, b.cat, b.ts, b.score), (5, 2, 1000, 0));
    }

    #[test]
    fn deep4_checks_all_green() {
        let cs = run_f040f_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
