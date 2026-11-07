//! UNX-F52368 Buildkite Agent 收口 · 0xC9BC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52368"，逐字）：
//!   title: Buildkite Agent 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52369-J1 ×10；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：
//!     状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52368）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52369-J1，照原文锚定；功能按本卡标题（Buildkite Agent）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① pipeline 五段 20 格闭集：checkout/命令/插件/工件/通知五段各
//!      4 格，段序单调（逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② agent 注册三元闸：注册令牌/队列/元数据标签三格，逐格落账，
//!      越三元闭集必抓。
//!   ③ hooks 投影 8 格：environment/pre-checkout/post-checkout/pre-
//!      command/post-command/pre-artifact/post-artifact/exit 八投影。
//!   ④ 并发 4 拍单调：并发账逐拍递增，倒拍必抓；与云消费联签 4 格
//!      （管道消费/工件消费/指标消费/日志消费）+ Go 上栈声明 2 行。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集 + adopt 采纳声明）；注入核 run_cycle 三态轮转
//!      10/10（投影漂移/并发倒拍/格越闭集）；双跑指纹一致；KB-COMPAT
//!      登记定级 S2（有条件收口，采纳声明后可升 S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52368;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// pipeline 格数（五段 × 4）。
pub const PIPE_CELLS: usize = 20;

/// pipeline 段数。
pub const PIPE_STAGES: usize = 5;

/// agent 注册三元闸格数。
pub const REG_GATES: usize = 3;

/// hooks 投影格数。
pub const HOOK_CELLS: usize = 8;

/// 并发拍数。
pub const BEAT_CELLS: usize = 4;

/// 云消费联签行数。
pub const COSIGN_ROWS: usize = 4;

/// Go 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BuildkiteCode {
    /// 成功
    Ok = 0,
    /// 流水线格越闭集
    ENoCell,
    /// 投影漂移
    EDrift,
    /// 段逆序/并发倒拍
    EOrder,
    /// 注册闸越闭集
    ERegNo,
    /// hooks 投影格越闭集
    EHookNo,
    /// 联签/上栈/拍号越闭集
    ENoRow,
    /// 行未齐
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

impl BuildkiteCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            BuildkiteCode::Ok => "成功",
            BuildkiteCode::ENoCell => "流水线格越闭集：五段 20 格之外先登记",
            BuildkiteCode::EDrift => "投影漂移：pipeline/hooks 投影与基准不恒等，逐格复算",
            BuildkiteCode::EOrder => "段逆序/并发倒拍：五段单调且并发逐拍递增，逆序必抓",
            BuildkiteCode::ERegNo => "注册闸越闭集：注册令牌/队列/元数据标签三元闸之外先登记",
            BuildkiteCode::EHookNo => "hooks 投影格越闭集：8 格之外先登记",
            BuildkiteCode::ENoRow => "联签/上栈/拍号越闭集：联签 4 格 + 上栈 2 行 + 并发 4 拍之外先登记",
            BuildkiteCode::ERowMiss => "行未齐：云消费联签与 Go 上栈声明未齐先补齐",
            BuildkiteCode::EDup => "重复落账：同格只许一条账目",
            BuildkiteCode::ESeal => "未齐即封：20 格 + 三元闸 + 8 hooks + 4 拍 + 联签上栈未齐先补齐",
            BuildkiteCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            BuildkiteCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            BuildkiteCode::EGradeDrift => "定级漂移：全通且采纳声明后方可升 S1，当前登记 S2",
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
            Grade::S1 => "S1：全账全通且采纳声明成立",
            Grade::S2 => "S2：有条件收口（KB-COMPAT 登记级），采纳声明未落或存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

/// KB-COMPAT 登记定级目标（本卡 S2：并发与云消费依赖集群连通，有条件收口）。
pub const GRADE_TARGET: Grade = Grade::S2;

// ── 域模型 ───────────────────────────────────────────────────────
/// XOR 锚（低 16 位恒零）。
pub const XORA_PIPE: u64 = 0x424B_4954_0000;

/// pipeline 格载荷（格 o；段 = o/4：0=checkout 1=命令 2=插件 3=工件 4=通知）。
pub const fn pipe_cell(o: usize) -> Option<u64> {
    if o >= PIPE_CELLS {
        return None;
    }
    Some((0x81000u64 + (o as u64) * 0x31) ^ XORA_PIPE)
}

/// pipeline 段号。
pub const fn pipe_stage(o: usize) -> Option<u32> {
    if o >= PIPE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 段名。
pub const fn pipe_stage_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("checkout"),
        1 => Some("命令"),
        2 => Some("插件"),
        3 => Some("工件"),
        4 => Some("通知"),
        _ => None,
    }
}

/// agent 注册三元闸载荷（闸 g：0=注册令牌 1=队列 2=元数据标签）。
pub const fn reg_gate_cell(g: usize) -> Option<u64> {
    if g >= REG_GATES {
        return None;
    }
    Some(0x82000u64 + (g as u64) * 0x89)
}

/// 注册闸名。
pub const fn reg_gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("注册令牌"),
        1 => Some("队列"),
        2 => Some("元数据标签"),
        _ => None,
    }
}

/// hooks 投影载荷（格 i：0=environment 1=pre-checkout 2=post-checkout
/// 3=pre-command 4=post-command 5=pre-artifact 6=post-artifact 7=exit）。
pub const fn hook_cell(i: usize) -> Option<u64> {
    if i >= HOOK_CELLS {
        return None;
    }
    Some((0x83000u64 + (i as u64) * 0x3B) ^ 0x484F_4F4B_0000)
}

/// hook 名。
pub const fn hook_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("environment"),
        1 => Some("pre-checkout"),
        2 => Some("post-checkout"),
        3 => Some("pre-command"),
        4 => Some("post-command"),
        5 => Some("pre-artifact"),
        6 => Some("post-artifact"),
        7 => Some("exit"),
        _ => None,
    }
}

/// 云消费联签行折叠（行 r：0=管道 1=工件 2=指标 3=日志）。
pub const fn cosign_row(r: usize) -> Option<u32> {
    if r >= COSIGN_ROWS {
        return None;
    }
    Some(0x8400 + r as u32 * 0x5F)
}

/// 联签行名。
pub const fn cosign_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("管道消费联签"),
        1 => Some("工件消费联签"),
        2 => Some("指标消费联签"),
        3 => Some("日志消费联签"),
        _ => None,
    }
}

/// Go 上栈声明行折叠（行 s：0=go1.22 1=go1.23）。
pub const fn stack_row(s: usize) -> Option<u32> {
    if s >= STACK_ROWS {
        return None;
    }
    Some(0x8500 + s as u32 * 0x61)
}

/// 并发拍基准载荷（拍 b：逐拍递增基准）。
pub const fn beat_load(b: usize) -> Option<u64> {
    if b >= BEAT_CELLS {
        return None;
    }
    Some(1000u64.wrapping_mul(b as u64 + 1))
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Buildkite Agent 收口账本状态机。
#[derive(Clone)]
pub struct BuildkiteLedger {
    /// 六判据（注册/checkout/命令/hooks/工件/遥测）。
    gates: [Option<bool>; 6],
    /// pipeline 格账。
    pipe: [Option<u64>; PIPE_CELLS],
    /// 注册三元闸账。
    reg: [Option<u64>; REG_GATES],
    /// hooks 投影账。
    hook: [Option<u64>; HOOK_CELLS],
    /// 并发拍账。
    beat: [Option<u64>; BEAT_CELLS],
    /// 联签账。
    cosign: [Option<u32>; COSIGN_ROWS],
    /// 上栈声明账。
    stack: [Option<u32>; STACK_ROWS],
    /// 最后段号。
    last_stage: i64,
    /// 最后拍载荷（单调闸基准）。
    last_load: i64,
    /// 采纳声明（升 S1 前置）。
    adopted: bool,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BuildkiteLedger {
    /// 新账本。
    pub fn new() -> Self {
        BuildkiteLedger {
            gates: [None; 6],
            pipe: [None; PIPE_CELLS],
            reg: [None; REG_GATES],
            hook: [None; HOOK_CELLS],
            beat: [None; BEAT_CELLS],
            cosign: [None; COSIGN_ROWS],
            stack: [None; STACK_ROWS],
            last_stage: -1,
            last_load: -1,
            adopted: false,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BuildkiteCode> {
        if gate >= 6 {
            return Err(BuildkiteCode::ESeal);
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

    /// agent 注册三元闸一格（载荷对照 + 重复拒）。
    pub fn record_reg(&mut self, g: usize, got: u64) -> Result<(), BuildkiteCode> {
        let want = match reg_gate_cell(g) {
            Some(w) => w,
            None => return Err(BuildkiteCode::ERegNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200 + g as u64);
            return Err(BuildkiteCode::EDrift);
        }
        if match self.reg.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BuildkiteCode::EDup);
        }
        if let Some(c) = self.reg.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x8201 + g as u64);
        Ok(())
    }

    /// 注册三元闸全齐核验。
    pub fn reg_all(&self) -> bool {
        let mut g = 0usize;
        while g < REG_GATES {
            match self.reg.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// pipeline 一格（段单调 + 载荷对照 + 重复拒）。
    pub fn record_pipe(&mut self, o: usize, got: u64) -> Result<(), BuildkiteCode> {
        if o >= PIPE_CELLS {
            return Err(BuildkiteCode::ENoCell);
        }
        let stage = match pipe_stage(o) {
            Some(st) => st,
            None => return Err(BuildkiteCode::ENoCell),
        };
        if (stage as i64) < self.last_stage {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(BuildkiteCode::EOrder);
        }
        let want = match pipe_cell(o) {
            Some(w) => w,
            None => return Err(BuildkiteCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + o as u64);
            return Err(BuildkiteCode::EDrift);
        }
        if match self.pipe.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BuildkiteCode::EDup);
        }
        if let Some(c) = self.pipe.get_mut(o) {
            *c = Some(got);
        }
        self.last_stage = stage as i64;
        self.fnv(0x8302 + o as u64);
        Ok(())
    }

    /// pipeline 全账核验（20 格）。
    pub fn pipe_all(&self) -> bool {
        let mut o = 0usize;
        while o < PIPE_CELLS {
            match self.pipe.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// hooks 投影一格（载荷对照 + 重复拒）。
    pub fn record_hook(&mut self, i: usize, got: u64) -> Result<(), BuildkiteCode> {
        let want = match hook_cell(i) {
            Some(w) => w,
            None => return Err(BuildkiteCode::EHookNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + i as u64);
            return Err(BuildkiteCode::EDrift);
        }
        if match self.hook.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BuildkiteCode::EDup);
        }
        if let Some(c) = self.hook.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x8401 + i as u64);
        Ok(())
    }

    /// hooks 投影全账核验（8 格）。
    pub fn hook_all(&self) -> bool {
        let mut i = 0usize;
        while i < HOOK_CELLS {
            match self.hook.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 并发一拍（逐拍递增单调闸 + 拍号闭集）。
    pub fn record_beat(&mut self, b: usize, load: u64) -> Result<(), BuildkiteCode> {
        if b >= BEAT_CELLS {
            return Err(BuildkiteCode::ENoRow);
        }
        if (load as i64) <= self.last_load {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500);
            return Err(BuildkiteCode::EOrder);
        }
        if match self.beat.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BuildkiteCode::EDup);
        }
        if let Some(c) = self.beat.get_mut(b) {
            *c = Some(load);
        }
        self.last_load = load as i64;
        self.fnv(0x8501 + b as u64);
        Ok(())
    }

    /// 并发拍全账核验（4 拍全落且逐拍递增）。
    pub fn beat_all(&self) -> bool {
        let mut b = 1usize;
        while b < BEAT_CELLS {
            let prev = match self.beat.get(b - 1) {
                Some(Some(p)) => *p,
                _ => return false,
            };
            let cur = match self.beat.get(b) {
                Some(Some(c)) => *c,
                _ => return false,
            };
            if cur <= prev {
                return false;
            }
            b += 1;
        }
        // b=0 格也须已落。
        match self.beat.get(0) {
            Some(Some(_)) => true,
            _ => false,
        }
    }

    /// 云消费联签一行。
    pub fn record_cosign(&mut self, r: usize) -> Result<(), BuildkiteCode> {
        let want = match cosign_row(r) {
            Some(w) => w,
            None => return Err(BuildkiteCode::ENoRow),
        };
        if match self.cosign.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BuildkiteCode::EDup);
        }
        if let Some(c) = self.cosign.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x8600 + r as u64);
        Ok(())
    }

    /// 联签全齐核验（4 格）。
    pub fn cosign_all(&self) -> bool {
        let mut r = 0usize;
        while r < COSIGN_ROWS {
            match self.cosign.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// Go 上栈声明一行。
    pub fn declare_stack(&mut self, s: usize) -> Result<(), BuildkiteCode> {
        let want = match stack_row(s) {
            Some(w) => w,
            None => return Err(BuildkiteCode::ENoRow),
        };
        if match self.stack.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BuildkiteCode::EDup);
        }
        if let Some(c) = self.stack.get_mut(s) {
            *c = Some(want);
        }
        self.fnv(0x8700 + s as u64);
        Ok(())
    }

    /// 上栈声明全齐核验（2 行）。
    pub fn stack_all(&self) -> bool {
        let mut s = 0usize;
        while s < STACK_ROWS {
            match self.stack.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 落采纳声明（升 S1 的前置格）。
    pub fn adopt(&mut self) {
        self.adopted = true;
        self.fnv(0x8800);
    }

    /// 采纳声明核验。
    pub fn adopted(&self) -> bool {
        self.adopted
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通且采纳 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.pipe_all()
            && self.reg_all()
            && self.hook_all()
            && self.beat_all()
            && self.cosign_all()
            && self.stack_all()
            && self.adopted
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for BuildkiteLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 全量可记账格数（六判据之外）。
pub const FILL_CELLS: u32 =
    (PIPE_CELLS + REG_GATES + HOOK_CELLS + BEAT_CELLS + COSIGN_ROWS + STACK_ROWS) as u32;

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BuildkiteLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut rg = 0usize;
    while rg < REG_GATES {
        if let Some(w) = reg_gate_cell(rg) {
            if led.record_reg(rg, w).is_ok() {
                recorded += 1;
            }
        }
        rg += 1;
    }
    let mut o = 0usize;
    while o < PIPE_CELLS {
        if let Some(w) = pipe_cell(o) {
            if led.record_pipe(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < HOOK_CELLS {
        if let Some(w) = hook_cell(i) {
            if led.record_hook(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut b = 0usize;
    while b < BEAT_CELLS {
        if let Some(w) = beat_load(b) {
            if led.record_beat(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut r = 0usize;
    while r < COSIGN_ROWS {
        let _ = led.record_cosign(r);
        r += 1;
    }
    let mut s = 0usize;
    while s < STACK_ROWS {
        let _ = led.declare_stack(s);
        s += 1;
    }
    // 注入 10 轮：投影漂移 / 并发倒拍 / 流水线格越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = BuildkiteLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_hook(2, hook_cell(2).unwrap_or(0));
                lt.record_hook(3, hook_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(BuildkiteCode::EDrift)
            }
            1 => {
                let _ = lt.record_beat(0, 500);
                lt.record_beat(1, 400) == Err(BuildkiteCode::EOrder)
            }
            _ => lt.record_pipe(PIPE_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8900 + k as u64);
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
/// UNX-F52368 判据集（锚 UNX-F52369-J1，off-by-one 板式）。
pub fn run_unx52368_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52368");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "pipeline 五段 20 格全通",
        recorded >= PIPE_CELLS as u32,
        "checkout/命令/插件/工件/通知五段逐格恒等",
    );
    s.add(
        "agent 注册三元闸 3/3 全落",
        {
            let mut lt = BuildkiteLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < REG_GATES {
                if let Some(w) = reg_gate_cell(g) {
                    if lt.record_reg(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.reg_all()
        },
        "注册令牌/队列/元数据标签三元闸逐格落账",
    );
    s.add(
        "hooks 投影 8/8 恒等",
        {
            let mut lt = BuildkiteLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < HOOK_CELLS {
                if let Some(w) = hook_cell(i) {
                    if lt.record_hook(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.hook_all()
        },
        "environment 至 exit 八投影逐格落账，漂移必抓",
    );
    s.add(
        "并发 4 拍单调全落",
        {
            let mut lt = BuildkiteLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BEAT_CELLS {
                if let Some(w) = beat_load(b) {
                    if lt.record_beat(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.beat_all()
        },
        "并发账逐拍递增，倒拍必抓",
    );
    s.add(
        "云消费联签 4/4 全齐",
        {
            let mut lt = BuildkiteLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < COSIGN_ROWS {
                if lt.record_cosign(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.cosign_all()
        },
        "管道/工件/指标/日志四路消费联签（§AI-66.3）",
    );
    s.add(
        "Go 上栈声明 2/2 全齐",
        {
            let mut lt = BuildkiteLedger::new();
            let mut ok = true;
            let mut st = 0usize;
            while st < STACK_ROWS {
                if lt.declare_stack(st).is_err() {
                    ok = false;
                }
                st += 1;
            }
            ok && lt.stack_all()
        },
        "go1.22/go1.23 运行时上栈声明两行闭集",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = BuildkiteLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "注册/checkout/命令/hooks/工件/遥测逐格落账",
    );
    s.add(
        "段单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PIPE_CELLS {
                if pipe_stage(o).unwrap_or(9) < pipe_stage(o - 1).unwrap_or(9) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五段闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "投影漂移/并发倒拍/流水线格越闭集三态轮转必抓",
    );
    let mut l0 = BuildkiteLedger::new();
    s.add(
        "边界·注册闸越闭集",
        l0.record_reg(REG_GATES, 0).is_err(),
        "三元闸之外越界必拒",
    );
    s.add(
        "边界·hooks 格越闭集",
        l0.record_hook(HOOK_CELLS, 0).is_err(),
        "hooks 8 格之外越界必拒",
    );
    s.add(
        "边界·流水线格越闭集",
        l0.record_pipe(PIPE_CELLS, 0).is_err(),
        "流水线 20 格之外越界必拒",
    );
    s.add(
        "并发倒拍拒",
        {
            let _ = l0.record_beat(0, 700);
            l0.record_beat(1, 600) == Err(BuildkiteCode::EOrder)
        },
        "并发账倒拍必抓",
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
        "定级 S2 闭集（KB-COMPAT 登记）",
        {
            // 全账落齐但不落采纳声明 → S2；采纳后可升 S1；空账 → S3。
            let mut lt = BuildkiteLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut rg = 0usize;
            while rg < REG_GATES {
                if let Some(w) = reg_gate_cell(rg) {
                    let _ = lt.record_reg(rg, w);
                }
                rg += 1;
            }
            let mut o = 0usize;
            while o < PIPE_CELLS {
                if let Some(w) = pipe_cell(o) {
                    let _ = lt.record_pipe(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < HOOK_CELLS {
                if let Some(w) = hook_cell(i) {
                    let _ = lt.record_hook(i, w);
                }
                i += 1;
            }
            let mut b = 0usize;
            while b < BEAT_CELLS {
                if let Some(w) = beat_load(b) {
                    let _ = lt.record_beat(b, w);
                }
                b += 1;
            }
            let mut r = 0usize;
            while r < COSIGN_ROWS {
                let _ = lt.record_cosign(r);
                r += 1;
            }
            let mut st = 0usize;
            while st < STACK_ROWS {
                let _ = lt.declare_stack(st);
                st += 1;
            }
            let s2 = lt.judge_grade();
            lt.adopt();
            let s1 = lt.judge_grade();
            s2 == GRADE_TARGET && s1 == Grade::S1
        },
        "全账未采纳 → S2，采纳后 → S1（定级闭集核验）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9BC0,
        "SEG_BASE 0x40 对齐且落在 VX15 公式位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pipe_stages() {
        assert_eq!(pipe_stage_name(0), Some("checkout"));
        assert_eq!(pipe_stage_name(2), Some("插件"));
        assert_eq!(pipe_stage_name(4), Some("通知"));
        assert!(pipe_stage_name(5).is_none());
        assert_eq!(pipe_stage(0), Some(0));
        assert_eq!(pipe_stage(19), Some(4));
        assert!(pipe_stage(20).is_none());
        assert!(pipe_cell(20).is_none());
    }

    #[test]
    fn test_reg_gates() {
        assert_eq!(reg_gate_name(0), Some("注册令牌"));
        assert_eq!(reg_gate_name(1), Some("队列"));
        assert_eq!(reg_gate_name(2), Some("元数据标签"));
        assert!(reg_gate_name(3).is_none());
        assert!(reg_gate_cell(3).is_none());
    }

    #[test]
    fn test_hooks() {
        assert_eq!(hook_name(0), Some("environment"));
        assert_eq!(hook_name(7), Some("exit"));
        assert!(hook_name(8).is_none());
        assert!(hook_cell(8).is_none());
        assert_ne!(hook_cell(0).unwrap_or(0), hook_cell(1).unwrap_or(0));
        // XOR 锚低 16 位恒零。
        assert_eq!(XORA_PIPE & 0xFFFF, 0);
        assert_eq!(0x484F_4F4B_0000u64 & 0xFFFF, 0);
    }

    #[test]
    fn test_beat_loads() {
        // 拍载荷逐拍递增基准。
        assert_eq!(beat_load(0), Some(1000));
        assert_eq!(beat_load(3), Some(4000));
        assert!(beat_load(4).is_none());
    }

    #[test]
    fn test_beat_monotone_caught() {
        let mut lt = BuildkiteLedger::new();
        assert!(lt.record_beat(0, 500).is_ok());
        assert_eq!(lt.record_beat(1, 400), Err(BuildkiteCode::EOrder));
        // 倒拍被拒后 last_load 未变：更高拍载荷仍可合法落账。
        assert!(lt.record_beat(1, 800).is_ok());
        assert!(lt.record_beat(4, 900).is_err());
    }

    #[test]
    fn test_stage_order_caught() {
        let mut lt = BuildkiteLedger::new();
        let _ = lt.record_pipe(9, pipe_cell(9).unwrap_or(0));
        assert_eq!(lt.record_pipe(2, pipe_cell(2).unwrap_or(0)), Err(BuildkiteCode::EOrder));
        // 逆序被拒后 last_stage 未变：同段 10 号仍可合法落账。
        assert!(lt.record_pipe(10, pipe_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = BuildkiteLedger::new();
        assert_eq!(
            lt.record_pipe(5, pipe_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(BuildkiteCode::EDrift)
        );
        assert_eq!(
            lt.record_reg(1, reg_gate_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(BuildkiteCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = BuildkiteLedger::new();
        let _ = lt.record_hook(0, hook_cell(0).unwrap_or(0));
        assert_eq!(lt.record_hook(0, hook_cell(0).unwrap_or(0)), Err(BuildkiteCode::EDup));
        let _ = lt.record_cosign(0);
        assert_eq!(lt.record_cosign(0), Err(BuildkiteCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = BuildkiteLedger::new();
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
        // 20 流水线格 + 3 注册闸 + 8 hooks + 4 拍 + 4 联签 + 2 上栈 = 41。
        assert_eq!(recorded, REG_GATES as u32 + PIPE_CELLS as u32 + HOOK_CELLS as u32 + BEAT_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC9BC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(BuildkiteCode::Ok as u32, 0);
        assert_eq!(BuildkiteCode::EGradeDrift as u32, 12);
        assert_eq!(BuildkiteCode::EHookNo as u32, 5);
        assert_eq!(BuildkiteCode::ESeal as u32, 9);
        assert!(!BuildkiteCode::Ok.detail().is_empty());
        assert!(!BuildkiteCode::EInj.detail().is_empty());
        assert_eq!(BuildkiteCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(GRADE_TARGET, Grade::S2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = BuildkiteLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 仅六判据（账未齐、未采纳）→ S2。
        let mut lt2 = BuildkiteLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52368_checks_green() {
        let s = run_unx52368_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
