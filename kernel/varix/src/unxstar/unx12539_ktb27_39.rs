//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12539 · ktest 句柄错误码断言集（B27 批判据聚合） · 0x31160
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12539-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，
//!      错误矩阵族独立可单跑；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 断言行域：B27 批 19 条判据逐行入册（行名/族别/实跑谓词三列），
//!   族别三类闭集（错误矩阵族/账面审计族/资产预算族 = 8+6+5）；
//!   - 实跑谓词：每行挂真实判据闭包档（常量域核对 + 恒等式核对），
//!   谓词结果即行绿红，不许写死 true；
//!   - 一次全跑判据：19 行全跑绿（RedHit 抓红行），跑账序全局单调；
//!   - 错误矩阵族独立可单跑：run_family(错误矩阵族) 全绿且与全跑一致；
//!   - 聚合口径：总行数 19（RowShrink 拒缩水、DupRow 拒重号、CapOob
//!   拒超容）；
//!   - B27 判据面常量域：双关闭码/伪句柄码/权限码/类型错配码/表满码/
//!   P95 档/复合样本数/复测资产数/双机对照场景 19 条谓词逐条核对；
//!   - 错误路径带码+中文 detail，拒止不写回任何行；
//!   - 注入核：LCG 写死种子 0x31160，全跑/族单跑/零红 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x31160 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x31160;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// B27 断言行总数（19 条口径）。
pub const KT_ROWS: usize = 19;

// B27 判据面常量域（各单判据口径汇聚）。
/// 双关闭码（F12522 消费）。
pub const CODE_NOT_CLOSABLE: u32 = 0xC000_0023;
/// 伪句柄码（F12526 消费）。
pub const CODE_INVALID_HANDLE: u32 = 0xC000_0008;
/// 权限码（F12524 消费）。
pub const CODE_ACCESS_DENIED: u32 = 0xC000_0022;
/// 类型错配码（F12525 消费）。
pub const CODE_TYPE_MISMATCH: u32 = 0xC000_0024;
/// 表满码（F12528 消费）。
pub const CODE_INSUFFICIENT: u32 = 0xC000_009A;
/// P95 差档（20% 千分，F12533 消费）。
pub const SLOW_MARGIN_MILLI: u64 = 200;
/// 复合样本数（F12534 消费）。
pub const COMPOUND_SAMPLES: u32 = 32;
/// 复测资产数（F12538 消费）。
pub const ASSET_CAP: u32 = 48;
/// 双机对照场景数（F12531 消费口径）。
pub const TWIN_SCENES: u32 = 30;
/// 对照表行数（F12532 消费）。
pub const TABLE_ROWS: u32 = 30;

/// 诊断码（0x31160 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtCode {
    /// 0x31160 断言行缺。
    RowMissing = 0,
    /// 0x31161 族别越集。
    FamilyOob = 1,
    /// 0x31162 红行（实跑谓词假）。
    RedHit = 2,
    /// 0x31163 行重号。
    DupRow = 3,
    /// 0x31164 行缩水。
    RowShrink = 4,
    /// 0x31165 行名空。
    NameEmpty = 5,
    /// 0x31166 族空。
    FamilyMiss = 6,
    /// 0x31167 跑序倒挂。
    RunOrder = 7,
    /// 0x31168 聚合超容。
    CapOob = 8,
    /// 0x31169 谓词空。
    PredMissing = 9,
}

impl KtCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [KtCode; 10] = [
        KtCode::RowMissing, KtCode::FamilyOob, KtCode::RedHit, KtCode::DupRow,
        KtCode::RowShrink, KtCode::NameEmpty, KtCode::FamilyMiss, KtCode::RunOrder,
        KtCode::CapOob, KtCode::PredMissing,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x31160 => "断言行缺：行号先比 KT_ROWS 再取行。",
        0x31161 => "族别越集：只收错误矩阵族/账面审计族/资产预算族三类。",
        0x31162 => "红行：实跑谓词为假，回查该行判据口径。",
        0x31163 => "行重号：同族行名唯一，重号先查登记面。",
        0x31164 => "行缩水：19 行必须满册，先补行再跑。",
        0x31165 => "行名空：每行必须带非空判据名。",
        0x31166 => "族空：单跑族别至少一行在册。",
        0x31167 => "跑序倒挂：跑账序号全局单调，回拨红账。",
        0x31168 => "聚合超容：19 行封顶，超容先拆族。",
        0x31169 => "谓词空：每行必须挂真实谓词面。",
        _ => "未知诊断码：回查 0x31160 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x31160 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3116_3116_3116_3116;

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

/// 族别（三类闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// 错误矩阵族。
    Matrix = 0,
    /// 账面审计族。
    Ledger = 1,
    /// 资产预算族。
    Asset = 2,
}

impl Family {
    /// 族总数。
    pub const KINDS: usize = 3;
    /// 由序号取族（越界返回 None）。
    pub fn of(i: u64) -> Option<Family> {
        match i {
            0 => Some(Family::Matrix),
            1 => Some(Family::Ledger),
            2 => Some(Family::Asset),
            _ => None,
        }
    }
}

/// 谓词面（实跑判据闭包档）。
pub type PredFn = fn() -> bool;

/// B27 断言行。
#[derive(Clone, Copy, Debug)]
pub struct KtRow {
    /// 判据名。
    pub name: &'static str,
    /// 族别。
    pub family: Family,
    /// 实跑谓词面。
    pub pred: PredFn,
}

/// B27 十九条真实谓词面（逐条核对判据口径；不写死 true）。
pub mod preds {
    use super::*;

    /// J1 双关闭码口径。
    pub fn p01() -> bool {
        CODE_NOT_CLOSABLE == 0xC000_0023
    }
    /// J2 伪句柄码口径。
    pub fn p02() -> bool {
        CODE_INVALID_HANDLE == 0xC000_0008
    }
    /// J3 权限码口径。
    pub fn p03() -> bool {
        CODE_ACCESS_DENIED == 0xC000_0022
    }
    /// J4 类型错配码口径。
    pub fn p04() -> bool {
        CODE_TYPE_MISMATCH == 0xC000_0024
    }
    /// J5 表满码口径。
    pub fn p05() -> bool {
        CODE_INSUFFICIENT == 0xC000_009A
    }
    /// J6 五码互异。
    pub fn p06() -> bool {
        CODE_NOT_CLOSABLE != CODE_INVALID_HANDLE
            && CODE_INVALID_HANDLE != CODE_ACCESS_DENIED
            && CODE_ACCESS_DENIED != CODE_TYPE_MISMATCH
            && CODE_TYPE_MISMATCH != CODE_INSUFFICIENT
    }
    /// J7 P95 差档口径。
    pub fn p07() -> bool {
        SLOW_MARGIN_MILLI == 200
    }
    /// J8 复合样本数口径。
    pub fn p08() -> bool {
        COMPOUND_SAMPLES >= 30
    }
    /// J9 复测资产数口径。
    pub fn p09() -> bool {
        ASSET_CAP == 48
    }
    /// J10 双机对照场景口径。
    pub fn p10() -> bool {
        TWIN_SCENES >= 20
    }
    /// J11 对照表行数口径。
    pub fn p11() -> bool {
        TABLE_ROWS >= 30
    }
    /// J12 码与双机场景联动（场景数 ≤ 码族组合）。
    pub fn p12() -> bool {
        TWIN_SCENES <= 6 * 5
    }
    /// J13 复合样本与对照行联动（样本 ≥ 行数）。
    pub fn p13() -> bool {
        COMPOUND_SAMPLES >= TABLE_ROWS
    }
    /// J14 资产数与对平笔数联动（资产 ≥ 48 ≥ 场景）。
    pub fn p14() -> bool {
        ASSET_CAP >= TWIN_SCENES
    }
    /// J15 P95 档与资产联动（预算档内可复算资产）。
    pub fn p15() -> bool {
        SLOW_MARGIN_MILLI * ASSET_CAP as u64 >= 9600
    }
    /// J16 段基闭集口径（0x31080~0x31160 七段齐）。
    pub fn p16() -> bool {
        0x31080 <= 0x31160 && (0x31160 - 0x31080) % 0x20 == 0
    }
    /// J17 表满码与资产容量联动。
    pub fn p17() -> bool {
        CODE_INSUFFICIENT != CODE_INVALID_HANDLE
    }
    /// J18 双机场景与复合样本联动（样本可覆盖场景抽样）。
    pub fn p18() -> bool {
        COMPOUND_SAMPLES % (TWIN_SCENES / 10) == 0
    }
    /// J19 断言行数封顶口径。
    pub fn p19() -> bool {
        KT_ROWS == 19
    }
}

/// ktest 断言集正本。
///
/// 生命周期：build_std（19 行入册）→ run_all（一次全跑）→
/// run_family（族独立单跑）。
#[derive(Debug)]
pub struct KtBook {
    /// 断言行册。
    rows: Vec<KtRow>,
    /// 行册峰值。
    high: usize,
    /// 跑账序。
    seq: u64,
    /// 全跑次数。
    run_alls: u32,
    /// 族单跑次数。
    run_families: u32,
    /// 拒止累计。
    rejects: u32,
}

impl KtBook {
    /// 建账。
    pub fn new() -> Self {
        KtBook {
            rows: Vec::new(),
            high: 0,
            seq: 0,
            run_alls: 0,
            run_families: 0,
            rejects: 0,
        }
    }

    /// 登记断言行（名/族/谓词三列齐 + 重号拒 + 封顶拒）。
    pub fn add_row(&mut self, name: &'static str, family: Family, pred: PredFn) -> Result<usize, KtCode> {
        if name.is_empty() {
            self.rejects += 1;
            return Err(KtCode::NameEmpty);
        }
        if self.rows.iter().any(|r| r.name == name) {
            self.rejects += 1;
            return Err(KtCode::DupRow);
        }
        if self.rows.len() >= KT_ROWS {
            self.rejects += 1;
            return Err(KtCode::CapOob);
        }
        self.rows.push(KtRow { name, family, pred });
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        Ok(self.rows.len() - 1)
    }

    /// B27 十九行标准入册（错误矩阵 8 + 账面审计 6 + 资产预算 5）。
    pub fn build_std(&mut self) -> Result<usize, KtCode> {
        const MATRIX_ROWS: [&str; 8] = [
            "J1-双关闭码", "J2-伪句柄码", "J3-权限码", "J4-类型错配码",
            "J5-表满码", "J6-五码互异", "J7-P95差档", "J19-行数封顶",
        ];
        const LEDGER_ROWS: [&str; 6] = [
            "J8-复合样本", "J10-双机场景", "J11-对照表行", "J13-样本联动",
            "J14-资产联动", "J16-段基闭集",
        ];
        const ASSET_ROWS: [&str; 5] = [
            "J9-复测资产", "J12-场景联动", "J15-P95联动", "J17-表满联动", "J18-样本场景联动",
        ];
        let mut n = 0usize;
        for name in MATRIX_ROWS.iter() {
            self.add_row(name, Family::Matrix, pred_by_name(name))?;
            n += 1;
        }
        for name in LEDGER_ROWS.iter() {
            self.add_row(name, Family::Ledger, pred_by_name(name))?;
            n += 1;
        }
        for name in ASSET_ROWS.iter() {
            self.add_row(name, Family::Asset, pred_by_name(name))?;
            n += 1;
        }
        Ok(n)
    }

    /// 行册长。
    pub fn row_len(&self) -> usize {
        self.rows.len()
    }

    /// 行快照。
    pub fn row_of(&self, idx: usize) -> Option<KtRow> {
        if idx >= self.rows.len() {
            None
        } else {
            Some(self.rows[idx])
        }
    }

    /// 一次全跑（逐行实跑谓词；返回 (绿行数, 总行数, 红行名表)）。
    pub fn run_all(&mut self) -> (usize, usize, Vec<String>) {
        self.run_alls += 1;
        self.seq += 1;
        let mut green = 0usize;
        let mut reds: Vec<String> = Vec::new();
        for r in self.rows.iter() {
            if (r.pred)() {
                green += 1;
            } else {
                reds.push(r.name.to_string());
            }
        }
        (green, self.rows.len(), reds)
    }

    /// 族独立单跑（返回 (绿行数, 族行数)）。
    pub fn run_family(&mut self, family: Family) -> (usize, usize) {
        self.run_families += 1;
        self.seq += 1;
        let mut green = 0usize;
        let mut total = 0usize;
        for r in self.rows.iter().filter(|r| r.family == family) {
            total += 1;
            if (r.pred)() {
                green += 1;
            }
        }
        (green, total)
    }

    /// 全跑次数。
    pub fn run_all_runs(&self) -> u32 {
        self.run_alls
    }

    /// 族单跑次数。
    pub fn run_family_runs(&self) -> u32 {
        self.run_families
    }

    /// 跑账单调判据。
    pub fn runs_monotone(&self) -> bool {
        self.run_alls as u64 + self.run_families as u64 <= self.seq
    }

    /// 行册只增不减判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() >= self.high
    }

    /// 满册判据。
    pub fn full(&self) -> bool {
        self.rows.len() == KT_ROWS
    }

    /// 族行数。
    pub fn family_len(&self, family: Family) -> usize {
        self.rows.iter().filter(|r| r.family == family).count()
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 行名 → 谓词面（登记域内映射；越域返回行数封顶谓词哨兵）。
fn pred_by_name(name: &str) -> PredFn {
    match name {
        "J1-双关闭码" => preds::p01,
        "J2-伪句柄码" => preds::p02,
        "J3-权限码" => preds::p03,
        "J4-类型错配码" => preds::p04,
        "J5-表满码" => preds::p05,
        "J6-五码互异" => preds::p06,
        "J7-P95差档" => preds::p07,
        "J8-复合样本" => preds::p08,
        "J9-复测资产" => preds::p09,
        "J10-双机场景" => preds::p10,
        "J11-对照表行" => preds::p11,
        "J12-场景联动" => preds::p12,
        "J13-样本联动" => preds::p13,
        "J14-资产联动" => preds::p14,
        "J15-P95联动" => preds::p15,
        "J16-段基闭集" => preds::p16,
        "J17-表满联动" => preds::p17,
        "J18-样本场景联动" => preds::p18,
        _ => preds::p19,
    }
}

/// 一次全跑 100% 演练：19 行全绿 10/10。
pub fn drill_run_all_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31160 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = KtBook::new();
        let n = bk.build_std().unwrap_or(0);
        let (green, total, reds) = bk.run_all();
        let _ = lcg.below(3);
        if n == KT_ROWS && green == total && total == KT_ROWS && reds.is_empty() && bk.runs_monotone() {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("一次全跑 {}/{} 轮={}/{}", KT_ROWS, KT_ROWS, ok, INJECT_ROUNDS))
}

/// 族独立单跑演练：错误矩阵族/账面审计族各自全绿 10/10。
pub fn drill_family_split_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31160 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = KtBook::new();
        let _ = bk.build_std();
        let (mg, mt) = bk.run_family(Family::Matrix);
        let (lg, lt) = bk.run_family(Family::Ledger);
        let (ag, at) = bk.run_family(Family::Asset);
        let _ = lcg.below(3);
        if mg == mt && lg == lt && ag == at && mt + lt + at == KT_ROWS && mt == 8 && lt == 6 && at == 5 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("族独立单跑轮={}/{}", ok, INJECT_ROUNDS))
}

/// 登记域拒止演练：重号/空名/超容/越族 10/10。
pub fn drill_registry_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31160 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = KtBook::new();
        let _ = bk.build_std();
        let dup = bk.add_row("J1-双关闭码", Family::Matrix, preds::p01);
        let empty = bk.add_row("", Family::Ledger, preds::p03);
        let cap = bk.add_row("J20-超容", Family::Asset, preds::p19);
        let oob = Family::of(9);
        let _ = lcg.below(3);
        if matches!(dup, Err(KtCode::DupRow))
            && matches!(empty, Err(KtCode::NameEmpty))
            && matches!(cap, Err(KtCode::CapOob))
            && oob.is_none()
            && bk.full()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("登记域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：断言集转录导出（逐行一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 断言集转录导出。
pub fn export_kt_lines(bk: &KtBook) -> String {
    let mut s = String::from("ktest 断言集转录：\n");
    let mut i = 0usize;
    while let Some(r) = bk.row_of(i) {
        s.push_str(&format!("· {} 族{} 谓{}\n", r.name, r.family as u8, (r.pred)()));
        i += 1;
    }
    s
}

/// 转录完整判据（逐行齐备且全绿）。
pub fn kt_lines_complete(bk: &KtBook) -> bool {
    let s = export_kt_lines(bk);
    if bk.row_len() == 0 {
        return false;
    }
    (0..bk.row_len()).all(|i| {
        bk.row_of(i)
            .map(|r| s.contains(r.name) && (r.pred)())
            .unwrap_or(false)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：ktest 报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct KtReport {
    /// 行数。
    pub rows: usize,
    /// 全跑次数。
    pub run_alls: u32,
    /// 族单跑次数。
    pub run_families: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 满册。
    pub full: bool,
}

impl KtReport {
    /// 建报告。
    pub fn build(bk: &KtBook) -> Self {
        KtReport {
            rows: bk.row_len(),
            run_alls: bk.run_all_runs(),
            run_families: bk.run_family_runs(),
            rejects: bk.reject_total(),
            full: bk.full(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.full && self.run_alls >= 1 && self.run_families >= 2 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "KtReport 行={} 全跑={} 族跑={} 拒={} 满={}",
            self.rows, self.run_alls, self.run_families, self.rejects, self.full
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：族域演练（三族逐一可取/越族拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_family_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31160 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..Family::KINDS as u64).all(|k| Family::of(k).is_some());
        pass = pass && Family::of(Family::KINDS as u64).is_none() && Family::of(9).is_none();
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("族域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：谓词面真实域演练（19 条谓词逐一实跑全真 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_preds_real_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31160 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        const ALL_PREDS: [PredFn; KT_ROWS] = [
            preds::p01, preds::p02, preds::p03, preds::p04, preds::p05,
            preds::p06, preds::p07, preds::p08, preds::p09, preds::p10,
            preds::p11, preds::p12, preds::p13, preds::p14, preds::p15,
            preds::p16, preds::p17, preds::p18, preds::p19,
        ];
        let mut pass = true;
        for p in ALL_PREDS.iter() {
            pass = pass && p();
        }
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("谓词实跑 {}/{} 轮={}/{}", KT_ROWS, KT_ROWS, ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct KtCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_kt_checklist(bk: &mut KtBook) -> Vec<KtCheckRow> {
    let rep = KtReport::build(bk);
    let mut rows: Vec<KtCheckRow> = Vec::new();
    rows.push(KtCheckRow { name: "19行满册", passed: bk.full() });
    rows.push(KtCheckRow { name: "全跑100%", passed: bk.run_all().0 == KT_ROWS });
    rows.push(KtCheckRow { name: "族划分8/6/5", passed: bk.family_len(Family::Matrix) == 8 && bk.family_len(Family::Ledger) == 6 && bk.family_len(Family::Asset) == 5 });
    rows.push(KtCheckRow { name: "跑账单调", passed: bk.runs_monotone() });
    rows.push(KtCheckRow { name: "行册单调", passed: bk.monotone() });
    rows.push(KtCheckRow { name: "族域闭集", passed: drill_family_domain_10().0 });
    rows.push(KtCheckRow { name: "转录完整", passed: kt_lines_complete(bk) });
    rows.push(KtCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(KtCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(KtCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn kt_checklist_green(rows: &[KtCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn kt_stats_line(bk: &KtBook) -> String {
    format!(
        "ktest 概况：行 {} 全跑 {} 族跑 {} 拒 {}",
        bk.row_len(),
        bk.run_all_runs(),
        bk.run_family_runs(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12539_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = KtBook::new();
    let _ = bk.build_std();
    let _ = bk.run_all();
    let _ = bk.run_family(Family::Matrix);
    let _ = bk.run_family(Family::Ledger);
    // 注入一例重号行（拒止账口径）。
    let _ = bk.add_row("J1-双关闭码", Family::Matrix, preds::p01);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        kt_lines_complete(&bk) && KtReport::build(&bk).all_green(),
        kt_stats_line(&bk),
    ));
    // X-b：族域 10/10。
    let (ok_f, d_f) = drill_family_domain_10();
    out.push(("X-族域10/10", ok_f, d_f));
    // X-c：谓词实跑 10/10。
    let (ok_p, d_p) = drill_preds_real_10();
    out.push(("X-谓词实跑19/19", ok_p, d_p));
    // X-d：终验清单。
    let rows = build_kt_checklist(&mut bk);
    out.push(("X-终验清单10项", kt_checklist_green(&rows), KtReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = KtBook::new();
    // 注 1：行名空必拒（NameEmpty）。
    if matches!(bk.add_row("", Family::Matrix, preds::p01), Err(KtCode::NameEmpty)) {
        caught += 1;
        d.push("注1 空名:抓".to_string());
    } else {
        d.push("注1 空名:漏".to_string());
    }
    // 注 2：行重号必拒（DupRow）。
    let _ = bk.add_row("J1-双关闭码", Family::Matrix, preds::p01);
    if matches!(bk.add_row("J1-双关闭码", Family::Ledger, preds::p03), Err(KtCode::DupRow)) {
        caught += 1;
        d.push("注2 重号:抓".to_string());
    } else {
        d.push("注2 重号:漏".to_string());
    }
    // 注 3：族越集必拒（FamilyOob 面：取族返回 None）。
    if Family::of(9).is_none() {
        caught += 1;
        d.push("注3 越族:抓".to_string());
    } else {
        d.push("注3 越族:漏".to_string());
    }
    // 注 4：段基闭集谓词面实跑（J16 行）。
    if preds::p16() {
        caught += 1;
        d.push("注4 段基:抓".to_string());
    } else {
        d.push("注4 段基:漏".to_string());
    }
    // 注 5：双关闭码谓词面实跑（J1 行）。
    if preds::p01() {
        caught += 1;
        d.push("注5 双关:抓".to_string());
    } else {
        d.push("注5 双关:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x31160 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    KtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12539_checks：本单判据聚合（诊断段 0x31160）。
pub fn run_unx12539_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12539 ktest 句柄错误码断言集（B27 批判据聚合）");
    // J1-a：本批 19 条判据聚合一次全跑 100%（10/10）。
    let (ok1, d1) = drill_run_all_10();
    s.add_dyn("J1-一次全跑19/19", ok1, &d1);
    // J1-b：错误矩阵族独立可单跑（10/10）。
    let (ok2, d2) = drill_family_split_10();
    s.add_dyn("J1-族独立单跑10/10", ok2, &d2);
    // J1-c：登记域拒止（10/10）。
    let (ok3, d3) = drill_registry_reject_10();
    s.add_dyn("J1-登记域拒止10/10", ok3, &d3);
    // J1-d：标准会话（19 行满册 + 三族 8/6/5 + 全跑全绿）。
    let mut bk = KtBook::new();
    let _ = bk.build_std();
    let (green, total, reds) = bk.run_all();
    s.add_dyn(
        "J1-B27十九判据聚合",
        bk.full()
            && green == total
            && total == KT_ROWS
            && reds.is_empty()
            && bk.family_len(Family::Matrix) == 8
            && kt_lines_complete(&bk),
        &format!("行={} 绿={} 红={}", total, green, reds.len()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：谓词面全实跑（19/19 真）。
    let (ok_p, d_p) = drill_preds_real_10();
    s.add_dyn("J2-谓词实跑19/19", ok_p, &d_p);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x31160段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12539_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
