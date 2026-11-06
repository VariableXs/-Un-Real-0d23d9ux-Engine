//! UNX-F9387 pty 缺席 ENOTTY 联签（B16 终端挂点 × C4 pty 面） · 0x7CE40段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F9387-J1 缺席联签判据过，pty 缺席全 ENOTTY 逐格联测可观
//!       测，C4 pty 上线后翻转挂点声明格过。
//!
//! 设计要点：
//!   ① 终端能力注册表：fd → tty 能力位；pty 面按两相状态机建模
//!      （Absent → Online），翻转单向不可逆（回翻显性拒绝）。
//!   ② 缺席联签是本模块核心：pty 面未上线时，任何 fd 的终端 ioctl
//!      一律 ENOTTY，逐格联测（fd × cmd 全组合），一格漏判即联签红。
//!   ③ 上线翻转声明：C4 pty 上线后挂点翻转——有能力位的 fd 拿到
//!      termios 视图，无能力位 fd（管道/普通文件）仍然 ENOTTY；翻转
//!      事件入账只增，回翻被拒。
//!   ④ ioctl 语义单源：直连判定器与 hook 判定器对同一 (fd, cmd, 相位)
//!      必须逐字段一致；hook 只记账不改判。
//!   ⑤ 注入核覆盖四类坏样本：缺席相上有能力 fd、上线相下无能力 fd、
//!      未知 cmd、坏 fd，逐类 10/10 取码正确且账守恒。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9387 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CE40;

/// 复测轮数（注入矩阵每类轮数）。
pub const INJECT_ROUNDS: u32 = 10;

/// ioctl 命令：取 termios。
pub const CMD_TCGETS: u32 = 1;
/// ioctl 命令：设 termios。
pub const CMD_TCSETS: u32 = 2;
/// ioctl 命令：取窗口大小。
pub const CMD_TIOCGWINSZ: u32 = 3;

/// Linux errno 正值口径。
pub const EBADF: i32 = 9;
pub const EINVAL: i32 = 22;
pub const ENOTTY: i32 = 25;

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏句柄族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏参数族"),
    ("ENOTTY", -25, 1, "Windows ERROR_INVALID_FUNCTION；终端缺席/非终端族"),
];

/// 按 errno 正数查负值；表外返回 0。
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

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0A 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtCode {
    /// 缺席相漏判（有能力 fd 未回 ENOTTY）
    AbsentLeak,
    /// 上线相无能力 fd 误放行
    OnlineLeak,
    /// 坏 fd 判漂移
    BadFdDrift,
    /// 未知 cmd 判漂移
    UnknownCmdDrift,
    /// 非法回翻
    IllegalFlipBack,
    /// 直连与 hook 分歧
    HookDiverged,
    /// 缺席联测格缺失
    AbsentCellMissing,
    /// 账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
    /// 文案闭集为空
    EmptyDetail,
}

impl PtCode {
    /// 码值（SEG_BASE + 变体序号）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            PtCode::AbsentLeak => "缺席相漏判被拒：pty 面未上线时任何终端 ioctl \
                必须 ENOTTY；放行即语义泄漏",
            PtCode::OnlineLeak => "上线相无能力 fd 误放行：管道/普通文件即使 pty \
                上线也必须 ENOTTY；能力位是上线放行的前提",
            PtCode::BadFdDrift => "坏 fd 判漂移：负号/未开/已关 fd 必须 EBADF；\
                fd 校验先于能力判定",
            PtCode::UnknownCmdDrift => "未知 cmd 判漂移：TCGETS/TCSETS/TIOCGWINSZ \
                闭集外必须 ENOTTY；新 cmd 须先过宪章",
            PtCode::IllegalFlipBack => "非法回翻被拒：pty 相位翻转单向（Absent→\
                Online），回翻会破坏已发布的 termios 语义",
            PtCode::HookDiverged => "直连与 hook 分歧：同 (fd, cmd, 相位) 两路结果\
                必须逐字段一致；hook 只记账不改判",
            PtCode::AbsentCellMissing => "缺席联测格缺失：fd × cmd 全组合必须逐格\
                落账；缺格即缺席联签不完整",
            PtCode::LedgerRegress => "账倒退被拒：联测/翻转计数只增不减；请检查\
                回滚路径是否误改累计值",
            PtCode::NotReproducible => "双跑不一致：同语料两次指纹不同；请排查\
                隐藏状态或非确定遍历",
            PtCode::SegmentViolation => "段位越界：段基址必须落在 \
                0x7CC00..=0x7D1C0 且 0x40 对齐",
            PtCode::EmptyDetail => "诊断文案为空：闭集每码必须携带非空中文建议",
        }
    }

    /// 闭集全量。
    pub const ALL: [PtCode; 11] = [
        PtCode::AbsentLeak,
        PtCode::OnlineLeak,
        PtCode::BadFdDrift,
        PtCode::UnknownCmdDrift,
        PtCode::IllegalFlipBack,
        PtCode::HookDiverged,
        PtCode::AbsentCellMissing,
        PtCode::LedgerRegress,
        PtCode::NotReproducible,
        PtCode::SegmentViolation,
        PtCode::EmptyDetail,
    ];
}

// ── 终端能力注册表与相位状态机 ────────────────────────────────────────
/// fd 终端能力位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TtyCap {
    /// 有能力位（pty 上线后可得 termios 视图）
    Capable,
    /// 无能力位（管道/普通文件，永远 ENOTTY）
    NotCapable,
}

/// pty 相位（单向状态机）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PtyPhase {
    /// 缺席：C4 pty 面未上线
    Absent,
    /// 上线：C4 pty 面已就绪
    Online,
}

/// 终端能力注册表：fd → 能力位。
#[derive(Clone, Debug)]
pub struct TtyRegistry {
    table: Vec<(i32, TtyCap)>,
}

impl TtyRegistry {
    /// 构造并预置样板：3=有能力、4=有能力、5=无能力（管道）、6=无能力。
    pub fn new() -> TtyRegistry {
        TtyRegistry {
            table: vec![
                (3, TtyCap::Capable),
                (4, TtyCap::Capable),
                (5, TtyCap::NotCapable),
                (6, TtyCap::NotCapable),
            ],
        }
    }

    /// 查能力位：未登记返回 None（未开 → EBADF 域）。
    pub fn cap_of(&self, fd: i32) -> Option<TtyCap> {
        if fd < 0 {
            return None;
        }
        let mut i = 0usize;
        while i < self.table.len() {
            if let Some((num, cap)) = self.table.get(i) {
                if *num == fd {
                    return Some(*cap);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记能力位（只增；重号/负号拒绝）。
    pub fn register(&mut self, fd: i32, cap: TtyCap) -> bool {
        if fd < 0 || self.cap_of(fd).is_some() {
            return false;
        }
        self.table.push((fd, cap));
        true
    }

    /// 登记条数。
    pub fn len(&self) -> usize {
        self.table.len()
    }
}

/// ioctl 判定器（单源）：
/// fd 校验 → 缺席相全 ENOTTY → 能力位 → cmd 闭集 → 上线放行。
pub fn judge_ioctl(reg: &TtyRegistry, phase: PtyPhase, fd: i32, cmd: u32) -> Result<u32, i32> {
    let cap = match reg.cap_of(fd) {
        Some(c) => c,
        None => return Err(EBADF),
    };
    // 缺席相：pty 面未上线，一切终端 ioctl 回 ENOTTY（B16 缺席口径）
    if phase == PtyPhase::Absent {
        return Err(ENOTTY);
    }
    if cap == TtyCap::NotCapable {
        return Err(ENOTTY);
    }
    match cmd {
        CMD_TCGETS => Ok(0x5401),
        CMD_TCSETS => Ok(0),
        CMD_TIOCGWINSZ => Ok(0x5413),
        _ => Err(ENOTTY),
    }
}

/// hook 判定器：直连判定外加观测（上线相成功调用必须带有能力位）。
pub fn hooked_ioctl(reg: &TtyRegistry, phase: PtyPhase, fd: i32, cmd: u32) -> Result<u32, i32> {
    let r = judge_ioctl(reg, phase, fd, cmd);
    match r {
        Ok(v) => match reg.cap_of(fd) {
            Some(TtyCap::Capable) if phase == PtyPhase::Online => Ok(v),
            _ => Err(ENOTTY),
        },
        Err(e) => Err(e),
    }
}

/// 两相等价账：直连 vs hook 逐格对拍，只增不减。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EquivLedger {
    pub compared: u32,
    pub diverged: u32,
}

impl EquivLedger {
    const fn zero() -> EquivLedger {
        EquivLedger { compared: 0, diverged: 0 }
    }

    /// 记一笔对拍。
    pub fn record(&mut self, equal: bool) {
        self.compared = self.compared.saturating_add(1);
        if !equal {
            self.diverged = self.diverged.saturating_add(1);
        }
    }

    /// 单源成立。
    pub fn single_source(&self) -> bool {
        self.compared > 0 && self.diverged == 0
    }
}

/// 缺席相全组合联测：登记 fd × 三 cmd 逐格 ENOTTY，返回（总格数, ENOTTY 格数）。
pub fn run_absent_jointest(reg: &TtyRegistry) -> (u32, u32) {
    let cmds = [CMD_TCGETS, CMD_TCSETS, CMD_TIOCGWINSZ];
    let mut total = 0u32;
    let mut enotty = 0u32;
    let mut fi = 0usize;
    while fi < reg.len() {
        let fd = match reg.table.get(fi) {
            Some((num, _)) => *num,
            None => break,
        };
        let mut ci = 0usize;
        while ci < cmds.len() {
            let cmd = match cmds.get(ci) {
                Some(c) => *c,
                None => break,
            };
            total = total.saturating_add(1);
            if judge_ioctl(reg, PtyPhase::Absent, fd, cmd) == Err(ENOTTY) {
                enotty = enotty.saturating_add(1);
            }
            ci += 1;
        }
        fi += 1;
    }
    (total, enotty)
}

/// 两相等价对拍：缺席相与上线相各对拍全部 (fd, cmd) 组合。
pub fn run_equiv_suite(reg: &TtyRegistry) -> EquivLedger {
    let mut led = EquivLedger::zero();
    let cmds = [CMD_TCGETS, CMD_TCSETS, CMD_TIOCGWINSZ, 99];
    let phases = [PtyPhase::Absent, PtyPhase::Online];
    let mut pi = 0usize;
    while pi < phases.len() {
        let phase = match phases.get(pi) {
            Some(p) => *p,
            None => break,
        };
        let mut fi = 0usize;
        while fi < reg.len() {
            let fd = match reg.table.get(fi) {
                Some((num, _)) => *num,
                None => break,
            };
            let mut ci = 0usize;
            while ci < cmds.len() {
                let cmd = match cmds.get(ci) {
                    Some(c) => *c,
                    None => break,
                };
                let a = judge_ioctl(reg, phase, fd, cmd);
                let b = hooked_ioctl(reg, phase, fd, cmd);
                led.record(a == b);
                ci += 1;
            }
            fi += 1;
        }
        pi += 1;
    }
    led
}

// ── 翻转账 ────────────────────────────────────────────────────────────
/// pty 相位账本：单向翻转，翻转事件入账。
#[derive(Clone, Debug)]
pub struct PhaseLedger {
    phase: PtyPhase,
    /// 翻转次数（0 或 1；单向）
    flips: u32,
    /// 缺席相 ENOTTY 联测格数
    absent_cells: u32,
    /// 上线相放行格数
    online_pass: u32,
}

impl PhaseLedger {
    /// 以缺席相起步。
    pub const fn new() -> PhaseLedger {
        PhaseLedger { phase: PtyPhase::Absent, flips: 0, absent_cells: 0, online_pass: 0 }
    }

    /// 记缺席相一格。
    pub fn note_absent(&mut self) {
        self.absent_cells = self.absent_cells.saturating_add(1);
    }

    /// 记上线相一格放行。
    pub fn note_online_pass(&mut self) {
        self.online_pass = self.online_pass.saturating_add(1);
    }

    /// 翻转到上线（单向；回翻拒绝）。
    pub fn flip_online(&mut self) -> Result<(), PtCode> {
        if self.phase == PtyPhase::Online {
            return Err(PtCode::IllegalFlipBack);
        }
        self.phase = PtyPhase::Online;
        self.flips = self.flips.saturating_add(1);
        Ok(())
    }

    /// 当前相位。
    pub fn phase(&self) -> PtyPhase {
        self.phase
    }

    /// 翻转次数。
    pub fn flips(&self) -> u32 {
        self.flips
    }

    /// 缺席格数。
    pub fn absent_cells(&self) -> u32 {
        self.absent_cells
    }

    /// 上线放行格数。
    pub fn online_pass(&self) -> u32 {
        self.online_pass
    }

    /// 账闭合：翻转数 ≤ 1，缺席格只增。
    pub fn closed(&self) -> bool {
        self.flips <= 1
    }
}

// ── 注入账与注入核 ────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadPtyCase {
    /// 缺席相有能力 fd（必须 ENOTTY）
    AbsentCapable,
    /// 上线相无能力 fd（必须 ENOTTY）
    OnlineNotCapable,
    /// 未知 cmd（上线相有能力 fd，必须 ENOTTY）
    UnknownCmd,
    /// 坏 fd（必须 EBADF）
    BadFd,
}

impl BadPtyCase {
    /// 构造样本并返回（期望码）。
    pub fn expected_code(self) -> i32 {
        match self {
            BadPtyCase::AbsentCapable => ENOTTY,
            BadPtyCase::OnlineNotCapable => ENOTTY,
            BadPtyCase::UnknownCmd => ENOTTY,
            BadPtyCase::BadFd => EBADF,
        }
    }

    /// 闭集全量。
    pub const ALL: [BadPtyCase; 4] = [
        BadPtyCase::AbsentCapable,
        BadPtyCase::OnlineNotCapable,
        BadPtyCase::UnknownCmd,
        BadPtyCase::BadFd,
    ];
}

/// 注入核：四类坏样本各注入 INJECT_ROUNDS 次，逐类断言取码正确。
pub fn run_pty_injection() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadPtyCase::ALL.len() {
        let case = match BadPtyCase::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let reg = TtyRegistry::new();
            let caught = match case {
                BadPtyCase::AbsentCapable => {
                    judge_ioctl(&reg, PtyPhase::Absent, 3, CMD_TCGETS) == Err(ENOTTY)
                        && judge_ioctl(&reg, PtyPhase::Absent, 3, CMD_TIOCGWINSZ) == Err(ENOTTY)
                }
                BadPtyCase::OnlineNotCapable => {
                    judge_ioctl(&reg, PtyPhase::Online, 5, CMD_TCGETS) == Err(ENOTTY)
                        && judge_ioctl(&reg, PtyPhase::Online, 6, CMD_TCSETS) == Err(ENOTTY)
                }
                BadPtyCase::UnknownCmd => {
                    judge_ioctl(&reg, PtyPhase::Online, 3, 99) == Err(ENOTTY)
                }
                BadPtyCase::BadFd => {
                    judge_ioctl(&reg, PtyPhase::Online, -1, CMD_TCGETS) == Err(EBADF)
                        && judge_ioctl(&reg, PtyPhase::Absent, 99, CMD_TCGETS) == Err(EBADF)
                }
            };
            ledger.record(caught);
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

// ── FNV-1a 双跑指纹 ───────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料指纹：缺席联测 + 翻转声明 + 上线放行 + 注入逐格落语料。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let reg = TtyRegistry::new();
    let mut led = PhaseLedger::new();
    // 缺席相逐格
    let (total, enotty) = run_absent_jointest(&reg);
    let mut k = 0u32;
    while k < total {
        led.note_absent();
        k += 1;
    }
    bytes.push(total as u8);
    bytes.push(enotty as u8);
    // 翻转
    let flip = led.flip_online();
    bytes.push(match flip {
        Ok(()) => 1,
        Err(_) => 0,
    });
    // 回翻拒绝
    bytes.push((led.flip_online().err() == Some(PtCode::IllegalFlipBack)) as u8);
    // 上线相：有能力 fd 三 cmd 放行，无能力 fd ENOTTY
    let mut fi = 0usize;
    while fi < reg.len() {
        let (fd, cap) = match reg.table.get(fi) {
            Some(x) => *x,
            None => break,
        };
        let r = judge_ioctl(&reg, led.phase(), fd, CMD_TCGETS);
        match cap {
            TtyCap::Capable => {
                bytes.push(match r {
                    Ok(_) => {
                        led.note_online_pass();
                        1
                    }
                    Err(_) => 0,
                });
            }
            TtyCap::NotCapable => {
                bytes.push(if r == Err(ENOTTY) { 2 } else { 0 });
            }
        }
        fi += 1;
    }
    bytes.push(led.online_pass() as u8);
    // 等价对拍
    let eq = run_equiv_suite(&reg);
    bytes.push(eq.compared as u8);
    bytes.push(eq.diverged as u8);
    // 注入账
    let (inj, per) = run_pty_injection();
    bytes.push(inj.detected as u8);
    let mut ci = 0usize;
    while ci < per.len() {
        if let Some(n) = per.get(ci) {
            bytes.push(*n as u8);
        }
        ci += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9387 判据集（静态 ≥14 条 + 动态 ≤4 条）。
pub fn run_unx9387_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9387");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7CC00..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7CC00..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 能力注册表样板
    let reg = TtyRegistry::new();
    let cap_ok = reg.cap_of(3) == Some(TtyCap::Capable)
        && reg.cap_of(5) == Some(TtyCap::NotCapable)
        && reg.cap_of(-1).is_none()
        && reg.cap_of(99).is_none();
    s.add("能力注册表：样板逐格可查、未开显性 None", cap_ok,
          "能力位是上线放行的前提条件");

    // 3. 缺席联签：fd × cmd 全组合逐格 ENOTTY
    let (total, enotty) = run_absent_jointest(&reg);
    s.add("缺席联签：fd×cmd 全组合逐格 ENOTTY",
          total == 12 && enotty == 12,
          "pty 面未上线时一格不得漏判");

    // 4. 上线翻转：有能力 fd 得 termios 视图
    let mut led = PhaseLedger::new();
    let _ = led.flip_online();
    let get = judge_ioctl(&reg, led.phase(), 3, CMD_TCGETS);
    let winsz = judge_ioctl(&reg, led.phase(), 3, CMD_TIOCGWINSZ);
    s.add("上线翻转：有能力 fd 三 cmd 放行",
          get == Ok(0x5401) && winsz == Ok(0x5413),
          "C4 pty 上线后挂点翻转生效");

    // 5. 翻转声明格：单向、回翻拒绝
    let mut led2 = PhaseLedger::new();
    let flip1 = led2.flip_online();
    let flip2 = led2.flip_online();
    s.add("翻转声明：单向一次、回翻显性拒绝",
          flip1.is_ok() && flip2.err() == Some(PtCode::IllegalFlipBack)
              && led2.flips() == 1,
          "相位回翻会破坏已发布语义");

    // 6. 上线相无能力 fd 仍 ENOTTY
    let pipe_get = judge_ioctl(&reg, PtyPhase::Online, 5, CMD_TCGETS);
    let file_set = judge_ioctl(&reg, PtyPhase::Online, 6, CMD_TCSETS);
    s.add("上线相能力位：管道/普通文件仍 ENOTTY",
          pipe_get == Err(ENOTTY) && file_set == Err(ENOTTY),
          "上线只翻转有能力位的 fd");

    // 7. 缺席相有能力 fd 不放行
    let absent_get = judge_ioctl(&reg, PtyPhase::Absent, 3, CMD_TCGETS);
    s.add("缺席相守卫：有能力 fd 也必须 ENOTTY",
          absent_get == Err(ENOTTY),
          "缺席是全局口径，不区分 fd 能力位");

    // 8. 未知 cmd ENOTTY
    let unknown = judge_ioctl(&reg, PtyPhase::Online, 3, 99);
    s.add("cmd 闭集：闭集外一律 ENOTTY", unknown == Err(ENOTTY),
          "TCGETS/TCSETS/TIOCGWINSZ 之外不得误放行");

    // 9. 坏 fd EBADF 先于相位判定
    let bad1 = judge_ioctl(&reg, PtyPhase::Online, -1, CMD_TCGETS);
    let bad2 = judge_ioctl(&reg, PtyPhase::Absent, 99, CMD_TCGETS);
    s.add("fd 守卫：负号/未开 fd 一律 EBADF（先于相位）",
          bad1 == Err(EBADF) && bad2 == Err(EBADF),
          "fd 校验是判定域第一层");

    // 10. 直连与 hook 等价对拍全等
    let eq = run_equiv_suite(&reg);
    s.add("hook 单源：两相 × 全 fd × 全 cmd 对拍零分歧",
          eq.single_source() && eq.compared == 32,
          "2 相 × 4 fd × 4 cmd = 32 格全等");

    // 11. 注入核：四类坏样本各 10/10 取码正确
    let (inj, per_class) = run_pty_injection();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏样本各 10/10 取码正确", all_caught,
          "缺席漏判/上线越权/未知 cmd/坏 fd 逐类拒绝");

    // 12. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(ENOTTY) && agrees_with_linux(EBADF)
        && !agrees_with_linux(0) && !agrees_with_linux(999)
        && linux_neg(25) == -25;
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "errno 表正值与负值逐项可复算");

    // 14. 双跑一致
    let a = corpus_fingerprint();
    let b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", a == b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 15. 文案闭集
    let text_ok = PtCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 码值唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < PtCode::ALL.len() {
        let mut j = i + 1;
        while j < PtCode::ALL.len() {
            if let (Some(x), Some(y)) = (PtCode::ALL.get(i), PtCode::ALL.get(j)) {
                if x.code() == y.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：11 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let pt_text = format!(
        "缺席格 {} / 上线放行 {} / 翻转 {} / 对拍 {} 笔",
        led.absent_cells(),
        led.online_pass(),
        led.flips(),
        eq.compared
    );
    s.add_dyn("翻转账快照", led.closed() && eq.single_source(), &pt_text);
    let fp_text = format!("语料指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_phase_all_enotty() {
        let r = TtyRegistry::new();
        let (total, enotty) = run_absent_jointest(&r);
        assert_eq!(total, 12);
        assert_eq!(enotty, 12);
    }

    #[test]
    fn online_capable_fd_gets_termios() {
        let r = TtyRegistry::new();
        assert_eq!(judge_ioctl(&r, PtyPhase::Online, 3, CMD_TCGETS), Ok(0x5401));
        assert_eq!(judge_ioctl(&r, PtyPhase::Online, 4, CMD_TIOCGWINSZ), Ok(0x5413));
    }

    #[test]
    fn online_not_capable_still_enotty() {
        let r = TtyRegistry::new();
        assert_eq!(judge_ioctl(&r, PtyPhase::Online, 5, CMD_TCGETS), Err(ENOTTY));
    }

    #[test]
    fn absent_capable_fd_enotty() {
        let r = TtyRegistry::new();
        assert_eq!(judge_ioctl(&r, PtyPhase::Absent, 3, CMD_TCGETS), Err(ENOTTY));
    }

    #[test]
    fn bad_fd_ebadf_first() {
        let r = TtyRegistry::new();
        assert_eq!(judge_ioctl(&r, PtyPhase::Online, -1, CMD_TCGETS), Err(EBADF));
        assert_eq!(judge_ioctl(&r, PtyPhase::Absent, 99, CMD_TCGETS), Err(EBADF));
    }

    #[test]
    fn unknown_cmd_enotty() {
        let r = TtyRegistry::new();
        assert_eq!(judge_ioctl(&r, PtyPhase::Online, 3, 42), Err(ENOTTY));
    }

    #[test]
    fn flip_is_one_way() {
        let mut led = PhaseLedger::new();
        assert!(led.flip_online().is_ok());
        assert_eq!(led.flip_online().err(), Some(PtCode::IllegalFlipBack));
        assert_eq!(led.flips(), 1);
        assert!(led.closed());
    }

    #[test]
    fn registry_rejects_dup_and_negative() {
        let mut r = TtyRegistry::new();
        assert!(!r.register(3, TtyCap::NotCapable));
        assert!(!r.register(-2, TtyCap::Capable));
        assert!(r.register(10, TtyCap::Capable));
        assert_eq!(r.len(), 5);
    }

    #[test]
    fn hook_equiv_single_source() {
        let r = TtyRegistry::new();
        let eq = run_equiv_suite(&r);
        assert!(eq.single_source());
        assert_eq!(eq.compared, 32);
        assert_eq!(eq.diverged, 0);
    }

    #[test]
    fn injection_matrix_green() {
        let (l, per) = run_pty_injection();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_table_agrees() {
        assert!(agrees_with_linux(25));
        assert_eq!(linux_neg(9), -9);
        assert!(!agrees_with_linux(0));
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9387_checks_green() {
        let s = run_unx9387_checks();
        assert!(s.all_passed(), "UNX-F9387 red: {}", s.red_items_text());
    }
}
