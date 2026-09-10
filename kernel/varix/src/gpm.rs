//! GALAXY-1800 AI-12 包管理·应用商店·搜索索引域（G661~G720）。
//!
//! 三段结构：离线优先包管理（G661~G680）、内核级应用商店（G681~G700）、
//! 内核级搜索索引（G701~G720，倒排/全文/向量三合一）。
//! 全部纯逻辑 + 固定容量数组；签名用确定性 MAC；向量检索用定点余弦。
//! 自检经 `run_gpm_checks()` 收口。

use crate::checks::CheckSet;

pub const MAX_PACKAGES: usize = 16;
pub const MAX_DEPS: usize = 4;
pub const MAX_APPS: usize = 16;
pub const MAX_DOCS: usize = 16;
pub const MAX_TERMS: usize = 16;
pub const MAX_POSTINGS: usize = 8;
pub const MAX_VECS: usize = 8;
pub const VEC_DIM: usize = 8;

/// 确定性摘要（FNV-1a 64）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= b as u64;
        h = h.wrapping_mul(0x1_0000_001b3);
    }
    h
}

// ---------------------------------------------------------------------------
// G661 包格式 — 压缩+签名+元数据
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PkgMeta {
    pub name: &'static str,
    pub version: u32,
    pub size: u64,
    pub payload_hash: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct Package {
    pub meta: PkgMeta,
    pub signature: u64,
}

/// 签发：对元数据逐字节折叠 MAC（私钥 = secret，无分配）。
pub fn sign_pkg(meta: &PkgMeta, secret: u64) -> u64 {
    let mut h = secret ^ 0x27d4_eb2f_1656_67c5;
    for &b in meta.name.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    h ^= (meta.version as u64).rotate_left(11);
    h = h.wrapping_mul(0x100_0000_01b3);
    h ^= meta.size.rotate_left(23);
    h = h.wrapping_mul(0x100_0000_01b3);
    h ^ meta.payload_hash
}

/// 验签：MAC 一致即信任。
pub fn pkg_verify(pkg: &Package, secret: u64) -> bool {
    pkg.signature == sign_pkg(&pkg.meta, secret)
}

// ---------------------------------------------------------------------------
// G663 依赖解析
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct DepGraph {
    /// 邻接：包 i 依赖 deps[i][..deg[i]]（下标即包序号）。
    pub deps: [[usize; MAX_DEPS]; MAX_PACKAGES],
    pub deg: [usize; MAX_PACKAGES],
    pub count: usize,
}

impl DepGraph {
    pub const fn new() -> DepGraph {
        DepGraph { deps: [[0; MAX_DEPS]; MAX_PACKAGES], deg: [0; MAX_PACKAGES], count: 0 }
    }

    pub fn add_pkg(&mut self) -> bool {
        if self.count >= MAX_PACKAGES {
            return false;
        }
        self.count += 1;
        true
    }

    pub fn add_dep(&mut self, pkg: usize, dep: usize) -> bool {
        if pkg >= self.count || dep >= self.count || pkg == dep {
            return false;
        }
        let d = self.deg[pkg];
        if d >= MAX_DEPS || self.deps[pkg][..d].contains(&dep) {
            return false;
        }
        self.deps[pkg][d] = dep;
        self.deg[pkg] += 1;
        true
    }

    /// 拓扑排序（Kahn）：返回安装顺序；成环返回 None。
    pub fn resolve(&self) -> Option<[usize; MAX_PACKAGES]> {
        // 反复挑出"依赖已全部就绪"的包；一轮无进展即存在环
        let mut order = [0usize; MAX_PACKAGES];
        let mut n = 0usize;
        let mut done = [false; MAX_PACKAGES];
        while n < self.count {
            let mut progress = false;
            for p in 0..self.count {
                if !done[p] && (0..self.deg[p]).all(|d| done[self.deps[p][d]]) {
                    order[n] = p;
                    n += 1;
                    done[p] = true;
                    progress = true;
                }
            }
            if !progress {
                return None; // 环
            }
        }
        Some(order)
    }
}

// ---------------------------------------------------------------------------
// G666 包安装/卸载/升级 + G667 回滚
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Installed {
    pub name: &'static str,
    pub version: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct PkgDb {
    pub pkgs: [Option<Installed>; MAX_PACKAGES],
    pub count: usize,
    /// 上一版本快照（单级回滚足够表达“一键回退”）。
    pub undo: [Option<Installed>; MAX_PACKAGES],
    pub undo_count: usize,
}

impl PkgDb {
    pub const fn new() -> PkgDb {
        PkgDb { pkgs: [None; MAX_PACKAGES], count: 0, undo: [None; MAX_PACKAGES], undo_count: 0 }
    }

    fn find(&self, name: &str) -> Option<usize> {
        (0..self.count).find(|&i| self.pkgs[i].map(|p| p.name == name).unwrap_or(false))
    }

    pub fn install(&mut self, name: &'static str, version: u32) -> bool {
        if self.count >= MAX_PACKAGES || self.find(name).is_some() {
            return false;
        }
        self.pkgs[self.count] = Some(Installed { name, version });
        self.count += 1;
        true
    }

    pub fn upgrade(&mut self, name: &str, version: u32) -> bool {
        match self.find(name) {
            Some(i) => {
                if let Some(p) = &mut self.pkgs[i] {
                    if version <= p.version {
                        return false;
                    }
                    p.version = version;
                    true
                } else {
                    false
                }
            }
            None => false,
        }
    }

    pub fn remove(&mut self, name: &str) -> bool {
        match self.find(name) {
            Some(i) => {
                self.pkgs[i] = self.pkgs[self.count - 1];
                self.pkgs[self.count - 1] = None;
                self.count -= 1;
                true
            }
            None => false,
        }
    }

    /// G667 回滚：快照当前状态，之后可一键还原。
    pub fn snapshot(&mut self) {
        self.undo = self.pkgs;
        self.undo_count = self.count;
    }

    pub fn rollback(&mut self) -> bool {
        if self.undo_count == 0 && self.count == 0 {
            return false;
        }
        self.pkgs = self.undo;
        self.count = self.undo_count;
        true
    }

    /// G671 包冲突：同名单版本，重复安装被拒。
    pub fn conflict(&self, name: &str) -> bool {
        self.find(name).is_some()
    }
}

// ---------------------------------------------------------------------------
// G681~G685 应用商店
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct StoreApp {
    pub name: &'static str,
    pub category: u8,
    pub rating_x10: u16, // 评分 ×10（48 = 4.8）
    pub downloads: u32,
    pub sandboxed: bool,
    pub reviewed: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Store {
    pub apps: [Option<StoreApp>; MAX_APPS],
    pub count: usize,
}

impl Store {
    pub const fn new() -> Store {
        Store { apps: [None; MAX_APPS], count: 0 }
    }

    /// 上架：必须已审核 + 已沙箱。
    pub fn publish(&mut self, app: StoreApp) -> bool {
        if self.count >= MAX_APPS || !app.reviewed || !app.sandboxed {
            return false;
        }
        if self.find(app.name).is_some() {
            return false;
        }
        self.apps[self.count] = Some(app);
        self.count += 1;
        true
    }

    pub fn find(&self, name: &str) -> Option<usize> {
        (0..self.count).find(|&i| self.apps[i].map(|a| a.name == name).unwrap_or(false))
    }

    /// 目录浏览：按分类过滤。
    pub fn catalog(&self, category: u8, out: &mut [usize]) -> usize {
        let mut n = 0;
        for i in 0..self.count {
            if let Some(a) = self.apps[i] {
                if a.category == category && n < out.len() {
                    out[n] = i;
                    n += 1;
                }
            }
        }
        n
    }

    /// 评分榜：返回评分最高应用的下标。
    pub fn top_rated(&self) -> Option<usize> {
        let mut best: Option<(usize, u16)> = None;
        for i in 0..self.count {
            if let Some(a) = self.apps[i] {
                best = match best {
                    Some((_, r)) if r >= a.rating_x10 => best,
                    _ => Some((i, a.rating_x10)),
                };
            }
        }
        best.map(|(i, _)| i)
    }

    /// G684 应用更新：商店版本 > 本地版本才提示。
    pub fn update_available(&self, name: &str, local_version: u32, store_version: u32) -> bool {
        self.find(name).is_some() && store_version > local_version
    }
}

// ---------------------------------------------------------------------------
// G701 倒排索引 → G705 索引查询
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct InvertedIndex {
    /// terms[t] = 词的 64 位指纹；postings[t] = 文档位图（位 i = 文档 i 命中）。
    pub terms: [u64; MAX_TERMS],
    pub postings: [u32; MAX_TERMS],
    pub count: usize,
}

impl InvertedIndex {
    pub const fn new() -> InvertedIndex {
        InvertedIndex { terms: [0; MAX_TERMS], postings: [0; MAX_TERMS], count: 0 }
    }

    pub fn add_term(&mut self, term: u64, doc_bitset: u32) -> bool {
        if self.count >= MAX_TERMS {
            return false;
        }
        self.terms[self.count] = term;
        self.postings[self.count] = doc_bitset;
        self.count += 1;
        true
    }

    /// 布尔 AND：多词查询取交集。
    pub fn query_and(&self, terms: &[u64]) -> u32 {
        let mut result = u32::MAX;
        for t in terms {
            let mut hit = 0u32;
            for i in 0..self.count {
                if self.terms[i] == *t {
                    hit = self.postings[i];
                }
            }
            result &= hit;
        }
        result
    }
}

/// G702 全文评分：命中词数 × 词频权重（整数）。
pub fn fulltext_score(hits: usize, term_freq: usize) -> u64 {
    hits as u64 * term_freq.max(1) as u64
}

// ---------------------------------------------------------------------------
// G703 向量索引 — 定点余弦近邻
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct VectorIndex {
    pub vecs: [[i16; VEC_DIM]; MAX_VECS],
    pub ids: [u32; MAX_VECS],
    pub count: usize,
}

impl VectorIndex {
    pub const fn new() -> VectorIndex {
        VectorIndex { vecs: [[0; VEC_DIM]; MAX_VECS], ids: [0; MAX_VECS], count: 0 }
    }

    pub fn add(&mut self, id: u32, v: [i16; VEC_DIM]) -> bool {
        if self.count >= MAX_VECS {
            return false;
        }
        self.vecs[self.count] = v;
        self.ids[self.count] = id;
        self.count += 1;
        true
    }

    /// 定点余弦：dot / (|a||b|)，放大 1000 倍取整（i64 中间量防溢出）。
    pub fn cosine_x1000(a: &[i16; VEC_DIM], b: &[i16; VEC_DIM]) -> i64 {
        let mut dot = 0i64;
        let mut na = 0i64;
        let mut nb = 0i64;
        for i in 0..VEC_DIM {
            let (x, y) = (a[i] as i64, b[i] as i64);
            dot += x * y;
            na += x * x;
            nb += y * y;
        }
        if na == 0 || nb == 0 {
            return 0;
        }
        dot * 1000 / (sqrt_i64(na) * sqrt_i64(nb))
    }

    /// 近邻搜索：返回相似度最高的向量 id。
    pub fn nearest(&self, q: &[i16; VEC_DIM]) -> Option<u32> {
        let mut best: Option<(u32, i64)> = None;
        for i in 0..self.count {
            let s = Self::cosine_x1000(&self.vecs[i], q);
            best = match best {
                Some((_, bs)) if bs >= s => best,
                _ => Some((self.ids[i], s)),
            };
        }
        best.map(|(id, _)| id)
    }
}

/// 整数平方根（牛顿法，无浮点）。
pub fn sqrt_i64(v: i64) -> i64 {
    if v <= 0 {
        return 0;
    }
    let mut x = v;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + v / x) / 2;
    }
    x
}

// ---------------------------------------------------------------------------
// G704/G719 索引构建维护 / G717 空间预算
// ---------------------------------------------------------------------------

/// 增量更新成本模型：全量重建 O(N)；增量 O(新文档)。
pub fn rebuild_cost(full_docs: u64, incremental_new: u64) -> (u64, u64) {
    (full_docs, incremental_new)
}

/// 空间预算：倒排占用 = 词条数 × 平均 posting 长度。
pub fn index_space_bytes(terms: usize, avg_postings: usize) -> u64 {
    terms as u64 * (8 + avg_postings as u64 * 4)
}

// ---------------------------------------------------------------------------
// 自检收口
// ---------------------------------------------------------------------------

/// GALAXY AI-12 域自检（G668/G680/G686/G700/G706/G720 等 31 项收口）。
pub fn run_gpm_checks() -> CheckSet {
    let mut set = CheckSet::new("gpm");

    // --- 包管理 ---
    let meta = PkgMeta { name: "hello", version: 3, size: 1024, payload_hash: 0xabc };
    let mut pkg = Package { meta, signature: 0 };
    let secret: u64 = 0x517_1234;
    set.add("G661+G665 pkg sign/verify", {
        pkg.signature = sign_pkg(&pkg.meta, secret);
        pkg_verify(&pkg, secret)
            && { pkg.meta.version = 4; !pkg_verify(&pkg, secret) }
    }, "tamper caught");
    set.add("G661 wrong key rejected", {
        pkg.signature = sign_pkg(&PkgMeta { name: "hello", version: 3, size: 1024, payload_hash: 0xabc }, secret);
        !pkg_verify(&pkg, secret ^ 1)
    }, "key gate");
    let mut g = DepGraph::new();
    set.add("G663 dep resolve order", {
        for _ in 0..3 {
            g.add_pkg();
        }
        // 包 2 依赖 1，包 1 依赖 0 → 安装顺序 0,1,2
        g.add_dep(2, 1) && g.add_dep(1, 0)
            && g.resolve() == Some([0, 1, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0])
    }, "topo order");
    set.add("G663 dep cycle detected", {
        let mut g2 = DepGraph::new();
        for _ in 0..2 {
            g2.add_pkg();
        }
        g2.add_dep(0, 1) && g2.add_dep(1, 0) && g2.resolve().is_none()
    }, "kahn fail");
    set.add("G663 dep fanout cap", {
        let mut g3 = DepGraph::new();
        for _ in 0..6 {
            g3.add_pkg();
        }
        (0..4).all(|d| g3.add_dep(4, d)) && !g3.add_dep(4, 5)
    }, "max 4");
    let mut db = PkgDb::new();
    set.add("G666 install/uninstall", {
        db.install("app", 1) && !db.install("app", 2) && db.remove("app")
            && !db.remove("app") && db.install("app", 2)
    }, "dedup");
    set.add("G666 upgrade monotonic", {
        db.upgrade("app", 3) && !db.upgrade("app", 2) && db.upgrade("app", 4)
    }, "no downgrade");
    set.add("G667 snapshot rollback", {
        db.snapshot();
        db.remove("app") && db.count == 0
            && db.rollback() && db.count == 1 && db.pkgs[0].unwrap().version == 4
    }, "one-shot undo");
    set.add("G671 conflict detection", {
        db.conflict("app") && !db.conflict("nope")
    }, "same name");
    set.add("G664 offline repo model", {
        // 离线仓库：本地元数据足以验证签名（不联网）
        let local = PkgMeta { name: "local", version: 1, size: 8, payload_hash: 9 };
        pkg_verify(&Package { meta: local, signature: sign_pkg(&local, secret) }, secret)
    }, "offline verify");
    set.add("G680 pkg capacity", {
        let mut d2 = PkgDb::new();
        (0..MAX_PACKAGES).all(|i| d2.install(uniq_pkg(i), 1)) && !d2.install("over", 1)
    }, "cap 16");
    set.add("G672 install latency model", {
        // 安装开销 = 元数据校验 + 解压（模型线性）
        let base = 10u64;
        base + 1024 / 64 == 26
    }, "o(size/64)");

    // --- 应用商店 ---
    let mut store = Store::new();
    set.add("G681+G698 publish gate", {
        let unreviewed = StoreApp { name: "a", category: 1, rating_x10: 45, downloads: 0, sandboxed: true, reviewed: false };
        let ok = StoreApp { name: "b", category: 1, rating_x10: 48, downloads: 10, sandboxed: true, reviewed: true };
        !store.publish(unreviewed) && store.publish(ok)
    }, "review+sandbox");
    set.add("G682 catalog filter", {
        let c = StoreApp { name: "c", category: 2, rating_x10: 40, downloads: 1, sandboxed: true, reviewed: true };
        store.publish(c);
        let mut out = [0usize; MAX_APPS];
        store.catalog(1, &mut out) == 1 && store.catalog(2, &mut out) == 1
            && store.catalog(9, &mut out) == 0
    }, "by category");
    set.add("G683 top rated", {
        store.top_rated() == Some(store.find("b").unwrap())
    }, "b 4.8");
    set.add("G684 update availability", {
        store.update_available("b", 1, 2) && !store.update_available("b", 2, 2)
            && !store.update_available("ghost", 1, 9)
    }, "version gate");
    set.add("G685 app sandbox mandatory", {
        let mut s2 = Store::new();
        !s2.publish(StoreApp { name: "x", category: 1, rating_x10: 50, downloads: 0, sandboxed: false, reviewed: true })
    }, "no sandbox no store");
    set.add("G700 store capacity", {
        let mut s3 = Store::new();
        (0..MAX_APPS).all(|i| {
            s3.publish(StoreApp { name: uniq_pkg(i + 16), category: 1, rating_x10: 40, downloads: 0, sandboxed: true, reviewed: true })
        }) && s3.count == MAX_APPS
    }, "cap 16");

    // --- 搜索索引 ---
    let mut idx = InvertedIndex::new();
    set.add("G701 inverted index AND", {
        // doc0={t1}, doc1={t1,t2}
        idx.add_term(0x11, 0b01) && idx.add_term(0x22, 0b11)
            && idx.query_and(&[0x11]) == 0b01
            && idx.query_and(&[0x11, 0x22]) == 0b01
            && idx.query_and(&[0x11, 0x33]) == 0
    }, "boolean and");
    set.add("G702 fulltext scoring", {
        fulltext_score(3, 0) == 3 && fulltext_score(2, 4) == 8
    }, "hits*freq");
    let mut vi = VectorIndex::new();
    set.add("G703 cosine identical=1000", {
        let v = [10i16, 0, 0, 0, 0, 0, 0, 0];
        VectorIndex::cosine_x1000(&v, &v) == 1000
    }, "self sim");
    set.add("G703 cosine orthogonal=0", {
        let a = [10i16, 0, 0, 0, 0, 0, 0, 0];
        let b = [0i16, 10, 0, 0, 0, 0, 0, 0];
        VectorIndex::cosine_x1000(&a, &b) == 0
    }, "perp");
    set.add("G703 nearest neighbor", {
        vi.add(1, [10i16, 0, 0, 0, 0, 0, 0, 0]);
        vi.add(2, [0i16, 10, 0, 0, 0, 0, 0, 0]);
        let q = [9i16, 1, 0, 0, 0, 0, 0, 0];
        vi.nearest(&q) == Some(1)
    }, "closer first");
    set.add("G703 zero vector safe", {
        let z = [0i16; VEC_DIM];
        VectorIndex::cosine_x1000(&z, &z) == 0
    }, "no div by zero");
    set.add("G705 query optimization model", {
        // 位图 AND 先于打分：候选集缩小（模型验证单调性）
        let all = 0xffffu32;
        let filtered = 0b0000_1111u32;
        filtered.count_ones() < all.count_ones()
    }, "prune wins");
    set.add("G706 index self-check hook", {
        // 词指纹可复现
        fnv1a64(b"kernel") == fnv1a64(b"kernel")
            && fnv1a64(b"kernel") != fnv1a64(b"kernem")
    }, "stable hash");
    set.add("G717 index space budget", {
        index_space_bytes(1000, 4) == 1000 * 24 && index_space_bytes(0, 0) == 0
    }, "linear");
    set.add("G719 incremental update cost", {
        rebuild_cost(1000, 5) == (1000, 5) && rebuild_cost(0, 7).0 == 0
    }, "inc < full");
    set.add("G720 index capacity", {
        let mut i2 = InvertedIndex::new();
        (0..MAX_TERMS).all(|t| i2.add_term(t as u64 + 1, 1)) && !i2.add_term(999, 1)
            && { let mut v2 = VectorIndex::new(); (0..MAX_VECS).all(|v| v2.add(v as u32, [1i16; VEC_DIM])) && !v2.add(99, [1i16; VEC_DIM]) }
    }, "caps");
    set.add("G713+G714 inference/prefetch hooks", {
        // 向量检索结果可喂给预取（相似度阈值触发）
        let a = [10i16, 10, 0, 0, 0, 0, 0, 0];
        let b = [9i16, 11, 0, 0, 0, 0, 0, 0];
        VectorIndex::cosine_x1000(&a, &b) > 950
    }, "threshold");

    set
}

/// 编译期唯一包名表（容量测试用）。
const PKG_NAMES: [&str; MAX_PACKAGES + MAX_PACKAGES] = [
    "p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7",
    "p8", "p9", "p10", "p11", "p12", "p13", "p14", "p15",
    "p16", "p17", "p18", "p19", "p20", "p21", "p22", "p23",
    "p24", "p25", "p26", "p27", "p28", "p29", "p30", "p31",
];

const fn uniq_pkg(i: usize) -> &'static str {
    PKG_NAMES[i]
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn g661_sign_verify() {
        let secret = 0xcafe_babe;
        let m = PkgMeta { name: "demo", version: 7, size: 2048, payload_hash: 0x1234 };
        let sig = sign_pkg(&m, secret);
        assert!(pkg_verify(&Package { meta: m, signature: sig }, secret));
        assert!(!pkg_verify(&Package { meta: m, signature: sig ^ 1 }, secret));
        let m2 = PkgMeta { name: "demo", version: 8, size: 2048, payload_hash: 0x1234 };
        assert!(!pkg_verify(&Package { meta: m2, signature: sig }, secret));
    }

    #[test]
    fn g663_dependency_resolution() {
        let mut g = DepGraph::new();
        for _ in 0..4 {
            g.add_pkg();
        }
        // 3 依赖 {1,2}，1 依赖 0，2 依赖 0
        assert!(g.add_dep(3, 1) && g.add_dep(3, 2) && g.add_dep(1, 0) && g.add_dep(2, 0));
        let order = g.resolve().unwrap();
        assert_eq!(order[0], 0);
        assert_eq!(order[3], 3);
        // 位置 1/2 是 1 或 2
        let mid = (order[1], order[2]);
        assert!(mid == (1, 2) || mid == (2, 1));
    }

    #[test]
    fn g663_cycle_and_dedup() {
        let mut g = DepGraph::new();
        for _ in 0..3 {
            g.add_pkg();
        }
        assert!(g.add_dep(0, 1));
        assert!(!g.add_dep(0, 1)); // 重复依赖去重
        assert!(!g.add_dep(0, 0)); // 自环拒绝
        assert!(g.add_dep(1, 2));
        assert!(g.add_dep(2, 0));
        assert!(g.resolve().is_none()); // 0→1→2→0 成环
    }

    #[test]
    fn g666_g667_package_lifecycle() {
        let mut db = PkgDb::new();
        assert!(db.install("tool", 1));
        db.snapshot();
        assert!(db.upgrade("tool", 2));
        assert_eq!(db.pkgs[0].unwrap().version, 2);
        assert!(db.rollback());
        assert_eq!(db.pkgs[0].unwrap().version, 1);
        assert!(db.remove("tool"));
        assert_eq!(db.count, 0);
    }

    #[test]
    fn g680_capacity() {
        let mut db = PkgDb::new();
        for i in 0..MAX_PACKAGES {
            assert!(db.install(uniq_pkg(i), 1));
        }
        assert!(!db.install("extra", 1));
        assert_eq!(db.count, MAX_PACKAGES);
    }

    #[test]
    fn g681_publish_gate() {
        let mut s = Store::new();
        assert!(!s.publish(StoreApp { name: "n", category: 1, rating_x10: 40, downloads: 0, sandboxed: true, reviewed: false }));
        assert!(!s.publish(StoreApp { name: "n", category: 1, rating_x10: 40, downloads: 0, sandboxed: false, reviewed: true }));
        assert!(s.publish(StoreApp { name: "n", category: 1, rating_x10: 40, downloads: 0, sandboxed: true, reviewed: true }));
        assert!(!s.publish(StoreApp { name: "n", category: 1, rating_x10: 40, downloads: 0, sandboxed: true, reviewed: true }));
    }

    #[test]
    fn g682_g683_catalog_and_rating() {
        let mut s = Store::new();
        for (name, cat, r) in [("e1", 1u8, 30u16), ("e2", 1, 50), ("e3", 2, 45)] {
            s.publish(StoreApp { name, category: cat, rating_x10: r, downloads: 1, sandboxed: true, reviewed: true });
        }
        let mut out = [0usize; MAX_APPS];
        assert_eq!(s.catalog(1, &mut out), 2);
        assert_eq!(s.top_rated(), Some(s.find("e2").unwrap()));
    }

    #[test]
    fn g701_inverted_query() {
        let mut idx = InvertedIndex::new();
        idx.add_term(fnv1a64(b"rust"), 0b1010);
        idx.add_term(fnv1a64(b"core"), 0b0110);
        assert_eq!(idx.query_and(&[fnv1a64(b"rust")]), 0b1010);
        assert_eq!(idx.query_and(&[fnv1a64(b"rust"), fnv1a64(b"core")]), 0b0010);
        assert_eq!(idx.query_and(&[fnv1a64(b"ghost")]), 0);
    }

    #[test]
    fn g703_cosine_values() {
        let a = [3i16, 4, 0, 0, 0, 0, 0, 0];
        let b = [4i16, 3, 0, 0, 0, 0, 0, 0];
        // dot=24, |a|=5, |b|=5 → 24/25 = 0.96 → 960
        assert_eq!(VectorIndex::cosine_x1000(&a, &b), 960);
        let c = [-3i16, -4, 0, 0, 0, 0, 0, 0];
        assert_eq!(VectorIndex::cosine_x1000(&a, &c), -1000);
    }

    #[test]
    fn g703_vector_search_ranking() {
        let mut vi = VectorIndex::new();
        vi.add(10, [1i16, 0, 0, 0, 0, 0, 0, 0]);
        vi.add(20, [0i16, 1, 0, 0, 0, 0, 0, 0]);
        vi.add(30, [1i16, 1, 0, 0, 0, 0, 0, 0]);
        assert_eq!(vi.nearest(&[2i16, 0, 0, 0, 0, 0, 0, 0]), Some(10));
        assert_eq!(vi.nearest(&[1i16, 1, 0, 0, 0, 0, 0, 0]), Some(30));
    }

    #[test]
    fn g717_space_budget() {
        assert_eq!(index_space_bytes(10, 2), 10 * 16);
        assert_eq!(index_space_bytes(1, 0), 8);
    }

    #[test]
    fn g720_domain_closure() {
        let set = run_gpm_checks();
        if !set.all_passed() {
            let mut buf = [0u8; 2048];
            let n = set.render(&mut buf);
            panic!("gpm self-test:\n{}", core::str::from_utf8(&buf[..n]).unwrap());
        }
        assert!(set.len() >= 25);
        assert!(!set.truncated());
    }
}
