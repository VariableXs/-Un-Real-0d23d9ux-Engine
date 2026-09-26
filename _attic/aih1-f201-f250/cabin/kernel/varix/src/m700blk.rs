//! m700blk — VARIX-M700 AI-10 块设备与存储域 (F226~F250)
//!
//! 块层跳转表/多队列车道/请求合并官/IO 调度谱/块设备档案/
//! 写屏障律/TRIM 谱/块错误分类官/IO 深度自适应/分区裁判/
//! RAID 侦察舱/块加密舱/IO 延迟分位仪/坏块地图/快照块层/
//! IO 配额执行/断电注入台/块层回放流/多路径仲裁/块统计分账/
//! 缓存盘加速舱/块设备自描述/块层压力剧本/块层回归走廊/块域年报。
//!
//! 硬约束：no_std / 无 alloc / 无浮点（全部 permille/定点）/ 纯逻辑。
//! 自检入口：`run_m700blk_checks()`（铁律：断言中凡"操作→读→再操作"，
//! 中间读数必须先落入独立 `let` 变量，禁止末态读取）。

use crate::checks::CheckSet;

// ===========================================================================
// F226 — 块层跳转表：主设备号 → 处理器，注册去重
// ===========================================================================

pub const BLK_MAJORS_MAX: usize = 8;

pub struct BlkDispatch {
    majors: [u16; BLK_MAJORS_MAX],
    handlers: [u16; BLK_MAJORS_MAX],
    count: usize,
    pub rejects: u32,
}

impl BlkDispatch {
    pub const fn new() -> BlkDispatch {
        BlkDispatch { majors: [0; BLK_MAJORS_MAX], handlers: [0; BLK_MAJORS_MAX], count: 0, rejects: 0 }
    }

    /// 主设备号重复拒绝，容量封顶拒绝。
    pub fn register(&mut self, major: u16, handler: u16) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.majors[i] == major {
                self.rejects += 1;
                return false;
            }
            i += 1;
        }
        if self.count >= BLK_MAJORS_MAX {
            self.rejects += 1;
            return false;
        }
        self.majors[self.count] = major;
        self.handlers[self.count] = handler;
        self.count += 1;
        true
    }

    pub fn dispatch(&self, major: u16) -> Option<u16> {
        let mut i = 0usize;
        while i < self.count {
            if self.majors[i] == major {
                return Some(self.handlers[i]);
            }
            i += 1;
        }
        None
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F227 — 多队列车道：CPU→车道散列，本道满则借道
// ===========================================================================

pub const BLK_LANES_MAX: usize = 8;
pub const BLK_LANE_DEPTH: u16 = 4;

#[derive(Clone, Copy, Debug)]
pub struct BlkMqLanes {
    pub depth: [u16; BLK_LANES_MAX],
    pub lanes: usize,
}

impl BlkMqLanes {
    pub const fn new(lanes: usize) -> BlkMqLanes {
        BlkMqLanes { depth: [0; BLK_LANES_MAX], lanes }
    }

    pub fn submit(&mut self, cpu: u32) -> bool {
        let home = blk_lane_of(cpu, self.lanes);
        let mut k = 0usize;
        while k < self.lanes {
            let lane = (home + k) % self.lanes;
            if self.depth[lane] < BLK_LANE_DEPTH {
                self.depth[lane] += 1;
                return true;
            }
            k += 1;
        }
        false
    }

    pub fn complete(&mut self, lane: usize) -> bool {
        if lane < self.lanes && self.depth[lane] > 0 {
            self.depth[lane] -= 1;
            true
        } else {
            false
        }
    }
}

pub fn blk_lane_of(cpu: u32, lanes: usize) -> usize {
    (cpu % lanes as u32) as usize
}

// ===========================================================================
// F228 — 请求合并官：同向 + 后继相邻/小间隙 + 总长封顶
// ===========================================================================

pub const BLK_MERGE_MAX_GAP: u64 = 16;
pub const BLK_MERGE_MAX_SECTORS: u64 = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlkRequest {
    pub start: u64,
    pub len: u64,
    pub write: bool,
}

pub fn blk_mergeable(a: &BlkRequest, b: &BlkRequest) -> bool {
    if a.write != b.write {
        return false;
    }
    let a_end = a.start + a.len;
    if b.start < a_end {
        return false;
    }
    if b.start - a_end > BLK_MERGE_MAX_GAP {
        return false;
    }
    b.start + b.len - a.start <= BLK_MERGE_MAX_SECTORS
}

/// 合并：a 在前 b 在后，返回合并后的请求。
pub fn blk_merge(a: &BlkRequest, b: &BlkRequest) -> Option<BlkRequest> {
    if !blk_mergeable(a, b) {
        return None;
    }
    Some(BlkRequest { start: a.start, len: b.start + b.len - a.start, write: a.write })
}

// ===========================================================================
// F229 — IO 调度谱：3 级（实时 > 尽力 > 闲置），取最高非空级
// ===========================================================================

pub const BLK_SCHED_CLASSES: usize = 3;

#[derive(Clone, Copy, Debug)]
pub struct BlkSched {
    pub pending: [u32; BLK_SCHED_CLASSES],
}

impl BlkSched {
    pub const fn new() -> BlkSched {
        BlkSched { pending: [0; BLK_SCHED_CLASSES] }
    }

    pub fn pick(&self) -> Option<usize> {
        let mut c = 0usize;
        while c < BLK_SCHED_CLASSES {
            if self.pending[c] > 0 {
                return Some(c);
            }
            c += 1;
        }
        None
    }

    pub fn pop(&mut self, class: usize) -> bool {
        if class < BLK_SCHED_CLASSES && self.pending[class] > 0 {
            self.pending[class] -= 1;
            true
        } else {
            false
        }
    }
}

// ===========================================================================
// F230 — 块设备档案：几何合法性、容量、只读闸门
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct BlkDevice {
    pub sectors: u64,
    pub lba_size: u32,
    pub rotational: bool,
    pub writable: bool,
}

pub fn blk_device_ok(d: &BlkDevice) -> bool {
    (d.lba_size == 512 || d.lba_size == 4096) && d.sectors > 0
}

pub fn blk_capacity_bytes(d: &BlkDevice) -> u64 {
    d.sectors * d.lba_size as u64
}

/// 写请求必须过只读闸门。
pub fn blk_write_allowed(d: &BlkDevice, is_write: bool) -> bool {
    !is_write || d.writable
}

// ===========================================================================
// F231 — 写屏障律：屏障在途时禁新写；排空后屏障方可冲刷
// ===========================================================================

#[derive(Clone, Copy, Debug, Default)]
pub struct BlkBarrierSeq {
    pub writes_out: u32,
    pub writes_done: u32,
    pub armed: bool,
}

impl BlkBarrierSeq {
    pub fn submit_write(&mut self) -> bool {
        if self.armed {
            return false;
        }
        self.writes_out += 1;
        true
    }

    pub fn complete_write(&mut self) -> bool {
        if self.writes_done >= self.writes_out {
            return false;
        }
        self.writes_done += 1;
        true
    }

    pub fn arm_barrier(&mut self) -> bool {
        if self.armed {
            return false;
        }
        self.armed = true;
        true
    }

    /// 屏障冲刷：必须已布防且在途写全部落盘。
    pub fn try_flush(&mut self) -> bool {
        if self.armed && self.writes_out == self.writes_done {
            self.armed = false;
            true
        } else {
            false
        }
    }

    pub fn drained(&self) -> bool {
        self.writes_out == self.writes_done
    }
}

// ===========================================================================
// F232 — TRIM 谱：区间标记去重，完全覆盖的 TRIM 不再计功
// ===========================================================================

pub const BLK_TRIM_SECTORS: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct BlkTrimMap {
    pub trimmed: [bool; BLK_TRIM_SECTORS],
    pub trims_issued: u32,
}

impl BlkTrimMap {
    pub const fn new() -> BlkTrimMap {
        BlkTrimMap { trimmed: [false; BLK_TRIM_SECTORS], trims_issued: 0 }
    }

    /// 区间 TRIM：范围非法拒绝；区间内已有新位才发出（去重）。
    pub fn trim_range(&mut self, start: usize, len: usize) -> bool {
        if len == 0 || start + len > BLK_TRIM_SECTORS {
            return false;
        }
        let mut has_new = false;
        let mut i = start;
        while i < start + len {
            if !self.trimmed[i] {
                has_new = true;
            }
            i += 1;
        }
        if !has_new {
            return false;
        }
        i = start;
        while i < start + len {
            self.trimmed[i] = true;
            i += 1;
        }
        self.trims_issued += 1;
        true
    }

    pub fn trimmed_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < BLK_TRIM_SECTORS {
            if self.trimmed[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ===========================================================================
// F233 — 块错误分类官：可重试类别 + 指数退避（10ms 起翻倍封顶 160ms）
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlkErr {
    /// 介质错误，附已重试次数。
    Media(u8),
    /// 传输错误，链路抖动总可重试。
    Transport(u8),
    ReadOnly,
    NoDevice,
}

pub fn blk_err_retryable(e: BlkErr) -> bool {
    match e {
        BlkErr::Media(retries) => retries < 3,
        BlkErr::Transport(_) => true,
        BlkErr::ReadOnly | BlkErr::NoDevice => false,
    }
}

pub fn blk_err_backoff_ms(retries: u8) -> u32 {
    let mut ms = 10u32;
    let mut i = 0u8;
    while i < retries && ms < 160 {
        ms *= 2;
        i += 1;
    }
    if ms > 160 {
        160
    } else {
        ms
    }
}

// ===========================================================================
// F234 — IO 深度自适应：高时延砍半（下限 1），低时延翻倍（上限 32）
// ===========================================================================

pub const BLK_QD_MIN: u32 = 1;
pub const BLK_QD_MAX: u32 = 32;
pub const BLK_QD_LAT_HIGH_US: u32 = 8000;
pub const BLK_QD_LAT_LOW_US: u32 = 1000;

pub fn blk_qd_adjust(cur: u32, last_lat_us: u32) -> u32 {
    if last_lat_us >= BLK_QD_LAT_HIGH_US {
        let half = cur / 2;
        if half < BLK_QD_MIN {
            BLK_QD_MIN
        } else {
            half
        }
    } else if last_lat_us <= BLK_QD_LAT_LOW_US {
        let dbl = cur * 2;
        if dbl > BLK_QD_MAX {
            BLK_QD_MAX
        } else {
            dbl
        }
    } else {
        cur
    }
}

// ===========================================================================
// F235 — 分区裁判：编号去重 + 盘内范围 + 区间不重叠
// ===========================================================================

pub const BLK_PARTS_MAX: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlkPartition {
    pub partno: u8,
    pub start: u64,
    pub len: u64,
}

pub struct BlkPartTable {
    parts: [Option<BlkPartition>; BLK_PARTS_MAX],
    count: usize,
    pub disk_sectors: u64,
    pub rejects: u32,
}

impl BlkPartTable {
    pub const fn new(disk_sectors: u64) -> BlkPartTable {
        BlkPartTable { parts: [None; BLK_PARTS_MAX], count: 0, disk_sectors, rejects: 0 }
    }

    pub fn add_part(&mut self, partno: u8, start: u64, len: u64) -> bool {
        if len == 0 || start + len > self.disk_sectors {
            self.rejects += 1;
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if let Some(p) = self.parts[i] {
                if p.partno == partno {
                    self.rejects += 1;
                    return false;
                }
                if start < p.start + p.len && p.start < start + len {
                    self.rejects += 1;
                    return false;
                }
            }
            i += 1;
        }
        if self.count >= BLK_PARTS_MAX {
            self.rejects += 1;
            return false;
        }
        self.parts[self.count] = Some(BlkPartition { partno, start, len });
        self.count += 1;
        true
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ===========================================================================
// F236 — RAID 侦察舱：RAID5 异或奇偶 + 单盘重建 + 最小盘数
// ===========================================================================

pub fn blk_raid5_parity(stripe: &[u8]) -> u8 {
    let mut p = 0u8;
    let mut i = 0usize;
    while i < stripe.len() {
        p ^= stripe[i];
        i += 1;
    }
    p
}

/// 用存活数据盘与奇偶盘重建缺失盘。
pub fn blk_raid5_recover(survivors: &[u8], parity: u8) -> u8 {
    parity ^ blk_raid5_parity(survivors)
}

pub fn blk_raid5_min_disks(data_disks: u32) -> u32 {
    data_disks + 1
}

// ===========================================================================
// F237 — 块加密舱：扇区号参与密钥流的对称 XOR 加解密
// ===========================================================================

pub const BLK_CRYPTO_SECTOR_MULT: u64 = 0x9E37;

pub fn blk_keystream_byte(sector: u64, index: usize) -> u8 {
    let v = sector
        .wrapping_mul(BLK_CRYPTO_SECTOR_MULT)
        .wrapping_add(index as u64 * 7);
    (v ^ (v >> 8) ^ (v >> 16)) as u8
}

/// 加密与解密同一运算（XOR 对合）。
pub fn blk_xor_crypt(sector: u64, data: &mut [u8]) {
    let mut i = 0usize;
    while i < data.len() {
        data[i] ^= blk_keystream_byte(sector, i);
        i += 1;
    }
}

// ===========================================================================
// F238 — IO 延迟分位仪：指数分桶直方图 + 累计分位 permille
// ===========================================================================

pub const BLK_LAT_BUCKETS_US: [u32; 4] = [250, 1000, 4000, 16000];

#[derive(Clone, Copy, Debug, Default)]
pub struct BlkLatHist {
    /// 桶 0..=3 为各阈值内，桶 4 为超时尾。
    pub counts: [u32; 5],
}

pub fn blk_lat_bucket(lat_us: u32) -> usize {
    let mut b = 0usize;
    while b < 4 {
        if lat_us <= BLK_LAT_BUCKETS_US[b] {
            return b;
        }
        b += 1;
    }
    4
}

/// ≤ 第 upto 桶阈值的请求占比（‰）；空直方图为 0。
pub fn blk_lat_permille_under(h: &BlkLatHist, upto_bucket: usize) -> u32 {
    let mut total = 0u64;
    let mut under = 0u64;
    let mut b = 0usize;
    while b < 5 {
        total += h.counts[b] as u64;
        if b <= upto_bucket {
            under += h.counts[b] as u64;
        }
        b += 1;
    }
    if total == 0 {
        0
    } else {
        (under * 1000 / total) as u32
    }
}

// ===========================================================================
// F239 — 坏块地图：标记去重 + 重映射池 + 健康度 permille
// ===========================================================================

pub const BLK_BADMAP_SECTORS: usize = 64;
pub const BLK_REMAP_POOL: u32 = 8;

#[derive(Clone, Copy, Debug)]
pub struct BlkBadMap {
    pub bad: [bool; BLK_BADMAP_SECTORS],
    pub remapped: u32,
}

impl BlkBadMap {
    pub const fn new() -> BlkBadMap {
        BlkBadMap { bad: [false; BLK_BADMAP_SECTORS], remapped: 0 }
    }

    /// 标坏：重复标记拒绝。
    pub fn mark_bad(&mut self, sector: usize) -> bool {
        if sector >= BLK_BADMAP_SECTORS || self.bad[sector] {
            return false;
        }
        self.bad[sector] = true;
        true
    }

    pub fn remap(&mut self) -> bool {
        if self.remapped >= BLK_REMAP_POOL {
            return false;
        }
        self.remapped += 1;
        true
    }

    pub fn bad_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < BLK_BADMAP_SECTORS {
            if self.bad[i] {
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn healthy_permille(&self) -> u32 {
        (BLK_BADMAP_SECTORS as u32 - self.bad_count()) * 1000 / BLK_BADMAP_SECTORS as u32
    }
}

// ===========================================================================
// F240 — 快照块层：快照后首写触发 COW，重复写不重复复制
// ===========================================================================

pub const BLK_SNAPSHOT_SECTORS: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct BlkSnapshot {
    pub base_gen: u32,
    pub cow: [bool; BLK_SNAPSHOT_SECTORS],
    pub cow_count: u32,
}

impl BlkSnapshot {
    pub const fn new(base_gen: u32) -> BlkSnapshot {
        BlkSnapshot { base_gen, cow: [false; BLK_SNAPSHOT_SECTORS], cow_count: 0 }
    }

    /// 返回是否本次写入触发了 COW 复制。
    pub fn write_sector(&mut self, sector: usize, cur_gen: u32) -> bool {
        if sector >= BLK_SNAPSHOT_SECTORS || cur_gen <= self.base_gen {
            return false;
        }
        if self.cow[sector] {
            return false;
        }
        self.cow[sector] = true;
        self.cow_count += 1;
        true
    }
}

// ===========================================================================
// F241 — IO 配额执行：窗口内 IOPS 限额 + 窗口翻新
// ===========================================================================

#[derive(Clone, Copy, Debug)]
pub struct BlkIoQuota {
    pub limit: u32,
    pub used: u32,
}

impl BlkIoQuota {
    pub const fn new(limit: u32) -> BlkIoQuota {
        BlkIoQuota { limit, used: 0 }
    }

    pub fn try_take(&mut self, n: u32) -> bool {
        if self.used + n > self.limit {
            return false;
        }
        self.used += n;
        true
    }

    pub fn refill(&mut self) {
        self.used = 0;
    }
}

// ===========================================================================
// F242 — 断电注入台：在提交标记序列的第 k 拍断电，清点持久事务
// ===========================================================================

pub const BLK_CUT_MARKS_MAX: usize = 16;

pub fn blk_durable_tx_count(commit_marks: &[bool], cut_at: u32) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < commit_marks.len() && i < cut_at as usize {
        if commit_marks[i] {
            n += 1;
        }
        i += 1;
    }
    n
}

// ===========================================================================
// F243 — 块层回放流：录制的 IO 序列决定论重放，校验和分账
// ===========================================================================

pub const BLK_REPLAY_MAX: usize = 16;

#[derive(Clone, Copy, Debug)]
pub struct BlkReplayLog {
    pub sectors: [u32; BLK_REPLAY_MAX],
    pub writes: [bool; BLK_REPLAY_MAX],
    pub count: usize,
}

impl BlkReplayLog {
    pub const fn new() -> BlkReplayLog {
        BlkReplayLog { sectors: [0; BLK_REPLAY_MAX], writes: [false; BLK_REPLAY_MAX], count: 0 }
    }

    /// 录制一条 IO；容量封顶拒绝。
    pub fn record_op(&mut self, sector: u32, write: bool) -> bool {
        if self.count >= BLK_REPLAY_MAX {
            return false;
        }
        self.sectors[self.count] = sector;
        self.writes[self.count] = write;
        self.count += 1;
        true
    }

    /// 决定论校验和：写扇区 ×4、读扇区 ×1 累加（回绕）。
    pub fn replay_checksum(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.count {
            sum = sum.wrapping_add(self.sectors[i].wrapping_mul(if self.writes[i] { 4 } else { 1 }));
            i += 1;
        }
        sum
    }
}

// ===========================================================================
// F244 — 多路径仲裁：健康优先、优先级最高、平局取先
// ===========================================================================

pub const BLK_PATHS_MAX: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct BlkPath {
    pub id: u32,
    pub prio: u8,
    pub healthy: bool,
}

pub fn blk_path_pick(paths: &[BlkPath]) -> Option<u32> {
    let mut best: Option<&BlkPath> = None;
    let mut i = 0usize;
    while i < paths.len() {
        let p = &paths[i];
        if p.healthy {
            match best {
                Some(b) if b.prio >= p.prio => {}
                _ => best = Some(p),
            }
        }
        i += 1;
    }
    best.map(|b| b.id)
}

// ===========================================================================
// F245 — 块统计分账：按车道分账读/写/冲刷，读占比 permille
// ===========================================================================

pub const BLK_STAT_LANES: usize = 8;

#[derive(Clone, Copy, Debug, Default)]
pub struct BlkLaneStat {
    pub reads: u64,
    pub writes: u64,
    pub flushes: u64,
}

impl BlkLaneStat {
    pub fn total_ops(&self) -> u64 {
        self.reads + self.writes + self.flushes
    }

    /// 读在数据操作（读+写）中的占比（‰）。
    pub fn read_permille(&self) -> u32 {
        let data_ops = self.reads + self.writes;
        if data_ops == 0 {
            0
        } else {
            (self.reads * 1000 / data_ops) as u32
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BlkStatBook {
    pub lanes: [BlkLaneStat; BLK_STAT_LANES],
}

impl BlkStatBook {
    pub const fn new() -> BlkStatBook {
        BlkStatBook { lanes: [BlkLaneStat { reads: 0, writes: 0, flushes: 0 }; BLK_STAT_LANES] }
    }

    pub fn total_ops(&self) -> u64 {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < BLK_STAT_LANES {
            sum += self.lanes[i].total_ops();
            i += 1;
        }
        sum
    }
}

// ===========================================================================
// F246 — 缓存盘加速舱：命中升层，脏比超 700‰ 拒收新条目
// ===========================================================================

pub const BLK_CACHE_PROMOTE_HITS: u32 = 4;
pub const BLK_CACHE_DIRTY_MAX_PERMILLE: u32 = 700;

#[derive(Clone, Copy, Debug, Default)]
pub struct BlkCacheTier {
    pub entries: u32,
    pub dirty: u32,
    pub hits: u32,
}

pub fn blk_cache_promote(tier: &BlkCacheTier) -> bool {
    tier.hits >= BLK_CACHE_PROMOTE_HITS
}

/// 空舱必收；否则脏比必须低于上限。
pub fn blk_cache_admit_allowed(tier: &BlkCacheTier) -> bool {
    if tier.entries == 0 {
        return true;
    }
    tier.dirty * 1000 < BLK_CACHE_DIRTY_MAX_PERMILLE * tier.entries
}

// ===========================================================================
// F247 — 块设备自描述：IDENT 几何 + 特性位封闭 + 序列号非零
// ===========================================================================

pub const BLK_IDENT_FEATURE_TRIM: u32 = 1 << 0;
pub const BLK_IDENT_FEATURE_CRYPTO: u32 = 1 << 1;
pub const BLK_IDENT_FEATURES_KNOWN: u32 = BLK_IDENT_FEATURE_TRIM | BLK_IDENT_FEATURE_CRYPTO;

#[derive(Clone, Copy, Debug)]
pub struct BlkIdent {
    pub sectors: u64,
    pub lba_size: u32,
    pub serial: [u8; 8],
    pub features: u32,
}

pub fn blk_ident_ok(d: &BlkIdent) -> bool {
    if d.lba_size != 512 && d.lba_size != 4096 {
        return false;
    }
    if d.sectors == 0 {
        return false;
    }
    if d.features & !BLK_IDENT_FEATURES_KNOWN != 0 {
        return false;
    }
    let mut serial_zero = true;
    let mut i = 0usize;
    while i < 8 {
        if d.serial[i] != 0 {
            serial_zero = false;
        }
        i += 1;
    }
    !serial_zero
}

/// 扇区计数按 LBA 尺寸对齐（4096 LBA 时须为 8 的倍数）。
pub fn blk_ident_aligned(d: &BlkIdent) -> bool {
    if d.lba_size == 4096 {
        d.sectors % 8 == 0
    } else {
        true
    }
}

// ===========================================================================
// F248 — 块层压力剧本：读/写/TRIM/冲刷配比 permille，合计恒 1000
// ===========================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlkStressMix {
    pub read_pm: u32,
    pub write_pm: u32,
    pub trim_pm: u32,
    pub flush_pm: u32,
}

pub fn blk_mix_ok(m: &BlkStressMix) -> bool {
    m.read_pm + m.write_pm + m.trim_pm + m.flush_pm == 1000
}

/// 按配比折算总操作数中各类的次数。
pub fn blk_mix_ops(m: &BlkStressMix, total_ops: u64) -> (u64, u64, u64, u64) {
    (
        total_ops * m.read_pm as u64 / 1000,
        total_ops * m.write_pm as u64 / 1000,
        total_ops * m.trim_pm as u64 / 1000,
        total_ops * m.flush_pm as u64 / 1000,
    )
}

// ===========================================================================
// F249 — 块层回归走廊：固定 IO 剧本 → 固定校验和
// ===========================================================================

/// 金样剧本：写 100、写 200、读 300 → 校验和 400+800+300 = 1500。
pub fn blk_golden_log() -> BlkReplayLog {
    let mut log = BlkReplayLog::new();
    log.record_op(100, true);
    log.record_op(200, true);
    log.record_op(300, false);
    log
}

pub const BLK_GOLDEN_CHECKSUM: u32 = 1500;

pub fn blk_golden_matches(log: &BlkReplayLog) -> bool {
    log.count == 3 && log.replay_checksum() == BLK_GOLDEN_CHECKSUM
}

// ===========================================================================
// F250 — 块域年报：年报章节完备性
// ===========================================================================

pub const BLK_REPORT_SECTIONS: [&str; 5] = ["dispatch", "queues", "merges", "errors", "stats"];

pub fn blk_report_complete(filled: u32) -> bool {
    filled >= BLK_REPORT_SECTIONS.len() as u32
}

// ===========================================================================
// 域自检
// ===========================================================================

pub fn run_m700blk_checks() -> CheckSet {
    let mut set = CheckSet::new("m700blk");

    // F226 块层跳转表
    let mut disp = BlkDispatch::new();
    let r1 = disp.register(8, 100);
    let r_dup = disp.register(8, 101);
    let mut major = 9u16;
    while major <= 15 {
        disp.register(major, major as u16 * 10);
        major += 1;
    }
    let r_cap = disp.register(16, 160);
    set.add(
        "F226 dispatch register",
        r1 && !r_dup && !r_cap && disp.len() == 8 && disp.rejects == 2,
        "major dedup + cap",
    );
    set.add(
        "F226 dispatch route",
        disp.dispatch(8) == Some(100) && disp.dispatch(99).is_none(),
        "lookup",
    );

    // F227 多队列车道
    let mut mq = BlkMqLanes::new(2);
    let s1 = mq.submit(0);
    let s2 = mq.submit(2);
    let s3 = mq.submit(4);
    let s4 = mq.submit(6);
    let home_full = mq.depth[0];
    let stolen = mq.submit(0);
    set.add(
        "F227 lane fill",
        s1 && s2 && s3 && s4 && home_full == 4 && stolen,
        "home lane depth 4",
    );
    let steal_lane = mq.depth[1];
    let drained = mq.complete(0);
    let home_after = mq.depth[0];
    set.add(
        "F227 lane steal+drain",
        steal_lane == 1 && drained && home_after == 3,
        "borrowed then drained",
    );

    // F228 请求合并官
    let a = BlkRequest { start: 0, len: 8, write: true };
    let b = BlkRequest { start: 8, len: 8, write: true };
    let gap = BlkRequest { start: 25, len: 4, write: true };
    let read = BlkRequest { start: 8, len: 8, write: false };
    let huge = BlkRequest { start: 200, len: 100, write: true };
    let merged = blk_merge(&a, &b);
    set.add(
        "F228 merge adjacent",
        merged == Some(BlkRequest { start: 0, len: 16, write: true }),
        "contiguous joins",
    );
    set.add(
        "F228 merge rejects",
        !blk_mergeable(&a, &gap) && !blk_mergeable(&a, &read) && !blk_mergeable(&a, &huge),
        "gap / direction / cap",
    );

    // F229 IO 调度谱
    let mut sched = BlkSched::new();
    sched.pending[1] = 2;
    sched.pending[2] = 5;
    let pick1 = sched.pick();
    sched.pop(1);
    let left_c1 = sched.pending[1];
    let pick2 = sched.pop(1);
    set.add(
        "F229 sched order",
        pick1 == Some(1) && left_c1 == 1 && pick2,
        "class 1 before 2",
    );
    let mut guard = 0;
    while sched.pick().is_some() && guard < 16 {
        if !sched.pop(2) {
            break;
        }
        guard += 1;
    }
    set.add("F229 sched empty", sched.pick().is_none(), "all drained");

    // F230 块设备档案
    let dev = BlkDevice { sectors: 2048, lba_size: 512, rotational: true, writable: true };
    let ro = BlkDevice { sectors: 2048, lba_size: 512, rotational: false, writable: false };
    let bad = BlkDevice { sectors: 0, lba_size: 100, rotational: false, writable: true };
    set.add(
        "F230 device dossier",
        blk_device_ok(&dev) && !blk_device_ok(&bad) && blk_capacity_bytes(&dev) == 1_048_576,
        "geometry + capacity",
    );
    set.add(
        "F230 read-only gate",
        blk_write_allowed(&dev, true) && blk_write_allowed(&ro, false) && !blk_write_allowed(&ro, true),
        "writes need writable",
    );

    // F231 写屏障律
    let mut bar = BlkBarrierSeq::default();
    let w1 = bar.submit_write();
    let w2 = bar.submit_write();
    let arm1 = bar.arm_barrier();
    let arm2 = bar.arm_barrier();
    let w3 = bar.submit_write();
    let flush_early = bar.try_flush();
    set.add(
        "F231 barrier holds",
        w1 && w2 && arm1 && !arm2 && !w3 && !flush_early,
        "armed blocks writes+flush",
    );
    let c1 = bar.complete_write();
    let drained_mid = bar.drained();
    let c2 = bar.complete_write();
    let flush_ok = bar.try_flush();
    set.add(
        "F231 barrier releases",
        c1 && !drained_mid && c2 && flush_ok && !bar.armed,
        "drain then flush",
    );

    // F232 TRIM 谱
    let mut trim = BlkTrimMap::new();
    let t1 = trim.trim_range(0, 4);
    let count_after_first = trim.trimmed_count();
    let t_dup = trim.trim_range(0, 4);
    let t_part = trim.trim_range(2, 4);
    let count_after_overlap = trim.trimmed_count();
    set.add(
        "F232 trim dedup",
        t1 && count_after_first == 4 && !t_dup && t_part && count_after_overlap == 6,
        "overlap only marks new",
    );
    let t_bad = trim.trim_range(30, 4);
    let t_zero = trim.trim_range(8, 0);
    set.add(
        "F232 trim bounds",
        !t_bad && !t_zero && trim.trims_issued == 2,
        "range checked",
    );

    // F233 块错误分类官
    set.add(
        "F233 error classes",
        blk_err_retryable(BlkErr::Media(2)) && !blk_err_retryable(BlkErr::Media(3))
            && blk_err_retryable(BlkErr::Transport(9))
            && !blk_err_retryable(BlkErr::ReadOnly) && !blk_err_retryable(BlkErr::NoDevice),
        "media exhaustion, transport always",
    );
    set.add(
        "F233 error backoff",
        blk_err_backoff_ms(0) == 10 && blk_err_backoff_ms(2) == 40 && blk_err_backoff_ms(5) == 160,
        "double capped 160",
    );

    // F234 IO 深度自适应
    set.add(
        "F234 qd adapt",
        blk_qd_adjust(16, 9000) == 8 && blk_qd_adjust(4, 500) == 8 && blk_qd_adjust(16, 4000) == 16,
        "halve / double / hold",
    );
    set.add(
        "F234 qd bounds",
        blk_qd_adjust(20, 100) == 32 && blk_qd_adjust(1, 9000) == 1,
        "max 32, floor 1",
    );

    // F235 分区裁判
    let mut parts = BlkPartTable::new(100);
    let p1 = parts.add_part(1, 0, 40);
    let p_overlap = parts.add_part(2, 30, 40);
    let p_out = parts.add_part(2, 80, 40);
    let p_dup = parts.add_part(1, 50, 20);
    let p2 = parts.add_part(2, 50, 20);
    set.add(
        "F235 part arbiter",
        p1 && !p_overlap && !p_out && !p_dup && p2 && parts.len() == 2 && parts.rejects == 3,
        "range + overlap + dedup",
    );

    // F236 RAID 侦察舱
    let parity = blk_raid5_parity(&[0x0F, 0x33, 0x55]);
    let restored = blk_raid5_recover(&[0x0F, 0x55], parity);
    set.add(
        "F236 raid5 parity",
        parity == 0x69 && restored == 0x33,
        "xor parity rebuild",
    );
    set.add(
        "F236 raid5 min disks",
        blk_raid5_min_disks(3) == 4 && blk_raid5_min_disks(1) == 2,
        "data + 1 parity",
    );

    // F237 块加密舱
    let mut plain = [1u8, 2, 3];
    blk_xor_crypt(0, &mut plain);
    let enc = plain;
    blk_xor_crypt(0, &mut plain);
    set.add(
        "F237 crypto roundtrip",
        enc == [1, 5, 13] && plain == [1, 2, 3],
        "xor involutive",
    );
    let k_sector0 = blk_keystream_byte(0, 0);
    let k_sector1 = blk_keystream_byte(1, 0);
    set.add("F237 crypto sector salt", k_sector0 == 0 && k_sector1 == 0xA9, "sector changes stream");

    // F238 IO 延迟分位仪
    set.add(
        "F238 lat buckets",
        blk_lat_bucket(250) == 0 && blk_lat_bucket(251) == 1 && blk_lat_bucket(16_000) == 3
            && blk_lat_bucket(16_001) == 4,
        "threshold exact",
    );
    let mut hist = BlkLatHist::default();
    hist.counts = [1, 1, 1, 1, 0];
    set.add(
        "F238 lat permille",
        blk_lat_permille_under(&hist, 1) == 500 && blk_lat_permille_under(&hist, 4) == 1000
            && blk_lat_permille_under(&BlkLatHist::default(), 3) == 0,
        "cumulative shares",
    );

    // F239 坏块地图
    let mut map = BlkBadMap::new();
    let m1 = map.mark_bad(0);
    let m_dup = map.mark_bad(0);
    let mut sector = 1usize;
    while sector < 16 {
        map.mark_bad(sector);
        sector += 1;
    }
    let healthy = map.healthy_permille();
    set.add(
        "F239 badmap marks",
        m1 && !m_dup && map.bad_count() == 16 && healthy == 750,
        "16/64 bad = 750",
    );
    let mut remapped_ok = 0u32;
    let mut k = 0u32;
    while k < 9 {
        if map.remap() {
            remapped_ok += 1;
        }
        k += 1;
    }
    set.add("F239 remap pool", remapped_ok == 8, "pool capped at 8");

    // F240 快照块层
    let mut snap = BlkSnapshot::new(1);
    let pre = snap.write_sector(0, 0);
    let cow1 = snap.write_sector(0, 2);
    let count_after_first = snap.cow_count;
    let cow2 = snap.write_sector(0, 2);
    let cow3 = snap.write_sector(1, 2);
    set.add(
        "F240 snapshot cow",
        !pre && cow1 && count_after_first == 1 && !cow2 && cow3 && snap.cow_count == 2,
        "first write copies, once",
    );

    // F241 IO 配额执行
    let mut quota = BlkIoQuota::new(10);
    let q1 = quota.try_take(6);
    let used_mid = quota.used;
    let q2 = quota.try_take(6);
    set.add("F241 quota take", q1 && used_mid == 6 && !q2, "over limit refused");
    let q3 = quota.try_take(4);
    let exhausted = quota.try_take(1);
    quota.refill();
    let after_refill = quota.try_take(1);
    set.add(
        "F241 quota refill",
        q3 && !exhausted && after_refill && quota.used == 1,
        "window renewal",
    );

    // F242 断电注入台
    let marks = [true, true, false, true];
    let d4 = blk_durable_tx_count(&marks, 4);
    let d3 = blk_durable_tx_count(&marks, 3);
    let d0 = blk_durable_tx_count(&marks, 0);
    set.add(
        "F242 power cut durable",
        d4 == 3 && d3 == 2 && d0 == 0,
        "only committed marks count",
    );

    // F243 块层回放流
    let mut log_a = BlkReplayLog::new();
    log_a.record_op(10, true);
    log_a.record_op(20, false);
    let mut log_b = BlkReplayLog::new();
    log_b.record_op(10, true);
    log_b.record_op(20, false);
    let mut log_c = BlkReplayLog::new();
    log_c.record_op(10, true);
    log_c.record_op(21, false);
    let ck_a = log_a.replay_checksum();
    let ck_b = log_b.replay_checksum();
    let ck_c = log_c.replay_checksum();
    set.add(
        "F243 replay deterministic",
        ck_a == 60 && ck_b == 60 && ck_c == 61,
        "writes x4, reads x1",
    );
    let mut full = BlkReplayLog::new();
    let mut op = 0u32;
    let mut recorded = 0u32;
    while op < 18 {
        if full.record_op(op, true) {
            recorded += 1;
        }
        op += 1;
    }
    set.add("F243 replay capacity", recorded == 16 && full.count == 16, "16 ops max");

    // F244 多路径仲裁
    let paths = [
        BlkPath { id: 1, prio: 2, healthy: true },
        BlkPath { id: 2, prio: 5, healthy: false },
        BlkPath { id: 3, prio: 3, healthy: true },
    ];
    let picked = blk_path_pick(&paths);
    let dead = [
        BlkPath { id: 1, prio: 9, healthy: false },
        BlkPath { id: 2, prio: 9, healthy: false },
    ];
    set.add(
        "F244 path pick",
        picked == Some(3) && blk_path_pick(&dead).is_none(),
        "healthy highest prio",
    );
    let tie = [
        BlkPath { id: 7, prio: 4, healthy: true },
        BlkPath { id: 8, prio: 4, healthy: true },
    ];
    set.add("F244 path tie", blk_path_pick(&tie) == Some(7), "first wins tie");

    // F245 块统计分账
    let mut book = BlkStatBook::new();
    book.lanes[0].reads = 3;
    book.lanes[0].writes = 1;
    book.lanes[1].flushes = 2;
    let total = book.total_ops();
    let read_pm = book.lanes[0].read_permille();
    set.add(
        "F245 stats account",
        total == 6 && read_pm == 750 && book.lanes[1].read_permille() == 0,
        "per-lane ledger",
    );

    // F246 缓存盘加速舱
    let hot = BlkCacheTier { entries: 10, dirty: 6, hits: 4 };
    let cold = BlkCacheTier { entries: 10, dirty: 6, hits: 3 };
    let soggy = BlkCacheTier { entries: 10, dirty: 8, hits: 9 };
    let empty_tier = BlkCacheTier { entries: 0, dirty: 0, hits: 0 };
    set.add(
        "F246 cache promote",
        blk_cache_promote(&hot) && !blk_cache_promote(&cold),
        "4 hits to promote",
    );
    set.add(
        "F246 cache admit",
        blk_cache_admit_allowed(&hot) && !blk_cache_admit_allowed(&soggy) && blk_cache_admit_allowed(&empty_tier),
        "dirty ratio gate",
    );

    // F247 块设备自描述
    let ident = BlkIdent {
        sectors: 16,
        lba_size: 4096,
        serial: [0x56, 0x41, 0x52, 0x49, 0x58, 0x30, 0x31, 0x00],
        features: BLK_IDENT_FEATURE_TRIM,
    };
    let mut no_serial = ident;
    no_serial.serial = [0; 8];
    let mut bad_feat = ident;
    bad_feat.features |= 1 << 7;
    set.add(
        "F247 ident ok",
        blk_ident_ok(&ident) && !blk_ident_ok(&no_serial) && !blk_ident_ok(&bad_feat),
        "serial + known features",
    );
    let mut misaligned = ident;
    misaligned.sectors = 15;
    set.add(
        "F247 ident alignment",
        blk_ident_aligned(&ident) && !blk_ident_aligned(&misaligned),
        "4k lba needs x8 sectors",
    );

    // F248 块层压力剧本
    let mix = BlkStressMix { read_pm: 600, write_pm: 300, trim_pm: 50, flush_pm: 50 };
    let bad_mix = BlkStressMix { read_pm: 600, write_pm: 300, trim_pm: 50, flush_pm: 49 };
    let scaled = blk_mix_ops(&mix, 1000);
    set.add(
        "F248 stress mix",
        blk_mix_ok(&mix) && !blk_mix_ok(&bad_mix) && scaled == (600, 300, 50, 50),
        "permille sums to 1000",
    );

    // F249 块层回归走廊
    let golden = blk_golden_log();
    set.add("F249 golden matches", blk_golden_matches(&golden), "checksum 1500");
    let mut drifted = blk_golden_log();
    drifted.record_op(400, true);
    set.add("F249 golden catches drift", !blk_golden_matches(&drifted), "extra op detected");

    // F250 块域年报
    set.add(
        "F250 blk report",
        BLK_REPORT_SECTIONS.len() == 5 && blk_report_complete(5) && !blk_report_complete(4),
        "sections complete",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f227_lane_mapping() {
        assert_eq!(blk_lane_of(0, 4), 0);
        assert_eq!(blk_lane_of(5, 4), 1);
        assert_eq!(blk_lane_of(7, 8), 7);
        let mut mq = BlkMqLanes::new(2);
        let mut k = 0;
        while k < 8 {
            assert!(mq.submit(k));
            k += 1;
        }
        assert!(!mq.submit(0)); // 两车道全部打满（深度各 4）
    }

    #[test]
    fn f231_barrier_never_skips() {
        let mut bar = BlkBarrierSeq::default();
        assert!(bar.submit_write());
        assert!(bar.arm_barrier());
        // 屏障在途：不允许新写，也不允许提前冲刷
        assert!(!bar.submit_write());
        assert!(!bar.try_flush());
        assert!(bar.complete_write());
        assert!(bar.try_flush());
        assert_eq!(bar.writes_out, bar.writes_done);
    }

    #[test]
    fn f234_qd_floor_and_cap() {
        assert_eq!(blk_qd_adjust(2, 10_000), 1);
        assert_eq!(blk_qd_adjust(31, 0), 32);
        assert_eq!(blk_qd_adjust(9, 5000), 9);
    }

    #[test]
    fn f236_raid5_xor_identity() {
        // 任意盘缺一块都能由其余盘异或重建
        let disks = [0xA5u8, 0x5A, 0xFF, 0x01];
        let p = blk_raid5_parity(&disks);
        assert_eq!(blk_raid5_recover(&[disks[1], disks[2], disks[3]], p), disks[0]);
        assert_eq!(blk_raid5_recover(&[disks[0], disks[2], disks[3]], p), disks[1]);
    }

    #[test]
    fn f239_healthy_math() {
        let mut m = BlkBadMap::new();
        assert_eq!(m.healthy_permille(), 1000);
        let mut i = 0usize;
        while i < 32 {
            m.mark_bad(i * 2);
            i += 1;
        }
        assert_eq!(m.bad_count(), 32);
        assert_eq!(m.healthy_permille(), 500);
    }

    #[test]
    fn f248_mix_scaling() {
        let m = BlkStressMix { read_pm: 250, write_pm: 250, trim_pm: 250, flush_pm: 250 };
        assert!(blk_mix_ok(&m));
        assert_eq!(blk_mix_ops(&m, 400), (100, 100, 100, 100));
        let zero = BlkStressMix { read_pm: 1000, write_pm: 0, trim_pm: 0, flush_pm: 0 };
        assert_eq!(blk_mix_ops(&zero, 7), (7, 0, 0, 0));
    }

    #[test]
    fn blk_selfcheck_all_pass() {
        let set = run_m700blk_checks();
        assert!(set.len() >= 25, "got {}", set.len());
        assert!(set.all_passed());
    }
}
