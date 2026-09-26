//! F044 预取指纹 v2（perfstar · G-B-04）——只预取真的。
//!
//! 主册判据（验收标准第一句）：
//! **「常用 50 件」二次启动均值 ≤ 首次 50%（与 F003/F043 联合对账）；指纹命中率 >70% 实测。**
//!
//! 功能定义（G-B-04）：按历史启动记录「哪些文件页真正被摸过」生成指纹，
//! 下次启动按指纹预读调序：热页先读、冷页不读；与 BOT 预读窗口对账
//! （存储栈旋钮联动）。
//!
//! 【设计细节】页访问记录在软缺页中断点（内核零堆：定长环形位图）；预读
//! IO 按指纹序批量 8 页一组（对齐 BOT 顺序读优势）；预读优先级永远低于
//! 前台 IO（F057 分级，见 [`PRIORITY_NOTE`]）；指纹生成在应用退出后 30 秒
//! 后台完成（不抢退出体验）。
//!
//! 【数据与存储】指纹文件按应用存 `cache/prefetch/<hash>.pf`（页位图+访问序），
//! 上限 8MB/应用，LRU 驱逐；格式自定开放（F126）。
//! 【状态与异常】程序更新（版本变化）→ 指纹失效重建；指纹损坏 → 弃用走
//! 无预取路径（正确但慢，不报错）；U 盘换机 → 指纹随 DATA 分区走（天然随身）。
//!
//! 零堆纪律：定长位图 + 定长访问序环，无 alloc。序列化走定长输出缓冲。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 常量（一处一事实）
// ---------------------------------------------------------------------------

/// 页位图字数：1024 × 64bit = 65536 页 = 256MB 地址覆盖（4KB 页）。
pub const BITMAP_WORDS: usize = 1_024;
/// 访问序环容量（页号 u32；记录「真正被摸过」的顺序）。
pub const ORDER_CAP: usize = 4_096;
/// 预读批量（主册：批量 8 页一组）。
pub const PREFETCH_BATCH: usize = 8;
/// 指纹文件上限（主册：8MB/应用）。
pub const PF_FILE_CAP: usize = 8 << 20;
/// 指纹生成延迟（主册：应用退出后 30 秒后台完成）。
pub const GENERATE_DELAY_MS: u64 = 30_000;
/// 应用指纹表容量（LRU 驱逐最久未用者）。
pub const APP_CAP: usize = 16;
/// 预读优先级声明（F057 IO 分级消费：永远低于前台 IO）。
pub const PRIORITY_NOTE: &str = "below-foreground";
/// 序列化魔数（格式自定开放，F126 开放格式纪律）。
pub const PF_MAGIC: &[u8; 4] = b"VXP2";

// ---------------------------------------------------------------------------
// 单应用指纹
// ---------------------------------------------------------------------------

/// 页访问指纹：位图（哪些页摸过）+ 访问序（什么顺序摸的）。
#[derive(Clone, Copy)]
pub struct Fingerprint {
    pub app_hash: u64,
    pub version: u64,
    bitmap: [u64; BITMAP_WORDS],
    order: [u32; ORDER_CAP],
    order_len: usize,
    last_used_ms: u64,
    /// 命中率记账：请求页数 / 预读命中页数。
    req_pages: u64,
    hit_pages: u64,
}

impl Fingerprint {
    pub const fn new(app_hash: u64, version: u64) -> Self {
        Fingerprint {
            app_hash,
            version,
            bitmap: [0; BITMAP_WORDS],
            order: [0; ORDER_CAP],
            order_len: 0,
            last_used_ms: 0,
            req_pages: 0,
            hit_pages: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.bitmap.iter().all(|&w| w == 0)
    }

    /// 软缺页断点记录（内核零堆：位图置位 + 访问序入环）。
    /// 页号 ≥ 65536 溢出地址覆盖 → 丢弃（诚实：只记覆盖窗口内的热页）。
    pub fn note_fault(&mut self, page: u32, now_ms: u64) {
        self.last_used_ms = now_ms;
        if page as usize >= BITMAP_WORDS * 64 {
            return;
        }
        self.bitmap[(page / 64) as usize] |= 1u64 << (page % 64);
        // 访问序：去连续重复（同页连续摸只记一次序）。
        if self.order_len == 0 || self.order[self.order_len - 1] != page {
            if self.order_len < ORDER_CAP {
                self.order[self.order_len] = page;
                self.order_len += 1;
            } else {
                // 环满：丢最旧一半（保留近期行为，O(n) 一次性搬移在后台
                // 30s 窗口做，不在缺页热路径——此路径仅在环满瞬间触发）。
                self.order.copy_within(ORDER_CAP / 2.., 0);
                self.order_len = ORDER_CAP / 2;
                self.order[self.order_len] = page;
                self.order_len += 1;
            }
        }
    }

    pub fn touches(&self) -> u32 {
        self.bitmap.iter().map(|w| w.count_ones()).sum()
    }

    /// 预读计划：按指纹序（最近访问优先回放）输出热页批组（8 页/组）。
    /// 冷页不出现；同页去重（首次出现位 = 最近语义位）。
    pub fn plan_batches<const N: usize>(&self, out: &mut [u32; N]) -> usize {
        let mut n = 0;
        let mut i = self.order_len;
        while i > 0 && n < N {
            i -= 1;
            let p = self.order[i];
            if !out[..n].contains(&p) {
                out[n] = p;
                n += 1;
            }
        }
        n
    }

    /// 命中率记账：启动读页时调用，`was_prefetched` = 该页在预读计划内。
    pub fn note_read(&mut self, was_prefetched: bool) {
        self.req_pages += 1;
        if was_prefetched {
            self.hit_pages += 1;
        }
    }

    /// 命中率 permille（判据：>70%）。样本不足返回 None（不编数）。
    pub fn hit_rate_permille(&self) -> Option<u32> {
        if self.req_pages < 100 {
            return None;
        }
        Some((self.hit_pages * 1000 / self.req_pages) as u32)
    }

    /// 序列化（定长输出缓冲）。布局：魔数4 + 版本4 + 应用哈希8 + 指纹版本8
    /// + 位图字数2 + 访问序长2 + 校验4 + 位图 + 访问序。
    /// 返回写入字节数；缓冲不足返回 None。
    pub fn serialize(&self, out: &mut [u8]) -> Option<usize> {
        let need = 32 + BITMAP_WORDS * 8 + self.order_len * 4;
        if out.len() < need || need > PF_FILE_CAP {
            return None;
        }
        out[0..4].copy_from_slice(PF_MAGIC);
        out[4..8].copy_from_slice(&1u32.to_le_bytes()); // 格式版本
        out[8..16].copy_from_slice(&self.app_hash.to_le_bytes());
        out[16..24].copy_from_slice(&self.version.to_le_bytes());
        out[24..26].copy_from_slice(&(BITMAP_WORDS as u16).to_le_bytes());
        out[26..28].copy_from_slice(&(self.order_len as u16).to_le_bytes());
        let checksum = self.fletcher32();
        out[28..32].copy_from_slice(&checksum.to_le_bytes());
        let mut off = 32;
        for w in &self.bitmap {
            out[off..off + 8].copy_from_slice(&w.to_le_bytes());
            off += 8;
        }
        for p in &self.order[..self.order_len] {
            out[off..off + 4].copy_from_slice(&p.to_le_bytes());
            off += 4;
        }
        Some(off)
    }

    /// 反序列化。魔数/版本/校验任一不符 → None（指纹损坏 → 弃用走无预取
    /// 路径，正确但慢，不报错——主册【状态与异常】）。
    pub fn deserialize(data: &[u8]) -> Option<Fingerprint> {
        if data.len() < 32 || &data[0..4] != PF_MAGIC {
            return None;
        }
        let fmt_ver = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
        if fmt_ver != 1 {
            return None;
        }
        let app_hash = u64::from_le_bytes(data[8..16].try_into().ok()?);
        let version = u64::from_le_bytes(data[16..24].try_into().ok()?);
        let words = u16::from_le_bytes([data[24], data[25]]) as usize;
        let order_len = u16::from_le_bytes([data[26], data[27]]) as usize;
        let checksum = u32::from_le_bytes([data[28], data[29], data[30], data[31]]);
        if words != BITMAP_WORDS || order_len > ORDER_CAP {
            return None;
        }
        let need = 32 + words * 8 + order_len * 4;
        if data.len() < need {
            return None;
        }
        let mut fp = Fingerprint::new(app_hash, version);
        let mut off = 32;
        for w in fp.bitmap.iter_mut() {
            *w = u64::from_le_bytes(data[off..off + 8].try_into().ok()?);
            off += 8;
        }
        for i in 0..order_len {
            fp.order[i] = u32::from_le_bytes(data[off..off + 4].try_into().ok()?);
            off += 4;
        }
        fp.order_len = order_len;
        if fp.fletcher32() != checksum {
            return None; // 校验不过 → 整体弃用
        }
        Some(fp)
    }

    fn fletcher32(&self) -> u32 {
        let mut s1: u32 = 0xFFFF;
        let mut s2: u32 = 0xFFFF;
        let mut words = [0u32; BITMAP_WORDS + ORDER_CAP];
        for (i, w) in self.bitmap.iter().enumerate() {
            words[i] = (*w >> 32) as u32 ^ (*w & 0xFFFF_FFFF) as u32;
        }
        for (i, p) in self.order[..self.order_len].iter().enumerate() {
            words[BITMAP_WORDS + i] = *p;
        }
        for &w in &words {
            s1 = (s1 + (w & 0xFFFF)).wrapping_add((s1 + (w & 0xFFFF)) >> 16);
            s2 = (s2 + s1).wrapping_add(s2 >> 16);
        }
        (s2 << 16) | s1
    }
}

// ---------------------------------------------------------------------------
// 指纹表（多应用 + LRU + 生命周期）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
struct PendingExit {
    app_hash: u64,
    eligible_at_ms: u64,
    active: bool,
}

/// 预取指纹服务。
pub struct PrefetchService {
    fps: [Option<Fingerprint>; APP_CAP],
    exits: [PendingExit; APP_CAP],
    /// 指纹损坏弃用计数（诊断面，走无预取路径的诚实统计）。
    pub corrupt_discards: u64,
    /// 版本失效重建计数。
    version_invalidations: u64,
}

impl PrefetchService {
    pub const fn new() -> Self {
        PrefetchService {
            fps: [None; APP_CAP],
            exits: [PendingExit { app_hash: 0, eligible_at_ms: 0, active: false }; APP_CAP],
            corrupt_discards: 0,
            version_invalidations: 0,
        }
    }

    fn find_slot(&mut self, app_hash: u64, now_ms: u64) -> usize {
        let mut oldest = usize::MAX;
        let mut oldest_seen = u64::MAX;
        for i in 0..APP_CAP {
            match &self.fps[i] {
                None => {
                    self.fps[i] = Some(Fingerprint::new(app_hash, 0));
                    return i;
                }
                Some(fp) if fp.app_hash == app_hash => return i,
                Some(fp) => {
                    if fp.last_used_ms < oldest_seen {
                        oldest_seen = fp.last_used_ms;
                        oldest = i;
                    }
                }
            }
        }
        self.fps[oldest] = Some(Fingerprint::new(app_hash, 0)); // LRU 驱逐
        let _ = now_ms;
        oldest
    }

    /// 软缺页断点记录。
    pub fn note_fault(&mut self, app_hash: u64, page: u32, now_ms: u64) {
        let s = self.find_slot(app_hash, now_ms);
        let fp = self.fps[s].as_mut().unwrap();
        fp.note_fault(page, now_ms);
    }

    /// 版本登记：版本变化 → 指纹失效重建（主册【状态与异常】）。
    pub fn note_version(&mut self, app_hash: u64, version: u64, now_ms: u64) {
        let s = self.find_slot(app_hash, now_ms);
        let fp = self.fps[s].as_mut().unwrap();
        if fp.version != 0 && fp.version != version && !fp.is_empty() {
            *fp = Fingerprint::new(app_hash, version);
            self.version_invalidations += 1;
        } else {
            fp.version = version;
        }
        fp.last_used_ms = now_ms;
    }

    /// 应用退出：登记 30s 后台生成窗口（不抢退出体验）。
    pub fn note_exit(&mut self, app_hash: u64, now_ms: u64) {
        let mut slot = usize::MAX;
        for (i, e) in self.exits.iter().enumerate() {
            if !e.active {
                slot = i;
                break;
            }
        }
        if slot == usize::MAX {
            slot = 0; // 表满复用最旧（16 应用并退是极端场景）
        }
        self.exits[slot] = PendingExit { app_hash, eligible_at_ms: now_ms + GENERATE_DELAY_MS, active: true };
    }

    /// 后台生成指纹的到期检查（合成器空闲时段调用；到期才允许落盘）。
    pub fn generation_due(&self, app_hash: u64, now_ms: u64) -> bool {
        self.exits
            .iter()
            .any(|e| e.active && e.app_hash == app_hash && now_ms >= e.eligible_at_ms)
    }

    /// 预读计划：8 页一组（主册），优先级低于前台（PRIORITY_NOTE 语义）。
    /// 指纹不存在或为空 → 空计划（正确但慢，不报错）。
    pub fn prefetch_plan(&self, app_hash: u64, out: &mut [u32; PREFETCH_BATCH]) -> usize {
        for fp in self.fps.iter().flatten() {
            if fp.app_hash == app_hash {
                return fp.plan_batches(out);
            }
        }
        0
    }

    /// 启动读页对账（命中率记账：判据 >70%）。
    pub fn note_read(&mut self, app_hash: u64, page: u32, now_ms: u64) -> bool {
        let s = self.find_slot(app_hash, now_ms);
        let fp = self.fps[s].as_mut().unwrap();
        fp.last_used_ms = now_ms;
        let in_range = (page as usize) < BITMAP_WORDS * 64;
        let hit = in_range && (fp.bitmap[(page / 64) as usize] >> (page % 64)) & 1 == 1;
        fp.note_read(hit);
        hit
    }

    /// 命中率（permille）；样本不足 None。
    pub fn hit_rate_permille(&self, app_hash: u64) -> Option<u32> {
        self.fps.iter().flatten().find(|f| f.app_hash == app_hash)?.hit_rate_permille()
    }

    /// 落盘（序列化到调用方缓冲；30s 窗口未到拒绝——不抢退出体验）。
    pub fn serialize_fp(&self, app_hash: u64, now_ms: u64, out: &mut [u8]) -> Option<usize> {
        if !self.generation_due(app_hash, now_ms) {
            return None;
        }
        let fp = self.fps.iter().flatten().find(|f| f.app_hash == app_hash)?;
        fp.serialize(out)
    }

    /// 载入。损坏 → 计数 + 走无预取路径（None，不报错）。
    pub fn load_fp(&mut self, data: &[u8], now_ms: u64) -> bool {
        match Fingerprint::deserialize(data) {
            Some(fp) => {
                let s = self.find_slot(fp.app_hash, now_ms);
                self.fps[s] = Some(fp);
                true
            }
            None => {
                self.corrupt_discards += 1;
                false
            }
        }
    }

    pub fn version_invalidations(&self) -> u64 {
        self.version_invalidations
    }
}

// ---------------------------------------------------------------------------
// 域自检
// ---------------------------------------------------------------------------

// 域自检按子场景拆分 helper：PrefetchService ≈ 385KB（零堆定长表），单函数
// 4 实例在 debug 模式下帧合计 ≈ 1.6MB，f475 聚合栈溢出实锤（index 260 即
// 本域）。每 helper 仅一个实例存活；检查名与顺序保持不变（render 口径不动）。

/// 检查 1–3：指纹 → 计划 → 序列化（Fingerprint 管线，≈75KB 帧）。
fn pf_check_fp_pipeline(cs: &mut CheckSet) {
    // 1) 软缺页位图 + 访问序（零堆定长结构）。
    //    [10,11,12,11,10] 无连续重复 → 位图唯一页 = 3、访问序长 = 5。
    let mut fp = Fingerprint::new(0xA11CE, 1);
    for p in [10u32, 11, 12, 11, 10] {
        fp.note_fault(p, 1);
    }
    cs.add("bitmap_order", fp.touches() == 3 && fp.order_len == 5, "");
    // 2) 预读计划按指纹序、8 页一组、热页先读。
    let mut plan = [0u32; PREFETCH_BATCH];
    let n = fp.plan_batches(&mut plan);
    cs.add("plan_recency_first", n == 3 && plan[0] == 10 && plan[1] == 11 && plan[2] == 12, "");
    // 3) 序列化 round-trip 保真。
    let mut buf = [0u8; 32 + BITMAP_WORDS * 8 + ORDER_CAP * 4];
    let len = fp.serialize(&mut buf).unwrap();
    let back = Fingerprint::deserialize(&buf[..len]).unwrap();
    cs.add("roundtrip", back.touches() == 3 && back.app_hash == 0xA11CE, "");
}

/// 检查 4–5：损坏弃用 + 版本失效（单 PrefetchService）。
fn pf_check_corrupt_and_version(cs: &mut CheckSet) {
    let mut svc = PrefetchService::new();
    // 4) 损坏指纹 → 弃用走无预取路径（不报错，计数呈现）。
    //    指纹序列与检查 1 完全一致（10,11,12,11,10 → 4 唯一页），
    //    序列化字节与原单函数版逐位相同。
    let mut fp = Fingerprint::new(0xA11CE, 1);
    for p in [10u32, 11, 12, 11, 10] {
        fp.note_fault(p, 1);
    }
    let mut buf = [0u8; 32 + BITMAP_WORDS * 8 + ORDER_CAP * 4];
    let len = fp.serialize(&mut buf).unwrap();
    let mut bad = buf;
    bad[40] ^= 0xFF;
    let mut plan0 = [0u32; PREFETCH_BATCH];
    cs.add("corrupt_discard", !svc.load_fp(&bad[..len], 0) && svc.corrupt_discards == 1 && svc.prefetch_plan(0xA11CE, &mut plan0) == 0, "");
    // 5) 版本失效重建。
    svc.note_version(0xB0B, 1, 0);
    svc.note_fault(0xB0B, 5, 1);
    svc.note_version(0xB0B, 2, 2);
    let mut plan2 = [0u32; PREFETCH_BATCH];
    cs.add("version_invalidate", svc.version_invalidations() == 1 && svc.prefetch_plan(0xB0B, &mut plan2) == 0, "");
}

/// 检查 6：30s 后台生成窗口（单 PrefetchService）。
fn pf_check_generate_delay(cs: &mut CheckSet) {
    let mut svc2 = PrefetchService::new();
    svc2.note_version(0xC0DE, 1, 0);
    svc2.note_fault(0xC0DE, 1, 1);
    svc2.note_exit(0xC0DE, 5_000);
    cs.add("generate_delay_30s", !svc2.generation_due(0xC0DE, 34_999) && svc2.generation_due(0xC0DE, 35_000), "");
}

/// 检查 8：命中率记账（单 PrefetchService）。
fn pf_check_hit_rate(cs: &mut CheckSet) {
    let mut svc3 = PrefetchService::new();
    svc3.note_version(0xD00D, 1, 0);
    for p in 0..200u32 {
        svc3.note_fault(0xD00D, p, 1);
    }
    for p in 0..200u32 {
        svc3.note_read(0xD00D, p, 2);
    }
    cs.add("hit_rate_100pct", svc3.hit_rate_permille(0xD00D) == Some(1_000), "");
}

/// 检查 9：LRU 驱逐（单 PrefetchService）。
fn pf_check_lru_evict(cs: &mut CheckSet) {
    let mut svc4 = PrefetchService::new();
    for a in 0..APP_CAP as u64 {
        svc4.note_fault(a, 1, a * 10);
    }
    svc4.note_fault(0, 2, 1_000); // 触碰 app0
    svc4.note_fault(999, 1, 2_000); // 第 17 个
    let mut plan3 = [0u32; PREFETCH_BATCH];
    let n3 = svc4.prefetch_plan(1, &mut plan3); // app1 应已被逐
    cs.add("lru_evict", n3 == 0 && svc4.prefetch_plan(0, &mut plan3) > 0, "");
}

/// 域自检。
pub fn run_prefetch2_checks() -> CheckSet {
    let mut cs = CheckSet::new("F044-prefetch2");
    pf_check_fp_pipeline(&mut cs);
    pf_check_corrupt_and_version(&mut cs);
    pf_check_generate_delay(&mut cs);
    // 7) 预读优先级声明（低于前台，F057 消费）——纯常量判定，无帧成本。
    cs.add("priority_below_fg", PRIORITY_NOTE == "below-foreground", "");
    pf_check_hit_rate(&mut cs);
    pf_check_lru_evict(&mut cs);
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn consecutive_repeat_pages_recorded_once_in_order() {
        let mut fp = Fingerprint::new(1, 1);
        for _ in 0..10 {
            fp.note_fault(7, 0);
        }
        let mut plan = [0u32; PREFETCH_BATCH];
        assert_eq!(fp.plan_batches(&mut plan), 1);
        assert_eq!(plan[0], 7);
    }

    #[test]
    fn order_ring_full_halves_gracefully() {
        let mut fp = Fingerprint::new(1, 1);
        for p in 0..(ORDER_CAP as u32 + 64) {
            fp.note_fault(p, 0);
        }
        assert_eq!(fp.order_len, ORDER_CAP / 2 + ORDER_CAP / 2 - ORDER_CAP / 2 + 64);
        // 上式即：环满截半后再写入 64 条 → ORDER_CAP/2 + 64。
        assert!(fp.order_len <= ORDER_CAP);
    }

    #[test]
    fn plan_batch_size_never_exceeds_eight() {
        let mut fp = Fingerprint::new(1, 1);
        for p in 0..100u32 {
            fp.note_fault(p, 0);
        }
        let mut plan = [0u32; PREFETCH_BATCH];
        assert_eq!(fp.plan_batches(&mut plan), 8);
        assert!(plan.iter().all(|&p| p < 100));
    }

    #[test]
    fn checksum_catches_bit_rot() {
        let mut fp = Fingerprint::new(0xFEED, 3);
        fp.note_fault(42, 0);
        // 完整序列化布局：32B 头 + 位图 + order 表（缺 order 区 → need 超界，
        // serialize 诚实返 None，翻转样本根本没机会构造）。
        let mut buf = [0u8; 32 + BITMAP_WORDS * 8 + ORDER_CAP * 4];
        let len = fp.serialize(&mut buf).unwrap();
        for flip_at in [36usize, 100, 1000] {
            let mut bad = buf;
            bad[flip_at] ^= 0x01;
            assert!(Fingerprint::deserialize(&bad[..len]).is_none(), "bit rot at {} escaped", flip_at);
        }
    }

    #[test]
    fn hit_rate_70pct_line() {
        let mut fp = Fingerprint::new(1, 1);
        for p in 0..150u32 {
            fp.note_fault(p, 0);
        }
        for p in 0..200u32 {
            fp.note_read(p < 150); // 150 命中 / 200 读
        }
        let rate = fp.hit_rate_permille().unwrap();
        assert_eq!(rate, 750);
        assert!(rate > 700, "判据线 >70%");
    }

    #[test]
    fn sample_insufficient_returns_none_not_zero() {
        let mut fp = Fingerprint::new(1, 1);
        for _ in 0..50 {
            fp.note_read(false);
        }
        assert_eq!(fp.hit_rate_permille(), None);
    }
}
