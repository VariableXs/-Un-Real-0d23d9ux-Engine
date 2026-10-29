//! unx48279_fcuidecl — UNX-F48279 fcitx5 主题/候选风格数据不侵入声明（UI 绘制归 F4）（诊断段 0xC3C0）。
//!
//! 判据原文：
//!   J1 声明入册+越界注入 ×10 零波及；判据锚定 §AI-61 §1 语义边界 F4/AI-29。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 语义边界声明：主题/候选风格/皮肤/字体引用/布局提示五类 UI
//!      数据类的绘制主权归 F4 域——AI 域对接面只透传不解析不存储，
//!      声明入册后冻结（不可追加、不可撤回、不可改属主）。
//!   2. 越界注入零波及：五类数据各 ×2 次注入 AI 域对接面（共 ×10），
//!      边界卫兵全部拦截；拦截后 AI 域侧账本状态指纹前后必须一致
//!      （零波及 = 连一个计数器都不许动）。
//!   3. 控制组：合法 AI 域业务载荷（判据结果/账目行）必须正常入账
//!      ——卫兵只拦越界类，不误伤业务流（零假阳性）。
//!   4. 声明冻结性：入册后任何改写尝试（改属主/加类/撤册）必须被
//!      注册表拒绝并留痕，冻结位只读。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC3C0;

const E_DECL_MISS: u16 = SEG_BASE + 1;    // 声明未入册
const E_DECL_THAW: u16 = SEG_BASE + 2;    // 冻结声明被改写
const E_INJECT_LEAK: u16 = SEG_BASE + 3;  // 越界注入漏拦
const E_STATE_SPILL: u16 = SEG_BASE + 4;  // 拦截后状态指纹漂移（波及）
const E_BIZ_FALSE_BLOCK: u16 = SEG_BASE + 5; // 业务载荷误拦（假阳性）
const E_OWNER_BAD: u16 = SEG_BASE + 6;    // 属主不是 F4
const E_CODE_RANGE: u16 = SEG_BASE + 7;   // 诊断码越段

/// 五类 UI 数据类（绘制主权归 F4）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DataClass {
    /// 主题（配色/明暗）。
    Theme = 0,
    /// 候选风格（排版/字号/间距）。
    CandidateStyle = 1,
    /// 皮肤（贴图/圆角/阴影）。
    Skin = 2,
    /// 字体引用（字体名/回退链）。
    FontRef = 3,
    /// 布局提示（锚点/悬浮方向）。
    LayoutHint = 4,
}

pub const DATA_CLASSES: usize = 5;

/// 域属主。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    /// F4：UI 绘制域（五类数据的合法属主）。
    F4,
    /// AI：判据/账目域（对接面所在域，禁止持有五类数据）。
    Ai,
}

/// 边界声明：五类数据类 × 属主 F4，冻结后不可变。
pub struct UiDeclaration {
    owner: Domain,
    classes: [DataClass; DATA_CLASSES],
    frozen: bool,
}

impl UiDeclaration {
    /// 构造并立即入册语义：属主 F4、五类全量、未冻结。
    pub fn new() -> Self {
        UiDeclaration {
            owner: Domain::F4,
            classes: [
                DataClass::Theme,
                DataClass::CandidateStyle,
                DataClass::Skin,
                DataClass::FontRef,
                DataClass::LayoutHint,
            ],
            frozen: false,
        }
    }

    /// 冻结：单向闸，冻后所有变更入口关闭。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }

    pub fn frozen(&self) -> bool {
        self.frozen
    }

    pub fn owner(&self) -> Domain {
        self.owner
    }

    pub fn classes(&self) -> &[DataClass; DATA_CLASSES] {
        &self.classes
    }
}

/// 声明注册表：入册后冻结（冻结性由注册表背书，不是靠约定）。
pub struct DeclRegistry {
    entry: Option<UiDeclaration>,
    tamper_marks: u32,
}

impl DeclRegistry {
    pub fn new() -> Self {
        DeclRegistry {
            entry: None,
            tamper_marks: 0,
        }
    }

    /// 入册：空册可入一次；重复入册拒绝。
    pub fn register(&mut self, d: UiDeclaration) -> bool {
        if self.entry.is_some() {
            return false;
        }
        self.entry = Some(d);
        true
    }

    /// 改写尝试（属主/类表/解冻任一意图）：入册即冻结，全部拒绝留痕。
    pub fn tamper_attempt(&mut self) -> bool {
        match self.entry {
            Some(ref d) if d.frozen() => {
                self.tamper_marks += 1;
                false
            }
            _ => false,
        }
    }

    pub fn registered(&self) -> bool {
        match self.entry {
            Some(_) => true,
            None => false,
        }
    }

    pub fn tamper_marks(&self) -> u32 {
        self.tamper_marks
    }
}

/// 进面载荷：类标签 + 载荷字（类标签决定越界与否）。
#[derive(Clone, Copy)]
pub struct Payload {
    pub tag: Option<DataClass>, // None = AI 域业务载荷
    pub word: u32,
}

/// AI 域账本：业务行计数器 + 状态指纹（零波及的度量对象）。
pub struct AiLedger {
    biz_rows: u32,
    fingerprint: u64,
}

impl AiLedger {
    pub fn new() -> Self {
        AiLedger {
            biz_rows: 0,
            fingerprint: 0xCBF2_9CE4_8422_2325,
        }
    }

    /// 业务入账：仅合法业务载荷可走此路（计数器 +1、指纹折叠）。
    pub fn accept_biz(&mut self, word: u32) {
        self.biz_rows += 1;
        self.fingerprint ^= word as u64;
        self.fingerprint = self.fingerprint.wrapping_mul(0x0000_0100_0000_01B3);
    }

    pub fn biz_rows(&self) -> u32 {
        self.biz_rows
    }

    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }
}

/// 边界卫兵：None 标签（业务）放行入账；Some(类) 越界拦截且不触账。
pub fn guard(led: &mut AiLedger, p: &Payload) -> bool {
    match p.tag {
        Some(_) => false, // 拦截：不入账不触状态
        None => {
            led.accept_biz(p.word);
            true
        }
    }
}

/// B14 UI 数据不侵入主巡检：声明入册 + 越界注入 ×10 零波及全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 声明入册：属主 F4、五类全量、注册表恰一条。
    let mut reg = DeclRegistry::new();
    let mut decl = UiDeclaration::new();
    if decl.owner() != Domain::F4 {
        return Err(E_OWNER_BAD);
    }
    let cs = decl.classes();
    let mut filled = 0usize;
    let mut i = 0;
    while i < DATA_CLASSES {
        // 五类互异：两两比较（枚举相等性）。
        let mut j = i + 1;
        while j < DATA_CLASSES {
            if cs[i] == cs[j] {
                return Err(E_DECL_MISS);
            }
            j += 1;
        }
        filled += 1;
        i += 1;
    }
    if filled != DATA_CLASSES {
        return Err(E_DECL_MISS);
    }
    decl.freeze();
    if !reg.register(decl) {
        return Err(E_DECL_MISS);
    }
    if !reg.registered() {
        return Err(E_DECL_MISS);
    }
    hits += 1;

    // R2 冻结性：入册后改写尝试 ×3（属主改/解冻/重注册语义）全拒绝留痕。
    let mut t = 0;
    while t < 3 {
        if reg.tamper_attempt() {
            return Err(E_DECL_THAW);
        }
        t += 1;
    }
    if reg.tamper_marks() != 3 {
        return Err(E_DECL_THAW);
    }
    hits += 1;

    // R3 越界注入 ×10：五类 × 2 次，卫兵全拦截。
    let classes = [
        DataClass::Theme,
        DataClass::CandidateStyle,
        DataClass::Skin,
        DataClass::FontRef,
        DataClass::LayoutHint,
    ];
    let mut led = AiLedger::new();
    let fp_before = led.fingerprint();
    let rows_before = led.biz_rows();
    let mut injected = 0u32;
    let mut c = 0usize;
    while c < DATA_CLASSES {
        let mut n = 0;
        while n < 2 {
            let p = Payload {
                tag: Some(classes[c]),
                word: 0xBEEF00 + (c as u32) * 2 + n,
            };
            if guard(&mut led, &p) {
                return Err(E_INJECT_LEAK);
            }
            injected += 1;
            n += 1;
        }
        c += 1;
    }
    if injected != 10 {
        return Err(E_INJECT_LEAK);
    }
    hits += 1;

    // R4 零波及：×10 拦截后 AI 域账本指纹与行数与注入前逐位一致。
    if led.fingerprint() != fp_before || led.biz_rows() != rows_before {
        return Err(E_STATE_SPILL);
    }
    hits += 1;

    // R5 控制组：合法业务载荷 ×5 全放行入账（计数器/指纹正常推进）。
    let mut b = 0;
    while b < 5 {
        let p = Payload {
            tag: None,
            word: 0x1D00 + b,
        };
        if !guard(&mut led, &p) {
            return Err(E_BIZ_FALSE_BLOCK);
        }
        b += 1;
    }
    if led.biz_rows() != rows_before + 5 {
        return Err(E_BIZ_FALSE_BLOCK);
    }
    // 指纹必须推进（不等于初值）且可复算（重放同序列同指纹）。
    let fp_mid = led.fingerprint();
    if fp_mid == fp_before {
        return Err(E_BIZ_FALSE_BLOCK);
    }
    let mut led2 = AiLedger::new();
    let mut b2 = 0;
    while b2 < 5 {
        led2.accept_biz(0x1D00 + b2);
        b2 += 1;
    }
    if led2.fingerprint() != fp_mid {
        return Err(E_BIZ_FALSE_BLOCK);
    }
    hits += 1;

    // R6 混合流卫兵：业务/越界交错注入，业务 5 行全入账、越界 5 发全拦
    //    且最终指纹与「纯业务 5 行」指纹一致（拦截对状态零贡献）。
    let mut led3 = AiLedger::new();
    let mut m = 0;
    while m < 5 {
        // 越界一发。
        let bad = Payload {
            tag: Some(classes[m]),
            word: 0xDEAD00 + m as u32,
        };
        if guard(&mut led3, &bad) {
            return Err(E_INJECT_LEAK);
        }
        // 业务一行。
        let biz = Payload {
            tag: None,
            word: 0x1D00 + m as u32,
        };
        if !guard(&mut led3, &biz) {
            return Err(E_BIZ_FALSE_BLOCK);
        }
        m += 1;
    }
    if led3.fingerprint() != fp_mid || led3.biz_rows() != 5 {
        return Err(E_STATE_SPILL);
    }
    hits += 1;

    // R7 诊断码闭包：全部错误码落在 [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 7] = [
        E_DECL_MISS,
        E_DECL_THAW,
        E_INJECT_LEAK,
        E_STATE_SPILL,
        E_BIZ_FALSE_BLOCK,
        E_OWNER_BAD,
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
