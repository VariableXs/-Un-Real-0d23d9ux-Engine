//! ves329_s17verify — VE-F4089 · T05 多语言资源验证
//! 语言包安装前五检：manifest 完整/校验和/字体字形覆盖/资产引用闭合/无障碍资产齐备——五检全过才可切换 + 验证报告留痕 + 任一检不过拒绝激活并列明细 + 字形缺口回退链提示 + 引用断定位缺资产。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x95C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VyCode {
    Ok = 0,
    ManifestIncomplete = SEG_BASE + 0x01,
    ChecksumMismatch = SEG_BASE + 0x02,
    GlyphGap = SEG_BASE + 0x03,
    RefBroken = SEG_BASE + 0x04,
    A11yAbsent = SEG_BASE + 0x05,
}

/// 五检项标签单源。
pub const CHECK_LABELS: [&'static str; 5] = [
    "manifest", "checksum", "glyph-coverage", "ref-closure", "a11y-complete"];

/// 验证报告：每检一明细（标签+过否+线索）。
#[derive(Clone)]
pub struct CheckLine { pub label: &'static str, pub pass: bool, pub clue: &'static str }

#[derive(Clone)]
pub struct Report { pub lang: &'static str, pub lines: Vec<CheckLine>, pub activated: bool }

impl Report {
    pub fn all_pass(&self) -> bool { self.lines.iter().all(|l| l.pass) }
    pub fn failed_labels(&self) -> Vec<&'static str> {
        self.lines.iter().filter(|l| !l.pass).map(|l| l.label).collect()
    }
}

/// manifest 完整性：语种/版本/体积/校验和四要件非零。
fn check_manifest(lang: &str, version: u32, total_kb: u32, checksum: u32) -> Result<(), VyCode> {
    if lang.is_empty() || version == 0 || total_kb == 0 || checksum == 0 {
        return Err(VyCode::ManifestIncomplete);
    }
    Ok(())
}

/// 字形覆盖：需要的码点集 ⊆ 字体声明覆盖集；缺口 → GlyphGap 并给回退链提示。
pub struct GlyphChecker { pub covered: Vec<u32> }

impl GlyphChecker {
    pub fn gap(&self, need: &[u32]) -> Vec<u32> {
        need.iter().filter(|c| !self.covered.contains(c)).map(|c| *c).collect()
    }
}

/// 引用闭合：清单引用的资产 id ⊆ 包内实际资产 id；断点定位。
pub struct RefChecker { pub present: Vec<&'static str> }

impl RefChecker {
    pub fn broken(&self, refs: &[&'static str]) -> Vec<&'static str> {
        refs.iter().filter(|r| !self.present.contains(r)).map(|r| *r).collect()
    }
}

/// 验证器：跑五检 → 报告留痕；全过才激活。
pub struct Verifier {
    pub glyphs: GlyphChecker,
    pub refs: RefChecker,
    pub reports: Vec<Report>,
}

impl Verifier {
    pub fn new(covered: Vec<u32>, present: Vec<&'static str>) -> Self {
        Verifier { glyphs: GlyphChecker { covered }, refs: RefChecker { present }, reports: Vec::new() }
    }

    pub fn verify(&mut self,
        lang: &'static str, version: u32, total_kb: u32, checksum: u32,
        recompute_checksum: u32,
        need_cps: &[u32], pack_refs: &[&'static str],
        a11y_assets: usize, a11y_required: usize,
    ) -> Report {
        let mut lines: Vec<CheckLine> = Vec::new();
        // 检一 manifest 完整。
        lines.push(match check_manifest(lang, version, total_kb, checksum) {
            Ok(()) => CheckLine { label: CHECK_LABELS[0], pass: true, clue: "四要件齐" },
            Err(_) => CheckLine { label: CHECK_LABELS[0], pass: false, clue: "manifest 缺要件" },
        });
        // 检二校验和（登记值 vs 重算值）。
        lines.push(CheckLine {
            label: CHECK_LABELS[1], pass: checksum == recompute_checksum,
            clue: if checksum == recompute_checksum { "校验和一致" } else { "校验和不一致" },
        });
        // 检三字形覆盖。
        let gap = self.glyphs.gap(need_cps);
        lines.push(CheckLine {
            label: CHECK_LABELS[2], pass: gap.is_empty(),
            clue: if gap.is_empty() { "字形全覆盖" } else { "缺口走回退链" },
        });
        // 检四引用闭合。
        let broken = self.refs.broken(pack_refs);
        lines.push(CheckLine {
            label: CHECK_LABELS[3], pass: broken.is_empty(),
            clue: if broken.is_empty() { "引用闭合" } else { "存在断引用" },
        });
        // 检五无障碍资产齐备（硬检）。
        lines.push(CheckLine {
            label: CHECK_LABELS[4], pass: a11y_assets >= a11y_required,
            clue: if a11y_assets >= a11y_required { "无障碍资产齐" } else { "无障碍资产缺" },
        });
        let all_pass = lines.iter().all(|l| l.pass);
        let r = Report { lang, lines, activated: all_pass };
        self.reports.push(r.clone());
        r
    }
}

/// 激活门：只有最近报告全过才可切。
pub fn activation_gate(r: &Report) -> Result<(), VyCode> {
    if !r.all_pass() { return Err(VyCode::GlyphGap); } // 具体码由 failed_labels 细分
    Ok(())
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves329_checks() -> CheckSet {
    let mut set = CheckSet::new("ves329");

    // C4089-01 五检标签单源：五项齐备。
    chk(&mut set, "C4089-01", CHECK_LABELS.len() == 5 && CHECK_LABELS[4] == "a11y-complete", "五检标签齐备");

    // C4089-02 manifest 缺件（体积 0）→ 检一红。
    let mut v = Verifier::new(vec![0x41, 0x9111], vec!["txt", "fnt", "img"]);
    let r1 = v.verify("ja", 9002, 0, 12345, 12345, &[0x41], &["txt", "fnt"], 3, 3).clone();
    chk(&mut set, "C4089-02", r1.failed_labels() == vec!["manifest"] && !r1.activated, "体积 0 → manifest 检红");

    // C4089-03 校验和不一致 → 检二红。
    let r2 = v.verify("ja", 9002, 2900, 12345, 99999, &[0x41], &["txt", "fnt"], 3, 3).clone();
    chk(&mut set, "C4089-03", r2.failed_labels().contains(&"checksum"), "校验和不一致 → 红");

    // C4089-04 字形缺口 → 检三红 + 回退链提示线索。
    let r3 = v.verify("ja", 9002, 2900, 12345, 12345, &[0x41, 0x9112], &["txt", "fnt"], 3, 3).clone();
    let l3 = r3.lines.iter().find(|l| l.label == "glyph-coverage").unwrap();
    chk(&mut set, "C4089-04", !l3.pass && l3.clue == "缺口走回退链", "0x9112 缺口 → 回退链提示");

    // C4089-05 引用断 → 检四红 + 定位缺资产。
    let r4 = v.verify("ja", 9002, 2900, 12345, 12345, &[0x41], &["txt", "aud-missing"], 3, 3).clone();
    let broken = v.refs.broken(&["txt", "aud-missing"]);
    chk(&mut set, "C4089-05", r4.failed_labels().contains(&"ref-closure") && broken == vec!["aud-missing"], "断引用定位 aud-missing");

    // C4089-06 无障碍资产缺 → 检五红（硬检）。
    let r5 = v.verify("ja", 9002, 2900, 12345, 12345, &[0x41], &["txt", "fnt"], 2, 3).clone();
    chk(&mut set, "C4089-06", r5.failed_labels().contains(&"a11y-complete"), "无障碍 2/3 → 硬检红");

    // C4089-07 五检全过 → 激活。
    let r7 = v.verify("ja", 9002, 2900, 12345, 12345, &[0x41, 0x9111], &["txt", "fnt", "img"], 3, 3).clone();
    chk(&mut set, "C4089-07", r7.all_pass() && r7.activated, "五检全过激活");

    // C4089-08 激活门：不过 → 拒绝激活。
    chk(&mut set, "C4089-08", activation_gate(&r5).is_err() && activation_gate(&r7).is_ok(), "门只放全过报告");

    // C4089-09 报告留痕：多次验证历史可查。
    chk(&mut set, "C4089-09", v.reports.len() == 6 && v.reports[0].lang == "ja", "六份报告留痕");

    // C4089-10 明细清单：失败标签列表单源复述。
    let mut mixed = Report { lang: "ko", lines: vec![
        CheckLine { label: CHECK_LABELS[0], pass: true, clue: "" },
        CheckLine { label: CHECK_LABELS[1], pass: false, clue: "" },
        CheckLine { label: CHECK_LABELS[2], pass: true, clue: "" },
        CheckLine { label: CHECK_LABELS[3], pass: false, clue: "" },
        CheckLine { label: CHECK_LABELS[4], pass: true, clue: "" },
    ], activated: false };
    chk(&mut set, "C4089-10", mixed.failed_labels() == vec!["checksum", "ref-closure"], "明细列双红");

    // C4089-11 字形 gap 计算单源。
    let g = v.glyphs.gap(&[0x41, 0x62, 0x9111]);
    chk(&mut set, "C4089-11", g == vec![0x62], "覆盖 0x41/0x9111 → 唯一缺 0x62");

    // C4089-12 五检五线：报告结构含且仅含五检标签。
    chk(&mut set, "C4089-12", r7.lines.len() == 5
        && r7.lines.iter().map(|l| l.label).collect::<Vec<_>>() == CHECK_LABELS.to_vec(), "报告五线序齐");

    let _ = &mut mixed;
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves329_smoke() {
        let set = run_ves329_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves329 red: {:?}", red);
    }

    #[test]
    fn ves329_ref_closure() {
        let rc = RefChecker { present: vec!["a", "b"] };
        assert_eq!(rc.broken(&["a", "c", "d"]), vec!["c", "d"]);
        assert!(rc.broken(&["a"]).is_empty());
    }
}
