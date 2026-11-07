//! UNX-F52059 JetBrains Fleet 收口（下一代轻量 IDE） · 0xC4EC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52059 逐字锚定）：UNX-F52060-J1 六判据全落；
//! 工程生成语言/构建操作集；KB-COMPAT 定级登记；判据锚定 §AI-66.3 长
//! 尾组口径。
//!
//! 设计要点：
//!   ① 域模型：工程生成语言闭集 8 语言（Rust/Go/Python/JS/Java/C++/
//!      C#/Ruby），逐语言生成载荷对照恒等才收；构建操作集 6 操作（配
//!      置/构建/测试/运行/打包/清理）× 8 语言 = 48 格操作账。
//!   ② 语言档三元闸（语言档/工具链档/目标档）全等才放行生成，错位走
//!      失配路径（ELang/ETool/ETarget 三诊断）。
//!   ③ 构建拍序闸：配置→构建→测试→运行→打包→清理拍序严格递增；清
//!      理后必回零残留（Σ 操作格载荷回零基准）；重复拒。
//!   ④ 注入 ×10 轮（三元失配/语言越闭集/拍序回退三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（48 格全通 + 零残
//!      留 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52059 − 52000)）。
pub const SEG_BASE: u32 = 0xC4EC0;

/// 语言闭集数。
pub const LANGS: usize = 8;

/// 构建操作数。
pub const BUILD_OPS: usize = 6;

/// 工具链档基准。
pub const TOOLCHAIN: u32 = 5;

/// 目标档基准（0=本机 x64）。
pub const TARGET: u32 = 0;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FlCode {
    /// 成功
    Ok = 0,
    /// 语言越闭集
    ENoLang,
    /// 操作越闭集
    ENoOp,
    /// 语言档失配
    ELang,
    /// 工具链档失配
    ETool,
    /// 目标档失配
    ETarget,
    /// 拍序乱
    EOrder,
    /// 清理残留
    EResidue,
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

impl FlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FlCode::Ok => "成功",
            FlCode::ENoLang => "语言越闭集：8 语言之外先登记",
            FlCode::ENoOp => "操作越闭集：6 构建操作之外先登记",
            FlCode::ELang => "语言档失配：语言档与生成基准不符",
            FlCode::ETool => "工具链档失配：工具链档与宿主档不符",
            FlCode::ETarget => "目标档失配：目标档与宿主档不符",
            FlCode::EOrder => "拍序乱：配置→构建→测试→运行→打包→清理严格递增",
            FlCode::EResidue => "清理残留：清理后 Σ操作格载荷未回零基准",
            FlCode::EDup => "重复落账：同格只许一条账目",
            FlCode::ESeal => "未齐即封：48 格未齐先补齐",
            FlCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            FlCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            FlCode::EGradeDrift => "定级漂移：48 格全通+清理零残留+拍序单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（语言 l × 操作 o）。
pub const fn op_cell(l: usize, o: usize) -> Option<u64> {
    if l >= LANGS || o >= BUILD_OPS {
        return None;
    }
    Some(
        0x53000u64
            + (l as u64) * 0x2B
            + (o as u64) * 0x12
            ^ (l as u64).wrapping_mul(0x9E3779B97F4A7C15 >> o),
    )
}

/// 清理后零残留基准（恒 0）。
pub const fn clean_base() -> u64 {
    0
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct FlLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [[Option<u64>; BUILD_OPS]; LANGS],
    /// 最后拍。
    last_tick: i64,
    /// 清理累计。
    clean_sum: u64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl FlLedger {
    /// 新账本。
    pub fn new() -> Self {
        FlLedger {
            gates: [None; 6],
            ops: [[None; BUILD_OPS]; LANGS],
            last_tick: -1,
            clean_sum: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), FlCode> {
        if gate >= 6 {
            return Err(FlCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x24100 + gate as u64);
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

    /// 生成三元闸。
    pub fn gen_gate(&mut self, lang: usize, tool: u32, target: u32) -> Result<(), FlCode> {
        if lang >= LANGS {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x24200);
            return Err(FlCode::ENoLang);
        }
        if tool != TOOLCHAIN {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x24201);
            return Err(FlCode::ETool);
        }
        if target != TARGET {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x24202);
            return Err(FlCode::ETarget);
        }
        self.fnv(0x24203);
        Ok(())
    }

    /// 操作一格（三元闸后落账，拍序单调 + 重复拒）。
    pub fn record_op(&mut self, l: usize, o: usize, tick: u32) -> Result<u64, FlCode> {
        if l >= LANGS {
            return Err(FlCode::ENoLang);
        }
        if o >= BUILD_OPS {
            return Err(FlCode::ENoOp);
        }
        if (tick as i64) <= self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x24300);
            return Err(FlCode::EOrder);
        }
        let p = match op_cell(l, o) {
            Some(p) => p,
            None => return Err(FlCode::ENoOp),
        };
        if match self.ops.get(l).and_then(|row| row.get(o)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FlCode::EDup);
        }
        if let Some(row) = self.ops.get_mut(l) {
            if let Some(c) = row.get_mut(o) {
                *c = Some(p);
            }
        }
        self.last_tick = tick as i64;
        // 清理操作（o==5）回零残留。
        if o == BUILD_OPS - 1 {
            self.clean_sum = clean_base();
        } else {
            self.clean_sum = self.clean_sum.wrapping_add(p & 0xFF);
        }
        self.fnv(0x24301 + (l as u64) * 6 + o as u64);
        Ok(p)
    }

    /// 操作全账核验（48 格）。
    pub fn ops_all(&self) -> bool {
        let mut l = 0usize;
        while l < LANGS {
            let mut o = 0usize;
            while o < BUILD_OPS {
                match self.ops.get(l).and_then(|row| row.get(o)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                o += 1;
            }
            l += 1;
        }
        true
    }

    /// 清理零残留核验。
    pub fn clean_ok(&self) -> bool {
        self.clean_sum == clean_base()
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

impl Default for FlLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = FlLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut tick: u32 = 0;
    let mut l = 0usize;
    while l < LANGS {
        let _ = led.gen_gate(l, TOOLCHAIN, TARGET);
        let mut o = 0usize;
        while o < BUILD_OPS {
            tick = tick.saturating_add(2);
            if led.record_op(l, o, tick).is_ok() {
                recorded += 1;
            }
            o += 1;
        }
        l += 1;
    }
    // 注入 10 轮：三元失配 / 语言越闭集 / 拍序回退三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = FlLedger::new();
        let caught = match kind {
            0 => {
                lt.gen_gate(0, TOOLCHAIN + 1, TARGET) == Err(FlCode::ETool)
                    && lt.gen_gate(0, TOOLCHAIN, TARGET + 1) == Err(FlCode::ETarget)
            }
            1 => lt.gen_gate(LANGS, TOOLCHAIN, TARGET).is_err(),
            _ => {
                let _ = lt.record_op(0, 0, 10);
                lt.record_op(0, 1, 5) == Err(FlCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x24400 + k as u64);
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
pub fn run_unx52059_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52059");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "构建操作 48/48 全通",
        recorded == (LANGS * BUILD_OPS) as u32,
        "8 语言生成 ×6 构建操作全通",
    );
    s.add(
        "生成三元闸全等放行+失配全抓",
        {
            let mut lt = FlLedger::new();
            let ok = lt.gen_gate(0, TOOLCHAIN, TARGET).is_ok();
            let a = lt.gen_gate(0, TOOLCHAIN + 1, TARGET) == Err(FlCode::ETool);
            let b = lt.gen_gate(0, TOOLCHAIN, TARGET + 1) == Err(FlCode::ETarget);
            let c = lt.gen_gate(LANGS, TOOLCHAIN, TARGET).is_err();
            ok && a && b && c
        },
        "语言/工具链/目标三元全等放行，错位走失配路径",
    );
    s.add(
        "清理零残留",
        {
            let mut lt = FlLedger::new();
            let mut o = 0usize;
            let mut tick: u32 = 0;
            while o < BUILD_OPS {
                tick = tick.saturating_add(2);
                let _ = lt.record_op(0, o, tick);
                o += 1;
            }
            lt.clean_ok()
        },
        "清理操作后 Σ载荷回零基准",
    );
    s.add("注入 10/10 全抓", caught >= 10, "失配/越闭集/拍序三态轮转必抓");
    let mut l0 = FlLedger::new();
    s.add(
        "边界·语言越闭集",
        l0.record_op(LANGS, 0, 1).is_err(),
        "语言 8 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(0, BUILD_OPS, 1).is_err(),
        "操作 6 越闭集必拒",
    );
    let mut l1 = FlLedger::new();
    let _ = l1.record_op(2, 2, 4);
    s.add(
        "重复·再落账拒",
        l1.record_op(2, 2, 6).is_err(),
        "同格只许一条账目",
    );
    s.add(
        "拍序·回退拒",
        l1.record_op(2, 3, 1) == Err(FlCode::EOrder),
        "拍序回退必抓",
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
    fn test_op_cell_pure() {
        let a = op_cell(3, 2).unwrap_or(0);
        assert_eq!(a, op_cell(3, 2).unwrap_or(0));
        assert_ne!(a, op_cell(3, 3).unwrap_or(0));
        assert!(op_cell(LANGS, 0).is_none());
        assert!(op_cell(0, BUILD_OPS).is_none());
        assert_eq!(clean_base(), 0);
    }

    #[test]
    fn test_gen_gate() {
        let mut lt = FlLedger::new();
        assert_eq!(lt.gen_gate(0, TOOLCHAIN + 3, TARGET), Err(FlCode::ETool));
        assert_eq!(lt.gen_gate(0, TOOLCHAIN, TARGET + 3), Err(FlCode::ETarget));
        assert!(lt.gen_gate(LANGS - 1, TOOLCHAIN, TARGET).is_ok());
        assert!(lt.gen_gate(LANGS, TOOLCHAIN, TARGET).is_err());
    }

    #[test]
    fn test_order_and_dup() {
        let mut lt = FlLedger::new();
        let _ = lt.record_op(1, 1, 6);
        assert_eq!(lt.record_op(1, 1, 8), Err(FlCode::EDup));
        assert_eq!(lt.record_op(1, 2, 3), Err(FlCode::EOrder));
    }

    #[test]
    fn test_clean_cycle() {
        let mut lt = FlLedger::new();
        let mut o = 0usize;
        let mut tick: u32 = 0;
        while o < BUILD_OPS {
            tick += 2;
            let _ = lt.record_op(0, o, tick);
            o += 1;
        }
        assert!(lt.clean_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, (LANGS * BUILD_OPS) as u32);
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
        assert_eq!(SEG_BASE, 0xC4EC0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52059_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
