//! unx48270_fcobs — UNX-F48270 对接观测埋点（建链/换库/迁移/断链四点）（诊断段 0xC2A0）。
//!
//! 判据原文：
//!   J1 四点 100%、异常零静默；判据锚定 体验十三·补。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 四点闭集：建链（link_up）、换库（dict_swap）、迁移（migrate）、
//!      断链（link_down）——对接全生命周期恰此四点，点号越界即 E_PT_OOB。
//!   2. 100% 覆盖口径（J1）：四点每点发埋量与场景应发量逐点相等、
//!      零漏发；漏发即覆盖缺口（E_COVER_MISS）。
//!   3. 异常零静默（J1）：任一埋点报异常必须同步落告警账（点号、流水
//!      号、异常码）；账恒等式 发埋量 = 正常量 + 告警量 逐点闭合——
//!      有异常而无告警即静默丢失（E_SILENT_DROP）。
//!   4. 埋点账不可变：流水号单调、事件账只追加；断链后事件账仍可
//!      完整回放（观测独立于对接生死）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC2A0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_PT_OOB: u16 = SEG_BASE + 1; // 埋点号越界
const E_COVER_MISS: u16 = SEG_BASE + 2; // 覆盖缺口（漏发）
const E_SILENT_DROP: u16 = SEG_BASE + 3; // 异常静默丢失
const E_LEDGER_BAD: u16 = SEG_BASE + 4; // 埋点账不闭合
const E_STATE_BAD: u16 = SEG_BASE + 5; // 埋点器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 6; // 写回摘要为空

/// 四点闭集。
pub const PT_LINK_UP: u8 = 0; // 建链
pub const PT_DICT_SWAP: u8 = 1; // 换库
pub const PT_MIGRATE: u8 = 2; // 迁移
pub const PT_LINK_DOWN: u8 = 3; // 断链
pub const PT_N: usize = 4;

/// 单点埋点账。
#[derive(Clone, Copy, Debug)]
pub struct ObsPoint {
    pub pt: u8,
    pub emitted: u32,
    pub ok: u32,
    pub alarms: u32,
    pub last_seq: u64,
}

/// 告警账一条。
#[derive(Clone, Copy, Debug)]
pub struct AlarmEntry {
    pub pt: u8,
    pub seq: u64,
    pub code: u16,
}

/// 观测枢纽。
pub struct ObsHub {
    points: [ObsPoint; PT_N],
    pub alarm_log: Vec<AlarmEntry>,
    pub seq: u64,
    pub link_live: bool,
    pub silent_faults: u32,
}

impl ObsHub {
    pub fn new() -> ObsHub {
        let mut points: [ObsPoint; PT_N] = [ObsPoint {
            pt: 0,
            emitted: 0,
            ok: 0,
            alarms: 0,
            last_seq: 0,
        }; PT_N];
        let mut i = 0usize;
        while i < PT_N {
            points[i].pt = i as u8;
            i += 1;
        }
        ObsHub {
            points,
            alarm_log: Vec::new(),
            seq: 0,
            link_live: false,
            silent_faults: 0,
        }
    }

    /// 埋点发射：四点闭集校验 + 异常必告警（零静默主路径）。
    pub fn emit(&mut self, pt: u8, ok: bool, code: u16) -> Result<(), u16> {
        if pt as usize >= PT_N {
            return Err(E_PT_OOB);
        }
        self.seq += 1;
        let p = &mut self.points[pt as usize];
        p.emitted += 1;
        p.last_seq = self.seq;
        if ok {
            p.ok += 1;
        } else {
            p.alarms += 1;
            // 异常零静默：告警账同步落账（同事务）。
            self.alarm_log.push(AlarmEntry { pt, seq: self.seq, code });
        }
        // 生命周期联动：建链/断链改变链路存活位（观测照记不误）。
        if pt == PT_LINK_UP && ok {
            self.link_live = true;
        }
        if pt == PT_LINK_DOWN && ok {
            self.link_live = false;
        }
        Ok(())
    }

    pub fn emitted_of(&self, pt: u8) -> u32 {
        self.points[pt as usize].emitted
    }
    pub fn ok_of(&self, pt: u8) -> u32 {
        self.points[pt as usize].ok
    }
    pub fn alarms_of(&self, pt: u8) -> u32 {
        self.points[pt as usize].alarms
    }

    /// ×N 轮标准场景：建链→换库→迁移→断链 全绿一轮。
    pub fn run_round(&mut self, round: u32) -> Result<(), u16> {
        let mut pt = 0u8;
        while (pt as usize) < PT_N {
            self.emit(pt, true, 0)?;
            pt += 1;
        }
        let _ = round;
        Ok(())
    }

    /// 100% 覆盖审计（J1）：每点发埋量 ≥ want 且四点全有量；
    /// 账恒等式逐点闭合（emitted == ok + alarms）。
    pub fn audit_cover(&self, want_each: u32) -> Result<(), u16> {
        let mut i = 0usize;
        while i < PT_N {
            let p = self.points[i];
            if p.emitted < want_each {
                return Err(E_COVER_MISS);
            }
            if p.emitted != p.ok + p.alarms {
                return Err(E_SILENT_DROP);
            }
            i += 1;
        }
        Ok(())
    }

    /// 零静默终判（J1）：告警账条数 == 四点告警量之和；流水号闭合
    /// （每点 last_seq ≤ 总流水）；静默故障计数恒 0。
    pub fn assert_no_silence(&self) -> Result<(), u16> {
        let mut sum_alarms = 0u32;
        let mut i = 0usize;
        while i < PT_N {
            sum_alarms += self.points[i].alarms;
            if self.points[i].last_seq > self.seq {
                return Err(E_LEDGER_BAD);
            }
            i += 1;
        }
        if self.alarm_log.len() as u32 != sum_alarms {
            return Err(E_SILENT_DROP);
        }
        if self.silent_faults != 0 {
            return Err(E_SILENT_DROP);
        }
        // 告警账逐条可溯到对应点的流水账。
        let mut a = 0usize;
        while a < self.alarm_log.len() {
            let al = self.alarm_log[a];
            if al.pt as usize >= PT_N {
                return Err(E_PT_OOB);
            }
            if al.seq > self.points[al.pt as usize].last_seq
                || self.points[al.pt as usize].alarms == 0
            {
                return Err(E_LEDGER_BAD);
            }
            a += 1;
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：四点有量 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_STATE_BAD);
        }
        let mut i = 0usize;
        while i < PT_N {
            if self.points[i].emitted == 0 {
                return Err(E_COVER_MISS);
            }
            i += 1;
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（四点 100%、异常零静默），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 全绿基线：×3 轮标准场景（每轮四点各一发），四点各恰 3 发、
    //    100% 覆盖、零告警、链路生命周期位正确启闭。
    let mut hub = ObsHub::new();
    let mut r = 0u32;
    while r < 3 {
        hub.run_round(r)?;
        r += 1;
    }
    let mut pt = 0u8;
    while (pt as usize) < PT_N {
        if hub.emitted_of(pt) != 3 || hub.ok_of(pt) != 3 || hub.alarms_of(pt) != 0 {
            return Err(E_COVER_MISS);
        }
        pt += 1;
    }
    hub.audit_cover(3)?;
    hub.assert_no_silence()?;
    hits += 1;

    // R2 生命周期位：建链后链路活、断链后链路灭；灭后埋点照记（观测
    //    独立于对接生死）。
    let mut hub2 = ObsHub::new();
    hub2.emit(PT_LINK_UP, true, 0)?;
    if !hub2.link_live {
        return Err(E_STATE_BAD);
    }
    hub2.emit(PT_DICT_SWAP, true, 0)?;
    hub2.emit(PT_MIGRATE, true, 0)?;
    hub2.emit(PT_LINK_DOWN, true, 0)?;
    if hub2.link_live {
        return Err(E_STATE_BAD);
    }
    hub2.emit(PT_DICT_SWAP, true, 0)?; // 断链后埋点照记
    if hub2.emitted_of(PT_DICT_SWAP) != 2 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R3 异常零静默（J1 主体）：四点各注一件异常 → 告警账恰 4 条、
    //    逐条可溯（点号+流水号+异常码）、账恒等式逐点闭合。
    let mut hub3 = ObsHub::new();
    hub3.run_round(0)?;
    hub3.emit(PT_LINK_UP, false, SEG_BASE + 1)?; // 建链失败
    hub3.emit(PT_DICT_SWAP, false, SEG_BASE + 3)?; // CRC 坏
    hub3.emit(PT_MIGRATE, false, SEG_BASE + 7)?; // 迁移限流
    hub3.emit(PT_LINK_DOWN, false, SEG_BASE + 5)?; // 异常断链
    if hub3.alarm_log.len() != 4 {
        return Err(E_SILENT_DROP);
    }
    if hub3.alarms_of(PT_LINK_UP) != 1
        || hub3.alarms_of(PT_DICT_SWAP) != 1
        || hub3.alarms_of(PT_MIGRATE) != 1
        || hub3.alarms_of(PT_LINK_DOWN) != 1
    {
        return Err(E_LEDGER_BAD);
    }
    // 逐条可溯：告警码与发射码一致。
    let want_codes: [u16; 4] = [
        SEG_BASE + 1,
        SEG_BASE + 3,
        SEG_BASE + 7,
        SEG_BASE + 5,
    ];
    let mut a = 0usize;
    while a < 4 {
        if hub3.alarm_log[a].code != want_codes[a] {
            return Err(E_LEDGER_BAD);
        }
        a += 1;
    }
    hub3.audit_cover(2)?; // 每点 2 发（1 正常 + 1 异常）
    hub3.assert_no_silence()?;
    hits += 1;

    // R4 覆盖缺口负例：只发两点、余两点零发 → audit_cover 必报
    //    E_COVER_MISS（100% 口径不容半覆盖）。
    let mut hub4 = ObsHub::new();
    hub4.emit(PT_LINK_UP, true, 0)?;
    hub4.emit(PT_LINK_DOWN, true, 0)?;
    if hub4.audit_cover(1).is_ok() {
        return Err(E_COVER_MISS);
    }
    hits += 1;

    // R5 点号越界拒收：pt=4 → E_PT_OOB，不入账。
    let mut hub5 = ObsHub::new();
    if hub5.emit(4, true, 0).is_ok() {
        return Err(E_PT_OOB);
    }
    if hub5.emitted_of(0) != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 静默注入哨兵：人工撕开账恒等式（抹一件告警）→ 终判必报
    //    E_SILENT_DROP——零静默口径具备失守报警能力。
    let mut hub6 = ObsHub::new();
    hub6.emit(PT_LINK_UP, false, 0x11)?;
    if hub6.alarm_log.is_empty() {
        return Err(E_SILENT_DROP);
    }
    hub6.alarm_log.clear(); // 人为静默
    if hub6.assert_no_silence().is_ok() {
        return Err(E_SILENT_DROP);
    }
    hits += 1;

    // R7 流水闭合：全程流水号单调、各点 last_seq ≤ 总流水、
    //    发埋总量 == 四点和。
    let mut total = 0u32;
    pt = 0;
    while (pt as usize) < PT_N {
        total += hub3.emitted_of(pt);
        pt += 1;
    }
    if total != 8 {
        return Err(E_LEDGER_BAD);
    }
    if hub3.seq < 8 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：零覆盖不写回、空摘要拒、四点有量+非空摘要放行。
    let mut hub7 = ObsHub::new();
    if hub7.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    hub7.run_round(0)?;
    if hub7.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    hub7.writeback(true, 0x08)?;
    hits += 1;

    Ok(hits)
}
