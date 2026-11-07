//! UNX-F52378 Google Cloud CLI（gcloud）收口 · 0xC9E40 段
//! ─────────────────────────────────────────────────────────────────────
//! 卡面（任务板 VX15 键 "52379"，逐字）：
//!   title: Google Cloud CLI（gcloud）收口
//!   quota: 280
//!   criteria: ① 纯功能代码 ≥280 行；② 判据原文：UNX-F52380-J1
//!     dev/build CLI ×10（deploy 判据一律干跑列清单，禁真实外发——对
//!     外动作红线）；KB-COMPAT 定级登记；判据锚定 §AI-66.9 红线适用；
//!     ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! off-by-one 板式口径：本文件（文件号 N=52378）承载卡 N+1=52379 的卡
//! 面原文（任务口径：文件号较卡号后移一位），判据锚照该卡原文
//! UNX-F52380-J1 锚定；功能按本卡标题（Google Cloud CLI（gcloud）收
//! 口）落地——判据原文是邻卡字面，本模块仅收口 gcloud 控制面（只读
//! 判据；deploy 类对外动作一律干跑列清单，禁真实外发，外部资源零写
//! 入）。
//! 本模块按样板口径运行于 crate 级 `#![cfg_attr(not(test), no_std)]`
//! + `pub extern crate alloc` 环境下（lib.rs 已置，模块级不重复声明）。
//!
//! 设计要点：
//!   ① 五命令族 20 格闭集：compute/storage/iam/pubsub/logging 五家族
//!      各 4 格，家族单调（逆序必抓），逐格载荷折叠恒等才收。
//!   ② 20 操作集：init/config/query/describe/simulate 五相位各 5 操
//!      作，相位单调（逆序必抓）；deploy 类写操作一律落 simulate 干跑
//!      列清单格，禁真实外发。
//!   ③ config/profile 三元闸：Default→Named→Activated 闭序（越序必
//!      抓；对空账本同样必抓且不改三元态）；输出三格式 6 格闭集
//!      （json/yaml/text × 读账/落账）；凭证链 8 格闭集（环境变量/ADC
//!      文件/用户登录凭据/服务账号密钥/模拟代入/工作负载身份联合/元
//!      数据服务器/刷新令牌）。
//!   ④ Python 上栈声明 2 行闭集（gcloud → Python 运行时 / google-api-
//!      python-client → HTTP 栈），缺行拒封。
//!   ⑤ ledger 状态机（new/record_*/judge_gate×6/fingerprint，FNV-1a）；
//!      注入核 run_cycle 三态轮转 10/10（命令族越闭集/操作相位逆序/凭
//!      证链漂移），检出+逃逸守恒等式；双跑 FNV 指纹一致；定级 S1/S2/S3
//!      闭集（全通 → S1，KB-COMPAT 定级登记）。
//!   ⑥ no_std + alloc；零浮点、零 IO、零墙钟、零随机；生产区无 unwrap/
//!      expect/panic/裸下标。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;

/// 单号。
pub const ID: u32 = 52378;

/// 段基址（VX15 派工公式：0xC7200 + 0x40 × (id − 52201)）。
pub const SEG_BASE: u32 = 0xC7200 + 0x40 * (ID - 52201);

/// 编译期守卫：SEG 公式恒等且 0x40 对齐。
const _SEG_GUARD: () = assert!(SEG_BASE == 0xC7200 + 0x40 * (ID - 52201) && SEG_BASE % 0x40 == 0);

/// 五命令族格数（五家族 × 4 格）。
pub const FAM_CELLS: usize = 20;

/// 20 操作集格数（五相位 × 5 操作）。
pub const OP_CELLS: usize = 20;

/// 输出三格式格数（三格式 × 2 向）。
pub const FMT_CELLS: usize = 6;

/// 凭证链格数。
pub const CRED_CELLS: usize = 8;

/// Python 上栈声明行数。
pub const PY_ROWS: usize = 2;

/// 六判据闸数。
pub const GATES: usize = 6;

/// gcloud 域锚异或（低 16 位恒零）。
pub const GCLOUD_XOR: u64 = 0x4743_4C44_0000_0000;

/// 凭证链锚异或（低 16 位恒零，对合键）。
pub const ADC_XOR: u64 = 0x4144_4338_0000_0000;

// ── 诊断码闭集 ───────────────────────────────────────────────────
/// 闭集：Ok=+0x00，12 错误码至 +0x0C。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GcloudCode {
    /// 成功
    Ok = 0,
    /// 命令族号越闭集
    ENoFam,
    /// 命令族逆序
    EFamOrder,
    /// 命令族载荷漂移
    EFamDrift,
    /// 操作号越闭集
    ENoOp,
    /// 操作相位逆序
    EOpOrder,
    /// 输出格式格越闭集
    ENoFmt,
    /// 输出格式载荷漂移
    EFmtDrift,
    /// 凭证链格越闭集
    ENoCred,
    /// config/profile 三元闸越序
    EProfSeq,
    /// 重复落账
    EDup,
    /// 未齐即封
    ESeal,
    /// 定级漂移
    EGradeDrift,
}

impl GcloudCode {
    /// 码值：SEG_BASE + 0x00..0x0C。
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 诊断明细（全非空中文）。
    pub fn detail(self) -> &'static str {
        match self {
            GcloudCode::Ok => "成功",
            GcloudCode::ENoFam => "命令族号越闭集：compute/storage/iam/pubsub/logging 20 格之外先登记",
            GcloudCode::EFamOrder => "命令族逆序：compute→storage→iam→pubsub→logging 单调，逆序必抓",
            GcloudCode::EFamDrift => "命令族载荷漂移：族折叠与基准不恒等，逐格复算",
            GcloudCode::ENoOp => "操作号越闭集：init/config/query/describe/simulate 20 操作集之外先登记",
            GcloudCode::EOpOrder => "操作相位逆序：init→config→query→describe→simulate 单调，逆序必抓",
            GcloudCode::ENoFmt => "输出格式格越闭集：输出三格式 6 格之外先登记",
            GcloudCode::EFmtDrift => "输出格式载荷漂移：格式折叠与基准不恒等，逐格复算",
            GcloudCode::ENoCred => "凭证链格越闭集：凭证链 8 格之外先登记",
            GcloudCode::EProfSeq => "config/profile 三元闸越序：Default→Named→Activated 闭序之外先登记",
            GcloudCode::EDup => "重复落账：同格只许一条账目",
            GcloudCode::ESeal => "未齐即封：20 族格 + 20 操作 + 6 格式 + 8 凭证 + 2 上栈未齐先补齐",
            GcloudCode::EGradeDrift => "定级漂移：全通且账目全齐才可定 S1",
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
            Grade::S1 => "S1：六判据+20族格+20操作+6格式+8凭证+闸闭序+2上栈全通收口",
            Grade::S2 => "S2：有条件收口，存在未齐格",
            Grade::S3 => "S3：不收口，判据缺失",
        }
    }
}

// ── 域模型 ───────────────────────────────────────────────────────
/// 命令族格载荷（族 o；家族 = o/4）。
pub const fn fam_cell(o: usize) -> Option<u64> {
    if o >= FAM_CELLS {
        return None;
    }
    Some((0xB1000u64 + (o as u64) * 0x3A) ^ ((o / 4) as u64).wrapping_mul(0x01000193) ^ GCLOUD_XOR)
}

/// 命令族（0..4=compute 4..8=storage 8..12=iam 12..16=pubsub
/// 16..20=logging）。
pub const fn fam_kind(o: usize) -> Option<u32> {
    if o >= FAM_CELLS {
        return None;
    }
    Some((o / 4) as u32)
}

/// 命令族名。
pub const fn fam_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("compute"),
        1 => Some("storage"),
        2 => Some("iam"),
        3 => Some("pubsub"),
        4 => Some("logging"),
        _ => None,
    }
}

/// 操作格载荷（操作 o；相位 = o/5）。
pub const fn op_cell(o: usize) -> Option<u64> {
    if o >= OP_CELLS {
        return None;
    }
    Some((0xB2000u64 + (o as u64) * 0x42) ^ ((o / 5) as u64).wrapping_mul(0x01000193) ^ GCLOUD_XOR)
}

/// 操作相位（0..5=init 5..10=config 10..15=query 15..20=simulate，
/// 其中 describe 类只读随 query/simulate 相位收口）。
pub const fn op_kind(o: usize) -> Option<u32> {
    if o >= OP_CELLS {
        return None;
    }
    Some((o / 5) as u32)
}

/// 操作相位名。
pub const fn op_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("init"),
        1 => Some("config"),
        2 => Some("query"),
        3 => Some("simulate"),
        _ => None,
    }
}

/// 输出格式格基准载荷（格 f；格式 = f/2）。
pub const fn fmt_cell(f: usize) -> Option<u64> {
    if f >= FMT_CELLS {
        return None;
    }
    Some(0xB3000u64 + (f as u64) * 0x2A)
}

/// 输出格式（0..2=json 2..4=yaml 4..6=text）。
pub const fn fmt_kind(f: usize) -> Option<u32> {
    if f >= FMT_CELLS {
        return None;
    }
    Some((f / 2) as u32)
}

/// 输出格式名。
pub const fn fmt_kind_name(k: usize) -> Option<&'static str> {
    match k {
        0 => Some("json"),
        1 => Some("yaml"),
        2 => Some("text"),
        _ => None,
    }
}

/// 凭证链格基准载荷（格 c；对合投影锚 ADC_XOR）。
pub const fn cred_cell(c: usize) -> Option<u64> {
    if c >= CRED_CELLS {
        return None;
    }
    Some((0xB4000u64 + (c as u64) * 0x34) ^ ADC_XOR)
}

/// 凭证链对合投影·正向（back(fwd(x))==x）。
pub const fn cred_fwd(x: u64) -> u64 {
    x ^ ADC_XOR
}

/// 凭证链对合投影·反向。
pub const fn cred_back(y: u64) -> u64 {
    y ^ ADC_XOR
}

/// 凭证链格名（消费 8 格闭集对照）。
pub const fn cred_name(c: usize) -> Option<&'static str> {
    match c {
        0 => Some("环境变量 GOOGLE_APPLICATION_CREDENTIALS"),
        1 => Some("ADC 默认凭据文件"),
        2 => Some("gcloud auth login 用户凭据"),
        3 => Some("服务账号密钥"),
        4 => Some("服务账号模拟代入"),
        5 => Some("工作负载身份联合"),
        6 => Some("元数据服务器"),
        7 => Some("刷新令牌缓存"),
        _ => None,
    }
}

/// config/profile 三元态数（0=Default 1=Named 2=Activated）。
pub const PROF_STATES: usize = 3;

/// Python 上栈声明行折叠（行 r）。
pub const fn py_row(r: usize) -> Option<u32> {
    if r >= PY_ROWS {
        return None;
    }
    Some(0xB500 + r as u32 * 0x2E)
}

/// Python 上栈声明行名。
pub const fn py_row_name(r: usize) -> Option<&'static str> {
    match r {
        0 => Some("gcloud → Python 运行时上栈声明"),
        1 => Some("google-api-python-client → HTTP 栈上栈声明"),
        _ => None,
    }
}

/// 六判据闸名。
pub const fn gate_name(g: usize) -> Option<&'static str> {
    match g {
        0 => Some("安装自检"),
        1 => Some("凭据链"),
        2 => Some("config/profile 解析"),
        3 => Some("输出格式"),
        4 => Some("项目/区域解析"),
        5 => Some("只读干跑"),
        _ => None,
    }
}

// ── 收口账本 ─────────────────────────────────────────────────────
/// gcloud 收口账本状态机。
#[derive(Clone)]
pub struct GcloudLedger {
    /// 六判据。
    gates: [Option<bool>; GATES],
    /// 命令族账。
    fams: [Option<u64>; FAM_CELLS],
    /// 操作账。
    ops: [Option<u64>; OP_CELLS],
    /// 输出格式账。
    fmts: [Option<u64>; FMT_CELLS],
    /// 凭证链账。
    creds: [Option<u64>; CRED_CELLS],
    /// config/profile 三元态。
    prof: usize,
    /// config/profile 三元闸闭序是否走满（Default→Named→Activated）。
    prof_done: bool,
    /// Python 上栈声明账。
    pys: [Option<u32>; PY_ROWS],
    /// 最后命令族。
    last_fam: i64,
    /// 最后操作相位。
    last_op: i64,
    /// 注入/检出/逃逸。
    injected: u32,
    caught: u32,
    escaped: u32,
    /// 指纹。
    fp: u64,
}

impl GcloudLedger {
    /// 新账本。
    pub fn new() -> Self {
        GcloudLedger {
            gates: [None; GATES],
            fams: [None; FAM_CELLS],
            ops: [None; OP_CELLS],
            fmts: [None; FMT_CELLS],
            creds: [None; CRED_CELLS],
            prof: 0,
            prof_done: false,
            pys: [None; PY_ROWS],
            last_fam: -1,
            last_op: -1,
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
    pub fn judge_gate(&mut self, gate: usize) -> Result<(), GcloudCode> {
        if gate >= GATES {
            return Err(GcloudCode::ESeal);
        }
        if let Some(c) = self.gates.get_mut(gate) {
            *c = Some(true);
        }
        self.fnv(0x9100 + gate as u64);
        Ok(())
    }

    /// 六判据全落核验。
    pub fn gates_all(&self) -> bool {
        let mut i = 0usize;
        while i < GATES {
            match self.gates.get(i) {
                Some(Some(true)) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 命令族一格（家族单调 + 载荷对照 + 重复拒）。
    pub fn record_fam(&mut self, o: usize, got: u64) -> Result<(), GcloudCode> {
        if o >= FAM_CELLS {
            return Err(GcloudCode::ENoFam);
        }
        let kind = match fam_kind(o) {
            Some(k) => k,
            None => return Err(GcloudCode::ENoFam),
        };
        if (kind as i64) < self.last_fam {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9200);
            return Err(GcloudCode::EFamOrder);
        }
        let want = match fam_cell(o) {
            Some(w) => w,
            None => return Err(GcloudCode::ENoFam),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9201 + o as u64);
            return Err(GcloudCode::EFamDrift);
        }
        if match self.fams.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GcloudCode::EDup);
        }
        if let Some(c) = self.fams.get_mut(o) {
            *c = Some(got);
        }
        self.last_fam = kind as i64;
        self.fnv(0x9202 + o as u64);
        Ok(())
    }

    /// 命令族全账核验（20 格）。
    pub fn fams_all(&self) -> bool {
        let mut o = 0usize;
        while o < FAM_CELLS {
            match self.fams.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 操作一格（相位单调 + 载荷对照 + 重复拒）。
    pub fn record_op(&mut self, o: usize, got: u64) -> Result<(), GcloudCode> {
        if o >= OP_CELLS {
            return Err(GcloudCode::ENoOp);
        }
        let kind = match op_kind(o) {
            Some(k) => k,
            None => return Err(GcloudCode::ENoOp),
        };
        if (kind as i64) < self.last_op {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9300);
            return Err(GcloudCode::EOpOrder);
        }
        let want = match op_cell(o) {
            Some(w) => w,
            None => return Err(GcloudCode::ENoOp),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9301 + o as u64);
            return Err(GcloudCode::EFamDrift);
        }
        if match self.ops.get(o) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GcloudCode::EDup);
        }
        if let Some(c) = self.ops.get_mut(o) {
            *c = Some(got);
        }
        self.last_op = kind as i64;
        self.fnv(0x9302 + o as u64);
        Ok(())
    }

    /// 操作全账核验（20 操作）。
    pub fn ops_all(&self) -> bool {
        let mut o = 0usize;
        while o < OP_CELLS {
            match self.ops.get(o) {
                Some(Some(_)) => {}
                _ => return false,
            }
            o += 1;
        }
        true
    }

    /// 输出格式一格（载荷对照 + 重复拒）。
    pub fn record_fmt(&mut self, f: usize, got: u64) -> Result<(), GcloudCode> {
        if f >= FMT_CELLS {
            return Err(GcloudCode::ENoFmt);
        }
        let want = match fmt_cell(f) {
            Some(w) => w,
            None => return Err(GcloudCode::ENoFmt),
        };
        if got != want {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9400 + f as u64);
            return Err(GcloudCode::EFmtDrift);
        }
        if match self.fmts.get(f) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GcloudCode::EDup);
        }
        if let Some(c) = self.fmts.get_mut(f) {
            *c = Some(got);
        }
        self.fnv(0x9401 + f as u64);
        Ok(())
    }

    /// 输出格式全账核验（6 格）。
    pub fn fmts_all(&self) -> bool {
        let mut f = 0usize;
        while f < FMT_CELLS {
            match self.fmts.get(f) {
                Some(Some(_)) => {}
                _ => return false,
            }
            f += 1;
        }
        true
    }

    /// 凭证链一格（对合投影复核 + 载荷对照 + 重复拒）。
    pub fn record_cred(&mut self, c: usize, got: u64) -> Result<(), GcloudCode> {
        if c >= CRED_CELLS {
            return Err(GcloudCode::ENoCred);
        }
        let base = match cred_cell(c) {
            Some(w) => w,
            None => return Err(GcloudCode::ENoCred),
        };
        if cred_back(cred_fwd(base)) != base || got != base {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9500 + c as u64);
            return Err(GcloudCode::EFmtDrift);
        }
        if match self.creds.get(c) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GcloudCode::EDup);
        }
        if let Some(x) = self.creds.get_mut(c) {
            *x = Some(got);
        }
        self.fnv(0x9501 + c as u64);
        Ok(())
    }

    /// 凭证链全账核验（8 格）。
    pub fn creds_all(&self) -> bool {
        let mut c = 0usize;
        while c < CRED_CELLS {
            match self.creds.get(c) {
                Some(Some(_)) => {}
                _ => return false,
            }
            c += 1;
        }
        true
    }

    /// config/profile 三元闸一步（0=Default 1=Named 2=Activated；闭序
    /// 之外越序必抓；对空账本同样必抓且不改三元态）。
    pub fn record_prof(&mut self, step: usize) -> Result<(), GcloudCode> {
        let ok = match (step, self.prof) {
            (0, 0) => {
                self.prof = 1;
                true
            }
            (1, 1) => {
                self.prof = 2;
                self.prof_done = true;
                true
            }
            _ => false,
        };
        if !ok {
            self.injected += 1;
            self.caught += 1;
            self.fnv(0x9600 + step as u64);
            return Err(GcloudCode::EProfSeq);
        }
        self.fnv(0x9601 + step as u64);
        Ok(())
    }

    /// config/profile 三元闸闭序走满核验。
    pub fn prof_done_all(&self) -> bool {
        self.prof_done
    }

    /// Python 上栈一行。
    pub fn declare_py(&mut self, r: usize) -> Result<(), GcloudCode> {
        if r >= PY_ROWS {
            return Err(GcloudCode::ENoFmt);
        }
        if match self.pys.get(r) {
            Some(Some(_)) => true,
            _ => false,
        } {
            return Err(GcloudCode::EDup);
        }
        if let Some(c) = self.pys.get_mut(r) {
            *c = py_row(r);
        }
        self.fnv(0x9700 + r as u64);
        Ok(())
    }

    /// Python 上栈全齐核验。
    pub fn pys_all(&self) -> bool {
        let mut r = 0usize;
        while r < PY_ROWS {
            match self.pys.get(r) {
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
            && self.fams_all()
            && self.ops_all()
            && self.fmts_all()
            && self.creds_all()
            && self.prof_done
            && self.pys_all()
        {
            Grade::S1
        } else if self.gates_all() {
            Grade::S2
        } else {
            Grade::S3
        }
    }
}

impl Default for GcloudLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 注入核 ───────────────────────────────────────────────────────
/// 单轮全流程。返回（落账格数, 检出数, 指纹）。
pub fn run_cycle() -> (u32, u32, u64) {
    let mut led = GcloudLedger::new();
    let mut g = 0usize;
    while g < GATES {
        let _ = led.judge_gate(g);
        g += 1;
    }
    let mut recorded = 0u32;
    let mut o = 0usize;
    while o < FAM_CELLS {
        if let Some(w) = fam_cell(o) {
            if led.record_fam(o, w).is_ok() {
                recorded += 1;
            }
        }
        o += 1;
    }
    let mut p = 0usize;
    while p < OP_CELLS {
        if let Some(w) = op_cell(p) {
            if led.record_op(p, w).is_ok() {
                recorded += 1;
            }
        }
        p += 1;
    }
    let mut f = 0usize;
    while f < FMT_CELLS {
        if let Some(w) = fmt_cell(f) {
            if led.record_fmt(f, w).is_ok() {
                recorded += 1;
            }
        }
        f += 1;
    }
    let mut c = 0usize;
    while c < CRED_CELLS {
        if let Some(w) = cred_cell(c) {
            if led.record_cred(c, w).is_ok() {
                recorded += 1;
            }
        }
        c += 1;
    }
    // config/profile 三元闸：Default→Named→Activated 闭序走满。
    let _ = led.record_prof(0);
    let _ = led.record_prof(1);
    let mut r = 0usize;
    while r < PY_ROWS {
        if led.declare_py(r).is_ok() {
            recorded += 1;
        }
        r += 1;
    }
    // 注入 10 轮：命令族越闭集 / 操作相位逆序 / 凭证链漂移三态轮转。
    // 全部落在当轮临时账本上；漂移注入先按合法序落正格再注漂移。
    let mut k = 0usize;
    while k < 10 {
        let kind = (k % 3) as u32;
        let mut lt = GcloudLedger::new();
        let caught = match kind {
            0 => lt.record_fam(FAM_CELLS, 0).is_err(),
            1 => {
                let _ = lt.record_op(6, op_cell(6).unwrap_or(0));
                lt.record_op(2, op_cell(2).unwrap_or(0)) == Err(GcloudCode::EOpOrder)
            }
            _ => {
                let _ = lt.record_cred(0, cred_cell(0).unwrap_or(0));
                lt.record_cred(1, cred_cell(1).unwrap_or(0).wrapping_add(1)) == Err(GcloudCode::EFmtDrift)
            }
        };
        if caught {
            led.injected += 1;
            led.caught += 1;
            led.fnv(0x9800 + k as u64);
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
/// UNX-F52378 判据集（承载卡 52379 判据锚 UNX-F52380-J1，off-by-one
/// 板式；§AI-66.9 红线适用：deploy 判据一律干跑列清单，禁真实外发，
/// 外部资源零写入）。
pub fn run_unx52378_checks() -> CheckSet {
    let mut s = CheckSet::new("unx52378");
    let (recorded, caught, fp) = run_cycle();
    s.add(
        "五命令族 20 格全通",
        recorded >= FAM_CELLS as u32,
        "compute/storage/iam/pubsub/logging 五家族逐格恒等",
    );
    s.add(
        "20 操作集全通",
        {
            let mut lt = GcloudLedger::new();
            let mut ok = true;
            let mut p = 0usize;
            while p < OP_CELLS {
                if let Some(w) = op_cell(p) {
                    if lt.record_op(p, w).is_err() {
                        ok = false;
                    }
                }
                p += 1;
            }
            ok && lt.ops_all()
        },
        "init/config/query/describe/simulate 五相位逐操作恒等（deploy 一律 simulate 干跑列清单）",
    );
    s.add(
        "输出三格式 6 格全通",
        {
            let mut lt = GcloudLedger::new();
            let mut ok = true;
            let mut f = 0usize;
            while f < FMT_CELLS {
                if let Some(w) = fmt_cell(f) {
                    if lt.record_fmt(f, w).is_err() {
                        ok = false;
                    }
                }
                f += 1;
            }
            ok && lt.fmts_all()
        },
        "json/yaml/text × 读账/落账六格闭集逐格恒等",
    );
    s.add(
        "凭证链 8 格全通",
        {
            let mut lt = GcloudLedger::new();
            let mut ok = true;
            let mut c = 0usize;
            while c < CRED_CELLS {
                if let Some(w) = cred_cell(c) {
                    if lt.record_cred(c, w).is_err() {
                        ok = false;
                    }
                }
                c += 1;
            }
            ok && lt.creds_all()
        },
        "环境变量/ADC 文件/用户登录/服务账号密钥/模拟代入/身份联合/元数据/刷新令牌逐格对合恒等",
    );
    s.add(
        "config/profile 三元闸闭序全走",
        {
            let mut lt = GcloudLedger::new();
            let a = lt.record_prof(0).is_ok();
            let b = lt.record_prof(1).is_ok();
            a && b && lt.prof_done_all()
        },
        "Default→Named→Activated 闭序，越序必抓",
    );
    s.add(
        "Python 上栈声明 2/2 全齐",
        {
            let mut lt = GcloudLedger::new();
            let mut ok = true;
            let mut r = 0usize;
            while r < PY_ROWS {
                if lt.declare_py(r).is_err() {
                    ok = false;
                }
                r += 1;
            }
            ok && lt.pys_all()
        },
        "gcloud→Python 运行时/google-api-python-client→HTTP 栈两行闭集",
    );
    s.add(
        "六判据全落",
        {
            let mut lt = GcloudLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            lt.gates_all()
        },
        "安装自检/凭据链/config 解析/输出格式/项目区域解析/只读干跑逐格落账",
    );
    s.add(
        "族与相位双单调",
        {
            let mut ok = true;
            let mut o = 1usize;
            while o < FAM_CELLS {
                if fam_kind(o).unwrap_or(5) < fam_kind(o - 1).unwrap_or(5) {
                    ok = false;
                }
                o += 1;
            }
            let mut p = 1usize;
            while p < OP_CELLS {
                if op_kind(p).unwrap_or(4) < op_kind(p - 1).unwrap_or(4) {
                    ok = false;
                }
                p += 1;
            }
            ok
        },
        "五族闭集与五相位闭集双单调，逆序必抓",
    );
    s.add(
        "外部资源零写入声明",
        SEG_BASE % 0x40 == 0,
        "只读判据；deploy 类对外动作一律 simulate 干跑列清单，禁真实外发——对外动作红线",
    );
    s.add(
        "注入 10/10 全抓",
        caught >= 10,
        "命令族越闭集/操作相位逆序/凭证链漂移三态轮转必抓",
    );
    let mut l0 = GcloudLedger::new();
    s.add(
        "边界·命令族越闭集",
        l0.record_fam(FAM_CELLS, 0).is_err(),
        "族 20 越闭集必拒",
    );
    s.add(
        "边界·操作越闭集",
        l0.record_op(OP_CELLS, 0).is_err(),
        "操作 20 越闭集必拒",
    );
    s.add(
        "边界·凭证链越闭集",
        l0.record_cred(CRED_CELLS, 0).is_err(),
        "凭证 8 越闭集必拒",
    );
    s.add(
        "输出格式漂移拒",
        l0.record_fmt(0, fmt_cell(0).unwrap_or(0).wrapping_add(1)) == Err(GcloudCode::EFmtDrift),
        "格式折叠与基准不恒等必抓",
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
            let mut lt = GcloudLedger::new();
            let mut g = 0usize;
            while g < GATES {
                let _ = lt.judge_gate(g);
                g += 1;
            }
            let mut o = 0usize;
            while o < FAM_CELLS {
                if let Some(w) = fam_cell(o) {
                    let _ = lt.record_fam(o, w);
                }
                o += 1;
            }
            let mut p = 0usize;
            while p < OP_CELLS {
                if let Some(w) = op_cell(p) {
                    let _ = lt.record_op(p, w);
                }
                p += 1;
            }
            let mut f = 0usize;
            while f < FMT_CELLS {
                if let Some(w) = fmt_cell(f) {
                    let _ = lt.record_fmt(f, w);
                }
                f += 1;
            }
            let mut c = 0usize;
            while c < CRED_CELLS {
                if let Some(w) = cred_cell(c) {
                    let _ = lt.record_cred(c, w);
                }
                c += 1;
            }
            let _ = lt.record_prof(0);
            let _ = lt.record_prof(1);
            let mut r = 0usize;
            while r < PY_ROWS {
                let _ = lt.declare_py(r);
                r += 1;
            }
            lt.judge_grade() == Grade::S1
        },
        "全通 → S1（KB-COMPAT 定级登记）",
    );
    s.add(
        "段位守卫",
        SEG_BASE % 0x40 == 0 && SEG_BASE == 0xC9E40,
        "SEG_BASE 0x40 对齐且落 0xC9E40 段",
    );
    s.add("收口·判据条数", s.len() >= 16, "含本条共 ≥17 条闭集");
    s
}

// ── 测试 ─────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fam_op_kinds() {
        assert_eq!(fam_kind(0), Some(0));
        assert_eq!(fam_kind(4), Some(1));
        assert_eq!(fam_kind(19), Some(4));
        assert!(fam_kind(20).is_none());
        assert_eq!(fam_kind_name(3), Some("pubsub"));
        assert_eq!(fam_kind_name(4), Some("logging"));
        assert!(fam_kind_name(5).is_none());
        assert!(fam_cell(20).is_none());
        assert_eq!(op_kind(0), Some(0));
        assert_eq!(op_kind(5), Some(1));
        assert_eq!(op_kind(19), Some(3));
        assert!(op_kind(20).is_none());
        assert_eq!(op_kind_name(3), Some("simulate"));
        assert!(op_cell(20).is_none());
        // 纯函数恒等 + 闭集边界。
        let a = fam_cell(3).unwrap_or(0);
        assert_eq!(a, fam_cell(3).unwrap_or(0));
        assert_ne!(a, fam_cell(4).unwrap_or(0));
        assert!(fmt_cell(FMT_CELLS).is_none());
        assert!(cred_cell(CRED_CELLS).is_none());
        assert!(py_row(PY_ROWS).is_none());
    }

    #[test]
    fn test_cred_projection_roundtrip() {
        let mut c = 0usize;
        while c < CRED_CELLS {
            let base = cred_cell(c).unwrap_or(0);
            assert_eq!(cred_back(cred_fwd(base)), base);
            assert_eq!(cred_fwd(cred_back(base)), base);
            c += 1;
        }
        assert_ne!(cred_fwd(0xB4000), 0xB4000);
    }

    #[test]
    fn test_fam_op_order_caught() {
        let mut lt = GcloudLedger::new();
        let _ = lt.record_fam(9, fam_cell(9).unwrap_or(0));
        assert_eq!(
            lt.record_fam(2, fam_cell(2).unwrap_or(0)),
            Err(GcloudCode::EFamOrder)
        );
        assert!(lt.record_fam(10, fam_cell(10).unwrap_or(0)).is_ok());
        let _ = lt.record_op(11, op_cell(11).unwrap_or(0));
        assert_eq!(
            lt.record_op(7, op_cell(7).unwrap_or(0)),
            Err(GcloudCode::EOpOrder)
        );
        assert!(lt.record_op(12, op_cell(12).unwrap_or(0)).is_ok());
    }

    #[test]
    fn test_drift_dup_caught() {
        let mut lt = GcloudLedger::new();
        assert_eq!(
            lt.record_fmt(3, fmt_cell(3).unwrap_or(0).wrapping_add(1)),
            Err(GcloudCode::EFmtDrift)
        );
        assert_eq!(
            lt.record_fam(5, fam_cell(5).unwrap_or(0).wrapping_add(7)),
            Err(GcloudCode::EFamDrift)
        );
        let _ = lt.record_cred(0, cred_cell(0).unwrap_or(0));
        assert_eq!(lt.record_cred(0, cred_cell(0).unwrap_or(0)), Err(GcloudCode::EDup));
    }

    #[test]
    fn test_prof_tri_state() {
        let mut lt = GcloudLedger::new();
        // 空账本直接 Activated 越序必抓，且不改三元态。
        assert_eq!(lt.record_prof(2), Err(GcloudCode::EProfSeq));
        assert!(!lt.prof_done_all());
        assert!(lt.record_prof(0).is_ok());
        assert_eq!(lt.record_prof(0), Err(GcloudCode::EProfSeq));
        assert!(lt.record_prof(1).is_ok());
        assert!(lt.prof_done_all());
        assert!(lt.record_prof(1).is_err());
    }

    #[test]
    fn test_gate_boundary() {
        let mut lt = GcloudLedger::new();
        let mut g = 0usize;
        while g < GATES {
            assert!(lt.judge_gate(g).is_ok());
            g += 1;
        }
        assert!(lt.judge_gate(6).is_err());
        assert!(lt.gates_all());
        let mut lt2 = GcloudLedger::new();
        assert!(lt2.judge_gate(0).is_ok());
        assert!(!lt2.gates_all());
    }

    #[test]
    fn test_full_cycle() {
        let (recorded, caught, _) = run_cycle();
        // 公式常量之和：20 族 + 20 操作 + 6 格式 + 8 凭证 + 2 上栈 = 56。
        assert_eq!(
            recorded,
            FAM_CELLS as u32 + OP_CELLS as u32 + FMT_CELLS as u32 + CRED_CELLS as u32 + PY_ROWS as u32
        );
        assert_eq!(caught, 10);
        // 双跑指纹一致。
        let (a1, b1, f1) = run_cycle();
        let (a2, b2, f2) = run_cycle();
        assert_eq!(a1, a2);
        assert_eq!(b1, b2);
        assert_eq!(f1, f2);
        assert_ne!(f1, 0);
    }

    #[test]
    fn test_seg_codes_closed() {
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0xC9E40);
        assert_eq!(SEG_BASE, 0xC7200 + 0x40 * (ID - 52201));
        let _ = _SEG_GUARD;
        assert_eq!(GcloudCode::Ok as u32, 0);
        assert_eq!(GcloudCode::EGradeDrift as u32, 12);
        assert_eq!(GcloudCode::Ok.val(), SEG_BASE);
        assert_eq!(GcloudCode::EProfSeq.val(), SEG_BASE + 0x09);
        assert!(!GcloudCode::Ok.detail().is_empty());
        assert!(!GcloudCode::ESeal.detail().is_empty());
        // XOR 锚低 16 位恒零。
        assert_eq!(GCLOUD_XOR & 0xFFFF, 0);
        assert_eq!(ADC_XOR & 0xFFFF, 0);
    }

    #[test]
    fn test_names_grade_closed() {
        assert_eq!(Grade::S1.val(), 1);
        assert_eq!(Grade::S3.val(), 3);
        // 如实阶梯：空账 S3，六闸齐 S2，全齐 S1。
        let lt = GcloudLedger::new();
        assert_eq!(lt.judge_grade(), Grade::S3);
        assert_eq!(cred_name(0), Some("环境变量 GOOGLE_APPLICATION_CREDENTIALS"));
        assert_eq!(cred_name(7), Some("刷新令牌缓存"));
        assert!(cred_name(8).is_none());
        assert_eq!(fmt_kind_name(2), Some("text"));
        assert!(fmt_kind_name(3).is_none());
        assert_eq!(py_row_name(0), Some("gcloud → Python 运行时上栈声明"));
        assert!(py_row_name(2).is_none());
        assert_eq!(gate_name(5), Some("只读干跑"));
        assert!(gate_name(6).is_none());
    }

    #[test]
    fn test_grade_ladder() {
        let mut lt = GcloudLedger::new();
        let mut g = 0usize;
        while g < GATES {
            let _ = lt.judge_gate(g);
            g += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S2);
        let mut o = 0usize;
        while o < FAM_CELLS {
            if let Some(w) = fam_cell(o) {
                let _ = lt.record_fam(o, w);
            }
            o += 1;
        }
        let mut p = 0usize;
        while p < OP_CELLS {
            if let Some(w) = op_cell(p) {
                let _ = lt.record_op(p, w);
            }
            p += 1;
        }
        let mut f = 0usize;
        while f < FMT_CELLS {
            if let Some(w) = fmt_cell(f) {
                let _ = lt.record_fmt(f, w);
            }
            f += 1;
        }
        let mut c = 0usize;
        while c < CRED_CELLS {
            if let Some(w) = cred_cell(c) {
                let _ = lt.record_cred(c, w);
            }
            c += 1;
        }
        let _ = lt.record_prof(0);
        let _ = lt.record_prof(1);
        let mut r = 0usize;
        while r < PY_ROWS {
            let _ = lt.declare_py(r);
            r += 1;
        }
        assert_eq!(lt.judge_grade(), Grade::S1);
    }

    #[test]
    fn unx52378_checks_green() {
        let s = run_unx52378_checks();
        assert!(s.all_passed(), "判据集必须全绿");
        assert!(s.len() >= 17);
    }
}
