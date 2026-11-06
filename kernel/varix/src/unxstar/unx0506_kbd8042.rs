//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0506 8042 键盘控制器探测账（只读预备） · 0x76340段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定）：
//!   探测三件账齐（存在判定/双口态/配置字节口径），A1 期零控制器写
//!   断言（轮询预备口径），探测账封账指纹重放。
//! 设计要点：
//!   1. 探测三件闭集：存在判定（状态口 0x64 口径自检位 = bit2 系统标志
//!      位，POST 自检通过置位）/ 双口态（0x60 数据窗 + 0x64 命令窗两
//!      窗口在账）/ 配置字节（0x20 读命令口径，取冻结常量 CFG_FROZEN=
//!      0x45，手册默认配置口径），三件缺一即红；
//!   2. A1 期零控制器写断言：探测期写计数恒 0——写路径 API 一律显性
//!      拒（A1WriteForbidden），尝试记独立注入账，真发写计数不增；
//!   3. 轮询预备口径：仅状态口读（0x64 读 = 状态寄存器镜像），OBF
//!      bit0 就绪位读账，不做任何控制器写；
//!   4. 探测账只追加：三件逐事件落账，行号连续递增；
//!   5. 封账指纹重放：FNV-1a 32 位对账全文压缩封账，重放复算必须
//!      逐字节一致，篡改即 ReplayMismatch；
//!   6. 注入三路（控制器写/双口缺账/封账篡改）×10 轮必抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x76340，AI-VX7-1104 批7 O 组独占区）。
pub const SEG_BASE: u32 = 0x76340;

/// 数据口（0x60，8042 手册口径）。
pub const PORT_DATA: u16 = 0x60;
/// 命令/状态口（0x64，8042 手册口径）。
pub const PORT_CMD: u16 = 0x64;
/// 配置字节读命令（0x20，8042 手册口径）。
pub const CMD_READ_CFG: u8 = 0x20;
/// 配置字节冻结口径（0x45：bit6 翻译使能 + bit0/bit2 两口中断使能，
/// 8042 手册默认配置口径）。
pub const CFG_FROZEN: u8 = 0x45;
/// 状态口自检位（bit2 系统标志位：POST 自检通过置位）。
pub const STATUS_SELFTEST_BIT: u8 = 2;
/// 状态口就绪位（bit0 OBF：输出缓冲满，轮询预备口径）。
pub const STATUS_OBF_BIT: u8 = 0;
/// 状态口冻结镜像（自检位 + OBF 就绪，轮询预备口径）。
pub const STATUS_FROZEN: u8 = (1 << STATUS_SELFTEST_BIT) | (1 << STATUS_OBF_BIT);
/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;
/// 注入路数（控制器写/双口缺账/封账篡改 三路）。
pub const INJ_ROUTES: usize = 3;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KbdCode {
    /// A1 期控制器写：探测期写路径一律显性拒
    A1WriteForbidden,
    /// 双口缺账：0x60/0x64 两窗口任一不在账
    PortMissing,
    /// 存在判定未落账：自检位口径未读
    PresenceUnknown,
    /// 配置字节缺账：0x20 口径未取
    ConfigMissing,
    /// 封账指纹失配：重放复算与封账不等
    ReplayMismatch,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl KbdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KbdCode::A1WriteForbidden => "A1 期控制器写：探测期零写红线，写路径请移步 A2 期初始化",
            KbdCode::PortMissing => "双口缺账：0x60 数据窗与 0x64 命令窗必须双双在账",
            KbdCode::PresenceUnknown => "存在判定未落账：先读 0x64 状态口自检位（bit2）",
            KbdCode::ConfigMissing => "配置字节缺账：按 0x20 读命令口径取冻结配置 0x45",
            KbdCode::ReplayMismatch => "封账指纹失配：探测账只追加不许改，篡改行请整账重开",
            KbdCode::NotReproducible => "双跑不一致：同口径探测必须同账同指纹，建议固定镜像",
        }
    }
    pub const ALL: [KbdCode; 6] = [
        KbdCode::A1WriteForbidden,
        KbdCode::PortMissing,
        KbdCode::PresenceUnknown,
        KbdCode::ConfigMissing,
        KbdCode::ReplayMismatch,
        KbdCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < KbdCode::ALL.len() {
        if let Some(c) = KbdCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x76340 段闭集内"
}

// ── FNV-1a 32 位指纹（封账重放） ────────────────────────────────
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h = h.wrapping_mul(0x0100_0193) ^ (b as u32);
    }
    h
}

// ── LCG（种子固定） ─────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 真机探测种子（冻结 = SEG_BASE）。
pub fn machine_seed() -> u64 {
    SEG_BASE as u64
}

// ── 双口态窗口账 ────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PortLedger {
    /// 端口号（0x60 / 0x64）。
    pub port: u16,
    /// 读计数（轮询预备口径）。
    pub reads: u32,
    /// 真发写计数（A1 期恒 0）。
    pub writes: u32,
    /// 在账标志。
    pub in_ledger: bool,
}

impl PortLedger {
    pub fn new(port: u16) -> PortLedger {
        PortLedger { port, reads: 0, writes: 0, in_ledger: false }
    }
}

// ── 探测账行（只追加） ──────────────────────────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeRow {
    pub seq: u64,
    pub text: String,
}

/// 8042 探测机（只读预备口径）。
#[derive(Clone, Debug)]
pub struct Probe8042 {
    /// 状态口镜像（0x64 读，冻结口径）。
    pub status: u8,
    /// 双口窗口账：槽 0 = 数据窗，槽 1 = 命令窗。
    pub ports: [PortLedger; 2],
    /// 存在判定结果（自检位口径）。
    pub presence: Option<bool>,
    /// 配置字节（0x20 口径）。
    pub cfg: Option<u8>,
    /// 控制器真发写计数（A1 期恒 0 断言）。
    pub write_count: u32,
    /// 写尝试计数（注入账，独立于真发写）。
    pub write_attempts: u32,
    /// 探测账（只追加）。
    pub rows: Vec<ProbeRow>,
    row_seq: u64,
    /// 封账指纹。
    pub sealed: Option<u32>,
    /// 注入记账 tries/caught。
    pub inj_tries: [u32; INJ_ROUTES],
    pub inj_caught: [u32; INJ_ROUTES],
}

impl Probe8042 {
    pub fn new() -> Probe8042 {
        Probe8042 {
            status: STATUS_FROZEN,
            ports: [PortLedger::new(PORT_DATA), PortLedger::new(PORT_CMD)],
            presence: None,
            cfg: None,
            write_count: 0,
            write_attempts: 0,
            rows: Vec::new(),
            row_seq: 0,
            sealed: None,
            inj_tries: [0; INJ_ROUTES],
            inj_caught: [0; INJ_ROUTES],
        }
    }

    fn push_row(&mut self, text: &str) {
        self.row_seq += 1;
        self.rows.push(ProbeRow { seq: self.row_seq, text: String::from(text) });
    }

    /// 轮询预备口径：读状态口（0x64 读 = 状态寄存器镜像，零写）。
    pub fn read_status(&mut self) -> u8 {
        if let Some(p) = self.ports.get_mut(1) {
            p.reads += 1;
            p.in_ledger = true;
        }
        self.status
    }

    /// 自检位读（bit2 系统标志位口径）。
    pub fn selftest_bit_set(&self) -> bool {
        self.status & (1 << STATUS_SELFTEST_BIT) != 0
    }

    /// OBF 就绪位读（bit0 轮询预备口径）。
    pub fn obf_ready(&self) -> bool {
        self.status & (1 << STATUS_OBF_BIT) != 0
    }

    /// 探测件一：存在判定（状态口 0x64 口径自检位）。
    pub fn probe_presence(&mut self) -> Result<bool, u32> {
        let s = self.read_status();
        let present = s & (1 << STATUS_SELFTEST_BIT) != 0;
        self.presence = Some(present);
        if present {
            self.push_row("存在判定·自检位=1·在位");
        } else {
            self.push_row("存在判定·自检位=0·不在位");
        }
        Ok(present)
    }

    /// 数据窗读（0x60 读 = 数据字节读口径，OBF 就绪前提，零写）。
    pub fn read_data_window(&mut self) -> Result<u8, u32> {
        if !self.obf_ready() {
            return Err(KbdCode::PortMissing.val());
        }
        if let Some(p) = self.ports.get_mut(0) {
            p.reads += 1;
            p.in_ledger = true;
        }
        Ok(0xD1)
    }

    /// 探测件二：双口态核验（0x60 数据 + 0x64 命令两窗口**已在账**）。
    pub fn probe_dual_ports(&mut self) -> Result<(), u32> {
        let (d_ok, c_ok) = match (self.ports.get(0), self.ports.get(1)) {
            (Some(d), Some(c)) => (d.in_ledger, c.in_ledger),
            _ => (false, false),
        };
        if d_ok && c_ok {
            self.push_row("双口态·0x60数据+0x64命令·两窗在账");
            Ok(())
        } else {
            self.push_row("双口态·缺窗·红");
            Err(KbdCode::PortMissing.val())
        }
    }

    /// 探测件三：配置字节（0x20 读命令口径，A1 期零写——命令只做
    /// 口径记账不真发，数据取冻结常量）。
    pub fn read_config(&mut self) -> Result<u8, u32> {
        // 0x20 口径记账（命令窗读口径，无写）。
        if let Some(p) = self.ports.get_mut(1) {
            p.reads += 1;
            p.in_ledger = true;
        }
        self.cfg = Some(CFG_FROZEN);
        self.push_row(&format!("配置字节·0x{:02X}口径·0x{:02X}", CMD_READ_CFG, CFG_FROZEN));
        Ok(CFG_FROZEN)
    }

    /// 控制器写路径（A1 期一律显性拒，真发写计数不增）。
    pub fn write_controller(&mut self, _port: u16, _val: u8) -> Result<(), u32> {
        self.write_attempts += 1;
        Err(KbdCode::A1WriteForbidden.val())
    }

    /// 探测三件账齐判定。
    pub fn three_ledgers_ok(&self) -> bool {
        let (d_ok, c_ok) = match (self.ports.get(0), self.ports.get(1)) {
            (Some(d), Some(c)) => (d.in_ledger, c.in_ledger),
            _ => (false, false),
        };
        self.presence.is_some() && d_ok && c_ok && self.cfg.is_some()
    }

    /// 探测账全文（封账面）。
    pub fn ledger_bytes(&self) -> Vec<u8> {
        let mut bytes: Vec<u8> = Vec::new();
        for row in &self.rows {
            for &b in row.text.as_bytes() {
                bytes.push(b);
            }
            bytes.push(0x0A);
        }
        bytes
    }

    /// 封账：FNV-1a 对账全文压缩。
    pub fn seal(&mut self) -> Result<u32, u32> {
        if !self.three_ledgers_ok() {
            return Err(KbdCode::PresenceUnknown.val());
        }
        let fp = fnv1a(&self.ledger_bytes());
        self.sealed = Some(fp);
        self.push_row(&format!("封账·指纹0x{:08X}", fp));
        Ok(fp)
    }

    /// 重放：复算指纹与封账逐字节对拍（末尾封账行自身不计入复算面）。
    pub fn replay(&self) -> Result<u32, u32> {
        let sealed = match self.sealed {
            Some(fp) => fp,
            None => return Err(KbdCode::ReplayMismatch.val()),
        };
        let mut bytes = self.ledger_bytes();
        // 末尾封账行不计入指纹复算（封账行随封账动作落账，不在封账面内）。
        if let Some(last) = self.rows.last() {
            if last.text.starts_with("封账") {
                let cut = bytes.len().saturating_sub(last.text.len() + 1);
                bytes.truncate(cut);
            }
        }
        let re = fnv1a(&bytes);
        if re == sealed {
            Ok(re)
        } else {
            Err(KbdCode::ReplayMismatch.val())
        }
    }

    /// 探测账只追加守卫：行号连续递增。
    pub fn ledger_append_only_ok(&self) -> bool {
        let mut prev: u64 = 0;
        for row in &self.rows {
            if row.seq != prev + 1 {
                return false;
            }
            prev = row.seq;
        }
        true
    }

    /// A1 期零写断言：真发写计数恒 0。
    pub fn zero_write_ok(&self) -> bool {
        let (dw, cw) = match (self.ports.get(0), self.ports.get(1)) {
            (Some(d), Some(c)) => (d.writes, c.writes),
            _ => (0, 0),
        };
        self.write_count == 0 && dw == 0 && cw == 0
    }

    /// 注入路演练：人为构造违约，必被闸抓。
    /// 路 0：控制器写注入；路 1：双口缺账注入；路 2：封账篡改注入。
    pub fn inject_fault(&mut self, route: usize) -> Result<bool, u32> {
        if route >= INJ_ROUTES {
            return Err(KbdCode::PortMissing.val());
        }
        match self.inj_tries.get_mut(route) {
            Some(t) => *t += 1,
            None => return Err(KbdCode::PortMissing.val()),
        }
        let caught = match route {
            0 => {
                // 控制器写注入：写路径必拒且真发写计数仍为 0。
                let r = self.write_controller(PORT_CMD, 0xAA);
                r.is_err() && self.zero_write_ok() && self.write_attempts >= 1
            }
            1 => {
                // 双口缺账注入：数据窗不在账 ⇒ 双口态探针必红。
                let mut rogue = Probe8042::new();
                if let Some(p) = rogue.ports.get_mut(0) {
                    p.in_ledger = false;
                }
                // 命令窗在账（读一次状态）。
                let _ = rogue.read_status();
                match rogue.probe_dual_ports() {
                    Err(e) => e == KbdCode::PortMissing.val(),
                    Ok(()) => false,
                }
            }
            _ => {
                // 封账篡改注入：封账后改一行账 ⇒ 重放必失配。
                if self.probe_all().is_err() {
                    return Err(KbdCode::PresenceUnknown.val());
                }
                let _ = self.seal()?;
                // 篡改第 0 行文本（人为违约；文本嵌入单调 seq，逐轮必变值）。
                let tag = self.row_seq;
                let tainted = match self.rows.get_mut(0) {
                    Some(row) => {
                        row.text = format!("篡改行·违约注入·seq{}", tag);
                        true
                    }
                    None => false,
                };
                if !tainted {
                    return Err(KbdCode::ReplayMismatch.val());
                }
                match self.replay() {
                    Err(e) => e == KbdCode::ReplayMismatch.val(),
                    Ok(_) => false,
                }
            }
        };
        if caught {
            if let Some(c) = self.inj_caught.get_mut(route) {
                *c += 1;
            }
        }
        Ok(caught)
    }

    /// 三件连探（正常流：存在判定 → 双口数据读 → 双口态核验 → 配置字节）。
    pub fn probe_all(&mut self) -> Result<(), u32> {
        self.probe_presence()?;
        let _ = self.read_data_window()?;
        self.probe_dual_ports()?;
        self.read_config()?;
        if !self.three_ledgers_ok() {
            return Err(KbdCode::PresenceUnknown.val());
        }
        Ok(())
    }
}

// ── 黄金全链：三件齐 → 零写断言 → 封账 → 重放一致 → 三路注入全抓 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut p = Probe8042::new();
    p.probe_all()?;
    let faces = (p.presence == Some(true)) as u32
        + (p.three_ledgers_ok() && true) as u32
        + (p.cfg == Some(CFG_FROZEN)) as u32;
    // A1 期零写断言。
    if !p.zero_write_ok() {
        return Err(KbdCode::A1WriteForbidden.val());
    }
    // 封账 + 重放。
    let fp0 = p.seal()?;
    p.replay()?;
    // 注入三路 10 轮全抓。
    let mut caught = 0;
    let mut route = 0;
    while route < INJ_ROUTES {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            if p.inject_fault(route)? {
                caught += 1;
            }
            r += 1;
        }
        route += 1;
    }
    // 封账后零写断言仍成立。
    if !p.zero_write_ok() {
        return Err(KbdCode::A1WriteForbidden.val());
    }
    Ok((faces, caught, fp0))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：3 面 + 30 抓 + 封账指纹非零。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((3, 30, fp)) if fp != 0 => Ok(()),
        _ => Err(KbdCode::NotReproducible.val()),
    }
}

/// 序 B：零写守卫——写路径恒拒且真发写计数恒 0（10 次尝试仍 0）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut p = Probe8042::new();
    p.probe_all()?;
    let mut n = 0;
    while n < 10 {
        if p.write_controller(PORT_DATA, 0xFF).is_ok() {
            return Err(KbdCode::NotReproducible.val());
        }
        n += 1;
    }
    if p.write_count != 0 || p.write_attempts != 10 {
        return Err(KbdCode::NotReproducible.val());
    }
    Ok(())
}

/// 序 C：未封账重放必拒（ReplayMismatch）。
pub fn probe_seq_c() -> Result<(), u32> {
    let p = Probe8042::new();
    match p.replay() {
        Err(e) if e == KbdCode::ReplayMismatch.val() => Ok(()),
        _ => Err(KbdCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    // 种子口径：探测种子冻结 = SEG_BASE（双跑同源前提）。
    if machine_seed() != SEG_BASE as u64 {
        return Err(KbdCode::NotReproducible.val());
    }
    // 同口径两台探测机三件账与指纹全等。
    let mut a = Probe8042::new();
    let mut b = Probe8042::new();
    a.probe_all()?;
    b.probe_all()?;
    let fa = a.seal()?;
    let fb = b.seal()?;
    if fa != fb || a.rows != b.rows || a.ports != b.ports {
        return Err(KbdCode::NotReproducible.val());
    }
    let g1 = golden_run()?;
    let g2 = golden_run()?;
    if g1 != g2 {
        return Err(KbdCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < KbdCode::ALL.len() {
        if let Some(c) = KbdCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// J1：存在判定落账（自检位口径）。
fn presence_ok() -> bool {
    let mut p = Probe8042::new();
    match p.probe_presence() {
        Ok(true) => p.selftest_bit_set() && p.obf_ready() && p.presence == Some(true),
        _ => false,
    }
}

/// J1：双口态在账（先读两窗再核验）。
fn dual_ports_ok() -> bool {
    let mut p = Probe8042::new();
    let _ = p.read_status();
    let data = p.read_data_window();
    match (data, p.probe_dual_ports()) {
        (Ok(_), Ok(())) => match (p.ports.get(0), p.ports.get(1)) {
            (Some(d), Some(c)) => d.in_ledger && c.in_ledger && d.port == PORT_DATA && c.port == PORT_CMD,
            _ => false,
        },
        _ => false,
    }
}

/// J1：配置字节 0x20 口径。
fn cfg_ok() -> bool {
    let mut p = Probe8042::new();
    match p.read_config() {
        Ok(v) => v == CFG_FROZEN && p.cfg == Some(CFG_FROZEN),
        Err(_) => false,
    }
}

/// J1：A1 期零控制器写断言。
fn zero_write_ok() -> bool {
    let mut p = Probe8042::new();
    if p.probe_all().is_err() {
        return false;
    }
    if p.write_controller(PORT_CMD, 0x60).is_ok() {
        return false;
    }
    p.write_count == 0 && p.zero_write_ok()
}

/// J1：封账指纹重放一致。
fn seal_replay_ok() -> bool {
    let mut p = Probe8042::new();
    if p.probe_all().is_err() {
        return false;
    }
    match p.seal() {
        Ok(fp) => match p.replay() {
            Ok(re) => re == fp && fp != 0,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// J1：探测三件账齐。
fn three_ok() -> bool {
    let mut p = Probe8042::new();
    if p.probe_all().is_err() {
        return false;
    }
    p.three_ledgers_ok() && p.ledger_append_only_ok() && p.rows.len() == 3
}

// ── 判据面（extra_checks：全 add_dyn） ──────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J0-段位守卫",
        segment_guard(),
        format!("SEG_BASE=0x{:X} 窗口 0x75000..=0x781C0 且 0x40 对齐", SEG_BASE),
    ));
    v.push((
        "J1-端口冻结表",
        PORT_DATA == 0x60 && PORT_CMD == 0x64 && CMD_READ_CFG == 0x20 && CFG_FROZEN == 0x45,
        String::from("0x60/0x64/0x20/0x45（8042 手册口径）"),
    ));
    v.push((
        "J1-存在判定",
        presence_ok(),
        String::from("0x64 状态口自检位 bit2 + OBF bit0 轮询预备口径"),
    ));
    v.push((
        "J1-双口态",
        dual_ports_ok(),
        String::from("0x60 数据窗 + 0x64 命令窗 两窗在账"),
    ));
    v.push((
        "J1-配置字节口径",
        cfg_ok(),
        format!("0x20 读命令口径 冻结配置 0x{:02X}", CFG_FROZEN),
    ));
    v.push((
        "J1-A1期零写断言",
        zero_write_ok(),
        String::from("写路径恒拒 真发写计数恒 0"),
    ));
    v.push((
        "J1-三件账齐",
        three_ok(),
        String::from("存在/双口/配置 三件齐 账行号连续"),
    ));
    v.push((
        "J1-封账指纹重放",
        seal_replay_ok(),
        String::from("FNV 封账 重放复算逐字节一致"),
    ));
    // 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((faces, caught, fp)) => *faces == 3 && *caught == 30 && *fp != 0,
            Err(_) => false,
        },
        match &g {
            Ok((faces, caught, fp)) => format!("faces={} caught={}/30 fp=0x{:08X}", faces, caught, fp),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 注入三路 10 轮。
    let mut p = Probe8042::new();
    let mut tries = 0;
    let mut caught = 0;
    let mut inj_ok = true;
    let mut route = 0;
    while route < INJ_ROUTES {
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            tries += 1;
            match p.inject_fault(route) {
                Ok(true) => caught += 1,
                _ => inj_ok = false,
            }
            r += 1;
        }
        route += 1;
    }
    v.push((
        "J1-注入10/10",
        inj_ok && caught == tries,
        format!("t/c {}/{} 三路（写/缺窗/篡改）", caught, tries),
    ));
    // 零写守卫探针。
    v.push((
        "J2-零写守卫探针",
        probe_seq_b().is_ok(),
        String::from("10 次写尝试全拒 真发写计数仍 0"),
    ));
    // 探针序 A/C。
    v.push(("J2-探针序A", probe_seq_a().is_ok(), String::from("3面+30抓+指纹非零")));
    v.push((
        "J2-未封账重放拒",
        probe_seq_c().is_ok(),
        String::from("未封账重放 ⇒ ReplayMismatch"),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("同口径两机账/指纹全等 + golden 两跑同值"),
    ));
    // 诊断码闭集。
    v.push((
        "J3-诊断码闭集",
        detail(SEG_BASE + 0x3F).starts_with("未知") && detail(KbdCode::A1WriteForbidden.val()) == KbdCode::A1WriteForbidden.detail(),
        String::from("闭集查表命中 + 窗口外未知"),
    ));
    v
}

pub fn run_unx0506_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0506 8042 键盘控制器探测账（只读预备）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golden_chain_all_green() {
        let g = golden_run();
        match g {
            Ok((faces, caught, fp)) => {
                assert_eq!(faces, 3);
                assert_eq!(caught, 30);
                assert!(fp != 0);
            }
            Err(e) => panic!("黄金链应全绿 err=0x{:x}", e),
        }
    }

    #[test]
    fn three_ledgers_present() {
        let mut p = Probe8042::new();
        assert!(p.probe_all().is_ok());
        assert!(p.three_ledgers_ok());
        assert_eq!(p.presence, Some(true));
        assert_eq!(p.cfg, Some(CFG_FROZEN));
        // 账行：三件 + 封账前 3 行。
        assert_eq!(p.rows.len(), 3);
        let mut expect = 1u64;
        for row in &p.rows {
            assert_eq!(row.seq, expect);
            assert!(!row.text.is_empty());
            expect += 1;
        }
    }

    #[test]
    fn a1_zero_write_invariant() {
        let mut p = Probe8042::new();
        p.probe_all().ok();
        let mut n = 0;
        while n < 16 {
            let port = match n % 2 {
                0 => PORT_DATA,
                _ => PORT_CMD,
            };
            assert_eq!(p.write_controller(port, 0xD1), Err(KbdCode::A1WriteForbidden.val()));
            assert!(p.zero_write_ok(), "真发写计数必须恒 0");
            n += 1;
        }
        assert_eq!(p.write_attempts, 16);
        assert_eq!(p.write_count, 0);
    }

    #[test]
    fn seal_replay_roundtrip() {
        let mut p = Probe8042::new();
        p.probe_all().ok();
        let fp = p.seal().ok().unwrap_or(0);
        assert!(fp != 0);
        assert_eq!(p.replay(), Ok(fp));
        // 封账行落账。
        assert_eq!(p.rows.len(), 4);
        assert!(p.ledger_append_only_ok());
    }

    #[test]
    fn tamper_injection_caught() {
        let mut p = Probe8042::new();
        p.probe_all().ok();
        let _ = p.seal();
        // 篡改账行 → 重放必失配。
        if let Some(row) = p.rows.get_mut(1) {
            row.text = String::from("被篡改的双口态行");
        }
        assert_eq!(p.replay(), Err(KbdCode::ReplayMismatch.val()));
    }

    #[test]
    fn all_injection_routes_caught() {
        let mut p = Probe8042::new();
        let mut route = 0;
        while route < INJ_ROUTES {
            let mut r = 0;
            while r < JUDGE_ROUNDS {
                match p.inject_fault(route) {
                    Ok(true) => {}
                    other => panic!("注入路{}轮{} 必抓，实得 {:?}", route, r, other),
                }
                r += 1;
            }
            route += 1;
        }
        let mut i = 0;
        while i < INJ_ROUTES {
            let (t, c) = match (p.inj_tries.get(i), p.inj_caught.get(i)) {
                (Some(t), Some(c)) => (*t, *c),
                _ => (0, 0),
            };
            assert_eq!(c, t, "路{}全抓", i);
            assert_eq!(t, JUDGE_ROUNDS);
            i += 1;
        }
    }

    #[test]
    fn dual_port_missing_caught() {
        let mut rogue = Probe8042::new();
        // 数据窗不在账直接探双口态 → 必红。
        match rogue.probe_dual_ports() {
            Err(e) => assert_eq!(e, KbdCode::PortMissing.val()),
            Ok(()) => panic!("缺窗必须红"),
        }
    }

    #[test]
    fn double_run_identical() {
        assert!(repro_check().is_ok());
        // 指纹确定性。
        let fp1 = fnv1a(b"0x76340-kbd8042");
        let fp2 = fnv1a(b"0x76340-kbd8042");
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn unsealed_replay_rejected() {
        let p = Probe8042::new();
        assert_eq!(p.replay(), Err(KbdCode::ReplayMismatch.val()));
    }

    #[test]
    fn segment_window_guard() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        let mut i = 0;
        while i < KbdCode::ALL.len() {
            if let Some(c) = KbdCode::ALL.get(i) {
                let v = c.val();
                assert!(v >= SEG_BASE && v < SEG_BASE + 0x40);
            }
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知"));
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }
}
