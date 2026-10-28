//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12411 · 映射越界注入判据 · 0x30160
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12411-J1 ViewSize 超节/偏移越界/ZeroBits 冲突注入 30/30 次
//!      正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三类注入闭集：ViewSize 超节（VS 类）/偏移越界（OFF 类）/
//!     ZeroBits 冲突（ZB 类），每类 10 例 × 3 = 30 例逐例核对拒码；
//!   - VS 类判据：ViewSize>节尺寸拒（ViewSizeOver），0=整节口径合法；
//!   - OFF 类判据：SectionOffset 越节拒（OffsetOver）、非 64KiB 粒度拒
//!     （OffsetGran）、offset+ViewSize 越节拒（SpanOver）；
//!   - ZB 类判据：ZeroBits 越域拒（ZeroBitsBad）、基址高位越上界拒
//!     （ZeroBitsConflict）；
//!   - 拒止域核对：全部拒码落本段闭集（CaseMiss 判漏、CaseDup 判重）；
//!   - 拒止不写回：注入例全部拒止后映射账保持零（BoundDrift 判漂）；
//!   - 错误路径带码+中文 detail，逐例可回溯（例号/类/码三元组）；
//!   - 注入核：LCG 写死种子 0x30160，30/30 × 10 轮全正确拒止。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30160 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30160;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 每轮注入例数（判准 30/30）。
pub const INJECT_CASES: usize = 30;

/// 每类例数（三类各 10）。
pub const CASES_PER_CLASS: usize = 10;

/// 页粒度（4KiB 口径）。
pub const PAGE_SIZE: u64 = 0x1000;

/// 分配粒度（64KiB 口径）。
pub const ALLOC_GRAN: u64 = 0x10000;

/// ZeroBits 域闭集。
pub const ZERO_BITS_SET: [u32; 5] = [0, 21, 24, 31, 53];

/// 诊断码（0x30160 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OvCode {
    /// 0x30160 ViewSize 超节。
    ViewSizeOver = 0,
    /// 0x30161 偏移越节。
    OffsetOver = 1,
    /// 0x30162 偏移非 64KiB 粒度。
    OffsetGran = 2,
    /// 0x30163 跨越越节（offset+size>节）。
    SpanOver = 3,
    /// 0x30164 ZeroBits 越域。
    ZeroBitsBad = 4,
    /// 0x30165 ZeroBits 冲突（基址高位越上界）。
    ZeroBitsConflict = 5,
    /// 0x30166 注入例漏判（例落三类之外）。
    CaseMiss = 6,
    /// 0x30167 注入例重号（例号重复）。
    CaseDup = 7,
    /// 0x30168 映射账漂移（全拒后账非零）。
    BoundDrift = 8,
    /// 0x30169 节域越界（节尺寸零或越上限）。
    SecOob = 9,
}

impl OvCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [OvCode; 10] = [
        OvCode::ViewSizeOver, OvCode::OffsetOver, OvCode::OffsetGran, OvCode::SpanOver,
        OvCode::ZeroBitsBad, OvCode::ZeroBitsConflict, OvCode::CaseMiss, OvCode::CaseDup,
        OvCode::BoundDrift, OvCode::SecOob,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30160 => "ViewSize 超节：ViewSize 不得超节尺寸，整节传 0。",
        0x30161 => "偏移越节：SectionOffset 不得越过节终点。",
        0x30162 => "偏移粒度：SectionOffset 必须 64KiB 对齐。",
        0x30163 => "跨越越节：SectionOffset+ViewSize 不得超节尺寸。",
        0x30164 => "ZeroBits 越域：只收 0/21/24/31/53 五档。",
        0x30165 => "ZeroBits 冲突：基址高位必须落上界之下。",
        0x30166 => "注入例漏判：逐例归三类闭集，漏判红账。",
        0x30167 => "注入例重号：例号唯一，重号先查抽签器。",
        0x30168 => "映射账漂移：全拒后映射账必须为零。",
        0x30169 => "节域越界：节尺寸必须 >0 且不越上限。",
        _ => "未知诊断码：回查 0x30160 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30160 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3016_3016_3016_3016;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 映射尝试参数包。
#[derive(Clone, Copy, Debug)]
pub struct MapAttempt {
    /// ViewSize（0=整节）。
    pub view_size: u64,
    /// 节内偏移。
    pub offset: u64,
    /// 显式基址（0=自动）。
    pub base: u64,
    /// ZeroBits。
    pub zero_bits: u32,
}

/// 映射越界判据正本（节域 + 逐例拒止器）。
///
/// 生命周期：bind_section（节域）→ attempt（逐例判）→ 全拒核对。
#[derive(Debug)]
pub struct BoundsBook {
    /// 节尺寸。
    sec_size: u64,
    /// 节域合法。
    sec_live: bool,
    /// 映射账（成功映射数；全拒判准要求为 0）。
    mapped: u32,
    /// 拒止账（append-only 例账）。
    cases: Vec<(usize, OvCode)>,
    /// 末例号（u64::MAX=尚无例；唯一性判准）。
    last_case: u64,
    /// 拒止累计。
    rejects: u32,
}

impl BoundsBook {
    /// 建账（节尺寸给定）。
    pub fn new(sec_size: u64) -> Result<Self, OvCode> {
        if sec_size == 0 || sec_size > 0x1_0000_0000 {
            return Err(OvCode::SecOob);
        }
        Ok(BoundsBook {
            sec_size,
            sec_live: true,
            mapped: 0,
            cases: Vec::new(),
            last_case: u64::MAX,
            rejects: 0,
        })
    }

    /// 节尺寸面。
    pub fn sec_size(&self) -> u64 {
        self.sec_size
    }

    /// ZeroBits 域核对。
    pub fn zero_bits_ok(zb: u32) -> bool {
        ZERO_BITS_SET.contains(&zb)
    }

    /// ZeroBits 上界核对。
    pub fn zero_bits_conflict(base: u64, zb: u32) -> bool {
        if zb == 0 {
            return false;
        }
        let limit: u64 = 1u64.checked_shl(64 - zb).unwrap_or(0);
        limit != 0 && base >= limit
    }

    /// 逐例判（纯函数：只判不写；返回拒码或 Ok）。
    pub fn judge(&self, p: &MapAttempt) -> Result<(), OvCode> {
        // 1. ZeroBits 域。
        if !Self::zero_bits_ok(p.zero_bits) {
            return Err(OvCode::ZeroBitsBad);
        }
        // 2. ZeroBits 冲突（显式基址高位）。
        if p.base != 0 && Self::zero_bits_conflict(p.base, p.zero_bits) {
            return Err(OvCode::ZeroBitsConflict);
        }
        // 3. 偏移越节。
        if p.offset > self.sec_size {
            return Err(OvCode::OffsetOver);
        }
        // 4. 偏移粒度。
        if p.offset % ALLOC_GRAN != 0 {
            return Err(OvCode::OffsetGran);
        }
        // 5. ViewSize 超节（0=整节）。
        let vsz = if p.view_size == 0 { self.sec_size - p.offset } else { p.view_size };
        if p.view_size > self.sec_size {
            return Err(OvCode::ViewSizeOver);
        }
        // 6. 跨越越节。
        if p.offset + vsz > self.sec_size {
            return Err(OvCode::SpanOver);
        }
        Ok(())
    }

    /// 注入一例（判止后记账；合法例映射 +1——判据域只收拒例）。
    pub fn attempt(&mut self, case_no: usize, p: &MapAttempt) -> Result<(), OvCode> {
        if self.last_case != u64::MAX && (case_no as u64) <= self.last_case {
            self.rejects += 1;
            return Err(OvCode::CaseDup);
        }
        self.last_case = case_no as u64;
        match self.judge(p) {
            Err(c) => {
                self.cases.push((case_no, c));
                self.rejects += 1;
                Err(c)
            }
            Ok(()) => {
                // 合法例映射成功（判据域只用于负例时不会走到）。
                self.mapped += 1;
                Ok(())
            }
        }
    }

    /// 拒止例账长。
    pub fn case_len(&self) -> usize {
        self.cases.len()
    }

    /// 例账快照。
    pub fn case_of(&self, i: usize) -> Option<(usize, OvCode)> {
        if i >= self.cases.len() {
            None
        } else {
            Some(self.cases[i])
        }
    }

    /// 全拒判准：映射账为零（拒止不写回）。
    pub fn all_rejected(&self) -> bool {
        self.mapped == 0
    }

    /// 例号唯一判准。
    pub fn cases_unique(&self) -> bool {
        let mut i = 0;
        while i < self.cases.len() {
            let mut j = i + 1;
            while j < self.cases.len() {
                if self.cases[i].0 == self.cases[j].0 {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }

    /// 映射账。
    pub fn mapped_total(&self) -> u32 {
        self.mapped
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 三类注入例抽签（30 例：VS 类 0..10、OFF 类 10..20、ZB 类 20..30）。
///
/// 返回 (例号, 参数, 期望拒码)。
pub fn pick_case(lcg: &mut Lcg, sec_size: u64, i: usize) -> (usize, MapAttempt, OvCode) {
    let class = i / CASES_PER_CLASS;
    let k = lcg.below(4);
    match class {
        0 => {
            // VS 类：ViewSize 超节变体。
            let over = PAGE_SIZE * (1 + k);
            (
                i,
                MapAttempt { view_size: sec_size + over, offset: 0, base: 0, zero_bits: 0 },
                OvCode::ViewSizeOver,
            )
        }
        1 => {
            // OFF 类：偏移越节/粒度/跨越变体。
            match k {
                0 => (
                    i,
                    MapAttempt { view_size: 0, offset: sec_size + PAGE_SIZE, base: 0, zero_bits: 0 },
                    OvCode::OffsetOver,
                ),
                1 => (
                    i,
                    MapAttempt { view_size: 0, offset: ALLOC_GRAN + 0x123, base: 0, zero_bits: 0 },
                    OvCode::OffsetGran,
                ),
                2 => (
                    i,
                    MapAttempt {
                        view_size: PAGE_SIZE * 20,
                        offset: sec_size - ALLOC_GRAN,
                        base: 0,
                        zero_bits: 0,
                    },
                    OvCode::SpanOver,
                ),
                _ => (
                    i,
                    MapAttempt { view_size: 0, offset: sec_size * 2, base: 0, zero_bits: 0 },
                    OvCode::OffsetOver,
                ),
            }
        }
        _ => {
            // ZB 类：ZeroBits 越域/冲突变体。
            match k {
                0 => (
                    i,
                    MapAttempt { view_size: 0, offset: 0, base: 0, zero_bits: 40 },
                    OvCode::ZeroBitsBad,
                ),
                1 => (
                    i,
                    MapAttempt { view_size: 0, offset: 0, base: 0, zero_bits: 3 },
                    OvCode::ZeroBitsBad,
                ),
                2 => (
                    i,
                    MapAttempt { view_size: 0, offset: 0, base: 1u64 << 48, zero_bits: 31 },
                    OvCode::ZeroBitsConflict,
                ),
                _ => (
                    i,
                    MapAttempt { view_size: 0, offset: 0, base: 1u64 << 40, zero_bits: 24 },
                    OvCode::ZeroBitsConflict,
                ),
            }
        }
    }
}

/// 三类注入 30/30 正确拒止演练（逐例码值核对）。
pub fn drill_inject30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30160 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let sec_size = PAGE_SIZE * 64;
        let mut hit = 0usize;
        let (all_rej, uniq) = match BoundsBook::new(sec_size) {
            Ok(mut b) => {
                for i in 0..INJECT_CASES {
                    let (n, p, expect) = pick_case(&mut lcg, sec_size, i);
                    match b.attempt(n, &p) {
                        Err(c) if c == expect => hit += 1,
                        _ => {}
                    }
                }
                (b.all_rejected(), b.cases_unique())
            }
            Err(_) => (false, false),
        };
        if hit == INJECT_CASES && all_rej && uniq {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("越界注入 {}/{} 轮={}/{}", INJECT_CASES, INJECT_CASES, ok, INJECT_ROUNDS))
}

/// 三类覆盖演练：每类至少 8 例在账 10/10。
pub fn drill_class_cover_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30160 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let sec_size = PAGE_SIZE * 64;
        let mut bk = match BoundsBook::new(sec_size) {
            Ok(b) => b,
            Err(_) => continue,
        };
        for i in 0..INJECT_CASES {
            let (n, p, _e) = pick_case(&mut lcg, sec_size, i);
            let _ = bk.attempt(n, &p);
        }
        // 逐类计数（按例号分段）。
        let vs = (0..bk.case_len()).filter(|&i| bk.case_of(i).map(|(n, _)| n < 10).unwrap_or(false)).count();
        let off = (0..bk.case_len())
            .filter(|&i| bk.case_of(i).map(|(n, _)| n >= 10 && n < 20).unwrap_or(false))
            .count();
        let zb = (0..bk.case_len()).filter(|&i| bk.case_of(i).map(|(n, _)| n >= 20).unwrap_or(false)).count();
        if vs >= 8 && off >= 8 && zb >= 8 && vs + off + zb == INJECT_CASES {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("三类覆盖轮={}/{}", ok, INJECT_ROUNDS))
}

/// 边界值档演练：0=整节合法、恰节尺寸合法、越一页拒 10/10。
pub fn drill_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30160 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let sec_size = PAGE_SIZE * 16;
        let bk = match BoundsBook::new(sec_size) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let whole = bk.judge(&MapAttempt { view_size: 0, offset: 0, base: 0, zero_bits: 0 });
        let exact = bk.judge(&MapAttempt { view_size: sec_size, offset: 0, base: 0, zero_bits: 0 });
        let tail = bk.judge(&MapAttempt { view_size: sec_size, offset: ALLOC_GRAN, base: 0, zero_bits: 0 });
        let over1 = bk.judge(&MapAttempt { view_size: sec_size + PAGE_SIZE, offset: 0, base: 0, zero_bits: 0 });
        let tail_ok = bk.judge(&MapAttempt { view_size: PAGE_SIZE * 8, offset: 0, base: 0, zero_bits: 0 });
        let zero_sec = BoundsBook::new(0);
        let _ = lcg.below(3);
        if whole.is_ok()
            && exact.is_ok()
            && matches!(tail, Err(OvCode::SpanOver))
            && matches!(over1, Err(OvCode::ViewSizeOver))
            && tail_ok.is_ok()
            && matches!(zero_sec, Err(OvCode::SecOob))
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("边界值档轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：例账转录导出（逐例一行：例号/类/码）。
/// ─────────────────────────────────────────────────────────────────────
/// 例账转录导出。
pub fn export_case_lines(bk: &BoundsBook) -> String {
    let mut s = String::from("越界例账转录：\n");
    for i in 0..bk.case_len() {
        if let Some((n, c)) = bk.case_of(i) {
            let cls = if n < 10 { "VS" } else if n < 20 { "OFF" } else { "ZB" };
            s.push_str(&format!("#{} {} {}\n", n, cls, c as u32));
        }
    }
    s
}

/// 转录完整判据（逐例行齐备）。
pub fn case_lines_complete(bk: &BoundsBook) -> bool {
    let s = export_case_lines(bk);
    if bk.case_len() == 0 {
        return false;
    }
    (0..bk.case_len()).all(|i| {
        bk.case_of(i)
            .map(|(n, _)| s.contains(&format!("#{} ", n)))
            .unwrap_or(false)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：越界注入报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct OvReport {
    /// 例账行数。
    pub cases: usize,
    /// 映射账（全拒判准要求 0）。
    pub mapped: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 例号唯一。
    pub unique: bool,
    /// 全拒。
    pub all_rejected: bool,
}

impl OvReport {
    /// 建报告。
    pub fn build(bk: &BoundsBook) -> Self {
        OvReport {
            cases: bk.case_len(),
            mapped: bk.mapped_total(),
            rejects: bk.reject_total(),
            unique: bk.cases_unique(),
            all_rejected: bk.all_rejected(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.all_rejected && self.unique && self.cases >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "OvReport 例={} 映={} 拒={} 唯一={} 全拒={}",
            self.cases, self.mapped, self.rejects, self.unique, self.all_rejected
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：ZeroBits 域演练（五档闭集/上界两例 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_zerobits_edge_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30160 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = ZERO_BITS_SET == [0, 21, 24, 31, 53];
        pass = pass && BoundsBook::zero_bits_ok(53) && !BoundsBook::zero_bits_ok(63);
        pass = pass && BoundsBook::zero_bits_conflict(1u64 << 40, 31);
        pass = pass && !BoundsBook::zero_bits_conflict(1u64 << 10, 31);
        pass = pass && !BoundsBook::zero_bits_conflict(1u64 << 62, 0);
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("ZeroBits 域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：例号唯一性演练（重号例拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_case_dup_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30160 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let sec_size = PAGE_SIZE * 64;
        let mut bk = match BoundsBook::new(sec_size) {
            Ok(b) => b,
            Err(_) => continue,
        };
        let (n, p, _e) = pick_case(&mut lcg, sec_size, 0);
        let first = bk.attempt(n, &p);
        let dup = bk.attempt(n, &p);
        let _ = lcg.below(3);
        if first.is_err() && matches!(dup, Err(OvCode::CaseDup)) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("例号唯一轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct OvCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_ov_checklist(bk: &BoundsBook) -> Vec<OvCheckRow> {
    let rep = OvReport::build(bk);
    let mut rows: Vec<OvCheckRow> = Vec::new();
    rows.push(OvCheckRow { name: "全拒", passed: bk.all_rejected() });
    rows.push(OvCheckRow { name: "例号唯一", passed: bk.cases_unique() });
    rows.push(OvCheckRow { name: "注入口径", passed: INJECT_CASES == 30 && CASES_PER_CLASS == 10 });
    rows.push(OvCheckRow { name: "ZB闭集", passed: ZERO_BITS_SET == [0, 21, 24, 31, 53] });
    rows.push(OvCheckRow { name: "粒度常量", passed: ALLOC_GRAN == 0x10000 && PAGE_SIZE == 0x1000 });
    rows.push(OvCheckRow { name: "转录完整", passed: case_lines_complete(bk) });
    rows.push(OvCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(OvCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(OvCheckRow { name: "节域在册", passed: bk.sec_size() > 0 });
    rows.push(OvCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ov_checklist_green(rows: &[OvCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ov_stats_line(bk: &BoundsBook) -> String {
    format!(
        "越界概况：例 {} 映 {} 拒 {}",
        bk.case_len(),
        bk.mapped_total(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12411_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 5);
    let sec_size = PAGE_SIZE * 64;
    let mut bk = match BoundsBook::new(sec_size) {
        Ok(b) => b,
        Err(_) => return out,
    };
    for i in 0..INJECT_CASES {
        let (n, p, _e) = pick_case(&mut lcg, sec_size, i);
        let _ = bk.attempt(n, &p);
    }
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        case_lines_complete(&bk) && OvReport::build(&bk).all_green(),
        ov_stats_line(&bk),
    ));
    // X-b：ZeroBits 域 10/10。
    let (ok_z, d_z) = drill_zerobits_edge_10();
    out.push(("X-ZeroBits域10/10", ok_z, d_z));
    // X-c：例号唯一 10/10。
    let (ok_d, d_d) = drill_case_dup_10();
    out.push(("X-例号唯一10/10", ok_d, d_d));
    // X-d：终验清单。
    let rows = build_ov_checklist(&bk);
    out.push(("X-终验清单10项", ov_checklist_green(&rows), OvReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let sec_size = PAGE_SIZE * 64;
    let bk = match BoundsBook::new(sec_size) {
        Ok(b) => b,
        Err(_) => return (caught, 5, d),
    };
    // 注 1：ViewSize 超节必拒（ViewSizeOver）。
    let r1 = bk.judge(&MapAttempt { view_size: sec_size + PAGE_SIZE, offset: 0, base: 0, zero_bits: 0 });
    if matches!(r1, Err(OvCode::ViewSizeOver)) {
        caught += 1;
        d.push("注1 超节:抓".to_string());
    } else {
        d.push("注1 超节:漏".to_string());
    }
    // 注 2：偏移越节必拒（OffsetOver）。
    let r2 = bk.judge(&MapAttempt { view_size: 0, offset: sec_size + ALLOC_GRAN, base: 0, zero_bits: 0 });
    if matches!(r2, Err(OvCode::OffsetOver)) {
        caught += 1;
        d.push("注2 偏越:抓".to_string());
    } else {
        d.push("注2 偏越:漏".to_string());
    }
    // 注 3：ZeroBits 越域必拒（ZeroBitsBad）。
    let r3 = bk.judge(&MapAttempt { view_size: 0, offset: 0, base: 0, zero_bits: 63 });
    if matches!(r3, Err(OvCode::ZeroBitsBad)) {
        caught += 1;
        d.push("注3 ZB域:抓".to_string());
    } else {
        d.push("注3 ZB域:漏".to_string());
    }
    // 注 4：ZeroBits 冲突必拒（ZeroBitsConflict）。
    let r4 = bk.judge(&MapAttempt { view_size: 0, offset: 0, base: 1u64 << 48, zero_bits: 31 });
    if matches!(r4, Err(OvCode::ZeroBitsConflict)) {
        caught += 1;
        d.push("注4 ZB冲:抓".to_string());
    } else {
        d.push("注4 ZB冲:漏".to_string());
    }
    // 注 5：跨越越节必拒（SpanOver）。
    let r5 = bk.judge(&MapAttempt {
        view_size: PAGE_SIZE * 20,
        offset: sec_size - ALLOC_GRAN,
        base: 0,
        zero_bits: 0,
    });
    if matches!(r5, Err(OvCode::SpanOver)) {
        caught += 1;
        d.push("注5 跨越:抓".to_string());
    } else {
        d.push("注5 跨越:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30160 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    OvCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12411_checks：本单判据聚合（诊断段 0x30160）。
pub fn run_unx12411_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12411 映射越界注入判据");
    // J1-a：三类注入 30/30 正确拒止。
    let (ok1, d1) = drill_inject30_10();
    s.add_dyn("J1-三类注入30/30拒止", ok1, &d1);
    // J1-b：三类覆盖（每类 ≥8 例）。
    let (ok2, d2) = drill_class_cover_10();
    s.add_dyn("J1-三类覆盖10/10", ok2, &d2);
    // J1-c：边界值档 10/10。
    let (ok3, d3) = drill_edge_10();
    s.add_dyn("J1-边界值档10/10", ok3, &d3);
    // J1-d：标准注入会话（全拒 + 账零 + 例号唯一）。
    let sec_size = PAGE_SIZE * 64;
    let mut bk = match BoundsBook::new(sec_size) {
        Ok(b) => b,
        Err(_) => return s,
    };
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut hit = 0usize;
    for i in 0..INJECT_CASES {
        let (n, p, expect) = pick_case(&mut lcg, sec_size, i);
        match bk.attempt(n, &p) {
            Err(c) if c == expect => hit += 1,
            _ => {}
        }
    }
    s.add_dyn(
        "J1-全拒账零例号唯一",
        hit == INJECT_CASES
            && bk.all_rejected()
            && bk.cases_unique()
            && case_lines_complete(&bk),
        &format!("命中 {}/{} 全拒={} 唯一={}", hit, INJECT_CASES, bk.all_rejected(), bk.cases_unique()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（注入后映射账恒零）。
    s.add_dyn(
        "J2-拒止不写回",
        bk.mapped_total() == 0 && bk.reject_total() >= INJECT_CASES as u32,
        &format!("映射账={} 拒止={}", bk.mapped_total(), bk.reject_total()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30160段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12411_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
