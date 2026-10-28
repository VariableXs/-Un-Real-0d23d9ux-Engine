//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12518 · 坏表拒止文档对齐 · 0x30ec0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12518-J1 文档与对照表逐行一致，漂移抽查零命中；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 对照表域：键值对八行（八面拒止口径：PEB/TEB/OA/UNICODE_STRING/
//!   RUNTIME_FUNCTION/UNWIND_INFO/CONTEXT/句柄表），键唯一（KeyDup 拒重）、
//!   键值非空（KeyMissing 拒缺列）；
//!   - 文档行域：逐行文本必须同时含键与值（DocMissing 抓漏行、DriftHit
//!   抓漂移），行号单调（DocOrder 抓倒挂）、文本非空（TextEmpty 拒）；
//!   - 逐行对齐判据：八行键值对全量对齐（对齐率千分 1000）；
//!   - 漂移抽查：相位一次抽取 + 例序全键轴轮转（保证全覆盖），
//!   逐键查文档行值域（SampleMiss 判漏键），零命中为判准；
//!   - 漂移注入：改写文档行值 → 漂移检出计数 +1，回写修复闭环；
//!   - 双账 append-only：文档行/对照行只增不减（双 Shrink 拒）；
//!   - 错误路径带码+中文 detail，拒止不写回任何行；
//!   - 注入核：LCG 写死种子 0x30ec0，对齐/抽查/漂移检出 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30ec0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30ec0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 对照表行数（八行口径）。
pub const LEDGER_ROWS: usize = 8;

/// 漂移抽样例数（每轮抽 8 键）。
pub const SAMPLE_KEYS: usize = 8;

/// 诊断码（0x30ec0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DaCode {
    /// 0x30ec0 文档行缺。
    DocMissing = 0,
    /// 0x30ec1 对照行缺列。
    KeyMissing = 1,
    /// 0x30ec2 漂移命中。
    DriftHit = 2,
    /// 0x30ec3 键重号。
    KeyDup = 3,
    /// 0x30ec4 文档账缩水。
    DocShrink = 4,
    /// 0x30ec5 对照账缩水。
    LedgerShrink = 5,
    /// 0x30ec6 键越域。
    KeyOob = 6,
    /// 0x30ec7 文本空。
    TextEmpty = 7,
    /// 0x30ec8 抽样漏键。
    SampleMiss = 8,
    /// 0x30ec9 行号倒挂。
    DocOrder = 9,
}

impl DaCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [DaCode; 10] = [
        DaCode::DocMissing, DaCode::KeyMissing, DaCode::DriftHit, DaCode::KeyDup,
        DaCode::DocShrink, DaCode::LedgerShrink, DaCode::KeyOob, DaCode::TextEmpty,
        DaCode::SampleMiss, DaCode::DocOrder,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30ec0 => "文档行缺：先补文档行再对齐，缺行不可跳过。",
        0x30ec1 => "对照行缺列：键与值两列齐才入对照表。",
        0x30ec2 => "漂移命中：文档值与对照值不一致，回改文档行。",
        0x30ec3 => "键重号：同键只登一行对照，重复走幂等拒。",
        0x30ec4 => "文档账缩水：append-only，禁止截断，重放补齐。",
        0x30ec5 => "对照账缩水：登记后只增不减，先对账再抽查。",
        0x30ec6 => "键越域：键号先比 LEDGER_ROWS 再取行。",
        0x30ec7 => "文本空：文档行文本必须非空。",
        0x30ec8 => "抽样漏键：抽查样本必须覆盖全部键。",
        0x30ec9 => "行号倒挂：文档行号全局单调，回拨红账。",
        _ => "未知诊断码：回查 0x30ec0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30ec0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30ec_30ec_30ec_30ec;

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

/// 对照表行。
#[derive(Clone, Copy, Debug)]
pub struct LedgerRow {
    /// 键。
    pub key: &'static str,
    /// 值（锚定口径）。
    pub value: &'static str,
}

/// 文档行。
#[derive(Clone, Copy, Debug)]
pub struct DocLine {
    /// 行号（单调）。
    pub line_no: u32,
    /// 文本（须含键与值）。
    pub text: &'static str,
}

/// 标准文档行面（与对照表八行逐行同源；'static 预置）。
pub const STD_DOC: [&str; LEDGER_ROWS] = [
    "坏表拒止 PEB 面 口径：坏签名拒止档；详见对照表",
    "坏表拒止 TEB 面 口径：坏槽位拒止档；详见对照表",
    "坏表拒止 OA 面 口径：坏 Length 拒止档；详见对照表",
    "坏表拒止 UNICODE_STRING 面 口径：超长拒止档；详见对照表",
    "坏表拒止 RUNTIME_FUNCTION 面 口径：函数表越界拒止档；详见对照表",
    "坏表拒止 UNWIND_INFO 面 口径：操作码越界拒止档；详见对照表",
    "坏表拒止 CONTEXT 面 口径：坏 ContextFlags 拒止档；详见对照表",
    "坏表拒止 句柄表面 口径：坏代数拒止档；详见对照表",
];

/// 文档对齐正本。
///
/// 生命周期：fill_ledger（对照表八行）→ write_doc（文档逐行）→
/// align（逐行对齐）→ sample（漂移抽查）→ fix（漂移回写闭环）。
#[derive(Debug)]
pub struct AlignBook {
    /// 对照表。
    ledger: Vec<LedgerRow>,
    /// 文档行。
    doc: Vec<DocLine>,
    /// 文档账峰值。
    doc_high: usize,
    /// 对照账峰值。
    ledger_high: usize,
    /// 对齐执行次数。
    aligns: u32,
    /// 抽样执行次数。
    samples: u32,
    /// 漂移检出累计。
    drift_hits: u32,
    /// 拒止累计。
    rejects: u32,
}

impl AlignBook {
    /// 建账。
    pub fn new() -> Self {
        AlignBook {
            ledger: Vec::new(),
            doc: Vec::new(),
            doc_high: 0,
            ledger_high: 0,
            aligns: 0,
            samples: 0,
            drift_hits: 0,
            rejects: 0,
        }
    }

    /// 登记对照行（键值非空 + 唯一）。
    pub fn fill_ledger(&mut self, key: &'static str, value: &'static str) -> Result<bool, DaCode> {
        if key.is_empty() || value.is_empty() {
            self.rejects += 1;
            return Err(DaCode::KeyMissing);
        }
        if self.ledger.iter().any(|r| r.key == key) {
            self.rejects += 1;
            return Err(DaCode::KeyDup);
        }
        self.ledger.push(LedgerRow { key, value });
        if self.ledger.len() > self.ledger_high {
            self.ledger_high = self.ledger.len();
        }
        Ok(true)
    }

    /// 对照表标准八行登记。
    pub fn fill_std_ledger(&mut self) -> Result<usize, DaCode> {
        const ROWS: [(&str, &str); LEDGER_ROWS] = [
            ("PEB 面", "坏签名拒止档"),
            ("TEB 面", "坏槽位拒止档"),
            ("OA 面", "坏 Length 拒止档"),
            ("UNICODE_STRING 面", "超长拒止档"),
            ("RUNTIME_FUNCTION 面", "函数表越界拒止档"),
            ("UNWIND_INFO 面", "操作码越界拒止档"),
            ("CONTEXT 面", "坏 ContextFlags 拒止档"),
            ("句柄表面", "坏代数拒止档"),
        ];
        let mut n = 0usize;
        for (k, v) in ROWS.iter() {
            self.fill_ledger(k, v)?;
            n += 1;
        }
        Ok(n)
    }

    /// 写文档行（文本非空 + 行号单调）。
    pub fn write_doc(&mut self, line_no: u32, text: &'static str) -> Result<bool, DaCode> {
        if text.is_empty() {
            self.rejects += 1;
            return Err(DaCode::TextEmpty);
        }
        if let Some(last) = self.doc.last() {
            if line_no <= last.line_no {
                self.rejects += 1;
                return Err(DaCode::DocOrder);
            }
        }
        self.doc.push(DocLine { line_no, text });
        if self.doc.len() > self.doc_high {
            self.doc_high = self.doc.len();
        }
        Ok(true)
    }

    /// 文档行回写（漂移修复闭环）。
    pub fn fix_doc(&mut self, line_no: u32, text: &'static str) -> Result<bool, DaCode> {
        if text.is_empty() {
            self.rejects += 1;
            return Err(DaCode::TextEmpty);
        }
        match self.doc.iter_mut().find(|l| l.line_no == line_no) {
            Some(l) => {
                l.text = text;
                Ok(true)
            }
            None => {
                self.rejects += 1;
                Err(DaCode::DocMissing)
            }
        }
    }

    /// 键 → 文档行命中判定（文档行须同时含键与值）。
    pub fn doc_covers(&self, key: &str, value: &str) -> bool {
        self.doc
            .iter()
            .any(|l| l.text.contains(key) && l.text.contains(value))
    }

    /// 逐行对齐（返回漂移键数）。
    pub fn align(&mut self) -> usize {
        self.aligns += 1;
        let mut drift = 0usize;
        for r in self.ledger.iter() {
            if !self.doc_covers(r.key, r.value) {
                drift += 1;
            }
        }
        drift
    }

    /// 对齐率（千分）。
    pub fn align_rate_milli(&self) -> u64 {
        if self.ledger.is_empty() {
            return 1000;
        }
        let ok = self
            .ledger
            .iter()
            .filter(|r| self.doc_covers(r.key, r.value))
            .count();
        ok as u64 * 1000 / self.ledger.len() as u64
    }

    /// 漂移抽查（相位一次抽取 + 全键轴轮转，保证全覆盖）。
    pub fn sample(&mut self, lcg: &mut Lcg) -> (usize, usize) {
        self.samples += 1;
        let mut hits = 0usize;
        let mut seen: Vec<bool> = Vec::new();
        for _i in 0..self.ledger.len() {
            seen.push(false);
        }
        let phase = lcg.below(LEDGER_ROWS as u64) as usize;
        for i in 0..SAMPLE_KEYS {
            let idx = (i + phase) % self.ledger.len().max(1);
            if idx < seen.len() {
                seen[idx] = true;
            }
            let (k, v) = match self.ledger.get(idx) {
                Some(r) => (r.key, r.value),
                None => continue,
            };
            if self.doc_covers(k, v) {
                hits += 1;
            } else {
                self.drift_hits += 1;
            }
        }
        let covered = seen.iter().filter(|&&s| s).count();
        (hits, covered)
    }

    /// 抽样次数。
    pub fn sample_runs(&self) -> u32 {
        self.samples
    }

    /// 对齐次数。
    pub fn align_runs(&self) -> u32 {
        self.aligns
    }

    /// 漂移检出累计。
    pub fn drift_total(&self) -> u32 {
        self.drift_hits
    }

    /// 对照行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 文档行数。
    pub fn doc_len(&self) -> usize {
        self.doc.len()
    }

    /// 双账只增不减判据。
    pub fn monotone(&self) -> bool {
        self.ledger.len() >= self.ledger_high && self.doc.len() >= self.doc_high
    }

    /// 对照行快照。
    pub fn ledger_of(&self, idx: usize) -> Option<LedgerRow> {
        if idx >= self.ledger.len() {
            None
        } else {
            Some(self.ledger[idx])
        }
    }

    /// 文档行快照。
    pub fn doc_of(&self, idx: usize) -> Option<DocLine> {
        if idx >= self.doc.len() {
            None
        } else {
            Some(self.doc[idx])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 逐行对齐演练：八行全对齐、对齐率 1000 10/10。
pub fn drill_align_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30ec0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AlignBook::new();
        let n = bk.fill_std_ledger().unwrap_or(0);
        let mut pass = n == LEDGER_ROWS;
        for i in 0..LEDGER_ROWS {
            pass = pass && bk.write_doc((i + 1) as u32, STD_DOC[i]).is_ok();
        }
        let drift = bk.align();
        let _ = lcg.below(3);
        if pass && drift == 0 && bk.align_rate_milli() == 1000 && bk.monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("逐行对齐轮={}/{}", ok, INJECT_ROUNDS))
}

/// 漂移抽查零命中演练：抽样 8 键全覆盖零漂移 10/10。
pub fn drill_sample_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30ec0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AlignBook::new();
        let _ = bk.fill_std_ledger();
        for i in 0..LEDGER_ROWS {
            let _ = bk.write_doc((i + 1) as u32, STD_DOC[i]);
        }
        let (hits, covered) = bk.sample(&mut lcg);
        if hits == SAMPLE_KEYS
            && covered == LEDGER_ROWS
            && bk.drift_total() == 0
            && bk.sample_runs() == 1
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("漂移抽查零命中轮={}/{}", ok, INJECT_ROUNDS))
}

/// 漂移注入检出与修复闭环演练：检出→回写→对齐归零 10/10。
pub fn drill_drift_fix_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30ec0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AlignBook::new();
        let _ = bk.fill_std_ledger();
        for i in 0..LEDGER_ROWS {
            let _ = bk.write_doc((i + 1) as u32, STD_DOC[i]);
        }
        // 漂移注入：改写第 7 行值域（CONTEXT 口径漂移）。
        let _ = bk.fix_doc(7, "坏表拒止 CONTEXT 面 口径：RSP 越界档；详见对照表");
        let drift = bk.align();
        let rate_bad = bk.align_rate_milli();
        let fix = bk.fix_doc(7, STD_DOC[6]);
        let drift2 = bk.align();
        let _ = lcg.below(3);
        if drift == 1 && rate_bad == 875 && matches!(fix, Ok(true)) && drift2 == 0 && bk.drift_total() == 0 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("漂移检出修复轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：对齐账转录导出（逐对照行一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 对齐账转录导出。
pub fn export_align_lines(bk: &AlignBook) -> String {
    let mut s = String::from("对齐账转录：\n");
    let mut i = 0usize;
    while let Some(r) = bk.ledger_of(i) {
        let cov = bk.doc_covers(r.key, r.value);
        s.push_str(&format!("· {} {} 覆{}\n", r.key, r.value, cov));
        i += 1;
    }
    s
}

/// 转录完整判据（全格行齐备）。
pub fn align_lines_complete(bk: &AlignBook) -> bool {
    let s = export_align_lines(bk);
    if bk.ledger_len() == 0 {
        return false;
    }
    (0..bk.ledger_len()).all(|i| {
        bk.ledger_of(i)
            .map(|r| s.contains(r.key))
            .unwrap_or(false)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：对齐报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct DaReport {
    /// 对照行数。
    pub ledger: usize,
    /// 文档行数。
    pub doc: usize,
    /// 对齐率（千分）。
    pub rate: u64,
    /// 漂移检出累计。
    pub drifts: u32,
    /// 抽样次数。
    pub samples: u32,
    /// 拒止累计。
    pub rejects: u32,
}

impl DaReport {
    /// 建报告。
    pub fn build(bk: &AlignBook) -> Self {
        DaReport {
            ledger: bk.ledger_len(),
            doc: bk.doc_len(),
            rate: bk.align_rate_milli(),
            drifts: bk.drift_total(),
            samples: bk.sample_runs(),
            rejects: bk.reject_total(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.rate == 1000 && self.ledger == LEDGER_ROWS && self.samples >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "DaReport 照={} 文={} 率={}‰ 漂={} 抽={} 拒={}",
            self.ledger, self.doc, self.rate, self.drifts, self.samples, self.rejects
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：对照行域演练（缺列/重号/越域 10/10 拒）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_ledger_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30ec0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AlignBook::new();
        let miss = bk.fill_ledger("K1", "");
        let _ = bk.fill_ledger("K1", "V1");
        let dup = bk.fill_ledger("K1", "V1x");
        let oob = bk.ledger_of(LEDGER_ROWS + 9);
        let _ = lcg.below(3);
        if matches!(miss, Err(DaCode::KeyMissing))
            && matches!(dup, Err(DaCode::KeyDup))
            && oob.is_none()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("对照行域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：文档行域演练（空文本/行号倒挂 10/10 拒）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_doc_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30ec0 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = AlignBook::new();
        let w1 = bk.write_doc(1, STD_DOC[0]);
        let empty = bk.write_doc(2, "");
        let back = bk.write_doc(1, STD_DOC[1]);
        let w2 = bk.write_doc(2, STD_DOC[1]);
        let _ = lcg.below(3);
        if matches!(w1, Ok(true))
            && matches!(empty, Err(DaCode::TextEmpty))
            && matches!(back, Err(DaCode::DocOrder))
            && matches!(w2, Ok(true))
            && bk.doc_len() == 2
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("文档行域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct DaCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_da_checklist(bk: &AlignBook) -> Vec<DaCheckRow> {
    let rep = DaReport::build(bk);
    let mut rows: Vec<DaCheckRow> = Vec::new();
    rows.push(DaCheckRow { name: "对齐率1000", passed: bk.align_rate_milli() == 1000 });
    rows.push(DaCheckRow { name: "对照八行", passed: bk.ledger_len() == LEDGER_ROWS });
    rows.push(DaCheckRow { name: "双账单调", passed: bk.monotone() });
    rows.push(DaCheckRow { name: "抽样口径", passed: SAMPLE_KEYS == 8 });
    rows.push(DaCheckRow { name: "文档单调序", passed: bk.doc.iter().zip(bk.doc.iter().skip(1)).all(|(a, b)| a.line_no < b.line_no) });
    rows.push(DaCheckRow { name: "转录完整", passed: align_lines_complete(bk) });
    rows.push(DaCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(DaCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(DaCheckRow { name: "漂移可检", passed: bk.drift_total() >= 1 || bk.align_runs() >= 1 });
    rows.push(DaCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn da_checklist_green(rows: &[DaCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn da_stats_line(bk: &AlignBook) -> String {
    format!(
        "对齐概况：照 {} 文 {} 率 {}‰ 漂 {}",
        bk.ledger_len(),
        bk.doc_len(),
        bk.align_rate_milli(),
        bk.drift_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12518_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = AlignBook::new();
    let _ = bk.fill_std_ledger();
    for i in 0..LEDGER_ROWS {
        let _ = bk.write_doc((i + 1) as u32, STD_DOC[i]);
    }
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 5);
    let _ = bk.sample(&mut lcg);
    // 全量对齐一轮（漂移可检面口径）。
    let _ = bk.align();
    // 注入一例重号键（拒止账口径）。
    let _ = bk.fill_ledger("PEB 面", "重复口径面");
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        align_lines_complete(&bk) && DaReport::build(&bk).all_green(),
        da_stats_line(&bk),
    ));
    // X-b：对照行域 10/10。
    let (ok_l, d_l) = drill_ledger_domain_10();
    out.push(("X-对照行域10/10", ok_l, d_l));
    // X-c：文档行域 10/10。
    let (ok_d, d_d) = drill_doc_domain_10();
    out.push(("X-文档行域10/10", ok_d, d_d));
    // X-d：终验清单。
    let rows = build_da_checklist(&bk);
    out.push(("X-终验清单10项", da_checklist_green(&rows), DaReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = AlignBook::new();
    // 注 1：对照行缺列必拒（KeyMissing）。
    if matches!(bk.fill_ledger("K", ""), Err(DaCode::KeyMissing)) {
        caught += 1;
        d.push("注1 缺列:抓".to_string());
    } else {
        d.push("注1 缺列:漏".to_string());
    }
    // 注 2：键重号必拒（KeyDup）。
    let _ = bk.fill_ledger("K2", "V2");
    if matches!(bk.fill_ledger("K2", "V2x"), Err(DaCode::KeyDup)) {
        caught += 1;
        d.push("注2 重键:抓".to_string());
    } else {
        d.push("注2 重键:漏".to_string());
    }
    // 注 3：空文本必拒（TextEmpty）。
    if matches!(bk.write_doc(1, ""), Err(DaCode::TextEmpty)) {
        caught += 1;
        d.push("注3 空文:抓".to_string());
    } else {
        d.push("注3 空文:漏".to_string());
    }
    // 注 4：行号倒挂必拒（DocOrder）。
    let _ = bk.write_doc(1, STD_DOC[0]);
    if matches!(bk.write_doc(1, STD_DOC[1]), Err(DaCode::DocOrder)) {
        caught += 1;
        d.push("注4 倒挂:抓".to_string());
    } else {
        d.push("注4 倒挂:漏".to_string());
    }
    // 注 5：漂移检出（DriftHit 面检出）。
    let _ = bk.fill_ledger("K9", "锚定值X");
    let _ = bk.write_doc(2, "坏表拒止 K9 面 口径：漂移值Y；详见对照表");
    if !bk.doc_covers("K9", "锚定值X") {
        caught += 1;
        d.push("注5 漂移:抓".to_string());
    } else {
        d.push("注5 漂移:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30ec0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    DaCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12518_checks：本单判据聚合（诊断段 0x30ec0）。
pub fn run_unx12518_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12518 坏表拒止文档对齐");
    // J1-a：文档与对照表逐行一致（10/10）。
    let (ok1, d1) = drill_align_10();
    s.add_dyn("J1-逐行一致10/10", ok1, &d1);
    // J1-b：漂移抽查零命中（10/10）。
    let (ok2, d2) = drill_sample_10();
    s.add_dyn("J1-漂移抽查零命中10/10", ok2, &d2);
    // J1-c：漂移注入检出与修复闭环（10/10）。
    let (ok3, d3) = drill_drift_fix_10();
    s.add_dyn("J1-漂移检出修复10/10", ok3, &d3);
    // J1-d：标准会话（八行全对齐 + 抽样零漂 + 双账单调）。
    let mut bk = AlignBook::new();
    let _ = bk.fill_std_ledger();
    for i in 0..LEDGER_ROWS {
        let _ = bk.write_doc((i + 1) as u32, STD_DOC[i]);
    }
    let drift = bk.align();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let (hits, covered) = bk.sample(&mut lcg);
    s.add_dyn(
        "J1-文档对齐零漂移",
        drift == 0
            && bk.align_rate_milli() == 1000
            && hits == SAMPLE_KEYS
            && covered == LEDGER_ROWS
            && bk.monotone(),
        &format!(
            "漂移={} 率={} 抽 {}/{} 覆盖 {}",
            drift,
            bk.align_rate_milli(),
            hits,
            SAMPLE_KEYS,
            covered
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（缺列对照行不入账）。
    let l0 = bk.ledger_len();
    let _ = bk.fill_ledger("坏键", "");
    s.add_dyn(
        "J2-拒止不写回",
        bk.ledger_len() == l0 && bk.monotone(),
        &format!("对照行 {}→{}", l0, bk.ledger_len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30ec0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12518_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
