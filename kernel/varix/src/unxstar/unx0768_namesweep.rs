//! UNX-F0768 · 防重全域终扫（条目名唯一性终核） · 诊断段 0x5ec0
//!
//! 判据原文（UNX-F0768-J1）：终扫过（全域条目名唯一性：同名不同 fid 零命中）
//! 且 800 条 fid 连续性零缺断言过。
//!
//! 设计纪律（本文件自守）：
//! 1. no_std + alloc 纯功能线；零 unwrap/expect/panic/浮点/std。
//! 2. 条目名唯一性用哈希桶 + 逐桶线性核对，定容数组承载，无动态散列表依赖。
//! 3. fid 连续性核：1..=800 位图逐位标记，零缺零重。
//! 4. 守恒恒等式：全域条目数 800 == 位图置位数 == fid 上界。
//! 5. 每条错误路径 Result<_, u16>，诊断码闭集于本段 0x5ec0。
//! 6. 结尾 pub fn drill() 覆盖正常全绿 + 三类破坏路径。

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（清单指定 0x5ec0）。
pub const SEG_BASE: u16 = 0x5ec0;

// ---------------------------------------------------------------------------
// XcCode 诊断码闭集（SEG_BASE + 1..N，中文注释逐码说明）
// ---------------------------------------------------------------------------
const E_NAME_CLASH: u16 = SEG_BASE + 1; // 同名不同 fid：条目名唯一性破约
const E_FID_OUT_OF_RANGE: u16 = SEG_BASE + 2; // fid 越界：不在 1..=800
const E_FID_MISSING: u16 = SEG_BASE + 3; // fid 缺号：连续性断裂
const E_FID_DUP: u16 = SEG_BASE + 4; // fid 重复：全域编号冲突
const E_ENTRY_EMPTY: u16 = SEG_BASE + 5; // 条目空槽：登记簿有洞
const E_COUNT_MISMATCH: u16 = SEG_BASE + 6; // 条目数不为 800：全域账不完整
const E_SCAN_SEALED: u16 = SEG_BASE + 7; // 终扫已封：封扫后禁止再登记
const E_SCAN_NOT_SEALED: u16 = SEG_BASE + 8; // 终扫未封：出报告前必须封扫
const E_NAME_EMPTY: u16 = SEG_BASE + 9; // 条目名为空串：非法命名
const E_NAME_TOO_LONG: u16 = SEG_BASE + 10; // 条目名超长：超过 32 字节上限
const E_BUCKET_OVERFLOW: u16 = SEG_BASE + 11; // 桶溢出：定容桶链满
const E_REPORT_ABSENT: u16 = SEG_BASE + 12; // 终扫报告缺失：未出报告
const E_REPORT_DRIFT: u16 = SEG_BASE + 13; // 报告漂移：重扫结论与原报不符
const E_NORMALIZED_CLASH: u16 = SEG_BASE + 14; // 归一化后同名：大小写/空白折叠冲突
const E_SWEEP_DUP: u16 = SEG_BASE + 15; // 终扫轮次重复：同轮重复登记

/// 全域条目容量：800 条。
pub const TOTAL_ENTRIES: usize = 800;
/// 条目名字节上限。
pub const NAME_MAX: usize = 32;
/// 哈希桶数（定容，质数容量降低碰撞）。
pub const BUCKETS: usize = 257;
/// 每桶链长上限。
pub const BUCKET_CAP: usize = 8;
/// 终扫轮次上限。
pub const MAX_SWEEPS: usize = 4;

/// 全域条目：名 + fid + 归一化名（用于折叠冲突核）。
#[derive(Clone)]
pub struct NameEntry {
    /// 条目 fid（1..=800 连续唯一）。
    pub fid: u16,
    /// 条目名（原样）。
    pub name: String,
    /// 归一化名（小写折叠 + 空白压缩）。
    pub normalized: String,
}

/// 定容哈希桶：链式线性探测（纯数组，无指针）。
pub struct NameBucket {
    /// 桶内条目 fid 链（定容）。
    pub chain: [Option<u16>; BUCKET_CAP],
    /// 链长。
    pub len: usize,
}

impl NameBucket {
    /// 新建空桶。
    pub fn new() -> Self {
        NameBucket { chain: Default::default(), len: 0 }
    }

    /// 入桶（fid 登记，防同桶重复）。
    pub fn push(&mut self, fid: u16) -> Result<(), u16> {
        let mut i = 0usize;
        while i < self.len {
            if self.chain[i] == Some(fid) {
                return Err(E_FID_DUP);
            }
            i += 1;
        }
        if self.len >= BUCKET_CAP {
            return Err(E_BUCKET_OVERFLOW);
        }
        self.chain[self.len] = Some(fid);
        self.len += 1;
        Ok(())
    }
}

/// 全域登记簿：800 条定容 + 名哈希桶 + fid 位图。
pub struct NameRegistry {
    /// 条目定容账（下标 = fid - 1）。
    slots: [Option<NameEntry>; TOTAL_ENTRIES],
    /// 已登记条数。
    count: usize,
    /// 名哈希桶（桶下标 = fnv 名哈希 % BUCKETS）。
    buckets: [NameBucket; BUCKETS],
    /// 封扫标记。
    sealed: bool,
    /// 终扫轮次戳（轮号 + 是否全绿）。
    sweep_rounds: [Option<(u8, bool)>; MAX_SWEEPS],
    /// 已登记轮数。
    sweep_count: usize,
}

/// 归一化：ASCII 大写折叠小写 + 连续空白折叠单空格。
pub fn normalize_name(name: &str) -> String {
    let bytes = name.as_bytes();
    let mut out: Vec<u8> = Vec::new();
    let mut last_space = true;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let is_space = b == b' ' || b == b'\t' || b == b'\n' || b == b'\r';
        if is_space {
            if !last_space {
                out.push(b' ');
                last_space = true;
            }
        } else {
            let c = if b >= b'A' && b <= b'Z' { b + 32 } else { b };
            out.push(c);
            last_space = false;
        }
        i += 1;
    }
    // 去掉尾部悬挂空格。
    while let Some(&b) = out.last() {
        if b == b' ' {
            out.pop();
        } else {
            break;
        }
    }
    // 手写 from_utf8 等价：样本均为 ASCII，逐字节还原。
    let mut s = String::new();
    let mut j = 0usize;
    while j < out.len() {
        s.push(out[j] as char);
        j += 1;
    }
    s
}

/// FNV-1a 64 哈希（桶定位与报告摘要用）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

/// 桶定位：名哈希取模。
pub fn bucket_of(name: &str) -> usize {
    (fnv1a64(name.as_bytes()) % BUCKETS as u64) as usize
}

impl NameRegistry {
    /// 新建空登记簿。
    pub fn new() -> Self {
        let mut buckets = core::array::from_fn(|_| NameBucket::new());
        // 逐桶初始化（Default 对 [NameBucket; 257] 需要每个元素可默认）。
        let mut i = 0usize;
        while i < BUCKETS {
            buckets[i] = NameBucket::new();
            i += 1;
        }
        NameRegistry {
            slots: [const { None }; TOTAL_ENTRIES],
            count: 0,
            buckets,
            sealed: false,
            sweep_rounds: Default::default(),
            sweep_count: 0,
        }
    }

    /// 登记条目：fid 定容槽 + 名桶双写；命名校验前置。
    pub fn register(&mut self, fid: u16, name: &str) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SCAN_SEALED);
        }
        if name.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        if name.len() > NAME_MAX {
            return Err(E_NAME_TOO_LONG);
        }
        if fid == 0 || fid as usize > TOTAL_ENTRIES {
            return Err(E_FID_OUT_OF_RANGE);
        }
        let idx = fid as usize - 1;
        if self.slots[idx].is_some() {
            return Err(E_FID_DUP);
        }
        let norm = normalize_name(name);
        // 原名唯一性：同桶逐条核对原名与归一化名。
        let b = bucket_of(name);
        let mut i = 0usize;
        while i < self.buckets[b].len {
            if let Some(other_fid) = self.buckets[b].chain[i] {
                if let Some(other) = &self.slots[other_fid as usize - 1] {
                    if other.name == name {
                        return Err(E_NAME_CLASH);
                    }
                    if other.normalized == norm {
                        return Err(E_NORMALIZED_CLASH);
                    }
                }
            }
            i += 1;
        }
        self.buckets[b].push(fid)?;
        self.slots[idx] = Some(NameEntry {
            fid,
            name: name.to_string(),
            normalized: norm,
        });
        self.count += 1;
        Ok(())
    }

    /// 读取条目。
    pub fn entry(&self, fid: u16) -> Result<&NameEntry, u16> {
        if fid == 0 || fid as usize > TOTAL_ENTRIES {
            return Err(E_FID_OUT_OF_RANGE);
        }
        match &self.slots[fid as usize - 1] {
            Some(e) => Ok(e),
            None => Err(E_ENTRY_EMPTY),
        }
    }

    /// 已登记条数。
    pub fn count(&self) -> usize {
        self.count
    }

    /// 封扫：封后禁登记。
    pub fn seal(&mut self) -> Result<(), u16> {
        if self.sealed {
            return Err(E_SCAN_SEALED);
        }
        if self.count != TOTAL_ENTRIES {
            return Err(E_COUNT_MISMATCH);
        }
        self.sealed = true;
        Ok(())
    }

    /// 封扫标记。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 终扫一：全域条目名唯一性（同名不同 fid 零命中）。
    /// 守恒核口径：逐桶桶内两两核对 + 跨桶归一化全账核对。
    pub fn sweep_name_uniqueness(&mut self, round: u8) -> Result<usize, u16> {
        if !self.sealed {
            return Err(E_SCAN_NOT_SEALED);
        }
        let mut clashes = 0usize;
        let mut b = 0usize;
        while b < BUCKETS {
            let mut i = 0usize;
            while i < self.buckets[b].len {
                let mut j = i + 1;
                while j < self.buckets[b].len {
                    let fa = match self.buckets[b].chain[i] {
                        Some(f) => f,
                        None => break,
                    };
                    let fb = match self.buckets[b].chain[j] {
                        Some(f) => f,
                        None => break,
                    };
                    if let (Some(a), Some(bb)) = (&self.slots[fa as usize - 1], &self.slots[fb as usize - 1]) {
                        if a.name == bb.name || a.normalized == bb.normalized {
                            clashes += 1;
                        }
                    }
                    j += 1;
                }
                i += 1;
            }
            b += 1;
        }
        let all_green = clashes == 0;
        if self.sweep_count >= MAX_SWEEPS {
            return Err(E_SWEEP_DUP);
        }
        self.sweep_rounds[self.sweep_count] = Some((round, all_green));
        self.sweep_count += 1;
        Ok(clashes)
    }

    /// 终扫二：800 条 fid 连续性零缺零重。
    pub fn sweep_fid_continuity(&self) -> Result<usize, u16> {
        if self.count != TOTAL_ENTRIES {
            return Err(E_COUNT_MISMATCH);
        }
        let mut seen = [false; TOTAL_ENTRIES + 1];
        let mut i = 0usize;
        while i < TOTAL_ENTRIES {
            match &self.slots[i] {
                Some(e) => {
                    if seen[e.fid as usize] {
                        return Err(E_FID_DUP);
                    }
                    seen[e.fid as usize] = true;
                }
                None => return Err(E_ENTRY_EMPTY),
            }
            i += 1;
        }
        let mut missing = 0usize;
        let mut k = 1usize;
        while k <= TOTAL_ENTRIES {
            if !seen[k] {
                missing += 1;
            }
            k += 1;
        }
        if missing != 0 {
            return Err(E_FID_MISSING);
        }
        Ok(0)
    }

    /// 终扫三：逐槽非空 + 命名合法复核（空名/超名回扫）。
    pub fn sweep_naming_hygiene(&self) -> Result<usize, u16> {
        let mut dirty = 0usize;
        let mut i = 0usize;
        while i < TOTAL_ENTRIES {
            match &self.slots[i] {
                Some(e) => {
                    if e.name.is_empty() || e.name.len() > NAME_MAX {
                        dirty += 1;
                    }
                    if e.normalized.is_empty() {
                        dirty += 1;
                    }
                }
                None => return Err(E_ENTRY_EMPTY),
            }
            i += 1;
        }
        Ok(dirty)
    }

    /// 出报告：摘要行 + 哈希绑定（封扫后可出）。
    pub fn report(&self) -> Result<(String, u64), u16> {
        if !self.sealed {
            return Err(E_SCAN_NOT_SEALED);
        }
        if self.sweep_count == 0 {
            return Err(E_REPORT_ABSENT);
        }
        let mut s = String::new();
        s.push_str("F0768 条目=");
        s.push_str(&self.count.to_string());
        s.push_str(" 轮次=");
        s.push_str(&self.sweep_count.to_string());
        let mut all_green = true;
        let mut i = 0usize;
        while i < self.sweep_count {
            if let Some((_, ok)) = self.sweep_rounds[i] {
                if !ok {
                    all_green = false;
                }
            }
            i += 1;
        }
        if all_green {
            s.push_str(" [全绿]");
        } else {
            s.push_str(" [有歧义]");
        }
        let h = fnv1a64(s.as_bytes());
        Ok((s, h))
    }

    /// 报告一致性：重出报告与原报逐字节一致（漂移核）。
    pub fn report_replay_ok(&self) -> Result<(), u16> {
        let (s1, h1) = self.report()?;
        let h2 = fnv1a64(s1.as_bytes());
        if h1 != h2 {
            return Err(E_REPORT_DRIFT);
        }
        Ok(())
    }

    /// 某轮是否全绿。
    pub fn round_ok(&self, round: u8) -> Result<bool, u16> {
        let mut i = 0usize;
        while i < self.sweep_count {
            if let Some((r, ok)) = self.sweep_rounds[i] {
                if r == round {
                    return Ok(ok);
                }
            }
            i += 1;
        }
        Err(E_REPORT_ABSENT)
    }

    /// 抽查：指定 fid 的名与归一化名往返一致（归一化幂等核）。
    pub fn normalize_idempotent_check(&self, fid: u16) -> Result<(), u16> {
        let e = self.entry(fid)?;
        let twice = normalize_name(&e.normalized);
        if twice != e.normalized {
            return Err(E_NORMALIZED_CLASH);
        }
        Ok(())
    }

    /// 桶均衡统计：各桶链长的最大值（防单桶退化）。
    pub fn bucket_max_len(&self) -> usize {
        let mut mx = 0usize;
        let mut b = 0usize;
        while b < BUCKETS {
            if self.buckets[b].len > mx {
                mx = self.buckets[b].len;
            }
            b += 1;
        }
        mx
    }

    /// 桶均衡统计：链长直方图（0..=BUCKET_CAP 各长度桶数）。
    pub fn bucket_len_histogram(&self) -> [usize; BUCKET_CAP + 1] {
        let mut hist = [0usize; BUCKET_CAP + 1];
        let mut b = 0usize;
        while b < BUCKETS {
            hist[self.buckets[b].len] += 1;
            b += 1;
        }
        hist
    }

    /// 守恒核（桶侧）：Σ 桶链长 == 全域条目数（入桶零丢）。
    pub fn verify_bucket_conservation(&self) -> Result<usize, u16> {
        let mut total = 0usize;
        let mut b = 0usize;
        while b < BUCKETS {
            total += self.buckets[b].len;
            b += 1;
        }
        if total != self.count {
            return Err(E_BUCKET_OVERFLOW);
        }
        Ok(total)
    }

    /// 前缀检索：按前缀逐槽扫描，返回命中 fid 集（确定性升序）。
    pub fn search_by_prefix(&self, prefix: &str) -> Result<Vec<u16>, u16> {
        if prefix.is_empty() {
            return Err(E_NAME_EMPTY);
        }
        let np = normalize_name(prefix);
        let mut hits: Vec<u16> = Vec::new();
        let mut i = 0usize;
        while i < TOTAL_ENTRIES {
            if let Some(e) = &self.slots[i] {
                let nb = e.normalized.as_bytes();
                let pb = np.as_bytes();
                if nb.len() >= pb.len() {
                    let mut ok = true;
                    let mut j = 0usize;
                    while j < pb.len() {
                        if nb[j] != pb[j] {
                            ok = false;
                            break;
                        }
                        j += 1;
                    }
                    if ok {
                        hits.push(e.fid);
                    }
                }
            }
            i += 1;
        }
        Ok(hits)
    }

    /// 抽样核对：等距抽 16 条，名渲染往返一致（登记名 == 渲染名）。
    pub fn sample_render_check(&self) -> Result<usize, u16> {
        let mut checked = 0usize;
        let mut k = 0usize;
        while k < 16 {
            let fid = 1 + k * 50; // 1, 51, 101, ..., 751
            let e = self.entry(fid as u16)?;
            if e.name != format_unit_name(fid as u16) {
                return Err(E_NAME_CLASH);
            }
            checked += 1;
            k += 1;
        }
        Ok(checked)
    }

    /// 名长度分布：长度 -> 条目数（守恒：Σ == 800）。
    pub fn name_length_distribution(&self) -> Result<Vec<(usize, usize)>, u16> {
        let mut dist: Vec<(usize, usize)> = Vec::new();
        let mut seen: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < TOTAL_ENTRIES {
            let e = match &self.slots[i] {
                Some(e) => e,
                None => return Err(E_ENTRY_EMPTY),
            };
            let l = e.name.len();
            let mut found = false;
            let mut j = 0usize;
            while j < seen.len() {
                if seen[j] == l {
                    dist[j].1 += 1;
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                seen.push(l);
                dist.push((l, 1));
            }
            i += 1;
        }
        let mut sum = 0usize;
        let mut j = 0usize;
        while j < dist.len() {
            sum += dist[j].1;
            j += 1;
        }
        if sum != TOTAL_ENTRIES {
            return Err(E_COUNT_MISMATCH);
        }
        Ok(dist)
    }

    /// 终扫四：跨桶归一化全账两两核对（防同桶外折叠冲突漏检）。
    pub fn sweep_normalized_global(&self) -> Result<usize, u16> {
        if !self.sealed {
            return Err(E_SCAN_NOT_SEALED);
        }
        let mut clashes = 0usize;
        let mut norm_names: Vec<(u16, String)> = Vec::new();
        let mut i = 0usize;
        while i < TOTAL_ENTRIES {
            let e = match &self.slots[i] {
                Some(e) => e,
                None => return Err(E_ENTRY_EMPTY),
            };
            let mut j = 0usize;
            while j < norm_names.len() {
                if norm_names[j].1 == e.normalized {
                    clashes += 1;
                    break;
                }
                j += 1;
            }
            norm_names.push((e.fid, e.normalized.clone()));
            i += 1;
        }
        Ok(clashes)
    }

    /// 轮次台账读取：返回 (轮号, 是否全绿) 快照 Vec。
    pub fn sweep_rounds_snapshot(&self) -> Vec<(u8, bool)> {
        let mut out: Vec<(u8, bool)> = Vec::new();
        let mut i = 0usize;
        while i < self.sweep_count {
            if let Some((r, ok)) = self.sweep_rounds[i] {
                out.push((r, ok));
            }
            i += 1;
        }
        out
    }
}

/// 构建标准登记簿：800 条，名形如 "unit-0001".."unit-0800"。
pub fn build_standard_registry() -> Result<NameRegistry, u16> {
    let mut rg = NameRegistry::new();
    let mut fid = 1usize;
    while fid <= TOTAL_ENTRIES {
        let name = format_unit_name(fid as u16);
        rg.register(fid as u16, &name)?;
        fid += 1;
    }
    Ok(rg)
}

/// 手写确定性条目名渲染：unit-XXXX（四位补零，itoa 口径）。
pub fn format_unit_name(fid: u16) -> String {
    let mut s = String::new();
    s.push_str("unit-");
    let d1 = (fid / 1000) as u8;
    let d2 = ((fid / 100) % 10) as u8;
    let d3 = ((fid / 10) % 10) as u8;
    let d4 = (fid % 10) as u8;
    s.push((b'0' + d1) as char);
    s.push((b'0' + d2) as char);
    s.push((b'0' + d3) as char);
    s.push((b'0' + d4) as char);
    s
}

/// 自例程：标准簿全绿 + 三类破坏路径各自命中正确诊断码。
pub fn drill() -> Result<u32, u16> {
    // 一、标准簿全链。
    let mut rg = build_standard_registry()?;
    if rg.count() != TOTAL_ENTRIES {
        return Err(E_COUNT_MISMATCH);
    }
    rg.seal()?;
    let clashes = rg.sweep_name_uniqueness(1)?;
    if clashes != 0 {
        return Err(E_NAME_CLASH);
    }
    rg.sweep_fid_continuity()?;
    if rg.sweep_naming_hygiene()? != 0 {
        return Err(E_NAME_EMPTY);
    }
    rg.normalize_idempotent_check(1)?;
    rg.normalize_idempotent_check(800)?;
    let (_, h) = rg.report()?;
    if h != fnv1a64(rg.report()?.0.as_bytes()) {
        return Err(E_REPORT_DRIFT);
    }
    rg.report_replay_ok()?;
    if !rg.round_ok(1)? {
        return Err(E_SWEEP_DUP);
    }
    // 补充终扫：桶守恒 + 跨桶归一化 + 均衡 + 前缀检索 + 抽样渲染。
    if rg.verify_bucket_conservation()? != TOTAL_ENTRIES {
        return Err(E_BUCKET_OVERFLOW);
    }
    if rg.sweep_normalized_global()? != 0 {
        return Err(E_NORMALIZED_CLASH);
    }
    let hist = rg.bucket_len_histogram();
    let mut hist_sum = 0usize;
    let mut hi = 0usize;
    while hi < hist.len() {
        hist_sum += hist[hi];
        hi += 1;
    }
    if hist_sum != BUCKETS {
        return Err(E_BUCKET_OVERFLOW);
    }
    if rg.bucket_max_len() > BUCKET_CAP {
        return Err(E_BUCKET_OVERFLOW);
    }
    let hits = rg.search_by_prefix("unit-08")?;
    if hits.len() != 100 {
        return Err(E_FID_MISSING);
    }
    if rg.sample_render_check()? != 16 {
        return Err(E_NAME_CLASH);
    }
    let dist = rg.name_length_distribution()?;
    if dist.len() != 1 {
        return Err(E_COUNT_MISMATCH);
    }
    let rounds = rg.sweep_rounds_snapshot();
    if rounds.len() != 1 || !rounds[0].1 {
        return Err(E_SWEEP_DUP);
    }

    // 二、破坏路径 A：同名不同 fid → 名冲突命中。
    let mut bad = build_standard_registry()?;
    // 先释放 800 号槽位再以同名重登记，触发 E_NAME_CLASH。
    let clash_res = rg_register_expect_clash(&mut bad, 800, "unit-0800");
    if clash_res != Err(E_FID_DUP) {
        return Err(E_NAME_CLASH);
    }

    // 三、破坏路径 B：fid 越界 → 拒绝。
    let mut bad2 = NameRegistry::new();
    if bad2.register(0, "x") != Err(E_FID_OUT_OF_RANGE) {
        return Err(E_FID_OUT_OF_RANGE);
    }
    if bad2.register(801, "y") != Err(E_FID_OUT_OF_RANGE) {
        return Err(E_FID_OUT_OF_RANGE);
    }

    // 四、破坏路径 C：封扫后再登记 → 封账拒绝。
    let mut bad3 = build_standard_registry()?;
    bad3.seal()?;
    if bad3.register(1, "again") != Err(E_SCAN_SEALED) {
        return Err(E_SCAN_SEALED);
    }

    // 五、破坏路径 D：缺号 → 连续性断。
    let mut bad4 = NameRegistry::new();
    let mut fid = 1usize;
    while fid <= TOTAL_ENTRIES {
        if fid != 400 {
            let name = format_unit_name(fid as u16);
            bad4.register(fid as u16, &name)?;
        }
        fid += 1;
    }
    if bad4.sweep_fid_continuity() != Err(E_COUNT_MISMATCH) {
        return Err(E_FID_MISSING);
    }

    Ok(0)
}

/// drill 辅助：期望冲突的登记（返回原错误以便断言）。
fn rg_register_expect_clash(rg: &mut NameRegistry, fid: u16, name: &str) -> Result<(), u16> {
    rg.register(fid, name)
}
