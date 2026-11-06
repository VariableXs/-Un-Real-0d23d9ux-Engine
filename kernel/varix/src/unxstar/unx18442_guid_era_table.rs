//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18442 · supportedOS GUID 世代表（Vista→Win10/11 全集） · 0x7EA40 段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18442-J1 GUID→世代映射表与微软口径对照全集零偏差，
//!       伪造 GUID 注入识别 10/10
//! 本模块结构清单：
//!   1. 冻结常量层：微软官方 5 个 supportedOS GUID（winnt.h 口径），
//!      逐字写死为 16 字节数组；Win11 与 Win10 共用同一 GUID
//!      （{8e0f7a12-…}，微软口径：Win10 GUID 覆盖 10.0 及以后全部世代）；
//!   2. 诊断码闭集 GeCode（code()=SEG_BASE+n，段内 0x40 窗口）；
//!   3. 解析器：36 字符规范文本 → [u8;16]（段位/连字符/十六进制三闸），
//!      大小写不敏感（微软口径：十六进制大小写等价）；
//!   4. 世代表查找：解析字节与冻结表线性比对 → 世代秩
//!      （Vista=1 < 7=2 < 8=3 < 8.1=4 < 10=5，单调有序）；
//!   5. 零偏差账：5 规范 GUID 逐例 解析→查表→秩 与冻结期望逐例对拍，
//!      总偏差必须为 0；
//!   6. 注入核：10 个伪造 GUID（全零/全 F/单 nibble 翻转/缺连字符/
//!      非法字符/段数错/超长/截断/世代外私造/规范 GUID 重排）逐类
//!      断言被显式拒绝，识别 10/10；另加单字节扰动核（5 规范 GUID
//!      ×16 字节 ×2 扰动 = 160 样本，逐字节位判别 160/160）与 LCG
//!      确定性模糊注入（32 样本全拒）；
//!   7. 文本↔字节往返序列化器（format_guid）与世代政策表
//!      （秩→min_build 单调）；
//!   8. 边界：空输入/超长/重复解析幂等；
//!   9. run_unx18442_checks：20 条判据（断言性质）。
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 诊断段基址（0x7EA40，G1 工单 A05 批03 独占窗口，0x40 对齐）。
pub const SEG_BASE: u32 = 0x7EA40;

// ── 冻结常量层 ─────────────────────────────────────────────────────
/// 规范 GUID 数（写死 5：微软官方 supportedOS 全集）。
pub const GUID_N: usize = 5;
/// GUID 字节数（写死 16：RFC 4122 二进制布局）。
pub const GUID_BYTES: usize = 16;
/// 规范文本长度（写死 36：8-4-4-4-12 十六进制 + 4 连字符）。
pub const TEXT_LEN: usize = 36;
/// 世代秩（写死：Vista=1 起，Win10=5 止；Win11 复用 Win10 GUID 不单列）。
pub const RANK_VISTA: u8 = 1;
pub const RANK_WIN7: u8 = 2;
pub const RANK_WIN8: u8 = 3;
pub const RANK_WIN81: u8 = 4;
pub const RANK_WIN10: u8 = 5;
/// 伪造注入数（写死 10：判据原文识别 10/10 语义）。
pub const FORGED_N: usize = 10;
/// 判据轮数（写死 3）。
pub const JUDGE_ROUNDS: u32 = 3;

/// 世代名（冻结，与秩一一对应）。
pub const ERA_NAMES: [&str; GUID_N] = ["Vista", "Windows 7", "Windows 8", "Windows 8.1", "Windows 10/11"];

// 微软官方 GUID 逐字节冻结（winnt.h / manifest compatibility 口径）：
//   data1(4B 大端) + data2(2B) + data3(2B) + data4(8B 逐字节)。
// 例：{e2011457-1546-43c5-a5fe-008deee3d3f0} →
//   e2 01 14 57 | 15 46 | 43 c5 | a5 fe 00 8d ee e3 d3 f0。
pub const GUID_VISTA: [u8; 16] = [
    0xE2, 0x01, 0x14, 0x57, 0x15, 0x46, 0x43, 0xC5, 0xA5, 0xFE, 0x00, 0x8D, 0xEE, 0xE3, 0xD3, 0xF0,
];
pub const GUID_WIN7: [u8; 16] = [
    0x35, 0x13, 0x8B, 0x9A, 0x5D, 0x96, 0x4F, 0xBD, 0x8E, 0x2D, 0xA2, 0x44, 0x02, 0x25, 0xF9, 0x3A,
];
pub const GUID_WIN8: [u8; 16] = [
    0x4A, 0x2F, 0x28, 0xE3, 0x53, 0xB9, 0x44, 0x41, 0xBA, 0x9C, 0xD6, 0x9D, 0x4A, 0x4A, 0x6E, 0x38,
];
pub const GUID_WIN81: [u8; 16] = [
    0x1F, 0x67, 0x6C, 0x76, 0x80, 0xE1, 0x42, 0x39, 0x95, 0xBB, 0x83, 0xD0, 0xF6, 0xD0, 0xDA, 0x78,
];
pub const GUID_WIN10: [u8; 16] = [
    0x8E, 0x0F, 0x7A, 0x12, 0xBF, 0xB3, 0x4F, 0xE8, 0xB9, 0xA5, 0x48, 0xFD, 0x50, 0xA1, 0x5A, 0x9A,
];

/// 规范 GUID 冻结文本（与字节表一一对应，零偏差对拍的对照面）。
pub const GUID_TEXTS: [&str; GUID_N] = [
    "e2011457-1546-43c5-a5fe-008deee3d3f0",
    "35138b9a-5d96-4fbd-8e2d-a2440225f93a",
    "4a2f28e3-53b9-4441-ba9c-d69d4a4a6e38",
    "1f676c76-80e1-4239-95bb-83d0f6d0da78",
    "8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a",
];

/// 世代秩冻结期望（与 GUID_TEXTS 一一对应；微软口径逐例对拍）。
pub const ERA_RANKS: [u8; GUID_N] = [RANK_VISTA, RANK_WIN7, RANK_WIN8, RANK_WIN81, RANK_WIN10];

// ── 诊断码闭集（段内 0x40 窗口） ───────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GeCode {
    /// 空输入
    EmptyInput,
    /// 文本长度非法（≠36）
    TextLenBad,
    /// 连字符段位错（8/13/18/23 之外出现 '-' 或该位置缺失）
    DashPosBad,
    /// 非法十六进制字符
    HexBad,
    /// 未知 GUID：解析合法但不落微软闭集
    GuidUnknown,
    /// 世代秩越界（表内秩须落 1..5）
    RankOutOfRange,
    /// 表自坏：字节表与文本表互证不一致
    TableInconsistent,
    /// 秩序破坏：世代表非单调
    RankOrderBroken,
    /// 伪造未检出：注入账漏检
    ForgedMissed,
    /// 对照偏差非零：微软口径对照账必红
    DeviationNonZero,
    /// 双跑不一致
    NotReproducible,
    /// 大小写等价破坏：大写规范文本应与小写同解析
    CaseEquivalenceBroken,
}

impl GeCode {
    /// 诊断码取值：SEG_BASE + 枚举序（闭集，段内窗口 ≤0x40）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集，逐码非空）。
    pub fn detail(self) -> &'static str {
        match self {
            GeCode::EmptyInput => "空输入：解析拒",
            GeCode::TextLenBad => "文本长度非法：须 36 字符",
            GeCode::DashPosBad => "连字符段位错：8/13/18/23",
            GeCode::HexBad => "非法十六进制字符",
            GeCode::GuidUnknown => "未知 GUID：不落微软闭集",
            GeCode::RankOutOfRange => "世代秩越界：须落 1..5",
            GeCode::TableInconsistent => "表自坏：字节/文本互证不一致",
            GeCode::RankOrderBroken => "秩序破坏：世代非单调",
            GeCode::ForgedMissed => "伪造未检出：注入账漏检",
            GeCode::DeviationNonZero => "微软口径对照偏差非零",
            GeCode::NotReproducible => "双跑不一致：复现红线",
            GeCode::CaseEquivalenceBroken => "大小写等价破坏",
        }
    }
    /// 全集（闭集核对用）。
    pub const ALL: [GeCode; 12] = [
        GeCode::EmptyInput,
        GeCode::TextLenBad,
        GeCode::DashPosBad,
        GeCode::HexBad,
        GeCode::GuidUnknown,
        GeCode::RankOutOfRange,
        GeCode::TableInconsistent,
        GeCode::RankOrderBroken,
        GeCode::ForgedMissed,
        GeCode::DeviationNonZero,
        GeCode::NotReproducible,
        GeCode::CaseEquivalenceBroken,
    ];
}

/// 未知码回查（闭集外返回固定文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GeCode::ALL.len() {
        if GeCode::ALL[i].code() == code {
            return GeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7EA40 段闭集内"
}

// ── 解析器（三闸：长度/连字符/十六进制） ───────────────────────────
/// 单字符十六进制值（大小写不敏感；非法返回 None）。
fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// 规范文本 → [u8;16]。
/// 仓内口径：文本十六进制按位直接拼装（大端直读），与 winnt.h 字节
/// 排列一致——manifest 文本即 winnt.h GUID 的规范化十六进制投影。
pub fn parse_guid(text: &str) -> Result<[u8; 16], u32> {
    let b = text.as_bytes();
    if b.len() == 0 {
        return Err(GeCode::EmptyInput.code());
    }
    if b.len() != TEXT_LEN {
        return Err(GeCode::TextLenBad.code());
    }
    // 连字符闸：4 个固定段位。
    let dashes = [8usize, 13, 18, 23];
    let mut d = 0;
    while d < dashes.len() {
        if b[dashes[d]] != b'-' {
            return Err(GeCode::DashPosBad.code());
        }
        d += 1;
    }
    // 十六进制闸：非连字符位全为 hex。
    let mut out = [0u8; 16];
    let mut oi = 0usize;
    let mut i = 0usize;
    while i < TEXT_LEN {
        if b[i] == b'-' {
            i += 1;
            continue;
        }
        if i + 1 >= TEXT_LEN {
            return Err(GeCode::TextLenBad.code());
        }
        let hi = match hex_val(b[i]) {
            Some(v) => v,
            None => return Err(GeCode::HexBad.code()),
        };
        let lo = match hex_val(b[i + 1]) {
            Some(v) => v,
            None => return Err(GeCode::HexBad.code()),
        };
        if oi >= GUID_BYTES {
            return Err(GeCode::TextLenBad.code());
        }
        out[oi] = hi * 16 + lo;
        oi += 1;
        i += 2;
    }
    if oi != GUID_BYTES {
        return Err(GeCode::TextLenBad.code());
    }
    Ok(out)
}

/// 查找：16 字节与冻结表线性比对 → 世代秩。未知 → GuidUnknown。
pub fn lookup(g: &[u8; 16]) -> Result<(u8, &'static str), u32> {
    let table: [[u8; 16]; GUID_N] = [GUID_VISTA, GUID_WIN7, GUID_WIN8, GUID_WIN81, GUID_WIN10];
    let mut i = 0;
    while i < GUID_N {
        let mut eq = true;
        let mut k = 0;
        while k < GUID_BYTES {
            if g[k] != table[i][k] {
                eq = false;
                break;
            }
            k += 1;
        }
        if eq {
            let rank = ERA_RANKS[i];
            if rank == 0 || rank as usize > GUID_N {
                return Err(GeCode::RankOutOfRange.code());
            }
            return Ok((rank, ERA_NAMES[i]));
        }
        i += 1;
    }
    Err(GeCode::GuidUnknown.code())
}

/// 全链：文本→字节→秩。三闸+查表一线过。
pub fn resolve(text: &str) -> Result<(u8, &'static str), u32> {
    let g = parse_guid(text)?;
    lookup(&g)
}

// ── 表自检（字节表/文本表互证） ────────────────────────────────────
/// 表自坏检查：5 规范文本逐一解析，字节与冻结字节表逐字节相等。
pub fn table_selfcheck() -> Result<(), u32> {
    let table: [[u8; 16]; GUID_N] = [GUID_VISTA, GUID_WIN7, GUID_WIN8, GUID_WIN81, GUID_WIN10];
    let mut i = 0;
    while i < GUID_N {
        let parsed = parse_guid(GUID_TEXTS[i])?;
        let mut k = 0;
        while k < GUID_BYTES {
            if parsed[k] != table[i][k] {
                return Err(GeCode::TableInconsistent.code());
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 秩序检查：世代表单调（Vista<7<8<8.1<10）。
pub fn rank_order_check() -> Result<(), u32> {
    let mut i = 1;
    while i < GUID_N {
        if ERA_RANKS[i] <= ERA_RANKS[i - 1] {
            return Err(GeCode::RankOrderBroken.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 零偏差账（微软口径逐例对拍） ───────────────────────────────────
/// 对照一例：resolve 返回秩 vs 冻结期望秩。偏差计 1/0。
pub fn reconcile_case(idx: usize) -> Result<u32, u32> {
    if idx >= GUID_N {
        return Err(GeCode::DeviationNonZero.code());
    }
    match resolve(GUID_TEXTS[idx]) {
        Ok((rank, name)) => {
            // 名称与秩互证：秩 i 的名必须是 ERA_NAMES[i-1]。
            if name != ERA_NAMES[(rank - 1) as usize] {
                return Err(GeCode::TableInconsistent.code());
            }
            if rank == ERA_RANKS[idx] {
                Ok(0)
            } else {
                Ok(1)
            }
        }
        Err(_) => Ok(1),
    }
}

/// 全集对拍：5 例总偏差（微软口径零偏差要求总偏差 == 0）。
pub fn reconcile_all() -> Result<(u32, u32), u32> {
    let mut dev = 0u32;
    let mut i = 0;
    while i < GUID_N {
        dev += reconcile_case(i)?;
        i += 1;
    }
    Ok((dev, GUID_N as u32))
}

// ── 伪造 GUID 注入核（10 形态，逐类显式拒绝） ──────────────────────
/// 伪造样本 10 形态构造（确定性，每条注明伪造手法）：
///  0 全零 GUID（私造"未知世代"）
///  1 全 F GUID
///  2 Win10 GUID 单 nibble 翻转（末位 a→b，伪装近似）
///  3 Vista GUID 首字节翻转（e2→e3）
///  4 缺连字符（32 hex 直排）
///  5 非法字符（'g' 混入）
///  6 段数错（连字符位置偏移 1 位）
///  7 超长（40 字符）
///  8 截断（20 字符）
///  9 规范 Win7 GUID 段序重排（data1 与 data3 互换位段）
pub fn make_forged() -> [String; FORGED_N] {
    let mut v0 = String::from("00000000-0000-0000-0000-000000000000");
    let v1 = String::from("ffffffff-ffff-ffff-ffff-ffffffffffff");
    // Win10 文本末位 a→b。
    let mut v2 = String::from(GUID_TEXTS[4]);
    v2.replace_range(35..36, "b");
    // Vista 文本首 nibble e→d 之外：首字节 e2→f2（仍 hex 合法但不在闭集）。
    let mut v3 = String::from(GUID_TEXTS[0]);
    v3.replace_range(0..1, "f");
    let v4 = String::from("e2011457154643c5a5fe008deee3d3f0");
    let v5 = String::from("e2011457-1546-43c5-a5fe-008deee3d3fg");
    let v6 = String::from("e2011457-5154-63c5-a5fe-008deee3d3f0");
    let mut v7 = String::from(GUID_TEXTS[1]);
    v7.push_str("aaaa");
    let v8 = String::from(&GUID_TEXTS[2][0..20]);
    let _ = &mut v0; // v0 已在数组中使用（占位消 unused 警告）。
    [
        v0, v1, v2, v3, v4, v5, v6, v7, v8,
        String::from("35134fbd-8b9a-5d96-4fbd-8e2da2440225"),
    ]
}

/// 注入核：10 伪造逐个喂入 resolve，逐个断言被显式拒绝。
/// 返回（识别数，总数）；识别数必须 == 10。
pub fn forge_round(rounds: u32) -> Result<(u32, u32), u32> {
    let forged = make_forged();
    let mut caught = 0u32;
    let mut r = 0;
    while r < rounds {
        let mut i = 0;
        while i < forged.len() {
            if resolve(&forged[i]).is_err() {
                caught += 1;
            }
            i += 1;
        }
        r += 1;
    }
    let total = forged.len() as u32 * rounds;
    if caught != total {
        return Err(GeCode::ForgedMissed.code());
    }
    Ok((caught, total))
}

/// 大小写等价：大写规范文本与小写解析结果逐字节一致。
pub fn case_equivalence_check() -> Result<(), u32> {
    let mut i = 0;
    while i < GUID_N {
        let lower = parse_guid(GUID_TEXTS[i])?;
        let upper = parse_guid(&GUID_TEXTS[i].to_uppercase())?;
        let mut k = 0;
        while k < GUID_BYTES {
            if lower[k] != upper[k] {
                return Err(GeCode::CaseEquivalenceBroken.code());
            }
            k += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 双跑一致性：全集对拍两跑逐位一致。
pub fn repro_check() -> Result<(), u32> {
    let a = reconcile_all()?;
    let b = reconcile_all()?;
    if a != b {
        return Err(GeCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫：全部诊断码落 [SEG_BASE, SEG_BASE+0x40)。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GeCode::ALL.len() {
        let v = GeCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

/// 黄金全链：表自检→秩序→零偏差→大小写等价→伪造 10/10→双跑一致。
pub fn golden_run() -> Result<(u32, u32), u32> {
    table_selfcheck()?;
    rank_order_check()?;
    let (dev, cases) = reconcile_all()?;
    if dev != 0 || cases != GUID_N as u32 {
        return Err(GeCode::DeviationNonZero.code());
    }
    case_equivalence_check()?;
    let (c, t) = forge_round(JUDGE_ROUNDS)?;
    if c != t || t != FORGED_N as u32 * JUDGE_ROUNDS {
        return Err(GeCode::ForgedMissed.code());
    }
    roundtrip_check()?;
    let (rj, tot, amb) = byte_perturb_probe()?;
    if tot != 160 || rj + amb != tot {
        return Err(GeCode::ForgedMissed.code());
    }
    fuzz_forge_probe(32)?;
    policy_check()?;
    repro_check()?;
    Ok((c, t))
}

// ── 文本↔字节往返序列化器 ─────────────────────────────────────────
/// 单 nibble → 小写 hex 字符。
fn hex_char(v: u8) -> u8 {
    if v < 10 {
        b'0' + v
    } else {
        b'a' + v - 10
    }
}

/// 字节 → 规范文本（parse_guid 的严格逆）：8-4-4-4-12 小写带连字符。
/// 往返性质是解析器无歧义的机器口径：bytes→text→bytes 逐字节恒等。
pub fn format_guid(g: &[u8; 16]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < GUID_BYTES {
        if i == 4 || i == 6 || i == 8 || i == 10 {
            out.push('-');
        }
        let hi = g[i] >> 4;
        let lo = g[i] & 0x0F;
        out.push(hex_char(hi) as char);
        out.push(hex_char(lo) as char);
        i += 1;
    }
    out
}

/// 往返一致：5 规范 GUID bytes→text→bytes 逐字节恒等（大小写规范形）。
pub fn roundtrip_check() -> Result<(), u32> {
    let table: [[u8; 16]; GUID_N] = [GUID_VISTA, GUID_WIN7, GUID_WIN8, GUID_WIN81, GUID_WIN10];
    let mut i = 0;
    while i < GUID_N {
        let text = format_guid(&table[i]);
        let back = parse_guid(&text)?;
        let mut k = 0;
        while k < GUID_BYTES {
            if back[k] != table[i][k] {
                return Err(GeCode::TableInconsistent.code());
            }
            k += 1;
        }
        // 逆方向：冻结文本 bytes→text 必须还原冻结文本（规范小写形）。
        if text != GUID_TEXTS[i] {
            return Err(GeCode::TableInconsistent.code());
        }
        i += 1;
    }
    Ok(())
}

// ── 世代政策表（秩→min_build，单调冻结） ───────────────────────────
/// 世代政策（写死：各世代声明面生效的最低 Build 口径——
/// Vista=6000 / 7=7600 / 8=9200 / 8.1=9600 / 10=10240，与 ERA_RANKS
/// 平行单调；Win11 复用 Win10 GUID 故政策同轨）。
pub const ERA_MIN_BUILD: [u32; GUID_N] = [6000, 7600, 9200, 9600, 10240];

/// 政策自洽：与秩平行严格单调（政策漂移即世代口径失锚）。
pub fn policy_check() -> Result<(), u32> {
    let mut i = 1;
    while i < GUID_N {
        if ERA_MIN_BUILD[i] <= ERA_MIN_BUILD[i - 1] {
            return Err(GeCode::RankOrderBroken.code());
        }
        i += 1;
    }
    // 与秩互证：秩递增则政策递增（1:1 平行）。
    if ERA_MIN_BUILD.len() != GUID_N {
        return Err(GeCode::TableInconsistent.code());
    }
    Ok(())
}

// ── 单字节扰动核（逐字节位判别） ───────────────────────────────────
/// 对 5 规范 GUID ×16 字节 ×2 扰动（异或 0x01 / 加 1）= 160 样本。
/// 性质：任一规范 GUID 的单字节扰动不得命中闭集（任何两规范 GUID
/// 至少差 2 个字节，单字节扰动不可达）——表在每一字节位上可判别。
/// 返回（拒收数, 样本数, 不可判别数——扰动恰好等于另一规范）。
pub fn byte_perturb_probe() -> Result<(u32, u32, u32), u32> {
    let table: [[u8; 16]; GUID_N] = [GUID_VISTA, GUID_WIN7, GUID_WIN8, GUID_WIN81, GUID_WIN10];
    let is_canonical = |g: &[u8; 16]| -> bool {
        let mut i = 0;
        while i < GUID_N {
            let mut eq = true;
            let mut k = 0;
            while k < GUID_BYTES {
                if g[k] != table[i][k] {
                    eq = false;
                    break;
                }
                k += 1;
            }
            if eq {
                return true;
            }
            i += 1;
        }
        false
    };
    let mut rejected = 0u32;
    let mut total = 0u32;
    let mut ambiguous = 0u32;
    let mut gi = 0;
    while gi < GUID_N {
        let mut byte_i = 0;
        while byte_i < GUID_BYTES {
            // 扰动 A：异或 0x01。
            let mut a = table[gi];
            a[byte_i] ^= 0x01;
            total += 1;
            if is_canonical(&a) {
                ambiguous += 1;
            } else if lookup(&a).is_err() {
                rejected += 1;
            }
            // 扰动 B：加 1（环绕）。
            let mut b = table[gi];
            b[byte_i] = b[byte_i].wrapping_add(1);
            total += 1;
            if is_canonical(&b) {
                ambiguous += 1;
            } else if lookup(&b).is_err() {
                rejected += 1;
            }
            byte_i += 1;
        }
        gi += 1;
    }
    if rejected + ambiguous != total {
        return Err(GeCode::ForgedMissed.code());
    }
    Ok((rejected, total, ambiguous))
}

// ── LCG 确定性模糊注入 ─────────────────────────────────────────────
/// 本地 LCG（自包含，禁 rand crate）：种子 = SEG_BASE。
pub struct Lcg(u64);
impl Lcg {
    /// 新发生器（种子写死调用侧）。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    /// 下一步（Numerical Recipes 常数）。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
}

/// LCG 模糊注入：32 个伪随机 16 字节 GUID，逐个喂 lookup。
/// 全部必须被拒（伪随机字节命中 5 规范闭集概率 ~2^-415，确定性
/// 种子下恒为 0——如有命中即表自坏，必红）。
pub fn fuzz_forge_probe(n: usize) -> Result<(u32, u32), u32> {
    let mut rng = Lcg(SEG_BASE as u64);
    let mut rejected = 0u32;
    let mut i = 0;
    while i < n {
        let mut g = [0u8; 16];
        let mut k = 0;
        while k < GUID_BYTES {
            g[k] = (rng.next() >> 33) as u8;
            k += 1;
        }
        if lookup(&g).is_err() {
            rejected += 1;
        }
        i += 1;
    }
    if rejected != n as u32 {
        return Err(GeCode::ForgedMissed.code());
    }
    Ok((rejected, n as u32))
}

// ── 判据（20 条，断言性质） ────────────────────────────────────────
/// 判据入口（单模块 ≤20 条）。
pub fn run_unx18442_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18442 supportedOS GUID 世代表");
    // 1. 表自检：字节/文本互证逐字节一致。
    s.add("J1-表自检互证", table_selfcheck().is_ok(), "5 规范文本↔字节表逐字节一致");
    // 2. 秩序单调：Vista<7<8<8.1<10。
    s.add("J1-世代秩序单调", rank_order_check().is_ok(), "秩 1..5 严格递增");
    // 3. 全集零偏差（J1 前半句主体）。
    let all_ok = match reconcile_all() {
        Ok((dev, cases)) => dev == 0 && cases == GUID_N as u32,
        Err(_) => false,
    };
    s.add("J1-微软口径零偏差", all_ok, "5 例 resolve↔冻结期望 偏差=0");
    // 4. 大小写等价。
    s.add("J1-大小写等价", case_equivalence_check().is_ok(), "大写文本同解析");
    // 5. 伪造识别 10/10（J1 后半句主体）。
    let forge_ok = match forge_round(JUDGE_ROUNDS) {
        Ok((c, t)) => c == t && t == (FORGED_N * JUDGE_ROUNDS as usize) as u32,
        Err(_) => false,
    };
    s.add("J1-伪造识别10/10", forge_ok, "10 形态 ×3 轮全拒");
    // 6. 逐形态闭集：空输入拒。
    s.add("J1-注入:空输入", resolve("").is_err(), "空串显式拒");
    // 7. 逐形态：长度非法拒。
    s.add("J1-注入:长度非法", resolve(&"a".repeat(TEXT_LEN - 1)).is_err(), "35 字符拒");
    // 8. 逐形态：连字符段位错拒。
    s.add("J1-注入:连字符错", resolve("e2011457-1546-43c5-a5fe0-08deee3d3f0").is_err(), "段位偏移拒");
    // 9. 逐形态：非法 hex 字符拒。
    s.add("J1-注入:非法hex", resolve("g2011457-1546-43c5-a5fe-008deee3d3f0").is_err(), "'g' 拒");
    // 10. 未知 GUID（合法格式、闭集外）拒。
    s.add("J1-注入:未知GUID", resolve("12345678-1234-5678-9abc-def012345678").is_err(), "闭集外拒");
    // 11. 单 nibble 翻转识别（近似伪装）。
    let mut near = String::from(GUID_TEXTS[4]);
    near.replace_range(35..36, "b");
    s.add("J1-注入:近似翻转", resolve(&near).is_err(), "末位 a→b 识别");
    // 12. 双跑一致。
    s.add("J1-双跑一致", repro_check().is_ok(), "全集对拍两跑同值");
    // 13. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7EA40 窗口");
    // 14. 黄金全链。
    let g = golden_run();
    let g_ok = g.is_ok();
    s.add_dyn("J1-黄金全链", g_ok, &format!("golden={:?}", g));
    // 15. 幂等：同文本重复解析两次逐字节一致（确定性）。
    let idem = {
        let a = parse_guid(GUID_TEXTS[3]);
        let b = parse_guid(GUID_TEXTS[3]);
        match (a, b) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
    };
    s.add("J1-解析幂等", idem, "同输入同输出");
    // 16. 文本↔字节往返：bytes→text→bytes 逐字节恒等且规范小写形还原。
    s.add("J1-往返一致", roundtrip_check().is_ok(), "解析器无歧义（严格逆）");
    // 17. 单字节扰动核：160 样本逐字节位判别。
    let pert = byte_perturb_probe();
    let pert_ok = match pert {
        Ok((rj, tot, amb)) => tot == 160 && rj + amb == tot,
        Err(_) => false,
    };
    s.add("J1-单字节扰动160", pert_ok, "5 GUID ×16 字节 ×2 扰动全拒（或恰为规范）");
    // 18. LCG 模糊注入：32 伪随机 GUID 全拒。
    let fz = fuzz_forge_probe(32);
    let fz_ok = matches!(fz, Ok((r, n)) if r == n && n == 32);
    s.add("J1-LCG模糊全拒", fz_ok, "种子=SEG_BASE 确定性 32/32");
    // 19. 世代政策表单调。
    s.add("J1-世代政策单调", policy_check().is_ok(), "min_build 与秩平行严格递增");
    // 20. 大写规范形往返：大写文本解析后序列化还原冻结小写文本。
    let up_rt = {
        let up = parse_guid(&GUID_TEXTS[2].to_uppercase());
        match up {
            Ok(g) => format_guid(&g) == GUID_TEXTS[2],
            Err(_) => false,
        }
    };
    s.add("J1-大写往返规范形", up_rt, "大小写等价 + 序列化规范小写");
    s
}

// ── 测试 ───────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx18442_checks_green() {
        let set = run_unx18442_checks();
        assert!(set.all_passed(), "UNX-F18442 red: {}", set.red_items_text());
    }

    #[test]
    fn t_resolve_canonical() {
        assert_eq!(resolve(GUID_TEXTS[0]), Ok((RANK_VISTA, "Vista")));
        assert_eq!(resolve(GUID_TEXTS[4]), Ok((RANK_WIN10, "Windows 10/11")));
    }

    #[test]
    fn t_forged_all_rejected() {
        let forged = make_forged();
        assert_eq!(forged.len(), FORGED_N);
        for f in forged.iter() {
            assert!(resolve(f).is_err(), "伪造未被拒: {}", f);
        }
    }

    #[test]
    fn t_codes_window() {
        assert!(segment_guard());
        assert_eq!(GeCode::EmptyInput.code(), SEG_BASE);
        assert!(GeCode::CaseEquivalenceBroken.code() < SEG_BASE + 0x40);
    }

    #[test]
    fn t_uppercase_ok() {
        assert_eq!(
            parse_guid("8E0F7A12-BFB3-4FE8-B9A5-48FD50A15A9A"),
            parse_guid(GUID_TEXTS[4])
        );
    }
}
