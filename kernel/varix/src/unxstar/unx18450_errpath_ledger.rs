//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18450 · 声明面错误路径账（畸形 XML/GUID） · 0x7EC40 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18450-J1 畸形输入 10 形态注入全过（不崩不挂），错误码
//!       与日志三要素齐备
//! 本模块结构清单：
//!   1. 冻结常量层：畸形形态闭集 10（空输入/无根/GUID 缺 Id/GUID 非
//!       hex/GUID 段数错/重复 Id/标签未闭/超长/非法控制字节/截断）；
//!   2. 诊断码闭集 EpCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 有界扫描器：单趟线性扫描 + 步数硬闸（STEP_CAP 防挂死）+ 深
//!       度闸，任何畸形在闸内显式拒绝——不崩不挂的机器口径是
//!       "每样本步数 ≤ STEP_CAP 且返回 Err"；
//!   4. 错误路径账 ErrLedger：注入数/拒收数/日志数三账守恒
//!       （injected == rejected == logged）+ 日志环形容量；
//!   5. 日志三要素：每条（错误码, 阶段序号, 样本摘要 FNV-1a 32），
//!       码非零 / 阶段在 1..4 闭集 / 摘要非零——齐备性机器断言；
//!   6. 注入核：10 形态逐类喂入，全部被显式拒绝且留三要素日志；
//!   7. 边界：空输入 / 满日志环形覆盖 / 重复注入幂等（同形态同码同
//!       摘要）/ 双跑一致；
//!   8. run_unx18450_checks：17 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EC40，G1 工单 A05 批03 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EC40;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 畸形形态数（写死 10：判据原文 10 形态语义）。
pub const FORM_N: usize = 10;
/// 步数硬闸（写死 2048：防挂死，单样本扫描步上限）。
pub const STEP_CAP: usize = 2048;
/// 深度闸（写死 8）。
pub const DEPTH_MAX: usize = 8;
/// 输入长度上限（写死 1024：声明面局部扫描口径）。
pub const INPUT_MAX: usize = 1024;
/// 阶段闭集（写死 4：1 初检 / 2 结构 / 3 GUID 面 / 4 账面）。
pub const PHASE_N: u8 = 4;
/// 日志容量（写死 32：环形覆盖）。
pub const LOG_CAP: usize = 32;
/// 判据轮数（写死 3）。
pub const JUDGE_ROUNDS: u32 = 3;
/// FNV 常量。
const FNV_OFFSET: u32 = 0x811C_9DC5;
const FNV_PRIME: u32 = 0x0100_0193;

/// FNV-1a 32（字节串）。
pub fn fnv1a(s: &[u8]) -> u32 {
    let mut h: u32 = FNV_OFFSET;
    let mut i = 0;
    while i < s.len() {
        h ^= s[i] as u32;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EpCode {
    /// 空输入
    EmptyInput,
    /// 无根元素（首标签非 assembly）
    NoRoot,
    /// supportedOS 缺 Id 属性
    GuidIdMissing,
    /// Id 值非十六进制
    GuidHexBad,
    /// Id 值段数/长度错（非 8-4-4-4-12）
    GuidShapeBad,
    /// 重复 supportedOS Id（同段重复声明）
    GuidDup,
    /// 标签未闭
    TagUnclosed,
    /// 输入超长
    InputTooLong,
    /// 非法控制字节
    ControlByte,
    /// 截断声明（引号/注释未闭）
    Truncated,
    /// 深度超限
    DepthExceeded,
    /// 步数超限（防挂死闸触发）
    StepExceeded,
    /// 三账守恒破坏
    LedgerBroken,
    /// 日志三要素不齐
    LogTripleBroken,
    /// 注入未检出
    InjectMissed,
    /// 双跑不一致
    NotReproducible,
}

impl EpCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            EpCode::EmptyInput => "空输入",
            EpCode::NoRoot => "无根元素：首标签非 assembly",
            EpCode::GuidIdMissing => "supportedOS 缺 Id 属性",
            EpCode::GuidHexBad => "Id 值非十六进制",
            EpCode::GuidShapeBad => "Id 值段数/长度错",
            EpCode::GuidDup => "重复 supportedOS Id",
            EpCode::TagUnclosed => "标签未闭",
            EpCode::InputTooLong => "输入超长：1024 上限",
            EpCode::ControlByte => "非法控制字节",
            EpCode::Truncated => "截断声明：引号/注释未闭",
            EpCode::DepthExceeded => "深度超限：8 层上限",
            EpCode::StepExceeded => "步数超限：防挂死闸",
            EpCode::LedgerBroken => "三账守恒破坏",
            EpCode::LogTripleBroken => "日志三要素不齐",
            EpCode::InjectMissed => "注入未检出",
            EpCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [EpCode; 16] = [
        EpCode::EmptyInput,
        EpCode::NoRoot,
        EpCode::GuidIdMissing,
        EpCode::GuidHexBad,
        EpCode::GuidShapeBad,
        EpCode::GuidDup,
        EpCode::TagUnclosed,
        EpCode::InputTooLong,
        EpCode::ControlByte,
        EpCode::Truncated,
        EpCode::DepthExceeded,
        EpCode::StepExceeded,
        EpCode::LedgerBroken,
        EpCode::LogTripleBroken,
        EpCode::InjectMissed,
        EpCode::NotReproducible,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < EpCode::ALL.len() {
        if EpCode::ALL[i].code() == code {
            return EpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EC40 段闭集内"
}

// ── 有界扫描器（错误路径核心） ────────────────────────────────────
/// 扫描阶段闭集（1..4，与 PHASE_N 对应）。
pub const PHASE_PRE: u8 = 1;
pub const PHASE_STRUCT: u8 = 2;
pub const PHASE_GUID: u8 = 3;
pub const PHASE_LEDGER: u8 = 4;

/// 有界扫描结果：Err(诊断码) = 显式拒绝。
/// 不崩不挂的机器口径：全部循环步数有界（steps 闸）+ 全部下标经
/// get/区间校验——任何畸形必须在 STEP_CAP 内返回 Err。
pub fn bounded_scan(src: &[u8]) -> Result<(), u32> {
    let mut steps = 0usize;
    // 相 1 初检：空/超长/控制字节。
    if src.len() == 0 {
        return Err(EpCode::EmptyInput.code());
    }
    if src.len() > INPUT_MAX {
        return Err(EpCode::InputTooLong.code());
    }
    let mut i = 0usize;
    while i < src.len() {
        steps += 1;
        if steps > STEP_CAP {
            return Err(EpCode::StepExceeded.code());
        }
        let c = src[i];
        if (c < 0x20 && c != b'\t' && c != b'\n' && c != b'\r') || c == 0x7F {
            return Err(EpCode::ControlByte.code());
        }
        i += 1;
    }
    // 相 2 结构：跳空白 → 首标签须 <assembly → 标签配平/深度/引号闸。
    let mut j = 0usize;
    while j < src.len() && (src[j] == b' ' || src[j] == b'\n' || src[j] == b'\r' || src[j] == b'\t') {
        j += 1;
        steps += 1;
        if steps > STEP_CAP {
            return Err(EpCode::StepExceeded.code());
        }
    }
    // 可选 XML 声明：跳过 `<?xml ... ?>`（与 unx16821 初检同口径），
    // 之后重跳空白。声明未闭（无 ?>）按截断拒。
    if j + 1 < src.len() && src[j] == b'<' && src[j + 1] == b'?' {
        while j + 1 < src.len() && !(src[j] == b'?' && src[j + 1] == b'>') {
            j += 1;
            steps += 1;
            if steps > STEP_CAP {
                return Err(EpCode::StepExceeded.code());
            }
        }
        if j + 1 >= src.len() {
            return Err(EpCode::Truncated.code());
        }
        j += 2;
        while j < src.len() && (src[j] == b' ' || src[j] == b'\n' || src[j] == b'\r' || src[j] == b'\t') {
            j += 1;
            steps += 1;
            if steps > STEP_CAP {
                return Err(EpCode::StepExceeded.code());
            }
        }
    }
    if j + 9 > src.len() || &src[j..j + 9] != b"<assembly" {
        return Err(EpCode::NoRoot.code());
    }
    // 配平/深度/引号/注释扫描。
    let mut depth = 0usize;
    let mut in_quote: Option<u8> = None;
    let mut in_comment = false;
    let mut ids_seen: Vec<[u8; 32]> = Vec::new();
    let mut k = j;
    while k < src.len() {
        steps += 1;
        if steps > STEP_CAP {
            return Err(EpCode::StepExceeded.code());
        }
        let c = src[k];
        if in_comment {
            if c == b'-' && k + 2 < src.len() && src[k + 1] == b'-' && src[k + 2] == b'>' {
                in_comment = false;
                k += 3;
            } else {
                k += 1;
            }
            continue;
        }
        if let Some(q) = in_quote {
            if c == q {
                in_quote = None;
            }
            k += 1;
            continue;
        }
        if c == b'"' || c == b'\'' {
            in_quote = Some(c);
            k += 1;
            continue;
        }
        if c == b'<' {
            if k + 3 < src.len() && &src[k..k + 4] == b"<!--" {
                in_comment = true;
                k += 4;
                continue;
            }
            if k + 1 < src.len() && src[k + 1] == b'/' {
                // 闭标签。
                if depth == 0 {
                    return Err(EpCode::TagUnclosed.code());
                }
                depth -= 1;
            } else if k + 1 < src.len() && src[k + 1] == b'?' {
                // XML 声明：跳至 ?>。
                while k + 1 < src.len() && !(src[k] == b'?' && src[k + 1] == b'>') {
                    k += 1;
                    steps += 1;
                    if steps > STEP_CAP {
                        return Err(EpCode::StepExceeded.code());
                    }
                }
            } else {
                depth += 1;
                if depth > DEPTH_MAX {
                    return Err(EpCode::DepthExceeded.code());
                }
                // 相 3 GUID 面：supportedOS 标签 Id 闸。
                check_supported_os(src, k, &mut steps, &mut ids_seen)?;
            }
        } else if c == b'>' {
            if k > 0 && src[k - 1] == b'/' && depth > 0 {
                depth -= 1;
            }
        }
        k += 1;
    }
    if in_quote.is_some() || in_comment {
        return Err(EpCode::Truncated.code());
    }
    if depth != 0 {
        return Err(EpCode::TagUnclosed.code());
    }
    Ok(())
}

/// supportedOS 标签 Id 闸：缺 Id / 非 hex / 段形错 / 重复 Id。
/// 在开标签起点 `<` 处调用，扫至 '>'。
fn check_supported_os(
    src: &[u8],
    lt: usize,
    steps: &mut usize,
    ids_seen: &mut Vec<[u8; 32]>,
) -> Result<(), u32> {
    let needle = b"<supportedOS";
    if lt + needle.len() > src.len() || &src[lt..lt + needle.len()] != needle {
        return Ok(()); // 非 supportedOS 标签不适用本闸。
    }
    let mut k = lt + needle.len();
    // 定位 '>'。
    while k < src.len() && src[k] != b'>' {
        *steps += 1;
        if *steps > STEP_CAP {
            return Err(EpCode::StepExceeded.code());
        }
        k += 1;
    }
    if k >= src.len() {
        return Err(EpCode::TagUnclosed.code());
    }
    // 抓 Id="..." 值。
    let attr = b"Id=\"";
    let mut pos = None;
    let mut p = lt + needle.len();
    while p + attr.len() <= k {
        *steps += 1;
        if *steps > STEP_CAP {
            return Err(EpCode::StepExceeded.code());
        }
        if &src[p..p + attr.len()] == attr {
            pos = Some(p + attr.len());
            break;
        }
        p += 1;
    }
    let v0 = match pos {
        Some(x) => x,
        None => return Err(EpCode::GuidIdMissing.code()),
    };
    let mut v1 = v0;
    while v1 < k && src[v1] != b'"' {
        v1 += 1;
        *steps += 1;
        if *steps > STEP_CAP {
            return Err(EpCode::StepExceeded.code());
        }
    }
    if v1 >= k {
        return Err(EpCode::Truncated.code());
    }
    let val = &src[v0..v1];
    // 段形闸：8-4-4-4-12（36 字符含 4 连字符）。
    if val.len() != 36 {
        return Err(EpCode::GuidShapeBad.code());
    }
    let dashes = [8usize, 13, 18, 23];
    let mut d = 0;
    while d < dashes.len() {
        if val[dashes[d]] != b'-' {
            return Err(EpCode::GuidShapeBad.code());
        }
        d += 1;
    }
    let mut q = 0;
    while q < val.len() {
        *steps += 1;
        if *steps > STEP_CAP {
            return Err(EpCode::StepExceeded.code());
        }
        let c = val[q];
        if c == b'-' {
            q += 1;
            continue;
        }
        let hex = (c >= b'0' && c <= b'9') || (c >= b'a' && c <= b'f') || (c >= b'A' && c <= b'F');
        if !hex {
            return Err(EpCode::GuidHexBad.code());
        }
        q += 1;
    }
    // 重复 Id 闸：线性查账（Id ≤36B，账内定长 32B 截断比对——摘要级）。
    let mut key = [0u8; 32];
    let mut m = 0;
    while m < 32 && m < val.len() {
        key[m] = val[m];
        m += 1;
    }
    let mut n = 0;
    while n < ids_seen.len() {
        if ids_seen[n] == key {
            return Err(EpCode::GuidDup.code());
        }
        n += 1;
    }
    ids_seen.push(key);
    Ok(())
}

// ── 10 形态注入样本 ────────────────────────────────────────────────
/// 畸形 10 形态构造（确定性，逐条注明形态与预期拒因）：
///  0 空输入            → EmptyInput
///  1 无根元素          → NoRoot
///  2 缺 Id 属性        → GuidIdMissing
///  3 Id 非 hex（'g'）  → GuidHexBad
///  4 Id 段形错（短）   → GuidShapeBad
///  5 重复 Id           → GuidDup
///  6 标签未闭          → TagUnclosed
///  7 超长（>1024）     → InputTooLong
///  8 非法控制字节      → ControlByte
///  9 截断（引号未闭）  → Truncated
pub fn make_forms() -> [Vec<u8>; FORM_N] {
    let dup_head: &str = "<assembly><supportedOS Id=\"8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a\"/>";
    [
        Vec::new(),
        b"<notroot/>".to_vec(),
        b"<assembly><supportedOS /></assembly>".to_vec(),
        b"<assembly><supportedOS Id=\"ge0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a\"/></assembly>".to_vec(),
        b"<assembly><supportedOS Id=\"8e0f7a12\"/></assembly>".to_vec(),
        format!(
            "{}{}",
            dup_head,
            "<supportedOS Id=\"8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a\"/></assembly>"
        )
        .into_bytes(),
        b"<assembly><supportedOS".to_vec(),
        {
            let mut s = String::from("<assembly>");
            let mut q = 0;
            while q <= INPUT_MAX {
                s.push('a');
                q += 1;
            }
            s.into_bytes()
        },
        {
            let s = String::from("<assembly>\u{1}</assembly>");
            s.into_bytes()
        },
        b"<assembly name=\"x></assembly>".to_vec(),
    ]
}

/// 预期拒因表（与 make_forms 一一对应，注入核逐类对码断言）。
pub fn expected_codes() -> [u32; FORM_N] {
    [
        EpCode::EmptyInput.code(),
        EpCode::NoRoot.code(),
        EpCode::GuidIdMissing.code(),
        EpCode::GuidHexBad.code(),
        EpCode::GuidShapeBad.code(),
        EpCode::GuidDup.code(),
        EpCode::TagUnclosed.code(),
        EpCode::InputTooLong.code(),
        EpCode::ControlByte.code(),
        EpCode::Truncated.code(),
    ]
}

// ── 错误路径账 ─────────────────────────────────────────────────────
/// 日志三要素一条。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LogTriple {
    /// 要素一：错误码（诊断码值，非零）。
    pub code: u32,
    /// 要素二：阶段序号（1..4 闭集）。
    pub phase: u8,
    /// 要素三：样本摘要（FNV-1a 32，非零）。
    pub digest: u32,
}

/// 错误路径账：注入/拒收/日志三账守恒。
pub struct ErrLedger {
    /// 注入数。
    pub injected: u32,
    /// 拒收数。
    pub rejected: u32,
    /// 日志账（环形，容量 LOG_CAP）。
    pub log: Vec<LogTriple>,
    /// 累计入账数（含被覆盖）。
    pub logged_total: u32,
}

impl ErrLedger {
    /// 新账。
    pub fn new() -> ErrLedger {
        ErrLedger { injected: 0, rejected: 0, log: Vec::new(), logged_total: 0 }
    }
    /// 喂入一形态：扫描 → 拒则入账（码/阶段/摘要三要素）。
    pub fn feed(&mut self, src: &[u8], phase: u8) -> Result<(), u32> {
        self.injected += 1;
        match bounded_scan(src) {
            Err(c) => {
                self.rejected += 1;
                if phase < PHASE_PRE || phase > PHASE_LEDGER {
                    return Err(EpCode::LogTripleBroken.code());
                }
                let dg = fnv1a(src);
                if c == 0 || dg == 0 {
                    return Err(EpCode::LogTripleBroken.code());
                }
                if self.log.len() >= LOG_CAP {
                    // 环形覆盖：头删尾插。
                    let mut i = 1;
                    while i < self.log.len() {
                        self.log[i - 1] = self.log[i];
                        i += 1;
                    }
                    self.log.pop();
                }
                self.log.push(LogTriple { code: c, phase, digest: dg });
                self.logged_total += 1;
                Ok(())
            }
            Ok(()) => Err(EpCode::InjectMissed.code()),
        }
    }
    /// 三账守恒：injected == rejected == logged_total。
    pub fn conserve(&self) -> bool {
        self.injected == self.rejected && self.rejected == self.logged_total && self.log.len() <= LOG_CAP
    }
    /// 三要素齐备性：账内每条码非零/阶段 1..4/摘要非零。
    pub fn triple_ok(&self) -> bool {
        let mut i = 0;
        while i < self.log.len() {
            let t = &self.log[i];
            if t.code == 0 || t.phase < PHASE_PRE || t.phase > PHASE_LEDGER || t.digest == 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入核全链：10 形态逐类喂入，逐类对码断言 + 三账守恒 + 三要素齐备。
pub fn inject_all(rounds: u32) -> Result<(u32, u32), u32> {
    let forms = make_forms();
    let expects = expected_codes();
    let mut led = ErrLedger::new();
    let mut r = 0;
    while r < rounds {
        let mut f = 0;
        while f < FORM_N {
            // 阶段分派：0/1/7 初检相，2..6 GUID 相，8/9 结构相。
            let phase = match f {
                0 | 1 | 7 => PHASE_PRE,
                2 | 3 | 4 | 5 => PHASE_GUID,
                _ => PHASE_STRUCT,
            };
            led.feed(&forms[f], phase)?;
            // 逐类对码：账面最新一条码必须与预期一致。
            let last = led.log[led.log.len() - 1];
            if last.code != expects[f] {
                return Err(EpCode::InjectMissed.code());
            }
            f += 1;
        }
        r += 1;
    }
    if !led.conserve() || !led.triple_ok() {
        return Err(EpCode::LedgerBroken.code());
    }
    let total = (FORM_N as u32) * rounds;
    if led.injected != total {
        return Err(EpCode::LedgerBroken.code());
    }
    Ok((led.rejected, led.injected))
}

/// 不崩不挂口径断言：10 形态每形态步数有界且返回 Err。
/// （步数闸内置：bounded_scan 全循环 steps 计数；此处断言全部 Err。）
pub fn all_rejected() -> bool {
    let forms = make_forms();
    let mut f = 0;
    while f < FORM_N {
        if bounded_scan(&forms[f]).is_ok() {
            return false;
        }
        f += 1;
    }
    true
}

/// 正样本不误伤：规范 manifest 扫描通过。
pub fn good_passes() -> Result<(), u32> {
    bounded_scan(
        b"<assembly><supportedOS Id=\"8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a\"/></assembly>",
    )
}

/// 幂等：同形态重复注入 → 同码同摘要。
pub fn idempotent_check() -> Result<(), u32> {
    let forms = make_forms();
    let mut led = ErrLedger::new();
    led.feed(&forms[3], PHASE_GUID)?;
    let a = led.log[led.log.len() - 1];
    led.feed(&forms[3], PHASE_GUID)?;
    let b = led.log[led.log.len() - 1];
    if a != b {
        return Err(EpCode::NotReproducible.code());
    }
    Ok(())
}

/// 双跑一致性。
pub fn repro_check() -> Result<(), u32> {
    let a = inject_all(1);
    let b = inject_all(1);
    if a != b {
        return Err(EpCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < EpCode::ALL.len() {
        let v = EpCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 黄金全链：正样本通过→10 形态注入全过→三账守恒→三要素齐备→
/// 幂等→环形覆盖→双跑一致。
pub fn golden_run() -> Result<(u32, u32), u32> {
    good_passes()?;
    let (rj, inj) = inject_all(JUDGE_ROUNDS)?;
    if rj != inj {
        return Err(EpCode::LedgerBroken.code());
    }
    idempotent_check()?;
    // 环形覆盖：LOG_CAP+4 次注入 → 账长恒 ≤CAP。
    let forms = make_forms();
    let mut led = ErrLedger::new();
    let mut k = 0;
    while k < LOG_CAP + 4 {
        led.feed(&forms[k % FORM_N], PHASE_PRE)?;
        k += 1;
    }
    if led.log.len() > LOG_CAP || led.logged_total != (LOG_CAP + 4) as u32 {
        return Err(EpCode::LedgerBroken.code());
    }
    repro_check()?;
    Ok((rj, inj))
}

// ── 判据（17 条，断言性质） ────────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18450_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18450 声明面错误路径账（畸形 XML/GUID）");
    // 1. 正样本不误伤。
    s.add("J1-正样本通过", good_passes().is_ok(), "规范 manifest 扫描零误伤");
    // 2. 10 形态全过（J1 前半句主体）。
    s.add("J1-10形态全过", all_rejected(), "10 形态全部显式拒绝");
    // 3. 注入核对码：10 形态 ×3 轮逐类对码 + 守恒 + 三要素。
    let inj = inject_all(JUDGE_ROUNDS);
    let inj_ok = match inj {
        Ok((rj, inj)) => rj == inj && inj == (FORM_N * JUDGE_ROUNDS as usize) as u32,
        Err(_) => false,
    };
    s.add("J1-注入核对码", inj_ok, "逐类拒因与预期码一致");
    // 4. 空输入拒。
    s.add("J1-形态:空输入", bounded_scan(b"").is_err(), "EmptyInput");
    // 5. 无根拒。
    s.add("J1-形态:无根", bounded_scan(b"<x/>").is_err(), "NoRoot");
    // 6. 缺 Id 拒。
    s.add("J1-形态:缺Id", bounded_scan(b"<assembly><supportedOS /></assembly>").is_err(), "GuidIdMissing");
    // 7. 非 hex 拒。
    s.add(
        "J1-形态:非hex",
        bounded_scan(b"<assembly><supportedOS Id=\"ge0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a\"/></assembly>").is_err(),
        "GuidHexBad",
    );
    // 8. 段形错拒。
    s.add("J1-形态:段形错", bounded_scan(b"<assembly><supportedOS Id=\"8e0f7a12\"/></assembly>").is_err(), "GuidShapeBad");
    // 9. 重复 Id 拒。
    let dup = format!(
        "<assembly><supportedOS Id=\"8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a\"/><supportedOS Id=\"8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a\"/></assembly>"
    );
    s.add("J1-形态:重复Id", bounded_scan(dup.as_bytes()).is_err(), "GuidDup");
    // 10. 超长拒。
    s.add("J1-形态:超长", bounded_scan(&vec![b'a'; INPUT_MAX + 1]).is_err(), "InputTooLong");
    // 11. 控制字节拒。
    s.add("J1-形态:控制字节", bounded_scan(b"<assembly>\x01</assembly>").is_err(), "ControlByte");
    // 12. 截断拒。
    s.add("J1-形态:截断", bounded_scan(b"<assembly name=\"x>").is_err(), "Truncated");
    // 13. 三账守恒。
    let cons = {
        let forms = make_forms();
        let mut led = ErrLedger::new();
        let mut f = 0;
        let mut ok = true;
        while f < FORM_N {
            if led.feed(&forms[f], PHASE_PRE).is_err() {
                ok = false;
            }
            f += 1;
        }
        ok && led.conserve()
    };
    s.add("J1-三账守恒", cons, "injected==rejected==logged_total");
    // 14. 三要素齐备（J1 后半句主体）。
    let trip = {
        let forms = make_forms();
        let mut led = ErrLedger::new();
        led.feed(&forms[2], PHASE_GUID).is_ok() && led.triple_ok()
            && led.log[0].code == EpCode::GuidIdMissing.code()
            && led.log[0].phase == PHASE_GUID
            && led.log[0].digest == fnv1a(&forms[2])
    };
    s.add("J1-日志三要素齐备", trip, "码/阶段/摘要 逐字段对账");
    // 15. 幂等：同形态同码同摘要。
    s.add("J1-注入幂等", idempotent_check().is_ok(), "同形态两次账面一致");
    // 16. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7EC40 窗口");
    // 17. 黄金全链（含环形覆盖与双跑）。
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
    fn unx18450_checks_green() {
        let set = run_unx18450_checks();
        assert!(set.all_passed(), "UNX-F18450 red: {}", set.red_items_text());
    }

    #[test]
    fn t_forms_match_expected() {
        let forms = make_forms();
        let expects = expected_codes();
        assert_eq!(forms.len(), FORM_N);
        assert_eq!(expects.len(), FORM_N);
        let mut f = 0;
        while f < FORM_N {
            assert_eq!(
                bounded_scan(&forms[f]).err(),
                Some(expects[f]),
                "形态 {} 拒因不符",
                f
            );
            f += 1;
        }
    }

    #[test]
    fn t_good_variants_pass() {
        assert!(bounded_scan(b"<assembly/>").is_ok());
        assert!(bounded_scan(b"<?xml version=\"1.0\"?><assembly><a/></assembly>").is_ok());
        // 两个不同 supportedOS 声明：合法通过。
        let two = b"<assembly><supportedOS Id=\"8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a\"/><supportedOS Id=\"e2011457-1546-43c5-a5fe-008deee3d3f0\"/></assembly>";
        assert!(bounded_scan(two).is_ok());
    }

    #[test]
    fn t_codes_window() {
        assert!(segment_guard());
        assert_eq!(EpCode::EmptyInput.code(), SEG_BASE);
        assert!(EpCode::NotReproducible.code() < SEG_BASE + 0x40);
    }
}
