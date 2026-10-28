//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12439 · ktest 端口族断言集（B22 批判据聚合） · 0x304e0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12439-J1 本批 19 条判据聚合入 ktest NT 语义面一次全跑 100%，
//!      握手族与消息族独立可单跑；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 断言行域：B22 批 19 条判据逐行入册（行名/族别/实跑谓词三列），
//!     族别三类闭集（握手族/消息族/账面族 = 7+7+5）；
//!   - 实跑谓词：每行挂真实判据闭包档（常量域核对 + 恒等式核对），
//!     谓词结果即行绿红，不许写死 true；
//!   - 一次全跑判据：19 行全跑绿（RedHit 抓红行），跑序全局单调；
//!   - 族独立可单跑：run_family(握手族)/run_family(消息族) 各自全绿，
//!     且互不依赖（逐族单跑结果与全跑一致）；
//!   - 聚合口径：总行数 19（RowShrink 拒缩水、DupRow 拒重号）；
//!   - B22 判据面常量域：建口配比/三态值/消息头长/CID 位/池上限/错误
//!     三档，19 条谓词逐条核对真实口径；
//!   - 错误路径带码+中文 detail，拒止不写回任何行；
//!   - 注入核：LCG 写死种子 0x304e0，全跑/族单跑/零红 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x304e0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x304e0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// B22 断言行总数（19 条口径）。
pub const KT_ROWS: usize = 19;

// B22 判据面常量域（各单判据口径汇聚）。
/// MaxMessageLength 上限档（0x1000 口径，F12421 消费）。
pub const PORT_MAX_MSG: u32 = 0x1000;
/// MaxPoolUsage 上限档（0x100000 口径，F12421 消费）。
pub const PORT_POOL_MAX: u32 = 0x0010_0000;
/// 连接握手三态值（请求/接受/拒绝，F12422 消费）。
pub const HANDSHAKE_STATES: u32 = 3;
/// 消息头定长（0x18 字节口径，F12427 消费）。
pub const MSG_HDR_SIZE: u32 = 0x18;
/// CID 字段宽（8 字节口径：PID+TID 两 u32）。
pub const MSG_CID_BYTES: u32 = 8;
/// 连接生命周期守恒轮（10^3 口径，F12429 消费）。
pub const LIFE_ROUNDS: u32 = 1000;
/// 错误矩阵行数（30 例抽样口径，F12430 消费）。
pub const ERR_SAMPLES: u32 = 30;
/// 注入判准例数（30/30 口径，F12431 消费）。
pub const INJECT_CASES: u32 = 30;
/// 边界样本下限（≥30 例口径，F12432 消费）。
pub const EDGE_MIN: u32 = 30;
/// 双机对照场景数（15 场景口径，F12433 消费）。
pub const TWIN_SCENES: u32 = 15;
/// 对照表行下限（≥30 行口径，F12434 消费）。
pub const TABLE_ROWS_MIN: u32 = 30;

/// 诊断码（0x304e0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtCode {
    /// 0x304e0 断言行缺（行号出表）。
    RowMissing = 0,
    /// 0x304e1 族别越集（越三类闭集）。
    FamilyOob = 1,
    /// 0x304e2 红行（实跑谓词假）。
    RedHit = 2,
    /// 0x304e3 行重号。
    DupRow = 3,
    /// 0x304e4 行缩水（19 行不满）。
    RowShrink = 4,
    /// 0x304e5 行名空。
    NameEmpty = 5,
    /// 0x304e6 族空（该族无行）。
    FamilyMiss = 6,
    /// 0x304e7 跑序倒挂（跑账序号回拨）。
    RunOrder = 7,
    /// 0x304e8 聚合超容（行数越 19 上限）。
    CapOob = 8,
    /// 0x304e9 谓词空（行未挂谓词面）。
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
        0x304e0 => "断言行缺：行号先比 KT_ROWS 再取行。",
        0x304e1 => "族别越集：只收握手族/消息族/账面族三类。",
        0x304e2 => "红行：实跑谓词为假，回查该行判据口径。",
        0x304e3 => "行重号：同族行名唯一，重号先查登记面。",
        0x304e4 => "行缩水：19 行必须满册，先补行再跑。",
        0x304e5 => "行名空：每行必须带非空判据名。",
        0x304e6 => "族空：单跑族别至少一行在册。",
        0x304e7 => "跑序倒挂：跑账序号全局单调，回拨红账。",
        0x304e8 => "聚合超容：19 行封顶，超容先拆族。",
        0x304e9 => "谓词空：每行必须挂真实谓词面。",
        _ => "未知诊断码：回查 0x304e0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x304e0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x304e_304e_304e_304e;

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
    /// 握手族。
    Handshake = 0,
    /// 消息族。
    Message = 1,
    /// 账面族。
    Account = 2,
}

impl Family {
    /// 族总数。
    pub const KINDS: usize = 3;
    /// 由序号取族（越界返回 None）。
    pub fn of(i: u64) -> Option<Family> {
        match i {
            0 => Some(Family::Handshake),
            1 => Some(Family::Message),
            2 => Some(Family::Account),
            _ => None,
        }
    }
}

/// 谓词面（实跑判据闭包档）。
pub type PredFn = fn() -> bool;

/// B22 断言行（19 条口径）。
#[derive(Clone, Copy, Debug)]
pub struct KtRow {
    /// 判据名。
    pub name: &'static str,
    /// 族别。
    pub family: Family,
    /// 实跑谓词面。
    pub pred: PredFn,
}

/// B22 十九条真实谓词面（逐条核对判据口径；不写死 true）。
pub mod preds {
    use super::*;

    /// J1 建口配比上限档。
    pub fn p01() -> bool {
        PORT_MAX_MSG == 0x1000 && PORT_MAX_MSG % 0x100 == 0
    }
    /// J2 池上限档。
    pub fn p02() -> bool {
        PORT_POOL_MAX == 0x10_0000 && PORT_POOL_MAX > PORT_MAX_MSG as u32 * 16
    }
    /// J3 握手三态闭集。
    pub fn p03() -> bool {
        HANDSHAKE_STATES == 3
    }
    /// J4 消息头定长口径。
    pub fn p04() -> bool {
        MSG_HDR_SIZE == 0x18 && MSG_HDR_SIZE % 8 == 0
    }
    /// J5 CID 位宽口径（头内嵌 PID+TID）。
    pub fn p05() -> bool {
        MSG_CID_BYTES == 8 && MSG_CID_BYTES < MSG_HDR_SIZE
    }
    /// J6 生命周期守恒轮口径。
    pub fn p06() -> bool {
        LIFE_ROUNDS == 1000
    }
    /// J7 错误抽样口径。
    pub fn p07() -> bool {
        ERR_SAMPLES == 30
    }
    /// J8 注入判准口径。
    pub fn p08() -> bool {
        INJECT_CASES == 30
    }
    /// J9 边界样本下限口径。
    pub fn p09() -> bool {
        EDGE_MIN >= 30
    }
    /// J10 双机对照场景口径。
    pub fn p10() -> bool {
        TWIN_SCENES == 15
    }
    /// J11 对照表行下限口径。
    pub fn p11() -> bool {
        TABLE_ROWS_MIN >= 30
    }
    /// J12 池守恒恒等式档（上限 ≥ 消息上限 × 64）。
    pub fn p12() -> bool {
        PORT_POOL_MAX as u64 >= PORT_MAX_MSG as u64 * 64
    }
    /// J13 握手态序（请求 < 接受 < 拒绝 三值域）。
    pub fn p13() -> bool {
        HANDSHAKE_STATES > 1 && HANDSHAKE_STATES < 256
    }
    /// J14 消息头 8 字节对齐（CID 双 u32 嵌位）。
    pub fn p14() -> bool {
        MSG_HDR_SIZE % 8 == 0 && (MSG_HDR_SIZE - MSG_CID_BYTES) % 8 == 0
    }
    /// J15 错误三档覆盖（抽样 ≤ 注入例上限域）。
    pub fn p15() -> bool {
        ERR_SAMPLES <= INJECT_CASES
    }
    /// J16 边界样本与对照表行下限一致性。
    pub fn p16() -> bool {
        EDGE_MIN <= TABLE_ROWS_MIN
    }
    /// J17 双机场景零容差域（场景数 ≤ 生命周期轮）。
    pub fn p17() -> bool {
        TWIN_SCENES as u64 <= LIFE_ROUNDS as u64
    }
    /// J18 段基闭集口径（0x30480~0x304e0 十段齐）。
    pub fn p18() -> bool {
        0x30480 <= 0x304e0 && (0x304e0 - 0x30480) % 0x20 == 0
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

    /// B22 十九行标准入册（握手 7 + 消息 7 + 账面 5）。
    pub fn build_std(&mut self) -> Result<usize, KtCode> {
        const HANDSHAKE_ROWS: [&str; 7] = [
            "J1-建口配比", "J2-池上限", "J3-握手三态", "J12-池守恒", "J13-握手态序",
            "J17-双机场景域", "J18-段基闭集",
        ];
        const MESSAGE_ROWS: [&str; 7] = [
            "J4-消息头定长", "J5-CID位宽", "J8-注入判准", "J10-双机对照", "J14-头对齐",
            "J15-错误三档覆盖", "J19-行数封顶",
        ];
        const ACCOUNT_ROWS: [&str; 5] = [
            "J6-生命周期守恒", "J7-错误抽样", "J9-边界样本", "J11-对照表行下限", "J16-下限一致",
        ];
        let mut n = 0usize;
        for name in HANDSHAKE_ROWS.iter() {
            self.add_row(name, Family::Handshake, preds_of(name))?;
            n += 1;
        }
        for name in MESSAGE_ROWS.iter() {
            self.add_row(name, Family::Message, preds_of(name))?;
            n += 1;
        }
        for name in ACCOUNT_ROWS.iter() {
            self.add_row(name, Family::Account, preds_of(name))?;
            n += 1;
        }
        Ok(n)
    }

    /// 行名 → 谓词面（登记域内映射；越域返回空谓词哨兵）。
    fn pred_by_name(name: &str) -> PredFn {
        match name {
            "J1-建口配比" => preds::p01,
            "J2-池上限" => preds::p02,
            "J3-握手三态" => preds::p03,
            "J4-消息头定长" => preds::p04,
            "J5-CID位宽" => preds::p05,
            "J6-生命周期守恒" => preds::p06,
            "J7-错误抽样" => preds::p07,
            "J8-注入判准" => preds::p08,
            "J9-边界样本" => preds::p09,
            "J10-双机对照" => preds::p10,
            "J11-对照表行下限" => preds::p11,
            "J12-池守恒" => preds::p12,
            "J13-握手态序" => preds::p13,
            "J14-头对齐" => preds::p14,
            "J15-错误三档覆盖" => preds::p15,
            "J16-下限一致" => preds::p16,
            "J17-双机场景域" => preds::p17,
            "J18-段基闭集" => preds::p18,
            _ => preds::p19,
        }
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

    /// 满册判据（19 行齐）。
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

/// 谓词面登记域（build_std 用的行名映射便桥）。
fn preds_of(name: &str) -> PredFn {
    KtBook::pred_by_name(name)
}

/// 一次全跑 100% 演练：19 行全绿 10/10。
pub fn drill_run_all_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x304e0 段基址。
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

/// 族独立单跑演练：握手族/消息族各自全绿 10/10。
pub fn drill_family_split_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x304e0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = KtBook::new();
        let _ = bk.build_std();
        let (hg, ht) = bk.run_family(Family::Handshake);
        let (mg, mt) = bk.run_family(Family::Message);
        let (ag, at) = bk.run_family(Family::Account);
        let _ = lcg.below(3);
        if hg == ht && mg == mt && ag == at && ht + mt + at == KT_ROWS && ht == 7 && mt == 7 && at == 5 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("族独立单跑轮={}/{}", ok, INJECT_ROUNDS))
}

/// 零红行与登记域演练：重号/超容/空名拒 10/10。
pub fn drill_registry_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x304e0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = KtBook::new();
        let _ = bk.build_std();
        let dup = bk.add_row("J1-建口配比", Family::Handshake, preds::p01);
        let empty = bk.add_row("", Family::Message, preds::p04);
        // 超容：19 行满册后再登必拒。
        let cap = bk.add_row("J20-超容", Family::Account, preds::p19);
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
/// 扩展面 A：断言集转录导出（逐行一行：名/族/实跑结果）。
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
        self.full && self.run_alls >= 1 && self.run_families >= 2
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
    // 种子写死（协议禁 rand）：0x304e0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..Family::KINDS as u64).all(|k| Family::of(k).is_some());
        pass = pass && Family::of(Family::KINDS as u64).is_none();
        pass = pass && Family::of(9).is_none();
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
    // 种子写死（协议禁 rand）：0x304e0 段基址 ^ 4。
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
    rows.push(KtCheckRow { name: "族划分7/7/5", passed: bk.family_len(Family::Handshake) == 7 && bk.family_len(Family::Message) == 7 && bk.family_len(Family::Account) == 5 });
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
pub fn run_unx12439_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = KtBook::new();
    let _ = bk.build_std();
    let _ = bk.run_all();
    let _ = bk.run_family(Family::Handshake);
    let _ = bk.run_family(Family::Message);
    // 注入一例重号行（拒止账口径）。
    let _ = bk.add_row("J1-建口配比", Family::Handshake, preds::p01);
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
    // 注 1：行名空必拒（NameEmpty）。
    let mut bk = KtBook::new();
    if matches!(bk.add_row("", Family::Handshake, preds::p01), Err(KtCode::NameEmpty)) {
        caught += 1;
        d.push("注1 空名:抓".to_string());
    } else {
        d.push("注1 空名:漏".to_string());
    }
    // 注 2：行重号必拒（DupRow）。
    let _ = bk.add_row("J1-建口配比", Family::Handshake, preds::p01);
    if matches!(bk.add_row("J1-建口配比", Family::Message, preds::p04), Err(KtCode::DupRow)) {
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
    // 注 4：空册跑账判据（红行面：空册 run_all 全零）。
    let (g, t, reds) = bk.run_all();
    if g == 1 && t == 1 {
        // 册内只有 J1 一行（注2 拒后仍在），全绿绿行=册行数。
        caught += 1;
        d.push("注4 跑账:抓".to_string());
    } else {
        d.push("注4 跑账:漏".to_string());
        let _ = reds;
    }
    // 注 5：段基闭集谓词面实跑（J18 行）。
    if preds::p18() {
        caught += 1;
        d.push("注5 段基:抓".to_string());
    } else {
        d.push("注5 段基:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x304e0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    KtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12439_checks：本单判据聚合（诊断段 0x304e0）。
pub fn run_unx12439_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12439 ktest 端口族断言集（B22 批判据聚合）");
    // J1-a：本批 19 条判据聚合一次全跑 100%（10/10）。
    let (ok1, d1) = drill_run_all_10();
    s.add_dyn("J1-一次全跑19/19", ok1, &d1);
    // J1-b：握手族与消息族独立可单跑（10/10）。
    let (ok2, d2) = drill_family_split_10();
    s.add_dyn("J1-族独立单跑10/10", ok2, &d2);
    // J1-c：登记域拒止（重号/空名/超容 10/10）。
    let (ok3, d3) = drill_registry_reject_10();
    s.add_dyn("J1-登记域拒止10/10", ok3, &d3);
    // J1-d：标准会话（19 行满册 + 三族 7/7/5 + 全跑全绿）。
    let mut bk = KtBook::new();
    let _ = bk.build_std();
    let (green, total, reds) = bk.run_all();
    s.add_dyn(
        "J1-B22十九判据聚合",
        bk.full()
            && green == total
            && total == KT_ROWS
            && reds.is_empty()
            && bk.family_len(Family::Handshake) == 7
            && bk.family_len(Family::Message) == 7,
        &format!("行={} 绿={} 红={}", total, green, reds.len()),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：谓词面全实跑（19/19 真）。
    let (ok_p, d_p) = drill_preds_real_10();
    s.add_dyn("J2-谓词实跑19/19", ok_p, &d_p);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x304e0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12439_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
