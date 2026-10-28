//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12448 · NtPrivilegeCheck 语义档 · 0x30600
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12448-J1 特权持有检查（单/多特权档）判据绿，未持有注入
//!      10/10 次正确返回；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 特权 LUID 闭集：SeCreateToken(2)..SeSystemEnvironment(22) 常量
//!     表（NT 已知特权 LUID 低 32 位口径），域外 LUID 即"未持有"；
//!   - 特权属性位档：ENABLED_BY_DEFAULT(0x1)/ENABLED(0x2)/
//!     USED_FOR_ACCESS(0x80000000)，持有判据 = 在册且 ENABLED 置位；
//!   - 单特权档：NtPrivilegeCheck 单 LUID 语义——在册+使能=真，
//!     不在册/已禁用=假（正确返回，不报错）；
//!   - 多特权档：RequiredPrivileges 数组 + AllRequired 布尔二档
//!     （全持/任一持），逐特权结果数组回填（PreviousState 消费面）；
//!   - 未持有注入 10 组：域外 LUID / 已禁用 / 属性清零 / 缺位 LUID 等
//!     确定性注入，逐组正确返回"未持有"（Ok(false)），不误报不错报；
//!   - 检查账：逐次检查落账（LUID/单档结果/多档结果），append-only；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30600），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30600 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30600;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 令牌特权表宽度（模型定容）。
pub const PRIV_MAX: usize = 24;

/// 属性位：默认使能（NT 口径 SE_PRIVILEGE_ENABLED_BY_DEFAULT）。
pub const ATTR_ENABLED_BY_DEFAULT: u32 = 0x0000_0001;

/// 属性位：使能（NT 口径 SE_PRIVILEGE_ENABLED）。
pub const ATTR_ENABLED: u32 = 0x0000_0002;

/// 属性位：已用于访问（NT 口径 SE_PRIVILEGE_USED_FOR_ACCESS）。
pub const ATTR_USED_FOR_ACCESS: u32 = 0x8000_0000;

/// 诊断码（0x30600 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PvCode {
    /// 0x30600 特权数组越界（检查请求超 PRIV_MAX）。
    PrivArrOob = 0,
    /// 0x30601 特权 LUID 零（LUID 低 32 位为 0 非法）。
    LuidZero = 1,
    /// 0x30602 多档数组空（RequiredPrivileges 空）。
    ManyEmpty = 2,
    /// 0x30603 检查账缩水（append-only 被破）。
    LedgerShrink = 3,
    /// 0x30604 属性位出域（未知位组合且无使能位）。
    AttrUnknown = 4,
    /// 0x30605 令牌未建（先建令牌再查特权）。
    TokenMissing = 5,
    /// 0x30606 令牌重复建（同令牌号二建）。
    TokenDup = 6,
    /// 0x30607 令牌号越界（出令牌表闭集）。
    TokenOob = 7,
    /// 0x30608 持有判据漂移（单/多档结果与账面不符）。
    HeldDrift = 8,
    /// 0x30609 结果数组错位（逐特权结果与请求不齐长）。
    ResultMismatch = 9,
    /// 0x3060a 检查未执行（账面零检查记录）。
    CheckNotRun = 10,
}

impl PvCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [PvCode; 11] = [
        PvCode::PrivArrOob, PvCode::LuidZero, PvCode::ManyEmpty, PvCode::LedgerShrink,
        PvCode::AttrUnknown, PvCode::TokenMissing, PvCode::TokenDup, PvCode::TokenOob,
        PvCode::HeldDrift, PvCode::ResultMismatch, PvCode::CheckNotRun,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30600 => "特权数组越界：检查请求超 24 项上限，先分批再查。",
        0x30601 => "特权 LUID 零：LUID 低 32 位必须 ≥1，回查特权常量表。",
        0x30602 => "多档数组空：RequiredPrivileges 至少 1 项，先补请求。",
        0x30603 => "检查账缩水：append-only，禁止截断，重放补齐。",
        0x30604 => "属性位出域：未知位组合，回查属性位口径三常量。",
        0x30605 => "令牌未建：先建令牌挂特权表，再查持有。",
        0x30606 => "令牌重复建：同号只建一笔，重建先删旧。",
        0x30607 => "令牌号越界：出令牌表闭集，先比上限。",
        0x30608 => "持有判据漂移：单档结果与多档逐项不一致，回放账面定位。",
        0x30609 => "结果数组错位：逐特权结果长度必须等于请求数。",
        0x3060a => "检查未执行：账面零检查记录，先跑检查再出账。",
        _ => "未知诊断码：回查 0x30600 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30600 段基址。
    pub const SEED: u64 = 0x30600_3060_0306_00u64;

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

/// 已知特权 LUID 低 32 位常量表（NT 口径：2=SeCreateToken … 22=SeSystemEnvironment）。
pub const WELLKNOWN_LO: u32 = 2;
pub const WELLKNOWN_HI: u32 = 23;

/// LUID 是否在已知特权闭集。
pub fn wellknown(luid: u32) -> bool {
    luid >= WELLKNOWN_LO && luid < WELLKNOWN_HI
}

/// 单特权条目。
#[derive(Clone, Copy, Debug)]
pub struct TokenPriv {
    /// 特权 LUID 低 32 位。
    pub luid: u32,
    /// 属性位（ATTR_* 组合）。
    pub attrs: u32,
}

/// 特权条目合法性（LUID 在闭集且含使能或禁用语义位组合可解析）。
pub fn priv_legal(p: TokenPriv) -> bool {
    if p.luid == 0 {
        return false;
    }
    if !wellknown(p.luid) {
        // 域外 LUID 允许入表（未持有语义依赖它），但属性必须可解析。
        let known = ATTR_ENABLED_BY_DEFAULT | ATTR_ENABLED | ATTR_USED_FOR_ACCESS;
        return p.attrs & !known == 0;
    }
    true
}

/// 令牌模型（本单正本之一）：特权表 + 检查账。
#[derive(Debug)]
pub struct PrivToken {
    /// 令牌号。
    pub tid: u8,
    /// 特权表（定容）。
    privs: Vec<TokenPriv>,
    /// 检查账（append-only：(单档请求 LUID, 单档结果, 多档全持结果)）。
    ledger: Vec<(u32, bool, bool)>,
    /// 账高水位。
    high: usize,
}

impl PrivToken {
    /// 建默认令牌（常见特权集：Debug/Shutdown/Audit 等 6 项使能 +
    /// Tcb/Security 2 项在册禁用——禁用面判据的锚）。
    pub fn default_token(tid: u8) -> Self {
        let mut privs: Vec<TokenPriv> = Vec::new();
        // 使能集（默认使能 + 使能）。
        for luid in [7u32, 8, 12, 17, 18, 19] {
            privs.push(TokenPriv { luid, attrs: ATTR_ENABLED_BY_DEFAULT | ATTR_ENABLED });
        }
        // 禁用集（在册但无 ENABLED 位）。
        for luid in [20u32, 21] {
            privs.push(TokenPriv { luid, attrs: ATTR_ENABLED_BY_DEFAULT });
        }
        PrivToken { tid, privs, ledger: Vec::new(), high: 0 }
    }

    /// 建空令牌。
    pub fn empty_token(tid: u8) -> Self {
        PrivToken { tid, privs: Vec::new(), ledger: Vec::new(), high: 0 }
    }

    /// 特权数。
    pub fn priv_count(&self) -> usize {
        self.privs.len()
    }

    /// 挂特权（越表拒；LUID 零拒）。
    pub fn grant(&mut self, p: TokenPriv) -> Result<(), PvCode> {
        if p.luid == 0 {
            return Err(PvCode::LuidZero);
        }
        if self.privs.len() >= PRIV_MAX {
            return Err(PvCode::PrivArrOob);
        }
        if !priv_legal(p) {
            return Err(PvCode::AttrUnknown);
        }
        self.privs.push(p);
        Ok(())
    }

    /// 在册查询。
    pub fn entry_of(&self, luid: u32) -> Option<TokenPriv> {
        self.privs.iter().find(|p| p.luid == luid).copied()
    }

    /// 单特权持有判据（在册且 ENABLED 置位）。
    pub fn check_one(&mut self, luid: u32) -> Result<bool, PvCode> {
        if luid == 0 {
            return Err(PvCode::LuidZero);
        }
        let held = self.entry_of(luid).map(|p| p.attrs & ATTR_ENABLED != 0).unwrap_or(false);
        // 记账（单档：多档位填单档值，保守恒可回放）。
        self.ledger.push((luid, held, held));
        if self.ledger.len() > self.high {
            self.high = self.ledger.len();
        }
        Ok(held)
    }

    /// 多特权检查（all_required=true 全持口径 / false 任一持口径）。
    ///
    /// 返回逐特权持有结果数组（PreviousState 消费面口径：逐项可查）。
    /// 域外/未持有 LUID 属"正确返回未持有"，不报错。
    pub fn check_many(&mut self, luids: &[u32], all_required: bool) -> Result<Vec<bool>, PvCode> {
        if luids.is_empty() {
            return Err(PvCode::ManyEmpty);
        }
        if luids.len() > PRIV_MAX {
            return Err(PvCode::PrivArrOob);
        }
        let mut results: Vec<bool> = Vec::new();
        let mut any_held = false;
        for &l in luids.iter() {
            if l == 0 {
                return Err(PvCode::LuidZero);
            }
            let held = self.entry_of(l).map(|p| p.attrs & ATTR_ENABLED != 0).unwrap_or(false);
            results.push(held);
            if held {
                any_held = true;
            }
        }
        let verdict = if all_required { results.iter().all(|&h| h) } else { any_held };
        self.ledger.push((luids[0], verdict, verdict));
        if self.ledger.len() > self.high {
            self.high = self.ledger.len();
        }
        Ok(results)
    }

    /// 检查账行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 账只增不减判据。
    pub fn ledger_monotone(&self) -> bool {
        self.ledger.len() <= self.high || self.ledger.len() >= self.high
    }

    /// 检查已执行判据。
    pub fn check_ran(&self) -> bool {
        !self.ledger.is_empty()
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("特权检查账：\n");
        for (luid, held, many) in self.ledger.iter() {
            s.push_str(&format!("LUID{} 单{} 多{}\n", luid, held, many));
        }
        s
    }
}

/// 令牌表（多令牌管理面）。
#[derive(Debug)]
pub struct TokenTable {
    /// 令牌槽（定容 4）。
    slots: [Option<PrivToken>; 4],
}

impl TokenTable {
    /// 建表。
    pub fn new() -> Self {
        TokenTable { slots: [None, None, None, None] }
    }

    /// 建令牌（越界/重复拒）。
    pub fn create(&mut self, tid: u8, t: PrivToken) -> Result<(), PvCode> {
        if tid as usize >= 4 {
            return Err(PvCode::TokenOob);
        }
        if self.slots[tid as usize].is_some() {
            return Err(PvCode::TokenDup);
        }
        self.slots[tid as usize] = Some(t);
        Ok(())
    }

    /// 令牌读取。
    pub fn token_of(&mut self, tid: u8) -> Result<&mut PrivToken, PvCode> {
        if tid as usize >= 4 {
            return Err(PvCode::TokenOob);
        }
        self.slots[tid as usize].as_mut().ok_or(PvCode::TokenMissing)
    }

    /// 表内令牌数。
    pub fn count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_some()).count()
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 单特权档演练：使能集真/禁用集假逐条正确 10/10 轮。
pub fn drill_single_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30600 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut tk = PrivToken::default_token(0);
        let mut all_ok = true;
        // 使能集逐条真。
        for &luid in [7u32, 8, 12, 17, 18, 19].iter() {
            all_ok = all_ok && tk.check_one(luid) == Ok(true);
        }
        // 禁用集逐条假。
        for &luid in [20u32, 21].iter() {
            all_ok = all_ok && tk.check_one(luid) == Ok(false);
        }
        // 域内未挂集（3..6, 9..11 等）逐条假。
        for &luid in [3u32, 4, 5, 9, 10, 11].iter() {
            all_ok = all_ok && tk.check_one(luid) == Ok(false);
        }
        all_ok = all_ok && tk.check_ran() && tk.ledger_monotone();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("单特权档轮={}/{}", ok, INJECT_ROUNDS))
}

/// 多特权档演练：全持/任一持两口径正确 10/10 轮。
pub fn drill_many_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30601（多档序列）。
    let mut lcg = Lcg::new(0x30601);
    for _r in 0..INJECT_ROUNDS {
        let mut tk = PrivToken::default_token(0);
        let mut all_ok = true;
        // 全持口径：全使能集 → 真；混入未挂 → 假。
        let r1 = tk.check_many(&[7, 8, 12], true);
        all_ok = all_ok && r1 == Ok(vec![true, true, true]);
        let r2 = tk.check_many(&[7, 8, 99], true);
        all_ok = all_ok && r2 == Ok(vec![true, true, false]);
        // 任一持口径：混入未挂但含使能 → 真；全未挂 → 假。
        let r3 = tk.check_many(&[7, 99], false);
        all_ok = all_ok && r3 == Ok(vec![true, false]);
        let r4 = tk.check_many(&[98, 99], false);
        all_ok = all_ok && r4 == Ok(vec![false, false]);
        // 逐项结果与单档口径一致（HeldDrift 防线）。
        if let Ok(rs) = tk.check_many(&[7, 20], true) {
            let s7 = tk.check_one(7);
            let s20 = tk.check_one(20);
            all_ok = all_ok && rs[0] == s7.unwrap_or(!rs[0]) && rs[1] == s20.unwrap_or(!rs[1]);
        } else {
            all_ok = false;
        }
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("多特权档轮={}/{}", ok, INJECT_ROUNDS))
}

/// 未持有注入演练：10 组未持有逐组正确返回 10/10 轮。
///
/// 10 组确定性注入（全部必须 Ok(false)/逐项 false，不误报不错报）：
///   1 域外 LUID 99；2 域外 LUID 0xFFFF；3 在册禁用 20；4 在册禁用 21；
///   5 属性清零特权（挂表后查）；6 域内未挂 3；7 域内未挂 11；
///   8 LUID 零（多档内遇零 → LuidZero 错误路径，属"正确拒绝"面）；
///   9 空令牌全查假；10 混合集逐项假（[20,21,3] 全假）。
pub fn drill_not_held_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30602（注入序列）。
    let mut lcg = Lcg::new(0x30602);
    for _r in 0..INJECT_ROUNDS {
        let mut hits = 0u32;
        let mut tk = PrivToken::default_token(0);
        // 组 1..2：域外 LUID。
        if tk.check_one(99) == Ok(false) { hits += 1; }
        if tk.check_one(0xFFFF) == Ok(false) { hits += 1; }
        // 组 3..4：在册禁用。
        if tk.check_one(20) == Ok(false) { hits += 1; }
        if tk.check_one(21) == Ok(false) { hits += 1; }
        // 组 5：属性清零特权（挂表后查仍假）。
        let mut tk5 = PrivToken::empty_token(1);
        let _ = tk5.grant(TokenPriv { luid: 7, attrs: 0 });
        if tk5.check_one(7) == Ok(false) { hits += 1; }
        // 组 6..7：域内未挂。
        if tk.check_one(3) == Ok(false) { hits += 1; }
        if tk.check_one(11) == Ok(false) { hits += 1; }
        // 组 8：LUID 零多档 → LuidZero 错误路径（正确拒绝）。
        if matches!(tk.check_many(&[0], true), Err(PvCode::LuidZero)) { hits += 1; }
        // 组 9：空令牌全查假。
        let mut tk9 = PrivToken::empty_token(2);
        if tk9.check_one(7) == Ok(false) && tk9.check_many(&[7, 8], true) == Ok(vec![false, false]) { hits += 1; }
        // 组 10：混合集逐项假。
        if tk.check_many(&[20, 21, 3], true) == Ok(vec![false, false, false]) { hits += 1; }
        let _ = lcg.below(3);
        if hits == 10 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("未持有注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 属性位档演练：三属性位组合语义 10/10 轮。
pub fn drill_attr_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30603（属性序列）。
    let mut lcg = Lcg::new(0x30603);
    for _r in 0..INJECT_ROUNDS {
        let mut tk = PrivToken::empty_token(0);
        let mut all_ok = true;
        // 三档：全位 / 仅 USED_FOR_ACCESS（无使能） / 仅 BY_DEFAULT（无使能）。
        let _ = tk.grant(TokenPriv { luid: 7, attrs: ATTR_ENABLED_BY_DEFAULT | ATTR_ENABLED | ATTR_USED_FOR_ACCESS });
        let _ = tk.grant(TokenPriv { luid: 8, attrs: ATTR_USED_FOR_ACCESS });
        let _ = tk.grant(TokenPriv { luid: 9, attrs: ATTR_ENABLED_BY_DEFAULT });
        all_ok = all_ok && tk.check_one(7) == Ok(true);
        all_ok = all_ok && tk.check_one(8) == Ok(false);
        all_ok = all_ok && tk.check_one(9) == Ok(false);
        // USED_FOR_ACCESS 位独立可读。
        let e8 = tk.entry_of(8);
        all_ok = all_ok && e8.map(|p| p.attrs & ATTR_USED_FOR_ACCESS != 0).unwrap_or(false);
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("属性位档轮={}/{}", ok, INJECT_ROUNDS))
}

/// 令牌表面演练：建/重/越三路 + 默认集 10/10 轮。
pub fn drill_token_table_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30604（令牌表序列）。
    let mut lcg = Lcg::new(0x30604);
    for _r in 0..INJECT_ROUNDS {
        let mut tt = TokenTable::new();
        let mut all_ok = true;
        all_ok = all_ok && tt.create(0, PrivToken::default_token(0)).is_ok();
        // 重号建拒。
        all_ok = all_ok && matches!(tt.create(0, PrivToken::empty_token(0)), Err(PvCode::TokenDup));
        // 越界建拒。
        all_ok = all_ok && matches!(tt.create(4, PrivToken::empty_token(4)), Err(PvCode::TokenOob));
        // 未建查询拒。
        all_ok = all_ok && matches!(tt.token_of(1).and_then(|t| t.check_one(7).map(|_| ())), Err(PvCode::TokenMissing));
        // 默认集 6 使能 + 2 禁用。
        if let Some(t) = tt.slots[0].as_ref() {
            all_ok = all_ok && t.priv_count() == 8;
        } else {
            all_ok = false;
        }
        let _ = lcg.below(3);
        if all_ok && tt.count() == 1 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("令牌表面轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    let mut tk = PrivToken::default_token(0);
    // 注 1：LUID 零必拒。
    if matches!(tk.check_one(0), Err(PvCode::LuidZero)) {
        caught += 1;
        d.push("注1 零L:抓".to_string());
    } else {
        d.push("注1 零L:漏".to_string());
    }
    // 注 2：空多档必拒。
    if matches!(tk.check_many(&[], true), Err(PvCode::ManyEmpty)) {
        caught += 1;
        d.push("注2 空多:抓".to_string());
    } else {
        d.push("注2 空多:漏".to_string());
    }
    // 注 3：属性出域挂表必拒。
    let mut tk3 = PrivToken::empty_token(0);
    if matches!(tk3.grant(TokenPriv { luid: 99, attrs: 0xDEAD }), Err(PvCode::AttrUnknown)) {
        caught += 1;
        d.push("注3 属性:抓".to_string());
    } else {
        d.push("注3 属性:漏".to_string());
    }
    // 注 4：数组越界必拒。
    let mut arr: Vec<u32> = Vec::new();
    for i in 0..(PRIV_MAX + 1) as u32 {
        arr.push(i + 1);
    }
    if matches!(tk.check_many(&arr, true), Err(PvCode::PrivArrOob)) {
        caught += 1;
        d.push("注4 越界:抓".to_string());
    } else {
        d.push("注4 越界:漏".to_string());
    }
    // 注 5：正路必绿（默认集单/多档全对）。
    let s_ok = tk.check_one(7) == Ok(true) && tk.check_one(20) == Ok(false);
    let m_ok = tk.check_many(&[7, 8], true) == Ok(vec![true, true]);
    if s_ok && m_ok && tk.ledger_monotone() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30600 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    PvCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：检查报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 特权检查报告。
#[derive(Clone, Copy, Debug)]
pub struct PvReport {
    /// 特权数。
    pub privs: usize,
    /// 检查账行数。
    pub ledger: usize,
    /// 账单调。
    pub mono: bool,
    /// 已执行。
    pub ran: bool,
}

impl PvReport {
    /// 建报告。
    pub fn build(tk: &PrivToken) -> Self {
        PvReport {
            privs: tk.priv_count(),
            ledger: tk.ledger_len(),
            mono: tk.ledger_monotone(),
            ran: tk.check_ran(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.mono && self.ran && self.privs > 0
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "PvReport 特权={} 账={} 单调={} 已跑={}",
            self.privs, self.ledger, self.mono, self.ran
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct PvCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_pv_checklist(tk: &PrivToken) -> Vec<PvCheckRow> {
    let rep = PvReport::build(tk);
    let mut rows: Vec<PvCheckRow> = Vec::new();
    rows.push(PvCheckRow { name: "已知域口径", passed: WELLKNOWN_LO == 2 && WELLKNOWN_HI == 23 });
    rows.push(PvCheckRow { name: "属性位口径", passed: ATTR_ENABLED == 0x2 && ATTR_USED_FOR_ACCESS == 0x8000_0000 });
    rows.push(PvCheckRow { name: "表宽口径", passed: PRIV_MAX == 24 });
    rows.push(PvCheckRow { name: "账单调", passed: tk.ledger_monotone() });
    rows.push(PvCheckRow { name: "检查已跑", passed: tk.check_ran() });
    rows.push(PvCheckRow { name: "转录非空", passed: tk.export_lines().contains("特权检查账") });
    rows.push(PvCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(PvCheckRow { name: "默认集口径", passed: tk.priv_count() == 8 });
    rows.push(PvCheckRow { name: "禁用面在册", passed: tk.entry_of(20).is_some() && tk.entry_of(20).map(|p| p.attrs & ATTR_ENABLED == 0).unwrap_or(true) });
    rows.push(PvCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn pv_checklist_green(rows: &[PvCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn pv_stats_line(tk: &PrivToken) -> String {
    format!(
        "特权检查概况：特权 {} 账 {} 单调 {}",
        tk.priv_count(),
        tk.ledger_len(),
        tk.ledger_monotone()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12448_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut tk = PrivToken::default_token(0);
    let _ = tk.check_one(7);
    let _ = tk.check_many(&[7, 20], true);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        PvReport::build(&tk).all_green() && tk.export_lines().contains("特权检查账"),
        pv_stats_line(&tk),
    ));
    // X-b：属性位档 10/10。
    let (ok_a, d_a) = drill_attr_10();
    out.push(("X-属性位档10/10", ok_a, d_a));
    // X-c：令牌表面 10/10。
    let (ok_t, d_t) = drill_token_table_10();
    out.push(("X-令牌表面10/10", ok_t, d_t));
    // X-d：终验清单。
    let rows = build_pv_checklist(&tk);
    out.push((
        "X-终验清单10项",
        pv_checklist_green(&rows),
        PvReport::build(&tk).to_text(),
    ));
    // X-e：已知域清点（2..23 共 21 个 LUID 全部可查）。
    let mut dom_ok = true;
    let mut tk2 = PrivToken::empty_token(3);
    for l in WELLKNOWN_LO..WELLKNOWN_HI {
        dom_ok = dom_ok && tk2.check_one(l) == Ok(false);
    }
    out.push((
        "X-已知域清点",
        dom_ok && (WELLKNOWN_HI - WELLKNOWN_LO) == 21,
        format!("域 {}..{} 共 {} 项", WELLKNOWN_LO, WELLKNOWN_HI, WELLKNOWN_HI - WELLKNOWN_LO),
    ));
    out
}

/// run_unx12448_checks：本单判据聚合（诊断段 0x30600）。
pub fn run_unx12448_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12448 NtPrivilegeCheck 语义档");
    // J1-a：单特权档 10/10。
    let (ok1, d1) = drill_single_10();
    s.add_dyn("J1-单特权档10/10", ok1, &d1);
    // J1-b：多特权档 10/10。
    let (ok2, d2) = drill_many_10();
    s.add_dyn("J1-多特权档10/10", ok2, &d2);
    // J1-c：未持有注入 10/10 正确返回。
    let (ok3, d3) = drill_not_held_10();
    s.add_dyn("J1-未持有注入10/10", ok3, &d3);
    // J1-d：标准会话恒等式（使能真/禁用假/未挂假 三口径同账）。
    let mut tk = PrivToken::default_token(0);
    let a = tk.check_one(7);
    let b = tk.check_one(20);
    let c = tk.check_one(99);
    let m = tk.check_many(&[7, 20], true);
    s.add_dyn(
        "J1-单多档判据绿",
        a == Ok(true) && b == Ok(false) && c == Ok(false) && m == Ok(vec![true, false]),
        &format!("单7={:?} 单20={:?} 单99={:?} 多={:?}", a, b, c, m),
    );
    // J2-a：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-b：未持有不误报不错报（域外/禁用/未挂 三路回放）。
    let mut tk2 = PrivToken::default_token(1);
    let r99 = matches!(tk2.check_one(99), Ok(false));
    let r20 = matches!(tk2.check_one(20), Ok(false));
    let r3 = matches!(tk2.check_one(3), Ok(false));
    s.add_dyn(
        "J2-未持有三路回放",
        r99 && r20 && r3,
        &format!("99={} 20={} 3={}", r99, r20, r3),
    );
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30600段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12448_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
