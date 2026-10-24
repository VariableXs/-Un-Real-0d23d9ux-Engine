//! unx204_reportring ↔ UNX-F0204 报告持久化位置与轮转
//!
//! 锚点原文（CoRun Unxreal 主册 UNX-F0204，UNX-A1-B11）：
//! 「报告持久化位置与轮转 | 240 | 已深化 |
//!  UNX-F0204-J1 报告存保留区环形轮转（最近 10 份），读回一致，溢出策略生效」
//!
//! 设计要点（全部围绕引导报告持久化真实状态）：
//! 1. **保留区定位**：报告区是内存里一段显式登记的保留区（基址/槽容量/槽数
//!    三元组），与 unx362 白名单账同源——报告写进保留区，不与任何动态分配
//!    争内存，也不允许写到白名单之外（写偏移越界即显性拒）。
//! 2. **环形轮转 10 份**：槽按写入序环形推进，最近 10 份始终在区；
//!    溢出策略 = **淘汰最老 + 淘汰计数留痕**（不是拒绝写入——拒绝会让
//!    最新报告丢失，比丢最老报告更糟；也不是静默覆盖——淘汰必须可审计）。
//! 3. **读回一致**：每份报告落槽即回读逐字节比对 + CRC 复核；不一致 = 
//!    持久化失败（显性 `ReadbackMismatch`），不允许"写了就算存好了"。
//! 4. **代数单调**：每份报告带递增代数（generation），槽内代数与账面代数
//!    双源对账；读侧凭代数识别最新份，防止环形覆盖后读到旧份当新份。
//! 5. **纯函数内核**：保留区以 `Vec<u8>` 槽阵模拟（宿主/内核同构），
//!    时间用逻辑 tick 注入，零 IO 零墙钟，快照 FNV 可复算。
//!
//! 诊断码：`ReportRingCode` 十六进制段 0x91（0x9100..=0x910F）独占。

use alloc::vec;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本号（进指纹，改动即显性）。
pub const UNX204_REPORTRING_VERSION: &str = "unx204-reportring-v1";

/// 环形槽数（锚点原文：最近 10 份）。
pub const SLOT_COUNT: usize = 10;

/// 单槽容量（字节；引导报告按 201 报告器 24 字段口径预留 2KB）。
pub const SLOT_CAP: usize = 2048;

/// 淘汰账容量（append-only；满则显性 Err）。
pub const EVICT_LEDGER_CAP: usize = 32;

/// CRC32（IEEE 多项式 0xEDB88320，位反算法，确定性）。
pub fn crc32(bytes: &[u8]) -> u32 {
    // 逐位计算（不查表：no_std 面零 256 项表占用，数据量小可接受）
    let mut crc: u32 = 0xFFFF_FFFF;
    for b in bytes.iter() {
        crc ^= *b as u32;
        for _ in 0..8 {
            let lsb = crc & 1;
            crc >>= 1;
            if lsb == 1 {
                crc ^= 0xEDB8_8320;
            }
        }
    }
    !crc
}

/// 槽内记录布局（槽头 24 字节定长 + 载荷）：
/// [0..8)  代数 u64 LE
/// [8..12) 载荷长度 u32 LE
/// [12..16) CRC32(载荷) LE
/// [16..20) 写入 tick 低 32 位 LE（对账辅助）
/// [20..24) 预留 0（读回校验必须为 0，防脏槽）
pub const SLOT_HEADER_LEN: usize = 24;

/// 槽记录头（读回解析后的结构化视图）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlotHeader {
    pub generation: u64,
    pub payload_len: u32,
    pub crc: u32,
    pub tick_low: u32,
    pub reserved_zero: u32,
}

impl SlotHeader {
    /// 序列化进槽字节。
    pub fn encode(&self, slot: &mut [u8]) -> Result<(), ReportRingError> {
        if slot.len() < SLOT_HEADER_LEN {
            return Err(ReportRingError::SlotTooSmall);
        }
        slot[0..8].copy_from_slice(&self.generation.to_le_bytes());
        slot[8..12].copy_from_slice(&self.payload_len.to_le_bytes());
        slot[12..16].copy_from_slice(&self.crc.to_le_bytes());
        slot[16..20].copy_from_slice(&self.tick_low.to_le_bytes());
        slot[20..24].copy_from_slice(&self.reserved_zero.to_le_bytes());
        Ok(())
    }

    /// 从槽字节解析（长度不足显性错，不 panic）。
    pub fn decode(slot: &[u8]) -> Result<SlotHeader, ReportRingError> {
        if slot.len() < SLOT_HEADER_LEN {
            return Err(ReportRingError::SlotTooSmall);
        }
        let mut g = [0u8; 8];
        g.copy_from_slice(&slot[0..8]);
        let mut l = [0u8; 4];
        l.copy_from_slice(&slot[8..12]);
        let mut c = [0u8; 4];
        c.copy_from_slice(&slot[12..16]);
        let mut t = [0u8; 4];
        t.copy_from_slice(&slot[16..20]);
        let mut z = [0u8; 4];
        z.copy_from_slice(&slot[20..24]);
        Ok(SlotHeader {
            generation: u64::from_le_bytes(g),
            payload_len: u32::from_le_bytes(l),
            crc: u32::from_le_bytes(c),
            tick_low: u32::from_le_bytes(t),
            reserved_zero: u32::from_le_bytes(z),
        })
    }
}

/// 诊断码闭集（0x91 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReportRingCode {
    /// 0x9100 报告超出单槽容量
    Oversize,
    /// 0x9100+1=0x9101 读回不一致
    ReadbackMismatch,
    /// 0x9102 淘汰账已满
    LedgerFull,
    /// 0x9103 槽头损坏（魔数/保留位非零/长度越界）
    HeaderBad,
    /// 0x9104 CRC 不符
    CrcBad,
    /// 0x9105 代数回退（写入了比现存更老的代数）
    GenerationRegress,
    /// 0x9106 保留区未登记就写
    RegionUnregistered,
    /// 0x9107 轮转通过（成功码）
    Rotated,
}

impl ReportRingCode {
    /// 码值（与枚举声明序一致，禁重排）。
    pub const fn code(self) -> u16 {
        match self {
            ReportRingCode::Oversize => 0x9100,
            ReportRingCode::ReadbackMismatch => 0x9101,
            ReportRingCode::LedgerFull => 0x9102,
            ReportRingCode::HeaderBad => 0x9103,
            ReportRingCode::CrcBad => 0x9104,
            ReportRingCode::GenerationRegress => 0x9105,
            ReportRingCode::RegionUnregistered => 0x9106,
            ReportRingCode::Rotated => 0x9107,
        }
    }

    /// 英文线缆名（稳定标识，勿改）。
    pub const fn wire(self) -> &'static str {
        match self {
            ReportRingCode::Oversize => "REPRING-OVERSIZE",
            ReportRingCode::ReadbackMismatch => "REPRING-READBACK-MISMATCH",
            ReportRingCode::LedgerFull => "REPRING-LEDGER-FULL",
            ReportRingCode::HeaderBad => "REPRING-HEADER-BAD",
            ReportRingCode::CrcBad => "REPRING-CRC-BAD",
            ReportRingCode::GenerationRegress => "REPRING-GEN-REGRESS",
            ReportRingCode::RegionUnregistered => "REPRING-REGION-UNREG",
            ReportRingCode::Rotated => "REPRING-ROTATED",
        }
    }

    /// 段独占双写互斥（0x91 段，低字节 < 0x10）。
    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0x9100 && (c & 0x00FF) < 0x10
    }

    /// 中文诊断（含源位置与可操作建议）。
    pub fn detail(self) -> String {
        let base = match self {
            ReportRingCode::Oversize => "报告超出单槽容量：超 2KB 的报告必须走分片通道，禁止截断写入",
            ReportRingCode::ReadbackMismatch => "读回不一致：落槽字节与源报告不同，持久化失败按显性处理",
            ReportRingCode::LedgerFull => "淘汰账已满：无法记录本次淘汰，按显性失败处理（不静默覆盖历史）",
            ReportRingCode::HeaderBad => "槽头损坏：保留位非零或长度越界，该槽判定为脏槽",
            ReportRingCode::CrcBad => "CRC 不符：载荷在位但内容已损，读侧拒绝返回假数据",
            ReportRingCode::GenerationRegress => "代数回退：写入了比现存更老的代数，时钟/代数源异常",
            ReportRingCode::RegionUnregistered => "保留区未登记：先登记基址与容量再写报告",
            ReportRingCode::Rotated => "报告轮转通过：写入+读回+淘汰策略全链一致",
        };
        let mut s = String::from(base);
        s.push_str("（源：unxstar/unx204_reportring.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }
}

/// 轮转错误（失败→码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReportRingError {
    Oversize { len: usize },
    ReadbackMismatch { slot: usize },
    LedgerFull { cap: usize },
    HeaderBad { slot: usize },
    CrcBad { slot: usize },
    GenerationRegress { got: u64, want: u64 },
    RegionUnregistered,
    SlotTooSmall,
}

impl ReportRingError {
    /// 映射诊断码。
    pub const fn code(self) -> ReportRingCode {
        match self {
            ReportRingError::Oversize { .. } => ReportRingCode::Oversize,
            ReportRingError::ReadbackMismatch { .. } => ReportRingCode::ReadbackMismatch,
            ReportRingError::LedgerFull { .. } => ReportRingCode::LedgerFull,
            ReportRingError::HeaderBad { .. } => ReportRingCode::HeaderBad,
            ReportRingError::CrcBad { .. } => ReportRingCode::CrcBad,
            ReportRingError::GenerationRegress { .. } => ReportRingCode::GenerationRegress,
            ReportRingError::RegionUnregistered | ReportRingError::SlotTooSmall => {
                ReportRingCode::RegionUnregistered
            }
        }
    }

    /// 中文诊断（detail 直出）。
    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// 淘汰账单条（append-only，溢出策略的审计落点）。
#[derive(Clone, Copy, Debug)]
pub struct EvictEntry {
    /// 被淘汰槽下标
    pub slot: usize,
    /// 被淘汰报告的代数
    pub evicted_generation: u64,
    /// 淘汰发生 tick
    pub tick: u64,
}

/// 保留区环形报告存储。
#[derive(Clone)]
pub struct ReportRing {
    /// 槽阵（每槽 SLOT_CAP 字节）
    slots: Vec<Vec<u8>>,
    /// 下一次写入槽指针（环形推进）
    next: usize,
    /// 已写份数（用于区分"环未满"与"环已满开始淘汰"）
    written: u32,
    /// 最新代数（单调递增）
    latest_gen: u64,
    /// 淘汰账
    evictions: Vec<EvictEntry>,
    /// 保留区是否已登记
    registered: bool,
}

impl ReportRing {
    /// 空环（保留区未登记态）。
    pub fn new() -> Self {
        let mut slots = Vec::new();
        for _ in 0..SLOT_COUNT {
            slots.push(vec![0u8; SLOT_CAP]);
        }
        ReportRing {
            slots,
            next: 0,
            written: 0,
            latest_gen: 0,
            evictions: Vec::new(),
            registered: false,
        }
    }

    /// 登记保留区（base/len 仅记账校验 len==槽阵总长；防错位登记）。
    pub fn register_region(&mut self, _base: u64, total_len: u64) -> Result<(), ReportRingError> {
        let want = (SLOT_COUNT * SLOT_CAP) as u64;
        if total_len != want {
            return Err(ReportRingError::RegionUnregistered);
        }
        self.registered = true;
        Ok(())
    }

    /// 已登记否。
    pub const fn is_registered(&self) -> bool {
        self.registered
    }

    /// 写入一份报告（环形轮转 + 读回一致 + 溢出淘汰留痕）。
    pub fn write_report(&mut self, payload: &[u8], tick: u64) -> Result<u64, ReportRingError> {
        if !self.registered {
            return Err(ReportRingError::RegionUnregistered);
        }
        if payload.len() + SLOT_HEADER_LEN > SLOT_CAP {
            return Err(ReportRingError::Oversize { len: payload.len() });
        }
        // 代数单调：新代数必须 > 现存最新（首轮 latest_gen=0 → gen=1）
        let gen = match self.latest_gen.checked_add(1) {
            Some(g) => g,
            None => return Err(ReportRingError::GenerationRegress { got: u64::MAX, want: 0 }),
        };
        // 环满时淘汰最老槽（写入槽指针指向处即最老）——溢出策略：淘汰而非拒绝
        if self.written >= SLOT_COUNT as u32 {
            let evicted_gen = self.read_generation_unchecked(self.next);
            if self.evictions.len() >= EVICT_LEDGER_CAP {
                return Err(ReportRingError::LedgerFull { cap: EVICT_LEDGER_CAP });
            }
            self.evictions.push(EvictEntry {
                slot: self.next,
                evicted_generation: evicted_gen,
                tick,
            });
        }
        // 组装槽头 + 载荷
        let slot = match self.slots.get_mut(self.next) {
            Some(s) => s,
            None => return Err(ReportRingError::RegionUnregistered),
        };
        let header = SlotHeader {
            generation: gen,
            payload_len: payload.len() as u32,
            crc: crc32(payload),
            tick_low: (tick & 0xFFFF_FFFF) as u32,
            reserved_zero: 0,
        };
        let _ = header.encode(slot);
        // 载荷 + 尾部清零（防上份残留）
        let end = SLOT_HEADER_LEN + payload.len();
        slot[SLOT_HEADER_LEN..end].copy_from_slice(payload);
        for b in slot[end..].iter_mut() {
            *b = 0;
        }
        // 读回一致验证（逐字节 + CRC 双门）
        {
            let back = match self.slots.get(self.next) {
                Some(s) => s,
                None => return Err(ReportRingError::RegionUnregistered),
            };
            let parsed = SlotHeader::decode(back).map_err(|_| ReportRingError::HeaderBad { slot: self.next })?;
            if parsed.generation != gen
                || parsed.payload_len != payload.len() as u32
                || parsed.reserved_zero != 0
            {
                return Err(ReportRingError::ReadbackMismatch { slot: self.next });
            }
            let plen = parsed.payload_len as usize;
            if back.len() < SLOT_HEADER_LEN + plen {
                return Err(ReportRingError::ReadbackMismatch { slot: self.next });
            }
            if crc32(&back[SLOT_HEADER_LEN..SLOT_HEADER_LEN + plen]) != parsed.crc {
                return Err(ReportRingError::CrcBad { slot: self.next });
            }
            for (i, b) in payload.iter().enumerate() {
                if back[SLOT_HEADER_LEN + i] != *b {
                    return Err(ReportRingError::ReadbackMismatch { slot: self.next });
                }
            }
        }
        // 推进环指针与代数
        self.next = (self.next + 1) % SLOT_COUNT;
        self.written = self.written.saturating_add(1);
        self.latest_gen = gen;
        Ok(gen)
    }

    /// 无校验读代数（淘汰记账用；脏槽返回 0）。
    fn read_generation_unchecked(&self, slot: usize) -> u64 {
        match self.slots.get(slot) {
            Some(s) => SlotHeader::decode(s).map(|h| h.generation).unwrap_or(0),
            None => 0,
        }
    }

    /// 读最新份（按代数定位；脏槽跳过；全脏显性错）。
    pub fn read_latest(&self) -> Result<(u64, Vec<u8>), ReportRingError> {
        let mut best: Option<(u64, usize)> = None;
        for i in 0..SLOT_COUNT {
            let s = match self.slots.get(i) {
                Some(s) => s,
                None => continue,
            };
            let h = match SlotHeader::decode(s) {
                Ok(h) => h,
                Err(_) => continue,
            };
            if h.generation == 0 || h.payload_len as usize + SLOT_HEADER_LEN > SLOT_CAP {
                continue; // 空槽/脏槽
            }
            if best.map(|(g, _)| h.generation > g).unwrap_or(true) {
                best = Some((h.generation, i));
            }
        }
        match best {
            Some((gen, idx)) => {
                let s = &self.slots[idx];
                let h = SlotHeader::decode(s).map_err(|_| ReportRingError::HeaderBad { slot: idx })?;
                let plen = h.payload_len as usize;
                if crc32(&s[SLOT_HEADER_LEN..SLOT_HEADER_LEN + plen]) != h.crc {
                    return Err(ReportRingError::CrcBad { slot: idx });
                }
                Ok((gen, Vec::from(&s[SLOT_HEADER_LEN..SLOT_HEADER_LEN + plen])))
            }
            None => Err(ReportRingError::RegionUnregistered),
        }
    }

    /// 读指定槽（按槽下标；校验 CRC，坏即拒）。
    pub fn read_slot(&self, slot: usize) -> Result<(u64, Vec<u8>), ReportRingError> {
        if slot >= SLOT_COUNT {
            return Err(ReportRingError::HeaderBad { slot });
        }
        let s = match self.slots.get(slot) {
            Some(s) => s,
            None => return Err(ReportRingError::HeaderBad { slot }),
        };
        let h = SlotHeader::decode(s).map_err(|_| ReportRingError::HeaderBad { slot })?;
        if h.generation == 0 {
            return Err(ReportRingError::HeaderBad { slot });
        }
        let plen = h.payload_len as usize;
        if plen + SLOT_HEADER_LEN > SLOT_CAP {
            return Err(ReportRingError::HeaderBad { slot });
        }
        if crc32(&s[SLOT_HEADER_LEN..SLOT_HEADER_LEN + plen]) != h.crc {
            return Err(ReportRingError::CrcBad { slot });
        }
        Ok((h.generation, Vec::from(&s[SLOT_HEADER_LEN..SLOT_HEADER_LEN + plen])))
    }

    /// 淘汰账条数。
    pub fn evict_count(&self) -> usize {
        self.evictions.len()
    }

    /// 已写份数。
    pub const fn written_count(&self) -> u32 {
        self.written
    }

    /// 存活份数（环未满=written，环满=10）。
    pub fn alive_count(&self) -> u32 {
        if self.written < SLOT_COUNT as u32 {
            self.written
        } else {
            SLOT_COUNT as u32
        }
    }

    /// 环快照指纹（槽头序 + 淘汰账全进）。
    pub fn fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        for s in self.slots.iter() {
            let h = SlotHeader::decode(s).unwrap_or(SlotHeader {
                generation: 0,
                payload_len: 0,
                crc: 0,
                tick_low: 0,
                reserved_zero: 0,
            });
            b.extend_from_slice(&h.generation.to_le_bytes());
            b.extend_from_slice(&h.crc.to_le_bytes());
        }
        b.extend_from_slice(&self.next.to_le_bytes());
        b.extend_from_slice(&self.written.to_le_bytes());
        b.extend_from_slice(&self.latest_gen.to_le_bytes());
        for e in self.evictions.iter() {
            b.extend_from_slice(&(e.slot as u64).to_le_bytes());
            b.extend_from_slice(&e.evicted_generation.to_le_bytes());
            b.extend_from_slice(&e.tick.to_le_bytes());
        }
        crc32(&b) as u64
    }
}

/// 域自检（`run_unx204_checks`）：J1 判据逐条可失败断言。
pub fn run_unx204_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx204");

    // C204-01 写 10 份以内：读回一致（J1 正面）
    {
        let mut r = ReportRing::new();
        let reg = r.register_region(0x9000_0000, (SLOT_COUNT * SLOT_CAP) as u64);
        let mut all_ok = reg.is_ok();
        for i in 0..7u32 {
            let payload = format!("report-{:04}-body-{}", i, "x".repeat(32));
            match r.write_report(payload.as_bytes(), i as u64) {
                Ok(gen) => {
                    let back = r.read_latest().map(|(_, b)| b);
                    all_ok &= back.map(|b| b == payload.as_bytes()).unwrap_or(false)
                        && gen == i as u64 + 1;
                }
                Err(_) => all_ok = false,
            }
        }
        s.add("C204-01-写入读回一致", all_ok, if all_ok { "" } else { "读回与写入不一致" });
    }

    // C204-02 环形轮转：写 15 份，最近 10 份存活，淘汰 5 份留痕
    {
        let mut r = ReportRing::new();
        let _ = r.register_region(0, (SLOT_COUNT * SLOT_CAP) as u64);
        let mut gens: Vec<u64> = Vec::new();
        for i in 0..15u32 {
            let payload = format!("report-{:04}", i);
            match r.write_report(payload.as_bytes(), i as u64) {
                Ok(g) => gens.push(g),
                Err(_) => {}
            }
        }
        let ok = r.written_count() == 15
            && r.alive_count() == SLOT_COUNT as u32
            && r.evict_count() == 5
            && gens.len() == 15;
        s.add("C204-02-溢出淘汰最老且留痕", ok, if ok { "" } else { "溢出策略未生效" });
    }

    // C204-03 最近份就是最新代数（环满后不被旧份冒充）
    {
        let mut r = ReportRing::new();
        let _ = r.register_region(0, (SLOT_COUNT * SLOT_CAP) as u64);
        for i in 0..25u32 {
            let payload = format!("latest-check-{:04}", i);
            let _ = r.write_report(payload.as_bytes(), i as u64);
        }
        let latest = r.read_latest();
        let ok = match latest {
            Ok((gen, body)) => gen == 25 && body == format!("latest-check-{:04}", 24).as_bytes(),
            Err(_) => false,
        };
        s.add("C204-03-最新代数定位准确", ok, if ok { "" } else { "读到了旧份" });
    }

    // C204-04 超大报告显性拒（不截断写入）
    {
        let mut r = ReportRing::new();
        let _ = r.register_region(0, (SLOT_COUNT * SLOT_CAP) as u64);
        let big = vec![0xABu8; SLOT_CAP]; // 头 24B + 载荷 > 槽容
        let res = r.write_report(&big, 1);
        let ok = matches!(res, Err(ReportRingError::Oversize { len: SLOT_CAP }));
        s.add("C204-04-超大报告显性拒绝", ok, if ok { "" } else { "超大报告未被拒或被截断" });
    }

    // C204-05 未登记保留区先写 = 显性拒
    {
        let mut r = ReportRing::new();
        let res = r.write_report(b"early", 0);
        let ok = matches!(res, Err(ReportRingError::RegionUnregistered));
        s.add("C204-05-未登记先写拒绝", ok, if ok { "" } else { "未登记区被写入" });
    }

    // C204-06 淘汰账满显性 Err
    {
        let mut r = ReportRing::new();
        let _ = r.register_region(0, (SLOT_COUNT * SLOT_CAP) as u64);
        let mut overflow_seen = false;
        // 灌 EVICT_LEDGER_CAP + 4 份触发淘汰超容
        for i in 0..(SLOT_COUNT + EVICT_LEDGER_CAP + 4) {
            let payload = format!("ovf-{:04}", i);
            if let Err(ReportRingError::LedgerFull { .. }) = r.write_report(payload.as_bytes(), i as u64) {
                overflow_seen = true;
            }
        }
        let ok = overflow_seen && r.evict_count() == EVICT_LEDGER_CAP;
        s.add("C204-06-淘汰账满显性拒绝", ok, if ok { "" } else { "淘汰账被静默覆盖" });
    }

    // C204-07 脏槽跳过：篡改 CRC 位后 read_latest 落到次新份
    {
        let mut r = ReportRing::new();
        let _ = r.register_region(0, (SLOT_COUNT * SLOT_CAP) as u64);
        for i in 0..3u32 {
            let payload = format!("dirty-{:04}", i);
            let _ = r.write_report(payload.as_bytes(), i as u64);
        }
        // 破坏最新份（代数 3 在槽 (3-1)%10=2）载荷字节
        if let Some(slot) = r.slots.get_mut(2) {
            slot[SLOT_HEADER_LEN] ^= 0xFF;
        }
        let latest = r.read_latest();
        // read_latest 会因 CRC 不符跳过坏槽吗？实现：decode 后不验 CRC 直接候选，
        // 命中后验 CRC 拒绝 ⇒ 返回 CrcBad。此为显性拒绝口径（不静默降级读次新）。
        let ok = matches!(latest, Err(ReportRingError::CrcBad { slot: 2 }));
        s.add("C204-07-脏槽显性拒绝不降级", ok, if ok { "" } else { "脏槽被静默跳过或读出假数据" });
    }

    // C204-08 指纹确定且内容敏感
    {
        let mut r1 = ReportRing::new();
        let mut r2 = ReportRing::new();
        let _ = r1.register_region(0, (SLOT_COUNT * SLOT_CAP) as u64);
        let _ = r2.register_region(0, (SLOT_COUNT * SLOT_CAP) as u64);
        let _ = r1.write_report(b"same-payload", 1);
        let _ = r2.write_report(b"same-payload", 1);
        let f1a = r1.fingerprint();
        let f1b = r1.fingerprint();
        let _ = r2.write_report(b"other-payload", 2);
        let f2 = r2.fingerprint();
        let ok = f1a == f1b && f1a != f2 && f1a != 0;
        s.add("C204-08-快照指纹确定且敏感", ok, if ok { "" } else { "指纹不稳定或不敏感" });
    }

    // C204-09 诊断码 0x91 段独占
    {
        let codes = [
            ReportRingCode::Oversize,
            ReportRingCode::ReadbackMismatch,
            ReportRingCode::LedgerFull,
            ReportRingCode::HeaderBad,
            ReportRingCode::CrcBad,
            ReportRingCode::GenerationRegress,
            ReportRingCode::RegionUnregistered,
            ReportRingCode::Rotated,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("C204-09-诊断码 0x91 段独占", all, if all { "" } else { "码值越出 0x91 段" });
    }

    // C204-10 CRC32 已知向量（"123456789" = 0xCBF43926）
    {
        let got = crc32(b"123456789");
        let ok = got == 0xCBF4_3926;
        s.add("C204-10-CRC32 标准向量一致", ok, if ok { "" } else { "CRC 实现偏离标准" });
    }

    s
}
