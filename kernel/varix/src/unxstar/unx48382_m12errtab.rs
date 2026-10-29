//! unx48382_m12errtab — UNX-F48382 M12 错误码全域总表（B09–B20 并表入 D1 联签 AI-51）（诊断段 0xD0A0）。
//!
//! 判据原文：
//!   J1 并表零漂移零混淆；判据锚定 D1 同源、联签 AI-51。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 布段夹具：B09..B20 十二批诊断段 0xC2A0 起步、段距 0x40，每批
//!      错误码域 [seg+1, seg+0x1F]（31 码/批）；段距 > 域宽 → 批间域
//!      天然不重叠——并表零混淆的布段前提，实测段位以各批收官印在册
//!      值为准，本表逻辑对任意满足段距约束的布段成立。
//!   2. 并表全序：全域总表 = 十二批 31×12=372 码，按码值升序一列展开；
//!      相邻码恒严格递增（无重码、无倒序）——零混淆的表形态判据。
//!   3. 零漂移：每个码必须落在其登记批的域内（seg+1 ≤ 码 ≤ seg+0x1F），
//!      且按码值反查所属段必须与登记批一致——码漂出本批域即废表。
//!   4. 查表口径：域内首码/中位/末码三样点查表必中登记批；段间空隙
//!      （seg+0x20..seg+0x3F）查表必须落空——查有必有、查无必无。
//!   5. D1 联签：总表全域 FNV 折叠得联签；同表重建同签（可复现），
//!      任一码被篡改则联签必漂移且漂移扫描必定位越域码（防篡改双保险）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xD0A0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_COUNT_BAD: u16 = SEG_BASE + 1;  // 总表条数不符
const E_ORDER_BAD: u16 = SEG_BASE + 2;  // 全序破坏（重码/倒序）
const E_DRIFT: u16 = SEG_BASE + 3;      // 码漂出登记批域
const E_GAP_BAD: u16 = SEG_BASE + 4;    // 批间域相撞/批内断档
const E_LOOKUP_BAD: u16 = SEG_BASE + 5; // 查表口径失效
const E_SIG_DRIFT: u16 = SEG_BASE + 6;  // 联签复现漂移
const E_TAMPER_MISS: u16 = SEG_BASE + 7; // 篡改未检出
const E_COVER_BAD: u16 = SEG_BASE + 8;  // 批内覆盖不完整
const E_TABLE_BAD: u16 = SEG_BASE + 9;  // 批表规约违规
const E_CODE_RANGE: u16 = SEG_BASE + 10; // 诊断码越段

/// 批数（B09..B20）。
pub const BATCH_N: usize = 12;

/// 每批错误码域宽（seg+1 ..= seg+0x1F）。
pub const CODES_PER: u16 = 0x1F;

/// 批夹具段位：0xC2A0 起步、段距 0x40（> 域宽 0x1F，域间永不重叠）。
pub fn batch_seg(b: usize) -> u16 {
    0xC2A0 + (b as u16) * 0x40
}

/// 批序号：B09..B20（业务批号，严格递增）。
pub fn batch_no(b: usize) -> u32 {
    9 + b as u32
}

/// FNV-1a 折叠。
pub fn fnv1a(seed: u64, x: u64) -> u64 {
    let mut h = seed;
    h ^= x;
    h = h.wrapping_mul(0x0000_0100_0000_01B3);
    h
}

/// 全域错误码总表：(码, 登记批序 0..12) 有序一列。
pub struct ErrTab {
    entries: Vec<(u16, u8)>,
}

impl ErrTab {
    /// 并表：逐批展开 31 码，段升序拼接 → 全表天然按码升序。
    pub fn build() -> Self {
        let mut t = ErrTab { entries: Vec::new() };
        let mut b = 0usize;
        while b < BATCH_N {
            let base = batch_seg(b);
            let mut c = 1u16;
            while c <= CODES_PER {
                t.entries.push((base + c, b as u8));
                c += 1;
            }
            b += 1;
        }
        t
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn entries(&self) -> &[(u16, u8)] {
        &self.entries
    }

    /// 查表：码 → 登记批序；未登记码返回 None。
    pub fn lookup(&self, code: u16) -> Option<u8> {
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].0 == code {
                return Some(self.entries[i].1);
            }
            i += 1;
        }
        None
    }

    /// 全序扫描：相邻码恒严格递增（零混淆形态）。
    pub fn strictly_increasing(&self) -> Result<(), u16> {
        let mut i = 1;
        while i < self.entries.len() {
            if self.entries[i].0 <= self.entries[i - 1].0 {
                return Err(E_ORDER_BAD);
            }
            i += 1;
        }
        Ok(())
    }

    /// 零漂移扫描：每码落在登记批域内 + 反查段归属一致。
    pub fn no_drift(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.entries.len() {
            let (code, b) = self.entries[i];
            let base = batch_seg(b as usize);
            if code <= base || code > base + CODES_PER {
                return Err(E_DRIFT);
            }
            // 按码值反查所属段：唯一落入的批域必须就是登记批。
            let mut owner = None;
            let mut k = 0usize;
            while k < BATCH_N {
                let kb = batch_seg(k);
                if code > kb && code <= kb + CODES_PER {
                    owner = Some(k as u8);
                    break;
                }
                k += 1;
            }
            match owner {
                Some(o) if o == b => {}
                _ => return Err(E_DRIFT),
            }
            i += 1;
        }
        Ok(())
    }

    /// D1 联签：全表（码, 批序）有序 FNV 折叠。
    pub fn signature(&self) -> u64 {
        let mut h: u64 = 0xCBF2_9CE4_8422_2325;
        let mut i = 0;
        while i < self.entries.len() {
            h = fnv1a(h, self.entries[i].0 as u64);
            h = fnv1a(h, self.entries[i].1 as u64);
            i += 1;
        }
        h | (1 << 63)
    }
}

/// 布段规约：十二段严格递增、段距 > 域宽（域间永不相撞的前提判据）。
pub fn seg_table_ok() -> Result<(), u16> {
    let mut b = 1usize;
    while b < BATCH_N {
        if batch_seg(b) <= batch_seg(b - 1) {
            return Err(E_TABLE_BAD);
        }
        if batch_seg(b) - batch_seg(b - 1) <= CODES_PER {
            return Err(E_GAP_BAD);
        }
        b += 1;
    }
    // 批号严格递增。
    let mut b = 1usize;
    while b < BATCH_N {
        if batch_no(b) <= batch_no(b - 1) {
            return Err(E_TABLE_BAD);
        }
        b += 1;
    }
    Ok(())
}

/// 自例程判据：逐条落实 J1（并表零漂移零混淆），返回命中组数。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 布段规约：十二段严格递增、段距 > 0x1F；批号 9..20 严格递增。
    seg_table_ok()?;
    if batch_no(0) != 9 || batch_no(BATCH_N - 1) != 20 {
        return Err(E_TABLE_BAD);
    }
    hits += 1;

    // R2 并表全量：恰 31×12=372 条；按码升序全展开（零混淆形态）。
    let tab = ErrTab::build();
    if tab.len() != CODES_PER as usize * BATCH_N {
        return Err(E_COUNT_BAD);
    }
    tab.strictly_increasing()?;
    hits += 1;

    // R3 零漂移：372 码逐条域内 + 反查归属一致。
    tab.no_drift()?;
    hits += 1;

    // R4 批间域不撞：相邻批域上沿 < 下一批域下沿（结构性零混淆）。
    let mut b = 0usize;
    while b + 1 < BATCH_N {
        let up = batch_seg(b) + CODES_PER;
        let lo_next = batch_seg(b + 1) + 1;
        if up >= lo_next {
            return Err(E_GAP_BAD);
        }
        b += 1;
    }
    hits += 1;

    // R5 查表口径：×12 批各取域内首/中/末三样点必中登记批；
    //    段间空隙（seg+0x20）查表必须落空。
    let mut b = 0usize;
    while b < BATCH_N {
        let base = batch_seg(b);
        let probes = [base + 1, base + CODES_PER / 2, base + CODES_PER];
        let mut p = 0usize;
        while p < probes.len() {
            match tab.lookup(probes[p]) {
                Some(owner) if owner as usize == b => {}
                _ => return Err(E_LOOKUP_BAD),
            }
            p += 1;
        }
        match tab.lookup(base + CODES_PER + 1) {
            None => {}
            _ => return Err(E_LOOKUP_BAD),
        }
        b += 1;
    }
    hits += 1;

    // R6 批内覆盖：每批恰 31 条、域内连续无断档。
    let mut b = 0usize;
    while b < BATCH_N {
        let mut count = 0u32;
        let mut expect = batch_seg(b) + 1;
        let mut i = 0usize;
        while i < tab.len() {
            let (code, owner) = tab.entries()[i];
            if owner as usize == b {
                if code != expect {
                    return Err(E_COVER_BAD);
                }
                expect += 1;
                count += 1;
            }
            i += 1;
        }
        if count != CODES_PER as u32 {
            return Err(E_COVER_BAD);
        }
        b += 1;
    }
    hits += 1;

    // R7 D1 联签复现：同表重建同签；任一码篡改 → 联签必漂移，
    //    且漂移扫描必定位越域码（防篡改双保险）。
    let sig1 = tab.signature();
    let tab2 = ErrTab::build();
    if tab2.signature() != sig1 || sig1 == 0 {
        return Err(E_SIG_DRIFT);
    }
    let mut tam = ErrTab::build();
    // 篡改：把登记批 5 的一个码搬出域（+0x20 落进批间空隙）。
    let mut moved = 0usize;
    let mut i = 0usize;
    while i < tam.entries().len() {
        if tam.entries()[i].1 == 5 {
            let (code, owner) = tam.entries()[i];
            tam.entries[i] = (code + 0x20, owner);
            moved += 1;
            break;
        }
        i += 1;
    }
    if moved != 1 {
        return Err(E_TAMPER_MISS);
    }
    if tam.signature() == sig1 {
        return Err(E_TAMPER_MISS);
    }
    match tam.no_drift() {
        Err(E_DRIFT) => {}
        _ => return Err(E_TAMPER_MISS),
    }
    hits += 1;

    // R8 查表反例：域外远端码（未布段值）查表落空。
    match tab.lookup(0x1234) {
        None => {}
        _ => return Err(E_LOOKUP_BAD),
    }
    hits += 1;

    // R9 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 10] = [
        E_COUNT_BAD,
        E_ORDER_BAD,
        E_DRIFT,
        E_GAP_BAD,
        E_LOOKUP_BAD,
        E_SIG_DRIFT,
        E_TAMPER_MISS,
        E_COVER_BAD,
        E_TABLE_BAD,
        E_CODE_RANGE,
    ];
    let mut ci = 0;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
