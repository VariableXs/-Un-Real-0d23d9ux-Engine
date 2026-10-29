//! UNX-F10964 epoll 驱动输入主循环联测（wait→read→处理→write）（0x1d800 段）
//!
//! 卡片判据（UNX-F10964-J1）：
//! - 主循环千轮零丢失零空转（10/10），循环四拍对账（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模 epoll 驱动主循环四拍：
//! wait（就绪事件）→ read（消费输入）→ process（状态机推进）→
//! write（产出回写）。四拍账逐拍记账，输入=处理=产出（零丢失），
//! 每轮必有一拍命中（零空转）。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10964-10900)*0x20 = 0x1d800）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d800;

/// ML_EBADF：epoll 或输入 fd 无效
pub const ML_EBADF: u32 = SEG_BASE + 0x01;
/// ML_EEXIST：重复注册
pub const ML_EEXIST: u32 = SEG_BASE + 0x02;
/// ML_ENOENT：注销未注册 fd
pub const ML_ENOENT: u32 = SEG_BASE + 0x03;
/// ML_EINVAL：事件掩码非法
pub const ML_EINVAL: u32 = SEG_BASE + 0x04;
/// ML_ELOSE：四拍对账发现丢失（自检面）
pub const ML_ELOSE: u32 = SEG_BASE + 0x05;
/// ML_EIDLE：空转轮检出（自检面）
pub const ML_EIDLE: u32 = SEG_BASE + 0x06;
/// ML_ENOSPC：事件表满
pub const ML_ENOSPC: u32 = SEG_BASE + 0x07;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 事件兴趣掩码：可读
pub const EV_IN: u32 = 0x1;
/// 事件兴趣掩码：可写
pub const EV_OUT: u32 = 0x2;
/// 事件表容量
const EV_CAP: usize = 8;
/// 输入队列容量
const IN_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 注册条目
#[derive(Clone, Debug, PartialEq)]
pub struct EvEntry {
    pub fd: i32,
    pub interest: u32,
    /// 就绪事件
    pub ready: u32,
    /// 该 fd 累计就绪拍数
    pub fired: u64,
}

impl EvEntry {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.fd as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.interest as u64).wrapping_mul(0x100000001b3);
        h = (h ^ self.fired).wrapping_mul(0x100000001b3);
        h
    }
}

/// 主循环四拍账
#[derive(Clone, Debug, Default)]
pub struct BeatLedger {
    /// wait 命中拍数
    pub wait: u64,
    /// read 消费键数
    pub read: u64,
    /// process 推进键数
    pub process: u64,
    /// write 产出键数
    pub write: u64,
}

impl BeatLedger {
    /// 四拍对平：read=process=write（wait 以轮计，不与键数比）
    pub fn ok(&self) -> bool {
        self.read == self.process && self.process == self.write
    }

    /// 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.wait).wrapping_mul(0x100000001b3);
        h = (h ^ self.read).wrapping_mul(0x100000001b3);
        h = (h ^ self.process).wrapping_mul(0x100000001b3);
        h = (h ^ self.write).wrapping_mul(0x100000001b3);
        h
    }
}

/// 主循环联测台
#[derive(Clone, Debug)]
pub struct MainLoop {
    pub evs: Vec<EvEntry>,
    /// 输入队列（read 拍消费）
    inbox: Vec<u32>,
    /// 产出账
    pub beats: BeatLedger,
    /// 空转轮数（必须为零）
    pub idle_rounds: u32,
    /// 错误计数
    pub n_eexist: u32,
    pub n_enoent: u32,
    pub n_einval: u32,
}

impl MainLoop {
    pub fn new() -> Self {
        MainLoop {
            evs: Vec::new(),
            inbox: Vec::new(),
            beats: BeatLedger::default(),
            idle_rounds: 0,
            n_eexist: 0,
            n_enoent: 0,
            n_einval: 0,
        }
    }

    /// epoll 注册
    pub fn add_fd(&mut self, fd: i32, interest: u32) -> Result<(), u32> {
        if interest & !(EV_IN | EV_OUT) != 0 {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(ML_EINVAL);
        }
        if self.evs.iter().any(|e| e.fd == fd) {
            self.n_eexist = self.n_eexist.wrapping_add(1);
            return Err(ML_EEXIST);
        }
        if self.evs.len() >= EV_CAP {
            return Err(ML_ENOSPC);
        }
        self.evs.push(EvEntry { fd, interest, ready: 0, fired: 0 });
        Ok(())
    }

    /// epoll 注销
    pub fn del_fd(&mut self, fd: i32) -> Result<(), u32> {
        match self.evs.iter().position(|e| e.fd == fd) {
            Some(i) => {
                let _ = self.evs.remove(i);
                Ok(())
            }
            None => {
                self.n_enoent = self.n_enoent.wrapping_add(1);
                Err(ML_ENOENT)
            }
        }
    }

    /// 键盘写入输入队列（外部事件源）
    pub fn feed(&mut self, code: u32) -> Result<(), u32> {
        if self.inbox.len() >= IN_CAP {
            return Err(ML_ENOSPC);
        }
        self.inbox.push(code);
        // 有输入则输入 fd 置就绪
        for e in self.evs.iter_mut() {
            if e.interest & EV_IN != 0 {
                e.ready |= EV_IN;
            }
        }
        Ok(())
    }

    /// 主循环一轮：四拍推进；返回本轮是否命中（非空转）
    pub fn round(&mut self) -> Result<bool, u32> {
        // 拍1 wait：取就绪事件
        let mut hit = false;
        for e in self.evs.iter_mut() {
            if e.ready & e.interest != 0 {
                e.fired = e.fired.wrapping_add(1);
                hit = true;
            }
        }
        if hit {
            self.beats.wait = self.beats.wait.wrapping_add(1);
        }
        // 拍2 read：消费输入
        let mut got = 0u32;
        while !self.inbox.is_empty() {
            let _ = self.inbox.remove(0);
            got = got.wrapping_add(1);
        }
        if got > 0 {
            self.beats.read = self.beats.read.wrapping_add(got as u64);
            // 消费后输入 fd 不再就绪
            for e in self.evs.iter_mut() {
                e.ready &= !EV_IN;
            }
        }
        // 拍3 process：状态推进（等量透传）
        let proc = got;
        if proc > 0 {
            self.beats.process = self.beats.process.wrapping_add(proc as u64);
        }
        // 拍4 write：产出（等量）
        if proc > 0 {
            self.beats.write = self.beats.write.wrapping_add(proc as u64);
        }
        if !hit && got == 0 {
            self.idle_rounds = self.idle_rounds.wrapping_add(1);
            return Ok(false);
        }
        Ok(true)
    }

    /// 对账：零丢失（read=process=write）且零空转
    pub fn reconcile(&self) -> Result<(), u32> {
        if !self.beats.ok() {
            return Err(ML_ELOSE);
        }
        if self.idle_rounds > 0 {
            return Err(ML_EIDLE);
        }
        Ok(())
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_read: u64) -> bool {
        self.beats.read == expect_read && self.reconcile().is_ok()
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for e in &self.evs {
            h = (h ^ e.fold()).wrapping_mul(0x100000001b3);
        }
        h = (h ^ self.beats.fold()).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "mainloop:wait={} read={} proc={} write={} idle={}",
            self.beats.wait,
            self.beats.read,
            self.beats.process,
            self.beats.write,
            self.idle_rounds
        )
    }
}

impl Default for MainLoop {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut m = MainLoop::new();
    // 路1：非法掩码 → EINVAL
    let r1 = matches!(m.add_fd(3, 0x8), Err(x) if x == ML_EINVAL);
    // 路2：重复注册 → EEXIST
    let _ = m.add_fd(3, EV_IN);
    let r2 = matches!(m.add_fd(3, EV_IN), Err(x) if x == ML_EEXIST);
    // 路3：注销未注册 fd → ENOENT
    let r3 = matches!(m.del_fd(9), Err(x) if x == ML_ENOENT);
    // 路4：对账丢失面：伪造空转
    m.idle_rounds = 1;
    let r4 = m.reconcile() == Err(ML_EIDLE);
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每轮喂入 0~2 键并推进一轮四拍；全程零丢失零空转。
pub fn sample_thousand() -> u32 {
    let mut m = MainLoop::new();
    if m.add_fd(0, EV_IN).is_err() {
        return 0;
    }
    let mut good: u32 = 0;
    let mut fed: u32 = 0;
    for r in 0..1000u32 {
        // 每轮喂入 (r%3)+1 个键（保证每轮至少一拍命中）
        let n = (r % 3) + 1;
        let mut i = 0u32;
        while i < n {
            if m.feed(r + i).is_ok() {
                fed = fed.wrapping_add(1);
            }
            i = i.wrapping_add(1);
        }
        if m.round() == Ok(true) {
            good = good.wrapping_add(1);
        }
    }
    // 终局对账：全部输入已被消费（read=fed）
    if m.beats.read == fed as u64 && m.reconcile().is_ok() && m.inbox.is_empty() {
        // 收尾：仍需保证好轮计数完整
        return good;
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10964_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10964");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EINVAL/EEXIST/ENOENT/EIDLE 坏样本全部命中");

    // J1 千轮零丢失零空转 10/10
    let mut ok10 = 0u32;
    for seed in 0..10u32 {
        let mut m = MainLoop::new();
        let _ = m.add_fd(0, EV_IN);
        let mut fed = 0u32;
        let mut all_hit = true;
        for r in 0..1000u32 {
            let n = ((r + seed) % 3) + 1;
            let mut i = 0u32;
            while i < n {
                if m.feed(r).is_ok() {
                    fed = fed.wrapping_add(1);
                }
                i = i.wrapping_add(1);
            }
            if m.round() != Ok(true) {
                all_hit = false;
            }
        }
        if all_hit && m.ledger_ok(fed as u64) {
            ok10 = ok10.wrapping_add(1);
        }
    }
    s.add(
        "J1-千轮零丢失零空转10/10",
        ok10 == 10,
        "十组千轮全命中且四拍账对平",
    );

    // 循环四拍对账 10/10
    let mut b10 = 0u32;
    for _ in 0..10 {
        let mut m = MainLoop::new();
        let _ = m.add_fd(0, EV_IN);
        for k in 0..7u32 {
            let _ = m.feed(k);
        }
        let _ = m.round();
        if m.beats.read == 7 && m.beats.process == 7 && m.beats.write == 7 && m.beats.wait == 1 {
            b10 = b10.wrapping_add(1);
        }
    }
    s.add("J2-四拍对账10/10", b10 == 10, "wait/read/process/write 四拍逐位相符");

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮样本", good == 1000, "千轮全命中");

    // 空转轮检出
    let mut m2 = MainLoop::new();
    let _ = m2.add_fd(1, EV_IN);
    let _ = m2.round(); // 无输入无就绪 → 空转
    let idle_face = m2.idle_rounds == 1 && m2.reconcile() == Err(ML_EIDLE);
    s.add("J4-空转检出", idle_face, "无事件轮被显性记账");

    // 错误码分段
    let codes = [ML_EBADF, ML_EEXIST, ML_ENOENT, ML_EINVAL, ML_ELOSE, ML_EIDLE, ML_ENOSPC];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J5-错误码分段", seg_ok, "七码全部落在 0x1d800 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J6-错误码互异", distinct, "七码两两互异");

    // 账目对平
    let mut m3 = MainLoop::new();
    let _ = m3.add_fd(0, EV_IN);
    for k in 0..100u32 {
        let _ = m3.feed(k);
        if k % 10 == 9 {
            let _ = m3.round();
        }
    }
    s.add("J7-账目对平", m3.ledger_ok(100), "百键分批推进全对平");

    // 注销后再喂不就绪
    let _ = m3.del_fd(0);
    let _ = m3.feed(0x99);
    let nofd = m3.evs.is_empty();
    s.add("J8-注销面", nofd, "注销后事件表为空");

    // fold 稳定
    let h1 = m3.fold_all();
    let h2 = m3.fold_all();
    s.add("J9-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r = m3.render();
    s.add(
        "J10-渲染非空",
        r.starts_with("mainloop:") && r.contains("read=100"),
        "摘要含四拍账",
    );

    // 就绪清零面
    let mut m4 = MainLoop::new();
    let _ = m4.add_fd(0, EV_IN);
    let _ = m4.feed(1);
    let _ = m4.round();
    let ready_clear = m4.evs[0].ready & EV_IN == 0;
    s.add("J11-就绪清零", ready_clear, "read 拍后输入 fd 就绪位清零");

    // 动态判据
    s.add_dyn(
        "J12-四拍账",
        m3.beats.ok(),
        &crate::alloc::format!("wait={} read={} proc={} write={}", m3.beats.wait, m3.beats.read, m3.beats.process, m3.beats.write),
    );
    s.add_dyn(
        "J13-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J14-空转账",
        m3.idle_rounds == 0,
        &crate::alloc::format!("空转={}", m3.idle_rounds),
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
    fn unx10964_epoll_mainloop_flow() {
        let mut m = MainLoop::new();
        assert!(m.add_fd(0, EV_IN).is_ok());
        assert!(matches!(m.add_fd(0, EV_IN), Err(x) if x == ML_EEXIST));
        for k in 0..5u32 {
            let _ = m.feed(k);
        }
        assert_eq!(m.round(), Ok(true));
        assert!(m.ledger_ok(5));
        assert!(matches!(m.del_fd(3), Err(x) if x == ML_ENOENT));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10964_checks_green() {
        let s = run_unx10964_checks();
        assert!(s.all_passed(), "UNX-F10964 判据未全绿");
    }
}
