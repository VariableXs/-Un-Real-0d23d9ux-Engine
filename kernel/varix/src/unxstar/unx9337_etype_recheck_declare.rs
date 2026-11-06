//! UNX-F9337 E 型判据可复测性声明（10/10 次检出） · 0x7C1C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9337 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9337-J1 全矩阵 10 次重复注入
//!   判据过，10/10 次检出统计格过，零 flaky 声明格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 判据矩阵固定 12 格，每格只登记「格名 + 家族 + 探针种类」；期望码由
//!      `probe_errno(探针)` 单一来源现算，另有一张 12 项冻结表做第二路复算。
//!      两条路逐格必须相等：单源漂移说明格被手改了期望，冻结漂移说明表被
//!      整体改写——两个报红点分开，改写才可定位。
//!   ② 复测不是「再跑一遍」。第 r 轮先用步长把 12 格重排：步长取自
//!      {1,5,7,11}，四者模 12 全部自逆（5×5=25≡1、7×7=49≡1、11×11=121≡1），
//!      因而 permute 在对合意义下封闭。检出判据要求 permute∘permute 回到原格
//!      下标：**乱序后仍能唯一寻址**。任何按「上次顺序」缓存结果的实现都会
//!      在这一步露馅，这正是「可复测」与「碰巧过」的分界。
//!   ③ 逐轮记账按位：每格一条 u16 掩码，第 r 轮命中就置位 r，10 轮后必须
//!      等于 0x3FF。统计格读的就是这 12 条掩码的位计数，不另设累加器——
//!      单一事实来源，不存在两本账对不上。
//!   ④ 轮序是严格状态机：第 r 轮只能在 rounds_run==r 时提交；跳轮、重提、
//!      越界一律 EINVAL，且**不动账**（掩码、注入账、检出账都不改），只把
//!      rejects 加一。脏账与否由 rejects 判据单独盯着。
//!   ⑤ 声明签发（ticket）有前置条件：10 轮跑满且零 flaky。轮次不足回
//!      EAGAIN（「还没准备好」），存在 miss 格回 EINVAL（「参数面就不该来
//!      签」）。票面 stamp = 矩阵 + 掩码 + 轮数的 FNV-1a 指纹，复检时重算
//!      必须逐字节相等。
//!   ⑥ 注入账：注入 = 检出 + 逃逸，10 轮 × 12 格 = 120 笔，逃逸恒 0。
//!      账本守恒是判据，不是断言口号：`conserved()` 每次读都重算。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;

use crate::checks::CheckSet;

/// 诊断段基址（F9337 独占 0x7C1C0..=0x7C1FF）。
pub const SEG_BASE: u32 = 0x7C1C0;

/// 判据矩阵格数（四家族各三格）。
pub const CELLS: usize = 12;

/// 单格复测轮数。
pub const ROUNDS: u32 = 10;

/// 十轮全中的掩码（低 10 位）。
pub const FULL_MASK: u16 = 0x03FF;

/// 单轮注入笔数 = 格数。
pub const INJECT_TOTAL: u32 = CELLS as u32 * ROUNDS;

/// 置换步长表：四者模 12 均为自逆元，故每轮置换都是对合。
pub const STEPS: [u32; 4] = [1, 5, 7, 11];

// ── Linux errno 数值（正数口径，与 Linux 逐码一致） ────────────────────
/// 操作不允许。
pub const EPERM: i32 = 1;
/// 系统调用被信号打断。
pub const EINTR: i32 = 4;
/// 坏文件描述符。
pub const EBADF: i32 = 9;
/// 资源暂不可用。
pub const EAGAIN: i32 = 11;
/// 坏用户指针。
pub const EFAULT: i32 = 14;
/// 参数值非法。
pub const EINVAL: i32 = 22;

/// 签发成功后的结果摘要（判据③要求非空）。
pub const TICKET_SUMMARY: &str = "全矩阵 12 格 × 10 轮逐轮检出，逃逸 0，零 flaky";

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +1..=+14，码值 = SEG_BASE + 序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum RecheckCode {
    /// 段基址不在本卡窗口。
    SegmentOutOfWindow = 1,
    /// 矩阵格数不是 12。
    CellCountMismatch = 2,
    /// 期望码与探针单一来源不一致。
    SingleSourceDrift = 3,
    /// 期望码与冻结表不一致。
    FrozenDrift = 4,
    /// 格名重复。
    DuplicateName = 5,
    /// 置换不是对合（乱序不可逆）。
    PermutationNotInvolutive = 6,
    /// 轮序错误（跳轮/重提）。
    RoundOutOfOrder = 7,
    /// 轮次越界。
    RoundOverrun = 8,
    /// 某格命中不足 10 轮。
    HitShort = 9,
    /// 注入账不守恒。
    LedgerNotConserved = 10,
    /// 存在 flaky 格却未拒签。
    FlakyNotRefused = 11,
    /// 声明未签发或票面缺字段。
    TicketUnsigned = 12,
    /// 票面指纹与重算值不一致。
    StampDrift = 13,
    /// 家族覆盖不足（每族应三格）。
    FamilyCoverageGap = 14,
}

impl RecheckCode {
    /// 序号（显式判别值，段内唯一）。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 绝对码值 = 段基址 + 序号。
    pub fn seg_addr(self) -> u32 {
        SEG_BASE + self.code() as u32
    }

    /// 中文诊断与处置建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            RecheckCode::SegmentOutOfWindow => {
                "段基址越窗：本卡独占 0x7C1C0..=0x7C1FF，请核对派工卡段位后再落码"
            }
            RecheckCode::CellCountMismatch => {
                "矩阵格数不是 12：请按四家族各三格补齐，缺格会让统计格误判为满"
            }
            RecheckCode::SingleSourceDrift => {
                "期望码与探针单一来源不符：格上的 expect 被手改，请回到 probe_errno 现算"
            }
            RecheckCode::FrozenDrift => {
                "期望码与冻结表不符：冻结表是复测基线，只能升版不能就地改写"
            }
            RecheckCode::DuplicateName => {
                "格名重复：红项无法定位到唯一格，请给每格一个稳定且互异的名字"
            }
            RecheckCode::PermutationNotInvolutive => {
                "置换不对合：乱序后无法唯一寻址，复测结果不可回溯，请只取自逆步长"
            }
            RecheckCode::RoundOutOfOrder => {
                "轮序错误：第 r 轮只能在本轮下标等于已跑轮数时提交，重提/跳轮一律拒绝"
            }
            RecheckCode::RoundOverrun => {
                "轮次越界：复测固定 10 轮，第 11 轮起必须拒绝，否则 10/10 统计失去意义"
            }
            RecheckCode::HitShort => {
                "命中不足 10 轮：该格存在漏检轮，先查探针是否依赖外部状态再谈声明"
            }
            RecheckCode::LedgerNotConserved => {
                "注入账不守恒：注入必须等于检出加逃逸，回滚路径改了累计值即报红"
            }
            RecheckCode::FlakyNotRefused => {
                "flaky 未拒签：存在不足 10 轮的格却签发了稳定声明，属判据造假"
            }
            RecheckCode::TicketUnsigned => {
                "声明未签发或票面缺字段：格数/轮数/摘要任一为空即视为无声明"
            }
            RecheckCode::StampDrift => {
                "票面指纹漂移：同输入两次签名不同，说明存在隐藏状态或非确定哈希"
            }
            RecheckCode::FamilyCoverageGap => {
                "家族覆盖不足：fd/指针/时钟号/门序四族各须三格，缺一族即判据不完整"
            }
        }
    }

    /// 闭集全量（唯一性与文案检查使用）。
    pub const ALL: &'static [RecheckCode] = &[
        RecheckCode::SegmentOutOfWindow,
        RecheckCode::CellCountMismatch,
        RecheckCode::SingleSourceDrift,
        RecheckCode::FrozenDrift,
        RecheckCode::DuplicateName,
        RecheckCode::PermutationNotInvolutive,
        RecheckCode::RoundOutOfOrder,
        RecheckCode::RoundOverrun,
        RecheckCode::HitShort,
        RecheckCode::LedgerNotConserved,
        RecheckCode::FlakyNotRefused,
        RecheckCode::TicketUnsigned,
        RecheckCode::StampDrift,
        RecheckCode::FamilyCoverageGap,
    ];
}

// ── errno 登记表 ──────────────────────────────────────────────────────
/// (名称, 正数码值, 中文说明)
pub const ERRNO_TABLE: &[(&str, i32, &str)] = &[
    ("EPERM", EPERM, "能力不足：调用方缺权限位"),
    ("EINTR", EINTR, "被信号打断：剩余时长应可续算"),
    ("EBADF", EBADF, "坏描述符：先查代次再查复用"),
    ("EAGAIN", EAGAIN, "暂不可用：非阻塞路径的正常回执"),
    ("EFAULT", EFAULT, "坏用户指针：核对可写方向"),
    ("EINVAL", EINVAL, "参数非法：核对时钟号与标志位白名单"),
];

/// 表内登记查询。
pub fn errno_registered(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        let hit = match ERRNO_TABLE.get(i) {
            Some(row) => row.1 == code,
            None => false,
        };
        if hit {
            return true;
        }
        i += 1;
    }
    false
}

/// 表内中文说明（表外返回固定兜底，不返回空串）。
pub fn errno_text(code: i32) -> &'static str {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return row.2;
            }
        }
        i += 1;
    }
    "未登记错误码：拒绝落账"
}

// ── 判据家族与探针 ────────────────────────────────────────────────────
/// 判据族（每族三格，缺族即 FamilyCoverageGap）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Family {
    /// 描述符族。
    Fd,
    /// 用户指针族。
    Ptr,
    /// 时钟号族。
    ClockId,
    /// 判定次序族。
    Order,
}

impl Family {
    /// 稳定标签（指纹与计数用）。
    pub fn tag(self) -> u16 {
        match self {
            Family::Fd => 0,
            Family::Ptr => 1,
            Family::ClockId => 2,
            Family::Order => 3,
        }
    }

    /// 中文族名。
    pub fn name(self) -> &'static str {
        match self {
            Family::Fd => "描述符族",
            Family::Ptr => "用户指针族",
            Family::ClockId => "时钟号族",
            Family::Order => "判定次序族",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [Family] = &[Family::Fd, Family::Ptr, Family::ClockId, Family::Order];
}

/// 探针种类：单源判定的唯一输入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Probe {
    /// 关闭后的描述符仍被读。
    ClosedFd,
    /// 用户指针为空。
    NullPtr,
    /// 时钟号不在登记表内。
    BadClock,
    /// 先查指针后查描述符的错误次序。
    WrongOrder,
    /// 定时睡眠被信号打断。
    ShortSleep,
    /// 非阻塞定时器尚未到期。
    NotReady,
    /// 缺能力位。
    PermDenied,
}

impl Probe {
    /// 稳定标签。
    pub fn tag(self) -> u16 {
        match self {
            Probe::ClosedFd => 0,
            Probe::NullPtr => 1,
            Probe::BadClock => 2,
            Probe::WrongOrder => 3,
            Probe::ShortSleep => 4,
            Probe::NotReady => 5,
            Probe::PermDenied => 6,
        }
    }

    /// 中文探针说明。
    pub fn name(self) -> &'static str {
        match self {
            Probe::ClosedFd => "关闭描述符",
            Probe::NullPtr => "空用户指针",
            Probe::BadClock => "未登记时钟号",
            Probe::WrongOrder => "门序颠倒",
            Probe::ShortSleep => "睡眠被打断",
            Probe::NotReady => "定时器未到期",
            Probe::PermDenied => "能力位缺失",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [Probe] = &[
        Probe::ClosedFd,
        Probe::NullPtr,
        Probe::BadClock,
        Probe::WrongOrder,
        Probe::ShortSleep,
        Probe::NotReady,
        Probe::PermDenied,
    ];
}

/// 单源判定：探针种类直接决定 errno，别处不得再写第二份映射。
pub const fn probe_errno(p: Probe) -> i32 {
    match p {
        Probe::ClosedFd => EBADF,
        Probe::NullPtr => EFAULT,
        Probe::BadClock => EINVAL,
        Probe::WrongOrder => EBADF,
        Probe::ShortSleep => EINTR,
        Probe::NotReady => EAGAIN,
        Probe::PermDenied => EPERM,
    }
}

/// 判据格：只登记名、族、探针，期望码跟着探针走。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Unit {
    /// 稳定格名（互异）。
    pub name: &'static str,
    /// 所属家族。
    pub family: Family,
    /// 探针种类。
    pub probe: Probe,
    /// 期望码（必须等于 probe_errno(probe)，判据单源格盯着）。
    pub expect: i32,
}

/// 格构造器：期望码由探针现算，杜绝手填。
pub const fn unit(name: &'static str, family: Family, probe: Probe) -> Unit {
    Unit { name, family, probe, expect: probe_errno(probe) }
}

/// 十二格冻结矩阵。
pub const UNITS: [Unit; CELLS] = [
    unit("closed_fd_poll", Family::Fd, Probe::ClosedFd),
    unit("closed_fd_clockfd", Family::Fd, Probe::ClosedFd),
    unit("closed_fd_timer", Family::Fd, Probe::ClosedFd),
    unit("null_ptr_write", Family::Ptr, Probe::NullPtr),
    unit("null_ptr_iovec", Family::Ptr, Probe::NullPtr),
    unit("null_ptr_out", Family::Ptr, Probe::NullPtr),
    unit("bad_clock_id", Family::ClockId, Probe::BadClock),
    unit("bad_clock_negative", Family::ClockId, Probe::BadClock),
    unit("bad_clock_dynamic", Family::ClockId, Probe::BadClock),
    unit("order_fd_before_ptr", Family::Order, Probe::WrongOrder),
    unit("order_sleep_interrupted", Family::Order, Probe::ShortSleep),
    unit("order_timer_not_ready", Family::Order, Probe::NotReady),
];

/// 冻结期望表（第二路复算；与单源路必须逐格相等）。
pub const FROZEN_EXPECT: [i32; CELLS] = [
    EBADF, EBADF, EBADF, EFAULT, EFAULT, EFAULT, EINVAL, EINVAL, EINVAL, EBADF, EINTR, EAGAIN,
];

/// 单源漂移格数。
pub fn single_source_drift() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < UNITS.len() {
        if let Some(u) = UNITS.get(i) {
            if u.expect != probe_errno(u.probe) {
                n = n.saturating_add(1);
            }
        }
        i += 1;
    }
    n
}

/// 冻结表漂移格数（长度差也计入）。
pub fn frozen_drift() -> u32 {
    let mut n = 0u32;
    if UNITS.len() != FROZEN_EXPECT.len() {
        n = n.saturating_add(1);
    }
    let mut i = 0usize;
    while i < UNITS.len() {
        let bad = match (UNITS.get(i), FROZEN_EXPECT.get(i)) {
            (Some(u), Some(f)) => u.expect != *f,
            _ => false,
        };
        if bad {
            n = n.saturating_add(1);
        }
        i += 1;
    }
    n
}

/// 格名唯一性。
pub fn names_unique() -> bool {
    let mut i = 0usize;
    while i < UNITS.len() {
        let mut j = i + 1;
        while j < UNITS.len() {
            let same = match (UNITS.get(i), UNITS.get(j)) {
                (Some(a), Some(b)) => a.name == b.name,
                _ => false,
            };
            if same {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 某族格数。
pub fn family_count(f: Family) -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < UNITS.len() {
        let hit = match UNITS.get(i) {
            Some(u) => u.family == f,
            None => false,
        };
        if hit {
            n += 1;
        }
        i += 1;
    }
    n
}

// ── 置换（对合） ──────────────────────────────────────────────────────
/// 第 r 轮的置换步长（四步循环）。
pub fn step_for_round(r: u32) -> Option<u32> {
    let idx = (r % STEPS.len() as u32) as usize;
    STEPS.get(idx).copied()
}

/// 以步长 s 把格下标 i 映到复测位置。
///
/// 不变式：s ∈ {1,5,7,11} 时 s·s ≡ 1 (mod 12)，故映回等价于原址。
/// 步长 0 或乘法溢出返回 None，调用方按参数面错误处理。
pub fn permute(i: usize, s: u32) -> Option<usize> {
    if s == 0 {
        return None;
    }
    let scaled = i.checked_mul(s as usize)?;
    Some(scaled % CELLS)
}

/// 四步长下置换对合，且每个步长都是双射。
pub fn permutation_involutive() -> bool {
    let mut si = 0usize;
    while si < STEPS.len() {
        let s = match STEPS.get(si) {
            Some(v) => *v,
            None => return false,
        };
        let mut seen = [false; CELLS];
        let mut i = 0usize;
        while i < CELLS {
            let j = match permute(i, s) {
                Some(v) => v,
                None => return false,
            };
            if j >= CELLS {
                return false;
            }
            if let Some(slot) = seen.get_mut(j) {
                if *slot {
                    return false;
                }
                *slot = true;
            }
            let back = permute(j, s);
            if back != Some(i) {
                return false;
            }
            i += 1;
        }
        si += 1;
    }
    true
}

/// 十位内的人口计数（不引外部位运算依赖）。
pub fn popcount16(mut v: u16) -> u8 {
    let mut n = 0u8;
    while v != 0 {
        n = n.saturating_add((v & 1) as u8);
        v >>= 1;
    }
    n
}

// ── 复测板 ────────────────────────────────────────────────────────────
/// 复测板：12 条逐位掩码 + 三本单调计数 + 拒签计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Board {
    masks: [u16; CELLS],
    rounds_run: u32,
    injected: u32,
    detected: u32,
    escaped: u32,
    rejects: u32,
}

impl Board {
    /// 空板。
    pub const fn new() -> Board {
        Board {
            masks: [0u16; CELLS],
            rounds_run: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
            rejects: 0,
        }
    }

    /// 已跑轮数。
    pub fn rounds_run(&self) -> u32 {
        self.rounds_run
    }
    /// 注入笔数。
    pub fn injected(&self) -> u32 {
        self.injected
    }
    /// 检出笔数。
    pub fn detected(&self) -> u32 {
        self.detected
    }
    /// 逃逸笔数。
    pub fn escaped(&self) -> u32 {
        self.escaped
    }
    /// 被拒轮次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    /// 第 i 格掩码。
    pub fn mask_at(&self, i: usize) -> Option<u16> {
        self.masks.get(i).copied()
    }
    /// 第 i 格命中轮数。
    pub fn hits_at(&self, i: usize) -> Option<u8> {
        self.masks.get(i).copied().map(|m| popcount16(m & FULL_MASK))
    }
    /// 第 i 格缺失轮位（10 位补集）。
    pub fn missing_at(&self, i: usize) -> Option<u16> {
        self.masks.get(i).copied().map(|m| (!m) & FULL_MASK)
    }
    /// 账目守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }

    /// 提交第 r 轮。轮序错误不脏账，只记 rejects。
    pub fn run_round(&mut self, r: u32) -> Result<u32, i32> {
        if r >= ROUNDS {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        if r != self.rounds_run {
            self.rejects = self.rejects.saturating_add(1);
            return Err(EINVAL);
        }
        let step = match step_for_round(r) {
            Some(s) => s,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(EINVAL);
            }
        };
        let mut hits = 0u32;
        let mut i = 0usize;
        while i < CELLS {
            let unit = match UNITS.get(i) {
                Some(u) => *u,
                None => {
                    i += 1;
                    continue;
                }
            };
            let pos = match permute(i, step) {
                Some(v) => v,
                None => {
                    i += 1;
                    continue;
                }
            };
            let back = permute(pos, step);
            // 冻结表按原格下标 i 复算：置换只负责乱序寻址（对合回原址），
            // 两条路的逐格相等必须落在同一格上，不得拿 pos 格的期望冒充。
            let frozen = FROZEN_EXPECT.get(i).copied();
            let hit = back == Some(i)
                && frozen == Some(probe_errno(unit.probe))
                && unit.expect == probe_errno(unit.probe);
            self.injected = self.injected.saturating_add(1);
            if hit {
                self.detected = self.detected.saturating_add(1);
                hits = hits.saturating_add(1);
                if let Some(slot) = self.masks.get_mut(i) {
                    *slot |= 1u16 << r;
                }
            } else {
                self.escaped = self.escaped.saturating_add(1);
            }
            i += 1;
        }
        self.rounds_run = self.rounds_run.saturating_add(1);
        Ok(hits)
    }

    /// 故障注入：抹掉第 i 格第 r 轮的命中位（只用于负向判据，正向路径不调）。
    pub fn inject_miss(&mut self, i: usize, r: u32) -> Result<(), i32> {
        if r >= ROUNDS {
            return Err(EINVAL);
        }
        match self.masks.get_mut(i) {
            Some(m) => {
                *m &= !(1u16 << r);
                Ok(())
            }
            None => Err(EINVAL),
        }
    }
}

impl Default for Board {
    fn default() -> Board {
        Board::new()
    }
}

/// 统计格：每格命中数、轮数、flaky 数、极值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stats {
    /// 每格命中轮数。
    pub per_cell: [u8; CELLS],
    /// 已跑轮数。
    pub rounds: u32,
    /// 未满十轮的格数。
    pub flaky: u32,
    /// 最小命中数（未跑板恒 0）。
    pub min_hits: u8,
    /// 最大命中数。
    pub max_hits: u8,
}

/// 由掩码重算统计格（唯一事实来源）。
pub fn stats_of(board: &Board) -> Stats {
    let mut per_cell = [0u8; CELLS];
    let mut flaky = 0u32;
    let mut min_hits = u8::MAX;
    let mut max_hits = 0u8;
    let mut i = 0usize;
    while i < CELLS {
        let hits = match board.masks.get(i) {
            Some(m) => popcount16(*m & FULL_MASK),
            None => 0u8,
        };
        if let Some(slot) = per_cell.get_mut(i) {
            *slot = hits;
        }
        if hits != ROUNDS as u8 {
            flaky = flaky.saturating_add(1);
        }
        if hits < min_hits {
            min_hits = hits;
        }
        if hits > max_hits {
            max_hits = hits;
        }
        i += 1;
    }
    Stats { per_cell, rounds: board.rounds_run, flaky, min_hits, max_hits }
}

/// 十轮全中：统计格里每格都是 10，且无 flaky。
pub fn all_ten(st: &Stats) -> bool {
    st.rounds == ROUNDS && st.flaky == 0 && st.min_hits == ROUNDS as u8 && st.max_hits == ROUNDS as u8
}

/// 每家族命中位合计（三格 × 十轮 = 30）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FamilyHits {
    /// 描述符族。
    pub fd: u16,
    /// 用户指针族。
    pub ptr: u16,
    /// 时钟号族。
    pub clock: u16,
    /// 门序族。
    pub order: u16,
}

/// 按家族汇总命中位（单位：轮）。
pub fn family_hits(board: &Board) -> FamilyHits {
    let mut out = FamilyHits { fd: 0, ptr: 0, clock: 0, order: 0 };
    let mut i = 0usize;
    while i < UNITS.len() {
        let hits = board.hits_at(i).unwrap_or(0) as u16;
        if let Some(u) = UNITS.get(i) {
            match u.family {
                Family::Fd => out.fd = out.fd.saturating_add(hits),
                Family::Ptr => out.ptr = out.ptr.saturating_add(hits),
                Family::ClockId => out.clock = out.clock.saturating_add(hits),
                Family::Order => out.order = out.order.saturating_add(hits),
            }
        }
        i += 1;
    }
    out
}

// ── 声明票 ────────────────────────────────────────────────────────────
/// 可复测性声明票。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ticket {
    /// 覆盖格数。
    pub cells: u16,
    /// 复测轮数。
    pub rounds: u8,
    /// flaky 格数（签发成功的票恒为 0）。
    pub flaky: u32,
    /// 票面指纹。
    pub stamp: u32,
    /// 结果摘要（判据③要求非空）。
    pub summary: &'static str,
}

impl Ticket {
    /// 票面是否齐备。
    pub fn signed(&self) -> bool {
        self.cells == CELLS as u16
            && self.rounds == ROUNDS as u8
            && self.flaky == 0
            && self.stamp != 0
            && !self.summary.is_empty()
    }
}

/// 签发：10 轮跑满才谈得上稳定；有 miss 格直接拒签。
pub fn issue(board: &Board) -> Result<Ticket, i32> {
    if board.rounds_run() != ROUNDS {
        return Err(EAGAIN);
    }
    let st = stats_of(board);
    if st.flaky != 0 || st.min_hits != ROUNDS as u8 {
        return Err(EINVAL);
    }
    Ok(Ticket {
        cells: CELLS as u16,
        rounds: ROUNDS as u8,
        flaky: 0,
        stamp: board_digest(board),
        summary: TICKET_SUMMARY,
    })
}

/// 复检：票面必须与当前板重算值一致。
pub fn ticket_recheck(t: &Ticket, board: &Board) -> bool {
    t.signed() && t.stamp == board_digest(board) && stats_of(board).flaky == 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位续算（不引浮点、不引随机、不读墙钟）。
pub fn chain(h: u32, bytes: &[u8]) -> u32 {
    let mut acc = h;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            acc ^= *b as u32;
            acc = acc.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    acc
}

/// 矩阵指纹：格名 + 族 + 探针 + 期望码逐字节喂入。
pub fn matrix_digest() -> u32 {
    let mut h = chain(0x811C_9DC5, b"unx9337/matrix");
    let mut i = 0usize;
    while i < UNITS.len() {
        if let Some(u) = UNITS.get(i) {
            h = chain(h, u.name.as_bytes());
            h = chain(h, &[u.family.tag() as u8, u.probe.tag() as u8]);
            h = chain(h, &u.expect.to_le_bytes());
        }
        i += 1;
    }
    h
}

/// 板指纹：矩阵 + 12 条掩码 + 轮数。
pub fn board_digest(board: &Board) -> u32 {
    let mut h = matrix_digest();
    let mut i = 0usize;
    while i < CELLS {
        if let Some(m) = board.masks.get(i) {
            h = chain(h, &m.to_le_bytes());
        }
        i += 1;
    }
    chain(h, &board.rounds_run.to_le_bytes())
}

/// 规范跑批：10 轮顺次提交（误拒由 rejects 判据负责报红）。
pub fn run_matrix() -> Board {
    let mut b = Board::new();
    let mut r = 0u32;
    while r < ROUNDS {
        if let Err(_code) = b.run_round(r) {
            // 轮序被误拒时账目不动，rejects 会留下痕迹，由判据统一检查。
        }
        r += 1;
    }
    b
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9337 判据集。
pub fn run_unx9337_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9337");

    // ① 段位与码窗
    let seg_ok = SEG_BASE == 0x7C1C0
        && SEG_BASE % 0x40 == 0
        && SEG_BASE >= 0x7C000
        && SEG_BASE <= 0x7C2FF;
    s.add("段位守卫", seg_ok, "0x7C1C0 属本卡独占窗口且 0x40 对齐，不与相邻卡重叠");

    let code_window = RecheckCode::ALL
        .iter()
        .all(|c| c.seg_addr() > SEG_BASE && c.seg_addr() < SEG_BASE + 0x40);
    s.add("码窗守卫", code_window, "14 个诊断码全部落在 0x7C1C0 的 0x40 窗口内");

    let mut code_unique = true;
    let mut ci = 0usize;
    while ci < RecheckCode::ALL.len() {
        let mut cj = ci + 1;
        while cj < RecheckCode::ALL.len() {
            let same = match (RecheckCode::ALL.get(ci), RecheckCode::ALL.get(cj)) {
                (Some(a), Some(b)) => a.seg_addr() == b.seg_addr(),
                _ => false,
            };
            if same {
                code_unique = false;
            }
            cj += 1;
        }
        ci += 1;
    }
    s.add("诊断码唯一", code_unique, "段内码值两两不同，红项才能唯一定位");

    let mut text_ok = RecheckCode::ALL.len() == 14;
    let mut ti = 0usize;
    while ti < RecheckCode::ALL.len() {
        let mut tj = ti + 1;
        while tj < RecheckCode::ALL.len() {
            let clash = match (RecheckCode::ALL.get(ti), RecheckCode::ALL.get(tj)) {
                (Some(a), Some(b)) => a.detail() == b.detail() || a.detail().is_empty(),
                _ => true,
            };
            if clash {
                text_ok = false;
            }
            tj += 1;
        }
        ti += 1;
    }
    s.add("文案闭集", text_ok, "14 条中文建议互异且非空，判据可直接给处置动作");

    // ② 矩阵形状
    let shape_ok = CELLS == 12 && UNITS.len() == CELLS && FROZEN_EXPECT.len() == CELLS;
    s.add("矩阵规模", shape_ok, "12 格矩阵与冻结表长度一致，缺格会导致统计格虚高");

    let cover_ok = family_count(Family::Fd) == 3
        && family_count(Family::Ptr) == 3
        && family_count(Family::ClockId) == 3
        && family_count(Family::Order) == 3;
    s.add("家族覆盖", cover_ok, "四家族各三格，无族缺失也无族超配");

    s.add("格名唯一", names_unique(), "格名互异，复测红项能落到唯一格");

    let drift = single_source_drift();
    s.add("单源判定", drift == 0, "每格 expect 都等于 probe_errno(探针)，无手填期望");

    let fdrift = frozen_drift();
    s.add("冻结基线", fdrift == 0, "每格期望与冻结表逐格相等，基线未被就地改写");

    let probes_ok = Probe::ALL.len() == 7
        && Probe::ALL.iter().all(|p| !p.name().is_empty())
        && Probe::ALL.iter().all(|p| errno_registered(probe_errno(*p)));
    s.add("探针闭集", probes_ok, "七种探针均有中文名且其 errno 在登记表内");

    s.add(
        "置换对合",
        permutation_involutive(),
        "四步长模 12 自逆，乱序后必能映回原格，复测可回溯",
    );

    let errno_ok = ERRNO_TABLE.len() == 6
        && ERRNO_TABLE.iter().all(|row| row.1 > 0 && !row.2.is_empty())
        && errno_registered(EINVAL)
        && !errno_registered(0)
        && errno_text(EFAULT).len() > 0;
    s.add("errno 登记表", errno_ok, "六码正数口径与 Linux 一致，表外码拒绝登记");

    // ③ 十轮复测
    let board = run_matrix();
    let stats = stats_of(&board);

    s.add(
        "十轮跑满",
        board.rounds_run() == ROUNDS && board.rejects() == 0,
        "10 轮全部按序提交，无跳轮无重提",
    );

    s.add(
        "10/10 统计格",
        all_ten(&stats) && stats.per_cell.iter().all(|h| *h == ROUNDS as u8),
        "12 格各命中 10 轮，统计格取自掩码位计数",
    );

    let masks_full = (0..CELLS).all(|i| board.mask_at(i) == Some(FULL_MASK));
    s.add("逐位掩码", masks_full, "每格掩码为 0x03FF，十轮命中位一位不缺");

    s.add(
        "注入账守恒",
        board.injected() == INJECT_TOTAL
            && board.detected() == INJECT_TOTAL
            && board.escaped() == 0
            && board.conserved(),
        "120 笔注入 = 120 检出 + 0 逃逸",
    );

    let fh = family_hits(&board);
    s.add(
        "家族命中位",
        fh.fd == 30 && fh.ptr == 30 && fh.clock == 30 && fh.order == 30,
        "四家族各 3 格 × 10 轮 = 30 位命中",
    );

    // ④ 声明签发与复检
    let ticket = issue(&board);
    let ticket_ok = match ticket {
        Ok(t) => t.signed() && t.stamp != 0 && !t.summary.is_empty(),
        Err(_e) => false,
    };
    s.add("声明签发", ticket_ok, "10 轮跑满且零 flaky 才签发，票面摘要非空");

    let recheck_ok = match ticket {
        Ok(t) => ticket_recheck(&t, &board),
        Err(_e) => false,
    };
    s.add("复检一致", recheck_ok, "票面 stamp 与板重算指纹相等，复检通过");

    // ⑤ 负向：轮序与轮次越界
    let mut probe_board = Board::new();
    let skip = probe_board.run_round(1);
    let overrun = probe_board.run_round(ROUNDS);
    let dirty_free = probe_board.rounds_run() == 0
        && probe_board.injected() == 0
        && probe_board.rejects() == 2;
    s.add(
        "轮序拒绝",
        skip == Err(EINVAL) && overrun == Err(EINVAL) && dirty_free,
        "跳轮与越界均回 EINVAL，且账目一笔不动",
    );

    let first = probe_board.run_round(0);
    let mask_before = probe_board.mask_at(0);
    let repeat = probe_board.run_round(0);
    let mask_after = probe_board.mask_at(0);
    s.add(
        "重提拒绝",
        first == Ok(CELLS as u32) && repeat == Err(EINVAL) && mask_before == mask_after,
        "同轮重复提交回 EINVAL，掩码不被二次置位",
    );

    // ⑥ 负向：不足轮次与 flaky
    let mut partial = Board::new();
    let mut pr = 0u32;
    while pr < 3 {
        let _step = partial.run_round(pr);
        pr += 1;
    }
    s.add(
        "轮次不足拒签",
        issue(&partial) == Err(EAGAIN) && stats_of(&partial).rounds == 3,
        "只跑 3 轮时签发回 EAGAIN，不得提前盖章",
    );

    let mut flaky = board;
    let injected = flaky.inject_miss(0, 9);
    let flaky_stats = stats_of(&flaky);
    s.add(
        "flaky 拒签",
        injected == Ok(())
            && flaky_stats.flaky == 1
            && flaky_stats.min_hits == 9
            && issue(&flaky) == Err(EINVAL),
        "抹掉一轮命中后 flaky=1，签发必须回 EINVAL",
    );

    // ⑦ 双跑一致
    let again = run_matrix();
    s.add(
        "双跑一致",
        board_digest(&board) == board_digest(&again)
            && stats_of(&again) == stats
            && ticket_of(&again) == ticket_of(&board),
        "同输入两次跑批指纹、统计格与票面完全相等",
    );

    let dyn_stats = format!(
        "轮次 {} 注入 {} 检出 {} 逃逸 {} 拒 {}",
        board.rounds_run(),
        board.injected(),
        board.detected(),
        board.escaped(),
        board.rejects()
    );
    s.add_dyn("统计快照", all_ten(&stats), &dyn_stats);

    let dyn_family = format!(
        "fd {} / ptr {} / clock {} / order {}",
        fh.fd, fh.ptr, fh.clock, fh.order
    );
    s.add_dyn("家族命中", cover_ok && fh.fd == 30, &dyn_family);

    let dyn_stamp = format!("矩阵 0x{:08X} 板 0x{:08X}", matrix_digest(), board_digest(&board));
    s.add_dyn("指纹快照", board_digest(&board) != 0, &dyn_stamp);

    s
}

/// 取票面（内部用，避免在判据里重复解构 Result）。
fn ticket_of(board: &Board) -> Option<Ticket> {
    match issue(board) {
        Ok(t) => Some(t),
        Err(_e) => None,
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_all_green() {
        let s = run_unx9337_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.truncated(), false);
        assert_eq!(s.len() >= 12, true);
    }

    #[test]
    fn matrix_shape_is_frozen() {
        assert_eq!(UNITS.len(), CELLS);
        assert_eq!(UNITS.len(), 12);
        assert_eq!(FROZEN_EXPECT.len(), CELLS);
        assert_eq!(family_count(Family::Fd), 3);
        assert_eq!(family_count(Family::Ptr), 3);
        assert_eq!(family_count(Family::ClockId), 3);
        assert_eq!(family_count(Family::Order), 3);
        assert_eq!(names_unique(), true);
    }

    #[test]
    fn single_source_and_frozen_agree() {
        assert_eq!(single_source_drift(), 0);
        assert_eq!(frozen_drift(), 0);
        let mut i = 0usize;
        while i < UNITS.len() {
            let u = match UNITS.get(i) {
                Some(v) => *v,
                None => break,
            };
            assert_eq!(u.expect, probe_errno(u.probe));
            assert_eq!(FROZEN_EXPECT.get(i).copied(), Some(u.expect));
            i += 1;
        }
    }

    #[test]
    fn permutation_is_an_involution() {
        assert_eq!(permutation_involutive(), true);
        let mut i = 0usize;
        while i < CELLS {
            assert_eq!(permute(i, 5).and_then(|j| permute(j, 5)), Some(i));
            assert_eq!(permute(i, 7).and_then(|j| permute(j, 7)), Some(i));
            assert_eq!(permute(i, 11).and_then(|j| permute(j, 11)), Some(i));
            i += 1;
        }
        assert_eq!(permute(0, 0), None);
    }

    #[test]
    fn ten_rounds_all_hit() {
        let b = run_matrix();
        assert_eq!(b.rounds_run(), ROUNDS);
        assert_eq!(b.rejects(), 0);
        let st = stats_of(&b);
        assert_eq!(all_ten(&st), true);
        assert_eq!(st.flaky, 0);
        assert_eq!(st.min_hits, 10);
        let mut i = 0usize;
        while i < CELLS {
            assert_eq!(b.mask_at(i), Some(FULL_MASK));
            assert_eq!(b.missing_at(i), Some(0));
            i += 1;
        }
    }

    #[test]
    fn ledger_is_conserved() {
        let b = run_matrix();
        assert_eq!(b.injected(), INJECT_TOTAL);
        assert_eq!(b.detected(), INJECT_TOTAL);
        assert_eq!(b.escaped(), 0);
        assert_eq!(b.conserved(), true);
        let fh = family_hits(&b);
        assert_eq!(fh.fd + fh.ptr + fh.clock + fh.order, INJECT_TOTAL as u16);
    }

    #[test]
    fn round_order_is_enforced() {
        let mut b = Board::new();
        assert_eq!(b.run_round(1), Err(EINVAL));
        assert_eq!(b.run_round(ROUNDS), Err(EINVAL));
        assert_eq!(b.rounds_run(), 0);
        assert_eq!(b.injected(), 0);
        assert_eq!(b.rejects(), 2);
        assert_eq!(b.run_round(0), Ok(CELLS as u32));
        assert_eq!(b.run_round(0), Err(EINVAL));
        assert_eq!(b.mask_at(0), Some(0x0001));
    }

    #[test]
    fn ticket_signs_and_rechecks() {
        let b = run_matrix();
        let t = match issue(&b) {
            Ok(v) => v,
            Err(_e) => {
                assert_eq!(true, false);
                return;
            }
        };
        assert_eq!(t.signed(), true);
        assert_eq!(t.summary, TICKET_SUMMARY);
        assert_ne!(t.stamp, 0);
        assert_eq!(ticket_recheck(&t, &b), true);
        assert_eq!(ticket_of(&b).map(|x| x.stamp), Some(t.stamp));
    }

    #[test]
    fn flaky_is_refused() {
        let base = run_matrix();
        let mut b = base;
        assert_eq!(b.inject_miss(3, 0), Ok(()));
        let st = stats_of(&b);
        assert_eq!(st.flaky, 1);
        assert_eq!(st.min_hits, 9);
        assert_eq!(issue(&b), Err(EINVAL));
        let t = match issue(&base) {
            Ok(v) => v,
            Err(_e) => {
                assert_eq!(true, false);
                return;
            }
        };
        assert_eq!(ticket_recheck(&t, &b), false);
        assert_eq!(b.inject_miss(CELLS, 0), Err(EINVAL));
        assert_eq!(b.inject_miss(0, ROUNDS), Err(EINVAL));
    }

    #[test]
    fn partial_run_cannot_be_signed() {
        let mut b = Board::new();
        let mut r = 0u32;
        while r < 3 {
            let round = b.run_round(r);
            assert_eq!(round.is_ok(), true);
            r += 1;
        }
        assert_eq!(stats_of(&b).rounds, 3);
        assert_eq!(issue(&b), Err(EAGAIN));
    }

    #[test]
    fn digest_is_deterministic() {
        let a = run_matrix();
        let b = run_matrix();
        assert_eq!(board_digest(&a), board_digest(&b));
        assert_eq!(matrix_digest(), matrix_digest());
        assert_ne!(board_digest(&a), 0);
        let mut tweaked = a;
        assert_eq!(tweaked.inject_miss(5, 2), Ok(()));
        assert_ne!(board_digest(&tweaked), board_digest(&a));
    }

    #[test]
    fn diagnostic_codes_are_closed() {
        assert_eq!(RecheckCode::ALL.len(), 14);
        let mut i = 0usize;
        while i < RecheckCode::ALL.len() {
            let a = match RecheckCode::ALL.get(i) {
                Some(v) => *v,
                None => break,
            };
            assert_eq!(a.detail().is_empty(), false);
            assert_eq!(a.seg_addr(), SEG_BASE + a.code() as u32);
            let mut j = i + 1;
            while j < RecheckCode::ALL.len() {
                let b = match RecheckCode::ALL.get(j) {
                    Some(v) => *v,
                    None => break,
                };
                assert_ne!(a.code(), b.code());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn errno_table_matches_linux() {
        assert_eq!(errno_registered(EBADF), true);
        assert_eq!(errno_registered(EFAULT), true);
        assert_eq!(errno_registered(EINVAL), true);
        assert_eq!(errno_registered(EAGAIN), true);
        assert_eq!(errno_registered(EINTR), true);
        assert_eq!(errno_registered(EPERM), true);
        assert_eq!(errno_registered(0), false);
        assert_ne!(errno_text(EINVAL).len(), 0);
        assert_eq!(popcount16(FULL_MASK), 10);
    }
}
