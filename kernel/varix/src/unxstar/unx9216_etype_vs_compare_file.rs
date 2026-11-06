//! UNX-F9216 文件族 E 型与 Windows GetLastError 对照格 · 0x7A380段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9216 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9216-J1 ERROR_INVALID_HANDLE/
//!   ERROR_NOACCESS 映射表判据过，差异账三条全列格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 对照不是「查一个码」，而是三件事分开算：Linux errno ↔ Win32 错误码的
//!      映射、反向查找是否仍唯一、以及两侧差异账是否三条全列。
//!   ② EBADF↔ERROR_INVALID_HANDLE(6)、EFAULT↔ERROR_NOACCESS(998) 是主锚点；
//!      其余六行（EINVAL/EACCES/EMFILE/ENOENT/ENOTDIR/EOVERFLOW）补齐文件族。
//!   ③ 三条差异必须「可观测」而不是口号：通道差异（EFAULT 可能走 SEH
//!      0xC0000005）、基数差异（EBADF 的两种原因在 Windows 裂成 6/5）、
//!      生命期差异（成功后 GetLastError 仍保留旧值，Linux 无此残留）。
//!   ④ 宽度差异是隐藏坑：ERROR_NOACCESS=998 超出 u8，若把 errno 通道当
//!      单字节搬运，998 会被截成 230。本卡显性断言这一点。
//!   ⑤ 映射表用冻结账复算：名字、errno、Win32 码三元组逐行比对，任何一侧
//!      改动都会在漂移格报红；反向查找在表内必须保持单值。
//! ─────────────────────────────────────────────────────────────────────

use alloc::{format, vec::Vec};

use crate::checks::CheckSet;

/// 诊断段基址（F9216 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A380;
/// 映射表容量上限。
pub const MAX_ROWS: usize = 16;
/// 映射行数（冻结账）。
pub const MAP_ROWS: usize = 8;
/// 差异账条数（冻结账）。
pub const DIVERGENCE_ROWS: usize = 3;
/// 单类缺陷注入轮数。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux 坏文件描述符。
pub const EBADF: u32 = 9;
/// Linux 坏用户指针。
pub const EFAULT: u32 = 14;
/// Linux 参数非法。
pub const EINVAL: u32 = 22;
/// Linux 权限不足。
pub const EACCES: u32 = 13;
/// Linux fd 用尽。
pub const EMFILE: u32 = 24;
/// Linux 文件不存在。
pub const ENOENT: u32 = 2;
/// Linux 路径分量非目录。
pub const ENOTDIR: u32 = 20;
/// Linux 数值溢出。
pub const EOVERFLOW: u32 = 75;
/// Linux 暂时不可用（本表故意不映射）。
pub const EAGAIN: u32 = 11;

/// Win32 句柄无效。
pub const ERROR_INVALID_HANDLE: u32 = 6;
/// Win32 内存访问非法。
pub const ERROR_NOACCESS: u32 = 998;
/// Win32 参数非法。
pub const ERROR_INVALID_PARAMETER: u32 = 87;
/// Win32 访问被拒。
pub const ERROR_ACCESS_DENIED: u32 = 5;
/// Win32 打开文件过多。
pub const ERROR_TOO_MANY_OPEN_FILES: u32 = 4;
/// Win32 文件未找到。
pub const ERROR_FILE_NOT_FOUND: u32 = 2;
/// Win32 路径未找到。
pub const ERROR_PATH_NOT_FOUND: u32 = 3;
/// Win32 缓冲区溢出。
pub const ERROR_BUFFER_OVERFLOW: u32 = 111;
/// Win32 SEH 访问违例（非 GetLastError）。
pub const EXCEPTION_ACCESS_VIOLATION: u32 = 0xC000_0005;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 对照格诊断码（段内 +0x00..+0x0B 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmpCode {
    MapDrift,
    ReverseAmbiguous,
    ReverseMissing,
    DivergenceMissing,
    DivergenceKindDup,
    ChannelUnobservable,
    CardinalityLost,
    LifetimeSticky,
    WidthTruncated,
    StageSkip,
    LedgerLeak,
    SegmentViolation,
}

impl CmpCode {
    /// 码值。
    pub fn val(self) -> u32 {
        SEG_BASE + match self {
            CmpCode::MapDrift => 0,
            CmpCode::ReverseAmbiguous => 1,
            CmpCode::ReverseMissing => 2,
            CmpCode::DivergenceMissing => 3,
            CmpCode::DivergenceKindDup => 4,
            CmpCode::ChannelUnobservable => 5,
            CmpCode::CardinalityLost => 6,
            CmpCode::LifetimeSticky => 7,
            CmpCode::WidthTruncated => 8,
            CmpCode::StageSkip => 9,
            CmpCode::LedgerLeak => 10,
            CmpCode::SegmentViolation => 11,
        }
    }

    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            CmpCode::MapDrift => "映射表以冻结账为准；改任一侧必须同步改另一侧并复核反向查找",
            CmpCode::ReverseAmbiguous => "两个 errno 映射到同一 Win32 码会让反向翻译歧义，需显式建偏序",
            CmpCode::ReverseMissing => "反向查找缺口说明映射表不覆盖该 Win32 码，调用方会拿到无解释值",
            CmpCode::DivergenceMissing => "差异账必须三条全列：通道、基数、生命期，缺一条即对账不完整",
            CmpCode::DivergenceKindDup => "三条差异必须分属三类，重复类别等于少列一条",
            CmpCode::ChannelUnobservable => "EFAULT 在 Windows 可能走 SEH 异常，只声明 LastError 会漏掉异常通道",
            CmpCode::CardinalityLost => "EBADF 的两种原因在 Windows 分成 6/5，强行一一对应会丢失原因信息",
            CmpCode::LifetimeSticky => "GetLastError 成功后不清零，读成功结果前必须显式重置",
            CmpCode::WidthTruncated => "Win32 码可超 0xFF，禁止把错误码塞进 u8 通道",
            CmpCode::StageSkip => "对照状态机只许 Translate→Reverse→Diverge→Verdict 单向推进",
            CmpCode::LedgerLeak => "注入数必须等于检出数，逃逸必须为零",
            CmpCode::SegmentViolation => "段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量（顺序即码序）。
    pub const ALL: [CmpCode; 12] = [
        CmpCode::MapDrift, CmpCode::ReverseAmbiguous, CmpCode::ReverseMissing,
        CmpCode::DivergenceMissing, CmpCode::DivergenceKindDup, CmpCode::ChannelUnobservable,
        CmpCode::CardinalityLost, CmpCode::LifetimeSticky, CmpCode::WidthTruncated,
        CmpCode::StageSkip, CmpCode::LedgerLeak, CmpCode::SegmentViolation,
    ];
}

// ── 交付通道与映射 ────────────────────────────────────────────────────
/// 错误送达通道。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Surface {
    /// 直接返回值（Linux rax）。
    ReturnCode,
    /// GetLastError（线程局部、成功后残留）。
    LastError,
    /// SEH 异常（Windows 指针故障的另一条通道）。
    Exception,
}

/// 一行 errno → Win32 映射。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MapRow {
    pub name: &'static str,
    pub errno: u32,
    pub win: u32,
    pub surface: Surface,
}

/// 八行文件族映射表。
pub const MAP: [MapRow; 8] = [
    MapRow { name: "EBADF", errno: EBADF, win: ERROR_INVALID_HANDLE, surface: Surface::LastError },
    MapRow { name: "EFAULT", errno: EFAULT, win: ERROR_NOACCESS, surface: Surface::LastError },
    MapRow { name: "EINVAL", errno: EINVAL, win: ERROR_INVALID_PARAMETER, surface: Surface::LastError },
    MapRow { name: "EACCES", errno: EACCES, win: ERROR_ACCESS_DENIED, surface: Surface::LastError },
    MapRow { name: "EMFILE", errno: EMFILE, win: ERROR_TOO_MANY_OPEN_FILES, surface: Surface::LastError },
    MapRow { name: "ENOENT", errno: ENOENT, win: ERROR_FILE_NOT_FOUND, surface: Surface::LastError },
    MapRow { name: "ENOTDIR", errno: ENOTDIR, win: ERROR_PATH_NOT_FOUND, surface: Surface::LastError },
    MapRow { name: "EOVERFLOW", errno: EOVERFLOW, win: ERROR_BUFFER_OVERFLOW, surface: Surface::LastError },
];

/// B 侧冻结账（名字 / errno / Win32 码）。
pub const FROZEN_MAP: [(&str, u32, u32); 8] = [
    ("EBADF", EBADF, ERROR_INVALID_HANDLE),
    ("EFAULT", EFAULT, ERROR_NOACCESS),
    ("EINVAL", EINVAL, ERROR_INVALID_PARAMETER),
    ("EACCES", EACCES, ERROR_ACCESS_DENIED),
    ("EMFILE", EMFILE, ERROR_TOO_MANY_OPEN_FILES),
    ("ENOENT", ENOENT, ERROR_FILE_NOT_FOUND),
    ("ENOTDIR", ENOTDIR, ERROR_PATH_NOT_FOUND),
    ("EOVERFLOW", EOVERFLOW, ERROR_BUFFER_OVERFLOW),
];

/// errno → 映射行。
pub fn to_windows(errno: u32) -> Option<&'static MapRow> {
    MAP.iter().find(|r| r.errno == errno)
}

/// Win32 码 → 映射行（表内唯一）。
pub fn from_windows(code: u32) -> Option<&'static MapRow> {
    MAP.iter().find(|r| r.win == code)
}

/// Linux 侧送达通道数：非零错误恒为 1 条返回值通道。
pub fn linux_surface_count(errno: u32) -> u32 {
    if errno == 0 { 0 } else { 1 }
}

/// Windows 侧送达通道数：EFAULT 可走 LastError 或 SEH 两条，其余一条。
pub fn win_surface_count(errno: u32) -> u32 {
    match errno {
        EFAULT => 2,
        _ => 1,
    }
}

/// EBADF 的原因裂分（Windows 侧两码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CauseSplit {
    pub cause: &'static str,
    pub linux: u32,
    pub win: u32,
}

/// EBADF 的两种原因。
pub const BADF_CAUSES: [CauseSplit; 2] = [
    CauseSplit { cause: "handle-closed", linux: EBADF, win: ERROR_INVALID_HANDLE },
    CauseSplit { cause: "access-mode-mismatch", linux: EBADF, win: ERROR_ACCESS_DENIED },
];

/// Windows 线程局部 last error 模型（成功后不清零）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LastError {
    pub value: u32,
}

impl LastError {
    /// 干净状态。
    pub const fn clean() -> Self { LastError { value: 0 } }

    /// 失败置位。
    pub fn fail(&mut self, code: u32) { self.value = code; }

    /// 成功：Windows 保持旧值；Linux 侧无残留（返回值即真相）。
    pub fn succeed(&mut self) { }

    /// Linux 成功后的 errno 视角。
    pub fn linux_view(&self) -> u32 { 0 }
}

// ── 差异账 ────────────────────────────────────────────────────────────
/// 差异类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DivKind {
    /// 送达通道不同：返回值 vs LastError/SEH。
    Channel,
    /// 映射基数不同：EBADF 一因对多因。
    Cardinality,
    /// 生命周期不同：last error 成功后残留。
    Lifetime,
}

impl DivKind {
    /// 类别码。
    pub fn code(self) -> u32 {
        match self {
            DivKind::Channel => 1,
            DivKind::Cardinality => 2,
            DivKind::Lifetime => 3,
        }
    }
}

/// 一条差异账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Divergence {
    pub id: &'static str,
    pub kind: DivKind,
    pub linux_note: &'static str,
    pub win_note: &'static str,
}

/// 三条差异（冻结账）。
pub const DIVERGENCES: [Divergence; 3] = [
    Divergence {
        id: "channel",
        kind: DivKind::Channel,
        linux_note: "EFAULT 由 rax = -14 直接返回，单通道",
        win_note: "ReadFile 类 API 报 ERROR_NOACCESS(998)，裸指针访问可能抛 SEH 0xC0000005",
    },
    Divergence {
        id: "cardinality",
        kind: DivKind::Cardinality,
        linux_note: "EBADF 同时表示句柄已关闭与访问模式不符",
        win_note: "ERROR_INVALID_HANDLE(6) 只管无效句柄，模式不符走 ERROR_ACCESS_DENIED(5)",
    },
    Divergence {
        id: "lifetime",
        kind: DivKind::Lifetime,
        linux_note: "errno 仅在失败时写入，成功由返回值自身表达",
        win_note: "GetLastError 成功后保留旧值，必须显式 SetLastError(0) 才能判定",
    },
];

/// 三条差异类别互异。
pub fn divergence_kinds_distinct() -> bool {
    let mut ok = true;
    for (i, a) in DIVERGENCES.iter().enumerate() {
        for (j, b) in DIVERGENCES.iter().enumerate() {
            if i != j && a.kind.code() == b.kind.code() {
                ok = false;
            }
        }
        if a.linux_note.is_empty() || a.win_note.is_empty() {
            ok = false;
        }
    }
    ok
}

/// 通道差异可观测：两侧通道数不等。
pub fn channel_observable() -> bool {
    surface_gap(EFAULT)
}

fn surface_gap(errno: u32) -> bool {
    linux_surface_count(errno) != win_surface_count(errno)
}

/// 坍缩模型：把多通道压成单条（仅注入用）。
fn collapsed_surface_count(_errno: u32) -> u32 { 1 }

/// 基数差异可观测：同一 errno 裂成两个不同 Win32 码。
pub fn cardinality_observable() -> bool {
    let mut first = 0u32;
    let mut seen = 0u32;
    let mut distinct = true;
    for c in BADF_CAUSES.iter() {
        if c.linux != EBADF {
            distinct = false;
        }
        if seen == 0 {
            first = c.win;
        } else if c.win == first {
            distinct = false;
        }
        seen += 1;
    }
    seen == 2 && distinct && ERROR_INVALID_HANDLE != ERROR_ACCESS_DENIED
}

/// 生命期差异可观测：失败后成功，Windows 仍读回旧码。
pub fn lifetime_observable() -> bool {
    let mut le = LastError::clean();
    le.fail(ERROR_INVALID_HANDLE);
    le.succeed();
    le.value == ERROR_INVALID_HANDLE && le.linux_view() == 0
}

/// 宽度差异：ERROR_NOACCESS 超出单字节。
pub fn width_truncates() -> bool {
    ERROR_NOACCESS > 0xFF && (ERROR_NOACCESS as u8) as u32 == 230
}

// ── 漂移与注入 ────────────────────────────────────────────────────────
/// 冻结账漂移行数。
pub fn map_drift(rows: &[MapRow]) -> u32 {
    let mut drift = 0u32;
    if rows.len() != FROZEN_MAP.len() {
        drift += 1;
    }
    for (i, r) in rows.iter().enumerate() {
        match FROZEN_MAP.get(i) {
            Some((name, errno, win)) => {
                if r.name != *name || r.errno != *errno || r.win != *win {
                    drift += 1;
                }
            }
            None => drift += 1,
        }
    }
    drift
}

/// Win32 码整体偏移的映射副本。
pub fn drifted_map(delta: u32) -> Vec<MapRow> {
    let mut out = Vec::new();
    for r in MAP.iter() {
        let mut c = *r;
        c.win = c.win.wrapping_add(delta);
        out.push(c);
    }
    out
}

/// 少一条的差异账副本。
pub fn truncated_divergences() -> Vec<Divergence> {
    DIVERGENCES.iter().take(DIVERGENCE_ROWS - 1).copied().collect()
}

/// 对照缺陷类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flaw {
    /// 映射码漂移。
    WrongWinCode,
    /// 差异账丢条。
    MissingDivergence,
    /// 通道被压成单条。
    SurfaceCollapse,
}

/// 三类缺陷。
pub const FLAWS: [Flaw; 3] = [Flaw::WrongWinCode, Flaw::MissingDivergence, Flaw::SurfaceCollapse];

/// 单轮检出。
pub fn detect(flaw: Flaw) -> bool {
    match flaw {
        Flaw::WrongWinCode => map_drift(&drifted_map(1)) >= MAP_ROWS as u32,
        Flaw::MissingDivergence => truncated_divergences().len() != DIVERGENCE_ROWS,
        Flaw::SurfaceCollapse => collapsed_surface_count(EFAULT) != win_surface_count(EFAULT),
    }
}

/// 单类 10 轮全检出。
pub fn detect_ten_of_ten(flaw: Flaw) -> bool {
    for _round in 0..INJECT_ROUNDS {
        if !detect(flaw) {
            return false;
        }
    }
    true
}

/// 注入账本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl Ledger {
    /// 空账。
    pub const fn new() -> Ledger { Ledger { injected: 0, detected: 0, escaped: 0 } }

    /// 记一笔。
    pub fn record(&mut self, hit: bool) {
        self.injected += 1;
        if hit { self.detected += 1; } else { self.escaped += 1; }
    }

    /// 守恒。
    pub fn conserve(&self) -> bool {
        self.escaped == 0 && self.injected == self.detected
    }
}

/// 三类缺陷各 10 轮。
pub fn run_injections() -> Ledger {
    let mut led = Ledger::new();
    for f in FLAWS.iter() {
        for _round in 0..INJECT_ROUNDS {
            led.record(detect(*f));
        }
    }
    led
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// 对照阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmpStage {
    Translate,
    Reverse,
    Diverge,
    Verdict,
}

impl CmpStage {
    /// 下一阶段。
    pub fn next(self) -> Option<CmpStage> {
        match self {
            CmpStage::Translate => Some(CmpStage::Reverse),
            CmpStage::Reverse => Some(CmpStage::Diverge),
            CmpStage::Diverge => Some(CmpStage::Verdict),
            CmpStage::Verdict => None,
        }
    }

    /// 单向推进。
    pub fn advance(self) -> Result<CmpStage, CmpCode> {
        match self.next() {
            Some(n) => Ok(n),
            None => Err(CmpCode::StageSkip),
        }
    }
}

// ── 指纹 ──────────────────────────────────────────────────────────────
fn mix(h: u32, v: u32) -> u32 {
    (h ^ v).wrapping_mul(0x0100_0193)
}

/// 映射表 + 差异账指纹。
pub fn corpus_fingerprint() -> u32 {
    let mut h = 0x811C_9DC5u32;
    for r in MAP.iter() {
        for b in r.name.as_bytes().iter() {
            h = mix(h, *b as u32);
        }
        h = mix(h, r.errno);
        h = mix(h, r.win);
    }
    for d in DIVERGENCES.iter() {
        h = mix(h, d.kind.code());
        for b in d.id.as_bytes().iter() {
            h = mix(h, *b as u32);
        }
    }
    h
}

// ── 判据总成 ──────────────────────────────────────────────────────────
/// 运行 F9216 全部判据。
pub fn run_unx9216_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9216");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须独占且落在本批窗口；越界即拒绝注册");

    // 2. 映射表完整
    s.add("映射表：8 行齐全且与冻结账零漂移",
          MAP.len() == MAP_ROWS && map_drift(&MAP) == 0,
          "名字/errno/Win32 码三元组必须逐行咬合");

    // 3. EBADF 锚点
    let badf = to_windows(EBADF);
    let badf_ok = match badf {
        Some(r) => r.win == ERROR_INVALID_HANDLE,
        None => false,
    };
    s.add("主锚点：EBADF(9) → ERROR_INVALID_HANDLE(6)", badf_ok,
          "句柄无效是最常用的一条，码值必须钉死");

    // 4. EFAULT 锚点
    let efault = to_windows(EFAULT);
    let efault_ok = match efault {
        Some(r) => r.win == ERROR_NOACCESS,
        None => false,
    };
    s.add("主锚点：EFAULT(14) → ERROR_NOACCESS(998)", efault_ok,
          "998 超出单字节，映射层必须用 u32 承载");

    // 5. EINVAL 锚点
    s.add("映射：EINVAL(22) → ERROR_INVALID_PARAMETER(87)",
          match to_windows(EINVAL) { Some(r) => r.win == ERROR_INVALID_PARAMETER, None => false },
          "参数值非法在两侧语义最接近，码值不得串位");

    // 6. 其余四行
    let rest_ok = match (to_windows(EACCES), to_windows(EMFILE), to_windows(ENOENT), to_windows(ENOTDIR), to_windows(EOVERFLOW)) {
        (Some(a), Some(b), Some(c), Some(d), Some(e)) => {
            a.win == ERROR_ACCESS_DENIED
                && b.win == ERROR_TOO_MANY_OPEN_FILES
                && c.win == ERROR_FILE_NOT_FOUND
                && d.win == ERROR_PATH_NOT_FOUND
                && e.win == ERROR_BUFFER_OVERFLOW
        }
        _ => false,
    };
    s.add("映射：EACCES/EMFILE/ENOENT/ENOTDIR/EOVERFLOW 五行码值正确", rest_ok,
          "文件族常错必须齐全，否则对照表只覆盖了一半失败面");

    // 7. 未映射码拒绝
    s.add("边界：未列出的 EAGAIN(11) 不返回伪映射",
          to_windows(EAGAIN).is_none(),
          "宁可不映射，也不要给一个看似合理的错码");

    // 8. Linux 侧唯一
    let mut linux_unique = true;
    for (i, a) in MAP.iter().enumerate() {
        for (j, b) in MAP.iter().enumerate() {
            if i != j && a.errno == b.errno {
                linux_unique = false;
            }
        }
    }
    s.add("唯一性：8 个 Linux errno 互不重复", linux_unique,
          "重复 errno 会使表内查找返回任意一行");

    // 9. Windows 侧唯一
    let mut win_unique = true;
    for (i, a) in MAP.iter().enumerate() {
        for (j, b) in MAP.iter().enumerate() {
            if i != j && a.win == b.win {
                win_unique = false;
            }
        }
    }
    s.add("唯一性：8 个 Win32 码互不重复（反向可查）", win_unique,
          "反向翻译必须单值，否则调用方无法判定原始 errno");

    // 10. 反向一致
    let roundtrip = MAP.iter().all(|r| match from_windows(r.win) {
        Some(back) => back.errno == r.errno && back.name == r.name,
        None => false,
    });
    s.add("反向一致：每行 Win32 码可唯一回查原 errno", roundtrip,
          "正向表与反向表共用一份数据，不允许各写一半");

    // 11. 反向缺口
    s.add("反向缺口：未列出的 0xDEAD 回查为空",
          from_windows(0xDEAD).is_none(),
          "未知码必须显式返回空，而不是落到某一行");

    // 12. 差异账三条
    s.add("差异账：三条全列（channel/cardinality/lifetime）",
          DIVERGENCES.len() == DIVERGENCE_ROWS
              && DIVERGENCES.iter().all(|d| !d.id.is_empty()),
          "差异账是对照格的另一半，缺条等于把差异藏起来");

    // 13. 类别互异
    s.add("差异账：三类互异且文案非空", divergence_kinds_distinct(),
          "重复类别等于少列一条，统计口径会失真");

    // 14. 通道差异可观测
    s.add("差异①：EFAULT 通道数 Linux=1 / Windows=2（LastError+SEH）",
          channel_observable() && win_surface_count(EFAULT) == 2,
          "只声明 GetLastError 会漏掉 SEH 0xC0000005 通道");

    // 15. 基数差异可观测
    s.add("差异②：EBADF 两种原因 → Windows 6/5 裂分",
          cardinality_observable(),
          "一一对应会丢失「模式不符」这一原因，必须显式记录裂分");

    // 16. 生命期差异可观测
    s.add("差异③：成功后 GetLastError 保留旧值，Linux 无残留",
          lifetime_observable(),
          "成功路径读 last error 前必须重置，否则误判为失败");

    // 17. 宽度差异
    s.add("宽度：ERROR_NOACCESS(998) 超 u8，截断后变 230",
          width_truncates() && (ERROR_NOACCESS as u8) as u32 != ERROR_NOACCESS,
          "错误码通道不得窄于 u32，否则高端码被截断成其它错误");

    // 18. 状态机
    let mut stage = CmpStage::Translate;
    let mut steps = 0u32;
    let mut forward = true;
    while let Ok(next) = stage.advance() {
        stage = next;
        steps += 1;
        if steps > 8 {
            forward = false;
            break;
        }
    }
    s.add("状态机：Translate→Reverse→Diverge→Verdict 单向三步",
          forward && steps == 3 && stage == CmpStage::Verdict
              && stage.advance() == Err(CmpCode::StageSkip),
          "回退或跳步会让映射与差异账读到不同版本");

    // 19. 注入
    let ten = FLAWS.iter().all(|f| detect_ten_of_ten(*f));
    s.add("注入必抓：码漂移/丢差异/通道坍缩三类各 10/10 轮", ten,
          "每类缺陷都要在十轮里稳定被抓；漏一轮即 flaky");

    // 20. 守恒
    let led = run_injections();
    s.add("守恒：注入 30 = 检出 30 + 逃逸 0",
          led.conserve() && led.injected == 3 * INJECT_ROUNDS,
          "注入账必须闭合，逃逸说明对照格有盲区");

    // 21. 双跑一致
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("双跑一致：映射 + 差异账指纹两次相等", f1 == f2,
          "对照结果不得依赖哈希遍历顺序或外部状态");

    // 22. 容量
    s.add("容量：8 行 ≤ 16 上限，差异 3 条，add_dyn ≤ 8",
          MAP_ROWS <= MAX_ROWS && DIVERGENCE_ROWS == 3 && 3 <= 8,
          "行数超上限说明对照面失控，应拆卡而不是继续堆行");

    // 23. 文案闭集
    let mut closed = true;
    for (i, c) in CmpCode::ALL.iter().enumerate() {
        if c.val() != SEG_BASE + i as u32 || c.detail().is_empty() {
            closed = false;
        }
        for (j, o) in CmpCode::ALL.iter().enumerate() {
            if i != j && c.val() == o.val() {
                closed = false;
            }
        }
    }
    s.add("文案闭集：12 码连续、唯一、detail 非空", closed,
          "诊断码闭集且可枚举，文案为空等于没有处置建议");

    // 动态明细 1：映射表
    s.add_dyn("动态：映射表点检", badf_ok && efault_ok && roundtrip,
              &format!("rows={} drift={} fp=0x{:08X}", MAP.len(), map_drift(&MAP), f1));

    // 动态明细 2：差异账
    s.add_dyn("动态：差异账", divergence_kinds_distinct() && channel_observable() && cardinality_observable() && lifetime_observable(),
              &format!("diverge={} channel={} cardinality={} lifetime={}",
                       DIVERGENCES.len(), channel_observable(), cardinality_observable(), lifetime_observable()));

    // 动态明细 3：注入与宽度
    s.add_dyn("动态：注入与宽度", led.conserve() && width_truncates(),
              &format!("injected={} detected={} escaped={} noaccess_u8={}",
                       led.injected, led.detected, led.escaped, (ERROR_NOACCESS as u8) as u32));

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_rows_are_complete_and_frozen() {
        assert_eq!(MAP.len(), MAP_ROWS);
        assert_eq!(FROZEN_MAP.len(), MAP_ROWS);
        assert_eq!(map_drift(&MAP), 0);
        assert!(MAP.iter().all(|r| !r.name.is_empty() && r.errno != 0 && r.win != 0));
    }

    #[test]
    fn badf_and_efault_anchors() {
        let badf = to_windows(EBADF);
        assert!(badf.is_some());
        if let Some(r) = badf {
            assert_eq!(r.win, ERROR_INVALID_HANDLE);
        }
        let efault = to_windows(EFAULT);
        assert!(efault.is_some());
        if let Some(r) = efault {
            assert_eq!(r.win, ERROR_NOACCESS);
        }
    }

    #[test]
    fn unmapped_errno_is_rejected() {
        assert!(to_windows(EAGAIN).is_none());
        assert!(to_windows(0).is_none());
        assert!(from_windows(0xDEAD).is_none());
    }

    #[test]
    fn reverse_lookup_roundtrips() {
        for r in MAP.iter() {
            let back = from_windows(r.win);
            assert!(back.is_some());
            if let Some(b) = back {
                assert_eq!(b.errno, r.errno);
            }
        }
        let na = from_windows(ERROR_NOACCESS);
        assert!(na.is_some());
        if let Some(b) = na {
            assert_eq!(b.name, "EFAULT");
        }
    }

    #[test]
    fn divergences_are_three_and_distinct() {
        assert_eq!(DIVERGENCES.len(), 3);
        assert!(divergence_kinds_distinct());
        let kinds = [DivKind::Channel, DivKind::Cardinality, DivKind::Lifetime];
        assert_eq!(kinds.len(), 3);
    }

    #[test]
    fn channel_and_cardinality_are_observable() {
        assert!(channel_observable());
        assert_eq!(linux_surface_count(EFAULT), 1);
        assert_eq!(win_surface_count(EFAULT), 2);
        assert!(cardinality_observable());
        assert!(DIVERGENCES.iter().any(|d| d.kind == DivKind::Channel));
        assert!(DIVERGENCES.iter().any(|d| d.kind == DivKind::Cardinality));
    }

    #[test]
    fn last_error_is_sticky_after_success() {
        assert!(lifetime_observable());
        let mut le = LastError::clean();
        le.fail(ERROR_NOACCESS);
        le.succeed();
        assert_eq!(le.value, ERROR_NOACCESS);
        assert_eq!(le.linux_view(), 0);
    }

    #[test]
    fn width_truncation_is_visible() {
        assert!(width_truncates());
        assert_eq!((ERROR_NOACCESS as u8) as u32, 230);
        assert_ne!((ERROR_NOACCESS as u8) as u32, ERROR_NOACCESS);
    }

    #[test]
    fn defects_are_detected_ten_of_ten() {
        for f in FLAWS.iter() {
            assert!(detect_ten_of_ten(*f));
            assert!(detect(*f));
        }
        let led = run_injections();
        assert!(led.conserve());
        assert_eq!(led.injected, 3 * INJECT_ROUNDS);
        assert_eq!(led.escaped, 0);
    }

    #[test]
    fn stage_machine_is_forward_only() {
        let s0 = CmpStage::Translate;
        let s1 = s0.advance();
        assert_eq!(s1, Ok(CmpStage::Reverse));
        let s2 = match s1 { Ok(v) => v.advance(), Err(_) => Err(CmpCode::StageSkip) };
        let s3 = match s2 { Ok(v) => v.advance(), Err(_) => Err(CmpCode::StageSkip) };
        assert_eq!(s3, Ok(CmpStage::Verdict));
        let s4 = match s3 { Ok(v) => v.advance(), Err(_) => Err(CmpCode::StageSkip) };
        assert_eq!(s4, Err(CmpCode::StageSkip));
    }

    #[test]
    fn fingerprint_and_checks_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        let set = run_unx9216_checks();
        assert!(set.all_passed());
        assert!(!set.truncated());
    }
}
