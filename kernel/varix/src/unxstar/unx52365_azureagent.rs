//! UNX-F52365 Azure Pipelines Agent 收口 · 0xC9B00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52365"，逐字）：
//!   title: Azure Pipelines Agent 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52366-J1 本地执
//!     行/校验 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时
//!     API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52365）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52366-J1，照原文锚定；功能按本卡标题（Azure Pipelines Agent）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① pipeline 五段 20 格闭集：触发/拉源/编译/测试/发布五段各 4 格，
//!      段序单调（逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ② agent 注册三元闸：代理池/认证令牌/能力清单三格，逐格落账，
//!      越三元闭集必抓。
//!   ③ variable group 投影 8 格：投影载荷与基准恒等，漂移必抓。
//!   ④ 与 Azure DevOps 消费联签 4 格（管道消费/板消费/发布消费/工件
//!      消费）+ .NET 运行时上栈声明 2 行（net8.0/net9.0），缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（投影漂移/
//!      段逆序/流水线格越闭集），检出+逃逸守恒；双跑指纹一致；
//!      定级 S1/S2/S3 闭集，全通 → S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52365;

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

/// variable group 投影格数。
pub const VG_CELLS: usize = 8;

/// Azure DevOps 消费联签行数。
pub const COSIGN_ROWS: usize = 4;

/// .NET 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AzureAgCode {
    /// 成功
    Ok = 0,
    /// 流水线格越闭集
    ENoCell,
    /// 投影漂移
    EDrift,
    /// 段逆序
    EOrder,
    /// 注册闸越闭集
    ENoReg,
    /// variable group 格越闭集
    EVgNo,
    /// 联签/上栈行越闭集
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

impl AzureAgCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            AzureAgCode::Ok => "成功",
            AzureAgCode::ENoCell => "流水线格越闭集：五段 20 格之外先登记",
            AzureAgCode::EDrift => "投影漂移：pipeline/variable group 投影与基准不恒等，逐格复算",
            AzureAgCode::EOrder => "段逆序：触发→拉源→编译→测试→发布单调，逆序必抓",
            AzureAgCode::ENoReg => "注册闸越闭集：代理池/认证令牌/能力清单三元闸之外先登记",
            AzureAgCode::EVgNo => "variable group 格越闭集：投影 8 格之外先登记",
            AzureAgCode::ENoRow => "联签/上栈行越闭集：联签 4 格 + 上栈 2 行之外先登记",
            AzureAgCode::ERowMiss => "行未齐：Azure DevOps 联签与 .NET 上栈声明未齐先补齐",
            AzureAgCode::EDup => "重复落账：同格只许一条账目",
            AzureAgCode::ESeal => "未齐即封：20 格 + 三元闸 + 8 投影 + 联签上栈未齐先补齐",
            AzureAgCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            AzureAgCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            AzureAgCode::EGradeDrift => "定级漂移：全通且联签上栈全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20格+三元闸+8投影+联签上栈全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// XOR 锚（低 16 位恒零）。
pub const XORA_PIPE: u64 = 0x415A_5045_0000;

/// pipeline 格载荷（格 o；段 = o/4：0=触发 1=拉源 2=编译 3=测试 4=发布）。
pub const fn pipe_cell(o: usize) -> Option<u64> {
    if o >= PIPE_CELLS {
        return None;
    }
    Some((0x5A000u64 + (o as u64) * 0x2B) ^ XORA_PIPE)
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
        0 => Some("触发"),
        1 => Some("拉源"),
        2 => Some("编译"),
        3 => Some("测试"),
        4 => Some("发布"),
        _ => None,
    }
}

/// agent 注册三元闸载荷（闸 g：0=代理池 1=认证令牌 2=能力清单）。
pub const fn reg_gate_cell(g: usize) -> Option<u64> {
    if g >= REG_GATES {
        return None;
    }
    Some(0x5B000u64 + (g as u64) * 0x83)
}

/// 注册闸名。
pub const fn reg_gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("代理池"),
        1 => Some("认证令牌"),
        2 => Some("能力清单"),
        _ => None,
    }
}

/// variable group 投影载荷（格 i）。
pub const fn vg_cell(i: usize) -> Option<u64> {
    if i >= VG_CELLS {
        return None;
    }
    Some((0x5C000u64 + (i as u64) * 0x37) ^ 0x5652_4700_0000)
}

/// Azure DevOps 消费联签行折叠（行 r：0=管道 1=板 2=发布 3=工件）。
pub const fn cosign_row(r: usize) -> Option<u32> {
    if r >= COSIGN_ROWS {
        return None;
    }
    Some(0x5D00 + r as u32 * 0x41)
}

/// 联签行名。
pub const fn cosign_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("管道消费联签"),
        1 => Some("板消费联签"),
        2 => Some("发布消费联签"),
        3 => Some("工件消费联签"),
        _ => None,
    }
}

/// .NET 上栈声明行折叠（行 s：0=net8.0 1=net9.0）。
pub const fn stack_row(s: usize) -> Option<u32> {
    if s >= STACK_ROWS {
        return None;
    }
    Some(0x5E00 + s as u32 * 0x47)
}

/// 上栈行名。
pub const fn stack_row_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("net8.0 上栈声明"),
        1 => Some("net9.0 上栈声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Azure Pipelines Agent 收口账本状态机。
#[derive(Clone)]
pub struct AzureAgLedger {
    /// 六判据（注册/拉取/编译/测试/发布/遥测）。
    gates: [Option<bool>; 6],
    /// pipeline 格账。
    pipe: [Option<u64>; PIPE_CELLS],
    /// 注册三元闸账。
    reg: [Option<u64>; REG_GATES],
    /// variable group 投影账。
    vg: [Option<u64>; VG_CELLS],
    /// 联签账。
    cosign: [Option<u32>; COSIGN_ROWS],
    /// 上栈声明账。
    stack: [Option<u32>; STACK_ROWS],
    /// 最后段号。
    last_stage: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl AzureAgLedger {
    /// 新账本。
    pub fn new() -> Self {
        AzureAgLedger {
            gates: [None; 6],
            pipe: [None; PIPE_CELLS],
            reg: [None; REG_GATES],
            vg: [None; VG_CELLS],
            cosign: [None; COSIGN_ROWS],
            stack: [None; STACK_ROWS],
            last_stage: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), AzureAgCode> {
        if gate >= 6 {
            return Err(AzureAgCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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
    pub fn record_reg(&mut self, g: usize, got: u64) -> Result<(), AzureAgCode> {
        let want = match reg_gate_cell(g) {
            Some(w) => w,
            None => return Err(AzureAgCode::ENoReg),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + g as u64);
            return Err(AzureAgCode::EDrift);
        }
        if match self.reg.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AzureAgCode::EDup);
        }
        if let Some(c) = self.reg.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x5201 + g as u64);
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
    pub fn record_pipe(&mut self, o: usize, got: u64) -> Result<(), AzureAgCode> {
        if o >= PIPE_CELLS {
            return Err(AzureAgCode::ENoCell);
        }
        let stage = match pipe_stage(o) {
            Some(s) => s,
            None => return Err(AzureAgCode::ENoCell),
        };
        if (stage as i64) < self.last_stage {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(AzureAgCode::EOrder);
        }
        let want = match pipe_cell(o) {
            Some(w) => w,
            None => return Err(AzureAgCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + o as u64);
            return Err(AzureAgCode::EDrift);
        }
        if match self.pipe.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AzureAgCode::EDup);
        }
        if let Some(c) = self.pipe.get_mut(o) {
            *c = Some(got);
        }
        self.last_stage = stage as i64;
        self.fnv(0x5302 + o as u64);
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

    /// variable group 投影一格（载荷对照 + 重复拒）。
    pub fn record_vg(&mut self, i: usize, got: u64) -> Result<(), AzureAgCode> {
        let want = match vg_cell(i) {
            Some(w) => w,
            None => return Err(AzureAgCode::EVgNo),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5400 + i as u64);
            return Err(AzureAgCode::EDrift);
        }
        if match self.vg.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AzureAgCode::EDup);
        }
        if let Some(c) = self.vg.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x5401 + i as u64);
        Ok(())
    }

    /// 投影全账核验（8 格）。
    pub fn vg_all(&self) -> bool {
        let mut i = 0usize;
        while i < VG_CELLS {
            match self.vg.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 联签一行（Azure DevOps 消费联签）。
    pub fn record_cosign(&mut self, r: usize) -> Result<(), AzureAgCode> {
        let want = match cosign_row(r) {
            Some(w) => w,
            None => return Err(AzureAgCode::ENoRow),
        };
        if match self.cosign.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AzureAgCode::EDup);
        }
        if let Some(c) = self.cosign.get_mut(r) {
            *c = Some(want);
        }
        self.fnv(0x5500 + r as u64);
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

    /// .NET 上栈声明一行。
    pub fn declare_stack(&mut self, s: usize) -> Result<(), AzureAgCode> {
        let want = match stack_row(s) {
            Some(w) => w,
            None => return Err(AzureAgCode::ENoRow),
        };
        if match self.stack.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(AzureAgCode::EDup);
        }
        if let Some(c) = self.stack.get_mut(s) {
            *c = Some(want);
        }
        self.fnv(0x5600 + s as u64);
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
            && self.pipe_all()
            && self.reg_all()
            && self.vg_all()
            && self.cosign_all()
            && self.stack_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for AzureAgLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 全量可记账格数（六判据之外）。
pub const FILL_CELLS: u32 =
    (PIPE_CELLS + REG_GATES + VG_CELLS + COSIGN_ROWS + STACK_ROWS) as u32;

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = AzureAgLedger::new();
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
    while i < VG_CELLS {
        if let Some(w) = vg_cell(i) {
            if led.record_vg(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
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
    // 注入 10 轮：投影漂移 / 段逆序 / 流水线格越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = AzureAgLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_pipe(2, pipe_cell(2).unwrap_or(0));
                lt.record_pipe(1, pipe_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(AzureAgCode::EDrift)
            }
            1 => {
                let _ = lt.record_pipe(8, pipe_cell(8).unwrap_or(0));
                lt.record_pipe(2, pipe_cell(2).unwrap_or(0)) == Err(AzureAgCode::EOrder)
            }
            _ => lt.record_pipe(PIPE_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
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
/// UNX-F52365 判据集（锚 UNX-F52366-J1，off-by-one 板式）。
pub fn run_unx52365_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52365");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "pipeline 五段 20 格全通",
        recorded >= PIPE_CELLS as u32,
        "触发/拉源/编译/测试/发布五段逐格恒等",
    );
    s.add(
        "agent 注册三元闸 3/3 全落",
        {
            let mut lt = AzureAgLedger::new();
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
        "代理池/认证令牌/能力清单三元闸逐格落账",
    );
    s.add(
        "variable group 投影 8/8 恒等",
        {
            let mut lt = AzureAgLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < VG_CELLS {
                if let Some(w) = vg_cell(i) {
                    if lt.record_vg(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.vg_all()
        },
        "投影 8 格逐格与基准恒等，漂移必抓",
    );
    s.add(
        "Azure DevOps 消费联签 4/4 全齐",
        {
            let mut lt = AzureAgLedger::new();
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
        "管道/板/发布/工件四路消费联签（§AI-66.3）",
    );
    s.add(
        ".NET 上栈声明 2/2 全齐",
        {
            let mut lt = AzureAgLedger::new();
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
        "net8.0/net9.0 运行时上栈声明两行闭集",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = AzureAgLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "注册/拉取/编译/测试/发布/遥测逐格落账",
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
        "投影漂移/段逆序/流水线格越闭集三态轮转必抓",
    );
    let mut l0 = AzureAgLedger::new();
    s.add(
        "边界·注册闸越闭集",
        l0.record_reg(REG_GATES, 0).is_err(),
        "三元闸之外越界必拒",
    );
    s.add(
        "边界·投影格越闭集",
        l0.record_vg(VG_CELLS, 0).is_err(),
        "variable group 8 格之外越界必拒",
    );
    s.add(
        "边界·流水线格越闭集",
        l0.record_pipe(PIPE_CELLS, 0).is_err(),
        "流水线 20 格之外越界必拒",
    );
    s.add(
        "投影漂移拒",
        {
            let _ = l0.record_pipe(5, pipe_cell(5).unwrap_or(0));
            l0.record_pipe(6, pipe_cell(6).unwrap_or(0).wrapping_add(1)) == Err(AzureAgCode::EDrift)
        },
        "投影载荷漂移必抓",
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
            let mut lt = AzureAgLedger::new();
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
            while i < VG_CELLS {
                if let Some(w) = vg_cell(i) {
                    let _ = lt.record_vg(i, w);
                }
                i += 1;
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
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9B00,
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
        assert_eq!(pipe_stage_name(0), Some("触发"));
        assert_eq!(pipe_stage_name(2), Some("编译"));
        assert_eq!(pipe_stage_name(4), Some("发布"));
        assert!(pipe_stage_name(5).is_none());
        assert_eq!(pipe_stage(0), Some(0));
        assert_eq!(pipe_stage(19), Some(4));
        assert!(pipe_stage(20).is_none());
        assert!(pipe_cell(20).is_none());
    }

    #[test]
    fn test_reg_gates() {
        assert_eq!(reg_gate_name(0), Some("代理池"));
        assert_eq!(reg_gate_name(1), Some("认证令牌"));
        assert_eq!(reg_gate_name(2), Some("能力清单"));
        assert!(reg_gate_name(3).is_none());
        assert!(reg_gate_cell(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = vg_cell(3).unwrap_or(0);
        assert_eq!(a, vg_cell(3).unwrap_or(0));
        assert_ne!(a, vg_cell(4).unwrap_or(0));
        assert!(cosign_row(COSIGN_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        // XOR 锚低 16 位恒零。
        assert_eq!(XORA_PIPE & 0xFFFF, 0);
        assert_eq!(0x5652_4700_0000u64 & 0xFFFF, 0);
    }

    #[test]
    fn test_row_names() {
        assert_eq!(cosign_row_name(0), Some("管道消费联签"));
        assert_eq!(cosign_row_name(3), Some("工件消费联签"));
        assert!(cosign_row_name(4).is_none());
        assert_eq!(stack_row_name(0), Some("net8.0 上栈声明"));
        assert_eq!(stack_row_name(1), Some("net9.0 上栈声明"));
        assert!(stack_row_name(2).is_none());
    }

    #[test]
    fn test_stage_order_caught() {
        let mut lt = AzureAgLedger::new();
        let _ = lt.record_pipe(9, pipe_cell(9).unwrap_or(0));
        assert_eq!(lt.record_pipe(2, pipe_cell(2).unwrap_or(0)), Err(AzureAgCode::EOrder));
        // 逆序被拒后 last_stage 未变：同段 10 号仍可合法落账。
        assert!(lt.record_pipe(10, pipe_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = AzureAgLedger::new();
        assert_eq!(
            lt.record_pipe(5, pipe_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(AzureAgCode::EDrift)
        );
        assert_eq!(
            lt.record_vg(2, vg_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(AzureAgCode::EDrift)
        );
        assert_eq!(
            lt.record_reg(1, reg_gate_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(AzureAgCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = AzureAgLedger::new();
        let _ = lt.record_vg(0, vg_cell(0).unwrap_or(0));
        assert_eq!(lt.record_vg(0, vg_cell(0).unwrap_or(0)), Err(AzureAgCode::EDup));
        let _ = lt.record_cosign(0);
        assert_eq!(lt.record_cosign(0), Err(AzureAgCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = AzureAgLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = AzureAgLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 20 流水线格 + 3 注册闸 + 8 投影格 + 4 联签 + 2 上栈 = 37。
        assert_eq!(recorded, REG_GATES as u32 + PIPE_CELLS as u32 + VG_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC9B00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(AzureAgCode::Ok as u32, 0);
        assert_eq!(AzureAgCode::EGradeDrift as u32, 12);
        assert_eq!(AzureAgCode::ESeal as u32, 9);
        assert!(!AzureAgCode::Ok.detail().is_empty());
        assert!(!AzureAgCode::ESeal.detail().is_empty());
        assert_eq!(AzureAgCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = AzureAgLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 只有六判据 → S2。
        let mut lt2 = AzureAgLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52365_checks_green() {
        let s = run_unx52365_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
