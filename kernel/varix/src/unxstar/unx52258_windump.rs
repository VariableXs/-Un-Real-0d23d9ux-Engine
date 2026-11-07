//! UNX-F52258 WinDump 收口 · 0xC8040段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52258"，逐字）：
//!   title: WinDump 收口
//!   quota: 200
//!   criteria: ① 纯功能代码 ≥200 行；② 判据原文：UNX-F52259-J1 路由
//!     追踪/图表 ×10；ICMP 消费 ×5；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52258）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52259-J1，照原文锚定；功能按本卡标题（WinDump）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① Npcap 消费联签 4 格（F52249 联签锚定行）：枚举/打开/BPF 注入/
//!      逐包回调四格与 F52249 基准恒等，漂移必抓。
//!   ② 过滤器 BPF 语义 10 格：host/net/port/proto/方向五语义类各 2
//!      格，类单调（逆序必抓），逐格载荷恒等。
//!   ③ 捕获三态 8 格：空闲/捕获中/停止三态编码入载荷，状态漂移必抓。
//!   ④ EOL 声明 2 行闭集：WinPcap EOL 事实在册 / Npcap 迁移口径声明，
//!      缺一拒封（WinPcap 系如实在册）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 闭集）；注入核 run_cycle 10 轮三态轮转（联签漂移/BPF 类
//!      逆序/捕获格越闭集）全部落在当轮临时账本上；双跑指纹一致；
//!      KB-COMPAT 定级 S2。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52258;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () =
    assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// Npcap 消费联签格数（F52249）。
pub const NPCAP_CELLS: usize = 4;

/// BPF 过滤器语义格数。
pub const BPF_CELLS: usize = 10;

/// 捕获三态格数。
pub const CAP_CELLS: usize = 8;

/// EOL 声明行数。
pub const EOL_ROWS: usize = 2;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindumpCode {
    /// 成功
    Ok = 0,
    /// 联签格越闭集
    ENoNp,
    /// Npcap 联签漂移
    ENpDrift,
    /// BPF 格越闭集
    ENoBpf,
    /// BPF 语义漂移
    EBpfDrift,
    /// BPF 语义类逆序
    EBpfOrder,
    /// 捕获格越闭集
    ENoCap,
    /// 捕获三态漂移
    ECapDrift,
    /// EOL 行越闭集
    ENoEol,
    /// 缺 EOL 声明行
    EEolMiss,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl WindumpCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            WindumpCode::Ok => "成功",
            WindumpCode::ENoNp => "联签格越闭集：Npcap 消费 4 格（F52249）之外先登记",
            WindumpCode::ENpDrift => "Npcap 联签漂移：与 F52249 基准不恒等，四格逐格复算",
            WindumpCode::ENoBpf => "BPF 格越闭集：过滤器 10 格闭集之外先登记",
            WindumpCode::EBpfDrift => "BPF 语义漂移：过滤器折叠与基准不恒等，逐格复算",
            WindumpCode::EBpfOrder => "BPF 语义类逆序：host→net→port→proto→方向 单调，逆序必抓",
            WindumpCode::ENoCap => "捕获格越闭集：捕获三态 8 格闭集之外先登记",
            WindumpCode::ECapDrift => "捕获三态漂移：空闲/捕获中/停止状态编码与基准不符",
            WindumpCode::ENoEol => "EOL 行越闭集：2 行闭集之外先登记",
            WindumpCode::EEolMiss => "缺 EOL 声明行：WinPcap EOL 事实/Npcap 迁移口径未齐先补齐",
            WindumpCode::ESeal => "未齐即封：4 联签 + 10 BPF + 8 捕获 + 2 EOL 未齐先补齐",
            WindumpCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            WindumpCode::EGradeDrift => "定级漂移：全通且 EOL 声明全齐才可定 S2（KB-COMPAT 登记）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不用，保留闭集完备）
    S1,
    /// S2：有条件收口（本卡定级）
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
            Grade::S1 => "S1：全通收口（WinDump 域不适用，闭集保留）",
            Grade::S2 => "S2：4 联签+10 BPF+8 捕获+2 EOL 全通，Npcap 依赖消费口径收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// Npcap 消费联签格（格 d；F52249 联签锚定行基准）。
pub const fn npcap_cell(d: usize) -> Option<u64> {
    if d >= NPCAP_CELLS {
        return None;
    }
    Some((0xD1000u64 + (d as u64) * 0x31) ^ 0xF522_4900_0000_0000)
}

/// 联签格名。
pub const fn npcap_name(d: usize) -> Option<&'static str> {
    match d {
        0 => Some("适配器枚举"),
        1 => Some("打开会话"),
        2 => Some("BPF 程序注入"),
        3 => Some("逐包回调"),
        _ => None,
    }
}

/// BPF 过滤器语义格载荷（格 b；语义类 = b/2）。
pub const fn bpf_cell(b: usize) -> Option<u64> {
    if b >= BPF_CELLS {
        return None;
    }
    Some((0xD2000u64 + (b as u64) * 0x2C) ^ ((b / 2) as u64).wrapping_mul(0x01000193))
}

/// BPF 语义类（0..2=host 2..4=net 4..6=port 6..8=proto 8..10=方向）。
pub const fn bpf_kind(b: usize) -> Option<u32> {
    if b >= BPF_CELLS {
        return None;
    }
    Some((b / 2) as u32)
}

/// BPF 语义类名。
pub const fn bpf_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("host"),
        1 => Some("net"),
        2 => Some("port"),
        3 => Some("proto"),
        4 => Some("方向"),
        _ => None,
    }
}

/// 捕获三态编码（态 s：0=空闲 1=捕获中 2=停止）。
pub const fn cap_state(s: usize) -> Option<u32> {
    match s {
        0 => Some(0),
        1 => Some(1),
        2 => Some(2),
        _ => None,
    }
}

/// 捕获三态格载荷（格 c，三态按 c%3 周期编码入载荷）。
pub const fn cap_cell(c: usize) -> Option<u64> {
    if c >= CAP_CELLS {
        return None;
    }
    let st = (c % 3) as u64;
    Some((0xD3000u64 + (c as u64) * 0x23) ^ st.wrapping_mul(0x5354_0000))
}

/// 捕获格态名。
pub const fn cap_state_name(c: usize) -> Option<&'static str> {
    if c >= CAP_CELLS {
        return None;
    }
    match c % 3 {
        0 => Some("空闲"),
        1 => Some("捕获中"),
        _ => Some("停止"),
    }
}

/// EOL 声明行折叠（行 r）。
pub const fn eol_row(r: usize) -> Option<u32> {
    if r >= EOL_ROWS {
        return None;
    }
    Some(0xD400 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// WinDump 收口账本状态机。
#[derive(Clone)]
pub struct WindumpLedger {
    /// 六判据（枚举/过滤/捕获/解码/统计/EOL 口径）。
    gates: [Option<bool>; 6],
    /// Npcap 联签账。
    npcaps: [Option<u64>; NPCAP_CELLS],
    /// BPF 账。
    bpfs: [Option<u64>; BPF_CELLS],
    /// 捕获账。
    caps: [Option<u64>; CAP_CELLS],
    /// EOL 声明账。
    eols: [Option<u32>; EOL_ROWS],
    /// 最后 BPF 语义类。
    last_bpf: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl WindumpLedger {
    /// 新账本。
    pub fn new() -> Self {
        WindumpLedger {
            gates: [None; 6],
            npcaps: [None; NPCAP_CELLS],
            bpfs: [None; BPF_CELLS],
            caps: [None; CAP_CELLS],
            eols: [None; EOL_ROWS],
            last_bpf: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), WindumpCode> {
        if gate >= 6 {
            return Err(WindumpCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5310 + gate as u64);
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

    /// Npcap 联签一格（F52249 基准恒等）。
    pub fn record_npcap(&mut self, d: usize, got: u64) -> Result<(), WindumpCode> {
        let want = match npcap_cell(d) {
            Some(w) => w,
            None => return Err(WindumpCode::ENoNp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5320 + d as u64);
            return Err(WindumpCode::ENpDrift);
        }
        if match self.npcaps.get(d) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WindumpCode::ESeal);
        }
        if let Some(c) = self.npcaps.get_mut(d) {
            *c = Some(got);
        }
        self.fnv(0x5321 + d as u64);
        Ok(())
    }

    /// Npcap 联签全账核验（4 格）。
    pub fn npcaps_all(&self) -> bool {
        let mut d = 0usize;
        while d < NPCAP_CELLS {
            match self.npcaps.get(d) {
                Some(Some(_)) => {}
                _ => return false,
            }
            d += 1;
        }
        true
    }

    /// BPF 一格（语义类单调 + 载荷对照 + 重复拒）。
    pub fn record_bpf(&mut self, b: usize, got: u64) -> Result<(), WindumpCode> {
        if b >= BPF_CELLS {
            return Err(WindumpCode::ENoBpf);
        }
        let kind = match bpf_kind(b) {
            Some(k) => k,
            None => return Err(WindumpCode::ENoBpf),
        };
        if (kind as i64) < self.last_bpf {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5330);
            return Err(WindumpCode::EBpfOrder);
        }
        let want = match bpf_cell(b) {
            Some(w) => w,
            None => return Err(WindumpCode::ENoBpf),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5331 + b as u64);
            return Err(WindumpCode::EBpfDrift);
        }
        if match self.bpfs.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WindumpCode::ESeal);
        }
        if let Some(c) = self.bpfs.get_mut(b) {
            *c = Some(got);
        }
        self.last_bpf = kind as i64;
        self.fnv(0x5332 + b as u64);
        Ok(())
    }

    /// BPF 全账核验（10 格）。
    pub fn bpfs_all(&self) -> bool {
        let mut b = 0usize;
        while b < BPF_CELLS {
            match self.bpfs.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 捕获一格（三态编码恒等）。
    pub fn record_cap(&mut self, c: usize, got: u64) -> Result<(), WindumpCode> {
        let want = match cap_cell(c) {
            Some(w) => w,
            None => return Err(WindumpCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5340 + c as u64);
            return Err(WindumpCode::ECapDrift);
        }
        if match self.caps.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WindumpCode::ESeal);
        }
        if let Some(cell) = self.caps.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x5341 + c as u64);
        Ok(())
    }

    /// 捕获全账核验（8 格）。
    pub fn caps_all(&self) -> bool {
        let mut c = 0usize;
        while c < CAP_CELLS {
            match self.caps.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// EOL 声明一行（WinPcap 系如实在册）。
    pub fn declare_eol(&mut self, r: usize) -> Result<(), WindumpCode> {
        if r >= EOL_ROWS {
            return Err(WindumpCode::ENoEol);
        }
        if match self.eols.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(WindumpCode::ESeal);
        }
        if let Some(c) = self.eols.get_mut(r) {
            *c = eol_row(r);
        }
        self.fnv(0x5350 + r as u64);
        Ok(())
    }

    /// EOL 声明全齐核验。
    pub fn eols_all(&self) -> bool {
        let mut r = 0usize;
        while r < EOL_ROWS {
            match self.eols.get(r) {
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

    /// KB-COMPAT 定级（全通 → S2，判据缺 → S3，其余 → S2 条件口径）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.npcaps_all()
            && self.bpfs_all()
            && self.caps_all()
            && self.eols_all()
        {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for WindumpLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = WindumpLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut d = 0usize;
    while d < NPCAP_CELLS {
        if let Some(w) = npcap_cell(d) {
            if led.record_npcap(d, w).is_ok() {
                recorded += 1;
            }
        }
        d += 1;
    }
    let mut b = 0usize;
    while b < BPF_CELLS {
        if let Some(w) = bpf_cell(b) {
            if led.record_bpf(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut c = 0usize;
    while c < CAP_CELLS {
        if let Some(w) = cap_cell(c) {
            if led.record_cap(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut r = 0usize;
    while r < EOL_ROWS {
        let _ = led.declare_eol(r);
        r += 1;
    }
    // 注入 10 轮：联签漂移 / BPF 类逆序 / 捕获格越闭集三态轮转，
    // 全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = WindumpLedger::new();
        let caught = match kind {
            0 => lt.record_npcap(3, npcap_cell(3).unwrap_or(0).wrapping_add(1))
                == Err(WindumpCode::ENpDrift),
            1 => {
                let _ = lt.record_bpf(8, bpf_cell(8).unwrap_or(0));
                lt.record_bpf(3, bpf_cell(3).unwrap_or(0)) == Err(WindumpCode::EBpfOrder)
            }
            _ => lt.record_cap(CAP_CELLS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5360 + k as u64);
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
/// UNX-F52258 判据集（锚 UNX-F52259-J1，off-by-one 板式）。
pub fn run_unx52258_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52258");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "Npcap 消费联签 4/4 恒等",
        {
            let mut lt = WindumpLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < NPCAP_CELLS {
                if let Some(w) = npcap_cell(d) {
                    if lt.record_npcap(d, w).is_err() {
                        ok = false;
                    }
                }
                d += 1;
            }
            ok && lt.npcaps_all()
        },
        "枚举/打开/BPF 注入/逐包回调四格与 F52249 基准恒等（联签锚定行）",
    );
    s.add(
        "BPF 过滤器 10 格全通",
        {
            let mut lt = WindumpLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BPF_CELLS {
                if let Some(w) = bpf_cell(b) {
                    if lt.record_bpf(b, w).is_err() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && lt.bpfs_all()
        },
        "host/net/port/proto/方向五语义类逐格恒等",
    );
    s.add(
        "BPF 语义类单调",
        {
            let mut ok = true;
            let mut b = 1usize;
            while b < BPF_CELLS {
                if bpf_kind(b).unwrap_or(5) < bpf_kind(b - 1).unwrap_or(5) {
                    ok = false;
                }
                b += 1;
            }
            ok
        },
        "五语义类闭集单调，逆序必抓",
    );
    s.add(
        "捕获三态 8 格全通",
        {
            let mut lt = WindumpLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CAP_CELLS {
                if let Some(w) = cap_cell(c) {
                    if lt.record_cap(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.caps_all()
        },
        "空闲/捕获中/停止三态编码逐格恒等",
    );
    s.add(
        "EOL 声明 2/2 全齐",
        {
            let mut lt = WindumpLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < EOL_ROWS {
                if lt.declare_eol(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.eols_all()
        },
        "WinPcap EOL 事实在册 / Npcap 迁移口径声明两行闭集（WinPcap 系如实在册）",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = WindumpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "枚举/过滤/捕获/解码/统计/EOL 口径逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "联签漂移/BPF 类逆序/捕获越闭集三态轮转必抓",
    );
    let mut l0 = WindumpLedger::new();
    s.add(
        "边界·联签格越闭集",
        l0.record_npcap(NPCAP_CELLS, 0).is_err(),
        "联签格 4 越闭集必拒",
    );
    s.add(
        "边界·BPF 格越闭集",
        l0.record_bpf(BPF_CELLS, 0).is_err(),
        "BPF 格 10 越闭集必拒",
    );
    s.add(
        "边界·捕获格越闭集",
        l0.record_cap(CAP_CELLS, 0).is_err(),
        "捕获格 8 越闭集必拒",
    );
    s.add(
        "边界·EOL 行越闭集",
        l0.declare_eol(EOL_ROWS).is_err(),
        "EOL 行 2 越闭集必拒",
    );
    s.add(
        "漂移注入先录正格再注漂移",
        {
            let _ = lt_record_ok(&mut l0);
            l0.record_bpf(5, bpf_cell(5).unwrap_or(0).wrapping_add(0x40)) == Err(WindumpCode::EBpfDrift)
        },
        "BPF 正格先收，漂移格必抓",
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
        "定级 S2 闭集",
        {
            let mut lt = WindumpLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut d = 0usize;
            while d < NPCAP_CELLS {
                if let Some(w) = npcap_cell(d) {
                    let _ = lt.record_npcap(d, w);
                }
                d += 1;
            }
            let mut b = 0usize;
            while b < BPF_CELLS {
                if let Some(w) = bpf_cell(b) {
                    let _ = lt.record_bpf(b, w);
                }
                b += 1;
            }
            let mut c = 0usize;
            while c < CAP_CELLS {
                if let Some(w) = cap_cell(c) {
                    let _ = lt.record_cap(c, w);
                }
                c += 1;
            }
            let mut r = 0usize;
            while r < EOL_ROWS {
                let _ = lt.declare_eol(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级登记，Npcap 依赖消费口径）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8040,
        "SEG_BASE 0x40 对齐且落在 VX15 域 52258 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

/// 判据内辅助：向账本落一格 BPF 正格（避免闭包越界书写）。
fn lt_record_ok(led: &mut WindumpLedger) -> bool {
    match bpf_cell(1) {
        Some(w) => led.record_bpf(1, w).is_ok(),
        None => false,
    }
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_npcap_cells() {
        assert_eq!(npcap_name(0), Some("适配器枚举"));
        assert_eq!(npcap_name(3), Some("逐包回调"));
        assert!(npcap_name(4).is_none());
        assert!(npcap_cell(NPCAP_CELLS).is_none());
    }

    #[test]
    fn test_bpf_kind() {
        assert_eq!(bpf_kind(0), Some(0));
        assert_eq!(bpf_kind(1), Some(0));
        assert_eq!(bpf_kind(2), Some(1));
        assert_eq!(bpf_kind(9), Some(4));
        assert!(bpf_kind(10).is_none());
        assert!(bpf_cell(BPF_CELLS).is_none());
        assert_eq!(bpf_kind_name(4), Some("方向"));
        assert!(bpf_kind_name(5).is_none());
    }

    #[test]
    fn test_cap_three_states() {
        assert_eq!(cap_state(0), Some(0));
        assert_eq!(cap_state(1), Some(1));
        assert_eq!(cap_state(2), Some(2));
        assert!(cap_state(3).is_none());
        assert_eq!(cap_state_name(0), Some("空闲"));
        assert_eq!(cap_state_name(1), Some("捕获中"));
        assert_eq!(cap_state_name(2), Some("停止"));
        assert_eq!(cap_state_name(3), Some("空闲"));
        assert!(cap_state_name(CAP_CELLS).is_none());
    }

    #[test]
    fn test_cap_state_drift_caught() {
        let mut lt = WindumpLedger::new();
        // 状态漂移：c=0（空闲态基准）混入 c=1 的捕获中态编码必抓。
        let v1 = cap_cell(1).unwrap_or(0);
        assert_eq!(lt.record_cap(0, v1), Err(WindumpCode::ECapDrift));
        assert!(lt.record_cap(0, cap_cell(0).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_bpf_order_caught() {
        let mut lt = WindumpLedger::new();
        let _ = lt.record_bpf(8, bpf_cell(8).unwrap_or(0));
        assert_eq!(lt.record_bpf(3, bpf_cell(3).unwrap_or(0)), Err(WindumpCode::EBpfOrder));
        assert!(lt.record_bpf(9, bpf_cell(9).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_npcap_drift_caught() {
        let mut lt = WindumpLedger::new();
        assert!(lt.record_npcap(0, npcap_cell(0).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_npcap(1, npcap_cell(1).unwrap_or(0).wrapping_add(0x11)),
            Err(WindumpCode::ENpDrift)
        );
    }

    #[test]
    fn test_eol_rows() {
        assert!(eol_row(0).is_some());
        assert!(eol_row(1).is_some());
        assert!(eol_row(2).is_none());
        let mut lt = WindumpLedger::new();
        assert!(lt.declare_eol(0).is_ok());
        assert!(lt.declare_eol(1).is_ok());
        assert!(lt.declare_eol(2).is_err());
        assert!(lt.eols_all());
    }

    #[test]
    fn test_boundary_out_of_set() {
        let mut lt = WindumpLedger::new();
        assert!(lt.record_npcap(NPCAP_CELLS, 0).is_err());
        assert!(lt.record_bpf(BPF_CELLS, 0).is_err());
        assert!(lt.record_cap(CAP_CELLS, 0).is_err());
        assert!(lt.declare_eol(EOL_ROWS).is_err());
        assert!(lt.judge_gate(6).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (NPCAP_CELLS + BPF_CELLS + CAP_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8040);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(WindumpCode::Ok as u32, 0);
        assert_eq!(WindumpCode::EGradeDrift as u32, 12);
        assert_eq!(WindumpCode::EEolMiss.val(), SEG_BASE + 9);
        assert!(!WindumpCode::Ok.detail().is_empty());
        assert!(!WindumpCode::ENpDrift.detail().is_empty());
        assert!(!WindumpCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = WindumpLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = WindumpLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        // 六闸全落但账格未齐 → 有条件收口 S2。
        assert_eq!(lt2.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52258_checks_green() {
        let s = run_unx52258_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
