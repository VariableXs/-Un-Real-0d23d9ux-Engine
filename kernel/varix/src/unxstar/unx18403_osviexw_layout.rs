//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18403 · GetVersionExW 与 OSVERSIONINFOEXW 结构面
//! · 0x7E080 段 · 批 01 · 配额 280
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18403-J1 结构尺寸/字段偏移与 winnt.h 声明一致断言全过，
//!   dwOSVersionInfoSize 校验错误路径 10/10 覆盖。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 布局冻结：OSVERSIONINFOEXW 共 284 字节（OSVERSIONINFOW 276），
//!     11 个字段偏移/宽度写死与 winnt.h 声明逐项对齐：
//!       dwOSVersionInfoSize@0(4) / dwMajorVersion@4 / dwMinorVersion@8 /
//!       dwBuildNumber@12 / dwPlatformId@16 / szCSDVersion@20(256B=128WCHAR)
//!       / wServicePackMajor@276 / wServicePackMinor@278 / wSuiteMask@280
//!       / wProductType@282(1B) / wReserved@283(1B)；
//!   - 尺寸闸：dwOSVersionInfoSize 只收 276（INFOW）/ 284（EXW）两档，
//!     其余 10 变体注入逐轮显式拒绝（不 panic 不静默）；
//!   - kind 失配闸：size=276 但 EX 扩展字段（SP/套件/产品型）非零 → 拒；
//!   - 编解码核：结构 → 小端字节缓冲（encode）→ 结构（decode）往返一致；
//!     零缓冲 decode → 全零结构（空边界合法）；
//!   - 域守卫：wProductType ∈ {0..3}（0=未定 1=工作站 2=域控 3=服务器），
//!     PlatformId ∈ {0..2}，越域显式拒；
//!   - 账本：LayoutLedger（尝试/收/尺寸拒/kind 拒）+ 审计会话状态机
//!     Fresh → Scanned → Verified → Sealed 单向；
//!   - no_std + alloc；零 unwrap/expect/panic（生产区）；禁浮点/IO/墙钟。
//! 本模块结构清单：
//!   [错误路径] OsvCode 闭集（11 码）+ ALL + detail() + detail(code)
//!   [裁决表]   FIELD_LAYOUT 11 项冻结偏移/宽度表（winnt.h 口径）
//!   [解析器]   OsVersionInfoExW / encode / decode（小端往返）
//!   [状态机]   AuditPhase 四态单向 + LayoutSession
//!   [账本]     LayoutLedger（守恒核算）
//!   [注入核]   尺寸 10 变体 ×10 轮 + kind 失配 ×10 轮 + 域越界
//!   [收口]     golden_run + run_unx18403_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E080，AI-VX11-1105 G1 批01 独占区）。
pub const SEG_BASE: u32 = 0x7E080;

// ── 布局冻结常量（winnt.h 口径逐项推导） ─────────────────────────
/// OSVERSIONINFOEXW 总尺寸（推导：4×5 DWORD + 256B CSD + 2×3 WORD + 2B = 284）。
pub const SIZE_EXW: usize = 284;
/// OSVERSIONINFOW 总尺寸（EXW 去扩展段：276）。
pub const SIZE_INFOW: usize = 276;
/// szCSDVersion 容量（128 WCHAR）。
pub const CSD_WCHARS: usize = 128;
/// 字段数（含 size 自身与保留尾字节）。
pub const FIELD_COUNT: usize = 11;
/// wProductType 域上界（0..=3）。
pub const PRODUCT_TYPE_MAX: u8 = 3;
/// dwPlatformId 域上界（0..=2，NT=2）。
pub const PLATFORM_ID_MAX: u32 = 2;
/// 判据轮数。
pub const JUDGE_ROUNDS: u32 = 10;
/// 尺寸注入变体数（10 个坏尺寸）。
pub const BAD_SIZE_VARIANTS: usize = 10;

// ── 字段偏移冻结表（裁决表：与 winnt.h 声明逐项对齐） ────────────
/// 字段（名, 偏移, 宽度字节）。顺序即内存序，单调不重叠。
pub const FIELD_LAYOUT: [(&str, usize, usize); FIELD_COUNT] = [
    ("dwOSVersionInfoSize", 0, 4),
    ("dwMajorVersion", 4, 4),
    ("dwMinorVersion", 8, 4),
    ("dwBuildNumber", 12, 4),
    ("dwPlatformId", 16, 4),
    ("szCSDVersion", 20, 256),
    ("wServicePackMajor", 276, 2),
    ("wServicePackMinor", 278, 2),
    ("wSuiteMask", 280, 2),
    ("wProductType", 282, 1),
    ("wReserved", 283, 1),
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OsvCode {
    /// 尺寸非法（276/284 两档之外）。
    SizeInvalid,
    /// kind 失配（276 档但 EX 扩展字段非零）。
    SizeKindMismatch,
    /// wProductType 越域（>3）。
    ProductTypeRange,
    /// dwPlatformId 越域（>2）。
    PlatformIdRange,
    /// CSD 尾 NUL 缺失（128 WCHAR 内必须 NUL 收尾）。
    CsdNoTerminator,
    /// 偏移表越界读（内部防御）。
    LayoutIndex,
    /// 布局审计失败（重叠/越总尺寸）。
    LayoutOverlap,
    /// 会话越级。
    SessionPhaseSkipped,
    /// 账本守恒破坏。
    LedgerBroken,
    /// 双跑不一致。
    NotReproducible,
    /// 解码字段读越界（防御性，缓冲恒定长）。
    DecodeOverrun,
}

impl OsvCode {
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            OsvCode::SizeInvalid => "dwOSVersionInfoSize 非法：只收 276/284",
            OsvCode::SizeKindMismatch => "kind 失配：276 档不得带 EX 扩展字段",
            OsvCode::ProductTypeRange => "wProductType 越域：0..=3 之外",
            OsvCode::PlatformIdRange => "dwPlatformId 越域：0..=2 之外",
            OsvCode::CsdNoTerminator => "szCSDVersion 缺 NUL：128 WCHAR 内未收尾",
            OsvCode::LayoutIndex => "字段偏移表越界读",
            OsvCode::LayoutOverlap => "布局审计失败：字段重叠或越总尺寸",
            OsvCode::SessionPhaseSkipped => "会话越级：未扫描先验证",
            OsvCode::LedgerBroken => "账本守恒破坏",
            OsvCode::NotReproducible => "双跑不一致：复现红线",
            OsvCode::DecodeOverrun => "解码读越界：缓冲长度不足",
        }
    }
    pub const ALL: [OsvCode; 11] = [
        OsvCode::SizeInvalid,
        OsvCode::SizeKindMismatch,
        OsvCode::ProductTypeRange,
        OsvCode::PlatformIdRange,
        OsvCode::CsdNoTerminator,
        OsvCode::LayoutIndex,
        OsvCode::LayoutOverlap,
        OsvCode::SessionPhaseSkipped,
        OsvCode::LedgerBroken,
        OsvCode::NotReproducible,
        OsvCode::DecodeOverrun,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < OsvCode::ALL.len() {
        if OsvCode::ALL[i].code() == code {
            return OsvCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E080 段闭集内"
}

// ── 结构域模型 ──────────────────────────────────────────────────
/// 结构 kind（dwOSVersionInfoSize 两档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SizeKind {
    /// OSVERSIONINFOW（276，无 EX 扩展段）。
    Infow,
    /// OSVERSIONINFOEXW（284，带扩展段）。
    Exw,
}

/// OSVERSIONINFOEXW 域内投影（CSD 以 WCHAR 数组存，NUL 收尾）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OsVersionInfoExW {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
    pub platform_id: u32,
    /// szCSDVersion：128 WCHAR，NUL 收尾（空串即首个 WCHAR=0）。
    pub csd: [u16; CSD_WCHARS],
    pub sp_major: u16,
    pub sp_minor: u16,
    pub suite_mask: u16,
    /// 0=未定 1=工作站 2=域控 3=服务器。
    pub product_type: u8,
}

impl OsVersionInfoExW {
    /// 空结构（全零；域合法：product_type=0 未定、platform_id=0）。
    pub const fn empty() -> OsVersionInfoExW {
        OsVersionInfoExW {
            major: 0,
            minor: 0,
            build: 0,
            platform_id: 0,
            csd: [0; CSD_WCHARS],
            sp_major: 0,
            sp_minor: 0,
            suite_mask: 0,
            product_type: 0,
        }
    }
    /// 域守卫：product_type ≤ 3、platform_id ≤ 2。
    pub fn domain_ok(&self) -> bool {
        self.product_type <= PRODUCT_TYPE_MAX && self.platform_id <= PLATFORM_ID_MAX
    }
    /// CSD 是否 NUL 收尾（扫描 128 槽找 0；找不到即缺尾）。
    pub fn csd_terminated(&self) -> bool {
        let mut i = 0;
        while i < CSD_WCHARS {
            if self.csd[i] == 0 {
                return true;
            }
            i += 1;
        }
        false
    }
    /// EX 扩展段是否非零（kind 失配闸的判据）。
    pub fn ex_extension_nonzero(&self) -> bool {
        self.sp_major != 0 || self.sp_minor != 0 || self.suite_mask != 0 || self.product_type != 0
    }
}

// ── 小端编码 / 解码（解析器核） ─────────────────────────────────
/// u16 小端写两字节（buf 已按偏移+1 越界校验）。
fn put_u16(buf: &mut [u8], off: usize, v: u16) -> bool {
    if off + 2 > buf.len() {
        return false;
    }
    buf[off] = (v & 0xFF) as u8;
    buf[off + 1] = (v >> 8) as u8;
    true
}

/// u32 小端写四字节。
fn put_u32(buf: &mut [u8], off: usize, v: u32) -> bool {
    if off + 4 > buf.len() {
        return false;
    }
    buf[off] = (v & 0xFF) as u8;
    buf[off + 1] = ((v >> 8) & 0xFF) as u8;
    buf[off + 2] = ((v >> 16) & 0xFF) as u8;
    buf[off + 3] = ((v >> 24) & 0xFF) as u8;
    true
}

fn get_u32(buf: &[u8], off: usize) -> Option<u32> {
    if off + 4 > buf.len() {
        return None;
    }
    Some((buf[off] as u32)
        | ((buf[off + 1] as u32) << 8)
        | ((buf[off + 2] as u32) << 16)
        | ((buf[off + 3] as u32) << 24))
}

fn get_u16(buf: &[u8], off: usize) -> Option<u16> {
    if off + 2 > buf.len() {
        return None;
    }
    Some((buf[off] as u16) | ((buf[off + 1] as u16) << 8))
}

/// 编码：结构 → 284 字节小端缓冲；kind 决定 size 档。
/// size=Infow（276）时 EX 扩展字段必须全零，否则 kind 失配拒。
pub fn encode(info: &OsVersionInfoExW, kind: SizeKind, buf: &mut [u8; SIZE_EXW]) -> Result<(), u32> {
    if !info.domain_ok() {
        return Err(if info.product_type > PRODUCT_TYPE_MAX {
            OsvCode::ProductTypeRange.code()
        } else {
            OsvCode::PlatformIdRange.code()
        });
    }
    if !info.csd_terminated() {
        return Err(OsvCode::CsdNoTerminator.code());
    }
    let size_field = match kind {
        SizeKind::Exw => SIZE_EXW as u32,
        SizeKind::Infow => SIZE_INFOW as u32,
    };
    if kind == SizeKind::Infow && info.ex_extension_nonzero() {
        return Err(OsvCode::SizeKindMismatch.code());
    }
    // 清零后重填（幂等：重复编码同结构同字节）。
    let mut i = 0;
    while i < SIZE_EXW {
        buf[i] = 0;
        i += 1;
    }
    if !put_u32(buf, 0, size_field)
        || !put_u32(buf, 4, info.major)
        || !put_u32(buf, 8, info.minor)
        || !put_u32(buf, 12, info.build)
        || !put_u32(buf, 16, info.platform_id)
    {
        return Err(OsvCode::DecodeOverrun.code());
    }
    // CSD：逐 WCHAR 两字节写入（20 起，128×2=256 字节）。
    let mut w = 0;
    while w < CSD_WCHARS {
        if !put_u16(buf, 20 + w * 2, info.csd[w]) {
            return Err(OsvCode::DecodeOverrun.code());
        }
        w += 1;
    }
    // EX 扩展段（276..284）。
    if !put_u16(buf, 276, info.sp_major)
        || !put_u16(buf, 278, info.sp_minor)
        || !put_u16(buf, 280, info.suite_mask)
    {
        return Err(OsvCode::DecodeOverrun.code());
    }
    buf[282] = info.product_type;
    buf[283] = 0; // wReserved 恒 0。
    Ok(())
}

/// 解码：284 字节缓冲 → 结构。尺寸闸 + kind 失配闸 + 域守卫三闸。
pub fn decode(buf: &[u8; SIZE_EXW]) -> Result<OsVersionInfoExW, u32> {
    let size_field = match get_u32(buf, 0) {
        Some(v) => v,
        None => return Err(OsvCode::DecodeOverrun.code()),
    };
    let kind = match size_field as usize {
        SIZE_EXW => SizeKind::Exw,
        SIZE_INFOW => SizeKind::Infow,
        _ => return Err(OsvCode::SizeInvalid.code()),
    };
    let major = match get_u32(buf, 4) {
        Some(v) => v,
        None => return Err(OsvCode::DecodeOverrun.code()),
    };
    let minor = match get_u32(buf, 8) {
        Some(v) => v,
        None => return Err(OsvCode::DecodeOverrun.code()),
    };
    let build = match get_u32(buf, 12) {
        Some(v) => v,
        None => return Err(OsvCode::DecodeOverrun.code()),
    };
    let platform_id = match get_u32(buf, 16) {
        Some(v) => v,
        None => return Err(OsvCode::DecodeOverrun.code()),
    };
    let mut csd = [0u16; CSD_WCHARS];
    let mut w = 0;
    while w < CSD_WCHARS {
        let v = match get_u16(buf, 20 + w * 2) {
            Some(v) => v,
            None => return Err(OsvCode::DecodeOverrun.code()),
        };
        csd[w] = v;
        w += 1;
    }
    let sp_major = match get_u16(buf, 276) {
        Some(v) => v,
        None => return Err(OsvCode::DecodeOverrun.code()),
    };
    let sp_minor = match get_u16(buf, 278) {
        Some(v) => v,
        None => return Err(OsvCode::DecodeOverrun.code()),
    };
    let suite_mask = match get_u16(buf, 280) {
        Some(v) => v,
        None => return Err(OsvCode::DecodeOverrun.code()),
    };
    let product_type = buf[282];
    let info = OsVersionInfoExW {
        major,
        minor,
        build,
        platform_id,
        csd,
        sp_major,
        sp_minor,
        suite_mask,
        product_type,
    };
    // kind 失配闸：276 档下扩展段必须全零。
    if kind == SizeKind::Infow && info.ex_extension_nonzero() {
        return Err(OsvCode::SizeKindMismatch.code());
    }
    // 域守卫。
    if !info.domain_ok() {
        return Err(if info.product_type > PRODUCT_TYPE_MAX {
            OsvCode::ProductTypeRange.code()
        } else {
            OsvCode::PlatformIdRange.code()
        });
    }
    if !info.csd_terminated() {
        return Err(OsvCode::CsdNoTerminator.code());
    }
    Ok(info)
}

// ── 尺寸注入核（10 坏变体 × 10 轮全拒） ─────────────────────────
/// 10 个坏尺寸变体（冻结表：0/4/8/100/275/277/280/283/285/512）。
pub const BAD_SIZES: [u32; BAD_SIZE_VARIANTS] = [0, 4, 8, 100, 275, 277, 280, 283, 285, 512];

/// 构造一个假 size 头的缓冲（只改 size 字段，其余清零），解码必拒。
pub fn bad_size_sample(round: usize) -> ([u8; SIZE_EXW], u32) {
    let sz = BAD_SIZES[round % BAD_SIZE_VARIANTS];
    let mut buf = [0u8; SIZE_EXW];
    let _ = put_u32(&mut buf, 0, sz);
    (buf, sz)
}

/// 尺寸注入 rounds 轮：10 变体轮换逐轮必拒，返回拒轮数（全拒才 Ok）。
pub fn size_inject_rounds(rounds: u32) -> Result<u32, u32> {
    let mut rejected = 0u32;
    let mut r = 0;
    while r < rounds {
        let mut k = 0;
        while k < BAD_SIZE_VARIANTS {
            let (buf, _sz) = bad_size_sample(k);
            match decode(&buf) {
                Err(c) => {
                    if c != OsvCode::SizeInvalid.code() {
                        // 坏尺寸必须报尺寸码（报别的码=误诊）。
                        return Err(OsvCode::SizeInvalid.code());
                    }
                    rejected += 1;
                }
                Ok(_) => return Err(OsvCode::SizeInvalid.code()),
            }
            k += 1;
        }
        r += 1;
    }
    Ok(rejected)
}

/// kind 失配注入 rounds 轮：276 档 + EX 扩展非零（SP/套件/产品型轮换）。
pub fn kind_mismatch_inject_rounds(rounds: u32) -> Result<u32, u32> {
    let mut rejected = 0u32;
    let mut r = 0;
    while r < rounds {
        let mut info = OsVersionInfoExW::empty();
        match r % 3 {
            0 => info.sp_major = 2,
            1 => info.suite_mask = 0x0100,
            _ => info.product_type = 1,
        }
        let mut buf = [0u8; SIZE_EXW];
        match encode(&info, SizeKind::Infow, &mut buf) {
            Err(c) => {
                if c != OsvCode::SizeKindMismatch.code() {
                    return Err(OsvCode::SizeKindMismatch.code());
                }
                rejected += 1;
            }
            Ok(()) => return Err(OsvCode::SizeKindMismatch.code()),
        }
        r += 1;
    }
    Ok(rejected)
}

// ── 账本 ────────────────────────────────────────────────────────
/// 布局账本：尝试/收/尺寸拒/kind 拒四账并列，守恒核算。
#[derive(Clone, Copy, Debug)]
pub struct LayoutLedger {
    pub tries: u32,
    pub accepted: u32,
    pub size_rej: u32,
    pub kind_rej: u32,
}

impl LayoutLedger {
    pub fn new() -> LayoutLedger {
        LayoutLedger { tries: 0, accepted: 0, size_rej: 0, kind_rej: 0 }
    }
    /// 守恒：tries == accepted + size_rej + kind_rej。
    pub fn conserve(&self) -> bool {
        self.tries == self.accepted + self.size_rej + self.kind_rej
    }
}

// ── 审计会话状态机（Fresh→Scanned→Verified→Sealed 单向） ────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AuditPhase {
    Fresh,
    /// 偏移表已扫描（单调/不重叠核过）。
    Scanned,
    /// 往返样本已验证。
    Verified,
    Sealed,
}

/// 布局审计会话：扫描偏移表 → 往返验证 → 封印。
pub struct LayoutSession {
    phase: AuditPhase,
    pub ledger: LayoutLedger,
}

impl LayoutSession {
    pub fn new() -> LayoutSession {
        LayoutSession { phase: AuditPhase::Fresh, ledger: LayoutLedger::new() }
    }
    pub fn phase(&self) -> AuditPhase {
        self.phase
    }
    /// 扫描偏移表：单调递增、不重叠、末字段不越总尺寸。
    pub fn scan(&mut self) -> Result<(), u32> {
        if self.phase != AuditPhase::Fresh {
            return Err(OsvCode::SessionPhaseSkipped.code());
        }
        let mut i = 0;
        let mut cursor = 0usize;
        while i < FIELD_COUNT {
            let (name, off, size) = FIELD_LAYOUT[i];
            // 名非空、偏移不回退、宽度非零。
            if name.is_empty() || off < cursor || size == 0 {
                return Err(OsvCode::LayoutOverlap.code());
            }
            if off + size > SIZE_EXW {
                return Err(OsvCode::LayoutOverlap.code());
            }
            cursor = off + size;
            i += 1;
        }
        // 表尾必须精确到总尺寸（无洞无溢出）。
        if cursor != SIZE_EXW {
            return Err(OsvCode::LayoutOverlap.code());
        }
        self.phase = AuditPhase::Scanned;
        Ok(())
    }
    /// 往返验证：一个 EXW 快照 + 一个 INFOW 快照 encode→decode 一致。
    pub fn verify(&mut self) -> Result<(), u32> {
        if self.phase != AuditPhase::Scanned {
            return Err(OsvCode::SessionPhaseSkipped.code());
        }
        // EXW 快照：7.0 build 7601 + SP1 + 域控。
        let mut ex = OsVersionInfoExW::empty();
        ex.major = 6;
        ex.minor = 1;
        ex.build = 7601;
        ex.platform_id = 2;
        ex.sp_major = 1;
        ex.suite_mask = 0x0100;
        ex.product_type = 3;
        let mut buf = [0u8; SIZE_EXW];
        self.ledger.tries += 1;
        encode(&ex, SizeKind::Exw, &mut buf)?;
        let back = decode(&buf)?;
        if back != ex {
            return Err(OsvCode::LayoutOverlap.code());
        }
        self.ledger.accepted += 1;
        // INFOW 快照：全零 EX 段。
        let mut iw = OsVersionInfoExW::empty();
        iw.major = 5;
        iw.minor = 1;
        iw.build = 2600;
        iw.platform_id = 2;
        let mut buf2 = [0u8; SIZE_EXW];
        self.ledger.tries += 1;
        encode(&iw, SizeKind::Infow, &mut buf2)?;
        let back2 = decode(&buf2)?;
        if back2 != iw {
            return Err(OsvCode::LayoutOverlap.code());
        }
        self.ledger.accepted += 1;
        self.phase = AuditPhase::Verified;
        Ok(())
    }
    /// 封印：账守恒才可封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.phase != AuditPhase::Verified {
            return Err(OsvCode::SessionPhaseSkipped.code());
        }
        if !self.ledger.conserve() {
            return Err(OsvCode::LedgerBroken.code());
        }
        self.phase = AuditPhase::Sealed;
        Ok(())
    }
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链一次（确定性）。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let mut sess = LayoutSession::new();
    sess.scan()?;
    sess.verify()?;
    sess.seal()?;
    // 尺寸注入：10 变体 × 10 轮 = 100 拒。
    let size_rej = size_inject_rounds(JUDGE_ROUNDS)?;
    if size_rej != BAD_SIZE_VARIANTS as u32 * JUDGE_ROUNDS {
        return Err(OsvCode::SizeInvalid.code());
    }
    // kind 失配注入：10 轮 = 10 拒。
    let kind_rej = kind_mismatch_inject_rounds(JUDGE_ROUNDS)?;
    if kind_rej != JUDGE_ROUNDS {
        return Err(OsvCode::SizeKindMismatch.code());
    }
    // 布局表逐项偏移核对（winnt.h 口径魔数）。
    let layout_ok = FIELD_LAYOUT[5].1 == 20
        && FIELD_LAYOUT[5].2 == 256
        && FIELD_LAYOUT[6].1 == 276
        && FIELD_LAYOUT[9].1 == 282
        && FIELD_LAYOUT[10].1 == 283;
    if !layout_ok {
        return Err(OsvCode::LayoutOverlap.code());
    }
    Ok((sess.ledger.accepted, size_rej, kind_rej, sess.ledger.tries))
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(OsvCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < OsvCode::ALL.len() {
        let v = OsvCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：扫描→验证→封印正链成功。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut sess = LayoutSession::new();
    sess.scan()?;
    sess.verify()?;
    sess.seal()?;
    if sess.phase() != AuditPhase::Sealed {
        return Err(OsvCode::SessionPhaseSkipped.code());
    }
    Ok(())
}

/// 探针 B：Fresh 直接 verify（越级）→ 必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut sess = LayoutSession::new();
    sess.verify().map(|_| ())
}

/// 探针 C：合法 EXW 编码往返（10.0.19045 快照）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut info = OsVersionInfoExW::empty();
    info.major = 10;
    info.minor = 0;
    info.build = 19045;
    info.platform_id = 2;
    let mut buf = [0u8; SIZE_EXW];
    encode(&info, SizeKind::Exw, &mut buf)?;
    let back = decode(&buf)?;
    if back.major != 10 || back.build != 19045 {
        return Err(OsvCode::LayoutOverlap.code());
    }
    Ok(())
}

// ── 判据收口 ────────────────────────────────────────────────────
/// 判据集（8–20 条；动态明细 ≤8）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 结构总尺寸冻结（EXW=284 / W=276）。
    v.push((
        "J1-结构尺寸284/276",
        SIZE_EXW == 284 && SIZE_INFOW == 276,
        format!("EXW={} W={}", SIZE_EXW, SIZE_INFOW),
    ));

    // 2. 字段偏移逐项一致（11 项表 vs winnt.h 口径魔数组）。
    let want_off: [usize; FIELD_COUNT] = [0, 4, 8, 12, 16, 20, 276, 278, 280, 282, 283];
    let want_size: [usize; FIELD_COUNT] = [4, 4, 4, 4, 4, 256, 2, 2, 2, 1, 1];
    let off_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < FIELD_COUNT {
            if FIELD_LAYOUT[i].1 != want_off[i] || FIELD_LAYOUT[i].2 != want_size[i] {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-字段偏移11项对齐", off_ok, String::from("0..4..8..12..16..20..276..278..280..282..283")));

    // 3. 偏移单调不重叠且精确收口到 284（扫描会话）。
    let mut sess_scan = LayoutSession::new();
    let scan_ok = sess_scan.scan().is_ok();
    v.push(("J1-偏移单调无重叠", scan_ok, String::from("cursor 收口 284")));

    // 4. 合法两档尺寸（276/284）过、285 拒（探针 C）。
    let pc = probe_seq_c().is_ok();
    let mut bad285 = [0u8; SIZE_EXW];
    let _ = put_u32(&mut bad285, 0, 285);
    let b285 = decode(&bad285).is_err();
    v.push((
        "J1-尺寸两档285拒",
        pc && b285,
        format!("276过 284过 285拒={}", b285),
    ));

    // 5. 尺寸注入 10 变体 × 10 轮 = 100 全拒。
    let size_rej = size_inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-尺寸注入10变体10/10",
        match &size_rej {
            Ok(n) => *n == 100,
            Err(_) => false,
        },
        format!("{:?}", size_rej),
    ));

    // 6. kind 失配注入 10/10。
    let kind_rej = kind_mismatch_inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-kind失配注入10/10",
        match &kind_rej {
            Ok(n) => *n == JUDGE_ROUNDS,
            Err(_) => false,
        },
        format!("{:?}", kind_rej),
    ));

    // 7. 往返一致（EXW 快照 encode→decode 逐字段还原）。
    let mut ex = OsVersionInfoExW::empty();
    ex.major = 6;
    ex.minor = 3;
    ex.build = 9600;
    ex.platform_id = 2;
    ex.sp_major = 0;
    ex.suite_mask = 0x0004;
    ex.product_type = 3;
    let mut buf = [0u8; SIZE_EXW];
    let rt = match encode(&ex, SizeKind::Exw, &mut buf) {
        Ok(()) => match decode(&buf) {
            Ok(back) => back == ex,
            Err(_) => false,
        },
        Err(_) => false,
    };
    v.push(("J1-往返一致(8.1.9600)", rt, String::from("encode→decode 还原")));

    // 8. 空边界：零缓冲（size=0 档除外）——全零结构 encode/decode 合法。
    let zero = OsVersionInfoExW::empty();
    let mut zbuf = [0u8; SIZE_EXW];
    let empty_ok = match encode(&zero, SizeKind::Exw, &mut zbuf) {
        Ok(()) => match decode(&zbuf) {
            Ok(back) => back == zero,
            Err(_) => false,
        },
        Err(_) => false,
    };
    v.push(("J1-空结构零缓冲合法", empty_ok, String::from("全零域守卫通过")));

    // 9. 域越界注入：product_type=4 / platform_id=3 各拒。
    let mut bad_pt = OsVersionInfoExW::empty();
    bad_pt.product_type = 4;
    let mut pbuf = [0u8; SIZE_EXW];
    let pt_rej = encode(&bad_pt, SizeKind::Exw, &mut pbuf).is_err();
    let mut bad_pid = OsVersionInfoExW::empty();
    bad_pid.platform_id = 3;
    let mut qbuf = [0u8; SIZE_EXW];
    let pid_rej = encode(&bad_pid, SizeKind::Exw, &mut qbuf).is_err();
    v.push((
        "J1-域越界拒(pt=4,pid=3)",
        pt_rej && pid_rej,
        format!("pt={} pid={}", pt_rej, pid_rej),
    ));

    // 10. CSD 缺 NUL 注入：128 槽全非零 → 拒。
    let mut bad_csd = OsVersionInfoExW::empty();
    let mut w = 0;
    while w < CSD_WCHARS {
        bad_csd.csd[w] = 0x41 + (w % 26) as u16;
        w += 1;
    }
    let mut cbuf = [0u8; SIZE_EXW];
    let csd_rej = encode(&bad_csd, SizeKind::Exw, &mut cbuf).is_err();
    v.push(("J1-CSD缺NUL拒", csd_rej, String::from("128 槽无尾")));

    // 11. 账本守恒（会话内 2 试 2 收 0 拒）。
    let mut sess_full = LayoutSession::new();
    let ledger_ok = match sess_full.scan().and_then(|_| sess_full.verify()).and_then(|_| sess_full.seal()) {
        Ok(()) => sess_full.ledger.conserve() && sess_full.ledger.accepted == 2,
        Err(_) => false,
    };
    v.push(("J1-账本守恒(2试2收)", ledger_ok, String::from("EXW+INFOW 两笔")));

    // 12. 会话三序（正链过 / 越级拒 / 探针 C 过）。
    let pa = probe_seq_a().is_ok();
    let pb = probe_seq_b().is_err();
    v.push((
        "J1-会话三序",
        pa && pb && pc,
        format!("a={} b拒={}", pa, pb),
    ));

    // 13. 双跑一致。
    let rp = repro_check();
    v.push(("J2-双跑一致", rp.is_ok(), format!("{:?}", rp)));

    // 14. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        format!("{:06X}..{:06X}", SEG_BASE, SEG_BASE + 0x40),
    ));

    // 15. 诊断码闭集（11 码全非空中文明细）。
    let all_ok = {
        let mut ok = OsvCode::ALL.len() == 11;
        let mut i = 0;
        while i < OsvCode::ALL.len() {
            if OsvCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-诊断码闭集11码", all_ok, String::from("detail 全非空")));

    v
}

/// 收口：本模块自检 CheckSet（判据 15 条；前 8 条动态明细，其余静态）。
pub fn run_unx18403_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18403 OSVERSIONINFOEXW 结构面");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (name, passed, det) = &checks[i];
        if i < 8 {
            s.add_dyn(name, *passed, det);
        } else {
            s.add(name, *passed, "见 extra_checks 明细");
        }
        i += 1;
    }
    s
}

// ── 测试（≥8 个 #[test]） ───────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 总闸：全部判据绿。
    #[test]
    fn test_all_checks_green() {
        let set = run_unx18403_checks();
        assert!(set.all_passed(), "UNX-F18403 判据集应全绿");
    }

    /// 正常流：扫描→验证→封印正链成功。
    #[test]
    fn test_session_flow() {
        assert!(probe_seq_a().is_ok());
    }

    /// 边界：越级 verify 必拒。
    #[test]
    fn test_phase_skip_rejected() {
        assert!(probe_seq_b().is_err());
    }

    /// 注入：尺寸 10 变体 × 10 轮全拒。
    #[test]
    fn test_size_injection() {
        assert_eq!(size_inject_rounds(JUDGE_ROUNDS), Ok(100));
    }

    /// 注入：kind 失配 10/10。
    #[test]
    fn test_kind_mismatch_injection() {
        assert_eq!(kind_mismatch_inject_rounds(JUDGE_ROUNDS), Ok(10));
    }

    /// 往返：7.0 结构 encode→decode 逐字段还原。
    #[test]
    fn test_roundtrip_snapshot() {
        let mut ex = OsVersionInfoExW::empty();
        ex.major = 6;
        ex.minor = 1;
        ex.build = 7601;
        ex.platform_id = 2;
        ex.sp_major = 1;
        ex.product_type = 2;
        let mut buf = [0u8; SIZE_EXW];
        assert!(encode(&ex, SizeKind::Exw, &mut buf).is_ok());
        let back = match decode(&buf) {
            Ok(b) => b,
            Err(_) => {
                assert!(false, "decode 不应失败");
                return;
            }
        };
        assert_eq!(back, ex);
    }

    /// 域越界：product_type=4 显式拒（ProductTypeRange 码）。
    #[test]
    fn test_product_type_range() {
        let mut bad = OsVersionInfoExW::empty();
        bad.product_type = 4;
        let mut buf = [0u8; SIZE_EXW];
        match encode(&bad, SizeKind::Exw, &mut buf) {
            Err(c) => assert_eq!(c, OsvCode::ProductTypeRange.code()),
            Ok(()) => assert!(false, "product_type=4 必须拒"),
        }
    }

    /// 空边界：全零结构合法往返（空串 CSD=NUL 首槽）。
    #[test]
    fn test_empty_struct_ok() {
        let zero = OsVersionInfoExW::empty();
        assert!(zero.domain_ok());
        assert!(zero.csd_terminated());
        let mut buf = [0u8; SIZE_EXW];
        assert!(encode(&zero, SizeKind::Exw, &mut buf).is_ok());
        assert!(decode(&buf).is_ok());
    }

    /// 尺寸魔数：size 字段写在偏移 0，EXW 编码后首四字节 = 284 小端。
    /// （284 = 0x11C：低字节 0x1C、次低字节 0x01——魔数按 u32 推导后
    /// 显式落 u8，避免 284 字面量落入 u8 推断槽。）
    #[test]
    fn test_size_field_layout() {
        let mut buf = [0u8; SIZE_EXW];
        let info = OsVersionInfoExW::empty();
        assert!(encode(&info, SizeKind::Exw, &mut buf).is_ok());
        assert_eq!(buf[0], (284u32 & 0xFF) as u8);
        assert_eq!(buf[1], ((284u32 >> 8) & 0xFF) as u8);
        assert_eq!(buf[2], 0u8);
    }

    /// 复现红线：golden_run 双跑一致。
    #[test]
    fn test_reproducible() {
        assert!(repro_check().is_ok());
    }

    /// 段位守卫。
    #[test]
    fn test_segment_guard() {
        assert!(segment_guard());
    }
}
