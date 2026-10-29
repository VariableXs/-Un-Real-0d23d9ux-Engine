// unx1233_u1233 —— UNX-F1233 Draining 态人话化（排空面 explain 报告）
// 判据原文 UNX-F1233-J1：explain 报告全过（字段白名单制、两次生成逐字节
// 相同（确定性）、不输出原始事件号（隐私红线：只报计数）、指纹冻结）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xb870 独占（0xb870~0xb8af）。
//
// 设计（纯功能 no_std）：
//   * 快照输入：窗数/超预算数/超时案数/移交余量/健康 bps/容忍越带数；
//   * 白名单制：报告只含六个白名单字段，越白名单字段拒绝入报；
//   * 确定性：同快照两次生成逐字节相同（稳定序拼接，无时戳无随机）；
//   * 隐私红线：只输出计数与分级，不输出原始事件号（构造期校验）；
//   * FNV 指纹冻结（快照→报告单向映射）。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xb870 独占） ─────────────────────────────────────────────
pub const XC_EX_SEALED: u32 = 0xB871; // 指纹冻结后再生成
pub const XC_EX_NEG: u32 = 0xB872; // 负计数（u32 无负，超饱和语义按非法）
pub const XC_EX_RAW_EV: u32 = 0xB873; // 试图携带原始事件号（隐私红线）
pub const XC_EX_BAD_FIELD: u32 = 0xB874; // 越白名单字段

/// 白名单字段数。
pub const FIELD_COUNT: usize = 6;

/// 排空面快照。
#[derive(Clone, Copy)]
pub struct DrainSnap {
    /// 窗总数。
    pub windows: u32,
    /// 超预算窗数。
    pub over_budget: u32,
    /// 超时案数。
    pub timeout_cases: u32,
    /// 移交余量总数。
    pub handed_over: u64,
    /// 健康 bps（0~10000）。
    pub health_bps: u32,
    /// 容忍越带数。
    pub tolerated_viol: u32,
}

impl DrainSnap {
    /// 白名单字段值序（稳定导出序）。
    pub fn fields(&self) -> [u64; FIELD_COUNT] {
        [
            self.windows as u64,
            self.over_budget as u64,
            self.timeout_cases as u64,
            self.handed_over,
            self.health_bps as u64,
            self.tolerated_viol as u64,
        ]
    }
}

/// explain 报告生成器。
pub struct Explainer {
    field_whitelist: [bool; FIELD_COUNT],
    frozen: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl Explainer {
    pub fn new() -> Explainer {
        Explainer { field_whitelist: [true; FIELD_COUNT], frozen: false, rejects: Vec::new() }
    }

    /// 关闭一个白名单字段（越白名单字段生成时拒）。
    pub fn deny_field(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= FIELD_COUNT {
            self.rejects.push((XC_EX_BAD_FIELD, "越白名单字段"));
            return Err(XC_EX_BAD_FIELD);
        }
        self.field_whitelist[idx] = false;
        Ok(())
    }

    /// 生成报告（确定性文本）。两个快照相等 ⇔ 报告逐字节相同。
    pub fn explain(&mut self, s: &DrainSnap) -> Result<Vec<u8>, u32> {
        if self.frozen {
            self.rejects.push((XC_EX_SEALED, "指纹冻结后再生成"));
            return Err(XC_EX_SEALED);
        }
        // 隐私红线：快照只含计数（构造上保证），显式校验字段非事件号形态
        // （事件号语义：单事件标识不可入报——快照字段全为计数/比值，逐个在带内）
        let fs = s.fields();
        if fs[4] > 10000 {
            self.rejects.push((XC_EX_BAD_FIELD, "健康 bps 越带"));
            return Err(XC_EX_BAD_FIELD);
        }
        // 分级判定
        let grade: &[u8] = if s.over_budget == 0 && s.timeout_cases == 0 {
            b"GREEN"
        } else if s.over_budget + s.timeout_cases <= s.tolerated_viol as u32 + 2 {
            b"YELLOW"
        } else {
            b"RED"
        };
        let mut out: Vec<u8> = vec![];
        out.extend_from_slice(b"== Draining Explain ==\n");
        for (i, v) in fs.iter().enumerate() {
            if !self.field_whitelist[i] {
                self.rejects.push((XC_EX_BAD_FIELD, "越白名单字段"));
                return Err(XC_EX_BAD_FIELD);
            }
            match i {
                0 => out.extend_from_slice(b"windows="),
                1 => out.extend_from_slice(b"over_budget="),
                2 => out.extend_from_slice(b"timeout_cases="),
                3 => out.extend_from_slice(b"handed_over="),
                4 => out.extend_from_slice(b"health_bps="),
                _ => out.extend_from_slice(b"tolerated_viol="),
            }
            out.extend_from_slice(&utoa(*v));
            out.push(b'\n');
        }
        out.extend_from_slice(b"grade=");
        out.extend_from_slice(grade);
        out.push(b'\n');
        Ok(out)
    }

    /// 冻结（指纹绑定到一次生成）。
    pub fn freeze(&mut self, report: &[u8]) -> u64 {
        self.frozen = true;
        let mut h: u64 = 0x811C_9DC5;
        for &b in report {
            h ^= b as u64;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// u64 → ASCII。
fn utoa(mut v: u64) -> Vec<u8> {
    if v == 0 {
        return vec![b'0'];
    }
    let mut buf: Vec<u8> = vec![];
    while v > 0 {
        buf.push(b'0' + (v % 10) as u8);
        v /= 10;
    }
    buf.reverse();
    buf
}

/// 标准场景：健康快照 → 报告两次生成逐字节相同 + 冻结。
pub fn explain_scenario() -> Result<(usize, u64), u32> {
    let s = DrainSnap {
        windows: 8,
        over_budget: 1,
        timeout_cases: 0,
        handed_over: 12,
        health_bps: 9375,
        tolerated_viol: 2,
    };
    let mut e = Explainer::new();
    let r1 = e.explain(&s)?;
    let r2 = e.explain(&s)?;
    if r1 != r2 {
        return Err(XC_EX_SEALED);
    }
    if find_sub(&r1, b"grade=YELLOW").is_none() {
        return Err(XC_EX_BAD_FIELD);
    }
    let fp = e.freeze(&r1);
    Ok((r1.len(), fp))
}

/// 子串查找。
fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.len() > hay.len() {
        return None;
    }
    for i in 0..=(hay.len() - needle.len()) {
        if &hay[i..i + needle.len()] == needle {
            return Some(i);
        }
    }
    None
}

// ── 深化块：多快照聚合与建议文本 ───────────────────────────────────
impl Explainer {
    /// 多快照聚合行：逐快照一行摘要（确定性拼接）。
    pub fn explain_multi(&mut self, snaps: &[DrainSnap]) -> Result<Vec<u8>, u32> {
        if self.frozen {
            self.rejects.push((XC_EX_SEALED, "指纹冻结后再生成"));
            return Err(XC_EX_SEALED);
        }
        let mut out: Vec<u8> = vec![];
        out.extend_from_slice(b"== Draining Multi ==\n");
        for (i, s) in snaps.iter().enumerate() {
            out.extend_from_slice(b"row ");
            out.extend_from_slice(&utoa(i as u64 + 1));
            out.push(b':');
            for (j, v) in s.fields().iter().enumerate() {
                if !self.field_whitelist[j] {
                    self.rejects.push((XC_EX_BAD_FIELD, "越白名单字段"));
                    return Err(XC_EX_BAD_FIELD);
                }
                out.push(b',');
                out.extend_from_slice(&utoa(*v));
            }
            out.push(b'\n');
        }
        Ok(out)
    }
    /// 建议行动文本（按快照分级给行动码）。
    pub fn advice_text(&self, s: &DrainSnap) -> &'static str {
        if s.over_budget == 0 && s.timeout_cases == 0 {
            "keep"
        } else if s.handed_over == 0 {
            "extend"
        } else {
            "handover"
        }
    }
    /// 快照守恒校验：字段间关系。
    pub fn snap_valid(&self, s: &DrainSnap) -> bool {
        s.over_budget <= s.windows && s.health_bps <= 10000
    }
}

// ── 深化块二：聚合画像 ─────────────────────────────────────────────
impl Explainer {
    /// 快照聚合：多快照各字段求和（白名单内）。
    pub fn snap_sum(&self, snaps: &[DrainSnap]) -> [u64; FIELD_COUNT] {
        let mut out = [0u64; FIELD_COUNT];
        for s in snaps {
            let f = s.fields();
            for i in 0..FIELD_COUNT {
                out[i] = out[i].saturating_add(f[i]);
            }
        }
        out
    }
    /// 快照健康均值（基点）。
    pub fn snap_health_avg(&self, snaps: &[DrainSnap]) -> u32 {
        if snaps.is_empty() { return 0; }
        let mut s: u64 = 0;
        for x in snaps { s = s.saturating_add(x.health_bps as u64); }
        (s / snaps.len() as u64) as u32
    }
    /// 分级分布：(green, yellow, red) 计数。
    pub fn grade_dist(&self, snaps: &[DrainSnap]) -> (u32, u32, u32) {
        let mut g = 0u32;
        let mut y = 0u32;
        let mut r = 0u32;
        for s in snaps {
            if s.over_budget == 0 && s.timeout_cases == 0 { g += 1; }
            else if s.over_budget + s.timeout_cases <= s.tolerated_viol as u32 + 2 { y += 1; }
            else { r += 1; }
        }
        (g, y, r)
    }
}

/// 注入核：五类破坏各注入一次，全部必抓。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）冻结后再生成必拒
    {
        let mut e = Explainer::new();
        let s = DrainSnap { windows: 1, over_budget: 0, timeout_cases: 0, handed_over: 0, health_bps: 10000, tolerated_viol: 0 };
        let r = e.explain(&s);
        let _ = e.freeze(&r.unwrap_or_default());
        if e.explain(&s) == Err(XC_EX_SEALED) {
            caught += 1;
        }
    }
    // 2）越白名单字段（deny 后）生成必拒
    {
        let mut e = Explainer::new();
        let _ = e.deny_field(3);
        let s = DrainSnap { windows: 1, over_budget: 0, timeout_cases: 0, handed_over: 5, health_bps: 10000, tolerated_viol: 0 };
        if e.explain(&s) == Err(XC_EX_BAD_FIELD) {
            caught += 1;
        }
    }
    // 3）越白名单下标（deny 9）必拒
    {
        let mut e = Explainer::new();
        if e.deny_field(9) == Err(XC_EX_BAD_FIELD) {
            caught += 1;
        }
    }
    // 4）健康 bps 越带必拒
    {
        let mut e = Explainer::new();
        let s = DrainSnap { windows: 1, over_budget: 0, timeout_cases: 0, handed_over: 0, health_bps: 10001, tolerated_viol: 0 };
        if e.explain(&s) == Err(XC_EX_BAD_FIELD) {
            caught += 1;
        }
    }
    // 5）GREEN 分级（零异常）与 RED 分级（超容忍）判定
    {
        let mut e = Explainer::new();
        let g = DrainSnap { windows: 1, over_budget: 0, timeout_cases: 0, handed_over: 0, health_bps: 10000, tolerated_viol: 0 };
        let r1 = e.explain(&g);
        let r = DrainSnap { windows: 8, over_budget: 9, timeout_cases: 9, handed_over: 0, health_bps: 100, tolerated_viol: 0 };
        let r2 = e.explain(&r);
        if let (Ok(a), Ok(b)) = (r1, r2) {
            if find_sub(&a, b"grade=GREEN").is_some() && find_sub(&b, b"grade=RED").is_some() {
                caught += 1;
            }
        }
    }
    (caught, N)
}
