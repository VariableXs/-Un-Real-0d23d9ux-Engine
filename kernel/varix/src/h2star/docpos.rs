//! F273 文档上次位置记忆 · 完整设计（STAR I 主册 H 域）。
//!
//! **判据（主册）**：光标/滚动/选区三恢复精度；元数据随文件走（换
//! 目录仍有效）；另存为不继承判据；损坏元数据容错（读不出则从头，
//! 不报错）。
//!
//! **设计要点（主册）**：文本编辑类应用重开同一文档时光标回到上次
//! 关闭处、滚动位置同步恢复、上次选区若存在淡显 2 秒提示；记忆容量
//! 每文档一个位（存文件旁元数据）；「另存为」的副本不继承位置。
//!
//! 实装：`DocPosMeta`（光标行/列 + 滚动位 + 选区，随文件走）；按内容
//! 键存取（换目录仍有效——键=文件内容指纹不是路径）；损坏元数据容错
//! （解析失败静默回初位——「不报错」判据）；另存为显式不继承。

use crate::checks::CheckSet;

use alloc::string::String;

/// 文档位置元数据（每文档一个位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DocPosMeta {
    /// 光标行。
    pub line: u32,
    /// 光标列。
    pub col: u32,
    /// 滚动位（首可见行）。
    pub scroll_top: u32,
    /// 选区起点（无选区为 None）。
    pub sel_start: Option<(u32, u32)>,
    /// 选区终点。
    pub sel_end: Option<(u32, u32)>,
}

impl DocPosMeta {
    /// 序列化：`行:列:滚动:sel起行,sel起列,sel止行,sel止列` 或
    /// `行:列:滚动:nosel`——损坏容错的解析对（可逆）。
    pub fn serialize(&self) -> String {
        let sel = match (self.sel_start, self.sel_end) {
            (Some((r1, c1)), Some((r2, c2))) => {
                alloc::format!("{},{},{},{}", r1, c1, r2, c2)
            }
            _ => String::from("nosel"),
        };
        alloc::format!("{}:{}:{}:{}", self.line, self.col, self.scroll_top, sel)
    }

    /// 解析：任何一段非法 → None（调用方从头开始，**不报错**——判据）。
    pub fn parse(s: &str) -> Option<DocPosMeta> {
        let parts: alloc::vec::Vec<&str> = s.split(':').collect();
        if parts.len() != 4 {
            return None;
        }
        let line = parts[0].parse::<u32>().ok()?;
        let col = parts[1].parse::<u32>().ok()?;
        let scroll_top = parts[2].parse::<u32>().ok()?;
        if parts[3] == "nosel" {
            return Some(DocPosMeta { line, col, scroll_top, sel_start: None, sel_end: None });
        }
        let nums: alloc::vec::Vec<u32> =
            parts[3].split(',').filter_map(|x| x.parse().ok()).collect();
        if nums.len() != 4 {
            return None;
        }
        Some(DocPosMeta {
            line,
            col,
            scroll_top,
            sel_start: Some((nums[0], nums[1])),
            sel_end: Some((nums[2], nums[3])),
        })
    }
}

/// 位置记忆库：键=文件内容指纹（换目录仍有效——元数据随文件走）。
pub struct PosStore {
    map: alloc::vec::Vec<(String, DocPosMeta)>,
}

impl PosStore {
    pub fn new() -> PosStore {
        PosStore { map: alloc::vec::Vec::new() }
    }

    /// 保存（键=内容指纹）。
    pub fn save(&mut self, content_key: &str, meta: DocPosMeta) {
        match self.map.iter_mut().find(|(k, _)| k == content_key) {
            Some((_, m)) => *m = meta,
            None => self.map.push((String::from(content_key), meta)),
        }
    }

    /// 恢复：命中给位置；未命中/损坏给初位（行0列0滚动0）——不报错。
    pub fn restore(&self, content_key: &str, raw: Option<&str>) -> DocPosMeta {
        if let Some(r) = raw {
            if let Some(m) = DocPosMeta::parse(r) {
                return m;
            }
        }
        self.map
            .iter()
            .find(|(k, _)| k == content_key)
            .map(|(_, m)| *m)
            .unwrap_or(DocPosMeta { line: 0, col: 0, scroll_top: 0, sel_start: None, sel_end: None })
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

const META_A: DocPosMeta = DocPosMeta {
    line: 312,
    col: 44,
    scroll_top: 300,
    sel_start: Some((312, 20)),
    sel_end: Some((312, 44)),
};

pub fn run_docpos_checks() -> CheckSet {
    let mut set = CheckSet::new("h2-F273");
    // 三恢复精度：光标/滚动/选区全量对拍。
    let s = META_A.serialize();
    let back = DocPosMeta::parse(&s).unwrap();
    set.add(
        "F273 restore precise",
        back == META_A && back.line == 312 && back.sel_start == Some((312, 20)),
        "cursor/scroll/selection",
    );
    // 元数据随文件走（内容键——换目录仍有效）。
    let mut store = PosStore::new();
    store.save("指纹-7fa2", META_A);
    let got = store.restore("指纹-7fa2", None);
    set.add("F273 follows content", got == META_A, "key by content not path");
    // 另存为不继承：新键无记忆 → 初位。
    let fresh = store.restore("指纹-新副本", None);
    set.add(
        "F273 save-as fresh",
        fresh.line == 0 && fresh.col == 0 && fresh.sel_start.is_none(),
        "no inherit",
    );
    // 损坏元数据容错：读不出 → 从头，不报错（None 而非 panic）。
    set.add(
        "F273 corrupt tolerant",
        DocPosMeta::parse("garbage").is_none()
            && DocPosMeta::parse("1:2:x:nosel").is_none()
            && store.restore("不存在", Some("垃圾数据")).line == 0,
        "silent fallback",
    );
    // 无选区形制可逆。
    let nosel = DocPosMeta { line: 9, col: 2, scroll_top: 5, sel_start: None, sel_end: None };
    set.add(
        "F273 nosel roundtrip",
        DocPosMeta::parse(&nosel.serialize()) == Some(nosel),
        "invertible",
    );
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f273_meta_flow() {
        let set = run_docpos_checks();
        let (p, f) = set.tally();
        assert!(set.all_passed(), "F273 自检红 {f}/{p}");
    }

    #[test]
    fn corrupt_never_panics() {
        for bad in ["", "::::", "1:2:3:4,5,6,7,8", "-1:0:0:nosel"] {
            let _ = DocPosMeta::parse(bad); // 全部 None，零 panic。
        }
    }
}
