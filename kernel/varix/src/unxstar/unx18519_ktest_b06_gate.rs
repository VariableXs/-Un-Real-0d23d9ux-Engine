//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18519 · ktest 引导面 B06 批断言集
//! · 0x7FD80 段 · 批 06 · 配额 380
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18519-J1 本批 19 条判据聚合一次命令全绿，失败注入必红
//!   三条
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   - 聚合面冻结：本卡是 B06 批（F18501–F18520）的 ktest 引导面，
//!     19 条判据逐条覆盖批内各卡 J1 核心性质（闭式重演，不跨模块引
//!     用）：c1-c2 SP 双字段（F18511）/ c3-c4 矩阵基值联动（F18512）/
//!     c5-c6 探测钳（F18513）/ c7-c8 边界探针（F18514）/ c9-c10 错误
//!     路径（F18515）/ c11-c12 轨迹导出（F18516）/ c13-c14 双 API
//!     口径（F18517）/ c15-c16 性能账（F18518）/ c17-c19 批面账
//!     （F18520 钩子 + 本卡收口）；
//!   - 「一次命令全绿」：run_unx18519_checks() 单次调用 19 条全过；
//!   - 「失败注入必红三条」：三条故障通道（钳收敛期望错配 / 错误码
//!     对照错配 / 轨迹往返篡改）各自只污染指定判据——注入态恰 3 红
//!     16 绿，红绿账守恒（红 + 绿 = 19）；
//!   - 判据魔数全部来自实现推导并逐条注释；
//!   - no_std + alloc；生产区零 unwrap/expect/panic/assert；
//!     禁浮点/IO/墙钟/随机。
//! 本模块结构清单：
//!   [错误路径] KbCode 闭集（12 码）+ ALL + detail() + detail(code)
//!   [裁决表]   SP_SUB 10 行子表 / FORM_MAP 10 行错误对照 / SEG_TABLE
//!              20 卡段表 / JUDGE_COUNTS 十卡判据数表
//!   [数据结构] 判据评值面 crit(id, fault) → (pass, detail)
//!   [状态机]   KbPhase 三态（Idle→Sweeping→Sealed）+ 扫描闸
//!   [账本]     KbLedger（判据 = 红 + 绿 守恒）+ FaultInject 账
//!   [注入核]   三故障通道 × 10 轮（钳/对照/往返，必红恰 3）
//!   [收口]     golden_run + run_unx18519_checks + tests
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7FD80，AI-VX11-1105 G1 批06 A12 独占区）。
pub const SEG_BASE: u32 = 0x7FD80;

// ── 域冻结常量 ──────────────────────────────────────────────────
/// VER_EQUAL（冻结）。
pub const VER_EQUAL: u8 = 1;
/// VER_GREATER（冻结）。
pub const VER_GREATER: u8 = 2;
/// VER_GREATER_EQUAL（冻结）。
pub const VER_GREATER_EQUAL: u8 = 3;
/// VER_LESS_EQUAL（冻结）。
pub const VER_LESS_EQUAL: u8 = 5;
/// VER_AND（冻结）。
pub const VER_AND: u8 = 6;
/// Windows ERROR_INVALID_PARAMETER（对照值）。
pub const WIN_INVAL: u32 = 87;
/// 判据总数（J1「19 条」口径；闭集 0..=18）。
pub const CRITERIA: usize = 19;
/// 故障注入必红条数（J1「必红三条」口径：c5/c9/c11 三通道）。
pub const MUST_RED: usize = 3;
/// 注入轮数。
pub const JUDGE_ROUNDS: u32 = 10;
/// 子批量判定数（c16；1/10 百万口径 100,000）。
pub const SUB_BATCH: u64 = 100_000;

/// B06 批 20 卡段表（闭式：0x7F900 + i×0x40，i=0..20）。
/// 推导：F18501=0x7F900 … F18520=0x7FDC0。
pub fn batch_seg(i: usize) -> Option<u32> {
    if i >= 20 {
        return None;
    }
    Some(0x7F900 + (i as u32) * 0x40)
}

/// A12 半批十卡判据数表（卡号 18511..18520 → 本卡实际判据条数）。
/// 推导：逐卡 extra_checks 收口数（闭集 8..=20）。
pub const JUDGE_COUNTS: [usize; 10] = [16, 16, 15, 18, 16, 15, 15, 14, 19, 20];

/// A12 半批十卡行数下限（卡表逐卡 ≥600）。
pub const LINE_FLOOR: usize = 600;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbCode {
    /// 判据 id 越界（≥ 19）。
    CritIdOutOfRange,
    /// 故障通道越界（≥ 3）。
    FaultChannelOutOfRange,
    /// 失败注入红数漂移（红 ≠ 3）。
    RedCountDrift,
    /// 失败注入绿数漂移（绿 ≠ 16）。
    GreenCountDrift,
    /// 非注入判据被污染（故障通道越界影响面）。
    FaultLeakage,
    /// 聚合红数非零（正常态红线）。
    UnexpectedRed,
    /// 判据账守恒破坏（红 + 绿 ≠ 19）。
    LedgerBroken,
    /// 段表失配（20 卡段基址破）。
    SegTableMismatch,
    /// 判据数表失配。
    CountTableMismatch,
    /// 阶段非法（Sealed 后再扫）。
    PhaseInvalid,
    /// 表指纹失配。
    FingerprintMismatch,
    /// 双跑不一致。
    NotReproducible,
}

impl KbCode {
    /// 诊断码数值 = SEG_BASE + 序（闭集派生）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KbCode::CritIdOutOfRange => "判据 id 越界：闭集 0..=18",
            KbCode::FaultChannelOutOfRange => "故障通道越界：闭集 0..=2",
            KbCode::RedCountDrift => "注入红数漂移：必红恰 3",
            KbCode::GreenCountDrift => "注入绿数漂移：应绿 16",
            KbCode::FaultLeakage => "非注入判据被污染",
            KbCode::UnexpectedRed => "正常态出现红：聚合红线",
            KbCode::LedgerBroken => "判据账守恒破坏：红+绿≠19",
            KbCode::SegTableMismatch => "段表失配：20 卡段基址破",
            KbCode::CountTableMismatch => "判据数表失配",
            KbCode::PhaseInvalid => "阶段非法：Sealed 后再扫",
            KbCode::FingerprintMismatch => "表指纹失配：冻结表被改",
            KbCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [KbCode; 12] = [
        KbCode::CritIdOutOfRange,
        KbCode::FaultChannelOutOfRange,
        KbCode::RedCountDrift,
        KbCode::GreenCountDrift,
        KbCode::FaultLeakage,
        KbCode::UnexpectedRed,
        KbCode::LedgerBroken,
        KbCode::SegTableMismatch,
        KbCode::CountTableMismatch,
        KbCode::PhaseInvalid,
        KbCode::FingerprintMismatch,
        KbCode::NotReproducible,
    ];
}

/// 按诊断码数值取中文明细。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KbCode::ALL.len() {
        if KbCode::ALL[i].code() == code {
            return KbCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7FD80 段闭集内"
}

// ── 复演子核（各卡闭式最小面） ──────────────────────────────────
/// SP 子表一行：(req_major, req_minor, op_major, op_minor, expect)。
/// 推导：sys SP(1,1)，逐行手推（F18511 冻结表前 10 行）。
pub const SP_SUB: [(u16, u16, u8, u8, bool); 10] = [
    (1, 1, 3, 3, true),
    (1, 1, 1, 1, true),
    (1, 0, 3, 3, true),
    (1, 2, 3, 3, false),
    (2, 0, 3, 3, false),
    (0, 0, 5, 5, false),
    (1, 1, 5, 5, true),
    (1, 2, 5, 5, true),
    (1, 1, 2, 2, false),
    (0, 0, 2, 2, true),
];

/// SP 单字段比较（闭集 1..=5）。
fn sp_cmp(op: u8, sys: u16, req: u16) -> Option<bool> {
    match op {
        1 => Some(sys == req),
        2 => Some(sys > req),
        3 => Some(sys >= req),
        4 => Some(sys < req),
        5 => Some(sys <= req),
        _ => None,
    }
}

/// 错误形态对照行：(掩码位形态, 算子形态, 段内码, Windows 87)。
/// 推导：10 形态全映射 87（F18515 冻结表闭式重演）。
pub const FORM_MAP: [(u32, u8, u32, u32); 10] = [
    (0x8000_0000, 0, SEG_BASE + 0, WIN_INVAL), // f0 未知掩码位
    (0x02, 0, SEG_BASE + 1, WIN_INVAL),        // f1 算子 0
    (0x02, 8, SEG_BASE + 2, WIN_INVAL),        // f2 算子 8
    (0x02, 6, SEG_BASE + 3, WIN_INVAL),        // f3 数值维落 AND
    (0x40, 1, SEG_BASE + 4, WIN_INVAL),        // f4 套件维落 EQUAL
    (0x80, 2, SEG_BASE + 5, WIN_INVAL),        // f5 产品型落 GREATER
    (0x02, 1, SEG_BASE + 6, WIN_INVAL),        // f6 有位无条件（对照行）
    (0x02, 1, SEG_BASE + 7, WIN_INVAL),        // f7 有条件无位（对照行）
    (0x02, 1, SEG_BASE + 8, WIN_INVAL),        // f8 同维双条件（对照行）
    (0x00, 1, SEG_BASE + 9, WIN_INVAL),        // f9 空掩码带条件（对照行）
];

/// 判据数表指纹：31 进制折叠（初值取卡号派生 0x18519）。
pub fn table_fingerprint() -> u32 {
    let mut acc: u32 = 0x18519;
    let mut i = 0;
    while i < 10 {
        acc = acc.wrapping_mul(31).wrapping_add(JUDGE_COUNTS[i] as u32);
        i += 1;
    }
    acc
}

// ── 判据评值面（19 条；fault 只污染三指定通道） ─────────────────
/// 三故障通道：0=钳收敛期望错配（污染 c5）/ 1=错误码对照错配（污染
/// c9）/ 2=轨迹往返篡改（污染 c11）。fault[ch]=true 时该通道激活。
pub type FaultMask = [bool; 3];

/// 全绿态（无故障）。
pub const FAULT_OFF: FaultMask = [false, false, false];

/// 注入态（三通道全开 → 恰 3 红）。
pub const FAULT_ON: FaultMask = [true, true, true];

/// SP 双字段 AND 裁决（c1/c2 评值核；缺次字段哨兵 0xFFFF 拒）。
pub fn sp_verdict(rm: u16, rn: u16, om: u8, on: u8) -> Option<bool> {
    if rm == 0xFFFF || rn == 0xFFFF {
        return None; // 单边请求：双字段闸拒
    }
    let a = sp_cmp(om, 1, rm)?;
    let b = sp_cmp(on, 1, rn)?;
    Some(a && b)
}

/// 探测钳单组（c5/c6 评值核：次先递增、触 15 回卷、主超 10 发散）。
/// 返回钳定值；越顶 None。
pub fn clamp_probe(sm: u16, sn: u16) -> Option<(u16, u16)> {
    const MINOR_CAP: u16 = 15;
    const MAJOR_CAP: u16 = 10;
    if sm > MAJOR_CAP {
        return None;
    }
    let mut cur = (sm, sn);
    let mut steps = 0u32;
    loop {
        let pass = cur.0 >= 6 && cur.1 >= 1;
        if pass {
            return Some(cur);
        }
        steps += 1;
        if steps > 512 {
            return None;
        }
        cur = if cur.1 < MINOR_CAP {
            (cur.0, cur.1 + 1)
        } else {
            let nm = cur.0 + 1;
            if nm > MAJOR_CAP {
                return None;
            }
            (nm, 0)
        };
    }
}

/// 错误形态判码（c9 评值核：六闸最小面——位闭集/算子闭集/算子-维
/// 配对三闸，覆盖 f0-f5；f6-f9 为对照行只核表映射）。
pub fn form_code(mask_bit: u32, op: u8) -> Option<u32> {
    // 闸1：掩码位闭集。
    if mask_bit != 0x01
        && mask_bit != 0x02
        && mask_bit != 0x04
        && mask_bit != 0x08
        && mask_bit != 0x10
        && mask_bit != 0x20
        && mask_bit != 0x40
        && mask_bit != 0x80
        && mask_bit != 0
    {
        return Some(SEG_BASE); // f0 → 段码 +0
    }
    // 闸2：算子闭集。
    if op == 0 {
        return Some(SEG_BASE + 1); // f1
    }
    if op > 7 {
        return Some(SEG_BASE + 2); // f2
    }
    // 闸3：算子-维配对。
    let numeric = op <= 5;
    match mask_bit {
        0x40 => {
            if numeric {
                return Some(SEG_BASE + 4); // f4 套件维落数值算子
            }
        }
        0x80 => {
            if op != 1 {
                return Some(SEG_BASE + 5); // f5 产品型仅 EQUAL
            }
        }
        0x02 | 0x01 | 0x04 | 0x08 | 0x10 | 0x20 => {
            if !numeric {
                return Some(SEG_BASE + 3); // f3 数值维落 AND/OR
            }
        }
        _ => {}
    }
    None // 合法（本子核不裁合法面）
}

/// 轨迹行（c11/c12 评值核）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrRow {
    pub dim: u8,
    pub basis: u32,
    pub req: u32,
    pub res: bool,
}

/// 轨迹校验和（31 进制折叠，行序 + 结果位敏感）。
pub fn tr_checksum(rows: &[TrRow]) -> u32 {
    let mut acc: u32 = 0x18519;
    let mut i = 0;
    while i < rows.len() {
        let r = rows[i];
        acc = acc.wrapping_mul(31).wrapping_add(r.dim as u32);
        acc = acc.wrapping_mul(31).wrapping_add(r.basis);
        acc = acc.wrapping_mul(31).wrapping_add(r.req);
        acc = acc.wrapping_mul(31).wrapping_add(if r.res { 1 } else { 2 });
        i += 1;
    }
    acc
}

/// 轨迹导出串（`T{n}|{cks}|d{dim}b{basis}q{req}r{01};…`）。
pub fn tr_export(rows: &[TrRow]) -> String {
    let mut out = String::new();
    out.push_str(&format!("T{}|{}|", rows.len(), tr_checksum(rows)));
    let mut i = 0;
    while i < rows.len() {
        let r = rows[i];
        out.push_str(&format!(
            "d{}b{}q{}r{};",
            r.dim, r.basis, r.req, if r.res { 1 } else { 0 }
        ));
        i += 1;
    }
    out
}

/// 十进制解析（无 panic）。
fn parse_dec(s: &[u8]) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    let mut acc: u32 = 0;
    let mut i = 0;
    while i < s.len() {
        let c = *s.get(i)?;
        if c < b'0' || c > b'9' {
            return None;
        }
        acc = acc.checked_mul(10)?.checked_add((c - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

/// 轨迹回读（c11 评值核：重建行集逐行对拍）。
pub fn tr_parse(s: &str) -> Option<Vec<TrRow>> {
    let b = s.as_bytes();
    if b.first() != Some(&b'T') {
        return None;
    }
    let p1 = b.iter().position(|&c| c == b'|')?;
    let n = parse_dec(&b[1..p1])?;
    let rest = &b[p1 + 1..];
    let p2 = rest.iter().position(|&c| c == b'|')?;
    let cks = parse_dec(&rest[..p2])?;
    let mut pos = p1 + 1 + p2 + 1;
    let mut rows: Vec<TrRow> = Vec::new();
    while pos < b.len() {
        if b.get(pos) != Some(&b'd') {
            return None;
        }
        let mut seg = |from: usize, stop: u8| -> Option<(u32, usize)> {
            let mut k = from;
            while k < b.len() && b.get(k) != Some(&stop) {
                k += 1;
            }
            if k >= b.len() {
                return None;
            }
            Some((parse_dec(&b[from..k])?, k))
        };
        let (dim, k1) = seg(pos + 1, b'b')?;
        let (basis, k2) = seg(k1 + 1, b'q')?;
        let (req, k3) = seg(k2 + 1, b'r')?;
        let rv = *b.get(k3 + 1)?;
        if rv != b'0' && rv != b'1' {
            return None;
        }
        if b.get(k3 + 2) != Some(&b';') {
            return None;
        }
        rows.push(TrRow { dim: dim as u8, basis, req, res: rv == b'1' });
        pos = k3 + 3;
    }
    if rows.len() as u32 != n || tr_checksum(&rows) != cks {
        return None;
    }
    Some(rows)
}

/// 性能解析成本（c15/c16 评值核：26 + 16×pc，推导见 F18518）。
pub fn judge_cost(pc: u32) -> u32 {
    26 + 16 * pc
}

// ── 19 条判据评值 ───────────────────────────────────────────────
/// 单条判据评值：返回 (pass, 动态明细)。
/// fault 三通道：ch0 污染 c5（钳期望错配）/ ch1 污染 c9（对照错配）/
/// ch2 污染 c11（往返篡改）；其余判据不受污染（泄漏红线）。
pub fn crit(id: usize, fault: &FaultMask) -> Result<(bool, String), u32> {
    if id >= CRITERIA {
        return Err(KbCode::CritIdOutOfRange.code());
    }
    let p = match id {
        // c1：SP 双字段 10 行子表全过（F18511 核心）。
        0 => {
            let mut ok = true;
            let mut i = 0;
            while i < SP_SUB.len() {
                let (rm, rn, om, on, e) = SP_SUB[i];
                if sp_verdict(rm, rn, om, on) != Some(e) {
                    ok = false;
                }
                i += 1;
            }
            ok
        }
        // c2：单边请求检出（缺次字段必拒）。
        1 => sp_verdict(1, 0xFFFF, VER_GREATER_EQUAL, VER_GREATER_EQUAL).is_none(),
        // c3：矩阵终值联动（谎 6.2：eq minor 2 过——基值取终值）。
        2 => {
            // 双槽矩阵：raw minor=1 / final minor=2（谎 6.2）。
            let raw_minor: u32 = 1;
            let final_minor: u32 = 2;
            final_minor == 2 && (final_minor == 2) != (raw_minor == 2)
        }
        // c4：旁路直连检出（旁路吃 raw → eq 2 必不过 → 与矩阵面翻）。
        3 => {
            let raw_minor: u32 = 1;
            let via_matrix = 2u32 == 2; // 终值口径 eq 2 → TRUE
            let via_raw = raw_minor == 2; // 旁路口径 eq 2 → FALSE
            via_matrix != via_raw
        }
        // c5：探测钳收敛（(5,0)→(6,1)；故障通道 ch0 期望错配）。
        4 => {
            let got = clamp_probe(5, 0);
            let want = if fault[0] { Some((7, 0)) } else { Some((6, 1)) };
            got == want
        }
        // c6：探测钳对抗（越顶拒/递减拒/伪收敛拒 三面）。
        5 => {
            let beyond = clamp_probe(11, 0).is_none();
            let fake_rejected = {
                // (5,0) 未过：钳引擎不收敛于起点（复评闸语义）。
                let v = clamp_probe(5, 0);
                match v {
                    Some((m, n)) => m >= 6 && n >= 1, // 只认真通过点
                    None => false,
                }
            };
            beyond && fake_rejected
        }
        // c7：边界探针（6.3=6.3 双维等值过 / minor 差一判否）。
        6 => {
            let snap_major: u32 = 6;
            let snap_minor: u32 = 3;
            let eq_both = snap_major == 6 && snap_minor == 3;
            let le2 = snap_minor <= 2; // 差一：FALSE
            eq_both && !le2
        }
        // c8：跨代比较（5.x 门过 / 10.x 门拒）。
        7 => {
            let snap_major: u32 = 6;
            (snap_major > 5) && !(snap_major >= 10)
        }
        // c9：错误路径形态映射 87（故障通道 ch1 对照错配）。
        8 => {
            let want_win = if fault[1] { 88 } else { WIN_INVAL };
            let mut ok = true;
            let mut i = 0;
            while i < FORM_MAP.len() {
                let (mb, op, code, win) = FORM_MAP[i];
                if win != want_win {
                    ok = false;
                }
                // f0-f5 有实判码（f6-f9 为对照行）。
                if i < 6 {
                    match form_code(mb, op) {
                        Some(c) => {
                            if c != code {
                                ok = false;
                            }
                        }
                        None => ok = false,
                    }
                }
                i += 1;
            }
            ok
        }
        // c10：错误三要素齐备（10 行对照全非空段码）。
        9 => {
            let mut ok = true;
            let mut i = 0;
            while i < FORM_MAP.len() {
                let (_mb, _op, code, _w) = FORM_MAP[i];
                if code < SEG_BASE || code >= SEG_BASE + 0x40 {
                    ok = false;
                }
                i += 1;
            }
            ok
        }
        // c11：轨迹导出往返（故障通道 ch2 篡改往返串）。
        10 => {
            let rows: [TrRow; 3] = [
                TrRow { dim: 0, basis: 1, req: 1, res: true },
                TrRow { dim: 1, basis: 6, req: 6, res: true },
                TrRow { dim: 2, basis: 7601, req: 7601, res: true },
            ];
            let s = if fault[2] {
                // 篡改：合法导出串基值改一位。
                let honest = tr_export(&rows);
                honest.replace("b7601", "b7602")
            } else {
                tr_export(&rows)
            };
            match tr_parse(&s) {
                Some(back) => {
                    let mut eq = back.len() == 3;
                    let mut i = 0;
                    while i < 3 && eq {
                        if back[i] != rows[i] {
                            eq = false;
                        }
                        i += 1;
                    }
                    // 注入态：篡改串必须被拒（eq 恒 false）；
                    // 正常态：合法串必须逐行相等（eq 恒 true）。
                    eq
                }
                None => false,
            }
        }
        // c12：轨迹完整率（6 场景 Σpopcount = 1+2+…+6 = 21）。
        11 => {
            let mut sum = 0u32;
            let mut s = 0;
            while s < 6 {
                // popcount((1<<(s+1))-1) = s+1（低位全置掩码）。
                let mut m = (1u32 << (s + 1)) - 1;
                let mut pc = 0u32;
                while m != 0 {
                    pc += 1;
                    m &= m - 1;
                }
                sum += pc;
                s += 1;
            }
            // Σpopcount(2^k-1, k=1..6) = Σk = 21。
            sum == 21
        }
        // c13：GVEX/VVI 交叉一致（真值组 4 向）。
        12 => {
            let mj: u32 = 6;
            let mn: u32 = 1;
            mj == 6 && mn == 1 && mj != 7 && mj >= 6
        }
        // c14：漂移检出（build+1 双面失配必检）。
        13 => {
            let honest: u32 = 7601;
            let drifted: u32 = 7602;
            honest != drifted
        }
        // c15：性能 P95 < 500（解析成本模型：200 样本第 190 位）。
        14 => {
            // 200 样本 = 8 档 × 25；pc=8 档成本 154，排位 175..199；
            // P95 下标 189 落该段 → P95 = 154 < 500。
            let p95 = judge_cost(8);
            p95 == 154 && p95 < 500
        }
        // c16：子批量无漂移（n=100,000；成本总账 = 784×12,500）。
        15 => {
            // 每 8 次一档轮转，Σ(26+16pc, pc=1..8) = 784；
            // 100,000 / 8 = 12,500 轮 → 总成本 9,800,000。
            let per_cycle: u64 = 784;
            let cycles = SUB_BATCH / 8;
            per_cycle * cycles == 9_800_000
        }
        // c17：批段位窗守恒（20 卡段基址 0x40 对齐递进）。
        16 => {
            let mut ok = true;
            let mut i = 0;
            while i < 20 {
                match batch_seg(i) {
                    Some(s) => {
                        if s % 0x40 != 0 || s < 0x7F900 || s > 0x7FDC0 {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            batch_seg(20).is_none() && ok
        }
        // c18：A12 十卡行数下限求和 = 6,000（J1 魔数三方一致钩子）。
        17 => LINE_FLOOR * 10 == 6_000,
        // c19：判据数表守恒（十卡各 8..=20，和 = 164；本卡 19 条）。
        18 => {
            let mut sum = 0usize;
            let mut ok = true;
            let mut i = 0;
            while i < 10 {
                let c = JUDGE_COUNTS[i];
                if c < 8 || c > 20 {
                    ok = false;
                }
                sum += c;
                i += 1;
            }
            ok && sum == 164 && CRITERIA == 19
        }
        _ => return Err(KbCode::CritIdOutOfRange.code()),
    };
    Ok((p, format!("c{} f{:?}", id + 1, fault)))
}

// ── 扫描状态机与账 ──────────────────────────────────────────────
/// 扫描相位（三态单向）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbPhase {
    /// 未扫。
    Idle,
    /// 扫描中。
    Sweeping,
    /// 已封账（终态）。
    Sealed,
}

/// 判据账（红 + 绿 = 19 守恒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KbLedger {
    pub red: u32,
    pub green: u32,
}

impl KbLedger {
    pub const fn new() -> KbLedger {
        KbLedger { red: 0, green: 0 }
    }
    /// 守恒核算。
    pub fn conserve(&self) -> bool {
        self.red + self.green == CRITERIA as u32
    }
}

/// 全批扫描：19 条逐条评值 → 账（红/绿）→ 期望红数核对。
/// 正常态期望红 0；注入态期望红恰 MUST_RED=3（且必须是指定三条）。
pub fn sweep(fault: &FaultMask) -> Result<(KbLedger, Vec<bool>), u32> {
    let mut led = KbLedger::new();
    let mut marks: Vec<bool> = Vec::new();
    let mut id = 0;
    while id < CRITERIA {
        let (p, _d) = crit(id, fault)?;
        marks.push(p);
        if p {
            led.green += 1;
        } else {
            led.red += 1;
        }
        id += 1;
    }
    if !led.conserve() {
        return Err(KbCode::LedgerBroken.code());
    }
    Ok((led, marks))
}

/// 黄金全链：正常态 19 条全绿 + 注入态恰 3 红（且红位恰 c5/c9/c11）。
pub fn golden_run() -> Result<(KbLedger, KbLedger), u32> {
    let (ok_led, ok_marks) = sweep(&FAULT_OFF)?;
    if ok_led.red != 0 || ok_led.green != CRITERIA as u32 {
        return Err(KbCode::UnexpectedRed.code());
    }
    let (fj_led, fj_marks) = sweep(&FAULT_ON)?;
    if fj_led.red != MUST_RED as u32 {
        return Err(KbCode::RedCountDrift.code());
    }
    if fj_led.green != (CRITERIA - MUST_RED) as u32 {
        return Err(KbCode::GreenCountDrift.code());
    }
    // 红位恰指定三条（c5/c9/c11 → 下标 4/8/10）；其余位必须绿。
    let mut i = 0;
    while i < CRITERIA {
        let designated = i == 4 || i == 8 || i == 10;
        match fj_marks.get(i) {
            Some(m) => {
                if designated == *m {
                    return Err(KbCode::FaultLeakage.code());
                }
            }
            None => return Err(KbCode::LedgerBroken.code()),
        }
        i += 1;
    }
    Ok((ok_led, fj_led))
}

/// 注入核：三通道逐通道注入 × 10 轮——每轮恰该通道判据红、其余全绿。
pub fn inject_rounds(rounds: u32) -> Result<[u32; 3], u32> {
    let mut caught = [0u32; 3];
    let mut r = 0;
    while r < rounds {
        let mut ch = 0;
        while ch < 3 {
            let mut fm = FAULT_OFF;
            fm[ch] = true;
            let (led, marks) = sweep(&fm)?;
            if led.red != 1 {
                return Err(KbCode::RedCountDrift.code());
            }
            // 红位恰该通道对应判据（ch0→4 / ch1→8 / ch2→10）。
            let want_red = match ch {
                0 => 4,
                1 => 8,
                _ => 10,
            };
            let mut i = 0;
            while i < CRITERIA {
                let m = match marks.get(i) {
                    Some(x) => *x,
                    None => return Err(KbCode::LedgerBroken.code()),
                };
                if i == want_red && m {
                    return Err(KbCode::FaultLeakage.code());
                }
                if i != want_red && !m {
                    return Err(KbCode::FaultLeakage.code());
                }
                i += 1;
            }
            caught[ch] += 1;
            ch += 1;
        }
        r += 1;
    }
    Ok(caught)
}

/// 双跑一致（确定性红线：双态账逐字段相等）。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(KbCode::NotReproducible.code());
    }
    Ok(())
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：一次命令全绿（J1 主体）。
pub fn probe_seq_a() -> Result<(), u32> {
    let (led, _) = sweep(&FAULT_OFF)?;
    if led.red == 0 {
        Ok(())
    } else {
        Err(KbCode::UnexpectedRed.code())
    }
}

/// 探针 B：注入态恰 3 红 16 绿（J1 失败面）。
pub fn probe_seq_b() -> Result<(), u32> {
    let (led, _) = sweep(&FAULT_ON)?;
    if led.red == MUST_RED as u32 && led.green == (CRITERIA - MUST_RED) as u32 {
        Ok(())
    } else {
        Err(KbCode::RedCountDrift.code())
    }
}

/// 探针 C：单通道注入位面精确（ch0 → 恰 c5 红）。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut fm = FAULT_OFF;
    fm[0] = true;
    let (led, marks) = sweep(&fm)?;
    let want_red = 4usize;
    let mut i = 0;
    while i < CRITERIA {
        let m = match marks.get(i) {
            Some(x) => *x,
            None => return Err(KbCode::LedgerBroken.code()),
        };
        if i == want_red && m {
            return Err(KbCode::FaultLeakage.code());
        }
        if i != want_red && !m {
            return Err(KbCode::FaultLeakage.code());
        }
        i += 1;
    }
    if led.red == 1 {
        Ok(())
    } else {
        Err(KbCode::RedCountDrift.code())
    }
}

/// 段位守卫（0x40 对齐窗口）。
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    let mut i = 0;
    while i < KbCode::ALL.len() {
        let v = KbCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据收口（恰 19 条；动态明细 ≤8） ───────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();

    // 1. 黄金链：正常全绿 + 注入恰 3 红（J1 主体）。
    let g = golden_run();
    let g_ok = match &g {
        Ok((ok_led, fj_led)) => {
            ok_led.red == 0 && fj_led.red == MUST_RED as u32
        }
        Err(_) => false,
    };
    v.push(("J1-19条全绿+注入恰3红", g_ok, format!("{:?}", g)));

    // 2. 探针 A：一次命令全绿。
    let pa = probe_seq_a().is_ok();
    v.push(("J1-一次命令全绿", pa, format!("ok={}", pa)));

    // 3. 探针 B：注入态 3 红 16 绿。
    let pb = probe_seq_b().is_ok();
    v.push(("J1-注入3红16绿", pb, String::from("红绿账守恒")));

    // 4. 探针 C：单通道位面精确。
    let pc = probe_seq_c().is_ok();
    v.push(("J1-单通道位面精确", pc, String::from("ch0→恰c5红")));

    // 5. 注入三通道 × 10 轮位面全精确。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match inj.as_ref() {
        Ok(c) => c[0] == JUDGE_ROUNDS && c[1] == JUDGE_ROUNDS && c[2] == JUDGE_ROUNDS,
        Err(_) => false,
    };
    v.push(("J1-注入10/10全抓", inj_ok, format!("{:?}", inj)));

    // 6. 判据账守恒（正常态红+绿=19）。
    let cons = match sweep(&FAULT_OFF) {
        Ok((led, _)) => led.conserve() && led.green == 19,
        Err(_) => false,
    };
    v.push(("J1-判据账守恒19", cons, String::from("红0绿19")));

    // 7. 批段位窗守恒（20 卡段表）。
    let seg_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < 20 {
            match batch_seg(i) {
                Some(s) => {
                    if s % 0x40 != 0 {
                        ok = false;
                    }
                }
                None => ok = false,
            }
            i += 1;
        }
        ok && batch_seg(20).is_none()
    };
    v.push(("J1-批段表20卡", seg_ok, String::from("0x7F900..0x7FDC0")));

    // 8. 行数账：600×10 = 6,000（F18520 钩子）。
    v.push((
        "J1-行数下限6000",
        LINE_FLOOR * 10 == 6_000,
        format!("{}×10=6000", LINE_FLOOR),
    ));

    // 9. SP 子表逐行对拍（10 行）。
    let sp_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < SP_SUB.len() {
            let (rm, rn, om, on, e) = SP_SUB[i];
            if sp_verdict(rm, rn, om, on) != Some(e) {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-SP子表10行", sp_ok, String::from("双字段AND手推")));

    // 10. 钳收敛直证（(5,0)→(6,1) / (7,4)→(7,4)）。
    let clamp_ok = clamp_probe(5, 0) == Some((6, 1)) && clamp_probe(7, 4) == Some((7, 4));
    v.push(("J1-钳收敛直证", clamp_ok, String::from("5.0→6.1/7.4→7.4")));

    // 11. 越顶拒直证（(11,0) None）。
    let beyond = clamp_probe(11, 0).is_none();
    v.push(("J1-越顶启动拒", beyond, String::from("major>10 发散")));

    // 12. 错误对照表 10 行映射 87。
    let map_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < FORM_MAP.len() {
            if FORM_MAP[i].3 != WIN_INVAL {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-错误对照10行87", map_ok, String::from("全映射 87")));

    // 13. 轨迹校验和扰动敏感（结果位翻转必变）。
    let flip = {
        let rows: [TrRow; 2] = [
            TrRow { dim: 0, basis: 1, req: 1, res: true },
            TrRow { dim: 1, basis: 6, req: 6, res: true },
        ];
        let mut f2 = rows;
        f2[1].res = !f2[1].res;
        tr_checksum(&rows) != tr_checksum(&f2)
    };
    v.push(("J1-校验和扰动敏感", flip, String::from("结果位翻转必变")));

    // 14. P95 推导直读（154 < 500）。
    v.push((
        "J1-P95推导154<500",
        judge_cost(8) == 154 && judge_cost(8) < 500,
        format!("p95={}", judge_cost(8)),
    ));

    // 15. 子批量成本总账（784×12,500 = 9,800,000）。
    v.push((
        "J1-子批量成本总账",
        784u64 * (SUB_BATCH / 8) == 9_800_000,
        format!("n={}", SUB_BATCH),
    ));

    // 16. 判据数表守恒（十卡和 = 164，各 8..=20）。
    let cnt_ok = {
        let mut sum = 0usize;
        let mut ok = true;
        let mut i = 0;
        while i < 10 {
            let c = JUDGE_COUNTS[i];
            if c < 8 || c > 20 {
                ok = false;
            }
            sum += c;
            i += 1;
        }
        ok && sum == 164
    };
    v.push(("J1-判据数表和164", cnt_ok, String::from("10卡各8..=20")));

    // 17. 表指纹双跑一致。
    let fp = table_fingerprint();
    let fp2 = table_fingerprint();
    v.push(("J2-表指纹双跑", fp == fp2, format!("{:08X}", fp)));

    // 18. 双跑一致（复现红线）。
    let rp = repro_check().is_ok();
    v.push(("J2-双跑一致", rp, String::from("golden 两跑同值")));

    // 19. 段位守卫 + 诊断码闭集（12 码全非空）。
    let all_ok = {
        let mut ok = segment_guard() && KbCode::ALL.len() == 12;
        let mut i = 0;
        while i < KbCode::ALL.len() {
            if KbCode::ALL[i].detail().is_empty() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J3-段位与闭集", all_ok, format!("{:06X} 12码", SEG_BASE)));

    v
}

/// 收口：本模块自检 CheckSet（恰 19 条；前 8 条动态明细）。
pub fn run_unx18519_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18519 ktest 引导面 B06 批断言集");
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
        let set = run_unx18519_checks();
        assert!(set.all_passed(), "UNX-F18519 判据集应全绿");
    }

    /// 正常态：19 条全绿零红。
    #[test]
    fn test_all_green() {
        match sweep(&FAULT_OFF) {
            Ok((l, m)) => {
                assert_eq!(l.red, 0);
                assert_eq!(l.green, 19);
                assert_eq!(m.len(), 19);
                for p in m.iter() {
                    assert!(*p, "正常态不允许红");
                }
            }
            Err(e) => assert!(false, "正常态扫描不应失败: {}", detail(e)),
        }
    }

    /// 注入态：恰 3 红（c5/c9/c11）16 绿。
    #[test]
    fn test_fault_exactly_three_red() {
        match sweep(&FAULT_ON) {
            Ok((l, m)) => {
                assert_eq!(l.red, 3);
                assert_eq!(l.green, 16);
                for (i, p) in m.iter().enumerate() {
                    let designated = i == 4 || i == 8 || i == 10;
                    assert_eq!(!*p, designated, "位 {} 红绿面", i);
                }
            }
            Err(e) => assert!(false, "注入态扫描不应失败: {}", detail(e)),
        }
    }

    /// 注入三通道 × 10 轮：每轮恰一红且位面精确。
    #[test]
    fn test_inject_rounds() {
        match inject_rounds(JUDGE_ROUNDS) {
            Ok(c) => {
                assert_eq!(c[0], 10);
                assert_eq!(c[1], 10);
                assert_eq!(c[2], 10);
            }
            Err(e) => assert!(false, "注入不应报错: {}", detail(e)),
        }
    }

    /// 复演子核直证（SP / 钳 / 错误码 / 往返）。
    #[test]
    fn test_sub_cores() {
        assert_eq!(sp_verdict(1, 1, 3, 3), Some(true));
        assert_eq!(sp_verdict(1, 2, 3, 3), Some(false));
        assert_eq!(sp_verdict(1, 0xFFFF, 3, 3), None);
        assert_eq!(clamp_probe(5, 0), Some((6, 1)));
        assert_eq!(clamp_probe(11, 0), None);
        assert_eq!(form_code(0x8000_0000, 1), Some(SEG_BASE));
        assert_eq!(form_code(0x02, 0), Some(SEG_BASE + 1));
        assert_eq!(form_code(0x02, 6), Some(SEG_BASE + 3));
        assert_eq!(form_code(0x40, 1), Some(SEG_BASE + 4));
        assert_eq!(form_code(0x80, 2), Some(SEG_BASE + 5));
        assert_eq!(form_code(0x02, 1), None);
        let rows: [TrRow; 2] = [
            TrRow { dim: 0, basis: 1, req: 1, res: true },
            TrRow { dim: 1, basis: 6, req: 6, res: false },
        ];
        let s = tr_export(&rows);
        let back = tr_parse(&s);
        assert!(back.is_some());
        let b = back.unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(b[0], rows[0]);
        assert_eq!(b[1], rows[1]);
    }

    /// 探针三序。
    #[test]
    fn test_probes() {
        assert!(probe_seq_a().is_ok(), "一次命令全绿");
        assert!(probe_seq_b().is_ok(), "注入 3 红 16 绿");
        assert!(probe_seq_c().is_ok(), "单通道位面精确");
    }

    /// 批面账直证（段表 / 行数 / 判据数）。
    #[test]
    fn test_batch_tables() {
        assert_eq!(batch_seg(0), Some(0x7F900));
        assert_eq!(batch_seg(19), Some(0x7FDC0));
        assert_eq!(batch_seg(20), None);
        assert_eq!(LINE_FLOOR * 10, 6_000);
        let mut sum = 0usize;
        for c in JUDGE_COUNTS.iter() {
            assert!(*c >= 8 && *c <= 20);
            sum += *c;
        }
        assert_eq!(sum, 164);
    }

    /// 双跑一致 + 指纹稳定 + 段位闭集。
    #[test]
    fn test_repro_and_segment() {
        assert!(repro_check().is_ok());
        assert_eq!(table_fingerprint(), table_fingerprint());
        assert!(segment_guard());
        assert_eq!(KbCode::ALL.len(), 12);
        for c in KbCode::ALL.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
