//! UNX-F9206 open 坏路径指针 EFAULT 矩阵 · 0x7A100段
//! 判据原文（任务板 F9206 逐字）：坏路径指针注入全 EFAULT 判据过，路径截断
//! 注入零越界读格过，10/10 次检出格过。
//!
//! 设计要点：
//!   ① 路径是用户内存里的 C 串：open 必须逐字节扫到 NUL，任何一字节落在未映射
//!      段就立刻 EFAULT，已读字节数（残量）要如实声明，不许把半截路径当完整。
//!   ② 判定次序：dirfd（EBADF）→ 指针扫描（EFAULT）→ 长度（ENAMETOOLONG）
//!      → 空串/查名（ENOENT）。次序颠倒会把「坏指针 + 坏 dirfd」判错码。
//!   ③ 零越界读：扫描在 NUL 处立即停手，touched 恒等于 bytes+1；截断样本
//!      只读段内字节，绝不伸进下一段，更不做预读对齐。
//!   ④ 坏指针四类（空址/未映射/无 NUL 半截/溢出址）各 10 轮；截断三类
//!      （NUL 在段尾/段中截断/空段）逐类核残量。
//!   ⑤ 诊断码闭集 + 注入账守恒 + 双跑指纹；UNX 段内码值唯一。
use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 段基址（F9206 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A100;
/// Linux errno（正数口径）。
pub const ENOENT: i32 = 2;
pub const EBADF: i32 = 9;
pub const EFAULT: i32 = 14;
pub const EMFILE: i32 = 24;
pub const EINVAL: i32 = 22;
pub const ENAMETOOLONG: i32 = 36;
/// 路径长度上限（Linux PATH_MAX）。
pub const PATH_MAX: usize = 4096;
/// 注入轮数、坏指针类别数、段数、每段字节容量、打开表容量。
pub const INJECT_ROUNDS: u32 = 10;
pub const BAD_PTR_CLASSES: u32 = 4;
pub const SEGS: usize = 3;
pub const SEG_CAP: usize = 24;
pub const OPEN_CAP: u32 = 8;
/// openat 的当前目录哨兵。
pub const AT_FDCWD: i32 = -100;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenCode {
    NullPath = 1,
    UnmappedPath = 2,
    PartialPath = 3,
    TooLong = 4,
    EmptyPath = 5,
    BadDfd = 6,
    ZeroOob = 7,
    ResidualMismatch = 8,
    TableFull = 9,
    LedgerRegress = 10,
    NotReproducible = 11,
    SegmentViolation = 12,
}

impl OpenCode {
    /// 码值 = 段基址 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 关联的 Linux errno（正数）。
    pub fn errno(self) -> i32 {
        match self {
            OpenCode::NullPath | OpenCode::UnmappedPath | OpenCode::PartialPath => EFAULT,
            OpenCode::TooLong => ENAMETOOLONG,
            OpenCode::EmptyPath => ENOENT,
            OpenCode::BadDfd => EBADF,
            OpenCode::TableFull => EMFILE,
            _ => EINVAL,
        }
    }
    /// 中文诊断与可操作建议。
    pub fn detail(self) -> &'static str {
        match self {
            OpenCode::NullPath => "空路径指针：地址 0 一律 EFAULT；请检查路径变量是否初始化",
            OpenCode::UnmappedPath => "未映射地址：该页不在用户地址空间；请核对指针来源与生命周期",
            OpenCode::PartialPath => "半截路径：段内没有 NUL 就断了，已读部分不得当完整路径；请补终止符",
            OpenCode::TooLong => "路径过长：达到 PATH_MAX 仍无 NUL；请缩短路径或改用 openat 分段",
            OpenCode::EmptyPath => "空串路径：合法指针但零长度，Linux 回 ENOENT；请检查拼装结果",
            OpenCode::BadDfd => "坏目录 fd：openat 的 dirfd 既不等于 AT_FDCWD 也不是有效句柄",
            OpenCode::ZeroOob => "越界读：扫描读过了 NUL 或伸进下一段；请逐字节判定并即时停手",
            OpenCode::ResidualMismatch => "残量不符：声明的已读字节与实际不符；请用同一计数器出账",
            OpenCode::TableFull => "打开表满：EMFILE；请关闭闲置句柄或提高 RLIMIT_NOFILE",
            OpenCode::LedgerRegress => "注入账倒退：累计计数只增不减；请检查回滚路径",
            OpenCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查隐藏状态",
            OpenCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在 0x7A000..=0x7D1C0 窗口内",
        }
    }
    /// 闭集全量。
    pub const ALL: [OpenCode; 12] = [
        OpenCode::NullPath, OpenCode::UnmappedPath, OpenCode::PartialPath,
        OpenCode::TooLong, OpenCode::EmptyPath, OpenCode::BadDfd,
        OpenCode::ZeroOob, OpenCode::ResidualMismatch, OpenCode::TableFull,
        OpenCode::LedgerRegress, OpenCode::NotReproducible, OpenCode::SegmentViolation,
    ];
}

/// 系统调用结果：携带 Linux errno 正数码。
pub type SysResult<T> = Result<T, i32>;

// ── 用户地址空间 ──────────────────────────────────────────────────────
/// 一个可读段：基址 + 长度 + 数据起点。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Seg {
    pub base: u64,
    pub len: usize,
    pub off: usize,
    pub readable: bool,
}
impl Seg {
    const fn empty(base: u64, off: usize) -> Seg {
        Seg { base, len: 0, off, readable: false }
    }
    /// 是否覆盖该地址（长度为零的段不覆盖任何地址）。
    pub fn covers(self, addr: u64) -> bool {
        self.len > 0 && addr >= self.base && addr < self.base.saturating_add(self.len as u64)
    }
}

/// 定容用户空间：三段，各 24 字节。
#[derive(Clone, Copy, Debug)]
pub struct Space {
    segs: [Seg; SEGS],
    data: [u8; SEGS * SEG_CAP],
}
impl Space {
    /// 空空间：三段基址 0x1000/0x2000/0x3000，全部不可读。
    pub const fn new() -> Space {
        Space {
            segs: [
                Seg::empty(0x1000, 0),
                Seg::empty(0x2000, SEG_CAP),
                Seg::empty(0x3000, SEG_CAP * 2),
            ],
            data: [0u8; SEGS * SEG_CAP],
        }
    }
    /// 往第 seg 段写入字节并置可读；超容量返回 false。
    pub fn put(&mut self, seg: usize, bytes: &[u8]) -> bool {
        if bytes.len() > SEG_CAP {
            return false;
        }
        let off = seg.saturating_mul(SEG_CAP);
        let mut i = 0usize;
        while i < bytes.len() {
            match (bytes.get(i), off.checked_add(i)) {
                (Some(b), Some(at)) => {
                    if let Some(slot) = self.data.get_mut(at) {
                        *slot = *b;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        match self.segs.get_mut(seg) {
            Some(s) => {
                s.len = bytes.len();
                s.readable = true;
                true
            }
            None => false,
        }
    }
    /// 命中地址所在段号。
    pub fn segment_of(&self, addr: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < SEGS {
            if let Some(s) = self.segs.get(i) {
                if s.covers(addr) {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }
    /// 读一个用户字节：未映射或不可读返回 None。
    pub fn read_byte(&self, addr: u64) -> Option<u8> {
        let idx = self.segment_of(addr)?;
        let seg = self.segs.get(idx)?;
        if !seg.readable {
            return None;
        }
        let delta = usize::try_from(addr.saturating_sub(seg.base)).ok()?;
        self.data.get(seg.off.saturating_add(delta)).copied()
    }
}

// ── C 串扫描 ──────────────────────────────────────────────────────────
/// 扫描结局。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScanState {
    Complete = 1,
    Fault = 2,
    TooLong = 3,
}
/// 一次路径扫描的观测值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PathScan {
    pub state: ScanState,
    /// 路径字节数（不含 NUL）
    pub bytes: usize,
    /// 实际触碰的字节数（成功时 = bytes+1，含 NUL）
    pub touched: usize,
    /// 触碰到的不同段数
    pub segments: usize,
    /// 越界读计数（本实现恒为 0）
    pub oob: usize,
}
impl PathScan {
    const fn fault(bytes: usize, touched: usize, segments: usize) -> PathScan {
        PathScan { state: ScanState::Fault, bytes, touched, segments, oob: 0 }
    }
    /// 零越界读：成功时恰好读到 NUL，失败时不预读下一字节。
    pub fn zero_oob(&self) -> bool {
        (match self.state {
            ScanState::Complete => self.touched == self.bytes.saturating_add(1),
            _ => self.touched == self.bytes,
        }) && self.oob == 0
    }
}

/// 从 addr 起扫 C 串，最多 max 字节；NUL 前越界即 Fault。
pub fn scan_cstring(space: &Space, addr: u64, max: usize) -> PathScan {
    let mut n = 0usize;
    let mut touched = 0usize;
    let mut segments = 0usize;
    let mut last: Option<usize> = None;
    while n < max {
        let at = match addr.checked_add(n as u64) {
            Some(v) => v,
            None => return PathScan::fault(n, touched, segments),
        };
        match space.read_byte(at) {
            Some(0) => {
                touched = touched.saturating_add(1);
                if let Some(s) = space.segment_of(at) {
                    if last != Some(s) {
                        segments = segments.saturating_add(1);
                    }
                }
                return PathScan {
                    state: ScanState::Complete,
                    bytes: n,
                    touched,
                    segments,
                    oob: 0,
                };
            }
            Some(_) => {
                touched = touched.saturating_add(1);
                if let Some(s) = space.segment_of(at) {
                    if last != Some(s) {
                        segments = segments.saturating_add(1);
                        last = Some(s);
                    }
                }
                n = n.saturating_add(1);
            }
            None => return PathScan::fault(n, touched, segments),
        }
    }
    PathScan { state: ScanState::TooLong, bytes: n, touched, segments, oob: 0 }
}

// ── 名字表与世界 ──────────────────────────────────────────────────────
/// 一个名字条目（定长 16 字节）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Name {
    pub bytes: [u8; 16],
    pub len: usize,
}
impl Name {
    const fn empty() -> Name {
        Name { bytes: [0u8; 16], len: 0 }
    }
    /// 从切片构造；超 16 字节返回空条目。
    pub fn from(bytes: &[u8]) -> Name {
        if bytes.len() > 16 {
            return Name::empty();
        }
        let mut n = Name::empty();
        let mut i = 0usize;
        while i < bytes.len() {
            if let (Some(b), Some(slot)) = (bytes.get(i), n.bytes.get_mut(i)) {
                *slot = *b;
            }
            i += 1;
        }
        n.len = bytes.len();
        n
    }
}
/// 打开世界：地址空间 + 名字表 + 打开计数。
#[derive(Clone, Copy, Debug)]
pub struct World {
    pub space: Space,
    pub names: [Name; 4],
    pub nnames: usize,
    pub allocs: u32,
    pub closes: u32,
    pub live: u32,
}
impl World {
    /// 空世界。
    pub const fn new() -> World {
        World {
            space: Space::new(),
            names: [Name::empty(), Name::empty(), Name::empty(), Name::empty()],
            nnames: 0,
            allocs: 0,
            closes: 0,
            live: 0,
        }
    }
    /// 演示世界：0x1000 处 "demo.txt\0"，名字表含 demo.txt。
    pub fn demo() -> World {
        let mut w = World::new();
        let _ = w.space.put(0, b"demo.txt\x00");
        w.add_name(b"demo.txt");
        w
    }
    /// 添加名字；表满返回 false。
    pub fn add_name(&mut self, bytes: &[u8]) -> bool {
        if self.nnames >= self.names.len() {
            return false;
        }
        if let Some(slot) = self.names.get_mut(self.nnames) {
            *slot = Name::from(bytes);
            self.nnames = self.nnames.saturating_add(1);
            return true;
        }
        false
    }
    /// 按路径字节数在名字表里查找。
    pub fn lookup(&self, addr: u64, len: usize) -> Option<usize> {
        if len == 0 {
            return None;
        }
        let mut i = 0usize;
        while i < self.nnames {
            if let Some(name) = self.names.get(i) {
                if name.len == len {
                    let mut same = true;
                    let mut k = 0usize;
                    while k < len {
                        let want = name.bytes.get(k).copied();
                        let got = match addr.checked_add(k as u64) {
                            Some(a) => self.space.read_byte(a),
                            None => None,
                        };
                        if want != got {
                            same = false;
                        }
                        k = k.saturating_add(1);
                    }
                    if same {
                        return Some(i);
                    }
                }
            }
            i = i.saturating_add(1);
        }
        None
    }
    /// 打开：dirfd → 指针 → 长度 → 空串/查名。
    pub fn openat(&mut self, dirfd: i32, addr: u64, max: usize) -> SysResult<u16> {
        let dir_ok = dirfd == AT_FDCWD || (dirfd >= 0 && dirfd <= 2);
        if !dir_ok {
            return Err(EBADF);
        }
        let scan = scan_cstring(&self.space, addr, max);
        match scan.state {
            ScanState::Fault => return Err(EFAULT),
            ScanState::TooLong => return Err(ENAMETOOLONG),
            ScanState::Complete => {}
        }
        if scan.bytes == 0 {
            return Err(ENOENT);
        }
        let idx = match self.lookup(addr, scan.bytes) {
            Some(i) => i,
            None => return Err(ENOENT),
        };
        if self.live >= OPEN_CAP {
            return Err(EMFILE);
        }
        self.allocs = self.allocs.saturating_add(1);
        self.live = self.live.saturating_add(1);
        Ok((idx as u16).saturating_add(1))
    }
    /// 关闭一个打开令牌。
    pub fn close(&mut self, _token: u16) -> SysResult<()> {
        if self.live == 0 {
            return Err(EBADF);
        }
        self.live = self.live.saturating_sub(1);
        self.closes = self.closes.saturating_add(1);
        Ok(())
    }
    /// 打开账闭合：allocs = closes + live。
    pub fn balanced(&self) -> bool {
        self.allocs == self.closes.saturating_add(self.live)
    }
}

// ── 坏指针注入 ────────────────────────────────────────────────────────
/// 四类坏路径指针。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadPtrCase {
    Null = 1,
    Unmapped = 2,
    NoNul = 3,
    Overflow = 4,
}
impl BadPtrCase {
    /// 诊断码。
    pub fn code(self) -> OpenCode {
        match self {
            BadPtrCase::Null => OpenCode::NullPath,
            BadPtrCase::Unmapped => OpenCode::UnmappedPath,
            BadPtrCase::NoNul => OpenCode::PartialPath,
            BadPtrCase::Overflow => OpenCode::UnmappedPath,
        }
    }
    pub const ALL: [BadPtrCase; 4] = [
        BadPtrCase::Null, BadPtrCase::Unmapped,
        BadPtrCase::NoNul, BadPtrCase::Overflow,
    ];
    /// 样本地址。
    pub fn addr(self) -> u64 {
        match self {
            BadPtrCase::Null => 0,
            BadPtrCase::Unmapped => 0x9000,
            BadPtrCase::NoNul => 0x1000,
            BadPtrCase::Overflow => u64::MAX,
        }
    }
}

/// 三类截断样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TruncCase {
    NulAtLast = 1,
    CutMid = 2,
    EmptySeg = 3,
}
impl TruncCase {
    /// 期望的已读字节数（成功含 NUL，失败为已读前缀）。
    pub fn expect_touched(self) -> usize {
        match self {
            TruncCase::NulAtLast => 9,
            TruncCase::CutMid => 4,
            TruncCase::EmptySeg => 0,
        }
    }
    /// 期望结局。
    pub fn expect_state(self) -> ScanState {
        match self {
            TruncCase::NulAtLast => ScanState::Complete,
            TruncCase::CutMid | TruncCase::EmptySeg => ScanState::Fault,
        }
    }
    pub const ALL: [TruncCase; 3] = [
        TruncCase::NulAtLast, TruncCase::CutMid, TruncCase::EmptySeg,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}
impl InjectLedger {
    pub const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }
    /// 记一次；守恒破坏即回报账倒退。
    pub fn record(&mut self, caught: bool) -> SysResult<()> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(OpenCode::LedgerRegress as i32));
        }
        Ok(())
    }
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏指针各 10 轮：全 EFAULT，且零越界读。
pub fn run_bad_ptr_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per = [0u32; 4];
    for (i, case) in BadPtrCase::ALL.iter().enumerate() {
        let mut w = World::demo();
        // NoNul 样本：段内只有 4 字节且没有 NUL。
        if *case == BadPtrCase::NoNul {
            let _ = w.space.put(0, b"demo");
        }
        for _ in 0..INJECT_ROUNDS {
            let scan = scan_cstring(&w.space, case.addr(), PATH_MAX);
            let r = w.openat(AT_FDCWD, case.addr(), PATH_MAX);
            let ok = r == Err(EFAULT) && scan.state == ScanState::Fault && scan.zero_oob();
            let _ = ledger.record(ok);
            if ok {
                if let Some(n) = per.get_mut(i) {
                    *n = n.saturating_add(1);
                }
            }
        }
    }
    (ledger, per)
}

/// 单个截断样本的观测：结局、已读字节、越界读。
pub fn probe_truncation(which: TruncCase) -> (ScanState, usize, usize) {
    let mut space = Space::new();
    let payload: &[u8] = match which {
        TruncCase::NulAtLast => b"demo.txt\x00",
        TruncCase::CutMid => b"demo",
        TruncCase::EmptySeg => b"",
    };
    let _ = space.put(0, payload);
    let s = scan_cstring(&space, 0x1000, PATH_MAX);
    (s.state, s.touched, s.oob)
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, Linux 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；查无此名"),
    ("EBADF", -9, 6, "Windows ERROR_INVALID_HANDLE；坏 dirfd"),
    ("EFAULT", -14, 998, "Windows ERROR_NOACCESS；坏用户指针"),
    ("EMFILE", -24, 4, "Windows ERROR_TOO_MANY_OPEN_FILES；表满"),
    ("ENAMETOOLONG", -36, 206, "Windows ERROR_FILENAME_EXCED_RANGE；路径过长"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；参数族"),
];

/// 查表：按 errno 正数取负值；表外返回 0。
pub fn linux_neg(code: i32) -> i32 {
    for row in ERRNO_TABLE.iter() {
        if row.1 == -code {
            return row.1;
        }
    }
    0
}
/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool {
    code > 0 && linux_neg(code) == -code
}
/// FNV-1a 32 位。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for b in bytes.iter() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 规范剧本：正常开、空串、越界、坏 dirfd、关闭后再关。
pub fn scripted_fingerprint() -> u32 {
    let mut w = World::demo();
    let mut bytes: Vec<u8> = Vec::new();
    let _ = w.space.put(1, b"second\x00");
    w.add_name(b"second");
    let r1 = w.openat(AT_FDCWD, 0x1000, PATH_MAX);
    let r2 = w.openat(AT_FDCWD, 0x2000, PATH_MAX);
    let r3 = w.openat(AT_FDCWD, 0x9000, PATH_MAX);
    let r4 = w.openat(-7, 0x1000, PATH_MAX);
    let r5 = w.openat(AT_FDCWD, 0x1010, PATH_MAX);
    let script: [SysResult<u16>; 5] = [r1, r2, r3, r4, r5];
    for r in script.iter() {
        let v = match r {
            Ok(t) => *t as u32,
            Err(e) => 0x80u32 | (*e as u32 & 0x7F),
        };
        bytes.push((v & 0xFF) as u8);
        bytes.push(((v >> 8) & 0xFF) as u8);
    }
    if let Ok(t) = r1 {
        let _ = w.close(t);
        let _ = w.close(t);
    }
    bytes.push((w.allocs & 0xFF) as u8);
    bytes.push((w.closes & 0xFF) as u8);
    bytes.push((w.live & 0xFF) as u8);
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9206 判据集（18 条静态 + 4 条动态明细）。
pub fn run_unx9206_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9206");
    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");
    // 2. 正常扫描
    let w = World::demo();
    let scan = scan_cstring(&w.space, 0x1000, PATH_MAX);
    let normal = scan.state == ScanState::Complete && scan.bytes == 8
        && scan.touched == 9 && scan.segments == 1 && scan.zero_oob();
    s.add("正常路径：扫到 NUL 即停，touched=bytes+1", normal,
          "demo.txt 八个字节加终止符，恰好触碰九字节");
    // 3. 四类坏指针全 EFAULT
    let (ledger, per) = run_bad_ptr_matrix();
    let bad_ok = per.iter().all(|n| *n == INJECT_ROUNDS)
        && per.len() == BAD_PTR_CLASSES as usize;
    s.add("坏路径指针四类各 10/10 全 -EFAULT", bad_ok,
          "空址/未映射/无 NUL 半截/溢出址都必须 EFAULT");
    // 4. 半截路径残量
    let mut cut = World::demo();
    let _ = cut.space.put(0, b"demo");
    let half = scan_cstring(&cut.space, 0x1000, PATH_MAX);
    let half_ok = half.state == ScanState::Fault && half.bytes == 4
        && half.touched == 4 && half.zero_oob()
        && cut.openat(AT_FDCWD, 0x1000, PATH_MAX) == Err(EFAULT);
    s.add("半截路径：残量 4 字节如实声明且零越界", half_ok,
          "段内没有 NUL 就断，已读前缀不得当完整路径");
    // 5. 截断三类逐类核残量
    let mut trunc_ok = true;
    for c in TruncCase::ALL.iter() {
        let (state, touched, oob) = probe_truncation(*c);
        if state != c.expect_state() || touched != c.expect_touched() || oob != 0 {
            trunc_ok = false;
        }
    }
    s.add("截断三类：NUL 在段尾可成、段中截断回 EFAULT、零越界", trunc_ok,
          "三类样本的结局、已读字节、越界计数必须逐项对上");
    // 6. NUL 恰在容量边界
    let mut edge = Space::new();
    let _ = edge.put(0, b"abcdefg\x00");
    let e = scan_cstring(&edge, 0x1000, 8);
    s.add("边界：恰好在第 8 字节遇 NUL，仍算成功", 
          e.state == ScanState::Complete && e.bytes == 7 && e.touched == 8,
          "max 是上限不是截断点，NUL 落在上限内即合法");
    // 7. PATH_MAX 边界
    let mut long = Space::new();
    let mut buf = [b'a'; SEG_CAP];
    if let Some(last) = buf.get_mut(SEG_CAP - 1) {
        *last = 0;
    }
    let _ = long.put(0, &buf);
    let ok_long = scan_cstring(&long, 0x1000, SEG_CAP).state == ScanState::Complete;
    let too_long = scan_cstring(&long, 0x1000, SEG_CAP - 1).state == ScanState::TooLong;
    s.add("长度边界：NUL 在 max 内成功、超出 max 回 ENAMETOOLONG", ok_long && too_long,
          "max 取 SEG_CAP-1 时 NUL 越界，必须报名字过长");
    // 8. 空串 ENOENT
    let mut empty = World::demo();
    let _ = empty.space.put(2, b"\x00");
    let empty_ok = empty.openat(AT_FDCWD, 0x3000, PATH_MAX) == Err(ENOENT);
    s.add("空串：合法指针零长度回 ENOENT", empty_ok,
          "空路径不是 EFAULT，Linux 明确回 ENOENT");
    // 9. 次序：坏 dirfd 先于坏指针
    let mut order = World::demo();
    let order_ok = order.openat(-7, 0x9000, PATH_MAX) == Err(EBADF)
        && order.openat(5, 0, PATH_MAX) == Err(EBADF);
    s.add("判定次序：坏 dirfd 先于坏指针", order_ok,
          "dirfd 未通过就不该扫路径，否则 errno 会错位");
    // 10. 次序：坏指针先于查名
    let order2 = order.openat(AT_FDCWD, 0x9000, PATH_MAX) == Err(EFAULT)
        && order.openat(AT_FDCWD, 0x1000, PATH_MAX).is_ok();
    s.add("判定次序：坏指针先于查名，正常名可开", order2,
          "指针错就不能进入名字表，正常名必须开得出来");
    // 11. 名字不匹配 ENOENT
    let mut miss = World::demo();
    let _ = miss.space.put(1, b"other\x00");
    let miss_ok = miss.openat(AT_FDCWD, 0x2000, PATH_MAX) == Err(ENOENT);
    s.add("查名失败：合法路径但名字表无此项回 ENOENT", miss_ok,
          "名字表与路径必须逐字节相同，不许前缀匹配");
    // 12. 容量 EMFILE
    let mut cap = World::demo();
    let mut opened = 0u32;
    for _ in 0..OPEN_CAP {
        if cap.openat(AT_FDCWD, 0x1000, PATH_MAX).is_ok() {
            opened = opened.saturating_add(1);
        }
    }
    let full = cap.openat(AT_FDCWD, 0x1000, PATH_MAX) == Err(EMFILE);
    s.add("容量：打开表满回 EMFILE", opened == OPEN_CAP && full,
          "表满必须是 EMFILE，且账目仍闭合");
    // 13. 打开账闭合
    let mut bal = World::demo();
    let tok = bal.openat(AT_FDCWD, 0x1000, PATH_MAX);
    let closed = match tok {
        Ok(t) => bal.close(t).is_ok() && bal.close(t) == Err(EBADF),
        Err(_) => false,
    };
    s.add("打开账：allocs = closes + live 且重复关闭回 EBADF",
          closed && bal.balanced(),
          "只有真正释放的关闭才计数，账必须双闭合");
    // 14. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BAD_PTR_CLASSES * INJECT_ROUNDS,
          "四类 × 10 轮共 40 次注入，必须 40 次检出");
    // 15. Linux 逐码一致
    let codes_ok = agrees_with_linux(EFAULT) && agrees_with_linux(ENOENT)
        && agrees_with_linux(ENAMETOOLONG) && agrees_with_linux(EBADF)
        && agrees_with_linux(EMFILE) && !agrees_with_linux(0);
    s.add("Linux 逐码一致：五个 errno 与 errno.h 对齐", codes_ok,
          "EFAULT/ENOENT/ENAMETOOLONG/EBADF/EMFILE 负值逐项对齐");
    // 16. 文案闭集
    let text_ok = OpenCode::ALL.iter().all(|c| !c.detail().is_empty() && c.errno() > 0);
    let mut uniq = true;
    for (i, a) in OpenCode::ALL.iter().enumerate() {
        for b in OpenCode::ALL.iter().skip(i + 1) {
            if a.val() == b.val() {
                uniq = false;
            }
        }
    }
    s.add("文案闭集：12 个诊断码建议非空且码值唯一", text_ok && uniq,
          "诊断码要能取建议，也要能唯一定位判据");
    // 17. 双跑一致
    let a = scripted_fingerprint();
    let b = scripted_fingerprint();
    s.add("双跑一致：剧本指纹相等", a == b,
          "同剧本两次必须逐字段相等，隐藏状态即失败");
    // 动态明细（4 条）
    let bad_text = format!("坏指针 注入 {} 检出 {} 逃逸 {}",
                           ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("坏指针账", ledger.conserved(), &bad_text);
    let trunc_text = format!("截断样本 {} 类", TruncCase::ALL.len());
    s.add_dyn("截断覆盖", trunc_ok, &trunc_text);
    let scan_text = format!("正常扫描 bytes={} touched={} oob={}",
                            scan.bytes, scan.touched, scan.oob);
    s.add_dyn("扫描快照", scan.zero_oob(), &scan_text);
    let fp_text = format!("剧本指纹 0x{:08X}", a);
    s.add_dyn("指纹快照", a == b, &fp_text);
    // 18. 收口
    let n_before = s.len();
    s.add("收口：判据条数达到声明值", n_before >= 17,
          "收口断言条数只增不减，删条必须显式改声明");
    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scan_normal_and_bounds() {
        let mut sp = Space::new();
        assert!(sp.put(0, b"demo.txt\x00"));
        let s = scan_cstring(&sp, 0x1000, PATH_MAX);
        assert_eq!(s.state, ScanState::Complete);
        assert_eq!((s.bytes, s.touched, s.oob), (8, 9, 0));
        assert!(s.zero_oob());
    }
    #[test]
    fn null_and_unmapped_are_efault() {
        let sp = Space::new();
        assert_eq!(scan_cstring(&sp, 0, PATH_MAX).state, ScanState::Fault);
        assert_eq!(scan_cstring(&sp, 0x9000, PATH_MAX).state, ScanState::Fault);
        assert_eq!(scan_cstring(&sp, u64::MAX, PATH_MAX).state, ScanState::Fault);
    }
    #[test]
    fn half_path_declares_residual() {
        let mut sp = Space::new();
        assert!(sp.put(0, b"demo"));
        let s = scan_cstring(&sp, 0x1000, PATH_MAX);
        assert_eq!(s.state, ScanState::Fault);
        assert_eq!(s.bytes, 4);
        assert_eq!(s.touched, 4);
        assert!(s.zero_oob());
    }
    #[test]
    fn truncation_matrix() {
        for c in TruncCase::ALL.iter() {
            let (state, touched, oob) = probe_truncation(*c);
            assert_eq!(state, c.expect_state());
            assert_eq!(touched, c.expect_touched());
            assert_eq!(oob, 0);
        }
    }
    #[test]
    fn empty_path_is_enoent() {
        let mut w = World::demo();
        let _ = w.space.put(2, b"\x00");
        assert_eq!(w.openat(AT_FDCWD, 0x3000, PATH_MAX), Err(ENOENT));
    }
    #[test]
    fn bad_dirfd_precedes_efault() {
        let mut w = World::demo();
        assert_eq!(w.openat(-7, 0x9000, PATH_MAX), Err(EBADF));
        assert_eq!(w.openat(9, 0, PATH_MAX), Err(EBADF));
    }
    #[test]
    fn name_lookup_is_exact() {
        let mut w = World::demo();
        let _ = w.space.put(1, b"other\x00");
        assert_eq!(w.openat(AT_FDCWD, 0x2000, PATH_MAX), Err(ENOENT));
        assert!(w.openat(AT_FDCWD, 0x1000, PATH_MAX).is_ok());
    }
    #[test]
    fn open_table_accounting() {
        let mut w = World::demo();
        let t = w.openat(AT_FDCWD, 0x1000, PATH_MAX).unwrap_or(0);
        assert!(t > 0);
        assert!(w.close(t).is_ok());
        assert_eq!(w.close(t), Err(EBADF));
        assert!(w.balanced());
    }
    #[test]
    fn capacity_is_emfile() {
        let mut w = World::demo();
        for _ in 0..OPEN_CAP {
            assert!(w.openat(AT_FDCWD, 0x1000, PATH_MAX).is_ok());
        }
        assert_eq!(w.openat(AT_FDCWD, 0x1000, PATH_MAX), Err(EMFILE));
        assert!(w.balanced());
    }
    #[test]
    fn checks_all_green() {
        let s = run_unx9206_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
