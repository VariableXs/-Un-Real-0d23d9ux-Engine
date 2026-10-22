// unx3522 —— UNX-F3522 探针槽注册与激活协议（槽表/激活计数/独占挂载）
// 判据原文 J1：槽协议（注册→激活→执行→去激活四态——激活计数器语义）；
// M-280 激活/去激活往返（状态机全转移 + 双挂载拒绝）。
// 落位：kernel/varix/src/unxstar/；诊断段 0xFD00 独占（0xFD00~0xFD7F）。
//
// 设计（纯功能 no_std）：
//   * 槽四态闭集：Free→Registered→Active→Registered（去激活回退），
//     执行仅在 Active 态合法；
//   * 激活计数器语义：激活 +1 / 去激活 −1，计数归零即回 Registered；
//     计数下溢（未激活先去激活）拒绝落账；
//   * 独占挂载：一槽同时至多一个探针体挂载，双挂载拒绝；
//   * 注入核：非法迁移/双挂载/计数下溢/封账后激活 四类必抓。

use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0xFD00 独占） ─────────────────────────────────────────────
pub const XC_PSL_TRANS: u32 = 0xFD01; // 非法迁移
pub const XC_PSL_DUAL: u32 = 0xFD02; // 双挂载
pub const XC_PSL_UNDER: u32 = 0xFD03; // 计数下溢
pub const XC_PSL_SEAL: u32 = 0xFD04; // 封账后激活
pub const XC_PSL_NOFN: u32 = 0xFD05; // 未注册先激活

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x01000193);
    }
    h
}

/// 槽四态闭集。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    Free,
    Registered,
    Active,
}

impl SlotState {
    pub fn name(self) -> &'static str {
        match self {
            SlotState::Free => "free",
            SlotState::Registered => "reg",
            SlotState::Active => "act",
        }
    }
    pub fn can_exec(self) -> bool {
        self == SlotState::Active
    }
}

/// 探针槽。
pub struct ProbeSlot {
    pub id: u32,
    state: SlotState,
    probe: Option<&'static str>, // 挂载的探针体（独占）
    act_count: u64,              // 激活计数器
    exec_cnt: u64,               // 执行次数
    events: Vec<(u32, &'static str)>, // 迁移留痕
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl ProbeSlot {
    pub fn new(id: u32) -> Self {
        ProbeSlot {
            id,
            state: SlotState::Free,
            probe: None,
            act_count: 0,
            exec_cnt: 0,
            events: Vec::new(),
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 注册：Free → Registered。
    pub fn register(&mut self, probe: &'static str) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_PSL_SEAL, "封账后注册"));
            return Err(XC_PSL_SEAL);
        }
        if self.state != SlotState::Free {
            self.rejects.push((XC_PSL_TRANS, "非 Free 态注册"));
            return Err(XC_PSL_TRANS);
        }
        self.state = SlotState::Registered;
        self.probe = Some(probe);
        self.events.push((0, "register"));
        Ok(())
    }

    /// 激活：Registered → Active（计数 +1）。
    pub fn activate(&mut self) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_PSL_SEAL, "封账后激活"));
            return Err(XC_PSL_SEAL);
        }
        if self.state == SlotState::Free {
            self.rejects.push((XC_PSL_NOFN, "未注册先激活"));
            return Err(XC_PSL_NOFN);
        }
        if self.state == SlotState::Active {
            self.rejects.push((XC_PSL_DUAL, "双挂载（重复激活）"));
            return Err(XC_PSL_DUAL);
        }
        self.state = SlotState::Active;
        self.act_count += 1;
        self.events.push((1, "activate"));
        Ok(())
    }

    /// 执行（仅 Active 态合法）。
    pub fn execute(&mut self) -> Result<u64, u32> {
        if !self.state.can_exec() {
            self.rejects.push((XC_PSL_TRANS, "非 Active 态执行"));
            return Err(XC_PSL_TRANS);
        }
        self.exec_cnt += 1;
        Ok(self.exec_cnt)
    }

    /// 去激活：Active → Registered（计数 −1，下溢拒）。
    pub fn deactivate(&mut self) -> Result<(), u32> {
        if self.state != SlotState::Active {
            self.rejects.push((XC_PSL_UNDER, "未激活去激活（下溢）"));
            return Err(XC_PSL_UNDER);
        }
        self.state = SlotState::Registered;
        self.act_count -= 1;
        self.events.push((2, "deactivate"));
        Ok(())
    }

    /// 状态机全转移核：注册→激活→执行→去激活→再激活 往返。
    pub fn roundtrip_ok(&mut self) -> bool {
        self.register("p1").is_ok()
            && self.activate().is_ok()
            && self.execute().is_ok()
            && self.deactivate().is_ok()
            && self.activate().is_ok()
            && self.execute().is_ok()
            && self.deactivate().is_ok()
    }

    pub fn state(&self) -> SlotState {
        self.state
    }
    pub fn activation_count(&self) -> u64 {
        self.act_count
    }
    pub fn exec_count(&self) -> u64 {
        self.exec_cnt
    }
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
    pub fn probe_name(&self) -> Option<&'static str> {
        self.probe
    }
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_PSL_SEAL);
        }
        self.sealed = true;
        Ok(())
    }
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 槽表：N 槽管理（独占挂载全局闸：同一探针体不得同时挂两槽）。
pub struct SlotTable {
    slots: Vec<ProbeSlot>,
    mounted: Vec<&'static str>, // 已挂载探针体名（全局独占）
    sealed: bool,
    rejects: Vec<(u32, &'static str)>,
}

impl SlotTable {
    pub fn new(n: usize) -> Self {
        let mut v = Vec::new();
        for i in 0..n {
            v.push(ProbeSlot::new(i as u32));
        }
        SlotTable {
            slots: v,
            mounted: Vec::new(),
            sealed: false,
            rejects: Vec::new(),
        }
    }

    /// 注册 + 独占挂载闸。
    pub fn mount(&mut self, slot: usize, probe: &'static str) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_PSL_SEAL, "封账后挂载"));
            return Err(XC_PSL_SEAL);
        }
        if slot >= self.slots.len() {
            self.rejects.push((XC_PSL_TRANS, "槽号越界"));
            return Err(XC_PSL_TRANS);
        }
        if self.mounted.contains(&probe) {
            self.rejects.push((XC_PSL_DUAL, "探针体已挂他槽（双挂载）"));
            return Err(XC_PSL_DUAL);
        }
        self.slots[slot].register(probe)?;
        self.mounted.push(probe);
        Ok(())
    }

    pub fn slot(&self, i: usize) -> Option<&ProbeSlot> {
        self.slots.get(i)
    }
    pub fn slot_mut(&mut self, i: usize) -> Option<&mut ProbeSlot> {
        self.slots.get_mut(i)
    }
    pub fn len(&self) -> usize {
        self.slots.len()
    }
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_PSL_SEAL);
        }
        self.sealed = true;
        Ok(())
    }
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
}

/// 激活/去激活往返 280 次（M-280 全转移核）。
pub fn roundtrip_280() -> (bool, u64, u64) {
    let mut sl = ProbeSlot::new(0);
    let _ = sl.register("rt280");
    let mut act = 0;
    let mut deact = 0;
    let mut ok = true;
    for _ in 0..280 {
        if sl.activate().is_ok() {
            act += 1;
        } else {
            ok = false;
        }
        if sl.execute().is_err() {
            ok = false;
        }
        if sl.deactivate().is_ok() {
            deact += 1;
        } else {
            ok = false;
        }
    }
    // 收尾态：Registered，计数归零
    ok = ok && sl.state() == SlotState::Registered && sl.activation_count() == 0;
    (ok, act, deact)
}

/// 注入核：非法迁移/双挂载/计数下溢/封账后激活 四类必抓。
pub struct SlotInjectCore {
    pub caught: u32,
    pub total: u32,
}

impl SlotInjectCore {
    pub fn new() -> Self {
        SlotInjectCore { caught: 0, total: 0 }
    }
    pub fn inject_illegal_trans(&mut self) {
        self.total += 1;
        let mut sl = ProbeSlot::new(1);
        // 未注册先激活 → 拒；再直接执行 → 拒
        let a = sl.activate();
        let e = sl.execute();
        if a.is_err() && e.is_err() && sl.reject_count() == 2 {
            self.caught += 1;
        }
    }
    pub fn inject_dual_mount(&mut self) {
        self.total += 1;
        let mut t = SlotTable::new(4);
        let m1 = t.mount(0, "probe-x");
        let m2 = t.mount(1, "probe-x"); // 同体挂他槽
        if m1.is_ok() && m2.is_err() && t.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn inject_underflow(&mut self) {
        self.total += 1;
        let mut sl = ProbeSlot::new(2);
        let _ = sl.register("p");
        let d = sl.deactivate(); // 未激活先去激活
        if d.is_err() && sl.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn inject_sealed_activate(&mut self) {
        self.total += 1;
        let mut sl = ProbeSlot::new(3);
        let _ = sl.register("p");
        let _ = sl.seal();
        let a = sl.activate();
        if a.is_err() && sl.reject_count() == 1 {
            self.caught += 1;
        }
    }
    pub fn all_caught(&self) -> bool {
        self.total == 4 && self.caught == 4
    }
}

/// 判据集。
pub fn run_unx3522_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx3522");

    // C22-01 状态机全转移往返
    {
        let mut sl = ProbeSlot::new(9);
        let ok = sl.roundtrip_ok();
        s.add("C22-01-全转移", ok, if ok { "" } else { "迁移断" });
    }
    // C22-02 激活/去激活往返 280 次全过
    {
        let (ok, act, deact) = roundtrip_280();
        s.add(
            "C22-02-往返280",
            ok && act == 280 && deact == 280,
            if ok && act == 280 { "" } else { "往返断" },
        );
    }
    // C22-03 执行仅在 Active 态
    {
        let mut sl = ProbeSlot::new(4);
        let _ = sl.register("px");
        let e1 = sl.execute();
        let _ = sl.activate();
        let e2 = sl.execute();
        let ok = e1.is_err() && e2.is_ok();
        s.add("C22-03-执行门", ok, if ok { "" } else { "执行门失守" });
    }
    // C22-04 独占挂载闸（全局）
    {
        let mut t = SlotTable::new(4);
        let m1 = t.mount(0, "pa");
        let m2 = t.mount(1, "pa");
        let m3 = t.mount(1, "pb");
        let ok = m1.is_ok() && m2.is_err() && m3.is_ok();
        s.add("C22-04-独占挂载", ok, if ok { "" } else { "双挂载放行" });
    }
    // C22-05 激活计数器语义（+1/−1 归零）
    {
        let mut sl = ProbeSlot::new(5);
        let _ = sl.register("pc");
        let _ = sl.activate();
        let c1 = sl.activation_count();
        let _ = sl.deactivate();
        let c2 = sl.activation_count();
        let ok = c1 == 1 && c2 == 0;
        s.add("C22-05-计数器", ok, if ok { "" } else { "计数断" });
    }
    // C22-06 迁移留痕（事件账序 register→activate→deactivate）
    {
        let mut sl = ProbeSlot::new(6);
        let _ = sl.register("pd");
        let _ = sl.activate();
        let _ = sl.deactivate();
        let ok = sl.event_count() == 3 && sl.reject_count() == 0;
        s.add("C22-06-留痕", ok, if ok { "" } else { "留痕断" });
    }
    // C22-07 封账拒挂载
    {
        let mut t = SlotTable::new(2);
        let _ = t.seal();
        let r = t.mount(0, "late");
        let ok = r.is_err() && t.reject_count() == 1;
        s.add("C22-07-封账拒", ok, if ok { "" } else { "封账失守" });
    }
    // C22-08 注入核 4/4
    {
        let mut inj = SlotInjectCore::new();
        inj.inject_illegal_trans();
        inj.inject_dual_mount();
        inj.inject_underflow();
        inj.inject_sealed_activate();
        s.add(
            "C22-08-注入核4-4",
            inj.all_caught(),
            if inj.all_caught() { "" } else { "注入漏抓" },
        );
    }
    s
}
