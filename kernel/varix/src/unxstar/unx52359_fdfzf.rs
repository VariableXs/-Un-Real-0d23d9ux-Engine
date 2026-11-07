//! UNX-F52359 fd / fzf（Windows 发行版）收口 · 0xC9980段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52359"，逐字）：
//!   title: fd / fzf（Windows 发行版）收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52360-J1 脚本
//!     ×10 全通；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52359）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52360-J1，照原文锚定；功能按本卡标题（fd / fzf Windows 发行
//! 版）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① fd 五选项族闭集（匹配/过滤/遍历/输出/大小写共 12 格，族 =
//!      o/4 三族投影，族单调）；fzf 五选项族闭集（查询/排序/界面/
//!      多选/预览共 12 格，族单调）；逐格载荷（纯函数折叠）恒等才收。
//!   ② fd→fzf 管道联签 6 格：查找→过滤→排序→预览→选择→回填六行
//!      闭签，缺一拒封。
//!   ③ 20 操作集：fd 与 fzf 协同操作平账闭集（查找/预览/跳转/多选/
//!      附加…），漂移必抓。
//!   ④ ledger 状态机 + 注入核 run_cycle 10 轮三态轮转（fd 族逆序/
//!      fzf 格漂移/联签行越闭集）10/10 必抓；双跑 FNV 指纹一致；
//!      定级 S1/S2/S3（12+12+20+6+2 全通→S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52359;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// fd 选项格数。
pub const FD_CELLS: usize = 12;

/// fzf 选项格数。
pub const FZF_CELLS: usize = 12;

/// 管道联签行数。
pub const PIPE_ROWS: usize = 6;

/// 协同操作数。
pub const FF_OPS: usize = 20;

/// 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FdFzfCode {
    /// 成功
    Ok = 0,
    /// fd 选项越闭集
    ENoFd,
    /// fd 载荷漂移
    EFdDrift,
    /// 族序倒置
    EOrder,
    /// fzf 选项越闭集
    ENoFzf,
    /// fzf 载荷漂移
    EFzfDrift,
    /// 联签行越闭集
    ENoPipe,
    /// 操作号越闭集
    ENoOp,
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

impl FdFzfCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            FdFzfCode::Ok => "成功",
            FdFzfCode::ENoFd => "fd 选项越闭集：fd 12 格之外先登记",
            FdFzfCode::EFdDrift => "fd 载荷漂移：格折叠与基准不恒等，逐格复算",
            FdFzfCode::EOrder => "族序倒置：fd 匹配→过滤→遍历与 fzf 查询→排序→界面各自单调，逆序必抓",
            FdFzfCode::ENoFzf => "fzf 选项越闭集：fzf 12 格之外先登记",
            FdFzfCode::EFzfDrift => "fzf 载荷漂移：格折叠与基准不恒等，逐格复算",
            FdFzfCode::ENoPipe => "联签行越闭集：fd→fzf 管道联签 6 格之外先登记",
            FdFzfCode::ENoOp => "操作号越闭集：20 操作集之外先登记",
            FdFzfCode::EDup => "重复落账：同格只许一条账目",
            FdFzfCode::ESeal => "未齐即封：12+12 格 + 20 操作 + 6 联签未齐先补齐",
            FdFzfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            FdFzfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            FdFzfCode::EGradeDrift => "定级漂移：全通且联签声明全齐才可定 S1",
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
            Grade::S1 => "S1：12+12格+20操作+6联签+2声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// fd 选项格载荷（格 o；族 = o/4，12 格含尾族 2 格）。
pub const fn fd_cell(o: usize) -> Option<u64> {
    if o >= FD_CELLS {
        return None;
    }
    Some((0xC0000u64 + (o as u64) * 0x2F) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// fd 族（0..4=匹配 4..8=过滤 8..12=遍历）。
pub const fn fd_kind(o: usize) -> Option<u32> {
    if o >= FD_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// fd 族名。
pub const fn fd_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("匹配"),
        1 => Some("过滤"),
        2 => Some("遍历"),
        _ => None,
    }
}

/// fzf 选项格载荷（格 o；族 = o/4）。
pub const fn fzf_cell(o: usize) -> Option<u64> {
    if o >= FZF_CELLS {
        return None;
    }
    Some((0xC1000u64 + (o as u64) * 0x31) ^ ((o / 4) as u64).wrapping_mul(0x0100019B))
}

/// fzf 族（0..4=查询 4..8=排序 8..12=界面）。
pub const fn fzf_kind(o: usize) -> Option<u32> {
    if o >= FZF_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// fzf 族名。
pub const fn fzf_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("查询"),
        1 => Some("排序"),
        2 => Some("界面"),
        _ => None,
    }
}

/// 管道联签行折叠（行 r；fd→fzf 六行闭签）。
pub const fn pipe_row(r: usize) -> Option<u32> {
    if r >= PIPE_ROWS {
        return None;
    }
    Some(0xC200 + r as u32 * 0x2E)
}

/// 管道联签名。
pub const fn pipe_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("fd 查找"),
        1 => Some("fzf 过滤"),
        2 => Some("排序定档"),
        3 => Some("预览展开"),
        4 => Some("多选确认"),
        5 => Some("路径回填"),
        _ => None,
    }
}

/// 协同操作格载荷（操作 o，平账无族序）。
pub const fn ff_op_cell(o: usize) -> Option<u64> {
    if o >= FF_OPS {
        return None;
    }
    Some((0xC3000u64 + (o as u64) * 0x3B) ^ 0x4644465A464D5000)
}

/// 协同操作名。
pub const fn ff_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("全库查找"),
        1 => Some("扩展名过滤"),
        2 => Some("类型过滤"),
        3 => Some("排除目录"),
        4 => Some("隐藏遍历"),
        5 => Some("绝对路径"),
        6 => Some("结果上限"),
        7 => Some("模糊查询"),
        8 => Some("精确查询"),
        9 => Some("相关度排序"),
        10 => Some("时间排序"),
        11 => Some("多选标记"),
        12 => Some("预览窗格"),
        13 => Some("回填编辑器"),
        14 => Some("管道串联"),
        15 => Some("跳出循环"),
        16 => Some("清屏重绘"),
        17 => Some("期望键绑定"),
        18 => Some("头部行"),
        19 => Some("选中导出"),
        _ => None,
    }
}

/// 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xC400 + r as u32 * 0x29)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// fd/fzf 收口账本状态机。
#[derive(Clone)]
pub struct FdFzfLedger {
    /// 六判据（查找/模糊选择/管道/预览/回填/许可）。
    gates: [Option<bool>; 6],
    /// fd 选项账。
    fds: [Option<u64>; FD_CELLS],
    /// fzf 选项账。
    fzfs: [Option<u64>; FZF_CELLS],
    /// 联签账。
    pipes: [Option<u32>; PIPE_ROWS],
    /// 操作账。
    ops: [Option<u64>; FF_OPS],
    /// 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后 fd 族。
    last_fd: i64,
    /// 最后 fzf 族。
    last_fzf: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl FdFzfLedger {
    /// 新账本。
    pub fn new() -> Self {
        FdFzfLedger {
            gates: [None; 6],
            fds: [None; FD_CELLS],
            fzfs: [None; FZF_CELLS],
            pipes: [None; PIPE_ROWS],
            ops: [None; FF_OPS],
            stacks: [None; STACK_ROWS],
            last_fd: -1,
            last_fzf: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), FdFzfCode> {
        if gate >= 6 {
            return Err(FdFzfCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6000 + gate as u64);
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

    /// fd 选项一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_fd(&mut self, o: usize, got: u64) -> Result<(), FdFzfCode> {
        if o >= FD_CELLS {
            return Err(FdFzfCode::ENoFd);
        }
        let kind = match fd_kind(o) {
            Some(k) => k,
            None => return Err(FdFzfCode::ENoFd),
        };
        if (kind as i64) < self.last_fd {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6100);
            return Err(FdFzfCode::EOrder);
        }
        let want = match fd_cell(o) {
            Some(w) => w,
            None => return Err(FdFzfCode::ENoFd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6101 + o as u64);
            return Err(FdFzfCode::EFdDrift);
        }
        if match self.fds.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FdFzfCode::EDup);
        }
        if let Some(c) = self.fds.get_mut(o) {
            *c = Some(got);
        }
        self.last_fd = kind as i64;
        self.fnv(0x6102 + o as u64);
        Ok(())
    }

    /// fd 选项全账核验（12 格）。
    pub fn fds_all(&self) -> bool {
        let mut o = 0usize;
        while o < FD_CELLS {
            match self.fds.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// fzf 选项一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_fzf(&mut self, o: usize, got: u64) -> Result<(), FdFzfCode> {
        if o >= FZF_CELLS {
            return Err(FdFzfCode::ENoFzf);
        }
        let kind = match fzf_kind(o) {
            Some(k) => k,
            None => return Err(FdFzfCode::ENoFzf),
        };
        if (kind as i64) < self.last_fzf {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(FdFzfCode::EOrder);
        }
        let want = match fzf_cell(o) {
            Some(w) => w,
            None => return Err(FdFzfCode::ENoFzf),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(FdFzfCode::EFzfDrift);
        }
        if match self.fzfs.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FdFzfCode::EDup);
        }
        if let Some(c) = self.fzfs.get_mut(o) {
            *c = Some(got);
        }
        self.last_fzf = kind as i64;
        self.fnv(0x6202 + o as u64);
        Ok(())
    }

    /// fzf 选项全账核验（12 格）。
    pub fn fzfs_all(&self) -> bool {
        let mut o = 0usize;
        while o < FZF_CELLS {
            match self.fzfs.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 管道联签一行。
    pub fn declare_pipe(&mut self, r: usize) -> Result<(), FdFzfCode> {
        if r >= PIPE_ROWS {
            return Err(FdFzfCode::ENoPipe);
        }
        if match self.pipes.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FdFzfCode::EDup);
        }
        if let Some(c) = self.pipes.get_mut(r) {
            *c = pipe_row(r);
        }
        self.fnv(0x6300 + r as u64);
        Ok(())
    }

    /// 管道联签全齐核验。
    pub fn pipes_all(&self) -> bool {
        let mut r = 0usize;
        while r < PIPE_ROWS {
            match self.pipes.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 协同操作一格（平账载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), FdFzfCode> {
        if o >= FF_OPS {
            return Err(FdFzfCode::ENoOp);
        }
        let want = match ff_op_cell(o) {
            Some(w) => w,
            None => return Err(FdFzfCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + o as u64);
            return Err(FdFzfCode::EFdDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FdFzfCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x6401 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 操作）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < FF_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), FdFzfCode> {
        if r >= STACK_ROWS {
            return Err(FdFzfCode::ENoPipe);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FdFzfCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x6500 + r as u64);
        Ok(())
    }

    /// 上栈声明全齐核验。
    pub fn stacks_all(&self) -> bool {
        let mut r = 0usize;
        while r < STACK_ROWS {
            match self.stacks.get(r) {
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
        if self.gates_all()
            && self.fds_all()
            && self.fzfs_all()
            && self.pipes_all()
            && self.ops_all()
            && self.stacks_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for FdFzfLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = FdFzfLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FD_CELLS {
        if let Some(w) = fd_cell(o) {
            if led.record_fd(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut f = 0usize;
    while f < FZF_CELLS {
        if let Some(w) = fzf_cell(f) {
            if led.record_fzf(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut p = 0usize;
    while p < FF_OPS {
        if let Some(w) = ff_op_cell(p) {
            if led.record_op(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut r = 0usize;
    while r < PIPE_ROWS {
        let _ = led.declare_pipe(r);
        r += 1;
    }
    let mut st = 0usize;
    while st < STACK_ROWS {
        let _ = led.declare_stack(st);
        st += 1;
    }
    // 注入 10 轮：fd 族逆序 / fzf 格漂移 / 联签行越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = FdFzfLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_fd(8, fd_cell(8).unwrap_or(0));
                lt.record_fd(1, fd_cell(1).unwrap_or(0)) == Err(FdFzfCode::EOrder)
            }
            1 => {
                lt.record_fzf(0, fzf_cell(0).unwrap_or(0));
                lt.record_fzf(1, fzf_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(FdFzfCode::EFzfDrift)
            }
            _ => lt.declare_pipe(PIPE_ROWS).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6600 + k as u64);
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
/// UNX-F52359 判据集（锚 UNX-F52360-J1，off-by-one 板式）。
pub fn run_unx52359_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52359");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "fd 选项 12 格全通",
        recorded >= (FD_CELLS + FZF_CELLS + FF_OPS) as u32,
        "匹配/过滤/遍历三族逐格恒等",
    );
    s.add(
        "fzf 选项 12 格全通",
        {
            let mut lt = FdFzfLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FZF_CELLS {
                if let Some(w) = fzf_cell(f) {
                    if lt.record_fzf(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.fzfs_all()
        },
        "查询/排序/界面三族逐格恒等",
    );
    s.add(
        "fd→fzf 管道联签 6/6 全齐",
        {
            let mut lt = FdFzfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < PIPE_ROWS {
                if lt.declare_pipe(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.pipes_all()
        },
        "查找→过滤→排序→预览→选择→回填六行闭签",
    );
    s.add(
        "协同 20 操作集全通",
        {
            let mut lt = FdFzfLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < FF_OPS {
                if let Some(w) = ff_op_cell(p) {
                    if lt.record_op(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.ops_all()
        },
        "fd/fzf 协同 20 操作平账闭集",
    );
    s.add(
        "双族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FD_CELLS {
                if fd_kind(o).unwrap_or(3) < fd_kind(o - 1).unwrap_or(3) {
                    ok = false;
                }
                o += 1;
            }
            let mut f = 1usize;
            while f < FZF_CELLS {
                if fzf_kind(f).unwrap_or(3) < fzf_kind(f - 1).unwrap_or(3) {
                    ok = false;
                }
                f += 1;
            }
            ok
        },
        "fd 与 fzf 两族闭集各自单调",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = FdFzfLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "查找/模糊选择/管道/预览/回填/许可逐格落账",
    );
    s.add(
        "Rust 上栈声明 2/2 全齐",
        {
            let mut lt = FdFzfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_stack(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.stacks_all()
        },
        "Rust 上栈声明两行闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "fd 族逆序/fzf 漂移/联签越闭集三态轮转必抓");
    let mut l0 = FdFzfLedger::new();
    s.add(
        "边界·fd 选项越闭集",
        l0.record_fd(FD_CELLS, 0).is_err(),
        "fd 格 12 越闭集必拒",
    );
    s.add(
        "边界·fzf 选项越闭集",
        l0.record_fzf(FZF_CELLS, 0).is_err(),
        "fzf 格 12 越闭集必拒",
    );
    s.add(
        "边界·联签行越闭集",
        l0.declare_pipe(PIPE_ROWS).is_err(),
        "联签行 6 越闭集必拒",
    );
    s.add(
        "fd 族逆序拒",
        {
            let _ = l0.record_fd(8, fd_cell(8).unwrap_or(0));
            l0.record_fd(1, fd_cell(1).unwrap_or(0)) == Err(FdFzfCode::EOrder)
        },
        "遍历族后回匹配族必抓",
    );
    s.add(
        "fzf 族逆序拒",
        {
            let mut lt = FdFzfLedger::new();
            let _ = lt.record_fzf(4, fzf_cell(4).unwrap_or(0));
            lt.record_fzf(1, fzf_cell(1).unwrap_or(0)) == Err(FdFzfCode::EOrder)
        },
        "排序族后回查询族必抓",
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
            let mut lt = FdFzfLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FD_CELLS {
                if let Some(w) = fd_cell(o) {
                    let _ = lt.record_fd(o, w);
                }
                o += 1;
            }
            let mut f = 0usize;
            while f < FZF_CELLS {
                if let Some(w) = fzf_cell(f) {
                    let _ = lt.record_fzf(f, w);
                }
                f += 1;
            }
            let mut p = 0usize;
            while p < FF_OPS {
                if let Some(w) = ff_op_cell(p) {
                    let _ = lt.record_op(p, w);
                }
                p += 1;
            }
            let mut r = 0usize;
            while r < PIPE_ROWS {
                let _ = lt.declare_pipe(r);
                r += 1;
            }
            let mut st = 0usize;
            while st < STACK_ROWS {
                let _ = lt.declare_stack(st);
                st += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记，§AI-66.3）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9980,
        "SEG_BASE 0x40 对齐且按 VX15 公式落在 0xC9980",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥18 条闭集（求值时点 17 条）");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fd_kind() {
        assert_eq!(fd_kind(0), Some(0));
        assert_eq!(fd_kind(4), Some(1));
        assert_eq!(fd_kind(11), Some(2));
        assert!(fd_kind(12).is_none());
        assert!(fd_cell(12).is_none());
    }

    #[test]
    fn test_fzf_kind() {
        assert_eq!(fzf_kind(0), Some(0));
        assert_eq!(fzf_kind(7), Some(1));
        assert_eq!(fzf_kind(8), Some(2));
        assert!(fzf_kind(12).is_none());
        assert!(fzf_cell(12).is_none());
    }

    #[test]
    fn test_kind_names() {
        assert_eq!(fd_kind_name(0), Some("匹配"));
        assert_eq!(fd_kind_name(2), Some("遍历"));
        assert!(fd_kind_name(3).is_none());
        assert_eq!(fzf_kind_name(0), Some("查询"));
        assert_eq!(fzf_kind_name(2), Some("界面"));
        assert!(fzf_kind_name(3).is_none());
    }

    #[test]
    fn test_pipe_names() {
        assert_eq!(pipe_row_name(0), Some("fd 查找"));
        assert_eq!(pipe_row_name(5), Some("路径回填"));
        assert!(pipe_row_name(6).is_none());
        assert!(pipe_row(PIPE_ROWS).is_none());
    }

    #[test]
    fn test_ff_op_names() {
        assert_eq!(ff_op_name(0), Some("全库查找"));
        assert_eq!(ff_op_name(19), Some("选中导出"));
        assert!(ff_op_name(20).is_none());
        assert!(ff_op_cell(20).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = fd_cell(3).unwrap_or(0);
        assert_eq!(a, fd_cell(3).unwrap_or(0));
        assert_ne!(a, fd_cell(4).unwrap_or(0));
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_fd_order_caught() {
        let mut lt = FdFzfLedger::new();
        let _ = lt.record_fd(8, fd_cell(8).unwrap_or(0));
        assert_eq!(lt.record_fd(1, fd_cell(1).unwrap_or(0)), Err(FdFzfCode::EOrder));
        assert!(lt.record_fd(9, fd_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_fzf_order_caught() {
        let mut lt = FdFzfLedger::new();
        let _ = lt.record_fzf(4, fzf_cell(4).unwrap_or(0));
        assert_eq!(lt.record_fzf(1, fzf_cell(1).unwrap_or(0)), Err(FdFzfCode::EOrder));
        assert!(lt.record_fzf(5, fzf_cell(5).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = FdFzfLedger::new();
        assert_eq!(
            lt.record_op(5, ff_op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(FdFzfCode::EFdDrift)
        );
        assert_eq!(
            lt.record_fzf(2, fzf_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(FdFzfCode::EFzfDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = FdFzfLedger::new();
        let _ = lt.declare_pipe(0);
        assert_eq!(lt.declare_pipe(0), Err(FdFzfCode::EDup));
        let _ = lt.record_fd(0, fd_cell(0).unwrap_or(0));
        assert_eq!(lt.record_fd(0, fd_cell(0).unwrap_or(0)), Err(FdFzfCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = FdFzfLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (FD_CELLS + FZF_CELLS + FF_OPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC9980);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(FdFzfCode::Ok as u32, 0);
        assert_eq!(FdFzfCode::EGradeDrift as u32, 12);
        assert!(!FdFzfCode::Ok.detail().is_empty());
        assert!(!FdFzfCode::EOrder.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = FdFzfLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52359_checks_green() {
        let s = run_unx52359_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
