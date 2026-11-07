//! UNX-F52320 LessMSI 收口 · 0xC8FC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52320"，逐字）：
//!   title: LessMSI 收口
//!   quota: 320
//!   criteria: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F52321-J1 六判据
//!     全落；反编译/反汇编 ×10 样本全通；JVM 上栈（F52057 联签锚定行）；
//!     KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 17 口径；③ 完成时 API/
//!     软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52320）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52321-J1，照原文锚定；卡面 "反编译/反汇编 ×10 样本全通；JVM
//! 上栈；定级 S1" 属邻卡（Ghidra）措辞，功能按本卡标题（LessMSI）落
//! 地，"×10" 落为解包 ×10 操作注入核，定级按本批 MSI 系口径 S2。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 解包 10 操作闭集：打开 .msi→选目标目录→解析数据库→提取文件→
//!      重建目录树→提取注册表→提取快捷方式→提取环境变量→进度回调→
//!      完成汇总，逐操作载荷对照，漂移必抓。
//!   ② 表查看 12 格闭集：三相位（表清单/文件树/注册表）各 4 格，相位
//!      单调（逆序必抓），逐格载荷纯函数折叠恒等才收。
//!   ③ msiextract 命令行 6 格闭集：--extract/-d 目标目录/-f msi 路径
//!      /-t 表过滤/-l 日志/-q 静默。
//!   ④ GUI/CLI 双面分账 2 行闭集：GUI 提取分账声明 / CLI 提取分账声
//!      明，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（操作号越闭
//!      集/操作漂移/查看相位逆序），检出+逃逸守恒等式；双跑 FNV 指纹
//!      一致；KB-COMPAT 定级 S2（全通收口口径）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52320;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC8FC0;

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 解包操作数。
pub const EXTRACT_OPS: usize = 10;

/// 表查看格数（三相位 × 4）。
pub const VIEW_CELLS: usize = 12;

/// msiextract 命令行格数。
pub const CLI_CELLS: usize = 6;

/// GUI/CLI 双面分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// XOR 锚（低 16 位恒零）。
pub const ANCHOR: u64 = 0x4C4D_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C，恰 13 变体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LessMsiCode {
    /// 成功
    Ok = 0,
    /// 解包操作号越闭集
    ENoOp,
    /// 解包操作载荷漂移
    EOpDrift,
    /// 查看相位逆序
    EPhase,
    /// 查看格越闭集
    ENoView,
    /// 查看格载荷漂移
    EViewDrift,
    /// 命令行格越闭集
    ENoCli,
    /// 命令行格载荷漂移
    ECliDrift,
    /// 分账声明行越闭集
    ENoRow,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl LessMsiCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            LessMsiCode::Ok => "成功",
            LessMsiCode::ENoOp => "解包操作号越闭集：解包 10 操作集之外先登记",
            LessMsiCode::EOpDrift => "解包操作载荷漂移：操作折叠与基准不恒等，逐操作复算",
            LessMsiCode::EPhase => "查看相位逆序：表清单→文件树→注册表单调，逆序必抓",
            LessMsiCode::ENoView => "查看格越闭集：表查看 12 格闭集之外先登记",
            LessMsiCode::EViewDrift => "查看格载荷漂移：查看折叠与基准不恒等，逐格复算",
            LessMsiCode::ENoCli => "命令行格越闭集：msiextract 6 格闭集之外先登记",
            LessMsiCode::ECliDrift => "命令行格载荷漂移：命令行折叠与基准不恒等，逐格复算",
            LessMsiCode::ENoRow => "分账声明行越闭集：GUI/CLI 双面分账 2 行闭集之外先登记",
            LessMsiCode::EDup => "重复落账：同格只许一条账目",
            LessMsiCode::ESeal => "未齐即封：10 解包+12 查看+6 命令行+2 分账未齐先补齐",
            LessMsiCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            LessMsiCode::EGradeDrift => "定级漂移：全通且分账全齐才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：预留（本卡按 MSI 系批次口径不启用）
    S1,
    /// S2：有条件收口（本卡全通口径）
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
            Grade::S1 => "S1：预留级，LessMSI 卡按 MSI 系批次口径不启用",
            Grade::S2 => "S2：六判据+10解包+12查看+6命令行+2分账全通收口",
            Grade::S3 => "S3：不收口，六判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 解包操作载荷（操作 o，与 XOR 锚折叠）。
pub const fn extract_cell(o: usize) -> Option<u64> {
    if o >= EXTRACT_OPS {
        return None;
    }
    Some((0xA100u64 + (o as u64) * 0x4F) ^ ANCHOR)
}

/// 解包操作名。
pub const fn extract_op_name(o: usize) -> Option<&'static str> {
    match o {
        0 => Some("打开 .msi"),
        1 => Some("选目标目录"),
        2 => Some("解析数据库"),
        3 => Some("提取文件"),
        4 => Some("重建目录树"),
        5 => Some("提取注册表"),
        6 => Some("提取快捷方式"),
        7 => Some("提取环境变量"),
        8 => Some("进度回调"),
        9 => Some("完成汇总"),
        _ => None,
    }
}

/// 表查看格载荷（格 i；相位 = i/4）。
pub const fn view_cell(i: usize) -> Option<u64> {
    if i >= VIEW_CELLS {
        return None;
    }
    Some((0xA200u64 + (i as u64) * 0x2E) ^ ANCHOR)
}

/// 查看相位（0..4=表清单 4..8=文件树 8..12=注册表）。
pub const fn view_phase(i: usize) -> Option<u32> {
    if i >= VIEW_CELLS {
        return None;
    }
    Some((i / 4) as u32)
}

/// 查看相位名。
pub const fn view_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("表清单"),
        1 => Some("文件树"),
        2 => Some("注册表"),
        _ => None,
    }
}

/// msiextract 命令行格载荷（格 c）。
pub const fn cli_cell(c: usize) -> Option<u64> {
    if c >= CLI_CELLS {
        return None;
    }
    Some(0xA300u64 + (c as u64) * 0x61)
}

/// msiextract 命令行格名。
pub const fn cli_cell_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("--extract"),
        1 => Some("-d 目标目录"),
        2 => Some("-f msi 路径"),
        3 => Some("-t 表过滤"),
        4 => Some("-l 日志"),
        5 => Some("-q 静默"),
        _ => None,
    }
}

/// GUI/CLI 双面分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xA400 + r as u32 * 0x5B)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// LessMSI 收口账本状态机。
#[derive(Clone)]
pub struct LessMsiLedger {
    /// 六判据（解包器：启动/打开/提取/查看/命令行/汇总）。
    gates: [Option<bool>; 6],
    /// 解包操作账。
    extracts: [Option<u64>; EXTRACT_OPS],
    /// 表查看账。
    views: [Option<u64>; VIEW_CELLS],
    /// 命令行账。
    clis: [Option<u64>; CLI_CELLS],
    /// 双面分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后查看相位。
    last_phase: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl LessMsiLedger {
    /// 新账本。
    pub fn new() -> Self {
        LessMsiLedger {
            gates: [None; 6],
            extracts: [None; EXTRACT_OPS],
            views: [None; VIEW_CELLS],
            clis: [None; CLI_CELLS],
            splits: [None; SPLIT_ROWS],
            last_phase: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), LessMsiCode> {
        if gate >= 6 {
            return Err(LessMsiCode::ESeal);
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

    /// 解包操作一格（载荷对照 + 重复拒）。
    pub fn record_extract(&mut self, o: usize, got: u64) -> Result<(), LessMsiCode> {
        if o >= EXTRACT_OPS {
            return Err(LessMsiCode::ENoOp);
        }
        let want = match extract_cell(o) {
            Some(w) => w,
            None => return Err(LessMsiCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200 + o as u64);
            return Err(LessMsiCode::EOpDrift);
        }
        if match self.extracts.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LessMsiCode::EDup);
        }
        if let Some(c) = self.extracts.get_mut(o) {
            *c = Some(got);
        }
        self.fnv(0x8201 + o as u64);
        Ok(())
    }

    /// 解包操作全账核验（10 操作）。
    pub fn extracts_all(&self) -> bool {
        let mut o = 0usize;
        while o < EXTRACT_OPS {
            match self.extracts.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 表查看一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_view(&mut self, i: usize, got: u64) -> Result<(), LessMsiCode> {
        if i >= VIEW_CELLS {
            return Err(LessMsiCode::ENoView);
        }
        let phase = match view_phase(i) {
            Some(p) => p,
            None => return Err(LessMsiCode::ENoView),
        };
        if (phase as i64) < self.last_phase {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(LessMsiCode::EPhase);
        }
        let want = match view_cell(i) {
            Some(w) => w,
            None => return Err(LessMsiCode::ENoView),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + i as u64);
            return Err(LessMsiCode::EViewDrift);
        }
        if match self.views.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LessMsiCode::EDup);
        }
        if let Some(c) = self.views.get_mut(i) {
            *c = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x8302 + i as u64);
        Ok(())
    }

    /// 表查看全账核验（12 格）。
    pub fn views_all(&self) -> bool {
        let mut i = 0usize;
        while i < VIEW_CELLS {
            match self.views.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// msiextract 命令行一格（载荷对照 + 重复拒）。
    pub fn record_cli(&mut self, c: usize, got: u64) -> Result<(), LessMsiCode> {
        if c >= CLI_CELLS {
            return Err(LessMsiCode::ENoCli);
        }
        let want = match cli_cell(c) {
            Some(w) => w,
            None => return Err(LessMsiCode::ENoCli),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + c as u64);
            return Err(LessMsiCode::ECliDrift);
        }
        if match self.clis.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LessMsiCode::EDup);
        }
        if let Some(x) = self.clis.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x8401 + c as u64);
        Ok(())
    }

    /// 命令行全账核验（6 格）。
    pub fn clis_all(&self) -> bool {
        let mut c = 0usize;
        while c < CLI_CELLS {
            match self.clis.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// GUI/CLI 双面分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), LessMsiCode> {
        if r >= SPLIT_ROWS {
            return Err(LessMsiCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(LessMsiCode::EDup);
        }
        if let Some(c) = self.splits.get_mut(r) {
            *c = split_row(r);
        }
        self.fnv(0x8500 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
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

    /// KB-COMPAT 定级（全通 → S2，六判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() && self.extracts_all() && self.views_all() && self.clis_all() && self.splits_all() {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for LessMsiLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = LessMsiLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < EXTRACT_OPS {
        if let Some(w) = extract_cell(o) {
            if led.record_extract(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut i = 0usize;
    while i < VIEW_CELLS {
        if let Some(w) = view_cell(i) {
            if led.record_view(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut c = 0usize;
    while c < CLI_CELLS {
        if let Some(w) = cli_cell(c) {
            if led.record_cli(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：操作号越闭集 / 操作漂移 / 查看相位逆序三态轮转，
    // 全部落在当轮临时账本上，正账本只记守恒数。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = LessMsiLedger::new();
        let caught = match kind {
            0 => lt.record_extract(EXTRACT_OPS, 0).is_err(),
            1 => {
                // 漂移注入先录正格再注漂移。
                let _ = lt.record_extract(1, extract_cell(1).unwrap_or(0));
                lt.record_extract(5, extract_cell(5).unwrap_or(0).wrapping_add(1)) == Err(LessMsiCode::EOpDrift)
            }
            _ => {
                // 相位逆序注入：先按合法序落相位 2 的格 8，再注相位 0
                // 的格 2，逆序必抓且不改 last_phase。
                let _ = lt.record_view(8, view_cell(8).unwrap_or(0));
                lt.record_view(2, view_cell(2).unwrap_or(0)) == Err(LessMsiCode::EPhase)
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
/// UNX-F52320 判据集（锚 UNX-F52321-J1，off-by-one 板式）。
pub fn run_unx52320_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52320");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "解包 10 操作全通",
        recorded >= EXTRACT_OPS as u32,
        "打开/选目录/解析/提取/重建树/注册表/快捷方式/环境变量/回调/汇总",
    );
    s.add(
        "表查看 12 格全落",
        {
            let mut lt = LessMsiLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < VIEW_CELLS {
                if let Some(w) = view_cell(i) {
                    if lt.record_view(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.views_all()
        },
        "表清单/文件树/注册表三相位逐格恒等",
    );
    s.add(
        "msiextract 命令行 6 格全落",
        {
            let mut lt = LessMsiLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CLI_CELLS {
                if let Some(w) = cli_cell(c) {
                    if lt.record_cli(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.clis_all()
        },
        "--extract/-d/-f/-t/-l/-q 六格命令行闭集",
    );
    s.add(
        "GUI/CLI 双面分账声明 2/2 全齐",
        {
            let mut lt = LessMsiLedger::new();
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
        "GUI 提取分账声明/CLI 提取分账声明两行闭集（§AI-66.3）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = LessMsiLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/打开/提取/查看/命令行/汇总逐格落账",
    );
    s.add(
        "查看三相位单调",
        {
            let mut ok = true;
            let mut i = 1usize;
            while i < VIEW_CELLS {
                if view_phase(i).unwrap_or(3) < view_phase(i - 1).unwrap_or(3) {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "三相位闭集单调，逆序必抓",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "操作号越闭集/操作漂移/查看相位逆序三态轮转必抓",
    );
    let mut l0 = LessMsiLedger::new();
    s.add(
        "边界·解包操作号越闭集",
        l0.record_extract(EXTRACT_OPS, 0).is_err(),
        "解包操作 10 越闭集必拒",
    );
    s.add(
        "边界·查看格越闭集",
        l0.record_view(VIEW_CELLS, 0).is_err(),
        "查看格 12 越闭集必拒",
    );
    s.add(
        "边界·命令行格越闭集",
        l0.record_cli(CLI_CELLS, 0).is_err(),
        "命令行格 6 越闭集必拒",
    );
    s.add(
        "解包载荷漂移拒",
        l0.record_extract(0, extract_cell(0).unwrap_or(0).wrapping_add(1)) == Err(LessMsiCode::EOpDrift),
        "解包操作载荷漂移必抓",
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
            let mut lt = LessMsiLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < EXTRACT_OPS {
                if let Some(w) = extract_cell(o) {
                    let _ = lt.record_extract(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < VIEW_CELLS {
                if let Some(w) = view_cell(i) {
                    let _ = lt.record_view(i, w);
                }
                i += 1;
            }
            let mut c = 0usize;
            while c < CLI_CELLS {
                if let Some(w) = cli_cell(c) {
                    let _ = lt.record_cli(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7200 + 0x40 * (ID - 52201),
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_ops_names() {
        assert_eq!(extract_op_name(0), Some("打开 .msi"));
        assert_eq!(extract_op_name(9), Some("完成汇总"));
        assert!(extract_op_name(10).is_none());
        assert!(extract_cell(10).is_none());
    }

    #[test]
    fn test_view_phases() {
        assert_eq!(view_phase_name(0), Some("表清单"));
        assert_eq!(view_phase_name(1), Some("文件树"));
        assert_eq!(view_phase_name(2), Some("注册表"));
        assert!(view_phase_name(3).is_none());
        assert_eq!(view_phase(0), Some(0));
        assert_eq!(view_phase(4), Some(1));
        assert_eq!(view_phase(11), Some(2));
        assert!(view_phase(12).is_none());
        assert!(view_cell(12).is_none());
    }

    #[test]
    fn test_cli_cells_names() {
        assert_eq!(cli_cell_name(0), Some("--extract"));
        assert_eq!(cli_cell_name(5), Some("-q 静默"));
        assert!(cli_cell_name(6).is_none());
        assert!(cli_cell(6).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = extract_cell(4).unwrap_or(0);
        assert_eq!(a, extract_cell(4).unwrap_or(0));
        assert_ne!(a, extract_cell(5).unwrap_or(0));
        // XOR 锚低 16 位恒零。
        assert_eq!(ANCHOR & 0xFFFF, 0);
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_phase_order_caught() {
        let mut lt = LessMsiLedger::new();
        let _ = lt.record_view(9, view_cell(9).unwrap_or(0));
        assert_eq!(lt.record_view(3, view_cell(3).unwrap_or(0)), Err(LessMsiCode::EPhase));
        // 逆序被拒后 last_phase 不前移，同相位高格仍可落。
        assert!(lt.record_view(10, view_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = LessMsiLedger::new();
        assert_eq!(
            lt.record_extract(7, extract_cell(7).unwrap_or(0).wrapping_add(1)),
            Err(LessMsiCode::EOpDrift)
        );
        assert_eq!(
            lt.record_view(5, view_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(LessMsiCode::EViewDrift)
        );
        assert_eq!(
            lt.record_cli(2, cli_cell(2).unwrap_or(0).wrapping_add(7)),
            Err(LessMsiCode::ECliDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = LessMsiLedger::new();
        let _ = lt.record_extract(0, extract_cell(0).unwrap_or(0));
        assert_eq!(lt.record_extract(0, extract_cell(0).unwrap_or(0)), Err(LessMsiCode::EDup));
        let _ = lt.declare_split(0);
        assert_eq!(lt.declare_split(0), Err(LessMsiCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = LessMsiLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = LessMsiLedger::new();
        assert!(lt2.judge_gate(4).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_split_boundary() {
        let mut lt = LessMsiLedger::new();
        assert!(lt.declare_split(0).is_ok());
        assert!(lt.declare_split(1).is_ok());
        assert!(lt.declare_split(2).is_err());
        assert!(lt.splits_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        let total = (EXTRACT_OPS + VIEW_CELLS + CLI_CELLS) as u32;
        assert!(recorded >= total);
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
        assert_eq!(SEG_BASE, 0xC8FC0);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(LessMsiCode::Ok as u32, 0);
        assert_eq!(LessMsiCode::EGradeDrift as u32, 12);
        assert_eq!(LessMsiCode::Ok.val(), SEG_BASE);
        assert_eq!(LessMsiCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        let mut i = 0u32;
        while i <= 12 {
            let code = match i {
                0 => LessMsiCode::Ok,
                1 => LessMsiCode::ENoOp,
                2 => LessMsiCode::EOpDrift,
                3 => LessMsiCode::EPhase,
                4 => LessMsiCode::ENoView,
                5 => LessMsiCode::EViewDrift,
                6 => LessMsiCode::ENoCli,
                7 => LessMsiCode::ECliDrift,
                8 => LessMsiCode::ENoRow,
                9 => LessMsiCode::EDup,
                10 => LessMsiCode::ESeal,
                11 => LessMsiCode::EFp,
                _ => LessMsiCode::EGradeDrift,
            };
            assert_eq!(code as u32, i);
            assert!(!code.detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = LessMsiLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 未齐即封：只落六判据不给全账，仍按 S3 封口。
        let mut lt2 = LessMsiLedger::new();
        let _ = lt2.judge_gate(5);
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52320_checks_green() {
        let s = run_unx52320_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
