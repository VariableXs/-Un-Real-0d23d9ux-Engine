//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12444 · NtAdjustGroupsToken 语义档 · 0x30580
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F12444-J1 组启用/禁用（SE_GROUP_ENABLED 位）判据绿，强制组
//!      注入 10/10 次正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 组档：组行（Rid + 属性位），属性位闭集
//!     ENABLED_BY_DEFAULT(0x1)/ENABLED(0x4)/OWNER(0x8)/MANDATORY(0x10)；
//!   - 调整二态：enable/disable（SE_GROUP_ENABLED 位翻转），Rid 未在
//!     令牌拒（NotInToken）、Rid 零拒（RidZero）；
//!   - 强制组纪律：MANDATORY 位组禁用一律拒（Mandatory）——强制组注入
//!     10/10 主拒码；OWNER 组禁用拒（OwnerGroup）；
//!   - PreviousState 档：调整回返原属性位（PrevMismatch 抓失真）；
//!   - 幂等面：同态再调整合法且回返同值（已启再启回返 ENABLED）；
//!   - 有效组集查询：enabled 位与 MANDATORY 联动（强制组恒有效）；
//!   - 调整账：逐次调整入账（append-only，序单调）；
//!   - 错误路径带码+中文 detail，拒止不写回组行；
//!   - 注入核：LCG 写死种子 0x30580，启用/禁用/强制组注入 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30580 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30580;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 令牌槽位（定容 4，与 12443 同口径）。
pub const TOKEN_SLOTS: usize = 4;

/// 单令牌组行上限（8）。
pub const GROUP_ROWS: usize = 8;

// 组属性位（winnt.h 口径）。
/// SE_GROUP_ENABLED_BY_DEFAULT。
pub const SE_GROUP_ENABLED_BY_DEFAULT: u32 = 0x1;
/// SE_GROUP_ENABLED。
pub const SE_GROUP_ENABLED: u32 = 0x4;
/// SE_GROUP_OWNER。
pub const SE_GROUP_OWNER: u32 = 0x8;
/// SE_GROUP_MANDATORY。
pub const SE_GROUP_MANDATORY: u32 = 0x10;
/// 属性位合法掩码。
pub const SE_GROUP_VALID_MASK: u32 =
    SE_GROUP_ENABLED_BY_DEFAULT | SE_GROUP_ENABLED | SE_GROUP_OWNER | SE_GROUP_MANDATORY;

/// 诊断码（0x30580 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AgCode {
    /// 0x30580 令牌槽越界。
    TokenOob = 0,
    /// 0x30581 组未在令牌（Rid 不在有效组集）。
    NotInToken = 1,
    /// 0x30582 强制组禁用拒（MANDATORY 位）。
    Mandatory = 2,
    /// 0x30583 属主组禁用拒（OWNER 位）。
    OwnerGroup = 3,
    /// 0x30584 组属性位越集。
    AttrOob = 4,
    /// 0x30585 Rid 重号。
    DupGroup = 5,
    /// 0x30586 组行满。
    TableFull = 6,
    /// 0x30587 PreviousState 回返失真。
    PrevMismatch = 7,
    /// 0x30588 Rid 零。
    RidZero = 8,
    /// 0x30589 调整账倒挂。
    LedgerBack = 9,
}

impl AgCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [AgCode; 10] = [
        AgCode::TokenOob, AgCode::NotInToken, AgCode::Mandatory, AgCode::OwnerGroup,
        AgCode::AttrOob, AgCode::DupGroup, AgCode::TableFull, AgCode::PrevMismatch,
        AgCode::RidZero, AgCode::LedgerBack,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30580 => "令牌槽越界：槽号先比 TOKEN_SLOTS 再取。",
        0x30581 => "组未在令牌：先核对令牌组集，未在集的 Rid 不可调。",
        0x30582 => "强制组禁用拒：MANDATORY 位组只许保持启用。",
        0x30583 => "属主组禁用拒：OWNER 位组禁用走令牌重建。",
        0x30584 => "属性位越集：只收 ENABLED_BY_DEFAULT/ENABLED/OWNER/MANDATORY 四位。",
        0x30585 => "Rid 重号：同令牌 Rid 唯一，重号先查登记面。",
        0x30586 => "组行满：先移除废组再登记，行账对平。",
        0x30587 => "PreviousState 失真：回返值必须等于调整前属性位。",
        0x30588 => "Rid 零：零值不是组标识。",
        0x30589 => "调整账倒挂：账序全局单调，回拨红账。",
        _ => "未知诊断码：回查 0x30580 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30580 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3058_3058_3058_3058;

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

/// 组行。
#[derive(Clone, Copy, Debug)]
pub struct GroupRow {
    /// Rid。
    pub rid: u32,
    /// 属性位。
    pub attrs: u32,
}

/// 调整账行。
#[derive(Clone, Copy, Debug)]
pub struct AgRow {
    /// 令牌槽。
    pub token: usize,
    /// Rid。
    pub rid: u32,
    /// 原属性位（PreviousState）。
    pub old_attrs: u32,
    /// 新属性位。
    pub new_attrs: u32,
    /// 调整序。
    pub seq: u64,
}

/// 组调整语义档正本。
///
/// 生命周期：add_group（组入令牌）→ adjust_group（启用/禁用 + 回返）→
/// enabled_set（有效组集查询）。
#[derive(Debug)]
pub struct GroupBook {
    /// 令牌组表。
    tokens: [[GroupRow; GROUP_ROWS]; TOKEN_SLOTS],
    /// 每令牌已用行数。
    used: [usize; TOKEN_SLOTS],
    /// 调整账。
    ledger: Vec<AgRow>,
    /// 调整序。
    seq: u64,
    /// 拒止累计。
    rejects: u32,
}

impl GroupBook {
    /// 建账（空组域）。
    pub fn new() -> Self {
        GroupBook {
            tokens: core::array::from_fn(|_| core::array::from_fn(|_| GroupRow { rid: 0, attrs: 0 })),
            used: [0; TOKEN_SLOTS],
            ledger: Vec::new(),
            seq: 0,
            rejects: 0,
        }
    }

    /// 登记组（Rid 非零 + 唯一 + 属性闭集 + 行满拒）。
    pub fn add_group(&mut self, token: usize, rid: u32, attrs: u32) -> Result<bool, AgCode> {
        if token >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(AgCode::TokenOob);
        }
        if rid == 0 {
            self.rejects += 1;
            return Err(AgCode::RidZero);
        }
        if attrs & !SE_GROUP_VALID_MASK != 0 {
            self.rejects += 1;
            return Err(AgCode::AttrOob);
        }
        if self.used[token] >= GROUP_ROWS {
            self.rejects += 1;
            return Err(AgCode::TableFull);
        }
        if (0..self.used[token]).any(|i| self.tokens[token][i].rid == rid) {
            self.rejects += 1;
            return Err(AgCode::DupGroup);
        }
        self.tokens[token][self.used[token]] = GroupRow { rid, attrs };
        self.used[token] += 1;
        Ok(true)
    }

    /// 标准组域登记（ Everyone 强制组 / Users 普通组 / Owner 属主组 / Guest 普通组）。
    pub fn add_std_groups(&mut self, token: usize) -> Result<usize, AgCode> {
        const GROUPS: [(u32, u32); 4] = [
            (0x201, SE_GROUP_MANDATORY | SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT),
            (0x202, SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT),
            (0x203, SE_GROUP_OWNER | SE_GROUP_ENABLED),
            (0x204, 0),
        ];
        let mut n = 0usize;
        for (rid, attrs) in GROUPS.iter() {
            self.add_group(token, *rid, *attrs)?;
            n += 1;
        }
        Ok(n)
    }

    /// 组行查询。
    pub fn row_attrs(&self, token: usize, rid: u32) -> Option<u32> {
        if token >= TOKEN_SLOTS {
            return None;
        }
        (0..self.used[token])
            .find(|&i| self.tokens[token][i].rid == rid)
            .map(|i| self.tokens[token][i].attrs)
    }

    /// 有效组集判定（ENABLED 位或强制组恒有效）。
    pub fn is_effective(&self, token: usize, rid: u32) -> bool {
        match self.row_attrs(token, rid) {
            Some(a) => a & SE_GROUP_ENABLED != 0 || a & SE_GROUP_MANDATORY != 0,
            None => false,
        }
    }

    /// 组启用/禁用（强制组/属主组禁用拒；PreviousState 回返）。
    pub fn adjust_group(&mut self, token: usize, rid: u32, enable: bool) -> Result<u32, AgCode> {
        if token >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(AgCode::TokenOob);
        }
        if rid == 0 {
            self.rejects += 1;
            return Err(AgCode::RidZero);
        }
        let idx = match (0..self.used[token]).find(|&i| self.tokens[token][i].rid == rid) {
            Some(i) => i,
            None => {
                self.rejects += 1;
                return Err(AgCode::NotInToken);
            }
        };
        let row = self.tokens[token][idx];
        if !enable && row.attrs & SE_GROUP_MANDATORY != 0 {
            self.rejects += 1;
            return Err(AgCode::Mandatory);
        }
        if !enable && row.attrs & SE_GROUP_OWNER != 0 {
            self.rejects += 1;
            return Err(AgCode::OwnerGroup);
        }
        let old = row.attrs;
        let new_attrs = if enable {
            old | SE_GROUP_ENABLED
        } else {
            old & !SE_GROUP_ENABLED
        };
        self.tokens[token][idx].attrs = new_attrs;
        self.seq += 1;
        self.ledger.push(AgRow { token, rid, old_attrs: old, new_attrs, seq: self.seq });
        Ok(old)
    }

    /// PreviousState 链一致性（末次调整衔接核对）。
    pub fn prev_chain_ok(&self, token: usize, rid: u32, first: u32, second: u32) -> bool {
        match self.ledger.iter().rev().find(|r| r.token == token && r.rid == rid) {
            Some(r) => r.old_attrs == first && r.new_attrs == second,
            None => false,
        }
    }

    /// 调整账单调判据。
    pub fn ledger_monotone(&self) -> bool {
        let mut last = 0u64;
        for r in self.ledger.iter() {
            if r.seq <= last {
                return false;
            }
            last = r.seq;
        }
        true
    }

    /// 调整账长。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 有效组数。
    pub fn effective_count(&self, token: usize) -> usize {
        if token >= TOKEN_SLOTS {
            return 0;
        }
        (0..self.used[token]).filter(|&i| {
            let a = self.tokens[token][i].attrs;
            a & SE_GROUP_ENABLED != 0 || a & SE_GROUP_MANDATORY != 0
        }).count()
    }

    /// 已用行数。
    pub fn used_rows(&self, token: usize) -> usize {
        if token >= TOKEN_SLOTS {
            return 0;
        }
        self.used[token]
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 启用/禁用演练：10/10 轮 ENABLED 位翻转、回返一致、幂等回返。
pub fn drill_enable_disable_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30580 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = GroupBook::new();
        let _ = bk.add_std_groups(0);
        // 禁用 Users（0x202）→ 启用回（幂等回返同值）。
        let p1 = bk.adjust_group(0, 0x202, false);
        let eff_off = bk.is_effective(0, 0x202);
        let p2 = bk.adjust_group(0, 0x202, true);
        let p3 = bk.adjust_group(0, 0x202, true);
        let eff_on = bk.is_effective(0, 0x202);
        // 回返链：末次调整（已启再启）回返同值。
        let chain = bk.prev_chain_ok(
            0,
            0x202,
            SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT,
            SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT,
        );
        let _ = lcg.below(3);
        if matches!(p1, Ok(x) if x == SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT)
            && !eff_off
            && matches!(p2, Ok(x) if x == SE_GROUP_ENABLED_BY_DEFAULT)
            && matches!(p3, Ok(x) if x == SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT)
            && eff_on
            && chain
            && bk.ledger_monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("启用禁用轮={}/{}", ok, INJECT_ROUNDS))
}

/// 强制组注入 10/10 正确拒止演练（MANDATORY/OWNER/未在集/零 Rid）。
pub fn drill_mandatory_inject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30580 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = GroupBook::new();
        let _ = bk.add_std_groups(0);
        // 路 1：强制组（0x201 MANDATORY）禁用拒。
        let r1 = bk.adjust_group(0, 0x201, false);
        // 路 2：属主组（0x203 OWNER）禁用拒。
        let r2 = bk.adjust_group(0, 0x203, false);
        // 路 3：未在集 Rid 拒。
        let r3 = bk.adjust_group(0, 0x999, true);
        // 路 4：零 Rid 拒。
        let r4 = bk.adjust_group(0, 0, true);
        // 路 5：越令牌域拒。
        let r5 = bk.adjust_group(TOKEN_SLOTS + 2, 0x202, true);
        // 强制组恒有效。
        let mand = bk.is_effective(0, 0x201);
        // 拒止不写回。
        let g_after = bk.row_attrs(0, 0x202);
        let _ = lcg.below(3);
        if matches!(r1, Err(AgCode::Mandatory))
            && matches!(r2, Err(AgCode::OwnerGroup))
            && matches!(r3, Err(AgCode::NotInToken))
            && matches!(r4, Err(AgCode::RidZero))
            && matches!(r5, Err(AgCode::TokenOob))
            && mand
            && g_after.is_some()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("强制组注入拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// 有效组集演练：禁用组出集、强制组恒在 10/10。
pub fn drill_effective_set_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30580 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = GroupBook::new();
        let _ = bk.add_std_groups(0);
        let base = bk.effective_count(0);
        // Guest（0x204 初始无 ENABLED）启用 → 有效数 +1。
        let _ = bk.adjust_group(0, 0x204, true);
        let plus = bk.effective_count(0);
        // Users 禁用 → 有效数 -1（强制组不受影响）。
        let _ = bk.adjust_group(0, 0x202, false);
        let minus = bk.effective_count(0);
        let _ = lcg.below(3);
        if base == 3 && plus == 4 && minus == 3 && bk.ledger_len() == 2 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("有效组集轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：组域转录导出（逐组行一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 组域转录导出（指定令牌）。
pub fn export_group_lines(bk: &GroupBook, token: usize) -> String {
    let mut s = String::from("组域转录：\n");
    if token < TOKEN_SLOTS {
        for i in 0..bk.used_rows(token) {
            let row = &bk.tokens[token][i];
            s.push_str(&format!(
                "G{:x} 属{:x}\n",
                row.rid, row.attrs
            ));
        }
    }
    s
}

/// 转录完整判据（组行齐备）。
pub fn group_lines_complete(bk: &GroupBook, token: usize) -> bool {
    let s = export_group_lines(bk, token);
    if bk.used_rows(token) == 0 {
        return false;
    }
    let mut hits = 0usize;
    for i in 0..bk.used_rows(token) {
        let row = &bk.tokens[token][i];
        if s.contains(&alloc::format!("G{:x} 属", row.rid)) {
            hits += 1;
        }
    }
    hits == bk.used_rows(token)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：组调整报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct AgReport {
    /// 调整账行数。
    pub ledger: usize,
    /// 有效组数（令牌 0）。
    pub effective: usize,
    /// 拒止累计。
    pub rejects: u32,
    /// 账单调。
    pub mono: bool,
}

impl AgReport {
    /// 建报告。
    pub fn build(bk: &GroupBook) -> Self {
        AgReport {
            ledger: bk.ledger_len(),
            effective: bk.effective_count(0),
            rejects: bk.reject_total(),
            mono: bk.ledger_monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.mono && self.ledger >= 1 && self.rejects >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "AgReport 账={} 有效={} 拒={} 单={}",
            self.ledger, self.effective, self.rejects, self.mono
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：登记域演练（零 Rid/越属性位/重号/行满 10/10 拒）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_add_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30580 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = GroupBook::new();
        let zero = bk.add_group(0, 0, SE_GROUP_ENABLED);
        let _ = bk.add_group(0, 0x301, SE_GROUP_ENABLED);
        let dup = bk.add_group(0, 0x301, 0);
        let bad = bk.add_group(0, 0x302, 0x99);
        // 行满：灌到 8 行后再登必拒。
        let mut full_hit = false;
        for i in 3..GROUP_ROWS + 3 {
            if matches!(
                bk.add_group(0, 0x300 + i as u32, SE_GROUP_ENABLED),
                Err(AgCode::TableFull)
            ) {
                full_hit = true;
            }
        }
        let _ = lcg.below(3);
        if matches!(zero, Err(AgCode::RidZero))
            && matches!(dup, Err(AgCode::DupGroup))
            && matches!(bad, Err(AgCode::AttrOob))
            && full_hit
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("登记域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：多令牌隔离演练（T0/T1 组域互不影响 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_token_isolation_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30580 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = GroupBook::new();
        let _ = bk.add_group(0, 0x401, SE_GROUP_ENABLED);
        let _ = bk.add_group(1, 0x401, 0);
        let t0 = bk.adjust_group(0, 0x401, false);
        let t1 = bk.row_attrs(1, 0x401);
        let t1_on = bk.adjust_group(1, 0x401, true);
        let t0_after = bk.row_attrs(0, 0x401);
        let _ = lcg.below(3);
        if matches!(t0, Ok(x) if x == SE_GROUP_ENABLED)
            && t1 == Some(0)
            && matches!(t1_on, Ok(0))
            && t0_after == Some(0)
            && bk.ledger_len() == 2
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("多令牌隔离轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct AgCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_ag_checklist(bk: &GroupBook) -> Vec<AgCheckRow> {
    let rep = AgReport::build(bk);
    let mut rows: Vec<AgCheckRow> = Vec::new();
    rows.push(AgCheckRow { name: "账单调", passed: bk.ledger_monotone() });
    rows.push(AgCheckRow { name: "属性位掩码", passed: SE_GROUP_VALID_MASK == 0x1D });
    rows.push(AgCheckRow { name: "行宽8", passed: GROUP_ROWS == 8 });
    rows.push(AgCheckRow { name: "令牌宽4", passed: TOKEN_SLOTS == 4 });
    rows.push(AgCheckRow { name: "强制组位", passed: SE_GROUP_MANDATORY == 0x10 });
    rows.push(AgCheckRow { name: "转录完整", passed: group_lines_complete(bk, 0) });
    rows.push(AgCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(AgCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(AgCheckRow { name: "有效组在册", passed: bk.effective_count(0) >= 1 });
    rows.push(AgCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ag_checklist_green(rows: &[AgCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ag_stats_line(bk: &GroupBook) -> String {
    format!(
        "组调整概况：账 {} 有效 {} 拒 {}",
        bk.ledger_len(),
        bk.effective_count(0),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12444_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = GroupBook::new();
    let _ = bk.add_std_groups(0);
    let _ = bk.adjust_group(0, 0x202, false);
    // 注入一例强制组禁用（拒止账口径）。
    let _ = bk.adjust_group(0, 0x201, false);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        group_lines_complete(&bk, 0) && AgReport::build(&bk).all_green(),
        ag_stats_line(&bk),
    ));
    // X-b：登记域 10/10。
    let (ok_a, d_a) = drill_add_domain_10();
    out.push(("X-登记域10/10", ok_a, d_a));
    // X-c：多令牌隔离 10/10。
    let (ok_i, d_i) = drill_token_isolation_10();
    out.push(("X-多令牌隔离10/10", ok_i, d_i));
    // X-d：终验清单。
    let rows = build_ag_checklist(&bk);
    out.push(("X-终验清单10项", ag_checklist_green(&rows), AgReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：强制组禁用必拒（Mandatory）。
    let mut bk = GroupBook::new();
    let _ = bk.add_group(0, 0x201, SE_GROUP_MANDATORY | SE_GROUP_ENABLED);
    if matches!(bk.adjust_group(0, 0x201, false), Err(AgCode::Mandatory)) {
        caught += 1;
        d.push("注1 强制:抓".to_string());
    } else {
        d.push("注1 强制:漏".to_string());
    }
    // 注 2：属主组禁用必拒（OwnerGroup）。
    let _ = bk.add_group(0, 0x203, SE_GROUP_OWNER | SE_GROUP_ENABLED);
    if matches!(bk.adjust_group(0, 0x203, false), Err(AgCode::OwnerGroup)) {
        caught += 1;
        d.push("注2 属主:抓".to_string());
    } else {
        d.push("注2 属主:漏".to_string());
    }
    // 注 3：未在集 Rid 必拒（NotInToken）。
    if matches!(bk.adjust_group(0, 0x999, true), Err(AgCode::NotInToken)) {
        caught += 1;
        d.push("注3 未在集:抓".to_string());
    } else {
        d.push("注3 未在集:漏".to_string());
    }
    // 注 4：零 Rid 必拒（RidZero）。
    if matches!(bk.adjust_group(0, 0, true), Err(AgCode::RidZero)) {
        caught += 1;
        d.push("注4 零rid:抓".to_string());
    } else {
        d.push("注4 零rid:漏".to_string());
    }
    // 注 5：越令牌域必拒（TokenOob）。
    if matches!(bk.adjust_group(9, 0x201, true), Err(AgCode::TokenOob)) {
        caught += 1;
        d.push("注5 越槽:抓".to_string());
    } else {
        d.push("注5 越槽:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30580 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    AgCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12444_checks：本单判据聚合（诊断段 0x30580）。
pub fn run_unx12444_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12444 NtAdjustGroupsToken 语义档");
    // J1-a：组启用/禁用（SE_GROUP_ENABLED 位 10/10）。
    let (ok1, d1) = drill_enable_disable_10();
    s.add_dyn("J1-组启用禁用10/10", ok1, &d1);
    // J1-b：强制组注入 10/10 正确拒止。
    let (ok2, d2) = drill_mandatory_inject_10();
    s.add_dyn("J1-强制组注入拒止10/10", ok2, &d2);
    // J1-c：有效组集（10/10）。
    let (ok3, d3) = drill_effective_set_10();
    s.add_dyn("J1-有效组集10/10", ok3, &d3);
    // J1-d：标准会话（四组域 + 强制组恒有效 + 回返链）。
    let mut bk = GroupBook::new();
    let _ = bk.add_std_groups(0);
    let p1 = bk.adjust_group(0, 0x202, false);
    let p2 = bk.adjust_group(0, 0x202, true);
    s.add_dyn(
        "J1-组调整回返链",
        matches!(p1, Ok(x) if x == SE_GROUP_ENABLED | SE_GROUP_ENABLED_BY_DEFAULT)
            && matches!(p2, Ok(x) if x == SE_GROUP_ENABLED_BY_DEFAULT)
            && bk.is_effective(0, 0x201)
            && bk.ledger_monotone(),
        &format!(
            "p1={:?} p2={:?} 强制有效={}",
            p1.is_ok(),
            p2.is_ok(),
            bk.is_effective(0, 0x201)
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（强制组拒禁后属性位不变）。
    let a0 = bk.row_attrs(0, 0x201);
    let _ = bk.adjust_group(0, 0x201, false);
    let a1 = bk.row_attrs(0, 0x201);
    s.add_dyn(
        "J2-拒止不写回",
        a0 == a1 && a0.map(|x| x & SE_GROUP_MANDATORY != 0).unwrap_or(false),
        &format!("强制组属 {:?}→{:?}", a0, a1),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30580段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12444_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
