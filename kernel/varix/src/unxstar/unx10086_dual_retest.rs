//! UNX-F10086 双判据复测协议（10/10 双口径） · 0x92540 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10086（把 J1/J2 双口径 10/10 复测协议冻成账与闸）。
//! 锚点：UNX-F10087-J1 J2 红通道生效（反判据红入 F9747 挂 J2 标记
//!       10/10 次），混账 10/10 次拦截。
//!
//! 数据段说明：
//!   SEG_BASE = 0x92540（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 变体序号，段内两两唯一、段外不越界；
//!   全部码位由 `RetestCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 注册表 `DualTrackRegistry`：双判据条目逐条登记 J1/J2 双口径档位，
//!     缺口径档位 / 越界 / 重复 显性拒绝；
//!   - 复测协议机 `RetestProtocol`：状态机 Idle → J1Done → J2Done →
//!     Certified 单向推进，J1 先 J2 后、逐口径 10 轮；
//!   - 双口径留痕账 `TrackLedger`：J1 与 J2 各 10 轮逐轮留痕，
//!     账守恒（登记=留痕），缺口径留痕即协议不推进；
//!   - J2 红通道预核 `J2RedProbe`：反判据红必须挂 J2 标记才能入
//!     F9747 红账（联动 F10087），混账样本逐次拦截；
//!   - 注入核 `BadRetestCase`：单口径复测 / 口径顺序颠倒 / 轮数越界 /
//!     重复留痕 / 未认证即出报告 五类坏样本逐类 10/10 次拦截；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10086 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x92540;

/// 段窗上界（下一卡 0x92580 起点，闭开区间右端）。
pub const SEG_END: u32 = 0x92580;

/// 双判据条目槽数（6 条 J1/J2 成对条目）。
pub const DUAL_SLOTS: usize = 6;

/// 双口径复测轮数（J1 与 J2 各 10/10）。
pub const RETEST_ROUNDS: u32 = 10;

/// 注入复测轮数（逐类 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// J2 标记字面量（反判据红入账的必挂标记）。
pub const J2_TAG: &str = "J2:";

/// Linux errno 正数码（对照表口径）。
pub const EINVAL: i32 = 22;
pub const EALREADY: i32 = 114;
pub const EBUSY: i32 = 16;
pub const ENOENT: i32 = 2;
pub const EPERM: i32 = 1;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RetestCode {
    /// 缺口径档位（条目只挂 J1 或只挂 J2）
    TrackMissing,
    /// 口径顺序颠倒（J2 未毕先跑 J1 补测）
    TrackOrderBroken,
    /// 轮数越界（轮号出 1..=10 窗）
    RoundRange,
    /// 重复留痕（同条目同口径同轮号）
    DuplicateRow,
    /// 单口径复测（缺 J1 或缺 J2 的 10 轮）
    OneSided,
    /// 未认证即出报告
    CertMissing,
    /// 条目越界
    SlotRange,
    /// 重复登记同条目
    DuplicateSlot,
    /// 状态倒退
    StateRegress,
    /// 混账（J2 红缺 J2 标记 / J1 红错挂 J2 标记）
    MixedLedger,
    /// 账失恒（登记轮数≠留痕轮数）
    LedgerDrift,
    /// 段位越界
    SegmentViolation,
}

impl RetestCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RetestCode::TrackMissing => "缺口径档位被拒：条目必须 J1/J2 双口径齐挂；请补另一口径",
            RetestCode::TrackOrderBroken => "口径顺序颠倒被拒：J1 先 J2 后；请按协议序复测",
            RetestCode::RoundRange => "轮数越界被拒：轮号只允许 1..=10；请核对轮号",
            RetestCode::DuplicateRow => "重复留痕被拒：同条目同口径同轮号已留痕；请查重",
            RetestCode::OneSided => "单口径复测被拒：J1 与 J2 各 10 轮齐才算复测；请补缺口径",
            RetestCode::CertMissing => "未认证出报告被拒：先双口径认证再出报告；请按序推进",
            RetestCode::SlotRange => "条目越界被拒：登记下标只允许 0..6；请核对条目号",
            RetestCode::DuplicateSlot => "重复登记被拒：同条目已存在；请先核查再改",
            RetestCode::StateRegress => "状态倒退被拒：Idle→J1Done→J2Done→Certified 单向；请排查回滚",
            RetestCode::MixedLedger => "混账被拒：J2 红必须挂 J2 标记、J1 红不得挂；请对号入账",
            RetestCode::LedgerDrift => "账失恒被拒：登记轮数必须等于留痕轮数；请对账补录",
            RetestCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x92540..=0x92580",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [RetestCode; 12] = [
        RetestCode::TrackMissing,
        RetestCode::TrackOrderBroken,
        RetestCode::RoundRange,
        RetestCode::DuplicateRow,
        RetestCode::OneSided,
        RetestCode::CertMissing,
        RetestCode::SlotRange,
        RetestCode::DuplicateSlot,
        RetestCode::StateRegress,
        RetestCode::MixedLedger,
        RetestCode::LedgerDrift,
        RetestCode::SegmentViolation,
    ];
}

// ── 双口径档位与注册表 ────────────────────────────────────────────────
/// 双判据口径闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Track {
    /// J1 正口径
    J1,
    /// J2 反口径
    J2,
}

/// 一条双判据条目的档位：J1 与 J2 各占一位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualSlot {
    /// J1 口径已挂
    pub j1: bool,
    /// J2 口径已挂
    pub j2: bool,
}

/// 注册表：6 条双判据条目逐条登记口径档位。
#[derive(Clone, Debug)]
pub struct DualTrackRegistry {
    /// 条目档位
    pub slots: [DualSlot; DUAL_SLOTS],
}

impl DualTrackRegistry {
    /// 构造空注册表。
    pub const fn new() -> DualTrackRegistry {
        DualTrackRegistry { slots: [DualSlot { j1: false, j2: false }; DUAL_SLOTS] }
    }

    /// 挂一条口径：条目号必检、同口径重复必拒。
    pub fn attach(&mut self, slot: usize, track: Track) -> Result<usize, RetestCode> {
        if slot >= DUAL_SLOTS {
            return Err(RetestCode::SlotRange);
        }
        let cur = match self.slots.get(slot) {
            Some(s) => *s,
            None => return Err(RetestCode::SlotRange),
        };
        let dup = match track {
            Track::J1 => cur.j1,
            Track::J2 => cur.j2,
        };
        if dup {
            return Err(RetestCode::DuplicateSlot);
        }
        let next = match track {
            Track::J1 => DualSlot { j1: true, j2: cur.j2 },
            Track::J2 => DualSlot { j1: cur.j1, j2: true },
        };
        if let Some(s) = self.slots.get_mut(slot) {
            *s = next;
        }
        Ok(slot)
    }

    /// 条目双口径是否齐挂。
    pub fn slot_dual(&self, slot: usize) -> bool {
        match self.slots.get(slot) {
            Some(s) => s.j1 && s.j2,
            None => false,
        }
    }
}

/// 标准登记：6 条条目 J1/J2 双口径齐挂。
pub fn registry_standard(reg: &mut DualTrackRegistry) -> Result<(), RetestCode> {
    let mut n = 0usize;
    while n < DUAL_SLOTS {
        reg.attach(n, Track::J1)?;
        reg.attach(n, Track::J2)?;
        n += 1;
    }
    Ok(())
}

// ── 复测协议机（状态机） ──────────────────────────────────────────────
/// 状态机：Idle → J1Done → J2Done → Certified 单向推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProtoState {
    /// 空闲（J1 口径未复测）
    Idle,
    /// J1 口径已毕（10 轮留痕齐）
    J1Done,
    /// J2 口径已毕（10 轮留痕齐）
    J2Done,
    /// 双口径已认证
    Certified,
}

/// 复测协议机：按 J1 先 J2 后的顺序推进，缺口径即拒。
#[derive(Clone, Debug)]
pub struct RetestProtocol {
    /// 双口径留痕账（外部账由协议机逐轮写入）
    pub ledger: TrackLedger,
    /// 当前状态
    pub state: ProtoState,
}

impl RetestProtocol {
    /// 构造空协议机。
    pub fn new() -> RetestProtocol {
        RetestProtocol { ledger: TrackLedger::new(), state: ProtoState::Idle }
    }

    /// 复测 J1 口径：Idle 态专用，10 轮逐轮留痕。
    pub fn retest_j1(&mut self, item: usize) -> Result<(), RetestCode> {
        if self.state != ProtoState::Idle {
            return Err(RetestCode::StateRegress);
        }
        let mut r = 1u32;
        while r <= RETEST_ROUNDS {
            self.ledger.record(item, Track::J1, r, true)?;
            r += 1;
        }
        self.state = ProtoState::J1Done;
        Ok(())
    }

    /// 复测 J2 口径：J1Done 态专用，10 轮逐轮留痕。
    /// 未先跑 J1 即跑 J2 = 口径顺序颠倒，显性拒绝。
    pub fn retest_j2(&mut self, item: usize) -> Result<(), RetestCode> {
        if self.state != ProtoState::J1Done {
            return Err(RetestCode::TrackOrderBroken);
        }
        let mut r = 1u32;
        while r <= RETEST_ROUNDS {
            self.ledger.record(item, Track::J2, r, true)?;
            r += 1;
        }
        self.state = ProtoState::J2Done;
        Ok(())
    }

    /// 认证：J2Done 态专用，双口径各 10 轮齐才发证。
    pub fn certify(&mut self) -> Result<(), RetestCode> {
        if self.state != ProtoState::J2Done {
            return Err(RetestCode::CertMissing);
        }
        if self.ledger.track_rounds(Track::J1) != RETEST_ROUNDS
            || self.ledger.track_rounds(Track::J2) != RETEST_ROUNDS
        {
            return Err(RetestCode::OneSided);
        }
        self.state = ProtoState::Certified;
        Ok(())
    }

    /// 出报告：认证后才可出；未认证即拒。
    pub fn report(&self) -> Result<u32, RetestCode> {
        if self.state != ProtoState::Certified {
            return Err(RetestCode::CertMissing);
        }
        Ok(self.ledger.rows.len() as u32)
    }
}

// ── 双口径留痕账 ──────────────────────────────────────────────────────
/// 一轮留痕：条目号 + 口径 + 轮号 + 是否通过。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TrackRow {
    /// 条目号（0..6）
    pub item: usize,
    /// 口径
    pub track: Track,
    /// 轮号（1..=10）
    pub round: u32,
    /// 本轮是否通过
    pub passed: bool,
}

/// 双口径留痕账：逐轮留痕、账守恒、口径轮数可查。
#[derive(Clone, Debug)]
pub struct TrackLedger {
    /// 留痕行
    pub rows: Vec<TrackRow>,
    /// 登记的应测轮数
    pub registered: u32,
    /// 逃逸数（必须为 0）
    pub escapes: u32,
}

impl TrackLedger {
    /// 构造零账。
    pub fn new() -> TrackLedger {
        TrackLedger { rows: Vec::new(), registered: 0, escapes: 0 }
    }

    /// 记一轮留痕：轮号必检、重复必拒。
    pub fn record(&mut self, item: usize, track: Track, round: u32, passed: bool) -> Result<(), RetestCode> {
        if round == 0 || round > RETEST_ROUNDS {
            return Err(RetestCode::RoundRange);
        }
        if self.rows.iter().any(|r| r.item == item && r.track == track && r.round == round) {
            return Err(RetestCode::DuplicateRow);
        }
        self.rows.push(TrackRow { item, track, round, passed });
        Ok(())
    }

    /// 登记应测轮数：0 或超 500 拒绝。
    pub fn set_registered(&mut self, n: u32) -> Result<(), RetestCode> {
        if n == 0 || n > 500 {
            return Err(RetestCode::RoundRange);
        }
        self.registered = n;
        Ok(())
    }

    /// 守恒式：登记轮数 = 留痕轮数 才零漂移。
    pub fn drift(&self) -> Option<u32> {
        let done = self.rows.len() as u32;
        if done == self.registered {
            None
        } else if done < self.registered {
            Some(self.registered - done)
        } else {
            Some(0)
        }
    }

    /// 单口径轮数。
    pub fn track_rounds(&self, track: Track) -> u32 {
        self.rows.iter().filter(|r| r.track == track).count() as u32
    }
}

// ── J2 红通道预核（联动 F10087） ──────────────────────────────────────
/// 反判据红入账样本：挂标记 + 红值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RedEntry {
    /// 必挂标记（"J2:" 或空）
    pub tag: [u8; 8],
    /// 标记字节数
    pub tag_len: usize,
    /// 红值（1=有红）
    pub red: u32,
}

/// 构造一条 J2 标记红条目。
pub fn red_j2() -> RedEntry {
    let mut e = RedEntry { tag: [0u8; 8], tag_len: 0, red: 1 };
    let t = J2_TAG.as_bytes();
    let mut i = 0usize;
    while i < t.len() {
        if let Some(slot) = e.tag.get_mut(i) {
            if let Some(v) = t.get(i) {
                *slot = *v;
            }
        }
        i += 1;
    }
    e.tag_len = t.len();
    e
}

/// 构造一条无标记红条目（混账样本）。
pub fn red_untagged() -> RedEntry {
    RedEntry { tag: [0u8; 8], tag_len: 0, red: 1 }
}

/// J2 红通道预核：红值必须挂 J2 标记才可入 F9747 红账。
/// 混账（无标记红 / 非红挂标记）逐次拦截。
pub fn j2_red_probe(e: &RedEntry) -> Result<(), RetestCode> {
    let t = J2_TAG.as_bytes();
    if e.red == 0 {
        // 非红条目带不带标记都不入红账，但挂标记属混账
        if e.tag_len == t.len() {
            let mut i = 0usize;
            let mut eq = true;
            while i < t.len() {
                match (e.tag.get(i), t.get(i)) {
                    (Some(a), Some(b)) => {
                        if a != b {
                            eq = false;
                        }
                    }
                    _ => eq = false,
                }
                i += 1;
            }
            if eq {
                return Err(RetestCode::MixedLedger);
            }
        }
        return Err(RetestCode::MixedLedger);
    }
    if e.tag_len != t.len() {
        return Err(RetestCode::MixedLedger);
    }
    let mut i = 0usize;
    while i < t.len() {
        match (e.tag.get(i), t.get(i)) {
            (Some(a), Some(b)) => {
                if a != b {
                    return Err(RetestCode::MixedLedger);
                }
            }
            _ => return Err(RetestCode::MixedLedger),
        }
        i += 1;
    }
    Ok(())
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏复测样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRetestCase {
    /// 口径顺序颠倒（未跑 J1 先跑 J2）
    TrackOrderBroken,
    /// 单口径复测（只跑 J1 即认证）
    OneSided,
    /// 轮数越界（0 / 11 轮）
    RoundRange,
    /// 重复留痕
    DuplicateRow,
    /// 混账（无标记红入 J2 通道）
    MixedLedger,
}

impl BadRetestCase {
    /// 闭集全量。
    pub const ALL: [BadRetestCase; 5] = [
        BadRetestCase::TrackOrderBroken,
        BadRetestCase::OneSided,
        BadRetestCase::RoundRange,
        BadRetestCase::DuplicateRow,
        BadRetestCase::MixedLedger,
    ];

    /// 注入一轮：构造坏样本并触发校验，返回是否被显式拦截。
    pub fn inject(self) -> bool {
        match self {
            BadRetestCase::TrackOrderBroken => {
                let mut p = RetestProtocol::new();
                p.retest_j2(0) == Err(RetestCode::TrackOrderBroken)
            }
            BadRetestCase::OneSided => {
                let mut p = RetestProtocol::new();
                if p.retest_j1(0).is_err() {
                    return false;
                }
                p.certify() == Err(RetestCode::CertMissing)
            }
            BadRetestCase::RoundRange => {
                let mut l = TrackLedger::new();
                let a = l.record(0, Track::J1, 0, true).err() == Some(RetestCode::RoundRange);
                let b = l.record(0, Track::J1, 11, true).err() == Some(RetestCode::RoundRange);
                a && b
            }
            BadRetestCase::DuplicateRow => {
                let mut l = TrackLedger::new();
                let a = l.record(0, Track::J1, 1, true).is_ok();
                let b = l.record(0, Track::J1, 1, true).err() == Some(RetestCode::DuplicateRow);
                a && b
            }
            BadRetestCase::MixedLedger => {
                j2_red_probe(&red_untagged()).err() == Some(RetestCode::MixedLedger)
                    && j2_red_probe(&red_j2()).is_ok()
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类拦截数。
pub fn run_retest_injection() -> (TrackLedger, [u32; 5]) {
    let mut ledger = TrackLedger::new();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadRetestCase::ALL.len() {
        if let Some(case) = BadRetestCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = case.inject();
                if !caught {
                    ledger.escapes = ledger.escapes.saturating_add(1);
                }
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EALREADY", -114, 10037, "Windows WSAEALREADY；重复留痕/重复登记族"),
    ("EBUSY", -16, 170, "Windows ERROR_BUSY；口径顺序颠倒族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；轮数越界族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺口径档位族"),
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；未认证出报告族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料双跑指纹：登记→J1→J2→认证→出报告→J2 红预核全链入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut reg = DualTrackRegistry::new();
    let _ = registry_standard(&mut reg);
    bytes.push(match reg.slots.get(0) {
        Some(s) if s.j1 && s.j2 => 1,
        _ => 2,
    });
    let mut p = RetestProtocol::new();
    bytes.push(match p.retest_j1(0) {
        Ok(()) => 3,
        Err(_) => 4,
    });
    bytes.push(match p.retest_j2(0) {
        Ok(()) => 5,
        Err(_) => 6,
    });
    bytes.push(match p.certify() {
        Ok(()) => 7,
        Err(_) => 8,
    });
    bytes.push(match p.report() {
        Ok(n) => (n as u8).wrapping_add(100),
        Err(_) => 9,
    });
    bytes.push(match j2_red_probe(&red_j2()) {
        Ok(()) => 10,
        Err(_) => 11,
    });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10086 判据集（17 条静态 + 3 条动态明细）。
pub fn run_unx10086_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10086");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (SEG_BASE..SEG_END).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x92540..=0x92580", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝");

    // 2. 标准登记：6 条条目双口径齐挂
    let mut reg = DualTrackRegistry::new();
    let std_ok = registry_standard(&mut reg).is_ok();
    let mut dual_cnt = 0usize;
    let mut i = 0usize;
    while i < DUAL_SLOTS {
        if reg.slot_dual(i) {
            dual_cnt += 1;
        }
        i += 1;
    }
    s.add("口径登记：6 条条目 J1/J2 双口径齐挂", std_ok && dual_cnt == DUAL_SLOTS,
          "双判据条目必须 J1/J2 双口径齐挂才可复测");

    // 3. 缺口径档位：只挂 J1 的条目不齐挂
    let mut half = DualTrackRegistry::new();
    let _ = half.attach(0, Track::J1);
    let half_rej = !half.slot_dual(0);
    s.add("缺口径：只挂 J1 即非双口径", half_rej,
          "缺另一口径档位的条目不得进入复测协议");

    // 4. 协议机：J1 先 J2 后 认证 出报告 全链推进
    let mut p = RetestProtocol::new();
    let j1_ok = p.retest_j1(0).is_ok() && p.state == ProtoState::J1Done;
    let j2_ok = p.retest_j2(0).is_ok() && p.state == ProtoState::J2Done;
    let cert_ok = p.certify().is_ok() && p.state == ProtoState::Certified;
    let rep_ok = p.report() == Ok(20);
    s.add("协议链：J1→J2→认证→出报告逐段推进", j1_ok && j2_ok && cert_ok && rep_ok,
          "双口径各 10 轮 = 20 行留痕，认证后报告可出");

    // 5. 口径顺序颠倒 10/10 拦截
    let mut order_hits = 0u32;
    let mut round = 0u32;
    while round < INJECT_ROUNDS {
        let mut pp = RetestProtocol::new();
        if pp.retest_j2(0) == Err(RetestCode::TrackOrderBroken) {
            order_hits += 1;
        }
        round += 1;
    }
    s.add("顺序颠倒：未跑 J1 先跑 J2 10/10 拦截", order_hits == INJECT_ROUNDS,
          "J1 先 J2 后是协议硬序，颠倒逐次显性拒绝");

    // 6. 单口径复测 10/10 拦截（锚点判据的单口径面）
    let mut side_hits = 0u32;
    let mut round2 = 0u32;
    while round2 < INJECT_ROUNDS {
        let mut pp = RetestProtocol::new();
        let _ = pp.retest_j1(0);
        if pp.certify() == Err(RetestCode::CertMissing) {
            side_hits += 1;
        }
        round2 += 1;
    }
    s.add("单口径复测：只跑 J1 即认证 10/10 拦截", side_hits == INJECT_ROUNDS,
          "缺 J2 口径 10 轮不得认证，逐次显性拒绝");

    // 7. 双口径留痕：J1/J2 各 10 轮账守恒
    let dual_ok = p.ledger.track_rounds(Track::J1) == RETEST_ROUNDS
        && p.ledger.track_rounds(Track::J2) == RETEST_ROUNDS
        && p.ledger.rows.len() == 20;
    s.add("双口径留痕：J1/J2 各 10 轮合计 20 行", dual_ok,
          "登记=留痕=20，账实逐轮对齐");

    // 8. 轮数越界与重复留痕拦截
    let mut l = TrackLedger::new();
    let rng_ok = l.record(0, Track::J1, 0, true).err() == Some(RetestCode::RoundRange)
        && l.record(0, Track::J1, 11, true).err() == Some(RetestCode::RoundRange);
    let _ = l.record(0, Track::J1, 1, true);
    let dup_ok = l.record(0, Track::J1, 1, true).err() == Some(RetestCode::DuplicateRow);
    s.add("轮窗/查重：0 轮、11 轮、重复轮号全拒", rng_ok && dup_ok,
          "轮窗闭集 1..=10，同条目同口径同轮号唯一");

    // 9. 重复登记口径拦截
    let mut dreg = DualTrackRegistry::new();
    let _ = dreg.attach(0, Track::J1);
    let dreg_ok = dreg.attach(0, Track::J1).err() == Some(RetestCode::DuplicateSlot)
        && dreg.attach(DUAL_SLOTS, Track::J1).err() == Some(RetestCode::SlotRange);
    s.add("口径登记闸：同口径重复挂/越界挂全拒", dreg_ok,
          "一条目一口径一档，越界条目号显性拒绝");

    // 10. J2 红通道预核：挂 J2 标记的红入账（锚点判据）
    let mut tag_hits = 0u32;
    let mut round3 = 0u32;
    while round3 < INJECT_ROUNDS {
        if j2_red_probe(&red_j2()).is_ok() {
            tag_hits += 1;
        }
        round3 += 1;
    }
    s.add("J2 红通道：挂 J2 标记的红 10/10 次入账", tag_hits == INJECT_ROUNDS,
          "反判据红挂 J2 标记后逐次放行，联动 F10087 红账");

    // 11. 混账 10/10 拦截（锚点判据）
    let mut mix_hits = 0u32;
    let mut round4 = 0u32;
    while round4 < INJECT_ROUNDS {
        if j2_red_probe(&red_untagged()).err() == Some(RetestCode::MixedLedger) {
            mix_hits += 1;
        }
        round4 += 1;
    }
    s.add("混账拦截：无标记红 10/10 次拒收", mix_hits == INJECT_ROUNDS,
          "J2 红必须挂 J2 标记，无标记红逐次显性拦截");

    // 12. 未认证出报告拦截
    let up = RetestProtocol::new();
    let early_rej = up.report() == Err(RetestCode::CertMissing);
    s.add("认证闸：未认证出报告即拒", early_rej,
          "报告必须在双口径认证后出，跳步显性拒绝");

    // 13. 注入核：五类坏样本全检出
    let (ledger, per_class) = run_retest_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 检出", all_caught,
          "顺序颠倒/单口径/轮数越界/重复留痕/混账全被显式拦截");

    // 14. 账守恒逃逸=0
    s.add("账守恒：注入过程逃逸=0",
          ledger.escapes == 0 && ledger.rows.is_empty(),
          "注入核全程不得产生留痕逃逸");

    // 15. 文案闭集：非空
    let text_ok = RetestCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一 + 落段
    let mut uniq = true;
    let mut i2 = 0usize;
    while i2 < RetestCode::ALL.len() {
        let mut j = i2 + 1;
        while j < RetestCode::ALL.len() {
            if let (Some(x), Some(y)) = (RetestCode::ALL.get(i2), RetestCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i2 += 1;
    }
    let in_seg = RetestCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 个码值两两唯一且落段窗", uniq && in_seg,
          "码值闭集唯一且不得越过 +0x0C 窗口");

    // 17. Linux 逐码一致
    let codes_ok = agrees_with_linux(EALREADY)
        && agrees_with_linux(EBUSY)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EPERM)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 18. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字节相等，隐藏状态即失败");

    // 动态明细（≤8）：口径档位快照 + 留痕快照 + 指纹快照
    let reg_text = format!("双口径齐挂 {}/6 条", dual_cnt);
    s.add_dyn("口径快照", dual_cnt == DUAL_SLOTS, &reg_text);
    let row_text = format!("留痕 {}/20 行（J1×10+J2×10）", p.ledger.rows.len());
    s.add_dyn("留痕快照", p.ledger.rows.len() == 20, &row_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_dual_attach() {
        let mut reg = DualTrackRegistry::new();
        assert!(registry_standard(&mut reg).is_ok());
        let mut n = 0usize;
        let mut i = 0usize;
        while i < DUAL_SLOTS {
            if reg.slot_dual(i) {
                n += 1;
            }
            i += 1;
        }
        assert_eq!(n, DUAL_SLOTS);
    }

    #[test]
    fn attach_duplicate_and_range() {
        let mut reg = DualTrackRegistry::new();
        let _ = reg.attach(0, Track::J1);
        assert_eq!(reg.attach(0, Track::J1), Err(RetestCode::DuplicateSlot));
        assert_eq!(reg.attach(DUAL_SLOTS, Track::J1), Err(RetestCode::SlotRange));
    }

    #[test]
    fn protocol_order() {
        let mut p = RetestProtocol::new();
        assert_eq!(p.retest_j2(0), Err(RetestCode::TrackOrderBroken));
        assert!(p.retest_j1(0).is_ok());
        assert!(p.retest_j2(0).is_ok());
        assert!(p.certify().is_ok());
        assert_eq!(p.report(), Ok(20));
        assert_eq!(p.retest_j1(0), Err(RetestCode::StateRegress));
    }

    #[test]
    fn certify_requires_both() {
        let mut p = RetestProtocol::new();
        let _ = p.retest_j1(0);
        assert_eq!(p.certify(), Err(RetestCode::CertMissing));
        assert_eq!(p.report(), Err(RetestCode::CertMissing));
    }

    #[test]
    fn round_window_closed() {
        let mut l = TrackLedger::new();
        assert_eq!(l.record(0, Track::J1, 0, true), Err(RetestCode::RoundRange));
        assert_eq!(l.record(0, Track::J1, 11, true), Err(RetestCode::RoundRange));
    }

    #[test]
    fn duplicate_row_rejected() {
        let mut l = TrackLedger::new();
        let _ = l.record(0, Track::J1, 1, true);
        assert_eq!(l.record(0, Track::J1, 1, true), Err(RetestCode::DuplicateRow));
    }

    #[test]
    fn ledger_conservation() {
        let mut l = TrackLedger::new();
        let _ = l.set_registered(20);
        let mut r = 1u32;
        while r <= RETEST_ROUNDS {
            let _ = l.record(0, Track::J1, r, true);
            let _ = l.record(0, Track::J2, r, true);
            r += 1;
        }
        assert!(l.drift().is_none());
        assert_eq!(l.track_rounds(Track::J1), 10);
        assert_eq!(l.track_rounds(Track::J2), 10);
    }

    #[test]
    fn j2_red_channel() {
        assert!(j2_red_probe(&red_j2()).is_ok());
        assert_eq!(j2_red_probe(&red_untagged()), Err(RetestCode::MixedLedger));
    }

    #[test]
    fn injection_matrix_zero_escape() {
        let (l, per) = run_retest_injection();
        assert_eq!(l.escapes, 0);
        assert!(l.rows.is_empty());
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EALREADY));
        assert!(agrees_with_linux(EBUSY));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(EINVAL), -EINVAL);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10086_checks_green() {
        let s = run_unx10086_checks();
        assert!(s.all_passed(), "UNX-F10086 red: {}", s.red_items_text());
    }
}
