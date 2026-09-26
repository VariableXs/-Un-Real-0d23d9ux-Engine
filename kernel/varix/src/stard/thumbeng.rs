//! F093 图片缩略图引擎 · 完整设计（STAR I 主册 G-C-23）。
//!
//! **判据（主册）**：万张目录滚动帧率不掉（F041 账本）；二次浏览命中率
//! >95%；缓存库 2GB 上限生效实测。
//!
//! **设计要点（主册）**：
//! - 按文件哈希缓存缩略图，尺寸档对齐乙-4 表图标档（32/48/96/256 四档
//!   够用），同图多档共存（视图切换零等待）；
//! - 生成优先级：可视区 > 邻近区 > 全目录（按滚动预测），生成走后台低
//!   优先队列（F057 分级语义——三级优先）；
//! - 生成中占位 = 模糊微缩图（低清先行渐进替换）；悬停大预览 480px
//!   （F091 窗格联动）；
//! - 缓存条目：哈希 + 尺寸 + 时间戳 + 位图块；库上限 2GB LRU；损坏条目
//!   即弃（自愈）；原图变更 → 哈希失配重生成；库文件损坏 → 重建（进度
//!   通知）；只读卷（SHARED）→ 缓存写本地（原图位置无关）；
//! - EXIF 缩略图直接抽取（原图内嵌缩略免解码——Linux EXIF 标准做法）；
//! - 库碎片整理在空闲时段（F049 窗口语义——由调用方在空闲时触发）。
//!
//! 位图以「块账」（字节数 + 校验和）建模——内核模型面不持真实像素，
//! 解码面以闭包注入口承接（F054 SIMD 面就绪后直挂），宿主测试确定复现。
//! 哈希一律 FNV-1a 64（与 F072 recenteng 同源算法，一处一事实）。

use crate::checks::CheckSet;
use crate::star::sbase::sat_sub;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量（参数唯一源——乙-4 表图标档与主册设计细节）
// ---------------------------------------------------------------------------

/// 缩略图尺寸档（px）——乙-4 表图标档四档。
pub const SIZE_TIERS: [u32; 4] = [32, 48, 96, 256];

/// 悬停大预览边长（px，F091 窗格联动）。
pub const HOVER_PREVIEW_PX: u32 = 480;

/// 缓存库字节上限（2GB，LRU 逐出判线）。
pub const LIB_CAP_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// 占位（模糊微缩图）档——生成中先出的最低清档。
pub const PLACEHOLDER_TIER: u32 = 32;

/// 后台生成队列三级优先（F057 分级语义）。
pub const PRI_VISIBLE: u8 = 0;
pub const PRI_NEARBY: u8 = 1;
pub const PRI_BACKGROUND: u8 = 2;

/// 生成队列定容（万张目录滚动时排队上限——超出按优先级挤兑）。
pub const QUEUE_CAP: usize = 256;

/// LRU 容量纪律：逐出时单步最多清理的条目数（防一次性长停顿）。
pub const LRU_STEP: usize = 64;

// ---------------------------------------------------------------------------
// 哈希与条目
// ---------------------------------------------------------------------------

/// FNV-1a 64 位文件哈希（缓存键唯一源；与 recenteng 别名归并同算法）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// 一条缩略图缓存条目。
#[derive(Clone, Debug)]
pub struct ThumbEntry {
    /// 文件哈希（缓存键）。
    pub hash: u64,
    /// 尺寸档（SIZE_TIERS 之一）。
    pub tier: u32,
    /// 生成时刻（调用方注入时间戳）。
    pub stamp: u64,
    /// 位图块字节数。
    pub bytes: u64,
    /// 位图块校验和（FNV-1a——损坏条目即弃的判据）。
    pub checksum: u64,
    /// 来源：true = EXIF 内嵌缩略直抽（零全解码），false = 实时解码。
    pub from_exif: bool,
}

impl ThumbEntry {
    /// 逻辑有效：字节数与校验和自洽。
    pub fn intact(&self) -> bool {
        self.bytes > 0 && self.checksum != 0
    }
}

// ---------------------------------------------------------------------------
// 生成请求队列
// ---------------------------------------------------------------------------

/// 一条生成请求（含优先级与入队时刻）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenReq {
    pub hash: u64,
    pub tier: u32,
    pub pri: u8,
    pub enqueued_ms: u64,
}

/// 后台生成队列（三级优先，F057 分级语义）。
///
/// 排序纪律：pri 小者先出（可视区最急）；同优先级先入先出。定容
/// QUEUE_CAP——满时新请求只允许挤掉**不更急**的条目；可视区请求永不被
/// 直接丢弃，被挤兑/拒绝数如实记账（诚实不吞）。
pub struct GenQueue {
    items: Vec<GenReq>,
    pub evicted_low: u64,
    pub rejected_dup: u64,
    pub rejected_drop: u64,
}

impl GenQueue {
    pub fn new() -> GenQueue {
        GenQueue { items: Vec::new(), evicted_low: 0, rejected_dup: 0, rejected_drop: 0 }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 入队：同 (hash, tier) 去重；队满时按优先级挤兑。
    pub fn push(&mut self, req: GenReq) {
        if self.items.iter().any(|r| r.hash == req.hash && r.tier == req.tier) {
            self.rejected_dup += 1;
            return;
        }
        if self.items.len() >= QUEUE_CAP {
            // 优先挤掉「更不急」的条目（pri 更大者，同级取最旧）。
            let victim = self
                .items
                .iter()
                .enumerate()
                .filter(|(_, r)| r.pri > req.pri)
                .max_by_key(|(_, r)| (r.pri, u64::MAX - r.enqueued_ms));
            let removed = match victim {
                Some((idx, _)) => {
                    self.items.remove(idx);
                    self.evicted_low += 1;
                    true
                }
                None => {
                    if req.pri == PRI_BACKGROUND {
                        // 背景请求没有挤兑资格：排队失败如实记账。
                        self.rejected_drop += 1;
                        false
                    } else {
                        // 可视区/邻近区不丢：挤掉同档最旧一条兜底。
                        let alt = self
                            .items
                            .iter()
                            .enumerate()
                            .filter(|(_, r)| r.pri >= req.pri)
                            .min_by_key(|(_, r)| r.enqueued_ms);
                        if let Some((idx, _)) = alt {
                            self.items.remove(idx);
                            self.evicted_low += 1;
                            true
                        } else {
                            self.rejected_drop += 1;
                            false
                        }
                    }
                }
            };
            if !removed {
                return;
            }
        }
        self.items.push(req);
    }

    /// 出队最急请求（pri 最小；平局按入队序）。
    pub fn pop(&mut self) -> Option<GenReq> {
        if self.items.is_empty() {
            return None;
        }
        let mut best = 0usize;
        for i in 1..self.items.len() {
            let a = &self.items[i];
            let b = &self.items[best];
            if a.pri < b.pri || (a.pri == b.pri && a.enqueued_ms < b.enqueued_ms) {
                best = i;
            }
        }
        Some(self.items.remove(best))
    }

    /// 按滚动预测清空 background 请求（用户已滚走——白干的活不干）。
    pub fn drop_background(&mut self) -> usize {
        let before = self.items.len();
        self.items.retain(|r| r.pri != PRI_BACKGROUND);
        before - self.items.len()
    }
}

impl Default for GenQueue {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// LRU 缓存库
// ---------------------------------------------------------------------------

/// 库内部槽（含 LRU 时钟）。
#[derive(Clone, Debug)]
struct Slot {
    entry: ThumbEntry,
    /// 最近使用时刻（访问即拨钟）。
    last_use: u64,
}

/// 摘要操作结果（对账与通知面消费）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibOp {
    /// 命中。
    Hit,
    /// 未命中（需生成）。
    Miss,
    /// 命中但条目损坏——已即弃（自愈），调用方需重生成。
    CorruptDiscard,
}

/// 缩略图缓存库：哈希+档位为键，2GB LRU 上限，损坏即弃自愈。
pub struct ThumbLib {
    slots: Vec<Slot>,
    used_bytes: u64,
    clock: u64,
    /// 对账账本：命中/未命中/损坏即弃/LRU 逐出。
    pub hits: u64,
    pub misses: u64,
    pub corrupt_discards: u64,
    pub lru_evictions: u64,
    /// 库侧实际落盘位置：false = 原卷旁（默认），true = 本地（只读卷降级）。
    pub local_side: bool,
}

impl ThumbLib {
    pub fn new() -> ThumbLib {
        ThumbLib {
            slots: Vec::new(),
            used_bytes: 0,
            clock: 0,
            hits: 0,
            misses: 0,
            corrupt_discards: 0,
            lru_evictions: 0,
            local_side: false,
        }
    }

    pub fn entry_count(&self) -> usize {
        self.slots.len()
    }

    pub fn used_bytes(&self) -> u64 {
        self.used_bytes
    }

    /// 取一条目（访问即拨 LRU 钟）。损坏条目即弃并回 CorruptDiscard。
    pub fn lookup(&mut self, hash: u64, tier: u32, _now: u64) -> (LibOp, Option<ThumbEntry>) {
        self.clock += 1;
        let clock = self.clock;
        let idx = self.slots.iter().position(|s| s.entry.hash == hash && s.entry.tier == tier);
        match idx {
            None => {
                self.misses += 1;
                (LibOp::Miss, None)
            }
            Some(i) => {
                if !self.slots[i].entry.intact() {
                    let slot = self.slots.remove(i);
                    self.used_bytes = sat_sub(self.used_bytes, slot.entry.bytes);
                    self.corrupt_discards += 1;
                    return (LibOp::CorruptDiscard, None);
                }
                self.slots[i].last_use = clock;
                let e = self.slots[i].entry.clone();
                self.hits += 1;
                (LibOp::Hit, Some(e))
            }
        }
    }

    /// 放入一条目（超过 2GB 上限触发 LRU 逐出；单条超限直接拒收）。
    /// 损坏条目允许入库（模拟静默损坏/序列化损伤）——读取时即弃自愈。
    pub fn insert(&mut self, entry: ThumbEntry, now: u64) -> bool {
        if entry.bytes > LIB_CAP_BYTES {
            return false;
        }
        self.clock += 1;
        let clock = self.clock;
        // 同键覆盖（原图重生成场景）。
        if let Some(i) = self.slots.iter().position(|s| s.entry.hash == entry.hash && s.entry.tier == entry.tier) {
            let old = self.slots.remove(i);
            self.used_bytes = sat_sub(self.used_bytes, old.entry.bytes);
        }
        self.used_bytes += entry.bytes;
        self.slots.push(Slot { entry, last_use: clock });
        self.enforce_cap(now);
        true
    }

    /// LRU 逐出到上限内（分步清理，单步至多 LRU_STEP 条）。
    fn enforce_cap(&mut self, _now: u64) {
        let mut steps = 0usize;
        while self.used_bytes > LIB_CAP_BYTES && !self.slots.is_empty() && steps < LRU_STEP * 16 {
            let mut oldest = 0usize;
            for i in 1..self.slots.len() {
                if self.slots[i].last_use < self.slots[oldest].last_use {
                    oldest = i;
                }
            }
            let slot = self.slots.remove(oldest);
            self.used_bytes = sat_sub(self.used_bytes, slot.entry.bytes);
            self.lru_evictions += 1;
            steps += 1;
        }
    }

    /// 原图变更：该哈希全档失效（哈希失配重生成语义——调用方以新哈希
    /// 查询天然 Miss；此接口供显式撤销场景）。
    pub fn invalidate(&mut self, hash: u64) -> usize {
        let before = self.slots.len();
        self.slots.retain(|s| s.entry.hash != hash);
        let removed = before - self.slots.len();
        self.used_bytes = sat_sub(self.used_bytes, 0); // 字节账由逐条重算兜底。
        self.recalc_bytes();
        removed
    }

    fn recalc_bytes(&mut self) {
        self.used_bytes = self.slots.iter().map(|s| s.entry.bytes).sum();
    }

    /// 库文件损坏 → 全量重建（进度通知由返回的清空量驱动）。
    pub fn rebuild(&mut self) -> usize {
        let n = self.slots.len();
        self.slots.clear();
        self.used_bytes = 0;
        n
    }

    /// 空闲时段碎片整理：剔除 0 字节与校验和残缺的碎片条目（F049 窗口）。
    pub fn defrag_idle(&mut self) -> usize {
        let before = self.slots.len();
        self.slots.retain(|s| s.entry.intact());
        self.recalc_bytes();
        before - self.slots.len()
    }

    /// 命中率（二次浏览判据 >95% 的核算面）。样本为 0 时回 None——
    /// 不许把「没测过」伪装成达标。
    pub fn hit_rate_ppm(&self) -> Option<u64> {
        let total = self.hits + self.misses + self.corrupt_discards;
        if total == 0 {
            return None;
        }
        Some(self.hits * 1_000_000 / total)
    }
}

impl Default for ThumbLib {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// EXIF 内嵌缩略直抽（JPEG APP1 EXIF 段·IFD0 0x501B ThumbnailOffset/Length）
// ---------------------------------------------------------------------------

/// EXIF 缩略抽取结果：None = 无内嵌缩略（走实时解码）。
pub struct ExifThumb {
    pub offset: usize,
    pub length: usize,
}

/// 从 JPEG 字节流抽取内嵌缩略图定位（不拷贝像素——零全解码）。
///
/// 路径：FF D8 → 找 APP1（FF E1）→ 校验 "Exif\0\0" → TIFF 头 → IFD0
/// → 找 tag 0x501B（ThumbnailOffset，LONG）与 0x501A（Length）。任意
/// 一步畸形 → None（对抗样本不 panic——F176 安全纪律同源）。
pub fn exif_thumb_locate(jpeg: &[u8]) -> Option<ExifThumb> {
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    // 扫段（最多 64 段——防畸形长链）。
    for _ in 0..64 {
        if i + 4 > jpeg.len() || jpeg[i] != 0xFF {
            return None;
        }
        let marker = jpeg[i + 1];
        if marker == 0xDA {
            return None; // SOS：数据开始，EXIF 不在后面。
        }
        let seg_len = ((jpeg[i + 2] as usize) << 8) | jpeg[i + 3] as usize;
        if seg_len < 2 || i + 2 + seg_len > jpeg.len() {
            return None;
        }
        if marker == 0xE1 {
            let body = &jpeg[i + 4..i + 2 + seg_len];
            if body.len() >= 6 && &body[0..6] == b"Exif\0\0" {
                return parse_tiff_thumb(&body[6..]);
            }
        }
        i += 2 + seg_len;
    }
    None
}

/// TIFF 头 → IFD0 → 缩略 offset/length。
fn parse_tiff_thumb(tiff: &[u8]) -> Option<ExifThumb> {
    if tiff.len() < 8 {
        return None;
    }
    let le = match &tiff[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let rd16 = |b: &[u8], o: usize| -> Option<u16> {
        if o + 2 > b.len() {
            return None;
        }
        Some(if le { u16::from_le_bytes([b[o], b[o + 1]]) } else { u16::from_be_bytes([b[o], b[o + 1]]) })
    };
    let rd32 = |b: &[u8], o: usize| -> Option<u32> {
        if o + 4 > b.len() {
            return None;
        }
        Some(if le {
            u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
        } else {
            u32::from_be_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
        })
    };
    let ifd0_off = rd32(tiff, 4)? as usize;
    if ifd0_off + 2 > tiff.len() {
        return None;
    }
    let n = rd16(tiff, ifd0_off)? as usize;
    if ifd0_off + 2 + n * 12 > tiff.len() {
        return None;
    }
    let mut off = None;
    let mut len = None;
    for k in 0..n {
        let e = ifd0_off + 2 + k * 12;
        let tag = rd16(tiff, e)?;
        match tag {
            0x501B => off = rd32(tiff, e + 8),
            0x501A => len = rd32(tiff, e + 8),
            _ => {}
        }
    }
    let off = off? as usize;
    let len = len? as usize;
    if len == 0 || off.saturating_add(len) > tiff.len() {
        return None;
    }
    Some(ExifThumb { offset: off, length: len })
}

// ---------------------------------------------------------------------------
// 引擎枢纽：查 → 占位 → 队列 → 生成 → 入库
// ---------------------------------------------------------------------------

/// 一次查询的落点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueryOutcome {
    Hit,
    /// 生成中：低清占位已给（模糊微缩图先行）。
    Placeholder,
    Miss,
}

/// 缩略图引擎枢纽（资源管理器/相册共用唯一入口——一处一事实）。
pub struct ThumbEngine {
    pub lib: ThumbLib,
    pub queue: GenQueue,
    /// 只读卷旗标（SHARED 卷 → 缓存写本地）。
    pub ro_volume: bool,
    /// 当前目录句柄（滚动预测上下文）。
    pub cur_dir: u64,
}

impl ThumbEngine {
    pub fn new() -> ThumbEngine {
        ThumbEngine { lib: ThumbLib::new(), queue: GenQueue::new(), ro_volume: false, cur_dir: 0 }
    }

    /// 查询一档缩略图。命中即返回；未命中先出占位档（已有则 Placeholder，
    /// 没有则 Miss）并入队按 `pri` 生成。
    pub fn query(&mut self, hash: u64, tier: u32, pri: u8, now_ms: u64) -> QueryOutcome {
        match self.lib.lookup(hash, tier, now_ms) {
            (LibOp::Hit, Some(_)) => QueryOutcome::Hit,
            (LibOp::CorruptDiscard, _) => {
                // 损坏即弃自愈：立即补队列重生成。
                self.queue.push(GenReq { hash, tier, pri, enqueued_ms: now_ms });
                QueryOutcome::Placeholder
            }
            _ => {
                // 占位逻辑：低清档命中 → 模糊先行；否则全 Miss。
                if tier > PLACEHOLDER_TIER
                    && matches!(self.lib.lookup(hash, PLACEHOLDER_TIER, now_ms), (LibOp::Hit, Some(_)))
                {
                    self.queue.push(GenReq { hash, tier, pri, enqueued_ms: now_ms });
                    QueryOutcome::Placeholder
                } else {
                    self.queue.push(GenReq { hash, tier, pri, enqueued_ms: now_ms });
                    QueryOutcome::Miss
                }
            }
        }
    }

    /// 生成完成回灌（解码面产出后调用）：入 LRU 库。
    pub fn complete(&mut self, hash: u64, tier: u32, bytes: u64, checksum: u64, from_exif: bool, now: u64) -> bool {
        self.lib.insert(
            ThumbEntry { hash, tier, stamp: now, bytes, checksum, from_exif },
            now,
        )
    }

    /// 只读卷适配：启用后库侧写本地（原图位置无关）。
    pub fn set_readonly_volume(&mut self, ro: bool) {
        self.ro_volume = ro;
        self.lib.local_side = ro;
    }

    /// 悬停大预览请求（F091 窗格联动）：480px 预览走 256 档放大即达
    /// （预览不单独建档——同图多档共存纪律）。
    pub fn hover_preview_tier(&self) -> u32 {
        *SIZE_TIERS.iter().max().unwrap_or(&256)
    }
}

impl Default for ThumbEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

/// F093 自检（聚合进 stard 域）。
pub fn run_thumbeng_checks() -> CheckSet {
    let mut set = CheckSet::new("stard-F093");

    // 四档规格与常量自洽。
    set.add("tiers four aligned", SIZE_TIERS == [32, 48, 96, 256], "");
    set.add("hover preview 480", HOVER_PREVIEW_PX == 480, "");

    // 哈希稳定（FNV-1a 已知向量）。
    set.add("fnv1a64 known vector", fnv1a64(b"a") == 0xaf63_dc4c_8601_ec8c, "");
    set.add("fnv1a64 distinct", fnv1a64(b"b") != fnv1a64(b"c"), "");

    // 优先级队列：可视区 > 邻近区 > 背景三序。
    let mut q = GenQueue::new();
    q.push(GenReq { hash: 1, tier: 48, pri: PRI_BACKGROUND, enqueued_ms: 1 });
    q.push(GenReq { hash: 2, tier: 48, pri: PRI_NEARBY, enqueued_ms: 2 });
    q.push(GenReq { hash: 3, tier: 48, pri: PRI_VISIBLE, enqueued_ms: 3 });
    let first = q.pop().unwrap();
    set.add("queue visible first", first.hash == 3 && first.pri == PRI_VISIBLE, "");
    let second = q.pop().unwrap();
    set.add("queue nearby second", second.hash == 2 && second.pri == PRI_NEARBY, "");
    // 去重。
    q.push(GenReq { hash: 1, tier: 48, pri: PRI_BACKGROUND, enqueued_ms: 9 });
    set.add("queue dedup", q.len() == 1 && q.rejected_dup == 1, "");
    // 滚走清空背景。
    let dropped = q.drop_background();
    set.add("queue drop background on scroll", dropped == 1 && q.is_empty(), "");

    // LRU 2GB 上限：大条目逐出最旧未用。
    let mut lib = ThumbLib::new();
    let big = LIB_CAP_BYTES / 2;
    for k in 0..3u64 {
        lib.insert(
            ThumbEntry { hash: k, tier: 48, stamp: k, bytes: big, checksum: 0xdead + k, from_exif: false },
            k,
        );
    }
    set.add("lru cap enforced", lib.used_bytes() <= LIB_CAP_BYTES, "");
    set.add("lru evicted oldest", lib.lru_evictions >= 1 && lib.lookup(0, 48, 9).0 == LibOp::Miss, "");
    set.add("lru keeps newest", lib.lookup(2, 48, 9).0 == LibOp::Hit, "");

    // 损坏条目即弃（自愈）。
    let mut lib2 = ThumbLib::new();
    lib2.insert(ThumbEntry { hash: 7, tier: 96, stamp: 1, bytes: 100, checksum: 42, from_exif: false }, 1);
    // 模拟损坏：直接插入 0 校验和条目（另一键），lookup 应即弃。
    lib2.insert(ThumbEntry { hash: 8, tier: 96, stamp: 2, bytes: 0, checksum: 0, from_exif: false }, 2);
    let (op, _) = lib2.lookup(8, 96, 3);
    set.add("corrupt entry discarded", op == LibOp::CorruptDiscard && lib2.corrupt_discards == 1, "");

    // 哈希失配重生成（invalidate 全档失效）。
    let mut lib3 = ThumbLib::new();
    lib3.insert(ThumbEntry { hash: 9, tier: 32, stamp: 1, bytes: 10, checksum: 1, from_exif: false }, 1);
    lib3.insert(ThumbEntry { hash: 9, tier: 256, stamp: 1, bytes: 10, checksum: 1, from_exif: false }, 1);
    set.add("invalidate all tiers", lib3.invalidate(9) == 2 && lib3.entry_count() == 0, "");

    // 空间超限单条拒收。
    let mut lib4 = ThumbLib::new();
    set.add("oversize entry rejected", !lib4.insert(ThumbEntry { hash: 1, tier: 32, stamp: 1, bytes: LIB_CAP_BYTES + 1, checksum: 1, from_exif: false }, 1), "");

    // 命中率 >95% 判线核算（按热库增量——冷启动 miss 不混算）。
    let mut lib5 = ThumbLib::new();
    for k in 0..96u64 {
        lib5.lookup(k, 48, 1); // 冷库 96 miss
        lib5.insert(
            ThumbEntry { hash: k, tier: 48, stamp: 2, bytes: 64, checksum: 0xa5a5 + k, from_exif: false },
            2,
        );
    }
    let (h0, m0) = (lib5.hits, lib5.misses + lib5.corrupt_discards);
    for k in 0..96u64 {
        lib5.lookup(k, 48, 3); // 热库全命中
    }
    let dh = lib5.hits - h0;
    let dm = (lib5.misses + lib5.corrupt_discards) - m0;
    set.add("hit rate above 95pct on warm pass", dh == 96 && dm == 0, "");
    set.add("empty lib hit rate is none not fake", ThumbLib::new().hit_rate_ppm().is_none(), "");

    // EXIF 内嵌缩略直抽：构造最小合法 JPEG+EXIF。
    let mut jpeg = alloc::vec::Vec::new();
    jpeg.extend_from_slice(&[0xFF, 0xD8]); // SOI
    jpeg.extend_from_slice(&[0xFF, 0xE1]); // APP1
    let mut body = alloc::vec::Vec::new();
    body.extend_from_slice(b"Exif\0\0");
    body.extend_from_slice(b"II"); // little-endian
    body.extend_from_slice(&42u16.to_le_bytes());
    body.extend_from_slice(&8u32.to_le_bytes()); // IFD0 @ 8
    body.extend_from_slice(&2u16.to_le_bytes()); // 2 entries
    body.extend_from_slice(&0x501Bu16.to_le_bytes());
    body.extend_from_slice(&4u16.to_le_bytes());
    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend_from_slice(&16u32.to_le_bytes()); // offset=16
    body.extend_from_slice(&0x501Au16.to_le_bytes());
    body.extend_from_slice(&4u16.to_le_bytes());
    body.extend_from_slice(&1u32.to_le_bytes());
    body.extend_from_slice(&8u32.to_le_bytes()); // length=8
    body.extend_from_slice(&4u32.to_le_bytes()); // next IFD = 0
    body.extend_from_slice(&[0u8; 8]); // 缩略数据位
    let seg_len = (body.len() + 2) as u16;
    jpeg.extend_from_slice(&seg_len.to_be_bytes());
    jpeg.extend_from_slice(&body);
    jpeg.extend_from_slice(&[0xFF, 0xD9]);
    let loc = exif_thumb_locate(&jpeg);
    set.add("exif embedded thumb located", loc.is_some() && loc.unwrap().length == 8, "");
    // 对抗样本：截断/坏头不 panic 且拒绝。
    set.add("exif malformed rejected", exif_thumb_locate(&[0xFF, 0xD8, 0x00]).is_none(), "");
    set.add("exif non-jpeg rejected", exif_thumb_locate(&[0x89, 0x50, 0x4E, 0x47]).is_none(), "");

    // 引擎枢纽：miss → 占位渐进 → 命中。
    let mut eng = ThumbEngine::new();
    let o1 = eng.query(100, 256, PRI_VISIBLE, 10);
    let o2 = eng.query(100, 256, PRI_VISIBLE, 11);
    eng.complete(100, PLACEHOLDER_TIER, 32, 77, true, 12);
    let o3 = eng.query(100, 256, PRI_VISIBLE, 13);
    eng.complete(100, 256, 4096, 88, false, 14);
    let o4 = eng.query(100, 256, PRI_VISIBLE, 15);
    set.add("engine miss then placeholder then hit", o1 == QueryOutcome::Miss && o2 == QueryOutcome::Miss && o3 == QueryOutcome::Placeholder && o4 == QueryOutcome::Hit, "");
    // 占位档本身直出。
    let o5 = eng.query(100, PLACEHOLDER_TIER, PRI_VISIBLE, 16);
    set.add("placeholder tier hits directly", o5 == QueryOutcome::Hit, "");
    // 只读卷降级。
    eng.set_readonly_volume(true);
    set.add("ro volume caches local side", eng.ro_volume && eng.lib.local_side, "");
    // 多档共存（视图切换零等待）。
    set.add("multi tier coexist", eng.lib.lookup(100, 32, 20).0 == LibOp::Hit && eng.lib.lookup(100, 256, 20).0 == LibOp::Hit, "");
    // 库重建进度通知量。
    set.add("lib rebuild reports count", { let n = eng.lib.rebuild(); n == 2 }, "");

    set
}

// ---------------------------------------------------------------------------
// 单元测试（宿主 cargo test 直跑；时间全注入确定复现）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_priority_dedup_and_scroll_drop() {
        let mut q = GenQueue::new();
        q.push(GenReq { hash: 1, tier: 32, pri: PRI_BACKGROUND, enqueued_ms: 1 });
        q.push(GenReq { hash: 2, tier: 48, pri: PRI_VISIBLE, enqueued_ms: 2 });
        q.push(GenReq { hash: 3, tier: 96, pri: PRI_NEARBY, enqueued_ms: 3 });
        q.push(GenReq { hash: 2, tier: 48, pri: PRI_VISIBLE, enqueued_ms: 4 });
        assert_eq!(q.rejected_dup, 1);
        assert_eq!(q.pop().unwrap().hash, 2); // 可视区最急
        assert_eq!(q.pop().unwrap().hash, 3); // 邻近区次之
        assert_eq!(q.pop().unwrap().hash, 1); // 背景最后
        assert!(q.pop().is_none());
    }

    #[test]
    fn queue_cap_evicts_lower_priority_not_visible() {
        let mut q = GenQueue::new();
        for k in 0..QUEUE_CAP as u64 {
            q.push(GenReq { hash: k, tier: 48, pri: PRI_BACKGROUND, enqueued_ms: k });
        }
        assert_eq!(q.len(), QUEUE_CAP);
        // 可视区请求挤掉一条背景而非被拒。
        q.push(GenReq { hash: 9999, tier: 48, pri: PRI_VISIBLE, enqueued_ms: 10_000 });
        assert_eq!(q.len(), QUEUE_CAP);
        assert_eq!(q.evicted_low, 1);
        let top = q.pop().unwrap();
        assert_eq!(top.hash, 9999);
        // 出队后有空位：背景请求正常入队（len 回满）。
        q.push(GenReq { hash: 8888, tier: 48, pri: PRI_BACKGROUND, enqueued_ms: 10_001 });
        assert_eq!(q.len(), QUEUE_CAP);
        // 队满再推背景：无更低位可挤——诚实拒绝。
        q.push(GenReq { hash: 7777, tier: 48, pri: PRI_BACKGROUND, enqueued_ms: 10_002 });
        assert_eq!(q.len(), QUEUE_CAP);
        assert_eq!(q.rejected_drop, 1);
    }

    #[test]
    fn lru_eviction_order_by_last_use() {
        let mut lib = ThumbLib::new();
        let unit = LIB_CAP_BYTES / 4;
        for k in 0..5u64 {
            lib.insert(ThumbEntry { hash: k, tier: 48, stamp: k, bytes: unit, checksum: 1 + k, from_exif: false }, k);
            // 拨钟：让 0、1 保持最近使用。
            if k < 2 {
                lib.lookup(k, 48, 100 + k);
            }
        }
        assert!(lib.used_bytes() <= LIB_CAP_BYTES);
        assert!(lib.lookup(0, 48, 200).0 == LibOp::Miss, "最久未用的 0 应被逐出");
        assert!(lib.lookup(3, 48, 200).0 == LibOp::Hit, "较新的 3 应保留");
    }

    #[test]
    fn corrupt_selfheal_and_rebuild() {
        let mut lib = ThumbLib::new();
        lib.insert(ThumbEntry { hash: 5, tier: 48, stamp: 1, bytes: 0, checksum: 9, from_exif: false }, 1);
        let (op, _) = lib.lookup(5, 48, 2);
        assert_eq!(op, LibOp::CorruptDiscard);
        lib.insert(ThumbEntry { hash: 6, tier: 48, stamp: 2, bytes: 10, checksum: 2, from_exif: false }, 2);
        assert_eq!(lib.rebuild(), 1);
        assert_eq!(lib.entry_count(), 0);
        assert_eq!(lib.used_bytes(), 0);
    }

    #[test]
    fn defrag_removes_only_fragments() {
        let mut lib = ThumbLib::new();
        lib.insert(ThumbEntry { hash: 1, tier: 32, stamp: 1, bytes: 10, checksum: 1, from_exif: false }, 1);
        lib.insert(ThumbEntry { hash: 2, tier: 32, stamp: 1, bytes: 0, checksum: 0, from_exif: false }, 1);
        assert_eq!(lib.defrag_idle(), 1);
        assert_eq!(lib.entry_count(), 1);
    }

    #[test]
    fn exif_locate_rejects_truncations() {
        // 各类畸形输入零 panic 全拒绝（fuzz 前哨——F176 同纪律）。
        let samples: Vec<alloc::vec::Vec<u8>> = vec![
            alloc::vec![],
            alloc::vec![0xFF],
            alloc::vec![0xFF, 0xD8],
            alloc::vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00],
            alloc::vec![0xFF, 0xD8, 0xFF, 0xE1, 0x00, 0x04, 0x00, 0x00],
        ];
        for s in &samples {
            assert!(exif_thumb_locate(s).is_none(), "truncated sample must be rejected");
        }
    }

    #[test]
    fn engine_progressive_flow_with_hash_change() {
        let mut eng = ThumbEngine::new();
        let h = fnv1a64(b"photo1.jpg");
        assert_eq!(eng.query(h, 96, PRI_VISIBLE, 1), QueryOutcome::Miss);
        eng.complete(h, 96, 2048, 11, false, 2);
        assert_eq!(eng.query(h, 96, PRI_VISIBLE, 3), QueryOutcome::Hit);
        // 原图变更：新哈希（内容哈希随内容）天然 Miss，旧档失效。
        let h2 = fnv1a64(b"photo1.jpg.edited");
        assert_eq!(eng.query(h2, 96, PRI_VISIBLE, 4), QueryOutcome::Miss);
        assert_eq!(eng.lib.invalidate(h), 1);
        assert_eq!(eng.lib.lookup(h, 96, 5).0, LibOp::Miss);
    }

    #[test]
    fn hit_rate_matches_criteria_after_warmup() {
        let mut eng = ThumbEngine::new();
        // 二次浏览模型：首轮全 miss + 生成；次轮全 hit——命中率按**次轮
        // 增量**核算（库级累计口径含冷启动 miss，不能混算）。
        for k in 0..200u64 {
            let h = fnv1a64(format!("img{k}").as_bytes());
            eng.query(h, 48, PRI_BACKGROUND, k);
            eng.complete(h, 48, 96, 1 + k, false, k);
        }
        let (h0, m0) = (eng.lib.hits, eng.lib.misses + eng.lib.corrupt_discards);
        for k in 0..200u64 {
            let h = fnv1a64(format!("img{k}").as_bytes());
            assert_eq!(eng.query(h, 48, PRI_BACKGROUND, 1000 + k), QueryOutcome::Hit);
        }
        let dh = eng.lib.hits - h0;
        let dm = (eng.lib.misses + eng.lib.corrupt_discards) - m0;
        let rate = dh * 1_000_000 / (dh + dm).max(1);
        assert!(rate >= 950_000, "二次浏览命中率 >95%：{rate} ppm");
    }
}
