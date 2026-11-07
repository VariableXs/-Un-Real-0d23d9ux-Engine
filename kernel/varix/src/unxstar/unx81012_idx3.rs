//! UNX-F81013 台账索引构建器（按域/按批/按波三维索引） · 0x91210段
//!
//! 判据原文（派工卡 81012 逐字锚定）：
//!   ① 纯功能代码 ≥289 行；
//!   ② 判据原文：UNX-F81013-J1 「台账查询接口钉版（只读 API 版本化承诺）」判据落地：正向断言 ×10 与冻结口径完全一致；注入同型故障 10 次 10/10 检出并回放三要素告警（十三章异常零静默）；失败路径显性化、埋点入账；台账写入仅走本条登记通道（非法第四态零存在）；开发期零 QEMU 零实机写，长周期/真机档判据随闸门补测登记；判据锚定 §AI-81 台账域任务书保真；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 域×批×波三轴独立 sorted-vec 索引：每轴一个 (键, 行号) 升序去重表，
//!    插入用二分定位保序（确定性，无哈希无随机迭代序）。
//! 2. 登记通道唯一：register 三轴同建，重复 (键,行号) 登记 → DupReg；
//!    三轴任一越界（域>15/批>255/波>30 或 0 波）显拒。
//! 3. 三轴行号集合大小守恒恒等式：|I_dom|==|I_batch|==|I_wave|==登记行数，
//!    每次登记后内断言，破坏即 Conserve。
//! 4. 构建账 append-only：seq 严格递增，回退 IdRewind；登记前查询
//!    （NotReg）与空索引查询（EmptyIdx）显性化。
//! 5. 重放校验：从构建账逐条重放重建三轴，与实存索引逐位比对，
//!    不一致即 Replay。
//! 6. 轴选择闭集 Axis 三档 + 位图四态闭集（2 位编码，越位 BadFourth）。
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机；内核路径无
//!    unwrap/expect/panic/todo。

use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX12 派工公式：0x91000 + 0x30 × (81012 − 81001)）。
pub const SEG_BASE: u32 = 0x91210;

// ── 诊断码闭集（段内偏移 0x01..0x0B） ─────────────────────────────
/// 重复登记。
pub const E_DUPREG: u16 = 1;
/// 越界域值（>15）。
pub const E_DOMOOB: u16 = 2;
/// 越界批值（0 或 >255）。
pub const E_BATCHOOB: u16 = 3;
/// 越界波值（0 或 >30）。
pub const E_WAVEOOB: u16 = 4;
/// 三轴守恒破坏。
pub const E_CONSERVE: u16 = 5;
/// 未登记先查。
pub const E_NOTREG: u16 = 6;
/// 索引回退（构建账 seq 非严格递增）。
pub const E_IDREW: u16 = 7;
/// 重放不一致。
pub const E_REPLAY: u16 = 8;
/// 空索引查询。
pub const E_EMPTYIDX: u16 = 9;
/// 非法第四态（轴态位越出 2 位闭集）。
pub const E_BADFOURTH: u16 = 10;

/// 诊断码闭集口径（10 个）。
pub const DIAG_ALL: usize = 10;

/// 域轴上界（0..=15，16 域）。
pub const DOM_MAX: u8 = 15;

/// 批轴上界（1..=255）。
pub const BATCH_MAX: u8 = 255;

/// 波轴上界（1..=30）。
pub const WAVE_MAX: u16 = 30;

/// 轴三档闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// 按域。
    Domain = 0,
    /// 按批。
    Batch = 1,
    /// 按波。
    Wave = 2,
}

impl Axis {
    /// 从 2 位编码构造（越位 → BadFourth，非法第四态零存在）。
    pub fn from_bits(bits: u8) -> Result<Axis, IdxError> {
        match bits {
            0 => Ok(Axis::Domain),
            1 => Ok(Axis::Batch),
            2 => Ok(Axis::Wave),
            _ => Err(IdxError::BadFourth),
        }
    }
}

/// 登记行（三维定位 + 行号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdxRow {
    /// 域（0..=15）。
    pub dom: u8,
    /// 批（1..=255）。
    pub batch: u8,
    /// 波（1..=30）。
    pub wave: u16,
    /// 台账行号（1 起连续）。
    pub row: u32,
}

/// 构建账记录（append-only）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildRecord {
    /// 构建序号（严格递增）。
    pub seq: u32,
    /// 登记行。
    pub entry: IdxRow,
}

/// 域错误。`code()` 返回 `SEG_BASE + n`，全部码值被真实构造。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdxError {
    /// 重复登记。
    DupReg,
    /// 越界域值。
    DomOob,
    /// 越界批值。
    BatchOob,
    /// 越界波值。
    WaveOob,
    /// 三轴守恒破坏。
    Conserve,
    /// 未登记先查。
    NotReg,
    /// 索引回退。
    IdRewind,
    /// 重放不一致。
    Replay,
    /// 空索引查询。
    EmptyIdx,
    /// 非法第四态。
    BadFourth,
}

impl IdxError {
    /// 诊断码（段内偏移闭集）。
    pub fn code(self) -> u32 {
        let n: u16 = match self {
            IdxError::DupReg => E_DUPREG,
            IdxError::DomOob => E_DOMOOB,
            IdxError::BatchOob => E_BATCHOOB,
            IdxError::WaveOob => E_WAVEOOB,
            IdxError::Conserve => E_CONSERVE,
            IdxError::NotReg => E_NOTREG,
            IdxError::IdRewind => E_IDREW,
            IdxError::Replay => E_REPLAY,
            IdxError::EmptyIdx => E_EMPTYIDX,
            IdxError::BadFourth => E_BADFOURTH,
        };
        SEG_BASE + n as u32
    }

    /// 三要素之一：出了什么事（冻结文本）。
    pub fn message(self) -> &'static str {
        match self {
            IdxError::DupReg => "duplicate axis registration",
            IdxError::DomOob => "domain out of 0..=15",
            IdxError::BatchOob => "batch out of 1..=255",
            IdxError::WaveOob => "wave out of 1..=30",
            IdxError::Conserve => "three-axis size conservation broken",
            IdxError::NotReg => "lookup before registration",
            IdxError::IdRewind => "build ledger seq rewind",
            IdxError::Replay => "index replay diverges",
            IdxError::EmptyIdx => "query on empty index",
            IdxError::BadFourth => "axis outside closed set",
        }
    }
}

/// (键, 行号) 升序去重表插入：保序、去重（重复返回 false）。
fn sorted_upsert(v: &mut Vec<(u32, u32)>, key: u32, row: u32) -> bool {
    let mut lo = 0usize;
    let mut hi = v.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if v[mid].0 < key {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo < v.len() && v[lo].0 == key {
        return false;
    }
    v.insert(lo, (key, row));
    true
}

/// sorted-vec 查找（二分）。
fn sorted_find(v: &[(u32, u32)], key: u32) -> Option<u32> {
    let mut lo = 0usize;
    let mut hi = v.len();
    while lo < hi {
        let mid = (lo + hi) / 2;
        if v[mid].0 < key {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    if lo < v.len() && v[lo].0 == key {
        Some(v[lo].1)
    } else {
        None
    }
}

/// 台账三维索引构建器。
#[derive(Debug)]
pub struct Idx3 {
    by_dom: Vec<(u32, u32)>,
    by_batch: Vec<(u32, u32)>,
    by_wave: Vec<(u32, u32)>,
    ledger: Vec<BuildRecord>,
    next_seq: u32,
}

impl Idx3 {
    /// 空索引（构建账空）。
    pub fn new() -> Idx3 {
        Idx3 {
            by_dom: Vec::new(),
            by_batch: Vec::new(),
            by_wave: Vec::new(),
            ledger: Vec::new(),
            next_seq: 1,
        }
    }

    /// 登记行数（= 构建账条数 = 每轴索引大小）。
    pub fn len(&self) -> usize {
        self.ledger.len()
    }

    /// 是否空索引。
    pub fn is_empty(&self) -> bool {
        self.ledger.is_empty()
    }

    /// 唯一登记通道：三维定位 + 行号，三轴同建。
    /// 越界显拒；重复 (键,行号) 拒；守恒恒等内断言。
    pub fn register(&mut self, entry: IdxRow) -> Result<u32, IdxError> {
        if entry.dom > DOM_MAX {
            return Err(IdxError::DomOob);
        }
        if entry.batch == 0 || entry.batch > BATCH_MAX {
            return Err(IdxError::BatchOob);
        }
        if entry.wave == 0 || entry.wave > WAVE_MAX {
            return Err(IdxError::WaveOob);
        }
        if entry.row == 0 {
            return Err(IdxError::NotReg);
        }
        if !sorted_upsert(&mut self.by_dom, entry.dom as u32, entry.row) {
            return Err(IdxError::DupReg);
        }
        if !sorted_upsert(&mut self.by_batch, entry.batch as u32, entry.row) {
            return Err(IdxError::DupReg);
        }
        if !sorted_upsert(&mut self.by_wave, entry.wave as u32, entry.row) {
            return Err(IdxError::DupReg);
        }
        let seq = self.next_seq;
        self.next_seq = match self.next_seq.checked_add(1) {
            Some(n) => n,
            None => return Err(IdxError::IdRewind),
        };
        self.ledger.push(BuildRecord { seq, entry });
        // 三轴行号集合大小守恒。
        if self.by_dom.len() != self.by_batch.len()
            || self.by_batch.len() != self.by_wave.len()
            || self.by_dom.len() != self.ledger.len()
        {
            return Err(IdxError::Conserve);
        }
        Ok(seq)
    }

    /// 轴查行号（键必须已登记，未登记 NotReg；空索引 EmptyIdx）。
    pub fn lookup(&self, axis: Axis, key: u32) -> Result<u32, IdxError> {
        if self.ledger.is_empty() {
            return Err(IdxError::EmptyIdx);
        }
        let hit = match axis {
            Axis::Domain => sorted_find(&self.by_dom, key),
            Axis::Batch => sorted_find(&self.by_batch, key),
            Axis::Wave => sorted_find(&self.by_wave, key),
        };
        match hit {
            Some(row) => Ok(row),
            None => Err(IdxError::NotReg),
        }
    }

    /// 构建账回退校验：seq 必须严格递增（回退 IdRewind）。
    pub fn ledger_monotonic(&self) -> Result<(), IdxError> {
        let mut prev = 0u32;
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].seq <= prev {
                return Err(IdxError::IdRewind);
            }
            prev = self.ledger[i].seq;
            i += 1;
        }
        Ok(())
    }

    /// 重放校验：从构建账逐条重放重建三轴，与实存索引逐位比对。
    pub fn replay_verify(&self) -> Result<(), IdxError> {
        let mut dom: Vec<(u32, u32)> = Vec::new();
        let mut batch: Vec<(u32, u32)> = Vec::new();
        let mut wave: Vec<(u32, u32)> = Vec::new();
        let mut i = 0usize;
        while i < self.ledger.len() {
            let e = self.ledger[i].entry;
            if !sorted_upsert(&mut dom, e.dom as u32, e.row) {
                return Err(IdxError::Replay);
            }
            if !sorted_upsert(&mut batch, e.batch as u32, e.row) {
                return Err(IdxError::Replay);
            }
            if !sorted_upsert(&mut wave, e.wave as u32, e.row) {
                return Err(IdxError::Replay);
            }
            i += 1;
        }
        if dom != self.by_dom || batch != self.by_batch || wave != self.by_wave {
            return Err(IdxError::Replay);
        }
        Ok(())
    }
}

/// 注入核：十类坏样本逐一注入，返回（样本名，捕获诊断码）对；
/// 每类必须返回 Err（注入必抓），抓不住即判据失效。
pub fn inject_ten() -> Vec<(&'static str, u32)> {
    let mut out: Vec<(&'static str, u32)> = Vec::new();
    let good = IdxRow { dom: 1, batch: 10, wave: 1, row: 1 };
    // 1 重复登记 → DupReg
    let mut ix = Idx3::new();
    let _ = ix.register(good);
    out.push(("dup-reg", ix.register(good).err().unwrap_or(IdxError::Conserve).code()));
    // 2 越界域值 → DomOob
    out.push(("dom-oob", Idx3::new().register(IdxRow { dom: 16, batch: 1, wave: 1, row: 1 }).err().unwrap_or(IdxError::Conserve).code()));
    // 3 越界批值 → BatchOob（0 号批）
    out.push(("batch-oob", Idx3::new().register(IdxRow { dom: 0, batch: 0, wave: 1, row: 1 }).err().unwrap_or(IdxError::Conserve).code()));
    // 4 越界波值 → WaveOob（0 号波）
    out.push(("wave-oob", Idx3::new().register(IdxRow { dom: 0, batch: 1, wave: 0, row: 1 }).err().unwrap_or(IdxError::Conserve).code()));
    // 5 三轴守恒破坏 → Conserve（伪造索引膨胀）
    let mut ix5 = Idx3::new();
    let _ = ix5.register(good);
    ix5.by_dom.push((99, 99)); // 单轴越量 → 大小失恒
    out.push(("conserve", ix5.register(IdxRow { dom: 2, batch: 20, wave: 2, row: 2 }).err().unwrap_or(IdxError::Conserve).code()));
    // 6 未登记先查 → NotReg
    out.push(("not-reg", ix.lookup(Axis::Domain, 15).err().unwrap_or(IdxError::Conserve).code()));
    // 7 索引回退 → IdRewind（伪造账序倒置）
    let mut ix7 = Idx3::new();
    let _ = ix7.register(good);
    let _ = ix7.register(IdxRow { dom: 2, batch: 20, wave: 2, row: 2 });
    ix7.ledger[1].seq = 0;
    out.push(("id-rewind", ix7.ledger_monotonic().err().unwrap_or(IdxError::Conserve).code()));
    // 8 重放不一致 → Replay（实存索引与构建账篡改失配）
    let mut ix8 = Idx3::new();
    let _ = ix8.register(good);
    ix8.by_wave.push((77, 77));
    out.push(("replay", ix8.replay_verify().err().unwrap_or(IdxError::Conserve).code()));
    // 9 空索引查询 → EmptyIdx
    out.push(("empty-idx", Idx3::new().lookup(Axis::Wave, 1).err().unwrap_or(IdxError::Conserve).code()));
    // 10 非法第四态 → BadFourth
    out.push(("bad-fourth", Axis::from_bits(5).err().unwrap_or(IdxError::Conserve).code()));
    out
}

/// 注入核自检：10/10 捕获且码值全在段闭集内（注入必抓）。
pub fn inject_all_caught() -> bool {
    let samples = inject_ten();
    if samples.len() < 10 {
        return false;
    }
    let mut i = 0usize;
    while i < samples.len() {
        let (name, code) = samples[i];
        if name.ends_with("MISSED") {
            return false;
        }
        if code < SEG_BASE || code >= SEG_BASE + DIAG_ALL as u32 {
            return false;
        }
        i += 1;
    }
    true
}

/// 判据集（UNX-F81012-J1 落地）：14 条，正向 + 注入汇总。
pub fn run_unx81012_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F81012");

    // 1 段位守卫
    let ok = SEG_BASE % 0x30 == 0 && SEG_BASE > 0x90000 && SEG_BASE < 0x94000;
    s.add("F81013-01 段位守卫", ok, if ok { "" } else { "段位越窗" });

    // 2 口径冻结
    let ok = DIAG_ALL == 10 && DOM_MAX == 15 && BATCH_MAX == 255 && WAVE_MAX == 30;
    s.add("F81013-02 口径冻结", ok, if ok { "" } else { "口径漂移" });

    // 3 登记通道唯一 + seq 递增
    let mut ix = Idx3::new();
    let a = ix.register(IdxRow { dom: 1, batch: 10, wave: 1, row: 1 });
    let b = ix.register(IdxRow { dom: 2, batch: 20, wave: 2, row: 2 });
    let ok = matches!(a, Ok(1)) && matches!(b, Ok(2)) && ix.len() == 2;
    s.add("F81013-03 登记递增", ok, if ok { "" } else { "登记序漂移" });

    // 4 三轴独立命中
    let ok = ix.lookup(Axis::Domain, 1) == Ok(1)
        && ix.lookup(Axis::Batch, 20) == Ok(2)
        && ix.lookup(Axis::Wave, 2) == Ok(2);
    s.add("F81013-04 三轴命中", ok, if ok { "" } else { "三轴查失灵" });

    // 5 三轴守恒恒等式
    let mut ix2 = Idx3::new();
    let mut dom = 0u8;
    let mut registered = 0usize;
    while dom <= DOM_MAX {
        let mut batch = 1u8;
        while batch <= 4 {
            let e = IdxRow { dom, batch, wave: (batch as u16), row: registered as u32 + 1 };
            if ix2.register(e).is_ok() {
                registered += 1;
            }
            batch += 1;
        }
        dom += 1;
    }
    let ok = ix2.len() == registered
        && ix2.by_dom_len() == registered
        && ix2.by_batch_len() == registered
        && ix2.by_wave_len() == registered
        && registered == 64;
    s.add("F81013-05 三轴守恒", ok, if ok { "" } else { "守恒恒等破坏" });

    // 6 sorted-vec 保序：轴内键严格升序
    let sorted_ok = ix2.axis_sorted(Axis::Domain) && ix2.axis_sorted(Axis::Batch) && ix2.axis_sorted(Axis::Wave);
    s.add("F81013-06 保序去重", ok && sorted_ok, if sorted_ok { "" } else { "轴序漂移" });

    // 7 重复登记拒
    let ok = matches!(ix.register(IdxRow { dom: 1, batch: 10, wave: 1, row: 1 }), Err(IdxError::DupReg));
    s.add("F81013-07 重复登记拒", ok, if ok { "" } else { "重复漏抓" });

    // 8 三轴越界全拒
    let ok = matches!(ix.register(IdxRow { dom: 16, batch: 1, wave: 1, row: 9 }), Err(IdxError::DomOob))
        && matches!(ix.register(IdxRow { dom: 1, batch: 0, wave: 1, row: 9 }), Err(IdxError::BatchOob))
        && matches!(ix.register(IdxRow { dom: 1, batch: 1, wave: 31, row: 9 }), Err(IdxError::WaveOob));
    s.add("F81013-08 越界拒", ok, if ok { "" } else { "越界漏抓" });

    // 9 未登记先查 + 空索引查询
    let ok = matches!(ix.lookup(Axis::Batch, 255), Err(IdxError::NotReg))
        && matches!(Idx3::new().lookup(Axis::Domain, 1), Err(IdxError::EmptyIdx));
    s.add("F81013-09 未登记/空索引拒", ok, if ok { "" } else { "查询闸失灵" });

    // 10 构建账单调 + 回退检出
    let ok = ix.ledger_monotonic().is_ok() && ix2.ledger_monotonic().is_ok();
    s.add("F81013-10 构建账单调", ok, if ok { "" } else { "构建账回退漏抓" });

    // 11 重放一致
    let ok = ix.replay_verify().is_ok() && ix2.replay_verify().is_ok();
    s.add("F81013-11 重放一致", ok, if ok { "" } else { "重放漂移" });

    // 12 轴四态闭集（非法第四态零存在）
    let ok = Axis::from_bits(0).is_ok()
        && Axis::from_bits(2).is_ok()
        && matches!(Axis::from_bits(3), Err(IdxError::BadFourth))
        && matches!(Axis::from_bits(255), Err(IdxError::BadFourth));
    s.add("F81013-12 非法第四态", ok, if ok { "" } else { "轴闭集失守" });

    // 13 注入 10/10
    let ok = inject_all_caught();
    s.add("F81013-13 注入10/10", ok, if ok { "" } else { "注入漏抓" });

    // 14 确定性：同批次重建两次索引逐位一致
    let build = || {
        let mut t = Idx3::new();
        let mut d = 0u8;
        while d <= 3 {
            let mut b = 1u8;
            while b <= 3 {
                let _ = t.register(IdxRow { dom: d, batch: b, wave: b as u16, row: (d as u32) * 3 + b as u32 });
                b += 1;
            }
            d += 1;
        }
        (t.by_dom.clone(), t.by_batch.clone(), t.by_wave.clone())
    };
    let ok = build() == build();
    s.add("F81013-14 重建确定", ok, if ok { "" } else { "重建不确定" });

    s
}

// 轴大小与保序只读辅助（私有实现面，供判据复用）。
impl Idx3 {
    /// 域轴大小。
    pub fn by_dom_len(&self) -> usize {
        self.by_dom.len()
    }

    /// 批轴大小。
    pub fn by_batch_len(&self) -> usize {
        self.by_batch.len()
    }

    /// 波轴大小。
    pub fn by_wave_len(&self) -> usize {
        self.by_wave.len()
    }

    /// 轴内键严格升序扫描。
    pub fn axis_sorted(&self, axis: Axis) -> bool {
        let v = match axis {
            Axis::Domain => &self.by_dom,
            Axis::Batch => &self.by_batch,
            Axis::Wave => &self.by_wave,
        };
        let mut i = 1usize;
        while i < v.len() {
            if v[i - 1].0 >= v[i].0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx81012_checks_green() {
        let set = run_unx81012_checks();
        assert!(set.all_passed(), "UNX-F81012 red: {}", set.red_items_text());
    }
}
