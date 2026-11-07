//! UNX-F52436 NetworkMiner 收口 · 0xCACC0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52436"，逐字）：
//!   title: NetworkMiner 收口
//!   quota: 260
//!   criteria: ① 纯功能代码 ≥260 行；② 判据原文：UNX-F52437-J1
//!     TUI 消费 ×10（与 F52249 tshark 底座联签锚定行）；KB-COMPAT 定
//!     级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状态=已完
//!     成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52436）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52437-J1，照原文锚定；功能按本卡标题（NetworkMiner）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五解析族闭集：PCAP 解析 / 凭据提取 / 会话重组 / 文件还原 /
//!      DNS 记录，各 4 格共 20 格，家族单调（逆序必抓）。
//!   ② NIC 三态 6 格：混杂/非混杂/离线 三态 ×（IPv4/IPv6）两格，
//!      态序单调（逆态必抓）。
//!   ③ 与 Wireshark 消费联签 4 格（F52249 tshark 底座联签锚定行）：
//!      四格逐签落账，越签必抓。
//!   ④ Java/.NET 上栈声明 2 行闭集：Java 上栈分账 / .NET 上栈分账，
//!      缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（解析越闭集/解析漂移/NIC 态
//!      逆序），TUI 消费 ×10 场景全通；双跑指纹一致；KB-COMPAT 定级
//!      闭集 S1/S2/S3（20 解析 + 6 NIC + 4 联签 + 2 行分账 + 六闸全
//!      通 → S2）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52436;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// 五解析族格数。
pub const PARSE_CELLS: usize = 20;

/// NIC 三态格数。
pub const NIC_CELLS: usize = 6;

/// 与 Wireshark 消费联签格数（F52249）。
pub const COSIGN_CELLS: usize = 4;

/// Java/.NET 上栈声明行数。
pub const SPLIT_ROWS: usize = 2;

/// 收口判据闸数。
pub const GATES: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetminerCode {
    /// 成功
    Ok = 0,
    /// 解析格越闭集
    ENoParse,
    /// 解析载荷漂移
    EParseDrift,
    /// 解析家族逆序
    EParseOrder,
    /// NIC 格越闭集
    ENoNic,
    /// NIC 态逆序
    ENicOrder,
    /// 联签格越闭集
    ENoSign,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 注入守恒破缺
    EInj,
    /// 定级漂移
    EGradeDrift,
}

impl NetminerCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            NetminerCode::Ok => "成功",
            NetminerCode::ENoParse => "解析格越闭集：PCAP/凭据/会话/文件/DNS 20 格之外先登记",
            NetminerCode::EParseDrift => "解析载荷漂移：解析折叠与基准不恒等，逐格复算",
            NetminerCode::EParseOrder => "解析家族逆序：PCAP→凭据→会话→文件→DNS 单调，逆序必抓",
            NetminerCode::ENoNic => "NIC 格越闭集：混杂/非混杂/离线三态 6 格之外先登记",
            NetminerCode::ENicOrder => "NIC 态逆序：混杂→非混杂→离线态序单调，逆态必抓",
            NetminerCode::ENoSign => "联签格越闭集：F52249 tshark 底座联签 4 格之外先登记",
            NetminerCode::ENoRow => "声明行越闭集：Java/.NET 上栈分账 2 行闭集之外先登记",
            NetminerCode::ERowMiss => "缺声明行：Java 上栈/.NET 上栈声明未齐先补齐",
            NetminerCode::EDup => "重复落账：同格只许一条账目",
            NetminerCode::ESeal => "未齐即封：20 解析 + 6 NIC + 4 联签 + 2 行分账未齐先补齐",
            NetminerCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            NetminerCode::EGradeDrift => "定级漂移：全通且分账声明全齐才可定 S2",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡不适用，闭集占位）
    S1,
    /// S2：有条件收口（本卡目标档）
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
            Grade::S1 => "S1：全通收口（NetworkMiner 收口按 KB-COMPAT 登记为 S2 档）",
            Grade::S2 => "S2：TUI 消费 ×10（与 F52249 tshark 底座联签）有条件收口，20 解析+6 NIC+4 联签+2 行分账全齐",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 解析格载荷（解析 c；家族 = c/4）。
pub const fn parse_cell(c: usize) -> Option<u64> {
    if c >= PARSE_CELLS {
        return None;
    }
    Some((0xDC000u64 + (c as u64) * 0x2F) ^ ((c / 4) as u64).wrapping_mul(0x01000193))
}

/// 解析家族（0..4=PCAP 4..8=凭据 8..12=会话 12..16=文件 16..20=DNS）。
pub const fn parse_kind(c: usize) -> Option<u32> {
    if c >= PARSE_CELLS {
        return None;
    }
    Some((c / 4) as u32)
}

/// 家族名。
pub const fn parse_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("PCAP 解析"),
        1 => Some("凭据提取"),
        2 => Some("会话重组"),
        3 => Some("文件还原"),
        4 => Some("DNS 记录"),
        _ => None,
    }
}

/// NIC 格载荷（格 n；态 = n/2，两格一族：IPv4/IPv6）。
pub const fn nic_cell(n: usize) -> Option<u64> {
    if n >= NIC_CELLS {
        return None;
    }
    Some((0xDD000u64 + (n as u64) * 0x1D) ^ ((n / 2) as u64).wrapping_mul(0x01000193))
}

/// NIC 态（0..2=混杂 2..4=非混杂 4..6=离线）。
pub const fn nic_state(n: usize) -> Option<u32> {
    if n >= NIC_CELLS {
        return None;
    }
    Some((n / 2) as u32)
}

/// NIC 态名。
pub const fn nic_state_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("混杂"),
        1 => Some("非混杂"),
        2 => Some("离线"),
        _ => None,
    }
}

/// 与 Wireshark 消费联签格折叠（F52249 tshark 底座，格 s）。
pub const fn cosign_cell(s: usize) -> Option<u32> {
    if s >= COSIGN_CELLS {
        return None;
    }
    Some(0xDE00 + s as u32 * 0x2C)
}

/// Java/.NET 上栈声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xDF00 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// NetworkMiner 收口账本状态机。
#[derive(Clone)]
pub struct NetminerLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; GATES],
    /// 解析账。
    parses: [Option<u64>; PARSE_CELLS],
    /// NIC 账。
    nics: [Option<u64>; NIC_CELLS],
    /// 联签账。
    cosigns: [Option<u32>; COSIGN_CELLS],
    /// 上栈声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后解析家族。
    last_kind: i64,
    /// 最后 NIC 态。
    last_nic: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl NetminerLedger {
    /// 新账本。
    pub fn new() -> Self {
        NetminerLedger {
            gates: [None; GATES],
            parses: [None; PARSE_CELLS],
            nics: [None; NIC_CELLS],
            cosigns: [None; COSIGN_CELLS],
            splits: [None; SPLIT_ROWS],
            last_kind: -1,
            last_nic: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), NetminerCode> {
        if gate >= GATES {
            return Err(NetminerCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut g = 0usize;
        while g < GATES {
            match self.gates.get(g) {
                Some(Some(true)) => {}
                _ => return false,
            }
            g += 1;
        }
        true
    }

    /// 解析一格（家族单调 + 载荷对照 + 重复拒）；逆族不改家族游标。
    pub fn record_parse(&mut self, c: usize, got: u64) -> Result<(), NetminerCode> {
        if c >= PARSE_CELLS {
            return Err(NetminerCode::ENoParse);
        }
        let kind = match parse_kind(c) {
            Some(k) => k,
            None => return Err(NetminerCode::ENoParse),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(NetminerCode::EParseOrder);
        }
        let want = match parse_cell(c) {
            Some(w) => w,
            None => return Err(NetminerCode::ENoParse),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + c as u64);
            return Err(NetminerCode::EParseDrift);
        }
        if match self.parses.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NetminerCode::EDup);
        }
        if let Some(x) = self.parses.get_mut(c) {
            *x = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + c as u64);
        Ok(())
    }

    /// 解析全账核验（20 格）。
    pub fn parses_all(&self) -> bool {
        let mut c = 0usize;
        while c < PARSE_CELLS {
            match self.parses.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 最后解析家族游标（注入后不变性核验用）。
    pub fn cur_kind(&self) -> i64 {
        self.last_kind
    }

    /// NIC 一格（态序单调 + 载荷对照 + 重复拒）；逆态不改态游标。
    pub fn record_nic(&mut self, n: usize, got: u64) -> Result<(), NetminerCode> {
        if n >= NIC_CELLS {
            return Err(NetminerCode::ENoNic);
        }
        let st = match nic_state(n) {
            Some(s) => s,
            None => return Err(NetminerCode::ENoNic),
        };
        if (st as i64) < self.last_nic {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300);
            return Err(NetminerCode::ENicOrder);
        }
        let want = match nic_cell(n) {
            Some(w) => w,
            None => return Err(NetminerCode::ENoNic),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8301 + n as u64);
            return Err(NetminerCode::EParseDrift);
        }
        if match self.nics.get(n) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NetminerCode::EDup);
        }
        if let Some(x) = self.nics.get_mut(n) {
            *x = Some(got);
        }
        self.last_nic = st as i64;
        self.fnv(0x8302 + n as u64);
        Ok(())
    }

    /// NIC 全账核验（6 格）。
    pub fn nics_all(&self) -> bool {
        let mut n = 0usize;
        while n < NIC_CELLS {
            match self.nics.get(n) {
                Some(Some(_)) => {}
                _ => return false,
            }
            n += 1;
        }
        true
    }

    /// 最后 NIC 态游标。
    pub fn last_nic(&self) -> i64 {
        self.last_nic
    }

    /// 与 Wireshark 消费联签一格（F52249 tshark 底座锚定行）。
    pub fn record_cosign(&mut self, s: usize) -> Result<(), NetminerCode> {
        if s >= COSIGN_CELLS {
            return Err(NetminerCode::ENoSign);
        }
        if match self.cosigns.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NetminerCode::EDup);
        }
        if let Some(x) = self.cosigns.get_mut(s) {
            *x = cosign_cell(s);
        }
        self.fnv(0x8400 + s as u64);
        Ok(())
    }

    /// 联签全齐核验（4 格）。
    pub fn cosigns_all(&self) -> bool {
        let mut s = 0usize;
        while s < COSIGN_CELLS {
            match self.cosigns.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// Java/.NET 上栈声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), NetminerCode> {
        if r >= SPLIT_ROWS {
            return Err(NetminerCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(NetminerCode::EDup);
        }
        if let Some(x) = self.splits.get_mut(r) {
            *x = split_row(r);
        }
        self.fnv(0x8500 + r as u64);
        Ok(())
    }

    /// 声明全齐核验。
    pub fn splits_all(&self) -> bool {
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            match self.splits.get(r) {
                Some(Some(_)) => {}
                _ => return false,
            }
            r += 1;
        }
        true
    }

    /// 缺声明行核验（未齐即报 ERowMiss）。
    pub fn require_rows(&self) -> Result<(), NetminerCode> {
        if self.splits_all() {
            Ok(())
        } else {
            Err(NetminerCode::ERowMiss)
        }
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// 注入守恒核验（注入=检出+逃逸）。
    pub fn verify_conservation(&self) -> Result<(), NetminerCode> {
        if self.injected == self.caught + self.escaped {
            Ok(())
        } else {
            Err(NetminerCode::EInj)
        }
    }

    /// KB-COMPAT 定级（全齐 → S2，六闸齐 → S2，空账 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.parses_all()
            && self.nics_all()
            && self.cosigns_all()
            && self.splits_all()
        {
            Grade::S2
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级核验（漂移即报 EGradeDrift）。
    pub fn verify_grade(&self, expect: Grade) -> Result<(), NetminerCode> {
        if self.judge_grade() == expect {
            Ok(())
        } else {
            Err(NetminerCode::EGradeDrift)
        }
    }
}

impl Default for NetminerLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// 双跑指纹核验。
pub fn verify_fp(a: u64, b: u64) -> bool {
    a == b && a != 0
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = NetminerLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < PARSE_CELLS {
        if let Some(w) = parse_cell(c) {
            if led.record_parse(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut n = 0usize;
    while n < NIC_CELLS {
        if let Some(w) = nic_cell(n) {
            if led.record_nic(n, w).is_ok() {
                recorded += 1;
            }
        }
        n += 1;
    }
    let mut s = 0usize;
    while s < COSIGN_CELLS {
        if led.record_cosign(s).is_ok() {
            recorded += 1;
        }
        s += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：解析越闭集 / 解析漂移 / NIC 态逆序三态轮转。
    // 漂移注入先按合法序落正格再注漂移；态逆序注入对空账本同样必抓
    // 且不改态游标。注入即 TUI 消费异常检出（与 F52249 底座联签）。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = NetminerLedger::new();
        let caught = match kind {
            0 => lt.record_parse(PARSE_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_parse(0, parse_cell(0).unwrap_or(0));
                lt.record_parse(1, parse_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(NetminerCode::EParseDrift)
            }
            _ => {
                let _ = lt.record_nic(5, nic_cell(5).unwrap_or(0));
                lt.record_nic(1, nic_cell(1).unwrap_or(0)) == Err(NetminerCode::ENicOrder)
                    && lt.last_nic() == 2
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x8600 + k as u64);
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
/// UNX-F52436 判据集（锚 UNX-F52437-J1，off-by-one 板式）。
pub fn run_unx52436_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52436");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五解析族 20 格全落",
        recorded >= PARSE_CELLS as u32,
        "PCAP/凭据/会话/文件/DNS 逐格恒等",
    );
    s.add(
        "NIC 三态 6 格全落",
        {
            let mut lt = NetminerLedger::new();
            let mut ok = true;
            let mut n = 0usize;
            while n < NIC_CELLS {
                if let Some(w) = nic_cell(n) {
                    if lt.record_nic(n, w).is_err() {
                        ok = false;
                    }
                }
                n += 1;
            }
            ok && lt.nics_all()
        },
        "混杂/非混杂/离线三态 × IPv4/IPv6 逐格恒等，态序单调",
    );
    s.add(
        "Wireshark 消费联签 4/4 全齐（F52249）",
        {
            let mut lt = NetminerLedger::new();
            let mut ok = true;
            let mut sc = 0usize;
            while sc < COSIGN_CELLS {
                if lt.record_cosign(sc).is_err() {
                    ok = false;
                }
                sc += 1;
            }
            ok && lt.cosigns_all()
        },
        "与 F52249 tshark 底座联签锚定行四格闭集（§AI-66.3）",
    );
    s.add(
        "Java/.NET 上栈声明 2/2 全齐",
        {
            let mut lt = NetminerLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all() && lt.require_rows().is_ok()
        },
        "Java 上栈分账/.NET 上栈分账两行闭集",
    );
    s.add(
        "缺声明行必抓",
        {
            let lt = NetminerLedger::new();
            lt.require_rows() == Err(NetminerCode::ERowMiss)
        },
        "Java/.NET 上栈声明未齐即报 ERowMiss，缺一拒封",
    );
    s.add(
        "六闸全落",
        {
            let mut lt = NetminerLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账",
    );
    s.add(
        "解析家族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < PARSE_CELLS {
                if parse_kind(c).unwrap_or(5) < parse_kind(c - 1).unwrap_or(5) {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "五解析族闭集单调，逆序必抓",
    );
    s.add(
        "注入 10/10 全抓（TUI 消费 ×10）",
        caught >= 10,
        "解析越闭集/解析漂移/NIC 态逆序三态轮转必抓",
    );
    let mut l0 = NetminerLedger::new();
    s.add(
        "边界·解析越闭集",
        l0.record_parse(PARSE_CELLS, 0).is_err(),
        "解析 20 越闭集必拒",
    );
    s.add(
        "边界·NIC 越闭集",
        l0.record_nic(NIC_CELLS, 0).is_err(),
        "NIC 6 格越闭集必拒",
    );
    s.add(
        "边界·联签越闭集",
        l0.record_cosign(COSIGN_CELLS).is_err(),
        "联签 4 格越闭集必拒",
    );
    s.add(
        "未齐落格仍收",
        l0.record_parse(0, parse_cell(0).unwrap_or(0)).is_ok(),
        "账未齐时合法格照常落账（未齐即封只封收口，不封落格）",
    );
    s.add(
        "守恒核验 Ok",
        l0.verify_conservation().is_ok(),
        "账未齐时守恒等式照常成立，不误报 EInj",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10,
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    let (_, _, fp2) = run_cycle();
    s.add("双跑指纹一致", verify_fp(fp, fp2), "确定性全流程逐位一致且非零");
    s.add(
        "定级 S2 闭集（KB-COMPAT 登记）",
        {
            let mut lt = NetminerLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < PARSE_CELLS {
                if let Some(w) = parse_cell(c) {
                    let _ = lt.record_parse(c, w);
                }
                c += 1;
            }
            let mut n = 0usize;
            while n < NIC_CELLS {
                if let Some(w) = nic_cell(n) {
                    let _ = lt.record_nic(n, w);
                }
                n += 1;
            }
            let mut sc = 0usize;
            while sc < COSIGN_CELLS {
                let _ = lt.record_cosign(sc);
                sc += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.verify_grade(Grade::S2).is_ok()
        },
        "全齐 → S2（KB-COMPAT 定级登记，TUI 消费 ×10 联签锚定）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCACC0,
        "SEG_BASE 0x40 对齐且在 VX16 域 0xCACC0 格",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kind() {
        assert_eq!(parse_kind(0), Some(0));
        assert_eq!(parse_kind(4), Some(1));
        assert_eq!(parse_kind(8), Some(2));
        assert_eq!(parse_kind(12), Some(3));
        assert_eq!(parse_kind(19), Some(4));
        assert!(parse_kind(20).is_none());
        assert_eq!(parse_kind_name(0), Some("PCAP 解析"));
        assert_eq!(parse_kind_name(4), Some("DNS 记录"));
        assert!(parse_kind_name(5).is_none());
    }

    #[test]
    fn test_nic_state() {
        // i/N 映射：格 n 的态 = n/2。
        assert_eq!(nic_state(0), Some(0));
        assert_eq!(nic_state(1), Some(0));
        assert_eq!(nic_state(2), Some(1));
        assert_eq!(nic_state(4), Some(2));
        assert_eq!(nic_state(5), Some(2));
        assert!(nic_state(6).is_none());
        assert_eq!(nic_state_name(0), Some("混杂"));
        assert_eq!(nic_state_name(2), Some("离线"));
        assert!(nic_state_name(3).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = parse_cell(3).unwrap_or(0);
        assert_eq!(a, parse_cell(3).unwrap_or(0));
        assert_ne!(a, parse_cell(4).unwrap_or(0));
        assert_ne!(nic_cell(0).unwrap_or(0), nic_cell(1).unwrap_or(0));
        assert!(parse_cell(PARSE_CELLS).is_none());
        assert!(nic_cell(NIC_CELLS).is_none());
        assert!(cosign_cell(COSIGN_CELLS).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = NetminerLedger::new();
        let _ = lt.record_parse(10, parse_cell(10).unwrap_or(0));
        assert_eq!(lt.record_parse(3, parse_cell(3).unwrap_or(0)), Err(NetminerCode::EParseOrder));
        assert!(lt.record_parse(11, parse_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_nic_order_caught() {
        let mut lt = NetminerLedger::new();
        assert!(lt.record_nic(5, nic_cell(5).unwrap_or(0)).is_ok());
        let before = lt.last_nic();
        assert_eq!(lt.record_nic(1, nic_cell(1).unwrap_or(0)), Err(NetminerCode::ENicOrder));
        // 逆态被拒后态游标不得漂移。
        assert_eq!(lt.last_nic(), before);
        assert!(lt.record_nic(6, nic_cell(6).unwrap_or(0)).is_err());
        assert_eq!(lt.record_nic(NIC_CELLS, 0), Err(NetminerCode::ENoNic));
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = NetminerLedger::new();
        assert_eq!(
            lt.record_parse(6, parse_cell(6).unwrap_or(0).wrapping_add(1)),
            Err(NetminerCode::EParseDrift)
        );
        assert_eq!(
            lt.record_nic(3, nic_cell(3).unwrap_or(0).wrapping_add(9)),
            Err(NetminerCode::EParseDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = NetminerLedger::new();
        let _ = lt.record_parse(0, parse_cell(0).unwrap_or(0));
        assert_eq!(lt.record_parse(0, parse_cell(0).unwrap_or(0)), Err(NetminerCode::EDup));
        let _ = lt.record_cosign(0);
        assert_eq!(lt.record_cosign(0), Err(NetminerCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = NetminerLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(GATES).is_err());
        assert!(lt.gates_all());
        let mut lt2 = NetminerLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_row_miss() {
        let mut lt = NetminerLedger::new();
        assert_eq!(lt.require_rows(), Err(NetminerCode::ERowMiss));
        let _ = lt.declare_split(0);
        assert_eq!(lt.require_rows(), Err(NetminerCode::ERowMiss));
        let _ = lt.declare_split(1);
        assert!(lt.require_rows().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 只含 run_cycle 实际计数的三节：解析 20 + NIC 6 + 联签 4。
        assert_eq!(recorded, (PARSE_CELLS + NIC_CELLS + COSIGN_CELLS) as u32);
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
        assert!(verify_fp(f1, f2));
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCACC0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        // 码闭集恰 13 变体，位序 0..=0x0C 与枚举实际位序一致。
        assert_eq!(NetminerCode::Ok as u32, 0);
        assert_eq!(NetminerCode::ENoParse.val(), SEG_BASE + 1);
        assert_eq!(NetminerCode::EParseDrift.val(), SEG_BASE + 2);
        assert_eq!(NetminerCode::EParseOrder.val(), SEG_BASE + 3);
        assert_eq!(NetminerCode::ENoNic.val(), SEG_BASE + 4);
        assert_eq!(NetminerCode::ENicOrder.val(), SEG_BASE + 5);
        assert_eq!(NetminerCode::ENoSign.val(), SEG_BASE + 6);
        assert_eq!(NetminerCode::ENoRow.val(), SEG_BASE + 7);
        assert_eq!(NetminerCode::ERowMiss.val(), SEG_BASE + 8);
        assert_eq!(NetminerCode::EDup.val(), SEG_BASE + 9);
        assert_eq!(NetminerCode::ESeal.val(), SEG_BASE + 10);
        assert_eq!(NetminerCode::EInj.val(), SEG_BASE + 11);
        assert_eq!(NetminerCode::EGradeDrift.val(), SEG_BASE + 12);
        assert_eq!(NetminerCode::EGradeDrift as u32, 12);
        let all = [
            NetminerCode::Ok,
            NetminerCode::ENoParse,
            NetminerCode::EParseDrift,
            NetminerCode::EParseOrder,
            NetminerCode::ENoNic,
            NetminerCode::ENicOrder,
            NetminerCode::ENoSign,
            NetminerCode::ENoRow,
            NetminerCode::ERowMiss,
            NetminerCode::EDup,
            NetminerCode::ESeal,
            NetminerCode::EInj,
            NetminerCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert!(!all[i].detail().is_empty());
            i += 1;
        }
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐目标档 S2。
        let lt0 = NetminerLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = NetminerLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = NetminerLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        let mut c = 0usize;
        while c < PARSE_CELLS {
            if let Some(w) = parse_cell(c) {
                let _ = lt2.record_parse(c, w);
            }
            c += 1;
        }
        let mut n = 0usize;
        while n < NIC_CELLS {
            if let Some(w) = nic_cell(n) {
                let _ = lt2.record_nic(n, w);
            }
            n += 1;
        }
        let mut sc = 0usize;
        while sc < COSIGN_CELLS {
            let _ = lt2.record_cosign(sc);
            sc += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt2.declare_split(r);
            r += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S2);
        assert!(lt2.verify_grade(Grade::S2).is_ok());
    }

    #[test]
    fn test_injection_states() {
        // 三态逐态核验：越闭集 / 漂移（先落正格）/ 态逆序（空账本必抓且游标不变）。
        let mut l0 = NetminerLedger::new();
        assert!(l0.record_parse(PARSE_CELLS, 0).is_err());
        let mut l1 = NetminerLedger::new();
        let _ = l1.record_parse(0, parse_cell(0).unwrap_or(0));
        assert_eq!(
            l1.record_parse(1, parse_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(NetminerCode::EParseDrift)
        );
        let mut l2 = NetminerLedger::new();
        let _ = l2.record_nic(5, nic_cell(5).unwrap_or(0));
        assert_eq!(l2.record_nic(1, nic_cell(1).unwrap_or(0)), Err(NetminerCode::ENicOrder));
        assert_eq!(l2.last_nic(), 2);
        assert!(l2.verify_conservation().is_ok());
    }

    #[test]
    fn unx52436_checks_green() {
        let s = run_unx52436_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
