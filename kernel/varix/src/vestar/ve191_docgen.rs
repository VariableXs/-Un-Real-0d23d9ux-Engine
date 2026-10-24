//! ve191_docgen ↔ VE-F0191 域接口文档自动生成
//!
//! 从冻结清单（F0187）与能力矩阵（F0181）自动生成接口文档：签名、语义、错误码、
//! 使用示例四要素模板化输出；文档与实现一致性由生成机制保证（源头单一），
//! 人工注释区显性隔离。
//!
//! 判据：同源生成、四要素模板、注释隔离、示例验证。

use alloc::format;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 文档四要素模板。
#[derive(Debug, Clone)]
pub struct DocTemplate {
    /// 接口签名。
    pub signature: String,
    /// 语义描述。
    pub semantics: String,
    /// 错误码列表。
    pub error_codes: Vec<u16>,
    /// 使用示例。
    pub example: String,
}

/// 生成的文档条目。
#[derive(Debug, Clone)]
pub struct DocEntry {
    /// 接口名称。
    pub name: String,
    /// 模板内容。
    pub template: DocTemplate,
    /// 人工注释区（显性隔离，不影响自动生成内容）。
    pub human_notes: String,
    /// 生成版本。
    pub gen_version: u64,
}

/// 文档生成错误码。
pub mod err {
    /// 源头缺失。
    pub const E_MISSING_SOURCE: u16 = 0x8401;
    /// 注释区越界（试图改接口描述）。
    pub const E_NOTE_OVERREACH: u16 = 0x8402;
    /// 版本不匹配。
    pub const E_VERSION_MISMATCH: u16 = 0x8403;
    /// 接口名重复。
    pub const E_DUPLICATE: u16 = 0x8404;
}

/// 文档自动生成器。
pub struct DocGenerator {
    /// 文档条目表。
    entries: BTreeMap<String, DocEntry>,
    /// 生成版本号。
    version: u64,
    /// 注释区标记前缀。
    pub note_marker: String,
}

impl DocGenerator {
    /// 新建生成器。
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            version: 1,
            note_marker: "// ---- 人工注释区 ----".to_string(),
        }
    }

    /// 从源头生成文档（同源生成：签名/语义/错误码/示例四要素）。
    pub fn generate(
        &mut self,
        name: &str,
        signature: &str,
        semantics: &str,
        error_codes: &[u16],
        example: &str,
    ) -> Result<&str, u16> {
        // 源头缺失 → 生成失败显性报错不产空文档
        if name.is_empty() || signature.is_empty() || semantics.is_empty() {
            return Err(err::E_MISSING_SOURCE);
        }
        if self.entries.contains_key(name) {
            return Err(err::E_DUPLICATE);
        }

        let template = DocTemplate {
            signature: signature.to_string(),
            semantics: semantics.to_string(),
            error_codes: error_codes.to_vec(),
            example: example.to_string(),
        };

        let entry = DocEntry {
            name: name.to_string(),
            template,
            human_notes: String::new(),
            gen_version: self.version,
        };

        self.entries.insert(name.to_string(), entry);
        self.version += 1;
        self.entries
            .get(name)
            .map(|e| e.template.example.as_str())
            .ok_or(err::E_MISSING_SOURCE)
    }

    /// 添加人工注释（注释区隔离：不允许改接口描述字段）。
    pub fn add_human_note(&mut self, name: &str, note: &str) -> Result<(), u16> {
        let entry = self.entries.get_mut(name).ok_or(err::E_MISSING_SOURCE)?;
        // 注释区越界校验：注释内容不能包含签名/语义关键词（防止伪装改接口描述）
        if note.contains("fn ") || note.contains("pub fn") {
            return Err(err::E_NOTE_OVERREACH);
        }
        entry.human_notes = note.to_string();
        Ok(())
    }

    /// 重新生成（版本不匹配时提示重生成）。
    pub fn regenerate(&mut self, name: &str) -> Result<u64, u16> {
        let entry = self.entries.get_mut(name).ok_or(err::E_MISSING_SOURCE)?;
        entry.gen_version = self.version;
        self.version += 1;
        Ok(entry.gen_version)
    }

    /// 查询文档条目。
    pub fn get(&self, name: &str) -> Option<&DocEntry> {
        self.entries.get(name)
    }

    /// 渲染文档文本（四要素模板化输出）。
    pub fn render(&self, name: &str) -> Option<String> {
        self.entries.get(name).map(|e| {
            let mut out = String::new();
            out.push_str(&alloc::format!("# {}\n\n", e.name));
            out.push_str(&alloc::format!("## 签名\n{}\n\n", e.template.signature));
            out.push_str(&alloc::format!("## 语义\n{}\n\n", e.template.semantics));
            out.push_str("## 错误码\n");
            for code in &e.template.error_codes {
                out.push_str(&alloc::format!("- 0x{:04X}\n", code));
            }
            out.push('\n');
            out.push_str(&alloc::format!("## 使用示例\n{}\n\n", e.template.example));
            if !e.human_notes.is_empty() {
                out.push_str(&self.note_marker);
                out.push('\n');
                out.push_str(&e.human_notes);
                out.push('\n');
            }
            out
        })
    }

    /// 全量文档条目。
    pub fn all(&self) -> Vec<&DocEntry> {
        self.entries.values().collect()
    }

    /// 一致性检查：所有条目四要素非空。
    pub fn verify_completeness(&self) -> Vec<String> {
        self.entries
            .values()
            .filter(|e| {
                e.template.signature.is_empty()
                    || e.template.semantics.is_empty()
                    || e.template.example.is_empty()
            })
            .map(|e| alloc::format!("条目[{}]四要素不全", e.name))
            .collect()
    }

    /// 生成版本号。
    pub fn version(&self) -> u64 {
        self.version
    }

    /// 条目总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for DocGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_four_elements() {
        let mut g = DocGenerator::new();
        g.generate(
            "present_surface",
            "fn present(s: &Surface) -> Result<(), Error>",
            "呈现表面到屏幕",
            &[0x8301, 0x8302],
            "present_surface(&surface)?;",
        )
        .unwrap();
        assert_eq!(g.len(), 1);
        let doc = g.render("present_surface").unwrap();
        assert!(doc.contains("present_surface"));
        assert!(doc.contains("0x8301"));
        assert!(doc.contains("present_surface(&surface)?;"));
    }

    #[test]
    fn test_missing_source_fails() {
        let mut g = DocGenerator::new();
        assert_eq!(g.generate("", "sig", "sem", &[], "ex"), Err(err::E_MISSING_SOURCE));
    }

    #[test]
    fn test_note_isolation() {
        let mut g = DocGenerator::new();
        g.generate("api", "fn api()", "desc", &[], "api()").unwrap();
        // 越界注释被拒
        assert_eq!(
            g.add_human_note("api", "pub fn hacked()"),
            Err(err::E_NOTE_OVERREACH)
        );
        // 正常注释可以
        assert!(g.add_human_note("api", "这个接口用于呈现").is_ok());
    }

    #[test]
    fn test_render_includes_notes() {
        let mut g = DocGenerator::new();
        g.generate("api", "fn api()", "d", &[], "e").unwrap();
        g.add_human_note("api", "重要备注").unwrap();
        let doc = g.render("api").unwrap();
        assert!(doc.contains("人工注释区"));
        assert!(doc.contains("重要备注"));
    }

    #[test]
    fn test_regenerate() {
        let mut g = DocGenerator::new();
        g.generate("api", "fn api()", "d", &[], "e").unwrap();
        let v = g.regenerate("api").unwrap();
        assert!(v >= 2);
    }

    #[test]
    fn test_duplicate_rejected() {
        let mut g = DocGenerator::new();
        g.generate("x", "fn x()", "d", &[], "e").unwrap();
        assert_eq!(g.generate("x", "fn x()", "d", &[], "e"), Err(err::E_DUPLICATE));
    }
}
