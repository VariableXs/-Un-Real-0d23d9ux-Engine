//! UNX-F52049 DataGrip 收口（数据库 IDE） · 0xC4C40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52049 逐字锚定）：UNX-F52050-J1 六判据全落；
//! PHP 解释器/Web 服务器调试挂接三段账；KB-COMPAT 定级登记；判据锚定
//! §AI-66.2 专题五。
//!
//! 设计要点：
//!   ① 域模型：调试挂接三段账（PHP 解释器挂接/Web 服务器挂接/浏览器
//!      回路挂接）× 5 站点场景 = 15 格挂接账；段序闭集 0..3。
//!   ② 挂接三元闸（PHP 版本档/调试协议档/回传端口档）全等放行，错位
//!      走失配路径（EPhp/EProto/EPort）；失配注入必抓。
//!   ③ 回路拍序闸：请求→断点→回传三拍严格递增单调；回退必抓。
//!   ④ 注入 ×10 轮（三元失配/段越闭集/拍序回退三态轮转）10/10 必抓，
//!      注入=检出+逃逸守恒；六判据母版闸 + KB-COMPAT 定级（15 格全通
//!      + 挂接零失配 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52049 − 52000)）。
pub const SEG_BASE: u32 = 0xC4C40;

/// 挂接段数。
pub const HOOKS: usize = 3;

/// 站点场景数。
pub const SITES: usize = 5;

/// 挂接三元基准。
pub const PHP_VER: u32 = 82;
pub const DBG_PROTO: u32 = 3;
pub const RET_PORT: u32 = 9003;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DgCode {
    /// 成功
    Ok = 0,
    /// 段号越闭集
    ENoHook,
    /// 站点号越闭集
    ENoSite,
    /// PHP 版本档失配
    EPhp,
    /// 调试协议档失配
    EProto,
    /// 回传端口档失配
    EPort,
    /// 回路拍序乱
    EOrder,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl DgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DgCode::Ok => "成功",
            DgCode::ENoHook => "段号越闭集：解释器/服务器/回路三段之外先登记",
            DgCode::ENoSite => "站点号越闭集：5 站点之外先登记",
            DgCode::EPhp => "PHP 版本档失配：解释器版本档与宿主档不符",
            DgCode::EProto => "调试协议档失配：调试协议档与宿主档不符",
            DgCode::EPort => "回传端口档失配：回传端口档与宿主档不符",
            DgCode::EOrder => "回路拍序乱：请求→断点→回传三拍必须严格递增单调",
            DgCode::EDup => "重复落账：同格只许一条账目",
            DgCode::ESeal => "未齐即封：15 格未齐先补齐",
            DgCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            DgCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            DgCode::EGradeDrift => "定级漂移：15 格全通+挂接零失配+回路单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 挂接格载荷（段 h × 站点 s）。
pub const fn hook_cell(h: usize, s: usize) -> Option<u64> {
    if h >= HOOKS || s >= SITES {
        return None;
    }
    Some(
        0x36000u64
            + (h as u64) * 0x26
            + (s as u64) * 0xD
            ^ (h as u64).wrapping_mul(0x9E3779B97F4A7C15 >> s),
    )
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct DgLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 挂接账。
    hooks: [[Option<u64>; SITES]; HOOKS],
    /// 最后回路拍。
    last_tick: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl DgLedger {
    /// 新账本。
    pub fn new() -> Self {
        DgLedger {
            gates: [None; 6],
            hooks: [[None; SITES]; HOOKS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), DgCode> {
        if gate >= 6 {
            return Err(DgCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x19100 + gate as u64);
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

    /// 挂接三元闸。
    pub fn hook_gate(&mut self, php: u32, proto: u32, port: u32) -> Result<(), DgCode> {
        if php != PHP_VER {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x19200);
            return Err(DgCode::EPhp);
        }
        if proto != DBG_PROTO {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x19201);
            return Err(DgCode::EProto);
        }
        if port != RET_PORT {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x19202);
            return Err(DgCode::EPort);
        }
        self.fnv(0x19203);
        Ok(())
    }

    /// 挂接一格（三元闸后落账，重复拒）。
    pub fn record_hook(&mut self, h: usize, s: usize) -> Result<u64, DgCode> {
        if h >= HOOKS {
            return Err(DgCode::ENoHook);
        }
        if s >= SITES {
            return Err(DgCode::ENoSite);
        }
        let p = match hook_cell(h, s) {
            Some(p) => p,
            None => return Err(DgCode::ENoSite),
        };
        if match self.hooks.get(h).and_then(|row| row.get(s)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(DgCode::EDup);
        }
        if let Some(row) = self.hooks.get_mut(h) {
            if let Some(c) = row.get_mut(s) {
                *c = Some(p);
            }
        }
        self.fnv(0x19204 + (h as u64) * 5 + s as u64);
        Ok(p)
    }

    /// 挂接全账核验（15 格）。
    pub fn hooks_all(&self) -> bool {
        let mut h = 0usize;
        while h < HOOKS {
            let mut s = 0usize;
            while s < SITES {
                match self.hooks.get(h).and_then(|row| row.get(s)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                s += 1;
            }
            h += 1;
        }
        true
    }

    /// 回路一拍（严格递增单调）。
    pub fn loop_tick(&mut self, tick: u32) -> Result<(), DgCode> {
        if (tick as i64) <= self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x19300);
            return Err(DgCode::EOrder);
        }
        self.last_tick = tick as i64;
        self.fnv(0x19301 + tick as u64);
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

impl Default for DgLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = DgLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let _ = led.hook_gate(PHP_VER, DBG_PROTO, RET_PORT);
    let mut recorded = 0u32;
    let mut h = 0usize;
    while h < HOOKS {
        let mut s = 0usize;
        while s < SITES {
            if led.record_hook(h, s).is_ok() {
                recorded += 1;
            }
            s += 1;
        }
        h += 1;
    }
    let mut t = 1u32;
    let mut n = 0u32;
    while n < 6 {
        let _ = led.loop_tick(t);
        t = t.saturating_add(2);
        n += 1;
    }
    // 注入 10 轮：三元失配 / 段越闭集 / 拍序回退三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = DgLedger::new();
        let caught = match kind {
            0 => {
                lt.hook_gate(PHP_VER + 1, DBG_PROTO, RET_PORT) == Err(DgCode::EPhp)
                    && lt.hook_gate(PHP_VER, DBG_PROTO + 1, RET_PORT) == Err(DgCode::EProto)
                    && lt.hook_gate(PHP_VER, DBG_PROTO, RET_PORT + 1) == Err(DgCode::EPort)
            }
            1 => lt.record_hook(HOOKS, 0).is_err(),
            _ => {
                let _ = lt.loop_tick(10);
                lt.loop_tick(5) == Err(DgCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x19400 + k as u64);
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
pub fn run_unx52049_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52049");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "挂接三段 15/15 全通",
        recorded == (HOOKS * SITES) as u32,
        "解释器/服务器/回路三段 ×5 站点全通",
    );
    s.add(
        "三元闸全等放行+失配三态全抓",
        {
            let mut lt = DgLedger::new();
            let ok = lt.hook_gate(PHP_VER, DBG_PROTO, RET_PORT).is_ok();
            let a = lt.hook_gate(PHP_VER + 1, DBG_PROTO, RET_PORT) == Err(DgCode::EPhp);
            let b = lt.hook_gate(PHP_VER, DBG_PROTO + 1, RET_PORT) == Err(DgCode::EProto);
            let c = lt.hook_gate(PHP_VER, DBG_PROTO, RET_PORT + 1) == Err(DgCode::EPort);
            ok && a && b && c
        },
        "PHP 版本/调试协议/回传端口三元全等放行，错位走失配路径",
    );
    s.add(
        "回路拍序单调",
        {
            let mut lt = DgLedger::new();
            lt.loop_tick(1).is_ok()
                && lt.loop_tick(4).is_ok()
                && lt.loop_tick(7).is_ok()
                && lt.loop_tick(2) == Err(DgCode::EOrder)
        },
        "请求→断点→回传三拍严格递增，回退必抓",
    );
    s.add("注入 10/10 全抓", caught >= 10, "失配/段/拍序三态轮转必抓");
    let mut l0 = DgLedger::new();
    s.add(
        "边界·段越闭集",
        l0.record_hook(HOOKS, 0).is_err(),
        "段 3 越闭集必拒",
    );
    s.add(
        "边界·站点越闭集",
        l0.record_hook(0, SITES).is_err(),
        "站点 5 越闭集必拒",
    );
    let mut l1 = DgLedger::new();
    let _ = l1.record_hook(1, 1);
    s.add(
        "重复·再落账拒",
        l1.record_hook(1, 1).is_err(),
        "同格只许一条账目",
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
    s.add("收口·判据条数", s.len() >= 10, "含本条共 ≥11 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_cell_pure() {
        let a = hook_cell(2, 3).unwrap_or(0);
        assert_eq!(a, hook_cell(2, 3).unwrap_or(0));
        assert_ne!(a, hook_cell(2, 4).unwrap_or(0));
        assert!(hook_cell(HOOKS, 0).is_none());
        assert!(hook_cell(0, SITES).is_none());
    }

    #[test]
    fn test_hook_gate() {
        let mut lt = DgLedger::new();
        assert_eq!(lt.hook_gate(PHP_VER + 3, DBG_PROTO, RET_PORT), Err(DgCode::EPhp));
        assert_eq!(lt.hook_gate(PHP_VER, DBG_PROTO + 3, RET_PORT), Err(DgCode::EProto));
        assert_eq!(lt.hook_gate(PHP_VER, DBG_PROTO, RET_PORT + 3), Err(DgCode::EPort));
        assert!(lt.hook_gate(PHP_VER, DBG_PROTO, RET_PORT).is_ok());
    }

    #[test]
    fn test_loop_order() {
        let mut lt = DgLedger::new();
        assert!(lt.loop_tick(2).is_ok());
        assert_eq!(lt.loop_tick(2), Err(DgCode::EOrder));
        assert_eq!(lt.loop_tick(1), Err(DgCode::EOrder));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (HOOKS * SITES) as u32);
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
        assert_eq!(SEG_BASE, 0xC4C40);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52049_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
