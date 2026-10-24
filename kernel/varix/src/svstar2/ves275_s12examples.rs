//! ves275_s12examples — VE-F4035 · RTL 示例
//! 示例库 10 个（BiDi 基础 2/布局镜像 2/混合文本 2/控件镜像 2/输入法 2）+ 三段文档 + 反模式对照（每主题一反例）+ 示例即测试 + 完整工程 + 方向切换演示。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x6FC0;

pub const EXAMPLE_TARGET: u8 = 10;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u16)]
pub enum ExCode {
    Ok = 0,
    CountShort = SEG_BASE + 0x01,
    DocMissing = SEG_BASE + 0x02,
    AntiMissing = SEG_BASE + 0x03,
    ThemeImbalance = SEG_BASE + 0x04,
    Drift = SEG_BASE + 0x05,
    DemoMissing = SEG_BASE + 0x06,
    ProjIncomplete = SEG_BASE + 0x07,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Theme { BidiBasic = 0, LayoutMirror = 1, MixedText = 2, CtrlMirror = 3, Ime = 4 }

impl Theme {
    pub const ALL: [Theme; 5] = [Theme::BidiBasic, Theme::LayoutMirror, Theme::MixedText, Theme::CtrlMirror, Theme::Ime];
    pub fn name(self) -> &'static str {
        match self {
            Theme::BidiBasic => "BiDi 基础", Theme::LayoutMirror => "布局镜像",
            Theme::MixedText => "混合文本", Theme::CtrlMirror => "控件镜像", Theme::Ime => "输入法",
        }
    }
}

/// 反模式对照：每主题一反例（简化 BiDi/误镜像/表头错位）。
#[derive(Clone)]
pub struct AntiExample {
    pub theme: Theme,
    pub label: String,
}

impl AntiExample {
    pub fn canonical() -> Vec<AntiExample> {
        let mut v = Vec::new();
        let labels = ["简化 BiDi", "误镜像", "混向错排", "表头错位", "IME 锚错"];
        for i in 0..5 {
            v.push(AntiExample { theme: Theme::ALL[i], label: String::from(labels[i]) });
        }
        v
    }
}

/// 示例条目：三段文档（标题/正文/要点）+ 所属主题 + 反例引用 + 演示与工程标记。
#[derive(Clone)]
pub struct Example {
    pub id: u8,
    pub theme: Theme,
    pub doc_title: String,
    pub doc_body: String,
    pub doc_points: Vec<String>,
    pub anti_ref: Option<u8>,
    pub is_demo: bool,
    pub in_project: bool,
    pub as_test: bool,
}

impl Example {
    pub fn doc_complete(&self) -> bool {
        !self.doc_title.is_empty() && !self.doc_body.is_empty() && !self.doc_points.is_empty()
    }
}

pub struct ExampleBook {
    pub items: Vec<Example>,
}

impl ExampleBook {
    pub fn new() -> Self { ExampleBook { items: Vec::new() } }

    pub fn add(&mut self, e: Example) -> Result<(), ExCode> {
        if self.items.len() >= EXAMPLE_TARGET as usize { return Err(ExCode::CountShort); }
        for it in self.items.iter() {
            if it.id == e.id { return Err(ExCode::Drift); }
        }
        self.items.push(e);
        Ok(())
    }

    /// 主题均衡：每主题恰好 2 例。
    pub fn theme_balanced(&self) -> bool {
        for t in Theme::ALL.iter() {
            let n = self.items.iter().filter(|e| e.theme == *t).count();
            if n != 2 { return false; }
        }
        true
    }

    /// 全文档齐备。
    pub fn docs_complete(&self) -> bool {
        self.items.iter().all(|e| e.doc_complete())
    }

    /// 反例齐备：五主题反例全在册。
    pub fn anti_complete(&self, anti: &[AntiExample]) -> bool {
        for t in Theme::ALL.iter() {
            if !anti.iter().any(|a| a.theme == *t) { return false; }
        }
        true
    }

    /// 示例即测试：全部示例标记为测试。
    pub fn all_as_test(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|e| e.as_test)
    }

    /// 完整工程：全部示例入工程。
    pub fn all_in_project(&self) -> bool {
        !self.items.is_empty() && self.items.iter().all(|e| e.in_project)
    }

    /// 方向切换演示：每主题至少一演示。
    pub fn demo_cover(&self) -> bool {
        for t in Theme::ALL.iter() {
            if !self.items.iter().any(|e| e.theme == *t && e.is_demo) { return false; }
        }
        true
    }

    /// 对账：注册数与目标一致。
    pub fn reconciled(&self) -> bool {
        self.items.len() == EXAMPLE_TARGET as usize && self.theme_balanced()
    }
}

pub fn run_ves275_checks() -> CheckSet {
    let mut set = CheckSet::new("ves275_s12examples");
    let mut book = ExampleBook::new();
    let anti = AntiExample::canonical();

    // 1) 构建 10 例（五主题各 2）。
    let mut id = 1u8;
    for t in Theme::ALL.iter() {
        for _ in 0..2 {
            let _ = book.add(Example {
                id,
                theme: *t,
                doc_title: String::from("题"),
                doc_body: String::from("正文"),
                doc_points: vec![String::from("要点")],
                anti_ref: None,
                is_demo: true,
                in_project: true,
                as_test: true,
            });
            id += 1;
        }
    }
    set.add("C4035-01", book.items.len() == 10, "10 示例");
    set.add("C4035-02", book.theme_balanced(), "主题均衡");

    // 3) 超目标拒。
    let extra = Example {
        id: 99, theme: Theme::Ime, doc_title: String::from("x"), doc_body: String::from("y"),
        doc_points: vec![String::from("z")], anti_ref: None, is_demo: false, in_project: false, as_test: false,
    };
    set.add("C4035-03", book.add(extra) == Err(ExCode::CountShort), "超目标拒");

    // 4) 重复 id 拒。
    let dup = Example {
        id: 1, theme: Theme::Ime, doc_title: String::from("x"), doc_body: String::from("y"),
        doc_points: vec![String::from("z")], anti_ref: None, is_demo: false, in_project: false, as_test: false,
    };
    set.add("C4035-04", book.add(dup) == Err(ExCode::Drift), "重复 id 拒");

    // 5) 文档齐备。
    set.add("C4035-05", book.docs_complete(), "三段文档齐");

    // 6) 反例齐备（五主题）。
    set.add("C4035-06", book.anti_complete(&anti) && anti.len() == 5, "反例齐备");

    // 7) 反例标注：主题映射正确。
    set.add("C4035-07", anti[0].theme == Theme::BidiBasic && anti[3].theme == Theme::CtrlMirror, "反例主题映射");

    // 8) 示例即测试。
    set.add("C4035-08", book.all_as_test(), "示例即测试");

    // 9) 完整工程。
    set.add("C4035-09", book.all_in_project(), "完整工程");

    // 10) 方向切换演示覆盖。
    set.add("C4035-10", book.demo_cover(), "方向演示覆盖");

    // 11) 演示缺失检出。
    let mut book2 = ExampleBook::new();
    let _ = book2.add(Example {
        id: 1, theme: Theme::BidiBasic, doc_title: String::from("t"), doc_body: String::from("b"),
        doc_points: vec![String::from("p")], anti_ref: None, is_demo: false, in_project: true, as_test: true,
    });
    set.add("C4035-11", !book2.demo_cover(), "演示缺检出");

    // 12) 对账。
    set.add("C4035-12", book.reconciled(), "对账通过");

    // 13) 文档缺段检出。
    let mut bad = Example {
        id: 2, theme: Theme::Ime, doc_title: String::new(), doc_body: String::from("b"),
        doc_points: Vec::new(), anti_ref: None, is_demo: false, in_project: true, as_test: true,
    };
    set.add("C4035-13", !bad.doc_complete(), "文档缺段检出");
    bad.doc_title = String::from("t");
    set.add("C4035-14", !bad.doc_complete(), "要点缺检出");

    // 15) 反例标签非空。
    set.add("C4035-15", anti.iter().all(|a| !a.label.is_empty()), "反例标签非空");

    // 16) 段位自查。
    set.add("C4035-16", SEG_BASE == 0x6FC0, "段位 0x6FC0");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balance() {
        let mut b = ExampleBook::new();
        let mut id = 1;
        for t in Theme::ALL.iter() {
            for _ in 0..2 {
                let _ = b.add(Example {
                    id, theme: *t, doc_title: String::from("t"), doc_body: String::from("b"),
                    doc_points: vec![String::from("p")], anti_ref: None, is_demo: true,
                    in_project: true, as_test: true,
                });
                id += 1;
            }
        }
        assert!(b.reconciled());
    }
}
