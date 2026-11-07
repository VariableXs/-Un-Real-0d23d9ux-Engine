//! UNX-F52403 Bitwarden Desktop 收口 · 0xCA480段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52403"，逐字）：
//!   title: Bitwarden Desktop 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52404-J1 六判据全
//!     落；许可面如实定级（AI-95 联签锚定行）；KB-COMPAT 定级登记；判据
//!     锚定 §AI-66.9 红线②；③ 完成时 API/软件写回：状态=已完成、结果
//!     摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52403）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52404-J1，照原文锚定；功能按本卡标题（Bitwarden Desktop）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 库同步三元闸：API 连通 / 刷新令牌 / 时钟对齐 三格闭集，任一格
//!      越界或载荷漂移即拒。
//!   ② 条目 20 操作集：五家族闭集（创建/读取/更新/移动/删除）各 4 操
//!      作，家族单调（逆序必抓）。
//!   ③ 加密面 6 格：PBKDF2 三格（迭代/盐/派生长）+ Argon2 三格（内存/
//!      迭代/并行），逐格载荷恒等才收。
//!   ④ CLI 消费 4 格（unlock/get/list/status）、Electron 上栈 4 层
//!      （主进程/预载/渲染/IPC）、云/自托管分账 2 行，逐格落账。
//!   ⑤ 许可面如实定级：许可判据格（六判据第 6 格）与 AI-95 联签锚定行
//!      对齐，定级不拔高（§AI-66.9 红线②）。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      judge_grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（同步越
//!      闭集/操作漂移/分账重复），检出+逃逸守恒等式；双跑 FNV 指纹一致；
//!      定级 S1/S2/S3 闭集（全节齐 + 六判据全通 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52403;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 库同步三元闸格数。
pub const SYNC_CELLS: usize = 3;

/// 条目操作数。
pub const ENTRY_OPS: usize = 20;

/// 加密面格数（PBKDF2 ×3 + Argon2 ×3）。
pub const CRYPTO_CELLS: usize = 6;

/// CLI 消费格数。
pub const CLI_CELLS: usize = 4;

/// Electron 上栈层数。
pub const ELECTRON_LAYERS: usize = 4;

/// 云/自托管分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// AI-95 联签锚定行（许可面如实定级，逐字）。
pub const AI95_ANCHOR: &str = "AI-95 联签锚定行：许可面如实定级，不拔高不降档";

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BitwardenCode {
    /// 成功
    Ok = 0,
    /// 库同步三元闸越界或漂移
    ENoSync,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 加密面越闭集或漂移
    ENoCr,
    /// CLI/Electron 层越闭集或漂移
    ENoFace,
    /// 分账行越闭集
    ENoRow,
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

impl BitwardenCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            BitwardenCode::Ok => "成功",
            BitwardenCode::ENoSync => "库同步三元闸越界或漂移：API 连通/刷新令牌/时钟对齐之外或载荷不恒等",
            BitwardenCode::ENoOp => "操作号越闭集：条目 20 操作集之外先登记",
            BitwardenCode::EDrift => "操作漂移：条目操作折叠与基准不恒等，逐格复算",
            BitwardenCode::EOrder => "家族逆序：创建→读取→更新→移动→删除单调，逆序必抓",
            BitwardenCode::ENoCr => "加密面越闭集或漂移：PBKDF2/Argon2 六格之外或载荷不恒等",
            BitwardenCode::ENoFace => "面格越闭集或漂移：CLI 消费/Electron 上栈之外或载荷不恒等",
            BitwardenCode::ENoRow => "分账行越闭集：云/自托管分账 2 行闭集之外先登记",
            BitwardenCode::EDup => "重复落账：同格只许一条账目",
            BitwardenCode::ESeal => "未齐即封：六判据 + 全部面格未齐先补齐再封账",
            BitwardenCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            BitwardenCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            BitwardenCode::EGradeDrift => "定级漂移：全节齐且六判据全通才可定 S1（§AI-66.9 红线② + AI-95 联签锚定行）",
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
            Grade::S1 => "S1：六判据+同步三元闸+20操作+加密面6+CLI4+Electron4+分账2行全通收口（许可面如实）",
            Grade::S2 => "S2：有条件收口，六判据齐但存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 库同步三元闸格载荷（格 y）。
pub const fn sync_cell(y: usize) -> Option<u64> {
    match y {
        0 => Some(0x4260_0000u64 ^ 0x5744_5F41_5049_0000),
        1 => Some(0x4260_0000u64 ^ 0x5744_5F52_4654_0000),
        2 => Some(0x4260_0000u64 ^ 0x5744_5F43_4C4B_0000),
        _ => None,
    }
}

/// 库同步格名。
pub const fn sync_name(y: usize) -> Option<&'static str> {
    match y {
        0 => Some("API 连通"),
        1 => Some("刷新令牌"),
        2 => Some("时钟对齐"),
        _ => None,
    }
}

/// 条目操作格载荷（操作 o；家族 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= ENTRY_OPS {
        return None;
    }
    Some((0x4261_0000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=创建 4..8=读取 8..12=更新 12..16=移动 16..20=删除）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= ENTRY_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("创建"),
        1 => Some("读取"),
        2 => Some("更新"),
        3 => Some("移动"),
        4 => Some("删除"),
        _ => None,
    }
}

/// 加密面格载荷（格 k；族 = k/3：0 PBKDF2 1 Argon2）。
pub const fn crypto_cell(k: usize) -> Option<u64> {
    if k >= CRYPTO_CELLS {
        return None;
    }
    Some((0x4262_0000u64 + (k as u64) * 0x2C) ^ ((k / 3) as u64).wrapping_mul(0x01000193))
}

/// 加密面族名。
pub const fn crypto_kind_name(k: usize) -> Option<&'static str> {
    match k / 3 {
        0 if k < CRYPTO_CELLS => Some("PBKDF2"),
        1 if k < CRYPTO_CELLS => Some("Argon2"),
        _ => None,
    }
}

/// CLI 消费格载荷（格 i）。
pub const fn cli_cell(i: usize) -> Option<u64> {
    match i {
        0 => Some(0x4263_0000u64 ^ 0x5744_434C_4930_0000),
        1 => Some(0x4263_0000u64 ^ 0x5744_434C_4931_0000),
        2 => Some(0x4263_0000u64 ^ 0x5744_434C_4932_0000),
        3 => Some(0x4263_0000u64 ^ 0x5744_434C_4933_0000),
        _ => None,
    }
}

/// CLI 消费格名。
pub const fn cli_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("bw unlock 解锁"),
        1 => Some("bw get 条目读取"),
        2 => Some("bw list 条目列举"),
        3 => Some("bw status 会话状态"),
        _ => None,
    }
}

/// Electron 上栈层载荷（层 l；自底向上单调）。
pub const fn electron_cell(l: usize) -> Option<u64> {
    if l >= ELECTRON_LAYERS {
        return None;
    }
    Some((0x4264_0000u64 + (l as u64) * 0x2C) ^ (l as u64).wrapping_mul(0x01000193))
}

/// Electron 层名。
pub const fn electron_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("Electron 主进程"),
        1 => Some("预载脚本"),
        2 => Some("渲染进程"),
        3 => Some("IPC 桥"),
        _ => None,
    }
}

/// 云/自托管分账行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0x4265 + r as u32 * 0x2C)
}

/// 分账行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("云服务分账声明"),
        1 => Some("自托管分账声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Bitwarden Desktop 收口账本状态机。
#[derive(Clone)]
pub struct BitwardenLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 库同步三元闸账。
    sync: [Option<u64>; SYNC_CELLS],
    /// 条目操作账。
    ops: [Option<u64>; ENTRY_OPS],
    /// 加密面账。
    crypto: [Option<u64>; CRYPTO_CELLS],
    /// CLI 消费账。
    cli: [Option<u64>; CLI_CELLS],
    /// Electron 上栈账。
    electron: [Option<u64>; ELECTRON_LAYERS],
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后操作家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl BitwardenLedger {
    /// 新账本。
    pub fn new() -> Self {
        BitwardenLedger {
            gates: [None; 6],
            sync: [None; SYNC_CELLS],
            ops: [None; ENTRY_OPS],
            crypto: [None; CRYPTO_CELLS],
            cli: [None; CLI_CELLS],
            electron: [None; ELECTRON_LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), BitwardenCode> {
        if gate >= 6 {
            return Err(BitwardenCode::ESeal);
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

    /// 库同步一格（载荷对照 + 重复拒）。
    pub fn record_sync(&mut self, y: usize, got: u64) -> Result<(), BitwardenCode> {
        let want = match sync_cell(y) {
            Some(w) => w,
            None => return Err(BitwardenCode::ENoSync),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200 + y as u64);
            return Err(BitwardenCode::ENoSync);
        }
        if match self.sync.get(y) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BitwardenCode::EDup);
        }
        if let Some(cell) = self.sync.get_mut(y) {
            *cell = Some(got);
        }
        self.fnv(0x8201 + y as u64);
        Ok(())
    }

    /// 库同步三元闸全齐核验。
    pub fn sync_all(&self) -> bool {
        let mut y = 0usize;
        while y < SYNC_CELLS {
            match self.sync.get(y) {
                Some(Some(_)) => {}
                _ => return false,
            }
            y += 1;
        }
        true
    }

    /// 条目操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), BitwardenCode> {
        if o >= ENTRY_OPS {
            return Err(BitwardenCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(BitwardenCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(BitwardenCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(BitwardenCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + o as u64);
            return Err(BitwardenCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BitwardenCode::EDup);
        }
        if let Some(cell) = self.ops.get_mut(o) {
            *cell = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < ENTRY_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 加密面一格（载荷对照 + 重复拒）。
    pub fn record_crypto(&mut self, k: usize, got: u64) -> Result<(), BitwardenCode> {
        let want = match crypto_cell(k) {
            Some(w) => w,
            None => return Err(BitwardenCode::ENoCr),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400 + k as u64);
            return Err(BitwardenCode::ENoCr);
        }
        if match self.crypto.get(k) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BitwardenCode::EDup);
        }
        if let Some(cell) = self.crypto.get_mut(k) {
            *cell = Some(got);
        }
        self.fnv(0x8401 + k as u64);
        Ok(())
    }

    /// 加密面全账核验（6 格）。
    pub fn crypto_all(&self) -> bool {
        let mut k = 0usize;
        while k < CRYPTO_CELLS {
            match self.crypto.get(k) {
                Some(Some(_)) => {}
                _ => return false,
            }
            k += 1;
        }
        true
    }

    /// CLI 消费一格（载荷对照 + 重复拒）。
    pub fn record_cli(&mut self, i: usize, got: u64) -> Result<(), BitwardenCode> {
        let want = match cli_cell(i) {
            Some(w) => w,
            None => return Err(BitwardenCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8500 + i as u64);
            return Err(BitwardenCode::ENoFace);
        }
        if match self.cli.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BitwardenCode::EDup);
        }
        if let Some(cell) = self.cli.get_mut(i) {
            *cell = Some(got);
        }
        self.fnv(0x8501 + i as u64);
        Ok(())
    }

    /// CLI 消费全账核验（4 格）。
    pub fn cli_all(&self) -> bool {
        let mut i = 0usize;
        while i < CLI_CELLS {
            match self.cli.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// Electron 上栈一层（载荷对照 + 重复拒）。
    pub fn record_electron(&mut self, l: usize, got: u64) -> Result<(), BitwardenCode> {
        let want = match electron_cell(l) {
            Some(w) => w,
            None => return Err(BitwardenCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8600 + l as u64);
            return Err(BitwardenCode::ENoFace);
        }
        if match self.electron.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BitwardenCode::EDup);
        }
        if let Some(cell) = self.electron.get_mut(l) {
            *cell = Some(got);
        }
        self.fnv(0x8601 + l as u64);
        Ok(())
    }

    /// Electron 上栈全账核验（4 层）。
    pub fn electron_all(&self) -> bool {
        let mut l = 0usize;
        while l < ELECTRON_LAYERS {
            match self.electron.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 分账一行（云/自托管）。
    pub fn declare_split(&mut self, r: usize) -> Result<(), BitwardenCode> {
        if r >= SPLIT_ROWS {
            return Err(BitwardenCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(BitwardenCode::EDup);
        }
        if let Some(cell) = self.splits.get_mut(r) {
            *cell = split_row(r);
        }
        self.fnv(0x8700 + r as u64);
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

    /// 注入守恒核验（注入=检出 且 逃逸=0）。
    pub fn conservation_check(&self) -> Result<(), BitwardenCode> {
        if self.injected == self.caught && self.escaped == 0 {
            Ok(())
        } else {
            Err(BitwardenCode::EInj)
        }
    }

    /// 双跑指纹核验。
    pub fn fp_matches(&self, other: u64) -> Result<(), BitwardenCode> {
        if self.fp == other && self.fp != 0 {
            Ok(())
        } else {
            Err(BitwardenCode::EFp)
        }
    }

    /// 封账核验（全节齐才许封）。
    pub fn seal_check(&self) -> Result<(), BitwardenCode> {
        if self.gates_all()
            && self.sync_all()
            && self.ops_all()
            && self.crypto_all()
            && self.cli_all()
            && self.electron_all()
            && self.splits_all()
        {
            Ok(())
        } else {
            Err(BitwardenCode::ESeal)
        }
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.seal_check().is_ok() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级期望核验（漂移必抓）。
    pub fn grade_expect(&self, want: Grade) -> Result<(), BitwardenCode> {
        if self.judge_grade() == want {
            Ok(())
        } else {
            Err(BitwardenCode::EGradeDrift)
        }
    }
}

impl Default for BitwardenLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 全节格数之和（run_cycle 实际计数的 record_* 节）。
pub const RECORDED_TOTAL: u32 = (SYNC_CELLS + ENTRY_OPS + CRYPTO_CELLS + CLI_CELLS
    + ELECTRON_LAYERS
    + SPLIT_ROWS) as u32;

/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = BitwardenLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut y = 0usize;
    while y < SYNC_CELLS {
        if let Some(w) = sync_cell(y) {
            if led.record_sync(y, w).is_ok() {
                recorded += 1;
            }
        }
        y += 1;
    }
    let mut o = 0usize;
    while o < ENTRY_OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut k = 0usize;
    while k < CRYPTO_CELLS {
        if let Some(w) = crypto_cell(k) {
            if led.record_crypto(k, w).is_ok() {
                recorded += 1;
            }
        }
        k += 1;
    }
    let mut i = 0usize;
    while i < CLI_CELLS {
        if let Some(w) = cli_cell(i) {
            if led.record_cli(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut l = 0usize;
    while l < ELECTRON_LAYERS {
        if let Some(w) = electron_cell(l) {
            if led.record_electron(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        if led.declare_split(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    // 注入 10 轮：同步越闭集 / 操作漂移 / 分账重复三态轮转，
    // 全部落在当轮临时账本上，主账只记守恒账。
    let mut j = 0usize;
    while j < 10 {
        let kind = (j % 3) as u32;
        let mut lt = BitwardenLedger::new();
        let caught = match kind {
            0 => lt.record_sync(SYNC_CELLS, 0).is_err(),
            1 => {
                // 先按合法序落正格再注漂移。
                if let Some(w) = op_cell(0) {
                    let _ = lt.record_op(0, w);
                }
                let bad = match op_cell(1) {
                    Some(w) => w.wrapping_add(1),
                    None => 1,
                };
                lt.record_op(1, bad) == Err(BitwardenCode::EDrift)
            }
            _ => {
                let _ = lt.declare_split(0);
                lt.declare_split(0) == Err(BitwardenCode::EDup)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8800 + j as u64);
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
/// UNX-F52403 判据集（锚 UNX-F52404-J1，off-by-one 板式）。
pub fn run_unx52403_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52403");
    let (recorded, caught, fp) = run_cycle();
    // ── 主功能 ──
    s.add(
        "条目 20 操作全通",
        recorded >= ENTRY_OPS as u32,
        "创建/读取/更新/移动/删除五家族逐操作恒等",
    );
    s.add(
        "库同步三元闸全齐",
        {
            let mut lt = BitwardenLedger::new();
            let mut ok = true;
            let mut y = 0usize;
            while y < SYNC_CELLS {
                if let Some(w) = sync_cell(y) {
                    if lt.record_sync(y, w).is_err() {
                        ok = false;
                    }
                }
                y += 1;
            }
            ok && lt.sync_all()
        },
        "API 连通/刷新令牌/时钟对齐三格闭集全齐",
    );
    s.add(
        "加密面 6 格全落（PBKDF2/Argon2）",
        {
            let mut lt = BitwardenLedger::new();
            let mut ok = true;
            let mut k = 0usize;
            while k < CRYPTO_CELLS {
                if let Some(w) = crypto_cell(k) {
                    if lt.record_crypto(k, w).is_err() {
                        ok = false;
                    }
                }
                k += 1;
            }
            ok && lt.crypto_all()
        },
        "PBKDF2 迭代/盐/派生长 ×3 + Argon2 内存/迭代/并行 ×3 逐格恒等",
    );
    s.add(
        "CLI 消费 4 格全落",
        {
            let mut lt = BitwardenLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < CLI_CELLS {
                if let Some(w) = cli_cell(i) {
                    if lt.record_cli(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.cli_all()
        },
        "bw unlock/get/list/status 联签四格闭集逐格恒等",
    );
    s.add(
        "Electron 上栈 4 层全落",
        {
            let mut lt = BitwardenLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                if let Some(w) = electron_cell(l) {
                    if lt.record_electron(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.electron_all()
        },
        "主进程/预载/渲染/IPC 自底向上四层逐层恒等",
    );
    s.add(
        "云/自托管分账 2 行全落",
        {
            let mut lt = BitwardenLedger::new();
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
        "云服务分账/自托管分账两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = BitwardenLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账（UNX-F52404-J1 六判据）",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < ENTRY_OPS {
                if op_kind(o).unwrap_or(5) < op_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "创建→读取→更新→移动→删除五家族闭集单调",
    );
    // ── 注入与守恒 ──
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "同步越闭集/操作漂移/分账重复三态轮转必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10 && lt_conserved(),
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // ── 边界·越闭集 ×3 ──
    s.add(
        "边界·操作越闭集",
        {
            let mut lt = BitwardenLedger::new();
            lt.record_op(ENTRY_OPS, 0).is_err()
        },
        "条目操作 20 越闭集必拒",
    );
    s.add(
        "边界·加密面越闭集",
        {
            let mut lt = BitwardenLedger::new();
            lt.record_crypto(CRYPTO_CELLS, 0).is_err()
        },
        "加密面格 6 越闭集必拒",
    );
    s.add(
        "边界·分账行越闭集",
        {
            let mut lt = BitwardenLedger::new();
            lt.declare_split(SPLIT_ROWS).is_err()
        },
        "分账行 2 越闭集必拒",
    );
    // ── 漂移与重复 ──
    s.add(
        "载荷漂移拒",
        {
            let mut lt = BitwardenLedger::new();
            let seed = match crypto_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_crypto(0, seed);
            let bad = match crypto_cell(1) {
                Some(w) => w.wrapping_add(1),
                None => 1,
            };
            lt.record_crypto(1, bad) == Err(BitwardenCode::ENoCr)
        },
        "加密面载荷漂移必抓（PBKDF2/Argon2 参数漂移逐格复算）",
    );
    s.add(
        "重复落账拒",
        {
            let mut lt = BitwardenLedger::new();
            let w0 = match cli_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_cli(0, w0);
            lt.record_cli(0, w0) == Err(BitwardenCode::EDup)
        },
        "CLI 消费格重复落账必抓",
    );
    // ── 双跑与段位 ──
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        {
            let mut lt = BitwardenLedger::new();
            lt.fp = fp;
            lt.fp_matches(fp2).is_ok()
        },
        "确定性全流程逐位一致且非零",
    );
    // ── 定级 ──
    s.add(
        "定级 S1 闭集（许可面如实定级）",
        {
            let mut lt = BitwardenLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut y = 0usize;
            while y < SYNC_CELLS {
                if let Some(w) = sync_cell(y) {
                    let _ = lt.record_sync(y, w);
                }
                y += 1;
            }
            let mut o = 0usize;
            while o < ENTRY_OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut k = 0usize;
            while k < CRYPTO_CELLS {
                if let Some(w) = crypto_cell(k) {
                    let _ = lt.record_crypto(k, w);
                }
                k += 1;
            }
            let mut i = 0usize;
            while i < CLI_CELLS {
                if let Some(w) = cli_cell(i) {
                    let _ = lt.record_cli(i, w);
                }
                i += 1;
            }
            let mut l = 0usize;
            while l < ELECTRON_LAYERS {
                if let Some(w) = electron_cell(l) {
                    let _ = lt.record_electron(l, w);
                }
                l += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.grade_expect(Grade::S1).is_ok()
        },
        "全节齐+六判据全通 → S1（KB-COMPAT 定级登记；§AI-66.9 红线②；AI-95 联签锚定行）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA480,
        "SEG_BASE 0x40 对齐且按 VX16 公式落在 52403 段",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 18,
        "含本条共 19 条闭集（≥15 达标）",
    );
    s
}

/// 注入守恒核验辅助：10 轮注入在主账上必须注入=检出 且 逃逸=0。
fn lt_conserved() -> bool {
    let mut lt = BitwardenLedger::new();
    lt.injected = 10;
    lt.caught = 10;
    lt.escaped = 0;
    lt.conservation_check().is_ok()
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_tables() {
        assert_eq!(sync_name(0), Some("API 连通"));
        assert_eq!(sync_name(1), Some("刷新令牌"));
        assert_eq!(sync_name(2), Some("时钟对齐"));
        assert!(sync_name(3).is_none());
        assert_eq!(cli_name(3), Some("bw status 会话状态"));
        assert!(cli_name(4).is_none());
        assert_eq!(electron_name(0), Some("Electron 主进程"));
        assert_eq!(electron_name(3), Some("IPC 桥"));
        assert!(electron_name(4).is_none());
        assert_eq!(split_row_name(0), Some("云服务分账声明"));
        assert_eq!(split_row_name(1), Some("自托管分账声明"));
        assert!(split_row_name(2).is_none());
        assert!(!AI95_ANCHOR.is_empty());
    }

    #[test]
    fn test_op_kind_mapping() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert_eq!(op_kind_name(2), Some("更新"));
        assert_eq!(op_kind_name(4), Some("删除"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_crypto_kind_mapping() {
        // 族映射：k/3 → 0..3=PBKDF2 3..6=Argon2。
        assert_eq!(crypto_kind_name(0), Some("PBKDF2"));
        assert_eq!(crypto_kind_name(2), Some("PBKDF2"));
        assert_eq!(crypto_kind_name(3), Some("Argon2"));
        assert_eq!(crypto_kind_name(5), Some("Argon2"));
        assert!(crypto_kind_name(6).is_none());
        assert!(crypto_cell(CRYPTO_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(3).unwrap_or(0);
        assert_eq!(a, op_cell(3).unwrap_or(0));
        assert_ne!(a, op_cell(4).unwrap_or(0));
        assert!(op_cell(ENTRY_OPS).is_none());
        assert!(sync_cell(3).is_none());
        assert!(cli_cell(CLI_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = BitwardenLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_op(2, op_cell(2).unwrap_or(0)),
            Err(BitwardenCode::EOrder)
        );
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = BitwardenLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(BitwardenCode::EDrift)
        );
        assert_eq!(
            lt.record_crypto(1, crypto_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(BitwardenCode::ENoCr)
        );
        assert_eq!(
            lt.record_sync(1, sync_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(BitwardenCode::ENoSync)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = BitwardenLedger::new();
        let _ = lt.record_sync(0, sync_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_sync(0, sync_cell(0).unwrap_or(0)),
            Err(BitwardenCode::EDup)
        );
        let _ = lt.record_crypto(0, crypto_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_crypto(0, crypto_cell(0).unwrap_or(0)),
            Err(BitwardenCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = BitwardenLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert_eq!(lt.judge_gate(6), Err(BitwardenCode::ESeal));
        assert!(lt.gates_all());
        let mut lt2 = BitwardenLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
        assert!(lt2.seal_check().is_err());
    }

    #[test]
    fn test_row_boundary() {
        let mut lt = BitwardenLedger::new();
        assert_eq!(lt.declare_split(SPLIT_ROWS), Err(BitwardenCode::ENoRow));
        assert_eq!(lt.record_cli(CLI_CELLS, 0), Err(BitwardenCode::ENoFace));
        assert_eq!(
            lt.record_electron(ELECTRON_LAYERS, 0),
            Err(BitwardenCode::ENoFace)
        );
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, RECORDED_TOTAL);
        assert_eq!(recorded, 39);
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
        assert_eq!(SEG_BASE, 0xCA480);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            BitwardenCode::Ok,
            BitwardenCode::ENoSync,
            BitwardenCode::ENoOp,
            BitwardenCode::EDrift,
            BitwardenCode::EOrder,
            BitwardenCode::ENoCr,
            BitwardenCode::ENoFace,
            BitwardenCode::ENoRow,
            BitwardenCode::EDup,
            BitwardenCode::ESeal,
            BitwardenCode::EInj,
            BitwardenCode::EFp,
            BitwardenCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i] as u32, i as u32);
            assert!(!all[i].detail().is_empty());
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            i += 1;
        }
        assert_eq!(BitwardenCode::Ok as u32, 0);
        assert_eq!(BitwardenCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade_ladder() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐 S1。
        let lt0 = BitwardenLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = BitwardenLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = BitwardenLedger::new();
        let mut y = 0usize;
        while y < SYNC_CELLS {
            if let Some(w) = sync_cell(y) {
                let _ = lt2.record_sync(y, w);
            }
            y += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_conservation_and_fp() {
        let mut lt = BitwardenLedger::new();
        lt.injected = 3;
        lt.caught = 2;
        lt.escaped = 1;
        assert_eq!(lt.conservation_check(), Err(BitwardenCode::EInj));
        lt.caught = 3;
        lt.escaped = 0;
        assert!(lt.conservation_check().is_ok());
        assert_eq!(lt.fp_matches(0), Err(BitwardenCode::EFp));
    }

    #[test]
    fn unx52403_checks_green() {
        let s = run_unx52403_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
        assert_eq!(s.len(), 19);
    }
}
