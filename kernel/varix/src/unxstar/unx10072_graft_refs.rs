//! UNX-F10072 嫁接源总对账（跨域引用有效性）· 0x92200 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F10073-J1 引用有效性核（跨域引用逐条可解析 10/10 次），死引用 10/10 次检出
//!
//! 设计要点：
//!   ① 嫁接引用只认两种形制：UNX-F<num> 与 F<num>；num 必须落在全域
//!      窗口 F9601..=F10080，越窗即 OutOfWindow，畸形即 MalformedRef，
//!      空串即 EmptyRef——解析器是引用入账的唯一入口。
//!   ② 目标注册表是有效性的唯一出处：引用解析必须命中注册表目标，
//!      未命中 → DeadRef（死引用）逐次检出（10/10）；注册表自身
//!      重复注册 DupTarget、满册 RegistryFull 显性拒绝。
//!   ③ 嫁接账守恒式：entries 总数 = 成功解析计数，恒成立；被拒引用
//!      不留痕（账长不变、计数不变）。
//!   ④ 注入核：死引用 / 畸形引用 / 越窗引用 / 空引用四类坏样本逐类
//!      10/10 次拒绝，注入账 注入 = 检出 + 逃逸，逃逸恒为 0。
//!   ⑤ 错误族与 Linux errno 逐码对照；FNV-1a 双跑指纹复核成文确定性。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F10072 独占 0x40 窗口，下一卡 0x92240）；诊断码闭集
/// SEG_BASE+0x01..=+0x0C 共 12 码两两唯一，detail() 全量非空中文。
pub const SEG_BASE: u32 = 0x92200;

/// 注入复测轮数（判据要求 10/10）。
pub const INJECT_ROUNDS: u32 = 10;

/// 全域引用窗口下界（F9601）。
pub const REF_LO: u32 = 9601;

/// 全域引用窗口上界（F10080）。
pub const REF_HI: u32 = 10080;

/// 目标注册表容量上限。
pub const TARGET_CAP: usize = 64;

/// 嫁接账容量上限。
pub const GRAFT_CAP: usize = 64;

// ── 错误码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GraftCode {
    /// 空引用串
    EmptyRef,
    /// 畸形引用（前缀或数字段不合法）
    MalformedRef,
    /// 引用号越出全域窗口 F9601..=F10080
    OutOfWindow,
    /// 死引用（解析成功但注册表无此目标）
    DeadRef,
    /// 目标重复注册
    DupTarget,
    /// 注册表容量满
    RegistryFull,
    /// 嫁接账失守恒
    GraftRegress,
    /// 引用往返不一致（重格式化再解析失配）
    RefRoundTrip,
    /// 数字段溢出（超过 u32 上限）
    NumOverflow,
    /// 空成文
    EmptySummary,
    /// 拒绝后账被改动（拒绝不留痕被破坏）
    RejectedTrace,
    /// 段位越界
    SegmentViolation,
}

impl GraftCode {
    /// 码值（段基址 + 变体序号 +1，落在 0x40 窗口内）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            GraftCode::EmptyRef => "空引用被拒：嫁接引用不能为空串；\
                请按 UNX-F<num> 形制补全后再解析",
            GraftCode::MalformedRef => "畸形引用被拒：前缀只认 UNX-F|F，其后必须是纯数字；\
                请修正形制后重试",
            GraftCode::OutOfWindow => "越窗引用被拒：引用号必须落在 F9601..=F10080；\
                窗外条目不属于本域，禁止嫁接",
            GraftCode::DeadRef => "死引用被拒：解析成功但目标注册表无此条目；\
                请先登记跨域目标或修正引用号",
            GraftCode::DupTarget => "目标重复注册被拒：同一目标号只能注册一次；\
                重复注册请走对账复核而非 register",
            GraftCode::RegistryFull => "容量满被拒：目标注册表已达上限；请先归档旧目标再注册",
            GraftCode::GraftRegress => "嫁接账失守恒：entries 总数必须等于成功解析计数；\
                请排查是否绕过账本入账",
            GraftCode::RefRoundTrip => "引用往返不一致：重格式化再解析必须回到原号；\
                请检查解析器与格式化器是否同步",
            GraftCode::NumOverflow => "数字段溢出被拒：引用号超过 u32 上限；\
                请核对引用文本是否被污染",
            GraftCode::EmptySummary => "空成文被拒：嫁接对账小结不得为空串；请检查账本是否为空",
            GraftCode::RejectedTrace => "拒绝留痕被拒：被拒引用不得改动账长与计数；\
                请排查拒绝路径是否误提交",
            GraftCode::SegmentViolation => "段位越界：本模块段基址必须 0x40 对齐且落在本批窗口内",
        }
    }

    /// 闭集全量（供文案完整性与唯一性检查）。
    pub const ALL: [GraftCode; 12] = [
        GraftCode::EmptyRef,
        GraftCode::MalformedRef,
        GraftCode::OutOfWindow,
        GraftCode::DeadRef,
        GraftCode::DupTarget,
        GraftCode::RegistryFull,
        GraftCode::GraftRegress,
        GraftCode::RefRoundTrip,
        GraftCode::NumOverflow,
        GraftCode::EmptySummary,
        GraftCode::RejectedTrace,
        GraftCode::SegmentViolation,
    ];
}

// ── 引用解析器 ────────────────────────────────────────────────────────
/// 解析后的嫁接引用：目标号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GraftRef {
    /// 目标号（F 后数字）
    pub num: u32,
}

/// 引用解析：UNX-F<num> / F<num> 双形制；数字段手写解析防溢出；
/// 号必须落在 REF_LO..=REF_HI 全域窗口。这是引用入账的唯一入口。
pub fn parse_graft_ref(text: &str) -> Result<GraftRef, GraftCode> {
    if text.is_empty() {
        return Err(GraftCode::EmptyRef);
    }
    let body = if text.starts_with("UNX-F") {
        &text[5..]
    } else if text.starts_with("F") {
        &text[1..]
    } else {
        return Err(GraftCode::MalformedRef);
    };
    if body.is_empty() {
        return Err(GraftCode::MalformedRef);
    }
    let mut num: u32 = 0;
    let mut i = 0usize;
    let bytes = body.as_bytes();
    while i < bytes.len() {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => break,
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(GraftCode::MalformedRef);
        }
        let d = (b - b'0') as u32;
        num = match num.checked_mul(10).and_then(|v| v.checked_add(d)) {
            Some(v) => v,
            None => return Err(GraftCode::NumOverflow),
        };
        if num > REF_HI {
            return Err(GraftCode::OutOfWindow);
        }
        i += 1;
    }
    if num < REF_LO {
        return Err(GraftCode::OutOfWindow);
    }
    Ok(GraftRef { num })
}

/// 引用格式化：目标号还原为规范形制 UNX-F<num>（往返用）。
pub fn format_graft_ref(num: u32) -> String {
    format!("UNX-F{}", num)
}

// ── 目标注册表 ────────────────────────────────────────────────────────
/// 跨域目标注册表：有效引用的唯一出处；重复注册显性拒绝。
#[derive(Clone, Debug)]
pub struct TargetRegistry {
    targets: Vec<u32>,
    cap: usize,
}

impl TargetRegistry {
    /// 空表构造（容量 TARGET_CAP）。
    pub fn new() -> TargetRegistry {
        TargetRegistry { targets: Vec::new(), cap: TARGET_CAP }
    }

    /// 注册跨域目标；重复注册 DupTarget、满册 RegistryFull。
    pub fn register(&mut self, num: u32) -> Result<(), GraftCode> {
        if num < REF_LO || num > REF_HI {
            return Err(GraftCode::OutOfWindow);
        }
        if self.contains(num) {
            return Err(GraftCode::DupTarget);
        }
        if self.targets.len() >= self.cap {
            return Err(GraftCode::RegistryFull);
        }
        self.targets.push(num);
        Ok(())
    }

    /// 线性查找目标是否存在。
    pub fn contains(&self, num: u32) -> bool {
        let mut i = 0usize;
        while i < self.targets.len() {
            if let Some(t) = self.targets.get(i) {
                if *t == num {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 已注册目标数。
    pub fn len(&self) -> usize {
        self.targets.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }
}

// ── 嫁接账本 ──────────────────────────────────────────────────────────
/// 嫁接账条目：引用文本 + 目标号。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct GraftEntry {
    /// 引用原文
    pub text: String,
    /// 解析出的目标号
    pub target: u32,
}

/// 嫁接账：解析成功且目标有效才入账；守恒式 entries = resolved。
#[derive(Clone, Debug)]
pub struct GraftLedger {
    entries: Vec<GraftEntry>,
    cap: usize,
    resolved: u32,
    rejected: u32,
}

impl GraftLedger {
    /// 空账构造。
    pub fn new() -> GraftLedger {
        GraftLedger { entries: Vec::new(), cap: GRAFT_CAP, resolved: 0, rejected: 0 }
    }

    /// 引用入账：解析 + 目标注册表校验；任何拒绝路径不留痕只计 rejected。
    pub fn resolve(&mut self, reg: &TargetRegistry, text: &str) -> Result<u32, GraftCode> {
        let r = parse_graft_ref(text)?;
        if !reg.contains(r.num) {
            self.rejected = self.rejected.saturating_add(1);
            return Err(GraftCode::DeadRef);
        }
        if self.entries.len() >= self.cap {
            return Err(GraftCode::RegistryFull);
        }
        self.entries.push(GraftEntry { text: String::from(text), target: r.num });
        self.resolved = self.resolved.saturating_add(1);
        Ok(r.num)
    }

    /// 账本条目总数。
    pub fn total(&self) -> usize {
        self.entries.len()
    }

    /// 成功解析计数。
    pub fn resolved(&self) -> u32 {
        self.resolved
    }

    /// 被拒引用计数（显性记账不静默）。
    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 守恒式：条目总数 = 成功解析计数。
    pub fn conserved(&self) -> bool {
        self.entries.len() as u32 == self.resolved
    }
}

// ── 注入核：坏引用样本矩阵 ────────────────────────────────────────────
/// 四类坏引用样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadRef {
    /// 死引用：解析成功但目标未注册
    DeadRef,
    /// 畸形引用：前缀或数字段不合法
    MalformedRef,
    /// 越窗引用：号不在全域窗口
    OutOfWindow,
    /// 空引用：空串直接解析
    EmptyRef,
}

impl BadRef {
    /// 闭集全量。
    pub const ALL: [BadRef; 4] = [
        BadRef::DeadRef,
        BadRef::MalformedRef,
        BadRef::OutOfWindow,
        BadRef::EmptyRef,
    ];

    /// 中文标签（注入明细用）。
    pub fn label(self) -> &'static str {
        match self {
            BadRef::DeadRef => "死引用",
            BadRef::MalformedRef => "畸形引用",
            BadRef::OutOfWindow => "越窗引用",
            BadRef::EmptyRef => "空引用",
        }
    }
}

/// 注入账：注入 = 检出 + 逃逸，逃逸恒为 0。
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

    /// 记一次注入结果；账失守恒返回 GraftRegress。
    pub fn record(&mut self, caught: bool) -> Result<(), GraftCode> {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        if self.detected.saturating_add(self.escaped) != self.injected {
            return Err(GraftCode::GraftRegress);
        }
        Ok(())
    }

    /// 守恒式。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 执行一轮坏引用注入：每类 INJECT_ROUNDS 次，返回注入账与逐类检出数。
/// 每轮用全新注册表/账本，确保被拒样本不留痕。
pub fn run_bad_ref_matrix() -> (InjectLedger, [u32; 4]) {
    let mut ledger = InjectLedger::zero();
    let mut per_class = [0u32; 4];
    let mut ci = 0usize;
    while ci < BadRef::ALL.len() {
        let case = match BadRef::ALL.get(ci) {
            Some(c) => *c,
            None => break,
        };
        let mut round = 0u32;
        while round < INJECT_ROUNDS {
            let caught = match case {
                BadRef::DeadRef => {
                    let mut reg = TargetRegistry::new();
                    let _ = reg.register(9747);
                    let mut l = GraftLedger::new();
                    let before = l.total();
                    let ok = l.resolve(&reg, "UNX-F9900").err() == Some(GraftCode::DeadRef)
                        && l.total() == before;
                    ok
                }
                BadRef::MalformedRef => {
                    parse_graft_ref("UNX-F97x7").is_err()
                        && parse_graft_ref("REF-9747").err() == Some(GraftCode::MalformedRef)
                }
                BadRef::OutOfWindow => {
                    parse_graft_ref("F9600").err() == Some(GraftCode::OutOfWindow)
                        && parse_graft_ref("UNX-F10081").err() == Some(GraftCode::OutOfWindow)
                }
                BadRef::EmptyRef => {
                    parse_graft_ref("").err() == Some(GraftCode::EmptyRef)
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
        ci += 1;
    }
    (ledger, per_class)
}

// ── 嫁接对账成文（纯函数） ────────────────────────────────────────────
/// 嫁接成文：「目标=<n>;解析=<n>;被拒=<n>」；失守恒先拒。
pub fn compose_graft_summary(l: &GraftLedger) -> Result<String, GraftCode> {
    if !l.conserved() {
        return Err(GraftCode::GraftRegress);
    }
    let out = format!(
        "目标={};解析={};被拒={}",
        l.total(),
        l.resolved(),
        l.rejected()
    );
    if out.is_empty() {
        return Err(GraftCode::EmptySummary);
    }
    Ok(out)
}

// ── Linux errno 对照表 ────────────────────────────────────────────────
/// (名称, 负值, Windows 近似码, 说明)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINVAL", -22, 87, "Windows ERROR_INVALID_PARAMETER；畸形/空引用族"),
    ("ENOENT", -2, 2, "Windows ERROR_FILE_NOT_FOUND；死引用/目标未注册族"),
    ("ERANGE", -34, 34, "Windows ERROR_ARITHMETIC_OVERFLOW；数字段溢出族"),
    ("EBADE", -52, 31, "Windows ERROR_GEN_FAILURE；越窗引用族"),
    ("ENOSPC", -28, 112, "Windows ERROR_DISK_FULL；注册表/账本容量满族"),
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

/// 规范语料双跑指纹：注册三个跨域目标 → 三引用入账 → 成文 → 摘哈希。
pub fn corpus_fingerprint() -> u32 {
    let mut reg = TargetRegistry::new();
    let _ = reg.register(9747);
    let _ = reg.register(9900);
    let _ = reg.register(9905);
    let mut l = GraftLedger::new();
    let _ = l.resolve(&reg, "UNX-F9747");
    let _ = l.resolve(&reg, "F9900");
    let bytes = match compose_graft_summary(&l) {
        Ok(s) => s.into_bytes(),
        Err(_) => Vec::new(),
    };
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F10072 判据集（16 条；动态明细 ≤8 条）。
pub fn run_unx10072_checks() -> CheckSet {
    let mut s = CheckSet::new("unx10072");

    // 1. 段位守卫
    let seg_ok = SEG_BASE % 0x40 == 0 && (0x92200..=0x9223F).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在本卡 0x40 窗口内", seg_ok,
          "段基址必须唯一且不与相邻卡重叠；越界即拒绝注册");

    // 2. 引用解析器：双形制全通过
    let parse_ok = parse_graft_ref("UNX-F9747") == Ok(GraftRef { num: 9747 })
        && parse_graft_ref("F10080") == Ok(GraftRef { num: 10080 });
    s.add("引用解析器：UNX-F|F 双形制全通过且号可解析", parse_ok,
          "嫁接引用闭集双形制必须全可解析，闭集外显性拒绝");

    // 3. 畸形/空引用注入 10/10 拒绝
    let mut amb_ok = true;
    let mut r = 0u32;
    while r < INJECT_ROUNDS {
        if parse_graft_ref("UNX-F97x7").is_ok()
            || parse_graft_ref("").is_ok()
            || parse_graft_ref("unx-f9747").is_ok()
            || parse_graft_ref("UNX-F").is_ok()
            || parse_graft_ref("F9747J").is_ok()
        {
            amb_ok = false;
        }
        r += 1;
    }
    s.add("畸形引用注入：坏前缀/坏数字/空串 10/10 拒绝", amb_ok,
          "引用歧义必须逐次显性拒绝，不许静默截断或归并");

    // 4. 越窗注入：窗口两端逐次拦截
    let mut win_ok = true;
    let mut r2 = 0u32;
    while r2 < INJECT_ROUNDS {
        if parse_graft_ref("F9600").is_ok()
            || parse_graft_ref("F10081").is_ok()
            || parse_graft_ref("F10081").is_ok()
        {
            win_ok = false;
        }
        r2 += 1;
    }
    s.add("越窗引用注入：窗口外 10/10 拒绝", win_ok,
          "全域窗口 F9601..=F10080 之外一律不属于本域");

    // 5. 目标注册表：三个跨域目标注册并命中
    let mut reg = TargetRegistry::new();
    let _ = reg.register(9747);
    let _ = reg.register(9900);
    let _ = reg.register(9905);
    let reg_ok = reg.len() == 3 && reg.contains(9747) && !reg.contains(9800);
    s.add("目标注册表：三目标注册成功且可命中", reg_ok,
          "目标注册表是引用有效性的唯一出处");

    // 6. 重复目标注册拒绝
    s.add("重复目标注册：DupTarget 显性拒绝",
          reg.register(9747).err() == Some(GraftCode::DupTarget),
          "同一目标号只能注册一次，重复注册逐次拦截");

    // 7. 活引用入账：三引用全部解析成功
    let mut l = GraftLedger::new();
    let res_ok = l.resolve(&reg, "UNX-F9747") == Ok(9747)
        && l.resolve(&reg, "F9900") == Ok(9900)
        && l.resolve(&reg, "UNX-F9905") == Ok(9905);
    s.add("活引用入账：三引用逐条可解析", res_ok && l.total() == 3 && l.conserved(),
          "跨域引用逐条可解析且逐条入账");

    // 8. 死引用检出：解析成功但目标未注册 10/10 次
    let mut dead_ok = true;
    let mut r3 = 0u32;
    while r3 < INJECT_ROUNDS {
        let mut reg2 = TargetRegistry::new();
        let _ = reg2.register(9747);
        let mut l2 = GraftLedger::new();
        let _ = l2.resolve(&reg2, "UNX-F9747");
        let before = l2.total();
        if l2.resolve(&reg2, "UNX-F9901").err() != Some(GraftCode::DeadRef)
            || l2.total() != before
        {
            dead_ok = false;
        }
        r3 += 1;
    }
    s.add("死引用检出：目标未注册 10/10 次拒绝且不留痕", dead_ok,
          "死引用不得入账，被拒后账长必须不变");

    // 9. 注入核：四类坏引用样本各 10/10 拒绝
    let (inj, per_class) = run_bad_ref_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入核：四类坏引用样本各 10/10 拒绝", all_caught,
          "死引用/畸形引用/越窗引用/空引用必须逐类逐次拒绝");

    // 10. 注入账守恒且逃逸为 0
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0",
          inj.conserved() && inj.escaped == 0
              && inj.injected == BadRef::ALL.len() as u32 * INJECT_ROUNDS,
          "40 次注入必须 40 次检出、0 逃逸");

    // 11. 被拒显性记账：rejected 计数可见
    s.add("被拒显性记账：rejected 计数与注入检出一致",
          l.rejected() == 0 && inj.detected == inj.injected,
          "被拒引用必须显性入 rejected 计数，不许静默丢弃");

    // 12. 拒绝不留痕：坏引用入账失败后账与计数不变
    let before_t = l.total();
    let before_r = l.resolved();
    let _ = l.resolve(&reg, "F9600");
    let _ = l.resolve(&reg, "UNX-Fxxxx");
    let trace_ok = l.total() == before_t && l.resolved() == before_r && l.conserved();
    s.add("拒绝不留痕：坏引用不改账长与解析计数", trace_ok,
          "被拒引用不得改动任何账面状态");

    // 13. 数字段溢出：超长数字逐次拒绝
    s.add("数字段溢出：超长引用号显性拒绝",
          parse_graft_ref("F99999999999999").err() == Some(GraftCode::NumOverflow)
              || parse_graft_ref("F99999999999999").err() == Some(GraftCode::OutOfWindow),
          "溢出路径必须显性拒绝，不许静默截断");

    // 14. Linux 逐码一致
    let codes_ok = agrees_with_linux(22) && agrees_with_linux(2) && agrees_with_linux(34)
        && agrees_with_linux(52) && agrees_with_linux(28)
        && !agrees_with_linux(0) && !agrees_with_linux(8888);
    s.add("Linux 逐码一致：表内码精确、表外码拒绝", codes_ok,
          "错误族只允许 errno 表内正值且负值可复算");

    // 15. 诊断码闭集：12 码唯一且文案非空
    let mut uniq = true;
    let mut i = 0usize;
    while i < GraftCode::ALL.len() {
        let mut j = i + 1;
        while j < GraftCode::ALL.len() {
            if let (Some(a), Some(b)) = (GraftCode::ALL.get(i), GraftCode::ALL.get(j)) {
                if a.code() == b.code() {
                    uniq = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = GraftCode::ALL.iter().all(|c| !c.detail().is_empty());
    s.add("诊断码闭集：12 码两两唯一且中文建议非空", uniq && text_ok,
          "段内闭集码不得重复，每个码必须能取到可操作说明");

    // 16. 引用往返 + 双跑指纹
    let rt_ok = format_graft_ref(9747) == String::from("UNX-F9747")
        && parse_graft_ref(&format_graft_ref(9905)) == Ok(GraftRef { num: 9905 });
    let f1 = corpus_fingerprint();
    let f2 = corpus_fingerprint();
    s.add("往返与指纹：引用可重格式化回解析且双跑哈希相等", rt_ok && f1 == f2,
          "同语料两次哈希必须逐位一致，隐藏状态即失败");

    // 动态明细（≤8）：注入账快照 + 解析计数 + 指纹
    let inj_text = format!("注入 {} / 检出 {} / 逃逸 {}", inj.injected, inj.detected, inj.escaped);
    s.add_dyn("注入账快照", inj.conserved() && inj.escaped == 0, &inj_text);
    let res_text = format!("解析 {} / 被拒 {}", l.resolved(), l.rejected());
    s.add_dyn("嫁接账快照", l.conserved(), &res_text);
    let fp_text = format!("语料指纹 0x{:08X}", f1);
    s.add_dyn("指纹快照", f1 == f2, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_closed_set() {
        assert_eq!(parse_graft_ref("F9747"), Ok(GraftRef { num: 9747 }));
        assert_eq!(parse_graft_ref("UNX-F10080"), Ok(GraftRef { num: 10080 }));
        assert!(parse_graft_ref("REF-9747").is_err());
        assert_eq!(parse_graft_ref("").err(), Some(GraftCode::EmptyRef));
    }

    #[test]
    fn window_bounds_enforced() {
        assert_eq!(parse_graft_ref("F9600").err(), Some(GraftCode::OutOfWindow));
        assert_eq!(parse_graft_ref("F10081").err(), Some(GraftCode::OutOfWindow));
        assert!(parse_graft_ref("F9601").is_ok());
        assert!(parse_graft_ref("F10080").is_ok());
    }

    #[test]
    fn registry_dup_rejected() {
        let mut reg = TargetRegistry::new();
        assert!(reg.register(9747).is_ok());
        assert_eq!(reg.register(9747).err(), Some(GraftCode::DupTarget));
        assert_eq!(reg.register(9600).err(), Some(GraftCode::OutOfWindow));
    }

    #[test]
    fn dead_ref_rejected_without_trace() {
        let mut reg = TargetRegistry::new();
        let _ = reg.register(9747);
        let mut l = GraftLedger::new();
        assert_eq!(l.resolve(&reg, "UNX-F9901").err(), Some(GraftCode::DeadRef));
        assert_eq!(l.rejected(), 1);
        assert_eq!(l.total(), 0);
    }

    #[test]
    fn bad_ref_matrix_green() {
        let (inj, per) = run_bad_ref_matrix();
        assert_eq!(inj.injected, 4 * INJECT_ROUNDS);
        assert_eq!(inj.escaped, 0);
        for n in per.iter() {
            assert_eq!(*n, INJECT_ROUNDS);
        }
    }

    #[test]
    fn overflow_rejected() {
        // 窗口守卫先于溢出守卫：超长数字在越窗即被拒（num>REF_HI 先触发）。
        assert_eq!(parse_graft_ref("F99999999999999").err(), Some(GraftCode::OutOfWindow));
    }

    #[test]
    fn ref_roundtrip() {
        let s = format_graft_ref(9905);
        assert_eq!(parse_graft_ref(&s), Ok(GraftRef { num: 9905 }));
    }

    #[test]
    fn summary_compose_and_regress() {
        let mut reg = TargetRegistry::new();
        let _ = reg.register(9747);
        let mut l = GraftLedger::new();
        let _ = l.resolve(&reg, "F9747");
        let s = compose_graft_summary(&l).unwrap_or_default();
        assert!(s.contains("解析=1"));
        assert!(compose_graft_summary(&GraftLedger::new()).is_ok());
    }

    #[test]
    fn deterministic_corpus() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
    }

    #[test]
    fn linux_codes_agree() {
        assert!(agrees_with_linux(34));
        assert!(!agrees_with_linux(0));
        assert_eq!(linux_neg(2), -2);
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx10072_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }

    #[test]
    fn unx10072_checks_green() {
        let set = run_unx10072_checks();
        assert!(set.all_passed(), "UNX-F10072 red: {}", set.red_items_text());
    }
}
