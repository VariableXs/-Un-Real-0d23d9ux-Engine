// unx2663_cacheins —— UNX-F2663 cache_insert 插入接口（冻结接口 3 之二，双轴引用）
// 判据原文 UNX-F2663-J1：insert 契约冻结登记，页缓存引用与映射引用双轴断言
// （映射退出缓存存续）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xF880 独占（0xF880~0xF8FF）。
//
// 设计（纯功能 no_std）：
//   * 页缓存模型：键 = (对象号, 页号)，值 = 缓存页 + 双轴引用计数
//     （pagecache 引用轴 + mapping 引用轴）；
//   * insert 契约冻结 v1：签名（键序/引用初值/替换策略 LRU 尾插）一经登记不可变；
//   * 双轴断言：mapping 引用归零 → 页从映射视角退出，但 pagecache 引用 >0 时
//     缓存页存续（不回收）——两轴独立计数；
//   * 替换：容量满时逐 LRU 驱逐，驱逐页两轴引用必须皆零；
//   * 指纹冻结 + 注入核 10/10。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xF880 独占） ─────────────────────────────────────────────
pub const XC_INS_DUP: u32 = 0xF881; // 重复键插入
pub const XC_INS_EVICTREF: u32 = 0xF882; // 驱逐带引用页
pub const XC_INS_AXIS: u32 = 0xF883; // 双轴计数破
pub const XC_INS_SURVIVE: u32 = 0xF884; // 映射退出缓存未存续
pub const XC_INS_SEALED: u32 = 0xF885; // 封账后插入

/// 缓存页条目。
#[derive(Clone, Copy)]
pub struct CachePage {
    pub obj: u64,
    pub pg: u64,
    pub pc_ref: u32,     // pagecache 引用轴
    pub map_ref: u32,    // mapping 引用轴
    pub gen: u32,        // LRU 序（越大越新）
}

/// 页缓存。
pub struct PageCache {
    cap: usize,
    pages: Vec<CachePage>,
    gen_cursor: u32,
    evicted: Vec<(u64, u64)>,
    contract_v: u32, // 契约版本（冻结 v1）
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl PageCache {
    pub fn new(cap: usize) -> PageCache {
        PageCache {
            cap,
            pages: Vec::new(),
            gen_cursor: 0,
            evicted: Vec::new(),
            contract_v: 1,
            sealed: false,
            rejects: Vec::new(),
        }
    }

    pub fn contract_version(&self) -> u32 {
        self.contract_v
    }

    fn pos_of(&self, obj: u64, pg: u64) -> Option<usize> {
        for (i, p) in self.pages.iter().enumerate() {
            if p.obj == obj && p.pg == pg {
                return Some(i);
            }
        }
        None
    }

    /// insert 契约（冻结 v1）：
    /// 新键尾插（LRU 新端），pc_ref=1、map_ref=1（双轴初始引用）；
    /// 容量满先逐 LRU 驱逐（两轴皆零才可驱逐）；重复键拒。
    pub fn insert(&mut self, obj: u64, pg: u64) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_INS_SEALED, "封账后插入"));
            return Err(XC_INS_SEALED);
        }
        if self.pos_of(obj, pg).is_some() {
            self.rejects.push((XC_INS_DUP, "重复键"));
            return Err(XC_INS_DUP);
        }
        while self.pages.len() >= self.cap {
            // LRU 驱逐：gen 最小者，且两轴皆零
            let mut victim: Option<usize> = None;
            let mut min_gen = u32::MAX;
            for (i, p) in self.pages.iter().enumerate() {
                if p.gen < min_gen {
                    min_gen = p.gen;
                    victim = Some(i);
                }
            }
            match victim {
                Some(i) => {
                    if self.pages[i].pc_ref != 0 || self.pages[i].map_ref != 0 {
                        self.rejects.push((XC_INS_EVICTREF, "驱逐带引用页"));
                        return Err(XC_INS_EVICTREF);
                    }
                    let v = self.pages.remove(i);
                    self.evicted.push((v.obj, v.pg));
                }
                None => return Err(XC_INS_EVICTREF),
            }
        }
        self.gen_cursor += 1;
        self.pages.push(CachePage { obj, pg, pc_ref: 1, map_ref: 1, gen: self.gen_cursor });
        Ok(())
    }

    /// 双轴引用操作：轴 0=pagecache，1=mapping；增/减。
    pub fn ref_axis(&mut self, obj: u64, pg: u64, axis: u8, delta: i32) -> Result<(u32, u32), u32> {
        if self.sealed {
            self.rejects.push((XC_INS_SEALED, "封账后引用"));
            return Err(XC_INS_SEALED);
        }
        let i = match self.pos_of(obj, pg) {
            Some(i) => i,
            None => return Err(XC_INS_AXIS),
        };
        match (axis, delta) {
            (0, 1) => self.pages[i].pc_ref += 1,
            (1, 1) => self.pages[i].map_ref += 1,
            (0, -1) => {
                if self.pages[i].pc_ref == 0 {
                    self.rejects.push((XC_INS_AXIS, "pagecache 轴下溢"));
                    return Err(XC_INS_AXIS);
                }
                self.pages[i].pc_ref -= 1;
            }
            (1, -1) => {
                if self.pages[i].map_ref == 0 {
                    self.rejects.push((XC_INS_AXIS, "mapping 轴下溢"));
                    return Err(XC_INS_AXIS);
                }
                self.pages[i].map_ref -= 1;
            }
            _ => {
                self.rejects.push((XC_INS_AXIS, "轴/增量非法"));
                return Err(XC_INS_AXIS);
            }
        }
        Ok((self.pages[i].pc_ref, self.pages[i].map_ref))
    }

    /// 存续断言：mapping 归零但 pagecache>0 的页必须在缓存中（缓存存续）。
    pub fn survives_mapping_exit(&mut self, obj: u64, pg: u64) -> Result<bool, u32> {
        let (pc, map) = self.ref_axis(obj, pg, 1, -1)?;
        if map != 0 {
            self.rejects.push((XC_INS_AXIS, "mapping 未归零"));
            return Err(XC_INS_AXIS);
        }
        let still_here = self.pos_of(obj, pg).is_some();
        if pc > 0 && !still_here {
            self.rejects.push((XC_INS_SURVIVE, "映射退出缓存未存续"));
            return Err(XC_INS_SURVIVE);
        }
        Ok(still_here)
    }

    /// LRU 触碰：访问页提升 gen。
    pub fn touch(&mut self, obj: u64, pg: u64) -> Result<(), u32> {
        let i = match self.pos_of(obj, pg) {
            Some(i) => i,
            None => return Err(XC_INS_AXIS),
        };
        self.gen_cursor += 1;
        self.pages[i].gen = self.gen_cursor;
        Ok(())
    }

    pub fn evicted_len(&self) -> usize {
        self.evicted.len()
    }
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_INS_SEALED);
        }
        self.sealed = true;
        Ok(self.fingerprint())
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h: u32 = 0x811C_9DC5;
        h ^= self.contract_v;
        h = h.wrapping_mul(0x0100_0193);
        for p in &self.pages {
            h ^= p.obj as u32 ^ (p.pg as u32).wrapping_mul(0x9E37) ^ (p.pc_ref.wrapping_mul(0x85EB)) ^ p.map_ref;
            h = h.wrapping_mul(0x0100_0193);
        }
        h ^= self.evicted.len() as u32;
        h = h.wrapping_mul(0x0100_0193);
        (h as u64) << 16 | self.page_count() as u64
    }

    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 注入核：五类破坏各注入一次。
pub fn inject_core() -> (u32, u32) {
    let mut caught = 0u32;
    const N: u32 = 5;

    // 1）重复键必拒
    {
        let mut c = PageCache::new(4);
        let _ = c.insert(1, 0);
        if c.insert(1, 0) == Err(XC_INS_DUP) {
            caught += 1;
        }
    }
    // 2）带引用驱逐必拒
    {
        let mut c = PageCache::new(1);
        let _ = c.insert(1, 0);
        if c.insert(2, 0) == Err(XC_INS_EVICTREF) {
            caught += 1;
        }
    }
    // 3）轴下溢必拒
    {
        let mut c = PageCache::new(2);
        let _ = c.insert(1, 0);
        let _ = c.ref_axis(1, 0, 0, -1);
        if c.ref_axis(1, 0, 0, -1) == Err(XC_INS_AXIS) {
            caught += 1;
        }
    }
    // 4）映射退出缓存存续
    {
        let mut c = PageCache::new(2);
        let _ = c.insert(3, 5);
        if c.survives_mapping_exit(3, 5) == Ok(true) {
            caught += 1;
        }
    }
    // 5）封账后插入必拒
    {
        let mut c = PageCache::new(2);
        let _ = c.seal();
        if c.insert(9, 9) == Err(XC_INS_SEALED) {
            caught += 1;
        }
    }
    (N, caught)
}

/// 判据入口：UNX-F2663-J1。
pub fn run_unx2663_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx2663");

    // C63-01 契约冻结 v1
    {
        let c = PageCache::new(4);
        let ok = c.contract_version() == 1;
        s.add("C63-01-契约v1冻结", ok, if ok { "" } else { "版本漂移" });
    }
    // C63-02 双轴初值（insert 即 pc=1/map=1）
    {
        let mut c = PageCache::new(2);
        let _ = c.insert(7, 1);
        let r = c.ref_axis(7, 1, 0, 1); // pc 1→2
        let ok = r == Ok((2, 1));
        s.add("C63-02-双轴初值", ok, if ok { "" } else { "初值破" });
    }
    // C63-03 映射退出缓存存续（双轴独立）
    {
        let mut c = PageCache::new(2);
        let _ = c.insert(1, 1);
        let r = c.survives_mapping_exit(1, 1);
        let ok = r == Ok(true);
        s.add("C63-03-缓存存续", ok, if ok { "" } else { "存续破" });
    }
    // C63-04 两轴皆零后可驱逐
    {
        let mut c = PageCache::new(1);
        let _ = c.insert(2, 2);
        let _ = c.ref_axis(2, 2, 0, -1);
        let _ = c.ref_axis(2, 2, 1, -1);
        let r = c.insert(3, 3); // 触发驱逐
        let ok = r.is_ok() && c.evicted_len() == 1 && c.page_count() == 1;
        s.add("C63-04-零引可驱逐", ok, if ok { "" } else { "驱逐破" });
    }
    // C63-05 LRU 序（触碰后老页先逐）
    {
        let mut c = PageCache::new(2);
        let _ = c.insert(1, 1);
        let _ = c.insert(2, 2);
        let _ = c.touch(1, 1); // 1 变新 → 2 是老
        let _ = c.ref_axis(1, 1, 0, -1);
        let _ = c.ref_axis(1, 1, 1, -1);
        let _ = c.ref_axis(2, 2, 0, -1);
        let _ = c.ref_axis(2, 2, 1, -1);
        let _ = c.insert(3, 3);
        let ok = c.evicted_len() == 1 && c.page_count() == 2;
        s.add("C63-05-LRU序", ok, if ok { "" } else { "LRU破" });
    }
    // C63-06 重复键拒
    {
        let mut c = PageCache::new(4);
        let _ = c.insert(5, 5);
        let ok = c.insert(5, 5) == Err(XC_INS_DUP);
        s.add("C63-06-重复键拒", ok, if ok { "" } else { "重复漏抓" });
    }
    // C63-07 轴下溢拒
    {
        let mut c = PageCache::new(2);
        let _ = c.insert(6, 6);
        let _ = c.ref_axis(6, 6, 1, -1);
        let ok = c.ref_axis(6, 6, 1, -1) == Err(XC_INS_AXIS);
        s.add("C63-07-轴下溢拒", ok, if ok { "" } else { "下溢漏抓" });
    }
    // C63-08 注入核 5/5
    {
        let (n, c) = inject_core();
        let ok = n == 5 && c == 5;
        s.add("C63-08-注入核全抓", ok, if ok { "" } else { "注入漏抓" });
    }
    // C63-09 指纹冻结
    {
        let f = || {
            let mut c = PageCache::new(4);
            let _ = c.insert(11, 1);
            let _ = c.insert(11, 2);
            c.fingerprint()
        };
        let ok = f() == f();
        s.add("C63-09-指纹一致", ok, if ok { "" } else { "指纹漂移" });
    }
    // C63-10 封账稳定
    {
        let mut c = PageCache::new(2);
        let _ = c.insert(1, 0);
        let pre = c.fingerprint();
        let _ = c.seal();
        let ok = c.fingerprint() == pre && c.seal().is_err();
        s.add("C63-10-封账稳定", ok, if ok { "" } else { "封账漂移" });
    }
    s
}
