//! UNX-F48438 —— E22 异常呈现三要素（截断告警用户可见）
//! 判据原文：UNX-F48438-J1 截断/降级/越界类异常呈现必须齐备三要素：
//! ① 可见（带显式告警标记「【超长】」或「【降级】」前缀）；
//! ② 可行动（正文含处置建议关键词"拆分"或"兜底"）；
//! ③ 不阻塞（呈现层不得阻断继续输入，blocking 恒为假）。
//! 判据落地（drill 场景）：R1 要素检查器基准；R2 截断告警三要素；
//! R3 降级告警三要素；R4 越界告警三要素；R5 缺要素即拒（隐性告警拦截）；
//! R6 阻塞位恒假；R7 同输入渲染幂等；R8 三类正文互异；
//! R9 诊断码落在段窗内；R10 指纹稳定。
//! 设计纪律：no_std；零 unwrap/expect/panic；确定性 LCG；drill 判据全绿才返回 Ok。
use crate::alloc::vec::Vec;
use crate::alloc::vec;
use alloc::string::{String, ToString};

/// 诊断段基址（独立 0x20 窗）。
pub const SEG_BASE: u16 = 0xDAE0;

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

/// 呈现类别：1 截断 / 2 降级 / 3 越界。
pub const PRES_TRUNC: u8 = 1;
pub const PRES_DEGRADE: u8 = 2;
pub const PRES_OOB: u8 = 3;

/// 告警标记前缀（要素①）。
pub const MARK_TRUNC: &str = "【超长】";
pub const MARK_DEGRADE: &str = "【降级】";
pub const MARK_OOB: &str = "【越界】";
/// 处置建议关键词（要素②）。
pub const ACT_SPLIT: &str = "拆分";
pub const ACT_FALLBACK: &str = "兜底";

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
    let visible = has_kw(msg.body.as_bytes(), MARK_TRUNC.as_bytes())
        || has_kw(msg.body.as_bytes(), MARK_DEGRADE.as_bytes())
        || has_kw(msg.body.as_bytes(), MARK_OOB.as_bytes());
    let actionable = has_kw(msg.body.as_bytes(), ACT_SPLIT.as_bytes())
        || has_kw(msg.body.as_bytes(), ACT_FALLBACK.as_bytes());
    ElementAudit {
        visible,
        actionable,
        non_blocking: !msg.blocking,
    }
}

/// 渲染截断告警：标记 + 计数 + 拆分建议，永不阻塞。
pub fn render_trunc(shown: usize, total: usize) -> PresMsg {
    let mut b = String::new();
    b.push_str(MARK_TRUNC);
    b.push_str("已呈现 ");
    b.push_str(&(shown as u32).to_string());
    b.push_str("/");
    b.push_str(&(total as u32).to_string());
    b.push_str("，请");
    b.push_str(ACT_SPLIT);
    b.push_str("后再输入");
    PresMsg {
        code: SEG_BASE + 1,
        body: b,
        blocking: false,
    }
}

/// 渲染降级告警：标记 + 兜底说明，永不阻塞。
pub fn render_degrade() -> PresMsg {
    let mut b = String::new();
    b.push_str(MARK_DEGRADE);
    b.push_str("词库异常，已切换精简库");
    b.push_str(ACT_FALLBACK);
    b.push_str("，输入不受影响");
    PresMsg {
        code: SEG_BASE + 2,
        body: b,
        blocking: false,
    }
}

/// 渲染越界告警：标记 + 夹取说明 + 拆分建议，永不阻塞。
pub fn render_oob(pos: usize) -> PresMsg {
    let mut b = String::new();
    b.push_str(MARK_OOB);
    b.push_str("光标位 ");
    b.push_str(&(pos as u32).to_string());
    b.push_str(" 已夹取，请");
    b.push_str(ACT_SPLIT);
    b.push_str("长句");
    PresMsg {
        code: SEG_BASE + 3,
        body: b,
        blocking: false,
    }
}

/// 按类别分派渲染；未知类别返回诊断错误。
pub fn render(kind: u8, a: usize, b: usize) -> Result<PresMsg, u16> {
    match kind {
        PRES_TRUNC => Ok(render_trunc(a, b)),
        PRES_DEGRADE => Ok(render_degrade()),
        PRES_OOB => Ok(render_oob(a)),
        _ => Err(E_KIND),
    }
}

/// 三类正文指纹串联。
pub fn trio_fp(t: &PresMsg, d: &PresMsg, o: &PresMsg) -> u64 {
    let mut buf: Vec<u8> = vec![];
    buf.extend_from_slice(&t.code.to_le_bytes());
    buf.extend_from_slice(t.body.as_bytes());
    buf.push(0x1F);
    buf.extend_from_slice(&d.code.to_le_bytes());
    buf.extend_from_slice(d.body.as_bytes());
    buf.push(0x1E);
    buf.extend_from_slice(&o.code.to_le_bytes());
    buf.extend_from_slice(o.body.as_bytes());
    fnv1a64(&buf)
}

/// 主巡检入口：R1..R10 全绿才返回 Ok(hits)。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 要素检查器基准：构造全缺要素消息必须三项全挂。
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

    // R2 截断告警三要素。
    let t = render_trunc(120, 200);
    let at = audit(&t);
    if !at.all_pass() {
        if !at.visible {
            return Err(E_INVIS);
        }
        if !at.actionable {
            return Err(E_INERT);
        }
        return Err(E_BLOCK);
    }
    if t.code != SEG_BASE + 1 {
        return Err(E_SEG);
    }
    hits += 1;

    // R3 降级告警三要素。
    let d = render_degrade();
    let ad = audit(&d);
    if !ad.all_pass() {
        if !ad.visible {
            return Err(E_INVIS);
        }
        if !ad.actionable {
            return Err(E_INERT);
        }
        return Err(E_BLOCK);
    }
    if d.code != SEG_BASE + 2 {
        return Err(E_SEG);
    }
    hits += 1;

    // R4 越界告警三要素。
    let o = render_oob(333);
    let ao = audit(&o);
    if !ao.all_pass() {
        if !ao.visible {
            return Err(E_INVIS);
        }
        if !ao.actionable {
            return Err(E_INERT);
        }
        return Err(E_BLOCK);
    }
    if o.code != SEG_BASE + 3 {
        return Err(E_SEG);
    }
    hits += 1;

    // R5 缺要素即拒：空正文/无标记正文不得通过审计闸门。
    let bare = PresMsg {
        code: SEG_BASE + 1,
        body: "已呈现 120/200，请拆分后再输入".to_string(),
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
    let mut lcg = Lcg::new(0x48438);
    let mut rendered = 0u32;
    let mut k = 0u32;
    while k < 12 {
        let kind = (lcg.below(3) + 1) as u8;
        let m = render(kind, 10 + k as usize, 200)?;
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
    let t1 = render_trunc(120, 200);
    let t2 = render_trunc(120, 200);
    if t1 != t2 {
        return Err(E_IDEM);
    }
    hits += 1;

    // R8 三类正文互异：两两不同。
    if t1.body == d.body || d.body == o.body || t1.body == o.body {
        return Err(E_SAME);
    }
    hits += 1;

    // R9 诊断码段窗：全部落在 [SEG_BASE+1, SEG_BASE+0x1F]。
    for m in [&t1, &d, &o] {
        if m.code < SEG_BASE + 1 || m.code > SEG_BASE + 0x1F {
            return Err(E_SEG);
        }
    }
    hits += 1;

    // R10 指纹稳定：三体指纹两次一致且非零。
    let f1 = trio_fp(&t1, &d, &o);
    let f2 = trio_fp(&t1, &d, &o);
    if f1 != f2 || f1 == 0 {
        return Err(E_FP);
    }
    hits += 1;

    Ok(hits)
}

/// 呈现闸门：空正文拒收（E_EMPTY），否则交三要素审计，缺项逐一归位诊断码。
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
