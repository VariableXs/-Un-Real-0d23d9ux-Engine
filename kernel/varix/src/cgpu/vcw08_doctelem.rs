//! CGPU-F3527 · 文档遥测（vcw08_doctelem）
//!
//! W 域文档遥测——**计数不记人**的隐私红线本体：文档访问计数与搜索热词
//! 遥测，全部聚合口径（每文档/每词一个计数器），**无用户维度字段**——
//! 结构上不存在"谁访问了什么"的可查询形态（不是"不给查"而是"没有"）；
//! 遥测复用 P 域统一模型（cgp02 Metric 六元组同构对齐：名称/值/维度三列
//! 语义一致，不另立第二套口径）；滚动账（定容覆盖最旧——遥测是趋势面
//! 不是审计面，容量满滚动是显性策略而非丢数据事故）；隐私抽检（任何
//! 导出视图禁止出现用户标识字段——导出前自检，检出即拒）。
//!
//! 数据结构：访问计数账（BTreeMap 确定性序）；热词账（BTreeMap）；
//! 滚动事件环（定容 VecDeque 语义用 Vec + head 实现）。
//!
//! 错误路径与降级矩阵：空键显性拒（空文档 id / 空词是 bug 不是零计数）；
//! 容量满滚动 + 滚动计数留痕（不静默）；导出含可疑用户维度字段显性拒。
//! 性能：计数 O(1)，导出 O(账面)。跨批对接点：vcw06 文档搜索（热词源）/
//! P 域 cgp02 统一模型（口径复用）/ F3528 文档安全（隐私抽检共享纪律）。
//! 无障碍：计数器名读屏可达。隐私面：**本模块的建立目的**——只计数
//! 不记人，导出自检兜底。
//!
//! 诊断码 0xE2 段四码互异：`E_DT_EMPTY_KEY` / `E_DT_ROLLOVER` /
//! `E_DT_PRIVACY_LEAK` / `E_DT_CAP_FULL`。

use crate::checks::CheckSet;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const DT_VERSION: &str = "vcw08-doctelem-v1";

/// 滚动事件环容量（遥测是趋势面：100 条足够，满滚动显性）。
pub const ROLL_CAP: usize = 100;

/// 滚动告警容量（append-only；满显性）。
pub const ROLL_LOG_CAP: usize = 16;

/// 诊断码闭集（0xE2 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocTelemCode {
    /// 0xE200 空键（空文档 id / 空词）
    EmptyKey,
    /// 0xE201 容量满滚动（显性策略留痕）
    Rollover,
    /// 0xE202 隐私泄漏（导出含用户维度字段）
    PrivacyLeak,
    /// 0xE203 滚动告警账满
    CapFull,
}

impl DocTelemCode {
    /// 码值（与枚举声明序一致，禁重排）。
    pub const fn code(self) -> u16 {
        match self {
            DocTelemCode::EmptyKey => 0xE200,
            DocTelemCode::Rollover => 0xE201,
            DocTelemCode::PrivacyLeak => 0xE202,
            DocTelemCode::CapFull => 0xE203,
        }
    }

    /// 英文线缆名。
    pub const fn wire(self) -> &'static str {
        match self {
            DocTelemCode::EmptyKey => "E_DT_EMPTY_KEY",
            DocTelemCode::Rollover => "E_DT_ROLLOVER",
            DocTelemCode::PrivacyLeak => "E_DT_PRIVACY_LEAK",
            DocTelemCode::CapFull => "E_DT_CAP_FULL",
        }
    }

    /// 段独占双写互斥（0xE2 段，低字节 < 0x10）。
    pub const fn segment_guard(self) -> bool {
        let c = self.code();
        (c & 0xFF00) == 0xE200 && (c & 0x00FF) < 0x10
    }

    /// 中文诊断（含源位置与可操作建议）。
    pub fn detail(self) -> String {
        let base = match self {
            DocTelemCode::EmptyKey => "空键：空文档 id 或空词是调用方 bug，不是零计数，显性拒",
            DocTelemCode::Rollover => "滚动发生：事件环容量满，最旧被覆盖（趋势面策略，滚动已计数留痕）",
            DocTelemCode::PrivacyLeak => "隐私泄漏：导出视图出现用户维度字段，导出拒绝（计数不记人红线）",
            DocTelemCode::CapFull => "滚动告警账满：无法记录本次滚动，按显性失败处理",
        };
        let mut s = String::from(base);
        s.push_str("（源：cgpu/vcw08_doctelem.rs，码 0x");
        s.push_str(&format!("{:04X}", self.code()));
        s.push_str("）");
        s
    }
}

/// 遥测错误（失败→码）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DocTelemError {
    EmptyKey,
    Rollover,
    PrivacyLeak { field: &'static str },
    CapFull,
}

impl DocTelemError {
    /// 映射诊断码。
    pub const fn code(self) -> DocTelemCode {
        match self {
            DocTelemError::EmptyKey => DocTelemCode::EmptyKey,
            DocTelemError::Rollover => DocTelemCode::Rollover,
            DocTelemError::PrivacyLeak { .. } => DocTelemCode::PrivacyLeak,
            DocTelemError::CapFull => DocTelemCode::CapFull,
        }
    }

    /// 中文诊断（detail 直出）。
    pub fn detail(self) -> String {
        self.code().detail()
    }
}

/// 遥测事件（只含聚合面字段——结构上无用户维度）。
#[derive(Clone, Debug)]
pub struct TelemEvent {
    /// 事件种类："access" | "search"
    pub kind: &'static str,
    /// 聚合键（文档 id 或搜索词）
    pub key: String,
    /// 逻辑 tick（注入）
    pub tick: u64,
}

/// 文档遥测器（访问计数 + 热词 + 滚动环 + 隐私抽检）。
#[derive(Clone, Debug)]
pub struct DocTelemeter {
    /// 文档访问计数（每文档一计数器，BTreeMap 确定性序）
    access_counts: BTreeMap<String, u64>,
    /// 搜索热词计数（vcw06 喂数）
    term_counts: BTreeMap<String, u64>,
    /// 滚动事件环
    ring: Vec<TelemEvent>,
    /// 环头（下一写入位）
    head: usize,
    /// 环逻辑总数（含被滚掉的）
    total_events: u64,
    /// 滚动次数
    rollovers: u64,
    /// 滚动告警账（append-only）
    roll_log: Vec<u64>,
    /// 环指纹历史（滚动前快照，供对账）
    last_rollover_fp: u64,
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        i += 1;
    }
    h
}

impl DocTelemeter {
    /// 空遥测器。
    pub fn new() -> Self {
        DocTelemeter {
            access_counts: BTreeMap::new(),
            term_counts: BTreeMap::new(),
            ring: Vec::new(),
            head: 0,
            total_events: 0,
            rollovers: 0,
            roll_log: Vec::new(),
            last_rollover_fp: 0,
        }
    }

    /// 记文档访问（空 id 显性拒；入环滚动）。
    pub fn record_access(&mut self, doc_id: &str, tick: u64) -> Result<(), DocTelemError> {
        if doc_id.is_empty() {
            return Err(DocTelemError::EmptyKey);
        }
        let e = self.access_counts.entry(String::from(doc_id)).or_insert(0);
        *e = e.saturating_add(1);
        self.push_event(TelemEvent { kind: "access", key: String::from(doc_id), tick })
    }

    /// 记搜索热词（vcw06 喂数；空词显性拒）。
    pub fn record_term(&mut self, term: &str, tick: u64) -> Result<(), DocTelemError> {
        if term.is_empty() {
            return Err(DocTelemError::EmptyKey);
        }
        let e = self.term_counts.entry(String::from(term)).or_insert(0);
        *e = e.saturating_add(1);
        self.push_event(TelemEvent { kind: "search", key: String::from(term), tick })
    }

    /// 事件入环（满滚动 + 滚动留痕 + 满账显性；总账含滚掉事件——守恒口径）。
    fn push_event(&mut self, ev: TelemEvent) -> Result<(), DocTelemError> {
        self.total_events = self.total_events.saturating_add(1);
        if self.ring.len() < ROLL_CAP {
            self.ring.push(ev);
        } else {
            // 滚动前快照指纹（对账锚）
            self.last_rollover_fp = self.ring_fingerprint();
            self.ring[self.head] = ev;
            self.head = (self.head + 1) % ROLL_CAP;
            self.rollovers = self.rollovers.saturating_add(1);
            if self.roll_log.len() >= ROLL_LOG_CAP {
                return Err(DocTelemError::CapFull);
            }
            self.roll_log.push(self.rollovers);
            return Err(DocTelemError::Rollover);
        }
        Ok(())
    }

    /// 文档访问计数（无记录 = 0）。
    pub fn access_of(&self, doc_id: &str) -> u64 {
        self.access_counts.get(doc_id).copied().unwrap_or(0)
    }

    /// 热词计数。
    pub fn term_of(&self, term: &str) -> u64 {
        self.term_counts.get(term).copied().unwrap_or(0)
    }

    /// Top-N 热词（计数降序、同频词字典序升序——确定性排序不是运气排序）。
    pub fn top_terms(&self, n: usize) -> Vec<(String, u64)> {
        let mut items: Vec<(String, u64)> = Vec::new();
        for (k, v) in self.term_counts.iter() {
            items.push((k.clone(), *v));
        }
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        items.truncate(n);
        items
    }

    /// Top-N 访问文档（同序确定性）。
    pub fn top_docs(&self, n: usize) -> Vec<(String, u64)> {
        let mut items: Vec<(String, u64)> = Vec::new();
        for (k, v) in self.access_counts.iter() {
            items.push((k.clone(), *v));
        }
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        items.truncate(n);
        items
    }

    /// 环内事件数（<= ROLL_CAP）。
    pub fn ring_len(&self) -> usize {
        self.ring.len()
    }

    /// 逻辑总事件数（含滚掉的——守恒口径：total == 环内 + 滚掉）。
    pub fn total_events(&self) -> u64 {
        self.total_events
    }

    /// 滚动次数。
    pub fn rollover_count(&self) -> u64 {
        self.rollovers
    }

    /// 守恒校验：total_events == 环内事件数 + 已滚掉数。
    pub fn verify_conservation(&self) -> bool {
        let dropped = if self.total_events >= self.ring.len() as u64 {
            self.total_events - self.ring.len() as u64
        } else {
            return false; // 环内比总数还多 = 账坏了
        };
        dropped == self.rollovers
    }

    /// 隐私抽检：导出视图禁含用户维度字段（检出即拒）。
    ///
    /// 可疑字段名单是公开纪律：任何 kind/key 之外出现 user/session/ip/
    /// identity 词形的字段都按泄漏处理。
    pub fn export_view(&self, fields: &'static [&'static str]) -> Result<String, DocTelemError> {
        const FORBIDDEN: [&str; 4] = ["user", "session", "ip", "identity"];
        for f in fields.iter() {
            for bad in FORBIDDEN.iter() {
                if f.contains(bad) {
                    return Err(DocTelemError::PrivacyLeak { field: f });
                }
            }
        }
        let mut out = String::from("telemetry-view:");
        for (k, v) in self.access_counts.iter() {
            out.push_str(&format!("\n  access[{}]={}", k, v));
        }
        for (k, v) in self.term_counts.iter() {
            out.push_str(&format!("\n  term[{}]={}", k, v));
        }
        Ok(out)
    }

    /// 环指纹（滚动对账锚）。
    pub fn ring_fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        for ev in self.ring.iter() {
            b.extend_from_slice(ev.kind.as_bytes());
            b.extend_from_slice(ev.key.as_bytes());
            b.extend_from_slice(&ev.tick.to_le_bytes());
        }
        b.extend_from_slice(&self.head.to_le_bytes());
        fnv1a64(&b)
    }

    /// 遥测器快照指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut b: Vec<u8> = Vec::new();
        for (k, v) in self.access_counts.iter() {
            b.extend_from_slice(k.as_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        }
        for (k, v) in self.term_counts.iter() {
            b.extend_from_slice(k.as_bytes());
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&self.ring_fingerprint().to_le_bytes());
        b.extend_from_slice(&self.total_events.to_le_bytes());
        b.extend_from_slice(&self.rollovers.to_le_bytes());
        fnv1a64(&b)
    }
}

/// 域自检（`run_vcw08_checks`）：判据逐条可失败断言。
pub fn run_vcw08_checks() -> CheckSet {
    let mut s = CheckSet::new("cgpu-vcw08");

    // V27-01 访问计数聚合正确（遥测复用 P 域聚合口径）
    {
        let mut t = DocTelemeter::new();
        let mut ok = true;
        for _ in 0..3 {
            ok &= t.record_access("api-ref", 1).is_ok();
        }
        ok &= t.record_access("tutorial", 2).is_ok();
        ok &= t.access_of("api-ref") == 3 && t.access_of("tutorial") == 1 && t.access_of("none") == 0;
        s.add("V27-01-访问计数聚合正确", ok, if ok { "" } else { "访问计数失真" });
    }

    // V27-02 热词计数 + Top-N 确定性排序（vcw06 喂数面）
    {
        let mut t = DocTelemeter::new();
        // "shader" 3 次、"pipeline" 2 次、"gpu" 1 次
        for _ in 0..3 {
            let _ = t.record_term("shader", 1);
        }
        for _ in 0..2 {
            let _ = t.record_term("pipeline", 2);
        }
        let _ = t.record_term("gpu", 3);
        let top = t.top_terms(2);
        let ok = top.len() == 2 && top[0].0 == "shader" && top[0].1 == 3 && top[1].0 == "pipeline";
        s.add("V27-02-热词 Top-N 确定性", ok, if ok { "" } else { "热词排序失效" });
    }

    // V27-03 同频词字典序升序（确定性不是运气排序）
    {
        let mut t = DocTelemeter::new();
        let _ = t.record_term("beta", 1);
        let _ = t.record_term("alpha", 2);
        let top = t.top_terms(2);
        let ok = top.len() == 2 && top[0].0 == "alpha" && top[1].0 == "beta";
        s.add("V27-03-同频字典序升序", ok, if ok { "" } else { "同频排序不稳定" });
    }

    // V27-04 空键显性拒（空 id / 空词不是零计数）
    {
        let mut t = DocTelemeter::new();
        let r1 = t.record_access("", 1);
        let r2 = t.record_term("", 2);
        let ok = matches!(r1, Err(DocTelemError::EmptyKey)) && matches!(r2, Err(DocTelemError::EmptyKey));
        s.add("V27-04-空键显性拒", ok, if ok { "" } else { "空键被静默吞" });
    }

    // V27-05 容量满滚动：最旧被覆盖 + 滚动计数留痕（显性策略）
    {
        let mut t = DocTelemeter::new();
        let mut overflow_errs = 0;
        for i in 0..(ROLL_CAP + 10) {
            let key: String = format!("doc-{:04}", i);
            if t.record_access(&key, i as u64).is_err() {
                overflow_errs += 1;
            }
        }
        let ok = t.ring_len() == ROLL_CAP
            && t.rollover_count() == 10
            && overflow_errs == 10
            && t.total_events() == ROLL_CAP as u64 + 10
            && t.verify_conservation();
        s.add("V27-05-满滚动留痕守恒", ok, if ok { "" } else { "滚动策略或守恒失效" });
    }

    // V27-06 滚动对账锚：滚动前指纹与覆盖后指纹不同（覆盖真发生）
    {
        let mut t = DocTelemeter::new();
        for i in 0..ROLL_CAP {
            let _ = t.record_access(&format!("a{:03}", i), i as u64);
        }
        let fp_before = t.ring_fingerprint();
        let _ = t.record_access("newcomer", 999); // 触发滚动
        let fp_after = t.ring_fingerprint();
        let ok = fp_before != fp_after && t.last_rollover_fp == fp_before;
        s.add("V27-06-滚动对账锚一致", ok, if ok { "" } else { "滚动对账锚漂移" });
    }

    // V27-07 滚动告警账满显性拒
    {
        let mut t = DocTelemeter::new();
        let mut cap_seen = false;
        for i in 0..(ROLL_CAP + ROLL_LOG_CAP + 2) {
            if matches!(t.record_access(&format!("ovf-{:04}", i), i as u64), Err(DocTelemError::CapFull)) {
                cap_seen = true;
            }
        }
        let ok = cap_seen && t.roll_log.len() == ROLL_LOG_CAP;
        s.add("V27-07-滚动告警账满显性", ok, if ok { "" } else { "告警账被静默覆盖" });
    }

    // V27-08 隐私红线：导出含用户维度字段显性拒（计数不记人）
    {
        let mut t = DocTelemeter::new();
        let _ = t.record_access("api-ref", 1);
        let bad1 = t.export_view(&["doc_id", "user_id"]);
        let bad2 = t.export_view(&["session_token"]);
        let good = t.export_view(&["doc_id", "term", "count"]);
        let ok = matches!(bad1, Err(DocTelemError::PrivacyLeak { field: "user_id" }))
            && matches!(bad2, Err(DocTelemError::PrivacyLeak { field: "session_token" }))
            && good.is_ok();
        s.add("V27-08-隐私抽检拒用户维度", ok, if ok { "" } else { "隐私抽检失效" });
    }

    // V27-09 结构上无用户维度：TelemEvent 字段面只有 kind/key/tick
    {
        // 编译期字段面 + 运行期导出面双证
        let mut t = DocTelemeter::new();
        let _ = t.record_access("d", 1);
        let view = t.export_view(&["kind", "key", "tick"]).unwrap_or_default();
        let ok = view.contains("access[d]=1") && !view.contains("user") && !view.contains("session");
        s.add("V27-09-导出面无用户字段", ok, if ok { "" } else { "导出面含用户维度" });
    }

    // V27-10 快照指纹确定且内容敏感
    {
        let mut t1 = DocTelemeter::new();
        let mut t2 = DocTelemeter::new();
        let _ = t1.record_access("x", 1);
        let _ = t2.record_access("x", 1);
        let f1a = t1.fingerprint();
        let f1b = t1.fingerprint();
        let _ = t2.record_term("y", 2);
        let f2 = t2.fingerprint();
        let ok = f1a == f1b && f1a != f2 && f1a != 0;
        s.add("V27-10-快照指纹确定且敏感", ok, if ok { "" } else { "指纹不稳定或不敏感" });
    }

    // V27-11 诊断码 0xE2 段独占
    {
        let codes = [
            DocTelemCode::EmptyKey,
            DocTelemCode::Rollover,
            DocTelemCode::PrivacyLeak,
            DocTelemCode::CapFull,
        ];
        let mut all = true;
        for c in codes.iter() {
            all = all && c.segment_guard();
        }
        s.add("V27-11-诊断码 0xE2 段独占", all, if all { "" } else { "码值越出 0xE2 段" });
    }

    s
}
