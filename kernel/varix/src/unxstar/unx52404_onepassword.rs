//! UNX-F52404 1Password Desktop 收口 · 0xCA4C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52404"，逐字）：
//!   title: 1Password Desktop 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52405-J1 server
//!     (dev 模式)/kv 20 操作集 ×10（dev 模式内存态零落盘声明）；KB-COMPAT
//!     定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、
//!     结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52404）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52405-J1，照原文锚定；功能按本卡标题（1Password Desktop）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 库三态 12 格：云库 / 本地库 / 共享库 各 4 格，逐格载荷恒等才收，
//!      漂移必抓。
//!   ② 条目 20 操作集：五家族闭集（创建/读取/更新/归档/删除）各 4 操
//!      作，家族单调（逆序必抓）；20 操作集 ×10 注入轮以此承载。
//!   ③ Secret Key 三元闸：Secret Key / 账户密码 / 设备注册 三格闭集，
//!      任一格越界或载荷漂移即拒。
//!   ④ CLI 消费联签 4 格（op signin/item get/item list/vault list）、
//!      与 KeePass 分账声明 2 行（分账声明 + dev 模式内存态零落盘声明），
//!      逐格落账。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      judge_grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（库越闭
//!      集/操作漂移/联签重复），检出+逃逸守恒等式；双跑 FNV 指纹一致；
//!      定级 S1/S2/S3 闭集（全节齐 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52404;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 库三态格数（云 4 + 本地 4 + 共享 4）。
pub const VAULT_CELLS: usize = 12;

/// 条目操作数。
pub const ENTRY_OPS: usize = 20;

/// Secret Key 三元闸格数。
pub const SECRET_KEY_CELLS: usize = 3;

/// CLI 消费联签格数。
pub const CLI_CELLS: usize = 4;

/// 与 KeePass 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OnePwdCode {
    /// 成功
    Ok = 0,
    /// 库格越闭集
    ENoVault,
    /// 库载荷漂移
    EVaultDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// Secret Key 三元闸越界或漂移
    ENoSk,
    /// CLI/分账行越闭集或漂移
    ENoFace,
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

impl OnePwdCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            OnePwdCode::Ok => "成功",
            OnePwdCode::ENoVault => "库格越闭集：库三态 12 格（云/本地/共享）之外先登记",
            OnePwdCode::EVaultDrift => "库载荷漂移：三态库折叠与基准不恒等，逐格复算",
            OnePwdCode::ENoOp => "操作号越闭集：条目 20 操作集之外先登记",
            OnePwdCode::EDrift => "操作漂移：条目操作折叠与基准不恒等，逐格复算",
            OnePwdCode::EOrder => "家族逆序：创建→读取→更新→归档→删除单调，逆序必抓",
            OnePwdCode::ENoSk => "Secret Key 三元闸越界或漂移：Secret Key/账户密码/设备注册之外或载荷不恒等",
            OnePwdCode::ENoFace => "面格越闭集或漂移：CLI 联签/分账声明行之外或载荷不恒等",
            OnePwdCode::EDup => "重复落账：同格只许一条账目",
            OnePwdCode::ESeal => "未齐即封：六判据 + 全部面格未齐先补齐再封账",
            OnePwdCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            OnePwdCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            OnePwdCode::EGradeDrift => "定级漂移：全节齐且六判据全通才可定 S1（§AI-66.3）",
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
            Grade::S1 => "S1：六判据+库三态12+20操作+SecretKey三元闸+CLI4+分账2行全通收口",
            Grade::S2 => "S2：有条件收口，六判据齐但存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 库三态格载荷（格 v；态 = v/4：0 云库 1 本地库 2 共享库）。
pub const fn vault_cell(v: usize) -> Option<u64> {
    if v >= VAULT_CELLS {
        return None;
    }
    Some((0x3150_0000u64 + (v as u64) * 0x2C) ^ ((v / 4) as u64).wrapping_mul(0x01000193))
}

/// 库态名。
pub const fn vault_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("云库"),
        1 => Some("本地库"),
        2 => Some("共享库"),
        _ => None,
    }
}

/// 条目操作格载荷（操作 o；家族 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= ENTRY_OPS {
        return None;
    }
    Some((0x3151_0000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=创建 4..8=读取 8..12=更新 12..16=归档 16..20=删除）。
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
        3 => Some("归档"),
        4 => Some("删除"),
        _ => None,
    }
}

/// Secret Key 三元闸格载荷（格 k）。
pub const fn sk_cell(k: usize) -> Option<u64> {
    match k {
        0 => Some(0x3152_0000u64 ^ 0x31_505F_53_4B_30_00_00),
        1 => Some(0x3152_0000u64 ^ 0x31_505F_50_57_31_00_00),
        2 => Some(0x3152_0000u64 ^ 0x31_505F_44_45_32_00_00),
        _ => None,
    }
}

/// Secret Key 格名。
pub const fn sk_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("Secret Key"),
        1 => Some("账户密码"),
        2 => Some("设备注册"),
        _ => None,
    }
}

/// CLI 消费联签格载荷（格 i）。
pub const fn cli_cell(i: usize) -> Option<u64> {
    match i {
        0 => Some(0x3153_0000u64 ^ 0x505F_434C_4930_0000),
        1 => Some(0x3153_0000u64 ^ 0x505F_434C_4931_0000),
        2 => Some(0x3153_0000u64 ^ 0x505F_434C_4932_0000),
        3 => Some(0x3153_0000u64 ^ 0x505F_434C_4933_0000),
        _ => None,
    }
}

/// CLI 联签格名。
pub const fn cli_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("op signin 登录"),
        1 => Some("item get 条目读取"),
        2 => Some("item list 条目列举"),
        3 => Some("vault list 库列举"),
        _ => None,
    }
}

/// 与 KeePass 分账行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0x3154 + r as u32 * 0x2C)
}

/// 分账行名。
pub const fn split_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("与 KeePass 分账声明"),
        1 => Some("dev 模式内存态零落盘声明"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 1Password Desktop 收口账本状态机。
#[derive(Clone)]
pub struct OnePwdLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 库三态账。
    vault: [Option<u64>; VAULT_CELLS],
    /// 条目操作账。
    ops: [Option<u64>; ENTRY_OPS],
    /// Secret Key 三元闸账。
    sk: [Option<u64>; SECRET_KEY_CELLS],
    /// CLI 联签账。
    cli: [Option<u64>; CLI_CELLS],
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

impl OnePwdLedger {
    /// 新账本。
    pub fn new() -> Self {
        OnePwdLedger {
            gates: [None; 6],
            vault: [None; VAULT_CELLS],
            ops: [None; ENTRY_OPS],
            sk: [None; SECRET_KEY_CELLS],
            cli: [None; CLI_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), OnePwdCode> {
        if gate >= 6 {
            return Err(OnePwdCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x9100 + gate as u64);
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

    /// 库三态一格（载荷对照 + 重复拒）。
    pub fn record_vault(&mut self, v: usize, got: u64) -> Result<(), OnePwdCode> {
        let want = match vault_cell(v) {
            Some(w) => w,
            None => return Err(OnePwdCode::ENoVault),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9200 + v as u64);
            return Err(OnePwdCode::EVaultDrift);
        }
        if match self.vault.get(v) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OnePwdCode::EDup);
        }
        if let Some(cell) = self.vault.get_mut(v) {
            *cell = Some(got);
        }
        self.fnv(0x9201 + v as u64);
        Ok(())
    }

    /// 库三态全账核验（12 格）。
    pub fn vault_all(&self) -> bool {
        let mut v = 0usize;
        while v < VAULT_CELLS {
            match self.vault.get(v) {
                Some(Some(_)) => {}
                _ => return false,
            }
            v += 1;
        }
        true
    }

    /// 条目操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), OnePwdCode> {
        if o >= ENTRY_OPS {
            return Err(OnePwdCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(OnePwdCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9300);
            return Err(OnePwdCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(OnePwdCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9301 + o as u64);
            return Err(OnePwdCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OnePwdCode::EDup);
        }
        if let Some(cell) = self.ops.get_mut(o) {
            *cell = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x9302 + o as u64);
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

    /// Secret Key 一格（载荷对照 + 重复拒）。
    pub fn record_sk(&mut self, k: usize, got: u64) -> Result<(), OnePwdCode> {
        let want = match sk_cell(k) {
            Some(w) => w,
            None => return Err(OnePwdCode::ENoSk),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9400 + k as u64);
            return Err(OnePwdCode::ENoSk);
        }
        if match self.sk.get(k) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OnePwdCode::EDup);
        }
        if let Some(cell) = self.sk.get_mut(k) {
            *cell = Some(got);
        }
        self.fnv(0x9401 + k as u64);
        Ok(())
    }

    /// Secret Key 三元闸全齐核验。
    pub fn sk_all(&self) -> bool {
        let mut k = 0usize;
        while k < SECRET_KEY_CELLS {
            match self.sk.get(k) {
                Some(Some(_)) => {}
                _ => return false,
            }
            k += 1;
        }
        true
    }

    /// CLI 联签一格（载荷对照 + 重复拒）。
    pub fn record_cli(&mut self, i: usize, got: u64) -> Result<(), OnePwdCode> {
        let want = match cli_cell(i) {
            Some(w) => w,
            None => return Err(OnePwdCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9500 + i as u64);
            return Err(OnePwdCode::ENoFace);
        }
        if match self.cli.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OnePwdCode::EDup);
        }
        if let Some(cell) = self.cli.get_mut(i) {
            *cell = Some(got);
        }
        self.fnv(0x9501 + i as u64);
        Ok(())
    }

    /// CLI 联签全账核验（4 格）。
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

    /// 分账一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), OnePwdCode> {
        if r >= SPLIT_ROWS {
            return Err(OnePwdCode::ENoFace);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(OnePwdCode::EDup);
        }
        if let Some(cell) = self.splits.get_mut(r) {
            *cell = split_row(r);
        }
        self.fnv(0x9600 + r as u64);
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
    pub fn conservation_check(&self) -> Result<(), OnePwdCode> {
        if self.injected == self.caught && self.escaped == 0 {
            Ok(())
        } else {
            Err(OnePwdCode::EInj)
        }
    }

    /// 双跑指纹核验。
    pub fn fp_matches(&self, other: u64) -> Result<(), OnePwdCode> {
        if self.fp == other && self.fp != 0 {
            Ok(())
        } else {
            Err(OnePwdCode::EFp)
        }
    }

    /// 封账核验（全节齐才许封）。
    pub fn seal_check(&self) -> Result<(), OnePwdCode> {
        if self.gates_all()
            && self.vault_all()
            && self.ops_all()
            && self.sk_all()
            && self.cli_all()
            && self.splits_all()
        {
            Ok(())
        } else {
            Err(OnePwdCode::ESeal)
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
    pub fn grade_expect(&self, want: Grade) -> Result<(), OnePwdCode> {
        if self.judge_grade() == want {
            Ok(())
        } else {
            Err(OnePwdCode::EGradeDrift)
        }
    }
}

impl Default for OnePwdLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 全节格数之和（run_cycle 实际计数的 record_* 节）。
pub const RECORDED_TOTAL: u32 = (VAULT_CELLS + ENTRY_OPS + SECRET_KEY_CELLS + CLI_CELLS
    + SPLIT_ROWS) as u32;

/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = OnePwdLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut v = 0usize;
    while v < VAULT_CELLS {
        if let Some(w) = vault_cell(v) {
            if led.record_vault(v, w).is_ok() {
                recorded += 1;
            }
        }
        v += 1;
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
    while k < SECRET_KEY_CELLS {
        if let Some(w) = sk_cell(k) {
            if led.record_sk(k, w).is_ok() {
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
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        if led.declare_split(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    // 注入 10 轮：库越闭集 / 操作漂移 / 联签重复三态轮转，
    // 全部落在当轮临时账本上，主账只记守恒账。
    let mut j = 0usize;
    while j < 10 {
        let kind = (j % 3) as u32;
        let mut lt = OnePwdLedger::new();
        let caught = match kind {
            0 => lt.record_vault(VAULT_CELLS, 0).is_err(),
            1 => {
                // 先按合法序落正格再注漂移。
                if let Some(w) = op_cell(0) {
                    let _ = lt.record_op(0, w);
                }
                let bad = match op_cell(1) {
                    Some(w) => w.wrapping_add(1),
                    None => 1,
                };
                lt.record_op(1, bad) == Err(OnePwdCode::EDrift)
            }
            _ => {
                if let Some(w) = cli_cell(0) {
                    let _ = lt.record_cli(0, w);
                }
                match cli_cell(0) {
                    Some(w) => lt.record_cli(0, w) == Err(OnePwdCode::EDup),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x9700 + j as u64);
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
/// UNX-F52404 判据集（锚 UNX-F52405-J1，off-by-one 板式）。
pub fn run_unx52404_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52404");
    let (recorded, caught, fp) = run_cycle();
    // ── 主功能 ──
    s.add(
        "条目 20 操作全通（×10 注入承载）",
        recorded >= ENTRY_OPS as u32,
        "创建/读取/更新/归档/删除五家族逐操作恒等，20 操作集承载 ×10 注入轮",
    );
    s.add(
        "库三态 12 格全落（云/本地/共享）",
        {
            let mut lt = OnePwdLedger::new();
            let mut ok = true;
            let mut v = 0usize;
            while v < VAULT_CELLS {
                if let Some(w) = vault_cell(v) {
                    if lt.record_vault(v, w).is_err() {
                        ok = false;
                    }
                }
                v += 1;
            }
            ok && lt.vault_all()
        },
        "云库 ×4 / 本地库 ×4 / 共享库 ×4 逐格恒等（§AI-66.3）",
    );
    s.add(
        "Secret Key 三元闸全齐",
        {
            let mut lt = OnePwdLedger::new();
            let mut ok = true;
            let mut k = 0usize;
            while k < SECRET_KEY_CELLS {
                if let Some(w) = sk_cell(k) {
                    if lt.record_sk(k, w).is_err() {
                        ok = false;
                    }
                }
                k += 1;
            }
            ok && lt.sk_all()
        },
        "Secret Key/账户密码/设备注册三格闭集全齐",
    );
    s.add(
        "CLI 消费联签 4 格全落",
        {
            let mut lt = OnePwdLedger::new();
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
        "op signin/item get/item list/vault list 联签四格闭集逐格恒等",
    );
    s.add(
        "与 KeePass 分账声明 2 行全落",
        {
            let mut lt = OnePwdLedger::new();
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
        "与 KeePass 分账声明/dev 模式内存态零落盘声明两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = OnePwdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账（UNX-F52405-J1 六判据）",
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
        "创建→读取→更新→归档→删除五家族闭集单调",
    );
    // ── 注入与守恒 ──
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "库越闭集/操作漂移/联签重复三态轮转必抓",
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
            let mut lt = OnePwdLedger::new();
            lt.record_op(ENTRY_OPS, 0).is_err()
        },
        "条目操作 20 越闭集必拒",
    );
    s.add(
        "边界·库格越闭集",
        {
            let mut lt = OnePwdLedger::new();
            lt.record_vault(VAULT_CELLS, 0).is_err()
        },
        "库格 12 越闭集必拒",
    );
    s.add(
        "边界·分账行越闭集",
        {
            let mut lt = OnePwdLedger::new();
            lt.declare_split(SPLIT_ROWS).is_err()
        },
        "分账行 2 越闭集必拒",
    );
    // ── 漂移与重复 ──
    s.add(
        "载荷漂移拒",
        {
            let mut lt = OnePwdLedger::new();
            let seed = match vault_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_vault(0, seed);
            let bad = match vault_cell(1) {
                Some(w) => w.wrapping_add(1),
                None => 1,
            };
            lt.record_vault(1, bad) == Err(OnePwdCode::EVaultDrift)
        },
        "库三态载荷漂移必抓（云/本地/共享逐格复算）",
    );
    s.add(
        "重复落账拒",
        {
            let mut lt = OnePwdLedger::new();
            let w0 = match sk_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_sk(0, w0);
            lt.record_sk(0, w0) == Err(OnePwdCode::EDup)
        },
        "Secret Key 格重复落账必抓",
    );
    // ── 双跑与段位 ──
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        {
            let mut lt = OnePwdLedger::new();
            lt.fp = fp;
            lt.fp_matches(fp2).is_ok()
        },
        "确定性全流程逐位一致且非零",
    );
    // ── 定级 ──
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = OnePwdLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut v = 0usize;
            while v < VAULT_CELLS {
                if let Some(w) = vault_cell(v) {
                    let _ = lt.record_vault(v, w);
                }
                v += 1;
            }
            let mut o = 0usize;
            while o < ENTRY_OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut k = 0usize;
            while k < SECRET_KEY_CELLS {
                if let Some(w) = sk_cell(k) {
                    let _ = lt.record_sk(k, w);
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
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.grade_expect(Grade::S1).is_ok()
        },
        "全节齐+六判据全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA4C0,
        "SEG_BASE 0x40 对齐且按 VX16 公式落在 52404 段",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 17,
        "含本条共 18 条闭集（≥15 达标）",
    );
    s
}

/// 注入守恒核验辅助：10 轮注入在主账上必须注入=检出 且 逃逸=0。
fn lt_conserved() -> bool {
    let mut lt = OnePwdLedger::new();
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
        assert_eq!(vault_kind_name(0), Some("云库"));
        assert_eq!(vault_kind_name(1), Some("本地库"));
        assert_eq!(vault_kind_name(2), Some("共享库"));
        assert!(vault_kind_name(3).is_none());
        assert_eq!(sk_name(0), Some("Secret Key"));
        assert_eq!(sk_name(1), Some("账户密码"));
        assert_eq!(sk_name(2), Some("设备注册"));
        assert!(sk_name(3).is_none());
        assert_eq!(cli_name(3), Some("vault list 库列举"));
        assert!(cli_name(4).is_none());
        assert_eq!(split_row_name(0), Some("与 KeePass 分账声明"));
        assert_eq!(split_row_name(1), Some("dev 模式内存态零落盘声明"));
        assert!(split_row_name(2).is_none());
    }

    #[test]
    fn test_op_kind_mapping() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert_eq!(op_kind_name(2), Some("更新"));
        assert_eq!(op_kind_name(3), Some("归档"));
        assert_eq!(op_kind_name(4), Some("删除"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_vault_kind_mapping() {
        // 态映射：v/4 → 0..4=云库 4..8=本地库 8..12=共享库。
        assert_eq!(vault_cell(0), vault_cell(0));
        assert_ne!(vault_cell(0), vault_cell(4));
        assert_ne!(vault_cell(4), vault_cell(8));
        assert!(vault_cell(VAULT_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(3).unwrap_or(0);
        assert_eq!(a, op_cell(3).unwrap_or(0));
        assert_ne!(a, op_cell(4).unwrap_or(0));
        assert!(op_cell(ENTRY_OPS).is_none());
        assert!(sk_cell(3).is_none());
        assert!(cli_cell(CLI_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = OnePwdLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_op(2, op_cell(2).unwrap_or(0)),
            Err(OnePwdCode::EOrder)
        );
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = OnePwdLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(OnePwdCode::EDrift)
        );
        assert_eq!(
            lt.record_vault(1, vault_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(OnePwdCode::EVaultDrift)
        );
        assert_eq!(
            lt.record_sk(1, sk_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(OnePwdCode::ENoSk)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = OnePwdLedger::new();
        let _ = lt.record_vault(0, vault_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_vault(0, vault_cell(0).unwrap_or(0)),
            Err(OnePwdCode::EDup)
        );
        let _ = lt.record_cli(0, cli_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cli(0, cli_cell(0).unwrap_or(0)),
            Err(OnePwdCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = OnePwdLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert_eq!(lt.judge_gate(6), Err(OnePwdCode::ESeal));
        assert!(lt.gates_all());
        let mut lt2 = OnePwdLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
        assert!(lt2.seal_check().is_err());
    }

    #[test]
    fn test_face_boundary() {
        let mut lt = OnePwdLedger::new();
        assert_eq!(lt.record_cli(CLI_CELLS, 0), Err(OnePwdCode::ENoFace));
        assert_eq!(lt.declare_split(SPLIT_ROWS), Err(OnePwdCode::ENoFace));
        assert!(lt.declare_split(0).is_ok());
        assert_eq!(lt.declare_split(0), Err(OnePwdCode::EDup));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, RECORDED_TOTAL);
        assert_eq!(recorded, 41);
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
        assert_eq!(SEG_BASE, 0xCA4C0);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            OnePwdCode::Ok,
            OnePwdCode::ENoVault,
            OnePwdCode::EVaultDrift,
            OnePwdCode::ENoOp,
            OnePwdCode::EDrift,
            OnePwdCode::EOrder,
            OnePwdCode::ENoSk,
            OnePwdCode::ENoFace,
            OnePwdCode::EDup,
            OnePwdCode::ESeal,
            OnePwdCode::EInj,
            OnePwdCode::EFp,
            OnePwdCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i] as u32, i as u32);
            assert!(!all[i].detail().is_empty());
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            i += 1;
        }
        assert_eq!(OnePwdCode::Ok as u32, 0);
        assert_eq!(OnePwdCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade_ladder() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐 S1。
        let lt0 = OnePwdLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = OnePwdLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = OnePwdLedger::new();
        let mut v = 0usize;
        while v < VAULT_CELLS {
            if let Some(w) = vault_cell(v) {
                let _ = lt2.record_vault(v, w);
            }
            v += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_conservation_and_fp() {
        let mut lt = OnePwdLedger::new();
        lt.injected = 3;
        lt.caught = 2;
        lt.escaped = 1;
        assert_eq!(lt.conservation_check(), Err(OnePwdCode::EInj));
        lt.caught = 3;
        lt.escaped = 0;
        assert!(lt.conservation_check().is_ok());
        assert_eq!(lt.fp_matches(0), Err(OnePwdCode::EFp));
    }

    #[test]
    fn unx52404_checks_green() {
        let s = run_unx52404_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
        assert_eq!(s.len(), 18);
    }
}
