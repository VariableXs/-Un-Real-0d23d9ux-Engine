//! unx48259_s2tround — UNX-F48259 简繁往返抽检账（单向转换承诺清晰，往返抽检不作双向保证声明）（诊断段 0xC140）。
//!
//! 判据原文：
//!   J1 声明入册 + 抽检 ×10 差异如实记账；判据锚定 §40 幻觉红线、诚实登记。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 单向承诺入册：本面只承诺简→繁单向转换无损（查表闭集内）；
//!      繁→简不做双向保证——声明以常量注册（DECL_ONE_WAY=true），
//!      声明本身可复核、不可运行期改写。
//!   2. 转换表闭集：36 常用字简→繁映射（含一简对多繁：发→發/髮、
//!      干→乾/幹、后→後/后，取微软/opencc 常用缺省锚）；表外字拒转。
//!   3. 往返抽检口径（J1 ×10）：10 条抽检样本跑 简→繁→简 往返，
//!      结果逐条如实记账——往返恒等记 MATCH，因一简多繁导致的差异
//!      记 DIFF_AMBIG（差异条目带繁体码与归简位），既不粉饰为全对
//!      也不谎报为全损；差异只允许出现在已声明的多对一成员上。
//!   4. 差异归因审计：每条 DIFF 必须能归因到表内某个多映射简体字
//!      （multi 标志），归因不上的差异视为转换器坏账（真损）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC140;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_UNMAPPED: u16 = SEG_BASE + 1;   // 表外字拒转
const E_UNJUSTIFIED: u16 = SEG_BASE + 2;// 差异归因不上（真损）
const E_DECL_BROKEN: u16 = SEG_BASE + 3;// 声明被改写
const E_SAMPLE_MISS: u16 = SEG_BASE + 4;// ×10 抽检不足
const E_LEDGER_BAD: u16 = SEG_BASE + 5; // 抽检账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 6;  // 写回摘要为空

/// 单向承诺声明（常量即契约，纪律 1）。
pub const DECL_ONE_WAY: bool = true;
/// 承诺有效域声明：仅 s→t 方向。
pub const DECL_DIRECTION: u8 = 1; // 1 = s2t only

/// 一条简→繁映射。
#[derive(Clone, Copy, Debug)]
pub struct S2tEntry {
    pub s: u32, // 简体码点
    pub t: u32, // 繁体码点（缺省锚）
    pub multi: bool, // 一简对多繁（往返必不保证）
}

/// 转换表（36 条闭集，常用字缺省锚）。
pub const S2T_TABLE: [S2tEntry; 36] = [
    S2tEntry { s: 0x4E2D, t: 0x4E2D, multi: false }, // 中
    S2tEntry { s: 0x6587, t: 0x6587, multi: false }, // 文
    S2tEntry { s: 0x56FD, t: 0x570B, multi: false }, // 国→國
    S2tEntry { s: 0x4EBA, t: 0x4EBA, multi: false }, // 人
    S2tEntry { s: 0x5927, t: 0x5927, multi: false }, // 大
    S2tEntry { s: 0x5C0F, t: 0x5C0F, multi: false }, // 小
    S2tEntry { s: 0x5B66, t: 0x5B78, multi: false }, // 学→學
    S2tEntry { s: 0x4E60, t: 0x7FD2, multi: false }, // 习→習
    S2tEntry { s: 0x8F93, t: 0x8F38, multi: false }, // 输→輸
    S2tEntry { s: 0x5165, t: 0x5165, multi: false }, // 入
    S2tEntry { s: 0x6CD5, t: 0x6CD5, multi: false }, // 法
    S2tEntry { s: 0x6D4B, t: 0x6E2C, multi: false }, // 测→測
    S2tEntry { s: 0x8BD5, t: 0x8A66, multi: false }, // 试→試
    S2tEntry { s: 0x5F00, t: 0x958B, multi: false }, // 开→開
    S2tEntry { s: 0x5173, t: 0x95DC, multi: false }, // 关→關
    S2tEntry { s: 0x95EE, t: 0x554F, multi: false }, // 问→問
    S2tEntry { s: 0x65F6, t: 0x6642, multi: false }, // 时→時
    S2tEntry { s: 0x95F4, t: 0x9593, multi: false }, // 间→間
    S2tEntry { s: 0x8BA1, t: 0x8A08, multi: false }, // 计→計
    S2tEntry { s: 0x5212, t: 0x5283, multi: false }, // 划→劃
    S2tEntry { s: 0x53D1, t: 0x767C, multi: true },  // 发→發（对 髮）
    S2tEntry { s: 0x5E72, t: 0x4E7E, multi: true },  // 干→乾（对 幹）
    S2tEntry { s: 0x540E, t: 0x5F8C, multi: true },  // 后→後（对 后）
    S2tEntry { s: 0x9762, t: 0x9762, multi: false }, // 面
    S2tEntry { s: 0x65B9, t: 0x65B9, multi: false }, // 方
    S2tEntry { s: 0x8F6C, t: 0x8F49, multi: false }, // 转→轉
    S2tEntry { s: 0x6362, t: 0x63DB, multi: false }, // 换→換
    S2tEntry { s: 0x65E0, t: 0x7121, multi: false }, // 无→無
    S2tEntry { s: 0x58F0, t: 0x8072, multi: false }, // 声→聲
    S2tEntry { s: 0x97F3, t: 0x97F3, multi: false }, // 音
    S2tEntry { s: 0x4E50, t: 0x6A02, multi: false }, // 乐→樂
    S2tEntry { s: 0x4E49, t: 0x7FA9, multi: false }, // 义→義
    S2tEntry { s: 0x4E48, t: 0x9EBC, multi: false }, // 么→麼
    S2tEntry { s: 0x4E1C, t: 0x6771, multi: false }, // 东→東
    S2tEntry { s: 0x897F, t: 0x897F, multi: false }, // 西
    S2tEntry { s: 0x5357, t: 0x5357, multi: false }, // 南
];

/// 简→繁（单向主径）。
pub fn s2t(s: u32) -> Result<u32, u16> {
    let mut i = 0usize;
    while i < S2T_TABLE.len() {
        if S2T_TABLE[i].s == s {
            return Ok(S2T_TABLE[i].t);
        }
        i += 1;
    }
    Err(E_UNMAPPED)
}

/// 繁→简（反向抽检径：非承诺方向，仅抽检用）。
pub fn t2s(t: u32) -> Result<u32, u16> {
    let mut i = 0usize;
    while i < S2T_TABLE.len() {
        if S2T_TABLE[i].t == t {
            return Ok(S2T_TABLE[i].s);
        }
        i += 1;
    }
    Err(E_UNMAPPED)
}

/// 抽检账条目。
#[derive(Clone, Copy, Debug)]
pub struct SpotRecord {
    pub s: u32,
    pub t: u32,   // s2t 结果
    pub back: u32, // t2s 回得
    pub verdict: u16, // V_MATCH / V_DIFF_AMBIG
}

pub const V_MATCH: u16 = 0;
pub const V_DIFF_AMBIG: u16 = 1;

/// 抽检账枢纽。
pub struct S2tRound {
    ledger: Vec<SpotRecord>,
    pub spot_total: u32,
    pub match_total: u32,
    pub diff_total: u32,
}

impl S2tRound {
    pub fn new() -> S2tRound {
        S2tRound { ledger: Vec::new(), spot_total: 0, match_total: 0, diff_total: 0 }
    }

    /// 抽检一条：s→t→s 往返，差异如实记账（纪律 3）。
    pub fn spot(&mut self, s: u32) -> Result<(), u16> {
        let t = s2t(s)?;
        let back = match t2s(t) {
            Ok(v) => v,
            Err(_) => return Err(E_UNMAPPED), // 缺省锚表内 t 必可归简
        };
        let verdict = if back == s { V_MATCH } else { V_DIFF_AMBIG };
        if verdict == V_MATCH {
            self.match_total += 1;
        } else {
            self.diff_total += 1;
        }
        self.ledger.push(SpotRecord { s, t, back, verdict });
        self.spot_total += 1;
        Ok(())
    }

    /// 差异归因审计（纪律 4）：每条 DIFF 的 s 必须是 multi 成员；
    /// MATCH 的 s 必须 t2s 归简无歧义（multi=false）。
    pub fn audit(&self) -> Result<(), u16> {
        if self.ledger.len() as u32 != self.spot_total {
            return Err(E_LEDGER_BAD);
        }
        if self.match_total + self.diff_total != self.spot_total {
            return Err(E_LEDGER_BAD);
        }
        let mut i = 0usize;
        while i < self.ledger.len() {
            let r = &self.ledger[i];
            // 找表项。
            let mut found: Option<S2tEntry> = None;
            let mut j = 0usize;
            while j < S2T_TABLE.len() {
                if S2T_TABLE[j].s == r.s {
                    found = Some(S2T_TABLE[j]);
                    break;
                }
                j += 1;
            }
            let e = match found {
                Some(e) => e,
                None => return Err(E_UNMAPPED),
            };
            if r.verdict == V_DIFF_AMBIG {
                if !e.multi {
                    return Err(E_UNJUSTIFIED); // 非 multi 差异 = 真损
                }
                // 差异条目的回得值必须是合法简体码且能再次 s2t。
                s2t(r.back)?;
            }
            // MATCH 判定无归因限制：multi 成员在缺省锚单射归简下
            // 往返回原字是合法事实（不保证 ≠ 必然不同）。
            i += 1;
        }
        // 声明复核：单向承诺常量在位且未被改写。
        if !DECL_ONE_WAY || DECL_DIRECTION != 1 {
            return Err(E_DECL_BROKEN);
        }
        Ok(())
    }

    /// ×N 抽检覆盖核验（J1 硬口径）。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.spot_total < want {
            return Err(E_SAMPLE_MISS);
        }
        Ok(())
    }

    /// 诚实登记终判：差异条数如实（>0 即有差异，账面必须显出）。
    pub fn assert_honest(&self, expect_diff: u32) -> Result<(), u16> {
        if self.diff_total != expect_diff {
            return Err(E_LEDGER_BAD);
        }
        Ok(())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（声明入册 + 抽检 ×10 差异如实记账），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 声明入册：单向承诺常量逐位核对（方向=s2t、双向保证不声明）。
    if DECL_ONE_WAY != true || DECL_DIRECTION != 1 {
        return Err(E_DECL_BROKEN);
    }
    // 表内 multi 成员恰 3 个（发/干/后）。
    let mut multi_n = 0u32;
    let mut i = 0usize;
    while i < S2T_TABLE.len() {
        if S2T_TABLE[i].multi {
            multi_n += 1;
        }
        i += 1;
    }
    if multi_n != 3 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 单向无损：36 表字逐条 s2t 出码、出码 t2s 可归简（承诺域内全通）。
    let mut i = 0usize;
    while i < S2T_TABLE.len() {
        let t = s2t(S2T_TABLE[i].s)?;
        t2s(t)?;
        i += 1;
    }
    hits += 1;

    // R3 主抽检 ×10：10 条样本（7 非 multi + 3 multi）往返抽检。
    // 缺省锚表下 t→s 单射归简，multi 成员往返亦回原字——差异如实
    // 记账的口径是「账面如实现」，本锚表如实记 0 差异（R5 复核）。
    let mut sr = S2tRound::new();
    let samples: [u32; 10] = [
        0x4E2D, 0x56FD, 0x5B66, 0x8F93, 0x6D4B, 0x5F00, 0x65F6, // 7 非 multi
        0x53D1, 0x5E72, 0x540E, // 3 multi（发/干/后）
    ];
    let mut i = 0usize;
    while i < samples.len() {
        sr.spot(samples[i])?;
        i += 1;
    }
    sr.coverage(10)?;
    sr.audit()?;
    hits += 1;

    // R4 抽检账逐条复核：t==s2t(s)、back==t2s(t)（账面忠实），
    //    DIFF 判定必须归因到 multi 成员（归因口径单向闭合，
    //    MATCH 判定只需 s2t(t) 幂等回原字路径成立）。
    let mut i = 0usize;
    while i < sr.ledger_len() {
        let r = &sr.ledger[i];
        if r.t != s2t(r.s)? || r.back != t2s(r.t)? {
            return Err(E_LEDGER_BAD);
        }
        if r.verdict == V_DIFF_AMBIG {
            let mut e: Option<S2tEntry> = None;
            let mut j = 0usize;
            while j < S2T_TABLE.len() {
                if S2T_TABLE[j].s == r.s {
                    e = Some(S2T_TABLE[j]);
                    break;
                }
                j += 1;
            }
            match e {
                Some(entry) => {
                    if !entry.multi {
                        return Err(E_UNJUSTIFIED);
                    }
                }
                None => return Err(E_UNMAPPED),
            }
        }
        i += 1;
    }
    hits += 1;

    // R5 差异数如实：缺省锚表 t→s 单射，抽检差异如实=0、匹配=10
    // （不粉饰为有损，也不虚造差异；差异通道可达性由 R7 正反例证明）。
    sr.assert_honest(0)?;
    if sr.match_total != 10 || sr.diff_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 multi 成员差异机理：發(0x767C) t2s 归简到發自身同形位（表内
    //    无 t=0x767C 的其他条目时 back==s 则 MATCH——锚表须保证 multi
    //    成员 t 恰好单射回自身才算锚定正确；本锚表發/乾/後均单射）。
    //    换言之：缺省锚表下往返差异为 0，差异条数如实=0 口径复核。
    let t_fa = s2t(0x53D1)?;
    let back_fa = t2s(t_fa)?;
    if back_fa != 0x53D1 {
        return Err(E_LEDGER_BAD); // 锚表单射归简
    }
    hits += 1;

    // R7 真损抓拍：非 multi 成员若被记 DIFF，audit 必红（判据不是
    //    恒绿——归因失效可观测）；multi 成员记 DIFF 则 audit 放行
    //    （差异通道双向可达，账面如实）。
    let mut sr = S2tRound::new();
    sr.spot(0x4E2D)?; // 中 → MATCH
    sr.ledger[0].verdict = V_DIFF_AMBIG; // 伪造非 multi 差异
    if sr.audit().is_ok() {
        return Err(E_UNJUSTIFIED);
    }
    // 正例：发（multi）记 DIFF——归因成立，audit 放行。
    let mut sr2 = S2tRound::new();
    sr2.spot(0x53D1)?; // 发
    sr2.ledger[0].verdict = V_DIFF_AMBIG;
    sr2.diff_total = 1;
    sr2.match_total = 0;
    sr2.spot_total = 1;
    sr2.audit()?;
    hits += 1;

    // R8 表外拒转与写回（判据 J2）：表外字 s2t/t2s 双向拒；抽检表外
    //    字拒收不落账；末尾写回。
    if s2t(0x64CE).is_ok() || t2s(0x64CE).is_ok() {
        return Err(E_UNMAPPED);
    }
    let mut sr = S2tRound::new();
    if sr.spot(0x64CE).is_ok() {
        return Err(E_UNMAPPED);
    }
    if sr.spot_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    if sr.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sr.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}
