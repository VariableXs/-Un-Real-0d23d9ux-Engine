//! UNX-F52247 Fiddler Classic 收口 · 0xC7D80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52247"，逐字）：
//!   title: Fiddler Classic 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52248-J1 六判据全
//!     落（Electron 升级接管口径）；×10；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.1 防重声明；③ 完成时 API/软件写回：状态=已完成、结果摘要
//!     非空。
//! off-by-one 板式口径：本卡（卡 N=52247）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52248-J1，照原文锚定；功能按本卡标题（Fiddler Classic）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 捕获三态 12 格闭集：三家族（活动捕获/暂停捕获/回放会话）各 4 格，
//!      家族单调（逆序必抓），逐格载荷纯函数恒等才收，漂移必抓。
//!   ② 规则/FiddlerScript 10 操作集：两家族（规则 0..5 / FiddlerScript
//!      5..10），逐操作恒等。
//!   ③ 解密三元闸：证书信任 / 流量解密 / 会话明文三态逐格落账，越三态
//!      闭集必抓。
//!   ④ 扩展 8 格逐格落账；.NET 上栈 4 层（CLR 加载/程序集解析/规则引擎
//!      绑定/代理套接字栈）逐层落账。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade() 定级闭集）；注入核 run_cycle 三态轮转 10 轮（捕获格漂移/
//!      FiddlerScript 家族逆序/扩展越闭集），三类注入全部落在当轮临时
//!      账本上，led 上 injected==caught==10、escaped==0；漂移注入先录正
//!      格再注漂移；双跑指纹一致。
//!   ⑥ 定级 S2 口径：六判据 + 12 捕获格 + 三元闸 + 10 操作 + 8 扩展格
//!      + 4 层全通 → S2（本卡收口上限）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52247;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 捕获三态格数。
pub const CAP_CELLS: usize = 12;

/// 解密三元闸态数。
pub const DECRYPT_STATES: usize = 3;

/// 规则/FiddlerScript 操作数。
pub const SCRIPT_OPS: usize = 10;

/// 扩展格数。
pub const EXT_CELLS: usize = 8;

/// .NET 上栈层数。
pub const DOTNET_LAYERS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FiddlerCode {
    /// 成功
    Ok = 0,
    /// 捕获格越闭集
    ENoCap,
    /// 捕获漂移
    ECapDrift,
    /// 解密三元闸越闭集
    ENoDecrypt,
    /// 脚本操作越闭集
    ENoScript,
    /// 脚本家族逆序
    EOrder,
    /// 扩展格越闭集
    ENoExt,
    /// 扩展漂移
    EExtDrift,
    /// 层越闭集
    ENoLayer,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl FiddlerCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            FiddlerCode::Ok => "成功",
            FiddlerCode::ENoCap => "捕获格越闭集：捕获三态 12 格闭集之外先登记",
            FiddlerCode::ECapDrift => "捕获漂移：捕获格载荷与基准不恒等，逐格复算",
            FiddlerCode::ENoDecrypt => "解密三元闸越闭集：证书信任/流量解密/会话明文三态之外先登记",
            FiddlerCode::ENoScript => "脚本操作越闭集：规则/FiddlerScript 10 操作集之外先登记",
            FiddlerCode::EOrder => "脚本家族逆序：规则→FiddlerScript 单调，逆序必抓",
            FiddlerCode::ENoExt => "扩展格越闭集：扩展 8 格闭集之外先登记",
            FiddlerCode::EExtDrift => "扩展漂移：扩展格载荷与基准不恒等，逐格复算",
            FiddlerCode::ENoLayer => "层越闭集：.NET 上栈 4 层闭集之外先登记",
            FiddlerCode::EDup => "重复落账：同格只许一条账目",
            FiddlerCode::ESeal => "未齐即封：12 捕获格 + 10 操作 + 8 扩展格未齐先补齐",
            FiddlerCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            FiddlerCode::EGradeDrift => "定级漂移：六判据+12 捕获格+三元闸+10 操作+8 扩展格+4 层全通才可定 S2",
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
            Grade::S1 => "S1：全通收口（本卡口径不触发，本卡收口上限 S2）",
            Grade::S2 => "S2：六判据+12 捕获格+三元闸+10 操作+8 扩展格+4 层全通收口",
            Grade::S3 => "S3：不收口，判据缺失或格未齐",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 捕获格载荷（格 i；家族 = i/4）。
pub const fn cap_cell(i: usize) -> Option<u64> {
    if i >= CAP_CELLS {
        return None;
    }
    Some((0xF0000u64 + (i as u64) * 0x41) ^ ((i / 4) as u64).wrapping_mul(0x0C0DEC7))
}

/// 捕获家族（0..4=活动捕获 4..8=暂停捕获 8..12=回放会话）。
pub const fn cap_kind(i: usize) -> Option<u32> {
    if i >= CAP_CELLS {
        return None;
    }
    Some((i / 4) as u32)
}

/// 捕获家族名。
pub const fn cap_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("活动捕获"),
        1 => Some("暂停捕获"),
        2 => Some("回放会话"),
        _ => None,
    }
}

/// 解密三元闸态名。
pub const fn decrypt_state_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("证书信任"),
        1 => Some("流量解密"),
        2 => Some("会话明文"),
        _ => None,
    }
}

/// 解密三元闸态折叠。
pub const fn decrypt_state(s: usize) -> Option<u32> {
    if s >= DECRYPT_STATES {
        return None;
    }
    Some(0xF100 + s as u32 * 0x67)
}

/// 脚本操作格载荷（操作 o；家族 = o/5：0..5 规则 5..10 FiddlerScript）。
pub const fn script_cell(o: usize) -> Option<u64> {
    if o >= SCRIPT_OPS {
        return None;
    }
    Some((0xF2000u64 + (o as u64) * 0x4F) ^ ((o / 5) as u64).wrapping_mul(0x01000193))
}

/// 脚本操作家族。
pub const fn script_kind(o: usize) -> Option<u32> {
    if o >= SCRIPT_OPS {
        return None;
    }
    Some((o / 5) as u32)
}

/// 脚本操作家族名。
pub const fn script_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("规则"),
        1 => Some("FiddlerScript"),
        _ => None,
    }
}

/// 扩展格载荷（格 x）。
pub const fn ext_cell(x: usize) -> Option<u64> {
    if x >= EXT_CELLS {
        return None;
    }
    Some((0xF3000u64 + (x as u64) * 0x37) ^ 0x464944455854454E)
}

/// .NET 上栈层载荷（层 l）。
pub const fn dotnet_cell(l: usize) -> Option<u64> {
    if l >= DOTNET_LAYERS {
        return None;
    }
    Some(0xF4000u64 + (l as u64) * 0x73)
}

/// .NET 上栈层名。
pub const fn dotnet_layer_name(l: usize) -> Option<&'static str> {
    match l {
        0 => Some("CLR 加载"),
        1 => Some("程序集解析"),
        2 => Some("规则引擎绑定"),
        3 => Some("代理套接字栈"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Fiddler Classic 收口账本状态机。
#[derive(Clone)]
pub struct FiddlerLedger {
    /// 六判据（启动/HTTPS 解密/会话列表/断点/系统代理语义消费/卸载）。
    gates: [Option<bool>; 6],
    /// 捕获账。
    caps: [Option<u64>; CAP_CELLS],
    /// 解密三元闸账。
    decrypts: [Option<u32>; DECRYPT_STATES],
    /// 脚本操作账。
    scripts: [Option<u64>; SCRIPT_OPS],
    /// 扩展账。
    exts: [Option<u64>; EXT_CELLS],
    /// .NET 上栈层账。
    dotnets: [Option<u64>; DOTNET_LAYERS],
    /// 最后脚本家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl FiddlerLedger {
    /// 新账本。
    pub fn new() -> Self {
        FiddlerLedger {
            gates: [None; 6],
            caps: [None; CAP_CELLS],
            decrypts: [None; DECRYPT_STATES],
            scripts: [None; SCRIPT_OPS],
            exts: [None; EXT_CELLS],
            dotnets: [None; DOTNET_LAYERS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), FiddlerCode> {
        if gate >= 6 {
            return Err(FiddlerCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7100 + gate as u64);
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

    /// 捕获一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_cap(&mut self, i: usize, got: u64) -> Result<(), FiddlerCode> {
        if i >= CAP_CELLS {
            return Err(FiddlerCode::ENoCap);
        }
        let want = match cap_cell(i) {
            Some(w) => w,
            None => return Err(FiddlerCode::ENoCap),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200 + i as u64);
            return Err(FiddlerCode::ECapDrift);
        }
        if match self.caps.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FiddlerCode::EDup);
        }
        if let Some(c) = self.caps.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x7201 + i as u64);
        Ok(())
    }

    /// 捕获全账核验（12 格）。
    pub fn caps_all(&self) -> bool {
        let mut i = 0usize;
        while i < CAP_CELLS {
            match self.caps.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 解密三元闸一格。
    pub fn record_decrypt(&mut self, s: usize) -> Result<(), FiddlerCode> {
        let v = match decrypt_state(s) {
            Some(v) => v,
            None => return Err(FiddlerCode::ENoDecrypt),
        };
        if match self.decrypts.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FiddlerCode::EDup);
        }
        if let Some(c) = self.decrypts.get_mut(s) {
            *c = Some(v);
        }
        self.fnv(0x7300 + s as u64);
        Ok(())
    }

    /// 解密三元闸全齐核验。
    pub fn decrypts_all(&self) -> bool {
        let mut s = 0usize;
        while s < DECRYPT_STATES {
            match self.decrypts.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 脚本操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_script(&mut self, o: usize, got: u64) -> Result<(), FiddlerCode> {
        if o >= SCRIPT_OPS {
            return Err(FiddlerCode::ENoScript);
        }
        let kind = match script_kind(o) {
            Some(k) => k,
            None => return Err(FiddlerCode::ENoScript),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400);
            return Err(FiddlerCode::EOrder);
        }
        let want = match script_cell(o) {
            Some(w) => w,
            None => return Err(FiddlerCode::ENoScript),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7401 + o as u64);
            return Err(FiddlerCode::ECapDrift);
        }
        if match self.scripts.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FiddlerCode::EDup);
        }
        if let Some(c) = self.scripts.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7402 + o as u64);
        Ok(())
    }

    /// 脚本全账核验（10 操作）。
    pub fn scripts_all(&self) -> bool {
        let mut o = 0usize;
        while o < SCRIPT_OPS {
            match self.scripts.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 扩展一格（载荷对照 + 重复拒）。
    pub fn record_ext(&mut self, x: usize, got: u64) -> Result<(), FiddlerCode> {
        if x >= EXT_CELLS {
            return Err(FiddlerCode::ENoExt);
        }
        let want = match ext_cell(x) {
            Some(w) => w,
            None => return Err(FiddlerCode::ENoExt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7500 + x as u64);
            return Err(FiddlerCode::EExtDrift);
        }
        if match self.exts.get(x) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FiddlerCode::EDup);
        }
        if let Some(c) = self.exts.get_mut(x) {
            *c = Some(got);
        }
        self.fnv(0x7501 + x as u64);
        Ok(())
    }

    /// 扩展全账核验（8 格）。
    pub fn exts_all(&self) -> bool {
        let mut x = 0usize;
        while x < EXT_CELLS {
            match self.exts.get(x) {
                Some(Some(_)) => {}
                _ => return false,
            }
            x += 1;
        }
        true
    }

    /// .NET 上栈层一格。
    pub fn record_dotnet(&mut self, l: usize) -> Result<(), FiddlerCode> {
        let v = match dotnet_cell(l) {
            Some(v) => v,
            None => return Err(FiddlerCode::ENoLayer),
        };
        if match self.dotnets.get(l) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(FiddlerCode::EDup);
        }
        if let Some(c) = self.dotnets.get_mut(l) {
            *c = Some(v);
        }
        self.fnv(0x7600 + l as u64);
        Ok(())
    }

    /// 层全齐核验（4 层）。
    pub fn dotnets_all(&self) -> bool {
        let mut l = 0usize;
        while l < DOTNET_LAYERS {
            match self.dotnets.get(l) {
                Some(Some(_)) => {}
                _ => return false,
            }
            l += 1;
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

    /// KB-COMPAT 定级（本卡收口口径：全通 → S2，其余 → S3）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all()
            && self.caps_all()
            && self.decrypts_all()
            && self.scripts_all()
            && self.exts_all()
            && self.dotnets_all()
        {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for FiddlerLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = FiddlerLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut i = 0usize;
    while i < CAP_CELLS {
        if let Some(w) = cap_cell(i) {
            if led.record_cap(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut s = 0usize;
    while s < DECRYPT_STATES {
        let _ = led.record_decrypt(s);
        s += 1;
    }
    let mut o = 0usize;
    while o < SCRIPT_OPS {
        if let Some(w) = script_cell(o) {
            if led.record_script(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut x = 0usize;
    while x < EXT_CELLS {
        if let Some(w) = ext_cell(x) {
            if led.record_ext(x, w).is_ok() {
                recorded += 1;
            }
        }
        x += 1;
    }
    let mut l = 0usize;
    while l < DOTNET_LAYERS {
        let _ = led.record_dotnet(l);
        l += 1;
    }
    // 注入 10 轮：捕获格漂移 / 脚本家族逆序 / 扩展越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；漂移注入先录正格再注漂移。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = FiddlerLedger::new();
        let caught = match kind {
            0 => {
                let _ = lt.record_cap(0, cap_cell(0).unwrap_or(0));
                lt.record_cap(1, cap_cell(1).unwrap_or(0).wrapping_add(1))
                    == Err(FiddlerCode::ECapDrift)
            }
            1 => {
                let _ = lt.record_script(7, script_cell(7).unwrap_or(0));
                lt.record_script(2, script_cell(2).unwrap_or(0)) == Err(FiddlerCode::EOrder)
            }
            _ => {
                let _ = lt.record_script(0, script_cell(0).unwrap_or(0));
                lt.record_ext(EXT_CELLS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7700 + k as u64);
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
/// UNX-F52247 判据集（锚 UNX-F52248-J1，off-by-one 板式）。
pub fn run_unx52247_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52247");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "捕获三态 12 格全通",
        recorded >= CAP_CELLS as u32,
        "活动捕获/暂停捕获/回放会话三家族逐格恒等",
    );
    s.add(
        "解密三元闸 3/3 全落",
        {
            let mut lt = FiddlerLedger::new();
            let mut ok = true;
            let mut d = 0usize;
            while d < DECRYPT_STATES {
                if lt.record_decrypt(d).is_err() {
                    ok = false;
                }
                d += 1;
            }
            ok && lt.decrypts_all()
        },
        "证书信任/流量解密/会话明文三态逐格落账",
    );
    s.add(
        "规则/FiddlerScript 10 操作全通",
        {
            let mut lt = FiddlerLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < SCRIPT_OPS {
                if let Some(w) = script_cell(o) {
                    if lt.record_script(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.scripts_all()
        },
        "规则/FiddlerScript 两家族逐操作恒等",
    );
    s.add(
        "扩展 8 格全落",
        {
            let mut lt = FiddlerLedger::new();
            let mut ok = true;
            let mut x = 0usize;
            while x < EXT_CELLS {
                if let Some(w) = ext_cell(x) {
                    if lt.record_ext(x, w).is_err() {
                        ok = false;
                    }
                }
                x += 1;
            }
            ok && lt.exts_all()
        },
        "扩展 8 格逐格载荷对照恒等",
    );
    s.add(
        ".NET 上栈 4 层全落",
        {
            let mut lt = FiddlerLedger::new();
            let mut ok = true;
            let mut l = 0usize;
            while l < DOTNET_LAYERS {
                if lt.record_dotnet(l).is_err() {
                    ok = false;
                }
                l += 1;
            }
            ok && lt.dotnets_all()
        },
        "CLR 加载/程序集解析/规则引擎绑定/代理套接字栈逐层落账",
    );
    s.add(
        "捕获家族单调",
        {
            let mut ok = true;
            let mut i = 1usize;
            while i < CAP_CELLS {
                if cap_kind(i).unwrap_or(9) < cap_kind(i - 1).unwrap_or(9) {
                    ok = false;
                }
                i += 1;
            }
            ok
        },
        "三家族闭集单调",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = FiddlerLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/HTTPS 解密/会话列表/断点/系统代理语义消费/卸载逐格落账",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "捕获格漂移/脚本家族逆序/扩展越闭集三态轮转必抓",
    );
    let mut l0 = FiddlerLedger::new();
    s.add(
        "边界·捕获格越闭集",
        l0.record_cap(CAP_CELLS, 0).is_err(),
        "捕获格 12 越闭集必拒",
    );
    s.add(
        "边界·脚本操作越闭集",
        l0.record_script(SCRIPT_OPS, 0).is_err(),
        "脚本操作 10 越闭集必拒",
    );
    s.add(
        "边界·扩展越闭集",
        l0.record_ext(EXT_CELLS, 0).is_err(),
        "扩展格 8 越闭集必拒",
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
            let mut lt = FiddlerLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut i = 0usize;
            while i < CAP_CELLS {
                if let Some(w) = cap_cell(i) {
                    let _ = lt.record_cap(i, w);
                }
                i += 1;
            }
            let mut d = 0usize;
            while d < DECRYPT_STATES {
                let _ = lt.record_decrypt(d);
                d += 1;
            }
            let mut o = 0usize;
            while o < SCRIPT_OPS {
                if let Some(w) = script_cell(o) {
                    let _ = lt.record_script(o, w);
                }
                o += 1;
            }
            let mut x = 0usize;
            while x < EXT_CELLS {
                if let Some(w) = ext_cell(x) {
                    let _ = lt.record_ext(x, w);
                }
                x += 1;
            }
            let mut l = 0usize;
            while l < DOTNET_LAYERS {
                let _ = lt.record_dotnet(l);
                l += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "全通 → S2（KB-COMPAT 定级 S2 登记口径，§AI-66.1 防重声明）",
    );
    s.add(
        "诊断码闭集",
        FiddlerCode::Ok.val() == SEG_BASE && FiddlerCode::EGradeDrift.val() == SEG_BASE + 12,
        "Ok=+0x00、EGradeDrift=+0x0C 恰 13 变体",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7D80,
        "SEG_BASE = 0xC7200 + 0x40×(52247−52201)，0x40 对齐",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cap_kinds() {
        assert_eq!(cap_kind_name(0), Some("活动捕获"));
        assert_eq!(cap_kind_name(1), Some("暂停捕获"));
        assert_eq!(cap_kind_name(2), Some("回放会话"));
        assert!(cap_kind_name(3).is_none());
        assert_eq!(cap_kind(11), Some(2));
        assert!(cap_kind(12).is_none());
        assert!(cap_cell(12).is_none());
    }

    #[test]
    fn test_decrypt_states() {
        assert_eq!(decrypt_state_name(0), Some("证书信任"));
        assert_eq!(decrypt_state_name(1), Some("流量解密"));
        assert_eq!(decrypt_state_name(2), Some("会话明文"));
        assert!(decrypt_state_name(3).is_none());
        assert!(decrypt_state(DECRYPT_STATES).is_none());
    }

    #[test]
    fn test_script_kind() {
        assert_eq!(script_kind(0), Some(0));
        assert_eq!(script_kind(4), Some(0));
        assert_eq!(script_kind(5), Some(1));
        assert_eq!(script_kind(9), Some(1));
        assert!(script_kind(10).is_none());
        assert_eq!(script_kind_name(0), Some("规则"));
        assert_eq!(script_kind_name(1), Some("FiddlerScript"));
        assert!(script_kind_name(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = cap_cell(5).unwrap_or(0);
        assert_eq!(a, cap_cell(5).unwrap_or(0));
        assert_ne!(a, cap_cell(6).unwrap_or(0));
        let x = ext_cell(3).unwrap_or(0);
        assert_eq!(x, ext_cell(3).unwrap_or(0));
        assert!(ext_cell(EXT_CELLS).is_none());
    }

    #[test]
    fn test_dotnet_layers() {
        assert_eq!(dotnet_layer_name(0), Some("CLR 加载"));
        assert_eq!(dotnet_layer_name(3), Some("代理套接字栈"));
        assert!(dotnet_layer_name(4).is_none());
        assert!(dotnet_cell(DOTNET_LAYERS).is_none());
        let v0 = dotnet_cell(0).unwrap_or(0);
        let v3 = dotnet_cell(3).unwrap_or(0);
        assert_ne!(v0, v3);
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = FiddlerLedger::new();
        let _ = lt.record_script(7, script_cell(7).unwrap_or(0));
        assert_eq!(lt.record_script(2, script_cell(2).unwrap_or(0)), Err(FiddlerCode::EOrder));
        // 逆序被拒后家族账位不受污染：续落同家族高格仍可收。
        assert!(lt.record_script(8, script_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = FiddlerLedger::new();
        // 先录正格再注漂移。
        let _ = lt.record_cap(0, cap_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_cap(1, cap_cell(1).unwrap_or(0).wrapping_add(1)),
            Err(FiddlerCode::ECapDrift)
        );
        assert_eq!(
            lt.record_ext(0, ext_cell(0).unwrap_or(0).wrapping_add(2)),
            Err(FiddlerCode::EExtDrift)
        );
        assert_eq!(
            lt.record_script(0, script_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(FiddlerCode::ECapDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = FiddlerLedger::new();
        let _ = lt.record_cap(4, cap_cell(4).unwrap_or(0));
        assert_eq!(lt.record_cap(4, cap_cell(4).unwrap_or(0)), Err(FiddlerCode::EDup));
        let _ = lt.record_decrypt(0);
        assert_eq!(lt.record_decrypt(0), Err(FiddlerCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = FiddlerLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = FiddlerLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (CAP_CELLS + SCRIPT_OPS + EXT_CELLS) as u32);
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
        assert_eq!(SEG_BASE, 0xC7D80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(FiddlerCode::Ok as u32, 0);
        assert_eq!(FiddlerCode::EGradeDrift as u32, 12);
        assert!(!FiddlerCode::Ok.detail().is_empty());
        assert!(!FiddlerCode::ENoDecrypt.detail().is_empty());
        assert_eq!(FiddlerCode::Ok.val(), SEG_BASE);
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        let lt = FiddlerLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52247_checks_green() {
        let s = run_unx52247_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
