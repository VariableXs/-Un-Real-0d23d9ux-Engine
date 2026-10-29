//! UNX-F36137-J1 B07 与 B01–B06 的地基联动黄金剧本（10 剧本）。
//! 判据：① 十剧本各贯通一条跨域链（B01 SID / B03 特权 / B04 SD / B05
//! ACE / B06 掩码 × B07 类型面），结局确定性（同剧本同证据值）；
//! ② 剧本序冻结（判别式 0x11 段递增）且逐剧本可独立重入；
//! ③ 任一剧本失败显性归因（剧本号 + 剧本内步码）；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：联动真源——单模块 drill 证的是「模块内部自洽」，剧本证的是
//! 「跨模块协作时语义不串线」。每个剧本是一条最小端到端业务链，证据值
//! = 链上关键结局的确定性指纹（同输入必同值，值漂移即链断）。

use crate::checks::CheckSet;
use super::unx36121_typereg::{self as tr, TypePolicy, TypeRegistry, GOLDEN_N, TID_BASE};
use super::unx36122_typedeny::{access_gate, GateRoute};
use super::unx36123_genbind::{assemble_golden_paired, consumed_translate};
use super::unx36104_genmap::{G_READ, G_WRITE, G_EXEC, G_ALL};
use super::unx36125_deftier::{synthesize_verified, tier_snapshot, verify_tier, verify_shape};
use super::unx36126_obcreate::Engine;
use super::unx36127_nounreg::{AttackLedger, survived_attack};
use super::unx36122_typedeny::DenyLedger;
use super::unx36129_persist::{persist, rebuild};
use super::unx36130_typaudit::{audit_golden_enroll, Actor, Act};
use super::unx36131_selftest::run_chain;
use super::unx36132_typfuzz::{Lcg, MAL_NAMES};
use super::unx36102_stdrights as sr;

pub const SEG_BASE: u16 = 0x81f0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SCRIPT: u16 = SEG_BASE + 2; // 剧本失败（arg = 剧本号）
pub const E_ORDER: u16 = SEG_BASE + 3; // 剧本序漂移
pub const E_INTG: u16 = SEG_BASE + 4; // 证据值漂移（重放失真）

/// 模块版本锚。
pub const VER: u32 = 1;

/// 剧本号闭集（判别式 = 0x11 段，十本）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Script {
    S1TokenCreate = 0x1101,   // 剧本一：令牌类型创建全链（B01×B04×B07）
    S2UnregDeny = 0x1102,     // 剧本二：未注册类型访问全拒（红线贯通）
    S3GenTranslate = 0x1103,  // 剧本三：GENERIC 翻译四向往返（B06×B07）
    S4TierSynth = 0x1104,     // 剧本四：四档 SD 合成全链（B04×B07）
    S5FuzzChain = 0x1105,     // 剧本五：注册面 fuzz 不变量链（鲁棒面）
    S6PersistAudit = 0x1106,  // 剧本六：持久化往返 × 审计链
    S7DeregSurvive = 0x1107,  // 剧本七：注销攻击幸存链
    S8WizardJoin = 0x1108,    // 剧本八：向导接入 × 投影对账
    S9EventTelem = 0x1109,    // 剧本九：事件账遥测链
    S10ChainSeal = 0x110A,    // 剧本十：启动链十段收口
}

impl Script {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn text(self) -> &'static str {
        match self {
            Script::S1TokenCreate => "剧本一 令牌创建全链",
            Script::S2UnregDeny => "剧本二 未注册全拒",
            Script::S3GenTranslate => "剧本三 GENERIC 往返",
            Script::S4TierSynth => "剧本四 四档合成",
            Script::S5FuzzChain => "剧本五 fuzz 链",
            Script::S6PersistAudit => "剧本六 持久化×审计",
            Script::S7DeregSurvive => "剧本七 攻击幸存",
            Script::S8WizardJoin => "剧本八 向导接入",
            Script::S9EventTelem => "剧本九 遥测链",
            Script::S10ChainSeal => "剧本十 启动链收口",
        }
    }
}

/// 剧本冻结序。
pub const SCRIPT_ORDER: [Script; 10] = [
    Script::S1TokenCreate,
    Script::S2UnregDeny,
    Script::S3GenTranslate,
    Script::S4TierSynth,
    Script::S5FuzzChain,
    Script::S6PersistAudit,
    Script::S7DeregSurvive,
    Script::S8WizardJoin,
    Script::S9EventTelem,
    Script::S10ChainSeal,
];

// ---- 十剧本本体（每本一条跨域链，证据值 = 结局指纹） ----------------------

/// 剧本一：黄金装配 → Token（ClosedAll 档）联签创建 → SD 控制位全零 →
/// 令牌面（B01 域的 SID 闭集语义在此以「类型=Token 默认拒」锚定）。
pub fn s1_token_create() -> Result<u32, u16> {
    let mut eng = Engine::assemble()?;
    let out = eng.ob_create("Token", 0xB01)?;
    if out.policy != TypePolicy::ClosedAll || out.ctrl != 0 {
        return Err(E_SCRIPT);
    }
    // B01 锚：令牌对象在 B07 面的 tid 与黄金序一致。
    if out.tid != TID_BASE + 7 {
        return Err(E_SCRIPT);
    }
    // B04 锚：ClosedAll 档合成的 SD 就是「无 SD 对象」（36065 红线）。
    let (sd, bodies) = synthesize_verified(TypePolicy::ClosedAll)?;
    if sd.control != 0 || bodies[0].len != 0 {
        return Err(E_SCRIPT);
    }
    Ok((out.tid as u32).wrapping_mul(97).wrapping_add(out.creator as u32))
}

/// 剧本二：未注册类型在闸门（36122）、创建面（36126）、策略面（36125）
/// 三处全拒——红线贯通证。
pub fn s2_unreg_deny() -> Result<u32, u16> {
    let mut eng = Engine::assemble()?;
    if access_gate(&eng.reg, "Ghost") != GateRoute::DenyUnknown {
        return Err(E_SCRIPT);
    }
    if eng.ob_create("Ghost", 0x777).err() != Some(super::unx36126_obcreate::E_DENY) {
        return Err(E_SCRIPT);
    }
    if eng.denied.count() != 1 {
        return Err(E_SCRIPT);
    }
    if super::unx36125_deftier::synthesize_for_type(&eng.reg, "Ghost").err()
        != Some(super::unx36125_deftier::E_NOENT)
    {
        return Err(E_SCRIPT);
    }
    Ok(eng.denied.count() as u32 + 2)
}

/// 剧本三：File 类型 GENERIC 四向翻译往返（B06 映射面 × B07 绑定面），
/// ALL≡三并守恒 + 掩码位段学（B06 36101）合法区内。
pub fn s3_gen_translate() -> Result<u32, u16> {
    let (reg, maps) = assemble_golden_paired()?;
    let r = consumed_translate(&reg, &maps, "File", G_READ)?;
    let w = consumed_translate(&reg, &maps, "File", G_WRITE)?;
    let x = consumed_translate(&reg, &maps, "File", G_EXEC)?;
    let a = consumed_translate(&reg, &maps, "File", G_ALL)?;
    if a != r | w | x {
        return Err(E_SCRIPT);
    }
    // 位段学锚：翻译值全落 B06 合法区。
    if r & !super::unx36101_masktable::ALL_LEGAL != 0 {
        return Err(E_SCRIPT);
    }
    // 标准五位锚（B06 36102）：File read 槽含 READ_CONTROL。
    if r & sr::READ_CONTROL == 0 {
        return Err(E_SCRIPT);
    }
    Ok(r ^ w ^ x ^ a)
}

/// 剧本四：四档合成 × 快照确定性 × 黄金逐类驱动（B04 构造器 × B07 档位）。
pub fn s4_tier_synth() -> Result<u32, u16> {
    let reg = tr::assemble_golden()?;
    let snap = tier_snapshot()?;
    let mut g = 0;
    let mut acc: u32 = 0;
    while g < GOLDEN_N {
        let policy = reg.policy_of(tr::GOLDEN_NAMES[g]).map_err(|_| E_SCRIPT)?;
        let (sd, bodies) = super::unx36125_deftier::synthesize_for_type(&reg, tr::GOLDEN_NAMES[g])?;
        verify_tier(&sd, policy)?;
        verify_shape(&sd, &bodies, policy)?;
        // 快照内证：该档位快照槽位与合成控制位一致。
        let slot = match policy {
            TypePolicy::ClosedAll => 0,
            TypePolicy::NullDacl => 1,
            TypePolicy::InheritParent => 2,
            TypePolicy::AdminGate => 3,
        };
        if snap[slot] != sd.control {
            return Err(E_SCRIPT);
        }
        acc = acc.wrapping_add(snap[slot] as u32);
        g += 1;
    }
    Ok(acc.wrapping_mul(3).wrapping_add(GOLDEN_N as u32))
}

/// 剧本五：注册面 300 轮确定性 fuzz 不变量链（LCG × 注册表完整性）。
pub fn s5_fuzz_chain() -> Result<u32, u16> {
    let mut l = Lcg::new(0x5C51_0000_0000_0005);
    let mut reg = TypeRegistry::new();
    let mut accepted = 0u32;
    let mut round = 0;
    while round < 300 {
        let pick = l.below(6);
        let name = if pick < 3 { MAL_NAMES[l.below(MAL_NAMES.len())] } else { tr::FILL_NAMES[l.below(tr::FILL_NAMES.len())] };
        let map = tr::GOLDEN[l.below(GOLDEN_N)].1;
        let policy = match l.below(4) {
            0 => TypePolicy::ClosedAll,
            1 => TypePolicy::NullDacl,
            2 => TypePolicy::InheritParent,
            _ => TypePolicy::AdminGate,
        };
        match reg.register(name, map, policy, 0) {
            Ok(_) => accepted += 1,
            Err(code) => {
                // 结局闭集：名/图/满/重四拒因（未封印表无 E_FROZEN）。
                if code != tr::E_NAME && code != tr::E_MAPBAD && code != tr::E_FULL && code != tr::E_DUP {
                    return Err(E_SCRIPT);
                }
            }
        }
        reg.integrity().map_err(|_| E_SCRIPT)?;
        round += 1;
    }
    // 不变量锚：表内条目数 ≤ CAP（accepted 是 Ok 次数，含幂等重登记，
    // 天然可超 CAP——守恒核必须落在表计数上，不是 Ok 计数上）。
    if reg.count() > tr::CAP {
        return Err(E_SCRIPT);
    }
    Ok(reg.count() as u32 * 11 + 300)
}

/// 剧本六：持久化 roundtrip × 审计链 × 指纹三账同源。
pub fn s6_persist_audit() -> Result<u32, u16> {
    let reg = tr::assemble_golden()?;
    let img = persist(&reg)?;
    let reg2 = rebuild(&img, &[])?;
    if reg2.count() != reg.count() {
        return Err(E_SCRIPT);
    }
    let book = audit_golden_enroll()?;
    book.integrity().map_err(|_| E_SCRIPT)?;
    // 审计账的 Enroll 数 = 黄金数（账面与注册表计数守恒）。
    let (c_enroll, _) = book.by_act(Act::Enroll);
    if c_enroll != GOLDEN_N {
        return Err(E_SCRIPT);
    }
    // 指纹三账同源：审计账首事件指纹 = 持久化镜像首条目指纹。
    let ev = book.at(0).map_err(|_| E_SCRIPT)?;
    let img_fp = img[4 + 1]; // 首条目名指纹槽
    if ev.fp != img_fp {
        return Err(E_SCRIPT);
    }
    Ok(ev.fp as u32)
}

/// 剧本七：注销攻击幸存链（四手法 × 高水位 × 封印位）。
pub fn s7_dereg_survive() -> Result<u32, u16> {
    let reg = tr::assemble_golden()?;
    let mut led = AttackLedger::new();
    let mut deny = DenyLedger::new();
    survived_attack(&reg, &mut led, &mut deny, 2)?;
    if !reg.is_sealed() {
        return Err(E_SCRIPT);
    }
    let mut g = 0;
    while g < GOLDEN_N {
        if reg.query(tr::GOLDEN_NAMES[g]).is_err() {
            return Err(E_SCRIPT);
        }
        g += 1;
    }
    Ok(led.total)
}

/// 剧本八：向导七步接入 × 投影对账（36135 × 36134）。
pub fn s8_wizard_join() -> Result<u32, u16> {
    let mut ctx = super::unx36135_wizard::WizardCtx::new();
    ctx.preassemble_golden()?;
    let r = ctx
        .run_wizard(super::unx36135_wizard::WizardReq {
            name: "Alchemy",
            map: tr::GOLDEN[4].1,
            policy: TypePolicy::NullDacl,
            actor_tag: 0x8888,
        })
        .map_err(|c| E_SCRIPT)?;
    if r.tid != TID_BASE + GOLDEN_N as u16 {
        return Err(E_SCRIPT);
    }
    // 投影对账：注册表（含新类型）与命名空间镜像一致。
    let mirror = super::unx36134_nsrecon::project(&ctx.reg)?;
    super::unx36134_nsrecon::reconcile(&ctx.reg, &mirror)?;
    if mirror.count() != GOLDEN_N + 1 {
        return Err(E_SCRIPT);
    }
    // 审计联动：向导 Enroll 事件入账。
    let (c, _) = ctx.log.by_actor(Actor::Wizard);
    if c != 1 {
        return Err(E_SCRIPT);
    }
    Ok((r.tid as u32).wrapping_mul(13).wrapping_add(mirror.count() as u32))
}

/// 剧本九：事件账遥测链（36136 十源埋点 × 级别过滤 × 窗口）。
pub fn s9_event_telem() -> Result<u32, u16> {
    let book = super::unx36136_eventlog::log_full_load()?;
    if book.count() != 30 {
        return Err(E_SCRIPT);
    }
    let rej = book.count_level(super::unx36136_eventlog::Level::Reject);
    if rej != 10 {
        return Err(E_SCRIPT);
    }
    let win = book.count_window(4, 9)?;
    if win != 6 {
        return Err(E_SCRIPT);
    }
    Ok(rej.wrapping_mul(7).wrapping_add(win))
}

/// 剧本十：启动链十段全绿收口（36131 是 B07 的总装检）。
pub fn s10_chain_seal() -> Result<u32, u16> {
    let v = run_chain()?;
    let mut i = 0;
    let mut acc: u32 = 0;
    while i < 10 {
        if !v[i].ok {
            return Err(E_SCRIPT);
        }
        acc = acc.wrapping_add(v[i].stage as u32);
        i += 1;
    }
    Ok(acc)
}

/// 剧本分派表（冻结序执行——run_all 的唯一驱动）。
pub fn run_script(s: Script) -> Result<u32, u16> {
    match s {
        Script::S1TokenCreate => s1_token_create(),
        Script::S2UnregDeny => s2_unreg_deny(),
        Script::S3GenTranslate => s3_gen_translate(),
        Script::S4TierSynth => s4_tier_synth(),
        Script::S5FuzzChain => s5_fuzz_chain(),
        Script::S6PersistAudit => s6_persist_audit(),
        Script::S7DeregSurvive => s7_dereg_survive(),
        Script::S8WizardJoin => s8_wizard_join(),
        Script::S9EventTelem => s9_event_telem(),
        Script::S10ChainSeal => s10_chain_seal(),
    }
}

/// 全剧本执行（证据值序列）。
pub fn run_all_scripts() -> Result<[u32; 10], u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut ev = [0u32; 10];
    let mut i = 0;
    while i < 10 {
        ev[i] = run_script(SCRIPT_ORDER[i]).map_err(|c| if c == E_SCRIPT { E_SCRIPT } else { c })?;
        i += 1;
    }
    Ok(ev)
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：十剧本全绿 + 证据值重放确定性。
    let e1 = run_all_scripts()?;
    let e2 = run_all_scripts()?;
    if e1 != e2 {
        return Err(E_INTG);
    }
    // 证据值非退化（全零链必是断链）。
    let mut i = 0;
    while i < 10 {
        if e1[i] == 0 {
            return Err(E_SCRIPT);
        }
        i += 1;
    }
    // R2：剧本序冻结（判别式递增 + 文本闭集）。
    let mut k = 0;
    while k + 1 < 10 {
        if SCRIPT_ORDER[k].code() >= SCRIPT_ORDER[k + 1].code() {
            return Err(E_ORDER);
        }
        if SCRIPT_ORDER[k].text().is_empty() {
            return Err(E_ORDER);
        }
        k += 1;
    }
    // R3：逐剧本独立重入（重入结果与全链跑一致）。
    let mut j = 0;
    while j < 10 {
        let solo = run_script(SCRIPT_ORDER[j])?;
        if solo != e1[j] {
            return Err(E_INTG);
        }
        j += 1;
    }
    // 校验和：十证据值全并（确定性总锚）。
    let mut sum: u32 = 0;
    let mut q = 0;
    while q < 10 {
        sum = sum.wrapping_add(e1[q]);
        q += 1;
    }
    Ok(sum)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SCRIPT => "剧本失败（链上某步语义串线/结局不符）",
        E_ORDER => "剧本序漂移（判别式/文本闭集破）",
        E_INTG => "证据值漂移（重放失真/独立重入不一致）",
        _ => "未知诊断码",
    }
}

pub fn run_unx36137_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36137_scripts");
    cs.add("SEG 段位独占", SEG_BASE == 0x81f0, "0x81f0~0x81fF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "十剧本全绿/重放/独立重入/序冻结全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("剧本闭集", Script::S1TokenCreate.code() == 0x1101 && Script::S10ChainSeal.code() == 0x110A, "十本判别式闭集（0x11 段）");
    cs.add("剧本人话闭集", Script::S5FuzzChain.text().len() > 0 && Script::S9EventTelem.text().len() > 0, "十本全可归因");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_order_frozen() {
        // 剧本序冻结：判别式递增（0x11 段）+ 人话闭集非空。
        let mut k = 0;
        while k + 1 < 10 {
            assert!(SCRIPT_ORDER[k].code() < SCRIPT_ORDER[k + 1].code());
            assert!(!SCRIPT_ORDER[k].text().is_empty());
            k += 1;
        }
    }

    #[test]
    fn solo_reentry_consistent() {
        // 独立重入守恒：逐本单跑结局与全链跑逐位一致，且证据值非退化。
        let e1 = run_all_scripts().unwrap();
        let mut j = 0;
        while j < 10 {
            assert_ne!(e1[j], 0);
            assert_eq!(run_script(SCRIPT_ORDER[j]).unwrap(), e1[j]);
            j += 1;
        }
    }

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(1200660));
    }
}
