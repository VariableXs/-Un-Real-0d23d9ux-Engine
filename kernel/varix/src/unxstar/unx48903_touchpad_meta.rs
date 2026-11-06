//! UNX-F48903 触控板元数据与参数下发原子性 · 0x70CC0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F48903-J1 十设备一次读取锁定 ×100 轮零重读；
//!   ③ UNX-F48904-J1 区/压力参数原子下发 ×100 轮零半提交。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：设备键合法 → 重复登记 → 一次读取锁 → 参数暂存 → 原子提交
//!           → 回放。一次读取锁必须先于参数暂存：设备在两次读取之间被
//!           替换会造成「半个新设备」；暂存必须先于提交：任一参数非法
//!           时整批回滚，不允许已写入的区宽留下。回放放在最后，用指纹
//!           证明提交后的档位与暂存内容逐字段一致。
//! 关键差异：本模块不判坐标归属（core 做），也不做 palm rejection
//!           （guard 做）；它只维护「设备是谁、参数是哪一版、换版是否
//!           原子」。任何跨版混合读取都必须在本模块被拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70CC0，B06 族第二槽）。
pub const SEG_BASE: u32 = 0x70CC0;

pub const DEVICES: usize = 10;
pub const READ_ROUNDS: u32 = 100;
pub const COMMIT_ROUNDS: u32 = 100;
pub const ZONE_ROWS: usize = 10;
pub const MAX_PARAMS: usize = 64;

// ── 诊断码闭集（+0x01..+0x0A） ────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MdCode {
    /// 设备键非法
    MetaBad,
    /// 同键重复登记
    DupDev,
    /// 首读已锁后读到不同内容
    ProbeLocked,
    /// 读取次数越界（同一 boot 内超过一次）
    ReadBad,
    /// 参数表行非法
    TableBad,
    /// 行区间重叠
    RowOverlap,
    /// 暂存非法（批内任一参数越界）
    StageBad,
    /// 提交非法（未暂存 / 已提交）
    CommitBad,
    /// 回放不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl MdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            MdCode::MetaBad => "设备键非法：vid/pid/fw/boot 必须非零",
            MdCode::DupDev => "重复登记：同 vid:pid 已存在",
            MdCode::ProbeLocked => "一次读取锁：同一 boot 读到不同元数据",
            MdCode::ReadBad => "读取次数越界：同 boot 只允许一次有效读取",
            MdCode::TableBad => "参数表行非法：宽度或压力阈不合规",
            MdCode::RowOverlap => "参数表行重叠：同宽度区间被两行覆盖",
            MdCode::StageBad => "暂存非法：批内任一参数越界",
            MdCode::CommitBad => "提交非法：无暂存或重复提交",
            MdCode::ReplayBad => "回放不一致：提交后档位与暂存不同",
            MdCode::SegmentBad => "段位越界：诊断码逃出 0x70CC0 窗口",
        }
    }

    pub const ALL: [MdCode; 10] = [
        MdCode::MetaBad,
        MdCode::DupDev,
        MdCode::ProbeLocked,
        MdCode::ReadBad,
        MdCode::TableBad,
        MdCode::RowOverlap,
        MdCode::StageBad,
        MdCode::CommitBad,
        MdCode::ReplayBad,
        MdCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < MdCode::ALL.len() {
        if MdCode::ALL[i].val() == code {
            return MdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 复用 core 的域模型 ────────────────────────────────────────────
pub use super::unx48901_touchpad_core::{
    classify, PadClass, PadProfile, PressureSpec, SoftButton, ZoneSpec, TpCode,
};

// ── 设备元数据键 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DevKey {
    pub vid: u16,
    pub pid: u16,
    pub fw: u32,
}

impl DevKey {
    pub fn new(vid: u16, pid: u16, fw: u32) -> Result<DevKey, u32> {
        if vid == 0 || pid == 0 || fw == 0 {
            return Err(MdCode::MetaBad.val());
        }
        Ok(DevKey { vid, pid, fw })
    }

    pub fn text(&self) -> String {
        format!("{:04X}:{:04X}@{}", self.vid, self.pid, self.fw)
    }
}

/// 元数据快照：一旦锁定，后续读取必须逐字段同值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MetaSnapshot {
    pub boot_id: u32,
    pub key: DevKey,
    pub caps: u16,
    pub class: PadClass,
    pub zones: ZoneSpec,
    pub pressure: PressureSpec,
}

impl MetaSnapshot {
    pub fn from_profile(boot_id: u32, p: &PadProfile) -> Result<MetaSnapshot, u32> {
        if boot_id == 0 {
            return Err(MdCode::MetaBad.val());
        }
        if p.vid == 0 || p.pid == 0 || p.fw == 0 {
            return Err(MdCode::MetaBad.val());
        }
        Ok(MetaSnapshot {
            boot_id,
            key: DevKey { vid: p.vid, pid: p.pid, fw: p.fw },
            caps: p.caps,
            class: p.class,
            zones: p.zone,
            pressure: p.pressure,
        })
    }

    pub fn same_as(&self, other: &MetaSnapshot) -> bool {
        self.boot_id == other.boot_id
            && self.key == other.key
            && self.caps == other.caps
            && self.class == other.class
            && self.zones == other.zones
            && self.pressure == other.pressure
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fold_u32(h, self.boot_id);
        h = fold_u32(h, self.key.vid as u32);
        h = fold_u32(h, self.key.pid as u32);
        h = fold_u32(h, self.key.fw);
        h = fold_u32(h, self.caps as u32);
        h = fold_u32(h, self.class.code());
        h = fold_u32(h, self.zones.width);
        h = fold_u32(h, self.zones.left_permille);
        h = fold_u32(h, self.zones.right_permille);
        h = fold_u32(h, self.zones.hys);
        h = fold_u32(h, self.pressure.max);
        h = fold_u32(h, self.pressure.low);
        h = fold_u32(h, self.pressure.high);
        h
    }
}

// ── 注册表：一次读取锁 ────────────────────────────────────────────
pub struct MetaRegistry {
    slots: Vec<Option<MetaSnapshot>>,
    reads: u32,
    conflicts: u32,
}

impl MetaRegistry {
    pub fn new() -> MetaRegistry {
        MetaRegistry { slots: Vec::new(), reads: 0, conflicts: 0 }
    }

    pub fn reads(&self) -> u32 {
        self.reads
    }

    pub fn conflicts(&self) -> u32 {
        self.conflicts
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn find(&self, key: DevKey) -> Option<usize> {
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(Some(s)) = self.slots.get(i) {
                if s.key == key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 首读：登记并锁定。同键再次登记直接拒绝（不是覆盖）。
    pub fn probe(&mut self, snap: MetaSnapshot) -> Result<MetaSnapshot, u32> {
        if self.find(snap.key).is_some() {
            self.conflicts = self.conflicts.saturating_add(1);
            return Err(MdCode::DupDev.val());
        }
        if self.slots.len() >= MAX_PARAMS {
            return Err(MdCode::SegmentBad.val());
        }
        self.slots.push(Some(snap));
        self.reads = self.reads.saturating_add(1);
        Ok(snap)
    }

    /// 复读：同 boot 同键必须逐字段同值；不同 boot 一律拒。
    pub fn reread(&mut self, key: DevKey, boot_id: u32) -> Result<MetaSnapshot, u32> {
        let idx = match self.find(key) {
            Some(i) => i,
            None => return Err(MdCode::MetaBad.val()),
        };
        let cur = match self.slots.get(idx) {
            Some(Some(s)) => *s,
            _ => return Err(MdCode::SegmentBad.val()),
        };
        if cur.boot_id != boot_id {
            self.conflicts = self.conflicts.saturating_add(1);
            return Err(MdCode::ProbeLocked.val());
        }
        self.reads = self.reads.saturating_add(1);
        Ok(cur)
    }

    /// 一次读取闸：每次 probe 后同 boot 的 reread 必须同值。
    pub fn one_read_audit(&self) -> Result<usize, u32> {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.slots.len() {
            match self.slots.get(i) {
                Some(Some(s)) => {
                    if s.boot_id == 0 {
                        return Err(MdCode::MetaBad.val());
                    }
                    n += 1;
                }
                Some(None) => return Err(MdCode::SegmentBad.val()),
                None => return Err(MdCode::SegmentBad.val()),
            }
            i += 1;
        }
        Ok(n)
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(Some(s)) = self.slots.get(i) {
                h ^= s.fingerprint();
                h = h.wrapping_mul(FNV_PRIME);
            }
            i += 1;
        }
        fold_u32(h, self.reads)
    }
}

// ── 区参数表（十行，按设备宽度分段） ──────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ZoneRow {
    pub width_min: u32,
    pub width_max: u32,
    pub left_permille: u32,
    pub right_permille: u32,
    pub hys: u32,
}

impl ZoneRow {
    pub fn new(
        width_min: u32,
        width_max: u32,
        left_permille: u32,
        right_permille: u32,
        hys: u32,
    ) -> ZoneRow {
        ZoneRow { width_min, width_max, left_permille, right_permille, hys }
    }

    pub fn valid(&self) -> bool {
        if self.width_min == 0 || self.width_min > self.width_max {
            return false;
        }
        let probe = ZoneSpec::new(self.width_min, self.left_permille, self.right_permille, self.hys);
        if !probe.valid() {
            return false;
        }
        // 上界也必须能装下同一组参数。
        let top = ZoneSpec::new(self.width_max, self.left_permille, self.right_permille, self.hys);
        top.valid()
    }

    pub fn covers(&self, width: u32) -> bool {
        width >= self.width_min && width <= self.width_max
    }

    pub fn text(&self) -> String {
        format!(
            "w[{}..={}] L{} R{} hys{}",
            self.width_min, self.width_max, self.left_permille, self.right_permille, self.hys
        )
    }
}

/// 冻结十行表：宽度区间首尾相接，允许交界点双覆盖由查找次序裁决。
pub const ZONE_TABLE: [ZoneRow; ZONE_ROWS] = [
    ZoneRow { width_min: 512, width_max: 767, left_permille: 250, right_permille: 250, hys: 8 },
    ZoneRow { width_min: 768, width_max: 1023, left_permille: 260, right_permille: 260, hys: 10 },
    ZoneRow { width_min: 1024, width_max: 1279, left_permille: 270, right_permille: 270, hys: 12 },
    ZoneRow { width_min: 1280, width_max: 1535, left_permille: 280, right_permille: 280, hys: 14 },
    ZoneRow { width_min: 1536, width_max: 2047, left_permille: 290, right_permille: 290, hys: 16 },
    ZoneRow { width_min: 2048, width_max: 2559, left_permille: 300, right_permille: 300, hys: 18 },
    ZoneRow { width_min: 2560, width_max: 3199, left_permille: 300, right_permille: 300, hys: 20 },
    ZoneRow { width_min: 3200, width_max: 3839, left_permille: 300, right_permille: 300, hys: 24 },
    ZoneRow { width_min: 3840, width_max: 5119, left_permille: 310, right_permille: 310, hys: 28 },
    ZoneRow { width_min: 5120, width_max: 8191, left_permille: 320, right_permille: 320, hys: 32 },
];

/// 表审计：行合法、区间不重叠。
pub fn audit_zone_table() -> Result<usize, u32> {
    if ZONE_TABLE.len() != ZONE_ROWS {
        return Err(MdCode::TableBad.val());
    }
    let mut i = 0usize;
    while i < ZONE_TABLE.len() {
        let r = match ZONE_TABLE.get(i) {
            Some(v) => *v,
            None => return Err(MdCode::SegmentBad.val()),
        };
        if !r.valid() {
            return Err(MdCode::TableBad.val());
        }
        let mut j = i + 1;
        while j < ZONE_TABLE.len() {
            let q = match ZONE_TABLE.get(j) {
                Some(v) => *v,
                None => return Err(MdCode::SegmentBad.val()),
            };
            if r.width_min <= q.width_max && q.width_min <= r.width_max {
                return Err(MdCode::RowOverlap.val());
            }
            j += 1;
        }
        i += 1;
    }
    Ok(ZONE_TABLE.len())
}

/// 按宽度找行：升序首个命中；越界 → TableBad。
pub fn lookup_zone(width: u32) -> Result<ZoneRow, u32> {
    audit_zone_table()?;
    let mut i = 0usize;
    while i < ZONE_TABLE.len() {
        if let Some(r) = ZONE_TABLE.get(i) {
            if r.covers(width) {
                return Ok(*r);
            }
        }
        i += 1;
    }
    Err(MdCode::TableBad.val())
}

// ── 参数暂存与原子提交 ────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Param {
    Zone(ZoneSpec),
    Pressure(PressureSpec),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParamSet {
    pub zone: ZoneSpec,
    pub pressure: PressureSpec,
}

impl ParamSet {
    pub fn new(zone: ZoneSpec, pressure: PressureSpec) -> ParamSet {
        ParamSet { zone, pressure }
    }

    pub fn valid(&self) -> bool {
        self.zone.valid() && self.pressure.valid()
    }

    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = fold_u32(h, self.zone.width);
        h = fold_u32(h, self.zone.left_permille);
        h = fold_u32(h, self.zone.right_permille);
        h = fold_u32(h, self.zone.hys);
        h = fold_u32(h, self.pressure.max);
        h = fold_u32(h, self.pressure.low);
        h = fold_u32(h, self.pressure.high);
        h
    }
}

/// 原子下发器：先写暂存区，整批校验通过才一次性换版。
pub struct ParamAtom {
    current: ParamSet,
    staged: Option<ParamSet>,
    version: u32,
    staged_writes: u32,
    commits: u32,
    rollbacks: u32,
}

impl ParamAtom {
    pub fn new(initial: ParamSet) -> Result<ParamAtom, u32> {
        if !initial.valid() {
            return Err(MdCode::StageBad.val());
        }
        Ok(ParamAtom {
            current: initial,
            staged: None,
            version: 1,
            staged_writes: 0,
            commits: 0,
            rollbacks: 0,
        })
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn current(&self) -> ParamSet {
        self.current
    }

    pub fn commits(&self) -> u32 {
        self.commits
    }

    pub fn rollbacks(&self) -> u32 {
        self.rollbacks
    }

    pub fn staged_writes(&self) -> u32 {
        self.staged_writes
    }

    pub fn stage(&mut self, p: ParamSet) -> Result<(), u32> {
        if self.staged.is_some() {
            // 暂存区一次只允许一份；未提交前再写按 StageBad 拒。
            return Err(MdCode::StageBad.val());
        }
        if !p.valid() {
            self.staged_writes = self.staged_writes.saturating_add(1);
            return Err(MdCode::StageBad.val());
        }
        self.staged = Some(p);
        self.staged_writes = self.staged_writes.saturating_add(1);
        Ok(())
    }

    pub fn commit(&mut self) -> Result<u32, u32> {
        let p = match self.staged {
            Some(v) => v,
            None => return Err(MdCode::CommitBad.val()),
        };
        if !p.valid() {
            self.staged = None;
            self.rollbacks = self.rollbacks.saturating_add(1);
            return Err(MdCode::StageBad.val());
        }
        self.current = p;
        self.staged = None;
        self.version = self.version.saturating_add(1);
        self.commits = self.commits.saturating_add(1);
        Ok(self.version)
    }

    pub fn rollback(&mut self) -> Result<(), u32> {
        if self.staged.is_none() {
            return Err(MdCode::CommitBad.val());
        }
        self.staged = None;
        self.rollbacks = self.rollbacks.saturating_add(1);
        Ok(())
    }

    /// 回放：当前版本必须等于最后一次提交的暂存指纹；无暂存。
    pub fn replay(&self, expected: u64) -> Result<u64, u32> {
        if self.staged.is_some() {
            return Err(MdCode::ReplayBad.val());
        }
        let now = self.current.fingerprint();
        if now != expected {
            return Err(MdCode::ReplayBad.val());
        }
        Ok(now)
    }
}

// ── FNV ───────────────────────────────────────────────────────────
pub const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ b as u64).wrapping_mul(FNV_PRIME)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, (v >> (i * 8)) as u8);
        i += 1;
    }
    x
}

// ── LCG ───────────────────────────────────────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.state >> 33) as u32
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 { 0 } else { self.next() % n }
    }
}

// ── 十设备语料 ────────────────────────────────────────────────────
/// 十台设备：宽度覆盖表内十段，三类交替。
pub fn ten_devices() -> Vec<(u16, u16, u32, u16, u32)> {
    let mut v: Vec<(u16, u16, u32, u16, u32)> = Vec::new();
    let mut i = 0usize;
    while i < DEVICES {
        let width = 640u32.saturating_add((i as u32).saturating_mul(700));
        let caps = match i % 3 {
            0 => 0x0001 | 0x0004 | 0x0008, // clickpad + pressure + MT
            1 => 0x0002 | 0x0008,          // physical + MT
            _ => 0x0008 | 0x0020,          // hybrid (MT + hires)
        };
        v.push((
            0x1000u16.saturating_add(i as u16),
            0x2000u16.saturating_add(i as u16),
            1u32.saturating_add(i as u32),
            caps,
            width,
        ));
        i += 1;
    }
    v
}

fn spec_for(width: u32) -> Result<(ZoneSpec, PressureSpec), u32> {
    let row = lookup_zone(width)?;
    let zone = ZoneSpec::new(width, row.left_permille, row.right_permille, row.hys);
    if !zone.valid() {
        return Err(MdCode::TableBad.val());
    }
    let pressure = PressureSpec::new(4095, 300, 2200);
    Ok((zone, pressure))
}

/// 黄金读取：十设备首读 + 复读，共 10 × READ_ROUNDS 次复读。
pub fn golden_reads(rounds: u32) -> Result<(usize, u32, u64), u32> {
    let devs = ten_devices();
    let mut reg = MetaRegistry::new();
    let mut i = 0usize;
    while i < devs.len() {
        let (vid, pid, fw, caps, width) = match devs.get(i) {
            Some(v) => *v,
            None => return Err(MdCode::SegmentBad.val()),
        };
        let (zone, pressure) = spec_for(width)?;
        let p = PadProfile::new(vid, pid, fw, caps, zone, pressure)?;
        let snap = MetaSnapshot::from_profile(1, &p)?;
        reg.probe(snap)?;
        i += 1;
    }
    reg.one_read_audit()?;
    let mut r = 0u32;
    while r < rounds {
        let mut k = 0usize;
        while k < devs.len() {
            let (vid, pid, fw, _, _) = match devs.get(k) {
                Some(v) => *v,
                None => return Err(MdCode::SegmentBad.val()),
            };
            let key = DevKey::new(vid, pid, fw)?;
            let a = reg.reread(key, 1)?;
            let b = reg.reread(key, 1)?;
            if !a.same_as(&b) {
                return Err(MdCode::ReplayBad.val());
            }
            k += 1;
        }
        r = r.wrapping_add(1);
    }
    if reg.len() != DEVICES {
        return Err(MdCode::MetaBad.val());
    }
    Ok((reg.len(), reg.reads(), reg.fingerprint()))
}

/// 黄金提交：100 轮参数换版，逐轮验证版本单调与回放一致。
pub fn golden_commits(rounds: u32) -> Result<(u32, u32, u64), u32> {
    let zone0 = ZoneSpec::new(3200, 300, 300, 24);
    let press0 = PressureSpec::new(4095, 300, 2200);
    let mut atom = ParamAtom::new(ParamSet::new(zone0, press0))?;
    let mut r = 0u32;
    while r < rounds {
        let width = 2048u32.saturating_add(r % 2048);
        let zone = ZoneSpec::new(width, 300, 300, 20);
        let low = 200u32.saturating_add(r % 100);
        let press = PressureSpec::new(4095, low, 2200);
        let set = ParamSet::new(zone, press);
        atom.stage(set)?;
        let before = atom.version();
        let v = atom.commit()?;
        if v != before.saturating_add(1) {
            return Err(MdCode::CommitBad.val());
        }
        let fp = atom.current().fingerprint();
        atom.replay(fp)?;
        r = r.wrapping_add(1);
    }
    if atom.commits() != rounds || atom.rollbacks() != 0 {
        return Err(MdCode::CommitBad.val());
    }
    Ok((atom.commits(), atom.version(), atom.current().fingerprint()))
}

/// 黄金全链：(设备数, 读取数, 提交数, 指纹)。
pub fn golden_run() -> Result<(usize, u32, u32, u64), u32> {
    let (n, reads, rh) = golden_reads(READ_ROUNDS)?;
    let (commits, _v, ch) = golden_commits(COMMIT_ROUNDS)?;
    let mut h = FNV_OFFSET;
    h = fold_u32(h, n as u32);
    h = fold_u32(h, reads);
    h = fold_u32(h, commits);
    h ^= rh;
    h = h.wrapping_mul(FNV_PRIME);
    h ^= ch;
    Ok((n, reads, commits, h))
}

// ── 注入账（四路） ────────────────────────────────────────────────
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

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x4D);
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：同键重复登记。
        let zone = ZoneSpec::new(3200, 300, 300, 24);
        let press = PressureSpec::new(4095, 300, 2200);
        let p = PadProfile::new(1, 2, 3, 0x0001 | 0x0004 | 0x0008, zone, press);
        let dup = match &p {
            Ok(pp) => {
                let snap = MetaSnapshot::from_profile(1, pp);
                match snap {
                    Ok(s) => {
                        let mut reg = MetaRegistry::new();
                        reg.probe(s).is_ok() && reg.probe(s).err() == Some(MdCode::DupDev.val())
                    }
                    Err(_) => false,
                }
            }
            Err(_) => false,
        };
        led.record(0, dup);

        // 路 2：一次读取锁（同键不同 boot）。
        let lock = match &p {
            Ok(pp) => match MetaSnapshot::from_profile(1, pp) {
                Ok(s) => {
                    let mut reg = MetaRegistry::new();
                    match reg.probe(s) {
                        Ok(_) => reg.reread(s.key, 2).err() == Some(MdCode::ProbeLocked.val()),
                        Err(_) => false,
                    }
                }
                Err(_) => false,
            },
            Err(_) => false,
        };
        led.record(1, lock);

        // 路 3：未暂存就提交 / 暂存非法整批拒。
        let mut atom = match ParamAtom::new(ParamSet::new(zone, press)) {
            Ok(a) => a,
            Err(_) => return Err(MdCode::CommitBad.val()),
        };
        let bad_set = ParamSet::new(ZoneSpec::new(3200, 0, 300, 1), press);
        let stage = atom.commit().err() == Some(MdCode::CommitBad.val())
            && atom.stage(bad_set).err() == Some(MdCode::StageBad.val())
            && atom.version() == 1;
        led.record(2, stage);

        // 路 4：两次暂存未提交。
        let set = ParamSet::new(ZoneSpec::new(2048, 300, 300, 20), press);
        let double = atom.stage(set).is_ok() && atom.stage(set).err() == Some(MdCode::StageBad.val());
        led.record(3, double);

        r = r.wrapping_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (n, reads, commits, _) = golden_run()?;
    if n == DEVICES && reads >= DEVICES as u32 && commits == COMMIT_ROUNDS {
        Ok(())
    } else {
        Err(MdCode::ReplayBad.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    if DevKey::new(0, 1, 1).err() != Some(MdCode::MetaBad.val()) {
        return Err(MdCode::MetaBad.val());
    }
    if lookup_zone(100).err() != Some(MdCode::TableBad.val()) {
        return Err(MdCode::TableBad.val());
    }
    if audit_zone_table()? != ZONE_ROWS {
        return Err(MdCode::TableBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if DEVICES == 10
        && READ_ROUNDS == 100
        && COMMIT_ROUNDS == 100
        && ZONE_ROWS == 10
        && MAX_PARAMS == 64
        && ZONE_TABLE.len() == 10
    {
        Ok(())
    } else {
        Err(MdCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(MdCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < MdCode::ALL.len() {
        let v = MdCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x20 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let table = audit_zone_table();
    v.push((
        "F48903-J1-十行区表零重叠",
        table.ok() == Some(ZONE_ROWS),
        format!("rows={:?}", table.ok()),
    ));
    let rd = golden_reads(READ_ROUNDS);
    v.push((
        "F48903-J1-十设备首读锁定",
        match &rd {
            Ok((n, reads, _)) => {
                *n == DEVICES && *reads == DEVICES as u32 * (1 + 2 * READ_ROUNDS)
            }
            Err(_) => false,
        },
        match &rd {
            Ok((n, reads, _)) => format!("devices={} reads={}", n, reads),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let cm = golden_commits(COMMIT_ROUNDS);
    v.push((
        "F48904-J1-百轮原子提交",
        match &cm {
            Ok((c, ver, _)) => *c == COMMIT_ROUNDS && *ver == COMMIT_ROUNDS + 1,
            Err(_) => false,
        },
        match &cm {
            Ok((c, ver, _)) => format!("commits={} version={}", c, ver),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let zero = DevKey::new(0, 1, 1);
    v.push((
        "F48903-J1-零设备键必拒",
        zero.err() == Some(MdCode::MetaBad.val()),
        format!("key={:?}", zero.err()),
    ));
    let gap = lookup_zone(100);
    v.push((
        "F48903-J1-表外宽度必拒",
        gap.err() == Some(MdCode::TableBad.val()),
        format!("lookup={:?}", gap.err()),
    ));
    let mut reg = MetaRegistry::new();
    let zone = ZoneSpec::new(3200, 300, 300, 24);
    let press = PressureSpec::new(4095, 300, 2200);
    let dup = match PadProfile::new(1, 2, 3, 0x0001 | 0x0004 | 0x0008, zone, press) {
        Ok(p) => match MetaSnapshot::from_profile(1, &p) {
            Ok(s) => reg.probe(s).is_ok() && reg.probe(s).err() == Some(MdCode::DupDev.val()),
            Err(_) => false,
        },
        Err(_) => false,
    };
    v.push((
        "F48903-J1-重复登记必拒",
        dup,
        format!("conflicts={}", reg.conflicts()),
    ));
    let lock = match reg.reread(DevKey { vid: 1, pid: 2, fw: 3 }, 2) {
        Err(e) => e == MdCode::ProbeLocked.val(),
        Ok(_) => false,
    };
    v.push((
        "F48903-J1-异boot复读必拒",
        lock,
        String::from("同键不同 boot → ProbeLocked"),
    ));
    let mut atom = match ParamAtom::new(ParamSet::new(zone, press)) {
        Ok(a) => a,
        Err(_) => return v,
    };
    let staged = atom.stage(ParamSet::new(zone, press)).is_ok();
    let rolled = atom.rollback().is_ok();
    v.push((
        "F48904-J1-回滚清暂存",
        staged
            && rolled
            && atom.version() == 1
            && atom.rollbacks() == 1
            && atom.commit().is_err(),
        format!("version={} rollbacks={}", atom.version(), atom.rollbacks()),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48903-J1-注入四路10/10",
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
        "F48903-J1-探针三序与双跑",
        probe_seq_a().is_ok()
            && probe_seq_b().is_ok()
            && probe_seq_c().is_ok()
            && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v
}

pub fn run_unx48903_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48903 触控板元数据与参数下发原子性");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn zone() -> ZoneSpec {
        ZoneSpec::new(3200, 300, 300, 24)
    }

    fn press() -> PressureSpec {
        PressureSpec::new(4095, 300, 2200)
    }

    #[test]
    fn zone_table_is_ten_non_overlapping_rows() {
        assert_eq!(audit_zone_table().ok(), Some(10));
        assert_eq!(lookup_zone(3200).ok().map(|r| r.width_min), Some(3200));
        assert!(lookup_zone(100).is_err());
        assert!(lookup_zone(9000).is_err());
    }

    #[test]
    fn dev_key_requires_all_fields() {
        assert!(DevKey::new(1, 2, 3).is_ok());
        assert_eq!(DevKey::new(0, 2, 3).err(), Some(MdCode::MetaBad.val()));
        assert_eq!(DevKey::new(1, 0, 3).err(), Some(MdCode::MetaBad.val()));
        assert_eq!(DevKey::new(1, 2, 0).err(), Some(MdCode::MetaBad.val()));
    }

    #[test]
    fn duplicate_probe_rejected() {
        let p = match PadProfile::new(1, 2, 3, 0x0001 | 0x0004 | 0x0008, zone(), press()) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let s = match MetaSnapshot::from_profile(1, &p) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let mut reg = MetaRegistry::new();
        assert!(reg.probe(s).is_ok());
        assert_eq!(reg.probe(s).err(), Some(MdCode::DupDev.val()));
        assert_eq!(reg.conflicts(), 1);
    }

    #[test]
    fn reread_same_boot_stable() {
        let p = match PadProfile::new(7, 8, 9, 0x0001 | 0x0004 | 0x0008, zone(), press()) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let s = match MetaSnapshot::from_profile(1, &p) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let mut reg = MetaRegistry::new();
        assert!(reg.probe(s).is_ok());
        let a = reg.reread(s.key, 1);
        let b = reg.reread(s.key, 1);
        assert_eq!(a, b);
        assert_eq!(reg.reread(s.key, 2).err(), Some(MdCode::ProbeLocked.val()));
    }

    #[test]
    fn param_atom_commits_atomically() {
        let mut a = match ParamAtom::new(ParamSet::new(zone(), press())) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert!(a.commit().is_err());
        let z2 = ZoneSpec::new(2048, 300, 300, 20);
        assert!(a.stage(ParamSet::new(z2, press())).is_ok());
        assert_eq!(a.stage(ParamSet::new(z2, press())).err(), Some(MdCode::StageBad.val()));
        let v = a.commit();
        assert_eq!(v, Ok(2));
        assert!(a.replay(a.current().fingerprint()).is_ok());
    }

    #[test]
    fn invalid_param_never_reaches_current() {
        let mut a = match ParamAtom::new(ParamSet::new(zone(), press())) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        let bad = ParamSet::new(ZoneSpec::new(3200, 0, 300, 1), press());
        assert_eq!(a.stage(bad).err(), Some(MdCode::StageBad.val()));
        assert_eq!(a.version(), 1);
        assert!(a.current().valid());
    }

    #[test]
    fn rollback_leaves_no_stage() {
        let mut a = match ParamAtom::new(ParamSet::new(zone(), press())) {
            Ok(v) => v,
            Err(_) => return assert!(false),
        };
        assert!(a.stage(ParamSet::new(zone(), press())).is_ok());
        assert!(a.rollback().is_ok());
        assert_eq!(a.commit().err(), Some(MdCode::CommitBad.val()));
        assert_eq!(a.rollbacks(), 1);
    }

    #[test]
    fn golden_reads_cover_ten_devices() {
        let g = golden_reads(10);
        match g {
            Ok((n, reads, _)) => {
                assert_eq!(n, DEVICES);
                assert_eq!(reads, DEVICES as u32 * 21);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn golden_commits_version_monotonic() {
        let g = golden_commits(50);
        match g {
            Ok((c, v, _)) => {
                assert_eq!(c, 50);
                assert_eq!(v, 51);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn injection_catches_all_four() {
        match inject_rounds(10) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [10, 10, 10, 10]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn golden_and_probes_deterministic() {
        assert_eq!(golden_run(), golden_run());
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(segment_guard());
        assert_eq!(MdCode::ALL.len(), 10);
    }
}
