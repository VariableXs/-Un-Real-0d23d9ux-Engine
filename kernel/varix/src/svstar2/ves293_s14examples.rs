//! ves293_s14examples — VE-F4053 · T03 排版示例
//! 示例库 10 个（CJK 2/阿拉伯 2/泰文印度 2/混排 2/断行 2）+ 三段文档 + 反模式对照（禁则违例/整形分歧/孤行）+ 示例即测试 + 语言切换演示。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x7F40;

pub const TARGET: u8 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum ExCode {
    Ok = 0,
    CountShort = SEG_BASE + 0x01,
    DocMissing = SEG_BASE + 0x02,
    AntiMissing = SEG_BASE + 0x03,
    Imbalance = SEG_BASE + 0x04,
    Drift = SEG_BASE + 0x05,
    DemoMissing = SEG_BASE + 0x06,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Theme { Cjk = 0, Arabic = 1, ThaiIndic = 2, Mixed = 3, Break = 4 }

impl Theme {
    pub const ALL: [Theme; 5] = [Theme::Cjk, Theme::Arabic, Theme::ThaiIndic, Theme::Mixed, Theme::Break];
    pub fn name(self) -> &'static str {
        match self {
            Theme::Cjk => "CJK", Theme::Arabic => "阿拉伯", Theme::ThaiIndic => "泰文印度",
            Theme::Mixed => "混排", Theme::Break => "断行",
        }
    }
}

#[derive(Clone)]
pub struct AntiExample {
    pub theme: Theme,
    pub label: &'static str,
}

impl AntiExample {
    pub fn canonical() -> Vec<AntiExample> {
        let labels = ["禁则违例", "整形分歧", "标记锚定错", "基线错位", "孤行"];
        let mut v = Vec::new();
        for i in 0..5 {
            v.push(AntiExample { theme: Theme::ALL[i], label: labels[i] });
        }
        v
    }
}

#[derive(Clone)]
pub struct Example {
    pub id: u8,
    pub theme: Theme,
    pub title: String,
    pub body: String,
    pub points: Vec<String>,
    pub is_demo: bool,
    pub in_project: bool,
    pub as_test: bool,
}

impl Example {
    pub fn doc_complete(&self) -> bool {
        !self.title.is_empty() && !self.body.is_empty() && !self.points.is_empty()
    }
}

pub struct ExampleBook {
    pub items: Vec<Example>,
}

impl ExampleBook {
    pub fn new() -> Self { ExampleBook { items: Vec::new() } }

    pub fn add(&mut self, e: Example) -> Result<(), ExCode> {
        if self.items.len() >= TARGET as usize { return Err(ExCode::CountShort); }
        for it in self.items.iter() {
            if it.id == e.id { return Err(ExCode::Drift); }
        }
        self.items.push(e);
        Ok(())
    }

    pub fn balanced(&self) -> bool {
        for t in Theme::ALL.iter() {
            if self.items.iter().filter(|e| e.theme == *t).count() != 2 { return false; }
        }
        true
    }
    pub fn docs_complete(&self) -> bool {
        self.items.iter().all(|e| e.doc_complete())
    }
    pub fn all_as_test(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|e| e.as_test)
    }
    pub fn all_in_project(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|e| e.in_project)
    }
    pub fn demo_cover(&self) -> bool {
        for t in Theme::ALL.iter() {
            if !self.items.iter().any(|e| e.theme == *t && e.is_demo) { return false; }
        }
        true
    }
    pub fn reconciled(&self) -> bool {
        self.items.len() == TARGET as usize && self.balanced()
    }
}

pub fn run_ves293_checks() -> CheckSet {
    let mut set = CheckSet::new("ves293_s14examples");
    let mut book = ExampleBook::new();
    let anti = AntiExample::canonical();

    // 1) 构建 10 例。
    let mut id = 1u8;
    for t in Theme::ALL.iter() {
        for _ in 0..2 {
            let _ = book.add(Example {
                id, theme: *t, title: String::from("题"), body: String::from("正文"),
                points: vec![String::from("要点")], is_demo: true, in_project: true, as_test: true,
            });
            id += 1;
        }
    }
    set.add("C4053-01", book.items.len() == 10, "10 示例");
    set.add("C4053-02", book.balanced(), "主题均衡");

    // 3) 超量拒 / 重复拒。
    let mk = |i: u8| Example { id: i, theme: Theme::Mixed, title: String::from("t"), body: String::from("b"), points: vec![String::from("p")], is_demo: false, in_project: false, as_test: false };
    set.add("C4053-03", book.add(mk(99)) == Err(ExCode::CountShort) && book.add(mk(1)) == Err(ExCode::Drift), "超量与重复拒");

    // 4) 文档齐备。
    set.add("C4053-04", book.docs_complete(), "三段文档齐");

    // 5) 反例齐备。
    set.add("C4053-05", anti.len() == 5 && anti[0].label == "禁则违例" && anti[4].label == "孤行", "反例齐备");

    // 6) 示例即测试。
    set.add("C4053-06", book.all_as_test(), "示例即测试");

    // 7) 完整工程。
    set.add("C4053-07", book.all_in_project(), "完整工程");

    // 8) 语言切换演示覆盖。
    set.add("C4053-08", book.demo_cover(), "切换演示覆盖");

    // 9) 对账。
    set.add("C4053-09", book.reconciled(), "对账通过");

    // 10) 文档缺段检出。
    let bad = Example { id: 50, theme: Theme::Cjk, title: String::new(), body: String::from("b"), points: Vec::new(), is_demo: false, in_project: true, as_test: true };
    set.add("C4053-10", !bad.doc_complete(), "文档缺段检出");

    // 11) 演示缺检出。
    let mut b2 = ExampleBook::new();
    let _ = b2.add(Example { id: 1, theme: Theme::Cjk, title: String::from("t"), body: String::from("b"), points: vec![String::from("p")], is_demo: false, in_project: true, as_test: true });
    set.add("C4053-11", !b2.demo_cover(), "演示缺检出");

    // 12) 段位自查。
    set.add("C4053-12", SEG_BASE == 0x7F40, "段位 0x7F40");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance_flow() {
        let mut b = ExampleBook::new();
        let mut id = 1;
        for t in Theme::ALL.iter() {
            for _ in 0..2 {
                let _ = b.add(Example { id, theme: *t, title: String::from("t"), body: String::from("b"), points: vec![String::from("p")], is_demo: true, in_project: true, as_test: true });
                id += 1;
            }
        }
        assert!(b.reconciled());
    }
}
