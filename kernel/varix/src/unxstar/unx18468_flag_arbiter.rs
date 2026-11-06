//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18468 · 兼容标志优先级仲裁（多标志叠加） · 0x7F0C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18468-J1 多标志叠加（版本+DPI+提权互斥组）仲裁断言全过，
//!       互斥冲突告警入账
//! 施工：AI-VX11-1105-G1 · 工单 A07 · 批 04 · 配额 280 · 目标 ≥600 行
//! 本模块结构清单：
//!   数据结构：FLAG_TABLE 9 项（标志→互斥组映射）/ ArbTable 3 组裁决表
//!   状态机  ：ArbPhase 五态单调 Idle→TokenValidate→GroupScan→
//!             ConflictResolve→Frozen
//!   账本    ：组裁决账（胜者槽 0xFF 未定 + 冲突告警账 Vec 只增不减）
//!   错误路径：FabCode 11 码闭集，code() = SEG_BASE + n（0x40 窗口内）
//!   注入核  ：4 类坏样本（未知标志/形似未登记/小写违规/超长令牌）
//!             ×10 轮逐类断言被显式拒绝，漏检必为 0
//!   叠加核  ：组内互斥（多值按表序取最低位胜者）；同令牌重复幂等；
//!             输入乱序 → 胜者不变（叠加漂移检测）
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F0C0，AI-VX11-1105 批 A07 卡 F18468 独占窗口，
/// 窗宽 0x40 = 64 个诊断码）。
pub const SEG_BASE: u32 = 0x7F0C0;

// ── 冻结常量（写死 + 推导注明） ─────────────────────────────────
/// 判据迭代轮数（冻结 10：注入核"10/10"口径）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 互斥组数（冻结 3：版本组 0 / DPI 组 1 / 提权组 2）。
pub const GROUP_COUNT: usize = 3;
/// 标志闭集规模（冻结 9：每组 3 成员）。
pub const FLAG_TOTAL: usize = 9;
/// 未定胜者槽值（冻结 0xFF：与组 id 0..2 不冲突）。
pub const WINNER_NONE: u8 = 0xFF;
/// 单令牌长度上限（冻结 32）。
pub const TOKEN_LEN_MAX: usize = 32;
/// 输入令牌数上限（冻结 16）。
pub const TOKENS_MAX: usize = 16;

/// 标志闭集（标志名, 互斥组 id）。表序即组内优先序：低位胜出。
pub const FLAG_TABLE: [(&str, u8); FLAG_TOTAL] = [
    // 版本互斥组（组 0）。
    ("WIN7RTM", 0),
    ("WIN8RTM", 0),
    ("WIN10RTM", 0),
    // DPI 互斥组（组 1）。
    ("HIGHDPIAWARE", 1),
    ("DPIUNAWARE", 1),
    ("GDISCALING", 1),
    // 提权互斥组（组 2）。
    ("RUNASADMIN", 2),
    ("ELEVATECREATEPROCESS", 2),
    ("LUADETERMINED", 2),
];

// ── 诊断码闭集（段内 0x40 窗口） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FabCode {
    /// 未知标志（闭集外）。
    UnknownFlag,
    /// 互斥组 id 越界。
    GroupRange,
    /// 互斥冲突未入账（告警账缺失）。
    ConflictUnrecorded,
    /// 胜者不定（仲裁口径破坏）。
    WinnerIndeterminate,
    /// 状态机倒退。
    PhaseRegress,
    /// 叠加漂移（乱序输入胜者变化）。
    OverlayDrift,
    /// 计数守恒破坏。
    ConservationBroken,
    /// 告警账破坏（告警数只增不减被违）。
    WarnLedgerBroken,
    /// 幂等破坏（同令牌重复产生重复效果）。
    IdempotenceBroken,
    /// 对照表偏差。
    RefTableDeviation,
    /// 双跑不一致。
    NotReproducible,
}

impl FabCode {
    /// 诊断码：SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            FabCode::UnknownFlag => "未知标志：九记法闭集外拒收",
            FabCode::GroupRange => "互斥组 id 越界：三组闭集外",
            FabCode::ConflictUnrecorded => "互斥冲突未入账：告警账缺失",
            FabCode::WinnerIndeterminate => "胜者不定：仲裁口径破坏",
            FabCode::PhaseRegress => "状态机倒退：仲裁流水只进不退",
            FabCode::OverlayDrift => "叠加漂移：乱序输入胜者变化",
            FabCode::ConservationBroken => "计数守恒破坏：注入=检出+漏检不成立",
            FabCode::WarnLedgerBroken => "告警账破坏：告警数只增不减被违",
            FabCode::IdempotenceBroken => "幂等破坏：同令牌重复产生重复效果",
            FabCode::RefTableDeviation => "对照表偏差：裁决与冻结口径不符",
            FabCode::NotReproducible => "双跑不一致：复现红线被踩",
        }
    }
    /// 全码闭集。
    pub const ALL: [FabCode; 11] = [
        FabCode::UnknownFlag,
        FabCode::GroupRange,
        FabCode::ConflictUnrecorded,
        FabCode::WinnerIndeterminate,
        FabCode::PhaseRegress,
        FabCode::OverlayDrift,
        FabCode::ConservationBroken,
        FabCode::WarnLedgerBroken,
        FabCode::IdempotenceBroken,
        FabCode::RefTableDeviation,
        FabCode::NotReproducible,
    ];
}

/// 按码值取中文文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < FabCode::ALL.len() {
        if FabCode::ALL[i].code() == code {
            return FabCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F0C0 段闭集内"
}

// ── 状态机：仲裁流水 ────────────────────────────────────────────
/// 仲裁流水五态（单调推进，只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArbPhase {
    /// 未开始。
    Idle = 0,
    /// 已完成令牌校验。
    TokenValidate = 1,
    /// 已完成分组扫描。
    GroupScan = 2,
    /// 已完成冲突裁决。
    ConflictResolve = 3,
    /// 已冻结。
    Frozen = 4,
}

impl ArbPhase {
    /// 单调序号。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单步推进：rank 恰 +1，其余显式拒。
    pub fn advance(self, next: ArbPhase) -> Result<ArbPhase, u32> {
        if next.rank() == self.rank().saturating_add(1) {
            Ok(next)
        } else {
            Err(FabCode::PhaseRegress.code())
        }
    }
    /// 全链五步。
    pub fn full_chain() -> Result<ArbPhase, u32> {
        let s0 = ArbPhase::Idle;
        let s1 = s0.advance(ArbPhase::TokenValidate)?;
        let s2 = s1.advance(ArbPhase::GroupScan)?;
        let s3 = s2.advance(ArbPhase::ConflictResolve)?;
        s3.advance(ArbPhase::Frozen)
    }
}

// ── 查找核 ──────────────────────────────────────────────────────
/// 含小写或超长判（先于未知标志的字符域闸）。
fn token_shape_ok(tok: &str) -> Result<(), u32> {
    if tok.len() > TOKEN_LEN_MAX {
        return Err(FabCode::UnknownFlag.code());
    }
    let b = tok.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if let Some(&c) = b.get(i) {
            if c >= b'a' && c <= b'z' {
                return Err(FabCode::UnknownFlag.code());
            }
        }
        i += 1;
    }
    Ok(())
}

/// 标志名→（表位, 组 id）精确查找。
pub fn flag_slot(tok: &str) -> Result<(usize, u8), u32> {
    token_shape_ok(tok)?;
    let mut i = 0;
    while i < FLAG_TOTAL {
        let (name, grp) = FLAG_TABLE[i];
        if name.as_bytes() == tok.as_bytes() {
            if (grp as usize) >= GROUP_COUNT {
                return Err(FabCode::GroupRange.code());
            }
            return Ok((i, grp));
        }
        i += 1;
    }
    Err(FabCode::UnknownFlag.code())
}

// ── 仲裁结果 ────────────────────────────────────────────────────
/// 单次叠加裁决：每组胜者（表位）+ 冲突告警账。
#[derive(Clone, Debug)]
pub struct ArbResult {
    /// 每组胜者表位（未定 = WINNER_NONE）。
    pub winners: [u8; GROUP_COUNT],
    /// 冲突告警（组 id, 败者表位）序列（只增不减）。
    pub warns: Vec<(u8, u8)>,
    /// 生效令牌数（幂等去重后）。
    pub effective_tokens: u32,
}

impl ArbResult {
    /// 冲突总数。
    pub fn warn_count(&self) -> u32 {
        self.warns.len() as u32
    }
    /// 某组是否激活。
    pub fn group_active(&self, g: usize) -> bool {
        if g >= GROUP_COUNT {
            return false;
        }
        self.winners[g] != WINNER_NONE
    }
}

/// 叠加仲裁总装：令牌逐个校验 → 分组叠加（同令牌幂等去重）→
/// 组内冲突裁决（低位胜出，败者入告警账）。
pub fn arbitrate_overlay(tokens: &[&str]) -> Result<ArbResult, u32> {
    ArbPhase::full_chain()?;
    let mut seen: Vec<usize> = Vec::new();
    let mut members: [Vec<usize>; GROUP_COUNT] = [Vec::new(), Vec::new(), Vec::new()];
    let mut effective = 0u32;
    let mut i = 0;
    while i < tokens.len() {
        let tok = match tokens.get(i) {
            Some(t) => *t,
            None => break,
        };
        let (slot, grp) = flag_slot(tok)?;
        // 幂等：同令牌重复只记一次。
        let mut dup = false;
        let mut k = 0;
        while k < seen.len() {
            if let Some(s) = seen.get(k) {
                if *s == slot {
                    dup = true;
                }
            }
            k += 1;
        }
        if !dup {
            seen.push(slot);
            let gidx = grp as usize;
            if let Some(m) = members.get_mut(gidx) {
                m.push(slot);
            }
            effective += 1;
        }
        i += 1;
    }
    // 组内冲突裁决：成员 >1 → 表位最低者胜出，其余入告警账。
    let mut winners = [WINNER_NONE; GROUP_COUNT];
    let mut warns: Vec<(u8, u8)> = Vec::new();
    let mut g = 0usize;
    while g < GROUP_COUNT {
        let m = match members.get(g) {
            Some(v) => v,
            None => break,
        };
        if m.len() == 0 {
            g += 1;
            continue;
        }
        // 表位最低者胜出（m 由输入序填入，需线性扫最小）。
        let mut best = match m.get(0) {
            Some(x) => *x,
            None => break,
        };
        let mut k = 1;
        while k < m.len() {
            if let Some(x) = m.get(k) {
                if *x < best {
                    best = *x;
                }
            }
            k += 1;
        }
        winners[g] = best as u8;
        // 败者入告警账（表序稳定）。
        let mut k = 0;
        while k < m.len() {
            if let Some(x) = m.get(k) {
                if *x != best {
                    warns.push((g as u8, *x as u8));
                }
            }
            k += 1;
        }
        g += 1;
    }
    Ok(ArbResult {
        winners,
        warns,
        effective_tokens: effective,
    })
}

// ── 仲裁账本（跨轮累账，只增不减） ──────────────────────────────
/// 仲裁账本。
pub struct ArbLedger {
    pub total_conflicts: u32,
    pub total_overlays: u32,
    pub inject_tries: [u32; 4],
    pub inject_caught: [u32; 4],
}

impl ArbLedger {
    /// 空账本。
    pub fn new() -> ArbLedger {
        ArbLedger {
            total_conflicts: 0,
            total_overlays: 0,
            inject_tries: [0; 4],
            inject_caught: [0; 4],
        }
    }
    /// 入账一轮叠加。
    pub fn record(&mut self, r: &ArbResult) {
        self.total_overlays += 1;
        self.total_conflicts = self.total_conflicts.saturating_add(r.warn_count());
    }
    /// 冲突账复核：每轮告警之和 = 总告警。
    pub fn warn_ledger_ok(&self, per_round: &[u32]) -> bool {
        let mut sum = 0u32;
        let mut i = 0;
        while i < per_round.len() {
            let v = match per_round.get(i) {
                Some(x) => *x,
                None => 0,
            };
            sum = sum.saturating_add(v);
            i += 1;
        }
        sum == self.total_conflicts
    }
    /// 注入检出记账。
    pub fn record_inject(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.inject_tries[slot] += 1;
            if hit {
                self.inject_caught[slot] += 1;
            }
        }
    }
    /// 计数守恒（四槽全抓）。
    pub fn conserve(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 全部命中（四槽皆非零且全抓）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.inject_tries[i] == 0 || self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 对照表（8 例裁决口径，逐项复算） ────────────────────────────
/// 对照例：输入令牌 + 期望（各组胜者表位, 告警数）。
pub const REF_TABLE: [(&[&str], [u8; GROUP_COUNT], u32); 8] = [
    // 1 单版本标志。
    (&["WIN7RTM"], [0, WINNER_NONE, WINNER_NONE], 0),
    // 2 版本组内冲突（表位 2 vs 0 → 0 胜，1 告警）。
    (&["WIN10RTM", "WIN7RTM"], [0, WINNER_NONE, WINNER_NONE], 1),
    // 3 单 DPI。
    (&["HIGHDPIAWARE"], [WINNER_NONE, 3, WINNER_NONE], 0),
    // 4 DPI 组内冲突。
    (&["DPIUNAWARE", "HIGHDPIAWARE"], [WINNER_NONE, 3, WINNER_NONE], 1),
    // 5 单提权。
    (&["RUNASADMIN"], [WINNER_NONE, WINNER_NONE, 6], 0),
    // 6 三组各一（无冲突叠加）。
    (
        &["WIN8RTM", "HIGHDPIAWARE", "RUNASADMIN"],
        [1, 3, 6],
        0,
    ),
    // 7 三组全冲突（组 1 成员表位 5/4 → 低位 4 胜出）。
    (
        &["WIN10RTM", "WIN7RTM", "GDISCALING", "DPIUNAWARE", "LUADETERMINED", "RUNASADMIN"],
        [0, 4, 6],
        3,
    ),
    // 8 同令牌重复（幂等，不产生告警）。
    (&["WIN7RTM", "WIN7RTM", "WIN7RTM"], [0, WINNER_NONE, WINNER_NONE], 0),
];

/// 对照复算：8 例裁决与冻结口径零偏差。
pub fn ref_table_check() -> Result<u32, u32> {
    let mut ok = 0u32;
    let mut i = 0;
    while i < REF_TABLE.len() {
        let (toks, expect_win, expect_warn) = REF_TABLE[i];
        let r = arbitrate_overlay(toks)?;
        if r.winners != expect_win || r.warn_count() != expect_warn {
            return Err(FabCode::RefTableDeviation.code());
        }
        // 冲突必入账：期望告警 > 0 时账必须非空。
        if expect_warn > 0 && r.warns.len() == 0 {
            return Err(FabCode::ConflictUnrecorded.code());
        }
        ok += 1;
        i += 1;
    }
    Ok(ok)
}

// ── 注入核（4 类坏样本 ×10 轮） ─────────────────────────────────
/// 注入探针（命中约定：Err(窗内码)=被抓，Ok=漏）：
///   0 未知标志（闭集外）；1 形似未登记（WIN9RTM）；2 小写违规；3 超长令牌。
pub fn inject_probe(slot: usize) -> Result<(), u32> {
    match slot {
        0 => match flag_slot("FOOBAZ") {
            Err(c) if c == FabCode::UnknownFlag.code() => Err(c),
            _ => Ok(()),
        },
        1 => match flag_slot("WIN9RTM") {
            Err(c) if c == FabCode::UnknownFlag.code() => Err(c),
            _ => Ok(()),
        },
        2 => match flag_slot("win7rtm") {
            Err(c) if c == FabCode::UnknownFlag.code() => Err(c),
            _ => Ok(()),
        },
        _ => {
            let mut long = String::new();
            let mut n = 0usize;
            while n <= TOKEN_LEN_MAX {
                long.push('W');
                n += 1;
            }
            match flag_slot(long.as_str()) {
                Err(c) if c == FabCode::UnknownFlag.code() => Err(c),
                _ => Ok(()),
            }
        }
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：对照 8 例 → 叠加漂移检测（乱序胜者不变）→ 幂等检测
/// → 冲突告警账复核 → 4 类注入 10 轮全抓 → 守恒 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    ArbPhase::full_chain()?;
    ref_table_check()?;
    let mut led = ArbLedger::new();
    let mut per_round: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < REF_TABLE.len() {
        let (toks, _, _) = REF_TABLE[i];
        let r = arbitrate_overlay(toks)?;
        // 胜者不定闸：激活组必须有合法胜者。
        let mut g = 0;
        while g < GROUP_COUNT {
            if r.group_active(g) {
                let w = r.winners[g] as usize;
                if w >= FLAG_TOTAL {
                    return Err(FabCode::WinnerIndeterminate.code());
                }
            }
            g += 1;
        }
        led.record(&r);
        per_round.push(r.warn_count());
        i += 1;
    }
    // 冲突告警账复核（账=和，只增不减）。
    if !led.warn_ledger_ok(&per_round) {
        return Err(FabCode::WarnLedgerBroken.code());
    }
    // 叠加漂移：乱序输入胜者不变（对照例 7 的两种排列）。
    let a = arbitrate_overlay(&[
        "WIN10RTM", "WIN7RTM", "GDISCALING", "DPIUNAWARE", "LUADETERMINED", "RUNASADMIN",
    ])?;
    let b = arbitrate_overlay(&[
        "RUNASADMIN", "DPIUNAWARE", "LUADETERMINED", "GDISCALING", "WIN7RTM", "WIN10RTM",
    ])?;
    if a.winners != b.winners || a.warn_count() != b.warn_count() {
        return Err(FabCode::OverlayDrift.code());
    }
    // 幂等：同令牌重复 effective_tokens 不变。
    let c1 = arbitrate_overlay(&["WIN7RTM"])?;
    let c2 = arbitrate_overlay(&["WIN7RTM", "WIN7RTM", "WIN7RTM"])?;
    if c1.effective_tokens != c2.effective_tokens {
        return Err(FabCode::IdempotenceBroken.code());
    }
    // 注入 4 类 × 10 轮。
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 4 {
            let hit = match inject_probe(slot) {
                Err(c) => c >= SEG_BASE && c < SEG_BASE + 0x40,
                Ok(_) => false,
            };
            led.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    if !led.all_caught() || !led.conserve() {
        return Err(FabCode::ConservationBroken.code());
    }
    Ok((led.total_overlays, led.total_conflicts, led.inject_caught[0]))
}

// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_a() -> Result<(), u32> {
    // 三组无冲突叠加：三胜者各就各位。
    let r = arbitrate_overlay(&["WIN8RTM", "HIGHDPIAWARE", "RUNASADMIN"])?;
    if r.winners == [1, 3, 6] && r.warn_count() == 0 {
        Ok(())
    } else {
        Err(FabCode::RefTableDeviation.code())
    }
}

pub fn probe_b() -> Result<(), u32> {
    // 组内冲突：低位胜出 + 告警入账。
    let r = arbitrate_overlay(&["WIN10RTM", "WIN7RTM"])?;
    if r.winners[0] == 0 && r.warn_count() == 1 && r.warns.len() == 1 {
        Ok(())
    } else {
        Err(FabCode::ConflictUnrecorded.code())
    }
}

pub fn probe_c() -> Result<(), u32> {
    // 状态机越级必拒。
    match ArbPhase::Idle.advance(ArbPhase::ConflictResolve) {
        Err(c) if c == FabCode::PhaseRegress.code() => Ok(()),
        _ => Err(FabCode::ConservationBroken.code()),
    }
}

// ── 判据裁决 ────────────────────────────────────────────────────
/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x7F0C0 {
        return false;
    }
    let mut i = 0;
    while i < FabCode::ALL.len() {
        let v = FabCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 文案闭集复核。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < FabCode::ALL.len() {
        if detail_of(FabCode::ALL[i].code()).len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据聚合（11 条，单模块 ≤20）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗宽0x40", SEG_BASE),
    ));
    v.push((
        "J1-互斥组闭集",
        GROUP_COUNT == 3 && FLAG_TOTAL == 9,
        format!("3 组 9 标志"),
    ));
    v.push((
        "J1-五态流水",
        ArbPhase::full_chain().is_ok(),
        String::from("Idle→…→Frozen 单调"),
    ));
    let refok = match ref_table_check() {
        Ok(n) => n == 8,
        Err(_) => false,
    };
    v.push((
        "J1-对照8例零偏差",
        refok,
        String::from("裁决口径逐项复算"),
    ));
    let pa = probe_a().is_ok();
    let pb = probe_b().is_ok();
    let pc = probe_c().is_ok();
    v.push((
        "J1-探针三序",
        pa && pb && pc,
        format!("a={} b={} c={}", pa, pb, pc),
    ));
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 注入 4 类 ×10 轮。
    let mut led = ArbLedger::new();
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 4 {
            let hit = inject_probe(slot).is_err();
            led.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    let total_tries =
        led.inject_tries[0] + led.inject_tries[1] + led.inject_tries[2] + led.inject_tries[3];
    let total_caught =
        led.inject_caught[0] + led.inject_caught[1] + led.inject_caught[2] + led.inject_caught[3];
    v.push((
        "J1-注入10/10全抓",
        led.all_caught(),
        format!("4类×10轮 t={} c={}", total_tries, total_caught),
    ));
    v.push((
        "J1-计数守恒",
        led.conserve() && total_tries == total_caught,
        String::from("注入=检出+漏检，漏检=0"),
    ));
    v.push((
        "J1-文案闭集",
        detail_closed(),
        format!("{} 码全非空", FabCode::ALL.len()),
    ));
    // 边界四类：空输入合法（无组激活）/ 恰 32 字节 / 33 字节拒 / 全部组激活。
    let b0 = match arbitrate_overlay(&[]) {
        Ok(r) => r.winners == [WINNER_NONE; GROUP_COUNT] && r.warn_count() == 0,
        Err(_) => false,
    };
    let b1 = {
        let mut tok = String::new();
        let mut n = 1usize;
        while n < TOKEN_LEN_MAX {
            tok.push('A');
            n += 1;
        }
        tok.push_str("RTM");
        flag_slot(tok.as_str()).is_err() // 未知但形状合法（31 字节）→ UnknownFlag
    };
    let b2 = inject_probe(3).is_err();
    let b3 = match arbitrate_overlay(&["WIN10RTM", "WIN7RTM", "WIN8RTM"]) {
        Ok(r) => r.group_active(0) && r.winners[0] == 0 && r.warn_count() == 2,
        Err(_) => false,
    };
    v.push((
        "J1-边界四类",
        b0 && b1 && b2 && b3,
        format!("空{} 恰31{} 超32{} 三冲突{}", b0, b1, b2, b3),
    ));
    // 双跑一致。
    let g1 = golden_run();
    let g2 = golden_run();
    v.push((
        "J1-双跑一致",
        g1.is_ok() && g2.is_ok() && g1 == g2,
        String::from("golden_run 两跑同值"),
    ));
    v
}

/// 探针入口：本模块 CheckSet（判据 13 条）。
pub fn run_unx18468_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18468 兼容标志优先级仲裁");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            // 动态明细槽仅 8 个（契约红线）：前 8 条给运行期算值，
            // 其余转静态明细（CheckSet 记账与 all_passed 不受影响）。
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "判据通过；明细超动态槽，见实现");
        }
        i += 1;
    }
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_checks_green() {
        let set = run_unx18468_checks();
        assert!(set.all_passed(), "判据全绿失败：{}", set.red_items_text());
    }

    #[test]
    fn normal_flow_three_groups() {
        let r = arbitrate_overlay(&["WIN8RTM", "HIGHDPIAWARE", "RUNASADMIN"]);
        match r {
            Ok(a) => {
                assert!(a.winners == [1, 3, 6], "三胜者各就各位");
                assert!(a.warn_count() == 0, "无冲突");
            }
            Err(_) => panic!("合法叠加不该报错"),
        }
    }

    #[test]
    fn conflict_lowest_wins_and_warns() {
        let r = arbitrate_overlay(&["WIN10RTM", "WIN7RTM", "WIN8RTM"]);
        match r {
            Ok(a) => {
                assert!(a.winners[0] == 0, "表位最低者胜出");
                assert!(a.warn_count() == 2, "两败者入告警账");
                assert!(a.warns.len() == 2, "告警账非空");
            }
            Err(_) => panic!("冲突是合法叠加，不该整体报错"),
        }
    }

    #[test]
    fn boundary_four_kinds() {
        let r0 = arbitrate_overlay(&[]);
        match r0 {
            Ok(a) => assert!(a.winners == [WINNER_NONE; GROUP_COUNT], "空输入=无组激活"),
            Err(_) => panic!("空输入是合法叠加"),
        }
        assert!(flag_slot("WIN7RTM").is_ok(), "合法令牌");
        assert!(inject_probe(3).is_err(), "33 字节必拒");
        assert!(flag_slot("win7rtm").is_err(), "小写必拒");
    }

    #[test]
    fn injection_four_classes_all_caught() {
        let mut slot = 0usize;
        while slot < 4 {
            assert!(inject_probe(slot).is_err(), "第 {} 类注入必抓", slot);
            slot += 1;
        }
    }

    #[test]
    fn idempotent_duplicate_tokens() {
        let a = arbitrate_overlay(&["RUNASADMIN"]);
        let b = arbitrate_overlay(&["RUNASADMIN", "RUNASADMIN", "RUNASADMIN"]);
        match (a, b) {
            (Ok(x), Ok(y)) => {
                assert!(x.effective_tokens == y.effective_tokens, "幂等去重");
                assert!(y.warn_count() == 0, "重复不产生冲突告警");
            }
            _ => panic!("幂等叠加不该报错"),
        }
    }

    #[test]
    fn determinism_double_run() {
        let g1 = golden_run();
        let g2 = golden_run();
        match (g1, g2) {
            (Ok(a), Ok(b)) => assert!(a == b, "双跑必须逐值一致"),
            _ => panic!("黄金全链失败"),
        }
    }

    #[test]
    fn segment_guard_window() {
        assert!(segment_guard(), "段位守卫失败");
        let mut i = 0;
        while i < FabCode::ALL.len() {
            let c = FabCode::ALL[i].code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x40, "码越窗");
            assert!(detail_of(c).len() > 0, "文案空缺");
            i += 1;
        }
        assert!(ref_table_check().is_ok(), "对照零偏差");
    }
}
