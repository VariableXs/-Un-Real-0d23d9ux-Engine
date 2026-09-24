//! 篇 27.2 · 装载缓存与预取指纹（WP-105 · B-2702 优化件）。
//!
//! MD2 篇 27.2："首次成功启动时记录『段清单指纹』（应用的段映射序列：
//! 偏移、长度、目标地址），二次启动按指纹顺序发起异步预读（4MB 粒度）…
//! 指纹失效条件：应用文件哈希变化（升级）即弃用重建。"
//!
//! **正确性论证（这条写进代码注释是 MD2 的明文要求）**：指纹只加速，
//! 不改变装载语义。预取清单只决定**预读引擎提前把哪些块搬进读缓存**；
//! 装载路径的每一个字节仍然照常从文件读出并按段映射落位——错误指纹
//! 的最坏后果是预读了一批用不上的块（浪费 IO），绝不可能让装载器跳过
//! 读取、复用旧内容或接受任何未被本次读取验证过的字节。装载语义的
//! 权威永远是 [`crate::proc::elf`] 的解析与 [`crate::proc::loader`] 的
//! 逐页落位，指纹只是 IO 时序的暗示。
//!
//! 诚实边界：本模块是指纹与记账的**纯逻辑面**——记录的持久化（写盘）
//! 与异步预读引擎（篇 7.3 读缓存承接）挂 WP-203 存储栈；B-2702 的
//! 命中率实测与预算对账在存储栈就位后回补（MD3 WP-105 施工要点明文
//! "基线对账在存储栈就位后回补实测"）。命中/未命中/重建的口径与打点
//! 分解在本包定死，实测只消费这份口径。

use alloc::vec::Vec;

/// 预读粒度（MD2 篇 7.3 对齐：装载段按 4MB 粒度预读）。
pub const PREFETCH_GRAIN: u64 = 4 * 1024 * 1024;

/// 指纹输入用段的最小视图（与 `elf::LoadSegment` 字段同构，避免对上
/// 游结构的依赖蔓延——指纹是装载器的私有记账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SegView {
    pub vaddr: u64,
    pub offset: u64,
    pub filesz: u64,
    pub memsz: u64,
    pub flags: u32,
}

impl SegView {
    pub fn of(s: &crate::proc::elf::LoadSegment) -> SegView {
        SegView {
            vaddr: s.vaddr,
            offset: s.offset,
            filesz: s.filesz,
            memsz: s.memsz,
            flags: s.flags,
        }
    }
}

/// 段清单指纹：FNV-1a 32，与 `bootchain::hash_bytes` 同源（同一质数、
/// 同一偏移基）——全系统一套哈希口径，不让指纹另立门户。
///
/// 输入编码 = 每段按 (vaddr, offset, filesz, memsz, flags) 的小端字节
/// 序列拼接。段清单是镜像内容的确定性函数（链接器输出不变则清单不变），
/// 因此同文件必同指纹、改文件必改清单（改清单而不改文件哈希在算术上
/// 不可能，防御面另见 [`PrefetchCache::lookup`]）。
pub fn seg_fingerprint(segs: &[SegView]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut feed = |b: &[u8]| {
        for &x in b {
            h ^= x as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    };
    for s in segs {
        feed(&s.vaddr.to_le_bytes());
        feed(&s.offset.to_le_bytes());
        feed(&s.filesz.to_le_bytes());
        feed(&s.memsz.to_le_bytes());
        feed(&s.flags.to_le_bytes());
    }
    h
}

/// 一块 4MB 粒度的预读块：目标虚拟区间 + 对应文件区间。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrefetchBlock {
    /// 预读数据落位的目标虚拟地址（块对齐）。
    pub vaddr: u64,
    /// 对应的文件偏移。
    pub file_off: u64,
    /// 本块字节长（≤ PREFETCH_GRAIN；段尾块可短）。
    pub len: u64,
}

/// 把段清单展开成 4MB 粒度的预读清单——"二次启动按指纹顺序发起异步
/// 预读"的"顺序"就是这份清单的顺序（vaddr 升序，段内从低到高）。
pub fn prefetch_plan(segs: &[SegView]) -> Vec<PrefetchBlock> {
    let mut out = Vec::new();
    for s in segs {
        // 预读覆盖段的文件内容区间（BSS——memsz 超出 filesz 的零页部分
        // 不占 IO：预读只搬文件内容，零页由装载器落位）。
        let mut va = s.vaddr;
        let mut off = s.offset;
        let mut done = 0u64;
        while done < s.filesz {
            // 块长：到 4MB 粒度边界或到段尾，取小者。
            let to_boundary = PREFETCH_GRAIN - (off % PREFETCH_GRAIN);
            let block_len = to_boundary.min(s.filesz - done);
            out.push(PrefetchBlock {
                vaddr: va,
                file_off: off,
                len: block_len,
            });
            va += block_len;
            off += block_len;
            done += block_len;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 指纹记录与缓存判定
// ---------------------------------------------------------------------------

/// 一条预取记录：指纹 + 预读清单 + 失效凭据（应用文件哈希）+ 代数。
#[derive(Clone, Debug)]
pub struct PrefetchRecord {
    /// 应用文件哈希（调用方以任意口径给 64 位；变化即弃用重建）。
    pub app_hash: u64,
    pub fingerprint: u32,
    pub blocks: Vec<PrefetchBlock>,
    /// 重建代数——每弃用重建一次 +1（观测"升级即重建"的次数）。
    pub gen: u64,
}

/// 二次启动的判定结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LookupVerdict {
    /// 指纹命中——按 blocks 顺序发起异步预读。
    Hit(Vec<PrefetchBlock>),
    /// 无记录（首次启动）——装载照常，成功后由 record() 建档。
    Miss,
    /// 记录过期，已弃用重建（app_hash 变化=升级，或指纹漂移=防御）。
    /// 携带新指纹，装载照常。
    Rebuilt(u32),
}

/// 命中率与冷启动打点账本（B-707 vxbench 报表的数据源口径）。
///
/// 冷启动打点按"读盘时间与装载时间"分解（MD2 篇 27.2）：read_ms 累计
/// 文件读取耗时，load_ms 累计解析+映射耗时——预读的价值就是把 read_ms
/// 搬到后台，load_ms 里的缺页等待趋近零。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PrefetchStats {
    pub hits: u64,
    pub misses: u64,
    pub rebuilds: u64,
    pub read_ms: u64,
    pub load_ms: u64,
}

impl PrefetchStats {
    pub fn observe_read(&mut self, ms: u64) {
        self.read_ms += ms;
    }
    pub fn observe_load(&mut self, ms: u64) {
        self.load_ms += ms;
    }
    /// 命中率（万分比，避免浮点）：hit*10000 / (hit+miss)。无请求时 None。
    pub fn hit_rate_bp(&self) -> Option<u64> {
        let total = self.hits + self.misses;
        if total == 0 {
            None
        } else {
            Some(self.hits * 10_000 / total)
        }
    }
}

/// 预取缓存判定面。持有一条记录（`Option`——首次启动什么都没有）与
/// 统计账本。单实例，不追求并发（真实并发载体随 WP-203 定型）。
#[derive(Debug, Default)]
pub struct PrefetchCache {
    record: Option<PrefetchRecord>,
    pub stats: PrefetchStats,
}

impl PrefetchCache {
    pub fn new() -> PrefetchCache {
        PrefetchCache::default()
    }

    /// 首次成功启动建档（或同指纹幂等复建）。返回指纹。
    pub fn record(&mut self, app_hash: u64, segs: &[SegView]) -> u32 {
        let fp = seg_fingerprint(segs);
        match &self.record {
            Some(r) if r.app_hash == app_hash && r.fingerprint == fp => {
                // 同应用同指纹：幂等，不涨代数（重复 exec 不算升级）。
            }
            Some(r) => {
                let gen = r.gen + 1;
                self.record = Some(PrefetchRecord {
                    app_hash,
                    fingerprint: fp,
                    blocks: prefetch_plan(segs),
                    gen,
                });
            }
            None => {
                self.record = Some(PrefetchRecord {
                    app_hash,
                    fingerprint: fp,
                    blocks: prefetch_plan(segs),
                    gen: 1,
                });
            }
        }
        fp
    }

    /// 二次启动判定：命中返回预读清单；过期弃用重建；无记录 Miss。
    pub fn lookup(&mut self, app_hash: u64, segs: &[SegView]) -> LookupVerdict {
        let fp = seg_fingerprint(segs);
        match &self.record {
            None => {
                self.stats.misses += 1;
                LookupVerdict::Miss
            }
            Some(r) if r.app_hash != app_hash => {
                // 升级（文件哈希变化）——弃用重建，代数 +1。
                let gen = r.gen + 1;
                self.record = Some(PrefetchRecord {
                    app_hash,
                    fingerprint: fp,
                    blocks: prefetch_plan(segs),
                    gen,
                });
                self.stats.rebuilds += 1;
                self.stats.misses += 1;
                LookupVerdict::Rebuilt(fp)
            }
            Some(r) if r.fingerprint != fp => {
                // 防御面：文件哈希相同但段清单漂移在算术上不应发生；
                // 真发生了（哈希口径弱、清单口径漂移）也只走重建——
                // 正确性论证的兜底：错指纹最多浪费预读。
                let gen = r.gen + 1;
                self.record = Some(PrefetchRecord {
                    app_hash,
                    fingerprint: fp,
                    blocks: prefetch_plan(segs),
                    gen,
                });
                self.stats.rebuilds += 1;
                self.stats.misses += 1;
                LookupVerdict::Rebuilt(fp)
            }
            Some(r) => {
                self.stats.hits += 1;
                LookupVerdict::Hit(r.blocks.clone())
            }
        }
    }

    pub fn record_view(&self) -> Option<(u64, u32, u64)> {
        self.record
            .as_ref()
            .map(|r| (r.app_hash, r.fingerprint, r.gen))
    }
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(vaddr: u64, offset: u64, filesz: u64, memsz: u64, flags: u32) -> SegView {
        SegView { vaddr, offset, filesz, memsz, flags }
    }

    #[test]
    fn fingerprint_is_stable_and_field_sensitive() {
        let a = [seg(0x40_0000, 0, 0x1000, 0x1000, 5)];
        let b = [seg(0x40_0000, 0, 0x1000, 0x1000, 5)];
        assert_eq!(seg_fingerprint(&a), seg_fingerprint(&b), "同清单必须同指纹");

        // 任一字段变一位 → 指纹变（偏移、长度、目标地址都是清单的一部分）。
        for delta in [
            seg(0x40_0001, 0, 0x1000, 0x1000, 5), // vaddr
            seg(0x40_0000, 8, 0x1000, 0x1000, 5), // offset
            seg(0x40_0000, 0, 0x1001, 0x1000, 5), // filesz
            seg(0x40_0000, 0, 0x1000, 0x2000, 5), // memsz
            seg(0x40_0000, 0, 0x1000, 0x1000, 1), // flags
        ] {
            assert_ne!(seg_fingerprint(&a), seg_fingerprint(&[delta]));
        }
        // 段序也是清单的一部分。
        let c = [
            seg(0x40_1000, 0x2000, 0x1000, 0x1000, 5),
            seg(0x40_0000, 0, 0x1000, 0x1000, 5),
        ];
        let d = [
            seg(0x40_0000, 0, 0x1000, 0x1000, 5),
            seg(0x40_1000, 0x2000, 0x1000, 0x1000, 5),
        ];
        assert_ne!(seg_fingerprint(&c), seg_fingerprint(&d));
    }

    #[test]
    fn fingerprint_uses_the_bootchain_fnv_family() {
        // 同源证明：单段 (vaddr=1, offset=2, filesz=3, memsz=4, flags=5)
        // 的指纹必须等于对同一字节流手算的 FNV-1a（bootchain::hash_bytes
        // 的常量：0x811c9dc5 / 0x01000193）。
        let s = [seg(1, 2, 3, 4, 5)];
        let mut bytes = Vec::new();
        for f in [1u64, 2, 3, 4] {
            bytes.extend_from_slice(&f.to_le_bytes());
        }
        bytes.extend_from_slice(&5u32.to_le_bytes());
        let mut h: u32 = 0x811c_9dc5;
        for &x in &bytes {
            h ^= x as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        assert_eq!(seg_fingerprint(&s), h);
    }

    #[test]
    fn prefetch_plan_grains_to_4mb_in_order() {
        let grain = PREFETCH_GRAIN;
        // 9 个粒度长的段（offset 从 0 起）：展开成 9 块满粒度，
        // vaddr/file_off 单调推进。
        let segs = [seg(0x1000_0000, 0, 9 * grain, 9 * grain, 5)];
        let plan = prefetch_plan(&segs);
        assert_eq!(plan.len(), 9);
        for (i, b) in plan.iter().enumerate() {
            assert_eq!(b.len, grain);
            assert_eq!(b.vaddr, 0x1000_0000 + i as u64 * grain);
            assert_eq!(b.file_off, i as u64 * grain);
        }

        // offset 不在粒度边界（0x4000）：首块止于文件 4MB 边界（4MB-16KB），
        // 后续块回到满粒度，尾块收残——块边界始终是文件的 4MB 对齐线。
        let segs_b = [seg(0x1000_0000, 0x4000, 9 * grain, 9 * grain, 5)];
        let plan_b = prefetch_plan(&segs_b);
        assert_eq!(plan_b[0].len, grain - 0x4000);
        assert_eq!(plan_b[1].len, grain);
        assert_eq!(plan_b[0].file_off + plan_b[0].len, grain, "首块止于 4MB 对齐线");
        let total: u64 = plan_b.iter().map(|b| b.len).sum();
        assert_eq!(total, 9 * grain, "块长之和必须等于段文件内容");

        // 短段（3KB）一块收尾；BSS（memsz>filesz）不占预读块——零页由
        // 装载器落位，预读只搬文件内容。
        let segs2 = [seg(0x40_0000, 0x10, 3 * 1024, 16 * 1024, 6)];
        let plan2 = prefetch_plan(&segs2);
        assert_eq!(plan2.len(), 1);
        assert_eq!(plan2[0].len, 3 * 1024);

        // 多段顺序：vaddr 升序。
        let segs3 = [
            seg(0x40_0000, 0, 0x1000, 0x1000, 5),
            seg(0x40_1000, 0x1000, 0x1000, 0x1000, 6),
        ];
        let plan3 = prefetch_plan(&segs3);
        assert_eq!(plan3.len(), 2);
        assert!(plan3[0].vaddr < plan3[1].vaddr);
    }

    #[test]
    fn record_then_lookup_hits_with_the_recorded_plan() {
        let mut c = PrefetchCache::new();
        let segs = [seg(0x40_0000, 0, 0x2000, 0x2000, 5)];
        // 首启：Miss → record 建档。
        assert_eq!(c.lookup(0xAAAA, &segs), LookupVerdict::Miss);
        let fp = c.record(0xAAAA, &segs);
        assert_eq!(c.record_view(), Some((0xAAAA, fp, 1)));
        // 二启：Hit，清单与预读计划一致。
        match c.lookup(0xAAAA, &segs) {
            LookupVerdict::Hit(blocks) => assert_eq!(blocks, prefetch_plan(&segs)),
            other => panic!("expected hit, got {:?}", other),
        }
        assert_eq!(c.stats.hits, 1);
        assert_eq!(c.stats.misses, 1);
        assert_eq!(c.stats.rebuilds, 0);
    }

    #[test]
    fn hash_change_rebuilds_and_bumps_generation() {
        let mut c = PrefetchCache::new();
        let segs = [seg(0x40_0000, 0, 0x1000, 0x1000, 5)];
        c.record(0x1111, &segs);
        // 升级：app_hash 变 → Rebuilt + gen 2 + 计 rebuild。
        let v = c.lookup(0x2222, &segs);
        match v {
            LookupVerdict::Rebuilt(fp) => assert_eq!(fp, seg_fingerprint(&segs)),
            other => panic!("expected rebuild, got {:?}", other),
        }
        assert_eq!(c.stats.rebuilds, 1);
        assert_eq!(c.record_view(), Some((0x2222, seg_fingerprint(&segs), 2)));
        // 新代数下同 hash 再查 → 命中。
        assert!(matches!(c.lookup(0x2222, &segs), LookupVerdict::Hit(_)));
    }

    #[test]
    fn record_same_app_is_idempotent() {
        let mut c = PrefetchCache::new();
        let segs = [seg(0x40_0000, 0, 0x1000, 0x1000, 5)];
        c.record(0x77, &segs);
        c.record(0x77, &segs);
        c.record(0x77, &segs);
        // 同应用同指纹反复记录：代数不涨（重复 exec 不算升级）。
        assert_eq!(c.record_view(), Some((0x77, seg_fingerprint(&segs), 1)));
    }

    #[test]
    fn stats_decompose_read_vs_load_and_rate() {
        let mut s = PrefetchStats::default();
        assert_eq!(s.hit_rate_bp(), None, "无请求时命中率无意义");
        s.observe_read(120);
        s.observe_read(80);
        s.observe_load(45);
        s.hits = 3;
        s.misses = 1;
        assert_eq!(s.read_ms, 200);
        assert_eq!(s.load_ms, 45);
        assert_eq!(s.hit_rate_bp(), Some(7_500), "3/4 = 75.00%");
    }

    #[test]
    fn loader_segments_feed_the_fingerprint_unchanged() {
        // 与真实装载器的接口对账：elf::LoadSegment 经 SegView::of 进指纹，
        // 同一镜像的清单指纹在两次解析间稳定（解析是纯函数）。
        let img = crate::proc::elf::synth_image(0x40_0800, 0x40_0000, 64, 4096, 5, None);
        let e1 = crate::proc::elf::parse(&img).unwrap();
        let e2 = crate::proc::elf::parse(&img).unwrap();
        let v1: Vec<SegView> = e1.segments().iter().map(SegView::of).collect();
        let v2: Vec<SegView> = e2.segments().iter().map(SegView::of).collect();
        assert_eq!(seg_fingerprint(&v1), seg_fingerprint(&v2));

        let mut c = PrefetchCache::new();
        let fp = c.record(0xBEEF, &v1);
        assert!(matches!(c.lookup(0xBEEF, &v2), LookupVerdict::Hit(_)));
        assert_eq!(fp, seg_fingerprint(&v1));
    }
}
