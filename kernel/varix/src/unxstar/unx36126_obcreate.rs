//! UNX-F36126-J1 AccessCheck 引擎装配挂点（ObCreateObject 联签面）。
//! 判据：① 引擎就绪门（双面对账 + 黄金抽查）未就绪一律拒创建；
//! ② 就绪后按类型档位合成默认 SD 并联签入账（账目可重放）；
//! ③ 未注册类型创建显性拒（36122 红线在创建面复刻）；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：装配挂点真源——对象创建路径上「类型面 → 策略面 → SD 面」
//! 的唯一联签点。消费：类型表/映射表 36121/36123（配对真源）、闸门 36122、
//! 默认 SD 36125。产出 CreateOutcome 供 B09（AccessCheck 36161~）以
//! 「对象带 SD 进检查」的消费形态接续；创建账供 36130 审计面与 36136
//! 事件账消费。

use crate::checks::CheckSet;
use alloc::vec::Vec;
use super::unx36061_sdcore::Sd;
use super::unx36063_sdctor::SectSpec;
use super::unx36101_masktable as mt;
use super::unx36102_stdrights as sr;
use super::unx36104_genmap::Registry as MapReg;
use super::unx36121_typereg::{self as tr, TypePolicy, TypeRegistry, GOLDEN_N};
use super::unx36122_typedeny::{DenyLedger, GateRoute};
use super::unx36123_genbind::{face_audit, spot_audit};
use super::unx36125_deftier::{
    synthesize_for_type, synthesize_verified, tier_signature, verify_shape, verify_tier,
};

pub const SEG_BASE: u16 = 0x3b20;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_NOTREADY: u16 = SEG_BASE + 2; // 引擎未就绪（就绪门未过）
pub const E_DENY: u16 = SEG_BASE + 3; // 创建被闸门拒绝（未注册/名非法）
pub const E_SYN: u16 = SEG_BASE + 4; // 默认 SD 合成失败
pub const E_TIER: u16 = SEG_BASE + 5; // 合成后档位复核不过
pub const E_LEDGER: u16 = SEG_BASE + 6; // 创建账漂移
pub const E_INTG: u16 = SEG_BASE + 7; // 引擎状态矛盾
pub const E_NOENT: u16 = SEG_BASE + 8; // 类型面缺位（联签前段失败）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 创建账容量（联签事件上限；满即拒创建——高价值事件不顶替）。
pub const LEDGER_CAP: usize = 24;

/// 创建联签回执。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CreateOutcome {
    pub seq: u32,
    pub tid: u16,
    pub policy: TypePolicy, // 档位（SD 形状的裁决依据）
    pub ctrl: u16,          // 合成 SD 控制位（联签快照）
    pub creator: u16,       // 创建者标签（审计联签第二签）
}

/// AccessCheck 引擎装配态（四件套 + 就绪门）。
pub struct Engine {
    pub reg: TypeRegistry,
    pub maps: MapReg,
    pub denied: DenyLedger,
    ledger: [Option<CreateOutcome>; LEDGER_CAP],
    n: usize,
    ready: bool,
}

const NO_OUT: Option<CreateOutcome> = None;

impl Engine {
    /// 装配（配对黄金装配 + 就绪门评估）。
    pub fn assemble() -> Result<Engine, u16> {
        let (reg, maps) = super::unx36123_genbind::assemble_golden_paired()?;
        let mut e = Engine {
            reg,
            maps,
            denied: DenyLedger::new(),
            ledger: [NO_OUT; LEDGER_CAP],
            n: 0,
            ready: false,
        };
        e.readiness_gate()?;
        Ok(e)
    }

    /// 就绪门：双面对账全过 + 黄金八类逐名抽查 + 档位特征可合成。
    pub fn readiness_gate(&mut self) -> Result<(), u16> {
        face_audit(&self.reg, &self.maps).map_err(|_| E_NOTREADY)?;
        let mut i = 0;
        while i < GOLDEN_N {
            spot_audit(&self.reg, &self.maps, tr::GOLDEN_NAMES[i]).map_err(|_| E_NOTREADY)?;
            let policy = self.reg.policy_of(tr::GOLDEN_NAMES[i]).map_err(|_| E_NOTREADY)?;
            let (sd, bodies) = synthesize_verified(policy).map_err(|_| E_NOTREADY)?;
            verify_shape(&sd, &bodies, policy).map_err(|_| E_NOTREADY)?;
            i += 1;
        }
        self.ready = true;
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn ledger_count(&self) -> usize {
        self.n
    }

    /// ObCreateObject 联签面：类型面准入 → 默认 SD 合成 → 双复核 → 入账。
    /// 未注册类型 → E_DENY（拒绝入拒绝账，不创建、不入创建账）。
    pub fn ob_create(&mut self, type_name: &'static str, creator: u16) -> Result<CreateOutcome, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if !self.ready {
            return Err(E_NOTREADY);
        }
        if self.n >= LEDGER_CAP {
            return Err(E_LEDGER);
        }
        // 联签第一段：闸门（36122）。
        let route = super::unx36122_typedeny::access_gate(&self.reg, type_name);
        if route.is_deny() {
            self.denied.record(route.code(), type_name);
            return Err(E_DENY);
        }
        // 联签第二段：类型面（tid + 档位）。
        let entry = self.reg.query(type_name).map_err(|_| E_NOENT)?;
        // 联签第三段：默认 SD 合成 + 双复核（36125）。
        let (sd, bodies) = synthesize_for_type(&self.reg, type_name).map_err(|_| E_SYN)?;
        verify_tier(&sd, entry.policy).map_err(|_| E_TIER)?;
        verify_shape(&sd, &bodies, entry.policy).map_err(|_| E_TIER)?;
        // 联签第四段：入账（seq 连续）。
        let out = CreateOutcome {
            seq: self.n as u32 + 1,
            tid: entry.tid,
            policy: entry.policy,
            ctrl: sd.control,
            creator,
        };
        self.ledger[self.n] = Some(out);
        self.n += 1;
        Ok(out)
    }

    /// 按序位取回执。
    pub fn at(&self, i: usize) -> Result<CreateOutcome, u16> {
        if i >= self.n {
            return Err(E_LEDGER);
        }
        match self.ledger[i] {
            Some(o) => Ok(o),
            None => Err(E_LEDGER),
        }
    }

    /// 创建账守恒：seq 严格递增 + 档位复核 + 无空洞。
    pub fn ledger_integrity(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.n {
            let o = match self.ledger[i] {
                Some(o) => o,
                None => return Err(E_LEDGER),
            };
            if o.seq != i as u32 + 1 {
                return Err(E_LEDGER);
            }
            let (must, must_not) = tier_signature(o.policy);
            if o.ctrl & must != must || o.ctrl & must_not != 0 {
                return Err(E_TIER);
            }
            i += 1;
        }
        Ok(())
    }

    /// 就绪门失效注入（自检面）：装配后若注册表被污染，就绪门重评必须显性失败。
    pub fn readiness_recheck(&self) -> Result<(), u16> {
        face_audit(&self.reg, &self.maps).map_err(|_| E_NOTREADY)
    }
}

/// 创建期授权掩码面（B12 句柄路径的创建侧种子）：对象创建瞬间，创建者
/// 按档位获得初始授权掩码（打开时全查 / 用句柄时掩码过滤的分工契约见
/// 36176——创建授权是句柄掩码的种子，不绕过打开时检查）。
/// 档位 → 创建授权投影：
///   ClosedAll     → 0（全闭档创建者也不得豁免——默认拒绝红线）；
///   NullDacl      → READ_CONTROL | SYNCHRONIZE（读+等：空授权域的基本通行）；
///   InheritParent → sr::STD_ALL（继承档创建者全标准五位）；
///   AdminGate     → READ_CONTROL | WRITE_DAC | SYNCHRONIZE（管理门不含
///                    WRITE_OWNER——接管须走显式授权面）。
pub fn create_grant(policy: TypePolicy) -> u32 {
    match policy {
        TypePolicy::ClosedAll => 0,
        TypePolicy::NullDacl => sr::READ_CONTROL | sr::SYNCHRONIZE,
        TypePolicy::InheritParent => sr::STD_ALL,
        TypePolicy::AdminGate => sr::READ_CONTROL | sr::WRITE_DAC | sr::SYNCHRONIZE,
    }
}

/// 创建授权守恒核：掩码必落 B06 合法区 ∧ 全闭档恒零。
pub fn verify_grant(policy: TypePolicy, grant: u32) -> Result<(), u16> {
    if grant & !mt::ALL_LEGAL != 0 {
        return Err(E_INTG);
    }
    if policy == TypePolicy::ClosedAll && grant != 0 {
        return Err(E_INTG);
    }
    Ok(())
}

/// 回执 + 创建授权的联签快照（B12 消费锚：句柄表初始化按此种子）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrantSnap {
    pub seq: u32,
    pub grant: u32,
}

/// 全账授权快照（重放面：同序创建必得同授权序列）。
pub fn grant_snapshot(eng: &Engine) -> Result<Vec<GrantSnap>, u16> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < eng.ledger_count() {
        let o = eng.at(i)?;
        let g = create_grant(o.policy);
        verify_grant(o.policy, g)?;
        v.push(GrantSnap { seq: o.seq, grant: g });
        i += 1;
    }
    Ok(v)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：装配即就绪（黄金配对装配过就绪门）。
    let mut eng = Engine::assemble()?;
    if !eng.is_ready() {
        return Err(E_NOTREADY);
    }
    // R2：黄金八类逐类创建联签（档位特征逐类核对）。
    let mut i = 0;
    while i < GOLDEN_N {
        let name = tr::GOLDEN_NAMES[i];
        let out = eng.ob_create(name, 0x1000 + i as u16)?;
        let expect_policy = eng.reg.policy_of(name).map_err(|_| E_NOENT)?;
        if out.policy != expect_policy {
            return Err(E_TIER);
        }
        if out.tid != tr::TID_BASE + i as u16 {
            return Err(E_LEDGER);
        }
        let (must, must_not) = tier_signature(out.policy);
        if out.ctrl & must != must || out.ctrl & must_not != 0 {
            return Err(E_TIER);
        }
        if out.seq != i as u32 + 1 {
            return Err(E_LEDGER);
        }
        i += 1;
    }
    if eng.ledger_count() != GOLDEN_N {
        return Err(E_LEDGER);
    }
    eng.ledger_integrity()?;
    // R3：未注册类型创建显性拒 + 拒绝入账 + 创建账不动。
    let before = eng.ledger_count();
    if eng.ob_create("Ghost", 0x2000).err() != Some(E_DENY) {
        return Err(E_DENY);
    }
    if eng.ledger_count() != before {
        return Err(E_LEDGER);
    }
    if eng.denied.count() != 1 {
        return Err(E_LEDGER);
    }
    // 名非法同理（DenyName 路径）。
    if eng.ob_create("", 0x2000).err() != Some(E_DENY) {
        return Err(E_DENY);
    }
    if eng.denied.count() != 2 {
        return Err(E_LEDGER);
    }
    // R4：创建账容量（满后拒创建且账不顶替）。
    let mut j = eng.ledger_count();
    while j < LEDGER_CAP {
        eng.ob_create("Section", 0x3000)?;
        j += 1;
    }
    if eng.ledger_count() != LEDGER_CAP {
        return Err(E_LEDGER);
    }
    if eng.ob_create("Section", 0x3000).err() != Some(E_LEDGER) {
        return Err(E_LEDGER);
    }
    eng.ledger_integrity()?;
    // R5：回执重放确定性（新引擎同序创建 → 同回执序列）。
    let mut eng2 = Engine::assemble()?;
    let mut k = 0;
    while k < GOLDEN_N {
        let a = eng.at(k)?;
        let b = eng2.ob_create(tr::GOLDEN_NAMES[k], 0x1000 + k as u16)?;
        if a.tid != b.tid || a.policy != b.policy || a.ctrl != b.ctrl {
            return Err(E_LEDGER);
        }
        k += 1;
    }
    // R6：ClosedAll 档联签（Token）：控制位全零快照（默认拒绝锚在创建面复刻）。
    let tok = eng2.ob_create("Token", 0x4000)?;
    if tok.policy != TypePolicy::ClosedAll || tok.ctrl != 0 {
        return Err(E_TIER);
    }
    // R7：创建授权守恒（逐回执档位 → 授权 ∈ 合法区；Token 恒零）。
    let mut g = 0;
    while g < GOLDEN_N {
        let o = eng.at(g)?;
        let grant = create_grant(o.policy);
        verify_grant(o.policy, grant)?;
        if o.policy == TypePolicy::ClosedAll && grant != 0 {
            return Err(E_INTG);
        }
        g += 1;
    }
    // 授权快照重放确定性。
    let g1 = grant_snapshot(&eng)?;
    let g2 = grant_snapshot(&eng2)?;
    // 重放语义 = 「同序创建必得同授权序列」：对拍域取 R5 重放的黄金
    // 八类前缀。eng 第 9 条起是 R4 的 Section 灌账、eng2 第 9 条是 R6
    // 的 Token 联签——两引擎第 9 位不同构，不进对拍域。
    if g1.len() < GOLDEN_N || g2.len() < GOLDEN_N {
        return Err(E_LEDGER);
    }
    let mut w = 0;
    while w < GOLDEN_N {
        if g1[w].seq != g2[w].seq || g1[w].grant != g2[w].grant {
            return Err(E_LEDGER);
        }
        w += 1;
    }
    // 校验和：八类回执 tid 全并 × 账容量。
    let mut sum: u32 = 0;
    let mut q = 0;
    while q < GOLDEN_N {
        sum = sum.wrapping_add(eng.at(q).map_err(|_| E_LEDGER)?.tid as u32);
        q += 1;
    }
    Ok(sum.wrapping_mul(17).wrapping_add(LEDGER_CAP as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NOTREADY => "引擎未就绪（就绪门未过）",
        E_DENY => "创建被闸门拒绝（未注册/名非法）",
        E_SYN => "默认 SD 合成失败",
        E_TIER => "合成后档位复核不过",
        E_LEDGER => "创建账漂移（seq/容量/档位）",
        E_INTG => "引擎状态矛盾",
        E_NOENT => "类型面缺位（联签前段失败）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36126_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36126_obcreate");
    cs.add("SEG 段位独占", SEG_BASE == 0x3b20, "0x3b20~0x3b2F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "就绪门/联签四段/拒绝入账/容量/重放全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("账容量锚", LEDGER_CAP == 24, "创建账 24 槽（高价值事件不顶替）");
    cs.add("创建授权闭集", create_grant(TypePolicy::ClosedAll) == 0 && create_grant(TypePolicy::InheritParent) == sr::STD_ALL, "全闭恒零 / 继承档全五位");
    cs.add("就绪门语义", {
        // 未装配引擎不可手工构造就绪态——通过 assemble 唯一入口保证。
        match Engine::assemble() {
            Ok(e) => e.is_ready(),
            Err(_) => false,
        }
    }, "装配成功 ⇔ 就绪");
    cs.add("Token 全闭联签", {
        match Engine::assemble() {
            Ok(mut e) => match e.ob_create("Token", 1) {
                Ok(o) => o.policy == TypePolicy::ClosedAll && o.ctrl == 0,
                Err(_) => false,
            },
            Err(_) => false,
        }
    }, "默认拒绝档在创建面复刻");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(636));
    }

    #[test]
    fn closedall_ctrl_zero() {
        // ClosedAll 档令牌创建：SD 控制位全零（无 SD 对象红线）。
        let mut eng = Engine::assemble().unwrap();
        let out = eng.ob_create("Token", 0x0100).unwrap();
        assert_eq!(out.policy, super::super::unx36121_typereg::TypePolicy::ClosedAll);
        assert_eq!(out.ctrl, 0);
    }

    #[test]
    fn unreg_deny_accounted() {
        // 未注册类型创建显性拒且入拒账（账面守恒：拒账 +1）。
        let mut eng = Engine::assemble().unwrap();
        let before = eng.denied.count();
        assert_eq!(eng.ob_create("Ghost", 0x777).err(), Some(E_DENY));
        assert_eq!(eng.denied.count(), before + 1);
    }
}
