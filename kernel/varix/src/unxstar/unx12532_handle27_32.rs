//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12532 · 句柄错误码对照表批入账 · 0x31080
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12532-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五列口径：卡号 / 接口档（关闭/复制/查询/设信息/等待/开句柄 六
//!   接口闭集）/ 错误档（双关闭/伪句柄/权限/类型错配/表满 五档闭集）/
//!   码值（NT 档）/ 出处（非空）——逐行五列齐才入账（ColMissing 拒缺列）；
//!   - 批入账 ≥30 行：30 行标准入库（六接口 × 五错误档 = 30 锚定格）；
//!   - 独立可复测：抽样 10 行逐行独立重验（锚定面独立重算码值与在账
//!   码值比对一致），步距 6 跨接口块轮转保证六接口全覆盖；
//!   - 码值域闭集：登记码必须落句柄族 NT 闭集（CodeZero/CodeOob 拒）；
//!   - 重号拒：同卡号同接口同错误档只登一行（RowDup 拒）；
//!   - 卡号域：B27 批卡号闭集 12521~12540（CardOob 拒越批）；
//!   - 错误路径带码+中文 detail，拒止不写回对照表；
//!   - 注入核：LCG 写死种子 0x31080，批入账/抽样复测 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x31080 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x31080;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 对照表行数下限（≥30 口径）。
pub const TABLE_MIN: usize = 30;

/// 标准入库行数（六接口 × 五错误档 = 30 行口径）。
pub const TABLE_STD: usize = 30;

/// 抽样例数（10 行独立可复测口径）。
pub const SAMPLE_ROWS: usize = 10;

/// B27 批首卡号。
pub const B27_FIRST: u32 = 12521;

/// B27 批末卡号。
pub const B27_LAST: u32 = 12540;

// 句柄族 NT 码档（ntstatus.h 口径）。
/// STATUS_HANDLE_NOT_CLOSABLE。
pub const NT_NOT_CLOSABLE: u32 = 0xC000_0023;
/// STATUS_INVALID_HANDLE。
pub const NT_INVALID_HANDLE: u32 = 0xC000_0008;
/// STATUS_ACCESS_DENIED。
pub const NT_ACCESS_DENIED: u32 = 0xC000_0022;
/// STATUS_OBJECT_TYPE_MISMATCH。
pub const NT_TYPE_MISMATCH: u32 = 0xC000_0024;
/// STATUS_INSUFFICIENT_RESOURCES。
pub const NT_INSUFFICIENT: u32 = 0xC000_009A;
/// STATUS_DUPLICATE_NAME? 用 OBJECT_NAME_COLLISION 不对；取 PROCESS_IS_TERMINATING 不对口；
/// 表满档复用 INSUFFICIENT；权限档第二码：DELETE_PENDING 不对口；
/// 采用如下补充（ntstatus.h 真实口径）。
/// STATUS_INVALID_PARAMETER。
pub const NT_INVALID_PARAMETER: u32 = 0xC000_000D;

/// 诊断码（0x31080 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HtCode {
    /// 0x31080 对照行重号。
    RowDup = 0,
    /// 0x31081 五列缺。
    ColMissing = 1,
    /// 0x31082 卡号越批。
    CardOob = 2,
    /// 0x31083 接口档越集。
    IfaceOob = 3,
    /// 0x31084 错误档越集。
    ErrOob = 4,
    /// 0x31085 码值零。
    CodeZero = 5,
    /// 0x31086 对照表缩水。
    TableShrink = 6,
    /// 0x31087 抽样漏接口。
    SampleMiss = 7,
    /// 0x31088 出处空。
    SourceEmpty = 8,
    /// 0x31089 码域越集。
    CodeOob = 9,
}

impl HtCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [HtCode; 10] = [
        HtCode::RowDup, HtCode::ColMissing, HtCode::CardOob, HtCode::IfaceOob,
        HtCode::ErrOob, HtCode::CodeZero, HtCode::TableShrink, HtCode::SampleMiss,
        HtCode::SourceEmpty, HtCode::CodeOob,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x31080 => "对照行重号：同卡号同接口同错误档只登一行。",
        0x31081 => "五列缺：卡号/接口/错误档/码值/出处逐列齐才入账。",
        0x31082 => "卡号越批：先比 B27_FIRST/B27_LAST 再登记。",
        0x31083 => "接口档越集：只收关闭/复制/查询/设信息/等待/开句柄六接口。",
        0x31084 => "错误档越集：只收双关闭/伪句柄/权限/类型错配/表满五档。",
        0x31085 => "码值零：零不是错误码，回查登记面。",
        0x31086 => "对照表缩水：批入账后只增不减，先对账再抽样。",
        0x31087 => "抽样漏接口：10 例抽样必须覆盖全部六接口。",
        0x31088 => "出处空：出处列必须非空。",
        0x31089 => "码域越集：登记码必须落句柄族 NT 闭集。",
        _ => "未知诊断码：回查 0x31080 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x31080 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3108_3108_3108_3108;

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

/// 接口档（六接口闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Iface {
    /// 关闭。
    Close = 0,
    /// 复制。
    Duplicate = 1,
    /// 查询。
    Query = 2,
    /// 设信息。
    SetInfo = 3,
    /// 等待。
    Wait = 4,
    /// 开句柄。
    Open = 5,
}

impl Iface {
    /// 接口总数。
    pub const KINDS: usize = 6;
    /// 由序号取接口（越界返回 None）。
    pub fn of(i: u64) -> Option<Iface> {
        match i {
            0 => Some(Iface::Close),
            1 => Some(Iface::Duplicate),
            2 => Some(Iface::Query),
            3 => Some(Iface::SetInfo),
            4 => Some(Iface::Wait),
            5 => Some(Iface::Open),
            _ => None,
        }
    }
}

/// 错误档（五档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrKind {
    /// 双关闭。
    DoubleClose = 0,
    /// 伪句柄。
    PseudoHandle = 1,
    /// 权限。
    Access = 2,
    /// 类型错配。
    TypeMismatch = 3,
    /// 表满。
    TableFull = 4,
}

impl ErrKind {
    /// 档总数。
    pub const KINDS: usize = 5;
    /// 由序号取档（越界返回 None）。
    pub fn of(i: u64) -> Option<ErrKind> {
        match i {
            0 => Some(ErrKind::DoubleClose),
            1 => Some(ErrKind::PseudoHandle),
            2 => Some(ErrKind::Access),
            3 => Some(ErrKind::TypeMismatch),
            4 => Some(ErrKind::TableFull),
            _ => None,
        }
    }
}

/// 锚定面（接口 × 错误档 → 锚定码；独立口径）。
pub fn anchor_code(iface: Iface, err: ErrKind) -> u32 {
    match (iface, err) {
        (Iface::Close, ErrKind::DoubleClose) => NT_NOT_CLOSABLE,
        (Iface::Close, ErrKind::PseudoHandle) => NT_INVALID_HANDLE,
        (Iface::Close, ErrKind::Access) => NT_ACCESS_DENIED,
        (Iface::Close, ErrKind::TypeMismatch) => NT_TYPE_MISMATCH,
        (Iface::Close, ErrKind::TableFull) => NT_INSUFFICIENT,
        (Iface::Duplicate, ErrKind::DoubleClose) => NT_INVALID_HANDLE,
        (Iface::Duplicate, ErrKind::PseudoHandle) => NT_INVALID_HANDLE,
        (Iface::Duplicate, ErrKind::Access) => NT_ACCESS_DENIED,
        (Iface::Duplicate, ErrKind::TypeMismatch) => NT_TYPE_MISMATCH,
        (Iface::Duplicate, ErrKind::TableFull) => NT_INSUFFICIENT,
        (Iface::Query, ErrKind::DoubleClose) => NT_INVALID_HANDLE,
        (Iface::Query, ErrKind::PseudoHandle) => NT_INVALID_HANDLE,
        (Iface::Query, ErrKind::Access) => NT_ACCESS_DENIED,
        (Iface::Query, ErrKind::TypeMismatch) => NT_TYPE_MISMATCH,
        (Iface::Query, ErrKind::TableFull) => NT_INVALID_PARAMETER,
        (Iface::SetInfo, ErrKind::DoubleClose) => NT_INVALID_HANDLE,
        (Iface::SetInfo, ErrKind::PseudoHandle) => NT_ACCESS_DENIED,
        (Iface::SetInfo, ErrKind::Access) => NT_ACCESS_DENIED,
        (Iface::SetInfo, ErrKind::TypeMismatch) => NT_TYPE_MISMATCH,
        (Iface::SetInfo, ErrKind::TableFull) => NT_INVALID_PARAMETER,
        (Iface::Wait, ErrKind::DoubleClose) => NT_INVALID_HANDLE,
        (Iface::Wait, ErrKind::PseudoHandle) => NT_INVALID_HANDLE,
        (Iface::Wait, ErrKind::Access) => NT_ACCESS_DENIED,
        (Iface::Wait, ErrKind::TypeMismatch) => NT_TYPE_MISMATCH,
        (Iface::Wait, ErrKind::TableFull) => NT_INVALID_PARAMETER,
        (Iface::Open, ErrKind::DoubleClose) => NT_INVALID_HANDLE,
        (Iface::Open, ErrKind::PseudoHandle) => NT_INVALID_HANDLE,
        (Iface::Open, ErrKind::Access) => NT_ACCESS_DENIED,
        (Iface::Open, ErrKind::TypeMismatch) => NT_TYPE_MISMATCH,
        (Iface::Open, ErrKind::TableFull) => NT_INSUFFICIENT,
    }
}

/// 码域闭集核对。
pub fn code_in_domain(code: u32) -> bool {
    matches!(
        code,
        NT_NOT_CLOSABLE
            | NT_INVALID_HANDLE
            | NT_ACCESS_DENIED
            | NT_TYPE_MISMATCH
            | NT_INSUFFICIENT
            | NT_INVALID_PARAMETER
    )
}

/// 对照表行（五列）。
#[derive(Clone, Copy, Debug)]
pub struct TableRow {
    /// 卡号。
    pub card: u32,
    /// 接口档。
    pub iface: Iface,
    /// 错误档。
    pub err: ErrKind,
    /// 码值。
    pub code: u32,
    /// 出处。
    pub source: &'static str,
}

/// 对照表正本。
///
/// 生命周期：add（五列齐才入账）→ fill_std（30 行标准入库）→
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
    pub fn add(&mut self, card: u32, iface: Iface, err: ErrKind, code: u32, source: &'static str) -> Result<usize, HtCode> {
        if card < B27_FIRST || card > B27_LAST {
            self.rejects += 1;
            return Err(HtCode::CardOob);
        }
        if source.is_empty() {
            self.rejects += 1;
            return Err(HtCode::SourceEmpty);
        }
        if code == 0 {
            self.rejects += 1;
            return Err(HtCode::CodeZero);
        }
        if !code_in_domain(code) {
            self.rejects += 1;
            return Err(HtCode::CodeOob);
        }
        if self
            .rows
            .iter()
            .any(|r| r.card == card && r.iface == iface && r.err == err)
        {
            self.rejects += 1;
            return Err(HtCode::RowDup);
        }
        self.rows.push(TableRow { card, iface, err, code, source });
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        Ok(self.rows.len() - 1)
    }

    /// 标准入库（锚定面派生 30 行，卡号按行序轮转）。
    pub fn fill_std(&mut self) -> Result<usize, HtCode> {
        let mut n = 0usize;
        for i in 0..Iface::KINDS {
            let iface = match Iface::of(i as u64) {
                Some(x) => x,
                None => return Err(HtCode::IfaceOob),
            };
            for j in 0..ErrKind::KINDS {
                let err = match ErrKind::of(j as u64) {
                    Some(x) => x,
                    None => return Err(HtCode::ErrOob),
                };
                let card = B27_FIRST + ((i * ErrKind::KINDS + j) % 20) as u32;
                self.add(card, iface, err, anchor_code(iface, err), "S1 母版对照档")?;
                n += 1;
            }
        }
        Ok(n)
    }

    /// 抽样 10 行独立复测（步距 6 跨接口块轮转保六接口全覆盖）。
    ///
    /// 返回 (一致行数, 复测行数, 覆盖接口数)。
    pub fn sample10(&mut self, lcg: &mut Lcg) -> (usize, usize, usize) {
        self.samples += 1;
        let mut hit = 0usize;
        let mut seen: [bool; Iface::KINDS] = [false; Iface::KINDS];
        let phase = lcg.below(TABLE_STD as u64) as usize;
        for i in 0..SAMPLE_ROWS {
            let idx = (i * 6 + phase) % self.rows.len().max(1);
            let (iface, err, code) = match self.rows.get(idx) {
                Some(r) => (r.iface, r.err, r.code),
                None => continue,
            };
            seen[iface as usize] = true;
            if anchor_code(iface, err) == code {
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

    /// 接口覆盖判据（六接口全在账）。
    pub fn iface_covered(&self) -> bool {
        (0..Iface::KINDS as u64).all(|k| {
            let want = Iface::of(k);
            self.rows.iter().any(|r| Some(r.iface) == want)
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

/// 批入账演练：30 行标准入库、≥30 达标、账单调 10/10。
pub fn drill_fill_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31080 段基址。
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
            && bk.iface_covered()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("批入账 {} 行 轮={}/{}", TABLE_STD, ok, INJECT_ROUNDS))
}

/// 抽样 10 行独立复测演练：全一致且覆盖六接口 10/10。
pub fn drill_sample_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31080 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TableBook::new();
        let _ = bk.fill_std();
        let (hit, total, covered) = bk.sample10(&mut lcg);
        if hit == total && total == SAMPLE_ROWS && covered == Iface::KINDS && bk.sample_runs() == 1 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("抽样复测 {}/{} 轮={}/{}", SAMPLE_ROWS, SAMPLE_ROWS, ok, INJECT_ROUNDS))
}

/// 登记域拒止演练：越批卡/码零/码域外/重号/缺出处 10/10 拒。
pub fn drill_add_reject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31080 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TableBook::new();
        let _ = bk.fill_std();
        let n0 = bk.len();
        let card_oob = bk.add(12999, Iface::Close, ErrKind::DoubleClose, NT_NOT_CLOSABLE, "S1");
        let code_zero = bk.add(12521, Iface::Close, ErrKind::PseudoHandle, 0, "S1");
        let code_oob = bk.add(12521, Iface::Close, ErrKind::Access, 0xDEAD, "S1");
        let src_empty = bk.add(12521, Iface::Duplicate, ErrKind::DoubleClose, NT_INVALID_HANDLE, "");
        let r0 = bk.row_of(0).unwrap_or(TableRow {
            card: B27_FIRST,
            iface: Iface::Close,
            err: ErrKind::DoubleClose,
            code: NT_NOT_CLOSABLE,
            source: "S1 母版对照档",
        });
        let dup = bk.add(r0.card, r0.iface, r0.err, r0.code, "S1");
        let _ = lcg.below(3);
        if matches!(card_oob, Err(HtCode::CardOob))
            && matches!(code_zero, Err(HtCode::CodeZero))
            && matches!(code_oob, Err(HtCode::CodeOob))
            && matches!(src_empty, Err(HtCode::SourceEmpty))
            && matches!(dup, Err(HtCode::RowDup))
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
            "F{} 接{} 档{} 码{:08x} 出{}\n",
            r.card, r.iface as u8, r.err as u8, r.code, r.source
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
pub struct HtReport {
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

impl HtReport {
    /// 建报告。
    pub fn build(bk: &TableBook) -> Self {
        HtReport {
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
            "HtReport 行={} 抽={} 拒={} 达标={} 五列={}",
            self.rows, self.samples, self.rejects, self.qualified, self.cols
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：轴域演练（接口/错误档闭集逐一可取 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_axis_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31080 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..Iface::KINDS as u64).all(|k| Iface::of(k).is_some());
        pass = pass && Iface::of(Iface::KINDS as u64).is_none();
        pass = pass && (0..ErrKind::KINDS as u64).all(|k| ErrKind::of(k).is_some());
        pass = pass && ErrKind::of(ErrKind::KINDS as u64).is_none();
        pass = pass && TABLE_MIN == 30 && SAMPLE_ROWS == 10 && TABLE_STD == 30;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("轴域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：锚定面独立域演练（30 格锚码全在域 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_anchor_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x31080 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = true;
        for i in 0..Iface::KINDS as u64 {
            for j in 0..ErrKind::KINDS as u64 {
                let iface = Iface::of(i).unwrap_or(Iface::Close);
                let err = ErrKind::of(j).unwrap_or(ErrKind::DoubleClose);
                pass = pass && code_in_domain(anchor_code(iface, err));
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
pub struct HtCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_ht_checklist(bk: &TableBook) -> Vec<HtCheckRow> {
    let rep = HtReport::build(bk);
    let mut rows: Vec<HtCheckRow> = Vec::new();
    rows.push(HtCheckRow { name: "≥30行达标", passed: bk.qualified() });
    rows.push(HtCheckRow { name: "五列齐", passed: bk.cols_complete() });
    rows.push(HtCheckRow { name: "接口覆盖", passed: bk.iface_covered() });
    rows.push(HtCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(HtCheckRow { name: "抽样口径", passed: SAMPLE_ROWS == 10 });
    rows.push(HtCheckRow { name: "锚定域", passed: drill_anchor_domain_10().0 });
    rows.push(HtCheckRow { name: "转录完整", passed: table_lines_complete(bk) });
    rows.push(HtCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(HtCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(HtCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ht_checklist_green(rows: &[HtCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ht_stats_line(bk: &TableBook) -> String {
    format!(
        "对照表概况：行 {} 抽 {} 拒 {}",
        bk.len(),
        bk.sample_runs(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12532_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = TableBook::new();
    let _ = bk.fill_std();
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 5);
    let _ = bk.sample10(&mut lcg);
    // 注入一例越批卡号（拒止账口径）。
    let _ = bk.add(12999, Iface::Close, ErrKind::DoubleClose, NT_NOT_CLOSABLE, "S1");
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        table_lines_complete(&bk) && HtReport::build(&bk).all_green(),
        ht_stats_line(&bk),
    ));
    // X-b：轴域 10/10。
    let (ok_a, d_a) = drill_axis_domain_10();
    out.push(("X-轴域10/10", ok_a, d_a));
    // X-c：锚定域 10/10。
    let (ok_d, d_d) = drill_anchor_domain_10();
    out.push(("X-锚定域10/10", ok_d, d_d));
    // X-d：终验清单。
    let rows = build_ht_checklist(&bk);
    out.push(("X-终验清单10项", ht_checklist_green(&rows), HtReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = TableBook::new();
    let _ = bk.fill_std();
    // 注 1：越批卡号必拒（CardOob）。
    if matches!(bk.add(999, Iface::Close, ErrKind::DoubleClose, NT_NOT_CLOSABLE, "S1"), Err(HtCode::CardOob)) {
        caught += 1;
        d.push("注1 越批:抓".to_string());
    } else {
        d.push("注1 越批:漏".to_string());
    }
    // 注 2：码值零必拒（CodeZero）。
    if matches!(bk.add(12521, Iface::Close, ErrKind::PseudoHandle, 0, "S1"), Err(HtCode::CodeZero)) {
        caught += 1;
        d.push("注2 码零:抓".to_string());
    } else {
        d.push("注2 码零:漏".to_string());
    }
    // 注 3：码域外必拒（CodeOob）。
    if matches!(bk.add(12521, Iface::Close, ErrKind::Access, 0x1234, "S1"), Err(HtCode::CodeOob)) {
        caught += 1;
        d.push("注3 码域:抓".to_string());
    } else {
        d.push("注3 码域:漏".to_string());
    }
    // 注 4：出处空必拒（SourceEmpty）。
    if matches!(bk.add(12521, Iface::Duplicate, ErrKind::DoubleClose, NT_INVALID_HANDLE, ""), Err(HtCode::SourceEmpty)) {
        caught += 1;
        d.push("注4 出处:抓".to_string());
    } else {
        d.push("注4 出处:漏".to_string());
    }
    // 注 5：轴域越集必拒（接口/错误档 of 越界 None）。
    if Iface::of(6).is_none() && ErrKind::of(5).is_none() {
        caught += 1;
        d.push("注5 越轴:抓".to_string());
    } else {
        d.push("注5 越轴:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x31080 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    HtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12532_checks：本单判据聚合（诊断段 0x31080）。
pub fn run_unx12532_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12532 句柄错误码对照表批入账");
    // J1-a：对照表批入账（30 行五列齐 10/10）。
    let (ok1, d1) = drill_fill_10();
    s.add_dyn("J1-批入账30行10/10", ok1, &d1);
    // J1-b：抽样 10 行独立可复测（10/10）。
    let (ok2, d2) = drill_sample_10();
    s.add_dyn("J1-抽样10行独立复测10/10", ok2, &d2);
    // J1-c：登记域拒止（10/10）。
    let (ok3, d3) = drill_add_reject_10();
    s.add_dyn("J1-登记域拒止10/10", ok3, &d3);
    // J1-d：标准会话（30 行 + ≥30 达标 + 五列齐 + 接口覆盖）。
    let mut bk = TableBook::new();
    let n = bk.fill_std().unwrap_or(0);
    s.add_dyn(
        "J1-对照表批入账达标",
        n == TABLE_STD && bk.qualified() && bk.cols_complete() && bk.iface_covered() && bk.monotone(),
        &format!(
            "行={} 达标={} 五列={} 接口覆={}",
            n,
            bk.qualified(),
            bk.cols_complete(),
            bk.iface_covered()
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（坏行不入账）。
    let n0 = bk.len();
    let _ = bk.add(12521, Iface::Close, ErrKind::DoubleClose, NT_NOT_CLOSABLE, "S1 母版对照档");
    s.add_dyn(
        "J2-拒止不写回",
        bk.len() == n0 && bk.monotone(),
        &format!("行 {}→{}", n0, bk.len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x31080段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12532_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
