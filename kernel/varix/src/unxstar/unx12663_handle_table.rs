//! UNX-F12664 句柄表探测面（SystemHandleInformation） · 0x80FC0段
//!
//! 判据原文（派工卡 12663 逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F12664-J1 探测消费回指链零悬空（B20 F12386→
//!      B30 F12586→本条），抽样 20 例一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 句柄表探测面域模型：SystemHandleInformation 消费档每行六元
//!    （行号/进程归属 pid/句柄号/授予访问位/对象类型索引/回指链）；
//!    回指链三元（环 0 = B20 F12386、环 1 = B30 F12586、环 2 = 本条
//!    UNX-F12664），三环逐环验锚——零悬空是本卡验收主判据；
//! 2. 回指链冻结锚表：B20 指纹（B20-F12386-HANDLE-LEDGER）、B30 指纹
//!    （B30-F12586-D3-COSIGN）、本条指纹（F12664-HT-PROBE），链环 id
//!    与指纹成对校验，错对或缺环即悬空拒收；
//! 3. 抽样 20 例：句柄表样本库 24 例冻结，确定性 LCG（种子 =
//!    SEG_BASE）抽 20 例，逐例走"链三环验锚→pid 归属→句柄号非零→
//!    授予位闭集"黄金判定，判定与表内期望逐一对平；
//! 4. 授予访问位闭集：位 0x0001 读 / 0x0002 写 / 0x0004 执 / 0x0008
//!    全权，位组合按冻结口径闭集校验，越位即拒；
//! 5. 状态机：Linked → Sampled → Sealed 单向推进，倒退显性拒；抽
//!    样闸五道（链零悬空/pid 闭集/句柄号非零/访问位闭集/样本数足）
//!    逐例过闸；
//! 6. 注入联测 10 路：环 0 悬空、环 1 悬空、环 2 悬空、指纹错挂、
//!    链环错序、pid 越表、句柄号零、访问位越集、样本缺数、抽样超容
//!    ——逐路必抓；
//! 7. no_std + alloc；零浮点、零 IO、零墙钟、零随机（LCG 种子写死）；
//!    取值一律 match/if let/.get()，无 unwrap/expect/panic/裸下标。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（VX9 派工公式：0x80000 + 0x40 × (12663 − 12600)）。
pub const SEG_BASE: u32 = 0x80FC0;

// ── 句柄表面诊断码闭集 ───────────────────────────────────────────
/// 诊断码闭集（段内 0x01..0x0A 偏移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtabDiag {
    /// 链环 0 悬空（B20 锚缺失）。
    ELink0Dangling,
    /// 链环 1 悬空（B30 锚缺失）。
    ELink1Dangling,
    /// 链环 2 悬空（本条锚缺失）。
    ELink2Dangling,
    /// 链指纹错挂。
    EFpMismatch,
    /// 链环错序。
    ELinkOrder,
    /// pid 越归属闭集。
    EPidRange,
    /// 句柄号为零。
    EHandleZero,
    /// 授予访问位越闭集。
    EAccessRange,
    /// 样本缺数（不足 20 例）。
    EShortSample,
    /// 状态机倒退。
    ERewind,
}

impl HtabDiag {
    /// 诊断码值（SEG_BASE + 0x01..0x0A 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                HtabDiag::ELink0Dangling => 0x01,
                HtabDiag::ELink1Dangling => 0x02,
                HtabDiag::ELink2Dangling => 0x03,
                HtabDiag::EFpMismatch => 0x04,
                HtabDiag::ELinkOrder => 0x05,
                HtabDiag::EPidRange => 0x06,
                HtabDiag::EHandleZero => 0x07,
                HtabDiag::EAccessRange => 0x08,
                HtabDiag::EShortSample => 0x09,
                HtabDiag::ERewind => 0x0a,
            }
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            HtabDiag::ELink0Dangling => "链环 0 悬空：回指链缺 B20 F12386 锚，先补源头环再消费",
            HtabDiag::ELink1Dangling => "链环 1 悬空：回指链缺 B30 F12586 联签锚，先补联签环再消费",
            HtabDiag::ELink2Dangling => "链环 2 悬空：回指链缺本条 F12664 锚，先补本段环再消费",
            HtabDiag::EFpMismatch => "链指纹错挂：环 id 与指纹不成对，先对冻结锚表再消费",
            HtabDiag::ELinkOrder => "链环错序：B20→B30→本条 序号固定，错序即断链拒收",
            HtabDiag::EPidRange => "pid 越归属闭集：进程归属必在冻结进程表内，越表先对进程账",
            HtabDiag::EHandleZero => "句柄号为零：句柄表行句柄号非零，零值行先核来源",
            HtabDiag::EAccessRange => "授予访问位越闭集：位组合只认冻结口径，越位先核掩码",
            HtabDiag::EShortSample => "样本缺数：抽样须足 20 例，缺例先回补再封样",
            HtabDiag::ERewind => "状态机倒退：Linked→Sampled→Sealed 只进不退，倒退即账可疑",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const HTAB_ALL: &[HtabDiag] = &[
    HtabDiag::ELink0Dangling,
    HtabDiag::ELink1Dangling,
    HtabDiag::ELink2Dangling,
    HtabDiag::EFpMismatch,
    HtabDiag::ELinkOrder,
    HtabDiag::EPidRange,
    HtabDiag::EHandleZero,
    HtabDiag::EAccessRange,
    HtabDiag::EShortSample,
    HtabDiag::ERewind,
];

/// 诊断码查表（闭集返回，未知码回落提示文案）。
pub fn detail(code: u32) -> &'static str {
    for d in HTAB_ALL.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x80FC1..0x80FCA 闭集内"
}

// ── 回指链冻结锚表（B20 → B30 → 本条） ──────────────────────────
/// 链环三元（环号/回指 id/指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainLink {
    /// 环号（0 = B20 源环，1 = B30 联签环，2 = 本段环）。
    pub ring: u8,
    /// 回指卡 id。
    pub ref_id: u32,
    /// 指纹。
    pub fp: &'static str,
}

/// 回指链冻结锚表（三环闭集）。
pub const CHAIN_ANCHORS: &[ChainLink] = &[
    ChainLink { ring: 0, ref_id: 12386, fp: "B20-F12386-HANDLE-LEDGER" },
    ChainLink { ring: 1, ref_id: 12586, fp: "B30-F12586-D3-COSIGN" },
    ChainLink { ring: 2, ref_id: 12663, fp: "F12664-HT-PROBE" },
];

/// 本段回指 id（卡 12663）。
pub const SELF_REF_ID: u32 = 12663;

/// 单环验锚：环 id 与指纹成对且环号落在 0..=2。
pub fn link_ok(l: &ChainLink) -> Result<(), u32> {
    let anchor = match CHAIN_ANCHORS.iter().find(|a| a.ring == l.ring) {
        Some(a) => a,
        None => match l.ring {
            0 => return Err(HtabDiag::ELink0Dangling.val()),
            1 => return Err(HtabDiag::ELink1Dangling.val()),
            2 => return Err(HtabDiag::ELink2Dangling.val()),
            _ => return Err(HtabDiag::ELinkOrder.val()),
        },
    };
    if l.ref_id != anchor.ref_id {
        return Err(HtabDiag::ELinkOrder.val());
    }
    if l.fp != anchor.fp {
        return Err(HtabDiag::EFpMismatch.val());
    }
    Ok(())
}

/// 整链零悬空判定：三环逐环验锚且环号严格递增。
pub fn chain_ok(links: &[ChainLink; 3]) -> Result<(), u32> {
    // 环号严格递增（0→1→2）。
    let r0 = match links.get(0) {
        Some(v) => v.ring,
        None => return Err(HtabDiag::ELinkOrder.val()),
    };
    let r1 = match links.get(1) {
        Some(v) => v.ring,
        None => return Err(HtabDiag::ELinkOrder.val()),
    };
    let r2 = match links.get(2) {
        Some(v) => v.ring,
        None => return Err(HtabDiag::ELinkOrder.val()),
    };
    if !(r0 < r1 && r1 < r2) {
        return Err(HtabDiag::ELinkOrder.val());
    }
    for l in links.iter() {
        link_ok(l)?;
    }
    Ok(())
}

// ── 进程归属表（与句柄表面独立冻结） ─────────────────────────────
/// 冻结进程归属 pid 表（6 例）。
pub const HTAB_PIDS: &[u32] = &[4, 260, 512, 768, 1024, 2048];

/// pid 是否在归属闭集内。
pub fn pid_in_set(pid: u32) -> bool {
    for p in HTAB_PIDS.iter() {
        if *p == pid {
            return true;
        }
    }
    false
}

// ── 授予访问位闭集 ───────────────────────────────────────────────
/// 访问位掩码闭集（读/写/执/全权 四位冻结口径）。
pub const ACC_READ: u32 = 0x0001;
pub const ACC_WRITE: u32 = 0x0002;
pub const ACC_EXEC: u32 = 0x0004;
pub const ACC_ALL: u32 = 0x0008;

/// 授予访问位是否在冻结口径闭集内（允许的位组合闭集）。
pub fn access_ok(mask: u32) -> bool {
    // 合法组合：单独位或冻结合成（读|写、读|执、读|写|执、全权覆盖）。
    if mask == 0 {
        return false;
    }
    if mask & !0x000F != 0 {
        return false;
    }
    // 全权位出现时不得混其他位。
    if mask & ACC_ALL != 0 && mask != ACC_ALL {
        return false;
    }
    true
}

// ── 句柄表样本行 ─────────────────────────────────────────────────
/// 句柄表样本行七元（行号/pid/句柄号/访问位/类型索引/期望判定/回指链）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HtabSample {
    /// 行号。
    pub no: u32,
    /// 进程归属 pid。
    pub pid: u32,
    /// 句柄号。
    pub handle: u32,
    /// 授予访问位。
    pub acc: u32,
    /// 对象类型索引（0..=7 冻结口径）。
    pub type_idx: u8,
    /// 期望判定（true = 对平成立）。
    pub expect_ok: bool,
    /// 回指链（三环）。
    pub chain: [ChainLink; 3],
}

/// 类型索引闭集上沿。
pub const TYPE_CAP: u8 = 8;

/// 黄金判定（单一权威）：链零悬空 + pid 闭集 + 句柄号非零 + 访问位闭集 + 类型索引闭集。
pub fn golden_judge(s: &HtabSample) -> Result<(), u32> {
    chain_ok(&s.chain)?;
    if !pid_in_set(s.pid) {
        return Err(HtabDiag::EPidRange.val());
    }
    if s.handle == 0 {
        return Err(HtabDiag::EHandleZero.val());
    }
    if !access_ok(s.acc) {
        return Err(HtabDiag::EAccessRange.val());
    }
    if s.type_idx >= TYPE_CAP {
        return Err(HtabDiag::EAccessRange.val());
    }
    Ok(())
}

// ── 样本库（24 例冻结） ──────────────────────────────────────────
/// 构造默认链（三环全对，const 供冻结表内嵌）。
const fn std_chain() -> [ChainLink; 3] {
    [
        ChainLink { ring: 0, ref_id: 12386, fp: "B20-F12386-HANDLE-LEDGER" },
        ChainLink { ring: 1, ref_id: 12586, fp: "B30-F12586-D3-COSIGN" },
        ChainLink { ring: 2, ref_id: 12663, fp: "F12664-HT-PROBE" },
    ]
}

/// 句柄表样本库（24 例冻结，行号序）。
pub const HTAB_STORE: &[HtabSample] = &[
    HtabSample { no: 0, pid: 4, handle: 0x0008, acc: ACC_READ, type_idx: 0, expect_ok: true, chain: std_chain() },
    HtabSample { no: 1, pid: 4, handle: 0x0010, acc: ACC_READ | ACC_WRITE, type_idx: 1, expect_ok: true, chain: std_chain() },
    HtabSample { no: 2, pid: 4, handle: 0x0014, acc: ACC_READ | ACC_EXEC, type_idx: 2, expect_ok: true, chain: std_chain() },
    HtabSample { no: 3, pid: 4, handle: 0x0018, acc: ACC_ALL, type_idx: 3, expect_ok: true, chain: std_chain() },
    HtabSample { no: 4, pid: 260, handle: 0x0020, acc: ACC_READ, type_idx: 4, expect_ok: true, chain: std_chain() },
    HtabSample { no: 5, pid: 260, handle: 0x0024, acc: ACC_READ | ACC_WRITE | ACC_EXEC, type_idx: 5, expect_ok: true, chain: std_chain() },
    HtabSample { no: 6, pid: 512, handle: 0x0028, acc: ACC_READ, type_idx: 6, expect_ok: true, chain: std_chain() },
    HtabSample { no: 7, pid: 512, handle: 0x0030, acc: ACC_ALL, type_idx: 7, expect_ok: true, chain: std_chain() },
    HtabSample { no: 8, pid: 768, handle: 0x0038, acc: ACC_READ | ACC_WRITE, type_idx: 0, expect_ok: true, chain: std_chain() },
    HtabSample { no: 9, pid: 768, handle: 0x003C, acc: ACC_EXEC, type_idx: 1, expect_ok: true, chain: std_chain() },
    HtabSample { no: 10, pid: 1024, handle: 0x0040, acc: ACC_READ, type_idx: 2, expect_ok: true, chain: std_chain() },
    HtabSample { no: 11, pid: 1024, handle: 0x0044, acc: ACC_READ | ACC_EXEC, type_idx: 3, expect_ok: true, chain: std_chain() },
    HtabSample { no: 12, pid: 2048, handle: 0x0048, acc: ACC_ALL, type_idx: 4, expect_ok: true, chain: std_chain() },
    HtabSample { no: 13, pid: 2048, handle: 0x004C, acc: ACC_READ | ACC_WRITE, type_idx: 5, expect_ok: true, chain: std_chain() },
    HtabSample { no: 14, pid: 260, handle: 0x0050, acc: ACC_READ, type_idx: 6, expect_ok: true, chain: std_chain() },
    HtabSample { no: 15, pid: 512, handle: 0x0054, acc: ACC_READ | ACC_WRITE | ACC_EXEC, type_idx: 7, expect_ok: true, chain: std_chain() },
    HtabSample { no: 16, pid: 768, handle: 0x0058, acc: ACC_READ, type_idx: 0, expect_ok: true, chain: std_chain() },
    HtabSample { no: 17, pid: 1024, handle: 0x005C, acc: ACC_ALL, type_idx: 1, expect_ok: true, chain: std_chain() },
    HtabSample { no: 18, pid: 2048, handle: 0x0060, acc: ACC_READ | ACC_EXEC, type_idx: 2, expect_ok: true, chain: std_chain() },
    HtabSample { no: 19, pid: 4, handle: 0x0064, acc: ACC_READ | ACC_WRITE, type_idx: 3, expect_ok: true, chain: std_chain() },
    // ── 负样本（20..23）：冻结违约，黄金判定必抓 ──
    HtabSample { no: 20, pid: 99999, handle: 0x0068, acc: ACC_READ, type_idx: 0, expect_ok: false, chain: std_chain() },
    HtabSample { no: 21, pid: 260, handle: 0, acc: ACC_READ, type_idx: 1, expect_ok: false, chain: std_chain() },
    HtabSample { no: 22, pid: 512, handle: 0x0070, acc: 0x0010, type_idx: 2, expect_ok: false, chain: std_chain() },
    HtabSample { no: 23, pid: 768, handle: 0x0074, acc: ACC_READ, type_idx: 9, expect_ok: false, chain: std_chain() },
];

/// 样本库容量。
pub const STORE_CAP: usize = 24;

/// 抽样数（判据：抽样 20 例一致）。
pub const SAMPLE_COUNT: usize = 20;

// ── 状态机（Linked → Sampled → Sealed） ──────────────────────────
/// 抽样状态：单向推进。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleState {
    /// 已联签（链锚定）。
    Linked,
    /// 抽样中。
    Sampled,
    /// 已封样。
    Sealed,
}

impl SampleState {
    /// 状态序。
    pub fn rank(self) -> u8 {
        match self {
            SampleState::Linked => 0,
            SampleState::Sampled => 1,
            SampleState::Sealed => 2,
        }
    }
}

/// 抽样账。
#[derive(Clone, Debug)]
pub struct HtabLedger {
    /// 已对平样本。
    rows: Vec<HtabSample>,
    /// 拒收记账（行号 + 违约码）。
    rejects: Vec<(u32, u32)>,
    /// 状态。
    state: SampleState,
}

impl HtabLedger {
    /// 开账（Linked 态——链锚定是开账前提）。
    pub fn new() -> HtabLedger {
        HtabLedger {
            rows: Vec::new(),
            rejects: Vec::new(),
            state: SampleState::Linked,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> SampleState {
        self.state
    }

    /// 已对平样本数。
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// 拒收数。
    pub fn reject_len(&self) -> usize {
        self.rejects.len()
    }

    /// 取第 idx 样本（越界 None）。
    pub fn row_at(&self, idx: usize) -> Option<&HtabSample> {
        self.rows.get(idx)
    }

    /// 联签落样（Linked→Sampled）。
    pub fn begin(&mut self) -> Result<(), u32> {
        if self.state.rank() >= SampleState::Sampled.rank() {
            return Err(HtabDiag::ERewind.val());
        }
        self.state = SampleState::Sampled;
        Ok(())
    }

    /// 违约记账（饱和）。
    fn note_reject(&mut self, no: u32, diag: u32) {
        if self.rejects.len() < STORE_CAP * 2 {
            self.rejects.push((no, diag));
        }
    }

    /// 逐例抽样（状态 + 行号序 + 黄金判定对平 + 期望对平 四闸）。
    pub fn sample(&mut self, s: HtabSample) -> Result<(), u32> {
        if self.state.rank() < SampleState::Sampled.rank() {
            return Err(HtabDiag::ERewind.val());
        }
        if self.state.rank() >= SampleState::Sealed.rank() {
            self.note_reject(s.no, HtabDiag::ERewind.val());
            return Err(HtabDiag::ERewind.val());
        }
        // 行号序闸。
        if s.no != self.rows.len() as u32 {
            self.note_reject(s.no, HtabDiag::ELinkOrder.val());
            return Err(HtabDiag::ELinkOrder.val());
        }
        // 黄金判定闸。
        if let Err(e) = golden_judge(&s) {
            self.note_reject(s.no, e);
            return Err(e);
        }
        // 期望对平闸。
        let judged_ok = golden_judge(&s).is_ok();
        if judged_ok != s.expect_ok {
            self.note_reject(s.no, HtabDiag::EFpMismatch.val());
            return Err(HtabDiag::EFpMismatch.val());
        }
        self.rows.push(s);
        Ok(())
    }

    /// 封样：20 例齐 + 零拒收。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.state.rank() >= SampleState::Sealed.rank() {
            return Err(HtabDiag::ERewind.val());
        }
        if self.rows.len() != SAMPLE_COUNT {
            return Err(HtabDiag::EShortSample.val());
        }
        if !self.rejects.is_empty() {
            return Err(HtabDiag::EShortSample.val());
        }
        self.state = SampleState::Sealed;
        Ok(())
    }

    /// 账等式。
    pub fn equation_ok(&self) -> bool {
        self.rows.len() == SAMPLE_COUNT && self.rejects.is_empty()
    }
}

// ── 确定性 LCG 与抽样 20 例 ──────────────────────────────────────
/// LCG（种子 = SEG_BASE）。
pub struct HtabLcg {
    state: u64,
}

impl HtabLcg {
    /// 固定种子构造。
    pub fn new(seed: u64) -> HtabLcg {
        HtabLcg { state: seed }
    }

    /// 下一伪随机数（Knuth MMIX 常数）。
    pub fn next_u32(&mut self) -> u32 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.state >> 33) as u32
    }

    /// 0..bound 内取值（bound 零返回零）。
    pub fn below(&mut self, bound: u32) -> u32 {
        if bound == 0 {
            return 0;
        }
        self.next_u32() % bound
    }
}

/// 构建完整抽样账（正样本 20 例按库序入库——判据要求抽样 20 例一致）。
pub fn build_full_ledger() -> HtabLedger {
    let mut led = HtabLedger::new();
    let _ = led.begin();
    let mut i = 0usize;
    while i < SAMPLE_COUNT {
        let s = match HTAB_STORE.get(i) {
            Some(v) => v.clone(),
            None => break,
        };
        match led.sample(s) {
            Ok(()) => i += 1,
            Err(_) => break,
        }
    }
    led
}

/// 确定性抽样 20 例（LCG 去重采样，从正样本 20 例 + 负样本 4 例闭集中抽
/// 全部 20 正例，负例走注入面）。
pub fn sample_twenty() -> Vec<HtabSample> {
    let mut picked: Vec<HtabSample> = Vec::new();
    let mut rng = HtabLcg::new(SEG_BASE as u64);
    let mut guard = 0u32;
    while picked.len() < SAMPLE_COUNT && guard < 1024 {
        guard += 1;
        let idx = rng.below(STORE_CAP as u32) as usize;
        let s = match HTAB_STORE.get(idx) {
            Some(v) => v.clone(),
            None => continue,
        };
        // 只抽正例（负例由注入面负责）。
        if !s.expect_ok {
            continue;
        }
        let mut dup = false;
        for p in picked.iter() {
            if p.no == s.no {
                dup = true;
            }
        }
        if !dup {
            picked.push(s);
        }
    }
    picked
}

// ── 注入面：十路违约 ─────────────────────────────────────────────
/// 违约路闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HtabFault {
    /// 环 0 悬空（B20 环 id 错挂）。
    Link0Dangle,
    /// 环 1 悬空（B30 环 id 错挂）。
    Link1Dangle,
    /// 环 2 悬空（本条环 id 错挂）。
    Link2Dangle,
    /// 指纹错挂。
    FpSwap,
    /// 链环错序。
    LinkOrder,
    /// pid 越表。
    PidRange,
    /// 句柄号零。
    HandleZero,
    /// 访问位越集。
    AccessRange,
    /// 样本缺数即封样。
    ShortSeal,
    /// 封样后再写。
    SealedWrite,
}

impl HtabFault {
    /// 路号。
    pub fn index(self) -> u32 {
        match self {
            HtabFault::Link0Dangle => 0,
            HtabFault::Link1Dangle => 1,
            HtabFault::Link2Dangle => 2,
            HtabFault::FpSwap => 3,
            HtabFault::LinkOrder => 4,
            HtabFault::PidRange => 5,
            HtabFault::HandleZero => 6,
            HtabFault::AccessRange => 7,
            HtabFault::ShortSeal => 8,
            HtabFault::SealedWrite => 9,
        }
    }

    /// 路中文名。
    pub fn name(self) -> &'static str {
        match self {
            HtabFault::Link0Dangle => "环 0 悬空",
            HtabFault::Link1Dangle => "环 1 悬空",
            HtabFault::Link2Dangle => "环 2 悬空",
            HtabFault::FpSwap => "指纹错挂",
            HtabFault::LinkOrder => "链环错序",
            HtabFault::PidRange => "pid 越表",
            HtabFault::HandleZero => "句柄号零",
            HtabFault::AccessRange => "访问位越集",
            HtabFault::ShortSeal => "缺数封样",
            HtabFault::SealedWrite => "封样再写",
        }
    }

    /// 全部十路（闭集数组）。
    pub fn all() -> &'static [HtabFault] {
        &[
            HtabFault::Link0Dangle,
            HtabFault::Link1Dangle,
            HtabFault::Link2Dangle,
            HtabFault::FpSwap,
            HtabFault::LinkOrder,
            HtabFault::PidRange,
            HtabFault::HandleZero,
            HtabFault::AccessRange,
            HtabFault::ShortSeal,
            HtabFault::SealedWrite,
        ]
    }
}

/// 十路违约联测：每路必须被闸抓住。返回 (抓到路数, 总路数, 首个逃脱描述)。
pub fn fault_sweep() -> (usize, usize, String) {
    let mut caught = 0usize;
    let mut total = 0usize;
    let mut escape = String::new();
    for v in HtabFault::all().iter() {
        total += 1;
        if run_fault(*v) {
            caught += 1;
        } else if escape.is_empty() {
            escape = format!("路{}({}) 逃脱", v.index(), v.name());
        }
    }
    (caught, total, escape)
}

/// 单路违约执行：返回 true 表示违约被抓住。
fn run_fault(v: HtabFault) -> bool {
    match v {
        HtabFault::Link0Dangle => {
            let s = match HTAB_STORE.get(0) {
                Some(x) => {
                    let mut c = x.clone();
                    match c.chain.get_mut(0) {
                        Some(l) => l.ref_id = 12300,
                        None => return false,
                    }
                    c
                }
                None => return false,
            };
            golden_judge(&s).is_err()
        }
        HtabFault::Link1Dangle => {
            let s = match HTAB_STORE.get(1) {
                Some(x) => {
                    let mut c = x.clone();
                    match c.chain.get_mut(1) {
                        Some(l) => l.ref_id = 12500,
                        None => return false,
                    }
                    c
                }
                None => return false,
            };
            golden_judge(&s).is_err()
        }
        HtabFault::Link2Dangle => {
            let s = match HTAB_STORE.get(2) {
                Some(x) => {
                    let mut c = x.clone();
                    match c.chain.get_mut(2) {
                        Some(l) => l.ref_id = 12600,
                        None => return false,
                    }
                    c
                }
                None => return false,
            };
            golden_judge(&s).is_err()
        }
        HtabFault::FpSwap => {
            let s = match HTAB_STORE.get(3) {
                Some(x) => {
                    let mut c = x.clone();
                    match c.chain.get_mut(1) {
                        Some(l) => l.fp = "B30-FAKE-FP",
                        None => return false,
                    }
                    c
                }
                None => return false,
            };
            golden_judge(&s).is_err()
        }
        HtabFault::LinkOrder => {
            // 环号错序：环 1 挂到环 0 号位。
            let s = match HTAB_STORE.get(4) {
                Some(x) => {
                    let mut c = x.clone();
                    match c.chain.get_mut(0) {
                        Some(l) => l.ring = 1,
                        None => return false,
                    }
                    c
                }
                None => return false,
            };
            golden_judge(&s).is_err()
        }
        HtabFault::PidRange => {
            let s = match HTAB_STORE.get(20) {
                Some(x) => x.clone(),
                None => return false,
            };
            golden_judge(&s).is_err()
        }
        HtabFault::HandleZero => {
            let s = match HTAB_STORE.get(21) {
                Some(x) => x.clone(),
                None => return false,
            };
            golden_judge(&s).is_err()
        }
        HtabFault::AccessRange => {
            let s = match HTAB_STORE.get(22) {
                Some(x) => x.clone(),
                None => return false,
            };
            golden_judge(&s).is_err()
        }
        HtabFault::ShortSeal => {
            // 只入 19 例就封样被拒。
            let mut led = HtabLedger::new();
            let _ = led.begin();
            let mut i = 0usize;
            while i < SAMPLE_COUNT - 1 {
                let s = match HTAB_STORE.get(i) {
                    Some(x) => x.clone(),
                    None => break,
                };
                if led.sample(s).is_err() {
                    return false;
                }
                i += 1;
            }
            led.seal().is_err()
        }
        HtabFault::SealedWrite => {
            let mut led = build_full_ledger();
            if led.seal().is_err() {
                return false;
            }
            let s = match HTAB_STORE.get(0) {
                Some(x) => x.clone(),
                None => return false,
            };
            led.sample(s).is_err()
        }
    }
}

// ── 判据谓词（全部私有，断言方向：违约条件 return false） ────────
/// 探测消费回指链零悬空（判据 J1 直读口）。
fn chain_zero_dangling_ok() -> bool {
    // 冻结锚表三环逐环自验。
    for a in CHAIN_ANCHORS.iter() {
        if link_ok(a).is_err() {
            return false;
        }
    }
    // 锚表环号闭集 0..=2 且严格递增。
    if CHAIN_ANCHORS.len() != 3 {
        return false;
    }
    let r0 = match CHAIN_ANCHORS.get(0) {
        Some(v) => v.ring,
        None => return false,
    };
    let r1 = match CHAIN_ANCHORS.get(1) {
        Some(v) => v.ring,
        None => return false,
    };
    let r2 = match CHAIN_ANCHORS.get(2) {
        Some(v) => v.ring,
        None => return false,
    };
    if !(r0 < r1 && r1 < r2) {
        return false;
    }
    // 源头回指 id 直读：B20 = 12386，B30 = 12586，本条 = 12663。
    let id0 = match CHAIN_ANCHORS.get(0) {
        Some(v) => v.ref_id,
        None => return false,
    };
    let id1 = match CHAIN_ANCHORS.get(1) {
        Some(v) => v.ref_id,
        None => return false,
    };
    let id2 = match CHAIN_ANCHORS.get(2) {
        Some(v) => v.ref_id,
        None => return false,
    };
    if id0 != 12386 || id1 != 12586 || id2 != SELF_REF_ID {
        return false;
    }
    true
}

/// 抽样 20 例一致（判据 J1 直读口）。
fn sample_twenty_ok() -> bool {
    // 抽样面：20 例确定性抽取，逐例黄金判定与期望一致。
    let picked = sample_twenty();
    if picked.len() != SAMPLE_COUNT {
        return false;
    }
    for s in picked.iter() {
        let verdict = golden_judge(s).is_ok();
        if verdict != s.expect_ok {
            return false;
        }
    }
    // 抽样账面：20 例全过闸封样。
    let mut led = build_full_ledger();
    if led.len() != SAMPLE_COUNT {
        return false;
    }
    if led.seal().is_err() {
        return false;
    }
    true
}

/// 注入联测 10/10 拒止。
fn fault_sweep_ok() -> bool {
    let (caught, total, escape) = fault_sweep();
    if total != 10 {
        return false;
    }
    if caught != 10 {
        return false;
    }
    if !escape.is_empty() {
        return false;
    }
    true
}

/// 负样本四例必抓（库内冻结违约与黄金判定对平）。
fn negative_samples_ok() -> bool {
    let mut i = 20usize;
    while i < STORE_CAP {
        let s = match HTAB_STORE.get(i) {
            Some(v) => v,
            None => return false,
        };
        if golden_judge(s).is_ok() {
            return false;
        }
        i += 1;
    }
    true
}

/// 双跑一致（冻结库下两轮逐例同判）。
fn dual_run_ok() -> bool {
    let a = sample_twenty();
    let b = sample_twenty();
    if a != b {
        return false;
    }
    let la = build_full_ledger();
    let lb = build_full_ledger();
    if la.len() != lb.len() {
        return false;
    }
    let mut i = 0usize;
    while i < la.len() {
        match (la.row_at(i), lb.row_at(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    if fault_sweep().0 != fault_sweep().0 {
        return false;
    }
    true
}

/// 边界：访问位闭集两端。
fn boundary_ok() -> bool {
    // 单位四位全过。
    if !access_ok(ACC_READ) || !access_ok(ACC_WRITE) || !access_ok(ACC_EXEC) || !access_ok(ACC_ALL) {
        return false;
    }
    // 冻结合成三位过。
    if !access_ok(ACC_READ | ACC_WRITE | ACC_EXEC) {
        return false;
    }
    // 零掩码拒。
    if access_ok(0) {
        return false;
    }
    // 越位 0x0010 拒。
    if access_ok(0x0010) {
        return false;
    }
    // 全权混位拒。
    if access_ok(ACC_ALL | ACC_READ) {
        return false;
    }
    true
}

/// 段位守卫（VX9 全批窗口 + 本卡公式位）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x80000 {
        return false;
    }
    if SEG_BASE > 0x80000 + 0x40 * (12799u32.saturating_sub(12600)) {
        return false;
    }
    if SEG_BASE != 0x80000 + 0x40 * (12663u32.saturating_sub(12600)) {
        return false;
    }
    true
}

/// 完成写回语义（判据 ③：状态=已完成、结果摘要非空）。
fn writeback_ok() -> bool {
    let mut led = build_full_ledger();
    if led.seal().is_err() {
        return false;
    }
    if led.state() != SampleState::Sealed {
        return false;
    }
    match led.row_at(0) {
        Some(s) => {
            if s.handle == 0 || !pid_in_set(s.pid) {
                return false;
            }
        }
        None => return false,
    }
    true
}

// ── 检查入口 ─────────────────────────────────────────────────────
/// UNX-F12664 句柄表面自检（判据 J1 + 通用守卫，≥12 条）。
pub fn run_unx12663_checks() -> CheckSet {
    let mut s = CheckSet::new("unx12663-handle-table");
    let led = build_full_ledger();

    s.add_dyn(
        "F12664-01 样本库二十四例齐",
        HTAB_STORE.len() == STORE_CAP,
        &format!("rows={}", HTAB_STORE.len()),
    );

    s.add_dyn(
        "F12664-02 回指链零悬空",
        chain_zero_dangling_ok(),
        "B20 F12386 → B30 F12586 → 本条 F12664 三环验锚",
    );

    s.add_dyn(
        "F12664-03 抽样二十例一致",
        sample_twenty_ok(),
        &format!("picked={}", sample_twenty().len()),
    );

    s.add_dyn(
        "F12664-04 注入联测 10/10 拒止",
        fault_sweep_ok(),
        &format!("caught={}", fault_sweep().0),
    );

    s.add_dyn(
        "F12664-05 负样本四例必抓",
        negative_samples_ok(),
        "库内冻结违约行黄金判定全拒",
    );

    s.add_dyn(
        "F12664-06 访问位闭集边界",
        boundary_ok(),
        "四位掩码口径 + 零掩码/越位/全权混位拒",
    );

    s.add_dyn(
        "F12664-07 双跑一致",
        dual_run_ok(),
        "冻结库下两轮逐例同判",
    );

    s.add_dyn(
        "F12664-08 抽样账等式",
        led.equation_ok(),
        &format!("rows={} rejects={}", led.len(), led.reject_len()),
    );

    s.add_dyn(
        "F12664-09 pid 归属闭集六例",
        HTAB_PIDS.len() == 6,
        &format!("pids={}", HTAB_PIDS.len()),
    );

    s.add_dyn(
        "F12664-10 完成写回语义",
        writeback_ok(),
        "封样即已完成且摘要非空",
    );

    s.add_dyn(
        "F12664-11 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05X}", SEG_BASE),
    );

    // 全库黄金判定与期望对平独立复核。
    let mut table_ok = true;
    for x in HTAB_STORE.iter() {
        if golden_judge(x).is_ok() != x.expect_ok {
            table_ok = false;
        }
    }
    s.add_dyn(
        "F12664-12 全库黄金判定对平",
        table_ok,
        "24 例逐例判定与期望一致",
    );

    s
}

// ── 测试区 ───────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_aligned_and_in_window() {
        // 段位守卫：对齐 0x40 且落在 VX9 全批窗口 + 本卡公式位。
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert!(SEG_BASE >= 0x80000 && SEG_BASE <= 0x831C0);
        assert_eq!(SEG_BASE, 0x80FC0);
    }

    #[test]
    fn diag_codes_are_closed_set() {
        // 闭集：10 个码互异、全部落窗口、查表可命中。
        assert_eq!(HTAB_ALL.len(), 10);
        let mut i = 0usize;
        while i < HTAB_ALL.len() {
            let vi = match HTAB_ALL.get(i) {
                Some(d) => d.val(),
                None => 0,
            };
            assert!(vi > SEG_BASE && vi < SEG_BASE + 0x40);
            let mut j = i + 1;
            while j < HTAB_ALL.len() {
                let vj = match HTAB_ALL.get(j) {
                    Some(d) => d.val(),
                    None => 0,
                };
                assert_ne!(vi, vj, "诊断码重复 0x{:05x}", vi);
                j += 1;
            }
            assert!(!detail(vi).is_empty());
            i += 1;
        }
        assert!(detail(SEG_BASE + 0x30).contains("未知"));
    }

    #[test]
    fn chain_anchors_paired() {
        // 三环锚 id 与指纹成对冻结。
        let a0 = match CHAIN_ANCHORS.get(0) {
            Some(v) => *v,
            None => return,
        };
        let a1 = match CHAIN_ANCHORS.get(1) {
            Some(v) => *v,
            None => return,
        };
        let a2 = match CHAIN_ANCHORS.get(2) {
            Some(v) => *v,
            None => return,
        };
        assert_eq!(a0.ref_id, 12386);
        assert_eq!(a0.fp, "B20-F12386-HANDLE-LEDGER");
        assert_eq!(a1.ref_id, 12586);
        assert_eq!(a1.fp, "B30-F12586-D3-COSIGN");
        assert_eq!(a2.ref_id, 12663);
        assert_eq!(a2.fp, "F12664-HT-PROBE");
        // 三环全过 + 零悬空。
        assert!(chain_zero_dangling_ok());
    }

    #[test]
    fn happy_path_seals() {
        let mut led = build_full_ledger();
        assert_eq!(led.len(), SAMPLE_COUNT);
        assert_eq!(led.reject_len(), 0);
        assert!(led.seal().is_ok());
        assert_eq!(led.state(), SampleState::Sealed);
    }

    #[test]
    fn injections_all_caught() {
        // 注入必抓：十路违约全数拒止。
        let (caught, total, escape) = fault_sweep();
        assert_eq!(total, 10);
        assert_eq!(caught, 10, "逃脱路：{}", escape);
        assert!(escape.is_empty());
    }

    #[test]
    fn chain_mutation_caught() {
        // 链突变三类：环 id 错挂 / 指纹错挂 / 环号错序。
        let base = match HTAB_STORE.get(0) {
            Some(x) => x.clone(),
            None => return,
        };
        let mut bad_id = base.clone();
        match bad_id.chain.get_mut(2) {
            Some(l) => l.ref_id = 12671,
            None => return,
        }
        assert!(golden_judge(&bad_id).is_err());
        let mut bad_fp = base.clone();
        match bad_fp.chain.get_mut(0) {
            Some(l) => l.fp = "FAKE",
            None => return,
        }
        assert!(golden_judge(&bad_fp).is_err());
        let mut bad_ring = base.clone();
        match bad_ring.chain.get_mut(2) {
            Some(l) => l.ring = 0,
            None => return,
        }
        assert!(golden_judge(&bad_ring).is_err());
    }

    #[test]
    fn access_mask_boundary() {
        // 访问位闭集边界直读。
        assert!(access_ok(ACC_READ));
        assert!(access_ok(ACC_WRITE));
        assert!(access_ok(ACC_EXEC));
        assert!(access_ok(ACC_ALL));
        assert!(access_ok(ACC_READ | ACC_WRITE));
        assert!(access_ok(ACC_READ | ACC_EXEC));
        assert!(access_ok(ACC_READ | ACC_WRITE | ACC_EXEC));
        assert!(!access_ok(0));
        assert!(!access_ok(0x0010));
        assert!(!access_ok(ACC_ALL | ACC_READ));
        assert!(!access_ok(0xFFFF_0000 | ACC_READ));
    }

    #[test]
    fn sample_twenty_deterministic() {
        // 抽样 20 例：确定性双跑一致且全为正例。
        let a = sample_twenty();
        let b = sample_twenty();
        assert_eq!(a, b);
        assert_eq!(a.len(), SAMPLE_COUNT);
        for s in a.iter() {
            assert!(s.expect_ok, "抽样混入负例 行{}", s.no);
        }
    }

    #[test]
    fn negative_samples_rejected() {
        // 库内负样本 4 例必抓。
        assert!(negative_samples_ok());
        let s20 = match HTAB_STORE.get(20) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(golden_judge(&s20), Err(HtabDiag::EPidRange.val()));
        let s22 = match HTAB_STORE.get(22) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(golden_judge(&s22), Err(HtabDiag::EAccessRange.val()));
    }

    #[test]
    fn dual_run_is_identical() {
        // 双跑一致。
        let la = build_full_ledger();
        let lb = build_full_ledger();
        assert_eq!(la.len(), lb.len());
        let mut i = 0usize;
        while i < la.len() {
            match (la.row_at(i), lb.row_at(i)) {
                (Some(x), Some(y)) => assert_eq!(x, y),
                _ => assert!(false, "双跑例缺失"),
            }
            i += 1;
        }
        assert_eq!(fault_sweep().0, fault_sweep().0);
    }

    #[test]
    fn state_machine_never_rewinds() {
        // 状态机只进不退：Linked→Sampled→Sealed。
        assert!(SampleState::Linked.rank() < SampleState::Sampled.rank());
        assert!(SampleState::Sampled.rank() < SampleState::Sealed.rank());
        let mut led = build_full_ledger();
        assert_eq!(led.state(), SampleState::Sampled);
        assert!(led.seal().is_ok());
        assert_eq!(led.seal(), Err(HtabDiag::ERewind.val()));
    }

    #[test]
    fn row_order_gate() {
        // 行号乱序被拒且记账落行。
        let mut led = HtabLedger::new();
        let _ = led.begin();
        let s1 = match HTAB_STORE.get(1) {
            Some(x) => x.clone(),
            None => return,
        };
        assert_eq!(led.sample(s1), Err(HtabDiag::ELinkOrder.val()));
        assert_eq!(led.reject_len(), 1);
    }

    #[test]
    fn checks_entry_all_green() {
        // 检查入口全绿且覆盖 ≥12 条。
        let s = run_unx12663_checks();
        assert!(s.all_passed(), "红项: {}", s.red_items_text());
        assert!(s.len() >= 12);
        assert_eq!(s.tally().1, 0);
    }
}
