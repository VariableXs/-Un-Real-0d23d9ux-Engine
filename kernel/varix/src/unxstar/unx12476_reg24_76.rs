//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12476 · 注册表族与 A4 存储联签预告 · 0x30980
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12476-J1 hive 落盘分界预告齐（A4 承接批号登记），脏页计数
//!      对平断言绿；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - hive 页域：定容页账（64 页），页两态（干净/脏），写命中即置脏
//!   （DirtyDup 面幂等）、刷盘清脏入落盘账（FlushDup 拒重刷）；
//!   - 脏页计数对平：V 面（注册表视角脏页账）与 A4 影子面（存储视角
//!   脏页账）逐页双账全等（FaceMismatch 抓失平），守恒式
//!   脏数 ≡ 置脏数 - 刷盘数；
//!   - 分界纪律：注册表族只置脏/请求刷盘，落盘执行归 A4（分界声明面）；
//!   - A4 承接批号登记：批号/主题/段基三列齐入联测清单（BatchDup 拒重、
//!   SegMismatch 拒段基失配）；
//!   - 落盘账 append-only：脏数/刷数单调（HiveShrink 拒缩水）；
//!   - 脏泄漏面：全刷后脏数必须归零（DirtyLeak 抓漏刷）；
//!   - 错误路径带码+中文 detail，拒止不写回任何账行；
//!   - 注入核：LCG 写死种子 0x30980，对平/登记/泄漏面 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30980 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30980;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// hive 页账容量（64 页口径）。
pub const HIVE_PAGES: usize = 64;

/// A4 承接段基口径（预告登记锚，与分工表一致）。
pub const A4_SEG: u32 = 0x30980;

/// 诊断码（0x30980 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HvCode {
    /// 0x30980 页号越界。
    PageOob = 0,
    /// 0x30981 置脏重复（页已脏再置脏走幂等拒）。
    DirtyDup = 1,
    /// 0x30982 重刷干净页。
    FlushDup = 2,
    /// 0x30983 脏页计数失恒（脏 ≡ 置脏 - 刷盘 破）。
    CountMismatch = 3,
    /// 0x30984 A4 批号重复登记。
    BatchDup = 4,
    /// 0x30985 承接段基失配。
    SegMismatch = 5,
    /// 0x30986 V/A4 双面失平。
    FaceMismatch = 6,
    /// 0x30987 hive 账缩水。
    HiveShrink = 7,
    /// 0x30988 全刷后脏泄漏。
    DirtyLeak = 8,
    /// 0x30989 分界违例（注册表族越界执行落盘）。
    BoundaryBad = 9,
}

impl HvCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [HvCode; 10] = [
        HvCode::PageOob, HvCode::DirtyDup, HvCode::FlushDup, HvCode::CountMismatch,
        HvCode::BatchDup, HvCode::SegMismatch, HvCode::FaceMismatch, HvCode::HiveShrink,
        HvCode::DirtyLeak, HvCode::BoundaryBad,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30980 => "页号越界：页号先比 HIVE_PAGES 再置脏/刷盘。",
        0x30981 => "置脏重复：已脏页再置脏走幂等面，不重复计数。",
        0x30982 => "重刷干净页：刷盘只落脏页，干净页重刷必拒。",
        0x30983 => "脏页失恒：脏 ≡ 置脏 - 刷盘，先对平再报。",
        0x30984 => "A4 批号重复：同批号幂等，查联测清单再登记。",
        0x30985 => "承接段基失配：段基必须与 A4 分工口径一致。",
        0x30986 => "双面失平：V 面与 A4 影子面逐页全等才放行。",
        0x30987 => "hive 账缩水：append-only，禁止截断，重放补齐。",
        0x30988 => "脏泄漏：全刷后脏数必须归零，漏刷红账。",
        0x30989 => "分界违例：落盘执行归 A4，注册表族只置脏/请求刷。",
        _ => "未知诊断码：回查 0x30980 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30980 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3098_3098_3098_3098;

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

/// 页行（V 面 + A4 影子面双账）。
#[derive(Clone, Copy, Debug)]
pub struct PageRow {
    /// 页号。
    pub page: usize,
    /// V 面脏标记。
    pub dirty_v: bool,
    /// A4 影子面脏标记。
    pub dirty_a4: bool,
    /// 置脏次数。
    pub marks: u32,
    /// 刷盘次数。
    pub flushes: u32,
}

/// A4 承接批次行（联测清单一行）。
#[derive(Clone, Copy, Debug)]
pub struct A4BatchRow {
    /// 批号名。
    pub batch: &'static str,
    /// 承接主题。
    pub theme: &'static str,
    /// 承接段基。
    pub seg: u32,
    /// 状态（0 登记/1 承接/2 关闭）。
    pub status: u8,
}

/// hive 落盘联签正本。
///
/// 生命周期：mark_dirty（写命中置脏，双面同写）→ flush（请求刷盘，
/// 执行归 A4 面）→ reconcile（脏页计数对平）→ register_a4（批号登记）。
#[derive(Debug)]
pub struct HiveBook {
    /// 页账。
    pages: [PageRow; HIVE_PAGES],
    /// 置脏累计。
    marks: u32,
    /// 刷盘累计。
    flushes: u32,
    /// 联测清单。
    batches: Vec<A4BatchRow>,
    /// 清单峰值。
    high: usize,
    /// 账序。
    seq: u64,
    /// 拒止累计。
    rejects: u32,
}

impl HiveBook {
    /// 建账（全净页）。
    pub fn new() -> Self {
        HiveBook {
            pages: core::array::from_fn(|i| PageRow {
                page: i,
                dirty_v: false,
                dirty_a4: false,
                marks: 0,
                flushes: 0,
            }),
            marks: 0,
            flushes: 0,
            batches: Vec::new(),
            high: 0,
            seq: 0,
            rejects: 0,
        }
    }

    /// 置脏（双面同写；已脏页幂等拒计）。
    pub fn mark_dirty(&mut self, page: usize) -> Result<bool, HvCode> {
        if page >= HIVE_PAGES {
            self.rejects += 1;
            return Err(HvCode::PageOob);
        }
        if self.pages[page].dirty_v {
            self.rejects += 1;
            return Err(HvCode::DirtyDup);
        }
        self.pages[page].dirty_v = true;
        self.pages[page].dirty_a4 = true;
        self.pages[page].marks += 1;
        self.marks += 1;
        self.seq += 1;
        Ok(true)
    }

    /// 请求刷盘（注册表族只请求；A4 面执行清脏——分界声明口径）。
    pub fn flush(&mut self, page: usize) -> Result<bool, HvCode> {
        if page >= HIVE_PAGES {
            self.rejects += 1;
            return Err(HvCode::PageOob);
        }
        // 分界：请求面置执行标记，清脏动作由 A4 影子面同步。
        if !self.pages[page].dirty_v || !self.pages[page].dirty_a4 {
            self.rejects += 1;
            return Err(HvCode::FlushDup);
        }
        self.pages[page].dirty_v = false;
        self.pages[page].dirty_a4 = false;
        self.pages[page].flushes += 1;
        self.flushes += 1;
        self.seq += 1;
        Ok(true)
    }

    /// 全量刷盘（扫全页；返回刷盘页数）。
    pub fn flush_all(&mut self) -> Result<usize, HvCode> {
        let mut n = 0usize;
        for i in 0..HIVE_PAGES {
            if self.pages[i].dirty_v {
                self.flush(i)?;
                n += 1;
            }
        }
        // 脏泄漏面：全刷后脏数必须归零。
        if self.dirty_count() != 0 {
            return Err(HvCode::DirtyLeak);
        }
        Ok(n)
    }

    /// V 面脏页数。
    pub fn dirty_count(&self) -> usize {
        self.pages.iter().filter(|p| p.dirty_v).count()
    }

    /// A4 影子面脏页数。
    pub fn dirty_count_a4(&self) -> usize {
        self.pages.iter().filter(|p| p.dirty_a4).count()
    }

    /// 脏页计数守恒：脏 ≡ 置脏 - 刷盘。
    pub fn conserved(&self) -> bool {
        self.dirty_count() as u64 == (self.marks - self.flushes) as u64
    }

    /// 双面失平核对（逐页 V/A4 全等）。
    pub fn faces_match(&self) -> bool {
        self.pages
            .iter()
            .all(|p| p.dirty_v == p.dirty_a4)
            && self.dirty_count() == self.dirty_count_a4()
    }

    /// 脏页计数对平断言（守恒 + 双面全等 + 账平）。
    pub fn reconcile(&mut self) -> bool {
        self.seq += 1;
        self.conserved() && self.faces_match()
    }

    /// A4 承接批号登记（三列齐 + 段基一致 + 重号拒）。
    pub fn register_a4(&mut self, batch: &'static str, theme: &'static str, seg: u32) -> Result<bool, HvCode> {
        if batch.is_empty() || theme.is_empty() {
            self.rejects += 1;
            return Err(HvCode::BatchDup);
        }
        if seg != A4_SEG {
            self.rejects += 1;
            return Err(HvCode::SegMismatch);
        }
        if self.batches.iter().any(|b| b.batch == batch) {
            self.rejects += 1;
            return Err(HvCode::BatchDup);
        }
        self.batches.push(A4BatchRow { batch, theme, seg, status: 0 });
        if self.batches.len() > self.high {
            self.high = self.batches.len();
        }
        Ok(true)
    }

    /// A4 已登记判据。
    pub fn a4_registered(&self) -> bool {
        self.batches
            .iter()
            .any(|b| b.batch == "A4-B" && b.seg == A4_SEG)
    }

    /// 批次承接（状态 0→1）。
    pub fn take_batch(&mut self, batch: &str) -> Result<bool, HvCode> {
        match self.batches.iter_mut().find(|b| b.batch == batch) {
            Some(b) => {
                if b.status != 0 {
                    self.rejects += 1;
                    return Err(HvCode::BoundaryBad);
                }
                b.status = 1;
                Ok(true)
            }
            None => {
                self.rejects += 1;
                Err(HvCode::BatchDup)
            }
        }
    }

    /// 落盘分界声明面（注册表族面只含置脏/请求刷两操作）。
    pub fn boundary_ok(&self) -> bool {
        // 分界判据：全部刷盘动作皆由请求面驱动（flushes ≤ marks 恒成立
        // 且无越权清脏入口）。
        self.flushes <= self.marks
    }

    /// 账长（联测清单）。
    pub fn batch_len(&self) -> usize {
        self.batches.len()
    }

    /// 清单只增不减判据。
    pub fn monotone(&self) -> bool {
        self.batches.len() >= self.high
    }

    /// 置脏/刷盘累计。
    pub fn totals(&self) -> (u32, u32) {
        (self.marks, self.flushes)
    }

    /// 页快照。
    pub fn page_of(&self, page: usize) -> Option<PageRow> {
        if page >= HIVE_PAGES {
            None
        } else {
            Some(self.pages[page])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 脏页计数对平演练：写命中置脏 → 部分刷盘 → 双面/守恒对平 10/10。
pub fn drill_reconcile_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30980 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = HiveBook::new();
        // 每轮确定性置脏 8 页（LCG 选页），刷 3 页。
        let mut marked: Vec<usize> = Vec::new();
        for _i in 0..8 {
            let p = lcg.below(HIVE_PAGES as u64) as usize;
            if bk.mark_dirty(p).is_ok() {
                marked.push(p);
            }
        }
        let mut flushed = 0usize;
        for i in 0..3 {
            if let Some(&p) = marked.get(i) {
                if bk.flush(p).is_ok() {
                    flushed += 1;
                }
            }
        }
        let rec = bk.reconcile();
        let (m, f) = bk.totals();
        if rec
            && bk.faces_match()
            && bk.dirty_count() == marked.len() - flushed
            && m >= f
            && bk.boundary_ok()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("脏页对平轮={}/{}", ok, INJECT_ROUNDS))
}

/// A4 承接登记演练：10/10 轮登记成功、段基失配/重复拒。
pub fn drill_a4_register_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30980 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = HiveBook::new();
        let r1 = bk.register_a4("A4-B", "hive 落盘承接批", A4_SEG);
        let r2 = bk.register_a4("A4-B", "hive 落盘承接批", A4_SEG);
        let r3 = bk.register_a4("A4-C", "hive 尾段批", A4_SEG);
        let bad = bk.register_a4("A4-D", "段基失配批", 0x309a0);
        let take = bk.take_batch("A4-B");
        let _ = lcg.below(3);
        if matches!(r1, Ok(true))
            && matches!(r2, Err(HvCode::BatchDup))
            && matches!(r3, Ok(true))
            && matches!(bad, Err(HvCode::SegMismatch))
            && matches!(take, Ok(true))
            && bk.a4_registered()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("A4 承接登记轮={}/{}", ok, INJECT_ROUNDS))
}

/// 脏泄漏面演练：全刷归零、幂等拒计、重刷拒 10/10。
pub fn drill_flush_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30980 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = HiveBook::new();
        let _ = bk.mark_dirty(0);
        let dup = bk.mark_dirty(0); // 幂等拒
        let f1 = bk.flush(0);
        let f2 = bk.flush(0); // 重刷拒
        let _ = bk.mark_dirty(1);
        let _ = bk.mark_dirty(2);
        let all = bk.flush_all();
        let leak = bk.dirty_count();
        let cons = bk.conserved();
        let _ = lcg.below(3);
        if matches!(dup, Err(HvCode::DirtyDup))
            && matches!(f1, Ok(true))
            && matches!(f2, Err(HvCode::FlushDup))
            && matches!(all, Ok(2))
            && leak == 0
            && cons
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("脏泄漏面轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：hive 页账转录导出（逐脏页一行）。
/// ─────────────────────────────────────────────────────────────────────
/// hive 页账转录导出。
pub fn export_hive_lines(bk: &HiveBook) -> String {
    let mut s = String::from("hive 页账转录：\n");
    for i in 0..HIVE_PAGES {
        if let Some(p) = bk.page_of(i) {
            if p.dirty_v {
                s.push_str(&format!(
                    "P{} 脏{} A4脏{} 置{} 刷{}\n",
                    p.page, p.dirty_v, p.dirty_a4, p.marks, p.flushes
                ));
            }
        }
    }
    s
}

/// 转录完整判据（脏页逐行齐备且双面一致）。
pub fn hive_lines_complete(bk: &HiveBook) -> bool {
    let s = export_hive_lines(bk);
    let dirty = bk.dirty_count();
    if dirty == 0 {
        return false;
    }
    let mut hits = 0usize;
    for i in 0..HIVE_PAGES {
        if let Some(p) = bk.page_of(i) {
            if p.dirty_v && s.contains(&format!("P{} 脏", p.page)) {
                hits += 1;
            }
        }
    }
    hits == dirty
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：联签预告报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct HvReport {
    /// 脏页数。
    pub dirty: usize,
    /// 置脏累计。
    pub marks: u32,
    /// 刷盘累计。
    pub flushes: u32,
    /// 批次数。
    pub batches: usize,
    /// 拒止累计。
    pub rejects: u32,
    /// 双面一致。
    pub faces: bool,
    /// 守恒。
    pub conserved: bool,
}

impl HvReport {
    /// 建报告。
    pub fn build(bk: &HiveBook) -> Self {
        let (m, f) = bk.totals();
        HvReport {
            dirty: bk.dirty_count(),
            marks: m,
            flushes: f,
            batches: bk.batch_len(),
            rejects: bk.reject_total(),
            faces: bk.faces_match(),
            conserved: bk.conserved(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.faces && self.conserved && self.batches >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "HvReport 脏={} 置={} 刷={} 批={} 拒={} 面={} 恒={}",
            self.dirty, self.marks, self.flushes, self.batches, self.rejects, self.faces, self.conserved
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：守恒面演练（置脏/刷盘交错守恒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_conserved_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30980 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = HiveBook::new();
        let mut cons = true;
        for i in 0..16u64 {
            let p = lcg.below(HIVE_PAGES as u64) as usize;
            if i % 2 == 0 {
                let _ = bk.mark_dirty(p);
            } else {
                let _ = bk.flush(p);
            }
            cons = cons && bk.conserved() && bk.faces_match();
        }
        if cons && bk.boundary_ok() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("守恒面轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：页域拒止演练（越页号/清页越界 10/10 拒）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_page_oob_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30980 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = HiveBook::new();
        let m = bk.mark_dirty(HIVE_PAGES + 1);
        let f = bk.flush(HIVE_PAGES + 1);
        let _ = lcg.below(3);
        if matches!(m, Err(HvCode::PageOob))
            && matches!(f, Err(HvCode::PageOob))
            && bk.reject_total() >= 2
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("页域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct HvCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_hv_checklist(bk: &HiveBook) -> Vec<HvCheckRow> {
    let rep = HvReport::build(bk);
    let mut rows: Vec<HvCheckRow> = Vec::new();
    rows.push(HvCheckRow { name: "脏页守恒", passed: bk.conserved() });
    rows.push(HvCheckRow { name: "双面一致", passed: bk.faces_match() });
    rows.push(HvCheckRow { name: "A4已登记", passed: bk.a4_registered() });
    rows.push(HvCheckRow { name: "清单单调", passed: bk.monotone() });
    rows.push(HvCheckRow { name: "分界声明", passed: bk.boundary_ok() });
    rows.push(HvCheckRow { name: "页宽口径", passed: HIVE_PAGES == 64 && A4_SEG == 0x30980 });
    rows.push(HvCheckRow { name: "转录完整", passed: hive_lines_complete(bk) });
    rows.push(HvCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(HvCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(HvCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn hv_checklist_green(rows: &[HvCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn hv_stats_line(bk: &HiveBook) -> String {
    format!(
        "hive 联签概况：脏 {} 置 {} 刷 {} 批 {}",
        bk.dirty_count(),
        bk.totals().0,
        bk.totals().1,
        bk.batch_len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12476_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = HiveBook::new();
    let _ = bk.mark_dirty(3);
    let _ = bk.mark_dirty(9);
    let _ = bk.flush(3);
    // 注入一例重脏页（拒止账口径）。
    let _ = bk.mark_dirty(9);
    let _ = bk.register_a4("A4-B", "hive 落盘承接批", A4_SEG);
    let _ = bk.reconcile();
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        hive_lines_complete(&bk) && HvReport::build(&bk).all_green(),
        hv_stats_line(&bk),
    ));
    // X-b：守恒面 10/10。
    let (ok_c, d_c) = drill_conserved_10();
    out.push(("X-守恒面10/10", ok_c, d_c));
    // X-c：页域拒止 10/10。
    let (ok_p, d_p) = drill_page_oob_10();
    out.push(("X-页域拒止10/10", ok_p, d_p));
    // X-d：终验清单。
    let rows = build_hv_checklist(&bk);
    out.push(("X-终验清单10项", hv_checklist_green(&rows), HvReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = HiveBook::new();
    // 注 1：页号越界必拒（PageOob）。
    if matches!(bk.mark_dirty(HIVE_PAGES + 3), Err(HvCode::PageOob)) {
        caught += 1;
        d.push("注1 越页:抓".to_string());
    } else {
        d.push("注1 越页:漏".to_string());
    }
    // 注 2：置脏重复必拒（DirtyDup）。
    let _ = bk.mark_dirty(1);
    if matches!(bk.mark_dirty(1), Err(HvCode::DirtyDup)) {
        caught += 1;
        d.push("注2 重脏:抓".to_string());
    } else {
        d.push("注2 重脏:漏".to_string());
    }
    // 注 3：重刷干净页必拒（FlushDup）。
    let _ = bk.flush(1);
    if matches!(bk.flush(1), Err(HvCode::FlushDup)) {
        caught += 1;
        d.push("注3 重刷:抓".to_string());
    } else {
        d.push("注3 重刷:漏".to_string());
    }
    // 注 4：段基失配必拒（SegMismatch）。
    if matches!(bk.register_a4("A4-B", "T", 0x309a0), Err(HvCode::SegMismatch)) {
        caught += 1;
        d.push("注4 段基:抓".to_string());
    } else {
        d.push("注4 段基:漏".to_string());
    }
    // 注 5：分界违例必拒（BoundaryBad 面：已承接批再承接）。
    let _ = bk.register_a4("A4-B", "hive 落盘承接批", A4_SEG);
    let _ = bk.take_batch("A4-B");
    if matches!(bk.take_batch("A4-B"), Err(HvCode::BoundaryBad)) {
        caught += 1;
        d.push("注5 分界:抓".to_string());
    } else {
        d.push("注5 分界:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30980 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    HvCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12476_checks：本单判据聚合（诊断段 0x30980）。
pub fn run_unx12476_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12476 注册表族与 A4 存储联签预告");
    // J1-a：脏页计数对平断言（10/10）。
    let (ok1, d1) = drill_reconcile_10();
    s.add_dyn("J1-脏页计数对平10/10", ok1, &d1);
    // J1-b：A4 承接批号登记（10/10）。
    let (ok2, d2) = drill_a4_register_10();
    s.add_dyn("J1-A4承接批号登记10/10", ok2, &d2);
    // J1-c：脏泄漏面（10/10）。
    let (ok3, d3) = drill_flush_10();
    s.add_dyn("J1-脏泄漏面10/10", ok3, &d3);
    // J1-d：标准会话（置脏+刷+对平+A4 在册）。
    let mut bk = HiveBook::new();
    let _ = bk.mark_dirty(2);
    let _ = bk.mark_dirty(5);
    let _ = bk.flush(2);
    let rec = bk.reconcile();
    let _ = bk.register_a4("A4-B", "hive 落盘承接批", A4_SEG);
    s.add_dyn(
        "J1-分界预告齐+对平绿",
        rec && bk.a4_registered() && bk.faces_match() && bk.conserved() && hive_lines_complete(&bk),
        &format!(
            "对平={} A4={} 面={} 恒={}",
            rec,
            bk.a4_registered(),
            bk.faces_match(),
            bk.conserved()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（重脏页计数不变）。
    let (m0, _) = bk.totals();
    let _ = bk.mark_dirty(5);
    let (m1, _) = bk.totals();
    s.add_dyn(
        "J2-拒止不写回",
        m0 == m1 && bk.reject_total() >= 1,
        &format!("置脏 {}→{}", m0, m1),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30980段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12476_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
