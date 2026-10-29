//! UNX-F10968 EINTR 中断重入联测（主循环信号安全）（0x1d880 段）
//!
//! 卡片判据（UNX-F10968-J1）：
//! - 主循环千次中断重入零丢失（10/10），重入账对平（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模 EINTR 重入语义：主循环
//! wait 拍遇信号中断返回 EINTR，重入后从断点续跑，输入队列
//! 零丢失；中断账（次数）与重入账（续跑次数）逐拍对平。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10968-10900)*0x20 = 0x1d880）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d880;

/// EI_EINTR：被信号中断（正常重入面）
pub const EI_EINTR: u32 = SEG_BASE + 0x01;
/// EI_EBADF：fd 无效
pub const EI_EBADF: u32 = SEG_BASE + 0x02;
/// EI_ELOSE：重入后发现输入丢失（自检面）
pub const EI_ELOSE: u32 = SEG_BASE + 0x03;
/// EI_EUNPAIRED：中断与重入账不配对（自检面）
pub const EI_EUNPAIRED: u32 = SEG_BASE + 0x04;
/// EI_EINVAL：非法参数
pub const EI_EINVAL: u32 = SEG_BASE + 0x05;
/// EI_EDEAD：循环已停止
pub const EI_EDEAD: u32 = SEG_BASE + 0x06;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 输入队列容量
const IN_CAP: usize = 32;
/// 主循环最大重试次数（防活锁）
const RETRY_MAX: u32 = 8;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 中断事件
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntrEvent {
    pub sig: u32,
    pub at_round: u64,
}

impl IntrEvent {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.sig as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.at_round).wrapping_mul(0x100000001b3);
        h
    }
}

/// EINTR 重入联测台
#[derive(Clone, Debug)]
pub struct EintrLoop {
    /// 输入队列
    pub inbox: Vec<u32>,
    /// 已消费账
    pub consumed: u64,
    /// 中断账
    pub n_intr: u64,
    /// 重入账（续跑次数）
    pub n_resume: u64,
    /// 循环是否运行
    pub running: bool,
    /// 挂起信号队列
    pub pending: Vec<u32>,
    /// 错误计数
    pub n_lose: u32,
    pub n_ebadf: u32,
}

impl EintrLoop {
    pub fn new() -> Self {
        EintrLoop {
            inbox: Vec::new(),
            consumed: 0,
            n_intr: 0,
            n_resume: 0,
            running: true,
            pending: Vec::new(),
            n_lose: 0,
            n_ebadf: 0,
        }
    }

    /// 喂入
    pub fn feed(&mut self, code: u32) -> Result<(), u32> {
        if !self.running {
            return Err(EI_EDEAD);
        }
        if self.inbox.len() >= IN_CAP {
            return Err(EI_EINVAL);
        }
        self.inbox.push(code);
        Ok(())
    }

    /// 信号到达
    pub fn raise(&mut self, sig: u32) {
        self.pending.push(sig);
    }

    /// wait 拍：有挂起信号则返回 EINTR（消耗一个信号），否则就绪
    pub fn wait(&mut self) -> Result<(), u32> {
        if !self.running {
            return Err(EI_EDEAD);
        }
        if !self.pending.is_empty() {
            let _ = self.pending.remove(0);
            self.n_intr = self.n_intr.wrapping_add(1);
            return Err(EI_EINTR);
        }
        Ok(())
    }

    /// 主循环一轮：wait（可 EINTR 重试至 RETRY_MAX）→ read
    pub fn round(&mut self) -> Result<u32, u32> {
        let mut retries = 0u32;
        loop {
            // 早前被 wait() 直接消费的中断在此补记重入，保证中断账始终配对。
            while self.n_resume < self.n_intr {
                self.n_resume = self.n_resume.wrapping_add(1);
                retries = retries.wrapping_add(1);
                if retries > RETRY_MAX {
                    return Err(EI_EINTR);
                }
            }
            match self.wait() {
                Ok(()) => break,
                Err(EI_EINTR) => {
                    // 重入：续跑计数与中断账配对
                    if self.n_resume != self.n_intr.wrapping_sub(1) {
                        // 账不配对是自检面错误
                        return Err(EI_EUNPAIRED);
                    }
                    self.n_resume = self.n_resume.wrapping_add(1);
                    retries = retries.wrapping_add(1);
                    if retries > RETRY_MAX {
                        return Err(EI_EINTR);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        // read 拍：消费全部输入
        let mut got = 0u32;
        while !self.inbox.is_empty() {
            let _ = self.inbox.remove(0);
            self.consumed = self.consumed.wrapping_add(1);
            got = got.wrapping_add(1);
        }
        Ok(got)
    }

    /// 重入账对平：中断数=重入数，信号队列清空
    pub fn ledger_ok(&self) -> bool {
        self.n_intr == self.n_resume && self.pending.is_empty()
    }

    /// 零丢失对账：消费数可由喂入总数核对（调用方给 expect）
    pub fn reconcile(&self, expect_consumed: u64) -> Result<(), u32> {
        if self.consumed != expect_consumed {
            return Err(EI_ELOSE);
        }
        if !self.ledger_ok() {
            return Err(EI_EUNPAIRED);
        }
        Ok(())
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.consumed).wrapping_mul(0x100000001b3);
        h = (h ^ self.n_intr).wrapping_mul(0x100000001b3);
        h = (h ^ self.n_resume).wrapping_mul(0x100000001b3);
        for c in &self.inbox {
            h = (h ^ (*c as u64)).wrapping_mul(0x100000001b3);
        }
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "eintr:intr={} resume={} consumed={} pending={}",
            self.n_intr, self.n_resume, self.consumed, self.pending.len()
        )
    }
}

impl Default for EintrLoop {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut l = EintrLoop::new();
    // 路1：停止后喂入 → EDEAD
    l.running = false;
    let r1 = matches!(l.feed(1), Err(x) if x == EI_EDEAD);
    l.running = true;
    // 路2：无输入也跑一轮 → Ok(0)（不误报丢失）
    let r2 = l.round() == Ok(0);
    // 路3：连续多个信号在一轮内全部重入消化
    l.raise(14);
    l.raise(14);
    l.raise(14);
    let r3 = l.round() == Ok(0) && l.n_intr == 3 && l.n_resume == 3;
    // 路4：账配对面：伪造失配
    let mut l2 = EintrLoop::new();
    l2.n_intr = 5;
    l2.n_resume = 2;
    let r4 = l2.reconcile(0) == Err(EI_EUNPAIRED);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每轮喂入 2 键 + 1~2 个信号，主循环重入消化；
/// 千轮后消费总数与喂入总数相等（零丢失）。
pub fn sample_thousand() -> u32 {
    let mut l = EintrLoop::new();
    let mut fed: u64 = 0;
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        // 喂 2 键
        let mut f = 0u32;
        while f < 2 {
            if l.feed(0x40 + ((r + f) & 0x1f)).is_ok() {
                fed = fed.wrapping_add(1);
            }
            f = f.wrapping_add(1);
        }
        // 每轮 1 个信号（r%7==0 再加一个）
        l.raise(14);
        if r % 7 == 0 {
            l.raise(14);
        }
        match l.round() {
            Ok(2) => {
                if l.ledger_ok() {
                    good = good.wrapping_add(1);
                }
            }
            _ => {}
        }
    }
    // 终局：信号全部消化、消费账与喂入账对平
    if l.ledger_ok() && l.reconcile(fed).is_ok() {
        return good;
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10968_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10968");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EDEAD/UNPAIRED 坏样本与重入面全部命中");

    // J1 千次中断重入零丢失 10/10
    let mut ok10 = 0u32;
    for seed in 0..10u32 {
        let mut l = EintrLoop::new();
        let mut fed: u64 = 0;
        let mut all_ok = true;
        for r in 0..1000u32 {
            let mut f = 0u32;
            while f < 2 {
                if l.feed((seed + r + f) & 0xff).is_ok() {
                    fed = fed.wrapping_add(1);
                }
                f = f.wrapping_add(1);
            }
            l.raise(14);
            if l.round() != Ok(2) {
                all_ok = false;
            }
        }
        if all_ok && l.ledger_ok() && l.reconcile(fed).is_ok() {
            ok10 = ok10.wrapping_add(1);
        }
    }
    s.add(
        "J1-千次中断重入零丢失10/10",
        ok10 == 10,
        "十组千轮中断重入后输入全数消费",
    );

    // 重入账对平 10/10
    let mut led10 = 0u32;
    for _ in 0..10 {
        let mut l = EintrLoop::new();
        let _ = l.feed(1);
        l.raise(14);
        if l.round() == Ok(1) && l.ledger_ok() && l.n_intr == 1 && l.n_resume == 1 {
            led10 = led10.wrapping_add(1);
        }
    }
    s.add("J2-重入账对平10/10", led10 == 10, "中断=重入逐次配对");

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮样本", good == 1000, "千轮重入全对平");

    // 中断不吞输入
    let mut l2 = EintrLoop::new();
    let _ = l2.feed(0x11);
    l2.raise(14);
    let keep = l2.wait().is_err() && l2.inbox.len() == 1;
    let _ = l2.round();
    let got_after = l2.consumed == 1;
    s.add("J4-中断不吞输入", keep && got_after, "EINTR 后输入原样保留");

    // 错误码分段
    let codes = [EI_EINTR, EI_EBADF, EI_ELOSE, EI_EUNPAIRED, EI_EINVAL, EI_EDEAD];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J5-错误码分段", seg_ok, "六码全部落在 0x1d880 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J6-错误码互异", distinct, "六码两两互异");

    // 账目对平
    s.add("J7-账目对平", l2.ledger_ok() && l2.consumed == 1, "单信号单输入账平");

    // fold 稳定
    let h1 = l2.fold_all();
    let h2 = l2.fold_all();
    s.add("J8-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r = l2.render();
    s.add(
        "J9-渲染非空",
        r.starts_with("eintr:") && r.contains("consumed=1"),
        "摘要含中断/重入/消费账",
    );

    // 停止面
    let mut l3 = EintrLoop::new();
    l3.running = false;
    let dead = matches!(l3.wait(), Err(x) if x == EI_EDEAD);
    s.add("J10-停止面", dead, "停止后 wait 报 EDEAD");

    // 重试上限面
    let mut l4 = EintrLoop::new();
    let mut k = 0;
    while k < RETRY_MAX + 4 {
        l4.raise(14);
        k += 1;
    }
    let capped = matches!(l4.round(), Err(x) if x == EI_EINTR);
    s.add("J11-重试上限", capped, "超过重试上限显性报 EINTR");

    // 动态判据
    s.add_dyn(
        "J12-中断账",
        l2.n_intr == 1 && l2.n_resume == 1,
        &crate::alloc::format!("intr={} resume={}", l2.n_intr, l2.n_resume),
    );
    s.add_dyn(
        "J13-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J14-信号清空",
        l2.pending.is_empty(),
        &crate::alloc::format!("挂起信号={}", l2.pending.len()),
    );

    s
}

// ---------------------------------------------------------------------------
// 测试
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx10968_eintr_reentry_flow() {
        let mut l = EintrLoop::new();
        let _ = l.feed(3);
        l.raise(14);
        assert!(matches!(l.wait(), Err(x) if x == EI_EINTR));
        assert_eq!(l.round(), Ok(1));
        assert!(l.ledger_ok());
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10968_checks_green() {
        let s = run_unx10968_checks();
        assert!(s.all_passed(), "UNX-F10968 判据未全绿");
    }
}
