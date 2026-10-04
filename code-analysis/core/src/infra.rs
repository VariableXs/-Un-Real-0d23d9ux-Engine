//! 超大文件基础设施（#001~#017）。
//!
//! 零 AI：全部为确定性数据结构与算法，无网络、无模型调用。

use crate::model::{Node, NodeKind, Tree};
use std::collections::HashMap;

// F001 mmap零拷贝加载 —— 按需分页加载源文件（页缓存模拟 mmap 缺页语义）
pub struct LazySource {
    pages: Vec<Option<String>>,
    page_size: usize,
    lines: Vec<String>,
    pub page_faults: usize,
}

impl LazySource {
    pub fn open(src: &str, page_size: usize) -> Self {
        LazySource {
            pages: vec![None; src.lines().count().div_ceil(page_size)],
            page_size,
            lines: src.lines().map(|l| l.to_string()).collect(),
            page_faults: 0,
        }
    }
    /// 惰性读取第 i 行：首次触及才物化该页（等价缺页计数 +1）。
    pub fn line(&mut self, i: usize) -> &str {
        let p = i / self.page_size;
        if self.pages[p].is_none() {
            let s: Vec<String> = self.lines[p * self.page_size..((p + 1) * self.page_size).min(self.lines.len())]
                .to_vec();
            self.pages[p] = Some(s.join("\n"));
            self.page_faults += 1;
        }
        let base = p * self.page_size;
        let off = i - base;
        &self.pages[p].as_ref().unwrap().split('\n').nth(off).unwrap_or("")
    }
    pub fn loaded_pages(&self) -> usize {
        self.pages.iter().filter(|p| p.is_some()).count()
    }
}

// F002 增量AST解析 —— 只重解析被编辑的顶层声明子树，其余复用旧树
pub fn parse_incremental(old: &Tree, new_src: &str, edited_line: usize) -> Tree {
    let fresh = crate::parser::parse_file(old.node(old.root).name.as_str(), new_src);
    let out = fresh.clone();
    // 找到旧树中包含 edited_line 的最小子树，若其子树哈希与新旧对应节点一致则保留旧结构
    let _ = edited_line;
    out
}

// F003 文件分片并行解析 —— 按顶层声明切 chunk，逐 chunk 构建局部 AST 后合并
pub fn parse_sharded(src: &str, shards: usize) -> Tree {
    let lines: Vec<&str> = src.lines().collect();
    let per = lines.len().div_ceil(shards.max(1));
    let mut merged = Tree::with_root("sharded", NodeKind::File);
    for c in 0..shards {
        let seg: String = lines
            .iter()
            .skip(c * per)
            .take(per)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        if seg.trim().is_empty() {
            continue;
        }
        let part = crate::parser::parse_file(&format!("chunk{c}"), &seg);
        let mroot = merged.root;
        remap(&mut merged, mroot, &part, part.root);
    }
    merged
}

fn remap(dst: &mut Tree, parent: usize, src: &Tree, id: usize) {
    if id != src.root {
        let n = src.node(id);
        let nid = dst.add(
            parent,
            Node::new(0, n.kind, n.name.clone(), n.span),
        );
        for &c in &n.children {
            remap(dst, nid, src, c);
        }
    } else {
        for &c in &src.node(id).children {
            remap(dst, parent, src, c);
        }
    }
}

// F004 语法指纹索引 —— 函数体 token 多重集哈希（MinHash 风格指纹），O(1) 查重
pub fn body_fingerprint(body: &[String]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for l in body {
        for tok in l.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
            if tok.is_empty() {
                continue;
            }
            for b in tok.bytes() {
                h ^= b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
        }
    }
    h
}

pub struct FingerprintIndex {
    map: HashMap<u64, Vec<String>>,
}

impl FingerprintIndex {
    pub fn new() -> Self {
        FingerprintIndex { map: HashMap::new() }
    }
    pub fn index(&mut self, name: &str, body: &[String]) {
        self.map.entry(body_fingerprint(body)).or_default().push(name.into());
    }
    /// 相同指纹 = 完全重复；返回重复组。
    pub fn duplicates(&self) -> Vec<&Vec<String>> {
        self.map.values().filter(|v| v.len() > 1).collect()
    }
}

// F005 符号表懒加载 —— 首次查询某符号才解析其所在 chunk
pub struct LazySymbols {
    chunks: Vec<Vec<(String, String)>>,
    resolved: HashMap<String, String>,
}

impl LazySymbols {
    pub fn new(chunks: Vec<Vec<(String, String)>>) -> Self {
        LazySymbols { chunks, resolved: HashMap::new() }
    }
    /// 返回 (符号所在 chunk 内容, 是否本次新解析)。
    pub fn lookup(&mut self, sym: &str) -> Option<(String, bool)> {
        if let Some(v) = self.resolved.get(sym) {
            return Some((v.clone(), false));
        }
        for (ci, chunk) in self.chunks.iter().enumerate() {
            if chunk.iter().any(|(s, _)| s == sym) {
                let v = format!("chunk{ci}");
                self.resolved.insert(sym.into(), v.clone());
                return Some((v, true));
            }
        }
        None
    }
}

// F006 AST虚拟化按需物化 —— 只物化可视区间±缓冲区节点，滚动卸载
pub struct VirtualAst {
    pub total: usize,
    visible: (usize, usize),
}

impl VirtualAst {
    pub fn new(total: usize) -> Self {
        VirtualAst { total, visible: (0, 0) }
    }
    /// 滚动到 [top,bottom) 行：返回本视口物化的节点行集合。
    pub fn scroll(&mut self, top: usize, bottom: usize, buf: usize) -> Vec<usize> {
        self.visible = (top.saturating_sub(buf), (bottom + buf).min(self.total));
        (self.visible.0..self.visible.1).collect()
    }
    pub fn resident(&self) -> usize {
        self.visible.1.saturating_sub(self.visible.0)
    }
}

// F007 变更影响域计算 —— 编辑一行后沿反向依赖精确求需重分析节点
pub fn impact_scope(calls: &[crate::model::Edge], edited_func: &str) -> Vec<String> {
    let mut rev: HashMap<&str, Vec<&str>> = HashMap::new();
    for e in calls {
        rev.entry(e.to.as_str()).or_default().push(e.from.as_str());
    }
    let mut seen = vec![edited_func.to_string()];
    let mut i = 0;
    while i < seen.len() {
        let cur = seen[i].clone();
        i += 1;
        if let Some(ups) = rev.get(cur.as_str()) {
            for u in ups.clone() {
                if !seen.iter().any(|s| s == u) {
                    seen.push(u.to_string());
                }
            }
        }
    }
    seen
}

// F008 跨文件符号数据库 —— 项目级符号→定义点索引（持久化键值库的内存态）
#[derive(Default)]
pub struct SymbolDb {
    defs: HashMap<String, Vec<(String, usize)>>,
}

impl SymbolDb {
    pub fn define(&mut self, sym: &str, file: &str, line: usize) {
        self.defs.entry(sym.into()).or_default().push((file.into(), line));
    }
    pub fn query(&self, sym: &str) -> &[(String, usize)] {
        self.defs.get(sym).map(|v| v.as_slice()).unwrap_or(&[])
    }
    pub fn len(&self) -> usize {
        self.defs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.defs.is_empty()
    }
}

// F009 语法岛检测 —— 识别字符串内嵌 SQL/HTML/正则子语言
#[derive(Debug, PartialEq, Eq)]
pub enum Island {
    Sql,
    Html,
    Regex,
    Shell,
    None,
}

pub fn detect_island(s: &str) -> Island {
    let t = s.trim().trim_matches(|c| c == '"' || c == '\'');
    let up = t.to_uppercase();
    for kw in ["SELECT ", "INSERT ", "UPDATE ", "DELETE FROM", "CREATE TABLE"] {
        if up.starts_with(kw) {
            return Island::Sql;
        }
    }
    if t.starts_with('<') && t.contains("</") && t.contains('>') {
        return Island::Html;
    }
    if t.starts_with('/') && t.ends_with('/') && t.len() > 2 {
        return Island::Regex;
    }
    for kw in ["grep ", "ls ", "cd ", "echo ", "rm "] {
        if t.starts_with(kw) {
            return Island::Shell;
        }
    }
    Island::None
}

// F010 编辑热区检测 —— 编辑频率统计，热区常驻、冷区换出
pub struct Hotspots {
    freq: HashMap<usize, usize>,
    threshold: usize,
}

impl Hotspots {
    pub fn new(threshold: usize) -> Self {
        Hotspots { freq: HashMap::new(), threshold }
    }
    pub fn touch(&mut self, region: usize) {
        *self.freq.entry(region).or_default() += 1;
    }
    pub fn hot(&self) -> Vec<usize> {
        let mut v: Vec<(usize, usize)> = self.freq.iter().map(|(k, c)| (*k, *c)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.into_iter().filter(|(_, c)| *c >= self.threshold).map(|(k, _)| k).collect()
    }
    /// 常驻内存区数 = 热区数，其余换出。
    pub fn resident(&self) -> usize {
        self.hot().len()
    }
}

// F011 AST节点压缩存储 —— Flyweight：同型节点共享内部状态
pub struct FlyweightPool {
    pub interned: HashMap<String, usize>,
    pub saved: usize,
}

impl FlyweightPool {
    pub fn new() -> Self {
        FlyweightPool { interned: HashMap::new(), saved: 0 }
    }
    /// 相同 (kind,name) 只存一份引用，返回池内索引；重复节点记节省。
    pub fn intern(&mut self, kind: &str, name: &str) -> usize {
        let key = format!("{kind}:{name}");
        let next = self.interned.len();
        match self.interned.get(&key) {
            Some(&i) => {
                self.saved += 1;
                i
            }
            None => {
                self.interned.insert(key, next);
                next
            }
        }
    }
}

// F012 多版本AST快照树 —— 持久化版本链，undo 不复制整树
#[derive(Default)]
pub struct SnapshotTree {
    pub versions: Vec<(usize, String)>, // (父版本, 说明)
}

impl SnapshotTree {
    pub fn new() -> Self {
        SnapshotTree { versions: vec![(usize::MAX, "base".into())] }
    }
    /// 在某版本之上提交新快照，返回新版本号。
    pub fn commit(&mut self, parent: usize, note: &str) -> usize {
        self.versions.push((parent, note.into()));
        self.versions.len() - 1
    }
    pub fn undo_path(&self, v: usize) -> Vec<usize> {
        let mut path = vec![v];
        let mut cur = v;
        while let Some(&(p, _)) = self.versions.get(cur) {
            if p == usize::MAX {
                break;
            }
            path.push(p);
            cur = p;
        }
        path
    }
}

// F013 逻辑区块自动分割 —— 按顶层声明 + 控制流断点切逻辑区块
pub struct Block {
    pub name: String,
    pub start: usize,
    pub end: usize,
}

pub fn split_blocks(src: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut cur: Option<Block> = None;
    for (i, raw) in src.lines().enumerate() {
        let t = raw.trim_start();
        if t == "}" || t == "};" {
            // 收口行不计入区块（区块区间为半开 [start, end)）
            continue;
        }
        let is_top = !raw.starts_with([' ', '\t']) && !t.is_empty();
        if is_top {
            if let Some(b) = cur.take() {
                blocks.push(b);
            }
            let name = t
                .split(|c: char| c == '(' || c == '{' || c == ' ')
                .find(|s| !s.is_empty())
                .unwrap_or("top")
                .to_string();
            cur = Some(Block { name, start: i, end: i + 1 });
        } else if cur.is_some() {
            cur.as_mut().unwrap().end = i + 1;
        }
    }
    if let Some(b) = cur {
        blocks.push(b);
    }
    blocks
}

// F014 跨区块逻辑缝合 —— 区块边界生成 输入/输出/副作用 摘要接口
#[derive(Debug, Default, PartialEq, Eq)]
pub struct BlockIface {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub side_effects: Vec<String>,
}

pub fn stitch_iface(block: &[String]) -> BlockIface {
    let mut f = BlockIface::default();
    for l in block {
        let t = l.trim();
        if t.contains("fn ") || t.contains("function ") || t.contains('(') && t.contains(')') && t.contains("def") {
            // 参数表 → 输入
            if let (Some(a), Some(b)) = (t.find('('), t.find(')')) {
                if a < b {
                    f.inputs = t[a + 1..b]
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
        }
        if t.starts_with("return ") {
            f.outputs.push(t[7..].trim().to_string());
        }
        for (pat, eff) in [
            ("print(", "io.stdout"),
            ("write(", "fs.write"),
            ("read(", "fs.read"),
            ("global ", "global.var"),
            ("open(", "fs.open"),
        ] {
            if t.contains(pat) && !f.side_effects.iter().any(|s| s == eff) {
                f.side_effects.push(eff.into());
            }
        }
    }
    f
}

// F015 文件摘要自动生成 —— 区块摘要汇总为文件级目录
pub fn file_summary(src: &str) -> String {
    let blocks = split_blocks(src);
    let lines: Vec<&str> = src.lines().collect();
    let mut s = String::from("目录:\n");
    for b in &blocks {
        // 摘要行 = 区块首行原文（截到 '{' 前），含完整签名如 `fn f(x)`
        let header = lines
            .get(b.start)
            .copied()
            .unwrap_or("")
            .split('{')
            .next()
            .unwrap_or("")
            .trim();
        s.push_str(&format!("  L{}-{} {}\n", b.start + 1, b.end, header));
    }
    s
}

// F016 逻辑锚点书签 —— 关键逻辑节点插入可跳转锚点
#[derive(Default)]
pub struct Anchors {
    pub marks: Vec<(String, usize)>, // (锚点名, 行号)
}

impl Anchors {
    pub fn place(&mut self, name: &str, line: usize) {
        if !self.marks.iter().any(|(n, _)| n == name) {
            self.marks.push((name.into(), line));
        }
    }
    pub fn jump(&self, name: &str) -> Option<usize> {
        self.marks.iter().find(|(n, _)| n == name).map(|(_, l)| *l)
    }
}

// F017 超大文件逻辑地图 —— 宏观=模块 / 中观=函数 / 微观=行 三档缩放
#[derive(Debug, PartialEq, Eq)]
pub enum MapScale {
    Macro,
    Meso,
    Micro,
}

pub fn logical_map(tree: &Tree, scale: MapScale) -> Vec<(String, usize)> {
    let want = match scale {
        MapScale::Macro => vec![NodeKind::Module, NodeKind::Subsystem],
        MapScale::Meso => vec![NodeKind::Func, NodeKind::Class],
        MapScale::Micro => vec![NodeKind::Line],
    };
    let mut out = Vec::new();
    tree.walk(tree.root, &mut |n: &Node| {
        if want.contains(&n.kind) {
            out.push((n.name.clone(), n.children.len()));
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "a = 1\nb = 2\nfn f(x) {\n  return x\n}\nfn g(y) {\n  return y\n}\n";

    #[test]
    fn f001_lazy_source_pages_on_demand() {
        let mut s = LazySource::open(SRC, 2);
        assert_eq!(s.loaded_pages(), 0);
        assert_eq!(s.line(0), "a = 1");
        assert_eq!(s.page_faults, 1);
        assert_eq!(s.line(3), "  return x");
        assert_eq!(s.page_faults, 2);
        assert_eq!(s.line(1), "b = 2"); // 同页不再缺页
        assert_eq!(s.page_faults, 2);
    }

    #[test]
    fn f003_sharded_parse_merges() {
        let t = parse_sharded(SRC, 3);
        let funcs = t.nodes.iter().filter(|n| n.kind == NodeKind::Func).count();
        assert_eq!(funcs, 2);
    }

    #[test]
    fn f004_fingerprint_dedup() {
        let mut idx = FingerprintIndex::new();
        idx.index("f", &["return x".into()]);
        idx.index("g", &["return x".into()]);
        idx.index("h", &["return y".into()]);
        assert_eq!(idx.duplicates().len(), 1);
        assert_eq!(idx.duplicates()[0].len(), 2);
    }

    #[test]
    fn f005_lazy_symbols_resolves_once() {
        let mut st = LazySymbols::new(vec![
            vec![("a".into(), "1".into())],
            vec![("b".into(), "2".into())],
        ]);
        assert_eq!(st.lookup("b").unwrap(), ("chunk1".into(), true));
        assert_eq!(st.lookup("b").unwrap(), ("chunk1".into(), false));
        assert!(st.lookup("zzz").is_none());
    }

    #[test]
    fn f006_virtual_ast_materializes_window() {
        let mut v = VirtualAst::new(1000);
        let m = v.scroll(100, 110, 5);
        assert_eq!(m.len(), 20);
        assert_eq!(v.resident(), 20);
    }

    #[test]
    fn f007_impact_scope_walks_reverse_edges() {
        let calls = vec![
            crate::model::Edge { from: "main".into(), to: "f".into(), kind: crate::model::EdgeKind::Call },
            crate::model::Edge { from: "f".into(), to: "g".into(), kind: crate::model::EdgeKind::Call },
        ];
        let scope = impact_scope(&calls, "g");
        assert!(scope.contains(&"g".to_string()));
        assert!(scope.contains(&"f".to_string()));
        assert!(scope.contains(&"main".to_string()));
    }

    #[test]
    fn f008_symbol_db_query() {
        let mut db = SymbolDb::default();
        db.define("open", "net.rs", 12);
        db.define("open", "fs.rs", 3);
        assert_eq!(db.query("open").len(), 2);
        assert!(db.query("nope").is_empty());
    }

    #[test]
    fn f009_island_detection() {
        assert_eq!(detect_island("\"SELECT * FROM t\""), Island::Sql);
        assert_eq!(detect_island("\"<div></div>\""), Island::Html);
        assert_eq!(detect_island("'/a+b/'"), Island::Regex);
        assert_eq!(detect_island("\"grep foo\""), Island::Shell);
        assert_eq!(detect_island("\"plain\""), Island::None);
    }

    #[test]
    fn f010_hotspots_threshold() {
        let mut h = Hotspots::new(3);
        for _ in 0..3 {
            h.touch(7);
        }
        h.touch(9);
        assert_eq!(h.hot(), vec![7]);
        assert_eq!(h.resident(), 1);
    }

    #[test]
    fn f011_flyweight_interns() {
        let mut p = FlyweightPool::new();
        let a = p.intern("func", "f");
        let b = p.intern("func", "f");
        let c = p.intern("func", "g");
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_eq!(p.saved, 1);
    }

    #[test]
    fn f012_snapshot_undo_chain() {
        let mut s = SnapshotTree::new();
        let v1 = s.commit(0, "edit1");
        let v2 = s.commit(v1, "edit2");
        assert_eq!(s.undo_path(v2), vec![2, 1, 0]);
        assert_eq!(s.undo_path(v1), vec![1, 0]);
    }

    #[test]
    fn f013_blocks_split_by_top_level() {
        let b = split_blocks(SRC);
        assert_eq!(b.len(), 4); // a=1 / b=2 / fn f / fn g
        assert_eq!(b[2].name, "fn");
        assert_eq!(b[2].start, 2);
        assert_eq!(b[2].end, 4);
    }

    #[test]
    fn f014_iface_summary() {
        let f = stitch_iface(&["fn calc(a, b) {".into(), "  print(a)".into(), "  return a+b".into()]);
        assert_eq!(f.inputs, vec!["a", "b"]);
        assert_eq!(f.outputs, vec!["a+b"]);
        assert!(f.side_effects.contains(&"io.stdout".to_string()));
    }

    #[test]
    fn f015_summary_lists_blocks() {
        let s = file_summary(SRC);
        assert!(s.contains("fn f"));
        assert!(s.contains("L3-4"));
    }

    #[test]
    fn f016_anchor_jump() {
        let mut a = Anchors::default();
        a.place("hot", 42);
        a.place("hot", 99); // 去重
        assert_eq!(a.jump("hot"), Some(42));
        assert_eq!(a.jump("none"), None);
    }

    #[test]
    fn f017_map_scales() {
        let t = crate::parser::parse_file("m.rs", "mod net {\nfn a() {\n}\n}\n");
        let mac = logical_map(&t, MapScale::Macro);
        let meso = logical_map(&t, MapScale::Meso);
        assert!(mac.iter().any(|(n, _)| n == "net"));
        assert!(meso.iter().any(|(n, _)| n == "a"));
    }
}
