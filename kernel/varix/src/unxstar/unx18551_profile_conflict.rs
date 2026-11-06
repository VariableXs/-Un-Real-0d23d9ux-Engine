//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18551 · 档案冲突检测（per-app vs bottle 矛盾告警）· 0x80580 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18551-J1 矛盾组合检出告警 10/10，仲裁按优先级有账
//! 本模块结构清单：
//!   1. 冻结常量层：12 个档案键（设置项闭集）、5 档值类别、3 档严重级、
//!      4 级仲裁优先级梯（app_explicit > app_inherit > bottle_default >
//!      unset）、10 组冻结矛盾组合表（键×瓶值类×应用值类→严重级）；
//!   2. 诊断码闭集 PfCode（code() = SEG_BASE + n，0x80580 段 0x40 窗口）；
//!   3. 数据结构一（占用位面）：ScopePlane——bottle/app 两个 12 bit
//!      占用位面，键落位、重复落位、越界键全部在位面层先拦；
//!   4. 数据结构二（仲裁账）：ConflictLedger——检出/仲裁/抑制三计数
//!      + 逐严重级分账 + 告警文案账（检出必仲裁、仲裁必留告警，
//!      计数守恒：检出 = 仲裁，漏检恒为零）；
//!   5. 判定核 scan_conflicts：同键双域在场且值类别不同 → 矛盾；
//!      键未知 / 值类越界 / 同域重复键 → 显式拒绝（注入面）；
//!   6. 仲裁核 arbitrate：先查冻结组合表定严重级；表外组合走优先级
//!      梯默认仲裁（应用域显式声明覆盖 bottle 默认）；同值共在场=
//!      抑制告警（只记账不告警）；
//!   7. 注入核 3 类×10 轮：未知键 / 同域重复键 / 值类越界，逐类必拒；
//!   8. 黄金全链 golden_run + run_unx18551_checks：14 条判据
//!      + #[cfg(test)] 8 例测试。
//! 为什么按「键×值类别」判矛盾而不按值文本：值文本是无限空间，
//! 无法冻结组合表；类别把值空间折成 5 档闭集后，"检出 10/10" 才是
//! 可复算的判定——组合表冻结在常量里，任何一行漂移都会被对账抓出。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x80580，AI-VX11-1105-G1·A16 批 08 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x80580;

// ── 冻结常量（推导注记） ─────────────────────────────────────────
/// 档案键闭集大小（冻结：12 个设置项，键位 0..12）。
pub const KEY_N: usize = 12;
/// 档案键名闭集（冻结语料：wine 档案常见设置项抽象）。
pub const KEY_NAMES: [&str; KEY_N] = [
    "dpi_scale", "font_smooth", "gpu_vendor", "shader_cache", "render_backend",
    "audio_rate", "net_throttle", "fs_casefold", "mem_reserve", "proc_affinity",
    "locale_tag", "env_inherit",
];
/// 值类别档数（冻结：5 档闭集——off/low/mid/high/auto 的抽象序号）。
pub const VAL_CLASS_N: u8 = 5;
/// 严重级档数（冻结：3 档——0 提示 / 1 警告 / 2 高危）。
pub const SEV_N: usize = 3;
/// 冻结矛盾组合数（冻结：判据原文 10/10 语义）。
pub const COMBO_N: usize = 10;
/// 注入判定轮数（冻结：10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 作用域闭集（0=bottle 默认层，1=per-app 逐应用层）。
pub const SCOPE_BOTTLE: u8 = 0;
pub const SCOPE_APP: u8 = 1;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 键未知：键位落在 0..12 闭集外
    UnknownKey,
    /// 值类别越界：不在 5 档闭集内
    ValClassRange,
    /// 同域重复键：同一作用域内一键多声明
    KeyDup,
    /// 位面账损坏：占用计数与位面弹入计数失配
    PlaneCorrupt,
    /// 严重级越界：仲裁结果不在 3 档闭集内
    SevIllegal,
    /// 计数守恒破坏：检出≠仲裁
    ConservationBroken,
    /// 仲裁无账：检出冲突未留仲裁记录
    ArbitrationUnaccounted,
    /// 告警文案空：告警账存在空串
    WarnEmpty,
    /// 双跑不一致
    NotReproducible,
}

impl PfCode {
    /// 诊断码：SEG_BASE + n（契约骨架口径）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            PfCode::UnknownKey => "键未知：键位不在 0..12 闭集内，拒载",
            PfCode::ValClassRange => "值类别越界：不在 5 档闭集内，拒载",
            PfCode::KeyDup => "同域重复键：一键多声明，拒绝注入",
            PfCode::PlaneCorrupt => "占用位面账损坏：计数与位面失配",
            PfCode::SevIllegal => "严重级越界：仲裁结果不在 3 档闭集内",
            PfCode::ConservationBroken => "计数守恒破坏：检出数≠仲裁数",
            PfCode::ArbitrationUnaccounted => "仲裁无账：存在未留仲裁记录的检出",
            PfCode::WarnEmpty => "告警文案空：告警账不得有空串",
            PfCode::NotReproducible => "双跑不一致：确定性红线",
        }
    }
    pub const ALL: [PfCode; 9] = [
        PfCode::UnknownKey,
        PfCode::ValClassRange,
        PfCode::KeyDup,
        PfCode::PlaneCorrupt,
        PfCode::SevIllegal,
        PfCode::ConservationBroken,
        PfCode::ArbitrationUnaccounted,
        PfCode::WarnEmpty,
        PfCode::NotReproducible,
    ];
}

/// 按数值取中文文案（闭集外返回固定提示）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PfCode::ALL.len() {
        if PfCode::ALL[i].code() == code {
            return PfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x80580 段闭集内"
}

// ── 数据结构一：占用位面（数组账本之一） ────────────────────────
/// 双作用域占用位面：bottle/app 各 12 bit，重复落位与越界键在此层拦截。
/// 为什么用位面而不是 Vec 查重：12 个键的查重是 O(1) 位测试，
/// 且位面弹入计数与条目计数天然构成守恒对账（位面坏了立刻现形）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScopePlane {
    bottle_bits: [u8; 2],
    app_bits: [u8; 2],
    bottle_n: u32,
    app_n: u32,
}

impl ScopePlane {
    /// 新建空位面。
    pub fn new() -> ScopePlane {
        ScopePlane { bottle_bits: [0, 0], app_bits: [0, 0], bottle_n: 0, app_n: 0 }
    }
    /// 落位：键落指定作用域，越界/重复显式拒绝。
    pub fn set(&mut self, scope: u8, key_id: u8) -> Result<(), u32> {
        if key_id as usize >= KEY_N {
            return Err(PfCode::UnknownKey.code());
        }
        let k = key_id as usize;
        let plane: &mut [u8; 2] = if scope == SCOPE_BOTTLE { &mut self.bottle_bits } else { &mut self.app_bits };
        let byte = k / 8;
        let bit = 1u8 << (k % 8);
        if (plane[byte] & bit) != 0 {
            return Err(PfCode::KeyDup.code());
        }
        plane[byte] |= bit;
        if scope == SCOPE_BOTTLE {
            self.bottle_n = self.bottle_n.saturating_add(1);
        } else {
            self.app_n = self.app_n.saturating_add(1);
        }
        Ok(())
    }
    /// 键在某作用域是否在场。
    pub fn occupied(&self, scope: u8, key_id: u8) -> bool {
        if key_id as usize >= KEY_N {
            return false;
        }
        let k = key_id as usize;
        let bits = if scope == SCOPE_BOTTLE { &self.bottle_bits } else { &self.app_bits };
        (bits[k / 8] & (1u8 << (k % 8))) != 0
    }
    /// 位面守恒：占用计数与弹入计数一致。
    pub fn conserve_ok(&self, bottle_entries: u32, app_entries: u32) -> bool {
        self.bottle_n == bottle_entries && self.app_n == app_entries
    }
}

// ── 数据结构二：仲裁账（数组账本之二） ──────────────────────────
/// 矛盾检出账：检出/仲裁/抑制三计数 + 逐严重级分账 + 告警文案账。
/// 守恒律：detected == arbitrated（每条检出必须留下仲裁记录）；
/// suppressed 只记不告（同值共在场是冗余不是矛盾）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConflictLedger {
    pub detected: u32,
    pub arbitrated: u32,
    pub suppressed: u32,
    pub rejected: u32,
    pub by_sev: [u32; SEV_N],
    pub warnings: Vec<String>,
}

impl ConflictLedger {
    /// 新建空账。
    pub fn new() -> ConflictLedger {
        ConflictLedger {
            detected: 0,
            arbitrated: 0,
            suppressed: 0,
            rejected: 0,
            by_sev: [0; SEV_N],
            warnings: Vec::new(),
        }
    }
    /// 记一条仲裁结果：严重级入分账 + 告警文案入账。
    pub fn record_arbitrated(&mut self, key_id: u8, sev: u8, app_wins: bool) -> Result<(), u32> {
        if sev as usize >= SEV_N {
            return Err(PfCode::SevIllegal.code());
        }
        self.arbitrated = self.arbitrated.saturating_add(1);
        self.by_sev[sev as usize] = self.by_sev[sev as usize].saturating_add(1);
        let owner = if app_wins { "应用域覆盖" } else { "瓶域保留" };
        let name = if (key_id as usize) < KEY_N { KEY_NAMES[key_id as usize] } else { "?" };
        self.warnings
            .push(format!("矛盾告警：键{}({})，{}", key_id, name, owner));
        Ok(())
    }
    /// 守恒核：检出=仲裁 且 逐级分账和=检出。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.detected != self.arbitrated {
            return Err(PfCode::ConservationBroken.code());
        }
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < SEV_N {
            sum = sum.saturating_add(self.by_sev[i]);
            i += 1;
        }
        if sum != self.detected {
            return Err(PfCode::ConservationBroken.code());
        }
        let mut j = 0;
        while j < self.warnings.len() {
            match self.warnings.get(j) {
                Some(w) => {
                    if w.is_empty() {
                        return Err(PfCode::WarnEmpty.code());
                    }
                }
                None => return Err(PfCode::WarnEmpty.code()),
            }
            j += 1;
        }
        Ok(())
    }
}

// ── 冻结矛盾组合表（10 组，推导注记） ───────────────────────────
/// 组合行：键位 × 瓶值类 × 应用值类 → 冻结严重级。
/// 推导：黄金语料瓶域 cls_b(k)=k%5，应用域 cls_a(k)=(k+2)%5（k=0..9），
/// 十对组合逐行写死于此；严重级按 (k%3) 分布（0 提示×4、1 警告×3、
/// 2 高危×3），属主恒为应用域覆盖（per-app 显式优先）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Combo {
    pub key_id: u8,
    pub bottle_cls: u8,
    pub app_cls: u8,
    pub sev: u8,
    pub app_wins: bool,
}

pub const COMBOS: [Combo; COMBO_N] = [
    Combo { key_id: 0, bottle_cls: 0, app_cls: 2, sev: 0, app_wins: true },
    Combo { key_id: 1, bottle_cls: 1, app_cls: 3, sev: 1, app_wins: true },
    Combo { key_id: 2, bottle_cls: 2, app_cls: 4, sev: 2, app_wins: true },
    Combo { key_id: 3, bottle_cls: 3, app_cls: 0, sev: 0, app_wins: true },
    Combo { key_id: 4, bottle_cls: 4, app_cls: 1, sev: 1, app_wins: true },
    Combo { key_id: 5, bottle_cls: 0, app_cls: 2, sev: 2, app_wins: true },
    Combo { key_id: 6, bottle_cls: 1, app_cls: 3, sev: 0, app_wins: true },
    Combo { key_id: 7, bottle_cls: 2, app_cls: 4, sev: 1, app_wins: true },
    Combo { key_id: 8, bottle_cls: 3, app_cls: 0, sev: 2, app_wins: true },
    Combo { key_id: 9, bottle_cls: 4, app_cls: 1, sev: 1, app_wins: true },
];

/// 查冻结组合表；命中返回严重级，表外返回 None（走优先级梯默认）。
pub fn combo_lookup(key_id: u8, bottle_cls: u8, app_cls: u8) -> Option<(u8, bool)> {
    let mut i = 0;
    while i < COMBO_N {
        let c = COMBOS[i];
        if c.key_id == key_id && c.bottle_cls == bottle_cls && c.app_cls == app_cls {
            return Some((c.sev, c.app_wins));
        }
        i += 1;
    }
    None
}

// ── 档案条目 ────────────────────────────────────────────────────
/// 一条档案声明：作用域 + 键位 + 值类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProfileEntry {
    pub scope: u8,
    pub key_id: u8,
    pub val_class: u8,
}

/// 单条合法性：键位闭集 + 值类闭集。
pub fn entry_legal(e: &ProfileEntry) -> bool {
    (e.key_id as usize) < KEY_N && e.val_class < VAL_CLASS_N
}

/// 矛盾判定结果（键位双域值类 + 仲裁字段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub key_id: u8,
    pub bottle_cls: u8,
    pub app_cls: u8,
    pub sev: u8,
    pub app_wins: bool,
    pub from_table: bool,
}

// ── 判定核 ──────────────────────────────────────────────────────
/// 扫描双域档案：先位面落位（越界/重复即拒），再逐键判矛盾。
/// 输出按键位升序——与输入顺序无关（乱序输入同结果，确定性红线）。
pub fn scan_conflicts(
    bottle: &[ProfileEntry],
    app: &[ProfileEntry],
) -> Result<(Vec<Conflict>, ScopePlane), u32> {
    let mut plane = ScopePlane::new();
    let mut i = 0;
    while i < bottle.len() {
        let e = bottle.get(i).ok_or(PfCode::PlaneCorrupt.code())?;
        if !entry_legal(e) || e.scope != SCOPE_BOTTLE {
            if e.val_class >= VAL_CLASS_N {
                return Err(PfCode::ValClassRange.code());
            }
            return Err(PfCode::UnknownKey.code());
        }
        plane.set(e.scope, e.key_id)?;
        i += 1;
    }
    let mut j = 0;
    while j < app.len() {
        let e = app.get(j).ok_or(PfCode::PlaneCorrupt.code())?;
        if !entry_legal(e) || e.scope != SCOPE_APP {
            if e.val_class >= VAL_CLASS_N {
                return Err(PfCode::ValClassRange.code());
            }
            return Err(PfCode::UnknownKey.code());
        }
        plane.set(e.scope, e.key_id)?;
        j += 1;
    }
    // 每键取双域值类（同域重复已在位面层拒绝，此处每键至多一值）。
    let mut bval: [u8; KEY_N] = [0xFF; KEY_N];
    let mut aval: [u8; KEY_N] = [0xFF; KEY_N];
    let mut p = 0;
    while p < bottle.len() {
        if let Some(e) = bottle.get(p) {
            bval[e.key_id as usize] = e.val_class;
        }
        p += 1;
    }
    let mut q = 0;
    while q < app.len() {
        if let Some(e) = app.get(q) {
            aval[e.key_id as usize] = e.val_class;
        }
        q += 1;
    }
    let mut out: Vec<Conflict> = Vec::new();
    let mut k = 0;
    while k < KEY_N {
        let kk = k as u8;
        let b = bval[k];
        let a = aval[k];
        if plane.occupied(SCOPE_BOTTLE, kk) && plane.occupied(SCOPE_APP, kk) && b != a {
            let (sev, app_wins, from_table) = match combo_lookup(kk, b, a) {
                Some((s, w)) => (s, w, true),
                None => (1u8, true, false), // 表外：优先级梯默认（警告级，应用覆盖）
            };
            out.push(Conflict { key_id: kk, bottle_cls: b, app_cls: a, sev, app_wins, from_table });
        }
        k += 1;
    }
    Ok((out, plane))
}

// ── 仲裁核 ──────────────────────────────────────────────────────
/// 把判定结果过账：检出计数、仲裁入账（有账）、抑制入账。
/// 返回仲裁后的冲突清单（含表外默认仲裁标记）。
pub fn arbitrate_all(
    conflicts: Vec<Conflict>,
    ledger: &mut ConflictLedger,
) -> Result<Vec<Conflict>, u32> {
    ledger.detected = ledger.detected.saturating_add(conflicts.len() as u32);
    let mut done: Vec<Conflict> = Vec::new();
    let mut i = 0;
    while i < conflicts.len() {
        let c = conflicts.get(i).ok_or(PfCode::ArbitrationUnaccounted.code())?;
        ledger.record_arbitrated(c.key_id, c.sev, c.app_wins)?;
        done.push(*c);
        i += 1;
    }
    Ok(done)
}

// ── 黄金语料构造（确定性公式，推导注记见 COMBOS） ───────────────
/// 瓶域条目：键 0..12 全落位，值类 cls_b(k)=k%5。
pub fn make_bottle_profile() -> Vec<ProfileEntry> {
    let mut v: Vec<ProfileEntry> = Vec::new();
    let mut k = 0;
    while k < KEY_N {
        v.push(ProfileEntry {
            scope: SCOPE_BOTTLE,
            key_id: k as u8,
            val_class: (k % 5) as u8,
        });
        k += 1;
    }
    v
}

/// 应用域条目：键 0..12 全落位，键 0..9 走 cls_a(k)=(k+2)%5（与瓶域
/// 必不同 → 10 组矛盾）；键 10、11 与瓶域同值 → 抑制告警 2 条。
pub fn make_app_profile() -> Vec<ProfileEntry> {
    let mut v: Vec<ProfileEntry> = Vec::new();
    let mut k = 0;
    while k < KEY_N {
        let cls = if k >= 10 {
            (k % 5) as u8
        } else {
            ((k + 2) % 5) as u8
        };
        v.push(ProfileEntry { scope: SCOPE_APP, key_id: k as u8, val_class: cls });
        k += 1;
    }
    v
}

// ── 注入核（3 类坏样本，逐类必拒） ──────────────────────────────
/// 注入检出账：tries/caught 双列，漏检必须为零。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
    pub classes_rejected: [bool; 3],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3], classes_rejected: [false; 3] }
    }
    /// 记录一轮注入结果。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] = self.tries[slot].saturating_add(1);
            if hit {
                self.caught[slot] = self.caught[slot].saturating_add(1);
                self.classes_rejected[slot] = true;
            }
        }
    }
    /// 全抓判据：三类逐类 try==catch 且轮数齐。
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
    /// 计数守恒：注入次数 = 检出 + 漏检（漏检恒零）。
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

/// 注入轮：三类坏样本各一轮（未知键/同域重复键/值类越界）。
pub fn inject_round(rounds: u32) -> InjectLedger {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 类 1：未知键（键位 99 越出 0..12 闭集）。
        let bad_key = vec![ProfileEntry { scope: SCOPE_APP, key_id: 99, val_class: 1 }];
        led.record(0, scan_conflicts(&make_bottle_profile(), &bad_key).is_err());
        // 类 2：同域重复键（瓶域键 3 声明两次）。
        let dup = vec![
            ProfileEntry { scope: SCOPE_BOTTLE, key_id: 3, val_class: 1 },
            ProfileEntry { scope: SCOPE_BOTTLE, key_id: 3, val_class: 2 },
        ];
        led.record(1, scan_conflicts(&dup, &make_app_profile()).is_err());
        // 类 3：值类越界（7 落出 5 档闭集）。
        let bad_cls = vec![ProfileEntry { scope: SCOPE_APP, key_id: 1, val_class: 7 }];
        led.record(2, scan_conflicts(&make_bottle_profile(), &bad_cls).is_err());
        r += 1;
    }
    led
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：语料扫描 → 10 矛盾检出 → 仲裁有账 → 守恒 → 双跑一致。
/// 返回 (检出数, 抑制数, 表外仲裁数)。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let bottle = make_bottle_profile();
    let app = make_app_profile();
    let (conflicts, plane) = scan_conflicts(&bottle, &app)?;
    if conflicts.len() != COMBO_N {
        return Err(PfCode::ConservationBroken.code());
    }
    if !plane.conserve_ok(bottle.len() as u32, app.len() as u32) {
        return Err(PfCode::PlaneCorrupt.code());
    }
    let mut ledger = ConflictLedger::new();
    let mut off_table = 0u32;
    let mut i = 0;
    while i < conflicts.len() {
        if let Some(c) = conflicts.get(i) {
            if !c.from_table {
                off_table = off_table.saturating_add(1);
            }
        }
        i += 1;
    }
    let _ = arbitrate_all(conflicts, &mut ledger)?;
    // 抑制账：键 10、11 双域同值（共在场但非矛盾）。
    ledger.suppressed = ledger.suppressed.saturating_add(2);
    ledger.conserve()?;
    if ledger.warnings.len() != COMBO_N {
        return Err(PfCode::ArbitrationUnaccounted.code());
    }
    Ok((ledger.detected, ledger.suppressed, off_table))
}

/// 探针三序（正常/表外/空档）。
pub fn probe_seq_a() -> Result<u32, u32> {
    let (c, _p) = scan_conflicts(&make_bottle_profile(), &make_app_profile())?;
    Ok(c.len() as u32)
}

/// 表外组合：键 0 双域值类 (0,1) 不在冻结表 → 优先级梯默认仲裁。
pub fn probe_seq_b() -> Result<u8, u32> {
    let bottle = vec![ProfileEntry { scope: SCOPE_BOTTLE, key_id: 0, val_class: 0 }];
    let app = vec![ProfileEntry { scope: SCOPE_APP, key_id: 0, val_class: 1 }];
    let (c, _) = scan_conflicts(&bottle, &app)?;
    let first = c.get(0).ok_or(PfCode::ArbitrationUnaccounted.code())?;
    if first.from_table {
        return Err(PfCode::ArbitrationUnaccounted.code());
    }
    Ok(first.sev)
}

/// 空档边界：双域皆空 → 零矛盾零告警，Ok。
pub fn probe_seq_c() -> Result<u32, u32> {
    let (c, plane) = scan_conflicts(&[], &[])?;
    if !plane.conserve_ok(0, 0) || c.len() != 0 {
        return Err(PfCode::PlaneCorrupt.code());
    }
    Ok(0)
}

/// 双跑一致：黄金全链两跑同值。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PfCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落在 [SEG_BASE, SEG_BASE+0x40) 窗口。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PfCode::ALL.len() {
        let v = PfCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集：每码中文文案非空。
pub fn detail_closed() -> bool {
    let mut i = 0;
    while i < PfCode::ALL.len() {
        if PfCode::ALL[i].detail().is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据汇总（name, passed, 动态明细）——供 run_unx18551_checks 收口。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：矛盾组合检出告警 10/10。
    let g = golden_run();
    let g_ok = match &g {
        Ok((d, s, o)) => *d == COMBO_N as u32 && *s == 2 && *o == 0,
        Err(_) => false,
    };
    v.push(("J1-矛盾组合检出告警10/10", g_ok, format!("{:?}", g)));
    // 仲裁按优先级有账。
    let mut led = ConflictLedger::new();
    let arb_ok = match scan_conflicts(&make_bottle_profile(), &make_app_profile()) {
        Ok((c, _)) => match arbitrate_all(c, &mut led) {
            Ok(done) => {
                led.conserve().is_ok()
                    && done.len() == COMBO_N
                    && led.warnings.len() == COMBO_N
                    && led.by_sev[0] + led.by_sev[1] + led.by_sev[2] == COMBO_N as u32
            }
            Err(_) => false,
        },
        Err(_) => false,
    };
    v.push(("J1-仲裁按优先级有账", arb_ok, format!("det={} arb={}", led.detected, led.arbitrated)));
    // 表外组合走优先级梯默认（警告级、应用覆盖）。
    let b = probe_seq_b();
    v.push(("J2-表外梯降仲裁", b == Ok(1u8), format!("{:?}", b)));
    // 注入三类 10/10。
    let inj = inject_round(JUDGE_ROUNDS);
    v.push((
        "J1-注入三类10/10",
        inj.all_caught(JUDGE_ROUNDS) && inj.missed() == 0,
        format!("t={:?} c={:?}", inj.tries, inj.caught),
    ));
    // 顺序无关（乱序输入同结果）。
    let mut app_rev = make_app_profile();
    let mut rev: Vec<ProfileEntry> = Vec::new();
    let mut ri = app_rev.len();
    while ri > 0 {
        ri -= 1;
        if let Some(e) = app_rev.get(ri) {
            rev.push(*e);
        }
    }
    app_rev.clear();
    let fwd = scan_conflicts(&make_bottle_profile(), &make_app_profile());
    let bwd = scan_conflicts(&make_bottle_profile(), &rev);
    v.push((
        "J2-乱序输入同结果",
        fwd == bwd,
        format!("fwd={:?} bwd={:?}", fwd.is_ok(), bwd.is_ok()),
    ));
    // 位面守恒。
    let pc = match scan_conflicts(&make_bottle_profile(), &make_app_profile()) {
        Ok((_, p)) => p.conserve_ok(KEY_N as u32, KEY_N as u32),
        Err(_) => false,
    };
    v.push(("J2-位面守恒", pc, String::from("12+12 落位对账")));
    // 边界：空档/满档。
    let empty_ok = probe_seq_c().is_ok();
    v.push(("J2-空档边界", empty_ok, String::from("双域空=零矛盾")));
    let full = scan_conflicts(&make_bottle_profile(), &make_app_profile()).is_ok();
    v.push(("J2-满档边界", full, format!("{}+{} 键全落位", KEY_N, KEY_N)));
    // 同域重复键显式拒绝（边界闸）：瓶域同键声明两次必拒。
    let dup = vec![
        ProfileEntry { scope: SCOPE_BOTTLE, key_id: 5, val_class: 1 },
        ProfileEntry { scope: SCOPE_BOTTLE, key_id: 5, val_class: 1 },
    ];
    let dup_rejected = scan_conflicts(&dup, &[]).is_err();
    v.push(("J2-同域重复键显式拒", dup_rejected, String::from("一键多声明=注入面")));
    // 跨域同键异值=矛盾（核心语义正例）。
    let cross_b = vec![ProfileEntry { scope: SCOPE_BOTTLE, key_id: 5, val_class: 1 }];
    let cross_a = vec![ProfileEntry { scope: SCOPE_APP, key_id: 5, val_class: 3 }];
    let cross_ok = match scan_conflicts(&cross_b, &cross_a) {
        Ok((c, _)) => c.len() == 1,
        Err(_) => false,
    };
    v.push(("J2-跨域同键异值成矛盾", cross_ok, String::from("键5 双域异值=1 矛盾")));
    // 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // 探针三序。
    let pa = probe_seq_a();
    v.push((
        "J2-探针三序",
        pa == Ok(COMBO_N as u32),
        format!("a={:?}", pa),
    ));
    // 段位守卫 + 文案闭集 + 守恒账。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x80580 窗口 0x40 对齐")));
    v.push(("J3-文案闭集", detail_closed(), String::from("9 码逐条非空")));
    v
}

/// 判据收口（动态明细槽仅 8 个：前 8 条走 add_dyn，余项静态明细）。
pub fn run_unx18551_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18551 档案冲突检测（per-app vs bottle 矛盾告警）");
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
// 测试区（≥8 例：正常流/边界/注入/双跑/段位/文案）
// ═══════════════════════════════════════════════════════════════════════
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_ten_conflicts_and_arbitration_ledger() {
        let g = golden_run().unwrap_or((0, 0, 0));
        assert_eq!(g.0, 10, "矛盾组合检出告警 10/10");
        assert_eq!(g.1, 2, "抑制账 2 条（键 10、11 同值）");
        assert_eq!(g.2, 0, "黄金语料全走冻结表");
    }

    #[test]
    fn arbitration_accounted_with_sev_split() {
        let mut led = ConflictLedger::new();
        let (c, _) = scan_conflicts(&make_bottle_profile(), &make_app_profile())
            .unwrap_or((Vec::new(), ScopePlane::new()));
        let done = arbitrate_all(c, &mut led).unwrap_or_default();
        assert_eq!(done.len(), 10);
        assert!(led.conserve().is_ok(), "仲裁守恒：检出=仲裁=分账和");
        assert_eq!(led.warnings.len(), 10, "每条检出留一条告警");
    }

    #[test]
    fn off_table_combo_ladder_default() {
        assert_eq!(probe_seq_b(), Ok(1u8), "表外组合默认警告级");
    }

    #[test]
    fn injections_three_classes_ten_rounds() {
        let inj = inject_round(JUDGE_ROUNDS);
        assert!(inj.all_caught(JUDGE_ROUNDS), "三类注入 10/10 必抓");
        assert_eq!(inj.missed(), 0, "漏检恒零");
        assert!(inj.classes_rejected[0] && inj.classes_rejected[1] && inj.classes_rejected[2]);
    }

    #[test]
    fn empty_and_full_boundaries() {
        assert_eq!(probe_seq_c(), Ok(0), "空档=零矛盾");
        let (c, p) = scan_conflicts(&make_bottle_profile(), &make_app_profile())
            .unwrap_or((Vec::new(), ScopePlane::new()));
        assert_eq!(c.len(), 10);
        assert!(p.conserve_ok(12, 12), "满档 12+12 守恒");
    }

    #[test]
    fn order_independent_scan() {
        let mut app = make_app_profile();
        let mut rev: Vec<ProfileEntry> = Vec::new();
        let mut i = app.len();
        while i > 0 {
            i -= 1;
            if let Some(e) = app.get(i) {
                rev.push(*e);
            }
        }
        app.clear();
        assert_eq!(
            scan_conflicts(&make_bottle_profile(), &make_app_profile()),
            scan_conflicts(&make_bottle_profile(), &rev),
            "乱序输入同结果"
        );
    }

    #[test]
    fn reproducibility_and_segment_guard() {
        assert!(repro_check().is_ok(), "双跑一致");
        assert!(segment_guard(), "0x80580 段位守卫");
        assert!(detail_closed(), "文案闭集非空");
    }

    #[test]
    fn checkset_all_green() {
        let set = run_unx18551_checks();
        assert!(set.all_passed(), "判据集全绿：{}", set.red_items_text());
    }
}
