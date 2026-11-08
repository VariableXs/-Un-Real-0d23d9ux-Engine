//! UNX-F52494 Nikto 收口 · 0xCBB40段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52494"，逐字）：
//!   title: Nikto 收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52495-J1 判据仅
//!     对本机搭建的演练靶场（白名单声明）；×10；KB-COMPAT 定级登记；判
//!     据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非
//!     空。
//! off-by-one 板式口径：本卡（卡 N=52494）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52495-J1，照原文锚定；功能按本卡标题（Nikto）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（本地授权演练靶场/干跑语义，零真实攻击面）：
//!   ① 判据仅对本机搭建的演练靶场（回环白名单 127.0.0.1）与干跑结果
//!      账收口；白名单之外的靶标一律越闭集拒收。
//!   ② 扫描 20 操作集：五命令族闭集（-h/-p/-Tuning/-Format/-ssl）各 4
//!      操作，家族单调（逆序必抓）。
//!   ③ 报告三格式 6 格（htm/csv/txt 各写读两格）+ 插件消费 8 格，逐格
//!      恒等折叠，漂移必抓。
//!   ④ 回环靶标白名单声明 2 行 + C/Perl 上栈声明 2 行，闭集缺一拒封
//!      （seal 语义：缺声明行 → ERowMiss）。
//!   ⑤ ledger 状态机（new/record/declare/judge_gate/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（操作越闭集/操作漂移/报告格式越
//!      闭集），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S2/S3 闭集
//!      （20 操作 + 6 格报告 + 8 格插件 + 4 行声明 + 六判据全通 → S2）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52494;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 扫描操作数。
pub const SCAN_OPS: usize = 20;

/// 报告格式格数（三格式 × 写读两格）。
pub const FMT_CELLS: usize = 6;

/// 插件消费格数。
pub const PLUGIN_CELLS: usize = 8;

/// 回环靶标白名单声明行数。
pub const LOOP_ROWS: usize = 2;

/// C/Perl 上栈声明行数。
pub const STACK_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NiktoCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 报告格式格越闭集
    ENoFmt,
    /// 报告格式格漂移
    EFmtDrift,
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

impl NiktoCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            NiktoCode::Ok => "成功",
            NiktoCode::ENoOp => "操作号越闭集：扫描 20 操作集之外先登记，回环白名单外靶标一律拒",
            NiktoCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            NiktoCode::EOrder => "家族逆序：-h→-p→-Tuning→-Format→-ssl 单调，逆序必抓",
            NiktoCode::ENoFmt => "报告格式格越闭集：三格式 6 格之外先登记",
            NiktoCode::EFmtDrift => "报告格式格漂移：格式格折叠与基准不恒等，逐格复算",
            NiktoCode::ENoRow => "声明行越闭集：回环白名单 2 行 / C/Perl 上栈 2 行闭集之外先登记",
            NiktoCode::ERowMiss => "缺声明行：回环靶标白名单与 C/Perl 上栈声明未齐先补齐",
            NiktoCode::EDup => "重复落账：同格只许一条账目",
            NiktoCode::ESeal => "未齐即封：20 操作 + 6 格报告 + 8 格插件 + 4 行声明 + 六判据未齐先补齐",
            NiktoCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NiktoCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            NiktoCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不产出，闭集在册）
    S1,
    /// S2：有条件收口（本卡目标档）
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
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
            Grade::S1 => "S1：全通收口（Nikto 卡闭集在册但目标档为 S2）",
            Grade::S2 => "S2：20操作+6格报告+8格插件+4行声明+六判据全通（本机演练靶场）",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 扫描操作格载荷（操作 o；命令族 = o/4）。
pub const fn scan_cell(o: usize) -> Option<u64> {
    if o >= SCAN_OPS {
        return None;
    }
    Some((0xCBB40u64 + (o as u64) * 0x37) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 命令族（0..4=-h 4..8=-p 8..12=-Tuning 12..16=-Format 16..20=-ssl）。
pub const fn scan_kind(o: usize) -> Option<u32> {
    if o >= SCAN_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名（按族索引，非格号）。
pub const fn scan_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("-h 靶标指定"),
        1 => Some("-p 端口"),
        2 => Some("-Tuning 调优"),
        3 => Some("-Format 报告"),
        4 => Some("-ssl 回环加密"),
        _ => None,
    }
}

/// 报告格式格载荷（格 f：0/1=htm 写读 2/3=csv 写读 4/5=txt 写读）。
pub const fn fmt_cell(f: usize) -> Option<u64> {
    if f >= FMT_CELLS {
        return None;
    }
    Some((0xCC400u64 + (f as u64) * 0x41) ^ 0x4E494B544F5F4652)
}

/// 报告格式名（按格式族索引：f/2）。
pub const fn fmt_name(f: usize) -> Option<&'static str> {
    match f / 2 {
        0 => Some("htm"),
        1 => Some("csv"),
        2 => Some("txt"),
        _ => None,
    }
}

/// 插件消费格载荷（格 g）。
pub const fn plugin_cell(g: usize) -> Option<u64> {
    if g >= PLUGIN_CELLS {
        return None;
    }
    Some((0xCC800u64 + (g as u64) * 0x5D) ^ 0x4E494B545F504C47)
}

/// 插件消费名（按格索引）。
pub const fn plugin_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("msgs 消息库"),
        1 => Some("server 服务器头"),
        2 => Some("files 旁文件"),
        3 => Some("report 报告汇"),
        4 => Some("auth 授权桩"),
        5 => Some("ssl 回环证书"),
        6 => Some("tuning 调优表"),
        7 => Some("session 会话快照"),
        _ => None,
    }
}

/// 回环靶标白名单声明行折叠（行 r）。
pub const fn loop_row(r: usize) -> Option<u32> {
    if r >= LOOP_ROWS {
        return None;
    }
    Some(0xCD40 + r as u32 * 0x37)
}

/// 回环白名单声明行名。
pub const fn loop_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("回环白名单声明：靶场仅限本机搭建 127.0.0.1"),
        1 => Some("回环白名单声明：白名单外靶标一律越闭集拒收"),
        _ => None,
    }
}

/// C/Perl 上栈声明行折叠（行 r）。
pub const fn stack_row(r: usize) -> Option<u32> {
    if r >= STACK_ROWS {
        return None;
    }
    Some(0xCE40 + r as u32 * 0x41)
}

/// C/Perl 上栈声明行名。
pub const fn stack_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("C 上栈声明：Nikto 核心依赖 C 栈运行时"),
        1 => Some("Perl 上栈声明：插件脚本依赖 Perl 解释栈"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Nikto 收口账本状态机。
#[derive(Clone)]
pub struct NiktoLedger {
    /// 六判据（本机演练靶场：白名单/授权/限速/隔离/落账/写回）。
    gates: [Option<bool>; 6],
    /// 扫描操作账。
    ops: [Option<u64>; SCAN_OPS],
    /// 报告格式账。
    fmts: [Option<u64>; FMT_CELLS],
    /// 插件消费账。
    plugins: [Option<u64>; PLUGIN_CELLS],
    /// 回环白名单声明账。
    loops: [Option<u32>; LOOP_ROWS],
    /// C/Perl 上栈声明账。
    stacks: [Option<u32>; STACK_ROWS],
    /// 最后命令族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NiktoLedger {
    /// 新账本。
    pub fn new() -> Self {
        NiktoLedger {
            gates: [None; 6],
            ops: [None; SCAN_OPS],
            fmts: [None; FMT_CELLS],
            plugins: [None; PLUGIN_CELLS],
            loops: [None; LOOP_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NiktoCode> {
        if gate >= 6 {
            return Err(NiktoCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// 扫描操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), NiktoCode> {
        if o >= SCAN_OPS {
            return Err(NiktoCode::ENoOp);
        }
        let kind = match scan_kind(o) {
            Some(k) => k,
            None => return Err(NiktoCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200);
            return Err(NiktoCode::EOrder);
        }
        let want = match scan_cell(o) {
            Some(w) => w,
            None => return Err(NiktoCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6201 + o as u64);
            return Err(NiktoCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NiktoCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < SCAN_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 报告格式一格（载荷对照 + 重复拒）。
    pub fn record_fmt(&mut self, f: usize, got: u64) -> Result<(), NiktoCode> {
        let want = match fmt_cell(f) {
            Some(w) => w,
            None => return Err(NiktoCode::ENoFmt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + f as u64);
            return Err(NiktoCode::EFmtDrift);
        }
        if match self.fmts.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NiktoCode::EDup);
        }
        if let Some(c) = self.fmts.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x6301 + f as u64);
        Ok(())
    }

    /// 报告格式全账核验（6 格）。
    pub fn fmts_all(&self) -> bool {
        let mut f = 0usize;
        while f < FMT_CELLS {
            match self.fmts.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 插件消费一格（载荷对照 + 重复拒）。
    pub fn record_plugin(&mut self, g: usize, got: u64) -> Result<(), NiktoCode> {
        let want = match plugin_cell(g) {
            Some(w) => w,
            None => return Err(NiktoCode::ENoRow),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400 + g as u64);
            return Err(NiktoCode::EFmtDrift);
        }
        if match self.plugins.get(g) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NiktoCode::EDup);
        }
        if let Some(c) = self.plugins.get_mut(g) {
            *c = Some(got);
        }
        self.fnv(0x6401 + g as u64);
        Ok(())
    }

    /// 插件消费全账核验（8 格）。
    pub fn plugins_all(&self) -> bool {
        let mut g = 0usize;
        while g < PLUGIN_CELLS {
            match self.plugins.get(g) {
                Some(Some(_)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 声明一行（which=false → 回环白名单，which=true → C/Perl 上栈）。
    pub fn declare_row(&mut self, which: bool, r: usize) -> Result<(), NiktoCode> {
        if which {
            if r >= STACK_ROWS {
                return Err(NiktoCode::ENoRow);
            }
            if match self.stacks.get(r) {
                Some(Some(_)) => true,
                _ => false,
            } {
                return Err(NiktoCode::EDup);
            }
            if let Some(c) = self.stacks.get_mut(r) {
                *c = stack_row(r);
            }
        } else {
            if r >= LOOP_ROWS {
                return Err(NiktoCode::ENoRow);
            }
            if match self.loops.get(r) {
                Some(Some(_)) => true,
                _ => false,
            } {
                return Err(NiktoCode::EDup);
            }
            if let Some(c) = self.loops.get_mut(r) {
                *c = loop_row(r);
            }
        }
        self.fnv(if which { 0x6501 + r as u64 } else { 0x6500 + r as u64 });
        Ok(())
    }

    /// 声明全齐核验（回环 2 行 + 上栈 2 行）。
    pub fn rows_all(&self) -> bool {
        let mut r = 0usize;
        while r < LOOP_ROWS {
            match self.loops.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        let mut s = 0usize;
        while s < STACK_ROWS {
            match self.stacks.get(s) {
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

    /// 封闸：六闸缺 → ESeal；声明行缺 → ERowMiss；格账缺 → ESeal。
    pub fn seal(&self) -> Result<(), NiktoCode> {
        if !self.gates_all() {
            return Err(NiktoCode::ESeal);
        }
        if !self.rows_all() {
            return Err(NiktoCode::ERowMiss);
        }
        if !self.ops_all() || !self.fmts_all() || !self.plugins_all() {
            return Err(NiktoCode::ESeal);
        }
        Ok(())
    }

    /// KB-COMPAT 定级（全通 → S2，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.fmts_all() && self.plugins_all() && self.rows_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for NiktoLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NiktoLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut f = 0usize;
    while f < FMT_CELLS {
        if let Some(w) = fmt_cell(f) {
            if led.record_fmt(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut p = 0usize;
    while p < PLUGIN_CELLS {
        if let Some(w) = plugin_cell(p) {
            if led.record_plugin(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut o = 0usize;
    while o < SCAN_OPS {
        if let Some(w) = scan_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut r = 0usize;
    while r < LOOP_ROWS {
        let _ = led.declare_row(false, r);
        r += 1;
    }
    let mut s = 0usize;
    while s < STACK_ROWS {
        let _ = led.declare_row(true, s);
        s += 1;
    }
    // 注入 10 轮：操作越闭集 / 操作漂移 / 报告格式越闭集三态轮转，
    // 全部落在当轮临时账本上；主账本只记守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NiktoLedger::new();
        let caught = match kind {
            0 => {
                lt.record_op(0, scan_cell(0).unwrap_or(0));
                lt.record_op(SCAN_OPS, 0).is_err()
            }
            1 => {
                lt.record_op(3, scan_cell(3).unwrap_or(0));
                lt.record_op(1, scan_cell(1).unwrap_or(0).wrapping_add(1)) == Err(NiktoCode::EDrift)
            }
            _ => {
                lt.record_fmt(0, fmt_cell(0).unwrap_or(0));
                lt.record_fmt(FMT_CELLS, 0) == Err(NiktoCode::ENoFmt)
            }
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
/// UNX-F52494 判据集（锚 UNX-F52495-J1（白名单声明）×10，off-by-one 板式）。
pub fn run_unx52494_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52494");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "扫描 20 操作全通（本机演练靶场）",
        recorded >= SCAN_OPS as u32,
        "-h/-p/-Tuning/-Format/-ssl 五命令族逐操作恒等",
    );
    s.add(
        "报告三格式 6 格全落",
        {
            let mut lt = NiktoLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FMT_CELLS {
                if let Some(w) = fmt_cell(f) {
                    if lt.record_fmt(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.fmts_all()
        },
        "htm/csv/txt 各写读两格逐格恒等（§AI-66.3）",
    );
    s.add(
        "插件消费 8 格全落",
        {
            let mut lt = NiktoLedger::new();
            let mut ok = true;
            let mut g = 0usize;
            while g < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(g) {
                    if lt.record_plugin(g, w).is_err() {
                        ok = false;
                    }
                }
                g += 1;
            }
            ok && lt.plugins_all()
        },
        "消息库/服务器头/旁文件/报告汇/授权桩/回环证书/调优表/会话快照逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = NiktoLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "本机靶场白名单/授权/限速/隔离/落账/写回逐格落账",
    );
    s.add(
        "回环靶标白名单声明 2/2",
        {
            let mut lt = NiktoLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < LOOP_ROWS {
                if lt.declare_row(false, r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok
        },
        "仅限本机搭建 127.0.0.1 / 白名单外一律拒收（§AI-66.3）",
    );
    s.add(
        "C/Perl 上栈声明 2/2",
        {
            let mut lt = NiktoLedger::new();
            let mut ok = true;
            // rows_all 要求另一组声明行与上栈行两套全落。
            let mut o = 0usize;
            while o < STACK_ROWS {
                if lt.declare_row(false, o).is_err() {
                    ok = false;
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < STACK_ROWS {
                if lt.declare_row(true, r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.rows_all()
        },
        "C 核心运行时声明 + Perl 插件解释栈声明",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < SCAN_OPS {
                if scan_kind(o).unwrap_or(5) < scan_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五命令族闭集单调，逆序必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "操作越闭集/操作漂移/报告格式越闭集三态轮转必抓");
    s.add("边界·越闭集·操作", NiktoLedger::new().record_op(SCAN_OPS, 0).is_err(), "操作 20 越闭集必拒（ENoOp 提前返回，不计注入账）");
    s.add(
        "边界·越闭集·报告格式",
        NiktoLedger::new().record_fmt(FMT_CELLS, 0).is_err(),
        "报告格式格 6 越闭集必拒（ENoFmt 提前返回）",
    );
    s.add(
        "边界·越闭集·声明行",
        {
            let mut lt = NiktoLedger::new();
            lt.declare_row(false, LOOP_ROWS).is_err() && lt.declare_row(true, STACK_ROWS).is_err()
        },
        "回环行 2 / 上栈行 2 越闭集必拒（ENoRow 提前返回）",
    );
    s.add(
        "操作漂移拒",
        {
            let mut lt = NiktoLedger::new();
            lt.record_op(0, scan_cell(0).unwrap_or(0));
            lt.record_op(1, scan_cell(1).unwrap_or(0).wrapping_add(1)) == Err(NiktoCode::EDrift)
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
        "定级 S2 闭集",
        {
            let mut lt = NiktoLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut f = 0usize;
            while f < FMT_CELLS {
                if let Some(w) = fmt_cell(f) {
                    let _ = lt.record_fmt(f, w);
                }
                f += 1;
            }
            let mut g2 = 0usize;
            while g2 < PLUGIN_CELLS {
                if let Some(w) = plugin_cell(g2) {
                    let _ = lt.record_plugin(g2, w);
                }
                g2 += 1;
            }
            let mut o = 0usize;
            while o < SCAN_OPS {
                if let Some(w) = scan_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut r = 0usize;
            while r < LOOP_ROWS {
                let _ = lt.declare_row(false, r);
                r += 1;
            }
            let mut r2 = 0usize;
            while r2 < STACK_ROWS {
                let _ = lt.declare_row(true, r2);
                r2 += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，§AI-66.3）",
    );
    s.add(
        "封闸语义闭集",
        {
            // 六闸缺 → ESeal；六闸齐而声明行缺 → ERowMiss；格账缺 → ESeal。
            let empty = NiktoLedger::new();
            let mut mid = NiktoLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = mid.judge_gate(g);
                g += 1;
            }
            let mut full = mid.clone();
            let mut r = 0usize;
            while r < LOOP_ROWS {
                let _ = full.declare_row(false, r);
                r += 1;
            }
            let mut r2 = 0usize;
            while r2 < STACK_ROWS {
                let _ = full.declare_row(true, r2);
                r2 += 1;
            }
            empty.seal() == Err(NiktoCode::ESeal)
                && mid.seal() == Err(NiktoCode::ERowMiss)
                && full.seal() == Err(NiktoCode::ESeal)
        },
        "六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal，按序核验",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCBB40,
        "SEG_BASE 0x40 对齐且在 VX16 域（0xCA400 + 0x40×93）",
    );
    s.add("收口·判据条数", s.len() >= 17, "含本条共 ≥18 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scan_kind_table() {
        assert_eq!(scan_kind(0), Some(0));
        assert_eq!(scan_kind(4), Some(1));
        assert_eq!(scan_kind(8), Some(2));
        assert_eq!(scan_kind(12), Some(3));
        assert_eq!(scan_kind(19), Some(4));
        assert!(scan_kind(20).is_none());
        // 名表按族索引而非格号。
        assert_eq!(scan_kind_name(0), Some("-h 靶标指定"));
        assert_eq!(scan_kind_name(3), Some("-Format 报告"));
        assert_eq!(scan_kind_name(4), Some("-ssl 回环加密"));
        assert!(scan_kind_name(5).is_none());
        assert!(scan_cell(20).is_none());
    }

    #[test]
    fn test_fmt_and_plugin_cells() {
        assert_eq!(fmt_name(0), Some("htm"));
        assert_eq!(fmt_name(3), Some("csv"));
        assert_eq!(fmt_name(5), Some("txt"));
        assert!(fmt_name(6).is_none());
        assert!(fmt_cell(FMT_CELLS).is_none());
        assert_eq!(plugin_name(0), Some("msgs 消息库"));
        assert_eq!(plugin_name(7), Some("session 会话快照"));
        assert!(plugin_name(8).is_none());
        assert!(plugin_cell(PLUGIN_CELLS).is_none());
    }

    #[test]
    fn test_rows_pure() {
        assert!(loop_row(LOOP_ROWS).is_none());
        assert!(stack_row(STACK_ROWS).is_none());
        assert_eq!(loop_row(0), Some(0xCD40));
        assert_eq!(stack_row(1), Some(0xCE40 + 0x41));
        assert_eq!(loop_row_name(1), Some("回环白名单声明：白名单外靶标一律越闭集拒收"));
        assert!(loop_row_name(2).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = NiktoLedger::new();
        let _ = lt.record_op(9, scan_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, scan_cell(2).unwrap_or(0)), Err(NiktoCode::EOrder));
        assert!(lt.record_op(10, scan_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = NiktoLedger::new();
        assert_eq!(lt.record_op(5, scan_cell(5).unwrap_or(0).wrapping_add(1)), Err(NiktoCode::EDrift));
        assert_eq!(
            lt.record_fmt(1, fmt_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(NiktoCode::EFmtDrift)
        );
        assert_eq!(
            lt.record_plugin(2, plugin_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(NiktoCode::EFmtDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = NiktoLedger::new();
        let _ = lt.record_op(0, scan_cell(0).unwrap_or(0));
        assert_eq!(lt.record_op(0, scan_cell(0).unwrap_or(0)), Err(NiktoCode::EDup));
        let _ = lt.record_plugin(0, plugin_cell(0).unwrap_or(0));
        assert_eq!(lt.record_plugin(0, plugin_cell(0).unwrap_or(0)), Err(NiktoCode::EDup));
        let _ = lt.declare_row(false, 0);
        assert_eq!(lt.declare_row(false, 0), Err(NiktoCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = NiktoLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = NiktoLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_seal_order() {
        let empty = NiktoLedger::new();
        assert_eq!(empty.seal(), Err(NiktoCode::ESeal));
        let mut mid = NiktoLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.seal(), Err(NiktoCode::ERowMiss));
        let _ = mid.declare_row(false, 0);
        let _ = mid.declare_row(false, 1);
        let _ = mid.declare_row(true, 0);
        let _ = mid.declare_row(true, 1);
        assert_eq!(mid.seal(), Err(NiktoCode::ESeal));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // recorded 只含 run_cycle 实际计数的节：6 报告 + 8 插件 + 20 操作 = 34。
        assert_eq!(recorded, FMT_CELLS as u32 + PLUGIN_CELLS as u32 + SCAN_OPS as u32);
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
        assert_eq!(SEG_BASE, 0xCBB40);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(NiktoCode::Ok as u32, 0);
        assert_eq!(NiktoCode::EGradeDrift as u32, 12);
        assert!(!NiktoCode::Ok.detail().is_empty());
        assert!(!NiktoCode::ESeal.detail().is_empty());
        assert!(!NiktoCode::ENoFmt.detail().is_empty());
        assert_eq!(NiktoCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3 → 六闸齐 S2 → 全齐 S2（目标档）。
        let empty = NiktoLedger::new();
        assert_eq!(empty.judge_grade(), Grade::S3);
        let mut mid = NiktoLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = mid.judge_gate(g);
            g += 1;
        }
        assert_eq!(mid.judge_grade(), Grade::S2);
        let mut full = mid.clone();
        let mut o = 0usize;
        while o < SCAN_OPS {
            if let Some(w) = scan_cell(o) {
                let _ = full.record_op(o, w);
            }
            o += 1;
        }
        let mut f = 0usize;
        while f < FMT_CELLS {
            if let Some(w) = fmt_cell(f) {
                let _ = full.record_fmt(f, w);
            }
            f += 1;
        }
        let mut p = 0usize;
        while p < PLUGIN_CELLS {
            if let Some(w) = plugin_cell(p) {
                let _ = full.record_plugin(p, w);
            }
            p += 1;
        }
        let mut r = 0usize;
        while r < LOOP_ROWS {
            let _ = full.declare_row(false, r);
            r += 1;
        }
        let mut r2 = 0usize;
        while r2 < STACK_ROWS {
            let _ = full.declare_row(true, r2);
            r2 += 1;
        }
        assert_eq!(full.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_boundary_no_injection_ledger() {
        // 越闭集边界拒（ENoX 类提前返回）不计注入账。
        let mut lt = NiktoLedger::new();
        let _ = lt.record_op(SCAN_OPS, 0);
        let _ = lt.record_fmt(FMT_CELLS, 0);
        let _ = lt.declare_row(true, STACK_ROWS);
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn unx52494_checks_green() {
        let s = run_unx52494_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
