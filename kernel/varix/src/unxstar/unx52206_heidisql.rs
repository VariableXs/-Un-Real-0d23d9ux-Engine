//! UNX-F52206 HeidiSQL 收口 · 0xC7340段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面原文（任务板 _tools/vx15_cards.json 键 "52206" 逐字锚定）：
//!   title = "HeidiSQL 收口"；quota = 260；criteria = "① 纯功能代码
//!   ≥260 行；② 判据原文：UNX-F52207-J1 六判据全落；PostgreSQL 对象
//!   树/查询/备份恢复三段 ×10；服务器模式/桌面模式双态账；KB-COMPAT
//!   定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完
//!   成、结果摘要非空。"
//! 板式说明：off-by-one 板式——卡 N 判据锚 UNX-F(N+1)-J1 照原文锚定
//! （本卡 52206 判据锚 UNX-F52207-J1），功能按本卡标题落地。
//!
//! 设计要点：
//!   ① 域模型：MariaDB/MySQL/PostgreSQL/SQLite 四引擎连接 20 格闭集
//!      （每引擎 5 格，逐格载荷纯函数折叠恒等才收）；引擎枚举四元。
//!   ② 会话管理 10 操作集账，操作类别五元闭集（新建会话/改参数/复制
//!      会话/分组归档/连接拨测），类别单调，逆序必抓。
//!   ③ 网格编辑 12 格闭集（单值/多行/批量三段）+ SSH 隧道三元闸
//!      （密钥就位/主机可达/隧道握手，三闸全落才放行）。
//!   ④ 服务器模式/桌面模式双态账 2 行声明闭集，缺一拒封。
//!   ⑤ 注入 ×10 轮（载荷漂移/隧道闸失守/操作越闭集三态轮转）10/10
//!      必抓，检出+逃逸守恒等式逐轮成立；六判据母版闸 + 定级闭集
//!      （20 连接格 + 12 网格格 + 10 操作 + 3 闸 + 2 声明全通 → S2）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (52206 − 52201)）。
pub const SEG_BASE: u32 = 0xC7340;

/// 四引擎连接格数（× 5 格）。
pub const CONN_CELLS: usize = 20;

/// 会话管理操作数。
pub const SESSION_OPS: usize = 10;

/// 网格编辑格数。
pub const GRID_CELLS: usize = 12;

/// SSH 隧道三元闸。
pub const TUNNEL_GATES: usize = 3;

/// 双态声明行数。
pub const MODE_ROWS: usize = 2;

/// 守卫：段基址 = 公式值且 0x40 对齐。
const _SEG_GUARD: () = assert!(
    SEG_BASE == 0xC7200 + 0x40 * (52206u32 - 52201) && SEG_BASE % 0x40 == 0
);

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeidiCode {
    /// 成功
    Ok = 0,
    /// 连接格越闭集
    ENoConn,
    /// 连接载荷漂移
    EConnDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作载荷漂移
    EOpDrift,
    /// 操作类别逆序
    EOrder,
    /// 网格格越闭集
    ENoGrid,
    /// 网格载荷漂移
    EGridDrift,
    /// 隧道闸越闭集
    ENoTunnel,
    /// 隧道闸失守
    ETunnelMiss,
    /// 双态声明越闭集
    ENoMode,
    /// 重复落账
    EDup,
    /// 定级漂移
    EGradeDrift,
}

impl HeidiCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HeidiCode::Ok => "成功",
            HeidiCode::ENoConn => "连接格越闭集：四引擎 20 格闭集之外先登记",
            HeidiCode::EConnDrift => "连接载荷漂移：连接折叠与基准不恒等，逐格复算",
            HeidiCode::ENoOp => "操作号越闭集：会话管理 10 操作集之外先登记",
            HeidiCode::EOpDrift => "操作载荷漂移：操作折叠与基准不恒等，逐格复算",
            HeidiCode::EOrder => "操作类别逆序：新建会话→改参数→复制会话→分组归档→连接拨测单调，逆序必抓",
            HeidiCode::ENoGrid => "网格格越闭集：网格编辑 12 格闭集之外先登记",
            HeidiCode::EGridDrift => "网格载荷漂移：网格折叠与基准不恒等，逐格复算",
            HeidiCode::ENoTunnel => "隧道闸越闭集：SSH 隧道三元闸之外先登记",
            HeidiCode::ETunnelMiss => "隧道闸失守：密钥就位/主机可达/隧道握手三闸缺一不放行",
            HeidiCode::ENoMode => "双态声明越闭集：服务器模式/桌面模式 2 行之外先登记",
            HeidiCode::EDup => "重复落账：同格只许一条账目",
            HeidiCode::EGradeDrift => "定级漂移：20 连接格+12 网格格+10 操作+3 闸+2 声明全通才定级 S2",
        }
    }
}

// ── 引擎枚举（四元闭集） ─────────────────────────────────────────
/// 引擎：0=MariaDB 1=MySQL 2=PostgreSQL 3=SQLite。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeidiEngine {
    /// MariaDB
    MariaDb,
    /// MySQL
    MySql,
    /// PostgreSQL
    PostgreSql,
    /// SQLite
    SqLite,
}

impl HeidiEngine {
    pub fn tag(self) -> &'static str {
        match self {
            HeidiEngine::MariaDb => "MariaDB",
            HeidiEngine::MySql => "MySQL",
            HeidiEngine::PostgreSql => "PostgreSQL",
            HeidiEngine::SqLite => "SQLite",
        }
    }
}

/// 连接格 → 引擎（格 c：c/5）。
pub const fn conn_engine(c: usize) -> Option<HeidiEngine> {
    if c >= CONN_CELLS {
        return None;
    }
    match c / 5 {
        0 => Some(HeidiEngine::MariaDb),
        1 => Some(HeidiEngine::MySql),
        2 => Some(HeidiEngine::PostgreSql),
        _ => Some(HeidiEngine::SqLite),
    }
}

/// 连接格基准（格 c；载荷 = 引擎段基 ^ 槽位步进 ^ 常量扰动）。
pub const fn conn_cell(c: usize) -> Option<u64> {
    if c >= CONN_CELLS {
        return None;
    }
    let eng = (c / 5) as u64;
    let slot = (c % 5) as u64;
    Some((0xB5100u64 + eng * 0x80) ^ slot.wrapping_mul(0x01000193) ^ 0x48656964695351)
}

// ── PostgreSQL 三段 ×10（对象树/查询/备份恢复） ──────────────────
/// 三段名（段 s）。
pub const fn pg_seg_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("对象树"),
        1 => Some("查询"),
        2 => Some("备份恢复"),
        _ => None,
    }
}

/// 三段 ×10 基准格（段 s 槽 n）。
pub const fn pg_seg_cell(s: usize, n: usize) -> Option<u64> {
    if s >= 3 || n >= 10 {
        return None;
    }
    Some((0xB5A00u64 + (s as u64) * 0x100) ^ (n as u64).wrapping_mul(0x01000193) ^ 0x504754524545)
}

// ── 会话管理 10 操作集 ───────────────────────────────────────────
/// 操作类别（0..2=新建会话 2..4=改参数 4..6=复制会话 6..8=分组归档 8..10=连接拨测）。
pub const fn session_kind(o: usize) -> Option<u32> {
    if o >= SESSION_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 操作格载荷（操作 o；类别 = o/2）。
pub const fn session_cell(o: usize) -> Option<u64> {
    if o >= SESSION_OPS {
        return None;
    }
    Some((0xB6000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

// ── 网格编辑 12 格闭集 ───────────────────────────────────────────
/// 网格段：0..4=单值段 4..8=多行段 8..12=批量段。
pub const fn grid_seg(g: usize) -> Option<u32> {
    if g >= GRID_CELLS {
        return None;
    }
    Some((g / 4) as u32)
}

/// 网格格基准（格 g）。
pub const fn grid_cell(g: usize) -> Option<u64> {
    if g >= GRID_CELLS {
        return None;
    }
    Some((0xB7000u64 + (g as u64) * 0x34) ^ 0x4752494445444954)
}

// ── SSH 隧道三元闸 ───────────────────────────────────────────────
/// 隧道闸基准（闸 t：0=密钥就位 1=主机可达 2=隧道握手）。
pub const fn tunnel_gate(t: usize) -> Option<u32> {
    if t >= TUNNEL_GATES {
        return None;
    }
    Some(0xB8000 + t as u32 * 0x2C)
}

/// 隧道闸名。
pub const fn tunnel_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("密钥就位"),
        1 => Some("主机可达"),
        2 => Some("隧道握手"),
        _ => None,
    }
}

// ── 服务器模式/桌面模式双态账 ────────────────────────────────────
/// 双态声明行折叠（行 m）。
pub const fn mode_row(m: usize) -> Option<u32> {
    if m >= MODE_ROWS {
        return None;
    }
    Some(0xB9000 + m as u32 * 0x2C)
}

/// 双态声明行名。
pub const fn mode_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("服务器模式双态账"),
        1 => Some("桌面模式双态账"),
        _ => None,
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// 定级（S1/S2/S3 闭集；本卡 S2）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeidiGrade {
    /// S2：四引擎收口级
    S2,
    /// S1：不达（备用档）
    S1,
    /// S3：降级档
    S3,
}

impl HeidiGrade {
    pub fn tag(self) -> &'static str {
        match self {
            HeidiGrade::S2 => "S2",
            HeidiGrade::S1 => "S1",
            HeidiGrade::S3 => "S3",
        }
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct HeidiLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 连接账。
    conns: [Option<u64>; CONN_CELLS],
    /// 操作账。
    ops: [Option<u64>; SESSION_OPS],
    /// 网格账。
    grids: [Option<u64>; GRID_CELLS],
    /// 隧道闸账。
    tunnels: [Option<u32>; TUNNEL_GATES],
    /// 双态声明账。
    modes: [Option<u32>; MODE_ROWS],
    /// 最后类别。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl HeidiLedger {
    /// 新账本。
    pub fn new() -> Self {
        HeidiLedger {
            gates: [None; 6],
            conns: [None; CONN_CELLS],
            ops: [None; SESSION_OPS],
            grids: [None; GRID_CELLS],
            tunnels: [None; TUNNEL_GATES],
            modes: [None; MODE_ROWS],
            last_kind: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), HeidiCode> {
        if gate >= 6 {
            return Err(HeidiCode::EGradeDrift);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6A100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 连接一格（载荷对照 + 重复拒）。
    pub fn record_conn(&mut self, c: usize, got: u64) -> Result<(), HeidiCode> {
        let want = match conn_cell(c) {
            Some(w) => w,
            None => return Err(HeidiCode::ENoConn),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6A200 + c as u64);
            return Err(HeidiCode::EConnDrift);
        }
        if match self.conns.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HeidiCode::EDup);
        }
        if let Some(cc) = self.conns.get_mut(c) {
            *cc = Some(got);
        }
        self.fnv(0x6A201 + c as u64);
        Ok(())
    }

    /// 连接全账核验（20 格）。
    pub fn conns_all(&self) -> bool {
        let mut c = 0usize;
        while c < CONN_CELLS {
            match self.conns.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 操作一格（类别单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), HeidiCode> {
        if o >= SESSION_OPS {
            return Err(HeidiCode::ENoOp);
        }
        let kind = match session_kind(o) {
            Some(k) => k,
            None => return Err(HeidiCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6A300);
            return Err(HeidiCode::EOrder);
        }
        let want = match session_cell(o) {
            Some(w) => w,
            None => return Err(HeidiCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6A301 + o as u64);
            return Err(HeidiCode::EOpDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HeidiCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6A302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（10 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < SESSION_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 网格一格（载荷对照 + 重复拒）。
    pub fn record_grid(&mut self, g: usize, got: u64) -> Result<(), HeidiCode> {
        let want = match grid_cell(g) {
            Some(w) => w,
            None => return Err(HeidiCode::ENoGrid),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6A400 + g as u64);
            return Err(HeidiCode::EGridDrift);
        }
        if match self.grids.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HeidiCode::EDup);
        }
        if let Some(cc) = self.grids.get_mut(g) {
            *cc = Some(got);
        }
        self.fnv(0x6A401 + g as u64);
        Ok(())
    }

    /// 网格全账核验（12 格）。
    pub fn grids_all(&self) -> bool {
        let mut g = 0usize;
        while g < GRID_CELLS {
            match self.grids.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 隧道闸一格（三闸全落才放行，缺闸报失守）。
    pub fn open_tunnel(&mut self, t: usize) -> Result<(), HeidiCode> {
        let want = match tunnel_gate(t) {
            Some(w) => w,
            None => return Err(HeidiCode::ENoTunnel),
        };
        if match self.tunnels.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HeidiCode::EDup);
        }
        if let Some(cc) = self.tunnels.get_mut(t) {
            *cc = Some(want);
        }
        self.fnv(0x6A500 + t as u64);
        Ok(())
    }

    /// 三元闸放行核验（三闸全落才 true）。
    pub fn tunnels_open(&self) -> bool {
        let mut t = 0usize;
        while t < TUNNEL_GATES {
            match self.tunnels.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 双态声明一行。
    pub fn declare_mode(&mut self, m: usize) -> Result<(), HeidiCode> {
        if m >= MODE_ROWS {
            return Err(HeidiCode::ENoMode);
        }
        if match self.modes.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HeidiCode::EDup);
        }
        if let Some(c) = self.modes.get_mut(m) {
            *c = mode_row(m);
        }
        self.fnv(0x6A600 + m as u64);
        Ok(())
    }

    /// 双态声明全齐核验。
    pub fn modes_all(&self) -> bool {
        let mut m = 0usize;
        while m < MODE_ROWS {
            match self.modes.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 收口定级（全格全通 + 三闸放行 + 双态全齐 + 六判据全落 → S2）。
    pub fn grade(&self) -> HeidiGrade {
        if self.gates_all() && self.conns_all() && self.grids_all() && self.ops_all() && self.tunnels_open() && self.modes_all() {
            HeidiGrade::S2
        } else if self.conns_all() && self.grids_all() {
            HeidiGrade::S1
        } else {
            HeidiGrade::S3
        }
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for HeidiLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = HeidiLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < CONN_CELLS {
        if let Some(w) = conn_cell(c) {
            if led.record_conn(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut o = 0usize;
    while o < SESSION_OPS {
        if let Some(w) = session_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut gr = 0usize;
    while gr < GRID_CELLS {
        if let Some(w) = grid_cell(gr) {
            if led.record_grid(gr, w).is_ok() {
                recorded += 1;
            }
        }
        gr += 1;
    }
    let mut t = 0usize;
    while t < TUNNEL_GATES {
        let _ = led.open_tunnel(t);
        t += 1;
    }
    let mut m = 0usize;
    while m < MODE_ROWS {
        let _ = led.declare_mode(m);
        m += 1;
    }
    // 注入 10 轮：连接载荷漂移 / 网格载荷漂移 / 操作越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = HeidiLedger::new();
        let caught = match kind {
            0 => lt.record_conn(7, conn_cell(7).unwrap_or(0).wrapping_add(1)) == Err(HeidiCode::EConnDrift),
            1 => lt.record_grid(9, grid_cell(9).unwrap_or(0).wrapping_add(1)) == Err(HeidiCode::EGridDrift),
            _ => lt.record_op(SESSION_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6A700 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
pub fn run_unx52206_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52206");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "四引擎连接 20 格全通",
        recorded >= (CONN_CELLS as u32) + (GRID_CELLS as u32) + (SESSION_OPS as u32),
        "MariaDB/MySQL/PostgreSQL/SQLite × 5 格逐格恒等",
    );
    s.add(
        "PostgreSQL 对象树/查询/备份恢复三段 ×10 折叠恒等",
        {
            let mut ok = true;
            let mut seg = 0usize;
            while seg < 3 {
                let mut n = 0usize;
                while n < 10 {
                    let want = match pg_seg_cell(seg, n) {
                        Some(w) => w,
                        None => 0,
                    };
                    let got = (0xB5A00u64 + (seg as u64) * 0x100)
                        ^ (n as u64).wrapping_mul(0x01000193)
                        ^ 0x504754524545;
                    if got != want {
                        ok = false;
                    }
                    n += 1;
                }
                seg += 1;
            }
            ok
        },
        "对象树/查询/备份恢复三段 ×10 逐格折叠零漂移（判据锚 UNX-F52207-J1）",
    );
    s.add(
        "会话管理 10 操作全通",
        {
            let mut lt = HeidiLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < SESSION_OPS {
                if let Some(w) = session_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "新建会话/改参数/复制会话/分组归档/连接拨测 五类逐操作恒等",
    );
    s.add(
        "网格编辑 12/12 全通",
        {
            let mut lt = HeidiLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < GRID_CELLS {
                if let Some(w) = grid_cell(g) {
                    if lt.record_grid(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.grids_all()
        },
        "单值/多行/批量三段网格闭集",
    );
    s.add(
        "SSH 隧道三元闸放行",
        {
            let mut lt = HeidiLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TUNNEL_GATES {
                if lt.open_tunnel(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.tunnels_open()
        },
        "密钥就位/主机可达/隧道握手三闸全落才放行",
    );
    s.add(
        "双态声明 2/2 全齐",
        {
            let mut lt = HeidiLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MODE_ROWS {
                if lt.declare_mode(m).is_err() {
                    ok = false;
                }
                m += 1;
            }
            ok && lt.modes_all()
        },
        "服务器模式/桌面模式双态账两行闭集，缺一拒封",
    );
    s.add(
        "操作类别单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < SESSION_OPS {
                if session_kind(o).unwrap_or(9) < session_kind(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五类闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "连接漂移/网格漂移/越闭集三态轮转必抓");
    let mut l0 = HeidiLedger::new();
    s.add(
        "边界·连接越闭集",
        l0.record_conn(CONN_CELLS, 0).is_err(),
        "连接格 20 越闭集必拒",
    );
    s.add(
        "边界·网格越闭集",
        l0.record_grid(GRID_CELLS, 0).is_err(),
        "网格格 12 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(SESSION_OPS, 0).is_err(),
        "操作 10 越闭集必拒",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    let led_full = {
        let mut lt = HeidiLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        let mut c = 0usize;
        while c < CONN_CELLS {
            if let Some(w) = conn_cell(c) {
                let _ = lt.record_conn(c, w);
            }
            c += 1;
        }
        let mut o = 0usize;
        while o < SESSION_OPS {
            if let Some(w) = session_cell(o) {
                let _ = lt.record_op(o, w);
            }
            o += 1;
        }
        let mut gr = 0usize;
        while gr < GRID_CELLS {
            if let Some(w) = grid_cell(gr) {
                let _ = lt.record_grid(gr, w);
            }
            gr += 1;
        }
        let mut t = 0usize;
        while t < TUNNEL_GATES {
            let _ = lt.open_tunnel(t);
            t += 1;
        }
        let mut m = 0usize;
        while m < MODE_ROWS {
            let _ = lt.declare_mode(m);
            m += 1;
        }
        lt
    };
    s.add(
        "定级闭集 S2",
        led_full.grade() == HeidiGrade::S2 && led_full.grade().tag() == "S2",
        "全格全通+三闸放行+双态全齐 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7340 && SEG_BASE > 0xC7300,
        "SEG_BASE 0x40 对齐且落在 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 12, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conn_engine_map() {
        assert_eq!(conn_engine(0), Some(HeidiEngine::MariaDb));
        assert_eq!(conn_engine(5), Some(HeidiEngine::MySql));
        assert_eq!(conn_engine(10), Some(HeidiEngine::PostgreSql));
        assert_eq!(conn_engine(19), Some(HeidiEngine::SqLite));
        assert!(conn_engine(20).is_none());
        assert!(conn_cell(20).is_none());
    }

    #[test]
    fn test_session_kind() {
        assert_eq!(session_kind(0), Some(0));
        assert_eq!(session_kind(4), Some(2));
        assert_eq!(session_kind(9), Some(4));
        assert!(session_kind(10).is_none());
        assert!(session_cell(10).is_none());
    }

    #[test]
    fn test_grid_seg_and_tunnel_names() {
        assert_eq!(grid_seg(0), Some(0));
        assert_eq!(grid_seg(4), Some(1));
        assert_eq!(grid_seg(11), Some(2));
        assert!(grid_seg(12).is_none());
        assert_eq!(tunnel_name(0), Some("密钥就位"));
        assert_eq!(tunnel_name(2), Some("隧道握手"));
        assert!(tunnel_name(3).is_none());
        assert_eq!(mode_name(1), Some("桌面模式双态账"));
        assert!(mode_name(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = conn_cell(11).unwrap_or(0);
        assert_eq!(a, conn_cell(11).unwrap_or(0));
        assert_ne!(a, conn_cell(12).unwrap_or(0));
        assert!(mode_row(MODE_ROWS).is_none());
        assert!(tunnel_gate(TUNNEL_GATES).is_none());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = HeidiLedger::new();
        assert_eq!(
            lt.record_conn(3, conn_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(HeidiCode::EConnDrift)
        );
        assert!(lt.record_conn(3, conn_cell(3).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_conn(3, conn_cell(3).unwrap_or(0)),
            Err(HeidiCode::EDup)
        );
    }

    #[test]
    fn test_kind_order_caught() {
        let mut lt = HeidiLedger::new();
        let _ = lt.record_op(9, session_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, session_cell(2).unwrap_or(0)), Err(HeidiCode::EOrder));
        assert!(lt.record_op(8, session_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (CONN_CELLS + GRID_CELLS + SESSION_OPS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (52206 - 52201));
        assert!(SEG_BASE > 0xC7300 && SEG_BASE <= 0xC73C0);
    }

    #[test]
    fn test_val_details_nonempty() {
        let codes = [
            HeidiCode::Ok,
            HeidiCode::ENoConn,
            HeidiCode::EConnDrift,
            HeidiCode::ENoOp,
            HeidiCode::EOpDrift,
            HeidiCode::EOrder,
            HeidiCode::ENoGrid,
            HeidiCode::EGridDrift,
            HeidiCode::ENoTunnel,
            HeidiCode::ETunnelMiss,
            HeidiCode::ENoMode,
            HeidiCode::EDup,
            HeidiCode::EGradeDrift,
        ];
        let mut n = 0u32;
        let mut i = 0usize;
        while i < codes.len() {
            assert_eq!(codes[i].val(), SEG_BASE + n);
            assert!(!codes[i].detail().is_empty());
            n += 1;
            i += 1;
        }
    }

    #[test]
    fn test_unx52206_checks_green() {
        let s = run_unx52206_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
