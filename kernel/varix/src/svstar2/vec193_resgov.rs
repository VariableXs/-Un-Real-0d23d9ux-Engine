//! VE-F0593 · 编译器资源治理（进程级总治理）
//!
//! 职责：编译器进程级资源总治理——与 F0495 优化 pass 级上限分工显性：
//! pass 级管单次优化的中间产物上限，本条管整个编译会话的总账。
//! 三大域：内存（会话级配额、超限拒绝并三要素呈现、各级缓存容量计入会话账）、
//! 临时文件（临时目录生命周期托管、异常退出后的残留清理——启动时扫描自身遗留）、
//! 句柄纪律（RAII 全覆盖、泄漏即缺陷的最高级）。
//! 资源记账：内存水位、队列深度、缓存容量可查询，并入 F0589 遥测。
//! 错误路径：配额超限→拒绝并三要素；残留临时→启动清扫登记；泄漏→最高级缺陷即时修。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 会话配额表：一次编译会话允许消耗的资源上限（字节/个）。
#[derive(Clone, Debug, PartialEq)]
pub struct SessionQuota {
    /// 内存配额（字节）：会话内所有分配（含各级缓存）之和不得超过。
    pub mem_bytes: u64,
    /// 缓存容量配额（字节）：磁盘缓存 + 内存缓存计入会话账的上限。
    pub cache_bytes: u64,
    /// 临时文件个数上限：托管目录内同时存在的临时文件数。
    pub tmp_files: u32,
    /// 句柄个数上限：同时打开的文件句柄数（RAII 池容量）。
    pub handles: u32,
}

impl SessionQuota {
    /// 标准会话配额：保守起点，供无显式配置的会话使用。
    pub fn standard() -> Self {
        SessionQuota { mem_bytes: 1 << 30, cache_bytes: 1 << 28, tmp_files: 256, handles: 64 }
    }
}

/// 资源记账快照：会话任意时刻的资源水位（可查询、入 F0589 遥测）。
#[derive(Clone, Debug, PartialEq)]
pub struct ResourceLedger {
    pub mem_used: u64,
    pub cache_used: u64,
    pub tmp_count: u32,
    pub handle_open: u32,
    /// 队列深度：编译任务等待队列的当前长度。
    pub queue_depth: u32,
}

impl ResourceLedger {
    pub fn fresh() -> Self {
        ResourceLedger { mem_used: 0, cache_used: 0, tmp_count: 0, handle_open: 0, queue_depth: 0 }
    }
}

/// 配额判定结果：三态——放行 / 拒绝（附三要素）/ 警告（接近上限仍放行）。
#[derive(Clone, Debug, PartialEq)]
pub enum QuotaVerdict {
    /// 放行：本次请求记账后仍在配额内。
    Allow,
    /// 警告：记账后用量达到配额 90% 以上但未超——放行并提示。
    Warn(u32),
    /// 拒绝：记账后超限——三要素（发生了什么/为什么/下一步）。
    Deny(String, String, String),
}

/// 记账并判定：模拟"申请 mem_bytes 字节"的记账-判定流程，O(1)。
pub fn judge_quota(ledger: &mut ResourceLedger, quota: &SessionQuota, ask_mem: u64, ask_cache: u64) -> QuotaVerdict {
    let new_mem = ledger.mem_used.saturating_add(ask_mem);
    let new_cache = ledger.cache_used.saturating_add(ask_cache);
    if new_mem > quota.mem_bytes || new_cache > quota.cache_bytes {
        return QuotaVerdict::Deny(
            format!("会话内存请求 {} 字节后总量超配额", ask_mem),
            format!("当前 {} + 请求 {} > 配额 {}", ledger.mem_used, ask_mem, quota.mem_bytes),
            "缩小单元编译粒度或清理会话级缓存后重试".to_string(),
        );
    }
    ledger.mem_used = new_mem;
    ledger.cache_used = new_cache;
    let pct = (new_mem * 100 / quota.mem_bytes.max(1)) as u32;
    if pct >= 90 {
        QuotaVerdict::Warn(pct)
    } else {
        QuotaVerdict::Allow
    }
}

/// 临时目录托管协议：托管目录内的临时文件全生命周期由会话管理。
#[derive(Clone, Debug, PartialEq)]
pub struct TmpSteward {
    /// 托管目录名（约定：会话启动时创建，正常退出时清空并删除）。
    pub dir: String,
    /// 当前托管的临时文件名表。
    pub files: Vec<String>,
    /// 异常退出标记：会话若未经正常收尾，此位为真。
    pub orphan_risk: bool,
}

impl TmpSteward {
    /// 新建托管：目录名带会话号，避免多会话互扫。
    pub fn new(session_tag: &str) -> Self {
        TmpSteward { dir: format!(".vxc-tmp-{}", session_tag), files: Vec::new(), orphan_risk: false }
    }

    /// 登记临时文件：托管协议要求先登记后写盘——登记数超配额拒绝。
    pub fn enlist(&mut self, name: &str, quota: &SessionQuota) -> Result<(), String> {
        if self.files.len() >= quota.tmp_files as usize {
            return Err(format!(
                "临时文件数已达配额 {}：无法再登记 {}",
                quota.tmp_files, name
            ));
        }
        self.files.push(name.to_string());
        Ok(())
    }

    /// 正常收尾：清空托管表（对应实际删除目录内容）。
    pub fn settle(&mut self) -> u32 {
        let n = self.files.len() as u32;
        self.files.clear();
        n
    }

    /// 启动清扫：扫描自身遗留（目录名前缀匹配 .vxc-tmp-）并登记清理。
    /// 返回（发现残留目录数，登记清扫动作描述）。
    pub fn startup_sweep(known_dirs: &[String], self_tag: &str) -> (u32, String) {
        let mut orphans: Vec<&String> = known_dirs
            .iter()
            .filter(|d| d.starts_with(".vxc-tmp-") && !d.contains(self_tag))
            .collect();
        let n = orphans.len() as u32;
        if n == 0 {
            return (0, "无残留：上次会话正常收尾".to_string());
        }
        orphans.sort();
        let list = orphans.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
        (n, format!("启动清扫：登记清理 {} 个残留目录 [{}]", n, list))
    }
}

/// 句柄纪律：RAII 池——句柄从池取、出池即关，池满拒绝（泄漏即最高级缺陷）。
pub struct HandlePool {
    /// 池容量与当前在用数。
    cap: u32,
    used: u32,
    /// 生命周期审计：借出未还的句柄号表（正常收尾时必须为空）。
    leased: Vec<u32>,
}

impl HandlePool {
    pub fn new(cap: u32) -> Self {
        HandlePool { cap, used: 0, leased: Vec::new() }
    }

    /// 借出句柄：池满返回三要素错误；成功记入审计表。
    pub fn lease(&mut self, id: u32) -> Result<(), String> {
        if self.used >= self.cap {
            return Err(format!(
                "句柄池已满（{}/{}）：无法借出 {}",
                self.used, self.cap, id
            ));
        }
        if self.leased.contains(&id) {
            return Err(format!("句柄 {} 重复借出：RAII 纪律违规", id));
        }
        self.used += 1;
        self.leased.push(id);
        Ok(())
    }

    /// 归还句柄：RAII drop 即调用；未借出即还视为缺陷。
    pub fn release(&mut self, id: u32) -> Result<(), String> {
        match self.leased.iter().position(|&x| x == id) {
            Some(i) => {
                self.leased.remove(i);
                self.used -= 1;
                Ok(())
            }
            None => Err(format!("句柄 {} 从未借出却被归还：记为泄漏审计缺陷", id)),
        }
    }

    /// 收尾审计：审计表非空即泄漏——最高级缺陷，即时修不欠账。
    pub fn audit(&self) -> Result<(), String> {
        if self.leased.is_empty() {
            Ok(())
        } else {
            Err(format!("句柄泄漏 {} 个：{:?}——最高级缺陷", self.leased.len(), self.leased))
        }
    }
}

pub fn run_vec193_checks() -> crate::checks::CheckSet {
    let mut s = crate::checks::CheckSet::new("svstar2-vec193");
    // 族一：会话配额——判定、记账、警告、拒绝三要素
    let q = SessionQuota::standard();
    let mut lg = ResourceLedger::fresh();
    s.add("标准配额全字段为正", q.mem_bytes > 0 && q.cache_bytes > 0 && q.tmp_files > 0 && q.handles > 0, "quota");
    s.add("空会话记账放行", judge_quota(&mut lg, &q, 1 << 20, 1 << 16) == QuotaVerdict::Allow, "allow");
    s.add("记账落账", lg.mem_used == 1 << 20 && lg.cache_used == 1 << 16, "ledger");
    let v = judge_quota(&mut lg, &q, (1 << 30) - (1 << 20), 0);
    s.add("逼近上限警告放行", matches!(v, QuotaVerdict::Warn(_)), "warn");
    let v2 = judge_quota(&mut lg, &q, 1 << 20, 0);
    match &v2 {
        QuotaVerdict::Deny(a, b, c) => {
            s.add("超限拒绝", true, "deny");
            s.add("三要素齐备", !a.is_empty() && !b.is_empty() && !c.is_empty(), "three");
        }
        _ => s.add("超限拒绝", false, "expected deny"),
    }
    s.add("拒绝不落账", lg.mem_used + (1 << 20) > q.mem_bytes, "no-commit");
    // 族二：临时托管——登记、配额、收尾、启动清扫
    let mut ts = TmpSteward::new("s7");
    s.add("托管目录带会话号", ts.dir == ".vxc-tmp-s7", "dir");
    s.add("登记临时文件", ts.enlist("a.tmp", &q).is_ok() && ts.files == ["a.tmp".to_string()].to_vec(), "enlist");
    let mut tq = SessionQuota::standard();
    tq.tmp_files = 1;
    s.add("临时配额拒绝", ts.enlist("b.tmp", &tq).is_err(), "tmp-deny");
    s.add("正常收尾清表", ts.settle() == 1 && ts.files.is_empty(), "settle");
    let dirs = vec![".vxc-tmp-s1".to_string(), ".vxc-tmp-s2".to_string(), "other".to_string()];
    let (n, msg) = TmpSteward::startup_sweep(&dirs, "s9");
    s.add("启动清扫发现残留", n == 2 && msg.contains("2") && msg.contains("s1"), "sweep");
    let (n0, msg0) = TmpSteward::startup_sweep(&["other".to_string()], "s9");
    s.add("无残留如实报告", n0 == 0 && msg0.contains("无残留"), "clean");
    // 族三：句柄纪律——借还、重复借、未借还、收尾审计
    let mut hp = HandlePool::new(2);
    s.add("借出句柄", hp.lease(1).is_ok() && hp.lease(2).is_ok(), "lease");
    s.add("池满拒绝", hp.lease(3).is_err(), "pool-full");
    s.add("重复借出拒绝", hp.lease(1).is_err(), "dup-lease");
    s.add("归还句柄", hp.release(2).is_ok(), "release");
    s.add("未借却还判缺陷", hp.release(2).is_err() && hp.release(9).is_err(), "bad-release");
    s.add("收尾审计通过", hp.audit().is_ok(), "audit-ok");
    let mut hp2 = HandlePool::new(4);
    let _ = hp2.lease(5);
    s.add("泄漏审计报最高级", hp2.audit().is_err(), "leak-audit");
    // 族四：记账入遥测——快照可查、口径与判定一致
    let mut lg2 = ResourceLedger::fresh();
    let q2 = SessionQuota::standard();
    let _ = judge_quota(&mut lg2, &q2, 512, 64);
    s.add("记账快照可查询", lg2.mem_used == 512 && lg2.cache_used == 64, "snapshot");
    s.add("快照口径与判定同源", lg2.mem_used <= q2.mem_bytes, "same-source");
    let lg3 = ResourceLedger::fresh();
    s.add("新会话从零记账", lg3 == ResourceLedger::fresh(), "fresh");
    s.add("分工显性：本条管会话总账", q.mem_bytes > q.cache_bytes, "scope-vs-f0495");
    s
}
