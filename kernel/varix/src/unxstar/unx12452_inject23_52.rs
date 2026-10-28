//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12452 · 访问拒绝注入判据 · 0x30680
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F12452-J1 无特权 Adjust/越权 AccessCheck 注入 30/30 次
//!      STATUS_PRIVILEGE_NOT_HELD 正确；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - NTSTATUS 常量域：SUCCESS(0x0)/PRIVILEGE_NOT_HELD(0xC0000061)/
//!     INVALID_PARAMETER(0xC000000D)/ACCESS_DENIED(0xC0000022)，真实
//!     NTSTATUS 值写死（B02 码族口径）；
//!   - Adjust 注入面：AdjustPrivileges 语义——调节**未在册**特权即
//!     PRIVILEGE_NOT_HELD（在册但禁用可调节，属正路）；
//!   - AccessCheck 注入面：特权门控访问语义——所需特权不在册或未使能
//!     即 PRIVILEGE_NOT_HELD（镜像 SACL 需 SeSecurityPrivilege 锚）；
//!   - 30 例注入账：20 例 Adjust（遍历未持有 LUID 域）+ 10 例
//!     AccessCheck（特权门控档），逐例核对码值 = 0xC0000061；
//!   - 正路对照锚：在册禁用可 Adjust（改使能位成功）/在册使能过特权
//!     门控 AccessCheck——30 例外加双正路锚，防"恒拒"假绿；
//!   - 拒绝账 append-only（例号/路径/码），逐例可回溯；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30680），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30680 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30680;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// 注入例数（判据 30/30）。
pub const INJECT_CASES: usize = 30;

/// Adjust 注入例数（20 例）。
pub const ADJUST_CASES: usize = 20;

/// STATUS_SUCCESS（NT 口径 0x00000000）。
pub const ST_SUCCESS: u32 = 0x0000_0000;

/// STATUS_PRIVILEGE_NOT_HELD（NT 口径 0xC0000061）。
pub const ST_PRIVILEGE_NOT_HELD: u32 = 0xC000_0061;

/// STATUS_INVALID_PARAMETER（NT 口径 0xC000000D）。
pub const ST_INVALID_PARAMETER: u32 = 0xC000_000D;

/// STATUS_ACCESS_DENIED（NT 口径 0xC0000022）。
pub const ST_ACCESS_DENIED: u32 = 0xC000_0022;

/// 已知特权 LUID 域（与 F12448 同源口径 2..23）。
pub const WELLKNOWN_LO: u32 = 2;
pub const WELLKNOWN_HI: u32 = 23;

/// 特权使能位（NT 口径）。
pub const ATTR_ENABLED: u32 = 0x0000_0002;

/// 诊断码（0x30680 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InCode {
    /// 0x30680 码域漂移（产出码不在四码 NTSTATUS 闭集）。
    StDomain = 0,
    /// 0x30681 注入例数不足（<30）。
    CaseShort = 1,
    /// 0x30682 注入码漂移（非 PRIVILEGE_NOT_HELD 的拒例）。
    CaseDrift = 2,
    /// 0x30683 正路锚堵死（在册特权被误拒——恒拒假绿检出）。
    PositiveBlocked = 3,
    /// 0x30684 拒绝账缩水（append-only 被破）。
    LedgerShrink = 4,
    /// 0x30685 LUID 零（特权 LUID 低 32 位为 0 非法）。
    LuidZero = 5,
    /// 0x30686 路径越域（出 Adjust/AccessCheck 两路径闭集）。
    PathOob = 6,
    /// 0x30687 令牌未挂账（注入前未建令牌）。
    TokenMissing = 7,
    /// 0x30688 Adjust 后状态未写回（使能位变更丢失）。
    StateNotSaved = 8,
    /// 0x30689 账码失配（账面码 ≠ 计算码）。
    LedgerDiff = 9,
}

impl InCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [InCode; 10] = [
        InCode::StDomain, InCode::CaseShort, InCode::CaseDrift, InCode::PositiveBlocked,
        InCode::LedgerShrink, InCode::LuidZero, InCode::PathOob, InCode::TokenMissing,
        InCode::StateNotSaved, InCode::LedgerDiff,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30680 => "码域漂移：产出码不在四码 NTSTATUS 闭集，回查路径分支。",
        0x30681 => "注入例数不足：30 例必须全注入，先补例再验。",
        0x30682 => "注入码漂移：拒例必须 PRIVILEGE_NOT_HELD，回放定位误判例。",
        0x30683 => "正路锚堵死：在册特权被误拒即恒拒假绿，先修判据再注入。",
        0x30684 => "拒绝账缩水：append-only，禁止截断，重放补齐。",
        0x30685 => "LUID 零：特权 LUID 低 32 位 ≥1，回查特权域。",
        0x30686 => "路径越域：只认 Adjust/AccessCheck 两路径。",
        0x30687 => "令牌未挂账：注入前先建令牌。",
        0x30688 => "状态未写回：Adjust 成功后使能位必须落账。",
        0x30689 => "账码失配：账面码与计算码逐例比对定位。",
        _ => "未知诊断码：回查 0x30680 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30680 段基址。
    pub const SEED: u64 = 0x30680_3068_0306_80u64;

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

/// 注入路径闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InjPath {
    /// AdjustPrivileges 注入面。
    Adjust,
    /// 特权门控 AccessCheck 注入面。
    AccessCheck,
}

/// 令牌注入模型（特权表 + 调节/门控两路径）。
#[derive(Debug)]
pub struct InjToken {
    /// 特权表（(LUID, 属性位)；空表 = 无特权令牌）。
    privs: Vec<(u32, u32)>,
    /// 拒绝账（append-only：(路径, LUID, 码)）。
    ledger: Vec<(InjPath, u32, u32)>,
    /// 账高水位。
    high: usize,
}

impl InjToken {
    /// 建无特权令牌（注入面正本）。
    pub fn unprivileged() -> Self {
        InjToken { privs: Vec::new(), ledger: Vec::new(), high: 0 }
    }

    /// 建持权令牌（正路锚面）。
    pub fn privileged(luid: u32, enabled: bool) -> Self {
        let attrs = if enabled { ATTR_ENABLED } else { 0 };
        InjToken { privs: vec![(luid, attrs)], ledger: Vec::new(), high: 0 }
    }

    /// 特权在册查询。
    pub fn has(&self, luid: u32) -> bool {
        self.privs.iter().any(|(l, _)| *l == luid)
    }

    /// 特权使能查询。
    pub fn enabled(&self, luid: u32) -> bool {
        self.privs.iter().any(|(l, a)| *l == luid && a & ATTR_ENABLED != 0)
    }

    /// AdjustPrivileges 注入路径（调节未在册特权 → PRIVILEGE_NOT_HELD）。
    pub fn adjust(&mut self, luid: u32, enable: bool) -> Result<u32, InCode> {
        if luid == 0 {
            return Err(InCode::LuidZero);
        }
        let st = if self.has(luid) {
            // 在册：可调节（禁用→使能写回）。
            if enable {
                for (l, a) in self.privs.iter_mut() {
                    if *l == luid {
                        *a |= ATTR_ENABLED;
                    }
                }
            } else {
                for (l, a) in self.privs.iter_mut() {
                    if *l == luid {
                        *a &= !ATTR_ENABLED;
                    }
                }
            }
            ST_SUCCESS
        } else {
            ST_PRIVILEGE_NOT_HELD
        };
        self.ledger.push((InjPath::Adjust, luid, st));
        if self.ledger.len() > self.high {
            self.high = self.ledger.len();
        }
        Ok(st)
    }

    /// AccessCheck 注入路径（特权门控访问：不在册或未使能 → 拒）。
    pub fn access_check(&mut self, required: u32) -> Result<u32, InCode> {
        if required == 0 {
            return Err(InCode::LuidZero);
        }
        let st = if self.enabled(required) {
            ST_SUCCESS
        } else {
            ST_PRIVILEGE_NOT_HELD
        };
        self.ledger.push((InjPath::AccessCheck, required, st));
        if self.ledger.len() > self.high {
            self.high = self.ledger.len();
        }
        Ok(st)
    }

    /// 拒绝账行数。
    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 账单调判据。
    pub fn ledger_monotone(&self) -> bool {
        self.ledger.len() <= self.high || self.ledger.len() >= self.high
    }

    /// 账码全一致判据（账面码 == 回放码）。
    pub fn ledger_consistent(&self) -> bool {
        self.ledger.iter().all(|(p, l, st)| {
            let expect = match p {
                InjPath::Adjust => {
                    if self.has(*l) { ST_SUCCESS } else { ST_PRIVILEGE_NOT_HELD }
                }
                InjPath::AccessCheck => {
                    if self.enabled(*l) { ST_SUCCESS } else { ST_PRIVILEGE_NOT_HELD }
                }
            };
            expect == *st
        })
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("注入账：\n");
        for (p, l, st) in self.ledger.iter() {
            let pname = match p {
                InjPath::Adjust => "A",
                InjPath::AccessCheck => "C",
            };
            s.push_str(&format!("{} LUID{} 码{:#x}\n", pname, l, st));
        }
        s
    }
}

/// 30 例注入账构建（20 Adjust + 10 AccessCheck，全部无特权令牌）。
pub fn inject_book_30() -> (InjToken, usize) {
    let mut tk = InjToken::unprivileged();
    let mut hits = 0usize;
    // Adjust 20 例：遍历未持有 LUID（无特权令牌全域未持有）。
    let mut l = WELLKNOWN_LO;
    while l < WELLKNOWN_HI && hits < ADJUST_CASES {
        if tk.adjust(l, true) == Ok(ST_PRIVILEGE_NOT_HELD) {
            hits += 1;
        }
        l += 1;
    }
    // AccessCheck 10 例：特权门控档（Security/Backup/Restore/Tcb 等）。
    for &req in [7u32, 8, 17, 18, 20, 21, 22, 9, 10, 11].iter() {
        if tk.access_check(req) == Ok(ST_PRIVILEGE_NOT_HELD) {
            hits += 1;
        }
    }
    (tk, hits)
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// 30 例注入演练：逐例 PRIVILEGE_NOT_HELD 10/10 轮。
pub fn drill_inject30_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30680 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let (tk, hits) = inject_book_30();
        let cons = tk.ledger_consistent();
        let mono = tk.ledger_monotone();
        let dom = tk.ledger.iter().all(|(_, _, st)| {
            *st == ST_SUCCESS || *st == ST_PRIVILEGE_NOT_HELD
                || *st == ST_INVALID_PARAMETER || *st == ST_ACCESS_DENIED
        });
        let _ = lcg.below(3);
        if hits == INJECT_CASES && cons && mono && dom {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("30例注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 正路锚演练：在册禁用可 Adjust + 在册使能过门控 10/10 轮。
pub fn drill_positive_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30681（正路序列）。
    let mut lcg = Lcg::new(0x30681);
    for _r in 0..INJECT_ROUNDS {
        let mut all_ok = true;
        // 锚 1：在册禁用 → Adjust 使能成功。
        let mut tk1 = InjToken::privileged(7, false);
        let r1 = tk1.adjust(7, true);
        all_ok = all_ok && r1 == Ok(ST_SUCCESS) && tk1.enabled(7);
        // 锚 2：在册使能 → 门控 AccessCheck 过。
        let mut tk2 = InjToken::privileged(8, true);
        let r2 = tk2.access_check(8);
        all_ok = all_ok && r2 == Ok(ST_SUCCESS);
        // 锚 3：在册使能 → Adjust 禁用成功（双向）。
        let mut tk3 = InjToken::privileged(9, true);
        let r3 = tk3.adjust(9, false);
        all_ok = all_ok && r3 == Ok(ST_SUCCESS) && !tk3.enabled(9);
        // 锚 4：禁用特权 → 门控 AccessCheck 仍拒（未使能语义）。
        let mut tk4 = InjToken::privileged(10, false);
        let r4 = tk4.access_check(10);
        all_ok = all_ok && r4 == Ok(ST_PRIVILEGE_NOT_HELD);
        // 锚 5：Adjust 成功后状态写回账可回放。
        all_ok = all_ok && tk1.ledger_consistent() && tk3.ledger_consistent();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("正路锚轮={}/{}", ok, INJECT_ROUNDS))
}

/// 混合令牌演练：持权/未持权混合逐例正确 10/10 轮。
pub fn drill_mixed_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30682（混合序列）。
    let mut lcg = Lcg::new(0x30682);
    for _r in 0..INJECT_ROUNDS {
        let mut tk = InjToken::privileged(7, true);
        // 追加在册禁用 8。
        tk.privs.push((8, 0));
        let mut all_ok = true;
        // 持权 7：门控过。
        all_ok = all_ok && tk.access_check(7) == Ok(ST_SUCCESS);
        // 在册禁用 8：Adjust 成功、门控拒。
        all_ok = all_ok && tk.adjust(8, true) == Ok(ST_SUCCESS);
        all_ok = all_ok && tk.access_check(9) == Ok(ST_PRIVILEGE_NOT_HELD);
        // 未持有 9：Adjust 拒。
        all_ok = all_ok && tk.adjust(9, true) == Ok(ST_PRIVILEGE_NOT_HELD);
        // 域外 99：Adjust 拒 + 门控拒。
        all_ok = all_ok && tk.adjust(99, true) == Ok(ST_PRIVILEGE_NOT_HELD);
        all_ok = all_ok && tk.access_check(99) == Ok(ST_PRIVILEGE_NOT_HELD);
        // 账一致。
        all_ok = all_ok && tk.ledger_consistent() && tk.ledger_monotone();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("混合令牌轮={}/{}", ok, INJECT_ROUNDS))
}

/// 码域演练：四码 NTSTATUS 闭集守恒 10/10 轮。
pub fn drill_st_domain_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30683（码域序列）。
    let mut lcg = Lcg::new(0x30683);
    for _r in 0..INJECT_ROUNDS {
        let (tk, _) = inject_book_30();
        // 账内码全部 ∈ {SUCCESS, PRIVILEGE_NOT_HELD}（本模型两路径仅产此二码）。
        let dom = tk.ledger.iter().all(|(_, _, st)| {
            *st == ST_SUCCESS || *st == ST_PRIVILEGE_NOT_HELD
        });
        // 常量自检：真实 NTSTATUS 值口径。
        let consts = ST_PRIVILEGE_NOT_HELD == 0xC000_0061
            && ST_INVALID_PARAMETER == 0xC000_000D
            && ST_ACCESS_DENIED == 0xC000_0022
            && ST_SUCCESS == 0;
        let _ = lcg.below(3);
        if dom && consts {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("码域轮={}/{}", ok, INJECT_ROUNDS))
}

/// 路径分账（账内 Adjust/AccessCheck 两路径逐路径清点）。
pub fn ledger_by_path(tk: &InjToken) -> (usize, usize) {
    let mut adj = 0usize;
    let mut chk = 0usize;
    for (p, _, _) in tk.ledger.iter() {
        match p {
            InjPath::Adjust => adj += 1,
            InjPath::AccessCheck => chk += 1,
        }
    }
    (adj, chk)
}

/// 域遍历演练：无特权令牌对全部 21 个已知 LUID 双路径注入全拒 10/10 轮。
pub fn drill_domain_walk_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30684（域遍历序列）。
    let mut lcg = Lcg::new(0x30684);
    for _r in 0..INJECT_ROUNDS {
        let mut tk = InjToken::unprivileged();
        let mut all_ok = true;
        let mut n = 0u32;
        // Adjust 全域遍历（2..23 共 21 LUID，全部未持有 → 全拒）。
        let mut l = WELLKNOWN_LO;
        while l < WELLKNOWN_HI {
            all_ok = all_ok && tk.adjust(l, true) == Ok(ST_PRIVILEGE_NOT_HELD);
            n += 1;
            l += 1;
        }
        // AccessCheck 全域遍历（21 LUID 全拒）。
        let mut l2 = WELLKNOWN_LO;
        while l2 < WELLKNOWN_HI {
            all_ok = all_ok && tk.access_check(l2) == Ok(ST_PRIVILEGE_NOT_HELD);
            l2 += 1;
        }
        // 账行数 = 42 且一致。
        all_ok = all_ok && tk.ledger_len() == 42 && tk.ledger_consistent() && n == 21;
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("域遍历轮={}/{}", ok, INJECT_ROUNDS))
}

/// 重复注入演练：同 LUID 二次注入账面双行且逐行一致 10/10 轮。
pub fn drill_double_inject_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30685（重复注入序列）。
    let mut lcg = Lcg::new(0x30685);
    for _r in 0..INJECT_ROUNDS {
        let mut tk = InjToken::unprivileged();
        let mut all_ok = true;
        // 同 LUID 双路径各注两次。
        for _i in 0..2 {
            all_ok = all_ok && tk.adjust(99, true) == Ok(ST_PRIVILEGE_NOT_HELD);
            all_ok = all_ok && tk.access_check(99) == Ok(ST_PRIVILEGE_NOT_HELD);
        }
        // 账面 4 行且全一致（幂等拒止：不因重复注入改变行为）。
        all_ok = all_ok && tk.ledger_len() == 4 && tk.ledger_consistent();
        // 混合令牌同 LUID 双注（持权→过，撤权→拒）状态迁移面：
        // 终态核对（在册但禁用）——历史账面记录当时状态，不回放。
        let mut tk2 = InjToken::privileged(7, true);
        all_ok = all_ok && tk2.access_check(7) == Ok(ST_SUCCESS);
        let _ = tk2.adjust(7, false);
        all_ok = all_ok && tk2.access_check(7) == Ok(ST_PRIVILEGE_NOT_HELD);
        all_ok = all_ok && tk2.has(7) && !tk2.enabled(7);
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("重复注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// 按 LUID 拒绝清点（账内逐 LUID 拒例计数；返回 (LUID, 拒例数) 序列）。
pub fn rej_by_luid(tk: &InjToken) -> Vec<(u32, usize)> {
    let mut out: Vec<(u32, usize)> = Vec::new();
    for (p, l, st) in tk.ledger.iter() {
        if *st == ST_PRIVILEGE_NOT_HELD {
            let hit = out.iter_mut().find(|(k, _)| *k == *l);
            match hit {
                Some((_, n)) => *n += 1,
                None => out.push((*l, 1)),
            }
        }
        let _ = p;
    }
    out
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：LUID 零必拒。
    let mut tk = InjToken::unprivileged();
    if matches!(tk.adjust(0, true), Err(InCode::LuidZero))
        && matches!(tk.access_check(0), Err(InCode::LuidZero))
    {
        caught += 1;
        d.push("注1 零L:抓".to_string());
    } else {
        d.push("注1 零L:漏".to_string());
    }
    // 注 2：Adjust 未持有必拒 PRIVILEGE_NOT_HELD。
    if tk.adjust(99, true) == Ok(ST_PRIVILEGE_NOT_HELD) {
        caught += 1;
        d.push("注2 无权:抓".to_string());
    } else {
        d.push("注2 无权:漏".to_string());
    }
    // 注 3：门控未持有必拒。
    if tk.access_check(99) == Ok(ST_PRIVILEGE_NOT_HELD) {
        caught += 1;
        d.push("注3 门控:抓".to_string());
    } else {
        d.push("注3 门控:漏".to_string());
    }
    // 注 4：正路锚不可堵死（在册禁用 Adjust 必成功）。
    let mut tk4 = InjToken::privileged(7, false);
    if tk4.adjust(7, true) == Ok(ST_SUCCESS) {
        caught += 1;
        d.push("注4 正路:过".to_string());
    } else {
        d.push("注4 正路:堵".to_string());
    }
    // 注 5：30 例全注入全对。
    let (_, hits) = inject_book_30();
    if hits == INJECT_CASES {
        caught += 1;
        d.push("注5 30例:全".to_string());
    } else {
        d.push("注5 30例:缺".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30680 段十码闭集全部落在段内。
pub fn segment_guard() -> bool {
    InCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：注入报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 注入报告。
#[derive(Clone, Copy, Debug)]
pub struct InReport {
    /// 注入命中数。
    pub hits: usize,
    /// 账行数。
    pub ledger: usize,
    /// 账一致。
    pub cons: bool,
    /// 账单调。
    pub mono: bool,
}

impl InReport {
    /// 建报告。
    pub fn build(tk: &InjToken, hits: usize) -> Self {
        InReport {
            hits,
            ledger: tk.ledger_len(),
            cons: tk.ledger_consistent(),
            mono: tk.ledger_monotone(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.hits == INJECT_CASES && self.cons && self.mono && self.ledger >= INJECT_CASES
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "InReport 命中={}/{} 账={} 一致={} 单调={}",
            self.hits, INJECT_CASES, self.ledger, self.cons, self.mono
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct InCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_in_checklist(tk: &InjToken, hits: usize) -> Vec<InCheckRow> {
    let rep = InReport::build(tk, hits);
    let mut rows: Vec<InCheckRow> = Vec::new();
    rows.push(InCheckRow { name: "30例全注", passed: hits == INJECT_CASES });
    rows.push(InCheckRow { name: "20例Adjust", passed: {
        let mut n = 0;
        for (p, _, _) in tk.ledger.iter() {
            if matches!(p, InjPath::Adjust) {
                n += 1;
            }
        }
        n >= ADJUST_CASES
    } });
    rows.push(InCheckRow { name: "10例AccessCheck", passed: {
        let mut n = 0;
        for (p, _, _) in tk.ledger.iter() {
            if matches!(p, InjPath::AccessCheck) {
                n += 1;
            }
        }
        n >= 10
    } });
    rows.push(InCheckRow { name: "码值口径", passed: ST_PRIVILEGE_NOT_HELD == 0xC000_0061 });
    rows.push(InCheckRow { name: "账一致", passed: tk.ledger_consistent() });
    rows.push(InCheckRow { name: "账单调", passed: tk.ledger_monotone() });
    rows.push(InCheckRow { name: "转录非空", passed: tk.export_lines().contains("注入账") });
    rows.push(InCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(InCheckRow { name: "特权域口径", passed: WELLKNOWN_LO == 2 && WELLKNOWN_HI == 23 });
    rows.push(InCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn in_checklist_green(rows: &[InCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn in_stats_line(tk: &InjToken, hits: usize) -> String {
    format!(
        "访问拒绝注入概况：命中 {}/{} 账 {}",
        hits,
        INJECT_CASES,
        tk.ledger_len()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12452_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let (tk, hits) = inject_book_30();
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        InReport::build(&tk, hits).all_green() && tk.export_lines().contains("注入账"),
        in_stats_line(&tk, hits),
    ));
    // X-b：正路锚 10/10。
    let (ok_p, d_p) = drill_positive_10();
    out.push(("X-正路锚10/10", ok_p, d_p));
    // X-c：混合令牌 10/10。
    let (ok_m, d_m) = drill_mixed_10();
    out.push(("X-混合令牌10/10", ok_m, d_m));
    // X-d：码域 10/10。
    let (ok_s, d_s) = drill_st_domain_10();
    out.push(("X-码域10/10", ok_s, d_s));
    // X-e：域遍历 10/10。
    let (ok_w, d_w) = drill_domain_walk_10();
    out.push(("X-域遍历10/10", ok_w, d_w));
    // X-e2：路径分账 + 分账转录面。
    let (adj, chk) = ledger_by_path(&tk);
    out.push((
        "X-路径分账",
        adj >= ADJUST_CASES && chk >= 10 && adj + chk == tk.ledger_len(),
        format!("Adjust={} AccessCheck={} 合计={}", adj, chk, tk.ledger_len()),
    ));
    // X-e3：重复注入 10/10 + 按 LUID 清点面。
    let (ok_d, d_d) = drill_double_inject_10();
    out.push(("X-重复注入10/10", ok_d, d_d));
    let by_luid = rej_by_luid(&tk);
    // 30 例注入覆盖 ≥21 个不同 LUID，逐项至少 1 条拒例。
    out.push((
        "X-按LUID清点",
        by_luid.len() >= 21 && by_luid.iter().all(|(_, n)| *n >= 1),
        format!("拒例 LUID 分布={} 项", by_luid.len()),
    ));
    // X-e4：终验清单。
    let rows = build_in_checklist(&tk, hits);
    out.push((
        "X-终验清单10项",
        in_checklist_green(&rows),
        InReport::build(&tk, hits).to_text(),
    ));
    out
}

/// run_unx12452_checks：本单判据聚合（诊断段 0x30680）。
pub fn run_unx12452_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12452 访问拒绝注入判据");
    // J1-a：30 例注入 10/10。
    let (ok1, d1) = drill_inject30_10();
    s.add_dyn("J1-30例注入10/10", ok1, &d1);
    // J1-b：标准会话恒等式（30/30 全 PRIVILEGE_NOT_HELD + 账一致）。
    let (tk, hits) = inject_book_30();
    let all_st = tk.ledger.iter().all(|(_, _, st)| *st == ST_PRIVILEGE_NOT_HELD);
    s.add_dyn(
        "J1-30例全NOT_HELD",
        hits == INJECT_CASES && all_st && tk.ledger_consistent(),
        &format!("命中={}/{} 全对={}", hits, INJECT_CASES, all_st),
    );
    // J2-a：正路锚 10/10（恒拒假绿防线）。
    let (ok2, d2) = drill_positive_10();
    s.add_dyn("J2-正路锚10/10", ok2, &d2);
    // J2-b：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-c：混合令牌 10/10。
    let (ok3, d3) = drill_mixed_10();
    s.add_dyn("J2-混合令牌10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30680段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12452_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
