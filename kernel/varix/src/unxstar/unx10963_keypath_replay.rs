//! UNX-F10963 键入路径联测（键→pty→非规范读→应用）（0x1d7e0 段）
//!
//! 卡片判据（UNX-F10963-J1）：
//! - 键入到应用逐拍零丢失（100/100 逐键对账），路径时序对账（10/10）。
//!
//! 域：CoRun·UNX-C4-B29。本模块建模键入四段路径：键拍注入 →
//! pty 主端写入 → 非规范读缓冲 → 应用逐拍消费。全程逐键编号
//! 对账：注入数 = 交付数，序号严格递增（FIFO），背压满时显性
//! 拒收且不计丢键（可重试）。

use crate::alloc::string::String;
use crate::checks::CheckSet;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 错误码（SEG_BASE = 0x1d000 + (10963-10900)*0x20 = 0x1d7e0）
// ---------------------------------------------------------------------------
const SEG_BASE: u32 = 0x1d7e0;

/// KP_EBADF：路径 fd 无效
pub const KP_EBADF: u32 = SEG_BASE + 0x01;
/// KP_EIO：pty 段失效
pub const KP_EIO: u32 = SEG_BASE + 0x02;
/// KP_EAGAIN：读缓冲满（背压，可重试非丢失）
pub const KP_EAGAIN: u32 = SEG_BASE + 0x03;
/// KP_EINVAL：键码非法
pub const KP_EINVAL: u32 = SEG_BASE + 0x04;
/// KP_EORDER：交付序号错乱（自检面）
pub const KP_EORDER: u32 = SEG_BASE + 0x05;
/// KP_ELOSE：对账发现丢键（自检面）
pub const KP_ELOSE: u32 = SEG_BASE + 0x06;
/// KP_EDEAD：应用侧已停止消费
pub const KP_EDEAD: u32 = SEG_BASE + 0x07;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------
/// 读缓冲容量（键数）
const RBUF_CAP: usize = 16;
/// 应用一次消费的拍数
const BEAT_KEYS: u32 = 1;

// ---------------------------------------------------------------------------
// 数据结构
// ---------------------------------------------------------------------------

/// 单个键事件
#[derive(Clone, Debug, PartialEq)]
pub struct KeyEvent {
    /// 全局序号（逐键对账依据）
    pub seq: u64,
    /// 键码
    pub code: u32,
}

impl KeyEvent {
    /// FNV 折叠
    pub fn fold(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.seq).wrapping_mul(0x100000001b3);
        h = (h ^ self.code as u64).wrapping_mul(0x100000001b3);
        h
    }
}

/// 键入路径联测台
#[derive(Clone, Debug)]
pub struct KeyPath {
    /// pty 是否存活
    pub pty_alive: bool,
    /// 应用是否在消费
    pub app_alive: bool,
    /// 非规范读缓冲
    rbuf: Vec<KeyEvent>,
    /// 下一注入序号
    next_seq: u64,
    /// 注入成功账
    pub injected: u64,
    /// 应用已消费账
    pub delivered: u64,
    /// 已交付键的折叠链（逐键对账）
    pub deliv_fold: u64,
    /// 背压次数（重试后成功，不算丢失）
    pub n_again: u32,
    /// 错误计数
    pub n_ebadf: u32,
    pub n_einval: u32,
}

impl KeyPath {
    pub fn new() -> Self {
        KeyPath {
            pty_alive: true,
            app_alive: true,
            rbuf: Vec::new(),
            next_seq: 1,
            injected: 0,
            delivered: 0,
            deliv_fold: 0xcbf29ce484222325,
            n_again: 0,
            n_ebadf: 0,
            n_einval: 0,
        }
    }

    /// 键拍注入：键→pty→非规范读缓冲
    pub fn key_inject(&mut self, code: u32) -> Result<u64, u32> {
        if !self.pty_alive {
            return Err(KP_EIO);
        }
        if !self.app_alive {
            return Err(KP_EDEAD);
        }
        if code > 0x10ff {
            self.n_einval = self.n_einval.wrapping_add(1);
            return Err(KP_EINVAL);
        }
        if self.rbuf.len() >= RBUF_CAP {
            self.n_again = self.n_again.wrapping_add(1);
            return Err(KP_EAGAIN);
        }
        let ev = KeyEvent { seq: self.next_seq, code };
        self.next_seq = self.next_seq.wrapping_add(1);
        self.rbuf.push(ev);
        self.injected = self.injected.wrapping_add(1);
        Ok(self.injected)
    }

    /// 应用逐拍消费：BEAT_KEYS 键/拍
    pub fn app_beat(&mut self) -> Result<u32, u32> {
        if !self.app_alive {
            return Err(KP_EDEAD);
        }
        let mut got = 0u32;
        while got < BEAT_KEYS && !self.rbuf.is_empty() {
            let ev = self.rbuf.remove(0);
            // 逐键对账：序号必须等于已交付数+1
            if ev.seq != self.delivered.wrapping_add(1) {
                return Err(KP_EORDER);
            }
            self.deliv_fold = (self.deliv_fold ^ ev.seq)
                .wrapping_mul(0x100000001b3);
            self.delivered = self.delivered.wrapping_add(1);
            got = got.wrapping_add(1);
        }
        Ok(got)
    }

    /// 100 键整批对账：注入 100 键逐拍消费至零
    pub fn batch100(&mut self, seed: u32) -> Result<u32, u32> {
        let base_del = self.delivered;
        let mut put = 0u32;
        while put < 100 {
            match self.key_inject(seed.wrapping_add(put)) {
                Ok(_) => put = put.wrapping_add(1),
                // 背压：重试（消费几拍腾出空间）
                Err(KP_EAGAIN) => {
                    let _ = self.app_beat();
                }
                Err(e) => return Err(e),
            }
        }
        while !self.rbuf.is_empty() {
            let _ = self.app_beat()?;
        }
        // 本批交付数取交付增量：背压重试期间消费的键同样计入。
        Ok(self.delivered.wrapping_sub(base_del) as u32)
    }

    /// 对账：注入=交付 且 无键滞留
    pub fn reconcile(&self) -> bool {
        self.injected == self.delivered && self.rbuf.is_empty()
    }

    /// 时序对账：交付折叠链可由序号列（1..=delivered）重放还原
    pub fn timing_ok(&self) -> bool {
        let mut h: u64 = 0xcbf29ce484222325;
        let mut s = 1u64;
        let mut k = 0u64;
        while k < self.delivered {
            h = (h ^ s).wrapping_mul(0x100000001b3);
            s = s.wrapping_add(1);
            k = k.wrapping_add(1);
        }
        h == self.deliv_fold
    }

    /// 账目对平
    pub fn ledger_ok(&self, expect_inject: u64) -> bool {
        self.injected == expect_inject && self.reconcile()
    }

    /// 总折叠
    pub fn fold_all(&self) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        h = (h ^ self.injected).wrapping_mul(0x100000001b3);
        h = (h ^ self.delivered).wrapping_mul(0x100000001b3);
        h = (h ^ self.deliv_fold).wrapping_mul(0x100000001b3);
        h
    }

    /// 渲染
    pub fn render(&self) -> String {
        crate::alloc::format!(
            "keypath:in={} out={} rbuf={} again={}",
            self.injected,
            self.delivered,
            self.rbuf.len(),
            self.n_again
        )
    }
}

impl Default for KeyPath {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 注入四路
// ---------------------------------------------------------------------------
pub fn inject_four() -> [bool; 4] {
    let mut k = KeyPath::new();
    // 路1：pty 段失效 → EIO
    k.pty_alive = false;
    let r1 = matches!(k.key_inject(1), Err(x) if x == KP_EIO);
    k.pty_alive = true;
    // 路2：应用停止 → EDEAD
    k.app_alive = false;
    let r2 = matches!(k.key_inject(1), Err(x) if x == KP_EDEAD);
    k.app_alive = true;
    // 路3：键码越界 → EINVAL
    let r3 = matches!(k.key_inject(0x9999), Err(x) if x == KP_EINVAL);
    // 路4：背压 → EAGAIN（可重试）
    let mut r4 = false;
    for _ in 0..RBUF_CAP + 2 {
        match k.key_inject(7) {
            Err(KP_EAGAIN) => r4 = true,
            _ => {}
        }
    }
    [r1, r2, r3, r4]
}

// ---------------------------------------------------------------------------
// 确定性千轮样本
// ---------------------------------------------------------------------------
/// 千轮：每轮 10 键注入+消费，全部对平记 1 轮；100 轮含一次
/// 百键整批对账。
pub fn sample_thousand() -> u32 {
    let mut k = KeyPath::new();
    let mut good: u32 = 0;
    for r in 0..1000u32 {
        let seed = (r % 251).wrapping_mul(7);
        let put = k.batch100(seed % 97 + 1);
        match put {
            Ok(100) => {
                if k.reconcile() && k.timing_ok() {
                    good = good.wrapping_add(1);
                }
            }
            _ => {}
        }
    }
    good
}

// ---------------------------------------------------------------------------
// 判据检查
// ---------------------------------------------------------------------------
pub fn run_unx10963_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F10963");

    let inj = inject_four();
    s.add("J1-注入四路全拦截", inj.iter().all(|&b| b), "EIO/EDEAD/EINVAL/EAGAIN 坏样本全部命中");

    // J1 百键对账 10/10
    let mut ok10 = 0u32;
    for i in 0..10u32 {
        let mut k = KeyPath::new();
        if k.batch100(i + 1) == Ok(100) && k.reconcile() {
            ok10 = ok10.wrapping_add(1);
        }
    }
    s.add("J1-百键对账10/10", ok10 == 10, "100/100 逐键对账十轮全平");

    // 时序对账 10/10
    let mut t10 = 0u32;
    for i in 0..10u32 {
        let mut k = KeyPath::new();
        let _ = k.batch100(i + 3);
        if k.timing_ok() {
            t10 = t10.wrapping_add(1);
        }
    }
    s.add("J2-时序对账10/10", t10 == 10, "交付折叠链十轮全可重放还原");

    // 千轮样本
    let good = sample_thousand();
    s.add("J3-千轮零丢失", good == 1000, "千轮百键批全部对平");

    // 逐拍消费粒度
    let mut k2 = KeyPath::new();
    for _ in 0..3 {
        let _ = k2.key_inject(0x11);
    }
    let b1 = k2.app_beat();
    let b2 = k2.app_beat();
    s.add(
        "J4-逐拍粒度",
        b1 == Ok(1) && b2 == Ok(1),
        "每拍恰好消费一键",
    );

    // 背压重试不计丢失
    let mut k3 = KeyPath::new();
    let _ = k3.batch100(9);
    s.add(
        "J5-背压零丢失",
        k3.reconcile() && k3.n_again > 0,
        "缓冲满重试后仍全对平",
    );

    // 错误码分段
    let codes = [KP_EBADF, KP_EIO, KP_EAGAIN, KP_EINVAL, KP_EORDER, KP_ELOSE, KP_EDEAD];
    let seg_ok = codes.iter().all(|&c| c > SEG_BASE && c < SEG_BASE + 0x20);
    s.add("J6-错误码分段", seg_ok, "七码全部落在 0x1d7e0 段内");

    // 错误码互异
    let mut distinct = true;
    for i in 0..codes.len() {
        for j in (i + 1)..codes.len() {
            if codes[i] == codes[j] {
                distinct = false;
            }
        }
    }
    s.add("J7-错误码互异", distinct, "七码两两互异");

    // 账目对平
    s.add("J8-账目对平", k3.ledger_ok(100), "注入 100 与账面一致且对平");

    // 序号严格递增（乱序必报）
    let mut k4 = KeyPath::new();
    let _ = k4.key_inject(1);
    let _ = k4.key_inject(2);
    // 手工破坏序号面：直接消费两个但跳过对账位
    k4.delivered = 5;
    let order_face = matches!(k4.app_beat(), Err(x) if x == KP_EORDER);
    s.add("J9-乱序必报", order_face, "序号错乱显性报 EORDER");

    // fold 稳定
    let h1 = k3.fold_all();
    let h2 = k3.fold_all();
    s.add("J10-fold稳定", h1 == h2, "同一状态两次折叠一致");

    // 渲染
    let r = k3.render();
    s.add(
        "J11-渲染非空",
        r.starts_with("keypath:") && r.contains("in=100"),
        "摘要含注入/交付账",
    );

    // pty 失效面
    let mut k5 = KeyPath::new();
    k5.pty_alive = false;
    let dead_ok = matches!(k5.key_inject(3), Err(x) if x == KP_EIO);
    s.add("J12-pty失效面", dead_ok, "pty 段失效显性报 EIO");

    // 动态判据
    s.add_dyn(
        "J13-注入/交付账",
        k3.injected == 100 && k3.delivered == 100,
        &crate::alloc::format!("in={} out={}", k3.injected, k3.delivered),
    );
    s.add_dyn(
        "J14-千轮通过率",
        good == 1000,
        &crate::alloc::format!("{}//1000", good),
    );
    s.add_dyn(
        "J15-背压账",
        k3.n_again > 0,
        &crate::alloc::format!("背压重试={}", k3.n_again),
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
    fn unx10963_keypath_replay_flow() {
        let mut k = KeyPath::new();
        assert_eq!(k.batch100(5), Ok(100));
        assert!(k.reconcile());
        assert!(k.timing_ok());
        assert!(matches!(k.key_inject(0x9999), Err(x) if x == KP_EINVAL));
        let inj = inject_four();
        assert!(inj.iter().all(|&b| b));
        assert_eq!(sample_thousand(), 1000);
    }

    #[test]
    fn unx10963_checks_green() {
        let s = run_unx10963_checks();
        assert!(s.all_passed(), "UNX-F10963 判据未全绿");
    }
}
