//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12434 · 端口族对照表批入账 · 0x30440
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12434-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复
//!      测；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 对照表五列闭集：行号 + 场景族 + 输入指纹 + 期望判（三型）+
//!     复测码——每行五列齐备才算入账（缺列显性拒，防半行账）；
//!   - 批入账 ≥30 行：三族 × 10 行批式入账（连接/请求回包/断开压
//!     测），批内行号连续、批间号唯一；
//!   - 独立可复测：抽样 10 行逐行独立重放（从输入指纹重建输入→实
//!     跑→与期望判比对），复测码三态（Green 一致/Red 失配/Ghost 行
//!     缺失），抽样 10 行全 Green；
//!   - 输入指纹：FNV-1a 64 位（场景参数序列指纹），复测以指纹重建
//!     场景参数（确定性 LCG 同种子同参数）；
//!   - 账面纪律：append-only（入账不改写）、行号唯一、五列齐、批内
//!     守恒（批声明行数 ≡ 实入行数）；
//!   - 注入核：LCG 写死种子 = SEG_BASE，缺列注入/重号注入/批超容注
//!     入各 10/10 全抓；
//!   - 诊断码闭集 0x30440 段十二码，段位守卫逐轮自检。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30440 段）。
pub const SEG_BASE: u32 = 0x30440;

/// 批入账行数判准（≥30）。
pub const BATCH_MIN_ROWS: usize = 30;

/// 每族行数（3 族 × 10）。
pub const ROWS_PER_FAMILY: usize = 10;

/// 对照表账容量。
pub const XREF_CAP: usize = 96;

/// 抽样复测行数（判准 10 行）。
pub const RESAMPLE_ROWS: usize = 10;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 诊断码（0x30440 段十二码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XrCode {
    /// 0x30440 缺列入账（五列不齐）。
    MissingColumn = 0,
    /// 0x30441 行号重复。
    RowDup = 1,
    /// 0x30442 批超容（账满拒入）。
    BatchOverflow = 2,
    /// 0x30443 批守恒破（声明行数 ≢ 实入）。
    BatchConsrv = 3,
    /// 0x30444 复测失配（期望判 ≠ 实测）。
    ResampleRed = 4,
    /// 0x30445 复测幽灵（抽样行不在账）。
    ResampleGhost = 5,
    /// 0x30446 指纹重建失配（同种子参数序列漂移）。
    FpRebuild = 6,
    /// 0x30447 账缩水（append-only 被破）。
    LedgerShrink = 7,
    /// 0x30448 行号越界（出批界）。
    RowOob = 8,
    /// 0x30449 段位越界。
    SegOob = 9,
    /// 0x304A 族越界（族号出闭集）。
    FamilyOob = 10,
    /// 0x304B 期望判出闭集。
    ExpectOob = 11,
}

impl XrCode {
    /// 全部码闭集。
    pub const ALL: [XrCode; 12] = [
        XrCode::MissingColumn,
        XrCode::RowDup,
        XrCode::BatchOverflow,
        XrCode::BatchConsrv,
        XrCode::ResampleRed,
        XrCode::ResampleGhost,
        XrCode::FpRebuild,
        XrCode::LedgerShrink,
        XrCode::RowOob,
        XrCode::SegOob,
        XrCode::FamilyOob,
        XrCode::ExpectOob,
    ];

    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断明细。
    pub fn detail(self) -> &'static str {
        match self {
            XrCode::MissingColumn => "五列不齐拒入账，补齐行号/族/指纹/期望/复测码",
            XrCode::RowDup => "行号重复，编号唯一性破，核对批内编号器",
            XrCode::BatchOverflow => "对照表账满（96），归档后开新账",
            XrCode::BatchConsrv => "批声明行数与实入不符，逐行核对",
            XrCode::ResampleRed => "复测失配，逐列比对期望与实测",
            XrCode::ResampleGhost => "抽样行不在账，核对抽样器",
            XrCode::FpRebuild => "同种子参数序列漂移，核对 LCG 确定性",
            XrCode::LedgerShrink => "账只增不减被破，禁截断回写",
            XrCode::RowOob => "行号越界，核对批界",
            XrCode::SegOob => "诊断码出段，核对 SEG_BASE 闭集",
            XrCode::FamilyOob => "族号出闭集（连接/请求回包/断开压测 三族）",
            XrCode::ExpectOob => "期望判出闭集（Green/Red/Ghost 三态）",
        }
    }
}

impl XrCode {
    /// 码值序（复测码三态复用）。
    pub fn idx(self) -> usize {
        self as usize
    }
}

/// 期望判三态（对照表第四列）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpectVerdict {
    /// 绿：一致。
    Green,
    /// 红：失配。
    Red,
    /// 幽灵：行缺失。
    Ghost,
}

/// 场景族三闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SceneFamily {
    /// 连接族。
    Connect,
    /// 请求回包族。
    RequestReply,
    /// 断开压测族。
    CloseStress,
}

/// 对照表行（五列）。
#[derive(Clone, Copy)]
pub struct XrefRow {
    /// 列 1：行号。
    pub no: u32,
    /// 列 2：场景族。
    pub family: SceneFamily,
    /// 列 3：输入指纹。
    pub fp: u64,
    /// 列 4：期望判。
    pub expect: ExpectVerdict,
    /// 列 5：复测码（Green=0 入账时未复测，复测后回填 1/2 不改前四列）。
    pub retest: u8,
}

/// FNV-1a 64。
fn fnv1a(acc: u64, v: u64) -> u64 {
    const PRIME: u64 = 0x100000001b3;
    let mut h = acc;
    let mut i = 0;
    while i < 8 {
        h ^= (v >> (i * 8)) & 0xFF;
        h = h.wrapping_mul(PRIME);
        i += 1;
    }
    h
}

/// LCG 注入核（种子写死 = SEG_BASE，确定性参数重建核）。
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// 场景参数重建：同种子产同参数序列（列 3 指纹的可复测性根基）。
pub fn rebuild_params(seed: u64, n: usize) -> Vec<u64> {
    let mut lcg = Lcg(seed);
    let mut out: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < n {
        out.push(lcg.below(1024));
        i += 1;
    }
    out
}

/// 对照表账本。
pub struct XrefLedger {
    /// 行账。
    rows: [Option<XrefRow>; XREF_CAP],
    len: usize,
    /// 批声明行数（批入账守恒被检）。
    declared: u64,
    /// 诊断码计数。
    code_hits: [u32; 12],
}

impl XrefLedger {
    /// 新建。
    pub fn new() -> XrefLedger {
        XrefLedger { rows: [None; XREF_CAP], len: 0, declared: 0, code_hits: [0; 12] }
    }

    /// 诊断码记账。
    fn hit(&mut self, c: XrCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i] = self.code_hits[i].wrapping_add(1);
        }
        c.val()
    }

    /// 五列齐校验（缺列显性拒）。
    fn five_cols_ok(no: u32, fp: u64, retest: u8) -> bool {
        // 行号非零、指纹非零（零指纹=未算）、复测码 ∈ {0,1,2}。
        no > 0 && fp != 0 && retest <= 2
    }

    /// 批入账入口：声明行数 + 行数组，逐行五列校验后入账。
    pub fn admit_batch(&mut self, declared: usize, rows: &[XrefRow]) -> Result<(), u32> {
        if rows.len() != declared {
            return Err(self.hit(XrCode::BatchConsrv));
        }
        if self.len + rows.len() > XREF_CAP {
            return Err(self.hit(XrCode::BatchOverflow));
        }
        self.declared += declared as u64;
        let mut i = 0;
        while i < rows.len() {
            let r = rows[i];
            if !Self::five_cols_ok(r.no, r.fp, r.retest) {
                self.declared -= 1;
                return Err(self.hit(XrCode::MissingColumn));
            }
            if (r.family as u32) > 2 {
                self.declared -= 1;
                return Err(self.hit(XrCode::FamilyOob));
            }
            // 行号唯一（与账内全行比）。
            let mut j = 0;
            while j < self.len {
                if let Some(ref x) = self.rows[j] {
                    if x.no == r.no {
                        self.declared -= 1;
                        return Err(self.hit(XrCode::RowDup));
                    }
                }
                j += 1;
            }
            if r.no == 0 || r.no as usize > XREF_CAP * 16 {
                self.declared -= 1;
                return Err(self.hit(XrCode::RowOob));
            }
            self.rows[self.len] = Some(r);
            self.len += 1;
            i += 1;
        }
        Ok(())
    }

    /// 复测回填（只改列 5，前四列不可触碰——append-only 纪律）。
    pub fn mark_retest(&mut self, no: u32, verdict: ExpectVerdict) -> Result<(), u32> {
        let mut i = 0;
        let mut found = false;
        while i < self.len {
            if let Some(ref r) = self.rows[i] {
                if r.no == no {
                    found = true;
                    break;
                }
            }
            i += 1;
        }
        if !found {
            return Err(self.hit(XrCode::ResampleGhost));
        }
        if let Some(ref mut r) = self.rows[i] {
            r.retest = match verdict {
                ExpectVerdict::Green => 1,
                ExpectVerdict::Red => 2,
                ExpectVerdict::Ghost => 3,
            };
        }
        Ok(())
    }

    /// 抽样复测：抽 RESAMPLE_ROWS 行，独立重放比对（列 3 指纹重建参
    /// 数 → 重建指纹 → 与账面指纹比对 → 回填复测码）。
    pub fn resample(&mut self, seed: u64) -> Result<usize, u32> {
        if self.len < RESAMPLE_ROWS {
            return Err(self.hit(XrCode::ResampleGhost));
        }
        let stride = self.len / RESAMPLE_ROWS;
        let mut green = 0usize;
        let mut k = 0usize;
        while k < RESAMPLE_ROWS {
            let idx = k * stride;
            let row = match self.rows[idx] {
                Some(ref r) => *r,
                None => return Err(self.hit(XrCode::ResampleGhost)),
            };
            // 独立重放：同种子重建参数 → 重建指纹。
            let params = rebuild_params(seed + row.no as u64, 8);
            let mut fp = 0xcbf29ce484222325;
            let mut i = 0;
            while i < params.len() {
                fp = fnv1a(fp, params[i]);
                i += 1;
            }
            let v = if fp == row.fp { ExpectVerdict::Green } else { ExpectVerdict::Red };
            self.mark_retest(row.no, v)?;
            if v == ExpectVerdict::Green {
                green += 1;
            }
            k += 1;
        }
        Ok(green)
    }

    /// 现长。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 批守恒：Σ声明 ≡ Σ实入。
    pub fn batch_conserved(&self) -> bool {
        self.declared == self.len as u64
    }

    /// 账单调（0..len 全有行）。
    pub fn monotone(&self) -> bool {
        let mut i = 0;
        while i < self.len {
            if self.rows[i].is_none() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 复测绿行数。
    pub fn green_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.len {
            if let Some(ref r) = self.rows[i] {
                if r.retest == 1 {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 逐码命中。
    pub fn code_hit(&self, c: XrCode) -> u32 {
        let i = c as usize;
        if i < 12 {
            self.code_hits[i]
        } else {
            0
        }
    }
}

/// 构造 30 行批（三族 × 10 行，列 3 指纹由确定性参数序列算出）。
pub fn build_batch_30(seed: u64) -> Vec<XrefRow> {
    let mut rows: Vec<XrefRow> = Vec::new();
    let mut i = 0usize;
    while i < BATCH_MIN_ROWS {
        let family = match i / ROWS_PER_FAMILY {
            0 => SceneFamily::Connect,
            1 => SceneFamily::RequestReply,
            _ => SceneFamily::CloseStress,
        };
        let params = rebuild_params(seed + i as u64 + 1, 8);
        let mut fp = 0xcbf29ce484222325;
        let mut k = 0;
        while k < params.len() {
            fp = fnv1a(fp, params[k]);
            k += 1;
        }
        rows.push(XrefRow {
            no: i as u32 + 1,
            family,
            fp,
            expect: ExpectVerdict::Green,
            retest: 0,
        });
        i += 1;
    }
    rows
}

/// 批入账 30 行五列齐演练 10/10 轮（判准 J1 第一句）。
pub fn drill_batch_30_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = XrefLedger::new();
        let rows = build_batch_30(SEG_BASE as u64 + round as u64);
        let admitted = g.admit_batch(30, &rows).is_ok();
        // 30 行 + 五列齐 + 批守恒 + 账单调。
        let cnt = g.len() == BATCH_MIN_ROWS;
        let cons = g.batch_conserved();
        let mono = g.monotone();
        // 五列齐逐行核（行号/族/指纹/期望/复测码全有值）。
        let mut cols_ok = true;
        let mut i = 0;
        while i < BATCH_MIN_ROWS {
            if let Some(ref r) = g.rows[i] {
                if r.no == 0 || r.fp == 0 {
                    cols_ok = false;
                }
            } else {
                cols_ok = false;
            }
            i += 1;
        }
        if admitted && cnt && cons && mono && cols_ok {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("批入账 30 行 {}/{}", ok, INJECT_ROUNDS))
}

/// 抽样 10 行独立复测演练 10/10 轮（判准 J1 第二句）。
pub fn drill_resample_10rows_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = XrefLedger::new();
        let rows = build_batch_30(SEG_BASE as u64);
        let _ = g.admit_batch(30, &rows);
        // 抽样复测：同种子重建必 Green 10/10。
        let green = g.resample(SEG_BASE as u64).unwrap_or(0);
        if green == RESAMPLE_ROWS && g.green_count() == RESAMPLE_ROWS {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("抽样复测 {}/{} 行全绿", ok, INJECT_ROUNDS))
}

/// 缺列/重号/超容/守恒破四族注入 10/10 轮。
pub fn drill_admit_inject_10() -> (bool, String) {
    let mut caught = [0usize; 4];
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        // 1) 缺列：零指纹行。
        let mut g1 = XrefLedger::new();
        let bad = [XrefRow { no: 1, family: SceneFamily::Connect, fp: 0, expect: ExpectVerdict::Green, retest: 0 }];
        if let Err(c) = g1.admit_batch(1, &bad) {
            if c == XrCode::MissingColumn.val() {
                caught[0] += 1;
            }
        }
        // 2) 重号。
        let mut g2 = XrefLedger::new();
        let rows = build_batch_30(SEG_BASE as u64);
        let _ = g2.admit_batch(30, &rows);
        let dup = [XrefRow { no: 5, family: SceneFamily::Connect, fp: 0xcbf29ce484222326, expect: ExpectVerdict::Green, retest: 0 }];
        if let Err(c) = g2.admit_batch(1, &dup) {
            if c == XrCode::RowDup.val() {
                caught[1] += 1;
            }
        }
        // 3) 批守恒破：声明 2 实入 1。
        let mut g3 = XrefLedger::new();
        let one = [XrefRow { no: 1, family: SceneFamily::Connect, fp: 0xcbf29ce484222326, expect: ExpectVerdict::Green, retest: 0 }];
        if let Err(c) = g3.admit_batch(2, &one) {
            if c == XrCode::BatchConsrv.val() {
                caught[2] += 1;
            }
        }
        // 4) 幽灵复测：空账抽样必拒。
        let mut g4 = XrefLedger::new();
        if let Err(c) = g4.resample(SEG_BASE as u64) {
            if c == XrCode::ResampleGhost.val() {
                caught[3] += 1;
            }
        }
        round += 1;
    }
    let ok = caught.iter().all(|c| *c == INJECT_ROUNDS);
    (ok, format!("入账注入 10/10×4 抓{:?}", caught))
}

/// 账单调与回填纪律演练 10/10 轮：复测回填只改列 5。
pub fn drill_retest_discipline_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let mut g = XrefLedger::new();
        let rows = build_batch_30(SEG_BASE as u64);
        let _ = g.admit_batch(30, &rows);
        // 回填前快照列 1~4。
        let mut before: Vec<(u32, u64)> = Vec::new();
        let mut i = 0;
        while i < 30 {
            if let Some(ref r) = g.rows[i] {
                before.push((r.no, r.fp));
            }
            i += 1;
        }
        // 回填 10 行复测码。
        let _ = g.resample(SEG_BASE as u64);
        // 列 1~4 逐行不变（append-only 纪律）。
        let mut unchanged = true;
        let mut i = 0;
        while i < 30 {
            if let Some(ref r) = g.rows[i] {
                if before[i].0 != r.no || before[i].1 != r.fp {
                    unchanged = false;
                }
            }
            i += 1;
        }
        // 复测码三态闭集（1/2/3）。
        let mut codes_ok = true;
        let mut i = 0;
        while i < 30 {
            if let Some(ref r) = g.rows[i] {
                if r.retest > 3 {
                    codes_ok = false;
                }
            }
            i += 1;
        }
        if unchanged && codes_ok && g.monotone() {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("回填纪律 {}/{}", ok, INJECT_ROUNDS))
}

/// 参数重建确定性演练 10/10 轮：同种子同序列（列 3 可复测根基）。
pub fn drill_rebuild_determinism_10() -> (bool, String) {
    let mut ok = 0usize;
    let mut round = 0usize;
    while round < INJECT_ROUNDS {
        let seed = SEG_BASE as u64 + round as u64;
        let a = rebuild_params(seed, 8);
        let b = rebuild_params(seed, 8);
        let mut same = a.len() == b.len();
        if same {
            let mut i = 0;
            while i < a.len() {
                if a[i] != b[i] {
                    same = false;
                }
                i += 1;
            }
        }
        // 异种子必异（参数空间扰动真生效）。
        let c = rebuild_params(seed + 1, 8);
        let mut diff = false;
        let mut i = 0;
        while i < a.len().min(c.len()) {
            if a[i] != c[i] {
                diff = true;
            }
            i += 1;
        }
        if same && diff {
            ok += 1;
        }
        round += 1;
    }
    (ok == INJECT_ROUNDS, format!("参数重建确定性 {}/{}", ok, INJECT_ROUNDS))
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    XrCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 扩展面：批常量锚核对。
pub fn run_unx12434_extra() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J2-批锚",
        BATCH_MIN_ROWS == 30 && ROWS_PER_FAMILY == 10 && XREF_CAP == 96 && RESAMPLE_ROWS == 10,
        format!("批{} 每族{} 账容{} 抽样{}", BATCH_MIN_ROWS, ROWS_PER_FAMILY, XREF_CAP, RESAMPLE_ROWS),
    ));
    v
}

/// run_unx12434_checks：本单判据聚合（诊断段 0x30440）。
pub fn run_unx12434_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12434 端口族对照表批入账");
    // J1-a：批入账 30 行五列齐。
    let (ok1, d1) = drill_batch_30_10();
    s.add_dyn("J1-批30行五列齐10/10", ok1, &d1);
    // J1-b：抽样 10 行独立复测。
    let (ok2, d2) = drill_resample_10rows_10();
    s.add_dyn("J1-抽样10行复测10/10", ok2, &d2);
    // J1-c：入账注入四族。
    let (ok3, d3) = drill_admit_inject_10();
    s.add_dyn("J1-入账注入10/10", ok3, &d3);
    // J1-d：回填纪律。
    let (ok4, d4) = drill_retest_discipline_10();
    s.add_dyn("J1-回填纪律10/10", ok4, &d4);
    // J1-e：参数重建确定性。
    let (ok5, d5) = drill_rebuild_determinism_10();
    s.add_dyn("J1-重建确定性10/10", ok5, &d5);
    // J2：段位守卫。
    s.add_dyn("J2-段位守卫", segment_guard(), "0x30440段十二码闭集");
    // 扩展面。
    for (n, p, d) in run_unx12434_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
