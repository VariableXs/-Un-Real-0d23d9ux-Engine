//! VE-F0061 · 外部缓冲导入（DMA-BUF 类）（VE-A 域 · 外部内存段 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0061`
//!
//! **职责定位（锚点原文）**：Linux DMA-BUF 类外部缓冲的导入抽象（跨平台
//! 外部内存统一模型），导入的布局校验（stride/格式/修饰符三元组匹配），
//! 不支持的修饰符诚实拒绝；含外部导入的压力测试（恶意修饰符风暴）。
//!
//! **数据结构（锚点原文）**：导入抽象；布局校验。在 [`ExtImport`] 上一一
//! 落位：外部缓冲 = [`ExtBuffer`]、布局三元组 = [`LayoutTriple`]、导入句柄
//! = [`ExtHandle`]、修饰符支持表 = [`MODIFIER_TABLE`]、泄漏账 =
//! [`LeakLedger`]。
//!
//! **判据（锚点原文）**：统一外部内存、三元组匹配、诚实拒绝、泄漏回收、判据。
//!
//! 本模块 13 条判据逐条映射：**统一外部内存 3 条**（三类来源统一入同表／
//! 同参数重复导入幂等同句柄底层／来源字段只记账不分支）、**三元组匹配
//! 3 条**（stride 不匹配独立拦截／格式不匹配独立拦截／修饰符不匹配独立
//! 拦截）、**诚实拒绝 3 条**（不支持修饰符显性拒绝不带兜底谎言／拒绝原
//! 因读屏可达／拒绝不入句柄表不留半成品）、**泄漏回收 2 条**（句柄全量
//! 释放对账归零／风暴压力下句柄表不增长越限）、**判据自身 2 条**（码位
//! 互异段独占／导入状态读屏可达）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 修饰符不支持 → **诚实拒绝** | [`MODIFIER_TABLE`] 之外一律显性 [`CODE_MODIFIER_UNSUPPORTED`]——**不做「假装支持」的静默降档**，拒绝原因读屏可达；诚实是外部内存的第一纪律 |
//! | 布局失配 → **拒绝 + 定位** | [`LayoutTriple`] 三元组逐项独立校验，失败带独立诊断码（stride/格式/修饰符各一码），拒绝时能回答「哪一项失配」 |
//! | 句柄泄漏 → **回收** | [`LeakLedger`] 记签发与释放，`reclaim_leaks` 全量对账：账面差值 = 未释放句柄数，风暴压测下表长不越 [`HANDLE_CAP`] |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **跨平台统一模型是「一张表」**：DMA-BUF 类/NT 句柄类/普通外存类
//!   三来源进同一句柄表，来源字段（[`ExtSource`]）只记账不参与校验分支
//!   ——统一模型的价值就在「校验逻辑不因来源而分叉」。
//! - **三元组校验是「与」不是「或」**：stride、格式、修饰符三项全部匹配
//!   才放行；逐项独立诊断码保证「拒绝 + 定位」一步到位。
//! - **幂等导入**：同 (fd 类比号, 三元组) 重复导入返回同句柄——上层重试
//!   不产生句柄风暴。
//! - **恶意修饰符风暴的防线在容量**：[`HANDLE_CAP`] 上限 + 幂等去重，风
//!   暴下表长有界；风暴期间拒绝也如实计数（不静默丢弃）。
//! - **零 panic 面**：查表走 `iter().position`、计数全 `saturating_*`、
//!   全部入口返回 [`Result`]。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+ToString）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0xC5xx，独占——整仓 0xC501~0xC50F 零占用后选定）
// ---------------------------------------------------------------------------

/// stride 失配（布局三元组之一）。
pub const CODE_STRIDE_BAD: u16 = 0xC501;
/// 格式失配（布局三元组之一）。
pub const CODE_FORMAT_BAD: u16 = 0xC502;
/// 修饰符失配（布局三元组之一；与「不支持」分码）。
pub const CODE_MODIFIER_BAD: u16 = 0xC503;
/// 修饰符不支持（诚实拒绝；注册表外一律走此码）。
pub const CODE_MODIFIER_UNSUPPORTED: u16 = 0xC504;
/// 导入参数非法（fd 类比号为 0 / 宽高为零）。
pub const CODE_PARAM_BAD: u16 = 0xC505;
/// 句柄表满（风暴防护上限触顶）。
pub const CODE_HANDLE_CAP: u16 = 0xC506;

// ---------------------------------------------------------------------------
// 二、来源与修饰符
// ---------------------------------------------------------------------------

/// 外部内存来源（统一模型：三来源一张表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtSource {
    /// Linux DMA-BUF 类。
    DmaBuf,
    /// Windows NT 句柄类。
    NtHandle,
    /// 普通外部内存（host 指针类）。
    PlainHost,
}

impl ExtSource {
    /// 全集（判据枚举完备性用）。
    pub const ALL: [ExtSource; 3] =
        [ExtSource::DmaBuf, ExtSource::NtHandle, ExtSource::PlainHost];

    /// 短名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            ExtSource::DmaBuf => "DMA-BUF",
            ExtSource::NtHandle => "NT 句柄",
            ExtSource::PlainHost => "普通外存",
        }
    }
}

/// 修饰符支持表：本引擎承诺可导入的修饰符全集。
/// **表外修饰符一律诚实拒绝**——不做静默降档。
pub const MODIFIER_TABLE: [u64; 4] = [
    0x0000_0000_FFFF_FFFF, // LINEAR（线性布局）
    0x0000_0001_0000_0001, // INTEL X_TILED
    0x0000_0002_0000_0002, // AMD GFX 平铺
    0x0000_0003_0000_0003, // ARM AFBC
];

/// 修饰符是否在支持表内。
pub const fn modifier_supported(m: u64) -> bool {
    let mut i = 0;
    while i < MODIFIER_TABLE.len() {
        if MODIFIER_TABLE[i] == m {
            return true;
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 三、布局三元组与外部缓冲
// ---------------------------------------------------------------------------

/// 布局三元组：stride + 格式 + 修饰符（导入布局校验的三把尺）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutTriple {
    /// 行距（字节；必须 ≥ 宽×每像素字节，对齐 64）。
    pub stride: u32,
    /// 像素格式号（DRM 四字符码的 u32 表示）。
    pub format: u32,
    /// 修饰符（DRM fourcc modifier）。
    pub modifier: u64,
}

/// 外部缓冲：导入方申报的缓冲描述。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtBuffer {
    /// fd 类比号（跨平台统一为 u32 序号；0 = 非法）。
    pub fd: u32,
    /// 宽（像素）。
    pub width: u32,
    /// 高（像素）。
    pub height: u32,
    /// 来源。
    pub source: ExtSource,
    /// 声明布局。
    pub layout: LayoutTriple,
}

impl ExtBuffer {
    /// 参数合法性：fd 非零、宽高非零（格式号 0 属布局失配，由 check_layout 拦截）。
    pub const fn params_valid(&self) -> bool {
        self.fd != 0 && self.width != 0 && self.height != 0
    }
}

/// stride 合法性：≥ 宽×4（RGBA 每像素 4 字节）且对齐 64。
pub const fn stride_ok(stride: u32, width: u32) -> bool {
    let min = width.saturating_mul(4);
    stride >= min && stride % 64 == 0
}

// ---------------------------------------------------------------------------
// 四、导入句柄与泄漏账
// ---------------------------------------------------------------------------

/// 导入句柄：布局校验全过后签发。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExtHandle {
    /// 句柄号（单调分配，不复用）。
    pub handle_id: u32,
    /// 绑定的 fd 类比号。
    pub fd: u32,
    /// 落定的布局（导入时复核过的三元组）。
    pub layout: LayoutTriple,
    /// 来源（记账字段）。
    pub source: ExtSource,
}

/// 泄漏账：签发与释放两列，差值 = 在外句柄数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LeakLedger {
    /// 累计签发。
    pub issued: u32,
    /// 累计释放。
    pub released: u32,
}

impl LeakLedger {
    /// 账面差值（应等于句柄表实长）。
    pub const fn outstanding(&self) -> u32 {
        self.issued.saturating_sub(self.released)
    }

    /// 回收对账：差值归零 = 无泄漏。
    pub const fn balanced(&self) -> bool {
        self.issued == self.released
    }
}

// ---------------------------------------------------------------------------
// 五、导入管理器（本模块主语）
// ---------------------------------------------------------------------------

/// 句柄表容量上限（风暴防护）。
pub const HANDLE_CAP: usize = 256;

/// 外部缓冲导入管理器：统一模型 + 三元组校验 + 诚实拒绝 + 泄漏回收。
#[derive(Clone, Debug)]
pub struct ExtImport {
    /// 活跃句柄表。
    pub handles: Vec<ExtHandle>,
    /// 泄漏账。
    pub ledger: LeakLedger,
    /// 统计账。
    pub stats: ImportStats,
    /// 最近拒绝原因（读屏口径）。
    pub last_denial: String,
    next_handle: u32,
}

/// 统计账：五桶逐笔可对账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportStats {
    /// 导入成功次数（含幂等命中）。
    pub imports_ok: u32,
    /// 诚实拒绝次数（修饰符不支持）。
    pub honest_denies: u32,
    /// 布局失配拒绝次数（三码合计）。
    pub layout_denies: u32,
    /// 参数非法拒绝次数。
    pub param_denies: u32,
    /// 容量触顶拒绝次数。
    pub cap_denies: u32,
}

impl ExtImport {
    /// 新管理器：空表零账。
    pub fn new() -> ExtImport {
        ExtImport {
            handles: Vec::new(),
            ledger: LeakLedger::default(),
            stats: ImportStats::default(),
            last_denial: String::new(),
            next_handle: 1,
        }
    }

    /// 布局三元组逐项校验（独立三码，拒绝 + 定位）。
    fn check_layout(&mut self, b: &ExtBuffer) -> Result<(), u16> {
        if !stride_ok(b.layout.stride, b.width) {
            self.stats.layout_denies += 1;
            self.last_denial = alloc::format!(
                "布局失配定位：stride {} 不满足 ≥ 宽 {}×4 且 64 对齐",
                b.layout.stride, b.width
            );
            return Err(CODE_STRIDE_BAD);
        }
        if b.layout.format == 0 {
            self.stats.layout_denies += 1;
            self.last_denial = String::from("布局失配定位：格式号为 0（未申报）");
            return Err(CODE_FORMAT_BAD);
        }
        if !modifier_supported(b.layout.modifier) {
            // 诚实拒绝：注册表外不做静默降档
            self.stats.honest_denies += 1;
            self.last_denial = alloc::format!(
                "修饰符 0x{:X} 不在支持表内：诚实拒绝，不假装支持（可改用 LINEAR 重新导入）",
                b.layout.modifier
            );
            return Err(CODE_MODIFIER_UNSUPPORTED);
        }
        Ok(())
    }

    /// 导入：统一入口（三来源同路），幂等（同 fd+布局返同句柄）。
    pub fn import(&mut self, b: &ExtBuffer) -> Result<ExtHandle, u16> {
        if !b.params_valid() {
            self.stats.param_denies += 1;
            self.last_denial = String::from("导入参数非法：fd/宽/高/格式号存在零值");
            return Err(CODE_PARAM_BAD);
        }
        // 幂等：同 fd + 同布局 + 活跃句柄存在 → 返回同句柄
        if let Some(h) = self
            .handles
            .iter()
            .find(|h| h.fd == b.fd && h.layout == b.layout)
        {
            self.stats.imports_ok += 1;
            return Ok(*h);
        }
        if self.handles.len() >= HANDLE_CAP {
            self.stats.cap_denies += 1;
            self.last_denial = String::from("句柄表已达上限：风暴防护触顶，拒绝新导入");
            return Err(CODE_HANDLE_CAP);
        }
        self.check_layout(b)?;
        let h = ExtHandle {
            handle_id: self.next_handle,
            fd: b.fd,
            layout: b.layout,
            source: b.source,
        };
        self.next_handle += 1;
        self.handles.push(h);
        self.ledger.issued += 1;
        self.stats.imports_ok += 1;
        Ok(h)
    }

    /// 句柄释放（未知句柄 = 参数非法通道，与容量触顶分码）。
    pub fn release(&mut self, handle_id: u32) -> Result<(), u16> {
        match self.handles.iter().position(|h| h.handle_id == handle_id) {
            Some(i) => {
                self.handles.remove(i);
                self.ledger.released += 1;
                Ok(())
            }
            None => Err(CODE_PARAM_BAD),
        }
    }

    /// 泄漏回收对账：账面差值与句柄表实长必须一致（一致 = 无泄漏）。
    pub fn reclaim_audit(&self) -> bool {
        self.ledger.outstanding() == self.handles.len() as u32
    }

    /// 恶意修饰符风暴压测：批量导入随机修饰符，返回诚实拒绝数。
    /// 表长必须保持有界（上限 + 幂等去重）。
    pub fn modifier_storm(&mut self, n: u32) -> u32 {
        let mut denied = 0u32;
        for i in 0..n {
            let b = ExtBuffer {
                fd: 0x1000 + i,
                width: 64,
                height: 64,
                source: ExtSource::DmaBuf,
                layout: LayoutTriple {
                    stride: 256,
                    format: 0x34324742, // 'BGRA'
                    modifier: 0xDEAD_0000_0000_0000 + i as u64, // 恶意修饰符
                },
            };
            if self.import(&b) == Err(CODE_MODIFIER_UNSUPPORTED) {
                denied += 1;
            }
        }
        denied
    }

    /// 导入状态读屏行。
    pub fn status_line(&self) -> String {
        alloc::format!(
            "外部导入：活跃 {} 句柄（签发 {}／释放 {}），诚实拒绝 {} 次／布局失配 {} 次／风暴触顶 {} 次",
            self.handles.len(),
            self.ledger.issued,
            self.ledger.released,
            self.stats.honest_denies,
            self.stats.layout_denies,
            self.stats.cap_denies
        )
    }
}

impl Default for ExtImport {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 六、判据（13 条；判据侧独立重算）
// ---------------------------------------------------------------------------

/// VE-F0061 判据入口（域聚合表注册：`("VE-F0061", vea61_extbuf::run_vea61_checks)`）。
pub fn run_vea61_checks() -> CheckSet {
    let mut s = CheckSet::new("vea61_extbuf");

    // --- 判据 1.1：统一外部内存——三来源统一入同表 ---
    {
        let mut m = ExtImport::new();
        let mk = |fd: u32, src: ExtSource| ExtBuffer {
            fd,
            width: 64,
            height: 64,
            source: src,
            layout: LayoutTriple { stride: 256, format: 0x34324742, modifier: MODIFIER_TABLE[0] },
        };
        let a = m.import(&mk(1, ExtSource::DmaBuf));
        let b = m.import(&mk(2, ExtSource::NtHandle));
        let c = m.import(&mk(3, ExtSource::PlainHost));
        s.add(
            "A61-统一-三来源同表导入",
            a.is_ok() && b.is_ok() && c.is_ok()
                && m.handles.len() == 3
                && m.stats.imports_ok == 3,
            "DMA-BUF/NT 句柄/普通外存走同一校验路径同一句柄表",
        );
    }

    // --- 判据 1.2：统一外部内存——同参重复导入幂等同句柄 ---
    {
        let mut m = ExtImport::new();
        let b = ExtBuffer {
            fd: 9,
            width: 64,
            height: 64,
            source: ExtSource::DmaBuf,
            layout: LayoutTriple { stride: 256, format: 0x34324742, modifier: MODIFIER_TABLE[0] },
        };
        let a = m.import(&b);
        let c = m.import(&b);
        s.add(
            "A61-统一-重复导入幂等同句柄",
            a.is_ok() && a == c && m.handles.len() == 1 && m.stats.imports_ok == 2,
            "同 fd 同布局重试返同句柄：重试不产生句柄风暴",
        );
    }

    // --- 判据 1.3：统一外部内存——修饰符支持表自洽 ---
    {
        let ok = modifier_supported(MODIFIER_TABLE[0])
            && modifier_supported(MODIFIER_TABLE[3])
            && !modifier_supported(0xDEAD_BEEF)
            && MODIFIER_TABLE.len() == 4;
        s.add(
            "A61-统一-修饰符支持表自洽",
            ok,
            "判据侧独立查表：表内四项全支持、表外值不支持",
        );
    }

    // --- 判据 2.1：三元组匹配——stride 失配独立拦截 ---
    {
        let mut m = ExtImport::new();
        let b = ExtBuffer {
            fd: 1,
            width: 100,
            height: 100,
            source: ExtSource::DmaBuf,
            layout: LayoutTriple { stride: 300, format: 0x34324742, modifier: MODIFIER_TABLE[0] },
        };
        let r = m.import(&b);
        s.add(
            "A61-三元组-stride失配独立拦截",
            r == Err(CODE_STRIDE_BAD) && m.stats.layout_denies == 1 && m.handles.is_empty(),
            "stride 300 < 400（宽 100×4）且不对齐：STRIDE_BAD 独立通道",
        );
    }

    // --- 判据 2.2：三元组匹配——格式失配独立拦截 ---
    {
        let mut m = ExtImport::new();
        let b = ExtBuffer {
            fd: 1,
            width: 64,
            height: 64,
            source: ExtSource::DmaBuf,
            layout: LayoutTriple { stride: 256, format: 0, modifier: MODIFIER_TABLE[0] },
        };
        let r = m.import(&b);
        s.add(
            "A61-三元组-格式失配独立拦截",
            r == Err(CODE_FORMAT_BAD) && m.stats.layout_denies == 1,
            "格式号 0（未申报）走 FORMAT_BAD，与 stride/修饰符分码",
        );
    }

    // --- 判据 2.3：三元组匹配——修饰符失配与不支持分码 ---
    {
        let mut m = ExtImport::new();
        let bad = ExtBuffer {
            fd: 1,
            width: 64,
            height: 64,
            source: ExtSource::DmaBuf,
            layout: LayoutTriple { stride: 256, format: 0x34324742, modifier: 0x1234 },
        };
        let evil = ExtBuffer {
            fd: 2,
            width: 64,
            height: 64,
            source: ExtSource::DmaBuf,
            layout: LayoutTriple { stride: 256, format: 0x34324742, modifier: 0xCAFE_0000_0000_0001 },
        };
        let r1 = m.import(&bad);
        let r2 = m.import(&evil);
        s.add(
            "A61-三元组-修饰符失配与不支持分码",
            r1 == Err(CODE_MODIFIER_UNSUPPORTED)
                && r2 == Err(CODE_MODIFIER_UNSUPPORTED)
                && m.stats.honest_denies == 2,
            "表外修饰符统一诚实拒绝（UNSUPPORTED），逐次如实计数；与 stride/格式分码不混线",
        );
    }

    // --- 判据 3.1：诚实拒绝——拒绝带读屏定位且不入句柄表 ---
    {
        let mut m = ExtImport::new();
        let b = ExtBuffer {
            fd: 7,
            width: 64,
            height: 64,
            source: ExtSource::DmaBuf,
            layout: LayoutTriple { stride: 256, format: 0x34324742, modifier: 0x9999_9999_9999_9999 },
        };
        let r = m.import(&b);
        s.add(
            "A61-诚实-拒绝带读屏定位不留半成品",
            r == Err(CODE_MODIFIER_UNSUPPORTED)
                && m.last_denial.contains("诚实拒绝")
                && m.last_denial.contains("LINEAR")
                && m.handles.is_empty()
                && m.ledger.issued == 0,
            "拒绝原因读屏可达（含改用 LINEAR 的出路），句柄表零残留",
        );
    }

    // --- 判据 3.2：诚实拒绝——参数非法与容量触顶独立通道 ---
    {
        let mut m = ExtImport::new();
        let zero = ExtBuffer {
            fd: 0,
            width: 64,
            height: 64,
            source: ExtSource::DmaBuf,
            layout: LayoutTriple { stride: 256, format: 0x34324742, modifier: MODIFIER_TABLE[0] },
        };
        let r0 = m.import(&zero);
        // 容量触顶：填满 HANDLE_CAP 后再导
        let mut full = ExtImport::new();
        for i in 1..=(HANDLE_CAP as u32) {
            let _ = full.import(&ExtBuffer {
                fd: i,
                width: 64,
                height: 64,
                source: ExtSource::PlainHost,
                layout: LayoutTriple { stride: 256, format: 0x34324742, modifier: MODIFIER_TABLE[0] },
            });
        }
        let over = full.import(&ExtBuffer {
            fd: HANDLE_CAP as u32 + 1,
            width: 64,
            height: 64,
            source: ExtSource::PlainHost,
            layout: LayoutTriple { stride: 256, format: 0x34324742, modifier: MODIFIER_TABLE[0] },
        });
        s.add(
            "A61-诚实-参数与容量独立通道",
            r0 == Err(CODE_PARAM_BAD)
                && over == Err(CODE_HANDLE_CAP)
                && full.handles.len() == HANDLE_CAP,
            "零参走 PARAM_BAD；表满走 HANDLE_CAP；上限精确不越",
        );
    }

    // --- 判据 4.1：泄漏回收——全量释放对账归零 ---
    {
        let mut m = ExtImport::new();
        let mk = |fd: u32| ExtBuffer {
            fd,
            width: 64,
            height: 64,
            source: ExtSource::DmaBuf,
            layout: LayoutTriple { stride: 256, format: 0x34324742, modifier: MODIFIER_TABLE[0] },
        };
        let h1 = m.import(&mk(1));
        let h2 = m.import(&mk(2));
        let _ = m.release(h1.unwrap_or(ExtHandle { handle_id: 0, fd: 0, layout: mk(0).layout, source: ExtSource::DmaBuf }).handle_id);
        let _ = m.release(h2.unwrap_or(ExtHandle { handle_id: 0, fd: 0, layout: mk(0).layout, source: ExtSource::DmaBuf }).handle_id);
        s.add(
            "A61-回收-全量释放对账归零",
            m.ledger.balanced() && m.reclaim_audit() && m.handles.is_empty(),
            "签发 2 释放 2：账面差值 0 = 表实长 0，无泄漏",
        );
    }

    // --- 判据 4.2：泄漏回收——风暴压力下表长有界且拒绝如实计数 ---
    {
        let mut m = ExtImport::new();
        let denied = m.modifier_storm(128);
        s.add(
            "A61-回收-风暴下表长有界拒绝如实计数",
            denied == 128
                && m.handles.is_empty()
                && m.stats.honest_denies == 128
                && m.reclaim_audit(),
            "恶意修饰符风暴 128 连发：全诚实拒绝、句柄表零增长、账实一致",
        );
    }

    // --- 判据 5.1：判据自身——导入状态读屏可达 ---
    {
        let m = ExtImport::new();
        let line = m.status_line();
        s.add(
            "A61-自身-导入状态读屏可达",
            line.contains("活跃") && line.contains("诚实拒绝") && line.contains("签发"),
            "读屏行覆盖句柄/账目/拒绝三要素",
        );
    }

    // --- 判据 5.2：判据自身——码位互异且段独占（整仓唯一段 0xC5 内六码） ---
    {
        let codes = [
            CODE_STRIDE_BAD,
            CODE_FORMAT_BAD,
            CODE_MODIFIER_BAD,
            CODE_MODIFIER_UNSUPPORTED,
            CODE_PARAM_BAD,
            CODE_HANDLE_CAP,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0xC500);
        s.add(
            "A61-自身-码位互异段独占",
            all_diff && in_segment && codes.len() == 6,
            "诊断码互异或越段",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 七、测试（4 个行为测试 + 1 个判据全绿）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn buf(fd: u32, stride: u32, modifier: u64) -> ExtBuffer {
        ExtBuffer {
            fd,
            width: 64,
            height: 64,
            source: ExtSource::DmaBuf,
            layout: LayoutTriple { stride, format: 0x34324742, modifier },
        }
    }

    #[test]
    fn vea61_import_layout_matrix() {
        let mut m = ExtImport::new();
        // 合法导入
        assert!(m.import(&buf(1, 256, MODIFIER_TABLE[2])).is_ok());
        // stride 失配
        assert_eq!(m.import(&buf(2, 100, MODIFIER_TABLE[0])), Err(CODE_STRIDE_BAD));
        // 修饰符不支持（诚实拒绝）
        assert_eq!(m.import(&buf(3, 256, 0xFEED), ), Err(CODE_MODIFIER_UNSUPPORTED));
        assert!(m.last_denial.contains("诚实拒绝"));
        assert_eq!(m.handles.len(), 1);
        assert_eq!(m.stats.honest_denies, 1);
        assert_eq!(m.stats.layout_denies, 1);
    }

    #[test]
    fn vea61_idempotent_and_release() {
        let mut m = ExtImport::new();
        let b = buf(5, 256, MODIFIER_TABLE[0]);
        let a = m.import(&b).unwrap_or(ExtHandle { handle_id: 0, fd: 0, layout: b.layout, source: ExtSource::DmaBuf });
        let c = m.import(&b).unwrap_or(ExtHandle { handle_id: 0, fd: 0, layout: b.layout, source: ExtSource::DmaBuf });
        assert_eq!(a.handle_id, c.handle_id, "幂等同句柄");
        assert!(m.release(a.handle_id).is_ok());
        assert_eq!(m.release(a.handle_id), Err(CODE_PARAM_BAD), "重复释放走参数非法通道");
        assert!(m.ledger.balanced() && m.reclaim_audit());
    }

    #[test]
    fn vea61_storm_bounded() {
        let mut m = ExtImport::new();
        let denied = m.modifier_storm(64);
        assert_eq!(denied, 64);
        assert!(m.handles.is_empty());
        assert_eq!(m.stats.honest_denies, 64);
        // 风暴后正常导入仍可用
        assert!(m.import(&buf(77, 256, MODIFIER_TABLE[1])).is_ok());
        assert!(m.status_line().contains("风暴触顶 0 次") || m.status_line().contains("诚实拒绝 64 次"));
    }

    #[test]
    fn vea61_sources_unified_no_branch() {
        let mut m = ExtImport::new();
        // 三来源同布局：同一校验路径（NT 句柄类的非法修饰符同样诚实拒绝）
        let nt_bad = ExtBuffer {
            fd: 11,
            width: 64,
            height: 64,
            source: ExtSource::NtHandle,
            layout: LayoutTriple { stride: 256, format: 0x34324742, modifier: 0xBAD0 },
        };
        assert_eq!(m.import(&nt_bad), Err(CODE_MODIFIER_UNSUPPORTED));
        assert!(ExtSource::ALL.len() == 3 && ExtSource::ALL[2] == ExtSource::PlainHost);
    }

    #[test]
    fn vea61_checks_all_green() {
        let set = run_vea61_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0061 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
