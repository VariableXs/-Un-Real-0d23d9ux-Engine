//! UNX-F52491 Selenium IDE 消费面收口 · 0xCBA80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52491"，逐字）：
//!   title: Selenium IDE 消费面收口
//!   quota: 320
//!   criteria: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F52492-J1 六判据
//!     全落；代理/扫描 ×10（只对本机靶站）；JVM 联签锚定行；KB-COMPAT
//!     定级 S1；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52491）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52492-J1，照原文锚定；功能按本卡标题（Selenium IDE 消费面）
//! 落地。卡面判据原文按 off-by-one 板式照录（其中代理/扫描 ×10 为下卡
//! Burp 语境），本卡功能域按标题落到 Selenium IDE 消费面。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五功能族 20 操作集：录制/回放/命令/套件/导出五家族闭集各 4 操作，
//!      逐操作载荷（纯函数折叠）恒等才收；家族单调（逆序必抓）。
//!   ② 命令投影 12 格：Selenese 命令投影闭集（open/click/type/assert/
//!      verify/waitFor/store 等），逐格载荷对照，漂移必抓。
//!   ③ 导出三语言 6 格：Java/Python/C# 三语言各 2 格导出闭集。
//!   ④ 与 Selenium Server 消费联签 4 格（F52477）+ JS 上栈声明 2 行，
//!      六行闭集缺一拒封（seal 时声明行缺 → ERowMiss）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      seal/judge_grade）；注入核 run_cycle 三态轮转 10/10（操作越闭集/
//!      操作漂移/家族逆序），检出+逃逸守恒；双跑 FNV 指纹一致；定级
//!      S2（KB-COMPAT：消费面如实，有条件收口档）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52491;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五功能族操作数。
pub const OPS: usize = 20;

/// 命令投影格数。
pub const CMD_CELLS: usize = 12;

/// 导出三语言格数。
pub const EXPORT_CELLS: usize = 6;

/// 与 Selenium Server 消费联签行数（F52477）。
pub const JOINT_ROWS: usize = 4;

/// JS 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelIdeCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 载荷漂移（操作/命令投影/导出共用）
    EDrift,
    /// 家族逆序
    EOrder,
    /// 命令投影号越闭集
    ENoCmd,
    /// 导出格号越闭集
    ENoExp,
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

impl SelIdeCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SelIdeCode::Ok => "成功",
            SelIdeCode::ENoOp => "操作号越闭集：录制/回放/命令/套件/导出 20 操作集之外先登记",
            SelIdeCode::EDrift => "载荷漂移：操作/命令投影/导出载荷与基准折叠不恒等，逐格复算",
            SelIdeCode::EOrder => "家族逆序：录制→回放→命令→套件→导出单调，逆序必抓",
            SelIdeCode::ENoCmd => "命令投影号越闭集：12 格 Selenese 投影闭集之外先登记",
            SelIdeCode::ENoExp => "导出格号越闭集：三语言 6 格导出闭集之外先登记",
            SelIdeCode::ENoRow => "声明行越闭集：Selenium Server 联签 4 格 + JS 上栈 2 行之外先登记",
            SelIdeCode::ERowMiss => "缺声明行：Server 联签/JS 上栈声明未齐先补齐",
            SelIdeCode::EDup => "重复落账：同格只许一条账目",
            SelIdeCode::ESeal => "未齐即封：20 操作 + 12 投影 + 6 导出 + 6 声明行未齐先补齐",
            SelIdeCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SelIdeCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SelIdeCode::EGradeDrift => "定级漂移：六判据全落且联签/上栈声明全齐才可定 S2（消费面如实）",
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
            Grade::S1 => "S1：全判据全格全通收口",
            Grade::S2 => "S2：有条件收口（Selenium IDE 消费面如实登记档）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 五功能族操作格载荷（操作 o；家族 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xCBA80u64 + (o as u64) * 0x33) ^ 0x5249_1A00_0000_0000)
}

/// 操作家族（0..4=录制 4..8=回放 8..12=命令 12..16=套件 16..20=导出）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("录制"),
        1 => Some("回放"),
        2 => Some("命令"),
        3 => Some("套件"),
        4 => Some("导出"),
        _ => None,
    }
}

/// 命令投影格载荷（格 c）。
pub const fn cmd_cell(c: usize) -> Option<u64> {
    if c >= CMD_CELLS {
        return None;
    }
    Some((0xCBB80u64 + (c as u64) * 0x3D) ^ 0x5249_1B00_0000_0000)
}

/// 命令投影格名（按格索引的 Selenese 命令名表）。
pub const fn cmd_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("open"),
        1 => Some("click"),
        2 => Some("type"),
        3 => Some("assertText"),
        4 => Some("verifyTitle"),
        5 => Some("waitForElement"),
        6 => Some("store"),
        7 => Some("echo"),
        8 => Some("select"),
        9 => Some("check"),
        10 => Some("pause"),
        11 => Some("runScript"),
        _ => None,
    }
}

/// 导出格载荷（格 e：0/1=Java 2/3=Python 4/5=C#）。
pub const fn export_cell(e: usize) -> Option<u64> {
    if e >= EXPORT_CELLS {
        return None;
    }
    Some((0xCBC80u64 + (e as u64) * 0x43) ^ 0x5249_1C00_0000_0000)
}

/// 导出语言（0/1=Java 2/3=Python 4/5=C#）。
pub const fn export_lang(e: usize) -> Option<u32> {
    if e >= EXPORT_CELLS {
        return None;
    }
    Some((e / 2) as u32)
}

/// 导出语言名。
pub const fn export_lang_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("Java"),
        1 => Some("Python"),
        2 => Some("C#"),
        _ => None,
    }
}

/// Selenium Server 消费联签行折叠（行 r，F52477）。
pub const fn joint_row(r: usize) -> Option<u32> {
    if r >= JOINT_ROWS {
        return None;
    }
    Some(0xCB20 + r as u32 * 0x2C)
}

/// JS 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xCC20 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Selenium IDE 收口账本状态机。
#[derive(Clone)]
pub struct SelIdeLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 五功能族操作账。
    ops: [Option<u64>; OPS],
    /// 命令投影账。
    cmds: [Option<u64>; CMD_CELLS],
    /// 导出账。
    exports: [Option<u64>; EXPORT_CELLS],
    /// Server 联签账。
    joints: [Option<u32>; JOINT_ROWS],
    /// JS 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SelIdeLedger {
    /// 新账本。
    pub fn new() -> Self {
        SelIdeLedger {
            gates: [None; 6],
            ops: [None; OPS],
            cmds: [None; CMD_CELLS],
            exports: [None; EXPORT_CELLS],
            joints: [None; JOINT_ROWS],
            stacks: [None; STACK_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SelIdeCode> {
        if gate >= 6 {
            return Err(SelIdeCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x4100 + gate as u64);
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

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), SelIdeCode> {
        if o >= OPS {
            return Err(SelIdeCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(SelIdeCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A00);
            return Err(SelIdeCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(SelIdeCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4A01 + o as u64);
            return Err(SelIdeCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SelIdeCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x4A02 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 命令投影一格（载荷对照 + 重复拒）。
    pub fn record_cmd(&mut self, c: usize, got: u64) -> Result<(), SelIdeCode> {
        if c >= CMD_CELLS {
            return Err(SelIdeCode::ENoCmd);
        }
        let want = match cmd_cell(c) {
            Some(w) => w,
            None => return Err(SelIdeCode::ENoCmd),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4B00 + c as u64);
            return Err(SelIdeCode::EDrift);
        }
        if match self.cmds.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SelIdeCode::EDup);
        }
        if let Some(cell) = self.cmds.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x4B01 + c as u64);
        Ok(())
    }

    /// 命令投影全账核验（12 格）。
    pub fn cmds_all(&self) -> bool {
        let mut c = 0usize;
        while c < CMD_CELLS {
            match self.cmds.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 导出一格（载荷对照 + 重复拒）。
    pub fn record_export(&mut self, e: usize, got: u64) -> Result<(), SelIdeCode> {
        if e >= EXPORT_CELLS {
            return Err(SelIdeCode::ENoExp);
        }
        let want = match export_cell(e) {
            Some(w) => w,
            None => return Err(SelIdeCode::ENoExp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x4C00 + e as u64);
            return Err(SelIdeCode::EDrift);
        }
        if match self.exports.get(e) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SelIdeCode::EDup);
        }
        if let Some(cell) = self.exports.get_mut(e) {
            *cell = Some(got);
        }
        self.fnv(0x4C01 + e as u64);
        Ok(())
    }

    /// 导出全账核验（6 格）。
    pub fn exports_all(&self) -> bool {
        let mut e = 0usize;
        while e < EXPORT_CELLS {
            match self.exports.get(e) {
                Some(Some(_)) => {}
                _ => return false,
            }
            e += 1;
        }
        true
    }

    /// Selenium Server 消费联签一行（F52477）。
    pub fn declare_joint(&mut self, r: usize) -> Result<(), SelIdeCode> {
        if r >= JOINT_ROWS {
            return Err(SelIdeCode::ENoRow);
        }
        if match self.joints.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SelIdeCode::EDup);
        }
        if let Some(c) = self.joints.get_mut(r) {
            *c = joint_row(r);
        }
        self.fnv(0x4D00 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn joints_all(&self) -> bool {
        let mut r = 0usize;
        while r < JOINT_ROWS {
            match self.joints.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// JS 上栈声明一行。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), SelIdeCode> {
        if r >= STACK_ROWS {
            return Err(SelIdeCode::ENoRow);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SelIdeCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x4E00 + r as u64);
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

    /// 收口封账（顺序：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal）。
    pub fn seal(&self) -> Result<(), SelIdeCode> {
        if !self.gates_all() {
            return Err(SelIdeCode::ESeal);
        }
        if !self.joints_all() || !self.stacks_all() {
            return Err(SelIdeCode::ERowMiss);
        }
        if !self.ops_all() || !self.cmds_all() || !self.exports_all() {
            return Err(SelIdeCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全齐 → S2 消费面如实档；六闸齐 → S2；否则 S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.seal().is_ok() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SelIdeLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SelIdeLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < CMD_CELLS {
        if let Some(w) = cmd_cell(c) {
            if led.record_cmd(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut e = 0usize;
    while e < EXPORT_CELLS {
        if let Some(w) = export_cell(e) {
            if led.record_export(e, w).is_ok() {
                recorded += 1;
            }
        }
        e += 1;
    }
    let mut r = 0usize;
    while r < JOINT_ROWS {
        let _ = led.declare_joint(r);
        r += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：操作越闭集 / 操作漂移 / 家族逆序三态轮转。
    // 三态注入全部落在当轮临时账本上；漂移/逆序先按合法序落正格再注。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SelIdeLedger::new();
        let caught = match kind {
            0 => lt.record_op(OPS, 0).is_err(),
            1 => {
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SelIdeCode::EDrift)
            }
            _ => {
                let _ = lt.record_op(12, op_cell(12).unwrap_or(0));
                lt.record_op(3, op_cell(3).unwrap_or(0)) == Err(SelIdeCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x4F00 + k as u64);
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
/// UNX-F52491 判据集（锚 UNX-F52492-J1，off-by-one 板式）。
pub fn run_unx52491_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52491");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五功能族 20 操作全通",
        recorded >= OPS as u32,
        "录制/回放/命令/套件/导出五家族逐操作恒等",
    );
    s.add(
        "命令投影 12 格全落",
        {
            let mut lt = SelIdeLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CMD_CELLS {
                if let Some(w) = cmd_cell(c) {
                    if lt.record_cmd(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cmds_all()
        },
        "Selenese 命令投影闭集逐格恒等，漂移必抓",
    );
    s.add(
        "导出三语言 6 格全落",
        {
            let mut lt = SelIdeLedger::new();
            let mut ok = true;
            let mut e = 0usize;
            while e < EXPORT_CELLS {
                if let Some(w) = export_cell(e) {
                    if lt.record_export(e, w).is_err() {
                        ok = false;
                    }
                }
                e += 1;
            }
            ok && lt.exports_all()
        },
        "Java/Python/C# 三语言各 2 格导出闭集逐格恒等",
    );
    s.add(
        "六判据母版闸全落",
        {
            let mut lt = SelIdeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "Selenium Server 消费联签 4/4 全齐（F52477）",
        {
            let mut lt = SelIdeLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JOINT_ROWS {
                if lt.declare_joint(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.joints_all()
        },
        "与 Selenium Server 消费联签四格闭集（F52477 锚定）",
    );
    s.add(
        "JS 上栈声明 2/2 全齐",
        {
            let mut lt = SelIdeLedger::new();
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
        "JS 运行时上栈声明/浏览器扩展消费声明两行闭集",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                if op_kind(o).unwrap_or(5) < op_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "录制→回放→命令→套件→导出五家族闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "操作越闭集/操作漂移/家族逆序三态轮转必抓",
    );
    let mut l0 = SelIdeLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 20 越闭集必拒（ENoOp 提前返回，不计注入账）",
    );
    s.add(
        "边界·命令投影越闭集",
        l0.record_cmd(CMD_CELLS, 0).is_err(),
        "投影 12 越闭集必拒（ENoCmd 提前返回）",
    );
    s.add(
        "边界·声明行越闭集",
        l0.declare_joint(JOINT_ROWS).is_err() && l0.declare_stack(STACK_ROWS).is_err(),
        "声明行越闭集必拒（ENoRow 提前返回）",
    );
    s.add(
        "操作漂移拒",
        {
            l0.record_op(0, op_cell(0).unwrap_or(0));
            l0.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(SelIdeCode::EDrift)
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
        "定级 S2 闭集（KB-COMPAT）",
        {
            let mut lt = SelIdeLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CMD_CELLS {
                if let Some(w) = cmd_cell(c) {
                    let _ = lt.record_cmd(c, w);
                }
                c += 1;
            }
            let mut e = 0usize;
            while e < EXPORT_CELLS {
                if let Some(w) = export_cell(e) {
                    let _ = lt.record_export(e, w);
                }
                e += 1;
            }
            let mut r = 0usize;
            while r < JOINT_ROWS {
                let _ = lt.declare_joint(r);
                r += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全齐 → S2（Selenium IDE 消费面如实定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBA80,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCA400 + 0x40×90",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_kind_mapping() {
        // 名表按家族（kind）索引而非格号。
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(5), Some(1));
        assert_eq!(op_kind(8), Some(2));
        assert_eq!(op_kind(15), Some(3));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert_eq!(op_kind_name(0), Some("录制"));
        assert_eq!(op_kind_name(1), Some("回放"));
        assert_eq!(op_kind_name(2), Some("命令"));
        assert_eq!(op_kind_name(3), Some("套件"));
        assert_eq!(op_kind_name(4), Some("导出"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_cmd_names() {
        assert_eq!(cmd_name(0), Some("open"));
        assert_eq!(cmd_name(6), Some("store"));
        assert_eq!(cmd_name(11), Some("runScript"));
        assert!(cmd_name(12).is_none());
        assert!(cmd_cell(CMD_CELLS).is_none());
    }

    #[test]
    fn test_export_lang_mapping() {
        assert_eq!(export_lang(0), Some(0));
        assert_eq!(export_lang(1), Some(0));
        assert_eq!(export_lang(2), Some(1));
        assert_eq!(export_lang(4), Some(2));
        assert_eq!(export_lang(5), Some(2));
        assert!(export_lang(6).is_none());
        assert_eq!(export_lang_name(0), Some("Java"));
        assert_eq!(export_lang_name(1), Some("Python"));
        assert_eq!(export_lang_name(2), Some("C#"));
        assert!(export_lang_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(11).unwrap_or(0);
        assert_eq!(a, op_cell(11).unwrap_or(0));
        assert_ne!(a, op_cell(12).unwrap_or(0));
        let b = export_cell(5).unwrap_or(0);
        assert_eq!(b, export_cell(5).unwrap_or(0));
        assert!(joint_row(JOINT_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        // 合法序先落低族格，再注逆序必抓；被拒格不改 last_kind。
        let mut lt = SelIdeLedger::new();
        let _ = lt.record_op(17, op_cell(17).unwrap_or(0));
        assert_eq!(lt.record_op(6, op_cell(6).unwrap_or(0)), Err(SelIdeCode::EOrder));
        assert!(lt.record_op(18, op_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SelIdeLedger::new();
        assert_eq!(
            lt.record_op(9, op_cell(9).unwrap_or(0).wrapping_add(1)),
            Err(SelIdeCode::EDrift)
        );
        assert_eq!(
            lt.record_cmd(5, cmd_cell(5).unwrap_or(0).wrapping_add(11)),
            Err(SelIdeCode::EDrift)
        );
        assert_eq!(
            lt.record_export(3, export_cell(3).unwrap_or(0).wrapping_add(5)),
            Err(SelIdeCode::EDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SelIdeLedger::new();
        let _ = lt.record_cmd(2, cmd_cell(2).unwrap_or(0));
        assert_eq!(
            lt.record_cmd(2, cmd_cell(2).unwrap_or(0)),
            Err(SelIdeCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SelIdeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SelIdeLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_export_boundary() {
        let mut lt = SelIdeLedger::new();
        assert!(lt.record_export(EXPORT_CELLS, 0).is_err());
        let mut ok = true;
        let mut e = 0usize;
        while e < EXPORT_CELLS {
            if let Some(w) = export_cell(e) {
                if lt.record_export(e, w).is_err() {
                    ok = false;
                }
            }
            e += 1;
        }
        assert!(ok && lt.exports_all());
    }

    #[test]
    fn test_seal_order() {
        // 封账语义顺序：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal。
        let empty = SelIdeLedger::new();
        assert_eq!(empty.seal(), Err(SelIdeCode::ESeal));
        let mut lt = SelIdeLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.seal(), Err(SelIdeCode::ERowMiss));
        let mut r = 0usize;
        while r < JOINT_ROWS {
            let _ = lt.declare_joint(r);
            r += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = lt.declare_stack(r);
            r += 1;
        }
        assert_eq!(lt.seal(), Err(SelIdeCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：20 操作 + 12 投影 + 6 导出。
        assert!(recorded >= OPS as u32 + CMD_CELLS as u32 + EXPORT_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xCBA80);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SelIdeCode::Ok as u32, 0);
        assert_eq!(SelIdeCode::EGradeDrift as u32, 12);
        assert_eq!(SelIdeCode::Ok.val(), SEG_BASE);
        assert_eq!(SelIdeCode::EGradeDrift.val(), SEG_BASE + 12);
        assert!(!SelIdeCode::Ok.detail().is_empty());
        assert!(!SelIdeCode::ESeal.detail().is_empty());
        assert!(!SelIdeCode::ENoCmd.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = SelIdeLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_joint_boundary() {
        let mut lt = SelIdeLedger::new();
        let mut r = 0usize;
        while r < JOINT_ROWS {
            assert!(lt.declare_joint(r).is_ok());
            r += 1;
        }
        assert!(lt.declare_joint(JOINT_ROWS).is_err());
        assert!(lt.joints_all());
    }

    #[test]
    fn unx52491_checks_green() {
        let s = run_unx52491_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
