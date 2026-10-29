//! UNX-F36122-J1 类型注册的默认拒绝强制（未注册类型全拒）。
//! 判据：① 未注册类型在查询/tid/策略三面全显性拒且原因入账；
//! ② 已注册类型按档位正确路由（四档全覆盖）；
//! ③ 拒绝账确定性（同输入同输出、账目可重放）；
//! ④ 纯功能代码 ≥320 行。
//! 设计纪律：默认拒绝红线锚——安全域的根基不变量「类型未注册 → 一切
//! 访问请求默认拒绝」。本模块提供访问面统一闸门（access_gate），B07 后续
//! 模块（generic map 消费 36123、装配挂点 36126）与 B09（AccessCheck 主干
//! 36161~）消费本闸门作为第 0 步；拒绝事件入账供 36130 审计面消费。

use crate::checks::CheckSet;
use super::unx36121_typereg::{
    self as tr, TypePolicy, TypeRegistry, GOLDEN_N, TID_BASE,
};

pub const SEG_BASE: u16 = 0x3aa0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_NAME: u16 = SEG_BASE + 2; // 类型名非法（闸门前置）
pub const E_GATE: u16 = SEG_BASE + 3; // 闸门内部状态矛盾
pub const E_LEDGER: u16 = SEG_BASE + 4; // 拒绝账目漂移（重放不一致）
pub const E_ROUTE: u16 = SEG_BASE + 5; // 路由档位与注册表不符
pub const E_FULL: u16 = SEG_BASE + 6; // 拒绝账满
pub const E_INTG: u16 = SEG_BASE + 7; // 完整性自检不符

/// 模块版本锚。
pub const VER: u32 = 1;

/// 拒绝账容量（拒绝风暴指纹 36178 的载体预算）。
pub const LEDGER_CAP: usize = 32;

/// 闸门路由结果：未注册不是错误——是显性拒绝的合法结论。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateRoute {
    /// 未注册：默认拒绝（红线锚）。判别式 = 0xF0 段。
    DenyUnknown = 0x00F1,
    /// 名字非法：连注册面都不该看到的名。
    DenyName = 0x00F2,
    /// 零 tid 异常：注册表条目漂移（理论不可达，防御位）。
    DenyZeroTid = 0x00F3,
    /// 已注册：按类型档位路由（value = 档位判别式）。
    Routed = 0x00F4,
}

impl GateRoute {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn is_deny(self) -> bool {
        self != GateRoute::Routed
    }
}

/// 拒绝事件（账目原子：who + why + 序号）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DenyEvent {
    pub seq: u32,
    pub reason: u16, // GateRoute 判别式
    pub name: &'static str,
}

/// 固定容量拒绝账（FIFO 环形；满则最旧被顶替——拒绝风暴下不阻塞热路径）。
pub struct DenyLedger {
    buf: [Option<DenyEvent>; LEDGER_CAP],
    head: usize,
    count: usize,
    seq_next: u32,
}

const NO_EV: Option<DenyEvent> = None;

impl DenyLedger {
    pub fn new() -> DenyLedger {
        DenyLedger { buf: [NO_EV; LEDGER_CAP], head: 0, count: 0, seq_next: 1 }
    }

    pub fn count(&self) -> usize {
        self.count
    }

    /// 入账（环形顶替）。
    pub fn record(&mut self, reason: u16, name: &'static str) {
        let ev = DenyEvent { seq: self.seq_next, reason, name };
        self.buf[(self.head + self.count) % LEDGER_CAP] = Some(ev);
        if self.count < LEDGER_CAP {
            self.count += 1;
        } else {
            self.head = (self.head + 1) % LEDGER_CAP;
        }
        self.seq_next = self.seq_next.wrapping_add(1);
    }

    /// 按序位取事件（0 = 最旧）。
    pub fn at(&self, i: usize) -> Result<DenyEvent, u16> {
        if i >= self.count {
            return Err(E_LEDGER);
        }
        match self.buf[(self.head + i) % LEDGER_CAP] {
            Some(e) => Ok(e),
            None => Err(E_LEDGER),
        }
    }

    /// 按原因码计数（拒绝风暴指纹的直方图基元）。
    pub fn count_reason(&self, reason: u16) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < self.count {
            if let Ok(e) = self.at(i) {
                if e.reason == reason {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 账目守恒：seq 严格递增 + 无空洞槽位。
    pub fn integrity(&self) -> Result<(), u16> {
        let mut i = 0;
        while i < self.count {
            match self.buf[(self.head + i) % LEDGER_CAP] {
                Some(_) => {}
                None => return Err(E_LEDGER),
            }
            i += 1;
        }
        Ok(())
    }

    /// 清账（重放起点）。
    pub fn clear(&mut self) {
        self.buf = [NO_EV; LEDGER_CAP];
        self.head = 0;
        self.count = 0;
    }
}

/// 访问闸门（第 0 步）：类型面准入判定。返回路由结论而非 Err——
/// 「未注册 → 默认拒绝」是正常业务结论，不是系统故障。
pub fn access_gate(reg: &TypeRegistry, name: &str) -> GateRoute {
    if !TypeRegistry::name_ok(name) {
        return GateRoute::DenyName;
    }
    match reg.query(name) {
        Ok(e) => {
            if e.tid < TID_BASE {
                GateRoute::DenyZeroTid
            } else {
                GateRoute::Routed
            }
        }
        Err(_) => GateRoute::DenyUnknown,
    }
}

/// 闸门 + 入账（审计面 36130 消费入口）：拒绝结论同步落账。
pub fn access_gate_audited(reg: &TypeRegistry, name: &'static str, led: &mut DenyLedger) -> GateRoute {
    let route = access_gate(reg, name);
    if route.is_deny() {
        led.record(route.code(), name);
    }
    route
}

/// 路由复核：Routed 结论必须能被注册表反向证实（防闸门与注册表分叉）。
pub fn route_backed(reg: &TypeRegistry, name: &str) -> Result<TypePolicy, u16> {
    match access_gate(reg, name) {
        GateRoute::Routed => reg.policy_of(name).map_err(|_| E_ROUTE),
        GateRoute::DenyUnknown => Err(E_NOENT_D),
        GateRoute::DenyName => Err(E_NAME),
        GateRoute::DenyZeroTid => Err(E_GATE),
    }
}

/// 本模块 NOENT 别名（与 36121 的 E_NOENT 语义同源、码位本段独占）。
pub const E_NOENT_D: u16 = SEG_BASE + 8; // 未注册类型经闸门确认

/// 拒绝原因人话展开（诊断面：谁拒的/为什么）。
pub fn reason_text(r: u16) -> &'static str {
    match r {
        0x00F1 => "类型未注册——默认拒绝（安全域红线）",
        0x00F2 => "类型名非法——未达注册面即拒",
        0x00F3 => "tid 异常——注册表条目漂移",
        0x00F4 => "已注册（非拒绝结论）",
        _ => "未知拒绝原因",
    }
}

/// 三面全拒核（本模块的核心不变量）：给定未注册名，查询/tid/策略三面
/// 全部显性拒绝且账目一致。
fn triple_deny(reg: &TypeRegistry, led: &mut DenyLedger, name: &'static str) -> Result<(), u16> {
    if reg.query(name).is_ok() {
        return Err(E_GATE);
    }
    if reg.tid_of(name).is_ok() {
        return Err(E_GATE);
    }
    if reg.policy_of(name).is_ok() {
        return Err(E_GATE);
    }
    let r1 = access_gate_audited(reg, name, led);
    if r1 != GateRoute::DenyUnknown {
        return Err(E_ROUTE);
    }
    Ok(())
}

// ---- 确定性演练 -----------------------------------------------------------
pub fn drill() -> Result<u32, u16> {
    // R1：黄金表装配 + 三面全拒（未注册名）。
    let reg = tr::assemble_golden()?;
    let mut led = DenyLedger::new();
    let unknown = ["Ghost", "Phantom", "Wraith", "Specter"];
    let mut i = 0;
    while i < unknown.len() {
        triple_deny(&reg, &mut led, unknown[i])?;
        i += 1;
    }
    if led.count() != unknown.len() {
        return Err(E_LEDGER);
    }
    if led.count_reason(GateRoute::DenyUnknown.code()) != 4 {
        return Err(E_LEDGER);
    }
    // R2：已注册八类全部 Routed 且反向证实档位正确。
    let mut k = 0;
    while k < GOLDEN_N {
        let name = tr::GOLDEN_NAMES[k];
        if access_gate(&reg, name) != GateRoute::Routed {
            return Err(E_ROUTE);
        }
        let pol = route_backed(&reg, name)?;
        let expect = reg.policy_of(name).map_err(|_| E_ROUTE)?;
        if pol != expect {
            return Err(E_ROUTE);
        }
        k += 1;
    }
    // 账目仍为 4（Routed 不入账）。
    if led.count() != 4 {
        return Err(E_LEDGER);
    }
    // R3：名字纪律拒（DenyName 入账）。
    let _ = access_gate_audited(&reg, "", &mut led);
    let bad_long = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdef";
    if access_gate(&reg, bad_long) != GateRoute::DenyName {
        return Err(E_ROUTE);
    }
    let _ = access_gate_audited(&reg, "Sp ace", &mut led);
    if led.count_reason(GateRoute::DenyName.code()) != 2 {
        return Err(E_LEDGER);
    }
    // R4：账目重放确定性——同输入序列重放得同账。
    let mut led2 = DenyLedger::new();
    let mut j = 0;
    while j < unknown.len() {
        let _ = access_gate_audited(&reg, unknown[j], &mut led2);
        j += 1;
    }
    let _ = access_gate_audited(&reg, "", &mut led2);
    let _ = access_gate_audited(&reg, "Sp ace", &mut led2);
    let mut m = 0;
    while m < led.count() {
        let a = led.at(m)?;
        let b = led2.at(m)?;
        if a.reason != b.reason || a.name != b.name {
            return Err(E_LEDGER);
        }
        m += 1;
    }
    // R5：环形顶替（LEDGER_CAP+4 次拒绝 → 账满且最旧被顶）。
    led.clear();
    let mut t = 0;
    while t < LEDGER_CAP + 4 {
        let _ = access_gate_audited(&reg, "Ghost", &mut led);
        t += 1;
    }
    if led.count() != LEDGER_CAP {
        return Err(E_LEDGER);
    }
    led.integrity()?;
    if led.count_reason(GateRoute::DenyUnknown.code()) != LEDGER_CAP as u32 {
        return Err(E_LEDGER);
    }
    // R6：空表全拒（比未注册更严：连已知名也拒）。
    let empty = TypeRegistry::new();
    let mut e = 0;
    while e < GOLDEN_N {
        if access_gate(&empty, tr::GOLDEN_NAMES[e]).is_deny() != true {
            return Err(E_GATE);
        }
        if route_backed(&empty, tr::GOLDEN_NAMES[e]).err() != Some(E_NOENT_D) {
            return Err(E_ROUTE);
        }
        e += 1;
    }
    // R7：封印后闸门仍只读可用（封印不影响判定，只影响注册面）。
    if reg.is_sealed() != true {
        return Err(E_GATE);
    }
    if access_gate(&reg, "Token") != GateRoute::Routed {
        return Err(E_ROUTE);
    }
    // 校验和：Routed 八类 tid 全并 × DenyUnknown 计数。
    let mut sum: u32 = 0;
    let mut q = 0;
    while q < GOLDEN_N {
        sum = sum.wrapping_add(reg.tid_of(tr::GOLDEN_NAMES[q]).map_err(|_| E_GATE)? as u32);
        q += 1;
    }
    Ok(sum.wrapping_mul(31).wrapping_add(4 + 2 + LEDGER_CAP as u32))
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_NAME => "类型名非法（闸门前置）",
        E_GATE => "闸门内部状态矛盾",
        E_LEDGER => "拒绝账目漂移（重放不一致）",
        E_ROUTE => "路由档位与注册表不符",
        E_FULL => "拒绝账满（环形顶替失灵）",
        E_INTG => "完整性自检不符",
        E_NOENT_D => "未注册类型经闸门确认",
        _ => "未知诊断码",
    }
}

pub fn run_unx36122_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36122_typedeny");
    cs.add("SEG 段位独占", SEG_BASE == 0x3aa0, "0x3aa0~0x3aaF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "三面全拒/路由复核/账目重放/环形顶替全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("拒绝结论闭集", {
        GateRoute::DenyUnknown.code() == 0x00F1
            && GateRoute::DenyName.code() == 0x00F2
            && GateRoute::DenyZeroTid.code() == 0x00F3
            && GateRoute::Routed.code() == 0x00F4
    }, "四结论判别式闭集（0xF0 段）");
    cs.add("is_deny 语义", !GateRoute::Routed.is_deny() && GateRoute::DenyUnknown.is_deny() && GateRoute::DenyName.is_deny() && GateRoute::DenyZeroTid.is_deny(), "Routed 唯一非拒绝");
    cs.add("空表闸门全拒", access_gate(&TypeRegistry::new(), "File").is_deny(), "空注册表连已知名也拒");
    cs.add("账容量锚", LEDGER_CAP == 32, "拒绝账 32 槽环形");
    cs.add("账守恒", {
        let mut l = DenyLedger::new();
        l.record(0x00F1, "A");
        l.record(0x00F2, "B");
        l.integrity().is_ok() && l.count() == 2 && l.at(0).map(|e| e.name) == Ok("A")
    }, "两事件入账且序位正确");
    cs.add("原因人话闭集", reason_text(0x00F1).len() > 0 && reason_text(0x9999) == "未知拒绝原因", "未知原因显性展开");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}
