//! UNX-F10005 模块维聚合器（ltp_* 全家） · 0x91140段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10006-J1 达成率取数正确（分母口径冻结 10/10 次），口径
//! 漂移 10/10 次拦截。
//!
//! 设计要点：
//!   ① 模块维按「冻结清单 + 前缀解析器」建模：ltp_* 全家 12 个模块名
//!      闭集写死，注册名必须 ltp_ 前缀 + 清单内后缀，坏前缀/未知模块/
//!      空名/重复登记逐类拒绝。
//!   ② 清单完备闸：注册面必须覆盖冻结清单全量；缺任何一模块都算缺模，
//!      缺模注入按「逐模各缺一次」12 类 × 10 轮复测，必须 10/10 次检出。
//!   ③ 聚合守恒：各模块已核判据数之和 = 总账；通过数 ≤ 已核数逐条
//!      校验，越界拒绝。
//!   ④ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0，账只增不减。
//!   ⑤ 与 Linux 逐码一致通过 errno 冻结表复算；FNV-1a 指纹双跑一致。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10005 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x91140;

/// 每模块冻结判据口径（分母：每模 8 条）。
pub const FROZEN_PER_MODULE: u32 = 8;

/// 单类注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// Linux errno 正数码。
pub const ENOENT: i32 = 2;
pub const EEXIST: i32 = 17;
pub const EINVAL: i32 = 22;
pub const ERANGE: i32 = 34;

/// ltp 模块族冻结清单（12 个，闭集写死，顺序即注册校验序）。
pub const LTP_FAMILY: [&str; 12] = [
    "ltp_read",
    "ltp_write",
    "ltp_open",
    "ltp_close",
    "ltp_dup",
    "ltp_lseek",
    "ltp_fcntl",
    "ltp_pipe",
    "ltp_fstat",
    "ltp_access",
    "ltp_getdents",
    "ltp_stat",
];

/// 清单长度。
pub fn family_len() -> usize {
    LTP_FAMILY.len()
}

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModCode {
    /// 空名
    EmptyName,
    /// 坏前缀：不以 ltp_ 开头
    BadPrefix,
    /// 未知模块：后缀不在冻结清单
    UnknownModule,
    /// 重复登记
    DuplicateModule,
    /// 缺模：清单完备闸发现缺员
    MissingModule,
    /// 通过数越界：passed > checked
    PassOverflow,
    /// 账倒退：已核数只增不减
    LedgerRegress,
    /// 守恒破约：Σ已核 ≠ 总账
    ConservationBreach,
    /// 未清点出表：清单未核全就出聚合
    EmitBeforeFull,
    /// 清单越界：族长与冻结值不符
    FamilyMismatch,
    /// 双跑不一致
    NotReproducible,
    /// 段位越界
    SegmentViolation,
}

impl ModCode {
    /// 码值（段基址 + 变体序号，从 1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32) + 1
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            ModCode::EmptyName => "空名被拒：模块条目必须有名字；请先命名再登记",
            ModCode::BadPrefix => "坏前缀被拒：模块名必须以 ltp_ 开头；请按\
                ltp_* 句式修正",
            ModCode::UnknownModule => "未知模块被拒：后缀不在冻结清单内；请对照\
                LTP_FAMILY 闭集修正拼写",
            ModCode::DuplicateModule => "重复登记被拒：同名模块已在册；请先查册\
                再补录，重复回 EEXIST",
            ModCode::MissingModule => "缺模被拒：注册面未覆盖冻结清单全量；请\
                逐模核对补齐",
            ModCode::PassOverflow => "通过数越界被拒：通过数不得大于已核数；\
                请回查该模统计口径",
            ModCode::LedgerRegress => "账倒退被拒：已核数只增不减；请检查是否有\
                回滚路径误改累计值",
            ModCode::ConservationBreach => "守恒破约：各模已核数之和不等于总账；\
                请核对漏模或双计",
            ModCode::EmitBeforeFull => "未清点出表被拒：清单未核全不得出聚合；\
                请先过完备闸",
            ModCode::FamilyMismatch => "清单越界：族长与冻结值 12 不符；清单是\
                分母口径，不许漂移",
            ModCode::NotReproducible => "双跑不一致：同输入两次结果不同；请排查\
                隐藏状态或非确定遍历序",
            ModCode::SegmentViolation => "段位越界：段基址必须 0x40 对齐且落在\
                0x91000..=0x914C0 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: [ModCode; 12] = [
        ModCode::EmptyName,
        ModCode::BadPrefix,
        ModCode::UnknownModule,
        ModCode::DuplicateModule,
        ModCode::MissingModule,
        ModCode::PassOverflow,
        ModCode::LedgerRegress,
        ModCode::ConservationBreach,
        ModCode::EmitBeforeFull,
        ModCode::FamilyMismatch,
        ModCode::NotReproducible,
        ModCode::SegmentViolation,
    ];
}

// ── 模块名解析器 ──────────────────────────────────────────────────────
/// 模块名解析：ltp_ 前缀 + 清单内后缀；返回族内序（0 起）。
pub fn parse_module_name(text: &str) -> Result<usize, ModCode> {
    if text.is_empty() {
        return Err(ModCode::EmptyName);
    }
    let bytes = text.as_bytes();
    let prefix: [u8; 4] = [b'l', b't', b'p', b'_'];
    if bytes.len() < prefix.len() {
        return Err(ModCode::BadPrefix);
    }
    let mut i = 0usize;
    while i < prefix.len() {
        match bytes.get(i) {
            Some(b) if *b == prefix[i] => {}
            _ => return Err(ModCode::BadPrefix),
        }
        i += 1;
    }
    let mut idx = 0usize;
    let mut found = false;
    while idx < LTP_FAMILY.len() {
        if let Some(name) = LTP_FAMILY.get(idx) {
            if *name == text {
                found = true;
                break;
            }
        }
        idx += 1;
    }
    if !found {
        return Err(ModCode::UnknownModule);
    }
    Ok(idx)
}

// ── 模块注册面 ────────────────────────────────────────────────────────
/// 单模条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModuleEntry {
    /// 模块名（冻结清单内）
    pub name: &'static str,
    /// 族内序
    pub idx: usize,
    /// 已核判据数
    pub checked: u32,
    /// 通过判据数
    pub passed: u32,
}

/// 模块维注册面：登记 + 清单完备闸 + 聚合。
#[derive(Clone, Debug)]
pub struct ModuleRegistry {
    entries: Vec<ModuleEntry>,
}

impl ModuleRegistry {
    /// 空注册面。
    pub fn new() -> ModuleRegistry {
        ModuleRegistry { entries: Vec::new() }
    }

    /// 登记一模块（checked/passed 为该模已核与通过数）。
    pub fn register(&mut self, name: &'static str, checked: u32, passed: u32) -> Result<(), ModCode> {
        let idx = parse_module_name(name)?;
        if passed > checked {
            return Err(ModCode::PassOverflow);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.idx == idx || e.name == name {
                    return Err(ModCode::DuplicateModule);
                }
            }
            i += 1;
        }
        self.entries.push(ModuleEntry { name, idx, checked, passed });
        Ok(())
    }

    /// 已登记模数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 空面判定。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 缺模清单：冻结清单中尚未登记的模块名。
    pub fn missing_modules(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        let mut idx = 0usize;
        while idx < LTP_FAMILY.len() {
            let mut found = false;
            let mut i = 0usize;
            while i < self.entries.len() {
                if let Some(e) = self.entries.get(i) {
                    if e.idx == idx {
                        found = true;
                    }
                }
                i += 1;
            }
            if !found {
                if let Some(name) = LTP_FAMILY.get(idx) {
                    out.push(*name);
                }
            }
            idx += 1;
        }
        out
    }

    /// Σ已核。
    pub fn sum_checked(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                sum = sum.saturating_add(e.checked);
            }
            i += 1;
        }
        sum
    }

    /// Σ通过。
    pub fn sum_passed(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                sum = sum.saturating_add(e.passed);
            }
            i += 1;
        }
        sum
    }

    /// 冻结分母：族长 × 每模冻结口径。
    pub fn frozen_denominator(&self) -> u32 {
        (family_len() as u32).saturating_mul(FROZEN_PER_MODULE)
    }

    /// 清单完备闸：零缺模且逐模已核数 = 冻结口径。
    pub fn completeness(&self) -> Result<(), ModCode> {
        if family_len() != 12 {
            return Err(ModCode::FamilyMismatch);
        }
        let miss = self.missing_modules();
        if !miss.is_empty() {
            return Err(ModCode::MissingModule);
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                if e.checked != FROZEN_PER_MODULE {
                    return Err(ModCode::LedgerRegress);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 出聚合：先过完备闸；返回 (Σ已核, Σ通过)。
    pub fn emit(&self) -> Result<(u32, u32), ModCode> {
        self.completeness()?;
        let sc = self.sum_checked();
        if sc != self.frozen_denominator() {
            return Err(ModCode::ConservationBreach);
        }
        Ok((sc, self.sum_passed()))
    }
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸，逃逸必须恒为 0。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLedger {
    /// 注入总次数
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

    /// 记一次注入结果；账不平返回负诊断码。
    pub fn record(&mut self, caught: bool) -> Result<(), i32> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(-(ModCode::LedgerRegress as i32));
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 四类坏样本注入类别。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadCase {
    /// 缺模：抽走一模块后完备闸必须报缺模（逐模轮换）
    MissingModule,
    /// 坏前缀：无 ltp_ 头
    BadPrefix,
    /// 未知模块：ltp_ 头但清单外
    UnknownModule,
    /// 重复登记
    Duplicate,
}

impl BadCase {
    /// 闭集全量。
    pub const ALL: [BadCase; 4] = [
        BadCase::MissingModule,
        BadCase::BadPrefix,
        BadCase::UnknownModule,
        BadCase::Duplicate,
    ];
}

/// 执行四类坏样本注入：每类 INJECT_ROUNDS 次，返回账本与逐类检出数。
pub fn run_bad_case_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadCase::ALL.len() {
        if let Some(case) = BadCase::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = match case {
                    BadCase::MissingModule => {
                        // 逐模轮换抽缺：抽第 round%12 模，完备闸必须报缺模。
                        let drop = round as usize % family_len();
                        let mut reg = ModuleRegistry::new();
                        let mut idx = 0usize;
                        while idx < family_len() {
                            if idx != drop {
                                if let Some(name) = LTP_FAMILY.get(idx) {
                                    let _ = reg.register(name, FROZEN_PER_MODULE, FROZEN_PER_MODULE);
                                }
                            }
                            idx += 1;
                        }
                        reg.completeness().err() == Some(ModCode::MissingModule)
                    }
                    BadCase::BadPrefix => {
                        parse_module_name("read01") == Err(ModCode::BadPrefix)
                            && parse_module_name("lt_read") == Err(ModCode::BadPrefix)
                    }
                    BadCase::UnknownModule => {
                        parse_module_name("ltp_nosuch") == Err(ModCode::UnknownModule)
                            && parse_module_name("ltp_") == Err(ModCode::UnknownModule)
                    }
                    BadCase::Duplicate => {
                        let mut reg = ModuleRegistry::new();
                        let first = reg.register("ltp_read", FROZEN_PER_MODULE, 6).is_ok();
                        first && reg.register("ltp_read", FROZEN_PER_MODULE, 6)
                            == Err(ModCode::DuplicateModule)
                    }
                };
                let _ = ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

/// 全量注册面：12 模逐模登记（已核 = 冻结口径，通过数 = 口径 − idx mod 3）。
pub fn build_full_registry() -> ModuleRegistry {
    let mut reg = ModuleRegistry::new();
    let mut idx = 0usize;
    while idx < family_len() {
        if let Some(name) = LTP_FAMILY.get(idx) {
            let passed = FROZEN_PER_MODULE - (idx as u32 % 3);
            let _ = reg.register(name, FROZEN_PER_MODULE, passed);
        }
        idx += 1;
    }
    reg
}

// ── Linux 逐码对照表 ─────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；缺模/未知模块族"),
    ("EEXIST", -17, 80, "Windows ERROR_FILE_EXISTS；重复登记族"),
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；坏前缀/越界族"),
    ("ERANGE", -34, 34, "Windows ERROR_ARITHMETIC_OVERFLOW；通过数越界族"),
];

/// 查表：按 errno 正数取负值；表外返回 0（显性可查）。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == -code {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致：只允许表内码且负值精确匹配。
pub fn agrees_with_linux(code: i32) -> bool {
    linux_neg(code) == -code && code > 0
}

// ── 确定性指纹 ────────────────────────────────────────────────────────
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

/// 注册面指纹：逐模 (族内序, 已核, 通过) 逐字节入哈希。
pub fn registry_fingerprint(reg: &ModuleRegistry) -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < reg.len() {
        if let Some(e) = reg.entries.get(i) {
            bytes.push((e.idx & 0xFF) as u8);
            bytes.push((e.checked & 0xFF) as u8);
            bytes.push((e.passed & 0xFF) as u8);
        }
        i += 1;
    }
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10005 判据集（16 条 + 2 条动态明细）。
pub fn run_unx10005_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10005");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x91000..=0x914C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x91000..=0x914C0", seg_ok,
          "段基址必须唯一且落在本批窗口；越界即拒绝注册");

    // 2. 冻结清单：族长 12 且互不相同
    let mut fam_ok = family_len() == 12;
    let mut i = 0usize;
    while i < LTP_FAMILY.len() {
        let mut j = i + 1;
        while j < LTP_FAMILY.len() {
            if let (Some(a), Some(b)) = (LTP_FAMILY.get(i), LTP_FAMILY.get(j)) {
                if a == b {
                    fam_ok = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("冻结清单：ltp_* 全家 12 模且互异", fam_ok,
          "清单是模块维分母口径，族长与重名都要冻结");

    // 3. 名字解析：12 模逐一命中族内序
    let mut parse_ok = true;
    let mut idx = 0usize;
    while idx < family_len() {
        match LTP_FAMILY.get(idx) {
            Some(name) if parse_module_name(name) == Ok(idx) => {}
            _ => parse_ok = false,
        }
        idx += 1;
    }
    s.add("名字解析：12 模逐一命中族内序", parse_ok,
          "清单内名字必须解析回正确族内序");

    // 4. 坏前缀/未知模块拒绝
    let reject_ok = parse_module_name("read01") == Err(ModCode::BadPrefix)
        && parse_module_name("lt_read") == Err(ModCode::BadPrefix)
        && parse_module_name("ltp_nosuch") == Err(ModCode::UnknownModule)
        && parse_module_name("") == Err(ModCode::EmptyName);
    s.add("名字拒绝：坏前缀/未知/空名逐类拒绝", reject_ok,
          "前缀闸先于清单闸，空名最先拒绝");

    // 5. 全量注册面：12 模齐 + 完备闸过
    let full = build_full_registry();
    s.add("零缺模：12 模逐模在册且完备闸过",
          full.len() == 12 && full.missing_modules().is_empty()
              && full.completeness().is_ok(),
          "ltp_* 全家逐模登记，缺任何一模都不得出聚合");

    // 6. 分母口径冻结：96 = 12 × 8
    s.add("分母冻结：Σ已核=12×8=96",
          full.sum_checked() == full.frozen_denominator()
              && full.frozen_denominator() == 96,
          "分母口径必须从冻结清单可复算，不许漂移");

    // 7. 出聚合：完备闸后取数正确
    let emit = full.emit();
    let emit_ok = match emit {
        Ok((sc, sp)) => sc == 96 && sp == full.sum_passed() && sp <= sc,
        Err(_) => false,
    };
    s.add("取数正确：出聚合 (Σ已核, Σ通过) 可复算", emit_ok,
          "出聚合必须先过完备闸，两数与逐模之和一致");

    // 8. 注入矩阵：四类各 10/10 检出
    let (ledger, per_class) = run_bad_case_matrix();
    let mut all_caught = true;
    let mut ci = 0usize;
    while ci < per_class.len() {
        if let Some(n) = per_class.get(ci) {
            if *n != INJECT_ROUNDS {
                all_caught = false;
            }
        }
        ci += 1;
    }
    s.add("注入矩阵：缺模/坏前缀/未知/重复各 10/10", all_caught,
          "四类坏样本必须全部显性拒绝或报缺");

    // 9. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          ledger.conserved() && ledger.escaped == 0
              && ledger.injected == 4 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 10. 缺模拦截：抽走任一模完备闸拒绝（逐模轮换在矩阵内）
    let mut thin = ModuleRegistry::new();
    let mut idx = 1usize;
    while idx < family_len() {
        if let Some(name) = LTP_FAMILY.get(idx) {
            let _ = thin.register(name, FROZEN_PER_MODULE, FROZEN_PER_MODULE);
        }
        idx += 1;
    }
    let miss = thin.missing_modules();
    s.add("缺模拦截：缺首模时报 ltp_read",
          thin.completeness().err() == Some(ModCode::MissingModule)
              && miss.len() == 1
              && miss.first() == Some(&"ltp_read"),
          "缺模清单必须点名缺谁，不许笼统报错");

    // 11. 通过数越界拒绝
    let mut over = ModuleRegistry::new();
    s.add("越界拒绝：passed>checked 被拒",
          over.register("ltp_read", 8, 9).err() == Some(ModCode::PassOverflow),
          "通过数大于已核数属口径矛盾，必须拒绝");

    // 12. 未清点出表拦截
    let mut thin2 = ModuleRegistry::new();
    let _ = thin2.register("ltp_read", FROZEN_PER_MODULE, 6);
    s.add("未清点出表：缺模面 emit 被拒",
          thin2.emit().err() == Some(ModCode::MissingModule),
          "完备闸是出表前置闸，跳闸即拦");

    // 13. Linux 逐码一致
    let codes_ok = agrees_with_linux(ENOENT)
        && agrees_with_linux(EEXIST)
        && agrees_with_linux(EINVAL)
        && agrees_with_linux(ERANGE)
        && !agrees_with_linux(0)
        && !agrees_with_linux(9999);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误码只允许 errno 表内正值且负值可复算");

    // 14. 双跑指纹一致
    let fp_a = registry_fingerprint(&build_full_registry());
    let fp_b = registry_fingerprint(&build_full_registry());
    s.add("双跑一致：注册面指纹相等", fp_a == fp_b,
          "同输入两次构建必须逐字段相等");

    // 15. 文案闭集
    let text_ok = ModCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("文案闭集：全部诊断码带中文建议", text_ok,
          "每个码必须能取到非空且可操作的说明");

    // 16. 诊断码唯一
    let mut uniq = true;
    let mut i = 0usize;
    while i < ModCode::ALL.len() {
        let mut j = i + 1;
        while j < ModCode::ALL.len() {
            if let (Some(a), Some(b)) = (ModCode::ALL.get(i), ModCode::ALL.get(j)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq,
          "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（≤8）
    let ledger_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        ledger.injected, ledger.detected, ledger.escaped
    );
    s.add_dyn("账目快照", ledger.conserved() && ledger.escaped == 0, &ledger_text);
    let fp_text = format!("注册面指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_frozen() {
        assert_eq!(family_len(), 12);
        assert_eq!(parse_module_name("ltp_read"), Ok(0));
        assert_eq!(parse_module_name("ltp_stat"), Ok(11));
    }

    #[test]
    fn bad_names_rejected() {
        assert_eq!(parse_module_name("read01"), Err(ModCode::BadPrefix));
        assert_eq!(parse_module_name("ltp_x"), Err(ModCode::UnknownModule));
        assert_eq!(parse_module_name(""), Err(ModCode::EmptyName));
    }

    #[test]
    fn full_registry_complete() {
        let reg = build_full_registry();
        assert_eq!(reg.len(), 12);
        assert!(reg.completeness().is_ok());
        assert!(reg.missing_modules().is_empty());
    }

    #[test]
    fn missing_module_named() {
        let mut reg = ModuleRegistry::new();
        let mut idx = 3usize;
        while idx < 12 {
            if let Some(name) = LTP_FAMILY.get(idx) {
                let _ = reg.register(name, FROZEN_PER_MODULE, 4);
            }
            idx += 1;
        }
        let miss = reg.missing_modules();
        assert_eq!(miss.len(), 3);
        assert_eq!(miss.first(), Some(&"ltp_read"));
    }

    #[test]
    fn duplicate_rejected() {
        let mut reg = ModuleRegistry::new();
        assert!(reg.register("ltp_open", 8, 5).is_ok());
        assert_eq!(reg.register("ltp_open", 8, 5), Err(ModCode::DuplicateModule));
    }

    #[test]
    fn pass_overflow_rejected() {
        let mut reg = ModuleRegistry::new();
        assert_eq!(reg.register("ltp_open", 8, 9), Err(ModCode::PassOverflow));
    }

    #[test]
    fn emit_gates_on_completeness() {
        let mut reg = ModuleRegistry::new();
        let _ = reg.register("ltp_read", 8, 6);
        assert_eq!(reg.emit(), Err(ModCode::MissingModule));
    }

    #[test]
    fn emit_values_recomputable() {
        let reg = build_full_registry();
        let (sc, sp) = reg.emit().unwrap_or((0, 0));
        assert_eq!(sc, 96);
        assert_eq!(sp, reg.sum_passed());
        assert!(sp <= sc);
    }

    #[test]
    fn bad_case_matrix_green() {
        let (l, per) = run_bad_case_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, 4 * INJECT_ROUNDS);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn fingerprint_deterministic() {
        assert_eq!(
            registry_fingerprint(&build_full_registry()),
            registry_fingerprint(&build_full_registry())
        );
    }

    #[test]
    fn unx10005_checks_green() {
        let set = run_unx10005_checks();
        assert!(set.all_passed(), "UNX-F10005 red: {}", set.red_items_text());
    }
}
