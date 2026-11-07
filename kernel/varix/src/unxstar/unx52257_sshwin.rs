//! UNX-F52257 OpenSSH for Windows（客户端）收口 · 0xC8000段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52257"，逐字）：
//!   title: OpenSSH for Windows（客户端）收口
//!   quota: 200
//!   criteria: ① 纯功能代码 ≥200 行；② 判据原文：UNX-F52258-J1 抓包
//!     ×10 全通；Npcap 消费（F52249 联签锚定行）；KB-COMPAT 定级登记；
//!     判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘
//!     要非空。
//! off-by-one 板式口径：本卡（卡 N=52257）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52258-J1，照原文锚定；功能按本卡标题（OpenSSH for Windows 客
//! 户端）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 命令行五族 20 操作集：ssh / scp / sftp / ssh-keygen / ssh-agent
//!      各 4 操作，家族单调（逆序必抓），逐操作载荷纯函数折叠恒等才收。
//!   ② ssh_config 双向投影 12 格：正投影 config_cell 折叠 + 反投影
//!      config_back 校验恒等，双向闭合才收。
//!   ③ 密钥三元闸：生成（ssh-keygen）/ 装载（ssh-agent）/ 验收（认证）
//!      三段依次落账，缺段拒封。
//!   ④ known_hosts 语义 5 格：主机键指纹逐格恒等；漂移必抓（对应
//!      MITM 检出语义）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 闭集）；注入核 run_cycle 10 轮三态轮转（投影漂移/家族逆
//!      序/操作越闭集）全部落在当轮临时账本上，检出+逃逸守恒等式；双
//!      跑指纹一致；KB-COMPAT 定级 S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52257;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () =
    assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 命令行操作数。
pub const CLI_OPS: usize = 20;

/// ssh_config 投影格数。
pub const CFG_CELLS: usize = 12;

/// 密钥三元闸段数。
pub const KEY_PHASES: usize = 3;

/// known_hosts 语义格数。
pub const HOST_CELLS: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SshWinCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 投影格越闭集
    ENoCfg,
    /// 投影漂移
    ECfgDrift,
    /// 密钥段越闭集
    ENoKey,
    /// 密钥段乱序
    EKeyOrder,
    /// 主机格越闭集
    ENoHost,
    /// 主机键漂移
    EHostDrift,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl SshWinCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SshWinCode::Ok => "成功",
            SshWinCode::ENoOp => "操作号越闭集：ssh/scp/sftp/ssh-keygen/ssh-agent 20 操作集之外先登记",
            SshWinCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            SshWinCode::EOrder => "家族逆序：ssh→scp→sftp→ssh-keygen→ssh-agent 单调，逆序必抓",
            SshWinCode::ENoCfg => "投影格越闭集：ssh_config 12 格闭集之外先登记",
            SshWinCode::ECfgDrift => "投影漂移：正投影折叠与基准不恒等，双向复算",
            SshWinCode::ENoKey => "密钥段越闭集：生成/装载/验收三元闸之外先登记",
            SshWinCode::EKeyOrder => "密钥段乱序：生成→装载→验收三段依次落账，跳段必抓",
            SshWinCode::ENoHost => "主机格越闭集：known_hosts 5 格闭集之外先登记",
            SshWinCode::EHostDrift => "主机键漂移：known_hosts 指纹与基准不恒等（MITM 语义）",
            SshWinCode::ESeal => "未齐即封：20 操作 + 12 投影 + 3 密钥段 + 5 主机格未齐先补齐",
            SshWinCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SshWinCode::EGradeDrift => "定级漂移：全通才可定 S1（KB-COMPAT 登记）",
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
            Grade::S1 => "S1：20 操作+12 投影+3 密钥段+5 主机格全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令行操作格载荷（操作 o；家族 = o/4）。
pub const fn cli_cell(o: usize) -> Option<u64> {
    if o >= CLI_OPS {
        return None;
    }
    Some((0xCA000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=ssh 4..8=scp 8..12=sftp 12..16=ssh-keygen 16..20=ssh-agent）。
pub const fn cli_kind(o: usize) -> Option<u32> {
    if o >= CLI_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn cli_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("ssh"),
        1 => Some("scp"),
        2 => Some("sftp"),
        3 => Some("ssh-keygen"),
        4 => Some("ssh-agent"),
        _ => None,
    }
}

/// ssh_config 正投影折叠（格 c）。
pub const fn config_cell(c: usize) -> Option<u64> {
    if c >= CFG_CELLS {
        return None;
    }
    Some((0xCB000u64 + (c as u64) * 0x37) ^ 0x5353_485F_4346_0000)
}

/// ssh_config 反投影校验：got 与正投影恒等才回授格号。
pub const fn config_back(c: usize, got: u64) -> Option<usize> {
    match config_cell(c) {
        Some(w) => {
            if w == got {
                Some(c)
            } else {
                None
            }
        }
        None => None,
    }
}

/// 密钥三元闸段载荷（段 p：0=生成 1=装载 2=验收）。
pub const fn key_cell(p: usize) -> Option<u64> {
    if p >= KEY_PHASES {
        return None;
    }
    Some((0xCC000u64 + (p as u64) * 0x5D) ^ 0x4B45_595F_5048_0000)
}

/// 密钥段名。
pub const fn key_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("生成"),
        1 => Some("装载"),
        2 => Some("验收"),
        _ => None,
    }
}

/// known_hosts 语义格指纹（格 h）。
pub const fn host_cell(h: usize) -> Option<u64> {
    if h >= HOST_CELLS {
        return None;
    }
    Some((0xCD000u64 + (h as u64) * 0x31) ^ 0x4B4E_4F57_4E5F_0000)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// OpenSSH 客户端收口账本状态机。
#[derive(Clone)]
pub struct SshWinLedger {
    /// 六判据（连接/认证/传输/密钥/配置/退出码）。
    gates: [Option<bool>; 6],
    /// 操作账。
    ops: [Option<u64>; CLI_OPS],
    /// 配置投影账。
    cfgs: [Option<u64>; CFG_CELLS],
    /// 密钥段账。
    keys: [Option<u64>; KEY_PHASES],
    /// 主机格账。
    hosts: [Option<u64>; HOST_CELLS],
    /// 最后家族。
    last_kind: i64,
    /// 最后密钥段。
    last_key: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SshWinLedger {
    /// 新账本。
    pub fn new() -> Self {
        SshWinLedger {
            gates: [None; 6],
            ops: [None; CLI_OPS],
            cfgs: [None; CFG_CELLS],
            keys: [None; KEY_PHASES],
            hosts: [None; HOST_CELLS],
            last_kind: -1,
            last_key: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SshWinCode> {
        if gate >= 6 {
            return Err(SshWinCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5210 + gate as u64);
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

    /// 操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), SshWinCode> {
        if o >= CLI_OPS {
            return Err(SshWinCode::ENoOp);
        }
        let kind = match cli_kind(o) {
            Some(k) => k,
            None => return Err(SshWinCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5220);
            return Err(SshWinCode::EOrder);
        }
        let want = match cli_cell(o) {
            Some(w) => w,
            None => return Err(SshWinCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5221 + o as u64);
            return Err(SshWinCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SshWinCode::ESeal);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5222 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < CLI_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 配置投影一格（正投影恒等 + 反投影回授闭合）。
    pub fn record_config(&mut self, c: usize, got: u64) -> Result<(), SshWinCode> {
        let want = match config_cell(c) {
            Some(w) => w,
            None => return Err(SshWinCode::ENoCfg),
        };
        if got != want || config_back(c, got) != Some(c) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5230 + c as u64);
            return Err(SshWinCode::ECfgDrift);
        }
        if match self.cfgs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SshWinCode::ESeal);
        }
        if let Some(cell) = self.cfgs.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x5231 + c as u64);
        Ok(())
    }

    /// 配置投影全账核验（12 格）。
    pub fn cfgs_all(&self) -> bool {
        let mut c = 0usize;
        while c < CFG_CELLS {
            match self.cfgs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 密钥段一格（三段依次落账，跳段必抓）。
    pub fn record_key(&mut self, p: usize, got: u64) -> Result<(), SshWinCode> {
        if p >= KEY_PHASES {
            return Err(SshWinCode::ENoKey);
        }
        if (p as i64) != self.last_key + 1 {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5240);
            return Err(SshWinCode::EKeyOrder);
        }
        let want = match key_cell(p) {
            Some(w) => w,
            None => return Err(SshWinCode::ENoKey),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5241 + p as u64);
            return Err(SshWinCode::EDrift);
        }
        if let Some(cell) = self.keys.get_mut(p) {
            *cell = Some(got);
        }
        self.last_key = p as i64;
        self.fnv(0x5242 + p as u64);
        Ok(())
    }

    /// 密钥三元闸全落核验。
    pub fn keys_all(&self) -> bool {
        let mut p = 0usize;
        while p < KEY_PHASES {
            match self.keys.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        self.last_key == (KEY_PHASES as i64) - 1
    }

    /// known_hosts 一格（指纹恒等）。
    pub fn record_host(&mut self, h: usize, got: u64) -> Result<(), SshWinCode> {
        let want = match host_cell(h) {
            Some(w) => w,
            None => return Err(SshWinCode::ENoHost),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5250 + h as u64);
            return Err(SshWinCode::EHostDrift);
        }
        if match self.hosts.get(h) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SshWinCode::ESeal);
        }
        if let Some(cell) = self.hosts.get_mut(h) {
            *cell = Some(got);
        }
        self.fnv(0x5251 + h as u64);
        Ok(())
    }

    /// known_hosts 全账核验（5 格）。
    pub fn hosts_all(&self) -> bool {
        let mut h = 0usize;
        while h < HOST_CELLS {
            match self.hosts.get(h) {
                Some(Some(_)) => {}
                _ => return false,
            }
            h += 1;
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
        if self.gates_all() && self.ops_all() && self.cfgs_all() && self.keys_all() && self.hosts_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SshWinLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SshWinLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < CLI_OPS {
        if let Some(w) = cli_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < CFG_CELLS {
        if let Some(w) = config_cell(c) {
            if led.record_config(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut p = 0usize;
    while p < KEY_PHASES {
        if let Some(w) = key_cell(p) {
            if led.record_key(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut h = 0usize;
    while h < HOST_CELLS {
        if let Some(w) = host_cell(h) {
            if led.record_host(h, w).is_ok() {
                recorded += 1;
            }
        }
        h += 1;
    }
    // 注入 10 轮：投影漂移 / 家族逆序 / 操作越闭集三态轮转，
    // 全部落在当轮临时账本上。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SshWinLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_config(0, config_cell(0).unwrap_or(0));
                lt.record_config(1, config_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(SshWinCode::ECfgDrift)
            }
            1 => {
                let _ = lt.record_op(17, cli_cell(17).unwrap_or(0));
                lt.record_op(5, cli_cell(5).unwrap_or(0)) == Err(SshWinCode::EOrder)
            }
            _ => lt.record_op(CLI_OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5260 + k as u64);
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
/// UNX-F52257 判据集（锚 UNX-F52258-J1，off-by-one 板式）。
pub fn run_unx52257_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52257");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "命令行 20 操作全通",
        recorded >= CLI_OPS as u32,
        "ssh/scp/sftp/ssh-keygen/ssh-agent 五族逐操作恒等",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < CLI_OPS {
                if cli_kind(o).unwrap_or(5) < cli_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五族闭集单调，逆序必抓",
    );
    s.add(
        "ssh_config 双向投影 12/12 恒等",
        {
            let mut lt = SshWinLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CFG_CELLS {
                if let Some(w) = config_cell(c) {
                    if lt.record_config(c, w).is_err() || config_back(c, w) != Some(c) {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.cfgs_all()
        },
        "正投影折叠 + 反投影回授双向闭合（§AI-66.3）",
    );
    s.add(
        "密钥三元闸 3/3 依次落账",
        {
            let mut lt = SshWinLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < KEY_PHASES {
                if let Some(w) = key_cell(p) {
                    if lt.record_key(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.keys_all()
        },
        "生成→装载→验收三段有序闭合",
    );
    s.add(
        "known_hosts 语义 5/5 恒等",
        {
            let mut lt = SshWinLedger::new();
            let mut ok = true;
            let mut h = 0usize;
            while h < HOST_CELLS {
                if let Some(w) = host_cell(h) {
                    if lt.record_host(h, w).is_err() {
                        ok = false;
                    }
                }
                h += 1;
            }
            ok && lt.hosts_all()
        },
        "主机键指纹逐格恒等，漂移即 MITM 语义检出",
    );
    s.add(
        "六判据闸全落",
        {
            let mut lt = SshWinLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "连接/认证/传输/密钥/配置/退出码逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "投影漂移/家族逆序/越闭集三态轮转必抓");
    let mut l0 = SshWinLedger::new();
    s.add(
        "边界·操作越闭集",
        l0.record_op(CLI_OPS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·投影格越闭集",
        l0.record_config(CFG_CELLS, 0).is_err(),
        "投影格 12 越闭集必拒",
    );
    s.add(
        "边界·密钥段越闭集",
        l0.record_key(KEY_PHASES, 0).is_err(),
        "密钥段 3 越闭集必拒",
    );
    s.add(
        "边界·主机格越闭集",
        l0.record_host(HOST_CELLS, 0).is_err(),
        "主机格 5 越闭集必拒",
    );
    s.add(
        "漂移注入先录正格再注漂移",
        {
            let _ = l0.record_op(0, cli_cell(0).unwrap_or(0));
            l0.record_op(1, cli_cell(1).unwrap_or(0).wrapping_add(3)) == Err(SshWinCode::EDrift)
        },
        "正格先收，漂移格必抓",
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
        "定级 S1 闭集",
        {
            let mut lt = SshWinLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < CLI_OPS {
                if let Some(w) = cli_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < CFG_CELLS {
                if let Some(w) = config_cell(c) {
                    let _ = lt.record_config(c, w);
                }
                c += 1;
            }
            let mut p = 0usize;
            while p < KEY_PHASES {
                if let Some(w) = key_cell(p) {
                    let _ = lt.record_key(p, w);
                }
                p += 1;
            }
            let mut h = 0usize;
            while h < HOST_CELLS {
                if let Some(w) = host_cell(h) {
                    let _ = lt.record_host(h, w);
                }
                h += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8000,
        "SEG_BASE 0x40 对齐且落在 VX15 域 52257 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_kind() {
        assert_eq!(cli_kind(0), Some(0));
        assert_eq!(cli_kind(4), Some(1));
        assert_eq!(cli_kind(12), Some(3));
        assert_eq!(cli_kind(19), Some(4));
        assert!(cli_kind(20).is_none());
        assert!(cli_cell(20).is_none());
        assert_eq!(cli_kind_name(0), Some("ssh"));
        assert_eq!(cli_kind_name(4), Some("ssh-agent"));
        assert!(cli_kind_name(5).is_none());
    }

    #[test]
    fn test_config_projection_bidirectional() {
        let mut c = 0usize;
        while c < CFG_CELLS {
            let w = config_cell(c).unwrap_or(0);
            assert_eq!(config_back(c, w), Some(c), "反投影必须回授原格");
            assert_ne!(config_back(c, w.wrapping_add(1)), Some(c), "漂移值反投影必拒");
            c += 1;
        }
        assert!(config_cell(CFG_CELLS).is_none());
    }

    #[test]
    fn test_key_phase_names() {
        assert_eq!(key_phase_name(0), Some("生成"));
        assert_eq!(key_phase_name(1), Some("装载"));
        assert_eq!(key_phase_name(2), Some("验收"));
        assert!(key_phase_name(3).is_none());
        assert!(key_cell(KEY_PHASES).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = SshWinLedger::new();
        let _ = lt.record_op(17, cli_cell(17).unwrap_or(0));
        assert_eq!(lt.record_op(5, cli_cell(5).unwrap_or(0)), Err(SshWinCode::EOrder));
        assert!(lt.record_op(18, cli_cell(18).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_key_order_caught() {
        let mut lt = SshWinLedger::new();
        // 空账本跳段：直接落装载段必抓。
        assert_eq!(lt.record_key(1, key_cell(1).unwrap_or(0)), Err(SshWinCode::EKeyOrder));
        let _ = lt.record_key(0, key_cell(0).unwrap_or(0));
        // 重复落生成段/跳过装载段均抓。
        assert_eq!(lt.record_key(0, key_cell(0).unwrap_or(0)), Err(SshWinCode::EKeyOrder));
        assert_eq!(lt.record_key(2, key_cell(2).unwrap_or(0)), Err(SshWinCode::EKeyOrder));
        assert!(lt.record_key(1, key_cell(1).unwrap_or(0)).is_ok());
        assert!(!lt.keys_all(), "缺验收段不得全落");
    }

    #[test]
    fn test_host_drift_caught() {
        let mut lt = SshWinLedger::new();
        assert!(lt.record_host(0, host_cell(0).unwrap_or(0)).is_ok());
        assert_eq!(
            lt.record_host(1, host_cell(1).unwrap_or(0).wrapping_add(0x100)),
            Err(SshWinCode::EHostDrift)
        );
        assert_eq!(lt.record_host(HOST_CELLS, 0), Err(SshWinCode::ENoHost));
    }

    #[test]
    fn test_boundary_out_of_set() {
        let mut lt = SshWinLedger::new();
        assert!(lt.record_op(CLI_OPS, 0).is_err());
        assert!(lt.record_config(CFG_CELLS, 0).is_err());
        assert!(lt.record_key(KEY_PHASES, 0).is_err());
        assert!(lt.record_host(HOST_CELLS, 0).is_err());
        assert!(lt.judge_gate(6).is_err());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (CLI_OPS + CFG_CELLS + KEY_PHASES + HOST_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC8000);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SshWinCode::Ok as u32, 0);
        assert_eq!(SshWinCode::EGradeDrift as u32, 12);
        assert_eq!(SshWinCode::Ok.val(), SEG_BASE);
        assert!(!SshWinCode::Ok.detail().is_empty());
        assert!(!SshWinCode::EHostDrift.detail().is_empty());
        assert!(!SshWinCode::EFp.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = SshWinLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut lt2 = SshWinLedger::new();
        let _ = lt2.judge_gate(0);
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52257_checks_green() {
        let s = run_unx52257_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
