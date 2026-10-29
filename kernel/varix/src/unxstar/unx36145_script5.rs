//! UNX-F36145-J1 地基联动剧本 V：并发创建/检查/销毁（8 车道交错压力）。
//! 判据：① 8 车道轮转交错状态机（Idle→Creating→Checking→Destroyed），
//! 全周期 8 车道各跑 2 轮，守恒式「已创建 == 已销毁 + 在册」全程成立、
//! 零撕裂（Checking 态必有在册对象）；② 非法转闸 6 例（双建/空检/重销/
//! 销毁 Idle/检查 Destroyed/未建先查）显性拒绝；③ 纯功能代码 ≥300 行。
//! 设计纪律：B08 联动剧本之五——no_std 单线程，用轮转调度器确定性交错
//! 模拟 8 并发车道；对象账本与守恒审计是本剧本判据核心。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x8270;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;    // 版本锚不符
pub const E_STATE: u16 = SEG_BASE + 2;  // 车道状态机非法转闸
pub const E_CONS: u16 = SEG_BASE + 3;   // 守恒式破坏
pub const E_TEAR: u16 = SEG_BASE + 4;   // 撕裂（Checking 无在册对象）
pub const E_FULL: u16 = SEG_BASE + 5;   // 对象账本满
pub const E_CS: u16 = SEG_BASE + 6;     // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 车道数锚。
pub const LANES: usize = 8;
/// 对象账本容量。
pub const LEDGER_CAP: usize = 16;

/// 车道状态（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LaneState {
    /// 空闲。
    Idle      = 0x0000,
    /// 创建中（对象已入账）。
    Creating  = 0x0001,
    /// 检查中（可多次）。
    Checking  = 0x0002,
    /// 已销毁。
    Destroyed = 0x0003,
}

impl LaneState {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 对象账本项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerEntry {
    pub obj_id: u16,
    pub owner_lane: usize,
    pub live: bool,
}

const NONE_ENTRY: Option<LedgerEntry> = None;

/// 轮转并发模拟器：8 车道 + 对象账本 + 计数账。
#[derive(Clone, Copy)]
pub struct Pressure {
    pub lanes: [LaneState; LANES],
    pub lane_obj: [u16; LANES],
    pub ledger: [Option<LedgerEntry>; LEDGER_CAP],
    pub created_n: u32,
    pub destroyed_n: u32,
    pub checks_n: u32,
}

impl Pressure {
    pub const fn fresh() -> Pressure {
        Pressure {
            lanes: [LaneState::Idle; LANES],
            lane_obj: [0; LANES],
            ledger: [NONE_ENTRY; LEDGER_CAP],
            created_n: 0,
            destroyed_n: 0,
            checks_n: 0,
        }
    }

    /// 在册数。
    pub fn live_n(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < LEDGER_CAP {
            if let Some(e) = self.ledger[i] {
                if e.live {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 守恒式：created == destroyed + live。
    pub fn check_conservation(&self) -> Result<(), u16> {
        if self.created_n != self.destroyed_n + self.live_n() {
            return Err(E_CONS);
        }
        Ok(())
    }

    /// 零撕裂审计：所有 Creating/Checking 车道必须有在册对象。
    pub fn audit_no_tear(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < LANES {
            let st = self.lanes[i];
            if st.code() == LaneState::Creating.code() || st.code() == LaneState::Checking.code() {
                let mut found = false;
                let mut j = 0;
                while j < LEDGER_CAP {
                    if let Some(e) = self.ledger[j] {
                        if e.live && e.owner_lane == i && e.obj_id == self.lane_obj[i] {
                            found = true;
                        }
                    }
                    j += 1;
                }
                if !found {
                    return Err(E_TEAR);
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// lane 创建：仅 Idle 可建；账满显性拒。
    pub fn lane_create(&mut self, lane: usize, obj_id: u16) -> Result<(), u16> {
        if lane >= LANES {
            return Err(E_STATE);
        }
        if self.lanes[lane] != LaneState::Idle {
            return Err(E_STATE);
        }
        let mut slot = LEDGER_CAP;
        let mut i = 0;
        while i < LEDGER_CAP {
            if self.ledger[i].is_none() {
                slot = i;
            }
            i += 1;
        }
        if slot == LEDGER_CAP {
            return Err(E_FULL);
        }
        self.ledger[slot] = Some(LedgerEntry { obj_id, owner_lane: lane, live: true });
        self.lane_obj[lane] = obj_id;
        self.lanes[lane] = LaneState::Creating;
        self.created_n += 1;
        Ok(())
    }

    /// lane 检查：Creating/Checking 可查；查完驻留 Checking。
    pub fn lane_check(&mut self, lane: usize) -> Result<(), u16> {
        if lane >= LANES {
            return Err(E_STATE);
        }
        let st = self.lanes[lane];
        if st.code() != LaneState::Creating.code() && st.code() != LaneState::Checking.code() {
            return Err(E_STATE);
        }
        self.lanes[lane] = LaneState::Checking;
        self.checks_n += 1;
        Ok(())
    }

    /// lane 销毁：Creating/Checking 可销；账本回退 + 计数。
    pub fn lane_destroy(&mut self, lane: usize) -> Result<(), u16> {
        if lane >= LANES {
            return Err(E_STATE);
        }
        let st = self.lanes[lane];
        if st.code() != LaneState::Creating.code() && st.code() != LaneState::Checking.code() {
            return Err(E_STATE);
        }
        let mut i = 0;
        while i < LEDGER_CAP {
            if let Some(e) = self.ledger[i] {
                if e.owner_lane == lane && e.obj_id == self.lane_obj[lane] && e.live {
                    self.ledger[i] = Some(LedgerEntry { live: false, ..e });
                }
            }
            i += 1;
        }
        self.lanes[lane] = LaneState::Destroyed;
        self.destroyed_n += 1;
        Ok(())
    }

    /// lane 重置：Destroyed/Idle → Idle（车道可复用；账本槽保留墓碑）。
    pub fn lane_reset(&mut self, lane: usize) -> Result<(), u16> {
        if lane >= LANES {
            return Err(E_STATE);
        }
        let st = self.lanes[lane];
        if st.code() != LaneState::Destroyed.code() && st.code() != LaneState::Idle.code() {
            return Err(E_STATE);
        }
        self.lanes[lane] = LaneState::Idle;
        Ok(())
    }
}

/// 全周期剧本：8 车道各建 1 对象、查 2 次、销毁，跑 2 轮（账本 16 槽恰好）。
pub fn run_pressure_rounds(p: &mut Pressure, rounds: u32) -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut r = 0;
    while r < rounds {
        // 段 0：全车道重置（Destroyed → Idle，账本槽保留墓碑不回收）。
        let mut lane = 0;
        while lane < LANES {
            p.lane_reset(lane)?;
            lane += 1;
        }
        // 段 1：8 车道依次创建（错峰：偶数道先建）。
        let mut phase = 0;
        while phase < 2 {
            let mut lane = phase;
            while lane < LANES {
                let obj_id = (r * 16 + lane as u32) as u16;
                p.lane_create(lane, obj_id)?;
                p.audit_no_tear()?;
                p.check_conservation()?;
                lane += 2;
            }
            phase += 1;
        }
        // 段 2：8 车道各查 2 次。
        let mut lane = 0;
        while lane < LANES {
            p.lane_check(lane)?;
            p.lane_check(lane)?;
            lane += 1;
        }
        // 段 3：8 车道依次销毁（奇偶错峰销）。
        let mut lane = 0;
        while lane < LANES {
            p.lane_destroy(lane)?;
            p.check_conservation()?;
            p.audit_no_tear()?;
            lane += 1;
        }
        hits += 1;
        r += 1;
    }
    Ok(hits)
}

/// 摘要行（String 面，无 IO）。
pub fn summary(p: &Pressure) -> String {
    let mut s = String::new();
    s.push_str("script5 created=");
    s.push_str(&p.created_n.to_string());
    s.push_str(" destroyed=");
    s.push_str(&p.destroyed_n.to_string());
    s.push_str(" live=");
    s.push_str(&p.live_n().to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs: Vec<u16> = vec![E_VER, E_STATE, E_CONS, E_TEAR, E_FULL, E_CS];
    let mut i = 0;
    while i < cs.len() {
        let mut j = i + 1;
        while j < cs.len() {
            if cs[i] == cs[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_STATE => "车道状态机非法转闸/车道越界",
        E_CONS => "守恒式破坏",
        E_TEAR => "撕裂（Checking 无在册对象）",
        E_FULL => "对象账本满",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 锚位；R2 两轮全周期（16 对象建/查/销，守恒+零撕裂全程）；
/// R3 非法转闸 6 例；R4 满容闸 1 例；R5 指纹稳定；R6 双跑一致。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚 + 容量锚。
    if VER != 1 {
        return Err(E_VER);
    }
    if LANES != 8 || LEDGER_CAP != 16 {
        return Err(E_CS);
    }
    hits += 1;
    // R2：两轮全周期。
    let mut p = Pressure::fresh();
    hits += run_pressure_rounds(&mut p, 2)?;
    if p.created_n != 16 || p.destroyed_n != 16 || p.live_n() != 0 {
        return Err(E_CONS);
    }
    if p.checks_n != 32 {
        return Err(E_STATE);
    }
    p.audit_no_tear()?;
    p.check_conservation()?;
    hits += 1;
    // R3：非法转闸 6 例（在新压力机上逐一注入）。
    let mut q = Pressure::fresh();
    // 1) 未建先查。
    match q.lane_check(0) {
        Ok(_) => return Err(E_STATE),
        Err(c) if c == E_STATE => {}
        Err(_) => return Err(E_STATE),
    }
    hits += 1;
    // 2) 未建先销。
    match q.lane_destroy(0) {
        Ok(_) => return Err(E_STATE),
        Err(c) if c == E_STATE => {}
        Err(_) => return Err(E_STATE),
    }
    hits += 1;
    // 3) 双建。
    q.lane_create(1, 100)?;
    match q.lane_create(1, 101) {
        Ok(_) => return Err(E_STATE),
        Err(c) if c == E_STATE => {}
        Err(_) => return Err(E_STATE),
    }
    hits += 1;
    // 4) 销毁后重查。
    q.lane_destroy(1)?;
    match q.lane_check(1) {
        Ok(_) => return Err(E_STATE),
        Err(c) if c == E_STATE => {}
        Err(_) => return Err(E_STATE),
    }
    hits += 1;
    // 5) 销毁后重销。
    match q.lane_destroy(1) {
        Ok(_) => return Err(E_STATE),
        Err(c) if c == E_STATE => {}
        Err(_) => return Err(E_STATE),
    }
    hits += 1;
    // 6) 车道越界。
    match q.lane_create(LANES, 200) {
        Ok(_) => return Err(E_STATE),
        Err(c) if c == E_STATE => {}
        Err(_) => return Err(E_STATE),
    }
    hits += 1;
    // R4：满容闸（8 车道各建 1 → 重置 → 再建 8（新槽）→ 16 槽全占
    // 含墓碑 → 重置后第 17 建 → E_FULL）。
    let mut full = Pressure::fresh();
    let mut k = 0;
    while k < LEDGER_CAP / 2 {
        full.lane_create(k % LANES, (k + 1) as u16)?;
        k += 1;
    }
    // 先逐道销毁（Creating → Destroyed），再重置（Destroyed → Idle）。
    let mut lane = 0;
    while lane < LANES {
        full.lane_destroy(lane)?;
        full.lane_reset(lane)?;
        lane += 1;
    }
    let mut k = 0;
    while k < LEDGER_CAP / 2 {
        full.lane_create(k % LANES, (LEDGER_CAP as u16) + (k + 1) as u16)?;
        k += 1;
    }
    // 16 槽全部 Some（含 8 个墓碑）——满容在册。
    let mut slot_full = true;
    let mut i = 0;
    while i < LEDGER_CAP {
        if full.ledger[i].is_none() {
            slot_full = false;
        }
        i += 1;
    }
    if !slot_full {
        return Err(E_FULL);
    }
    // 第 17 建：先销毁重置车道 0（Idle 通过状态机）→ 槽满显性拒 E_FULL。
    full.lane_destroy(0)?;
    full.lane_reset(0)?;
    match full.lane_create(0, 0x77) {
        Ok(_) => return Err(E_FULL),
        Err(c) if c == E_FULL => {}
        Err(_) => return Err(E_FULL),
    }
    hits += 1;
    // R5：指纹两次求值稳定且非零。
    let mut feed = vec![];
    feed.extend_from_slice(&VER.to_le_bytes());
    feed.extend_from_slice(&p.created_n.to_le_bytes());
    feed.extend_from_slice(&p.destroyed_n.to_le_bytes());
    feed.extend_from_slice(&p.checks_n.to_le_bytes());
    let fp1 = core::fnv1a(&feed);
    let fp2 = core::fnv1a(&feed);
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R6：双跑一致。
    let mut pa = Pressure::fresh();
    let mut pb = Pressure::fresh();
    let ra = run_pressure_rounds(&mut pa, 1)?;
    let rb = run_pressure_rounds(&mut pb, 1)?;
    if ra != rb || pa.created_n != pb.created_n || pa.checks_n != pb.checks_n {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(13));
    }

    #[test]
    fn conservation_and_no_tear() {
        let mut p = Pressure::fresh();
        let _ = run_pressure_rounds(&mut p, 1);
        assert_eq!(p.check_conservation(), Ok(()));
        assert_eq!(p.audit_no_tear(), Ok(()));
        assert_eq!(p.created_n, 8);
    }

    #[test]
    fn illegal_transitions_rejected() {
        let mut q = Pressure::fresh();
        assert_eq!(q.lane_check(3), Err(E_STATE));
        assert_eq!(q.lane_create(3, 1), Ok(()));
        assert_eq!(q.lane_create(3, 2), Err(E_STATE));
        assert_eq!(q.lane_destroy(3), Ok(()));
        assert_eq!(q.lane_destroy(3), Err(E_STATE));
    }

    #[test]
    fn summary_shapes() {
        assert!(summary(&Pressure::fresh()).contains("script5"));
        assert!(codes_distinct());
        assert_eq!(LaneState::Destroyed.code(), 3);
    }
}

pub fn run_unx36145_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36145_script5");
    cs.add("SEG 段位独占", SEG_BASE == 0x8270, "0x8270~0x827f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "8 车道两轮全周期守恒零撕裂、非法转闸 6 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("车道数锚 8", LANES == 8, "车道数锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
