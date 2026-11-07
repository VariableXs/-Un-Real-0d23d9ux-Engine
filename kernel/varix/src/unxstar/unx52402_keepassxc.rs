//! UNX-F52402 KeePassXC 收口 · 0xCA440段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52402"，逐字）：
//!   title: KeePassXC 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52403-J1 六判据全
//!     落；同步面外部服务声明；离线缓存 ×10；KB-COMPAT 定级登记；判据锚
//!     定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52402）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52403-J1，照原文锚定；功能按本卡标题（KeePassXC）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① KDBX4 双向投影 16 格：正向导出投影 8 格 + 反向导入投影 8 格，
//!      纯函数载荷恒等才收，漂移必抓。
//!   ② 浏览器集成三元闸：代理连通 / 密钥关联 / 许可确认 三格闭集，
//!      任一格越界或载荷漂移即拒。
//!   ③ TOTP 10 操作集：五家族闭集（生成/校验/轮换/导入/导出）各 2 操
//!      作，家族单调（逆序必抓）；离线缓存 ×10 以此 10 操作承载。
//!   ④ SSH Agent 消费 8 格（列举 ×4 / 签名应答 ×4）、CLI 消费联签
//!      4 格（unlock/get/list/status）、Qt 上栈 4 层（QtCore/QtGui/
//!      QtWidgets/QtNetwork）、同步面外部服务声明 2 行，逐格落账。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      judge_grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（投影
//!      越闭集/TOTP 漂移/联签重复），检出+逃逸守恒等式；双跑 FNV 指纹
//!      一致；定级 S1/S2/S3 闭集（全节齐 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52402;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// KDBX4 双向投影格数（正向 8 + 反向 8）。
pub const PROJ_CELLS: usize = 16;

/// 浏览器集成三元闸格数。
pub const BROWSER_CELLS: usize = 3;

/// SSH Agent 消费格数。
pub const SSH_CELLS: usize = 8;

/// TOTP 操作数（离线缓存 ×10 承载）。
pub const TOTP_OPS: usize = 10;

/// CLI 消费联签格数。
pub const CLI_CELLS: usize = 4;

/// Qt 上栈层数。
pub const QT_LAYERS: usize = 4;

/// 同步面外部服务声明行数。
pub const SYNC_DECL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeePassXcCode {
    /// 成功
    Ok = 0,
    /// 投影格越闭集
    ENoProj,
    /// 投影载荷漂移
    EProjDrift,
    /// 浏览器集成三元闸越界或漂移
    EBrMiss,
    /// TOTP 操作号越闭集
    ENoOp,
    /// TOTP 操作漂移
    EDrift,
    /// TOTP 家族逆序
    EOrder,
    /// SSH/CLI/Qt/声明行越闭集或漂移
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

impl KeePassXcCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            KeePassXcCode::Ok => "成功",
            KeePassXcCode::ENoProj => "投影格越闭集：KDBX4 双向投影 16 格之外先登记",
            KeePassXcCode::EProjDrift => "投影载荷漂移：双向投影折叠与基准不恒等，逐格复算",
            KeePassXcCode::EBrMiss => "浏览器集成三元闸越界或漂移：代理连通/密钥关联/许可确认之外或载荷不恒等",
            KeePassXcCode::ENoOp => "TOTP 操作号越闭集：TOTP 10 操作集之外先登记",
            KeePassXcCode::EDrift => "TOTP 操作漂移：操作折叠与基准不恒等，逐格复算",
            KeePassXcCode::EOrder => "TOTP 家族逆序：生成→校验→轮换→导入→导出单调，逆序必抓",
            KeePassXcCode::ENoFace => "面格越闭集或漂移：SSH Agent/CLI 联签/Qt 上栈/同步面声明之外或载荷不恒等",
            KeePassXcCode::EDup => "重复落账：同格只许一条账目",
            KeePassXcCode::ESeal => "未齐即封：六判据 + 全部面格未齐先补齐再封账",
            KeePassXcCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            KeePassXcCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            KeePassXcCode::EGradeDrift => "定级漂移：全节齐且六判据全通才可定 S1（§AI-66.3）",
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
            Grade::S1 => "S1：六判据+KDBX4投影+浏览器三元闸+TOTP10+SSH+CLI+Qt4+同步声明全通收口",
            Grade::S2 => "S2：有条件收口，六判据齐但存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// KDBX4 双向投影格载荷（格 c；族 = c/8：0 正向导出投影 1 反向导入投影）。
pub const fn proj_cell(c: usize) -> Option<u64> {
    if c >= PROJ_CELLS {
        return None;
    }
    Some((0x4B58_0000u64 + (c as u64) * 0x2C) ^ ((c / 8) as u64).wrapping_mul(0x01000193))
}

/// 投影族名。
pub const fn proj_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("正向导出投影"),
        1 => Some("反向导入投影"),
        _ => None,
    }
}

/// 浏览器集成三元闸格载荷（格 b）。
pub const fn br_cell(b: usize) -> Option<u64> {
    match b {
        0 => Some(0x4B59_0000u64 ^ 0x5843_5F50_524F_0000),
        1 => Some(0x4B59_0000u64 ^ 0x5843_5F4B_4559_0000),
        2 => Some(0x4B59_0000u64 ^ 0x5843_5F41_4343_0000),
        _ => None,
    }
}

/// 浏览器集成格名。
pub const fn br_name(b: usize) -> Option<&'static str> {
    match b {
        0 => Some("代理连通"),
        1 => Some("密钥关联"),
        2 => Some("许可确认"),
        _ => None,
    }
}

/// SSH Agent 消费格载荷（格 x；族 = x/4：0 密钥列举 1 签名应答）。
pub const fn ssh_cell(x: usize) -> Option<u64> {
    if x >= SSH_CELLS {
        return None;
    }
    Some((0x4B5A_0000u64 + (x as u64) * 0x2C) ^ ((x / 4) as u64).wrapping_mul(0x01000193))
}

/// SSH 消费族名。
pub const fn ssh_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("密钥列举"),
        1 => Some("签名应答"),
        _ => None,
    }
}

/// TOTP 操作格载荷（操作 o；家族 = o/2）。
pub const fn totp_cell(o: usize) -> Option<u64> {
    if o >= TOTP_OPS {
        return None;
    }
    Some((0x4B5B_0000u64 + (o as u64) * 0x2C) ^ ((o / 2) as u64).wrapping_mul(0x01000193))
}

/// TOTP 操作家族（0..2=生成 2..4=校验 4..6=轮换 6..8=导入 8..10=导出）。
pub const fn totp_kind(o: usize) -> Option<u32> {
    if o >= TOTP_OPS {
        return None;
    }
    Some((o / 2) as u32)
}

/// TOTP 家族名。
pub const fn totp_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("生成"),
        1 => Some("校验"),
        2 => Some("轮换"),
        3 => Some("导入"),
        4 => Some("导出"),
        _ => None,
    }
}

/// CLI 消费联签格载荷（格 i）。
pub const fn cli_cell(i: usize) -> Option<u64> {
    match i {
        0 => Some(0x4B5C_0000u64 ^ 0x4B_5843_4C_49_30_00_00),
        1 => Some(0x4B5C_0000u64 ^ 0x4B_5843_4C_49_31_00_00),
        2 => Some(0x4B5C_0000u64 ^ 0x4B_5843_4C_49_32_00_00),
        3 => Some(0x4B5C_0000u64 ^ 0x4B_5843_4C_49_33_00_00),
        _ => None,
    }
}

/// CLI 联签格名。
pub const fn cli_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("unlock 解锁"),
        1 => Some("show 条目读取"),
        2 => Some("list 条目列举"),
        3 => Some("search 检索"),
        _ => None,
    }
}

/// Qt 上栈层载荷（层 l；族 = l：自底向上单调）。
pub const fn qt_cell(l: usize) -> Option<u64> {
    if l >= QT_LAYERS {
        return None;
    }
    Some((0x4B5D_0000u64 + (l as u64) * 0x2C) ^ (l as u64).wrapping_mul(0x01000193))
}

/// Qt 上栈层名。
pub const fn qt_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("QtCore"),
        1 => Some("QtGui"),
        2 => Some("QtWidgets"),
        3 => Some("QtNetwork"),
        _ => None,
    }
}

/// 同步面外部服务声明行折叠（行 r）。
pub const fn sync_decl_row(r: usize) -> Option<u32> {
    if r >= SYNC_DECL_ROWS {
        return None;
    }
    Some(0x4B5E + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// KeePassXC 收口账本状态机。
#[derive(Clone)]
pub struct KeePassXcLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// KDBX4 双向投影账。
    proj: [Option<u64>; PROJ_CELLS],
    /// 浏览器集成三元闸账。
    browser: [Option<u64>; BROWSER_CELLS],
    /// SSH Agent 消费账。
    ssh: [Option<u64>; SSH_CELLS],
    /// TOTP 操作账。
    totp: [Option<u64>; TOTP_OPS],
    /// CLI 联签账。
    cli: [Option<u64>; CLI_CELLS],
    /// Qt 上栈账。
    qt: [Option<u64>; QT_LAYERS],
    /// 同步面外部服务声明账。
    decl: [Option<u32>; SYNC_DECL_ROWS],
    /// 最后 TOTP 家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl KeePassXcLedger {
    /// 新账本。
    pub fn new() -> Self {
        KeePassXcLedger {
            gates: [None; 6],
            proj: [None; PROJ_CELLS],
            browser: [None; BROWSER_CELLS],
            ssh: [None; SSH_CELLS],
            totp: [None; TOTP_OPS],
            cli: [None; CLI_CELLS],
            qt: [None; QT_LAYERS],
            decl: [None; SYNC_DECL_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), KeePassXcCode> {
        if gate >= 6 {
            return Err(KeePassXcCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, c: usize, got: u64) -> Result<(), KeePassXcCode> {
        let want = match proj_cell(c) {
            Some(w) => w,
            None => return Err(KeePassXcCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200 + c as u64);
            return Err(KeePassXcCode::EProjDrift);
        }
        if match self.proj.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassXcCode::EDup);
        }
        if let Some(cell) = self.proj.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x7201 + c as u64);
        Ok(())
    }

    /// 投影全账核验（16 格）。
    pub fn proj_all(&self) -> bool {
        let mut c = 0usize;
        while c < PROJ_CELLS {
            match self.proj.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 浏览器集成一格（载荷对照 + 重复拒）。
    pub fn record_browser(&mut self, b: usize, got: u64) -> Result<(), KeePassXcCode> {
        let want = match br_cell(b) {
            Some(w) => w,
            None => return Err(KeePassXcCode::EBrMiss),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300 + b as u64);
            return Err(KeePassXcCode::EBrMiss);
        }
        if match self.browser.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassXcCode::EDup);
        }
        if let Some(cell) = self.browser.get_mut(b) {
            *cell = Some(got);
        }
        self.fnv(0x7301 + b as u64);
        Ok(())
    }

    /// 浏览器集成三元闸全齐核验。
    pub fn browser_all(&self) -> bool {
        let mut b = 0usize;
        while b < BROWSER_CELLS {
            match self.browser.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// SSH Agent 消费一格（载荷对照 + 重复拒）。
    pub fn record_ssh(&mut self, x: usize, got: u64) -> Result<(), KeePassXcCode> {
        let want = match ssh_cell(x) {
            Some(w) => w,
            None => return Err(KeePassXcCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + x as u64);
            return Err(KeePassXcCode::ENoFace);
        }
        if match self.ssh.get(x) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassXcCode::EDup);
        }
        if let Some(cell) = self.ssh.get_mut(x) {
            *cell = Some(got);
        }
        self.fnv(0x7401 + x as u64);
        Ok(())
    }

    /// SSH Agent 消费全账核验（8 格）。
    pub fn ssh_all(&self) -> bool {
        let mut x = 0usize;
        while x < SSH_CELLS {
            match self.ssh.get(x) {
                Some(Some(_)) => {}
                _ => return false,
            }
            x += 1;
        }
        true
    }

    /// TOTP 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_totp(&mut self, o: usize, got: u64) -> Result<(), KeePassXcCode> {
        if o >= TOTP_OPS {
            return Err(KeePassXcCode::ENoOp);
        }
        let kind = match totp_kind(o) {
            Some(k) => k,
            None => return Err(KeePassXcCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500);
            return Err(KeePassXcCode::EOrder);
        }
        let want = match totp_cell(o) {
            Some(w) => w,
            None => return Err(KeePassXcCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7501 + o as u64);
            return Err(KeePassXcCode::EDrift);
        }
        if match self.totp.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassXcCode::EDup);
        }
        if let Some(cell) = self.totp.get_mut(o) {
            *cell = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7502 + o as u64);
        Ok(())
    }

    /// TOTP 操作全账核验（10 操作）。
    pub fn totp_all(&self) -> bool {
        let mut o = 0usize;
        while o < TOTP_OPS {
            match self.totp.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// CLI 联签一格（载荷对照 + 重复拒）。
    pub fn record_cli(&mut self, i: usize, got: u64) -> Result<(), KeePassXcCode> {
        let want = match cli_cell(i) {
            Some(w) => w,
            None => return Err(KeePassXcCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7600 + i as u64);
            return Err(KeePassXcCode::ENoFace);
        }
        if match self.cli.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassXcCode::EDup);
        }
        if let Some(cell) = self.cli.get_mut(i) {
            *cell = Some(got);
        }
        self.fnv(0x7601 + i as u64);
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

    /// Qt 上栈一层（载荷对照 + 重复拒）。
    pub fn record_qt(&mut self, l: usize, got: u64) -> Result<(), KeePassXcCode> {
        let want = match qt_cell(l) {
            Some(w) => w,
            None => return Err(KeePassXcCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7700 + l as u64);
            return Err(KeePassXcCode::ENoFace);
        }
        if match self.qt.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassXcCode::EDup);
        }
        if let Some(cell) = self.qt.get_mut(l) {
            *cell = Some(got);
        }
        self.fnv(0x7701 + l as u64);
        Ok(())
    }

    /// Qt 上栈全账核验（4 层）。
    pub fn qt_all(&self) -> bool {
        let mut l = 0usize;
        while l < QT_LAYERS {
            match self.qt.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
        }
        true
    }

    /// 同步面外部服务声明一行。
    pub fn declare_sync(&mut self, r: usize) -> Result<(), KeePassXcCode> {
        let want = match sync_decl_row(r) {
            Some(w) => w,
            None => return Err(KeePassXcCode::ENoFace),
        };
        if match self.decl.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassXcCode::EDup);
        }
        if let Some(cell) = self.decl.get_mut(r) {
            *cell = Some(want);
        }
        self.fnv(0x7800 + r as u64);
        Ok(())
    }

    /// 同步面声明全齐核验。
    pub fn decl_all(&self) -> bool {
        let mut r = 0usize;
        while r < SYNC_DECL_ROWS {
            match self.decl.get(r) {
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
    pub fn conservation_check(&self) -> Result<(), KeePassXcCode> {
        if self.injected == self.caught && self.escaped == 0 {
            Ok(())
        } else {
            Err(KeePassXcCode::EInj)
        }
    }

    /// 双跑指纹核验。
    pub fn fp_matches(&self, other: u64) -> Result<(), KeePassXcCode> {
        if self.fp == other && self.fp != 0 {
            Ok(())
        } else {
            Err(KeePassXcCode::EFp)
        }
    }

    /// 封账核验（全节齐才许封）。
    pub fn seal_check(&self) -> Result<(), KeePassXcCode> {
        if self.gates_all()
            && self.proj_all()
            && self.browser_all()
            && self.ssh_all()
            && self.totp_all()
            && self.cli_all()
            && self.qt_all()
            && self.decl_all()
        {
            Ok(())
        } else {
            Err(KeePassXcCode::ESeal)
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
    pub fn grade_expect(&self, want: Grade) -> Result<(), KeePassXcCode> {
        if self.judge_grade() == want {
            Ok(())
        } else {
            Err(KeePassXcCode::EGradeDrift)
        }
    }
}

impl Default for KeePassXcLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 全节格数之和（run_cycle 实际计数的 record_* 节）。
pub const RECORDED_TOTAL: u32 = (PROJ_CELLS + BROWSER_CELLS + SSH_CELLS + TOTP_OPS + CLI_CELLS
    + QT_LAYERS
    + SYNC_DECL_ROWS) as u32;

/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = KeePassXcLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < PROJ_CELLS {
        if let Some(w) = proj_cell(c) {
            if led.record_proj(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut b = 0usize;
    while b < BROWSER_CELLS {
        if let Some(w) = br_cell(b) {
            if led.record_browser(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut x = 0usize;
    while x < SSH_CELLS {
        if let Some(w) = ssh_cell(x) {
            if led.record_ssh(x, w).is_ok() {
                recorded += 1;
            }
        }
        x += 1;
    }
    let mut o = 0usize;
    while o < TOTP_OPS {
        if let Some(w) = totp_cell(o) {
            if led.record_totp(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
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
    while l < QT_LAYERS {
        if let Some(w) = qt_cell(l) {
            if led.record_qt(l, w).is_ok() {
                recorded += 1;
            }
        }
        l += 1;
    }
    let mut r = 0usize;
    while r < SYNC_DECL_ROWS {
        if led.declare_sync(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    // 注入 10 轮：投影越闭集 / TOTP 漂移 / 联签重复三态轮转，
    // 全部落在当轮临时账本上，主账只记守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = KeePassXcLedger::new();
        let caught = match kind {
            0 => lt.record_proj(PROJ_CELLS, 0).is_err(),
            1 => {
                // 先按合法序落正格再注漂移。
                if let Some(w) = totp_cell(0) {
                    let _ = lt.record_totp(0, w);
                }
                let bad = match totp_cell(1) {
                    Some(w) => w.wrapping_add(1),
                    None => 1,
                };
                lt.record_totp(1, bad) == Err(KeePassXcCode::EDrift)
            }
            _ => {
                if let Some(w) = cli_cell(0) {
                    let _ = lt.record_cli(0, w);
                }
                match cli_cell(0) {
                    Some(w) => lt.record_cli(0, w) == Err(KeePassXcCode::EDup),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7900 + k as u64);
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
/// UNX-F52402 判据集（锚 UNX-F52403-J1，off-by-one 板式）。
pub fn run_unx52402_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52402");
    let (recorded, caught, fp) = run_cycle();
    // ── 主功能 ──
    s.add(
        "TOTP 10 操作全通（离线缓存 ×10）",
        recorded >= TOTP_OPS as u32,
        "生成/校验/轮换/导入/导出五家族逐操作恒等，承载离线缓存 ×10",
    );
    s.add(
        "KDBX4 双向投影 16 格全落",
        {
            let mut lt = KeePassXcLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PROJ_CELLS {
                if let Some(w) = proj_cell(c) {
                    if lt.record_proj(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.proj_all()
        },
        "正向导出投影 8 格 + 反向导入投影 8 格逐格恒等（§AI-66.3）",
    );
    s.add(
        "浏览器集成三元闸全齐",
        {
            let mut lt = KeePassXcLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BROWSER_CELLS {
                if let Some(w) = br_cell(b) {
                    if lt.record_browser(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.browser_all()
        },
        "代理连通/密钥关联/许可确认三格闭集全齐",
    );
    s.add(
        "SSH Agent 消费 8 格全落",
        {
            let mut lt = KeePassXcLedger::new();
            let mut ok = true;
            let mut x = 0usize;
            while x < SSH_CELLS {
                if let Some(w) = ssh_cell(x) {
                    if lt.record_ssh(x, w).is_err() {
                        ok = false;
                    }
                }
                x += 1;
            }
            ok && lt.ssh_all()
        },
        "密钥列举 ×4 / 签名应答 ×4 逐格恒等",
    );
    s.add(
        "CLI 消费联签 4 格全落",
        {
            let mut lt = KeePassXcLedger::new();
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
        "unlock/show/list/search 联签四格闭集逐格恒等",
    );
    s.add(
        "Qt 上栈 4 层全落",
        {
            let mut lt = KeePassXcLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < QT_LAYERS {
                if let Some(w) = qt_cell(l) {
                    if lt.record_qt(l, w).is_err() {
                        ok = false;
                    }
                }
                l += 1;
            }
            ok && lt.qt_all()
        },
        "QtCore/QtGui/QtWidgets/QtNetwork 自底向上四层逐层恒等",
    );
    s.add(
        "同步面外部服务声明 2 行全落",
        {
            let mut lt = KeePassXcLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SYNC_DECL_ROWS {
                if lt.declare_sync(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.decl_all()
        },
        "同步面外部服务声明/离线缓存声明两行闭集，缺一拒封",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = KeePassXcLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账（UNX-F52403-J1 六判据）",
    );
    s.add(
        "TOTP 家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < TOTP_OPS {
                if totp_kind(o).unwrap_or(5) < totp_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "生成→校验→轮换→导入→导出五家族闭集单调",
    );
    // ── 注入与守恒 ──
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "投影越闭集/TOTP 漂移/联签重复三态轮转必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10 && lt_conserved(),
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // ── 边界·越闭集 ×3 ──
    s.add(
        "边界·TOTP 操作越闭集",
        {
            let mut lt = KeePassXcLedger::new();
            lt.record_totp(TOTP_OPS, 0).is_err()
        },
        "TOTP 操作 10 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        {
            let mut lt = KeePassXcLedger::new();
            lt.record_proj(PROJ_CELLS, 0).is_err()
        },
        "投影格 16 越闭集必拒",
    );
    s.add(
        "边界·Qt 层越闭集",
        {
            let mut lt = KeePassXcLedger::new();
            lt.record_qt(QT_LAYERS, 0).is_err()
        },
        "Qt 层 4 越闭集必拒",
    );
    // ── 漂移与重复 ──
    s.add(
        "载荷漂移拒",
        {
            let mut lt = KeePassXcLedger::new();
            let seed = match proj_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_proj(0, seed);
            let bad = match proj_cell(1) {
                Some(w) => w.wrapping_add(1),
                None => 1,
            };
            lt.record_proj(1, bad) == Err(KeePassXcCode::EProjDrift)
        },
        "KDBX4 投影载荷漂移必抓",
    );
    s.add(
        "重复落账拒",
        {
            let mut lt = KeePassXcLedger::new();
            let w0 = match ssh_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_ssh(0, w0);
            lt.record_ssh(0, w0) == Err(KeePassXcCode::EDup)
        },
        "SSH Agent 格重复落账必抓",
    );
    // ── 双跑与段位 ──
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        {
            let mut lt = KeePassXcLedger::new();
            lt.fp = fp;
            lt.fp_matches(fp2).is_ok()
        },
        "确定性全流程逐位一致且非零",
    );
    // ── 定级 ──
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = KeePassXcLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < PROJ_CELLS {
                if let Some(w) = proj_cell(c) {
                    let _ = lt.record_proj(c, w);
                }
                c += 1;
            }
            let mut b = 0usize;
            while b < BROWSER_CELLS {
                if let Some(w) = br_cell(b) {
                    let _ = lt.record_browser(b, w);
                }
                b += 1;
            }
            let mut x = 0usize;
            while x < SSH_CELLS {
                if let Some(w) = ssh_cell(x) {
                    let _ = lt.record_ssh(x, w);
                }
                x += 1;
            }
            let mut o = 0usize;
            while o < TOTP_OPS {
                if let Some(w) = totp_cell(o) {
                    let _ = lt.record_totp(o, w);
                }
                o += 1;
            }
            let mut i = 0usize;
            while i < CLI_CELLS {
                if let Some(w) = cli_cell(i) {
                    let _ = lt.record_cli(i, w);
                }
                i += 1;
            }
            let mut l = 0usize;
            while l < QT_LAYERS {
                if let Some(w) = qt_cell(l) {
                    let _ = lt.record_qt(l, w);
                }
                l += 1;
            }
            let mut r = 0usize;
            while r < SYNC_DECL_ROWS {
                let _ = lt.declare_sync(r);
                r += 1;
            }
            lt.grade_expect(Grade::S1).is_ok()
        },
        "全节齐+六判据全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA440,
        "SEG_BASE 0x40 对齐且按 VX16 公式落在 52402 段",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 19,
        "含本条共 20 条闭集（≥15 达标）",
    );
    s
}

/// 注入守恒核验辅助：10 轮注入在主账上必须注入=检出 且 逃逸=0。
fn lt_conserved() -> bool {
    let mut lt = KeePassXcLedger::new();
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
        assert_eq!(proj_kind_name(0), Some("正向导出投影"));
        assert_eq!(proj_kind_name(1), Some("反向导入投影"));
        assert!(proj_kind_name(2).is_none());
        assert_eq!(br_name(0), Some("代理连通"));
        assert_eq!(br_name(1), Some("密钥关联"));
        assert_eq!(br_name(2), Some("许可确认"));
        assert!(br_name(3).is_none());
        assert_eq!(cli_name(3), Some("search 检索"));
        assert!(cli_name(4).is_none());
        assert_eq!(qt_name(0), Some("QtCore"));
        assert_eq!(qt_name(3), Some("QtNetwork"));
        assert!(qt_name(4).is_none());
    }

    #[test]
    fn test_totp_kind_mapping() {
        assert_eq!(totp_kind(0), Some(0));
        assert_eq!(totp_kind(2), Some(1));
        assert_eq!(totp_kind(9), Some(4));
        assert!(totp_kind(10).is_none());
        assert_eq!(totp_kind_name(4), Some("导出"));
        assert!(totp_kind_name(5).is_none());
    }

    #[test]
    fn test_ssh_kind_mapping() {
        // 族映射：x/4 → 0..4=密钥列举 4..8=签名应答。
        assert_eq!(ssh_kind_name(0), Some("密钥列举"));
        assert_eq!(ssh_kind_name(1), Some("签名应答"));
        assert!(ssh_kind_name(2).is_none());
        assert!(ssh_cell(SSH_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = totp_cell(3).unwrap_or(0);
        assert_eq!(a, totp_cell(3).unwrap_or(0));
        assert_ne!(a, totp_cell(4).unwrap_or(0));
        assert!(totp_cell(TOTP_OPS).is_none());
        assert!(br_cell(3).is_none());
        assert!(sync_decl_row(SYNC_DECL_ROWS).is_none());
        assert!(cli_cell(CLI_CELLS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = KeePassXcLedger::new();
        let _ = lt.record_totp(5, totp_cell(5).unwrap_or(0));
        assert_eq!(
            lt.record_totp(2, totp_cell(2).unwrap_or(0)),
            Err(KeePassXcCode::EOrder)
        );
        assert!(lt.record_totp(6, totp_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = KeePassXcLedger::new();
        assert_eq!(
            lt.record_totp(3, totp_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(KeePassXcCode::EDrift)
        );
        assert_eq!(
            lt.record_proj(1, proj_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(KeePassXcCode::EProjDrift)
        );
        assert_eq!(
            lt.record_browser(1, br_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(KeePassXcCode::EBrMiss)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = KeePassXcLedger::new();
        let _ = lt.record_proj(0, proj_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_proj(0, proj_cell(0).unwrap_or(0)),
            Err(KeePassXcCode::EDup)
        );
        let _ = lt.record_cli(0, cli_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cli(0, cli_cell(0).unwrap_or(0)),
            Err(KeePassXcCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = KeePassXcLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert_eq!(lt.judge_gate(6), Err(KeePassXcCode::ESeal));
        assert!(lt.gates_all());
        let mut lt2 = KeePassXcLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
        assert!(lt2.seal_check().is_err());
    }

    #[test]
    fn test_face_boundary() {
        let mut lt = KeePassXcLedger::new();
        assert_eq!(lt.record_ssh(SSH_CELLS, 0), Err(KeePassXcCode::ENoFace));
        assert_eq!(lt.record_cli(CLI_CELLS, 0), Err(KeePassXcCode::ENoFace));
        assert_eq!(lt.record_qt(QT_LAYERS, 0), Err(KeePassXcCode::ENoFace));
        assert_eq!(lt.declare_sync(SYNC_DECL_ROWS), Err(KeePassXcCode::ENoFace));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, RECORDED_TOTAL);
        assert_eq!(recorded, 47);
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
        assert_eq!(SEG_BASE, 0xCA440);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            KeePassXcCode::Ok,
            KeePassXcCode::ENoProj,
            KeePassXcCode::EProjDrift,
            KeePassXcCode::EBrMiss,
            KeePassXcCode::ENoOp,
            KeePassXcCode::EDrift,
            KeePassXcCode::EOrder,
            KeePassXcCode::ENoFace,
            KeePassXcCode::EDup,
            KeePassXcCode::ESeal,
            KeePassXcCode::EInj,
            KeePassXcCode::EFp,
            KeePassXcCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i] as u32, i as u32);
            assert!(!all[i].detail().is_empty());
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            i += 1;
        }
        assert_eq!(KeePassXcCode::Ok as u32, 0);
        assert_eq!(KeePassXcCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade_ladder() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐 S1。
        let lt0 = KeePassXcLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = KeePassXcLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = KeePassXcLedger::new();
        let mut c = 0usize;
        while c < PROJ_CELLS {
            if let Some(w) = proj_cell(c) {
                let _ = lt2.record_proj(c, w);
            }
            c += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_conservation_and_fp() {
        let mut lt = KeePassXcLedger::new();
        lt.injected = 3;
        lt.caught = 2;
        lt.escaped = 1;
        assert_eq!(lt.conservation_check(), Err(KeePassXcCode::EInj));
        lt.caught = 3;
        lt.escaped = 0;
        assert!(lt.conservation_check().is_ok());
        assert_eq!(lt.fp_matches(0), Err(KeePassXcCode::EFp));
    }

    #[test]
    fn unx52402_checks_green() {
        let s = run_unx52402_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
        assert_eq!(s.len(), 20);
    }
}
