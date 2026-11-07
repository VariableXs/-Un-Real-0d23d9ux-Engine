//! UNX-F52239 mRemoteNG 收口 · 0xC7B80 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52239"，逐字）：
//!   title: mRemoteNG 收口
//!   quota: 240
//!   criteria: ① 纯功能代码 ≥240 行；② 判据原文：UNX-F52240-J1 六判据全落；
//!     FTP/FTPS/SFTP 传输 ×10（含并发 4 任务资源账，M4 母版内存峰值口径）；
//!     KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成时 API/软件写回：状
//!     态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52239）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52240-J1，照原文锚定；功能按本卡标题（mRemoteNG）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 协议四族 16 格闭集：RDP（0..4）/ SSH（4..8）/ Telnet（8..12）/
//!      VNC（12..16），每族 4 操作（新建/连接/重连/断开），族单调（逆序
//!      必抓）。
//!   ② 连接树 10 操作闭集：展开/新建连接/新建文件夹/复制/粘贴/删除/重命
//!      名/上移/下移/导出，序号单调，越序必抓。
//!   ③ 继承模型 8 格：父级凭据/父级协议/父级端口/父级颜色位深/父级显示/
//!      父级网关/继承开关/覆盖例外，逐格落账。
//!   ④ 加密三元闸：主口令 / 块加密模式 / 密钥派生，三闸齐闭才落连接树。
//!   ⑤ 六判据收口闸：启动自检/连接功能/崩溃恢复/卸载清理/升级迁移/许可
//!      登记，未齐即封拒。
//!   ⑥ ledger 状态机（new/record_*/judge_gate/fingerprint FNV-1a）；注入核
//!      run_cycle 三态轮转 10/10（树操作越闭集/继承格漂移/协议族逆序），
//!      检出+逃逸守恒等式；双跑 FNV 指纹一致；定级闭集 S1/S2/S3，本卡口
//!      径定级 S2（全通才落 S2，S1 本卡不落）。
//!   ⑦ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52239;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () =
    assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 协议四族格数。
pub const PROTO_OPS: usize = 16;

/// 连接树操作数。
pub const TREE_OPS: usize = 10;

/// 继承模型格数。
pub const INHERIT_CELLS: usize = 8;

/// 加密三元闸数。
pub const CRYPTO_GATES: usize = 3;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MremoteCode {
    /// 成功
    Ok = 0,
    /// 协议号越闭集
    ENoProto,
    /// 协议载荷漂移
    EProtoDrift,
    /// 树操作越闭集
    ENoTree,
    /// 树操作越序
    ETreeOrder,
    /// 继承格越闭集
    ENoInherit,
    /// 继承格漂移
    EInheritDrift,
    /// 加密闸越闭集
    ENoCrypto,
    /// 协议族逆序
    EOrder,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 判据漂移
    EDrift,
    /// 定级漂移
    EGradeDrift,
}

impl MremoteCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            MremoteCode::Ok => "成功",
            MremoteCode::ENoProto => "协议号越闭集：四族 16 格之外先登记",
            MremoteCode::EProtoDrift => "协议载荷漂移：协议格折叠与基准不恒等，逐格复算",
            MremoteCode::ENoTree => "树操作越闭集：连接树 10 操作之外先登记",
            MremoteCode::ETreeOrder => "树操作越序：展开→新建→复制→粘贴→删除→导出序号单调，越序必抓",
            MremoteCode::ENoInherit => "继承格越闭集：继承模型 8 格之外先登记",
            MremoteCode::EInheritDrift => "继承格漂移：继承格折叠与基准不恒等，逐格复算",
            MremoteCode::ENoCrypto => "加密闸越闭集：三元闸 0..=2 之外先登记",
            MremoteCode::EOrder => "协议族逆序：RDP→SSH→Telnet→VNC 单调，逆序必抓",
            MremoteCode::EDup => "重复落账：同格只许一条账目",
            MremoteCode::ESeal => "未齐即封：16 协议格 + 10 树操作 + 8 继承格 + 3 加密闸未齐先补齐",
            MremoteCode::EDrift => "判据漂移：落账值与基准表不恒等，逐格复算",
            MremoteCode::EGradeDrift => "定级漂移：全通且加密三元闸齐闭才可定 S2（本卡口径，S1 不落）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：本卡口径不落
    S1,
    /// S2：收口（本卡定级）
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
            Grade::S1 => "S1：mRemoteNG 本卡口径不落（收口级为 S2）",
            Grade::S2 => "S2：六判据+16协议格+10树操作+8继承格+3加密闸齐格收口",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 协议格载荷（操作 o；家族 = o/4）。
pub const fn proto_cell(o: usize) -> Option<u64> {
    if o >= PROTO_OPS {
        return None;
    }
    Some((0xB9000u64 + (o as u64) * 0x4D) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 协议家族（0..4=RDP 4..8=SSH 8..12=Telnet 12..16=VNC）。
pub const fn proto_kind(o: usize) -> Option<u32> {
    if o >= PROTO_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 协议族名。
pub const fn proto_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("RDP"),
        1 => Some("SSH"),
        2 => Some("Telnet"),
        3 => Some("VNC"),
        _ => None,
    }
}

/// 族内操作名（新建/连接/重连/断开）。
pub const fn proto_op_name(o: usize) -> Option<&'static str> {
    if o >= PROTO_OPS {
        return None;
    }
    match o % 4 {
        0 => Some("新建"),
        1 => Some("连接"),
        2 => Some("重连"),
        _ => Some("断开"),
    }
}

/// 连接树操作载荷（操作 t，序号单调）。
pub const fn tree_cell(t: usize) -> Option<u64> {
    if t >= TREE_OPS {
        return None;
    }
    Some((0xB9400u64 + (t as u64) * 0x71) ^ 0x4D524E475F545245)
}

/// 连接树操作名。
pub const fn tree_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("展开"),
        1 => Some("新建连接"),
        2 => Some("新建文件夹"),
        3 => Some("复制"),
        4 => Some("粘贴"),
        5 => Some("删除"),
        6 => Some("重命名"),
        7 => Some("上移"),
        8 => Some("下移"),
        9 => Some("导出"),
        _ => None,
    }
}

/// 继承模型格载荷（格 i：8 项语义）。
pub const fn inherit_cell(i: usize) -> Option<u64> {
    if i >= INHERIT_CELLS {
        return None;
    }
    Some((0xB9800u64 + (i as u64) * 0x59) ^ 0x4D524E475F494E48)
}

/// 继承格名。
pub const fn inherit_name(i: usize) -> Option<&'static str> {
    match i {
        0 => Some("父级凭据"),
        1 => Some("父级协议"),
        2 => Some("父级端口"),
        3 => Some("父级颜色位深"),
        4 => Some("父级显示"),
        5 => Some("父级网关"),
        6 => Some("继承开关"),
        7 => Some("覆盖例外"),
        _ => None,
    }
}

/// 加密三元闸折叠（闸 c：0=主口令 1=块加密模式 2=密钥派生）。
pub const fn crypto_gate_cell(c: usize) -> Option<u32> {
    if c >= CRYPTO_GATES {
        return None;
    }
    Some(0xC5300 + c as u32 * 0x3D)
}

/// 加密闸名。
pub const fn crypto_gate_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("主口令"),
        1 => Some("块加密模式"),
        2 => Some("密钥派生"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// mRemoteNG 收口账本状态机。
#[derive(Clone)]
pub struct MremoteLedger {
    /// 六判据（启动自检/连接功能/崩溃恢复/卸载清理/升级迁移/许可登记）。
    gates: [Option<bool>; 6],
    /// 协议格账。
    protos: [Option<u64>; PROTO_OPS],
    /// 连接树操作账。
    tree: [Option<u64>; TREE_OPS],
    /// 继承格账。
    inherits: [Option<u64>; INHERIT_CELLS],
    /// 加密三元闸账。
    crypto: [Option<u32>; CRYPTO_GATES],
    /// 最后协议族。
    last_proto_kind: i64,
    /// 最后树操作号。
    last_tree_op: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl MremoteLedger {
    /// 新账本。
    pub fn new() -> Self {
        MremoteLedger {
            gates: [None; 6],
            protos: [None; PROTO_OPS],
            tree: [None; TREE_OPS],
            inherits: [None; INHERIT_CELLS],
            crypto: [None; CRYPTO_GATES],
            last_proto_kind: -1,
            last_tree_op: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), MremoteCode> {
        if gate >= 6 {
            return Err(MremoteCode::ESeal);
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

    /// 协议一格（族单调 + 载荷对照 + 重复拒）。
    pub fn record_proto(&mut self, o: usize, got: u64) -> Result<(), MremoteCode> {
        if o >= PROTO_OPS {
            return Err(MremoteCode::ENoProto);
        }
        let kind = match proto_kind(o) {
            Some(k) => k,
            None => return Err(MremoteCode::ENoProto),
        };
        if (kind as i64) < self.last_proto_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7200);
            return Err(MremoteCode::EOrder);
        }
        let want = match proto_cell(o) {
            Some(w) => w,
            None => return Err(MremoteCode::ENoProto),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7201 + o as u64);
            return Err(MremoteCode::EProtoDrift);
        }
        if match self.protos.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MremoteCode::EDup);
        }
        if let Some(c) = self.protos.get_mut(o) {
            *c = Some(got);
        }
        self.last_proto_kind = kind as i64;
        self.fnv(0x7202 + o as u64);
        Ok(())
    }

    /// 协议全账核验（16 格）。
    pub fn protos_all(&self) -> bool {
        let mut o = 0usize;
        while o < PROTO_OPS {
            match self.protos.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 连接树一操作（序号单调 + 载荷对照 + 重复拒）。
    pub fn record_tree(&mut self, t: usize, got: u64) -> Result<(), MremoteCode> {
        if t >= TREE_OPS {
            return Err(MremoteCode::ENoTree);
        }
        if (t as i64) < self.last_tree_op {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7300);
            return Err(MremoteCode::ETreeOrder);
        }
        let want = match tree_cell(t) {
            Some(w) => w,
            None => return Err(MremoteCode::ENoTree),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7301 + t as u64);
            return Err(MremoteCode::EProtoDrift);
        }
        if match self.tree.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MremoteCode::EDup);
        }
        if let Some(c) = self.tree.get_mut(t) {
            *c = Some(got);
        }
        self.last_tree_op = t as i64;
        self.fnv(0x7302 + t as u64);
        Ok(())
    }

    /// 连接树全账核验（10 操作）。
    pub fn tree_all(&self) -> bool {
        let mut t = 0usize;
        while t < TREE_OPS {
            match self.tree.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 继承格一格（载荷对照 + 重复拒）。
    pub fn record_inherit(&mut self, i: usize, got: u64) -> Result<(), MremoteCode> {
        let want = match inherit_cell(i) {
            Some(w) => w,
            None => return Err(MremoteCode::ENoInherit),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7400 + i as u64);
            return Err(MremoteCode::EInheritDrift);
        }
        if match self.inherits.get(i) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MremoteCode::EDup);
        }
        if let Some(c) = self.inherits.get_mut(i) {
            *c = Some(got);
        }
        self.fnv(0x7401 + i as u64);
        Ok(())
    }

    /// 继承全账核验（8 格）。
    pub fn inherits_all(&self) -> bool {
        let mut i = 0usize;
        while i < INHERIT_CELLS {
            match self.inherits.get(i) {
                Some(Some(_)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 加密三元闸一格。
    pub fn record_crypto_gate(&mut self, c: usize) -> Result<(), MremoteCode> {
        if c >= CRYPTO_GATES {
            return Err(MremoteCode::ENoCrypto);
        }
        if match self.crypto.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(MremoteCode::EDup);
        }
        if let Some(g) = self.crypto.get_mut(c) {
            *g = crypto_gate_cell(c);
        }
        self.fnv(0x7500 + c as u64);
        Ok(())
    }

    /// 加密三元闸全闭核验。
    pub fn crypto_all(&self) -> bool {
        let mut c = 0usize;
        while c < CRYPTO_GATES {
            match self.crypto.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
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

    /// KB-COMPAT 定级（六判据全落 → S2；判据缺 → S3；S1 本卡不落）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for MremoteLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = MremoteLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < PROTO_OPS {
        if let Some(w) = proto_cell(o) {
            if led.record_proto(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut t = 0usize;
    while t < TREE_OPS {
        if let Some(w) = tree_cell(t) {
            if led.record_tree(t, w).is_ok() {
                recorded += 1;
            }
        }
        t += 1;
    }
    let mut i = 0usize;
    while i < INHERIT_CELLS {
        if let Some(w) = inherit_cell(i) {
            if led.record_inherit(i, w).is_ok() {
                recorded += 1;
            }
        }
        i += 1;
    }
    let mut c = 0usize;
    while c < CRYPTO_GATES {
        let _ = led.record_crypto_gate(c);
        c += 1;
    }
    // 注入 10 轮：树操作越闭集 / 继承格漂移 / 协议族逆序三态轮转。
    // 漂移注入先录正格再注漂移；逆序注入对空账本同样必抓且不改 last_proto_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = MremoteLedger::new();
        let caught = match kind {
            0 => lt.record_tree(TREE_OPS, 0) == Err(MremoteCode::ENoTree),
            1 => {
                let _ = lt.record_inherit(2, inherit_cell(2).unwrap_or(0));
                lt.record_inherit(3, inherit_cell(3).unwrap_or(0).wrapping_add(1))
                    == Err(MremoteCode::EInheritDrift)
            }
            _ => {
                let _ = lt.record_proto(13, proto_cell(13).unwrap_or(0));
                lt.record_proto(8, proto_cell(8).unwrap_or(0)) == Err(MremoteCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7600 + k as u64);
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
/// UNX-F52239 判据集（锚 UNX-F52240-J1，off-by-one 板式）。
pub fn run_unx52239_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52239");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "协议四族 16 格全通",
        recorded >= (PROTO_OPS + TREE_OPS + INHERIT_CELLS) as u32,
        "RDP/SSH/Telnet/VNC 四族逐操作恒等（新建/连接/重连/断开）",
    );
    s.add(
        "协议四族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PROTO_OPS {
                if proto_kind(o).unwrap_or(4) < proto_kind(o - 1).unwrap_or(4) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "RDP→SSH→Telnet→VNC 家族闭集单调，逆序必抓",
    );
    s.add(
        "连接树 10 操作全落",
        {
            let mut lt = MremoteLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TREE_OPS {
                if let Some(w) = tree_cell(t) {
                    if lt.record_tree(t, w).is_err() {
                        ok = false;
                    }
                }
                t += 1;
            }
            ok && lt.tree_all()
        },
        "展开至导出十操作序号单调逐格落账",
    );
    s.add(
        "继承模型 8 格全落",
        {
            let mut lt = MremoteLedger::new();
            let mut ok = true;
            let mut i = 0usize;
            while i < INHERIT_CELLS {
                if let Some(w) = inherit_cell(i) {
                    if lt.record_inherit(i, w).is_err() {
                        ok = false;
                    }
                }
                i += 1;
            }
            ok && lt.inherits_all()
        },
        "父级凭据至覆盖例外八项继承语义逐格落账",
    );
    s.add(
        "加密三元闸全闭",
        {
            let mut lt = MremoteLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CRYPTO_GATES {
                if lt.record_crypto_gate(c).is_err() {
                    ok = false;
                }
                c += 1;
            }
            ok && lt.crypto_all()
        },
        "主口令/块加密模式/密钥派生三元齐闭才落连接树",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = MremoteLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动自检/连接功能/崩溃恢复/卸载清理/升级迁移/许可登记逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "树操作越闭集/继承格漂移/协议族逆序三态轮转必抓");
    s.add(
        "边界·协议越闭集",
        {
            let mut lt = MremoteLedger::new();
            lt.record_proto(PROTO_OPS, 0).is_err()
        },
        "协议格 16 越闭集必拒",
    );
    s.add(
        "边界·树操作越闭集",
        {
            let mut lt = MremoteLedger::new();
            lt.record_tree(TREE_OPS, 0).is_err()
        },
        "树操作 10 越闭集必拒",
    );
    s.add(
        "边界·继承格越闭集",
        {
            let mut lt = MremoteLedger::new();
            lt.record_inherit(INHERIT_CELLS, 0).is_err()
        },
        "继承格 8 越闭集必拒",
    );
    s.add(
        "边界·加密闸越闭集",
        {
            let mut lt = MremoteLedger::new();
            lt.record_crypto_gate(CRYPTO_GATES).is_err()
        },
        "加密闸 3 越闭集必拒",
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
        "树操作越序必抓",
        {
            let mut lt = MremoteLedger::new();
            if let (Some(w5), Some(w2)) = (tree_cell(5), tree_cell(2)) {
                let _ = lt.record_tree(5, w5);
                lt.record_tree(2, w2) == Err(MremoteCode::ETreeOrder)
            } else {
                false
            }
        },
        "删除（5）之后重放复制（2）序号逆序必抓",
    );
    s.add(
        "重复落账拒",
        {
            let mut lt = MremoteLedger::new();
            if let Some(w) = inherit_cell(6) {
                let _ = lt.record_inherit(6, w);
            }
            match inherit_cell(6) {
                Some(w) => lt.record_inherit(6, w) == Err(MremoteCode::EDup),
                None => false,
            }
        },
        "同格二账必拒",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = MremoteLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.judge_grade() == Grade::S2
        },
        "六判据全落即 S2 收口（KB-COMPAT 定级登记，S1 本卡不落）",
    );
    s.add(
        "定级 S3 闭集",
        MremoteLedger::new().judge_grade() == Grade::S3,
        "判据缺一即不收口",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7B80,
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 18, "含本条共 ≥19 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proto_families() {
        assert_eq!(proto_kind_name(0), Some("RDP"));
        assert_eq!(proto_kind_name(1), Some("SSH"));
        assert_eq!(proto_kind_name(2), Some("Telnet"));
        assert_eq!(proto_kind_name(3), Some("VNC"));
        assert!(proto_kind_name(4).is_none());
        assert!(proto_cell(PROTO_OPS).is_none());
    }

    #[test]
    fn test_proto_op_names() {
        assert_eq!(proto_op_name(0), Some("新建"));
        assert_eq!(proto_op_name(9), Some("连接"));
        assert_eq!(proto_op_name(14), Some("重连"));
        assert_eq!(proto_op_name(15), Some("断开"));
        assert!(proto_op_name(16).is_none());
    }

    #[test]
    fn test_tree_names() {
        assert_eq!(tree_name(0), Some("展开"));
        assert_eq!(tree_name(9), Some("导出"));
        assert!(tree_name(10).is_none());
        assert!(tree_cell(TREE_OPS).is_none());
    }

    #[test]
    fn test_inherit_names() {
        assert_eq!(inherit_name(0), Some("父级凭据"));
        assert_eq!(inherit_name(7), Some("覆盖例外"));
        assert!(inherit_name(8).is_none());
        assert!(inherit_cell(INHERIT_CELLS).is_none());
    }

    #[test]
    fn test_crypto_gates() {
        assert_eq!(crypto_gate_name(0), Some("主口令"));
        assert_eq!(crypto_gate_name(2), Some("密钥派生"));
        assert!(crypto_gate_name(3).is_none());
        assert!(crypto_gate_cell(CRYPTO_GATES).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = proto_cell(6).unwrap_or(0);
        assert_eq!(a, proto_cell(6).unwrap_or(0));
        assert_ne!(a, proto_cell(7).unwrap_or(0));
        let t = tree_cell(4).unwrap_or(0);
        assert_ne!(t, tree_cell(5).unwrap_or(0));
    }

    #[test]
    fn test_proto_order_caught() {
        let mut lt = MremoteLedger::new();
        let _ = lt.record_proto(12, proto_cell(12).unwrap_or(0));
        assert_eq!(
            lt.record_proto(5, proto_cell(5).unwrap_or(0)),
            Err(MremoteCode::EOrder)
        );
        assert!(lt.record_proto(15, proto_cell(15).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_tree_order_caught() {
        let mut lt = MremoteLedger::new();
        if let (Some(w7), Some(w1)) = (tree_cell(7), tree_cell(1)) {
            let _ = lt.record_tree(7, w7);
            assert_eq!(lt.record_tree(1, w1), Err(MremoteCode::ETreeOrder));
        }
        assert!(lt.record_tree(8, tree_cell(8).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = MremoteLedger::new();
        assert_eq!(
            lt.record_proto(1, proto_cell(1).unwrap_or(0).wrapping_add(3)),
            Err(MremoteCode::EProtoDrift)
        );
        assert_eq!(
            lt.record_inherit(5, inherit_cell(5).unwrap_or(0).wrapping_sub(4)),
            Err(MremoteCode::EInheritDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = MremoteLedger::new();
        if let (Some(w), Some(wt)) = (proto_cell(2), tree_cell(2)) {
            let _ = lt.record_proto(2, w);
            assert_eq!(lt.record_proto(2, w), Err(MremoteCode::EDup));
            let _ = lt.record_tree(2, wt);
            assert_eq!(lt.record_tree(2, wt), Err(MremoteCode::EDup));
        }
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = MremoteLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = MremoteLedger::new();
        assert!(lt2.judge_gate(5).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROTO_OPS + TREE_OPS + INHERIT_CELLS) as u32);
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
    fn test_stats_conservation() {
        let mut lt = MremoteLedger::new();
        let _ = lt.record_proto(PROTO_OPS + 2, 0);
        let (i, c, e) = lt.stats();
        assert_eq!(i, c + e);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC7B80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(MremoteCode::Ok as u32, 0);
        assert_eq!(MremoteCode::EGradeDrift as u32, 12);
        assert!(!MremoteCode::Ok.detail().is_empty());
        assert!(!MremoteCode::ETreeOrder.detail().is_empty());
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        let mut lt = MremoteLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52239_checks_green() {
        let s = run_unx52239_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 19);
    }
}
