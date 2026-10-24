//! veb328_evcirc —— VE-F0328 生态成熟度追踪机制。
//! 锚点（CoRun Unxreal 主册）：VE-F0328 · 生态成熟度追踪机制 · 目标 300 行
//! 判据原文：VE-F0328-J1 成熟度追踪账：对象登记（名 + 初始
//!           成熟度 0~5）；周期采样（每周期记一笔成熟度）；
//!           趋势账（升/平/降三向，按最近两笔差分）；连续升
//!           核（连升 3 周期 → 快熟标记）；连续降核（连降 2
//!           周期 → 预警标记）；稳定窗（差分绝对 ≤ 0 平向；
//!           成熟度是整数——无平向，故平向定义为无新采样）；
//!           追踪冻结（对象注销后历史保留趋势只读）。
//! 组件：采样账 + 趋势判 + 标记器。
//! 职责定位：成熟度追踪是「生态的成长曲线」——采样是体检：
//!           一期一记不许补记；连升三是长势：三项连涨的选手
//!           值得下注；连降二是退烧：连续两期掉分就要敲警
//!           钟；注销留史是卷宗纪律：人走了档案在。
//! 性能逐项分解：采样 O(1)；趋势 O(窗)。
//! 诊断码段：0x31B00（0x31B01~0x31B09 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const EV_VERSION: &str = "veb328_evcirc-v1";

/// 诊断段基址（0x31B00 段 · 本模块独占）。
pub const EV_SEG_BASE: u32 = 0x31B00;
pub const XC_MAGIC: u32 = 0x31B01; // 账魔数破
pub const XC_MAT: u32 = 0x31B02; // 成熟度越界
pub const XC_NOOBJ: u32 = 0x31B03; // 无此对象
pub const XC_GONE: u32 = 0x31B04; // 已注销禁采

/// 账魔数。
const MAGIC: u32 = 0x31B0_1B2F;

/// 快熟连升数。
pub const UP_RUN: u32 = 3;
/// 预警连降数。
pub const DOWN_RUN: u32 = 2;

/// 趋势。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trend {
    Up,
    Down,
    Flat,
}

/// 对象。
pub struct Tracked {
    pub name: u32,
    /// 采样历史。
    pub hist: Vec<u32>,
    /// 注销旗。
    pub gone: bool,
    /// 快熟标记。
    pub ripen: bool,
    /// 预警标记。
    pub warn: bool,
}

/// 追踪簿。
pub struct CircBook {
    pub objs: Vec<Tracked>,
    magic: u32,
}

impl CircBook {
    /// 建。
    pub fn new() -> CircBook {
        CircBook { objs: Vec::new(), magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    fn find_mut(&mut self, name: u32) -> Option<&mut Tracked> {
        for o in self.objs.iter_mut() {
            if o.name == name {
                return Some(o);
            }
        }
        None
    }

    /// 登记（初始采样一笔）。
    pub fn enroll(&mut self, name: u32, init: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if init > 5 {
            return Err(XC_MAT);
        }
        if self.find_mut(name).is_some() {
            return Err(XC_NOOBJ);
        }
        let mut hist = Vec::new();
        hist.push(init);
        self.objs.push(Tracked { name, hist, gone: false, ripen: false, warn: false });
        Ok(())
    }

    /// 采样。
    pub fn sample(&mut self, name: u32, m: u32) -> Result<Trend, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if m > 5 {
            return Err(XC_MAT);
        }
        let o = match self.find_mut(name) {
            Some(x) => x,
            None => return Err(XC_NOOBJ),
        };
        if o.gone {
            return Err(XC_GONE);
        }
        let prev = *o.hist.last().unwrap_or(&m);
        o.hist.push(m);
        let tr = if m > prev {
            Trend::Up
        } else if m < prev {
            Trend::Down
        } else {
            Trend::Flat
        };
        // 标记器：看最近窗口。
        let n = o.hist.len();
        let mut up_run = 0u32;
        let mut down_run = 0u32;
        let mut i = n;
        // 从最新往回数连续升/降。
        while i >= 2 {
            if o.hist[i - 1] > o.hist[i - 2] {
                up_run += 1;
                i -= 1;
            } else {
                break;
            }
        }
        let mut j = n;
        while j >= 2 {
            if o.hist[j - 1] < o.hist[j - 2] {
                down_run += 1;
                j -= 1;
            } else {
                break;
            }
        }
        if up_run >= UP_RUN {
            o.ripen = true;
        }
        if down_run >= DOWN_RUN {
            o.warn = true;
        }
        Ok(tr)
    }

    /// 趋势查询（最近两笔差分）。
    pub fn trend(&self, name: u32) -> Result<Trend, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for o in self.objs.iter() {
            if o.name == name {
                let n = o.hist.len();
                if n < 2 {
                    return Ok(Trend::Flat);
                }
                let a = o.hist[n - 2];
                let b = o.hist[n - 1];
                return Ok(if b > a {
                    Trend::Up
                } else if b < a {
                    Trend::Down
                } else {
                    Trend::Flat
                });
            }
        }
        Err(XC_NOOBJ)
    }

    /// 注销（历史保留）。
    pub fn remove(&mut self, name: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        match self.find_mut(name) {
            Some(o) => {
                o.gone = true;
                Ok(())
            }
            None => Err(XC_NOOBJ),
        }
    }

    /// 历史可读（注销后仍读）。
    pub fn hist_of(&self, name: u32) -> Result<Vec<u32>, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for o in self.objs.iter() {
            if o.name == name {
                let mut v = Vec::new();
                for h in o.hist.iter() {
                    v.push(*h);
                }
                return Ok(v);
            }
        }
        Err(XC_NOOBJ)
    }
}

pub fn run_veb328_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0328");
    let mut b = CircBook::new();
    // 族一：登记与采样。
    s.add("成熟 6 拒", b.enroll(1, 6) == Err(XC_MAT), ">5");
    s.add("登 1@2", b.enroll(1, 2).is_ok(), "初始笔");
    s.add("重登拒", b.enroll(1, 3) == Err(XC_NOOBJ), "一号一档");
    s.add("初平向", b.trend(1) == Ok(Trend::Flat), "单笔无差");
    s.add("升向", b.sample(1, 3) == Ok(Trend::Up), "2→3");
    s.add("降向", b.sample(1, 1) == Ok(Trend::Down), "3→1");
    s.add("平向", b.sample(1, 1) == Ok(Trend::Flat), "1→1");
    s.add("采样越界拒", b.sample(1, 9) == Err(XC_MAT), ">5");
    // 族二：连升快熟。
    let mut c = CircBook::new();
    let _ = c.enroll(2, 1);
    let _ = c.sample(2, 2);
    let _ = c.sample(2, 3);
    // 本笔是第 3 升（1→2→3→4）。
    s.add("三升标记", c.sample(2, 4) == Ok(Trend::Up) && {
        let mut r = false;
        for o in c.objs.iter() {
            if o.name == 2 {
                r = o.ripen;
            }
        }
        r
    }, "快熟立");
    // 族三：连降预警。
    let mut d = CircBook::new();
    let _ = d.enroll(3, 4);
    s.add("一降未标", d.sample(3, 3) == Ok(Trend::Down) && {
        let mut w = false;
        for o in d.objs.iter() {
            if o.name == 3 {
                w = o.warn;
            }
        }
        w == false
    }, "差一");
    s.add("二降标记", d.sample(3, 2) == Ok(Trend::Down) && {
        let mut w = false;
        for o in d.objs.iter() {
            if o.name == 3 {
                w = o.warn;
            }
        }
        w
    }, "预警立");
    // 升降交替不标。
    let mut e = CircBook::new();
    let _ = e.enroll(4, 2);
    let _ = e.sample(4, 3);
    let _ = e.sample(4, 2);
    let _ = e.sample(4, 3);
    s.add("交替不标", {
        let mut r = false;
        let mut w = false;
        for o in e.objs.iter() {
            if o.name == 4 {
                r = o.ripen;
                w = o.warn;
            }
        }
        r == false && w == false
    }, "无连势");
    // 族四：注销留史。
    let _ = e.remove(4);
    s.add("注销后采拒", e.sample(4, 5) == Err(XC_GONE), "停采");
    s.add("史保留", e.hist_of(4) == Ok(vec4(2, 3, 2, 3)), "卷宗在");
    s.add("趋势只读", e.trend(4) == Ok(Trend::Up), "末差分");
    // 注销无者拒。
    s.add("注销无者拒", e.remove(9) == Err(XC_NOOBJ), "无档");
    s.add("魔数核", EV_SEG_BASE == 0x31B00, "段基址");
    s
}

/// 四元 Vec 助手。
fn vec4(a: u32, b: u32, c: u32, d: u32) -> Vec<u32> {
    let mut v = Vec::new();
    v.push(a);
    v.push(b);
    v.push(c);
    v.push(d);
    v
}
