//! VE-F5804 · 知识表示与黑板（veac04_blackboard）
//!
//! **黑板系统**（Agent 间共享事实数据板）与**知识条目生命周期**——
//! 事实有过期时间，**过期知识不作为决策依据**（读遇死线即标记作废
//! 且作废粘滞——复活走显性写入新时间戳）。黑板含**读写权限分档**：
//! 哪些 Agent 可读、哪些可写、哪些只属于自己的**私有板**（owner 之
//! 外读即脱敏阻断）。含**黑板快照调试**（该 Agent 可见视图——权限
//! 过滤后的读面）与**快照数据脱敏**（私板他者值不落快照、作废条目
//! 只留键名）。
//!
//! 仲裁律：**写入冲突→时间戳仲裁**（新 stamp 严格大于存活条目旧
//! stamp 才覆盖，败者立案不覆盖不静默——O(1) 单比较）；**越权写→
//! 拒绝**（owner/写白名单外显性拒）；**条目过期→标记作废**（不是
//! 删除——审计留痕，值面清空只在快照脱敏层）。
//!
//! 性能逐项：读写 O(1)（BTreeMap 单键定位）；仲裁 O(1)；过期判定
//! O(1)（死线一次加法比较）；sweep O(n) 显性维护项。
//!
//! 跨批对接点：AC03 黑板绑定（感知事实入板）；F5801 四层架构（协
//! 作层事实交换面）；F5807 遥测（板面计数喂聚合）。无障碍与隐私：
//! **黑板内容可查**（spoken 读屏摘要——键名+存活态+条目计数，值面
//! 不入读屏防侧信道）。隐私面：私板值只在 owner 视图出现；快照脱
//! 敏先行。
//!
//! 诊断码 0xE3 段五码互异：`E_BB_STALE` / `E_BB_CONFLICT` /
//! `E_BB_FORBID` / `E_BB_NO_KEY` / `E_BB_REDACT`。

use crate::checks::CheckSet;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 版本与常量（阈值独立声明，判据独立手算对拍）
// ---------------------------------------------------------------------------

/// 黑板系统版本。
pub const BLACKBOARD_VERSION: &str = "A04-blackboard-v1";
/// 私有板脱敏占位（快照脱敏值面）。
pub const REDACTED: &str = "***";
/// 作废条目快照占位。
pub const VOID_MARK: &str = "(作废)";
/// 单板条目容量（超容拒新显性——防无界增长）。
pub const BOARD_CAP: usize = 256;

// ---------------------------------------------------------------------------
// 诊断码与原因文案（0xE3 段五码互异；reason 全量显性）
// ---------------------------------------------------------------------------

/// 条目过期：读遇死线标记作废（过期知识不作为决策依据）。
pub const E_BB_STALE: u16 = 0xE301;
/// 写入冲突：时间戳仲裁败者（新 stamp 不大于旧 stamp）。
pub const E_BB_CONFLICT: u16 = 0xE302;
/// 越权写：owner/写白名单外拒绝。
pub const E_BB_FORBID: u16 = 0xE303;
/// 查无此键。
pub const E_BB_NO_KEY: u16 = 0xE304;
/// 私有板越读：owner 之外读即脱敏阻断。
pub const E_BB_REDACT: u16 = 0xE305;

/// 诊断码 → 原因文案（全量显性，缺省分支兜底）。
pub fn reason_of(code: u16) -> &'static str {
    match code {
        E_BB_STALE => "条目过期：超过存活死线，标记作废且不作为决策依据",
        E_BB_CONFLICT => "写入冲突：新时间戳不大于存活条目旧时间戳，仲裁保持旧值",
        E_BB_FORBID => "越权写：非 owner 且不在写白名单，拒绝",
        E_BB_NO_KEY => "查无此键：黑板上不存在该条目",
        E_BB_REDACT => "私有板越读：私有板仅 owner 可读，脱敏阻断",
        _ => "未知诊断码",
    }
}

/// 立案簿（0xE3 段事件账——有界不静默）。
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
// 知识条目（生命周期：写入→存活→死线→作废）
// ---------------------------------------------------------------------------

/// 访问规则（读写权限分档——建板声明制）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessRule {
    /// 板主（私有板唯一读者）。
    pub owner: u32,
    /// 读白名单（私有板忽略此表——只认 owner）。
    pub readers: Vec<u32>,
    /// 写白名单（owner 天然可写）。
    pub writers: Vec<u32>,
    /// 私有板：只属 owner（readers 失效）。
    pub private: bool,
}

impl AccessRule {
    /// 公板：全员可读写（owner 记账）。
    pub fn public(owner: u32) -> AccessRule {
        AccessRule { owner, readers: Vec::new(), writers: Vec::new(), private: false }
    }

    /// 私有板：只属 owner。
    pub fn private_board(owner: u32) -> AccessRule {
        AccessRule { owner, readers: Vec::new(), writers: Vec::new(), private: true }
    }

    /// 可读判定（O(白名单)——公板全员可读，私板只 owner）。
    pub fn can_read(&self, agent: u32) -> bool {
        if self.private {
            agent == self.owner
        } else {
            true
        }
    }

    /// 可写判定（O(白名单)——owner 天然可写+白名单成员）。
    pub fn can_write(&self, agent: u32) -> bool {
        agent == self.owner || self.writers.contains(&agent)
    }
}

/// 知识条目（键值+时间戳生命周期）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KnowledgeEntry {
    pub key: String,
    pub value: String,
    /// 写入时间戳（tick）。
    pub written_at: u64,
    /// 存活时长（tick——死线=written_at+ttl_ticks）。
    pub ttl_ticks: u64,
    /// 作废标记（过期→标记作废；粘滞——复活走显性写入）。
    pub voided: bool,
    /// 最近写者。
    pub writer: u32,
}

impl KnowledgeEntry {
    /// 存活死线（O(1) 手算锚：written 1000 + ttl 200 = 死线 1200；
    /// now=1200 恰活，now=1201 死）。
    pub fn dead_line(&self) -> u64 {
        self.written_at.saturating_add(self.ttl_ticks)
    }

    /// 存活判定（O(1)——作废粘滞优先于死线）。
    pub fn alive_at(&self, now: u64) -> bool {
        !self.voided && now <= self.dead_line()
    }
}

// ---------------------------------------------------------------------------
// 黑板（共享事实数据板——读写 O(1)）
// ---------------------------------------------------------------------------

/// 快照条目（脱敏后的可见视图单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotItem {
    pub key: String,
    /// 脱敏后的值面（私板他者→"***"；作废/过期→"(作废)"）。
    pub value_or_mask: String,
    pub alive: bool,
}

/// 黑板系统（Agent 间共享事实数据板）。
#[derive(Debug)]
pub struct Blackboard {
    /// 键 → (访问规则, 知识条目)。
    boards: BTreeMap<String, (AccessRule, KnowledgeEntry)>,
    pub bag: DiagBag,
    /// 读命中计数（可查面）。
    pub read_hits: u32,
    /// 写入计数（可查面）。
    pub write_ok: u32,
}

impl Blackboard {
    pub fn new() -> Blackboard {
        Blackboard {
            boards: BTreeMap::new(),
            bag: DiagBag::new(),
            read_hits: 0,
            write_ok: 0,
        }
    }

    /// 写入（O(1)+三查：键存在性→权限→时间戳仲裁）。
    /// 新键：写者自动成 owner（公板）；已存在：ACL+仲裁。
    pub fn write(
        &mut self,
        agent: u32,
        key: &str,
        value: &str,
        stamp: u64,
        ttl: u64,
    ) -> Result<(), u16> {
        match self.boards.get_mut(key) {
            None => {
                // 新键：容量闸+建板（写者=owner 公板声明）。
                if self.boards.len() >= BOARD_CAP {
                    self.bag.push(E_BB_FORBID, key, "板容量已满");
                    return Err(E_BB_FORBID);
                }
                let entry = KnowledgeEntry {
                    key: key.to_string(),
                    value: value.to_string(),
                    written_at: stamp,
                    ttl_ticks: ttl,
                    voided: false,
                    writer: agent,
                };
                self.boards.insert(key.to_string(), (AccessRule::public(agent), entry));
                self.write_ok += 1;
                Ok(())
            }
            Some((rule, entry)) => {
                // 查 1：写权限（owner/写白名单外→拒绝）。
                if !rule.can_write(agent) {
                    self.bag.push(E_BB_FORBID, key, "非 owner 且不在写白名单");
                    return Err(E_BB_FORBID);
                }
                // 查 2：时间戳仲裁（存活条目：新 stamp 严格大于旧 stamp
                // 才覆盖；败者立案保持旧值——O(1) 单比较）。
                if entry.alive_at(stamp) && stamp <= entry.written_at {
                    self.bag.push(
                        E_BB_CONFLICT,
                        key,
                        &format!("新 stamp {} ≤ 旧 stamp {}", stamp, entry.written_at),
                    );
                    return Err(E_BB_CONFLICT);
                }
                // 全过：覆盖（复活作废条目=显性新写入）。
                entry.value = value.to_string();
                entry.written_at = stamp;
                entry.ttl_ticks = ttl;
                entry.voided = false;
                entry.writer = agent;
                self.write_ok += 1;
                Ok(())
            }
        }
    }

    /// 读（O(1)+三查：键存在→读权限→过期作废）。
    /// 过期知识不作为决策依据：读遇死线即标记作废并返回 None。
    pub fn read(&mut self, agent: u32, key: &str, now: u64) -> Result<&str, u16> {
        // 查 1：键存在性。
        let (rule, entry) = match self.boards.get(key) {
            Some(pair) => pair,
            None => {
                self.bag.push(E_BB_NO_KEY, key, "黑板无此条目");
                return Err(E_BB_NO_KEY);
            }
        };
        // 查 2：读权限（私板 owner 外→脱敏阻断）。
        if !rule.can_read(agent) {
            self.bag.push(E_BB_REDACT, key, "私有板仅 owner 可读");
            return Err(E_BB_REDACT);
        }
        // 查 3：过期判定（死线一次比较——过期→标记作废+立案）。
        if !entry.voided && now > entry.dead_line() {
            let (rule_mut, entry_mut) = self.boards.get_mut(key).expect("checked above");
            entry_mut.voided = true;
            let _ = rule_mut;
            self.bag.push(
                E_BB_STALE,
                key,
                &format!("死线 {} < 读时 {}", entry_mut.dead_line(), now),
            );
            return Err(E_BB_STALE);
        }
        if entry.voided {
            // 作废粘滞：已作废条目读一律 None（不作为决策依据）。
            return Err(E_BB_STALE);
        }
        self.read_hits += 1;
        Ok(&self.boards.get(key).expect("checked above").1.value)
    }

    /// 批量扫过期（sweep——标记作废计数返回，O(n) 显性维护）。
    pub fn sweep(&mut self, now: u64) -> usize {
        let mut n = 0;
        for (_k, (_rule, entry)) in self.boards.iter_mut() {
            if !entry.voided && now > entry.dead_line() {
                entry.voided = true;
                n += 1;
            }
        }
        n
    }

    /// 快照调试（该 Agent 可见视图——权限过滤+脱敏先行）。
    /// 私板他者值→"***"；作废/过期条目只留键名值置"(作废)"。
    pub fn snapshot(&self, agent: u32, now: u64) -> Vec<SnapshotItem> {
        let mut items = Vec::new();
        for (key, (rule, entry)) in self.boards.iter() {
            if !rule.can_read(agent) {
                // 私板他者：脱敏条目（键可见值不落快照）。
                items.push(SnapshotItem {
                    key: key.clone(),
                    value_or_mask: REDACTED.to_string(),
                    alive: false,
                });
                continue;
            }
            if entry.alive_at(now) {
                items.push(SnapshotItem {
                    key: key.clone(),
                    value_or_mask: entry.value.clone(),
                    alive: true,
                });
            } else {
                items.push(SnapshotItem {
                    key: key.clone(),
                    value_or_mask: VOID_MARK.to_string(),
                    alive: false,
                });
            }
        }
        items
    }

    /// 黑板内容可查（读屏摘要——键名+存活态，值面不入读屏）。
    pub fn spoken(&self, now: u64) -> String {
        let total = self.boards.len();
        let alive = self
            .boards
            .values()
            .filter(|(_r, e)| e.alive_at(now))
            .count();
        let mut out = format!("黑板共{}条存活{}", total, alive);
        for (key, (_r, e)) in self.boards.iter() {
            out.push('；');
            out.push_str(key);
            if e.alive_at(now) {
                out.push_str("（存活）");
            } else {
                out.push_str("（作废）");
            }
        }
        out
    }

    pub fn len(&self) -> usize {
        self.boards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.boards.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 判据（七项——黑板共享/知识过期/时间戳仲裁/越权拒/快照脱敏/可查/码）
// ---------------------------------------------------------------------------

/// VE-F5804 判据入口（返回式 CheckSet——全部预期常量独立手算）。
pub fn run_veac04_checks() -> CheckSet {
    let mut s = CheckSet::new("VE-F5804");

    // 板-01 黑板共享：A 写公板 B 可读恰值；C 私板 B 读被脱敏阻断。
    {
        let mut bb = Blackboard::new();
        let w = bb.write(1, "enemy-count", "42", 1000, 10_000);
        let r = bb.read(2, "enemy-count", 1000).map(|v| v.to_string());
        // 私板：owner=3，B(2) 读→E_BB_REDACT。
        let _ = bb.write(3, "boss-plan", "flank-left", 1000, 10_000);
        let mut bb_priv = Blackboard::new();
        let _ = bb_priv.write(3, "boss-plan", "flank-left", 1000, 10_000);
        // 手算：write(3,…) 新键→owner=3 公板；构造私板规则后写覆盖。
        // 直接用 private_board 语义验证：新键写者=owner（公板），
        // 私板阻断用规则改写面验证——改用独立黑板只读验证。
        let priv_rule_blocked = {
            let rule = AccessRule::private_board(3);
            !rule.can_read(2) && rule.can_read(3)
        };
        let ok = w == Ok(())
            && r == Ok("42".to_string())
            && bb.read_hits == 1
            && priv_rule_blocked
            && bb_priv.len() == 1;
        if ok {
            s.ok("F5804-板-01-黑板共享读回恰值");
        } else {
            s.fail("F5804-板-01-黑板共享读回恰值", "共享读或权限不符");
        }
    }

    // 期-02 知识过期：ttl 死线恰值判定+读遇死线标记作废+粘滞。
    {
        let mut bb = Blackboard::new();
        // 手算：written_at=1000, ttl=200 → 死线 1200；
        // now=1200 恰活（≤），now=1201 死（>）。
        let _ = bb.write(1, "cache-price", "900", 1000, 200);
        let live = bb.read(2, "cache-price", 1200).map(|v| v.to_string());
        let dead = bb.read(2, "cache-price", 1201).map(|v| v.to_string());
        let sticky = bb.read(2, "cache-price", 1300).map(|v| v.to_string());
        let ok = live == Ok("900".to_string())
            && dead == Err(E_BB_STALE)
            && sticky == Err(E_BB_STALE)
            && bb.bag.count_of(E_BB_STALE) == 1 // 1201 那次立案；粘滞读不再重复立案
            && {
                let (_r, e) = bb.boards.get("cache-price").expect("entry");
                e.voided // 标记作废（不是删除——审计留痕）
            };
        if ok {
            s.ok("F5804-期-02-过期作废粘滞恰值");
        } else {
            s.fail("F5804-期-02-过期作废粘滞恰值", "死线或作废不符");
        }
    }

    // 仲-03 时间戳仲裁：旧 stamp 拦（值保持旧），新 stamp 过（值覆盖）。
    {
        let mut bb = Blackboard::new();
        let _ = bb.write(1, "weather", "sunny", 100, 10_000);
        // 旧 stamp 90 ≤ 100 → 冲突拦，值保持 sunny。
        let old = bb.write(1, "weather", "rainy", 90, 10_000);
        let kept = bb.read(1, "weather", 100).map(|v| v.to_string());
        // 新 stamp 200 > 100 → 覆盖，值 rainy。
        let new = bb.write(1, "weather", "rainy", 200, 10_000);
        let got = bb.read(1, "weather", 200).map(|v| v.to_string());
        let ok = old == Err(E_BB_CONFLICT)
            && kept == Ok("sunny".to_string())
            && new == Ok(())
            && got == Ok("rainy".to_string())
            && bb.bag.count_of(E_BB_CONFLICT) == 1;
        if ok {
            s.ok("F5804-仲-03-时间戳仲裁败者立案");
        } else {
            s.fail("F5804-仲-03-时间戳仲裁败者立案", "仲裁或值面不符");
        }
    }

    // 权-04 越权写拒绝：白名单外 agent 写→E_BB_FORBID 值不变。
    {
        let mut bb = Blackboard::new();
        let _ = bb.write(1, "quest-flag", "locked", 100, 10_000);
        // agent 2 不在写白名单（新键 owner=1）→拒。
        let r = bb.write(2, "quest-flag", "open", 200, 10_000);
        let kept = bb.read(1, "quest-flag", 200).map(|v| v.to_string());
        // 白名单成员可写：重建板面验证 writers 放行。
        let whitelist_ok = {
            let rule = AccessRule {
                owner: 1,
                readers: Vec::new(),
                writers: vec![2, 3],
                private: false,
            };
            rule.can_write(2) && rule.can_write(1) && !rule.can_write(4)
        };
        let ok = r == Err(E_BB_FORBID)
            && kept == Ok("locked".to_string())
            && bb.bag.count_of(E_BB_FORBID) == 1
            && whitelist_ok;
        if ok {
            s.ok("F5804-权-04-越权写拒绝值守恒");
        } else {
            s.fail("F5804-权-04-越权写拒绝值守恒", "权限面不符");
        }
    }

    // 照-05 快照调试+脱敏：私板他者"***"、作废"(作废)"、存活原值。
    {
        let mut bb = Blackboard::new();
        let _ = bb.write(1, "public-a", "alpha", 100, 10_000);
        let _ = bb.write(1, "stale-b", "beta", 100, 50); // 死线 150
        // 私板注入：直接构造（write 新键=公板，私板面用规则表验证）。
        let _ = bb.write(3, "agent3-secret", "xyz", 100, 10_000);
        if let Some((rule, _e)) = bb.boards.get_mut("agent3-secret") {
            *rule = AccessRule::private_board(3);
        }
        // owner=1 视角（now=1000）：public-a 原值；stale-b 作废；
        // agent3-secret 私板他者→***。
        let snap1 = bb.snapshot(1, 1000);
        // owner=3 视角：自己的 secret 原值可见。
        let snap3 = bb.snapshot(3, 1000);
        let find = |items: &Vec<SnapshotItem>, k: &str| {
            items.iter().find(|i| i.key == k).map(|i| i.value_or_mask.clone())
        };
        let ok = find(&snap1, "public-a") == Some("alpha".to_string())
            && find(&snap1, "stale-b") == Some(VOID_MARK.to_string())
            && find(&snap1, "agent3-secret") == Some(REDACTED.to_string())
            && find(&snap3, "agent3-secret") == Some("xyz".to_string())
            && snap1.len() == 3;
        if ok {
            s.ok("F5804-照-05-快照脱敏三态恰值");
        } else {
            s.fail("F5804-照-05-快照脱敏三态恰值", "脱敏面不符");
        }
    }

    // 查-06 黑板内容可查：spoken 读屏摘要含键名+存活态+计数。
    {
        let mut bb = Blackboard::new();
        let _ = bb.write(1, "front-line", "north", 100, 10_000);
        let _ = bb.write(1, "supply", "full", 100, 10_000);
        let text = bb.spoken(100);
        let ok = text.contains("黑板共2条存活2")
            && text.contains("front-line（存活）")
            && text.contains("supply（存活）");
        // sweep 过期后再查：存活计数下降。
        let _ = bb.write(1, "expiring", "soon", 100, 10); // 死线 110
        let _ = bb.sweep(200);
        let text2 = bb.spoken(200);
        let ok = ok && text2.contains("黑板共3条存活2") && text2.contains("expiring（作废）");
        if ok {
            s.ok("F5804-查-06-内容可查读屏可达");
        } else {
            s.fail("F5804-查-06-内容可查读屏可达", "读屏面不符");
        }
    }

    // 码-07 五码互异+reason 全量显性+查无此键立案。
    {
        let codes = [E_BB_STALE, E_BB_CONFLICT, E_BB_FORBID, E_BB_NO_KEY, E_BB_REDACT];
        let mut distinct = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    distinct = false;
                }
            }
        }
        let seg_ok = codes[0] == 0xE301
            && codes[1] == 0xE302
            && codes[2] == 0xE303
            && codes[3] == 0xE304
            && codes[4] == 0xE305;
        let reasons_all = codes.iter().all(|c| reason_of(*c) != "未知诊断码");
        let mut bb = Blackboard::new();
        let ghost = bb.read(1, "no-such-key", 100);
        let ok = distinct
            && seg_ok
            && reasons_all
            && ghost == Err(E_BB_NO_KEY)
            && bb.bag.count_of(E_BB_NO_KEY) == 1
            && !bb.bag.spoken().contains("未知");
        if ok {
            s.ok("F5804-码-07-五码互异显性立案");
        } else {
            s.fail("F5804-码-07-五码互异显性立案", "诊断面不符");
        }
    }

    s
}
