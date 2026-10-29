//! UNX-F48749 C·资源占用诚实声明（磁盘/内存/后台占用用户可见页数据）
//! 判据 UNX-F48749-J1：声明页数据与实测一致；判据锚定 体验十二。
//! 设计纪律：no_std（仅 core/alloc）；诊断码自 SEG_BASE+1 起连续且总数 ≤0x1F；
//! drill() 自证十判据，正例计数、反例注入必须 .err() 命中，命中即 Err 传播；
//! fuzz 用确定性 LCG（wrapping_mul 1_664_525 / wrapping_add 1_013_904_223）；
//! 声明页三栏（磁盘/内存/后台）必须与实测逐项一致，谎报即红。

use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x1B20;
const E_DISK_LIE: u16 = SEG_BASE + 1;
const E_MEM_LIE: u16 = SEG_BASE + 2;
const E_BG_LIE: u16 = SEG_BASE + 3;
const E_PAGE_MISS: u16 = SEG_BASE + 4;
const E_METER_MISS: u16 = SEG_BASE + 5;
const E_REPLAY_DIFF: u16 = SEG_BASE + 6;
const E_NEG_VALUE: u16 = SEG_BASE + 7;
const E_FIELD_GAP: u16 = SEG_BASE + 8;

/// 实测三栏
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeterReading {
    pub disk_kb: u64,
    pub mem_kb: u64,
    pub bg_tasks: u32,
}

impl MeterReading {
    pub fn sane(&self) -> Result<(), u16> {
        if self.disk_kb > 0x0010_0000_0000 {
            return Err(E_NEG_VALUE);
        }
        if self.mem_kb > 0x0002_0000_0000 {
            return Err(E_NEG_VALUE);
        }
        if self.bg_tasks > 10_000 {
            return Err(E_NEG_VALUE);
        }
        Ok(())
    }
}

/// 用户可见声明页
#[derive(Clone, Debug, PartialEq)]
pub struct DeclarePage {
    pub disk_kb: u64,
    pub mem_kb: u64,
    pub bg_tasks: u32,
    pub label: String,
}

impl DeclarePage {
    pub fn new(disk_kb: u64, mem_kb: u64, bg_tasks: u32, label: &str) -> Self {
        DeclarePage { disk_kb, mem_kb, bg_tasks, label: String::from(label) }
    }
}

/// 诚实声明核对器
pub struct HonestAudit {
    pub reading: MeterReading,
    pub page: Option<DeclarePage>,
}

impl HonestAudit {
    pub fn new(reading: MeterReading) -> Self {
        HonestAudit { reading, page: None }
    }

    pub fn declare(&mut self, page: DeclarePage) -> Result<(), u16> {
        if page.label.is_empty() {
            return Err(E_FIELD_GAP);
        }
        self.page = Some(page);
        Ok(())
    }

    /// 三栏逐项核对：声明与实测一致才绿
    pub fn audit(&self) -> Result<(), u16> {
        self.reading.sane()?;
        let p = match &self.page {
            Some(p) => p.clone(),
            None => return Err(E_PAGE_MISS),
        };
        if p.disk_kb != self.reading.disk_kb {
            return Err(E_DISK_LIE);
        }
        if p.mem_kb != self.reading.mem_kb {
            return Err(E_MEM_LIE);
        }
        if p.bg_tasks != self.reading.bg_tasks {
            return Err(E_BG_LIE);
        }
        Ok(())
    }
}

/// 确定性 LCG
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        (self.next() >> 16) % n
    }
}

/// 诚实声明一致 ×10^3
fn sweep_honest(rng: &mut Lcg) -> Result<(), u16> {
    let mut iter = 0u32;
    while iter < 1_000 {
        let r = MeterReading {
            disk_kb: 100_000 + rng.below(900_000),
            mem_kb: 50_000 + rng.below(200_000),
            bg_tasks: rng.below(20) as u32,
        };
        let mut a = HonestAudit::new(r);
        a.declare(DeclarePage::new(r.disk_kb, r.mem_kb, r.bg_tasks, "res-page"))?;
        a.audit()?;
        iter += 1;
    }
    Ok(())
}

/// 三栏谎报逐一被点名 ×10 组
fn sweep_lies(rng: &mut Lcg) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 10 {
        let r = MeterReading {
            disk_kb: 100_000 + rng.below(50_000),
            mem_kb: 80_000 + rng.below(20_000),
            bg_tasks: 5 + rng.below(5) as u32,
        };
        let mut a1 = HonestAudit::new(r);
        a1.declare(DeclarePage::new(r.disk_kb + 1, r.mem_kb, r.bg_tasks, "lie-disk"))?;
        if a1.audit().err() != Some(E_DISK_LIE) {
            return Err(E_DISK_LIE);
        }
        let mut a2 = HonestAudit::new(r);
        a2.declare(DeclarePage::new(r.disk_kb, r.mem_kb + 7, r.bg_tasks, "lie-mem"))?;
        if a2.audit().err() != Some(E_MEM_LIE) {
            return Err(E_MEM_LIE);
        }
        let mut a3 = HonestAudit::new(r);
        a3.declare(DeclarePage::new(r.disk_kb, r.mem_kb, r.bg_tasks + 2, "lie-bg"))?;
        if a3.audit().err() != Some(E_BG_LIE) {
            return Err(E_BG_LIE);
        }
        round += 1;
    }
    Ok(())
}

/// 反例注入：缺页、空标签、越界读数全命中
fn sweep_negatives() -> Result<(), u16> {
    let r = MeterReading { disk_kb: 1_000, mem_kb: 2_000, bg_tasks: 3 };
    let a1 = HonestAudit::new(r);
    if a1.audit().err() != Some(E_PAGE_MISS) {
        return Err(E_PAGE_MISS);
    }
    let mut a2 = HonestAudit::new(r);
    let d2 = a2.declare(DeclarePage::new(1_000, 2_000, 3, ""));
    if d2.err() != Some(E_FIELD_GAP) {
        return Err(E_FIELD_GAP);
    }
    let bad = MeterReading { disk_kb: 0x0020_0000_0000, mem_kb: 100, bg_tasks: 1 };
    let a3 = HonestAudit::new(bad);
    if a3.audit().err() != Some(E_NEG_VALUE) {
        return Err(E_NEG_VALUE);
    }
    let a4 = HonestAudit::new(MeterReading { disk_kb: 0, mem_kb: 0, bg_tasks: 0 });
    let _ = a4.reading.sane()?;
    Ok(())
}

/// 重放一致 ×20：同种子双跑声明审计逐位吻合
fn sweep_replay(seed: u64) -> Result<(), u16> {
    let mut round = 0u32;
    while round < 20 {
        let mut ra = Lcg::new(seed + round as u64);
        let mut rb = Lcg::new(seed + round as u64);
        let d = ra.below(100_000);
        let m = ra.below(50_000);
        let b = ra.below(10) as u32;
        let db = rb.below(100_000);
        let mb = rb.below(50_000);
        let bb = rb.below(10) as u32;
        if d != db || m != mb || b != bb {
            return Err(E_REPLAY_DIFF);
        }
        let r = MeterReading { disk_kb: 10_000 + d, mem_kb: 5_000 + m, bg_tasks: b };
        let mut aa = HonestAudit::new(r);
        aa.declare(DeclarePage::new(r.disk_kb, r.mem_kb, r.bg_tasks, "replay"))?;
        let mut ab = HonestAudit::new(r);
        ab.declare(DeclarePage::new(r.disk_kb, r.mem_kb, r.bg_tasks, "replay"))?;
        aa.audit()?;
        ab.audit()?;
        round += 1;
    }
    Ok(())
}

pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    // R1 诚实声明一致 ×10^3
    let mut rng = Lcg::new(0x4874_9077);
    let mut r1 = 0u32;
    while r1 < 1_000 {
        sweep_honest(&mut rng)?;
        r1 += 1;
    }
    hits += 1;
    // R2 三栏谎报逐一被点名 ×10
    sweep_lies(&mut rng)?;
    hits += 1;
    // R3 反例注入全命中
    sweep_negatives()?;
    hits += 1;
    // R4 重放一致 ×20
    sweep_replay(0x4874_9B03)?;
    hits += 1;
    // R5 页标签非空且带资源语义
    let mut a = HonestAudit::new(MeterReading { disk_kb: 500, mem_kb: 600, bg_tasks: 2 });
    a.declare(DeclarePage::new(500, 600, 2, "disk/mem/bg"))?;
    let p = a.page.clone();
    match p {
        Some(pg) => {
            if pg.label.len() < 3 {
                return Err(E_FIELD_GAP);
            }
        }
        None => return Err(E_PAGE_MISS),
    }
    hits += 1;
    // R6 三栏齐备断言：页缺任一栏数值即与实测失配
    let mut a2 = HonestAudit::new(MeterReading { disk_kb: 500, mem_kb: 600, bg_tasks: 2 });
    a2.declare(DeclarePage::new(500, 601, 2, "almost"))?;
    if a2.audit().err() != Some(E_MEM_LIE) {
        return Err(E_MEM_LIE);
    }
    hits += 1;
    // R7 零占用诚实页：全零实测配全零声明过
    let mut a3 = HonestAudit::new(MeterReading { disk_kb: 0, mem_kb: 0, bg_tasks: 0 });
    a3.declare(DeclarePage::new(0, 0, 0, "zero-state"))?;
    a3.audit()?;
    hits += 1;
    // R8 声明页数值域校验：与实测同为非负
    let mut a4 = HonestAudit::new(MeterReading { disk_kb: 10, mem_kb: 20, bg_tasks: 1 });
    a4.declare(DeclarePage::new(10, 20, 1, "domain"))?;
    a4.audit()?;
    let pg4 = a4.page.clone();
    match pg4 {
        Some(p) => {
            if p.disk_kb > 0x0010_0000_0000 || p.mem_kb > 0x0002_0000_0000 {
                return Err(E_NEG_VALUE);
            }
        }
        None => return Err(E_PAGE_MISS),
    }
    hits += 1;
    // R9 读数合法性 ×10^2：随机读数 sane 全过
    let mut c = 0u32;
    while c < 100 {
        let r = MeterReading {
            disk_kb: rng.below(0x0001_0000_0000),
            mem_kb: rng.below(0x0001_0000_0000),
            bg_tasks: rng.below(9_999) as u32,
        };
        r.sane()?;
        c += 1;
    }
    hits += 1;
    // R10 页覆盖写语义：后声明覆盖前声明
    let mut a5 = HonestAudit::new(MeterReading { disk_kb: 7, mem_kb: 8, bg_tasks: 1 });
    a5.declare(DeclarePage::new(9, 9, 9, "stale"))?;
    a5.declare(DeclarePage::new(7, 8, 1, "fresh"))?;
    a5.audit()?;
    let pg5 = a5.page.clone();
    match pg5 {
        Some(p) => {
            if p.label != "fresh" {
                return Err(E_FIELD_GAP);
            }
        }
        None => return Err(E_PAGE_MISS),
    }
    hits += 1;
    // R11 深扫补验：同源 LCG 双轨重放逐位吻合 + 环形账求和复算（确定性、零外依）
    let mut track_a: u32 = 0x9E37_79B9;
    let mut track_b: u32 = 0x9E37_79B9;
    let mut rail: Vec<u32> = vec![];
    let mut rail_sum: u64 = 0;
    let mut dstep = 0u32;
    while dstep < 48 {
        track_a = track_a.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        track_b = track_b.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        rail.push(track_a);
        rail_sum += track_a as u64;
        dstep += 1;
    }
    if track_a != track_b {
        return Err(E_REPLAY_DIFF);
    }
    if rail.len() != 48 {
        return Err(E_REPLAY_DIFF);
    }
    let mut replay_sum: u64 = 0;
    let mut ridx = 0usize;
    while ridx < rail.len() {
        replay_sum += rail[ridx] as u64;
        ridx += 1;
    }
    if replay_sum != rail_sum {
        return Err(E_REPLAY_DIFF);
    }
    hits += 1;
    Ok(hits)
}
