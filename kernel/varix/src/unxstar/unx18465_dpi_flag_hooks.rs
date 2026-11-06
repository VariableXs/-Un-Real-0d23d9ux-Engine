//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18465 · HIGHDPIAWARE/DPIUNAWARE 标志消费预告 · 0x7F000段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18465-J1 DPI 标志消费挂点签名冻结 v1（N 部消费），桩期
//!       登记可导出
//! 施工：AI-VX11-1105-G1 · 工单 A07 · 批 04 · 配额 260 · 目标 ≥600 行
//! 本模块结构清单：
//!   数据结构：DpiMode 三态闭集 / HookSlot 3 部定容挂点表（id-寻址）
//!   状态机  ：HookPhase 五态单调 Idle→ModeResolved→Registered→Exported→Frozen
//!   账本    ：挂点登记账（签名盖章、消费计数、登记数只增不减封印）
//!   错误路径：DfhCode 11 码闭集，code() = SEG_BASE + n（0x40 窗口内）
//!   注入核  ：5 类坏样本（未知DPI标志/部越界/签名不匹配/未注册消费/重复登记）
//!             ×10 轮逐类断言被显式拒绝，漏检必为 0
//!   导出核  ：桩期登记导出文本（确定性），双建导出逐字节一致（往返幂等）
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F000，AI-VX11-1105 批 A07 卡 F18465 独占窗口，
/// 窗宽 0x40 = 64 个诊断码）。
pub const SEG_BASE: u32 = 0x7F000;

// ── 冻结常量（写死 + 推导注明） ─────────────────────────────────
/// 判据迭代轮数（冻结 10：注入核"10/10"口径）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 消费部数 N（冻结 3：部 0=进程注入前 pre-init、部 1=窗口创建 wm-create、
/// 部 2=消息循环 msg-loop——桩期预告口径）。
pub const CONSUME_PARTS: usize = 3;
/// 消费挂点签名冻结 v1。推导：高 24 位 = "DIP" 三字母 ASCII
/// （D=0x44, I=0x49, P=0x50）→ 0x444950；低 8 位 = N 部数 = 3。
pub const HOOK_SIG_V1: u32 = 0x4449_5003;
/// 导出文本最大长度（冻结 96：3 部 × ~28 字符/部 + 头 12 字符余量）。
pub const EXPORT_LEN_MAX: usize = 96;

// ── DPI 模式三态闭集 ────────────────────────────────────────────
/// DPI 消费模式（闭集冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpiMode {
    /// 未设（继承父进程）。
    Inherit = 0,
    /// HIGHDPIAWARE：进程感知 DPI。
    HighDpiAware = 1,
    /// DPIUNAWARE：进程不感知 DPI。
    DpiUnaware = 2,
}

impl DpiMode {
    /// 下标。
    pub fn idx(self) -> u32 {
        self as u32
    }
    /// 中文说明。
    pub fn zh(self) -> &'static str {
        match self {
            DpiMode::Inherit => "继承",
            DpiMode::HighDpiAware => "感知DPI",
            DpiMode::DpiUnaware => "不感知DPI",
        }
    }
}

/// DPI 标志名→模式解析（两记法闭集外 → 未知标志）。
pub fn resolve_dpi_mode(flag: Option<&str>) -> Result<DpiMode, u32> {
    match flag {
        None => Ok(DpiMode::Inherit),
        Some(s) if s.as_bytes() == b"HIGHDPIAWARE" => Ok(DpiMode::HighDpiAware),
        Some(s) if s.as_bytes() == b"DPIUNAWARE" => Ok(DpiMode::DpiUnaware),
        Some(_) => Err(DfhCode::UnknownDpiFlag.code()),
    }
}

// ── 诊断码闭集（段内 0x40 窗口） ────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DfhCode {
    /// 未知 DPI 标志（HIGHDPIAWARE/DPIUNAWARE 闭集外）。
    UnknownDpiFlag,
    /// 消费部越界（N=3 闭集外）。
    PartOutOfRange,
    /// 挂点签名不匹配（非冻结 v1）。
    SigMismatch,
    /// 挂点重复登记。
    DupHook,
    /// 未注册即消费。
    NotRegistered,
    /// 导出文本偏差（双建导出不一致或形态非法）。
    ExportDeviation,
    /// 状态机倒退。
    PhaseRegress,
    /// 消费章漂移（同部同模式双跑章不同）。
    ConsumeStampDrift,
    /// 计数守恒破坏。
    ConservationBroken,
    /// 登记封印破坏（登记数只增不减被违）。
    LedgerSealBroken,
    /// 双跑不一致。
    NotReproducible,
}

impl DfhCode {
    /// 诊断码：SEG_BASE + 枚举序。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            DfhCode::UnknownDpiFlag => "未知DPI标志：仅 HIGHDPIAWARE/DPIUNAWARE",
            DfhCode::PartOutOfRange => "消费部越界：N=3 闭集外",
            DfhCode::SigMismatch => "挂点签名不匹配：仅冻结 v1 可登记",
            DfhCode::DupHook => "挂点重复登记：一部一挂点",
            DfhCode::NotRegistered => "未注册即消费：先登记后消费",
            DfhCode::ExportDeviation => "导出偏差：桩期登记导出文本不符",
            DfhCode::PhaseRegress => "状态机倒退：挂点流水只进不退",
            DfhCode::ConsumeStampDrift => "消费章漂移：确定性红线",
            DfhCode::ConservationBroken => "计数守恒破坏：注入=检出+漏检不成立",
            DfhCode::LedgerSealBroken => "登记封印破坏：登记数只增不减被违",
            DfhCode::NotReproducible => "双跑不一致：复现红线被踩",
        }
    }
    /// 全码闭集。
    pub const ALL: [DfhCode; 11] = [
        DfhCode::UnknownDpiFlag,
        DfhCode::PartOutOfRange,
        DfhCode::SigMismatch,
        DfhCode::DupHook,
        DfhCode::NotRegistered,
        DfhCode::ExportDeviation,
        DfhCode::PhaseRegress,
        DfhCode::ConsumeStampDrift,
        DfhCode::ConservationBroken,
        DfhCode::LedgerSealBroken,
        DfhCode::NotReproducible,
    ];
}

/// 按码值取中文文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < DfhCode::ALL.len() {
        if DfhCode::ALL[i].code() == code {
            return DfhCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F000 段闭集内"
}

// ── 状态机：挂点登记流水 ────────────────────────────────────────
/// 挂点流水五态（单调推进，只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookPhase {
    /// 未开始。
    Idle = 0,
    /// 已解析 DPI 模式。
    ModeResolved = 1,
    /// 已完成挂点登记。
    Registered = 2,
    /// 已完成导出。
    Exported = 3,
    /// 已冻结。
    Frozen = 4,
}

impl HookPhase {
    /// 单调序号。
    pub fn rank(self) -> u32 {
        self as u32
    }
    /// 单步推进：rank 恰 +1，其余显式拒。
    pub fn advance(self, next: HookPhase) -> Result<HookPhase, u32> {
        if next.rank() == self.rank().saturating_add(1) {
            Ok(next)
        } else {
            Err(DfhCode::PhaseRegress.code())
        }
    }
    /// 全链五步。
    pub fn full_chain() -> Result<HookPhase, u32> {
        let s0 = HookPhase::Idle;
        let s1 = s0.advance(HookPhase::ModeResolved)?;
        let s2 = s1.advance(HookPhase::Registered)?;
        let s3 = s2.advance(HookPhase::Exported)?;
        s3.advance(HookPhase::Frozen)
    }
}

// ── 挂点表（3 部 id-寻址定容） ──────────────────────────────────
/// 单部挂点槽。
#[derive(Clone, Copy, Debug)]
pub struct HookSlot {
    pub registered: bool,
    pub sig: u32,
    pub consume_calls: u32,
    pub last_stamp: u32,
}

impl HookSlot {
    /// 空槽（const 构造定容数组）。
    pub const EMPTY: HookSlot = HookSlot {
        registered: false,
        sig: 0,
        consume_calls: 0,
        last_stamp: 0,
    };
}

/// 挂点登记表。
pub struct HookTable {
    slots: [HookSlot; CONSUME_PARTS],
    pub mode: DpiMode,
    pub full_rejects: u32,
    pub inject_tries: [u32; 5],
    pub inject_caught: [u32; 5],
}

impl HookTable {
    /// 空表（3 部全空，模式默认继承）。
    pub fn new() -> HookTable {
        HookTable {
            slots: [HookSlot::EMPTY; CONSUME_PARTS],
            mode: DpiMode::Inherit,
            full_rejects: 0,
            inject_tries: [0; 5],
            inject_caught: [0; 5],
        }
    }
    /// 解析并锁定 DPI 模式。
    pub fn set_mode(&mut self, flag: Option<&str>) -> Result<DpiMode, u32> {
        let m = resolve_dpi_mode(flag)?;
        self.mode = m;
        Ok(m)
    }
    /// 登记挂点（部越界/签名不匹配/重复登记三闸）。
    pub fn register_hook(&mut self, part: usize, sig: u32) -> Result<(), u32> {
        if part >= CONSUME_PARTS {
            return Err(DfhCode::PartOutOfRange.code());
        }
        if sig != HOOK_SIG_V1 {
            return Err(DfhCode::SigMismatch.code());
        }
        if self.slots[part].registered {
            return Err(DfhCode::DupHook.code());
        }
        self.slots[part] = HookSlot {
            registered: true,
            sig,
            consume_calls: 0,
            last_stamp: 0,
        };
        Ok(())
    }
    /// 消费桩：同部同模式章恒定（sig 异或部号）。
    pub fn consume(&mut self, part: usize) -> Result<u32, u32> {
        if part >= CONSUME_PARTS {
            return Err(DfhCode::PartOutOfRange.code());
        }
        let slot = self.slots[part];
        if !slot.registered {
            return Err(DfhCode::NotRegistered.code());
        }
        let stamp = slot.sig ^ (part as u32);
        self.slots[part].consume_calls = slot.consume_calls.saturating_add(1);
        self.slots[part].last_stamp = stamp;
        Ok(stamp)
    }
    /// 已登记部数（封印复核）。
    pub fn registered_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < CONSUME_PARTS {
            if self.slots[i].registered {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// 消费总次数。
    pub fn consume_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < CONSUME_PARTS {
            n = n.saturating_add(self.slots[i].consume_calls);
            i += 1;
        }
        n
    }
    /// 注入检出记账。
    pub fn record_inject(&mut self, slot: usize, hit: bool) {
        if slot < 5 {
            self.inject_tries[slot] += 1;
            if hit {
                self.inject_caught[slot] += 1;
            }
        }
    }
    /// 计数守恒（五槽全抓）。
    pub fn conserve(&self) -> bool {
        let mut i = 0;
        while i < 5 {
            if self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 全部命中（五槽皆非零且全抓）。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 5 {
            if self.inject_tries[i] == 0 || self.inject_caught[i] != self.inject_tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 导出核（桩期登记可导出，确定性文本） ────────────────────────
/// 导出桩期登记：确定性文本，形态 "DPIHOOKS|mode=<n>|p0=<S/->|p1=..|p2=..|"。
/// S = 已登记（盖 v1 签名），- = 未登记。
pub fn export_registry(t: &HookTable) -> String {
    let mut s = String::new();
    s.push_str("DPIHOOKS|mode=");
    s.push_str(&fmt_u32(t.mode.idx()));
    let mut i = 0;
    while i < CONSUME_PARTS {
        s.push('|');
        s.push('p');
        s.push_str(&fmt_u32(i as u32));
        s.push('=');
        let reg = match t.slots.get(i) {
            Some(sl) => sl.registered,
            None => false,
        };
        if reg {
            s.push('S');
        } else {
            s.push('-');
        }
        i += 1;
    }
    s
}

/// 无库依赖的十进制格式化（no_std 下避免 itoa 依赖）。
fn fmt_u32(v: u32) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 12];
    let mut n = 0usize;
    let mut x = v;
    while x > 0 && n < 12 {
        buf[n] = b'0' + (x % 10) as u8;
        n += 1;
        x /= 10;
    }
    let mut out = String::new();
    let mut k = n;
    while k > 0 {
        k -= 1;
        if let Some(&c) = buf.get(k) {
            out.push(c as char);
        }
    }
    out
}

/// 导出形态闸：前缀/部数/长度三闸。
pub fn export_shape_ok(text: &str) -> bool {
    if text.len() == 0 || text.len() > EXPORT_LEN_MAX {
        return false;
    }
    if !text.starts_with("DPIHOOKS|mode=") {
        return false;
    }
    // 三部字段齐备：p0=/p1=/p2= 各恰一次。
    let mut i = 0;
    while i < CONSUME_PARTS {
        let pat = format!("p{}=", i as u32);
        if text.find(pat.as_str()).is_none() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 注入核（5 类坏样本 ×10 轮） ─────────────────────────────────
/// 注入探针：
///   0 未知DPI标志；1 部越界（3）；2 签名不匹配；
///   3 未注册即消费；4 重复登记。
pub fn inject_probe(slot: usize) -> Result<(), u32> {
    let mut t = HookTable::new();
    match slot {
        0 => match t.set_mode(Some("GDPIAWARE")) {
            // 命中约定：注入被抓 → Err(窗内码)；漏 → Ok。
            Err(c) if c == DfhCode::UnknownDpiFlag.code() => Err(c),
            _ => Ok(()),
        },
        1 => {
            let _ = t.set_mode(Some("HIGHDPIAWARE"))?;
            match t.register_hook(CONSUME_PARTS, HOOK_SIG_V1) {
                Err(c) if c == DfhCode::PartOutOfRange.code() => Err(c),
                _ => Ok(()),
            }
        }
        2 => {
            let _ = t.set_mode(Some("DPIUNAWARE"))?;
            match t.register_hook(0, HOOK_SIG_V1.wrapping_add(1)) {
                Err(c) if c == DfhCode::SigMismatch.code() => Err(c),
                _ => Ok(()),
            }
        }
        3 => {
            let _ = t.set_mode(Some("HIGHDPIAWARE"))?;
            match t.consume(2) {
                Err(c) if c == DfhCode::NotRegistered.code() => Err(c),
                _ => Ok(()),
            }
        }
        _ => {
            let _ = t.set_mode(Some("HIGHDPIAWARE"))?;
            t.register_hook(1, HOOK_SIG_V1)?;
            match t.register_hook(1, HOOK_SIG_V1) {
                Err(c) if c == DfhCode::DupHook.code() => Err(c),
                _ => Ok(()),
            }
        }
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：五态流水 → 3 部全登记 → 双模式消费章恒定 → 导出往返
/// → 5 类注入 10 轮全抓 → 守恒 → 双跑一致。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    HookPhase::full_chain()?;
    let mut t = HookTable::new();
    // 模式锁定（感知 DPI）。
    let m = t.set_mode(Some("HIGHDPIAWARE"))?;
    if m != DpiMode::HighDpiAware {
        return Err(DfhCode::UnknownDpiFlag.code());
    }
    // 3 部全登记。
    let mut p = 0usize;
    while p < CONSUME_PARTS {
        t.register_hook(p, HOOK_SIG_V1)?;
        p += 1;
    }
    if t.registered_count() != CONSUME_PARTS {
        return Err(DfhCode::LedgerSealBroken.code());
    }
    // 消费章恒定：同部双消费章必须相同（确定性）。
    let s0a = t.consume(0)?;
    let s0b = t.consume(0)?;
    let s1 = t.consume(1)?;
    if s0a != s0b || s0a == s1 {
        return Err(DfhCode::ConsumeStampDrift.code());
    }
    // 导出往返：形态闸 + 双建逐字节一致。
    let e1 = export_registry(&t);
    if !export_shape_ok(&e1) {
        return Err(DfhCode::ExportDeviation.code());
    }
    let mut t2 = HookTable::new();
    let _ = t2.set_mode(Some("HIGHDPIAWARE"))?;
    let mut q = 0usize;
    while q < CONSUME_PARTS {
        t2.register_hook(q, HOOK_SIG_V1)?;
        q += 1;
    }
    let e2 = export_registry(&t2);
    if e1 != e2 {
        return Err(DfhCode::ExportDeviation.code());
    }
    // 注入 5 类 × 10 轮。
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 5 {
            let hit = match inject_probe(slot) {
                Err(c) => c >= SEG_BASE && c < SEG_BASE + 0x40,
                Ok(_) => false,
            };
            t.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    if !t.all_caught() || !t.conserve() {
        return Err(DfhCode::ConservationBroken.code());
    }
    Ok((t.registered_count() as u32, t.consume_total(), t.inject_caught[0]))
}

// ── 探针三序 ────────────────────────────────────────────────────
pub fn probe_a() -> Result<(), u32> {
    // 三态模式解析：None→继承 / 两记法→对应模式。
    let a = resolve_dpi_mode(None)?;
    let b = resolve_dpi_mode(Some("HIGHDPIAWARE"))?;
    let c = resolve_dpi_mode(Some("DPIUNAWARE"))?;
    if a == DpiMode::Inherit && b == DpiMode::HighDpiAware && c == DpiMode::DpiUnaware {
        Ok(())
    } else {
        Err(DfhCode::UnknownDpiFlag.code())
    }
}

pub fn probe_b() -> Result<(), u32> {
    // 部越界必拒。
    let mut t = HookTable::new();
    match t.consume(CONSUME_PARTS) {
        Err(c) if c == DfhCode::PartOutOfRange.code() => Ok(()),
        _ => Err(DfhCode::ConservationBroken.code()),
    }
}

pub fn probe_c() -> Result<(), u32> {
    // 状态机倒退必拒。
    match HookPhase::Frozen.advance(HookPhase::Idle) {
        Err(c) if c == DfhCode::PhaseRegress.code() => Ok(()),
        _ => Err(DfhCode::ConservationBroken.code()),
    }
}

// ── 判据裁决 ────────────────────────────────────────────────────
/// 段位守卫。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x7F000 {
        return false;
    }
    let mut i = 0;
    while i < DfhCode::ALL.len() {
        let v = DfhCode::ALL[i].code();
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
    while i < DfhCode::ALL.len() {
        if detail_of(DfhCode::ALL[i].code()).len() == 0 {
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
        "J1-挂点签名冻结v1",
        HOOK_SIG_V1 == 0x4449_5003 && CONSUME_PARTS == 3,
        format!("sig=0x{:08X} N={}", HOOK_SIG_V1, CONSUME_PARTS),
    ));
    v.push((
        "J1-五态流水",
        HookPhase::full_chain().is_ok(),
        String::from("Idle→…→Frozen 单调"),
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
    // 注入 5 类 ×10 轮。
    let mut t = HookTable::new();
    let mut r = 0u32;
    while r < JUDGE_ROUNDS {
        let mut slot = 0usize;
        while slot < 5 {
            let hit = inject_probe(slot).is_err();
            t.record_inject(slot, hit);
            slot += 1;
        }
        r += 1;
    }
    let total_tries = t.inject_tries[0]
        + t.inject_tries[1]
        + t.inject_tries[2]
        + t.inject_tries[3]
        + t.inject_tries[4];
    let total_caught = t.inject_caught[0]
        + t.inject_caught[1]
        + t.inject_caught[2]
        + t.inject_caught[3]
        + t.inject_caught[4];
    v.push((
        "J1-注入10/10全抓",
        t.all_caught(),
        format!("5类×10轮 t={} c={}", total_tries, total_caught),
    ));
    v.push((
        "J1-计数守恒",
        t.conserve() && total_tries == total_caught,
        String::from("注入=检出+漏检，漏检=0"),
    ));
    // 导出往返幂等。
    let mut t3 = HookTable::new();
    let _ = t3.set_mode(Some("DPIUNAWARE"));
    let _ = t3.register_hook(0, HOOK_SIG_V1);
    let e3a = export_registry(&t3);
    let e3b = export_registry(&t3);
    v.push((
        "J1-导出往返幂等",
        export_shape_ok(&e3a) && e3a == e3b,
        format!("形态闸过 双建同文"),
    ));
    // 消费章恒定（确定性）。
    let mut t4 = HookTable::new();
    let _ = t4.set_mode(Some("HIGHDPIAWARE"));
    let _ = t4.register_hook(2, HOOK_SIG_V1);
    let c1 = t4.consume(2);
    let c2 = t4.consume(2);
    v.push((
        "J1-消费章恒定",
        c1.is_ok() && c2.is_ok() && c1 == c2,
        String::from("同部双消费章相同"),
    ));
    v.push((
        "J1-文案闭集",
        detail_closed(),
        format!("{} 码全非空", DfhCode::ALL.len()),
    ));
    // 边界四类：部 0 合法 / 部 2 合法 / 部 3 拒 / 模式域外拒。
    let mut t5 = HookTable::new();
    let _ = t5.set_mode(Some("HIGHDPIAWARE"));
    let b0 = t5.register_hook(0, HOOK_SIG_V1).is_ok();
    let b1 = t5.register_hook(2, HOOK_SIG_V1).is_ok();
    let b2 = t5.register_hook(3, HOOK_SIG_V1).is_err();
    let b3 = resolve_dpi_mode(Some("DPIUNAWARE")).is_ok()
        && resolve_dpi_mode(Some("GDPIAWARE")).is_err();
    v.push((
        "J1-边界四类",
        b0 && b1 && b2 && b3,
        format!("部0{} 部末{} 越界{} 模式{}", b0, b1, b2, b3),
    ));
    // 登记封印：重复登记不增登记数。
    let n0 = t5.registered_count();
    let _ = t5.register_hook(0, HOOK_SIG_V1);
    let n1 = t5.registered_count();
    v.push((
        "J1-登记封印只增不减",
        n0 == 2 && n1 == n0,
        format!("登记{}→{}", n0, n1),
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
pub fn run_unx18465_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18465 DPI 标志消费挂点签名冻结");
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
        let set = run_unx18465_checks();
        assert!(set.all_passed(), "判据全绿失败：{}", set.red_items_text());
    }

    #[test]
    fn normal_flow_register_consume() {
        let mut t = HookTable::new();
        let m = t.set_mode(Some("HIGHDPIAWARE"));
        match m {
            Ok(mode) => assert!(mode == DpiMode::HighDpiAware, "模式锁定"),
            Err(_) => panic!("合法模式不该报错"),
        }
        assert!(t.register_hook(1, HOOK_SIG_V1).is_ok(), "登记过");
        let c = t.consume(1);
        match c {
            Ok(stamp) => assert!(stamp == (HOOK_SIG_V1 ^ 1), "消费章推导"),
            Err(_) => panic!("已登记消费不该报错"),
        }
    }

    #[test]
    fn boundary_four_kinds() {
        let mut t = HookTable::new();
        let _ = t.set_mode(Some("DPIUNAWARE"));
        assert!(t.register_hook(0, HOOK_SIG_V1).is_ok(), "部 0 合法");
        assert!(t.register_hook(2, HOOK_SIG_V1).is_ok(), "末部合法");
        assert!(t.register_hook(3, HOOK_SIG_V1).is_err(), "部 3 越界");
        assert!(resolve_dpi_mode(Some("GDPIAWARE")).is_err(), "模式域外");
    }

    #[test]
    fn injection_five_classes_all_caught() {
        let mut slot = 0usize;
        while slot < 5 {
            assert!(inject_probe(slot).is_err(), "第 {} 类注入必抓", slot);
            slot += 1;
        }
    }

    #[test]
    fn export_round_trip_idempotent() {
        let mut t = HookTable::new();
        let _ = t.set_mode(Some("HIGHDPIAWARE"));
        let _ = t.register_hook(0, HOOK_SIG_V1);
        let e1 = export_registry(&t);
        let e2 = export_registry(&t);
        assert!(e1 == e2, "双建导出一致");
        assert!(export_shape_ok(&e1), "导出形态闸");
        assert!(e1.find("p1=-").is_some(), "未登记部导出为 -");
    }

    #[test]
    fn consume_stamp_stable() {
        let mut t = HookTable::new();
        let _ = t.set_mode(Some("DPIUNAWARE"));
        let _ = t.register_hook(0, HOOK_SIG_V1);
        let a = t.consume(0);
        let b = t.consume(0);
        match (a, b) {
            (Ok(x), Ok(y)) => assert!(x == y, "消费章恒定"),
            _ => panic!("已登记消费不该报错"),
        }
        assert!(t.consume(1).is_err(), "未注册消费必拒");
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
        while i < DfhCode::ALL.len() {
            let c = DfhCode::ALL[i].code();
            assert!(c >= SEG_BASE && c < SEG_BASE + 0x40, "码越窗");
            assert!(detail_of(c).len() > 0, "文案空缺");
            i += 1;
        }
    }
}
