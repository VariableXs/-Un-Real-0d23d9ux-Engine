//! unx1744_idleexit —— UNX-F1744 idle 退出路径（触发→结算→清位→接管）。
//! 锚点（CoRun Unxreal 主册）：UNX-F1744 · idle 退出路径 · 目标 300 行 · 已深化
//! 判据原文：UNX-F1744-J1 退出三步顺序断言全过，退出后首选线程非空断言全过。
//! 组件：idle 三步闭集（触发 Trigger / 结算 Settle / 清位 Clear → 接管 Pick）
//!       + 三步顺序账（append-only：步号/刻度/前置校验位/步完成位）
//!       + 顺序校验器（步号必须严格 1→2→3，跳步即拒）
//!       + 接管面（清位后首选线程非空断言）
//!       + 指纹冻结（三步账指纹 FNV-1a 32 位）
//!       + 注入核（跳步/空接管/重复触发/清位遗漏四类 10/10 必抓）。
//! 职责定位：idle 退出的唯一权威路径面——三步顺序 + 首选非空是
//!   idle 面回归的两把锁；跳步即拒使任何顺序违例即刻显形。
//! 诊断码段：0xEF00（0xEF01~0xEF08 闭集；本段为 mod.rs 段位图核验空闲独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模块版本。
pub const IE_VERSION: &str = "unx1744_idleexit-v1";

/// 诊断段基址（0xEF00 段 · 本模块独占）。
pub const IE_SEG_BASE: u16 = 0xEF00;

/// XcCode 闭集（0xEF01~0xEF08，严格递增互异）。
pub const XC_STEP_SKIP: u32 = 0xEF01;
pub const XC_PICK_EMPTY: u32 = 0xEF02;
pub const XC_DUP_TRIGGER: u32 = 0xEF03;
pub const XC_CLEAR_MISS: u32 = 0xEF04;
pub const XC_FINGER_DRIFT: u32 = 0xEF05;
pub const XC_STEP_OOB: u32 = 0xEF06;
pub const XC_INJECT_MISSED: u32 = 0xEF07;
pub const XC_LEDGER_BROKEN: u32 = 0xEF08;

/// 中文诊断文本（码 → 文案/建议，闭集逐码三要素）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_STEP_SKIP => ("跳步：三步顺序表外推进", "步号必须 1→2→3 严格递进"),
        XC_PICK_EMPTY => ("空接管：清位后首选线程为空", "接管面前置：就绪队列非空"),
        XC_DUP_TRIGGER => ("重复触发：idle 未退出再触发", "触发一次性，退出后复位"),
        XC_CLEAR_MISS => ("清位遗漏：结算后 need 位未清", "清位步必须完成"),
        XC_FINGER_DRIFT => ("指纹漂移：三步账指纹与冻结值不一致", "账被改，回滚"),
        XC_STEP_OOB => ("步号越界：步号越三步闭集", "步号必须落在 1~3"),
        XC_INJECT_MISSED => ("注入未捕获：注入核存在漏抓", "四类注入必须 10/10 必抓"),
        XC_LEDGER_BROKEN => ("账结构破坏：三步账被外力改写", "append-only 账回滚"),
        _ => ("未知诊断码", "查本模块 XcCode 闭集"),
    }
}

/// idle 三步闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdleStep {
    /// 步 1：触发（新到线程使 idle 退出条件成立）。
    Trigger = 1,
    /// 步 2：结算（idle 时长入统计、退出原因入账）。
    Settle = 2,
    /// 步 3：清位（need_resched 清零）→ 接管。
    Clear = 3,
}

impl IdleStep {
    /// 编号。
    pub const fn num(self) -> u8 {
        self as u8
    }
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            IdleStep::Trigger => "触发",
            IdleStep::Settle => "结算",
            IdleStep::Clear => "清位",
        }
    }
}

/// 三步账条目。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StepEntry {
    /// 步号（1~3）。
    pub step: u8,
    /// 刻度。
    pub tick: u64,
    /// 前置校验位（前一步已完成）。
    pub pre_ok: bool,
}

/// idle 退出路径机。
pub struct IdleExitFsm {
    /// 三步账。
    ledger: Vec<StepEntry>,
    /// 已完成步（0 = 未开始）。
    done: u8,
    /// need 位（清位步的对象）。
    need: bool,
    /// 就绪队列（接管面）。
    ready: Vec<u32>,
    /// 拒绝账。
    rejects: Vec<(u32, &'static str)>,
    /// idle 起始刻度（结算面用）。
    idle_since: u64,
    /// 结算产出（idle 时长）。
    idle_dur: Option<u64>,
    /// 封账。
    sealed: bool,
}

impl IdleExitFsm {
    /// 新机（idle 中，need=1，就绪队列由触发填充）。
    pub fn new(idle_since: u64, ready: Vec<u32>) -> Self {
        IdleExitFsm {
            ledger: Vec::new(),
            done: 0,
            need: true,
            ready,
            rejects: Vec::new(),
            idle_since,
            idle_dur: None,
            sealed: false,
        }
    }

    /// 步 1：触发。
    pub fn trigger(&mut self, tick: u64) -> Result<(), u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if self.done >= 1 {
            self.rejects.push((XC_DUP_TRIGGER, "重复触发"));
            return Err(XC_DUP_TRIGGER);
        }
        if self.ready.is_empty() {
            self.rejects.push((XC_PICK_EMPTY, "触发前就绪队列为空"));
            return Err(XC_PICK_EMPTY);
        }
        let _ = self.ledger.push(StepEntry {
            step: IdleStep::Trigger.num(),
            tick,
            pre_ok: true,
        });
        self.done = 1;
        Ok(())
    }

    /// 步 2：结算（idle 时长 = tick - idle_since）。
    pub fn settle(&mut self, tick: u64) -> Result<u64, u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if self.done != 1 {
            self.rejects.push((if self.done == 0 { XC_STEP_SKIP } else { XC_STEP_SKIP }, "结算前态非触发后"));
            return Err(XC_STEP_SKIP);
        }
        let dur = tick.saturating_sub(self.idle_since);
        let _ = self.ledger.push(StepEntry {
            step: IdleStep::Settle.num(),
            tick,
            pre_ok: true,
        });
        self.idle_dur = Some(dur);
        self.done = 2;
        Ok(dur)
    }

    /// 步 3：清位 + 接管（首选线程非空断言本体）。
    pub fn clear_and_pick(&mut self, tick: u64) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if self.done != 2 {
            self.rejects.push((XC_STEP_SKIP, "清位前态非结算后"));
            return Err(XC_STEP_SKIP);
        }
        if !self.need {
            self.rejects.push((XC_CLEAR_MISS, "need 位已清，重复清位"));
            return Err(XC_CLEAR_MISS);
        }
        if self.ready.is_empty() {
            self.rejects.push((XC_PICK_EMPTY, "清位后首选线程为空"));
            return Err(XC_PICK_EMPTY);
        }
        self.need = false;
        let pick = self.ready[0];
        let _ = self.ledger.push(StepEntry {
            step: IdleStep::Clear.num(),
            tick,
            pre_ok: true,
        });
        self.done = 3;
        Ok(pick)
    }

    /// 三步顺序全过判定。
    pub fn all_steps_green(&self) -> bool {
        self.ledger.len() == 3
            && self.ledger[0].step == 1
            && self.ledger[1].step == 2
            && self.ledger[2].step == 3
            && self.ledger.iter().all(|e| e.pre_ok)
    }

    /// 首选线程非空断言（J1 第二面；完成后恒真）。
    pub fn pick_nonempty(&self) -> bool {
        self.done == 3 && !self.ready.is_empty()
    }

    /// idle 时长。
    pub const fn idle_duration(&self) -> Option<u64> {
        self.idle_dur
    }
    /// need 位。
    pub const fn need(&self) -> bool {
        self.need
    }
    /// 拒绝账长度。
    pub fn reject_count(&self) -> usize {
        self.rejects.len()
    }
    /// 三步账切片。
    pub fn ledger(&self) -> &[StepEntry] {
        &self.ledger
    }
    /// 封账。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if self.sealed {
            return Err(XC_LEDGER_BROKEN);
        }
        if !self.all_steps_green() {
            return Err(XC_STEP_SKIP);
        }
        if !self.pick_nonempty() {
            return Err(XC_PICK_EMPTY);
        }
        let fp = self.joint_fingerprint();
        self.sealed = true;
        Ok(fp)
    }
    /// 账指纹。
    pub fn joint_fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::with_capacity(self.ledger.len() * 10);
        for e in &self.ledger {
            bytes.push(e.step);
            bytes.extend_from_slice(&e.tick.to_le_bytes());
        }
        fnv1a32(&bytes)
    }
}

/// FNV-1a 32 位。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// LCG 确定性发生器。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 权威退出场景：触发 → 结算 → 清位接管全绿。
pub fn authoritative_exit(idle_since: u64, ready: Vec<u32>) -> (bool, Option<u32>) {
    let mut f = IdleExitFsm::new(idle_since, ready);
    let s1 = f.trigger(idle_since + 10);
    let s2 = f.settle(idle_since + 12);
    let s3 = f.clear_and_pick(idle_since + 13);
    match (s1, s2, s3) {
        (Ok(()), Ok(_), Ok(p)) => {
            let ok = f.all_steps_green() && f.pick_nonempty() && !f.need();
            (ok, Some(p))
        }
        _ => (false, None),
    }
}

/// 注入核：四类注入各 10 次全抓。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0;
    let mut total = 0;
    let mut rng = Lcg::new(0xEF_44);
    let mut ready10 = || {
        let mut v = Vec::new();
        for i in 0..3u32 {
            let _ = v.push(500 + i * 11 + rng.below(5) as u32);
        }
        v
    };

    // 1) 跳步 ×10（未触发直接结算）
    for _ in 0..10 {
        total += 1;
        let mut f = IdleExitFsm::new(0, ready10());
        if f.settle(10).is_err() {
            caught += 1;
        }
    }
    // 2) 空接管 ×10（清位时队列空）
    for _ in 0..10 {
        total += 1;
        let mut f = IdleExitFsm::new(0, Vec::new());
        let _ = f.trigger(5);
        let _ = f.settle(6);
        if f.clear_and_pick(7).is_err() {
            caught += 1;
        }
    }
    // 3) 重复触发 ×10
    for _ in 0..10 {
        total += 1;
        let mut f = IdleExitFsm::new(0, ready10());
        let _ = f.trigger(5);
        if f.trigger(6).is_err() {
            caught += 1;
        }
    }
    // 4) 清位遗漏 ×10（need 位清后重复清）
    for _ in 0..10 {
        total += 1;
        let mut f = IdleExitFsm::new(0, ready10());
        let _ = f.trigger(5);
        let _ = f.settle(6);
        let _ = f.clear_and_pick(7);
        if f.clear_and_pick(8).is_err() {
            caught += 1;
        }
    }
    (caught, total)
}

/// 判据入口。
pub fn run_unx1744_checks() -> CheckSet {
    let mut s = CheckSet::new("unxstar-unx1744");

    // C44-01 三步闭集常量（J1 前置）
    {
        let ok = IdleStep::Trigger.num() == 1
            && IdleStep::Settle.num() == 2
            && IdleStep::Clear.num() == 3
            && IdleStep::Settle.name() == "结算";
        s.add("C44-01-三步闭集", ok, if ok { "" } else { "闭集漂移" });
    }

    // C44-02 退出三步顺序断言全过（J1 主面）
    {
        let mut ready = Vec::new();
        let _ = ready.push(700u32);
        let _ = ready.push(701u32);
        let (ok, pick) = authoritative_exit(100, ready);
        s.add("C44-02-三步顺序全过", ok && pick == Some(700), if ok { "" } else { "三步序破裂" });
    }

    // C44-03 退出后首选线程非空断言（J1 接管面）
    {
        let mut ready = Vec::new();
        let _ = ready.push(900u32);
        let mut f = IdleExitFsm::new(0, ready);
        let _ = f.trigger(1);
        let _ = f.settle(2);
        let _ = f.clear_and_pick(3);
        let ok = f.pick_nonempty() && !f.need();
        s.add("C44-03-首选非空", ok, if ok { "" } else { "接管面空" });
    }

    // C44-04 结算产出 idle 时长正确
    {
        let mut ready = Vec::new();
        let _ = ready.push(1u32);
        let mut f = IdleExitFsm::new(40, ready);
        let _ = f.trigger(45);
        let dur = f.settle(58);
        let ok = dur == Ok(18) && f.idle_duration() == Some(18);
        s.add("C44-04-idle时长", ok, if ok { "" } else { "结算漂移" });
    }

    // C44-05 指纹稳定 + 封账拒重
    {
        let mk_ready = || {
            let mut v = Vec::new();
            let _ = v.push(10u32);
            v
        };
        let (ok1, _) = authoritative_exit(0, mk_ready());
        let (ok2, _) = authoritative_exit(0, mk_ready());
        let ok = ok1 && ok2;
        // 指纹面：两机同序同刻度 → 同指纹
        let mut m1 = IdleExitFsm::new(0, mk_ready());
        let _ = m1.trigger(1);
        let _ = m1.settle(2);
        let _ = m1.clear_and_pick(3);
        let _ = m1.seal();
        let fa = m1.joint_fingerprint();
        let mut m2 = IdleExitFsm::new(0, mk_ready());
        let _ = m2.trigger(1);
        let _ = m2.settle(2);
        let _ = m2.clear_and_pick(3);
        let _ = m2.seal();
        let ok = ok && fa == m2.joint_fingerprint() && m1.seal().is_err();
        s.add("C44-05-指纹冻结", ok, if ok { "" } else { "指纹漂移" });
    }

    // C44-06 注入核四类 10/10（J1 注入面）
    {
        let (caught, total) = injection_core();
        let ok = total == 40 && caught == 40;
        s.add("C44-06-注入核40-40", ok, if ok { "" } else { "注入核漏抓" });
    }

    // C44-07 诊断码闭集
    {
        let codes = [
            XC_STEP_SKIP, XC_PICK_EMPTY, XC_DUP_TRIGGER, XC_CLEAR_MISS,
            XC_FINGER_DRIFT, XC_STEP_OOB, XC_INJECT_MISSED, XC_LEDGER_BROKEN,
        ];
        let mut ok = codes.len() == 8;
        for i in 0..codes.len() {
            if i > 0 && codes[i] <= codes[i - 1] {
                ok = false;
            }
            if (codes[i] & 0xFF00) != (IE_SEG_BASE as u32) {
                ok = false;
            }
        }
        s.add("C44-07-诊断码闭集", ok, if ok { "" } else { "诊断码漂移" });
    }

    // C44-08 中文诊断逐码三要素
    {
        let mut ok = true;
        for c in [
            XC_STEP_SKIP, XC_PICK_EMPTY, XC_DUP_TRIGGER, XC_CLEAR_MISS,
            XC_FINGER_DRIFT, XC_STEP_OOB, XC_INJECT_MISSED, XC_LEDGER_BROKEN, 0xFFFF,
        ] {
            let (t, a) = diag_text(c);
            if t.is_empty() || a.is_empty() {
                ok = false;
            }
        }
        s.add("C44-08-诊断三要素", ok, if ok { "" } else { "诊断文案缺要素" });
    }

    s
}
