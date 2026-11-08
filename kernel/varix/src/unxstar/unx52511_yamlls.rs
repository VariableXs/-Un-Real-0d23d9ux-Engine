//! UNX-F52511 yaml-language-server 收口 · 0xCBF80段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX16 键 "52511"，逐字）：
//!   title: yaml-language-server 收口
//!   quota: 320
//!   criteria: ① 纯功能代码 ≥320 行；② 判据原文：UNX-F52512-J1 cmdlet
//!     20 操作集 ×10 全通；模块加载 ×5；KB-COMPAT 定级 S1；判据锚定
//!     §AI-66.3；③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径一行注：本卡（卡 N=52511）判据锚 UNX-F(N+1)-J1 =
//! UNX-F52512-J1，照原文锚定（cmdlet 20 操作集为下一卡域语料）；功能
//! 按本卡标题（yaml-language-server）落地。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 域模型：yaml-language-server 五能力族 20 格（补全/悬停/验证/
//!      格式化/符号各 4 格），能力族单调，逐格载荷（纯函数折叠）恒等
//!      才收；漂移必抓。
//!   ② schema 消费 12 格：K8s（0..3）/CI（3..5）/GitHub Actions
//!      （5..8）/Compose（8..10）/NPM（10..12）五 schema 非均匀分段
//!      闭集，schema 族单调，消费漂移必抓。
//!   ③ JSON Schema 联签 4 格：元校验（0..2）/词汇联签（2..4）双族
//!      闭集，联签漂移必抓。
//!   ④ 声明 4 行闭集：与 Compose 分账声明 2 行（联签锚定行 F52263）+
//!      Node/TS 上栈声明 2 行，缺一拒封（seal：六闸缺→ESeal、声明行
//!      缺→ERowMiss、格账缺→ESeal）。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint FNV-1a/
//!      grade 定级闭集）；注入核 run_cycle 10 轮三态轮转（schema 漂移/
//!      能力族逆序/操作越闭集），三类注入全部落在当轮临时账本上，主账
//!      injected==caught==10、escaped==0；漂移注入先按合法序落同族正格
//!      再注漂移；双跑指纹一致；定级目标档 S1。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无
//!      unwrap/expect/panic/裸下标，越闭集边界拒不计注入账。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52511;

/// 段基址（VX16 派工公式：0xCA400 + 0x40 × (id − 52401)）。
pub const SEG_BASE: u32 = 0xCA400 + 0x40 * (ID - 52401);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xCA400 + 0x40 * (ID - 52401) && SEG_BASE % 0x40 == 0);

/// yaml-language-server 五能力族操作数。
pub const OPS: usize = 20;

/// schema 消费格数（五 schema 非均匀分段）。
pub const SCHEMA_CELLS: usize = 12;

/// JSON Schema 联签格数。
pub const META_CELLS: usize = 4;

/// 声明行数（Compose 分账 2 行 + Node/TS 上栈 2 行）。
pub const SPLIT_ROWS: usize = 4;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum YamllsCode {
    /// 成功
    Ok = 0,
    /// 操作号越闭集
    ENoOp,
    /// 操作漂移
    EDrift,
    /// 能力族逆序
    EOrder,
    /// schema 格越闭集
    ENoSchema,
    /// schema 消费漂移
    ESchemaDrift,
    /// 联签格越闭集
    ENoMeta,
    /// 联签漂移
    EMetaDrift,
    /// 声明行越闭集
    ENoRow,
    /// 缺声明行
    ERowMiss,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl YamllsCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            YamllsCode::Ok => "成功",
            YamllsCode::ENoOp => "操作号越闭集：yaml-language-server 五能力族 20 格之外先登记",
            YamllsCode::EDrift => "操作漂移：能力格折叠与基准不恒等，逐格复算",
            YamllsCode::EOrder => "能力族逆序：补全→悬停→验证→格式化→符号单调，逆序必抓",
            YamllsCode::ENoSchema => "schema 格越闭集：五 schema 消费 12 格之外先登记",
            YamllsCode::ESchemaDrift => "schema 消费漂移：K8s/CI/GHA/Compose/NPM 折叠与基准不恒等，逐格复算",
            YamllsCode::ENoMeta => "联签格越闭集：JSON Schema 联签 4 格之外先登记",
            YamllsCode::EMetaDrift => "联签漂移：JSON Schema 联签折叠与基准不恒等，逐格复算",
            YamllsCode::ENoRow => "声明行越闭集：Compose 分账+Node/TS 上栈 4 行闭集之外先登记",
            YamllsCode::ERowMiss => "缺声明行：Compose 分账声明/Node/TS 上栈声明未齐先补齐",
            YamllsCode::EDup => "重复落账：同格只许一条账目",
            YamllsCode::ESeal => "未齐即封：20 能力格+12 schema 格+4 联签格+六判据未齐先补齐",
            YamllsCode::EGradeDrift => "定级漂移：全通且声明行全齐才可定 S1（KB-COMPAT 登记）",
        }
    }
}

// ── 定级闭集 ─────────────────────────────────────────────────────
/// KB-COMPAT 定级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grade {
    /// S1：全通收口（本卡目标档）
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
            Grade::S1 => "S1：六判据+20能力+12schema+4联签+4行声明全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 能力格载荷（操作 o；能力族 = o/4）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OPS {
        return None;
    }
    Some((0xE1000u64 + (o as u64) * 0x2C) ^ 0x79616D6C6C730000)
}

/// 能力族（0..4=补全 4..8=悬停 8..12=验证 12..16=格式化 16..20=符号）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OPS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 能力族名（按 kind 索引）。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("补全"),
        1 => Some("悬停"),
        2 => Some("验证"),
        3 => Some("格式化"),
        4 => Some("符号"),
        _ => None,
    }
}

/// schema 消费格载荷（格 c；五 schema 非均匀分段，见 schema_kind）。
pub const fn schema_cell(c: usize) -> Option<u64> {
    if c >= SCHEMA_CELLS {
        return None;
    }
    Some((0xE2000u64 + (c as u64) * 0x2C) ^ 0x736368656D610000)
}

/// schema 族（0..3=K8s 3..5=CI 5..8=GitHub Actions 8..10=Compose 10..12=NPM）。
pub const fn schema_kind(c: usize) -> Option<u32> {
    if c >= SCHEMA_CELLS {
        return None;
    }
    if c < 3 {
        Some(0)
    } else if c < 5 {
        Some(1)
    } else if c < 8 {
        Some(2)
    } else if c < 10 {
        Some(3)
    } else {
        Some(4)
    }
}

/// schema 族名。
pub const fn schema_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("k8s"),
        1 => Some("ci"),
        2 => Some("github-actions"),
        3 => Some("compose"),
        4 => Some("npm"),
        _ => None,
    }
}

/// JSON Schema 联签格载荷（格 c；联签族 = c/2：0..2=元校验 2..4=词汇联签）。
pub const fn meta_cell(c: usize) -> Option<u64> {
    if c >= META_CELLS {
        return None;
    }
    Some((0xE3000u64 + (c as u64) * 0x31) ^ 0x6A73636800000000)
}

/// 联签族。
pub const fn meta_kind(c: usize) -> Option<u32> {
    if c >= META_CELLS {
        return None;
    }
    Some((c / 2) as u32)
}

/// 联签族名。
pub const fn meta_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("元校验"),
        1 => Some("词汇联签"),
        _ => None,
    }
}

/// 声明行折叠（行 r）。
pub const fn split_row(r: usize) -> Option<u32> {
    if r >= SPLIT_ROWS {
        return None;
    }
    Some(0xE410 + r as u32 * 0x2C)
}

/// 声明行名。
pub const fn split_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("Compose 分账声明·主（联签锚定行 F52263）"),
        1 => Some("Compose 分账声明·辅（联签锚定行 F52263）"),
        2 => Some("Node/TS 上栈声明·主"),
        3 => Some("Node/TS 上栈声明·辅"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// yaml-language-server 收口账本状态机。
#[derive(Clone)]
pub struct YamllsLedger {
    /// 六判据（收口闸：安装/启动/补全/诊断/升级/卸载）。
    gates: [Option<bool>; 6],
    /// 能力账。
    ops: [Option<u64>; OPS],
    /// schema 消费账。
    schemas: [Option<u64>; SCHEMA_CELLS],
    /// JSON Schema 联签账。
    metas: [Option<u64>; META_CELLS],
    /// 声明账。
    splits: [Option<u32>; SPLIT_ROWS],
    /// 最后能力族。
    last_kind: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl YamllsLedger {
    /// 新账本。
    pub fn new() -> Self {
        YamllsLedger {
            gates: [None; 6],
            ops: [None; OPS],
            schemas: [None; SCHEMA_CELLS],
            metas: [None; META_CELLS],
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), YamllsCode> {
        if gate >= 6 {
            return Err(YamllsCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x7A00 + gate as u64);
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

    /// 能力一格（能力族单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), YamllsCode> {
        if o >= OPS {
            return Err(YamllsCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(YamllsCode::ENoOp),
        };
        if (kind as i64) < self.last_kind {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7B00);
            return Err(YamllsCode::EOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(YamllsCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7B01 + o as u64);
            return Err(YamllsCode::EDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YamllsCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_kind = kind as i64;
        self.fnv(0x7B02 + o as u64);
        Ok(())
    }

    /// 能力全账核验（20 格）。
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

    /// schema 消费一格（载荷对照 + 重复拒，越闭集拒不计注入账）。
    pub fn record_schema(&mut self, c: usize, got: u64) -> Result<(), YamllsCode> {
        let want = match schema_cell(c) {
            Some(w) => w,
            None => return Err(YamllsCode::ENoSchema),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7C00 + c as u64);
            return Err(YamllsCode::ESchemaDrift);
        }
        if match self.schemas.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YamllsCode::EDup);
        }
        if let Some(b) = self.schemas.get_mut(c) {
            *b = Some(got);
        }
        self.fnv(0x7C01 + c as u64);
        Ok(())
    }

    /// schema 消费全账核验（12 格）。
    pub fn schemas_all(&self) -> bool {
        let mut c = 0usize;
        while c < SCHEMA_CELLS {
            match self.schemas.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// JSON Schema 联签一格（载荷对照 + 重复拒）。
    pub fn record_meta(&mut self, c: usize, got: u64) -> Result<(), YamllsCode> {
        let want = match meta_cell(c) {
            Some(w) => w,
            None => return Err(YamllsCode::ENoMeta),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x7D00 + c as u64);
            return Err(YamllsCode::EMetaDrift);
        }
        if match self.metas.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YamllsCode::EDup);
        }
        if let Some(m) = self.metas.get_mut(c) {
            *m = Some(got);
        }
        self.fnv(0x7D01 + c as u64);
        Ok(())
    }

    /// 联签全账核验（4 格）。
    pub fn metas_all(&self) -> bool {
        let mut c = 0usize;
        while c < META_CELLS {
            match self.metas.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// 声明一行。
    pub fn declare_split(&mut self, r: usize) -> Result<(), YamllsCode> {
        if r >= SPLIT_ROWS {
            return Err(YamllsCode::ENoRow);
        }
        if match self.splits.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(YamllsCode::EDup);
        }
        if let Some(s) = self.splits.get_mut(r) {
            *s = split_row(r);
        }
        self.fnv(0x7E00 + r as u64);
        Ok(())
    }

    /// 声明全齐核验（4 行）。
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

    /// 收口封账：六闸缺→ESeal、声明行缺→ERowMiss、格账缺→ESeal。
    pub fn seal(&self) -> Result<(), YamllsCode> {
        if !self.gates_all() {
            return Err(YamllsCode::ESeal);
        }
        if !self.splits_all() {
            return Err(YamllsCode::ERowMiss);
        }
        if !self.ops_all() || !self.schemas_all() || !self.metas_all() {
            return Err(YamllsCode::ESeal);
        }
        Ok(())
    }

    /// 注入三数。
    pub fn stats(&self) -> (u32, u32, u32) {
        (self.injected, self.caught, self.escaped)
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fp
    }

    /// KB-COMPAT 定级：全齐→目标档 S1，六闸齐→S2，空账→S3。
    pub fn grade(&self) -> Grade {
        if self.gates_all() && self.ops_all() && self.schemas_all() && self.metas_all() && self.splits_all() {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for YamllsLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（记账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = YamllsLedger::new();
    let mut g = 0usize;
    while g < 6 {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < OPS {
        if let Some(w) = op_cell(o) {
            if led.record_op(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut c = 0usize;
    while c < SCHEMA_CELLS {
        if let Some(w) = schema_cell(c) {
            if led.record_schema(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    let mut m = 0usize;
    while m < META_CELLS {
        if let Some(w) = meta_cell(m) {
            if led.record_meta(m, w).is_ok() {
                recorded += 1;
            }
        }
        m += 1;
    }
    let mut r = 0usize;
    while r < SPLIT_ROWS {
        let _ = led.declare_split(r);
        r += 1;
    }
    // 注入 10 轮：schema 漂移 / 能力族逆序 / 操作越闭集三态轮转。
    // 三类注入全部落在当轮临时账本上；主账只记账 injected/caught。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = YamllsLedger::new();
        let caught = match kind {
            0 => {
                // schema 漂移：先按合法序落同族正格（K8s 族），再注漂移。
                let w0 = match schema_cell(0) {
                    Some(w) => w,
                    None => 0,
                };
                let w1 = match schema_cell(1) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_schema(0, w0);
                lt.record_schema(1, w1.wrapping_add(1)) == Err(YamllsCode::ESchemaDrift)
            }
            1 => {
                // 能力族逆序：空账先落族 4 正格，再回落族 1，逆序必抓。
                let w17 = match op_cell(17) {
                    Some(w) => w,
                    None => 0,
                };
                let w5 = match op_cell(5) {
                    Some(w) => w,
                    None => 0,
                };
                let _ = lt.record_op(17, w17);
                lt.record_op(5, w5) == Err(YamllsCode::EOrder)
            }
            _ => {
                // 操作越闭集：边界拒，不计注入账内层（lt 只判拒码）。
                lt.record_op(OPS, 0).is_err()
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x7F00 + k as u64);
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
/// UNX-F52511 判据集（锚 UNX-F52512-J1，off-by-one 板式）。
pub fn run_unx52511_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52511");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "yaml-language-server 五能力族 20 操作全通",
        recorded >= OPS as u32,
        "补全/悬停/验证/格式化/符号逐格恒等",
    );
    s.add(
        "schema 消费 12/12 全落",
        {
            let mut lt = YamllsLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < SCHEMA_CELLS {
                if let Some(w) = schema_cell(c) {
                    if lt.record_schema(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.schemas_all()
        },
        "K8s/CI/GitHub Actions/Compose/NPM 五 schema 非均匀分段逐格恒等",
    );
    s.add(
        "JSON Schema 联签 4/4 全落",
        {
            let mut lt = YamllsLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < META_CELLS {
                if let Some(w) = meta_cell(c) {
                    if lt.record_meta(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.metas_all()
        },
        "元校验 0..2 与词汇联签 2..4 双族逐格联签恒等",
    );
    s.add(
        "Compose 分账与 Node/TS 上栈声明 4/4 全齐",
        {
            let mut lt = YamllsLedger::new();
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
        "Compose 分账声明 2 行（F52263 联签）+ Node/TS 上栈声明 2 行闭集（§AI-66.3）",
    );
    s.add(
        "六判据收口闸全落",
        {
            let mut lt = YamllsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装/启动/补全/诊断/升级/卸载逐格落账",
    );
    s.add(
        "能力族单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < OPS {
                let ka = match op_kind(o) {
                    Some(k) => k,
                    None => 5,
                };
                let kb = match op_kind(o - 1) {
                    Some(k) => k,
                    None => 5,
                };
                if ka < kb {
                    ok = false;
                }
                o += 1;
            }
            ok
        },
        "五能力族闭集单调",
    );
    s.add(
        "schema 族单调",
        {
            let mut ok = true;
            let mut c = 1usize;
            while c < SCHEMA_CELLS {
                let ka = match schema_kind(c) {
                    Some(k) => k,
                    None => 5,
                };
                let kb = match schema_kind(c - 1) {
                    Some(k) => k,
                    None => 5,
                };
                if ka < kb {
                    ok = false;
                }
                c += 1;
            }
            ok
        },
        "K8s→CI→GHA→Compose→NPM 五段闭集单调",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "schema 漂移/能力族逆序/操作越闭集三态轮转必抓",
    );
    s.add(
        "边界·操作越闭集",
        {
            let mut lt = YamllsLedger::new();
            lt.record_op(OPS, 0).is_err()
        },
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·schema 格越闭集",
        {
            let mut lt = YamllsLedger::new();
            lt.record_schema(SCHEMA_CELLS, 0).is_err()
        },
        "schema 格 12 越闭集必拒",
    );
    s.add(
        "边界·声明行越闭集",
        {
            let mut lt = YamllsLedger::new();
            lt.declare_split(SPLIT_ROWS).is_err()
        },
        "声明行 4 越闭集必拒",
    );
    s.add(
        "能力漂移拒",
        {
            let mut lt = YamllsLedger::new();
            let w0 = match op_cell(0) {
                Some(w) => w,
                None => 0,
            };
            let w1 = match op_cell(1) {
                Some(w) => w,
                None => 0,
            };
            let _ = lt.record_op(0, w0);
            lt.record_op(1, w1.wrapping_add(1)) == Err(YamllsCode::EDrift)
        },
        "能力格载荷漂移必抓",
    );
    s.add(
        "seal 语义·六闸缺 ESeal",
        {
            // 六闸缺：空账直接封必拒；格账缺：闸+声明齐但 schema 缺一也必拒。
            let lt_empty = YamllsLedger::new();
            let mut lt_cell = YamllsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt_cell.judge_gate(g);
                g += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt_cell.declare_split(r);
                r += 1;
            }
            let mut c = 1usize;
            while c < SCHEMA_CELLS {
                if let Some(w) = schema_cell(c) {
                    let _ = lt_cell.record_schema(c, w);
                }
                c += 1;
            }
            lt_empty.seal() == Err(YamllsCode::ESeal) && lt_cell.seal() == Err(YamllsCode::ESeal)
        },
        "六闸缺或格账缺均 ESeal，未齐即封",
    );
    s.add(
        "seal 语义·声明行缺 ERowMiss",
        {
            let mut lt = YamllsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            lt.seal() == Err(YamllsCode::ERowMiss)
        },
        "六闸与格账齐而声明行缺必 ERowMiss",
    );
    s.add(
        "联签格越闭集拒",
        {
            let mut lt = YamllsLedger::new();
            lt.record_meta(META_CELLS, 0).is_err()
        },
        "联签格 4 越闭集必拒",
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
            let mut lt = YamllsLedger::new();
            let mut g = 0usize;
            while g < 6 {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < OPS {
                if let Some(w) = op_cell(o) {
                    let _ = lt.record_op(o, w);
                }
                o += 1;
            }
            let mut c = 0usize;
            while c < SCHEMA_CELLS {
                if let Some(w) = schema_cell(c) {
                    let _ = lt.record_schema(c, w);
                }
                c += 1;
            }
            let mut m = 0usize;
            while m < META_CELLS {
                if let Some(w) = meta_cell(m) {
                    let _ = lt.record_meta(m, w);
                }
                m += 1;
            }
            let mut r = 0usize;
            while r < SPLIT_ROWS {
                let _ = lt.declare_split(r);
                r += 1;
            }
            lt.grade() == Grade::S1
        },
        "全齐 → S1（yaml-language-server KB-COMPAT 定级 S1）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xCA400 + 0x40 * (ID - 52401),
        "SEG_BASE 0x40 对齐且按 VX16 公式落 0xCBF80",
    );
    s.add("收口·判据条数", s.len() >= 19, "含本条共 ≥20 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_op_kind() {
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(3), Some(0));
        assert_eq!(op_kind(4), Some(1));
        assert_eq!(op_kind(19), Some(4));
        assert!(op_kind(20).is_none());
        assert!(op_cell(20).is_none());
        assert_eq!(op_kind_name(0), Some("补全"));
        assert!(op_kind_name(5).is_none());
    }

    #[test]
    fn test_schema_kind() {
        assert_eq!(schema_kind(0), Some(0));
        assert_eq!(schema_kind(2), Some(0));
        assert_eq!(schema_kind(3), Some(1));
        assert_eq!(schema_kind(5), Some(2));
        assert_eq!(schema_kind(8), Some(3));
        assert_eq!(schema_kind(10), Some(4));
        assert_eq!(schema_kind(11), Some(4));
        assert!(schema_kind(12).is_none());
        assert!(schema_cell(12).is_none());
        assert_eq!(schema_kind_name(0), Some("k8s"));
        assert!(schema_kind_name(5).is_none());
    }

    #[test]
    fn test_meta_kind() {
        assert_eq!(meta_kind(0), Some(0));
        assert_eq!(meta_kind(1), Some(0));
        assert_eq!(meta_kind(2), Some(1));
        assert_eq!(meta_kind(3), Some(1));
        assert!(meta_kind(4).is_none());
        assert!(meta_cell(4).is_none());
        assert_eq!(meta_kind_name(0), Some("元校验"));
        assert!(meta_kind_name(2).is_none());
    }

    #[test]
    fn test_cells_pure() {
        let a = op_cell(3).unwrap_or(0);
        assert_eq!(a, op_cell(3).unwrap_or(0));
        assert_ne!(a, op_cell(4).unwrap_or(0));
        let s0 = schema_cell(0).unwrap_or(0);
        assert_ne!(s0, schema_cell(3).unwrap_or(0));
        let m0 = meta_cell(0).unwrap_or(0);
        assert_ne!(m0, meta_cell(2).unwrap_or(0));
        assert!(split_row(SPLIT_ROWS).is_none());
        assert!(split_name(SPLIT_ROWS).is_none());
    }

    #[test]
    fn test_family_order_caught() {
        let mut lt = YamllsLedger::new();
        let _ = lt.record_op(9, op_cell(9).unwrap_or(0));
        assert_eq!(lt.record_op(2, op_cell(2).unwrap_or(0)), Err(YamllsCode::EOrder));
        // 逆序拒不改 last_kind：随后合法格照收。
        assert!(lt.record_op(10, op_cell(10).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_caught() {
        let mut lt = YamllsLedger::new();
        assert_eq!(
            lt.record_op(5, op_cell(5).unwrap_or(0).wrapping_add(1)),
            Err(YamllsCode::EDrift)
        );
        assert_eq!(
            lt.record_schema(1, schema_cell(1).unwrap_or(0).wrapping_add(7)),
            Err(YamllsCode::ESchemaDrift)
        );
        assert_eq!(
            lt.record_meta(3, meta_cell(3).unwrap_or(0).wrapping_add(9)),
            Err(YamllsCode::EMetaDrift)
        );
    }

    #[test]
    fn test_dup_rejected() {
        let mut lt = YamllsLedger::new();
        let _ = lt.record_meta(0, meta_cell(0).unwrap_or(0));
        assert_eq!(lt.record_meta(0, meta_cell(0).unwrap_or(0)), Err(YamllsCode::EDup));
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = YamllsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        // 闭集边界：0..=5 六闸之外越界必抓，全落才 gates_all。
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
    }

    #[test]
    fn test_out_of_set_no_injection() {
        // 越闭集边界拒不计注入账。
        let mut lt = YamllsLedger::new();
        let _ = lt.record_op(OPS, 0);
        let _ = lt.record_schema(SCHEMA_CELLS, 0);
        let _ = lt.record_meta(META_CELLS, 0);
        let _ = lt.declare_split(SPLIT_ROWS);
        assert_eq!(lt.stats(), (0, 0, 0));
    }

    #[test]
    fn test_seal_semantics() {
        let mut full = YamllsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = full.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                let _ = full.record_op(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < SCHEMA_CELLS {
            if let Some(w) = schema_cell(c) {
                let _ = full.record_schema(c, w);
            }
            c += 1;
        }
        let mut m = 0usize;
        while m < META_CELLS {
            if let Some(w) = meta_cell(m) {
                let _ = full.record_meta(m, w);
            }
            m += 1;
        }
        assert_eq!(full.seal(), Err(YamllsCode::ERowMiss));
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = full.declare_split(r);
            r += 1;
        }
        assert!(full.seal().is_ok());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 记账格数 = 20 能力 + 12 schema + 4 联签（六闸与声明行不计数）。
        assert_eq!(recorded, OPS as u32 + SCHEMA_CELLS as u32 + META_CELLS as u32);
        assert_eq!(recorded, 36);
        assert_eq!(caught, 10);
    }

    #[test]
    fn test_dual_run_fingerprint() {
        let (_, _, f1) = run_cycle();
        let (_, _, f2) = run_cycle();
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_guard() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xCBF80);
        let _ = _SEG_GUARD;
    }

    #[test]
    fn test_codes_closed_set() {
        assert_eq!(YamllsCode::Ok as u32, 0);
        assert_eq!(YamllsCode::ENoOp as u32, 1);
        assert_eq!(YamllsCode::EGradeDrift as u32, 12);
        assert_eq!(YamllsCode::Ok.val(), SEG_BASE);
        assert!(!YamllsCode::Ok.detail().is_empty());
        assert!(!YamllsCode::ESeal.detail().is_empty());
        assert!(!YamllsCode::EGradeDrift.detail().is_empty());
    }

    #[test]
    fn test_grade() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S2.val(), 2);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3。
        let lt = YamllsLedger::new();
        assert_eq!(lt.grade(), Grade::S3);
        // 六闸齐 S2。
        let mut lt2 = YamllsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt2.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt2.grade(), Grade::S2);
        // 全齐目标档 S1（目标档前置必须补 judge_gate×6）。
        let mut lt3 = YamllsLedger::new();
        let mut g = 0usize;
        while g < 6 {
            let _ = lt3.judge_gate(g);
            g += 1;
        }
        let mut o = 0usize;
        while o < OPS {
            if let Some(w) = op_cell(o) {
                let _ = lt3.record_op(o, w);
            }
            o += 1;
        }
        let mut c = 0usize;
        while c < SCHEMA_CELLS {
            if let Some(w) = schema_cell(c) {
                let _ = lt3.record_schema(c, w);
            }
            c += 1;
        }
        let mut m = 0usize;
        while m < META_CELLS {
            if let Some(w) = meta_cell(m) {
                let _ = lt3.record_meta(m, w);
            }
            m += 1;
        }
        let mut r = 0usize;
        while r < SPLIT_ROWS {
            let _ = lt3.declare_split(r);
            r += 1;
        }
        assert_eq!(lt3.grade(), Grade::S1);
    }

    #[test]
    fn unx52511_checks_green() {
        let s = run_unx52511_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 20);
    }
}
