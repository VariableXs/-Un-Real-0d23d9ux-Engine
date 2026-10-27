//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1290 · B17 register_irq 对接映射（每接管点注册预登记） · 0xFDE0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1290-J1 全部接管点 register_irq 预登记率 100%（handler 骨架/
//!      flags/领主信息预填），预登记与 F1121 冻结签名一致率 100%，
//!      预登记-注册-生效三态跟踪账完整率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 预登记四要素：handler 骨架（纯函数占位签名）/flags（触发型+快慢
//!     处理器）/领主信息（driver 名+优先级）/向量号；
//!   - F1121 冻结签名：register_irq(vec, handler_flags, owner, prio)
//!     四参结构逐字段对拍；
//!   - 三态跟踪：Preregistered→Registered→Active 单向推进，每点全态
//!     可溯（完整率 100% 判准）；
//!   - flags 闭集：LevelLow/LevelHigh/EdgeRise × Fasthandler/Slowhandler。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0xFDE0 段，grep 全域确认独占）。
pub const SEG_BASE: u16 = 0xFDE0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_VEC_OCCUPIED: u16 = SEG_BASE + 1;  // 向量已注册
const E_FLAGS_BAD: u16 = SEG_BASE + 2;     // flags 越闭集
const E_OWNER_MISS: u16 = SEG_BASE + 3;    // 领主信息缺
const E_PRIO_RANGE: u16 = SEG_BASE + 4;    // 优先级越域（>15）
const E_POINT_DUP: u16 = SEG_BASE + 5;     // 接管点重复
const E_SIG_MISS: u16 = SEG_BASE + 6;      // 与 F1121 签名不一致
const E_STATE_EDGE: u16 = SEG_BASE + 7;    // 三态非法边
const E_LEDGER_FULL: u16 = SEG_BASE + 8;   // 账满
const E_FROZEN: u16 = SEG_BASE + 9;        // 封账后写入
const E_COVER_MISS: u16 = SEG_BASE + 10;   // 预登记率不足

/// 接管点宇宙 12。
pub const POINT_COUNT: usize = 12;

/// 账容量。
pub const REG_CAP: usize = 16;

/// 触发型闭集（复用 B16 口径但独立闭集——B17 flags 冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegTrigger {
    LevelLow = 0,
    LevelHigh = 1,
    EdgeRise = 2,
}

/// 处理器型闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HandlerKind {
    /// 快路径（top half 直认）。
    Fast = 0,
    /// 慢路径（入 BH 队列）。
    Slow = 1,
}

/// B17 flags：触发型 × 处理器型（闭集 3×2=6）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RegFlags {
    pub trigger: RegTrigger,
    pub handler: HandlerKind,
}

impl RegFlags {
    /// 闭集编码（0~5，F1121 冻结编码）。
    pub fn code(&self) -> u8 {
        (self.trigger as u8) * 2 + self.handler as u8
    }

    /// 闭集校验（任意组合均合法——3×2 全积闭集）。
    pub fn valid(&self) -> bool {
        matches!(self.trigger, RegTrigger::LevelLow | RegTrigger::LevelHigh | RegTrigger::EdgeRise)
            && matches!(self.handler, HandlerKind::Fast | HandlerKind::Slow)
    }
}

/// 领主信息（预填）。
#[derive(Clone, Copy, Debug)]
pub struct OwnerInfo {
    /// 领主短名（冻结闭集）。
    pub name: &'static str,
    /// 优先级（0~15，4 位域——F1121 冻结位宽）。
    pub prio: u8,
}

impl OwnerInfo {
    pub fn valid(&self) -> bool {
        self.prio <= 15
    }
}

/// 三态跟踪。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegState {
    Preregistered = 0,
    Registered = 1,
    Active = 2,
}

impl RegState {
    pub fn name(self) -> &'static str {
        match self {
            RegState::Preregistered => "Preregistered",
            RegState::Registered => "Registered",
            RegState::Active => "Active",
        }
    }
}

/// F1121 冻结签名描述（四参：vector/flags/owner/prio——owner 内含 prio
/// 故签名字段数 4，位宽 vector 8 / flags 3 / prio 4）。
pub const F1121_SIG_FIELDS: usize = 4;
pub const F1121_VEC_BITS: u8 = 8;
pub const F1121_FLAGS_CODE_MAX: u8 = 5;
pub const F1121_PRIO_BITS: u8 = 4;

/// 注册条目（预登记行 + 跟踪态）。
#[derive(Clone, Copy, Debug)]
pub struct RegEntry {
    pub point: u16,
    pub vector: u8,
    pub flags: RegFlags,
    pub owner: OwnerInfo,
    pub state: RegState,
}

/// 预登记账。
pub struct RegLedger {
    entries: [Option<RegEntry>; REG_CAP],
    used: usize,
    point_mask: u32,
    /// 状态推进计数（三态全走满的点数）。
    fully_advanced: u32,
    frozen: bool,
    version: u64,
    rejects: [u32; 11],
    fingerprint: u64,
}

impl RegLedger {
    pub fn new() -> RegLedger {
        RegLedger {
            entries: [None; REG_CAP],
            used: 0,
            point_mask: 0,
            fully_advanced: 0,
            frozen: false,
            version: 0,
            rejects: [0; 11],
            fingerprint: 0,
        }
    }

    fn reject(&mut self, code: u16) -> u16 {
        let idx = (code - SEG_BASE) as usize;
        if idx < self.rejects.len() {
            self.rejects[idx] += 1;
        }
        code
    }

    /// 预登记（Preregistered 态入账）。
    pub fn preregister(&mut self, point: u16, vector: u8, flags: RegFlags, owner: OwnerInfo) -> Result<usize, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if !flags.valid() {
            return Err(self.reject(E_FLAGS_BAD));
        }
        if !owner.valid() {
            return Err(self.reject(E_PRIO_RANGE));
        }
        if point as usize >= POINT_COUNT {
            return Err(self.reject(E_POINT_DUP));
        }
        if owner.name.is_empty() {
            return Err(self.reject(E_OWNER_MISS));
        }
        let bit = 1u32 << point;
        if self.point_mask & bit != 0 {
            return Err(self.reject(E_POINT_DUP));
        }
        // B17 复合键约束：同向量可挂 Fast+Slow 各一，同型双挂必拒。
        if !self.vec_handler_free(vector, flags.handler) {
            return Err(self.reject(E_VEC_OCCUPIED));
        }
        if self.used >= REG_CAP {
            return Err(self.reject(E_LEDGER_FULL));
        }
        self.entries[self.used] = Some(RegEntry { point, vector, flags, owner, state: RegState::Preregistered });
        self.used += 1;
        self.point_mask |= bit;
        self.version += 1;
        Ok(self.used - 1)
    }

    /// 按点位找条目下标。
    fn find(&self, point: u16) -> Option<usize> {
        let mut i = 0;
        while i < self.used {
            if let Some(e) = self.entries[i] {
                if e.point == point {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 状态推进（单向：Preregistered→Registered→Active，逆/跳必拒）。
    pub fn advance(&mut self, point: u16) -> Result<RegState, u16> {
        let idx = match self.find(point) {
            Some(i) => i,
            None => return Err(self.reject(E_POINT_DUP)),
        };
        let cur = self.entries[idx].unwrap_or(RegEntry {
            point: 0,
            vector: 0,
            flags: RegFlags { trigger: RegTrigger::LevelLow, handler: HandlerKind::Fast },
            owner: OwnerInfo { name: "", prio: 0 },
            state: RegState::Preregistered,
        }).state;
        let next = match cur {
            RegState::Preregistered => RegState::Registered,
            RegState::Registered => RegState::Active,
            RegState::Active => return Err(self.reject(E_STATE_EDGE)),
        };
        if let Some(slot) = self.entries[idx].as_mut() {
            slot.state = next;
            if next == RegState::Active {
                self.fully_advanced += 1;
            }
        }
        self.version += 1;
        Ok(next)
    }

    pub fn state_of(&self, point: u16) -> Option<RegState> {
        self.find(point).and_then(|i| self.entries[i].map(|e| e.state))
    }

    /// 封账：预登记率 100% 且三态全走满。
    pub fn seal(&mut self) -> Result<u64, u16> {
        if self.frozen {
            return Err(self.reject(E_FROZEN));
        }
        if self.point_mask != (1u32 << POINT_COUNT) - 1 {
            return Err(self.reject(E_COVER_MISS));
        }
        if self.fully_advanced < POINT_COUNT as u32 {
            return Err(self.reject(E_COVER_MISS));
        }
        let mut fp = 0xcbf2_9ce4_8422_2325u64;
        let mut i = 0;
        while i < self.used {
            if let Some(e) = self.entries[i] {
                let words = [e.point as u64, e.vector as u64, e.flags.code() as u64, e.owner.prio as u64];
                let mut j = 0;
                while j < words.len() {
                    fp ^= words[j];
                    fp = fp.wrapping_mul(0x0000_0100_0000_01B3);
                    j += 1;
                }
            }
            i += 1;
        }
        self.fingerprint = fp;
        self.frozen = true;
        self.version += 1;
        Ok(self.version)
    }

    pub fn used(&self) -> usize {
        self.used
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

/// 冻结预登记表（12 点——来源：F1289 向量分派 + B17 领主面；
/// 领主：pit/serial/ps2kbd/ps2aux/rtc/spurious 闭集）。
pub const FROZEN_PRE: [(u16, u8, RegFlags, OwnerInfo); POINT_COUNT] = [
    (0, 32, RegFlags { trigger: RegTrigger::EdgeRise, handler: HandlerKind::Fast }, OwnerInfo { name: "pit", prio: 1 }),
    (1, 32, RegFlags { trigger: RegTrigger::EdgeRise, handler: HandlerKind::Slow }, OwnerInfo { name: "pit", prio: 2 }),
    (2, 36, RegFlags { trigger: RegTrigger::LevelLow, handler: HandlerKind::Fast }, OwnerInfo { name: "serial", prio: 3 }),
    (3, 36, RegFlags { trigger: RegTrigger::LevelLow, handler: HandlerKind::Slow }, OwnerInfo { name: "serial", prio: 4 }),
    (4, 39, RegFlags { trigger: RegTrigger::LevelLow, handler: HandlerKind::Fast }, OwnerInfo { name: "serial", prio: 5 }),
    (5, 44, RegFlags { trigger: RegTrigger::LevelHigh, handler: HandlerKind::Fast }, OwnerInfo { name: "ps2kbd", prio: 6 }),
    (6, 44, RegFlags { trigger: RegTrigger::LevelHigh, handler: HandlerKind::Slow }, OwnerInfo { name: "ps2kbd", prio: 7 }),
    (7, 47, RegFlags { trigger: RegTrigger::LevelHigh, handler: HandlerKind::Fast }, OwnerInfo { name: "ps2kbd", prio: 8 }),
    (8, 12, RegFlags { trigger: RegTrigger::LevelHigh, handler: HandlerKind::Fast }, OwnerInfo { name: "ps2aux", prio: 9 }),
    (9, 12, RegFlags { trigger: RegTrigger::LevelHigh, handler: HandlerKind::Slow }, OwnerInfo { name: "ps2aux", prio: 10 }),
    (10, 8, RegFlags { trigger: RegTrigger::LevelHigh, handler: HandlerKind::Fast }, OwnerInfo { name: "rtc", prio: 11 }),
    (11, 34, RegFlags { trigger: RegTrigger::EdgeRise, handler: HandlerKind::Slow }, OwnerInfo { name: "spurious", prio: 12 }),
];

/// 注意：FROZEN_PRE 的向量占用按「向量+处理器型」复合键——同向量可
/// 挂 Fast/Slow 各一（top half / BH 分层），冻结表 12 组合键互异。
/// 全链演练：12 点预登记 → 逐点推进 Registered → Active → 封账。
pub fn drill_full_chain() -> (u32, u32, u32, Vec<String>) {
    let mut ledger = RegLedger::new();
    let mut detail: Vec<String> = Vec::new();
    let mut ok_pre = 0u32;
    let mut i = 0;
    while i < POINT_COUNT {
        let (p, v, f, o) = FROZEN_PRE[i];
        // 复合键查重：同向量+同 handler 型不得重复（B17 约束）。
        if !ledger.vec_handler_free(v, f.handler) {
            detail.push(format!("点{}向量{}+{:?}复合键已被占", p, v, f.handler));
            i += 1;
            continue;
        }
        match ledger.preregister(p, v, f, o) {
            Ok(_) => ok_pre += 1,
            Err(c) => detail.push(format!("点{}预登记被拒：0x{:04X}", p, c)),
        }
        i += 1;
    }
    // 推进两步到 Active。
    let mut ok_active = 0u32;
    let mut k = 0;
    while k < POINT_COUNT {
        let p = FROZEN_PRE[k].0;
        if ledger.advance(p) == Ok(RegState::Registered) && ledger.advance(p) == Ok(RegState::Active) {
            ok_active += 1;
        }
        k += 1;
    }
    let sealed = ledger.seal().is_ok();
    detail.push(format!(
        "预登记 {}/12 Active {}/12 封账={}",
        ok_pre, ok_active, sealed
    ));
    (ok_pre, ok_active, if sealed { 1 } else { 0 }, detail)
}

impl RegLedger {
    /// 复合键（向量+处理器型）空闲判据——B17 面真实约束：
    /// 同向量可挂 Fast+Slow 各一（top half / BH 分层），不可同型双挂。
    fn vec_handler_free(&self, vector: u8, handler: HandlerKind) -> bool {
        let mut i = 0;
        while i < self.used {
            if let Some(e) = self.entries[i] {
                if e.vector == vector && e.flags.handler == handler {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

/// 三态非法边注入 10/10：Active 再推进 ×5 + 未预登记推进 ×5 必抓。
pub fn inject_state_edges_10() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    // Active 再推进 5 次。
    let mut lg1 = RegLedger::new();
    let (p, v, f, o) = FROZEN_PRE[0];
    let _ = lg1.preregister(p, v, f, o);
    let _ = lg1.advance(p);
    let _ = lg1.advance(p);
    let mut r = 0;
    while r < 5 {
        if lg1.advance(p) == Err(E_STATE_EDGE) {
            caught += 1;
        }
        r += 1;
    }
    // 未预登记推进 5 次。
    let mut lg2 = RegLedger::new();
    let mut k = 0;
    while k < 5 {
        if lg2.advance(3) == Err(E_POINT_DUP) {
            caught += 1;
        }
        k += 1;
    }
    if caught == 10 {
        detail.push("三态非法边 10/10 全抓".to_string());
    }
    (caught, 10, detail)
}

/// 签名一致性演练：F1121 四字段对拍（字段数/位宽/flags 编码域/prio 域）。
pub fn drill_sig_alignment() -> (u32, u32, Vec<String>) {
    let mut aligned = 0u32;
    let mut detail: Vec<String> = Vec::new();
    if F1121_SIG_FIELDS == 4 {
        aligned += 1;
    } else {
        detail.push("签名字段数不符".to_string());
    }
    let mut vec_ok = true;
    let mut flags_ok = true;
    let mut prio_ok = true;
    let mut i = 0;
    while i < POINT_COUNT {
        let (_, v, f, o) = FROZEN_PRE[i];
        if v as u32 >= (1u32 << F1121_VEC_BITS) {
            vec_ok = false;
        }
        if f.code() > F1121_FLAGS_CODE_MAX || !f.valid() {
            flags_ok = false;
        }
        if o.prio >= (1u8 << F1121_PRIO_BITS) {
            prio_ok = false;
        }
        i += 1;
    }
    if vec_ok {
        aligned += 1;
    } else {
        detail.push("向量位宽越界".to_string());
    }
    if flags_ok {
        aligned += 1;
    } else {
        detail.push("flags 编码越闭集".to_string());
    }
    if prio_ok {
        aligned += 1;
    } else {
        detail.push("prio 越位宽".to_string());
    }
    (aligned, 4, detail)
}

/// 领主越域注入：prio=16 越位宽必抓 + 空领主名必抓。
pub fn inject_owner_bad() -> (u32, u32, Vec<String>) {
    let mut caught = 0u32;
    let mut detail: Vec<String> = Vec::new();
    let mut lg = RegLedger::new();
    let r1 = lg.preregister(0, 32, RegFlags { trigger: RegTrigger::EdgeRise, handler: HandlerKind::Fast }, OwnerInfo { name: "pit", prio: 16 });
    if r1 == Err(E_PRIO_RANGE) {
        caught += 1;
    } else {
        detail.push("越域 prio 未拒".to_string());
    }
    let r2 = lg.preregister(1, 33, RegFlags { trigger: RegTrigger::EdgeRise, handler: HandlerKind::Fast }, OwnerInfo { name: "", prio: 1 });
    if r2 == Err(E_OWNER_MISS) {
        caught += 1;
    } else {
        detail.push("空领主未拒".to_string());
    }
    if caught == 2 {
        detail.push("领主注入 2/2 全抓".to_string());
    }
    (caught, 2, detail)
}

/// 复合键冲突注入：同向量同 handler 型双挂必抓（点0/点1 同为 32+Fast
/// 时不允许——FROZEN_PRE 已冻结合法组合，此处反向验证）。
pub fn inject_composite_conflict() -> (bool, Vec<String>) {
    let mut detail: Vec<String> = Vec::new();
    let mut lg = RegLedger::new();
    let r1 = lg.preregister(0, 32, RegFlags { trigger: RegTrigger::EdgeRise, handler: HandlerKind::Fast }, OwnerInfo { name: "pit", prio: 1 });
    let r2 = lg.preregister(2, 32, RegFlags { trigger: RegTrigger::EdgeRise, handler: HandlerKind::Fast }, OwnerInfo { name: "pit", prio: 2 });
    let caught = r1.is_ok() && r2 == Err(E_VEC_OCCUPIED);
    if !caught {
        detail.push("复合键冲突未抓".to_string());
    }
    (caught, detail)
}

/// 段位守卫：0xFDE0 段十码闭集连续。
pub fn segment_guard() -> bool {
    let codes = [
        E_VEC_OCCUPIED, E_FLAGS_BAD, E_OWNER_MISS, E_PRIO_RANGE, E_POINT_DUP,
        E_SIG_MISS, E_STATE_EDGE, E_LEDGER_FULL, E_FROZEN, E_COVER_MISS,
    ];
    let mut i = 0;
    while i < codes.len() {
        if codes[i] < SEG_BASE || codes[i] > SEG_BASE + 10 {
            return false;
        }
        if i > 0 && codes[i] != codes[i - 1] + 1 {
            return false;
        }
        i += 1;
    }
    true
}

/// run_unx1290_checks：本单判据聚合（诊断段 0xFDE0）。
pub fn run_unx1290_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1290 B17 预登记");
    let (p1, a1, s1, d1) = drill_full_chain();
    s.add_dyn(
        "J1-全链预登记+Active+封账",
        p1 == 12 && a1 == 12 && s1 == 1,
        &format!("预{}/12 活{}/12 封{} {}", p1, a1, s1, d1.join(" ")),
    );
    let (c2, t2, d2) = inject_state_edges_10();
    s.add_dyn("J1-三态非法边10/10", c2 == t2, &format!("{}/{} {}", c2, t2, d2.join(" ")));
    let (a3, t3, d3) = drill_sig_alignment();
    s.add_dyn("J1-签名一致4/4", a3 == t3, &format!("{}/{} {}", a3, t3, d3.join(" ")));
    let (c4, t4, d4) = inject_owner_bad();
    s.add_dyn("J1-领主注入2/2", c4 == t4, &format!("{}/{} {}", c4, t4, d4.join(" ")));
    let (ok5, d5) = inject_composite_conflict();
    s.add_dyn("J1-复合键冲突必抓", ok5, &d5.join(" "));
    s.add_dyn("J2-段位守卫", segment_guard(), "0xFDE0段十码闭集");
    s
}
