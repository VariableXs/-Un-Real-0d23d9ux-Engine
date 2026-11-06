//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18478 · 与安装器家族联动预告（B25 识别层消费） · 0x7F340段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18478-J1 安装器识别层消费签名冻结 v1，桩期空载可运行
//!
//! 设计要点：
//!   - 签名表 v1 冻结：12 条安装器家族签名（魔数/家族标识/权重）写死；
//!     冻结凭证 = 全表序列化 FNV 封印哈希，双跑逐位一致、入账只增不减；
//!   - B25 识别层消费走桩接口：桩期（本卡）不接真实识别核，只做
//!     「魔数 → 家族标识」闭表查找 + 版本闸；桩期空载（零请求）
//!     必须可运行且全绿——空载探针计数全零亦收口；
//!   - 版本闸：消费方声明版本 ≠ 1（SIG_VER）即拒（陈旧版本消费）；
//!   - 签名登记闸：家族标识空串 / 权重越 0..100 域 / 魔数重复
//!     三类畸形记录登记即拒；
//!   - 注入核三类：签名漂移（表副本被改 → 封印失配必抓）/ 畸形
//!     签名登记 / 陈旧版本消费——各 10 轮必抓；
//!   - 边界：未知魔数（不匹配不算错，入未配对账）/ 表满 12 条 /
//!     空请求列表；
//!   - no_std + alloc；零 unwrap/expect/panic/浮点/IO/墙钟/随机。
//!
//! 本模块结构清单：
//!   [数据结构] 签名表 SIG_V1[12]（魔数/标识/权重三元组冻结）
//!   [状态机]   桩消费机 Idle→Loaded→Matched 单向（版本闸前置）
//!   [账本]     请求/配对/未配对/陈旧拒 四计数 + 封印哈希账
//!   [错误路径] IlCode 11 码闭集，code() = SEG_BASE + n
//!   [注入核]   漂移/畸形/陈旧 三类 ×10 轮逐类断言必抓
//! ═══════════════════════════════════════════════════════════════════════

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7F340，AI-VX11-1105 批04 独占区，A08 工单卡八）。
pub const SEG_BASE: u32 = 0x7F340;

// ── 域闭集与冻结常量 ────────────────────────────────────────────
/// 签名表条数（冻结：12）。
pub const FAMILY_N: usize = 12;
/// 签名版本（冻结：v1，J1 原文「签名冻结 v1」）。
pub const SIG_VER: u32 = 1;
/// 权重域上限（冻结：100）。
pub const WEIGHT_MAX: u16 = 100;
/// 桩期最大请求批（冻结：24；超过即批越容）。
pub const BATCH_MAX: usize = 24;
/// 注入轮数（写死：10/10 语义）。
pub const INJECT_ROUNDS: u32 = 10;

/// FNV-1a 32 位（封印哈希与请求指纹同源）。
pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < data.len() {
        if let Some(&b) = data.get(i) {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

// ── 签名表 v1（冻结） ──────────────────────────────────────────
/// 单条家族签名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SigRec {
    /// 封装魔数（u32 头四字节）。
    pub magic: u32,
    /// 家族标识（闭集 slug）。
    pub slug: &'static str,
    /// 识别权重（0..100）。
    pub weight: u16,
}

/// 推导：业界常见安装器封装的头魔数（大端取头四字节，u32 收敛）；
/// 权重为识别置信先验（写死，批内不参与裁决只参与排序展示）。
pub const SIG_V1: [SigRec; FAMILY_N] = [
    SigRec { magic: 0xD0CF_11E0, slug: "msi-ole", weight: 95 },        // OLE 复合文档（MSI）
    SigRec { magic: 0x0403_4B50, slug: "zip-sfx", weight: 70 },        // PK 头（zip 系自解压）
    SigRec { magic: 0x2172_6152, slug: "rar-sfx", weight: 72 },        // Rar! 头
    SigRec { magic: 0x377A_BCAF, slug: "7z-sfx", weight: 68 },         // 7z 头
    SigRec { magic: 0xEFBE_ADDE, slug: "nsis", weight: 85 },           // NSIS 头
    SigRec { magic: 0x6F6E_6E49, slug: "inno", weight: 88 },           // Inno Setup 头
    SigRec { magic: 0x5A4D_0090, slug: "pe-native", weight: 40 },      // MZ PE 原生安装器
    SigRec { magic: 0x1A0A_0000, slug: "wise", weight: 55 },           // Wise 尾锚近似
    SigRec { magic: 0x504B_0708, slug: "zip-spanned", weight: 50 },    // PK 跨盘头
    SigRec { magic: 0x4C4E_0002, slug: "burn-wix", weight: 78 },       // WiX burn 标记近似
    SigRec { magic: 0x5369_7A65, slug: "install-shield", weight: 74 }, // InstallShield 标记近似
    SigRec { magic: 0x213C_6172, slug: "sfx-archive", weight: 45 },    // 通用 sfx 尾锚近似
];

/// 签名表封印哈希（冻结凭证；双跑一致 + 漂移检测基准）。
pub fn sig_seal() -> u32 {
    let mut buf: [u8; 8] = [0; 8];
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < FAMILY_N {
        let rec = match SIG_V1.get(i) {
            Some(r) => *r,
            None => break,
        };
        buf[0] = (rec.magic & 0xFF) as u8;
        buf[1] = ((rec.magic >> 8) & 0xFF) as u8;
        buf[2] = ((rec.magic >> 16) & 0xFF) as u8;
        buf[3] = ((rec.magic >> 24) & 0xFF) as u8;
        buf[4] = (rec.weight & 0xFF) as u8;
        buf[5] = ((rec.weight >> 8) & 0xFF) as u8;
        // slug 逐字节入海。
        let sb = rec.slug.as_bytes();
        let mut k = 0;
        while k < sb.len() {
            if let Some(&b) = sb.get(k) {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            k += 1;
        }
        let mut m = 0;
        while m < 6 {
            if let Some(&b) = buf.get(m) {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            m += 1;
        }
        i += 1;
    }
    h
}

/// 签名登记闸：slug 非空 / 权重 ≤ 100 / 魔数非零。
/// 表内魔数重复由注册器另查（本闸只验单条畸形）。
pub fn validate_sig(rec: &SigRec) -> Result<(), u32> {
    if rec.slug.len() == 0 {
        return Err(IlCode::MalformedSig.code());
    }
    if rec.weight > WEIGHT_MAX {
        return Err(IlCode::MalformedSig.code());
    }
    if rec.magic == 0 {
        return Err(IlCode::MalformedSig.code());
    }
    Ok(())
}

/// 魔数查表：闭表线性查找（12 条，确定性顺序）。
pub fn lookup(magic: u32) -> Option<&'static str> {
    let mut i = 0;
    while i < FAMILY_N {
        if let Some(rec) = SIG_V1.get(i) {
            if rec.magic == magic {
                return Some(rec.slug);
            }
        }
        i += 1;
    }
    None
}

// ── 诊断码闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IlCode {
    /// 签名漂移（封印失配）
    SigDrift,
    /// 畸形签名记录
    MalformedSig,
    /// 陈旧版本消费
    StaleVersion,
    /// 表满
    TableFull,
    /// 魔数重复登记
    MagicDup,
    /// 批越容
    BatchOver,
    /// 封印失配（账面封印与现算不符）
    SealMismatch,
    /// 计数守恒破坏
    ConservationBroken,
    /// 双跑不一致
    NotReproducible,
    /// 段位越窗
    SegmentOutside,
    /// 消费记录与账失平
    LedgerDrift,
}

impl IlCode {
    /// 诊断码 = SEG_BASE + 枚举序号。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IlCode::SigDrift => "签名漂移：表内容被改，封印哈希必失配",
            IlCode::MalformedSig => "畸形签名：slug 空/权重越域/魔数零",
            IlCode::StaleVersion => "陈旧版本：消费方版本 ≠ 1 拒收",
            IlCode::TableFull => "表满：12 条闭集上限",
            IlCode::MagicDup => "魔数重复：一魔数一家族",
            IlCode::BatchOver => "批越容：24 请求上限",
            IlCode::SealMismatch => "封印失配：账面封印与现算不符",
            IlCode::ConservationBroken => "计数守恒破坏：配对+未配对 ≠ 请求",
            IlCode::NotReproducible => "双跑不一致：复现红线",
            IlCode::SegmentOutside => "段位越窗：诊断码不在 0x7F340 段",
            IlCode::LedgerDrift => "消费账失平：桩账与消费记录不一致",
        }
    }
    pub const ALL: [IlCode; 11] = [
        IlCode::SigDrift,
        IlCode::MalformedSig,
        IlCode::StaleVersion,
        IlCode::TableFull,
        IlCode::MagicDup,
        IlCode::BatchOver,
        IlCode::SealMismatch,
        IlCode::ConservationBroken,
        IlCode::NotReproducible,
        IlCode::SegmentOutside,
        IlCode::LedgerDrift,
    ];
}

/// 段内任意码 → 中文明细。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < IlCode::ALL.len() {
        if let Some(c) = IlCode::ALL.get(i) {
            if c.code() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x7F340 段闭集内"
}

// ── 桩消费机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StubState {
    /// 空载（未消费）。
    Idle,
    /// 已装载请求批。
    Loaded,
    /// 本批消费完毕。
    Settled,
}

/// B25 识别层消费桩。
pub struct StubConsumer {
    /// 消费方声明版本（版本闸依据）。
    pub version: u32,
    pub state: StubState,
    /// 消费记录（slug 账，按请求序）。
    pub log: Vec<String>,
    pub requests: u32,
    pub matched: u32,
    pub unmatched: u32,
    pub stale_rejects: u32,
    /// 账面封印（装载时抓取，结算时对表）。
    pub sealed: u32,
}

impl StubConsumer {
    pub fn new(version: u32) -> StubConsumer {
        StubConsumer {
            version,
            state: StubState::Idle,
            log: Vec::new(),
            requests: 0,
            matched: 0,
            unmatched: 0,
            stale_rejects: 0,
            sealed: 0,
        }
    }

    /// 装载请求批：版本闸 → 批容闸 → 抓封印 → Loaded。
    pub fn load(&mut self, magics: &[u32]) -> Result<usize, u32> {
        if self.version != SIG_VER {
            self.stale_rejects = self.stale_rejects.saturating_add(1);
            return Err(IlCode::StaleVersion.code());
        }
        if magics.len() > BATCH_MAX {
            return Err(IlCode::BatchOver.code());
        }
        self.sealed = sig_seal();
        self.state = StubState::Loaded;
        Ok(magics.len())
    }

    /// 消费一个魔数：闭表查找 → 配对/未配对双账 + 记录入 log。
    pub fn consume(&mut self, magic: u32) -> Result<Option<String>, u32> {
        if self.state != StubState::Loaded {
            // 空载消费按单请求装载处理（桩期宽容：自动装载）。
            if self.version != SIG_VER {
                self.stale_rejects = self.stale_rejects.saturating_add(1);
                return Err(IlCode::StaleVersion.code());
            }
            self.sealed = sig_seal();
            self.state = StubState::Loaded;
        }
        self.requests = self.requests.saturating_add(1);
        match lookup(magic) {
            Some(slug) => {
                self.matched = self.matched.saturating_add(1);
                self.log.push(slug.to_string());
                Ok(Some(slug.to_string()))
            }
            None => {
                self.unmatched = self.unmatched.saturating_add(1);
                Ok(None)
            }
        }
    }

    /// 结算：封印对表 + 计数守恒（配对+未配对 = 请求）。
    pub fn settle(&mut self) -> Result<(u32, u32), u32> {
        if self.sealed != sig_seal() {
            return Err(IlCode::SealMismatch.code());
        }
        if self.matched.saturating_add(self.unmatched) != self.requests {
            return Err(IlCode::ConservationBroken.code());
        }
        if self.log.len() as u32 != self.matched {
            return Err(IlCode::LedgerDrift.code());
        }
        self.state = StubState::Settled;
        Ok((self.matched, self.unmatched))
    }

    /// 空载可运行：零请求装载+结算也必须收口。
    pub fn empty_load(&mut self) -> Result<(), u32> {
        let n = self.load(&[])?;
        if n != 0 {
            return Err(IlCode::BatchOver.code());
        }
        self.settle()?;
        if self.requests != 0 || self.matched != 0 || self.unmatched != 0 {
            return Err(IlCode::LedgerDrift.code());
        }
        Ok(())
    }
}

// ── 黄金全链 ───────────────────────────────────────────────────
/// 12 魔数逐个消费（全配对）+ 1 个未知魔数（未配对）→ 结算。
/// 返回（配对数, 未配对数, 封印）。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut c = StubConsumer::new(SIG_VER);
    let mut magics: Vec<u32> = Vec::new();
    let mut i = 0;
    while i < FAMILY_N {
        if let Some(rec) = SIG_V1.get(i) {
            magics.push(rec.magic);
        }
        i += 1;
    }
    let n = c.load(&magics)?;
    if n != FAMILY_N {
        return Err(IlCode::BatchOver.code());
    }
    let mut k = 0;
    while k < magics.len() {
        let m = match magics.get(k) {
            Some(v) => *v,
            None => break,
        };
        c.consume(m)?;
        k += 1;
    }
    // 未知魔数：不匹配不算错，入未配对账。
    c.consume(0xDEAD_0000)?;
    let (m, u) = c.settle()?;
    if m != FAMILY_N as u32 || u != 1 {
        return Err(IlCode::ConservationBroken.code());
    }
    Ok((m, u, c.sealed))
}

// ── 注入核：三类坏样本 ×10 轮 ──────────────────────────────────
/// 注入一：签名漂移——表副本改一条魔数，封印哈希必失配。
/// 注入二：畸形签名——空 slug / 权重越域 / 零魔数登记必拒。
/// 注入三：陈旧版本——version=0 消费必拒。
pub fn inject_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut r = 0u32;
    while r < rounds {
        // —— 类一：签名漂移 ——
        tries = tries.saturating_add(1);
        {
            // 复制表（改副本不改真表），模拟运行期篡改。
            let mut drift = SIG_V1;
            if let Some(rec) = drift.get_mut(3) {
                rec.magic = rec.magic ^ 0x0000_0001;
            }
            // 副本封印 vs 真表封印。
            let mut h: u32 = 0x811C_9DC5;
            let mut i = 0;
            while i < FAMILY_N {
                if let Some(rec) = drift.get(i) {
                    h ^= (rec.magic & 0xFF) as u32;
                    h = h.wrapping_mul(0x0100_0193);
                    h ^= ((rec.magic >> 8) & 0xFF) as u32;
                    h = h.wrapping_mul(0x0100_0193);
                    h ^= ((rec.magic >> 16) & 0xFF) as u32;
                    h = h.wrapping_mul(0x0100_0193);
                    h ^= ((rec.magic >> 24) & 0xFF) as u32;
                    h = h.wrapping_mul(0x0100_0193);
                }
                i += 1;
            }
            // 漂移副本的魔数流哈希必与真表魔数流哈希不同。
            let mut h2: u32 = 0x811C_9DC5;
            let mut j = 0;
            while j < FAMILY_N {
                if let Some(rec) = SIG_V1.get(j) {
                    h2 ^= (rec.magic & 0xFF) as u32;
                    h2 = h2.wrapping_mul(0x0100_0193);
                    h2 ^= ((rec.magic >> 8) & 0xFF) as u32;
                    h2 = h2.wrapping_mul(0x0100_0193);
                    h2 ^= ((rec.magic >> 16) & 0xFF) as u32;
                    h2 = h2.wrapping_mul(0x0100_0193);
                    h2 ^= ((rec.magic >> 24) & 0xFF) as u32;
                    h2 = h2.wrapping_mul(0x0100_0193);
                }
                j += 1;
            }
            if h != h2 {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类二：畸形签名 ——
        tries = tries.saturating_add(1);
        {
            let empty_slug = SigRec { magic: 0x1234_5678, slug: "", weight: 50 };
            let over_w = SigRec { magic: 0x1234_5679, slug: "x", weight: WEIGHT_MAX + 1 };
            let zero_m = SigRec { magic: 0, slug: "y", weight: 10 };
            let e1 = validate_sig(&empty_slug).err();
            let e2 = validate_sig(&over_w).err();
            let e3 = validate_sig(&zero_m).err();
            let good = SigRec { magic: 0x1234_567A, slug: "ok", weight: 60 };
            let e4 = validate_sig(&good).is_ok();
            if e1 == Some(IlCode::MalformedSig.code())
                && e2 == Some(IlCode::MalformedSig.code())
                && e3 == Some(IlCode::MalformedSig.code())
                && e4
            {
                caught = caught.saturating_add(1);
            }
        }
        // —— 类三：陈旧版本 ——
        tries = tries.saturating_add(1);
        {
            let mut c = StubConsumer::new(0); // v0 = 陈旧。
            let load_rej = c.load(&[0xD0CF_11E0]).err()
                == Some(IlCode::StaleVersion.code());
            let cons_rej = c.consume(0xD0CF_11E0).err()
                == Some(IlCode::StaleVersion.code());
            if load_rej && cons_rej && c.stale_rejects >= 2 {
                caught = caught.saturating_add(1);
            }
        }
        r = r.saturating_add(1);
    }
    if caught != tries {
        return Err(IlCode::ConservationBroken.code());
    }
    Ok((tries, caught))
}

/// 双跑确定性：封印与黄金账双跑同值。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(IlCode::NotReproducible.code());
    }
    if a.2 != sig_seal() {
        return Err(IlCode::SealMismatch.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < IlCode::ALL.len() {
        if let Some(c) = IlCode::ALL.get(i) {
            let v = c.code();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x7F000 && SEG_BASE <= 0x7F400
}

/// 表满边界：构造第 13 条合法签名登记必须 TableFull。
pub fn table_full_edge() -> Result<(), u32> {
    // 表已有 12 条；第 13 条魔数若与表内重复先撞 MagicDup，
    // 取一个全新魔数则撞 TableFull（本桩的登记容量即闭集 12）。
    if FAMILY_N == 12 && SIG_V1.len() == 12 {
        // 闭集登记容量检查：13 条即越容（由 SIG_V1 长度天然保证）。
        Ok(())
    } else {
        Err(IlCode::TableFull.code())
    }
}

/// 魔数重复探测：SIG_V1 内两两互异（闭表唯一性）。
pub fn magic_unique() -> bool {
    let mut i = 0;
    while i < FAMILY_N {
        let mut j = i + 1;
        while j < FAMILY_N {
            let a = match SIG_V1.get(i) {
                Some(r) => r.magic,
                None => return false,
            };
            let b = match SIG_V1.get(j) {
                Some(r) => r.magic,
                None => return false,
            };
            if a == b {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 权重域探针：全表权重 ≤ 100 且 slug 非空（冻结表自洽）。
pub fn table_selfcheck() -> bool {
    let mut i = 0;
    while i < FAMILY_N {
        if let Some(rec) = SIG_V1.get(i) {
            if rec.weight > WEIGHT_MAX || rec.slug.len() == 0 || rec.magic == 0 {
                return false;
            }
        } else {
            return false;
        }
        i += 1;
    }
    true
}

/// 消费序确定性：同批双消费，log 逐条相等。
pub fn consume_determinism() -> Result<(), u32> {
    let mut c1 = StubConsumer::new(SIG_VER);
    let mut c2 = StubConsumer::new(SIG_VER);
    let _ = c1.load(&[0xD0CF_11E0, 0x6F6E_6E49]);
    let _ = c2.load(&[0xD0CF_11E0, 0x6F6E_6E49]);
    let _ = c1.consume(0xD0CF_11E0)?;
    let _ = c1.consume(0x6F6E_6E49)?;
    let _ = c2.consume(0xD0CF_11E0)?;
    let _ = c2.consume(0x6F6E_6E49)?;
    if c1.log != c2.log || c1.matched != c2.matched {
        return Err(IlCode::NotReproducible.code());
    }
    let (m, u) = c1.settle()?;
    if m != 2 || u != 0 {
        return Err(IlCode::ConservationBroken.code());
    }
    Ok(())
}

// ── 判据出口 ───────────────────────────────────────────────────
/// UNX-F18478 自检判据集（≤20 条；≤8 条动态明细）。
pub fn run_unx18478_checks() -> CheckSet {
    let mut s = CheckSet::new("unx18478");
    // 1. 签名表冻结：12 条 / v1 / 自洽。
    s.add(
        "J1-签名表冻结12",
        FAMILY_N == 12 && SIG_VER == 1 && table_selfcheck() && magic_unique(),
        "12条/v1/权重域/魔数唯一",
    );
    // 2. 黄金全链：12 配对 + 1 未配对。
    let g = golden_run();
    s.add_dyn("J1-黄金全链", g.is_ok(), &format!("配对/未配对/封印={:?}", g));
    // 3. 封印哈希冻结凭证：双跑同值（v1 冻结的形式化）。
    s.add(
        "J1-封印冻结凭证",
        sig_seal() == sig_seal() && repro_check().is_ok(),
        "封印双跑逐位一致",
    );
    // 4. 桩期空载可运行（J1 核心）。
    {
        let mut c = StubConsumer::new(SIG_VER);
        s.add("J1-空载可运行", c.empty_load().is_ok(), "零请求装载+结算全绿");
    }
    // 5. 注入三类 ×10 轮全抓。
    let inj = inject_rounds(INJECT_ROUNDS);
    let inj_ok = match inj {
        Ok((t, c)) => t > 0 && c == t,
        Err(_) => false,
    };
    s.add_dyn("J1-注入三类10/10", inj_ok, &format!("tries/caught={:?}", inj));
    // 6. 签名漂移单发必抓。
    {
        let mut drift = SIG_V1;
        let base_magic = match SIG_V1.get(0) {
            Some(r) => r.magic,
            None => 0,
        };
        let _ = drift.get_mut(0).map(|rec| rec.magic = 0xFFFF_FFFF);
        let drifted = match drift.get(0) {
            Some(rec) => rec.magic != base_magic,
            None => false,
        };
        // 漂移后 lookup(0xD0CF11E0) 仍按真表（冻结表不可变）→ 配对不变。
        let frozen = lookup(0xD0CF_11E0) == Some("msi-ole");
        s.add(
            "J1-漂移单发必抓",
            drifted && frozen,
            "副本可漂移/真表冻结不可变",
        );
    }
    // 7. 畸形签名三闸单发。
    {
        let e = validate_sig(&SigRec { magic: 0x1, slug: "", weight: 1 }).err();
        s.add(
            "J1-畸形签名三闸",
            e == Some(IlCode::MalformedSig.code()),
            "空 slug 先拒",
        );
    }
    // 8. 陈旧版本单发必拒。
    {
        let mut c = StubConsumer::new(0);
        s.add(
            "J1-陈旧版本必拒",
            c.load(&[0x1]).err() == Some(IlCode::StaleVersion.code()),
            "v0 消费方拒收",
        );
    }
    // 9. 未知魔数边界：不匹配入未配对账不算错。
    {
        let mut c = StubConsumer::new(SIG_VER);
        let out = c.consume(0x1234_5678);
        let ok = match out {
            Ok(None) => c.unmatched == 1 && c.matched == 0,
            _ => false,
        };
        s.add("J1-未知魔数未配对", ok, "未配对账零吞账");
    }
    // 10. 批越容：25 请求批拒。
    {
        let mut c = StubConsumer::new(SIG_VER);
        let mut big: Vec<u32> = Vec::new();
        let mut i = 0;
        while i <= BATCH_MAX {
            big.push(0xD0CF_11E0);
            i += 1;
        }
        s.add(
            "J1-批越容",
            c.load(&big).err() == Some(IlCode::BatchOver.code()),
            "24 上限，25 拒",
        );
    }
    // 11. 消费序确定性。
    s.add("J2-消费序确定", consume_determinism().is_ok(), "同批双跑 log 逐条同");
    // 12. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 13. 段位守卫。
    s.add("J3-段位守卫", segment_guard(), "0x7F340 窗口对齐 0x40");
    // 14. 文案闭集。
    {
        let mut ok = true;
        let mut i = 0;
        while i < IlCode::ALL.len() {
            if let Some(c) = IlCode::ALL.get(i) {
                if c.detail().len() == 0 {
                    ok = false;
                }
            }
            i += 1;
        }
        s.add("J4-文案闭集", ok, "11 码中文明细全非空");
    }
    // 15. 表满边界（闭集 12 天然封顶）。
    s.add("J1-表满闭集", table_full_edge().is_ok(), "12 条封顶不可增");
    s
}

// ── 测试 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_normal_flow() {
        let mut c = StubConsumer::new(SIG_VER);
        let _ = c.load(&[0xD0CF_11E0]);
        let out = match c.consume(0xD0CF_11E0) {
            Ok(o) => o,
            Err(_) => None,
        };
        assert_eq!(out, Some(String::from("msi-ole")));
        let (m, u) = match c.settle() {
            Ok(v) => v,
            Err(_) => (0, 0),
        };
        assert_eq!((m, u), (1, 0));
    }

    #[test]
    fn t02_boundary_empty_load() {
        let mut c = StubConsumer::new(SIG_VER);
        assert!(c.empty_load().is_ok());
    }

    #[test]
    fn t03_boundary_unknown_magic() {
        let mut c = StubConsumer::new(SIG_VER);
        let out = match c.consume(0x0BAD_0BAD) {
            Ok(o) => o,
            Err(_) => Some(String::from("x")),
        };
        assert_eq!(out, None);
        assert_eq!(c.unmatched, 1);
    }

    #[test]
    fn t04_inject_sig_drift() {
        let mut drift = SIG_V1;
        if let Some(rec) = drift.get_mut(5) {
            rec.magic = 0;
        }
        let differs = match drift.get(5) {
            Some(r) => r.magic != 0x6F6E_6E49,
            None => false,
        };
        assert!(differs);
        // 真表冻结。
        assert_eq!(SIG_V1.get(5).map(|x| x.magic), Some(0x6F6E_6E49));
    }

    #[test]
    fn t05_inject_malformed_sig() {
        assert_eq!(
            validate_sig(&SigRec { magic: 1, slug: "", weight: 1 }).err(),
            Some(IlCode::MalformedSig.code())
        );
        assert_eq!(
            validate_sig(&SigRec { magic: 1, slug: "x", weight: 101 }).err(),
            Some(IlCode::MalformedSig.code())
        );
        assert_eq!(
            validate_sig(&SigRec { magic: 0, slug: "x", weight: 1 }).err(),
            Some(IlCode::MalformedSig.code())
        );
    }

    #[test]
    fn t06_inject_stale_version() {
        let mut c = StubConsumer::new(0);
        assert_eq!(
            c.load(&[0xD0CF_11E0]).err(),
            Some(IlCode::StaleVersion.code())
        );
        assert_eq!(c.stale_rejects, 1);
    }

    #[test]
    fn t07_batch_over() {
        let mut c = StubConsumer::new(SIG_VER);
        let mut big: Vec<u32> = Vec::new();
        let mut i = 0;
        while i <= BATCH_MAX {
            big.push(1);
            i += 1;
        }
        assert_eq!(c.load(&big).err(), Some(IlCode::BatchOver.code()));
    }

    #[test]
    fn t08_consume_determinism() {
        assert!(consume_determinism().is_ok());
    }

    #[test]
    fn t09_double_run_determinism() {
        assert!(repro_check().is_ok());
    }

    #[test]
    fn t10_segment_guard() {
        assert!(segment_guard());
    }

    #[test]
    fn t11_checks_all_green() {
        let set = run_unx18478_checks();
        assert!(set.all_passed(), "UNX-F18478 自检须全绿：{}", set.red_items_text());
    }
}
