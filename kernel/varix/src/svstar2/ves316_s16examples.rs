//! ves316_s16examples — VE-F4076 · T04 翻译示例
//! 新语种接入全流程示例 + 常见错误正反例（错译/术语漂移/占位符丢失）+ 示例可执行（跑通即验收）+ 语种缺示例接入阻断 + 虚构语料红线。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x8F40;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum ExCode {
    Ok = 0,
    RunFail = SEG_BASE + 0x01,
    LangMissing = SEG_BASE + 0x02,
    PairMismatch = SEG_BASE + 0x03,
    NotFictional = SEG_BASE + 0x04,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ExampleKind { FullPipeline = 0, GoodBad = 1 }

#[derive(Clone)]
pub struct Example {
    pub id: u8,
    pub kind: ExampleKind,
    pub lang: &'static str,
    pub title: String,
    pub executable: bool,
    pub fictional: bool,
    pub run_ok: bool,
}

pub struct ExampleBook {
    pub items: Vec<Example>,
    /// 每个接入语种必须至少一全流程示例。
    pub onboard_langs: Vec<&'static str>,
}

impl ExampleBook {
    pub fn new() -> Self { ExampleBook { items: Vec::new(), onboard_langs: Vec::new() } }

    pub fn add(&mut self, e: Example) -> Result<(), ExCode> {
        if !e.fictional { return Err(ExCode::NotFictional); }
        self.items.push(e);
        Ok(())
    }

    /// 语种缺全流程示例 → 接入阻断。
    pub fn blocked_langs(&self) -> Vec<&'static str> {
        let mut v = Vec::new();
        for l in self.onboard_langs.iter() {
            if !self.items.iter().any(|e| e.lang == *l && e.kind == ExampleKind::FullPipeline) {
                v.push(*l);
            }
        }
        v
    }

    /// 可执行验收：全部示例标记可执行且跑通。
    pub fn all_verified(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|e| e.executable && e.run_ok)
    }

    /// 示例跑挂 → 示例即缺陷（立案清单）。
    pub fn defects(&self) -> Vec<u8> {
        let mut v = Vec::new();
        for e in self.items.iter() {
            if e.executable && !e.run_ok { v.push(e.id); }
        }
        v
    }
}

/// 正反例对：错误类型 ↔ 正确示范。
#[derive(Clone)]
pub struct GoodBadPair {
    pub err_kind: &'static str,
    pub bad: String,
    pub good: String,
}

impl GoodBadPair {
    pub fn canonical() -> Vec<GoodBadPair> {
        vec![
            GoodBadPair { err_kind: "错译", bad: String::from("把 Save 译成 储存上载"), good: String::from("把 Save 译成 保存") },
            GoodBadPair { err_kind: "术语漂移", bad: String::from("file 一会儿译 文件 一会儿译 档案"), good: String::from("file 全库统一译 文件") },
            GoodBadPair { err_kind: "占位符丢失", bad: String::from("Hi {name} → 你好"), good: String::from("Hi {name} → 你好 {name}") },
        ]
    }
    pub fn pair_ok(&self) -> bool {
        !self.bad.is_empty() && !self.good.is_empty() && self.bad != self.good
    }
}

pub fn run_ves316_checks() -> CheckSet {
    let mut set = CheckSet::new("ves316_s16examples");
    let mut eb = ExampleBook::new();
    eb.onboard_langs = vec!["ko", "ar"];

    // 1) 非虚构语料拒。
    set.add("C4076-01", eb.add(Example { id: 1, kind: ExampleKind::FullPipeline, lang: "ko", title: String::from("t"), executable: true, fictional: false, run_ok: true }) == Err(ExCode::NotFictional), "非虚构拒");

    // 2) 韩语全流程示例入库。
    let _ = eb.add(Example { id: 1, kind: ExampleKind::FullPipeline, lang: "ko", title: String::from("韩语接入全流程"), executable: true, fictional: true, run_ok: true });
    set.add("C4076-02", eb.blocked_langs() == ["ar"], "接入阻断");

    // 3) 阿语补齐后放行。
    let _ = eb.add(Example { id: 2, kind: ExampleKind::FullPipeline, lang: "ar", title: String::from("阿语接入全流程"), executable: true, fictional: true, run_ok: true });
    set.add("C4076-03", eb.blocked_langs().is_empty(), "接入放行");

    // 4) 示例跑挂立案。
    let _ = eb.add(Example { id: 3, kind: ExampleKind::GoodBad, lang: "ko", title: String::from("坏例"), executable: true, fictional: true, run_ok: false });
    set.add("C4076-04", eb.defects() == [3], "跑挂立案");

    // 5) 可执行验收。
    set.add("C4076-05", !eb.all_verified(), "跑挂不验收");

    // 6) 正反例对齐。
    let pairs = GoodBadPair::canonical();
    set.add("C4076-06", pairs.len() == 3 && pairs.iter().all(|p| p.pair_ok()), "正反例对");

    // 7) 三类错误覆盖。
    set.add("C4076-07", pairs[0].err_kind == "错译" && pairs[1].err_kind == "术语漂移"
        && pairs[2].err_kind == "占位符丢失", "三类错误");

    // 8) 修复后验收。
    eb.items[2].run_ok = true;
    set.add("C4076-08", eb.all_verified(), "修复验收");

    // 9) 段位自查。
    set.add("C4076-09", SEG_BASE == 0x8F40, "段位 0x8F40");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_flow() {
        let mut eb = ExampleBook::new();
        eb.onboard_langs = vec!["de"];
        assert_eq!(eb.blocked_langs(), ["de"]);
        let _ = eb.add(Example { id: 1, kind: ExampleKind::FullPipeline, lang: "de", title: String::from("t"), executable: true, fictional: true, run_ok: true });
        assert!(eb.blocked_langs().is_empty());
    }
}
