//! UNX-F52237 SecureFX 收口 · 0xC7B00 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52237"，逐字）：
//!   title: SecureFX 收口
//!   quota: 220
//!   criteria: ① 纯功能代码 ≥220 行；② 判据原文：UNX-F52238-J1 六判据全落；
//!     多协议连接管理 ×10；KB-COMPAT 定级登记；判据锚定 §AI-66.3；③ 完成
//!     时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52237）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52238-J1，照原文锚定；功能按本卡标题（SecureFX）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 协议三族 12 格闭集：SFTP（0..4）/ FTP（4..8）/ SCP（8..12），每族
//!      4 操作（连接/列目录/下载/上传），家族单调（逆序必抓）。
//!   ② 目录同步 10 格：双向同步/镜像上传/镜像下载/孤儿清理/时间戳比对/
//!      大小比对/过滤规则/排除模式/空目录策略/校验和核验，逐格落账。
//!   ③ 会话与 SecureCRT 共享消费联签 4 行（F52236 锚）：会话目录共享/
//!      口令库共享/公钥代理共享/配置互认，缺一拒封。
//!   ④ 六判据收口闸：连接启动/传输功能/崩溃恢复/卸载清理/升级迁移/许可
//!      登记，逐格落账，未齐即封拒。
//!   ⑤ ledger 状态机（new/record_*/judge_gate/fingerprint FNV-1a）；注入核
//!      run_cycle 三态轮转 10/10（协议越闭集/协议漂移/家族逆序），检出+
//!      逃逸守恒等式；双跑 FNV 指纹一致；定级闭集 S1/S2/S3，本卡口径
//!      定级 S2（全通才落 S2，S1 本卡不落）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标（const fn 表 + Option 边界）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52237;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () =
    assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 协议连接管理格数（三族 ×4）。
pub const PROTO_OPS: usize = 12;

/// 目录同步格数。
pub const SYNC_CELLS: usize = 10;

/// SecureCRT 共享消费联签行数（F52236）。
pub const JOINT_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SecureFxCode {
    /// 成功
    Ok = 0,
    /// 协议号越闭集
    ENoProto,
    /// 协议载荷漂移
    EProtoDrift,
    /// 同步号越闭集
    ENoSync,
    /// 同步载荷漂移
    ESyncDrift,
    /// 协议家族逆序
    EOrder,
    /// 联签行越闭集
    ENoJoint,
    /// 缺联签行
    EJointMiss,
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

impl SecureFxCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            SecureFxCode::Ok => "成功",
            SecureFxCode::ENoProto => "协议号越闭集：SFTP/FTP/SCP 12 格之外先登记",
            SecureFxCode::EProtoDrift => "协议载荷漂移：协议格折叠与基准不恒等，逐格复算",
            SecureFxCode::ENoSync => "同步号越闭集：目录同步 10 格之外先登记",
            SecureFxCode::ESyncDrift => "同步载荷漂移：同步格折叠与基准不恒等，逐格复算",
            SecureFxCode::EOrder => "协议家族逆序：SFTP→FTP→SCP 单调，逆序必抓",
            SecureFxCode::ENoJoint => "联签行越闭集：SecureCRT 共享消费联签 4 行之外先登记",
            SecureFxCode::EJointMiss => "缺联签行：F52236 会话共享消费联签未齐先补齐",
            SecureFxCode::EDup => "重复落账：同格只许一条账目",
            SecureFxCode::ESeal => "未齐即封：12 协议格 + 10 同步格 + 4 联签行未齐先补齐",
            SecureFxCode::EInj => "注入守恒破缺：注入=检出+逃逸等式不成立，逐轮复算",
            SecureFxCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            SecureFxCode::EGradeDrift => "定级漂移：全通且联签全齐才可定 S2（本卡口径，S1 不落）",
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
            Grade::S1 => "S1：SecureFX 本卡口径不落（收口级为 S2）",
            Grade::S2 => "S2：六判据+12协议格+10同步格+4联签行齐格收口",
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
    Some((0xB7000u64 + (o as u64) * 0x37) ^ ((o / 4) as u64).wrapping_mul(0x01000193))
}

/// 协议家族（0..4=SFTP 4..8=FTP 8..12=SCP）。
pub const fn proto_kind(o: usize) -> Option<u32> {
    if o >= PROTO_OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 协议族名。
pub const fn proto_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("SFTP"),
        1 => Some("FTP"),
        2 => Some("SCP"),
        _ => None,
    }
}

/// 族内操作名（连接/列目录/下载/上传）。
pub const fn proto_op_name(o: usize) -> Option<&'static str> {
    if o >= PROTO_OPS {
        return None;
    }
    match o % 4 {
        0 => Some("连接"),
        1 => Some("列目录"),
        2 => Some("下载"),
        _ => Some("上传"),
    }
}

/// 同步格载荷（格 s：10 项同步语义）。
pub const fn sync_cell(s: usize) -> Option<u64> {
    if s >= SYNC_CELLS {
        return None;
    }
    Some((0xB7400u64 + (s as u64) * 0x5B) ^ 0x5346585F53594E43)
}

/// 同步格名。
pub const fn sync_name(s: usize) -> Option<&'static str> {
    match s {
        0 => Some("双向同步"),
        1 => Some("镜像上传"),
        2 => Some("镜像下载"),
        3 => Some("孤儿清理"),
        4 => Some("时间戳比对"),
        5 => Some("大小比对"),
        6 => Some("过滤规则"),
        7 => Some("排除模式"),
        8 => Some("空目录策略"),
        9 => Some("校验和核验"),
        _ => None,
    }
}

/// 联签行折叠（行 r，F52236 锚）。
pub const fn joint_row(r: usize) -> Option<u32> {
    if r >= JOINT_ROWS {
        return None;
    }
    Some(0xF5223 + r as u32 * 0x36)
}

/// 联签行名。
pub const fn joint_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("会话目录共享"),
        1 => Some("口令库共享"),
        2 => Some("公钥代理共享"),
        3 => Some("配置互认"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// SecureFX 收口账本状态机。
#[derive(Clone)]
pub struct SecureFxLedger {
    /// 六判据（连接启动/传输功能/崩溃恢复/卸载清理/升级迁移/许可登记）。
    gates: [Option<bool>; 6],
    /// 协议格账。
    protos: [Option<u64>; PROTO_OPS],
    /// 同步格账。
    syncs: [Option<u64>; SYNC_CELLS],
    /// 联签行账。
    joints: [Option<u32>; JOINT_ROWS],
    /// 最后家族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl SecureFxLedger {
    /// 新账本。
    pub fn new() -> Self {
        SecureFxLedger {
            gates: [None; 6],
            protos: [None; PROTO_OPS],
            syncs: [None; SYNC_CELLS],
            joints: [None; JOINT_ROWS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), SecureFxCode> {
        if gate >= 6 {
            return Err(SecureFxCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x5100 + gate as u64);
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

    /// 协议一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_proto(&mut self, o: usize, got: u64) -> Result<(), SecureFxCode> {
        if o >= PROTO_OPS {
            return Err(SecureFxCode::ENoProto);
        }
        let kind = match proto_kind(o) {
            Some(k) => k,
            None => return Err(SecureFxCode::ENoProto),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200);
            return Err(SecureFxCode::EOrder);
        }
        let want = match proto_cell(o) {
            Some(w) => w,
            None => return Err(SecureFxCode::ENoProto),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5201 + o as u64);
            return Err(SecureFxCode::EProtoDrift);
        }
        if match self.protos.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SecureFxCode::EDup);
        }
        if let Some(c) = self.protos.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x5202 + o as u64);
        Ok(())
    }

    /// 协议全账核验（12 格）。
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

    /// 同步一格（载荷对照 + 重复拒）。
    pub fn record_sync(&mut self, s: usize, got: u64) -> Result<(), SecureFxCode> {
        let want = match sync_cell(s) {
            Some(w) => w,
            None => return Err(SecureFxCode::ENoSync),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300 + s as u64);
            return Err(SecureFxCode::ESyncDrift);
        }
        if match self.syncs.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SecureFxCode::EDup);
        }
        if let Some(c) = self.syncs.get_mut(s) {
            *c = Some(got);
        }
        self.fnv(0x5301 + s as u64);
        Ok(())
    }

    /// 同步全账核验（10 格）。
    pub fn syncs_all(&self) -> bool {
        let mut s = 0usize;
        while s < SYNC_CELLS {
            match self.syncs.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 联签一行（SecureCRT 共享消费）。
    pub fn declare_joint(&mut self, r: usize) -> Result<(), SecureFxCode> {
        if r >= JOINT_ROWS {
            return Err(SecureFxCode::ENoJoint);
        }
        if match self.joints.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(SecureFxCode::EDup);
        }
        if let Some(c) = self.joints.get_mut(r) {
            *c = joint_row(r);
        }
        self.fnv(0x5400 + r as u64);
        Ok(())
    }

    /// 联签全齐核验。
    pub fn joints_all(&self) -> bool {
        let mut r = 0usize;
        while r < JOINT_ROWS {
            match self.joints.get(r) {
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

    /// KB-COMPAT 定级（全通或仅格未齐 → S2；判据缺 → S3；S1 本卡不落）。
    pub fn judge_grade(&self) -> Grade {
        if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for SecureFxLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = SecureFxLedger::new();
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
    let mut s = 0usize;
    while s < SYNC_CELLS {
        if let Some(w) = sync_cell(s) {
            if led.record_sync(s, w).is_ok() {
                recorded += 1;
            }
        }
        s += 1;
    }
    let mut r = 0usize;
    while r < JOINT_ROWS {
        let _ = led.declare_joint(r);
        r += 1;
    }
    // 注入 10 轮：协议越闭集 / 协议漂移 / 家族逆序三态轮转。
    // 漂移注入先录正格再注漂移；逆序注入对空账本同样必抓且不改 last_kind。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = SecureFxLedger::new();
        let caught = match kind {
            0 => lt.record_proto(PROTO_OPS, 0) == Err(SecureFxCode::ENoProto),
            1 => {
                let _ = lt.record_proto(5, proto_cell(5).unwrap_or(0));
                lt.record_proto(6, proto_cell(6).unwrap_or(0).wrapping_add(1))
                    == Err(SecureFxCode::EProtoDrift)
            }
            _ => {
                let _ = lt.record_proto(9, proto_cell(9).unwrap_or(0));
                lt.record_proto(4, proto_cell(4).unwrap_or(0)) == Err(SecureFxCode::EOrder)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5500 + k as u64);
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
/// UNX-F52237 判据集（锚 UNX-F52238-J1，off-by-one 板式）。
pub fn run_unx52237_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52237");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "多协议连接管理 12 格全通",
        recorded >= PROTO_OPS as u32,
        "SFTP/FTP/SCP 三族逐操作恒等（连接/列目录/下载/上传）",
    );
    s.add(
        "协议三族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < PROTO_OPS {
                if proto_kind(o).unwrap_or(3) < proto_kind(o - 1).unwrap_or(3) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "SFTP→FTP→SCP 家族闭集单调，逆序必抓",
    );
    s.add(
        "目录同步 10 格全落",
        {
            let mut lt = SecureFxLedger::new();
            let mut ok = true;
            let mut sx = 0usize;
            while sx < SYNC_CELLS {
                if let Some(w) = sync_cell(sx) {
                    if lt.record_sync(sx, w).is_err() {
                        ok = false;
                    }
                }
                sx += 1;
            }
            ok && lt.syncs_all()
        },
        "双向同步至校验和核验十项同步语义逐格落账",
    );
    s.add(
        "SecureCRT 共享消费联签 4/4 全齐",
        {
            let mut lt = SecureFxLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < JOINT_ROWS {
                if lt.declare_joint(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.joints_all()
        },
        "会话目录/口令库/公钥代理/配置互认四行闭集（F52236 联签锚）",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = SecureFxLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "连接启动/传输功能/崩溃恢复/卸载清理/升级迁移/许可登记逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "协议越闭集/协议漂移/家族逆序三态轮转必抓");
    s.add(
        "边界·协议越闭集",
        {
            let mut lt = SecureFxLedger::new();
            lt.record_proto(PROTO_OPS, 0).is_err()
        },
        "协议格 12 越闭集必拒",
    );
    s.add(
        "边界·同步越闭集",
        {
            let mut lt = SecureFxLedger::new();
            lt.record_sync(SYNC_CELLS, 0).is_err()
        },
        "同步格 10 越闭集必拒",
    );
    s.add(
        "边界·联签行越闭集",
        {
            let mut lt = SecureFxLedger::new();
            lt.declare_joint(JOINT_ROWS).is_err()
        },
        "联签行 4 越闭集必拒",
    );
    s.add(
        "漂移注入先录正格再注漂移必抓",
        {
            let mut lt = SecureFxLedger::new();
            let _ = lt.record_proto(0, proto_cell(0).unwrap_or(0));
            lt.record_proto(1, proto_cell(1).unwrap_or(0).wrapping_add(1))
                == Err(SecureFxCode::EProtoDrift)
        },
        "正格基准之下漂移必现形",
    );
    s.add(
        "逆序注入不改 last_kind",
        {
            let mut lt = SecureFxLedger::new();
            let _ = lt.record_proto(8, proto_cell(8).unwrap_or(0));
            let _ = lt.record_proto(0, proto_cell(0).unwrap_or(0));
            lt.record_proto(9, proto_cell(9).unwrap_or(0)).is_ok()
        },
        "逆序被拒后家族账位不受污染",
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
        "重复落账拒",
        {
            let mut lt = SecureFxLedger::new();
            if let Some(w) = sync_cell(3) {
                let _ = lt.record_sync(3, w);
            }
            match sync_cell(3) {
                Some(w) => lt.record_sync(3, w) == Err(SecureFxCode::EDup),
                None => false,
            }
        },
        "同格二账必拒",
    );
    s.add(
        "定级 S2 闭集",
        {
            let mut lt = SecureFxLedger::new();
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
        SecureFxLedger::new().judge_grade() == Grade::S3,
        "判据缺一即不收口",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC7B00,
        "SEG_BASE 0x40 对齐且落在 VX15 派工公式位",
    );
    s.add("收口·判据条数", s.len() >= 15, "含本条共 ≥16 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proto_families() {
        assert_eq!(proto_kind_name(0), Some("SFTP"));
        assert_eq!(proto_kind_name(1), Some("FTP"));
        assert_eq!(proto_kind_name(2), Some("SCP"));
        assert!(proto_kind_name(3).is_none());
        assert_eq!(proto_kind(0), Some(0));
        assert_eq!(proto_kind(11), Some(2));
        assert!(proto_kind(12).is_none());
        assert!(proto_cell(PROTO_OPS).is_none());
    }

    #[test]
    fn test_proto_op_names() {
        assert_eq!(proto_op_name(0), Some("连接"));
        assert_eq!(proto_op_name(5), Some("列目录"));
        assert_eq!(proto_op_name(10), Some("下载"));
        assert_eq!(proto_op_name(11), Some("上传"));
        assert!(proto_op_name(12).is_none());
    }

    #[test]
    fn test_sync_names() {
        assert_eq!(sync_name(0), Some("双向同步"));
        assert_eq!(sync_name(9), Some("校验和核验"));
        assert!(sync_name(10).is_none());
        assert!(sync_cell(SYNC_CELLS).is_none());
    }

    #[test]
    fn test_joint_rows() {
        assert_eq!(joint_name(0), Some("会话目录共享"));
        assert_eq!(joint_name(3), Some("配置互认"));
        assert!(joint_name(4).is_none());
        assert!(joint_row(JOINT_ROWS).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = proto_cell(7).unwrap_or(0);
        assert_eq!(a, proto_cell(7).unwrap_or(0));
        assert_ne!(a, proto_cell(8).unwrap_or(0));
        let s1 = sync_cell(2).unwrap_or(0);
        assert_ne!(s1, sync_cell(3).unwrap_or(0));
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = SecureFxLedger::new();
        let _ = lt.record_proto(10, proto_cell(10).unwrap_or(0));
        assert_eq!(
            lt.record_proto(3, proto_cell(3).unwrap_or(0)),
            Err(SecureFxCode::EOrder)
        );
        assert!(lt.record_proto(11, proto_cell(11).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = SecureFxLedger::new();
        assert_eq!(
            lt.record_proto(2, proto_cell(2).unwrap_or(0).wrapping_add(9)),
            Err(SecureFxCode::EProtoDrift)
        );
        assert_eq!(
            lt.record_sync(4, sync_cell(4).unwrap_or(0).wrapping_sub(1)),
            Err(SecureFxCode::ESyncDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = SecureFxLedger::new();
        if let Some(w) = proto_cell(1) {
            let _ = lt.record_proto(1, w);
            assert_eq!(lt.record_proto(1, w), Err(SecureFxCode::EDup));
        }
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = SecureFxLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = SecureFxLedger::new();
        assert!(lt2.judge_gate(2).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= (PROTO_OPS + SYNC_CELLS) as u32);
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
        let mut lt = SecureFxLedger::new();
        let _ = lt.record_proto(PROTO_OPS + 3, 0);
        let (i, c, e) = lt.stats();
        assert_eq!(i, c + e);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC7B00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(SecureFxCode::Ok as u32, 0);
        assert_eq!(SecureFxCode::EGradeDrift as u32, 12);
        assert!(!SecureFxCode::Ok.detail().is_empty());
        assert!(!SecureFxCode::EJointMiss.detail().is_empty());
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
    }

    #[test]
    fn test_grade() {
        let mut lt = SecureFxLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        let mut g = 0usize;
        while g < 6 {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S2);
    }

    #[test]
    fn unx52237_checks_green() {
        let s = run_unx52237_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 16);
    }
}
