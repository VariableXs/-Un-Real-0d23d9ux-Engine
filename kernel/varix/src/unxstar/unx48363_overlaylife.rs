//! unx48363_overlaylife — UNX-F48363 浮层生命线记录（候选窗开关全周期：谁开/怎么关/期间动作）（诊断段 0xCE40）。
//!
//! 判据原文：
//!   J1 生命线 ×100 完整可回放；判据锚定 体验十三。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 全周期三段式：每条浮层生命线必须完整覆盖 Open（谁开）→ Act*（期间
//!      动作）→ Close（怎么关）三段；缺开启、缺关闭、缺归因都算残线，残线
//!      不得入账。
//!   2. ×100 完整可回放：账内 100 条生命线全部能由原始事件流按 seq 升序
//!      确定性重演重建，重演结果与原账逐字段一致——「可回放」不是口号，
//!      是重演器逐字段比对得出的结论。
//!   3. 归因不许缺位：Open 必须携带开启者归因（输入引擎/UI 壳/应用宿主），
//!      Close 必须携带关闭方式归因（上屏/Esc/失焦/超时），匿名事件一律
//!      拒收并给出独立诊断码。
//!   4. 期间动作只许落在开与关之间：开之前的动作与关之后的动作都是账目
//!      违规（孤儿动作），不是正常事件。
//!   5. 账 append-only：同一事件流重演两次，账指纹必须相同；对注入的
//!      「关前先关」「未开先动」等篡改必须能检出（假阴性为零）。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xCE40;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_NO_OPEN: u16 = SEG_BASE + 1;       // 无开先关：关闭事件找不到在开浮层
const E_DOUBLE_OPEN: u16 = SEG_BASE + 2;   // 同一浮层重复开启
const E_ORPHAN_ACTION: u16 = SEG_BASE + 3; // 期间动作落在开合区间之外
const E_UNCLOSED: u16 = SEG_BASE + 4;      // 事件流耗尽仍有浮层未关闭
const E_MISSING_OWNER: u16 = SEG_BASE + 5; // 开启者归因缺位
const E_MISSING_CLOSER: u16 = SEG_BASE + 6; // 关闭方式归因缺位
const E_REPLAY_DRIFT: u16 = SEG_BASE + 7;  // 重演账与原账指纹不一致
const E_TAMPER_UNDETECTED: u16 = SEG_BASE + 8; // 注入违规未被检出
const E_LIFELINE_SHORT: u16 = SEG_BASE + 9; // 生命线不足 100 条
const E_FIELD_MISMATCH: u16 = SEG_BASE + 10; // 重演字段与生成意图不一致
const E_CODE_RANGE: u16 = SEG_BASE + 11;   // 诊断码越段

/// 浮层种类：候选窗 / 预编辑条 / 悬浮提示。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum OverlayKind {
    Candidate,
    Preedit,
    Hint,
}

/// 开启者归因（谁开）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// 输入引擎自行拉起。
    InputEngine,
    /// UI 壳按会话状态拉起。
    UiShell,
    /// 应用宿主显式请求拉起。
    AppHost,
}

/// 关闭方式归因（怎么关）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CloseHow {
    ByCommit,
    ByEsc,
    ByFocusLoss,
    ByTimeout,
}

/// 生命周期事件种类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Open,
    Act,
    Close,
}

/// 浮层事件：overlay_id 在其生命线内唯一，seq 全局单调。
#[derive(Clone, Copy)]
pub struct OverlayEvent {
    pub seq: u32,
    pub overlay_id: u16,
    pub kind: OverlayKind,
    pub op: Op,
    /// 仅 Open 必填：谁开。
    pub owner: Option<Owner>,
    /// 仅 Close 必填：怎么关。
    pub how: Option<CloseHow>,
    /// Act 动作码（翻页/高亮迁移/筛选等），Open/Close 恒为 0。
    pub act_code: u16,
}

/// 单条浮层生命线：开 → 动作串 → 关，全字段闭合。
pub struct Lifeline {
    pub overlay_id: u16,
    pub kind: OverlayKind,
    pub opener: Owner,
    pub closer: CloseHow,
    pub actions: Vec<u16>,
    pub open_seq: u32,
    pub close_seq: u32,
    pub closed: bool,
}

/// 生命线重演器：按 seq 升序消费事件流，重建生命线账。
pub struct Replayer {
    lines: Vec<Lifeline>,
}

impl Replayer {
    pub fn new() -> Self {
        Replayer {
            lines: Vec::new(),
        }
    }

    /// 消费一个事件；任何账目违规都以诊断码拒绝。
    pub fn consume(&mut self, e: &OverlayEvent) -> Result<(), u16> {
        match e.op {
            Op::Open => {
                if e.owner.is_none() {
                    return Err(E_MISSING_OWNER);
                }
                let mut i = 0usize;
                while i < self.lines.len() {
                    if self.lines[i].overlay_id == e.overlay_id && !self.lines[i].closed {
                        return Err(E_DOUBLE_OPEN);
                    }
                    i += 1;
                }
                self.lines.push(Lifeline {
                    overlay_id: e.overlay_id,
                    kind: e.kind,
                    opener: e.owner.unwrap_or(Owner::InputEngine),
                    closer: CloseHow::ByCommit,
                    actions: Vec::new(),
                    open_seq: e.seq,
                    close_seq: 0,
                    closed: false,
                });
                Ok(())
            }
            Op::Act => {
                // 只许落在最近一条同 id 在开生命线内。
                let mut target: Option<usize> = None;
                let mut i = 0usize;
                while i < self.lines.len() {
                    if self.lines[i].overlay_id == e.overlay_id && !self.lines[i].closed {
                        target = Some(i);
                    }
                    i += 1;
                }
                match target {
                    Some(t) => {
                        self.lines[t].actions.push(e.act_code);
                        Ok(())
                    }
                    None => Err(E_ORPHAN_ACTION),
                }
            }
            Op::Close => {
                if e.how.is_none() {
                    return Err(E_MISSING_CLOSER);
                }
                let mut target: Option<usize> = None;
                let mut i = 0usize;
                while i < self.lines.len() {
                    if self.lines[i].overlay_id == e.overlay_id && !self.lines[i].closed {
                        target = Some(i);
                    }
                    i += 1;
                }
                match target {
                    Some(t) => {
                        self.lines[t].closed = true;
                        self.lines[t].closer = e.how.unwrap_or(CloseHow::ByCommit);
                        self.lines[t].close_seq = e.seq;
                        Ok(())
                    }
                    None => Err(E_NO_OPEN),
                }
            }
        }
    }

    /// 收账：事件流耗尽后仍存在未闭合生命线 = 残线。
    pub fn finish(&self) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.lines.len() {
            if !self.lines[i].closed {
                return Err(E_UNCLOSED);
            }
            i += 1;
        }
        Ok(())
    }

    pub fn lines(&self) -> &[Lifeline] {
        &self.lines
    }
}

/// 账指纹：FNV-1a 折叠全部生命线字段（重演一致性的整账判据）。
pub fn fingerprint(lines: &[Lifeline]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0usize;
    while i < lines.len() {
        let l = &lines[i];
        h ^= l.overlay_id as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= l.kind as u8 as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= l.opener as u8 as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= l.closer as u8 as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= l.actions.len() as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        let mut j = 0usize;
        while j < l.actions.len() {
            h ^= l.actions[j] as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
            j += 1;
        }
        h ^= l.open_seq as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        h ^= l.close_seq as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 确定性线性同余发生器（剧本生成统一入口，禁真随机）。
pub struct Lcg(u32);

impl Lcg {
    pub fn new(seed: u32) -> Self {
        Lcg(seed | 1)
    }
    pub fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0
    }
}

/// 生命周期剧本：确定性生成 ×100 条完整生命线的事件流。
/// 返回（事件流，逐线的开启者/关闭方式意图，供字段级比对）。
pub fn script(seed: u32) -> (Vec<OverlayEvent>, Vec<(Owner, CloseHow, OverlayKind)>) {
    let mut lc = Lcg::new(seed);
    let mut evs: Vec<OverlayEvent> = Vec::new();
    let mut intent: Vec<(Owner, CloseHow, OverlayKind)> = Vec::new();
    let mut seq: u32 = 0;
    let mut id: u16 = 0;
    while id < 100 {
        let kind = match lc.next() % 3 {
            0 => OverlayKind::Candidate,
            1 => OverlayKind::Preedit,
            _ => OverlayKind::Hint,
        };
        let owner = match lc.next() % 3 {
            0 => Owner::InputEngine,
            1 => Owner::UiShell,
            _ => Owner::AppHost,
        };
        let how = match lc.next() % 4 {
            0 => CloseHow::ByCommit,
            1 => CloseHow::ByEsc,
            2 => CloseHow::ByFocusLoss,
            _ => CloseHow::ByTimeout,
        };
        let n_act = (lc.next() % 5) as u16; // 0..4 个期间动作
        evs.push(OverlayEvent {
            seq,
            overlay_id: id,
            kind,
            op: Op::Open,
            owner: Some(owner),
            how: None,
            act_code: 0,
        });
        seq += 1;
        let mut a = 0u16;
        while a < n_act {
            evs.push(OverlayEvent {
                seq,
                overlay_id: id,
                kind,
                op: Op::Act,
                owner: None,
                how: None,
                act_code: 0x1000 + (lc.next() % 64) as u16,
            });
            seq += 1;
            a += 1;
        }
        evs.push(OverlayEvent {
            seq,
            overlay_id: id,
            kind,
            op: Op::Close,
            owner: None,
            how: Some(how),
            act_code: 0,
        });
        seq += 1;
        intent.push((owner, how, kind));
        id += 1;
    }
    (evs, intent)
}

/// 便捷封装：重演整段事件流并收账。
pub fn replay(evs: &[OverlayEvent]) -> Result<Vec<Lifeline>, u16> {
    let mut rp = Replayer::new();
    let mut i = 0usize;
    while i < evs.len() {
        rp.consume(&evs[i])?;
        i += 1;
    }
    rp.finish()?;
    let mut out: Vec<Lifeline> = Vec::new();
    let mut i = 0usize;
    while i < rp.lines().len() {
        let src = &rp.lines()[i];
        let mut acts: Vec<u16> = Vec::new();
        let mut j = 0usize;
        while j < src.actions.len() {
            acts.push(src.actions[j]);
            j += 1;
        }
        out.push(Lifeline {
            overlay_id: src.overlay_id,
            kind: src.kind,
            opener: src.opener,
            closer: src.closer,
            actions: acts,
            open_seq: src.open_seq,
            close_seq: src.close_seq,
            closed: src.closed,
        });
        i += 1;
    }
    Ok(out)
}

/// 自例程判据：逐条落实 J1 的全部断言（生命线 ×100 完整可回放）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let (evs, intent) = script(0x4836_3001);

    // R1 ×100 完整性：重演恰好重建 100 条闭合生命线，id 0..=99 各一次。
    let lines = replay(&evs)?;
    if lines.len() != 100 {
        return Err(E_LIFELINE_SHORT);
    }
    let mut i = 0usize;
    while i < lines.len() {
        if lines[i].overlay_id != i as u16 || !lines[i].closed {
            return Err(E_LIFELINE_SHORT);
        }
        i += 1;
    }
    hits += 1;

    // R2 归因完整性：每条线的开启者/关闭方式与生成意图逐字段一致。
    i = 0;
    while i < lines.len() {
        let (ow, hw, kd) = intent[i];
        if lines[i].opener != ow || lines[i].closer != hw || lines[i].kind != kd {
            return Err(E_FIELD_MISMATCH);
        }
        i += 1;
    }
    hits += 1;

    // R3 开合区间：close_seq > open_seq，且 close_seq - open_seq = 事件数 + 1。
    i = 0;
    while i < lines.len() {
        if lines[i].close_seq <= lines[i].open_seq {
            return Err(E_ORPHAN_ACTION);
        }
        let span = lines[i].close_seq - lines[i].open_seq;
        if span != lines[i].actions.len() as u32 + 1 {
            return Err(E_ORPHAN_ACTION);
        }
        i += 1;
    }
    hits += 1;

    // R4 可回放幂等：同一事件流重演两次，整账指纹必须相同。
    let fp1 = fingerprint(&lines);
    let lines2 = replay(&evs)?;
    if fingerprint(&lines2) != fp1 {
        return Err(E_REPLAY_DRIFT);
    }
    hits += 1;

    // R5 假阴性自查·孤儿动作：未开先动必须被重演器拒收。
    let mut bad: Vec<OverlayEvent> = Vec::new();
    bad.push(OverlayEvent {
        seq: 0,
        overlay_id: 7,
        kind: OverlayKind::Candidate,
        op: Op::Act,
        owner: None,
        how: None,
        act_code: 0x1001,
    });
    let mut j = 0usize;
    while j < evs.len() {
        let mut e = evs[j];
        e.seq = (j as u32) + 1;
        bad.push(e);
        j += 1;
    }
    let mut rp = Replayer::new();
    let mut caught = false;
    j = 0;
    while j < bad.len() {
        if rp.consume(&bad[j]).is_err() {
            caught = true;
            break;
        }
        j += 1;
    }
    if !caught {
        return Err(E_TAMPER_UNDETECTED);
    }
    hits += 1;

    // R6 假阴性自查·无开先关：对在开浮层之外的 id 下关闭令必须被拒收。
    let mut rp2 = Replayer::new();
    let ghost = OverlayEvent {
        seq: 0,
        overlay_id: 999,
        kind: OverlayKind::Hint,
        op: Op::Close,
        owner: None,
        how: Some(CloseHow::ByEsc),
        act_code: 0,
    };
    if rp2.consume(&ghost).is_ok() {
        return Err(E_TAMPER_UNDETECTED);
    }
    hits += 1;

    // R7 残线自查：抽掉末尾的 Close 事件，收账必须报未闭合。
    let cut = evs.len() - 1;
    let mut rp3 = Replayer::new();
    let mut j = 0usize;
    let mut open_left = false;
    while j < cut {
        // 只重演到倒数第二个事件，最后一条生命线必然未闭合。
        let _ = rp3.consume(&evs[j]);
        j += 1;
        open_left = j == cut;
    }
    if open_left && rp3.finish().is_ok() {
        return Err(E_TAMPER_UNDETECTED);
    }
    hits += 1;

    // R8 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 11] = [
        E_NO_OPEN,
        E_DOUBLE_OPEN,
        E_ORPHAN_ACTION,
        E_UNCLOSED,
        E_MISSING_OWNER,
        E_MISSING_CLOSER,
        E_REPLAY_DRIFT,
        E_TAMPER_UNDETECTED,
        E_LIFELINE_SHORT,
        E_FIELD_MISMATCH,
        E_CODE_RANGE,
    ];
    let mut ci = 0usize;
    while ci < CODES.len() {
        if CODES[ci] <= SEG_BASE || CODES[ci] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    Ok(hits)
}
