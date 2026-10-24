//! unx0168_ebscb — UNX-F0168 ExitBootServices 前回调链。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0168-J1）：注册回调按序执行且异常
//! 隔离（单回调失败不阻断链），执行账完整。
//!
//! 职责定位：EBS 前回调链是内核收尾的最后机会（关中断、flush 缓存、
//! 拷贝持久数据）——单回调 panic 或卡死不得阻断全链，否则后续回调全丢。
//! 本模块：①注册按序（LIFO 是 UEFI CreateEventEx 语义，登记序留痕）；
//! ②隔离执行（单回调 Err 被捕获入账，链继续走——隔离账是判据核心）；
//! ③执行账完整（每个回调的序号/结果/耗时步全记录，重放可查）。
//! no_std + alloc；零 unwrap 零 expect；全逻辑步数无墙钟。
//! 诊断码段 0x2950（本会话独占，grep 全库零占用后领取，2026-10-22）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const CB_VERSION: &str = "B168-ebscallback-v1";

// ── 诊断码（0x295 段，本会话独占）──
/// 回调执行失败（被隔离——链继续）。
pub const XC_CB_ISOLATED: u16 = 0x2950;
/// 回调重复注册（同 ID 二次挂链）。
pub const XC_CB_DUP_ID: u16 = 0x2951;
/// 链已执行后再注册（EBS 已过——迟到注册拒）。
pub const XC_CB_LATE_REGISTER: u16 = 0x2952;
/// 执行账不平（回调数 ≡ 账内条目数）。
pub const XC_CB_LEDGER_UNBALANCED: u16 = 0x2953;
/// 步数超预算（回调卡死防线——预算化检测）。
pub const XC_CB_OVER_BUDGET: u16 = 0x2954;

/// 单回调步数预算（防卡死——超顶即隔离）。
pub const CB_STEP_BUDGET: u64 = 1_024;

/// 回调函数抽象（内核侧统一走步数化的执行体——返回消耗步数，
/// Err = 回调自报失败；两类都被隔离记账）。
pub trait EbsCallback {
    fn id(&self) -> u32;
    /// 执行：返回消耗步数；Err(code)=回调自报失败。
    fn run(&mut self) -> Result<u64, u16>;
}

/// 一条执行账。
#[derive(Clone, Debug)]
pub struct CbRecord {
    /// 注册序（LIFO 执行序的登记面）。
    pub reg_seq: u32,
    /// 回调 ID。
    pub id: u32,
    /// 结果：Ok(步数) / Err(码)。
    pub outcome: Result<u64, u16>,
}

/// EBS 前回调链。
#[derive(Debug)]
pub struct EbsCallbackChain {
    /// 注册链（登记序）。
    chain: Vec<(u32, usize)>, // (id, 内部索引)
    /// 回调体（由宿主持有——本账只存 id 与执行记录的桥）。
    records: Vec<CbRecord>,
    /// 链是否已执行（EBS 已过）。
    executed: bool,
    /// 隔离计数。
    pub isolated: u64,
    /// 注册计数。
    pub registered: u64,
}

impl EbsCallbackChain {
    pub fn boot() -> Self {
        EbsCallbackChain { chain: Vec::new(), records: Vec::new(), executed: false, isolated: 0, registered: 0 }
    }

    /// 注册（EBS 前有效；LIFO 语义由执行序实现）。
    pub fn register(&mut self, id: u32) -> Result<u32, u16> {
        if self.executed {
            return Err(XC_CB_LATE_REGISTER);
        }
        if self.chain.iter().any(|(i, _)| *i == id) {
            return Err(XC_CB_DUP_ID);
        }
        self.chain.push((id, 0));
        self.registered += 1;
        Ok(id)
    }

    /// 执行全链（LIFO 按注册逆序），单回调失败隔离不阻断。
    /// 回调体经 trait 对象传入（宿主侧实现执行语义）。
    pub fn run_all(&mut self, bodies: &mut [&mut dyn EbsCallback]) -> Result<(), u16> {
        if self.executed {
            return Err(XC_CB_LATE_REGISTER);
        }
        self.executed = true;
        // LIFO：注册逆序执行。
        let order: Vec<u32> = self.chain.iter().rev().map(|(i, _)| *i).collect();
        for id in order {
            let body = bodies.iter_mut().find(|b| b.id() == id);
            let outcome = match body {
                None => Err(XC_CB_LEDGER_UNBALANCED), // 注册了但体缺失——隔离。
                Some(b) => {
                    let r = b.run();
                    if r.is_err() {
                        r
                    } else {
                        let steps = r.unwrap_or(0);
                        if steps > CB_STEP_BUDGET {
                            Err(XC_CB_OVER_BUDGET)
                        } else {
                            Ok(steps)
                        }
                    }
                }
            };
            if outcome.is_err() {
                self.isolated += 1;
            }
            self.records.push(CbRecord { reg_seq: id, id, outcome });
        }
        Ok(())
    }

    /// 执行账完整（判据三）：每个注册回调都有账、账数 ≡ 注册数。
    pub fn ledger_complete(&self) -> bool {
        self.records.len() == self.registered as usize
            && self.chain.iter().all(|(id, _)| self.records.iter().any(|r| r.id == *id))
    }

    /// 隔离不阻断验证：链执行到底（记录数 = 注册数）且失败被标记。
    pub fn isolation_held(&self) -> bool {
        self.records.len() == self.registered as usize
            && self.records.iter().filter(|r| r.outcome.is_err()).count() as u64 == self.isolated
    }

    /// 执行账读数。
    pub fn records(&self) -> Vec<CbRecord> {
        self.records.clone()
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_CB_ISOLATED => "回调被隔离：失败已入账，链继续——查执行账定位失败回调。".to_string(),
            XC_CB_DUP_ID => "回调 ID 重复：同 ID 只许挂一次链。".to_string(),
            XC_CB_LATE_REGISTER => "EBS 已过：回调注册与执行都只许一次。".to_string(),
            XC_CB_LEDGER_UNBALANCED => "执行账不平：注册数 ≡ 账目数，先对账。".to_string(),
            XC_CB_OVER_BUDGET => "回调超步数预算：按卡死隔离，先修回调内部循环。".to_string(),
            _ => "未知诊断码：查 0x295 段分配表。".to_string(),
        }
    }
}

// ── 测试体（自证夹具）──
struct OkCb(u32, u64);
impl EbsCallback for OkCb {
    fn id(&self) -> u32 {
        self.0
    }
    fn run(&mut self) -> Result<u64, u16> {
        Ok(self.1)
    }
}
struct FailCb(u32, u16);
impl EbsCallback for FailCb {
    fn id(&self) -> u32 {
        self.0
    }
    fn run(&mut self) -> Result<u64, u16> {
        Err(self.1)
    }
}

/// 自证：按序 + 隔离 + 账完整三判据对拍。
pub fn self_audit() -> Result<(), u16> {
    // 一、LIFO 按序执行 + 单失败不阻断。
    let mut chain = EbsCallbackChain::boot();
    chain.register(1)?;
    chain.register(2)?;
    chain.register(3)?;
    if chain.register(3).is_ok() {
        return Err(XC_CB_DUP_ID);
    }
    let mut a = OkCb(1, 10);
    let mut b = FailCb(2, XC_CB_ISOLATED);
    let mut c = OkCb(3, 30);
    chain.run_all(&mut [&mut a, &mut b, &mut c])?;
    // 执行序：3 → 2 → 1（LIFO）。
    let recs = chain.records();
    if recs.len() != 3 || recs[0].id != 3 || recs[1].id != 2 || recs[2].id != 1 {
        return Err(XC_CB_LEDGER_UNBALANCED);
    }
    // 失败的 2 被隔离，1 和 3 正常入账。
    if recs[0].outcome != Ok(30) || recs[1].outcome != Err(XC_CB_ISOLATED) || recs[2].outcome != Ok(10) {
        return Err(XC_CB_ISOLATED);
    }
    if !chain.ledger_complete() || !chain.isolation_held() || chain.isolated != 1 {
        return Err(XC_CB_LEDGER_UNBALANCED);
    }
    // 二、二次执行拒 + 迟到注册拒。
    if chain.run_all(&mut [&mut a]).is_ok() {
        return Err(XC_CB_LATE_REGISTER);
    }
    if chain.register(4).is_ok() {
        return Err(XC_CB_LATE_REGISTER);
    }
    Ok(())
}
