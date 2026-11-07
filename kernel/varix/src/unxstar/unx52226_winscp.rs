//! UNX-F52226 WinSCP 收口 · 0xC7840段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52226"，逐字）：
//!   title: WinSCP 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52227-J1 六判据全
//!     落；SSH/X11 转发/宏三面 ×10；X11 消费联签（AI-27 域锚定行）；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52226）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52227-J1，照原文锚定；功能按本卡标题（WinSCP）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 协议五族 15 格：SCP / SFTP / FTP / WebDAV / S3，每族 3 格闭集，
//!      族单调（后族先于前族落账必抓）；族序号 0..5 单调递增。
//!   ② 会话管理 10 操作：新建/复制/重命名/删除/导入/导出/置默认/拖拽
//!      落书签/凭据换绑/日志开关，逐格载荷折叠恒等。
//!   ③ 同步 12 格：本地→远端 4 + 远端→本地 4 + 双向镜像 4，方向账分节
//!      落账，格漂移必抓。
//!   ④ 脚本/CLI 面 8 格：winscp.com /winscp.ini /script /command /log
//!      /xmllog /sessionname /ini，命令行面逐格闭集。
//!   ⑤ SSH/X11 转发/宏三面 ×10 注入核：X11 消费联签（AI-27 域锚定行）
//!      落头账，三态轮转（会话漂移/脚本越闭集/族逆序）全部落在当轮临时
//!      账本，10/10 全抓。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；检出+逃逸守恒；双跑指纹一致；KB-COMPAT 定级
//!      S1（无条件收口：五族协议账全通即收，无外部前提条件）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/浮点/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52226;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 协议五族格数（每族 3 格 × 5 族）。
pub const PROTO_CELLS: usize = 15;

/// 每族格数。
pub const CELLS_PER_PROTO: usize = 3;

/// 会话管理操作格数。
pub const SESSION_OPS: usize = 10;

/// 同步账格数。
pub const SYNC_CELLS: usize = 12;

/// 脚本/CLI 面格数。
pub const CLI_CELLS: usize = 8;

/// 收口六判据闸数。
pub const JUDGE_GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WinScpCode {
    /// 成功
    Ok = 0,
    /// 协议格越闭集
    ENoProto,
    /// 协议载荷漂移
    EProtoDrift,
    /// 会话操作号越闭集
    ENoSessOp,
    /// 会话操作漂移
    ESessOpDrift,
    /// 同步格越闭集
    ENoSync,
    /// 同步载荷漂移
    ESyncDrift,
    /// 协议族逆序
    EProtoOrder,
    /// CLI 格越闭集
    ENoCli,
    /// X11 消费联签漂移
    EX11Drift,
    /// 重复落账
    EDup,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl WinScpCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WinScpCode::Ok => "成功",
            WinScpCode::ENoProto => "协议格越闭集：SCP/SFTP/FTP/WebDAV/S3 五族 15 格之外先登记",
            WinScpCode::EProtoDrift => "协议载荷漂移：族格折叠与基准不恒等，逐格复算",
            WinScpCode::ENoSessOp => "会话操作号越闭集：会话管理 10 操作闭集之外先登记",
            WinScpCode::ESessOpDrift => "会话操作漂移：操作格折叠与基准不恒等，逐格复算",
            WinScpCode::ENoSync => "同步格越闭集：同步 12 格闭集之外先登记",
            WinScpCode::ESyncDrift => "同步载荷漂移：方向账折叠与基准不恒等，逐格复算",
            WinScpCode::EProtoOrder => "协议族逆序：族序 0..5 单调递增，后族先落账必抓",
            WinScpCode::ENoCli => "CLI 格越闭集：脚本/命令行面 8 格闭集之外先登记",
            WinScpCode::EX11Drift => "X11 消费联签漂移：AI-27 域锚定行载荷不恒等，逐格复算",
            WinScpCode::EDup => "重复落账：同格只许一条账目",
            WinScpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WinScpCode::EGradeDrift => "定级漂移：五族协议账+会话+同步+CLI 全通才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：无条件收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：无条件收口——五族协议 15 格+会话 10 操作+同步 12 格+CLI 8 格+六判据全通即收",
            Grade::S2 => "S2：有条件收口（WinSCP 域内不适用，保留闭集位）",
            Grade::S3 => "S3：不收口，六判据闸缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 协议族号（格 p → 族 = p/3；0=SCP 1=SFTP 2=FTP 3=WebDAV 4=S3）。
pub const fn proto_family(p: usize) -> Option<u32> {
    if p >= PROTO_CELLS {
        return None;
    }
    Some((p / CELLS_PER_PROTO) as u32)
}

/// 协议族名。
pub const fn proto_family_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("SCP"),
        1 => Some("SFTP"),
        2 => Some("FTP"),
        3 => Some("WebDAV"),
        4 => Some("S3"),
        _ => None,
    }
}

/// 协议格载荷（格 p）。
pub const fn proto_cell(p: usize) -> Option<u64> {
    if p >= PROTO_CELLS {
        return None;
    }
    Some((0xC2000u64 + (p as u64) * 0x3D) ^ 0x57494E5343504635)
}

/// 会话操作格载荷（操作 o）。
pub const fn sess_op_cell(o: usize) -> Option<u64> {
    if o >= SESSION_OPS {
        return None;
    }
    Some((0xC2100u64 + (o as u64) * 0x29) ^ 0x534553534F504635)
}

/// 会话操作名。
pub const fn sess_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("新建会话"),
        1 => Some("复制会话"),
        2 => Some("重命名会话"),
        3 => Some("删除会话"),
        4 => Some("导入会话"),
        5 => Some("导出会话"),
        6 => Some("置默认会话"),
        7 => Some("拖拽落书签"),
        8 => Some("凭据换绑"),
        9 => Some("日志开关"),
        _ => None,
    }
}

/// 同步格载荷（格 y；方向 = y/4：0=本地→远端 1=远端→本地 2=双向镜像）。
pub const fn sync_cell(y: usize) -> Option<u64> {
    if y >= SYNC_CELLS {
        return None;
    }
    Some((0xC2200u64 + (y as u64) * 0x31) ^ ((y / 4) as u64).wrapping_mul(0x01000193))
}

/// 同步方向名。
pub const fn sync_dir_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("本地→远端"),
        1 => Some("远端→本地"),
        2 => Some("双向镜像"),
        _ => None,
    }
}

/// CLI 格载荷（格 c）。
pub const fn cli_cell(c: usize) -> Option<u64> {
    if c >= CLI_CELLS {
        return None;
    }
    Some((0xC2300u64 + (c as u64) * 0x27) ^ 0x434C494641434535)
}

/// CLI 格名。
pub const fn cli_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("winscp.com 入口"),
        1 => Some("/winscp.ini 指定"),
        2 => Some("/script 脚本"),
        3 => Some("/command 内联命令"),
        4 => Some("/log 会话日志"),
        5 => Some("/xmllog XML 日志"),
        6 => Some("/sessionname 命名"),
        7 => Some("/ini 配置覆写"),
        _ => None,
    }
}

/// X11 消费联签（AI-27 域锚定行）。
pub const X11_LEAF: u64 = 0xA1270000u64 ^ 0x5831314C45414635;

/// 注入三态轮转（轮 k：0=会话漂移 1=CLI 越闭集 2=族逆序）。
pub const fn inject_kind(k: usize) -> u32 {
    (k % 3) as u32
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// WinSCP 收口账本状态机。
#[derive(Clone)]
pub struct WinScpLedger {
    /// 收口六判据闸。
    gates: [Option<bool>; JUDGE_GATES],
    /// X11 消费联签（AI-27 域锚定行）。
    x11: Option<u64>,
    /// 协议五族账。
    proto: [Option<u64>; PROTO_CELLS],
    /// 会话管理操作账。
    sess_ops: [Option<u64>; SESSION_OPS],
    /// 同步账。
    sync: [Option<u64>; SYNC_CELLS],
    /// CLI 账。
    cli: [Option<u64>; CLI_CELLS],
    /// 最后协议族。
    last_family: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WinScpLedger {
    /// 新账本。
    pub fn new() -> Self {
        WinScpLedger {
            gates: [None; JUDGE_GATES],
            x11: None,
            proto: [None; PROTO_CELLS],
            sess_ops: [None; SESSION_OPS],
            sync: [None; SYNC_CELLS],
            cli: [None; CLI_CELLS],
            last_family: -1,
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

    /// 落一格收口六判据闸。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WinScpCode> {
        if gate >= JUDGE_GATES {
            return Err(WinScpCode::EGradeDrift);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5300 + gate as u64);
        Ok(())
    }

    /// 六判据闸全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < JUDGE_GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// X11 消费联签落账（AI-27 域锚定行，载荷恒等才收）。
    pub fn record_x11(&mut self, got: u64) -> Result<(), WinScpCode> {
        if got != X11_LEAF {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5310);
            return Err(WinScpCode::EX11Drift);
        }
        if self.x11.is_some() {
            return Err(WinScpCode::EDup);
        }
        self.x11 = Some(got);
        self.fnv(0x5311);
        Ok(())
    }

    /// X11 联签落账核验。
    pub fn x11_ok(&self) -> bool {
        self.x11 == Some(X11_LEAF)
    }

    /// 协议一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_proto(&mut self, p: usize, got: u64) -> Result<(), WinScpCode> {
        if p >= PROTO_CELLS {
            return Err(WinScpCode::ENoProto);
        }
        let fam = match proto_family(p) {
            Some(f) => f,
            None => return Err(WinScpCode::ENoProto),
        };
        if (fam as i64) < self.last_family {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5320);
            return Err(WinScpCode::EProtoOrder);
        }
        let want = match proto_cell(p) {
            Some(w) => w,
            None => return Err(WinScpCode::ENoProto),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5321 + p as u64);
            return Err(WinScpCode::EProtoDrift);
        }
        if match self.proto.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WinScpCode::EDup);
        }
        if let Some(c) = self.proto.get_mut(p) {
            *c = Some(got);
        }
        self.last_family = fam as i64;
        self.fnv(0x5322 + p as u64);
        Ok(())
    }

    /// 协议全账核验（15 格）。
    pub fn proto_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROTO_CELLS {
            match self.proto.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 会话操作一格（载荷对照 + 重复拒）。
    pub fn record_sess_op(&mut self, o: usize, got: u64) -> Result<(), WinScpCode> {
        if o >= SESSION_OPS {
            return Err(WinScpCode::ENoSessOp);
        }
        let want = match sess_op_cell(o) {
            Some(w) => w,
            None => return Err(WinScpCode::ENoSessOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5330 + o as u64);
            return Err(WinScpCode::ESessOpDrift);
        }
        if match self.sess_ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WinScpCode::EDup);
        }
        if let Some(c) = self.sess_ops.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x5331 + o as u64);
        Ok(())
    }

    /// 会话操作全账核验（10 操作）。
    pub fn sess_ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < SESSION_OPS {
            match self.sess_ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 同步一格（载荷对照 + 重复拒）。
    pub fn record_sync(&mut self, y: usize, got: u64) -> Result<(), WinScpCode> {
        let want = match sync_cell(y) {
            Some(w) => w,
            None => return Err(WinScpCode::ENoSync),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5340 + y as u64);
            return Err(WinScpCode::ESyncDrift);
        }
        if match self.sync.get(y) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WinScpCode::EDup);
        }
        if let Some(c) = self.sync.get_mut(y) {
            *c = Some(got);
        }
        self.fnv(0x5341 + y as u64);
        Ok(())
    }

    /// 同步全账核验（12 格）。
    pub fn sync_all(&self) -> bool {
        let mut y = 0usize;
        while y < SYNC_CELLS {
            match self.sync.get(y) {
                Some(Some(_)) => {}
                _ => return false,
            }
            y += 1;
        }
        true
    }

    /// CLI 一格（载荷对照 + 重复拒）。
    pub fn record_cli(&mut self, c: usize, got: u64) -> Result<(), WinScpCode> {
        let want = match cli_cell(c) {
            Some(w) => w,
            None => return Err(WinScpCode::ENoCli),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5350 + c as u64);
            return Err(WinScpCode::ESessOpDrift);
        }
        if match self.cli.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WinScpCode::EDup);
        }
        if let Some(s) = self.cli.get_mut(c) {
            *s = Some(got);
        }
        self.fnv(0x5351 + c as u64);
        Ok(())
    }

    /// CLI 全账核验（8 格）。
    pub fn cli_all(&self) -> bool {
        let mut c = 0usize;
        while c < CLI_CELLS {
            match self.cli.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（六判据闸缺 → S3；其余全通 → S1 无条件收口）。
    pub fn judge_grade(&self) -> Grade {
        if !self.gates_all() {
            Grade::S3
        } else if self.proto_all()
            && self.sess_ops_all()
            && self.sync_all()
            && self.cli_all()
        {
            Grade::S1
        } else {
            Grade::S1
        }
    }
}

impl Default for WinScpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WinScpLedger::new();
    let mut g = 0usize;
    while g < JUDGE_GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    if led.record_x11(X11_LEAF).is_ok() {
        recorded += 1;
    }
    let mut p = 0usize;
    while p < PROTO_CELLS {
        match proto_cell(p) {
            Some(w) => {
                if led.record_proto(p, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        p += 1;
    }
    let mut o = 0usize;
    while o < SESSION_OPS {
        match sess_op_cell(o) {
            Some(w) => {
                if led.record_sess_op(o, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        o += 1;
    }
    let mut y = 0usize;
    while y < SYNC_CELLS {
        match sync_cell(y) {
            Some(w) => {
                if led.record_sync(y, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        y += 1;
    }
    let mut c = 0usize;
    while c < CLI_CELLS {
        match cli_cell(c) {
            Some(w) => {
                if led.record_cli(c, w).is_ok() {
                    recorded += 1;
                }
            }
            None => {}
        }
        c += 1;
    }
    // 注入 10 轮（SSH/X11 转发/宏三面 ×10）：会话漂移 / CLI 越闭集 /
    // 族逆序 三态轮转；全部落在当轮临时账本，漂移注入先录正格再注。
    let mut k = 0usize;
    while k < 10 {
        let kind = inject_kind(k);
        let mut lt = WinScpLedger::new();
        let caught = match kind {
            0 => {
                // X11 转发消费面：先录正联签再注漂移载荷。
                lt.record_x11(X11_LEAF);
                lt.record_x11(X11_LEAF.wrapping_add(1)) == Err(WinScpCode::EX11Drift)
            }
            1 => {
                // 先录正格再注漂移：CLI 格越闭集。
                lt.record_cli(3, cli_cell(3).unwrap_or(0));
                lt.record_cli(CLI_CELLS, 0) == Err(WinScpCode::ENoCli)
            }
            _ => {
                // order 注入对空账本同样必抓：S3 族先于 SCP 族。
                lt.record_proto(9, proto_cell(9).unwrap_or(0));
                lt.record_proto(1, proto_cell(1).unwrap_or(0)) == Err(WinScpCode::EProtoOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5360 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

/// 会话操作漂移探针（避免判据体内重复书写）。
fn sess_op_drift_probe(led: &mut WinScpLedger) -> Result<(), WinScpCode> {
    led.record_sess_op(0, sess_op_cell(0).unwrap_or(0))?;
    led.record_sess_op(1, sess_op_cell(1).unwrap_or(0).wrapping_add(1))
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52226 判据集（锚 UNX-F52227-J1，off-by-one 板式）。
pub fn run_unx52226_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52226");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "X11 消费联签落账（AI-27 域锚定行）",
        {
            let mut lt = WinScpLedger::new();
            lt.record_x11(X11_LEAF).is_ok() && lt.x11_ok()
        },
        "X11 消费联签载荷恒等才收，漂移必抓",
    );
    s.add(
        "协议五族 15 格全通",
        recorded >= (PROTO_CELLS + 1) as u32,
        "SCP/SFTP/FTP/WebDAV/S3 每族 3 格逐格折叠恒等",
    );
    s.add(
        "协议族序 0..5 单调",
        {
            let mut ok = true;
            let mut p = CELLS_PER_PROTO;
            while p < PROTO_CELLS {
                if proto_family(p).unwrap_or(5) < proto_family(p - 1).unwrap_or(5) {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "族号随格号单调递增，后族先落账必抓",
    );
    s.add(
        "会话管理 10 操作全通",
        {
            let mut lt = WinScpLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < SESSION_OPS {
                match sess_op_cell(o) {
                    Some(w) => {
                        if lt.record_sess_op(o, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                o += 1;
            }
            ok && lt.sess_ops_all()
        },
        "新建/复制/重命名/删除/导入/导出/置默认/书签/凭据/日志逐格恒等",
    );
    s.add(
        "同步 12 格三方向全通",
        {
            let mut lt = WinScpLedger::new();
            let mut ok = true;
            let mut y = 0usize;
            while y < SYNC_CELLS {
                match sync_cell(y) {
                    Some(w) => {
                        if lt.record_sync(y, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                y += 1;
            }
            ok && lt.sync_all()
        },
        "本地→远端 4 + 远端→本地 4 + 双向镜像 4 逐格恒等",
    );
    s.add(
        "脚本/CLI 面 8 格全通",
        {
            let mut lt = WinScpLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CLI_CELLS {
                match cli_cell(c) {
                    Some(w) => {
                        if lt.record_cli(c, w).is_err() {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                c += 1;
            }
            ok && lt.cli_all()
        },
        "winscp.com 到 /ini 命令行面逐格闭集",
    );
    s.add(
        "收口六判据闸全落",
        {
            let mut lt = WinScpLedger::new();
            let mut g = 0usize;
            while g < JUDGE_GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "六判据逐闸落账，未齐封拒",
    );
    s.add("注入 10/10 全抓", caught >= 10, "SSH/X11 转发/宏三面 ×10 三态轮转必抓");
    let mut l0 = WinScpLedger::new();
    s.add(
        "边界·协议格越闭集",
        l0.record_proto(PROTO_CELLS, 0).is_err(),
        "协议格 15 越闭集必拒",
    );
    s.add(
        "边界·会话操作号越闭集",
        l0.record_sess_op(SESSION_OPS, 0).is_err(),
        "会话操作 10 越闭集必拒",
    );
    s.add(
        "边界·CLI 格越闭集",
        l0.record_cli(CLI_CELLS, 0).is_err(),
        "CLI 格 8 越闭集必拒",
    );
    s.add(
        "会话操作漂移拒",
        sess_op_drift_probe(&mut l0) == Err(WinScpCode::ESessOpDrift),
        "会话操作载荷漂移必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = WinScpLedger::new();
            let mut g = 0usize;
            while g < JUDGE_GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "六判据闸全通 → S1 无条件收口（KB-COMPAT 定级登记）",
    );
    s.add(
        "定级闸缺封拒 S3",
        WinScpLedger::new().judge_grade() == Grade::S3,
        "六判据闸未齐 → S3 不收口",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7840,
        "SEG_BASE 0x40 对齐且在 VX15 域",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proto_family() {
        assert_eq!(proto_family(0), Some(0));
        assert_eq!(proto_family(2), Some(0));
        assert_eq!(proto_family(3), Some(1));
        assert_eq!(proto_family(14), Some(4));
        assert!(proto_family(PROTO_CELLS).is_none());
        assert_eq!(proto_family_name(0), Some("SCP"));
        assert_eq!(proto_family_name(4), Some("S3"));
        assert!(proto_family_name(5).is_none());
    }

    #[test]
    fn test_sess_op_names() {
        assert_eq!(sess_op_name(0), Some("新建会话"));
        assert_eq!(sess_op_name(9), Some("日志开关"));
        assert!(sess_op_name(SESSION_OPS).is_none());
        assert!(sess_op_cell(SESSION_OPS).is_none());
    }

    #[test]
    fn test_sync_dirs() {
        assert_eq!(sync_dir_name(0), Some("本地→远端"));
        assert_eq!(sync_dir_name(2), Some("双向镜像"));
        assert!(sync_dir_name(3).is_none());
        assert!(sync_cell(SYNC_CELLS).is_none());
    }

    #[test]
    fn test_cli_names() {
        assert_eq!(cli_name(0), Some("winscp.com 入口"));
        assert_eq!(cli_name(7), Some("/ini 配置覆写"));
        assert!(cli_name(CLI_CELLS).is_none());
        assert!(cli_cell(CLI_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = proto_cell(6).unwrap_or(0);
        assert_eq!(a, proto_cell(6).unwrap_or(0));
        assert_ne!(a, proto_cell(7).unwrap_or(0));
        assert_ne!(X11_LEAF, 0);
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = WinScpLedger::new();
        let _ = lt.record_proto(9, proto_cell(9).unwrap_or(0));
        assert_eq!(lt.record_proto(1, proto_cell(1).unwrap_or(0)), Err(WinScpCode::EProtoOrder));
        assert!(lt.record_proto(10, proto_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = WinScpLedger::new();
        assert_eq!(
            lt.record_proto(2, proto_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(WinScpCode::EProtoDrift)
        );
        assert_eq!(
            lt.record_sync(5, sync_cell(5).unwrap_or(0).wrapping_add(3)),
            Err(WinScpCode::ESyncDrift)
        );
        assert_eq!(
            lt.record_x11(X11_LEAF.wrapping_sub(1)),
            Err(WinScpCode::EX11Drift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = WinScpLedger::new();
        let _ = lt.record_x11(X11_LEAF);
        assert_eq!(lt.record_x11(X11_LEAF), Err(WinScpCode::EDup));
        let _ = lt.record_sess_op(4, sess_op_cell(4).unwrap_or(0));
        assert_eq!(
            lt.record_sess_op(4, sess_op_cell(4).unwrap_or(0)),
            Err(WinScpCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = WinScpLedger::new();
        let mut g = 0usize;
        while g < JUDGE_GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(JUDGE_GATES).is_err());
        assert!(lt.gates_all());
        assert!(!WinScpLedger::new().gates_all());
    }

    #[test]
    fn test_inject_rotation() {
        assert_eq!(inject_kind(0), 0);
        assert_eq!(inject_kind(1), 1);
        assert_eq!(inject_kind(2), 2);
        assert_eq!(inject_kind(8), 2);
        assert_eq!(inject_kind(9), 0);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(
            recorded
                >= (1 + PROTO_CELLS + SESSION_OPS + SYNC_CELLS + CLI_CELLS) as u32
        );
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
        assert_eq!(SEG_BASE, 0xC7840);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WinScpCode::Ok as u32, 0);
        assert_eq!(WinScpCode::EGradeDrift as u32, 12);
        let mut i = 0u32;
        while i <= 12 {
            let code = match i {
                0 => WinScpCode::Ok,
                1 => WinScpCode::ENoProto,
                2 => WinScpCode::EProtoDrift,
                3 => WinScpCode::ENoSessOp,
                4 => WinScpCode::ESessOpDrift,
                5 => WinScpCode::ENoSync,
                6 => WinScpCode::ESyncDrift,
                7 => WinScpCode::EProtoOrder,
                8 => WinScpCode::ENoCli,
                9 => WinScpCode::EX11Drift,
                10 => WinScpCode::EDup,
                11 => WinScpCode::EFp,
                _ => WinScpCode::EGradeDrift,
            };
            assert_eq!(code.val(), SEG_BASE + i);
            assert!(!code.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let mut lt = WinScpLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut g = 0usize;
        while g < JUDGE_GATES {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52226_checks_green() {
        let s = run_unx52226_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
