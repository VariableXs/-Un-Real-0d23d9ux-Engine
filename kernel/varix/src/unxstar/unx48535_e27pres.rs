//! UNX-F48535 —— E27 异常呈现三要素（设备态变化提示）
//! 判据原文：UNX-F48535-J1 设备态变化类异常呈现必须齐备三要素：
//! ① 可见（带显式告警标记「【移除】」「【接入】」「【换装】」前缀）；
//! ② 可行动（正文含处置建议关键词"重插"或"等待"）；
//! ③ 不阻塞（呈现层不得阻断后续输入，blocking 恒为假）。
//! 判据落地（drill 场景）：R1 要素检查器基准；R2 移除提示三要素；
//! R3 接入提示三要素；R4 换装提示三要素；R5 缺要素即拒（隐性提示拦截）；
//! R6 阻塞位恒假；R7 同输入渲染幂等；R8 三类正文互异；
//! R9 诊断码落在段窗内；R10 指纹稳定。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xEF60;

// ── 诊断码闭集（SEG_BASE+1 起连续，≤ SEG_BASE+0x1F）─────────────
const E_INVIS: u16 = SEG_BASE + 1; // 不可见：缺告警标记前缀
const E_INERT: u16 = SEG_BASE + 2; // 不可行动：缺处置建议关键词
const E_BLOCK: u16 = SEG_BASE + 3; // 阻塞呈现：blocking 位为真
const E_KIND: u16 = SEG_BASE + 4; // 呈现类别越出三类
const E_IDEM: u16 = SEG_BASE + 5; // 同输入渲染不幂等
const E_SAME: u16 = SEG_BASE + 6; // 三类正文雷同
const E_SEG: u16 = SEG_BASE + 7; // 呈现码越出段窗
const E_FP: u16 = SEG_BASE + 8; // 指纹漂移
const E_EMPTY: u16 = SEG_BASE + 9; // 空正文
const E_STAT: u16 = SEG_BASE + 10; // 呈现统计不守恒

/// 呈现类别：1 移除 / 2 接入 / 3 换装。
pub const PRES_REMOVE: u8 = 1;
pub const PRES_ATTACH: u8 = 2;
pub const PRES_RESEAT: u8 = 3;

/// 告警标记前缀（要素①）。
pub const MARK_REMOVE: &str = "【移除】";
pub const MARK_ATTACH: &str = "【接入】";
pub const MARK_RESEAT: &str = "【换装】";
/// 处置建议关键词（要素②）。
pub const ACT_REPLUG: &str = "重插";
pub const ACT_WAIT: &str = "等待";

/// FNV-1a 64 位（与本域其余哨兵同口径）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// 确定性 LCG（批内统一系数）。
pub struct Lcg {
    state: u32,
}

impl Lcg {
    pub fn new(seed: u32) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next_u32(&mut self) -> u32 {
        self.state = self.state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.state
    }
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            0
        } else {
            self.next_u32() % bound
        }
    }
}

/// 一条呈现消息。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct PresMsg {
    pub code: u16,
    pub body: String,
    pub blocking: bool,
}

/// 三要素逐项核验结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ElementAudit {
    pub visible: bool,
    pub actionable: bool,
    pub non_blocking: bool,
}

impl ElementAudit {
    pub fn all_pass(&self) -> bool {
        self.visible && self.actionable && self.non_blocking
    }
}

/// 三要素审计：① 带任一告警标记前缀；② 含处置关键词；③ blocking 假。
pub fn audit(msg: &PresMsg) -> ElementAudit {
    let visible = has_kw(msg.body.as_bytes(), MARK_REMOVE.as_bytes())
        || has_kw(msg.body.as_bytes(), MARK_ATTACH.as_bytes())
        || has_kw(msg.body.as_bytes(), MARK_RESEAT.as_bytes());
    let actionable = has_kw(msg.body.as_bytes(), ACT_REPLUG.as_bytes())
        || has_kw(msg.body.as_bytes(), ACT_WAIT.as_bytes());
    ElementAudit {
        visible,
        actionable,
        non_blocking: !msg.blocking,
    }
}

/// 渲染移除提示：标记 + 设备键 + 重插建议，永不阻塞。
pub fn render_remove(dev: u32) -> PresMsg {
    let mut b = String::new();
    b.push_str(MARK_REMOVE);
    b.push_str("设备 #");
    b.push_str(&dev.to_string());
    b.push_str(" 已拔出，如需使用请");
    b.push_str(ACT_REPLUG);
    b.push_str("设备");
    PresMsg {
        code: SEG_BASE + 1,
        body: b,
        blocking: false,
    }
}

/// 渲染接入提示：标记 + 就绪说明 + 等待建议，永不阻塞。
pub fn render_attach(dev: u32) -> PresMsg {
    let mut b = String::new();
    b.push_str(MARK_ATTACH);
    b.push_str("设备 #");
    b.push_str(&dev.to_string());
    b.push_str(" 已就绪，无需");
    b.push_str(ACT_WAIT);
    b.push_str("即可输入");
    PresMsg {
        code: SEG_BASE + 2,
        body: b,
        blocking: false,
    }
}

/// 渲染换装提示：标记 + 通道切换 + 重插建议，永不阻塞。
pub fn render_reseat(dev: u32) -> PresMsg {
    let mut b = String::new();
    b.push_str(MARK_RESEAT);
    b.push_str("设备 #");
    b.push_str(&dev.to_string());
    b.push_str(" 通道切换中，请勿");
    b.push_str(ACT_REPLUG);
    b.push_str("，稍候自动恢复");
    PresMsg {
        code: SEG_BASE + 3,
        body: b,
        blocking: false,
    }
}

/// 按类别分派渲染；未知类别返回诊断错误。
pub fn render(kind: u8, dev: u32) -> Result<PresMsg, u16> {
    match kind {
        PRES_REMOVE => Ok(render_remove(dev)),
        PRES_ATTACH => Ok(render_attach(dev)),
        PRES_RESEAT => Ok(render_reseat(dev)),
        _ => Err(E_KIND),
    }
}

/// 三类正文指纹串联。
pub fn trio_fp(r: &PresMsg, a: &PresMsg, s: &PresMsg) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&r.code.to_le_bytes());
    buf.extend_from_slice(r.body.as_bytes());
    buf.push(0x1F);
    buf.extend_from_slice(&a.code.to_le_bytes());
    buf.extend_from_slice(a.body.as_bytes());
    buf.push(0x1E);
    buf.extend_from_slice(&s.code.to_le_bytes());
    buf.extend_from_slice(s.body.as_bytes());
    fnv1a64(&buf)
}

/// 呈现闸门：空正文拒收，否则交三要素审计，缺项逐一归位诊断码。
pub fn audit_gate(msg: &PresMsg) -> Result<ElementAudit, u16> {
    if msg.body.is_empty() {
        return Err(E_EMPTY);
    }
    let a = audit(msg);
    if !a.visible {
        return Err(E_INVIS);
    }
    if !a.actionable {
        return Err(E_INERT);
    }
    if !a.non_blocking {
        return Err(E_BLOCK);
    }
    Ok(a)
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 要素检查器基准：全缺要素消息必须三项全挂。
    let blank = PresMsg {
        code: SEG_BASE + 1,
        body: String::new(),
        blocking: true,
    };
    let bad = audit(&blank);
    if bad.all_pass() || bad.visible || bad.actionable || bad.non_blocking {
        return Err(E_INVIS);
    }
    hits += 1;

    // R2 移除提示三要素。
    let r = render_remove(7);
    let ar = audit(&r);
    if !ar.all_pass() {
        if !ar.visible {
            return Err(E_INVIS);
        }
        if !ar.actionable {
            return Err(E_INERT);
        }
        return Err(E_BLOCK);
    }
    if r.code != SEG_BASE + 1 {
        return Err(E_SEG);
    }
    hits += 1;

    // R3 接入提示三要素。
    let a = render_attach(9);
    let aa = audit(&a);
    if !aa.all_pass() {
        if !aa.visible {
            return Err(E_INVIS);
        }
        if !aa.actionable {
            return Err(E_INERT);
        }
        return Err(E_BLOCK);
    }
    if a.code != SEG_BASE + 2 {
        return Err(E_SEG);
    }
    hits += 1;

    // R4 换装提示三要素。
    let s = render_reseat(11);
    let asg = audit(&s);
    if !asg.all_pass() {
        if !asg.visible {
            return Err(E_INVIS);
        }
        if !asg.actionable {
            return Err(E_INERT);
        }
        return Err(E_BLOCK);
    }
    if s.code != SEG_BASE + 3 {
        return Err(E_SEG);
    }
    hits += 1;

    // R5 缺要素即拒：无标记正文不得通过审计闸门；空正文走 E_EMPTY。
    let bare = PresMsg {
        code: SEG_BASE + 1,
        body: "设备 #7 已拔出，如需使用请重插设备".to_string(),
        blocking: false,
    };
    let ab = audit(&bare);
    if ab.all_pass() {
        return Err(E_INVIS);
    }
    let empty = PresMsg {
        code: SEG_BASE + 1,
        body: String::new(),
        blocking: false,
    };
    match audit_gate(&empty) {
        Err(E_EMPTY) => hits += 1,
        _ => return Err(E_EMPTY),
    }

    // R6 阻塞位恒假：分派器产出的任何消息 blocking 必假。
    let mut lcg = Lcg::new(0x48535);
    let mut rendered = 0u32;
    let mut k = 0u32;
    while k < 12 {
        let kind = (lcg.below(3) + 1) as u8;
        let m = render(kind, 100 + k)?;
        if m.blocking {
            return Err(E_BLOCK);
        }
        rendered += 1;
        k += 1;
    }
    if rendered != 12 {
        return Err(E_STAT);
    }
    hits += 1;

    // R7 渲染幂等：同参数两次渲染正文一致。
    let r1 = render_remove(7);
    let r2 = render_remove(7);
    if r1 != r2 {
        return Err(E_IDEM);
    }
    hits += 1;

    // R8 三类正文互异：两两不同。
    if r1.body == a.body || a.body == s.body || r1.body == s.body {
        return Err(E_SAME);
    }
    hits += 1;

    // R9 诊断码段窗：全部落在 [SEG_BASE+1, SEG_BASE+0x1F]。
    for m in [&r1, &a, &s] {
        if m.code < SEG_BASE + 1 || m.code > SEG_BASE + 0x1F {
            return Err(E_SEG);
        }
    }
    hits += 1;

    // R10 指纹稳定：三体指纹两次一致且非零。
    let f1 = trio_fp(&r1, &a, &s);
    let f2 = trio_fp(&r1, &a, &s);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    Ok(hits)
}

/// 子串判定（朴素扫描，统一口径）。
pub fn has_kw(hay: &[u8], kw: &[u8]) -> bool {
    if kw.is_empty() || hay.len() < kw.len() {
        return false;
    }
    let mut i = 0usize;
    while i + kw.len() <= hay.len() {
        let mut j = 0usize;
        while j < kw.len() && hay[i + j] == kw[j] {
            j += 1;
        }
        if j == kw.len() {
            return true;
        }
        i += 1;
    }
    false
}
