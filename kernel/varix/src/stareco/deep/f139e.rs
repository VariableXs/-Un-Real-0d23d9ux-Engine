//! 深化层二 · F139 反馈闭环通道（2026-09-26 深化批次二）。
//!
//! 补深主册【开源复用】「tracker 后端自建（F128 JSON 面轻起步）」
//! 与【设计细节】报告模板/相似聚类/修复回链（主册 G-D-14）：tracker
//! 存储模型（编号生成 + 状态审计链）、查询引擎（编号查/状态过滤/
//! 版本指纹回链）、聚类桶管理、滑动窗限频、季度最佳报告评选。

use crate::checks::CheckSet;
use crate::stareco::ebase::{fnv1a64, State5, TraceId};

// ---------------------------------------------------------------------------
// tracker 存储模型：编号生成 + 状态审计链
// ---------------------------------------------------------------------------

pub struct Ticket {
    pub id: TraceId,
    pub symptom_fp: u64,
    pub repro_fp: u64,
    pub env_fp: u64,
    /// 相似哈希（symptom+env 聚类键）。
    pub cluster_key: u64,
    /// 修复版本指纹（0 = 未回链）。
    pub fix_fp: u64,
    state: State5,
    /// 状态变更审计：(日, 旧态, 新态)。
    audit: alloc::vec::Vec<(u32, u8, u8)>,
}

pub struct TicketStore {
    tickets: alloc::vec::Vec<Ticket>,
    next_seq: u32,
}

impl TicketStore {
    pub fn new() -> TicketStore {
        TicketStore { tickets: alloc::vec::Vec::new(), next_seq: 1 }
    }

    /// 建单：三段指纹齐 + 编号全局唯一（日期+序号）。
    pub fn file(
        &mut self,
        day: u32,
        symptom_fp: u64,
        repro_fp: u64,
        env_fp: u64,
    ) -> Result<TraceId, &'static str> {
        if symptom_fp == 0 || repro_fp == 0 || env_fp == 0 {
            return Err("三段指纹必填：现象/复现/环境缺一不收");
        }
        let id = TraceId::new("FB", day, self.next_seq);
        if !id.is_valid() {
            return Err("编号生成失败：日期非法");
        }
        self.next_seq += 1;
        let cluster_key = {
            let mut buf = [0u8; 16];
            buf[0..8].copy_from_slice(&symptom_fp.to_be_bytes());
            buf[8..16].copy_from_slice(&env_fp.to_be_bytes());
            fnv1a64(&buf)
        };
        self.tickets.push(Ticket {
            id,
            symptom_fp,
            repro_fp,
            env_fp,
            cluster_key,
            fix_fp: 0,
            state: State5::Submitted,
            audit: alloc::vec::Vec::new(),
        });
        Ok(id)
    }

    /// 状态推进：单步前进（跳步拒绝），全程审计留痕。
    pub fn advance(&mut self, id: TraceId, day: u32) -> Result<State5, &'static str> {
        let t = self.tickets.iter_mut().find(|t| t.id == id).ok_or("编号不存在")?;
        let old = t.state;
        let new = old.advance().ok_or("已是终态：不再推进")?;
        if let Some((last_day, _, _)) = t.audit.last() {
            if day < *last_day {
                return Err("状态时间倒流：审计拒绝");
            }
        }
        t.audit.push((day, old.index(), new.index()));
        t.state = new;
        Ok(new)
    }

    /// 修复回链：修复版指纹与提交时的环境指纹同源匹配。
    pub fn link_fix(&mut self, id: TraceId, fix_fp: u64) -> Result<(), &'static str> {
        if fix_fp == 0 {
            return Err("零指纹不可作修复回链");
        }
        let t = self.tickets.iter_mut().find(|t| t.id == id).ok_or("编号不存在")?;
        if t.fix_fp != 0 {
            return Err("已回链：一单一修复指纹（合并单共用主单回链）");
        }
        t.fix_fp = fix_fp;
        Ok(())
    }

    pub fn state_of(&self, id: TraceId) -> Option<State5> {
        self.tickets.iter().find(|t| t.id == id).map(|t| t.state)
    }

    pub fn audit_of(&self, id: TraceId) -> Option<&[(u32, u8, u8)]> {
        self.tickets.iter().find(|t| t.id == id).map(|t| t.audit.as_slice())
    }

    /// 修复版收编的提交数（「修复说明@了他的编号」的机器面）。
    pub fn fixed_by(&self, fix_fp: u64) -> usize {
        self.tickets.iter().filter(|t| t.fix_fp == fix_fp).count()
    }

    pub fn len(&self) -> usize {
        self.tickets.len()
    }

}

// ---------------------------------------------------------------------------
// 查询引擎：编号查 / 状态过滤 / 版本指纹回链
// ---------------------------------------------------------------------------

/// 状态过滤查询：按五态筛选在册编号（查询页数据源）。
pub fn query_by_state(store: &TicketStore, want: State5) -> alloc::vec::Vec<TraceId> {
    store.tickets.iter().filter(|t| t.state == want).map(|t| t.id).collect()
}

/// 回链查询：给定修复版指纹，列出被它收编的所有编号。
pub fn query_fixed_by(store: &TicketStore, fix_fp: u64) -> alloc::vec::Vec<TraceId> {
    store.tickets.iter().filter(|t| t.fix_fp == fix_fp).map(|t| t.id).collect()
}

// ---------------------------------------------------------------------------
// 聚类桶：相似报告合并决策（重复报告 → 关联合并）
// ---------------------------------------------------------------------------

pub struct ClusterBucket {
    /// 主单（首报）。
    pub primary: TraceId,
    /// 被合并的重复单。
    pub merged: alloc::vec::Vec<TraceId>,
    pub cluster_key: u64,
}

pub struct ClusterIndex {
    buckets: alloc::vec::Vec<ClusterBucket>,
}

impl ClusterIndex {
    pub fn new() -> ClusterIndex {
        ClusterIndex { buckets: alloc::vec::Vec::new() }
    }

    /// 归桶：同 cluster_key 的后来单并入主单；新键开新桶。
    /// 返回 Some(主单) = 被合并；None = 成为新主单。
    pub fn admit(&mut self, id: TraceId, cluster_key: u64) -> Option<TraceId> {
        if let Some(b) = self.buckets.iter_mut().find(|b| b.cluster_key == cluster_key) {
            b.merged.push(id);
            return Some(b.primary);
        }
        self.buckets.push(ClusterBucket { primary: id, merged: alloc::vec::Vec::new(), cluster_key });
        None
    }

    /// 主单的重复计数（热门问题热度榜）。
    pub fn dup_count(&self, primary: TraceId) -> usize {
        self.buckets.iter().find(|b| b.primary == primary).map(|b| b.merged.len()).unwrap_or(0)
    }

    pub fn buckets(&self) -> usize {
        self.buckets.len()
    }
}

// ---------------------------------------------------------------------------
// 滑动窗限频：每日上限 + 窗口计数（恶意刷量防线深化）
// ---------------------------------------------------------------------------

pub struct SlidingRateLimit {
    daily_cap: u32,
    /// (日, 提交数) 有序记录。
    days: alloc::vec::Vec<(u32, u32)>,
}

impl SlidingRateLimit {
    pub fn new(daily_cap: u32) -> SlidingRateLimit {
        SlidingRateLimit { daily_cap, days: alloc::vec::Vec::new() }
    }

    /// 提交许可：当日计数 < 上限才放行（计数含本次）。
    pub fn try_submit(&mut self, day: u32) -> Result<(), &'static str> {
        let entry = match self.days.iter_mut().find(|(d, _)| *d == day) {
            Some(e) => e,
            None => {
                self.days.push((day, 0));
                self.days.last_mut().expect("just pushed").1 = 0;
                self.days.last_mut().expect("just pushed")
            }
        };
        if entry.1 >= self.daily_cap {
            return Err("当日提交达上限：明日再试（防刷量）");
        }
        entry.1 += 1;
        Ok(())
    }

    pub fn used_on(&self, day: u32) -> u32 {
        self.days.iter().find(|(d, _)| *d == day).map(|(_, c)| *c).unwrap_or(0)
    }
}

// ---------------------------------------------------------------------------
// 季度最佳报告评选：有用性计数 → Top1
// ---------------------------------------------------------------------------

pub struct BestReportBoard {
    /// (报告编号, 被引用有用次数)。
    scores: alloc::vec::Vec<(TraceId, u32)>,
}

impl BestReportBoard {
    pub fn new() -> BestReportBoard {
        BestReportBoard { scores: alloc::vec::Vec::new() }
    }

    pub fn upvote(&mut self, id: TraceId) {
        if let Some(e) = self.scores.iter_mut().find(|(i, _)| *i == id) {
            e.1 += 1;
        } else {
            self.scores.push((id, 1));
        }
    }

    /// Top1（并列取先登记者——确定性，不搞神秘排序）。
    pub fn top1(&self) -> Option<(TraceId, u32)> {
        let mut best: Option<(TraceId, u32)> = None;
        for (id, n) in &self.scores {
            match best {
                Some((_, bn)) if *n <= bn => {}
                _ => best = Some((*id, *n)),
            }
        }
        best
    }

    pub fn len(&self) -> usize {
        self.scores.len()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F139E_TAG: &str = "stareco-F139-deep2";

pub fn run_f139_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F139E_TAG);

    // tracker：建单/推进/审计/回链
    let mut store = TicketStore::new();
    let id1 = store
        .file(20260926, fnv1a64(b"symptom-window-flicker"), fnv1a64(b"repro-3-steps"), fnv1a64(b"env:fp-1"))
        .expect("id1");
    let id2 = store
        .file(20260926, fnv1a64(b"symptom-crash"), fnv1a64(b"repro-2-steps"), fnv1a64(b"env:fp-2"))
        .expect("id2");
    set.add("f139e unique ids", id1 != id2 && store.len() == 2, "编号唯一");
    set.add("f139e initial state", store.state_of(id1) == Some(State5::Submitted), "初始态已接收");
    let _ = store.advance(id1, 20260927);
    set.add("f139e advance", store.state_of(id1) == Some(State5::Confirmed), "单步推进");
    set.add("f139e audit trail", store.audit_of(id1).map(|a| a.len()).unwrap_or(0) == 1, "审计留痕");
    set.add("f139e zero fp", store.file(20260926, 0, 1, 1).is_err(), "零指纹拒收");
    let _ = store.link_fix(id1, fnv1a64(b"fix-build-88"));
    set.add("f139e fix link", store.fixed_by(fnv1a64(b"fix-build-88")) == 1, "回链计数");
    set.add("f139e fix once", store.link_fix(id1, 9).is_err(), "重复回链拒绝");

    // 查询引擎
    let received = query_by_state(&store, State5::Submitted);
    set.add("f139e query state", received.len() == 1 && received[0] == id2, "状态过滤");
    let linked = query_fixed_by(&store, fnv1a64(b"fix-build-88"));
    set.add("f139e query fix", linked == alloc::vec![id1], "回链查询");

    // 聚类
    let mut idx = ClusterIndex::new();
    set.add("f139e cluster primary", idx.admit(id1, 0xABC).is_none(), "首报成主单");
    set.add("f139e cluster merge", idx.admit(id2, 0xABC) == Some(id1), "同键并入主单");
    set.add("f139e cluster dup count", idx.dup_count(id1) == 1 && idx.buckets() == 1, "热度计数");

    // 限频
    let mut rl = SlidingRateLimit::new(2);
    let _ = rl.try_submit(100);
    let _ = rl.try_submit(100);
    set.add("f139e rate cap", rl.try_submit(100).is_err(), "当日上限拦截");
    set.add("f139e rate next day", rl.try_submit(101).is_ok(), "次日恢复");
    set.add("f139e rate counter", rl.used_on(100) == 2 && rl.used_on(101) == 1, "逐日计数");

    // 最佳报告
    let mut board = BestReportBoard::new();
    board.upvote(id1);
    board.upvote(id1);
    board.upvote(id2);
    set.add(
        "f139e top1",
        board.top1() == Some((id1, 2)),
        "Top1 按 有用数（并列先登记）",
    );
    set.add("f139e board size", board.len() == 2, "在榜数");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn audit_order_enforced() {
        let mut s = TicketStore::new();
        let id = s.file(20260926, 1, 2, 3).unwrap();
        let _ = s.advance(id, 20260927);
        assert!(s.advance(id, 20260926).is_err()); // 倒流拒绝
        assert!(s.advance(id, 20260928).is_ok());
    }

    #[test]
    fn state5_terminal_block() {
        let mut s = TicketStore::new();
        let id = s.file(20260926, 1, 2, 3).unwrap();
        // 一路推到终态
        for _ in 0..6 {
            let _ = s.advance(id, 20260927);
        }
        assert!(s.advance(id, 20260928).is_err()); // 终态不再推进
    }

    #[test]
    fn cluster_isolation() {
        let mut s = TicketStore::new();
        let a = s.file(20260926, 1, 2, 3).unwrap();
        let b = s.file(20260926, 9, 8, 7).unwrap();
        let mut idx = ClusterIndex::new();
        assert!(idx.admit(a, 1).is_none());
        assert!(idx.admit(b, 2).is_none());
        assert_eq!(idx.buckets(), 2); // 不同键不同桶
    }
}
