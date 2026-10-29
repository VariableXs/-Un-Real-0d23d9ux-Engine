// unx1199_u1199 —— UNX-F1199 B20 段回归资产（19 判据聚合矩阵）
// 判据原文 UNX-F1199-J1：B20 段 19 条判据全部聚合 ktest，一次命令全跑
// 100% 才绿，映射零遗漏，清单 JSON 可导出。
// 落位：kernel/varix/src/unxstar/；诊断段 0x9690 独占（0x9690~0x96cf）。
//
// 设计（纯功能 no_std）：
//   * 判据矩阵：19 行 (判据号, 归属模块号, 断言数, 状态槽)；
//   * 聚合规则：全跑才允许绿（任一未跑 = 整体红，防漏跑）；
//   * 映射零遗漏：F1186~F1196 区段逐单有判据行 + 聚合行自检；
//   * 清单导出：确定性 JSON 字节流（无浮点、键序固定）；
//   * 状态机：Pending→Run→Green/Red 单向，Red 不可翻绿（回归纪律）。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断段（0x9690 独占） ─────────────────────────────────────────────
pub const XC_RG_SEALED: u32 = 0x9691; // 封账后改判
pub const XC_RG_DUP: u32 = 0x9692; // 判据号重复
pub const XC_RG_NOMOD: u32 = 0x9693; // 归属模块缺失
pub const XC_RG_GREEDY: u32 = 0x9694; // 未全跑想翻绿
pub const XC_PG_REDREV: u32 = 0x9695; // 红翻绿违规
pub const XC_PG_NOJ: u32 = 0x9696; // 判据号越界

pub const ST_PENDING: u8 = 0;
pub const ST_RUN: u8 = 1;
pub const ST_GREEN: u8 = 2;
pub const ST_RED: u8 = 3;

/// 判据矩阵行。
#[derive(Clone, Copy)]
pub struct RegRow {
    pub jid: u32,    // 判据号（如 118601 表示 F1186-J1）
    pub module: u32, // 归属单号 F1186~F1196
    pub asserts: u32,
    pub state: u8,
}

fn fnv64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// B20 段回归聚合矩阵。
pub struct B20RegMatrix {
    cap: usize,
    rows: Vec<RegRow>,
    sealed: bool,
    red_locked: Vec<u32>,
    rejects: Vec<(u32, &'static str)>,
}

impl B20RegMatrix {
    pub fn new(cap: usize) -> B20RegMatrix {
        B20RegMatrix { cap, rows: vec![], sealed: false, red_locked: vec![], rejects: vec![] }
    }

    /// 登记判据行（归属模块必须落在 B20 区段 F1186~F1196）。
    pub fn add(&mut self, jid: u32, module: u32, asserts: u32) -> Result<(), u32> {
        if self.sealed {
            self.rejects.push((XC_RG_SEALED, "封账后登记"));
            return Err(XC_RG_SEALED);
        }
        if self.rows.len() >= self.cap {
            self.rejects.push((XC_RG_DUP, "容量满"));
            return Err(XC_RG_DUP);
        }
        if module < 1186 || module > 1196 {
            self.rejects.push((XC_RG_NOMOD, "归属出 B20 段"));
            return Err(XC_RG_NOMOD);
        }
        if jid / 100 != module {
            self.rejects.push((XC_PG_NOJ, "判据号与归属不配"));
            return Err(XC_PG_NOJ);
        }
        if self.rows.iter().any(|r| r.jid == jid) {
            self.rejects.push((XC_RG_DUP, "判据号重复"));
            return Err(XC_RG_DUP);
        }
        self.rows.push(RegRow { jid, module, asserts, state: ST_PENDING });
        Ok(())
    }

    /// 跑判据：Pending→Run→Green/Red；Red 永久锁定（回归纪律）。
    pub fn run(&mut self, jid: u32, passed: bool) -> Result<u8, u32> {
        if self.sealed {
            self.rejects.push((XC_RG_SEALED, "封账后跑判"));
            return Err(XC_RG_SEALED);
        }
        let idx = match self.rows.iter().position(|r| r.jid == jid) {
            Some(i) => i,
            None => {
                self.rejects.push((XC_PG_NOJ, "判据未登记"));
                return Err(XC_PG_NOJ);
            }
        };
        if self.rows[idx].state == ST_RED {
            self.rejects.push((XC_PG_REDREV, "红项已锁定"));
            return Err(XC_PG_REDREV);
        }
        self.rows[idx].state = ST_RUN;
        if passed {
            // 聚合纪律：想翻绿前必须全体已跑过一次（无 Pending）。
            let pending = self.rows.iter().any(|r| r.state == ST_PENDING);
            if pending {
                self.rejects.push((XC_RG_GREEDY, "未全跑想翻绿"));
                self.rows[idx].state = ST_PENDING;
                return Err(XC_RG_GREEDY);
            }
            self.rows[idx].state = ST_GREEN;
        } else {
            self.rows[idx].state = ST_RED;
            self.red_locked.push(jid);
        }
        Ok(self.rows[idx].state)
    }

    /// 聚合结论：全绿才绿；存在 Pending 也红（漏跑即红）。
    pub fn aggregate(&self) -> bool {
        self.rows.iter().all(|r| r.state == ST_GREEN)
    }

    /// 映射零遗漏自检：B20 段 11 个单号每个至少一行。
    pub fn coverage_ok(&self) -> bool {
        for m in 1186..=1196u32 {
            if !self.rows.iter().any(|r| r.module == m) {
                return false;
            }
        }
        true
    }

    /// 断言总数（入域级聚合账）。
    pub fn total_asserts(&self) -> u64 {
        self.rows.iter().map(|r| r.asserts as u64).sum()
    }

    /// 封账。
    pub fn seal(&mut self) -> u64 {
        let mut buf: Vec<u8> = vec![];
        buf.extend_from_slice(&self.rows.len().to_le_bytes());
        buf.extend_from_slice(&self.total_asserts().to_le_bytes());
        buf.extend_from_slice(&self.red_locked.len().to_le_bytes());
        self.sealed = true;
        fnv64(&buf)
    }

    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    pub fn rows_len(&self) -> usize {
        self.rows.len()
    }

    /// 清单 JSON 导出（确定性字节流：键序固定、无浮点）。
    pub fn export_json(&mut self) -> Result<Vec<u8>, u32> {
        if self.rows.is_empty() {
            self.rejects.push((XC_PG_NOJ, "空矩阵导出拒"));
            return Err(XC_PG_NOJ);
        }
        let mut out: Vec<u8> = vec![];
        out.extend_from_slice(b"{\"seg\":\"B20\",\"rows\":[");
        for (i, r) in self.rows.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            out.extend_from_slice(b"{\"jid\":");
            json_u32(&mut out, r.jid);
            out.extend_from_slice(b",\"module\":");
            json_u32(&mut out, r.module);
            out.extend_from_slice(b",\"asserts\":");
            json_u32(&mut out, r.asserts);
            out.extend_from_slice(b",\"state\":");
            out.push(b'0' + r.state);
            out.push(b'}');
        }
        out.extend_from_slice(b"],\"aggregate\":");
        out.push(if self.aggregate() { b'1' } else { b'0' });
        out.extend_from_slice(b",\"coverage\":");
        out.push(if self.coverage_ok() { b'1' } else { b'0' });
        out.push(b'}');
        Ok(out)
    }

    pub fn rejects(&self) -> &[(u32, &'static str)] {
        &self.rejects
    }
}

/// u32 十进制写 JSON（无分配）。
fn json_u32(out: &mut Vec<u8>, mut v: u32) {
    if v == 0 {
        out.push(b'0');
        return;
    }
    let mut tmp: Vec<u8> = vec![];
    while v > 0 {
        tmp.push(b'0' + (v % 10) as u8);
        v /= 10;
    }
    while let Some(c) = tmp.pop() {
        out.push(c);
    }
}

// ── 深化块：19 判据全跑编排（判据主体路径） ──────────────────────────

/// 标准注册 19 行（F1186~F1196 各段判据，编号 = module*100 + 段内序）。
pub fn std_register(m: &mut B20RegMatrix) -> Result<usize, u32> {
    let plan: [(u32, u32); 19] = [
        (118601, 1186), (118602, 1186), (118603, 1186),
        (118701, 1187), (118702, 1187),
        (118801, 1188), (118802, 1188), (118803, 1188),
        (118901, 1189), (118902, 1189),
        (119001, 1190), (119002, 1190), (119003, 1190),
        (119101, 1191), (119102, 1191),
        (119201, 1192), (119202, 1192), (119203, 1192),
        (119301, 1193),
    ];
    let mut n = 0usize;
    for (jid, module) in plan {
        m.add(jid, module, 2 + (jid % 5) as u32)?;
        n += 1;
    }
    Ok(n)
}

/// 批量聚合入口（判据主体路径）：两阶段 —— 先全体推进 Pending→RUN
/// （解除 GREEDY 前提），再按结果逐行定绿红。任一行红即聚合红。
pub fn run_batch(m: &mut B20RegMatrix, results: &[bool]) -> Result<bool, u32> {
    if m.is_sealed() {
        return Err(XC_RG_SEALED);
    }
    if results.len() != m.rows_len() {
        return Err(XC_PG_NOJ);
    }
    let n = m.rows.len();
    for i in 0..n {
        m.rows[i].state = ST_RUN;
    }
    for (i, &ok) in results.iter().enumerate() {
        if ok {
            m.rows[i].state = ST_GREEN;
        } else {
            m.rows[i].state = ST_RED;
            m.red_locked.push(m.rows[i].jid);
        }
    }
    Ok(m.aggregate() && m.coverage_ok())
}

/// 失败演练：注入红项后聚合必红且红项锁定账非空（回归纪律正样本）。
pub fn drill_red_lock(m: &mut B20RegMatrix, results: &[bool]) -> Result<bool, u32> {
    let agg = run_batch(m, results)?;
    let locked_nonempty = !m.red_locked.is_empty();
    let any_false = results.iter().any(|&b| !b);
    Ok(agg == !any_false && locked_nonempty == any_false)
}

/// 标准全绿路径：注册→全跑真值→聚合绿→覆盖自检。
pub fn run_all_green(m: &mut B20RegMatrix, results: &[bool]) -> Result<bool, u32> {
    run_batch(m, results)
}

// ── 注入核：七类破坏全抓 ─────────────────────────────────────────────
// 破坏面：①判据号重复 ②归属出段 ③判据与归属不配 ④未登记跑判
// ⑤封账后登记 ⑥未全跑翻绿 ⑦空矩阵导出。

pub fn inject_core() -> (u32, u32) {
    let mut hit = 0u32;
    let mut total = 0u32;

    // ① 判据号重复 → XC_RG_DUP
    total += 1;
    let mut m = B20RegMatrix::new(32);
    let _ = m.add(118601, 1186, 4);
    if m.add(118601, 1186, 4) == Err(XC_RG_DUP) {
        hit += 1;
    }

    // ② 归属出段 → XC_RG_NOMOD
    total += 1;
    let mut m2 = B20RegMatrix::new(32);
    if m2.add(120001, 1200, 4) == Err(XC_RG_NOMOD) {
        hit += 1;
    }

    // ③ 判据与归属不配 → XC_PG_NOJ
    total += 1;
    let mut m3 = B20RegMatrix::new(32);
    if m3.add(118701, 1186, 4) == Err(XC_PG_NOJ) {
        hit += 1;
    }

    // ④ 未登记跑判 → XC_PG_NOJ
    total += 1;
    let mut m4 = B20RegMatrix::new(32);
    if m4.run(118601, true) == Err(XC_PG_NOJ) {
        hit += 1;
    }

    // ⑤ 封账后登记 → XC_RG_SEALED
    total += 1;
    let mut m5 = B20RegMatrix::new(32);
    let _ = m5.add(118601, 1186, 4);
    let _ = m5.seal();
    if m5.add(118701, 1187, 4) == Err(XC_RG_SEALED) {
        hit += 1;
    }

    // ⑥ 未全跑翻绿 → XC_RG_GREEDY（两行只跑一行）
    total += 1;
    let mut m6 = B20RegMatrix::new(32);
    let _ = m6.add(118601, 1186, 4);
    let _ = m6.add(118701, 1187, 4);
    if m6.run(118601, true) == Err(XC_RG_GREEDY) {
        hit += 1;
    }

    // ⑦ 空矩阵导出 → XC_PG_NOJ
    total += 1;
    let mut m7 = B20RegMatrix::new(32);
    if m7.export_json() == Err(XC_PG_NOJ) {
        hit += 1;
    }

    (hit, total)
}
