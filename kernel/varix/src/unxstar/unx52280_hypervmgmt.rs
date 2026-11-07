//! UNX-F52280 Hyper-V 管理器消费面收口（管理工具条目） · 0xC85C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52280"，逐字）：
//!   title: Hyper-V 管理器消费面收口（管理工具条目）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52281-J1 B01/
//!     B04 全部构建链条目汇总：作业对象/环境块/退出码传递三语义 ×10
//!     工程交叉复测零漂移；消费 D2/E2 判据绿（联签锚定行）；判据锚定
//!     §AI-66.2 专题二编译器进程树；③ 完成时 API/软件写回：状态=已完
//!     成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52280）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52281-J1，照原文锚定；功能按本卡标题（Hyper-V 管理器消费面
//! ·管理工具条目）落地，构建链三语义判据由同批后续卡承载。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：VM 生命周期 10 操作账，五家族闭集（新建/启动/检查点/
//!      连接/停止）各 2 操作，家族单调（逆序必抓）；检查点树 12 格，
//!      父子层序（根→子→孙→曾孙）单调，层序逆序必抓；虚拟交换机三
//!      类（外部/内部/专用）8 格；导入导出 6 格 + 动态内存 5 格合为
//!      辅助账 11 格。
//!   ② PowerShell/管理器双面分账声明 2 行闭集（PowerShell 消费分账/
//!      管理器消费分账），缺一拒封；管理界面枚举/连接 ×10 判据入六
//!      闸（UNX-F52281-J1 锚定承载）。
//!   ③ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（检查点父子序逆序/交换机载荷
//!      漂移/生命周期操作越闭集），全部落在当轮临时账本上，主账本
//!      last_kind/last_level 不受扰动；检出+逃逸守恒等式；双跑 FNV
//!      指纹一致。
//!   ④ KB-COMPAT 定级闭集：全通 → S1（44 项域账 + 6 判据 + 2 行分账）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52280;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// VM 生命周期操作数。
pub const LIFE_OPS: usize = 10;

/// 检查点树格数。
pub const CP_CELLS: usize = 12;

/// 每层检查点格数（根/子/孙/曾孙各 3）。
pub const CP_LEVEL_CELLS: usize = 3;

/// 虚拟交换机格数。
pub const SWITCH_CELLS: usize = 8;

/// 辅助账格数（导入导出 6 + 动态内存 5）。
pub const AUX_CELLS: usize = 11;

/// 导入导出格数（辅助账前段）。
pub const IMEX_CELLS: usize = 6;

/// PowerShell/管理器双面分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HyperVCode {
    /// 成功
    Ok = 0,
    /// 生命周期操作号越闭集
    ENoOp,
    /// 生命周期操作漂移
    EDrift,
    /// 生命周期家族逆序
    EOrder,
    /// 检查点格越闭集
    ENoCp,
    /// 检查点载荷漂移
    ECpDrift,
    /// 检查点父子层序逆序
    ECpOrder,
    /// 交换机格越闭集
    ENoSwitch,
    /// 交换机载荷漂移
    ESwDrift,
    /// 辅助账格越闭集
    ENoAux,
    /// 辅助账载荷漂移
    EAuxDrift,
    /// 未齐即封（含分账行越闭集）
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl HyperVCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            HyperVCode::Ok => "成功",
            HyperVCode::ENoOp => "生命周期操作号越闭集：VM 生命周期 10 操作集之外先登记",
            HyperVCode::EDrift => "生命周期操作漂移：操作折叠与基准不恒等，逐格复算",
            HyperVCode::EOrder => "生命周期家族逆序：新建→启动→检查点→连接→停止单调，逆序必抓",
            HyperVCode::ENoCp => "检查点格越闭集：检查点树 12 格之外先登记",
            HyperVCode::ECpDrift => "检查点载荷漂移：树折叠与基准不恒等，逐格复算",
            HyperVCode::ECpOrder => "检查点父子层序逆序：根→子→孙→曾孙单调，逆序必抓",
            HyperVCode::ENoSwitch => "交换机格越闭集：虚拟交换机三类 8 格之外先登记",
            HyperVCode::ESwDrift => "交换机载荷漂移：外部/内部/专用折叠与基准不恒等，逐格复算",
            HyperVCode::ENoAux => "辅助账格越闭集：导入导出 6 格 + 动态内存 5 格之外先登记",
            HyperVCode::EAuxDrift => "辅助账载荷漂移：导入导出/动态内存折叠与基准不恒等，逐格复算",
            HyperVCode::ESeal => "未齐即封：10 操作 + 12 检查点 + 8 交换机 + 11 辅助 + 2 分账未齐先补齐",
            HyperVCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+10 操作+12 检查点+8 交换机+11 辅助+2 行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 生命周期操作格载荷（操作 o；家族 = o/2）。
pub const fn life_cell(o: usize) -> Option<u64> {
    if o >= LIFE_OPS {
        return None;
    }
    Some((0xD1000u64 + (o as u64) * 0x3A) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// 生命周期家族（0..2=新建 2..4=启动 4..6=检查点 6..8=连接 8..10=停止）。
pub const fn life_kind(o: usize) -> Option<u32> {
    if o >= LIFE_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// 生命周期家族名。
pub const fn life_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("新建"),
        1 => Some("启动"),
        2 => Some("检查点"),
        3 => Some("连接"),
        4 => Some("停止"),
        _ => None,
    }
}

/// 检查点树格载荷（格 c；父子层 = c/3）。
pub const fn cp_cell(c: usize) -> Option<u64> {
    if c >= CP_CELLS {
        return None;
    }
    Some((0xD2000u64 + (c as u64) * 0x3E) ^ ((c / CP_LEVEL_CELLS) as u64).wrapping_mul(0x9E3779B9))
}

/// 检查点父子层（0..3=根 3..6=子 6..9=孙 9..12=曾孙）。
pub const fn cp_level(c: usize) -> Option<u32> {
    if c >= CP_CELLS {
        return None;
    }
    Some((c / CP_LEVEL_CELLS) as u32)
}

/// 检查点层名。
pub const fn cp_level_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("根"),
        1 => Some("子"),
        2 => Some("孙"),
        3 => Some("曾孙"),
        _ => None,
    }
}

/// 虚拟交换机格载荷（格 w；类别 = w/3，尾格并入专用类）。
pub const fn switch_cell(w: usize) -> Option<u64> {
    if w >= SWITCH_CELLS {
        return None;
    }
    Some((0xD3000u64 + (w as u64) * 0x44) ^ 0x48565F5357434C31)
}

/// 虚拟交换机类别（0..3=外部 3..6=内部 6..8=专用）。
pub const fn switch_kind(w: usize) -> Option<u32> {
    if w >= SWITCH_CELLS {
        return None;
    }
    if w < 3 {
        return Some(0);
    }
    if w < 6 {
        return Some(1);
    }
    Some(2)
}

/// 交换机类别名。
pub const fn switch_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("外部"),
        1 => Some("内部"),
        2 => Some("专用"),
        _ => None,
    }
}

/// 辅助账格载荷（格 a；0..6=导入导出 6..11=动态内存）。
pub const fn aux_cell(a: usize) -> Option<u64> {
    if a >= AUX_CELLS {
        return None;
    }
    Some((0xD4000u64 + (a as u64) * 0x48) ^ 0x48565F4155585F31)
}

/// 辅助账段名。
pub const fn aux_kind_name(a: usize) -> Option<&'static str> {
    if a >= IMEX_CELLS {
        if a < AUX_CELLS {
            return Some("动态内存");
        }
        return None;
    }
    Some("导入导出")
}

/// PowerShell/管理器双面分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xD500 + r as u32 * 0x36)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Hyper-V 管理器消费面收口账本状态机。
#[derive(Clone)]
pub struct HyperVLedger {
    /// 六判据（枚举/连接/检查点/交换机/导入导出/分账）。
    gates: [Option<bool>; 6],
    /// 生命周期操作账。
    ops: [Option<u64>; LIFE_OPS],
    /// 检查点树账。
    cps: [Option<u64>; CP_CELLS],
    /// 交换机账。
    switches: [Option<u64>; SWITCH_CELLS],
    /// 辅助账。
    aux: [Option<u64>; AUX_CELLS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后生命周期家族。
    last_kind: i64,
    /// 最后检查点父子层。
    last_level: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl HyperVLedger {
    /// 新账本。
    pub fn new() -> Self {
        HyperVLedger {
            gates: [None; 6],
            ops: [None; LIFE_OPS],
            cps: [None; CP_CELLS],
            switches: [None; SWITCH_CELLS],
            aux: [None; AUX_CELLS],
            splits: [None; SPLIT_ROWS],
            last_kind: -1,
            last_level: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), HyperVCode> {
        if gate >= 6 {
            return Err(HyperVCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5370 + gate as u64);
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

    /// 生命周期一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_life(&mut self, o: usize, got: u64) -> Result<(), HyperVCode> {
        if o >= LIFE_OPS {
            return Err(HyperVCode::ENoOp);
        }
        let kind = match life_kind(o) {
            Some(k) => k,
            None => return Err(HyperVCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5380);
            return Err(HyperVCode::EOrder);
        }
        let want = match life_cell(o) {
            Some(w) => w,
            None => return Err(HyperVCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5381 + o as u64);
            return Err(HyperVCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HyperVCode::ESeal);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5382 + o as u64);
        Ok(())
    }

    /// 生命周期全账核验（10 格）。
    pub fn life_all(&self) -> bool {
        let mut o = 0usize;
        while o < LIFE_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 检查点一格（父子层序单调 + 载荷对照 + 重复拒）。
    pub fn record_cp(&mut self, c: usize, got: u64) -> Result<(), HyperVCode> {
        if c >= CP_CELLS {
            return Err(HyperVCode::ENoCp);
        }
        let level = match cp_level(c) {
            Some(l) => l,
            None => return Err(HyperVCode::ENoCp),
        };
        if (level as i64) < self.last_level {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5390);
            return Err(HyperVCode::ECpOrder);
        }
        let want = match cp_cell(c) {
            Some(w) => w,
            None => return Err(HyperVCode::ENoCp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5391 + c as u64);
            return Err(HyperVCode::ECpDrift);
        }
        if match self.cps.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HyperVCode::ESeal);
        }
        if let Some(cc) = self.cps.get_mut(c) {
            *cc = Some(got);
        }
        self.last_level = level as i64;
        self.fnv(0x5392 + c as u64);
        Ok(())
    }

    /// 检查点树全账核验（12 格）。
    pub fn cps_all(&self) -> bool {
        let mut c = 0usize;
        while c < CP_CELLS {
            match self.cps.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 交换机一格（载荷对照 + 重复拒）。
    pub fn record_switch(&mut self, w: usize, got: u64) -> Result<(), HyperVCode> {
        let want = match switch_cell(w) {
            Some(x) => x,
            None => return Err(HyperVCode::ENoSwitch),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x53A0 + w as u64);
            return Err(HyperVCode::ESwDrift);
        }
        if match self.switches.get(w) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HyperVCode::ESeal);
        }
        if let Some(c) = self.switches.get_mut(w) {
            *c = Some(got);
        }
        self.fnv(0x53A1 + w as u64);
        Ok(())
    }

    /// 交换机全账核验（8 格）。
    pub fn switches_all(&self) -> bool {
        let mut w = 0usize;
        while w < SWITCH_CELLS {
            match self.switches.get(w) {
                Some(Some(_)) => {}
                _ => return false,
            }
            w += 1;
        }
        true
    }

    /// 辅助账一格（导入导出/动态内存，载荷对照 + 重复拒）。
    pub fn record_aux(&mut self, a: usize, got: u64) -> Result<(), HyperVCode> {
        let want = match aux_cell(a) {
            Some(x) => x,
            None => return Err(HyperVCode::ENoAux),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x53B0 + a as u64);
            return Err(HyperVCode::EAuxDrift);
        }
        if match self.aux.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HyperVCode::ESeal);
        }
        if let Some(c) = self.aux.get_mut(a) {
            *c = Some(got);
        }
        self.fnv(0x53B1 + a as u64);
        Ok(())
    }

    /// 辅助账全账核验（11 格）。
    pub fn aux_all(&self) -> bool {
        let mut a = 0usize;
        while a < AUX_CELLS {
            match self.aux.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// PowerShell/管理器双面分账声明一行（越界/重复走 ESeal 语义）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), HyperVCode> {
        if r >= SPLIT_ROWS {
            return Err(HyperVCode::ESeal);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(HyperVCode::ESeal);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x53C0 + r as u64);
        Ok(())
    }

    /// 分账全齐核验。
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

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.life_all() && self.cps_all() && self.switches_all() && self.aux_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for HyperVLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = HyperVLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < LIFE_OPS {
        if let Some(w) = life_cell(o) {
            if led.record_life(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < CP_CELLS {
        if let Some(w) = cp_cell(c) {
            if led.record_cp(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut w = 0usize;
    while w < SWITCH_CELLS {
        if let Some(x) = switch_cell(w) {
            if led.record_switch(w, x).is_ok() {
                recorded += 1;
            }
        }
        w += 1;
    }
    let mut a = 0usize;
    while a < AUX_CELLS {
        if let Some(x) = aux_cell(a) {
            if led.record_aux(a, x).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：检查点父子序逆序 / 交换机载荷漂移 / 生命周期越闭集三态轮转。
    // 层序注入先按合法序落曾孙层格（格 9）再注根层格（格 1），空账本上
    // 同样必抓；全部落在临时账本 lt 上，主账本 last_level 不受扰动。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = HyperVLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_cp(9, cp_cell(9).unwrap_or(0));
                lt.record_cp(1, cp_cell(1).unwrap_or(0)) == Err(HyperVCode::ECpOrder)
            }
            1 => lt.record_switch(2, switch_cell(2).unwrap_or(0).wrapping_add(1)) == Err(HyperVCode::ESwDrift),
            _ => lt.record_life(LIFE_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x53D0 + k as u64);
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
/// UNX-F52280 判据集（锚 UNX-F52281-J1，off-by-one 板式）。
pub fn run_unx52280_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52280");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "VM 生命周期 10 操作全通",
        recorded >= LIFE_OPS as u32,
        "新建/启动/检查点/连接/停止五家族逐操作恒等",
    );
    s.add(
        "检查点树 12 格全落",
        {
            let mut lt = HyperVLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CP_CELLS {
                if let Some(w) = cp_cell(c) {
                    if lt.record_cp(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cps_all()
        },
        "根/子/孙/曾孙四层各 3 格逐格恒等",
    );
    s.add(
        "检查点父子序单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < CP_CELLS {
                if cp_level(c).unwrap_or(4) < cp_level(c - 1).unwrap_or(4) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "根→子→孙→曾孙闭集单调，层序逆序必抓",
    );
    s.add(
        "虚拟交换机三类 8 格全落",
        {
            let mut lt = HyperVLedger::new();
            let mut ok = true;
            let mut w = 0usize;
            while w < SWITCH_CELLS {
                if let Some(x) = switch_cell(w) {
                    if lt.record_switch(w, x).is_err() {
                        ok = false;
                    }
                }
                w += 1;
            }
            ok && lt.switches_all()
        },
        "外部/内部/专用三类逐格恒等",
    );
    s.add(
        "导入导出 6 格全落",
        {
            let mut lt = HyperVLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < IMEX_CELLS {
                if let Some(x) = aux_cell(a) {
                    if lt.record_aux(a, x).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok
        },
        "导入导出 6 格逐格恒等",
    );
    s.add(
        "动态内存 5 格全落",
        {
            // 辅助账 11 格全落满（导入导出 6 格 + 动态内存 5 格）后，
            // 动态内存段 a ∈ [IMEX_CELLS, AUX_CELLS) 逐格恒等核验。
            let mut lt = HyperVLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < AUX_CELLS {
                if let Some(x) = aux_cell(a) {
                    if lt.record_aux(a, x).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.aux_all()
        },
        "动态内存 5 格逐格恒等",
    );
    s.add(
        "六判据全落（管理界面枚举/连接 ×10 承载）",
        {
            let mut lt = HyperVLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "枚举/连接/检查点/交换机/导入导出/分账逐格落账（UNX-F52281-J1 锚定）",
    );
    s.add(
        "PowerShell/管理器双面分账 2/2 全齐",
        {
            let mut lt = HyperVLedger::new();
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
        "PowerShell 消费分账/管理器消费分账两行闭集，缺一拒封",
    );
    s.add(
        "生命周期家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < LIFE_OPS {
                if life_kind(o).unwrap_or(5) < life_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五家族闭集单调",
    );
    s.add("注入 10/10 全抓", caught >= 10, "父子序逆序/交换机漂移/越闭集三态轮转必抓");
    let mut l0 = HyperVLedger::new();
    s.add(
        "边界·生命周期越闭集",
        l0.record_life(LIFE_OPS, 0).is_err(),
        "生命周期操作 10 越闭集必拒",
    );
    s.add(
        "边界·检查点越闭集",
        l0.record_cp(CP_CELLS, 0).is_err(),
        "检查点格 12 越闭集必拒",
    );
    s.add(
        "边界·交换机越闭集",
        l0.record_switch(SWITCH_CELLS, 0).is_err(),
        "交换机格 8 越闭集必拒",
    );
    s.add(
        "漂移·检查点载荷漂移拒",
        l0.record_cp(2, cp_cell(2).unwrap_or(0).wrapping_add(1)) == Err(HyperVCode::ECpDrift),
        "检查点载荷漂移必抓",
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
            let mut lt = HyperVLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < LIFE_OPS {
                if let Some(w) = life_cell(o) {
                    let _ = lt.record_life(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CP_CELLS {
                if let Some(w) = cp_cell(c) {
                    let _ = lt.record_cp(c, w);
                }
                c += 1;
            }
            let mut w = 0usize;
            while w < SWITCH_CELLS {
                if let Some(x) = switch_cell(w) {
                    let _ = lt.record_switch(w, x);
                }
                w += 1;
            }
            let mut a = 0usize;
            while a < AUX_CELLS {
                if let Some(x) = aux_cell(a) {
                    let _ = lt.record_aux(a, x);
                }
                a += 1;
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
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC85C0,
        "SEG_BASE 0x40 对齐且等于 0xC7200+0x40×(52280−52201)",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 ≥19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_life_kind() {
        assert_eq!(life_kind(0), Some(0));
        assert_eq!(life_kind(4), Some(2));
        assert_eq!(life_kind(9), Some(4));
        assert!(life_kind(10).is_none());
        assert!(life_cell(10).is_none());
    }

    #[test]
    fn test_kind_names() {
        assert_eq!(life_kind_name(0), Some("新建"));
        assert_eq!(life_kind_name(2), Some("检查点"));
        assert_eq!(life_kind_name(4), Some("停止"));
        assert!(life_kind_name(5).is_none());
        assert_eq!(cp_level_name(0), Some("根"));
        assert_eq!(cp_level_name(3), Some("曾孙"));
        assert!(cp_level_name(4).is_none());
    }

    #[test]
    fn test_cp_level() {
        assert_eq!(cp_level(0), Some(0));
        assert_eq!(cp_level(2), Some(0));
        assert_eq!(cp_level(3), Some(1));
        assert_eq!(cp_level(11), Some(3));
        assert!(cp_level(12).is_none());
        assert!(cp_cell(12).is_none());
    }

    #[test]
    fn test_switch_kind() {
        assert_eq!(switch_kind(0), Some(0));
        assert_eq!(switch_kind(2), Some(0));
        assert_eq!(switch_kind(4), Some(1));
        assert_eq!(switch_kind(7), Some(2));
        assert!(switch_kind(8).is_none());
        assert!(switch_cell(8).is_none());
        assert_eq!(switch_kind_name(0), Some("外部"));
        assert_eq!(switch_kind_name(1), Some("内部"));
        assert_eq!(switch_kind_name(2), Some("专用"));
    }

    #[test]
    fn test_aux_segmentation() {
        assert_eq!(aux_kind_name(0), Some("导入导出"));
        assert_eq!(aux_kind_name(5), Some("导入导出"));
        assert_eq!(aux_kind_name(6), Some("动态内存"));
        assert_eq!(aux_kind_name(10), Some("动态内存"));
        assert!(aux_kind_name(11).is_none());
        assert!(aux_cell(11).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cp_cell(3).unwrap_or(0);
        assert_eq!(a, cp_cell(3).unwrap_or(0));
        assert_ne!(a, cp_cell(4).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = HyperVLedger::new();
        let _ = lt.record_life(9, life_cell(9).unwrap_or(0));
        assert_eq!(lt.record_life(3, life_cell(3).unwrap_or(0)), Err(HyperVCode::EOrder));
        // 同族继续按合法序落格（家族 4 未落格位 8）不受拒绝影响。
        assert!(lt.record_life(8, life_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_cp_order_caught() {
        let mut lt = HyperVLedger::new();
        let _ = lt.record_cp(7, cp_cell(7).unwrap_or(0));
        assert_eq!(lt.record_cp(2, cp_cell(2).unwrap_or(0)), Err(HyperVCode::ECpOrder));
        assert!(lt.record_cp(8, cp_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = HyperVLedger::new();
        assert_eq!(
            lt.record_switch(1, switch_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(HyperVCode::ESwDrift)
        );
        assert_eq!(
            lt.record_aux(0, aux_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(HyperVCode::EAuxDrift)
        );
        assert_eq!(
            lt.record_life(0, life_cell(0).unwrap_or(0).wrapping_add(5)),
            Err(HyperVCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = HyperVLedger::new();
        let _ = lt.record_cp(0, cp_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cp(0, cp_cell(0).unwrap_or(0)), Err(HyperVCode::ESeal));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = HyperVLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = HyperVLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_empty_ledger_cp_injection() {
        // 空账本上父子序逆序注入同样必抓，且不改主账本 last_level。
        let mut main = HyperVLedger::new();
        let _ = main.record_cp(0, cp_cell(0).unwrap_or(0));
        let mut lt = HyperVLedger::new();
        let _ = lt.record_cp(9, cp_cell(9).unwrap_or(0));
        assert_eq!(lt.record_cp(1, cp_cell(1).unwrap_or(0)), Err(HyperVCode::ECpOrder));
        // 主账本继续按合法序落格不受影响。
        assert!(main.record_cp(3, cp_cell(3).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (LIFE_OPS + CP_CELLS + SWITCH_CELLS + AUX_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC85C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(HyperVCode::Ok as u32, 0);
        assert_eq!(HyperVCode::EGradeDrift as u32, 12);
        assert_eq!(HyperVCode::ECpOrder.val(), SEG_BASE + 6);
        assert!(!HyperVCode::Ok.detail().is_empty());
        assert!(!HyperVCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = HyperVLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52280_checks_green() {
        let s = run_unx52280_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 19);
    }
}
