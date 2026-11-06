//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18446 · requestedExecutionLevel 解析消费 · 0x7EB40 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18446-J1 执行级别四档（asInvoker/highestAvailable/
//!       requireAdministrator/uiAccess）解析落账，与 B26 虚拟化开关
//!       衔接签名冻结 v1
//! 本模块结构清单：
//!   1. 冻结常量层：四档词元闭集（3 个 level 值 + uiAccess 属性名），
//!      级别档位 AsInvoker=0 / HighestAvailable=1 / RequireAdmin=2；
//!      B26 虚拟化衔接签名 v1 = FNV-1a("b26-virt-v1") const 期推导；
//!   2. 诊断码闭集 ElCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 解析器：requestedExecutionLevel 标签扫描，level 属性
//!      （3 词元闭集，大小写不敏感——Windows manifest 口径）+
//!      uiAccess 属性（"true"/"false" 闭集）；缺 level 属性 → 默认
//!      asInvoker（Windows 口径：无显式级别视同 asInvoker）；
//!      空串值 / 闭集外词元 / 非法 uiAccess 值 → 显式拒；
//!   4. 落账 ExecRecord：档位 + uiAccess + 虚拟化位 + 源标记；
//!   5. B26 衔接：虚拟化开关派生规则冻结——level==asInvoker 且
//!      uiAccess==false 才可开虚拟化（高权限进程虚拟化无意义且
//!      Windows 口径对提权进程禁用虚拟化）；
//!   6. 落账账 ExecLedger：记录账（容量 16）+ 守恒（记账数=解析数）；
//!   7. 注入核 4 类：非法 level 词元 / 非法 uiAccess 值 / 空值 /
//!      标签截断，逐类断言被显式拒绝；
//!   8. 边界：缺属性默认档 / 重复解析幂等 / 账满拒 / 双跑一致；
//!   9. run_unx18446_checks：16 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EB40，G1 工单 A05 批03 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EB40;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 档位数（写死 3：level 闭集 asInvoker/highestAvailable/requireAdministrator）。
pub const LEVEL_N: usize = 3;
/// 档位值（写死：Windows manifest 口径）。
pub const LVL_AS_INVOKER: u8 = 0;
pub const LVL_HIGHEST_AVAIL: u8 = 1;
pub const LVL_REQUIRE_ADMIN: u8 = 2;
/// 词元表（写死小写规范形；解析大小写不敏感）。
pub const LEVEL_TOKENS: [&str; LEVEL_N] = ["asInvoker", "highestAvailable", "requireAdministrator"];
/// uiAccess 词元（写死：第四档词元，布尔属性）。
pub const UIACCESS_TOKEN: &str = "uiAccess";
/// 落账账容量（写死 16）。
pub const LEDGER_CAP: usize = 16;
/// 判据轮数（写死 3）。
pub const JUDGE_ROUNDS: u32 = 3;
/// FNV 常量。
const FNV_OFFSET: u32 = 0x811C_9DC5;
const FNV_PRIME: u32 = 0x0100_0193;

/// const fn：FNV-1a 32 对字节串。
const fn fnv1a_str(s: &[u8]) -> u32 {
    let mut h: u32 = FNV_OFFSET;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u32;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// B26 虚拟化衔接签名 v1（const 期推导：字面串 "b26-virt-v1" 的 FNV）。
/// 推导：衔接签名把衔接版本烧进哈希——B26 侧签名漂移即互证失配，
/// 升版必须走 v1→v2 显式换签，不允许静默漂移。
pub const B26_HANDOFF_SIG_V1: u32 = fnv1a_str(b"b26-virt-v1");

/// 运行期同口径哈希。
pub fn fnv1a_str_rt(s: &[u8]) -> u32 {
    fnv1a_str(s)
}

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ElCode {
    /// 非法 level 词元：3 词元闭集外
    LevelTokenBad,
    /// 非法 uiAccess 值："true"/"false" 闭集外
    UiAccessBad,
    /// 空属性值
    EmptyValue,
    /// 标签截断：'>' 前未闭合
    TagTruncated,
    /// 档位越界：>2
    LevelOutOfRange,
    /// 账满：16 上限
    LedgerFull,
    /// 账守恒破坏：记账数 ≠ 解析数
    LedgerBroken,
    /// B26 衔接签名失配
    HandoffSigMismatch,
    /// 虚拟化规则破坏：提权档开启虚拟化
    VirtRuleBroken,
    /// 解析不幂等
    NotIdempotent,
    /// 注入未检出
    InjectMissed,
    /// 双跑不一致
    NotReproducible,
}

impl ElCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            ElCode::LevelTokenBad => "非法 level 词元：3 词元闭集外",
            ElCode::UiAccessBad => "非法 uiAccess 值：true/false 闭集外",
            ElCode::EmptyValue => "空属性值",
            ElCode::TagTruncated => "标签截断：'>' 前未闭合",
            ElCode::LevelOutOfRange => "档位越界：>2",
            ElCode::LedgerFull => "落账账满：16 上限",
            ElCode::LedgerBroken => "账守恒破坏",
            ElCode::HandoffSigMismatch => "B26 衔接签名失配",
            ElCode::VirtRuleBroken => "虚拟化规则破坏：提权档禁虚拟化",
            ElCode::NotIdempotent => "解析不幂等",
            ElCode::InjectMissed => "注入未检出",
            ElCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [ElCode; 12] = [
        ElCode::LevelTokenBad,
        ElCode::UiAccessBad,
        ElCode::EmptyValue,
        ElCode::TagTruncated,
        ElCode::LevelOutOfRange,
        ElCode::LedgerFull,
        ElCode::LedgerBroken,
        ElCode::HandoffSigMismatch,
        ElCode::VirtRuleBroken,
        ElCode::NotIdempotent,
        ElCode::InjectMissed,
        ElCode::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ElCode::ALL.len() {
        if ElCode::ALL[i].code() == code {
            return ElCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EB40 段闭集内"
}

// ── 落账记录 ───────────────────────────────────────────────────────
/// 一条执行级别落账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExecRecord {
    /// level 档位（0..2 闭集；缺属性默认 0）。
    pub level: u8,
    /// uiAccess 位。
    pub ui_access: bool,
    /// B26 虚拟化开关派生位：level==0 且 !ui_access 才为真。
    pub virtualization: bool,
    /// level 是否显式出现（false = 缺属性默认档）。
    pub explicit: bool,
}

impl ExecRecord {
    /// B26 虚拟化派生规则（冻结）：仅 asInvoker 且非 uiAccess 可开。
    /// 为什么：提权进程（highestAvailable 落到 admin / requireAdmin）
    /// 运行于管理员令牌，文件/注册表虚拟化对管理员令牌无意义且被
    /// Windows UAC 口径禁用；uiAccess 进程绕过 UIPI，同样禁虚拟化。
    pub fn derive_virt(level: u8, ui_access: bool) -> Result<bool, u32> {
        if level as usize >= LEVEL_N {
            return Err(ElCode::LevelOutOfRange.code());
        }
        Ok(level == LVL_AS_INVOKER && !ui_access)
    }
    /// 衔接签名互证：记录派生结果与 B26 v1 规则一致。
    pub fn b26_consistent(&self) -> Result<(), u32> {
        let v = Self::derive_virt(self.level, self.ui_access)?;
        if v != self.virtualization {
            return Err(ElCode::VirtRuleBroken.code());
        }
        Ok(())
    }
}

// ── 解析器 ─────────────────────────────────────────────────────────
/// 大小写不敏感词元匹配（ASCII：只按 a-zA-Z 折叠，与 Windows manifest
/// 解析口径一致——XML 属性值词元大小写不敏感）。
fn token_eq_ci(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        let ca = a[i].to_ascii_lowercase();
        let cb = b[i].to_ascii_lowercase();
        if ca != cb {
            return false;
        }
        i += 1;
    }
    true
}

/// level 词元 → 档位（大小写不敏感；闭集外 None）。
pub fn level_token_to_id(tok: &str) -> Option<u8> {
    let mut i = 0;
    while i < LEVEL_N {
        if token_eq_ci(tok.as_bytes(), LEVEL_TOKENS[i].as_bytes()) {
            return Some(i as u8);
        }
        i += 1;
    }
    None
}

/// uiAccess 词元值 → bool（"true"/"false" 大小写不敏感；闭集外 None）。
pub fn uiaccess_token_to_bool(tok: &str) -> Option<bool> {
    if token_eq_ci(tok.as_bytes(), b"true") {
        return Some(true);
    }
    if token_eq_ci(tok.as_bytes(), b"false") {
        return Some(false);
    }
    None
}

/// 标签内属性抓取：在 `name="value"` 序列中找指定属性值。
/// 返回 Some(值) / None（属性缺失）。空串值返回 Some("")。
fn find_attr(tag: &[u8], attr: &str) -> Option<String> {
    let needle = attr.as_bytes();
    let mut i = 0;
    while i + needle.len() <= tag.len() {
        if &tag[i..i + needle.len()] == needle {
            // 前界：必须是行首/空白/'<' 之后（避免前缀误匹配，如
            // "uiAccessX" 误匹配 "uiAccess"）。
            let left_ok = i == 0 || !(tag[i - 1] as char).is_ascii_alphanumeric();
            if !left_ok {
                i += 1;
                continue;
            }
            // 后随 '='。
            let mut k = i + needle.len();
            while k < tag.len() && (tag[k] == b' ') {
                k += 1;
            }
            if k >= tag.len() || tag[k] != b'=' {
                i += 1;
                continue;
            }
            k += 1;
            while k < tag.len() && tag[k] == b' ' {
                k += 1;
            }
            if k >= tag.len() || tag[k] != b'"' {
                // 无引号值：Windows manifest 规范要求引号，宽松口径取
                // 至空白——但本仓冻结严格口径：必须引号。
                return None;
            }
            k += 1;
            let v0 = k;
            while k < tag.len() && tag[k] != b'"' {
                k += 1;
            }
            if k >= tag.len() {
                return None; // 引号未闭。
            }
            return Some(String::from_utf8_lossy(&tag[v0..k]).into_owned());
        }
        i += 1;
    }
    None
}

/// 解析 requestedExecutionLevel 标签 → 落账记录。
/// 输入：标签体（`<requestedExecutionLevel ... />` 内容，含属性）。
/// 口径：缺 level 属性 → 默认 asInvoker（explicit=false）；缺
/// uiAccess 属性 → 默认 false；空串值/闭集外词元 → 显式拒。
pub fn parse_exec_level(tag: &str) -> Result<ExecRecord, u32> {
    let b = tag.as_bytes();
    // 标签截断闸：必须以 '>' 收尾（允许 '/>'）。
    if b.len() == 0 || b[b.len() - 1] != b'>' {
        return Err(ElCode::TagTruncated.code());
    }
    let lv = match find_attr(b, "level") {
        None => (LVL_AS_INVOKER, false),
        Some(v) => {
            if v.len() == 0 {
                return Err(ElCode::EmptyValue.code());
            }
            match level_token_to_id(&v) {
                Some(id) => (id, true),
                None => return Err(ElCode::LevelTokenBad.code()),
            }
        }
    };
    let ua = match find_attr(b, UIACCESS_TOKEN) {
        None => false,
        Some(v) => {
            if v.len() == 0 {
                return Err(ElCode::EmptyValue.code());
            }
            match uiaccess_token_to_bool(&v) {
                Some(x) => x,
                None => return Err(ElCode::UiAccessBad.code()),
            }
        }
    };
    let virt = ExecRecord::derive_virt(lv.0, ua)?;
    Ok(ExecRecord { level: lv.0, ui_access: ua, virtualization: virt, explicit: lv.1 })
}

// ── 落账账 ─────────────────────────────────────────────────────────
/// 落账账：记录账（容量 16）+ 守恒。
pub struct ExecLedger {
    /// 记录账。
    pub recs: Vec<ExecRecord>,
    /// 解析总数（含拒收）。
    pub parsed: u32,
    /// 拒收数。
    pub rejected: u32,
    /// 拒因账。
    pub reject_codes: Vec<u32>,
}

impl ExecLedger {
    /// 新账。
    pub fn new() -> ExecLedger {
        ExecLedger { recs: Vec::new(), parsed: 0, rejected: 0, reject_codes: Vec::new() }
    }
    /// 解析+落账一体：成功入账，拒收留因。
    pub fn feed(&mut self, tag: &str) -> Result<ExecRecord, u32> {
        self.parsed += 1;
        match parse_exec_level(tag) {
            Ok(r) => {
                if self.recs.len() >= LEDGER_CAP {
                    self.rejected += 1;
                    self.reject_codes.push(ElCode::LedgerFull.code());
                    return Err(ElCode::LedgerFull.code());
                }
                r.b26_consistent()?;
                self.recs.push(r);
                Ok(r)
            }
            Err(c) => {
                self.rejected += 1;
                self.reject_codes.push(c);
                Err(c)
            }
        }
    }
    /// 守恒：parsed == recs.len() + rejected。
    pub fn conserve(&self) -> bool {
        self.parsed as usize == self.recs.len() + self.rejected as usize
    }
}

// ── B26 衔接守卫 ───────────────────────────────────────────────────
/// B26 衔接签名互证：v1 冻结值与运行期 FNV 重算一致。
pub fn handoff_sig_check(sig: u32) -> Result<(), u32> {
    if sig != B26_HANDOFF_SIG_V1 {
        return Err(ElCode::HandoffSigMismatch.code());
    }
    if sig != fnv1a_str_rt(b"b26-virt-v1") {
        return Err(ElCode::HandoffSigMismatch.code());
    }
    Ok(())
}

// ── 注入核（4 类坏样本，逐类断言被显式拒绝） ───────────────────────
/// 注入 1：非法 level 词元（"runAsAdmin" 闭集外）。
pub fn inject_bad_level_token() -> Result<(), u32> {
    match parse_exec_level("<requestedExecutionLevel level=\"runAsAdmin\" />") {
        Err(c) if c == ElCode::LevelTokenBad.code() => Ok(()),
        _ => Err(ElCode::InjectMissed.code()),
    }
}

/// 注入 2：非法 uiAccess 值（"yes" 闭集外）。
pub fn inject_bad_uiaccess() -> Result<(), u32> {
    match parse_exec_level("<requestedExecutionLevel level=\"asInvoker\" uiAccess=\"yes\" />") {
        Err(c) if c == ElCode::UiAccessBad.code() => Ok(()),
        _ => Err(ElCode::InjectMissed.code()),
    }
}

/// 注入 3：空属性值（level=""）。
pub fn inject_empty_value() -> Result<(), u32> {
    match parse_exec_level("<requestedExecutionLevel level=\"\" />") {
        Err(c) if c == ElCode::EmptyValue.code() => Ok(()),
        _ => Err(ElCode::InjectMissed.code()),
    }
}

/// 注入 4：标签截断（无 '>' 收尾）。
pub fn inject_truncated() -> Result<(), u32> {
    match parse_exec_level("<requestedExecutionLevel level=\"asInvoker\"") {
        Err(c) if c == ElCode::TagTruncated.code() => Ok(()),
        _ => Err(ElCode::InjectMissed.code()),
    }
}

/// 注入轮次：JUDGE_ROUNDS 轮 4 类注入全拒。
pub fn inject_rounds() -> bool {
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if inject_bad_level_token().is_err()
            || inject_bad_uiaccess().is_err()
            || inject_empty_value().is_err()
            || inject_truncated().is_err()
        {
            return false;
        }
        r += 1;
    }
    true
}

/// 幂等检查：同标签两次解析记录逐位一致。
pub fn idempotent_check(tag: &str) -> Result<(), u32> {
    let a = parse_exec_level(tag)?;
    let b = parse_exec_level(tag)?;
    if a != b {
        return Err(ElCode::NotIdempotent.code());
    }
    Ok(())
}

/// 双跑一致性。
pub fn repro_check() -> Result<(), u32> {
    let a = inject_rounds();
    let b = inject_rounds();
    if a != b {
        return Err(ElCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ElCode::ALL.len() {
        let v = ElCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 黄金全链：衔接签名→四档词元全解析落账→虚拟化规则逐档互证→
/// 注入全拒→账守恒→幂等→双跑一致。
pub fn golden_run() -> Result<(u32, u32), u32> {
    handoff_sig_check(B26_HANDOFF_SIG_V1)?;
    let mut led = ExecLedger::new();
    // 四档全落账：3 level × uiAccess false + asInvoker × uiAccess true。
    let r0 = led.feed("<requestedExecutionLevel level=\"asInvoker\" uiAccess=\"false\" />")?;
    let r1 = led.feed("<requestedExecutionLevel level=\"highestAvailable\" uiAccess=\"false\" />")?;
    let r2 = led.feed("<requestedExecutionLevel level=\"requireAdministrator\" uiAccess=\"false\" />")?;
    let r3 = led.feed("<requestedExecutionLevel level=\"asInvoker\" uiAccess=\"true\" />")?;
    // 派生位核对：仅 r0 可开虚拟化。
    if !(r0.virtualization && !r1.virtualization && !r2.virtualization && !r3.virtualization) {
        return Err(ElCode::VirtRuleBroken.code());
    }
    // 缺属性默认档：explicit=false，level=0。
    let r4 = led.feed("<requestedExecutionLevel />")?;
    if r4.level != LVL_AS_INVOKER || r4.explicit || r4.virtualization != true {
        return Err(ElCode::LevelTokenBad.code());
    }
    if !led.conserve() {
        return Err(ElCode::LedgerBroken.code());
    }
    if !inject_rounds() {
        return Err(ElCode::InjectMissed.code());
    }
    idempotent_check("<requestedExecutionLevel level=\"highestAvailable\" uiAccess=\"true\" />")?;
    repro_check()?;
    Ok((led.parsed, led.rejected))
}

// ── 判据（16 条，断言性质） ────────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18446_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18446 requestedExecutionLevel 解析消费");
    // 1. 四档词元解析落账：3 level + uiAccess 全部成账（J1 前半句）。
    let four_ok = {
        let mut led = ExecLedger::new();
        let a = led.feed("<requestedExecutionLevel level=\"asInvoker\" uiAccess=\"false\" />");
        let b = led.feed("<requestedExecutionLevel level=\"highestAvailable\" uiAccess=\"false\" />");
        let c = led.feed("<requestedExecutionLevel level=\"requireAdministrator\" uiAccess=\"false\" />");
        let d = led.feed("<requestedExecutionLevel level=\"asInvoker\" uiAccess=\"true\" />");
        match (a, b, c, d) {
            (Ok(x), Ok(y), Ok(z), Ok(w)) => {
                x.level == LVL_AS_INVOKER
                    && y.level == LVL_HIGHEST_AVAIL
                    && z.level == LVL_REQUIRE_ADMIN
                    && w.ui_access
                    && led.conserve()
            }
            _ => false,
        }
    };
    s.add("J1-四档解析落账", four_ok, "asInvoker/highestAvail/requireAdmin/uiAccess 全成账");
    // 2. B26 衔接签名 v1（J1 后半句）。
    s.add("J1-B26衔接签名v1", handoff_sig_check(B26_HANDOFF_SIG_V1).is_ok(), "FNV(\"b26-virt-v1\") 冻结互证");
    // 3. 虚拟化规则：仅 asInvoker+非 uiAccess 可开。
    let v0 = ExecRecord::derive_virt(LVL_AS_INVOKER, false);
    let v1 = ExecRecord::derive_virt(LVL_HIGHEST_AVAIL, false);
    let v2 = ExecRecord::derive_virt(LVL_REQUIRE_ADMIN, false);
    let v3 = ExecRecord::derive_virt(LVL_AS_INVOKER, true);
    s.add(
        "J1-虚拟化派生规则",
        v0 == Ok(true) && v1 == Ok(false) && v2 == Ok(false) && v3 == Ok(false),
        "提权/uiAccess 档禁虚拟化",
    );
    // 4. 注入 1：非法 level 词元。
    s.add("J1-注入:非法level词元", inject_bad_level_token().is_ok(), "runAsAdmin 拒");
    // 5. 注入 2：非法 uiAccess 值。
    s.add("J1-注入:非法uiAccess", inject_bad_uiaccess().is_ok(), "\"yes\" 拒");
    // 6. 注入 3：空属性值。
    s.add("J1-注入:空属性值", inject_empty_value().is_ok(), "level=\"\" 拒");
    // 7. 注入 4：标签截断。
    s.add("J1-注入:标签截断", inject_truncated().is_ok(), "无 '>' 拒");
    // 8. 注入多轮复现。
    s.add("J1-注入3轮全拒", inject_rounds(), "4 类 ×3 轮");
    // 9. 缺属性默认档（asInvoker，explicit=false）。
    let dflt = parse_exec_level("<requestedExecutionLevel />");
    let dflt_ok = matches!(&dflt, Ok(r) if r.level == LVL_AS_INVOKER && !r.explicit && r.virtualization);
    s.add("J1-缺属性默认档", dflt_ok, "Windows 口径：无显式级别视同 asInvoker");
    // 10. 大小写不敏感：词元大小写等价。
    let ci = level_token_to_id("ASINVOKER") == Some(LVL_AS_INVOKER)
        && uiaccess_token_to_bool("TRUE") == Some(true);
    s.add("J1-词元大小写等价", ci, "Windows manifest 口径");
    // 11. 档位越界拒（虚拟化派生闸）。
    s.add("J1-档位越界拒", ExecRecord::derive_virt(3, false) == Err(ElCode::LevelOutOfRange.code()), "level=3 显式拒");
    // 12. 落账账守恒。
    let cons = {
        let mut led = ExecLedger::new();
        let _ = led.feed("<requestedExecutionLevel level=\"asInvoker\" />");
        let _ = led.feed("<requestedExecutionLevel level=\"bogus\" />");
        led.conserve() && led.parsed == 2 && led.rejected == 1
    };
    s.add("J1-落账账守恒", cons, "parsed==recs+rejected");
    // 13. 幂等。
    s.add(
        "J1-解析幂等",
        idempotent_check("<requestedExecutionLevel level=\"highestAvailable\" uiAccess=\"true\" />").is_ok(),
        "同输入同记录",
    );
    // 14. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "注入轮次两跑同值");
    // 15. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7EB40 窗口");
    // 16. 黄金全链。
    let g = golden_run();
    let g_ok = g.is_ok();
    s.add_dyn("J1-黄金全链", g_ok, &format!("golden={:?}", g));
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18446_checks_green() {
        let set = run_unx18446_checks();
        assert!(set.all_passed(), "UNX-F18446 red: {}", set.red_items_text());
    }

    #[test]
    fn t_level_tokens() {
        assert_eq!(level_token_to_id("asInvoker"), Some(0));
        assert_eq!(level_token_to_id("requireAdministrator"), Some(2));
        assert_eq!(level_token_to_id("bogus"), None);
    }

    #[test]
    fn t_attr_find() {
        let tag = b"<requestedExecutionLevel level=\"highestAvailable\" uiAccess=\"true\" />";
        assert_eq!(find_attr(tag, "level").as_deref(), Some("highestAvailable"));
        assert_eq!(find_attr(tag, UIACCESS_TOKEN).as_deref(), Some("true"));
        assert_eq!(find_attr(tag, "trustInfo"), None);
    }

    #[test]
    fn t_injections() {
        assert!(inject_bad_level_token().is_ok());
        assert!(inject_bad_uiaccess().is_ok());
        assert!(inject_empty_value().is_ok());
        assert!(inject_truncated().is_ok());
    }

    #[test]
    fn t_handoff_sig() {
        assert_eq!(handoff_sig_check(B26_HANDOFF_SIG_V1), Ok(()));
        assert_eq!(handoff_sig_check(0), Err(ElCode::HandoffSigMismatch.code()));
    }

    #[test]
    fn t_codes_window() {
        assert!(segment_guard());
        assert_eq!(ElCode::LevelTokenBad.code(), SEG_BASE);
    }
}
