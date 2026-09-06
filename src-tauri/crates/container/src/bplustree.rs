//! B+ 树（B-12）——Uxv 容器的 ChunkIndex 与文件表索引。
//!
//! 设计边界（如实）：
//! - 阶 ORDER=32 的内存 B+ 树：内部节点存分隔键，叶子存键值；插入走分裂，
//!   删除只摘除条目不做合并/再平衡（空间利用率换正确性简单，GC 批次 B-15 处理碎片）；
//! - 持久化为整树序列化（DFS 前序），B-12 阶段打开时全量载入；
//!   8TB 规模的节点页缓存（LRU + 按需载入）按蓝图 756 行登记为 B-15 多卷批次的扩展点，
//!   届时通过把 `Node` 换成页句柄实现，本模块公开的树语义不变。

/// 键编码契约。B+ 树序列化只依赖本 trait，新增索引键类型在此登记。
pub trait TreeKey: Ord + Clone {
    fn encode(&self, out: &mut Vec<u8>);
    fn decode(buf: &[u8], pos: &mut usize) -> Option<Self>;
}

/// 值编码契约（同上）。
pub trait TreeVal: Clone {
    fn encode(&self, out: &mut Vec<u8>);
    fn decode(buf: &[u8], pos: &mut usize) -> Option<Self>;
}

// ---------- 内置编码：定长与变长基元 ----------

impl TreeKey for String {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&(self.len() as u32).to_le_bytes());
        out.extend_from_slice(self.as_bytes());
    }
    fn decode(buf: &[u8], pos: &mut usize) -> Option<Self> {
        let n = u32::from_le_bytes(buf.get(*pos..*pos + 4)?.try_into().ok()?) as usize;
        *pos += 4;
        let s = std::str::from_utf8(buf.get(*pos..*pos + n)?).ok()?.to_string();
        *pos += n;
        Some(s)
    }
}

/// 32 字节内容寻址键（BLAKE3 摘要）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HashKey(pub [u8; 32]);

impl std::fmt::Display for HashKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for b in &self.0[..6] {
            write!(f, "{b:02x}")?;
        }
        f.write_str("…")
    }
}

impl TreeKey for HashKey {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.0);
    }
    fn decode(buf: &[u8], pos: &mut usize) -> Option<Self> {
        let mut k = [0u8; 32];
        k.copy_from_slice(buf.get(*pos..*pos + 32)?);
        *pos += 32;
        Some(HashKey(k))
    }
}

impl TreeVal for u64 {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
    fn decode(buf: &[u8], pos: &mut usize) -> Option<Self> {
        let mut b = [0u8; 8];
        b.copy_from_slice(buf.get(*pos..*pos + 8)?);
        *pos += 8;
        Some(u64::from_le_bytes(b))
    }
}

impl TreeKey for u64 {
    fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.to_le_bytes());
    }
    fn decode(buf: &[u8], pos: &mut usize) -> Option<Self> {
        let b = buf.get(*pos..*pos + 8)?;
        *pos += 8;
        Some(u64::from_le_bytes(b.try_into().ok()?))
    }
}

// ---------- 树本体 ----------

/// 阶数：每叶至多 ORDER-1 键。32 ≈ 4KB 页内 32×(32B 键 + 24B 值)，与蓝图页缓存对齐。
const ORDER: usize = 32;

#[derive(Debug, Clone)]
enum Node<K: TreeKey, V: TreeVal> {
    Leaf {
        keys: Vec<K>,
        vals: Vec<V>,
    },
    Internal {
        keys: Vec<K>, // 分隔键：keys[i] < children[i+1] 子树全部键
        children: Vec<Node<K, V>>,
    },
}

#[derive(Debug, Clone)]
pub struct BPlusTree<K: TreeKey, V: TreeVal> {
    root: Node<K, V>,
    len: usize,
}

impl<K: TreeKey, V: TreeVal> Default for BPlusTree<K, V> {
    fn default() -> Self {
        BPlusTree {
            root: Node::Leaf {
                keys: Vec::new(),
                vals: Vec::new(),
            },
            len: 0,
        }
    }
}

/// 分裂结果：新右节点 + 上提分隔键。
struct Split<K: TreeKey, V: TreeVal> {
    sep: K,
    right: Node<K, V>,
}

impl<K: TreeKey, V: TreeVal> Node<K, V> {
    fn leaf(keys: Vec<K>, vals: Vec<V>) -> Self {
        Node::Leaf { keys, vals }
    }

    /// 插入；返回 None = 已存在（值被覆盖），Some = 分裂上提。
    fn insert(&mut self, k: K, v: V) -> Option<Split<K, V>> {
        match self {
            Node::Leaf { keys, vals } => match keys.binary_search(&k) {
                Ok(i) => {
                    vals[i] = v;
                    None
                }
                Err(i) => {
                    keys.insert(i, k);
                    vals.insert(i, v);
                    if keys.len() < ORDER {
                        None
                    } else {
                        let mid = keys.len() / 2;
                        let right = Node::leaf(keys.split_off(mid), vals.split_off(mid));
                        Some(Split {
                            sep: keys.last().expect("split 后左叶必有键").clone(),
                            right,
                        })
                    }
                }
            },
            Node::Internal { keys, children } => {
                let i = lower_bound(keys, &k);
                if let Some(sp) = children[i].insert(k, v) {
                    keys.insert(i, sp.sep);
                    children.insert(i + 1, sp.right);
                    if keys.len() < ORDER {
                        None
                    } else {
                        let mid = keys.len() / 2;
                        let sep = keys.remove(mid);
                        let right_keys = keys.split_off(mid);
                        let right_children = children.split_off(mid + 1);
                        Some(Split {
                            sep,
                            right: Node::Internal {
                                keys: right_keys,
                                children: right_children,
                            },
                        })
                    }
                } else {
                    None
                }
            }
        }
    }

    fn get<'a, Q: ?Sized + Ord>(&'a self, k: &Q) -> Option<&'a V>
    where
        K: std::borrow::Borrow<Q>,
    {
        match self {
            Node::Leaf { keys, vals } => keys
                .binary_search_by(|probe| probe.borrow().cmp(k))
                .ok()
                .map(|i| &vals[i]),
            Node::Internal { keys, children } => {
                let i = lower_bound_by(keys, |x| x.borrow().cmp(k));
                children[i].get(k)
            }
        }
    }

    fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        match self {
            Node::Leaf { keys, vals } => keys.binary_search(k).ok().map(|i| &mut vals[i]),
            Node::Internal { keys, children } => {
                let i = lower_bound(keys, k);
                children[i].get_mut(k)
            }
        }
    }

    fn remove(&mut self, k: &K) -> bool {
        match self {
            Node::Leaf { keys, vals } => {
                if let Ok(i) = keys.binary_search(k) {
                    keys.remove(i);
                    vals.remove(i);
                    true
                } else {
                    false
                }
            }
            Node::Internal { keys, children } => {
                let i = lower_bound(keys, k);
                let removed = children[i].remove(k);
                if removed && is_empty(&children[i]) {
                    // 子树空则收缩：摘除分隔键 keys[i-1]（i=0 时摘 keys[0]）；
                    // 不做兄弟合并（见模块头边界声明）。
                    children.remove(i);
                    if !keys.is_empty() {
                        keys.remove(i.saturating_sub(1).min(keys.len() - 1));
                    }
                }
                removed
            }
        }
    }

    fn iter_into<'a>(&'a self, out: &mut Vec<(K, V)>) {
        match self {
            Node::Leaf { keys, vals } => {
                for (k, v) in keys.iter().zip(vals.iter()) {
                    out.push((k.clone(), v.clone()));
                }
            }
            Node::Internal { children, .. } => {
                for c in children {
                    c.iter_into(out);
                }
            }
        }
    }
}

fn lower_bound_by<K: Ord, F: FnMut(&K) -> std::cmp::Ordering>(keys: &[K], mut cmp: F) -> usize {
    let mut lo = 0usize;
    let mut hi = keys.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if cmp(&keys[mid]) == std::cmp::Ordering::Less {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

fn lower_bound<K: Ord>(keys: &[K], k: &K) -> usize {
    // 第一个 >= k 的分隔键位置；子树 i 覆盖 [keys[i], keys[i+1]) 区间
    //（分隔键 = 左叶最大键且留存于左叶，等值键必须路由到左子树）。
    let mut lo = 0usize;
    let mut hi = keys.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if &keys[mid] < k {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

#[allow(dead_code)]
fn upper_bound<K: Ord>(keys: &[K], k: &K) -> usize {
    // 第一个 > k 的分隔键位置；子树 i 覆盖 (keys[i-1], keys[i]] 区间。
    let mut lo = 0usize;
    let mut hi = keys.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if &keys[mid] <= k {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

fn is_empty<K: TreeKey, V: TreeVal>(n: &Node<K, V>) -> bool {
    match n {
        Node::Leaf { keys, .. } => keys.is_empty(),
        Node::Internal { children, .. } => children.is_empty(),
    }
}

impl<K: TreeKey, V: TreeVal> BPlusTree<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 插入/覆盖。返回 true = 新键。
    pub fn insert(&mut self, k: K, v: V) -> bool {
        let is_new = self.root.get(&k).is_none();
        if is_new {
            self.len += 1;
        }
        match self.root.insert(k, v) {
            None => is_new,
            Some(sp) => {
                self.root = Node::Internal {
                    keys: vec![sp.sep],
                    children: vec![std::mem::replace(
                        &mut self.root,
                        Node::leaf(Vec::new(), Vec::new()),
                    ), sp.right],
                };
                is_new
            }
        }
    }

    pub fn get<Q: ?Sized + Ord>(&self, k: &Q) -> Option<V>
    where
        K: std::borrow::Borrow<Q>,
    {
        self.root.get(k).cloned()
    }

    pub fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        self.root.get_mut(k)
    }

    pub fn contains(&self, k: &K) -> bool {
        self.root.get(k).is_some()
    }

    /// 删除。返回 true = 键存在并被摘除。
    pub fn remove(&mut self, k: &K) -> bool {
        if self.root.remove(k) {
            self.len = self.len.saturating_sub(1);
            true
        } else {
            false
        }
    }

    /// 全量有序快照（校验/序列化/GC 扫描用；数量级由调用方控制）。
    pub fn iter(&self) -> Vec<(K, V)> {
        let mut out = Vec::with_capacity(self.len);
        self.root.iter_into(&mut out);
        out
    }

    // ---------- 序列化（前序 DFS，重建时保序） ----------

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.len as u64).to_le_bytes());
        encode_node(&self.root, &mut out);
        out
    }

    pub fn decode(buf: &[u8]) -> Option<Self> {
        let mut pos = 0usize;
        let len = <u64 as TreeVal>::decode(buf, &mut pos)? as usize;
        let root = decode_node(buf, &mut pos)?;
        pos.checked_add(0).filter(|_| pos == buf.len())?;
        Some(BPlusTree { root, len })
    }
}

fn encode_node<K: TreeKey, V: TreeVal>(n: &Node<K, V>, out: &mut Vec<u8>) {
    match n {
        Node::Leaf { keys, vals } => {
            out.push(0u8);
            out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
            for k in keys {
                k.encode(out);
            }
            for v in vals {
                v.encode(out);
            }
        }
        Node::Internal { keys, children } => {
            out.push(1u8);
            out.extend_from_slice(&(keys.len() as u32).to_le_bytes());
            for k in keys {
                k.encode(out);
            }
            for c in children {
                encode_node(c, out);
            }
        }
    }
}

fn decode_node<K: TreeKey, V: TreeVal>(buf: &[u8], pos: &mut usize) -> Option<Node<K, V>> {
    let tag = *buf.get(*pos)?;
    *pos += 1;
    let n = u32::from_le_bytes(buf.get(*pos..*pos + 4)?.try_into().ok()?) as usize;
    *pos += 4;
    if tag == 0 {
        let mut keys = Vec::with_capacity(n);
        for _ in 0..n {
            keys.push(K::decode(buf, pos)?);
        }
        let mut vals = Vec::with_capacity(n);
        for _ in 0..n {
            vals.push(V::decode(buf, pos)?);
        }
        Some(Node::Leaf { keys, vals })
    } else {
        let mut keys = Vec::with_capacity(n);
        for _ in 0..n {
            keys.push(K::decode(buf, pos)?);
        }
        let mut children = Vec::with_capacity(n + 1);
        for _ in 0..=n {
            children.push(decode_node(buf, pos)?);
        }
        Some(Node::Internal { keys, children })
    }
}

// ---------- 测试：以 BTreeMap 为 oracle 的随机差分 + 序列化往返 ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded(seed: u64) -> impl Iterator<Item = u64> {
        let mut s = seed | 1;
        std::iter::from_fn(move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            Some(s)
        })
    }

    #[test]
    fn random_ops_match_btree_oracle() {
        let mut tree: BPlusTree<String, u64> = BPlusTree::new();
        let mut oracle = std::collections::BTreeMap::new();
        let mut rng = seeded(0x5EED);
        for step in 0..4000 {
            let k = format!("k{:06}", rng.next().unwrap() % 800);
            let op = rng.next().unwrap() % 3;
            match op {
                0 | 1 => {
                    let v = rng.next().unwrap();
                    let is_new = tree.insert(k.clone(), v);
                    assert_eq!(is_new, oracle.insert(k, v).is_none(), "step {step}");
                }
                _ => {
                    let removed = tree.remove(&k);
                    assert_eq!(removed, oracle.remove(&k).is_some(), "step {step}");
                }
            }
            assert_eq!(tree.len(), oracle.len(), "step {step}");
        }
        let dumped = tree.iter();
        assert_eq!(dumped.len(), oracle.len());
        for ((k, v), (ok, ov)) in dumped.iter().zip(oracle.iter()) {
            assert_eq!(k, ok);
            assert_eq!(v, ov);
        }
    }

    #[test]
    fn ordered_iteration_and_split_depth() {
        let mut tree: BPlusTree<u64, u64> = BPlusTree::new();
        for i in 0..2000u64 {
            tree.insert(i, i * 3);
        }
        assert_eq!(tree.len(), 2000);
        for (k, v) in tree.iter() {
            assert_eq!(k, v / 3);
        }
        // 覆盖写不计新键
        assert!(!tree.insert(42u64, 7));
        assert_eq!(tree.get(&42u64), Some(7));
        assert_eq!(tree.get(&1999u64), Some(1999 * 3));
        assert_eq!(tree.get(&2000u64), None);
    }

    #[test]
    fn encode_decode_roundtrip_all_types() {
        let mut ftree: BPlusTree<String, u64> = BPlusTree::new();
        for i in 0..500 {
            ftree.insert(format!("home/file{i}.txt"), i as u64 * 1024);
        }
        let enc = ftree.encode();
        let dec = BPlusTree::<String, u64>::decode(&enc).expect("解码成功");
        assert_eq!(dec.len(), ftree.len());
        assert_eq!(dec.iter(), ftree.iter());

        let mut htree: BPlusTree<HashKey, u64> = BPlusTree::new();
        for i in 0..500u64 {
            let mut k = [0u8; 32];
            k[..8].copy_from_slice(&i.to_le_bytes());
            htree.insert(HashKey(k), i * 77);
        }
        let enc = htree.encode();
        let dec = BPlusTree::<HashKey, u64>::decode(&enc).unwrap();
        assert_eq!(dec.iter(), htree.iter());
    }

    #[test]
    fn decode_rejects_truncated() {
        let mut t: BPlusTree<String, u64> = BPlusTree::new();
        t.insert("a".into(), 1);
        let enc = t.encode();
        assert!(BPlusTree::<String, u64>::decode(&enc[..enc.len() / 2]).is_none());
    }
}
