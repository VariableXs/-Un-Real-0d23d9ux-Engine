//! UNX-F48843 MT 协议 A/B 双协议识别 · slot 表与槽位管理 · 0x70540
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   F48843 ① 纯功能代码 ≥400 行；
//!          ② UNX-F48843-J1 双协议 ×10 设备识别正确；
//!   F48844 ① 纯功能代码 ≥360 行；
//!          ② UNX-F48844-J1 slot ×10^2 分配回收零泄漏。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：元数据完整性 → 能力码可判 → 协议唯一 → 一次读取锁存 →
//!           slot 界内 → 槽位空闲 → 代际匹配 → 回收对账。
//!   次序不可倒：元数据缺 X/Y 就判协议，会把「轴都没上报的残设备」误
//!   认成协议 A；不在能力码层先分 A/B 就直接看 slot 事件，会把协议 A
//!   的隐式槽位当成 0 号槽反复覆盖；代际校验必须先于回收动作，否则
//!   已复用的槽位会被一张旧句柄二次释放，账面上看不出泄漏却双重计费。
//! 关键差异：本模块只回答两个问题——「这台设备是 A 还是 B」与「这个
//!   slot 句柄现在还活着吗」。A/B 的逐帧轴解析归 unx48845_mt_guard，
//!   触点轨迹归 unx48848_mt_dispatch。本模块绝不接受原始事件流，只接受
//!   枚举期一次读到的能力码与调用方递交的 slot 操作；任何原始轴值带出
//!   本模块都视为越界（诊断码一律落 0x70540 窗口）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70540，VX6 批 B03 第二槽）。
pub const SEG_BASE: u32 = 0x70540;

/// 槽位上限（冻结 10：与 5 触点并发判据留 2 倍余量）。
pub const MAX_SLOTS: usize = 10;
/// 代际初值（0 恒为「未分配」哨兵，绝不出现在活句柄里）。
pub const GEN_FIRST: u32 = 1;
/// 代际上限（到顶即烧槽，防回绕重用）。
pub const GEN_MAX: u32 = 0xFFFF;
/// 判据设备数（F48843-J1 ×10）。
pub const DEVICES: usize = 10;
/// 判据分配回收轮数（F48844-J1 ×10^2）。
pub const CYCLES: u32 = 100;

// ── 诊断码闭集（+0x01..+0x0C） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MtCode {
    /// 元数据缺项：vid/pid/轴范围任一为零
    MetaBad,
    /// 能力码无法判定协议：缺 X/Y 或 slot 与 tracking 互相矛盾
    ProtoBad,
    /// 二次探测：协议已在枚举期锁存
    ProbeLocked,
    /// 重复设备：boot_id 或 vid/pid/fw 三元组已登记
    DuplicateDev,
    /// 槽位越界或全表耗尽
    SlotOver,
    /// 槽位未分配
    SlotFree,
    /// 槽位已占用
    SlotBusy,
    /// 句柄代际过期（已被回收或复用）
    GenStale,
    /// 代际到顶：该槽已烧毁
    GenOver,
    /// 回收对账不平：分配数 ≠ 回收数 + 在用量
    LeakBad,
    /// 重放不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl MtCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            MtCode::MetaBad => "元数据缺项：vid/pid/轴范围必须非零",
            MtCode::ProtoBad => "协议不可判定：缺 ABS_MT_POSITION_X/Y 或 slot 与 tracking 矛盾",
            MtCode::ProbeLocked => "协议已锁存：枚举期一次读取后禁止二次探测",
            MtCode::DuplicateDev => "重复设备：boot_id 或 vid/pid/fw 三元组已登记",
            MtCode::SlotOver => "槽位越界或全表耗尽：只允许 0..=9",
            MtCode::SlotFree => "槽位未分配：句柄指向空槽",
            MtCode::SlotBusy => "槽位已占用：不得重复分配同一槽",
            MtCode::GenStale => "句柄代际过期：槽位已被回收并复用",
            MtCode::GenOver => "代际到顶：槽位已烧毁，禁止回绕重用",
            MtCode::LeakBad => "回收对账不平：分配数 ≠ 回收数 + 在用量",
            MtCode::ReplayBad => "重放不一致：操作日志重演结果不同",
            MtCode::SegmentBad => "段位越界：诊断码逃出 0x70540 窗口",
        }
    }

    pub const ALL: [MtCode; 12] = [
        MtCode::MetaBad,
        MtCode::ProtoBad,
        MtCode::ProbeLocked,
        MtCode::DuplicateDev,
        MtCode::SlotOver,
        MtCode::SlotFree,
        MtCode::SlotBusy,
        MtCode::GenStale,
        MtCode::GenOver,
        MtCode::LeakBad,
        MtCode::ReplayBad,
        MtCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MtCode::ALL.len() {
        if MtCode::ALL[i].val() == code {
            return MtCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x70540 段闭集内"
}

// ── Linux input 轴码冻结（本模块只认这七枚） ────────────────────────
/// EV_ABS 事件类型。
pub const EV_ABS: u16 = 0x03;
/// ABS_MT_SLOT：协议 B 的槽位选择码。
pub const ABS_MT_SLOT: u16 = 0x2F;
/// ABS_MT_TOUCH_MAJOR：接触长轴。
pub const ABS_MT_TOUCH_MAJOR: u16 = 0x30;
/// ABS_MT_POSITION_X：接触点 X。
pub const ABS_MT_POSITION_X: u16 = 0x35;
/// ABS_MT_POSITION_Y：接触点 Y。
pub const ABS_MT_POSITION_Y: u16 = 0x36;
/// ABS_MT_TRACKING_ID：触点评分。
pub const ABS_MT_TRACKING_ID: u16 = 0x39;
/// ABS_MT_PRESSURE：压力轴。
pub const ABS_MT_PRESSURE: u16 = 0x3A;

/// 能力码表查询（线性扫描，能力码表最多十几项，不做散列）。
pub fn caps_has(caps: &[u16], code: u16) -> bool {
    let mut i = 0;
    while i < caps.len() {
        match caps.get(i) {
            Some(c) => {
                if *c == code {
                    return true;
                }
            }
            None => return false,
        }
        i += 1;
    }
    false
}

// ── 协议识别（枚举期一次读取） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MtProto {
    /// 协议 A：无 ABS_MT_SLOT，逐包携带完整触点状态
    A,
    /// 协议 B：ABS_MT_SLOT 选槽 + ABS_MT_TRACKING_ID 定触点
    B,
}

impl MtProto {
    pub fn text(self) -> &'static str {
        match self {
            MtProto::A => "MT-A",
            MtProto::B => "MT-B",
        }
    }

    pub fn needs_slot(self) -> bool {
        match self {
            MtProto::A => false,
            MtProto::B => true,
        }
    }
}

/// 协议判定：能力码 → A/B。
///
/// 判死条件（任一即 ProtoBad）：
/// - X 与 Y 必须同时出现（只有 X 的残设备不可识别）；
/// - B 必须同时具备 SLOT 与 TRACKING_ID（只有 SLOT 是固件半成品）；
/// - A 不得携带 SLOT（携带即说明它是 B，不能降格按 A 解析）。
pub fn detect(caps: &[u16]) -> Result<MtProto, u32> {
    let x = caps_has(caps, ABS_MT_POSITION_X);
    let y = caps_has(caps, ABS_MT_POSITION_Y);
    if !x || !y {
        return Err(MtCode::ProtoBad.val());
    }
    let slot = caps_has(caps, ABS_MT_SLOT);
    let track = caps_has(caps, ABS_MT_TRACKING_ID);
    match (slot, track) {
        (true, true) => Ok(MtProto::B),
        (false, true) => Ok(MtProto::A),
        (false, false) => {
            // 无 tracking_id 的单点屏：按协议 A 的最简形态接收，
            // 但必须至少有压力或长轴之一，否则视为残设备。
            let major = caps_has(caps, ABS_MT_TOUCH_MAJOR);
            let press = caps_has(caps, ABS_MT_PRESSURE);
            if major || press {
                Ok(MtProto::A)
            } else {
                Err(MtCode::ProtoBad.val())
            }
        }
        (true, false) => Err(MtCode::ProtoBad.val()),
    }
}

// ── 设备元数据 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DeviceMeta {
    /// 枚举序号：一次启动内唯一，热插拔后复用。
    pub boot_id: u32,
    pub vid: u16,
    pub pid: u16,
    /// 固件版本：quirk 表按它分档。
    pub fw: u32,
    /// X 轴量程（原始单位）。
    pub max_x: u32,
    /// Y 轴量程（原始单位）。
    pub max_y: u32,
    /// 枚举期一次读到的协议。
    pub proto: MtProto,
}

impl DeviceMeta {
    pub fn new(
        boot_id: u32,
        vid: u16,
        pid: u16,
        fw: u32,
        max_x: u32,
        max_y: u32,
        proto: MtProto,
    ) -> DeviceMeta {
        DeviceMeta { boot_id, vid, pid, fw, max_x, max_y, proto }
    }

    pub fn text(&self) -> String {
        format!(
            "dev{} {:04X}:{:04X} fw={} range={}x{} {}",
            self.boot_id,
            self.vid,
            self.pid,
            self.fw,
            self.max_x,
            self.max_y,
            self.proto.text()
        )
    }

    /// 元数据完备性：四元组 + 量程全部非零。
    pub fn complete(&self) -> bool {
        self.vid != 0 && self.pid != 0 && self.max_x != 0 && self.max_y != 0
    }

    /// 同型号判定（vid/pid/fw 三元组）。
    pub fn same_model(&self, other: &DeviceMeta) -> bool {
        self.vid == other.vid && self.pid == other.pid && self.fw == other.fw
    }
}

/// 从能力码一次生成元数据（这是「一次读取」的唯一入口）。
pub fn probe_meta(
    boot_id: u32,
    vid: u16,
    pid: u16,
    fw: u32,
    max_x: u32,
    max_y: u32,
    caps: &[u16],
) -> Result<DeviceMeta, u32> {
    if vid == 0 || pid == 0 || max_x == 0 || max_y == 0 {
        return Err(MtCode::MetaBad.val());
    }
    let proto = detect(caps)?;
    Ok(DeviceMeta::new(boot_id, vid, pid, fw, max_x, max_y, proto))
}

// ── 设备注册表（协议锁存 + 重复拒） ───────────────────────────────
pub struct DeviceRegistry {
    entries: Vec<DeviceMeta>,
    probe_attempts: u32,
    locked_rejects: u32,
    dup_rejects: u32,
}

impl DeviceRegistry {
    pub fn new() -> DeviceRegistry {
        DeviceRegistry {
            entries: Vec::new(),
            probe_attempts: 0,
            locked_rejects: 0,
            dup_rejects: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn probe_attempts(&self) -> u32 {
        self.probe_attempts
    }

    pub fn locked_rejects(&self) -> u32 {
        self.locked_rejects
    }

    pub fn dup_rejects(&self) -> u32 {
        self.dup_rejects
    }

    /// 登记：元数据完备 + boot_id 唯一 + 同型号不重复占用。
    ///
    /// 为什么同型号也拒：同型号第二台设备走同一 quirk 分档，协议必然
    /// 相同；若出现「同型号不同协议」，说明固件版本号被复用，必须直接
    /// 拒绝而不是让第二台覆盖第一台的协议锁存。
    pub fn register(&mut self, meta: DeviceMeta) -> Result<usize, u32> {
        if !meta.complete() {
            return Err(MtCode::MetaBad.val());
        }
        let mut i = 0;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) => {
                    if e.boot_id == meta.boot_id || e.same_model(&meta) {
                        self.dup_rejects = self.dup_rejects.saturating_add(1);
                        return Err(MtCode::DuplicateDev.val());
                    }
                }
                None => return Err(MtCode::SegmentBad.val()),
            }
            i += 1;
        }
        self.entries.push(meta);
        Ok(self.entries.len() - 1)
    }

    /// 二次探测：命中已登记设备一律 ProbeLocked。
    ///
    /// 判据语义：协议在枚举期读取一次后就是设备元数据的一部分；运行期
    /// 再探测只允许「比对」，不允许「改写」，比对不一致返回 ProtoBad。
    pub fn reprobe(&mut self, boot_id: u32, caps: &[u16]) -> Result<MtProto, u32> {
        self.probe_attempts = self.probe_attempts.saturating_add(1);
        let seen = detect(caps)?;
        let found = self.lookup(boot_id)?;
        if found.proto != seen {
            self.locked_rejects = self.locked_rejects.saturating_add(1);
            return Err(MtCode::ProtoBad.val());
        }
        self.locked_rejects = self.locked_rejects.saturating_add(1);
        Err(MtCode::ProbeLocked.val())
    }

    pub fn lookup(&self, boot_id: u32) -> Result<DeviceMeta, u32> {
        let mut i = 0;
        while i < self.entries.len() {
            match self.entries.get(i) {
                Some(e) => {
                    if e.boot_id == boot_id {
                        return Ok(*e);
                    }
                }
                None => return Err(MtCode::SegmentBad.val()),
            }
            i += 1;
        }
        Err(MtCode::MetaBad.val())
    }

    /// 协议分账：返回 (A 台数, B 台数)。
    pub fn proto_counts(&self) -> (usize, usize) {
        let mut a = 0usize;
        let mut b = 0usize;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                match e.proto {
                    MtProto::A => a += 1,
                    MtProto::B => b += 1,
                }
            }
            i += 1;
        }
        (a, b)
    }

    /// 注册表指纹（FNV-1a，按 boot_id 排序无关的顺序折叠）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                h = fold_word(h, e.boot_id);
                h = fold_word(h, e.vid as u32);
                h = fold_word(h, e.pid as u32);
                h = fold_word(h, e.fw);
                h = fold_word(h, e.max_x);
                h = fold_word(h, e.max_y);
                h = fold_byte(h, if e.proto.needs_slot() { 0x42 } else { 0x41 });
            }
            i += 1;
        }
        h
    }
}

// ── FNV-1a（本域基础） ────────────────────────────────────────────
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_word(h: u64, w: u32) -> u64 {
    let mut x = h;
    x = fold_byte(x, (w & 0xFF) as u8);
    x = fold_byte(x, ((w >> 8) & 0xFF) as u8);
    x = fold_byte(x, ((w >> 16) & 0xFF) as u8);
    fold_byte(x, ((w >> 24) & 0xFF) as u8)
}

pub fn fold_str(h: u64, s: &str) -> u64 {
    let bytes = s.as_bytes();
    let mut x = h;
    let mut i = 0;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            x = fold_byte(x, *b);
        }
        i += 1;
    }
    x
}

// ── slot 表（代际防重用） ─────────────────────────────────────────
/// 活句柄 = 槽位号 + 代际。任何一方不匹配都视为悬空。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotHandle {
    pub slot: u8,
    pub gen: u32,
}

impl SlotHandle {
    pub fn new(slot: u8, gen: u32) -> SlotHandle {
        SlotHandle { slot, gen }
    }

    pub fn text(&self) -> String {
        format!("slot{}#{}", self.slot, self.gen)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SlotEntry {
    pub used: bool,
    pub retired: bool,
    pub gen: u32,
    pub tracking: u16,
}

impl SlotEntry {
    pub fn empty() -> SlotEntry {
        SlotEntry { used: false, retired: false, gen: 0, tracking: 0 }
    }
}

/// slot 操作日志：重放的唯一输入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotOp {
    Alloc { tracking: u16 },
    Free { slot: u8, gen: u32 },
}

pub struct SlotTable {
    slots: [SlotEntry; MAX_SLOTS],
    log: Vec<SlotOp>,
    alloc_total: u32,
    recycle_total: u32,
    stale_rejects: u32,
    burn_total: u32,
}

impl SlotTable {
    pub fn new() -> SlotTable {
        SlotTable {
            slots: [SlotEntry::empty(); MAX_SLOTS],
            log: Vec::new(),
            alloc_total: 0,
            recycle_total: 0,
            stale_rejects: 0,
            burn_total: 0,
        }
    }

    pub fn alloc_total(&self) -> u32 {
        self.alloc_total
    }

    pub fn recycle_total(&self) -> u32 {
        self.recycle_total
    }

    pub fn stale_rejects(&self) -> u32 {
        self.stale_rejects
    }

    pub fn burn_total(&self) -> u32 {
        self.burn_total
    }

    pub fn log_len(&self) -> usize {
        self.log.len()
    }

    pub fn used_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0;
        while i < MAX_SLOTS {
            if let Some(e) = self.slots.get(i) {
                if e.used {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    pub fn entry(&self, slot: u8) -> Result<SlotEntry, u32> {
        match self.slots.get(slot as usize) {
            Some(e) => Ok(*e),
            None => Err(MtCode::SlotOver.val()),
        }
    }

    /// 分配：首个「未占用且未烧毁」槽。
    ///
    /// 代际推进：首次使用置 GEN_FIRST；回收后再分配 advance_gen 一次。
    /// 到顶不回头：烧槽并返回 GenOver（该槽永久退出池），由调用方
    /// 换槽；这样旧句柄永远不可能在回绕后重新命中。
    pub fn alloc(&mut self, tracking: u16) -> Result<SlotHandle, u32> {
        let mut i = 0usize;
        while i < MAX_SLOTS {
            let free = match self.slots.get(i) {
                Some(e) => !e.used && !e.retired,
                None => false,
            };
            if free {
                let next_gen = match self.slots.get(i) {
                    Some(e) => {
                        if e.gen == 0 {
                            GEN_FIRST
                        } else {
                            match e.gen.checked_add(1) {
                                Some(v) => v,
                                None => GEN_MAX,
                            }
                        }
                    }
                    None => GEN_FIRST,
                };
                if next_gen > GEN_MAX {
                    if let Some(e) = self.slots.get_mut(i) {
                        e.retired = true;
                    }
                    self.burn_total = self.burn_total.saturating_add(1);
                    return Err(MtCode::GenOver.val());
                }
                if let Some(e) = self.slots.get_mut(i) {
                    e.used = true;
                    e.gen = next_gen;
                    e.tracking = tracking;
                }
                self.alloc_total = self.alloc_total.saturating_add(1);
                self.log.push(SlotOp::Alloc { tracking });
                return Ok(SlotHandle::new(i as u8, next_gen));
            }
            i += 1;
        }
        Err(MtCode::SlotOver.val())
    }

    /// 回收：槽位在界 + 已占用 + 代际匹配，三闸全过才回收。
    pub fn free(&mut self, h: SlotHandle) -> Result<(), u32> {
        let idx = h.slot as usize;
        let entry = match self.slots.get(idx) {
            Some(e) => *e,
            None => return Err(MtCode::SlotOver.val()),
        };
        if !entry.used {
            self.stale_rejects = self.stale_rejects.saturating_add(1);
            return Err(MtCode::SlotFree.val());
        }
        if entry.gen != h.gen {
            self.stale_rejects = self.stale_rejects.saturating_add(1);
            return Err(MtCode::GenStale.val());
        }
        if let Some(e) = self.slots.get_mut(idx) {
            e.used = false;
            e.tracking = 0;
            if e.gen >= GEN_MAX {
                e.retired = true;
                self.burn_total = self.burn_total.saturating_add(1);
            }
        }
        self.recycle_total = self.recycle_total.saturating_add(1);
        self.log.push(SlotOp::Free { slot: h.slot, gen: h.gen });
        Ok(())
    }

    /// 按 tracking_id 回收（触点抬起只带 tracking 时的归一入口）。
    pub fn free_tracking(&mut self, tracking: u16) -> Result<SlotHandle, u32> {
        let mut i = 0usize;
        while i < MAX_SLOTS {
            let hit = match self.slots.get(i) {
                Some(e) => e.used && e.tracking == tracking,
                None => false,
            };
            if hit {
                let h = match self.slots.get(i) {
                    Some(e) => SlotHandle::new(i as u8, e.gen),
                    None => return Err(MtCode::SlotOver.val()),
                };
                self.free(h)?;
                return Ok(h);
            }
            i += 1;
        }
        Err(MtCode::SlotFree.val())
    }

    /// 句柄校验：在界 + 占用 + 代际匹配。
    pub fn validate(&self, h: SlotHandle) -> Result<(), u32> {
        let entry = match self.slots.get(h.slot as usize) {
            Some(e) => *e,
            None => return Err(MtCode::SlotOver.val()),
        };
        if !entry.used {
            return Err(MtCode::SlotFree.val());
        }
        if entry.gen != h.gen {
            return Err(MtCode::GenStale.val());
        }
        Ok(())
    }

    /// 对账：alloc_total == recycle_total + used_count。
    ///
    /// 烧槽不影响该恒等式：烧毁发生在 free 之后，槽位已经计入
    /// recycle_total，不会重复计。
    pub fn audit(&self) -> Result<(), u32> {
        let used = self.used_count() as u32;
        if self.alloc_total != self.recycle_total.saturating_add(used) {
            return Err(MtCode::LeakBad.val());
        }
        let mut i = 0usize;
        while i < MAX_SLOTS {
            match self.slots.get(i) {
                Some(e) => {
                    if e.retired && e.used {
                        return Err(MtCode::LeakBad.val());
                    }
                    if e.used && e.gen == 0 {
                        return Err(MtCode::GenStale.val());
                    }
                }
                None => return Err(MtCode::SegmentBad.val()),
            }
            i += 1;
        }
        Ok(())
    }

    /// 表指纹：槽位状态 + 三本计数全折叠。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < MAX_SLOTS {
            if let Some(e) = self.slots.get(i) {
                h = fold_byte(h, if e.used { 1 } else { 0 });
                h = fold_byte(h, if e.retired { 2 } else { 0 });
                h = fold_word(h, e.gen);
                h = fold_word(h, e.tracking as u32);
            }
            i += 1;
        }
        h = fold_word(h, self.alloc_total);
        h = fold_word(h, self.recycle_total);
        h = fold_word(h, self.stale_rejects);
        h = fold_word(h, self.burn_total);
        fold_word(h, self.log.len() as u32)
    }

    /// 重放：按日志重建，逐条与现值对齐。
    pub fn replay(&self) -> Result<u64, u32> {
        let mut rebuilt = SlotTable::new();
        let mut i = 0usize;
        while i < self.log.len() {
            match self.log.get(i) {
                Some(SlotOp::Alloc { tracking }) => {
                    let _ = rebuilt.alloc(*tracking)?;
                }
                Some(SlotOp::Free { slot, gen }) => {
                    rebuilt.free(SlotHandle::new(*slot, *gen))?;
                }
                None => return Err(MtCode::ReplayBad.val()),
            }
            i += 1;
        }
        if rebuilt.seal() != self.seal() {
            return Err(MtCode::ReplayBad.val());
        }
        Ok(rebuilt.seal())
    }
}

// ── 黄金全链：十设备识别 + 百轮分配回收 + 代际拒 + 对账 + 重放 ────
/// 协议 A 能力码（无 SLOT，有 TRACKING）。
pub fn caps_a() -> Vec<u16> {
    vec![ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_TRACKING_ID, ABS_MT_PRESSURE]
}

/// 协议 B 能力码（有 SLOT + TRACKING）。
pub fn caps_b() -> Vec<u16> {
    vec![
        ABS_MT_SLOT,
        ABS_MT_POSITION_X,
        ABS_MT_POSITION_Y,
        ABS_MT_TRACKING_ID,
        ABS_MT_TOUCH_MAJOR,
    ]
}

/// 十台设备语料：前 5 台 A，后 5 台 B，vid/pid/fw 互不相同。
pub fn ten_devices() -> Vec<(u32, u16, u16, u32, u32, u32, MtProto)> {
    vec![
        (1, 0x1A2B, 0x0011, 100, 4096, 4096, MtProto::A),
        (2, 0x1A2B, 0x0012, 101, 4096, 4096, MtProto::A),
        (3, 0x0EEF, 0x2001, 7, 2048, 1536, MtProto::A),
        (4, 0x0EEF, 0x2002, 8, 2048, 1536, MtProto::A),
        (5, 0x0534, 0x0300, 3, 1024, 768, MtProto::A),
        (6, 0x04F3, 0x3100, 55, 8192, 8192, MtProto::B),
        (7, 0x04F3, 0x3101, 56, 8192, 8192, MtProto::B),
        (8, 0x06CB, 0x7E10, 9, 6144, 4096, MtProto::B),
        (9, 0x06CB, 0x7E11, 10, 6144, 4096, MtProto::B),
        (10, 0x2575, 0x0A01, 1, 1280, 720, MtProto::B),
    ]
}

/// 识别十设备，返回 (识别数, A 数, B 数)。
pub fn golden_detect() -> Result<(usize, usize, usize), u32> {
    let mut reg = DeviceRegistry::new();
    let spec = ten_devices();
    let mut i = 0usize;
    while i < spec.len() {
        let (id, vid, pid, fw, mx, my, want) = match spec.get(i) {
            Some(v) => *v,
            None => return Err(MtCode::SegmentBad.val()),
        };
        let caps = match want {
            MtProto::A => caps_a(),
            MtProto::B => caps_b(),
        };
        let meta = probe_meta(id, vid, pid, fw, mx, my, &caps)?;
        if meta.proto != want {
            return Err(MtCode::ProtoBad.val());
        }
        reg.register(meta)?;
        i += 1;
    }
    let (a, b) = reg.proto_counts();
    if reg.len() != DEVICES || a + b != DEVICES {
        return Err(MtCode::MetaBad.val());
    }
    Ok((reg.len(), a, b))
}

/// 百轮分配回收：10 槽轮转，每轮分配 10 次、回收 10 次。
pub fn golden_slots(cycles: u32) -> Result<SlotTable, u32> {
    let mut t = SlotTable::new();
    let mut r = 0u32;
    let mut tracking = 1u16;
    while r < cycles {
        let mut hs: Vec<SlotHandle> = Vec::new();
        let mut n = 0usize;
        while n < MAX_SLOTS {
            let h = t.alloc(tracking)?;
            hs.push(h);
            tracking = tracking.wrapping_add(1);
            n += 1;
        }
        let mut k = 0usize;
        while k < hs.len() {
            if let Some(h) = hs.get(k) {
                t.free(*h)?;
            }
            k += 1;
        }
        r += 1;
    }
    t.audit()?;
    Ok(t)
}

/// 代际防重用：回收→复用后，旧句柄必须 GenStale。
pub fn golden_generation() -> Result<u32, u32> {
    let mut t = SlotTable::new();
    let old = t.alloc(0x1001)?;
    t.free(old)?;
    let new = t.alloc(0x1002)?;
    if new.slot != old.slot || new.gen == old.gen {
        return Err(MtCode::GenStale.val());
    }
    match t.validate(old) {
        Err(e) => {
            if e != MtCode::GenStale.val() {
                return Err(e);
            }
        }
        Ok(_) => return Err(MtCode::GenStale.val()),
    }
    t.validate(new)?;
    if t.free(old) != Err(MtCode::GenStale.val()) {
        return Err(MtCode::GenStale.val());
    }
    Ok(new.gen - old.gen)
}

/// 全链：识别 → 分账 → 百轮 → 代际 → 对账 → 重放。
pub fn golden_run() -> Result<(usize, u32, u32, u64), u32> {
    let (n, _a, _b) = golden_detect()?;
    let t = golden_slots(CYCLES)?;
    let gap = golden_generation()?;
    if gap != 1 {
        return Err(MtCode::GenStale.val());
    }
    let sealed = t.seal();
    let replayed = t.replay()?;
    if replayed != sealed {
        return Err(MtCode::ReplayBad.val());
    }
    Ok((n, t.alloc_total(), t.recycle_total(), sealed))
}

// ── LCG（注入轮驱动） ─────────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入账（四路：坏元数据/歧义能力/过期句柄/泄漏） ───────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 注入轮：每轮四路，全部必须被拒。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：vid=0 的残元数据。
        let bad_meta = probe_meta(1, 0, 1, 1, 100, 100, &caps_a()).is_err();
        led.record(0, bad_meta);
        // 路 2：缺 Y 轴的歧义能力。
        let bad_caps = vec![ABS_MT_POSITION_X, ABS_MT_TRACKING_ID];
        let amb = detect(&bad_caps).is_err()
            && detect(&[ABS_MT_SLOT, ABS_MT_TRACKING_ID]).is_err();
        led.record(1, amb);
        // 路 3：回收复用后的旧句柄。
        let mut t = SlotTable::new();
        let stale = match t.alloc(7) {
            Ok(old) => match t.free(old) {
                Ok(()) => match t.alloc(8) {
                    Ok(_new) => t.validate(old) == Err(MtCode::GenStale.val()),
                    Err(_) => false,
                },
                Err(_) => false,
            },
            Err(_) => false,
        };
        led.record(2, stale);
        // 路 4：回收后再回收（二次释放）。
        let mut twice = SlotTable::new();
        let caught = match twice.alloc(9) {
            Ok(h) => twice.free(h).is_ok() && twice.free(h).err() == Some(MtCode::SlotFree.val()),
            Err(_) => false,
        };
        led.record(3, caught);
        r = r.saturating_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
/// 序 A：十设备识别 + 百轮零泄漏。
pub fn probe_seq_a() -> Result<(), u32> {
    let (n, alloc, recycle, _) = golden_run()?;
    if n == DEVICES && alloc == CYCLES.saturating_mul(MAX_SLOTS as u32) && recycle == alloc {
        Ok(())
    } else {
        Err(MtCode::LeakBad.val())
    }
}

/// 序 B：非法输入逐路拒（残缺能力/二次探测/越界槽）。
pub fn probe_seq_b() -> Result<(), u32> {
    if detect(&[]).is_err() {
        // ok
    } else {
        return Err(MtCode::ProtoBad.val());
    }
    let mut reg = DeviceRegistry::new();
    let meta = probe_meta(1, 0x1111, 0x2222, 1, 100, 100, &caps_b())?;
    reg.register(meta)?;
    if reg.reprobe(1, &caps_a()) != Err(MtCode::ProtoBad.val()) {
        return Err(MtCode::ProbeLocked.val());
    }
    let t = SlotTable::new();
    if t.entry(MAX_SLOTS as u8).err() != Some(MtCode::SlotOver.val()) {
        return Err(MtCode::SlotOver.val());
    }
    Ok(())
}

/// 序 C：冻结常量（10 槽 / 10 设备 / 100 轮 / 代际上限 0xFFFF）。
pub fn probe_seq_c() -> Result<(), u32> {
    if MAX_SLOTS == 10 && DEVICES == 10 && CYCLES == 100 && GEN_MAX == 0xFFFF && GEN_FIRST == 1
    {
        Ok(())
    } else {
        Err(MtCode::SegmentBad.val())
    }
}

/// 双跑一致。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MtCode::ReplayBad.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < MtCode::ALL.len() {
        let v = MtCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x20 {
            return false;
        }
        i += 1;
    }
    true
}

/// 语料审计：十设备规格与能力码一一对应。
pub fn audit_corpus() -> Result<usize, u32> {
    let spec = ten_devices();
    if spec.len() != DEVICES {
        return Err(MtCode::MetaBad.val());
    }
    let mut a = 0usize;
    let mut b = 0usize;
    let mut i = 0usize;
    while i < spec.len() {
        let (_, _, _, _, _, _, p) = match spec.get(i) {
            Some(v) => *v,
            None => return Err(MtCode::SegmentBad.val()),
        };
        match p {
            MtProto::A => {
                if detect(&caps_a())? != MtProto::A {
                    return Err(MtCode::ProtoBad.val());
                }
                a += 1;
            }
            MtProto::B => {
                if detect(&caps_b())? != MtProto::B {
                    return Err(MtCode::ProtoBad.val());
                }
                b += 1;
            }
        }
        i += 1;
    }
    if a != 5 || b != 5 {
        return Err(MtCode::ProtoBad.val());
    }
    Ok(a + b)
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let det = golden_detect();
    v.push((
        "F48843-J1-双协议十设备识别",
        match &det {
            Ok((n, a, b)) => *n == 10 && *a == 5 && *b == 5,
            Err(_) => false,
        },
        match &det {
            Ok((n, a, b)) => format!("devices={} A={} B={}", n, a, b),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let one = probe_meta(77, 0x1234, 0x5678, 3, 800, 600, &caps_b());
    v.push((
        "F48843-J1-一次读取锁存协议",
        match &one {
            Ok(m) => m.proto == MtProto::B && m.complete() && m.boot_id == 77,
            Err(_) => false,
        },
        match &one {
            Ok(m) => m.text(),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48843-J1-歧义能力拒",
        detect(&[ABS_MT_POSITION_X, ABS_MT_SLOT, ABS_MT_TRACKING_ID]).is_err()
            && detect(&[]).is_err()
            && detect(&[ABS_MT_SLOT]).is_err(),
        String::from("缺 Y / 只有 SLOT / 空能力表 三例全拒"),
    ));
    let slots = golden_slots(CYCLES);
    v.push((
        "F48844-J1-百轮分配回收零泄漏",
        match &slots {
            Ok(t) => t.audit().is_ok() && t.alloc_total() == 1000 && t.recycle_total() == 1000,
            Err(_) => false,
        },
        match &slots {
            Ok(t) => format!("alloc={} recycle={} used={}", t.alloc_total(), t.recycle_total(), t.used_count()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let gen = golden_generation();
    v.push((
        "F48844-J1-代际防重用",
        matches!(gen, Ok(1)),
        match &gen {
            Ok(g) => format!("gap={}", g),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48844-J1-操作日志重放",
        match &slots {
            Ok(t) => t.replay().is_ok() && t.log_len() == 2000,
            Err(_) => false,
        },
        match &slots {
            Ok(t) => format!("log={} replay={:?}", t.log_len(), t.replay().ok()),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48843-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48843-J1-探针三序",
        probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(),
        format!(
            "a={} b={} c={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok()
        ),
    ));
    v.push((
        "F48844-J1-语料审计与双跑",
        audit_corpus().ok() == Some(10) && repro_check().is_ok(),
        format!("corpus={:?} repro={}", audit_corpus().ok(), repro_check().is_ok()),
    ));
    v.push((
        "F48844-J1-段位守卫",
        segment_guard(),
        String::from("0x70540 窗口 12 码"),
    ));
    v
}

pub fn run_unx48843_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48843 MT 协议 A/B 双协议识别");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据（宿主侧 cargo test） ─────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_protocol_a_without_slot() {
        assert_eq!(detect(&caps_a()).ok(), Some(MtProto::A));
    }

    #[test]
    fn detect_protocol_b_with_slot() {
        assert_eq!(detect(&caps_b()).ok(), Some(MtProto::B));
    }

    #[test]
    fn slot_without_tracking_is_rejected() {
        let caps = [ABS_MT_SLOT, ABS_MT_POSITION_X, ABS_MT_POSITION_Y];
        assert_eq!(detect(&caps).err(), Some(MtCode::ProtoBad.val()));
    }

    #[test]
    fn missing_y_is_rejected() {
        let caps = [ABS_MT_POSITION_X, ABS_MT_TRACKING_ID];
        assert_eq!(detect(&caps).err(), Some(MtCode::ProtoBad.val()));
    }

    #[test]
    fn single_point_without_tracking_needs_axis() {
        let bare = [ABS_MT_POSITION_X, ABS_MT_POSITION_Y];
        assert_eq!(detect(&bare).err(), Some(MtCode::ProtoBad.val()));
        let with_major = [ABS_MT_POSITION_X, ABS_MT_POSITION_Y, ABS_MT_TOUCH_MAJOR];
        assert_eq!(detect(&with_major).ok(), Some(MtProto::A));
    }

    #[test]
    fn incomplete_meta_is_rejected() {
        let e = probe_meta(1, 0, 0x1234, 1, 100, 100, &caps_a());
        assert_eq!(e.err(), Some(MtCode::MetaBad.val()));
    }

    #[test]
    fn duplicate_device_is_rejected() {
        let mut reg = DeviceRegistry::new();
        let m = probe_meta(1, 0x1111, 0x2222, 1, 100, 100, &caps_a()).ok();
        assert!(m.is_some());
        if let Some(meta) = m {
            assert!(reg.register(meta).is_ok());
            assert_eq!(reg.register(meta).err(), Some(MtCode::DuplicateDev.val()));
        }
    }

    #[test]
    fn reprobe_is_locked() {
        let mut reg = DeviceRegistry::new();
        let meta = probe_meta(3, 0x1A2B, 0x0011, 9, 4096, 4096, &caps_b());
        assert!(meta.is_ok());
        if let Ok(m) = meta {
            assert!(reg.register(m).is_ok());
            assert_eq!(reg.reprobe(3, &caps_b()).err(), Some(MtCode::ProbeLocked.val()));
        }
    }

    #[test]
    fn reprobe_mismatch_is_proto_bad() {
        let mut reg = DeviceRegistry::new();
        let meta = probe_meta(4, 0x1A2B, 0x0011, 9, 4096, 4096, &caps_b());
        assert!(meta.is_ok());
        if let Ok(m) = meta {
            assert!(reg.register(m).is_ok());
            assert_eq!(reg.reprobe(4, &caps_a()).err(), Some(MtCode::ProtoBad.val()));
        }
    }

    #[test]
    fn ten_devices_split_evenly() {
        assert_eq!(golden_detect().ok(), Some((10, 5, 5)));
    }

    #[test]
    fn alloc_returns_first_free_slot() {
        let mut t = SlotTable::new();
        let h = t.alloc(1);
        assert!(h.is_ok());
        if let Ok(hh) = h {
            assert_eq!(hh.slot, 0);
            assert_eq!(hh.gen, GEN_FIRST);
        }
    }

    #[test]
    fn full_table_exhausts() {
        let mut t = SlotTable::new();
        let mut n = 0usize;
        while n < MAX_SLOTS {
            assert!(t.alloc(n as u16).is_ok());
            n += 1;
        }
        assert_eq!(t.alloc(99).err(), Some(MtCode::SlotOver.val()));
    }

    #[test]
    fn stale_handle_is_rejected() {
        let mut t = SlotTable::new();
        let old = t.alloc(5);
        assert!(old.is_ok());
        if let Ok(o) = old {
            assert!(t.free(o).is_ok());
            assert!(t.alloc(6).is_ok());
            assert_eq!(t.validate(o).err(), Some(MtCode::GenStale.val()));
        }
    }

    #[test]
    fn double_free_reports_free() {
        let mut t = SlotTable::new();
        let h = t.alloc(5);
        assert!(h.is_ok());
        if let Ok(hh) = h {
            assert!(t.free(hh).is_ok());
            assert_eq!(t.free(hh).err(), Some(MtCode::SlotFree.val()));
        }
    }

    #[test]
    fn hundred_cycles_no_leak() {
        let t = golden_slots(CYCLES);
        assert!(t.is_ok());
        if let Ok(tt) = t {
            assert!(tt.audit().is_ok());
            assert_eq!(tt.alloc_total(), 1000);
            assert_eq!(tt.recycle_total(), 1000);
            assert_eq!(tt.used_count(), 0);
        }
    }

    #[test]
    fn outstanding_slot_stays_balanced() {
        let mut t = SlotTable::new();
        assert!(t.alloc(1).is_ok());
        assert_eq!(t.used_count(), 1);
        assert!(t.audit().is_ok());
        assert!(t.alloc(2).is_ok());
        assert_eq!(t.used_count(), 2);
        assert!(t.audit().is_ok());
    }

    #[test]
    fn free_by_tracking_recycles() {
        let mut t = SlotTable::new();
        assert!(t.alloc(0xABCD).is_ok());
        let h = t.free_tracking(0xABCD);
        assert!(h.is_ok());
        assert_eq!(t.used_count(), 0);
        assert_eq!(t.free_tracking(0xABCD).err(), Some(MtCode::SlotFree.val()));
    }

    #[test]
    fn replay_matches_seal() {
        let t = golden_slots(3);
        assert!(t.is_ok());
        if let Ok(tt) = t {
            assert_eq!(tt.replay().ok(), Some(tt.seal()));
        }
    }

    #[test]
    fn double_run_is_identical() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn corpus_audit_freezes_ten() {
        assert_eq!(audit_corpus().ok(), Some(10));
    }

    #[test]
    fn all_codes_inside_segment() {
        assert!(segment_guard());
        assert_eq!(MtCode::ALL.len(), 12);
        assert_eq!(MtCode::MetaBad.val(), SEG_BASE + 1);
        assert_eq!(MtCode::SegmentBad.val(), SEG_BASE + 12);
    }
}
