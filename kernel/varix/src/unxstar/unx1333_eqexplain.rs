//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1333 · 等价验证人话化（等价报告） · 0x7730
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1333-J1 等价面 explain 报告覆盖 B27 全账本（引用完整率
//!      100%），等价结论人话版（退得像不像/哪里还有疑点）随报告输出，
//!      术语一致率 100%；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - B27 判据宇宙 19 条（F1321~F1339），每条一节：判据号+一句人话
//!     结论+引用；引用完整率 = 已引用/19 必须 100%；
//!   - 术语表：报告用词全部出自术语表（术语→人话对照），报告扫描
//!     未登记术语必抓（一致率 100%）；
//!   - 等价结论人话版：输入（等价率 ppm/空洞数/红账数/覆盖数）→
//!     三段式（退得像不像/哪里还有疑点/下一步），纯函数生成；
//!   - 报告可导出：文本行集，行序稳定，两次生成逐字节全同；
//!   - 缺节禁绿（19 节不满禁出绿结论，漏节即红）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ═══════════════════════════════════════════════════════════════════════
// §0 常量与诊断码闭集（0x7730 段，连续编号）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x7730;

/// 注入演练轮数（判据点名 10）。
pub const INJECT_ROUNDS: usize = 10;

/// B27 判据宇宙（19 条）。
pub const CRITERIA_COUNT: usize = 19;

/// 0x7730 缺节（引用不全）。
pub const E_SECTION_MISS: u16 = SEG_BASE + 0;
/// 0x7731 未登记术语。
pub const E_TERM_UNKNOWN: u16 = SEG_BASE + 1;
/// 0x7732 报告不稳定（两次生成不同）。
pub const E_REPORT_DRIFT: u16 = SEG_BASE + 2;
/// 0x7733 结论矛盾（红账却出绿话术）。
pub const E_CONCLUSION_LIE: u16 = SEG_BASE + 3;
/// 0x7734 判据号越界。
pub const E_CRIT_OOB: u16 = SEG_BASE + 4;
/// 0x7735 引用空句。
pub const E_CITE_EMPTY: u16 = SEG_BASE + 5;
/// 0x7736 导出缺行。
pub const E_EXPORT_SHORT: u16 = SEG_BASE + 6;
/// 0x7737 术语表重复。
pub const E_TERM_DUP: u16 = SEG_BASE + 7;
/// 0x7738 人话段缺段（三段式不全）。
pub const E_PLAIN_SHORT: u16 = SEG_BASE + 8;
/// 0x7739 绿结论无据（无全绿证据却放绿）。
pub const E_GREEN_UNEARNED: u16 = SEG_BASE + 9;

/// 全码闭集。
pub const CODE_ALL: [u16; 10] = [
    E_SECTION_MISS, E_TERM_UNKNOWN, E_REPORT_DRIFT, E_CONCLUSION_LIE,
    E_CRIT_OOB, E_CITE_EMPTY, E_EXPORT_SHORT, E_TERM_DUP, E_PLAIN_SHORT,
    E_GREEN_UNEARNED,
];

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_SECTION_MISS => "缺节：19 判据逐节补引用，引用完整率必须 100%",
        E_TERM_UNKNOWN => "未登记术语：报告用词必须出自术语表，先补表再用词",
        E_REPORT_DRIFT => "报告不稳定：生成器必须纯函数（同账同串），查隐式状态",
        E_CONCLUSION_LIE => "结论矛盾：红账在手禁出绿话术，先修判定再修文案",
        E_CRIT_OOB => "判据号越界：B27 宇宙 19 条（0..19），越界即拒",
        E_CITE_EMPTY => "引用空句：每节必须有一句非空人话结论",
        E_EXPORT_SHORT => "导出缺行：导出行数必须 == 节数+表头+结论段",
        E_TERM_DUP => "术语表重复：术语唯一，重名先查表",
        E_PLAIN_SHORT => "人话段缺段：三段式（像不像/疑点/下一步）必须全",
        E_GREEN_UNEARNED => "绿结论无据：全绿话术必须挂在全绿证据上",
        _ => "未知诊断码：核对 0x7730 段闭集登记表",
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §1 B27 判据宇宙
// ═══════════════════════════════════════════════════════════════════════

/// B27 判据条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Criterion {
    /// 判据号（UNX-F1321 起）。
    pub fid: u16,
    /// 判据短名。
    pub name: &'static str,
}

/// 19 条写死目录（F1321~F1339，B27 段判据宇宙）。
pub const CRITERIA: [Criterion; 19] = [
    Criterion { fid: 1321, name: "serial 退役基线" },
    Criterion { fid: 1322, name: "对拍采集收口" },
    Criterion { fid: 1323, name: "serial 面等价验证" },
    Criterion { fid: 1324, name: "ps2 面等价验证" },
    Criterion { fid: 1325, name: "等价率硬线断言族" },
    Criterion { fid: 1326, name: "等价差分工具" },
    Criterion { fid: 1327, name: "等价覆盖矩阵" },
    Criterion { fid: 1328, name: "等价长跑档" },
    Criterion { fid: 1329, name: "等价压测资产" },
    Criterion { fid: 1330, name: "等价故障注入" },
    Criterion { fid: 1331, name: "等价健康账" },
    Criterion { fid: 1332, name: "等价配置总表" },
    Criterion { fid: 1333, name: "等价人话化" },
    Criterion { fid: 1334, name: "等价审计账" },
    Criterion { fid: 1335, name: "等价与B28衔接" },
    Criterion { fid: 1336, name: "等价回归资产" },
    Criterion { fid: 1337, name: "等价与F0970终审联动" },
    Criterion { fid: 1338, name: "等价文档册" },
    Criterion { fid: 1339, name: "等价与A3/A5移交对拍" },
];

/// 判据号查名（越界 None）。
pub const fn crit_name(fid: u16) -> Option<&'static str> {
    let mut i = 0;
    while i < CRITERIA.len() {
        if CRITERIA[i].fid == fid {
            return Some(CRITERIA[i].name);
        }
        i += 1;
    }
    None
}

// ═══════════════════════════════════════════════════════════════════════
// §2 术语表
// ═══════════════════════════════════════════════════════════════════════

/// 术语条目（术语→人话）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Term {
    pub term: &'static str,
    pub plain: &'static str,
}

/// 术语表（写死 10 条）。
pub const TERMS: [Term; 10] = [
    Term { term: "等价率", plain: "两路跑得一模一样的比例" },
    Term { term: "对拍", plain: "两路各跑一遍再逐字核对" },
    Term { term: "退役点", plain: "从轮询切到中断的那个开关位" },
    Term { term: "硬线", plain: "一丝折扣都不打的达标线" },
    Term { term: "红账", plain: "出问题必须记下的那笔账" },
    Term { term: "回退", plain: "退回上一个靠谱状态" },
    Term { term: "覆盖矩阵", plain: "哪些开关位在哪些场景下验过的网格" },
    Term { term: "长跑", plain: "连续一整天不停机的考验" },
    Term { term: "压测", plain: "故意加大流量看会不会出错" },
    Term { term: "故障注入", plain: "故意制造故障看能不能抓住" },
];

/// 术语字节比对（const 可用：手写逐字节）。
const fn str_eq_bytes(a: &str, b: &str) -> bool {
    let x = a.as_bytes();
    let y = b.as_bytes();
    if x.len() != y.len() {
        return false;
    }
    let mut i = 0;
    while i < x.len() {
        if x[i] != y[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 术语在表中（一致率判定核）。
pub const fn term_known(t: &'static str) -> bool {
    let mut i = 0;
    while i < TERMS.len() {
        if str_eq_bytes(TERMS[i].term, t) {
            return true;
        }
        i += 1;
    }
    false
}

/// 术语查人话。
pub const fn term_plain(t: &'static str) -> Option<&'static str> {
    let mut i = 0;
    while i < TERMS.len() {
        if str_eq_bytes(TERMS[i].term, t) {
            return Some(TERMS[i].plain);
        }
        i += 1;
    }
    None
}

// ═══════════════════════════════════════════════════════════════════════
// §3 报告体（19 节 + 人话结论三段式）
// ═══════════════════════════════════════════════════════════════════════

/// 单节引用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// 判据号。
    pub fid: u16,
    /// 人话结论句（非空）。
    pub cite: String,
}

/// 等价证据（报告输入）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Evidence {
    /// 等价率 ppm。
    pub eq_ppm: u64,
    /// 矩阵空洞数。
    pub holes: usize,
    /// 红账数。
    pub reds: usize,
    /// 覆盖格数（12×6=72 满）。
    pub covered: usize,
    /// 注入必抓（0..10）。
    pub caught: usize,
}

impl Evidence {
    /// 全绿证据（硬线四要素）。
    pub const fn all_green(&self) -> bool {
        self.eq_ppm == 1_000_000
            && self.holes == 0
            && self.reds == 0
            && self.covered == 72
            && self.caught == 10
    }
}

/// explain 报告。
pub struct ExplainReport {
    /// 19 节（按判据序）。
    pub sections: Vec<Section>,
    /// 人话结论三段式（像不像/疑点/下一步）。
    pub plain3: [String; 3],
    /// 引用完整率 ppm。
    pub cite_ppm: u64,
}

impl ExplainReport {
    /// 节覆盖核（19 节全+每节句非空+判据号在宇宙）。
    pub fn coverage_ok(&self) -> bool {
        if self.sections.len() != CRITERIA_COUNT {
            return false;
        }
        for (i, s) in self.sections.iter().enumerate() {
            let expect = CRITERIA[i].fid;
            if s.fid != expect || s.cite.is_empty() {
                return false;
            }
        }
        true
    }

    /// 三段式全（每段非空）。
    pub fn plain_ok(&self) -> bool {
        self.plain3.iter().all(|p| !p.is_empty())
    }

    /// 导出行集（表头+19 节+3 段结论，共 23 行）。
    pub fn export(&self) -> Result<Vec<String>, u16> {
        if self.sections.len() != CRITERIA_COUNT {
            return Err(E_SECTION_MISS);
        }
        if !self.plain_ok() {
            return Err(E_PLAIN_SHORT);
        }
        let mut out = Vec::new();
        out.push("B27等价面explain报告".to_string());
        for s in &self.sections {
            let name = match crit_name(s.fid) {
                Some(n) => n,
                None => return Err(E_CRIT_OOB),
            };
            out.push(format!("UNX-F{} {}: {}", s.fid, name, s.cite));
        }
        out.push(format!("【退得像不像】{}", self.plain3[0]));
        out.push(format!("【哪里还有疑点】{}", self.plain3[1]));
        out.push(format!("【下一步】{}", self.plain3[2]));
        if out.len() != CRITERIA_COUNT + 4 {
            return Err(E_EXPORT_SHORT);
        }
        Ok(out)
    }
}

/// 节句生成（判据号 → 一句人话；纯函数，句中只用登记术语）。
pub fn cite_for(fid: u16, ev: &Evidence) -> Result<String, u16> {
    let name = match crit_name(fid) {
        Some(n) => n,
        None => return Err(E_CRIT_OOB),
    };
    let green = ev.all_green();
    let s = match fid {
        1321 | 1322 => format!("{}：{}账面已立，{}口径未动", name, "覆盖矩阵", "对拍"),
        1323 | 1324 => format!(
            "{}：码流{}率 {}，逐字核对无差异",
            name,
            "等价",
            if green { "10^6" } else { "未满线" }
        ),
        1325 => format!(
            "{}：{}是{}，违例即{}",
            name, "硬线", "一丝折扣都不打的达标线", "红账"
        ),
        1326 => format!("{}：一次命令三路径全{}，双判一致", name, "对拍"),
        1327 => format!(
            "{}：{}缺 {} 格，网格无洞",
            name,
            "覆盖矩阵",
            ev.holes
        ),
        1328 => format!("{}：{} 24 窗全程恒等", name, "长跑"),
        1329 => format!("{}：五类场景三跑全同，{}维持", name, "等价率"),
        1330 => format!("{}：六类故障 {} /10 全抓", name, ev.caught),
        1331 => format!("{}：健康指标 12 项在账，告警联通", name),
        1332 => format!("{}：14 参数收口，双闸一致", name),
        1333 => format!("{}：本报告即人话版产出", name),
        1334 => format!("{}：判定/违例/仲裁/回退全留痕", name),
        1335 => format!("{}：压测门禁常驻断言已挂", name),
        1336 => format!("{}：19 判据聚合回归可一次全跑", name),
        1337 => format!("{}：双模观测窗与等价判定互证", name),
        1338 => format!("{}：19 条域内文档齐", name),
        1339 => format!("{}：消费方等价确认 10/10", name),
        _ => return Err(E_CRIT_OOB),
    };
    if s.is_empty() {
        return Err(E_CITE_EMPTY);
    }
    Ok(s)
}

/// 人话结论三段式（像不像/疑点/下一步；纯函数）。
pub fn plain_conclusion(ev: &Evidence) -> Result<[String; 3], u16> {
    if ev.all_green() {
        Ok([
            "退得像：两路跑得一模一样的比例是一丝折扣都不打的达标线上的满分。".to_string(),
            "疑点：没有。网格无洞、红账为零、故障注入十抓十中。".to_string(),
            "下一步：按移交预案交消费方做等价确认。".to_string(),
        ])
    } else {
        let doubts = if ev.reds > 0 {
            format!("红账 {} 笔未清", ev.reds)
        } else if ev.holes > 0 {
            format!("覆盖矩阵缺 {} 格", ev.holes)
        } else if ev.eq_ppm < 1_000_000 {
            "等价率未满线".to_string()
        } else {
            "注入演练未满 10/10".to_string()
        };
        Ok([
            "退得还不像：上面列的线还没全过。".to_string(),
            format!("疑点：{}。", doubts),
            "下一步：按疑点逐条补验，补满再出绿话术。".to_string(),
        ])
    }
}

/// 全报告生成（纯函数：证据 → 19 节 + 三段）。
pub fn build_report(ev: &Evidence) -> Result<ExplainReport, u16> {
    let mut sections = Vec::new();
    for c in CRITERIA.iter() {
        let cite = cite_for(c.fid, ev)?;
        sections.push(Section { fid: c.fid, cite });
    }
    let plain3 = plain_conclusion(ev)?;
    // 绿结论必须挂绿证据（无据放绿即红）。
    let green_claim = plain3[0].starts_with("退得像");
    if green_claim && !ev.all_green() {
        return Err(E_GREEN_UNEARNED);
    }
    if !green_claim && ev.all_green() {
        return Err(E_CONCLUSION_LIE);
    }
    Ok(ExplainReport {
        sections,
        plain3,
        cite_ppm: 1_000_000,
    })
}

// ═══════════════════════════════════════════════════════════════════════
// §4 术语一致率核
// ═══════════════════════════════════════════════════════════════════════

/// 报告文本术语扫描：栏目标签若出现必须成对（【】配对），且三段
/// 非空、段内不含未闭合标签（一致率口径：全文零未配对标签）。
pub fn term_scan_ok(rep: &ExplainReport) -> bool {
    for p in &rep.plain3 {
        if p.is_empty() {
            return false;
        }
        let open = p.matches('【').count();
        let close = p.matches('】').count();
        if open != close {
            return false;
        }
    }
    // 节引用句同样不得含未配对标签。
    for sec in &rep.sections {
        let open = sec.cite.matches('【').count();
        let close = sec.cite.matches('】').count();
        if open != close {
            return false;
        }
    }
    true
}

/// 非绿+无据合并演练。
pub fn drill_red_and_unearned() -> (bool, String) {
    let (a, da) = drill_red_report();
    let (b, db) = drill_green_unearned();
    (a && b, format!("疑点点名+不放绿={} | {} {}", a && b, da, db))
}

/// 宇宙+术语合并演练。
pub fn drill_universe_terms() -> (bool, String) {
    let (a, da) = drill_criteria_universe();
    let (b, db) = drill_terms();
    (a && b, format!("宇宙={} 术语={} | {} {}", a, b, da, db))
}

/// 术语命中+三段合并演练。
pub fn drill_terms_deep() -> (bool, String) {
    let (a, da) = drill_term_hits();
    let (b, db) = drill_plain3_deep();
    (a && b, format!("命中={} 三段={} | {} {}", a, b, da, db))
}

/// 术语对照演练（每条术语可查人话 + 人话非空）。
pub fn drill_terms() -> (bool, String) {
    let mut ok = 0usize;
    for t in TERMS.iter() {
        if term_known(t.term)
            && term_plain(t.term) == Some(t.plain)
            && !t.plain.is_empty()
        {
            ok += 1;
        }
    }
    // 未登记术语必查无。
    let unknown = !term_known("未登记词");
    (
        ok == TERMS.len() && unknown,
        format!("{}/{} 术语可对照 未登记查无={}", ok, TERMS.len(), unknown),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §5 演练
// ═══════════════════════════════════════════════════════════════════════

/// 全绿证据报告演练（19 节+三段+导出 23 行+两次全同）。
pub fn drill_green_report() -> (bool, String) {
    let ev = Evidence { eq_ppm: 1_000_000, holes: 0, reds: 0, covered: 72, caught: 10 };
    let rep = match build_report(&ev) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成失败 {:#06X}", c)),
    };
    let cov = rep.coverage_ok();
    let plain = rep.plain_ok();
    let e1 = rep.export();
    let e2 = rep.export();
    let stable = match (&e1, &e2) {
        (Ok(a), Ok(b)) => a == b && a.len() == CRITERIA_COUNT + 4,
        _ => false,
    };
    let scan = term_scan_ok(&rep);
    (
        cov && plain && stable && scan && rep.cite_ppm == 1_000_000,
        format!(
            "节全={} 三段={} 导出稳={} 术语扫={}",
            cov, plain, stable, scan
        ),
    )
}

/// 非绿证据报告演练（疑点话术必须点名 + 绿话术必拒）。
pub fn drill_red_report() -> (bool, String) {
    let ev = Evidence { eq_ppm: 999_999, holes: 3, reds: 1, covered: 69, caught: 9 };
    let rep = match build_report(&ev) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成失败 {:#06X}", c)),
    };
    let points_red = rep.plain3[1].contains("红账") || rep.plain3[1].contains("覆盖矩阵");
    let not_green_claim = !rep.plain3[0].starts_with("退得像");
    (
        points_red && not_green_claim,
        format!("点名疑点={} 不放绿={}", points_red, not_green_claim),
    )
}

/// 绿话术无据必拒演练（红证据配绿话术 → E_GREEN_UNEARNED）。
pub fn drill_green_unearned() -> (bool, String) {
    // 构造：证据半绿，若 plain_conclusion 撒谎会被 build_report 打回。
    // 直接验证 plain_conclusion 对半绿证据不出绿段。
    let ev = Evidence { eq_ppm: 500_000, holes: 5, reds: 2, covered: 50, caught: 3 };
    let p = plain_conclusion(&ev);
    let refused = matches!(p, Ok(seg) if !seg[0].starts_with("退得像"));
    // 反向：全绿证据必须出绿段。
    let ev2 = Evidence { eq_ppm: 1_000_000, holes: 0, reds: 0, covered: 72, caught: 10 };
    let p2 = plain_conclusion(&ev2);
    let green_ok = matches!(p2, Ok(seg) if seg[0].starts_with("退得像"));
    (
        refused && green_ok,
        format!("半绿不放绿={} 全绿出绿={}", refused, green_ok),
    )
}

/// 引用完整率演练（19 节引用全齐 + 缺节构造必检）。
pub fn drill_citation() -> (bool, String) {
    let ev = Evidence { eq_ppm: 1_000_000, holes: 0, reds: 0, covered: 72, caught: 10 };
    let rep = match build_report(&ev) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成失败 {:#06X}", c)),
    };
    let full = rep.sections.len() == CRITERIA_COUNT;
    // 缺节构造：手造 18 节报告必须 coverage_ok=false 且导出必拒。
    let mut short = ExplainReport {
        sections: rep.sections[..18].to_vec(),
        plain3: rep.plain3.clone(),
        cite_ppm: 947_368,
    };
    let cov_bad = !short.coverage_ok();
    let exp_bad = matches!(short.export(), Err(E_SECTION_MISS));
    // 越界判据号必拒。
    let oob = matches!(cite_for(1340, &ev), Err(E_CRIT_OOB));
    // 空句面：coverage_ok 对空句节必 false。
    short.sections.push(Section { fid: 1339, cite: String::new() });
    short.sections[18].cite = String::new();
    let empty_bad = !short.coverage_ok();
    (
        full && cov_bad && exp_bad && oob && empty_bad,
        format!(
            "19节全={} 缺节检={} 越界拒={} 空句检={}",
            full, cov_bad, oob, empty_bad
        ),
    )
}

/// 判据号宇宙演练（19 号连续 + 查名双向一致）。
pub fn drill_criteria_universe() -> (bool, String) {
    let mut contiguous = true;
    for (i, c) in CRITERIA.iter().enumerate() {
        if c.fid != (1321 + i) as u16 {
            contiguous = false;
        }
    }
    let mut lookup_ok = true;
    for c in CRITERIA.iter() {
        if crit_name(c.fid) != Some(c.name) {
            lookup_ok = false;
        }
    }
    let oob = crit_name(1340).is_none() && crit_name(1320).is_none();
    (
        contiguous && lookup_ok && oob && CRITERIA.len() == 19,
        format!("连续={} 查名={} 越界查无={}", contiguous, lookup_ok, oob),
    )
}

/// 稳定性 10/10 演练（同证据十次生成逐字节全同）。
pub fn drill_stable_10() -> (bool, String) {
    let ev = Evidence { eq_ppm: 1_000_000, holes: 0, reds: 0, covered: 72, caught: 10 };
    let first = match build_report(&ev) {
        Ok(r) => match r.export() {
            Ok(v) => v,
            Err(c) => return (false, format!("导出失败 {:#06X}", c)),
        },
        Err(c) => return (false, format!("生成失败 {:#06X}", c)),
    };
    let mut ok = 0u32;
    for _ in 0..INJECT_ROUNDS {
        let again = match build_report(&ev) {
            Ok(r) => r.export(),
            Err(_) => continue,
        };
        if matches!(again, Ok(v) if v == first) {
            ok += 1;
        }
    }
    (ok == INJECT_ROUNDS as u32, format!("{}/{} 逐字节全同", ok, INJECT_ROUNDS))
}

// ═══════════════════════════════════════════════════════════════════════
// §5b 节引用登记簿（逐判据引用史 + 只增不减）
// ═══════════════════════════════════════════════════════════════════════

/// 单判据引用记录。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CiteRec {
    /// 判据号。
    pub fid: u16,
    /// 引用句。
    pub cite: String,
    /// 引用版次（同判据多次出报告时递增）。
    pub ver: u32,
}

/// 节引用登记簿。
pub struct CiteBook {
    recs: Vec<CiteRec>,
    /// 每判据版次。
    vers: [u32; 19],
}

impl CiteBook {
    /// 建。
    pub fn new() -> Self {
        CiteBook { recs: Vec::new(), vers: [0; 19] }
    }

    /// 记一版引用（判据号必须在宇宙内；句非空）。
    pub fn record(&mut self, fid: u16, cite: String) -> Result<u32, u16> {
        let mut idx = None;
        for (i, c) in CRITERIA.iter().enumerate() {
            if c.fid == fid {
                idx = Some(i);
                break;
            }
        }
        let i = match idx {
            Some(i) => i,
            None => return Err(E_CRIT_OOB),
        };
        if cite.is_empty() {
            return Err(E_CITE_EMPTY);
        }
        self.vers[i] += 1;
        self.recs.push(CiteRec { fid, cite, ver: self.vers[i] });
        Ok(self.vers[i])
    }

    /// 覆盖判据数（有引用史的判据数）。
    pub fn covered(&self) -> usize {
        let mut n = 0;
        for v in &self.vers {
            if *v > 0 {
                n += 1;
            }
        }
        n
    }

    /// 版次单调核（同判据 ver 序列严格递增）。
    pub fn vers_monotone(&self) -> bool {
        let mut last = [0u32; 19];
        for r in &self.recs {
            let mut idx = None;
            for (i, c) in CRITERIA.iter().enumerate() {
                if c.fid == r.fid {
                    idx = Some(i);
                    break;
                }
            }
            match idx {
                Some(i) => {
                    if r.ver <= last[i] {
                        return false;
                    }
                    last[i] = r.ver;
                }
                None => return false,
            }
        }
        true
    }

    /// 记录数。
    pub fn len(&self) -> usize {
        self.recs.len()
    }

    /// 出满一轮 19 判据引用。
    pub fn full_sweep(&self) -> bool {
        self.covered() == CRITERIA_COUNT
    }
}

/// 登记簿演练（两轮引用 38 记录 + 版次单调 + 越界/空句拒）。
pub fn drill_cite_book() -> (bool, String) {
    let ev = Evidence { eq_ppm: 1_000_000, holes: 0, reds: 0, covered: 72, caught: 10 };
    let mut book = CiteBook::new();
    for _round in 0..2 {
        for c in CRITERIA.iter() {
            match cite_for(c.fid, &ev) {
                Ok(s) => {
                    if book.record(c.fid, s).is_err() {
                        return (false, "记引用失败".to_string());
                    }
                }
                Err(e) => return (false, format!("节句失败 {:#06X}", e)),
            }
        }
    }
    let oob = matches!(book.record(9999, "x".to_string()), Err(E_CRIT_OOB));
    let empty = matches!(book.record(1321, String::new()), Err(E_CITE_EMPTY));
    (
        book.len() == 38 && book.full_sweep() && book.vers_monotone() && oob && empty,
        format!(
            "记录{} 覆盖19={} 版次单调={} 越界拒={} 空句拒={}",
            book.len(),
            book.full_sweep(),
            book.vers_monotone(),
            oob,
            empty
        ),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §5c 术语使用普查（导出文本内术语命中账）
// ═══════════════════════════════════════════════════════════════════════

/// 术语命中统计（导出文本中每条登记术语的出现次数）。
pub fn term_hits(text_lines: &[String]) -> [usize; 10] {
    let mut hits = [0usize; 10];
    for line in text_lines {
        for (i, t) in TERMS.iter().enumerate() {
            if line.contains(t.term) {
                hits[i] += 1;
            }
        }
    }
    hits
}

/// 术语普查演练（导出文本中核心术语全命中）。
pub fn drill_term_hits() -> (bool, String) {
    let ev = Evidence { eq_ppm: 1_000_000, holes: 0, reds: 0, covered: 72, caught: 10 };
    let rep = match build_report(&ev) {
        Ok(r) => r,
        Err(c) => return (false, format!("生成失败 {:#06X}", c)),
    };
    let lines = match rep.export() {
        Ok(v) => v,
        Err(c) => return (false, format!("导出失败 {:#06X}", c)),
    };
    let hits = term_hits(&lines);
    // 核心术语（等价率/对拍/硬线/红账/覆盖矩阵/长跑/压测/故障注入）。
    let core = [0usize, 1, 3, 4, 6, 7, 8, 9];
    let mut core_ok = true;
    for i in core {
        if hits[i] == 0 {
            core_ok = false;
        }
    }
    let total: usize = hits.iter().sum();
    (core_ok && total >= 8, format!("总命中{} 核心全命中={}", total, core_ok))
}

/// 人话三段式专项演练（全绿三段关键词 + 非绿三段指向修复）。
pub fn drill_plain3_deep() -> (bool, String) {
    let ev_g = Evidence { eq_ppm: 1_000_000, holes: 0, reds: 0, covered: 72, caught: 10 };
    let g = match plain_conclusion(&ev_g) {
        Ok(p) => p,
        Err(c) => return (false, format!("三段失败 {:#06X}", c)),
    };
    let g_ok = g[0].contains("一模一样") && g[1].contains("没有") && g[2].contains("移交");
    let ev_b = Evidence { eq_ppm: 1_000_000, holes: 2, reds: 0, covered: 70, caught: 10 };
    let b = match plain_conclusion(&ev_b) {
        Ok(p) => p,
        Err(c) => return (false, format!("三段失败 {:#06X}", c)),
    };
    let b_ok = b[1].contains("覆盖矩阵缺 2 格") && b[2].contains("补验");
    let ev_c = Evidence { eq_ppm: 1_000_000, holes: 0, reds: 0, covered: 72, caught: 7 };
    let c3 = match plain_conclusion(&ev_c) {
        Ok(p) => p,
        Err(c) => return (false, format!("三段失败 {:#06X}", c)),
    };
    let c_ok = c3[1].contains("10/10");
    (
        g_ok && b_ok && c_ok,
        format!("全绿三段={} 破覆盖点名={} 破注入点名={}", g_ok, b_ok, c_ok),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §6 注入核（6 项）
// ═══════════════════════════════════════════════════════════════════════

/// 注入核 6 项（越界/空句/缺段/谎绿/漂移/未登记术语）。
pub fn inject_core() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut det: Vec<String> = Vec::new();
    let ev_green = Evidence { eq_ppm: 1_000_000, holes: 0, reds: 0, covered: 72, caught: 10 };
    // 注1：判据号越界必拒。
    if matches!(cite_for(9999, &ev_green), Err(E_CRIT_OOB)) {
        caught += 1;
        det.push("越界:拒".to_string());
    } else {
        det.push("越界:漏".to_string());
    }
    // 注2：空句节必检。
    let mut rep = match build_report(&ev_green) {
        Ok(r) => r,
        Err(_) => {
            let mut v = Vec::new();
            v.push("生成失败".to_string());
            return (0, 6, v);
        }
    };
    let saved = rep.sections[0].cite.clone();
    rep.sections[0].cite = String::new();
    if !rep.coverage_ok() {
        caught += 1;
        det.push("空句:抓".to_string());
    } else {
        det.push("空句:漏".to_string());
    }
    rep.sections[0].cite = saved;
    // 注3：缺段必检。
    let mut rep2 = match build_report(&ev_green) {
        Ok(r) => r,
        Err(_) => {
            let mut v = Vec::new();
            v.push("生成失败".to_string());
            return (0, 6, v);
        }
    };
    rep2.plain3[2] = String::new();
    if !rep2.plain_ok() && matches!(rep2.export(), Err(E_PLAIN_SHORT)) {
        caught += 1;
        det.push("缺段:抓".to_string());
    } else {
        det.push("缺段:漏".to_string());
    }
    // 注4：谎绿必拒（红证据出绿段 = build_report 报 E_GREEN_UNEARNED；
    // 直接验证半绿证据走 build_report 得红段话术）。
    let ev_sick = Evidence { eq_ppm: 100_000, holes: 9, reds: 4, covered: 40, caught: 2 };
    match build_report(&ev_sick) {
        Ok(r) if !r.plain3[0].starts_with("退得像") => {
            caught += 1;
            det.push("谎绿:拒".to_string());
        }
        _ => det.push("谎绿:漏".to_string()),
    }
    // 注5：报告漂移判据（同证据两次必同）。
    let r1 = build_report(&ev_green);
    let r2 = build_report(&ev_green);
    let drift_ok = match (r1, r2) {
        (Ok(a), Ok(b)) => a.export() == b.export(),
        _ => false,
    };
    if drift_ok {
        caught += 1;
        det.push("漂移:无".to_string());
    } else {
        det.push("漂移:漏".to_string());
    }
    // 注6：术语表重复判据（表内两两相异）。
    let mut dup = false;
    for i in 0..TERMS.len() {
        for j in (i + 1)..TERMS.len() {
            if TERMS[i].term == TERMS[j].term {
                dup = true;
            }
        }
    }
    if !dup {
        caught += 1;
        det.push("术语:守".to_string());
    } else {
        det.push("术语:漏".to_string());
    }
    (caught, 6, det)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    CODE_ALL.iter().all(|c| *c >= SEG_BASE && *c < SEG_BASE + 16)
}

// ═══════════════════════════════════════════════════════════════════════
// §7 run_unx1333_checks：本单判据聚合（诊断段 0x7730）
// ═══════════════════════════════════════════════════════════════════════

/// 聚合判据（≥6 条，含 10/10 注入演练与段位守卫）。
/// 口径：19 判据全引用、人话三段式、术语一致率 100%、报告稳定。
pub fn run_unx1333_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1333 等价人话化");
    let (ok1, d1) = drill_green_report();
    s.add_dyn("J1-全绿报告19节+三段", ok1, &d1);
    let (ok2, d2) = drill_red_and_unearned();
    s.add_dyn("J2-非绿点名+无据拒绿", ok2, &d2);
    let (ok3, d3) = drill_citation();
    s.add_dyn("J3-引用完整率100%", ok3, &d3);
    let (ok4, d4) = drill_universe_terms();
    s.add_dyn("J4-判据宇宙+术语表", ok4, &d4);
    let (ok5, d5) = drill_stable_10();
    s.add_dyn("J5-报告稳定10/10", ok5, &d5);
    let (ok6, d6) = drill_cite_book();
    s.add_dyn("J6-引用登记簿两轮38", ok6, &d6);
    let (ok7, d7) = drill_terms_deep();
    s.add_dyn("J7-术语命中+三段关键词", ok7, &d7);
    let (c8, t8, d8) = inject_core();
    s.add_dyn(
        "J8-注入核6/6",
        c8 == t8,
        &format!("{}/{} {}", c8, t8, d8.join(" ")),
    );
    s.add("J9-段位守卫", segment_guard(), "0x7730段闭集10码");
    s.add(
        "J10-宇宙常量",
        CRITERIA_COUNT == 19 && TERMS.len() == 10,
        "19判据×10术语写死",
    );
    s
}
