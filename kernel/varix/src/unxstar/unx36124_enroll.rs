//! UNX-F36124-J1 客体类型登记协议（J2 三级对象 B01 前向登记锚）。
//! 判据：① 五步协议（预核→闸门→绑定→配额→审计）顺序不可交换；
//! ② 每步失败显性码且不留半状态；③ 登记回执可重放；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：制度化入口——新对象类型接入的唯一合法路径。J2 三级对象
//! （B01 前向锚）与 36135 接入向导都消费本协议；任何绕开协议直写类型表
//! 或映射表的行为由对账面（36123 face_audit）在装配自检 36131 识破。
//! 协议五步引用：名纪律 36121::name_ok / 闸门 36122::access_gate /
//! 绑定 36123::bind_type_map / 配额=映射表容量-黄金数 / 审计=本模块事件账。

use crate::checks::CheckSet;
use super::unx36104_genmap::{GenMap, Registry as MapReg};
use super::unx36121_typereg::{
    self as tr, TypePolicy, TypeRegistry, GOLDEN_N, F_GOLDEN,
};
use super::unx36122_typedeny::{DenyLedger, GateRoute};
use super::unx36123_genbind::{bind_type_map, face_audit, BindCoord};

pub const SEG_BASE: u16 = 0x3b00;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_NAME: u16 = SEG_BASE + 2; // 步① 名纪律不过
pub const E_MAPBAD: u16 = SEG_BASE + 3; // 步① 图守恒不过
pub const E_GATED: u16 = SEG_BASE + 4; // 步② 闸门结论非预期（同名已注册走幂等而非新登记）
pub const E_FLAGGED: u16 = SEG_BASE + 5; // 步② 名字在拒绝账有 DenyName 前科
pub const E_BIND: u16 = SEG_BASE + 6; // 步③ 双面绑定失败
pub const E_QUOTA: u16 = SEG_BASE + 7; // 步④ 接入配额耗尽
pub const E_AUDIT: u16 = SEG_BASE + 8; // 步⑤ 审计入账失败
pub const E_INTG: u16 = SEG_BASE + 9; // 协议状态矛盾

/// 模块版本锚。
pub const VER: u32 = 1;

/// 接入配额：映射表容量 − 黄金八类 = 非黄金类型接入上限。
pub const WIZARD_QUOTA: usize = super::unx36123_genbind::CAP_MAP - GOLDEN_N;

/// 审计事件种类（判别式 = 0xB0 段）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EvKind {
    EnrollOk = 0x0B01,     // 登记成功（含回执坐标）
    EnrollIdem = 0x0B02,   // 幂等重登记（返回既有坐标）
    EnrollReject = 0x0B03, // 登记拒绝（含拒绝码）
    QuotaWarn = 0x0B04,    // 配额耗尽预警
}

impl EvKind {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 审计事件原子。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Ev {
    pub seq: u32,
    pub kind: EvKind,
    pub name: &'static str,
    pub arg: u16, // Ok: tid / Idem: tid / Reject: 诊断码 / Warn: 剩余配额
}

/// 固定容量审计账（追加式；满则协议拒绝而非顶替——登记是低频高价值事件）。
pub struct EvLog {
    buf: [Option<Ev>; 24],
    n: usize,
    seq_next: u32,
}

const NO_EV: Option<Ev> = None;

impl EvLog {
    pub fn new() -> EvLog {
        EvLog { buf: [NO_EV; 24], n: 0, seq_next: 1 }
    }

    pub fn count(&self) -> usize {
        self.n
    }

    /// 追加（满 → E_AUDIT，不顶替）。
    pub fn push(&mut self, kind: EvKind, name: &'static str, arg: u16) -> Result<u32, u16> {
        if self.n >= 24 {
            return Err(E_AUDIT);
        }
        let seq = self.seq_next;
        self.buf[self.n] = Some(Ev { seq, kind, name, arg });
        self.n += 1;
        self.seq_next = self.seq_next.wrapping_add(1);
        Ok(seq)
    }

    pub fn at(&self, i: usize) -> Result<Ev, u16> {
        if i >= self.n {
            return Err(E_AUDIT);
        }
        match self.buf[i] {
            Some(e) => Ok(e),
            None => Err(E_AUDIT),
        }
    }

    /// 账目守恒：seq 严格递增、无空洞。
    pub fn integrity(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.n {
            match self.buf[i] {
                Some(e) => {
                    if e.seq != i as u32 + 1 {
                        return Err(E_AUDIT);
                    }
                }
                None => return Err(E_AUDIT),
            }
            i += 1;
        }
        Ok(())
    }
}

/// 登记请求（协议入参闭集）。
#[derive(Clone, Copy)]
pub struct EnrollReq {
    pub name: &'static str,
    pub map: GenMap,
    pub policy: TypePolicy,
    pub golden: bool,
}

/// 登记回执（协议出参：双面坐标 + 审计序号）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct EnrollReceipt {
    pub coord: BindCoord,
    pub ev_seq: u32,
}

/// 协议上下文（四件套：类型表/映射表/拒绝账/审计账）。
pub struct EnrollCtx {
    pub reg: TypeRegistry,
    pub maps: MapReg,
    pub denied: DenyLedger,
    pub log: EvLog,
    pub wizard_used: usize,
}

impl EnrollCtx {
    pub fn new() -> EnrollCtx {
        EnrollCtx {
            reg: TypeRegistry::new(),
            maps: MapReg::new(),
            denied: DenyLedger::new(),
            log: EvLog::new(),
            wizard_used: 0,
        }
    }
}

/// 五步登记协议（顺序不可交换：预核→闸门→绑定→配额→审计）。
/// 前科扫描：拒绝账内是否有该名的 DenyName 历史（FIFO 环形账逐条比名）。
fn has_deny_history(led: &DenyLedger, name: &str) -> bool {
    let mut i = 0;
    while i < led.count() {
        if let Ok(ev) = led.at(i) {
            if ev.reason == GateRoute::DenyName.code() && ev.name == name {
                return true;
            }
        }
        i += 1;
    }
    false
}

pub fn enroll(ctx: &mut EnrollCtx, req: EnrollReq) -> Result<EnrollReceipt, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    // 步①：预核（名纪律 + 图守恒 + 策略闭集）——不碰任何表。
    if !TypeRegistry::name_ok(req.name) {
        let _ = ctx.log.push(EvKind::EnrollReject, req.name, E_NAME);
        return Err(E_NAME);
    }
    if !req.map.wellformed() {
        let _ = ctx.log.push(EvKind::EnrollReject, req.name, E_MAPBAD);
        return Err(E_MAPBAD);
    }
    if req.policy.code() < 0x0001 || req.policy.code() > 0x0004 {
        return Err(E_INTG);
    }
    // 步②：闸门预核（36122）——DenyName 前科名拒（历史事实先于路由
    // 结论）；已注册名走幂等分支；未注册新名进入配额流。
    if has_deny_history(&ctx.denied, req.name) {
        ctx.denied.record(GateRoute::DenyName.code(), req.name);
        let _ = ctx.log.push(EvKind::EnrollReject, req.name, E_FLAGGED);
        return Err(E_FLAGGED);
    }
    let route = super::unx36122_typedeny::access_gate(&ctx.reg, req.name);
    if route == GateRoute::DenyName {
        ctx.denied.record(GateRoute::DenyName.code(), req.name);
        let _ = ctx.log.push(EvKind::EnrollReject, req.name, E_FLAGGED);
        return Err(E_FLAGGED);
    }
    let idem = route == GateRoute::Routed;
    // 步③：双面绑定（幂等名重绑返回原坐标；新名先核配额；
    // 黄金装配走协议但不占向导位——配额只对非黄金新名生效）。
    if !idem && !req.golden {
        if ctx.wizard_used >= WIZARD_QUOTA {
            let _ = ctx.log.push(EvKind::QuotaWarn, req.name, 0);
            return Err(E_QUOTA);
        }
    }
    let flags = if req.golden { F_GOLDEN } else { 0 };
    let coord = bind_type_map(&mut ctx.reg, &mut ctx.maps, req.name, req.map, req.policy, flags)
        .map_err(|_| E_BIND)?;
    // 步④：配额记账（幂等重登/黄金装配不耗配额）。
    if !idem && !req.golden {
        ctx.wizard_used += 1;
    }
    // 步⑤：审计入账（回执带序号；账满协议失败——登记事件不许丢）。
    let kind = if idem { EvKind::EnrollIdem } else { EvKind::EnrollOk };
    let ev_seq = ctx.log.push(kind, req.name, coord.tid)?;
    ctx.log.integrity()?;
    face_audit(&ctx.reg, &ctx.maps).map_err(|_| E_BIND)?;
    Ok(EnrollReceipt { coord, ev_seq })
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：黄金八类走协议装配（与 36121 直装配等价，且带审计账）。
    let mut ctx = EnrollCtx::new();
    let mut i = 0;
    while i < GOLDEN_N {
        let (name, map, policy, _) = tr::GOLDEN[i];
        let r = enroll(&mut ctx, EnrollReq { name, map, policy, golden: true })?;
        if r.coord.tid != tr::TID_BASE + i as u16 {
            return Err(super::unx36123_genbind::E_SEQ);
        }
        i += 1;
    }
    if ctx.reg.count() != GOLDEN_N || ctx.maps.count() != GOLDEN_N {
        return Err(E_BIND);
    }
    if ctx.log.count() != GOLDEN_N {
        return Err(E_AUDIT);
    }
    // R2：幂等重登记（同参 → 原回执坐标 + EnrollIdem 事件）。
    let (gname, gmap, gpol, _) = tr::GOLDEN[0];
    let r1 = enroll(&mut ctx, EnrollReq { name: gname, map: gmap, policy: gpol, golden: true })?;
    let r2 = enroll(&mut ctx, EnrollReq { name: gname, map: gmap, policy: gpol, golden: true })?;
    if r1.coord != r2.coord {
        return Err(E_BIND);
    }
    let last = ctx.log.at(ctx.log.count() - 1)?;
    if last.kind != EvKind::EnrollIdem {
        return Err(E_AUDIT);
    }
    // R3：步①拒绝（名纪律/图守恒）且不留半状态。
    if enroll(&mut ctx, EnrollReq { name: "", map: gmap, policy: gpol, golden: false }).err() != Some(E_NAME) {
        return Err(E_NAME);
    }
    let badmap = GenMap { read: 0x8000_0000 | gmap.read, write: gmap.write, exec: gmap.exec };
    if enroll(&mut ctx, EnrollReq { name: "OkName", map: badmap, policy: gpol, golden: false }).err() != Some(E_MAPBAD) {
        return Err(E_MAPBAD);
    }
    // 半状态核：两张表与配额都未动。
    if ctx.reg.count() != GOLDEN_N || ctx.maps.count() != GOLDEN_N || ctx.wizard_used != 0 {
        return Err(E_INTG);
    }
    // R4：步①先于前科核——非可见名在步①即被拦（纪律顺序的显性证明）。
    if enroll(&mut ctx, EnrollReq { name: "Sp ace", map: gmap, policy: gpol, golden: false }).err() != Some(E_NAME) {
        return Err(E_NAME);
    }
    // 前科名核（合法名先入拒绝账再登记 → E_FLAGGED）：
    // 构造方式：向拒绝账直接 record 一个合法名（模拟 36122 判 DenyName 的历史事实）。
    ctx.denied.record(GateRoute::DenyName.code(), "Flagged");
    if enroll(&mut ctx, EnrollReq { name: "Flagged", map: gmap, policy: gpol, golden: false }).err() != Some(E_FLAGGED) {
        return Err(E_FLAGGED);
    }
    // R5：配额耗尽（向导位 8 用满后第 9 个非黄金名 → E_QUOTA）。
    // 用 FILL_NAMES（非黄金名）走协议，黄金名重登走幂等不耗配额。
    let mut q = 0;
    while q < WIZARD_QUOTA {
        let (gmap_q, gpol_q) = (tr::GOLDEN[q].1, tr::GOLDEN[q].2);
        enroll(&mut ctx, EnrollReq { name: tr::FILL_NAMES[q], map: gmap_q, policy: gpol_q, golden: false }).map_err(|_| E_BIND)?;
        q += 1;
    }
    if ctx.wizard_used != WIZARD_QUOTA {
        return Err(E_QUOTA);
    }
    if enroll(&mut ctx, EnrollReq { name: "Wizard9", map: gmap, policy: gpol, golden: false }).err() != Some(E_QUOTA) {
        return Err(E_QUOTA);
    }
    // QuotaWarn 事件在账。
    let mut saw_warn = false;
    let mut e = 0;
    while e < ctx.log.count() {
        if ctx.log.at(e)?.kind == EvKind::QuotaWarn {
            saw_warn = true;
        }
        e += 1;
    }
    if !saw_warn {
        return Err(E_AUDIT);
    }
    // R6：审计账守恒（seq 严格递增无空洞）。
    ctx.log.integrity()?;
    // 校验和：回执 tid 全并 × 事件种数。
    let mut sum: u32 = 0;
    let mut k = 0;
    while k < GOLDEN_N {
        sum = sum.wrapping_add(ctx.reg.tid_of(tr::GOLDEN_NAMES[k]).map_err(|_| E_INTG)? as u32);
        k += 1;
    }
    Ok(sum.wrapping_mul(7).wrapping_add(ctx.log.count() as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NAME => "步①名纪律不过",
        E_MAPBAD => "步①图守恒不过",
        E_GATED => "步②闸门结论非预期",
        E_FLAGGED => "步②名字有 DenyName 前科",
        E_BIND => "步③双面绑定失败",
        E_QUOTA => "步④接入配额耗尽",
        E_AUDIT => "步⑤审计入账失败（登记事件不许丢）",
        E_INTG => "协议状态矛盾",
        _ => "未知诊断码",
    }
}

pub fn run_unx36124_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36124_enroll");
    cs.add("SEG 段位独占", SEG_BASE == 0x3b00, "0x3b00~0x3b0F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "五步协议/幂等/半状态核/前科名/配额/审计守恒全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("事件种闭集", EvKind::EnrollOk.code() == 0x0B01 && EvKind::EnrollIdem.code() == 0x0B02 && EvKind::EnrollReject.code() == 0x0B03 && EvKind::QuotaWarn.code() == 0x0B04, "四事件判别式闭集（0xB0 段）");
    cs.add("配额锚", WIZARD_QUOTA == 8, "映射表 16 − 黄金 8 = 向导配额 8");
    cs.add("审计账容量", { let mut l = EvLog::new(); l.push(EvKind::EnrollOk, "A", 1).is_ok() && l.count() == 1 && l.integrity().is_ok() }, "追加式 24 槽；满即拒不顶替");
    cs.add("空账取越界", EvLog::new().at(0).err() == Some(E_AUDIT), "空账显性拒");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(275));
    }

    #[test]
    fn golden_no_wizard_slot() {
        // 黄金装配不占向导位：配额余量锚（黄金是装配事实，不走向导位）。
        assert_eq!(WIZARD_QUOTA, 8);
    }

    #[test]
    fn deny_history_blocks_route() {
        // 前科核：曾入拒账的名字可检出；无前科名不误伤。
        let mut led = DenyLedger::new();
        led.record(GateRoute::DenyName.code(), "FzX");
        assert!(has_deny_history(&led, "FzX"));
        assert!(!has_deny_history(&led, "FzY"));
        assert_eq!(led.count(), 1);
    }
}
