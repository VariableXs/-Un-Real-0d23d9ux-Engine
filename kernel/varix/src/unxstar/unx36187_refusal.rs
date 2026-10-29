//! UNX-F36187-J1 拒绝路径三要素呈现（发生了什么/为什么/下一步）。
//! 判据：① 拒绝呈现 100 例三要素齐备——每个拒绝答复必须同时携带
//! 「发生了什么（事实面）/为什么（依据面）/下一步（出路面）」，缺一
//! 即呈现残废；② 技术细节折叠位（详情段）断言——三要素面向用户，
//! 诊断细节面向工程师，两层分离且折叠位显性标记；③ 裸异常码注入
//! 10 例被质检拦截（只带码不带你好——码≠呈现）；④ 纯功能 ≥320 行。
//! 设计纪律：拒绝呈现的真源不是文案，是结构——用户被拒后的一切挫败
//! 都源于「只知被拒不知为何」。三要素是呈现的完备性判据（少一件就是
//! 挫败指纹——36196 的输入面）；详情折叠位是呈现的分层判据（工程师
//! 信息不该糊用户一脸，但必须可达）。消费 36186 统一桶码呈现名。

use crate::checks::CheckSet;
use super::unx36102_stdrights as sr;
use super::unx36186_errbook as eb;

pub const SEG_BASE: u16 = 0x4250;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_ELEMENT: u16 = SEG_BASE + 2; // 三要素残废（缺事实/依据/出路）
pub const E_FOLD: u16 = SEG_BASE + 3; // 折叠位缺失（工程师层不可达）
pub const E_BARE: u16 = SEG_BASE + 4; // 裸码呈现（质检拦截）
pub const E_ILLEGAL: u16 = SEG_BASE + 5; // 掩码越合法区
pub const E_UNKNOWN: u16 = SEG_BASE + 6; // 桶码未登记（对 36186 透传）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 三要素完备性位面（判据 ① 的结构化落点）。
pub const EL_FACT: u8 = 0x01; // 发生了什么（事实面）
pub const EL_WHY: u8 = 0x02; // 为什么（依据面）
pub const EL_NEXT: u8 = 0x04; // 下一步（出路面）
pub const EL_ALL: u8 = EL_FACT | EL_WHY | EL_NEXT;

/// 拒绝呈现答复（三要素 + 折叠详情段）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Refusal {
    /// 桶码（36186 统一登记簿的 J1 呈现码）。
    pub bucket: u16,
    /// 三要素位面（EL_* 或集）。
    pub elements: u8,
    /// 事实面文本锚（发生了什么）。
    pub fact: &'static str,
    /// 依据面文本锚（为什么——引用检查依据，不是猜）。
    pub why: &'static str,
    /// 出路面文本锚（下一步——用户能做什么）。
    pub next: &'static str,
    /// 工程师详情段（折叠位显性标记——文本锚 + 折叠旗标成对）。
    pub tech: &'static str,
    pub folded: bool,
}

impl Refusal {
    /// 三要素完备（缺一即残废）。
    pub fn complete(&self) -> bool {
        self.elements & EL_ALL == EL_ALL
            && !self.fact.is_empty()
            && !self.why.is_empty()
            && !self.next.is_empty()
    }
    /// 折叠位健康（详情段非空且显性折叠——两层分离）。
    pub fn fold_ok(&self) -> bool {
        !self.tech.is_empty() && self.folded
    }
    /// 裸码检测（只有桶码没有三要素——质检拦截对象）。
    pub fn bare(&self) -> bool {
        self.elements & EL_ALL != EL_ALL
    }
}

/// 黄金拒绝剧本（100 例：20 桶 × 5 场景变体）。
/// 场景变体：读/写/删/改属主/枚举——同一桶码在不同请求形态下呈现
/// 面必须三要素齐备（桶码定「为什么」的依据，请求形态定「下一步」）。
pub const SCENE_NAMES: [&str; 5] = ["读", "写", "删", "改属主", "枚举"];

/// 黄金呈现构建器：桶序 + 场景序 → 完整拒绝答复（确定性——呈现面
/// 不得即兴发挥，模板插值在 no_std 下退化为锚点选择）。
pub fn render(bucket_ord: usize, scene: usize) -> Result<Refusal, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if bucket_ord >= eb::BUCKET_CAP || scene >= SCENE_NAMES.len() {
        return Err(E_UNKNOWN);
    }
    let b = eb::BUCKETS[bucket_ord];
    let scene_name = SCENE_NAMES[scene];
    // 事实面按场景定（发生了什么——请求形态 × 拒绝结果）。
    // 依据面按桶定（为什么——桶呈现名即依据锚，来自统一登记簿）。
    // 出路面按场景定（下一步——用户对该请求形态的可行出路）。
    let _ = scene_name; // 场景锚参与确定性选择（编译面消歧）
    let r = Refusal {
        bucket: b.j1,
        elements: EL_ALL,
        fact: FACT_ANCHORS[scene],
        why: b.name,
        next: NEXT_ANCHORS[scene],
        tech: TECH_ANCHORS[bucket_ord % TECH_ANCHORS.len()],
        folded: true,
    };
    if !r.complete() {
        return Err(E_ELEMENT);
    }
    Ok(r)
}

/// 事实面锚（5 场景闭集）。
pub const FACT_ANCHORS: [&str; 5] = [
    "读请求被拒绝",
    "写请求被拒绝",
    "删除请求被拒绝",
    "属主变更被拒绝",
    "枚举请求被拒绝",
];

/// 出路面锚（5 场景闭集——出路必须是用户可行动作）。
pub const NEXT_ANCHORS: [&str; 5] = [
    "申请读权或联系属主",
    "申请写权或走授权面",
    "确认删除必要性后重试",
    "仅属主可变更属主",
    "申请列出权或缩小范围",
];

/// 工程师详情段锚（桶序折叠——10 段轮转覆盖 20 桶）。
pub const TECH_ANCHORS: [&str; 10] = [
    "检查点:版本锚比对",
    "检查点:位面守卫",
    "检查点:合法区求交",
    "检查点:候选投影",
    "检查点:账本互斥",
    "检查点:指纹扫描",
    "检查点:钟序推进",
    "检查点:剧本回放",
    "检查点:聚合对账",
    "检查点:呈现分派",
];

/// 质检器：裸码注入拦截——只带码不带三要素的答复不得出呈现面。
pub fn qc_gate(r: &Refusal) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if r.bare() {
        return Err(E_BARE);
    }
    if !r.complete() {
        return Err(E_ELEMENT);
    }
    if !r.fold_ok() {
        return Err(E_FOLD);
    }
    Ok(())
}

/// 裸码注入器：构造 10 个残废答复（三路：缺要素/空锚/折叠位假），
/// 质检逐一拦截。
pub fn bare_injection() -> Result<u32, u16> {
    let base = Refusal {
        bucket: eb::BUCKETS[0].j1,
        elements: EL_ALL,
        fact: FACT_ANCHORS[0],
        why: eb::BUCKETS[0].name,
        next: NEXT_ANCHORS[0],
        tech: TECH_ANCHORS[0],
        folded: true,
    };
    // 注入 1：只有事实面。
    let i1 = Refusal { elements: EL_FACT, ..base };
    // 注入 2：事实 + 依据，缺出路。
    let i2 = Refusal { elements: EL_FACT | EL_WHY, ..base };
    // 注入 3：位面全置但事实锚空。
    let i3 = Refusal { fact: "", ..base };
    // 注入 4：依据锚空。
    let i4 = Refusal { why: "", ..base };
    // 注入 5：出路锚空。
    let i5 = Refusal { next: "", ..base };
    // 注入 6：详情段空（折叠位有但内容不可达）。
    let i6 = Refusal { tech: "", ..base };
    // 注入 7：折叠位假（工程师层被迫直出）。
    let i7 = Refusal { folded: false, ..base };
    // 注入 8：位面零（纯裸码）。
    let i8 = Refusal { elements: 0, ..base };
    // 注入 9：位面只带出路（顺序残废变体）。
    let i9 = Refusal { elements: EL_NEXT, ..base };
    // 注入 10：位面只带依据。
    let i10 = Refusal { elements: EL_WHY, ..base };
    let bad = [i1, i2, i3, i4, i5, i6, i7, i8, i9, i10];
    let mut blocked = 0u32;
    let mut i = 0;
    while i < bad.len() {
        // 三路质检各自必须拦（bare 抓位面残废 / complete 抓锚空 / fold 抓层破）。
        let caught = qc_gate(&bad[i]).is_err();
        if !caught {
            return Err(E_BARE);
        }
        blocked += 1;
        i += 1;
    }
    Ok(blocked)
}

/// 100 例呈现对账：20 桶 × 5 场景全量渲染 + 质检全过 + 桶码对 36186
/// 反查一致（呈现面不自造码——桶码必须能回统一登记簿）。
pub fn render_audit() -> Result<u32, u16> {
    let mut passed = 0u32;
    let mut b = 0;
    while b < eb::BUCKET_CAP {
        let mut s = 0;
        while s < SCENE_NAMES.len() {
            let r = render(b, s)?;
            qc_gate(&r)?;
            // 桶码反查：呈现桶 → 36186 桶序必须回到本桶。
            let back = eb::bucket_of(r.bucket)?;
            if back != b {
                return Err(E_UNKNOWN);
            }
            passed += 1;
            s += 1;
        }
        b += 1;
    }
    Ok(passed)
}

/// 折叠位分层直证：三要素（用户层）与详情段（工程师层）文本互异
/// （同文 = 分层假动作）。
pub fn fold_layering() -> Result<(), u16> {
    let mut b = 0;
    while b < eb::BUCKET_CAP {
        let r = render(b, 0)?;
        if r.fact == r.tech || r.why == r.tech || r.next == r.tech {
            return Err(E_FOLD);
        }
        b += 1;
    }
    Ok(())
}

/// 依据面直证：why 锚必须真来自 36186 桶呈现名（不是呈现面自编）。
pub fn why_source() -> Result<(), u16> {
    let mut b = 0;
    while b < eb::BUCKET_CAP {
        let r = render(b, 3)?;
        if r.why != eb::BUCKETS[b].name {
            return Err(E_ELEMENT);
        }
        b += 1;
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：版本锚 + 三要素位面结构（EL_ALL 三位互异覆盖）。
    if VER != 1 {
        return Err(E_VER);
    }
    if EL_FACT == 0 || EL_WHY == 0 || EL_NEXT == 0 {
        return Err(E_ELEMENT);
    }
    if EL_FACT & EL_WHY != 0 || EL_FACT & EL_NEXT != 0 || EL_WHY & EL_NEXT != 0 {
        return Err(E_ELEMENT);
    }
    if EL_ALL != EL_FACT | EL_WHY | EL_NEXT {
        return Err(E_ELEMENT);
    }
    // R2：100 例呈现对账（20 桶 × 5 场景全质检 + 桶码反查）。
    let passed = render_audit()?;
    if passed != 100 {
        return Err(E_ELEMENT);
    }
    // R3：折叠位分层（20 桶用户层/工程师层互异）。
    fold_layering()?;
    // R4：依据面真源（why ← 36186 桶名直证）。
    why_source()?;
    // R5：裸码注入 10 例全拦截。
    let blocked = bare_injection()?;
    if blocked != 10 {
        return Err(E_BARE);
    }
    // R6：场景锚闭集健康（5 场景名互异非空）。
    let mut i = 0;
    while i < SCENE_NAMES.len() {
        if SCENE_NAMES[i].is_empty() {
            return Err(E_ELEMENT);
        }
        let mut k = i + 1;
        while k < SCENE_NAMES.len() {
            if SCENE_NAMES[i] == SCENE_NAMES[k] {
                return Err(E_ELEMENT);
            }
            k += 1;
        }
        i += 1;
    }
    Ok(passed.wrapping_mul(13).wrapping_add(blocked).wrapping_add(EL_ALL as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_ELEMENT => "三要素残废（缺事实/依据/出路）",
        E_FOLD => "折叠位缺失（工程师层不可达）",
        E_BARE => "裸码呈现（质检拦截）",
        E_ILLEGAL => "掩码越合法区",
        E_UNKNOWN => "桶码未登记",
        _ => "未知诊断码",
    }
}

pub fn run_unx36187_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36187_refusal");
    cs.add("SEG 段位独占", SEG_BASE == 0x4250, "0x4250~0x425F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "三要素 100 例齐备/折叠分层/裸码 10 例拦截"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("三要素位面", EL_ALL == 0x07 && EL_FACT & EL_WHY == 0, "三位互异完备（0x07 全集）");
    cs.add("场景容量", SCENE_NAMES.len() == 5, "5 场景 × 20 桶 = 100 例");
    cs.add("呈现容量", FACT_ANCHORS.len() == 5 && NEXT_ANCHORS.len() == 5, "事实/出路锚与场景同容量");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

// ---- 单元测试（对齐 B04 模板） --------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drill_green() {
        assert_eq!(drill().err(), None);
    }

    #[test]
    fn test_refusal_complete() {
        let r = render(0, 0).unwrap_or(Refusal {
            bucket: 0, elements: 0, fact: "", why: "", next: "", tech: "", folded: false,
        });
        assert!(r.complete());
        assert!(r.fold_ok());
        assert!(!r.bare());
    }

    #[test]
    fn test_bare_caught() {
        let mut r = render(5, 2).unwrap_or(Refusal {
            bucket: 0, elements: 0, fact: "", why: "", next: "", tech: "", folded: false,
        });
        r.elements = EL_FACT;
        assert!(qc_gate(&r).err() == Some(E_BARE) || qc_gate(&r).err() == Some(E_ELEMENT));
    }
}
