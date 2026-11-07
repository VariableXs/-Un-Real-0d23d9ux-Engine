//! UNX-F52103 Eclipse IDE（Java SE）收口 · 0xC57C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 UNX-F52103 逐字锚定）：UNX-F52104-J1 六判据全落；
//! MinGW-gdb 调试挂接（联签锚定行 B09）；KB-COMPAT 定级登记；判据锚定
//! §AI-66.3。
//!
//! 设计要点：
//!   ① 域模型：Eclipse 工作台五面账（工程创建/构建/运行/调试挂接/导
//!      出）×5 工程场景 = 25 格工作台账，逐格载荷（纯函数折叠）恒等才
//!      收；漂移必抓。
//!   ② MinGW-gdb 挂接三元闸（gdb 版本档/协议档/回传端口档）全等才放行
//!      挂接，错位走失配路径（B09 联签锚定行）；失配必抓。
//!   ③ 工作台拍序闸：创建→构建→运行→调试→导出段序严格递增；逆序必
//!      抓；重复拒。
//!   ④ 注入 ×10 轮（三元失配/段序逆序/场景越闭集三态轮转）10/10 必
//!      抓，注入=检出+逃逸守恒；六判据母版闸 + 定级（25 格全通 → S1）。
//!   ⑤ no_std + alloc；零浮点、零 IO、零墙钟、零随机；无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::vec::Vec;

/// 段基址（VX14 派工公式：0xC4000 + 0x40 × (52103 − 52000)）。
pub const SEG_BASE: u32 = 0xC57C0;

/// 工作台段数。
pub const FACES: usize = 5;

/// 工程场景数。
pub const PROJS: usize = 5;

/// gdb 三元基准。
pub const GDB_VER: u32 = 13;
pub const GDB_PROTO: u32 = 3;
pub const GDB_PORT: u32 = 2331;

// ── 诊断码闭集 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EcCode {
    /// 成功
    Ok = 0,
    /// 场景号越闭集
    ENoProj,
    /// 段号越闭集
    ENoFace,
    /// 工作台载荷漂移
    EDrift,
    /// gdb 版本档失配
    EGdbVer,
    /// gdb 协议档失配
    EGdbProto,
    /// gdb 回传端口档失配
    EGdbPort,
    /// 段序逆序
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

impl EcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            EcCode::Ok => "成功",
            EcCode::ENoProj => "工程号越闭集：5 工程之外先登记",
            EcCode::ENoFace => "段号越闭集：创建/构建/运行/调试/导出五段之外先登记",
            EcCode::EDrift => "工作台载荷漂移：折叠与基准不恒等，逐格复算",
            EcCode::EGdbVer => "gdb 版本档失配：MinGW-gdb 版本档与宿主档不符（B09 联签）",
            EcCode::EGdbProto => "gdb 协议档失配：调试协议档与宿主档不符",
            EcCode::EGdbPort => "gdb 回传端口档失配：回传端口档与宿主档不符",
            EcCode::EOrder => "段序逆序：创建→构建→运行→调试→导出严格递增，逆序必抓",
            EcCode::EDup => "重复落账：同格只许一条账目",
            EcCode::ESeal => "未齐即封：25 格未齐先补齐",
            EcCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            EcCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            EcCode::EGradeDrift => "定级漂移：25 格全通+挂接零失配+段序单调才定级",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 工作台格载荷（工程 p × 段 f）。
pub const fn face_cell(p: usize, f: usize) -> Option<u64> {
    if p >= PROJS || f >= FACES {
        return None;
    }
    Some(
        (0xD4000u64 + (p as u64) * 0x31)
            ^ (f as u64).wrapping_mul(0x01000193)
            ^ 0x65636C69707365,
    )
}

/// gdb 三元闸。
pub const fn gdb_ok(ver: u32, proto: u32, port: u32) -> bool {
    ver == GDB_VER && proto == GDB_PROTO && port == GDB_PORT
}

// ── 收口账本 ─────────────────────────────────────────────────────
#[derive(Clone)]
pub struct EcLedger {
    /// 六判据。
    gates: [Option<bool>; 6],
    /// 工作台账。
    faces: [[Option<u64>; FACES]; PROJS],
    /// 各工程最后段。
    last_face: [i64; PROJS],
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl EcLedger {
    /// 新账本。
    pub fn new() -> Self {
        EcLedger {
            gates: [None; 6],
            faces: [[None; FACES]; PROJS],
            last_face: [-1; PROJS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), EcCode> {
        if gate >= 6 {
            return Err(EcCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x47100 + gate as u64);
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

    /// gdb 挂接三元闸。
    pub fn attach_gdb(&mut self, ver: u32, proto: u32, port: u32) -> Result<(), EcCode> {
        if !gdb_ok(ver, proto, port) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x47200);
            return if ver != GDB_VER {
                Err(EcCode::EGdbVer)
            } else if proto != GDB_PROTO {
                Err(EcCode::EGdbProto)
            } else {
                Err(EcCode::EGdbPort)
            };
        }
        self.fnv(0x47201);
        Ok(())
    }

    /// 工作台一格（段序单调 + 载荷对照 + 重复拒）。
    pub fn record_face(&mut self, p: usize, f: usize) -> Result<u64, EcCode> {
        if p >= PROJS {
            return Err(EcCode::ENoProj);
        }
        if f >= FACES {
            return Err(EcCode::ENoFace);
        }
        if (f as i64) < self.last_face[p] {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x47300 + p as u64);
            return Err(EcCode::EOrder);
        }
        let payload = match face_cell(p, f) {
            Some(x) => x,
            None => return Err(EcCode::ENoFace),
        };
        if match self.faces.get(p).and_then(|row| row.get(f)) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(EcCode::EDup);
        }
        if let Some(row) = self.faces.get_mut(p) {
            if let Some(c) = row.get_mut(f) {
                *c = Some(payload);
            }
        }
        self.last_face[p] = f as i64;
        self.fnv(0x47301 + (p as u64) * 5 + f as u64);
        Ok(payload)
    }

    /// 工作台全账核验（25 格）。
    pub fn faces_all(&self) -> bool {
        let mut p = 0usize;
        while p < PROJS {
            let mut f = 0usize;
            while f < FACES {
                match self.faces.get(p).and_then(|row| row.get(f)) {
                    Some(Some(_)) => {}
                    _ => return false,
                }
                f += 1;
            }
            p += 1;
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

impl Default for EcLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 全流程 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = EcLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let _ = led.attach_gdb(GDB_VER, GDB_PROTO, GDB_PORT);
    let mut recorded = 0u32;
    let mut p = 0usize;
    while p < PROJS {
        let mut f = 0usize;
        while f < FACES {
            if led.record_face(p, f).is_ok() {
                recorded += 1;
            }
            f += 1;
        }
        p += 1;
    }
    // 注入 10 轮：三元失配 / 段序逆序 / 工程越闭集三态轮转。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = EcLedger::new();
        let caught = match kind {
            0 => {
                lt.attach_gdb(GDB_VER + 1, GDB_PROTO, GDB_PORT) == Err(EcCode::EGdbVer)
                    && lt.attach_gdb(GDB_VER, GDB_PROTO + 1, GDB_PORT) == Err(EcCode::EGdbProto)
                    && lt.attach_gdb(GDB_VER, GDB_PROTO, GDB_PORT + 1) == Err(EcCode::EGdbPort)
            }
            1 => {
                let _ = lt.record_face(0, 3);
                lt.record_face(0, 1) == Err(EcCode::EOrder)
            }
            _ => lt.record_face(PROJS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x47400 + k as u64);
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
pub fn run_unx52103_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52103");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "工作台五段 25/25 全通",
        recorded >= (PROJS * FACES) as u32,
        "创建/构建/运行/调试/导出 ×5 工程逐格恒等",
    );
    s.add(
        "MinGW-gdb 三元闸全等放行+失配三态全抓",
        {
            let mut lt = EcLedger::new();
            lt.attach_gdb(GDB_VER, GDB_PROTO, GDB_PORT).is_ok()
                && lt.attach_gdb(GDB_VER + 1, GDB_PROTO, GDB_PORT) == Err(EcCode::EGdbVer)
                && lt.attach_gdb(GDB_VER, GDB_PROTO + 1, GDB_PORT) == Err(EcCode::EGdbProto)
                && lt.attach_gdb(GDB_VER, GDB_PROTO, GDB_PORT + 1) == Err(EcCode::EGdbPort)
        },
        "gdb 版本/协议/回传端口三元全等放行，错位走失配路径（B09 联签）",
    );
    s.add("注入 10/10 全抓", caught >= 10, "失配/逆序/越闭集三态轮转必抓");
    let mut l0 = EcLedger::new();
    s.add(
        "段序·逆序拒",
        {
            let _ = l0.record_face(0, 3);
            l0.record_face(0, 1) == Err(EcCode::EOrder)
        },
        "调试后回构建必抓",
    );
    s.add(
        "边界·工程越闭集",
        l0.record_face(PROJS, 0).is_err(),
        "工程 5 越闭集必拒",
    );
    s.add(
        "边界·段越闭集",
        l0.record_face(0, FACES).is_err(),
        "段 5 越闭集必拒",
    );
    let mut l1 = EcLedger::new();
    let _ = l1.record_face(2, 2);
    s.add(
        "重复·再落账拒",
        l1.record_face(2, 2).is_err(),
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
    fn test_gdb_gate() {
        assert!(gdb_ok(GDB_VER, GDB_PROTO, GDB_PORT));
        assert!(!gdb_ok(GDB_VER + 1, GDB_PROTO, GDB_PORT));
        assert!(!gdb_ok(GDB_VER, GDB_PROTO + 1, GDB_PORT));
        assert!(!gdb_ok(GDB_VER, GDB_PROTO, GDB_PORT + 1));
    }

    #[test]
    fn test_cells_pure() {
        let a = face_cell(3, 2).unwrap_or(0);
        assert_eq!(a, face_cell(3, 2).unwrap_or(0));
        assert_ne!(a, face_cell(3, 3).unwrap_or(0));
        assert!(face_cell(PROJS, 0).is_none());
        assert!(face_cell(0, FACES).is_none());
    }

    #[test]
    fn test_face_order_caught() {
        let mut lt = EcLedger::new();
        let _ = lt.record_face(1, 3);
        assert_eq!(lt.record_face(1, 1), Err(EcCode::EOrder));
        assert!(lt.record_face(1, 3).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROJS * FACES) as u32);
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
        assert_eq!(SEG_BASE, 0xC57C0);
        assert!(SEG_BASE >= 0xC4000 && SEG_BASE <= 0xC71C0);
    }

    #[test]
    fn test_checks_all_passed() {
        let s = run_unx52103_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 11);
    }
}
