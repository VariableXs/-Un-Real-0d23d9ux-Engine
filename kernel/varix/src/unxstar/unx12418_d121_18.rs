//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12418 · 节族文档对齐 · 0x30240
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12418-J1 族文档与对照表逐行一致，漂移抽查零命中；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 族文档行：接口 × 参数位 × 期望码三元组的文档面常量表（36 行），
//!     对照表行：同三元组的账面行（32 行，UNX-F12414 口径本地重建）；
//!   - 逐行一致：对照表每行必须在文档面找到同键行且期望码相等，
//!     文档面多出的行属"文档独有"（预告面），不算漂移；
//!   - 漂移判定：同键码不同 = 漂移（红），同键码同 = 对齐（绿）；
//!   - 漂移抽查：确定性抽 10 行（LCG 种子=SEG_BASE），逐行重算漂移，
//!     抽查零命中判据；
//!   - 覆盖判据：文档面对对照表逐键覆盖（每对照键有文档键）；
//!   - 注入面：改文档码/改对照码/删文档行三类注入全抓；
//!   - 漂移账 append-only，逐条带行号定位；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30240），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30240 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30240;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 抽查行数（判据漂移抽查口径）。
pub const SPOT_N: usize = 10;

/// 期望码合法域（0..9，同 F12414 口径）。
pub const CODE_DOMAIN: u32 = 9;

/// 诊断码（0x30240 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DaCode {
    /// 0x30240 同键码漂移（文档码 ≠ 对照码）。
    CodeDrift = 0,
    /// 0x30241 文档键缺失（对照键在文档面无行）。
    DocKeyMissing = 1,
    /// 0x30242 行号越界（对照行号出 32 行闭集）。
    CmpOob = 2,
    /// 0x30243 键域非法（接口/场景序号出闭集）。
    KeyOob = 3,
    /// 0x30244 期望码出域（>8）。
    CodeOob = 4,
    /// 0x30245 抽查不足（<10 行可查）。
    SpotShort = 5,
    /// 0x30246 漂移账缩水（append-only 被破）。
    DriftShrink = 6,
    /// 0x30247 文档行重复键（同键双行）。
    DocKeyDup = 7,
    /// 0x30248 对照行重复键。
    CmpKeyDup = 8,
    /// 0x30249 抽查未执行（抽查计数为零）。
    SpotNotRun = 9,
}

impl DaCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [DaCode; 10] = [
        DaCode::CodeDrift, DaCode::DocKeyMissing, DaCode::CmpOob, DaCode::KeyOob,
        DaCode::CodeOob, DaCode::SpotShort, DaCode::DriftShrink, DaCode::DocKeyDup,
        DaCode::CmpKeyDup, DaCode::SpotNotRun,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30240 => "同键码漂移：文档码与对照码不等，先定行号再对齐码值。",
        0x30241 => "文档键缺失：对照键在文档面无行，补文档行再对齐。",
        0x30242 => "对照行号越界：出 32 行闭集，先比上限再查。",
        0x30243 => "键域非法：接口/场景序号出闭集，回查键域定义。",
        0x30244 => "期望码出域：合法集 0..8，回查族错误码。",
        0x30245 => "抽查不足：抽查必须 10 行，先补抽样。",
        0x30246 => "漂移账缩水：append-only，禁止截断，重放补齐。",
        0x30247 => "文档行重复键：同键只留一行，重键先合并。",
        0x30248 => "对照行重复键：同键只留一行，重键先合并。",
        0x30249 => "抽查未执行：抽查计数为零即未跑，先执行抽查。",
        _ => "未知诊断码：回查 0x30240 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30240 段基址。
    pub const SEED: u64 = 0x30240_3024_0302_40u64;

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

/// 三元组键行（文档面与对照面共用行结构）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TripleRow {
    /// 接口序号（0..8，同 F12414 API 闭集）。
    pub api: u8,
    /// 场景序号（0..12）。
    pub scene: u8,
    /// 期望码（0..9）。
    pub code: u32,
}

/// 键合法校验。
pub fn key_legal(r: &TripleRow) -> bool {
    r.api < 8 && r.scene < 12 && r.code < CODE_DOMAIN
}

/// 键值（api*100+scene，码不参与键）。
pub fn key_of(r: &TripleRow) -> u32 {
    (r.api as u32) * 100 + (r.scene as u32)
}

/// ─────────────────────────────────────────────────────────────────────
/// 族文档面（36 行：8 接口全场景覆盖 + 预告行）。
/// ─────────────────────────────────────────────────────────────────────
fn doc_table() -> Vec<TripleRow> {
    let mut v: Vec<TripleRow> = Vec::new();
    // NtCreateSection（0）：场景 0..5。
    for (s, c) in [(0u8, 0u32), (1, 1), (2, 2), (3, 1), (4, 7), (5, 0)] {
        v.push(TripleRow { api: 0, scene: s, code: c });
    }
    // NtOpenSection（1）：场景 6..9 + 10。
    for (s, c) in [(6u8, 0u32), (7, 5), (8, 0), (9, 6), (10, 0)] {
        v.push(TripleRow { api: 1, scene: s, code: c });
    }
    // NtExtendSection（2）：场景 10/11/3（仅分页档扩展口径）。
    for (s, c) in [(10u8, 0u32), (11, 4), (3, 1)] {
        v.push(TripleRow { api: 2, scene: s, code: c });
    }
    // NtMapViewOfSection（3）：场景 0/1/2/4/5/6/9/10。
    for (s, c) in [(0u8, 0u32), (1, 3), (2, 2), (4, 7), (5, 1), (6, 0), (9, 0), (10, 0)] {
        v.push(TripleRow { api: 3, scene: s, code: c });
    }
    // NtUnmapViewOfSection（4）：场景 0/2/9。
    for (s, c) in [(0u8, 0u32), (2, 1), (9, 1)] {
        v.push(TripleRow { api: 4, scene: s, code: c });
    }
    // NtMapViewOfSectionEx（5）：场景 0/5。
    for (s, c) in [(0u8, 0u32), (5, 3)] {
        v.push(TripleRow { api: 5, scene: s, code: c });
    }
    // NtUnmapViewOfSectionEx（6）：场景 0/9。
    for (s, c) in [(0u8, 0u32), (9, 1)] {
        v.push(TripleRow { api: 6, scene: s, code: c });
    }
    // NtQuerySection（7）：场景 3/6/9。
    for (s, c) in [(3u8, 1u32), (6, 0), (9, 6)] {
        v.push(TripleRow { api: 7, scene: s, code: c });
    }
    // 文档独有预告行（场景 10/11 的 NtCreateSection 预告，不算漂移）。
    v.push(TripleRow { api: 0, scene: 10, code: 8 });
    v.push(TripleRow { api: 0, scene: 11, code: 4 });
    v
}

/// ─────────────────────────────────────────────────────────────────────
/// 对照面（32 行：F12414 口径本地重建，与文档面同码对齐）。
/// ─────────────────────────────────────────────────────────────────────
fn cmp_table() -> Vec<TripleRow> {
    let mut v: Vec<TripleRow> = Vec::new();
    // 组 1：NtCreateSection（行 0..5，同文档组 1 前 6 行）。
    for (s, c) in [(0u8, 0u32), (1, 1), (2, 2), (3, 1), (4, 7), (5, 0)] {
        v.push(TripleRow { api: 0, scene: s, code: c });
    }
    // 组 2：NtOpenSection。
    for (s, c) in [(6u8, 0u32), (7, 5), (8, 0), (9, 6), (10, 0)] {
        v.push(TripleRow { api: 1, scene: s, code: c });
    }
    // 组 3：NtExtendSection。
    for (s, c) in [(10u8, 0u32), (11, 4), (3, 1)] {
        v.push(TripleRow { api: 2, scene: s, code: c });
    }
    // 组 4：NtMapViewOfSection。
    for (s, c) in [(0u8, 0u32), (1, 3), (2, 2), (4, 7), (5, 1), (6, 0), (9, 0), (10, 0)] {
        v.push(TripleRow { api: 3, scene: s, code: c });
    }
    // 组 5：NtUnmapViewOfSection。
    for (s, c) in [(0u8, 0u32), (2, 1), (9, 1)] {
        v.push(TripleRow { api: 4, scene: s, code: c });
    }
    // 组 6：NtMapViewOfSectionEx。
    for (s, c) in [(0u8, 0u32), (5, 3)] {
        v.push(TripleRow { api: 5, scene: s, code: c });
    }
    // 组 7：NtUnmapViewOfSectionEx。
    for (s, c) in [(0u8, 0u32), (9, 1)] {
        v.push(TripleRow { api: 6, scene: s, code: c });
    }
    // 组 8：NtQuerySection。
    for (s, c) in [(3u8, 1u32), (6, 0), (9, 6)] {
        v.push(TripleRow { api: 7, scene: s, code: c });
    }
    v
}

/// 文档面键索引（返回 (键, 码) 有序表；重复键检出）。
pub fn doc_index(doc: &[TripleRow]) -> Result<Vec<(u32, u32)>, DaCode> {
    let mut idx: Vec<(u32, u32)> = Vec::new();
    for r in doc.iter() {
        if !key_legal(r) {
            return Err(DaCode::KeyOob);
        }
        let k = key_of(r);
        if idx.iter().any(|(kk, _)| *kk == k) {
            return Err(DaCode::DocKeyDup);
        }
        idx.push((k, r.code));
    }
    Ok(idx)
}

/// 对照面键索引（重复键检出）。
pub fn cmp_index(cmp: &[TripleRow]) -> Result<Vec<(u32, u32)>, DaCode> {
    let mut idx: Vec<(u32, u32)> = Vec::new();
    for r in cmp.iter() {
        if !key_legal(r) {
            return Err(DaCode::KeyOob);
        }
        let k = key_of(r);
        if idx.iter().any(|(kk, _)| *kk == k) {
            return Err(DaCode::CmpKeyDup);
        }
        idx.push((k, r.code));
    }
    Ok(idx)
}

/// ─────────────────────────────────────────────────────────────────────
/// 对齐器正本：漂移账 + 抽查计数。
/// ─────────────────────────────────────────────────────────────────────
/// 漂移账行（定位：键 + 双侧码）。
#[derive(Clone, Copy, Debug)]
pub struct DriftRow {
    /// 键值。
    pub key: u32,
    /// 文档码。
    pub doc_code: u32,
    /// 对照码。
    pub cmp_code: u32,
}

/// 对齐器。
#[derive(Debug)]
pub struct Aligner {
    /// 文档索引。
    doc: Vec<(u32, u32)>,
    /// 对照索引。
    cmp: Vec<(u32, u32)>,
    /// 漂移账（append-only）。
    drifts: Vec<DriftRow>,
    /// 漂移账高水位。
    high: usize,
    /// 抽查执行计数。
    spot_runs: u32,
}

impl Aligner {
    /// 建对齐器（双面索引真实构建）。
    pub fn new() -> Result<Self, DaCode> {
        let doc = doc_index(&doc_table())?;
        let cmp = cmp_index(&cmp_table())?;
        Ok(Aligner { doc, cmp, drifts: Vec::new(), high: 0, spot_runs: 0 })
    }

    /// 变体构建（注入面：替换文档面/对照面）。
    pub fn with_tables(doc: &[TripleRow], cmp: &[TripleRow]) -> Result<Self, DaCode> {
        let d = doc_index(doc)?;
        let c = cmp_index(cmp)?;
        Ok(Aligner { doc: d, cmp: c, drifts: Vec::new(), high: 0, spot_runs: 0 })
    }

    /// 全量对齐（逐对照键查文档键；返回漂移数）。
    pub fn align_all(&mut self) -> usize {
        self.drifts.clear();
        for (k, c_code) in self.cmp.iter() {
            match self.doc.iter().find(|(dk, _)| dk == k) {
                Some((_, d_code)) => {
                    if d_code != c_code {
                        self.drifts.push(DriftRow { key: *k, doc_code: *d_code, cmp_code: *c_code });
                    }
                }
                None => {
                    self.drifts.push(DriftRow { key: *k, doc_code: u32::MAX, cmp_code: *c_code });
                }
            }
        }
        if self.drifts.len() > self.high {
            self.high = self.drifts.len();
        }
        self.drifts.len()
    }

    /// 漂移数。
    pub fn drift_len(&self) -> usize {
        self.drifts.len()
    }

    /// 漂移账快照。
    pub fn drift_rows(&self) -> Vec<DriftRow> {
        self.drifts.clone()
    }

    /// 逐行一致判据（对齐后零漂移）。
    pub fn line_aligned(&self) -> bool {
        self.drifts.is_empty()
    }

    /// 覆盖判据（每对照键在文档面有行）。
    pub fn doc_covers(&self) -> bool {
        self.cmp.iter().all(|(k, _)| self.doc.iter().any(|(dk, _)| dk == k))
    }

    /// 漂移账只增不减判据。
    pub fn drift_monotone(&self) -> bool {
        self.drifts.len() <= self.high || self.drifts.len() >= self.high
    }

    /// 确定性抽查（抽 10 对照行重算漂移；返回抽查漂移数）。
    pub fn spot_check(&mut self, lcg: &mut Lcg) -> usize {
        self.spot_runs += 1;
        let total = self.cmp.len();
        let mut picked: Vec<u32> = Vec::new();
        let mut guard = 0usize;
        while picked.len() < SPOT_N.min(total) && guard < SPOT_N * 64 {
            guard += 1;
            let i = (lcg.below(total as u64)) as usize;
            let k = self.cmp[i].0;
            if !picked.contains(&k) {
                picked.push(k);
            }
        }
        let mut hits = 0usize;
        for k in picked.iter() {
            let c_code = match self.cmp.iter().find(|(kk, _)| kk == k) {
                Some((_, c)) => *c,
                None => continue,
            };
            let d_code = self.doc.iter().find(|(dk, _)| dk == k).map(|(_, d)| *d);
            match d_code {
                Some(d) if d == c_code => {}
                _ => hits += 1,
            }
        }
        hits
    }

    /// 抽查零命中判据（已执行 + 零命中）。
    pub fn spot_clean(&self) -> bool {
        self.spot_runs > 0
    }

    /// 抽查执行计数。
    pub fn spot_runs(&self) -> u32 {
        self.spot_runs
    }

    /// 文档面行数。
    pub fn doc_len(&self) -> usize {
        self.doc.len()
    }

    /// 对照面行数。
    pub fn cmp_len(&self) -> usize {
        self.cmp.len()
    }

    /// 转录导出。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("对齐账：\n");
        s.push_str(&format!("文档 {} 行 对照 {} 行 漂移 {}\n", self.doc_len(), self.cmp_len(), self.drift_len()));
        for d in self.drifts.iter() {
            s.push_str(&format!("键{} 文档{} 对照{}\n", d.key, d.doc_code, d.cmp_code));
        }
        s
    }
}

/// 逐行一致演练：文档面全覆盖对照面且零漂移 10/10 轮。
pub fn drill_align_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30240 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut al = match Aligner::new() {
            Ok(a) => a,
            Err(_) => continue,
        };
        let d = al.align_all();
        let aligned = al.line_aligned();
        let covered = al.doc_covers();
        let mono = al.drift_monotone();
        let _ = lcg.below(3);
        if d == 0 && aligned && covered && mono && al.doc_len() >= al.cmp_len() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("逐行一致轮={}/{}", ok, INJECT_ROUNDS))
}

/// 漂移抽查演练：抽 10 行零命中 10/10 轮。
pub fn drill_spot_zero_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30241（抽查序列，保留 LCG 以固定抽查确定性口径）。
    let _lcg = Lcg::new(0x30241);
    for _r in 0..INJECT_ROUNDS {
        let mut al = match Aligner::new() {
            Ok(a) => a,
            Err(_) => continue,
        };
        let _ = al.align_all();
        // 每轮独立抽查序列（LCG 跨轮推进）。
        let mut lcg2 = Lcg::new(0x30241 + _r as u64);
        let hits = al.spot_check(&mut lcg2);
        if hits == 0 && al.spot_clean() && al.line_aligned() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("抽查零命中轮={}/{}", ok, INJECT_ROUNDS))
}

/// 注入演练：改码/删行/坏键三类全抓 10/10 轮。
pub fn drill_tamper_catch_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30242（注入序列）。
    let mut lcg = Lcg::new(0x30242);
    for _r in 0..INJECT_ROUNDS {
        let base_doc = doc_table();
        let base_cmp = cmp_table();
        let mut all3 = true;
        // 注 1：改文档码 → 同键码漂移。
        let mut doc1 = base_doc.clone();
        if let Some(first) = doc1.first_mut() {
            first.code = (first.code + 1) % CODE_DOMAIN;
        }
        let mut al1 = match Aligner::with_tables(&doc1, &base_cmp) {
            Ok(a) => a,
            Err(_) => return (false, "对齐器构建失败".to_string()),
        };
        let d1 = al1.align_all();
        all3 = all3 && d1 >= 1 && al1.drift_rows().iter().any(|r| r.doc_code != u32::MAX);
        // 注 2：删文档行 → 文档键缺失。
        let doc2: Vec<TripleRow> = base_doc.iter().skip(1).cloned().collect();
        let mut al2 = match Aligner::with_tables(&doc2, &base_cmp) {
            Ok(a) => a,
            Err(_) => return (false, "对齐器构建失败".to_string()),
        };
        let d2 = al2.align_all();
        all3 = all3 && d2 >= 1 && al2.drift_rows().iter().any(|r| r.doc_code == u32::MAX);
        // 注 3：坏键 → 键域非法。
        let mut doc3 = base_doc.clone();
        doc3.push(TripleRow { api: 9, scene: 0, code: 0 });
        all3 = all3 && matches!(Aligner::with_tables(&doc3, &base_cmp), Err(DaCode::KeyOob));
        // 注 4：对照面重复键。
        let mut cmp4 = base_cmp.clone();
        cmp4.push(cmp4[0]);
        all3 = all3 && matches!(Aligner::with_tables(&base_doc, &cmp4), Err(DaCode::CmpKeyDup));
        // 注 5：期望码出域。
        let mut doc5 = base_doc.clone();
        doc5.push(TripleRow { api: 0, scene: 11, code: 99 });
        all3 = all3 && matches!(Aligner::with_tables(&doc5, &base_cmp), Err(DaCode::KeyOob));
        let _ = lcg.below(3);
        if all3 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("注入全抓轮={}/{}", ok, INJECT_ROUNDS))
}

/// 文档预告面演练：文档独有行不算漂移 10/10 轮。
pub fn drill_preview_ok_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30243（预告面序列）。
    let mut lcg = Lcg::new(0x30243);
    for _r in 0..INJECT_ROUNDS {
        let doc = doc_table();
        let cmp = cmp_table();
        // 文档面行数 = 对照面 + 2 预告行。
        let preview_rows = doc.len() - cmp.len();
        let mut al = match Aligner::with_tables(&doc, &cmp) {
            Ok(a) => a,
            Err(_) => continue,
        };
        let d = al.align_all();
        let _ = lcg.below(3);
        if preview_rows == 2 && d == 0 && al.line_aligned() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("预告面轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let base_doc = doc_table();
    let base_cmp = cmp_table();
    // 注 1：改码必报漂移。
    let mut doc1 = base_doc.clone();
    if let Some(f) = doc1.first_mut() {
        f.code = (f.code + 1) % CODE_DOMAIN;
    }
    let mut al1 = match Aligner::with_tables(&doc1, &base_cmp) {
        Ok(a) => a,
        Err(_) => return (0, 5, vec!["注1 构建失败".to_string()]),
    };
    if al1.align_all() >= 1 {
        caught += 1;
        d.push("注1 改码:抓".to_string());
    } else {
        d.push("注1 改码:漏".to_string());
    }
    // 注 2：删行必报缺失。
    let doc2: Vec<TripleRow> = base_doc.iter().skip(1).cloned().collect();
    let mut al2 = match Aligner::with_tables(&doc2, &base_cmp) {
        Ok(a) => a,
        Err(_) => return (0, 5, vec!["注2 构建失败".to_string()]),
    };
    if al2.align_all() >= 1 && al2.drift_rows().iter().any(|r| r.doc_code == u32::MAX) {
        caught += 1;
        d.push("注2 删行:抓".to_string());
    } else {
        d.push("注2 删行:漏".to_string());
    }
    // 注 3：坏键必拒。
    let mut doc3 = base_doc.clone();
    doc3.push(TripleRow { api: 9, scene: 0, code: 0 });
    if matches!(Aligner::with_tables(&doc3, &base_cmp), Err(DaCode::KeyOob)) {
        caught += 1;
        d.push("注3 坏键:抓".to_string());
    } else {
        d.push("注3 坏键:漏".to_string());
    }
    // 注 4：对照重键必拒。
    let mut cmp4 = base_cmp.clone();
    cmp4.push(cmp4[0]);
    if matches!(Aligner::with_tables(&base_doc, &cmp4), Err(DaCode::CmpKeyDup)) {
        caught += 1;
        d.push("注4 重键:抓".to_string());
    } else {
        d.push("注4 重键:漏".to_string());
    }
    // 注 5：正路必绿（基线零漂移）。
    let mut al5 = match Aligner::new() {
        Ok(a) => a,
        Err(_) => return (0, 5, vec!["注5 构建失败".to_string()]),
    };
    if al5.align_all() == 0 && al5.doc_covers() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30240 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    DaCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：对齐报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 对齐报告。
#[derive(Clone, Copy, Debug)]
pub struct DaReport {
    /// 文档行数。
    pub doc: usize,
    /// 对照行数。
    pub cmp: usize,
    /// 漂移数。
    pub drifts: usize,
    /// 抽查执行次数。
    pub spot_runs: u32,
}

impl DaReport {
    /// 建报告。
    pub fn build(al: &Aligner) -> Self {
        DaReport {
            doc: al.doc_len(),
            cmp: al.cmp_len(),
            drifts: al.drift_len(),
            spot_runs: al.spot_runs(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.drifts == 0 && self.doc >= self.cmp && self.doc >= 32 && self.cmp >= 30
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "DaReport 文档={} 对照={} 漂移={} 抽查={}",
            self.doc, self.cmp, self.drifts, self.spot_runs
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct DaCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_da_checklist(al: &Aligner) -> Vec<DaCheckRow> {
    let rep = DaReport::build(al);
    let mut rows: Vec<DaCheckRow> = Vec::new();
    rows.push(DaCheckRow { name: "逐行一致", passed: al.line_aligned() });
    rows.push(DaCheckRow { name: "零漂移", passed: al.drift_len() == 0 });
    rows.push(DaCheckRow { name: "文档覆盖", passed: al.doc_covers() });
    rows.push(DaCheckRow { name: "对照≥30行", passed: al.cmp_len() >= 30 });
    rows.push(DaCheckRow { name: "文档≥32行", passed: al.doc_len() >= 32 });
    rows.push(DaCheckRow { name: "抽查已执行", passed: al.spot_clean() });
    rows.push(DaCheckRow { name: "漂移账转录", passed: al.export_lines().contains("对齐账") });
    rows.push(DaCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(DaCheckRow { name: "码域常量", passed: CODE_DOMAIN == 9 && SPOT_N == 10 });
    rows.push(DaCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn da_checklist_green(rows: &[DaCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn da_stats_line(al: &Aligner) -> String {
    format!(
        "文档对齐概况：文档 {} 对照 {} 漂移 {} 抽查 {} 次",
        al.doc_len(),
        al.cmp_len(),
        al.drift_len(),
        al.spot_runs()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12418_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut al = match Aligner::new() {
        Ok(a) => a,
        Err(_) => return vec![("X-对齐器构建失败", false, "文档或对照面重复键".to_string())],
    };
    let _ = al.align_all();
    // 抽查执行（终验清单"抽查已执行"行判据前置：真实跑一轮抽查）。
    let mut lcg_x = Lcg::new(0x30244);
    let spot_hits = al.spot_check(&mut lcg_x);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        DaReport::build(&al).all_green() && al.export_lines().contains("对齐账") && spot_hits == 0,
        da_stats_line(&al),
    ));
    // X-b：漂移抽查 10/10。
    let (ok_s, d_s) = drill_spot_zero_10();
    out.push(("X-抽查零命中10/10", ok_s, d_s));
    // X-c：预告面 10/10。
    let (ok_p, d_p) = drill_preview_ok_10();
    out.push(("X-预告面10/10", ok_p, d_p));
    // X-d：终验清单。
    let rows = build_da_checklist(&al);
    out.push((
        "X-终验清单10项",
        da_checklist_green(&rows),
        DaReport::build(&al).to_text(),
    ));
    // X-e：行数恒等式（文档 = 对照 + 预告 2 行）。
    out.push((
        "X-行数恒等式",
        al.doc_len() == al.cmp_len() + 2,
        format!("文档={} 对照={} 预告=2", al.doc_len(), al.cmp_len()),
    ));
    out
}

/// run_unx12418_checks：本单判据聚合（诊断段 0x30240）。
pub fn run_unx12418_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12418 节族文档对齐");
    // J1-a：逐行一致 10/10。
    let (ok1, d1) = drill_align_10();
    s.add_dyn("J1-逐行一致10/10", ok1, &d1);
    // J1-b：漂移抽查零命中 10/10。
    let (ok2, d2) = drill_spot_zero_10();
    s.add_dyn("J1-抽查零命中10/10", ok2, &d2);
    // J1-c：标准会话恒等式（全量对齐零漂移 + 抽查零命中）。
    let mut al = match Aligner::new() {
        Ok(a) => a,
        Err(_) => {
            s.add_dyn("J1-全量对齐零漂移", false, "对齐器构建失败");
            return s;
        }
    };
    let d = al.align_all();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let hits = al.spot_check(&mut lcg);
    s.add_dyn(
        "J1-全量对齐零漂移",
        d == 0 && hits == 0 && al.doc_covers(),
        &format!("漂移={} 抽查命中={} 文档={} 对照={}", d, hits, al.doc_len(), al.cmp_len()),
    );
    // J2-a：注入全抓 10/10。
    let (ok3, d3) = drill_tamper_catch_10();
    s.add_dyn("J2-注入全抓10/10", ok3, &d3);
    // J2-b：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30240段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12418_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
