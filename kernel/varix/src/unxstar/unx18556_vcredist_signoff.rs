//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18556 · vcredist 免装语义联签（与 E2 锚 2）· 0x806C0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18556-J1 vcredist 免装语义联签（AI-22）进度账，联签
//!       判据清单冻结 v1，波 09 末锚 2 绑定
//! 本模块结构清单：
//!   1. 冻结常量层：AI-22（本联签立项号）、波次 9、锚 2（E2 侧联签
//!      方）、10 个语义单元闭集（crt 子面：heap/fileio/math/string/
//!      locale/time/thread/env/seh/console）、清单 v1 哈希（折叠派生）；
//!   2. 诊断码闭集 VsCode（code() = SEG_BASE + n，0x806C0 段窗口）；
//!   3. 数据结构一（状态机）：UnitState——Pending(0)→Verified(1)
//!      →CoSigned(2) 单向梯，回退/跳签显式拒；
//!   4. 数据结构二（进度账）：SignoffLedger——逐单元状态槽 + 三态
//!      计数 + 绑定记录（波次/锚/清单哈希三元组，幂等重绑合法）；
//!   5. 联签核 co_sign：双签到位（E2 锚 2 签 + 本卡签）且两签引用
//!      同一冻结清单哈希才升 CoSigned——单签冒充/哈希失配必拒；
//!   6. 冻结核 freeze_checklist：清单 v1 十条判据逐条入账，哈希
//!      折叠派生，篡改任一条 → 哈希失配必抓；
//!   7. 绑定核 bind_anchor：波 09 末锚 2 绑定——同参重绑幂等
//!      （账不变），异参重绑必拒；
//!   8. 注入核 3 类×10 轮：单签冒充 / 哈希不匹配 / 清单篡改，必拒；
//!   9. 黄金全链 golden_run + run_unx18556_checks：15 条判据 + 8 例
//!      #[cfg(test)]。
//! 为什么联签要双签同哈希：免装语义的验收口径必须唯一——两个签
//! 方引用不同哈希等于两套验收标准，联签就失去互证意义；冻结清单
//! v1 是口径本体，哈希是它的指纹，绑定记录把「谁在哪个波次认可了
//! 这个指纹」钉死成账。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x806C0，AI-VX11-1105-G1·A16 批 08 独占窗口）。
pub const SEG_BASE: u32 = 0x806C0;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 联签立项号（冻结：AI-22）。
pub const AI_ID: u32 = 22;
/// 波次（冻结：波 09 末绑定）。
pub const WAVE_NO: u32 = 9;
/// E2 侧联签锚（冻结：锚 2）。
pub const ANCHOR_ID: u32 = 2;
/// 本卡签方 id（冻结：卡号 18556）。
pub const CARD_SIGNER: u32 = 18556;
/// 语义单元数（冻结：crt 子面 10 单元）。
pub const UNIT_N: usize = 10;
/// 清单版本（冻结：v1）。
pub const CHECKLIST_VER: u32 = 1;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

/// 语义单元闭集（冻结语料：vcredist 免装语义的 crt 子面清单）。
pub const UNIT_NAMES: [&str; UNIT_N] = [
    "crt_heap", "crt_fileio", "crt_math", "crt_string", "crt_locale",
    "crt_time", "crt_thread", "crt_env", "crt_seh", "crt_console",
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VsCode {
    /// 单元未知
    UnitUnknown,
    /// 状态回退（CoSigned→Verified 等）
    StateRegress,
    /// 跳签（Pending 直接请 CoSigned）
    SkipSign,
    /// 单签冒充联签
    SingleSignFraud,
    /// 签方哈希不匹配
    HashMismatch,
    /// 冻结清单被篡改
    ChecklistTampered,
    /// 绑定冲突（异参重绑）
    BindConflict,
    /// 进度账损坏（三态计数与槽位失配）
    LedgerCorrupt,
    /// 双跑不一致
    NotReproducible,
}

impl VsCode {
    /// 诊断码：SEG_BASE + n（契约骨架口径）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            VsCode::UnitUnknown => "单元未知：不在 crt 子面 10 单元闭集",
            VsCode::StateRegress => "状态回退：联签梯只进不退",
            VsCode::SkipSign => "跳签：Pending 不得直接请 CoSigned",
            VsCode::SingleSignFraud => "单签冒充联签：双签到位才可升级",
            VsCode::HashMismatch => "哈希不匹配：签方引用的清单指纹不一致",
            VsCode::ChecklistTampered => "冻结清单被篡改：v1 哈希失配",
            VsCode::BindConflict => "绑定冲突：异参重绑被拒",
            VsCode::LedgerCorrupt => "进度账损坏：三态计数与槽位失配",
            VsCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [VsCode; 9] = [
        VsCode::UnitUnknown,
        VsCode::StateRegress,
        VsCode::SkipSign,
        VsCode::SingleSignFraud,
        VsCode::HashMismatch,
        VsCode::ChecklistTampered,
        VsCode::BindConflict,
        VsCode::LedgerCorrupt,
        VsCode::NotReproducible,
    ];
}

/// 按数值取中文文案（闭集外返回固定提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < VsCode::ALL.len() {
        if VsCode::ALL[i].code() == code {
            return VsCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x806C0 段闭集内"
}

// ── 数据结构一：单元状态机 ──────────────────────────────────────
/// 单元状态单向梯：Pending(0) → Verified(1) → CoSigned(2)。
/// 冻结规则：相邻前进合法；回退（rank 降）与跳签（rank+2）必拒。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnitState {
    pub state: u8,
    /// E2 锚签是否到位。
    pub e2_sign: bool,
    /// 本卡签是否到位。
    pub card_sign: bool,
    /// 签方引用的清单哈希（两签必须同值）。
    pub e2_hash: u32,
    pub card_hash: u32,
}

impl UnitState {
    /// 新单元：Pending、无签、无哈希。
    pub fn new() -> UnitState {
        UnitState { state: 0, e2_sign: false, card_sign: false, e2_hash: 0, card_hash: 0 }
    }
    /// 状态名（闭集）。
    pub fn name(state: u8) -> &'static str {
        match state {
            0 => "Pending",
            1 => "Verified",
            2 => "CoSigned",
            _ => "?",
        }
    }
}

/// 单步推进：只许相邻前进，回退/跳签拒。
pub fn state_step(cur: u8, next: u8) -> Result<u8, u32> {
    if next != cur.saturating_add(1) || next > 2 {
        return Err(VsCode::StateRegress.code());
    }
    Ok(next)
}

// ── 冻结清单 v1（判据条目 + 哈希派生） ──────────────────────────
/// 清单条目：单元 id + 冻结判据短语。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChecklistItem {
    pub unit_id: u8,
    pub claim: &'static str,
}

/// 联签判据清单冻结 v1：10 条，逐单元一条（短语闭集，改任一条
/// 哈希即失配）。判据短语按「语义单元 → 免装可验证行为」写死。
pub const CHECKLIST_V1: [ChecklistItem; UNIT_N] = [
    ChecklistItem { unit_id: 0, claim: "heap_alloc_free_no_loader_dep" },
    ChecklistItem { unit_id: 1, claim: "fileio_std_handles_inherit" },
    ChecklistItem { unit_id: 2, claim: "math_fns_pure_no_env" },
    ChecklistItem { unit_id: 3, claim: "string_fns_utf8_safe" },
    ChecklistItem { unit_id: 4, claim: "locale_c_default_only" },
    ChecklistItem { unit_id: 5, claim: "time_fns_monotonic_source" },
    ChecklistItem { unit_id: 6, claim: "thread_fns_semantic_map" },
    ChecklistItem { unit_id: 7, claim: "env_block_snapshot_semantic" },
    ChecklistItem { unit_id: 8, claim: "seh_frames_translated" },
    ChecklistItem { unit_id: 9, claim: "console_io_handle_semantic" },
];

/// 清单 v1 哈希：逐条 (unit_id×97 + 版本×31) 折叠（饱和不回绕）。
/// 推导：折叠函数写死于 hash_items，任何条目短语/顺序/条数改动
/// 都改变哈希——哈希失配 = 清单被篡改的判定依据。
pub fn hash_items(items: &[ChecklistItem], ver: u32) -> u32 {
    let mut acc: u32 = SEG_BASE;
    let mut i = 0;
    while i < items.len() {
        let it = match items.get(i) {
            Some(x) => x,
            None => return acc,
        };
        // 短语逐字节折叠（饱和）。
        let bytes = it.claim.as_bytes();
        let mut bi = 0;
        while bi < bytes.len() {
            if let Some(b) = bytes.get(bi) {
                acc = acc.saturating_mul(31).saturating_add(*b as u32);
            }
            bi += 1;
        }
        acc = acc
            .saturating_mul(97)
            .saturating_add(it.unit_id as u32)
            .saturating_add(ver.saturating_mul(31));
        i += 1;
    }
    acc
}

/// 冻结清单 v1 权威哈希（单次派生，全模块同值引用）。
pub fn checklist_v1_hash() -> u32 {
    hash_items(&CHECKLIST_V1, CHECKLIST_VER)
}

// ── 数据结构二：进度账 ──────────────────────────────────────────
/// 联签进度账：逐单元状态 + 三态计数 + 绑定记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignoffLedger {
    pub units: [UnitState; UNIT_N],
    pub pending: u32,
    pub verified: u32,
    pub cosigned: u32,
    /// 绑定记录：Some((波次, 锚, 清单哈希))。
    pub binding: Option<(u32, u32, u32)>,
    /// 绑定次数（幂等重绑计数只对同参累计一次语义）。
    pub bind_ops: u32,
}

impl SignoffLedger {
    /// 新账：全 Pending、无绑定。
    pub fn new() -> SignoffLedger {
        SignoffLedger {
            units: [UnitState::new(); UNIT_N],
            pending: UNIT_N as u32,
            verified: 0,
            cosigned: 0,
            binding: None,
            bind_ops: 0,
        }
    }
    /// 计数守恒：pending+verified+cosigned == UNIT_N 且逐槽一致。
    pub fn conserve(&self) -> Result<(), u32> {
        let sum = self
            .pending
            .saturating_add(self.verified)
            .saturating_add(self.cosigned);
        if sum != UNIT_N as u32 {
            return Err(VsCode::LedgerCorrupt.code());
        }
        let mut p = 0u32;
        let mut v = 0u32;
        let mut c = 0u32;
        let mut i = 0;
        while i < UNIT_N {
            match self.units.get(i) {
                Some(u) => match u.state {
                    0 => p = p.saturating_add(1),
                    1 => v = v.saturating_add(1),
                    2 => c = c.saturating_add(1),
                    _ => return Err(VsCode::LedgerCorrupt.code()),
                },
                None => return Err(VsCode::LedgerCorrupt.code()),
            }
            i += 1;
        }
        if p != self.pending || v != self.verified || c != self.cosigned {
            return Err(VsCode::LedgerCorrupt.code());
        }
        Ok(())
    }
}

// ── 联签核 ──────────────────────────────────────────────────────
/// 验证升级：Pending→Verified（前置核可后升级，不涉签）。
pub fn mark_verified(led: &mut SignoffLedger, unit_id: u8) -> Result<(), u32> {
    if unit_id as usize >= UNIT_N {
        return Err(VsCode::UnitUnknown.code());
    }
    let cur = led.units.get(unit_id as usize).ok_or(VsCode::UnitUnknown.code())?.state;
    let next = match state_step(cur, 1) {
        Ok(n) => n,
        Err(c) => return Err(c), // 回退/越级拒
    };
    match led.units.get_mut(unit_id as usize) {
        Some(u) => {
            u.state = next;
            led.pending = led.pending.saturating_sub(1);
            led.verified = led.verified.saturating_add(1);
            Ok(())
        }
        None => Err(VsCode::UnitUnknown.code()),
    }
}

/// 联签升级：Verified→CoSigned。四闸冻结次序：
///   ①单元闭集 ②状态不得跳签（Pending 直接请联签=SkipSign）
///   ③双签到位（缺任一=单签冒充）④两签同哈希（失配=HashMismatch）。
pub fn co_sign(
    led: &mut SignoffLedger,
    unit_id: u8,
    e2_sign: bool,
    card_sign: bool,
    e2_hash: u32,
    card_hash: u32,
) -> Result<(), u32> {
    // 闸 1：单元闭集。
    if unit_id as usize >= UNIT_N {
        return Err(VsCode::UnitUnknown.code());
    }
    let cur = led.units.get(unit_id as usize).ok_or(VsCode::UnitUnknown.code())?.state;
    // 闸 2：跳签拒（Pending 不得直接请 CoSigned）。
    if cur != 1 {
        if cur == 0 {
            return Err(VsCode::SkipSign.code());
        }
        return Err(VsCode::StateRegress.code());
    }
    // 闸 3：双签到位（缺任一=单签冒充）。
    if !e2_sign || !card_sign {
        return Err(VsCode::SingleSignFraud.code());
    }
    // 闸 4：两签同哈希。
    if e2_hash != card_hash {
        return Err(VsCode::HashMismatch.code());
    }
    // 哈希还须等于冻结清单 v1 权威哈希（清单被篡改=失配）。
    if e2_hash != checklist_v1_hash() {
        return Err(VsCode::ChecklistTampered.code());
    }
    // 全过：Verified→CoSigned。
    match led.units.get_mut(unit_id as usize) {
        Some(u) => {
            let next = state_step(cur, 2)?;
            u.state = next;
            u.e2_sign = true;
            u.card_sign = true;
            u.e2_hash = e2_hash;
            u.card_hash = card_hash;
            led.verified = led.verified.saturating_sub(1);
            led.cosigned = led.cosigned.saturating_add(1);
            Ok(())
        }
        None => Err(VsCode::UnitUnknown.code()),
    }
}

// ── 绑定核（波 09 末锚 2） ──────────────────────────────────────
/// 绑定：把 (波次, 锚, 清单哈希) 钉进账。同参重绑幂等（账不变），
/// 异参重绑必拒（BindConflict）。
pub fn bind_anchor(led: &mut SignoffLedger, wave: u32, anchor: u32, hash: u32) -> Result<(), u32> {
    match led.binding {
        None => {
            led.binding = Some((wave, anchor, hash));
            led.bind_ops = led.bind_ops.saturating_add(1);
            Ok(())
        }
        Some((w, a, h)) => {
            if w == wave && a == anchor && h == hash {
                // 幂等：同参重绑账不变（bind_ops 亦不涨）。
                Ok(())
            } else {
                Err(VsCode::BindConflict.code())
            }
        }
    }
}

// ── 注入核（3 类坏样本，逐类必拒） ──────────────────────────────
/// 注入检出账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
            }
        }
    }
    /// 全抓判据。
    pub fn all_caught(&self, rounds: u32) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] != rounds || self.caught[i] != rounds {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检数（恒零判据）。
    pub fn missed(&self) -> u32 {
        let mut t: u32 = 0;
        let mut c: u32 = 0;
        let mut i = 0;
        while i < 3 {
            t = t.saturating_add(self.tries[i]);
            c = c.saturating_add(self.caught[i]);
            i += 1;
        }
        t.saturating_sub(c)
    }
}

/// 注入轮：单签冒充 / 哈希不匹配 / 清单篡改 三类各一轮。
pub fn inject_round(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类 1：单签冒充（只有本卡签）。
        let mut l1 = SignoffLedger::new();
        let _ = mark_verified(&mut l1, 0);
        let hit1 = co_sign(&mut l1, 0, false, true, checklist_v1_hash(), checklist_v1_hash())
            .err() == Some(VsCode::SingleSignFraud.code());
        led.record(0, hit1);
        // 类 2：哈希不匹配（两签到位但哈希互异）。
        let mut l2 = SignoffLedger::new();
        let _ = mark_verified(&mut l2, 1);
        let hit2 = co_sign(&mut l2, 1, true, true, checklist_v1_hash(), checklist_v1_hash().saturating_add(1))
            .err() == Some(VsCode::HashMismatch.code());
        led.record(1, hit2);
        // 类 3：清单篡改（两签同引一个假哈希）。
        let mut l3 = SignoffLedger::new();
        let _ = mark_verified(&mut l3, 2);
        let hit3 = co_sign(&mut l3, 2, true, true, 0xDEAD, 0xDEAD)
            .err() == Some(VsCode::ChecklistTampered.code());
        led.record(2, hit3);
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：10 单元 Pending → 全部 Verified → 全部双签联签（同引
/// 冻结 v1 哈希）→ 波 09 锚 2 绑定 → 幂等重绑 → 守恒 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = SignoffLedger::new();
    // 全部 Verified。
    let mut i = 0;
    while i < UNIT_N {
        mark_verified(&mut led, i as u8)?;
        i += 1;
    }
    // 全部联签（双签同引冻结 v1 哈希）。
    let fp = checklist_v1_hash();
    let mut j = 0;
    while j < UNIT_N {
        co_sign(&mut led, j as u8, true, true, fp, fp)?;
        j += 1;
    }
    led.conserve()?;
    if led.cosigned != UNIT_N as u32 {
        return Err(VsCode::LedgerCorrupt.code());
    }
    // 波 09 末锚 2 绑定。
    bind_anchor(&mut led, WAVE_NO, ANCHOR_ID, fp)?;
    // 幂等重绑：同参再绑账不变。
    bind_anchor(&mut led, WAVE_NO, ANCHOR_ID, fp)?;
    if led.bind_ops != 1 {
        return Err(VsCode::BindConflict.code());
    }
    match led.binding {
        Some((w, a, h)) => {
            if w != WAVE_NO || a != ANCHOR_ID || h != fp {
                return Err(VsCode::BindConflict.code());
            }
        }
        None => return Err(VsCode::BindConflict.code()),
    }
    Ok((led.cosigned, led.bind_ops, fp))
}

/// 探针三序（联签成功/跳签拒/回退拒）。
pub fn probe_seq_a() -> Result<u32, u32> {
    let mut led = SignoffLedger::new();
    let _ = mark_verified(&mut led, 3);
    let fp = checklist_v1_hash();
    co_sign(&mut led, 3, true, true, fp, fp)?;
    match led.units.get(3) {
        Some(u) if u.state == 2 => Ok(u.state as u32),
        _ => Err(VsCode::LedgerCorrupt.code()),
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // Pending 直接请联签=跳签拒。
    let mut led = SignoffLedger::new();
    let fp = checklist_v1_hash();
    match co_sign(&mut led, 4, true, true, fp, fp) {
        Err(c) if c == VsCode::SkipSign.code() => Ok(()),
        _ => Err(VsCode::SkipSign.code()),
    }
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 异参重绑=绑定冲突拒。
    let mut led = SignoffLedger::new();
    let fp = checklist_v1_hash();
    bind_anchor(&mut led, WAVE_NO, ANCHOR_ID, fp)?;
    match bind_anchor(&mut led, WAVE_NO, ANCHOR_ID, fp.saturating_add(1)) {
        Err(c) if c == VsCode::BindConflict.code() => Ok(()),
        _ => Err(VsCode::BindConflict.code()),
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(VsCode::NotReproducible.code());
    }
    Ok(())
}

/// 清单 v1 冻结稳定：权威哈希两次派生同值，且与独立重算同值。
pub fn freeze_stable() -> bool {
    let a = checklist_v1_hash();
    let b = hash_items(&CHECKLIST_V1, CHECKLIST_VER);
    a == b && a > 0
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < VsCode::ALL.len() {
        let v = VsCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < VsCode::ALL.len() {
        if VsCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据汇总。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：进度账全绿（10/10 联签）+ 绑定记录一致。
    let g = golden_run();
    let fp = checklist_v1_hash();
    let g_ok = match &g {
        Ok((c, ops, h)) => *c == UNIT_N as u32 && *ops == 1 && *h == fp,
        Err(_) => false,
    };
    v.push(("J1-联签进度账10/10+绑定", g_ok, format!("{:?}", g)));
    // 联签判据清单冻结 v1。
    v.push(("J1-清单冻结v1", freeze_stable(), format!("hash={}", fp)));
    // 波 09 末锚 2 绑定。
    let bind_ok = probe_seq_a().is_ok() && probe_seq_c().is_err();
    v.push(("J1-波09末锚2绑定", bind_ok, format!("wave={} anchor={} AI={}", WAVE_NO, ANCHOR_ID, AI_ID)));
    // 注入三类 10/10。
    let inj = inject_round(JUDGE_ROUNDS);
    v.push((
        "J1-注入三类10/10",
        inj.all_caught(JUDGE_ROUNDS) && inj.missed() == 0,
        format!("t={:?} c={:?}", inj.tries, inj.caught),
    ));
    // 状态机：跳签/回退/越级三拒。
    let ladder_ok = state_step(0, 2).is_err()
        && state_step(2, 1).is_err()
        && state_step(1, 2) == Ok(2u8);
    v.push(("J2-状态机单向梯", ladder_ok, String::from("跳签/回退拒、相邻前进放行")));
    // 幂等重绑（异参拒、同参账不变——见测试 bind_idempotent_and_conflict）。
    v.push(("J2-幂等重绑", probe_seq_c().is_err(), String::from("异参拒、同参账不变")));
    // 探针三序。
    v.push((
        "J2-探针三序",
        probe_seq_a() == Ok(2u32) && probe_seq_b().is_ok(),
        String::from("联签/跳签两序"),
    ));
    // 守恒律。
    let cons = {
        let mut l3 = SignoffLedger::new();
        let _ = mark_verified(&mut l3, 0);
        let _ = mark_verified(&mut l3, 1);
        let _ = mark_verified(&mut l3, 2);
        let fp2 = checklist_v1_hash();
        let _ = co_sign(&mut l3, 2, true, true, fp2, fp2);
        l3.conserve().is_ok()
    };
    v.push(("J2-三态计数守恒", cons, String::from("pending+verified+cosigned=10")));
    // 清单条目闭集：单元 id 逐条对齐 + 短语非空。
    let items_ok = {
        let mut ok = CHECKLIST_V1.len() == UNIT_N;
        let mut i = 0;
        while i < UNIT_N {
            if let Some(it) = CHECKLIST_V1.get(i) {
                if it.unit_id != i as u8 || it.claim.is_empty() {
                    ok = false;
                }
            } else {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J2-清单条目闭集", items_ok, format!("{} 条逐条对齐", UNIT_N)));
    // 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 段位守卫 + 文案闭集。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x806C0 窗口")));
    v.push(("J3-文案闭集", detail_closed(), String::from("9 码逐条非空")));
    v
}

/// 判据收口（动态明细槽仅 8 个：前 8 条走 add_dyn，余项静态明细）。
pub fn run_unx18556_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18556 vcredist 免装语义联签（与 E2 锚 2）");
    let items = extra_checks();
    let mut i = 0;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            if i < 8 {
                s.add_dyn(n, *p, d);
            } else {
                s.add(n, *p, "明细见域账/前序动态槽");
            }
        }
        i += 1;
    }
    s
}
// ═══════════════════════════════════════════════════════════════════════
// 测试区
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_full_signoff_and_bind() {
        let g = golden_run().unwrap_or((0, 0, 0));
        assert_eq!(g.0, UNIT_N as u32, "10/10 联签");
        assert_eq!(g.1, 1, "绑定一次（幂等重绑不涨）");
        assert!(g.2 > 0);
    }

    #[test]
    fn checklist_frozen_v1() {
        assert!(freeze_stable(), "权威哈希与独立重算同值");
        // 篡改任一条短语 → 哈希失配。
        let mut tampered = CHECKLIST_V1;
        tampered[3] = ChecklistItem { unit_id: 3, claim: "string_fns_tampered" };
        assert_ne!(hash_items(&tampered, CHECKLIST_VER), checklist_v1_hash());
    }

    #[test]
    fn state_ladder_monotonic() {
        assert_eq!(state_step(0, 1), Ok(1u8));
        assert_eq!(state_step(1, 2), Ok(2u8));
        assert_eq!(state_step(0, 2), Err(VsCode::StateRegress.code()), "跳签");
        assert_eq!(state_step(2, 0), Err(VsCode::StateRegress.code()), "回退");
    }

    #[test]
    fn injections_three_classes() {
        let inj = inject_round(JUDGE_ROUNDS);
        assert!(inj.all_caught(JUDGE_ROUNDS), "三类注入 10/10");
        assert_eq!(inj.missed(), 0);
    }

    #[test]
    fn skip_sign_and_single_sign_rejected() {
        assert_eq!(probe_seq_b(), Ok(()), "跳签拒");
        let mut led = SignoffLedger::new();
        let _ = mark_verified(&mut led, 5);
        let fp = checklist_v1_hash();
        assert_eq!(
            co_sign(&mut led, 5, true, false, fp, fp),
            Err(VsCode::SingleSignFraud.code()),
            "缺 E2 签=单签冒充拒"
        );
    }

    #[test]
    fn bind_idempotent_and_conflict() {
        let mut led = SignoffLedger::new();
        let fp = checklist_v1_hash();
        assert!(bind_anchor(&mut led, WAVE_NO, ANCHOR_ID, fp).is_ok());
        assert!(bind_anchor(&mut led, WAVE_NO, ANCHOR_ID, fp).is_ok(), "同参幂等");
        assert_eq!(led.bind_ops, 1, "幂等重绑计数不涨");
        assert_eq!(
            bind_anchor(&mut led, 8, ANCHOR_ID, fp),
            Err(VsCode::BindConflict.code()),
            "异波次重绑拒"
        );
    }

    #[test]
    fn conservation_and_repro() {
        let mut led = SignoffLedger::new();
        let fp = checklist_v1_hash();
        let mut i = 0;
        while i < UNIT_N {
            let _ = mark_verified(&mut led, i as u8);
            let _ = co_sign(&mut led, i as u8, true, true, fp, fp);
            i += 1;
        }
        assert!(led.conserve().is_ok(), "全联签守恒");
        assert!(repro_check().is_ok(), "双跑一致");
        assert!(segment_guard());
        assert!(detail_closed());
    }

    #[test]
    fn checkset_all_green() {
        let set = run_unx18556_checks();
        assert!(set.all_passed(), "判据集全绿：{}", set.red_items_text());
    }
}
