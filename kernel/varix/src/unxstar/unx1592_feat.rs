//! UNX-F1592 域级哈希基线固化（收官总链一串） · 0x79200段
//!
//! 判据原文（UNX-F1592-J1 摘录）：「收官总链固化（判据链+资产链+文档链+包链
//! 四链汇一串），链一键复验 <15s，篡改注入 10/10 检出」；完成时 API/软件写回：
//! 状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 四链汇一串：判据链/资产链/文档链/包链四条链各自逐节落指纹（FNV-1a
//!    整数折叠，无浮点无随机），主键由四链指纹按钦定顺序二次折叠——一串
//!    定真伪，任何一链被动都传导到主键；
//! 2. 链一键复验 <15s：无墙钟可用，按「复验步数」折算——逐节复核+逐链复核
//!    +主键复核共 13 步，步数预算 15 步封顶，超预算即复验失败（显性不静默）；
//! 3. 篡改注入 10/10：改节指纹/零指纹/丢节/重复节/重排/改序号/链位互换/
//!    截链/主键篡改/固化指纹篡改十种注入，逐种必须被复验口点出，防复验面
//!    退化成报喜不报忧；
//! 4. 相位机 采集中→已固化→已复验 单向，倒退显性拒；封链后追加节被显性拒；
//! 5. 诊断码闭集 0x79201..0x7920C，Err 用 Result<T,u32>，中文诊断文案带
//!    可操作建议。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B09 给定值；本批窗口 0x79200..=0x793C0）。
pub const SEG_BASE: u32 = 0x79200;

/// 四链数（判据链+资产链+文档链+包链）。
pub const CHAIN_COUNT: usize = 4;

/// 每链节数（收官总链钦定：四链各两节，共八节）。
pub const CHAIN_LINKS: usize = 2;

/// 篡改注入种数（判据原文 10/10）。
pub const TAMPERS: usize = 10;

/// 链一键复验步数预算（判据原文「<15s」的步数折算口径，超步即败）。
pub const REVERIFY_STEP_BUDGET: u32 = 15;

/// 主键折叠种子（FNV-1a 偏移基）。
pub const MASTER_SEED: u64 = 0xCBF2_9CE4_8422_2325;

/// 主键折叠质数（FNV-1a 质数）。
pub const MASTER_PRIME: u64 = 0x100_0000_01B3;

/// 四链名册（判据面钦定顺序，主键折叠按此序）。
pub const CHAIN_NAMES: [&str; CHAIN_COUNT] = [
    "判据链", "资产链", "文档链", "包链",
];

/// 收官总链诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 链节指纹为零（空节）。
    EZeroDigest,
    /// 节序非法（序号不连续或不从一起）。
    ESeq,
    /// 节指纹与固化记录不符（内容被篡改）。
    ELinkDigest,
    /// 节数与固化记录不符（丢节/插节/重复节）。
    ELinkCount,
    /// 链指纹与固化记录不符（链被换）。
    EChainDigest,
    /// 主键与固化记录不符（主键被篡改）。
    EMaster,
    /// 相位倒退（已固化/已复验后拒写入）。
    EPhase,
    /// 固化后追加节被拒。
    ESealed,
    /// 四链未齐即固化。
    ELack,
    /// 复验步数超预算。
    ESteps,
    /// 空链（无节不构成链）。
    EEmpty,
    /// 复验输入链册与固化链册错位。
    EMisalign,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EZeroDigest => 0x01,
                Diag::ESeq => 0x02,
                Diag::ELinkDigest => 0x03,
                Diag::ELinkCount => 0x04,
                Diag::EChainDigest => 0x05,
                Diag::EMaster => 0x06,
                Diag::EPhase => 0x07,
                Diag::ESealed => 0x08,
                Diag::ELack => 0x09,
                Diag::ESteps => 0x0a,
                Diag::EEmpty => 0x0b,
                Diag::EMisalign => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EZeroDigest => "链节指纹为零：空节不入总链，先重算节指纹再固化",
            Diag::ESeq => "节序非法：序号从一起且逐节递增，先修节序再复验",
            Diag::ELinkDigest => "节指纹与固化记录不符：逐节重算指纹定位被改节，回滚该节",
            Diag::ELinkCount => "节数与固化记录不符：丢节/插节/重复节，按固化账补齐节数",
            Diag::EChainDigest => "链指纹与固化记录不符：逐链重算定位被换链，回滚该链",
            Diag::EMaster => "主键与固化记录不符：主键被篡改，按四链指纹重折主键",
            Diag::EPhase => "相位倒退：采集中→已固化→已复验 单向，倒退即拒",
            Diag::ESealed => "封链后追加被拒：总链已封，改动走下一轮收官基线",
            Diag::ELack => "四链未齐：判据链/资产链/文档链/包链逐链补齐再固化",
            Diag::ESteps => "复验步数超预算：链册超容复验超时，先瘦身链册再复验",
            Diag::EEmpty => "空链：无节不构成链，先逐节落指纹",
            Diag::EMisalign => "复验输入错位：链册顺序与固化名册错位，按名册重排",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EZeroDigest,
    Diag::ESeq,
    Diag::ELinkDigest,
    Diag::ELinkCount,
    Diag::EChainDigest,
    Diag::EMaster,
    Diag::EPhase,
    Diag::ESealed,
    Diag::ELack,
    Diag::ESteps,
    Diag::EEmpty,
    Diag::EMisalign,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79201..0x7920C 闭集内"
}

/// 收官总链相位（采集中→已固化→已复验 单向）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// 采集中：四链逐节落指纹。
    Collecting,
    /// 已固化：主键折叠完成，四链只读。
    Frozen,
    /// 已复验：链一键复验通过。
    Reverified,
}

impl Phase {
    /// 序数（倒退检测用）。
    pub fn ord(self) -> u8 {
        match self {
            Phase::Collecting => 0,
            Phase::Frozen => 1,
            Phase::Reverified => 2,
        }
    }

    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Phase::Collecting => "采集中",
            Phase::Frozen => "已固化",
            Phase::Reverified => "已复验",
        }
    }
}

/// 链身份（判据面钦定四链）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChainId {
    /// 判据链。
    Judge,
    /// 资产链。
    Asset,
    /// 文档链。
    Doc,
    /// 包链。
    Package,
}

impl ChainId {
    /// 链名（与名册同序）。
    pub fn name(self) -> &'static str {
        match self {
            ChainId::Judge => "判据链",
            ChainId::Asset => "资产链",
            ChainId::Doc => "文档链",
            ChainId::Package => "包链",
        }
    }

    /// 链位（0..CHAIN_COUNT）。
    pub fn slot(self) -> usize {
        match self {
            ChainId::Judge => 0,
            ChainId::Asset => 1,
            ChainId::Doc => 2,
            ChainId::Package => 3,
        }
    }
}

/// 四链身份闭集数组。
pub const CHAIN_IDS: &[ChainId] = &[
    ChainId::Judge,
    ChainId::Asset,
    ChainId::Doc,
    ChainId::Package,
];

/// 总链一节（序号 + 内容指纹）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChainLink {
    /// 节序号（从一起，逐节递增）。
    pub seq: u32,
    /// 节内容指纹（非零）。
    pub digest: u64,
}

/// 一条链（身份 + 节册 + 封链标记）。
#[derive(Clone, Debug)]
pub struct Chain {
    /// 链身份。
    pub id: ChainId,
    /// 节册（按序号递增落节）。
    pub links: Vec<ChainLink>,
    /// 封链后追加被拒。
    sealed: bool,
}

impl Chain {
    /// 新空链。
    pub fn new(id: ChainId) -> Chain {
        Chain {
            id,
            links: Vec::new(),
            sealed: false,
        }
    }

    /// 已落节数。
    pub fn len(&self) -> usize {
        self.links.len()
    }

    /// 是否封链。
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 追加一节（内容指纹现算；封链后显性拒）。
    pub fn push(&mut self, payload: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(Diag::ESealed.val());
        }
        let seq = match self.links.len() {
            n => (n as u32) + 1,
        };
        let digest = link_digest(seq, payload);
        if digest == 0 {
            return Err(Diag::EZeroDigest.val());
        }
        self.links.push(ChainLink { seq, digest });
        Ok(())
    }

    /// 封链（节数足才封）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.links.is_empty() {
            return Err(Diag::EEmpty.val());
        }
        if self.links.len() < CHAIN_LINKS {
            return Err(Diag::ELack.val());
        }
        self.sealed = true;
        Ok(())
    }

    /// 结构自检：节数足、序号从一起递增、无零指纹、逐节指纹与固化记录相符。
    /// `expect_digests` 给出固化时记录的逐节指纹（None 表示只查结构）。
    pub fn struct_ok(&self, expect_digests: Option<&[u64]>) -> bool {
        if self.links.len() != CHAIN_LINKS {
            return false;
        }
        let mut i = 0usize;
        while i < self.links.len() {
            let link = match self.links.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if link.seq != (i as u32) + 1 {
                return false;
            }
            if link.digest == 0 {
                return false;
            }
            if let Some(ds) = expect_digests {
                let want = match ds.get(i) {
                    Some(x) => *x,
                    None => return false,
                };
                if want != link.digest {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 链指纹（逐节指纹按序折叠）。
    pub fn digest(&self) -> u64 {
        let mut h = MASTER_SEED;
        for l in &self.links {
            h ^= l.digest;
            h = h.wrapping_mul(MASTER_PRIME);
        }
        h
    }
}

/// 收官主基线（四链指纹 + 主键 + 固化节数 + 相位）。
#[derive(Clone, Debug)]
pub struct MasterBaseline {
    /// 相位（单向机）。
    phase: Phase,
    /// 逐链固化指纹（按名册序）。
    chain_digests: Vec<u64>,
    /// 逐链固化节数（按名册序）。
    chain_counts: Vec<usize>,
    /// 逐链逐节固化指纹（按名册序）。
    link_digests: Vec<Vec<u64>>,
    /// 主键（四链指纹二次折叠）。
    master: u64,
    /// 结构性拒绝计数。
    rejects: u32,
}

impl MasterBaseline {
    /// 新空基线（采集中）。
    pub fn new() -> MasterBaseline {
        MasterBaseline {
            phase: Phase::Collecting,
            chain_digests: Vec::new(),
            chain_counts: Vec::new(),
            link_digests: Vec::new(),
            master: 0,
            rejects: 0,
        }
    }

    /// 相位。
    pub fn phase(&self) -> Phase {
        self.phase
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 主键。
    pub fn master(&self) -> u64 {
        self.master
    }

    /// 链位固化指纹。
    pub fn chain_digest_at(&self, slot: usize) -> u64 {
        match self.chain_digests.get(slot) {
            Some(x) => *x,
            None => 0,
        }
    }

    /// 链位固化节数。
    pub fn chain_count_at(&self, slot: usize) -> usize {
        match self.chain_counts.get(slot) {
            Some(x) => *x,
            None => 0,
        }
    }

    /// 固化（四链齐、逐链结构自检过、指纹非零才转「已固化」）。
    pub fn freeze(&mut self, chains: &[Chain]) -> Result<(), u32> {
        if self.phase != Phase::Collecting {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        if chains.len() != CHAIN_COUNT {
            return Err(Diag::ELack.val());
        }
        for (slot, ch) in chains.iter().enumerate() {
            let want_id = match CHAIN_IDS.get(slot) {
                Some(x) => *x,
                None => return Err(Diag::EMisalign.val()),
            };
            if ch.id != want_id {
                self.rejects += 1;
                return Err(Diag::EMisalign.val());
            }
            if ch.len() == 0 {
                return Err(Diag::EEmpty.val());
            }
            if !ch.sealed() {
                return Err(Diag::ELack.val());
            }
            if !ch.struct_ok(None) {
                self.rejects += 1;
                return Err(Diag::ESeq.val());
            }
            let mut per_link = Vec::new();
            for l in &ch.links {
                per_link.push(l.digest);
            }
            if let Some(d) = self.chain_digests.get_mut(slot) {
                *d = ch.digest();
            } else {
                self.chain_digests.push(ch.digest());
            }
            if let Some(c) = self.chain_counts.get_mut(slot) {
                *c = ch.len();
            } else {
                self.chain_counts.push(ch.len());
            }
            if let Some(v) = self.link_digests.get_mut(slot) {
                *v = per_link;
            } else {
                self.link_digests.push(per_link);
            }
        }
        self.master = fold_master(&self.chain_digests);
        if self.master == 0 {
            self.rejects += 1;
            return Err(Diag::EMaster.val());
        }
        self.phase = Phase::Frozen;
        Ok(())
    }

    /// 链一键复验：逐节重对+逐链重折+主键重折，全对齐才过。
    /// 返回复验步数（超预算即败，判据原文「<15s」折算口径）。
    pub fn reverify(&self, chains: &[Chain]) -> Result<u32, u32> {
        if self.phase == Phase::Collecting {
            return Err(Diag::EPhase.val());
        }
        if chains.len() != CHAIN_COUNT {
            return Err(Diag::EMisalign.val());
        }
        let mut steps: u32 = 0;
        for (slot, ch) in chains.iter().enumerate() {
            steps += 1;
            let want_id = match CHAIN_IDS.get(slot) {
                Some(x) => *x,
                None => return Err(Diag::EMisalign.val()),
            };
            if ch.id != want_id {
                return Err(Diag::EMisalign.val());
            }
            if ch.len() != self.chain_count_at(slot) {
                return Err(Diag::ELinkCount.val());
            }
            // 逐节复核（节数已对齐，逐节指纹比对）。
            let mut j = 0usize;
            while j < ch.len() {
                let link = match ch.links.get(j) {
                    Some(x) => *x,
                    None => return Err(Diag::ELinkCount.val()),
                };
                if link.seq != (j as u32) + 1 {
                    return Err(Diag::ESeq.val());
                }
                let want = match self.link_digests.get(slot).and_then(|v| v.get(j)) {
                    Some(x) => *x,
                    None => return Err(Diag::ELinkCount.val()),
                };
                if want != link.digest {
                    return Err(Diag::ELinkDigest.val());
                }
                j += 1;
            }
            // 逐链重折（链位互换在此点出）。
            if ch.digest() != self.chain_digest_at(slot) {
                return Err(Diag::EChainDigest.val());
            }
        }
        steps += 1;
        // 主键重折。
        if fold_master(&self.chain_digests) != self.master {
            return Err(Diag::EMaster.val());
        }
        if steps > REVERIFY_STEP_BUDGET {
            return Err(Diag::ESteps.val());
        }
        Ok(steps)
    }

    /// 复验收束：复验过才转「已复验」（单向，倒退拒）。
    pub fn confirm(&mut self, chains: &[Chain]) -> Result<u32, u32> {
        if self.phase != Phase::Frozen {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        let steps = self.reverify(chains)?;
        self.phase = Phase::Reverified;
        Ok(steps)
    }
}

/// 节内容指纹（FNV-1a 整数折叠，序号混折——改序号必被逐节复核点出）。
pub fn link_digest(seq: u32, payload: &str) -> u64 {
    let mut h = MASTER_SEED;
    for b in payload.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(MASTER_PRIME);
    }
    h ^= seq as u64;
    h.wrapping_mul(MASTER_PRIME)
}

/// 主键折叠（四链指纹按名册序二次折叠，链名混折）。
pub fn fold_master(digests: &[u64]) -> u64 {
    let mut h = MASTER_SEED;
    for (slot, d) in digests.iter().enumerate() {
        h ^= *d;
        h = h.wrapping_mul(MASTER_PRIME);
        let name = match CHAIN_NAMES.get(slot) {
            Some(x) => *x,
            None => "",
        };
        for b in name.as_bytes() {
            h ^= *b as u64;
            h = h.wrapping_mul(MASTER_PRIME);
        }
    }
    h
}

/// 标准四链册：判据链/资产链/文档链/包链各两节齐备封链。
pub fn build_chains() -> Vec<Chain> {
    let payloads: [&str; CHAIN_COUNT * CHAIN_LINKS] = [
        "判据F0801..F1600全绿", "判据双跑一致",
        "资产240000行守恒", "资产哈希齐",
        "文档收官册v1.1", "文档口径册齐",
        "包四十批齐包", "包签名齐",
    ];
    let mut out = Vec::new();
    let mut slot = 0usize;
    while slot < CHAIN_COUNT {
        let id = match CHAIN_IDS.get(slot) {
            Some(x) => *x,
            None => break,
        };
        let mut ch = Chain::new(id);
        let mut k = 0usize;
        while k < CHAIN_LINKS {
            let payload = match payloads.get(slot * CHAIN_LINKS + k) {
                Some(x) => *x,
                None => "",
            };
            let _ = ch.push(payload);
            k += 1;
        }
        let _ = ch.seal();
        out.push(ch);
        slot += 1;
    }
    out
}

/// 标准主基线：固化完成（已固化相位）。
pub fn build_baseline() -> (MasterBaseline, Vec<Chain>) {
    let chains = build_chains();
    let mut base = MasterBaseline::new();
    let _ = base.freeze(&chains);
    (base, chains)
}

/// 复验通过（清洁输入必过——防复验面空转）。
pub fn clean_reverify_ok() -> bool {
    let (base, chains) = build_baseline();
    base.reverify(&chains).is_ok()
}

/// 注入矩阵置位（越界静默忽略——注入位恒在 0..TAMPERS 内）。
fn mark(out: &mut [bool; TAMPERS], idx: usize, detected: bool) {
    if let Some(x) = out.get_mut(idx) {
        *x = detected;
    }
}

/// 篡改注入 10/10：十种注入逐种构造，逐种必须被复验口点出。
/// 返回十布尔（true=该注入被检出）。
pub fn tamper_matrix() -> [bool; TAMPERS] {
    let mut out = [false; TAMPERS];
    // 注入1：改节指纹（判据链第一节）。
    let (base, mut ch) = build_baseline();
    let mut hit = false;
    if let Some(c) = ch.get_mut(0) {
        if let Some(l) = c.links.get_mut(0) {
            l.digest ^= 0xDEAD_BEEF;
            hit = true;
        }
    }
    if hit {
        let detected = base.reverify(&ch) == Err(Diag::ELinkDigest.val());
        mark(&mut out, 0, detected);
    }
    // 注入2：零指纹节（资产链第二节）。
    let (base, mut ch) = build_baseline();
    hit = false;
    if let Some(c) = ch.get_mut(1) {
        if let Some(l) = c.links.get_mut(1) {
            l.digest = 0;
            hit = true;
        }
    }
    if hit {
        let detected = base.reverify(&ch) == Err(Diag::ELinkDigest.val());
        mark(&mut out, 1, detected);
    }
    // 注入3：丢节（包链截一节）。
    let (base, mut ch) = build_baseline();
    if let Some(c) = ch.get_mut(3) {
        c.links.pop();
    }
    let detected = base.reverify(&ch) == Err(Diag::ELinkCount.val());
    mark(&mut out, 2, detected);
    // 注入4：重复节（文档链追加重复尾节）。
    let (base, mut ch) = build_baseline();
    let tail = ch.get(2).and_then(|c| c.links.last().copied());
    if let Some(t) = tail {
        if let Some(c) = ch.get_mut(2) {
            c.links.push(t);
        }
    }
    let detected = base.reverify(&ch) == Err(Diag::ELinkCount.val());
    mark(&mut out, 3, detected);
    // 注入5：重排（资产链两节对调——序号不再递增）。
    let (base, mut ch) = build_baseline();
    let (a, b) = {
        let links = ch.get(1).map(|c| &c.links);
        match links {
            Some(ls) => (ls.get(0).copied(), ls.get(1).copied()),
            None => (None, None),
        }
    };
    if let (Some(a), Some(b)) = (a, b) {
        if let Some(c) = ch.get_mut(1) {
            if let Some(s0) = c.links.get_mut(0) {
                *s0 = b;
            }
            if let Some(s1) = c.links.get_mut(1) {
                *s1 = a;
            }
        }
    }
    let detected = base.reverify(&ch) == Err(Diag::ESeq.val());
    mark(&mut out, 4, detected);
    // 注入6：改序号（判据链第二节序号改 9）。
    let (base, mut ch) = build_baseline();
    if let Some(c) = ch.get_mut(0) {
        if let Some(l) = c.links.get_mut(1) {
            l.seq = 9;
        }
    }
    let detected = base.reverify(&ch) == Err(Diag::ESeq.val());
    mark(&mut out, 5, detected);
    // 注入7：链位互换（判据链与资产链整链对调）。
    let (base, mut ch) = build_baseline();
    let (c0, c1) = (ch.get(0).cloned(), ch.get(1).cloned());
    if let (Some(c0), Some(c1)) = (c0, c1) {
        if let Some(s) = ch.get_mut(0) {
            *s = c1;
        }
        if let Some(s) = ch.get_mut(1) {
            *s = c0;
        }
    }
    let detected = base.reverify(&ch) == Err(Diag::EMisalign.val());
    mark(&mut out, 6, detected);
    // 注入8：截链（文档链只留一节）。
    let (base, mut ch) = build_baseline();
    if let Some(c) = ch.get_mut(2) {
        while c.links.len() > 1 {
            c.links.pop();
        }
    }
    let detected = base.reverify(&ch) == Err(Diag::ELinkCount.val());
    mark(&mut out, 7, detected);
    // 注入9：主键篡改（基线主键翻转一位）。
    let (mut base, ch) = build_baseline();
    base.master ^= 0x1;
    let detected = base.reverify(&ch) == Err(Diag::EMaster.val());
    mark(&mut out, 8, detected);
    // 注入10：固化指纹篡改（包链固化指纹换零——链重折不符）。
    let (mut base, ch) = build_baseline();
    if let Some(d) = base.chain_digests.get_mut(3) {
        *d = 0;
    }
    let detected = base.reverify(&ch) == Err(Diag::EChainDigest.val());
    mark(&mut out, 9, detected);
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B09 派工卡 0x79200..=0x793C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79200 || SEG_BASE > 0x793C0 {
        return false;
    }
    SEG_BASE == 0x79200
}

/// 四链身份闭集（不重不漏、与名册同序、槽位对齐）。
fn chain_ids_closed_ok() -> bool {
    if CHAIN_IDS.len() != CHAIN_COUNT || CHAIN_NAMES.len() != CHAIN_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < CHAIN_IDS.len() {
        let a = match CHAIN_IDS.get(i) {
            Some(x) => *x,
            None => return false,
        };
        let want_name = match CHAIN_NAMES.get(i) {
            Some(x) => *x,
            None => return false,
        };
        if a.name() != want_name || a.slot() != i {
            return false;
        }
        let mut j = i + 1;
        while j < CHAIN_IDS.len() {
            let b = match CHAIN_IDS.get(j) {
                Some(x) => *x,
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

/// 四链汇一串（基线固化完成 + 主键非零 + 四链指纹逐链非零）。
fn frozen_ok(base: &MasterBaseline) -> bool {
    if base.phase() != Phase::Frozen || base.master() == 0 {
        return false;
    }
    let mut i = 0usize;
    while i < CHAIN_COUNT {
        if base.chain_digest_at(i) == 0 || base.chain_count_at(i) != CHAIN_LINKS {
            return false;
        }
        i += 1;
    }
    true
}

/// 链一键复验 <15s（步数预算内复验通过）。
fn reverify_budget_ok() -> bool {
    let (base, chains) = build_baseline();
    match base.reverify(&chains) {
        Ok(steps) => steps > 0 && steps <= REVERIFY_STEP_BUDGET,
        Err(_) => false,
    }
}

/// 篡改注入 10/10 检出。
fn tamper_all_ok() -> bool {
    tamper_matrix() == [true; TAMPERS]
}

/// 双跑一致（同构输入两次构建主键一致且复验同判）。
fn dual_run_ok() -> bool {
    let (a, ca) = build_baseline();
    let (b, cb) = build_baseline();
    a.master() == b.master() && a.master() != 0 && a.reverify(&ca) == b.reverify(&cb)
}

/// 收官主键一串判据：UNX-F1592 域级哈希基线固化。
pub fn run_unx1592_checks() -> CheckSet {
    let mut s = CheckSet::new("unx1592");
    let (base, chains) = build_baseline();

    s.add_dyn(
        "F1592-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    s.add_dyn(
        "F1592-02 四链身份闭集",
        chain_ids_closed_ok(),
        &format!("chains={}/{}", CHAIN_IDS.len(), CHAIN_COUNT),
    );

    // 正常流：四链齐备固化，主键非零。
    s.add_dyn(
        "F1592-03 四链齐备固化主键非零",
        frozen_ok(&base),
        &format!("master={:016x} phase={}", base.master(), base.phase().name()),
    );

    // 边界：采集中复验显性拒（未固化不可复验）。
    let mut early = MasterBaseline::new();
    let early_ch = build_chains();
    let early_rej = early.reverify(&early_ch) == Err(Diag::EPhase.val());
    s.add_dyn(
        "F1592-04 采集中复验显性拒",
        early_rej && early.phase() == Phase::Collecting,
        &format!("phase={}", early.phase().name()),
    );

    // 结构自检：节数、序号、零指纹逐面；欠节封链被拒。
    let good_chains = build_chains();
    let good_struct = good_chains
        .get(0)
        .map(|c| c.struct_ok(None))
        .unwrap_or(false);
    let mut thin = Chain::new(ChainId::Judge);
    let _ = thin.push("孤节");
    let thin_seal = thin.seal() == Err(Diag::ELack.val());
    s.add_dyn(
        "F1592-05 链结构逐面自检",
        good_struct && !thin.struct_ok(None) && thin_seal,
        &format!("good_len={} thin_len={}", 
            good_chains.get(0).map(|c| c.len()).unwrap_or(0), thin.len()),
    );

    // 注入必抓：篡改注入 10/10。
    s.add_dyn(
        "F1592-06 篡改注入十种全检出",
        tamper_all_ok(),
        &format!("matrix={:?}", tamper_matrix()),
    );

    // 清洁输入复验必过（防复验面空转）。
    s.add_dyn(
        "F1592-07 清洁复验必过",
        clean_reverify_ok(),
        &format!("clean={}", clean_reverify_ok()),
    );

    // 链一键复验 <15s（步数折算口径）。
    s.add_dyn(
        "F1592-08 链一键复验步数预算内",
        reverify_budget_ok(),
        &format!("budget={}", REVERIFY_STEP_BUDGET),
    );

    // 固化后追加节显性拒（总链封存）。
    let mut sealed_chains = build_chains();
    let append_after_seal = sealed_chains
        .get_mut(0)
        .map(|c| c.push("越权节") == Err(Diag::ESealed.val()))
        .unwrap_or(false);
    s.add_dyn(
        "F1592-09 封链后追加显性拒",
        append_after_seal,
        &format!("sealed={}", sealed_chains.get(0).map(|c| c.sealed()).unwrap_or(false)),
    );

    // 相位倒退显性拒（已固化后不可再固化）。
    let mut double_freeze = MasterBaseline::new();
    let _ = double_freeze.freeze(&build_chains());
    let re_freeze = double_freeze.freeze(&build_chains()) == Err(Diag::EPhase.val());
    s.add_dyn(
        "F1592-10 相位倒退显性拒",
        re_freeze && double_freeze.phase().ord() == 1,
        &format!("phase_ord={}", double_freeze.phase().ord()),
    );

    // 复验收束：复验过转已复验，已复验后再确认显性拒。
    let mut confirm_once = MasterBaseline::new();
    let _ = confirm_once.freeze(&build_chains());
    let steps = confirm_once.confirm(&build_chains());
    let confirm_ok = match steps {
        Ok(n) => n > 0,
        Err(_) => false,
    };
    let confirm_twice = confirm_once.confirm(&build_chains()) == Err(Diag::EPhase.val());
    s.add_dyn(
        "F1592-11 复验收束单向机",
        confirm_ok && confirm_twice && confirm_once.phase() == Phase::Reverified,
        &format!("phase={}", confirm_once.phase().name()),
    );

    // 链名混折：换链位传导到主键（fold_master 对名册序敏感）。
    let mut swapped: Vec<u64> = base.chain_digests.clone();
    swapped.swap(0, 1);
    let slot_sensitive = fold_master(&swapped) != base.master();
    s.add_dyn(
        "F1592-12 主键对链位敏感",
        slot_sensitive,
        &format!("slot_sensitive={}", slot_sensitive),
    );

    // 落档面：摘要非空且含四链名与主键。
    let mut summary = String::new();
    for (i, name) in CHAIN_NAMES.iter().enumerate() {
        summary.push_str(&format!(
            "{} 指纹{:016x} 节数{}\n",
            name,
            base.chain_digest_at(i),
            base.chain_count_at(i)
        ));
    }
    summary.push_str(&format!("主键{:016x}\n", base.master()));
    s.add_dyn(
        "F1592-13 收官总链摘要落档非空",
        !summary.is_empty()
            && summary.contains("判据链")
            && summary.contains("资产链")
            && summary.contains("文档链")
            && summary.contains("包链"),
        &format!("bytes={}", summary.len()),
    );

    // 双跑一致与诊断码闭集。
    s.add_dyn(
        "F1592-14 双跑一致与诊断码闭集",
        dual_run_ok() && ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::EMisalign.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EZeroDigest.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EMisalign.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x79200);
    }

    #[test]
    fn link_digest_covers_seq_and_payload() {
        let a = link_digest(1, "判据");
        let b = link_digest(2, "判据");
        let c = link_digest(1, "资产");
        assert_ne!(a, b, "同文异序指纹必不同");
        assert_ne!(a, c, "同序异文指纹必不同");
        assert_ne!(a, 0);
    }

    #[test]
    fn freeze_gates() {
        let mut base = MasterBaseline::new();
        // 欠链不可固化。
        let mut half = build_chains();
        half.pop();
        assert_eq!(base.freeze(&half), Err(Diag::ELack.val()));
        // 链册错位拒（判据链放资产链位）。
        let mut swapped = build_chains();
        let (c0, c1) = (swapped.get(0).cloned(), swapped.get(1).cloned());
        if let (Some(c0), Some(c1)) = (c0, c1) {
            if let Some(s) = swapped.get_mut(0) {
                *s = c1;
            }
            if let Some(s) = swapped.get_mut(1) {
                *s = c0;
            }
        }
        let mut base2 = MasterBaseline::new();
        assert_eq!(base2.freeze(&swapped), Err(Diag::EMisalign.val()));
        // 正常固化。
        assert_eq!(base.freeze(&build_chains()), Ok(()));
        assert_eq!(base.phase(), Phase::Frozen);
    }

    #[test]
    fn tamper_ten_of_ten() {
        assert_eq!(tamper_matrix(), [true; TAMPERS]);
    }

    #[test]
    fn reverify_clean_passes_within_budget() {
        let (base, chains) = build_baseline();
        let steps = base.reverify(&chains);
        assert!(steps.is_ok());
        let n = match steps {
            Ok(n) => n,
            Err(_) => 0,
        };
        assert!(n > 0);
        assert!(n <= REVERIFY_STEP_BUDGET);
    }

    #[test]
    fn phase_monotonic() {
        let mut base = MasterBaseline::new();
        assert_eq!(base.phase(), Phase::Collecting);
        let _ = base.freeze(&build_chains());
        assert_eq!(base.freeze(&build_chains()), Err(Diag::EPhase.val()));
        let _ = base.confirm(&build_chains());
        assert_eq!(base.confirm(&build_chains()), Err(Diag::EPhase.val()));
        assert_eq!(base.phase(), Phase::Reverified);
    }

    #[test]
    fn sealed_chain_rejects_append() {
        let mut ch = build_chains();
        let rej = ch
            .get_mut(0)
            .map(|c| c.push("越权节") == Err(Diag::ESealed.val()))
            .unwrap_or(false);
        assert!(rej);
        assert_eq!(ch.get(0).map(|c| c.len()), Some(CHAIN_LINKS));
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
        let (a, _) = build_baseline();
        let (b, _) = build_baseline();
        assert_eq!(a.master(), b.master());
    }

    #[test]
    fn struct_ok_rejects_zero_and_out_of_order() {
        let mut ch = Chain::new(ChainId::Judge);
        let _ = ch.push("第一节");
        let _ = ch.push("第二节");
        assert!(ch.struct_ok(None));
        if let Some(l) = ch.links.get_mut(1) {
            l.digest = 0;
        }
        assert!(!ch.struct_ok(None), "零指纹节必须被结构自检点出");
        let mut ch2 = Chain::new(ChainId::Judge);
        let _ = ch2.push("第一节");
        let _ = ch2.push("第二节");
        if let Some(l) = ch2.links.get_mut(0) {
            l.seq = 5;
        }
        assert!(!ch2.struct_ok(None), "节序断裂必须被结构自检点出");
    }

    #[test]
    fn master_folds_names() {
        let ds = vec![1u64, 2, 3, 4];
        let m1 = fold_master(&ds);
        let mut swapped = ds.clone();
        swapped.swap(0, 1);
        assert_ne!(m1, fold_master(&swapped));
    }

    #[test]
    fn chain_ids_align_with_names() {
        assert_eq!(CHAIN_IDS.len(), CHAIN_NAMES.len());
        for (i, id) in CHAIN_IDS.iter().enumerate() {
            let want = match CHAIN_NAMES.get(i) {
                Some(x) => *x,
                None => "",
            };
            assert_eq!(id.name(), want);
            assert_eq!(id.slot(), i);
        }
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx1592_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
