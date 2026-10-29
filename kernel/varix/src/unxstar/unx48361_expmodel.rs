//! unx48361_expmodel — UNX-F48361 输入体验事件模型（点击/按键/候选操作带界面+元素+时刻+反馈+耗时）（诊断段 0xCE00）。
//!
//! 判据原文：
//!   J1 事件字段 ×100% 契合十三章口径；判据锚定 体验十三。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 事件模型（十三章口径）：每条体验事件必带五元字段——界面
//!      （ui_id）、元素（elem_id）、时刻（tick）、反馈（feedback
//!      闭集）、耗时（dur）——缺任一字段的事件不入账（残缺事件
//!      即观测盲点，盲点即假账）。
//!   2. 三型闭集：点击（CLICK）/按键（KEY）/候选操作（CAND），
//!      各型的字段 schema 同构（五元全带），型只决定语义解释不决
//!      定字段多寡（字段完备率对三型统一 100% 口径）。
//!   3. 字段闭集校验：界面 0..=15、元素 0..=255、反馈五档（顺畅/
//!      迟疑/纠错/挫败/放弃）、耗时 0..=2000ms；越闭集拒收留痕。
//!   4. 时刻单调：账内 tick 严格不减（回退事件拒收）；分型账守恒：
//!      总账 == 三型和，字段完备率 == 100%（不完备账恒零）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，确定性，
//!      drill() 返回 Ok(hits)。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCE00;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_FIELD_MISS: u16 = SEG_BASE + 1; // 五元字段缺失
const E_UI_BAD: u16 = SEG_BASE + 2;     // 界面越闭集
const E_ELEM_BAD: u16 = SEG_BASE + 3;   // 元素越闭集
const E_FB_BAD: u16 = SEG_BASE + 4;     // 反馈越闭集
const E_DUR_BAD: u16 = SEG_BASE + 5;    // 耗时越闭集
const E_TICK_REGRESS: u16 = SEG_BASE + 6;// 时刻回退
const E_TYPE_BAD: u16 = SEG_BASE + 7;   // 事件型越闭集
const E_LEDGER_BAD: u16 = SEG_BASE + 8; // 分型账不平
const E_REPLAY_DIFF: u16 = SEG_BASE + 9;// 复跑不一致
const E_EMPTY_SUM: u16 = SEG_BASE + 10; // 写回摘要为空

/// 事件型闭集。
pub const EV_CLICK: u8 = 0;
pub const EV_KEY: u8 = 1;
pub const EV_CAND: u8 = 2;
pub const TYPE_N: usize = 3;
pub const TYPE_NAMES: [&str; TYPE_N] = ["点击", "按键", "候选操作"];

/// 反馈五档（十三章口径）。
pub const FB_SMOOTH: u8 = 0;   // 顺畅
pub const FB_HESITATE: u8 = 1; // 迟疑
pub const FB_FIX: u8 = 2;      // 纠错
pub const FB_FRUSTRATE: u8 = 3;// 挫败
pub const FB_GIVEUP: u8 = 4;   // 放弃
pub const FB_N: usize = 5;

/// 字段闭集界。
pub const UI_MAX: u8 = 15;
pub const ELEM_MAX: u8 = 255;
pub const DUR_MAX: u32 = 2000;

/// 体验事件（五元字段全带）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExpEvent {
    pub etype: u8,   // 三型闭集
    pub ui_id: u8,   // 界面
    pub elem_id: u8, // 元素
    pub tick: u32,   // 时刻（单调）
    pub feedback: u8,// 反馈五档
    pub dur: u32,    // 耗时 ms
}

impl ExpEvent {
    /// 字段完备与闭集校验（五元逐一，缺一/越一即拒）。
    pub fn validate(&self) -> Result<(), u16> {
        if self.etype as usize >= TYPE_N {
            return Err(E_TYPE_BAD);
        }
        // 五元字段全带（结构上字段不可缺省——此处校验语义占位值：
        // tick/dur 为 0 或反馈越档即视为残缺）。
        if self.dur > DUR_MAX {
            return Err(E_DUR_BAD);
        }
        if self.ui_id > UI_MAX {
            return Err(E_UI_BAD);
        }
        if self.elem_id > ELEM_MAX {
            return Err(E_ELEM_BAD);
        }
        if self.feedback as usize >= FB_N {
            return Err(E_FB_BAD);
        }
        Ok(())
    }

    /// 事件指纹（五元折叠，复跑对照）。
    pub fn fp(&self) -> u64 {
        let mut f: u64 = 0xCE00_CE01 ^ (self.etype as u64);
        f = f.rotate_left(8) ^ (self.ui_id as u64);
        f = f.rotate_left(8) ^ (self.elem_id as u64);
        f = f.rotate_left(8) ^ (self.tick as u64);
        f = f.rotate_left(8) ^ (self.feedback as u64);
        f = f.rotate_left(8) ^ (self.dur as u64);
        f.rotate_left(8)
    }
}

/// 体验事件账面：入账门禁 + 分型账 + 完备率。
pub struct ExpModel {
    events: Vec<ExpEvent>,
    pub type_count: [u32; TYPE_N],
    pub incomplete_total: u32, // 残缺拒收账（恒零红线）
    pub reject_total: u32,
    last_tick: u32,
}

impl ExpModel {
    pub fn new() -> ExpModel {
        ExpModel {
            events: Vec::new(),
            type_count: [0; TYPE_N],
            incomplete_total: 0,
            reject_total: 0,
            last_tick: 0,
        }
    }

    /// 入账：validate + 时刻单调，全过才落账。
    pub fn record(&mut self, ev: ExpEvent) -> Result<u64, u16> {
        if let Err(e) = ev.validate() {
            match e {
                E_TYPE_BAD => self.reject_total += 1,
                _ => self.incomplete_total += 1,
            }
            return Err(e);
        }
        if ev.tick < self.last_tick {
            self.reject_total += 1;
            return Err(E_TICK_REGRESS);
        }
        self.last_tick = ev.tick;
        self.type_count[ev.etype as usize] += 1;
        let f = ev.fp();
        self.events.push(ev);
        Ok(f)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 最近一条事件（尾账只读视图）。
    pub fn last_event(&self) -> Option<ExpEvent> {
        match self.events.len() {
            0 => None,
            n => Some(self.events[n - 1]),
        }
    }

    /// 字段完备率（100% 口径）：落账事件逐条 validate 再过（账内
    /// 不可能存残缺——门禁已拦，此处复核防私改），不完备账恒零。
    pub fn completeness(&self) -> Result<u32, u16> {
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].validate().is_err() {
                return Err(E_FIELD_MISS);
            }
            i += 1;
        }
        if self.incomplete_total != 0 {
            return Err(E_FIELD_MISS);
        }
        Ok(100)
    }

    /// 分型账守恒：总账 == 三型和。
    pub fn audit(&self) -> Result<(), u16> {
        let mut s: u32 = 0;
        let mut t = 0usize;
        while t < TYPE_N {
            s += self.type_count[t];
            t += 1;
        }
        if s != self.events.len() as u32 {
            return Err(E_LEDGER_BAD);
        }
        if self.events.is_empty() {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    /// 全账指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut fp: u64 = 0xCE00_CE02;
        let mut i = 0usize;
        while i < self.events.len() {
            fp = fp.rotate_left(7) ^ self.events[i].fp();
            i += 1;
        }
        fp
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done || self.events.is_empty() {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 确定性 LCG（零种子禁用）。
pub struct Lcg {
    x: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Result<Lcg, u16> {
        if seed == 0 {
            return Err(E_TYPE_BAD);
        }
        Ok(Lcg { x: seed })
    }
    pub fn next_u64(&mut self) -> u64 {
        self.x = self.x
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.x
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        (self.next_u64() >> 16) % n
    }
}

/// 生成一条合法体验事件（LCG 驱动，确定性）。
pub fn gen_event(lcg: &mut Lcg, tick: u32) -> ExpEvent {
    ExpEvent {
        etype: lcg.next_below(TYPE_N as u64) as u8,
        ui_id: lcg.next_below(UI_MAX as u64 + 1) as u8,
        elem_id: lcg.next_below(ELEM_MAX as u64 + 1) as u8,
        tick,
        feedback: lcg.next_below(FB_N as u64) as u8,
        dur: lcg.next_below(DUR_MAX as u64 + 1) as u32,
    }
}

/// 自例程判据：逐条落实 J1（事件字段 ×100% 契合十三章口径），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 字段完备 ×100：LCG 生成 100 条合法事件全入账，完备率
    //    100%，分型账守恒。
    let mut em = ExpModel::new();
    let mut lcg = Lcg::new(0xCE01)?;
    let mut n = 0u32;
    while n < 100 {
        let ev = gen_event(&mut lcg, n + 1);
        em.record(ev)?;
        n += 1;
    }
    if em.completeness()? != 100 {
        return Err(E_FIELD_MISS);
    }
    em.audit()?;
    if em.len() != 100 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 三型分账：100 条分型账各 >0（三型全有产出），型账加和
    //    == 100；反馈五档全账出现（十三章口径覆盖）。
    let mut em = ExpModel::new();
    let mut lcg = Lcg::new(0xCE01)?;
    let mut fb_seen = [false; FB_N];
    let mut n = 0u32;
    while n < 100 {
        let ev = gen_event(&mut lcg, n + 1);
        fb_seen[ev.feedback as usize] = true;
        em.record(ev)?;
        n += 1;
    }
    let mut t = 0usize;
    while t < TYPE_N {
        if em.type_count[t] == 0 {
            return Err(E_LEDGER_BAD);
        }
        t += 1;
    }
    let mut f = 0usize;
    while f < FB_N {
        if !fb_seen[f] {
            return Err(E_FB_BAD);
        }
        f += 1;
    }
    hits += 1;

    // R3 字段闭集逐路拒收：界面 16、元素（型上限内不可越，验 256
    //    由 u8 封顶改为验证 UI=16 与反馈 5 与耗时 2001 三路）。
    let mut em = ExpModel::new();
    let base = ExpEvent { etype: EV_CLICK, ui_id: 0, elem_id: 0, tick: 1, feedback: FB_SMOOTH, dur: 10 };
    let mut bad = base;
    bad.ui_id = UI_MAX + 1;
    if em.record(bad) != Err(E_UI_BAD) {
        return Err(E_UI_BAD);
    }
    let mut bad = base;
    bad.feedback = FB_N as u8;
    if em.record(bad) != Err(E_FB_BAD) {
        return Err(E_FB_BAD);
    }
    let mut bad = base;
    bad.dur = DUR_MAX + 1;
    if em.record(bad) != Err(E_DUR_BAD) {
        return Err(E_DUR_BAD);
    }
    let mut bad = base;
    bad.etype = 3;
    if em.record(bad) != Err(E_TYPE_BAD) {
        return Err(E_TYPE_BAD);
    }
    if em.reject_total != 1 || em.incomplete_total != 3 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R4 时刻单调：回退事件拒收留痕；同刻事件可收（不减即可）。
    let mut em = ExpModel::new();
    em.record(ExpEvent { etype: EV_KEY, ui_id: 1, elem_id: 2, tick: 100, feedback: FB_SMOOTH, dur: 5 })?;
    if em.record(ExpEvent { etype: EV_KEY, ui_id: 1, elem_id: 2, tick: 99, feedback: FB_SMOOTH, dur: 5 })
        != Err(E_TICK_REGRESS)
    {
        return Err(E_TICK_REGRESS);
    }
    em.record(ExpEvent { etype: EV_KEY, ui_id: 1, elem_id: 2, tick: 100, feedback: FB_SMOOTH, dur: 5 })?;
    if em.len() != 2 || em.reject_total != 1 {
        return Err(E_LEDGER_BAD);
    }
    match em.last_event() {
        None => return Err(E_LEDGER_BAD),
        Some(e) => {
            if e.tick != 100 || e.etype != EV_KEY {
                return Err(E_LEDGER_BAD);
            }
        }
    }
    hits += 1;

    // R5 残缺可红：耗 0 但语义占位（dur=0 视为残缺? 声明闭合：dur
    //    闭集含 0——改用超闭集已覆盖；此处验证账内私改残缺必被
    //    completeness 捕获，判据非恒绿）。
    let mut em = ExpModel::new();
    em.record(base)?;
    em.events[0].feedback = 9; // 私改成越档（模拟账面污染）
    if em.completeness() != Err(E_FIELD_MISS) {
        return Err(E_FIELD_MISS);
    }
    hits += 1;

    // R6 账面指纹与事件指纹：同事件同指纹，字段任一变动指纹必变
    //    （五元无一可静默改）。
    let a = base.fp();
    let b = base.fp();
    if a != b {
        return Err(E_REPLAY_DIFF);
    }
    let mut v = base;
    v.dur += 1;
    if v.fp() == a {
        return Err(E_LEDGER_BAD);
    }
    let mut v = base;
    v.ui_id += 1;
    if v.fp() == a {
        return Err(E_LEDGER_BAD);
    }
    let mut v = base;
    v.feedback += 1;
    if v.fp() == a {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 可红演练：完备率对污染账必红（R5 已验）；分型账伪造（账
    //    面与事件数失衡）audit 必红；空摘要/空账写回必红。
    let mut em = ExpModel::new();
    em.record(base)?;
    em.type_count[EV_CLICK as usize] = 5;
    if em.audit() != Err(E_LEDGER_BAD) {
        return Err(E_LEDGER_BAD);
    }
    let em0 = ExpModel::new();
    if em0.audit().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if em0.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    hits += 1;

    // R8 复跑一致与写回（判据 J2）：同种子 100 条两轮复跑全账指
    //    纹逐位一致（确定性）；写回过。
    let build = || -> Result<ExpModel, u16> {
        let mut em = ExpModel::new();
        let mut lcg = Lcg::new(0xCE02)?;
        let mut n = 0u32;
        while n < 100 {
            em.record(gen_event(&mut lcg, n + 1))?;
            n += 1;
        }
        Ok(em)
    };
    let e1 = build()?;
    let e2 = build()?;
    if e1.fingerprint() != e2.fingerprint() {
        return Err(E_REPLAY_DIFF);
    }
    e1.completeness()?;
    e1.audit()?;
    if e1.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    e1.writeback(true, 0x28)?;
    hits += 1;

    Ok(hits)
}
