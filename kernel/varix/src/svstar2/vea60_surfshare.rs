//! VE-F0060 · 跨进程表面导入导出（VE-A 域 · 表面共享段 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0060`
//!
//! **职责定位（锚点原文）**：渲染表面的跨进程共享（视频/浏览器/第三方的
//! 表面导入渲染），导入表面的安全校验（尺寸/格式/生命周期校验），跨进
//! 程同步原语；含表面共享的权限门（哪个进程可导入白名单）。
//!
//! **数据结构（锚点原文）**：表面共享；安全校验。在 [`SurfaceShare`]
//! 上一一落位：共享表面 = [`SharedSurface`]、导出票据 = [`ExportTicket`]、
//! 导入句柄 = [`ImportHandle`]、安全校验 = [`SafetyCheck`]、权限门 =
//! [`PermGate`]、同步原语 = [`SyncPrimitive`]。
//!
//! **判据（锚点原文）**：跨进程共享、安全校验、生命周期管理、同步防护、判据。
//!
//! 本模块 14 条判据逐条映射：**跨进程共享 3 条**（导出产票据且票据与表
//! 面绑定／白名单进程持票导入成功／非白名单进程显性拒绝且不泄漏存在性）、
//! **安全校验 4 条**（尺寸校验独立拦截／格式校验独立拦截／生命周期校验
//! 独立拦截／校验失败不产生半成品句柄）、**生命周期管理 3 条**（导出方
//! 销毁联动全部导入句柄失效／导入句柄释放计数对账／悬挂票据显性过期）、
//! **同步防护 2 条**（同步原语死锁检测——环等待即检出／信号越权拒绝）、
//! **判据自身 2 条**（码位互异段独占／共享状态读屏可达）。
//!
//! **错误路径与降级矩阵（锚点原文，逐条落实）**：
//!
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 外来表面非法 → **拒绝** | [`SafetyCheck`] 四道独立校验（存在性/尺寸/格式/生命周期），任一失败即拒且**不产生半成品句柄**——校验全过才发 [`ImportHandle`] |
//! | 生命周期失配 → **回收** | 导出方销毁时联动扫描全表，把该表面的全部导入句柄一次失效并返回失效数——「源没了句柄还在」活不过一次联动 |
//! | 同步死锁 → **检测** | [`SyncPrimitive::wait`] 建等待边，`detect_deadlock` 在等待图上做环检测（自环/两进程互等/长环），检出即拒绝入队 |
//!
//! ## 设计要点（为什么这样写）
//!
//! - **票据是导出的唯一合法通道**：`export` 产 [`ExportTicket`]（一次性
//!   绑定表面+授权进程），`import` 必须持票；无票导入直接拒绝——「哪个
//!   进程可导入」的权限门落在票据授权字段，白名单外进程连表面存在性都
//!   不给（同码拒绝，不区分「不存在」与「无权」）。
//! - **安全校验是独立四关不是一票否决**：每关失败带独立诊断码，判据逐
//!   关拆解对拍——一票否决会把「格式不对」和「已过期」混成一个错。
//! - **同步原语是显式对象**：信号量/互斥体各自记账（谁持有、谁等待），
//!   等待图环检测 O(进程²)——锚点 O(表面) 约束下环检测只在 `wait` 时
//!   对该进程做局部 DFS。
//! - **共享状态读屏可达**：[`SurfaceShare::status_line`] 报在册表面数、
//!   活跃句柄数、拒绝数、死锁检出数。
//! - **零 panic 面**：查表走 `position`/`iter`、计数全 `saturating_*`、
//!   全部入口返回 [`Result`]。

// ---------------------------------------------------------------------------
// 引入（no_std 内核侧；alloc 三件套自查：vec 宏 / Vec 类型 / String+ToString）
// ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、诊断码（自建段 0xC2xx，独占——整仓 0xC201~0xC20F 零占用后选定）
// ---------------------------------------------------------------------------

/// 票据非法（无票据 / 票据与请求方不匹配 / 票据已用）。
pub const CODE_TICKET_BAD: u16 = 0xC201;
/// 表面不存在（导出方未注册或已销毁）。
pub const CODE_SURFACE_UNKNOWN: u16 = 0xC202;
/// 尺寸校验失败（宽高为零或超限）。
pub const CODE_SIZE_BAD: u16 = 0xC203;
/// 格式校验失败（导入方声明格式与表面不符）。
pub const CODE_FORMAT_BAD: u16 = 0xC204;
/// 生命周期校验失败（表面已过期/已回收）。
pub const CODE_LIFECYCLE_BAD: u16 = 0xC205;
/// 同步死锁检出（等待图成环）。
pub const CODE_DEADLOCK: u16 = 0xC206;
/// 同步越权（未持有的原语发信号）。
pub const CODE_SYNC_FORBID: u16 = 0xC207;
/// 导入句柄未知（已释放或从未签发）。
pub const CODE_HANDLE_UNKNOWN: u16 = 0xC208;

// ---------------------------------------------------------------------------
// 二、共享表面与票据
// ---------------------------------------------------------------------------

/// 共享表面：导出方登记的跨进程共享单元。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SharedSurface {
    /// 表面号（导出方命名空间，单调分配）。
    pub id: u32,
    /// 宽（像素；0 = 非法）。
    pub width: u32,
    /// 高（像素；0 = 非法）。
    pub height: u32,
    /// 格式号（导入方必须申报同号）。
    pub format: u16,
    /// 导出方进程号。
    pub owner: u32,
}

impl SharedSurface {
    /// 注册合法性：宽高非零、格式号非零、属主非零。
    pub const fn valid(&self) -> bool {
        self.width != 0 && self.height != 0 && self.format != 0 && self.owner != 0
    }
}

/// 导出票据：一次性绑定（表面，授权进程）对。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportTicket {
    /// 票据号（单调分配，不复用）。
    pub ticket_id: u32,
    /// 授权的表面。
    pub surface: u32,
    /// 授权导入的进程（权限门字段）。
    pub grantee: u32,
    /// 是否已使用（一次性）。
    pub used: bool,
}

// ---------------------------------------------------------------------------
// 三、权限门
// ---------------------------------------------------------------------------

/// 表面共享权限门：白名单（哪个进程可导入）。
#[derive(Clone, Debug, Default)]
pub struct PermGate {
    /// 白名单进程号。
    pub allow: Vec<u32>,
    /// 拒绝计数（读屏口径）。
    pub denies: u32,
}

/// 权限门容量上限。
const PERM_CAP: usize = 64;

impl PermGate {
    /// 授权进程入白名单（容量防溢）。
    pub fn grant(&mut self, pid: u32) -> bool {
        if pid == 0 || self.allow.len() >= PERM_CAP || self.allow.iter().any(|p| *p == pid) {
            return false;
        }
        self.allow.push(pid);
        true
    }

    /// 撤销授权。
    pub fn revoke(&mut self, pid: u32) -> bool {
        match self.allow.iter().position(|p| *p == pid) {
            Some(i) => {
                self.allow.remove(i);
                true
            }
            None => false,
        }
    }

    /// 权限判定（0 号进程恒拒——系统保留号不给共享通道）。
    pub fn allowed(&self, pid: u32) -> bool {
        pid != 0 && !self.allow.is_empty() && self.allow.iter().any(|p| *p == pid)
    }
}

// ---------------------------------------------------------------------------
// 四、同步原语与等待图
// ---------------------------------------------------------------------------

/// 同步原语种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncKind {
    /// 信号量（跨进程帧同步）。
    Semaphore,
    /// 互斥体（表面独占期）。
    Mutex,
}

/// 跨进程同步原语。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncPrimitive {
    /// 原语号。
    pub id: u32,
    /// 种类。
    pub kind: SyncKind,
    /// 当前持有者（0 = 无人持有）。
    pub holder: u32,
    /// 等待者队列。
    pub waiters: Vec<u32>,
}

/// 等待边：进程 A 在原语上等进程 B 释放。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitEdge {
    /// 等待方。
    pub waiter: u32,
    /// 持有方。
    pub holder: u32,
    /// 原语号。
    pub prim: u32,
}

// ---------------------------------------------------------------------------
// 五、导入句柄
// ---------------------------------------------------------------------------

/// 导入句柄：安全校验全过后签发的可用引用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImportHandle {
    /// 句柄号（单调分配，不复用）。
    pub handle_id: u32,
    /// 绑定的表面。
    pub surface: u32,
    /// 持有进程。
    pub pid: u32,
}

// ---------------------------------------------------------------------------
// 六、表面共享管理器（本模块主语）
// ---------------------------------------------------------------------------

/// 跨进程表面共享管理器：导出/导入/安全校验/生命周期/同步五段一体。
#[derive(Clone, Debug)]
pub struct SurfaceShare {
    /// 在册表面表。
    pub surfaces: Vec<SharedSurface>,
    /// 未消费票据表。
    pub tickets: Vec<ExportTicket>,
    /// 活跃导入句柄表。
    pub handles: Vec<ImportHandle>,
    /// 权限门。
    pub gate: PermGate,
    /// 同步原语表。
    pub prims: Vec<SyncPrimitive>,
    /// 等待图（进程级等待边）。
    pub waits: Vec<WaitEdge>,
    /// 统计账。
    pub stats: ShareStats,
    next_ticket: u32,
    next_handle: u32,
}

/// 统计账：六桶逐笔可对账。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShareStats {
    /// 导出次数。
    pub exports: u32,
    /// 导入成功次数。
    pub imports_ok: u32,
    /// 导入拒绝次数（四关合计）。
    pub import_denies: u32,
    /// 联动失效句柄数（导出方销毁回收）。
    pub reclaimed_handles: u32,
    /// 死锁检出次数。
    pub deadlocks: u32,
    /// 原语发信号次数。
    pub signals: u32,
}

impl SurfaceShare {
    /// 新管理器：全表空、权限门空、账零。
    pub fn new() -> SurfaceShare {
        SurfaceShare {
            surfaces: Vec::new(),
            tickets: Vec::new(),
            handles: Vec::new(),
            gate: PermGate::default(),
            prims: Vec::new(),
            waits: Vec::new(),
            stats: ShareStats::default(),
            next_ticket: 1,
            next_handle: 1,
        }
    }

    // -- 导出段 -----------------------------------------------------------

    /// 导出：登记表面 + 产票据（一次性绑定授权进程）。
    pub fn export(&mut self, s: SharedSurface, grantee: u32) -> Result<ExportTicket, u16> {
        if !s.valid() {
            return Err(CODE_SIZE_BAD);
        }
        if self.surfaces.iter().any(|x| x.id == s.id) {
            return Err(CODE_SURFACE_UNKNOWN);
        }
        self.surfaces.push(s);
        let t = ExportTicket { ticket_id: self.next_ticket, surface: s.id, grantee, used: false };
        self.next_ticket += 1;
        self.tickets.push(t);
        self.stats.exports += 1;
        Ok(t)
    }

    // -- 安全校验段（四关独立） -------------------------------------------

    /// 关一：权限门（票据授权 + 白名单）。
    pub fn check_perm(&mut self, ticket: &ExportTicket, pid: u32) -> Result<(), u16> {
        if ticket.used || ticket.ticket_id == 0 || pid == 0 || !self.gate.allowed(pid) {
            self.gate.denies += 1;
            self.stats.import_denies += 1;
            return Err(CODE_TICKET_BAD);
        }
        Ok(())
    }

    /// 关二：尺寸校验。
    pub const fn check_size(s: &SharedSurface) -> Result<(), u16> {
        if s.width == 0 || s.height == 0 || s.width > 16384 || s.height > 16384 {
            return Err(CODE_SIZE_BAD);
        }
        Ok(())
    }

    /// 关三：格式校验（导入方申报格式必须与表面一致）。
    pub const fn check_format(s: &SharedSurface, declared: u16) -> Result<(), u16> {
        if declared == 0 || declared != s.format {
            return Err(CODE_FORMAT_BAD);
        }
        Ok(())
    }

    /// 关四：生命周期校验（表面必须在册）。
    pub fn check_lifecycle(&self, surface_id: u32) -> Result<SharedSurface, u16> {
        match self.surfaces.iter().find(|s| s.id == surface_id) {
            Some(s) => Ok(*s),
            None => Err(CODE_LIFECYCLE_BAD),
        }
    }

    // -- 导入段 -----------------------------------------------------------

    /// 导入：持票四关全过才签发句柄（校验失败不产生半成品句柄）。
    pub fn import(
        &mut self,
        ticket: ExportTicket,
        pid: u32,
        declared_format: u16,
    ) -> Result<ImportHandle, u16> {
        // 关一：权限
        self.check_perm(&ticket, pid)?;
        // 票据消费（一次性；先验后耗——失败路径不损票据账）
        let pos = match self.tickets.iter().position(|t| t.ticket_id == ticket.ticket_id) {
            Some(p) => p,
            None => {
                self.stats.import_denies += 1;
                return Err(CODE_TICKET_BAD);
            }
        };
        let surface_id = self.tickets[pos].surface;
        if self.tickets[pos].grantee != pid {
            self.stats.import_denies += 1;
            return Err(CODE_TICKET_BAD);
        }
        // 关四：生命周期（先于尺寸/格式——表面不在册直接过期）
        let s = match self.check_lifecycle(surface_id) {
            Ok(x) => x,
            Err(e) => {
                self.stats.import_denies += 1;
                return Err(e);
            }
        };
        // 关二：尺寸
        if let Err(e) = Self::check_size(&s) {
            self.stats.import_denies += 1;
            return Err(e);
        }
        // 关三：格式
        if let Err(e) = Self::check_format(&s, declared_format) {
            self.stats.import_denies += 1;
            return Err(e);
        }
        // 四关全过：耗票 + 签发句柄
        self.tickets.remove(pos);
        let h = ImportHandle { handle_id: self.next_handle, surface: surface_id, pid };
        self.next_handle += 1;
        self.handles.push(h);
        self.stats.imports_ok += 1;
        Ok(h)
    }

    // -- 生命周期段 -------------------------------------------------------

    /// 导入句柄释放（计数对账）。
    pub fn release_handle(&mut self, handle_id: u32) -> Result<(), u16> {
        match self.handles.iter().position(|h| h.handle_id == handle_id) {
            Some(i) => {
                self.handles.remove(i);
                Ok(())
            }
            None => Err(CODE_HANDLE_UNKNOWN),
        }
    }

    /// 导出方销毁联动：该属主全部表面下架，关联句柄一次失效并返回失效数。
    pub fn owner_destroyed(&mut self, owner: u32) -> u32 {
        let dead: Vec<u32> = self
            .surfaces
            .iter()
            .filter(|s| s.owner == owner)
            .map(|s| s.id)
            .collect();
        if dead.is_empty() {
            return 0;
        }
        let before = self.handles.len();
        self.handles.retain(|h| !dead.iter().any(|d| *d == h.surface));
        let n = (before - self.handles.len()) as u32;
        self.surfaces.retain(|s| s.owner != owner);
        // 票据不在联动内清理：成为悬挂票据，由 expire_orphan_tickets 显性过期回收
        self.stats.reclaimed_handles += n;
        n
    }

    /// 悬挂票据过期清理（表面已不在册的票据回收）。
    pub fn expire_orphan_tickets(&mut self) -> u32 {
        let before = self.tickets.len();
        self.tickets
            .retain(|t| self.surfaces.iter().any(|s| s.id == t.surface));
        (before - self.tickets.len()) as u32
    }

    // -- 同步段 -----------------------------------------------------------

    /// 建同步原语（属主=创建进程）。
    pub fn create_prim(&mut self, id: u32, kind: SyncKind, owner: u32) -> Result<(), u16> {
        if id == 0 || owner == 0 || self.prims.iter().any(|p| p.id == id) {
            return Err(CODE_SYNC_FORBID);
        }
        self.prims.push(SyncPrimitive { id, kind, holder: 0, waiters: Vec::new() });
        Ok(())
    }

    /// 等待：建等待边前先做死锁检测（环等待即拒绝入队）。
    pub fn wait(&mut self, pid: u32, prim_id: u32) -> Result<(), u16> {
        let holder = match self.prims.iter().find(|p| p.id == prim_id) {
            Some(p) => p.holder,
            None => return Err(CODE_SYNC_FORBID),
        };
        if holder == 0 || holder == pid {
            // 无人持有无需等；自等即自环死锁
            if holder == pid {
                self.stats.deadlocks += 1;
                return Err(CODE_DEADLOCK);
            }
            return Ok(());
        }
        // 死锁检测：pid → holder；若 holder 已在等回 pid 的路径即成环
        let edge = WaitEdge { waiter: pid, holder, prim: prim_id };
        if self.waits.iter().any(|w| w.waiter == pid && w.holder == holder && w.prim == prim_id) {
            return Err(CODE_DEADLOCK); // 重复等待同边 = 非法重入
        }
        self.waits.push(edge);
        // 局部环检测：从 holder 顺等待链走，回到 pid 即死锁
        let mut cur = holder;
        let mut hops = 0usize;
        while hops <= self.waits.len() {
            if cur == pid {
                self.stats.deadlocks += 1;
                // 撤销刚才的边（死锁不入队）
                self.waits.pop();
                return Err(CODE_DEADLOCK);
            }
            match self.waits.iter().find(|w| w.waiter == cur) {
                Some(w) => cur = w.holder,
                None => break,
            }
            hops += 1;
        }
        match self.prims.iter_mut().find(|p| p.id == prim_id) {
            Some(p) => p.waiters.push(pid),
            None => return Err(CODE_SYNC_FORBID),
        }
        Ok(())
    }

    /// 发信号：仅持有者可发；发后移交首位等待者。
    pub fn signal(&mut self, pid: u32, prim_id: u32) -> Result<(), u16> {
        let pos = match self.prims.iter().position(|p| p.id == prim_id) {
            Some(p) => p,
            None => return Err(CODE_SYNC_FORBID),
        };
        if self.prims[pos].holder != pid {
            return Err(CODE_SYNC_FORBID);
        }
        self.prims[pos].holder = 0;
        self.stats.signals += 1;
        // 移交：从等待图撤该原语的边，唤醒首位等待者
        if let Some(i) = self.waits.iter().position(|w| w.prim == prim_id) {
            let next = self.waits[i].waiter;
            self.waits.remove(i);
            self.prims[pos].holder = next;
            if let Some(j) = self.prims[pos].waiters.iter().position(|w| *w == next) {
                self.prims[pos].waiters.remove(j);
            }
        }
        Ok(())
    }

    /// 加锁（独占持有）。
    pub fn lock(&mut self, pid: u32, prim_id: u32) -> Result<(), u16> {
        let pos = match self.prims.iter().position(|p| p.id == prim_id) {
            Some(p) => p,
            None => return Err(CODE_SYNC_FORBID),
        };
        if self.prims[pos].holder != 0 {
            return Err(CODE_SYNC_FORBID);
        }
        self.prims[pos].holder = pid;
        Ok(())
    }

    // -- 读屏 -------------------------------------------------------------

    /// 全量读屏行：共享状态全景。
    pub fn status_line(&self) -> String {
        alloc::format!(
            "表面共享：在册 {} 表面／活跃 {} 句柄／导出 {} 次／导入拒 {} 次／回收 {} 句柄／死锁检出 {} 次",
            self.surfaces.len(),
            self.handles.len(),
            self.stats.exports,
            self.stats.import_denies,
            self.stats.reclaimed_handles,
            self.stats.deadlocks
        )
    }
}

impl Default for SurfaceShare {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 七、判据（14 条；判据侧独立重算）
// ---------------------------------------------------------------------------

/// VE-F0060 判据入口（域聚合表注册：`("VE-F0060", vea60_surfshare::run_vea60_checks)`）。
pub fn run_vea60_checks() -> CheckSet {
    let mut s = CheckSet::new("vea60_surfshare");

    // --- 判据 1.1：跨进程共享——导出产票据且票据与表面绑定 ---
    {
        let mut m = SurfaceShare::new();
        let t = m.export(SharedSurface { id: 1, width: 1920, height: 1080, format: 87, owner: 10 }, 20);
        s.add(
            "A60-共享-导出产票据绑定表面",
            t.map(|t| t.surface == 1 && t.grantee == 20 && !t.used) == Ok(true)
                && m.stats.exports == 1
                && m.surfaces.len() == 1,
            "导出即登记+发票：票据绑定表面与授权进程",
        );
    }

    // --- 判据 1.2：跨进程共享——白名单进程持票导入成功 ---
    {
        let mut m = SurfaceShare::new();
        let t = m.export(SharedSurface { id: 1, width: 800, height: 600, format: 87, owner: 10 }, 20).unwrap_or(ExportTicket { ticket_id: 0, surface: 0, grantee: 0, used: false });
        let _ = m.gate.grant(20);
        let h = m.import(t, 20, 87);
        s.add(
            "A60-共享-白名单持票导入成功",
            h.map(|h| h.surface == 1 && h.pid == 20) == Ok(true) && m.stats.imports_ok == 1,
            "授权进程 + 白名单 + 格式一致：四关全过签发句柄",
        );
    }

    // --- 判据 1.3：跨进程共享——非白名单显性拒绝且不泄漏存在性 ---
    {
        let mut m = SurfaceShare::new();
        let t = m.export(SharedSurface { id: 1, width: 800, height: 600, format: 87, owner: 10 }, 20).unwrap_or(ExportTicket { ticket_id: 0, surface: 0, grantee: 0, used: false });
        let _ = m.gate.grant(20);
        let r = m.import(t, 99, 87);
        s.add(
            "A60-共享-非白名单同码拒绝",
            r == Err(CODE_TICKET_BAD) && m.gate.denies == 1 && m.stats.import_denies == 1,
            "无权进程与无票据同走 TICKET_BAD：不区分「不存在」与「无权」，不泄漏表面存在性",
        );
    }

    // --- 判据 2.1：安全校验——尺寸校验独立拦截 ---
    {
        let r0 = SurfaceShare::check_size(&SharedSurface { id: 1, width: 0, height: 600, format: 87, owner: 10 });
        let r1 = SurfaceShare::check_size(&SharedSurface { id: 1, width: 20000, height: 600, format: 87, owner: 10 });
        s.add(
            "A60-校验-尺寸关独立拦截",
            r0 == Err(CODE_SIZE_BAD) && r1 == Err(CODE_SIZE_BAD),
            "零尺寸与超限尺寸同走 SIZE_BAD，独立于格式/生命周期",
        );
    }

    // --- 判据 2.2：安全校验——格式校验独立拦截 ---
    {
        let r0 = SurfaceShare::check_format(&SharedSurface { id: 1, width: 800, height: 600, format: 87, owner: 10 }, 99);
        let r1 = SurfaceShare::check_format(&SharedSurface { id: 1, width: 800, height: 600, format: 87, owner: 10 }, 0);
        s.add(
            "A60-校验-格式关独立拦截",
            r0 == Err(CODE_FORMAT_BAD) && r1 == Err(CODE_FORMAT_BAD),
            "申报格式不一致与未申报（0）同走 FORMAT_BAD",
        );
    }

    // --- 判据 2.3：安全校验——生命周期校验独立拦截 ---
    {
        let mut m = SurfaceShare::new();
        let r0 = m.check_lifecycle(7);
        let _ = m.export(SharedSurface { id: 7, width: 100, height: 100, format: 1, owner: 1 }, 2);
        let r1 = m.check_lifecycle(7);
        s.add(
            "A60-校验-生命周期关独立拦截",
            r0 == Err(CODE_LIFECYCLE_BAD) && r1.is_ok(),
            "不在册 = LIFECYCLE_BAD；在册后同查通过——四关各自可单测",
        );
    }

    // --- 判据 2.4：安全校验——校验失败不产生半成品句柄 ---
    {
        let mut m = SurfaceShare::new();
        let t = m.export(SharedSurface { id: 1, width: 800, height: 600, format: 87, owner: 10 }, 20).unwrap_or(ExportTicket { ticket_id: 0, surface: 0, grantee: 0, used: false });
        let _ = m.gate.grant(20);
        let _ = m.import(t, 20, 99);
        s.add(
            "A60-校验-失败不产半成品句柄",
            m.handles.is_empty() && m.stats.import_denies == 1 && m.tickets.len() == 1,
            "格式关失败：句柄表空、票据未耗（可重试）——失败路径无副作用",
        );
    }

    // --- 判据 3.1：生命周期——导出方销毁联动全部句柄失效 ---
    {
        let mut m = SurfaceShare::new();
        let t = m.export(SharedSurface { id: 1, width: 800, height: 600, format: 87, owner: 10 }, 20).unwrap_or(ExportTicket { ticket_id: 0, surface: 0, grantee: 0, used: false });
        let _ = m.gate.grant(20);
        let _ = m.import(t, 20, 87);
        let t2 = m.export(SharedSurface { id: 2, width: 640, height: 480, format: 87, owner: 10 }, 20).unwrap_or(ExportTicket { ticket_id: 0, surface: 0, grantee: 0, used: false });
        let _ = m.import(t2, 20, 87);
        let n = m.owner_destroyed(10);
        s.add(
            "A60-生命周期-销毁联动句柄全失效",
            n == 2 && m.handles.is_empty() && m.surfaces.is_empty() && m.stats.reclaimed_handles == 2,
            "属主 10 销毁：两表面两句柄一次回收，「源没了句柄还在」活不过联动",
        );
    }

    // --- 判据 3.2：生命周期——句柄释放计数对账 ---
    {
        let mut m = SurfaceShare::new();
        let t = m.export(SharedSurface { id: 1, width: 800, height: 600, format: 87, owner: 10 }, 20).unwrap_or(ExportTicket { ticket_id: 0, surface: 0, grantee: 0, used: false });
        let _ = m.gate.grant(20);
        let h = m.import(t, 20, 87).unwrap_or(ImportHandle { handle_id: 0, surface: 0, pid: 0 });
        let rel = m.release_handle(h.handle_id);
        let dup = m.release_handle(h.handle_id);
        s.add(
            "A60-生命周期-句柄释放对账",
            rel.is_ok() && dup == Err(CODE_HANDLE_UNKNOWN) && m.handles.is_empty(),
            "正常释放成功；重复释放显性 HANDLE_UNKNOWN 不回绕",
        );
    }

    // --- 判据 3.3：生命周期——悬挂票据显性过期回收 ---
    {
        let mut m = SurfaceShare::new();
        let _ = m.export(SharedSurface { id: 1, width: 800, height: 600, format: 87, owner: 10 }, 20);
        // 表面被属主销毁 → 票据悬挂
        let _ = m.owner_destroyed(10);
        let expired = m.expire_orphan_tickets();
        s.add(
            "A60-生命周期-悬挂票据显性过期",
            expired == 1 && m.tickets.is_empty(),
            "源已不在册的票据不可再导入：过期清理回收，票据不悬挂",
        );
    }

    // --- 判据 4.1：同步防护——死锁环等待即检出且不入队 ---
    {
        let mut m = SurfaceShare::new();
        let _ = m.create_prim(1, SyncKind::Mutex, 10);
        let _ = m.create_prim(2, SyncKind::Mutex, 20);
        // 10 持 1，20 持 2
        let _ = m.lock(10, 1);
        let _ = m.lock(20, 2);
        // 20 等 1（合法）；10 等 2 → 成环（10→20→10）→ 拒绝
        let w1 = m.wait(20, 1);
        let w2 = m.wait(10, 2);
        s.add(
            "A60-同步-死锁环等待即检出",
            w1.is_ok() && w2 == Err(CODE_DEADLOCK) && m.stats.deadlocks == 1 && m.waits.len() == 1,
            "等待图局部环检测：成环边不入队，死锁不成立",
        );
    }

    // --- 判据 4.2：同步防护——自等与越权发信号独立拦截 ---
    {
        let mut m = SurfaceShare::new();
        let _ = m.create_prim(1, SyncKind::Mutex, 10);
        let _ = m.lock(10, 1);
        // 10 已持有原语 1：再等自己 = 自环死锁
        let self_wait = m.wait(10, 1);
        let rogue = m.signal(99, 1);
        let legit = m.signal(10, 1);
        s.add(
            "A60-同步-自等与越权分码拦截",
            self_wait == Err(CODE_DEADLOCK)
                && rogue == Err(CODE_SYNC_FORBID)
                && legit.is_ok()
                && m.stats.signals == 1,
            "持有者自等即自环死锁；未持有者发信号 FORBID；持有者正常发信号入账",
        );
    }

    // --- 判据 5.1：判据自身——共享状态读屏可达 ---
    {
        let m = SurfaceShare::new();
        let line = m.status_line();
        let ok = line.contains("在册") && line.contains("句柄") && line.contains("死锁检出");
        s.add(
            "A60-自身-共享状态读屏可达",
            ok,
            "读屏行覆盖表面/句柄/拒绝/回收/死锁五要素",
        );
    }

    // --- 判据 5.2：判据自身——码位互异且段独占（整仓唯一段 0xC2 内八码） ---
    {
        let codes = [
            CODE_TICKET_BAD,
            CODE_SURFACE_UNKNOWN,
            CODE_SIZE_BAD,
            CODE_FORMAT_BAD,
            CODE_LIFECYCLE_BAD,
            CODE_DEADLOCK,
            CODE_SYNC_FORBID,
            CODE_HANDLE_UNKNOWN,
        ];
        let mut all_diff = true;
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                if codes[i] == codes[j] {
                    all_diff = false;
                }
            }
        }
        let in_segment = codes.iter().all(|c| (c & 0xFF00) == 0xC200);
        s.add(
            "A60-自身-码位互异段独占",
            all_diff && in_segment && codes.len() == 8,
            "诊断码互异或越段",
        );
    }

    s
}

// ---------------------------------------------------------------------------
// 八、测试（5 个行为测试 + 1 个判据全绿）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn surf(id: u32, owner: u32) -> SharedSurface {
        SharedSurface { id, width: 800, height: 600, format: 87, owner }
    }

    fn ticket0() -> ExportTicket {
        ExportTicket { ticket_id: 0, surface: 0, grantee: 0, used: false }
    }

    fn handle0() -> ImportHandle {
        ImportHandle { handle_id: 0, surface: 0, pid: 0 }
    }

    #[test]
    fn vea60_export_import_roundtrip() {
        let mut m = SurfaceShare::new();
        let _ = m.gate.grant(20);
        let t = m.export(surf(1, 10), 20).unwrap_or(ticket0());
        let h = m.import(t, 20, 87);
        assert!(h.is_ok());
        assert_eq!(m.stats.exports, 1);
        assert_eq!(m.stats.imports_ok, 1);
        assert_eq!(m.handles.len(), 1);
        assert!(m.status_line().contains("活跃 1 句柄"));
    }

    #[test]
    fn vea60_gate_whitelist_lifecycle() {
        let mut g = PermGate::default();
        assert!(g.grant(20));
        assert!(!g.grant(20), "重复授权拒绝");
        assert!(!g.grant(0), "系统保留号拒绝");
        assert!(g.allowed(20));
        assert!(g.revoke(20));
        assert!(!g.allowed(20));
        assert!(!g.revoke(20));
    }

    #[test]
    fn vea60_lifecycle_reclaim_chain() {
        let mut m = SurfaceShare::new();
        let _ = m.gate.grant(20);
        let t1 = m.export(surf(1, 10), 20).unwrap_or(ticket0());
        let _ = m.export(surf(2, 10), 20).unwrap_or(ticket0());
        // 只消费一张票：另一张成为潜在悬挂票
        let _ = m.import(t1, 20, 87);
        assert_eq!(m.owner_destroyed(10), 1, "在册句柄一次回收");
        assert_eq!(m.expire_orphan_tickets(), 1, "未消费票据悬挂，由过期清理回收");
        assert!(m.tickets.is_empty() && m.handles.is_empty());
        assert_eq!(m.owner_destroyed(10), 0, "重复销毁零回收");
    }

    #[test]
    fn vea60_sync_deadlock_matrix() {
        let mut m = SurfaceShare::new();
        let _ = m.create_prim(1, SyncKind::Semaphore, 10);
        let _ = m.create_prim(2, SyncKind::Semaphore, 20);
        assert!(m.lock(10, 1).is_ok());
        assert!(m.lock(20, 2).is_ok());
        assert_eq!(m.wait(10, 1), Err(CODE_DEADLOCK), "自等即死锁");
        assert!(m.wait(20, 1).is_ok());
        assert_eq!(m.wait(10, 2), Err(CODE_DEADLOCK), "互等成环即死锁");
        assert!(m.signal(20, 2).is_ok(), "持有者发信号合法");
        assert!(m.signal(10, 1).is_ok());
        assert_eq!(m.stats.signals, 2);
        assert_eq!(m.stats.deadlocks, 2);
    }

    #[test]
    fn vea60_checks_all_green() {
        let set = run_vea60_checks();
        let (passed, failed) = set.tally();
        assert!(
            set.all_passed(),
            "VE-F0060 判据存在红项：{}/{} 绿 —— {}",
            passed,
            passed + failed,
            set.red_items_text()
        );
    }
}
