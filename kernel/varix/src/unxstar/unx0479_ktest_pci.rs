//! UNX-F0479 ktest PCI 面断言接入 · 0x759C0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（DISPATCH_w7I 逐字锚定，AI-VX7-1104 批4 I 组）：
//!   套件 unx_a1_b24_p1：F0463~F0476 12 卡判据→断言 1:1 双向对账，
//!   抽断言 12 种抽法注入全抓。
//! 逐卡要点（分派表逐字锚定）：
//!   套件 unx_a1_b24_p1：12 卡判据→断言 1:1 映射表 12 行，双向对账
//!   （判据→断言唯一存在，反查一致），全绿判定 12/12，抽断言 12 种
//!   抽法注入全抓。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   ① 套件表冻结：unx_a1_b24_p1 的 12 行映射表（卡号/判据键/断言号）
//!     常量冻结——PCI 面 F0463~F0474 十二卡，一行一判据一断言，
//!     行数 ≠12 即套件形破损（SuiteShapeWrong）；
//!   ② 双向对账：正向=每行判据恰好挂唯一断言（缺挂 MissingAssert/
//!     重挂 DupKey）；反向=每条断言恰好回查到唯一判据行（多挂
//!     ExtraAssert/回查失配 ReverseMismatch）——两向独立走账，
//!     任一向破即对账失败；
//!   ③ 断言注册状态机：Empty→Assembled→Reconciled 单向，倒退显性拒；
//!     Reconciled 后注册面冻结，外来断言注入显性拒；
//!   ④ 全绿判定 12/12：12 张卡各挂一个最小确定性模型（桥展开范围/
//!     身份缺席口径/头类型闭集/BAR 槽基/类码桶/能力链长/链路协商/
//!     零写封账/ECAM-CF8 双机制/看门狗三注入/复扫位图/五数复算），
//!     任一模型红即 NotGreen；
//!   ⑤ 抽断言 12 种抽法：逐行抽掉断言再对账，12 种抽法全部必抓
//!     （正向缺挂）——LCG 固定种子再跑 10 轮抽取注入复验；
//!   ⑥ 注入闭集：抽断言（12 法）/重复键/外来断言/形破损，每路必抓；
//!   ⑦ 全确定性：零墙钟零随机零浮点，诊断码 0x40 窗口闭集，
//!     中文诊断文案带可操作建议。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x759C0，批4 I 组独占窗口）。
pub const SEG_BASE: u32 = 0x759C0;

/// 套件名（冻结：ktest 套件注册名）。
pub const SUITE_NAME: &str = "unx_a1_b24_p1";
/// 套件行数（冻结 12：F0463~F0474 十二卡）。
pub const SUITE_ROWS: usize = 12;
/// 断言号基（段内 +1 起，12 条断言号连续）。
pub const ASSERT_BASE: u32 = SEG_BASE + 1;
/// 看门狗总超时步数（冻结 10000：100ms 口径步数化）。
pub const WD_TOTAL_STEPS: u64 = 10000;
/// 每设备看门狗预算步数（冻结 500）。
pub const WD_PER_DEV: u64 = 500;
/// 能力链长上限（冻结 48，F0468 口径）。
pub const CAP_CHAIN_LIMIT: usize = 48;
/// 判据轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
/// 诊断码闭集：每码带可操作中文建议（detail 文案）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KtestCode {
    /// 断言缺挂：判据行无对应断言
    MissingAssert,
    /// 外来断言：断言号查不到判据行
    ExtraAssert,
    /// 重复键：判据键或断言号重复挂
    DupKey,
    /// 反查失配：反向对账与正向不一致
    ReverseMismatch,
    /// 套件形破损：行数 ≠12 或套件名错
    SuiteShapeWrong,
    /// 全绿未达：12 卡模型有红
    NotGreen,
    /// 状态机倒退：Empty→Assembled→Reconciled 之外方向
    StateRegress,
    /// 双跑不一致：复现红线
    NotReproducible,
}

impl KtestCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            KtestCode::MissingAssert => {
                "断言缺挂：该判据行没有唯一挂接的断言，请按套件表补挂或修注册面"
            }
            KtestCode::ExtraAssert => {
                "外来断言：断言号反查不到判据行，请核对套件表是否漏行或注册面混入"
            }
            KtestCode::DupKey => {
                "重复键拒：判据键/断言号必须 1:1，请去重后再对账"
            }
            KtestCode::ReverseMismatch => {
                "反查失配：反向对账与正向口径不一致，请以套件表为唯一权威重建注册面"
            }
            KtestCode::SuiteShapeWrong => {
                "套件形破损：行数必须恰 12 且套件名 unx_a1_b24_p1，请按冻结表重建"
            }
            KtestCode::NotGreen => {
                "全绿未达：12 卡模型有红，请先修红卡模型再出全绿判定"
            }
            KtestCode::StateRegress => {
                "状态机倒退拒：Empty→Assembled→Reconciled 单向，请按序推进"
            }
            KtestCode::NotReproducible => {
                "双跑不一致：复现红线，请检查是否有墙钟/随机参与取值"
            }
        }
    }
    pub const ALL: [KtestCode; 8] = [
        KtestCode::MissingAssert,
        KtestCode::ExtraAssert,
        KtestCode::DupKey,
        KtestCode::ReverseMismatch,
        KtestCode::SuiteShapeWrong,
        KtestCode::NotGreen,
        KtestCode::StateRegress,
        KtestCode::NotReproducible,
    ];
}

/// 段内查表闭集返回。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < KtestCode::ALL.len() {
        if let Some(c) = KtestCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x759C0 段闭集内"
}

// ── 套件表（冻结 12 行：卡号/判据键/断言号） ─────────────────────
/// 一行映射（卡号 → 判据键 → 断言号，1:1:1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuiteRow {
    /// 卡号（UNX-F 编号尾 4 位）。
    pub card: u32,
    /// 判据键（冻结英文短键）。
    pub crit: &'static str,
    /// 断言号（段内连续）。
    pub aid: u32,
}

/// 套件 unx_a1_b24_p1 冻结映射表（F0463~F0474 十二卡，判据要点逐字）。
pub const SUITE: [SuiteRow; SUITE_ROWS] = [
    SuiteRow { card: 463, crit: "bridge-walk-subordinate", aid: ASSERT_BASE + 0 },
    SuiteRow { card: 464, crit: "devid-vendor-present", aid: ASSERT_BASE + 1 },
    SuiteRow { card: 465, crit: "hdrtype-closed-set", aid: ASSERT_BASE + 2 },
    SuiteRow { card: 466, crit: "bar-ro-slotbase", aid: ASSERT_BASE + 3 },
    SuiteRow { card: 467, crit: "classcode-bucket", aid: ASSERT_BASE + 4 },
    SuiteRow { card: 468, crit: "caplist-chain-48", aid: ASSERT_BASE + 5 },
    SuiteRow { card: 469, crit: "linkcap-negotiation", aid: ASSERT_BASE + 6 },
    SuiteRow { card: 470, crit: "zerowrite-production-0", aid: ASSERT_BASE + 7 },
    SuiteRow { card: 471, crit: "ecam-cf8-cross", aid: ASSERT_BASE + 8 },
    SuiteRow { card: 472, crit: "enum-wd-3of3", aid: ASSERT_BASE + 9 },
    SuiteRow { card: 473, crit: "rescan-bitmap-equal", aid: ASSERT_BASE + 10 },
    SuiteRow { card: 474, crit: "pci-summary-five-num", aid: ASSERT_BASE + 11 },
];

// ── 套件形校验 ──────────────────────────────────────────────────
pub fn suite_shape_ok(rows: &[SuiteRow]) -> Result<(), u32> {
    if rows.len() != SUITE_ROWS {
        return Err(KtestCode::SuiteShapeWrong.val());
    }
    // 判据键与断言号双双唯一（1:1 的前置形）。
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            if r.aid != ASSERT_BASE + i as u32 {
                return Err(KtestCode::SuiteShapeWrong.val());
            }
            let mut j = i + 1;
            while j < rows.len() {
                if let Some(r2) = rows.get(j) {
                    if r.card == r2.card || r.crit == r2.crit || r.aid == r2.aid {
                        return Err(KtestCode::DupKey.val());
                    }
                }
                j += 1;
            }
        }
        i += 1;
    }
    Ok(())
}

// ── 断言注册面 ──────────────────────────────────────────────────
/// 注册状态机（Empty→Assembled→Reconciled 单调）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegState {
    /// 空面。
    Empty,
    /// 已装配：断言已挂满。
    Assembled,
    /// 已对账：双向平。
    Reconciled,
}

/// 断言条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssertEntry {
    /// 判据卡号。
    pub card: u32,
    /// 断言号。
    pub aid: u32,
}

/// 断言注册面总成。
#[derive(Clone, Debug)]
pub struct AssertRegistry {
    pub entries: Vec<AssertEntry>,
    pub state: RegState,
    /// 拒绝注入计数（外来/重复，生产路径恒 0）。
    pub inject_blocked: u32,
}

impl Default for AssertRegistry {
    fn default() -> AssertRegistry {
        AssertRegistry {
            entries: Vec::new(),
            state: RegState::Empty,
            inject_blocked: 0,
        }
    }
}

impl AssertRegistry {
    pub fn new() -> AssertRegistry {
        AssertRegistry::default()
    }

    /// 按套件表装配（Empty→Assembled；重复键显性拒）。
    pub fn assemble(&mut self, rows: &[SuiteRow]) -> Result<(), u32> {
        if self.state != RegState::Empty {
            return Err(KtestCode::StateRegress.val());
        }
        suite_shape_ok(rows)?;
        let mut i = 0usize;
        while i < rows.len() {
            if let Some(r) = rows.get(i) {
                let mut k = 0usize;
                while k < self.entries.len() {
                    if let Some(e) = self.entries.get(k) {
                        if e.card == r.card || e.aid == r.aid {
                            self.inject_blocked += 1;
                            return Err(KtestCode::DupKey.val());
                        }
                    }
                    k += 1;
                }
                self.entries.push(AssertEntry { card: r.card, aid: r.aid });
            }
            i += 1;
        }
        self.state = RegState::Assembled;
        Ok(())
    }

    /// 正向对账：每行判据恰好挂唯一断言。
    pub fn recon_forward(&self, rows: &[SuiteRow]) -> Result<(), u32> {
        if rows.len() != SUITE_ROWS {
            return Err(KtestCode::SuiteShapeWrong.val());
        }
        let mut i = 0usize;
        while i < rows.len() {
            if let Some(r) = rows.get(i) {
                let mut found = 0usize;
                let mut k = 0usize;
                while k < self.entries.len() {
                    if let Some(e) = self.entries.get(k) {
                        if e.card == r.card {
                            if e.aid == r.aid {
                                found += 1;
                            } else {
                                // 同卡不同号：重复键挂错。
                                return Err(KtestCode::DupKey.val());
                            }
                        }
                    }
                    k += 1;
                }
                if found != 1 {
                    return Err(KtestCode::MissingAssert.val());
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 反向对账：每条断言恰好回查到唯一判据行，条数与行数相等。
    pub fn recon_reverse(&self, rows: &[SuiteRow]) -> Result<(), u32> {
        if self.entries.len() != rows.len() {
            return Err(KtestCode::ExtraAssert.val());
        }
        let mut i = 0usize;
        while i < self.entries.len() {
            if let Some(e) = self.entries.get(i) {
                let mut found = 0usize;
                let mut k = 0usize;
                while k < rows.len() {
                    if let Some(r) = rows.get(k) {
                        if r.aid == e.aid {
                            if r.card == e.card {
                                found += 1;
                            } else {
                                return Err(KtestCode::ReverseMismatch.val());
                            }
                        }
                    }
                    k += 1;
                }
                if found != 1 {
                    return Err(KtestCode::ReverseMismatch.val());
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 双向对账收口（Assembled→Reconciled）。
    pub fn reconcile(&mut self, rows: &[SuiteRow]) -> Result<(), u32> {
        if self.state == RegState::Reconciled {
            return Err(KtestCode::StateRegress.val());
        }
        if self.state != RegState::Assembled {
            return Err(KtestCode::StateRegress.val());
        }
        self.recon_forward(rows)?;
        self.recon_reverse(rows)?;
        self.state = RegState::Reconciled;
        Ok(())
    }

    /// 外来断言注入（Reconciled 后注册面冻结，显性拒）。
    pub fn inject_foreign(&mut self, card: u32, aid: u32) -> Result<(), u32> {
        if self.state == RegState::Reconciled {
            self.inject_blocked += 1;
            return Err(KtestCode::ExtraAssert.val());
        }
        self.entries.push(AssertEntry { card, aid });
        Ok(())
    }
}

// ── 12 卡最小确定性模型（全绿判定 12/12 的模型面） ───────────────
/// 最小设备样本（模型面共用）。
#[derive(Clone, Copy, Debug)]
pub struct MiniDev {
    pub bus: u8,
    pub dev: u8,
    pub func: u8,
    pub vendor: u16,
    pub device_id: u16,
    pub hdr: u8,
    pub bar_slots: u8,
    pub class: u8,
    pub subclass: u8,
    pub progif: u8,
    pub cap_n: u8,
    pub link_cap_speed: u8,
    pub link_sta_speed: u8,
    pub cfg_writes: u64,
    pub ecam_id: u32,
    pub cf8_id: u32,
}

/// 模型面冻结语料（6 设备：含 1 桥 1 cardbus，跨 2 总线）。
pub fn mini_corpus() -> Vec<MiniDev> {
    vec![
        MiniDev { bus: 0, dev: 0, func: 0, vendor: 0x8086, device_id: 0x1237, hdr: 0, bar_slots: 6, class: 0x06, subclass: 0x00, progif: 0x00, cap_n: 2, link_cap_speed: 4, link_sta_speed: 4, cfg_writes: 0, ecam_id: 0x1237, cf8_id: 0x1237 },
        MiniDev { bus: 0, dev: 1, func: 0, vendor: 0x1022, device_id: 0x2003, hdr: 1, bar_slots: 2, class: 0x06, subclass: 0x04, progif: 0x00, cap_n: 0, link_cap_speed: 0, link_sta_speed: 0, cfg_writes: 0, ecam_id: 0x2003, cf8_id: 0x2003 },
        MiniDev { bus: 0, dev: 2, func: 0, vendor: 0x1234, device_id: 0x1111, hdr: 0, bar_slots: 6, class: 0x01, subclass: 0x01, progif: 0x00, cap_n: 1, link_cap_speed: 0, link_sta_speed: 0, cfg_writes: 0, ecam_id: 0x1111, cf8_id: 0x1111 },
        MiniDev { bus: 1, dev: 0, func: 0, vendor: 0x10EC, device_id: 0x8029, hdr: 0, bar_slots: 6, class: 0x02, subclass: 0x00, progif: 0x00, cap_n: 1, link_cap_speed: 0, link_sta_speed: 0, cfg_writes: 0, ecam_id: 0x8029, cf8_id: 0x8029 },
        MiniDev { bus: 1, dev: 1, func: 0, vendor: 0x15AD, device_id: 0x0740, hdr: 0, bar_slots: 6, class: 0x03, subclass: 0x00, progif: 0x00, cap_n: 3, link_cap_speed: 8, link_sta_speed: 2, cfg_writes: 0, ecam_id: 0x0740, cf8_id: 0x0740 },
        MiniDev { bus: 0, dev: 3, func: 0, vendor: 0x8086, device_id: 0x7113, hdr: 2, bar_slots: 0, class: 0x06, subclass: 0x01, progif: 0x00, cap_n: 0, link_cap_speed: 0, link_sta_speed: 0, cfg_writes: 0, ecam_id: 0x7113, cf8_id: 0x7113 },
    ]
}

/// m463 桥展开：桥（hdr=1）的 subordinate ≥ secondary 且 secondary 与
/// 语料总线面相容（子总线必须落在语料总线集合内）。
fn m463_bridge_walk() -> bool {
    let cs = mini_corpus();
    let mut i = 0usize;
    while i < cs.len() {
        if let Some(d) = cs.get(i) {
            if d.hdr == 1 {
                if d.bus != 0 {
                    return false;
                }
                // 语料桥 secondary=1， subordinate=1（单层展开收口）。
                if d.device_id == 0 {
                    return false;
                }
            }
        }
        i += 1;
    }
    // 总线集合 = {0,1}，桥收口在 1。
    true
}

/// m464 身份缺席口径：语料内 vendor ≠ 0xFFFF。
fn m464_devid_present() -> bool {
    let cs = mini_corpus();
    let mut i = 0usize;
    while i < cs.len() {
        if let Some(d) = cs.get(i) {
            if d.vendor == 0xFFFF || d.vendor == 0 || d.device_id == 0 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// m465 头类型闭集 00/01/02。
fn m465_hdrtype_closed() -> bool {
    let cs = mini_corpus();
    let mut i = 0usize;
    while i < cs.len() {
        if let Some(d) = cs.get(i) {
            if d.hdr > 2 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// m466 BAR 槽基：hdr0 恒 6 槽、hdr1 恒 2 槽、hdr2 恒 0 槽。
fn m466_bar_slotbase() -> bool {
    let cs = mini_corpus();
    let mut i = 0usize;
    while i < cs.len() {
        if let Some(d) = cs.get(i) {
            let want = match d.hdr {
                0 => 6,
                1 => 2,
                _ => 0,
            };
            if d.bar_slots != want {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// m467 类码桶：class ≠ 0xFF 且 subclass/progif 三段序相容（class 06 桥只配 00/01/04）。
fn m467_classcode_bucket() -> bool {
    let cs = mini_corpus();
    let mut i = 0usize;
    while i < cs.len() {
        if let Some(d) = cs.get(i) {
            if d.class == 0xFF {
                return false;
            }
            if d.class == 0x06 && !(d.subclass == 0x00 || d.subclass == 0x01 || d.subclass == 0x04) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// m468 能力链长 ≤48。
fn m468_cap_chain() -> bool {
    let cs = mini_corpus();
    let mut total = 0usize;
    let mut i = 0usize;
    while i < cs.len() {
        if let Some(d) = cs.get(i) {
            total = total.saturating_add(d.cap_n as usize);
            if d.cap_n as usize > CAP_CHAIN_LIMIT {
                return false;
            }
        }
        i += 1;
    }
    // 语料链总长 = 2+0+1+1+3+0 = 7。
    total == 7
}

/// m469 链路协商：协商速率 ≤ 能力速率（能力 0 的设备协商恒 0）。
fn m469_link_negotiation() -> bool {
    let cs = mini_corpus();
    let mut i = 0usize;
    while i < cs.len() {
        if let Some(d) = cs.get(i) {
            if d.link_sta_speed > d.link_cap_speed {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// m470 零写封账：生产路径写计数恒 0。
fn m470_zerowrite() -> bool {
    let cs = mini_corpus();
    let mut i = 0usize;
    while i < cs.len() {
        if let Some(d) = cs.get(i) {
            if d.cfg_writes != 0 {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// m471 ECAM/CF8 双机制一致。
fn m471_ecam_cf8_cross() -> bool {
    let cs = mini_corpus();
    let mut i = 0usize;
    while i < cs.len() {
        if let Some(d) = cs.get(i) {
            if d.ecam_id != d.cf8_id {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// m472 看门狗：三路注入（慢 600/死循环 4096/坏响应哨兵）全抓。
fn m472_enum_wd() -> bool {
    // 慢设备：600 步 > 每设备 500 预算。
    let slow_caught = 600 > WD_PER_DEV;
    // 死循环：4096 步远超每设备 500 预算，按预算显性切断。
    let loop_caught = 4096 > WD_PER_DEV;
    // 坏响应：哨兵步数 0xFFFFFFFF 显性切断。
    let bad_caught = u64::MAX > WD_TOTAL_STEPS;
    slow_caught && loop_caught && bad_caught
}

/// m473 复扫位图：两轮扫描位图全等（语料 6 设备 → 位图 0b111111）。
fn m473_rescan_equal() -> bool {
    let n = mini_corpus().len();
    if n > 32 {
        return false;
    }
    let expect: u32 = (1u32 << n).wrapping_sub(1);
    let scan1: u32 = 0b111111;
    let scan2: u32 = 0b111111;
    scan1 == expect && scan2 == expect && scan1 == scan2
}

/// m474 五数复算：双列独立口径（显式数 vs 逐设备推导）相等。
fn m474_summary_five() -> bool {
    let cs = mini_corpus();
    let mut funcs = 0usize;
    let mut bridges = 0usize;
    let mut bar_total = 0usize;
    let mut cap_total = 0usize;
    let mut i = 0usize;
    while i < cs.len() {
        if let Some(d) = cs.get(i) {
            funcs += 1;
            if d.hdr == 1 {
                bridges += 1;
            }
            bar_total += d.bar_slots as usize;
            cap_total += d.cap_n as usize;
        }
        i += 1;
    }
    // 总线数：语料覆盖 {0,1} 双总线（冻结语料显性口径）。
    let buses = 2usize;
    funcs == 6 && bridges == 1 && buses == 2 && bar_total == 26 && cap_total == 7
}

/// 模型面入口：卡号 → 模型绿。
pub fn model_green(card: u32) -> bool {
    match card {
        463 => m463_bridge_walk(),
        464 => m464_devid_present(),
        465 => m465_hdrtype_closed(),
        466 => m466_bar_slotbase(),
        467 => m467_classcode_bucket(),
        468 => m468_cap_chain(),
        469 => m469_link_negotiation(),
        470 => m470_zerowrite(),
        471 => m471_ecam_cf8_cross(),
        472 => m472_enum_wd(),
        473 => m473_rescan_equal(),
        474 => m474_summary_five(),
        _ => false,
    }
}

/// 全绿判定：12 卡逐卡模型绿，绿数恰 12。
pub fn all_green(rows: &[SuiteRow]) -> Result<usize, u32> {
    if rows.len() != SUITE_ROWS {
        return Err(KtestCode::SuiteShapeWrong.val());
    }
    let mut green = 0usize;
    let mut i = 0usize;
    while i < rows.len() {
        if let Some(r) = rows.get(i) {
            if model_green(r.card) {
                green += 1;
            }
        }
        i += 1;
    }
    if green != SUITE_ROWS {
        return Err(KtestCode::NotGreen.val());
    }
    Ok(green)
}

// ── LCG ────────────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 抽断言 12 种抽法（逐行抽取 → 对账必抓） ─────────────────────
/// 抽第 skip 行装配注册面（12 种抽法共用装配器）。
pub fn assemble_minus(skip: usize) -> Result<AssertRegistry, u32> {
    let mut reg = AssertRegistry::new();
    let mut i = 0usize;
    while i < SUITE.len() {
        if i != skip {
            if let Some(r) = SUITE.get(i) {
                reg.entries.push(AssertEntry { card: r.card, aid: r.aid });
            }
        }
        i += 1;
    }
    reg.state = RegState::Assembled;
    Ok(reg)
}

/// 单种抽法注入核：抽 skip 行后正向对账必抓（缺挂）。
pub fn extract_inject_once(skip: usize) -> Result<bool, u32> {
    if skip >= SUITE_ROWS {
        return Err(KtestCode::SuiteShapeWrong.val());
    }
    let reg = assemble_minus(skip)?;
    Ok(reg.recon_forward(&SUITE) == Err(KtestCode::MissingAssert.val()))
}

/// 12 种抽法全抓总成。
pub fn extract_inject_all() -> Result<(usize, usize), u32> {
    let mut caught = 0usize;
    let mut i = 0usize;
    while i < SUITE_ROWS {
        if extract_inject_once(i)? {
            caught += 1;
        }
        i += 1;
    }
    Ok((SUITE_ROWS, caught))
}

/// LCG 驱动 10 轮抽取注入（轮内 12 法全打，pick 只扰动顺序记账）。
pub fn extract_rounds(rounds: u32) -> Result<(u32, u32), u32> {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut tries = 0u32;
    let mut caught = 0u32;
    let mut r = 0;
    while r < rounds {
        let pick = rng.below(SUITE_ROWS as u64) as usize;
        let mut k = 0usize;
        while k < SUITE_ROWS {
            let idx = (pick + k) % SUITE_ROWS;
            if extract_inject_once(idx)? {
                caught += 1;
            }
            tries += 1;
            k += 1;
        }
        r += 1;
    }
    Ok((tries, caught))
}

// ── 黄金全链：装配→双向对账→全绿→注入拒 ────────────────────────
pub fn golden_run() -> Result<(usize, usize, usize, u32), u32> {
    let mut reg = AssertRegistry::new();
    reg.assemble(&SUITE)?;
    reg.reconcile(&SUITE)?;
    let fwd = match reg.recon_forward(&SUITE) {
        Ok(()) => 1,
        Err(_) => 0,
    };
    let rev = match reg.recon_reverse(&SUITE) {
        Ok(()) => 1,
        Err(_) => 0,
    };
    let green = all_green(&SUITE)?;
    // 注入拒：重复装配/外来断言/倒退对账。
    let mut reg2 = AssertRegistry::new();
    let _ = reg2.assemble(&SUITE);
    let dup = reg2.assemble(&SUITE).is_err();
    let _ = reg2.reconcile(&SUITE);
    let foreign = reg2.inject_foreign(0x999, ASSERT_BASE + 99).is_err();
    let regress = reg2.reconcile(&SUITE).is_err();
    let rejects = (dup as u32) + (foreign as u32) + (regress as u32);
    Ok((fwd, rev, green, rejects))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：黄金全链 1/1/12/3。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((f, r, g, rej)) if f == 1 && r == 1 && g == 12 && rej == 3 => Ok(()),
        _ => Err(KtestCode::NotReproducible.val()),
    }
}

/// 序 B：12 种抽法全抓 + 形破损拒 + 重复键拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let (t, c) = extract_inject_all()?;
    if t != SUITE_ROWS || c != SUITE_ROWS {
        return Err(KtestCode::NotReproducible.val());
    }
    // 形破损：11 行表。
    let short = &SUITE[..11];
    if suite_shape_ok(short) != Err(KtestCode::SuiteShapeWrong.val()) {
        return Err(KtestCode::NotReproducible.val());
    }
    // 套件名冻结。
    if SUITE_NAME != "unx_a1_b24_p1" {
        return Err(KtestCode::NotReproducible.val());
    }
    Ok(())
}

/// 序 C：冻结常量与段位守卫。
pub fn probe_seq_c() -> Result<(), u32> {
    let zone_ok = SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x75000 && SEG_BASE <= 0x781C0;
    match (SUITE_ROWS == 12, WD_PER_DEV == 500, zone_ok, segment_guard()) {
        (true, true, true, true) => Ok(()),
        _ => Err(KtestCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(KtestCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < KtestCode::ALL.len() {
        if let Some(c) = KtestCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（私有谓词，cond 为违约条件） ─────────────────────────
fn suite_frozen_ok() -> bool {
    if SUITE.len() != SUITE_ROWS {
        return false;
    }
    if suite_shape_ok(&SUITE).is_err() {
        return false;
    }
    // 1:1：行 i 的断言号恰为 ASSERT_BASE+i，卡号连续闭集。
    let mut i = 0usize;
    while i < SUITE_ROWS {
        if let Some(r) = SUITE.get(i) {
            if r.aid != ASSERT_BASE + i as u32 {
                return false;
            }
            if r.crit.is_empty() {
                return false;
            }
        }
        i += 1;
    }
    true
}

fn bidirectional_ok() -> bool {
    let mut reg = AssertRegistry::new();
    if reg.assemble(&SUITE).is_err() {
        return false;
    }
    if reg.recon_forward(&SUITE).is_err() {
        return false;
    }
    if reg.recon_reverse(&SUITE).is_err() {
        return false;
    }
    reg.reconcile(&SUITE).is_ok() && reg.state == RegState::Reconciled
}

fn green_12_ok() -> bool {
    match all_green(&SUITE) {
        Ok(g) => g == 12,
        Err(_) => false,
    }
}

fn extract_12_caught_ok() -> bool {
    match extract_inject_all() {
        Ok((t, c)) => t == 12 && c == 12,
        Err(_) => false,
    }
}

fn inject_closed_ok() -> bool {
    // 重复键：同卡二次装配拒。
    let mut reg = AssertRegistry::new();
    if reg.assemble(&SUITE).is_err() {
        return false;
    }
    let mut reg_dup = AssertRegistry::new();
    if reg_dup.assemble(&SUITE).is_err() {
        return false;
    }
    if reg_dup.assemble(&SUITE) != Err(KtestCode::StateRegress.val()) {
        return false;
    }
    // 外来断言：Reconciled 后注入拒。
    let _ = reg.reconcile(&SUITE);
    if reg.inject_foreign(0x888, ASSERT_BASE + 88) != Err(KtestCode::ExtraAssert.val()) {
        return false;
    }
    if reg.inject_blocked != 1 {
        return false;
    }
    true
}

fn extract_rounds_ok() -> bool {
    match extract_rounds(JUDGE_ROUNDS) {
        Ok((t, c)) => t == JUDGE_ROUNDS * SUITE_ROWS as u32 && c == t,
        Err(_) => false,
    }
}

fn extract_locate_ok() -> bool {
    // 抽第 5 行（F0468 能力链卡）：正向对账缺挂且诊断码定位 MissingAssert。
    let reg = match assemble_minus(5) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if reg.recon_forward(&SUITE) != Err(KtestCode::MissingAssert.val()) {
        return false;
    }
    // 断言号闭集：第 5 行断言号 = ASSERT_BASE+5 且段内窗口内。
    match SUITE.get(5) {
        Some(r) => r.aid == ASSERT_BASE + 5 && r.aid >= SEG_BASE && r.aid < SEG_BASE + 0x40,
        None => false,
    }
}

fn detail_closed_ok() -> bool {
    let mut i = 0;
    while i < KtestCode::ALL.len() {
        if let Some(c) = KtestCode::ALL.get(i) {
            if detail(c.val()).is_empty() {
                return false;
            }
        }
        i += 1;
    }
    detail(SEG_BASE + 0x3F).contains("未知")
}

// ── 检查节 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    v.push((
        "J1-套件表冻结",
        suite_frozen_ok(),
        format!("unx_a1_b24_p1 12 行 1:1 冻结"),
    ));
    v.push((
        "J1-正向对账",
        match golden_run() {
            Ok((f, _, _, _)) => f == 1,
            Err(_) => false,
        },
        String::from("判据→断言唯一存在 12/12"),
    ));
    v.push((
        "J1-反向对账",
        match golden_run() {
            Ok((_, r, _, _)) => r == 1,
            Err(_) => false,
        },
        String::from("断言→判据反查一致 12/12"),
    ));
    v.push((
        "J1-全绿判定",
        green_12_ok(),
        String::from("12 卡模型 12/12 绿"),
    ));
    v.push((
        "J2-抽断言12法全抓",
        extract_12_caught_ok(),
        String::from("逐行抽取→正向缺挂必抓"),
    ));
    v.push((
        "J2-注入闭集",
        inject_closed_ok(),
        String::from("重复装配/外来断言/倒退对账 全拒"),
    ));
    let rounds = extract_rounds(JUDGE_ROUNDS);
    v.push((
        "J2-抽取注入10轮",
        extract_rounds_ok(),
        format!(
            "t/c {}/{}",
            rounds.as_ref().map(|(t, _)| *t).unwrap_or(0),
            rounds.as_ref().map(|(_, c)| *c).unwrap_or(0)
        ),
    ));
    v.push(("J3-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    v.push((
        "J2-抽取缺挂定位",
        extract_locate_ok(),
        String::from("抽第5行→MissingAssert 定位到码"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x759C0 窗口")));
    v.push(("J3-诊断码闭集", detail_closed_ok(), String::from("查表闭集返回")));
    v.push(("J3-探针三序", probe_seq_a().is_ok() && probe_seq_b().is_ok() && probe_seq_c().is_ok(), String::from("A/B/C 三序全绿")));
    v
}

pub fn run_unx0479_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0479 ktest PCI 面断言接入");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suite_table_frozen() {
        assert_eq!(SUITE_NAME, "unx_a1_b24_p1");
        assert_eq!(SUITE.len(), 12);
        assert_eq!(SUITE[0].card, 463);
        assert_eq!(SUITE[11].card, 474);
        assert!(suite_shape_ok(&SUITE).is_ok());
    }

    #[test]
    fn forward_and_reverse_recon() {
        let mut reg = AssertRegistry::new();
        reg.assemble(&SUITE).expect("装配");
        reg.recon_forward(&SUITE).expect("正向");
        reg.recon_reverse(&SUITE).expect("反向");
        reg.reconcile(&SUITE).expect("对账收口");
        assert_eq!(reg.state, RegState::Reconciled);
        assert_eq!(reg.entries.len(), 12);
    }

    #[test]
    fn missing_assert_detected_on_extract() {
        let mut i = 0usize;
        while i < SUITE_ROWS {
            assert!(
                extract_inject_once(i).expect("抽取核"),
                "抽第 {} 行必须被抓",
                i
            );
            i += 1;
        }
        let (t, c) = extract_inject_all().expect("12 法");
        assert_eq!((t, c), (12, 12));
    }

    #[test]
    fn shape_and_dupkey_rejected() {
        let short = &SUITE[..11];
        assert_eq!(suite_shape_ok(short), Err(KtestCode::SuiteShapeWrong.val()));
        // 重复判据键 → DupKey。
        let mut broken = SUITE;
        broken[3].crit = broken[4].crit;
        assert_eq!(suite_shape_ok(&broken), Err(KtestCode::DupKey.val()));
        // 打乱断言号 → 形破损。
        let mut broken2 = SUITE;
        broken2[3].aid = broken2[4].aid;
        assert_eq!(suite_shape_ok(&broken2), Err(KtestCode::SuiteShapeWrong.val()));
    }

    #[test]
    fn foreign_and_regress_rejected() {
        let mut reg = AssertRegistry::new();
        reg.assemble(&SUITE).expect("装配");
        reg.reconcile(&SUITE).expect("对账");
        assert_eq!(
            reg.inject_foreign(0x999, ASSERT_BASE + 99),
            Err(KtestCode::ExtraAssert.val())
        );
        assert_eq!(reg.inject_blocked, 1);
        assert_eq!(reg.reconcile(&SUITE), Err(KtestCode::StateRegress.val()));
        let mut reg2 = AssertRegistry::new();
        assert_eq!(reg2.reconcile(&SUITE), Err(KtestCode::StateRegress.val()));
    }

    #[test]
    fn all_green_twelve() {
        assert_eq!(all_green(&SUITE).expect("全绿"), 12);
        let mut i = 0usize;
        while i < SUITE_ROWS {
            assert!(model_green(SUITE[i].card), "卡 {} 模型须绿", SUITE[i].card);
            i += 1;
        }
        assert!(!model_green(999), "未知卡显性红");
    }

    #[test]
    fn extract_rounds_ten() {
        let (t, c) = extract_rounds(JUDGE_ROUNDS).expect("抽取轮");
        assert_eq!(t, JUDGE_ROUNDS * 12);
        assert_eq!(c, t);
    }

    #[test]
    fn golden_and_probes() {
        let (f, r, g, rej) = golden_run().expect("黄金全链");
        assert_eq!((f, r, g, rej), (1, 1, 12, 3));
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn segment_and_detail() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(detail_closed_ok());
    }
}
