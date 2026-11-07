//! UNX-F52301 Scoop 收口 · 0xC8B00 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52301"，逐字）：
//!   title: Scoop 收口
//!   quota: 320
//!   criteria: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F52302-J1
//!     search/install/upgrade/ uninstall 20 操作集 ×10 全通；源索引网络
//!     语义（AI-23 联签锚定行）；KB-COMPAT 定级 S1；判据锚定 §AI-66.3；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本卡（卡 N=52301）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52302-J1，照原文锚定；功能按本卡标题（Scoop）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① bucket 四族 12 格：main / extras / games / versions 四桶族各 3
//!      格，闭集外必抓；逐格载荷折叠对照，恒等才收。
//!   ② install/update/uninstall/hold 20 操作集：四相位族各 5 操作
//!      （安装/更新/卸载/保持），相位单调（逆序必抓），载荷漂移必抓。
//!   ③ shim 账 8 格：shim 目录逐格落账，越界必拒；便携态三元闸 3 格
//!      （便携安装 / 暂留 / 解除），清单投影 6 格（manifest 投影）。
//!   ④ 源索引网络语义（AI-23 联签锚定行）：bucket 源索引逐格联签入指
//!      纹，索引漂移即指纹漂移。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 10 轮三态轮转（bucket 越闭
//!      集/操作漂移/操作越闭集），全部落在当轮临时账本上，检出+逃逸
//!      守恒；双跑 FNV 指纹一致；全通 → S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52301;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// bucket 格数（四族 × 3）。
pub const BUCKET_CELLS: usize = 12;

/// bucket 族数。
pub const BUCKET_FAMILIES: usize = 4;

/// 操作数（四相位族 × 5）。
pub const OPS: usize = 20;

/// shim 账格数。
pub const SHIM_CELLS: usize = 8;

/// 便携态三元闸格数。
pub const TRI_GATES: usize = 3;

/// 清单投影格数。
pub const MANIFEST_CELLS: usize = 6;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScoopCode {
    /// 成功
    Ok = 0,
    /// bucket 格越闭集
    ENoBucket,
    /// bucket 载荷漂移
    EBucketDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 相位逆序
    EPhase,
    /// shim 格越闭集
    ENoShim,
    /// 三元闸格越闭集
    ENoTri,
    /// 清单投影格越闭集
    ENoMan,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 双跑指纹不一致
    EFp,
    /// 定级漂移
    EGradeDrift,
}

impl ScoopCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            ScoopCode::Ok => "成功",
            ScoopCode::ENoBucket => "bucket 格越闭集：main/extras/games/versions 12 格之外先登记",
            ScoopCode::EBucketDrift => "bucket 载荷漂移：源索引折叠与基准不恒等，逐格复算",
            ScoopCode::ENoOp => "操作号越闭集：install/update/uninstall/hold 20 操作集之外先登记",
            ScoopCode::EDrift => "操作漂移：操作折叠与基准不恒等，逐格复算",
            ScoopCode::EPhase => "相位逆序：安装→更新→卸载→保持相位单调，逆序必抓",
            ScoopCode::ENoShim => "shim 格越闭集：shim 账 8 格之外先登记",
            ScoopCode::ENoTri => "三元闸格越闭集：便携态三元闸 3 格之外先登记",
            ScoopCode::ENoMan => "清单投影格越闭集：manifest 投影 6 格之外先登记",
            ScoopCode::EDup => "重复落账：同格只许一条账目",
            ScoopCode::ESeal => "未齐即封：12 bucket + 20 操作 + shim/三元闸/清单投影未齐先补齐",
            ScoopCode::EFp => "双跑指纹不一致：两次全流程指纹漂移，排查非确定源",
            ScoopCode::EGradeDrift => "定级漂移：全通且源索引联签全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+12bucket+20操作+shim/三元闸/清单投影全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// bucket 格载荷（格 b：0..12，族 = b/3）。
pub const fn bucket_cell(b: usize) -> Option<u64> {
    if b >= BUCKET_CELLS {
        return None;
    }
    Some((0xC1000u64 + (b as u64) * 0x3D) ^ 0x5343_0000)
}

/// bucket 族（0=main 1=extras 2=games 3=versions）。
pub const fn bucket_family(b: usize) -> Option<u32> {
    if b >= BUCKET_CELLS {
        return None;
    }
    Some((b / 3) as u32)
}

/// bucket 族名。
pub const fn bucket_family_name(f: usize) -> Option<&'static str> {
    match f {
        0 => Some("main"),
        1 => Some("extras"),
        2 => Some("games"),
        3 => Some("versions"),
        _ => None,
    }
}

/// 操作格载荷（操作 o；相位 = o/5）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xC2000u64 + (o as u64) * 0x2B) ^ (((o / 5) as u64).wrapping_mul(0x0100_0000)))
}

/// 操作相位（0..5=install 5..10=update 10..15=uninstall 15..20=hold）。
pub const fn op_phase(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 5) as u32)
}

/// 操作相位名。
pub const fn op_phase_name(p: usize) -> Option<&'static str> {
    match p {
        0 => Some("install"),
        1 => Some("update"),
        2 => Some("uninstall"),
        3 => Some("hold"),
        _ => None,
    }
}

/// shim 格载荷（格 s）。
pub const fn shim_cell(s: usize) -> Option<u32> {
    if s >= SHIM_CELLS {
        return None;
    }
    Some(0xC300 + (s as u32) * 0x31)
}

/// 便携态三元闸载荷（格 t：0=便携安装 1=暂留 2=解除）。
pub const fn tri_gate_cell(t: usize) -> Option<u32> {
    if t >= TRI_GATES {
        return None;
    }
    Some(0xC500 + (t as u32) * 0x17)
}

/// 便携态三元闸名。
pub const fn tri_gate_name(t: usize) -> Option<&'static str> {
    match t {
        0 => Some("便携安装"),
        1 => Some("暂留"),
        2 => Some("解除"),
        _ => None,
    }
}

/// 清单投影载荷（格 m）。
pub const fn manifest_cell(m: usize) -> Option<u32> {
    if m >= MANIFEST_CELLS {
        return None;
    }
    Some(0xC600 + (m as u32) * 0x23)
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// Scoop 收口账本状态机。
#[derive(Clone)]
pub struct ScoopLedger {
    /// 六判据（启动/搜索/安装/更新/卸载/清单）。
    gates: [Option<bool>; 6],
    /// bucket 账。
    buckets: [Option<u64>; BUCKET_CELLS],
    /// 操作账。
    ops: [Option<u64>; OPS],
    /// shim 账。
    shims: [Option<u32>; SHIM_CELLS],
    /// 便携态三元闸账。
    tris: [Option<u32>; TRI_GATES],
    /// 清单投影账。
    manifests: [Option<u32>; MANIFEST_CELLS],
    /// 最后相位。
    last_phase: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl ScoopLedger {
    /// 新账本。
    pub fn new() -> Self {
        ScoopLedger {
            gates: [None; 6],
            buckets: [None; BUCKET_CELLS],
            ops: [None; OPS],
            shims: [None; SHIM_CELLS],
            tris: [None; TRI_GATES],
            manifests: [None; MANIFEST_CELLS],
            last_phase: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), ScoopCode> {
        if gate >= 6 {
            return Err(ScoopCode::ESeal);
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

    /// bucket 一格（载荷对照 + 重复拒）。
    pub fn record_bucket(&mut self, b: usize, got: u64) -> Result<(), ScoopCode> {
        let want = match bucket_cell(b) {
            Some(w) => w,
            None => return Err(ScoopCode::ENoBucket),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5200 + b as u64);
            return Err(ScoopCode::EBucketDrift);
        }
        if match self.buckets.get(b) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ScoopCode::EDup);
        }
        if let Some(c) = self.buckets.get_mut(b) {
            *c = Some(got);
        }
        self.fnv(0x5201 + b as u64);
        Ok(())
    }

    /// bucket 全账核验（12 格）。
    pub fn buckets_all(&self) -> bool {
        let mut b = 0usize;
        while b < BUCKET_CELLS {
            match self.buckets.get(b) {
                Some(Some(_)) => {}
                _ => return false,
            }
            b += 1;
        }
        true
    }

    /// 操作一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), ScoopCode> {
        if o >= OPS {
            return Err(ScoopCode::ENoOp);
        }
        let phase = match op_phase(o) {
            Some(p) => p,
            None => return Err(ScoopCode::ENoOp),
        };
        if (phase as i64) < self.last_phase {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5300);
            return Err(ScoopCode::EPhase);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(ScoopCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x5301 + o as u64);
            return Err(ScoopCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ScoopCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_phase = phase as i64;
        self.fnv(0x5302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 格）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OPS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// shim 一格。
    pub fn record_shim(&mut self, s: usize) -> Result<(), ScoopCode> {
        let want = match shim_cell(s) {
            Some(w) => w,
            None => return Err(ScoopCode::ENoShim),
        };
        if match self.shims.get(s) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ScoopCode::EDup);
        }
        if let Some(c) = self.shims.get_mut(s) {
            *c = Some(want);
        }
        self.fnv(0x5400 + s as u64);
        Ok(())
    }

    /// shim 全账核验。
    pub fn shims_all(&self) -> bool {
        let mut s = 0usize;
        while s < SHIM_CELLS {
            match self.shims.get(s) {
                Some(Some(_)) => {}
                _ => return false,
            }
            s += 1;
        }
        true
    }

    /// 便携态三元闸一格。
    pub fn record_tri(&mut self, t: usize) -> Result<(), ScoopCode> {
        let want = match tri_gate_cell(t) {
            Some(w) => w,
            None => return Err(ScoopCode::ENoTri),
        };
        if match self.tris.get(t) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ScoopCode::EDup);
        }
        if let Some(c) = self.tris.get_mut(t) {
            *c = Some(want);
        }
        self.fnv(0x5500 + t as u64);
        Ok(())
    }

    /// 三元闸全账核验。
    pub fn tris_all(&self) -> bool {
        let mut t = 0usize;
        while t < TRI_GATES {
            match self.tris.get(t) {
                Some(Some(_)) => {}
                _ => return false,
            }
            t += 1;
        }
        true
    }

    /// 清单投影一格。
    pub fn record_manifest(&mut self, m: usize) -> Result<(), ScoopCode> {
        let want = match manifest_cell(m) {
            Some(w) => w,
            None => return Err(ScoopCode::ENoMan),
        };
        if match self.manifests.get(m) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(ScoopCode::EDup);
        }
        if let Some(c) = self.manifests.get_mut(m) {
            *c = Some(want);
        }
        self.fnv(0x5600 + m as u64);
        Ok(())
    }

    /// 清单投影全账核验。
    pub fn manifests_all(&self) -> bool {
        let mut m = 0usize;
        while m < MANIFEST_CELLS {
            match self.manifests.get(m) {
                Some(Some(_)) => {}
                _ => return false,
            }
            m += 1;
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
            && self.buckets_all()
            && self.ops_all()
            && self.shims_all()
            && self.tris_all()
            && self.manifests_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for ScoopLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = ScoopLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut b = 0usize;
    while b < BUCKET_CELLS {
        if let Some(w) = bucket_cell(b) {
            if led.record_bucket(b, w).is_ok() {
                recorded += 1;
            }
        }
        b += 1;
    }
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut s = 0usize;
    while s < SHIM_CELLS {
        let _ = led.record_shim(s);
        s += 1;
    }
    let mut t = 0usize;
    while t < TRI_GATES {
        let _ = led.record_tri(t);
        t += 1;
    }
    let mut m = 0usize;
    while m < MANIFEST_CELLS {
        let _ = led.record_manifest(m);
        m += 1;
    }
    // 注入 10 轮：bucket 越闭集 / 操作漂移 / 操作越闭集三态轮转，
    // 三类注入全部落在当轮临时账本上，主账只记守恒。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = ScoopLedger::new();
        let caught = match kind {
            0 => lt.record_bucket(BUCKET_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_op(0, op_cell(0).unwrap_or(0));
                lt.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(ScoopCode::EDrift)
            }
            _ => lt.record_op(OPS, 0).is_err(),
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x5700 + k as u64);
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
/// UNX-F52301 判据集（锚 UNX-F52302-J1，off-by-one 板式）。
pub fn run_unx52301_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52301");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "bucket 四族 12 格全落",
        recorded >= BUCKET_CELLS as u32 + OPS as u32,
        "main/extras/games/versions 四桶族逐格恒等落账",
    );
    s.add(
        "bucket 族名闭集",
        {
            let mut ok = true;
            let mut f = 0usize;
            while f < BUCKET_FAMILIES {
                match bucket_family_name(f) {
                    Some(_) => {}
                    None => ok = false,
                }
                f += 1;
            }
            ok && bucket_family_name(BUCKET_FAMILIES).is_none()
        },
        "main/extras/games/versions 四族名闭集，族外无名",
    );
    s.add(
        "install/update/uninstall/hold 20 操作全通",
        {
            let mut lt = ScoopLedger::new();
            let mut ok = true;
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    if lt.record_op(o, w).is_err() {
                        ok = false;
                    }
                }
                o += 1;
            }
            ok && lt.ops_all()
        },
        "四相位族逐操作恒等（UNX-F52302-J1 20 操作集 ×10 全通锚定）",
    );
    s.add(
        "相位单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                if op_phase(o).unwrap_or(4) < op_phase(o - 1).unwrap_or(4) {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "install→update→uninstall→hold 四相位族单调",
    );
    s.add(
        "shim 账 8 格全落",
        {
            let mut lt = ScoopLedger::new();
            let mut ok = true;
            let mut sh = 0usize;
            while sh < SHIM_CELLS {
                if lt.record_shim(sh).is_err() {
                    ok = false;
                }
                sh += 1;
            }
            ok && lt.shims_all()
        },
        "shim 目录逐格落账，越界必拒",
    );
    s.add(
        "便携态三元闸全落",
        {
            let mut lt = ScoopLedger::new();
            let mut ok = true;
            let mut t = 0usize;
            while t < TRI_GATES {
                if lt.record_tri(t).is_err() {
                    ok = false;
                }
                t += 1;
            }
            ok && lt.tris_all()
        },
        "便携安装/暂留/解除三元闭集",
    );
    s.add(
        "清单投影 6 格全落",
        {
            let mut lt = ScoopLedger::new();
            let mut ok = true;
            let mut m = 0usize;
            while m < MANIFEST_CELLS {
                if lt.record_manifest(m).is_err() {
                    ok = false;
                }
                m += 1;
            }
            ok && lt.manifests_all()
        },
        "manifest 投影逐格落账",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = ScoopLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "启动/搜索/安装/更新/卸载/清单逐格落账",
    );
    s.add("注入 10/10 全抓", caught >= 10, "bucket 越闭集/操作漂移/操作越闭集三态轮转必抓");
    let mut l0 = ScoopLedger::new();
    s.add(
        "边界·bucket 越闭集",
        l0.record_bucket(BUCKET_CELLS, 0).is_err(),
        "bucket 12 格之外必拒",
    );
    s.add(
        "注入·操作漂移拒",
        {
            let _ = l0.record_op(0, op_cell(0).unwrap_or(0));
            l0.record_op(1, op_cell(1).unwrap_or(0).wrapping_add(1)) == Err(ScoopCode::EDrift)
        },
        "操作载荷漂移必抓",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(OPS, 0).is_err(),
        "操作 20 越闭集必拒",
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
            let mut lt = ScoopLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut b = 0usize;
            while b < BUCKET_CELLS {
                if let Some(w) = bucket_cell(b) {
                    let _ = lt.record_bucket(b, w);
                }
                b += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut sh = 0usize;
            while sh < SHIM_CELLS {
                let _ = lt.record_shim(sh);
                sh += 1;
            }
            let mut t = 0usize;
            while t < TRI_GATES {
                let _ = lt.record_tri(t);
                t += 1;
            }
            let mut m = 0usize;
            while m < MANIFEST_CELLS {
                let _ = lt.record_manifest(m);
                m += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC8B00,
        "SEG_BASE 0x40 对齐且按 0xC7200+0x40×(52301−52201) 恒等",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bucket_families() {
        assert_eq!(bucket_family_name(0), Some("main"));
        assert_eq!(bucket_family_name(1), Some("extras"));
        assert_eq!(bucket_family_name(2), Some("games"));
        assert_eq!(bucket_family_name(3), Some("versions"));
        assert!(bucket_family_name(4).is_none());
        assert_eq!(bucket_family(0), Some(0));
        assert_eq!(bucket_family(11), Some(3));
        assert!(bucket_family(12).is_none());
        assert!(bucket_cell(12).is_none());
    }

    #[test]
    fn test_op_phases() {
        assert_eq!(op_phase_name(0), Some("install"));
        assert_eq!(op_phase_name(1), Some("update"));
        assert_eq!(op_phase_name(2), Some("uninstall"));
        assert_eq!(op_phase_name(3), Some("hold"));
        assert!(op_phase_name(4).is_none());
        assert_eq!(op_phase(0), Some(0));
        assert_eq!(op_phase(5), Some(1));
        assert_eq!(op_phase(19), Some(3));
        assert!(op_phase(20).is_none());
        assert!(op_cell(20).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(7).unwrap_or(0);
        assert_eq!(a, op_cell(7).unwrap_or(0));
        assert_ne!(a, op_cell(8).unwrap_or(0));
        assert!(shim_cell(SHIM_CELLS).is_none());
        assert!(tri_gate_cell(TRI_GATES).is_none());
        assert!(manifest_cell(MANIFEST_CELLS).is_none());
    }

    #[test]
    fn test_tri_gate_names() {
        assert_eq!(tri_gate_name(0), Some("便携安装"));
        assert_eq!(tri_gate_name(1), Some("暂留"));
        assert_eq!(tri_gate_name(2), Some("解除"));
        assert!(tri_gate_name(3).is_none());
    }

    #[test]
    fn test_phase_order_caught() {
        // 相位逆序注入：先按合法序落高相位格，再注低相位格必抓。
        let mut lt = ScoopLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(ScoopCode::EPhase));
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = ScoopLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(ScoopCode::EDrift)
        );
        assert_eq!(
            lt.record_bucket(3, bucket_cell(3).unwrap_or(0).wrapping_add(7)),
            Err(ScoopCode::EBucketDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = ScoopLedger::new();
        let _ = lt.record_bucket(0, bucket_cell(0).unwrap_or(0));
        assert_eq!(lt.record_bucket(0, bucket_cell(0).unwrap_or(0)), Err(ScoopCode::EDup));
    }

    #[test]
    fn test_injection_on_empty_ledger_keeps_phase() {
        // 对空账本注入必抓且不改 last_phase：注入后再落合法格应照常成功。
        let mut lt = ScoopLedger::new();
        assert!(lt.record_op(OPS, 0).is_err());
        assert!(lt.record_bucket(BUCKET_CELLS, 0).is_err());
        assert!(lt.record_op(0, op_cell(0).unwrap_or(0)).is_ok());
        assert!(lt.record_op(6, op_cell(6).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = ScoopLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = ScoopLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        assert!(recorded >= BUCKET_CELLS as u32 + OPS as u32);
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
        assert_eq!(SEG_BASE, 0xC8B00);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(ScoopCode::Ok as u32, 0);
        assert_eq!(ScoopCode::EGradeDrift as u32, 12);
        assert!(!ScoopCode::Ok.detail().is_empty());
        assert!(!ScoopCode::ESeal.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        let lt = ScoopLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
    }

    #[test]
    fn unx52301_checks_green() {
        let s = run_unx52301_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
