//! unx4176_casesens — UNX-F4176 大小写敏感全局声明（对照 D3 联签）（诊断段 0x97A0）。
//!
//! 判据原文：
//!   J1 UNX-F4176-J1 "AbC" 与 "abc" 解析为不同条目（1,000 例断言），D3
//!      转换层折叠入场契约锚定（联签边界注记）——B1 只认敏感路径的全局
//!      纪律落账。
//! 设计纪律：
//!   1. B1 解析面恒字节敏感：条目键按原始字节串全等匹配，"AbC" 与
//!      "abc" 是两个独立条目，任何查找路径不做大小写折叠。
//!   2. D3 联签边界：大小写折叠只允许发生在 D3 转换层入场边界，折叠
//!      后的派生键与 B1 原生键同表分账；折叠键撞 B1 键即联签违约，
//!      记 E_FOLD_COLLIDE 拒收。
//!   3. 折叠算法确定性：ASCII 范围内 'A'..'Z' 平移 +0x20，其余字节
//!      原样保留（整数算术，无查表无浮点）。
//!   4. 断言账：敏感区分断言 1,000 例入账，误折叠（敏感失守）计数恒零。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::string::{String, ToString};
use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x97A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_EMPTY_NAME: u16 = SEG_BASE + 1;  // 空条目名拒收
const E_DUP_EXACT: u16 = SEG_BASE + 2;   // 同一字节串重复登记
const E_NOENT: u16 = SEG_BASE + 3;       // 精确查找未命中
const E_FOLD_COLLIDE: u16 = SEG_BASE + 4;// D3 折叠键撞 B1 原生键（联签违约）
const E_FOLD_NOTED: u16 = SEG_BASE + 5;  // 联签边界注记未落账即用折叠入场
const E_LEDGER: u16 = SEG_BASE + 6;      // 敏感账失守（误折叠计数非零）

/// D3 联签边界注记（判据 J1 锚定文本，随账落定）。
pub const D3_FOLD_ANCHOR: &str =
    "D3-FOLD-PACT: 大小写折叠仅发生在 D3 转换层入场边界；B1 解析面恒字节敏感，\
     折叠键不得改写 B1 原生键，撞键即联签违约拒收。";

/// 折叠入场注记落账下限（判据联签锚定次数）。
pub const PACT_NOTES: u32 = 100;
/// 敏感区分断言次数（判据 J1：1,000 例）。
pub const ASSERT_TIMES: u32 = 1000;

/// 条目种类：B1 原生键（字节敏感）与 D3 折叠派生键（边界入场）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyOrigin {
    B1Native, // B1 原生登记（字节敏感面）
    D3Folded, // D3 折叠入场（联签边界派生键）
}

/// 条目状态机：Registered→（联签审查）→Sealed；违约键永不入 Sealed。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyState {
    Registered,
    Sealed,
}

/// 名字条目：键 + 条目号 + 来源 + 状态。
pub struct CaseEntry {
    pub key: String,     // 条目键（B1 原生字节串或 D3 折叠派生串）
    pub id: u32,         // 条目号
    pub origin: KeyOrigin,
    pub state: KeyState,
}

/// 大小写敏感注册表：B1 敏感面 + D3 折叠边界分账。
pub struct CaseTable {
    pub rows: Vec<CaseEntry>,
    pub next_id: u32,        // 条目号发生器
    pub sens_asserts: u32,   // 敏感区分断言数（判据 1,000 例）
    pub sens_faults: u32,    // 误折叠计数（判据要求恒零）
    pub pact_notes: u32,     // 联签边界注记落账数
    pub collide_rejects: u32,// 折叠撞键拒收数（错误账）
}

impl CaseTable {
    pub fn new() -> CaseTable {
        CaseTable {
            rows: Vec::new(),
            next_id: 1,
            sens_asserts: 0,
            sens_faults: 0,
            pact_notes: 0,
            collide_rejects: 0,
        }
    }

    /// B1 原生登记：字节串全等查重，同名（含仅大小写不同的真同串）拒收。
    pub fn register(&mut self, key: &str) -> Result<u32, u16> {
        if key.is_empty() {
            return Err(E_EMPTY_NAME);
        }
        if self.find_exact(key).is_some() {
            return Err(E_DUP_EXACT);
        }
        let id = self.next_id;
        self.next_id += 1;
        self.rows.push(CaseEntry {
            key: key.to_string(),
            id,
            origin: KeyOrigin::B1Native,
            state: KeyState::Registered,
        });
        Ok(id)
    }

    /// B1 精确查找：字节串全等，不做任何折叠（判据 J1 主路径）。
    pub fn lookup_exact(&self, key: &str) -> Option<u32> {
        self.find_exact(key).map(|i| self.rows[i].id)
    }

    fn find_exact(&self, key: &str) -> Option<usize> {
        for i in 0..self.rows.len() {
            if self.rows[i].key == key {
                return Some(i);
            }
        }
        None
    }

    /// D3 折叠入场：先落联签注记，折叠后查重——撞 B1 原生键即违约拒收。
    pub fn d3_fold_enter(&mut self, raw: &str) -> Result<u32, u16> {
        if raw.is_empty() {
            return Err(E_EMPTY_NAME);
        }
        if self.pact_notes == 0 {
            // 联签边界注记未落账不得启用折叠入场。
            return Err(E_FOLD_NOTED);
        }
        let folded = fold_ascii(raw);
        match self.find_exact(&folded) {
            Some(i) => {
                // 撞键即联签违约：无论撞 B1 还是撞 D3 派生键均拒收。
                self.collide_rejects += 1;
                let _ = i;
                Err(E_FOLD_COLLIDE)
            }
            None => {
                let id = self.next_id;
                self.next_id += 1;
                self.rows.push(CaseEntry {
                    key: folded,
                    id,
                    origin: KeyOrigin::D3Folded,
                    state: KeyState::Registered,
                });
                Ok(id)
            }
        }
    }

    /// 联签边界注记落账（启用折叠入场的前置条件）。
    pub fn note_pact(&mut self) {
        self.pact_notes += 1;
    }

    /// 封账：全部 Registered 条目升 Sealed（违约键在入场即被拒，不会到这）。
    pub fn seal_all(&mut self) -> u32 {
        let mut n = 0u32;
        for i in 0..self.rows.len() {
            if self.rows[i].state == KeyState::Registered {
                self.rows[i].state = KeyState::Sealed;
                n += 1;
            }
        }
        n
    }

    /// 敏感区分断言：两键字节不等且各自精确查找结果不等则记 1 例；
    /// 若出现"不同字节串查得同一条目"即误折叠，敏感失守计数 +1。
    pub fn assert_sens(&mut self, a: &str, b: &str) -> Result<(), u16> {
        let ra = self.lookup_exact(a);
        let rb = self.lookup_exact(b);
        if a != b && ra != rb {
            self.sens_asserts += 1;
            Ok(())
        } else {
            self.sens_faults += 1;
            Err(E_LEDGER)
        }
    }

    /// 账面守恒：断言达量、误折叠恒零、联签注记达量。
    pub fn conserved(&self) -> bool {
        self.sens_asserts >= ASSERT_TIMES && self.sens_faults == 0 && self.pact_notes >= PACT_NOTES
    }

    /// 账面导出。
    pub fn snapshot(&self) -> [u32; 5] {
        [
            self.rows.len() as u32,
            self.sens_asserts,
            self.sens_faults,
            self.pact_notes,
            self.collide_rejects,
        ]
    }
}

/// ASCII 大小写折叠：'A'..'Z' 平移 +0x20，其余字节原样（确定性整数算术）。
pub fn fold_ascii(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b >= b'A' && b <= b'Z' {
            out.push((b + 0x20) as char);
        } else {
            out.push(b as char);
        }
    }
    out
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_EMPTY_NAME => "空条目名拒收",
        E_DUP_EXACT => "同一字节串重复登记",
        E_NOENT => "精确查找未命中",
        E_FOLD_COLLIDE => "D3 折叠键撞既有键（联签违约拒收）",
        E_FOLD_NOTED => "联签边界注记未落账即用折叠入场",
        E_LEDGER => "敏感账失守（误折叠计数非零）",
        _ => "未知诊断码",
    }
}

/// 确定性演练：敏感登记→1,000 例区分断言→联签注记→折叠入场与撞键注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut ct = CaseTable::new();

    // R1：B1 原生登记三键——"AbC"/"abc"/"ABC" 三个独立条目。
    let id1 = ct.register("AbC")?;
    let id2 = ct.register("abc")?;
    let _id3 = ct.register("ABC")?;
    if id1 == id2 || ct.lookup_exact("AbC") != Some(id1) || ct.lookup_exact("abc") != Some(id2) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R2：未登记变体精确查找必未命中（敏感面负样本）。
    if ct.lookup_exact("aBc").is_some() || ct.lookup_exact("abC").is_some() {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R3：敏感区分断言 1,000 例——"AbC" vs "abc" 每例两查两异。
    let mut n = 0u32;
    while n < ASSERT_TIMES {
        ct.assert_sens("AbC", "abc")?;
        n += 1;
    }
    if ct.sens_asserts != ASSERT_TIMES || ct.sens_faults != 0 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // R4：联签边界注记落账 100 次后折叠入场才可用。
    if ct.d3_fold_enter("AbC") != Err(E_FOLD_NOTED) {
        return Err(E_FOLD_NOTED);
    }
    let mut m = 0u32;
    while m < PACT_NOTES {
        ct.note_pact();
        m += 1;
    }
    hits += 1;

    // R5：折叠入场派生键——"xyz" 折叠仍 "xyz"，与原生键不同表不撞。
    let idf = ct.d3_fold_enter("xyz")?;
    if ct.lookup_exact("xyz") != Some(idf) {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I1：折叠撞 B1 原生键——"ABC" 折叠成 "abc" 撞已有条目，违约拒收。
    if ct.d3_fold_enter("ABC") != Err(E_FOLD_COLLIDE) {
        return Err(E_FOLD_COLLIDE);
    }
    // I2：折叠撞 D3 派生键——"XYZ" 折叠成 "xyz" 同拒。
    if ct.d3_fold_enter("XYZ") != Err(E_FOLD_COLLIDE) {
        return Err(E_FOLD_COLLIDE);
    }
    if ct.collide_rejects != 2 {
        return Err(E_LEDGER);
    }
    hits += 1;

    // I3：空名与真同串重复登记拒收。
    if ct.register("") != Err(E_EMPTY_NAME) {
        return Err(E_EMPTY_NAME);
    }
    if ct.register("abc") != Err(E_DUP_EXACT) {
        return Err(E_DUP_EXACT);
    }
    hits += 1;

    // R6：封账 + 守恒终审。
    let sealed = ct.seal_all();
    if sealed != ct.rows.len() as u32 {
        return Err(E_LEDGER);
    }
    if !ct.conserved() {
        return Err(E_LEDGER);
    }
    hits += 1;

    Ok(hits)
}
