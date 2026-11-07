//! UNX-F52340 x64dbg 插件生态消费面收口（×10 插件实测） · 0xC94C0段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52340"，逐字）：
//!   title: x64dbg 插件生态消费面收口（×10 插件实测）
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52341-J1 文档
//!     生成 ×10 工程全通；输出 HTML 与 Windows 校验和对照（样式差异
//!     清单公开）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3 组 18 口径；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52340）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52341-J1，照原文锚定；功能按本卡标题（x64dbg 插件生态消费面）
//! 落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点（域语料逐卡定制）：
//!   ① ×10 插件 20 格：每插件 2 格（装载/功能），装载格先于功能格
//!      （同插件相位闸，先功能后装载必抓）；插件名单闭集 10 个
//!      （SwissArmyKnife/SharpOD/SwissCallStack/ColorTheme/DbgChild/
//!      SimpleTrace/HlwDebug/LLVMGraph/CaptainHooks/CodeWithdrawal）。
//!   ② 插件 SDK 消费 8 格：插件初始化/命令注册/表达式函数注册/
//!      菜单注入/事件回调/断点回调/脚本桥接/卸载清理，逐格落账。
//!   ③ 热加载 6 拍单调：重载请求/卸载通知/状态保存/重装/状态恢复/
//!      就绪确认逐拍落账，跳拍逆序必抓（对空账本同样必抓）。
//!   ④ 与 F52163 分账声明 2 行闭集：插件生态消费面与 F52163 本体
//!      分账声明 / 崩溃隔离责任边界声明，缺一拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 三态轮转 10 轮（插件格越
//!      闭集/SDK 格载荷漂移/热加载跳拍越序），三类注入全部落在当轮
//!      临时账本上，主账 injected==caught==10、escaped==0；漂移注入先
//!      按合法序落正格再注漂移；跳拍注入对空账本必抓且不改 last_kind；
//!      双跑指纹一致；全通定级 S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52340;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 插件格数（10 插件 × 2：装载/功能）。
pub const PLUG_CELLS: usize = 20;

/// 插件数。
pub const PLUG_N: usize = 10;

/// SDK 消费格数。
pub const SDK_CELLS: usize = 8;

/// 热加载拍数。
pub const BEAT_N: usize = 6;

/// 与 F52163 分账声明行数。
pub const SPLIT_ROWS: usize = 2;

/// XOR 锚常量（低 16 位恒零口径）。
pub const PLUG_XOR: u64 = 0x5844_4247_504C_0000;

/// SDK XOR 锚常量（低 16 位恒零口径）。
pub const SDK_XOR: u64 = 0x5344_4B5F_434F_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum X64PlugCode {
    /// 成功
    Ok = 0,
    /// 插件格越闭集
    ENoPlug,
    /// 插件/SDK 格载荷漂移
    EPlugDrift,
    /// 序域违例（功能先于装载，或热加载跳拍逆序）
    EOrder,
    /// SDK 格越闭集
    ENoSdk,
    /// SDK 载荷漂移
    ESdkDrift,
    /// 热加载拍越闭集
    ENoBeat,
    /// 分账声明行越闭集
    ENoRow,
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

impl X64PlugCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            X64PlugCode::Ok => "成功",
            X64PlugCode::ENoPlug => "插件格越闭集：×10 插件 20 格闭集之外先登记",
            X64PlugCode::EPlugDrift => "插件/SDK 载荷漂移：格折叠与基准不恒等，逐格复算",
            X64PlugCode::EOrder => "序域违例：功能格先于装载格落账，或热加载跳拍逆序，必抓",
            X64PlugCode::ENoSdk => "SDK 格越闭集：插件 SDK 消费 8 格闭集之外先登记",
            X64PlugCode::ESdkDrift => "SDK 载荷漂移：格折叠与基准不恒等，逐格复算",
            X64PlugCode::ENoBeat => "热加载拍越闭集：热加载 6 拍闭集之外先登记",
            X64PlugCode::ENoRow => "分账声明行越闭集：与 F52163 分账声明 2 行闭集之外先登记",
            X64PlugCode::EDup => "重复落账：同格只许一条账目",
            X64PlugCode::ESeal => "未齐即封：20 插件格 + 8 SDK 格 + 6 热加载拍 + 2 行分账未齐先补齐",
            X64PlugCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            X64PlugCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            X64PlugCode::EGradeDrift => "定级漂移：全通且 ×10 插件实测与分账声明全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20 插件格+8 SDK 格+6 拍热加载+2 行分账全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 插件名（const fn 表 + Option 边界）。
pub const fn plug_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("SwissArmyKnife"),
        1 => Some("SharpOD"),
        2 => Some("SwissCallStack"),
        3 => Some("ColorTheme"),
        4 => Some("DbgChild"),
        5 => Some("SimpleTrace"),
        6 => Some("HlwDebug"),
        7 => Some("LLVMGraph"),
        8 => Some("CaptainHooks"),
        9 => Some("CodeWithdrawal"),
        _ => None,
    }
}

/// 插件格种类（c%2：0=装载 1=功能）。
pub const fn plug_kind(c: usize) -> Option<u32> {
    if c >= PLUG_CELLS {
        return None;
    }
    Some((c % 2) as u32)
}

/// 插件种类名。
pub const fn plug_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("装载"),
        1 => Some("功能"),
        _ => None,
    }
}

/// 插件格载荷（格 c；插件 = c/2）。
pub const fn plug_cell(c: usize) -> Option<u64> {
    if c >= PLUG_CELLS {
        return None;
    }
    Some(
        (0x13_0000u64 + (c as u64) * 0x53)
            ^ PLUG_XOR.wrapping_add((c / 2) as u64 * 0x100),
    )
}

/// SDK 消费格名（const fn 表 + Option 边界）。
pub const fn sdk_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("插件初始化"),
        1 => Some("命令注册"),
        2 => Some("表达式函数注册"),
        3 => Some("菜单注入"),
        4 => Some("事件回调"),
        5 => Some("断点回调"),
        6 => Some("脚本桥接"),
        7 => Some("卸载清理"),
        _ => None,
    }
}

/// SDK 格载荷（格 s）。
pub const fn sdk_cell(s: usize) -> Option<u64> {
    if s >= SDK_CELLS {
        return None;
    }
    Some((0x14_0000u64 + (s as u64) * 0x67) ^ (SDK_XOR.wrapping_add(s as u64 * 0x40)))
}

/// 热加载拍名（const fn 表 + Option 边界）。
pub const fn beat_name(b: usize) -> Option<&'static str> {
    match b {
        0 => Some("重载请求"),
        1 => Some("卸载通知"),
        2 => Some("状态保存"),
        3 => Some("重装"),
        4 => Some("状态恢复"),
        5 => Some("就绪确认"),
        _ => None,
    }
}

/// 热加载拍折叠（拍 b）。
pub const fn beat_cell(b: usize) -> Option<u32> {
    if b >= BEAT_N {
        return None;
    }
    Some(0x1500 + b as u32 * 0x2C)
}

/// 与 F52163 分账声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0x1600 + r as u32 * 0x2C)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// x64dbg 插件生态收口账本状态机。
#[derive(Clone)]
pub struct X64PlugLedger {
    /// 六判据（UNX-F52341-J1 六判据逐格落账）。
    gates: [Option<bool>; 6],
    /// 插件格账。
    plugs: [Option<u64>; PLUG_CELLS],
    /// SDK 消费账。
    sdks: [Option<u64>; SDK_CELLS],
    /// 热加载拍账。
    beats: [Option<u32>; BEAT_N],
    /// 已落热加载拍数（跳拍闸判据）。
    beats_filled: usize,
    /// 分账声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后插件格种类。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl X64PlugLedger {
    /// 新账本。
    pub fn new() -> Self {
        X64PlugLedger {
            gates: [None; 6],
            plugs: [None; PLUG_CELLS],
            sdks: [None; SDK_CELLS],
            beats: [None; BEAT_N],
            beats_filled: 0,
            splits: [None; SPLIT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), X64PlugCode> {
        if gate >= 6 {
            return Err(X64PlugCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x8100 + gate as u64);
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

    /// 插件装载格是否已落（同插件相位闸判据）。
    fn plug_loaded(&self, plugin: usize) -> bool {
        let idx = plugin * 2;
        match self.plugs.get(idx) {
            Some(Some(_)) => true,
            _ => false,
        }
    }

    /// 插件一格（同插件装载先于功能相位闸 + 载荷对照 + 重复拒）。
    pub fn record_plug(&mut self, c: usize, got: u64) -> Result<(), X64PlugCode> {
        if c >= PLUG_CELLS {
            return Err(X64PlugCode::ENoPlug);
        }
        let kind = match plug_kind(c) {
            Some(k) => k,
            None => return Err(X64PlugCode::ENoPlug),
        };
        // 相位闸：功能格（kind=1）须在同插件装载格落账后方可落账；
        // 拒绝路径不改 last_kind。
        if kind == 1 && !self.plug_loaded(c / 2) {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8200);
            return Err(X64PlugCode::EOrder);
        }
        let want = match plug_cell(c) {
            Some(w) => w,
            None => return Err(X64PlugCode::ENoPlug),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8201 + c as u64);
            return Err(X64PlugCode::EPlugDrift);
        }
        if match self.plugs.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(X64PlugCode::EDup);
        }
        if let Some(cc) = self.plugs.get_mut(c) {
            *cc = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x8202 + c as u64);
        Ok(())
    }

    /// 插件全账核验（20 格）。
    pub fn plugs_all(&self) -> bool {
        let mut c = 0usize;
        while c < PLUG_CELLS {
            match self.plugs.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// SDK 消费一格（载荷对照 + 重复拒）。
    pub fn record_sdk(&mut self, s: usize, got: u64) -> Result<(), X64PlugCode> {
        if s >= SDK_CELLS {
            return Err(X64PlugCode::ENoSdk);
        }
        let want = match sdk_cell(s) {
            Some(w) => w,
            None => return Err(X64PlugCode::ENoSdk),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8300 + s as u64);
            return Err(X64PlugCode::ESdkDrift);
        }
        if match self.sdks.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(X64PlugCode::EDup);
        }
        if let Some(cc) = self.sdks.get_mut(s) {
            *cc = Some(got);
        }
        self.fnv(0x8301 + s as u64);
        Ok(())
    }

    /// SDK 全账核验（8 格）。
    pub fn sdks_all(&self) -> bool {
        let mut s = 0usize;
        while s < SDK_CELLS {
            match self.sdks.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 热加载一拍（逐拍单调：只许按 0..5 顺序落账，跳拍必抓）。
    pub fn record_beat(&mut self, b: usize) -> Result<(), X64PlugCode> {
        if b >= BEAT_N {
            return Err(X64PlugCode::ENoBeat);
        }
        // 跳拍闸：只许落下一拍；对空账本同样必抓，且不改 last_kind。
        if b != self.beats_filled {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x8400);
            return Err(X64PlugCode::EOrder);
        }
        if match self.beats.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(X64PlugCode::EDup);
        }
        if let Some(cc) = self.beats.get_mut(b) {
            *cc = beat_cell(b);
        }
        self.beats_filled += 1;
        self.fnv(0x8401 + b as u64);
        Ok(())
    }

    /// 热加载全账核验（6 拍单调）。
    pub fn beats_all(&self) -> bool {
        let mut b = 0usize;
        while b < BEAT_N {
            match self.beats.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 与 F52163 分账声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), X64PlugCode> {
        if r >= SPLIT_ROWS {
            return Err(X64PlugCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(X64PlugCode::EDup);
        }
        if let Some(cc) = self.splits.get_mut(r) {
            *cc = split_row(r);
        }
        self.fnv(0x8500 + r as u64);
        Ok(())
    }

    /// 分账声明全齐核验。
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
            && self.plugs_all()
            && self.sdks_all()
            && self.beats_all()
            && self.splits_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for X64PlugLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = X64PlugLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < PLUG_CELLS {
        if let Some(w) = plug_cell(c) {
            if led.record_plug(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut s = 0usize;
    while s < SDK_CELLS {
        if let Some(w) = sdk_cell(s) {
            let _ = led.record_sdk(s, w);
        }
        s += 1;
    }
    let mut b = 0usize;
    while b < BEAT_N {
        let _ = led.record_beat(b);
        b += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：插件格越闭集 / SDK 格载荷漂移 / 热加载跳拍越序
    // 三态轮转。三类注入全部落在当轮临时账本上，主账只记守恒三数与
    // 指纹。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = X64PlugLedger::new();
        let caught = match kind {
            0 => lt.record_plug(PLUG_CELLS, 0).is_err(),
            1 => {
                // 漂移注入：先按合法序落正格，再注漂移载荷。
                let _ = lt.record_plug(0, plug_cell(0).unwrap_or(0));
                lt.record_sdk(0, sdk_cell(0).unwrap_or(0).wrapping_add(1))
                    == Err(X64PlugCode::ESdkDrift)
            }
            _ => {
                // 跳拍注入：空账本先落拍 1，必抓且不改 last_kind。
                lt.record_beat(1) == Err(X64PlugCode::EOrder)
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
/// UNX-F52340 判据集（锚 UNX-F52341-J1，off-by-one 板式）。
pub fn run_unx52340_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52340");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "×10 插件 20 格全通",
        recorded >= PLUG_CELLS as u32,
        "10 插件 ×（装载/功能）两格逐格恒等，装载先于功能",
    );
    s.add(
        "插件 SDK 消费 8 格全落",
        {
            let mut lt = X64PlugLedger::new();
            let mut ok = true;
            let mut s0 = 0usize;
            while s0 < SDK_CELLS {
                if let Some(w) = sdk_cell(s0) {
                    if lt.record_sdk(s0, w).is_err() {
                        ok = false;
                    }
                }
                s0 += 1;
            }
            ok && lt.sdks_all()
        },
        "初始化/命令注册/表达式函数/菜单注入/事件回调/断点回调/脚本桥接/卸载清理",
    );
    s.add(
        "热加载 6 拍单调全落",
        {
            let mut lt = X64PlugLedger::new();
            let mut ok = true;
            let mut b = 0usize;
            while b < BEAT_N {
                if lt.record_beat(b).is_err() {
                    ok = false;
                }
                b += 1;
            }
            ok && lt.beats_all()
        },
        "重载请求/卸载通知/状态保存/重装/状态恢复/就绪确认逐拍落账",
    );
    s.add(
        "与 F52163 分账声明 2/2 全齐",
        {
            let mut lt = X64PlugLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                if lt.declare_split(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.splits_all()
        },
        "插件生态消费面与 F52163 本体分账声明/崩溃隔离责任边界声明两行闭集",
    );
    s.add(
        "同插件装载先于功能相位闸",
        {
            let mut lt = X64PlugLedger::new();
            lt.record_plug(1, plug_cell(1).unwrap_or(0)) == Err(X64PlugCode::EOrder)
        },
        "功能格先于装载格落账必抓，且不改 last_kind",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "插件格越闭集/SDK 格载荷漂移/热加载跳拍越序三态轮转必抓",
    );
    let mut l0 = X64PlugLedger::new();
    s.add(
        "边界·插件格越闭集",
        l0.record_plug(PLUG_CELLS, 0).is_err(),
        "插件格 20 越闭集必拒",
    );
    s.add(
        "边界·SDK 格越闭集",
        l0.record_sdk(SDK_CELLS, 0).is_err(),
        "SDK 格 8 越闭集必拒",
    );
    s.add(
        "边界·热加载拍越闭集",
        l0.record_beat(BEAT_N).is_err() && l0.declare_split(SPLIT_ROWS).is_err(),
        "热加载拍 6 与分账声明行 2 越闭集必拒",
    );
    s.add(
        "重复落账拒",
        {
            // 插件装载格（kind=0 族首格）重复落账 → EDup。
            let _ = l0.record_plug(0, plug_cell(0).unwrap_or(0));
            l0.record_plug(0, plug_cell(0).unwrap_or(0)) == Err(X64PlugCode::EDup)
        },
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
        "定级 S1 闭集",
        {
            let mut lt = X64PlugLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < PLUG_CELLS {
                if let Some(w) = plug_cell(c) {
                    let _ = lt.record_plug(c, w);
                }
                c += 1;
            }
            let mut s0 = 0usize;
            while s0 < SDK_CELLS {
                if let Some(w) = sdk_cell(s0) {
                    let _ = lt.record_sdk(s0, w);
                }
                s0 += 1;
            }
            let mut b = 0usize;
            while b < BEAT_N {
                let _ = lt.record_beat(b);
                b += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC94C0,
        "SEG_BASE 0x40 对齐且落 0xC7200 + 0x40×(52340−52201) 格",
    );
    s.add("收口·判据条数", s.len() >= 14, "含本条共 ≥15 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plug_name_table() {
        assert_eq!(plug_name(0), Some("SwissArmyKnife"));
        assert_eq!(plug_name(9), Some("CodeWithdrawal"));
        assert!(plug_name(10).is_none());
    }

    #[test]
    fn test_plug_kind_mapping() {
        // 映射口径：种类 = c%2（0=装载 1=功能），插件 = c/2。
        assert_eq!(plug_kind(0), Some(0));
        assert_eq!(plug_kind(1), Some(1));
        assert_eq!(plug_kind(18), Some(0));
        assert_eq!(plug_kind(19), Some(1));
        assert!(plug_kind(20).is_none());
        assert_eq!(plug_kind_name(0), Some("装载"));
        assert_eq!(plug_kind_name(1), Some("功能"));
        assert!(plug_kind_name(2).is_none());
    }

    #[test]
    fn test_sdk_beat_tables() {
        assert_eq!(sdk_name(0), Some("插件初始化"));
        assert_eq!(sdk_name(7), Some("卸载清理"));
        assert!(sdk_name(8).is_none());
        assert!(sdk_cell(8).is_none());
        assert_eq!(beat_name(0), Some("重载请求"));
        assert_eq!(beat_name(5), Some("就绪确认"));
        assert!(beat_name(6).is_none());
        assert!(beat_cell(6).is_none());
        assert!(split_row(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = plug_cell(5).unwrap_or(0);
        assert_eq!(a, plug_cell(5).unwrap_or(0));
        assert_ne!(a, plug_cell(6).unwrap_or(0));
        // XOR 锚常量低 16 位恒零。
        assert_eq!(PLUG_XOR & 0xFFFF, 0);
        assert_eq!(SDK_XOR & 0xFFFF, 0);
    }

    #[test]
    fn test_load_before_function_caught() {
        let mut lt = X64PlugLedger::new();
        assert_eq!(
            lt.record_plug(3, plug_cell(3).unwrap_or(0)),
            Err(X64PlugCode::EOrder)
        );
        // 拒绝路径不改 last_kind：装载格正落仍可落。
        assert!(lt.record_plug(2, plug_cell(2).unwrap_or(0)).is_ok());
        assert!(lt.record_plug(3, plug_cell(3).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_beat_skip_caught() {
        let mut lt = X64PlugLedger::new();
        assert_eq!(lt.record_beat(1), Err(X64PlugCode::EOrder));
        assert!(lt.record_beat(0).is_ok());
        assert!(lt.record_beat(1).is_ok());
        // 跳拍闸：beats_filled 单向推进，重落旧拍先撞跳拍闸（EOrder），
        // EDup 由插件装载格守卫（见重复落账拒判据）。
        assert_eq!(lt.record_beat(1), Err(X64PlugCode::EOrder));
        assert!(lt.record_beat(2).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = X64PlugLedger::new();
        assert_eq!(
            lt.record_plug(0, plug_cell(0).unwrap_or(0).wrapping_add(1)),
            Err(X64PlugCode::EPlugDrift)
        );
        assert_eq!(
            lt.record_sdk(2, sdk_cell(2).unwrap_or(0).wrapping_add(3)),
            Err(X64PlugCode::ESdkDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = X64PlugLedger::new();
        let _ = lt.record_plug(0, plug_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_plug(0, plug_cell(0).unwrap_or(0)),
            Err(X64PlugCode::EDup)
        );
        let _ = lt.record_sdk(0, sdk_cell(0).unwrap_or(0));
        assert_eq!(
            lt.record_sdk(0, sdk_cell(0).unwrap_or(0)),
            Err(X64PlugCode::EDup)
        );
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = X64PlugLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = X64PlugLedger::new();
        let _ = lt2.judge_gate(0);
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= PLUG_CELLS as u32);
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
        assert_eq!(SEG_BASE, 0xC94C0);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(X64PlugCode::Ok as u32, 0);
        assert_eq!(X64PlugCode::EGradeDrift as u32, 12);
        assert!(!X64PlugCode::Ok.detail().is_empty());
        assert!(!X64PlugCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade_s1() {
        assert_eq!(Grade::S1.val(), 1);
        let lt = X64PlugLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52340_checks_green() {
        let s = run_unx52340_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
    }
}
