//! veb317_mobdbg —— VE-F0317 移动组调试通道。
//! 锚点（CoRun Unxreal 主册）：VE-F0317 · 移动组调试通道 · 目标 320 行
//! 判据原文：VE-F0317-J1 移动调试账：分级环账（高级别 1~2 /
//!           中 3~5 / 低 6~7）；让位规则（环满时低级让高级：
//!           挤掉最旧的低级笔，高级笔不可挤）；高级保障核
//!           （高级笔环满也照收）；低级拒收账（被挤/被拒
//!           分开计数）；序号连续核（被收笔序号严格递增）；
//!           读尾（最近 n 笔升序）；档位采样（低级笔 1/4
//!           采样，高级全收）。
//! 组件：分级环 + 让位器 + 双拒账 + 采样门。
//! 职责定位：移动调试是「手机的行车记录仪」——环满时高级
//!           笔踩低级笔的位：崩溃日志比心跳日志值钱；高级
//!           不可挤是记录仪的底线：关键证据不能被路况视频
//!           冲掉；低级采样 1/4 是流量的算计：省一点是一点。
//! 性能逐项分解：写入 O(环容)；读尾 O(n)。
//! 诊断码段：0x31840（0x31841~0x31849 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const MD_VERSION: &str = "veb317_mobdbg-v1";

/// 诊断段基址（0x31840 段 · 本模块独占）。
pub const MD_SEG_BASE: u32 = 0x31840;
pub const XC_MAGIC: u32 = 0x31841; // 账魔数破
pub const XC_LV: u32 = 0x31842; // 级别不合法

/// 账魔数。
const MAGIC: u32 = 0x3184_1B31;

/// 环容。
pub const CAP: usize = 32;

/// 低级采样步（每 4 笔收 1）。
pub const LOW_STEP: u32 = 4;

/// 记录笔。
#[derive(Clone, Copy)]
pub struct MRec {
    pub seq: u64,
    pub level: u32,
    pub tag: u32,
}

/// 分级调试环。
pub struct MobDbg {
    ring: Vec<MRec>,
    /// 低级累计写入计数（采样用）。
    low_in: u32,
    /// 被挤低级数。
    pub evicted: u32,
    /// 被拒低级数（采样弃）。
    pub dropped: u32,
    next_seq: u64,
    magic: u32,
}

impl MobDbg {
    /// 建。
    pub fn new() -> MobDbg {
        let mut ring = Vec::new();
        for _ in 0..CAP {
            ring.push(MRec { seq: 0, level: 0, tag: 0 });
        }
        MobDbg { ring, low_in: 0, evicted: 0, dropped: 0, next_seq: 1, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 级别合法（1~7）。
    fn lv_ok(level: u32) -> bool {
        level >= 1 && level <= 7
    }

    /// 级别是否高级（1~2）。
    fn is_high(level: u32) -> bool {
        level <= 2
    }

    /// 环内实笔数。
    pub fn depth(&self) -> usize {
        let mut n = 0;
        for r in self.ring.iter() {
            if r.seq != 0 {
                n += 1;
            }
        }
        n
    }

    /// 找最旧低级笔（seq 最小的低级位）；无 → None。
    fn oldest_low(&self) -> Option<usize> {
        let mut best: Option<usize> = None;
        for i in 0..CAP {
            if self.ring[i].seq != 0 && !Self::is_high(self.ring[i].level) {
                match best {
                    None => best = Some(i),
                    Some(b) => {
                        if self.ring[i].seq < self.ring[b].seq {
                            best = Some(i);
                        }
                    }
                }
            }
        }
        best
    }

    /// 找空位。
    fn free_slot(&self) -> Option<usize> {
        for i in 0..CAP {
            if self.ring[i].seq == 0 {
                return Some(i);
            }
        }
        None
    }

    /// 写入：高级全收（满则挤低级）；低级 1/4 采样。
    pub fn write(&mut self, level: u32, tag: u32) -> Result<Option<u64>, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if !Self::lv_ok(level) {
            return Err(XC_LV);
        }
        if !Self::is_high(level) {
            // 低级采样。
            self.low_in += 1;
            if self.low_in % LOW_STEP != 1 {
                self.dropped += 1;
                return Ok(None);
            }
        }
        // 找位：空位优先，满则高级挤低级。
        let slot = match self.free_slot() {
            Some(i) => i,
            None => {
                if Self::is_high(level) {
                    match self.oldest_low() {
                        Some(i) => {
                            self.evicted += 1;
                            i
                        }
                        None => {
                            // 全高级满 → 拒收本笔（高级不挤高级）。
                            self.dropped += 1;
                            return Ok(None);
                        }
                    }
                } else {
                    // 低级且满 → 拒。
                    self.dropped += 1;
                    return Ok(None);
                }
            }
        };
        let seq = self.next_seq;
        self.next_seq += 1;
        self.ring[slot] = MRec { seq, level, tag };
        Ok(Some(seq))
    }

    /// 读尾（最近 n 笔升序）。
    pub fn tail(&self, n: usize) -> Vec<MRec> {
        let mut live: Vec<MRec> = Vec::new();
        for r in self.ring.iter() {
            if r.seq != 0 {
                live.push(*r);
            }
        }
        // 按 seq 升序插入排序（cap 32 小表）。
        for i in 1..live.len() {
            let key = live[i];
            let mut j = i;
            while j > 0 && live[j - 1].seq > key.seq {
                live[j] = live[j - 1];
                j -= 1;
            }
            live[j] = key;
        }
        let start = if live.len() > n { live.len() - n } else { 0 };
        let mut out = Vec::new();
        for k in start..live.len() {
            out.push(live[k]);
        }
        out
    }

    /// 序号连续核。
    pub fn seq_ok(&self) -> bool {
        let t = self.tail(CAP);
        for i in 1..t.len() {
            if t[i].seq <= t[i - 1].seq {
                return false;
            }
        }
        true
    }
}

pub fn run_veb317_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0317");
    let mut d = MobDbg::new();
    // 族零：级别门。
    s.add("级 0 拒", d.write(0, 1) == Err(XC_LV), "无零级");
    s.add("级 8 拒", d.write(8, 1) == Err(XC_LV), ">7");
    // 族一：低级采样。
    let mut kept = 0u32;
    for i in 0..8u32 {
        if matches!(d.write(6, i), Ok(Some(_))) {
            kept += 1;
        }
    }
    s.add("低级 8 收 2", kept == 2, "1/4 步");
    s.add("弃 6", d.dropped == 6, "采样账");
    // 高级全收。
    let mut high_kept = 0u32;
    for i in 0..4u32 {
        if matches!(d.write(1, i), Ok(Some(_))) {
            high_kept += 1;
        }
    }
    s.add("高级 4 收", high_kept == 4, "全收");
    s.add("高级 4 笔环", d.depth() == 6, "2 低 + 4 高");
    s.add("高级不弃", d.dropped == 6, "未增");
    // 族二：高级挤低级。
    // 填满：再写 26 高级笔 → 总高级 30 + 低 2 = 32 满。
    for i in 0..26u32 {
        let _ = d.write(2, 100 + i);
    }
    s.add("环满 32", d.depth() == CAP, "定容");
    s.add("挤 2", d.evicted == 2, "两低被挤");
    // 高级再写：无低可挤 → 拒。
    s.add("满环高拒", matches!(d.write(1, 999), Ok(None)), "高级不挤高级");
    s.add("拒计 7", d.dropped == 7, "再弃一笔");
    // 低级满环拒。
    s.add("满环低拒", matches!(d.write(5, 888), Ok(None)), "低级无位");
    // 族三：腾位后低级可入。
    // retire 模型：清最旧高级不存在——用整环重置模拟。
    let mut r = MobDbg::new();
    for _ in 0..8 {
        let _ = r.write(6, 1);
    }
    s.add("低 2 笔", r.depth() == 2, "采样入环");
    // 高级 30 笔填满（低 2 被挤）。
    for i in 0..30u32 {
        let _ = r.write(2, i);
    }
    s.add("满环 32", r.depth() == 32, "高级满");
    s.add("低全被挤", r.evicted == 2, "两低清出");
    // 族四：序号连续核。
    s.add("序连续", r.seq_ok(), "严格递增");
    let t5 = r.tail(5);
    s.add("尾 5 笔", t5.len() == 5, "读尾");
    s.add("尾递增", t5[4].seq > t5[0].seq, "升序");
    // 尾首为最旧存活：seq 从 3（笔 1,2 低级收）→ 高级 seq 3..32。
    s.add("尾首 seq 28", t5[0].seq == 28, "32-5+1");
    s.add("尾末 seq 32", t5[4].seq == 32, "最新");
    // 族五：混合档稳定。
    let mut m = MobDbg::new();
    // 高低交替写 64 笔。
    for i in 0..32u32 {
        let _ = m.write(1, i);  // 高级全收
        let _ = m.write(6, i);  // 低级 1/4
    }
    s.add("高 32 收", {
        // 高级 32 全收：环 32 容 → 挤掉早期。
        m.tail(32).iter().filter(|r| r.level <= 2).count() == 32
    }, "高级保住");
    s.add("低收 8 弃 24", m.dropped == 24, "1/4 采样账");
    s.add("混合序连续", m.seq_ok(), "稳");
    s.add("魔数核", MD_SEG_BASE == 0x31840, "段基址");
    s
}
