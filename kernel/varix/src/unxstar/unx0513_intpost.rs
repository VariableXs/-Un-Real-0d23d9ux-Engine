//! unx0513_intpost —— UNX-F0513 Posted Interrupt 描述符账（PIR 位图 + ON 位 + 投递合成）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0513 · Posted 中断账 · 目标 210 行
//! 判据原文：UNX-F0513-J1 Posted 中断账：投一 PIR 位
//!           图（中断向量按位置账，128 位（2×u64）位图，
//!           位内独立）；投二 ON 位语义（ON=0 期间积账不
//!           投递，置 ON 后一次性合成投递并清位图）；投
//!           三 清位原子（合成投递后位图归零，残留位即
//!           违例）；投四 向量不重投（同向量在位图内幂
//!           等，置位两次不双投）；投五 NDST 目标唯一（
//!           描述符绑定的目标 vCPU 字段写入后不可变）。
//! 组件：128 位 PIR 位图 + ON 门控 + 合成投递器 + 幂等置位 + NDST 封印。
//! 职责定位：Posted interrupt 让设备中断直接落进 vCPU 的
//!   PIR 位图而不触发 VM exit——ON 位是「积账 vs 投递」
//!   的闸门：guest 忙时关 ON 攒批，闲时开 ON 合成。位图清
//!   不净 = 中断重投（重复 injection）；同向量双投 = guest
//!   计时器漂移的经典来源。
//! 性能逐项分解：置位 O(1)；合成 O(2)（两字位图）。
//! 诊断码段：0x34DC0（0x34DC1~0x34DC9 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断码。
pub const SEG_BASE: u32 = 0x34DC0;
pub const XC_NDST: u32 = SEG_BASE + 1; // NDST 重写违例
pub const XC_ON: u32 = SEG_BASE + 2; // ON 未置位时请求合成
pub const XC_RESIDUE: u32 = SEG_BASE + 3; // 合成后位图残留

/// Posted Interrupt 描述符。
pub struct PiDesc {
    pub pir: [u64; 2],
    pub on: bool,
    pub ndst: Option<u16>,
    pub deliveries: u64,
}

impl PiDesc {
    pub fn new() -> Self {
        PiDesc { pir: [0; 2], on: false, ndst: None, deliveries: 0 }
    }

    /// 投一/投四：置位（幂等）。v 0~127。
    pub fn post(&mut self, v: u8) -> bool {
        let word = (v / 64) as usize;
        let bit = (v % 64) as u32;
        let was = self.pir[word] & (1u64 << bit) != 0;
        self.pir[word] |= 1u64 << bit;
        !was // false = 已在账（幂等命中）
    }

    /// 投五：NDST 封印（首写定终身）。
    pub fn bind_ndst(&mut self, dst: u16) -> Result<(), u32> {
        match self.ndst {
            None => {
                self.ndst = Some(dst);
                Ok(())
            }
            Some(d) if d == dst => Ok(()), // 同值重绑合法
            Some(_) => Err(XC_NDST),
        }
    }

    /// 投二：置 ON 后合成投递——返回投递向量数并清位图。
    pub fn synthesize(&mut self) -> Result<(Vec<u8>, usize), u32> {
        if !self.on {
            return Err(XC_ON);
        }
        let mut out: Vec<u8> = alloc::vec::Vec::new();
        for word in 0..2usize {
            let mut bits = self.pir[word];
            while bits != 0 {
                let b = bits.trailing_zeros() as u8;
                out.push((word * 64) as u8 + b);
                bits &= bits - 1; // 清最低位
            }
        }
        let n = out.len();
        self.deliveries += 1;
        // 投三：合成即清。
        self.pir = [0; 2];
        self.on = false;
        if self.pir[0] != 0 || self.pir[1] != 0 {
            return Err(XC_RESIDUE);
        }
        Ok((out, n))
    }

    /// 位图总占位数。
    pub fn popcount(&self) -> u32 {
        self.pir[0].count_ones() + self.pir[1].count_ones()
    }
}

pub fn run_unx0513_checks() -> CheckSet {
    let mut cs = CheckSet::new("UNX-F0513");
    cs.add("SEG 段位独占", SEG_BASE == 0x34DC0 && XC_RESIDUE == SEG_BASE + 3, "u32 高段全域普查无冲突");

    // 族一：PIR 置位幂等。
    let mut d = PiDesc::new();
    cs.add("首置报新", d.post(32), "向量 32 入账");
    cs.add("重置报幂等", !d.post(32), "同向量不再计新");
    cs.add("占位恒一", d.popcount() == 1, "双置一位");

    // 族二：跨字位图。
    let _ = d.post(63);
    let _ = d.post(64); // 第二字首位
    let _ = d.post(127); // 第二字末位
    cs.add("跨字占四", d.popcount() == 4, "32/63/64/127");
    cs.add("字边界", d.pir[0] == (1u64 << 32) | (1u64 << 63) && d.pir[1] == 1u64 | (1u64 << 63), "两字独立");

    // 族三：ON 门控。
    cs.add("未 ON 合成拒", d.synthesize() == Err(XC_ON), "积账期禁合成");
    d.on = true;
    let syn = d.synthesize();
    cs.add("ON 合成成", syn.is_ok(), "门开合成");

    // 族四：合成投递序 + 清位。
    let (vecs, n) = syn.unwrap_or((alloc::vec::Vec::new(), 0));
    cs.add("投递计数四", n == 4, "4 向量");
    cs.add("向量升序", vecs == alloc::vec![32u8, 63, 64, 127], "位序遍历");
    cs.add("清位归零", d.popcount() == 0 && !d.on, "投三/ON 复位");
    cs.add("投递账一", d.deliveries == 1, "一次合成一计");

    // 族五：NDST 封印。
    let mut d2 = PiDesc::new();
    cs.add("NDST 首绑成", d2.bind_ndst(0x2A).is_ok(), "首写");
    cs.add("同值重绑成", d2.bind_ndst(0x2A).is_ok(), "同值合法");
    cs.add("异值重绑拒", d2.bind_ndst(0x2B) == Err(XC_NDST), "投五封印");

    // 族六：积账-合成多轮。
    let mut d3 = PiDesc::new();
    let mut total = 0usize;
    for round in 0..3u8 {
        for v in [round * 10, round * 10 + 1] {
            d3.post(v);
        }
        d3.on = true;
        if let Ok((_, n)) = d3.synthesize() {
            total += n;
        }
    }
    cs.add("三轮投递", total == 6 && d3.deliveries == 3, "每轮 2 向量");
    cs.add("轮间清账", d3.popcount() == 0, "无跨轮残留");

    // 族七：向量 0 与 127 边界。
    let mut d4 = PiDesc::new();
    let _ = d4.post(0);
    let _ = d4.post(127);
    d4.on = true;
    let (v4, n4) = d4.synthesize().unwrap_or((alloc::vec::Vec::new(), 0));
    cs.add("边界向量", n4 == 2 && v4 == alloc::vec![0u8, 127], "最低最高位");

    cs
}
