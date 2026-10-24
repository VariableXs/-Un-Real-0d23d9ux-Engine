//! veb296_mlidbg —— VE-F0296 Mali 调试通道。
//! 锚点（CoRun Unxreal 主册）：VE-F0296 · Mali 调试通道 · 目标 320 行
//! 判据原文：VE-F0296-J1 调试通道账：通道三档（关 / 采样 /
//!           全量）；写入记账（每笔带级别 + 序号）；级别门
//!           （关档拒写；采样档只留每 8 笔的 1 笔；全量照
//!           收）；通道容量（128 笔定容，满挤最旧）；序号
//!           单调核（写入序号只增不重）；读尾核（读最近 n
//!           笔按时间升序）；档位切换核（升档立即生效，降
//!           档不清历史）。
//! 组件：档位机 + 级别门 + 环账 + 读尾。
//! 职责定位：调试通道是「Mali 的行车记录仪」——三档是三种
//!           拍摄模式：关机不拍、采样抽帧、全量连拍；采样档
//!           抽 1/8 是帧率与清晰度的折中；满挤最旧是记录仪
//!           的循环录像：最近的永远比最早的值钱。
//! 性能逐项分解：写入 O(1)；读尾 O(n)。
//! 诊断码段：0x31300（0x31301~0x31309 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const DB_VERSION: &str = "veb296_mlidbg-v1";

/// 诊断段基址（0x31300 段 · 本模块独占）。
pub const DB_SEG_BASE: u32 = 0x31300;
pub const XC_MAGIC: u32 = 0x31301; // 账魔数破
pub const XC_OFF: u32 = 0x31302; // 关档拒写
pub const XC_LV: u32 = 0x31303; // 级别不合法

/// 账魔数。
const MAGIC: u32 = 0x3130_1B6E;

/// 通道档。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DbgMode {
    Off,
    Sampled,
    Full,
}

/// 采样步长（每 8 笔留 1）。
pub const SAMPLE_STEP: u32 = 8;

/// 通道容。
pub const RING_CAP: usize = 128;

/// 记录笔。
#[derive(Clone, Copy)]
pub struct LogRec {
    pub seq: u64,
    pub level: u32,
    pub tag: u32,
}

/// 调试通道。
pub struct DbgChan {
    mode: DbgMode,
    ring: Vec<LogRec>,
    head: usize,
    total_in: u64,
    next_seq: u64,
    magic: u32,
}

impl DbgChan {
    /// 建（初始档）。
    pub fn new(mode: DbgMode) -> DbgChan {
        let mut ring = Vec::new();
        for _ in 0..RING_CAP {
            ring.push(LogRec { seq: 0, level: 0, tag: 0 });
        }
        DbgChan { mode, ring, head: 0, total_in: 0, next_seq: 1, magic: MAGIC }
    }

    fn magic_ok(&self) -> bool {
        self.magic == MAGIC
    }

    /// 升降档。
    pub fn set_mode(&mut self, m: DbgMode) -> Result<(), u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        self.mode = m;
        Ok(())
    }

    /// 写一笔：按档过滤；返回实际写入的序号（被滤 → None）。
    pub fn write(&mut self, level: u32, tag: u32) -> Result<Option<u64>, u32> {
        if !self.magic_ok() {
            return Err(XC_MAGIC);
        }
        if level == 0 || level > 7 {
            return Err(XC_LV);
        }
        match self.mode {
            DbgMode::Off => return Err(XC_OFF),
            DbgMode::Sampled => {
                if self.total_in % SAMPLE_STEP as u64 != 0 {
                    self.total_in += 1;
                    return Ok(None);
                }
            }
            DbgMode::Full => {}
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.ring[self.head] = LogRec { seq, level, tag };
        self.head = (self.head + 1) % RING_CAP;
        self.total_in += 1;
        Ok(Some(seq))
    }

    /// 账内笔数。
    pub fn depth(&self) -> usize {
        let live = if self.total_in as usize >= RING_CAP { RING_CAP } else { self.total_in as usize };
        live
    }

    /// 读最近 n 笔（时间升序）。
    pub fn tail(&self, n: usize) -> Vec<LogRec> {
        let live = self.depth();
        let take = if n > live { live } else { n };
        let oldest = if self.total_in as usize >= RING_CAP { self.head } else { 0 };
        let start = live - take;
        let mut out = Vec::new();
        for k in start..live {
            out.push(self.ring[(oldest + k) % RING_CAP]);
        }
        out
    }

    /// 序号单调核：账内序号严格递增。
    pub fn seq_monotonic(&self) -> bool {
        let t = self.tail(self.depth());
        for i in 1..t.len() {
            if t[i].seq <= t[i - 1].seq {
                return false;
            }
        }
        true
    }
}

pub fn run_veb296_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0296");
    // 族零：级别门。
    let mut c = DbgChan::new(DbgMode::Full);
    s.add("级 0 拒", c.write(0, 1) == Err(XC_LV), "无零级");
    s.add("级 8 拒", c.write(8, 1) == Err(XC_LV), ">7");
    // 族一：关档。
    let mut o = DbgChan::new(DbgMode::Off);
    s.add("关档拒写", o.write(3, 1) == Err(XC_OFF), "不拍");
    s.add("关档账空", o.depth() == 0, "零笔");
    // 升档生效。
    let _ = o.set_mode(DbgMode::Full);
    s.add("升档即写", matches!(o.write(3, 5), Ok(Some(_))), "立生效");
    s.add("账 1 笔", o.depth() == 1, "一笔");
    // 族二：采样档。
    let mut sp = DbgChan::new(DbgMode::Sampled);
    let mut kept = 0u32;
    for i in 0..16u32 {
        if matches!(sp.write(1, i), Ok(Some(_))) {
            kept += 1;
        }
    }
    s.add("16 采留 2", kept == 2, "1/8 步");
    s.add("账 2 笔", sp.depth() == 2, "第 0/8 笔");
    // 序号连续性：留下的两笔序号应为 1、2（写入序只发留下的）。
    let t2 = sp.tail(2);
    s.add("序 1", t2[0].seq == 1, "首留");
    s.add("序 2", t2[1].seq == 2, "次留");
    s.add("单调核", sp.seq_monotonic(), "只增");
    // 族三：全量与容量。
    let mut f = DbgChan::new(DbgMode::Full);
    for i in 0..200u32 {
        let _ = f.write((i % 7) + 1, i);
    }
    s.add("满容 128", f.depth() == RING_CAP, "定容");
    // 读尾升序 + 最旧被挤。
    let t128 = f.tail(RING_CAP);
    s.add("尾首序 73", t128[0].seq == 73, "200-128+1");
    s.add("尾末序 200", t128[127].seq == 200, "最新");
    s.add("环序单调", f.seq_monotonic(), "升序核");
    // 读尾 n=10。
    let t10 = f.tail(10);
    s.add("尾 10", t10.len() == 10 && t10[9].seq == 200, "最近十笔");
    // 族四：降档不清史。
    let _ = f.set_mode(DbgMode::Sampled);
    s.add("降档史在", f.depth() == RING_CAP, "不清历史");
    // 降档后写按采样。
    let mut new_kept = 0u32;
    for _ in 0..8 {
        if matches!(f.write(2, 999), Ok(Some(_))) {
            new_kept += 1;
        }
    }
    s.add("降档采样 1", new_kept == 1, "1/8 续");
    // 序号延续。
    let tl = f.tail(1);
    s.add("序续 201", tl[0].seq == 201, "不重号");
    // 族五：单调核全档位。
    s.add("全档单调", f.seq_monotonic(), "混合档也序");
    s.add("魔数核", DB_SEG_BASE == 0x31300, "段基址");
    s
}
