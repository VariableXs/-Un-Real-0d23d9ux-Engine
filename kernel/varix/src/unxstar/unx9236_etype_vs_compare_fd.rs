//! UNX-F9236 fd 族 E 型与 Windows INVALID_HANDLE_VALUE 对照格 · 0x7A880段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9236 逐字）：
//!   ① 纯功能代码 ≥300 行；② 判据原文：UNX-F9236-J1 INVALID_HANDLE_VALUE
//!   映射判据过，WSA 错误差异账三条全列格过；③ 完成时 API/软件写回：
//!   状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 哨兵不是数字：INVALID_HANDLE_VALUE 是 (HANDLE)-1，即 64 位全 1；
//!      NULL 句柄是 0，二者都必须判无效但语义不同——0 是「空」，全 1 是
//!      「坏」。把 0 当坏或把全 1 当空，都会在关闭路径上重复释放。
//!   ② 伪句柄撞车：GetCurrentProcess 返回的伪句柄恰好也等于全 1，与坏句柄
//!      同值不同义；对照格必须显式记录这条，不能靠数值相等做有效性判断。
//!   ③ fd 是 32 位小整数，HANDLE 是指针宽度不透明值。零扩展后 32 位 fd 永远
//!      触不到全 1 哨兵；但若把有符号 -1 强转宽整型，就会精确撞上哨兵。
//!   ④ Win32 错误码宽度不一：ERROR_NOACCESS=998、ERROR_RETRY=1237 都放不进
//!      8 位，任何按 u8 落库的对照表都会截断成 230/213，必须按 16 位以上存。
//!   ⑤ WSA 错误账三条全列：通道差异（WSA 码只走 WSAGetLastError）、基数差异
//!      （10000 段不与 Win32 1..1999 冲突）、生命期差异（每线程粘滞，成功不
//!      清）。缺一条即差异账不完整。
//!   ⑥ LastError 粘滞：成功调用不清零，读取方必须显式 ClearLastError 或等下
//!      一次失败覆盖；errno 只在失败时写，二者时序不同，不可混读。
//! ─────────────────────────────────────────────────────────────────────
use alloc::{format, vec::Vec};
use crate::checks::CheckSet;
/// 诊断段基址（F9236 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A880;
/// 对照行容量上限。
pub const MAX_MAP: usize = 16;
/// 错误码对照行数。
pub const MAP_ROWS: usize = 8;
/// 坏句柄成因总数。
pub const BADF_TOTAL: usize = 11;
/// Linux 侧坏 fd 成因数。
pub const BADF_LINUX: usize = 6;
/// Windows 侧坏句柄成因数。
pub const BADF_WIN: usize = 5;
/// WSA 差异账条数。
pub const DIVERGENCE_COUNT: usize = 3;
/// 单类检出轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 无效句柄哨兵：(HANDLE)-1，64 位全 1。
pub const INVALID_HANDLE_VALUE: u64 = u64::MAX;
/// 空句柄。
pub const NULL_HANDLE: u64 = 0;
/// 伪句柄：当前进程，恰与坏句柄同值。
pub const PSEUDO_PROCESS: u64 = u64::MAX;
/// 伪句柄：当前线程。
pub const PSEUDO_THREAD: u64 = u64::MAX - 1;
/// ERROR_SUCCESS。
pub const ERROR_SUCCESS: u32 = 0;
/// ERROR_INVALID_HANDLE。
pub const ERROR_INVALID_HANDLE: u32 = 6;
/// ERROR_NOACCESS（坏指针）。
pub const ERROR_NOACCESS: u32 = 998;
/// ERROR_INVALID_PARAMETER。
pub const ERROR_INVALID_PARAMETER: u32 = 87;
/// ERROR_TOO_MANY_OPEN_FILES。
pub const ERROR_TOO_MANY_OPEN_FILES: u32 = 4;
/// ERROR_RETRY。
pub const ERROR_RETRY: u32 = 1237;
/// WSAEFAULT。
pub const WSAEFAULT: u32 = 10014;
/// WSAEINVAL。
pub const WSAEINVAL: u32 = 10022;
/// WSAEMFILE。
pub const WSAEMFILE: u32 = 10024;
/// WSAEWOULDBLOCK。
pub const WSAEWOULDBLOCK: u32 = 10035;
/// WSAENOTSOCK。
pub const WSAENOTSOCK: u32 = 10038;
/// 平台面。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Linux,
    Win32,
    Winsock,
}
impl Surface {
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Surface::Linux => 0,
            Surface::Win32 => 1,
            Surface::Winsock => 2,
        }
    }
    /// 面名。
    pub fn name(self) -> &'static str {
        match self {
            Surface::Linux => "Linux errno",
            Surface::Win32 => "Win32 GetLastError",
            Surface::Winsock => "Winsock WSAGetLastError",
        }
    }
    /// 三面全量。
    pub const ALL: [Surface; 3] = [Surface::Linux, Surface::Win32, Surface::Winsock];
}
/// 错误类别。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Class {
    Basic,
    Pointer,
    Limit,
    Path,
    Seek,
    Pipe,
    Sharing,
}
impl Class {
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Class::Basic => 0,
            Class::Pointer => 1,
            Class::Limit => 2,
            Class::Path => 3,
            Class::Seek => 4,
            Class::Pipe => 5,
            Class::Sharing => 6,
        }
    }
}
/// 一行对照：linux errno、Win32 码、WSA 码（0 表示无对应）。
#[derive(Clone, Copy)]
pub struct MapRow {
    pub linux: u32,
    pub win: u32,
    pub ws: u32,
    pub class: Class,
}
/// 八行对照表。
pub const MAP: [MapRow; 8] = [
    MapRow { linux: 9, win: ERROR_INVALID_HANDLE, ws: WSAENOTSOCK, class: Class::Basic },
    MapRow { linux: 14, win: ERROR_NOACCESS, ws: WSAEFAULT, class: Class::Pointer },
    MapRow { linux: 22, win: ERROR_INVALID_PARAMETER, ws: WSAEINVAL, class: Class::Basic },
    MapRow { linux: 24, win: ERROR_TOO_MANY_OPEN_FILES, ws: WSAEMFILE, class: Class::Limit },
    MapRow { linux: 2, win: 2, ws: 0, class: Class::Path },
    MapRow { linux: 29, win: 132, ws: 0, class: Class::Seek },
    MapRow { linux: 32, win: 109, ws: 0, class: Class::Pipe },
    MapRow { linux: 16, win: 32, ws: 0, class: Class::Sharing },
];
/// 冻结的 Win32 码（逐行）。
pub const FROZEN_MAP: [u32; 8] = [
    ERROR_INVALID_HANDLE,
    ERROR_NOACCESS,
    ERROR_INVALID_PARAMETER,
    ERROR_TOO_MANY_OPEN_FILES,
    2,
    132,
    109,
    32,
];
/// Linux errno → Win32 码。
pub fn to_windows(e: u32) -> u32 {
    for r in MAP.iter() {
        if r.linux == e {
            return r.win;
        }
    }
    ERROR_SUCCESS
}
/// Win32 码 → Linux errno。
pub fn from_windows(w: u32) -> u32 {
    for r in MAP.iter() {
        if r.win == w {
            return r.linux;
        }
    }
    0
}
/// Linux errno → WSA 码（0 表示无对应）。
pub fn to_wsa(e: u32) -> u32 {
    for r in MAP.iter() {
        if r.linux == e {
            return r.ws;
        }
    }
    0
}
/// 对照行是否自洽：三轮往返相等。
pub fn round_trip_ok(r: &MapRow) -> bool {
    to_windows(r.linux) == r.win && from_windows(r.win) == r.linux && to_wsa(r.linux) == r.ws
}
/// 全表往返。
pub fn all_round_trip() -> bool {
    MAP.iter().all(|r| round_trip_ok(r))
}
/// 有 WSA 对应的行数。
pub fn wsa_rows() -> usize {
    MAP.iter().filter(|r| r.ws != 0).count()
}
/// 冻结漂移计数。
pub fn map_drift_of(rows: &[MapRow]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < rows.len() && i < FROZEN_MAP.len() {
        if rows[i].win != FROZEN_MAP[i] {
            n += 1;
        }
        i += 1;
    }
    n
}
/// 当前表漂移。
pub fn map_drift() -> usize {
    map_drift_of(&MAP)
}
/// 故意改坏第 2 行的 Win32 码。
pub fn drifted_map() -> Vec<MapRow> {
    let mut v: Vec<MapRow> = MAP.iter().copied().collect();
    if let Some(r) = v.get_mut(2) {
        r.win = 87 + 1;
    }
    v
}
/// 句柄载入：32 位 fd 零扩展。
pub fn handle_from_fd(fd: u32) -> u64 {
    fd as u64
}
/// 有符号 fd 宽化（危险路径）。
pub fn handle_from_signed_fd(fd: i64) -> u64 {
    fd as u64
}
/// 句柄有效性：既非空也非坏。
pub fn handle_valid(h: u64) -> bool {
    h != NULL_HANDLE && h != INVALID_HANDLE_VALUE
}
/// 8 位可容纳。
pub fn fits_u8(v: u32) -> bool {
    v <= 0xFF
}
/// 16 位可容纳。
pub fn fits_u16(v: u32) -> bool {
    v <= 0xFFFF
}
/// 按 8 位截断。
pub fn truncate_u8(v: u32) -> u8 {
    (v & 0xFF) as u8
}
/// 截断后是否失真。
pub fn width_truncates(v: u32) -> bool {
    truncate_u8(v) as u32 != v
}
/// 坏句柄成因。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    LinuxClosed,
    LinuxReused,
    LinuxNegative,
    LinuxBeyondLimit,
    LinuxWrongMode,
    LinuxDirAsFile,
    WinInvalidSentinel,
    WinNullHandle,
    WinClosedHandle,
    WinPseudoMisuse,
    WinForeignHandle,
}
impl Cause {
    /// 所属面。
    pub fn surface(self) -> Surface {
        match self {
            Cause::LinuxClosed
            | Cause::LinuxReused
            | Cause::LinuxNegative
            | Cause::LinuxBeyondLimit
            | Cause::LinuxWrongMode
            | Cause::LinuxDirAsFile => Surface::Linux,
            _ => Surface::Win32,
        }
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Cause::LinuxClosed => 0,
            Cause::LinuxReused => 1,
            Cause::LinuxNegative => 2,
            Cause::LinuxBeyondLimit => 3,
            Cause::LinuxWrongMode => 4,
            Cause::LinuxDirAsFile => 5,
            Cause::WinInvalidSentinel => 6,
            Cause::WinNullHandle => 7,
            Cause::WinClosedHandle => 8,
            Cause::WinPseudoMisuse => 9,
            Cause::WinForeignHandle => 10,
        }
    }
}
/// 11 条成因。
pub const BADF_CAUSES: [Cause; 11] = [
    Cause::LinuxClosed,
    Cause::LinuxReused,
    Cause::LinuxNegative,
    Cause::LinuxBeyondLimit,
    Cause::LinuxWrongMode,
    Cause::LinuxDirAsFile,
    Cause::WinInvalidSentinel,
    Cause::WinNullHandle,
    Cause::WinClosedHandle,
    Cause::WinPseudoMisuse,
    Cause::WinForeignHandle,
];
/// 某面成因计数。
pub fn causes_of(s: Surface) -> usize {
    BADF_CAUSES.iter().filter(|c| c.surface() == s).count()
}
/// LastError 槽（粘滞）。
pub struct LastError {
    value: u32,
}
impl LastError {
    /// 清零起步。
    pub fn new() -> Self {
        LastError { value: ERROR_SUCCESS }
    }
    /// 失败写入。
    pub fn set(&mut self, e: u32) {
        self.value = e;
    }
    /// 显式清除。
    pub fn clear(&mut self) {
        self.value = ERROR_SUCCESS;
    }
    /// 读取。
    pub fn get(&self) -> u32 {
        self.value
    }
    /// 成功调用：不清零。
    pub fn success(&mut self) {
        // 故意不写 value：Windows 语义即如此。
    }
}
/// errno 槽（失败才写，成功保持）。
pub struct ErrnoCell {
    value: u32,
}
impl ErrnoCell {
    /// 起步为 0。
    pub fn new() -> Self {
        ErrnoCell { value: 0 }
    }
    /// 失败写入。
    pub fn fail(&mut self, e: u32) {
        self.value = e;
    }
    /// 成功：errno 一般不改写。
    pub fn ok(&mut self) {
        // 保持原值，与 C 库实践一致（不保证清零）。
    }
    /// 读取。
    pub fn get(&self) -> u32 {
        self.value
    }
}
/// 错误通道。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Errno,
    LastError,
    WsaLastError,
}
impl Channel {
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Channel::Errno => 0,
            Channel::LastError => 1,
            Channel::WsaLastError => 2,
        }
    }
}
/// 面到通道。
pub fn channel_for(s: Surface) -> Channel {
    match s {
        Surface::Linux => Channel::Errno,
        Surface::Win32 => Channel::LastError,
        Surface::Winsock => Channel::WsaLastError,
    }
}
/// 通道判定是否与面一致。
pub fn channel_ok(c: Channel, s: Surface) -> bool {
    c == channel_for(s)
}
/// 差异种类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DivKind {
    Channel,
    Cardinality,
    Lifetime,
}
impl DivKind {
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            DivKind::Channel => 0,
            DivKind::Cardinality => 1,
            DivKind::Lifetime => 2,
        }
    }
    /// 中文说明。
    pub fn detail(self) -> &'static str {
        match self {
            DivKind::Channel => "WSA 码只走 WSAGetLastError，读 GetLastError 得到陈旧值",
            DivKind::Cardinality => "fd 是小整数、HANDLE 是指针宽度不透明值，不可比大小",
            DivKind::Lifetime => "LastError 粘滞且每线程独立，成功不清零",
        }
    }
}
/// 一条差异账。
#[derive(Clone, Copy)]
pub struct Divergence {
    pub kind: DivKind,
    pub surface: Surface,
    pub linux_side: &'static str,
    pub win_side: &'static str,
}
/// 三条差异账全列。
pub const DIVERGENCES: [Divergence; 3] = [
    Divergence {
        kind: DivKind::Channel,
        surface: Surface::Winsock,
        linux_side: "errno 单通道",
        win_side: "WSA 码只走 WSAGetLastError",
    },
    Divergence {
        kind: DivKind::Cardinality,
        surface: Surface::Win32,
        linux_side: "fd 为 0..1023 小整数",
        win_side: "HANDLE 为 64 位不透明值，全 1 为坏",
    },
    Divergence {
        kind: DivKind::Lifetime,
        surface: Surface::Win32,
        linux_side: "errno 仅失败时写",
        win_side: "LastError 粘滞到下次写或显式清",
    },
];
/// 差异种类去重计数。
pub fn divergence_kinds() -> usize {
    let mut seen = [false; 3];
    for d in DIVERGENCES.iter() {
        seen[d.kind.tag() as usize] = true;
    }
    seen.iter().filter(|x| **x).count()
}
/// 取前 n 条（用于「三条全列」判据）。
pub fn truncated_divergences(n: usize) -> Vec<Divergence> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < n && i < DIVERGENCES.len() {
        v.push(DIVERGENCES[i]);
        i += 1;
    }
    v
}
/// 差异账完整性。
pub fn divergences_complete(n: usize) -> bool {
    n == DIVERGENCE_COUNT && divergence_kinds() == DIVERGENCE_COUNT
}
/// 缺陷种类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Flaw {
    WrongWinCode,
    MissingDivergence,
    WsaCollapse,
}
impl Flaw {
    /// 三种缺陷。
    pub fn all() -> [Flaw; 3] {
        [Flaw::WrongWinCode, Flaw::MissingDivergence, Flaw::WsaCollapse]
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            Flaw::WrongWinCode => 0,
            Flaw::MissingDivergence => 1,
            Flaw::WsaCollapse => 2,
        }
    }
}
/// 缺陷是否被对照格抓住。
pub fn flaw_detected(f: Flaw) -> bool {
    match f {
        Flaw::WrongWinCode => map_drift_of(&drifted_map()) == 1,
        Flaw::MissingDivergence => !divergences_complete(2),
        Flaw::WsaCollapse => !channel_ok(Channel::LastError, Surface::Winsock),
    }
}
/// 缺陷注入账。
pub struct FlawLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
    pub rounds: Vec<(u8, u32)>,
}

impl FlawLedger {
    /// 空账。
    pub fn new() -> Self {
        FlawLedger { injected: 0, detected: 0, escaped: 0, rounds: Vec::new() }
    }

    /// 记一轮。
    pub fn record(&mut self, tag: u8, hit: bool) {
        self.injected += 1;
        if hit {
            self.detected += 1;
        } else {
            self.escaped += 1;
        }
        self.rounds.push((tag, if hit { 1 } else { 0 }));
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped
    }
}

/// 3 类 × 10 轮缺陷注入。
pub fn run_flaws() -> FlawLedger {
    let mut l = FlawLedger::new();
    let mut r = 0;
    while r < INJECT_ROUNDS {
        for f in Flaw::all().iter() {
            l.record(f.tag() as u8, flaw_detected(*f));
        }
        r += 1;
    }
    l
}

/// 每类检出数。
pub fn per_flaw(l: &FlawLedger) -> [u32; 3] {
    let mut out = [0u32; 3];
    for pair in l.rounds.iter() {
        let (tag, hit) = *pair;
        out[(tag as usize) % 3] += hit;
    }
    out
}

/// 对照状态机。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CmpStage {
    Idle,
    Mapped,
    Compared,
    Declared,
    Sealed,
}
impl CmpStage {
    /// 单步前进。
    pub fn can_go(self, next: CmpStage) -> bool {
        matches!(
            (self, next),
            (CmpStage::Idle, CmpStage::Mapped)
                | (CmpStage::Mapped, CmpStage::Compared)
                | (CmpStage::Compared, CmpStage::Declared)
                | (CmpStage::Declared, CmpStage::Sealed)
        )
    }
    /// 稳定标签。
    pub fn tag(self) -> u32 {
        match self {
            CmpStage::Idle => 0,
            CmpStage::Mapped => 1,
            CmpStage::Compared => 2,
            CmpStage::Declared => 3,
            CmpStage::Sealed => 4,
        }
    }
}
/// 诊断码闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CmpCode {
    MapMissing,
    MapDrift,
    SentinelBroken,
    NullTreatedInvalid,
    PseudoCollision,
    CauseCardinality,
    ChannelCollapsed,
    LifetimeCollapsed,
    WidthTruncated,
    DivergenceMissing,
    StageIllegal,
    SegmentOutOfRange,
}
impl CmpCode {
    /// 码值 = 段基址 + 序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            CmpCode::MapMissing => "对照行缺失：八行必须齐全后再生效",
            CmpCode::MapDrift => "Win32 码漂移：按冻结表逐行回填",
            CmpCode::SentinelBroken => "坏句柄哨兵被改：必须保持 64 位全 1",
            CmpCode::NullTreatedInvalid => "空句柄被判坏：0 是空不是坏，语义不可合并",
            CmpCode::PseudoCollision => "伪句柄撞车未记：全 1 同时是当前进程句柄",
            CmpCode::CauseCardinality => "坏句柄成因数错：应为 11 条且 Linux 6 Windows 5",
            CmpCode::ChannelCollapsed => "错误通道塌缩：WSA 面必须读 WSA 通道",
            CmpCode::LifetimeCollapsed => "生命期塌缩：LastError 粘滞、成功不清零",
            CmpCode::WidthTruncated => "错误码被 u8 截断：998/1237 须按 16 位存",
            CmpCode::DivergenceMissing => "差异账不足三条：通道/基数/生命期缺一不可",
            CmpCode::StageIllegal => "阶段跳变：对照只许单步前进",
            CmpCode::SegmentOutOfRange => "码值越出 0x7A880 窗口：段位守卫拒绝",
        }
    }
    /// 全量闭集。
    pub const ALL: [CmpCode; 12] = [
        CmpCode::MapMissing,
        CmpCode::MapDrift,
        CmpCode::SentinelBroken,
        CmpCode::NullTreatedInvalid,
        CmpCode::PseudoCollision,
        CmpCode::CauseCardinality,
        CmpCode::ChannelCollapsed,
        CmpCode::LifetimeCollapsed,
        CmpCode::WidthTruncated,
        CmpCode::DivergenceMissing,
        CmpCode::StageIllegal,
        CmpCode::SegmentOutOfRange,
    ];
}
/// FNV-1a 32 位。
pub fn fnv1a(seed: u32, bytes: &[u8]) -> u32 {
    let mut h = seed;
    for b in bytes.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}
/// 对照表指纹。
pub fn map_digest() -> u32 {
    let mut h = fnv1a(0x811C_9DC5, b"unx9236/map");
    for r in MAP.iter() {
        h = fnv1a(h, &r.linux.to_le_bytes());
        h = fnv1a(h, &r.win.to_le_bytes());
        h = fnv1a(h, &r.ws.to_le_bytes());
    }
    h
}
/// 差异账指纹。
pub fn divergence_digest() -> u32 {
    let mut h = fnv1a(0x811C_9DC5, b"unx9236/div");
    for d in DIVERGENCES.iter() {
        h = fnv1a(h, &[d.kind.tag() as u8, d.surface.tag() as u8]);
        h = fnv1a(h, d.linux_side.as_bytes());
        h = fnv1a(h, d.win_side.as_bytes());
    }
    h
}
/// 跑批指纹。
pub fn run_digest() -> u32 {
    let mut h = fnv1a(map_digest(), b"run");
    h = fnv1a(h, &divergence_digest().to_le_bytes());
    h = fnv1a(h, &(causes_of(Surface::Linux) as u32).to_le_bytes());
    h = fnv1a(h, &(causes_of(Surface::Win32) as u32).to_le_bytes());
    h = fnv1a(h, &(wsa_rows() as u32).to_le_bytes());
    h
}
/// 文案闭集自检。
pub fn text_ok() -> bool {
    if CmpCode::ALL.len() != 12 {
        return false;
    }
    let mut i = 0;
    while i < CmpCode::ALL.len() {
        let a = CmpCode::ALL[i];
        if a.detail().is_empty() {
            return false;
        }
        let mut j = i + 1;
        while j < CmpCode::ALL.len() {
            let b = CmpCode::ALL[j];
            if a.detail() == b.detail() || a.val() == b.val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}
/// WSA 码段是否与 Win32 段不重叠。
pub fn wsa_namespace_separate() -> bool {
    MAP.iter().all(|r| r.ws == 0 || r.ws >= 10_000) && MAP.iter().all(|r| r.win < 2_000)
}
/// 自检总成。
pub fn run_unx9236_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9236");
    s.add("seg_base", SEG_BASE == 0x7A880, "段基址应为 0x7A880");
    s.add("code_window", CmpCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 0x40), "码值落在 0x7A880..0x7A8C0");
    s.add("text_closed_set", text_ok(), "12 条闭集、值唯一、建议非空");
    s.add("sentinel", INVALID_HANDLE_VALUE == u64::MAX && INVALID_HANDLE_VALUE == !0u64, "坏句柄为 64 位全 1");
    s.add("null_distinct", NULL_HANDLE != INVALID_HANDLE_VALUE && !handle_valid(NULL_HANDLE), "空句柄 0 与坏句柄不同值");
    s.add("bad_check", !handle_valid(INVALID_HANDLE_VALUE) && handle_valid(1) && handle_valid(handle_from_fd(63)), "有效性判定区分空/坏/正常");
    s.add("pseudo_collision", PSEUDO_PROCESS == INVALID_HANDLE_VALUE && PSEUDO_THREAD != INVALID_HANDLE_VALUE, "当前进程伪句柄与坏句柄同值");
    s.add("fd_widen_safe", (0..64u32).all(|fd| handle_from_fd(fd) != INVALID_HANDLE_VALUE), "32 位 fd 零扩展不触哨兵");
    s.add("neg_one_collides", handle_from_signed_fd(-1) == INVALID_HANDLE_VALUE, "有符号 -1 宽化后撞哨兵");
    s.add("map_rows", MAP.len() == MAP_ROWS, "对照表应为 8 行");
    s.add("map_roundtrip", all_round_trip(), "八行三轮往返一致");
    s.add("map_frozen", map_drift() == 0, "Win32 码与冻结表一致");
    s.add("map_capacity", MAP.len() <= MAX_MAP, "对照行不超过 16");
    s.add("wsa_rows", wsa_rows() == 4, "有 WSA 对应者四行（EBADF/EFAULT/EINVAL/EMFILE）");
    s.add("wsa_namespace", wsa_namespace_separate(), "WSA 万段与 Win32 千段不重叠");
    s.add("wsa_anchor", to_wsa(9) == WSAENOTSOCK && to_wsa(14) == WSAEFAULT && to_wsa(22) == WSAEINVAL, "三条锚点映射正确");
    s.add("badf_cardinality", BADF_CAUSES.len() == BADF_TOTAL && causes_of(Surface::Linux) == BADF_LINUX && causes_of(Surface::Win32) == BADF_WIN, "成因 11 条，Linux 6 / Windows 5");
    s.add("cause_surface", BADF_CAUSES.iter().all(|c| c.surface() != Surface::Winsock), "成因只归 Linux 与 Win32 两面");
    s.add("width_noaccess", !fits_u8(ERROR_NOACCESS) && fits_u16(ERROR_NOACCESS) && width_truncates(ERROR_NOACCESS) && truncate_u8(ERROR_NOACCESS) == 230, "998 溢出 u8，截断为 230");
    s.add("width_retry", !fits_u8(ERROR_RETRY) && fits_u16(ERROR_RETRY) && width_truncates(ERROR_RETRY), "1237 溢出 u8");
    s.add("channel_map", channel_for(Surface::Linux) == Channel::Errno && channel_for(Surface::Win32) == Channel::LastError && channel_for(Surface::Winsock) == Channel::WsaLastError, "三面各走各的通道");
    s.add("channel_crosstalk", !channel_ok(Channel::LastError, Surface::Winsock), "Winsock 读 Win32 通道即错");
    let mut le = LastError::new();
    le.set(ERROR_NOACCESS);
    le.success();
    let sticky_ok = le.get() == ERROR_NOACCESS;
    le.clear();
    let clear_ok = le.get() == ERROR_SUCCESS;
    let mut er = ErrnoCell::new();
    er.fail(9);
    er.ok();
    let errno_hold = er.get() == 9;
    s.add("last_error_sticky", sticky_ok && clear_ok && errno_hold, "LastError 成功不清、显式清才清；errno 保持");
    s.add("divergence_three", DIVERGENCES.len() == DIVERGENCE_COUNT && divergence_kinds() == DIVERGENCE_COUNT, "差异账三条且种类互异");
    s.add("divergence_complete", divergences_complete(truncated_divergences(3).len()), "三条全列才算完整");
    s.add("divergence_text", DIVERGENCES.iter().all(|d| !d.linux_side.is_empty() && !d.win_side.is_empty() && !d.kind.detail().is_empty()), "差异账文案非空");
    s.add("divergence_truncated", truncated_divergences(2).len() == 2 && !divergences_complete(2), "少列一条即判不完整");
    s.add("flaws_detected", Flaw::all().iter().all(|f| flaw_detected(*f)), "三类缺陷均可检出");

    let fl = run_flaws();
    let fh = per_flaw(&fl);
    s.add("flaw_ten_of_ten", fh == [INJECT_ROUNDS; 3], "三类缺陷各检出 10/10");
    s.add("flaw_ledger", fl.conserved() && fl.escaped == 0 && fl.injected == INJECT_ROUNDS * 3, "缺陷注入 30 = 检出 30 + 逃逸 0");
    let stage_ok = CmpStage::Idle.can_go(CmpStage::Mapped)
        && CmpStage::Mapped.can_go(CmpStage::Compared)
        && CmpStage::Compared.can_go(CmpStage::Declared)
        && CmpStage::Declared.can_go(CmpStage::Sealed)
        && !CmpStage::Idle.can_go(CmpStage::Sealed)
        && !CmpStage::Sealed.can_go(CmpStage::Idle);
    s.add("state_machine", stage_ok, "对照阶段只许单步前进");
    let d1 = run_digest();
    let d2 = run_digest();
    let fl2 = run_flaws();
    s.add("double_run", d1 == d2 && fl2.injected == fl.injected && per_flaw(&fl2) == fh, "双跑指纹与缺陷注入账一致");
    s.add_dyn("map_detail", map_drift() == 0, &format!("行={} 漂移={} WSA 行={}", MAP.len(), map_drift(), wsa_rows()));
    s.add_dyn("cause_detail", causes_of(Surface::Linux) + causes_of(Surface::Win32) == BADF_TOTAL, &format!("Linux={} Windows={}", causes_of(Surface::Linux), causes_of(Surface::Win32)));
    s.add_dyn("digest_detail", d1 != 0, &format!("表={:08x} 差异={:08x} 跑={:08x}", map_digest(), divergence_digest(), d1));
    s
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checks_all_green() {
        let s = run_unx9236_checks();
        assert!(s.all_passed());
        assert!(!s.truncated());
        assert!(s.len() >= 12);
    }
    #[test]
    fn sentinel_and_null_are_distinct() {
        assert_eq!(INVALID_HANDLE_VALUE, u64::MAX);
        assert_eq!(NULL_HANDLE, 0);
        assert_ne!(NULL_HANDLE, INVALID_HANDLE_VALUE);
        assert!(!handle_valid(NULL_HANDLE));
        assert!(!handle_valid(INVALID_HANDLE_VALUE));
        assert!(handle_valid(3));
    }
    #[test]
    fn pseudo_handle_is_the_trap() {
        assert_eq!(PSEUDO_PROCESS, INVALID_HANDLE_VALUE);
        assert_ne!(PSEUDO_THREAD, INVALID_HANDLE_VALUE);
        assert!(PSEUDO_THREAD == u64::MAX - 1);
    }
    #[test]
    fn fd_widening_never_hits_sentinel() {
        for fd in 0..1024u32 {
            assert_ne!(handle_from_fd(fd), INVALID_HANDLE_VALUE);
        }
        assert_eq!(handle_from_signed_fd(-1), INVALID_HANDLE_VALUE);
    }
    #[test]
    fn map_round_trips() {
        assert_eq!(MAP.len(), MAP_ROWS);
        assert!(all_round_trip());
        for r in MAP.iter() {
            assert_eq!(to_windows(r.linux), r.win);
            assert_eq!(from_windows(r.win), r.linux);
            assert_eq!(to_wsa(r.linux), r.ws);
        }
    }
    #[test]
    fn wsa_three_anchors() {
        assert_eq!(wsa_rows(), 4);
        assert_eq!(to_wsa(9), WSAENOTSOCK);
        assert_eq!(to_wsa(14), WSAEFAULT);
        assert_eq!(to_wsa(22), WSAEINVAL);
        assert!(wsa_namespace_separate());
    }
    #[test]
    fn badf_cause_cardinality() {
        assert_eq!(BADF_CAUSES.len(), 11);
        assert_eq!(causes_of(Surface::Linux), 6);
        assert_eq!(causes_of(Surface::Win32), 5);
        assert_eq!(causes_of(Surface::Winsock), 0);
    }
    #[test]
    fn width_is_not_eight_bits() {
        assert!(!fits_u8(998));
        assert!(fits_u16(998));
        assert_eq!(truncate_u8(998), 230);
        assert!(!fits_u8(1237));
        assert_eq!(truncate_u8(1237), 213);
    }
    #[test]
    fn last_error_is_sticky() {
        let mut le = LastError::new();
        assert_eq!(le.get(), ERROR_SUCCESS);
        le.set(ERROR_NOACCESS);
        le.success();
        assert_eq!(le.get(), ERROR_NOACCESS);
        le.clear();
        assert_eq!(le.get(), ERROR_SUCCESS);
    }
    #[test]
    fn divergences_are_complete_and_distinct() {
        assert_eq!(DIVERGENCES.len(), 3);
        assert_eq!(divergence_kinds(), 3);
        assert!(divergences_complete(3));
        assert!(!divergences_complete(2));
        assert_eq!(truncated_divergences(2).len(), 2);
    }
    #[test]
    fn flaws_are_caught() {
        for f in Flaw::all().iter() {
            assert!(flaw_detected(*f));
        }
        assert_eq!(map_drift_of(&drifted_map()), 1);
        let l = run_flaws();
        assert_eq!(per_flaw(&l), [10, 10, 10]);
        assert_eq!(l.escaped, 0);
        assert!(l.conserved());
    }
    #[test]
    fn stage_and_digest_stable() {
        let mut cur = CmpStage::Idle;
        for nxt in [CmpStage::Mapped, CmpStage::Compared, CmpStage::Declared, CmpStage::Sealed].iter() {
            assert!(cur.can_go(*nxt));
            cur = *nxt;
        }
        assert!(!CmpStage::Idle.can_go(CmpStage::Sealed));
        assert_eq!(run_digest(), run_digest());
        assert_ne!(map_digest(), 0);
    }
}
