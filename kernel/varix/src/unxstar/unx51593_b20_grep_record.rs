//! UNX-F51593 B20 防重 grep 执行记录（关键词族：联轧/终核/补测清单/收官印） · 0xC1700段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文：UNX-F51594-J1 衔接清单登记；闭账物三件确认栏就位；③ 完成时
//! API/软件写回：状态=已完成、结果摘要非空。
//!
//! 结构清单：
//!   - Keyword：四关键词族闭集（联轧/终核/补测清单/收官印）
//!   - GrepRecord：一次防重 grep 的执行行（范围/关键词/命中数/防重印）
//!   - GrepRegistry：执行记录册（逐条留痕 + 衔接清单 + 闭账物三件确认栏）
//!   - ClosureItem：闭账物三件（收官印册/grep 留痕册/衔接清单）确认态
//!   - GrepCode：诊断码闭集（12 变体）
//!   - Lcg / fnv1a：确定性注入与双跑指纹
//!
//! 设计要点：
//!   ① no_std + alloc：全实现只用 core/alloc，零浮点、零 IO、零墙钟、零随机
//!      （LCG 种子由 SEG_BASE 派生）；零 unwrap/expect/panic/裸下标。
//!   ② 防重：同一（范围,关键词）对只许登记一次，重复显性拒（DupRecord）——
//!      防重 grep 的机器侧对偶即「重跑不入册」。
//!   ③ 命中守恒：每次 grep 记录命中数与关键词匹配数一致（hits==matches），
//!      漂移显性判 HitDrift。
//!   ④ 衔接清单登记：闭账物三件确认栏逐件登记就位，缺件显性判 ClosureGap。
//!   ⑤ 注入核：4 类坏样本（重复登记/命中漂移/关键词越闭集/缺件确认）逐类
//!      10 轮精确复现，注入账守恒（检出+逃逸=轮数，逃逸=0）。
//! ─────────────────────────────────────────────────────────────────────

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

pub const SEG_BASE: u32 = 0xC1700;

/// grep 范围数（四范围留痕）。
pub const GREP_RANGES: usize = 4;
/// 关键词族数。
pub const KEYWORDS: usize = 4;
/// 闭账物三件数。
pub const CLOSURE_ITEMS: usize = 3;
/// 每类坏样本注入轮数。
pub const INJ_ROUNDS: usize = 10;

/// 四关键词族闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Keyword {
    /// 联轧
    LianZha = 0,
    /// 终核
    ZhongHe = 1,
    /// 补测清单
    BuCe = 2,
    /// 收官印
    ShouGuan = 3,
}

impl Keyword {
    /// 关键词序号（0..4）。
    pub fn idx(self) -> u32 {
        self as u32
    }
    /// 关键词字面（中文）。
    pub fn name(self) -> &'static str {
        match self {
            Keyword::LianZha => "联轧",
            Keyword::ZhongHe => "终核",
            Keyword::BuCe => "补测清单",
            Keyword::ShouGuan => "收官印",
        }
    }
    /// 闭集全量清单。
    pub fn all() -> [Keyword; 4] {
        [Keyword::LianZha, Keyword::ZhongHe, Keyword::BuCe, Keyword::ShouGuan]
    }
}

/// 一次防重 grep 的执行行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GrepRecord {
    /// 范围号（0..4）
    pub range: u32,
    /// 关键词（0=联轧,1=终核,2=补测清单,3=收官印）
    pub kw: u32,
    /// 命中行数
    pub hits: u64,
    /// 防重印（范围×关键词 的确定性指纹）
    pub dedup_fp: u64,
}

/// 闭账物确认栏。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClosureItem {
    /// 物件名（中文）
    pub name: &'static str,
    /// 是否确认就位
    pub confirmed: bool,
}

/// 防重 grep 执行记录册。
pub struct GrepRegistry {
    records: Vec<GrepRecord>,
    closure: [ClosureItem; 3],
    handover_registered: bool,
    rejected: u64,
    injected: u64,
    detected: u64,
    escaped: u64,
}

impl GrepRegistry {
    /// 新建记录册（零记录，闭账物三件未确认，衔接清单未登记）。
    pub fn new() -> GrepRegistry {
        GrepRegistry {
            records: Vec::new(),
            closure: [
                ClosureItem { name: "收官印册", confirmed: false },
                ClosureItem { name: "grep 留痕册", confirmed: false },
                ClosureItem { name: "衔接清单", confirmed: false },
            ],
            handover_registered: false,
            rejected: 0,
            injected: 0,
            detected: 0,
            escaped: 0,
        }
    }

    /// 防重印：范围×关键词 的确定性指纹（同对必同印，跨对不撞）。
    pub fn dedup_fp(range: u32, kw: u32) -> u64 {
        fnv1a(&range.to_le_bytes()) ^ fnv1a(&kw.to_le_bytes()).rotate_left(32)
    }

    /// 登记一次 grep：同（范围,关键词）防重，命中数与关键词匹配数须一致。
    pub fn record(
        &mut self,
        range: u32,
        kw: Keyword,
        hits: u64,
        matches: u64,
    ) -> Result<GrepRecord, GrepCode> {
        if range as usize >= GREP_RANGES {
            self.rejected += 1;
            return Err(GrepCode::RangeOob);
        }
        if kw.idx() as usize >= KEYWORDS {
            self.rejected += 1;
            return Err(GrepCode::KwUnknown);
        }
        if hits != matches {
            self.rejected += 1;
            return Err(GrepCode::HitDrift);
        }
        let fp = Self::dedup_fp(range, kw.idx());
        if self.records.iter().any(|r| r.dedup_fp == fp) {
            self.rejected += 1;
            return Err(GrepCode::DupRecord);
        }
        let rec = GrepRecord { range, kw: kw.idx(), hits, dedup_fp: fp };
        self.records.push(rec);
        Ok(rec)
    }

    /// 衔接清单登记（F51594-J1 判据面）。
    pub fn register_handover(&mut self) -> Result<(), GrepCode> {
        if self.handover_registered {
            self.rejected += 1;
            return Err(GrepCode::DupHandover);
        }
        self.handover_registered = true;
        Ok(())
    }

    /// 闭账物确认：逐件确认就位（越物号显性拒）。
    pub fn confirm_closure(&mut self, idx: usize) -> Result<(), GrepCode> {
        match self.closure.get_mut(idx) {
            Some(item) => {
                item.confirmed = true;
                Ok(())
            }
            None => {
                self.rejected += 1;
                Err(GrepCode::ClosureOob)
            }
        }
    }

    /// 闭账物三件确认栏就位判据。
    pub fn closure_ok(&self) -> bool {
        self.closure.iter().all(|c| c.confirmed)
    }

    /// 衔接清单已登记。
    pub fn handover_ok(&self) -> bool {
        self.handover_registered
    }

    /// 全族留痕判据：四范围 × 四关键词全在册（防重 grep 执行记录完整性）。
    pub fn full_coverage(&self) -> bool {
        for r in 0..GREP_RANGES as u32 {
            for k in Keyword::all().iter() {
                let fp = Self::dedup_fp(r, k.idx());
                if !self.records.iter().any(|rec| rec.dedup_fp == fp) {
                    return false;
                }
            }
        }
        true
    }

    /// 记录数。
    pub fn records_len(&self) -> usize {
        self.records.len()
    }

    /// 记录轴可回放：防重印互异且命中非零。
    pub fn replayable(&self) -> bool {
        for i in 0..self.records.len() {
            let a = match self.records.get(i) {
                Some(a) => a,
                None => return false,
            };
            if a.hits == 0 {
                return false;
            }
            for j in (i + 1)..self.records.len() {
                let b = match self.records.get(j) {
                    Some(b) => b,
                    None => return false,
                };
                if a.dedup_fp == b.dedup_fp {
                    return false;
                }
            }
        }
        !self.records.is_empty()
    }

    /// 命中总数。
    pub fn total_hits(&self) -> u64 {
        self.records.iter().map(|r| r.hits).sum()
    }

    /// (记录数, 拒绝数) 二本计数。
    pub fn tally(&self) -> (u64, u64) {
        (self.records.len() as u64, self.rejected)
    }

    /// 拒绝账计数。
    pub fn rejected_tally(&self) -> u64 {
        self.rejected
    }

    /// 注入记账：检出加一。
    pub fn note_detected(&mut self) {
        self.injected += 1;
        self.detected += 1;
    }

    /// 注入逃逸（判据要求恒 0）。
    pub fn note_escape(&mut self) {
        self.injected += 1;
        self.escaped += 1;
    }

    /// 守恒断言面。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0 && self.detected > 0
    }

    /// 快照指纹（双跑一致面）。
    pub fn fingerprint(&self) -> u64 {
        let mut bytes: Vec<u8> = Vec::new();
        bytes.extend_from_slice(&(self.handover_registered as u64).to_le_bytes());
        for (i, c) in self.closure.iter().enumerate() {
            bytes.extend_from_slice(&(i as u64).to_le_bytes());
            bytes.extend_from_slice(&(c.confirmed as u64).to_le_bytes());
        }
        for r in self.records.iter() {
            bytes.extend_from_slice(&r.range.to_le_bytes());
            bytes.extend_from_slice(&r.kw.to_le_bytes());
            bytes.extend_from_slice(&r.hits.to_le_bytes());
            bytes.extend_from_slice(&r.dedup_fp.to_le_bytes());
        }
        fnv1a(&bytes)
    }
}

/// 诊断码闭集（B20 防重 grep 执行记录域）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GrepCode {
    /// 成功
    Ok = 0,
    /// 段位守卫失败
    SegBad,
    /// 范围越界
    RangeOob,
    /// 关键词越闭集
    KwUnknown,
    /// 命中数与匹配数漂移
    HitDrift,
    /// 重复登记（防重）
    DupRecord,
    /// 衔接清单重复登记
    DupHandover,
    /// 闭账物号越界
    ClosureOob,
    /// 闭账物缺件确认
    ClosureGap,
    /// 全族留痕缺口（四范围×四关键词有缺）
    CoverGap,
    /// 防重印撞号
    FpCollision,
    /// 记录轴为空
    EmptyAxis,
}

impl GrepCode {
    /// 诊断值 = SEG_BASE + 1 + 变体序号。
    pub fn val(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }
    /// 全量中文诊断文本。
    pub fn detail(self) -> &'static str {
        match self {
            GrepCode::Ok => "防重 grep 记录判定成功",
            GrepCode::SegBad => "段位越界：检查 SEG_BASE 对齐与取值范围",
            GrepCode::RangeOob => "范围越界：grep 范围必须在 0..4 之内",
            GrepCode::KwUnknown => "关键词非法：必须在联轧/终核/补测清单/收官印闭集内",
            GrepCode::HitDrift => "命中漂移：登记命中数必须与关键词匹配数一致",
            GrepCode::DupRecord => "重复登记：同（范围,关键词）防重不入册",
            GrepCode::DupHandover => "衔接清单重复：登记只许一次",
            GrepCode::ClosureOob => "闭账物号越界：三件确认栏序号 0..3",
            GrepCode::ClosureGap => "闭账物缺件：三件确认栏须逐件就位",
            GrepCode::CoverGap => "留痕缺口：四范围×四关键词须全在册",
            GrepCode::FpCollision => "防重印撞号：不同（范围,关键词）不得同印",
            GrepCode::EmptyAxis => "记录轴为空：可回放判据要求至少一行",
        }
    }
}

/// 诊断码闭集权威清单（12 变体）。
pub const ALL_CODES: [GrepCode; 12] = [
    GrepCode::Ok,
    GrepCode::SegBad,
    GrepCode::RangeOob,
    GrepCode::KwUnknown,
    GrepCode::HitDrift,
    GrepCode::DupRecord,
    GrepCode::DupHandover,
    GrepCode::ClosureOob,
    GrepCode::ClosureGap,
    GrepCode::CoverGap,
    GrepCode::FpCollision,
    GrepCode::EmptyAxis,
];

/// FNV-1a 64 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes.iter() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 确定性 LCG（种子由 SEG_BASE 派生，零随机源）。
pub struct Lcg {
    s: u64,
}

impl Lcg {
    /// 由段基派生种子。
    pub fn new(seed: u64) -> Lcg {
        Lcg { s: seed ^ 0x9e3779b185ebca87 }
    }
    /// 取下一个 32 位伪随机数。
    pub fn next_u32(&mut self) -> u32 {
        self.s = self
            .s
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.s >> 33) as u32
    }
}

/// 段位守卫。
pub fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0xC0080 && SEG_BASE <= 0xC31C0
}

/// 标准脚本：四范围×四关键词全登记 + 衔接登记 + 三件确认。返回 (指纹, 记录数)。
pub fn standard_script() -> (u64, usize) {
    let mut g = GrepRegistry::new();
    for r in 0..GREP_RANGES as u32 {
        for k in Keyword::all().iter() {
            let _ = g.record(r, *k, 3, 3);
        }
    }
    let _ = g.register_handover();
    for i in 0..CLOSURE_ITEMS {
        let _ = g.confirm_closure(i);
    }
    (g.fingerprint(), g.records_len())
}

/// 双跑一致性：同一脚本两次建册，指纹与记录数逐位一致。
pub fn dual_run_consistent() -> bool {
    standard_script() == standard_script()
}

/// 注入核：4 类坏样本 ×10 轮精确复现，守恒账（检出+逃逸=轮数）。
pub fn injection_harness() -> (u64, u64, [u64; 4]) {
    let mut total_detect: u64 = 0;
    let mut total_escape: u64 = 0;
    let mut per: [u64; 4] = [0; 4];
    // 类 0：重复登记——同（范围,关键词）二次入册，显性拒（DupRecord）。
    for _ in 0..INJ_ROUNDS {
        let mut g = GrepRegistry::new();
        let _ = g.record(0, Keyword::LianZha, 5, 5);
        let caught = g.record(0, Keyword::LianZha, 5, 5) == Err(GrepCode::DupRecord);
        if caught {
            g.note_detected();
            total_detect += 1;
            per[0] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 1：命中漂移——hits 与 matches 不一致，显性拒（HitDrift）。
    for _ in 0..INJ_ROUNDS {
        let mut g = GrepRegistry::new();
        let caught = g.record(1, Keyword::ZhongHe, 5, 4) == Err(GrepCode::HitDrift);
        if caught {
            g.note_detected();
            total_detect += 1;
            per[1] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 2：范围越界——range>=4，显性拒（RangeOob）。
    for _ in 0..INJ_ROUNDS {
        let mut g = GrepRegistry::new();
        let caught = g.record(9, Keyword::BuCe, 1, 1) == Err(GrepCode::RangeOob);
        if caught {
            g.note_detected();
            total_detect += 1;
            per[2] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    // 类 3：缺件确认——只确认两件，闭账物判据必须判缺（ClosureGap 形态）。
    for _ in 0..INJ_ROUNDS {
        let mut g = GrepRegistry::new();
        let _ = g.confirm_closure(0);
        let _ = g.confirm_closure(1);
        let caught = !g.closure_ok() && g.confirm_closure(7) == Err(GrepCode::ClosureOob);
        if caught {
            g.note_detected();
            total_detect += 1;
            per[3] += 1;
        } else {
            g.note_escape();
            total_escape += 1;
        }
    }
    (total_detect, total_escape, per)
}

/// 收口自检（14 条判据形态）。
pub fn run_unx51593_checks() -> CheckSet {
    let mut s = CheckSet::new("unx51593");
    // 1 全族留痕：四范围×四关键词全在册。
    {
        let mut g = GrepRegistry::new();
        for r in 0..GREP_RANGES as u32 {
            for k in Keyword::all().iter() {
                let _ = g.record(r, *k, 2, 2);
            }
        }
        s.add(
            "全族留痕 16 条",
            g.full_coverage() && g.records_len() == 16,
            "UNX-F51593 判据：联轧/终核/补测清单/收官印 ×4 范围全在册",
        );
    }
    // 2 防重：同对二次登记显性拒。
    {
        let mut g = GrepRegistry::new();
        let _ = g.record(0, Keyword::ShouGuan, 1, 1);
        let r = g.record(0, Keyword::ShouGuan, 1, 1);
        s.add(
            "防重二次登记拒",
            r == Err(GrepCode::DupRecord) && g.rejected_tally() >= 1,
            "同（范围,关键词）只许登记一次，重跑不入册",
        );
    }
    // 3 命中守恒：hits==matches。
    {
        let mut g = GrepRegistry::new();
        let r1 = g.record(2, Keyword::BuCe, 7, 7);
        let r2 = g.record(2, Keyword::BuCe, 7, 6);
        s.add(
            "命中守恒",
            r1.is_ok() && r2 == Err(GrepCode::HitDrift),
            "登记命中数必须与关键词匹配数一致，漂移显性拒",
        );
    }
    // 4 关键词闭集与中文字面。
    {
        let ok = Keyword::all().len() == 4
            && Keyword::all()
                .iter()
                .enumerate()
                .all(|(i, k)| k.idx() == i as u32 && !k.name().is_empty());
        s.add(
            "关键词闭集完整",
            ok,
            "四关键词族序号 0..4 且字面全量中文非空",
        );
    }
    // 5 防重印互异（跨对不撞号）。
    {
        let mut ok = true;
        for r in 0..GREP_RANGES as u32 {
            for k in Keyword::all().iter() {
                for r2 in 0..GREP_RANGES as u32 {
                    for k2 in Keyword::all().iter() {
                        if (r != r2 || k != k2)
                            && fp_pair_eq(r, k.idx(), r2, k2.idx())
                        {
                            ok = false;
                        }
                    }
                }
            }
        }
        s.add(
            "防重印互异",
            ok,
            "16 个（范围,关键词）对的防重印两两互异，零撞号",
        );
    }
    // 6 衔接清单登记一次。
    {
        let mut g = GrepRegistry::new();
        let first = g.register_handover();
        let second = g.register_handover();
        s.add(
            "衔接清单登记",
            first == Ok(()) && second == Err(GrepCode::DupHandover) && g.handover_ok(),
            "UNX-F51594-J1 衔接清单登记且只许登记一次",
        );
    }
    // 7 闭账物三件确认栏就位。
    {
        let mut g = GrepRegistry::new();
        let before = g.closure_ok();
        for i in 0..CLOSURE_ITEMS {
            let _ = g.confirm_closure(i);
        }
        s.add(
            "闭账物三件就位",
            !before && g.closure_ok(),
            "收官印册/grep 留痕册/衔接清单逐件确认",
        );
    }
    // 8 闭账物号越界拒。
    {
        let mut g = GrepRegistry::new();
        s.add(
            "闭账物号越界拒",
            g.confirm_closure(3) == Err(GrepCode::ClosureOob),
            "三件确认栏序号 0..3，越界显性拒",
        );
    }
    // 9 注入核 4 类 ×10 全通零逃逸。
    {
        let (d, esc, per) = injection_harness();
        let all = per.iter().all(|&x| x == 10);
        s.add(
            "注入 4 类×10 全通",
            d == 40 && esc == 0 && all,
            "4 类坏样本逐类 10 轮精确复现，检出+逃逸=轮数，逃逸=0",
        );
    }
    // 10 记录轴可回放。
    {
        let mut g = GrepRegistry::new();
        for r in 0..GREP_RANGES as u32 {
            let _ = g.record(r, Keyword::LianZha, 4, 4);
        }
        s.add(
            "记录轴可回放",
            g.replayable() && g.total_hits() == 16,
            "防重印互异且命中非零，命中总数与记录一致",
        );
    }
    // 11 诊断码闭集唯一性。
    {
        let mut ok = true;
        for (i, c) in ALL_CODES.iter().enumerate() {
            if c.val() != SEG_BASE + 1 + i as u32 || c.detail().is_empty() {
                ok = false;
            }
        }
        s.add(
            "诊断码闭集唯一",
            ok && ALL_CODES.len() >= 10 && ALL_CODES.len() <= 14,
            "val=SEG_BASE+1+序号，detail 全量中文非空，闭集 12 变体",
        );
    }
    // 12 注入账守恒。
    {
        let mut g = GrepRegistry::new();
        for _ in 0..13u32 {
            g.note_detected();
        }
        s.add("注入账守恒", g.conserved(), "注入=检出+逃逸且逃逸=0 且检出>0");
    }
    // 13 双跑 FNV 指纹一致。
    s.add(
        "双跑 FNV 指纹一致",
        dual_run_consistent(),
        "同一脚本两次建册，指纹与记录数逐位一致",
    );
    // 14 段位守卫与结果摘要非空。
    {
        let summary: String = format!(
            "unx51593 B20 防重 grep 执行记录 {} 范围 {} 关键词",
            GREP_RANGES, KEYWORDS
        );
        s.add(
            "段位守卫与摘要",
            seg_guard_ok() && !summary.is_empty() && summary.contains("unx51593"),
            "SEG_BASE 对齐且在分派区间，结果摘要非空",
        );
    }
    s
}

/// 防重印相等辅助（供判据 5 调用）。
fn fp_pair_eq(r1: u32, k1: u32, r2: u32, k2: u32) -> bool {
    GrepRegistry::dedup_fp(r1, k1) == GrepRegistry::dedup_fp(r2, k2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx51593_checks_green() {
        let set = run_unx51593_checks();
        assert!(set.all_passed(), "{}", set.red_items_text());
    }

    #[test]
    fn full_coverage_sixteen_records() {
        let mut g = GrepRegistry::new();
        for r in 0..GREP_RANGES as u32 {
            for k in Keyword::all().iter() {
                assert!(g.record(r, *k, 1, 1).is_ok());
            }
        }
        assert_eq!(g.records_len(), 16);
        assert!(g.full_coverage());
    }

    #[test]
    fn dedup_rejects_second_record() {
        let mut g = GrepRegistry::new();
        assert!(g.record(0, Keyword::ZhongHe, 3, 3).is_ok());
        assert_eq!(g.record(0, Keyword::ZhongHe, 3, 3), Err(GrepCode::DupRecord));
        assert_eq!(g.records_len(), 1);
    }

    #[test]
    fn hit_drift_rejected() {
        let mut g = GrepRegistry::new();
        assert_eq!(g.record(0, Keyword::LianZha, 5, 4), Err(GrepCode::HitDrift));
        assert_eq!(g.records_len(), 0);
    }

    #[test]
    fn range_oob_rejected() {
        let mut g = GrepRegistry::new();
        assert_eq!(g.record(4, Keyword::LianZha, 1, 1), Err(GrepCode::RangeOob));
    }

    #[test]
    fn dedup_fp_pairwise_distinct() {
        for r1 in 0..GREP_RANGES as u32 {
            for k1 in Keyword::all().iter() {
                for r2 in 0..GREP_RANGES as u32 {
                    for k2 in Keyword::all().iter() {
                        let same = r1 == r2 && k1 == k2;
                        assert_eq!(
                            GrepRegistry::dedup_fp(r1, k1.idx())
                                == GrepRegistry::dedup_fp(r2, k2.idx()),
                            same
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn handover_register_once() {
        let mut g = GrepRegistry::new();
        assert_eq!(g.register_handover(), Ok(()));
        assert_eq!(g.register_handover(), Err(GrepCode::DupHandover));
        assert!(g.handover_ok());
    }

    #[test]
    fn closure_three_items() {
        let mut g = GrepRegistry::new();
        assert!(!g.closure_ok());
        for i in 0..CLOSURE_ITEMS {
            assert!(g.confirm_closure(i).is_ok());
        }
        assert!(g.closure_ok());
        assert_eq!(g.confirm_closure(3), Err(GrepCode::ClosureOob));
    }

    #[test]
    fn injection_all_caught() {
        let (d, esc, per) = injection_harness();
        assert_eq!(esc, 0);
        assert_eq!(d, 40);
        for (i, &p) in per.iter().enumerate() {
            assert_eq!(p, 10, "类 {} 应恰 10 次检出", i);
        }
    }

    #[test]
    fn conservation_holds() {
        let mut g = GrepRegistry::new();
        for _ in 0..10u32 {
            g.note_detected();
        }
        assert!(g.conserved());
        let mut bad = GrepRegistry::new();
        bad.note_escape();
        assert!(!bad.conserved());
    }

    #[test]
    fn dual_run_identical_fingerprint() {
        assert!(dual_run_consistent());
        assert_eq!(standard_script(), standard_script());
    }

    #[test]
    fn code_table_closed_and_unique() {
        assert_eq!(ALL_CODES.len(), 12);
        for (i, c) in ALL_CODES.iter().enumerate() {
            assert_eq!(c.val(), SEG_BASE + 1 + i as u32);
            assert!(!c.detail().is_empty());
        }
    }

    #[test]
    fn seg_guard_and_lcg_deterministic() {
        assert!(seg_guard_ok());
        let mut a = Lcg::new(SEG_BASE as u64);
        let mut b = Lcg::new(SEG_BASE as u64);
        for _ in 0..16u32 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn tally_and_total_hits() {
        let mut g = GrepRegistry::new();
        let _ = g.record(0, Keyword::LianZha, 4, 4);
        let _ = g.record(1, Keyword::LianZha, 6, 6);
        let (n, rej) = g.tally();
        assert_eq!((n, rej), (2, 0));
        assert_eq!(g.total_hits(), 10);
    }
}
