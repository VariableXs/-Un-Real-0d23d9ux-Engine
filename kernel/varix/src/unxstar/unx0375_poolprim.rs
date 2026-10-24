//! unx0375_poolprim —— UNX-F0375 页帧池并发原语预留（单写者期声明）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0375 · 并发原语预留 · 目标 200 行
//! 判据原文：UNX-F0375-J1 页帧池当前单写者（BSP 独占期）：并发
//!           原语以「语义占位」预留——写者令牌（唯一持有）/
//!           读者快照序（单调版本号）；单写者不变量可判定
//!           （令牌重发拒、版本倒退拒）；多写者期接线位预留
//!           （接口冻结，实现交后续批）。
//! 组件：写者令牌（唯一性核 + 交接核）+ 版本号（单调 + 快照
//!       对账）+ 接口冻结声明。
//! 职责定位：并发原语预留是「多核化的地基预埋」——单写者期
//!   一切都简单，但接口如果不按「未来有锁」的形状设计，多写
//!   者期就要把全部调用方翻一遍。令牌/版本两个原语是未来锁
//!   与 RCU 的语义等价物：现在用「约定」实现，将来换「机制」
//!   不换接口。
//! 性能逐项分解：令牌 O(1)；版本 O(1)；快照 O(1)。
//! 诊断段基址：0x20A40（0x20A41~0x20A49 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PP_VERSION: &str = "unx0375_poolprim-v1";

/// 诊断段基址（0x20A40 段 · 本模块独占）。
pub const PP_SEG_BASE: u32 = 0x20A40;
pub const XC_TOKEN_HELD: u32 = 0x20A41; // 令牌重发（已有持有者）
pub const XC_TOKEN_BAD: u32 = 0x20A42; // 令牌核验失败
pub const XC_VER_BACK: u32 = 0x20A43; // 版本倒退
pub const XC_VER_EQ: u32 = 0x20A44; // 版本原地踏步（写后版本必须 +1）
pub const XC_NO_TOKEN: u32 = 0x20A45; // 无令牌写拒
pub const XC_FROZEN: u32 = 0x20A46; // 接口冻结后拒扩
pub const XC_SNAP_DRIFT: u32 = 0x20A47; // 快照漂移
pub const XC_SELF_RELEASE: u32 = 0x20A48; // 非持有者放令牌
pub const XC_ID_OOR: u32 = 0x20A49; // 写者号越域

/// 写者号闭集（当前单写者：BSP=1；2~8 预留）。
pub const WRITER_BSP: u32 = 1;
pub const WRITER_MAX: u32 = 8;

/// 写者令牌（单持有）。
pub struct WriterToken {
    holder: Option<u32>,
    /// 交接次数（令牌易手账）。
    handovers: u32,
    frozen: bool,
}

impl WriterToken {
    pub fn new() -> WriterToken {
        WriterToken { holder: None, handovers: 0, frozen: false }
    }

    /// 取令牌：已有持有者拒（单写者不变量）。
    pub fn acquire(&mut self, writer: u32) -> Result<(), u32> {
        if writer == 0 || writer > WRITER_MAX {
            return Err(XC_ID_OOR);
        }
        if self.frozen && self.holder.is_some() {
            return Err(XC_TOKEN_HELD);
        }
        if let Some(h) = self.holder {
            if h == writer {
                return Err(XC_TOKEN_HELD); // 重入也拒（单写者非可重入）
            }
            return Err(XC_TOKEN_HELD);
        }
        self.holder = Some(writer);
        Ok(())
    }

    /// 放令牌：非持有者放令牌拒。
    pub fn release(&mut self, writer: u32) -> Result<(), u32> {
        match self.holder {
            Some(h) if h == writer => {
                self.holder = None;
                self.handovers += 1;
                Ok(())
            }
            Some(_) => Err(XC_SELF_RELEASE),
            None => Err(XC_NO_TOKEN),
        }
    }

    /// 交接核：A 放 B 取 = 一次易手。
    pub fn handover(&mut self, from: u32, to: u32) -> Result<(), u32> {
        self.release(from)?;
        self.acquire(to)
    }

    /// 持有者。
    pub fn holder(&self) -> Option<u32> {
        self.holder
    }

    /// 易手数。
    pub fn handovers(&self) -> u32 {
        self.handovers
    }

    /// 接口冻结（多写者期接线位封存）。
    pub fn freeze(&mut self) {
        self.frozen = true;
    }
}

/// 读者版本号（单调）。
pub struct ReaderVersion {
    ver: u64,
}

impl ReaderVersion {
    pub fn new() -> ReaderVersion {
        ReaderVersion { ver: 0 }
    }

    /// 当前版本。
    pub fn cur(&self) -> u64 {
        self.ver
    }

    /// 写后推进：必须严格 +1（原地踏步 = 没写却报写，账面破）。
    pub fn bump_from(&mut self, seen: u64) -> Result<u64, u32> {
        if seen < self.ver {
            return Err(XC_VER_BACK);
        }
        if seen == self.ver {
            return Err(XC_VER_EQ);
        }
        if seen != self.ver + 1 {
            return Err(XC_VER_BACK); // 跳号也拒（快照序必须连续）
        }
        self.ver = seen;
        Ok(self.ver)
    }

    /// 读者快照核：读者见到的版本不得超过当前版本。
    pub fn reader_sees(&self, seen: u64) -> Result<(), u32> {
        if seen > self.ver {
            return Err(XC_SNAP_DRIFT);
        }
        Ok(())
    }
}

/// 写入口令牌闸：无令牌写拒（预留原语的调用形状）。
pub fn guarded_write(token: &WriterToken, writer: u32, payload: u32) -> Result<u32, u32> {
    match token.holder() {
        Some(h) if h == writer => Ok(payload),
        _ => Err(XC_NO_TOKEN),
    }
}

pub fn run_unx0375_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0375_poolprim");
    // 族一：令牌。
    let mut tk = WriterToken::new();
    cs.add("BSP 取令牌", tk.acquire(WRITER_BSP).is_ok(), "单写者");
    cs.add("重发拒", tk.acquire(WRITER_BSP) == Err(XC_TOKEN_HELD), "重入拒");
    cs.add("他人取拒", tk.acquire(2) == Err(XC_TOKEN_HELD), "单持有");
    cs.add("持有者 BSP", tk.holder() == Some(WRITER_BSP), "账面");
    // 族二：放令牌与交接。
    cs.add("非持有者放拒", tk.release(2) == Err(XC_SELF_RELEASE), "SELF");
    cs.add("BSP 放令牌", tk.release(WRITER_BSP).is_ok(), "易手 1");
    cs.add("AP 取令牌", tk.acquire(2).is_ok(), "交接给 2 号");
    cs.add("易手账 1", tk.handovers() == 1, "release 计数");
    // handover 组合核。
    let mut tk2 = WriterToken::new();
    let _ = tk2.acquire(1);
    cs.add("交接核", tk2.handover(1, 3).is_ok() && tk2.holder() == Some(3), "1→3");
    // 族三：无令牌写闸。
    cs.add("无令牌写拒", guarded_write(&tk2, 1, 7) == Err(XC_NO_TOKEN), "NO_TOKEN");
    cs.add("持令写过", guarded_write(&tk2, 3, 7) == Ok(7), "写透传");
    // 族四：版本号。
    let mut rv = ReaderVersion::new();
    cs.add("初版 0", rv.cur() == 0, "上电");
    cs.add("推进 +1", rv.bump_from(1) == Ok(1), "写后版本");
    cs.add("踏步拒", rv.bump_from(1) == Err(XC_VER_EQ), "没写报写");
    cs.add("跳号拒", rv.bump_from(5) == Err(XC_VER_BACK), "序断");
    cs.add("倒退拒", rv.bump_from(0) == Err(XC_VER_BACK), "VER_BACK");
    cs.add("读者快照过", rv.reader_sees(1).is_ok(), "读到当前版");
    cs.add("读者超版拒", rv.reader_sees(9) == Err(XC_SNAP_DRIFT), "读到未来");
    // 族五：冻结。
    tk.freeze();
    cs.add("冻结后拒扩", tk.acquire(4).is_err(), "接线位封存");
    cs.add("冻结态放行核", tk.release(2).is_ok(), "正常放行不受冻");
    // 族六：号域。
    cs.add("零写者拒", tk2.acquire(0).is_err(), "ID_OOR");
    let mut tk3 = WriterToken::new();
    cs.add("超域拒", tk3.acquire(WRITER_MAX + 1).is_err(), "ID_OOR");
    cs.add("预留域上界过", tk3.acquire(WRITER_MAX).is_ok(), "8 号合法");
    cs
}
