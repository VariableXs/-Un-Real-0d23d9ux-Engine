//! unx48204_gaplist — UNX-F48204 X11 老应用缺失项清单账（对照 L2 如实并列缺失项）（诊断段 0xBA60）。
//!
//! 判据原文：
//!   J1 清单入册零编造；判据锚定 §AI-61 §2 专题三类型三、§40 幻觉红线。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 「零编造」红线：清单条目必须有来源标签（L2 实测探针回执 /
//!      协议层能力探测回执），无来源的条目一律拒收——宁缺勿编。
//!   2. 缺失项闭集：缺失按能力域闭集登记（caret 探测/字体会话/预编辑
//!      提交/候选窗坐标/状态回写 五域），域外条目拒收。
//!   3. 「缺失」与「未测」严格二分：未测只能进 pending，不许冒充缺失
//!      （幻觉红线：把没测的写成缺的 = 编造）。
//!   4. 幂等重入：同一 (应用指纹， 能力域) 重复登记只更新证据链，不重记。
//!   5. 清单冻结后指纹封账；解冻重扫必须升版（v+1），旧版留档可追溯。
//!   6. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;
use crate::alloc::string::String;

pub const SEG_BASE: u16 = 0xBA60;

const E_NO_SOURCE: u16 = SEG_BASE + 1;  // 无来源条目拒收（零编造）
const E_BAD_DOMAIN: u16 = SEG_BASE + 2; // 能力域闭集外
const E_NOT_MEASURED: u16 = SEG_BASE + 3; // 未测冒充缺失
const E_DUP_CONFLICT: u16 = SEG_BASE + 4; // 幂等重入但证据冲突
const E_FROZEN: u16 = SEG_BASE + 5;     // 冻结后写拒
const E_BAD_FP: u16 = SEG_BASE + 6;     // 应用指纹格式坏
const E_SEAL_MISMATCH: u16 = SEG_BASE + 7; // 封账指纹不符
const E_UNFREEZE_STALE: u16 = SEG_BASE + 8; // 解冻未升版

/// 能力域闭集（五域）：老应用 IME 能力盘点口径。
pub const DOM_CARET: u8 = 1;     // caret 探测（光标坐标能力）
pub const DOM_FONTSET: u8 = 2;   // 字体会话（字体列表/度量回执）
pub const DOM_PREEDIT: u8 = 3;   // 预编辑提交（preedit 回显能力）
pub const DOM_CANDWIN: u8 = 4;   // 候选窗坐标（spot 坐标供给能力）
pub const DOM_STATUS: u8 = 5;    // 状态回写（转换状态回读能力）

/// 条目状态：缺失（有证据）/ 未测（pending）/ 具备（有反证）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemState {
    Missing,
    Pending,
    Present,
}

/// 来源标签闭集：无来源即拒。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    L2Probe,     // L2 实测探针回执
    ProtoProbe,  // 协议层能力探测回执（XGetWindowAttributes 等）
    ReAudit,     // 复扫升级回执（解冻升版后重新认定）
}

/// 应用指纹：窗口类名哈希 + 实例名哈希 双列（u32×2，确定性 FNV）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppFp {
    pub cls: u32,
    pub inst: u32,
}

/// 一条缺失项账。
#[derive(Debug, Clone)]
pub struct GapItem {
    pub fp: AppFp,
    pub domain: u8,
    pub state: ItemState,
    pub src: Option<Source>, // 证据来源（Pending 无来源=None）
    pub note: String,   // 证据摘要（如实抄录，不扩写）
    pub ver: u32,       // 登记时清单版本
}

/// 清单总账（版本化 + 冻结 + 指纹封账）。
#[derive(Debug)]
pub struct GapList {
    ver: u32,
    frozen: bool,
    seal_fp: u32,
    items: Vec<GapItem>,
    reject_log: Vec<(AppFp, u8, u16)>, // 拒收账（零编造红线执行记录）
    pub total_reg: u32,
    pub total_reject: u32,
}

fn fnv2(a: u32, b: u32) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for v in [a, b] {
        h ^= v;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

impl GapList {
    pub fn new() -> Self {
        GapList {
            ver: 1,
            frozen: false,
            seal_fp: 0,
            items: Vec::new(),
            reject_log: Vec::new(),
            total_reg: 0,
            total_reject: 0,
        }
    }

    pub fn ver(&self) -> u32 {
        self.ver
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 登记一条能力盘点回执。state=Missing 必须携带 Source（零编造）。
    /// Pending 允许无证据，但绝不允许 Pending 之外的状态缺来源。
    pub fn register(
        &mut self,
        fp: AppFp,
        domain: u8,
        state: ItemState,
        src: Option<Source>,
        note: &str,
    ) -> Result<(), u16> {
        if self.frozen {
            self.total_reject += 1;
            self.reject_log.push((fp, domain, E_FROZEN));
            return Err(E_FROZEN);
        }
        if domain < DOM_CARET || domain > DOM_STATUS {
            self.total_reject += 1;
            self.reject_log.push((fp, domain, E_BAD_DOMAIN));
            return Err(E_BAD_DOMAIN);
        }
        if fp.cls == 0 && fp.inst == 0 {
            self.total_reject += 1;
            self.reject_log.push((fp, domain, E_BAD_FP));
            return Err(E_BAD_FP);
        }
        match state {
            ItemState::Missing | ItemState::Present => {
                // 幻觉红线：缺来源 = 编造嫌疑，拒收并留账。
                let s = match src {
                    Some(s) => s,
                    None => {
                        self.total_reject += 1;
                        self.reject_log.push((fp, domain, E_NO_SOURCE));
                        return Err(E_NO_SOURCE);
                    }
                };
                // 幂等重入：同 (fp, domain) 已有定论时只更新证据链。
                if let Some(old) = self
                    .items
                    .iter_mut()
                    .find(|it| it.fp == fp && it.domain == domain)
                {
                    if old.state == state && old.src == Some(s) {
                        old.note = note_str(note);
                        old.ver = self.ver;
                        self.total_reg += 1;
                        return Ok(());
                    }
                    // 同键异来源：同结论=佐证放行（保留更高优先级来源）；
                    // 异结论=按来源优先级仲裁（L2Probe > ProtoProbe > ReAudit）。
                    let rank = |s: Source| match s {
                        Source::L2Probe => 3,
                        Source::ProtoProbe => 2,
                        Source::ReAudit => 1,
                    };
                    if old.state == state {
                        // 佐证：结论一致，仅强化证据链；来源取优先级高者。
                        if rank(s) > rank(old.src.unwrap_or(Source::ReAudit)) {
                            old.src = Some(s);
                        }
                        old.note = note_str(note);
                        old.ver = self.ver;
                        self.total_reg += 1;
                        Ok(())
                    } else if rank(s) >= rank(old.src.unwrap_or(Source::ReAudit)) {
                        old.state = state;
                        old.src = Some(s);
                        old.note = note_str(note);
                        old.ver = self.ver;
                        self.total_reg += 1;
                        Ok(())
                    } else {
                        self.total_reject += 1;
                        self.reject_log.push((fp, domain, E_DUP_CONFLICT));
                        Err(E_DUP_CONFLICT)
                    }
                } else {
                    self.items.push(GapItem {
                        fp,
                        domain,
                        state,
                        src: Some(s),
                        note: note_str(note),
                        ver: self.ver,
                    });
                    self.total_reg += 1;
                    Ok(())
                }
            }
            ItemState::Pending => {
                // 未测：不许覆盖已有定论（缺失/具备都是定论）。
                if let Some(old) = self
                    .items
                    .iter()
                    .find(|it| it.fp == fp && it.domain == domain && it.state != ItemState::Pending)
                {
                    let _ = old;
                    self.total_reject += 1;
                    self.reject_log.push((fp, domain, E_NOT_MEASURED));
                    return Err(E_NOT_MEASURED);
                }
                if !self.items.iter().any(|it| it.fp == fp && it.domain == domain) {
                    self.items.push(GapItem {
                        fp,
                        domain,
                        state: ItemState::Pending,
                        src: None,
                        note: note_str(note),
                        ver: self.ver,
                    });
                    self.total_reg += 1;
                }
                Ok(())
            }
        }
    }

    /// 冻结封账：指纹 = 全条目 (fp.cls,fp.inst,domain,state) FNV。
    pub fn freeze(&mut self) -> Result<u32, u16> {
        if self.frozen {
            return Err(E_FROZEN);
        }
        if self.items.iter().any(|it| it.state == ItemState::Missing && it.src.is_none()) {
            return Err(E_NO_SOURCE); // 封账前自检：缺失项必须全带来源
        }
        let fp = self.compute_fp();
        self.seal_fp = fp;
        self.frozen = true;
        Ok(fp)
    }

    /// 解冻重扫：必须升版，旧指纹留档核对后开新一轮。
    pub fn unfreeze_rescan(&mut self, old_fp: u32) -> Result<u32, u16> {
        if !self.frozen {
            return Err(E_UNFREEZE_STALE);
        }
        if self.seal_fp != old_fp {
            return Err(E_SEAL_MISMATCH);
        }
        self.frozen = false;
        self.ver += 1;
        self.seal_fp = 0;
        Ok(self.ver)
    }

    /// 缺失项导出（只导 Missing，Pending/Present 不入缺失面）。
    pub fn missing(&self) -> Vec<(AppFp, u8)> {
        let mut out = Vec::new();
        for it in &self.items {
            if it.state == ItemState::Missing {
                out.push((it.fp, it.domain));
            }
        }
        out
    }

    /// 按域统计缺失数（五域逐项，供对照 L2 汇总）。
    pub fn missing_by_domain(&self) -> [u32; 5] {
        let mut acc = [0u32; 5];
        for it in &self.items {
            if it.state == ItemState::Missing && it.domain >= DOM_CARET && it.domain <= DOM_STATUS {
                acc[(it.domain - 1) as usize] += 1;
            }
        }
        acc
    }

    pub fn compute_fp(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        for it in &self.items {
            h ^= fnv2(it.fp.cls, it.fp.inst);
            h = h.wrapping_mul(0x0100_0193);
            h ^= it.domain as u32;
            h = h.wrapping_mul(0x0100_0193);
            h ^= it.state as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        h
    }

    pub fn reject_len(&self) -> usize {
        self.reject_log.len()
    }
}

fn note_str(s: &str) -> String {
    // 截断到 64 字符，防超长注水；如实抄录不扩写。
    let mut out = String::new();
    for (i, b) in s.bytes().enumerate() {
        if i >= 64 {
            break;
        }
        out.push(b as char);
    }
    out
}

/// 自例程判据：落实 J1「清单入册零编造」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let xt = AppFp { cls: 0x9C37, inst: 0x1111 }; // xterm 样例指纹

    // R1 场景：无来源缺失登记 → 拒收留账（零编造红线）。
    let mut g = GapList::new();
    match g.register(xt, DOM_CARET, ItemState::Missing, None, "无证据") {
        Err(E_NO_SOURCE) => {}
        _ => return Err(E_NO_SOURCE),
    }
    if g.reject_len() != 1 || g.total_reg != 0 {
        return Err(E_NO_SOURCE);
    }
    hits += 1;

    // R2 场景：带来源缺失登记 → 入册；重复登记幂等（更新证据链不重记）。
    g.register(xt, DOM_CARET, ItemState::Missing, Some(Source::L2Probe), "L2 探针：无 XIM 报")?;
    let n0 = g.len();
    g.register(xt, DOM_CARET, ItemState::Missing, Some(Source::L2Probe), "L2 探针复测同判")?;
    if g.len() != n0 {
        return Err(E_DUP_CONFLICT); // 幂等：不得新增条目
    }
    hits += 1;

    // R3 场景：五域闭集外拒收。
    for bad in [0u8, 6u8, 255u8] {
        match g.register(xt, bad, ItemState::Missing, Some(Source::L2Probe), "越域") {
            Err(E_BAD_DOMAIN) => {}
            _ => return Err(E_BAD_DOMAIN),
        }
    }
    hits += 1;

    // R4 场景：未测冒充缺失拒收 —— Pending 之后来无源 Missing 必拒；
    //        重复 Pending 幂等放行（不重记不新增）。
    let mut g2 = GapList::new();
    g2.register(xt, DOM_CANDWIN, ItemState::Pending, None, "未测")?;
    match g2.register(xt, DOM_CANDWIN, ItemState::Missing, None, "冒充") {
        Err(E_NO_SOURCE) => {}
        _ => return Err(E_NO_SOURCE),
    }
    let n_before = g2.len();
    g2.register(xt, DOM_CANDWIN, ItemState::Pending, None, "重复未测")?;
    if g2.len() != n_before {
        return Err(E_DUP_CONFLICT); // 幂等：不得新增条目
    }
    hits += 1;

    // R5 场景：已定论（Present）后再报 Pending → 拒收（不许回退成未测）。
    g2.register(xt, DOM_PREEDIT, ItemState::Present, Some(Source::ProtoProbe), "XIM 有输入样式")?;
    match g2.register(xt, DOM_PREEDIT, ItemState::Pending, None, "想回退") {
        Err(E_NOT_MEASURED) => {}
        _ => return Err(E_NOT_MEASURED),
    }
    hits += 1;

    // R6 场景：来源优先级仲裁 —— 低优先级不得推翻高优先级定论；高可覆低。
    let mut g3 = GapList::new();
    g3.register(xt, DOM_FONTSET, ItemState::Missing, Some(Source::L2Probe), "L2 实测缺")?;
    match g3.register(xt, DOM_FONTSET, ItemState::Present, Some(Source::ReAudit), "复扫说有") {
        Err(E_DUP_CONFLICT) => {}
        _ => return Err(E_DUP_CONFLICT),
    }
    g3.register(xt, DOM_FONTSET, ItemState::Missing, Some(Source::ProtoProbe), "协议探测补证")?; // 同向补证放行
    let mut g4 = GapList::new();
    g4.register(xt, DOM_STATUS, ItemState::Pending, None, "先未测")?;
    g4.register(xt, DOM_STATUS, ItemState::Present, Some(Source::ProtoProbe), "协议层证明有")?;
    g4.register(xt, DOM_STATUS, ItemState::Missing, Some(Source::L2Probe), "L2 实测推翻")?; // 高覆低放行
    if g4.missing_by_domain()[DOM_STATUS as usize - 1] != 1 {
        return Err(E_DUP_CONFLICT);
    }
    hits += 1;

    // R7 场景：坏指纹拒收（全零指纹）。
    let zero = AppFp { cls: 0, inst: 0 };
    match g4.register(zero, DOM_CARET, ItemState::Missing, Some(Source::L2Probe), "零指纹") {
        Err(E_BAD_FP) => {}
        _ => return Err(E_BAD_FP),
    }
    hits += 1;

    // R8 场景：冻结封账 → 写拒；指纹一致性核对；解冻不升版/错指纹必拒。
    let mut g5 = GapList::new();
    g5.register(xt, DOM_CARET, ItemState::Missing, Some(Source::L2Probe), "缺 caret")?;
    let fp1 = g5.freeze()?;
    match g5.register(xt, DOM_PREEDIT, ItemState::Missing, Some(Source::L2Probe), "冻结后写") {
        Err(E_FROZEN) => {}
        _ => return Err(E_FROZEN),
    }
    match g5.unfreeze_rescan(fp1.wrapping_add(1)) {
        Err(E_SEAL_MISMATCH) => {}
        _ => return Err(E_SEAL_MISMATCH),
    }
    let v2 = g5.unfreeze_rescan(fp1)?;
    if v2 != 2 || g5.is_frozen() {
        return Err(E_UNFREEZE_STALE);
    }
    hits += 1;

    // R9 场景：封账自检 —— 缺失项无来源时 freeze 必拒（正常路径已在 R1 挡住，
    //        这里验证内部自检防线：用 ReAudit 来源补一条缺失，冻结应成功）。
    let mut g6 = GapList::new();
    g6.register(xt, DOM_STATUS, ItemState::Missing, Some(Source::ReAudit), "复扫认定缺")?;
    let fp2 = g6.freeze()?;
    if fp2 != g6.compute_fp() {
        return Err(E_SEAL_MISMATCH);
    }
    hits += 1;

    // R10 场景：五域分账统计口径 —— 三缺失两具备一 pending 各归其位。
    let mut g7 = GapList::new();
    g7.register(xt, DOM_CARET, ItemState::Missing, Some(Source::L2Probe), "d1")?;
    g7.register(xt, DOM_PREEDIT, ItemState::Missing, Some(Source::L2Probe), "d3")?;
    g7.register(xt, DOM_CANDWIN, ItemState::Missing, Some(Source::ProtoProbe), "d4")?;
    g7.register(xt, DOM_FONTSET, ItemState::Present, Some(Source::L2Probe), "d2")?;
    g7.register(xt, DOM_STATUS, ItemState::Pending, None, "d5")?;
    let m = g7.missing_by_domain();
    if !(m[0] == 1 && m[1] == 0 && m[2] == 1 && m[3] == 1 && m[4] == 0) {
        return Err(E_BAD_DOMAIN);
    }
    if g7.missing().len() != 3 {
        return Err(E_NO_SOURCE);
    }
    hits += 1;

    // R11 场景：64 字符摘要截断（防注水）。
    let long_note = "0123456789012345678901234567890123456789012345678901234567890123456789超长部分";
    let mut g8 = GapList::new();
    g8.register(xt, DOM_CANDWIN, ItemState::Missing, Some(Source::L2Probe), long_note)?;
    let it = &g8.items[0];
    if it.note.chars().count() > 64 {
        return Err(E_NO_SOURCE);
    }
    hits += 1;

    Ok(hits)
}
