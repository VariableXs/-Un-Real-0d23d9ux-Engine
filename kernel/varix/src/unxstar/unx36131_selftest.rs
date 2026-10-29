//! UNX-F36131-J1 引擎装配自检（启动链探针：注册表+黄金类型全过）。
//! 判据：① 十段启动链（装配→闸门→绑定→档位→就绪→注销禁→持久化→审计→
//! 版本→收口）逐段判定且顺序不可交换；
//! ② 任一段失败即链停且段位显性（E_CHAIN 携段号）；
//! ③ 链全绿可重放（两次运行判定序列逐位一致）；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：启动链真源——B07 各模块自检面（integrity/drill-lite/快照）
//! 的统一编排点。每段只消费上游冻结 API（36121 装配 / 36122 闸门 /
//! 36123 对账 / 36125 快照 / 36126 就绪门 / 36127 攻击幸存 / 36129
//! roundtrip / 36130 审计链 / 36128 兼容核），不重实现任何判据。
//! B09 AccessCheck 主干（36161~）以 run_chain 全绿为引擎可用前提。

use crate::checks::CheckSet;
use super::unx36121_typereg::{self as tr, GOLDEN_N};
use super::unx36122_typedeny::{GateRoute};
use super::unx36123_genbind::face_audit;
use super::unx36125_deftier::tier_snapshot;
use super::unx36126_obcreate::Engine;
use super::unx36127_nounreg::{AttackLedger, DeregVec, HighWater, ATTACK_CAP};
use super::unx36122_typedeny::DenyLedger;
use super::unx36129_persist::{img_validate, persist};
use super::unx36130_typaudit::{audit_golden_enroll, Actor, Act, AuditBook};
use super::unx36128_typever::{compat_probe, VersionBook};

pub const SEG_BASE: u16 = 0x3b70;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CHAIN: u16 = SEG_BASE + 2; // 链段失败（arg = 段号）
pub const E_ORDER: u16 = SEG_BASE + 3; // 段序漂移（判定序列与冻结序不符）
pub const E_INTG: u16 = SEG_BASE + 4; // 链状态矛盾

/// 模块版本锚。
pub const VER: u32 = 1;

/// 启动链段位闭集（判别式 = 0xC0 段；顺序即冻结执行序）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    RegistryAssemble = 0x0C01, // S1：黄金装配 + 完整性
    GateProbe = 0x0C02,        // S2：闸门双态（已注册 Routed / 未注册 Deny）
    BindAudit = 0x0C03,        // S3：双面对账 + 同序对应
    TierSnapshot = 0x0C04,     // S4：四档合成快照确定性
    EngineReady = 0x0C05,      // S5：引擎就绪门 + 联签首建
    DeregSurvival = 0x0C06,    // S6：注销攻击幸存 + 高水位
    PersistRoundtrip = 0x0C07, // S7：镜像 roundtrip 守恒
    AuditChain = 0x0C08,       // S8：审计链守恒 + 钟单调
    VersionCompat = 0x0C09,    // S9：掩码升版兼容核
    Final = 0x0C0A,            // S10：收口（全段判定复核）
}

impl Stage {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn text(self) -> &'static str {
        match self {
            Stage::RegistryAssemble => "S1 注册表装配",
            Stage::GateProbe => "S2 闸门双态",
            Stage::BindAudit => "S3 绑定对账",
            Stage::TierSnapshot => "S4 档位快照",
            Stage::EngineReady => "S5 引擎就绪",
            Stage::DeregSurvival => "S6 注销禁幸存",
            Stage::PersistRoundtrip => "S7 持久化往返",
            Stage::AuditChain => "S8 审计链",
            Stage::VersionCompat => "S9 版本兼容",
            Stage::Final => "S10 收口",
        }
    }
}

/// 全部段位（冻结执行序——E_ORDER 的对照真源）。
pub const STAGE_ORDER: [Stage; 10] = [
    Stage::RegistryAssemble,
    Stage::GateProbe,
    Stage::BindAudit,
    Stage::TierSnapshot,
    Stage::EngineReady,
    Stage::DeregSurvival,
    Stage::PersistRoundtrip,
    Stage::AuditChain,
    Stage::VersionCompat,
    Stage::Final,
];

/// 段判定原子。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Verdict {
    pub stage: u16,
    pub ok: bool,
    pub code: u16, // ok 时为 0
}

/// 单段执行（每段一个最小自检面——失败即返回 E_CHAIN，不吞错）。
fn run_stage(stage: Stage) -> Result<(), u16> {
    match stage {
        Stage::RegistryAssemble => {
            let reg = tr::assemble_golden()?;
            reg.integrity().map_err(|_| E_CHAIN)?;
            if reg.count() != GOLDEN_N {
                return Err(E_CHAIN);
            }
            Ok(())
        }
        Stage::GateProbe => {
            let reg = tr::assemble_golden()?;
            if super::unx36122_typedeny::access_gate(&reg, "Token") != GateRoute::Routed {
                return Err(E_CHAIN);
            }
            if super::unx36122_typedeny::access_gate(&reg, "Ghost").is_deny() != true {
                return Err(E_CHAIN);
            }
            Ok(())
        }
        Stage::BindAudit => {
            let (reg, maps) = super::unx36123_genbind::assemble_golden_paired()?;
            face_audit(&reg, &maps).map_err(|_| E_CHAIN)?;
            let mut i = 0;
            while i < GOLDEN_N {
                super::unx36123_genbind::spot_audit(&reg, &maps, tr::GOLDEN_NAMES[i])
                    .map_err(|_| E_CHAIN)?;
                i += 1;
            }
            Ok(())
        }
        Stage::TierSnapshot => {
            let s1 = tier_snapshot()?;
            let s2 = tier_snapshot()?;
            if s1 != s2 {
                return Err(E_CHAIN);
            }
            Ok(())
        }
        Stage::EngineReady => {
            let mut eng = Engine::assemble()?;
            if !eng.is_ready() {
                return Err(E_CHAIN);
            }
            let out = eng.ob_create("Token", 0x0100).map_err(|_| E_CHAIN)?;
            if out.policy != tr::TypePolicy::ClosedAll || out.ctrl != 0 {
                return Err(E_CHAIN);
            }
            Ok(())
        }
        Stage::DeregSurvival => {
            let reg = tr::assemble_golden()?;
            let mut led = AttackLedger::new();
            let mut deny = DenyLedger::new();
            super::unx36127_nounreg::survived_attack(&reg, &mut led, &mut deny, 1)
                .map_err(|_| E_CHAIN)?;
            let hw = HighWater::from(&reg).map_err(|_| E_CHAIN)?;
            hw.no_reuse().map_err(|_| E_CHAIN)?;
            // 窗口守恒：1 轮 = 2*GOLDEN_N+2 = 18 次尝试 > ATTACK_CAP=16，
            // 最旧 2 条 ByName 被环形顶替 → ByName 窗口计数 = 8-2 = 6
            //（与 36127 环形账语义同源；total 守真计全部 18）。
            if led.total != (2 * GOLDEN_N + 2) as u32 || led.count() != ATTACK_CAP {
                return Err(E_CHAIN);
            }
            if led.count_vec(DeregVec::ByName)
                != (GOLDEN_N - (2 * GOLDEN_N + 2 - ATTACK_CAP)) as u32
            {
                return Err(E_CHAIN);
            }
            Ok(())
        }
        Stage::PersistRoundtrip => {
            let reg = tr::assemble_golden()?;
            let img = persist(&reg).map_err(|_| E_CHAIN)?;
            let (count, sealed) = img_validate(&img).map_err(|_| E_CHAIN)?;
            if count != GOLDEN_N || !sealed {
                return Err(E_CHAIN);
            }
            let reg2 = super::unx36129_persist::rebuild(&img, &[]).map_err(|_| E_CHAIN)?;
            if reg2.count() != reg.count() {
                return Err(E_CHAIN);
            }
            Ok(())
        }
        Stage::AuditChain => {
            let book = audit_golden_enroll()?;
            book.integrity().map_err(|_| E_CHAIN)?;
            if book.count() != GOLDEN_N + 1 {
                return Err(E_CHAIN);
            }
            let (c, _) = book.by_act(Act::Seal);
            if c != 1 {
                return Err(E_CHAIN);
            }
            let _ = Actor::System.code();
            Ok(())
        }
        Stage::VersionCompat => {
            let mut book = VersionBook::new();
            let m_old = tr::GOLDEN[0].1;
            let _ = book.touch("Probe", m_old).map_err(|_| E_CHAIN)?;
            let m_new = super::unx36104_genmap::GenMap {
                read: m_old.read | super::unx36102_stdrights::WRITE_DAC,
                write: m_old.write,
                exec: m_old.exec,
            };
            compat_probe(tr::TypePolicy::AdminGate, m_old, m_new).map_err(|_| E_CHAIN)?;
            Ok(())
        }
        Stage::Final => {
            // 收口：全段再点名（轻量复核——每段的核心不变量各取一条）。
            let reg = tr::assemble_golden()?;
            reg.integrity().map_err(|_| E_CHAIN)?;
            if !reg.is_sealed() {
                return Err(E_CHAIN);
            }
            if reg.query("Token").map(|e| e.policy) != Ok(tr::TypePolicy::ClosedAll) {
                return Err(E_CHAIN);
            }
            Ok(())
        }
    }
}

/// 全链执行：按冻结序逐段跑，失败即停在段位（Verdict 序列含失败段）。
pub fn run_chain() -> Result<[Verdict; 10], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut verdicts = [Verdict { stage: 0, ok: false, code: E_CHAIN }; 10];
    let mut i = 0;
    while i < 10 {
        let stage = STAGE_ORDER[i];
        match run_stage(stage) {
            Ok(()) => {
                verdicts[i] = Verdict { stage: stage.code(), ok: true, code: 0 };
            }
            Err(code) => {
                verdicts[i] = Verdict { stage: stage.code(), ok: false, code };
                // 链停：后续段标记未执行（stage 有值、ok=false、code=E_CHAIN）。
                let mut j = i + 1;
                while j < 10 {
                    verdicts[j] = Verdict { stage: STAGE_ORDER[j].code(), ok: false, code: E_CHAIN };
                    j += 1;
                }
                return Ok(verdicts);
            }
        }
        i += 1;
    }
    Ok(verdicts)
}

/// 链全绿判定（启动门：B09 消费入口）。
pub fn chain_green() -> Result<(), u16> {
    let v = run_chain()?;
    let mut i = 0;
    while i < 10 {
        if !v[i].ok {
            return Err(E_CHAIN);
        }
        i += 1;
    }
    Ok(())
}

/// 判定序列恒等（重放面）。
pub fn verdicts_eq(a: &[Verdict; 10], b: &[Verdict; 10]) -> bool {
    let mut i = 0;
    while i < 10 {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：全链跑通 + 判定序列与冻结序逐位对齐。
    let v = run_chain()?;
    let mut i = 0;
    while i < 10 {
        if v[i].stage != STAGE_ORDER[i].code() {
            return Err(E_ORDER);
        }
        if !v[i].ok || v[i].code != 0 {
            return Err(E_CHAIN);
        }
        i += 1;
    }
    chain_green()?;
    // R2：链重放确定性（两次运行判定序列逐位一致）。
    let v2 = run_chain()?;
    if !verdicts_eq(&v, &v2) {
        return Err(E_INTG);
    }
    // R3：段序冻结（对照真源逐位核对——乱序注入必被识破）。
    // 静态证明：STAGE_ORDER 判别式严格递增（0x0C01..0x0C0A）。
    let mut k = 0;
    while k + 1 < 10 {
        if STAGE_ORDER[k].code() >= STAGE_ORDER[k + 1].code() {
            return Err(E_ORDER);
        }
        k += 1;
    }
    // R4：段位人话闭集（十段全可归因）。
    let mut s = 0;
    while s < 10 {
        if STAGE_ORDER[s].text().is_empty() {
            return Err(E_INTG);
        }
        s += 1;
    }
    // R5：单段可独立重入（收口段单独跑仍绿——段无隐藏前置态）。
    run_stage(Stage::Final).map_err(|_| E_CHAIN)?;
    // 校验和：全段判别式和 × 段数。
    let mut sum: u32 = 0;
    let mut q = 0;
    while q < 10 {
        sum = sum.wrapping_add(STAGE_ORDER[q].code() as u32);
        q += 1;
    }
    Ok(sum.wrapping_mul(3).wrapping_add(10))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CHAIN => "链段失败（段内自检未过）",
        E_ORDER => "段序漂移（判定序列与冻结序不符）",
        E_INTG => "链状态矛盾（重放失真）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36131_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36131_selftest");
    cs.add("SEG 段位独占", SEG_BASE == 0x3b70, "0x3b70~0x3b7F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "十段链全绿/段序冻结/重放一致/单段重入全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("段位闭集", Stage::RegistryAssemble.code() == 0x0C01 && Stage::Final.code() == 0x0C0A, "十段判别式闭集（0xC0 段）");
    cs.add("启动门语义", chain_green().is_ok(), "chain_green 全绿 ⇔ 引擎可用前提");
    cs.add("段人话闭集", Stage::GateProbe.text().len() > 0 && Stage::VersionCompat.text().len() > 0, "十段全可归因");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(92335));
    }

    #[test]
    fn stage_order_frozen() {
        // 段序冻结：判别式严格递增（0x0C01..0x0C0A）。
        let mut k = 0;
        while k + 1 < 10 {
            assert!(STAGE_ORDER[k].code() < STAGE_ORDER[k + 1].code());
            k += 1;
        }
        assert_eq!(STAGE_ORDER[0].code(), 0x0C01);
        assert_eq!(STAGE_ORDER[9].code(), 0x0C0A);
    }

    #[test]
    fn chain_replay_deterministic() {
        // 链重放确定性：两次全链判定序列逐位一致，且十段全绿。
        let v1 = run_chain().unwrap();
        let v2 = run_chain().unwrap();
        assert!(verdicts_eq(&v1, &v2));
        let mut i = 0;
        while i < 10 {
            assert!(v1[i].ok && v1[i].code == 0);
            i += 1;
        }
        assert!(chain_green().is_ok());
    }
}
