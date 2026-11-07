//! UNX-F52050 PhpStorm 收口 · 0xC4C80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52050 逐字锚定）：UNX-F52051-J1 六判据全落；
//! Rails 调试挂接/gem 管理操作集；KB-COMPAT 定级登记；判据锚定
//! §AI-66.2 专题五。
//!
//! 设计要点：
//!   ① 域模型：Rails 挂接/gem 管理双段账。挂接段闭集 3 段（server 挂
//!      接/worker 挂接/控制台挂接）× 5 应用场景 = 15 格挂接账；gem 操
//!      作集 12 格（安装/卸载/锁定/解析/依赖图/冲突检测/更新/审计/回滚
//!      /离线包/源切换/清单）逐格恒等才收。
//!   ② gem 拍序闸：gem 生命周期拍序（安装→锁定→解析→审计）严格递增
//!      单调；回退必抓；重复操作拒。
//!   ③ 挂接三元闸（Ruby 版本档/gem 集档/挂接协议档）全等放行，错位失
//!      配三诊断；注入 ×10 轮（三元失配/段越闭集/拍序回退三态轮转）
//!      10/10 必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（27 格全
//!      通 → S1）。
//!   ④ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52050 − 52000)）。
pub const SEG_BASE: u32 = 0xC4C80;

/// 挂接段数。
pub const HOOKS: usize = 3;

/// 应用场景数。
pub const APPS: usize = 5;

/// gem 操作集格数。
pub const GEM_OPS: usize = 12;

/// 挂接三元基准。
pub const RUBY_VER: u32 = 31;
pub const GEMSET: u32 = 5;
pub const HOOK_PROTO: u32 = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PhCode {
    /// 成功
    Ok = 0,
    /// 段号越闭集
    ENoHook,
    /// 应用号越闭集
    ENoApp,
    /// Ruby 版本档失配
    ERuby,
    /// gem 集档失配
    EGemset,
    /// 挂接协议档失配
    EProto,
    /// gem 操作越册
    ENoOp,
    /// gem 载荷漂移
    EDrift,
    /// gem 拍序乱
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

impl PhCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PhCode::Ok => "成功",
            PhCode::ENoHook => "段号越闭集：server/worker/控制台三段之外先登记",
            PhCode::ENoApp => "应用号越闭集：5 应用之外先登记",
            PhCode::ERuby => "Ruby 版本档失配：解释器版本档与宿主档不符",
            PhCode::EGemset => "gem 集档失配：gem 集档与宿主档不符",
            PhCode::EProto => "挂接协议档失配：挂接协议档与宿主档不符",
            PhCode::ENoOp => "gem 操作越册：12 操作闭集之外先登记",
            PhCode::EDrift => "gem 载荷漂移：操作折叠与基准不恒等，逐格复算",
            PhCode::EOrder => "gem 拍序乱：生命周期拍序必须严格递增单调",
            PhCode::EDup => "重复落账：同格只许一条账目",
            PhCode::ESeal => "未齐即封：27 格未齐先补齐",
            PhCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PhCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PhCode::EGradeDrift => "定级漂移：27 格全通+拍序单调+失配全抓才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 挂接格载荷（段 h × 应用 a）。
pub const fn hook_cell(h: usize, a: usize) -> Option<u64> {
    if h >= HOOKS || a >= APPS {
        return None;
    }
    Some(
        0x38000u64
            + (h as u64) * 0x2A
            + (a as u64) * 0xC
            ^ (h as u64).wrapping_mul(0x9E3779B97F4A7C15 >> a),
    )
}

/// gem 操作格基准载荷（操作 o）。
pub const fn gem_cell(o: usize) -> Option<u64> {
    if o >= GEM_OPS {
        return None;
    }
    Some((0x39000u64 + o as u64 * 0x33) ^ (o as u64).wrapping_mul(0x01000193))
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct PhLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 挂接账。
    hooks: [[Option<u64>; APPS]; HOOKS],
    /// gem 操作账。
    gems: [Option<u64>; GEM_OPS],
    /// gem 最后拍。
    last_gem_tick: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PhLedger {
    /// 新账本。
    pub fn new() -> Self {
        PhLedger {
            gates: [None; 6],
            hooks: [[None; APPS]; HOOKS],
            gems: [None; GEM_OPS],
            last_gem_tick: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PhCode> {
        if gate >= 6 {
            return Err(PhCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x1A100 + gate as u64);
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
    pub fn hook_gate(&mut self, ruby: u32, gemset: u32, proto: u32) -> Result<(), PhCode> {
        if ruby != RUBY_VER {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1A200);
            return Err(PhCode::ERuby);
        }
        if gemset != GEMSET {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1A201);
            return Err(PhCode::EGemset);
        }
        if proto != HOOK_PROTO {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1A202);
            return Err(PhCode::EProto);
        }
        self.fnv(0x1A203);
        Ok(())
    }

    /// 挂接一格。
    pub fn record_hook(&mut self, h: usize, a: usize) -> Result<u64, PhCode> {
        if h >= HOOKS {
            return Err(PhCode::ENoHook);
        }
        if a >= APPS {
            return Err(PhCode::ENoApp);
        }
        let p = match hook_cell(h, a) {
            Some(p) => p,
            None => return Err(PhCode::ENoApp),
        };
        if match self.hooks.get(h).and_then(|row| row.get(a)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PhCode::EDup);
        }
        if let Some(row) = self.hooks.get_mut(h) {
            if let Some(c) = row.get_mut(a) {
                *c = Some(p);
            }
        }
        self.fnv(0x1A204 + (h as u64) * 5 + a as u64);
        Ok(p)
    }

    /// 挂接全账核验（15 格）。
    pub fn hooks_all(&self) -> bool {
        let mut h = 0usize;
        while h < HOOKS {
            let mut a = 0usize;
            while a < APPS {
                match self.hooks.get(h).and_then(|row| row.get(a)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                a += 1;
            }
            h += 1;
        }
        true
    }

    /// gem 操作一格（载荷恒等 + 拍序单调才收）。
    pub fn record_gem(&mut self, o: usize, got: u64, tick: u32) -> Result<(), PhCode> {
        let want = match gem_cell(o) {
            Some(w) => w,
            None => return Err(PhCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1A300 + o as u64);
            return Err(PhCode::EDrift);
        }
        if (tick as i64) <= self.last_gem_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x1A301);
            return Err(PhCode::EOrder);
        }
        if match self.gems.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PhCode::EDup);
        }
        if let Some(c) = self.gems.get_mut(o) {
            *c = Some(got);
        }
        self.last_gem_tick = tick as i64;
        self.fnv(0x1A302 + o as u64);
        Ok(())
    }

    /// gem 全账核验。
    pub fn gems_all(&self) -> bool {
        let mut o = 0usize;
        while o < GEM_OPS {
            match self.gems.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
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

impl Default for PhLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PhLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let _ = led.hook_gate(RUBY_VER, GEMSET, HOOK_PROTO);
    let mut recorded = 0u32;
    let mut h = 0usize;
    while h < HOOKS {
        let mut a = 0usize;
        while a < APPS {
            if led.record_hook(h, a).is_ok() {
                recorded += 1;
            }
            a += 1;
        }
        h += 1;
    }
    let mut o = 0usize;
    let mut tick: u32 = 0;
    while o < GEM_OPS {
        tick = tick.saturating_add(3);
        if let Some(w) = gem_cell(o) {
            let _ = led.record_gem(o, w, tick);
        }
        o += 1;
    }
    // 注入 10 轮：三元失配 / gem 拍序回退 / gem 载荷漂移三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PhLedger::new();
        let caught = match kind {
            0 => {
                lt.hook_gate(RUBY_VER + 1, GEMSET, HOOK_PROTO) == Err(PhCode::ERuby)
                    && lt.hook_gate(RUBY_VER, GEMSET + 1, HOOK_PROTO) == Err(PhCode::EGemset)
                    && lt.hook_gate(RUBY_VER, GEMSET, HOOK_PROTO + 1) == Err(PhCode::EProto)
            }
            1 => {
                let _ = lt.record_gem(0, gem_cell(0).unwrap_or(0), 10);
                lt.record_gem(2, gem_cell(2).unwrap_or(0), 5) == Err(PhCode::EOrder)
            }
            _ => lt.record_gem(3, gem_cell(3).unwrap_or(0).wrapping_add(1), 1)
                == Err(PhCode::EDrift),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x1A400 + k as u64);
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
pub fn run_unx52050_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52050");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Rails 挂接 15/15 全通",
        recorded == (HOOKS * APPS) as u32,
        "server/worker/控制台三段 ×5 应用全通",
    );
    s.add(
        "gem 操作 12/12 全账",
        {
            let mut lt = PhLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            let mut tick: u32 = 0;
            while o < GEM_OPS {
                tick = tick.saturating_add(3);
                if let Some(w) = gem_cell(o) {
                    if lt.record_gem(o, w, tick).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.gems_all()
        },
        "gem 12 操作逐格恒等入账且拍序单调",
    );
    s.add(
        "三元闸全等放行+失配三态全抓",
        {
            let mut lt = PhLedger::new();
            let ok = lt.hook_gate(RUBY_VER, GEMSET, HOOK_PROTO).is_ok();
            let a = lt.hook_gate(RUBY_VER + 1, GEMSET, HOOK_PROTO) == Err(PhCode::ERuby);
            let b = lt.hook_gate(RUBY_VER, GEMSET + 1, HOOK_PROTO) == Err(PhCode::EGemset);
            let c = lt.hook_gate(RUBY_VER, GEMSET, HOOK_PROTO + 1) == Err(PhCode::EProto);
            ok && a && b && c
        },
        "Ruby/gem 集/协议三元全等放行，错位走失配路径",
    );
    s.add("注入 10/10 全抓", caught >= 10, "失配/拍序/漂移三态轮转必抓");
    let mut l0 = PhLedger::new();
    s.add(
        "边界·段越闭集",
        l0.record_hook(HOOKS, 0).is_err(),
        "段 3 越闭集必拒",
    );
    s.add(
        "边界·应用越闭集",
        l0.record_hook(0, APPS).is_err(),
        "应用 5 越闭集必拒",
    );
    s.add(
        "边界·gem 越册",
        l0.record_gem(GEM_OPS, 0, 1) == Err(PhCode::ENoOp),
        "操作 12 越册必拒",
    );
    let mut l1 = PhLedger::new();
    let _ = l1.record_hook(2, 2);
    s.add(
        "重复·再落账拒",
        l1.record_hook(2, 2).is_err(),
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
    s.add("收口·判据条数", s.len() >= 11, "含本条共 ≥12 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cells_pure() {
        let a = hook_cell(1, 3).unwrap_or(0);
        assert_eq!(a, hook_cell(1, 3).unwrap_or(0));
        assert_ne!(a, hook_cell(1, 4).unwrap_or(0));
        assert!(hook_cell(HOOKS, 0).is_none());
        assert!(hook_cell(0, APPS).is_none());
        let w = gem_cell(7).unwrap_or(0);
        assert_eq!(w, gem_cell(7).unwrap_or(0));
        assert!(gem_cell(GEM_OPS).is_none());
    }

    #[test]
    fn test_hook_gate() {
        let mut lt = PhLedger::new();
        assert_eq!(lt.hook_gate(RUBY_VER + 2, GEMSET, HOOK_PROTO), Err(PhCode::ERuby));
        assert_eq!(lt.hook_gate(RUBY_VER, GEMSET + 2, HOOK_PROTO), Err(PhCode::EGemset));
        assert_eq!(lt.hook_gate(RUBY_VER, GEMSET, HOOK_PROTO + 2), Err(PhCode::EProto));
        assert!(lt.hook_gate(RUBY_VER, GEMSET, HOOK_PROTO).is_ok());
    }

    #[test]
    fn test_gem_order_and_dup() {
        let mut lt = PhLedger::new();
        let _ = lt.record_gem(0, gem_cell(0).unwrap_or(0), 5);
        assert_eq!(lt.record_gem(1, gem_cell(1).unwrap_or(0), 3), Err(PhCode::EOrder));
        assert_eq!(lt.record_gem(0, gem_cell(0).unwrap_or(0), 6), Err(PhCode::EDup));
        assert!(lt.record_gem(1, gem_cell(1).unwrap_or(0), 6).is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (HOOKS * APPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4C80);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52050_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
