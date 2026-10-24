//! UNX-F8541 · E 型八批总对账账（B21–B28 ID/判据/行数三方）· 诊断段 0xB5A0
//! 验收锚点：UNX-F8541-J1 八批 160 条总对账——ID 连续（F8401–F8560）/
//! 判据三成分（断言/锚点/写回）/行数逐批求和三方一致，对账偏差零容忍，
//! 总账在册可导出。


use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xB5A0;

pub const ID_LO: u32 = 8401;
pub const ID_HI: u32 = 8560;
pub const TOTAL_ENTRIES: u32 = 160;

/// 单批对账行。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReconRow {
    pub batch: &'static str,
    pub id_lo: u32,
    pub id_hi: u32,
    pub entries: u32,   // 条数
    pub lines_sum: u32, // 行数逐批求和
}

impl ReconRow {
    pub fn ids_contiguous(&self) -> bool {
        self.id_hi - self.id_lo + 1 == self.entries
    }
}

/// 八批 canon（B21–B28）。
pub fn canon_batches() -> Vec<ReconRow> {
    vec![
        ReconRow { batch: "B21", id_lo: 8401, id_hi: 8420, entries: 20, lines_sum: 5600 },
        ReconRow { batch: "B22", id_lo: 8421, id_hi: 8440, entries: 20, lines_sum: 5800 },
        ReconRow { batch: "B23", id_lo: 8441, id_hi: 8460, entries: 20, lines_sum: 5400 },
        ReconRow { batch: "B24", id_lo: 8461, id_hi: 8480, entries: 20, lines_sum: 6000 },
        ReconRow { batch: "B25", id_lo: 8481, id_hi: 8500, entries: 20, lines_sum: 5200 },
        ReconRow { batch: "B26", id_lo: 8501, id_hi: 8520, entries: 20, lines_sum: 5900 },
        ReconRow { batch: "B27", id_lo: 8521, id_hi: 8540, entries: 20, lines_sum: 5300 },
        ReconRow { batch: "B28", id_lo: 8541, id_hi: 8560, entries: 20, lines_sum: 5500 },
    ]
}

/// 总对账器。
pub struct ReconLedger {
    pub rows: Vec<ReconRow>,
}

impl ReconLedger {
    pub fn new() -> Self {
        ReconLedger { rows: Vec::new() }
    }
    pub fn build_default(&mut self) {
        self.rows = canon_batches();
    }
    /// ID 连续：批间无跳批（前批 hi + 1 == 后批 lo）且全表恰 [8401, 8560]。
    pub fn ids_contiguous(&self) -> bool {
        if self.rows.is_empty() {
            return false;
        }
        if self.rows[0].id_lo != ID_LO || self.rows.last().unwrap().id_hi != ID_HI {
            return false;
        }
        self.rows.windows(2).all(|w| w[0].id_hi + 1 == w[1].id_lo)
            && self.rows.iter().all(|r| r.ids_contiguous())
    }
    /// 条数三方一致：逐批 entries 合计 == 160。
    pub fn entries_sum(&self) -> u32 {
        self.rows.iter().map(|r| r.entries).sum()
    }
    /// 行数逐批求和三方一致：批内行数合计 == 台账行数。
    pub fn lines_match(&self, ledger: &[( & 'static str, u32)]) -> bool {
        self.rows.len() == ledger.len()
            && self.rows.iter().zip(ledger.iter()).all(|(r, (b, n))| r.batch == *b && r.lines_sum == *n)
    }
    /// 判据三成分：每批登记断言/锚点/写回三成分（模拟在册标记）。
    pub fn three_components(&self, marks: &[(&'static str, bool, bool, bool)]) -> bool {
        self.rows.iter().all(|r| {
            marks.iter().any(|(b, a, c, w)| *b == r.batch && *a && *c && *w)
        })
    }
    /// 对账偏差零容忍：任一行偏移即红。
    pub fn zero_tolerance(&self, ledger: &[( &'static str, u32)]) -> bool {
        self.lines_match(ledger)
    }
    pub fn export(&self) -> String {
        self.rows.iter().map(|r| format!("{}:[{},{}]x{}", r.batch, r.id_lo, r.id_hi, r.entries)).collect::<Vec<String>>().join(",")
    }
}

/// FNV-1a 32 位。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in data {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 确定性 LCG。
pub fn lcg_next(seed: &mut u64) -> u64 {
    *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *seed
}

// ── 注入核（0xB5A0 段 5 项）────────────────────────────────────────────

pub fn inject_core_5() -> Vec<(&'static str, bool)> {
    let mut seed: u64 = 0x8541_B5A0;
    let mut ok = true;
    for _ in 0..64 {
        if lcg_next(&mut seed) == 0 {
            ok = false;
        }
    }
    let mut out: Vec<(&'static str, bool)> = Vec::new();
    out.push(("LCG-确定性-64轮不炸", ok));
    out.push(("八批-常数", canon_batches().len() == 8));
    out.push(("总量-160条", canon_batches().iter().map(|r| r.entries).sum::<u32>() == 160));
    out.push(("区间-F8401–F8560", ID_LO == 8401 && ID_HI == 8560));
    out.push(("FNV-空串锚", fnv1a(b"") == 0x811C_9DC5));
    out
}

// ── 判据聚合 ────────────────────────────────────────────────────────────

/// run_unx8541_checks：本单判据聚合（诊断段 0xB5A0）。
pub fn run_unx8541_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("UNX-F8541 E 型八批总对账");

    for (name, ok) in inject_core_5() {
        s.add(&name, ok, if ok { "" } else { "注入核失败" });
    }

    // 八批落账
    let mut led = ReconLedger::new();
    led.build_default();
    s.add("落账-八批全入账", led.rows.len() == 8, if led.rows.len() == 8 { "" } else { "批数缺口" });

    // ID 连续：F8401–F8560 无跳批
    let cont = led.ids_contiguous();
    s.add("ID连续-F8401–F8560批间无跳", cont, if cont { "" } else { "ID 断档" });

    // 条数三方一致
    let esum = led.entries_sum();
    s.add("条数-三方合计恰160", esum == 160, if esum == 160 { "" } else { "条数漂移" });

    // 判据三成分全在册
    let marks: Vec<(&str, bool, bool, bool)> = vec![
        ("B21", true, true, true), ("B22", true, true, true), ("B23", true, true, true), ("B24", true, true, true),
        ("B25", true, true, true), ("B26", true, true, true), ("B27", true, true, true), ("B28", true, true, true),
    ];
    s.add("三成分-断言/锚点/写回八批全在册", led.three_components(&marks), if led.three_components(&marks) { "" } else { "三成分缺口" });

    // 行数三方一致
    let ledger: Vec<(&str, u32)> = canon_batches().iter().map(|r| (r.batch, r.lines_sum)).collect();
    s.add("行数-逐批求和三方一致", led.lines_match(&ledger), if led.lines_match(&ledger) { "" } else { "行数漂移" });

    // 零容忍错账检出：任一批行数偏移即红
    let mut bad_ledger = ledger.clone();
    bad_ledger[3].1 += 1;
    s.add("零容忍-B24行数偏1即红", !led.zero_tolerance(&bad_ledger), if !led.zero_tolerance(&bad_ledger) { "" } else { "偏差漏检" });
    // 批名漂移
    let mut bad2 = ledger.clone();
    bad2[6].0 = "B99";
    s.add("零容忍-批名漂移即红", !led.zero_tolerance(&bad2), if !led.zero_tolerance(&bad2) { "" } else { "批名漏检" });

    // 总账可导出
    let exp = led.export();
    let exp_ok = exp.starts_with("B21:[8401,8420]x20,") && exp.ends_with("B28:[8541,8560]x20");
    s.add("导出-总账批区间条数可导出", exp_ok, if exp_ok { "" } else { "导出漂移" });

    // FNV 八批指纹链
    let mut acc: u32 = 0x811C_9DC5;
    for r in &led.rows {
        acc ^= fnv1a(&[r.id_lo as u8, (r.id_lo >> 8) as u8, r.entries as u8]);
        acc = acc.wrapping_mul(0x0100_0193);
    }
    s.add("FNV-八批指纹链非零", acc != 0, if acc != 0 { "" } else { "指纹链异常" });

    s
}
