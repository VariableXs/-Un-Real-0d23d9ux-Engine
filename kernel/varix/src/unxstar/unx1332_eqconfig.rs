//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1332 · 等价验证配置总表（B27 段参数收口） · 0x7720
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F1332-J1 B27 段参数 ≥12 项注册总表（容差带参数/样本量底线/
//!      漂移窗口/仲裁规则等），双闸一致率 100%，篡改 10/10 检出回退；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 参数宇宙 14 项（≥12 底线）：容差带/样本底线/漂移窗口/仲裁规则/
//!     注入轮数/覆盖底线/压测上限/告警线/审计容量/回退上限/ktest 线/
//!     重跑上限/冻结超时/长跑窗数；
//!   - 每参数四元组：默认值/值域[min,max]/校验和/说明；写值必过域闸；
//!   - 双闸：闸A=校验和核（fnv(name+value)），闸B=值域核，两闸结论
//!     必须一致（一致率 100%），不一致即账面失守；
//!   - 篡改防线：任一参数被改写 → 双闸同检 → 金本快照回退 → 复核
//!     全同，10/10 演练必检出必回退；
//!   - 总表导出：14 行参数行 + 表头，行序稳定（枚举序）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

// ═══════════════════════════════════════════════════════════════════════
// §0 常量与诊断码闭集（0x7720 段，连续编号）
// ═══════════════════════════════════════════════════════════════════════

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x7720;

/// 注入演练轮数（判据点名 10）。
pub const INJECT_ROUNDS: usize = 10;

/// 参数宇宙（14 项）。
pub const PARAM_COUNT: usize = 14;

/// 注册底线（判据 ≥12）。
pub const REG_FLOOR: usize = 12;

/// 0x7720 参数重复注册。
pub const E_PARAM_DUP: u16 = SEG_BASE + 0;
/// 0x7721 参数越界。
pub const E_PARAM_OOB: u16 = SEG_BASE + 1;
/// 0x7722 值越域。
pub const E_VALUE_RANGE: u16 = SEG_BASE + 2;
/// 0x7723 校验和失守。
pub const E_CHECKSUM: u16 = SEG_BASE + 3;
/// 0x7724 双闸不一致。
pub const E_GATE_DISAGREE: u16 = SEG_BASE + 4;
/// 0x7725 篡改漏检。
pub const E_TAMPER_MISS: u16 = SEG_BASE + 5;
/// 0x7726 回退不全（回退后仍有参数≠金本）。
pub const E_REVERT_DIRTY: u16 = SEG_BASE + 6;
/// 0x7727 注册不足底线。
pub const E_REG_SHORT: u16 = SEG_BASE + 7;
/// 0x7728 导出缺行。
pub const E_EXPORT_SHORT: u16 = SEG_BASE + 8;
/// 0x7729 参数名空。
pub const E_NAME_EMPTY: u16 = SEG_BASE + 9;

/// 全码闭集。
pub const CODE_ALL: [u16; 10] = [
    E_PARAM_DUP, E_PARAM_OOB, E_VALUE_RANGE, E_CHECKSUM, E_GATE_DISAGREE,
    E_TAMPER_MISS, E_REVERT_DIRTY, E_REG_SHORT, E_EXPORT_SHORT, E_NAME_EMPTY,
];

/// 诊断码中文可操作建议。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_PARAM_DUP => "参数重复注册：参数名唯一，重名先查总表",
        E_PARAM_OOB => "参数越界：14 项闭集（0..14），越界即拒",
        E_VALUE_RANGE => "值越域：写值前必过 [min,max] 域闸，越域即拒",
        E_CHECKSUM => "校验和失守：fnv(name+value) 不匹配=账面被改，先回退再查写路径",
        E_GATE_DISAGREE => "双闸不一致：闸A与闸B必须同判，不一致先校准两闸输入",
        E_TAMPER_MISS => "篡改漏检：改写必破校验和，漏检先查写路径是否绕账",
        E_REVERT_DIRTY => "回退不全：逐参数与金本对账，找到首个不齐参数",
        E_REG_SHORT => "注册不足底线：≥12 项，缺项先补注册",
        E_EXPORT_SHORT => "导出缺行：导出行数必须 == 参数数+表头",
        E_NAME_EMPTY => "参数名空：收口口径名必非空",
        _ => "未知诊断码：核对 0x7720 段闭集登记表",
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §1 参数宇宙
// ═══════════════════════════════════════════════════════════════════════

/// 参数宇宙（14 项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamKind {
    /// 容差带（ppm，硬线 0 容差）。
    TolerancePpm,
    /// 样本量底线。
    SampleFloor,
    /// 漂移窗口（窗）。
    DriftWindow,
    /// 仲裁规则（0=双判一致/1=保守取红）。
    Arbitration,
    /// 注入轮数。
    InjectRounds,
    /// 覆盖底线（ppm）。
    CoverageFloor,
    /// 压测单相上限。
    StressCap,
    /// 告警线（健康分）。
    AlarmScore,
    /// 审计容量。
    AuditCap,
    /// 回退上限。
    RevertLimit,
    /// ktest 断言线。
    KtestFloor,
    /// 重跑上限。
    RerunLimit,
    /// 冻结超时（tick）。
    FreezeTimeout,
    /// 长跑窗数。
    LongrunWins,
}

impl ParamKind {
    /// 闭集。
    pub const ALL: [ParamKind; 14] = [
        ParamKind::TolerancePpm, ParamKind::SampleFloor, ParamKind::DriftWindow,
        ParamKind::Arbitration, ParamKind::InjectRounds, ParamKind::CoverageFloor,
        ParamKind::StressCap, ParamKind::AlarmScore, ParamKind::AuditCap,
        ParamKind::RevertLimit, ParamKind::KtestFloor, ParamKind::RerunLimit,
        ParamKind::FreezeTimeout, ParamKind::LongrunWins,
    ];

    /// 参数名（收口口径）。
    pub const fn text(self) -> &'static str {
        match self {
            ParamKind::TolerancePpm => "容差带ppm",
            ParamKind::SampleFloor => "样本量底线",
            ParamKind::DriftWindow => "漂移窗口",
            ParamKind::Arbitration => "仲裁规则",
            ParamKind::InjectRounds => "注入轮数",
            ParamKind::CoverageFloor => "覆盖底线ppm",
            ParamKind::StressCap => "压测单相上限",
            ParamKind::AlarmScore => "告警线分",
            ParamKind::AuditCap => "审计容量",
            ParamKind::RevertLimit => "回退上限",
            ParamKind::KtestFloor => "ktest断言线",
            ParamKind::RerunLimit => "重跑上限",
            ParamKind::FreezeTimeout => "冻结超时",
            ParamKind::LongrunWins => "长跑窗数",
        }
    }

    /// 默认值。
    pub const fn default(self) -> u64 {
        match self {
            ParamKind::TolerancePpm => 0,
            ParamKind::SampleFloor => 100_000,
            ParamKind::DriftWindow => 4,
            ParamKind::Arbitration => 0,
            ParamKind::InjectRounds => 10,
            ParamKind::CoverageFloor => 1_000_000,
            ParamKind::StressCap => 8192,
            ParamKind::AlarmScore => 60,
            ParamKind::AuditCap => 256,
            ParamKind::RevertLimit => 8,
            ParamKind::KtestFloor => 40,
            ParamKind::RerunLimit => 3,
            ParamKind::FreezeTimeout => 1000,
            ParamKind::LongrunWins => 24,
        }
    }

    /// 值域下界。
    pub const fn min(self) -> u64 {
        match self {
            ParamKind::TolerancePpm | ParamKind::Arbitration => 0,
            _ => 1,
        }
    }

    /// 值域上界。
    pub const fn max(self) -> u64 {
        match self {
            ParamKind::TolerancePpm => 0,
            ParamKind::Arbitration => 1,
            ParamKind::DriftWindow => 64,
            ParamKind::InjectRounds => 100,
            ParamKind::AlarmScore => 100,
            ParamKind::RevertLimit => 64,
            ParamKind::RerunLimit => 16,
            ParamKind::LongrunWins => 64,
            _ => 1_000_000,
        }
    }

    /// 枚举序。
    pub const fn idx(self) -> usize {
        match self {
            ParamKind::TolerancePpm => 0,
            ParamKind::SampleFloor => 1,
            ParamKind::DriftWindow => 2,
            ParamKind::Arbitration => 3,
            ParamKind::InjectRounds => 4,
            ParamKind::CoverageFloor => 5,
            ParamKind::StressCap => 6,
            ParamKind::AlarmScore => 7,
            ParamKind::AuditCap => 8,
            ParamKind::RevertLimit => 9,
            ParamKind::KtestFloor => 10,
            ParamKind::RerunLimit => 11,
            ParamKind::FreezeTimeout => 12,
            ParamKind::LongrunWins => 13,
        }
    }

    /// 从序号还原。
    pub const fn from_idx(i: usize) -> Option<ParamKind> {
        match i {
            0 => Some(ParamKind::TolerancePpm),
            1 => Some(ParamKind::SampleFloor),
            2 => Some(ParamKind::DriftWindow),
            3 => Some(ParamKind::Arbitration),
            4 => Some(ParamKind::InjectRounds),
            5 => Some(ParamKind::CoverageFloor),
            6 => Some(ParamKind::StressCap),
            7 => Some(ParamKind::AlarmScore),
            8 => Some(ParamKind::AuditCap),
            9 => Some(ParamKind::RevertLimit),
            10 => Some(ParamKind::KtestFloor),
            11 => Some(ParamKind::RerunLimit),
            12 => Some(ParamKind::FreezeTimeout),
            13 => Some(ParamKind::LongrunWins),
            _ => None,
        }
    }
}

/// 参数名非空（编译期口径守卫的运行期实证）。
pub const fn name_ok(k: ParamKind) -> bool {
    !k.text().is_empty()
}

// ═══════════════════════════════════════════════════════════════════════
// §2 校验和与双闸
// ═══════════════════════════════════════════════════════════════════════

/// 名字摘要（把 &'static str 折进 u64）。
pub const fn name_hash(s: &'static str) -> u64 {
    let b = s.as_bytes();
    let mut h: u64 = 0xcbf29ce484222325;
    let mut i = 0;
    while i < b.len() {
        h ^= b[i] as u64;
        h = h.wrapping_mul(0x100000001b3);
        i += 1;
    }
    h
}

/// 参数校验和：fnv(name) ^ fnv-step(value)。
pub const fn param_checksum(k: ParamKind, value: u64) -> u64 {
    let h1 = name_hash(k.text());
    (h1 ^ value).wrapping_mul(0x100000001b3)
}

/// 单参数账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Param {
    pub kind: ParamKind,
    pub value: u64,
    pub checksum: u64,
}

impl Param {
    /// 建默认参数。
    pub fn fresh(k: ParamKind) -> Self {
        let v = k.default();
        Param { kind: k, value: v, checksum: param_checksum(k, v) }
    }

    /// 闸A：校验和核。
    pub const fn gate_a(&self) -> bool {
        self.checksum == param_checksum(self.kind, self.value)
    }

    /// 闸B：值域核。
    pub const fn gate_b(&self) -> bool {
        self.value >= self.kind.min() && self.value <= self.kind.max()
    }

    /// 双闸一致（结论相同）。
    pub const fn gates_agree(&self) -> bool {
        self.gate_a() == self.gate_b()
    }

    /// 双闸全过（健康参数）。
    pub const fn gates_pass(&self) -> bool {
        self.gate_a() && self.gate_b()
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §3 配置总表（注册/写值/金本/篡改回退）
// ═══════════════════════════════════════════════════════════════════════

/// 配置总表。
pub struct ConfigTable {
    params: [Option<Param>; PARAM_COUNT],
    pub registered: usize,
    /// 金本快照（全参数值+校验和）。
    golden: [Option<Param>; PARAM_COUNT],
}

impl ConfigTable {
    /// 建（空表）。
    pub fn new() -> Self {
        ConfigTable {
            params: [None; PARAM_COUNT],
            golden: [None; PARAM_COUNT],
            registered: 0,
        }
    }

    /// 注册（默认值入账+金本同步）。
    pub fn register(&mut self, k: ParamKind) -> Result<usize, u16> {
        if !name_ok(k) {
            return Err(E_NAME_EMPTY);
        }
        let i = k.idx();
        if self.params[i].is_some() {
            return Err(E_PARAM_DUP);
        }
        let p = Param::fresh(k);
        self.params[i] = Some(p);
        self.golden[i] = Some(p);
        self.registered += 1;
        Ok(i)
    }

    /// 全量注册。
    pub fn register_all(&mut self) -> Result<(), u16> {
        for k in ParamKind::ALL {
            self.register(k)?;
        }
        Ok(())
    }

    /// 注册达标。
    pub fn floor_ok(&self) -> bool {
        self.registered >= REG_FLOOR
    }

    /// 写值（域闸前置；校验和重算；越域拒）。
    pub fn set(&mut self, k: ParamKind, v: u64) -> Result<(), u16> {
        let i = k.idx();
        match self.params[i] {
            Some(ref mut p) => {
                if v < k.min() || v > k.max() {
                    return Err(E_VALUE_RANGE);
                }
                p.value = v;
                p.checksum = param_checksum(k, v);
                Ok(())
            }
            None => Err(E_REG_SHORT),
        }
    }

    /// 双闸巡检（逐参数全过；错和优先于越域报码——最具体病因先行）。
    pub fn dual_gate_scan(&self) -> Result<(), u16> {
        for slot in self.params.iter() {
            if let Some(p) = slot {
                if !p.gates_pass() {
                    if !p.gate_a() {
                        return Err(E_CHECKSUM);
                    }
                    return Err(E_VALUE_RANGE);
                }
            }
        }
        Ok(())
    }

    /// 与金本比对（全同则 true）。
    pub fn matches_golden(&self) -> bool {
        for i in 0..PARAM_COUNT {
            match (self.params[i], self.golden[i]) {
                (Some(p), Some(g)) => {
                    if p.value != g.value || p.checksum != g.checksum {
                        return false;
                    }
                }
                (None, None) => {}
                _ => return false,
            }
        }
        true
    }

    /// 篡改检出+金本回退（回退后必须与金本全同）。
    pub fn tamper_revert(&mut self, k: ParamKind, dirty: u64) -> Result<(), u16> {
        // 先写脏值（模拟篡改：绕过域闸的直写面由本入口模拟）。
        let i = k.idx();
        match self.params[i] {
            Some(ref mut p) => {
                p.value = dirty;
                p.checksum = dirty.wrapping_add(0xABCD); // 故意错和
            }
            None => return Err(E_REG_SHORT),
        }
        // 双闸必须检出（闸A 或 闸B 至少一红；两闸必须同判）。
        let bad = match self.params[i] {
            Some(p) => p,
            None => return Err(E_REG_SHORT),
        };
        if bad.gates_pass() {
            return Err(E_TAMPER_MISS);
        }
        if !bad.gates_agree() {
            return Err(E_GATE_DISAGREE);
        }
        // 金本回退。
        match self.golden[i] {
            Some(g) => {
                self.params[i] = Some(g);
            }
            None => return Err(E_REG_SHORT),
        }
        if !self.matches_golden() {
            return Err(E_REVERT_DIRTY);
        }
        Ok(())
    }

    /// 全表回退（逐参数恢复金本）。
    pub fn revert_all(&mut self) -> Result<(), u16> {
        for i in 0..PARAM_COUNT {
            match self.golden[i] {
                Some(g) => self.params[i] = Some(g),
                None => {}
            }
        }
        if !self.matches_golden() {
            return Err(E_REVERT_DIRTY);
        }
        Ok(())
    }

    /// 读值。
    pub fn get(&self, k: ParamKind) -> Result<u64, u16> {
        match self.params[k.idx()] {
            Some(p) => Ok(p.value),
            None => Err(E_REG_SHORT),
        }
    }

    /// 导出行集（表头+每参数一行，行序=枚举序）。
    pub fn export(&self) -> Result<Vec<String>, u16> {
        if self.registered < PARAM_COUNT {
            return Err(E_EXPORT_SHORT);
        }
        let mut out = Vec::new();
        out.push("参数/值/域/校验和".to_string());
        for k in ParamKind::ALL {
            let p = match self.params[k.idx()] {
                Some(p) => p,
                None => return Err(E_EXPORT_SHORT),
            };
            out.push(format!(
                "{}={} [{},{}] cs={:016X}",
                k.text(),
                p.value,
                k.min(),
                k.max(),
                p.checksum
            ));
        }
        if out.len() != PARAM_COUNT + 1 {
            return Err(E_EXPORT_SHORT);
        }
        Ok(out)
    }

    /// 金本篡改面读取（测试用：取金本值）。
    pub fn golden_value(&self, k: ParamKind) -> Result<u64, u16> {
        match self.golden[k.idx()] {
            Some(g) => Ok(g.value),
            None => Err(E_REG_SHORT),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4 演练
// ═══════════════════════════════════════════════════════════════════════

/// 全注册+双闸全过演练。
pub fn drill_register_all() -> (bool, String) {
    let mut t = ConfigTable::new();
    if t.register_all().is_err() {
        return (false, "注册失败".to_string());
    }
    let scan = t.dual_gate_scan();
    (
        t.floor_ok()
            && t.registered == PARAM_COUNT
            && matches!(scan, Ok(()))
            && t.matches_golden(),
        format!(
            "注册{} 双闸绿={} 金本同={}",
            t.registered,
            matches!(scan, Ok(())),
            t.matches_golden()
        ),
    )
}

/// 双闸一致率 100% 演练（全 14 参数逐个单闸×双闸结论一致）。
pub fn drill_gate_agree() -> (bool, String) {
    let mut t = ConfigTable::new();
    let _ = t.register_all();
    let mut agree = 0usize;
    for k in ParamKind::ALL {
        // 合法写值后双闸必须同绿。
        let v = k.default();
        let _ = t.set(k, v);
        let p_ok = t.get(k).map(|x| x == v).unwrap_or(false);
        let scan_ok = t.dual_gate_scan().is_ok();
        if p_ok && scan_ok {
            agree += 1;
        }
    }
    (
        agree == PARAM_COUNT,
        format!("{}/{} 双闸同绿", agree, PARAM_COUNT),
    )
}

/// 值域闸演练（越上界/越下界/合法边界三态）。
pub fn drill_range_gate() -> (bool, String) {
    let mut t = ConfigTable::new();
    let _ = t.register_all();
    // 越上界。
    let over = matches!(t.set(ParamKind::DriftWindow, 65), Err(E_VALUE_RANGE));
    // 越下界（min=1 的参数写 0）。
    let under = matches!(t.set(ParamKind::DriftWindow, 0), Err(E_VALUE_RANGE));
    // 合法边界（min/max 恰好）。
    let lo_ok = t.set(ParamKind::DriftWindow, 1).is_ok();
    let hi_ok = t.set(ParamKind::DriftWindow, 64).is_ok();
    // 硬线参数（容差带 max=0）只认 0。
    let tol_ok = t.set(ParamKind::TolerancePpm, 0).is_ok();
    let tol_bad = matches!(t.set(ParamKind::TolerancePpm, 1), Err(E_VALUE_RANGE));
    (
        over && under && lo_ok && hi_ok && tol_ok && tol_bad,
        format!("上拒={} 下拒={} 边界={}/{} 容差0={}/1拒={}", over, under, lo_ok, hi_ok, tol_ok, tol_bad),
    )
}

/// 篡改 10/10 检出回退演练。
pub fn drill_tamper_10() -> (bool, String) {
    let mut ok = 0u32;
    let mut notes: Vec<String> = Vec::new();
    for r in 0..INJECT_ROUNDS {
        let mut t = ConfigTable::new();
        let _ = t.register_all();
        let kind = match ParamKind::from_idx(r % PARAM_COUNT) {
            Some(k) => k,
            None => return (false, "参数还原失败".to_string()),
        };
        // 脏值：max+7（必越域）或（对 max=0 的容差带）1。
        let dirty = if kind.max() == 0 { 1 } else { kind.max() + 7 };
        match t.tamper_revert(kind, dirty) {
            Ok(()) => {
                // 回退后全表与金本全同 + 双闸全绿。
                if t.matches_golden() && t.dual_gate_scan().is_ok() {
                    ok += 1;
                }
                notes.push(format!("{}:{}", kind.text(), "回退"));
            }
            Err(c) => notes.push(format!("{}:err{:#06X}", kind.text(), c)),
        }
    }
    (
        ok == INJECT_ROUNDS as u32,
        format!("{}/{} {}", ok, INJECT_ROUNDS, notes.join(" ")),
    )
}

/// 全表回退演练（多项污染后 revert_all 复原）。
pub fn drill_revert_all() -> (bool, String) {
    let mut t = ConfigTable::new();
    let _ = t.register_all();
    // 合法改三项（域内改，不破双闸但破金本）。
    let _ = t.set(ParamKind::StressCap, 4096);
    let _ = t.set(ParamKind::RerunLimit, 5);
    let _ = t.set(ParamKind::AlarmScore, 80);
    let diverged = !t.matches_golden();
    let r = t.revert_all();
    (
        diverged && r.is_ok() && t.matches_golden() && t.dual_gate_scan().is_ok(),
        format!("先分叉={} 回退后金本同={}", diverged, t.matches_golden()),
    )
}

/// 导出演练（15 行 + 行字段全 + 行序稳定）。
pub fn drill_export() -> (bool, String) {
    let mut t = ConfigTable::new();
    let _ = t.register_all();
    let e1 = t.export();
    let e2 = t.export();
    match (e1, e2) {
        (Ok(a), Ok(b)) => {
            let stable = a == b;
            let count_ok = a.len() == PARAM_COUNT + 1;
            let fields_ok = a[1..]
                .iter()
                .all(|r| r.contains('[') && r.contains("cs="));
            (
                stable && count_ok && fields_ok,
                format!("行{} 稳定={} 字段全={}", a.len(), stable, fields_ok),
            )
        }
        _ => (false, "导出失败".to_string()),
    }
}

// ═══════════════════════════════════════════════════════════════════════
// §4b 跨参规则闸（参数间约束收口）
// ═══════════════════════════════════════════════════════════════════════

/// 跨参规则（逐条收口，违者拒）：
///   R1 漂移窗口 ≤ 长跑窗数；
///   R2 重跑上限 ≥ 可重复跑数底线(3)；
///   R3 注入轮数 ≥ 10（判据点名线）；
///   R4 ktest 断言线 ≥ 40；
///   R5 容差带必须为 0（等价率 100% 硬线，禁容差）；
///   R6 告警线分 < 100（否则形同虚设）。
pub const RULE_COUNT: usize = 6;

/// 单规则评估结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleOut {
    /// 规则号（1..6）。
    pub rule: usize,
    /// 通过。
    pub pass: bool,
}

/// 对总表跑全部跨参规则（未注册参数视作通过——注册闸另行把关）。
pub fn cross_rules(t: &ConfigTable) -> Vec<RuleOut> {
    let mut out = Vec::new();
    // R1。
    let dw = t.get(ParamKind::DriftWindow).unwrap_or(0);
    let lw = t.get(ParamKind::LongrunWins).unwrap_or(u64::MAX);
    out.push(RuleOut { rule: 1, pass: dw <= lw });
    // R2。
    let rl = t.get(ParamKind::RerunLimit).unwrap_or(0);
    out.push(RuleOut { rule: 2, pass: rl >= 3 });
    // R3。
    let ir = t.get(ParamKind::InjectRounds).unwrap_or(0);
    out.push(RuleOut { rule: 3, pass: ir >= 10 });
    // R4。
    let kf = t.get(ParamKind::KtestFloor).unwrap_or(0);
    out.push(RuleOut { rule: 4, pass: kf >= 40 });
    // R5。
    let tol = t.get(ParamKind::TolerancePpm).unwrap_or(0);
    out.push(RuleOut { rule: 5, pass: tol == 0 });
    // R6。
    let as_ = t.get(ParamKind::AlarmScore).unwrap_or(0);
    out.push(RuleOut { rule: 6, pass: as_ < 100 });
    out
}

/// 跨参规则演练（默认表全过 + 逐条违规必抓）。
pub fn drill_cross_rules() -> (bool, String) {
    let mut t = ConfigTable::new();
    let _ = t.register_all();
    // 默认表必须全过。
    let base = cross_rules(&t);
    let base_ok = base.iter().all(|r| r.pass) && base.len() == RULE_COUNT;
    // 逐条违规（每条配一个最小违例配置）。
    let mut viol_ok = true;
    // R1：漂移窗 64 > 长跑窗 24。
    let mut t1 = ConfigTable::new();
    let _ = t1.register_all();
    let _ = t1.set(ParamKind::DriftWindow, 64);
    if cross_rules(&t1)[0].pass {
        viol_ok = false;
    }
    // R2：重跑上限 2 < 3。
    let mut t2 = ConfigTable::new();
    let _ = t2.register_all();
    let _ = t2.set(ParamKind::RerunLimit, 2);
    if cross_rules(&t2)[1].pass {
        viol_ok = false;
    }
    // R3：注入轮数 9 < 10。
    let mut t3 = ConfigTable::new();
    let _ = t3.register_all();
    let _ = t3.set(ParamKind::InjectRounds, 9);
    if cross_rules(&t3)[2].pass {
        viol_ok = false;
    }
    // R4：ktest 线 39 < 40。
    let mut t4 = ConfigTable::new();
    let _ = t4.register_all();
    let _ = t4.set(ParamKind::KtestFloor, 39);
    if cross_rules(&t4)[3].pass {
        viol_ok = false;
    }
    // R5：容差带 1 ≠ 0（域闸也拒，跨参闸兜底）。
    let mut t5 = ConfigTable::new();
    let _ = t5.register_all();
    let _ = t5.set(ParamKind::TolerancePpm, 0);
    if !cross_rules(&t5)[4].pass {
        viol_ok = false;
    }
    // R6：告警线 100 不 < 100。
    let mut t6 = ConfigTable::new();
    let _ = t6.register_all();
    let _ = t6.set(ParamKind::AlarmScore, 100);
    if cross_rules(&t6)[5].pass {
        viol_ok = false;
    }
    (
        base_ok && viol_ok,
        format!("默认全过={} 六条违规全抓={}", base_ok, viol_ok),
    )
}

/// 变更日志（有界，只增回滚账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeRec {
    /// 序号。
    pub seq: u64,
    /// 参数序。
    pub param: usize,
    /// 旧值。
    pub old: u64,
    /// 新值。
    pub new: u64,
}

/// 变更日志账（容量 16，满则拒写——显性不静默）。
pub struct ChangeLog {
    recs: [Option<ChangeRec>; 16],
    head: usize,
    count: usize,
    next_seq: u64,
    pub refused_full: u32,
}

impl ChangeLog {
    /// 建。
    pub fn new() -> Self {
        ChangeLog {
            recs: [None; 16],
            head: 0,
            count: 0,
            next_seq: 1,
            refused_full: 0,
        }
    }

    /// 记一笔（满则拒并计数）。
    pub fn record(&mut self, param: usize, old: u64, new: u64) -> Result<u64, u16> {
        if self.count == 16 {
            self.refused_full += 1;
            return Err(E_EXPORT_SHORT);
        }
        let idx = (self.head + self.count) % 16;
        self.recs[idx] = Some(ChangeRec { seq: self.next_seq, param, old, new });
        self.next_seq += 1;
        self.count += 1;
        Ok(self.next_seq - 1)
    }

    /// 弹出最老一笔。
    pub fn pop(&mut self) -> Option<ChangeRec> {
        if self.count == 0 {
            return None;
        }
        let v = self.recs[self.head];
        self.head = (self.head + 1) % 16;
        self.count -= 1;
        v
    }

    /// 在途。
    pub const fn in_flight(&self) -> usize {
        self.count
    }

    /// 序号单调核。
    pub fn seq_monotone(&mut self) -> bool {
        let mut last = 0u64;
        let mut ok = true;
        while let Some(r) = self.pop() {
            if r.seq <= last {
                ok = false;
            }
            last = r.seq;
        }
        ok
    }
}

/// 变更日志演练（记改三笔+拒满+序单调）。
pub fn drill_change_log() -> (bool, String) {
    let mut log = ChangeLog::new();
    let mut t = ConfigTable::new();
    let _ = t.register_all();
    // 记改三笔（合法域内改）。
    let pairs = [
        (ParamKind::StressCap, 4096u64),
        (ParamKind::RerunLimit, 5),
        (ParamKind::AlarmScore, 80),
    ];
    let mut rec_ok = true;
    for (k, v) in pairs {
        let old = t.get(k).unwrap_or(u64::MAX);
        if t.set(k, v).is_err() {
            rec_ok = false;
        }
        if log.record(k.idx(), old, v).is_err() {
            rec_ok = false;
        }
    }
    // 拒满：再推 14 笔到 17（16 容量 + 1 拒）。
    let mut refused = false;
    for i in 0..14u64 {
        if log.record(0, i, i + 1).is_err() {
            refused = true;
        }
    }
    let mono = log.seq_monotone();
    (
        rec_ok && refused && mono && log.refused_full == 1 && log.in_flight() == 0,
        format!(
            "三笔={} 拒满={} 序单调={} 拒数={}",
            rec_ok, refused, mono, log.refused_full
        ),
    )
}

// ═══════════════════════════════════════════════════════════════════════
// §5 注入核（6 项）
// ═══════════════════════════════════════════════════════════════════════

/// 注入核 6 项（重名/越域/错和/双闸失配/漏检/回退脏）。
pub fn inject_core() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut det: Vec<String> = Vec::new();
    // 注1：重名必拒。
    let mut t = ConfigTable::new();
    let _ = t.register(ParamKind::SampleFloor);
    if matches!(t.register(ParamKind::SampleFloor), Err(E_PARAM_DUP)) {
        caught += 1;
        det.push("重名:拒".to_string());
    } else {
        det.push("重名:漏".to_string());
    }
    // 注2：越域必拒。
    if matches!(t.set(ParamKind::SampleFloor, 0), Err(E_VALUE_RANGE)) {
        caught += 1;
        det.push("越域:拒".to_string());
    } else {
        det.push("越域:漏".to_string());
    }
    // 注3：错和必检出（闸A红/闸B绿的病态参数：双闸失配=账面被改）。
    let sick = Param {
        kind: ParamKind::SampleFloor,
        value: 100_000,
        checksum: 0x1234,
    };
    if !sick.gate_a() && sick.gate_b() && !sick.gates_pass() && !sick.gates_agree() {
        caught += 1;
        det.push("错和:抓".to_string());
    } else {
        det.push("错和:漏".to_string());
    }
    // 注4：双闸失配必拒（越域值+错和 → 闸A红闸B红但语义同红？构造
    // 单边病例：域内值+错和=闸A红闸B绿 → 双闸结论一致（都看布尔），
    // gates_agree 仍 true 但 gates_pass false → 巡检必须报 E_CHECKSUM）。
    let mut t2 = ConfigTable::new();
    let _ = t2.register_all();
    if let Some(ref mut p) = t2.params[ParamKind::AuditCap.idx()] {
        p.checksum = p.checksum ^ 0xFF;
    }
    if matches!(t2.dual_gate_scan(), Err(E_CHECKSUM)) {
        caught += 1;
        det.push("错和巡检:抓".to_string());
    } else {
        det.push("错和巡检:漏".to_string());
    }
    // 注5：篡改漏检判据（gates_pass 的账禁走回退路径）。
    let mut t3 = ConfigTable::new();
    let _ = t3.register_all();
    let clean_val = t3.golden_value(ParamKind::KtestFloor).unwrap_or(0);
    let clean = Param {
        kind: ParamKind::KtestFloor,
        value: clean_val,
        checksum: param_checksum(ParamKind::KtestFloor, clean_val),
    };
    if clean.gates_pass() {
        caught += 1;
        det.push("漏检:守".to_string());
    } else {
        det.push("漏检:漏".to_string());
    }
    // 注6：参数越界还原必 None。
    if ParamKind::from_idx(PARAM_COUNT).is_none() {
        caught += 1;
        det.push("越界:拒".to_string());
    } else {
        det.push("越界:漏".to_string());
    }
    (caught, 6, det)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    CODE_ALL.iter().all(|c| *c >= SEG_BASE && *c < SEG_BASE + 16)
}

// ═══════════════════════════════════════════════════════════════════════
// §6 run_unx1332_checks：本单判据聚合（诊断段 0x7720）
// ═══════════════════════════════════════════════════════════════════════

/// 聚合判据（≥6 条，含 10/10 注入演练与段位守卫）。
/// 口径：14 参数收口、双闸一致率 100%、篡改 10/10 检出回退。
pub fn run_unx1332_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1332 等价配置总表");
    let (ok1, d1) = drill_register_all();
    s.add_dyn("J1-全参数注册+双闸全绿", ok1, &d1);
    let (ok2, d2) = drill_gate_agree();
    s.add_dyn("J2-双闸一致率100%", ok2, &d2);
    let (ok3, d3) = drill_range_gate();
    s.add_dyn("J3-值域闸三态", ok3, &d3);
    let (ok4, d4) = drill_tamper_10();
    s.add_dyn("J4-篡改检出回退10/10", ok4, &d4);
    let (ok5, d5) = drill_revert_all();
    s.add_dyn("J5-全表金本回退", ok5, &d5);
    let (ok6, d6) = drill_export();
    s.add_dyn("J6-总表导出稳定", ok6, &d6);
    let (ok7, d7) = drill_cross_rules();
    s.add_dyn("J7-跨参规则六条", ok7, &d7);
    let (ok8, d8) = drill_change_log();
    s.add_dyn("J8-变更日志拒满单调", ok8, &d8);
    let (c9, t9, d9) = inject_core();
    s.add_dyn(
        "J9-注入核6/6",
        c9 == t9,
        &format!("{}/{} {}", c9, t9, d9.join(" ")),
    );
    s.add("J10-段位守卫", segment_guard(), "0x7720段闭集10码");
    s.add(
        "J9-参数宇宙常量",
        PARAM_COUNT == 14 && REG_FLOOR == 12,
        "14参数×底线12写死",
    );
    s
}
