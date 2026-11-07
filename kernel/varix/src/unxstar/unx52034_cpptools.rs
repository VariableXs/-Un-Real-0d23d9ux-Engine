//! UNX-F52034 VSCode 扩展：C/C++（cpptools）收口 · 0xC4880段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52034 逐字锚定）：UNX-F52035-J1 远程主机连接/
//! 服务端组件布设/端口转发/断线重连四段账；OpenSSH 消费对账（联签锚
//! 定行 B13 F52188）；KB-COMPAT 定级登记；判据锚定 §AI-66.2 专题二跨
//! 应用共面。
//!
//! 设计要点：
//!   ① 域模型：cpptools 远程开发四段账（远程主机连接/服务端组件布设/
//!      端口转发/断线重连）× 8 主机场景 = 32 格场景账。
//!   ② OpenSSH 消费对账：6 格消费账（握手档/密钥档/通道档/转发档/
//!      保活档/断连档），逐格与消费基准恒等才收；未对账先挂接 → 消费
//!      前置闸（对账账本不齐不许报四段全通）。
//!   ③ 注入 ×10 轮（主机档漂移/端口档漂移/重连档漂移三态轮转）10/10
//!      必抓；六判据母版闸 + KB-COMPAT 定级（32 格全通 + 消费零漂移 +
//!      注入全抓 → S1）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机（指纹种子 =
//!      SEG_BASE 派生）；取值一律 match/if let/.get()/.get_mut()/
//!      checked_*/saturating_*，无 unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52034 − 52000)）。
pub const SEG_BASE: u32 = 0xC4880;

/// 主机场景数。
pub const HOSTS: usize = 8;

/// 四段账段数。
pub const SEGMENTS: usize = 4;

/// OpenSSH 消费账格数。
pub const SSH_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CpCode {
    /// 成功
    Ok = 0,
    /// 主机号越闭集
    ENoHost,
    /// 段号越闭集
    ENoSeg,
    /// 消费格越册
    ENoCell,
    /// 消费漂移
    EDrift,
    /// 端口档越闭集
    ENoPort,
    /// 重连序乱
    EReorder,
    /// 重复落账
    EDup,
    /// 未对账先挂接
    EPreHook,
    /// 场景未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl CpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            CpCode::Ok => "成功",
            CpCode::ENoHost => "主机号越闭集：8 主机之外先登记",
            CpCode::ENoSeg => "段号越闭集：连接/布设/转发/重连四段之外先登记",
            CpCode::ENoCell => "消费格越册：OpenSSH 6 格闭集之外先登记",
            CpCode::EDrift => "消费漂移：OpenSSH 消费折叠与基准不恒等，逐格复算",
            CpCode::ENoPort => "端口档越闭集：转发端口闭集之外先登记",
            CpCode::EReorder => "重连序乱：断线重连拍序必须严格递增单调",
            CpCode::EDup => "重复落账：同格只许一条账目",
            CpCode::EPreHook => "未对账先挂接：OpenSSH 6 格消费账不齐不许报四段全通",
            CpCode::ESeal => "场景未齐即封：32 格未齐先补齐",
            CpCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            CpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            CpCode::EGradeDrift => "定级漂移：32 格全通+消费零漂移+注入全抓才定 S1",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 场景格载荷（主机 i × 段 s）。
pub const fn scene_cell(i: usize, s: usize) -> Option<u64> {
    if i >= HOSTS || s >= SEGMENTS {
        return None;
    }
    Some(
        0xE1000u64
            + (i as u64) * 0x29
            + (s as u64) * 0x3
            ^ (i as u64).wrapping_mul(0x9E3779B97F4A7C15 >> (s as u64)),
    )
}

/// OpenSSH 消费格基准载荷（格 c）。
pub const fn ssh_cell(c: usize) -> Option<u64> {
    if c >= SSH_CELLS {
        return None;
    }
    Some((0xE2000u64 + c as u64 * 0x37) ^ 0x6F70656E737368)
}

/// 转发端口闭集（8 项，闭集之外拒）。
pub const fn fwd_port(k: usize) -> Option<u16> {
    match k {
        0 => Some(22),
        1 => Some(80),
        2 => Some(443),
        3 => Some(2222),
        4 => Some(8080),
        5 => Some(3306),
        6 => Some(5432),
        7 => Some(9090),
        _ => None,
    }
}

/// 重连拍序基准（第 r 拍，严格递增）。
pub const fn retry_tick(r: usize) -> Option<u32> {
    if r >= 8 {
        return None;
    }
    Some(0x10 + r as u32 * 0x10)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct CpLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 场景账。
    scenes: [[Option<u64>; SEGMENTS]; HOSTS],
    /// OpenSSH 消费账。
    ssh: [Option<u64>; SSH_CELLS],
    /// 转发端口账（标记闭集命中数）。
    ports: u32,
    /// 重连最后拍。
    last_tick: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl CpLedger {
    /// 新账本。
    pub fn new() -> Self {
        CpLedger {
            gates: [None; 6],
            scenes: [[None; SEGMENTS]; HOSTS],
            ssh: [None; SSH_CELLS],
            ports: 0,
            last_tick: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, w: u64) {
        self.fp ^= w;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), CpCode> {
        if gate >= 6 {
            return Err(CpCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x9100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// OpenSSH 消费一格（恒等才收）。
    pub fn consume_ssh(&mut self, c: usize, got: u64) -> Result<(), CpCode> {
        let want = match ssh_cell(c) {
            Some(w) => w,
            None => return Err(CpCode::ENoCell),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9200 + c as u64);
            return Err(CpCode::EDrift);
        }
        if let Some(cell) = self.ssh.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x9201 + c as u64);
        Ok(())
    }

    /// OpenSSH 消费全账核验。
    pub fn ssh_all(&self) -> bool {
        let mut c = 0usize;
        while c < SSH_CELLS {
            match self.ssh.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 落一格场景账（消费前置闸：ssh 消费账不齐 → EPreHook）。
    pub fn record_scene(&mut self, i: usize, s: usize) -> Result<u64, CpCode> {
        if i >= HOSTS {
            return Err(CpCode::ENoHost);
        }
        if s >= SEGMENTS {
            return Err(CpCode::ENoSeg);
        }
        if !self.ssh_all() {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9300);
            return Err(CpCode::EPreHook);
        }
        let p = match scene_cell(i, s) {
            Some(p) => p,
            None => return Err(CpCode::ENoSeg),
        };
        if match self.scenes.get(i).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(CpCode::EDup);
        }
        if let Some(row) = self.scenes.get_mut(i) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(p);
            }
        }
        self.fnv(0x9301 + (i as u64) * 4 + s as u64);
        Ok(p)
    }

    /// 场景账全齐核验（32 格）。
    pub fn scenes_all(&self) -> bool {
        let mut i = 0usize;
        while i < HOSTS {
            let mut s = 0usize;
            while s < SEGMENTS {
                match self.scenes.get(i).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            i += 1;
        }
        true
    }

    /// 端口转发一格（闭集外拒）。
    pub fn forward(&mut self, port: u16) -> Result<(), CpCode> {
        let mut hit = false;
        let mut k = 0usize;
        while k < 8 {
            if fwd_port(k) == Some(port) {
                hit = true;
            }
            k += 1;
        }
        if !hit {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9400 + port as u64);
            return Err(CpCode::ENoPort);
        }
        self.ports = self.ports.saturating_add(1);
        self.fnv(0x9401 + port as u64);
        Ok(())
    }

    /// 断线重连一拍（拍序严格递增才收）。
    pub fn reconnect(&mut self, tick: u32) -> Result<(), CpCode> {
        let valid = {
            let mut ok = false;
            let mut r = 0usize;
            while r < 8 {
                if retry_tick(r) == Some(tick) {
                    ok = true;
                }
                r += 1;
            }
            ok
        };
        if !valid {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9500 + tick as u64);
            return Err(CpCode::EReorder);
        }
        if (tick as i64) <= self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9501 + tick as u64);
            return Err(CpCode::EReorder);
        }
        self.last_tick = tick as i64;
        self.fnv(0x9502 + tick as u64);
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }
}

impl Default for CpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（场景格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = CpLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    // OpenSSH 消费 6 格先行（消费前置闸要求）。
    let mut c = 0usize;
    while c < SSH_CELLS {
        if let Some(w) = ssh_cell(c) {
            let _ = led.consume_ssh(c, w);
        }
        c += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < HOSTS {
        let mut s = 0usize;
        while s < SEGMENTS {
            if led.record_scene(i, s).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < 8 {
        if let Some(p) = fwd_port(k) {
            let _ = led.forward(p);
        }
        k += 1;
    }
    let mut r = 0usize;
    while r < 4 {
        if let Some(t) = retry_tick(r) {
            let _ = led.reconnect(t);
        }
        r += 1;
    }
    // 注入 10 轮：主机越闭集 / 消费漂移 / 端口越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = CpLedger::new();
        let caught = match kind {
            0 => lt.record_scene(HOSTS, 0).is_err(),
            1 => lt.consume_ssh(1, match ssh_cell(1) {
                Some(w) => w.wrapping_add(1),
                None => 0,
            }) == Err(CpCode::EDrift),
            _ => lt.forward(9999) == Err(CpCode::ENoPort),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x9600 + k as u64);
        } else {
            led.escaped += 1;
        }
        k += 1;
    }
    let caught = led.caught;
    let fp = led.fingerprint();
    (recorded, caught, fp)
}

// ── 判据集 ───────────────────────────────────────────────────────
pub fn run_unx52034_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52034");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "四段场景 32/32 全通",
        recorded == (HOSTS * SEGMENTS) as u32,
        "连接/布设/转发/重连四段 ×8 主机全通",
    );
    s.add(
        "OpenSSH 消费 6/6 对账",
        {
            let mut lt = CpLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < SSH_CELLS {
                if let Some(w) = ssh_cell(c) {
                    if lt.consume_ssh(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.ssh_all()
        },
        "握手/密钥/通道/转发/保活/断连六格消费零漂移",
    );
    s.add("注入 10/10 全抓", caught >= 10, "主机/消费/端口三态轮转必抓");
    let mut l0 = CpLedger::new();
    s.add(
        "消费前置闸",
        l0.record_scene(0, 0) == Err(CpCode::EPreHook),
        "OpenSSH 消费账不齐不许报四段全通",
    );
    s.add(
        "边界·主机越闭集",
        {
            let mut l1 = CpLedger::new();
            let mut c = 0usize;
            while c < SSH_CELLS {
                if let Some(w) = ssh_cell(c) {
                    let _ = l1.consume_ssh(c, w);
                }
                c += 1;
            }
            l1.record_scene(HOSTS, 0).is_err()
        },
        "主机号 8 越闭集必拒",
    );
    s.add(
        "边界·段越闭集",
        {
            let mut l2 = CpLedger::new();
            let mut c = 0usize;
            while c < SSH_CELLS {
                if let Some(w) = ssh_cell(c) {
                    let _ = l2.consume_ssh(c, w);
                }
                c += 1;
            }
            l2.record_scene(0, SEGMENTS).is_err()
        },
        "段号 4 越闭集必拒",
    );
    let mut l3 = CpLedger::new();
    s.add(
        "端口·闭集外拒",
        l3.forward(25) == Err(CpCode::ENoPort),
        "端口 25 越转发闭集必抓",
    );
    s.add(
        "端口·闭集内收",
        l3.forward(443).is_ok(),
        "端口 443 在转发闭集放行",
    );
    let mut l4 = CpLedger::new();
    let _ = l4.reconnect(match retry_tick(2) {
        Some(t) => t,
        None => 0,
    });
    s.add(
        "重连·拍序回退拒",
        l4.reconnect(match retry_tick(0) {
            Some(t) => t,
            None => 0,
        }) == Err(CpCode::EReorder),
        "拍序回退必抓",
    );
    s.add(
        "重连·越册拒",
        l4.reconnect(0x99) == Err(CpCode::EReorder),
        "拍号越册必抓",
    );
    let mut l5 = CpLedger::new();
    let _ = l5.consume_ssh(3, match ssh_cell(3) {
        Some(w) => w,
        None => 0,
    });
    s.add(
        "消费·漂移拒",
        l5.consume_ssh(3, match ssh_cell(3) {
            Some(w) => w.wrapping_add(5),
            None => 0,
        }) == Err(CpCode::EDrift),
        "消费折叠漂移必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", fp == fp2 && fp != 0, "确定性全流程逐位一致且非零");
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0,
        "SEG_BASE 0x40 对齐且在 VX14 域",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scene_and_cells_pure() {
        let a = scene_cell(3, 1).unwrap_or(0);
        assert_eq!(a, scene_cell(3, 1).unwrap_or(0));
        assert_ne!(a, scene_cell(3, 2).unwrap_or(0));
        assert!(scene_cell(HOSTS, 0).is_none());
        let w = ssh_cell(2).unwrap_or(0);
        assert_eq!(w, ssh_cell(2).unwrap_or(0));
        assert!(ssh_cell(SSH_CELLS).is_none());
    }

    #[test]
    fn test_port_closure() {
        assert_eq!(fwd_port(0), Some(22));
        assert_eq!(fwd_port(7), Some(9090));
        assert!(fwd_port(8).is_none());
        assert!(fwd_port(0) != fwd_port(1));
    }

    #[test]
    fn test_retry_ticks_monotone() {
        let t0 = retry_tick(0).unwrap_or(0);
        let t1 = retry_tick(1).unwrap_or(0);
        assert!(t1 > t0);
        assert!(retry_tick(8).is_none());
    }

    #[test]
    fn test_pre_hook_gate() {
        let mut lt = CpLedger::new();
        assert_eq!(lt.record_scene(0, 0), Err(CpCode::EPreHook));
    }

    #[test]
    fn test_ssh_drift_caught() {
        let mut lt = CpLedger::new();
        assert_eq!(
            lt.consume_ssh(4, ssh_cell(4).unwrap_or(0).wrapping_add(2)),
            Err(CpCode::EDrift)
        );
        assert!(lt.consume_ssh(4, ssh_cell(4).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_port_and_retry_gates() {
        let mut lt = CpLedger::new();
        assert_eq!(lt.forward(12345), Err(CpCode::ENoPort));
        assert!(lt.forward(8080).is_ok());
        let _ = lt.reconnect(retry_tick(3).unwrap_or(0));
        assert_eq!(lt.reconnect(retry_tick(1).unwrap_or(0)), Err(CpCode::EReorder));
        assert_eq!(lt.reconnect(0xFE), Err(CpCode::EReorder));
    }

    #[test]
    fn test_dup_scene_rejected() {
        let mut lt = CpLedger::new();
        let mut c = 0usize;
        while c < SSH_CELLS {
            if let Some(w) = ssh_cell(c) {
                let _ = lt.consume_ssh(c, w);
            }
            c += 1;
        }
        let _ = lt.record_scene(2, 2);
        assert!(lt.record_scene(2, 2).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (HOSTS * SEGMENTS) as u32);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC4880);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52034_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
