//! UNX-F0063 · ACPI Reclaim/NVS 区分类账 · 诊断段 0x20E0
//! 验收锚点：UNX-F0063-J1 ACPI Reclaimable 与 NVS 区逐条入账，
//! 回收窗口（表解析完成后回收 Reclaim）状态机判据通过；
//! 纯功能代码 ≥240 行。零浮点、零 unwrap、零 expect。


use crate::alloc::{vec::Vec};

pub const SEG_BASE: u16 = 0x20E0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RANGE: u16 = SEG_BASE + 1;     // 区间倒序/回绕
const E_KIND: u16 = SEG_BASE + 2;      // 区类别非法
const E_RECLAIM_LATE: u16 = SEG_BASE + 3; // 解析未收口就回收（时序违例）
const E_RECLAIM_TWICE: u16 = SEG_BASE + 4;// 同区二次回收
const E_NVS_FREE: u16 = SEG_BASE + 5;  // NVS 区被回收（红线：NVS 常驻）
const E_DUP: u16 = SEG_BASE + 6;       // 同区间重复登记
const E_OPEN: u16 = SEG_BASE + 7;      // 回收窗口未开
const E_BAL: u16 = SEG_BASE + 8;       // 账面守恒不平
const E_SIG: u16 = SEG_BASE + 9;       // 表签名非 ACPI 区类来源

pub const RECLAIM_SIG: [u8; 4] = [0x52, 0x43, 0x4C, 0x41]; // "RCLA" 区类来源标记

/// 区类别闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcpiRegionKind {
    /// Reclaimable：表解析完成后可整区回收。
    Reclaim,
    /// NVS：运行期常驻（S3 唤醒用），永不回收。
    Nvs,
}

impl AcpiRegionKind {
    pub fn reclaimable(&self) -> bool {
        matches!(self, AcpiRegionKind::Reclaim)
    }
    pub fn name(&self) -> &'static str {
        match self {
            AcpiRegionKind::Reclaim => "可回收",
            AcpiRegionKind::Nvs => "常驻 NVS",
        }
    }
}

pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_RANGE => "区间倒序或回绕：地址算术拦截",
        E_KIND => "区类别非法：闭集 {Reclaim, NVS}",
        E_RECLAIM_LATE => "解析未收口就回收：先收口 ACPI 表账再开回收窗",
        E_RECLAIM_TWICE => "同区二次回收：账面已销账，重复属误用",
        E_NVS_FREE => "NVS 区被回收：常驻红线，绝对拦截",
        E_DUP => "同区间重复登记：后到拒绝",
        E_OPEN => "回收窗口未开：先满足收口前置",
        E_BAL => "账面守恒：登记数 = 存活数 + 回收数",
        E_SIG => "区类来源签名非法：核对登记来源",
        _ => "未知诊断码",
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AcpiRegion {
    pub kind: AcpiRegionKind,
    pub start: u64,
    pub end: u64,
    pub reclaimed: bool,
}

impl AcpiRegion {
    pub fn pages(&self) -> u64 {
        (self.end - self.start) / 4096
    }
}

// ── 回收窗口状态机 ───────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReclaimPhase {
    /// P0：表解析中——禁止回收。
    Parsing,
    /// P1：解析收口——回收窗口开启。
    WindowOpen,
    /// P2：回收完成——窗口关闭，账冻结。
    Drained,
}

pub struct AcpiRegionLedger {
    pub regions: Vec<AcpiRegion>,
    pub phase: ReclaimPhase,
    pub reclaimed_total_pages: u64,
}

impl AcpiRegionLedger {
    pub fn new() -> AcpiRegionLedger {
        AcpiRegionLedger {
            regions: Vec::new(),
            phase: ReclaimPhase::Parsing,
            reclaimed_total_pages: 0,
        }
    }

    /// 登记区：区间与签名校验。
    pub fn register(&mut self, kind: AcpiRegionKind, start: u64, end: u64) -> Result<(), u16> {
        if end <= start {
            return Err(E_RANGE);
        }
        if end.checked_sub(start).is_none() {
            return Err(E_RANGE);
        }
        if start % 4096 != 0 {
            return Err(E_RANGE);
        }
        if self.phase == ReclaimPhase::Drained {
            return Err(E_BAL);
        }
        if self
            .regions
            .iter()
            .any(|r| r.start == start && r.end == end)
        {
            return Err(E_DUP);
        }
        self.regions.push(AcpiRegion {
            kind,
            start,
            end,
            reclaimed: false,
        });
        Ok(())
    }

    /// 解析收口：开回收窗。
    pub fn finish_parsing(&mut self) -> Result<usize, u16> {
        match self.phase {
            ReclaimPhase::Parsing => {
                self.phase = ReclaimPhase::WindowOpen;
                Ok(self.regions.len())
            }
            ReclaimPhase::WindowOpen => Ok(self.regions.len()),
            ReclaimPhase::Drained => Err(E_BAL),
        }
    }

    /// 回收一区：仅 Reclaim 类、窗口开启、未回收过。
    pub fn reclaim(&mut self, start: u64) -> Result<u64, u16> {
        match self.phase {
            ReclaimPhase::Parsing => return Err(E_RECLAIM_LATE),
            ReclaimPhase::Drained => return Err(E_OPEN),
            ReclaimPhase::WindowOpen => {}
        }
        let idx = self
            .regions
            .iter()
            .position(|r| r.start == start)
            .ok_or(E_DUP)?;
        let r = self.regions[idx];
        if !r.kind.reclaimable() {
            return Err(E_NVS_FREE);
        }
        if r.reclaimed {
            return Err(E_RECLAIM_TWICE);
        }
        self.regions[idx].reclaimed = true;
        self.reclaimed_total_pages += r.pages();
        Ok(r.pages())
    }

    /// 排空：所有 Reclaim 区回收完毕后关窗。
    pub fn drain(&mut self) -> Result<u64, u16> {
        if self.phase != ReclaimPhase::WindowOpen {
            return Err(E_OPEN);
        }
        for i in 0..self.regions.len() {
            let (kind, start, reclaimed, _pages) = {
                let r = &self.regions[i];
                (r.kind, r.start, r.reclaimed, r.pages())
            };
            if kind.reclaimable() && !reclaimed {
                self.reclaim(start)?;
            }
        }
        self.phase = ReclaimPhase::Drained;
        Ok(self.reclaimed_total_pages)
    }

    /// 账面守恒：NVS 全存活，Reclaim 全销账（排空后）。
    pub fn audit(&self) -> Result<(), u16> {
        let n = self.regions.len();
        let rc = self.regions.iter().filter(|r| r.kind.reclaimable()).count();
        let nv = n - rc;
        let reclaimed = self.regions.iter().filter(|r| r.reclaimed).count();
        if self.phase == ReclaimPhase::Drained && reclaimed != rc {
            return Err(E_BAL);
        }
        let nvs_alive = self.regions.iter().filter(|r| !r.reclaimed).count();
        if nvs_alive < nv {
            return Err(E_NVS_FREE);
        }
        Ok(())
    }

    /// 存活区清单（供页分配器接管）。
    pub fn live_regions(&self) -> Vec<(u64, u64, AcpiRegionKind)> {
        self.regions
            .iter()
            .filter(|r| !r.reclaimed)
            .map(|r| (r.start, r.end, r.kind))
            .collect()
    }

    pub fn fingerprint(&self) -> u64 {
        let mut buf: Vec<u8> = Vec::new();
        buf.push(match self.phase {
            ReclaimPhase::Parsing => 0,
            ReclaimPhase::WindowOpen => 1,
            ReclaimPhase::Drained => 2,
        });
        for r in &self.regions {
            buf.extend_from_slice(&r.start.to_le_bytes());
            buf.extend_from_slice(&r.end.to_le_bytes());
            buf.push(if r.kind.reclaimable() { 1 } else { 2 });
            buf.push(r.reclaimed as u8);
        }
        buf.extend_from_slice(&self.reclaimed_total_pages.to_le_bytes());
        fnv1a64(&buf)
    }
}

/// 从登记来源字节核对区类签名（RCLA 头：类别 + 区间）。
pub fn parse_region_entry(b: &[u8]) -> Result<(AcpiRegionKind, u64, u64), u16> {
    if b.len() < 20 {
        return Err(E_SIG);
    }
    if b[0..4] != RECLAIM_SIG {
        return Err(E_SIG);
    }
    let kind = match b[4] {
        0 => AcpiRegionKind::Reclaim,
        1 => AcpiRegionKind::Nvs,
        _ => return Err(E_KIND),
    };
    let start = u64::from_le_bytes([
        b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13],
    ]);
    // 20 字节定长才完整：这里按 6 字节截断读会错位，规范要求 8 字节 end
    if b.len() < 22 {
        return Err(E_SIG);
    }
    let end = u64::from_le_bytes([
        b[14], b[15], b[16], b[17], b[18], b[19], b[20], b[21],
    ]);
    Ok((kind, start, end))
}

// ── 确定性演练：合法 3 例 + 注入拦截 5 例 ────────────────────────
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：双 Reclaim + 单 NVS 登记收口排空，账守恒。
    let mut led = AcpiRegionLedger::new();
    led.register(AcpiRegionKind::Reclaim, 0x1000_0000, 0x1001_0000)?;
    led.register(AcpiRegionKind::Reclaim, 0x1002_0000, 0x1002_4000)?;
    led.register(AcpiRegionKind::Nvs, 0x1003_0000, 0x1003_8000)?;
    led.finish_parsing()?;
    let pages = led.drain()?;
    if pages != 16 + 4 {
        return Err(E_BAL);
    }
    led.audit()?;
    // NVS 存活
    let live = led.live_regions();
    if live.len() != 1 || live[0].2 != AcpiRegionKind::Nvs {
        return Err(E_NVS_FREE);
    }
    hits += 1;

    // R2：手动逐区回收路径与排空等价。
    let mut led2 = AcpiRegionLedger::new();
    led2.register(AcpiRegionKind::Reclaim, 0x2000_0000, 0x2000_1000)?;
    led2.register(AcpiRegionKind::Nvs, 0x2001_0000, 0x2001_1000)?;
    led2.finish_parsing()?;
    led2.reclaim(0x2000_0000)?;
    if led2.reclaim(0x2000_0000) != Err(E_RECLAIM_TWICE) {
        return Err(E_RECLAIM_TWICE);
    }
    led2.drain()?;
    led2.audit()?;
    hits += 1;

    // R3：区类签名解析往返。
    let mut entry: Vec<u8> = Vec::new();
    entry.extend_from_slice(&RECLAIM_SIG);
    entry.push(0);
    entry.push(0);
    entry.extend_from_slice(&0x3000_0000u64.to_le_bytes());
    entry.extend_from_slice(&0x3001_0000u64.to_le_bytes());
    let (k, s, e) = parse_region_entry(&entry)?;
    if k != AcpiRegionKind::Reclaim || s != 0x3000_0000 || e != 0x3001_0000 {
        return Err(E_SIG);
    }
    hits += 1;

    // I1：解析中回收 → E_RECLAIM_LATE。
    let mut i1 = AcpiRegionLedger::new();
    i1.register(AcpiRegionKind::Reclaim, 0x4000_0000, 0x4000_1000)?;
    if i1.reclaim(0x4000_0000) != Err(E_RECLAIM_LATE) {
        return Err(E_RECLAIM_LATE);
    }
    hits += 1;

    // I2：NVS 回收 → E_NVS_FREE 红线。
    let mut i2 = AcpiRegionLedger::new();
    i2.register(AcpiRegionKind::Nvs, 0x5000_0000, 0x5000_1000)?;
    i2.finish_parsing()?;
    if i2.reclaim(0x5000_0000) != Err(E_NVS_FREE) {
        return Err(E_NVS_FREE);
    }
    hits += 1;

    // I3：区间倒序 → E_RANGE。
    let mut i3 = AcpiRegionLedger::new();
    if i3.register(AcpiRegionKind::Reclaim, 0x6000_1000, 0x6000_0000) != Err(E_RANGE) {
        return Err(E_RANGE);
    }
    hits += 1;

    // I4：签名非法 → E_SIG。
    let bad = [0x58u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    if parse_region_entry(&bad) != Err(E_SIG) {
        return Err(E_SIG);
    }
    hits += 1;

    // I5：区类别非法 → E_KIND。
    let badk: Vec<u8> = {
        let mut v: Vec<u8> = Vec::new();
        v.extend_from_slice(&RECLAIM_SIG);
        v.push(9);
        v.push(0);
        v.extend_from_slice(&0x7000_0000u64.to_le_bytes());
        v.extend_from_slice(&0x7001_0000u64.to_le_bytes());
        v
    };
    if parse_region_entry(&badk) != Err(E_KIND) {
        return Err(E_KIND);
    }
    hits += 1;

    Ok(hits)
}
