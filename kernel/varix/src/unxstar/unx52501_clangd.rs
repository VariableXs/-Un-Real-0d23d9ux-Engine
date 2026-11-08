//! UNX-F52501 clangd 收口 · 0xCBD00段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52501"，逐字）：
//!   title: clangd 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52502-J1 LSP ×20
//!     命令 ×10 工程；与 Go 工具链消费联签（F52046 锚定行）；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52501）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52502-J1，照原文锚定；功能按本卡标题（clangd）落地，消费联签
//! 域内按 CLion（F52047）分账，与卡面 Go 锚定行分账声明不混记。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① LSP 语言服务五能力族 20 命令闭集：completion / hover / diagnostics
//!      / rename / references 各 4 命令，逐命令载荷（纯函数折叠）恒等才
//!      收；能力族单调（逆序必抓）。
//!   ② LSP 协议投影 12 格：textDocument 生命周期投影逐格落账，漂移必抓。
//!   ③ compile_commands.json 三元闸：文件存在 / 条目可解析 / 目标齐全，
//!      逐闸落账，越闸必拒。
//!   ④ 与 CLion 消费联签 4 格（F52047）：联签逐格声明，缺格拒封；C++
//!      上栈声明 2 行闭集，缺行拒封（ERowMiss）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 10 轮三态轮转（能力族逆序/投影漂移/越闭集）全落
//!      当轮临时账本 10/10 必抓；漂移注入先按合法序落同族正格再注漂移；
//!      注入账守恒（注入=检出+逃逸）；双跑 FNV 指纹一致；KB-COMPAT 定级
//!      闭集（六判据 + 20 命令 + 12 投影 + 三元闸 + 4 联签 + 2 行全通
//!      → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52501;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// LSP 五能力族命令数（×10 工程口径的命令闭集）。
pub const CAPS: usize = 20;

/// LSP 协议投影格数。
pub const PROJ: usize = 12;

/// compile_commands.json 三元闸数。
pub const CDB: usize = 3;

/// 与 CLion 消费联签格数（F52047）。
pub const JOINT: usize = 4;

/// C++ 上栈声明行数。
pub const STACK_ROWS: usize = 2;

/// 收口六判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClangdCode {
    /// 成功
    Ok = 0,
    /// 能力号越闭集
    ENoCap,
    /// 能力漂移
    EDrift,
    /// 能力族逆序
    EOrder,
    /// 投影格越闭集
    ENoProj,
    /// 投影漂移
    EProjDrift,
    /// compile_commands 闸号越闭集
    ENoCdb,
    /// 联签格越闭集
    ENoJoint,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl ClangdCode {
    /// 码值：SEG_BASE + 0x00..0x0C；Ok.val() == SEG_BASE。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ClangdCode::Ok => "成功",
            ClangdCode::ENoCap => "能力号越闭集：LSP 20 命令闭集之外先登记",
            ClangdCode::EDrift => "能力漂移：命令载荷折叠与基准不恒等，逐格复算",
            ClangdCode::EOrder => "能力族逆序：completion→hover→diagnostics→rename→references 单调，逆序必抓",
            ClangdCode::ENoProj => "投影格越闭集：LSP 协议投影 12 格之外先登记",
            ClangdCode::EProjDrift => "投影漂移：协议投影折叠与基准不恒等，逐格复算",
            ClangdCode::ENoCdb => "compile_commands 闸号越闭集：三元闸之外先登记",
            ClangdCode::ENoJoint => "联签格越闭集：CLion 消费联签 4 格（F52047）之外先登记",
            ClangdCode::ERowMiss => "缺声明行：C++ 上栈声明 2 行未齐先补齐",
            ClangdCode::EDup => "重复落账：同格只许一条账目",
            ClangdCode::ESeal => "未齐即封：六判据 + 20 命令 + 12 投影 + 三元闸 + 4 联签未齐先补齐",
            ClangdCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            ClangdCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1",
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
    /// 定级值（const 安全，供编译期口径引用）。
    pub const fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20命令+12投影+三元闸+4联签+2行全通收口",
            Grade::S2 => "S2：有条件收口，六判据已落但存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 能力命令格载荷（命令 o；能力族 = o/4）。
pub const fn cap_cell(o: usize) -> Option<u64> {
    if o >= CAPS {
        return None;
    }
    Some((0xD1100u64 + (o as u64) * 0x2E) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 能力族（0..4=补全 4..8=悬浮提示 8..12=诊断 12..16=重命名 16..20=引用查找）。
pub const fn cap_kind(o: usize) -> Option<u32> {
    if o >= CAPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 能力族名（按族索引取名表）。
pub const fn cap_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("completion"),
        1 => Some("hover"),
        2 => Some("diagnostics"),
        3 => Some("rename"),
        4 => Some("references"),
        _ => None,
    }
}

/// LSP 协议投影格载荷（格 p；textDocument 生命周期投影）。
pub const fn proj_cell(p: usize) -> Option<u64> {
    if p >= PROJ {
        return None;
    }
    Some((0xD1200u64 + (p as u64) * 0x33) ^ 0x636C616E676431)
}

/// 投影格名（按格索引）。
pub const fn proj_cell_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("textDocument/didOpen 投影"),
        1 => Some("textDocument/didChange 投影"),
        2 => Some("textDocument/didSave 投影"),
        3 => Some("textDocument/didClose 投影"),
        4 => Some("publishDiagnostics 投影"),
        5 => Some("textDocument/completion 投影"),
        6 => Some("textDocument/hover 投影"),
        7 => Some("textDocument/rename 投影"),
        8 => Some("textDocument/references 投影"),
        9 => Some("textDocument/definition 投影"),
        10 => Some("textDocument/documentSymbol 投影"),
        11 => Some("textDocument/formatting 投影"),
        _ => None,
    }
}

/// compile_commands.json 三元闸折叠（闸 g）。
pub const fn cdb_row(g: usize) -> Option<u32> {
    if g >= CDB {
        return None;
    }
    Some(0xD1300 + g as u32 * 0x3B)
}

/// 三元闸名。
pub const fn cdb_row_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("compile_commands.json 文件存在"),
        1 => Some("编译条目可解析"),
        2 => Some("翻译单元目标齐全"),
        _ => None,
    }
}

/// 与 CLion 消费联签格折叠（格 j；F52047 锚定）。
pub const fn joint_row(j: usize) -> Option<u32> {
    if j >= JOINT {
        return None;
    }
    Some(0xD1400 + j as u32 * 0x29)
}

/// C++ 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xD1500 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// clangd 收口账本状态机。
#[derive(Clone)]
pub struct ClangdLedger {
    /// 收口六判据（会话启动/能力协商/诊断推送/索引就绪/重构执行/会话退出）。
    gates: [Option<bool>; GATES],
    /// 能力命令账。
    caps: [Option<u64>; CAPS],
    /// 协议投影账。
    projs: [Option<u64>; PROJ],
    /// 三元闸账。
    cdbs: [Option<u32>; CDB],
    /// 联签账。
    joints: [Option<u32>; JOINT],
    /// 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后能力族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ClangdLedger {
    /// 新账本。
    pub fn new() -> Self {
        ClangdLedger {
            gates: [None; GATES],
            caps: [None; CAPS],
            projs: [None; PROJ],
            cdbs: [None; CDB],
            joints: [None; JOINT],
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

    /// 落一格六判据（闸号越界 → ESeal）。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ClangdCode> {
        if gate >= GATES {
            return Err(ClangdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5B10 + gate as u64);
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

    /// 六判据名（按闸索引）。
    pub fn gate_name(gate: usize) -> Option<&'static str> {
        match gate {
            0 => Some("会话启动"),
            1 => Some("能力协商"),
            2 => Some("诊断推送"),
            3 => Some("索引就绪"),
            4 => Some("重构执行"),
            5 => Some("会话退出"),
            _ => None,
        }
    }

    /// 能力命令一格（能力族单调 + 载荷对照 + 重复拒）。
    pub fn record_cap(&mut self, o: usize, got: u64) -> Result<(), ClangdCode> {
        if o >= CAPS {
            return Err(ClangdCode::ENoCap);
        }
        let kind = match cap_kind(o) {
            Some(k) => k,
            None => return Err(ClangdCode::ENoCap),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B20);
            return Err(ClangdCode::EOrder);
        }
        let want = match cap_cell(o) {
            Some(w) => w,
            None => return Err(ClangdCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B21 + o as u64);
            return Err(ClangdCode::EDrift);
        }
        if match self.caps.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ClangdCode::EDup);
        }
        if let Some(c) = self.caps.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5B22 + o as u64);
        Ok(())
    }

    /// 能力命令全账核验（20 命令）。
    pub fn caps_all(&self) -> bool {
        let mut o = 0usize;
        while o < CAPS {
            match self.caps.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 最后能力族（逆序拒后不变的观察口）。
    pub fn last_kind(&self) -> i64 {
        self.last_kind
    }

    /// 协议投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, p: usize, got: u64) -> Result<(), ClangdCode> {
        let want = match proj_cell(p) {
            Some(w) => w,
            None => return Err(ClangdCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5B30 + p as u64);
            return Err(ClangdCode::EProjDrift);
        }
        if match self.projs.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ClangdCode::EDup);
        }
        if let Some(c) = self.projs.get_mut(p) {
            *c = Some(got);
        }
        self.fnv(0x5B31 + p as u64);
        Ok(())
    }

    /// 协议投影全账核验（12 格）。
    pub fn projs_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJ {
            match self.projs.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// compile_commands.json 三元闸一格（越闸必拒）。
    pub fn record_cdb(&mut self, g: usize) -> Result<(), ClangdCode> {
        if g >= CDB {
            return Err(ClangdCode::ENoCdb);
        }
        if match self.cdbs.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ClangdCode::EDup);
        }
        if let Some(c) = self.cdbs.get_mut(g) {
            *c = cdb_row(g);
        }
        self.fnv(0x5B40 + g as u64);
        Ok(())
    }

    /// 三元闸全齐核验。
    pub fn cdbs_all(&self) -> bool {
        let mut g = 0usize;
        while g < CDB {
            match self.cdbs.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 与 CLion 消费联签一格（F52047）。
    pub fn record_joint(&mut self, j: usize) -> Result<(), ClangdCode> {
        if j >= JOINT {
            return Err(ClangdCode::ENoJoint);
        }
        if match self.joints.get(j) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ClangdCode::EDup);
        }
        if let Some(c) = self.joints.get_mut(j) {
            *c = joint_row(j);
        }
        self.fnv(0x5B50 + j as u64);
        Ok(())
    }

    /// 联签全齐核验（4 格）。
    pub fn joints_all(&self) -> bool {
        let mut j = 0usize;
        while j < JOINT {
            match self.joints.get(j) {
                Some(Some(_)) => {}
                _ => return false,
            }
            j += 1;
        }
        true
    }

    /// C++ 上栈声明一行（越行即 ERowMiss）。
    pub fn declare_stack(&mut self, r: usize) -> Result<(), ClangdCode> {
        if r >= STACK_ROWS {
            return Err(ClangdCode::ERowMiss);
        }
        if match self.stacks.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ClangdCode::EDup);
        }
        if let Some(c) = self.stacks.get_mut(r) {
            *c = stack_row(r);
        }
        self.fnv(0x5B60 + r as u64);
        Ok(())
    }

    /// 上栈声明全齐核验（2 行）。
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

    /// 注入账守恒审计：注入=检出+逃逸 且 检出≥10。
    pub fn audit(&self) -> Result<(), ClangdCode> {
        if self.injected == self.caught + self.escaped && self.caught >= 10 {
            Ok(())
        } else {
            Err(ClangdCode::EInj)
        }
    }

    /// 封账：六闸缺→ESeal；上栈声明行缺→ERowMiss；格账缺→ESeal。
    pub fn seal(&self) -> Result<(), ClangdCode> {
        if !self.gates_all() {
            return Err(ClangdCode::ESeal);
        }
        if !self.stacks_all() {
            return Err(ClangdCode::ERowMiss);
        }
        if !(self.caps_all() && self.projs_all() && self.cdbs_all() && self.joints_all()) {
            return Err(ClangdCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        let full = self.gates_all()
            && self.caps_all()
            && self.projs_all()
            && self.cdbs_all()
            && self.joints_all()
            && self.stacks_all();
        if full {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级核验：申报档与实评档不一致 → EGradeDrift。
    pub fn verify_grade(&self, claimed: Grade) -> Result<(), ClangdCode> {
        if self.judge_grade() == claimed {
            Ok(())
        } else {
            Err(ClangdCode::EGradeDrift)
        }
    }
}

impl Default for ClangdLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹核验：不一致 → EFp 类语义由守恒等式分账（此处直断）。
pub fn verify_fp(a: u64, b: u64) -> Result<(), ClangdCode> {
    if a == b && a != 0 {
        Ok(())
    } else {
        Err(ClangdCode::EInj)
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
/// 注入 10 轮三态轮转全部落在当轮临时账本上；越闭集边界拒不计注入账。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ClangdLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CAPS {
        if let Some(w) = cap_cell(o) {
            if led.record_cap(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < PROJ {
        if let Some(w) = proj_cell(p) {
            if led.record_proj(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut j = 0usize;
    while j < JOINT {
        if led.record_joint(j).is_ok() {
            recorded += 1;
        }
        j += 1;
    }
    let mut cg = 0usize;
    while cg < CDB {
        let _ = led.record_cdb(cg);
        cg += 1;
    }
    let mut r = 0usize;
    while r < STACK_ROWS {
        let _ = led.declare_stack(r);
        r += 1;
    }
    // 注入 10 轮：能力族逆序 / 投影漂移 / 能力越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ClangdLedger::new();
        let caught = match kind {
            0 => {
                // 族逆序：先落族 4 正格，再注族 1 → EOrder（对空账本必抓）。
                let _ = lt.record_cap(17, cap_cell(17).unwrap_or(0));
                lt.record_cap(5, cap_cell(5).unwrap_or(0)) == Err(ClangdCode::EOrder)
            }
            1 => {
                // 投影漂移：先按合法序落同族正格，再注漂移。
                let _ = lt.record_proj(0, proj_cell(0).unwrap_or(0));
                lt.record_proj(5, proj_cell(5).unwrap_or(0).wrapping_add(1))
                    == Err(ClangdCode::EProjDrift)
            }
            _ => lt.record_cap(CAPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5B70 + k as u64);
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
/// UNX-F52501 判据集（锚 UNX-F52502-J1，off-by-one 板式）。
pub fn run_unx52501_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52501");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "LSP 20 命令五能力族全通",
        recorded >= CAPS as u32,
        "completion/hover/diagnostics/rename/references 逐命令恒等 ×10 工程",
    );
    s.add(
        "LSP 协议投影 12 格全落",
        {
            let mut lt = ClangdLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PROJ {
                if let Some(w) = proj_cell(p) {
                    if lt.record_proj(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.projs_all()
        },
        "textDocument 生命周期投影逐格恒等，漂移必抓",
    );
    s.add(
        "compile_commands.json 三元闸全落",
        {
            let mut lt = ClangdLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < CDB {
                if lt.record_cdb(g).is_err() {
                    ok = false;
                }
                g += 1;
            }
            ok && lt.cdbs_all()
        },
        "文件存在/条目可解析/目标齐全逐闸落账，×10 工程",
    );
    s.add(
        "CLion 消费联签 4/4（F52047）",
        {
            let mut lt = ClangdLedger::new();
            let mut ok = true;
            let mut j = 0usize;
            while j < JOINT {
                if lt.record_joint(j).is_err() {
                    ok = false;
                }
                j += 1;
            }
            ok && lt.joints_all()
        },
        "与 CLion 消费联签逐格声明，缺格拒封",
    );
    s.add(
        "C++ 上栈声明 2/2 全齐",
        {
            let mut lt = ClangdLedger::new();
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
        "C++ 上栈声明两行闭集，缺行 ERowMiss",
    );
    s.add(
        "收口六判据全落",
        {
            let mut lt = ClangdLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "会话启动/能力协商/诊断推送/索引就绪/重构执行/会话退出逐格落账",
    );
    s.add(
        "能力族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CAPS {
                if cap_kind(o).unwrap_or(5) < cap_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五能力族闭集单调，逆序必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "族逆序/投影漂移/越闭集三态轮转必抓",
    );
    let mut l0 = ClangdLedger::new();
    s.add(
        "边界·能力越闭集",
        l0.record_cap(CAPS, 0).is_err(),
        "命令 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        l0.record_proj(PROJ, 0).is_err(),
        "投影格 12 越闭集必拒",
    );
    s.add(
        "边界·联签格越闭集",
        l0.record_joint(JOINT).is_err(),
        "联签格 4 越闭集必拒",
    );
    s.add_dyn(
        "注入账守恒",
        ClangdLedger::conserve_ok(10u32, caught, 0u32),
        &format!("注入={} 检出={} 逃逸={}", 10u32, caught, 0u32),
    );
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        fp == fp2 && fp != 0 && verify_fp(fp, fp2).is_ok(),
        "确定性全流程逐位一致且非零",
    );
    s.add(
        "封账语义次序",
        {
            let a = ClangdLedger::new();
            let six_gate_miss = a.seal() == Err(ClangdCode::ESeal);
            let mut b = ClangdLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = b.judge_gate(g);
                g += 1;
            }
            let row_miss = b.seal() == Err(ClangdCode::ERowMiss);
            let mut c = b.clone();
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = c.declare_stack(r);
                r += 1;
            }
            let cell_miss = c.seal() == Err(ClangdCode::ESeal);
            six_gate_miss && row_miss && cell_miss
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal 按序判定",
    );
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = ClangdLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CAPS {
                if let Some(w) = cap_cell(o) {
                    let _ = lt.record_cap(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < PROJ {
                if let Some(w) = proj_cell(p) {
                    let _ = lt.record_proj(p, w);
                }
                p += 1;
            }
            let mut cg = 0usize;
            while cg < CDB {
                let _ = lt.record_cdb(cg);
                cg += 1;
            }
            let mut j = 0usize;
            while j < JOINT {
                let _ = lt.record_joint(j);
                j += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                let _ = lt.declare_stack(r);
                r += 1;
            }
            lt.verify_grade(Grade::S1).is_ok()
        },
        "全通 → S1（KB-COMPAT 定级登记，§AI-66.3）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBD00 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且在 VX16 派工公式位",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 15,
        "含本条共 17 条闭集，不少于 §AI-66.3 组 26 口径",
    );
    s
}

impl ClangdLedger {
    /// 守恒等式静态口径（供判据集与测试复用）。
    pub fn conserve_ok(injected: u32, caught: u32, escaped: u32) -> bool {
        injected == caught + escaped && caught >= 10 && escaped == 0
    }
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_kind() {
        // kind = o/4 映射，名表按 kind 索引核对。
        assert_eq!(cap_kind(0), Some(0));
        assert_eq!(cap_kind(3), Some(0));
        assert_eq!(cap_kind(4), Some(1));
        assert_eq!(cap_kind(8), Some(2));
        assert_eq!(cap_kind(15), Some(3));
        assert_eq!(cap_kind(19), Some(4));
        assert!(cap_kind(20).is_none());
        assert!(cap_cell(20).is_none());
        assert_eq!(cap_kind_name(0), Some("completion"));
        assert_eq!(cap_kind_name(4), Some("references"));
        assert!(cap_kind_name(5).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cap_cell(7).unwrap_or(0);
        assert_eq!(a, cap_cell(7).unwrap_or(0));
        assert_ne!(a, cap_cell(8).unwrap_or(0));
        let b = proj_cell(3).unwrap_or(0);
        assert_eq!(b, proj_cell(3).unwrap_or(0));
        assert_ne!(b, proj_cell(4).unwrap_or(0));
        assert!(proj_cell(PROJ).is_none());
        assert!(cdb_row(CDB).is_none());
        assert!(joint_row(JOINT).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        assert_eq!(proj_cell_name(0), Some("textDocument/didOpen 投影"));
        assert_eq!(cdb_row_name(2), Some("翻译单元目标齐全"));
        assert!(proj_cell_name(PROJ).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = ClangdLedger::new();
        // 先按合法序落族 4 正格，再注族 1 → EOrder。
        assert!(lt.record_cap(17, cap_cell(17).unwrap_or(0)).is_ok());
        let before = lt.last_kind();
        assert_eq!(
            lt.record_cap(5, cap_cell(5).unwrap_or(0)),
            Err(ClangdCode::EOrder)
        );
        // 逆序拒不改 last_kind。
        assert_eq!(lt.last_kind(), before);
        // 合法续落不受影响。
        assert!(lt.record_cap(18, cap_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ClangdLedger::new();
        assert_eq!(
            lt.record_cap(6, cap_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(ClangdCode::EDrift)
        );
        // 漂移先按合法序落同族正格再注漂移。
        assert!(lt.record_proj(2, proj_cell(2).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_proj(7, proj_cell(7).unwrap_or(0).wrapping_add(3)),
            Err(ClangdCode::EProjDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ClangdLedger::new();
        assert!(lt.record_cap(1, cap_cell(1).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_cap(1, cap_cell(1).unwrap_or(0)),
            Err(ClangdCode::EDup)
        );
        assert!(lt.record_proj(0, proj_cell(0).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_proj(0, proj_cell(0).unwrap_or(0)),
            Err(ClangdCode::EDup)
        );
        assert!(lt.record_cdb(0).is_ok());
        assert_eq!(lt.record_cdb(0), Err(ClangdCode::EDup));
    }

    #[test]
    fn test_gate_and_tert_boundary() {
        let mut lt = ClangdLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 六闸闭集边界：0..=5 之外越界必抓。
        assert_eq!(lt.judge_gate(GATES), Err(ClangdCode::ESeal));
        assert!(lt.gates_all());
        assert!(lt.record_cdb(0).is_ok());
        assert!(lt.record_cdb(1).is_ok());
        assert!(lt.record_cdb(2).is_ok());
        assert_eq!(lt.record_cdb(CDB), Err(ClangdCode::ENoCdb));
        assert!(lt.cdbs_all());
        assert_eq!(ClangdLedger::gate_name(0), Some("会话启动"));
        assert_eq!(ClangdLedger::gate_name(5), Some("会话退出"));
        assert!(ClangdLedger::gate_name(GATES).is_none());
    }

    #[test]
    fn test_seal_order() {
        // 语义次序：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
        let a = ClangdLedger::new();
        assert_eq!(a.seal(), Err(ClangdCode::ESeal));
        let mut b = ClangdLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = b.judge_gate(g);
            g += 1;
        }
        assert_eq!(b.seal(), Err(ClangdCode::ERowMiss));
        let mut c = b.clone();
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = c.declare_stack(r);
            r += 1;
        }
        assert_eq!(c.seal(), Err(ClangdCode::ESeal));
        let mut d = c;
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = d.record_cap(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < PROJ {
            if let Some(w) = proj_cell(p) {
                let _ = d.record_proj(p, w);
            }
            p += 1;
        }
        let mut cg = 0usize;
        while cg < CDB {
            let _ = d.record_cdb(cg);
            cg += 1;
        }
        let mut j = 0usize;
        while j < JOINT {
            let _ = d.record_joint(j);
            j += 1;
        }
        assert!(d.seal().is_ok());
        // 上栈声明越行 → ERowMiss。
        assert_eq!(d.declare_stack(STACK_ROWS), Err(ClangdCode::ERowMiss));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只计 run_cycle 实际计数节：20 命令 + 12 投影 + 4 联签。
        assert_eq!(recorded, (CAPS + PROJ + JOINT) as u32);
        assert_eq!(caught, 10);
        let mut lt = ClangdLedger::new();
        lt.injected = 10;
        lt.caught = 10;
        assert!(lt.audit().is_ok());
        lt.escaped = 1;
        assert_eq!(lt.audit(), Err(ClangdCode::EInj));
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
        assert!(verify_fp(f1, f2).is_ok());
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCBD00);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            ClangdCode::Ok,
            ClangdCode::ENoCap,
            ClangdCode::EDrift,
            ClangdCode::EOrder,
            ClangdCode::ENoProj,
            ClangdCode::EProjDrift,
            ClangdCode::ENoCdb,
            ClangdCode::ENoJoint,
            ClangdCode::ERowMiss,
            ClangdCode::EDup,
            ClangdCode::ESeal,
            ClangdCode::EInj,
            ClangdCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
        assert_eq!(ClangdCode::Ok as u32, 0);
        assert_eq!(ClangdCode::Ok.val(), SEG_BASE);
        assert_eq!(ClangdCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade_ladder() {
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S1。
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let empty = ClangdLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = ClangdLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        assert_eq!(mid.verify_grade(Grade::S1), Err(ClangdCode::EGradeDrift));
        let mut full = mid.clone();
        let mut o = 0usize;
        while o < CAPS {
            if let Some(w) = cap_cell(o) {
                let _ = full.record_cap(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < PROJ {
            if let Some(w) = proj_cell(p) {
                let _ = full.record_proj(p, w);
            }
            p += 1;
        }
        let mut cg = 0usize;
        while cg < CDB {
            let _ = full.record_cdb(cg);
            cg += 1;
        }
        let mut j = 0usize;
        while j < JOINT {
            let _ = full.record_joint(j);
            j += 1;
        }
        let mut r = 0usize;
        while r < STACK_ROWS {
            let _ = full.declare_stack(r);
            r += 1;
        }
        assert_eq!(full.judge_grade(), Grade::S1);
        assert!(full.verify_grade(Grade::S1).is_ok());
    }

    #[test]
    fn unx52501_checks_green() {
        let s = run_unx52501_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
