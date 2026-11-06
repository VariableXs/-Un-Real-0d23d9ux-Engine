//! UNX-F10018 波次报表门（收口前置） · 0x91480 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡号：UNX-F10018（B21 波次报表线）。
//! 锚点：UNX-F10019-J1 同步留痕（报表版本↔handoff 版本对齐 10/10 次），漂移 10/10 次检出。
//!
//! 数据段说明：
//!   SEG_BASE = 0x91480（卡面 seg 闭集）。诊断码占用段内 +0x01..+0x0C
//!   闭集窗口，码值 = SEG_BASE + 1 + 变体序号，段内两两唯一；全部码位
//!   由 `GateCode::ALL` 封闭枚举给出，禁止运行期造码。
//!
//! 结构清单：
//!   - 前置注册表 `WaveFreezeBoard`：六波冻结板（wave-01..06 各挂版本
//!     号与冻结指纹），缺一波即缺前置；
//!   - 状态机 `GateState`：Open（未冻结=红）→ Armed（六波齐冻结=绿）
//!     → Passed（收口放行）单向推进，Passed 后不可重开；
//!   - 账本 `GateLedger`：收口请求 = 放行 + 拦截，越闸恒 0，只增不减；
//!   - 注入核 `BadGateCase`：本期未冻结收口 / 缺波 / 冻结版本漂移 /
//!     放行后重开 / 空指纹冻结五类坏样本逐类 10/10 次拦截；
//!   - Linux errno 对照表 + FNV-1a 双跑指纹（无浮点、无随机）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10018 独占，0x40 对齐窗口起点）。
pub const SEG_BASE: u32 = 0x91480;

/// 波次数（wave-01..06）。
pub const WAVE_N: usize = 6;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码（对照表口径）。
pub const EBADF: i32 = 9;
pub const EACCES: i32 = 13;
pub const ENOENT: i32 = 2;
pub const EINVAL: i32 = 22;
pub const EBUSY: i32 = 16;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateCode {
    /// 本期报表未冻结即收口
    UnfrozenClose,
    /// 缺波（六波不满）
    MissingWave,
    /// 冻结版本漂移（板内版本与请求版本不符）
    FreezeVersionDrift,
    /// 放行后重开
    ReopenAfterPass,
    /// 空指纹冻结
    EmptyFingerprint,
    /// 账本倒退
    LedgerRegress,
    /// 重复冻结
    DuplicateFreeze,
    /// 波号越界
    WaveOutOfRange,
    /// 门未武装即放行
    NotArmed,
    /// 冻结指纹不符
    FingerprintMismatch,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl GateCode {
    /// 码值（段基址 +0x01 起连续编号，闭集唯一）。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            GateCode::UnfrozenClose => "未冻结收口被拦：本期报表尚未冻结；请先走\
                冻结段再发起收口（判据原文：未冻结收口拦截 10/10 次）",
            GateCode::MissingWave => "缺波被拦：六波冻结板有缺位；请补齐全部波次\
                冻结记录",
            GateCode::FreezeVersionDrift => "冻结版本漂移：请求版本与冻结板登记\
                版本不一致；请对齐版本后重试",
            GateCode::ReopenAfterPass => "放行后重开被拒：门已 Passed；收口一次\
                一放行，重开必须走新期",
            GateCode::EmptyFingerprint => "空指纹冻结被拒：指纹 0 视为未冻结；请\
                先计算内容指纹",
            GateCode::LedgerRegress => "门账倒退被拒：计数只增不减；请排查回滚\
                路径误改累计值",
            GateCode::DuplicateFreeze => "重复冻结被拒：同波已冻结；请走差异通道",
            GateCode::WaveOutOfRange => "波号越界被拒：波号必须落在 0..6；请核对",
            GateCode::NotArmed => "门未武装：六波未齐即请求放行；请先补齐前置",
            GateCode::FingerprintMismatch => "指纹不符：请求携带指纹与冻结板不符；\
                请以冻结板为准",
            GateCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定哈希",
            GateCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 \
                0x91480..=0x91800",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [GateCode; 12] = [
        GateCode::UnfrozenClose,
        GateCode::MissingWave,
        GateCode::FreezeVersionDrift,
        GateCode::ReopenAfterPass,
        GateCode::EmptyFingerprint,
        GateCode::LedgerRegress,
        GateCode::DuplicateFreeze,
        GateCode::WaveOutOfRange,
        GateCode::NotArmed,
        GateCode::FingerprintMismatch,
        GateCode::NotReproducible,
        GateCode::SegmentViolation,
    ];
}

// ── 门状态机 ──────────────────────────────────────────────────────────
/// 门状态：单向 Open → Armed → Passed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateState {
    /// 未冻结（红）
    Open,
    /// 六波齐冻结（绿，可放行）
    Armed,
    /// 已放行（本期收口完成）
    Passed,
}

// ── 六波冻结板 ────────────────────────────────────────────────────────
/// 单波冻结记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaveFreeze {
    /// 冻结版本号（逐期 +1）
    pub version: u32,
    /// 内容指纹（非 0）
    pub fingerprint: u32,
}

/// 六波冻结板：wave-01..06 各挂一条冻结记录。
pub struct WaveFreezeBoard {
    slots: [Option<WaveFreeze>; WAVE_N],
    /// 已冻结波数
    pub frozen: usize,
}

impl WaveFreezeBoard {
    /// 空板。
    pub fn new() -> WaveFreezeBoard {
        WaveFreezeBoard { slots: [None; WAVE_N], frozen: 0 }
    }

    /// 冻结一波：波号越界/重复/空指纹显性拒绝。
    pub fn freeze(&mut self, wave: usize, rec: WaveFreeze) -> Result<(), GateCode> {
        if wave >= WAVE_N {
            return Err(GateCode::WaveOutOfRange);
        }
        if rec.fingerprint == 0 {
            return Err(GateCode::EmptyFingerprint);
        }
        if let Some(slot) = self.slots.get(wave) {
            if slot.is_some() {
                return Err(GateCode::DuplicateFreeze);
            }
        }
        if let Some(slot) = self.slots.get_mut(wave) {
            *slot = Some(rec);
            self.frozen = self.frozen.saturating_add(1);
            Ok(())
        } else {
            Err(GateCode::WaveOutOfRange)
        }
    }

    /// 取波冻结记录。
    pub fn get(&self, wave: usize) -> Option<WaveFreeze> {
        if wave >= WAVE_N {
            return None;
        }
        match self.slots.get(wave) {
            Some(Some(rec)) => Some(*rec),
            _ => None,
        }
    }

    /// 六波是否齐冻结。
    pub fn complete(&self) -> bool {
        self.frozen == WAVE_N
    }
}

// ── 报表门 ────────────────────────────────────────────────────────────
/// 波次报表门：收口前置闸。
pub struct WaveGate {
    /// 冻结板
    pub board: WaveFreezeBoard,
    /// 门状态
    pub state: GateState,
    /// 本期版本号（收口请求必须与此一致）
    pub period_version: u32,
}

impl WaveGate {
    /// 新门（Open=红）。
    pub fn new(period_version: u32) -> WaveGate {
        WaveGate {
            board: WaveFreezeBoard::new(),
            state: GateState::Open,
            period_version,
        }
    }

    /// 冻结一波并尝试武装：六波齐时 Open→Armed。
    pub fn freeze_wave(&mut self, wave: usize, rec: WaveFreeze) -> Result<GateState, GateCode> {
        if self.state == GateState::Passed {
            return Err(GateCode::ReopenAfterPass);
        }
        if rec.version != self.period_version {
            return Err(GateCode::FreezeVersionDrift);
        }
        self.board.freeze(wave, rec)?;
        if self.board.complete() {
            self.state = GateState::Armed;
        }
        Ok(self.state)
    }

    /// 收口请求：门必须 Armed 且版本一致；未冻结/漂移/未武装一律拦截。
    pub fn request_close(&mut self, version: u32, fp: u32) -> Result<(), GateCode> {
        match self.state {
            GateState::Open => Err(GateCode::UnfrozenClose),
            GateState::Armed => {
                if version != self.period_version {
                    return Err(GateCode::FreezeVersionDrift);
                }
                // 抽 wave-01 核指纹
                let rec = match self.board.get(0) {
                    Some(r) => r,
                    None => return Err(GateCode::MissingWave),
                };
                if rec.fingerprint != fp {
                    return Err(GateCode::FingerprintMismatch);
                }
                self.state = GateState::Passed;
                Ok(())
            }
            GateState::Passed => Err(GateCode::ReopenAfterPass),
        }
    }
}

// ── 门账 ──────────────────────────────────────────────────────────────
/// 门账：收口请求 = 放行 + 拦截，越闸（放行不该放的）恒 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct GateLedger {
    /// 收口请求总数
    pub requests: u32,
    /// 放行数
    pub passed: u32,
    /// 拦截数
    pub blocked: u32,
    /// 越闸数（不该放行的放行，必须为 0）
    pub breached: u32,
}

impl GateLedger {
    /// 记一次收口判定。
    pub fn record(&mut self, passed: bool, legitimate: bool) -> Result<(), GateCode> {
        self.requests = self.requests.saturating_add(1);
        if passed {
            self.passed = self.passed.saturating_add(1);
            if !legitimate {
                self.breached = self.breached.saturating_add(1);
            }
        } else {
            self.blocked = self.blocked.saturating_add(1);
        }
        if self.passed.saturating_add(self.blocked) != self.requests {
            return Err(GateCode::LedgerRegress);
        }
        Ok(())
    }

    /// 守恒式：零越闸。
    pub fn no_breach(&self) -> bool {
        self.breached == 0
            && self.passed.saturating_add(self.blocked) == self.requests
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏收口样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadGateCase {
    /// 本期未冻结即收口
    Unfrozen,
    /// 缺波（只冻 5 波即收口）
    MissingWave,
    /// 冻结版本漂移
    VersionDrift,
    /// 放行后重开
    Reopen,
    /// 空指纹冻结
    EmptyFp,
}

impl BadGateCase {
    /// 闭集全量。
    pub const ALL: [BadGateCase; 5] = [
        BadGateCase::Unfrozen,
        BadGateCase::MissingWave,
        BadGateCase::VersionDrift,
        BadGateCase::Reopen,
        BadGateCase::EmptyFp,
    ];

    /// 注入一轮：返回是否被显式拦截。
    pub fn inject(self) -> bool {
        match self {
            BadGateCase::Unfrozen => {
                let mut g = WaveGate::new(10);
                g.request_close(10, 0xABCD) == Err(GateCode::UnfrozenClose)
            }
            BadGateCase::MissingWave => {
                let mut g = WaveGate::new(11);
                let mut w = 0usize;
                while w < WAVE_N - 1 {
                    let _ = g.freeze_wave(w, WaveFreeze { version: 11, fingerprint: 0x1111 + w as u32 });
                    w += 1;
                }
                g.request_close(11, 0x1111) == Err(GateCode::UnfrozenClose)
            }
            BadGateCase::VersionDrift => {
                let mut g = WaveGate::new(12);
                g.freeze_wave(0, WaveFreeze { version: 9, fingerprint: 0x1 })
                    == Err(GateCode::FreezeVersionDrift)
            }
            BadGateCase::Reopen => {
                let mut g = WaveGate::new(13);
                let mut w = 0usize;
                while w < WAVE_N {
                    let _ = g.freeze_wave(w, WaveFreeze { version: 13, fingerprint: 0x2222 + w as u32 });
                    w += 1;
                }
                if g.request_close(13, 0x2222).is_err() {
                    return false;
                }
                g.request_close(13, 0x2222) == Err(GateCode::ReopenAfterPass)
            }
            BadGateCase::EmptyFp => {
                let mut g = WaveGate::new(14);
                g.freeze_wave(0, WaveFreeze { version: 14, fingerprint: 0 })
                    == Err(GateCode::EmptyFingerprint)
            }
        }
    }
}

/// 执行五类坏样本注入：每类 INJECT_ROUNDS 次。
pub fn run_gate_injection() -> (GateLedger, [u32; 5]) {
    let mut ledger = GateLedger::default();
    let mut per_class = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadGateCase::ALL.len() {
        if let Some(_case) = BadGateCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = BadGateCase::ALL.get(ci).copied().unwrap_or(BadGateCase::Unfrozen).inject();
                // 坏样本被拦=blocked；逃逸=不该放行的放行（越闸）。
                let _ = ledger.record(!caught, false);
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
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏闸柄族"),
    ("EACCES", -13, 5, "Windows ERROR_ACCESS_DENIED；未武装放行族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺波缺冻结族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；版本漂移族"),
    ("EBUSY", -16, 33, "Windows ERROR_DEVICE_IN_USE；门已放行占位"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
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

/// 规范语料双跑指纹：冻结六波→武装→放行全链逐字节入哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut g = WaveGate::new(100);
    let mut bytes: Vec<u8> = Vec::new();
    let mut w = 0usize;
    while w < WAVE_N {
        let fp = fnv1a(&[b'w', (w + 1) as u8]);
        let r = g.freeze_wave(w, WaveFreeze { version: 100, fingerprint: fp });
        bytes.push(if r.is_ok() { 1 } else { 0 });
        w += 1;
    }
    bytes.push(match g.state {
        GateState::Armed => 2,
        GateState::Open => 3,
        GateState::Passed => 4,
    });
    let fp0 = g.board.get(0).map(|r| r.fingerprint).unwrap_or(0);
    let close = g.request_close(100, fp0);
    bytes.push(if close.is_ok() { 5 } else { 6 });
    let reopen = g.request_close(100, fp0);
    bytes.push(if reopen.is_err() { 7 } else { 8 });
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10018 判据集（17 条静态 + 2 条动态明细）。
pub fn run_unx10018_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10018");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91480..=0x91800).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91480..=0x91800", seg_ok,
          "段基址唯一且落在本卡窗口；越界即拒绝注册");

    // 2. 未冻结收口 10/10 拦截
    let mut hits = 0u32;
    let mut n = 0u32;
    while n < INJECT_ROUNDS {
        let mut g = WaveGate::new(1000 + n);
        if g.request_close(1000 + n, 1) == Err(GateCode::UnfrozenClose) {
            hits += 1;
        }
        n += 1;
    }
    s.add("报表门：未冻结收口 10/10 次拦截", hits == INJECT_ROUNDS,
          "本期报表未冻结时门保持红（判据原文）");

    // 3. 六波齐冻结名 → Armed
    let mut g = WaveGate::new(20);
    let mut w = 0usize;
    while w < WAVE_N {
        let _ = g.freeze_wave(w, WaveFreeze { version: 20, fingerprint: 0x9000 + w as u32 });
        w += 1;
    }
    s.add("武装：六波齐冻结门转 Armed", g.state == GateState::Armed && g.board.complete(),
          "六波全冻是武装前置，缺一波保持红");

    // 4. 缺一波保持红
    let mut g2 = WaveGate::new(21);
    let mut w2 = 0usize;
    while w2 < WAVE_N - 1 {
        let _ = g2.freeze_wave(w2, WaveFreeze { version: 21, fingerprint: 0x8000 + w2 as u32 });
        w2 += 1;
    }
    s.add("缺波：五波冻结门保持 Open", g2.state == GateState::Open,
          "缺波即缺前置，不得武装");

    // 5. 武装后放行成功
    let fp0 = g.board.get(0).map(|r| r.fingerprint).unwrap_or(0);
    let passed = g.request_close(20, fp0).is_ok() && g.state == GateState::Passed;
    s.add("放行：Armed 且版本指纹一致即 Passed", passed,
          "收口一次一放行，放行后门封");

    // 6. 放行后重开拦截
    let reopen = g.request_close(20, fp0) == Err(GateCode::ReopenAfterPass);
    s.add("重开拦截：Passed 后再请求被拒", reopen,
          "放行后不可重开，新期必须新门");

    // 7. 冻结版本漂移拦截
    let mut g3 = WaveGate::new(22);
    let drift = g3.freeze_wave(0, WaveFreeze { version: 99, fingerprint: 1 })
        == Err(GateCode::FreezeVersionDrift);
    s.add("版本漂移：冻结版本≠本期版本被拦", drift,
          "冻结必须挂本期版本号（判据原文：漂移 10/10 次检出）");

    // 8. 收口版本漂移拦截
    let mut g4 = WaveGate::new(23);
    let mut w4 = 0usize;
    while w4 < WAVE_N {
        let _ = g4.freeze_wave(w4, WaveFreeze { version: 23, fingerprint: 0x7000 + w4 as u32 });
        w4 += 1;
    }
    let close_drift = g4.request_close(24, 0x7000) == Err(GateCode::FreezeVersionDrift);
    s.add("版本漂移：收口版本≠本期版本被拦", close_drift,
          "收口请求版本必须与门同期");

    // 9. 注入核：五类坏样本全拦截
    let (ledger, per_class) = run_gate_injection();
    let all_caught = per_class.iter().all(|v| *v == INJECT_ROUNDS);
    s.add("注入核：五类坏样本各 10/10 拦截", all_caught,
          "未冻结/缺波/漂移/重开/空指纹全被显式拒绝");

    // 10. 门账守恒：零越闸
    s.add("门账守恒：请求=放行+拦截且越闸=0",
          ledger.no_breach() && ledger.requests == 5 * INJECT_ROUNDS,
          "50 次请求全部入账，越闸恒 0");

    // 11. 空指纹冻结拒绝
    let empty_fp = WaveGate::new(30).freeze_wave(0, WaveFreeze { version: 30, fingerprint: 0 })
        == Err(GateCode::EmptyFingerprint);
    s.add("边界：空指纹冻结被拒", empty_fp,
          "指纹 0 等价未冻结，防假冻结");

    // 12. 重复冻结拒绝
    let mut g5 = WaveGate::new(31);
    let _ = g5.freeze_wave(0, WaveFreeze { version: 31, fingerprint: 5 });
    let dup = g5.freeze_wave(0, WaveFreeze { version: 31, fingerprint: 6 })
        == Err(GateCode::DuplicateFreeze);
    s.add("防重：同波二次冻结被拒", dup,
          "一波一期一冻结，重冻走差异通道");

    // 13. 诊断码闭集：文案非空
    let text_ok = GateCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 14. 诊断码唯一且落段内
    let mut uniq = true;
    let mut i = 0usize;
    while i < GateCode::ALL.len() {
        let mut j = i + 1;
        while j < GateCode::ALL.len() {
            if let (Some(x), Some(y)) = (GateCode::ALL.get(i), GateCode::ALL.get(j)) {
                if x.val() == y.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let in_seg = GateCode::ALL.iter().all(|c| c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0C);
    s.add("诊断码：12 码两两唯一且全部落段内", uniq && in_seg,
          "段内闭集码不得重复或越窗");

    // 15. Linux 逐码一致
    let codes_ok = agrees_with_linux(EBADF)
        && agrees_with_linux(EACCES)
        && agrees_with_linux(ENOENT)
        && agrees_with_linux(EINVAL)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 16. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", f1 == f2,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 17. 波号越界拒绝
    let oob = WaveGate::new(40).freeze_wave(WAVE_N, WaveFreeze { version: 40, fingerprint: 1 })
        == Err(GateCode::WaveOutOfRange);
    s.add("边界：波号越界（≥6）被拒", oob,
          "波号必须落在 0..6");

    // 动态明细（≤8）：门账一行 + 指纹一行
    let ledger_text = format!(
        "请求 {} / 放行 {} / 拦截 {} / 越闸 {}",
        ledger.requests, ledger.passed, ledger.blocked, ledger.breached
    );
    s.add_dyn("账目快照", ledger.no_breach(), &ledger_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unfrozen_close_blocked() {
        let mut g = WaveGate::new(1);
        assert_eq!(g.request_close(1, 1), Err(GateCode::UnfrozenClose));
    }

    #[test]
    fn five_waves_not_armed() {
        let mut g = WaveGate::new(2);
        let mut w = 0usize;
        while w < WAVE_N - 1 {
            let _ = g.freeze_wave(w, WaveFreeze { version: 2, fingerprint: 3 + w as u32 });
            w += 1;
        }
        assert_eq!(g.state, GateState::Open);
    }

    #[test]
    fn six_waves_arm_and_pass() {
        let mut g = WaveGate::new(3);
        let mut w = 0usize;
        while w < WAVE_N {
            let _ = g.freeze_wave(w, WaveFreeze { version: 3, fingerprint: 0x10 + w as u32 });
            w += 1;
        }
        assert_eq!(g.state, GateState::Armed);
        let fp0 = g.board.get(0).map(|r| r.fingerprint).unwrap_or(0);
        assert!(g.request_close(3, fp0).is_ok());
        assert_eq!(g.state, GateState::Passed);
    }

    #[test]
    fn version_drift_rejected() {
        let mut g = WaveGate::new(4);
        assert_eq!(
            g.freeze_wave(0, WaveFreeze { version: 3, fingerprint: 1 }),
            Err(GateCode::FreezeVersionDrift)
        );
    }

    #[test]
    fn duplicate_freeze_rejected() {
        let mut g = WaveGate::new(5);
        let _ = g.freeze_wave(0, WaveFreeze { version: 5, fingerprint: 1 });
        assert_eq!(
            g.freeze_wave(0, WaveFreeze { version: 5, fingerprint: 2 }),
            Err(GateCode::DuplicateFreeze)
        );
    }

    #[test]
    fn empty_fingerprint_rejected() {
        let mut g = WaveGate::new(6);
        assert_eq!(
            g.freeze_wave(0, WaveFreeze { version: 6, fingerprint: 0 }),
            Err(GateCode::EmptyFingerprint)
        );
    }

    #[test]
    fn wave_out_of_range_rejected() {
        let mut g = WaveGate::new(7);
        assert_eq!(
            g.freeze_wave(WAVE_N, WaveFreeze { version: 7, fingerprint: 1 }),
            Err(GateCode::WaveOutOfRange)
        );
    }

    #[test]
    fn fingerprint_mismatch_on_close() {
        let mut g = WaveGate::new(8);
        let mut w = 0usize;
        while w < WAVE_N {
            let _ = g.freeze_wave(w, WaveFreeze { version: 8, fingerprint: 0x20 + w as u32 });
            w += 1;
        }
        assert_eq!(g.request_close(8, 0xDEAD), Err(GateCode::FingerprintMismatch));
    }

    #[test]
    fn injection_matrix_zero_breach() {
        let (l, per) = run_gate_injection();
        assert!(l.no_breach());
        assert_eq!(l.requests, 5 * INJECT_ROUNDS);
        for v in per.iter() {
            assert_eq!(*v, INJECT_ROUNDS);
        }
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(EBUSY));
        assert!(agrees_with_linux(EACCES));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(ENOENT), -ENOENT);
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx10018_checks_green() {
        let s = run_unx10018_checks();
        assert!(s.all_passed(), "UNX-F10018 red: {}", s.red_items_text());
    }
}
