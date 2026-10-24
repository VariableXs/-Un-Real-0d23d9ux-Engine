//! VE-F3985 · S 域安全终审
//!
//! 终审：S 域安全三面终审（读屏语义注入 / 色标配置注入 / 工具插件面
//! ——F3969 复审升级为终审）+ 红线全量复核 + 越权偏好写入专项断言。
//! 数据结构：终审册（三面）；红线复核表；专项断言集。
//! 错误路径：面未清→阻断收口；红线破→最高级立案；断言缺→补立。
//! 性能：终审 O(面×项)；断言 O(集)；立案 O(1)。
//! 跨批对接：F3969 上游；F3992 清账前提。
//! 无障碍与隐私：安全终审与隐私红线并审（偏好不越权为核心断言——域
//! 本色）；无新增面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3FC0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3FC0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T5SCode(pub u16);

impl T5SCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const FACE_DIRTY: Self = Self(SEG_BASE + 1);
    pub const REDLINE_BROKEN: Self = Self(SEG_BASE + 2);
    pub const ASSERT_MISSING: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::FACE_DIRTY => "面未清→阻断收口",
            Self::REDLINE_BROKEN => "红线破→最高级立案",
            Self::ASSERT_MISSING => "断言缺→补立",
            _ => "未知",
        }
    }
}

// ── 安全三面 ─────────────────────────────────────────────────────────

/// 终审三面。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecFace {
    /// 读屏语义注入面。
    SemInj,
    /// 色标配置注入面。
    ColorInj,
    /// 工具插件面。
    PluginFace,
}

pub const SEC_FACES: [SecFace; 3] = [SecFace::SemInj, SecFace::ColorInj, SecFace::PluginFace];

impl SecFace {
    pub fn name(self) -> &'static str {
        match self {
            SecFace::SemInj => "读屏语义注入",
            SecFace::ColorInj => "色标配置注入",
            SecFace::PluginFace => "工具插件面",
        }
    }

    pub fn slot(self) -> usize {
        match self {
            SecFace::SemInj => 0,
            SecFace::ColorInj => 1,
            SecFace::PluginFace => 2,
        }
    }
}

/// 单面终审：扫描项 + 清除数 + 残留数。
#[derive(Debug, Clone)]
pub struct FaceAudit {
    /// 扫描项数。
    pub scanned: u32,
    /// 清除数。
    pub cleared: u32,
    /// 残留数（>0 = 面未清）。
    pub residual: u32,
}

/// 终审册（三面）。
#[derive(Debug, Clone)]
pub struct SecFinalBook {
    pub faces: [Option<FaceAudit>; 3],
    /// 红线复核表：红线名 → 通过。
    pub redlines: Vec<(&'static str, bool)>,
    /// 专项断言集：断言名 → 已立。
    pub asserts: Vec<(&'static str, bool)>,
}

pub const REDLINE_TOTAL: usize = 6;

pub const REDLINE_NAMES: [&str; REDLINE_TOTAL] = [
    "语义注入红线",
    "色标注入红线",
    "插件沙箱红线",
    "越权偏好红线",
    "隐私采集红线",
    "录证脱敏红线",
];

impl SecFinalBook {
    pub fn new() -> Self {
        Self {
            faces: [None, None, None],
            redlines: Vec::new(),
            asserts: Vec::new(),
        }
    }

    /// 登记一面。
    pub fn put_face(&mut self, face: SecFace, scanned: u32, cleared: u32) {
        let residual = scanned.saturating_sub(cleared);
        self.faces[face.slot()] = Some(FaceAudit { scanned, cleared, residual });
    }

    /// 面未清检出（任一面残留 > 0 或缺面）。
    pub fn face_dirty(&self) -> bool {
        self.faces.iter().any(|f| match f {
            Some(a) => a.residual > 0,
            None => true, // 缺面 = 未清
        })
    }

    /// 红线全量复核登记。
    pub fn put_redline(&mut self, name: &'static str, pass: bool) -> bool {
        if !REDLINE_NAMES.contains(&name) {
            return false;
        }
        if let Some(slot) = self.redlines.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = pass;
        } else {
            self.redlines.push((name, pass));
        }
        true
    }

    /// 红线全量：六条齐备且全过。
    pub fn redlines_full_pass(&self) -> bool {
        self.redlines.len() == REDLINE_TOTAL && self.redlines.iter().all(|(_, p)| *p)
    }

    /// 破红线清单（最高级立案对象）。
    pub fn broken_redlines(&self) -> Vec<&'static str> {
        self.redlines.iter().filter(|(_, p)| !p).map(|(n, _)| *n).collect()
    }

    /// 专项断言立集：越权偏好写入为核心断言。
    pub fn put_assert(&mut self, name: &'static str, established: bool) {
        if let Some(slot) = self.asserts.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = established;
        } else {
            self.asserts.push((name, established));
        }
    }

    /// 断言缺检（登记面里未立的）。
    pub fn missing_asserts(&self) -> Vec<&'static str> {
        self.asserts.iter().filter(|(_, e)| !e).map(|(n, _)| *n).collect()
    }
}

// ── 越权偏好写入专项断言 ─────────────────────────────────────────────

/// 偏好写入审计：写动作 → 是否越权。
#[derive(Debug, Clone)]
pub struct PrefWriteAudit {
    /// 写动作（偏好键）。
    pub key: String,
    /// 写入者。
    pub writer: String,
    /// 越权标记。
    pub overreach: bool,
}

/// 核心断言：偏好不越权（无障碍配置只允许用户与授权代理写）。
pub fn pref_overreach<'a>(writes: &'a [PrefWriteAudit], authorized: &[&str]) -> Vec<&'a PrefWriteAudit> {
    writes.iter().filter(|w| {
        w.overreach || !authorized.contains(&w.writer.as_str())
    }).collect()
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves239_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves239_s10sec");

    // C3985-01 诊断码段不越窗。
    chk(&mut set, "C3985-01",
        T5SCode::ASSERT_MISSING.0 < SEG_BASE + 16 && SEG_BASE == 0x3FC0, "err");

    // C3985-02 三面齐备。
    chk(&mut set, "C3985-02",
        SEC_FACES.len() == 3
            && SEC_FACES[0].name() == "读屏语义注入"
            && SEC_FACES[2].name() == "工具插件面", "err");

    // C3985-03 面登记：全清残留 0。
    let mut b = SecFinalBook::new();
    b.put_face(SecFace::SemInj, 40, 40);
    b.put_face(SecFace::ColorInj, 25, 25);
    b.put_face(SecFace::PluginFace, 60, 60);
    chk(&mut set, "C3985-03", !b.face_dirty(), "err");

    // C3985-04 残留 > 0 → 面未清（阻断收口）。
    b.put_face(SecFace::ColorInj, 25, 24);
    chk(&mut set, "C3985-04",
        b.face_dirty()
            && b.faces[SecFace::ColorInj.slot()].as_ref().map(|a| a.residual).unwrap_or(0) == 1, "err");

    // C3985-05 缺面 = 未清。
    let mut b2 = SecFinalBook::new();
    b2.put_face(SecFace::SemInj, 10, 10);
    b2.put_face(SecFace::PluginFace, 10, 10);
    chk(&mut set, "C3985-05", b2.face_dirty(), "err");

    // C3985-06 红线全量登记：越名拒绝。
    let mut b3 = SecFinalBook::new();
    chk(&mut set, "C3985-06", !b3.put_redline("伪红线", true), "err");

    // C3985-07 六红线齐备全过。
    for n in REDLINE_NAMES.iter() {
        b3.put_redline(n, true);
    }
    chk(&mut set, "C3985-07", b3.redlines_full_pass() && b3.broken_redlines().is_empty(), "err");

    // C3985-08 一条破 → 破清单 + 全量不过。
    b3.put_redline("越权偏好红线", false);
    chk(&mut set, "C3985-08",
        !b3.redlines_full_pass() && b3.broken_redlines() == vec!["越权偏好红线"], "err");

    // C3985-09 断言缺检：未立断言清单。
    let mut b4 = SecFinalBook::new();
    b4.put_assert("偏好不越权写入", true);
    b4.put_assert("语义树只读", false);
    b4.put_assert("插件沙箱逃逸零", false);
    chk(&mut set, "C3985-09",
        b4.missing_asserts() == vec!["语义树只读", "插件沙箱逃逸零"], "err");

    // C3985-10 断言补立后清单清空。
    b4.put_assert("语义树只读", true);
    b4.put_assert("插件沙箱逃逸零", true);
    chk(&mut set, "C3985-10", b4.missing_asserts().is_empty(), "err");

    // C3985-11 越权偏好写入专项：越权写检出。
    let writes = [
        PrefWriteAudit { key: String::from("色标模式"), writer: String::from("user"), overreach: false },
        PrefWriteAudit { key: String::from("朗读语速"), writer: String::from("plugin-x"), overreach: false },
        PrefWriteAudit { key: String::from("焦点环"), writer: String::from("user"), overreach: true },
    ];
    let over = pref_overreach(&writes, &["user", "agent-a11y"]);
    chk(&mut set, "C3985-11", over.len() == 2, "err");

    // C3985-12 授权代理写入放行。
    let w2 = [PrefWriteAudit { key: String::from("色标模式"), writer: String::from("agent-a11y"), overreach: false }];
    chk(&mut set, "C3985-12", pref_overreach(&w2, &["user", "agent-a11y"]).is_empty(), "err");

    // C3985-13 槽位双射。
    let slots: Vec<usize> = SEC_FACES.iter().map(|f| f.slot()).collect();
    chk(&mut set, "C3985-13", slots[0] != slots[1] && slots[1] != slots[2] && slots[0] != slots[2], "err");

    // C3985-14 红线名表六条互异。
    let mut uniq = true;
    for i in 0..REDLINE_TOTAL {
        for j in (i + 1)..REDLINE_TOTAL {
            if REDLINE_NAMES[i] == REDLINE_NAMES[j] { uniq = false; }
        }
    }
    chk(&mut set, "C3985-14", uniq && REDLINE_TOTAL == 6, "err");

    // C3985-15 面审计 scanned < cleared 钳制（residual 不下溢）。
    let mut b5 = SecFinalBook::new();
    b5.put_face(SecFace::SemInj, 5, 9);
    chk(&mut set, "C3985-15",
        b5.faces[0].as_ref().map(|a| a.residual).unwrap_or(99) == 0, "err");

    // C3985-16 断言重立更新（不重复登记）。
    let mut b6 = SecFinalBook::new();
    b6.put_assert("核心断言", false);
    b6.put_assert("核心断言", true);
    chk(&mut set, "C3985-16", b6.asserts.len() == 1 && !b6.missing_asserts().is_empty() == false, "err");

    // C3985-17 破红线语义（最高级立案复述）。
    chk(&mut set, "C3985-17", T5SCode::REDLINE_BROKEN.explain().contains("最高级立案"), "err");

    // C3985-18 隐私红线在六红线表内（并审）。
    chk(&mut set, "C3985-18",
        REDLINE_NAMES.contains(&"隐私采集红线")
            && REDLINE_NAMES.contains(&"录证脱敏红线"), "err");

    // C3985-19 越权标记直达检出（即使 writer 授权）。
    let w3 = [PrefWriteAudit { key: String::from("焦点环"), writer: String::from("user"), overreach: true }];
    chk(&mut set, "C3985-19", pref_overreach(&w3, &["user"]).len() == 1, "err");

    // C3985-20 错误码互异。
    let codes = [T5SCode::OK.0, T5SCode::FACE_DIRTY.0, T5SCode::REDLINE_BROKEN.0,
        T5SCode::ASSERT_MISSING.0];
    chk(&mut set, "C3985-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn residual_blocks() {
        let mut b = SecFinalBook::new();
        b.put_face(SecFace::SemInj, 10, 8);
        assert!(b.face_dirty());
    }

    #[test]
    fn redline_full_pass() {
        let mut b = SecFinalBook::new();
        for n in REDLINE_NAMES.iter() {
            b.put_redline(n, true);
        }
        assert!(b.redlines_full_pass());
        b.put_redline("隐私采集红线", false);
        assert!(!b.redlines_full_pass());
    }

    #[test]
    fn pref_overreach_detects() {
        let w = [PrefWriteAudit { key: String::from("k"), writer: String::from("rogue"), overreach: false }];
        assert_eq!(pref_overreach(&w, &["user"]).len(), 1);
    }
}
