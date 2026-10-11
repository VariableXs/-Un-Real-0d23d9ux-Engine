//! VE-F0008 · 资源句柄表与引用治理（VE-A 域 · 内核图形抽象层 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0008`
//!
//! **判据（锚点原文）**：统一句柄表、悬垂检测、复用清零、失效显性、判据；
//! 句柄失效的显性化含崩溃前拦截（悬垂使用在崩溃前检出并报告）；
//! 句柄表含生成号校验（复用句柄带代数戳——旧代句柄自动失效）；
//! 引用治理含泄漏责任人标注（谁借的谁还可归因）。
//!
//! **设计要点**：
//! - 统一句柄表：全 GPU 资源一个表，句柄 = (槽位, 代数戳) 二元组；
//!   跨域对接（AD02 同构）只认这个二元组，不认裸指针；
//! - 生成号校验：槽位复用时代数戳 +1，旧代句柄自动失效——
//!   "用已销毁资源"在悬垂成真之前就被代数戳拦住（崩溃前拦截的实质）；
//! - 悬垂使用 = 阻断级：`access()` 对死句柄/旧代句柄返回 Err 并留
//!   阻断级事件，绝不解引用——检出并报告，而不是崩给用户看；
//! - 复用清零：槽位复用前强制清零并断言（借用人表清空、计数归零）——
//!   复用污染（旧账带进新资源）按构建期缺陷处理；
//! - 泄漏责任人标注：借用按 owner 标签记账，谁借的谁还可归因，
//!   泄漏报告直接点名责任人。

use crate::checks::CheckSet;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、句柄与槽位
// ---------------------------------------------------------------------------

/// 句柄 = (槽位, 代数戳)。代数戳是悬垂防护的凭据。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    pub slot: u32,
    pub generation: u32,
}

impl Handle {
    pub fn raw(&self) -> u64 {
        (self.slot as u64) | ((self.generation as u64) << 32)
    }
}

/// 资源类别（统一表内的类别标签）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceKind {
    Texture,
    Buffer,
    Pipeline,
    Sampler,
    Swapchain,
}

impl ResourceKind {
    pub fn label(self) -> &'static str {
        match self {
            ResourceKind::Texture => "纹理",
            ResourceKind::Buffer => "缓冲",
            ResourceKind::Pipeline => "管线",
            ResourceKind::Sampler => "采样器",
            ResourceKind::Swapchain => "交换链",
        }
    }
}

/// 一条借用记录（责任人标注的最小单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BorrowRecord {
    /// 责任人标签（子系统名/服务名——谁借的谁还可归因）
    pub owner: String,
    pub refs: u32,
}

/// 槽位条目。
#[derive(Clone, Debug)]
pub struct SlotEntry {
    pub kind: ResourceKind,
    pub generation: u32,
    pub refcount: u32,
    /// 按责任人记账的借用表
    pub borrowers: Vec<BorrowRecord>,
    pub alive: bool,
    /// 创建时的标签（泄漏归因用）
    pub created_by: String,
}

/// 句柄错误（三要素 + 严重度）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleError {
    pub code: &'static str,
    /// 严重度：悬垂使用 = 阻断级（Blocked）
    pub severity: Severity,
    pub what: String,
    pub why: String,
    pub next: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 阻断级：悬垂使用/计数漂移（规格点名）
    Blocked,
    /// 拒绝级：参数错（不涉及悬垂）
    Rejected,
}

impl HandleError {
    pub fn is_complete(&self) -> bool {
        !self.what.is_empty() && !self.why.is_empty() && !self.next.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 二、句柄表
// ---------------------------------------------------------------------------

/// 统一句柄表。
pub struct HandleTable {
    slots: Vec<SlotEntry>,
    free: Vec<u32>,
    capacity: u32,
    /// 阻断级事件留痕（悬垂使用在崩溃前检出并报告的证据链）
    pub blocked_events: Vec<String>,
    /// 复用清零断言的违例数（必须恒零——违例即表实现缺陷）
    pub zeroing_violations: u64,
    pub created_total: u64,
    pub destroyed_total: u64,
}

impl HandleTable {
    pub fn new(capacity: u32) -> HandleTable {
        HandleTable {
            slots: Vec::new(),
            free: Vec::new(),
            capacity,
            blocked_events: Vec::new(),
            zeroing_violations: 0,
            created_total: 0,
            destroyed_total: 0,
        }
    }

    pub fn live_count(&self) -> usize {
        self.slots.iter().filter(|s| s.alive).count()
    }

    /// 槽位只读视图（域自检与读屏消费；写路径一律走方法）。
    pub fn slot_entry(&self, slot: u32) -> Option<&SlotEntry> {
        self.slots.get(slot as usize)
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 统一入口：分配句柄。优先复用空槽（代数戳 +1 + 强制清零）。
    pub fn acquire(
        &mut self,
        kind: ResourceKind,
        created_by: &str,
    ) -> Result<Handle, HandleError> {
        // 空槽复用：代数戳 +1（旧代句柄自动失效的机制点）
        if let Some(slot) = self.free.pop() {
            let gen = self.slots[slot as usize].generation + 1;
            let entry = &mut self.slots[slot as usize];
            // 复用清零断言：清零后的旧账必须为空（污染 = 表实现缺陷）
            if !entry.borrowers.is_empty() || entry.refcount != 0 {
                self.zeroing_violations += 1;
            }
            *entry = SlotEntry {
                kind,
                generation: gen,
                refcount: 0,
                borrowers: Vec::new(),
                alive: true,
                created_by: created_by.to_string(),
            };
            self.created_total += 1;
            return Ok(Handle {
                slot,
                generation: gen,
            });
        }
        if self.slots.len() as u32 >= self.capacity {
            return Err(HandleError {
                code: "E_TABLE_FULL",
                severity: Severity::Rejected,
                what: format!("句柄表满（容量 {}）", self.capacity),
                why: "句柄表容量是句柄炸弹的防线".to_string(),
                next: "先释放闲置句柄，或经评审扩容句柄表".to_string(),
            });
        }
        let slot = self.slots.len() as u32;
        self.slots.push(SlotEntry {
            kind,
            generation: 1,
            refcount: 0,
            borrowers: Vec::new(),
            alive: true,
            created_by: created_by.to_string(),
        });
        self.created_total += 1;
        Ok(Handle {
            slot,
            generation: 1,
        })
    }

    /// 槽位校验（统一出口）：活着 + 代数戳匹配。任何不匹配都按悬垂处置。
    fn validate(&mut self, h: &Handle, op: &str) -> Result<usize, HandleError> {
        let idx = h.slot as usize;
        if idx >= self.slots.len() {
            let e = HandleError {
                code: "E_HANDLE_RANGE",
                severity: Severity::Blocked,
                what: format!(
                    "操作 {} 的句柄槽位 {} 越出表范围 {}",
                    op,
                    h.slot,
                    self.slots.len()
                ),
                why: "越界槽位不是合法句柄——解引用就是越界访问，阻断在崩溃前"
                    .to_string(),
                next: "核对句柄来源；伪造/损坏的句柄按安全事件上报".to_string(),
            };
            self.blocked_events
                .push(format!("[阻断] {}: {}", op, e.what));
            return Err(e);
        }
        let entry = &self.slots[idx];
        if !entry.alive || entry.generation != h.generation {
            let e = HandleError {
                code: "E_HANDLE_DANGLING",
                severity: Severity::Blocked,
                what: format!(
                    "操作 {} 命中悬垂句柄（槽 {}，代 {} 对 {}，{}）",
                    op,
                    h.slot,
                    h.generation,
                    entry.generation,
                    if entry.alive { "代数戳不符" } else { "资源已销毁" }
                ),
                why: "用已销毁资源或旧代句柄 = 阻断级缺陷——本表在解引用之前\
用代数戳拦截，悬垂使用到不了崩溃点".to_string(),
                next: "检查持有方是否漏了销毁通知；销毁方应广播失效事件".to_string(),
            };
            self.blocked_events
                .push(format!("[阻断] {}: {}", op, e.what));
            return Err(e);
        }
        Ok(idx)
    }

    /// 悬垂检测的读侧入口：访问句柄（校验代数戳与存活，崩溃前拦截）。
    pub fn access(&mut self, h: &Handle) -> Result<ResourceKind, HandleError> {
        let idx = self.validate(h, "access")?;
        Ok(self.slots[idx].kind)
    }

    /// 引用计数 +1（持有）。
    pub fn retain(&mut self, h: &Handle) -> Result<u32, HandleError> {
        let idx = self.validate(h, "retain")?;
        self.slots[idx].refcount += 1;
        Ok(self.slots[idx].refcount)
    }

    /// 引用计数 -1。减到负数方向 = 计数漂移（审计留痕，阻断级）。
    pub fn release(&mut self, h: &Handle) -> Result<u32, HandleError> {
        let idx = self.validate(h, "release")?;
        let entry = &mut self.slots[idx];
        if entry.refcount == 0 {
            let e = HandleError {
                code: "E_REF_DRIFT",
                severity: Severity::Blocked,
                what: format!("槽 {} 引用计数已经为 0 还在 release", h.slot),
                why: "计数漂移说明有人多还了或不该还——账面对不上，后面必然泄漏或悬垂"
                    .to_string(),
                next: "审计 retain/release 配对（审计记录在 borrow 表）".to_string(),
            };
            self.blocked_events
                .push(format!("[阻断] release: {}", e.what));
            return Err(e);
        }
        entry.refcount -= 1;
        Ok(entry.refcount)
    }

    /// 按责任人借用（泄漏归因：谁借的记谁）。
    pub fn borrow(&mut self, h: &Handle, owner: &str) -> Result<u32, HandleError> {
        let idx = self.validate(h, "borrow")?;
        let entry = &mut self.slots[idx];
        entry.refcount += 1;
        match entry.borrowers.iter_mut().find(|b| b.owner == owner) {
            Some(b) => b.refs += 1,
            None => entry.borrowers.push(BorrowRecord {
                owner: owner.to_string(),
                refs: 1,
            }),
        }
        Ok(entry.refcount)
    }

    /// 按责任人归还（谁借的谁还可——别人代还显性拒绝）。
    ///
    /// 借用记录归零后**保留**（不立即移除）——这样"还多于借"的漂移
    /// 才能被检出（E_GIVEBACK_OVER），而不是退化成"从未借过"。
    pub fn give_back(&mut self, h: &Handle, owner: &str) -> Result<u32, HandleError> {
        let idx = self.validate(h, "give_back")?;
        let entry = &mut self.slots[idx];
        match entry.borrowers.iter_mut().find(|b| b.owner == owner) {
            Some(b) if b.refs > 0 => {
                b.refs -= 1;
                entry.refcount = entry.refcount.saturating_sub(1);
                Ok(entry.refcount)
            }
            Some(_) => Err(HandleError {
                code: "E_GIVEBACK_OVER",
                severity: Severity::Blocked,
                what: format!("责任人 {} 对槽 {} 的归还数超过借入数", owner, h.slot),
                why: "还多于借是计数漂移的又一种形态".to_string(),
                next: "审计该责任人的 borrow/give_back 配对".to_string(),
            }),
            None => Err(HandleError {
                code: "E_GIVEBACK_UNKNOWN",
                severity: Severity::Rejected,
                what: format!("责任人 {} 从未借过槽 {}，无从归还", owner, h.slot),
                why: "借还必须同主——代还不允许（归因会断）".to_string(),
                next: "核对责任人标签；确系代还走正式转移接口".to_string(),
            }),
        }
    }

    /// 销毁：槽位标记死亡（代数戳不变——复用时才 +1）。
    pub fn destroy(&mut self, h: &Handle) -> Result<(), HandleError> {
        let idx = self.validate(h, "destroy")?;
        let entry = &mut self.slots[idx];
        entry.alive = false;
        entry.refcount = 0;
        entry.borrowers.clear();
        self.free.push(h.slot);
        self.destroyed_total += 1;
        Ok(())
    }

    /// 泄漏报告：活着且仍有未还借用的逐条点名责任人（谁借的谁还可归因）。
    pub fn leak_report(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (i, s) in self.slots.iter().enumerate() {
            if !s.alive {
                continue;
            }
            let owners: Vec<String> = s
                .borrowers
                .iter()
                .filter(|b| b.refs > 0)
                .map(|b| format!("{}×{}", b.owner, b.refs))
                .collect();
            if owners.is_empty() {
                continue;
            }
            out.push(format!(
                "槽 {}（{}，创建人 {}）仍被借用：{}",
                i,
                s.kind.label(),
                s.created_by,
                owners.join("、")
            ));
        }
        out
    }

    /// 读屏可达状态摘要。
    pub fn a11y_summary(&self) -> String {
        format!(
            "句柄表：在册 {} 项（累计创建 {}、销毁 {}），阻断级事件 {} 起，\
复用清零违例 {}（须恒零）",
            self.live_count(),
            self.created_total,
            self.destroyed_total,
            self.blocked_events.len(),
            self.zeroing_violations
        )
    }
}
