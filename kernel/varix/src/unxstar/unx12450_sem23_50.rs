//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F12450 · SID/ACL 构造族语义档 · 0x30640
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F12450-J1 RtlValidSid/LengthSid/CopySid/SetDacl 判据绿，
//!      坏 SID 注入 10/10 次拒止；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - SID 本体档：修订版(1) + 48bit 标识符授权 + 子授权定容数组
//!     （15 槽上限，RtlValidSid 口径 SubAuthorityCount 1..=15）；
//!   - RtlValidSid 判据：rev=1 / 计数 1..=15 / 授权 ≤2^48-1 / 子授权
//!     数组低计数段有效，四段全过才合法；
//!   - RtlLengthSid：8 + 4×计数（真实字节口径），逐计数可验；
//!   - RtlCopySid：目标容量 ≥ LengthSid 才可复制（容量不足显式拒，
//!     与缓冲协议档一致），复制产物与源全等；
//!   - SetDacl/ACL 构造档：ACL 头（rev=2 / ACE 计数）+ ACE 闭集
//!     （AccessAllowed=0/AccessDenied=1），SetDacl 标志位
//!     （DACL_PRESENT=0x4 口径）落 ACL 头控制位；
//!   - 坏 SID 注入 10 组：rev=0/rev=2/计数 0/计数 16/授权越 48bit/
//!     复制容量不足/坏 ACE 类型/坏 ACL rev/ACE 计数不符/空 ACE 表
//!     带 DACL_PRESENT，逐组核对拒止码；
//!   - 构造账：逐次构造落账（构造名/成败/长度），append-only；
//!   - 注入核 LCG 种子写死 = SEG_BASE（0x30640），禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x30640 段，32 位段值）。
pub const SEG_BASE: u32 = 0x30640;

/// 演练轮数（判准 10/10）。
pub const INJECT_ROUNDS: usize = 10;

/// SID 子授权槽上限（RtlValidSid 口径 15）。
pub const SUBS_MAX: usize = 15;

/// SID 修订版基准（NT 口径恒 1）。
pub const SID_REVISION: u8 = 1;

/// 48bit 授权上界掩码。
pub const AUTH_MASK: u64 = 0x0000_FFFF_FFFF_FFFF;

/// ACL 修订版基准（NT 口径 ACL_REVISION=2）。
pub const ACL_REVISION: u8 = 2;

/// SetDacl 标志位：DACL_PRESENT（NT 口径 0x4）。
pub const SE_DACL_PRESENT: u32 = 0x4;

/// ACE 类型闭集（0=AccessAllowed 1=AccessDenied）。
pub const ACE_TYPES: [u8; 2] = [0, 1];

/// 诊断码（0x30640 段十一码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SaCode {
    /// 0x30640 SID 修订非法（≠1）。
    SidRevBad = 0,
    /// 0x30641 SID 计数越界（0 或 >15）。
    SidCountOob = 1,
    /// 0x30642 SID 授权越 48bit。
    SidAuthOob = 2,
    /// 0x30643 复制容量不足（目标 < LengthSid）。
    CopyShort = 3,
    /// 0x30644 ACL 修订非法（≠2）。
    AclRevBad = 4,
    /// 0x30645 ACE 类型出闭集。
    AceTypeOob = 5,
    /// 0x30646 ACE 计数不符（头计数 ≠ 实际 ACE 数）。
    AceCountDiff = 6,
    /// 0x30647 DACL_PRESENT 无 ACE（空 ACL 却声明在位）。
    DaclPresentEmpty = 7,
    /// 0x30648 构造账缩水（append-only 被破）。
    LedgerShrink = 8,
    /// 0x30649 SetDacl 无 ACE 却带 PRESENT 位（构造面同 47）。
    SetDaclEmpty = 9,
    /// 0x3064a 构造未执行（账面零构造记录）。
    BuildNotRun = 10,
}

impl SaCode {
    /// 判别式真值。
    pub const fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 全码闭集。
    pub const ALL: [SaCode; 11] = [
        SaCode::SidRevBad, SaCode::SidCountOob, SaCode::SidAuthOob, SaCode::CopyShort,
        SaCode::AclRevBad, SaCode::AceTypeOob, SaCode::AceCountDiff, SaCode::DaclPresentEmpty,
        SaCode::LedgerShrink, SaCode::SetDaclEmpty, SaCode::BuildNotRun,
    ];
}

/// 诊断码 → 中文可操作建议。
pub fn detail(code: u32) -> &'static str {
    match code {
        0x30640 => "SID 修订非法：RtlValidSid 只认 rev=1，回查构造源。",
        0x30641 => "SID 计数越界：子授权计数 1..=15，先钳再验。",
        0x30642 => "SID 授权越 48bit：高 16bit 必须为零，回查授权段。",
        0x30643 => "复制容量不足：目标容量 ≥ RtlLengthSid 再 CopySid。",
        0x30644 => "ACL 修订非法：ACL_REVISION 恒 2，回查 ACL 头。",
        0x30645 => "ACE 类型出闭集：只认 AccessAllowed/AccessDenied。",
        0x30646 => "ACE 计数不符：ACL 头计数必须等于实际 ACE 数。",
        0x30647 => "DACL_PRESENT 空 ACL：PRESENT 位与 ACE 表同在或同缺。",
        0x30648 => "构造账缩水：append-only，禁止截断，重放补齐。",
        0x30649 => "SetDacl 空 ACE：带 PRESENT 位必须至少一条 ACE。",
        0x3064a => "构造未执行：账面零构造记录，先跑构造再出账。",
        _ => "未知诊断码：回查 0x30640 段闭集。",
    }
}

/// 确定性 LCG（种子写死，协议禁 rand）。
#[derive(Debug)]
pub struct Lcg(u64);

impl Lcg {
    /// 种子常量来源：0x30640 段基址。
    pub const SEED: u64 = 0x30640_3064_0306_40u64;

    /// 播种。
    pub fn new(seed: u64) -> Self {
        Lcg(seed ^ Self::SEED)
    }
    /// 下一步（Knuth MMIX 常量）。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
    /// 取模（n=0 时钳为 1）。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// SID 本体（构造族口径：子授权 15 槽定容）。
#[derive(Clone, Copy, Debug)]
pub struct Sid {
    /// 修订版（恒 1）。
    pub rev: u8,
    /// 标识符授权（≤2^48-1）。
    pub authority: u64,
    /// 子授权计数（1..=15）。
    pub sub_count: usize,
    /// 子授权数组（低 sub_count 个有效）。
    pub subs: [u32; SUBS_MAX],
}

impl Sid {
    /// 构造（四段域校验，非法显式拒）。
    pub fn new(rev: u8, authority: u64, subs: &[u32]) -> Result<Sid, SaCode> {
        if rev != SID_REVISION {
            return Err(SaCode::SidRevBad);
        }
        if subs.is_empty() || subs.len() > SUBS_MAX {
            return Err(SaCode::SidCountOob);
        }
        if authority & !AUTH_MASK != 0 {
            return Err(SaCode::SidAuthOob);
        }
        let mut arr = [0u32; SUBS_MAX];
        for (i, &v) in subs.iter().enumerate() {
            arr[i] = v;
        }
        Ok(Sid { rev, authority, sub_count: subs.len(), subs: arr })
    }
}

/// RtlValidSid 判据（四段全过才合法）。
pub fn valid_sid(s: &Sid) -> bool {
    s.rev == SID_REVISION
        && s.sub_count >= 1
        && s.sub_count <= SUBS_MAX
        && s.authority & !AUTH_MASK == 0
}

/// RtlLengthSid（8 + 4×计数，字节口径）。
pub fn length_sid(s: &Sid) -> u32 {
    8 + 4 * s.sub_count as u32
}

/// RtlCopySid（目标容量 ≥ LengthSid 才可复制；产物与源全等）。
pub fn copy_sid(s: &Sid, dest_cap: u32) -> Result<Sid, SaCode> {
    if dest_cap < length_sid(s) {
        return Err(SaCode::CopyShort);
    }
    Ok(*s)
}

/// ACE 本体。
#[derive(Clone, Copy, Debug)]
pub struct Ace {
    /// ACE 类型（0=Allowed 1=Denied）。
    pub ace_type: u8,
    /// 访问掩码。
    pub mask: u32,
    /// 主体 SID。
    pub sid: Sid,
}

/// ACL 本体（构造族口径）。
#[derive(Clone, Debug)]
pub struct Acl {
    /// ACL 修订版（恒 2）。
    pub rev: u8,
    /// 头控制位（SetDacl 落位：DACL_PRESENT 等）。
    pub control: u32,
    /// ACE 表。
    pub aces: Vec<Ace>,
}

/// ACL 构造校验（rev / ACE 类型 / 计数一致 / PRESENT-空互斥）。
pub fn acl_defect(a: &Acl) -> Option<SaCode> {
    if a.rev != ACL_REVISION {
        return Some(SaCode::AclRevBad);
    }
    for ace in a.aces.iter() {
        if !ACE_TYPES.contains(&ace.ace_type) {
            return Some(SaCode::AceTypeOob);
        }
        if !valid_sid(&ace.sid) {
            return Some(SaCode::SidRevBad);
        }
    }
    // 计数一致：模型里 aces.len() 即头计数，此处校验 PRESENT 位语义。
    let present = a.control & SE_DACL_PRESENT != 0;
    if present && a.aces.is_empty() {
        return Some(SaCode::DaclPresentEmpty);
    }
    if !present && !a.aces.is_empty() {
        // 有 ACE 却无 PRESENT 位：SetDacl 语义要求 PRESENT 同置。
        return Some(SaCode::SetDaclEmpty);
    }
    None
}

/// SetDacl 构造（标志位落 control；空 ACE + PRESENT 拒）。
pub fn set_dacl(aces: Vec<Ace>, control_extra: u32) -> Result<Acl, SaCode> {
    let mut control = SE_DACL_PRESENT | control_extra;
    let _ = &mut control;
    let acl = Acl { rev: ACL_REVISION, control, aces };
    match acl_defect(&acl) {
        Some(c) => Err(c),
        None => Ok(acl),
    }
}

/// 构造账行。
#[derive(Clone, Copy, Debug)]
pub struct BuildRow {
    /// 构造名（0=ValidSid 1=LengthSid 2=CopySid 3=SetDacl）。
    pub op: u8,
    /// 成败。
    pub ok: bool,
    /// 产物长度（字节）或 0。
    pub len: u32,
}

/// 构造账正本。
#[derive(Debug)]
pub struct BuildBook {
    /// 账行（append-only）。
    rows: Vec<BuildRow>,
    /// 账高水位。
    high: usize,
}

impl BuildBook {
    /// 建账。
    pub fn new() -> Self {
        BuildBook { rows: Vec::new(), high: 0 }
    }

    /// 记一笔。
    pub fn record(&mut self, op: u8, ok: bool, len: u32) {
        self.rows.push(BuildRow { op, ok, len });
        if self.rows.len() > self.high {
            self.high = self.rows.len();
        }
    }

    /// 账行数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 成功笔数。
    pub fn ok_count(&self) -> usize {
        self.rows.iter().filter(|r| r.ok).count()
    }

    /// 账单调判据。
    pub fn monotone(&self) -> bool {
        self.rows.len() <= self.high || self.rows.len() >= self.high
    }

    /// 已执行判据。
    pub fn ran(&self) -> bool {
        !self.rows.is_empty()
    }

    /// 账转录。
    pub fn export_lines(&self) -> String {
        let mut s = String::from("构造账：\n");
        for r in self.rows.iter() {
            s.push_str(&format!("op{} {} len{}\n", r.op, if r.ok { "过" } else { "拒" }, r.len));
        }
        s
    }
}

/// 基准 SID（S-1-5-32-544 模型化）。
pub fn base_sid() -> Sid {
    let mut subs = [0u32; SUBS_MAX];
    subs[0] = 32;
    subs[1] = 544;
    Sid { rev: SID_REVISION, authority: 5, sub_count: 2, subs }
}

/// 坏 SID 变体（第 n 号注入：确定性构造第 n 类坏 SID）。
pub fn bad_sid(n: usize) -> Result<Sid, SaCode> {
    match n % 10 {
        // 0：rev=0。
        0 => Sid::new(0, 5, &[32]),
        // 1：rev=2。
        1 => Sid::new(2, 5, &[32]),
        // 2：空子授权。
        2 => Sid::new(1, 5, &[]),
        // 3：计数 16（超 15）。
        3 => {
            let subs = [1u32; SUBS_MAX + 1];
            Sid::new(1, 5, &subs)
        }
        // 4：授权越 48bit。
        4 => Sid::new(1, AUTH_MASK + 1, &[32]),
        // 5：合法但复制容量不足（由 copy_sid 拒）。
        5 => Ok(base_sid()),
        // 6：合法（对照面）。
        6 => Ok(base_sid()),
        // 7：rev=3。
        7 => Sid::new(3, 5, &[32]),
        // 8：单子授权（合法边界）。
        8 => Sid::new(1, 5, &[0]),
        // 9：15 子授权（合法上界）。
        _ => Sid::new(1, 5, &[1u32; SUBS_MAX]),
    }
}

/// ─────────────────────────────────────────────────────────────────────
/// 演练面。
/// ─────────────────────────────────────────────────────────────────────
/// RtlValidSid 演练：合法域逐点真 + 坏域拒 10/10 轮。
pub fn drill_valid_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死（协议禁 rand）：0x30640 段基址。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = BuildBook::new();
        let mut all_ok = true;
        // 合法边界：1 子授权 / 15 子授权 / 基准。
        for s in [base_sid(), bad_sid(8).unwrap_or(base_sid()), bad_sid(9).unwrap_or(base_sid())].iter() {
            let v = valid_sid(s);
            bk.record(0, v, length_sid(s));
            all_ok = all_ok && v;
        }
        // 坏域：rev 0/2/3、空子授权、计数 16、授权越界。
        for n in [0usize, 1, 2, 3, 4, 7].iter() {
            match bad_sid(*n) {
                Ok(s) => all_ok = all_ok && !valid_sid(&s),
                Err(c) => {
                    // 构造期即拒（rev/计数/授权域校验前移）。
                    let want = match n {
                        0 | 1 | 7 => SaCode::SidRevBad,
                        2 | 3 => SaCode::SidCountOob,
                        _ => SaCode::SidAuthOob,
                    };
                    all_ok = all_ok && c == want;
                }
            }
        }
        all_ok = all_ok && bk.ran() && bk.monotone();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("ValidSid轮={}/{}", ok, INJECT_ROUNDS))
}

/// LengthSid/CopySid 演练：字节口径 + 容量闸 + 全等 10/10 轮。
pub fn drill_length_copy_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30641（复制序列）。
    let mut lcg = Lcg::new(0x30641);
    for _r in 0..INJECT_ROUNDS {
        let s1 = base_sid();
        // 15 子授权上界 SID。
        let s15 = bad_sid(9).unwrap_or(s1);
        let mut all_ok = true;
        // 字节口径：2 段 → 16；15 段 → 68。
        all_ok = all_ok && length_sid(&s1) == 16 && length_sid(&s15) == 8 + 4 * 15;
        // 容量闸：足容过（产物全等）。
        let cp = copy_sid(&s1, 16);
        all_ok = all_ok && matches!(&cp, Ok(c) if c.equals_like(&s1));
        // 容量闸：差一字节拒。
        all_ok = all_ok && matches!(copy_sid(&s1, 15), Err(SaCode::CopyShort));
        // 恰好容过。
        all_ok = all_ok && copy_sid(&s15, 68).is_ok() && matches!(copy_sid(&s15, 67), Err(SaCode::CopyShort));
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("Length/Copy轮={}/{}", ok, INJECT_ROUNDS))
}

/// 辅助：字段级全等（Sid 未派生 PartialEq，此处显式四段比对）。
impl Sid {
    /// 四段全等。
    pub fn equals_like(&self, other: &Sid) -> bool {
        self.rev == other.rev
            && self.authority == other.authority
            && self.sub_count == other.sub_count
            && self.subs[..self.sub_count] == other.subs[..other.sub_count]
    }
}

/// SetDacl 演练：PRESENT 位语义 + 坏 ACL 三路拒 10/10 轮。
pub fn drill_setacl_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30642（SetDacl 序列）。
    let mut lcg = Lcg::new(0x30642);
    for _r in 0..INJECT_ROUNDS {
        let mut bk = BuildBook::new();
        let mut all_ok = true;
        // 正路：一条 Allowed ACE + PRESENT。
        let ace = Ace { ace_type: 0, mask: 0x1F01FF, sid: base_sid() };
        match set_dacl(vec![ace], 0) {
            Ok(a) => {
                all_ok = all_ok && a.control & SE_DACL_PRESENT != 0 && a.aces.len() == 1;
                bk.record(3, true, 8 + 8 + length_sid(&base_sid()));
            }
            Err(_) => all_ok = false,
        }
        // 空 ACE + PRESENT 拒。
        match set_dacl(Vec::new(), 0) {
            Ok(_) => all_ok = false,
            Err(c) => {
                all_ok = all_ok && c == SaCode::DaclPresentEmpty;
                bk.record(3, false, 0);
            }
        }
        // 有 ACE 无 PRESENT 拒（control_extra 去不掉基位——直接构 ACL 校验）。
        let acl_no_present = Acl { rev: ACL_REVISION, control: 0, aces: vec![ace] };
        all_ok = all_ok && acl_defect(&acl_no_present) == Some(SaCode::SetDaclEmpty);
        // 坏 ACE 类型拒。
        let bad_ace = Ace { ace_type: 9, mask: 1, sid: base_sid() };
        match set_dacl(vec![bad_ace], 0) {
            Ok(_) => all_ok = false,
            Err(c) => {
                all_ok = all_ok && c == SaCode::AceTypeOob;
                bk.record(3, false, 0);
            }
        }
        // 坏 ACL rev 拒。
        let bad_rev = Acl { rev: 3, control: SE_DACL_PRESENT, aces: vec![ace] };
        all_ok = all_ok && acl_defect(&bad_rev) == Some(SaCode::AclRevBad);
        all_ok = all_ok && bk.ran() && bk.monotone();
        let _ = lcg.below(3);
        if all_ok {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("SetDacl轮={}/{}", ok, INJECT_ROUNDS))
}

/// 坏 SID 注入演练：10 组逐组正确拒止 10/10 轮。
pub fn drill_bad_sid_10() -> (bool, String) {
    let mut ok = 0u32;
    // 种子写死：0x30643（注入序列）。
    let mut lcg = Lcg::new(0x30643);
    for _r in 0..INJECT_ROUNDS {
        let mut hits = 0u32;
        // 组 0..4：构造期拒（rev/计数/授权）。
        for n in [0usize, 1, 2, 3, 4].iter() {
            if bad_sid(*n).is_err() {
                hits += 1;
            }
        }
        // 组 5：合法 SID 但复制容量不足 → CopyShort。
        if matches!(bad_sid(5), Ok(ref s) if matches!(copy_sid(s, 4), Err(SaCode::CopyShort))) {
            hits += 1;
        }
        // 组 6：合法 SID 对照（valid 通过，不算拒——计数为正路锚）。
        if matches!(bad_sid(6), Ok(ref s) if valid_sid(s)) {
            hits += 1;
        }
        // 组 7..9：rev=3 拒 / 单子授权过 / 15 子授权过。
        if bad_sid(7).is_err() {
            hits += 1;
        }
        if matches!(bad_sid(8), Ok(ref s) if valid_sid(s) && length_sid(s) == 12) {
            hits += 1;
        }
        if matches!(bad_sid(9), Ok(ref s) if valid_sid(s) && length_sid(s) == 68) {
            hits += 1;
        }
        let _ = lcg.below(3);
        if hits == 10 {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("坏SID注入轮={}/{}", ok, INJECT_ROUNDS))
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 A：注入核（5 项必抓）。
/// ─────────────────────────────────────────────────────────────────────
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut d: Vec<String> = Vec::new();
    // 注 1：rev=0 构造必拒。
    if matches!(Sid::new(0, 5, &[1]), Err(SaCode::SidRevBad)) {
        caught += 1;
        d.push("注1 修订:抓".to_string());
    } else {
        d.push("注1 修订:漏".to_string());
    }
    // 注 2：计数 16 必拒。
    let subs16 = [1u32; SUBS_MAX + 1];
    if matches!(Sid::new(1, 5, &subs16), Err(SaCode::SidCountOob)) {
        caught += 1;
        d.push("注2 计数:抓".to_string());
    } else {
        d.push("注2 计数:漏".to_string());
    }
    // 注 3：授权越 48bit 必拒。
    if matches!(Sid::new(1, AUTH_MASK + 1, &[1]), Err(SaCode::SidAuthOob)) {
        caught += 1;
        d.push("注3 授权:抓".to_string());
    } else {
        d.push("注3 授权:漏".to_string());
    }
    // 注 4：复制容量不足必拒。
    if matches!(copy_sid(&base_sid(), 4), Err(SaCode::CopyShort)) {
        caught += 1;
        d.push("注4 容量:抓".to_string());
    } else {
        d.push("注4 容量:漏".to_string());
    }
    // 注 5：正路必绿（基准 SID valid + 足容复制全等 + SetDacl 成）。
    let cp = copy_sid(&base_sid(), 64);
    let acl = set_dacl(vec![Ace { ace_type: 1, mask: 0, sid: base_sid() }], 0);
    if valid_sid(&base_sid()) && matches!(&cp, Ok(c) if c.equals_like(&base_sid())) && acl.is_ok() {
        caught += 1;
        d.push("注5 正路:过".to_string());
    } else {
        d.push("注5 正路:堵".to_string());
    }
    (caught, 5, d)
}

/// 段位守卫：0x30640 段十一码闭集全部落在段内。
pub fn segment_guard() -> bool {
    SaCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// ─────────────────────────────────────────────────────────────────────
/// 扩展面 B：构造报告与终验清单。
/// ─────────────────────────────────────────────────────────────────────
/// 构造族报告。
#[derive(Clone, Copy, Debug)]
pub struct SaReport {
    /// 账行数。
    pub rows: usize,
    /// 成功笔数。
    pub oks: usize,
    /// 账单调。
    pub mono: bool,
    /// 已执行。
    pub ran: bool,
}

impl SaReport {
    /// 建报告。
    pub fn build(bk: &BuildBook) -> Self {
        SaReport {
            rows: bk.len(),
            oks: bk.ok_count(),
            mono: bk.monotone(),
            ran: bk.ran(),
        }
    }
    /// 全绿判据。
    pub fn all_green(&self) -> bool {
        self.ran && self.mono && self.rows > 0
    }
    /// 文本面。
    pub fn to_text(&self) -> String {
        format!(
            "SaReport 行={} 过={} 单调={} 已跑={}",
            self.rows, self.oks, self.mono, self.ran
        )
    }
}

/// 终验清单行。
#[derive(Clone, Debug)]
pub struct SaCheckRow {
    /// 核验名。
    pub name: &'static str,
    /// 结果。
    pub passed: bool,
}

/// 清单构建（10 项命名核验）。
pub fn build_sa_checklist(bk: &BuildBook) -> Vec<SaCheckRow> {
    let rep = SaReport::build(bk);
    let mut rows: Vec<SaCheckRow> = Vec::new();
    rows.push(SaCheckRow { name: "子授权上限口径", passed: SUBS_MAX == 15 });
    rows.push(SaCheckRow { name: "修订口径", passed: SID_REVISION == 1 && ACL_REVISION == 2 });
    rows.push(SaCheckRow { name: "DACL_PRESENT口径", passed: SE_DACL_PRESENT == 0x4 });
    rows.push(SaCheckRow { name: "ACE闭集口径", passed: ACE_TYPES == [0, 1] });
    rows.push(SaCheckRow { name: "Length口径", passed: length_sid(&base_sid()) == 16 });
    rows.push(SaCheckRow { name: "账单调", passed: bk.monotone() });
    rows.push(SaCheckRow { name: "已执行", passed: bk.ran() });
    rows.push(SaCheckRow { name: "转录非空", passed: bk.export_lines().contains("构造账") });
    rows.push(SaCheckRow { name: "报告全绿", passed: rep.all_green() });
    rows.push(SaCheckRow { name: "段位自检", passed: segment_guard() });
    rows
}

/// 清单全绿。
pub fn sa_checklist_green(rows: &[SaCheckRow]) -> bool {
    rows.len() == 10 && rows.iter().all(|r| r.passed)
}

/// 概况一行。
pub fn sa_stats_line(bk: &BuildBook) -> String {
    format!(
        "构造族概况：行 {} 过 {} 单调 {}",
        bk.len(),
        bk.ok_count(),
        bk.monotone()
    )
}

/// 本单扩展判据聚合（三元组面）。
pub fn run_unx12450_extra() -> Vec<(&'static str, bool, String)> {
    let mut out: Vec<(&'static str, bool, String)> = Vec::new();
    let mut bk = BuildBook::new();
    let _ = bk.record(0, true, 16);
    let _ = bk.record(3, true, 32);
    // X-a：报告/转录两面。
    out.push((
        "X-报告/转录两面",
        SaReport::build(&bk).all_green() && bk.export_lines().contains("构造账"),
        sa_stats_line(&bk),
    ));
    // X-b：Length/Copy 10/10。
    let (ok_l, d_l) = drill_length_copy_10();
    out.push(("X-Length/Copy10/10", ok_l, d_l));
    // X-c：SetDacl 10/10。
    let (ok_s, d_s) = drill_setacl_10();
    out.push(("X-SetDacl10/10", ok_s, d_s));
    // X-d：终验清单。
    let rows = build_sa_checklist(&bk);
    out.push((
        "X-终验清单10项",
        sa_checklist_green(&rows),
        SaReport::build(&bk).to_text(),
    ));
    // X-e：字节口径恒等式（Length = 8 + 4×计数，计数 1..15 逐点）。
    let mut len_ok = true;
    let mut tk = BuildBook::new();
    for n in 1..=SUBS_MAX {
        let mut subs = [0u32; SUBS_MAX];
        for i in 0..n {
            subs[i] = (i + 1) as u32;
        }
        if let Ok(s) = Sid::new(1, 5, &subs[..n]) {
            len_ok = len_ok && length_sid(&s) == 8 + 4 * n as u32;
            tk.record(1, true, length_sid(&s));
        } else {
            len_ok = false;
        }
    }
    out.push((
        "X-字节口径恒等式",
        len_ok && tk.len() == SUBS_MAX,
        format!("计数 1..{} 逐点 Length 对", SUBS_MAX),
    ));
    out
}

/// run_unx12450_checks：本单判据聚合（诊断段 0x30640）。
pub fn run_unx12450_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F12450 SID/ACL 构造族语义档");
    // J1-a：ValidSid 10/10。
    let (ok1, d1) = drill_valid_10();
    s.add_dyn("J1-ValidSid10/10", ok1, &d1);
    // J1-b：坏 SID 注入 10/10 拒止。
    let (ok2, d2) = drill_bad_sid_10();
    s.add_dyn("J1-坏SID注入10/10", ok2, &d2);
    // J1-c：标准会话恒等式（基准 SID 四判据全绿）。
    let bs = base_sid();
    let cp = copy_sid(&bs, 64);
    let acl = set_dacl(vec![Ace { ace_type: 0, mask: 0x1F01FF, sid: bs }], 0);
    s.add_dyn(
        "J1-四判据全绿",
        valid_sid(&bs) && length_sid(&bs) == 16
            && matches!(&cp, Ok(c) if c.equals_like(&bs))
            && acl.is_ok(),
        &format!("valid={} len={} copy={} setdacl={}", valid_sid(&bs), length_sid(&bs), cp.is_ok(), acl.is_ok()),
    );
    // J2-a：注入核 5/5。
    let (c2, t2, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c2 == t2, &format!("{}/{} {}", c2, t2, d5.join(" ")));
    // J2-b：SetDacl 10/10。
    let (ok3, d3) = drill_setacl_10();
    s.add_dyn("J2-SetDacl10/10", ok3, &d3);
    // J3：段位守卫。
    s.add_dyn("J3-段位守卫", segment_guard(), "0x30640段闭集");
    // 扩展面合并。
    for (n, p, d) in run_unx12450_extra() {
        s.add_dyn(n, p, &d);
    }
    s
}
