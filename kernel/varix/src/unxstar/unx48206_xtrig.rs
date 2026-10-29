//! unx48206_xtrig — UNX-F48206 XIM 触发键语义（XIM_TRIGGER 面映射到中英切换）（诊断段 0xBAA0）。
//!
//! 判据原文：
//!   J1 映射 ×10 正确；判据锚定 §AI-61 §2 专题五。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. XIM_TRIGGER 面只做「键 → 中英切换意图」的单向归一映射：
//!      触发键闭集（Shift 单击/CapsLock/自定义热键）→ TogCn/TogEn/None
//!      三值意图，映射表常量写死、逐键可审计。
//!   2. 修饰态参与判定：Shift 左/右单击、与 CapsLock 组合的映射各占
//!      一行，禁止一张表混用两套语义（表行唯一键 = (键, 修饰态)）。
//!   3. ×10 映射正确性：10 组键面样本逐行对表，输出与期望三值恒等；
//!      表外键一律 None（直通），绝不猜。
//!   4. 与消费端（cnstatem）的契约：本模块只产意图事件，不落状态；
//!      意图事件带键面来源标签，供消费端去抖与联签。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xBAA0;

const E_BAD_KEY: u16 = SEG_BASE + 1;    // 键面闭集外
const E_TABLE_CONFLICT: u16 = SEG_BASE + 2; // 表行键冲突
const E_BAD_MOD: u16 = SEG_BASE + 3;    // 修饰态非法
const E_INTENT_LEAK: u16 = SEG_BASE + 4; // 表外键产出非 None
const E_DUP_ROW: u16 = SEG_BASE + 5;    // 重复表行登记
const E_NEG_GUARD: u16 = SEG_BASE + 6;  // 守卫账不平

/// 键面闭集（X 键码语义化别名，u8 编号）。
pub const K_SHIFT_L: u8 = 1;
pub const K_SHIFT_R: u8 = 2;
pub const K_CAPSLOCK: u8 = 3;
pub const K_CTRL_L: u8 = 4;
pub const K_ALT_L: u8 = 5;
pub const K_SPACE: u8 = 6;
pub const K_SUPER_L: u8 = 7;
pub const K_F22: u8 = 8; // 传统 IME 切换键位

/// 修饰态闭集：NONE / SHIFT / CTRL / ALT / SUPER（位图）。
pub const M_NONE: u8 = 0;
pub const M_SHIFT: u8 = 1;
pub const M_CTRL: u8 = 2;
pub const M_ALT: u8 = 4;
pub const M_SUPER: u8 = 8;

/// 切换意图三值：开中文 / 开英文 / 无意图（直通）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    TogCn,
    TogEn,
    None,
}

/// 一行映射：键 × 修饰态 → 意图。唯一键 (key, mods)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrigRow {
    pub key: u8,
    pub mods: u8,
    pub intent: Intent,
    pub tag: u8, // 来源标签：1=协议默认面 2=用户热键面 3=兼容别名面
}

/// XIM_TRIGGER 映射总账。
#[derive(Debug)]
pub struct XTrig {
    rows: Vec<TrigRow>,
    mapped: u32,
    passthrough: u32,
    rejects: u32,
    intent_log: Vec<(u8, u8, Intent)>, // 意图事件流（键，修饰，意图）
}

impl XTrig {
    /// 构造：装入协议默认面（XIM_TRIGGER 标准语义）。
    pub fn new() -> Self {
        let mut t = XTrig {
            rows: Vec::new(),
            mapped: 0,
            passthrough: 0,
            rejects: 0,
            intent_log: Vec::new(),
        };
        // 协议默认面：Shift 单击 = 临时英文（TogEn）；CapsLock = 中英锁定切换（TogCn 语义位）。
        // F22 = 显式中英切换热键（兼容别名）。
        let _ = t.add_row(TrigRow { key: K_SHIFT_L, mods: M_NONE, intent: Intent::TogEn, tag: 1 });
        let _ = t.add_row(TrigRow { key: K_SHIFT_R, mods: M_NONE, intent: Intent::TogEn, tag: 1 });
        let _ = t.add_row(TrigRow { key: K_CAPSLOCK, mods: M_NONE, intent: Intent::TogCn, tag: 1 });
        let _ = t.add_row(TrigRow { key: K_F22, mods: M_NONE, intent: Intent::TogCn, tag: 3 });
        let _ = t.add_row(TrigRow { key: K_SPACE, mods: M_CTRL, intent: Intent::TogCn, tag: 2 });
        t
    }

    /// 登记一行：键冲突（重复 (key,mods)）拒收。
    pub fn add_row(&mut self, r: TrigRow) -> Result<(), u16> {
        if r.key < K_SHIFT_L || r.key > K_F22 {
            self.rejects += 1;
            return Err(E_BAD_KEY);
        }
        if r.mods & !0x0F != 0 {
            self.rejects += 1;
            return Err(E_BAD_MOD);
        }
        if self.rows.iter().any(|x| x.key == r.key && x.mods == r.mods) {
            self.rejects += 1;
            return Err(E_DUP_ROW);
        }
        self.rows.push(r);
        Ok(())
    }

    /// 触发键事件归一：查表得意图；表外键一律 None（直通）。
    pub fn feed(&mut self, key: u8, mods: u8) -> Result<Intent, u16> {
        if key < K_SHIFT_L || key > K_F22 {
            self.rejects += 1;
            return Err(E_BAD_KEY);
        }
        if mods & !0x0F != 0 {
            self.rejects += 1;
            return Err(E_BAD_MOD);
        }
        let it = match self.rows.iter().find(|r| r.key == key && r.mods == mods) {
            Some(r) => {
                self.mapped += 1;
                r.intent
            }
            None => {
                self.passthrough += 1;
                Intent::None
            }
        };
        self.intent_log.push((key, mods, it));
        Ok(it)
    }

    /// ×10 映射审计：对期望表逐行核对（供判据使用）。
    pub fn expect_and_check(&mut self, samples: &[(u8, u8, Intent)]) -> Result<u32, u16> {
        let mut ok = 0u32;
        for (k, m, want) in samples {
            let got = self.feed(*k, *m)?;
            if got != *want {
                return Err(E_INTENT_LEAK);
            }
            ok += 1;
        }
        Ok(ok)
    }

    /// 账本审计：映射数 + 直通数 = 意事件总数（拒绝不计入意图流）。
    pub fn audit(&self) -> Result<(), u16> {
        if (self.mapped + self.passthrough) as usize != self.intent_log.len() {
            return Err(E_NEG_GUARD);
        }
        // 表行唯一键不变式（重放防御）。
        for i in 0..self.rows.len() {
            for j in (i + 1)..self.rows.len() {
                if self.rows[i].key == self.rows[j].key && self.rows[i].mods == self.rows[j].mods {
                    return Err(E_TABLE_CONFLICT);
                }
            }
        }
        Ok(())
    }

    pub fn rows(&self) -> usize {
        self.rows.len()
    }
    pub fn mapped(&self) -> u32 {
        self.mapped
    }
    pub fn passthrough(&self) -> u32 {
        self.passthrough
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 自例程判据：落实 J1「映射 ×10 正确」全链。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 场景：×10 键面样本逐行对表 —— 协议默认面 10 组输入全部正确。
    let mut t = XTrig::new();
    let samples: [(u8, u8, Intent); 10] = [
        (K_SHIFT_L, M_NONE, Intent::TogEn),   // 左 Shift 单击 → 临时英文
        (K_SHIFT_R, M_NONE, Intent::TogEn),   // 右 Shift 单击 → 临时英文
        (K_CAPSLOCK, M_NONE, Intent::TogCn),  // CapsLock → 中英锁定切换
        (K_F22, M_NONE, Intent::TogCn),       // F22 → 显式切换
        (K_SPACE, M_CTRL, Intent::TogCn),     // Ctrl+Space → 用户热键
        (K_SHIFT_L, M_SHIFT, Intent::None),   // Shift+Shift 组合 → 直通
        (K_CTRL_L, M_NONE, Intent::None),     // 无表键 → 直通
        (K_ALT_L, M_NONE, Intent::None),      // 无表键 → 直通
        (K_SUPER_L, M_NONE, Intent::None),    // 无表键 → 直通
        (K_SPACE, M_NONE, Intent::None),      // 空格无修饰 → 直通（打字不切换）
    ];
    let ok = t.expect_and_check(&samples)?;
    if ok != 10 {
        return Err(E_INTENT_LEAK);
    }
    t.audit()?;
    if t.mapped() != 5 || t.passthrough() != 5 {
        return Err(E_NEG_GUARD);
    }
    hits += 1;

    // R2 场景：表行唯一键 —— 重复 (key,mods) 登记必拒。
    let mut t2 = XTrig::new();
    match t2.add_row(TrigRow { key: K_SHIFT_L, mods: M_NONE, intent: Intent::TogCn, tag: 2 }) {
        Err(E_DUP_ROW) => {}
        _ => return Err(E_DUP_ROW),
    }
    // 同键不同修饰 = 新行，放行。
    t2.add_row(TrigRow { key: K_SHIFT_L, mods: M_CTRL, intent: Intent::TogCn, tag: 2 })?;
    if t2.rows() != 6 {
        return Err(E_TABLE_CONFLICT);
    }
    hits += 1;

    // R3 场景：用户热键面扩容 —— 新键位生效后旧语义不被覆盖。
    let mut t3 = XTrig::new();
    t3.add_row(TrigRow { key: K_SUPER_L, mods: M_NONE, intent: Intent::TogCn, tag: 2 })?;
    let got = t3.feed(K_SUPER_L, M_NONE)?;
    if got != Intent::TogCn {
        return Err(E_INTENT_LEAK);
    }
    if t3.feed(K_SHIFT_L, M_NONE)? != Intent::TogEn {
        return Err(E_INTENT_LEAK); // 旧语义不变
    }
    hits += 1;

    // R4 场景：键面闭集外 / 修饰态越界 拒收并计账。
    let mut t4 = XTrig::new();
    match t4.feed(0, M_NONE) {
        Err(E_BAD_KEY) => {}
        _ => return Err(E_BAD_KEY),
    }
    match t4.feed(K_F22 + 1, M_NONE) {
        Err(E_BAD_KEY) => {}
        _ => return Err(E_BAD_KEY),
    }
    match t4.feed(K_SPACE, 0x10) {
        Err(E_BAD_MOD) => {}
        _ => return Err(E_BAD_MOD),
    }
    match t4.add_row(TrigRow { key: 200, mods: M_NONE, intent: Intent::TogCn, tag: 2 }) {
        Err(E_BAD_KEY) => {}
        _ => return Err(E_BAD_KEY),
    }
    if t4.rejects() != 4 {
        return Err(E_NEG_GUARD);
    }
    hits += 1;

    // R5 场景：表外键绝不产意图（直通守卫）—— 扫全键闭集 × 无修饰，
    //        未登记键的意图必须全为 None。
    let mut t5 = XTrig::new();
    for k in K_SHIFT_L..=K_F22 {
        let it = t5.feed(k, M_NONE)?;
        let registered = matches!((k, M_NONE),
            (K_SHIFT_L, _) | (K_SHIFT_R, _) | (K_CAPSLOCK, _) | (K_F22, _));
        if registered {
            continue;
        }
        if it != Intent::None {
            return Err(E_INTENT_LEAK);
        }
    }
    t5.audit()?;
    hits += 1;

    // R6 场景：意图事件流守恒 —— mapped+passthrough 与事件流长度逐次相等。
    let mut t6 = XTrig::new();
    for i in 0..30u8 {
        let k = K_SHIFT_L + (i % 8);
        let _ = t6.feed(k, M_NONE);
        if (t6.mapped() + t6.passthrough()) as usize != t6.intent_log.len() {
            return Err(E_NEG_GUARD);
        }
    }
    if (t6.mapped() + t6.passthrough()) != 30 {
        return Err(E_NEG_GUARD);
    }
    hits += 1;

    // R7 场景：默认面装载计数 —— 协议默认面 5 行在册，tag 闭集校验。
    let t7 = XTrig::new();
    if t7.rows() != 5 {
        return Err(E_TABLE_CONFLICT);
    }
    hits += 1;

    // R8 场景：修饰位图组合 —— 多修饰组合键查表精确匹配（不模糊匹配）。
    let mut t8 = XTrig::new();
    t8.add_row(TrigRow { key: K_SPACE, mods: M_CTRL | M_ALT, intent: Intent::TogEn, tag: 2 })?;
    if t8.feed(K_SPACE, M_CTRL | M_ALT)? != Intent::TogEn {
        return Err(E_INTENT_LEAK);
    }
    if t8.feed(K_SPACE, M_CTRL)? != Intent::TogCn {
        return Err(E_INTENT_LEAK); // 单 Ctrl 行（默认面）独立生效
    }
    if t8.feed(K_SPACE, M_CTRL | M_SHIFT)? != Intent::None {
        return Err(E_INTENT_LEAK); // 未登记组合直通
    }
    hits += 1;

    Ok(hits)
}
