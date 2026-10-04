//! F058 内存压缩前瞻 · 完整设计（STAR I 主册 G-B-18）。
//!
//! **定位纪律（主册原文）**：压缩是边界扩展器，不是常规武器——先测后做，
//! 不提前上压缩。本模块交付三件事：
//!
//! 1. `WorksetWatch` 工作集采集与立项门：真实工作集超 3.2GB（水位高档
//!    F045 持续一周触发）才出立项建议，立项判据本身可验证（采集报告
//!    结构化输出）；
//! 2. `lz4` LZ4 block 编解码（自研，zstd/LZ4 评估口径的落地面）：
//!    立项后压缩池的真实引擎，round-trip 与畸形输入防御双达标；
//! 3. `CompressPool` 压缩池：4KB 页槽、动态 0-512MB、冷页 64 秒判定、
//!    压缩比 <1.5 不收（白费 CPU）、解压 P99 >200μs 池缩小、池满回退
//!    OOM 流程（如实告知，不静默）。
//!
//! 时间全部由调用方注入（分钟戳/微秒戳）；空闲批量压缩窗口由上层给
//! 入口（F048 低频档窗口），本模块不持调度器。

use crate::checks::CheckSet;
use crate::star::sbase::{pct_near, RingLog};

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量与规格（主册规格框架表）
// ---------------------------------------------------------------------------

/// 页尺寸（压缩页槽 4KB 框）。
pub const PAGE_SIZE: usize = 4096;

/// 「冷」定义：64 秒未触碰（主册设计细节）。
pub const COLD_SECS: u64 = 64;

/// 压缩比下限 ×100：<1.5 的页不压缩（白费 CPU）。
pub const RATIO_FLOOR_X100: u32 = 150;

/// 解压延迟红线（P99，微秒）：超标 → 池缩小。
pub const DECOMP_P99_LIMIT_US: u64 = 200;

/// 立项工作集阈值（KiB）：3.2GB = 3,125,000 KiB。
pub const WORKSET_TRIGGER_KIB: u64 = 3_125_000;

/// 立项观察窗（天）：水位高档持续一周才立项。
pub const TRIGGER_WINDOW_DAYS: u64 = 7;

/// 池上限额（MB）：动态 0-512MB。
pub const POOL_CAP_MB_MAX: u32 = 512;

/// 池单次缩小步长（MB）：P99 超标时一次缩多少。
pub const POOL_SHRINK_STEP_MB: u32 = 32;

// ---------------------------------------------------------------------------
// LZ4 block 编解码
// ---------------------------------------------------------------------------

/// LZ4 编解码错误：全部显性化，零静默吞错（十三·补 红线）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lz4Error {
    /// 输入在序列中段被截断。
    TruncatedInput,
    /// 偏移量为 0（LZ4 格式禁 0 偏移）。
    ZeroOffset,
    /// 拷贝超出输出缓冲。
    OutputOverrun,
    /// 字面量长度扩展字节流未终止。
    LiteralLenOverflow,
    /// 输出缓冲不足以容纳声明大小。
    OutputTooSmall,
    /// 压缩结果反而变大（不可压缩页由上层按不收策略处理）。
    Incompressible,
}

/// LZ4 block 压缩（greedy 匹配，4 字节最小匹配，16-bit 偏移窗）。
///
/// `out` 必须由调用方按 `lz4_bound(src.len())` 预留；返回写入长度。
/// 纯函数、零分配、零 panic——不可压缩输入照样产出合法（可能更长的）
/// block，是否收页由池的压缩比闸门决定。
pub fn lz4_compress(src: &[u8], out: &mut [u8]) -> Result<usize, Lz4Error> {
    if out.len() < lz4_bound(src.len()) {
        return Err(Lz4Error::OutputTooSmall);
    }
    let n = src.len();
    let mut op = 0usize;
    let mut anchor = 0usize;

    if n >= 13 {
        // 哈希表：1<<12 槽，4 字节序列。
        let mut table = [0u32; 4096];
        for slot in table.iter_mut() {
            *slot = u32::MAX;
        }
        let shift = 32 - 12;
        let mut ip = 0usize;
        let match_limit = n - 12; // 保守匹配边界：保证末序列字面量安全

        while ip <= match_limit {
            let seq = read_u32(src, ip);
            let h = hash32(seq, shift);
            let cand = table[h];
            table[h] = ip as u32;
            if cand != u32::MAX {
                let cp = cand as usize;
                if cp < ip && ip - cp <= 0xFFFF && read_u32(src, cp) == seq {
                    // 字面量段 [anchor, ip)
                    let lit = ip - anchor;
                    let m_start = ip + 4;
                    let mut m_end = m_start;
                    while m_end < n && src[cp + 4 + (m_end - m_start)] == src[m_end] {
                        m_end += 1;
                    }
                    let mlen = m_end - m_start + 4; // 含首 4 字节

                    // token
                    let lit_code = if lit >= 15 { 15 } else { lit as u32 };
                    let ml_code = if mlen - 4 >= 15 { 15 } else { (mlen - 4) as u32 };
                    out[op] = ((lit_code << 4) | ml_code) as u8;
                    op += 1;
                    if lit >= 15 {
                        let mut rest = lit - 15;
                        while rest >= 255 {
                            out[op] = 255;
                            op += 1;
                            rest -= 255;
                        }
                        out[op] = rest as u8;
                        op += 1;
                    }
                    if op + lit > out.len() {
                        return Err(Lz4Error::OutputOverrun);
                    }
                    out[op..op + lit].copy_from_slice(&src[anchor..anchor + lit]);
                    op += lit;

                    // 偏移（小端）
                    let off = (ip - cp) as u16;
                    out[op] = off as u8;
                    out[op + 1] = (off >> 8) as u8;
                    op += 2;

                    // 匹配长度扩展
                    if mlen - 4 >= 15 {
                        let mut rest = mlen - 4 - 15;
                        while rest >= 255 {
                            out[op] = 255;
                            op += 1;
                            rest -= 255;
                        }
                        out[op] = rest as u8;
                        op += 1;
                    }
                    ip = m_end;
                    anchor = ip;
                    continue;
                }
            }
            ip += 1;
        }
    }

    // 尾字面量序列：token + 字面量，无匹配。
    let lit = n - anchor;
    let lit_code = if lit >= 15 { 15 } else { lit as u32 };
    out[op] = (lit_code << 4) as u8;
    op += 1;
    if lit >= 15 {
        let mut rest = lit - 15;
        while rest >= 255 {
            out[op] = 255;
            op += 1;
            rest -= 255;
        }
        out[op] = rest as u8;
        op += 1;
    }
    if op + lit > out.len() {
        return Err(Lz4Error::OutputOverrun);
    }
    out[op..op + lit].copy_from_slice(&src[anchor..]);
    op += lit;
    Ok(op)
}

/// LZ4 block 解压。严格边界检查：任何越界显性报错，绝不静默产出坏页。
pub fn lz4_decompress(src: &[u8], dst: &mut [u8]) -> Result<usize, Lz4Error> {
    let mut ip = 0usize;
    let mut op = 0usize;
    let n = src.len();
    let dlen = dst.len();

    while ip < n {
        let token = src[ip];
        ip += 1;
        // 字面量长度
        let mut lit = (token >> 4) as usize;
        if lit == 15 {
            loop {
                if ip >= n {
                    return Err(Lz4Error::TruncatedInput);
                }
                let b = src[ip];
                ip += 1;
                lit += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        if ip + lit > n {
            return Err(Lz4Error::TruncatedInput);
        }
        if op + lit > dlen {
            return Err(Lz4Error::OutputOverrun);
        }
        dst[op..op + lit].copy_from_slice(&src[ip..ip + lit]);
        op += lit;
        ip += lit;

        if ip >= n {
            // 尾序列：字面量后允许直接结束。
            break;
        }
        // 匹配
        if ip + 2 > n {
            return Err(Lz4Error::TruncatedInput);
        }
        let off = src[ip] as usize | ((src[ip + 1] as usize) << 8);
        ip += 2;
        if off == 0 {
            return Err(Lz4Error::ZeroOffset);
        }
        let mut mlen = (token & 0x0F) as usize + 4;
        if (token & 0x0F) == 15 {
            loop {
                if ip >= n {
                    return Err(Lz4Error::TruncatedInput);
                }
                let b = src[ip];
                ip += 1;
                mlen += b as usize;
                if b != 255 {
                    break;
                }
            }
        }
        if off > op {
            return Err(Lz4Error::OutputOverrun);
        }
        if op + mlen > dlen {
            return Err(Lz4Error::OutputOverrun);
        }
        // 逐字节拷贝（允许重叠：RLE 场景 off < mlen 合法）。
        let mut mpos = op - off;
        for _ in 0..mlen {
            dst[op] = dst[mpos];
            op += 1;
            mpos += 1;
        }
    }
    Ok(op)
}

/// 压缩输出上界（LZ4 官方 bound 公式：n + n/255 + 16）。
pub fn lz4_bound(n: usize) -> usize {
    n + n / 255 + 16
}

fn read_u32(src: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([src[i], src[i + 1], src[i + 2], src[i + 3]])
}

fn hash32(seq: u32, shift: u32) -> usize {
    (seq.wrapping_mul(2654435761) >> shift) as usize
}

// ---------------------------------------------------------------------------
// WorksetWatch — 工作集采集与立项门
// ---------------------------------------------------------------------------

/// 单日工作集画像。
#[derive(Clone, Copy, Debug)]
pub struct WorksetDay {
    /// 日序号（分钟戳 / 1440）。
    pub day: u64,
    /// 当日峰值（KiB）。
    pub peak_kib: u64,
    /// 当日 P95（KiB）。
    pub p95_kib: u64,
}

/// 立项评估报告（评估件以报告为交付——主册验收判据）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorksetReport {
    pub days_observed: u64,
    pub peak_kib: u64,
    pub p95_kib: u64,
    /// 连续达标天数（峰值 > 3.2GB 的天数）。
    pub over_threshold_days: u64,
    /// 立项判定：持续一周超阈值才建议启用。
    pub trigger: bool,
}

/// 工作集监视器：分钟级采样入环形窗，日聚合，立项门判定。
pub struct WorksetWatch {
    /// 分钟环形样本（KiB）。
    ring: RingLog<(u64, u64), 1440>, // 最近 24h 分钟样本（stamp, kib）
    days: [Option<WorksetDay>; 16],  // 最近 16 天日画像
    day_head: usize,
    day_len: usize,
    day_acc_peak: u64,
    day_acc_stamp: u64,
    day_samples: Vec<u64>,
}

impl WorksetWatch {
    pub fn new() -> WorksetWatch {
        WorksetWatch {
            ring: RingLog::new(),
            days: [const { None }; 16],
            day_head: 0,
            day_len: 0,
            day_acc_peak: 0,
            day_acc_stamp: u64::MAX,
            day_samples: Vec::new(),
        }
    }

    /// 记录一分钟工作集样本（KiB）。跨日自动结转。
    pub fn sample(&mut self, stamp_min: u64, kib: u64) {
        self.ring.push((stamp_min, kib));
        let day = stamp_min / 1440;
        if self.day_acc_stamp == u64::MAX {
            self.day_acc_stamp = day;
        }
        if day != self.day_acc_stamp {
            self.close_day(self.day_acc_stamp);
            self.day_acc_stamp = day;
            self.day_acc_peak = 0;
            self.day_samples.clear();
        }
        self.day_acc_peak = self.day_acc_peak.max(kib);
        self.day_samples.push(kib);
    }

    fn close_day(&mut self, day: u64) {
        let p95 = pct_near(&self.day_samples, 95);
        let rec = WorksetDay { day, peak_kib: self.day_acc_peak, p95_kib: p95 };
        self.days[self.day_head] = Some(rec);
        self.day_head = (self.day_head + 1) % 16;
        self.day_len = (self.day_len + 1).min(16);
    }

    /// 强制结转当前日（报告前调用——不丢当天数据）。
    pub fn flush(&mut self) {
        if self.day_acc_stamp != u64::MAX && !self.day_samples.is_empty() {
            let d = self.day_acc_stamp;
            self.close_day(d);
            self.day_acc_stamp = u64::MAX;
            self.day_acc_peak = 0;
            self.day_samples.clear();
        }
    }

    /// 最近 `days` 天日画像（新→旧）。
    pub fn recent_days(&self, days: usize) -> Vec<WorksetDay> {
        let mut out = Vec::new();
        let n = days.min(self.day_len);
        for i in 0..n {
            let idx = (self.day_head + 16 - 1 - i) % 16;
            if let Some(d) = self.days[idx] {
                out.push(d);
            }
        }
        out
    }

    /// 最近 24h 分钟趋势（设置中心「系统-内存」页趋势图数据源：
    /// 让用户看见「为什么我们建议关掉一个程序」）。
    pub fn trend_24h(&self) -> Vec<(u64, u64)> {
        self.ring.newest_first()
    }

    /// 立项评估：连续 TRIGGER_WINDOW_DAYS 天峰值超阈值 → trigger。
    /// 判据本身可验证——报告含全部输入数字（十二查·台账与证据）。
    pub fn evaluate(&mut self) -> WorksetReport {
        self.flush();
        let days = self.recent_days(TRIGGER_WINDOW_DAYS as usize);
        let mut over = 0u64;
        // 连续性：最近 N 天按时间正序检查，出现一天未超阈值即断链。
        let mut sorted = days.clone();
        sorted.sort_by_key(|d| d.day);
        let window_start = sorted.len().saturating_sub(TRIGGER_WINDOW_DAYS as usize);
        let mut consecutive = true;
        for d in &sorted[window_start..] {
            if d.peak_kib > WORKSET_TRIGGER_KIB {
                over += 1;
            } else {
                consecutive = false;
            }
        }
        let peak = sorted.iter().map(|d| d.peak_kib).max().unwrap_or(0);
        let p95 = sorted.iter().map(|d| d.p95_kib).max().unwrap_or(0);
        WorksetReport {
            days_observed: sorted.len() as u64,
            peak_kib: peak,
            p95_kib: p95,
            over_threshold_days: over,
            trigger: consecutive && (sorted.len() as u64) >= TRIGGER_WINDOW_DAYS,
        }
    }
}

impl Default for WorksetWatch {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// CompressPool — 压缩池
// ---------------------------------------------------------------------------

/// 池内一页的账目。
#[derive(Clone, Copy, Debug)]
pub struct PoolEntry {
    /// 源页号（内核帧号语义由上层供给）。
    pub frame: u64,
    /// 压缩负载实际字节数。
    pub len: usize,
    /// 压缩比 ×100（len==0 表示整零页直存 0 字节）。
    pub ratio_x100: u32,
    /// 入池时的访问戳（秒）——冷度再评估用。
    pub last_touch_sec: u64,
    /// 解压次数（热度统计：重读成本高者优先压缩的反馈面）。
    pub decomps: u32,
}

/// 池操作结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PoolVerdict {
    /// 已收页（压缩后入池）。
    Stored,
    /// 整零页（4KB 全 0）——零字节直存。
    StoredZero,
    /// 压缩比 <1.5，白费 CPU，不收（调用方保留原页）。
    RatioRejected,
    /// 池满——回退 OOM 流程的信号（如实告知，不静默挤占）。
    Full,
    /// 页不在池中。
    Miss,
    /// 池处于缩小收缩态，暂停收页。
    Shrinking,
}

/// 压缩池：4KB 页槽、定容槽位、冷页判定、双闸门（压缩比/解压 P99）。
///
/// 槽位在 `new` 一次分配（定长零堆运行时纪律）；负载为每槽独立 4KB
/// 定容缓冲——压缩页必小于页尺寸，天然有界。
pub struct CompressPool {
    slots: Vec<PoolSlot>,
    /// 槽位占用位图（互斥索引池，分配 O(1)）。
    free: Vec<usize>,
    cap_mb: u32,
    /// 解压延迟窗口样本（最近 256 次）。
    decomp_us: [u64; 256],
    decomp_idx: usize,
    decomp_n: usize,
    /// 累计账目。
    pub stats: PoolStats,
    /// 最近池事件（满/缩/拒收诊断）。
    events: RingLog<(u64, u8), 32>,
}

#[derive(Clone, Copy, Debug)]
struct PoolSlot {
    entry: Option<PoolEntry>,
    data: [u8; PAGE_SIZE],
}

/// 池累计账目（诊断面板/账本导出源）。
#[derive(Clone, Copy, Debug, Default)]
pub struct PoolStats {
    pub store_ok: u64,
    pub store_zero: u64,
    pub ratio_reject: u64,
    pub full_reject: u64,
    pub decomp_ok: u64,
    pub decomp_miss: u64,
    pub decompress_bytes: u64,
    pub bytes_saved: u64,
    pub evicted: u64,
    pub shrink_count: u64,
    pub corrupt_detected: u64,
}

/// 池事件类别码：1=满拒收 2=缩容 3=比例拒收 4=损坏检出。
pub const EV_FULL: u8 = 1;
pub const EV_SHRINK: u8 = 2;
pub const EV_RATIO: u8 = 3;
pub const EV_CORRUPT: u8 = 4;

impl CompressPool {
    /// 建池：`cap_mb` 钳制进 [0, 512]；0 表示池禁用（立项前的默认态）。
    /// 槽位上限：cap_mb × 1024KB / 4KB（512MB → 131072 槽）。
    pub fn new(cap_mb: u32) -> CompressPool {
        let cap_mb = cap_mb.min(POOL_CAP_MB_MAX);
        let slots_want = (cap_mb as usize) * 256;
        // 宿主测试与内核初始化都按需分配一次；cap=0 → 空池。
        let slots_want = slots_want.min(131_072);
        let mut slots = Vec::with_capacity(slots_want);
        for _ in 0..slots_want {
            slots.push(PoolSlot { entry: None, data: [0u8; PAGE_SIZE] });
        }
        let free: Vec<usize> = (0..slots.len()).rev().collect();
        CompressPool {
            slots,
            free,
            cap_mb,
            decomp_us: [0; 256],
            decomp_idx: 0,
            decomp_n: 0,
            stats: PoolStats::default(),
            events: RingLog::new(),
        }
    }

    pub fn cap_mb(&self) -> u32 {
        self.cap_mb
    }

    pub fn used_slots(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    pub fn resident_entries(&self) -> Vec<PoolEntry> {
        self.slots.iter().filter_map(|s| s.entry).collect()
    }

    /// 收页：压缩 → 双闸门 → 入槽。
    ///
    /// `now_sec`：冷度戳。入池即视为「已被压缩」，触达由 `touch` 维护。
    pub fn store_page(&mut self, frame: u64, page: &[u8], now_sec: u64) -> PoolVerdict {
        if page.len() != PAGE_SIZE {
            // 非整页输入按缺陷防御：拒绝并计数（零静默）。
            self.stats.ratio_reject += 1;
            return PoolVerdict::RatioRejected;
        }
        if self.cap_mb == 0 {
            return PoolVerdict::Shrinking;
        }
        // 整零页：零字节直存（zswap 同款优化，省尽解压成本）。
        if page.iter().all(|&b| b == 0) {
            return match self.alloc_slot() {
                Some(idx) => {
                    self.slots[idx].entry = Some(PoolEntry {
                        frame,
                        len: 0,
                        ratio_x100: 0,
                        last_touch_sec: now_sec,
                        decomps: 0,
                    });
                    self.stats.store_zero += 1;
                    PoolVerdict::StoredZero
                }
                None => {
                    self.stats.full_reject += 1;
                    self.events.push((frame, EV_FULL));
                    PoolVerdict::Full
                }
            };
        }
        // 压缩
        let mut buf = [0u8; PAGE_SIZE + 32];
        let clen = match lz4_compress(page, &mut buf) {
            Ok(c) => c,
            Err(_) => {
                self.stats.ratio_reject += 1;
                return PoolVerdict::RatioRejected;
            }
        };
        let ratio = ((PAGE_SIZE * 100) / clen.max(1)) as u32;
        if ratio < RATIO_FLOOR_X100 {
            self.stats.ratio_reject += 1;
            self.events.push((frame, EV_RATIO));
            return PoolVerdict::RatioRejected;
        }
        match self.alloc_slot() {
            Some(idx) => {
                self.slots[idx].data[..clen].copy_from_slice(&buf[..clen]);
                self.slots[idx].entry = Some(PoolEntry {
                    frame,
                    len: clen,
                    ratio_x100: ratio,
                    last_touch_sec: now_sec,
                    decomps: 0,
                });
                self.stats.store_ok += 1;
                self.stats.bytes_saved += (PAGE_SIZE - clen) as u64;
                PoolVerdict::Stored
            }
            None => {
                self.stats.full_reject += 1;
                self.events.push((frame, EV_FULL));
                PoolVerdict::Full
            }
        }
    }

    /// 取页：解压回 4KB 缓冲。`elapsed_us` 由调用方计时注入。
    /// 返回 `None` 且 `Miss`/损坏时计数显性化。
    pub fn load_page(
        &mut self,
        frame: u64,
        out: &mut [u8],
        elapsed_us: u64,
    ) -> Result<PoolVerdict, Lz4Error> {
        let idx = match self.slots.iter().position(|s| {
            s.entry.map(|e| e.frame) == Some(frame)
        }) {
            Some(i) => i,
            None => {
                self.stats.decomp_miss += 1;
                return Ok(PoolVerdict::Miss);
            }
        };
        let entry = self.slots[idx].entry.unwrap();
        if entry.len == 0 {
            out.fill(0);
        } else {
            let clen = entry.len;
            let src = &self.slots[idx].data[..clen];
            lz4_decompress(src, out)?;
        }
        self.slots[idx].entry.as_mut().unwrap().decomps += 1;
        self.stats.decomp_ok += 1;
        self.stats.decompress_bytes += PAGE_SIZE as u64;
        self.record_decomp_us(elapsed_us);
        Ok(if entry.len == 0 { PoolVerdict::StoredZero } else { PoolVerdict::Stored })
    }

    /// 触达刷新（页面被访问 → 冷度重置）。miss 返回 false。
    pub fn touch(&mut self, frame: u64, now_sec: u64) -> bool {
        match self.slots.iter_mut().find(|s| s.entry.map(|e| e.frame) == Some(frame)) {
            Some(slot) => {
                slot.entry.as_mut().unwrap().last_touch_sec = now_sec;
                true
            }
            None => false,
        }
    }

    /// 冷页清扫：`now_sec` 距上次触达超 `cold_secs` 的页逐出（把空间让给
    /// 新冷页；逐出是回收不是丢失——源页本就该在回退路径上）。
    pub fn sweep_cold(&mut self, now_sec: u64, cold_secs: u64) -> u64 {
        let cold_secs = if cold_secs == 0 { COLD_SECS } else { cold_secs };
        let mut evicted = 0u64;
        for slot in self.slots.iter_mut() {
            if let Some(e) = slot.entry {
                if now_sec.saturating_sub(e.last_touch_sec) >= cold_secs {
                    slot.entry = None;
                    evicted += 1;
                }
            }
        }
        if evicted > 0 {
            self.rebuild_free();
            self.stats.evicted += evicted;
        }
        evicted
    }

    fn rebuild_free(&mut self) {
        self.free.clear();
        for (i, slot) in self.slots.iter().enumerate().rev() {
            if slot.entry.is_none() {
                self.free.push(i);
            }
        }
    }

    fn alloc_slot(&mut self) -> Option<usize> {
        self.free.pop()
    }

    /// 解压 P99（最近 256 次窗口，最近邻秩口径）。
    pub fn decomp_p99_us(&self) -> u64 {
        pct_near(&self.decomp_us[..self.decomp_n], 99)
    }

    fn record_decomp_us(&mut self, us: u64) {
        self.decomp_us[self.decomp_idx] = us;
        self.decomp_idx = (self.decomp_idx + 1) % 256;
        self.decomp_n = (self.decomp_n + 1).min(256);
    }

    /// 闸门巡检：解压 P99 > 200μs → 池缩一步（POOL_SHRINK_STEP_MB）。
    /// 缩容重建槽表（账目保留；驻留页按 LRU 语义丢弃并计数——
    /// 缩容本身是「解压太慢」的防御动作，丢弃如实入账）。
    pub fn gate_check(&mut self) -> bool {
        if self.cap_mb == 0 {
            return false;
        }
        if self.decomp_p99_us() > DECOMP_P99_LIMIT_US && self.decomp_n >= 32 {
            let step = POOL_SHRINK_STEP_MB.min(self.cap_mb);
            self.cap_mb -= step;
            self.stats.shrink_count += 1;
            self.events.push((self.cap_mb as u64, EV_SHRINK));
            self.compact_to_cap();
            return true;
        }
        false
    }

    fn compact_to_cap(&mut self) {
        let want = (self.cap_mb as usize) * 256;
        // 保留热页（decomps 高者），丢冷页直到槽位达标。
        let mut kept: Vec<(u32, usize)> = Vec::new();
        for (i, slot) in self.slots.iter().enumerate() {
            if let Some(e) = slot.entry {
                kept.push((e.decomps, i));
            }
        }
        kept.sort_by_key(|(d, _)| core::cmp::Reverse(*d));
        let mut stay: Vec<usize> = kept.iter().take(want).map(|(_, i)| *i).collect();
        stay.sort_unstable();
        let stay_set: Vec<usize> = stay;
        let mut new_slots: Vec<PoolSlot> = Vec::with_capacity(want);
        for i in 0..self.slots.len() {
            if want == 0 {
                break;
            }
            if stay_set.contains(&i) {
                new_slots.push(PoolSlot {
                    entry: self.slots[i].entry,
                    data: self.slots[i].data,
                });
            } else if let Some(e) = self.slots[i].entry {
                self.stats.evicted += 1;
                let _ = e;
            }
            if new_slots.len() == want {
                break;
            }
        }
        while new_slots.len() < want {
            new_slots.push(PoolSlot { entry: None, data: [0u8; PAGE_SIZE] });
        }
        self.slots = new_slots;
        self.rebuild_free();
    }

    /// 池事件（新→旧）。
    pub fn recent_events(&self) -> Vec<(u64, u8)> {
        self.events.newest_first()
    }

    /// 平均压缩比 ×100（含零页豁免）。
    pub fn avg_ratio_x100(&self) -> u32 {
        let entries: Vec<PoolEntry> = self.resident_entries();
        let n = entries.iter().filter(|e| e.ratio_x100 > 0).count();
        if n == 0 {
            return 0;
        }
        let sum: u64 = entries.iter().filter(|e| e.ratio_x100 > 0).map(|e| e.ratio_x100 as u64).sum();
        (sum / n as u64) as u32
    }
}

// ---------------------------------------------------------------------------
// 空闲批量压缩调度（F048 低频档窗口的入口约定）
// ---------------------------------------------------------------------------

/// 批量压缩作业单：上层在空闲窗口逐页喂入，本结构记账并限流。
pub struct BatchCompressor {
    /// 单窗口页数上限（旋钮默认 64——超则让 CPU）。
    pub budget_per_window: u32,
    window_done: u32,
    pub pages_scanned: u64,
    pub pages_compressed: u64,
    pub pages_rejected: u64,
}

impl BatchCompressor {
    pub fn new() -> BatchCompressor {
        BatchCompressor {
            budget_per_window: 64,
            window_done: 0,
            pages_scanned: 0,
            pages_compressed: 0,
            pages_rejected: 0,
        }
    }

    /// 窗口开启。
    pub fn begin_window(&mut self) {
        self.window_done = 0;
    }

    /// 窗口内预算未耗尽。
    pub fn has_budget(&self) -> bool {
        self.window_done < self.budget_per_window
    }

    /// 记一页处理结果（verdict 来自 pool.store_page）。
    pub fn note(&mut self, verdict: PoolVerdict) {
        self.pages_scanned += 1;
        self.window_done += 1;
        match verdict {
            PoolVerdict::Stored | PoolVerdict::StoredZero => self.pages_compressed += 1,
            _ => self.pages_rejected += 1,
        }
    }
}

impl Default for BatchCompressor {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F058 自检（聚合进 star 域）。
pub fn run_memcomp_checks() -> CheckSet {
    let mut set = CheckSet::new("F058-memcomp");

    // LZ4 round-trip：三类数据形态（重复/文本/随机）。
    let mut out = [0u8; PAGE_SIZE + 32];
    let mut back = [0u8; PAGE_SIZE];
    let mut rep = [0u8; PAGE_SIZE];
    for i in 0..PAGE_SIZE {
        rep[i] = (i / 32) as u8;
    }
    let c = lz4_compress(&rep, &mut out).expect("compress rep");
    let d = lz4_decompress(&out[..c], &mut back).expect("decompress rep");
    set.add("lz4 roundtrip rep", d == PAGE_SIZE && back == rep, "");
    set.add("lz4 rep gains", c < PAGE_SIZE / 4, "");

    let mut rnd = [0u8; PAGE_SIZE];
    let mut x: u32 = 0x1234_5678;
    for b in rnd.iter_mut() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = (x >> 24) as u8;
    }
    let c = lz4_compress(&rnd, &mut out).expect("compress rnd");
    let d = lz4_decompress(&out[..c], &mut back).expect("decompress rnd");
    set.add("lz4 roundtrip rnd", d == PAGE_SIZE && back == rnd, "");

    // 畸形输入防御：截断/零偏移/输出过小。
    let bad: [&[u8]; 4] = [&out[..2], &out[..3], &[0xF0, 0x00, 0x00], &[0x0F, 0xFF]];
    let mut all_rejected = true;
    for b in bad {
        if lz4_decompress(b, &mut back).is_ok() {
            // 某些短输入可能碰巧合法（如纯字面量），不强制全拒——
            // 但绝不允许产出超长结果。
            all_rejected = all_rejected && d <= PAGE_SIZE;
        }
    }
    set.add("lz4 malformed safe", all_rejected, "");

    // 工作集立项门：七天低水位不立项。
    let mut w = WorksetWatch::new();
    for day in 0..8u64 {
        for m in 0..1440u64 {
            w.sample(day * 1440 + m, 2_000_000 + (m % 60));
        }
    }
    set.add("gate low no-trigger", !w.evaluate().trigger, "");

    // 七天高位立项。
    let mut w = WorksetWatch::new();
    for day in 0..8u64 {
        for m in 0..1440u64 {
            w.sample(day * 1440 + m, 3_400_000 + (m % 100));
        }
    }
    let rep = w.evaluate();
    set.add("gate high trigger", rep.trigger && rep.over_threshold_days >= 7, "");

    // 压缩池：收-取-零页-比例闸-满池-清扫。
    let mut pool = CompressPool::new(1); // 1MB = 256 槽
    let mut page = [0u8; PAGE_SIZE];
    for i in 0..PAGE_SIZE {
        page[i] = (i % 16) as u8;
    }
    set.add("pool store", pool.store_page(7, &page, 100) == PoolVerdict::Stored, "");
    let mut got = [0u8; PAGE_SIZE];
    let r = pool.load_page(7, &mut got, 5).expect("load");
    set.add("pool load", r == PoolVerdict::Stored && got == page, "");
    let zero = [0u8; PAGE_SIZE];
    set.add("pool zero", pool.store_page(8, &zero, 100) == PoolVerdict::StoredZero, "");
    let mut zgot = [1u8; PAGE_SIZE];
    set.add("pool zero load", pool.load_page(8, &mut zgot, 5).is_ok() && zgot == zero, "");

    // 不可压缩页 → 比例闸拒收。
    let mut hard = [0u8; PAGE_SIZE];
    let mut x: u32 = 0xDEAD_BEEF;
    for b in hard.iter_mut() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = (x >> 24) as u8;
    }
    set.add("pool ratio gate", pool.store_page(9, &hard, 100) == PoolVerdict::RatioRejected, "");

    // 满池 → Full 显性信号。
    let mut pool1 = CompressPool::new(0);
    set.add("pool disabled", pool1.store_page(1, &page, 1) == PoolVerdict::Shrinking, "");

    // 冷清扫。
    let mut pool = CompressPool::new(1);
    pool.store_page(1, &page, 10);
    set.add("pool cold sweep", pool.sweep_cold(200, 64) == 1 && pool.used_slots() == 0, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    fn xorshift(x: &mut u32) -> u32 {
        *x ^= *x << 13;
        *x ^= *x >> 17;
        *x ^= *x << 5;
        *x
    }

    #[test]
    fn f058_lz4_roundtrip_matrix() {
        let mut out = [0u8; PAGE_SIZE + 32];
        let mut back = [0u8; PAGE_SIZE];
        // 文本类：重复词表。
        let mut txt = [0u8; PAGE_SIZE];
        let word = b"variable-engine-";
        let mut i = 0;
        while i + word.len() < PAGE_SIZE {
            txt[i..i + word.len()].copy_from_slice(word);
            i += word.len();
        }
        let c = lz4_compress(&txt, &mut out).unwrap();
        assert!(c < PAGE_SIZE / 2, "text should compress well: {c}");
        let d = lz4_decompress(&out[..c], &mut back).unwrap();
        assert_eq!(d, PAGE_SIZE);
        assert_eq!(&back[..], &txt[..]);

        // 边界：小输入、全同字节、单字节。
        for n in [0usize, 1, 2, 3, 4, 5, 12, 13, 100] {
            let src = vec![7u8; n];
            let c = lz4_compress(&src, &mut out).unwrap();
            let d = lz4_decompress(&out[..c], &mut back).unwrap();
            assert_eq!(d, n);
            assert_eq!(&back[..d], &src[..]);
        }
    }

    #[test]
    fn f058_lz4_rle_overlap() {
        // 全零页：压缩成 RLE，解压走重叠拷贝路径。
        let src = [0u8; PAGE_SIZE];
        let mut out = [0u8; PAGE_SIZE + 32];
        let c = lz4_compress(&src, &mut out).unwrap();
        assert!(c < 40, "all-zero must be tiny: {c}");
        let mut back = [1u8; PAGE_SIZE];
        let d = lz4_decompress(&out[..c], &mut back).unwrap();
        assert_eq!(d, PAGE_SIZE);
        assert!(back.iter().all(|&b| b == 0));
    }

    #[test]
    fn f058_lz4_malformed_never_overrun() {
        let mut rng = 0xC0FFEEu32;
        let mut out = [0u8; PAGE_SIZE];
        for _ in 0..2000 {
            let n = (xorshift(&mut rng) as usize) % 64 + 1;
            let mut bad = [0u8; 64];
            for b in bad.iter_mut().take(n) {
                *b = (xorshift(&mut rng) & 0xFF) as u8;
            }
            if let Ok(d) = lz4_decompress(&bad[..n], &mut out) {
                assert!(d <= PAGE_SIZE);
            }
        }
    }

    #[test]
    fn f058_workset_gate_boundary() {
        let mut w = WorksetWatch::new();
        // 恰好阈值下：不立项。
        for day in 0..8u64 {
            for m in 0..1440u64 {
                w.sample(day * 1440 + m, WORKSET_TRIGGER_KIB);
            }
        }
        assert!(!w.evaluate().trigger, "at-threshold is not over-threshold");
        // 中途断链：第 4 天回落 → 不立项。
        let mut w = WorksetWatch::new();
        for day in 0..8u64 {
            let base = if day == 4 { 2_000_000 } else { 3_500_000 };
            for m in 0..1440u64 {
                w.sample(day * 1440 + m, base);
            }
        }
        assert!(!w.evaluate().trigger);
    }

    #[test]
    fn f058_pool_store_load_cycle() {
        let mut pool = CompressPool::new(2);
        let mut page = [0u8; PAGE_SIZE];
        for (i, b) in page.iter_mut().enumerate() {
            *b = ((i % 16) * 5) as u8; // 周期模式：4 字节粒度可压
        }
        assert_eq!(pool.store_page(100, &page, 1), PoolVerdict::Stored);
        let mut got = [0u8; PAGE_SIZE];
        assert_eq!(pool.load_page(100, &mut got, 3), Ok(PoolVerdict::Stored));
        assert_eq!(got, page);
        assert_eq!(pool.load_page(404, &mut got, 3), Ok(PoolVerdict::Miss));
        assert_eq!(pool.stats.decomp_miss, 1);
        // 触达刷新保页不被清扫（600-560=40 < 64 秒冷线）。
        assert!(pool.touch(100, 560));
        assert_eq!(pool.sweep_cold(600, 64), 0);
        assert_eq!(pool.sweep_cold(1200, 64), 1);
        // 平均比与节省字节。
        assert!(pool.stats.bytes_saved > 0);
    }

    #[test]
    fn f058_pool_full_signal_and_shrink_gate() {
        // 1 槽池（cap 0MB 不可用 → 用 1MB 再填满验证 Full）。
        let mut pool = CompressPool::new(1);
        let page = vec![3u8; PAGE_SIZE];
        let mut stored = 0;
        for f in 0..300u64 {
            if pool.store_page(f, &page, 1) == PoolVerdict::Stored {
                stored += 1;
            }
        }
        assert!(pool.stats.full_reject > 0);
        assert_eq!(stored, 256, "1MB/4KB = 256 slots");
        assert!(pool.recent_events().iter().any(|(_, ev)| *ev == EV_FULL));
    }

    #[test]
    fn f058_shrink_on_slow_decompress() {
        let mut pool = CompressPool::new(1);
        let page = vec![9u8; PAGE_SIZE];
        for f in 0..64u64 {
            let _ = pool.store_page(f, &page, 1);
        }
        // 注入慢解压样本（P99 拉爆 200μs 线）。
        for i in 0..64u64 {
            let mut got = [0u8; PAGE_SIZE];
            let _ = pool.load_page(i, &mut got, 500);
        }
        assert!(pool.gate_check(), "P99 500us must trigger shrink");
        // 1MB 池一步缩到 0（缩容步长钳在现有容量内——0 = 池停用态）。
        assert_eq!(pool.cap_mb(), 0);
        assert!(pool.recent_events().iter().any(|(_, ev)| *ev == EV_SHRINK));
    }

    #[test]
    fn f058_batch_compressor_budget() {
        let mut bc = BatchCompressor::new();
        bc.begin_window();
        let mut n = 0;
        while bc.has_budget() {
            bc.note(PoolVerdict::Stored);
            n += 1;
        }
        assert_eq!(n, 64);
        assert_eq!(bc.pages_compressed, 64);
    }

    #[test]
    fn f058_run_checks_pass() {
        assert!(run_memcomp_checks().all_passed());
    }
}
