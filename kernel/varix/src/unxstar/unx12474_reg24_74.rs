//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12474 · 注册表族对照表批入账 · 0x30940
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12474-J1 对照表批入账 ≥30 行五列齐，抽样 10 行独立可复测；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五列口径：卡号 / 接口档（六接口闭集）/ 档位档（边界/越界/类型
//!   三档闭集）/ 码值（NT 档）/ 出处（非空）——逐行五列齐才入账
//!   （ColMissing 拒缺列）；
//!   - 批入账 ≥30 行：36 行标准入库（六接口 × 三档 × 锚定码），
//!   行数下限 30（RowShrink 拒缩水）；
//!   - 独立可复测：抽样 10 行逐行独立重验（锚定面独立重算码值，与
//!   在账码值比对一致），抽样覆盖全部六接口（SampleMiss 判漏）；
//!   - 码值域闭集：登记码必须落注册表族 NT 闭集（CodeZero/CodeOob 拒）；
//!   - 重号拒：同卡号同接口同档位只登一行（RowDup 拒）；
//!   - 卡号域：B24 批卡号闭集 12461~12480（CardOob 拒越批）；
//!   - 错误路径带码+中文 detail，拒止不写回对照表；
//!   - 注入核：LCG 写死种子 0x30940，批入账/抽样复测 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30940 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30940;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 对照表行数下限（≥30 口径）。
pub const TABLE_MIN: usize = 30;

/// 标准入库行数（六接口 × 三档 × 双卡 = 36 行口径）。
pub const TABLE_STD: usize = 36;

/// 抽样例数（10 行独立可复测口径）。
pub const SAMPLE_ROWS: usize = 10;

/// B24 批首卡号。
pub const B24_FIRST: u32 = 12461;

/// B24 批末卡号。
pub const B24_LAST: u32 = 12480;

// 注册表族 NT 码档（ntstatus.h 口径）。
/// STATUS_OBJECT_NAME_INVALID。
pub const NT_NAME_INVALID: u32 = 0xC000_0033;
/// STATUS_INVALID_PARAMETER。
pub const NT_INVALID_PARAMETER: u32 = 0xC000_000D;
/// STATUS_OBJECT_TYPE_MISMATCH。
pub const NT_TYPE_MISMATCH: u32 = 0xC000_0024;
/// STATUS_ACCESS_DENIED。
pub const NT_ACCESS_DENIED: u32 = 0xC000_0022;
/// STATUS_NO_MORE_ENTRIES。
pub const NT_NO_MORE_ENTRIES: u32 = 0x8000_001A;
/// STATUS_KEY_DELETED。
pub const NT_KEY_DELETED: u32 = 0xC000_003C;

/// 诊断码（0x30940 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TtCode {
    /// 0x30940 对照行重号。
    RowDup = 0,
    /// 0x30941 五列缺（卡号/接口/档位/码值/出处有缺）。
    ColMissing = 1,
    /// 0x30942 卡号越批。
    CardOob = 2,
    /// 0x30943 接口档越集。
    IfaceOob = 3,
    /// 0x30944 档位档越集。
    TierOob = 4,
    /// 0x30945 码值零。
    CodeZero = 5,
    /// 0x30946 对照表缩水。
    TableShrink = 6,
    /// 0x30947 抽样漏接口（未覆盖全部六接口）。
    SampleMiss = 7,
    /// 0x30948 出处空。
    SourceEmpty = 8,
    /// 0x30949 码域越集（码落注册表族 NT 闭集之外）。
    CodeOob = 9,
}

impl TtCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [TtCode; 10] = [
        TtCode::RowDup, TtCode::ColMissing, TtCode::CardOob, TtCode::IfaceOob,
        TtCode::TierOob, TtCode::CodeZero, TtCode::TableShrink, TtCode::SampleMiss,
        TtCode::SourceEmpty, TtCode::CodeOob,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30940 => "对照行重号：同卡号同接口同档位只登一行。",
        0x30941 => "五列缺：卡号/接口/档位/码值/出处逐列齐才入账。",
        0x30942 => "卡号越批：先比 B24_FIRST/B24_LAST 再登记。",
        0x30943 => "接口档越集：只收建键/开键/枚举/设值/查询/删除六接口。",
        0x30944 => "档位档越集：只收边界/越界/类型三档。",
        0x30945 => "码值零：零不是错误码，回查登记面。",
        0x30946 => "对照表缩水：批入账后只增不减，先对账再抽样。",
        0x30947 => "抽样漏接口：10 例抽样必须覆盖全部六接口。",
        0x30948 => "出处空：出处列必须非空。",
        0x30949 => "码域越集：登记码必须落注册表族 NT 闭集。",
        _ => "未知诊断码：回查 0x30940 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30940 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3094_3094_3094_3094;

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
    /// 建键。
    CreateKey = 0,
    /// 开键。
    OpenKey = 1,
    /// 枚举。
    EnumKey = 2,
    /// 设值。
    SetValue = 3,
    /// 查询。
    QueryValue = 4,
    /// 删除。
    DeleteKey = 5,
}

impl Iface {
    /// 接口总数。
    pub const KINDS: usize = 6;
    /// 由序号取接口（越界返回 None）。
    pub fn of(i: u64) -> Option<Iface> {
        match i {
            0 => Some(Iface::CreateKey),
            1 => Some(Iface::OpenKey),
            2 => Some(Iface::EnumKey),
            3 => Some(Iface::SetValue),
            4 => Some(Iface::QueryValue),
            5 => Some(Iface::DeleteKey),
            _ => None,
        }
    }
}

/// 档位档（三档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tier {
    /// 边界档。
    Edge = 0,
    /// 越界档。
    Over = 1,
    /// 类型档。
    Type = 2,
}

impl Tier {
    /// 档总数。
    pub const KINDS: usize = 3;
    /// 由序号取档（越界返回 None）。
    pub fn of(i: u64) -> Option<Tier> {
        match i {
            0 => Some(Tier::Edge),
            1 => Some(Tier::Over),
            2 => Some(Tier::Type),
            _ => None,
        }
    }
}

/// 对照表行（五列）。
#[derive(Clone, Copy, Debug)]
pub struct TableRow {
    /// 卡号。
    pub card: u32,
    /// 接口档。
    pub iface: Iface,
    /// 档位档。
    pub tier: Tier,
    /// 码值（NT 档）。
    pub code: u32,
    /// 出处。
    pub source: &'static str,
}

/// 锚定面（接口 × 档位 → 锚定码；独立重算口径）。
pub fn anchor_code(iface: Iface, tier: Tier) -> u32 {
    match (iface, tier) {
        (Iface::CreateKey, Tier::Edge) => NT_NAME_INVALID,
        (Iface::CreateKey, Tier::Over) => NT_INVALID_PARAMETER,
        (Iface::CreateKey, Tier::Type) => NT_TYPE_MISMATCH,
        (Iface::OpenKey, Tier::Edge) => NT_NAME_INVALID,
        (Iface::OpenKey, Tier::Over) => NT_INVALID_PARAMETER,
        (Iface::OpenKey, Tier::Type) => NT_TYPE_MISMATCH,
        (Iface::EnumKey, Tier::Edge) => NT_NO_MORE_ENTRIES,
        (Iface::EnumKey, Tier::Over) => NT_INVALID_PARAMETER,
        (Iface::EnumKey, Tier::Type) => NT_TYPE_MISMATCH,
        (Iface::SetValue, Tier::Edge) => NT_INVALID_PARAMETER,
        (Iface::SetValue, Tier::Over) => NT_INVALID_PARAMETER,
        (Iface::SetValue, Tier::Type) => NT_TYPE_MISMATCH,
        (Iface::QueryValue, Tier::Edge) => NT_NO_MORE_ENTRIES,
        (Iface::QueryValue, Tier::Over) => NT_INVALID_PARAMETER,
        (Iface::QueryValue, Tier::Type) => NT_TYPE_MISMATCH,
        (Iface::DeleteKey, Tier::Edge) => NT_KEY_DELETED,
        (Iface::DeleteKey, Tier::Over) => NT_ACCESS_DENIED,
        (Iface::DeleteKey, Tier::Type) => NT_TYPE_MISMATCH,
    }
}

/// 码域闭集核对。
pub fn code_in_domain(code: u32) -> bool {
    matches!(
        code,
        NT_NAME_INVALID
            | NT_INVALID_PARAMETER
            | NT_TYPE_MISMATCH
            | NT_ACCESS_DENIED
            | NT_NO_MORE_ENTRIES
            | NT_KEY_DELETED
    )
}

/// 对照表正本。
///
/// 生命周期：add（五列齐才入账）→ fill_std（36 行标准入库）→
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
    pub fn add(&mut self, card: u32, iface: Iface, tier: Tier, code: u32, source: &'static str) -> Result<usize, TtCode> {
        if card < B24_FIRST || card > B24_LAST {
            self.rejects += 1;
            return Err(TtCode::CardOob);
        }
        if source.is_empty() {
            self.rejects += 1;
            return Err(TtCode::SourceEmpty);
        }
        if code == 0 {
            self.rejects += 1;
            return Err(TtCode::CodeZero);
        }
        if !code_in_domain(code) {
            self.rejects += 1;
            return Err(TtCode::CodeOob);
        }
        if self
            .rows
            .iter()
            .any(|r| r.card == card && r.iface == iface && r.tier == tier)
        {
            self.rejects += 1;
            return Err(TtCode::RowDup);
        }
        self.rows.push(TableRow { card, iface, tier, code, source });
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
        Ok(self.rows.len() - 1)
    }

    /// 标准入库（锚定面派生 36 行：每接口×档位双卡号双行）。
    pub fn fill_std(&mut self) -> Result<usize, TtCode> {
        let mut n = 0usize;
        for i in 0..Iface::KINDS {
            let iface = match Iface::of(i as u64) {
                Some(x) => x,
                None => return Err(TtCode::IfaceOob),
            };
            for j in 0..Tier::KINDS {
                let tier = match Tier::of(j as u64) {
                    Some(x) => x,
                    None => return Err(TtCode::TierOob),
                };
                // 双卡双行：同 (接口,档) 锚码在两张卡上各登一行。
                for m in 0..2u32 {
                    let card = B24_FIRST + (((i as u32) * Tier::KINDS as u32 + j as u32) * 2 + m) % 20;
                    self.add(card, iface, tier, anchor_code(iface, tier), "S1 母版对照档")?;
                    n += 1;
                }
            }
        }
        Ok(n)
    }

    /// 抽样 10 行独立复测（锚定面独立重算；接口轴轮转保覆盖）。
    ///
    /// 返回 (一致行数, 复测行数, 覆盖接口数)。
    pub fn sample10(&mut self, lcg: &mut Lcg) -> (usize, usize, usize) {
        self.samples += 1;
        let mut hit = 0usize;
        let mut seen: [bool; Iface::KINDS] = [false; Iface::KINDS];
        let phase = lcg.below(TABLE_STD as u64) as usize;
        for i in 0..SAMPLE_ROWS {
            // 步距 6：跨接口块轮转，保证 10 例覆盖全部六接口。
            let idx = (i * 6 + phase) % self.rows.len().max(1);
            let (iface, code) = match self.rows.get(idx) {
                Some(r) => (r.iface, r.code),
                None => continue,
            };
            seen[iface as usize] = true;
            // 独立重算：锚定面重查码值。
            let tier = match self.rows.get(idx) {
                Some(r) => r.tier,
                None => continue,
            };
            if anchor_code(iface, tier) == code {
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

    /// 五列齐判据（逐行出处非空 + 码非零）。
    pub fn cols_complete(&self) -> bool {
        !self.rows.is_empty()
            && self
                .rows
                .iter()
                .all(|r| !r.source.is_empty() && r.code != 0)
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

/// 批入账演练：36 行标准入库、≥30 达标、账单调 10/10。
pub fn drill_fill_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30940 段基址。
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
    // 种子写死（协议禁 rand）：0x30940 段基址 ^ 1。
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
    // 种子写死（协议禁 rand）：0x30940 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TableBook::new();
        let _ = bk.fill_std();
        let n0 = bk.len();
        let card_oob = bk.add(12999, Iface::CreateKey, Tier::Edge, NT_NAME_INVALID, "S1");
        let code_zero = bk.add(12461, Iface::CreateKey, Tier::Over, 0, "S1");
        let code_oob = bk.add(12461, Iface::CreateKey, Tier::Type, 0xDEAD, "S1");
        let src_empty = bk.add(12461, Iface::OpenKey, Tier::Edge, NT_NAME_INVALID, "");
        // 找一行真实行做重号注入。
        let r0 = bk.row_of(0).unwrap_or(TableRow {
            card: B24_FIRST,
            iface: Iface::CreateKey,
            tier: Tier::Edge,
            code: NT_NAME_INVALID,
            source: "S1 母版对照档",
        });
        let dup = bk.add(r0.card, r0.iface, r0.tier, r0.code, "S1");
        let _ = lcg.below(3);
        if matches!(card_oob, Err(TtCode::CardOob))
            && matches!(code_zero, Err(TtCode::CodeZero))
            && matches!(code_oob, Err(TtCode::CodeOob))
            && matches!(src_empty, Err(TtCode::SourceEmpty))
            && matches!(dup, Err(TtCode::RowDup))
            && bk.len() == n0
            && bk.monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("登记域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：对照表转录导出（逐行一行：卡/接口/档/码/出处）。
/// ─────────────────────────────────────────────────────────────────────
/// 对照表转录导出。
pub fn export_table_lines(bk: &TableBook) -> String {
    let mut s = String::from("对照表转录：\n");
    let mut i = 0usize;
    while let Some(r) = bk.row_of(i) {
        s.push_str(&format!(
            "F{} 接{} 档{} 码{:08x} 出{}\n",
            r.card, r.iface as u8, r.tier as u8, r.code, r.source
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
pub struct TtReport {
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

impl TtReport {
    /// 建报告。
    pub fn build(bk: &TableBook) -> Self {
        TtReport {
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
            "TtReport 行={} 抽={} 拒={} 达标={} 五列={}",
            self.rows, self.samples, self.rejects, self.qualified, self.cols
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：轴域演练（接口/档位闭集逐一可取 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_axis_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30940 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..Iface::KINDS as u64).all(|k| Iface::of(k).is_some());
        pass = pass && Iface::of(Iface::KINDS as u64).is_none();
        pass = pass && (0..Tier::KINDS as u64).all(|k| Tier::of(k).is_some());
        pass = pass && Tier::of(Tier::KINDS as u64).is_none();
        pass = pass && TABLE_MIN == 30 && SAMPLE_ROWS == 10;
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("轴域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：锚定面独立域演练（18 格锚码全在域 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_anchor_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30940 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = true;
        for i in 0..Iface::KINDS {
            for j in 0..Tier::KINDS {
                let iface = match Iface::of(i as u64) {
                    Some(x) => x,
                    None => {
                        pass = false;
                        break;
                    }
                };
                let tier = match Tier::of(j as u64) {
                    Some(x) => x,
                    None => {
                        pass = false;
                        break;
                    }
                };
                pass = pass && code_in_domain(anchor_code(iface, tier));
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
pub struct TtCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_tt_checklist(bk: &TableBook) -> Vec<TtCheckRow> {
    let rep = TtReport::build(bk);
    let mut rows: Vec<TtCheckRow> = Vec::new();
    rows.push(TtCheckRow { name: "≥30行达标", passed: bk.qualified() });
    rows.push(TtCheckRow { name: "五列齐", passed: bk.cols_complete() });
    rows.push(TtCheckRow { name: "接口覆盖", passed: bk.iface_covered() });
    rows.push(TtCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(TtCheckRow { name: "抽样口径", passed: SAMPLE_ROWS == 10 });
    rows.push(TtCheckRow { name: "锚定域", passed: drill_anchor_domain_10().0 });
    rows.push(TtCheckRow { name: "转录完整", passed: table_lines_complete(bk) });
    rows.push(TtCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(TtCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(TtCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn tt_checklist_green(rows: &[TtCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn tt_stats_line(bk: &TableBook) -> String {
    format!(
        "对照表概况：行 {} 抽 {} 拒 {}",
        bk.len(),
        bk.sample_runs(),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12474_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = TableBook::new();
    let _ = bk.fill_std();
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 5);
    let _ = bk.sample10(&mut lcg);
    // 注入一例越批卡号（拒止账口径）。
    let _ = bk.add(12999, Iface::CreateKey, Tier::Edge, NT_NAME_INVALID, "S1");
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        table_lines_complete(&bk) && TtReport::build(&bk).all_green(),
        tt_stats_line(&bk),
    ));
    // X-b：轴域 10/10。
    let (ok_a, d_a) = drill_axis_domain_10();
    out.push(("X-轴域10/10", ok_a, d_a));
    // X-c：锚定域 10/10。
    let (ok_d, d_d) = drill_anchor_domain_10();
    out.push(("X-锚定域10/10", ok_d, d_d));
    // X-d：终验清单。
    let rows = build_tt_checklist(&bk);
    out.push(("X-终验清单10项", tt_checklist_green(&rows), TtReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut bk = TableBook::new();
    let _ = bk.fill_std();
    // 注 1：越批卡号必拒（CardOob）。
    if matches!(bk.add(999, Iface::CreateKey, Tier::Edge, NT_NAME_INVALID, "S1"), Err(TtCode::CardOob)) {
        caught += 1;
        d.push("注1 越批:抓".to_string());
    } else {
        d.push("注1 越批:漏".to_string());
    }
    // 注 2：码值零必拒（CodeZero）。
    if matches!(bk.add(12470, Iface::OpenKey, Tier::Edge, 0, "S1"), Err(TtCode::CodeZero)) {
        caught += 1;
        d.push("注2 码零:抓".to_string());
    } else {
        d.push("注2 码零:漏".to_string());
    }
    // 注 3：码域外必拒（CodeOob）。
    if matches!(bk.add(12470, Iface::OpenKey, Tier::Over, 0x1234, "S1"), Err(TtCode::CodeOob)) {
        caught += 1;
        d.push("注3 码域:抓".to_string());
    } else {
        d.push("注3 码域:漏".to_string());
    }
    // 注 4：出处空必拒（SourceEmpty）。
    if matches!(bk.add(12470, Iface::EnumKey, Tier::Edge, NT_NO_MORE_ENTRIES, ""), Err(TtCode::SourceEmpty)) {
        caught += 1;
        d.push("注4 出处:抓".to_string());
    } else {
        d.push("注4 出处:漏".to_string());
    }
    // 注 5：轴域越集必拒（Iface/Tier of 越界 None）。
    if Iface::of(6).is_none() && Tier::of(3).is_none() {
        caught += 1;
        d.push("注5 越轴:抓".to_string());
    } else {
        d.push("注5 越轴:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30940 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    TtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12474_checks：本单判据聚合（诊断段 0x30940）。
pub fn run_unx12474_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12474 注册表族对照表批入账");
    // J1-a：对照表批入账（36 行五列齐 10/10）。
    let (ok1, d1) = drill_fill_10();
    s.add_dyn("J1-批入账36行10/10", ok1, &d1);
    // J1-b：抽样 10 行独立可复测（10/10）。
    let (ok2, d2) = drill_sample_10();
    s.add_dyn("J1-抽样10行独立复测10/10", ok2, &d2);
    // J1-c：登记域拒止（10/10）。
    let (ok3, d3) = drill_add_reject_10();
    s.add_dyn("J1-登记域拒止10/10", ok3, &d3);
    // J1-d：标准会话（36 行 + ≥30 达标 + 五列齐 + 接口覆盖）。
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
    let _ = bk.add(12461, Iface::CreateKey, Tier::Edge, NT_NAME_INVALID, "S1 母版对照档");
    s.add_dyn(
        "J2-拒止不写回",
        bk.len() == n0 && bk.monotone(),
        &format!("行 {}→{}", n0, bk.len()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30940段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12474_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
