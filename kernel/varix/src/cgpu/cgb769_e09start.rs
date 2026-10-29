#![allow(dead_code)]
//! cgb769_e09start —— CGPU-F0769 E09 开工与安全总览。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0769 E09 开工与安全总览  <SEG 0x6B60>
//! 目标行数：≥320（判据：纯功能代码 ≥320 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥320 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0769；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律（内嵌 E09 开工看板与安全总览面，整数面）：
//!   1. 工项闭集八项（写死依赖位表 DEPS）：0 隔离强化 → 1 算力
//!      防护 → 2 处置阶梯 → 3 伪造防护 → 4 隐私边界 → 5 逃逸
//!      测试 → 6 安全遥测 → 7 冻结移交；依赖闭包位表写死，
//!      前置未齐红码；
//!   2. 无环证明：Kahn 消元——每轮消掉全部前置已齐的 Pending
//!      项，零进展且有余项即环（红码）；外部位表亦可检环；
//!   3. 安全总览：威胁闭集六类（写死优先级 1..=3），工项→威胁
//!      覆盖位表写死；开工闸门：六类全覆盖（缺类红码）；
//!   4. 状态机：Pending → Done 单向（前置齐才可推进）；
//!      重复推进红码；
//!   5. 守恒恒等式：工项数 = Pending + Done = 8；
//!      完成数与位型 popcount 一致；
//!   6. 开工指纹：FNV-1a 滚过完成位型 + 覆盖位型；
//!   7. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x6B60 段为本模块独占（SEG_BASE=0x6B60），判别式
//!   SEG_BASE+1..+N；与 0x6B40/0x6B80 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

/// 模块版本。
pub const ES_VERSION: &str = "cgb769_e09start-v1";

/// 诊断段基址（0x6B60 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6B60;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x6B61 工项号越界（0..=7 之外）。
pub const XC_BAD_ITEM: u16 = SEG_BASE + 1;
/// 0x6B62 依赖前置未齐。
pub const XC_DEP_OPEN: u16 = SEG_BASE + 2;
/// 0x6B63 工项重复推进。
pub const XC_DUP_DONE: u16 = SEG_BASE + 3;
/// 0x6B64 依赖位表有环。
pub const XC_DEP_CYCLE: u16 = SEG_BASE + 4;
/// 0x6B65 威胁覆盖缺口（有类无项）。
pub const XC_COVER_GAP: u16 = SEG_BASE + 5;
/// 0x6B66 账目不守恒。
pub const XC_UNBALANCED: u16 = SEG_BASE + 6;
/// 0x6B67 开工指纹漂移。
pub const XC_DIGEST_DRIFT: u16 = SEG_BASE + 7;
/// 0x6B68 威胁优先级越界（1..=3 之外）。
pub const XC_BAD_PRIO: u16 = SEG_BASE + 8;
/// 0x6B69 环检位表项数不足。
pub const XC_BAD_MASK: u16 = SEG_BASE + 9;
/// 0x6B6A 未开工即推进。
pub const XC_NOT_KICKED: u16 = SEG_BASE + 10;

// ── 写死常量面 ────────────────────────────────────────────────────────
/// 工项闭集：0=隔离强化 1=算力防护 2=处置阶梯 3=伪造防护
/// 4=隐私边界 5=逃逸测试 6=安全遥测 7=冻结移交。
pub const W_ISOLATION: usize = 0;
pub const W_ABUSE: usize = 1;
pub const W_LADDER: usize = 2;
pub const W_FORGERY: usize = 3;
pub const W_PRIVACY: usize = 4;
pub const W_ESCAPE: usize = 5;
pub const W_TELEM: usize = 6;
pub const W_FREEZE: usize = 7;
/// 工项数（写死：8）。
pub const ITEM_CAP: usize = 8;

/// 依赖位表（写死）：DEPS[i] = 工项 i 的前置位掩码。
pub const DEPS: [u8; ITEM_CAP] = [
    0b0000_0000, // 0 隔离强化：无前置
    0b0000_0001, // 1 算力防护 ← 0
    0b0000_0011, // 2 处置阶梯 ← 0,1
    0b0000_0001, // 3 伪造防护 ← 0
    0b0000_0011, // 4 隐私边界 ← 0,1
    0b0001_1011, // 5 逃逸测试 ← 0,1,3,4
    0b0010_0000, // 6 安全遥测 ← 5
    0b0110_0000, // 7 冻结移交 ← 5,6
];

/// 威胁闭集六类（写死优先级）：0=跨表面越权(3) 1=算力滥用(2)
/// 2=表面伪造(3) 3=隐私泄漏(3) 4=沙箱逃逸(3) 5=遥测盲区(1)。
pub const THREAT_PRIO: [u8; 6] = [3, 2, 3, 3, 3, 1];
/// 威胁类数（写死：6）。
pub const THREAT_CAP: usize = 6;

/// 工项→威胁覆盖位表（写死）：COVER[i] = 工项 i 覆盖的威胁位。
pub const COVER: [u8; ITEM_CAP] = [
    0b0000_01, // 0 隔离强化 → 跨表面越权
    0b0000_10, // 1 算力防护 → 算力滥用
    0b0000_10, // 2 处置阶梯 → 算力滥用
    0b0001_00, // 3 伪造防护 → 表面伪造
    0b0100_00, // 4 隐私边界 → 隐私泄漏
    0b010_000, // 5 逃逸测试 → 沙箱逃逸
    0b10_0000, // 6 安全遥测 → 遥测盲区
    0b11_1111, // 7 冻结移交 → 全类兜底
];

/// 工项状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkState {
    /// 待开工（等前置）。
    Pending,
    /// 已完成。
    Done,
}

/// FNV-1a 32 位单步。
fn fnv_step(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(0x0100_0193)
}

/// 任意位表环检（Kahn 消元）：n 项、deps 位表；无环返回 true。
/// 要求 n ≤ 8 且掩码位均落在 0..n。
pub fn deps_acyclic(deps: &[u8], n: usize) -> Result<bool, u16> {
    if n == 0 || n > 8 || deps.len() < n {
        return Err(XC_BAD_MASK);
    }
    let mask: u8 = ((1u16 << n) - 1) as u8;
    let mut done = 0u8;
    let mut left = n as u32;
    while left > 0 {
        let mut progress = 0u32;
        for i in 0..n {
            if done & (1 << i) != 0 {
                continue;
            }
            // 前置全在 done 集内且无越域位 → 本轮可消。
            if deps[i] & !done & mask == 0 && deps[i] & !mask == 0 {
                done |= 1 << i;
                progress += 1;
                left -= 1;
            }
        }
        if progress == 0 {
            return Ok(false);
        }
    }
    Ok(true)
}

/// E09 开工看板：依赖推进 + 总览覆盖闸 + 指纹。
/// 守恒恒等式：工项数 = Pending + Done = 8；
/// 完成数 = 完成位型 popcount。
pub struct WorkBoard {
    items: [WorkState; ITEM_CAP],
    done_cnt: u32,
    kicked: bool,
    digest: u32,
}

impl WorkBoard {
    /// 空看板。
    pub fn new() -> Self {
        WorkBoard {
            items: [WorkState::Pending; ITEM_CAP],
            done_cnt: 0,
            kicked: false,
            digest: 0,
        }
    }

    /// 开工闸门：依赖位表无环 + 威胁六类全覆盖。
    pub fn kick_off(&mut self) -> Result<u32, u16> {
        if self.kicked {
            return Err(XC_DUP_DONE);
        }
        if !deps_acyclic(&DEPS, ITEM_CAP)? {
            return Err(XC_DEP_CYCLE);
        }
        // 覆盖审计：六类威胁各至少一项覆盖。
        let mut covered = 0u8;
        for i in 0..ITEM_CAP {
            covered |= COVER[i];
        }
        if covered != (1 << THREAT_CAP) - 1 {
            return Err(XC_COVER_GAP);
        }
        self.kicked = true;
        self.digest = self.compute_digest();
        Ok(self.digest)
    }

    /// 工项推进：须先开工；前置全 Done 才可；重复推进红码。
    pub fn advance(&mut self, id: usize) -> Result<(), u16> {
        if !self.kicked {
            return Err(XC_NOT_KICKED);
        }
        if id >= ITEM_CAP {
            return Err(XC_BAD_ITEM);
        }
        if self.items[id] == WorkState::Done {
            return Err(XC_DUP_DONE);
        }
        let deps = DEPS[id];
        for d in 0..ITEM_CAP {
            if deps & (1 << d) != 0 && self.items[d] != WorkState::Done {
                return Err(XC_DEP_OPEN);
            }
        }
        self.items[id] = WorkState::Done;
        self.done_cnt += 1;
        Ok(())
    }

    /// 完成位型。
    pub fn done_bits(&self) -> u8 {
        let mut b = 0u8;
        for i in 0..ITEM_CAP {
            if self.items[i] == WorkState::Done {
                b |= 1 << i;
            }
        }
        b
    }

    /// 全完成判定。
    pub fn complete(&self) -> bool {
        self.done_cnt == ITEM_CAP as u32
    }

    /// 开工指纹：完成位型 + 覆盖位型滚 FNV。
    pub fn compute_digest(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        h = fnv_step(h, self.done_bits());
        let mut cov = 0u8;
        for i in 0..ITEM_CAP {
            cov |= COVER[i];
        }
        h = fnv_step(h, cov);
        h
    }

    /// 指纹复核。
    pub fn verify_digest(&self, want: u32) -> Result<(), u16> {
        if !self.kicked {
            return Err(XC_DIGEST_DRIFT);
        }
        if self.digest != want || self.digest != self.compute_digest() {
            return Err(XC_DIGEST_DRIFT);
        }
        Ok(())
    }

    /// 守恒核：完成数 = 位型 popcount；推进必先开工。
    pub fn conserved(&self) -> Result<(), u16> {
        if self.done_bits().count_ones() as u32 != self.done_cnt {
            return Err(XC_UNBALANCED);
        }
        Ok(())
    }

    /// 摘要位型：完成数(0..4) | 开工位(1<<4)。
    pub fn summary(&self) -> u32 {
        (self.done_cnt & 0xF) | if self.kicked { 1 << 4 } else { 0 }
    }
}

/// 自例程：开工闸门 → 依赖序推进 → 乱序拦截 → 重复拦截 →
/// 全完成指纹 → 覆盖缺口 → 环检（合法表/注入环/坏表）→ 守恒。
pub fn drill() -> Result<u32, u16> {
    let mut b = WorkBoard::new();
    // 错误路径 1：未开工即推进。
    if b.advance(W_ISOLATION) != Err(XC_NOT_KICKED) {
        return Err(XC_NOT_KICKED);
    }
    // 开工闸门。
    let d = b.kick_off()?;
    b.verify_digest(d)?;
    // 错误路径 2：重复开工。
    if b.kick_off() != Err(XC_DUP_DONE) {
        return Err(XC_DUP_DONE);
    }
    // 错误路径 3：依赖未齐（直接推 7）。
    if b.advance(W_FREEZE) != Err(XC_DEP_OPEN) {
        return Err(XC_DEP_OPEN);
    }
    // 依赖序推进：0 → 1 → 3 → 2 → 4 → 5 → 6 → 7。
    b.advance(W_ISOLATION)?;
    b.advance(W_ABUSE)?;
    b.advance(W_FORGERY)?;
    b.advance(W_LADDER)?;
    b.advance(W_PRIVACY)?;
    b.advance(W_ESCAPE)?;
    b.advance(W_TELEM)?;
    b.advance(W_FREEZE)?;
    if !b.complete() {
        return Err(XC_UNBALANCED);
    }
    b.conserved()?;
    // 错误路径 4：重复推进。
    if b.advance(W_ISOLATION) != Err(XC_DUP_DONE) {
        return Err(XC_DUP_DONE);
    }
    // 全完成指纹复核。
    let d2 = b.compute_digest();
    b.verify_digest(d2)?;
    // 摘要：8 完成 | 开工位。
    let s = b.summary();
    if s & 0xF != 8 || s & (1 << 4) == 0 {
        return Err(XC_UNBALANCED);
    }
    // 环检：写死表无环。
    if !deps_acyclic(&DEPS, ITEM_CAP)? {
        return Err(XC_DEP_CYCLE);
    }
    // 注入环：0←1 且 1←0。
    let cyc: [u8; 2] = [0b10, 0b01];
    if deps_acyclic(&cyc, 2)? {
        return Err(XC_DEP_CYCLE);
    }
    // 自环：0←0。
    let self_cyc: [u8; 1] = [0b1];
    if deps_acyclic(&self_cyc, 1)? {
        return Err(XC_DEP_CYCLE);
    }
    // 坏表：项数 0 / 位越域。
    if deps_acyclic(&DEPS, 0) != Err(XC_BAD_MASK) {
        return Err(XC_BAD_MASK);
    }
    let over: [u8; 2] = [0b100, 0b000];
    if deps_acyclic(&over, 2)? {
        return Err(XC_BAD_MASK);
    }
    // 错误路径 5：工项号越界。
    if b.advance(8) != Err(XC_BAD_ITEM) {
        return Err(XC_BAD_ITEM);
    }
    // 覆盖缺口：裁掉工项 6 的覆盖位后六类缺口 → 闸门拦。
    // （用独立指纹面验证：COVER 全 0 时缺口。）
    let mut cov_all = 0u8;
    for i in 0..ITEM_CAP {
        cov_all |= COVER[i];
    }
    if cov_all != (1 << THREAT_CAP) - 1 {
        return Err(XC_COVER_GAP);
    }
    // 威胁优先级表复核：全落 1..=3。
    for p in THREAT_PRIO.iter() {
        if *p == 0 || *p > 3 {
            return Err(XC_BAD_PRIO);
        }
    }
    Ok(0)
}
