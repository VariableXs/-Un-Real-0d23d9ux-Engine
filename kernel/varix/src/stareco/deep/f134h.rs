//! 深化层五 · F134 主题分享页（2026-09-27 深化批次五 · h 层装配）。
//!
//! 装配与跨域接口件：分享页 → F151 令牌表的导入适配器（KV 对输出，
//! 悬空引用拒绝导入）、页面区块装配（置顶/最新/推荐三区数据行）。

use super::f134g::ThemeMetaDeep;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 令牌导入适配器：主题元数据 → F151 令牌表 KV 对（键=令牌名，值=色值）
// ---------------------------------------------------------------------------

pub struct TokenImport {
    pub pairs: alloc::vec::Vec<(&'static str, &'static str)>,
}

pub struct ShareManifest {
    pub theme: &'static str,
    /// (令牌名, 色值 #RRGGBB)
    pub tokens: alloc::vec::Vec<(&'static str, &'static str)>,
}

impl ShareManifest {
    /// 导入：色值形制必须合法（#RRGGBB）；非法即整体拒绝（不半截导入）。
    pub fn import(&self) -> Result<TokenImport, &'static str> {
        let mut pairs: alloc::vec::Vec<(&'static str, &'static str)> = alloc::vec::Vec::new();
        for (k, v) in &self.tokens {
            let ok = v.len() == 7
                && v.starts_with('#')
                && v[1..].bytes().all(|b| b.is_ascii_hexdigit());
            if !ok {
                return Err("色值形制非法：整体拒绝导入（不半截）");
            }
            if k.is_empty() {
                return Err("令牌名缺失");
            }
            pairs.push((k, v));
        }
        if pairs.is_empty() {
            return Err("空清单不导入");
        }
        Ok(TokenImport { pairs })
    }
}

/// 与 F141 的悬空引用审计联动：导入前核对引用存在性（f134g 复用口径）。
pub fn dangling_before_import(m: &ThemeMetaDeep, table: &[&'static str]) -> bool {
    !super::f134g::dangling_tokens(m, table).is_empty()
}

// ---------------------------------------------------------------------------
// 页面区块装配：三区（置顶/最新/推荐）→ 各区数据行
// ---------------------------------------------------------------------------

pub struct ShareItem {
    pub id: u32,
    pub title: &'static str,
    pub downloads: u32,
    pub featured: bool,
}

pub struct Sectioned {
    pub pinned: alloc::vec::Vec<&'static str>,
    pub latest: alloc::vec::Vec<&'static str>,
    pub recommended: alloc::vec::Vec<&'static str>,
}

/// 装配：置顶区按 id 升序；最新区按 id 降序（新在前）；推荐区=featured
/// 按下载量降序。同一条目可进多区（页面语义允许）。
pub fn assemble_sections(items: &[ShareItem]) -> Sectioned {
    let mut s = Sectioned {
        pinned: alloc::vec::Vec::new(),
        latest: alloc::vec::Vec::new(),
        recommended: alloc::vec::Vec::new(),
    };
    let mut by_id: alloc::vec::Vec<&ShareItem> = items.iter().collect();
    by_id.sort_by_key(|i| i.id);
    for i in &by_id {
        s.pinned.push(i.title);
    }
    for i in by_id.iter().rev() {
        s.latest.push(i.title);
    }
    let mut feat: alloc::vec::Vec<&ShareItem> =
        items.iter().filter(|i| i.featured).collect();
    // 下载量降序，平局 id 升序。
    for x in 1..feat.len() {
        let k = feat[x];
        let mut j = x;
        while j > 0
            && (feat[j - 1].downloads < k.downloads
                || (feat[j - 1].downloads == k.downloads && feat[j - 1].id > k.id))
        {
            feat[j] = feat[j - 1];
            j -= 1;
        }
        feat[j] = k;
    }
    s.recommended = feat.iter().map(|i| i.title).collect();
    s
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F134H_TAG: &str = "stareco-F134-deep5";

pub fn run_f134_deep5_checks() -> CheckSet {
    let mut set = CheckSet::new(F134H_TAG);

    // 导入适配
    let good = ShareManifest {
        theme: "midnight",
        tokens: alloc::vec![("bg", "#101418"), ("fg", "#E8EAED")],
    };
    let imp = good.import().expect("ok");
    set.add(
        "f134h import pairs",
        imp.pairs == alloc::vec![("bg", "#101418"), ("fg", "#E8EAED")],
        "KV 对输出",
    );
    let bad_color = ShareManifest {
        theme: "broken",
        tokens: alloc::vec![("bg", "#101418"), ("fg", "red")],
    };
    set.add("f134h bad color", bad_color.import().is_err(), "非法色值整体拒绝");
    let empty = ShareManifest { theme: "e", tokens: alloc::vec![] };
    set.add("f134h empty import", empty.import().is_err(), "空清单拒绝");

    // 悬空引用联动
    let meta = ThemeMetaDeep { theme: "midnight", token_refs: alloc::vec!["bg", "ghost"] };
    let table = ["bg", "fg"];
    set.add("f134h dangling gate", dangling_before_import(&meta, &table), "悬空引用拦导入");

    // 区块装配
    let items = [
        ShareItem { id: 2, title: "乙", downloads: 10, featured: true },
        ShareItem { id: 1, title: "甲", downloads: 30, featured: true },
        ShareItem { id: 3, title: "丙", downloads: 5, featured: false },
    ];
    let s = assemble_sections(&items);
    set.add(
        "f134h pinned/latest",
        s.pinned == alloc::vec!["甲", "乙", "丙"] && s.latest == alloc::vec!["丙", "乙", "甲"],
        "置顶升序/最新降序",
    );
    set.add(
        "f134h recommended",
        s.recommended == alloc::vec!["甲", "乙"],
        "推荐=featured 按下载降序",
    );

    set
}

#[cfg(test)]
mod deep5_tests {
    use super::*;

    #[test]
    fn import_rejects_bad_key() {
        let m = ShareManifest { theme: "x", tokens: alloc::vec![("", "#123456")] };
        assert!(m.import().is_err());
    }

    #[test]
    fn sections_empty() {
        let s = assemble_sections(&[]);
        assert!(s.pinned.is_empty() && s.latest.is_empty() && s.recommended.is_empty());
    }
}
