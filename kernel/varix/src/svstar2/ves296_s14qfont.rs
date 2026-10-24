//! ves296_s14qfont — VE-F4056 · T03 排版与 Q 域字体资源
//! 排版字体需求→Q 管线收敛复述单源 + 按语言字体包流送（复用流送单源）+ 缺字→Q 回退链复用 + 私下载字体 P1 收敛红线。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x82C0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum QFontCode {
    Ok = 0,
    PrivateDownload = SEG_BASE + 0x01,
    StreamBroken = SEG_BASE + 0x02,
    MissingSilent = SEG_BASE + 0x03,
    PackageCross = SEG_BASE + 0x04,
    ConvergeSplit = SEG_BASE + 0x05,
}

/// 语言字体包。
#[derive(Clone)]
pub struct FontPackage {
    pub lang: &'static str,
    pub package_id: u32,
    pub streamed: bool,
}

pub struct QFontConverge {
    pub packages: Vec<FontPackage>,
    /// 收敛单源声明：全部字体需求走 Q 管线。
    pub via_pipeline_only: bool,
}

impl QFontConverge {
    pub fn new() -> Self {
        QFontConverge { packages: Vec::new(), via_pipeline_only: true }
    }

    pub fn add_package(&mut self, lang: &'static str, pid: u32) -> Result<(), QFontCode> {
        for p in self.packages.iter() {
            if p.package_id == pid { return Err(QFontCode::PackageCross); }
        }
        self.packages.push(FontPackage { lang, package_id: pid, streamed: false });
        Ok(())
    }

    /// 流送：按语言流送字体包。
    pub fn stream(&mut self, lang: &str) -> Result<u32, QFontCode> {
        for p in self.packages.iter_mut() {
            if p.lang == lang {
                p.streamed = true;
                return Ok(p.package_id);
            }
        }
        Err(QFontCode::StreamBroken)
    }

    /// 收敛红线：私下载字体（不经管线）= P1。此处断言收敛开关不可关。
    pub fn enforce_converge(&self, via_pipeline: bool) -> Result<(), QFontCode> {
        if !via_pipeline { return Err(QFontCode::PrivateDownload); }
        if !self.via_pipeline_only { return Err(QFontCode::ConvergeSplit); }
        Ok(())
    }

    /// 缺字协同：缺字必须计数显性（不静默）。
    pub fn missing_glyph(&self, count: &mut u32, _lang: &str, n: u32) -> Result<(), QFontCode> {
        if n == 0 { return Ok(()); }
        *count += n;
        Ok(())
    }
    /// 缺字静默检出：计数为零但缺字报告存在。
    pub fn silent_missing(reported: u32, counted: u32) -> QFontCode {
        if reported > 0 && counted == 0 { QFontCode::MissingSilent } else { QFontCode::Ok }
    }

    /// 包对齐断言：每个已注册语言都有包。
    pub fn packages_aligned(&self, langs: &[&str]) -> bool {
        langs.iter().all(|l| self.packages.iter().any(|p| p.lang == *l))
    }
}

pub fn run_ves296_checks() -> CheckSet {
    let mut set = CheckSet::new("ves296_s14qfont");
    let mut fc = QFontConverge::new();

    // 1) 收敛单源默认开。
    set.add("C4056-01", fc.via_pipeline_only, "收敛单源");

    // 2) 私下载拒（P1 红线）。
    set.add("C4056-02", fc.enforce_converge(false) == Err(QFontCode::PrivateDownload), "私下载拒");

    // 3) 管线路径放行。
    set.add("C4056-03", fc.enforce_converge(true).is_ok(), "管线放行");

    // 4) 字体包注册。
    let _ = fc.add_package("cjk", 101);
    let _ = fc.add_package("arabic", 102);
    set.add("C4056-04", fc.packages.len() == 2, "包注册");

    // 5) 包跨项拒（同 id）。
    set.add("C4056-05", fc.add_package("thai", 101) == Err(QFontCode::PackageCross), "包跨项拒");

    // 6) 流送。
    set.add("C4056-06", fc.stream("cjk") == Ok(101) && fc.packages[0].streamed, "流送");

    // 7) 流送断（无包语言）。
    set.add("C4056-07", fc.stream("klingon") == Err(QFontCode::StreamBroken), "流送断");

    // 8) 缺字计数显性。
    let mut cnt = 0u32;
    let _ = fc.missing_glyph(&mut cnt, "cjk", 7);
    set.add("C4056-08", cnt == 7, "缺字计数");

    // 9) 缺字静默检出。
    set.add("C4056-09", QFontConverge::silent_missing(5, 0) == QFontCode::MissingSilent
        && QFontConverge::silent_missing(5, 5) == QFontCode::Ok, "静默检出");

    // 10) 包对齐。
    set.add("C4056-10", fc.packages_aligned(&["cjk", "arabic"]) && !fc.packages_aligned(&["cjk", "thai"]), "包对齐");

    // 11) 零缺字放行。
    let mut c2 = 0u32;
    set.add("C4056-11", fc.missing_glyph(&mut c2, "cjk", 0).is_ok() && c2 == 0, "零缺字");

    // 12) 段位自查。
    set.add("C4056-12", SEG_BASE == 0x82C0, "段位 0x82C0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converge_flow() {
        let mut fc = QFontConverge::new();
        assert_eq!(fc.enforce_converge(false), Err(QFontCode::PrivateDownload));
        let _ = fc.add_package("cjk", 1);
        assert_eq!(fc.stream("cjk"), Ok(1));
        assert_eq!(fc.stream("xx"), Err(QFontCode::StreamBroken));
    }
}
