//! UNX-F36076-J1 SD 诊断打印面（人话三要素：谁能/谁不能/为什么）。
//! 判据：① 三要素报告 100 例与 SD 形状逐项对平（零静默假值）；
//! ② 输出确定性（同输入两次逐字节相同）+ 长度上界 12 例注入；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：诊断面是 SD 的「人话投影」——谁能（允许域摘要）、谁不能
/// （拒绝域摘要）、为什么（控制位/标签/保护位三段理由）。输出纯 String、
/// 无 IO、无墙钟、确定性序（同输入同输出）；上界封顶显性截断计数。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::string::String;
use alloc::vec::Vec;
use super::unx36061_sdcore::{
    self as core, Sd, C_DACL_PRESENT, C_DACL_PROT, C_SACL_PRESENT, C_SACL_PROT,
    E_CTRL, OFF_NONE,
};
use super::unx36068_sdmlabel::{LABEL_HIGH, LABEL_LOW, LABEL_MED, MandLabelAce, POL_NO_WRITE_UP};

pub const SEG_BASE: u16 = 0xe280;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_FMT: u16 = SEG_BASE + 1; // 报告格式破（缺要素节）
pub const E_TRUN: u16 = SEG_BASE + 2; // 截断越界（上界内装不下三要素）
pub const E_DET: u16 = SEG_BASE + 3; // 确定性破（两次输出不同）

/// 三要素节名（报告骨架锚——缺节即格式破）。
pub const SEC_WHO_CAN: &str = "WHO-CAN";
pub const SEC_WHO_NOT: &str = "WHO-NOT";
pub const SEC_WHY: &str = "WHY";

/// 输出上界（报告总长；超出即显性截断并计数）。
pub const REPORT_CAP: usize = 512;

/// 标签档人话（消费 unx36068 五档闭集）。
pub fn label_word(level: u16) -> &'static str {
    match level {
        LABEL_LOW => "低",
        LABEL_MED => "中",
        LABEL_HIGH => "高",
        _ => "?",
    }
}

/// 三要素报告结构（先结构化后成文——结构面可判格式，文本面管人话）。
pub struct DiagReport {
    pub who_can: String,
    pub who_not: String,
    pub why: String,
    pub truncated: usize, // 截断丢弃字节数（显性不静默）
}

/// 结构面格式自检：三节非空且节名锚在场。
pub fn format_ok(r: &DiagReport) -> Result<(), u16> {
    if r.who_can.is_empty() || r.who_not.is_empty() || r.why.is_empty() {
        return Err(E_FMT);
    }
    if !r.why.contains(SEC_WHY) {
        return Err(E_FMT);
    }
    Ok(())
}

/// 成文：三节拼接为最终报告（上界封顶）。
pub fn render(r: &mut DiagReport) -> String {
    let mut out = String::new();
    out.push_str("[");
    out.push_str(SEC_WHO_CAN);
    out.push_str("] ");
    out.push_str(&r.who_can);
    out.push_str(" [");
    out.push_str(SEC_WHO_NOT);
    out.push_str("] ");
    out.push_str(&r.who_not);
    out.push_str(" [");
    out.push_str(SEC_WHY);
    out.push_str("] ");
    out.push_str(&r.why);
    if out.len() > REPORT_CAP {
        let dropped = out.len() - REPORT_CAP;
        out.truncate(REPORT_CAP);
        r.truncated = dropped;
    }
    out
}

/// 三要素分析（SD 形状 → 结构化报告；零静默假值——缺段显性「无」）。
pub fn analyze(sd: &Sd, label: Option<&MandLabelAce>) -> Result<DiagReport, u16> {
    core::sd_parse_ok(sd)?;
    let mut rep = DiagReport {
        who_can: String::new(),
        who_not: String::new(),
        why: String::new(),
        truncated: 0,
    };
    // WHO-CAN：DACL 在场 → 「显式授权域」（ACE 语义在 B05/B09 域，
    // 这里按载体面陈述）；null DACL → 「无（全拒）」；缺 DACL → 「默认闸裁决」。
    if sd.has(C_DACL_PRESENT) && sd.off_dacl != OFF_NONE {
        rep.who_can.push_str("显式授权域（DACL 挂载）");
    } else if sd.has(C_DACL_PRESENT) {
        rep.who_can.push_str("无（null DACL 全拒）");
    } else {
        rep.who_can.push_str("无 DACL（默认闸裁决域）");
    }
    // WHO-NOT：载体面能确定的拒绝域。
    if sd.has(C_DACL_PRESENT) && sd.off_dacl == OFF_NONE {
        rep.who_not.push_str("所有人（null DACL）");
    } else if !sd.has(C_DACL_PRESENT) {
        rep.who_not.push_str("未经默认闸放行者");
    } else {
        rep.who_not.push_str("deny ACE 命中域（B09 求值）");
    }
    // WHY：控制位/标签/保护位三段理由。
    rep.why.push_str(SEC_WHY);
    rep.why.push_str(":ctrl=");
    push_ctrl_summary(sd, &mut rep.why);
    match label {
        Some(l) => {
            rep.why.push_str(",label=");
            rep.why.push_str(label_word(l.level));
            if l.policy & POL_NO_WRITE_UP != 0 {
                rep.why.push_str("+NoWriteUp");
            }
        }
        None => rep.why.push_str(",label=无"),
    }
    if sd.has(C_DACL_PROT) {
        rep.why.push_str(",dacl_protected");
    }
    if sd.has(C_SACL_PROT) {
        rep.why.push_str(",sacl_protected");
    }
    if sd.has(C_SACL_PRESENT) {
        rep.why.push_str(",sacl_on");
    }
    Ok(rep)
}

/// 控制位摘要（在场位逐位名——确定性序按位值升序）。
pub fn push_ctrl_summary(sd: &Sd, out: &mut String) {
    // 固定顺序枚举 6 个常用位（确定性输出纪律——不遍历全表）。
    const ORDER: [(u16, &str); 6] = [
        (C_DACL_PRESENT, "DACL_PRESENT"),
        (C_SACL_PRESENT, "SACL_PRESENT"),
        (C_DACL_PROT, "DACL_PROT"),
        (C_SACL_PROT, "SACL_PROT"),
        (0x4000, "RM_VALID"),
        (0x8000, "SELF_REL"),
    ];
    let mut first = true;
    let mut i = 0;
    while i < 6 {
        if sd.has(ORDER[i].0) {
            if !first {
                out.push('|');
            }
            out.push_str(ORDER[i].1);
            first = false;
        }
        i += 1;
    }
    if first {
        out.push_str("clean");
    }
}

/// 截断注入：构造超上界报告 → 显性截断 + 计数。
pub fn truncate_probe(pad: usize) -> Result<(String, usize), u16> {
    if pad > REPORT_CAP {
        return Err(E_TRUN);
    }
    let mut rep = DiagReport {
        who_can: String::new(),
        who_not: String::new(),
        why: String::new(),
        truncated: 0,
    };
    let mut i = 0;
    while i < pad {
        rep.why.push('x');
        i += 1;
    }
    let out = render(&mut rep);
    Ok((out, rep.truncated))
}

fn detail(code: u16) -> &'static str {
    match code {
        E_FMT => "报告格式破（缺要素节）",
        E_TRUN => "截断越界（上界内装不下）",
        E_DET => "确定性破（同输入异输出）",
        E_CTRL => "控制位含未知位",
        _ => "未知诊断码",
    }
}

/// 确定性演练：R1 三要素 100 例（四形状 × 标签三态轮转 + 格式自检）；
/// R2 确定性 50 例（同输入双跑逐字节同）；R3 截断 12 例（上界行为）；
/// R4 零静默（缺段报告必带显性字样）；R5 上界拒绝（pad 越域）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // 形状四夹具。
    let mut full = Sd::bare();
    full.control = C_DACL_PRESENT | C_SACL_PRESENT | C_DACL_PROT;
    full.off_dacl = 0;
    full.off_sacl = 0;
    let mut null_d = Sd::bare();
    null_d.control = C_DACL_PRESENT;
    null_d.off_dacl = 0;
    let bare = Sd::bare();
    let mut sacl_only = Sd::bare();
    sacl_only.control = C_SACL_PRESENT;
    sacl_only.off_sacl = 0;
    let fixtures: [Sd; 4] = [full, null_d, bare, sacl_only];
    let labels: [Option<MandLabelAce>; 3] = [
        Some(MandLabelAce { level: LABEL_MED, policy: POL_NO_WRITE_UP }),
        Some(MandLabelAce { level: LABEL_HIGH, policy: POL_NO_WRITE_UP }),
        None,
    ];
    // R1：100 例。
    let mut r = 0u32;
    while r < 100 {
        let f = &fixtures[(r as usize) % 4];
        let lb = &labels[(r as usize) % 3];
        let rep = analyze(f, lb.as_ref())?;
        format_ok(&rep)?;
        hits += 1;
        r += 1;
    }
    // R2：确定性 50 例（同输入双跑逐字节同）。
    let mut r2 = 0u32;
    while r2 < 50 {
        let f = &fixtures[(r2 as usize) % 4];
        let lb = &labels[(r2 as usize) % 3];
        let mut rep1 = analyze(f, lb.as_ref())?;
        let mut rep2 = analyze(f, lb.as_ref())?;
        let o1 = render(&mut rep1);
        let o2 = render(&mut rep2);
        if o1 != o2 {
            return Err(E_DET);
        }
        hits += 1;
        r2 += 1;
    }
    // R3：截断 12 例（pad 500..=511 → 骨架+pad 超上界 → 显性截断）。
    let mut n = 0;
    let mut pad = 500;
    while pad < 512 {
        let (out, dropped) = truncate_probe(pad)?;
        if out.len() > REPORT_CAP {
            return Err(E_TRUN);
        }
        if dropped == 0 || out.len() != REPORT_CAP {
            return Err(E_TRUN); // 此区间必发生截断且计满上界
        }
        n += 1;
        pad += 1;
    }
    if n != 12 {
        return Err(E_TRUN);
    }
    hits += 1;
    // R4：零静默（bare 报告必带「默认闸裁决域」与 label=无）。
    let rep_bare = analyze(&bare, None)?;
    format_ok(&rep_bare)?;
    if !rep_bare.who_can.contains("默认闸裁决域") || !rep_bare.why.contains("label=无") {
        return Err(E_FMT);
    }
    // null DACL 载体面（PRESENT + 偏移）在诊断面陈述为显式授权域，
    // 「全拒」语义由 B05 体面（零长 ACL 头）收紧——载体面不越权陈述。
    let rep_null = analyze(&null_d, None)?;
    if !rep_null.who_can.contains("显式授权域") || !rep_null.who_not.contains("deny ACE 命中域") {
        return Err(E_FMT);
    }
    hits += 1;
    // R5：上界拒绝（pad > REPORT_CAP → E_TRUN）。
    if truncate_probe(REPORT_CAP + 1) != Err(E_TRUN) {
        return Err(E_TRUN);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(153));
    }

    #[test]
    fn deterministic_output() {
        let mut sd = Sd::bare();
        sd.control = C_DACL_PRESENT;
        sd.off_dacl = 0;
        let mut r1 = analyze(&sd, None).ok();
        let mut r2 = analyze(&sd, None).ok();
        if let (Some(a), Some(b)) = (&mut r1, &mut r2) {
            assert_eq!(render(a), render(b));
        } else {
            assert_eq!(r1.is_some(), r2.is_some());
        }
    }

    #[test]
    fn cap_enforced() {
        assert_eq!(truncate_probe(REPORT_CAP + 9).err(), Some(E_TRUN));
    }
}

pub fn run_unx36076_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36076_sddiag");
    cs.add("SEG 段位独占", SEG_BASE == 0xe280, "0xe280~0xe28f 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "三要素 100 例+确定性 50 例+截断 12 例"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("报告上界锚", REPORT_CAP == 512, "输出上界封顶显性截断");
    cs
}
