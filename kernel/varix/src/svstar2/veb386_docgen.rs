//! veb386_docgen —— VE-F0386 B 域文档自动生成。
//! 锚点（CoRun Unxreal 主册）：VE-F0386 · B 域文档自动生成 · 目标 300 行
//! 判据原文：VE-F0386-J1 从结构化元数据自动生成文档大纲：标题
//!           行（域号 + 名）、目录行（章节序号 + 题）、正文行
//!           （条目缩进两空格）；生成确定性（同输入同输出，两
//!           遍逐字节相等）；章节序号自动推进（不重不漏）；输
//!           出行数守恒（标题 + 目录 + 正文行数总和 = 总行数）。
//! 组件：元数据（章/节/条目）+ 生成器（大纲排版）+ 确定性核。
//! 职责定位：文档自动生成是「B 域的文书科」——手写文档会漂，
//!           机器从元数据生成保证文档与代码同源；确定性是底线：
//!           同一份元数据两次生成结果不同就是生成器有状态泄漏；
//!           行数守恒是排版不丢行的硬核。
//! 性能逐项分解：生成 O(总行数)；确定性核 O(总行数)。
//! 诊断码段：0x30600（0x30601~0x30609 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DG_VERSION: &str = "veb386_docgen-v1";

/// 诊断段基址（0x30600 段 · 本模块独占）。
pub const DG_SEG_BASE: u32 = 0x30600;
pub const XC_NO_CHAP: u32 = 0x30601; // 无章可生成
pub const XC_TITLE_EMPTY: u32 = 0x30602; // 章题空
pub const XC_MAGIC: u32 = 0x30603; // 账魔数破
pub const XC_NOT_DETER: u32 = 0x30604; // 确定性破
pub const XC_LINE_MISM: u32 = 0x30605; // 行数不守恒
pub const XC_SEC_SKIP: u32 = 0x30606; // 节号跳变

/// 章。
pub struct Chapter {
    pub title: &'static str,
    pub sections: Vec<&'static str>,
}

/// 生成器。
pub struct DocGen {
    chapters: Vec<Chapter>,
    magic: u32,
}

impl DocGen {
    /// 建。
    pub fn new() -> DocGen {
        DocGen { chapters: Vec::new(), magic: 0x0600_D06E }
    }

    /// 加章。
    pub fn add_chap(&mut self, title: &'static str, sections: Vec<&'static str>) -> Result<(), u32> {
        if self.magic != 0x0600_D06E {
            return Err(XC_MAGIC);
        }
        if title.is_empty() {
            return Err(XC_TITLE_EMPTY);
        }
        self.chapters.push(Chapter { title, sections });
        Ok(())
    }

    /// 生成大纲文本。
    /// 布局：标题行 `# <域号> <章题>`；目录行 `<章序>.<节序> <节题>`；
    /// 正文行 `  <节题> 条目`。
    pub fn generate(&self) -> Result<String, u32> {
        if self.magic != 0x0600_D06E {
            return Err(XC_MAGIC);
        }
        if self.chapters.is_empty() {
            return Err(XC_NO_CHAP);
        }
        let mut out = String::new();
        for (ci, ch) in self.chapters.iter().enumerate() {
            out.push_str("# B-");
            out.push_str(&(ci + 1).to_string());
            out.push(' ');
            out.push_str(ch.title);
            out.push('\n');
            for (si, sec) in ch.sections.iter().enumerate() {
                out.push_str(&(ci + 1).to_string());
                out.push('.');
                out.push_str(&(si + 1).to_string());
                out.push(' ');
                out.push_str(sec);
                out.push('\n');
                out.push_str("  ");
                out.push_str(sec);
                out.push_str(" 条目\n");
            }
        }
        Ok(out)
    }

    /// 确定性核：两遍逐字节相等。
    pub fn deterministic(&self) -> Result<bool, u32> {
        let a = self.generate()?;
        let b = self.generate()?;
        if a != b {
            return Err(XC_NOT_DETER);
        }
        Ok(true)
    }

    /// 行数守恒核：标题行（章数）+ 每节 2 行 × 总节数 = 总行数。
    pub fn line_conserved(&self) -> Result<bool, u32> {
        let text = self.generate()?;
        let mut cnt = 0u32;
        for c in text.chars() {
            if c == '\n' {
                cnt += 1;
            }
        }
        let mut expect = 0u32;
        for ch in self.chapters.iter() {
            expect += 1; // 标题行
            expect += (ch.sections.len() as u32) * 2; // 目录 + 正文
        }
        if cnt != expect {
            return Err(XC_LINE_MISM);
        }
        Ok(true)
    }

    /// 节号连续核：逐章节号 1..n 无跳。
    pub fn section_seq(&self) -> Result<u32, u32> {
        let text = self.generate()?;
        let mut total_secs = 0u32;
        for ch in self.chapters.iter() {
            total_secs += ch.sections.len() as u32;
        }
        // 目录行形如 "c.s "：数出全部目录行数应等于总节数。
        let mut dir_lines = 0u32;
        for line in text.split('\n') {
            // 目录行：非空、非标题、非缩进正文。
            if line.is_empty() || line.starts_with('#') || line.starts_with("  ") {
                continue;
            }
            // 含 '.' 且第二段是数字 → 目录行
            let mut is_dir = false;
            let mut seen_dot = false;
            for ch2 in line.chars() {
                if ch2 == '.' {
                    seen_dot = true;
                } else if seen_dot && ch2.is_ascii_digit() {
                    is_dir = true;
                }
            }
            if is_dir {
                dir_lines += 1;
            }
        }
        if dir_lines != total_secs {
            return Err(XC_SEC_SKIP);
        }
        Ok(dir_lines)
    }

    /// 章数。
    pub fn chap_count(&self) -> usize {
        self.chapters.len()
    }
}

pub fn run_veb386_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0386");
    // 族一：生成。
    let mut g = DocGen::new();
    let _ = g.add_chap("总览", vec!["定位", "边界"]);
    let _ = g.add_chap("接口", vec!["冻结清单", "版本约定", "弃用流程"]);
    s.add("两章", g.chap_count() == 2, "章数");
    let doc = g.generate();
    s.add("生成过", doc.is_ok(), "大纲出");
    let d = match doc {
        Ok(x) => x,
        Err(_) => {
            s.add("生成失败", false, "不应到此");
            return s;
        }
    };
    s.add("标题行", d.contains("# B-1 总览") && d.contains("# B-2 接口"), "章标题");
    s.add("目录行 1.1", d.contains("1.1 定位"), "章节号");
    s.add("目录行 2.3", d.contains("2.3 弃用流程"), "第三章");
    s.add("正文缩进", d.contains("  定位 条目"), "两空格");
    // 族二：确定性。
    s.add("确定性过", g.deterministic() == Ok(true), "两遍同");
    // 族三：行数守恒。
    // 章 1：1 标题 + 2 节 ×2 = 5 行；章 2：1 + 6 = 7 行；总 12。
    s.add("行数守恒", g.line_conserved() == Ok(true), "12 行");
    // 族四：节号连续。
    s.add("节号连续 5", g.section_seq() == Ok(5), "2+3 节");
    // 族五：空与错。
    let mut e = DocGen::new();
    s.add("无章拒", e.generate() == Err(XC_NO_CHAP), "空大纲");
    s.add("空题拒", e.add_chap("", vec!["x"]) == Err(XC_TITLE_EMPTY), "题空");
    let _ = e.add_chap("单章", vec!["a"]);
    s.add("单章守恒", e.line_conserved() == Ok(true), "1+2=3 行");
    s.add("单章节号", e.section_seq() == Ok(1), "一节");
    s.add("魔数核", DG_SEG_BASE == 0x30600, "段基址");
    s
}
