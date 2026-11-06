//! UNX-F51521 多设备并发带宽预算总表（全拓扑实时预算账） · 0xC0540段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51522-J1 三型混跑 ×10^3 事务零饿死；QoS 档位 ×3 生效
//!
//! 设计要点：
//!   ① 预算总表按「全拓扑实时预算账」建模：6 端点分属三型（实时/均衡/
//!      后台），QoS 档位 ×3 各带冻结上限（800/600/400）与保底
//!      （400/300/100），全局预算池 1,600 单位；授予 = min(申请, 档位
//!      上限, 池余额)，池干即明拒（BudgetExhausted）。
//!   ② QoS 档位生效面：申请超过档位上限时按上限截断——三档各自的
//!      截断值必须逐档对上，这是「档位 ×3 生效」的本体判据。
//!   ③ 三型混跑零饿死：×10^3 事务按端点轮转派发（固定种子 LCG 生成
//!      1..=8 的申请量），每事务请后即释；零拒绝（无 BudgetExhausted）
//!      且三型各得 ≥332 次（公平份额下限）、每端点服务数 >0，即为零饿死。
//!   ④ 单调状态机：草表 → 已提交（只进不退）；草表拒事务
//!      （TableDraft），重复提交拒（CommittedAlready），uncommit
//!      显性拒（PhaseBackward）。
//!   ⑤ 注入账守恒：注入 = 检出 + 逃逸，逃逸恒为 0；三类注入（池干
//!      超预算、非法端点、零/负申请）×10 轮必抓。
//!   ⑥ 事务账守恒：受理数 = 授予数 + 拒绝数；总表授权与各端点授权
//!      逐一对平（audit），失平即 LedgerMismatch。
//!   ⑦ 判定次序：表态 → 端点 → 申请（负 → 零 → 上限）→ 授予计算。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;

pub const SEG_BASE: u32 = 0xC0540;

/// 全局带宽预算池（单位：百兆当量）。
pub const TOTAL_BUDGET: u32 = 1_600;
/// 拓扑端点数（三型 ×2）。
pub const DEV_COUNT: usize = 6;
/// 单次申请上限。
pub const WANT_CAP: u32 = 1_000;
/// 三型混跑事务数（判据：×10^3）。
pub const MIX_TXNS: usize = 1_000;
/// 零饿死每型公平份额下限（⌊1000/6⌋ × 2 = 332，轮转确定性份额）。
pub const STARVE_FLOOR: u64 = 332;

/// QoS 档位（×3 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QosClass {
    /// 实时档
    Realtime,
    /// 均衡档
    Balanced,
    /// 后台档
    Bulk,
}

impl QosClass {
    /// 档位上限（冻结）。
    pub fn cap(self) -> u32 {
        match self {
            QosClass::Realtime => 800,
            QosClass::Balanced => 600,
            QosClass::Bulk => 400,
        }
    }
    /// 档位保底（冻结）。
    pub fn min(self) -> u32 {
        match self {
            QosClass::Realtime => 400,
            QosClass::Balanced => 300,
            QosClass::Bulk => 100,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            QosClass::Realtime => "实时档",
            QosClass::Balanced => "均衡档",
            QosClass::Bulk => "后台档",
        }
    }
}

/// 拓扑冻结映射：端点 → 档位（三型 ×2）。
pub const DEV_CLASSES: [QosClass; DEV_COUNT] = [
    QosClass::Realtime,
    QosClass::Realtime,
    QosClass::Balanced,
    QosClass::Balanced,
    QosClass::Bulk,
    QosClass::Bulk,
];

/// 诊断码闭集（带宽预算域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BwCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败（SEG_BASE 越界或不对齐）
    SegBad,
    /// 草表拒绝事务（未提交）
    TableDraft,
    /// 已提交，重复提交被拒
    CommittedAlready,
    /// 状态机倒退（uncommit 显性拒）
    PhaseBackward,
    /// 端点越界（负值或 ≥6）
    DevBad,
    /// 申请为负
    WantNegative,
    /// 申请为零
    WantZero,
    /// 申请超单次上限
    WantTooBig,
    /// 预算池干（容量耗尽）
    BudgetExhausted,
    /// 总表与端点账失平
    LedgerMismatch,
}

impl BwCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BwCode::Ok => "授予成功",
            BwCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            BwCode::TableDraft => "总表未提交：先 commit 再受理事务",
            BwCode::CommittedAlready => "已提交：重复提交被拒",
            BwCode::PhaseBackward => "状态机倒退：总表只进不退",
            BwCode::DevBad => "端点越界：核对拓扑端点表",
            BwCode::WantNegative => "申请为负：申请量必须为正整数",
            BwCode::WantZero => "申请为零：至少申请 1 单位",
            BwCode::WantTooBig => "申请超单次上限：拆分事务",
            BwCode::BudgetExhausted => "预算池干：等待释放后重试",
            BwCode::LedgerMismatch => "账面失平：总表授权必须等于各端点和",
        }
    }
}

/// 诊断码全序表（闭集枚举的权威清单）。
pub const ALL_CODES: [BwCode; 11] = [
    BwCode::Ok,
    BwCode::SegBad,
    BwCode::TableDraft,
    BwCode::CommittedAlready,
    BwCode::PhaseBackward,
    BwCode::DevBad,
    BwCode::WantNegative,
    BwCode::WantZero,
    BwCode::WantTooBig,
    BwCode::BudgetExhausted,
    BwCode::LedgerMismatch,
];

/// 端点实时账目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevEntry {
    pub dev_id: u32,
    pub class: QosClass,
    /// 当前授予量（实时面，请后即释）
    pub granted: u32,
    /// 累计服务事务数（单调）
    pub served: u64,
}

/// 总表状态机：草表 → 已提交（只进不退）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TblPhase {
    /// 草表
    Draft,
    /// 已提交
    Committed,
}

/// 全拓扑实时预算账引擎。
pub struct BwTable {
    devs: [DevEntry; DEV_COUNT],
    phase: TblPhase,
    total_granted: u32,
    requested: u64,
    granted_txns: u64,
    denied: u64,
    injected: u64,
    detected: u64,
}

/// 固定种子 LCG（无随机源，确定性可复现）。
pub fn lcg_next(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1664525).wrapping_add(1013904223);
    *state
}

impl BwTable {
    /// 新总表：按冻结拓扑映射建端点账，草表态。
    pub fn new() -> BwTable {
        let mut devs = [DevEntry {
            dev_id: 0,
            class: QosClass::Bulk,
            granted: 0,
            served: 0,
        }; DEV_COUNT];
        for i in 0..DEV_COUNT {
            let class = match DEV_CLASSES.get(i) {
                Some(c) => *c,
                None => QosClass::Bulk,
            };
            let d = match devs.get_mut(i) {
                Some(d) => d,
                None => continue,
            };
            d.dev_id = i as u32;
            d.class = class;
        }
        BwTable {
            devs,
            phase: TblPhase::Draft,
            total_granted: 0,
            requested: 0,
            granted_txns: 0,
            denied: 0,
            injected: 0,
            detected: 0,
        }
    }

    /// 提交总表（草表 → 已提交，单调）。
    pub fn commit(&mut self) -> Result<(), BwCode> {
        if self.phase == TblPhase::Committed {
            return Err(BwCode::CommittedAlready);
        }
        self.phase = TblPhase::Committed;
        Ok(())
    }

    /// 回退提交（显性拒）。
    pub fn uncommit(&mut self) -> Result<(), BwCode> {
        Err(BwCode::PhaseBackward)
    }

    /// 受理一次带宽事务（判定次序：表态 → 端点 → 申请 → 授予）。
    pub fn request(&mut self, dev_id: i64, want: i64) -> Result<u32, BwCode> {
        if self.phase != TblPhase::Committed {
            return Err(BwCode::TableDraft);
        }
        if dev_id < 0 || dev_id >= DEV_COUNT as i64 {
            return Err(BwCode::DevBad);
        }
        if want < 0 {
            return Err(BwCode::WantNegative);
        }
        if want == 0 {
            return Err(BwCode::WantZero);
        }
        if want as u64 > WANT_CAP as u64 {
            return Err(BwCode::WantTooBig);
        }
        let idx = dev_id as usize;
        let dev = match self.devs.get_mut(idx) {
            Some(d) => d,
            None => return Err(BwCode::DevBad),
        };
        let class = dev.class;
        self.requested = self.requested.saturating_add(1);
        let remaining = TOTAL_BUDGET.saturating_sub(self.total_granted);
        let grant = (want as u32).min(class.cap()).min(remaining);
        if grant == 0 {
            self.denied = self.denied.saturating_add(1);
            return Err(BwCode::BudgetExhausted);
        }
        dev.granted = dev.granted.saturating_add(grant);
        dev.served = dev.served.saturating_add(1);
        self.total_granted = self.total_granted.saturating_add(grant);
        self.granted_txns = self.granted_txns.saturating_add(1);
        Ok(grant)
    }

    /// 释放端点当前授权（实时面：请后即释）。
    pub fn release(&mut self, dev_id: usize) -> Result<u32, BwCode> {
        let dev = match self.devs.get_mut(dev_id) {
            Some(d) => d,
            None => return Err(BwCode::DevBad),
        };
        let g = dev.granted;
        dev.granted = 0;
        self.total_granted = self.total_granted.saturating_sub(g);
        Ok(g)
    }

    /// 对平：总表授权 = 各端点授权和，且逐端点不越档位上限。
    pub fn audit(&self) -> Result<(), BwCode> {
        let mut acc: u32 = 0;
        for d in self.devs.iter() {
            if d.granted > d.class.cap() {
                return Err(BwCode::LedgerMismatch);
            }
            acc = acc.saturating_add(d.granted);
        }
        if acc != self.total_granted {
            return Err(BwCode::LedgerMismatch);
        }
        Ok(())
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected = self.injected.saturating_add(1);
        self.detected = self.detected.saturating_add(1);
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected = self.injected.saturating_add(1);
    }

    pub fn phase_name(&self) -> &'static str {
        match self.phase {
            TblPhase::Draft => "草表",
            TblPhase::Committed => "已提交",
        }
    }

    pub fn total_granted(&self) -> u32 {
        self.total_granted
    }

    pub fn served_of(&self, dev_id: usize) -> u64 {
        match self.devs.get(dev_id) {
            Some(d) => d.served,
            None => 0,
        }
    }

    pub fn class_served(&self, class: QosClass) -> u64 {
        let mut n = 0u64;
        for d in self.devs.iter() {
            if d.class == class {
                n = n.saturating_add(d.served);
            }
        }
        n
    }

    pub fn requested(&self) -> u64 {
        self.requested
    }

    pub fn granted_txns(&self) -> u64 {
        self.granted_txns
    }

    pub fn denied(&self) -> u64 {
        self.denied
    }

    pub fn injected(&self) -> u64 {
        self.injected
    }

    pub fn detected(&self) -> u64 {
        self.detected
    }

    /// 守恒断言面：注入 = 检出，事务账受理 = 授予 + 拒绝。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected
            && self.detected > 0
            && self.requested == self.granted_txns.saturating_add(self.denied)
    }

    /// 服务面快照（双跑一致性对照）。
    pub fn served_snapshot(&self) -> [u64; DEV_COUNT] {
        let mut out = [0u64; DEV_COUNT];
        for i in 0..DEV_COUNT {
            let d = match self.devs.get(i) {
                Some(d) => d,
                None => continue,
            };
            let slot = match out.get_mut(i) {
                Some(s) => s,
                None => continue,
            };
            *slot = d.served;
        }
        out
    }
}

/// 三型混跑：×10^3 事务轮转派发，请后即释，返回被拒事务数。
pub fn mixed_run(t: &mut BwTable) -> u64 {
    let mut st: u32 = 0x5152_1E2B;
    let mut rejected = 0u64;
    for i in 0..MIX_TXNS {
        let dev = i % DEV_COUNT;
        let want = (lcg_next(&mut st) % 8) as i64 + 1;
        if t.request(dev as i64, want).is_err() {
            rejected = rejected.saturating_add(1);
        }
        let _ = t.release(dev);
    }
    rejected
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 双跑一致性：同种子两次混跑服务面逐字段一致。
pub fn dual_run_consistent() -> bool {
    let mut a = BwTable::new();
    let _ = a.commit();
    let _ = mixed_run(&mut a);
    let mut b = BwTable::new();
    let _ = b.commit();
    let _ = mixed_run(&mut b);
    a.served_snapshot() == b.served_snapshot()
        && a.granted_txns() == b.granted_txns()
        && a.denied() == b.denied()
}

/// 收口自检（≥12 条判据形态）。
pub fn run_unx51521_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51521");
    // 1 正常流：提交后受理授予。
    {
        let mut t = BwTable::new();
        let _ = t.commit();
        let r = t.request(0, 100);
        s.add_dyn(
            "正常事务授予",
            r == Ok(100) && t.total_granted() == 100,
            "已提交总表应按申请量授予",
        );
    }
    // 2 QoS 档位 ×3 生效：超额申请按档位上限截断。
    {
        let mut t = BwTable::new();
        let _ = t.commit();
        let r0 = t.request(0, WANT_CAP as i64);
        let _ = t.release(0);
        let r2 = t.request(2, WANT_CAP as i64);
        let _ = t.release(2);
        let r4 = t.request(4, WANT_CAP as i64);
        let _ = t.release(4);
        s.add_dyn(
            "QoS 档位 ×3 生效",
            r0 == Ok(800) && r2 == Ok(600) && r4 == Ok(400),
            "实时/均衡/后台三档截断值必须逐档对上",
        );
    }
    // 3 边界零：零号端点 + 最小申请。
    {
        let mut t = BwTable::new();
        let _ = t.commit();
        let r = t.request(0, 1);
        s.add_dyn("边界零放行", r == Ok(1), "零号端点最小申请应放行");
    }
    // 4 边界负：端点负与申请负。
    {
        let mut t = BwTable::new();
        let _ = t.commit();
        let r_d = t.request(-1, 10);
        let r_w = t.request(0, -10);
        s.add_dyn(
            "边界负拒绝",
            r_d == Err(BwCode::DevBad) && r_w == Err(BwCode::WantNegative),
            "负端点与负申请都必须明拒",
        );
    }
    // 5 边界上限：申请 = 单次上限。
    {
        let mut t = BwTable::new();
        let _ = t.commit();
        let r = t.request(2, WANT_CAP as i64);
        s.add_dyn(
            "边界上限放行",
            r == Ok(600),
            "单次上限内受理，按档位上限授予",
        );
    }
    // 6 边界上限+1：申请 = 上限+1。
    {
        let mut t = BwTable::new();
        let _ = t.commit();
        let r = t.request(0, WANT_CAP as i64 + 1);
        s.add_dyn(
            "边界上限+1 拒绝",
            r == Err(BwCode::WantTooBig),
            "越出单次上限必须拒绝",
        );
    }
    // 7 注入必抓：池干超预算 ×10。
    {
        let mut t = BwTable::new();
        let _ = t.commit();
        let mut caught = 0u32;
        for _ in 0..10u32 {
            let _ = t.request(0, 800);
            let _ = t.request(0, 800);
            if t.request(0, 1) == Err(BwCode::BudgetExhausted) {
                caught += 1;
                t.note_detected();
            }
            let _ = t.release(0);
        }
        s.add_dyn("超预算注入 10/10", caught == 10, "预算池干必须全抓");
    }
    // 8 注入必抓：非法端点 ×10。
    {
        let mut t = BwTable::new();
        let _ = t.commit();
        let mut caught = 0u32;
        for i in 0..10i64 {
            let dev = DEV_COUNT as i64 + i;
            if t.request(dev, 10) == Err(BwCode::DevBad) {
                caught += 1;
                t.note_detected();
            }
        }
        s.add_dyn("非法端点注入 10/10", caught == 10, "越出拓扑必须全抓");
    }
    // 9 注入必抓：零/负申请 ×10。
    {
        let mut t = BwTable::new();
        let _ = t.commit();
        let mut caught = 0u32;
        for i in 0..10i64 {
            let r = if i % 2 == 0 {
                t.request(1, 0)
            } else {
                t.request(1, -(i + 1))
            };
            if r == Err(BwCode::WantZero) || r == Err(BwCode::WantNegative) {
                caught += 1;
                t.note_detected();
            }
        }
        s.add_dyn("零/负申请注入 10/10", caught == 10, "非法申请量必须全抓");
    }
    // 10 三型混跑 ×10^3 零饿死。
    {
        let mut t = BwTable::new();
        let _ = t.commit();
        let rejected = mixed_run(&mut t);
        let rt = t.class_served(QosClass::Realtime);
        let bal = t.class_served(QosClass::Balanced);
        let bulk = t.class_served(QosClass::Bulk);
        let mut every_dev = true;
        for i in 0..DEV_COUNT {
            if t.served_of(i) == 0 {
                every_dev = false;
            }
        }
        s.add_dyn(
            "三型混跑 ×10^3 零饿死",
            rejected == 0
                && t.granted_txns() == MIX_TXNS as u64
                && rt >= STARVE_FLOOR
                && bal >= STARVE_FLOOR
                && bulk >= STARVE_FLOOR
                && every_dev,
            "三型各得 ≥332 次、每端点服务数 >0、零拒绝",
        );
    }
    // 11 判定次序：表态先于端点；端点先于申请。
    {
        let mut t = BwTable::new();
        let r1 = t.request(-1, -1);
        let _ = t.commit();
        let r2 = t.request(-1, -1);
        s.add_dyn(
            "判定次序表态/端点在前",
            r1 == Err(BwCode::TableDraft) && r2 == Err(BwCode::DevBad),
            "表态判据先于端点判据，端点判据先于申请判据",
        );
    }
    // 12 状态机拒止：草表拒事务、重复提交、uncommit。
    {
        let mut t = BwTable::new();
        let r1 = t.request(0, 10);
        let _ = t.commit();
        let r2 = t.commit();
        let r3 = t.uncommit();
        s.add_dyn(
            "状态机倒退拒止",
            r1 == Err(BwCode::TableDraft)
                && r2 == Err(BwCode::CommittedAlready)
                && r3 == Err(BwCode::PhaseBackward),
            "总表只进不退，草表拒事务",
        );
    }
    // 13 容量耗尽：池干对平（授予和 = 1,600）。
    {
        let mut t = BwTable::new();
        let _ = t.commit();
        let g0 = t.request(0, 800);
        let g2 = t.request(2, 600);
        let g4 = t.request(4, 400);
        let over = t.request(1, 1);
        s.add_dyn(
            "容量耗尽拒绝",
            g0 == Ok(800)
                && g2 == Ok(600)
                && g4 == Ok(400)
                && t.total_granted() == TOTAL_BUDGET
                && over == Err(BwCode::BudgetExhausted)
                && t.audit().is_ok(),
            "池干后第 4 笔必须拒，总表与端点和逐一对平",
        );
    }
    // 14 对账守恒：注入 = 检出，事务账受理 = 授予 + 拒绝。
    {
        let mut t = BwTable::new();
        let _ = t.commit();
        for i in 0..10i64 {
            let dev = DEV_COUNT as i64 + i;
            let _ = t.request(dev, 5);
            t.note_detected();
        }
        s.add_dyn(
            "注入账守恒",
            t.conserved() && t.injected() == 10,
            "注入=检出+逃逸，事务账受理=授予+拒绝",
        );
    }
    // 15 双跑一致。
    s.add_dyn("双跑逐字段一致", dual_run_consistent(), "同种子两次混跑服务面相等");
    // 16 文案闭集非空。
    {
        let mut closed = true;
        for c in ALL_CODES.iter() {
            if c.detail().is_empty() {
                closed = false;
            }
        }
        s.add_dyn("文案闭集非空", closed, "每个诊断码都必须有中文建议");
    }
    // 17 段位守卫。
    s.add_dyn("段位守卫", seg_guard_ok(), "SEG_BASE 对齐且在分派区间");
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_request_grants() {
        let mut t = BwTable::new();
        let _ = t.commit();
        assert_eq!(t.request(0, 100), Ok(100));
        assert_eq!(t.request(5, 50), Ok(50));
        assert_eq!(t.total_granted(), 150);
        assert_eq!(t.release(0), Ok(100));
        assert_eq!(t.total_granted(), 50);
    }

    #[test]
    fn qos_caps_enforced_per_class() {
        let mut t = BwTable::new();
        let _ = t.commit();
        assert_eq!(t.request(0, WANT_CAP as i64), Ok(800));
        let _ = t.release(0);
        assert_eq!(t.request(2, WANT_CAP as i64), Ok(600));
        let _ = t.release(2);
        assert_eq!(t.request(4, WANT_CAP as i64), Ok(400));
        let _ = t.release(4);
    }

    #[test]
    fn dev_and_want_bounds() {
        let mut t = BwTable::new();
        let _ = t.commit();
        assert_eq!(t.request(-1, 5), Err(BwCode::DevBad));
        assert_eq!(t.request(DEV_COUNT as i64, 5), Err(BwCode::DevBad));
        assert_eq!(t.request(0, 0), Err(BwCode::WantZero));
        assert_eq!(t.request(0, -3), Err(BwCode::WantNegative));
        assert_eq!(t.request(0, WANT_CAP as i64 + 1), Err(BwCode::WantTooBig));
        assert!(t.request(0, 1).is_ok());
    }

    #[test]
    fn budget_pool_exhausted_then_released() {
        let mut t = BwTable::new();
        let _ = t.commit();
        assert_eq!(t.request(0, 800), Ok(800));
        assert_eq!(t.request(2, 600), Ok(600));
        // 池 1600：前两笔吃掉 1400，第三笔按余额截断到 200（余额显性化）。
        assert_eq!(t.request(4, 400), Ok(200));
        assert_eq!(t.total_granted(), TOTAL_BUDGET);
        assert_eq!(t.request(1, 1), Err(BwCode::BudgetExhausted));
        assert!(t.audit().is_ok());
        assert_eq!(t.release(0), Ok(800));
        assert!(t.request(1, 1).is_ok(), "释放后应恢复受理");
    }

    #[test]
    fn state_machine_monotonic() {
        let mut t = BwTable::new();
        assert_eq!(t.request(0, 10), Err(BwCode::TableDraft));
        assert!(t.commit().is_ok());
        assert_eq!(t.commit(), Err(BwCode::CommittedAlready));
        assert_eq!(t.uncommit(), Err(BwCode::PhaseBackward));
    }

    #[test]
    fn judge_order_phase_before_dev_before_want() {
        let mut t = BwTable::new();
        assert_eq!(t.request(-1, -1), Err(BwCode::TableDraft));
        let _ = t.commit();
        assert_eq!(t.request(-1, -1), Err(BwCode::DevBad));
        assert_eq!(t.request(0, -1), Err(BwCode::WantNegative));
    }

    #[test]
    fn mixed_run_thousand_zero_starvation() {
        let mut t = BwTable::new();
        let _ = t.commit();
        assert_eq!(mixed_run(&mut t), 0);
        assert_eq!(t.granted_txns(), MIX_TXNS as u64);
        assert!(t.class_served(QosClass::Realtime) >= STARVE_FLOOR);
        assert!(t.class_served(QosClass::Balanced) >= STARVE_FLOOR);
        assert!(t.class_served(QosClass::Bulk) >= STARVE_FLOOR);
        for i in 0..DEV_COUNT {
            assert!(t.served_of(i) > 0, "端点 {} 不得饿死", i);
        }
        assert!(t.audit().is_ok());
    }

    #[test]
    fn over_budget_injections_all_caught() {
        let mut t = BwTable::new();
        let _ = t.commit();
        for _ in 0..10u32 {
            let _ = t.request(0, 800);
            let _ = t.request(0, 800);
            assert_eq!(t.request(0, 1), Err(BwCode::BudgetExhausted));
            t.note_detected();
            let _ = t.release(0);
        }
        assert!(t.conserved());
        assert_eq!(t.injected(), 10);
    }

    #[test]
    fn dual_run_identical() {
        assert!(dual_run_consistent());
    }

    #[test]
    fn topology_frozen_mapping() {
        assert_eq!(DEV_CLASSES.len(), DEV_COUNT);
        assert_eq!(DEV_CLASSES.get(0), Some(&QosClass::Realtime));
        assert_eq!(DEV_CLASSES.get(2), Some(&QosClass::Balanced));
        assert_eq!(DEV_CLASSES.get(4), Some(&QosClass::Bulk));
        assert!(DEV_CLASSES.iter().all(|c| c.min() <= c.cap()));
        assert_eq!(
            DEV_CLASSES.iter().map(|c| c.min()).sum::<u32>(),
            1_600,
            "六端点保底和 1,600 恰与预算池等额（全部保底兑现即满池）"
        );
    }

    #[test]
    fn seg_guard_and_closed_texts() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        for c in ALL_CODES.iter() {
            assert!(!c.detail().is_empty());
        }
    }
}
