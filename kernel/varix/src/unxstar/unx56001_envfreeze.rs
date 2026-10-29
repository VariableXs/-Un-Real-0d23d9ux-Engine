//! UNX-F56001 对照基准机环境冻结规范与探针（0x60000 段）
//!
//! 锚点：UNX-F56001-J1 探针两次连跑 env_hash 一致（SHA-256 环境哈希）；
//! J2 冻结语义（冻结后拒改、重冻结须新波次且严格递增）；J3 探针比对核
//! （漂移落账带诊断码 + 「环境漂移-禁止入账」拒收痕迹）三判据全绿。
//! 域：CoRun·UNX-F2-B40（对照基准机域·环境冻结面）。
//!
//! 结构：12 类环境维度登记册（固件版本/CPU 频率策略/内存刷新率/电源计划/
//! 后台进程清单/文件系统布局/盘符拓扑/驱动清单/时钟源/系统版本锚/采集时段窗/
//! 外设静默态，逐维落账含 SHA-256 哈希指纹）+ 环境冻结核（冻结拒写、
//! 重冻结升波次、指纹完整性复核）+ 探针核（冻结剖面 vs 当前剖面逐维比对，
//! 漂移落账）+ 探针日志（申报与实况一致性闸，漂移隐匿必抓）+ SHA-256 纯实现
//! （空串与 "abc" 已知向量自证）+ 注入核（未知维度/空值/重复/乱序/未齐冻结/
//! 冻结后写/波次回退/指纹篡改/漂移拒收/漂移隐匿/未冻结探针 全必抓）+
//! 渲染面（登记册文本导出，供上级判据消费）。

use crate::alloc::format;
use crate::alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x60000 独占 0x10）。
pub const SEG_BASE: u32 = 0x60000;
pub const ENV_DIM_UNKNOWN: u32 = SEG_BASE + 0x01; // 未知环境维度（非 12 类闭集）
pub const ENV_DIM_DUP: u32 = SEG_BASE + 0x02; // 维度重复登记（同维二次落账）
pub const ENV_VAL_EMPTY: u32 = SEG_BASE + 0x03; // 维度值为空串
pub const ENV_FROZEN_WR: u32 = SEG_BASE + 0x04; // 冻结后写拒
pub const ENV_WAVE_BACK: u32 = SEG_BASE + 0x05; // 波次回退（重冻结波次 ≤ 既有波次）
pub const ENV_TAMPER: u32 = SEG_BASE + 0x06; // 冻结指纹篡改（账面哈希与重算不一致）
pub const ENV_DRIFT: u32 = SEG_BASE + 0x07; // 环境漂移（探针比对失配）
pub const ENV_PROBE_HIDE: u32 = SEG_BASE + 0x08; // 漂移隐匿（申报结论与探针实况不符）
pub const ENV_ORDER_BAD: u32 = SEG_BASE + 0x09; // 登记序非规范（维度未按序落账）
pub const ENV_PROBE_STATE: u32 = SEG_BASE + 0x0A; // 探针状态机非法（未冻结即探针等）
pub const ENV_INCOMPLETE: u32 = SEG_BASE + 0x0B; // 冻结前置缺失（12 维未齐即冻结）

/// 12 类环境维度闭集（判据与登记共用同源序，验收清单原文序）。
pub const DIM_FW: usize = 0; // 固件版本
pub const DIM_CPUFREQ: usize = 1; // CPU 频率策略
pub const DIM_MEMREFRESH: usize = 2; // 内存刷新率
pub const DIM_POWERPLAN: usize = 3; // 电源计划
pub const DIM_BGPROC: usize = 4; // 后台进程清单
pub const DIM_FSLAYOUT: usize = 5; // 文件系统布局
pub const DIM_DRVTOPO: usize = 6; // 盘符拓扑
pub const DIM_DRIVER: usize = 7; // 驱动清单
pub const DIM_CLOCKSRC: usize = 8; // 时钟源
pub const DIM_SYSANCHOR: usize = 9; // 系统版本锚
pub const DIM_WIN: usize = 10; // 采集时段窗
pub const DIM_PERIPH: usize = 11; // 外设静默态
pub const DIM_COUNT: usize = 12;

/// 维度名（账目渲染与探针比对共用）。
pub const DIM_NAMES: [&str; DIM_COUNT] = [
    "固件版本",
    "CPU频率策略",
    "内存刷新率",
    "电源计划",
    "后台进程清单",
    "文件系统布局",
    "盘符拓扑",
    "驱动清单",
    "时钟源",
    "系统版本锚",
    "采集时段窗",
    "外设静默态",
];

/// 拒收痕迹（漂移即拒采集，留此账面标记）。
pub const REFUSAL_NOTE: &str = "环境漂移-禁止入账";

/// 冻结波次语义：从 1 起严格递增（0 号保留 = 未冻结哨兵）。
pub const WAVE_NONE: u32 = 0;

/// 登记册渲染头（渲染面完整性判据锚）。
pub const REG_HEADER: &str = "UNX-F56001-ENVREG";

/// SHA-256 已知向量（实现自证锚：空串与 "abc"）。
pub const SHA256_EMPTY: &str =
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
pub const SHA256_ABC: &str =
    "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

/// SHA-256 轮常量 K（前 64 素数立方根小数部前 32 位，标准闭集）。
const SHA_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
    0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
    0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256 纯实现（no_std，单块流式内联；输入任意字节流 → 32 字节摘要）。
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let bit_len: u64 = (data.len() as u64).wrapping_mul(8);
    let mut msg: Vec<u8> = Vec::new();
    msg.extend_from_slice(data);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    let mut i: usize = 0;
    while i < 8 {
        msg.push((bit_len >> (56 - i * 8)) as u8);
        i += 1;
    }
    let mut chunk: usize = 0;
    while chunk < msg.len() {
        let mut w = [0u32; 64];
        let mut t: usize = 0;
        while t < 16 {
            let o = chunk + t * 4;
            w[t] = u32::from_be_bytes([msg[o], msg[o + 1], msg[o + 2], msg[o + 3]]);
            t += 1;
        }
        let mut t: usize = 16;
        while t < 64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
            t += 1;
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        let mut t: usize = 0;
        while t < 64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA_K[t])
                .wrapping_add(w[t]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
            t += 1;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
        chunk += 64;
    }
    let mut out = [0u8; 32];
    let mut i: usize = 0;
    while i < 8 {
        out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes());
        i += 1;
    }
    out
}

/// 32 字节摘要 → 64 位小写十六进制指纹（账面口径唯一）。
pub fn hex32(d: &[u8; 32]) -> String {
    let mut s = String::new();
    for b in d.iter() {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// 单维环境样本：维度闭集下标 × 值。
pub struct EnvSample {
    pub dim: usize,
    pub value: String,
}

/// 环境剖面（12 类维度登记册；登记序强制按维度闭集序，幂等可复算）。
pub struct EnvProfile {
    pub samples: Vec<EnvSample>,
}

impl EnvProfile {
    pub fn new() -> Self {
        EnvProfile { samples: Vec::new() }
    }

    /// 逐维落账：闭集校验 + 空值拒 + 重复拒 + 规范序拒（乱序落账即 ENV_ORDER_BAD）。
    pub fn record(&mut self, dim: usize, value: &str) -> Result<(), u32> {
        if dim >= DIM_COUNT {
            return Err(ENV_DIM_UNKNOWN);
        }
        if value.is_empty() {
            return Err(ENV_VAL_EMPTY);
        }
        for s in self.samples.iter() {
            if s.dim == dim {
                return Err(ENV_DIM_DUP);
            }
        }
        if let Some(last) = self.samples.last() {
            if dim <= last.dim {
                return Err(ENV_ORDER_BAD);
            }
        }
        self.samples.push(EnvSample { dim, value: value.to_string() });
        Ok(())
    }

    /// 12 维齐套（登记数 == 闭集规模即全齐，重复/越界在登记口已拦）。
    pub fn complete(&self) -> bool {
        self.samples.len() == DIM_COUNT
    }

    /// 规范序列化（哈希输入口径：下标|维度名|值，逐维换行）。
    pub fn canonical(&self) -> String {
        let mut out = String::new();
        for s in self.samples.iter() {
            out.push_str(&format!("{}|{}|{}\n", s.dim, DIM_NAMES[s.dim], s.value));
        }
        out
    }

    /// 环境哈希（SHA-256 × 规范序 → 64 位十六进制指纹，幂等）。
    pub fn env_hash(&self) -> String {
        hex32(&sha256(self.canonical().as_bytes()))
    }
}

/// 环境冻结件（剖面 + 波次 + 指纹 + 封账位；冻结后剖面拒改）。
pub struct EnvFreeze {
    pub profile: EnvProfile,
    pub wave: u32,
    pub frozen_hash: String,
    pub sealed: bool,
    /// 重冻结次数（纪律审计：每次严格升波次 +1）。
    pub refreeze_n: u32,
}

impl EnvFreeze {
    pub fn new(p: EnvProfile) -> Self {
        EnvFreeze { profile: p, wave: WAVE_NONE, frozen_hash: String::new(), sealed: false, refreeze_n: 0 }
    }

    /// 委托落账：封账后一律拒写（冻结语义硬闸）。
    pub fn record(&mut self, dim: usize, value: &str) -> Result<(), u32> {
        if self.sealed {
            return Err(ENV_FROZEN_WR);
        }
        self.profile.record(dim, value)
    }

    /// 首次冻结：12 维齐套 + 波次 ≥1 → 计算指纹并封账。
    pub fn freeze(&mut self, wave: u32) -> Result<String, u32> {
        if self.sealed {
            return Err(ENV_FROZEN_WR);
        }
        if wave == WAVE_NONE {
            return Err(ENV_WAVE_BACK);
        }
        if !self.profile.complete() {
            return Err(ENV_INCOMPLETE);
        }
        self.frozen_hash = self.profile.env_hash();
        self.wave = wave;
        self.sealed = true;
        Ok(self.frozen_hash.clone())
    }

    /// 重冻结：必须新波次且严格大于既有波次；重算指纹与账面一致才放行。
    pub fn refreeze(&mut self, wave: u32) -> Result<String, u32> {
        if !self.sealed {
            return Err(ENV_PROBE_STATE);
        }
        if wave <= self.wave {
            return Err(ENV_WAVE_BACK);
        }
        let re = self.profile.env_hash();
        if re != self.frozen_hash {
            return Err(ENV_TAMPER);
        }
        self.wave = wave;
        self.refreeze_n += 1;
        Ok(re)
    }

    /// 完整性复核：重算剖面哈希与冻结指纹逐位一致（内存篡改即 ENV_TAMPER）。
    pub fn verify_integrity(&self) -> Result<(), u32> {
        if !self.sealed {
            return Err(ENV_PROBE_STATE);
        }
        if self.profile.env_hash() != self.frozen_hash {
            return Err(ENV_TAMPER);
        }
        Ok(())
    }
}

/// 探针裁决：漂移清单（维度 × 冻结值 × 当前值）+ 拒收痕迹。
pub struct ProbeVerdict {
    pub pass: bool,
    pub drift_n: usize,
    pub drifts: Vec<(usize, String, String)>,
    pub code: u32,
    pub refusal_note: String,
}

/// 探针核：冻结剖面 vs 当前剖面逐维比对；任一维漂移即拒收并留痕。
pub fn probe(f: &EnvFreeze, cur: &EnvProfile) -> Result<ProbeVerdict, u32> {
    if !f.sealed {
        return Err(ENV_PROBE_STATE);
    }
    if !cur.complete() {
        return Err(ENV_INCOMPLETE);
    }
    let mut drifts: Vec<(usize, String, String)> = Vec::new();
    for s in f.profile.samples.iter() {
        let mut found: Option<&EnvSample> = None;
        for c in cur.samples.iter() {
            if c.dim == s.dim {
                found = Some(c);
            }
        }
        match found {
            None => return Err(ENV_DIM_UNKNOWN),
            Some(c) => {
                if c.value != s.value {
                    drifts.push((s.dim, s.value.clone(), c.value.clone()));
                }
            }
        }
    }
    let pass = drifts.is_empty();
    Ok(ProbeVerdict {
        pass,
        drift_n: drifts.len(),
        drifts,
        code: if pass { 0 } else { ENV_DRIFT },
        refusal_note: if pass { String::new() } else { REFUSAL_NOTE.to_string() },
    })
}

/// 探针日志单笔：波次 × 冻结指纹 × 结论 × 漂移数 × 诊断码。
pub struct ProbeRun {
    pub wave: u32,
    pub env_hash: String,
    pub pass: bool,
    pub drift_n: usize,
    pub code: u32,
}

/// 探针日志：逐次落账；申报结论与探针实况不符即 ENV_PROBE_HIDE（隐匿必抓）。
pub struct ProbeJournal {
    pub runs: Vec<ProbeRun>,
}

impl ProbeJournal {
    pub fn new() -> Self {
        ProbeJournal { runs: Vec::new() }
    }

    /// 落账闸：三重一致性（申报位、漂移数与结论自洽、冻结状态合法）。
    pub fn log(&mut self, f: &EnvFreeze, v: &ProbeVerdict, claimed_pass: bool) -> Result<(), u32> {
        if v.pass != claimed_pass {
            return Err(ENV_PROBE_HIDE);
        }
        if v.pass != (v.drift_n == 0) {
            return Err(ENV_PROBE_HIDE);
        }
        if !f.sealed || f.wave == WAVE_NONE {
            return Err(ENV_PROBE_STATE);
        }
        self.runs.push(ProbeRun {
            wave: f.wave,
            env_hash: f.frozen_hash.clone(),
            pass: v.pass,
            drift_n: v.drift_n,
            code: v.code,
        });
        Ok(())
    }

    /// 拒收笔数（留痕审计口径）。
    pub fn refusals(&self) -> usize {
        let mut n = 0usize;
        for r in self.runs.iter() {
            if !r.pass {
                n += 1;
            }
        }
        n
    }
}

/// 登记册渲染导出（人类可读清单：头行 + 12 维逐行 + 指纹尾行）。
pub fn render_registry(f: &EnvFreeze) -> String {
    let mut out = String::new();
    out.push_str(REG_HEADER);
    out.push('\n');
    for s in f.profile.samples.iter() {
        out.push_str(&format!("{}={}\n", DIM_NAMES[s.dim], s.value));
    }
    out.push_str(&format!("ENVHASH|{}|WAVE|{}\n", f.frozen_hash, f.wave));
    out
}

/// 渲染完整性核：行数 == 14、头行锚、12 维逐行前缀、指纹尾行齐。
pub fn render_complete(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() != DIM_COUNT + 2 {
        return false;
    }
    if lines[0] != REG_HEADER {
        return false;
    }
    let mut i: usize = 0;
    while i < DIM_COUNT {
        if !lines[i + 1].starts_with(DIM_NAMES[i]) {
            return false;
        }
        i += 1;
    }
    lines[DIM_COUNT + 1].starts_with("ENVHASH|")
}

/// 标准夹具：12 维基准机剖面（固定值，构造必成功）。
pub fn standard_profile() -> EnvProfile {
    let vals: [&str; DIM_COUNT] = [
        "VARIX-FW-7.3.1",
        "perf-locked-4.20GHz",
        "refresh-64ms",
        "plan-balanced-frozen",
        "svc-audit;svc-probe;svc-ledger",
        "/vxs/{bin,lib,etc}",
        "C:1.0TB-Varix|D:1.0TB-Win",
        "virtio;ahci;xhci",
        "tsc-invariant",
        "unxreal-anchor-1.0",
        "win-02:00-04:00",
        "periph-quiet-all",
    ];
    let mut p = EnvProfile::new();
    let mut d: usize = 0;
    while d < DIM_COUNT {
        p.record(d, vals[d]).expect("fixture dim record must succeed");
        d += 1;
    }
    p
}

/// 标准夹具：12 维齐套剖面于第 3 波次冻结（构造必成功）。
pub fn standard_freeze() -> EnvFreeze {
    let mut f = EnvFreeze::new(standard_profile());
    f.freeze(3).expect("standard freeze must succeed");
    f
}

/// 注入核：11 注入必抓（未知维度/空值/重复/乱序/未齐冻结/冻结后写/
/// 波次回退/指纹篡改/漂移拒收/漂移隐匿/未冻结探针）。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    // 1) 未知维度（dim == DIM_COUNT 越闭集）
    let mut p1 = EnvProfile::new();
    if p1.record(DIM_COUNT, "x").err() == Some(ENV_DIM_UNKNOWN) {
        caught += 1;
    }
    // 2) 空值维度
    if p1.record(DIM_FW, "").err() == Some(ENV_VAL_EMPTY) {
        caught += 1;
    }
    // 3) 维度重复登记
    p1.record(DIM_FW, "v").expect("first record");
    if p1.record(DIM_FW, "v2").err() == Some(ENV_DIM_DUP) {
        caught += 1;
    }
    // 4) 登记乱序（先 2 后 1）
    let mut p4 = EnvProfile::new();
    p4.record(DIM_MEMREFRESH, "v").expect("order rec 2");
    if p4.record(DIM_CPUFREQ, "v").err() == Some(ENV_ORDER_BAD) {
        caught += 1;
    }
    // 5) 12 维未齐即冻结
    let mut f5 = EnvFreeze::new(p4);
    if f5.freeze(1).err() == Some(ENV_INCOMPLETE) {
        caught += 1;
    }
    // 6) 冻结后写拒
    let mut f6 = standard_freeze();
    if f6.record(DIM_PERIPH, "tamper").err() == Some(ENV_FROZEN_WR) {
        caught += 1;
    }
    // 7) 波次回退（既有 3，重冻结 2）
    if f6.refreeze(2).err() == Some(ENV_WAVE_BACK) {
        caught += 1;
    }
    // 8) 冻结指纹篡改（账面指纹被改 → 完整性复核必炸）
    let mut f8 = standard_freeze();
    f8.frozen_hash = "tampered-fingerprint".to_string();
    if f8.verify_integrity().err() == Some(ENV_TAMPER) {
        caught += 1;
    }
    // 9) 探针漂移拒收（电源计划被改 → 拒收痕迹必留）
    let f9 = standard_freeze();
    let mut cur = standard_profile();
    cur.samples[DIM_POWERPLAN].value = "plan-HIGH-PERF".to_string();
    let v9 = probe(&f9, &cur).expect("drift probe must run");
    if !v9.pass && v9.drift_n == 1 && v9.code == ENV_DRIFT && v9.refusal_note == REFUSAL_NOTE {
        caught += 1;
    }
    // 10) 漂移隐匿（申报 pass 与实况不符）
    let mut j10 = ProbeJournal::new();
    if j10.log(&f9, &v9, true).err() == Some(ENV_PROBE_HIDE) {
        caught += 1;
    }
    // 11) 未冻结即探针（状态机非法）
    let uf = EnvFreeze::new(standard_profile());
    if probe(&uf, &standard_profile()).err() == Some(ENV_PROBE_STATE) {
        caught += 1;
    }
    (caught, 11)
}

/// 判据入口（CheckSet）。
pub fn run_unx56001_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F56001");
    // J1-1 12 维登记全量 + 哈希指纹 64 位口径
    let p = standard_profile();
    let ok = p.complete() && p.env_hash().len() == 64;
    s.add("56001-J1-12维登记全量", ok, if ok { "" } else { "12 维不全或指纹口径漂移" });
    // J1-2 探针两次连跑 env_hash 一致（幂等）
    let ok = standard_profile().env_hash() == standard_profile().env_hash();
    s.add("56001-J1-哈希连跑一致", ok, if ok { "" } else { "同剖面两次哈希漂移" });
    // J1-3 SHA-256 空串已知向量
    let ok = hex32(&sha256(b"")) == SHA256_EMPTY;
    s.add("56001-J1-SHA256空串向量", ok, if ok { "" } else { "摘要实现失准" });
    // J1-4 SHA-256 "abc" 已知向量
    let ok = hex32(&sha256(b"abc")) == SHA256_ABC;
    s.add("56001-J1-SHA256abc向量", ok, if ok { "" } else { "摘要实现失准" });
    // J1-5 单维篡改 ×12 全检出：任一维值变更 → 环境哈希必变
    let base = standard_profile().env_hash();
    let mut all_diff = true;
    let mut d: usize = 0;
    while d < DIM_COUNT {
        let mut p2 = standard_profile();
        p2.samples[d].value = format!("tampered-{}", d);
        if p2.env_hash() == base {
            all_diff = false;
        }
        d += 1;
    }
    s.add("56001-J1-单维篡改哈希必变", all_diff, if all_diff { "" } else { "存在篡改不敏感维" });
    // J2-1 冻结语义：封账后写拒
    let mut f = standard_freeze();
    let ok = f.sealed && f.wave == 3 && f.record(DIM_PERIPH, "x").err() == Some(ENV_FROZEN_WR);
    s.add("56001-J2-冻结后拒写", ok, if ok { "" } else { "冻结闸失效" });
    // J2-2 重冻结：升波次放行 + 回退拒 + 指纹不漂
    let mut f2 = standard_freeze();
    let h5 = f2.refreeze(5);
    let mut ok = h5.is_ok() && f2.wave == 5 && f2.refreeze_n == 1;
    if let Ok(h) = h5 {
        ok = ok && h == f2.frozen_hash;
    }
    ok = ok && f2.refreeze(4).err() == Some(ENV_WAVE_BACK) && f2.wave == 5;
    s.add("56001-J2-重冻结升波纪律", ok, if ok { "" } else { "重冻结波次纪律破" });
    // J2-3 完整性复核：正常过 + 篡改即抓
    let f3 = standard_freeze();
    let mut f3b = standard_freeze();
    f3b.frozen_hash = "x".to_string();
    let ok = f3.verify_integrity().is_ok() && f3b.verify_integrity().err() == Some(ENV_TAMPER);
    s.add("56001-J2-指纹完整性复核", ok, if ok { "" } else { "完整性核失效" });
    // J3-1 同剖面探针 pass（零漂移零码）
    let f4 = standard_freeze();
    let v = probe(&f4, &standard_profile()).expect("same-profile probe must run");
    let ok = v.pass && v.drift_n == 0 && v.code == 0 && v.refusal_note.is_empty();
    s.add("56001-J3-同剖面探针通过", ok, if ok { "" } else { "同剖面误报漂移" });
    // J3-2 漂移指认：单维漂移定位准确 + 拒收痕迹原文
    let mut cur = standard_profile();
    cur.samples[DIM_CLOCKSRC].value = "tsc-drifted".to_string();
    let v2 = probe(&f4, &cur).expect("drift probe must run");
    let ok = !v2.pass
        && v2.drifts.len() == 1
        && v2.drifts[0].0 == DIM_CLOCKSRC
        && v2.drifts[0].1 == "tsc-invariant"
        && v2.drifts[0].2 == "tsc-drifted"
        && v2.refusal_note == REFUSAL_NOTE;
    s.add("56001-J3-漂移指认与拒收痕迹", ok, if ok { "" } else { "漂移定位或痕迹失真" });
    // J3-3 探针落账：两笔齐 + 拒收一笔带诊断码
    let mut j = ProbeJournal::new();
    j.log(&f4, &v, true).expect("clean run must log");
    j.log(&f4, &v2, false).expect("drift run must log");
    let ok = j.runs.len() == 2
        && j.refusals() == 1
        && j.runs[1].code == ENV_DRIFT
        && j.runs[1].wave == 3
        && j.runs[0].drift_n == 0;
    s.add("56001-J3-探针逐次落账", ok, if ok { "" } else { "落账语义漂移" });
    // J3-4 渲染面：登记册导出齐整（头行 + 12 维 + 指纹尾行）
    let text = render_registry(&standard_freeze());
    let ok = render_complete(&text)
        && text.contains(&format!("{}=tsc-invariant", DIM_NAMES[DIM_CLOCKSRC]))
        && text.contains("WAVE|3");
    s.add("56001-J3-登记册渲染导出", ok, if ok { "" } else { "渲染面漂移" });
    // 边界-空集：0 维剖面不可冻结且状态不被污染
    let mut fe = EnvFreeze::new(EnvProfile::new());
    let ok = fe.freeze(1).err() == Some(ENV_INCOMPLETE) && !fe.sealed && fe.wave == WAVE_NONE;
    s.add("56001-边界-空剖面拒冻结", ok, if ok { "" } else { "空剖面冻结闸失效" });
    // 边界-越界：维度下标越闭集拒收且不落账
    let mut pe = EnvProfile::new();
    let ok = pe.record(DIM_COUNT + 5, "x").err() == Some(ENV_DIM_UNKNOWN) && pe.samples.is_empty();
    s.add("56001-边界-维度越界拒收", ok, if ok { "" } else { "越界维度漏收" });
    // 注入核 11/11
    let (c, n) = injection_core();
    let ok = c == n;
    let detail = if ok { String::new() } else { format!("注入 {c}/{n}") };
    s.add_dyn("56001-注入核必抓", ok, &detail);
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx56001_all_checks_green() {
        let set = run_unx56001_checks();
        assert!(set.all_passed(), "UNX-F56001 checks must be green: {}", set.red_items_text());
    }

    #[test]
    fn unx56001_env_hash_idempotent() {
        assert_eq!(standard_profile().env_hash(), standard_profile().env_hash());
    }
}
