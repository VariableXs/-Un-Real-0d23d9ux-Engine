//! F306 文件内容全文搜索 · 完整设计（AI-H3 · 主册 G-H 区段）。
//!
//! **判据（主册）**：内容命中高亮用例；索引构建空闲执行判据；索引状态
//! 可视化；10MB 截断标注；索引损坏自重建。
//!
//! **设计要点（主册）**：
//! - 搜索不止文件名：文本类文件（.txt/.md/.vxnote）建立内容索引（空闲
//!   时段构建，F050 联动），搜「季度预算」能命中正文含该词的文件（结果
//!   高亮命中行片段）；
//! - 索引状态在搜索页可见（构建中显示进度、完成显示「已索引 N 文件」）；
//! - 大文件（>10MB）只索引前 1MB 并标注；
//! - 无感标准：记得内容忘了文件名也能找到；索引从不拖慢前台（空闲干活
//!   的纪律）。
//!
//! 实现形态：空闲驱动的索引器（busy 时零进展——纪律面）+ 行片段命中
//! 高亮 + 校验和损坏自重建。文件源由调用方注入口供给（不依赖真实 VFS）。

use crate::checks::CheckSet;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 截断阈值：只索引前 1MB。
pub const TRUNCATE_BYTES: usize = 1 << 20;

/// 大文件判定：>10MB 标注为大文件（截断行为同上——1MB 索引量）。
pub const BIG_FILE_BYTES: usize = 10 << 20;

/// 单轮空闲索引配额（文件数——空闲窗口内干多少，可注入）。
pub const IDLE_QUOTA: usize = 4;

// ---------------------------------------------------------------------------
// 数据面
// ---------------------------------------------------------------------------

/// 一个已索引文档。
#[derive(Clone, Debug)]
struct IndexedDoc {
    path: String,
    /// 已索引内容（截断后）。
    content: String,
    /// 源内容总长（截断标注依据）。
    source_len: usize,
    truncated: bool,
    /// 内容校验和（FNV-1a——损坏检测）。
    checksum: u32,
}

impl IndexedDoc {
    fn new(path: &str, source: &str) -> IndexedDoc {
        let take = source.len().min(TRUNCATE_BYTES);
        // 字符边界截断（多字节安全——按字符收集到配额）。
        let mut content = String::new();
        let mut used = 0usize;
        for c in source.chars() {
            let l = c.len_utf8();
            if used + l > take {
                break;
            }
            content.push(c);
            used += l;
        }
        IndexedDoc {
            path: String::from(path),
            checksum: fnv1a(&content),
            content,
            source_len: source.len(),
            truncated: source.len() > TRUNCATE_BYTES,
        }
    }
}

/// FNV-1a 32 位（与 themepack 同族——域内统一实现一份）。
fn fnv1a(data: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in data.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 文本类扩展名判定（.txt/.md/.vxnote 三类——主册口径）。
pub fn is_text_file(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.ends_with(".txt") || p.ends_with(".md") || p.ends_with(".vxnote")
}

// ---------------------------------------------------------------------------
// 索引器
// ---------------------------------------------------------------------------

/// 索引状态（可视化面——三态诚实）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IndexPhase {
    /// 队列非空、等待空闲窗口。
    Pending,
    /// 空闲窗口构建中。
    Building,
    /// 全量完成。
    Complete,
}

/// 空闲驱动内容索引器。
pub struct ContentIndex {
    docs: Vec<IndexedDoc>,
    /// 待索引队列（路径+源内容——由注入口灌入）。
    queue: Vec<(String, String)>,
    /// 已完成的空闲轮次（记账——空闲执行判据对账面）。
    pub idle_rounds: u64,
    /// 忙时段被拒的进度请求（纪律对账——应等于注入的 busy 次数）。
    pub busy_deferrals: u64,
    /// 损坏自重建次数。
    pub rebuilds: u64,
}

impl ContentIndex {
    pub fn new() -> ContentIndex {
        ContentIndex { docs: Vec::new(), queue: Vec::new(), idle_rounds: 0, busy_deferrals: 0, rebuilds: 0 }
    }

    /// 灌入待索引文件（只收文本类；非文本类显式拒绝——不静默吞）。
    pub fn enqueue(&mut self, path: &str, content: &str) -> bool {
        if !is_text_file(path) {
            return false;
        }
        self.queue.push((String::from(path), String::from(content)));
        true
    }

    /// 空闲推进：busy=true 时零进展（纪律），空闲时每轮最多 IDLE_QUOTA 个。
    /// 返回本轮实际完成的文件数。
    pub fn tick(&mut self, busy: bool) -> usize {
        if busy {
            self.busy_deferrals += 1;
            return 0;
        }
        self.idle_rounds += 1;
        let mut done = 0usize;
        while done < IDLE_QUOTA && !self.queue.is_empty() {
            let (path, content) = self.queue.remove(0);
            self.docs.push(IndexedDoc::new(&path, &content));
            done += 1;
        }
        done
    }

    /// 状态可视化（三态 + 已索引数 + 待建数——「已索引 N 文件」判线）。
    pub fn status(&self) -> (IndexPhase, usize, usize) {
        if self.queue.is_empty() {
            (IndexPhase::Complete, self.docs.len(), 0)
        } else if self.docs.is_empty() && self.idle_rounds == 0 {
            (IndexPhase::Pending, 0, self.queue.len())
        } else {
            (IndexPhase::Building, self.docs.len(), self.queue.len())
        }
    }

    /// 内容搜索：命中文件 + 高亮行片段（命中行原文 + 列区间——渲染面
    /// 直接可用）。排序：命中次数降序、同数次按路径字典序（确定）。
    pub fn search(&self, term: &str) -> Vec<ContentHit> {
        let mut out: Vec<ContentHit> = Vec::new();
        if term.is_empty() {
            return out;
        }
        for d in &self.docs {
            let mut lines: Vec<LineFrag> = Vec::new();
            let mut total = 0usize;
            for (lineno, line) in d.content.lines().enumerate() {
                let mut cols: Vec<(usize, usize)> = Vec::new();
                let lb = line.as_bytes();
                let tb = term.as_bytes();
                if !tb.is_empty() && lb.len() >= tb.len() {
                    // 字节级滑动窗（ASCII 术语口径——高亮列即字节列）。
                    let mut i = 0;
                    while i + tb.len() <= lb.len() {
                        if &lb[i..i + tb.len()] == tb {
                            cols.push((i, i + tb.len()));
                            total += 1;
                            i += tb.len();
                        } else {
                            i += 1;
                        }
                    }
                }
                if !cols.is_empty() {
                    lines.push(LineFrag { lineno: lineno + 1, line: String::from(line), highlight_cols: cols });
                }
            }
            if total > 0 {
                out.push(ContentHit {
                    path: d.path.clone(),
                    total_matches: total,
                    truncated: d.truncated,
                    big_file: d.source_len > BIG_FILE_BYTES,
                    lines,
                });
            }
        }
        out.sort_by(|a, b| b.total_matches.cmp(&a.total_matches).then(a.path.cmp(&b.path)));
        out
    }

    /// 损坏自重建：逐文档校验；校验不过的文档按路径从队列源重建。
    /// 返回重建的路径清单（自愈显性化——不静默）。
    pub fn verify_and_rebuild(&mut self, sources: &[(&str, &str)]) -> Vec<String> {
        let mut rebuilt: Vec<String> = Vec::new();
        for i in 0..self.docs.len() {
            let ok = fnv1a(&self.docs[i].content) == self.docs[i].checksum;
            if !ok {
                let path = self.docs[i].path.clone();
                if let Some((_, src)) = sources.iter().find(|(p, _)| String::from(*p) == path) {
                    self.docs[i] = IndexedDoc::new(&path, src);
                    self.rebuilds += 1;
                    rebuilt.push(path);
                }
            }
        }
        rebuilt
    }

    pub fn indexed_count(&self) -> usize {
        self.docs.len()
    }
}

impl Default for ContentIndex {
    fn default() -> ContentIndex {
        ContentIndex::new()
    }
}

/// 一条内容命中（可视化单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentHit {
    pub path: String,
    pub total_matches: usize,
    /// 大文件标注（>10MB 源）。
    pub big_file: bool,
    /// 截断标注（只索引了前 1MB）。
    pub truncated: bool,
    /// 命中行片段（最多按文档内全部命中行）。
    pub lines: Vec<LineFrag>,
}

/// 一行高亮片段。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineFrag {
    pub lineno: usize,
    pub line: String,
    /// 高亮列区间（字节闭开区间——渲染面直接用）。
    pub highlight_cols: Vec<(usize, usize)>,
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F306 自检（判据：高亮；空闲执行；状态可视化；10MB 截断；损坏自重建）。
pub fn run_fulltext_checks() -> CheckSet {
    let mut set = CheckSet::new("F306-fulltext");

    // 1. 文本类判定：三类收、其他拒。
    let mut idx = ContentIndex::new();
    set.add(
        "text ext gate",
        is_text_file("a.TXT") && is_text_file("b.md") && is_text_file("c.vxnote")
            && !idx.enqueue("d.exe", "binary-ish") && idx.enqueue("e.txt", "x"),
        "",
    );

    // 2. 空闲执行判据：busy 时零进展、计数入账；空闲才推进。
    let mut idx = ContentIndex::new();
    for i in 0..6 {
        let name: &'static str = match i {
            0 => "f0.txt", 1 => "f1.txt", 2 => "f2.txt", 3 => "f3.txt", 4 => "f4.txt", _ => "f5.txt",
        };
        idx.enqueue(name, "季度预算见表格\n其余内容");
    }
    let d1 = idx.tick(true);
    let d2 = idx.tick(false);
    set.add(
        "idle only progress",
        d1 == 0 && idx.busy_deferrals == 1 && d2 == IDLE_QUOTA && idx.idle_rounds == 1,
        "",
    );

    // 3. 状态可视化：构建中显进度 → 完成显「已索引 N」。
    let (phase, done_n, pending_n) = idx.status();
    set.add(
        "status visible building",
        phase == IndexPhase::Building && done_n == IDLE_QUOTA && pending_n == 2,
        "",
    );
    idx.tick(false);
    idx.tick(false); // 队列只剩 2，第二轮清空。
    let (phase, done_n, pending_n) = idx.status();
    set.add(
        "status complete counts",
        phase == IndexPhase::Complete && done_n == 6 && pending_n == 0,
        "",
    );

    // 4. 内容命中 + 高亮：搜「季度预算」命中正文（非文件名）。
    let mut idx = ContentIndex::new();
    idx.enqueue("报表.md", "标题行\n这是季度预算的明细\n备注：季度预算需复核\n尾行");
    idx.tick(false); // 空闲窗口建索引。
    let hits = idx.search("季度预算");
    set.add(
        "content hit with highlight",
        hits.len() == 1
            && hits[0].total_matches == 2
            && hits[0].lines.len() == 2
            && hits[0].lines[0].lineno == 2
            && hits[0].lines[0].highlight_cols[0] == (6, 18), // 「季度预算」12 字节（6..18）。
        "",
    );

    // 5. 截断标注：>1MB 只索引前 1MB（构造超限源——用重复段；>10MB 判
    //    大文件标注）。源用 1MB+100 字节的 ASCII。
    let big = "x".repeat(TRUNCATE_BYTES + 100);
    let huge = "y".repeat(BIG_FILE_BYTES + 1);
    let mut idx = ContentIndex::new();
    idx.enqueue("big.txt", &big);
    idx.enqueue("huge.txt", &huge);
    idx.tick(false);
    idx.tick(false);
    let hits = idx.search("xxxx");
    set.add(
        "truncation marked",
        hits.len() == 1 && hits[0].path == "big.txt" && hits[0].truncated && !hits[0].big_file,
        "",
    );
    let hits = idx.search("yyyy");
    set.add(
        "big file marked",
        hits.len() == 1 && hits[0].path == "huge.txt" && hits[0].truncated && hits[0].big_file,
        "",
    );

    // 6. 损坏自重建：篡改一个文档内容 → 校验抓出 → 从源重建 → 再查命中。
    let mut idx = ContentIndex::new();
    idx.enqueue("a.txt", "季度预算在第三行");
    idx.tick(false);
    let sources: Vec<(&str, &str)> = alloc::vec![("a.txt", "季度预算在第三行")];
    // 模拟损坏：直改内部（测试面绕过封装——校验必须能抓出）。
    idx.docs[0].content.push_str("污染尾巴");
    let rebuilt = idx.verify_and_rebuild(&sources);
    set.add(
        "corruption auto rebuild",
        rebuilt == vec![String::from("a.txt")]
            && idx.rebuilds == 1
            && idx.search("季度预算").len() == 1
            && idx.search("污染尾巴").is_empty(),
        "",
    );

    // 7. 排序确定性：命中次数降序、同次路径序。
    let mut idx = ContentIndex::new();
    idx.enqueue("b.txt", "词 词 词");
    idx.enqueue("a.txt", "词");
    idx.enqueue("c.txt", "无关内容");
    idx.tick(false);
    let hits = idx.search("词");
    set.add(
        "rank deterministic",
        hits.len() == 2 && hits[0].path == "b.txt" && hits[0].total_matches == 3 && hits[1].path == "a.txt",
        "",
    );

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_batching() {
        let mut idx = ContentIndex::new();
        for i in 0..IDLE_QUOTA * 2 + 1 {
            let name: &'static str = match i {
                0 => "a.txt", 1 => "b.txt", 2 => "c.txt", 3 => "d.txt", 4 => "e.txt",
                5 => "f.txt", 6 => "g.txt", 7 => "h.txt", 8 => "i.txt", _ => "j.txt",
            };
            idx.enqueue(name, "词");
        }
        assert_eq!(idx.tick(false), IDLE_QUOTA);
        assert_eq!(idx.tick(false), IDLE_QUOTA);
        assert_eq!(idx.tick(false), 1);
        assert!(matches!(idx.status(), (IndexPhase::Complete, _, _)));
    }

    #[test]
    fn multibyte_truncation_is_char_safe() {
        // 源以多字节字符跨过 1MB 边界——截断不得劈开字符（内容合法 UTF-8）。
        let pad = "a".repeat(TRUNCATE_BYTES - 12);
        let src = pad + "季度预算" + "尾";
        let d = IndexedDoc::new("t.txt", &src);
        assert!(d.truncated);
        assert!(d.content.ends_with("季度预算"), "多字节字符完整保留");
        assert!(!d.content.ends_with("尾"), "越界字符按字节配额安全截断");
    }

    #[test]
    fn empty_term_yields_nothing() {
        let mut idx = ContentIndex::new();
        idx.enqueue("a.txt", "词");
        idx.tick(false);
        assert!(idx.search("").is_empty());
    }

    #[test]
    fn fnv1a_known_vector() {
        assert_eq!(fnv1a("a"), 0xe40c_292c);
    }
}
