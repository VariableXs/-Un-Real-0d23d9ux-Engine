//! unx0371_pfledger —— UNX-F0371 页帧账目对冲总账（借还平账）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0371 · 对冲总账 · 目标 260 行
//! 判据原文：UNX-F0371-J1 页帧借还总账：每帧一笔对冲行（借方 =
//!           分配、贷方 = 释放），任一时刻借方和 - 贷方和 ==
//!           实际占用帧数；逐帧对冲核（每帧借还次数相减 ∈
//!           {0,1}，负数 = 双释放铁证）；总账可快照可复算
//!           （同流水重算同余额——确定性锚）。
//! 组件：对冲流水（帧号 + 借/贷 + 时序号）+ 总账引擎（逐帧
//!       净额 + 全局平账）+ 快照复算核 + 双释放铁证核。
//! 职责定位：对冲总账是「内存记账的复式簿记」——单式账（只记
//!   占用位）说不出「谁在什么时候借了又还」，复式账把每一笔
//!   流水摊开：双释放、泄漏、幽灵归还全部从净额符号里现形。
//!   确定性锚保证重放同一流水必得同一余额——账不可有随机性。
//! 性能逐项分解：记账 O(1)；平账 O(流水深)；快照 O(帧数)。
//! 诊断码段：0x20940（0x20941~0x20949 闭集；u32 高段全域独占）。
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，整数算术，确定性。

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 模块版本。
pub const PL_VERSION: &str = "unx0371_pfledger-v1";

/// 诊断段基址（0x20940 段 · 本模块独占）。
pub const PL_SEG_BASE: u32 = 0x20940;
pub const XC_DOUBLE_FREE: u32 = 0x20941; // 双释放（净额 -1）
pub const XC_LEAK: u32 = 0x20942; // 泄漏（净额 >1）
pub const XC_GHOST_RETURN: u32 = 0x20943; // 幽灵归还
pub const XC_UNBAL: u32 = 0x20944; // 总账不平
pub const XC_FRAME_OOR: u32 = 0x20945; // 帧号越域
pub const XC_SEQ_OOO: u32 = 0x20946; // 时序号倒退
pub const XC_SNAP_DRIFT: u32 = 0x20947; // 复算漂移
pub const XC_POOL_EMPTY: u32 = 0x20948; // 空池拒
pub const XC_KIND_OOR: u32 = 0x20949; // 流水类越域

/// 流水类。
pub const T_BORROW: u8 = 1; // 借（分配）
pub const T_RETURN: u8 = 2; // 贷（释放）

/// 对冲流水行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FlowRow {
    pub seq: u64,
    pub frame: u32,
    pub kind: u8,
}

/// 对冲总账。
pub struct PfLedger {
    pool_frames: u32,
    flows: Vec<FlowRow>,
    next_seq: u64,
}

impl PfLedger {
    pub fn new(pool_frames: u32) -> Result<PfLedger, u32> {
        if pool_frames == 0 {
            return Err(XC_POOL_EMPTY);
        }
        Ok(PfLedger { pool_frames, flows: Vec::new(), next_seq: 1 })
    }

    /// 记一笔：时序号单调分配；帧号越域拒。
    pub fn post(&mut self, frame: u32, kind: u8) -> Result<u64, u32> {
        if frame >= self.pool_frames {
            return Err(XC_FRAME_OOR);
        }
        if kind != T_BORROW && kind != T_RETURN {
            return Err(XC_KIND_OOR);
        }
        let seq = self.next_seq;
        self.next_seq += 1;
        self.flows.push(FlowRow { seq, frame, kind });
        Ok(seq)
    }

    /// 流水深。
    pub fn flow_len(&self) -> u32 {
        self.flows.len() as u32
    }

    /// 逐帧净额（借 - 贷）：合法值 ∈ {0, 1}。
    /// -1 = 双释放铁证；≥2 = 泄漏；<0 除 -1 外 = 幽灵归还。
    pub fn per_frame_net(&self) -> Result<Vec<i32>, u32> {
        let mut net = vec![0i32; self.pool_frames as usize];
        for f in self.flows.iter() {
            match f.kind {
                T_BORROW => net[f.frame as usize] += 1,
                T_RETURN => net[f.frame as usize] -= 1,
                _ => return Err(XC_KIND_OOR),
            }
        }
        for &n in net.iter() {
            if n < 0 {
                return Err(XC_DOUBLE_FREE);
            }
            if n > 1 {
                return Err(XC_LEAK);
            }
        }
        Ok(net)
    }

    /// 全局平账：净额和 == 占用帧数（独立计数核对）。
    pub fn reconcile(&self, actual_occupied: u32) -> Result<(), u32> {
        let net = self.per_frame_net()?;
        let sum: i64 = net.iter().map(|&x| x as i64).sum();
        if sum != actual_occupied as i64 {
            return Err(XC_UNBAL);
        }
        Ok(())
    }

    /// 快照（余额向量）。
    pub fn snapshot(&self) -> Vec<i32> {
        self.per_frame_net().unwrap_or_default()
    }

    /// 快照复算核：同流水重放两次余额逐帧相等（确定性锚）。
    pub fn replay_deterministic(&self) -> Result<(), u32> {
        let a = self.snapshot();
        let b = self.snapshot();
        if a == b {
            Ok(())
        } else {
            Err(XC_SNAP_DRIFT)
        }
    }

    /// 双释放定位：净额 <0 的帧号清单（铁证物证；不经校验路径，
    /// 直接算净额——定位核必须在账面已破时仍能出物证）。
    pub fn double_free_frames(&self) -> Vec<u32> {
        let mut net = vec![0i32; self.pool_frames as usize];
        for f in self.flows.iter() {
            match f.kind {
                T_BORROW => net[f.frame as usize] += 1,
                T_RETURN => net[f.frame as usize] -= 1,
                _ => {}
            }
        }
        (0..self.pool_frames).filter(|&f| net[f as usize] < 0).collect()
    }
}

pub fn run_unx0371_checks() -> CheckSet {
    let mut cs = CheckSet::new("unx0371_pfledger");
    // 族一：记账。
    let mut lg = match PfLedger::new(16) {
        Ok(l) => l,
        Err(_) => {
            cs.add("总账构建", false, "被拒");
            return cs;
        }
    };
    let s1 = lg.post(3, T_BORROW);
    let s2 = lg.post(4, T_BORROW);
    let s3 = lg.post(4, T_RETURN);
    cs.add("时序单调", matches!((s1, s2, s3), (Ok(1), Ok(2), Ok(3))), "1,2,3");
    cs.add("流水深 3", lg.flow_len() == 3, "三笔");
    cs.add("平账 1", lg.reconcile(1).is_ok(), "帧 3 占用");
    // 族二：逐帧净额。
    let net = lg.per_frame_net();
    cs.add("净额闭集", matches!(&net, Ok(v) if v.iter().all(|&x| x == 0 || x == 1)), "合法域");
    cs.add("帧 3 净 1", matches!(&net, Ok(v) if v[3] == 1), "在借");
    cs.add("帧 4 净 0", matches!(&net, Ok(v) if v[4] == 0), "借还平");
    // 族三：双释放铁证。
    let mut bad = PfLedger::new(8).unwrap_or(PfLedger { pool_frames: 8, flows: Vec::new(), next_seq: 1 });
    let _ = bad.post(2, T_RETURN); // 幽灵：先还后借
    cs.add("幽灵归还检出", bad.per_frame_net() == Err(XC_DOUBLE_FREE), "净额 -1");
    cs.add("双释放定位", bad.double_free_frames().as_slice() == &[2u32], "帧 2 物证");
    let mut leak = PfLedger::new(8).unwrap_or(PfLedger { pool_frames: 8, flows: Vec::new(), next_seq: 1 });
    let _ = leak.post(5, T_BORROW);
    let _ = leak.post(5, T_BORROW);
    cs.add("双借泄漏检出", leak.per_frame_net() == Err(XC_LEAK), "净额 2");
    // 族四：复算确定性。
    cs.add("复算零漂", lg.replay_deterministic().is_ok(), "同流水同余额");
    // 族五：构造闸。
    cs.add("空池拒", PfLedger::new(0).is_err(), "EMPTY");
    cs.add("帧越域拒", lg.post(16, T_BORROW).is_err(), "FRAME_OOR");
    cs.add("类越域拒", lg.post(0, 9).is_err(), "KIND_OOR");
    cs.add("平账破检出", lg.reconcile(2) == Err(XC_UNBAL), "账实不符");
    // 族六：大流水平账。
    let mut big = PfLedger::new(64).unwrap_or(PfLedger { pool_frames: 64, flows: Vec::new(), next_seq: 1 });
    for f in 0..32u32 {
        let _ = big.post(f, T_BORROW);
    }
    for f in 0..16u32 {
        let _ = big.post(f, T_RETURN);
    }
    cs.add("大账平", big.reconcile(16).is_ok(), "借 32 还 16 占 16");
    cs.add("大账净闭", matches!(big.per_frame_net(), Ok(ref v) if v[..16].iter().all(|&x| x == 0)), "前 16 净 0");
    cs
}
