//! ves327_s17dl — VE-F4087 · T05 语言包按需下载
//! 市场位三流程（浏览/下载/更新）+ 断点续传（分块+续传记录）+ 先下后切（校验后才切换现役语言——失败不破坏现役）+ 计量网络须用户确认 + 下载记录不含用户画像。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x94C0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DlCode {
    Ok = 0,
    MeteredDenied = SEG_BASE + 0x01,
    ChunkBad = SEG_BASE + 0x02,
    VerifyFail = SEG_BASE + 0x03,
    NotReady = SEG_BASE + 0x04,
    DupSession = SEG_BASE + 0x05,
}

pub const CHUNK_KB: u32 = 256;

/// 分块状态：已收块位图（简化为布尔向量）。
#[derive(Clone)]
pub struct Session {
    pub lang: &'static str,
    pub version: u32,
    pub total_kb: u32,
    pub chunks: Vec<bool>,
    /// 计量网络标记。
    pub metered: bool,
    /// 计量网用户确认。
    pub user_ok: bool,
    pub done: bool,
}

impl Session {
    pub fn new(lang: &'static str, version: u32, total_kb: u32, metered: bool) -> Session {
        let n = (total_kb + CHUNK_KB - 1) / CHUNK_KB;
        Session { lang, version, total_kb, chunks: vec![false; n as usize], metered, user_ok: false, done: false }
    }

    pub fn chunk_count(&self) -> u32 { self.chunks.len() as u32 }

    /// 收块：序号越界拒绝；重复收块幂等。
    pub fn put_chunk(&mut self, idx: u32) -> Result<(), DlCode> {
        if idx >= self.chunk_count() { return Err(DlCode::ChunkBad); }
        self.chunks[idx as usize] = true;
        Ok(())
    }

    pub fn received_kb(&self) -> u32 {
        self.chunks.iter().filter(|c| **c).count() as u32 * CHUNK_KB
    }

    pub fn complete(&self) -> bool { self.chunks.iter().all(|c| *c) }
}

/// 下载器：三流程 + 续传 + 预切校验。
pub struct Downloader {
    pub sessions: Vec<Session>,
    /// 现役语言（切换基准——先下后切保护对象）。
    pub active_lang: &'static str,
    /// 镜像校验函数注入（测试/实现分离）。
    pub verify_ok: bool,
}

impl Downloader {
    pub fn new(active_lang: &'static str) -> Self {
        Downloader { sessions: Vec::new(), active_lang, verify_ok: false }
    }

    /// 流程一：浏览——列可下载包（lang, size）。
    pub fn browse(&self, catalog: &[(&'static str, u32, u32)]) -> Vec<(&'static str, u32)> {
        catalog.iter().map(|(l, _, sz)| (*l, *sz)).collect()
    }

    /// 流程二：下载——开会话；计量网须先确认；同语种重复会话拒。
    pub fn start(&mut self, lang: &'static str, version: u32, total_kb: u32, metered: bool, user_ok: bool) -> Result<(), DlCode> {
        if metered && !user_ok { return Err(DlCode::MeteredDenied); }
        if self.sessions.iter().any(|s| s.lang == lang && !s.done) { return Err(DlCode::DupSession); }
        let mut s = Session::new(lang, version, total_kb, metered);
        s.user_ok = user_ok;
        self.sessions.push(s);
        Ok(())
    }

    /// 断点续传：找会话继续收块（received 保留）。
    pub fn resume(&mut self, lang: &str, idx: u32) -> Result<(), DlCode> {
        let s = self.sessions.iter_mut().find(|s| s.lang == lang).ok_or(DlCode::NotReady)?;
        s.put_chunk(idx)
    }

    /// 会话完成态推进。
    pub fn finish(&mut self, lang: &str) -> Result<(), DlCode> {
        let s = self.sessions.iter_mut().find(|s| s.lang == lang).ok_or(DlCode::NotReady)?;
        if !s.complete() { return Err(DlCode::NotReady); }
        if !self.verify_ok { return Err(DlCode::VerifyFail); }
        s.done = true;
        Ok(())
    }

    /// 流程三：更新——同语种新版本可再开会话（旧会话必须 done）。
    pub fn update(&mut self, lang: &'static str, new_ver: u32, total_kb: u32) -> Result<(), DlCode> {
        let old_ok = self.sessions.iter().all(|s| s.lang != lang || s.done);
        if !old_ok { return Err(DlCode::DupSession); }
        self.start(lang, new_ver, total_kb, false, true)
    }

    /// 先下后切：只有包 done + 校验过才切现役；任何失败不碰现役语言。
    pub fn switch(&mut self, lang: &'static str) -> Result<(), DlCode> {
        let s = self.sessions.iter().find(|s| s.lang == lang).ok_or(DlCode::NotReady)?;
        if !s.done { return Err(DlCode::NotReady); }
        self.active_lang = lang;
        Ok(())
    }

    /// 下载记录：不含用户画像——只有语种/版本/体积/块数（结构即证明）。取最新会话。
    pub fn record(&self, lang: &str) -> Option<(u32, u32)> {
        self.sessions.iter().rev().find(|s| s.lang == lang).map(|s| (s.version, s.total_kb))
    }
}

/// 校验失败处置：丢弃重下（清位图保会话）+ 立案。
pub struct RetryCase { pub lang: &'static str, pub reason: &'static str }

pub struct RetryBook { pub cases: Vec<RetryCase> }

impl RetryBook {
    pub fn new() -> Self { RetryBook { cases: Vec::new() } }
    pub fn open(&mut self, lang: &'static str, reason: &'static str) { self.cases.push(RetryCase { lang, reason }); }
    /// 丢弃重下：清块位图，计数保留。
    pub fn drop_and_restart(s: &mut Session) { for c in s.chunks.iter_mut() { *c = false; } }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves327_checks() -> CheckSet {
    let mut set = CheckSet::new("ves327");
    let mut dl = Downloader::new("zh");

    let catalog: [(&'static str, u32, u32); 3] = [("ja", 9002, 2900), ("ko", 8800, 2600), ("de", 9001, 800)];

    // C4087-01 浏览三流程之一：目录可见体积。
    let list = dl.browse(&catalog);
    chk(&mut set, "C4087-01", list.len() == 3 && list[0] == ("ja", 2900), "浏览列 3 包体积");

    // C4087-02 计量网未确认 → 拒下载。
    chk(&mut set, "C4087-02", dl.start("ja", 9002, 1024, true, false) == Err(DlCode::MeteredDenied), "计量网未确认 => 拒");

    // C4087-03 用户确认后计量网可下。
    chk(&mut set, "C4087-03", dl.start("ja", 9002, 1024, true, true).is_ok(), "确认后计量网放行");

    // C4087-04 分块数与收块：1024KB = 4 块。
    chk(&mut set, "C4087-04", dl.sessions[0].chunk_count() == 4, "1024KB → 4×256KB");

    // C4087-05 越界块拒。
    chk(&mut set, "C4087-05", dl.resume("ja", 4) == Err(DlCode::ChunkBad), "块 4 越界 => 拒");

    // C4087-06 断点续传：收 2 块中断→继续收余下。
    let _ = dl.resume("ja", 0);
    let _ = dl.resume("ja", 1);
    chk(&mut set, "C4087-06", dl.sessions[0].received_kb() == 512, "续传保留 512KB");

    // C4087-07 未收完不可 done。
    chk(&mut set, "C4087-07", dl.finish("ja") == Err(DlCode::NotReady), "缺 2 块 => NotReady");

    // C4087-08 收完 + 校验失败 → VerifyFail（不破坏现役）。
    let _ = dl.resume("ja", 2);
    let _ = dl.resume("ja", 3);
    dl.verify_ok = false;
    chk(&mut set, "C4087-08", dl.finish("ja") == Err(DlCode::VerifyFail) && dl.active_lang == "zh", "校验失败拒切 现役 zh 不动");

    // C4087-09 丢弃重下 + 立案。
    let mut rb = RetryBook::new();
    if dl.finish("ja") == Err(DlCode::VerifyFail) {
        RetryBook::drop_and_restart(&mut dl.sessions[0]);
        rb.open("ja", "校验失败丢弃重下");
    }
    chk(&mut set, "C4087-09", rb.cases.len() == 1 && dl.sessions[0].received_kb() == 0, "位图清零 + 立案");

    // C4087-10 校验过后切换成功。
    dl.verify_ok = true;
    let _ = dl.finish("ja");
    chk(&mut set, "C4087-10", dl.switch("ja").is_ok() && dl.active_lang == "ja", "先下后切成功");

    // C4087-11 更新流程：旧会话 done 后可开新版本。
    chk(&mut set, "C4087-11", dl.update("ja", 9003, 1024).is_ok() && dl.sessions.len() == 2, "更新开会话 v9003");

    // C4087-12 下载记录无画像：仅 (版本,体积)。
    chk(&mut set, "C4087-12", dl.record("ja") == Some((9003, 1024)), "记录仅版本+体积");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves327_smoke() {
        let set = run_ves327_checks();
        assert!(set.failed_count() == 0, "ves327 red: {}", set.red_items_text());
    }

    #[test]
    fn ves327_chunking() {
        let s = Session::new("de", 1, 1, false); // 1KB → 1 块
        assert_eq!(s.chunk_count(), 1);
        let s2 = Session::new("de", 1, 257, false); // 257KB → 2 块
        assert_eq!(s2.chunk_count(), 2);
    }
}
