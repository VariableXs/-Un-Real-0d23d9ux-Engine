//! ves338_s18fuzz — VE-F4098 · T05 语言资源 fuzz
//! fuzz 三面：manifest 解析（畸形字段）/ 字体文件（畸形字形表）/ 字幕时间轴（乱序越界）——三面语料库 + 三零线（崩溃 0/挂起 0/越权写 0）+ 线未达阻断 + 最小复现入库 + 越权写为零线复述。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x9CC0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FzCode {
    Ok = 0,
    Crash = SEG_BASE + 0x01,
    Hang = SEG_BASE + 0x02,
    PrivWrite = SEG_BASE + 0x03,
    FaceMissing = SEG_BASE + 0x04,
}

/// 三个 fuzz 面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FuzzFace { ManifestParse = 0, FontGlyphTable = 1, SubtitleTimeline = 2 }

impl FuzzFace {
    pub const ALL: [FuzzFace; 3] = [
        FuzzFace::ManifestParse, FuzzFace::FontGlyphTable, FuzzFace::SubtitleTimeline];
    pub fn name(self) -> &'static str {
        match self { FuzzFace::ManifestParse => "manifest-parse",
            FuzzFace::FontGlyphTable => "font-glyph-table", FuzzFace::SubtitleTimeline => "subtitle-timeline" }
    }
}

/// 语料样本：面 + 畸形内容 + 期望行为。
#[derive(Clone)]
pub struct CorpusSample { pub face: FuzzFace, pub payload: &'static str, pub malformed: bool }

/// 语料库：三面语料齐备（面缺 → 补面）。
pub struct CorpusBook { pub samples: Vec<CorpusSample> }

impl CorpusBook {
    pub fn new() -> Self { CorpusBook { samples: Vec::new() } }

    pub fn add(&mut self, s: CorpusSample) { self.samples.push(s); }

    /// 面覆盖：三面每面至少 2 样本。
    pub fn covered(&self) -> bool {
        FuzzFace::ALL.iter().all(|f| self.samples.iter().filter(|s| s.face == *f).count() >= 2)
    }

    pub fn missing_faces(&self) -> Vec<&'static str> {
        FuzzFace::ALL.iter()
            .filter(|f| self.samples.iter().filter(|s| s.face == **f).count() < 2)
            .map(|f| f.name())
            .collect()
    }
}

/// 畸形 manifest 解析器（被测面一）：字段必须 `k=v` 且 k 非空。
pub fn parse_manifest_line(line: &str) -> Result<(&str, &str), FzCode> {
    let (k, v) = match line.split_once('=') {
        Some((k, v)) => (k, v),
        None => return Err(FzCode::Crash), // 畸形：无 = → 崩溃线样本（解析器必须报错不崩）
    };
    if k.is_empty() { return Err(FzCode::Crash); }
    Ok((k, v))
}

/// 字形表校验器（被测面二）：字形项 (偏移, 长度)，长度 0 或偏移+长度越界 → 拒。
pub fn check_glyph_table(entries: &[(u32, u32)], blob_len: u32) -> Result<(), FzCode> {
    for (off, len) in entries.iter() {
        if *len == 0 { return Err(FzCode::Crash); }
        if off.checked_add(*len).map_or(true, |end| end > blob_len) {
            return Err(FzCode::PrivWrite); // 越界读/写线
        }
    }
    Ok(())
}

/// 字幕时间轴校验器（被测面三）：时间戳单调递增且非负、不越片长。
pub fn check_timeline(stamps: &[u32], video_ms: u32) -> Result<(), FzCode> {
    if stamps.is_empty() { return Err(FzCode::Crash); }
    for w in stamps.windows(2) {
        if w[1] <= w[0] { return Err(FzCode::Crash); } // 乱序
    }
    if let Some(last) = stamps.last() {
        if *last > video_ms { return Err(FzCode::PrivWrite); } // 越界
    }
    Ok(())
}

/// 三零线记录：崩溃 0 / 挂起 0 / 越权写 0。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ZeroLine { pub crashes: u32, pub hangs: u32, pub priv_writes: u32 }

impl ZeroLine {
    pub fn clean() -> Self { ZeroLine { crashes: 0, hangs: 0, priv_writes: 0 } }
    pub fn reached(&self) -> bool { self.crashes == 0 && self.hangs == 0 && self.priv_writes == 0 }
}

/// fuzz 执行器：跑语料 → 解析器全报错不崩；任何"崩溃样本被接受"记线破。
pub struct FuzzRunner { pub zero: ZeroLine, pub minimals: Vec<&'static str>, pub blocked: bool }

impl FuzzRunner {
    pub fn new() -> Self {
        FuzzRunner { zero: ZeroLine::clean(), minimals: Vec::new(), blocked: false }
    }

    /// 跑单样本：解析器返回 Err 即正确拒绝（畸形必拒）；返回 Ok 且 malformed → 记崩溃线。
    pub fn run_manifest(&mut self, s: &CorpusSample) -> Result<(), FzCode> {
        let r = parse_manifest_line(s.payload);
        match (s.malformed, r) {
            (true, Ok(_)) => { self.zero.crashes += 1; self.minimals.push(s.payload); self.blocked = true; Err(FzCode::Crash) }
            (true, Err(e)) => Err(e),
            (false, Ok(_)) => Ok(()),
            (false, Err(_)) => { self.zero.crashes += 1; self.minimals.push(s.payload); self.blocked = true; Err(FzCode::Crash) }
        }
    }

    /// 线未达 → 阻断。
    pub fn gate(&self) -> Result<(), FzCode> {
        if self.zero.reached() && !self.blocked { Ok(()) } else { Err(FzCode::Crash) }
    }

    /// 最小复现入库数。
    pub fn minimal_count(&self) -> usize { self.minimals.len() }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves338_checks() -> CheckSet {
    let mut set = CheckSet::new("ves338");

    // C338-01 三面名单单源。
    chk(&mut set, "C338-01",
        FuzzFace::ALL[0].name() == "manifest-parse" && FuzzFace::ALL[2].name() == "subtitle-timeline",
        "三面单源");

    // C338-02 语料三面齐备。
    let mut cb = CorpusBook::new();
    cb.add(CorpusSample { face: FuzzFace::ManifestParse, payload: "lang=zh", malformed: false });
    cb.add(CorpusSample { face: FuzzFace::ManifestParse, payload: "=novalue", malformed: true });
    cb.add(CorpusSample { face: FuzzFace::FontGlyphTable, payload: "glyph-table", malformed: true });
    cb.add(CorpusSample { face: FuzzFace::FontGlyphTable, payload: "glyph-table-ok", malformed: false });
    cb.add(CorpusSample { face: FuzzFace::SubtitleTimeline, payload: "timeline", malformed: true });
    cb.add(CorpusSample { face: FuzzFace::SubtitleTimeline, payload: "timeline-ok", malformed: false });
    chk(&mut set, "C338-02", cb.covered() && cb.missing_faces().is_empty(), "三面各 2 样本齐");

    // C338-03 面缺检出。
    let mut cb2 = CorpusBook::new();
    cb2.add(CorpusSample { face: FuzzFace::ManifestParse, payload: "a=1", malformed: false });
    chk(&mut set, "C338-03", cb2.missing_faces() == vec!["font-glyph-table", "subtitle-timeline"], "缺两面定位");

    // C338-04 畸形 manifest 必拒不崩。
    let mut fr = FuzzRunner::new();
    let bad = CorpusSample { face: FuzzFace::ManifestParse, payload: "novalue", malformed: true };
    chk(&mut set, "C338-04", fr.run_manifest(&bad) == Err(FzCode::Crash) && fr.zero.reached(), "无 = 畸形 → 拒");

    // C338-05 合法 manifest 放行。
    let good = CorpusSample { face: FuzzFace::ManifestParse, payload: "checksum=1234", malformed: false };
    let mut fr2 = FuzzRunner::new();
    chk(&mut set, "C338-05", fr2.run_manifest(&good).is_ok() && fr2.gate().is_ok(), "合法 → 放行且零线达");

    // C338-06 畸形字形表：长度 0 拒。
    chk(&mut set, "C338-06", check_glyph_table(&[(0, 0)], 100) == Err(FzCode::Crash), "len=0 → 拒");

    // C338-07 字形表越界 → 越权写线。
    chk(&mut set, "C338-07", check_glyph_table(&[(90, 20)], 100) == Err(FzCode::PrivWrite), "90+20>100 => 越界");

    // C338-08 字形表合法放行（含边界贴合）。
    chk(&mut set, "C338-08", check_glyph_table(&[(0, 100)], 100).is_ok(), "0+100=100 贴界放行");

    // C338-09 字幕乱序拒。
    chk(&mut set, "C338-09", check_timeline(&[100, 300, 200], 1000) == Err(FzCode::Crash), "乱序 → 拒");

    // C338-10 字幕越片长 → 越权线。
    chk(&mut set, "C338-10", check_timeline(&[100, 2000], 1000) == Err(FzCode::PrivWrite), "尾戳 2000>1000 => 越界");

    // C338-11 字幕合法放行。
    chk(&mut set, "C338-11", check_timeline(&[100, 300, 900], 1000).is_ok(), "单调且未越界");

    // C338-12 最小复现入库 + 阻断权。
    let mut fr3 = FuzzRunner::new();
    let evil = CorpusSample { face: FuzzFace::ManifestParse, payload: "k=", malformed: false };
    let _ = fr3.run_manifest(&evil);
    chk(&mut set, "C338-12", fr3.minimal_count() == 1 && fr3.gate().is_err() && fr3.blocked, "复现入库 + 线破阻断");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves338_smoke() {
        let set = run_ves338_checks();
        assert!(set.failed_count() == 0, "ves338 red: {}", set.red_items_text());
    }

    #[test]
    fn ves338_manifest_parse() {
        assert!(parse_manifest_line("a=b").is_ok());
        assert!(parse_manifest_line("").is_err());
        assert!(parse_manifest_line("novalue").is_err());
    }
}
