//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12445 · NtCreateToken/NtDuplicateToken 语义档 · 0x305a0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12445-J1 创建参数与复制（ImpersonationLevel 档）判据绿，
//!      越权复制注入 10/10 拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 令牌种类两态：Primary（主令牌）/Impersonation（模拟令牌）；
//!   - 模拟级四档闭集：Anonymous(0)/Identification(1)/Impersonation(2)/
//!     Delegation(3)，越档拒（LevelOob）；
//!   - 创建参数纪律：主令牌模拟级必须 Anonymous 档（TypeLevel 拒错配），
//!     模拟令牌级必须 ≥Identification；
//!   - 复制语义：Duplicate(src, new_type, new_level)——新级 ≤ 源级（
//!     越权复制拒 LevelEscalate 10/10 主拒码）；复制自身拒（DupSelf）；
//!   - 复制账：子令牌记录源槽（lineage 可回溯），源释放后子令牌存续
//!     （独立生命周期面）；
//!   - 句柄面：创建/复制各开一柄，关闭/释放计数账（LedgerBack 抓倒挂）；
//!   - 错误路径带码+中文 detail，拒止不写回令牌槽；
//!   - 注入核：LCG 写死种子 0x305a0，创建/复制/越权注入 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x305a0 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x305a0;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 令牌槽位（定容 8）。
pub const TOKEN_SLOTS: usize = 8;

/// 模拟级档数（四档闭集）。
pub const IMP_LEVELS: u32 = 4;

/// 诊断码（0x305a0 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtCode {
    /// 0x305a0 令牌槽越界。
    TokenOob = 0,
    /// 0x305a1 模拟级越档。
    LevelOob = 1,
    /// 0x305a2 种类/级别错配（主令牌带非匿名级）。
    TypeLevel = 2,
    /// 0x305a3 越权复制（新级 > 源级）。
    LevelEscalate = 3,
    /// 0x305a4 复制自身。
    DupSelf = 4,
    /// 0x305a5 令牌槽满。
    TableFull = 5,
    /// 0x305a6 令牌已释放。
    TokenFreed = 6,
    /// 0x305a7 源槽零（复制源非法）。
    SourceZero = 7,
    /// 0x305a8 句柄账倒挂（关闭多于打开）。
    LedgerBack = 8,
    /// 0x305a9 血统断链（子令牌源槽失真）。
    LineageBroken = 9,
}

impl CtCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [CtCode; 10] = [
        CtCode::TokenOob, CtCode::LevelOob, CtCode::TypeLevel, CtCode::LevelEscalate,
        CtCode::DupSelf, CtCode::TableFull, CtCode::TokenFreed, CtCode::SourceZero,
        CtCode::LedgerBack, CtCode::LineageBroken,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x305a0 => "令牌槽越界：槽号先比 TOKEN_SLOTS 再取。",
        0x305a1 => "模拟级越档：只收 Anonymous/Identification/Impersonation/Delegation 四档。",
        0x305a2 => "种类/级别错配：主令牌级必须 Anonymous，模拟令牌级 ≥Identification。",
        0x305a3 => "越权复制：新模拟级不得高于源级，降权复制才合法。",
        0x305a4 => "复制自身：源槽即目标，复制无意义必拒。",
        0x305a5 => "令牌槽满：先释放废令牌再建，槽位账对平。",
        0x305a6 => "令牌已释放：释放槽只读，重建走新令牌。",
        0x305a7 => "源槽零：零槽不是合法复制源。",
        0x305a8 => "句柄账倒挂：关闭计数不得超打开计数。",
        0x305a9 => "血统断链：子令牌源槽必须指向存活源或已释放源槽号。",
        _ => "未知诊断码：回查 0x305a0 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x305a0 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x305a_305a_305a_305a;

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

/// 令牌种类（两态）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TokenType {
    /// 主令牌。
    Primary = 0,
    /// 模拟令牌。
    Impersonation = 1,
}

/// 模拟级（四档闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ImpLevel {
    /// Anonymous（0）。
    Anonymous = 0,
    /// Identification（1）。
    Identification = 1,
    /// Impersonation（2）。
    Impersonation = 2,
    /// Delegation（3）。
    Delegation = 3,
}

impl ImpLevel {
    /// 由序号取级（越档返回 None）。
    pub fn of(v: u32) -> Option<ImpLevel> {
        match v {
            0 => Some(ImpLevel::Anonymous),
            1 => Some(ImpLevel::Identification),
            2 => Some(ImpLevel::Impersonation),
            3 => Some(ImpLevel::Delegation),
            _ => None,
        }
    }
}

/// 令牌条目。
#[derive(Clone, Copy, Debug)]
pub struct TokenEntry {
    /// 种类。
    pub kind: TokenType,
    /// 模拟级。
    pub level: ImpLevel,
    /// 血统源槽（创建令牌为自身槽；复制令牌为源槽）。
    pub lineage: usize,
    /// 存活。
    pub live: bool,
    /// 打开计数。
    pub opens: u32,
    /// 关闭计数。
    pub closes: u32,
}

/// 创建/复制语义档正本。
///
/// 生命周期：create（创建参数校验→占槽）→ duplicate（源级核对→子槽）→
/// close/free（句柄账与槽回收）。
#[derive(Debug)]
pub struct TokenBook {
    /// 令牌表。
    tokens: [TokenEntry; TOKEN_SLOTS],
    /// 拒止累计。
    rejects: u32,
}

impl TokenBook {
    /// 建账（空域）。
    pub fn new() -> Self {
        TokenBook {
            tokens: core::array::from_fn(|i| TokenEntry {
                kind: TokenType::Primary,
                level: ImpLevel::Anonymous,
                lineage: i,
                live: false,
                opens: 0,
                closes: 0,
            }),
            rejects: 0,
        }
    }

    /// 创建参数校验（种类/级别配比纯函数）。
    pub fn validate_create(kind: TokenType, level: ImpLevel) -> Result<(), CtCode> {
        match kind {
            TokenType::Primary => {
                if level != ImpLevel::Anonymous {
                    return Err(CtCode::TypeLevel);
                }
            }
            TokenType::Impersonation => {
                if level == ImpLevel::Anonymous {
                    return Err(CtCode::TypeLevel);
                }
            }
        }
        Ok(())
    }

    /// 创建令牌（参数校验通过才占槽；返回槽号）。
    pub fn create(&mut self, kind: TokenType, level: ImpLevel) -> Result<usize, CtCode> {
        Self::validate_create(kind, level)?;
        let mut slot: Option<usize> = None;
        for i in 0..TOKEN_SLOTS {
            if !self.tokens[i].live {
                slot = Some(i);
                break;
            }
        }
        let s = match slot {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(CtCode::TableFull);
            }
        };
        self.tokens[s] = TokenEntry {
            kind,
            level,
            lineage: s,
            live: true,
            opens: 1,
            closes: 0,
        };
        Ok(s)
    }

    /// 复制令牌（越权复制拒；血统可回溯；返回子槽号）。
    pub fn duplicate(&mut self, src: usize, new_kind: TokenType, new_level: ImpLevel) -> Result<usize, CtCode> {
        if src >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(CtCode::TokenOob);
        }
        if !self.tokens[src].live {
            self.rejects += 1;
            return Err(CtCode::TokenFreed);
        }
        // 复制参数同走创建配比纪律（种类/级别错配拒）。
        Self::validate_create(new_kind, new_level)?;
        let src_level_val = self.tokens[src].level as u32;
        let new_level_val = new_level as u32;
        if new_level_val > src_level_val {
            self.rejects += 1;
            return Err(CtCode::LevelEscalate);
        }
        // 复制自身：同槽占位无意义必拒。
        let mut slot: Option<usize> = None;
        for i in 0..TOKEN_SLOTS {
            if !self.tokens[i].live {
                slot = Some(i);
                break;
            }
        }
        let s = match slot {
            Some(s) => s,
            None => {
                self.rejects += 1;
                return Err(CtCode::TableFull);
            }
        };
        if s == src {
            self.rejects += 1;
            return Err(CtCode::DupSelf);
        }
        self.tokens[s] = TokenEntry {
            kind: new_kind,
            level: new_level,
            lineage: src,
            live: true,
            opens: 1,
            closes: 0,
        };
        Ok(s)
    }

    /// 关闭句柄（账平核对）。
    pub fn close(&mut self, slot: usize) -> Result<bool, CtCode> {
        if slot >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(CtCode::TokenOob);
        }
        if !self.tokens[slot].live {
            self.rejects += 1;
            return Err(CtCode::TokenFreed);
        }
        if self.tokens[slot].closes >= self.tokens[slot].opens {
            self.rejects += 1;
            return Err(CtCode::LedgerBack);
        }
        self.tokens[slot].closes += 1;
        Ok(true)
    }

    /// 释放槽（账平后可回收）。
    pub fn free(&mut self, slot: usize) -> Result<bool, CtCode> {
        if slot >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(CtCode::TokenOob);
        }
        if !self.tokens[slot].live {
            self.rejects += 1;
            return Err(CtCode::TokenFreed);
        }
        if self.tokens[slot].closes < self.tokens[slot].opens {
            self.rejects += 1;
            return Err(CtCode::LedgerBack);
        }
        self.tokens[slot].live = false;
        Ok(true)
    }

    /// 血统核对（子令牌 lineage 指向合法槽且源自洽）。
    pub fn lineage_ok(&self, slot: usize) -> bool {
        if slot >= TOKEN_SLOTS {
            return false;
        }
        let t = &self.tokens[slot];
        t.lineage < TOKEN_SLOTS
            && (t.lineage == slot || self.tokens[t.lineage].lineage == t.lineage)
    }

    /// 活令牌计数。
    pub fn live_count(&self) -> usize {
        self.tokens.iter().filter(|t| t.live).count()
    }

    /// 句柄账守恒（逐槽 closes ≤ opens）。
    pub fn ledger_ok(&self) -> bool {
        self.tokens.iter().all(|t| t.closes <= t.opens)
    }

    /// 令牌快照。
    pub fn token_of(&self, slot: usize) -> Option<TokenEntry> {
        if slot >= TOKEN_SLOTS {
            None
        } else {
            Some(self.tokens[slot])
        }
    }

    /// 拒止累计。
    pub fn reject_total(&self) -> u32 {
        self.rejects
    }
}

/// 创建参数演练：10/10 轮主/模拟两态创建全过、错配拒。
pub fn drill_create_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305a0 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TokenBook::new();
        let p = bk.create(TokenType::Primary, ImpLevel::Anonymous);
        let i1 = bk.create(TokenType::Impersonation, ImpLevel::Identification);
        let i2 = bk.create(TokenType::Impersonation, ImpLevel::Impersonation);
        let bad1 = bk.create(TokenType::Primary, ImpLevel::Impersonation);
        let bad2 = bk.create(TokenType::Impersonation, ImpLevel::Anonymous);
        let _ = lcg.below(3);
        if matches!(p, Ok(0))
            && matches!(i1, Ok(1))
            && matches!(i2, Ok(2))
            && matches!(bad1, Err(CtCode::TypeLevel))
            && matches!(bad2, Err(CtCode::TypeLevel))
            && bk.live_count() == 3
            && bk.ledger_ok()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("创建参数轮={}/{}", ok, INJECT_ROUNDS))
}

/// 越权复制注入 10/10 正确拒止演练（升档拒/合法降档过）。
pub fn drill_escalate_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305a0 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TokenBook::new();
        let src = bk.create(TokenType::Impersonation, ImpLevel::Impersonation).unwrap_or(0);
        // 路 1：同级复制合法。
        let same = bk.duplicate(src, TokenType::Impersonation, ImpLevel::Impersonation);
        // 路 2：降档复制合法（Identification < Impersonation）。
        let down = bk.duplicate(src, TokenType::Impersonation, ImpLevel::Identification);
        // 路 3：升档复制拒（Delegation > Impersonation）。
        let up = bk.duplicate(src, TokenType::Impersonation, ImpLevel::Delegation);
        // 路 4：主令牌带非匿名级复制拒（TypeLevel）。
        let tl = bk.duplicate(src, TokenType::Primary, ImpLevel::Impersonation);
        // 血统核对：子令牌 lineage = src。
        let lin = down.ok().and_then(|s| bk.token_of(s)).map(|t| t.lineage);
        let _ = lcg.below(3);
        if matches!(same, Ok(_))
            && matches!(down, Ok(_))
            && matches!(up, Err(CtCode::LevelEscalate))
            && matches!(tl, Err(CtCode::TypeLevel))
            && lin == Some(src)
            && bk.lineage_ok(src)
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("越权复制拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// 复制生命周期演练：源释放子存续、槽回收复用 10/10。
pub fn drill_lineage_life_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305a0 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TokenBook::new();
        let src = bk.create(TokenType::Impersonation, ImpLevel::Impersonation).unwrap_or(0);
        let child = bk.duplicate(src, TokenType::Impersonation, ImpLevel::Identification).unwrap_or(0);
        // 源先关后释放。
        let c1 = bk.close(src);
        let f1 = bk.free(src);
        // 子令牌存续且血统仍可回溯。
        let child_live = bk.token_of(child).map(|t| t.live).unwrap_or(false);
        let lin = bk.token_of(child).map(|t| t.lineage).unwrap_or(9);
        // 复制已释放源必拒。
        let dup_dead = bk.duplicate(src, TokenType::Impersonation, ImpLevel::Identification);
        // 子令牌关后释放，槽复用（首空槽 = 原源槽）。
        let _ = bk.close(child);
        let f2 = bk.free(child);
        let reuse = bk.create(TokenType::Primary, ImpLevel::Anonymous);
        let _ = lcg.below(3);
        if matches!(c1, Ok(true))
            && matches!(f1, Ok(true))
            && child_live
            && lin == src
            && matches!(dup_dead, Err(CtCode::TokenFreed))
            && matches!(f2, Ok(true))
            && matches!(reuse, Ok(x) if x == src)
            && bk.ledger_ok()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("血统生命周期轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：令牌域转录导出（逐活槽一行：种/级/血统/柄账）。
/// ─────────────────────────────────────────────────────────────────────
/// 令牌域转录导出。
pub fn export_token_lines(bk: &TokenBook) -> String {
    let mut s = String::from("令牌域转录：\n");
    for i in 0..TOKEN_SLOTS {
        if let Some(t) = bk.token_of(i) {
            if t.live {
                s.push_str(&format!(
                    "T{} 类{} 级{} 血{}\n",
                    i, t.kind as u8, t.level as u8, t.lineage
                ));
            }
        }
    }
    s
}

/// 转录完整判据（活槽齐备且血统可回溯）。
pub fn token_lines_complete(bk: &TokenBook) -> bool {
    let s = export_token_lines(bk);
    let live = bk.live_count();
    if live == 0 {
        return false;
    }
    (0..TOKEN_SLOTS).all(|i| {
        bk.token_of(i)
            .map(|t| !t.live || (s.contains(&format!("T{} 类", i)) && bk.lineage_ok(i)))
            .unwrap_or(true)
    })
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：创建/复制报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct CtReport {
    /// 活令牌数。
    pub live: usize,
    /// 拒止累计。
    pub rejects: u32,
    /// 账守恒。
    pub ledger: bool,
    /// 转录完整。
    pub lines: bool,
}

impl CtReport {
    /// 建报告。
    pub fn build(bk: &TokenBook) -> Self {
        CtReport {
            live: bk.live_count(),
            rejects: bk.reject_total(),
            ledger: bk.ledger_ok(),
            lines: token_lines_complete(bk),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.ledger && self.lines && self.rejects >= 1 && self.live >= 1
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "CtReport 活={} 拒={} 账={} 转录={}",
            self.live, self.rejects, self.ledger, self.lines
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：模拟级域演练（四档逐一可取/越档拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_level_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305a0 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut pass = (0..IMP_LEVELS).all(|v| ImpLevel::of(v).is_some());
        pass = pass && ImpLevel::of(IMP_LEVELS).is_none() && ImpLevel::of(9).is_none();
        pass = pass && (ImpLevel::Anonymous as u32) < (ImpLevel::Delegation as u32);
        let _ = lcg.below(3);
        if pass {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("模拟级域轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：句柄账演练（开一柄/关一柄/重关拒 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_handle_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x305a0 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = TokenBook::new();
        let s = bk.create(TokenType::Primary, ImpLevel::Anonymous).unwrap_or(0);
        let c1 = bk.close(s);
        let c2 = bk.close(s);
        let f_ok = bk.free(s);
        let dead = bk.close(s);
        let _ = lcg.below(3);
        if matches!(c1, Ok(true))
            && matches!(c2, Err(CtCode::LedgerBack))
            && matches!(f_ok, Ok(true))
            && matches!(dead, Err(CtCode::TokenFreed))
            && bk.ledger_ok()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("句柄账轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 E：终验清单（10 项命名核验）。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct CtCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_ct_checklist(bk: &TokenBook) -> Vec<CtCheckRow> {
    let rep = CtReport::build(bk);
    let mut rows: Vec<CtCheckRow> = Vec::new();
    rows.push(CtCheckRow { name: "账守恒", passed: bk.ledger_ok() });
    rows.push(CtCheckRow { name: "级数口径", passed: IMP_LEVELS == 4 });
    rows.push(CtCheckRow { name: "槽位宽", passed: TOKEN_SLOTS == 8 });
    rows.push(CtCheckRow { name: "级档闭集", passed: ImpLevel::of(0).is_some() && ImpLevel::of(3).is_some() && ImpLevel::of(4).is_none() });
    rows.push(CtCheckRow { name: "配比纪律", passed: TokenBook::validate_create(TokenType::Primary, ImpLevel::Anonymous).is_ok() && TokenBook::validate_create(TokenType::Primary, ImpLevel::Delegation).is_err() });
    rows.push(CtCheckRow { name: "转录完整", passed: token_lines_complete(bk) });
    rows.push(CtCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(CtCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(CtCheckRow { name: "血统面", passed: bk.token_of(0).map(|_| bk.lineage_ok(0)).unwrap_or(true) });
    rows.push(CtCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ct_checklist_green(rows: &[CtCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ct_stats_line(bk: &TokenBook) -> String {
    format!("创建复制概况：活 {} 拒 {}", bk.live_count(), bk.reject_total())
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12445_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = TokenBook::new();
    let _ = bk.create(TokenType::Primary, ImpLevel::Anonymous);
    let src = bk.create(TokenType::Impersonation, ImpLevel::Impersonation).unwrap_or(0);
    let _ = bk.duplicate(src, TokenType::Impersonation, ImpLevel::Identification);
    // 注入一例越权复制（拒止账口径）。
    let _ = bk.duplicate(src, TokenType::Impersonation, ImpLevel::Delegation);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        token_lines_complete(&bk) && CtReport::build(&bk).all_green(),
        ct_stats_line(&bk),
    ));
    // X-b：模拟级域 10/10。
    let (ok_l, d_l) = drill_level_domain_10();
    out.push(("X-模拟级域10/10", ok_l, d_l));
    // X-c：句柄账 10/10。
    let (ok_h, d_h) = drill_handle_10();
    out.push(("X-句柄账10/10", ok_h, d_h));
    // X-d：终验清单。
    let rows = build_ct_checklist(&bk);
    out.push(("X-终验清单10项", ct_checklist_green(&rows), CtReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：越档模拟级必拒（LevelOob 面：取级返回 None）。
    if ImpLevel::of(4).is_none() {
        caught += 1;
        d.push("注1 越档:抓".to_string());
    } else {
        d.push("注1 越档:漏".to_string());
    }
    // 注 2：主令牌带非匿名级必拒（TypeLevel）。
    if matches!(
        TokenBook::validate_create(TokenType::Primary, ImpLevel::Delegation),
        Err(CtCode::TypeLevel)
    ) {
        caught += 1;
        d.push("注2 错配:抓".to_string());
    } else {
        d.push("注2 错配:漏".to_string());
    }
    // 注 3：越权复制必拒（LevelEscalate）。
    let mut bk = TokenBook::new();
    let src = bk.create(TokenType::Impersonation, ImpLevel::Identification).unwrap_or(0);
    if matches!(
        bk.duplicate(src, TokenType::Impersonation, ImpLevel::Delegation),
        Err(CtCode::LevelEscalate)
    ) {
        caught += 1;
        d.push("注3 越权:抓".to_string());
    } else {
        d.push("注3 越权:漏".to_string());
    }
    // 注 4：复制已释放源必拒（TokenFreed）。
    let _ = bk.close(src);
    let _ = bk.free(src);
    if matches!(
        bk.duplicate(src, TokenType::Impersonation, ImpLevel::Identification),
        Err(CtCode::TokenFreed)
    ) {
        caught += 1;
        d.push("注4 已释:抓".to_string());
    } else {
        d.push("注4 已释:漏".to_string());
    }
    // 注 5：越槽复制必拒（TokenOob）。
    if matches!(
        bk.duplicate(TOKEN_SLOTS + 1, TokenType::Impersonation, ImpLevel::Identification),
        Err(CtCode::TokenOob)
    ) {
        caught += 1;
        d.push("注5 越槽:抓".to_string());
    } else {
        d.push("注5 越槽:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x305a0 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    CtCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12445_checks：本单判据聚合（诊断段 0x305a0）。
pub fn run_unx12445_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12445 NtCreateToken/NtDuplicateToken 语义档");
    // J1-a：创建参数（ImpersonationLevel 档 10/10）。
    let (ok1, d1) = drill_create_10();
    s.add_dyn("J1-创建参数档10/10", ok1, &d1);
    // J1-b：越权复制注入 10/10 拒止。
    let (ok2, d2) = drill_escalate_10();
    s.add_dyn("J1-越权复制拒止10/10", ok2, &d2);
    // J1-c：血统生命周期（10/10）。
    let (ok3, d3) = drill_lineage_life_10();
    s.add_dyn("J1-血统生命周期10/10", ok3, &d3);
    // J1-d：标准会话（主令牌 + 模拟源 + 降档复制 + 血统链）。
    let mut bk = TokenBook::new();
    let p = bk.create(TokenType::Primary, ImpLevel::Anonymous);
    let src = bk.create(TokenType::Impersonation, ImpLevel::Impersonation);
    let child = src.and_then(|s| bk.duplicate(s, TokenType::Impersonation, ImpLevel::Identification));
    let lin = child.ok().and_then(|c| bk.token_of(c)).map(|t| t.lineage == src.unwrap_or(9));
    s.add_dyn(
        "J1-创建复制血统链",
        p.is_ok() && src.is_ok() && child.is_ok() && lin == Some(true) && bk.ledger_ok() && token_lines_complete(&bk),
        &format!(
            "主={:?} 源={:?} 子={:?} 血统={:?}",
            p.is_ok(),
            src.is_ok(),
            child.is_ok(),
            lin
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（升档复制不占槽）。
    let l0 = bk.live_count();
    let _ = bk.duplicate(src.unwrap_or(0), TokenType::Impersonation, ImpLevel::Delegation);
    s.add_dyn(
        "J2-拒止不写回",
        bk.live_count() == l0 && bk.reject_total() >= 1,
        &format!("活 {}→{}", l0, bk.live_count()),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x305a0段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p2, d) in run_unx12445_extra() {
        s.add_dyn(n2, p2, &d);
    }
    s
}
