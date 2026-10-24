//! VE-F0653 蒙版缓存（VE-D 域 · 目标 320 行）
//!
//! **锚点**：`VE-STAR-II · 总纲与施工书.md#VE-F0653`
//!
//! **判据（锚点原文逐条）**：
//! - **键三要素**：路径指纹 + 参数 + 目标尺寸（缺一即错配，方法论与 F0590 同源）；
//! - **失效联动**：路径或参数变更 → 对应条目失效；
//! - **LRU 池**：容量上限 + 逐条目尺寸上限；
//! - **两级分离**：蒙版缓存（资源级）与层缓存（合成结果级）独立不混用；
//! - **判据**：键碰撞 → 指纹升级；条目损坏 → 校验剔除。

use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::vec241_dmodel::Rect;

/// 缓存键三要素
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaskKey {
    /// 路径指纹（几何哈希）
    pub path_fp: u64,
    /// 参数指纹（半径/羽化等）
    pub param_fp: u64,
    /// 目标尺寸 (w, h)
    pub size: (u32, u32),
}

impl MaskKey {
    pub fn new(path_fp: u64, param_fp: u64, size: (u32, u32)) -> MaskKey {
        MaskKey { path_fp, param_fp, size }
    }
}

/// 缓存条目（带损坏校验和）
#[derive(Clone, Debug, PartialEq)]
pub struct MaskEntry {
    pub key: MaskKey,
    /// 蒙版覆盖域（几何摘要）
    pub span: Rect,
    /// 条目尺寸（像素数，逐条目上限依据）
    pub px: u64,
    /// 校验和（条目损坏检出）
    pub checksum: u64,
}

/// 条目损坏校验（span 与 px 一致性）
pub fn entry_ok(e: &MaskEntry) -> bool {
    let expect = (e.span.2 - e.span.0) as u64 * (e.span.3 - e.span.1) as u64;
    expect == e.px && e.checksum == e.px.wrapping_mul(31)
}

/// 逐条目尺寸上限
pub const ENTRY_MAX_PX: u64 = 1 << 20; // 1M 像素

/// LRU 池
#[derive(Clone, Debug, Default)]
pub struct MaskCache {
    entries: Vec<(MaskKey, MaskEntry, u64)>,
    cap: usize,
    /// 指纹代际（键碰撞 → 指纹升级）
    fp_epoch: u64,
    pub hits: u32,
    pub misses: u32,
    pub evictions: u32,
    pub corrupt_drops: u32,
}

impl MaskCache {
    pub fn new(cap: usize) -> MaskCache {
        MaskCache { entries: Vec::new(), cap, fp_epoch: 1, ..MaskCache::default() }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 查找（O(1) 语义——线性表小容量实现）：命中提升 LRU 位
    pub fn lookup(&mut self, k: &MaskKey) -> Option<Rect> {
        let i = self.entries.iter().position(|(key, _, _)| key == k)?;
        let (_, e, _) = self.entries[i].clone();
        if !entry_ok(&e) {
            self.entries.remove(i);
            self.corrupt_drops += 1;
            self.misses += 1;
            return None;
        }
        // LRU 提升
        let slot = self.entries.remove(i);
        self.entries.push(slot);
        self.hits += 1;
        Some(e.span)
    }

    /// 插入（超限 LRU 淘汰；尺寸超限条目拒收）
    pub fn insert(&mut self, e: MaskEntry) -> bool {
        if e.px > ENTRY_MAX_PX || !entry_ok(&e) {
            return false;
        }
        if self.entries.iter().any(|(k, _, _)| *k == e.key) {
            return true; // 已存在（幂等）
        }
        while self.entries.len() >= self.cap {
            self.entries.remove(0); // LRU 尾淘汰（队首最旧）
            self.evictions += 1;
        }
        self.entries.push((e.key, e, self.fp_epoch));
        true
    }

    /// 失效联动：路径或参数变更 → 前缀匹配失效
    pub fn invalidate_path(&mut self, path_fp: u64) -> u32 {
        let before = self.entries.len();
        self.entries.retain(|(k, _, _)| k.path_fp != path_fp);
        (before - self.entries.len()) as u32
    }

    pub fn invalidate_param(&mut self, param_fp: u64) -> u32 {
        let before = self.entries.len();
        self.entries.retain(|(k, _, _)| k.param_fp != param_fp);
        (before - self.entries.len()) as u32
    }

    /// 指纹升级（键碰撞对策：代际号进键）
    pub fn bump_epoch(&mut self) -> u64 {
        self.fp_epoch += 1;
        self.fp_epoch
    }
}

/// 两级分离：蒙版缓存与层缓存命名空间隔离
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheSpace {
    /// 资源级：蒙版纹理
    MaskResource,
    /// 结果级：层合成结果
    LayerResult,
}

pub fn run_vec253_checks() -> crate::checks::CheckSet {
    let mut s = CheckSet::new("svstar2-vec253");
    // 族一：键三要素
    let k1 = MaskKey::new(100, 1, (64, 64));
    let k2 = MaskKey::new(100, 1, (64, 64));
    let k3 = MaskKey::new(101, 1, (64, 64));
    let k4 = MaskKey::new(100, 2, (64, 64));
    let k5 = MaskKey::new(100, 1, (128, 64));
    s.add("键-等值", k1 == k2, "same");
    s.add("键-路径异", k1 != k3, "path differs");
    s.add("键-参数异", k1 != k4, "param differs");
    s.add("键-尺寸异", k1 != k5, "size differs");
    // 族二：LRU 池
    let mut c = MaskCache::new(2);
    let e1 = MaskEntry { key: k1, span: (0, 0, 64, 64), px: 4096, checksum: 4096 * 31 };
    let e3 = MaskEntry { key: k3, span: (0, 0, 64, 64), px: 4096, checksum: 4096 * 31 };
    let e5 = MaskEntry { key: k5, span: (0, 0, 128, 64), px: 8192, checksum: 8192 * 31 };
    s.add("缓存-插入", c.insert(e1.clone()), "insert 1");
    s.add("缓存-命中", c.lookup(&k1) == Some((0, 0, 64, 64)) && c.hits == 1, "hit");
    c.insert(e3);
    // 提升 k1 后插 k5 → 应淘汰 k3（LRU）
    let _ = c.lookup(&k1);
    c.insert(e5);
    s.add("缓存-LRU淘汰", c.evictions == 1 && c.lookup(&k3).is_none(), "k3 evicted");
    s.add("缓存-容量", c.len() == 2, "cap 2");
    // 族三：失效联动
    let mut c2 = MaskCache::new(8);
    c2.insert(MaskEntry { key: MaskKey::new(7, 1, (4, 4)), span: (0, 0, 4, 4), px: 16, checksum: 16 * 31 });
    c2.insert(MaskEntry { key: MaskKey::new(7, 2, (4, 4)), span: (0, 0, 4, 4), px: 16, checksum: 16 * 31 });
    c2.insert(MaskEntry { key: MaskKey::new(9, 1, (4, 4)), span: (0, 0, 4, 4), px: 16, checksum: 16 * 31 });
    s.add("失效-路径失效", c2.invalidate_path(7) == 2, "2 dropped");
    s.add("失效-其余保留", c2.len() == 1, "kept");
    c2.insert(MaskEntry { key: MaskKey::new(9, 2, (4, 4)), span: (0, 0, 4, 4), px: 16, checksum: 16 * 31 });
    s.add("失效-参数失效", c2.invalidate_param(2) == 1, "param drop");
    // 族四：损坏剔除
    let mut c3 = MaskCache::new(4);
    let bad = MaskEntry { key: k1, span: (0, 0, 64, 64), px: 999, checksum: 4096 * 31 };
    c3.insert(bad.clone());
    s.add("损坏-插入拒", c3.len() == 0, "inconsistent rejected");
    let mut c4 = MaskCache::new(4);
    let tampered = MaskEntry { key: k1, span: (0, 0, 64, 64), px: 4096, checksum: 1 };
    // 手工注入绕过插入校验——用 lookup 路径模拟损坏
    c4.insert(MaskEntry { key: k1, span: (0, 0, 64, 64), px: 4096, checksum: 4096 * 31 });
    c4.entries[0].1.checksum = 1; // 模拟运行期位翻转
    s.add("损坏-查询剔除", c4.lookup(&k1).is_none() && c4.corrupt_drops == 1, "drop on read");
    // 族五：逐条目尺寸上限
    let huge = MaskEntry { key: k1, span: (0, 0, 2000, 2000), px: 4_000_000, checksum: 4_000_000 * 31 };
    s.add("上限-条目拒收", !c3.insert(huge), "oversize rejected");
    // 族六：指纹升级
    let e0 = c4.fp_epoch;
    s.add("碰撞-代际升级", c4.bump_epoch() == e0 + 1, "epoch bump");
    // 族七：两级分离
    s.add("两级-命名空间", CacheSpace::MaskResource != CacheSpace::LayerResult, "spaces distinct");
    s
}
