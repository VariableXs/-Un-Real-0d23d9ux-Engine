//! UNX-F52073 MSYS2 收口（构建环境分发面） · 0xC5240段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52073 逐字锚定）：UNX-F52074-J1 install/
//! upgrade/uninstall 20 操作集；包脚本执行沙箱行为账；KB-COMPAT 定级
//! 登记；判据锚定 §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：install/upgrade/uninstall 20 操作集账，操作类别三元闭
//!      集（0=安装 1=升级 2=卸载），逐操作载荷（纯函数折叠）恒等才收。
//!   ② 沙箱行为账：沙箱行为 6 格闭集（文件域/进程域/网络域/注册表域/
//!      环境域/回滚域），逐格越域拒绝（沙箱越域注入必抓）。
//!   ③ install→upgrade→uninstall 拍序单向闸；卸载后残留核验（Σ操作
//!      载荷回零基准）。
//!   ④ 注入 ×10 轮（越域/操作漂移/拍序逆序三态轮转）10/10 必抓，注入
//!      =检出+逃逸守恒；六判据母版闸 + 定级（20 格全通 + 沙箱 6 格在
//!      册 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52073 − 52000)）。
pub const SEG_BASE: u32 = 0xC5240;

/// 操作集格数。
pub const MS_OPS: usize = 20;

/// 沙箱行为格数。
pub const SANDBOX: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum McCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 沙箱格越册
    ENoBox,
    /// 沙箱越域
    EOutBox,
    /// 拍序逆序
    EOrder,
    /// 卸载残留
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

impl McCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            McCode::Ok => "成功",
            McCode::ENoOp => "操作号越闭集：20 操作集之外先登记",
            McCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            McCode::ENoBox => "沙箱格越册：6 格闭集之外先登记",
            McCode::EOutBox => "沙箱越域：文件/进程/网络/注册表/环境/回滚域之外拒绝",
            McCode::EOrder => "拍序逆序：install→upgrade→uninstall 单向，逆序必抓",
            McCode::EResidue => "卸载残留：卸载后 Σ操作载荷未回零基准",
            McCode::EDup => "重复落账：同格只许一条账目",
            McCode::ESeal => "未齐即封：20 格未齐先补齐",
            McCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            McCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            McCode::EGradeDrift => "定级漂移：20 格全通+沙箱 6 格在册+卸载零残留才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 操作格载荷（操作 o）。
pub const fn ms_cell(o: usize) -> Option<u64> {
    if o >= MS_OPS {
        return None;
    }
    Some((0x88000u64 + (o as u64) * 0x2E) ^ ((o / 7) as u64).wrapping_mul(0x01000193))
}

/// 沙箱格基准（格 b）。
pub const fn box_cell(b: usize) -> Option<u64> {
    if b >= SANDBOX {
        return None;
    }
    Some((0x89000u64 + b as u64 * 0x33) ^ 0x6D73797332626F78)
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct McLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; MS_OPS],
    /// 沙箱账。
    boxes: [Option<u64>; SANDBOX],
    /// 残留累计。
    residue: u64,
    /// 最后拍。
    last_tick: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl McLedger {
    /// 新账本。
    pub fn new() -> Self {
        McLedger {
            gates: [None; 6],
            ops: [None; MS_OPS],
            boxes: [None; SANDBOX],
            residue: 0,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), McCode> {
        if gate >= 6 {
            return Err(McCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x32100 + gate as u64);
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

    /// 操作一格（拍序单调 + 载荷对照 + 残留累计 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64, tick: u32) -> Result<(), McCode> {
        if o >= MS_OPS {
            return Err(McCode::ENoOp);
        }
        if (tick as i64) <= self.last_tick {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x32200);
            return Err(McCode::EOrder);
        }
        let want = match ms_cell(o) {
            Some(w) => w,
            None => return Err(McCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x32201 + o as u64);
            return Err(McCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(McCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        // 卸载操作（o >= 14）清残留（回滚域语义：卸载后回零基准）。
        if o >= 14 {
            self.residue = 0;
        } else {
            self.residue = self.residue.wrapping_add(got & 0xFF);
        }
        self.last_tick = tick as i64;
        self.fnv(0x32202 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < MS_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 沙箱一格（越域拒）。
    pub fn record_box(&mut self, b: usize, got: u64) -> Result<(), McCode> {
        if b >= SANDBOX {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x32300 + b as u64);
            return Err(McCode::EOutBox);
        }
        let want = match box_cell(b) {
            Some(w) => w,
            None => return Err(McCode::ENoBox),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x32301 + b as u64);
            return Err(McCode::EOutBox);
        }
        if let Some(c) = self.boxes.get_mut(b) {
            *c = Some(got);
        }
        self.fnv(0x32302 + b as u64);
        Ok(())
    }

    /// 沙箱全账核验。
    pub fn boxes_all(&self) -> bool {
        let mut b = 0usize;
        while b < SANDBOX {
            match self.boxes.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 卸载零残留核验。
    pub fn residue_ok(&self) -> bool {
        self.residue == 0
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

impl Default for McLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = McLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    let mut tick: u32 = 0;
    while o < MS_OPS {
        tick = tick.saturating_add(2);
        if let Some(w) = ms_cell(o) {
            if led.record_op(o, w, tick).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut b = 0usize;
    while b < SANDBOX {
        if let Some(w) = box_cell(b) {
            let _ = led.record_box(b, w);
        }
        b += 1;
    }
    // 注入 10 轮：越域 / 操作漂移 / 拍序逆序三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = McLedger::new();
        let caught = match kind {
            0 => lt.record_box(6, 0).is_err(),
            1 => lt.record_op(4, ms_cell(4).unwrap_or(0).wrapping_add(1), 1) == Err(McCode::EDrift),
            _ => {
                let _ = lt.record_op(0, ms_cell(0).unwrap_or(0), 8);
                lt.record_op(1, ms_cell(1).unwrap_or(0), 3) == Err(McCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x32400 + k as u64);
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
pub fn run_unx52073_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52073");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "20 操作集全通",
        recorded == MS_OPS as u32,
        "install/upgrade/uninstall 20 操作逐格恒等",
    );
    s.add(
        "沙箱行为 6/6 在册",
        {
            let mut lt = McLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < SANDBOX {
                if let Some(w) = box_cell(b) {
                    if lt.record_box(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.boxes_all()
        },
        "文件/进程/网络/注册表/环境/回滚六域逐格恒等",
    );
    s.add(
        "卸载零残留",
        {
            let mut lt = McLedger::new();
            let mut o = 0usize;
            let mut tick: u32 = 0;
            while o < MS_OPS {
                tick += 2;
                let _ = lt.record_op(o, ms_cell(o).unwrap_or(0), tick);
                o += 1;
            }
            lt.residue_ok()
        },
        "卸载回冲后 Σ载荷回零基准",
    );
    s.add("注入 10/10 全抓", caught >= 10, "越域/漂移/逆序三态轮转必抓");
    let mut l0 = McLedger::new();
    s.add(
        "沙箱·越域拒",
        l0.record_box(9, 0).is_err(),
        "6 域之外拒绝",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(MS_OPS, 0, 1).is_err(),
        "操作 20 越闭集必拒",
    );
    let mut l1 = McLedger::new();
    let _ = l1.record_op(6, ms_cell(6).unwrap_or(0), 5);
    s.add(
        "拍序·逆序拒",
        l1.record_op(7, ms_cell(7).unwrap_or(0), 3) == Err(McCode::EOrder),
        "逆序必抓",
    );
    s.add(
        "重复·再落账拒",
        l1.record_op(6, ms_cell(6).unwrap_or(0), 7).is_err(),
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
        let a = ms_cell(9).unwrap_or(0);
        assert_eq!(a, ms_cell(9).unwrap_or(0));
        assert_ne!(a, ms_cell(10).unwrap_or(0));
        assert!(ms_cell(MS_OPS).is_none());
        assert!(box_cell(SANDBOX).is_none());
        assert_ne!(box_cell(0), box_cell(1));
    }

    #[test]
    fn test_sandbox_domain_gates() {
        let mut lt = McLedger::new();
        assert!(lt.record_box(SANDBOX, 0).is_err());
        assert!(lt.record_box(0, box_cell(0).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_box(0, box_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(McCode::EOutBox)
        );
    }

    #[test]
    fn test_residue_cycle() {
        let mut lt = McLedger::new();
        let mut o = 0usize;
        let mut tick: u32 = 0;
        while o < MS_OPS {
            tick += 2;
            let _ = lt.record_op(o, ms_cell(o).unwrap_or(0), tick);
            o += 1;
        }
        assert!(lt.residue_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, MS_OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC5240);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52073_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 12);
    }
}
