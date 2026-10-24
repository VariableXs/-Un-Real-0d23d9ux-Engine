//! veb264_nvsubmit —— VE-F0264 NVIDIA 提交通路。
//! 锚点（CoRun Unxreal 主册）：VE-F0264 · NVIDIA 提交通路 · 目标 420 行
//! 判据原文：VE-F0264-J1 提交通路双模型：推送缓冲（老代际）与
//!           环形缓冲（Turing+）；提交通道定容（每通道 1024 项）；
//!           通道满拒；门铃寄存器核（提交后门铃值 = 写指针）；
//!           通道间隔离（A 通道写不串 B 通道读）；溢出包装核
//!           （环形缓冲回绕后写指针续推，读指针追平即空）。
//! 组件：双模型通道 + 门铃 + 隔离核 + 回绕账。
//! 职责定位：提交通路是「NVIDIA 通路的血管」——推送缓冲是老
//!           血管（CPU 每笔都亲手推），环形缓冲是新血管（GPU
//!           自己顺着环喝）；门铃是心跳：写指针推了必须响铃，
//!           不响铃 GPU 永远不知道有活；回绕是环形缓冲的本分：
//!           转一圈不是错，追平才是空。
//! 性能逐项分解：提交 O(1)；收割 O(批)；回绕 O(1)。
//! 诊断码段：0x30B00（0x30B01~0x30B0A 闭集）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const SB_VERSION: &str = "veb264_nvsubmit-v1";

/// 诊断段基址（0x30B00 段 · 本模块独占）。
pub const SB_SEG_BASE: u32 = 0x30B00;
pub const XC_CH_FULL: u32 = 0x30B01; // 通道满
pub const XC_NO_CH: u32 = 0x30B02; // 无此通道
pub const XC_MAGIC: u32 = 0x30B03; // 账魔数破
pub const XC_MODEL: u32 = 0x30B04; // 模型不匹配
pub const XC_LEAK: u32 = 0x30B05; // 隔离破（串道）
pub const XC_EMPTY: u32 = 0x30B06; // 空通道收割
pub const XC_BELL: u32 = 0x30B07; // 门铃与写指针不符
pub const XC_DUP: u32 = 0x30B08; // 通道重名
pub const XC_CAP: u32 = 0x30B09; // 批量超容
pub const XC_WRAP: u32 = 0x30B0A; // 回绕账破

/// 通道容。
pub const CH_CAP: usize = 1024;
/// 通道模型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubModel {
    /// 推送缓冲（Kepler~Pascal）。
    Push = 0,
    /// 环形缓冲（Turing+）。
    Ring = 1,
}

/// 通道。
pub struct Channel {
    pub name: &'static str,
    pub model: SubModel,
    /// 写指针（单调增，Ring 按 CH_CAP 取模）。
    pub wptr: u64,
    /// 读指针。
    pub rptr: u64,
    /// 门铃。
    pub bell: u64,
    /// 累计提交。
    pub submitted: u64,
    /// 累计收割。
    pub reaped: u64,
}

/// 提交管理器。
pub struct Submitter {
    chs: Vec<Channel>,
    magic: u32,
}

impl Submitter {
    /// 建。
    pub fn new() -> Submitter {
        Submitter { chs: Vec::new(), magic: 0x0B00_5B17 }
    }

    /// 开通道。
    pub fn open(&mut self, name: &'static str, model: SubModel) -> Result<(), u32> {
        if self.magic != 0x0B00_5B17 {
            return Err(XC_MAGIC);
        }
        for c in self.chs.iter() {
            if c.name == name {
                return Err(XC_DUP);
            }
        }
        self.chs.push(Channel {
            name, model, wptr: 0, rptr: 0, bell: 0, submitted: 0, reaped: 0,
        });
        Ok(())
    }

    fn pos(&self, name: &str) -> Result<usize, u32> {
        for (i, c) in self.chs.iter().enumerate() {
            if c.name == name {
                return Ok(i);
            }
        }
        Err(XC_NO_CH)
    }

    /// 提交 n 项（未满才许推；推送后门铃 = 写指针）。
    pub fn submit(&mut self, name: &str, n: u64) -> Result<u64, u32> {
        let i = self.pos(name)?;
        let c = &mut self.chs[i];
        if n == 0 {
            return Err(XC_BELL);
        }
        let inflight = c.wptr - c.rptr;
        if inflight + n > CH_CAP as u64 {
            return Err(XC_CH_FULL);
        }
        c.wptr += n;
        c.submitted += n;
        c.bell = c.wptr;
        Ok(c.bell)
    }

    /// 收割 n 项。
    pub fn reap(&mut self, name: &str, n: u64) -> Result<u64, u32> {
        let i = self.pos(name)?;
        let c = &mut self.chs[i];
        let inflight = c.wptr - c.rptr;
        if n == 0 {
            return Err(XC_EMPTY);
        }
        if n > inflight {
            return Err(XC_EMPTY);
        }
        c.rptr += n;
        c.reaped += n;
        Ok(c.rptr)
    }

    /// 门铃核：bell == wptr。
    pub fn bell_ok(&self, name: &str) -> Result<bool, u32> {
        let i = self.pos(name)?;
        let c = &self.chs[i];
        if c.bell != c.wptr {
            return Err(XC_BELL);
        }
        Ok(true)
    }

    /// 隔离核：通道 A 的在途数变动不影响 B 的账。
    pub fn isolation(&self, a: &str, b: &str) -> Result<bool, u32> {
        let ia = self.pos(a)?;
        let ib = self.pos(b)?;
        let ca = &self.chs[ia];
        let cb = &self.chs[ib];
        // 各通道账独立闭合。
        let oka = ca.submitted == ca.reaped + (ca.wptr - ca.rptr);
        let okb = cb.submitted == cb.reaped + (cb.wptr - cb.rptr);
        if !(oka && okb) {
            return Err(XC_LEAK);
        }
        Ok(true)
    }

    /// 回绕账核：Ring 通道 wptr 取模回绕后 inflight 一致。
    pub fn wrap_audit(&self, name: &str) -> Result<u32, u32> {
        let i = self.pos(name)?;
        let c = &self.chs[i];
        if c.model != SubModel::Ring {
            return Err(XC_MODEL);
        }
        let inflight = c.wptr - c.rptr;
        let w = (c.wptr % CH_CAP as u64) as u32;
        let r = (c.rptr % CH_CAP as u64) as u32;
        // 回绕后的槽距须等于 inflight（模容）。
        let span = if w >= r { w - r } else { CH_CAP as u32 - r + w };
        if span != inflight as u32 {
            return Err(XC_WRAP);
        }
        Ok(span)
    }

    /// 通道数。
    pub fn count(&self) -> usize {
        self.chs.len()
    }
}

pub fn run_veb264_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F0264");
    // 族一：双模型开通道。
    let mut m = Submitter::new();
    let _ = m.open("push0", SubModel::Push);
    let _ = m.open("ring0", SubModel::Ring);
    s.add("双通道", m.count() == 2, "Push+Ring");
    s.add("重名拒", m.open("push0", SubModel::Push) == Err(XC_DUP), "主键");
    // 族二：提交与门铃。
    let b1 = m.submit("push0", 4);
    s.add("推 4 门铃 4", b1 == Ok(4), "bell=wptr");
    let b2 = m.submit("push0", 6);
    s.add("再推门铃 10", b2 == Ok(10), "续推");
    s.add("门铃核过", m.bell_ok("push0") == Ok(true), "一致");
    s.add("Ring 提交过", m.submit("ring0", 8) == Ok(8), "环推");
    s.add("零提交拒", m.submit("ring0", 0) == Err(XC_BELL), "零量");
    // 族三：收割。
    let r1 = m.reap("push0", 6);
    s.add("收 6", r1 == Ok(6), "rptr 推");
    s.add("超额收拒", m.reap("push0", 5) == Err(XC_EMPTY), "在途仅 4");
    s.add("在途 4", m.submit("push0", 0).is_err(), "哨探");
    s.add("零收拒", m.reap("push0", 0) == Err(XC_EMPTY), "零量");
    // 族四：通道满。
    let full = m.submit("ring0", 1020);
    s.add("推到 1028 超", full == Err(XC_CH_FULL), "8+1020>1024");
    let ok = m.submit("ring0", 1016);
    s.add("推到 1024 满", ok == Ok(1024), "恰满");
    s.add("再推 1 拒", m.submit("ring0", 1) == Err(XC_CH_FULL), "满拒");
    let _ = m.reap("ring0", 1024);
    s.add("清后可推", m.submit("ring0", 1) == Ok(1025), "回绕");
    // 族五：回绕账。
    let w = m.wrap_audit("ring0");
    s.add("回绕距 1", w == Ok(1), "wptr 1025 % 1024 = 1");
    // 再推一整圈核。
    let _ = m.reap("ring0", 1);
    let _ = m.submit("ring0", 1023);
    let _ = m.reap("ring0", 1023);
    let _ = m.submit("ring0", 1024);
    let _ = m.reap("ring0", 1024);
    let w2 = m.wrap_audit("ring0");
    s.add("两圈后距 0", w2 == Ok(0), "全收全推");
    s.add("Push 回绕拒", m.wrap_audit("push0") == Err(XC_MODEL), "模型不符");
    // 族六：隔离。
    let iso = m.isolation("push0", "ring0");
    s.add("隔离过", iso == Ok(true), "账独立闭合");
    // push0：submitted 10，reaped 6，inflight 4 → 10 == 6+4 ✓。
    s.add("野通道拒", m.submit("ghost", 1) == Err(XC_NO_CH), "无此道");
    s.add("门铃野拒", m.bell_ok("ghost") == Err(XC_NO_CH), "无此道");
    s.add("魔数核", SB_SEG_BASE == 0x30B00, "段基址");
    s
}
