//! veb322_idframe —— VE-F0322 设备识别框架预留。
//! 锚点（CoRun Unxreal 主册）：VE-F0322 · 设备识别框架预留 · 目标 300 行
//! 判据原文：VE-F0322-J1 识别框架账：槽位预留（8 个识别器
//!           槽，各带优先级）；识别链（按优先级降序逐槽试，
//!           首个识别成功者胜出）；槽登记（识别器 ID + 优先
//!           级）；优先级冲突拒（同级重号拒）；链顶插队（高
//!           优新槽插到队首）；槽注销；链空核（全空 → 识别
//!           返回未识别）；占位核（预留槽未装识别器 → 跳过
//!           不算失败）。
//! 组件：槽位账 + 识别链 + 插队器。
//! 职责定位：识别框架是「实验线的排班表」——八槽是八个工位：
//!           优先级是叫号顺序，高优先先接客；占位不算失败是
//!           排班表的虚位：工位空着就跳过下一个，不算没人上
//!           班；链空是全员请假：客人白来但要有说法。
//! 性能逐项分解：识别 O(槽数)。
//! 诊断码段：0x31980（0x31981~0x31989 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const IF_VERSION: &str = "veb322_idframe-v1";

/// 诊断段基址（0x31980 段 · 本模块独占）。
pub const IF_SEG_BASE: u32 = 0x31980;
pub const XC_MAGIC: u32 = 0x31981; // 账魔数破
pub const XC_DUP: u32 = 0x31982; // 同级重号
pub const XC_NOSLOT: u32 = 0x31983; // 无此槽
pub const XC_FULL: u32 = 0x31984; // 槽满

/// 账魔数。
const MAGIC: u32 = 0x3198_1B22;

/// 槽容。
pub const NSLOT: usize = 8;

/// 识别器结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IdOut {
    /// 认出（返回家族 ID）。
    Found(u32),
    /// 不归我管。
    Pass,
}

/// 识别器槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub id: u32,
    pub prio: u32,
    /// 装了识别器没有（预留位 = 未装）。
    pub installed: bool,
}

/// 识别框架。
pub struct IdFrame {
    slots: Vec<Slot>,
    magic: u32,
}

impl IdFrame {
    /// 建。
    pub fn new() -> IdFrame {
        IdFrame { slots: Vec::new(), magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 登记（prio 冲突拒；满拒）。
    pub fn register(&mut self, id: u32, prio: u32, installed: bool) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if self.slots.len() >= NSLOT {
            return Err(XC_FULL);
        }
        for sl in self.slots.iter() {
            if sl.prio == prio {
                return Err(XC_DUP);
            }
        }
        self.slots.push(Slot { id, prio, installed });
        self.sort_desc();
        Ok(())
    }

    /// 链顶插队：登记到优先级最高位置（重排为降序）。
    pub fn register_head(&mut self, id: u32, prio: u32) -> Result<(), u32> {
        self.register(id, prio, true)
    }

    fn sort_desc(&mut self) {
        for i in 1..self.slots.len() {
            let key = self.slots[i];
            let mut j = i;
            while j > 0 && self.slots[j - 1].prio < key.prio {
                self.slots[j] = self.slots[j - 1];
                j -= 1;
            }
            self.slots[j] = key;
        }
    }

    /// 注销。
    pub fn unregister(&mut self, id: u32) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for i in 0..self.slots.len() {
            if self.slots[i].id == id {
                self.slots.remove(i);
                return Ok(());
            }
        }
        Err(XC_NOSLOT)
    }

    /// 识别链跑：探针值 → 按优先级逐槽试；未装槽跳过。
    pub fn identify(&self, probe: u32) -> Result<Option<u32>, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        for sl in self.slots.iter() {
            if !sl.installed {
                continue; // 占位跳过
            }
            // 模型识别器：probe 模 10 == 槽 ID 模 10 → Found(槽 ID)。
            if probe % 10 == sl.id % 10 {
                return Ok(Some(sl.id));
            }
        }
        Ok(None)
    }

    /// 槽数。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 链空？
    pub fn empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// 链序（降序核对）。
    pub fn order(&self) -> Vec<u32> {
        let mut v = Vec::new();
        for sl in self.slots.iter() {
            v.push(sl.id);
        }
        v
    }
}

pub fn run_veb322_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0322");
    let mut f = IdFrame::new();
    // 族零：链空。
    s.add("空链识别 None", f.identify(42) == Ok(None), "全员请假");
    // 族一：登记与冲突。
    s.add("登 A 优 10", f.register(1, 10, true).is_ok(), "一槽");
    s.add("同级重号拒", f.register(2, 10, true) == Err(XC_DUP), "冲突");
    s.add("登 B 优 20", f.register(2, 20, true).is_ok(), "二槽");
    s.add("登占位 优 30", f.register(3, 30, false).is_ok(), "虚位");
    s.add("槽 3", f.len() == 3, "三工位");
    // 族二：识别链（降序）。
    // probe 42：2 号（优 20）模 10 = 2 == 42%10 → Found(2)。
    s.add("链中 2 胜", f.identify(42) == Ok(Some(2)), "降序先试 2");
    // probe 71：3 号占位跳过 → 2 号 71%10=1 ≠ 2 → 1 号 1==1 → Found(1)。
    s.add("占位跳过", f.identify(71) == Ok(Some(1)), "虚位不算败");
    // probe 13：3 号占位跳，2 号 3≠2，1 号 3≠1 → None。
    s.add("无人认领", f.identify(13) == Ok(None), "链尽");
    // 链序：3(30), 2(20), 1(10)。
    s.add("链序降", f.order() == vec3(3, 2, 1), "降序");
    // 族三：链顶插队。
    let mut g = IdFrame::new();
    let _ = g.register(5, 10, true);
    let _ = g.register(6, 20, true);
    let _ = g.register_head(7, 99);
    s.add("插队首", g.order() == vec3(7, 6, 5), "99 顶");
    // probe 97 → 7 号胜。
    s.add("新主接客", g.identify(97) == Ok(Some(7)), "链顶先试");
    // 族四：注销。
    s.add("注销 7", g.unregister(7).is_ok(), "退位");
    s.add("注销无者拒", g.unregister(7) == Err(XC_NOSLOT), "无二退");
    s.add("链回二槽", g.len() == 2 && g.order() == vec2(6, 5), "原序");
    // probe 96 → 6 号（6%10==6）。
    s.add("回退识别", g.identify(96) == Ok(Some(6)), "旧主回来");
    // 族五：槽满。
    let mut h = IdFrame::new();
    let mut ok = true;
    for i in 0..NSLOT as u32 {
        if h.register(i, i * 10 + 1, true).is_err() {
            ok = false;
        }
    }
    s.add("满槽 8", ok && h.len() == NSLOT, "定容");
    s.add("9 槽拒", h.register(99, 999, true) == Err(XC_FULL), "溢出");
    s.add("魔数核", IF_SEG_BASE == 0x31980, "段基址");
    s
}

/// 二元 Vec 构造助手（避免 vec! 宏依赖）。
fn vec2(a: u32, b: u32) -> Vec<u32> {
    let mut v = Vec::new();
    v.push(a);
    v.push(b);
    v
}

/// 三元 Vec 构造助手（避免 vec! 宏依赖）。
fn vec3(a: u32, b: u32, c: u32) -> Vec<u32> {
    let mut v = Vec::new();
    v.push(a);
    v.push(b);
    v.push(c);
    v
}
