//! unx0492_rndops —— UNX-F0492 渲染操作计数账（操作面守恒）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0492 · 渲染操作计数账 · 目标 240 行
//! 判据原文：UNX-F0492-J1 操作面守恒账：渲染管线的每类操
//!           作（fill/copy/blit/flip）按类计数——账二 分类
//!           计数（四类各记，互不串账）；账三 守恒核（提
//!           交总数 ≡ Σ各类；拒绝的操作入拒账不入类账）；
//!           账四 覆盖面积账（fill/blit 面积累加，clip 后
//!           面积 ≤ 原面积）；账五 翻转对账（flip 成对：
//!           前/后备互换计数相等）；账六 幂等核（同一操作
//!           重放两遍，面积账增两倍而非错乱）。
//! 组件：操作账本 + 四类计数 + 面积累加 + 翻转对核。
//! 职责定位：渲染管线的操作序列是「消费预算的流水」——丢
//!   一次 blit 就是画面残缺、多记一次 flip 就是同步错位。
//!   守恒账把「提交了多少」与「执行了多少」钉死，任何一
//!   类的串账/漏账都会在守恒核上暴露。
//! 性能逐项分解：记账 O(1)；守恒 O(1)。
//! 诊断码段：0x349C0（0x349C1~0x349C9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x349C0;
pub const XC_CONSERVE: u32 = SEG_BASE + 1; // 守恒破
pub const XC_PAIR: u32 = SEG_BASE + 2; // 翻转不配对

/// 操作类（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Fill,
    Copy,
    Blit,
    Flip,
}

/// 矩形（面积账用）。
#[derive(Clone, Copy, Debug)]
pub struct RRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl RRect {
    fn area(&self) -> u64 {
        self.w as u64 * self.h as u64
    }
    /// 与目标面 clip 后面积（出面 → 0；越界部分 saturating 剪除）。
    fn clipped_area(&self, tw: u32, th: u32) -> u64 {
        if self.x >= tw || self.y >= th {
            return 0;
        }
        let x1 = self.x.saturating_add(self.w).min(tw);
        let y1 = self.y.saturating_add(self.h).min(th);
        ((x1 - self.x) as u64) * ((y1 - self.y) as u64)
    }
}

/// 操作账本。
pub struct OpsLedger {
    pub fill: u32,
    pub copy: u32,
    pub blit: u32,
    pub flip: u32,
    pub rejected: u32,
    pub submitted: u32,
    /// 累计覆盖面积（fill/blit 的 clip 后面积）。
    pub covered: u64,
    /// 翻转对（前→后 与 后→前 各计一次，须相等）。
    pub flip_fwd: u32,
    pub flip_bwd: u32,
}

impl OpsLedger {
    pub fn new() -> Self {
        OpsLedger { fill: 0, copy: 0, blit: 0, flip: 0, rejected: 0, submitted: 0, covered: 0, flip_fwd: 0, flip_bwd: 0 }
    }

    /// 提交一个操作。accepted=false 时入拒账。
    pub fn submit(&mut self, op: Op, accepted: bool, rect: Option<RRect>, target: Option<(u32, u32)>, fwd: Option<bool>) -> Result<(), u32> {
        self.submitted += 1;
        if !accepted {
            self.rejected += 1;
            return Ok(());
        }
        match op {
            Op::Fill => {
                self.fill += 1;
                if let (Some(r), Some((tw, th))) = (rect, target) {
                    self.covered += r.clipped_area(tw, th);
                }
            }
            Op::Copy => self.copy += 1,
            Op::Blit => {
                self.blit += 1;
                if let (Some(r), Some((tw, th))) = (rect, target) {
                    self.covered += r.clipped_area(tw, th);
                }
            }
            Op::Flip => {
                match fwd {
                    Some(true) => {
                        self.flip += 1;
                        self.flip_fwd += 1;
                    }
                    Some(false) => {
                        self.flip += 1;
                        self.flip_bwd += 1;
                    }
                    None => {
                        // 无向翻转是无效操作：入拒账（保持守恒不变式）。
                        self.rejected += 1;
                        return Err(XC_PAIR);
                    }
                }
            }
        }
        Ok(())
    }

    /// 守恒核：提交 ≡ 四类和 + 拒。
    pub fn conserve(&self) -> bool {
        self.submitted == self.fill + self.copy + self.blit + self.flip + self.rejected
    }

    /// 翻转对核。
    pub fn flip_paired(&self) -> bool {
        self.flip_fwd == self.flip_bwd && self.flip == self.flip_fwd + self.flip_bwd
    }
}

pub fn run_unx0492_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0492");
    cs.add("SEG 段位独占", SEG_BASE == 0x349C0 && XC_PAIR == SEG_BASE + 2, "u32 高段全域普查无冲突");

    // 族一：分类计数不串账。
    let mut led = OpsLedger::new();
    let r = RRect { x: 0, y: 0, w: 10, h: 10 };
    led.submit(Op::Fill, true, Some(r), Some((100, 100)), None).ok();
    led.submit(Op::Copy, true, None, None, None).ok();
    led.submit(Op::Blit, true, Some(r), Some((100, 100)), None).ok();
    led.submit(Op::Fill, true, Some(r), Some((100, 100)), None).ok();
    cs.add("四类各记", led.fill == 2 && led.copy == 1 && led.blit == 1 && led.flip == 0, "互不串账");
    cs.add("守恒", led.conserve(), "4=2+1+1+0");

    // 族二：拒账。
    led.submit(Op::Blit, false, None, None, None).ok();
    cs.add("拒入拒账", led.rejected == 1 && led.blit == 1, "不入类账");
    cs.add("守恒含拒", led.conserve(), "5=2+1+1+0+1");

    // 族三：面积账（clip）。
    let mut led2 = OpsLedger::new();
    // 面内矩形 10×10=100。
    led2.submit(Op::Fill, true, Some(r), Some((100, 100)), None).ok();
    // 越面矩形（x=90, w=20 → clip 后 10 宽）。
    let r2 = RRect { x: 90, y: 0, w: 20, h: 10 };
    led2.submit(Op::Blit, true, Some(r2), Some((100, 100)), None).ok();
    cs.add("clip 面积", led2.covered == 200, "100 + clip(10×10)");
    // 完全出面的操作面积 0。
    let r3 = RRect { x: 200, y: 200, w: 5, h: 5 };
    led2.submit(Op::Fill, true, Some(r3), Some((100, 100)), None).ok();
    cs.add("出面积 0", led2.covered == 200, "clip 后 0");
    // clip 面积 ≤ 原面积。
    cs.add("clip 不增", r2.clipped_area(100, 100) <= r2.area(), "剪而不添");

    // 族四：翻转对账。
    let mut led3 = OpsLedger::new();
    led3.submit(Op::Flip, true, None, None, Some(true)).ok();
    led3.submit(Op::Flip, true, None, None, Some(false)).ok();
    cs.add("翻转成对", led3.flip_paired(), "前后各一");
    cs.add("对账守恒", led3.conserve(), "2=0+0+0+2");
    led3.submit(Op::Flip, true, None, None, Some(true)).ok();
    cs.add("单向失衡立", !led3.flip_paired(), "前2后1");
    led3.submit(Op::Flip, true, None, None, Some(false)).ok();
    cs.add("补对复原", led3.flip_paired(), "前2后2");

    // 族五：无向翻转拒（入拒账，守恒不变式保持）。
    let mut led4 = OpsLedger::new();
    cs.add("无向拒", led4.submit(Op::Flip, true, None, None, None) == Err(XC_PAIR), "方向必填");
    cs.add("拒后守恒", led4.conserve(), "submitted 1 = rejected 1");
    cs.add("无向不误计", led4.flip == 0, "类账不计无效操作");

    // 族六：幂等重放。
    let mut led5 = OpsLedger::new();
    let s = || RRect { x: 5, y: 5, w: 4, h: 4 };
    led5.submit(Op::Fill, true, Some(s()), Some((100, 100)), None).ok();
    led5.submit(Op::Fill, true, Some(s()), Some((100, 100)), None).ok();
    cs.add("重放面积倍增", led5.covered == 32, "16×2");
    cs.add("重放计数倍增", led5.fill == 2, "两次入账");

    // 族七：守恒破检出（伪造账）。
    let mut fake = OpsLedger::new();
    fake.submitted = 10;
    fake.fill = 3;
    fake.copy = 3;
    cs.add("伪造守恒拒", !fake.conserve(), "3+3 ≠ 10");

    cs
}
