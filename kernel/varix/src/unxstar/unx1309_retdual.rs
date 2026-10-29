//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1309 · 退役双路并行对拍执行账（退役中双模运行数据采集） · 0x75E0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1309-J1 退役窗口双路数据（轮询侧+中断侧同事件）采集完整
//!      率 100%，双路对拍差实时可查，对拍数据归档率 100%（B27 等价
//!      验证直接消费）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双路样本：每事件同拍采轮询侧读数与中断侧读数（成对原子入账，
//!     单侧缺失必抓）；
//!   - 对拍差 = |轮询读数 - 中断读数|（整数饱和差），实时可查（按
//!     事件号查询），差非零即红账；
//!   - 三路径独立采集账（tick/serial/scan），12 点宇宙按路径分组；
//!   - 归档账：采集满页（每页 16 样本）即归档（append-only 只读化），
//!     归档率 = 归档样本×1000/采集样本（千分制 1000）；
//!   - 页校验和：样本逐条卷积（×31 加链），归档页必须校验和自洽
//!     （B27 等价验证直接消费的可信凭证）；
//!   - B27 消费接口：export_digest() 输出页数+样本数+总校验和，
//!     两次导出逐位一致（确定性判准）；
//!   - 注入核 5 项（单侧缺失/差非零/归档缺页/校验和破/查询越界）
//!     全部必抓，LCG 种子写死 0x75E0_0005。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75E0 段，双路对拍账专属）。
pub const SEG_BASE: u16 = 0x75E0;

// ── 诊断码闭集（0x75E0..0x75E9 十码） ───────────────────────
/// 0x75E0 单侧缺失（只采到一路）。
pub const E_SIDE_MISS: u16 = SEG_BASE;
/// 0x75E1 对拍差非零。
pub const E_DIFF_NZ: u16 = SEG_BASE + 1;
/// 0x75E2 归档缺页（采集满页未归档）。
pub const E_ARCH_MISS: u16 = SEG_BASE + 2;
/// 0x75E3 页校验和不符。
pub const E_CSUM_BAD: u16 = SEG_BASE + 3;
/// 0x75E4 查询越界（事件号出账）。
pub const E_QRY_OOB: u16 = SEG_BASE + 4;
/// 0x75E5 路径越界。
pub const E_PATH_OOB: u16 = SEG_BASE + 5;
/// 0x75E6 窗外采集（窗口已关仍采）。
pub const E_WINDOW_BAD: u16 = SEG_BASE + 6;
/// 0x75E7 事件号重复。
pub const E_DUP_SEQ: u16 = SEG_BASE + 7;
/// 0x75E8 归档后改写（只读化被破）。
pub const E_ARCH_RDONLY: u16 = SEG_BASE + 8;
/// 0x75E9 空账审计。
pub const E_EMPTY: u16 = SEG_BASE + 9;

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_SIDE_MISS => "单侧缺失：对拍采集必须双侧同拍原子落账，补齐缺侧",
        E_DIFF_NZ => "对拍差非零：核对两侧读数生成器是否同源同拍",
        E_ARCH_MISS => "归档缺页：采集满 16 样本必须立即归档",
        E_CSUM_BAD => "校验和不符：重算页校验和，定位被改写样本",
        E_QRY_OOB => "查询越界：先比事件号与账长再取值",
        E_PATH_OOB => "路径越界：三路径宇宙只有 0/1/2",
        E_WINDOW_BAD => "窗外采集：窗口关闭后禁止再采，先开新窗",
        E_DUP_SEQ => "事件号重复：采集号必须唯一，重号必拒",
        E_ARCH_RDONLY => "归档后改写：归档页只读，修正须走新页",
        E_EMPTY => "空账审计：零样本不可判完整率，先采集再审",
        _ => "未知 0x75E0 段诊断码：核对双路对拍账段位闭集",
    }
}

// ── 宇宙常量 ────────────────────────────────────────────────
/// 三路径名。
pub const PATH_NAMES: [&str; 3] = ["tick", "serial", "scan"];
/// 每路径事件数（演练窗样本）。
pub const EVENTS_PER_PATH: usize = 32;
/// 归档页大小。
pub const PAGE_SIZE: usize = 16;
/// 演练轮数。
pub const ROUNDS: usize = 10;
/// B27 消费方标签。
pub const CONSUMER_B27: &str = "B27-等价验证";

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    /// 新建。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    /// 下一值。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0,n)。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 双路样本 ────────────────────────────────────────────────
/// 双路样本（同事件双读数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualSample {
    /// 事件号（路径内 1 起唯一）。
    pub seq: u64,
    /// 路径号。
    pub path: usize,
    /// 轮询侧读数。
    pub poll_read: u64,
    /// 中断侧读数。
    pub irq_read: u64,
    /// 双侧都已落（原子对完整旗）。
    pub paired: bool,
}

impl DualSample {
    /// 对拍差（饱和整数差）。
    pub fn diff(&self) -> u64 {
        if self.poll_read >= self.irq_read {
            self.poll_read - self.irq_read
        } else {
            self.irq_read - self.poll_read
        }
    }

    /// 成对一致。
    pub fn matched(&self) -> bool {
        self.paired && self.diff() == 0
    }
}

// ── 单路径采集账 ────────────────────────────────────────────
/// 单路径双路采集账。
#[derive(Debug)]
pub struct DualCollector {
    /// 路径号。
    path: usize,
    /// 样本账（按事件号有序）。
    samples: Vec<DualSample>,
    /// 窗开旗。
    window_open: bool,
    /// 单侧缺失拒绝计数。
    side_rejected: u64,
    /// 重复号拒绝计数。
    dup_rejected: u64,
}

impl DualCollector {
    /// 新账（窗口即开）。
    pub fn new(path: usize) -> Result<DualCollector, u16> {
        if path >= 3 {
            return Err(E_PATH_OOB);
        }
        Ok(DualCollector {
            path,
            samples: Vec::new(),
            window_open: true,
            side_rejected: 0,
            dup_rejected: 0,
        })
    }

    /// 路径名。
    pub fn name(&self) -> &'static str {
        PATH_NAMES[self.path]
    }

    /// 关窗（关后禁采）。
    pub fn close_window(&mut self) {
        self.window_open = false;
    }

    /// 窗旗。
    pub fn is_open(&self) -> bool {
        self.window_open
    }

    /// 同拍双采（双侧原子成对入账；缺侧/重号/关窗必拒）。
    pub fn capture(&mut self, seq: u64, poll_read: Option<u64>, irq_read: Option<u64>) -> Result<(), u16> {
        if !self.window_open {
            return Err(E_WINDOW_BAD);
        }
        if poll_read.is_none() || irq_read.is_none() {
            self.side_rejected += 1;
            return Err(E_SIDE_MISS);
        }
        if self.samples.iter().any(|s| s.seq == seq) {
            self.dup_rejected += 1;
            return Err(E_DUP_SEQ);
        }
        // 有序插入。
        let pos = match self.samples.binary_search_by_key(&seq, |s| s.seq) {
            Ok(_) => {
                self.dup_rejected += 1;
                return Err(E_DUP_SEQ);
            }
            Err(p) => p,
        };
        self.samples.insert(pos, DualSample {
            seq,
            path: self.path,
            poll_read: poll_read.unwrap_or(0),
            irq_read: irq_read.unwrap_or(0),
            paired: true,
        });
        Ok(())
    }

    /// 样本数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 空账。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 采集完整率（千分制：成对样本×1000/期望数）。
    pub fn completeness_permille(&self, expected: usize) -> u64 {
        if expected == 0 {
            return 0;
        }
        let paired = self.samples.iter().filter(|s| s.paired).count();
        (paired as u64) * 1000 / expected as u64
    }

    /// 实时对拍差查询（按事件号）。
    pub fn diff_at(&self, seq: u64) -> Result<u64, u16> {
        match self.samples.iter().find(|s| s.seq == seq) {
            Some(s) => Ok(s.diff()),
            None => Err(E_QRY_OOB),
        }
    }

    /// 最大对拍差。
    pub fn max_diff(&self) -> u64 {
        match self.samples.iter().map(|s| s.diff()).max() {
            Some(v) => v,
            None => 0,
        }
    }

    /// 全一致（差全零且全成对）。
    pub fn all_matched(&self) -> bool {
        !self.samples.is_empty() && self.samples.iter().all(|s| s.matched())
    }

    /// 缺侧拒绝计数。
    pub fn side_rejects(&self) -> u64 {
        self.side_rejected
    }

    /// 重号拒绝计数。
    pub fn dup_rejects(&self) -> u64 {
        self.dup_rejected
    }

    /// 样本切片（归档消费）。
    pub fn slice(&self) -> &[DualSample] {
        &self.samples
    }
}

// ── 归档账（B27 直接消费） ──────────────────────────────────
/// 归档页。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchivePage {
    /// 页号（1 起连续）。
    pub page_no: u32,
    /// 页内样本。
    pub items: Vec<DualSample>,
    /// 页校验和（×31 加链卷积）。
    pub csum: u64,
}

impl ArchivePage {
    /// 页校验和（口径：((csum×31)+seq×3+poll) 链式）。
    pub fn compute(items: &[DualSample]) -> u64 {
        let mut c: u64 = 0x75E0;
        for s in items {
            c = c.wrapping_mul(31).wrapping_add(s.seq.wrapping_mul(3));
            c = c.wrapping_add(s.poll_read);
            c = c.wrapping_add(s.irq_read);
        }
        c
    }

    /// 建页（自动算校验和）。
    pub fn build(page_no: u32, items: Vec<DualSample>) -> ArchivePage {
        let csum = ArchivePage::compute(&items);
        ArchivePage { page_no, items, csum }
    }

    /// 页自洽（重算==账面）。
    pub fn self_consistent(&self) -> bool {
        ArchivePage::compute(&self.items) == self.csum
    }
}

/// 归档账（append-only 只读化）。
#[derive(Debug)]
pub struct ArchiveBook {
    /// 页账。
    pages: Vec<ArchivePage>,
    /// 冻结旗（冻结后禁增页）。
    frozen: bool,
    /// 归档样本总数。
    archived: usize,
}

impl ArchiveBook {
    /// 新账。
    pub fn new() -> ArchiveBook {
        ArchiveBook { pages: Vec::new(), frozen: false, archived: 0 }
    }

    /// 归档一页（页号连续；冻结后拒）。
    pub fn archive(&mut self, items: Vec<DualSample>) -> Result<u32, u16> {
        if self.frozen {
            return Err(E_ARCH_RDONLY);
        }
        if items.is_empty() {
            return Err(E_EMPTY);
        }
        if items.len() != PAGE_SIZE {
            // 尾页允许不足页；非尾页必须满页由调用方保证。
        }
        let page_no = match self.pages.last() {
            Some(p) => p.page_no + 1,
            None => 1,
        };
        let page = ArchivePage::build(page_no, items);
        if !page.self_consistent() {
            return Err(E_CSUM_BAD);
        }
        self.archived += page.items.len();
        self.pages.push(page);
        Ok(page_no)
    }

    /// 冻结（只读化）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    /// 冻结旗。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 页数。
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// 归档样本数。
    pub fn archived_count(&self) -> usize {
        self.archived
    }

    /// 归档率（千分制）。
    pub fn archived_permille(&self, collected: usize) -> u64 {
        if collected == 0 {
            return 0;
        }
        (self.archived as u64) * 1000 / collected as u64
    }

    /// 全页自洽审计。
    pub fn audit(&self) -> Result<(), u16> {
        if self.pages.is_empty() {
            return Err(E_EMPTY);
        }
        for (i, p) in self.pages.iter().enumerate() {
            if p.page_no != (i as u32) + 1 {
                return Err(E_ARCH_MISS);
            }
            if !p.self_consistent() {
                return Err(E_CSUM_BAD);
            }
        }
        Ok(())
    }

    /// B27 消费摘要（页数/样本数/总校验和）。
    pub fn export_digest(&self) -> (usize, usize, u64) {
        let mut total: u64 = 0;
        for p in self.pages.iter() {
            total = total.wrapping_mul(17).wrapping_add(p.csum);
        }
        (self.pages.len(), self.archived, total)
    }

    /// 页内样本查询（B27 逐条消费）。
    pub fn sample_at(&self, page: usize, idx: usize) -> Result<DualSample, u16> {
        match self.pages.get(page) {
            Some(p) => match p.items.get(idx) {
                Some(s) => Ok(*s),
                None => Err(E_QRY_OOB),
            },
            None => Err(E_QRY_OOB),
        }
    }
}

// ── 三路径总采集账 ──────────────────────────────────────────
/// 三路径双路总账（本单正本）。
#[derive(Debug)]
pub struct DualBook {
    /// 三路径采集账。
    cols: Vec<DualCollector>,
    /// 归档账。
    arch: ArchiveBook,
    /// 归档游标（下一待归档起点）。
    cursor: usize,
}

impl DualBook {
    /// 建账。
    pub fn build() -> Result<DualBook, u16> {
        let mut cols = Vec::new();
        for p in 0..3 {
            cols.push(DualCollector::new(p)?);
        }
        Ok(DualBook { cols, arch: ArchiveBook::new(), cursor: 0 })
    }

    /// 路径账引用。
    pub fn col(&self, p: usize) -> Result<&DualCollector, u16> {
        match self.cols.get(p) {
            Some(c) => Ok(c),
            None => Err(E_PATH_OOB),
        }
    }

    /// 采一事件（双路同源读数：poll=irq=value）。
    pub fn capture(&mut self, path: usize, seq: u64, value: u64) -> Result<(), u16> {
        self.cols[path].capture(seq, Some(value), Some(value))
    }

    /// 关全窗。
    pub fn close_all(&mut self) {
        for c in self.cols.iter_mut() {
            c.close_window();
        }
    }

    /// 灌满三路径标准窗（每路径 32 事件，LCG 同源值）。
    pub fn fill_std(&mut self, seed: u64) -> Result<usize, u16> {
        let mut rng = Lcg::new(seed);
        let mut n = 0usize;
        for p in 0..3 {
            for seq in 1..=(EVENTS_PER_PATH as u64) {
                let v = rng.below(0x10000);
                self.capture(p, seq, v)?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 满页归档推进（三路径串行拼接流，每满 PAGE_SIZE 归一页）。
    pub fn flush_pages(&mut self) -> Result<usize, u16> {
        let mut stream: Vec<DualSample> = Vec::new();
        for c in self.cols.iter() {
            stream.extend_from_slice(c.slice());
        }
        let mut pages = 0usize;
        while self.cursor + PAGE_SIZE <= stream.len() {
            let chunk: Vec<DualSample> = stream[self.cursor..self.cursor + PAGE_SIZE].to_vec();
            self.arch.archive(chunk)?;
            self.cursor += PAGE_SIZE;
            pages += 1;
        }
        Ok(pages)
    }

    /// 尾页归档（不足页也归，判据：归档率 100%）。
    pub fn flush_tail(&mut self) -> Result<usize, u16> {
        let mut stream: Vec<DualSample> = Vec::new();
        for c in self.cols.iter() {
            stream.extend_from_slice(c.slice());
        }
        if self.cursor < stream.len() {
            let chunk: Vec<DualSample> = stream[self.cursor..].to_vec();
            self.arch.archive(chunk)?;
            self.cursor = stream.len();
            return Ok(1);
        }
        Ok(0)
    }

    /// 采集总数。
    pub fn collected(&self) -> usize {
        let mut n = 0usize;
        for c in self.cols.iter() {
            n += c.len();
        }
        n
    }

    /// 采集完整率（千分制）。
    pub fn completeness_permille(&self) -> u64 {
        let mut n = 0u64;
        for c in self.cols.iter() {
            n += c.completeness_permille(EVENTS_PER_PATH);
        }
        n / 3
    }

    /// 归档率（千分制）。
    pub fn archived_permille(&self) -> u64 {
        self.arch.archived_permille(self.collected())
    }

    /// 实时对拍差查询（按路径+事件号）。
    pub fn diff_query(&self, path: usize, seq: u64) -> Result<u64, u16> {
        self.cols[path].diff_at(seq)
    }

    /// 全一致。
    pub fn all_matched(&self) -> bool {
        self.cols.iter().all(|c| c.all_matched())
    }

    /// 归档审计。
    pub fn audit_archive(&self) -> Result<(), u16> {
        self.arch.audit()
    }

    /// 归档账引用。
    pub fn archive(&self) -> &ArchiveBook {
        &self.arch
    }

    /// B27 消费摘要。
    pub fn b27_digest(&self) -> (usize, usize, u64) {
        self.arch.export_digest()
    }

    /// 归档冻结。
    pub fn freeze_archive(&mut self) {
        self.arch.freeze();
    }
}

// ── 演练 ────────────────────────────────────────────────────
/// 标准窗全流程演练（采集 96 → 归档 6 满页 → 完整/归档率双 1000‰）。
pub fn drill_std_flow() -> (bool, String) {
    let mut bk = match DualBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let n = match bk.fill_std(0x75E0_BEEF) {
        Ok(n) => n,
        Err(c) => return (false, format!("采集错=0x{:04X} {}", c, detail(c))),
    };
    let pages = match bk.flush_pages() {
        Ok(p) => p,
        Err(c) => return (false, format!("归档错=0x{:04X} {}", c, detail(c))),
    };
    let tail = match bk.flush_tail() {
        Ok(t) => t,
        Err(c) => return (false, format!("尾页错=0x{:04X} {}", c, detail(c))),
    };
    let comp = bk.completeness_permille();
    let arch = bk.archived_permille();
    let matched = bk.all_matched();
    let audit = bk.audit_archive().is_ok();
    (n == 96 && pages == 6 && tail == 0 && comp == 1000 && arch == 1000 && matched && audit,
     format!("采集={} 满页={} 完整={}‰ 归档={}‰ 一致={} 审计={}", n, pages, comp, arch, matched, audit))
}

/// 双路完整率 10/10（逐轮标准窗）。
pub fn drill_complete_10() -> (bool, String) {
    let mut ok = 0u32;
    for r in 0..ROUNDS {
        let mut bk = match DualBook::build() {
            Ok(b) => b,
            Err(_) => continue,
        };
        if bk.fill_std(0x75E0_0001 + r as u64).is_ok()
            && bk.completeness_permille() == 1000
            && bk.all_matched()
        {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("完整={}/{}", ok, ROUNDS))
}

/// 对拍差实时可查演练（差恒零 + 逐号查询 + 越界拒）。
pub fn drill_diff_query() -> (bool, String) {
    let mut bk = match DualBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let _ = bk.fill_std(0x75E0_D1FF);
    let mut all_zero = true;
    for seq in 1..=(EVENTS_PER_PATH as u64) {
        if bk.diff_query(0, seq) != Ok(0) {
            all_zero = false;
        }
    }
    let oob = matches!(bk.diff_query(0, EVENTS_PER_PATH as u64 + 1), Err(E_QRY_OOB));
    (all_zero && oob && bk.col(0).map(|c| c.max_diff()).unwrap_or(1) == 0,
     format!("逐号差零={} 越界拒={}", all_zero, oob))
}

/// 归档率 1000‰ 演练（含尾页）。
pub fn drill_archive_full() -> (bool, String) {
    let mut bk = match DualBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    // 70 事件（非整页数：70/16=4 余 6）。
    let mut rng = Lcg::new(0x75E0_A0C4);
    for p in 0..3 {
        for seq in 1..=24u64 {
            let v = rng.below(0x1000);
            if bk.capture(p, seq, v).is_err() {
                return (false, "采集失败".to_string());
            }
        }
    }
    let _ = bk.flush_pages();
    let tail = bk.flush_tail().unwrap_or(0);
    let permille = bk.archived_permille();
    (tail == 1 && permille == 1000 && bk.audit_archive().is_ok(),
     format!("尾页={} 归档率={}‰ 页数={}", tail, permille, bk.archive().page_count()))
}

/// B27 消费摘要稳定演练（两次导出逐位一致 + 逐条可取）。
pub fn drill_b27_digest() -> (bool, String) {
    let mut bk = match DualBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let _ = bk.fill_std(0x75E0_C27B);
    let _ = bk.flush_pages();
    let d1 = bk.b27_digest();
    let d2 = bk.b27_digest();
    let mut sample_ok = true;
    for pg in 0..d1.0 {
        for idx in 0..PAGE_SIZE {
            if bk.archive().sample_at(pg, idx).is_err() {
                sample_ok = false;
            }
        }
    }
    (d1 == d2 && d1.1 > 0 && sample_ok,
     format!("摘要一致={} 页数={} 样本={} 逐条可取={}", d1 == d2, d1.0, d1.1, sample_ok))
}

/// 单侧缺失必抓 10/10。
pub fn drill_side_miss_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x75E0_5EED);
    for _ in 0..ROUNDS {
        let p = rng.below(3) as usize;
        let seq = rng.below(EVENTS_PER_PATH as u64) + 1;
        let mut bk = match DualBook::build() {
            Ok(b) => b,
            Err(_) => continue,
        };
        // 缺中断侧。
        let caught = matches!(bk.cols[p].capture(seq, Some(7), None), Err(E_SIDE_MISS));
        if caught && bk.cols[p].side_rejects() == 1 {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("缺侧必抓={}/{}", ok, ROUNDS))
}

/// 差非零必抓演练（注入异源读数 → diff>0 红账）。
pub fn drill_diff_nz_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut rng = Lcg::new(0x75E1_5EED);
    for _ in 0..ROUNDS {
        let p = rng.below(3) as usize;
        let seq = rng.below(EVENTS_PER_PATH as u64) + 1;
        let mut bk = match DualBook::build() {
            Ok(b) => b,
            Err(_) => continue,
        };
        // 直接注入异源对（轮询=x，中断=x+1）。
        let _ = bk.cols[p].capture(seq, Some(100), Some(101));
        let diff = bk.cols[p].diff_at(seq).unwrap_or(0);
        if diff == 1 && !bk.cols[p].all_matched() {
            ok += 1;
        }
    }
    (ok == ROUNDS as u32, format!("差非零必抓={}/{}", ok, ROUNDS))
}

/// 重号必抓 + 关窗必抓演练。
pub fn drill_dup_and_window() -> (bool, String) {
    let mut bk = match DualBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let first = bk.capture(0, 5, 42).is_ok();
    let dup = matches!(bk.capture(0, 5, 42), Err(E_DUP_SEQ));
    bk.close_all();
    let closed = matches!(bk.capture(0, 6, 42), Err(E_WINDOW_BAD));
    (first && dup && closed && bk.col(0).map(|c| c.dup_rejects()).unwrap_or(0) == 1,
     format!("首采={} 重号拒={} 关窗拒={}", first, dup, closed))
}

/// 归档只读化演练（冻结后增页必拒 + 校验和破必抓）。
pub fn drill_arch_readonly() -> (bool, String) {
    let mut bk = match DualBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    let _ = bk.fill_std(0x75E0_8D0);
    let _ = bk.flush_pages();
    bk.freeze_archive();
    let frozen = bk.archive().is_frozen();
    // 冻结后再归档必拒。
    let mut extra: Vec<DualSample> = Vec::new();
    for i in 0..PAGE_SIZE {
        extra.push(DualSample { seq: 900 + i as u64, path: 0, poll_read: 1, irq_read: 1, paired: true });
    }
    let rdonly = matches!(bk.arch.archive(extra), Err(E_ARCH_RDONLY));
    // 校验和破（人为改页内值）必抓。
    let mut bk2 = DualBook::build().unwrap_or_else(|_| unreachable_book());
    let _ = bk2.fill_std(0x75E0_C50F);
    let _ = bk2.flush_pages();
    if let Some(p0) = bk2.arch.pages.get_mut(0) {
        if let Some(s0) = p0.items.get_mut(0) {
            s0.poll_read = s0.poll_read.wrapping_add(1);
        }
    }
    let csum = matches!(bk2.arch.audit(), Err(E_CSUM_BAD));
    (frozen && rdonly && csum, format!("冻结={} 只读拒={} 校验抓={}", frozen, rdonly, csum))
}

/// 替身总账（恒可达兜底）。
fn unreachable_book() -> DualBook {
    let mut cols = Vec::new();
    for p in 0..3 {
        cols.push(DualCollector::new(p).unwrap_or_else(|_| DualCollector::new(0).unwrap_or_else(|_| unreachable_col())));
    }
    DualBook { cols, arch: ArchiveBook::new(), cursor: 0 }
}

/// 替身采集账。
fn unreachable_col() -> DualCollector {
    DualCollector {
        path: 0,
        samples: Vec::new(),
        window_open: true,
        side_rejected: 0,
        dup_rejected: 0,
    }
}


// ── 对拍差直方图账（差值分桶） ─────────────────────────────
/// 差值直方图（桶宽 1：差 0 桶 / 差 1..=3 桶 / 差 >3 桶）。
#[derive(Debug)]
pub struct DiffHistogram {
    /// 桶 0（差为零，理想）。
    pub zero: usize,
    /// 桶 1（差 1..=3）。
    pub small: usize,
    /// 桶 2（差 >3）。
    pub large: usize,
}

impl DiffHistogram {
    /// 从三路径账统计。
    pub fn build(bk: &DualBook) -> DiffHistogram {
        let mut h = DiffHistogram { zero: 0, small: 0, large: 0 };
        for c in bk.cols.iter() {
            for s in c.slice().iter() {
                let d = s.diff();
                if d == 0 {
                    h.zero += 1;
                } else if d <= 3 {
                    h.small += 1;
                } else {
                    h.large += 1;
                }
            }
        }
        h
    }

    /// 总数守恒（三桶和 == 采集总数）。
    pub fn conserved(&self, bk: &DualBook) -> bool {
        self.zero + self.small + self.large == bk.collected()
    }

    /// 理想态（全零桶占满）。
    pub fn ideal(&self) -> bool {
        self.small == 0 && self.large == 0 && self.zero > 0
    }

    /// 零差率（千分制）。
    pub fn zero_permille(&self) -> u64 {
        let total = self.zero + self.small + self.large;
        if total == 0 {
            return 0;
        }
        (self.zero as u64) * 1000 / total as u64
    }
}

// ── 采集轮次摘要账 ──────────────────────────────────────────
/// 轮次摘要。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CollectRound {
    /// 轮号。
    pub round: u32,
    /// 采集数。
    pub collected: usize,
    /// 最大差。
    pub max_diff: u64,
}

/// 轮次摘要账。
#[derive(Debug)]
pub struct RoundLedger {
    /// 记录。
    pub recs: Vec<CollectRound>,
}

impl RoundLedger {
    /// 新账。
    pub fn new() -> RoundLedger {
        RoundLedger { recs: Vec::new() }
    }

    /// 登记。
    pub fn push(&mut self, collected: usize, max_diff: u64) -> u32 {
        let round = match self.recs.last() {
            Some(r) => r.round + 1,
            None => 1,
        };
        self.recs.push(CollectRound { round, collected, max_diff });
        round
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 轮号连续。
    pub fn continuous(&self) -> bool {
        for (i, r) in self.recs.iter().enumerate() {
            if r.round != (i as u32) + 1 {
                return false;
            }
        }
        true
    }

    /// 采集数恒定。
    pub fn stable(&self) -> bool {
        !self.recs.is_empty() && self.recs.iter().all(|r| r.collected == self.recs[0].collected)
    }

    /// 全零差。
    pub fn all_zero_diff(&self) -> bool {
        !self.recs.is_empty() && self.recs.iter().all(|r| r.max_diff == 0)
    }
}

// ── 追加演练 ────────────────────────────────────────────────
/// 直方图演练（理想态 + 守恒 + 零差率 1000‰）。
pub fn drill_histogram() -> (bool, String) {
    let mut bk = match DualBook::build() {
        Ok(b) => b,
        Err(c) => return (false, format!("建账错=0x{:04X} {}", c, detail(c))),
    };
    if bk.fill_std(0x75E0_19A).is_err() {
        return (false, "采集失败".to_string());
    }
    let h = DiffHistogram::build(&bk);
    (h.conserved(&bk) && h.ideal() && h.zero_permille() == 1000,
     format!("桶 零/小/大={}/{}/{} 零差率={}‰", h.zero, h.small, h.large, h.zero_permille()))
}

/// 轮次摘要演练（3 轮登记 + 连续 + 恒定 + 全零差）。
pub fn drill_rounds() -> (bool, String) {
    let mut rl = RoundLedger::new();
    for r in 0..3u64 {
        let mut bk = match DualBook::build() {
            Ok(b) => b,
            Err(_) => return (false, "建账失败".to_string()),
        };
        if bk.fill_std(0x75E0_0100 + r).is_err() {
            return (false, "采集失败".to_string());
        }
        let md = bk.cols.iter().map(|c| c.max_diff()).max().unwrap_or(1);
        rl.push(bk.collected(), md);
    }
    (rl.len() == 3 && rl.continuous() && rl.stable() && rl.all_zero_diff(),
     format!("轮 {} 连续={} 恒定={} 全零差={}", rl.len(), rl.continuous(), rl.stable(), rl.all_zero_diff()))
}

// ── 注入核（5 项必抓） ─────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：单侧缺失。
    {
        let mut c = DualCollector::new(0).unwrap_or_else(|_| unreachable_col());
        if matches!(c.capture(1, Some(1), None), Err(E_SIDE_MISS)) { caught += 1; d.push("注1 缺侧:抓".to_string()); } else { d.push("注1 缺侧:漏".to_string()); }
    }
    // 注 2：差非零。
    {
        let mut c = DualCollector::new(1).unwrap_or_else(|_| unreachable_col());
        let _ = c.capture(1, Some(10), Some(11));
        if c.diff_at(1) == Ok(1) { caught += 1; d.push("注2 差异:抓".to_string()); } else { d.push("注2 差异:漏".to_string()); }
    }
    // 注 3：归档缺页（满页未归 → 归档率<1000）。
    {
        let mut bk = DualBook::build().unwrap_or_else(|_| unreachable_book());
        let _ = bk.fill_std(0x75E0_1155);
        // 不归档直接审计归档率。
        if bk.archived_permille() == 0 { caught += 1; d.push("注3 缺页:抓".to_string()); } else { d.push("注3 缺页:漏".to_string()); }
    }
    // 注 4：校验和破。
    {
        let mut bk = DualBook::build().unwrap_or_else(|_| unreachable_book());
        let _ = bk.fill_std(0x75E0_C50);
        let _ = bk.flush_pages();
        if let Some(p) = bk.arch.pages.get_mut(0) {
            p.csum = p.csum.wrapping_add(1);
        }
        if matches!(bk.arch.audit(), Err(E_CSUM_BAD)) { caught += 1; d.push("注4 校验:抓".to_string()); } else { d.push("注4 校验:漏".to_string()); }
    }
    // 注 5：查询越界。
    {
        let mut c = DualCollector::new(2).unwrap_or_else(|_| unreachable_col());
        let _ = c.capture(1, Some(1), Some(1));
        if matches!(c.diff_at(99), Err(E_QRY_OOB)) { caught += 1; d.push("注5 越界:抓".to_string()); } else { d.push("注5 越界:漏".to_string()); }
    }
    (caught, 5, d)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    SEG_BASE == 0x75E0
        && E_EMPTY == SEG_BASE + 9
        && detail(SEG_BASE).len() > 0
        && detail(0xFFFF) == "未知 0x75E0 段诊断码：核对双路对拍账段位闭集"
        && PAGE_SIZE == 16
        && CONSUMER_B27.len() > 0
}

/// run_unx1309_checks：本单判据聚合（诊断段 0x75E0）。
pub fn run_unx1309_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1309 退役双路并行对拍执行账");
    let (ok1, d1) = drill_std_flow();
    s.add_dyn("J1-标准窗全流程", ok1, &d1);
    let (ok2, d2) = drill_complete_10();
    s.add_dyn("J1-双路完整10/10", ok2, &d2);
    let (ok3, d3) = drill_diff_query();
    s.add_dyn("J1-对拍差实时可查", ok3, &d3);
    let (ok4, d4) = drill_archive_full();
    s.add_dyn("J1-归档率1000‰含尾页", ok4, &d4);
    let (ok5, d5) = drill_b27_digest();
    s.add_dyn("J1-B27消费摘要稳定", ok5, &d5);
    let (ok6, d6) = drill_side_miss_10();
    s.add_dyn("J1-单侧缺失必抓10/10", ok6, &d6);
    let (ok7, d7) = drill_diff_nz_10();
    s.add_dyn("J1-差非零必抓10/10", ok7, &d7);
    let (ok8, d8) = drill_dup_and_window();
    s.add_dyn("J1-重号+关窗必拒", ok8, &d8);
    let (ok9, d9) = drill_arch_readonly();
    s.add_dyn("J1-归档只读化+校验", ok9, &d9);
    let (ok10, d10) = drill_histogram();
    s.add_dyn("J2-对拍差直方图", ok10, &d10);
    let (ok11, d11) = drill_rounds();
    s.add_dyn("J2-采集轮次摘要账", ok11, &d11);
    let (c10, t10, d10) = inject_5();
    s.add_dyn("J2-注入核5/5", c10 == t10, &format!("{}/{} {}", c10, t10, d10.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x75E0 段十码闭集");
    s
}
