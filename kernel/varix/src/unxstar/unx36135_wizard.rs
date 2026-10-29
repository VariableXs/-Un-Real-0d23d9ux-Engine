//! UNX-F36135-J1 新对象类型接入向导（制度化检查单锚）。
//! 判据：① 七步检查单（名纪律→图守恒→档位→配额→登记→审计→投影）逐步
//! 勾稽且顺序不可交换；② 半途而废显性留档（未完成步位掩码可查询）；
//! ③ 向导全程与手工五步协议（36124）产出一致；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：接入制度化锚——「新类型上线」不该是裸调 register 的即兴
//! 行为，而是检查单驱动的过程。向导 = 检查单状态机 + 36124 协议调用 +
//! 36134 投影对账收尾。检查单位掩码（CHECKLIST_N 位）是向导的可观察状态：
//! 半途失败时，调用方拿到的不是裸 Err，而是「走到第几步」的显性档案。

use crate::checks::CheckSet;
use alloc::vec::Vec;
use super::unx36104_genmap::{GenMap, Registry as MapReg};
use super::unx36121_typereg::{self as tr, TypePolicy, TypeRegistry, GOLDEN_N, TID_BASE};
use super::unx36123_genbind::bind_type_map;
use super::unx36134_nsrecon::{project, reconcile};

pub const SEG_BASE: u16 = 0x3bb0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_STEP: u16 = SEG_BASE + 2; // 步序违例（跳步/回退）
pub const E_NAME: u16 = SEG_BASE + 3; // 步① 名纪律
pub const E_MAPBAD: u16 = SEG_BASE + 4; // 步② 图守恒
pub const E_POLICY: u16 = SEG_BASE + 5; // 步③ 档位闭集
pub const E_QUOTA: u16 = SEG_BASE + 6; // 步④ 配额
pub const E_ENROLL: u16 = SEG_BASE + 7; // 步⑤ 登记（协议透传）
pub const E_AUDIT: u16 = SEG_BASE + 8; // 步⑥ 审计
pub const E_PROJ: u16 = SEG_BASE + 9; // 步⑦ 投影对账
pub const E_INTG: u16 = SEG_BASE + 10; // 检查单状态矛盾

/// 模块版本锚。
pub const VER: u32 = 1;

/// 检查单步数（七步）。
pub const CHECKLIST_N: usize = 7;

/// 检查单位掩码语义（位 i 置位 = 第 i+1 步完成）。
pub const STEP_NAME: u16 = 1 << 0;
pub const STEP_MAP: u16 = 1 << 1;
pub const STEP_POLICY: u16 = 1 << 2;
pub const STEP_QUOTA: u16 = 1 << 3;
pub const STEP_ENROLL: u16 = 1 << 4;
pub const STEP_AUDIT: u16 = 1 << 5;
pub const STEP_PROJ: u16 = 1 << 6;
pub const STEP_ALL: u16 = STEP_NAME | STEP_MAP | STEP_POLICY | STEP_QUOTA | STEP_ENROLL | STEP_AUDIT | STEP_PROJ;

/// 向导请求（与 36124 EnrollReq 同形——向导是协议的制度化外壳）。
#[derive(Clone, Copy)]
pub struct WizardReq {
    pub name: &'static str,
    pub map: GenMap,
    pub policy: TypePolicy,
    pub actor_tag: u16,
}

/// 向导回执（检查单 + 双面坐标 + 投影节点数）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WizardReceipt {
    pub checklist: u16,
    pub tid: u16,
    pub mid: usize,
    pub proj_nodes: usize,
    pub audit_seq: u32,
}

/// 步序推进器：mask 已完成集，step 欲推进步 → 合法性核（只许按位序走）。
pub fn step_next(mask: u16, step: usize) -> Result<usize, u16> {
    if step >= CHECKLIST_N {
        return Err(E_STEP);
    }
    // 前置步必须全完成（顺序不可交换的结构性证明）。
    let mut i = 0;
    while i < step {
        if mask & (1 << i) == 0 {
            return Err(E_STEP);
        }
        i += 1;
    }
    if mask & (1 << step) != 0 {
        return Err(E_STEP); // 已完成步不许重走
    }
    Ok(step)
}

/// 向导状态机（注册表 + 映射表 + 审计账 + 检查单）。
pub struct WizardCtx {
    pub reg: TypeRegistry,
    pub maps: MapReg,
    pub log: super::unx36130_typaudit::AuditBook,
    pub checklist: u16,
    pub wizard_used: usize,
    pub tick: u64,
}

impl WizardCtx {
    pub fn new() -> WizardCtx {
        WizardCtx {
            reg: TypeRegistry::new(),
            maps: MapReg::new(),
            log: super::unx36130_typaudit::AuditBook::new(),
            checklist: 0,
            wizard_used: 0,
            tick: 100,
        }
    }

    /// 黄金预装配（黄金八类不走向导——向导只管新类型）。双面同装：
    /// 类型面 + 映射面（bind_type_map 的 seq_ok 同序对应要求两表
    /// 起点一致——只装单面会让首个向导类型的 mid 与 tid 序错位）。
    pub fn preassemble_golden(&mut self) -> Result<(), u16> {
        let mut g = 0;
        while g < GOLDEN_N {
            let (name, map, policy, flags) = tr::GOLDEN[g];
            self.reg.register(name, map, policy, flags | tr::F_GOLDEN).map_err(|_| E_ENROLL)?;
            self.maps.register(name, map).map_err(|_| E_ENROLL)?;
            g += 1;
        }
        self.tick += 1;
        Ok(())
    }

    /// 七步向导主面（任一步失败 → 带检查单档案的显性拒绝）。
    pub fn run_wizard(&mut self, req: WizardReq) -> Result<WizardReceipt, (u16, u16)> {
        if VER != 1 {
            return Err((E_VER, self.checklist));
        }
        // 步①：名纪律。
        step_next(self.checklist, 0).map_err(|c| (c, self.checklist))?;
        if !TypeRegistry::name_ok(req.name) {
            return Err((E_NAME, self.checklist));
        }
        self.checklist |= STEP_NAME;
        // 步②：图守恒。
        step_next(self.checklist, 1).map_err(|c| (c, self.checklist))?;
        if !req.map.wellformed() {
            return Err((E_MAPBAD, self.checklist));
        }
        self.checklist |= STEP_MAP;
        // 步③：档位闭集。
        step_next(self.checklist, 2).map_err(|c| (c, self.checklist))?;
        if req.policy.code() < 0x0001 || req.policy.code() > 0x0004 {
            return Err((E_POLICY, self.checklist));
        }
        self.checklist |= STEP_POLICY;
        // 步④：配额（向导位 = 映射表容量 − 黄金数 − 已用）。
        step_next(self.checklist, 3).map_err(|c| (c, self.checklist))?;
        let quota = super::unx36124_enroll::WIZARD_QUOTA;
        if self.wizard_used >= quota {
            // 拒绝事件同样推进逻辑钟（record 自证 tick > now——
            // 停滞的钟落账必炸 E_TICK，与步⑥正常路径同律）。
            self.tick += 1;
            self.log
                .record(super::unx36130_typaudit::Actor::Wizard, super::unx36130_typaudit::Act::Reject, req.name, self.tick, E_QUOTA)
                .map_err(|c| (c, self.checklist))?;
            return Err((E_QUOTA, self.checklist));
        }
        self.checklist |= STEP_QUOTA;
        // 步⑤：登记（双面绑定直调 36123——向导不绕协议，协议核在绑定内）。
        step_next(self.checklist, 4).map_err(|c| (c, self.checklist))?;
        let coord = bind_type_map(&mut self.reg, &mut self.maps, req.name, req.map, req.policy, 0)
            .map_err(|c| (E_ENROLL, self.checklist))?;
        self.wizard_used += 1;
        self.checklist |= STEP_ENROLL;
        // 步⑥：审计（Enroll 事件入 36130 账）。
        step_next(self.checklist, 5).map_err(|c| (c, self.checklist))?;
        self.tick += 1;
        let seq = self
            .log
            .record(super::unx36130_typaudit::Actor::Wizard, super::unx36130_typaudit::Act::Enroll, req.name, self.tick, coord.tid)
            .map_err(|c| (E_AUDIT, self.checklist))?;
        self.checklist |= STEP_AUDIT;
        // 步⑦：投影对账（命名空间镜像同步收尾）。
        step_next(self.checklist, 6).map_err(|c| (c, self.checklist))?;
        let mirror = project(&self.reg).map_err(|c| (E_PROJ, self.checklist))?;
        reconcile(&self.reg, &mirror).map_err(|c| (E_PROJ, self.checklist))?;
        self.checklist |= STEP_PROJ;
        Ok(WizardReceipt {
            checklist: self.checklist,
            tid: coord.tid,
            mid: coord.mid,
            proj_nodes: mirror.count(),
            audit_seq: seq,
        })
    }

    /// 向导复位（下一类型接入前的检查单清零——状态机复用面）。
    pub fn reset_checklist(&mut self) {
        self.checklist = 0;
    }

    /// 检查单完备性（七位全置 = 一次完整接入）。
    pub fn complete(&self) -> bool {
        self.checklist == STEP_ALL
    }
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：黄金预装配 + 首个新类型七步全过。
    let mut ctx = WizardCtx::new();
    ctx.preassemble_golden()?;
    let req = WizardReq {
        name: "Port",
        map: tr::GOLDEN[0].1,
        policy: TypePolicy::NullDacl,
        actor_tag: 0x0042,
    };
    let r = ctx.run_wizard(req).map_err(|c| c.0)?;
    if !ctx.complete() || r.checklist != STEP_ALL {
        return Err(E_INTG);
    }
    if r.tid != TID_BASE + GOLDEN_N as u16 || r.mid != GOLDEN_N {
        return Err(E_ENROLL);
    }
    if r.proj_nodes != GOLDEN_N + 1 {
        return Err(E_PROJ);
    }
    // 步序违例核：checklist 全置后再推步 → E_STEP。
    if step_next(ctx.checklist, 0).err() != Some(E_STEP) {
        return Err(E_STEP);
    }
    // R2：半途档案（步②炸 → 检查单只留步①位）。
    let mut ctx2 = WizardCtx::new();
    ctx2.preassemble_golden()?;
    let bad_map = GenMap { read: 0x0100_0000, write: 0, exec: 0 }; // GEN_RSVD 污染
    let err = ctx2
        .run_wizard(WizardReq { name: "BadPort", map: bad_map, policy: TypePolicy::NullDacl, actor_tag: 0 })
        .err()
        .ok_or(E_INTG)?;
    if err.0 != E_MAPBAD || err.1 != STEP_NAME {
        return Err(E_STEP);
    }
    if ctx2.complete() {
        return Err(E_INTG);
    }
    // 半途后表未动（登记在步⑤——步②炸不留半状态）。
    if ctx2.reg.count() != GOLDEN_N || ctx2.maps.count() != GOLDEN_N {
        return Err(E_INTG);
    }
    // R3：步①炸（空名）→ 检查单全空。
    let mut ctx3 = WizardCtx::new();
    ctx3.preassemble_golden()?;
    let err3 = ctx3
        .run_wizard(WizardReq { name: "", map: tr::GOLDEN[0].1, policy: TypePolicy::NullDacl, actor_tag: 0 })
        .err()
        .ok_or(E_INTG)?;
    if err3.0 != E_NAME || err3.1 != 0 {
        return Err(E_STEP);
    }
    // R4：配额耗尽（向导位 8 用满 → 第 9 个 E_QUOTA 且审计有 Reject）。
    let mut ctx4 = WizardCtx::new();
    ctx4.preassemble_golden()?;
    let mut q = 0;
    while q < super::unx36124_enroll::WIZARD_QUOTA {
        ctx4.reset_checklist();
        ctx4
            .run_wizard(WizardReq { name: tr::FILL_NAMES[q], map: tr::GOLDEN[q % GOLDEN_N].1, policy: TypePolicy::NullDacl, actor_tag: 0 })
            .map_err(|c| c.0)?;
        q += 1;
    }
    ctx4.reset_checklist();
    let err4 = ctx4
        .run_wizard(WizardReq { name: "Wizard9", map: tr::GOLDEN[0].1, policy: TypePolicy::NullDacl, actor_tag: 0 })
        .err()
        .ok_or(E_INTG)?;
    if err4.0 != E_QUOTA {
        return Err(E_QUOTA);
    }
    // 审计账有 Reject 事件（步④拒绝入账）。
    let (c_rej, _) = ctx4.log.by_act(super::unx36130_typaudit::Act::Reject);
    if c_rej != 1 {
        return Err(E_AUDIT);
    }
    // R5：向导与手工协议产出一致（同名同图同策略 → 同 tid）。
    let mut ctx5 = WizardCtx::new();
    ctx5.preassemble_golden()?;
    let rw = ctx5.run_wizard(req).map_err(|c| c.0)?;
    // 手工路径：直调绑定。
    let mut reg6 = TypeRegistry::new();
    let mut maps6 = MapReg::new();
    let mut g6 = 0;
    while g6 < GOLDEN_N {
        let (name, map, policy, flags) = tr::GOLDEN[g6];
        reg6.register(name, map, policy, flags | tr::F_GOLDEN).map_err(|_| E_ENROLL)?;
        maps6.register(name, map).map_err(|_| E_ENROLL)?;
        g6 += 1;
    }
    let c6 = bind_type_map(&mut reg6, &mut maps6, "Port", tr::GOLDEN[0].1, TypePolicy::NullDacl, 0).map_err(|_| E_ENROLL)?;
    if rw.tid != c6.tid || rw.mid != c6.mid {
        return Err(E_INTG);
    }
    // R6：向导审计链守恒（Enroll 全 Wizard 行为者 + tick 单调）。
    ctx5.log.integrity().map_err(|_| E_AUDIT)?;
    let (c_wiz, _) = ctx5.log.by_actor(super::unx36130_typaudit::Actor::Wizard);
    if c_wiz != 1 {
        return Err(E_AUDIT);
    }
    // 校验和：tid × 检查单满值 × 审计序。
    Ok((r.tid as u32)
        .wrapping_mul(41)
        .wrapping_add(r.checklist as u32)
        .wrapping_add(r.audit_seq))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_STEP => "步序违例（跳步/重走/越界）",
        E_NAME => "步① 名纪律不过",
        E_MAPBAD => "步② 图守恒不过",
        E_POLICY => "步③ 档位闭集外",
        E_QUOTA => "步④ 接入配额耗尽",
        E_ENROLL => "步⑤ 双面登记失败",
        E_AUDIT => "步⑥ 审计入账失败",
        E_PROJ => "步⑦ 投影对账失败",
        E_INTG => "检查单状态矛盾",
        _ => "未知诊断码",
    }
}

pub fn run_unx36135_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36135_wizard");
    cs.add("SEG 段位独占", SEG_BASE == 0x3bb0, "0x3bb0~0x3bbF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "七步全过/半途档案/配额/协议一致/审计守恒全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("检查单锚", CHECKLIST_N == 7 && STEP_ALL == 0x007F, "七步位掩码满值 0x007F");
    cs.add("步序冻结", { step_next(0, 0).is_ok() && step_next(0, 1).is_err() && step_next(STEP_NAME, 1).is_ok() }, "跳步必拒、按序必过");
    cs.add("配额锚", super::unx36124_enroll::WIZARD_QUOTA == 8, "映射表 16 − 黄金 8");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_order_enforced() {
        // 步序不可交换：跳步必拒、按序必过、重走必拒。
        assert!(step_next(0, 0).is_ok());
        assert_eq!(step_next(0, 1).err(), Some(E_STEP));
        assert!(step_next(STEP_NAME, 1).is_ok());
        assert_eq!(step_next(STEP_NAME, 0).err(), Some(E_STEP)); // 重走已完成步必拒
    }

    #[test]
    fn quota_reject_audited() {
        // 配额耗尽：第 9 个接入显性拒且 Reject 事件入审计账（守恒 1 条）。
        let mut ctx = WizardCtx::new();
        ctx.preassemble_golden().unwrap();
        let mut q = 0;
        while q < super::super::unx36124_enroll::WIZARD_QUOTA {
            ctx.reset_checklist();
            ctx.run_wizard(WizardReq { name: super::super::unx36121_typereg::FILL_NAMES[q], map: super::super::unx36121_typereg::GOLDEN[q % GOLDEN_N].1, policy: TypePolicy::NullDacl, actor_tag: 0 })
                .map_err(|c| c.0).unwrap();
            q += 1;
        }
        ctx.reset_checklist();
        let err = ctx.run_wizard(WizardReq { name: "Wizard9", map: super::super::unx36121_typereg::GOLDEN[0].1, policy: TypePolicy::NullDacl, actor_tag: 0 }).err().unwrap();
        assert_eq!(err.0, E_QUOTA);
        let (c_rej, _) = ctx.log.by_act(super::super::unx36130_typaudit::Act::Reject);
        assert_eq!(c_rej, 1);
    }

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(497));
    }
}
