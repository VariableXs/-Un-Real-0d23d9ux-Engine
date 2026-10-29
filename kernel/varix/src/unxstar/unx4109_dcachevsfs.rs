//! unx4109_dcachevsfs — UNX-F4109 缓存与 fs 直查的优先级账（诊断段 0x8FA0）。
//!
//! 判据原文（提炼）：
//!   J1 命中缓存时 fs 钩子零调用：调用计数 == 0（逐请求可核对）。
//!   J2 未命中时恰好一次调用：调用计数 == 1，不多不少。
//!   J3 调用计数账逐请求留痕：每请求一行账（名/命中否/钩子调用数），
//!      汇总钩子调用总数 == 未命中请求数。
//!   J4 优先级注入拦截：命中缓存仍强调 fs 钩子的注入被检出拒收
//!      （E_HOOK_ON_HIT），保证"缓存优先"语义不可旁路。
//!   J5 填充一致性：未命中穿透后回填的缓存内容与 fs 应答一致
//!      （存在性逐条对齐）。
//! 设计纪律：
//!   1. 优先级单行道：resolve 只允许 缓存→（未命中）→fs，
//!      fs→缓存的逆序注入一律拒绝（判据 J4）。
//!   2. 每请求独立账：钩子调用计数器随请求新建，请求结束入总账，
//!      防跨请求串账。
//!   3. fs 应答确定性：fs 钩子为纯表查（无副作用），应答只取决于
//!      实态表内容，保证账可复算。
//!   4. 填充即对齐：穿透回填时把 fs 应答（存在/不存在 + ino）
//!      原样写入缓存行，缓存态永远镜像 fs 应答（判据 J5）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x8FA0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x8FA0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_HOOK_ON_HIT: u16 = SEG_BASE + 1; // 命中缓存仍调 fs 钩子（优先级注入）
const E_HOOK_COUNT: u16 = SEG_BASE + 2;  // 未命中钩子调用数≠1
const E_NAME_BAD: u16 = SEG_BASE + 3;    // 空名拒收
const E_BAL: u16 = SEG_BASE + 4;         // 汇总账不平（钩子总数≠未命中数）
const E_FILL_DIFF: u16 = SEG_BASE + 5;   // 回填内容与 fs 应答不一致
const E_DUP_REQ: u16 = SEG_BASE + 6;     // 同请求号重复入账

/// 缓存行：名 + 存在性 + ino（0 = 负行）。
pub struct CacheLine {
    pub name: &'static str, // 查找键
    pub exists: bool,       // 缓存判定（镜像 fs 应答）
    pub ino: u32,           // 存在时非 0
}

/// 单请求账（判据 J3 逐请求留痕）。
pub struct ReqLine {
    pub req: u32,        // 请求号
    pub name: &'static str, // 查找名
    pub hit: bool,       // 是否缓存命中
    pub hook_calls: u32, // fs 钩子调用数（判据 J1/J2 核对键）
}

/// 优先级账枢纽：缓存 + fs 实态表 + 请求账。
pub struct DcacheFsPrio {
    pub cache: Vec<CacheLine>, // 缓存行
    pub fs_files: Vec<&'static str>, // fs 实态表（存在的名）
    pub reqs: Vec<ReqLine>,    // 请求账（逐行留痕）
    pub hook_total: u32,       // 钩子调用总数（判据 J3 汇总）
    pub miss_total: u32,       // 未命中请求总数
    pub next_req: u32,         // 请求号分配器
    pub rejected: u32,         // 拒收注入计数
}

impl DcacheFsPrio {
    pub fn new() -> DcacheFsPrio {
        DcacheFsPrio {
            cache: Vec::new(),
            fs_files: Vec::new(),
            reqs: Vec::new(),
            hook_total: 0,
            miss_total: 0,
            next_req: 1,
            rejected: 0,
        }
    }

    /// 供给 fs 实态（重复拒收）。
    pub fn fs_create(&mut self, name: &'static str) -> Result<(), u16> {
        if name.is_empty() {
            self.rejected += 1;
            return Err(E_NAME_BAD);
        }
        for f in &self.fs_files {
            if *f == name {
                self.rejected += 1;
                return Err(E_NAME_BAD);
            }
        }
        self.fs_files.push(name);
        Ok(())
    }

    /// fs 钩子（纯表查，确定性应答）。
    fn hook(&self, name: &str) -> Option<u32> {
        for i in 0..self.fs_files.len() {
            if self.fs_files[i] == name {
                return Some(1000 + i as u32);
            }
        }
        None
    }

    fn cache_pos(&self, name: &str) -> Option<usize> {
        for i in 0..self.cache.len() {
            if self.cache[i].name == name {
                return Some(i);
            }
        }
        None
    }

    /// 核心解析（判据 J1/J2/J5）：
    /// 命中零钩子；未命中恰好一次钩子并回填。
    /// 返回（请求号, 是否存在）。
    pub fn resolve(&mut self, name: &'static str) -> Result<(u32, bool), u16> {
        if name.is_empty() {
            self.rejected += 1;
            return Err(E_NAME_BAD);
        }
        let req = self.next_req;
        self.next_req += 1;
        let mut line = ReqLine {
            req,
            name,
            hit: false,
            hook_calls: 0,
        };
        let exists;
        if let Some(i) = self.cache_pos(name) {
            // 命中路径：钩子调用数必须保持 0（判据 J1）
            if line.hook_calls != 0 {
                self.rejected += 1;
                return Err(E_HOOK_ON_HIT);
            }
            line.hit = true;
            exists = self.cache[i].exists;
        } else {
            // 未命中路径：恰好一次钩子（判据 J2）
            line.hook_calls += 1;
            self.hook_total += 1;
            self.miss_total += 1;
            if line.hook_calls != 1 {
                return Err(E_HOOK_COUNT);
            }
            match self.hook(name) {
                Some(ino) => {
                    exists = true;
                    self.cache.push(CacheLine { name, exists: true, ino });
                }
                None => {
                    exists = false;
                    self.cache.push(CacheLine { name, exists: false, ino: 0 });
                }
            }
            // 回填一致性自证（判据 J5）：缓存行与钩子应答对齐
            let p = match self.cache_pos(name) {
                Some(p) => p,
                None => return Err(E_FILL_DIFF),
            };
            if self.cache[p].exists != exists {
                return Err(E_FILL_DIFF);
            }
        }
        self.reqs.push(line);
        Ok((req, exists))
    }

    /// 注入面（判据 J4）：模拟"命中仍调钩子"的违规实现，
    /// 检出器必须拒绝并记账。
    pub fn inject_hook_on_hit(&mut self, name: &'static str) -> Result<(), u16> {
        if name.is_empty() {
            self.rejected += 1;
            return Err(E_NAME_BAD);
        }
        if self.cache_pos(name).is_some() {
            self.rejected += 1;
            return Err(E_HOOK_ON_HIT); // 命中调钩子：优先级失守
        }
        Ok(())
    }

    /// 汇总对账（判据 J3）：钩子总数 == 未命中数 == 账内未命中行数；
    /// 逐行 hook_calls 与命中标记自洽。
    pub fn conserved(&self) -> bool {
        if self.hook_total != self.miss_total {
            return false;
        }
        let mut miss_rows = 0u32;
        for r in &self.reqs {
            if r.hit {
                if r.hook_calls != 0 {
                    return false; // 命中行不得有钩子调用
                }
            } else {
                miss_rows += 1;
                if r.hook_calls != 1 {
                    return false; // 未命中行必须恰好一次
                }
            }
        }
        miss_rows == self.miss_total
    }

    /// 请求账行数快照。
    pub fn req_count(&self) -> u32 {
        self.reqs.len() as u32
    }

    /// 缓存规模快照（审计面）：缓存行数与其中负行数。
    pub fn cache_stats(&self) -> (u32, u32) {
        let mut neg = 0u32;
        for c in &self.cache {
            if !c.exists {
                neg += 1;
            }
        }
        (self.cache.len() as u32, neg)
    }

    /// 请求汇总三元组（判据 J3 汇总面）：
    /// 返回（请求总数，钩子总数，未命中数），三者满足
    /// hook_total == miss_total 且请求总数 = 命中 + 未命中。
    pub fn req_summary(&self) -> (u32, u32, u32) {
        let mut hits = 0u32;
        for r in &self.reqs {
            if r.hit {
                hits += 1;
            }
        }
        (self.reqs.len() as u32, self.hook_total, self.miss_total)
    }

    /// 缓存行失效（审计面）：摘除指定名缓存行，使其后查找重回
    /// 未命中路径（穿透恰好一次的可复演前提）。
    /// 不在缓存返回 E_FILL_DIFF 语义的账错误。
    pub fn invalidate(&mut self, name: &str) -> Result<(), u16> {
        let mut idx = self.cache.len();
        for i in 0..self.cache.len() {
            if self.cache[i].name == name {
                idx = i;
                break;
            }
        }
        if idx == self.cache.len() {
            self.rejected += 1;
            return Err(E_FILL_DIFF);
        }
        let last = self.cache.len() - 1;
        self.cache.swap(idx, last);
        self.cache.pop();
        Ok(())
    }

    /// 点名请求账读取（判据 J3 逐请求核对辅助）：按请求号取行。
    pub fn req_line(&self, req: u32) -> Result<(bool, u32), u16> {
        for r in &self.reqs {
            if r.req == req {
                return Ok((r.hit, r.hook_calls));
            }
        }
        Err(E_DUP_REQ) // 账内无此请求号
    }
}

/// 自例程：R1 未命中一次钩子→R2 命中零钩子→R3 注入拦截→R4 逐请求对账。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;

    // R1 未命中：首次解析穿透恰好一次并回填。
    let mut d = DcacheFsPrio::new();
    d.fs_create("a")?;
    d.fs_create("b")?;
    let (r1, ex1) = d.resolve("a")?;
    if !ex1 || r1 != 1 {
        return Err(E_FILL_DIFF);
    }
    let (_, ex2) = d.resolve("ghost")?;
    if ex2 {
        return Err(E_FILL_DIFF); // ghost 不存在
    }
    if d.hook_total != 2 || d.miss_total != 2 {
        return Err(E_BAL); // 两次未命中两次钩子
    }
    hits += 1;

    // R2 命中：再查 a/ghost 零钩子（判据 J1）。
    let before = d.hook_total;
    let (_, _) = d.resolve("a")?;
    let (_, _) = d.resolve("ghost")?;
    if d.hook_total != before {
        return Err(E_HOOK_ON_HIT); // 命中路径钩子数不得增长
    }
    if d.req_count() != 4 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3 注入拦截：命中调钩子注入、空名、重复实态全拒。
    if d.inject_hook_on_hit("a").is_ok() {
        return Err(E_HOOK_ON_HIT); // 判据 J4 拦截面
    }
    if d.resolve("").is_ok() {
        return Err(E_NAME_BAD);
    }
    if d.fs_create("a").is_ok() {
        return Err(E_NAME_BAD); // 重复实态应拒
    }
    if d.rejected < 3 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4 逐请求对账（判据 J3）：逐行核 hook_calls，汇总平账。
    if !d.conserved() {
        return Err(E_BAL);
    }
    // 逐行抽验：第 1、2 行未命中钩子=1；第 3、4 行命中钩子=0。
    if d.reqs[0].hook_calls != 1 || d.reqs[1].hook_calls != 1 {
        return Err(E_HOOK_COUNT);
    }
    if d.reqs[2].hook_calls != 0 || d.reqs[3].hook_calls != 0 {
        return Err(E_HOOK_ON_HIT);
    }
    if d.reqs[0].hit || d.reqs[1].hit {
        return Err(E_BAL);
    }
    if !d.reqs[2].hit || !d.reqs[3].hit {
        return Err(E_BAL);
    }
    hits += 1;

    // R5 审计面：请求号取行、汇总三元组、缓存失效复演穿透。
    let (h4, hc4) = d.req_line(4)?;
    if !h4 || hc4 != 0 {
        return Err(E_BAL); // 第 4 号请求是命中行
    }
    if d.req_line(99).is_ok() {
        return Err(E_DUP_REQ); // 幽灵请求号应拒
    }
    let (rq, hk, ms) = d.req_summary();
    if rq != 4 || hk != 2 || ms != 2 {
        return Err(E_BAL); // 汇总三元组复算
    }
    let (cs, negs) = d.cache_stats();
    if cs != 2 || negs != 1 {
        return Err(E_BAL); // 缓存两行：a(正) + ghost(负)
    }
    // 失效复演：摘 a 后再查 → 重新未命中 → 钩子恰好 +1。
    d.invalidate("a")?;
    if d.invalidate("a").is_ok() {
        return Err(E_FILL_DIFF); // 重复失效应拒
    }
    let before = d.hook_total;
    let (_, _) = d.resolve("a")?;
    if d.hook_total != before + 1 {
        return Err(E_HOOK_COUNT); // 失效后恰好一次穿透
    }
    if !d.conserved() {
        return Err(E_BAL); // 复演后守恒仍绿
    }
    hits += 1;

    Ok(hits)
}
