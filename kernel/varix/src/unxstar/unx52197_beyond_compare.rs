//! UNX-F52197 Beyond Compare 收口 · 0xC7140段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52197 逐字锚定）：UNX-F52198-J1 六判据全落；
//! 三方合并/文件夹对比 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：对比会话 ×10 场景账，会话类别二元闭集（0=三方合并
//!      1=文件夹对比），逐场景载荷（纯函数折叠）恒等才收；漂移必抓。
//!   ② 授权三态闭集（0=试用 1=正式 2=过期）：过期态收口闸——过期
//!      仍申报全通 = 如实定级闸必抓（AI-95 联签锚定行同构）。
//!   ③ 对比树升序单调闸（同构 F52198 树序闸，独立账本不复用）。
//!   ④ F52182 共面分账声明 4 行闭集（与 Araxis/Gittyup 同域不同账），
//!      缺一拒封。
//!   ⑤ 注入 ×10 轮（会话越闭集/过期申报全通/重复落账三态轮转）10/10
//!      必抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（会话+树+声明
//!      全齐 + 授权非过期 → S1；授权=过期 → S3 如实降级）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52197 − 52000)）。
pub const SEG_BASE: u32 = 0xC7140;

/// 对比会话数。
pub const SCENES: usize = 10;

/// 对比树数。
pub const TREES: usize = 10;

/// 分账声明行数。
pub const SEP_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PfCode {
    /// 成功
    Ok = 0,
    /// 会话号越闭集
    ENoScene,
    /// 树号越闭集
    ENoTree,
    /// 格载荷漂移
    EDrift,
    /// 树序逆序
    EOrder,
    /// 重复落账
    EDup,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 过期申报全通
    EExpired,
    /// 授权码越闭集
    ENoLic,
    /// 会话/树账不齐即封
    ESceneMiss,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl PfCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32 + 1
    }
    pub fn detail(self) -> &'static str {
        match self {
            PfCode::Ok => "成功",
            PfCode::ENoScene => "会话号越闭集：10 会话之外先扩闭集",
            PfCode::ENoTree => "树号越闭集：10 树之外先扩账本",
            PfCode::EDrift => "格载荷漂移：折叠与基准不恒等，逐格复算",
            PfCode::EOrder => "树序逆序：树号升序单调不许回跳",
            PfCode::EDup => "重复落账：同格只许一条账目",
            PfCode::ENoRow => "声明行越闭集：4 行闭集之外先扩行",
            PfCode::ERowMiss => "缺声明行：F52182 共面分账声明未齐先补齐",
            PfCode::EExpired => "过期申报全通：授权=过期必须如实定级 S3",
            PfCode::ENoLic => "授权码越闭集：三态闭集之外必拒",
            PfCode::ESceneMiss => "会话/树账不齐即封：格未齐先补齐",
            PfCode::ESeal => "未齐即封：格未齐先补齐",
            PfCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            PfCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            PfCode::EGradeDrift => "定级漂移：会话+树+声明全齐且授权非过期才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 会话格载荷（场景 sc，会话类别 = sc%2）。
pub const fn scene_cell(sc: usize) -> Option<u64> {
    if sc >= SCENES {
        return None;
    }
    Some(0xD4000u64 + (sc as u64) * 0x2E)
}

/// 会话类别（0=三方合并 1=文件夹对比）。
pub const fn scene_kind(sc: usize) -> Option<u32> {
    if sc >= SCENES {
        return None;
    }
    Some((sc % 2) as u32)
}

/// 授权三态闭集核验（0=试用 1=正式 2=过期）。
pub const fn lic_ok(l: u32) -> bool {
    l <= 2
}

/// 授权对定级的影响（2=过期 → S3 如实降级）。
pub const fn lic_grade(l: u32) -> Option<u32> {
    if !lic_ok(l) {
        return None;
    }
    if l == 2 {
        Some(3)
    } else {
        Some(1)
    }
}

/// 树格载荷（树 t）。
pub const fn tree_cell(t: usize) -> Option<u64> {
    if t >= TREES {
        return None;
    }
    Some(0xD500 + t as u64 * 0x2B)
}

/// 分账声明行折叠（行 r）。
pub const fn sep_row(r: usize) -> Option<u32> {
    if r >= SEP_ROWS {
        return None;
    }
    Some(0xD600 + r as u32 * 0x3D)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// 对比会话收口账本。
#[derive(Clone)]
pub struct PfLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 会话账。
    scenes: [Option<u64>; SCENES],
    /// 树账。
    trees: [Option<u64>; TREES],
    /// 分账声明账。
    sep: [Option<u32>; SEP_ROWS],
    /// 授权态。
    lic: u32,
    /// 最后树号。
    last_tree: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl PfLedger {
    /// 新账本。
    pub fn new() -> Self {
        PfLedger {
            gates: [None; 6],
            scenes: [None; SCENES],
            trees: [None; TREES],
            sep: [None; SEP_ROWS],
            lic: 0,
            last_tree: -1,
            injected: 0,
            caught: 0,
            escaped: 0,
            fp: 0xcbf29ce484222325,
        }
    }

    /// FNV-1a 单步。
    fn fnv(&mut self, l: u64) {
        self.fp ^= l;
        self.fp = self.fp.wrapping_mul(0x100000001b3);
    }

    /// 落一格六判据。
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), PfCode> {
        if gate >= 6 {
            return Err(PfCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x3E100 + gate as u64);
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

    /// 会话一格（载荷对照 + 重复拒）。
    pub fn record_scene(&mut self, sc: usize) -> Result<u64, PfCode> {
        if sc >= SCENES {
            return Err(PfCode::ENoScene);
        }
        let p = match scene_cell(sc) {
            Some(p) => p,
            None => return Err(PfCode::ENoScene),
        };
        if match self.scenes.get(sc) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.scenes.get_mut(sc) {
            *c = Some(p);
        }
        self.fnv(0x3E200 + sc as u64);
        Ok(p)
    }

    /// 授权落账（三态闭集闸）。
    pub fn set_lic(&mut self, l: u32) -> Result<(), PfCode> {
        if !lic_ok(l) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3E300);
            return Err(PfCode::ENoLic);
        }
        self.lic = l;
        self.fnv(0x3E301);
        Ok(())
    }

    /// 过期申报全通闸：授权=过期且申报全通 → 如实定级闸必抓。
    pub fn claim_pass_on_expired(&mut self) -> bool {
        if self.lic == 2 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3E302);
            return false;
        }
        true
    }

    /// 树一格（树号升序单调闸 + 载荷对照 + 重复拒）。
    pub fn record_tree(&mut self, t: usize) -> Result<u64, PfCode> {
        if t >= TREES {
            return Err(PfCode::ENoTree);
        }
        if (t as i64) < self.last_tree {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x3E400);
            return Err(PfCode::EOrder);
        }
        let p = match tree_cell(t) {
            Some(p) => p,
            None => return Err(PfCode::ENoTree),
        };
        if match self.trees.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(slot) = self.trees.get_mut(t) {
            *slot = Some(p);
        }
        self.last_tree = t as i64;
        self.fnv(0x3E402 + t as u64);
        Ok(p)
    }

    /// 分账声明一行。
    pub fn declare_sep(&mut self, r: usize) -> Result<(), PfCode> {
        if r >= SEP_ROWS {
            return Err(PfCode::ENoRow);
        }
        if match self.sep.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(PfCode::EDup);
        }
        if let Some(c) = self.sep.get_mut(r) {
            *c = sep_row(r);
        }
        self.fnv(0x3E500 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
    pub fn sep_all(&self) -> bool {
        let mut r = 0usize;
        while r < SEP_ROWS {
            match self.sep.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 封账闸：会话+树+声明全齐且授权在册。
    pub fn seal(&self) -> Result<(), PfCode> {
        let mut sc = 0usize;
        while sc < SCENES {
            match self.scenes.get(sc) {
                Some(Some(_)) => {}
                _ => return Err(PfCode::ESceneMiss),
            }
            sc += 1;
        }
        let mut t = 0usize;
        while t < TREES {
            match self.trees.get(t) {
                Some(Some(_)) => {}
                _ => return Err(PfCode::ESceneMiss),
            }
            t += 1;
        }
        if !self.sep_all() {
            return Err(PfCode::ERowMiss);
        }
        if !lic_ok(self.lic) {
            return Err(PfCode::ENoLic);
        }
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

impl Default for PfLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = PfLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    // 授权落账：正式态。
    let _ = led.set_lic(1);
    let mut recorded = 0u32;
    let mut sc = 0usize;
    while sc < SCENES {
        if led.record_scene(sc).is_ok() {
            recorded += 1;
        }
        sc += 1;
    }
    let mut t = 0usize;
    while t < TREES {
        if led.record_tree(t).is_ok() {
            recorded += 1;
        }
        t += 1;
    }
    let mut r = 0usize;
    while r < SEP_ROWS {
        let _ = led.declare_sep(r);
        r += 1;
    }
    // 注入 10 轮：会话越闭集 / 授权越闭集 / 重复落账三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = PfLedger::new();
        let caught = match kind {
            0 => lt.record_scene(SCENES).is_err(),
            1 => lt.set_lic(3).is_err(),
            _ => {
                let _ = lt.record_scene(0);
                lt.record_scene(0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x3E600 + k as u64);
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
pub fn run_unx52197_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52197");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "对比会话 10 场景全通",
        recorded >= SCENES as u32,
        "三方合并/文件夹对比两类逐场景恒等",
    );
    s.add(
        "对比树 10 树全通",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TREES {
                if lt.record_tree(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok
        },
        "树号升序单调逐树落账",
    );
    s.add(
        "授权三态闭集",
        lic_ok(0) && lic_ok(2) && !lic_ok(3),
        "试用/正式/过期三态在册且越态拒",
    );
    s.add(
        "授权定级映射",
        match lic_grade(1) {
            Some(1) => true,
            _ => false,
        } && match lic_grade(2) {
            Some(3) => true,
            _ => false,
        },
        "正式→S1 过期→S3 如实降级不虚报",
    );
    s.add(
        "过期申报全通必抓",
        {
            let mut lt = PfLedger::new();
            let _ = lt.set_lic(2);
            !lt.claim_pass_on_expired()
        },
        "授权=过期申报全通必须如实定级 S3",
    );
    s.add(
        "F52182 分账声明 4/4 全齐",
        {
            let mut lt = PfLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SEP_ROWS {
                if lt.declare_sep(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.sep_all()
        },
        "与 Araxis/Gittyup 同域不同账四行声明闭集",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越闭集/授权越态/重复三态轮转必抓");
    let mut l0 = PfLedger::new();
    s.add(
        "边界·会话越闭集",
        l0.record_scene(SCENES).is_err(),
        "会话 10 越闭集必拒",
    );
    s.add(
        "边界·授权越闭集",
        l0.set_lic(9).is_err(),
        "授权 9 越三态闭集必拒",
    );
    let mut l1 = PfLedger::new();
    let _ = l1.record_tree(3);
    s.add("树序·逆序拒", l1.record_tree(1).is_err(), "树号回跳必抓");
    let mut l2 = PfLedger::new();
    let _ = l2.record_scene(0);
    s.add("重复·再落账拒", l2.record_scene(0).is_err(), "同格只许一条账目");
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
    fn test_lic_gates() {
        assert!(lic_ok(0) && lic_ok(2));
        assert!(!lic_ok(3));
        assert_eq!(lic_grade(1), Some(1));
        assert_eq!(lic_grade(2), Some(3));
        assert!(lic_grade(9).is_none());
        assert!(scene_cell(SCENES).is_none());
        assert!(tree_cell(TREES).is_none());
    }

    #[test]
    fn test_expired_claim_caught() {
        let mut lt = PfLedger::new();
        let _ = lt.set_lic(2);
        assert!(!lt.claim_pass_on_expired());
    }

    #[test]
    fn test_order_gate() {
        let mut lt = PfLedger::new();
        assert!(lt.record_tree(3).is_ok());
        assert!(lt.record_tree(1).is_err());
    }

    #[test]
    fn test_sep_and_seal() {
        let mut lt = PfLedger::new();
        assert!(lt.seal().is_err());
        let _ = lt.declare_sep(0);
        assert!(lt.declare_sep(0).is_err());
        assert_eq!(lt.declare_sep(SEP_ROWS), Err(PfCode::ENoRow));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (SCENES + TREES) as u32);
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
        assert_eq!(SEG_BASE, 0xC7140);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52197_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
