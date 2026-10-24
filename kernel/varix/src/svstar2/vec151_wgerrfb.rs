//! VE-F0551 · WGSL 错误回传
//!
//! WGSL 校验错误回传：生态校验错误解析（错误文本格式登记解析器）、
//! 位置到 IR 到源码回溯（F0508 机制同构复用）、错误分类与规范版本关联
//! （规范更新导致的新失败单独维度）。
//!
//! 数据结构：错误解析器；回溯链复用；规范版本关联维。
//! 错误路径：解析格式变更→适配登记；回溯断链→标注补映射；规范更新引发→批量评估流程。
//! 性能：解析 O(输出量)；回溯 O(1)；关联 O(1)。
//! 跨批对接：上游 F0508 机制复用、F0476 装饰；下游 F0552 套件联动。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 校验器种类（WGSL 生态双前端）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Validator {
    /// tint 生态前端
    Tint,
    /// naga 生态前端
    Naga,
}

impl Validator {
    pub fn name(self) -> &'static str {
        match self {
            Validator::Tint => "tint",
            Validator::Naga => "naga",
        }
    }
}

/// 解析后的单条 WGSL 错误
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct WgError {
    /// 校验器来源
    pub source: &'static str,
    /// 原始行号（校验器侧，1 起）
    pub line: u32,
    /// 原始列号
    pub col: u32,
    /// 错误正文
    pub text: String,
    /// 错误类别
    pub class: ErrClass,
}

/// 错误类别（三分类）
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrClass {
    /// 语法类（文法不合法）
    Syntax,
    /// 语义类（类型/作用域/一致性不满足）
    Semantic,
    /// 规范版本类（新版本规范导致的新失败）
    SpecVersion,
}

/// 错误文本格式登记解析器
///
/// 每种校验器的错误文本格式登记在案；格式变更走适配登记，
/// 不改编析器主体。
pub struct ErrParser {
    /// 登记的格式版本号（每校验器一条）
    format_versions: [(Validator, u32); 2],
}

impl ErrParser {
    pub fn new() -> Self {
        ErrParser {
            format_versions: [(Validator::Tint, 1), (Validator::Naga, 1)],
        }
    }

    pub fn format_version(&self, v: Validator) -> u32 {
        self.format_versions.iter().find(|e| e.0 == v).map(|e| e.1).unwrap_or(0)
    }

    /// 格式变更适配登记： bump 版本号并返回登记凭据
    pub fn adapt_format(&mut self, v: Validator) -> u32 {
        for e in self.format_versions.iter_mut() {
            if e.0 == v {
                e.1 += 1;
                return e.1;
            }
        }
        0
    }

    /// 解析一行错误文本（登记格式：`line:col: text`）
    pub fn parse_line(&self, v: Validator, raw: &str) -> Option<WgError> {
        let head_end = raw.find(": ")?;
        let pos_part = &raw[..head_end];
        let text = raw[head_end + 2..].to_string();
        let mut it = pos_part.split(':');
        let line: u32 = it.next()?.parse().ok()?;
        let col: u32 = it.next()?.parse().ok()?;
        let class = classify(&text);
        Some(WgError {
            source: v.name(),
            line,
            col,
            text,
            class,
        })
    }

    /// 批量解析：多行文本 → 错误列表
    pub fn parse_all(&self, v: Validator, raw: &str) -> Vec<WgError> {
        raw.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| self.parse_line(v, l)).collect()
    }
}

/// 文本粗分类：规范版本类关键词单独维度
pub fn classify(text: &str) -> ErrClass {
    if text.contains("extension") || text.contains("not allowed in version") || text.contains("spec") {
        ErrClass::SpecVersion
    } else if text.contains("expected") || text.contains("token") || text.contains("unterminated") {
        ErrClass::Syntax
    } else {
        ErrClass::Semantic
    }
}

/// 回溯链：源码 → WGSL 文本 → IR → 源码（F0508 机制同构复用）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct BacktraceLink {
    /// WGSL 生成行 → IR 语句序号
    pub wgsl_to_ir: Vec<Option<u32>>,
    /// IR 语句序号 → 源码行号
    pub ir_to_src: Vec<Option<u32>>,
}

impl BacktraceLink {
    pub fn build(wgsl_to_ir: Vec<Option<u32>>, ir_to_src: Vec<Option<u32>>) -> Self {
        BacktraceLink { wgsl_to_ir, ir_to_src }
    }

    /// O(1) 回溯：WGSL 行 → 源码行；断链返回 None
    pub fn wgsl_to_src(&self, wgsl_line: u32) -> Option<u32> {
        let ir = *self.wgsl_to_ir.get(wgsl_line as usize)?;
        let ir = ir?;
        *self.ir_to_src.get(ir as usize)?
    }

    /// 断链标注：回溯失败的行为标注补映射留痕
    pub fn broken_lines(&self) -> Vec<u32> {
        let mut out = Vec::new();
        for (i, e) in self.wgsl_to_ir.iter().enumerate() {
            let broken = match e {
                None => true,
                Some(ir) => match self.ir_to_src.get(*ir as usize) {
                    None | Some(None) => true,
                    Some(Some(_)) => false,
                },
            };
            if broken {
                out.push(i as u32);
            }
        }
        out
    }

    /// 标注补映射：为断链行登记补映射（回填 wgsl→ir 侧）
    pub fn patch(&mut self, wgsl_line: u32, ir: u32, src: u32) -> bool {
        let n = self.wgsl_to_ir.len();
        if wgsl_line as usize >= n {
            return false;
        }
        self.wgsl_to_ir[wgsl_line as usize] = Some(ir);
        if src > 0 {
            let m = self.ir_to_src.len();
            if (ir as usize) < m {
                self.ir_to_src[ir as usize] = Some(src);
            }
        }
        true
    }
}

/// 规范版本关联维：规范更新导致的新失败单独登记
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SpecAssoc {
    /// 规范版本号（关联失败时的版本）
    pub spec_ver: u32,
    /// 由该版本更新引发的新失败数
    pub new_failures: u32,
}

impl SpecAssoc {
    pub fn new(spec_ver: u32) -> Self {
        SpecAssoc { spec_ver, new_failures: 0 }
    }

    /// 关联判定：错误文本命中规范更新关键词且历史版本可通过 → 计入新失败
    pub fn associate(&mut self, e: &WgError, passed_before: bool) -> bool {
        let hit = e.class == ErrClass::SpecVersion;
        if hit && passed_before {
            self.new_failures += 1;
            true
        } else {
            false
        }
    }

    /// 批量评估流程：新失败超过阈值即触发批量评估
    pub fn needs_review(&self, threshold: u32) -> bool {
        self.new_failures >= threshold
    }
}

pub fn run_vec151_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec151");
    // 族一：错误解析器
    let p = ErrParser::new();
    let e1 = p.parse_line(Validator::Tint, "12:5: expected ';' after statement");
    s.add("解析-行号列号", e1.as_ref().map(|e| e.line == 12 && e.col == 5).unwrap_or(false), "12:5");
    s.add("解析-语法分类", e1.as_ref().map(|e| e.class == ErrClass::Syntax).unwrap_or(false), "cls");
    let e2 = p.parse_line(Validator::Naga, "3:1: type mismatch in uniform binding");
    s.add("解析-语义分类", e2.as_ref().map(|e| e.class == ErrClass::Semantic).unwrap_or(false), "cls2");
    let e3 = p.parse_line(Validator::Tint, "1:1: feature not allowed in version 1.0 spec");
    s.add("解析-规范版本分类", e3.as_ref().map(|e| e.class == ErrClass::SpecVersion).unwrap_or(false), "cls3");
    s.add("解析-非法行拒绝", p.parse_line(Validator::Tint, "garbage line").is_none(), "reject");
    s.add("解析-批量条数", p.parse_all(Validator::Tint, "1:1: a\n2:2: b\n\n3:3: c").len() == 3, "batch3");
    s.add("格式版本初值", p.format_version(Validator::Tint) == 1 && p.format_version(Validator::Naga) == 1, "ver1");
    // 族二：格式变更适配登记
    let mut p2 = ErrParser::new();
    let v1 = p2.adapt_format(Validator::Tint);
    s.add("适配-版本递增", v1 == 2 && p2.format_version(Validator::Tint) == 2, "bump");
    s.add("适配-他器不受影响", p2.format_version(Validator::Naga) == 1, "iso");
    // 族三：回溯链复用（F0508 同构）
    let bt = BacktraceLink::build(vec![Some(0), Some(1), None, Some(2)], vec![Some(10), Some(11), Some(12)]);
    s.add("回溯-直连命中", bt.wgsl_to_src(0) == Some(10), "hit0");
    s.add("回溯-级联命中", bt.wgsl_to_src(3) == Some(12), "hit3");
    s.add("回溯-断链返回None", bt.wgsl_to_src(2).is_none(), "none");
    s.add("回溯-越界返回None", bt.wgsl_to_src(99).is_none(), "oob");
    s.add("断链-清单定位", bt.broken_lines() == vec![2], "broken");
    // 族四：断链标注补映射
    let mut bt2 = BacktraceLink::build(vec![Some(0), None], vec![Some(10), None]);
    s.add("补映射-登记成功", bt2.patch(1, 1, 11), "patch");
    s.add("补映射-补后可回溯", bt2.wgsl_to_src(1) == Some(11), "after");
    s.add("补映射-越界拒绝", !bt2.patch(9, 0, 0), "patch-oob");
    // 族五：规范版本关联维
    let mut sa = SpecAssoc::new(42);
    let ok_err = WgError { source: "tint", line: 1, col: 1, text: "x".to_string(), class: ErrClass::Semantic };
    s.add("关联-非规范类不计数", !sa.associate(&ok_err, true) && sa.new_failures == 0, "no-count");
    let spec_err = WgError { source: "tint", line: 1, col: 1, text: "spec".to_string(), class: ErrClass::SpecVersion };
    s.add("关联-历史通过才计数", sa.associate(&spec_err, true) && sa.new_failures == 1, "count");
    s.add("关联-历史未通过不计数", !sa.associate(&spec_err, false) && sa.new_failures == 1, "no-old");
    s.add("批量评估-阈值触发", sa.needs_review(1) && !sa.needs_review(2), "review");
    s
}
