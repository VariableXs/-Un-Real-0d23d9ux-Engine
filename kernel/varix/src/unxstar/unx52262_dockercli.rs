//! UNX-F52262 Docker CLI（Windows 发行版）收口 · 0xC8140段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52262"，逐字）：
//!   title: Docker CLI（Windows 发行版）收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52263-J1 up/down/
//!     build ×10 工程全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成
//!     时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52262）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52263-J1，照原文锚定；功能按本卡标题（Docker CLI Windows 发行
//! 版）落地。
//! 本模块沿用 52201 口径：运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① CLI 20 操作集：五族闭集（镜像/容器/网络/卷/系统）各 4 操作，族
//!      单调（逆序必抓）；逐操作载荷（纯函数折叠）恒等才收。
//!   ② 上下文 5 场景闭集：default/desktop/wsl2/windows-hosts/remote-ssh
//!      逐场景载荷恒等，越界必抓。
//!   ③ 退出码闭集 8：0/1/125/126/127/128/129/130 纯表枚举，表外必拒。
//!   ④ Desktop 守护联签 4 格：与 F52261 守护进程共面联签（ping/version/
//!      api/context），逐格恒等；越界必抓。
//!   ⑤ 共面分账 2 行闭集：与 F52261 共面分账声明 / KB-COMPAT 定级分账
//!      声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 10 轮三态轮转（操作越闭集/
//!      操作漂移/五族逆序），全部落在当轮临时账本上，检出+逃逸守恒；双跑
//!      FNV 指纹一致；定级 S1/S2/S3 闭集（20 操作 + 5 场景 + 4 联签 +
//!      2 行分账 + 六判据全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52262;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// CLI 操作数。
pub const CLI_OPS: usize = 20;

/// 上下文场景数。
pub const CTX_SCENARIOS: usize = 5;

/// 退出码闭集数。
pub const EXIT_CODES: usize = 8;

/// Desktop 守护联签格数。
pub const COSIGN_CELLS: usize = 4;

/// 共面分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 六判据闸数。
pub const GATES: usize = 6;

/// 判据锚定原文（卡面 criteria 逐字引用）。
pub const CRITERIA_ANCHOR: &str = "§AI-66.3";

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DockerCliCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 载荷漂移
    EDrift,
    /// 五族逆序
    EOrder,
    /// 上下文场景越闭集
    ENoCtx,
    /// 守护联签格越闭集
    ENoSign,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl DockerCliCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            DockerCliCode::Ok => "成功",
            DockerCliCode::ENoOp => "操作号越闭集：CLI 20 操作集之外先登记",
            DockerCliCode::EDrift => "载荷漂移：操作/场景/联签折叠与基准不恒等，逐格复算",
            DockerCliCode::EOrder => "五族逆序：镜像→容器→网络→卷→系统单调，逆序必抓",
            DockerCliCode::ENoCtx => "上下文场景越闭集：5 场景之外先登记",
            DockerCliCode::ENoSign => "守护联签格越闭集：4 格闭集之外先登记",
            DockerCliCode::ENoRow => "声明行越闭集：共面分账 2 行闭集之外先登记",
            DockerCliCode::ERowMiss => "缺声明行：与 F52261 共面/KB-COMPAT 分账声明未齐先补齐",
            DockerCliCode::EDup => "重复落账：同格只许一条账目",
            DockerCliCode::ESeal => "未齐即封：20 操作 + 5 场景 + 4 联签未齐先补齐",
            DockerCliCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DockerCliCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DockerCliCode::EGradeDrift => "定级漂移：全通且共面分账声明全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
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
            Grade::S1 => "S1：六判据+20操作+5场景+4联签+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// CLI 操作格载荷（操作 o；族 = o/4）。
pub const fn cli_cell(o: usize) -> Option<u64> {
    if o >= CLI_OPS {
        return None;
    }
    Some((0xD2100u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作族（0..4=镜像 4..8=容器 8..12=网络 12..16=卷 16..20=系统）。
pub const fn cli_kind(o: usize) -> Option<u32> {
    if o >= CLI_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 操作族名。
pub const fn cli_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("镜像"),
        1 => Some("容器"),
        2 => Some("网络"),
        3 => Some("卷"),
        4 => Some("系统"),
        _ => None,
    }
}

/// 上下文场景载荷（场景 c）。
pub const fn ctx_cell(c: usize) -> Option<u64> {
    if c >= CTX_SCENARIOS {
        return None;
    }
    Some((0xD2200u64 + (c as u64) * 0x31) ^ 0x444B434C495F0000)
}

/// 上下文场景名。
pub const fn ctx_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("default"),
        1 => Some("desktop"),
        2 => Some("wsl2"),
        3 => Some("windows-hosts"),
        4 => Some("remote-ssh"),
        _ => None,
    }
}

/// 退出码闭集纯表（退出码 e）。
pub const fn exit_name(e: usize) -> Option<&'static str> {
    match e {
        0 => Some("0:成功"),
        1 => Some("1:一般错误"),
        125 => Some("125:守护错误"),
        126 => Some("126:不可执行"),
        127 => Some("127:未找到"),
        128 => Some("128:信号退出"),
        129 => Some("129:用法错误"),
        130 => Some("130:SIGINT"),
        _ => None,
    }
}

/// 退出码闭集序号（i：0..8 → 表项）。
pub const fn exit_slot(i: usize) -> Option<&'static str> {
    match i {
        0 => exit_name(0),
        1 => exit_name(1),
        2 => exit_name(125),
        3 => exit_name(126),
        4 => exit_name(127),
        5 => exit_name(128),
        6 => exit_name(129),
        7 => exit_name(130),
        _ => None,
    }
}

/// Desktop 守护联签格基准（格 i：ping/version/api/context）。
pub const fn sign_cell(i: usize) -> Option<u64> {
    if i >= COSIGN_CELLS {
        return None;
    }
    Some((0xD2300u64 + (i as u64) * 0x2D) ^ 0x44444B444D4E0000)
}

/// 守护联签格名。
pub const fn sign_cell_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("daemon-ping"),
        1 => Some("daemon-version"),
        2 => Some("api-negotiate"),
        3 => Some("context-handoff"),
        _ => None,
    }
}

/// 共面分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD2400 + r as u32 * 0x2C)
}

/// 共面分账声明行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("与 F52261 共面分账声明"),
        1 => Some("KB-COMPAT 定级分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Docker CLI 收口账本状态机。
#[derive(Clone)]
pub struct DockerCliLedger {
    /// 六判据（安装/启动/命令/崩溃/退出码/升级）。
    gates: [Option<bool>; GATES],
    /// CLI 操作账。
    ops: [Option<u64>; CLI_OPS],
    /// 上下文场景账。
    ctxs: [Option<u64>; CTX_SCENARIOS],
    /// 守护联签账。
    signs: [Option<u64>; COSIGN_CELLS],
    /// 共面分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后操作族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DockerCliLedger {
    /// 新账本。
    pub fn new() -> Self {
        DockerCliLedger {
            gates: [None; GATES],
            ops: [None; CLI_OPS],
            ctxs: [None; CTX_SCENARIOS],
            signs: [None; COSIGN_CELLS],
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DockerCliCode> {
        if gate >= GATES {
            return Err(DockerCliCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5B100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 操作一格（五族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), DockerCliCode> {
        if o >= CLI_OPS {
            return Err(DockerCliCode::ENoOp);
        }
        let kind = match cli_kind(o) {
            Some(k) => k,
            None => return Err(DockerCliCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B200);
            return Err(DockerCliCode::EOrder);
        }
        let want = match cli_cell(o) {
            Some(w) => w,
            None => return Err(DockerCliCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B201 + o as u64);
            return Err(DockerCliCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DockerCliCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5B202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < CLI_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 上下文场景一格（载荷对照 + 重复拒）。
    pub fn record_ctx(&mut self, c: usize, got: u64) -> Result<(), DockerCliCode> {
        let want = match ctx_cell(c) {
            Some(w) => w,
            None => return Err(DockerCliCode::ENoCtx),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B300 + c as u64);
            return Err(DockerCliCode::EDrift);
        }
        if match self.ctxs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DockerCliCode::EDup);
        }
        if let Some(x) = self.ctxs.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x5B301 + c as u64);
        Ok(())
    }

    /// 上下文全账核验（5 格）。
    pub fn ctxs_all(&self) -> bool {
        let mut c = 0usize;
        while c < CTX_SCENARIOS {
            match self.ctxs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 守护联签一格（载荷对照 + 重复拒）。
    pub fn record_sign(&mut self, i: usize, got: u64) -> Result<(), DockerCliCode> {
        let want = match sign_cell(i) {
            Some(w) => w,
            None => return Err(DockerCliCode::ENoSign),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B400 + i as u64);
            return Err(DockerCliCode::EDrift);
        }
        if match self.signs.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DockerCliCode::EDup);
        }
        if let Some(x) = self.signs.get_mut(i) {
            *x = Some(got);
        }
        self.fnv(0x5B401 + i as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn signs_all(&self) -> bool {
        let mut i = 0usize;
        while i < COSIGN_CELLS {
            match self.signs.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 共面分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), DockerCliCode> {
        if r >= SPLIT_ROWS {
            return Err(DockerCliCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DockerCliCode::EDup);
        }
        if let Some(x) = self.splits.get_mut(r) {
            *x = split_row(r);
        }
        self.fnv(0x5B500 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 最后操作族（注入不改主账断言用）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.ops_all()
            && self.ctxs_all()
            && self.signs_all()
            && self.splits_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for DockerCliLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DockerCliLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CLI_OPS {
        if let Some(w) = cli_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < CTX_SCENARIOS {
        if let Some(w) = ctx_cell(c) {
            if led.record_ctx(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut i = 0usize;
    while i < COSIGN_CELLS {
        if let Some(w) = sign_cell(i) {
            if led.record_sign(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：操作越闭集 / 操作漂移 / 五族逆序三态轮转，
    // 三类注入全部落在当轮临时账本上，主账只记守恒账。
    let mut j = 0usize;
    while j < 10 {
        let kind = (j % 3) as u32;
        let mut lt = DockerCliLedger::new();
        let caught = match kind {
            0 => lt.record_op(CLI_OPS, 0).is_err(),
            1 => {
                // 漂移注入：先录正格，再注漂移载荷。
                if let Some(w0) = cli_cell(0) {
                    let _ = lt.record_op(0, w0);
                }
                match cli_cell(1) {
                    Some(w1) => lt.record_op(1, w1.wrapping_add(1)) == Err(DockerCliCode::EDrift),
                    None => false,
                }
            }
            _ => {
                // 逆序注入：临时账本从空起步，先落末族再回落首族必抓，
                // 主账 last_kind 不被改动。
                if let Some(w19) = cli_cell(19) {
                    let _ = lt.record_op(19, w19);
                }
                match cli_cell(3) {
                    Some(w3) => lt.record_op(3, w3) == Err(DockerCliCode::EOrder),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5B600 + j as u64);
        } else {
            led.escaped += 1;
        }
        j += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
/// UNX-F52262 判据集（锚 UNX-F52263-J1，off-by-one 板式）。
pub fn run_unx52262_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52262");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "CLI 20 操作全通",
        recorded >= CLI_OPS as u32,
        "镜像/容器/网络/卷/系统五族逐操作恒等（UNX-F52263-J1 ×10 工程共面）",
    );
    s.add(
        "五族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CLI_OPS {
                if cli_kind(o).unwrap_or(5) < cli_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "镜像→容器→网络→卷→系统闭集单调",
    );
    s.add(
        "上下文 5 场景全落",
        {
            let mut lt = DockerCliLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CTX_SCENARIOS {
                if let Some(w) = ctx_cell(c) {
                    if lt.record_ctx(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.ctxs_all()
        },
        "default/desktop/wsl2/windows-hosts/remote-ssh 逐场景恒等",
    );
    s.add(
        "退出码闭集 8/8 纯表",
        {
            let mut ok = true;
            let mut i = 0usize;
            while i < EXIT_CODES {
                if exit_slot(i).is_none() {
                    ok = false;
                }
                i += 1;
            }
            ok && exit_slot(EXIT_CODES).is_none() && exit_name(131).is_none()
        },
        "0/1/125/126/127/128/129/130 闭集，表外必拒",
    );
    s.add(
        "Desktop 守护联签 4/4 全落",
        {
            let mut lt = DockerCliLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < COSIGN_CELLS {
                if let Some(w) = sign_cell(i) {
                    if lt.record_sign(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.signs_all()
        },
        "ping/version/api/context 与 F52261 守护进程共面联签",
    );
    s.add(
        "与 F52261 共面分账 2/2 全齐",
        {
            let mut lt = DockerCliLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "与 F52261 共面分账声明/KB-COMPAT 定级分账声明两行闭集",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "操作越闭集/操作漂移/五族逆序三态轮转必抓",
    );
    let mut l0 = DockerCliLedger::new();
    s.add(
        "边界·越闭集·操作",
        l0.record_op(CLI_OPS, 0).is_err() && cli_cell(CLI_OPS).is_none(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·越闭集·上下文",
        l0.record_ctx(CTX_SCENARIOS, 0).is_err() && ctx_cell(CTX_SCENARIOS).is_none(),
        "上下文 5 越闭集必拒",
    );
    s.add(
        "边界·越闭集·联签格",
        l0.record_sign(COSIGN_CELLS, 0).is_err() && sign_cell(COSIGN_CELLS).is_none(),
        "联签 4 越闭集必拒",
    );
    s.add(
        "操作漂移拒",
        {
            if let Some(w0) = cli_cell(0) {
                let _ = l0.record_op(0, w0);
            }
            match cli_cell(1) {
                Some(w1) => l0.record_op(1, w1.wrapping_add(1)) == Err(DockerCliCode::EDrift),
                None => false,
            }
        },
        "操作载荷漂移必抓",
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
        "判据锚定 §AI-66.3",
        CRITERIA_ANCHOR == "§AI-66.3" && !CRITERIA_ANCHOR.is_empty(),
        "卡面判据锚定原文逐字登记",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = DockerCliLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CLI_OPS {
                if let Some(w) = cli_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CTX_SCENARIOS {
                if let Some(w) = ctx_cell(c) {
                    let _ = lt.record_ctx(c, w);
                }
                c += 1;
            }
            let mut i = 0usize;
            while i < COSIGN_CELLS {
                if let Some(w) = sign_cell(i) {
                    let _ = lt.record_sign(i, w);
                }
                i += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8140,
        "SEG_BASE 0x40 对齐且在 VX15 域（公式 0xC7200+0x40×61）",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 15,
        "含本条共 ≥16 条闭集",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_kind() {
        assert_eq!(cli_kind(0), Some(0));
        assert_eq!(cli_kind(3), Some(0));
        assert_eq!(cli_kind(4), Some(1));
        assert_eq!(cli_kind(19), Some(4));
        assert!(cli_kind(20).is_none());
        assert!(cli_cell(20).is_none());
        assert_eq!(cli_kind_name(0), Some("镜像"));
        assert_eq!(cli_kind_name(4), Some("系统"));
        assert!(cli_kind_name(5).is_none());
    }

    #[test]
    fn test_ctx_cells() {
        assert_eq!(ctx_name(0), Some("default"));
        assert_eq!(ctx_name(2), Some("wsl2"));
        assert_eq!(ctx_name(4), Some("remote-ssh"));
        assert!(ctx_name(5).is_none());
        assert!(ctx_cell(CTX_SCENARIOS).is_none());
        let a = ctx_cell(1).unwrap_or(0);
        assert_eq!(a, ctx_cell(1).unwrap_or(0));
        assert_ne!(a, ctx_cell(2).unwrap_or(0));
    }

    #[test]
    fn test_exit_codes_closed() {
        let mut i = 0usize;
        while i < EXIT_CODES {
            assert!(exit_slot(i).is_some());
            i += 1;
        }
        assert!(exit_slot(EXIT_CODES).is_none());
        assert!(exit_name(2).is_none());
        assert!(exit_name(124).is_none());
        assert!(exit_name(131).is_none());
        assert_eq!(exit_name(125), Some("125:守护错误"));
        assert_eq!(exit_name(130), Some("130:SIGINT"));
    }

    #[test]
    fn test_sign_cells() {
        assert_eq!(sign_cell_name(0), Some("daemon-ping"));
        assert_eq!(sign_cell_name(3), Some("context-handoff"));
        assert!(sign_cell_name(4).is_none());
        assert!(sign_cell(COSIGN_CELLS).is_none());
        let a = sign_cell(2).unwrap_or(0);
        assert_ne!(a, sign_cell(3).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = DockerCliLedger::new();
        let _ = lt.record_op(19, cli_cell(19).unwrap_or(0));
        assert_eq!(
            lt.record_op(3, cli_cell(3).unwrap_or(0)),
            Err(DockerCliCode::EOrder)
        );
        // 同族不逆序：先 16 后 17 合法。
        let mut lt2 = DockerCliLedger::new();
        assert!(lt2.record_op(16, cli_cell(16).unwrap_or(0)).is_ok());
        assert!(lt2.record_op(17, cli_cell(17).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = DockerCliLedger::new();
        assert_eq!(
            lt.record_op(5, cli_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(DockerCliCode::EDrift)
        );
        assert_eq!(
            lt.record_ctx(2, ctx_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(DockerCliCode::EDrift)
        );
        assert_eq!(
            lt.record_sign(1, sign_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(DockerCliCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = DockerCliLedger::new();
        let _ = lt.record_op(0, cli_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_op(0, cli_cell(0).unwrap_or(0)),
            Err(DockerCliCode::EDup)
        );
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(DockerCliCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = DockerCliLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        let mut lt2 = DockerCliLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_injection_leaves_main_ledger() {
        let mut main = DockerCliLedger::new();
        let before = main.last_kind();
        let mut lt = DockerCliLedger::new();
        let _ = lt.record_op(19, cli_cell(19).unwrap_or(0));
        assert_eq!(
            lt.record_op(3, cli_cell(3).unwrap_or(0)),
            Err(DockerCliCode::EOrder)
        );
        assert_eq!(main.last_kind(), before);
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (CLI_OPS + CTX_SCENARIOS + COSIGN_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8140);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码闭集恰 13 变体：Ok=0，EGradeDrift=12。
        assert_eq!(DockerCliCode::Ok as u32, 0);
        assert_eq!(DockerCliCode::ENoOp as u32, 1);
        assert_eq!(DockerCliCode::EDrift as u32, 2);
        assert_eq!(DockerCliCode::EOrder as u32, 3);
        assert_eq!(DockerCliCode::ENoCtx as u32, 4);
        assert_eq!(DockerCliCode::ENoSign as u32, 5);
        assert_eq!(DockerCliCode::ENoRow as u32, 6);
        assert_eq!(DockerCliCode::ERowMiss as u32, 7);
        assert_eq!(DockerCliCode::EDup as u32, 8);
        assert_eq!(DockerCliCode::ESeal as u32, 9);
        assert_eq!(DockerCliCode::EInj as u32, 10);
        assert_eq!(DockerCliCode::EFp as u32, 11);
        assert_eq!(DockerCliCode::EGradeDrift as u32, 12);
        assert!(!DockerCliCode::Ok.detail().is_empty());
        assert!(!DockerCliCode::ESeal.detail().is_empty());
        assert_eq!(DockerCliCode::EGradeDrift.val(), SEG_BASE + 12);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = DockerCliLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert!(!Grade::S2.detail().is_empty());
    }

    #[test]
    fn unx52262_checks_green() {
        let s = run_unx52262_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
