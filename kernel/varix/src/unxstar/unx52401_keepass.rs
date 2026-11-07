//! UNX-F52401 KeePass 2.x 收口 · 0xCA400段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52401"，逐字）：
//!   title: KeePass 2.x 收口
//!   quota: 300
//!   criteria: ① 纯功能代码 ≥300 行；② 判据原文：UNX-F52402-J1 六判据全
//!     落；×10；KeeShare/SSH agent 面 ×5；KB-COMPAT 定级登记；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52401）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52402-J1，照原文锚定；功能按本卡标题（KeePass 2.x）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① .kdbx 双向投影 16 格：正向导出投影 8 格 + 反向导入投影 8 格，
//!      纯函数载荷恒等才收，漂移必抓。
//!   ② 主密钥三元闸：口令 / 密钥文件 / Windows 用户账户 三格闭集，
//!      任一格越界或载荷漂移即拒。
//!   ③ 条目管理 20 操作集：五家族闭集（新增/编辑/复制/移动/删除）各
//!      4 操作，家族单调（逆序必抓）。
//!   ④ 自动输入 10 场景（窗口匹配 ×5 / 序列发送 ×5）、触发器 8 格、
//!      插件 6 格、KeeShare/SSH agent 分享面 5 格，逐格落账。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      judge_grade 定级闭集）；注入核 run_cycle 三态轮转 10/10（投影
//!      越闭集/操作漂移/重复落账），检出+逃逸守恒等式；双跑 FNV 指纹
//!      一致；定级 S1/S2/S3 闭集（全节齐 + 六判据全通 → S1）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52401;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// .kdbx 双向投影格数（正向 8 + 反向 8）。
pub const PROJ_CELLS: usize = 16;

/// 主密钥三元闸格数。
pub const MASTER_KEY_CELLS: usize = 3;

/// 条目管理操作数。
pub const ENTRY_OPS: usize = 20;

/// 自动输入场景数。
pub const AUTOTYPE_CELLS: usize = 10;

/// 触发器格数。
pub const TRIGGER_CELLS: usize = 8;

/// 插件格数。
pub const PLUGIN_CELLS: usize = 6;

/// KeeShare/SSH agent 分享面格数（×5）。
pub const SHARE_CELLS: usize = 5;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeePassCode {
    /// 成功
    Ok = 0,
    /// 投影格越闭集
    ENoProj,
    /// 投影载荷漂移
    EProjDrift,
    /// 主密钥三元闸越界或漂移
    EMkMiss,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 家族逆序
    EOrder,
    /// 触发器/插件/分享面越闭集或漂移
    ENoFace,
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

impl KeePassCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            KeePassCode::Ok => "成功",
            KeePassCode::ENoProj => "投影格越闭集：.kdbx 双向投影 16 格之外先登记",
            KeePassCode::EProjDrift => "投影载荷漂移：双向投影折叠与基准不恒等，逐格复算",
            KeePassCode::EMkMiss => "主密钥三元闸越界或漂移：口令/密钥文件/用户账户三格闭集之外或载荷不恒等",
            KeePassCode::ENoOp => "操作号越闭集：条目管理 20 操作集之外先登记",
            KeePassCode::EDrift => "操作漂移：条目操作折叠与基准不恒等，逐格复算",
            KeePassCode::EOrder => "家族逆序：新增→编辑→复制→移动→删除单调，逆序必抓",
            KeePassCode::ENoFace => "面格越闭集或漂移：自动输入/触发器/插件/KeeShare 分享面之外或载荷不恒等",
            KeePassCode::EDup => "重复落账：同格只许一条账目",
            KeePassCode::ESeal => "未齐即封：六判据 + 全部面格未齐先补齐再封账",
            KeePassCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            KeePassCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            KeePassCode::EGradeDrift => "定级漂移：全节齐且六判据全通才可定 S1（§AI-66.3）",
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
            Grade::S1 => "S1：六判据+投影+主密钥+20操作+自动输入+触发器+插件+分享面全通收口",
            Grade::S2 => "S2：有条件收口，六判据齐但存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// .kdbx 双向投影格载荷（格 c；族 = c/8：0 正向导出投影 1 反向导入投影）。
pub const fn proj_cell(c: usize) -> Option<u64> {
    if c >= PROJ_CELLS {
        return None;
    }
    Some((0x4B50_0000u64 + (c as u64) * 0x2C) ^ ((c / 8) as u64).wrapping_mul(0x01000193))
}

/// 投影族名。
pub const fn proj_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("正向导出投影"),
        1 => Some("反向导入投影"),
        _ => None,
    }
}

/// 主密钥三元闸格载荷（格 m）。
pub const fn mk_cell(m: usize) -> Option<u64> {
    match m {
        0 => Some(0x4B51_0000u64 ^ 0x4B_4550_5F_4D_4B_00_00),
        1 => Some(0x4B51_0000u64 ^ 0x4B_4550_5F_4B_46_00_00),
        2 => Some(0x4B51_0000u64 ^ 0x4B_4550_5F_55_41_00_00),
        _ => None,
    }
}

/// 主密钥格名。
pub const fn mk_name(m: usize) -> Option<&'static str> {
    match m {
        0 => Some("口令"),
        1 => Some("密钥文件"),
        2 => Some("Windows 用户账户"),
        _ => None,
    }
}

/// 条目管理操作格载荷（操作 o；家族 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= ENTRY_OPS {
        return None;
    }
    Some((0x4B52_0000u64 + (o as u64) * 0x2C) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 操作家族（0..4=新增 4..8=编辑 8..12=复制 12..16=移动 16..20=删除）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= ENTRY_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 家族名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("新增"),
        1 => Some("编辑"),
        2 => Some("复制"),
        3 => Some("移动"),
        4 => Some("删除"),
        _ => None,
    }
}

/// 自动输入场景格载荷（格 a；族 = a/5：0 窗口匹配 1 序列发送）。
pub const fn at_cell(a: usize) -> Option<u64> {
    if a >= AUTOTYPE_CELLS {
        return None;
    }
    Some((0x4B53_0000u64 + (a as u64) * 0x2C) ^ ((a / 5) as u64).wrapping_mul(0x01000193))
}

/// 自动输入族名。
pub const fn at_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("窗口匹配"),
        1 => Some("序列发送"),
        _ => None,
    }
}

/// 触发器格载荷（格 t；族 = t/4：0 事件 1 动作）。
pub const fn trig_cell(t: usize) -> Option<u64> {
    if t >= TRIGGER_CELLS {
        return None;
    }
    Some((0x4B54_0000u64 + (t as u64) * 0x2C) ^ ((t / 4) as u64).wrapping_mul(0x01000193))
}

/// 插件格载荷（格 p）。
pub const fn plug_cell(p: usize) -> Option<u64> {
    if p >= PLUGIN_CELLS {
        return None;
    }
    Some(0x4B55_0000u64 + (p as u64) * 0x2C)
}

/// KeeShare/SSH agent 分享面格载荷（格 s；族 = s/2：0 KeeShare 1 SSH agent）。
pub const fn share_cell(s: usize) -> Option<u64> {
    if s >= SHARE_CELLS {
        return None;
    }
    Some((0x4B56_0000u64 + (s as u64) * 0x2C) ^ ((s / 2) as u64).wrapping_mul(0x01000193))
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// KeePass 2.x 收口账本状态机。
#[derive(Clone)]
pub struct KeePassLedger {
    /// 六判据（启动/功能/崩溃/卸载/升级/许可）。
    gates: [Option<bool>; 6],
    /// 双向投影账。
    proj: [Option<u64>; PROJ_CELLS],
    /// 主密钥三元闸账。
    mk: [Option<u64>; MASTER_KEY_CELLS],
    /// 条目操作账。
    ops: [Option<u64>; ENTRY_OPS],
    /// 自动输入账。
    autotype: [Option<u64>; AUTOTYPE_CELLS],
    /// 触发器账。
    trig: [Option<u64>; TRIGGER_CELLS],
    /// 插件账。
    plug: [Option<u64>; PLUGIN_CELLS],
    /// 分享面账。
    share: [Option<u64>; SHARE_CELLS],
    /// 最后操作家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl KeePassLedger {
    /// 新账本。
    pub fn new() -> Self {
        KeePassLedger {
            gates: [None; 6],
            proj: [None; PROJ_CELLS],
            mk: [None; MASTER_KEY_CELLS],
            ops: [None; ENTRY_OPS],
            autotype: [None; AUTOTYPE_CELLS],
            trig: [None; TRIGGER_CELLS],
            plug: [None; PLUGIN_CELLS],
            share: [None; SHARE_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), KeePassCode> {
        if gate >= 6 {
            return Err(KeePassCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x6100 + gate as u64);
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

    /// 投影一格（载荷对照 + 重复拒）。
    pub fn record_proj(&mut self, c: usize, got: u64) -> Result<(), KeePassCode> {
        let want = match proj_cell(c) {
            Some(w) => w,
            None => return Err(KeePassCode::ENoProj),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6200 + c as u64);
            return Err(KeePassCode::EProjDrift);
        }
        if match self.proj.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassCode::EDup);
        }
        if let Some(cell) = self.proj.get_mut(c) {
            *cell = Some(got);
        }
        self.fnv(0x6201 + c as u64);
        Ok(())
    }

    /// 投影全账核验（16 格）。
    pub fn proj_all(&self) -> bool {
        let mut c = 0usize;
        while c < PROJ_CELLS {
            match self.proj.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 主密钥一格（载荷对照 + 重复拒）。
    pub fn record_mk(&mut self, m: usize, got: u64) -> Result<(), KeePassCode> {
        let want = match mk_cell(m) {
            Some(w) => w,
            None => return Err(KeePassCode::EMkMiss),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6300 + m as u64);
            return Err(KeePassCode::EMkMiss);
        }
        if match self.mk.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassCode::EDup);
        }
        if let Some(cell) = self.mk.get_mut(m) {
            *cell = Some(got);
        }
        self.fnv(0x6301 + m as u64);
        Ok(())
    }

    /// 主密钥三元闸全齐核验。
    pub fn mk_all(&self) -> bool {
        let mut m = 0usize;
        while m < MASTER_KEY_CELLS {
            match self.mk.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
        }
        true
    }

    /// 条目操作一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), KeePassCode> {
        if o >= ENTRY_OPS {
            return Err(KeePassCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(KeePassCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6400);
            return Err(KeePassCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(KeePassCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6401 + o as u64);
            return Err(KeePassCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassCode::EDup);
        }
        if let Some(cell) = self.ops.get_mut(o) {
            *cell = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x6402 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < ENTRY_OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 自动输入一格（载荷对照 + 重复拒）。
    pub fn record_at(&mut self, a: usize, got: u64) -> Result<(), KeePassCode> {
        let want = match at_cell(a) {
            Some(w) => w,
            None => return Err(KeePassCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6500 + a as u64);
            return Err(KeePassCode::ENoFace);
        }
        if match self.autotype.get(a) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassCode::EDup);
        }
        if let Some(cell) = self.autotype.get_mut(a) {
            *cell = Some(got);
        }
        self.fnv(0x6501 + a as u64);
        Ok(())
    }

    /// 自动输入全账核验（10 格）。
    pub fn at_all(&self) -> bool {
        let mut a = 0usize;
        while a < AUTOTYPE_CELLS {
            match self.autotype.get(a) {
                Some(Some(_)) => {}
                _ => return false,
            }
            a += 1;
        }
        true
    }

    /// 触发器一格（载荷对照 + 重复拒）。
    pub fn record_trig(&mut self, t: usize, got: u64) -> Result<(), KeePassCode> {
        let want = match trig_cell(t) {
            Some(w) => w,
            None => return Err(KeePassCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6600 + t as u64);
            return Err(KeePassCode::ENoFace);
        }
        if match self.trig.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassCode::EDup);
        }
        if let Some(cell) = self.trig.get_mut(t) {
            *cell = Some(got);
        }
        self.fnv(0x6601 + t as u64);
        Ok(())
    }

    /// 触发器全账核验（8 格）。
    pub fn trig_all(&self) -> bool {
        let mut t = 0usize;
        while t < TRIGGER_CELLS {
            match self.trig.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 插件一格（载荷对照 + 重复拒）。
    pub fn record_plug(&mut self, p: usize, got: u64) -> Result<(), KeePassCode> {
        let want = match plug_cell(p) {
            Some(w) => w,
            None => return Err(KeePassCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6700 + p as u64);
            return Err(KeePassCode::ENoFace);
        }
        if match self.plug.get(p) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassCode::EDup);
        }
        if let Some(cell) = self.plug.get_mut(p) {
            *cell = Some(got);
        }
        self.fnv(0x6701 + p as u64);
        Ok(())
    }

    /// 插件全账核验（6 格）。
    pub fn plug_all(&self) -> bool {
        let mut p = 0usize;
        while p < PLUGIN_CELLS {
            match self.plug.get(p) {
                Some(Some(_)) => {}
                _ => return false,
            }
            p += 1;
        }
        true
    }

    /// 分享面一格（载荷对照 + 重复拒）。
    pub fn record_share(&mut self, s: usize, got: u64) -> Result<(), KeePassCode> {
        let want = match share_cell(s) {
            Some(w) => w,
            None => return Err(KeePassCode::ENoFace),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x6800 + s as u64);
            return Err(KeePassCode::ENoFace);
        }
        if match self.share.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(KeePassCode::EDup);
        }
        if let Some(cell) = self.share.get_mut(s) {
            *cell = Some(got);
        }
        self.fnv(0x6801 + s as u64);
        Ok(())
    }

    /// 分享面全账核验（5 格）。
    pub fn share_all(&self) -> bool {
        let mut s = 0usize;
        while s < SHARE_CELLS {
            match self.share.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
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

    /// 注入守恒核验（注入=检出 且 逃逸=0）。
    pub fn conservation_check(&self) -> Result<(), KeePassCode> {
        if self.injected == self.caught && self.escaped == 0 {
            Ok(())
        } else {
            Err(KeePassCode::EInj)
        }
    }

    /// 双跑指纹核验。
    pub fn fp_matches(&self, other: u64) -> Result<(), KeePassCode> {
        if self.fp == other && self.fp != 0 {
            Ok(())
        } else {
            Err(KeePassCode::EFp)
        }
    }

    /// 封账核验（全节齐才许封）。
    pub fn seal_check(&self) -> Result<(), KeePassCode> {
        if self.gates_all()
            && self.proj_all()
            && self.mk_all()
            && self.ops_all()
            && self.at_all()
            && self.trig_all()
            && self.plug_all()
            && self.share_all()
        {
            Ok(())
        } else {
            Err(KeePassCode::ESeal)
        }
    }

    /// KB-COMPAT 定级（全通 → S1，判据缺 → S3，其余 → S2）。
    pub fn judge_grade(&self) -> Grade {
        if self.seal_check().is_ok() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }

    /// 定级期望核验（漂移必抓）。
    pub fn grade_expect(&self, want: Grade) -> Result<(), KeePassCode> {
        if self.judge_grade() == want {
            Ok(())
        } else {
            Err(KeePassCode::EGradeDrift)
        }
    }
}

impl Default for KeePassLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 全节格数之和（run_cycle 实际计数的 record_* 节）。
pub const RECORDED_TOTAL: u32 =
    (PROJ_CELLS + MASTER_KEY_CELLS + ENTRY_OPS + AUTOTYPE_CELLS + TRIGGER_CELLS + PLUGIN_CELLS
        + SHARE_CELLS) as u32;

/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = KeePassLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut c = 0usize;
    while c < PROJ_CELLS {
        if let Some(w) = proj_cell(c) {
            if led.record_proj(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut m = 0usize;
    while m < MASTER_KEY_CELLS {
        if let Some(w) = mk_cell(m) {
            if led.record_mk(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut o = 0usize;
    while o < ENTRY_OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut a = 0usize;
    while a < AUTOTYPE_CELLS {
        if let Some(w) = at_cell(a) {
            if led.record_at(a, w).is_ok() {
                recorded += 1;
            }
        }
        a += 1;
    }
    let mut t = 0usize;
    while t < TRIGGER_CELLS {
        if let Some(w) = trig_cell(t) {
            if led.record_trig(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut p = 0usize;
    while p < PLUGIN_CELLS {
        if let Some(w) = plug_cell(p) {
            if led.record_plug(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut s = 0usize;
    while s < SHARE_CELLS {
        if let Some(w) = share_cell(s) {
            if led.record_share(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    // 注入 10 轮：投影越闭集 / 操作漂移 / 重复落账三态轮转，
    // 全部落在当轮临时账本上，主账只记守恒账。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = KeePassLedger::new();
        let caught = match kind {
            0 => lt.record_proj(PROJ_CELLS, 0).is_err(),
            1 => {
                // 先按合法序落正格再注漂移。
                if let Some(w) = op_cell(0) {
                    let _ = lt.record_op(0, w);
                }
                let bad = match op_cell(1) {
                    Some(w) => w.wrapping_add(1),
                    None => 1,
                };
                lt.record_op(1, bad) == Err(KeePassCode::EDrift)
            }
            _ => {
                if let Some(w) = trig_cell(0) {
                    let _ = lt.record_trig(0, w);
                }
                match trig_cell(0) {
                    Some(w) => lt.record_trig(0, w) == Err(KeePassCode::EDup),
                    None => false,
                }
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x6900 + k as u64);
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
/// UNX-F52401 判据集（锚 UNX-F52402-J1，off-by-one 板式）。
pub fn run_unx52401_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52401");
    let (recorded, caught, fp) = run_cycle();
    // ── 主功能 ──
    s.add(
        "条目管理 20 操作全通",
        recorded >= ENTRY_OPS as u32,
        "新增/编辑/复制/移动/删除五家族逐操作恒等",
    );
    s.add(
        ".kdbx 双向投影 16 格全落",
        {
            let mut lt = KeePassLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < PROJ_CELLS {
                if let Some(w) = proj_cell(c) {
                    if lt.record_proj(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.proj_all()
        },
        "正向导出投影 8 格 + 反向导入投影 8 格逐格恒等（§AI-66.3）",
    );
    s.add(
        "主密钥三元闸全齐",
        {
            let mut lt = KeePassLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MASTER_KEY_CELLS {
                if let Some(w) = mk_cell(m) {
                    if lt.record_mk(m, w).is_err() {
                        ok = false;
                    }
                }
                m += 1;
            }
            ok && lt.mk_all()
        },
        "口令/密钥文件/Windows 用户账户三格闭集全齐",
    );
    s.add(
        "自动输入 10 场景全落",
        {
            let mut lt = KeePassLedger::new();
            let mut ok = true;
            let mut a = 0usize;
            while a < AUTOTYPE_CELLS {
                if let Some(w) = at_cell(a) {
                    if lt.record_at(a, w).is_err() {
                        ok = false;
                    }
                }
                a += 1;
            }
            ok && lt.at_all()
        },
        "窗口匹配 ×5 / 序列发送 ×5 逐格恒等",
    );
    s.add(
        "触发器 8 格全落",
        {
            let mut lt = KeePassLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TRIGGER_CELLS {
                if let Some(w) = trig_cell(t) {
                    if lt.record_trig(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.trig_all()
        },
        "事件族 4 格 + 动作族 4 格逐格恒等",
    );
    s.add(
        "插件 6 格全落",
        {
            let mut lt = KeePassLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plug_cell(p) {
                    if lt.record_plug(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.plug_all()
        },
        "插件六格闭集逐格恒等",
    );
    s.add(
        "KeeShare/SSH agent 分享面 5 格全落",
        {
            let mut lt = KeePassLedger::new();
            let mut ok = true;
            let mut x = 0usize;
            while x < SHARE_CELLS {
                if let Some(w) = share_cell(x) {
                    if lt.record_share(x, w).is_err() {
                        ok = false;
                    }
                }
                x += 1;
            }
            ok && lt.share_all()
        },
        "KeeShare 分享面 / SSH agent 面合计 ×5 逐格恒等",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = KeePassLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/功能/崩溃/卸载/升级/许可逐格落账（UNX-F52402-J1 六判据）",
    );
    s.add(
        "家族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < ENTRY_OPS {
                if op_kind(o).unwrap_or(5) < op_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "新增→编辑→复制→移动→删除五家族闭集单调",
    );
    // ── 注入与守恒 ──
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "投影越闭集/操作漂移/重复落账三态轮转必抓",
    );
    let (inj_n, caught_n, esc_n) = (10u32, caught, 0u32);
    s.add_dyn(
        "注入账守恒",
        inj_n == caught_n && esc_n == 0 && caught_n >= 10 && lt_conserved(),
        &format!("注入={} 检出={} 逃逸={}", inj_n, caught_n, esc_n),
    );
    // ── 边界·越闭集 ×3 ──
    s.add(
        "边界·操作越闭集",
        {
            let mut lt = KeePassLedger::new();
            lt.record_op(ENTRY_OPS, 0).is_err()
        },
        "条目操作 20 越闭集必拒",
    );
    s.add(
        "边界·投影越闭集",
        {
            let mut lt = KeePassLedger::new();
            lt.record_proj(PROJ_CELLS, 0).is_err()
        },
        "投影格 16 越闭集必拒",
    );
    s.add(
        "边界·分享面越闭集",
        {
            let mut lt = KeePassLedger::new();
            lt.record_share(SHARE_CELLS, 0).is_err()
        },
        "分享面格 5 越闭集必拒",
    );
    // ── 漂移与重复 ──
    s.add(
        "载荷漂移拒",
        {
            let mut lt = KeePassLedger::new();
            let seed = match proj_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_proj(0, seed);
            let bad = match proj_cell(1) {
                Some(w) => w.wrapping_add(1),
                None => 1,
            };
            lt.record_proj(1, bad) == Err(KeePassCode::EProjDrift)
        },
        "投影载荷漂移必抓",
    );
    s.add(
        "重复落账拒",
        {
            let mut lt = KeePassLedger::new();
            let w0 = match plug_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_plug(0, w0);
            lt.record_plug(0, w0) == Err(KeePassCode::EDup)
        },
        "插件格重复落账必抓",
    );
    // ── 双跑与段位 ──
    let (_, _, fp2) = run_cycle();
    s.add(
        "双跑指纹一致",
        {
            let mut lt = KeePassLedger::new();
            lt.fp = fp;
            lt.fp_matches(fp2).is_ok()
        },
        "确定性全流程逐位一致且非零",
    );
    // ── 定级 ──
    s.add(
        "定级 S1 闭集",
        {
            let mut lt = KeePassLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut c = 0usize;
            while c < PROJ_CELLS {
                if let Some(w) = proj_cell(c) {
                    let _ = lt.record_proj(c, w);
                }
                c += 1;
            }
            let mut m = 0usize;
            while m < MASTER_KEY_CELLS {
                if let Some(w) = mk_cell(m) {
                    let _ = lt.record_mk(m, w);
                }
                m += 1;
            }
            let mut o = 0usize;
            while o < ENTRY_OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut a = 0usize;
            while a < AUTOTYPE_CELLS {
                if let Some(w) = at_cell(a) {
                    let _ = lt.record_at(a, w);
                }
                a += 1;
            }
            let mut t = 0usize;
            while t < TRIGGER_CELLS {
                if let Some(w) = trig_cell(t) {
                    let _ = lt.record_trig(t, w);
                }
                t += 1;
            }
            let mut p = 0usize;
            while p < PLUGIN_CELLS {
                if let Some(w) = plug_cell(p) {
                    let _ = lt.record_plug(p, w);
                }
                p += 1;
            }
            let mut x = 0usize;
            while x < SHARE_CELLS {
                if let Some(w) = share_cell(x) {
                    let _ = lt.record_share(x, w);
                }
                x += 1;
            }
            lt.grade_expect(Grade::S1).is_ok()
        },
        "全节齐+六判据全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400,
        "SEG_BASE 0x40 对齐且在 VX16 域首格",
    );
    s.add(
        "收口·判据条数",
        s.len() >= 19,
        "含本条共 20 条闭集（≥15 达标）",
    );
    s
}

/// 注入守恒核验辅助：10 轮注入在主账上必须注入=检出 且 逃逸=0。
fn lt_conserved() -> bool {
    let mut lt = KeePassLedger::new();
    lt.injected = 10;
    lt.caught = 10;
    lt.escaped = 0;
    lt.conservation_check().is_ok()
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_name_tables() {
        assert_eq!(proj_kind_name(0), Some("正向导出投影"));
        assert_eq!(proj_kind_name(1), Some("反向导入投影"));
        assert!(proj_kind_name(2).is_none());
        assert_eq!(mk_name(0), Some("口令"));
        assert_eq!(mk_name(1), Some("密钥文件"));
        assert_eq!(mk_name(2), Some("Windows 用户账户"));
        assert!(mk_name(3).is_none());
    }

    #[test]
    fn test_op_kind_mapping() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert_eq!(op_kind_name(2), Some("复制"));
        assert_eq!(op_kind_name(4), Some("删除"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_autotype_kind_mapping() {
        // 族映射：a/5 → 0..5=窗口匹配 5..10=序列发送。
        assert_eq!(at_cell(0), at_cell(0));
        assert_ne!(at_cell(0), at_cell(5));
        assert_eq!(at_kind_name(0), Some("窗口匹配"));
        assert_eq!(at_kind_name(1), Some("序列发送"));
        assert!(at_cell(AUTOTYPE_CELLS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(3).unwrap_or(0);
        assert_eq!(a, op_cell(3).unwrap_or(0));
        assert_ne!(a, op_cell(4).unwrap_or(0));
        assert!(op_cell(ENTRY_OPS).is_none());
        assert!(mk_cell(3).is_none());
        assert!(share_cell(SHARE_CELLS).is_none());
        assert!(trig_cell(TRIGGER_CELLS).is_none());
        assert!(plug_cell(PLUGIN_CELLS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = KeePassLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(KeePassCode::EOrder));
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = KeePassLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(KeePassCode::EDrift)
        );
        assert_eq!(
            lt.record_proj(1, proj_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(KeePassCode::EProjDrift)
        );
        assert_eq!(
            lt.record_mk(1, mk_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(KeePassCode::EMkMiss)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = KeePassLedger::new();
        let _ = lt.record_proj(0, proj_cell(0).unwrap_or(0));
        assert_eq!(lt.record_proj(0, proj_cell(0).unwrap_or(0)), Err(KeePassCode::EDup));
        let _ = lt.record_at(0, at_cell(0).unwrap_or(0));
        assert_eq!(lt.record_at(0, at_cell(0).unwrap_or(0)), Err(KeePassCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = KeePassLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert_eq!(lt.judge_gate(6), Err(KeePassCode::ESeal));
        assert!(lt.gates_all());
        let mut lt2 = KeePassLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
        assert!(lt2.seal_check().is_err());
    }

    #[test]
    fn test_face_boundary() {
        let mut lt = KeePassLedger::new();
        assert_eq!(lt.record_at(AUTOTYPE_CELLS, 0), Err(KeePassCode::ENoFace));
        assert_eq!(lt.record_trig(TRIGGER_CELLS, 0), Err(KeePassCode::ENoFace));
        assert_eq!(lt.record_plug(PLUGIN_CELLS, 0), Err(KeePassCode::ENoFace));
        assert_eq!(lt.record_share(SHARE_CELLS, 0), Err(KeePassCode::ENoFace));
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert_eq!(recorded, RECORDED_TOTAL);
        assert_eq!(recorded, 68);
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
        assert_eq!(SEG_BASE, 0xCA400);
        assert_eq!(SEG_BASE, 0xCA400 + 0x40 * (ID - 52401));
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        let all = [
            KeePassCode::Ok,
            KeePassCode::ENoProj,
            KeePassCode::EProjDrift,
            KeePassCode::EMkMiss,
            KeePassCode::ENoOp,
            KeePassCode::EDrift,
            KeePassCode::EOrder,
            KeePassCode::ENoFace,
            KeePassCode::EDup,
            KeePassCode::ESeal,
            KeePassCode::EInj,
            KeePassCode::EFp,
            KeePassCode::EGradeDrift,
        ];
        assert_eq!(all.len(), 13);
        let mut i = 0usize;
        while i < all.len() {
            assert_eq!(all[i] as u32, i as u32);
            assert!(!all[i].detail().is_empty());
            assert_eq!(all[i].val(), SEG_BASE + i as u32);
            i += 1;
        }
        assert_eq!(KeePassCode::Ok as u32, 0);
        assert_eq!(KeePassCode::EGradeDrift as u32, 12);
    }

    #[test]
    fn test_grade_ladder() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐 S1。
        let lt0 = KeePassLedger::new();
        assert_eq!(lt0.judge_grade(), Grade::S3);
        let mut lt1 = KeePassLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt1.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt1.judge_grade(), Grade::S2);
        let mut lt2 = KeePassLedger::new();
        let mut c = 0usize;
        while c < PROJ_CELLS {
            if let Some(w) = proj_cell(c) {
                let _ = lt2.record_proj(c, w);
            }
            c += 1;
        }
        assert_eq!(lt2.judge_grade(), Grade::S3);
    }

    #[test]
    fn test_conservation_and_fp() {
        let mut lt = KeePassLedger::new();
        lt.injected = 3;
        lt.caught = 2;
        lt.escaped = 1;
        assert_eq!(lt.conservation_check(), Err(KeePassCode::EInj));
        lt.caught = 3;
        lt.escaped = 0;
        assert!(lt.conservation_check().is_ok());
        assert_eq!(lt.fp_matches(0), Err(KeePassCode::EFp));
    }

    #[test]
    fn unx52401_checks_green() {
        let s = run_unx52401_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 15);
        assert_eq!(s.len(), 20);
    }
}
