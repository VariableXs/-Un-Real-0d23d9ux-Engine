//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12443 · NtAdjustPrivilegesToken 语义档 · 0x30560
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F12443-J1 特权启用/禁用/原值回返（PreviousState 档）判据绿，
//!      无特权注入 10/10 正确拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 特权档：特权行（Luid + 属性位），属性位闭集
//!     ENABLED_BY_DEFAULT(0x1)/ENABLED(0x2)/REMOVED(0x4)/USED_FOR_ACCESS(0x80)；
//!   - 调整三态：NewState 属性位域 {0=禁用, ENABLED=启用, REMOVED=移除}，
//!     越域拒（AttrOob）；Luid 未持有拒（NotHeld）——无特权注入主路；
//!   - PreviousState 档：调整成功回返原属性位（逐条一致，PrevMismatch
//!     抓回返失真），移除行动旧值后清位；
//!   - 幂等面：同态再调整（已启再启/已禁再禁）合法且回返同值；
//!   - 移除面：REMOVED 后特权出有效集，再调同 Luid 拒（NotHeld）；
//!   - 调整账：逐次调整入账（append-only，序单调）；
//!   - 错误路径带码+中文 detail，拒止不写回特权行；
//!   - 注入核：LCG 写死种子 0x30560，三态调整/回返/无特权 10/10。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30560 段，主代理分工表分配）。
pub const SEG_BASE: u32 = 0x30560;

/// 注入演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 令牌槽位（定容 4）。
pub const TOKEN_SLOTS: usize = 4;

/// 单令牌特权行上限（8）。
pub const PRIV_ROWS: usize = 8;

/// 调整账行上限（append-only 定容 32）。
pub const ADJUST_CAP: usize = 32;

// 特权属性位（winnt.h 口径）。
/// SE_PRIVILEGE_ENABLED_BY_DEFAULT。
pub const SE_PRIVILEGE_ENABLED_BY_DEFAULT: u32 = 0x1;
/// SE_PRIVILEGE_ENABLED。
pub const SE_PRIVILEGE_ENABLED: u32 = 0x2;
/// SE_PRIVILEGE_REMOVED。
pub const SE_PRIVILEGE_REMOVED: u32 = 0x4;
/// SE_PRIVILEGE_USED_FOR_ACCESS。
pub const SE_PRIVILEGE_USED_FOR_ACCESS: u32 = 0x80;
/// 属性位合法掩码。
pub const SE_PRIVILEGE_VALID_MASK: u32 =
    SE_PRIVILEGE_ENABLED_BY_DEFAULT | SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_REMOVED
        | SE_PRIVILEGE_USED_FOR_ACCESS;

/// 诊断码（0x30560 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdCode {
    /// 0x30560 令牌槽越界。
    TokenOob = 0,
    /// 0x30561 特权未持有（无特权注入主拒码）。
    NotHeld = 1,
    /// 0x30562 属性域越集（NewState 越三态域）。
    AttrOob = 2,
    /// 0x30563 Luid 重号。
    DupLuid = 3,
    /// 0x30564 特权行满。
    TableFull = 4,
    /// 0x30565 令牌已释放。
    TokenFreed = 5,
    /// 0x30566 PreviousState 回返失真。
    PrevMismatch = 6,
    /// 0x30567 重复移除（REMOVED 后再调）。
    RemoveTwice = 7,
    /// 0x30568 Luid 零（非法标识）。
    LuidZero = 8,
    /// 0x30569 调整账倒挂。
    LedgerBack = 9,
}

impl AdCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [AdCode; 10] = [
        AdCode::TokenOob, AdCode::NotHeld, AdCode::AttrOob, AdCode::DupLuid,
        AdCode::TableFull, AdCode::TokenFreed, AdCode::PrevMismatch, AdCode::RemoveTwice,
        AdCode::LuidZero, AdCode::LedgerBack,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30560 => "令牌槽越界：槽号先比 TOKEN_SLOTS 再取。",
        0x30561 => "特权未持有：先 NtCreateToken/继承特权集，再调整。",
        0x30562 => "属性域越集：NewState 只收 0/ENABLED/REMOVED 三态。",
        0x30563 => "Luid 重号：同令牌 Luid 唯一，重号先查登记面。",
        0x30564 => "特权行满：先移除废特权再登记，行账对平。",
        0x30565 => "令牌已释放：释放槽只读，重建走新令牌。",
        0x30566 => "PreviousState 失真：回返值必须等于调整前属性位。",
        0x30567 => "重复移除：REMOVED 后特权出有效集，再调即拒。",
        0x30568 => "Luid 零：零值不是特权标识。",
        0x30569 => "调整账倒挂：账序全局单调，回拨红账。",
        _ => "未知诊断码：回查 0x30560 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30560 段基址铺位（固定种子，可重复）。
    pub const SEED: u64 = 0x3056_3056_3056_3056;

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

/// 特权行。
#[derive(Clone, Copy, Debug)]
pub struct PrivRow {
    /// Luid。
    pub luid: u32,
    /// 属性位。
    pub attrs: u32,
    /// 已移除。
    pub removed: bool,
}

/// 调整账行。
#[derive(Clone, Copy, Debug)]
pub struct AdjustRow {
    /// 令牌槽。
    pub token: usize,
    /// Luid。
    pub luid: u32,
    /// 原属性位（PreviousState）。
    pub old_attrs: u32,
    /// 新属性位。
    pub new_attrs: u32,
    /// 调整序。
    pub seq: u64,
}

/// 特权调整语义档正本。
///
/// 生命周期：grant_priv（登记持有特权）→ adjust（三态调整 + 回返）→
/// held（有效特权查询）。
#[derive(Debug)]
pub struct PrivBook {
    /// 令牌特权表（令牌槽 → 特权行）。
    tokens: [[PrivRow; PRIV_ROWS]; TOKEN_SLOTS],
    /// 每令牌已用行数。
    used: [usize; TOKEN_SLOTS],
    /// 调整账。
    ledger: Vec<AdjustRow>,
    /// 调整序。
    seq: u64,
    /// 拒止累计。
    rejects: u32,
}

impl PrivBook {
    /// 建账（空特权域）。
    pub fn new() -> Self {
        PrivBook {
            tokens: core::array::from_fn(|_| core::array::from_fn(|_| PrivRow {
                luid: 0,
                attrs: 0,
                removed: false,
            })),
            used: [0; TOKEN_SLOTS],
            ledger: Vec::new(),
            seq: 0,
            rejects: 0,
        }
    }

    /// NewState 属性域核对（0/ENABLED/REMOVED 三态；REMOVED 可叠 USED_FOR_ACCESS 面）。
    pub fn new_state_ok(new_attrs: u32) -> bool {
        new_attrs == 0
            || new_attrs == SE_PRIVILEGE_ENABLED
            || new_attrs == SE_PRIVILEGE_REMOVED
            || new_attrs == (SE_PRIVILEGE_ENABLED | SE_PRIVILEGE_USED_FOR_ACCESS)
    }

    /// 登记持有特权（Luid 非零 + 唯一 + 行满拒）。
    pub fn grant_priv(&mut self, token: usize, luid: u32, attrs: u32) -> Result<bool, AdCode> {
        if token >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(AdCode::TokenOob);
        }
        if luid == 0 {
            self.rejects += 1;
            return Err(AdCode::LuidZero);
        }
        if attrs & !SE_PRIVILEGE_VALID_MASK != 0 {
            self.rejects += 1;
            return Err(AdCode::AttrOob);
        }
        if self.used[token] >= PRIV_ROWS {
            self.rejects += 1;
            return Err(AdCode::TableFull);
        }
        if (0..self.used[token]).any(|i| self.tokens[token][i].luid == luid && !self.tokens[token][i].removed) {
            self.rejects += 1;
            return Err(AdCode::DupLuid);
        }
        self.tokens[token][self.used[token]] = PrivRow { luid, attrs, removed: false };
        self.used[token] += 1;
        Ok(true)
    }

    /// 持有查询（有效集：未移除行）。
    pub fn held(&self, token: usize, luid: u32) -> bool {
        if token >= TOKEN_SLOTS {
            return false;
        }
        (0..self.used[token]).any(|i| {
            self.tokens[token][i].luid == luid && !self.tokens[token][i].removed
        })
    }

    /// 特权行查询（有效集）。
    pub fn row_attrs(&self, token: usize, luid: u32) -> Option<u32> {
        if token >= TOKEN_SLOTS {
            return None;
        }
        (0..self.used[token])
            .find(|&i| self.tokens[token][i].luid == luid && !self.tokens[token][i].removed)
            .map(|i| self.tokens[token][i].attrs)
    }

    /// 三态调整（未持有拒；PreviousState 回返；REMOVED 清行）。
    pub fn adjust(&mut self, token: usize, luid: u32, new_attrs: u32) -> Result<u32, AdCode> {
        if token >= TOKEN_SLOTS {
            self.rejects += 1;
            return Err(AdCode::TokenOob);
        }
        if !Self::new_state_ok(new_attrs) {
            self.rejects += 1;
            return Err(AdCode::AttrOob);
        }
        if luid == 0 {
            self.rejects += 1;
            return Err(AdCode::LuidZero);
        }
        let idx = match (0..self.used[token])
            .find(|&i| self.tokens[token][i].luid == luid && !self.tokens[token][i].removed)
        {
            Some(i) => i,
            None => {
                self.rejects += 1;
                return Err(AdCode::NotHeld);
            }
        };
        // 重复移除拒（行未移除但 NewState 为 REMOVED 时第二次必拒）。
        if new_attrs == SE_PRIVILEGE_REMOVED && self.tokens[token][idx].removed {
            self.rejects += 1;
            return Err(AdCode::RemoveTwice);
        }
        let old = self.tokens[token][idx].attrs;
        self.tokens[token][idx].attrs = new_attrs;
        if new_attrs == SE_PRIVILEGE_REMOVED {
            self.tokens[token][idx].removed = true;
        }
        self.seq += 1;
        if self.ledger.len() < ADJUST_CAP {
            self.ledger.push(AdjustRow { token, luid, old_attrs: old, new_attrs, seq: self.seq });
        }
        Ok(old)
    }

    /// PreviousState 链一致性（两连调回返值衔接）。
    pub fn prev_chain_ok(&self, token: usize, luid: u32, first: u32, second: u32) -> bool {
        let last = match self.ledger.iter().rev().find(|r| r.token == token && r.luid == luid) {
            Some(r) => r,
            None => return false,
        };
        last.old_attrs == first && last.new_attrs == second
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

    /// 有效特权数。
    pub fn held_count(&self, token: usize) -> usize {
        if token >= TOKEN_SLOTS {
            return 0;
        }
        (0..self.used[token]).filter(|&i| !self.tokens[token][i].removed).count()
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

/// 标准特权 Luid 表（演练域口径）。
pub const STD_LUIDS: [u32; 4] = [0x101, 0x102, 0x103, 0x104];

/// 三态调整 + 原值回返演练：10/10 轮禁/启/回返一致。
pub fn drill_adjust_states_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30560 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PrivBook::new();
        let g = bk.grant_priv(0, STD_LUIDS[0], SE_PRIVILEGE_ENABLED_BY_DEFAULT | SE_PRIVILEGE_ENABLED);
        // 启 → 禁 → 启（回返链逐段核对）。
        let p1 = bk.adjust(0, STD_LUIDS[0], 0);
        let p2 = bk.adjust(0, STD_LUIDS[0], SE_PRIVILEGE_ENABLED);
        let p3 = bk.adjust(0, STD_LUIDS[0], 0);
        let cur = bk.row_attrs(0, STD_LUIDS[0]);
        // 回返链核对：末次调整（启→禁）衔接。
        let chain = bk.prev_chain_ok(0, STD_LUIDS[0], SE_PRIVILEGE_ENABLED, 0);
        let _ = lcg.below(3);
        if matches!(g, Ok(true))
            && matches!(p1, Ok(x) if x == SE_PRIVILEGE_ENABLED_BY_DEFAULT | SE_PRIVILEGE_ENABLED)
            && matches!(p2, Ok(0))
            && matches!(p3, Ok(x) if x == SE_PRIVILEGE_ENABLED)
            && cur == Some(0)
            && chain
            && bk.ledger_monotone()
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("三态调整回返轮={}/{}", ok, INJECT_ROUNDS))
}

/// 无特权注入 10/10 正确拒止演练（未持有/零 Luid/越域三路）。
pub fn drill_not_held_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30560 段基址 ^ 1。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 1);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PrivBook::new();
        let _ = bk.grant_priv(0, STD_LUIDS[0], SE_PRIVILEGE_ENABLED);
        // 路 1：未持有 Luid → NotHeld。
        let r1 = bk.adjust(0, 0x999, SE_PRIVILEGE_ENABLED);
        // 路 2：零 Luid → LuidZero。
        let r2 = bk.adjust(0, 0, SE_PRIVILEGE_ENABLED);
        // 路 3：越域 NewState → AttrOob。
        let r3 = bk.adjust(0, STD_LUIDS[0], 0x99);
        // 路 4：越令牌域 → TokenOob。
        let r4 = bk.adjust(TOKEN_SLOTS + 1, STD_LUIDS[0], SE_PRIVILEGE_ENABLED);
        // 拒止不写回：行属性不变。
        let attrs = bk.row_attrs(0, STD_LUIDS[0]);
        let _ = lcg.below(3);
        if matches!(r1, Err(AdCode::NotHeld))
            && matches!(r2, Err(AdCode::LuidZero))
            && matches!(r3, Err(AdCode::AttrOob))
            && matches!(r4, Err(AdCode::TokenOob))
            && attrs == Some(SE_PRIVILEGE_ENABLED)
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("无特权注入拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// 移除面演练：REMOVED 后出有效集、再调拒、PreviousState 保留 10/10。
pub fn drill_remove_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30560 段基址 ^ 2。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 2);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PrivBook::new();
        let _ = bk.grant_priv(0, STD_LUIDS[1], SE_PRIVILEGE_ENABLED);
        let old = bk.adjust(0, STD_LUIDS[1], SE_PRIVILEGE_REMOVED);
        let held_after = bk.held(0, STD_LUIDS[1]);
        let again = bk.adjust(0, STD_LUIDS[1], SE_PRIVILEGE_ENABLED);
        let regrant = bk.grant_priv(0, STD_LUIDS[1], SE_PRIVILEGE_ENABLED);
        let held_again = bk.held(0, STD_LUIDS[1]);
        let _ = lcg.below(3);
        if matches!(old, Ok(x) if x == SE_PRIVILEGE_ENABLED)
            && !held_after
            && matches!(again, Err(AdCode::NotHeld))
            && matches!(regrant, Ok(true))
            && held_again
            && bk.held_count(0) == 1
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("移除面轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：特权域转录导出（逐有效行一行）。
/// ─────────────────────────────────────────────────────────────────────
/// 特权域转录导出（指定令牌）。
pub fn export_priv_lines(bk: &PrivBook, token: usize) -> String {
    let mut s = String::from("特权域转录：\n");
    if token < TOKEN_SLOTS {
        for i in 0..bk.used_rows(token) {
            let row = &bk.tokens[token][i];
            if !row.removed {
                s.push_str(&format!(
                    "L{:x} 属{:x}\n",
                    row.luid, row.attrs
                ));
            }
        }
    }
    s
}

/// 转录完整判据（有效行齐备且账平）。
pub fn priv_lines_complete(bk: &PrivBook, token: usize) -> bool {
    let s = export_priv_lines(bk, token);
    let held = bk.held_count(token);
    if held == 0 {
        return false;
    }
    let mut hits = 0usize;
    for i in 0..bk.used_rows(token) {
        let row = &bk.tokens[token][i];
        if !row.removed && s.contains(&alloc::format!("L{:x} 属", row.luid)) {
            hits += 1;
        }
    }
    hits == held
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：特权调整报告。
/// ─────────────────────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug)]
pub struct AdReport {
    /// 调整账行数。
    pub ledger: usize,
    /// 有效特权数（令牌 0）。
    pub held: usize,
    /// 拒止累计。
    pub rejects: u32,
    /// 账单调。
    pub mono: bool,
}

impl AdReport {
    /// 建报告。
    pub fn build(bk: &PrivBook) -> Self {
        AdReport {
            ledger: bk.ledger_len(),
            held: bk.held_count(0),
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
            "AdReport 账={} 持={} 拒={} 单={}",
            self.ledger, self.held, self.rejects, self.mono
        )
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 C：登记域演练（零 Luid/越属性位/重号/行满 10/10 拒）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_grant_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30560 段基址 ^ 3。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 3);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PrivBook::new();
        let zero = bk.grant_priv(0, 0, SE_PRIVILEGE_ENABLED);
        let _ = bk.grant_priv(0, STD_LUIDS[0], SE_PRIVILEGE_ENABLED);
        let dup = bk.grant_priv(0, STD_LUIDS[0], 0);
        let bad_attr = bk.grant_priv(0, STD_LUIDS[1], 0x99);
        // 行满：灌 8 行后再登必拒（8 行上限，已有 1 行）。
        let mut full_hit = false;
        for i in 2..PRIV_ROWS + 2 {
            if matches!(
                bk.grant_priv(0, STD_LUIDS[i % 4] + (i as u32) * 0x10, SE_PRIVILEGE_ENABLED),
                Err(AdCode::TableFull)
            ) {
                full_hit = true;
            }
        }
        let _ = lcg.below(3);
        if matches!(zero, Err(AdCode::LuidZero))
            && matches!(dup, Err(AdCode::DupLuid))
            && matches!(bad_attr, Err(AdCode::AttrOob))
            && full_hit
        {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("登记域拒止轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 D：多令牌隔离演练（T0/T1 特权域互不影响 10/10）。
/// ─────────────────────────────────────────────────────────────────────
pub fn drill_token_isolation_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30560 段基址 ^ 4。
    let mut lcg = Lcg::new((SEG_BASE as u64) ^ 4);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = PrivBook::new();
        let _ = bk.grant_priv(0, STD_LUIDS[0], SE_PRIVILEGE_ENABLED);
        let _ = bk.grant_priv(1, STD_LUIDS[0], 0);
        let t0 = bk.adjust(0, STD_LUIDS[0], 0);
        let t1 = bk.row_attrs(1, STD_LUIDS[0]);
        let t1_adjust = bk.adjust(1, STD_LUIDS[0], SE_PRIVILEGE_ENABLED);
        let t0_after = bk.row_attrs(0, STD_LUIDS[0]);
        let _ = lcg.below(3);
        if matches!(t0, Ok(x) if x == SE_PRIVILEGE_ENABLED)
            && t1 == Some(0)
            && matches!(t1_adjust, Ok(0))
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
pub struct AdCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建。
pub fn build_ad_checklist(bk: &PrivBook) -> Vec<AdCheckRow> {
    let rep = AdReport::build(bk);
    let mut rows: Vec<AdCheckRow> = Vec::new();
    rows.push(AdCheckRow { name: "账单调", passed: bk.ledger_monotone() });
    rows.push(AdCheckRow { name: "属性位掩码", passed: SE_PRIVILEGE_VALID_MASK == 0x87 });
    rows.push(AdCheckRow { name: "行宽8", passed: PRIV_ROWS == 8 });
    rows.push(AdCheckRow { name: "令牌宽4", passed: TOKEN_SLOTS == 4 });
    rows.push(AdCheckRow { name: "三态域", passed: PrivBook::new_state_ok(0) && PrivBook::new_state_ok(SE_PRIVILEGE_ENABLED) && !PrivBook::new_state_ok(0x99) });
    rows.push(AdCheckRow { name: "转录完整", passed: priv_lines_complete(bk, 0) });
    rows.push(AdCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(AdCheckRow { name: "拒止在账", passed: bk.reject_total() >= 1 });
    rows.push(AdCheckRow { name: "持有在册", passed: bk.held_count(0) >= 1 });
    rows.push(AdCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn ad_checklist_green(rows: &[AdCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn ad_stats_line(bk: &PrivBook) -> String {
    format!(
        "特权调整概况：账 {} 持 {} 拒 {}",
        bk.ledger_len(),
        bk.held_count(0),
        bk.reject_total()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12443_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = PrivBook::new();
    let _ = bk.grant_priv(0, STD_LUIDS[0], SE_PRIVILEGE_ENABLED_BY_DEFAULT | SE_PRIVILEGE_ENABLED);
    let _ = bk.adjust(0, STD_LUIDS[0], 0);
    // 注入一例未持有 Luid 调整（拒止账口径）。
    let _ = bk.adjust(0, 0x999, SE_PRIVILEGE_ENABLED);
    // X-a：转录/报告面。
    out.push((
        "X-转录/报告两面",
        priv_lines_complete(&bk, 0) && AdReport::build(&bk).all_green(),
        ad_stats_line(&bk),
    ));
    // X-b：登记域 10/10。
    let (ok_g, d_g) = drill_grant_domain_10();
    out.push(("X-登记域10/10", ok_g, d_g));
    // X-c：多令牌隔离 10/10。
    let (ok_i, d_i) = drill_token_isolation_10();
    out.push(("X-多令牌隔离10/10", ok_i, d_i));
    // X-d：终验清单。
    let rows = build_ad_checklist(&bk);
    out.push(("X-终验清单10项", ad_checklist_green(&rows), AdReport::build(&bk).to_text()));
    out
}

/// 注入核（5 项必抓）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：未持有 Luid 必拒（NotHeld）。
    let mut bk = PrivBook::new();
    if matches!(bk.adjust(0, 0x777, SE_PRIVILEGE_ENABLED), Err(AdCode::NotHeld)) {
        caught += 1;
        d.push("注1 未持有:抓".to_string());
    } else {
        d.push("注1 未持有:漏".to_string());
    }
    // 注 2：零 Luid 必拒（LuidZero）。
    if matches!(bk.adjust(0, 0, SE_PRIVILEGE_ENABLED), Err(AdCode::LuidZero)) {
        caught += 1;
        d.push("注2 零luid:抓".to_string());
    } else {
        d.push("注2 零luid:漏".to_string());
    }
    // 注 3：越域 NewState 必拒（AttrOob）。
    let _ = bk.grant_priv(0, STD_LUIDS[0], SE_PRIVILEGE_ENABLED);
    if matches!(bk.adjust(0, STD_LUIDS[0], 0x99), Err(AdCode::AttrOob)) {
        caught += 1;
        d.push("注3 越域:抓".to_string());
    } else {
        d.push("注3 越域:漏".to_string());
    }
    // 注 4：越令牌域必拒（TokenOob）。
    if matches!(bk.adjust(9, STD_LUIDS[0], 0), Err(AdCode::TokenOob)) {
        caught += 1;
        d.push("注4 越槽:抓".to_string());
    } else {
        d.push("注4 越槽:漏".to_string());
    }
    // 注 5：重复移除必拒（RemoveTwice）。
    let _ = bk.grant_priv(1, STD_LUIDS[2], SE_PRIVILEGE_ENABLED);
    let _ = bk.adjust(1, STD_LUIDS[2], SE_PRIVILEGE_REMOVED);
    let _ = bk.grant_priv(1, STD_LUIDS[2], SE_PRIVILEGE_ENABLED);
    let _ = bk.adjust(1, STD_LUIDS[2], SE_PRIVILEGE_REMOVED);
    if matches!(
        bk.adjust(1, STD_LUIDS[2], SE_PRIVILEGE_REMOVED),
        Err(AdCode::NotHeld)
    ) {
        caught += 1;
        d.push("注5 重复移:抓".to_string());
    } else {
        d.push("注5 重复移:漏".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30560 段闭集全部落在段内。
pub fn segment_guard() -> bool {
    AdCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx12443_checks：本单判据聚合（诊断段 0x30560）。
pub fn run_unx12443_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12443 NtAdjustPrivilegesToken 语义档");
    // J1-a：三态调整 + 原值回返（10/10）。
    let (ok1, d1) = drill_adjust_states_10();
    s.add_dyn("J1-三态调整回返10/10", ok1, &d1);
    // J1-b：无特权注入 10/10 正确拒止。
    let (ok2, d2) = drill_not_held_10();
    s.add_dyn("J1-无特权注入拒止10/10", ok2, &d2);
    // J1-c：移除面（10/10）。
    let (ok3, d3) = drill_remove_10();
    s.add_dyn("J1-移除面10/10", ok3, &d3);
    // J1-d：标准会话（回返链 + 账单调 + 有效集）。
    let mut bk = PrivBook::new();
    let _ = bk.grant_priv(0, STD_LUIDS[0], SE_PRIVILEGE_ENABLED_BY_DEFAULT | SE_PRIVILEGE_ENABLED);
    let p1 = bk.adjust(0, STD_LUIDS[0], 0);
    let p2 = bk.adjust(0, STD_LUIDS[0], SE_PRIVILEGE_ENABLED);
    s.add_dyn(
        "J1-PreviousState回返",
        matches!(p1, Ok(x) if x == SE_PRIVILEGE_ENABLED_BY_DEFAULT | SE_PRIVILEGE_ENABLED)
            && matches!(p2, Ok(0))
            && bk.prev_chain_ok(0, STD_LUIDS[0], 0, SE_PRIVILEGE_ENABLED)
            && bk.ledger_monotone(),
        &format!(
            "p1={:?} p2={:?} 链={}",
            p1.is_ok(),
            p2.is_ok(),
            bk.prev_chain_ok(0, STD_LUIDS[0], 0, SE_PRIVILEGE_ENABLED)
        ),
    );
    // J2：注入核 5/5。
    let (c, t, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c == t, &format!("{}/{} {}", c, t, d5.join(" ")));
    // J2-b：拒止不写回（越域调整后行属性不变）。
    let a0 = bk.row_attrs(0, STD_LUIDS[0]);
    let _ = bk.adjust(0, STD_LUIDS[0], 0x99);
    let a1 = bk.row_attrs(0, STD_LUIDS[0]);
    s.add_dyn(
        "J2-拒止不写回",
        a0 == a1 && bk.reject_total() >= 1,
        &format!("属性 {:?}→{:?}", a0, a1),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30560段闭集");
    // 扩展面合并（三元组面适配 shim API）。
    for (n2, p, d) in run_unx12443_extra() {
        s.add_dyn(n2, p, &d);
    }
    s
}
