//! UNX-F10029 豁免率实时计算（≤5% 哨兵） · 0x91740段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10030-J1 版本迁移零丢失（逐条态迁移核对 10/10 次），条目丢失 10/10 次拦截
//!
//! 设计要点：
//!   ① 豁免率用千分比整数计算（rate = exempt×1000/total），禁浮点；
//!      total=0 时按 0‰ 处理（除零安全，不 panic）；哨兵阈 50‰
//!      （=5%），30‰ 以下 Green、31..=50‰ Armed、越 50‰ Tripped。
//!   ② 即时刷新语义：任何账面变动（新增豁免/清偿恢复/版本迁移）都
//!      立即推 version 并重算 rate；用旧 version 查快照即 StaleSnapshot，
//!      滞后刷新 10/10 次显性检出，不许读到旧占比。
//!   ③ 超阈拦截：新增豁免若把占比推过 50‰ 直接拒绝且账不动
//!      （拒绝不留痕）；恰好 50‰ 是合法边界（≤5% 含等号）。
//!   ④ 版本迁移零丢失：迁移前后 total/exempt 逐字段核对，不一致即
//!      MigrationLoss 10/10 次拦截；迁移本身也推 version。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸；四类坏样本（超阈注入/滞后
//!      快照/迁移丢失/账越界）各 10/10 次显性拒绝；FNV-1a 双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10029 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91740;

/// 哨兵阈值：50‰（=5%，含等号为合法边界）。
pub const TRIP_PERMILLE: u32 = 50;

/// Green 上限：30‰ 以下不预警。
pub const GREEN_MAX_PERMILLE: u32 = 30;

/// 千分比放大系数。
pub const PERMILLE: u32 = 1000;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RateCode {
    /// 超阈：新增豁免把占比推过 50‰
    ThresholdBreach,
    /// 滞后快照：version 已推进而快照用旧号
    StaleSnapshot,
    /// 迁移丢失：迁移前后账面不一致
    MigrationLoss,
    /// 哨兵 Tripped 时新增豁免
    SentinelTripped,
    /// 账越界：豁免数越过总条目数
    BadCounts,
    /// 迁移空账
    EmptyMigration,
    /// 重复清偿：条目非豁免态被清
    DupClear,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 放大溢出守卫
    OverflowGuard,
    /// 账面回退
    NegativeGuard,
    /// 段位越界
    SegmentViolation,
}

impl RateCode {
    /// 码值（段基址 + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RateCode::ThresholdBreach => "超阈被拒：新增豁免将把占比推过 50‰（5%）；\
                请先清偿存量豁免或走特批流程",
            RateCode::StaleSnapshot => "滞后快照被拒：version 已推进而快照仍持旧号；\
                请重新刷新快照再读占比",
            RateCode::MigrationLoss => "迁移丢失被拒：迁移前后 total/exempt 逐字段不一致；\
                请按逐条态迁移核对后再提交",
            RateCode::SentinelTripped => "哨兵 Tripped 被拒：占比越阈期间冻结新增豁免；\
                请先降占比回 Green/Armed",
            RateCode::BadCounts => "账越界被拒：豁免数不得越过总条目数；\
                请核对账面口径",
            RateCode::EmptyMigration => "迁移空账被拒：零条目账不参与版本迁移；\
                请先入账再迁移",
            RateCode::DupClear => "重复清偿被拒：目标条目已非豁免态；\
                请核对清偿台账避免双清",
            RateCode::LedgerRegress => "注入账倒退被拒：计数只增不减；请排查回滚路径",
            RateCode::NotReproducible => "双跑不一致：同账面两次计算占比不同；\
                请排查隐藏状态或非确定顺序",
            RateCode::OverflowGuard => "放大溢出被拒：exempt×1000 越出安全范围；\
                请分段计算或扩位",
            RateCode::NegativeGuard => "账面回退被拒：total/exempt 不许变负；\
                请检查是否走了未记账的删除路径",
            RateCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量。
    pub const ALL: [RateCode; 12] = [
        RateCode::ThresholdBreach,
        RateCode::StaleSnapshot,
        RateCode::MigrationLoss,
        RateCode::SentinelTripped,
        RateCode::BadCounts,
        RateCode::EmptyMigration,
        RateCode::DupClear,
        RateCode::LedgerRegress,
        RateCode::NotReproducible,
        RateCode::OverflowGuard,
        RateCode::NegativeGuard,
        RateCode::SegmentViolation,
    ];
}

// ── 哨兵状态机 ────────────────────────────────────────────────────────
/// 哨兵三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SentinelState {
    /// ≤30‰
    Green,
    /// 31..=50‰
    Armed,
    /// >50‰（仅迁移带入，输入侧被超阈闸拦死）
    Tripped,
}

/// 由千分比导出哨兵态（纯函数）。
pub fn state_of(permille: u32) -> SentinelState {
    if permille <= GREEN_MAX_PERMILLE {
        SentinelState::Green
    } else if permille <= TRIP_PERMILLE {
        SentinelState::Armed
    } else {
        SentinelState::Tripped
    }
}

// ── 速率账本（账本 + 即时刷新） ──────────────────────────────────────
/// 占比账本：total/exempt/version 三账。
#[derive(Clone, Copy, Debug)]
pub struct RateLedger {
    total: u32,
    exempt: u32,
    version: u32,
    /// 清偿恢复累计数（只增）
    recoveries: u32,
}

impl RateLedger {
    /// 空账。
    pub const fn new() -> RateLedger {
        RateLedger { total: 0, exempt: 0, version: 1, recoveries: 0 }
    }

    /// 入账 n 条（豁免 0 条）：只推 total。
    pub fn add_entries(&mut self, n: u32) -> Result<(), RateCode> {
        if n == 0 {
            return Ok(());
        }
        self.total = match self.total.checked_add(n) {
            Some(t) => t,
            None => return Err(RateCode::OverflowGuard),
        };
        self.version = self.version.wrapping_add(1);
        Ok(())
    }

    /// 千分比占比：total=0 按 0‰（除零安全）。
    pub fn permille(&self) -> u32 {
        if self.total == 0 {
            return 0;
        }
        let scaled = match (self.exempt as u64).checked_mul(PERMILLE as u64) {
            Some(v) => v,
            None => return u32::MAX,
        };
        (scaled / self.total as u64) as u32
    }

    /// 哨兵态。
    pub fn state(&self) -> SentinelState {
        state_of(self.permille())
    }

    /// 新增豁免：账越界/超阈/Tripped 三道闸，全部拒绝不留痕；
    /// 成功即 version+1（即时刷新），返回新占比。
    pub fn add_exemption(&mut self, n: u32) -> Result<u32, RateCode> {
        if self.state() == SentinelState::Tripped {
            return Err(RateCode::SentinelTripped);
        }
        let next_exempt = match self.exempt.checked_add(n) {
            Some(v) => v,
            None => return Err(RateCode::OverflowGuard),
        };
        if next_exempt > self.total {
            return Err(RateCode::BadCounts);
        }
        let scaled = match (next_exempt as u64).checked_mul(PERMILLE as u64) {
            Some(v) => v,
            None => return Err(RateCode::OverflowGuard),
        };
        let next_rate = (scaled / self.total.max(1) as u64) as u32;
        if self.total > 0 && next_rate > TRIP_PERMILLE {
            return Err(RateCode::ThresholdBreach);
        }
        self.exempt = next_exempt;
        self.version = self.version.wrapping_add(1);
        Ok(self.permille())
    }

    /// 清偿恢复：把一条豁免清掉（exempt-1），version+1 即时刷新。
    pub fn clear_exemption(&mut self) -> Result<u32, RateCode> {
        if self.exempt == 0 {
            return Err(RateCode::DupClear);
        }
        self.exempt = self.exempt.saturating_sub(1);
        self.recoveries = self.recoveries.saturating_add(1);
        self.version = self.version.wrapping_add(1);
        Ok(self.permille())
    }

    /// 快照查询：version 必须是当前号，旧号即滞后（StaleSnapshot）。
    pub fn snapshot(&self, version: u32) -> Result<(u32, SentinelState), RateCode> {
        if version != self.version {
            return Err(RateCode::StaleSnapshot);
        }
        Ok((self.permille(), self.state()))
    }

    /// 版本迁移：逐字段核对 total/exempt，不一致即 MigrationLoss；
    /// 一致则 version+1（迁移即刷新）。
    pub fn migrate(&mut self, target_total: u32, target_exempt: u32) -> Result<u32, RateCode> {
        if self.total == 0 {
            return Err(RateCode::EmptyMigration);
        }
        if target_exempt > target_total {
            return Err(RateCode::MigrationLoss);
        }
        if target_total != self.total || target_exempt != self.exempt {
            return Err(RateCode::MigrationLoss);
        }
        self.total = target_total;
        self.exempt = target_exempt;
        self.version = self.version.wrapping_add(1);
        Ok(self.permille())
    }

    /// 账守恒：0 ≤ exempt ≤ total。
    pub fn conserved(&self) -> bool {
        self.exempt <= self.total
    }

    /// 只读视图。
    pub fn counts(&self) -> (u32, u32, u32) {
        (self.total, self.exempt, self.version)
    }

    /// 累计清偿恢复数。
    pub fn recoveries(&self) -> u32 {
        self.recoveries
    }
}

// ── 注入核：坏占比样本矩阵 ────────────────────────────────────────────
/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRateCase {
    /// 超阈注入：再加一条就越 50‰
    Breach,
    /// 滞后快照：旧 version 查询
    Stale,
    /// 迁移丢失：目标账面缺条目
    MigrationLoss,
    /// 账越界：豁免数越过总数
    OverCount,
}

impl BadRateCase {
    /// 闭集全量。
    pub const ALL: [BadRateCase; 4] = [
        BadRateCase::Breach,
        BadRateCase::Stale,
        BadRateCase::MigrationLoss,
        BadRateCase::OverCount,
    ];

    /// 期望诊断码。
    pub fn code(self) -> RateCode {
        match self {
            BadRateCase::Breach => RateCode::ThresholdBreach,
            BadRateCase::Stale => RateCode::StaleSnapshot,
            BadRateCase::MigrationLoss => RateCode::MigrationLoss,
            BadRateCase::OverCount => RateCode::BadCounts,
        }
    }

    /// 施加坏样本，返回是否按期望拒绝。
    pub fn attempt(self) -> bool {
        let mut l = RateLedger::new();
        let _ = l.add_entries(100);
        // 4 条豁免 → 40‰（Armed），距阈值一步
        let _ = l.add_exemption(4);
        match self {
            BadRateCase::Breach => {
                // 50‰ 合法，60‰ 必拒
                let at_edge = l.add_exemption(1) == Ok(TRIP_PERMILLE);
                let over = l.add_exemption(1).err() == Some(RateCode::ThresholdBreach);
                at_edge && over
            }
            BadRateCase::Stale => {
                let (_, _, v) = l.counts();
                let stale_v = v.wrapping_sub(1);
                l.snapshot(stale_v).err() == Some(RateCode::StaleSnapshot)
                    && l.snapshot(v).is_ok()
            }
            BadRateCase::MigrationLoss => {
                // 目标账面丢 1 条 total
                l.migrate(99, 4).err() == Some(RateCode::MigrationLoss)
            }
            BadRateCase::OverCount => {
                l.add_exemption(97).err() == Some(RateCode::BadCounts)
            }
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RateLedgerStat {
    /// 注入总次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl RateLedgerStat {
    const fn zero() -> RateLedgerStat {
        RateLedgerStat { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) -> Result<(), RateCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(RateCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏占比注入：四类各 INJECT_ROUNDS 次。
pub fn run_bad_rate_matrix() -> (RateLedgerStat, [u32; 4]) {
    let mut ledger = RateLedgerStat::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadRateCase::ALL.len() {
        let case = match BadRateCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = case.attempt();
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per_class.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EPERM", -1, 5, "Windows ERROR_ACCESS_DENIED；超阈/Tripped 拦截族"),
    ("ESTALE", -116, 0x8007, "Windows ERROR_INVALID_SESSION；滞后快照族"),
    ("EIO", -5, 21, "Windows ERROR_NOT_READY；迁移丢失族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；账越界族"),
    ("EOVERFLOW", -75, 0x8007, "Windows ERROR_ARITHMETIC_OVERFLOW；放大溢出族"),
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

/// 规范语料双跑指纹：入账 200 → 豁免 8（40‰）→ 清 3 → 迁移核对 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut l = RateLedger::new();
    let _ = l.add_entries(200);
    let _ = l.add_exemption(8);
    let _ = l.clear_exemption();
    let _ = l.clear_exemption();
    let _ = l.clear_exemption();
    let _ = l.migrate(200, 5);
    let (total, exempt, version) = l.counts();
    let mut bytes: Vec<u8> = Vec::new();
    bytes.push((total & 0xFF) as u8);
    bytes.push((exempt & 0xFF) as u8);
    bytes.push((version & 0xFF) as u8);
    bytes.push((l.permille() & 0xFF) as u8);
    bytes.push((l.recoveries() & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10029 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10029_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10029");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91500..=0x91FFF).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91500..=0x91FFF", seg_ok,
          "段基址必须唯一且落在本批窗口");

    // 2. 千分比计算：200 总 10 豁免 → 50‰
    let mut calc = RateLedger::new();
    let _ = calc.add_entries(200);
    let _ = calc.add_exemption(10);
    s.add("占比计算：200 总 10 豁免 → 50‰", calc.permille() == TRIP_PERMILLE,
          "rate = exempt×1000/total，整数千分比不含浮点");

    // 3. 即时刷新：每笔变动 version 立即推进且占比同步
    let mut live = RateLedger::new();
    let _ = live.add_entries(100);
    let v0 = live.counts().2;
    let _ = live.add_exemption(4);
    let v1 = live.counts().2;
    let rate_now = live.permille();
    s.add("即时刷新：变动后 version+1 且占比即时为 40‰",
          v1 == v0 + 1 && rate_now == 40 && live.snapshot(v1).is_ok(),
          "清偿后占比即时刷新，不许滞后一拍");

    // 4. 哨兵状态机：Green→Armed→（清偿）→Green 单向可逆
    let st0 = live.state() == SentinelState::Armed;
    let _ = live.clear_exemption();
    let _ = live.clear_exemption();
    let recovered = live.state() == SentinelState::Green;
    s.add("哨兵状态机：40‰ Armed，清偿后回 Green", st0 && recovered,
          "状态由占比纯函数导出，清偿降占比即降级");

    // 5. 边界性质：恰好 50‰ 合法、51‰ 拒绝
    let mut edge = RateLedger::new();
    let _ = edge.add_entries(100);
    let _ = edge.add_exemption(5); // 50‰
    let at_edge = edge.permille() == TRIP_PERMILLE;
    let over = edge.add_exemption(1).err() == Some(RateCode::ThresholdBreach);
    s.add("边界性质：≤50‰ 合法、>50‰ 拒绝", at_edge && over,
          "阈值含等号：恰 5% 放行，越线拦截");

    // 6. 注入核：四类坏样本各 10/10 拒绝
    let (inj, per_class) = run_bad_rate_matrix();
    let all_caught = per_class.iter().all(|x| *x == INJECT_ROUNDS);
    s.add("注入核：超阈/滞后快照/迁移丢失/账越界各 10/10 拒绝", all_caught,
          "四类坏样本逐类逐次显性拒绝");

    // 7. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadRateCase::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 8. 滞后快照检出：旧 version 查询 10/10 拒绝
    let mut stale_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        let mut g = RateLedger::new();
        let _ = g.add_entries(50);
        let v = g.counts().2;
        let _ = g.add_exemption(1);
        if g.snapshot(v).is_ok() {
            stale_ok = false;
        }
        r += 1;
    }
    s.add("滞后快照检出：旧 version 查询 10/10 拒绝", stale_ok,
          "快照必须持当前 version，旧号一律 StaleSnapshot");

    // 9. 版本迁移零丢失：逐字段核对通过 + 丢条目拦截 10/10
    let mut mig = RateLedger::new();
    let _ = mig.add_entries(120);
    let _ = mig.add_exemption(6);
    let mig_ok = mig.migrate(120, 6).is_ok()
        && mig.counts() == (120, 6, mig.counts().2);
    let mut loss_ok = true;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        let mut g = RateLedger::new();
        let _ = g.add_entries(120);
        let _ = g.add_exemption(6);
        if g.migrate(119, 6).is_ok() || g.migrate(120, 5).is_ok() {
            loss_ok = false;
        }
        r2 += 1;
    }
    s.add("版本迁移零丢失：核对通过且丢条目 10/10 拦截", mig_ok && loss_ok,
          "迁移前后 total/exempt 逐字段核对，缺一即 MigrationLoss");

    // 10. 除零安全：空账占比 0‰ 且不出 panic
    let empty = RateLedger::new();
    s.add("除零安全：空账占比 0‰ 且哨兵 Green",
          empty.permille() == 0 && empty.state() == SentinelState::Green,
          "total=0 按 0‰ 处理，不许 panic 也不许 Tripped");

    // 11. 账守恒：全操作后 exempt ≤ total 恒成立
    s.add("账守恒：exempt ≤ total 恒成立", edge.conserved() && live.conserved(),
          "任何路径不得把豁免数推过总数");

    // 12. 清偿恢复留痕：recoveries 只增
    let rec_ok = live.recoveries() == 2;
    s.add("清偿恢复留痕：2 次清偿 recoveries=2", rec_ok,
          "清偿动作逐笔记账，只增不减");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(1) && agrees_with_linux(116) && agrees_with_linux(5)
        && agrees_with_linux(22) && agrees_with_linux(75)
        && !agrees_with_linux(0) && !agrees_with_linux(2401);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 14. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < RateCode::ALL.len() {
        let mut j = i + 1;
        while j < RateCode::ALL.len() {
            if let (Some(a), Some(b)) = (RateCode::ALL.get(i), RateCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = RateCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 15. 双跑指纹一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑指纹：规范语料两次哈希相等", f1 == f2,
          "同账面两次计算+迁移必须逐位一致");

    // 16. 状态导出纯函数：三段边界逐一复算
    let state_ok = state_of(0) == SentinelState::Green
        && state_of(GREEN_MAX_PERMILLE) == SentinelState::Green
        && state_of(GREEN_MAX_PERMILLE + 1) == SentinelState::Armed
        && state_of(TRIP_PERMILLE) == SentinelState::Armed
        && state_of(TRIP_PERMILLE + 1) == SentinelState::Tripped;
    s.add("状态导出：30/31/50/51‰ 边界逐一复算", state_ok,
          "哨兵态由占比纯函数导出，边界逐点核对");

    // 动态明细（≤8）：占比 + 注入账 + 指纹
    let rate_text = format!(
        "total {} / 豁免 {} / {}‰ / {:?}",
        live.counts().0, live.counts().1, live.permille(), live.state()
    );
    s.add_dyn("占比快照", live.state() == SentinelState::Green, &rate_text);
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permille_math() {
        let mut l = RateLedger::new();
        let _ = l.add_entries(200);
        let _ = l.add_exemption(10);
        assert_eq!(l.permille(), 50);
    }

    #[test]
    fn empty_book_zero_rate() {
        let l = RateLedger::new();
        assert_eq!(l.permille(), 0);
        assert_eq!(l.state(), SentinelState::Green);
    }

    #[test]
    fn threshold_boundary() {
        let mut l = RateLedger::new();
        let _ = l.add_entries(100);
        assert_eq!(l.add_exemption(5), Ok(50));
        assert_eq!(l.add_exemption(1).err(), Some(RateCode::ThresholdBreach));
    }

    #[test]
    fn stale_snapshot_detected() {
        let mut l = RateLedger::new();
        let _ = l.add_entries(50);
        let v = l.counts().2;
        let _ = l.add_exemption(1);
        assert_eq!(l.snapshot(v).err(), Some(RateCode::StaleSnapshot));
        assert!(l.snapshot(l.counts().2).is_ok());
    }

    #[test]
    fn migration_zero_loss() {
        let mut l = RateLedger::new();
        let _ = l.add_entries(120);
        let _ = l.add_exemption(6);
        assert!(l.migrate(120, 6).is_ok());
        assert_eq!(l.migrate(119, 6).err(), Some(RateCode::MigrationLoss));
    }

    #[test]
    fn over_count_rejected() {
        let mut l = RateLedger::new();
        let _ = l.add_entries(10);
        assert_eq!(l.add_exemption(11).err(), Some(RateCode::BadCounts));
    }

    #[test]
    fn clear_recovers_state() {
        let mut l = RateLedger::new();
        let _ = l.add_entries(100);
        let _ = l.add_exemption(5);
        assert_eq!(l.state(), SentinelState::Armed);
        // 5→2 条豁免 = 20‰，回 Green；单清一笔只降到 40‰ 仍 Armed
        let _ = l.clear_exemption();
        assert_eq!(l.state(), SentinelState::Armed);
        let _ = l.clear_exemption();
        let _ = l.clear_exemption();
        assert_eq!(l.state(), SentinelState::Green);
        assert_eq!(l.recoveries(), 3);
    }

    #[test]
    fn zero_clear_rejected() {
        let mut l = RateLedger::new();
        let _ = l.add_entries(10);
        assert_eq!(l.clear_exemption().err(), Some(RateCode::DupClear));
    }

    #[test]
    fn state_boundaries() {
        assert_eq!(state_of(30), SentinelState::Green);
        assert_eq!(state_of(31), SentinelState::Armed);
        assert_eq!(state_of(50), SentinelState::Armed);
        assert_eq!(state_of(51), SentinelState::Tripped);
    }

    #[test]
    fn bad_rate_matrix_green() {
        let (inj, per) = run_bad_rate_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(116));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(75), -75);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10029_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
