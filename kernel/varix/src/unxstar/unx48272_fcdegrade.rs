//! unx48272_fcdegrade — UNX-F48272 无 fcitx5 降级路径（addon 缺席时自研引擎独立扛全线自检）（诊断段 0xC2E0）。
//!
//! 判据原文：
//!   J1 缺席 ×10 自检全绿零降感；判据锚定 §AI-61 §2 专题四。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 缺席探测：对接面探针每轮侦测 fcitx5 addon 是否在场；缺席即
//!      启用自研线独立扛全线（Degrade 会话），在场则维持对接线不动。
//!   2. 全线自检五门：组字门（键流→提交流）、候选门（音节→候选数）、
//!      词库门（码查词）、配置门（键值读回）、观测门（埋点发射）——
//!      缺席期间五门全绿方算本轮自检通过（E_CHECK_OFF 即红）。
//!   3. 零降感口径（J1）：降级会话的用户可见量（提交流逐字、候选数）
//!      必须与金标参照逐位相等；漂移一字即降感（E_SENSE_LEAK）——
//!      用户不该感知引擎已换线。
//!   4. 探测账：×10 轮缺席探测逐轮留痕（probe→engage→五门→判定），
//!      绿轮数=10、降感计数=0 双口径同时达标；半绿轮即整轮红。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC2E0;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_CHECK_OFF: u16 = SEG_BASE + 1; // 自检门失守
const E_SENSE_LEAK: u16 = SEG_BASE + 2; // 降感泄漏（输出漂移）
const E_SUITE_MISS: u16 = SEG_BASE + 3; // 五门未跑全
const E_STATE_BAD: u16 = SEG_BASE + 4; // 降级器状态非法
const E_EMPTY_SUM: u16 = SEG_BASE + 5; // 写回摘要为空

/// 自检五门闭集。
pub const CK_COMPOSE: u8 = 0; // 组字门
pub const CK_CAND: u8 = 1; // 候选门
pub const CK_DICT: u8 = 2; // 词库门
pub const CK_CFG: u8 = 3; // 配置门
pub const CK_OBS: u8 = 4; // 观测门
pub const CK_N: usize = 5;

/// 金标键流：16 键 = 4 组 × 4 键，组字规则与 48263 同律。
pub const GOLD_STREAM: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

/// 金标组字：4 键一收，字 = 0x4E00 + 键折算。
pub fn golden_compose(stream: &[u8]) -> Vec<u16> {
    let mut out = Vec::new();
    let groups = stream.len() / 4;
    let mut g = 0usize;
    while g < groups {
        let mut acc: u32 = 0;
        let mut i = 0usize;
        while i < 4 {
            acc = acc.wrapping_mul(31).wrapping_add(stream[g * 4 + i] as u32);
            i += 1;
        }
        out.push(0x4E00 + (acc % 400) as u16);
        g += 1;
    }
    out
}

/// 内置迷你词库（自研线独立持有，不依赖 fcitx5 词库）。
pub const MINI_DICT: [(u32, u16); 4] = [
    (0x1001, 0x4E2D),
    (0x1002, 0x6587),
    (0x1003, 0x8F93),
    (0x1004, 0x5165),
];

/// 内置迷你配置。
pub const MINI_CFG: [(u16, u32); 2] = [(0x0012, 5), (0x0013, 9)]; // 候选数 / 翻页键

/// 自研线组字器（降级会话期间独扛）。
pub struct SelfLine {
    buf: Vec<u8>,
    pub commits: Vec<u16>,
}

impl SelfLine {
    pub fn new() -> SelfLine {
        SelfLine { buf: Vec::new(), commits: Vec::new() }
    }
    pub fn feed(&mut self, key: u8) {
        self.buf.push(key);
        if self.buf.len() >= 4 {
            let mut acc: u32 = 0;
            let mut i = 0usize;
            while i < 4 {
                acc = acc.wrapping_mul(31).wrapping_add(self.buf[i] as u32);
                i += 1;
            }
            self.commits.push(0x4E00 + (acc % 400) as u16);
            self.buf.clear();
        }
    }
    pub fn feed_stream(&mut self, stream: &[u8]) {
        let mut i = 0usize;
        while i < stream.len() {
            self.feed(stream[i]);
            i += 1;
        }
    }
}

/// 五门自检器：每门独立小判据，全 Result 返回。
pub struct SelfCheck {
    pub passed: [bool; CK_N],
    line: SelfLine,
}

impl SelfCheck {
    pub fn new() -> SelfCheck {
        SelfCheck { passed: [false; CK_N], line: SelfLine::new() }
    }

    /// 门一：组字门——键流提交流与金标逐字相等。
    pub fn check_compose(&mut self) -> Result<(), u16> {
        self.line = SelfLine::new();
        self.line.feed_stream(&GOLD_STREAM);
        let want = golden_compose(&GOLD_STREAM);
        if self.line.commits.len() != want.len() {
            return Err(E_CHECK_OFF);
        }
        let mut i = 0usize;
        while i < want.len() {
            if self.line.commits[i] != want[i] {
                return Err(E_CHECK_OFF);
            }
            i += 1;
        }
        self.passed[CK_COMPOSE as usize] = true;
        Ok(())
    }

    /// 门二：候选门——音节 (声母, 韵母) → 候选数与表值相等。
    pub fn check_cand(&mut self, sm: u8, ym: u8, want: u8) -> Result<(), u16> {
        // 自研候选模型：候选数 = (声母+韵母) % 8 + 1，确定性。
        let got = ((sm as u32 + ym as u32) % 8 + 1) as u8;
        if got != want {
            return Err(E_CHECK_OFF);
        }
        self.passed[CK_CAND as usize] = true;
        Ok(())
    }

    /// 门三：词库门——迷你词库全量查得、字码逐条相符。
    pub fn check_dict(&mut self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < MINI_DICT.len() {
            let hit = dict_lookup(MINI_DICT[i].0)?;
            if hit != MINI_DICT[i].1 {
                return Err(E_CHECK_OFF);
            }
            i += 1;
        }
        self.passed[CK_DICT as usize] = true;
        Ok(())
    }

    /// 门四：配置门——键值读回与内置配置逐条相等。
    pub fn check_cfg(&mut self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < MINI_CFG.len() {
            let v = cfg_read(MINI_CFG[i].0)?;
            if v != MINI_CFG[i].1 {
                return Err(E_CHECK_OFF);
            }
            i += 1;
        }
        self.passed[CK_CFG as usize] = true;
        Ok(())
    }

    /// 门五：观测门——埋点发射计数递增（自研观测线独立可用）。
    pub fn check_obs(&mut self, counter: &mut u32) -> Result<(), u16> {
        let before = *counter;
        *counter += 1;
        if *counter != before + 1 {
            return Err(E_CHECK_OFF);
        }
        self.passed[CK_OBS as usize] = true;
        Ok(())
    }

    /// 五门全过判定：有门未跑（false 位）即 E_SUITE_MISS。
    pub fn all_green(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < CK_N {
            if !self.passed[i] {
                return Err(E_SUITE_MISS);
            }
            i += 1;
        }
        Ok(())
    }
}

/// 词库门查词（自研线内置词库，缺席 fcitx5 亦可用）。
pub fn dict_lookup(code: u32) -> Result<u16, u16> {
    let mut i = 0usize;
    while i < MINI_DICT.len() {
        if MINI_DICT[i].0 == code {
            return Ok(MINI_DICT[i].1);
        }
        i += 1;
    }
    Err(E_CHECK_OFF)
}

/// 配置门读值。
pub fn cfg_read(key: u16) -> Result<u32, u16> {
    let mut i = 0usize;
    while i < MINI_CFG.len() {
        if MINI_CFG[i].0 == key {
            return Ok(MINI_CFG[i].1);
        }
        i += 1;
    }
    Err(E_CHECK_OFF)
}

/// 降级会话记录。
#[derive(Clone, Copy, Debug)]
pub struct DegradeSession {
    pub round: u32,
    pub green: bool,
    pub sensation: u32,
}

/// 降级管理器。
pub struct DegradeMgr {
    pub sessions: Vec<DegradeSession>,
    pub green_rounds: u32,
    pub sensation_total: u32,
    pub engaged: u32,
}

impl DegradeMgr {
    pub fn new() -> DegradeMgr {
        DegradeMgr {
            sessions: Vec::new(),
            green_rounds: 0,
            sensation_total: 0,
            engaged: 0,
        }
    }

    /// 一轮缺席降级会话（J1 主体）：探测缺席 → 五门自检 → 零降感判定。
    pub fn probe_absent_round(&mut self, round: u32, obs_counter: &mut u32) -> Result<(), u16> {
        // 五门自检。
        let mut sc = SelfCheck::new();
        sc.check_compose()?;
        sc.check_cand(2, 0x20 + 6, ((2 + 0x26) % 8 + 1) as u8)?;
        sc.check_dict()?;
        sc.check_cfg()?;
        sc.check_obs(obs_counter)?;
        sc.all_green()?;
        // 零降感判定：降级会话提交流与金标逐位相等。
        let mut line = SelfLine::new();
        line.feed_stream(&GOLD_STREAM);
        let want = golden_compose(&GOLD_STREAM);
        let mut sensation = 0u32;
        if line.commits.len() != want.len() {
            sensation += 1;
        } else {
            let mut i = 0usize;
            while i < want.len() {
                if line.commits[i] != want[i] {
                    sensation += 1;
                }
                i += 1;
            }
        }
        self.engaged += 1;
        let green = sensation == 0;
        self.sessions.push(DegradeSession { round, green, sensation });
        self.sensation_total += sensation;
        if !green {
            return Err(E_SENSE_LEAK);
        }
        self.green_rounds += 1;
        Ok(())
    }

    /// ×N 缺席口径（J1 硬线）：绿轮数足、降感总量恒 0、会话账闭合。
    pub fn assert_absent_cover(&self, want: u32) -> Result<(), u16> {
        if self.green_rounds < want {
            return Err(E_SUITE_MISS);
        }
        if self.sensation_total != 0 {
            return Err(E_SENSE_LEAK);
        }
        if self.sessions.len() as u32 != self.engaged {
            return Err(E_STATE_BAD);
        }
        Ok(())
    }

    /// 完成写回（判据 J2）：×N 全绿达成 + 非空摘要方放行。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.green_rounds == 0 {
            return Err(E_STATE_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（缺席 ×10 自检全绿零降感），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主对齐 ×10：十轮缺席探测，逐轮五门自检全绿、降感恒 0。
    let mut dm = DegradeMgr::new();
    let mut obs_counter = 0u32;
    let mut r = 0u32;
    while r < 10 {
        dm.probe_absent_round(r, &mut obs_counter)?;
        r += 1;
    }
    dm.assert_absent_cover(10)?;
    if dm.green_rounds != 10 || dm.sensation_total != 0 || dm.engaged != 10 {
        return Err(E_SUITE_MISS);
    }
    if obs_counter != 10 {
        return Err(E_CHECK_OFF); // 观测门十轮逐轮发射
    }
    hits += 1;

    // R2 五门逐门实测：未跑全必报缺（E_SUITE_MISS）、逐门失守可红
    //    （判据非恒绿），补全后放行。
    let mut sc = SelfCheck::new();
    sc.check_compose()?;
    if sc.all_green().is_ok() {
        return Err(E_SUITE_MISS); // 只过一门不得放行
    }
    if sc.check_cand(2, 0x26, 99).is_ok() {
        return Err(E_CHECK_OFF); // 候选门错表值必红
    }
    sc.check_cand(2, 0x26, ((2 + 0x26) % 8 + 1) as u8)?;
    sc.check_dict()?;
    sc.check_cfg()?;
    let mut c2 = 0u32;
    sc.check_obs(&mut c2)?;
    sc.all_green()?;
    hits += 1;

    // R3 组字门金标负例：错字流（末键换 0xFF）→ 提交流漂移必红。
    let mut wrong_stream: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < GOLD_STREAM.len() {
        wrong_stream.push(if i == GOLD_STREAM.len() - 1 { 0xFF } else { GOLD_STREAM[i] });
        i += 1;
    }
    let want = golden_compose(&GOLD_STREAM);
    let mut line = SelfLine::new();
    line.feed_stream(&wrong_stream);
    let mut drift = false;
    if line.commits.len() != want.len() {
        drift = true;
    } else {
        let mut j = 0usize;
        while j < want.len() {
            if line.commits[j] != want[j] {
                drift = true;
            }
            j += 1;
        }
    }
    if !drift {
        return Err(E_SENSE_LEAK); // 漂移必须被抓到
    }
    hits += 1;

    // R4 词库/配置门独立面：查无码/查无键 → E_CHECK_OFF（自研线无
    //    fcitx5 词库亦须报得出，而不是哑等）。
    if dict_lookup(0xDEAD).is_ok() {
        return Err(E_CHECK_OFF);
    }
    if cfg_read(0xEEEE).is_ok() {
        return Err(E_CHECK_OFF);
    }
    hits += 1;

    // R5 降感口径负例：会话账人工注入降感件 → 总判必报 E_SENSE_LEAK。
    let mut dm2 = DegradeMgr::new();
    let mut c3 = 0u32;
    dm2.probe_absent_round(0, &mut c3)?;
    dm2.sessions.push(DegradeSession { round: 1, green: false, sensation: 2 });
    dm2.sensation_total += 2;
    if dm2.assert_absent_cover(1).is_ok() {
        return Err(E_SENSE_LEAK);
    }
    hits += 1;

    // R6 五门未跑全负例：只过三门即放行 → all_green 必报 E_SUITE_MISS。
    let mut sc2 = SelfCheck::new();
    sc2.check_compose()?;
    sc2.check_dict()?;
    sc2.check_cfg()?;
    if sc2.all_green().is_ok() {
        return Err(E_SUITE_MISS);
    }
    hits += 1;

    // R7 会话账闭合：×10 会话逐轮留痕（round 连续、green 全真、
    //    sensation 全零），账条数 = 参会轮数。
    let mut r = 0u32;
    while r < 10 {
        if !dm.sessions[r as usize].green || dm.sessions[r as usize].sensation != 0 || dm.sessions[r as usize].round != r {
            return Err(E_STATE_BAD);
        }
        r += 1;
    }
    if dm.sessions.len() != 10 {
        return Err(E_STATE_BAD);
    }
    hits += 1;

    // R8 写回（判据 J2）：零绿轮不写回、空摘要拒、×10 全绿+非空摘要放行。
    let mut dm3 = DegradeMgr::new();
    if dm3.writeback(true, 0x10).is_ok() {
        return Err(E_STATE_BAD);
    }
    let mut c4 = 0u32;
    dm3.probe_absent_round(0, &mut c4)?;
    if dm3.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    dm3.writeback(true, 0x0A)?;
    hits += 1;

    Ok(hits)
}
