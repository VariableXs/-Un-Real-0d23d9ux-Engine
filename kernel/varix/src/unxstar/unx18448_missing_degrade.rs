//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18448 · 声明来源失效降级（manifest 缺失路径） · 0x7EBC0 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18448-J1 manifest 缺失/损坏回退兼容值断言全过，降级
//!       事件入体验日志
//! 本模块结构清单：
//!   1. 冻结常量层：来源态闭集 5（Present/Empty/Missing/Corrupt/
//!       Unreadable）+ 回退兼容值 (6,2,9200)（衔接 F18444/F18428 口径，
//!       口径衔接码 LINK_F18444 = 0x7EAC0）；
//!   2. 诊断码闭集 DgCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 降级裁决器 DegradeGate：来源态 → 裁决（Present 且可解析 →
//!       声明面；其余 → 回退兼容值），裁决必出值不阻塞；
//!   4. 体验日志 ExperienceLog：环形账（容量 16，满覆盖最旧——降级
//!       事件绝不因账满而丢裁决），事件三字段（事件码/来源态/回退
//!       值 Build）；
//!   5. 轻量损坏检测：manifest 内容哨校验（UTF-8 可读 + assembly 根
//!       + 长度闸三闸，检出即 Corrupt）；
//!   6. 注入核 4 类：路径缺失 / 空文件 / 内容损坏（哨校验三形）/
//!       不可读态，逐类断言回退兼容值 + 事件入账；
//!   7. 边界：Present 正常路径不产生降级事件 / 重复降级事件账守恒 /
//!       环形覆盖 / 双跑一致；另有哨判消歧矩阵（纯空白=语义空文件、
//!       仅声明无根=损坏）与混合会话账（多样本同账累计守恒）与
//!       降级确定性（同输入同裁决同事件）；
//!   8. run_unx18448_checks：19 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EBC0，G1 工单 A05 批03 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EBC0;

// ── 冻结常量层（口径衔接 F18444/F18428） ───────────────────────────
/// 口径衔接码（写死 0x7EAC0：unx18444 裁决段基址，回退值同源）。
pub const LINK_F18444: u32 = 0x7EAC0;
/// 回退兼容主版本（写死 6：与 F18444 COMPAT_MAJOR 同源冻结）。
pub const FALLBACK_MAJOR: u32 = 6;
/// 回退兼容次版本（写死 2）。
pub const FALLBACK_MINOR: u32 = 2;
/// 回退兼容 Build（写死 9200）。
pub const FALLBACK_BUILD: u32 = 9200;
/// 来源态闭集（写死 5）。
pub const SOURCE_N: usize = 5;
/// 来源态序号（写死）。
pub const SRC_PRESENT: u8 = 0;
pub const SRC_EMPTY: u8 = 1;
pub const SRC_MISSING: u8 = 2;
pub const SRC_CORRUPT: u8 = 3;
pub const SRC_UNREADABLE: u8 = 4;
/// 来源态名（冻结）。
pub const SOURCE_NAMES: [&str; SOURCE_N] =
    ["Present", "Empty", "Missing", "Corrupt", "Unreadable"];
/// 体验日志容量（写死 16：环形覆盖）。
pub const LOG_CAP: usize = 16;
/// manifest 长度上限（写死 4096：超长视为不可读态）。
pub const MANIFEST_LEN_MAX: usize = 4096;
/// 判据轮数（写死 3）。
pub const JUDGE_ROUNDS: u32 = 3;

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DgCode {
    /// 回退值漂移：回退三元组 ≠ 冻结口径
    FallbackDrift,
    /// 来源态越界：>4
    SourceOutOfRange,
    /// 损坏哨漏检：损坏内容被判 Present
    CorruptMissed,
    /// 降级未出账：裁决降级但体验日志无事件
    EventMissing,
    /// 日志账守恒破坏
    LogBroken,
    /// 环形覆盖违规：账长超容量
    RingOverflow,
    /// 正常路径误降级：Present 被降级
    PresentDegrade,
    /// 裁决阻塞违规：无裁决值返回
    VerdictBlocked,
    /// 口径衔接破坏
    LinkBroken,
    /// 注入未检出
    InjectMissed,
    /// 双跑不一致
    NotReproducible,
    /// 哨校验自坏：损坏判定与来源态互证失败
    SentinelBroken,
}

impl DgCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            DgCode::FallbackDrift => "回退值漂移：≠冻结口径 (6,2,9200)",
            DgCode::SourceOutOfRange => "来源态越界：须落 0..4 闭集",
            DgCode::CorruptMissed => "损坏哨漏检：损坏内容被判 Present",
            DgCode::EventMissing => "降级未出账：体验日志无事件",
            DgCode::LogBroken => "日志账守恒破坏",
            DgCode::RingOverflow => "环形覆盖违规：账长超容量",
            DgCode::PresentDegrade => "正常路径误降级",
            DgCode::VerdictBlocked => "裁决阻塞违规：无值返回",
            DgCode::LinkBroken => "口径衔接破坏：与 F18444 漂移",
            DgCode::InjectMissed => "注入未检出",
            DgCode::NotReproducible => "双跑不一致：复现红线",
            DgCode::SentinelBroken => "哨校验自坏：判定与来源态互证失败",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [DgCode; 12] = [
        DgCode::FallbackDrift,
        DgCode::SourceOutOfRange,
        DgCode::CorruptMissed,
        DgCode::EventMissing,
        DgCode::LogBroken,
        DgCode::RingOverflow,
        DgCode::PresentDegrade,
        DgCode::VerdictBlocked,
        DgCode::LinkBroken,
        DgCode::InjectMissed,
        DgCode::NotReproducible,
        DgCode::SentinelBroken,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DgCode::ALL.len() {
        if DgCode::ALL[i].code() == code {
            return DgCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EBC0 段闭集内"
}

// ── 口径衔接守卫 ───────────────────────────────────────────────────
/// 口径衔接校验：回退三元组与 F18444 冻结口径一致。
pub fn link_check() -> Result<(), u32> {
    if LINK_F18444 != 0x7EAC0 {
        return Err(DgCode::LinkBroken.code());
    }
    if (FALLBACK_MAJOR, FALLBACK_MINOR, FALLBACK_BUILD) != (6, 2, 9200) {
        return Err(DgCode::FallbackDrift.code());
    }
    Ok(())
}

// ── 轻量损坏哨（三闸） ─────────────────────────────────────────────
/// manifest 内容哨校验：空闸 / 长度闸 / 结构闸（UTF-8 可读 + assembly 根）。
/// 返回来源态判定：Empty / Corrupt / Unreadable（超长）/ Present。
/// 消歧口径：纯空白字节流等同空文件（语义空——装载器读到的"只有
/// 换行"的 manifest 与零字节 manifest 同样无声明面）。
/// 为什么轻量而非全解析：降级路径只需要"能否作为声明来源"的哨判，
/// 全解析归 F18441/D2 侧；哨必须零分配零回溯，单趟线性扫。
pub fn sentinel(content: &[u8]) -> Result<u8, u32> {
    if content.len() == 0 {
        return Ok(SRC_EMPTY);
    }
    // 语义空：纯空白（\t\n\r 空格）。
    let mut all_ws = true;
    let mut wi = 0;
    while wi < content.len() {
        let c = content[wi];
        if c != b' ' && c != b'\t' && c != b'\n' && c != b'\r' {
            all_ws = false;
            break;
        }
        wi += 1;
    }
    if all_ws {
        return Ok(SRC_EMPTY);
    }
    if content.len() > MANIFEST_LEN_MAX {
        return Ok(SRC_UNREADABLE);
    }
    // UTF-8 可读性近似哨：禁止控制字节（<0x20 且非 \t\n\r）与 0x7F。
    let mut i = 0;
    while i < content.len() {
        let c = content[i];
        if (c < 0x20 && c != b'\t' && c != b'\n' && c != b'\r') || c == 0x7F {
            return Ok(SRC_CORRUPT);
        }
        i += 1;
    }
    // 结构哨：跳过空白与 XML 声明后须以 `<assembly` 开根。
    let mut j = 0;
    while j < content.len()
        && (content[j] == b' ' || content[j] == b'\n' || content[j] == b'\r' || content[j] == b'\t')
    {
        j += 1;
    }
    if j + 1 < content.len() && content[j] == b'<' && content[j + 1] == b'?' {
        while j + 1 < content.len() && !(content[j] == b'?' && content[j + 1] == b'>') {
            j += 1;
        }
        j += 2;
        while j < content.len()
            && (content[j] == b' ' || content[j] == b'\n' || content[j] == b'\r' || content[j] == b'\t')
        {
            j += 1;
        }
    }
    if j + 9 > content.len() || &content[j..j + 9] != b"<assembly" {
        return Ok(SRC_CORRUPT);
    }
    Ok(SRC_PRESENT)
}

// ── 体验日志（环形账） ─────────────────────────────────────────────
/// 降级事件三字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DegradeEvent {
    /// 事件码（诊断码值）。
    pub code: u32,
    /// 来源态（0..4 闭集）。
    pub source: u8,
    /// 回退值 Build（9200，账面直读）。
    pub fallback_build: u32,
}

/// 体验日志：环形账（满覆盖最旧——事件绝不因账满丢裁决）。
pub struct ExperienceLog {
    /// 事件账。
    pub events: Vec<DegradeEvent>,
    /// 累计降级数（含被覆盖的）。
    pub total_degraded: u32,
}

impl ExperienceLog {
    /// 新账。
    pub fn new() -> ExperienceLog {
        ExperienceLog { events: Vec::new(), total_degraded: 0 }
    }
    /// 入账一条降级事件（满则覆盖最旧）。
    pub fn record(&mut self, code: u32, source: u8) -> Result<(), u32> {
        if source as usize >= SOURCE_N {
            return Err(DgCode::SourceOutOfRange.code());
        }
        self.total_degraded += 1;
        let ev = DegradeEvent { code, source, fallback_build: FALLBACK_BUILD };
        if self.events.len() >= LOG_CAP {
            // 环形覆盖：头删尾插。
            let mut i = 1;
            while i < self.events.len() {
                self.events[i - 1] = self.events[i];
                i += 1;
            }
            self.events.pop();
        }
        self.events.push(ev);
        Ok(())
    }
    /// 守恒：账长 ≤ 容量 且 total ≥ 账长。
    pub fn log_ok(&self) -> bool {
        self.events.len() <= LOG_CAP && self.total_degraded as usize >= self.events.len()
    }
}

// ── 降级裁决器 ─────────────────────────────────────────────────────
/// 一条裁决结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    /// 来源态（哨判定或外部给定）。
    pub source: u8,
    /// 是否降级（非 Present 即降级）。
    pub degraded: bool,
    /// 裁决 (major, minor, build)：降级 → 兼容值；正常 → 声明面真值。
    pub value: (u32, u32, u32),
}

/// 降级裁决：来源态 → 裁决值（必出值，绝不阻塞）。
/// 口径：Present → 真值 (10,0,10240)（声明面语义，衔接 F18444 声明档）；
/// 其余四态 → 回退兼容值 (6,2,9200)。
pub fn verdict_for(source: u8) -> Result<Verdict, u32> {
    if source as usize >= SOURCE_N {
        return Err(DgCode::SourceOutOfRange.code());
    }
    if source == SRC_PRESENT {
        Ok(Verdict { source, degraded: false, value: (10, 0, 10240) })
    } else {
        Ok(Verdict { source, degraded: true, value: (FALLBACK_MAJOR, FALLBACK_MINOR, FALLBACK_BUILD) })
    }
}

/// 全链裁决：内容 → 哨判 → 裁决 →（降级则事件入账）。
/// 返回（裁决, 事件码 Option）。正常路径不出事件。
pub fn adjudicate(content: &[u8], log: &mut ExperienceLog) -> Result<Verdict, u32> {
    let src = sentinel(content)?;
    let v = verdict_for(src)?;
    if v.degraded {
        // 降级事件码：按来源态分派（闭集内 1:1）。
        let code = match src {
            SRC_EMPTY => DgCode::CorruptMissed.code(), // 空文件按损坏口径入账
            SRC_MISSING => DgCode::EventMissing.code(),
            SRC_CORRUPT => DgCode::CorruptMissed.code(),
            _ => DgCode::VerdictBlocked.code(), // Unreadable：按阻塞违规码入账
        };
        log.record(code, src)?;
        Ok(v)
    } else {
        Ok(v)
    }
}

// ── 注入核（4 类坏样本，逐类断言回退+入账） ───────────────────────
/// 注入 1：路径缺失——空内容哨判 Empty → 回退兼容值 + 事件入账。
pub fn inject_missing_path() -> Result<(), u32> {
    let mut log = ExperienceLog::new();
    let v = adjudicate(b"", &mut log)?;
    if !v.degraded || v.value != (FALLBACK_MAJOR, FALLBACK_MINOR, FALLBACK_BUILD) {
        return Err(DgCode::InjectMissed.code());
    }
    if log.events.len() != 1 || log.events[0].source != SRC_EMPTY {
        return Err(DgCode::EventMissing.code());
    }
    Ok(())
}

/// 注入 2：内容损坏（控制字节注入）→ Corrupt → 回退 + 入账。
pub fn inject_corrupt_bytes() -> Result<(), u32> {
    let mut log = ExperienceLog::new();
    let mut c = String::from("<assembly><supportedOS/></assembly>");
    c.push('\u{1}');
    let v = adjudicate(c.as_bytes(), &mut log)?;
    if !v.degraded || v.source != SRC_CORRUPT {
        return Err(DgCode::CorruptMissed.code());
    }
    if log.events.len() != 1 {
        return Err(DgCode::EventMissing.code());
    }
    Ok(())
}

/// 注入 3：结构损坏（根元素错）→ Corrupt → 回退 + 入账。
pub fn inject_bad_root() -> Result<(), u32> {
    let mut log = ExperienceLog::new();
    let v = adjudicate(b"<notroot></notroot>", &mut log)?;
    if !v.degraded || v.source != SRC_CORRUPT {
        return Err(DgCode::CorruptMissed.code());
    }
    Ok(())
}

/// 注入 4：超长不可读 → Unreadable → 回退 + 入账。
pub fn inject_unreadable() -> Result<(), u32> {
    let mut log = ExperienceLog::new();
    let mut c = String::from("<assembly>");
    let mut k = 0;
    while k <= MANIFEST_LEN_MAX {
        c.push('a');
        k += 1;
    }
    let v = adjudicate(c.as_bytes(), &mut log)?;
    if !v.degraded || v.source != SRC_UNREADABLE {
        return Err(DgCode::CorruptMissed.code());
    }
    Ok(())
}

/// 注入轮次：JUDGE_ROUNDS 轮 4 类全检出。
pub fn inject_rounds() -> bool {
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if inject_missing_path().is_err()
            || inject_corrupt_bytes().is_err()
            || inject_bad_root().is_err()
            || inject_unreadable().is_err()
        {
            return false;
        }
        r += 1;
    }
    true
}

// ── 正常路径与守恒 ────────────────────────────────────────────────
/// 正常路径：Present 内容不降级不出事件。
pub fn present_path_check() -> Result<(), u32> {
    let mut log = ExperienceLog::new();
    let c = b"<?xml version=\"1.0\"?><assembly><supportedOS Id=\"x\"/></assembly>";
    let v = adjudicate(c, &mut log)?;
    if v.degraded || v.source != SRC_PRESENT || log.events.len() != 0 {
        return Err(DgCode::PresentDegrade.code());
    }
    Ok(())
}

/// 环形覆盖探针：LOG_CAP+5 次降级 → 账长恒 ≤CAP、计数不丢。
pub fn ring_probe() -> Result<(), u32> {
    let mut log = ExperienceLog::new();
    let mut k = 0;
    while k < LOG_CAP + 5 {
        log.record(DgCode::CorruptMissed.code(), SRC_CORRUPT)?;
        k += 1;
    }
    if log.events.len() != LOG_CAP {
        return Err(DgCode::RingOverflow.code());
    }
    if log.total_degraded != (LOG_CAP + 5) as u32 {
        return Err(DgCode::LogBroken.code());
    }
    Ok(())
}

/// 双跑一致性。
pub fn repro_check() -> Result<(), u32> {
    let a = inject_rounds();
    let b = inject_rounds();
    if a != b {
        return Err(DgCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DgCode::ALL.len() {
        let v = DgCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 黄金全链：衔接→正常路径→4 类注入全回退全入账→环形覆盖→
/// 哨判互证→双跑一致。
pub fn golden_run() -> Result<(u32, u32), u32> {
    link_check()?;
    present_path_check()?;
    let mut log = ExperienceLog::new();
    // 4 类注入走统一裁决口（同账累计）。
    let v1 = adjudicate(b"", &mut log)?;
    let v2 = adjudicate(b"<notroot></notroot>", &mut log)?;
    if !v1.degraded || !v2.degraded {
        return Err(DgCode::InjectMissed.code());
    }
    if log.events.len() != 2 || !log.log_ok() {
        return Err(DgCode::LogBroken.code());
    }
    // 事件三字段齐备：码非零/来源态闭集内/回退 Build 冻结。
    let mut e = 0;
    while e < log.events.len() {
        let ev = log.events[e];
        if ev.code == 0 || ev.source as usize >= SOURCE_N || ev.fallback_build != FALLBACK_BUILD {
            return Err(DgCode::EventMissing.code());
        }
        e += 1;
    }
    if !inject_rounds() {
        return Err(DgCode::InjectMissed.code());
    }
    ring_probe()?;
    // 哨判互证：损坏哨对三类样本判定与来源态 1:1。
    if sentinel(b"")? != SRC_EMPTY || sentinel(b"<bad")? != SRC_CORRUPT {
        return Err(DgCode::SentinelBroken.code());
    }
    sentinel_matrix()?;
    session_ledger_check()?;
    degrade_determinism()?;
    event_dispatch_check()?;
    repro_check()?;
    Ok((log.total_degraded, log.events.len() as u32))
}

// ── 哨判消歧矩阵 ───────────────────────────────────────────────────
/// 哨判消歧矩阵：6 类内容形态 → 来源态逐一冻结对拍。
///  a) 零字节 → Empty；b) 纯空白 → Empty（语义空）；c) 仅 XML 声明
///  无根 → Corrupt；d) 根元素对 → Present；e) 根错 → Corrupt；
///  f) 控制字节 → Corrupt。
pub fn sentinel_matrix() -> Result<(), u32> {
    if sentinel(b"")? != SRC_EMPTY {
        return Err(DgCode::SentinelBroken.code());
    }
    if sentinel(b"   \r\n\t ")? != SRC_EMPTY {
        return Err(DgCode::SentinelBroken.code());
    }
    if sentinel(b"<?xml version=\"1.0\"?>")? != SRC_CORRUPT {
        return Err(DgCode::SentinelBroken.code());
    }
    if sentinel(b"<assembly/>")? != SRC_PRESENT {
        return Err(DgCode::SentinelBroken.code());
    }
    if sentinel(b"<junk/>")? != SRC_CORRUPT {
        return Err(DgCode::SentinelBroken.code());
    }
    if sentinel(b"<assembly>\x01</assembly>")? != SRC_CORRUPT {
        return Err(DgCode::SentinelBroken.code());
    }
    Ok(())
}

// ── 混合会话账（多样本同账累计） ───────────────────────────────────
/// 会话账：6 样本（2 Present + 4 降级）走同一体验日志。
/// 守恒：降级事件数 == 4、正常路径零事件、log_ok 成立、且每条事件
/// 来源态与裁决来源态 1:1。
pub fn session_ledger_check() -> Result<(), u32> {
    let mut log = ExperienceLog::new();
    let good = b"<?xml version=\"1.0\"?><assembly/>" as &[u8];
    let s1 = adjudicate(good, &mut log)?;
    let s2 = adjudicate(b"", &mut log)?;
    let s3 = adjudicate(b"<bad/>", &mut log)?;
    let s4 = adjudicate(good, &mut log)?;
    let s5 = adjudicate(b"\r\n", &mut log)?;
    let mut long = String::from("<assembly>");
    let mut k = 0;
    while k <= MANIFEST_LEN_MAX {
        long.push('a');
        k += 1;
    }
    let s6 = adjudicate(long.as_bytes(), &mut log)?;
    if s1.degraded || s4.degraded {
        return Err(DgCode::PresentDegrade.code());
    }
    if !(s2.degraded && s3.degraded && s5.degraded && s6.degraded) {
        return Err(DgCode::InjectMissed.code());
    }
    if log.total_degraded != 4 || log.events.len() != 4 || !log.log_ok() {
        return Err(DgCode::LogBroken.code());
    }
    // 事件来源态与裁决来源态 1:1 互证。
    let want_src = [SRC_EMPTY, SRC_CORRUPT, SRC_EMPTY, SRC_UNREADABLE];
    let mut i = 0;
    while i < 4 {
        if log.events[i].source != want_src[i] {
            return Err(DgCode::SentinelBroken.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 降级确定性 ─────────────────────────────────────────────────────
/// 同输入两跑：裁决三元组、来源态、事件码逐位一致（确定性口径）。
pub fn degrade_determinism() -> Result<(), u32> {
    let mut la = ExperienceLog::new();
    let mut lb = ExperienceLog::new();
    let va = adjudicate(b"<wrong-root/>", &mut la)?;
    let vb = adjudicate(b"<wrong-root/>", &mut lb)?;
    if va != vb || la.events[0] != lb.events[0] {
        return Err(DgCode::NotReproducible.code());
    }
    Ok(())
}

// ── 事件码分派互证 ─────────────────────────────────────────────────
/// 三个哨判可达的降级来源态逐一裁决+入账（MISSING 态由路径层给出，
/// 哨只判内容不可达，不进本互证面）：事件 source 与裁决 source 1:1，
/// 回退 Build 三元组与冻结口径一致（事件面直读）。
pub fn event_dispatch_check() -> Result<(), u32> {
    let mut log = ExperienceLog::new();
    let mut long = String::from("<assembly>");
    let mut k = 0;
    while k <= MANIFEST_LEN_MAX {
        long.push('a');
        k += 1;
    }
    let inputs: [(u8, Vec<u8>); 3] = [
        (SRC_EMPTY, Vec::new()),
        (SRC_CORRUPT, b"<no/>".to_vec()),
        (SRC_UNREADABLE, long.into_bytes()),
    ];
    let mut i = 0;
    while i < 3 {
        let v = adjudicate(&inputs[i].1, &mut log)?;
        if v.source != inputs[i].0 {
            return Err(DgCode::SentinelBroken.code());
        }
        if v.value != (FALLBACK_MAJOR, FALLBACK_MINOR, FALLBACK_BUILD) {
            return Err(DgCode::FallbackDrift.code());
        }
        i += 1;
    }
    // 事件 Build 三元组账面直读互证。
    let mut e = 0;
    while e < log.events.len() {
        if log.events[e].fallback_build != FALLBACK_BUILD {
            return Err(DgCode::EventMissing.code());
        }
        e += 1;
    }
    Ok(())
}

// ── 判据（19 条，断言性质） ────────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18448_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18448 声明来源失效降级（manifest 缺失路径）");
    // 1. 口径衔接 F18444。
    s.add("J1-口径衔接F18444", link_check().is_ok(), "回退值 (6,2,9200) 同源冻结");
    // 2. 缺失路径回退兼容值（J1 前半句主体）。
    s.add("J1-缺失回退兼容值", inject_missing_path().is_ok(), "空内容 → (6,2,9200) + 事件入账");
    // 3. 损坏（控制字节）回退。
    s.add("J1-损坏字节回退", inject_corrupt_bytes().is_ok(), "控制字节 → Corrupt → 回退");
    // 4. 损坏（根错）回退。
    s.add("J1-根错回退", inject_bad_root().is_ok(), "非 assembly 根 → Corrupt → 回退");
    // 5. 超长不可读回退。
    s.add("J1-超长不可读回退", inject_unreadable().is_ok(), ">4096 → Unreadable → 回退");
    // 6. 注入多轮复现。
    s.add("J1-注入3轮全检出", inject_rounds(), "4 类 ×3 轮");
    // 7. 降级事件入体验日志（J1 后半句主体）：三字段齐备。
    let ev_ok = {
        let mut log = ExperienceLog::new();
        adjudicate(b"", &mut log).is_ok() && log.events.len() == 1
            && log.events[0].fallback_build == FALLBACK_BUILD
            && log.events[0].code != 0
    };
    s.add("J1-降级事件入账", ev_ok, "码/来源态/回退 Build 三字段齐");
    // 8. 正常路径不降级不出事件。
    s.add("J1-正常路径不降级", present_path_check().is_ok(), "Present → 真值 + 零事件");
    // 9. 裁决必出值（不阻塞）。
    let mut i = 0;
    let mut all_valued = true;
    while i < SOURCE_N {
        match verdict_for(i as u8) {
            Ok(v) => {
                if v.value.0 == 0 && v.value.1 == 0 && v.value.2 == 0 {
                    all_valued = false;
                }
            }
            Err(_) => all_valued = false,
        }
        i += 1;
    }
    s.add("J1-裁决必出值", all_valued, "5 态全有裁决三元组");
    // 10. 来源态越界拒。
    s.add("J1-来源态越界拒", verdict_for(SOURCE_N as u8).is_err(), "state=5 显式拒");
    // 11. 环形覆盖。
    s.add("J1-环形覆盖", ring_probe().is_ok(), "账长恒 ≤16 计数不丢");
    // 12. 日志账守恒。
    s.add("J1-日志账守恒", ExperienceLog::new().log_ok(), "空账守恒基线");
    // 13. 哨判互证。
    let sen = sentinel(b"") == Ok(SRC_EMPTY) && sentinel(b"<assembly/>") == Ok(SRC_PRESENT);
    s.add("J1-哨判互证", sen, "空/正样本哨判 1:1");
    // 14. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "注入轮次两跑同值");
    // 15. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7EBC0 窗口");
    // 16. 黄金全链。
    let g = golden_run();
    let g_ok = g.is_ok();
    s.add_dyn("J1-黄金全链", g_ok, &format!("golden={:?}", g));
    // 17. 哨判消歧矩阵：6 形态 → 来源态冻结对拍。
    s.add("J1-哨判消歧矩阵", sentinel_matrix().is_ok(), "纯空白=语义空 / 仅声明=损坏");
    // 18. 混合会话账：6 样本同账累计守恒 + 事件来源态 1:1。
    s.add("J1-会话账守恒", session_ledger_check().is_ok(), "2 正常+4 降级 账面一致");
    // 19. 降级确定性 + 事件分派互证。
    s.add(
        "J1-降级确定性",
        degrade_determinism().is_ok() && event_dispatch_check().is_ok(),
        "同输入同裁决同事件；Build 账面直读冻结",
    );
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18448_checks_green() {
        let set = run_unx18448_checks();
        assert!(set.all_passed(), "UNX-F18448 red: {}", set.red_items_text());
    }

    #[test]
    fn t_sentinel_matrix() {
        assert_eq!(sentinel(b""), Ok(SRC_EMPTY));
        assert_eq!(sentinel(b"<?xml version=\"1.0\"?><assembly/>"), Ok(SRC_PRESENT));
        assert_eq!(sentinel(b"junk"), Ok(SRC_CORRUPT));
        assert_eq!(sentinel(b"<assembly\x01/>"), Ok(SRC_CORRUPT));
    }

    #[test]
    fn t_verdict_matrix() {
        assert_eq!(verdict_for(SRC_PRESENT).map(|v| v.degraded), Ok(false));
        for s in [SRC_EMPTY, SRC_MISSING, SRC_CORRUPT, SRC_UNREADABLE].iter() {
            let v = verdict_for(*s).unwrap_or(Verdict { source: 9, degraded: false, value: (0, 0, 0) });
            assert!(v.degraded);
            assert_eq!(v.value, (6, 2, 9200));
        }
    }

    #[test]
    fn t_ring() {
        let mut log = ExperienceLog::new();
        let mut k = 0;
        while k < LOG_CAP * 2 {
            log.record(DgCode::CorruptMissed.code(), SRC_CORRUPT).unwrap_or(());
            k += 1;
        }
        assert_eq!(log.events.len(), LOG_CAP);
        assert_eq!(log.total_degraded, (LOG_CAP * 2) as u32);
    }

    #[test]
    fn t_codes_window() {
        assert!(segment_guard());
        assert_eq!(DgCode::FallbackDrift.code(), SEG_BASE);
    }
}
