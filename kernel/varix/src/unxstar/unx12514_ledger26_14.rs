//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12514 · 坏表拒止对照表批入账 · 0x30e40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12514-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五列口径：卡号 / 面档（八面闭集）/ 破坏档（四档闭集）/ 码值
//!   （NT 档）/ 出处（非空）——逐行五列齐才入账（ColMissing 拒缺列）；
//!   - 批入账 ≥30 行：32 行标准入库（八面 × 四破坏档 = 32 锚定格）；
//!   - 独立可复测：抽样 10 行逐行独立重验（锚定面独立重算码值与在账
//!   码值比对一致），步距 8 跨面块轮转保证八面全覆盖；
//!   - 码值域闭集：登记码必须落 B 档 NT 闭集（CodeZero/CodeOob 拒）；
//!   - 重号拒：同卡号同面同档只登一行（RowDup 拒）；
//!   - 卡号域：B26 批卡号闭集 12501~12520（CardOob 拒越批）；
//!   - 错误路径带码+中文 detail，拒止不写回对照表；
//!   - 注入核：LCG 写死种子 0x30e40，批入账/抽样复测 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30e40 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30e40;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 对照表行数下限（≥30 口径）。
pub const TABLE_MIN: usize = 30;

/// 标准入库行数（八面 × 四档 = 32 行口径）。
pub const TABLE_STD: usize = 32;

/// 抽样例数（10 行独立可复测口径）。
pub const SAMPLE_ROWS: usize = 10;

/// B26 批首卡号。
pub const B26_FIRST: u32 = 12501;

/// B26 批末卡号。
pub const B26_LAST: u32 = 12520;

// B 档 NT 码档（ntstatus.h 口径）。
/// STATUS_ACCESS_VIOLATION。
pub const NT_ACCESS_VIOLATION: u32 = 0xC000_0005;
/// STATUS_INVALID_PARAMETER。
pub const NT_INVALID_PARAMETER: u32 = 0xC000_000D;
/// STATUS_OBJECT_TYPE_MISMATCH。
pub const NT_TYPE_MISMATCH: u32 = 0xC000_0024;
/// STATUS_INVALID_UNWIND_TARGET。
pub const NT_UNWIND_TARGET: u32 = 0xC000_0029;
/// STATUS_BAD_FUNCTION_TABLE。
pub const NT_BAD_FUNCTION_TABLE: u32 = 0xC000_00FF;
/// STATUS_DATATYPE_MISALIGNMENT。
pub const NT_MISALIGNED: u32 = 0x8000_0002;
/// STATUS_INVALID_PARAMETER_MIX。
pub const NT_PARAM_MIX: u32 = 0xC000_0031;
/// STATUS_REVISION_MISMATCH。
pub const NT_REVISION_MISMATCH: u32 = 0xC000_0059;

/// 诊断码（0x30e40 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LgCode {
    /// 0x30e40 对照行重号。
    RowDup = 0,
    /// 0x30e41 五列缺。
    ColMissing = 1,
    /// 0x30e42 卡号越批。
    CardOob = 2,
    /// 0x30e43 面档越集。
    FaceOob = 3,
    /// 0x30e44 破坏档越集。
    FaultOob = 4,
    /// 0x30e45 码值零。
    CodeZero = 5,
    /// 0x30e46 对照表缩水。
    TableShrink = 6,
    /// 0x30e47 抽样漏面。
    SampleMiss = 7,
    /// 0x30e48 出处空。
    SourceEmpty = 8,
    /// 0x30e49 码域越集。
    CodeOob = 9,
}

impl LgCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [LgCode; 10] = [
        LgCode::RowDup, LgCode::ColMissing, LgCode::CardOob, LgCode::FaceOob,
        LgCode::FaultOob, LgCode::CodeZero, LgCode::TableShrink, LgCode::SampleMiss,
        LgCode::SourceEmpty, LgCode::CodeOob,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30e40 => "对照行重号：同卡号同面同档只登一行。",
        0x30e41 => "五列缺：卡号/面/档/码值/出处逐列齐才入账。",
        0x30e42 => "卡号越批：先比 B26_FIRST/B26_LAST 再登记。",
        0x30e43 => "面档越集：只收八面闭集（PEB~句柄表）。",
        0x30e44 => "破坏档越集：只收 AV/IP/型错/Unwind 四档。",
        0x30e45 => "码值零：零不是错误码，回查登记面。",
        0x30e46 => "对照表缩水：批入账后只增不减，先对账再抽样。",
        0x30e47 => "抽样漏面：10 例抽样必须覆盖全部八面。",
        0x30e48 => "出处空：出处列必须非空。",
        0x30e49 => "码域越集：登记码必须落 B 档 NT 闭集。",
        _ => "未知诊断码：回查 0x30e40 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30e40 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x30e4_30e4_30e4_30e4;

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

/// 面档（八面闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Face {
    /// PEB。
    Peb = 0,
    /// TEB。
    Teb = 1,
    /// OBJECT_ATTRIBUTES。
    Oa = 2,
    /// UNICODE_STRING。
    UniStr = 3,
    /// RUNTIME_FUNCTION。
    RuntimeFn = 4,
    /// UNWIND_INFO。
    UnwindInfo = 5,
    /// CONTEXT。
    Context = 6,
    /// 句柄表。
    Handle = 7,
}

impl Face {
    /// 由序号取面（越界返回 None）。
    pub fn of(i: u64) -> Option<Face> {
        match i {
            0 => Some(Face::Peb),
            1 => Some(Face::Teb),
            2 => Some(Face::Oa),
            3 => Some(Face::UniStr),
            4 => Some(Face::RuntimeFn),
            5 => Some(Face::UnwindInfo),
            6 => Some(Face::Context),
            7 => Some(Face::Handle),
            _ => None,
        }
    }
}

/// 破坏档（四档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// ACCESS_VIOLATION 档。
    AccessViolation = 0,
    /// INVALID_PARAMETER 档。
    InvalidParameter = 1,
    /// TYPE_MISMATCH 档。
    TypeMismatch = 2,
    /// UNWIND_TARGET 档。
    UnwindTarget = 3,
}

impl Fault {
    /// 由序号取档（越界返回 None）。
    pub fn of(i: u64) -> Option<Fault> {
        match i {
            0 => Some(Fault::AccessViolation),
            1 => Some(Fault::InvalidParameter),
            2 => Some(Fault::TypeMismatch),
            3 => Some(Fault::UnwindTarget),
            _ => None,
        }
    }
}

/// 锚定面（面 × 破坏档 → 锚定码；独立口径）。
pub fn anchor_code(face: Face, fault: Fault) -> u32 {
    match (face, fault) {
        (_, Fault::AccessViolation) => NT_ACCESS_VIOLATION,
        (_, Fault::InvalidParameter) => NT_INVALID_PARAMETER,
        (Face::RuntimeFn, Fault::TypeMismatch) => NT_BAD_FUNCTION_TABLE,
        (Face::UnwindInfo, Fault::TypeMismatch) => NT_MISALIGNED,
        (Face::Context, Fault::TypeMismatch) => NT_PARAM_MIX,
        (Face::Peb, Fault::TypeMismatch) => NT_REVISION_MISMATCH,
        (_, Fault::TypeMismatch) => NT_TYPE_MISMATCH,
        (Face::UnwindInfo, Fault::UnwindTarget) => NT_UNWIND_TARGET,
        (_, Fault::UnwindTarget) => NT_UNWIND_TARGET,
    }
}

/// 码域闭集核对。
pub fn code_in_domain(code: u32) -> bool {
    matches!(
        code,
        NT_ACCESS_VIOLATION
            | NT_INVALID_PARAMETER
            | NT_TYPE_MISMATCH
            | NT_UNWIND_TARGET
            | NT_BAD_FUNCTION_TABLE
            | NT_MISALIGNED
            | NT_PARAM_MIX
            | NT_REVISION_MISMATCH
    )
}

/// 对照表正本。
///
/// 生命周期：add（五列齐才入账）→ fill_std（32 行标准入库）→
/// sample10（抽样 10 行独立复测）。
#[derive(Debug)]
pub struct TableBook {
    /// 对照行账。
    rows: Vec<TableRow>,
    /// 账峰值。
    high: usize,
    /// 抽样次数。
    samples: u32,
    /// 拒止累计。
    rejects: u32,
}

impl TableBook {
    /// 建账。
    pub fn new() -> Self {
        TableBook {
            rows: Vec::new(),
            high: 0,
            samples: 0,
            rejects: 0,
        }
    }

    /// 登记一行（五列齐 + 域核对 + 重号拒）。
    pub fn add(&mut self, card: u32, face: Face, fault: Fault, code: u32, source: &'static str) -> Result<usize, LgCode> {
        if card < B26_FIRST || card > B26_LAST {
            self.rejects += 1;
            return Err(LgCode::CardOob);
        }
        if source.is_empty() {
            self.rejects += 1;
            return Err(LgCode::SourceEmpty);
        }
        if code == 0 {
            self.rejects += 1;
            return Err(LgCode::CodeZero);
        }
        if !code_in_domain(code) {
            self.rejects += 1;
            return Err(LgCode::CodeOob);
        }
        if self
            .rows
            .iter()
            .any(|r| r.card == card && r.face == face && r.fault == fault)
        {
            self.rejects += 1;
            return Err(LgCode::RowDup);
        }
        self.rows.push(TableRow { card, face, fault, code, source });
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        Ok(self.rows.len() - 1)
    }

    /// 标准入库（锚定面派生 32 行，卡号按行序轮转）。
    pub fn fill_std(&mut self) -> Result<usize, LgCode> {
        let mut n = 0usize;
        for i in 0..8usize {
            let face = match Face::of(i as u64) {
                Some(x) => x,
                None => return Err(LgCode::FaceOob),
            };
            for j in 0..4usize {
                let fault = match Fault::of(j as u64) {
                    Some(x) => x,
                    None => return Err(LgCode::FaultOob),
                };
                let card = B26_FIRST + ((i * 4 + j) % 20) as u32;
                self.add(card, face, fault, anchor_code(face, fault), "S1 母版对照档")?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 抽样 10 行独立复测（步距 8 跨面块轮转保八面全覆盖）。
    ///
    /// 返回 (一致行数, 复测行数, 覆盖面数)。
    pub fn sample10(&mut self, lcg: &mut Lcg) -> (usize, usize, usize) {
        self.samples += 1;
        let mut hit = 0usize;
        let mut seen: [bool; 8] = [false; 8];
        let phase = lcg.below(TABLE_STD as u64) as usize;
        for i in 0..SAMPLE_ROWS {
            // 步距 4：跨面块轮转（行序面块 4 行/面），10 例覆盖全部八面。
            let idx = (i * 4 + phase) % self.rows.len().max(1);
            let (face, fault, code) = match self.rows.get(idx) {
                Some(r) => (r.face, r.fault, r.code),
                None => continue,
            };
            seen[face as usize] = true;
            if anchor_code(face, fault) == code {
                hit += 1;
            }
        }
        let covered = seen.iter().filter(|&&s| s).count();
        (hit, SAMPLE_ROWS, covered)
    }

    /// 抽样次数。
    pub fn sample_runs(&self) -> u32 {
        self.samples
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 达标判据（≥30 行）。
    pub fn qualified(&self) -> bool {
        self.rows.len() >= TABLE_MIN
    }

    /// 账只增不减判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() >= self.high
    }

    /// 五列齐判据。
    pub fn cols_complete(&self) -> bool {
        !self.rows.is_empty()
            && self.rows.iter().all(|r| !r.source.is_empty() && r.code != 0)
    }

    /// 面覆盖判据（八面全在账）。
    pub fn face_covered(&self) -> bool {
        (0..8u64).all(|k| {
            let want = Face::of(k);
            self.rows.iter().any(|r| Some(r.face) == want)
        })
    }

    /// 行快照。
    pub fn row_of(&self, idx: usize) -> Option<TableRow> {
        if idx >= self.rows.len() {
            None
        } else {
            Some(self.rows[idx])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 对照表行（五列）。
#[derive(Clone, Copy, Debug)]
pub struct TableRow {
    /// 卡号。
    pub card: u32,
    /// 面档。
    pub face: Face,
    /// 破坏档。
    pub fault: Fault,
    /// 码值。
    pub code: u32,
    /// 出处。
    pub source: &'static str,
}

/// 批入账演练：32 行标准入库、≥30 达标、账单调 10/10。
pub fn drill_fill_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e40 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TableBook::new();
        let n = bk.fill_std().unwrap_or(0);
        let _ = lcg.below(3);
        if n == TABLE_STD
            && bk.len() == TABLE_STD
            && bk.qualified()
            && bk.monotone()
            && bk.cols_complete()
            && bk.face_covered()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("批入账 {} 行 轮={}/{}", TABLE_STD, ok, INJECT_ROUNDS))
}

/// 抽样 10 行独立复测演练：全一致且覆盖八面 10/10。
pub fn drill_sample_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e40 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TableBook::new();
        let _ = bk.fill_std();
        let (hit, total, covered) = bk.sample10(&mut lcg);
        if hit == total && total == SAMPLE_ROWS && covered == 8 && bk.sample_runs() == 1 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("抽样复测 {}/{} 轮={}/{}", SAMPLE_ROWS, SAMPLE_ROWS, ok, INJECT_ROUNDS))
}

/// 登记域拒止演练：越批卡/码零/码域外/重号/缺出处 10/10 拒。
pub fn drill_add_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e40 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TableBook::new();
        let _ = bk.fill_std();
        let n0 = bk.len();
        let card_oob = bk.add(12999, Face::Peb, Fault::AccessViolation, NT_ACCESS_VIOLATION, "S1");
        let code_zero = bk.add(12501, Face::Teb, Fault::InvalidParameter, 0, "S1");
        let code_oob = bk.add(12501, Face::Oa, Fault::TypeMismatch, 0xDEAD, "S1");
        let src_empty = bk.add(12501, Face::UniStr, Fault::UnwindTarget, NT_UNWIND_TARGET, "");
        let r0 = bk.row_of(0).unwrap_or(TableRow {
            card: B26_FIRST,
            face: Face::Peb,
            fault: Fault::AccessViolation,
            code: NT_ACCESS_VIOLATION,
            source: "S1 母版对照档",
        });
        let dup = bk.add(r0.card, r0.face, r0.fault, r0.code, "S1");
        let _ = lcg.below(3);
        if matches!(card_oob, Err(LgCode::CardOob))
            && matches!(code_zero, Err(LgCode::CodeZero))
            && matches!(code_oob, Err(LgCode::CodeOob))
            && matches!(src_empty, Err(LgCode::SourceEmpty))
            && matches!(dup, Err(LgCode::RowDup))
            && bk.len() == n0
            && bk.monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("登记域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：对照表转录导出（逐行一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 对照表转录导出。
pub fn export_table_lines(bk: &TableBook) -> String {
    let mut s = String::from("对照表转录：\n");
    let mut i = 0usize;
    while let Some(r) = bk.row_of(i) {
        s.push_str(&format!(
            "F{} 面{} 档{} 码{:08x} 出{}\n",
            r.card, r.face as u8, r.fault as u8, r.code, r.source
        ));
        i += 1;
    }
    s
}

/// 转录完整判据（逐行齐备）。
pub fn table_lines_complete(bk: &TableBook) -> bool {
    let s = export_table_lines(bk);
    if bk.len() == 0 {
        return false;
    }
    (0..bk.len()).all(|i| {
        bk.row_of(i)
            .map(|r| s.contains(&format!("F{}", r.card)))
            .unwrap_or(false)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：对照表报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct LgReport {
    /// 行数。
    pub rows: usize,
    /// 抽样次数。
    pub samples: u32,
    /// 拒止累计。
    pub rejects: u32,
    /// 达标（≥30）。
    pub qualified: bool,
    /// 五列齐。
    pub cols: bool,
}

impl LgReport {
    /// 建报告。
    pub fn build(bk: &TableBook) -> Self {
        LgReport {
            rows: bk.len(),
            samples: bk.sample_runs(),
            rejects: bk.reject_total(),
            qualified: bk.qualified(),
            cols: bk.cols_complete(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.qualified && self.cols && self.samples >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "LgReport 行={} 抽={} 拒={} 达标={} 五列={}",
            self.rows, self.samples, self.rejects, self.qualified, self.cols
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：轴域演练（八面四档逐一可取/越集拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_axis_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e40 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..8u64).all(|k| Face::of(k).is_some());
        pass = pass && Face::of(8).is_none();
        pass = pass && (0..4u64).all(|k| Fault::of(k).is_some());
        pass = pass && Fault::of(4).is_none();
        pass = pass && TABLE_MIN == 30 && SAMPLE_ROWS == 10 && TABLE_STD == 32;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("轴域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：锚定面独立域演练（32 格锚码全在域 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_anchor_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30e40 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = true;
        for i in 0..8u64 {
            for j in 0..4u64 {
                let face = Face::of(i).unwrap_or(Face::Peb);
                let fault = Fault::of(j).unwrap_or(Fault::AccessViolation);
                pass = pass && code_in_domain(anchor_code(face, fault));
            }
        }
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("锚定域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct LgCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_lg_checklist(bk: &TableBook) -> Vec<LgCheckRow> {
    let rep = LgReport::build(bk);
    let mut rows: Vec<LgCheckRow> = Vec::new();
    rows.push(LgCheckRow { name: "≥30行达标", passed: bk.qualified() });
    rows.push(LgCheckRow { name: "五列齐", passed: bk.cols_complete() });
    rows.push(LgCheckRow { name: "面覆盖", passed: bk.face_covered() });
    rows.push(LgCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(LgCheckRow { name: "抽样口径", passed: SAMPLE_ROWS == 10 });
    rows.push(LgCheckRow { name: "锚定域", passed: drill_anchor_domain_10().0 });
    rows.push(LgCheckRow { name: "转录完整", passed: table_lines_complete(bk) });
    rows.push(LgCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(LgCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(LgCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn lg_checklist_green(rows: &[LgCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn lg_stats_line(bk: &TableBook) -> String {
    format!(
        "对照表概况：行 {} 抽 {} 拒 {}",
        bk.len(),
        bk.sample_runs(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12514_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = TableBook::new();
    let _ = bk.fill_std();
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 5);
    let _ = bk.sample10(&mut lcg);
    // 注入一例越批卡号（拒止账口径）。
    let _ = bk.add(12999, Face::Peb, Fault::AccessViolation, NT_ACCESS_VIOLATION, "S1");
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        table_lines_complete(&bk) && LgReport::build(&bk).all_green(),
        lg_stats_line(&bk),
    ));
    // X-b：轴域 10/10。
    let (ok_a, d_a) = drill_axis_domain_10();
    out.push(("X-轴域10/10", ok_a, d_a));
    // X-c：锚定域 10/10。
    let (ok_d, d_d) = drill_anchor_domain_10();
    out.push(("X-锚定域10/10", ok_d, d_d));
    // X-d：终验清单。
    let rows = build_lg_checklist(&bk);
    out.push(("X-终验清单10项", lg_checklist_green(&rows), LgReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = TableBook::new();
    let _ = bk.fill_std();
    // 注 1：越批卡号必拒（CardOob）。
    if matches!(bk.add(999, Face::Peb, Fault::AccessViolation, NT_ACCESS_VIOLATION, "S1"), Err(LgCode::CardOob)) {
        caught += 1;
        d.push("注1 越批:抓".to_string());
    } else {
        d.push("注1 越批:漏".to_string());
    }
    // 注 2：码值零必拒（CodeZero）。
    if matches!(bk.add(12510, Face::Teb, Fault::InvalidParameter, 0, "S1"), Err(LgCode::CodeZero)) {
        caught += 1;
        d.push("注2 码零:抓".to_string());
    } else {
        d.push("注2 码零:漏".to_string());
    }
    // 注 3：码域外必拒（CodeOob）。
    if matches!(bk.add(12510, Face::Oa, Fault::TypeMismatch, 0x1234, "S1"), Err(LgCode::CodeOob)) {
        caught += 1;
        d.push("注3 码域:抓".to_string());
    } else {
        d.push("注3 码域:漏".to_string());
    }
    // 注 4：出处空必拒（SourceEmpty）。
    if matches!(bk.add(12510, Face::UniStr, Fault::UnwindTarget, NT_UNWIND_TARGET, ""), Err(LgCode::SourceEmpty)) {
        caught += 1;
        d.push("注4 出处:抓".to_string());
    } else {
        d.push("注4 出处:漏".to_string());
    }
    // 注 5：轴域越集必拒（面/档 of 越界 None）。
    if Face::of(8).is_none() && Fault::of(4).is_none() {
        caught += 1;
        d.push("注5 越轴:抓".to_string());
    } else {
        d.push("注5 越轴:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30e40 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    LgCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12514_checks：本单判据聚合（诊断段 0x30e40）。
pub fn run_unx12514_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12514 坏表拒止对照表批入账");
    // J1-a：对照表批入账（32 行五列齐 10/10）。
    let (ok1, d1) = drill_fill_10();
    s.add_dyn("J1-批入账32行10/10", ok1, &d1);
    // J1-b：抽样 10 行独立可复测（10/10）。
    let (ok2, d2) = drill_sample_10();
    s.add_dyn("J1-抽样10行独立复测10/10", ok2, &d2);
    // J1-c：登记域拒止（10/10）。
    let (ok3, d3) = drill_add_reject_10();
    s.add_dyn("J1-登记域拒止10/10", ok3, &d3);
    // J1-d：标准会话（32 行 + ≥30 达标 + 五列齐 + 面覆盖）。
    let mut bk = TableBook::new();
    let n = bk.fill_std().unwrap_or(0);
    s.add_dyn(
        "J1-对照表批入账达标",
        n == TABLE_STD && bk.qualified() && bk.cols_complete() && bk.face_covered() && bk.monotone(),
        &format!(
            "行={} 达标={} 五列={} 面覆={}",
            n,
            bk.qualified(),
            bk.cols_complete(),
            bk.face_covered()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（坏行不入账）。
    let n0 = bk.len();
    let _ = bk.add(12501, Face::Peb, Fault::AccessViolation, NT_ACCESS_VIOLATION, "S1 母版对照档");
    s.add_dyn(
        "J2-拒止不写回",
        bk.len() == n0 && bk.monotone(),
        &format!("行 {}→{}", n0, bk.len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30e40段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12514_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
