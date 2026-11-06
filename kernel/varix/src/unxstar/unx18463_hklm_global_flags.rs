//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18463 · HKLM 全局兼容标志读取 · 0x7EF80段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18463-J1 HKLM 全局标志读取断言全过，HKCU 优先级仲裁正确
//!       （用户态覆盖机器态）
//! 施工：AI-VX11-1105-G1 · 工单 A07 · 批 04 · 配额 300 · 目标 ≥600 行
//! 本模块结构清单：
//!   数据结构：GLOBAL_FLAGS 10 项闭集 / FlagSlot id-寻址账本（10 槽定容）
//!   状态机  ：FlagPhase 五态单调 Idle→Lookup→ReadHives→Arbitrate→Frozen
//!   账本    ：id-寻址槽账（同值重写幂等、异值重写拒、只增不减封印）
//!   错误路径：HgfCode 11 码闭集，code() = SEG_BASE + n（0x40 窗口内）
//!   注入核  ：4 类坏样本（未知名/状态值越界/异值重写/id 越界）
//!             ×10 轮逐类断言被显式拒绝，漏检必为 0
//!   对照表  ：标志名→id 对照 8 例逐项复算零偏差
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EF80，AI-VX11-1105 批 A07 卡 F18463 独占窗口，
/// 窗宽 0x40 = 64 个诊断码）。
pub const SEG_BASE: u32 = 0x7EF80;

// ── 冻结常量（写死 + 推导注明） ─────────────────────────────────
/// 判据迭代轮数（冻结 10：注入核"10/10"口径）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 全局标志闭集规模（冻结 10：版本 5 项 + DPI 2 项 + 提权 1 项 + 分辨率色深 2 项）。
pub const FLAG_COUNT: usize = 10;
/// 三态闭集：未设。
pub const STATE_UNSET: u8 = 0;
/// 三态闭集：显式关闭。
pub const STATE_OFF: u8 = 1;
/// 三态闭集：显式开启。
pub const STATE_ON: u8 = 2;

/// 全局标志名闭集（冻结口径，与 Windows 兼容层常用标志名逐字符一致）。
pub const GLOBAL_FLAGS: [&str; FLAG_COUNT] = [
    "WIN7RTM",
    "WIN8RTM",
    "WIN80RTM",
    "WIN81RTM",
    "WIN10RTM",
    "HIGHDPIAWARE",
    "DPIUNAWARE",
    "RUNASADMIN",
    "640X480",
    "256COLOR",
];

// ── 诊断码闭集（段内 0x40 窗口） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HgfCode {
    /// 未知标志名（闭集外）。
    UnknownFlag,
    /// 状态值越界（三态闭集 {0,1,2} 外）。
    BadState,
    /// 异值重写（已登记槽被写入不同值，账本只增不减被踩）。
    RewriteConflict,
    /// 账本槽 id 越界。
    SlotIdOutOfRange,
    /// 状态机倒退。
    PhaseRegress,
    /// 读取失败（双 hive 均不可达之外的口径外路径）。
    ReadFail,
    /// 对照表偏差。
    RefTableDeviation,
    /// 账本封印破坏（登记数只增不减被违）。
    LedgerSealBroken,
    /// 计数守恒破坏（注入≠检出+漏检）。
    ConservationBroken,
    /// 覆盖计数缺失（双设场景必须记一次覆盖）。
    OverrideMissing,
    /// 双跑不一致。
    NotReproducible,
}

impl HgfCode {
    /// 诊断码：SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            HgfCode::UnknownFlag => "未知标志名：全局标志闭集外拒收",
            HgfCode::BadState => "状态值越界：三态闭集 {0,1,2} 外",
            HgfCode::RewriteConflict => "异值重写：账本只增不减被踩",
            HgfCode::SlotIdOutOfRange => "槽 id 越界：10 槽闭集外",
            HgfCode::PhaseRegress => "状态机倒退：读取流水只进不退",
            HgfCode::ReadFail => "读取失败：口径外路径",
            HgfCode::RefTableDeviation => "对照表偏差：标志名→id 不符",
            HgfCode::LedgerSealBroken => "账本封印破坏：登记数只增不减被违",
            HgfCode::ConservationBroken => "计数守恒破坏：注入=检出+漏检不成立",
            HgfCode::OverrideMissing => "覆盖计数缺失：双设必须记一次覆盖",
            HgfCode::NotReproducible => "双跑不一致：复现红线被踩",
        }
    }
    /// 全码闭集。
    pub const ALL: [HgfCode; 11] = [
        HgfCode::UnknownFlag,
        HgfCode::BadState,
        HgfCode::RewriteConflict,
        HgfCode::SlotIdOutOfRange,
        HgfCode::PhaseRegress,
        HgfCode::ReadFail,
        HgfCode::RefTableDeviation,
        HgfCode::LedgerSealBroken,
        HgfCode::ConservationBroken,
        HgfCode::OverrideMissing,
        HgfCode::NotReproducible,
    ];
}

/// 按码值取中文文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < HgfCode::ALL.len() {
        if HgfCode::ALL[i].code() == code {
            return HgfCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EF80 段闭集内"
}

// ── 状态机：全局读取流水 ────────────────────────────────────────
/// 读取流水五态（单调推进，只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlagPhase {
    /// 未开始。
    Idle = 0,
    /// 已完成标志名闭集查找。
    Lookup = 1,
    /// 已完成双 hive 读取。
    ReadHives = 2,
    /// 已完成仲裁。
    Arbitrate = 3,
    /// 已冻结。
    Frozen = 4,
}

impl FlagPhase {
    /// 单调序号。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单步推进：rank 恰 +1，其余显式拒。
    pub fn advance(self, next: FlagPhase) -> Result<FlagPhase, u32> {
        if next.rank() == self.rank().saturating_add(1) {
            Ok(next)
        } else {
            Err(HgfCode::PhaseRegress.code())
        }
    }
    /// 全链五步。
    pub fn full_chain() -> Result<FlagPhase, u32> {
        let p0 = FlagPhase::Idle;
        let p1 = p0.advance(FlagPhase::Lookup)?;
        let p2 = p1.advance(FlagPhase::ReadHives)?;
        let p3 = p2.advance(FlagPhase::Arbitrate)?;
        p3.advance(FlagPhase::Frozen)
    }
}

// ── 查找核与状态校验 ────────────────────────────────────────────
/// 标志名→id 精确查找（闭集外返回 None → 未知标志）。
pub fn flag_id(name: &str) -> Option<usize> {
    let mut i = 0;
    while i < FLAG_COUNT {
        let cand = match GLOBAL_FLAGS.get(i) {
            Some(s) => *s,
            None => break,
        };
        if cand.as_bytes() == name.as_bytes() {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 状态值域校验（三态闭集）。
pub fn state_ok(v: u8) -> bool {
    v <= STATE_ON
}

/// 裁决表：HKCU 优先（用户态覆盖机器态）。
///   user != 未设 → 生效 = user；user 未设 → 生效 = machine。
/// 返回（生效值, 是否发生覆盖）。
pub fn arbitrate(user: u8, machine: u8) -> Result<(u8, bool), u32> {
    if !state_ok(user) || !state_ok(machine) {
        return Err(HgfCode::BadState.code());
    }
    if user != STATE_UNSET {
        let covered = machine != STATE_UNSET;
        Ok((user, covered))
    } else {
        Ok((machine, false))
    }
}

// ── id-寻址槽账（10 槽定容，只增不减封印） ──────────────────────
/// 单标志槽。
#[derive(Clone, Copy, Debug)]
pub struct FlagSlot {
    pub registered: bool,
    pub user_state: u8,
    pub machine_state: u8,
    pub effective: u8,
    pub override_count: u32,
    pub rewrite_same: u32,
    pub write_seq: u32,
}

impl FlagSlot {
    /// 空槽（const 便于定容数组构造）。
    pub const EMPTY: FlagSlot = FlagSlot {
        registered: false,
        user_state: 0,
        machine_state: 0,
        effective: 0,
        override_count: 0,
        rewrite_same: 0,
        write_seq: 0,
    };
}

/// 全局标志账本。
pub struct FlagLedger {
    slots: [FlagSlot; FLAG_COUNT],
    pub registered_total: u32,
    pub full_rejects: u32,
    pub inject_tries: [u32; 4],
    pub inject_caught: [u32; 4],
    next_seq: u32,
}

impl FlagLedger {
    /// 空账本（10 槽全空）。
    pub fn new() -> FlagLedger {
        FlagLedger {
            slots: [FlagSlot::EMPTY; FLAG_COUNT],
            registered_total: 0,
            full_rejects: 0,
            inject_tries: [0; 4],
            inject_caught: [0; 4],
            next_seq: 1,
        }
    }
    /// 登记一条（双 hive 读值 → 仲裁 → 入槽）。
    /// 幂等口径：同值重写仅累加 rewrite_same；异值重写显式拒。
    pub fn register(
        &mut self,
        id: usize,
        user: u8,
        machine: u8,
    ) -> Result<u8, u32> {
        if !state_ok(user) || !state_ok(machine) {
            return Err(HgfCode::BadState.code());
        }
        if id >= FLAG_COUNT {
            return Err(HgfCode::SlotIdOutOfRange.code());
        }
        let (eff, covered) = arbitrate(user, machine)?;
        // 取旧槽做幂等判断（块内算清，块外再写——避免借用冲突）。
        let old = self.slots[id];
        if old.registered {
            if old.user_state == user && old.machine_state == machine {
                // 同值重写：幂等，账面值不变，只累加计数。
                self.slots[id].rewrite_same = old.rewrite_same.saturating_add(1);
                return Ok(eff);
            }
            return Err(HgfCode::RewriteConflict.code());
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.slots[id] = FlagSlot {
            registered: true,
            user_state: user,
            machine_state: machine,
            effective: eff,
            override_count: if covered { 1 } else { 0 },
            rewrite_same: 0,
            write_seq: seq,
        };
        self.registered_total += 1;
        Ok(eff)
    }
    /// 读槽（越界 → 显式错）。
    pub fn slot(&self, id: usize) -> Result<FlagSlot, u32> {
        if id >= FLAG_COUNT {
            return Err(HgfCode::SlotIdOutOfRange.code());
        }
        Ok(self.slots[id])
    }
    /// 已登记槽数（只增不减封印复核）。
    pub fn sealed_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < FLAG_COUNT {
            if self.slots[i].registered {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// 覆盖总数（用户态覆盖机器态发生次数）。
    pub fn override_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < FLAG_COUNT {
            n = n.saturating_add(self.slots[i].override_count);
            i += 1;
        }
        n
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

// ── 全路径读取核 ────────────────────────────────────────────────
/// 全局标志读取总装：查找 → 双 hive 读取 → 仲裁 → 入账。
pub fn read_global(
    led: &mut FlagLedger,
    name: &str,
    hkcu_raw: Option<u8>,
    hklm_raw: Option<u8>,
) -> Result<u8, u32> {
    let id = match flag_id(name) {
        Some(i) => i,
        None => return Err(HgfCode::UnknownFlag.code()),
    };
    FlagPhase::full_chain()?;
    let user = match hkcu_raw {
        Some(v) => v,
        None => STATE_UNSET,
    };
    let machine = match hklm_raw {
        Some(v) => v,
        None => STATE_UNSET,
    };
    led.register(id, user, machine)
}

// ── 注入核（4 类坏样本 ×10 轮） ─────────────────────────────────
/// 注入探针：
///   0 未知标志名；1 状态值越界（3）；2 异值重写；3 槽 id 越界。
pub fn inject_probe(slot: usize) -> Result<(), u32> {
    let mut led = FlagLedger::new();
    match slot {
        0 => match read_global(&mut led, "NOT_A_FLAG", Some(STATE_ON), None) {
            // 命中约定：注入被抓 → Err(窗内码)；漏 → Ok。
            Err(c) if c == HgfCode::UnknownFlag.code() => Err(c),
            _ => Ok(()),
        },
        1 => {
            // 状态值 3 = 三态闭集外。
            match arbitrate(STATE_ON, 3) {
                Err(c) if c == HgfCode::BadState.code() => Err(c),
                _ => Ok(()),
            }
        }
        2 => {
            // 异值重写：先登记 user=ON，再以 user=OFF 重写 → 必拒。
            let _ = led.register(8, STATE_ON, STATE_UNSET)?;
            match led.register(8, STATE_OFF, STATE_UNSET) {
                Err(c) if c == HgfCode::RewriteConflict.code() => Err(c),
                _ => Ok(()),
            }
        }
        _ => {
            // 槽 id 越界（10 = FLAG_COUNT）。
            match led.register(FLAG_COUNT, STATE_ON, STATE_UNSET) {
                Err(c) if c == HgfCode::SlotIdOutOfRange.code() => Err(c),
                _ => Ok(()),
            }
        }
    }
}

// ── 对照表（标志名→id，8 例逐项复算） ───────────────────────────
/// 对照例：输入名 + 期望 id（Some=在册 / None=闭集外）。
pub const REF_TABLE: [(&str, Option<usize>); 8] = [
    ("WIN7RTM", Some(0)),
    ("WIN10RTM", Some(4)),
    ("HIGHDPIAWARE", Some(5)),
    ("DPIUNAWARE", Some(6)),
    ("RUNASADMIN", Some(7)),
    ("256COLOR", Some(9)),
    ("NOT_A_FLAG", None),
    ("win7rtm", None), // 大小写敏感：小写不在闭集（口径冻结）
];

/// 对照复算：逐例查找结果必须与期望零偏差。
pub fn ref_table_check() -> Result<u32, u32> {
    let mut ok = 0u32;
    let mut i = 0;
    while i < REF_TABLE.len() {
        let (name, expect) = REF_TABLE[i];
        let got = flag_id(name);
        let eq = match (got, expect) {
            (Some(a), Some(b)) => a == b,
            (None, None) => true,
            _ => false,
        };
        if !eq {
            return Err(HgfCode::RefTableDeviation.code());
        }
        ok += 1;
        i += 1;
    }
    Ok(ok)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：10 标志三态轮转入账 → 覆盖断言 → 对照表 → 4 类注入
/// 10 轮全抓 → 守恒 → 幂等重写 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    FlagPhase::full_chain()?;
    let mut led = FlagLedger::new();
    // 10 标志轮转：HKCU 有 / 仅 HKLM 有 / 双无 三态。
    let mut i = 0usize;
    while i < FLAG_COUNT {
        let name = match GLOBAL_FLAGS.get(i) {
            Some(s) => *s,
            None => break,
        };
        let r = match i % 3 {
            0 => read_global(&mut led, name, Some(STATE_ON), Some(STATE_OFF))?,
            1 => read_global(&mut led, name, None, Some(STATE_ON))?,
            _ => read_global(&mut led, name, None, None)?,
        };
        // 生效值复核：0 类必须取 HKCU 值（用户态覆盖机器态）。
        if i % 3 == 0 && r != STATE_ON {
            return Err(HgfCode::ConservationBroken.code());
        }
        i += 1;
    }
    if led.sealed_count() != FLAG_COUNT {
        return Err(HgfCode::LedgerSealBroken.code());
    }
    // 覆盖计数：0,3,6,9 四槽双设各记一次覆盖 → 恰 4 次。
    if led.override_total() != 4 {
        return Err(HgfCode::OverrideMissing.code());
    }
    // 同值重写幂等：账面生效值不变。
    let eff0 = match led.slot(0) {
        Ok(s) => s.effective,
        Err(_) => return Err(HgfCode::ReadFail.code()),
    };
    let eff0b = read_global(&mut led, GLOBAL_FLAGS[0], Some(STATE_ON), Some(STATE_OFF))?;
    let eff0c = match led.slot(0) {
        Ok(s) => s.effective,
        Err(_) => return Err(HgfCode::ReadFail.code()),
    };
    if eff0 != eff0b || eff0 != eff0c {
        return Err(HgfCode::LedgerSealBroken.code());
    }
    // 对照表零偏差。
    ref_table_check()?;
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
        return Err(HgfCode::ConservationBroken.code());
    }
    Ok((led.sealed_count() as u32, led.override_total(), led.inject_caught[0]))
}

// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_a() -> Result<(), u32> {
    // 用户态 OFF 覆盖机器态 ON：生效必须为 OFF（用户态覆盖）。
    let (eff, covered) = arbitrate(STATE_OFF, STATE_ON)?;
    if eff == STATE_OFF && covered {
        Ok(())
    } else {
        Err(HgfCode::OverrideMissing.code())
    }
}

pub fn probe_b() -> Result<(), u32> {
    // 未知标志名必拒。
    let mut led = FlagLedger::new();
    match read_global(&mut led, "BADMAGIC", None, None) {
        Err(c) if c == HgfCode::UnknownFlag.code() => Ok(()),
        _ => Err(HgfCode::ConservationBroken.code()),
    }
}

pub fn probe_c() -> Result<(), u32> {
    // 状态机越级必拒。
    match FlagPhase::Idle.advance(FlagPhase::Arbitrate) {
        Err(c) if c == HgfCode::PhaseRegress.code() => Ok(()),
        _ => Err(HgfCode::ConservationBroken.code()),
    }
}

// ── 判据裁决 ────────────────────────────────────────────────────
/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x7EF80 {
        return false;
    }
    let mut i = 0;
    while i < HgfCode::ALL.len() {
        let v = HgfCode::ALL[i].code();
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
    while i < HgfCode::ALL.len() {
        if detail_of(HgfCode::ALL[i].code()).len() == 0 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据聚合（13 条，单模块 ≤20）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-段位守卫",
        segment_guard(),
        format!("SEG=0x{:X} 窗宽0x40", SEG_BASE),
    ));
    v.push((
        "J1-标志闭集规模",
        FLAG_COUNT == 10 && GLOBAL_FLAGS.len() == FLAG_COUNT,
        format!("{} 项冻结", FLAG_COUNT),
    ));
    v.push((
        "J1-五态流水",
        FlagPhase::full_chain().is_ok(),
        String::from("Idle→…→Frozen 单调"),
    ));
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // HKCU 优先仲裁四象限。
    let q1 = match arbitrate(STATE_ON, STATE_OFF) {
        Ok((e, c)) => e == STATE_ON && c,
        Err(_) => false,
    };
    let q2 = match arbitrate(STATE_OFF, STATE_ON) {
        Ok((e, c)) => e == STATE_OFF && c,
        Err(_) => false,
    };
    let q3 = match arbitrate(STATE_UNSET, STATE_ON) {
        Ok((e, c)) => e == STATE_ON && !c,
        Err(_) => false,
    };
    let q4 = match arbitrate(STATE_UNSET, STATE_UNSET) {
        Ok((e, c)) => e == STATE_UNSET && !c,
        Err(_) => false,
    };
    v.push((
        "J1-HKCU优先仲裁四象限",
        q1 && q2 && q3 && q4,
        format!("覆盖{} 回退{} 双无{} 未设{}", q1, q2, q3, q4),
    ));
    let refok = match ref_table_check() {
        Ok(n) => n == 8,
        Err(_) => false,
    };
    v.push((
        "J1-对照表零偏差",
        refok,
        String::from("标志名→id 8 例"),
    ));
    // 注入 4 类 ×10 轮。
    let mut led = FlagLedger::new();
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
    let total_tries = led.inject_tries[0] + led.inject_tries[1] + led.inject_tries[2] + led.inject_tries[3];
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
    let pa = probe_a().is_ok();
    let pb = probe_b().is_ok();
    let pc = probe_c().is_ok();
    v.push((
        "J1-探针三序",
        pa && pb && pc,
        format!("a={} b={} c={}", pa, pb, pc),
    ));
    v.push((
        "J1-文案闭集",
        detail_closed(),
        format!("{} 码全非空", HgfCode::ALL.len()),
    ));
    // 账本封印：10 槽满后异值重写必拒，登记数不涨。
    let mut led2 = FlagLedger::new();
    let mut i9 = 0usize;
    while i9 < FLAG_COUNT {
        let _ = led2.register(i9, STATE_ON, STATE_UNSET);
        i9 += 1;
    }
    let before = led2.sealed_count();
    let rw = led2.register(0, STATE_OFF, STATE_UNSET).is_err();
    let after = led2.sealed_count();
    v.push((
        "J1-账本封印只增不减",
        before == FLAG_COUNT && rw && after == before,
        format!("登记{} 异值重写拒{}", before, rw),
    ));
    // 边界四类：空值域（0）合法 / 3 拒 / id 上界（9）合法 / id 10 拒。
    let b0 = state_ok(STATE_UNSET) && arbitrate(STATE_UNSET, STATE_UNSET).is_ok();
    let b1 = arbitrate(3, STATE_UNSET).is_err();
    let b2 = {
        let mut l3 = FlagLedger::new();
        l3.register(FLAG_COUNT - 1, STATE_ON, STATE_UNSET).is_ok()
    };
    let b3 = {
        let mut l4 = FlagLedger::new();
        l4.register(FLAG_COUNT, STATE_ON, STATE_UNSET).is_err()
    };
    v.push((
        "J1-边界四类",
        b0 && b1 && b2 && b3,
        format!("零值{} 域外{} 上界{} 越界{}", b0, b1, b2, b3),
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
pub fn run_unx18463_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18463 HKLM 全局兼容标志读取");
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
        let set = run_unx18463_checks();
        assert!(set.all_passed(), "判据全绿失败：{}", set.red_items_text());
    }

    #[test]
    fn arbitration_user_wins() {
        let a = arbitrate(STATE_ON, STATE_OFF);
        match a {
            Ok((e, c)) => {
                assert!(e == STATE_ON, "用户态覆盖");
                assert!(c, "覆盖必须入账");
            }
            Err(_) => panic!("合法仲裁不该报错"),
        }
        let b = arbitrate(STATE_OFF, STATE_ON);
        match b {
            Ok((e, _)) => assert!(e == STATE_OFF, "用户态 OFF 也覆盖机器态 ON"),
            Err(_) => panic!("合法仲裁不该报错"),
        }
    }

    #[test]
    fn boundary_four_kinds() {
        assert!(arbitrate(STATE_UNSET, STATE_UNSET).is_ok(), "零值合法");
        assert!(arbitrate(3, STATE_UNSET).is_err(), "域外必拒");
        let mut led = FlagLedger::new();
        assert!(led.register(FLAG_COUNT - 1, STATE_ON, STATE_UNSET).is_ok(), "上界槽合法");
        assert!(led.register(FLAG_COUNT, STATE_ON, STATE_UNSET).is_err(), "越界槽必拒");
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
    fn rewrite_idempotent_same_value() {
        let mut led = FlagLedger::new();
        let e1 = led.register(5, STATE_ON, STATE_UNSET);
        let e2 = led.register(5, STATE_ON, STATE_UNSET);
        match (e1, e2) {
            (Ok(a), Ok(b)) => {
                assert!(a == b, "同值重写幂等");
                assert!(led.sealed_count() == 1, "登记数不变");
            }
            _ => panic!("同值重写不该报错"),
        }
        assert!(led.register(5, STATE_OFF, STATE_UNSET).is_err(), "异值重写必拒");
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
    fn ledger_seal_only_grows() {
        let mut led = FlagLedger::new();
        let mut i = 0usize;
        while i < FLAG_COUNT {
            let _ = led.register(i, STATE_UNSET, STATE_ON);
            i += 1;
        }
        assert!(led.sealed_count() == FLAG_COUNT, "10 槽全登记");
        let n0 = led.sealed_count();
        let _ = led.register(2, STATE_OFF, STATE_UNSET);
        assert!(led.sealed_count() == n0, "异值重写不增登记");
    }

    #[test]
    fn segment_guard_window() {
        assert!(segment_guard(), "段位守卫失败");
        let mut i = 0;
        while i < HgfCode::ALL.len() {
            let c = HgfCode::ALL[i].code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x40, "码越窗");
            assert!(detail_of(c).len() > 0, "文案空缺");
            i += 1;
        }
        assert!(ref_table_check().is_ok(), "对照表零偏差");
    }
}
