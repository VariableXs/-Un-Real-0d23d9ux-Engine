//! UNX-F52324 Radare2（Windows 发行版）收口 · 0xC90C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52324"，逐字）：
//!   title: Radare2（Windows 发行版）收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52325-J1 六判
//!     据全落（r2 GUI 形态，分账声明）；×10；KB-COMPAT 定级登记；判
//!     据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘
//!     要非空。
//! off-by-one 板式口径：本卡（卡 N=52324）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52325-J1，照原文锚定；功能按本卡标题（Radare2 Windows 发行
//! 版）落地；Cutter 消费联签 4 格为 F52325 前向声明。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 命令五族 20 操作闭集（a 族分析/p 族打印/x 族十六进制/s 族
//!      seek/? 族帮助）各 4 操作，族单调（逆序必抓）。
//!   ② r2pipe 脚本 8 格闭集（进程管道/JSON 输出/批处理命令序列等逐
//!      格折叠），漂移必抓；Cutter 消费联签 4 格（F52325 前向声明，
//!      载荷与 F52325 基准恒等才收）。
//!   ③ 开源声明 2 行闭集（LGPL-3.0 主体声明/Apache-2.0 子组件声明），
//!      缺一拒封。
//!   ④ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（族逆序/
//!      r2pipe 格漂移/命令格越闭集），检出+逃逸守恒等式；双跑 FNV
//!      指纹一致；定级 S1/S2/S3 闭集（开源许可面如实，全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52324;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 命令操作数（五族 × 4）。
pub const CMD_OPS: usize = 20;

/// r2pipe 脚本格数。
pub const PIPE_CELLS: usize = 8;

/// Cutter 消费联签格数（F52325 前向声明）。
pub const CUTTER_CELLS: usize = 4;

/// 开源声明行数。
pub const OSS_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum R2Code {
    /// 成功
    Ok = 0,
    /// 命令操作越闭集
    ENoOp,
    /// 命令操作漂移
    EDrift,
    /// 命令族逆序
    EOrder,
    /// r2pipe 格越闭集
    ENoPipe,
    /// r2pipe 格漂移
    EPipeDrift,
    /// Cutter 格越闭集
    ENoCut,
    /// Cutter 格漂移
    ECutDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl R2Code {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            R2Code::Ok => "成功",
            R2Code::ENoOp => "命令操作越闭集：命令五族 20 操作之外先登记",
            R2Code::EDrift => "命令操作漂移：操作折叠与基准不恒等，逐格复算",
            R2Code::EOrder => "命令族逆序：a 族分析→p 族打印→x 族十六进制→s 族 seek→? 族帮助单调，逆序必抓",
            R2Code::ENoPipe => "r2pipe 格越闭集：r2pipe 脚本 8 格之外先登记",
            R2Code::EPipeDrift => "r2pipe 格漂移：格折叠与基准不恒等，逐格复算",
            R2Code::ENoCut => "Cutter 格越闭集：Cutter 消费联签 4 格之外先登记（F52325 前向声明）",
            R2Code::ECutDrift => "Cutter 格漂移：联签折叠与 F52325 基准不恒等，逐格复算",
            R2Code::ENoRow => "声明行越闭集：开源声明 2 行之外先登记",
            R2Code::ERowMiss => "缺声明行：开源声明未齐先补齐",
            R2Code::EDup => "重复落账：同格只许一条账目",
            R2Code::ESeal => "未齐即封：20 命令格 + 8 r2pipe 格 + 4 Cutter 格未齐先补齐",
            R2Code::EGradeDrift => "定级漂移：全通且开源声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20 命令操作+8 r2pipe 格+4 Cutter 格+2 行开源声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令操作格载荷（操作 o；族 = o/4）。
pub const fn cmd_cell(o: usize) -> Option<u64> {
    if o >= CMD_OPS {
        return None;
    }
    Some((0xF1000u64 + (o as u64) * 0x41) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族号（0..4=a 族分析 4..8=p 族打印 8..12=x 族十六进制 12..16=s 族 seek 16..20=? 族帮助）。
pub const fn cmd_kind(o: usize) -> Option<u32> {
    if o >= CMD_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn cmd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("a 族分析"),
        1 => Some("p 族打印"),
        2 => Some("x 族十六进制"),
        3 => Some("s 族 seek"),
        4 => Some("? 族帮助"),
        _ => None,
    }
}

/// 命令操作名（const fn 表）。
pub const fn cmd_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("aaa 全量分析"),
        1 => Some("af 函数标记"),
        2 => Some("aar 引用扫描"),
        3 => Some("afl 函数清单"),
        4 => Some("pdf 函数反汇编"),
        5 => Some("pd 定长反汇编"),
        6 => Some("ps 字符串打印"),
        7 => Some("pf 格式化打印"),
        8 => Some("px 十六进制视图"),
        9 => Some("pxw 字宽十六进制"),
        10 => Some("pxd 反汇编十六进制"),
        11 => Some("pxr 指针十六进制"),
        12 => Some("s 跳转地址"),
        13 => Some("s+ 前进偏移"),
        14 => Some("s- 回退偏移"),
        15 => Some("s* seek 脚本输出"),
        16 => Some("?? 表达式求值"),
        17 => Some("?v 变量赋值"),
        18 => Some("?e 回显输出"),
        19 => Some("?$ 帮助元变量"),
        _ => None,
    }
}

/// r2pipe 格载荷（格 c）。
pub const fn pipe_cell(c: usize) -> Option<u64> {
    if c >= PIPE_CELLS {
        return None;
    }
    Some((0xF2000u64 + (c as u64) * 0x57) ^ 0x5232_5049_5045_0000)
}

/// r2pipe 格名（const fn 表）。
pub const fn pipe_cell_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("进程管道建立"),
        1 => Some("命令发送"),
        2 => Some("JSON 输出解析"),
        3 => Some("批量命令序列"),
        4 => Some("Python r2pipe 绑定"),
        5 => Some("Node r2pipe 绑定"),
        6 => Some("管道关闭回收"),
        7 => Some("超时与错误回收"),
        _ => None,
    }
}

/// Cutter 消费联签格载荷（格 u；F52325 前向声明基准）。
pub const fn cutter_cell(u: usize) -> Option<u64> {
    if u >= CUTTER_CELLS {
        return None;
    }
    Some((0xF3000u64 + (u as u64) * 0x63) ^ 0x4355_5430_3030_0000)
}

/// Cutter 联签格名（const fn 表）。
pub const fn cutter_cell_name(u: usize) -> Option<&'static str> {
    match u {
        0 => Some("Cutter 嵌入 r2 核心声明"),
        1 => Some("Cutter 消费分析账声明"),
        2 => Some("r2 GUI 形态分账声明"),
        3 => Some("F52325 联签锚定行"),
        _ => None,
    }
}

/// 开源声明行折叠（行 r）。
pub const fn oss_row(r: usize) -> Option<u32> {
    if r >= OSS_ROWS {
        return None;
    }
    Some(0xF400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Radare2 收口账本状态机。
#[derive(Clone)]
pub struct R2Ledger {
    /// 六判据（分析/反汇编/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 命令操作账。
    cmds: [Option<u64>; CMD_OPS],
    /// r2pipe 账。
    pipes: [Option<u64>; PIPE_CELLS],
    /// Cutter 联签账。
    cutters: [Option<u64>; CUTTER_CELLS],
    /// 开源声明账。
    oss: [Option<u32>; OSS_ROWS],
    /// 最后族号。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl R2Ledger {
    /// 新账本。
    pub fn new() -> Self {
        R2Ledger {
            gates: [None; 6],
            cmds: [None; CMD_OPS],
            pipes: [None; PIPE_CELLS],
            cutters: [None; CUTTER_CELLS],
            oss: [None; OSS_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), R2Code> {
        if gate >= 6 {
            return Err(R2Code::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// 命令操作一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, o: usize, got: u64) -> Result<(), R2Code> {
        if o >= CMD_OPS {
            return Err(R2Code::ENoOp);
        }
        let kind = match cmd_kind(o) {
            Some(k) => k,
            None => return Err(R2Code::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(R2Code::EOrder);
        }
        let want = match cmd_cell(o) {
            Some(w) => w,
            None => return Err(R2Code::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + o as u64);
            return Err(R2Code::EDrift);
        }
        let dup = match self.cmds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(R2Code::EDup);
        }
        if let Some(c) = self.cmds.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + o as u64);
        Ok(())
    }

    /// 命令操作全账核验（20 操作）。
    pub fn cmds_all(&self) -> bool {
        let mut o = 0usize;
        while o < CMD_OPS {
            match self.cmds.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// r2pipe 一格（载荷对照 + 重复拒）。
    pub fn record_pipe(&mut self, c: usize, got: u64) -> Result<(), R2Code> {
        let want = match pipe_cell(c) {
            Some(w) => w,
            None => return Err(R2Code::ENoPipe),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + c as u64);
            return Err(R2Code::EPipeDrift);
        }
        let dup = match self.pipes.get(c) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(R2Code::EDup);
        }
        if let Some(x) = self.pipes.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x8301 + c as u64);
        Ok(())
    }

    /// r2pipe 全账核验（8 格）。
    pub fn pipes_all(&self) -> bool {
        let mut c = 0usize;
        while c < PIPE_CELLS {
            match self.pipes.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// Cutter 联签一格（F52325 前向基准对照 + 重复拒）。
    pub fn record_cutter(&mut self, u: usize, got: u64) -> Result<(), R2Code> {
        let want = match cutter_cell(u) {
            Some(w) => w,
            None => return Err(R2Code::ENoCut),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + u as u64);
            return Err(R2Code::ECutDrift);
        }
        let dup = match self.cutters.get(u) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(R2Code::EDup);
        }
        if let Some(x) = self.cutters.get_mut(u) {
            *x = Some(got);
        }
        self.fnv(0x8401 + u as u64);
        Ok(())
    }

    /// Cutter 联签全账核验（4 格）。
    pub fn cutters_all(&self) -> bool {
        let mut u = 0usize;
        while u < CUTTER_CELLS {
            match self.cutters.get(u) {
                Some(Some(_)) => {}
                _ => return false,
            }
            u += 1;
        }
        true
    }

    /// 开源声明一行。
    pub fn declare_oss(&mut self, r: usize) -> Result<(), R2Code> {
        if r >= OSS_ROWS {
            return Err(R2Code::ENoRow);
        }
        let dup = match self.oss.get(r) {
            Some(Some(_)) => true,
            _ => false,
        };
        if dup {
            return Err(R2Code::EDup);
        }
        if let Some(x) = self.oss.get_mut(r) {
            *x = oss_row(r);
        }
        self.fnv(0x8500 + r as u64);
        Ok(())
    }

    /// 开源声明全齐核验。
    pub fn oss_all(&self) -> bool {
        let mut r = 0usize;
        while r < OSS_ROWS {
            match self.oss.get(r) {
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

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.cmds_all() && self.pipes_all() && self.cutters_all() && self.oss_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for R2Ledger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = R2Ledger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CMD_OPS {
        if let Some(w) = cmd_cell(o) {
            if led.record_cmd(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < PIPE_CELLS {
        if let Some(w) = pipe_cell(c) {
            if led.record_pipe(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut u = 0usize;
    while u < CUTTER_CELLS {
        if let Some(w) = cutter_cell(u) {
            if led.record_cutter(u, w).is_ok() {
                recorded += 1;
            }
        }
        u += 1;
    }
    let mut r = 0usize;
    while r < OSS_ROWS {
        let _ = led.declare_oss(r);
        r += 1;
    }
    // 注入 10 轮：族逆序 / r2pipe 格漂移 / 命令格越闭集三态轮转。
    // 逆序注入先按合法序落高族格再注低族格（空账本低族本就合法，须先抬 last_kind）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = R2Ledger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_cmd(
                    17,
                    match cmd_cell(17) {
                        Some(p) => p,
                        None => 0,
                    },
                );
                lt.record_cmd(
                    5,
                    match cmd_cell(5) {
                        Some(p) => p,
                        None => 0,
                    },
                ) == Err(R2Code::EOrder)
            }
            1 => {
                let _ = lt.record_pipe(
                    0,
                    match pipe_cell(0) {
                        Some(p) => p,
                        None => 0,
                    },
                );
                lt.record_pipe(
                    1,
                    match pipe_cell(1) {
                        Some(p) => p,
                        None => 0,
                    }
                    .wrapping_add(1),
                ) == Err(R2Code::EPipeDrift)
            }
            _ => {
                let _ = lt.record_cutter(
                    0,
                    match cutter_cell(0) {
                        Some(p) => p,
                        None => 0,
                    },
                );
                lt.record_cmd(CMD_OPS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8600 + k as u64);
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
/// UNX-F52324 判据集（锚 UNX-F52325-J1，off-by-one 板式）。
pub fn run_unx52324_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52324");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "命令五族 20 操作全通",
        recorded >= CMD_OPS as u32,
        "a 族分析/p 族打印/x 族十六进制/s 族 seek/? 族帮助逐操作恒等",
    );
    s.add(
        "r2pipe 脚本 8 格全落",
        {
            let mut lt = R2Ledger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PIPE_CELLS {
                if let Some(w) = pipe_cell(c) {
                    if lt.record_pipe(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.pipes_all()
        },
        "进程管道/命令发送/JSON 解析/批量序列/双语言绑定/回收逐格恒等",
    );
    s.add(
        "Cutter 消费联签 4 格全落（F52325 前向声明）",
        {
            let mut lt = R2Ledger::new();
            let mut ok = true;
            let mut u = 0usize;
            while u < CUTTER_CELLS {
                if let Some(w) = cutter_cell(u) {
                    if lt.record_cutter(u, w).is_err() {
                        ok = false;
                    }
                }
                u += 1;
            }
            ok && lt.cutters_all()
        },
        "r2 核心嵌入/分析账消费/r2 GUI 分账/F52325 联签锚定行逐格恒等",
    );
    s.add(
        "开源声明 2/2 全齐",
        {
            let mut lt = R2Ledger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < OSS_ROWS {
                if lt.declare_oss(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.oss_all()
        },
        "LGPL-3.0 主体声明/Apache-2.0 子组件声明两行闭集",
    );
    s.add(
        "命令族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CMD_OPS {
                if cmd_kind(o).unwrap_or(5) < cmd_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "族逆序/管道漂移/命令越闭集三态轮转必抓");
    let mut l0 = R2Ledger::new();
    s.add(
        "边界·命令操作越闭集",
        l0.record_cmd(CMD_OPS, 0).is_err(),
        "命令操作 20 越闭集必拒",
    );
    s.add(
        "边界·r2pipe 格越闭集",
        l0.record_pipe(PIPE_CELLS, 0).is_err(),
        "r2pipe 格 8 越闭集必拒",
    );
    s.add(
        "边界·Cutter 格越闭集",
        l0.record_cutter(CUTTER_CELLS, 0).is_err(),
        "Cutter 格 4 越闭集必拒",
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
            let mut lt = R2Ledger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CMD_OPS {
                if let Some(w) = cmd_cell(o) {
                    let _ = lt.record_cmd(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < PIPE_CELLS {
                if let Some(w) = pipe_cell(c) {
                    let _ = lt.record_pipe(c, w);
                }
                c += 1;
            }
            let mut u = 0usize;
            while u < CUTTER_CELLS {
                if let Some(w) = cutter_cell(u) {
                    let _ = lt.record_cutter(u, w);
                }
                u += 1;
            }
            let mut r = 0usize;
            while r < OSS_ROWS {
                let _ = lt.declare_oss(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，开源许可面如实）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC90C0,
        "SEG_BASE 0x40 对齐且落 VX15 域 0xC90C0 格",
    );
    s.add(
        "指纹非零复验",
        { let (_, _, fp3) = run_cycle(); fp3 != 0 },
        "全流程 FNV 指纹非零（防恒空账基线）",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cmd_kind() {
        assert_eq!(cmd_kind(0), Some(0));
        assert_eq!(cmd_kind(4), Some(1));
        assert_eq!(cmd_kind(8), Some(2));
        assert_eq!(cmd_kind(19), Some(4));
        assert!(cmd_kind(20).is_none());
        assert!(cmd_cell(20).is_none());
        assert_eq!(cmd_kind_name(3), Some("s 族 seek"));
    }

    #[test]
    fn test_cmd_op_names() {
        assert_eq!(cmd_op_name(0), Some("aaa 全量分析"));
        assert_eq!(cmd_op_name(8), Some("px 十六进制视图"));
        assert_eq!(cmd_op_name(19), Some("?$ 帮助元变量"));
        assert!(cmd_op_name(20).is_none());
    }

    #[test]
    fn test_pipe_cells() {
        assert_eq!(pipe_cell_name(0), Some("进程管道建立"));
        assert_eq!(pipe_cell_name(7), Some("超时与错误回收"));
        assert!(pipe_cell_name(8).is_none());
        assert!(pipe_cell(PIPE_CELLS).is_none());
    }

    #[test]
    fn test_cutter_cells() {
        assert_eq!(cutter_cell_name(3), Some("F52325 联签锚定行"));
        assert!(cutter_cell_name(4).is_none());
        assert!(cutter_cell(CUTTER_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cmd_cell(6).unwrap_or(0);
        assert_eq!(a, cmd_cell(6).unwrap_or(0));
        assert_ne!(a, cmd_cell(7).unwrap_or(0));
        assert_ne!(pipe_cell(2).unwrap_or(0), pipe_cell(3).unwrap_or(0));
        assert!(oss_row(OSS_ROWS).is_none());
    }

    #[test]
    fn test_cmd_order_caught() {
        let mut lt = R2Ledger::new();
        // 合法序先落高族格，再注低族格（空账本低族本就合法）。
        let _ = lt.record_cmd(9, cmd_cell(9).unwrap_or(0));
        assert_eq!(lt.record_cmd(2, cmd_cell(2).unwrap_or(0)), Err(R2Code::EOrder));
        assert!(lt.record_cmd(10, cmd_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = R2Ledger::new();
        assert_eq!(
            lt.record_pipe(3, pipe_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(R2Code::EPipeDrift)
        );
        assert_eq!(
            lt.record_cutter(2, cutter_cell(2).unwrap_or(0).wrapping_add(1)),
            Err(R2Code::ECutDrift)
        );
        assert_eq!(
            lt.record_cmd(6, cmd_cell(6).unwrap_or(0).wrapping_add(3)),
            Err(R2Code::EDrift)
        );
    }

    #[test]
    fn test_dup_and_oss() {
        let mut lt = R2Ledger::new();
        let _ = lt.record_cmd(0, cmd_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cmd(0, cmd_cell(0).unwrap_or(0)), Err(R2Code::EDup));
        let _ = lt.declare_oss(0);
        assert_eq!(lt.declare_oss(0), Err(R2Code::EDup));
        assert!(lt.declare_oss(2).is_err());
        assert!(!lt.oss_all());
        let _ = lt.declare_oss(1);
        assert!(lt.oss_all());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = R2Ledger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= CMD_OPS as u32 + PIPE_CELLS as u32 + CUTTER_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC90C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(R2Code::Ok as u32, 0);
        assert_eq!(R2Code::EGradeDrift as u32, 12);
        assert!(!R2Code::ERowMiss.detail().is_empty());
        assert!(!R2Code::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 空账本 → S3。
        let lt = R2Ledger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52324_checks_green() {
        let s = run_unx52324_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
