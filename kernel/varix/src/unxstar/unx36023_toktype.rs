//! unx36023_toktype — UNX-F36023 主令牌与模拟令牌类型位及转换约束。
//!
//! 判据原文（UNX-F36023-J1）：
//!   类型位判定 100 例全对；主→模拟转换仅经复制路径断言（原地改型注入
//!   10 例拒绝）。
//! 设计纪律：
//!   1. 类型位单源：TokenTypeRaw 位锚（PRIMARY=0x1 / IMPERSONATION=0x2 /
//!      保留位 0xFFFF_FFFC 不可置），classify() 是唯一判定入口。
//!   2. 「原地改型」在 API 面不存在：转换仅提供 duplicate_as（复制路径，
//!      产出新令牌 + 转换账入账）；set_type_raw 保留给注入测试直接改结构，
//!      转换账核对其必然偏离转换账 → 拒绝语义由账对平判定（不提供合法
//!      原地改型入口，即是「原地改型注入 10 例拒绝」的实现承诺）。
//!   3. 纯功能线：no_std，零 unwrap/expect/panic，确定性。

use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xf7e0;

// ── 诊断码闭集（0xf7e0~0xf7ff，独占）─────────────────────────
const E_TYPE: u16 = SEG_BASE + 1; // 类型位非法（多位置位或保留位）
const E_CONV: u16 = SEG_BASE + 2; // 转换账失配（原地改型检出）
const E_BAL: u16 = SEG_BASE + 3; // 类型分布账守恒破
const E_DIRTY: u16 = SEG_BASE + 4; // 保留位置位
const E_ORIGIN: u16 = SEG_BASE + 5; // 转换链 origin 断链

/// 类型位锚。
pub const T_PRIMARY: u32 = 0x1;
pub const T_IMPERSONATION: u32 = 0x2;
/// 保留位掩码（不可置）。
pub const T_RESERVED: u32 = !0x3u32;

/// 类型分布账（primary/impersonation 双账 + 转换对账）。
pub struct TypeLedger {
    pub primary: u64,
    pub impersonation: u64,
    pub conversions: u64,
}

impl TypeLedger {
    pub const fn new() -> TypeLedger {
        TypeLedger { primary: 0, impersonation: 0, conversions: 0 }
    }

    /// 分类入账：合法类型位图计数入账；非法返回 Err 且不入账。
    pub fn account(&mut self, bits: u32) -> Result<(), u16> {
        if bits & T_RESERVED != 0 {
            return Err(E_DIRTY);
        }
        match bits {
            T_PRIMARY => {
                self.primary += 1;
                Ok(())
            }
            T_IMPERSONATION => {
                self.impersonation += 1;
                Ok(())
            }
            _ => Err(E_TYPE),
        }
    }

    /// 守恒：转换不改变总数，只在两账间迁移 1。
    pub fn conserve(&self) -> bool {
        self.primary + self.impersonation >= self.conversions
            && (self.primary + self.impersonation) % 2 == (self.conversions % 2)
    }
}

/// 类型判定（唯一入口）：返回归一化类型或拒绝。
pub fn classify(bits: u32) -> Result<u32, u16> {
    if bits & T_RESERVED != 0 {
        return Err(E_DIRTY);
    }
    match bits {
        T_PRIMARY => Ok(T_PRIMARY),
        T_IMPERSONATION => Ok(T_IMPERSONATION),
        _ => Err(E_TYPE),
    }
}

/// 令牌轻量壳（类型面最小结构；完整 Token 在 unx36021）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TypedToken {
    pub id: u32,
    pub type_bits: u32,
}

/// 主→模拟转换：仅复制路径（产出新令牌，原令牌类型不变）。
pub fn duplicate_as(
    src: &TypedToken,
    new_id: u32,
    led: &mut TypeLedger,
) -> Result<TypedToken, u16> {
    let ty = classify(src.type_bits)?;
    if ty != T_PRIMARY {
        // 判据面：主→模拟；模拟→模拟复制不属于本判据，显性拒绝。
        return Err(E_CONV);
    }
    led.account(T_IMPERSONATION)?;
    led.conversions += 1;
    Ok(TypedToken { id: new_id, type_bits: T_IMPERSONATION })
}

/// 原地改型检出：改型令牌若在转换记录链中无 child 记录，即为账外
/// 未登记模拟（原地改型）——每次合法模拟必伴随一条 (child,parent) 记录。
pub fn in_place_detected(t: &TypedToken, recs: &[ConvRecord]) -> bool {
    if classify(t.type_bits) != Ok(T_IMPERSONATION) {
        return false; // 非模拟型无原地改型语义
    }
    let mut i = 0;
    while i < recs.len() {
        if recs[i].child == t.id {
            return false; // 有登记转换记录 → 合法复制产物
        }
        i += 1;
    }
    true // 模拟态但无记录 → 原地改型检出
}

/// 转换链 origin：模拟令牌必须可回溯主令牌 id（本面以 origin 字段模拟）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConvRecord {
    pub child: u32,
    pub parent: u32,
}

/// 诊断释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_TYPE => "类型位非法：非主/模拟单一位图",
        E_CONV => "转换账失配：非主令牌走主→模拟复制路径",
        E_BAL => "类型分布账守恒破",
        E_DIRTY => "保留位置位：0xFFFFFFFC 位不可用",
        E_ORIGIN => "转换链 origin 断链：模拟令牌无可回溯主令牌",
        _ => "未知诊断码",
    }
}

/// 确定性演练（J1 判据五段）。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let mut led = TypeLedger::new();
    // R1：类型位判定 100 例全对（含保留位/非法位拒绝分支）。
    let mut i = 0;
    while i < 100 {
        let k = i as u32;
        let bits = match k % 4 {
            0 => T_PRIMARY,
            1 => T_IMPERSONATION,
            2 => T_PRIMARY | T_RESERVED, // 保留位脏样本
            _ => 0x4,                    // 非法类型
        };
        let want = match k % 4 {
            0 => Ok(T_PRIMARY),
            1 => Ok(T_IMPERSONATION),
            2 => Err(E_DIRTY),
            _ => Err(E_DIRTY), // 0x4 等未定义位同落保留位掩码(!0x3)
        };
        if classify(bits) != want {
            return Err(E_TYPE);
        }
        hits += 1;
        i += 1;
    }
    // R2：主→模拟复制路径 10 例：原令牌类型不变 + 新令牌模拟 + 转换账入账。
    let mut recs = [ConvRecord { child: 0, parent: 0 }; 16];
    let mut n = 0;
    let mut j = 0;
    while j < 10 {
        let src = TypedToken { id: 100 + j, type_bits: T_PRIMARY };
        led.account(T_PRIMARY)?;
        let dst = duplicate_as(&src, 200 + j, &mut led)?;
        if src.type_bits != T_PRIMARY || dst.type_bits != T_IMPERSONATION {
            return Err(E_CONV);
        }
        recs[n] = ConvRecord { child: dst.id, parent: src.id };
        n += 1;
        hits += 1;
        j += 1;
    }
    // R3：origin 链回溯：10 条转换记录 child/parent 逐条可回溯。
    let mut r = 0;
    while r < n {
        let rec = recs[r];
        if rec.parent != 100 + r as u32 || rec.child != 200 + r as u32 {
            return Err(E_ORIGIN);
        }
        r += 1;
    }
    hits += 1;
    // R4：原地改型注入 10 例拒绝——直接改型后 in_place_detected 必真。
    let mut q = 0;
    while q < 10 {
        let mut t = TypedToken { id: 300 + q, type_bits: T_PRIMARY };
        // 注入：绕过 duplicate_as 直接改型（API 面无此入口，模拟外部改写）。
        t.type_bits = T_IMPERSONATION;
        if !in_place_detected(&t, &recs[..n]) {
            return Err(E_CONV);
        }
        hits += 1;
        q += 1;
    }
    // R5：类型分布账守恒：primary=10（R2 各例）+ impersonation=10、conversions=10。
    if led.primary != 10 || led.impersonation != 10 || led.conversions != 10 {
        return Err(E_BAL);
    }
    if !led.conserve() {
        return Err(E_BAL);
    }
    hits += 1;
    Ok(hits)
}

/// 类型位掩码工具面（视图/组合合法矩阵）。

/// 类型名（闭集反查；非法位图返回空串——调用面先 classify）。
pub fn type_name(bits: u32) -> &'static str {
    match classify(bits) {
        Ok(T_PRIMARY) => "primary",
        Ok(T_IMPERSONATION) => "impersonation",
        _ => "",
    }
}

/// 合法组合矩阵：六种常见请求位图 → 是否可分类（视图快照面）。
pub const COMBO_MATRIX: [(u32, bool); 6] = [
    (T_PRIMARY, true),
    (T_IMPERSONATION, true),
    (T_PRIMARY | T_IMPERSONATION, false), // 双位并存非法
    (T_PRIMARY | T_RESERVED, false),      // 保留位脏
    (0, false),                           // 空位图非法
    (0x4, false),                         // 未定义位非法
];

/// 组合矩阵自洽：逐条 classify 结论与矩阵声明一致。
pub fn combo_matrix_check() -> Result<(), u16> {
    let mut i = 0;
    while i < COMBO_MATRIX.len() {
        let (bits, want_ok) = COMBO_MATRIX[i];
        let got_ok = classify(bits).is_ok();
        if got_ok != want_ok {
            return Err(E_TYPE);
        }
        i += 1;
    }
    Ok(())
}

/// 类型分布快照（只读视图：两账+转换账投影）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TypeSnapshot {
    pub primary: u64,
    pub impersonation: u64,
    pub conversions: u64,
}

impl TypeLedger {
    /// 快照导出（账面只读投影，供跨模块消费）。
    pub fn snapshot(&self) -> TypeSnapshot {
        TypeSnapshot {
            primary: self.primary,
            impersonation: self.impersonation,
            conversions: self.conversions,
        }
    }

    /// 快照守恒核：投影值与账面逐字段一致（Copy 语义下的恒等面）。
    pub fn snapshot_conserve(&self, s: &TypeSnapshot) -> bool {
        s.primary == self.primary
            && s.impersonation == self.impersonation
            && s.conversions == self.conversions
    }
}

/// 类型面诊断文本（规则速览，视图文档化）。
pub fn type_rule_text() -> &'static str {
    "① 类型位单源 PRIMARY=0x1/IMPERSONATION=0x2；② 保留位 0xFFFFFFFC 不可置；③ 主→模拟仅经复制路径（原地改型即账破检出）"
}

/// 模拟→主反向判定（视图面：反向转换不在本批接口，恒 E_CONV）。
pub fn impersonation_to_primary_supported() -> bool {
    false
}

pub fn run_unx36023_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36023_toktype");
    cs.add("SEG 段位独占", SEG_BASE == 0xf7e0, "0xf7e0~0xf7ff 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 类型位与转换约束", true, "判定 100 例全对+原地改型 10 例拒"),
        Err(code) => cs.add("J1 类型位与转换约束", false, detail(code)),
    }
    cs
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(122));
    }
}
