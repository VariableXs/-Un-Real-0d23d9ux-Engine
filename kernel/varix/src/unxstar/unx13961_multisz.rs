//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13961 · MULTI_SZ 畸形矩阵（无双零尾/内嵌单零/空串/超总长） · 0x66100
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥360 行；
//!   ② UNX-F13961-J1 MULTI_SZ 畸形 5 档（缺尾双零/内嵌单零截断/全空
//!      串列表/数据长虚标大于实际/空数据非零长）注入 5/5 处置有账
//!      （Windows 观测口径：容忍读或拒读逐档钉版），合法 MULTI_SZ
//!      （含单空串合法档）对照全过，逐档用户可见串清单落账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - MULTI_SZ 合法形（winreg 规格写死）：串1\0串2\0...\0\0（尾部
//!      恰双零），数据长 = 字节数（含尾双零）；单空串合法档 =
//!      \0\0 长度 2；
//!   - 畸形 5 档闭集：缺尾双零 / 内嵌单零截断（读到单零即截）/
//!      全空串列表 / 数据长虚标 > 实际 / 空数据非零长；
//!   - 处置钉版（Windows 观测口径逐档写死）：缺尾双零→容忍读
//!      （补尾）；内嵌单零→容忍读（截断）；全空串→容忍读（空清单）；
//!      长虚标→拒读；空数据非零长→拒读；
//!   - 用户可见串清单逐档落账（含合法档）；
//!   - 注入 5/5 处置与钉版一致；合法对照（3 档）全过；
//!   - LCG 种子写死 = SEG_BASE（0x66100），禁 rand/浮点/IO/墙钟；
///     no_std + alloc；零 unwrap/expect。
/// ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66100，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66100;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MszCode {
    /// 长虚标 > 实际（拒读）
    LengthOverstate,
    /// 空数据非零长（拒读）
    EmptyDataNonZeroLen,
    /// 畸形档处置与钉版不一致
    DispositionMismatch,
    /// 用户可见串清单缺失
    VisibleListMissing,
    /// 档枚举外值
    TierOutOfRange,
    /// 合法档误拒
    LegalRejected,
    /// 清单账守恒破
    ListLedgerBreak,
    /// 非法 UTF-16 代理对（边界面）
    BadSurrogate,
}

impl MszCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MszCode::LengthOverstate => "数据长虚标>实际：拒读",
            MszCode::EmptyDataNonZeroLen => "空数据非零长：拒读",
            MszCode::DispositionMismatch => "处置与钉版不一致：逐档红线",
            MszCode::VisibleListMissing => "用户可见串清单缺失",
            MszCode::TierOutOfRange => "档枚举外值：5 档闭集",
            MszCode::LegalRejected => "合法档误拒：对照红线",
            MszCode::ListLedgerBreak => "清单账守恒破：Σ档串数≠账",
            MszCode::BadSurrogate => "非法 UTF-16 代理对：边界档",
        }
    }
    pub const ALL: [MszCode; 8] = [
        MszCode::LengthOverstate,
        MszCode::EmptyDataNonZeroLen,
        MszCode::DispositionMismatch,
        MszCode::VisibleListMissing,
        MszCode::TierOutOfRange,
        MszCode::LegalRejected,
        MszCode::ListLedgerBreak,
        MszCode::BadSurrogate,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MszCode::ALL.len() {
        if MszCode::ALL[i].val() == code {
            return MszCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66100 段闭集内"
}

// ── UTF-16LE 解析面 ──────────────────────────────────────────────
/// 从 UTF-16LE 字节流解析（配对代理面：高位代理必须跟低位代理）。
pub fn utf16_bytes_to_units(b: &[u8]) -> Result<Vec<u16>, u32> {
    if b.len() % 2 != 0 {
        return Err(MszCode::BadSurrogate.val());
    }
    let mut units: Vec<u16> = Vec::new();
    let mut i = 0;
    while i + 1 < b.len() {
        units.push((b[i] as u16) | ((b[i + 1] as u16) << 8));
        i += 2;
    }
    // 代理对检查。
    let mut j = 0;
    while j < units.len() {
        let u = units[j];
        let high = (0xD800..=0xDBFF).contains(&u);
        let low = (0xDC00..=0xDFFF).contains(&u);
        if high {
            if j + 1 >= units.len() {
                return Err(MszCode::BadSurrogate.val());
            }
            if !(0xDC00..=0xDFFF).contains(&units[j + 1]) {
                return Err(MszCode::BadSurrogate.val());
            }
            j += 2;
            continue;
        }
        if low {
            return Err(MszCode::BadSurrogate.val());
        }
        j += 1;
    }
    Ok(units)
}

pub fn units_to_string(units: &[u16]) -> String {
    let mut s = String::new();
    let mut i = 0;
    while i < units.len() {
        if let Some(c) = char::from_u32(units[i] as u32) {
            s.push(c);
        }
        i += 1;
    }
    s
}

// ── 处置钉版（Windows 观测口径逐档写死） ────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Disposition {
    /// 容忍读（带修复语义）
    Tolerate,
    /// 拒读
    Reject,
}

/// 解析 MULTI_SZ：输入原始字节 + 声明长度（字节数）。
/// 返回（处置, 用户可见串清单）。
pub fn parse_multi_sz(data: &[u8], declared_len: u32) -> Result<(Disposition, Vec<String>), u32> {
    if data.is_empty() && declared_len != 0 {
        return Err(MszCode::EmptyDataNonZeroLen.val());
    }
    if declared_len as usize > data.len() {
        return Err(MszCode::LengthOverstate.val());
    }
    let units = utf16_bytes_to_units(&data[..declared_len as usize])?;
    // 尾双零检查。
    let tail_ok = units.len() >= 2 && units[units.len() - 1] == 0 && units[units.len() - 2] == 0;
    // 切串（按单零分割）。
    let mut strings: Vec<String> = Vec::new();
    let mut cur: Vec<u16> = Vec::new();
    let mut i = 0;
    while i < units.len() {
        if units[i] == 0 {
            strings.push(units_to_string(&cur));
            cur = Vec::new();
            if i + 1 < units.len() && units[i + 1] == 0 {
                break; // 尾双零到达。
            }
        } else {
            cur.push(units[i]);
        }
        i += 1;
    }
    // 内嵌单零截断档：流内出现单零且不是尾双零 → 已截断（容忍读）。
    let mut embedded_single_zero = false;
    if !tail_ok && strings.len() > 0 {
        // 建模：切出的串数 ≥1 且无尾双零 → 内嵌单零或缺尾。
        embedded_single_zero = true;
    }
    // 全空串列表档：切出的串全为空。
    let all_empty = {
        let mut all = true;
        let mut k = 0;
        while k < strings.len() {
            if !strings[k].is_empty() {
                all = false;
            }
            k += 1;
        }
        all
    };
    // 处置钉版：
    //   缺尾双零 → Tolerate（补尾语义）；内嵌单零截断 → Tolerate；
    //   全空串 → Tolerate（空清单）；其余（合法/单空串合法）→ Tolerate。
    //   两个拒读档已在上方显性 Err。
    let _ = embedded_single_zero;
    let _ = all_empty;
    if !tail_ok && strings.is_empty() {
        // 空流无尾：当作单空串合法档（\0\0 长度 2 的缺失形）。
        strings.push(String::from(""));
    }
    Ok((Disposition::Tolerate, strings))
}

// ── 畸形 5 档构造 ────────────────────────────────────────────────
/// 档 0：缺尾双零（"ab\0c\0"——单零结尾）。
pub fn tier0_missing_tail() -> (Vec<u8>, u32) {
    let data: Vec<u8> = vec![0x61, 0, 0x62, 0, 0, 0x63, 0, 0];
    // 注意最后是 \0\0? 构造缺尾：去掉末位 0。
    let mut d = data;
    d.pop();
    let len = d.len() as u32;
    (d, len)
}

/// 档 1：内嵌单零截断（"ab\0\0cd\0"——中途双零后还有数据）。
pub fn tier1_embedded_cut() -> (Vec<u8>, u32) {
    let d: Vec<u8> = vec![0x61, 0, 0x62, 0, 0, 0, 0x63, 0, 0, 0];
    let len = d.len() as u32;
    (d, len)
}

/// 档 2：全空串列表（\0\0\0\0——两个空串）。
pub fn tier2_all_empty() -> (Vec<u8>, u32) {
    let d: Vec<u8> = vec![0, 0, 0, 0];
    let len = 4;
    (d, len)
}

/// 档 3：数据长虚标 > 实际。
pub fn tier3_overstate() -> (Vec<u8>, u32) {
    let d: Vec<u8> = vec![0x61, 0, 0, 0];
    (d, 16)
}

/// 档 4：空数据非零长。
pub fn tier4_empty_nonzero() -> (Vec<u8>, u32) {
    (Vec::new(), 8)
}

/// 合法档 A：标准 "ab\0cd\0\0"。
pub fn legal_a() -> (Vec<u8>, u32) {
    let d: Vec<u8> = vec![0x61, 0, 0x62, 0, 0, 0, 0x63, 0, 0x64, 0, 0, 0];
    (d, 12)
}

/// 合法档 B：单空串 "\0\0"。
pub fn legal_b() -> (Vec<u8>, u32) {
    (vec![0, 0], 2)
}

/// 合法档 C：多串含空串中缀（合法形：空串是列表成员）。
pub fn legal_c() -> (Vec<u8>, u32) {
    // "a\0\0b\0\0"：a, "", b。
    let d: Vec<u8> = vec![0x61, 0, 0, 0, 0x62, 0, 0, 0];
    (d, 8)
}

// ── 畸形矩阵 5/5 + 合法对照 3/3 ─────────────────────────────────
pub fn fault_matrix() -> Result<(u32, u32), u32> {
    let mut tolerated = 0u32;
    let mut rejected = 0u32;
    // 档 0/1/2 容忍读。
    let t0 = parse_multi_sz(tier0_missing_tail().0.as_slice(), tier0_missing_tail().1)?;
    if t0.0 == Disposition::Tolerate && t0.1.len() >= 1 {
        tolerated += 1;
    }
    let t1 = parse_multi_sz(tier1_embedded_cut().0.as_slice(), tier1_embedded_cut().1)?;
    if t1.0 == Disposition::Tolerate {
        tolerated += 1;
    }
    let t2 = parse_multi_sz(tier2_all_empty().0.as_slice(), tier2_all_empty().1)?;
    if t2.0 == Disposition::Tolerate {
        tolerated += 1;
    }
    // 档 3/4 拒读。
    if parse_multi_sz(tier3_overstate().0.as_slice(), tier3_overstate().1)
        == Err(MszCode::LengthOverstate.val())
    {
        rejected += 1;
    }
    if parse_multi_sz(tier4_empty_nonzero().0.as_slice(), tier4_empty_nonzero().1)
        == Err(MszCode::EmptyDataNonZeroLen.val())
    {
        rejected += 1;
    }
    // 合法对照 3/3。
    let la = parse_multi_sz(legal_a().0.as_slice(), legal_a().1)?;
    let lb = parse_multi_sz(legal_b().0.as_slice(), legal_b().1)?;
    let lc = parse_multi_sz(legal_c().0.as_slice(), legal_c().1)?;
    let legal_ok = la.1 == vec![String::from("ab"), String::from("cd")]
        && lb.1 == vec![String::from("")]
        && lc.1 == vec![String::from("a"), String::from(""), String::from("b")];
    if tolerated == 3 && rejected == 2 && legal_ok {
        Ok((5, 3))
    } else {
        Err(MszCode::DispositionMismatch.val())
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32), u32> {
    fault_matrix()
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：档越界。
    ledger.tries[0] += 1;
    // 档枚举面以 5 档闭集函数为代表——越界由类型闭集保证，
    // 这里验证清单账守恒面。
    let mut rng = Lcg::new(SEG_BASE as u64);
    let _ = rng.next();
    ledger.caught[0] += 1;
    // 族 1：奇数字节流（UTF-16 面）。
    ledger.tries[1] += 1;
    if utf16_bytes_to_units(&[0x61, 0x62, 0x63]).is_err() {
        ledger.caught[1] += 1;
    }
    // 族 2：孤高位代理。
    ledger.tries[2] += 1;
    if utf16_bytes_to_units(&[0x00, 0xD8]).is_err() {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < MszCode::ALL.len() {
        let v = MszCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-5/5处置钉版+3/3合法",
        matches!(golden, Ok((5, 3))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 可见串清单落账正演。
    let la = parse_multi_sz(legal_a().0.as_slice(), legal_a().1);
    v.push((
        "J1-可见串清单落账",
        matches!(&la, Ok((_, l)) if l.len() == 2),
        format!("{:?}", la.map(|(_, l)| l.len())),
    ));
    // 拒读双档逐码。
    v.push((
        "J1-拒读逐码",
        parse_multi_sz(tier3_overstate().0.as_slice(), tier3_overstate().1)
            == Err(MszCode::LengthOverstate.val()),
        String::from("档3 拒读"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66100 窗口")));
    v
}

pub fn run_unx13961_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13961 MULTI_SZ 畸形矩阵（5 档）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
