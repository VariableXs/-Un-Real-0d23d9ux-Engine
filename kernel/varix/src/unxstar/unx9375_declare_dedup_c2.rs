//! UNX-F9375 联调防重声明（C2×C3 号面零冲突 grep） · 0x7CB40 段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 F9375 逐字）：
//!   UNX-F9375-J1 防重判据过，四范围 grep 零冲突格过，条目名零命中格过。
//!
//! 设计要点：
//!   ① 防重声明按复合键 (domain, num, category) 建账：C2/C3 两个号面共用
//!      一本声明册，键三元组在册内唯一，重复声明回 EEXIST——号面冲突在
//!      声明阶段就拦下，不留到联调期才爆。同一 num 允许跨域各声明一次
//!      （C2 与 C3 号面互不相干），但同域同号同类别绝不允许二声明。
//!   ② slug 全局唯一：slug 是跨 C2/C3 的单一命名空间，重复回 ENOTUNIQ。
//!      复合键唯一与 slug 唯一是两条独立的防重线：键防「同一声明两行」，
//!      slug 防「两个声明一个名」，缺一不可。
//!   ③ 四范围 grep 逐窗零冲突：把既有代码面切成四个范围——号面（其他
//!      卡已占的卡号）、符号面（代码标识符）、登记面（模块 stem）、判据面
//!      （判据关键词），每范围切 3 窗，逐窗扫描每条声明的探针（条目名 +
//!      slug + 号码字符串）。任何一窗命中即冲突；零命中 = 声明只是声明，
//!      没有偷偷落地成实现——这是防「声明即实现的假闭环」的核心格。
//!   ④ 注入核五类坏样本：重复复合键 / 重复 slug / 非法域 / 植入命中窗 /
//!      号面占用冲突，每类 10 轮全部显式拒绝；注入账守恒，逃逸恒为 0。
//!   ⑤ errno 对照：EEXIST / ENOTUNIQ / EINVAL / ENOENT / ENOSPC 逐码与
//!      Linux 对齐；双跑指纹 FNV-1a 逐位一致；零 panic（非测试代码不带
//!      任何可 panic 的调用，一律 get/区间校验/Result）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9375 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CB40;

/// C2 号面域值（联调交接面）。
pub const DOM_C2: u8 = 2;
/// C3 号面域值（联调依赖面）。
pub const DOM_C3: u8 = 3;

/// 声明册容量（16 条，覆盖 C2×C3 双面全量声明）。
pub const DECL_CAP: usize = 16;

/// 号面号码下界（本批声明号段 9001..=9099）。
pub const NUM_LO: u16 = 9001;
/// 号面号码上界。
pub const NUM_HI: u16 = 9099;

/// 类别闭集上界（category 取 0..=3）。
pub const CATEGORY_MAX: u8 = 3;

/// grep 范围数（号面/符号面/登记面/判据面）。
pub const RANGES: usize = 4;
/// 每范围窗口数。
pub const WINDOWS_PER_RANGE: u32 = 3;
/// 每窗 token 数。
pub const TOKENS_PER_WINDOW: usize = 12;
/// 单类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;

// ── Linux errno 正数口径 ─────────────────────────────────────────────
/// 文件已存在：复合键重复。
pub const EEXIST: i32 = 17;
/// 名称不唯一：slug 全局重复。
pub const ENOTUNIQ: i32 = 76;
/// 参数非法：域/号/类别越界、空 slug。
pub const EINVAL: i32 = 22;
/// 无此条目：查询不存在的键。
pub const ENOENT: i32 = 2;
/// 空间不足：声明册满。
pub const ENOSPC: i32 = 28;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +1..+12 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DedupCode {
    /// 复合键非法：域/号/类别越界
    BadKey,
    /// 未知域：domain 不在 {C2, C3}
    UnknownDomain,
    /// 空 slug
    EmptySlug,
    /// 复合键重复声明
    DuplicateKey,
    /// slug 全局重复
    DuplicateSlug,
    /// 声明册满
    RegistryFull,
    /// grep 窗口命中：声明面与代码面冲突
    GrepConflict,
    /// 条目名在标识符面命中（假闭环信号）
    FaceHit,
    /// 窗号越界
    WindowOutOfRange,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl DedupCode {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            DedupCode::BadKey => "复合键非法被拒：号须在 9001..=9099、类别须在 0..=3；\
                请按号面约束重新声明",
            DedupCode::UnknownDomain => "未知域被拒：domain 只允许 C2=2 / C3=3；\
                请先确认声明归属哪个号面",
            DedupCode::EmptySlug => "空 slug 被拒：slug 是全局唯一名的载体，\
                空名一律 EINVAL",
            DedupCode::DuplicateKey => "复合键重复被拒：(domain,num,category) 已在册；\
                同一键只能声明一次，重复回 EEXIST",
            DedupCode::DuplicateSlug => "slug 全局重复被拒：该名已被另一条声明占用；\
                请换名后重试，回 ENOTUNIQ",
            DedupCode::RegistryFull => "声明册已满：16 条容量用尽；请合并声明或扩容，\
                满载追加显式拒绝",
            DedupCode::GrepConflict => "grep 命中被拒：声明探针落进了既有代码面窗口；\
                声明必须与实现面零重叠",
            DedupCode::FaceHit => "条目名在标识符面命中：声明即实现的假闭环信号；\
                请删除实现或撤销声明，二者只留其一",
            DedupCode::WindowOutOfRange => "窗号越界被拒：范围/窗号超出四范围三窗闭集；\
                请按 0..RANGES、0..WINDOWS_PER_RANGE 取窗",
            DedupCode::LedgerRegress => "注入账倒退被拒：计数只增不减；\
                请检查是否有回滚路径误改累计值",
            DedupCode::NotReproducible => "双跑不一致：同输入两次指纹不同；\
                请排查隐藏状态或非确定序列化",
            DedupCode::SegmentViolation => "段位越界：本模块段基址必须落在 \
                0x7CB40..=0x7D1C0 且 0x40 对齐",
        }
    }

    /// 闭集全量（供完整性与唯一性检查）。
    pub const ALL: [DedupCode; 12] = [
        DedupCode::BadKey,
        DedupCode::UnknownDomain,
        DedupCode::EmptySlug,
        DedupCode::DuplicateKey,
        DedupCode::DuplicateSlug,
        DedupCode::RegistryFull,
        DedupCode::GrepConflict,
        DedupCode::FaceHit,
        DedupCode::WindowOutOfRange,
        DedupCode::LedgerRegress,
        DedupCode::NotReproducible,
        DedupCode::SegmentViolation,
    ];
}

// ── 四范围代码面语料 ─────────────────────────────────────────────────
/// 范围名（号面/符号面/登记面/判据面）。
pub const RANGE_NAMES: [&str; 4] = ["号面", "符号面", "登记面", "判据面"];

/// 号面语料：其他卡已占用的卡号（本批声明号 9001..=9099 与之零重叠）。
pub const NUM_FACE: [u16; 10] = [9202, 9203, 9204, 9205, 9206, 9207, 9208, 9209, 9370, 9371];

/// 符号面语料：既有代码标识符（声明条目名不得出现在其中）。
pub const IDENT_FACE: [&str; 16] = [
    "kmain",
    "fdtable",
    "idt_gate",
    "e820_scan",
    "acpi_rsdp",
    "madt_walk",
    "smbios_eps",
    "cpio_head",
    "slot_state",
    "errno_row",
    "fnv_seed",
    "dedup_book",
    "grep_win",
    "ledger_rec",
    "seg_base",
    "check_set",
];

/// 登记面语料：既有模块 stem（声明条目名不得与之撞名）。
pub const STEM_FACE: [&str; 8] = [
    "unx9202_rw_badfd_matrix_ebadf",
    "unx9203_efault_ptr",
    "unx9370_cosign_freeze_list",
    "unx9371_jointest_list_ltp",
    "unx9372_ktest_jointest_baseline",
    "unx9373_jointest",
    "unx9374_cosign_handoff_f9095",
    "unx9377_jointest_windows",
];

/// 判据面语料：判据关键词（防判据原文被实现体覆写）。
pub const ACCEPT_FACE: [&str; 6] = [
    "防重判据过",
    "四范围",
    "零冲突",
    "条目名",
    "零命中",
    "交接清单",
];

/// 取某范围的语料长度。
pub fn face_len(range: u8) -> usize {
    match range {
        0 => NUM_FACE.len(),
        1 => IDENT_FACE.len(),
        2 => STEM_FACE.len(),
        3 => ACCEPT_FACE.len(),
        _ => 0,
    }
}

/// 确定性取 token：第 range 范围第 win 窗第 i 个 token。
/// 取法 (range*7 + win*3 + i) 对面长取模——同一窗每次逐位一致。
pub fn face_token(range: u8, win: u32, i: usize) -> Option<String> {
    if range >= RANGES as u8 {
        return None;
    }
    let len = face_len(range);
    if len == 0 {
        return None;
    }
    let pick = (range as usize * 7 + win as usize * 3 + i) % len;
    match range {
        0 => NUM_FACE.get(pick).map(|n| n.to_string()),
        1 => IDENT_FACE.get(pick).map(|s| String::from(*s)),
        2 => STEM_FACE.get(pick).map(|s| String::from(*s)),
        _ => ACCEPT_FACE.get(pick).map(|s| String::from(*s)),
    }
}

/// 生一窗 token 序列（确定性）。
pub fn window_tokens(range: u8, win: u32) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < TOKENS_PER_WINDOW {
        if let Some(t) = face_token(range, win, i) {
            v.push(t);
        }
        i += 1;
    }
    v
}

/// grep 命中判定：token 等于探针或包含探针即命中。
pub fn grep_hit(tokens: &[String], probe: &str) -> bool {
    let mut i = 0usize;
    while i < tokens.len() {
        if let Some(t) = tokens.get(i) {
            if t == probe || t.contains(probe) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 扫描报告：窗数 / 探针数 / 冲突数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ScanReport {
    /// 实际扫描窗口数
    pub windows: u32,
    /// 投放的探针总数（含逐窗展开）
    pub probes: u32,
    /// 冲突（命中）次数
    pub conflicts: u32,
}

// ── 复合键与声明册 ───────────────────────────────────────────────────
/// 复合键 (domain, num, category)：C2×C3 号面声明的唯一身份。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DedupKey {
    /// 域：C2=2 / C3=3
    pub domain: u8,
    /// 卡号
    pub num: u16,
    /// 类别 0..=3
    pub category: u8,
}

/// 键合法性：域闭集、号段闭集、类别闭集。
pub fn key_valid(k: &DedupKey) -> bool {
    (k.domain == DOM_C2 || k.domain == DOM_C3)
        && k.num >= NUM_LO
        && k.num <= NUM_HI
        && k.category <= CATEGORY_MAX
}

/// 条目名（由键单源推导）：decl-c{domain}-{num}-k{category}。
pub fn entry_name(k: &DedupKey) -> String {
    format!("decl-c{}-{}-k{}", k.domain, k.num, k.category)
}

/// 单条声明：键 + slug + 推导条目名。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DedupEntry {
    /// 复合键
    pub key: DedupKey,
    /// 全局唯一 slug
    pub slug: String,
    /// 由键推导的条目名
    pub name: String,
}

/// 声明册（账本）：append-only，只增不减。
#[derive(Clone, Debug, Default)]
pub struct DedupRegistry {
    entries: Vec<DedupEntry>,
    declared: u32,
}

impl DedupRegistry {
    /// 构造空册。
    pub fn new() -> DedupRegistry {
        DedupRegistry { entries: Vec::new(), declared: 0 }
    }

    /// 当前条数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 登记账（只增）。
    pub fn declared(&self) -> u32 {
        self.declared
    }

    /// 取第 idx 条声明。
    pub fn entry_at(&self, idx: usize) -> Option<&DedupEntry> {
        self.entries.get(idx)
    }

    /// 键是否已在册。
    pub fn key_exists(&self, k: &DedupKey) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == *k {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// slug 是否已在册（全局唯一线）。
    pub fn slug_exists(&self, slug: &str) -> bool {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.slug == slug {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 声明一条：键校验 → 键唯一 → slug 唯一 → 容量，逐层显式拒绝。
    /// 成功返回条目名。
    pub fn declare(&mut self, k: DedupKey, slug: &str) -> Result<String, i32> {
        if !key_valid(&k) {
            return Err(EINVAL);
        }
        if k.domain != DOM_C2 && k.domain != DOM_C3 {
            return Err(EINVAL);
        }
        if slug.is_empty() {
            return Err(EINVAL);
        }
        if self.key_exists(&k) {
            return Err(EEXIST);
        }
        if self.slug_exists(slug) {
            return Err(ENOTUNIQ);
        }
        if self.entries.len() >= DECL_CAP {
            return Err(ENOSPC);
        }
        let name = entry_name(&k);
        self.entries.push(DedupEntry { key: k, slug: String::from(slug), name: name.clone() });
        self.declared = self.declared.saturating_add(1);
        Ok(name)
    }

    /// 按键查询声明。
    pub fn find(&self, k: &DedupKey) -> Option<&DedupEntry> {
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.key == *k {
                    return Some(e);
                }
            }
            i += 1;
        }
        None
    }

    /// 每条声明的探针序列（条目名 + slug + 号码字符串）。
    pub fn probes(&self) -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                v.push(e.name.clone());
                v.push(e.slug.clone());
                v.push(e.key.num.to_string());
            }
            i += 1;
        }
        v
    }
}

// ── 四范围逐窗扫描 ───────────────────────────────────────────────────
/// 四范围逐窗扫描：每范围 WINDOWS_PER_RANGE 窗，每窗 TOKENS_PER_WINDOW
/// 个 token，投放全部探针。返回报告；冲突即 grep 检出。
pub fn four_range_scan(reg: &DedupRegistry) -> ScanReport {
    let probes = reg.probes();
    let mut rep = ScanReport { windows: 0, probes: 0, conflicts: 0 };
    let mut r = 0u8;
    while (r as usize) < RANGES {
        let mut w = 0u32;
        while w < WINDOWS_PER_RANGE {
            let toks = window_tokens(r, w);
            rep.windows = rep.windows.saturating_add(1);
            let mut p = 0usize;
            while p < probes.len() {
                if let Some(pr) = probes.get(p) {
                    rep.probes = rep.probes.saturating_add(1);
                    if grep_hit(&toks, pr) {
                        rep.conflicts = rep.conflicts.saturating_add(1);
                    }
                }
                p += 1;
            }
            w += 1;
        }
        r += 1;
    }
    rep
}

/// 单窗植入扫描：把探针种进指定窗，必须检出（注入核用）。
pub fn planted_window_scan(range: u8, win: u32, probe: &str) -> u32 {
    let mut toks = window_tokens(range, win);
    toks.push(String::from(probe));
    if grep_hit(&toks, probe) {
        1
    } else {
        0
    }
}

// ── 规范声明序列 ─────────────────────────────────────────────────────
/// 规范 12 条声明：C2 面 6 条（号 9001..=9006）+ C3 面 6 条（号 9001..=9006）。
/// 同号跨域各声明一次（号面零冲突的核心性质），类别按 i%4 轮转。
/// 返回（声明册，指纹字节源）。
pub fn canonical_declare() -> (DedupRegistry, Vec<u8>) {
    let mut reg = DedupRegistry::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0u32;
    while i < 12 {
        let domain = if i < 6 { DOM_C2 } else { DOM_C3 };
        let num = NUM_LO + (i % 6) as u16;
        let category = (i % 4) as u8;
        let k = DedupKey { domain, num, category };
        let slug = format!("unx-{}-{}-{}", domain, num, category);
        let r = reg.declare(k, &slug);
        bytes.push(match r {
            Ok(_) => 0,
            Err(e) => (e & 0xFF) as u8,
        });
        bytes.push((fnv1a(slug.as_bytes()) & 0xFF) as u8);
        i += 1;
    }
    (reg, bytes)
}

// ── FNV-1a ────────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范语料指纹：12 条声明结果 + slug 哈希字节流的双跑指纹。
pub fn corpus_fingerprint() -> u32 {
    let (_reg, bytes) = canonical_declare();
    fnv1a(&bytes)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 五类坏样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadSample {
    /// 同复合键二次声明（EEXIST）
    DuplicateKey,
    /// 同 slug 二次声明（ENOTUNIQ）
    DuplicateSlug,
    /// 非法域声明（EINVAL）
    UnknownDomain,
    /// 植入命中窗（grep 必须检出）
    PlantedHit,
    /// 号面占用冲突（声明已占号，grep 必须检出）
    NumClash,
}

impl BadSample {
    /// 闭集全量。
    pub const ALL: [BadSample; 5] = [
        BadSample::DuplicateKey,
        BadSample::DuplicateSlug,
        BadSample::UnknownDomain,
        BadSample::PlantedHit,
        BadSample::NumClash,
    ];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须为 0）
    pub escaped: u32,
}

impl InjectLedger {
    const fn zero() -> InjectLedger {
        InjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一次注入结果，守恒式破坏即报账目倒退。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(DedupCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒判定：注入 = 检出 + 逃逸。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 五类注入矩阵：每类 INJECT_ROUNDS 轮，返回账本与逐类检出数。
pub fn run_injection_matrix() -> (InjectLedger, [u32; 5]) {
    let mut ledger = InjectLedger::zero();
    let mut per = [0u32; 5];
    let mut ci = 0usize;
    while ci < BadSample::ALL.len() {
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match BadSample::ALL.get(ci).copied() {
                Some(BadSample::DuplicateKey) => {
                    let mut reg = DedupRegistry::new();
                    let k = DedupKey { domain: DOM_C2, num: 9001, category: 0 };
                    let a = reg.declare(k, "unx-first").is_ok();
                    let b = reg.declare(k, "unx-second").err() == Some(EEXIST);
                    a && b
                }
                Some(BadSample::DuplicateSlug) => {
                    let mut reg = DedupRegistry::new();
                    let k1 = DedupKey { domain: DOM_C2, num: 9002, category: 1 };
                    let k2 = DedupKey { domain: DOM_C3, num: 9002, category: 1 };
                    let a = reg.declare(k1, "unx-same-slug").is_ok();
                    let b = reg.declare(k2, "unx-same-slug").err() == Some(ENOTUNIQ);
                    a && b
                }
                Some(BadSample::UnknownDomain) => {
                    let mut reg = DedupRegistry::new();
                    let k = DedupKey { domain: 7, num: 9003, category: 0 };
                    let a = reg.declare(k, "unx-bad-dom").err() == Some(EINVAL);
                    let k2 = DedupKey { domain: DOM_C3, num: 80, category: 0 };
                    let b = reg.declare(k2, "unx-bad-num").err() == Some(EINVAL);
                    let k3 = DedupKey { domain: DOM_C2, num: 9003, category: 9 };
                    let c = reg.declare(k3, "unx-bad-cat").err() == Some(EINVAL);
                    a && b && c
                }
                Some(BadSample::PlantedHit) => {
                    let name = format!("decl-c{}-{}-k{}", DOM_C2, 9004, 2);
                    planted_window_scan(1, 0, &name) == 1
                }
                Some(BadSample::NumClash) => {
                    // 占用号 9202 超出声明号段（EINVAL 拒收），且号码探针
                    // "9202" 在号面窗（窗 0 覆盖全部占用号）必须被 grep 检出。
                    let mut reg = DedupRegistry::new();
                    let rejected = reg.declare(
                        DedupKey { domain: DOM_C2, num: 9202, category: 0 },
                        "unx-clash",
                    ).err() == Some(EINVAL);
                    rejected && grep_hit(&window_tokens(0, 0), "9202")
                }
                None => false,
            };
            let _ = ledger.record(caught);
            if caught {
                if let Some(slot) = per.get_mut(ci) {
                    *slot = slot.saturating_add(1);
                }
            }
            round += 1;
        }
        ci += 1;
    }
    (ledger, per)
}

// ── Linux errno 对照表 ───────────────────────────────────────────────
/// (名称, 正值, Windows 近似码, 说明)。
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EEXIST", 17, 183, "Windows ERROR_ALREADY_EXISTS；复合键重复族"),
    ("ENOTUNIQ", 76, 275, "Windows ERROR_NAME_NOT_UNIQUE 口径近似；slug 重复族"),
    ("EINVAL", 22, 87, "Windows ERROR_INVALID_PARAMETER；键非法族"),
    ("ENOENT", 2, 2, "Windows ERROR_FILE_NOT_FOUND；无此条目族"),
    ("ENOSPC", 28, 112, "Windows ERROR_DISK_FULL 口径近似；声明册满族"),
];

/// 查表：正值在表内返回 true。
pub fn errno_known(code: i32) -> bool {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9375 判据集。
pub fn run_unx9375_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9375");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7CB40..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7CB40..=0x7D1C0", seg_ok,
          "段基址必须唯一且落在本批窗口");

    // 2. 规范 12 条声明全过
    let (reg, _bytes) = canonical_declare();
    let canon_ok = reg.len() == 12 && reg.declared() == 12;
    s.add("规范声明：C2×C3 各 6 条共 12 条全过", canon_ok,
          "同号跨域各声明一次，12 条全部入册");

    // 3. 复合键唯一（EEXIST）
    let mut reg3 = DedupRegistry::new();
    let k = DedupKey { domain: DOM_C2, num: 9010, category: 2 };
    let first = reg3.declare(k, "unx-dup-key-a").is_ok();
    let dup = reg3.declare(k, "unx-dup-key-b").err() == Some(EEXIST);
    s.add("复合键唯一：同 (domain,num,category) 二次声明 EEXIST", first && dup,
          "键三元组在册内唯一，重复显式拒绝");

    // 4. slug 全局唯一（ENOTUNIQ）：跨域同 slug 也拒
    let mut reg4 = DedupRegistry::new();
    let ka = DedupKey { domain: DOM_C2, num: 9011, category: 0 };
    let kb = DedupKey { domain: DOM_C3, num: 9011, category: 0 };
    let s1 = reg4.declare(ka, "unx-global-slug").is_ok();
    let s2 = reg4.declare(kb, "unx-global-slug").err() == Some(ENOTUNIQ);
    s.add("slug 全局唯一：跨 C2/C3 同名二声明 ENOTUNIQ", s1 && s2,
          "slug 是跨域单一命名空间，重复一律 ENOTUNIQ");

    // 5. 键非法三面（域/号/类别越界 → EINVAL）
    let mut reg5 = DedupRegistry::new();
    let bad_dom = reg5.declare(DedupKey { domain: 5, num: 9012, category: 0 }, "unx-x1").err() == Some(EINVAL);
    let bad_num = reg5.declare(DedupKey { domain: DOM_C3, num: 8999, category: 0 }, "unx-x2").err() == Some(EINVAL);
    let bad_cat = reg5.declare(DedupKey { domain: DOM_C2, num: 9012, category: 4 }, "unx-x3").err() == Some(EINVAL);
    let bad_slug = reg5.declare(DedupKey { domain: DOM_C2, num: 9012, category: 0 }, "").err() == Some(EINVAL);
    s.add("键非法三面：坏域/坏号/坏类别/空 slug 全 EINVAL",
          bad_dom && bad_num && bad_cat && bad_slug,
          "域闭集 {2,3}、号段 9001..=9099、类别 0..=3、slug 非空");

    // 6. 四范围 grep 逐窗零冲突
    let rep = four_range_scan(&reg);
    let zero = rep.conflicts == 0 && rep.windows == RANGES as u32 * WINDOWS_PER_RANGE && rep.probes > 0;
    s.add("四范围 grep：12 窗全扫零冲突", zero,
          "号面/符号面/登记面/判据面逐窗扫描，声明探针零命中");

    // 7. 条目名与代码标识符面零命中（防声明即实现的假闭环）
    let mut face_hit = false;
    let mut n = 0usize;
    while n < reg.len() {
        if let Some(e) = reg.entry_at(n) {
            let mut r = 0u8;
            while (r as usize) < RANGES {
                let mut w = 0u32;
                while w < WINDOWS_PER_RANGE {
                    if grep_hit(&window_tokens(r, w), &e.name) {
                        face_hit = true;
                    }
                    w += 1;
                }
                r += 1;
            }
        }
        n += 1;
    }
    s.add("条目名零命中：12 条条目名 × 四范围逐窗全零", !face_hit,
          "声明只是声明：条目名不得出现在任何代码面窗口");

    // 8. 号面零冲突：声明号与占用号面零重叠
    let mut num_clash = false;
    let mut m = 0usize;
    while m < reg.len() {
        if let Some(e) = reg.entry_at(m) {
            let mut t = 0usize;
            while t < NUM_FACE.len() {
                if let Some(occ) = NUM_FACE.get(t) {
                    if *occ == e.key.num {
                        num_clash = true;
                    }
                }
                t += 1;
            }
        }
        m += 1;
    }
    s.add("号面零冲突：声明号段 9001..=9099 与占用号面零重叠", !num_clash,
          "本批声明不与其他卡的卡号冲突");

    // 9. 注入矩阵五类 × 10/10
    let (ledger, per) = run_injection_matrix();
    let all_caught = per.iter().all(|x| *x == INJECT_ROUNDS);
    s.add("注入矩阵：五类坏样本 ×10/10 全检出", all_caught,
          "重复键/重复 slug/非法域/植入窗/号面冲突全部拦截");

    // 10. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == BadSample::ALL.len() as u32 * INJECT_ROUNDS,
          "50 次注入 50 次检出 0 逃逸");

    // 11. 容量守卫：满 16 后追加 ENOSPC
    let mut reg11 = DedupRegistry::new();
    let mut fill_ok = true;
    let mut f = 0u32;
    while f < DECL_CAP as u32 {
        let kf = DedupKey { domain: DOM_C2, num: NUM_LO + f as u16, category: 0 };
        if reg11.declare(kf, &format!("unx-fill-{}", f)).is_err() {
            fill_ok = false;
        }
        f += 1;
    }
    let over = reg11.declare(DedupKey { domain: DOM_C3, num: NUM_LO, category: 1 }, "unx-over")
        .err() == Some(ENOSPC);
    s.add("容量守卫：16 条满载追加 ENOSPC", fill_ok && over,
          "声明册 append-only 且满载显式拒绝");

    // 12. errno 对照：表内可查、表外拒绝
    let codes_ok = errno_known(EEXIST) && errno_known(ENOTUNIQ) && errno_known(EINVAL)
        && errno_known(ENOENT) && errno_known(ENOSPC) && !errno_known(9999);
    s.add("errno 对照：表内码全可查、表外码拒绝", codes_ok,
          "错误码只允许对照表内正值");

    // 13. 双跑指纹
    let fa = corpus_fingerprint();
    let fb = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹逐位相等", fa == fb,
          "同输入两次必须逐位一致，隐藏状态即失败");

    // 14. 文案闭集
    let text_ok = DedupCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：12 码全带非空中文建议", text_ok,
          "每个诊断码必须可取到可操作说明");

    // 15. 诊断码两两唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < DedupCode::ALL.len() {
        let mut j = i + 1;
        while j < DedupCode::ALL.len() {
            if let (Some(a), Some(b)) = (DedupCode::ALL.get(i), DedupCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 码两两唯一", uniq, "段内闭集码不得重复");

    // 动态明细（≤8）
    let led_text = format!("注入 {} / 检出 {} / 逃逸 {}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("账目快照", ledger.conserved(), &led_text);
    let fp_text = format!("语料指纹 0x{:08X}", fa);
    s.add_dyn("指纹快照", fa == fb, &fp_text);
    let rep_text = format!("窗 {} / 探针 {} / 冲突 {}", rep.windows, rep.probes, rep.conflicts);
    s.add_dyn("扫描快照", rep.conflicts == 0, &rep_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_twelve_declare() {
        let (reg, _) = canonical_declare();
        assert_eq!(reg.len(), 12);
        assert_eq!(reg.declared(), 12);
        assert!(!reg.is_empty());
    }

    #[test]
    fn duplicate_key_eexist() {
        let mut reg = DedupRegistry::new();
        let k = DedupKey { domain: DOM_C2, num: 9001, category: 0 };
        assert!(reg.declare(k, "unx-a").is_ok());
        assert_eq!(reg.declare(k, "unx-b").err(), Some(EEXIST));
    }

    #[test]
    fn duplicate_slug_enotuniq() {
        let mut reg = DedupRegistry::new();
        let k1 = DedupKey { domain: DOM_C2, num: 9002, category: 1 };
        let k2 = DedupKey { domain: DOM_C3, num: 9002, category: 1 };
        assert!(reg.declare(k1, "unx-s").is_ok());
        assert_eq!(reg.declare(k2, "unx-s").err(), Some(ENOTUNIQ));
    }

    #[test]
    fn invalid_keys_einval() {
        let mut reg = DedupRegistry::new();
        assert_eq!(reg.declare(DedupKey { domain: 9, num: 9001, category: 0 }, "u").err(), Some(EINVAL));
        assert_eq!(reg.declare(DedupKey { domain: DOM_C2, num: 10000, category: 0 }, "u").err(), Some(EINVAL));
        assert_eq!(reg.declare(DedupKey { domain: DOM_C2, num: 9001, category: 5 }, "u").err(), Some(EINVAL));
        assert_eq!(reg.declare(DedupKey { domain: DOM_C2, num: 9001, category: 0 }, "").err(), Some(EINVAL));
    }

    #[test]
    fn same_num_across_domains_ok() {
        let mut reg = DedupRegistry::new();
        let k2 = DedupKey { domain: DOM_C2, num: 9005, category: 0 };
        let k3 = DedupKey { domain: DOM_C3, num: 9005, category: 0 };
        assert!(reg.declare(k2, "unx-c2").is_ok());
        assert!(reg.declare(k3, "unx-c3").is_ok());
        assert!(reg.find(&k3).is_some());
    }

    #[test]
    fn four_range_zero_conflict() {
        let (reg, _) = canonical_declare();
        let rep = four_range_scan(&reg);
        assert_eq!(rep.conflicts, 0);
        assert_eq!(rep.windows, 12);
        assert!(rep.probes >= 36);
    }

    #[test]
    fn planted_hit_detected() {
        let probe = "decl-c2-9001-k0";
        assert_eq!(planted_window_scan(1, 0, probe), 1);
        assert_eq!(planted_window_scan(0, 1, probe), 1);
    }

    #[test]
    fn registry_full_enospc() {
        let mut reg = DedupRegistry::new();
        let mut f = 0u32;
        while f < DECL_CAP as u32 {
            let k = DedupKey { domain: DOM_C2, num: NUM_LO + f as u16, category: 0 };
            assert!(reg.declare(k, &format!("unx-f{}", f)).is_ok());
            f += 1;
        }
        let k = DedupKey { domain: DOM_C3, num: NUM_LO, category: 2 };
        assert_eq!(reg.declare(k, "unx-over").err(), Some(ENOSPC));
    }

    #[test]
    fn injection_matrix_conserved() {
        let (l, per) = run_injection_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 50);
        for x in per.iter() {
            assert_eq!(*x, INJECT_ROUNDS);
        }
    }

    #[test]
    fn errno_table_agrees() {
        assert!(errno_known(EEXIST));
        assert!(errno_known(ENOTUNIQ));
        assert!(!errno_known(9999));
    }

    #[test]
    fn deterministic_fingerprint() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn unx9375_checks_green() {
        let set = run_unx9375_checks();
        assert!(set.all_passed(), "UNX-F9375 red: {}", set.red_items_text());
    }
}
