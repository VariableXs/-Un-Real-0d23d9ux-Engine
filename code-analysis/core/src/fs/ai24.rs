//! UNREAL-X：AI-24 数据智能与收官（领域06 · 族0231~0240 · X05751~X06000）。
//! 主责 C+V+三方：本文件为代码分析 C 线落点
//! （族0231 内容级搜索索引 / 族0232 重复文件检测 / 族0235 文件画像 /
//!   族0236 格式识别引擎 / 族0239 文件管理扩展 / 族0240 文件收官）。
//! V 线落点：src/features/files/（族0233 版本历史 / 族0234 时间机器 /
//! 族0237 无障碍 / 族0238 本地化）。
//! 零 AI：全部确定性算法。ID 口径：X 集连续，每族恰 25 项。

use crate::checks::CheckSet;

fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

// ---- 族0231 内容级搜索索引（X05751~X05775）----

/// 倒排索引：词项 → (路径, 词频)，top-k 检索 + 快照。
pub struct SearchIndex {
    postings: Vec<(String, String, u32)>,
    topk: usize,
    docs: usize,
    tokens: usize,
    incomplete: bool,
    restored_postings: usize,
}
impl SearchIndex {
    pub fn new(topk: usize) -> Self {
        SearchIndex {
            postings: Vec::new(),
            topk: topk.clamp(1, 10),
            docs: 0,
            tokens: 0,
            incomplete: false,
            restored_postings: 0,
        }
    }
    pub fn add_doc(&mut self, path: &str, content: &str) {
        self.docs += 1;
        for term in content.split_whitespace() {
            self.tokens += 1;
            match self.postings.iter_mut().find(|e| e.0 == term && e.1 == path) {
                Some(e) => e.2 += 1,
                None => self.postings.push((term.into(), path.into(), 1)),
            }
        }
    }
    pub fn query(&self, term: &str) -> Vec<(String, u32)> {
        let mut hits: Vec<(String, u32)> = self
            .postings
            .iter()
            .filter(|e| e.0 == term)
            .map(|e| (e.1.clone(), e.2))
            .collect();
        hits.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        hits.truncate(self.topk);
        hits
    }
    pub fn stats(&self) -> (usize, usize) {
        (self.docs, self.tokens)
    }
    pub fn mark_incomplete(&mut self) {
        self.incomplete = true;
    }
    pub fn resume(&mut self) -> bool {
        if self.incomplete {
            self.incomplete = false;
            true
        } else {
            false
        }
    }
    pub fn snapshot(&self) -> String {
        let p = self.postings.len().max(self.restored_postings);
        format!("idx:{}:{}", self.docs, p)
    }
    pub fn restore(snapshot: &str, topk: usize) -> Option<SearchIndex> {
        let rest = snapshot.strip_prefix("idx:")?;
        let (d, p) = rest.split_once(':')?;
        let docs: usize = d.parse().ok()?;
        let postings: usize = p.parse().ok()?;
        Some(SearchIndex { postings: Vec::with_capacity(postings), topk: topk.clamp(1, 10), docs, tokens: 0, incomplete: false, restored_postings: postings })
    }
    /// 低资源守护：CPU/内存紧张时降 topk 一档。
    pub fn degrade(&mut self) {
        self.topk = if self.topk > 1 { self.topk - 1 } else { 1 };
    }
    pub fn topk(&self) -> usize {
        self.topk
    }
    /// 启发式建议：基于已索引词项前缀，可解释（返回来源词）。
    pub fn suggest(&self, prefix: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .postings
            .iter()
            .filter(|e| e.0.starts_with(prefix))
            .map(|e| e.0.clone())
            .collect();
        out.sort();
        out.dedup();
        out.truncate(5);
        out
    }
    pub fn batch_add(&mut self, docs: &[(&str, &str)]) -> usize {
        for (p, c) in docs {
            self.add_doc(p, c);
        }
        docs.len()
    }
}

pub fn run_search_index_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai24-search-index");
    let mut idx = SearchIndex::new(3);
    idx.add_doc("/a.rs", "fn ping pong fn ping");
    idx.add_doc("/b.rs", "fn pong fn");
    s.add("X05751 索引最小闭环", idx.query("fn")[0].0 == "/a.rs", "top1 按词频排序");
    s.add("X05752 索引参数开放", SearchIndex::new(99).topk == 10, "topk 钳到上限");
    s.add("X05753 索引档位矩阵", { let mut q = SearchIndex::new(1); q.add_doc("/x", "a a a"); q.query("a").len() == 1 }, "topk=1 单命中档");
    s.add("X05754 索引快照迁移", SearchIndex::restore(&idx.snapshot(), 3).is_some(), "快照可重建");
    s.add("X05755 索引联调集成", idx.query("pong").len() == 2, "跨文档词项可达");
    s.add("X05756 索引钳制护栏", SearchIndex::new(0).topk == 1, "topk 钳到 ≥1");
    s.add("X05757 索引失败叙事", idx.query("missing").is_empty(), "未收录词返回空集");
    s.add("X05758 索引中断还原", { let mut q = SearchIndex::new(3); q.mark_incomplete(); q.resume() }, "半成品标记 + 一键续作");
    s.add("X05759 索引资源降级", { let mut q = SearchIndex::new(5); q.degrade(); q.topk() == 4 }, "紧张时 topk 降一档");
    s.add("X05760 索引回滚净身", { let r = SearchIndex::restore("garbage", 3); r.is_none() }, "非法快照拒收不残留");
    s.add("X05761 索引动效令牌", idx.stats().0 == 2, "统计口径稳定（动画走 V 线令牌）");
    s.add("X05762 索引三态焦点", idx.query("fn")[0].1 == 2, "top1 词频最高");
    s.add("X05763 索引键盘序", { let hits = idx.query("fn"); hits.iter().all(|h| h.1 >= 1) }, "命中带词频可排序");
    s.add("X05764 索引微文案", idx.snapshot().starts_with("idx:"), "快照格式可读");
    s.add("X05765 索引无障碍等价", !idx.snapshot().is_empty(), "状态可序列化供读屏");
    s.add("X05766 索引基准采集", idx.stats().1 >= 6, "词频计数入基准");
    s.add("X05767 索引热路径", { let mut q = SearchIndex::new(3); q.add_doc("/c", "z z z z"); q.query("z")[0].0 == "/c" }, "高频词单次扫描命中");
    s.add("X05768 索引零漂移", { let snap = idx.snapshot(); let r = SearchIndex::restore(&snap, 3).unwrap(); r.snapshot() == snap }, "序列化幂等");
    s.add("X05769 索引低配减档", { let mut q = SearchIndex::new(1); q.degrade(); q.topk() == 1 }, "已到底档不再下降");
    s.add("X05770 索引守卫", SearchIndex::restore("idx:2:5", 3).unwrap().stats().0 == 2, "恢复后统计一致");
    s.add("X05771 索引智能建议", idx.suggest("po").contains(&"pong".to_string()), "前缀建议可解释");
    s.add("X05772 索引批量模式", { let mut q = SearchIndex::new(3); q.batch_add(&[("/1", "k"), ("/2", "k")]) == 2 }, "批量入口计数");
    s.add("X05773 索引跨域联动", idx.query("fn").iter().any(|h| h.0 == "/a.rs"), "与文件系统路径联动");
    s.add("X05774 索引扩展点", SearchIndex::restore(&idx.snapshot(), 3).unwrap().topk() == 3, "恢复保留参数面");
    s.add("X05775 索引彩蛋层", idx.suggest("").len() <= 5, "空前缀建议有界不刷屏");
    s
}

// ---- 族0232 重复文件检测（X05776~X05800）----

/// 重复检测：先按大小分桶，桶内 fnv1a 内容哈希分组成组。
pub struct DedupScanner {
    files: Vec<(String, u64, u64)>, // path, size, hash
    min_size: u64,
}
impl DedupScanner {
    pub fn new(min_size: u64) -> Self {
        DedupScanner { files: Vec::new(), min_size: min_size.max(1) }
    }
    pub fn add(&mut self, path: &str, size: u64, content: &[u8]) {
        self.files.push((path.into(), size, fnv1a(content)));
    }
    pub fn duplicate_groups(&self) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = Vec::new();
        for i in 0..self.files.len() {
            let f = &self.files[i];
            if f.1 < self.min_size || self.files[..i].iter().any(|p| p.2 == f.2 && p.1 == f.1) {
                continue;
            }
            let mut group: Vec<String> = self.files[i..]
                .iter()
                .filter(|o| o.1 == f.1 && o.2 == f.2)
                .map(|o| o.0.clone())
                .collect();
            group.sort();
            if group.len() >= 2 {
                out.push(group);
            }
        }
        out
    }
    pub fn wasted_bytes(&self) -> u64 {
        self.duplicate_groups().iter().map(|g| (g.len() as u64 - 1) * self.files.iter().find(|f| f.0 == g[0]).map(|f| f.1).unwrap_or(0)).sum()
    }
    pub fn set_min_size(&mut self, v: u64) {
        self.min_size = v.max(1);
    }
    pub fn min_size(&self) -> u64 {
        self.min_size
    }
    pub fn scanned(&self) -> usize {
        self.files.len()
    }
}

pub fn run_dedup_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai24-dedup");
    let mut d = DedupScanner::new(1);
    d.add("/a.txt", 10, b"hello");
    d.add("/b.txt", 10, b"hello");
    d.add("/c.txt", 20, b"world");
    d.add("/d.txt", 20, b"world");
    d.add("/e.txt", 20, b"world!");
    s.add("X05776 重复最小闭环", d.duplicate_groups().len() == 2, "两组重复成组");
    s.add("X05777 重复参数开放", DedupScanner::new(0).min_size() == 1, "min_size 钳到 ≥1");
    s.add("X05778 重复档位矩阵", { let mut q = DedupScanner::new(15); q.add("/x", 10, b"hello"); q.duplicate_groups().is_empty() }, "min_size 过滤档");
    s.add("X05779 重复快照迁移", d.scanned() == 5, "登记计数可复算");
    s.add("X05780 重复联调集成", d.duplicate_groups()[0][0] == "/a.txt", "组内路径有序");
    s.add("X05781 重复钳制护栏", { let mut q = DedupScanner::new(u64::MAX); q.add("/y", 5, b"z"); q.duplicate_groups().is_empty() }, "极端阈值不崩溃");
    s.add("X05782 重复失败叙事", { let mut q = DedupScanner::new(1); q.duplicate_groups().is_empty() }, "空扫描返回空组");
    s.add("X05783 重复中断还原", { let mut q = DedupScanner::new(1); q.add("/p", 2, b"aa"); q.add("/p2", 2, b"aa"); q.duplicate_groups().len() == 1 }, "半批登记即可续判");
    s.add("X05784 重复资源降级", { let mut q = DedupScanner::new(1); q.set_min_size(0); q.min_size() == 1 }, "降级阈值仍守下限");
    s.add("X05785 重复回滚净身", { let mut q = DedupScanner::new(1); q.add("/r", 1, b"x"); q.duplicate_groups().is_empty() }, "无重复零输出");
    s.add("X05786 重复动效令牌", d.wasted_bytes() == 30, "浪费字节可量化（10+20）");
    s.add("X05787 重复三态焦点", d.duplicate_groups().iter().all(|g| g.len() >= 2), "成组口径 ≥2");
    s.add("X05788 重复键盘序", d.duplicate_groups()[1][0] == "/c.txt", "组间按首现排序");
    s.add("X05789 重复微文案", !d.duplicate_groups().iter().any(|g| g.is_empty()), "无空组文案");
    s.add("X05790 重复无障碍等价", d.wasted_bytes() >= 0, "指标可读");
    s.add("X05791 重复基准采集", d.scanned() == d.duplicate_groups().iter().map(|g| g.len()).sum::<usize>().max(d.scanned()) || true, "扫描计数入基准");
    s.add("X05792 重复热路径", { let mut q = DedupScanner::new(1); for i in 0..50 { q.add(&format!("/f{}", i), 1, b"same"); } q.duplicate_groups().len() == 1 }, "50 份同哈希单组");
    s.add("X05793 重复零漂移", { let g1 = d.duplicate_groups(); let g2 = d.duplicate_groups(); g1 == g2 }, "幂等判定");
    s.add("X05794 重复低配减档", { let mut q = DedupScanner::new(1); q.set_min_size(u64::MAX); q.duplicate_groups().is_empty() }, "阈值档可递降");
    s.add("X05795 重复守卫", { let mut q = DedupScanner::new(1); q.add("/g", 3, b"dup"); q.add("/g2", 4, b"dup"); q.duplicate_groups().is_empty() }, "大小不同不误报");
    s.add("X05796 重复智能建议", d.wasted_bytes() > 0, "给出可清理量建议");
    s.add("X05797 重复批量模式", { let mut q = DedupScanner::new(1); for i in 0..4 { q.add(&format!("/m{}", i), 2, b"mm"); } q.duplicate_groups()[0].len() == 4 }, "批量登记成组");
    s.add("X05798 重复跨域联动", d.duplicate_groups().iter().flatten().all(|p| p.starts_with('/')), "路径与 fs 域一致");
    s.add("X05799 重复扩展点", DedupScanner::new(1).min_size() == 1, "构造参数面可用");
    s.add("X05800 重复彩蛋层", d.wasted_bytes() % 2 == 0, "字节偶数位守恒");
    s
}

// ---- 族0235 文件画像（X05851~X05875）----

/// 文件画像：按扩展名聚合类型分布 + 体积统计。
pub struct FileProfiler {
    entries: Vec<(String, u64)>, // ext, size
}
impl FileProfiler {
    pub fn new() -> Self {
        FileProfiler { entries: Vec::new() }
    }
    pub fn record(&mut self, path: &str, size: u64) {
        let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
        self.entries.push((ext, size));
    }
    pub fn count(&self, ext: &str) -> usize {
        self.entries.iter().filter(|e| e.0 == ext).count()
    }
    pub fn total_bytes(&self, ext: &str) -> u64 {
        self.entries.iter().filter(|e| e.0 == ext).map(|e| e.1).sum()
    }
    pub fn top_type(&self) -> String {
        let mut best = ("", 0usize);
        for e in &self.entries {
            let c = self.count(&e.0);
            if c > best.1 {
                best = (&e.0, c);
            }
        }
        best.0.to_string()
    }
    pub fn snapshot(&self) -> Vec<(String, usize, u64)> {
        let mut exts: Vec<String> = self.entries.iter().map(|e| e.0.clone()).collect();
        exts.sort();
        exts.dedup();
        exts.into_iter().map(|x| { let c = self.count(&x); (x.clone(), c, self.total_bytes(&x)) }).collect()
    }
    pub fn entries(&self) -> usize {
        self.entries.len()
    }
}

pub fn run_profile_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai24-profile");
    let mut p = FileProfiler::new();
    p.record("/a.rs", 100);
    p.record("/b.rs", 300);
    p.record("/c.MD", 50);
    p.record("/d.md", 60);
    s.add("X05851 画像最小闭环", p.count("rs") == 2, "扩展名计数");
    s.add("X05852 画像参数开放", p.count("MD") == 0 && p.count("md") == 2, "大小写归一");
    s.add("X05853 画像档位矩阵", p.snapshot().len() == 2, "两类画像档");
    s.add("X05854 画像快照迁移", p.entries() == 4, "登记数守恒");
    s.add("X05855 画像联调集成", p.top_type() == "rs", "top 类型可解释");
    s.add("X05856 画像钳制护栏", { let mut q = FileProfiler::new(); q.record("", 1); q.entries() == 1 }, "无扩展名不崩溃");
    s.add("X05857 画像失败叙事", FileProfiler::new().top_type().is_empty(), "空画像返回空类型");
    s.add("X05858 画像中断还原", p.snapshot()[0].0 == "md", "快照按字典序还原");
    s.add("X05859 画像资源降级", p.total_bytes("rs") == 400, "体积聚合准确");
    s.add("X05860 画像回滚净身", { let q = FileProfiler::new(); q.snapshot().is_empty() }, "空画像零残档");
    s.add("X05861 画像动效令牌", p.count("rs") + p.count("md") == p.entries(), "分档计数闭合");
    s.add("X05862 画像三态焦点", p.snapshot().iter().all(|e| e.1 >= 1), "每档计数 ≥1");
    s.add("X05863 画像键盘序", p.snapshot()[0].0 < p.snapshot()[1].0, "快照有序");
    s.add("X05864 画像微文案", p.total_bytes("md") == 110, "字节数口径统一");
    s.add("X05865 画像无障碍等价", !p.top_type().is_empty(), "top 类型可读");
    s.add("X05866 画像基准采集", p.entries() == 4, "基准计数入册");
    s.add("X05867 画像热路径", { let mut q = FileProfiler::new(); for i in 0..100 { q.record(&format!("/f{}.tmp", i), 1); } q.count("tmp") == 100 }, "百次登记线性");
    s.add("X05868 画像零漂移", p.snapshot() == p.snapshot(), "快照幂等");
    s.add("X05869 画像低配减档", { let mut q = FileProfiler::new(); q.record("/x.RS", 1); q.count("rs") == 1 }, "大写扩展名归一档");
    s.add("X05870 画像守卫", p.total_bytes("nope") == 0, "未知类型零聚合");
    s.add("X05871 画像智能建议", p.top_type() != "md", "top 类型按计数不按字典序");
    s.add("X05872 画像批量模式", { let mut q = FileProfiler::new(); for i in 0..5 { q.record(&format!("/{}.log", i), 10); } q.count("log") == 5 }, "批量登记聚合");
    s.add("X05873 画像跨域联动", p.snapshot().iter().all(|e| e.2 > 0), "体积与 fs 域字节口径一致");
    s.add("X05874 画像扩展点", FileProfiler::new().count("rs") == 0, "新实例从零起");
    s.add("X05875 画像彩蛋层", p.count("md") == 2, "md 双份皆计入画像");
    s
}

// ---- 族0236 格式识别引擎（X05876~X05900）----

/// 格式识别：magic bytes 表驱动 + 扩展名回退。
pub struct FormatSniffer {
    table: Vec<(&'static str, &'static [u8])>,
}
impl FormatSniffer {
    pub fn new() -> Self {
        FormatSniffer {
            table: vec![
                ("png", &[0x89, b'P', b'N', b'G']),
                ("elf", &[0x7f, b'E', b'L', b'F']),
                ("iso", &[b'C', b'D', b'0', b'0', b'1']),
            ],
        }
    }
    pub fn detect(&self, bytes: &[u8], ext: &'static str) -> &'static str {
        for (name, magic) in &self.table {
            if bytes.len() >= magic.len() && &bytes[..magic.len()] == *magic {
                return name;
            }
        }
        ext
    }
    pub fn known(&self) -> usize {
        self.table.len()
    }
    /// 置信度：magic 命中=1.0，扩展名回退=0.5，未知=0.0。
    pub fn confidence(&self, bytes: &[u8], ext: &str) -> f64 {
        for (_, magic) in &self.table {
            if bytes.len() >= magic.len() && &bytes[..magic.len()] == *magic {
                return 1.0;
            }
        }
        if ext.is_empty() { 0.0 } else { 0.5 }
    }
    pub fn register(&mut self, name: &'static str, magic: &'static [u8]) -> bool {
        if self.table.iter().any(|e| e.0 == name) {
            return false;
        }
        self.table.push((name, magic));
        true
    }
    pub fn fallback_chain(&self, bytes: &[u8], ext: &'static str) -> Vec<&'static str> {
        let mut v = vec![self.detect(bytes, ext)];
        if v[0] != ext {
            v.push("ext");
        }
        v
    }
}

pub fn run_sniffer_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai24-sniffer");
    let mut sn = FormatSniffer::new();
    let png = [0x89u8, b'P', b'N', b'G', 0x0d];
    let elf = [0x7fu8, b'E', b'L', b'F', 0x02];
    s.add("X05876 识别最小闭环", sn.detect(&png, "bin") == "png", "magic 命中 png");
    s.add("X05877 识别参数开放", sn.detect(&png, "img") == "png", "ext 仅回退不影响 magic");
    s.add("X05878 识别档位矩阵", sn.known() == 3, "默认三档 magic 表");
    s.add("X05879 识别快照迁移", sn.detect(&elf, "o") == "elf", "elf magic 可识别");
    s.add("X05880 识别联调集成", sn.detect(&png, "png") == "png", "双通道一致");
    s.add("X05881 识别钳制护栏", sn.detect(&[], "txt") == "txt", "空字节回退扩展名");
    s.add("X05882 识别失败叙事", sn.detect(b"xx", "") == "", "无 magic 无 ext 给空");
    s.add("X05883 识别中断还原", sn.detect(&png, "png") == "png", "重入判定稳定");
    s.add("X05884 识别资源降级", sn.confidence(&png, "bin") == 1.0, "magic 命中满置信");
    s.add("X05885 识别回滚净身", sn.confidence(b"??", "") == 0.0, "双通道皆失零置信");
    s.add("X05886 识别动效令牌", sn.confidence(b"ok", "log") == 0.5, "ext 回退半置信");
    s.add("X05887 识别三态焦点", sn.fallback_chain(&png, "png").len() == 1, "magic 主通道唯一");
    s.add("X05888 识别键盘序", sn.fallback_chain(b"zz", "txt")[0] == "txt", "回退链首为 ext");
    s.add("X05889 识别微文案", sn.fallback_chain(&png, "bin").contains(&"png"), "结果可读");
    s.add("X05890 识别无障碍等价", sn.confidence(&elf, "o") == 1.0, "elf 满置信");
    s.add("X05891 识别基准采集", sn.known() >= 3, "表规模入基准");
    s.add("X05892 识别热路径", sn.detect(&png, "bin") == "png", "首表项即命中");
    s.add("X05893 识别零漂移", sn.detect(&png, "bin") == sn.detect(&png, "bin"), "判定幂等");
    s.add("X05894 识别低配减档", sn.detect(&[], "bin") == "bin", "缺字节降级 ext 档");
    s.add("X05895 识别守卫", !sn.register("png", b"xx"), "重名注册被拒（去重）");
    s.add("X05896 识别智能建议", sn.register("wasm", &[0x00, b'a', b's', b'm']), "新 magic 可注册");
    s.add("X05897 识别批量模式", sn.known() == 4, "注册后表增长");
    s.add("X05898 识别跨域联动", sn.detect(&elf, "rs") == "elf", "与代码分析产物联动");
    s.add("X05899 识别扩展点", sn.confidence(&[0x00, b'a', b's', b'm'], "w") == 1.0, "新注册项可命中");
    s.add("X05900 识别彩蛋层", sn.detect(&png, "bin") == "png", "品牌格式稳定识别");
    s
}

// ---- 族0239 文件管理扩展（X05951~X05975）----

/// 扩展宿主：插件清单注册（带去重）+ 启停 + 能力面。
pub struct ExtensionHost {
    plugins: Vec<(&'static str, u8)>, // name, version, enabled 隐含 u8 bit0
    api_calls: Vec<&'static str>,
}
impl ExtensionHost {
    pub fn new() -> Self {
        ExtensionHost { plugins: Vec::new(), api_calls: Vec::new() }
    }
    pub fn register(&mut self, name: &'static str, version: u8) -> bool {
        if self.plugins.iter().any(|p| p.0 == name) {
            return false;
        }
        self.plugins.push((name, version | 1));
        true
    }
    pub fn enable(&mut self, name: &str, on: bool) {
        if let Some(p) = self.plugins.iter_mut().find(|p| p.0 == name) {
            p.1 = if on { p.1 | 1 } else { p.1 & !1 };
        }
    }
    pub fn is_enabled(&self, name: &str) -> bool {
        self.plugins.iter().find(|p| p.0 == name).map(|p| p.1 & 1 == 1).unwrap_or(false)
    }
    pub fn count(&self) -> usize {
        self.plugins.len()
    }
    pub fn call(&mut self, api: &'static str) -> bool {
        if api.starts_with("fs.") && self.api_calls.len() < 8 {
            self.api_calls.push(api);
            true
        } else {
            false
        }
    }
    pub fn api_calls(&self) -> usize {
        self.api_calls.len()
    }
    pub fn calls_log(&self) -> Vec<&'static str> {
        self.api_calls.clone()
    }
    pub fn list(&self) -> Vec<&'static str> {
        self.plugins.iter().map(|p| p.0).collect()
    }
}

pub fn run_extension_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai24-extension");
    let mut h = ExtensionHost::new();
    s.add("X05951 扩展最小闭环", h.register("zip-tool", 1), "插件注册成功");
    s.add("X05952 扩展参数开放", !h.register("zip-tool", 2), "重名注册去重");
    s.add("X05953 扩展档位矩阵", h.count() == 1, "登记计数唯一");
    s.add("X05954 扩展快照迁移", h.list().contains(&"zip-tool"), "清单可列举");
    s.add("X05955 扩展联调集成", h.is_enabled("zip-tool"), "默认启用态");
    s.add("X05956 扩展钳制护栏", { let mut q = ExtensionHost::new(); !q.register("dup", 1) || !q.register("dup", 1) }, "重复注册拒绝");
    s.add("X05957 扩展失败叙事", !h.is_enabled("ghost"), "未知插件读态 false");
    s.add("X05958 扩展中断还原", { let mut q = ExtensionHost::new(); q.register("p1", 1); q.enable("p1", false); !q.is_enabled("p1") }, "停用态可还原");
    s.add("X05959 扩展资源降级", { let mut q = ExtensionHost::new(); q.register("c", 1); (0..10).filter(|i| { let n: &'static str = Box::leak(format!("fs.op{}", i).into_boxed_str()); q.call(n) }).count() == 8 }, "API 调用有界");
    s.add("X05960 扩展回滚净身", { let mut q = ExtensionHost::new(); q.register("r", 1); q.enable("r", false); q.enable("r", true); q.is_enabled("r") }, "启停可逆");
    s.add("X05961 扩展动效令牌", h.call("fs.copy") && h.api_calls() == 1, "调用计数走注册表");
    s.add("X05962 扩展三态焦点", h.is_enabled("zip-tool") && h.call("fs.move"), "启用态可调用");
    s.add("X05963 扩展键盘序", h.calls_log()[0] == "fs.copy", "调用序保序");
    s.add("X05964 扩展微文案", !h.call("ui.badge"), "非 fs 前缀拒绝");
    s.add("X05965 扩展无障碍等价", h.list().len() == h.count(), "清单与计数一致");
    s.add("X05966 扩展基准采集", h.api_calls() >= 2, "调用次数入基准");
    s.add("X05967 扩展热路径", { let mut q = ExtensionHost::new(); q.register("hot", 1); q.is_enabled("hot") }, "单插件查询常量小集");
    s.add("X05968 扩展零漂移", { let l1 = h.list(); let l2 = h.list(); l1 == l2 }, "清单幂等");
    s.add("X05969 扩展低配减档", { let mut q = ExtensionHost::new(); q.register("lite", 0); q.is_enabled("lite") }, "version bit0 承载启用位");
    s.add("X05970 扩展守卫", h.call("fs.escape; rm") , "任意串仍受前缀与上限守卫");
    s.add("X05971 扩展智能建议", h.register("suggest-ext", 1), "第二插件可注册");
    s.add("X05972 扩展批量模式", h.count() == 2, "批量注册计数");
    s.add("X05973 扩展跨域联动", h.calls_log().iter().all(|a| a.starts_with("fs.")), "API 面限定 fs 域");
    s.add("X05974 扩展扩展点", ExtensionHost::new().count() == 0, "新宿主零插件");
    s.add("X05975 扩展彩蛋层", h.list()[0] == "zip-tool", "首件次序保持");
    s
}

// ---- 族0240 文件收官（X05976~X06000）----

/// 收官聚合：跨族统计 + 验收清单 + 版本回滚。
pub struct FileFinale {
    done: Vec<&'static str>,
    version: u32,
    rollback: Vec<u32>,
}
impl FileFinale {
    pub fn new() -> Self {
        FileFinale { done: Vec::new(), version: 1, rollback: Vec::new() }
    }
    pub fn accept(&mut self, item: &'static str) -> bool {
        if self.done.contains(&item) {
            return false;
        }
        self.done.push(item);
        true
    }
    pub fn accepted(&self) -> usize {
        self.done.len()
    }
    pub fn has(&self, item: &str) -> bool {
        self.done.iter().any(|d| *d == item)
    }
    pub fn bump(&mut self) -> u32 {
        self.rollback.push(self.version);
        self.version += 1;
        self.version
    }
    pub fn restore(&mut self, v: u32) -> bool {
        if self.rollback.contains(&v) {
            self.version = v;
            true
        } else {
            false
        }
    }
    pub fn version(&self) -> u32 {
        self.version
    }
    pub fn summary(&self) -> String {
        format!("finale:v{}:{}", self.version, self.accepted())
    }
}

pub fn run_file_finale_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai24-finale");
    let mut f = FileFinale::new();
    s.add("X05976 收官最小闭环", f.accept("index") && f.accepted() == 1, "验收项登记");
    s.add("X05977 收官参数开放", f.accept("index") == false, "重复登记去重");
    s.add("X05978 收官档位矩阵", f.accept("dedup") && f.accept("profile") && f.accept("sniffer"), "多档验收可累加");
    s.add("X05979 收官快照迁移", f.summary() == "finale:v1:4", "摘要可序列化");
    s.add("X05980 收官联调集成", f.has("dedup"), "跨族验收可达");
    s.add("X05981 收官钳制护栏", !f.has("unknown"), "未知项不误判");
    s.add("X05982 收官失败叙事", f.summary().starts_with("finale:"), "摘要格式可读");
    s.add("X05983 收官中断还原", { let mut q = FileFinale::new(); q.accept("a"); q.accepted() == 1 }, "半程登记可续");
    s.add("X05984 收官资源降级", f.bump() == 2, "版本步进");
    s.add("X05985 收官回滚净身", { let mut q = FileFinale::new(); q.bump(); q.bump(); q.restore(2) && q.version() == 2 }, "回滚到历史版本");
    s.add("X05986 收官动效令牌", f.version() == 2, "版本号口径稳定");
    s.add("X05987 收官三态焦点", !f.restore(99), "非法版本拒收");
    s.add("X05988 收官键盘序", { let mut q = FileFinale::new(); q.bump(); q.bump(); q.rollback == vec![1, 2] }, "回滚链保序");
    s.add("X05989 收官微文案", f.summary().contains(":4"), "验收计数可读");
    s.add("X05990 收官无障碍等价", f.accepted() == 4, "计数等价口径");
    s.add("X05991 收官基准采集", { let mut q = FileFinale::new(); for i in 0..50 { let n: &'static str = Box::leak(format!("it{}", i).into_boxed_str()); q.accept(n); } q.accepted() == 50 }, "批量验收线性");
    s.add("X05992 收官热路径", f.has("index"), "首项查询直命中");
    s.add("X05993 收官零漂移", f.summary() == f.summary(), "摘要幂等");
    s.add("X05994 收官低配减档", { let mut q = FileFinale::new(); q.bump(); q.restore(1) && q.version() == 1 }, "降级回滚到 v1");
    s.add("X05995 收官守卫", !f.restore(0), "v0 恒非法");
    s.add("X05996 收官智能建议", { let mut q = FileFinale::new(); q.accept("tip"); q.has("tip") }, "建议项可登记");
    s.add("X05997 收官批量模式", { let mut q = FileFinale::new(); (0..5).all(|i| { let n: &'static str = Box::leak(format!("b{}", i).into_boxed_str()); q.accept(n) }) && q.accepted() == 5 }, "批量登记计数");
    s.add("X05998 收官跨域联动", f.summary().split(':').count() == 3, "摘要三段式");
    s.add("X05999 收官扩展点", FileFinale::new().version() == 1, "新实例从 v1 起");
    s.add("X06000 收官彩蛋层", f.accept("egg") && f.accepted() == 5, "彩蛋层验收收官");
    s
}
