//! ves314_s16debug — VE-F4074 · T04 翻译调试
//! 键翻译链路追踪（键→五段流水线当前状态+历史）+ 译文差异对拍器（同键多来源冲突定位+采纳序建议）+ 伪本地化即时预览（失败降级键名模式）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8E40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum DbgCode {
    Ok = 0,
    LinkBroken = SEG_BASE + 0x01,
    ConflictFound = SEG_BASE + 0x02,
    PreviewFail = SEG_BASE + 0x03,
}

/// 链路事件。
#[derive(Clone)]
pub struct LinkEvent {
    pub stage: &'static str,
    pub note: String,
}

pub struct LinkTracer {
    pub links: Vec<(String, Vec<LinkEvent>)>,
}

impl LinkTracer {
    pub fn new() -> Self { LinkTracer { links: Vec::new() } }

    pub fn trace(&mut self, key: &str, stage: &'static str, note: &str) {
        for l in self.links.iter_mut() {
            if l.0 == key {
                l.1.push(LinkEvent { stage, note: String::from(note) });
                return;
            }
        }
        self.links.push((String::from(key), vec![LinkEvent { stage, note: String::from(note) }]));
    }

    /// 当前状态：链路最后一站。
    pub fn current(&self, key: &str) -> Result<&'static str, DbgCode> {
        for l in self.links.iter() {
            if l.0 == key {
                return l.1.last().map(|e| e.stage).ok_or(DbgCode::LinkBroken);
            }
        }
        Err(DbgCode::LinkBroken)
    }

    /// 链路断点：缺失段清单（五段齐才完整）。
    pub fn missing_stages(&self, key: &str) -> Vec<&'static str> {
        const ALL: [&str; 5] = ["抽取", "翻译", "评审", "合入", "回归"];
        let mut have: Vec<&str> = Vec::new();
        for l in self.links.iter() {
            if l.0 == key {
                for e in l.1.iter() { have.push(e.stage); }
            }
        }
        let mut miss = Vec::new();
        for s in ALL.iter() {
            if !have.contains(s) { miss.push(*s); }
        }
        miss
    }
}

/// 译文差异对拍器：同键多来源。
#[derive(Clone)]
pub struct SourceVer {
    pub origin: &'static str,
    pub text: String,
    pub rank: u8, // 建议采纳序（小者优先）
}

pub struct DiffArbiter {
    pub by_key: Vec<(String, Vec<SourceVer>)>,
}

impl DiffArbiter {
    pub fn new() -> Self { DiffArbiter { by_key: Vec::new() } }

    pub fn add(&mut self, key: &str, origin: &'static str, text: &str, rank: u8) {
        for k in self.by_key.iter_mut() {
            if k.0 == key {
                k.1.push(SourceVer { origin, text: String::from(text), rank });
                return;
            }
        }
        self.by_key.push((String::from(key), vec![SourceVer { origin, text: String::from(text), rank }]));
    }

    /// 冲突检出：同键两来源文本不一致。
    pub fn conflicts(&self, key: &str) -> bool {
        for k in self.by_key.iter() {
            if k.0 == key {
                if k.1.len() < 2 { return false; }
                let first = &k.1[0].text;
                return k.1.iter().any(|v| &v.text != first);
            }
        }
        false
    }

    /// 建议采纳：rank 最小者。
    pub fn suggested(&self, key: &str) -> Result<&str, DbgCode> {
        for k in self.by_key.iter() {
            if k.0 == key {
                let mut best: Option<&SourceVer> = None;
                for v in k.1.iter() {
                    match best {
                        None => best = Some(v),
                        Some(b) if v.rank < b.rank => best = Some(v),
                        _ => {}
                    }
                }
                return best.map(|v| v.text.as_str()).ok_or(DbgCode::ConflictFound);
            }
        }
        Err(DbgCode::ConflictFound)
    }
}

/// 伪本地化即时预览（失败降级键名模式）。
pub fn preview(pseudo: Option<&str>, key: &str) -> String {
    match pseudo {
        Some(t) => String::from(t),
        None => String::from(key), // 降级键名模式
    }
}

pub fn run_ves314_checks() -> CheckSet {
    let mut set = CheckSet::new("ves314_s16debug");
    let mut lt = LinkTracer::new();

    // 1) 链路追踪。
    lt.trace("btn.ok", "抽取", "源码扫描");
    lt.trace("btn.ok", "翻译", "TM 命中");
    set.add("C4074-01", lt.current("btn.ok") == Ok("翻译"), "链路追踪");

    // 2) 查无键断链显性。
    set.add("C4074-02", lt.current("ghost") == Err(DbgCode::LinkBroken), "断链显性");

    // 3) 缺段清单。
    let miss = lt.missing_stages("btn.ok");
    set.add("C4074-03", miss == ["评审", "合入", "回归"], "缺段清单");

    // 4) 五段齐后无缺。
    lt.trace("btn.ok", "评审", "LQA 过");
    lt.trace("btn.ok", "合入", "合入主库");
    lt.trace("btn.ok", "回归", "回归绿");
    set.add("C4074-04", lt.missing_stages("btn.ok").is_empty(), "五段齐");

    // 5) 对拍器冲突检出。
    let mut da = DiffArbiter::new();
    da.add("btn.ok", "tm", "保存", 2);
    da.add("btn.ok", "mt", "储存", 3);
    set.add("C4074-05", da.conflicts("btn.ok"), "冲突检出");

    // 6) 无冲突。
    da.add("btn.save", "tm", "保存文件", 1);
    da.add("btn.save", "mt", "保存文件", 2);
    set.add("C4074-06", !da.conflicts("btn.save"), "无冲突");

    // 7) 建议采纳序。
    set.add("C4074-07", da.suggested("btn.ok") == Ok("保存"), "采纳序");

    // 8) 查无键拒。
    set.add("C4074-08", da.suggested("none") == Err(DbgCode::ConflictFound), "查无拒");

    // 9) 伪本地化预览。
    set.add("C4074-09", preview(Some("äpplë"), "k") == "äpplë", "伪本地化预览");

    // 10) 预览失败降级键名。
    set.add("C4074-10", preview(None, "btn.ok") == "btn.ok", "降级键名");

    // 11) 段位自查。
    set.add("C4074-11", SEG_BASE == 0x8E40, "段位 0x8E40");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trace_flow() {
        let mut lt = LinkTracer::new();
        lt.trace("k", "抽取", "n");
        assert_eq!(lt.current("k"), Ok("抽取"));
        assert_eq!(lt.current("x"), Err(DbgCode::LinkBroken));
    }
}
