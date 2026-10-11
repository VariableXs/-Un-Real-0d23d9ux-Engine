//! VE-F5420 · AA01 组收口双签（vef72_aasign）
//!
//! **家族双签模板同域沿用**（AA 域首组）。**双签三件**：① 台账深
//! 化完成（组内台账逐单深化状态回写——回写 O(组规模)）；② 会话
//! S185 回写（会话锚写回台账——失败即**整组回滚**，半回写状态不
//! 允许存在）；③ **字数核对**（声明卡字数与台账字数一致——不符
//! **复核禁签**，核验 O(1)）。**AA01 交付声明**：四层架构+双通道+
//! 补偿框架**全线就绪**（三就绪位闭集——缺一不签）。
//!
//! 错误路径与降级矩阵：**字数不符→复核禁签**（不是自动改数——数
//! 字对不上就停下来人查）；**红线未销→拒绝收口**（红线账非空收
//! 口闸不放——红线是承诺不是装饰）；**回写失败→整组回滚**（committed
//! 清零重走——半回写是最坏状态）。
//!
//! 性能逐项：回写 O(组规模)（组内逐单）；核验 O(1)（单整数比较）；
//! 归档 O(证据数)（证据逐条入册）。
//!
//! 跨批对接点：F5419 上游；F5421 AA02 开工下游。无障碍：报告结构
//! 化（域本色）；无隐私面。
//!
//! 诊断码 0xEC 段五码互异：`E_AS_WORDCOUNT` / `E_AS_REDLINE_OPEN` /
//! `E_AS_WRITEBACK_FAIL` / `E_AS_NO_DECL` / `E_AS_SIGN_ORDER`。

use crate::checks::CheckSet;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// AA01 收口双签版本。
pub const AA_SIGN_VERSION: &str = "F72-aasign-v1";
/// AA01 组内单数（F5401~F5419 十九单——回写 O(组规模) 的规模锚）。
pub const GROUP_UNIT_COUNT: usize = 19;
/// 会话回写锚（S185——台账回写目标的会话号）。
pub const SESSION_ANCHOR: &str = "S185";
/// 双签人闭集。
pub const SIGNERS: [&str; 2] = ["builder", "engineer"];

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0xEC 段五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 字数不符：声明卡与台账字数不一致，复核禁签。
pub const E_AS_WORDCOUNT: u16 = 0xEC01;
/// 红线未销：红线账非空，拒绝收口。
pub const E_AS_REDLINE_OPEN: u16 = 0xEC02;
/// 回写失败：单笔写回失败，整组回滚。
pub const E_AS_WRITEBACK_FAIL: u16 = 0xEC03;
/// 交付声明缺项：三就绪位未齐。
pub const E_AS_NO_DECL: u16 = 0xEC04;
/// 签署缺位：双签人未齐或顺序错。
pub const E_AS_SIGN_ORDER: u16 = 0xEC05;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_AS_WORDCOUNT => "字数不符：声明卡与台账字数不一致，复核禁签",
        E_AS_REDLINE_OPEN => "红线未销：红线账非空，拒绝收口",
        E_AS_WRITEBACK_FAIL => "回写失败：单笔写回失败，整组回滚清零",
        E_AS_NO_DECL => "交付声明缺项：四层/双通道/补偿三就绪位未齐",
        E_AS_SIGN_ORDER => "签署缺位：双签人未齐，双签不成立",
        _ => "未知诊断码",
    }
}

/// 立案簿（0xEC 段事件账——有界不静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagNote {
    pub code: u16,
    pub subject: String,
    pub detail: String,
}

#[derive(Debug)]
pub struct DiagBag {
    notes: Vec<DiagNote>,
    cap: usize,
    dropped: usize,
}

impl DiagBag {
    pub fn new() -> DiagBag {
        DiagBag { notes: Vec::new(), cap: 64, dropped: 0 }
    }

    pub fn push(&mut self, code: u16, subject: &str, detail: &str) {
        if self.notes.len() >= self.cap {
            self.notes.remove(0);
            self.dropped += 1;
        }
        self.notes.push(DiagNote {
            code,
            subject: subject.to_string(),
            detail: detail.to_string(),
        });
    }

    pub fn len(&self) -> usize {
        self.notes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
    }

    pub fn count_of(&self, code: u16) -> usize {
        self.notes.iter().filter(|n| n.code == code).count()
    }

    pub fn has(&self, code: u16) -> bool {
        self.count_of(code) > 0
    }

    pub fn spoken(&self) -> String {
        if self.notes.is_empty() {
            return "诊断簿空".to_string();
        }
        let mut out = format!("诊断簿共{}条", self.notes.len());
        for n in self.notes.iter() {
            out.push('；');
            out.push_str(reason_of(n.code));
            out.push('：');
            out.push_str(&n.subject);
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 台账回写（O(组规模)——失败整组回滚）
// ---------------------------------------------------------------------------

/// 台账回写器（组内单据逐笔写回——committed/回滚两态）。
#[derive(Debug)]
pub struct LedgerWriteback {
    /// 单据 id → 回写状态（true=已回写）。
    writes: BTreeMap<u32, bool>,
    /// 已提交回写数（回滚清零）。
    pub committed: u32,
    /// 回写目标会话锚。
    pub session: &'static str,
    pub bag: DiagBag,
}

impl LedgerWriteback {
    pub fn new(session: &'static str) -> LedgerWriteback {
        LedgerWriteback {
            writes: BTreeMap::new(),
            committed: 0,
            session,
            bag: DiagBag::new(),
        }
    }

    /// 单笔回写（O(1)/笔——O(组规模) 总量；失败立案并整组回滚）。
    pub fn write_one(&mut self, unit_id: u32, ok: bool) -> Result<(), u16> {
        if !ok {
            self.bag.push(
                E_AS_WRITEBACK_FAIL,
                self.session,
                &format!("单据{}回写失败——整组回滚", unit_id),
            );
            self.rollback_all();
            return Err(E_AS_WRITEBACK_FAIL);
        }
        self.writes.insert(unit_id, true);
        self.committed += 1;
        Ok(())
    }

    /// 整组回滚（committed 清零——半回写状态不允许存在）。
    pub fn rollback_all(&mut self) {
        self.writes.clear();
        self.committed = 0;
    }

    pub fn committed_count(&self) -> usize {
        self.writes.len()
    }

    pub fn is_clean(&self) -> bool {
        self.writes.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 红线闸与字数核对（核验 O(1)）
// ---------------------------------------------------------------------------

/// 红线闸（红线账——非空拒收口）。
#[derive(Debug)]
pub struct RedlineGate {
    /// 未销红线清单。
    open: Vec<&'static str>,
    pub bag: DiagBag,
}

impl RedlineGate {
    pub fn new() -> RedlineGate {
        RedlineGate { open: Vec::new(), bag: DiagBag::new() }
    }

    /// 红线登记。
    pub fn raise(&mut self, tag: &'static str) {
        self.open.push(tag);
    }

    /// 红线销账（O(n) 查删）。
    pub fn clear(&mut self, tag: &str) -> bool {
        let before = self.open.len();
        self.open.retain(|t| *t != tag);
        before != self.open.len()
    }

    /// 收口放行判定（O(1) 长度查——非空拒）。
    pub fn allow_close(&mut self) -> Result<(), u16> {
        if !self.open.is_empty() {
            self.bag.push(
                E_AS_REDLINE_OPEN,
                "close",
                &format!("{}条红线未销", self.open.len()),
            );
            return Err(E_AS_REDLINE_OPEN);
        }
        Ok(())
    }

    pub fn open_count(&self) -> usize {
        self.open.len()
    }
}

/// 字数核对（声明卡 vs 台账——不符复核禁签，O(1)）。
pub fn wordcount_gate(ledger_words: u64, card_words: u64, bag: &mut DiagBag) -> Result<(), u16> {
    if ledger_words != card_words {
        bag.push(
            E_AS_WORDCOUNT,
            "wordcount",
            &format!("台账{}≠声明卡{}", ledger_words, card_words),
        );
        return Err(E_AS_WORDCOUNT);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 交付声明卡（三就绪位——缺一不签）
// ---------------------------------------------------------------------------

/// 交付声明卡（四层架构+双通道+补偿框架全线就绪）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DelivCard {
    /// 四层架构就绪。
    pub four_layer: bool,
    /// 双通道就绪。
    pub dual_channel: bool,
    /// 补偿框架就绪。
    pub compensation: bool,
    /// 声明卡字数（核对锚）。
    pub words: u64,
}

impl DelivCard {
    /// 三就绪位齐判定（O(1)）——缺项立案。
    pub fn ready(&self, bag: &mut DiagBag) -> Result<(), u16> {
        if !(self.four_layer && self.dual_channel && self.compensation) {
            bag.push(E_AS_NO_DECL, "deliv", "三就绪位未齐");
            return Err(E_AS_NO_DECL);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 双签单（三件齐+双人签——收口唯一正门）
// ---------------------------------------------------------------------------

/// 双签单（台账深化/回写/字数三件+双签人）。
#[derive(Debug)]
pub struct SignSheet {
    /// 三件状态。
    pub ledger_deepened: bool,
    pub session_written: bool,
    pub wordcount_ok: bool,
    /// 已签人（BTreeMap 保序——签署缺位判定）。
    signed: BTreeMap<&'static str, bool>,
    /// 收口成立。
    pub closed: bool,
    pub bag: DiagBag,
}

impl SignSheet {
    pub fn new() -> SignSheet {
        SignSheet {
            ledger_deepened: false,
            session_written: false,
            wordcount_ok: false,
            signed: BTreeMap::new(),
            closed: false,
            bag: DiagBag::new(),
        }
    }

    /// 签署（双签人齐+三件全 true→收口成立；缺位→E_AS_SIGN_ORDER）。
    pub fn sign(
        &mut self,
        who: &'static str,
        card: DelivCard,
    ) -> Result<(), u16> {
        // 查 1：签人人闭集。
        if !SIGNERS.contains(&who) {
            self.bag.push(E_AS_SIGN_ORDER, who, "非双签人");
            return Err(E_AS_SIGN_ORDER);
        }
        // 查 2：三件齐（台账深化+回写+字数）。
        if !(self.ledger_deepened && self.session_written && self.wordcount_ok) {
            self.bag.push(E_AS_SIGN_ORDER, who, "三件未齐禁签");
            return Err(E_AS_SIGN_ORDER);
        }
        // 查 3：交付声明三就绪位。
        card.ready(&mut self.bag)?;
        // 查 4：红线闸（外部传入由调用方先核——此处存签名）。
        self.signed.insert(who, true);
        if self.signed.len() == SIGNERS.len() {
            self.closed = true;
        }
        Ok(())
    }

    pub fn signed_count(&self) -> usize {
        self.signed.len()
    }

    pub fn has_signed(&self, who: &str) -> bool {
        self.signed.contains_key(who)
    }
}

// ---------------------------------------------------------------------------
// 判据（六项——双签三件/字数禁签/红线拒收/整组回滚/三就绪/码）
// ---------------------------------------------------------------------------

/// VE-F5420 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_vef72_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5420");

    // 签-01 双签三件：台账深化+S185 回写+字数核对齐→双人签→收口。
    {
        let mut wb = LedgerWriteback::new(SESSION_ANCHOR);
        // 回写 19 单全成功（O(组规模)——手算：committed=19）。
        for id in 1..=GROUP_UNIT_COUNT as u32 {
            let _ = wb.write_one(id, true);
        }
        let mut sheet = SignSheet::new();
        sheet.ledger_deepened = true;
        sheet.session_written = wb.committed == GROUP_UNIT_COUNT as u32;
        // 字数核对：台账 1200=声明卡 1200 恰值。
        let mut bag = DiagBag::new();
        sheet.wordcount_ok = wordcount_gate(1200, 1200, &mut bag) == Ok(());
        let s1 = sheet.sign("builder", DelivCard {
            four_layer: true, dual_channel: true, compensation: true, words: 1200,
        });
        let s2 = sheet.sign("engineer", DelivCard {
            four_layer: true, dual_channel: true, compensation: true, words: 1200,
        });
        // 手算：committed=19；双签齐→closed；字数恰值过。
        let ok = wb.committed == 19
            && sheet.session_written
            && sheet.wordcount_ok
            && s1 == Ok(())
            && s2 == Ok(())
            && sheet.signed_count() == 2
            && sheet.closed
            && bag.is_empty();
        if ok {
            s.ok("F5420-签-01-双签三件收口成立");
        } else {
            s.fail("F5420-签-01-双签三件收口成立", "双签面不符");
        }
    }

    // 签-02 字数不符禁签：台账 1201≠声明 1200→复核禁签立案。
    {
        let mut bag = DiagBag::new();
        let r = wordcount_gate(1201, 1200, &mut bag);
        // 手算：1201≠1200→E_AS_WORDCOUNT；禁签面复核不自动改数。
        let ok = r == Err(E_AS_WORDCOUNT)
            && bag.count_of(E_AS_WORDCOUNT) == 1
            && bag.spoken().contains("复核禁签");
        if ok {
            s.ok("F5420-签-02-字数不符复核禁签");
        } else {
            s.fail("F5420-签-02-字数不符复核禁签", "禁签面不符");
        }
    }

    // 收-03 红线未销拒收口：红线账非空拒；销账后放行。
    {
        let mut g = RedlineGate::new();
        g.raise("预算口径未对齐");
        g.raise("遥测字段越界");
        let deny = g.allow_close();
        // 销一条还剩一条——仍拒。
        let _ = g.clear("预算口径未对齐");
        let deny2 = g.allow_close();
        // 全销——放行。
        let _ = g.clear("遥测字段越界");
        let allow = g.allow_close();
        // 手算：2 条红线拒；1 条仍拒；0 条放行。
        let ok = deny == Err(E_AS_REDLINE_OPEN)
            && deny2 == Err(E_AS_REDLINE_OPEN)
            && allow == Ok(())
            && g.open_count() == 0
            && g.bag.count_of(E_AS_REDLINE_OPEN) == 2;
        if ok {
            s.ok("F5420-收-03-红线未销拒绝收口");
        } else {
            s.fail("F5420-收-03-红线未销拒绝收口", "红线面不符");
        }
    }

    // 滚-04 回写失败整组回滚：19 单中第 10 单失败→committed 清零。
    {
        let mut wb = LedgerWriteback::new(SESSION_ANCHOR);
        for id in 1..=9u32 {
            let _ = wb.write_one(id, true);
        }
        let mid = wb.committed; // 9
        let fail = wb.write_one(10, false); // 失败→整组回滚
        // 手算：前 9 笔 committed=9；第 10 笔失败→回滚 committed=0。
        let ok = mid == 9
            && fail == Err(E_AS_WRITEBACK_FAIL)
            && wb.committed == 0
            && wb.is_clean()
            && wb.bag.count_of(E_AS_WRITEBACK_FAIL) == 1
            && wb.bag.spoken().contains("整组回滚");
        if ok {
            s.ok("F5420-滚-04-回写失败整组回滚");
        } else {
            s.fail("F5420-滚-04-回写失败整组回滚", "回滚面不符");
        }
    }

    // 声-05 三就绪位：全 true 过；缺项立案禁签。
    {
        let mut bag = DiagBag::new();
        let full = DelivCard { four_layer: true, dual_channel: true, compensation: true, words: 100 };
        let r_full = full.ready(&mut bag);
        let missing = DelivCard { four_layer: true, dual_channel: false, compensation: true, words: 100 };
        let r_miss = missing.ready(&mut bag);
        // 手算：三就绪过；双通道缺→E_AS_NO_DECL。
        let ok = r_full == Ok(())
            && r_miss == Err(E_AS_NO_DECL)
            && bag.count_of(E_AS_NO_DECL) == 1;
        if ok {
            s.ok("F5420-声-05-三就绪缺项立案");
        } else {
            s.fail("F5420-声-05-三就绪缺项立案", "声明面不符");
        }
    }

    // 码-06 五码互异+reason 全量显性+签署缺位立案。
    {
        let codes = [E_AS_WORDCOUNT, E_AS_REDLINE_OPEN, E_AS_WRITEBACK_FAIL, E_AS_NO_DECL, E_AS_SIGN_ORDER];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        let seg_ok = codes[0] == 0xEC01
            && codes[1] == 0xEC02
            && codes[2] == 0xEC03
            && codes[3] == 0xEC04
            && codes[4] == 0xEC05;
        let reasons_all = codes.iter().all(|c| reason_of(*c) != "未知诊断码");
        // 签署缺位：三件未齐就签→E_AS_SIGN_ORDER。
        let mut sheet = SignSheet::new();
        let early = sheet.sign("builder", DelivCard {
            four_layer: true, dual_channel: true, compensation: true, words: 1,
        });
        let ok = distinct
            && seg_ok
            && reasons_all
            && early == Err(E_AS_SIGN_ORDER)
            && sheet.bag.count_of(E_AS_SIGN_ORDER) == 1
            && !sheet.has_signed("builder");
        if ok {
            s.ok("F5420-码-06-五码互异缺位立案");
        } else {
            s.fail("F5420-码-06-五码互异缺位立案", "诊断面不符");
        }
    }

    s
}
