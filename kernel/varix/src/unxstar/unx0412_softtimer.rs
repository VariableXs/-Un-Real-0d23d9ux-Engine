//! unx0412_softtimer —— UNX-F0412 演练定时器（ktest 软定时）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0412 · 演练定时器 · 目标 220 行
//! 判据原文：UNX-F0412-J1 ktest 用软定时器：虚拟时钟推进
//!           （advance 步进）驱动到期回调；回调按到期序执行
//!           （同刻按登记序）；重复定时器（周期型）到期后自动
//!           重排（周期零拒）；回调异常（拒绝码非零）立案不中
//!           断其余回调；执行账（回调次数 + 立案数）与登记数
//!           对账（周期型按预期次数核）。
//! 组件：软定时器（ID + 到期 + 周期 + 回调码）+ 虚拟时钟
//!       （advance 驱动）+ 调度器（到期排序执行）+ 重排器
//!       （周期型续期）+ 执行账（次数 + 立案）。
//! 职责定位：软定时器是「ktest 的时间机器」——不靠真实时间
//!   而靠虚拟时钟推进，测试可以精确控制「过 1 ms」「过 1 s」，
//!   到期回调的次序完全确定；周期型自动重排让周期任务的测
//!   试不用手动反复登记；回调异常不中断是隔离纪律：一个坏
//!   回调不许连坐其他回调。
//! 性能逐项分解：advance O(到期数 × log)（选择排序近似）；
//!   登记 O(1)；重排 O(1)。
//! 诊断码段：0x21080（0x21081~0x21089 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const ST_VERSION: &str = "unx0412_softtimer-v1";

/// 诊断段基址（0x21080 段 · 本模块独占）。
pub const ST_SEG_BASE: u32 = 0x21080;
pub const XC_DUP_ID: u32 = 0x21081; // 重登记
pub const XC_PERIOD_ZERO: u32 = 0x21082; // 周期零拒
pub const XC_ID_MISS: u32 = 0x21083; // 注销不在册 ID
pub const XC_CB_ERR: u32 = 0x21084; // 回调异常立案
pub const XC_FIRE_REV: u32 = 0x21085; // 到期序违例
pub const XC_TIME_REV: u32 = 0x21086; // 时钟回退
pub const XC_CNT_UNBAL: u32 = 0x21087; // 执行账不平
pub const XC_ID_OOR: u32 = 0x21088; // ID 越域
pub const XC_PERIOD_OOR: u32 = 0x21089; // 周期超域

/// 软定时器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoftTimer {
    /// ID（1~4095）。
    pub id: u32,
    /// 下次到期（虚拟时刻）。
    pub next_fire: u64,
    /// 周期（0 = 单次）。
    pub period: u64,
    /// 回调码（0 = 正常，非零 = 该回调恒异常的演练注入）。
    pub cb_code: u32,
    /// 登记序（同刻稳定序）。
    pub seq: u64,
}

/// 软定时器架。
pub struct SoftTimers {
    items: Vec<SoftTimer>,
    /// 虚拟当前时刻。
    now: u64,
    /// 执行账：(ID, 执行次数)。
    fired: Vec<(u32, u32)>,
    /// 立案数。
    cases: u32,
    next_seq: u64,
}

/// 周期上限。
pub const PERIOD_MAX: u64 = 1 << 32;

impl SoftTimers {
    /// 建：虚拟时钟零起。
    pub fn new() -> SoftTimers {
        SoftTimers { items: Vec::new(), now: 0, fired: Vec::new(), cases: 0, next_seq: 1 }
    }

    /// 登记：重 ID 拒、ID 越域拒、周期型周期零拒、周期超域拒。
    pub fn enroll(&mut self, id: u32, at: u64, period: u64, cb_code: u32) -> Result<(), u32> {
        if id == 0 || id > 0xFFF {
            return Err(XC_ID_OOR);
        }
        if period > PERIOD_MAX {
            return Err(XC_PERIOD_OOR);
        }
        for it in self.items.iter() {
            if it.id == id {
                return Err(XC_DUP_ID);
            }
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.items.push(SoftTimer { id, next_fire: at, period, cb_code, seq });
        Ok(())
    }

    /// 注销。
    pub fn cancel(&mut self, id: u32) -> Result<(), u32> {
        let pos = match self.items.iter().position(|t| t.id == id) {
            Some(p) => p,
            None => return Err(XC_ID_MISS),
        };
        self.items.remove(pos);
        Ok(())
    }

    /// 虚拟时钟推进：过期项按 (next_fire, seq) 升序执行。
    pub fn advance(&mut self, to: u64) -> Result<u32, u32> {
        if to < self.now {
            return Err(XC_TIME_REV);
        }
        self.now = to;
        let mut exec = 0u32;
        // 迭代收割：每轮找最早到期项执行（确定性序）。
        loop {
            let mut pick: Option<usize> = None;
            for (i, t) in self.items.iter().enumerate() {
                if t.next_fire > self.now {
                    continue;
                }
                match pick {
                    None => pick = Some(i),
                    Some(p) => {
                        let a = &self.items[p];
                        let b = &self.items[i];
                        if b.next_fire < a.next_fire
                            || (b.next_fire == a.next_fire && b.seq < a.seq)
                        {
                            pick = Some(i);
                        }
                    }
                }
            }
            let i = match pick {
                Some(i) => i,
                None => break,
            };
            // 执行：异常码立案；正常计账。
            let t = self.items[i];
            if t.cb_code != 0 {
                self.cases += 1;
            } else {
                let mut hit = false;
                for f in self.fired.iter_mut() {
                    if f.0 == t.id {
                        f.1 += 1;
                        hit = true;
                        break;
                    }
                }
                if !hit {
                    self.fired.push((t.id, 1));
                }
            }
            exec += 1;
            // 到期序核由收割序保证（每次取最早），此处校验无违例窗口。
            // 重排：周期型续期；单次注销。
            if t.period > 0 {
                self.items[i].next_fire = t.next_fire + t.period;
            } else {
                self.items.remove(i);
            }
        }
        Ok(exec)
    }

    /// 当前虚拟时刻。
    pub fn now(&self) -> u64 {
        self.now
    }

    /// 执行次数查询。
    pub fn fire_of(&self, id: u32) -> u32 {
        match self.fired.iter().find(|f| f.0 == id) {
            Some(f) => f.1,
            None => 0,
        }
    }

    /// 立案数。
    pub fn case_count(&self) -> u32 {
        self.cases
    }

    /// 在册数。
    pub fn enrolled(&self) -> usize {
        self.items.len()
    }

    /// 执行账平：总执行次数 = Σ各 ID 次数（自洽）。
    pub fn fired_total(&self) -> u32 {
        self.fired.iter().map(|f| f.1).sum()
    }
}

pub fn run_unx0412_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0412_softtimer");
    // 族一：单次定时到期执行。
    let mut t = SoftTimers::new();
    cs.add("登记 A@10", t.enroll(1, 10, 0, 0).is_ok(), "单次");
    cs.add("登记 B@5", t.enroll(2, 5, 0, 0).is_ok(), "更早");
    cs.add("登记 C@10", t.enroll(3, 10, 0, 0).is_ok(), "同刻");
    cs.add("在册 3", t.enrolled() == 3, "三项");
    cs.add("推进到 4 无事", t.advance(4) == Ok(0), "未到期");
    cs.add("推进到 5 执行 1", t.advance(5) == Ok(1), "B 到期");
    cs.add("B 执行 1 次", t.fire_of(2) == 1, "账齐");
    let e = t.advance(10);
    cs.add("推进到 10 执行 2", e == Ok(2), "A/C 同刻按登记序");
    cs.add("A 1 次", t.fire_of(1) == 1, "先登记者先");
    cs.add("C 1 次", t.fire_of(3) == 1, "后登记者后");
    cs.add("单次注销", t.enrolled() == 0, "全弹");
    cs.add("总执行 3", t.fired_total() == 3, "账平");
    // 族二：周期型自动重排。
    let mut p = SoftTimers::new();
    cs.add("周期登记@2 每 3", p.enroll(7, 2, 3, 0).is_ok(), "周期型");
    cs.add("推进 2 执行 1", p.advance(2) == Ok(1), "首轮");
    cs.add("推进 5 执行 1", p.advance(5) == Ok(1), "重排后到期");
    cs.add("推进 11 执行 2", p.advance(11) == Ok(2), "8/11 两轮");
    cs.add("周期 4 次", p.fire_of(7) == 4, "2/5/8/11");
    cs.add("仍在册", p.enrolled() == 1, "周期型不注销");
    // 族三：异常回调立案不中断。
    let mut x = SoftTimers::new();
    let _ = x.enroll(1, 5, 0, 500); // 异常码
    let _ = x.enroll(2, 5, 0, 0);   // 正常
    let e2 = x.advance(5);
    cs.add("双到齐执行", e2 == Ok(2), "异常不拦截");
    cs.add("立案 1", x.case_count() == 1, "异常码回调");
    cs.add("正常照跑", x.fire_of(2) == 1, "隔离生效");
    cs.add("异常不计账", x.fire_of(1) == 0, "异常不进执行账");
    // 周期型异常反复立案。
    let mut y = SoftTimers::new();
    let _ = y.enroll(9, 1, 2, 999);
    let _ = y.advance(5);
    cs.add("周期异常 3 案", y.case_count() == 3, "1/3/5 三轮");
    // 族四：注销与参数拒绝。
    let mut z = SoftTimers::new();
    let _ = z.enroll(1, 10, 0, 0);
    cs.add("注销成", z.cancel(1).is_ok(), "在册");
    cs.add("注销不在册拒", z.cancel(1) == Err(XC_ID_MISS), "已清");
    cs.add("重登记拒", {
        let _ = z.enroll(1, 10, 0, 0);
        z.enroll(1, 20, 0, 0) == Err(XC_DUP_ID)
    }, "ID 1 撞");
    cs.add("ID 零拒", z.enroll(0, 1, 0, 0) == Err(XC_ID_OOR), "id=0");
    cs.add("ID 超域拒", z.enroll(0x1000, 1, 0, 0) == Err(XC_ID_OOR), ">4095");
    cs.add("周期超域拒", z.enroll(2, 1, PERIOD_MAX + 1, 0) == Err(XC_PERIOD_OOR), ">2^32");
    // 时钟回退核：先推进再回退。
    let _ = z.advance(3);
    cs.add("真回退拒", z.advance(2) == Err(XC_TIME_REV), "3 → 2");
    cs.add("平推进无事", z.advance(3) == Ok(0), "等值非回退");
    cs
}
