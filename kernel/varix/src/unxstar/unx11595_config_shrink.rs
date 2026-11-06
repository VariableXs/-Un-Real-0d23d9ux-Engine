//! UNX-F11593 CONFIG 裁剪重建 applet 表收缩判据
//!
//! busybox 的 `.config` 不是「一堆独立开关」，而是一张**蕴含图**：打开
//! `CONFIG_ASH` 连带出 `ash / sh / [` 三个 applet，打开 `CONFIG_GREP`
//! 连带出 `grep / egrep / fgrep`，打开 `CONFIG_TAR` 连带出 `tar / gzip`
//! （`tar -z` 要 gzip 的压缩后端）。裁剪重建的目标不是「少编译几个文件」，
//! 而是**收缩后的 applet 表必须仍然自洽**：每个还在表里的 applet，其
//! 依赖（`tar → gzip`、`egrep → grep`、`sh → ash`）必须在表里；每个消失
//! 的 applet，其门控特性必须在配置里是关的。
//!
//! 本模块把这条链路拆成四层，逐层可对账：
//!
//! ```text
//! 位集   Config  = 12 个特性位（u16 低 12 位），高位一律拒（EBIT）
//! 闭包   rebuild = 逐特性查 IMPL 表取并集，得到 16 槽 applet 位集
//! 校验   validate 检依赖（REQS），audit_gates 检「applet 的门控特性还开着」
//! 收缩   shrink   给出 kept/lost/gained 与省下的字节数，非正当损失即拒
//! ```
//!
//! 「非正当损失」是这套判据的核心：`shrink` 不只统计少了几个 applet，
//! 还要拿**重建用的配置**去反查每个丢失项的 `GATES`——只有当它的所有
//! 门控特性都关着时，这个丢失才算「裁掉的」；否则就是「构建脚本漏了」，
//! 必须落 `CF_ELOST`。同理，收缩不允许**凭空多出** baseline 里没有的
//! applet（`CF_EGAIN`），那说明重建链把两张表接错了。
//!
//! 尺寸模型 `size_est = 4096 + 512 × 表项数` 是刻意取整的线性近似：判据
//! 只关心「省了多少个 512 字节块」这一整数事实，不假装知道真实 ELF 布局。
//!
//! 闸门次序：特性位 → 闭包 → 依赖 → 门控 → 收缩 → 增益 → 损失正当性。
//!
//! 账本 14 格（8 成 6 败）在 `audit` 逐格对账；冻结后重放须同指纹。
//! 无 std、无 unsafe、无 panic、无 unwrap。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址（本卡专属）。
pub const SEG_BASE: u32 = 0xfa80;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 特性数（12）。
pub const FEATURE_N: usize = 12;

/// applet 槽数（16）。
pub const APPLET_N: usize = 16;

/// 尺寸模型：固定开销。
pub const SIZE_BASE: u32 = 4096;

/// 尺寸模型：每个 applet 的增量。
pub const SIZE_PER_APP: u32 = 512;

// ---------------------------------------------------------------------------
// 特性编号
// ---------------------------------------------------------------------------

/// `CONFIG_ASH`。
pub const F_ASH: u8 = 0;
/// `CONFIG_GREP`。
pub const F_GREP: u8 = 1;
/// `CONFIG_TAR`。
pub const F_TAR: u8 = 2;
/// `CONFIG_PS`。
pub const F_PS: u8 = 3;
/// `CONFIG_SED`。
pub const F_SED: u8 = 4;
/// `CONFIG_FIND`。
pub const F_FIND: u8 = 5;
/// `CONFIG_LS`。
pub const F_LS: u8 = 6;
/// `CONFIG_CAT`。
pub const F_CAT: u8 = 7;
/// `CONFIG_INIT`。
pub const F_INIT: u8 = 8;
/// `CONFIG_VI`。
pub const F_VI: u8 = 9;
/// `CONFIG_AWK`。
pub const F_AWK: u8 = 10;
/// `CONFIG_GZIP`（独立压缩入口，与 tar 的 gzip 后端共享实现）。
pub const F_ZIP: u8 = 11;

/// 特性名表（与编号一一对应）。
pub const FEATURES: [&str; FEATURE_N] = [
    "ASH", "GREP", "TAR", "PS", "SED", "FIND", "LS", "CAT", "INIT", "VI", "AWK", "ZIP",
];

/// 全开配置的原始位（12 位全 1）。
pub const ALL_BITS: u16 = 0x0fff;

// ---------------------------------------------------------------------------
// applet 编号
// ---------------------------------------------------------------------------

/// applet 名表（与编号一一对应）。
pub const APPS: [&str; APPLET_N] = [
    "ash", "sh", "[", "grep", "egrep", "fgrep", "tar", "gzip", "ps", "sed", "find", "ls",
    "cat", "init", "vi", "awk",
];

/// 蕴含表：特性 → 连带 applet 编号。
///
/// 这张表是「一个开关带出几个入口」的唯一事实来源；`rebuild` 只查它，
/// 不再看特性名，避免把字符串比较混进热路径。
pub const IMPL: [(u8, &[u8]); FEATURE_N] = [
    (F_ASH, &[0, 1, 2]),
    (F_GREP, &[3, 4, 5]),
    (F_TAR, &[6, 7]),
    (F_PS, &[8]),
    (F_SED, &[9]),
    (F_FIND, &[10]),
    (F_LS, &[11]),
    (F_CAT, &[12]),
    (F_INIT, &[13]),
    (F_VI, &[14]),
    (F_AWK, &[15]),
    (F_ZIP, &[7]),
];

/// 依赖表：applet → 它运行时必须同在的 applet。
///
/// `tar` 的 `-z` 分支直接调 gzip 的实现；`egrep/fgrep` 是 grep 的编译期
/// 改名入口；`sh` 只是 ash 的别名实现体。三者缺一，链接期就会断。
pub const REQS: [(u8, u8); 3] = [(6, 7), (4, 3), (1, 0)];

/// 门控表：applet → 能启用它的全部特性。
///
/// 与 `IMPL` 互为反向索引；`gzip` 有两个门（TAR 的后端、ZIP 的独立入口），
/// 所以丢失正当性判断必须看「门控集合里**是否还有**开着的」，而不是只看
/// 第一个门。
pub const GATES: [&[u8]; APPLET_N] = [
    &[F_ASH],
    &[F_ASH],
    &[F_ASH],
    &[F_GREP],
    &[F_GREP],
    &[F_GREP],
    &[F_TAR],
    &[F_TAR, F_ZIP],
    &[F_PS],
    &[F_SED],
    &[F_FIND],
    &[F_LS],
    &[F_CAT],
    &[F_INIT],
    &[F_VI],
    &[F_AWK],
];

// ---------------------------------------------------------------------------
// 错误码：8 个域错误 + 5 个账本错误
// ---------------------------------------------------------------------------

/// 特性位越界（`from_raw` 的高位脏位，或未知特性编号）。
pub const CF_EBIT: u32 = SEG_BASE + 0x01;
/// 未知 applet 编号。
pub const CF_ESET: u32 = SEG_BASE + 0x02;
/// 门控缺失：applet 在表里，但启用它的特性一个都没开。
pub const CF_EIMPLY: u32 = SEG_BASE + 0x03;
/// 依赖缺失：applet 在表里，但它的依赖不在。
pub const CF_EDEP: u32 = SEG_BASE + 0x04;
/// 空 applet 集（重建出零个入口，等价于「没编进任何东西」）。
pub const CF_EEMPTY: u32 = SEG_BASE + 0x05;
/// 收缩增益：重建结果含 baseline 里没有的 applet。
pub const CF_EGAIN: u32 = SEG_BASE + 0x06;
/// 非正当损失：applet 消失，但它的门控特性还开着。
pub const CF_ELOST: u32 = SEG_BASE + 0x07;
/// 容量/编号越界（特性编号 ≥ 12、applet 编号 ≥ 16）。
pub const CF_ECAP: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const CF_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const CF_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const CF_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const CF_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const CF_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// 位集操作。
pub const OP_BITSET: u8 = 0x01;
/// 蕴含闭包。
pub const OP_IMPLY: u8 = 0x02;
/// 重建。
pub const OP_REBUILD: u8 = 0x03;
/// 收缩报告。
pub const OP_SHRINK: u8 = 0x04;
/// 依赖/门控校验。
pub const OP_VALIDATE: u8 = 0x05;
/// 尺寸估计。
pub const OP_SIZE: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希：FNV-1a，每字节 8 次乘法
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节（每字节 8 次乘法）。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字。
fn fold(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut sh = 0u32;
    while sh < 64 {
        let b = ((w >> sh) & 0xff) as u8;
        v = fold_byte(v, b);
        sh += 8;
    }
    v
}

/// 折入一段字节。
fn fold_bytes(mut h: u64, s: &[u8]) -> u64 {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            h = fold_byte(h, *b);
        }
        i += 1;
    }
    h
}

/// 十进制拼进 String。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut at = buf.len();
    while v > 0 {
        let d = (v % 10) as u8;
        v /= 10;
        at = at.saturating_sub(1);
        if let Some(dst) = buf.get_mut(at) {
            *dst = b'0' + d;
        }
    }
    let mut i = at;
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// 配置位集
// ---------------------------------------------------------------------------

/// 特性位集（12 位有效）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    bits: u16,
}

impl Config {
    /// 空配置（全关）。
    pub const fn empty() -> Config {
        Config { bits: 0 }
    }

    /// 全开配置。
    pub const fn full() -> Config {
        Config { bits: ALL_BITS }
    }

    /// 位掩码 → 配置：高位脏位一律 EBIT。
    pub fn from_raw(raw: u16) -> Result<Config, u32> {
        if raw & !ALL_BITS != 0 {
            return Err(CF_EBIT);
        }
        Ok(Config { bits: raw })
    }

    /// 原始位。
    pub fn raw(&self) -> u16 {
        self.bits
    }

    /// 是否启用某特性（越界编号视为关）。
    pub fn has(&self, f: u8) -> bool {
        if (f as usize) >= FEATURE_N {
            return false;
        }
        (self.bits & (1u16 << f)) != 0
    }

    /// 开启某特性（越界编号 → ECAP）。
    pub fn set(&mut self, f: u8) -> Result<(), u32> {
        if (f as usize) >= FEATURE_N {
            return Err(CF_ECAP);
        }
        self.bits |= 1u16 << f;
        Ok(())
    }

    /// 关闭某特性（越界编号静默无操作——关闭不需要构造新的位）。
    pub fn clear(&mut self, f: u8) {
        if (f as usize) < FEATURE_N {
            self.bits &= !(1u16 << f);
        }
    }

    /// 开着的特性数。
    pub fn count(&self) -> usize {
        let mut n = 0usize;
        let mut f = 0u8;
        while (f as usize) < FEATURE_N {
            if self.has(f) {
                n += 1;
            }
            f = f.saturating_add(1);
        }
        n
    }

    /// 并集。
    pub fn union(&self, other: &Config) -> Config {
        Config { bits: self.bits | other.bits }
    }

    /// 交集。
    pub fn intersect(&self, other: &Config) -> Config {
        Config { bits: self.bits & other.bits }
    }

    /// 文本（空格分隔）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut f = 0usize;
        while f < FEATURE_N {
            if self.has(f as u8) {
                if let Some(n) = FEATURES.get(f) {
                    if !s.is_empty() {
                        s.push('+');
                    }
                    s.push_str(n);
                }
            }
            f += 1;
        }
        s
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        fold(FNV_OFF, self.bits as u64)
    }
}

/// 便捷构造：特性编号列表 → 配置。
pub fn config_of(feats: &[u8]) -> Result<Config, u32> {
    let mut c = Config::empty();
    let mut i = 0usize;
    while i < feats.len() {
        match feats.get(i) {
            Some(f) => c.set(*f)?,
            None => return Err(CF_EBIT),
        }
        i += 1;
    }
    Ok(c)
}

// ---------------------------------------------------------------------------
// applet 位集
// ---------------------------------------------------------------------------

/// applet 位集（16 位全用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Applets {
    bits: u16,
}

impl Applets {
    /// 空集。
    pub const fn empty() -> Applets {
        Applets { bits: 0 }
    }

    /// 全表。
    pub const fn full() -> Applets {
        Applets { bits: 0xffff }
    }

    /// 加入一个 applet（编号越界 → ESET）。
    pub fn add(&mut self, i: u8) -> Result<(), u32> {
        if (i as usize) >= APPLET_N {
            return Err(CF_ESET);
        }
        self.bits |= 1u16 << i;
        Ok(())
    }

    /// 是否在表里。
    pub fn has(&self, i: u8) -> bool {
        if (i as usize) >= APPLET_N {
            return false;
        }
        (self.bits & (1u16 << i)) != 0
    }

    /// 表项数。
    pub fn count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0u8;
        while (i as usize) < APPLET_N {
            if self.has(i) {
                n += 1;
            }
            i = i.saturating_add(1);
        }
        n
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.bits == 0
    }

    /// 原始位。
    pub fn raw(&self) -> u16 {
        self.bits
    }

    /// 交集。
    pub fn intersect(&self, other: &Applets) -> Applets {
        Applets { bits: self.bits & other.bits }
    }

    /// 差集（self − other）。
    pub fn minus(&self, other: &Applets) -> Applets {
        Applets { bits: self.bits & !other.bits }
    }

    /// 并集。
    pub fn union(&self, other: &Applets) -> Applets {
        Applets { bits: self.bits | other.bits }
    }

    /// 是否子集。
    pub fn is_subset(&self, other: &Applets) -> bool {
        (self.bits & !other.bits) == 0
    }

    /// 文本（空格分隔，按编号序）。
    pub fn list(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < APPLET_N {
            if self.has(i as u8) {
                if let Some(n) = APPS.get(i) {
                    if !s.is_empty() {
                        s.push(' ');
                    }
                    s.push_str(n);
                }
            }
            i += 1;
        }
        s
    }

    /// 指纹（逐项折名 + 尾折位集）。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < APPLET_N {
            if self.has(i as u8) {
                if let Some(n) = APPS.get(i) {
                    h = fold_bytes(h, n.as_bytes());
                    h = fold_byte(h, b',');
                }
            }
            i += 1;
        }
        fold(h, self.bits as u64)
    }
}

/// 位集 → applet 集（编号列表，忽略越界项）。
pub fn applets_of(ids: &[u8]) -> Applets {
    let mut a = Applets::empty();
    let mut i = 0usize;
    while i < ids.len() {
        if let Some(x) = ids.get(i) {
            let _ = a.add(*x);
        }
        i += 1;
    }
    a
}

// ---------------------------------------------------------------------------
// 重建（蕴含闭包）
// ---------------------------------------------------------------------------

/// 配置 → applet 集：逐特性查 IMPL 取并集。
pub fn rebuild(cfg: &Config) -> Applets {
    let mut a = Applets::empty();
    let mut f = 0usize;
    while f < FEATURE_N {
        if cfg.has(f as u8) {
            if let Some((_, list)) = IMPL.get(f) {
                let mut i = 0usize;
                while i < list.len() {
                    if let Some(x) = list.get(i) {
                        let _ = a.add(*x);
                    }
                    i += 1;
                }
            }
        }
        f += 1;
    }
    a
}

/// 反查最小配置：让 `a` 里每个 applet 的**全部**门控特性都开。
///
/// 为什么取「全部门控」而不是第一个门：`gzip` 同时被 TAR 与 ZIP 门控，
/// 只开 ZIP 也够运行，但 `minimal_for` 的语义是「解释这张表需要哪些开关」，
/// 取并集才与 `rebuild` 互逆（`rebuild(minimal_for(a)) ⊇ a`）。
pub fn minimal_for(a: &Applets) -> Config {
    let mut c = Config::empty();
    let mut ap = 0usize;
    while ap < APPLET_N {
        if a.has(ap as u8) {
            if let Some(gs) = GATES.get(ap) {
                let mut i = 0usize;
                while i < gs.len() {
                    if let Some(f) = gs.get(i) {
                        let _ = c.set(*f);
                    }
                    i += 1;
                }
            }
        }
        ap += 1;
    }
    c
}

// ---------------------------------------------------------------------------
// 校验
// ---------------------------------------------------------------------------

/// 依赖校验：非空，且每个在表 applet 的依赖都在表里。
pub fn validate(a: &Applets) -> Result<(), u32> {
    if a.is_empty() {
        return Err(CF_EEMPTY);
    }
    let mut i = 0usize;
    while i < REQS.len() {
        if let Some((from, to)) = REQS.get(i) {
            if a.has(*from) && !a.has(*to) {
                return Err(CF_EDEP);
            }
        }
        i += 1;
    }
    Ok(())
}

/// 门控校验：每个在表 applet 至少有一个门控特性开着。
pub fn audit_gates(cfg: &Config, a: &Applets) -> Result<(), u32> {
    if a.is_empty() {
        return Err(CF_EEMPTY);
    }
    let mut ap = 0usize;
    while ap < APPLET_N {
        if a.has(ap as u8) {
            let mut open = false;
            if let Some(gs) = GATES.get(ap) {
                let mut i = 0usize;
                while i < gs.len() {
                    if let Some(f) = gs.get(i) {
                        if cfg.has(*f) {
                            open = true;
                        }
                    }
                    i += 1;
                }
            }
            if !open {
                return Err(CF_EIMPLY);
            }
        }
        ap += 1;
    }
    Ok(())
}

/// 单个 applet 是否存在任一开着的门控特性。
pub fn gate_open(cfg: &Config, applet: u8) -> bool {
    let mut open = false;
    if let Some(gs) = GATES.get(applet as usize) {
        let mut i = 0usize;
        while i < gs.len() {
            if let Some(f) = gs.get(i) {
                if cfg.has(*f) {
                    open = true;
                }
            }
            i += 1;
        }
    }
    open
}

// ---------------------------------------------------------------------------
// 收缩报告
// ---------------------------------------------------------------------------

/// 收缩结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 保住（baseline ∩ rebuilt）。
    pub kept: usize,
    /// 丢失（baseline − rebuilt）。
    pub lost: usize,
    /// 多出（rebuilt − baseline）。
    pub gained: usize,
    /// 省下的字节（线性尺寸模型）。
    pub saved: u32,
}

impl Report {
    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("kept=");
        push_u64(&mut s, self.kept as u64);
        s.push_str(" lost=");
        push_u64(&mut s, self.lost as u64);
        s.push_str(" gained=");
        push_u64(&mut s, self.gained as u64);
        s.push_str(" saved=");
        push_u64(&mut s, self.saved as u64);
        s
    }

    /// 编码（kept*100 + lost*10 + gained）。
    pub fn code(&self) -> u32 {
        (self.kept as u32)
            .saturating_mul(100)
            .saturating_add((self.lost as u32).saturating_mul(10))
            .saturating_add(self.gained as u32)
    }
}

/// 尺寸估计（线性模型）。
pub fn size_est(a: &Applets) -> u32 {
    SIZE_BASE.saturating_add(SIZE_PER_APP.saturating_mul(a.count() as u32))
}

/// 收缩：先拒空、再拒增益、最后逐项判损失正当性。
///
/// 三段顺序不能换：空集是「构建没产物」，增益是「表接错」，非正当损失
/// 是「配置与产物不一致」——诊断优先级按此从重到轻。
pub fn shrink(baseline: &Applets, rebuilt: &Applets, cfg: &Config) -> Result<Report, u32> {
    if rebuilt.is_empty() {
        return Err(CF_EEMPTY);
    }
    let kept = baseline.intersect(rebuilt).count();
    let lost = baseline.minus(rebuilt);
    let gained = rebuilt.minus(baseline);
    if gained.count() > 0 {
        return Err(CF_EGAIN);
    }
    let mut ap = 0usize;
    while ap < APPLET_N {
        if lost.has(ap as u8) && gate_open(cfg, ap as u8) {
            return Err(CF_ELOST);
        }
        ap += 1;
    }
    Ok(Report {
        kept,
        lost: lost.count(),
        gained: gained.count(),
        saved: size_est(baseline).saturating_sub(size_est(rebuilt)),
    })
}

// ---------------------------------------------------------------------------
// 样本与账本
// ---------------------------------------------------------------------------

/// 一格样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// 账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记（严格槽序，冻结后拒）。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CF_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len = self.len.saturating_add(1);
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(CF_EBAL);
        }
        const VALS: [u32; 8] = [12, 3, 6, 16, 3, 13, 12288, 4095];
        const CODES: [u32; 6] = [
            CF_EBIT,
            CF_EIMPLY,
            CF_EDEP,
            CF_EEMPTY,
            CF_EGAIN,
            CF_ELOST,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CF_EBAL),
            };
            if s.slot as usize != i {
                return Err(CF_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CF_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CF_EVALUE);
                        }
                    }
                    None => return Err(CF_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CF_EVALUE);
                        }
                    }
                    None => return Err(CF_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CF_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(CF_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
        } else {
            Err(CF_ESEAL)
        }
    }

    /// 冻结根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// `Result<u32, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 账本组装：8 成 6 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let full = Config::full();
    let base = rebuild(&full);

    // 0: 全开配置 12 个特性。
    let _ = l.register(cell(0, OP_BITSET, Ok(full.count() as u32)));

    // 1: 仅 ASH 重建出 3 个 applet。
    let _ = l.register(cell(
        1,
        OP_IMPLY,
        match config_of(&[F_ASH]) {
            Ok(c) => Ok(rebuild(&c).count() as u32),
            Err(e) => Err(e),
        },
    ));

    // 2: ASH + GREP 重建出 6 个。
    let _ = l.register(cell(
        2,
        OP_IMPLY,
        match config_of(&[F_ASH, F_GREP]) {
            Ok(c) => Ok(rebuild(&c).count() as u32),
            Err(e) => Err(e),
        },
    ));

    // 3: 全表依赖校验通过，表项 16。
    let v3: Result<u32, u32> = match validate(&base) {
        Ok(()) => Ok(base.count() as u32),
        Err(e) => Err(e),
    };
    let _ = l.register(cell(3, OP_VALIDATE, v3));

    // 4: 全表 → 仅 ASH，保住 3 项。
    let _ = l.register(cell(
        4,
        OP_SHRINK,
        match config_of(&[F_ASH]) {
            Ok(c) => match shrink(&base, &rebuild(&c), &c) {
                Ok(r) => Ok(r.kept as u32),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        },
    ));

    // 5: 同上，丢失 13 项。
    let _ = l.register(cell(
        5,
        OP_SHRINK,
        match config_of(&[F_ASH]) {
            Ok(c) => match shrink(&base, &rebuild(&c), &c) {
                Ok(r) => Ok(r.lost as u32),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        },
    ));

    // 6: 全表尺寸 4096 + 512×16。
    let _ = l.register(cell(6, OP_SIZE, Ok(size_est(&base))));

    // 7: 全表反查最小配置 = 0x0fff。
    let _ = l.register(cell(7, OP_REBUILD, Ok(minimal_for(&base).raw() as u32)));

    // 8: 高位脏位 0x1000。
    let _ = l.register(cell(
        8,
        OP_BITSET,
        match Config::from_raw(0x1000) {
            Ok(_) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 9: 空配置下声称有 gzip → 门控缺失。
    let _ = l.register(cell(
        9,
        OP_VALIDATE,
        match audit_gates(&Config::empty(), &applets_of(&[7])) {
            Ok(()) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 10: 只有 tar 没有 gzip → 依赖缺失。
    let _ = l.register(cell(
        10,
        OP_VALIDATE,
        match validate(&applets_of(&[6])) {
            Ok(()) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 11: 空集。
    let _ = l.register(cell(
        11,
        OP_VALIDATE,
        match validate(&Applets::empty()) {
            Ok(()) => Ok(0),
            Err(c) => Err(c),
        },
    ));

    // 12: 收缩出 baseline 没有的 cat → 增益。
    let _ = l.register(cell(
        12,
        OP_SHRINK,
        match config_of(&[F_LS, F_CAT]) {
            Ok(c) => match shrink(&applets_of(&[11]), &applets_of(&[11, 12]), &c) {
                Ok(_) => Ok(0),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        },
    ));

    // 13: 全配置但只重建 ASH → 非正当损失。
    let _ = l.register(cell(
        13,
        OP_SHRINK,
        match config_of(&[F_ASH]) {
            Ok(c) => match shrink(&base, &rebuild(&c), &full) {
                Ok(_) => Ok(0),
                Err(e) => Err(e),
            },
            Err(e) => Err(e),
        },
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据面
// ---------------------------------------------------------------------------

/// 位集：开/关/计数/并交/高位。
fn bitset_ok() -> bool {
    let e0 = Config::empty();
    if e0.count() != 0 || e0.raw() != 0 || e0.text().as_str() != "" {
        return false;
    }
    let a = match config_of(&[F_ASH]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let mut b = a;
    if b.set(F_GREP) != Ok(()) {
        return false;
    }
    let c = b.count() == 2 && b.has(F_ASH) && b.has(F_GREP) && !b.has(F_TAR);
    b.clear(F_ASH);
    let d = b.count() == 1 && !b.has(F_ASH) && b.has(F_GREP);
    let e = match Config::from_raw(ALL_BITS) {
        Ok(v) => v.count() == 12,
        Err(_) => false,
    };
    let f = Config::from_raw(0x1000) == Err(CF_EBIT);
    let mut g = Config::empty();
    let h = g.set(12) == Err(CF_ECAP);
    c && d && e && f && h && !a.has(F_TAR)
}

/// 蕴含闭包：开关 → applet 集。
fn imply_ok() -> bool {
    let a = match config_of(&[F_ASH]) {
        Ok(c) => rebuild(&c),
        Err(_) => return false,
    };
    let b = a.count() == 3 && a.has(0) && a.has(1) && a.has(2) && !a.has(3);
    let g = match config_of(&[F_GREP]) {
        Ok(c) => rebuild(&c),
        Err(_) => return false,
    };
    let c = g.count() == 3 && g.has(3) && g.has(4) && g.has(5) && !g.has(0);
    let t = match config_of(&[F_TAR]) {
        Ok(c) => rebuild(&c),
        Err(_) => return false,
    };
    let d = t.count() == 2 && t.has(6) && t.has(7);
    let both = match config_of(&[F_ASH, F_GREP]) {
        Ok(c) => rebuild(&c),
        Err(_) => return false,
    };
    let e = both.count() == 6;
    b && c && d && e
}

/// 依赖校验。
fn dep_ok() -> bool {
    let a = validate(&applets_of(&[6])) == Err(CF_EDEP);
    let b = validate(&applets_of(&[6, 7])) == Ok(());
    let c = validate(&applets_of(&[4])) == Err(CF_EDEP);
    let d = validate(&applets_of(&[4, 3])) == Ok(());
    let e = validate(&applets_of(&[1])) == Err(CF_EDEP);
    let f = validate(&applets_of(&[1, 0])) == Ok(());
    a && b && c && d && e && f
}

/// 空集与编号越界。
fn empty_ok() -> bool {
    let a = validate(&Applets::empty()) == Err(CF_EEMPTY);
    let b = audit_gates(&Config::full(), &Applets::empty()) == Err(CF_EEMPTY);
    let c = shrink(&Applets::empty(), &Applets::empty(), &Config::full()) == Err(CF_EEMPTY);
    let mut d = Applets::empty();
    let e = d.add(16) == Err(CF_ESET);
    let f = d.add(15) == Ok(()) && d.has(15) && !d.has(14);
    let g = Applets::full().count() == 16;
    a && b && c && e && f && g
}

/// 收缩报告：数值与文本。
fn report_ok() -> bool {
    let full = Config::full();
    let base = rebuild(&full);
    let cfg = match config_of(&[F_ASH]) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let r = match shrink(&base, &rebuild(&cfg), &cfg) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = r.kept == 3 && r.lost == 13 && r.gained == 0;
    let b = r.saved == 512 * 13;
    let c = r.text().as_str() == "kept=3 lost=13 gained=0 saved=6656";
    let d = r.code() == 3 * 100 + 13 * 10;
    a && b && c && d
}

/// 损失正当性：全关特性下丢失 OK，任一开着的门控即拒。
fn justified_ok() -> bool {
    let full = Config::full();
    let base = rebuild(&full);
    let mut ok = true;
    let pairs: [(u8, u32); 3] = [(F_ASH, 3), (F_GREP, 3), (F_ZIP, 1)];
    let mut i = 0usize;
    while i < pairs.len() {
        if let Some((f, expect)) = pairs.get(i) {
            match config_of(&[*f]) {
                Ok(c) => match shrink(&base, &rebuild(&c), &c) {
                    Ok(r) => {
                        if r.kept != *expect as usize {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                },
                Err(_) => ok = false,
            }
        }
        i += 1;
    }
    let bad = match config_of(&[F_GREP]) {
        Ok(c) => shrink(&base, &rebuild(&c), &full).err() == Some(CF_ELOST),
        Err(_) => false,
    };
    ok && bad
}

/// 增益拒收。
fn gain_ok() -> bool {
    let cfg = match config_of(&[F_LS, F_CAT]) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let a = shrink(&applets_of(&[11]), &applets_of(&[11, 12]), &cfg) == Err(CF_EGAIN);
    let b = shrink(&applets_of(&[11, 12]), &applets_of(&[11, 12]), &cfg).is_ok();
    let c = match config_of(&[F_CAT]) {
        Ok(x) => shrink(&applets_of(&[11, 12]), &applets_of(&[12]), &x),
        Err(_) => return false,
    };
    match c {
        Ok(r) => {
            a && b && r.kept == 1 && r.lost == 1 && r.gained == 0 && r.saved == 512
        }
        Err(_) => false,
    }
}

/// 最小配置反查。
fn minimal_ok() -> bool {
    let all = rebuild(&Config::full());
    let a = minimal_for(&all) == Config::full();
    let b = minimal_for(&applets_of(&[11])).raw() == (1u16 << F_LS);
    let c = minimal_for(&applets_of(&[7])).raw() == ((1u16 << F_TAR) | (1u16 << F_ZIP));
    let d = minimal_for(&Applets::empty()).raw() == 0;
    let e = minimal_for(&applets_of(&[0, 3])).raw() == ((1u16 << F_ASH) | (1u16 << F_GREP));
    a && b && c && d && e
}

/// 尺寸模型。
fn size_ok() -> bool {
    let a = size_est(&Applets::empty()) == SIZE_BASE;
    let b = size_est(&rebuild(&Config::full())) == 4096 + 512 * 16;
    let c = size_est(&applets_of(&[0, 1, 2])) == 4096 + 512 * 3;
    let d = size_est(&applets_of(&[0])) == 4608;
    a && b && c && d
}

/// 门控校验。
fn gate_ok() -> bool {
    let full = Config::full();
    let a = audit_gates(&full, &rebuild(&full)) == Ok(());
    let b = audit_gates(&Config::empty(), &applets_of(&[7])) == Err(CF_EIMPLY);
    let c = match config_of(&[F_ZIP]) {
        Ok(x) => audit_gates(&x, &applets_of(&[7])) == Ok(()),
        Err(_) => false,
    };
    let d = gate_open(&Config::empty(), 0) == false;
    let e = gate_open(&full, 0) && gate_open(&full, 15) && !gate_open(&full, 16);
    a && b && c && d && e
}

/// 文本与指纹。
fn text_ok() -> bool {
    let c = match config_of(&[F_ASH, F_GREP]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = c.text().as_str() == "ASH+GREP";
    let b = Config::empty().text().is_empty();
    let ap = rebuild(&c);
    let d = ap.list().as_str() == "ash sh [ grep egrep fgrep";
    let e = c.seal() == config_of(&[F_GREP, F_ASH]).map(|x| x.seal()).unwrap_or(0);
    let f = ap.seal() == rebuild(&c).seal() && ap.seal() != Applets::empty().seal();
    a && b && d && e && f
}

/// 账本闭环。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    if l.len() != SAMPLES || l.ok_count() != 8 || l.err_count() != 6 {
        return false;
    }
    if l.seal() != build_ledger().seal() {
        return false;
    }
    let mut l2 = build_ledger();
    let root = match l2.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if l2.replay() != Ok(root) || !l2.is_frozen() || l2.root() != root {
        return false;
    }
    if l2.freeze() != Err(CF_EBAL) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    if let Some(dst) = out.get_mut(0) {
        *dst = bitset_ok() && imply_ok() && dep_ok() && empty_ok();
    }
    if let Some(dst) = out.get_mut(1) {
        *dst = report_ok() && justified_ok() && gain_ok();
    }
    if let Some(dst) = out.get_mut(2) {
        *dst = minimal_ok() && size_ok() && gate_ok();
    }
    if let Some(dst) = out.get_mut(3) {
        *dst = text_ok() && ledger_ok();
    }
    out
}

/// 判据集合（13 项）。
pub fn run_unx11595_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11595.shrink");

    s.add("CF01-配置位集", bitset_ok(), "static");
    s.add("CF02-蕴含闭包", imply_ok(), "static");
    s.add("CF03-依赖校验", dep_ok(), "static");
    s.add("CF04-空集拒收", empty_ok(), "static");
    s.add("CF05-收缩报告", report_ok(), "static");
    s.add("CF06-损失正当", justified_ok(), "static");
    s.add("CF07-增益拒收", gain_ok(), "static");
    s.add("CF08-最小配置", minimal_ok(), "static");
    s.add("CF09-尺寸模型", size_ok(), "static");
    s.add("CF10-门控校验", gate_ok(), "static");
    s.add("CF11-文本与指纹", text_ok(), "static");
    s.add("CF12-账本对账", build_ledger().audit() == Ok(()), "static");
    let four = inject_four();
    s.add("CF13-冻结重放四注入", four[0] && four[1] && four[2] && four[3], "static");

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitset_roundtrip() {
        let c = config_of(&[F_ASH, F_GREP, F_ZIP]).expect("c");
        assert_eq!(c.count(), 3);
        assert_eq!(c.raw(), (1 << F_ASH) | (1 << F_GREP) | (1 << F_ZIP));
        assert_eq!(Config::from_raw(0x1000).err(), Some(CF_EBIT));
        assert_eq!(Config::from_raw(0x0fff).map(|x| x.count()), Ok(12));
    }

    #[test]
    fn set_out_of_range() {
        let mut c = Config::empty();
        assert_eq!(c.set(12), Err(CF_ECAP));
        assert_eq!(c.set(11), Ok(()));
        c.clear(11);
        assert_eq!(c.count(), 0);
    }

    #[test]
    fn implication_sets() {
        assert_eq!(rebuild(&config_of(&[F_ASH]).expect("a")).list().as_str(), "ash sh [");
        assert_eq!(rebuild(&config_of(&[F_GREP]).expect("g")).count(), 3);
        assert_eq!(rebuild(&config_of(&[F_TAR]).expect("t")).list().as_str(), "tar gzip");
        assert_eq!(rebuild(&config_of(&[F_ASH, F_GREP]).expect("b")).count(), 6);
        assert_eq!(rebuild(&Config::empty()).count(), 0);
    }

    #[test]
    fn dependencies() {
        assert_eq!(validate(&applets_of(&[6])), Err(CF_EDEP));
        assert_eq!(validate(&applets_of(&[6, 7])), Ok(()));
        assert_eq!(validate(&applets_of(&[4])), Err(CF_EDEP));
        assert_eq!(validate(&Applets::empty()), Err(CF_EEMPTY));
    }

    #[test]
    fn gates() {
        assert_eq!(audit_gates(&Config::empty(), &applets_of(&[7])), Err(CF_EIMPLY));
        assert_eq!(audit_gates(&config_of(&[F_ZIP]).expect("z"), &applets_of(&[7])), Ok(()));
        assert_eq!(audit_gates(&Config::full(), &rebuild(&Config::full())), Ok(()));
        assert!(!gate_open(&Config::empty(), 7));
    }

    #[test]
    fn full_table_shape() {
        let a = rebuild(&Config::full());
        assert_eq!(a.count(), 16);
        assert_eq!(validate(&a), Ok(()));
        assert_eq!(a.list().as_str().split(' ').count(), 16);
    }

    #[test]
    fn shrink_numbers() {
        let full = Config::full();
        let base = rebuild(&full);
        let cfg = config_of(&[F_ASH]).expect("c");
        let r = shrink(&base, &rebuild(&cfg), &cfg).expect("r");
        assert_eq!(r.kept, 3);
        assert_eq!(r.lost, 13);
        assert_eq!(r.gained, 0);
        assert_eq!(r.saved, 6656);
        assert_eq!(r.text().as_str(), "kept=3 lost=13 gained=0 saved=6656");
        assert_eq!(r.code(), 430);
    }

    #[test]
    fn unjustified_loss() {
        let full = Config::full();
        let base = rebuild(&full);
        let cfg = config_of(&[F_ASH]).expect("c");
        assert_eq!(shrink(&base, &rebuild(&cfg), &full).err(), Some(CF_ELOST));
    }

    #[test]
    fn gained_rejected() {
        let cfg = config_of(&[F_LS, F_CAT]).expect("c");
        assert_eq!(
            shrink(&applets_of(&[11]), &applets_of(&[11, 12]), &cfg).err(),
            Some(CF_EGAIN)
        );
        let cfg_cat = config_of(&[F_CAT]).expect("cat");
        let ok = shrink(&applets_of(&[11, 12]), &applets_of(&[12]), &cfg_cat).expect("ok");
        assert_eq!(ok.kept, 1);
        assert_eq!(ok.lost, 1);
    }

    #[test]
    fn minimal_config() {
        let all = rebuild(&Config::full());
        assert_eq!(minimal_for(&all), Config::full());
        assert_eq!(minimal_for(&applets_of(&[7])).raw(), (1 << F_TAR) | (1 << F_ZIP));
        assert_eq!(minimal_for(&applets_of(&[11])).raw(), 1 << F_LS);
        assert_eq!(minimal_for(&Applets::empty()).raw(), 0);
    }

    #[test]
    fn size_model() {
        assert_eq!(size_est(&Applets::empty()), 4096);
        assert_eq!(size_est(&rebuild(&Config::full())), 12288);
        assert_eq!(size_est(&applets_of(&[0])), 4608);
    }

    #[test]
    fn applet_set_ops() {
        let a = applets_of(&[0, 1, 2]);
        let b = applets_of(&[1, 2, 3]);
        assert_eq!(a.intersect(&b).count(), 2);
        assert_eq!(a.minus(&b).count(), 1);
        assert_eq!(a.union(&b).count(), 4);
        assert!(applets_of(&[1]).is_subset(&a));
        assert!(!a.is_subset(&applets_of(&[1])));
        assert_eq!(Applets::full().count(), 16);
        assert_eq!(Applets::empty().add(16).err(), Some(CF_ESET));
    }

    #[test]
    fn config_text_and_seal() {
        let c = config_of(&[F_GREP, F_ASH]).expect("c");
        assert_eq!(c.text().as_str(), "ASH+GREP");
        assert!(Config::empty().text().is_empty());
        assert_eq!(c.seal(), Config::empty().union(&c).seal());
        assert!(c.seal() != Config::full().seal());
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(12));
        assert_eq!(l.sample(6).map(|s| s.value), Some(12288));
        assert_eq!(l.sample(8).map(|s| s.code), Some(CF_EBIT));
        assert_eq!(l.sample(13).map(|s| s.code), Some(CF_ELOST));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(CF_EBAL));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn injections() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11595_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
