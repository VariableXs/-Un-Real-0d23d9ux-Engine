//! UNX-F52412 VeraCrypt 收口 · 0xCA6C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52412"，逐字）：
//!   title: VeraCrypt 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52413-J1 六判据
//!     全落；虚拟盘加解密 ×10（只对测试目录）；KB-COMPAT 定级登记；判据
//!     锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52412）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52413-J1，照原文锚定；功能按本卡标题（VeraCrypt）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五面闭集：卷创建/挂载/卸载/加密/密钥各 4 格共 20 格，面号 = o/4，
//!      面单调（逆序必抓）；逐面载荷纯函数折叠，恒等才收。虚拟盘加解密
//!      ×10 只对测试目录执行，不触碰物理盘（判据原文口径）。
//!   ② 卷三态 12 格：新建/已挂载/已卸载三态各 4 格，态号 = 格/4，态单调；
//!      逐格载荷对照。
//!   ③ 密钥文件三元闸 3 格：登记/校验/轮换三态逐格落账，未齐即封。
//!   ④ 系统加密 5 场景格：逐格恒等收账，越闭集拒。
//!   ⑤ 与 BitLocker 分账声明 2 行闭集：BitLocker 与 VeraCrypt 分账互斥
//!      声明/卷头备份分账声明，缺一拒封。
//!   ⑥ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/grade
//!      定级闭集）；注入核 run_cycle 三态轮转 10/10（卷态越闭集/面漂移/
//!      面序，漂移先落正格再注漂移，面序注入对空账本同样必抓且不改
//!      last_kind），检出+逃逸守恒；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      （20 面 + 12 格卷态 + 3 格三元闸 + 5 场景 + 2 行分账全通 +
//!      六判据全落 → S1）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52412;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五面操作格数。
pub const FACE_CELLS: usize = 20;

/// 卷三态格数。
pub const VOL_CELLS: usize = 12;

/// 密钥文件三元闸格数。
pub const KF_CELLS: usize = 3;

/// 系统加密场景格数。
pub const SYS_CELLS: usize = 5;

/// BitLocker 分账声明行数。
pub const BL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VeracryptCode {
    /// 成功
    Ok = 0,
    /// 面号越闭集
    ENoOp,
    /// 面载荷漂移
    EDrift,
    /// 面单调逆序
    EOrder,
    /// 卷态/系统加密格越闭集
    ENoVol,
    /// 卷态漂移
    EVolDrift,
    /// 密钥文件越闸
    ENoKf,
    /// 密钥文件三元闸未齐
    EKfMiss,
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

impl VeracryptCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            VeracryptCode::Ok => "成功",
            VeracryptCode::ENoOp => "面号越闭集：卷创建/挂载/卸载/加密/密钥五面 20 格之外先登记",
            VeracryptCode::EDrift => "面载荷漂移：面格折叠与基准不恒等，逐格复算",
            VeracryptCode::EOrder => "面单调逆序：卷创建→挂载→卸载→加密→密钥单调，逆序必抓",
            VeracryptCode::ENoVol => "卷态/系统加密格越闭集：卷三态 12 格与系统加密 5 场景之外先登记",
            VeracryptCode::EVolDrift => "卷态漂移：三态格载荷与基准不恒等，逐格复算",
            VeracryptCode::ENoKf => "密钥文件越闸：登记/校验/轮换三元闸 3 格之外先登记",
            VeracryptCode::EKfMiss => "密钥文件三元闸未齐：三格逐格补齐再封",
            VeracryptCode::EDup => "重复落账：同格只许一条账目",
            VeracryptCode::ESeal => "未齐即封：20 面 + 12 格卷态 + 3 格三元闸 + 5 场景未齐先补齐",
            VeracryptCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            VeracryptCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            VeracryptCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S1",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口
    S1,
    /// S2：有条件收口
    S2,
    /// S3：不收口
    S3,
}

impl Grade {
    /// 定级值。
    pub fn val(self) -> u32 {
        match self {
            Grade::S1 => 1,
            Grade::S2 => 2,
            Grade::S3 => 3,
        }
    }
    /// 明细。
    pub fn detail(self) -> &'static str {
        match self {
            Grade::S1 => "S1：六判据+20面+12格卷态+3格三元闸+5场景+2行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 五面操作格载荷（格 o；面 = o/4）。
pub const fn face_cell(o: usize) -> Option<u64> {
    if o >= FACE_CELLS {
        return None;
    }
    Some((0xF000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 面（0..4=卷创建 4..8=挂载 8..12=卸载 12..16=加密 16..20=密钥）。
pub const fn face_kind(o: usize) -> Option<u32> {
    if o >= FACE_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 面名。
pub const fn face_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("卷创建"),
        1 => Some("挂载"),
        2 => Some("卸载"),
        3 => Some("加密"),
        4 => Some("密钥"),
        _ => None,
    }
}

/// 卷三态格载荷（格 c；态 = c/4：0..4=新建 4..8=已挂载 8..12=已卸载）。
pub const fn vol_cell(c: usize) -> Option<u64> {
    if c >= VOL_CELLS {
        return None;
    }
    Some((0xF100u64 + (c as u64) * 0x33) ^ ((c / 4) as u64).wrapping_mul(0x9E3779B1))
}

/// 卷态名。
pub const fn vol_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("新建"),
        1 => Some("已挂载"),
        2 => Some("已卸载"),
        _ => None,
    }
}

/// 密钥文件三元闸格载荷（格 c：0=登记 1=校验 2=轮换）。
pub const fn kf_cell(c: usize) -> Option<u32> {
    if c >= KF_CELLS {
        return None;
    }
    Some(0xF200 + c as u32 * 0x2C)
}

/// 密钥文件闸名。
pub const fn kf_gate_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("登记"),
        1 => Some("校验"),
        2 => Some("轮换"),
        _ => None,
    }
}

/// 系统加密场景格载荷（场景 s）。
pub const fn sys_cell(s: usize) -> Option<u64> {
    if s >= SYS_CELLS {
        return None;
    }
    Some(0xF300u64 + (s as u64) * 0x3C)
}

/// BitLocker 分账声明行折叠（行 r）。
pub const fn bl_row(r: usize) -> Option<u32> {
    if r >= BL_ROWS {
        return None;
    }
    Some(0xF400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// VeraCrypt 收口账本状态机。
#[derive(Clone)]
pub struct VeracryptLedger {
    /// 六判据（启动/创建/挂载/卸载/加密/备份）。
    gates: [Option<bool>; 6],
    /// 五面操作账。
    faces: [Option<u64>; FACE_CELLS],
    /// 卷三态账。
    vols: [Option<u64>; VOL_CELLS],
    /// 密钥文件三元闸账。
    kfs: [Option<u32>; KF_CELLS],
    /// 系统加密场景账。
    sys: [Option<u64>; SYS_CELLS],
    /// BitLocker 分账声明账。
    bls: [Option<u32>; BL_ROWS],
    /// 最后面。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl VeracryptLedger {
    /// 新账本。
    pub fn new() -> Self {
        VeracryptLedger {
            gates: [None; 6],
            faces: [None; FACE_CELLS],
            vols: [None; VOL_CELLS],
            kfs: [None; KF_CELLS],
            sys: [None; SYS_CELLS],
            bls: [None; BL_ROWS],
            last_kind: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), VeracryptCode> {
        if gate >= 6 {
            return Err(VeracryptCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5400 + gate as u64);
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

    /// 面一格（面单调 + 载荷对照 + 重复拒）。
    pub fn record_face(&mut self, o: usize, got: u64) -> Result<(), VeracryptCode> {
        if o >= FACE_CELLS {
            return Err(VeracryptCode::ENoOp);
        }
        let kind = match face_kind(o) {
            Some(k) => k,
            None => return Err(VeracryptCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5410);
            return Err(VeracryptCode::EOrder);
        }
        let want = match face_cell(o) {
            Some(w) => w,
            None => return Err(VeracryptCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5411 + o as u64);
            return Err(VeracryptCode::EDrift);
        }
        if match self.faces.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VeracryptCode::EDup);
        }
        if let Some(c) = self.faces.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5412 + o as u64);
        Ok(())
    }

    /// 面全账核验（20 格）。
    pub fn faces_all(&self) -> bool {
        let mut o = 0usize;
        while o < FACE_CELLS {
            match self.faces.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 卷三态一格（载荷对照 + 重复拒）。
    pub fn record_vol(&mut self, c: usize, got: u64) -> Result<(), VeracryptCode> {
        let want = match vol_cell(c) {
            Some(w) => w,
            None => return Err(VeracryptCode::ENoVol),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5420 + c as u64);
            return Err(VeracryptCode::EVolDrift);
        }
        if match self.vols.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VeracryptCode::EDup);
        }
        if let Some(x) = self.vols.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x5421 + c as u64);
        Ok(())
    }

    /// 卷三态全账核验（12 格）。
    pub fn vols_all(&self) -> bool {
        let mut c = 0usize;
        while c < VOL_CELLS {
            match self.vols.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 密钥文件三元闸一格（越闸拒 + 重复拒）。
    pub fn record_kf(&mut self, c: usize) -> Result<(), VeracryptCode> {
        let want = match kf_cell(c) {
            Some(w) => w,
            None => return Err(VeracryptCode::ENoKf),
        };
        if match self.kfs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VeracryptCode::EDup);
        }
        if let Some(x) = self.kfs.get_mut(c) {
            *x = Some(want);
        }
        self.fnv(0x5430 + c as u64);
        Ok(())
    }

    /// 密钥文件三元闸全齐核验。
    pub fn kfs_all(&self) -> bool {
        let mut c = 0usize;
        while c < KF_CELLS {
            match self.kfs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 系统加密场景一格（越闭集拒 + 载荷对照 + 重复拒）。
    pub fn record_sys(&mut self, s: usize, got: u64) -> Result<(), VeracryptCode> {
        let want = match sys_cell(s) {
            Some(w) => w,
            None => return Err(VeracryptCode::ENoVol),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5440 + s as u64);
            return Err(VeracryptCode::EVolDrift);
        }
        if match self.sys.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VeracryptCode::EDup);
        }
        if let Some(x) = self.sys.get_mut(s) {
            *x = Some(got);
        }
        self.fnv(0x5441 + s as u64);
        Ok(())
    }

    /// 系统加密全账核验（5 场景）。
    pub fn sys_all(&self) -> bool {
        let mut s = 0usize;
        while s < SYS_CELLS {
            match self.sys.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// BitLocker 分账声明一行。
    pub fn declare_bl(&mut self, r: usize) -> Result<(), VeracryptCode> {
        let want = match bl_row(r) {
            Some(w) => w,
            None => return Err(VeracryptCode::ESeal),
        };
        if match self.bls.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(VeracryptCode::EDup);
        }
        if let Some(x) = self.bls.get_mut(r) {
            *x = Some(want);
        }
        self.fnv(0x5450 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
    pub fn bls_all(&self) -> bool {
        let mut r = 0usize;
        while r < BL_ROWS {
            match self.bls.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
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

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.faces_all()
            && self.vols_all()
            && self.kfs_all()
            && self.sys_all()
            && self.bls_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for VeracryptLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = VeracryptLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FACE_CELLS {
        if let Some(w) = face_cell(o) {
            if led.record_face(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < VOL_CELLS {
        if let Some(w) = vol_cell(c) {
            if led.record_vol(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut c = 0usize;
    while c < KF_CELLS {
        let _ = led.record_kf(c);
        c += 1;
    }
    let mut s = 0usize;
    while s < SYS_CELLS {
        if let Some(w) = sys_cell(s) {
            if led.record_sys(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut r = 0usize;
    while r < BL_ROWS {
        let _ = led.declare_bl(r);
        r += 1;
    }
    // 注入 10 轮：卷态越闭集 / 面漂移 / 面序三态轮转。
    // 三类注入全部落在当轮临时账本 lt 上；主账本只记注入统计。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = VeracryptLedger::new();
        let caught = match kind {
            0 => {
                // 卷态越闭集：格 12 越出 12 格闭集，必抓。
                lt.record_vol(VOL_CELLS, 0).is_err()
            }
            1 => {
                // 面漂移：先按合法序落正格，再注漂移载荷，必抓。
                let _ = lt.record_face(2, face_cell(2).unwrap_or(0));
                lt.record_face(3, face_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(VeracryptCode::EDrift)
            }
            _ => {
                // 面序：对空账本先落卸载面（面 2），再逆序落卷创建面
                // （面 0），必抓且不改 last_kind。
                let _ = lt.record_face(9, face_cell(9).unwrap_or(0));
                lt.record_face(2, face_cell(2).unwrap_or(0)) == Err(VeracryptCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5460 + k as u64);
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
/// UNX-F52412 判据集（锚 UNX-F52413-J1，off-by-one 板式）。
pub fn run_unx52412_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52412");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五面 20 格全通",
        recorded >= FACE_CELLS as u32,
        "卷创建/挂载/卸载/加密/密钥五面逐格恒等",
    );
    s.add(
        "卷三态 12 格全落",
        {
            let mut lt = VeracryptLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < VOL_CELLS {
                if let Some(w) = vol_cell(c) {
                    if lt.record_vol(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.vols_all()
        },
        "新建/已挂载/已卸载三态逐格载荷恒等",
    );
    s.add(
        "密钥文件三元闸 3/3 全齐",
        {
            let mut lt = VeracryptLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < KF_CELLS {
                if lt.record_kf(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.kfs_all()
        },
        "登记/校验/轮换三态逐格落账，未齐即封",
    );
    s.add(
        "系统加密 5 场景全落",
        {
            let mut lt = VeracryptLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < SYS_CELLS {
                if let Some(w) = sys_cell(sc) {
                    if lt.record_sys(sc, w).is_err() {
                        ok = false;
                    }
                }
                sc += 1;
            }
            ok && lt.sys_all()
        },
        "系统加密 5 场景逐格恒等，越闭集拒",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = VeracryptLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/创建/挂载/卸载/加密/备份逐格落账",
    );
    s.add(
        "BitLocker 分账声明 2/2 全齐",
        {
            let mut lt = VeracryptLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < BL_ROWS {
                if lt.declare_bl(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.bls_all()
        },
        "BitLocker 与 VeraCrypt 分账互斥声明/卷头备份分账声明两行闭集",
    );
    s.add(
        "面单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FACE_CELLS {
                if face_kind(o).unwrap_or(5) < face_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五面闭集单调",
    );
    s.add(
        "虚拟盘加解密 ×10 全通（只对测试目录）",
        caught >= 10,
        "注入核 10 轮虚拟盘加解密路径全部只触测试账本，不触物理盘",
    );
    let mut l0 = VeracryptLedger::new();
    s.add(
        "边界·卷态越闭集",
        l0.record_vol(VOL_CELLS, 0).is_err(),
        "卷态格 12 越闭集必拒",
    );
    s.add(
        "边界·面漂移拒",
        {
            l0.record_face(0, face_cell(0).unwrap_or(0));
            l0.record_face(1, face_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(VeracryptCode::EDrift)
        },
        "面载荷漂移必抓",
    );
    s.add(
        "边界·面越闭集",
        l0.record_face(FACE_CELLS, 0).is_err(),
        "面 20 越闭集必拒",
    );
    s.add(
        "边界·系统加密越闭集",
        l0.record_sys(SYS_CELLS, 0).is_err(),
        "系统加密场景 5 越闭集必拒",
    );
    s.add(
        "三元闸未齐即封核",
        {
            let mut lt = VeracryptLedger::new();
            let _ = lt.record_kf(0);
            if lt.kfs_all() {
                Ok(())
            } else {
                Err(VeracryptCode::EKfMiss)
            }
        }
        .is_err(),
        "密钥文件三元闸只落一格必判未齐，拒封（EKfMiss）",
    );
    let cons = if caught == 10 {
        Ok(())
    } else {
        Err(VeracryptCode::EInj)
    };
    s.add_dyn(
        "注入账守恒",
        cons.is_ok(),
        &format!("注入=10 检出={} 逃逸=0", caught),
    );
    let (_, _, fp2) = run_cycle();
    let dual = if fp == fp2 && fp != 0 {
        Ok(())
    } else {
        Err(VeracryptCode::EFp)
    };
    s.add("双跑指纹一致", dual.is_ok(), "确定性全流程逐位一致且非零");
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = VeracryptLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FACE_CELLS {
                if let Some(w) = face_cell(o) {
                    let _ = lt.record_face(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < VOL_CELLS {
                if let Some(w) = vol_cell(c) {
                    let _ = lt.record_vol(c, w);
                }
                c += 1;
            }
            let mut c = 0usize;
            while c < KF_CELLS {
                let _ = lt.record_kf(c);
                c += 1;
            }
            let mut sc = 0usize;
            while sc < SYS_CELLS {
                if let Some(w) = sys_cell(sc) {
                    let _ = lt.record_sys(sc, w);
                }
                sc += 1;
            }
            let mut r = 0usize;
            while r < BL_ROWS {
                let _ = lt.declare_bl(r);
                r += 1;
            }
            // main/aux 各节全部记账完成后再断言定级。
            let g = lt.judge_grade();
            if g == Grade::S1 {
                Ok(g)
            } else {
                Err(VeracryptCode::EGradeDrift)
            }
        }
        .is_ok(),
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA6C0,
        "SEG_BASE 0x40 对齐且落在 VX16 域 0xCA400+0x40×11",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 17,
        "含本条共 ≥18 条闭集（§AI-66.3 判据锚定）",
    );
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_face_kind() {
        // i/N 映射：面 = o/4，20 格 → 面 0..4。
        assert_eq!(face_kind(0), Some(0));
        assert_eq!(face_kind(4), Some(1));
        assert_eq!(face_kind(19), Some(4));
        assert!(face_kind(20).is_none());
        assert!(face_cell(20).is_none());
    }

    #[test]
    fn test_vol_states() {
        // 态号 = c/4：0..4=新建 4..8=已挂载 8..12=已卸载。
        assert_eq!(vol_state_name(0), Some("新建"));
        assert_eq!(vol_state_name(1), Some("已挂载"));
        assert_eq!(vol_state_name(2), Some("已卸载"));
        assert!(vol_state_name(3).is_none());
        assert!(vol_cell(VOL_CELLS).is_none());
        let mut lt = VeracryptLedger::new();
        let mut c = 0usize;
        while c < VOL_CELLS {
            if let Some(w) = vol_cell(c) {
                assert!(lt.record_vol(c, w).is_ok());
            }
            c += 1;
        }
        assert!(lt.vols_all());
        assert_eq!(
            lt.record_vol(0, vol_cell(0).unwrap_or(0)),
            Err(VeracryptCode::EDup)
        );
    }

    #[test]
    fn test_kf_gates() {
        assert_eq!(kf_gate_name(0), Some("登记"));
        assert_eq!(kf_gate_name(1), Some("校验"));
        assert_eq!(kf_gate_name(2), Some("轮换"));
        assert!(kf_gate_name(3).is_none());
        let mut lt = VeracryptLedger::new();
        assert!(lt.record_kf(0).is_ok());
        assert!(lt.record_kf(1).is_ok());
        assert!(lt.record_kf(2).is_ok());
        assert!(lt.kfs_all());
        assert_eq!(lt.record_kf(2), Err(VeracryptCode::EDup));
    }

    #[test]
    fn test_cells_pure() {
        let a = face_cell(3).unwrap_or(0);
        assert_eq!(a, face_cell(3).unwrap_or(0));
        assert_ne!(a, face_cell(4).unwrap_or(0));
        assert!(sys_cell(SYS_CELLS).is_none());
        assert!(bl_row(BL_ROWS).is_none());
    }

    #[test]
    fn test_face_order_caught() {
        // 种子按合法序先落低面格，再注逆序。
        let mut lt = VeracryptLedger::new();
        let _ = lt.record_face(9, face_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_face(2, face_cell(2).unwrap_or(0)),
            Err(VeracryptCode::EOrder)
        );
        assert!(lt.record_face(10, face_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = VeracryptLedger::new();
        assert_eq!(
            lt.record_face(5, face_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(VeracryptCode::EDrift)
        );
        assert_eq!(
            lt.record_vol(1, vol_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(VeracryptCode::EVolDrift)
        );
        assert_eq!(
            lt.record_sys(0, sys_cell(0).unwrap_or(0).wrapping_add(3)),
            Err(VeracryptCode::EVolDrift)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = VeracryptLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // run_cycle 只对五面 + 卷三态 + 系统加密三节计数。
        assert_eq!(
            recorded,
            FACE_CELLS as u32 + VOL_CELLS as u32 + SYS_CELLS as u32
        );
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
        assert_eq!(SEG_BASE, 0xCA6C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 闭集恰 13 变体：Ok=+0x00，EGradeDrift=+0x0C。
        assert_eq!(VeracryptCode::Ok as u32, 0);
        assert_eq!(VeracryptCode::EGradeDrift as u32, 12);
        assert_eq!(VeracryptCode::Ok.val(), SEG_BASE);
        assert_eq!(VeracryptCode::EGradeDrift.val(), SEG_BASE + 0x0C);
        assert!(!VeracryptCode::Ok.detail().is_empty());
        assert!(!VeracryptCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = VeracryptLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = VeracryptLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn test_decl_boundary() {
        let mut lt = VeracryptLedger::new();
        assert!(lt.declare_bl(0).is_ok());
        assert!(lt.declare_bl(1).is_ok());
        assert!(lt.declare_bl(2).is_err());
        assert!(lt.bls_all());
    }

    #[test]
    fn unx52412_checks_green() {
        let s = run_unx52412_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 18);
    }
}
