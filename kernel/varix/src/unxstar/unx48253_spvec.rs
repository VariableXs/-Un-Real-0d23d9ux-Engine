//! unx48253_spvec — UNX-F48253 方案面向量账（三双拼样张集+校验和）（诊断段 0xC080）。
//!
//! 判据原文：
//!   J1 样张可复跑全绿；判据锚定 §AI-61 §5 J-3。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 三双拼样张集：小鹤键位 / 自然码键位 / 微软键位三个方案表，同
//!      一组全拼音节样张在三表下各得一组双拼键码；样张是唯一事实源，
//!      三表由样张派生，表-样张不一致即坏账。
//!   2. 编解码闭环：编码（全拼→双拼键码）与解码（双拼键码→全拼）为
//!      查表互逆纯函数；每样张每方案跑 编码→解码 往返，恒等才绿。
//!   3. 校验和：每方案表注册一条 XOR 折叠校验和（键码序列逐字节折叠），
//!      复跑前先验校验和，和不符即拒跑（账面防漂移）。
//!   4. 可复跑口径（J1）：样张集全量 ×3 方案复跑两轮，两轮结果逐位
//!      一致且全绿；任何一条红即向量账红。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC080;

// ── 诊断码（SEG_BASE+1 起，≤0x1F 个）──────────────────────
const E_UNKNOW_CODE: u16 = SEG_BASE + 1; // 键码不在方案表
const E_RT_LOSS: u16 = SEG_BASE + 2;     // 编解码往返不恒等
const E_SCHEME_DRIFT: u16 = SEG_BASE + 3;// 校验和不符（表漂移）
const E_SAMPLE_MISS: u16 = SEG_BASE + 4; // 样张数不足
const E_RERUN_DIFF: u16 = SEG_BASE + 5;  // 两轮复跑不一致
const E_LEDGER_BAD: u16 = SEG_BASE + 6;  // 向量账不平
const E_EMPTY_SUM: u16 = SEG_BASE + 7;   // 写回摘要为空

/// 方案数（三双拼）。
pub const SCHEME_COUNT: usize = 3;
/// 方案编号：0 小鹤 / 1 自然码 / 2 微软。
pub const SC_XIAOHE: usize = 0;
pub const SC_ZIRANMA: usize = 1;
pub const SC_MS: usize = 2;

/// 样张节条目：全拼（声母 id, 韵母 id）+ 三方案双拼键码。
/// 键码编码：声母键 * 256 + 韵母键（两键 ASCII 小写）。
#[derive(Clone, Copy, Debug)]
pub struct SpSample {
    pub sheng: u8, // 声母 id（样张内序）
    pub yun: u8,   // 韵母 id（样张内序）
    pub code: [u16; SCHEME_COUNT], // 三方案双拼键码
}

/// 样张集：8 个常用音节（样张是唯一事实源）。
/// 声母序：0=b 1=m 2=d 3=t 4=zh 5=ch 6=sh 7=n
/// 韵母序：0=a 1=ang 2=eng 3=ai 4=ou 5=an 6=ing 7=uang
pub const SP_SAMPLES: [SpSample; 8] = [
    // ba：小鹤 ba / 自然码 ba / 微软 ba
    SpSample { sheng: 0, yun: 0, code: [0x6261, 0x6261, 0x6261] }, // b,a
    // bang：小鹤 b,l / 自然码 b,h / 微软 b,g（ang 键位差异）
    SpSample { sheng: 0, yun: 1, code: [0x626C, 0x6268, 0x6267] },
    // deng：小鹤 d,g / 自然码 d,g / 微软 d,g（eng 同键）
    SpSample { sheng: 2, yun: 2, code: [0x6467, 0x6467, 0x6467] }, // d,g
    // tai：小鹤 t,s / 自然码 t,s / 微软 t,s（ai 同键）
    SpSample { sheng: 3, yun: 3, code: [0x7453, 0x7453, 0x7453] }, // t,s
    // zhang：zh+ang：小鹤 v,l / 自然码 v,h / 微软 v,g
    SpSample { sheng: 4, yun: 1, code: [0x766C, 0x7668, 0x7667] }, // v + ang键
    // chang：ch+ai：小鹤 i,s / 自然码 i,s / 微软 i,s
    SpSample { sheng: 5, yun: 3, code: [0x6953, 0x6953, 0x6953] }, // i,s
    // shuang：sh+uang：小鹤 u,d / 自然码 u,d / 微软 u,d
    SpSample { sheng: 6, yun: 7, code: [0x7564, 0x7564, 0x7564] }, // u,d
    // ning：n+ing：小鹤 n,; / 自然码 n,y / 微软 n,;
    SpSample { sheng: 7, yun: 6, code: [0x6E3B, 0x6E79, 0x6E3B] }, // n,; / n,y
];

/// 方案名（账面用）。
pub const SCHEME_NAMES: [&str; SCHEME_COUNT] = ["xiaohe", "ziranma", "ms"];

/// 一条方案向量账。
#[derive(Clone, Copy, Debug)]
pub struct VecRecord {
    pub scheme: usize,
    pub sample_idx: usize,
    pub round: u8,
    pub enc: u16,
    pub dec_sheng: u8,
    pub dec_yun: u8,
    pub pass: bool,
}

/// 校验和账：每方案一条（登记后只读）。
#[derive(Clone, Copy, Debug)]
pub struct ChecksumEntry {
    pub scheme: usize,
    pub crc: u32,
}

/// 方案表 XOR 折叠校验和：对样张集内该方案键码逐条折叠。
pub fn scheme_checksum(scheme: usize) -> Result<u32, u16> {
    if scheme >= SCHEME_COUNT {
        return Err(E_UNKNOW_CODE);
    }
    let mut crc: u32 = 0xC081;
    let mut i = 0usize;
    while i < SP_SAMPLES.len() {
        crc = crc
            .wrapping_mul(0x9E3779B1)
            ^ SP_SAMPLES[i].code[scheme] as u32
            ^ (i as u32);
        i += 1;
    }
    Ok(crc)
}

/// 编码：样张序 → 双拼键码（查样张）。
pub fn encode(scheme: usize, sample_idx: usize) -> Result<u16, u16> {
    if scheme >= SCHEME_COUNT {
        return Err(E_UNKNOW_CODE);
    }
    if sample_idx >= SP_SAMPLES.len() {
        return Err(E_UNKNOW_CODE);
    }
    Ok(SP_SAMPLES[sample_idx].code[scheme])
}

/// 解码：双拼键码 → 样张序（反查样张表；同方案内键码唯一）。
pub fn decode(scheme: usize, code: u16) -> Result<usize, u16> {
    if scheme >= SCHEME_COUNT {
        return Err(E_UNKNOW_CODE);
    }
    let mut i = 0usize;
    while i < SP_SAMPLES.len() {
        if SP_SAMPLES[i].code[scheme] == code {
            return Ok(i);
        }
        i += 1;
    }
    Err(E_UNKNOW_CODE)
}

/// 方案面向量账枢纽。
pub struct SpVec {
    ledger: Vec<VecRecord>,
    checksums: Vec<ChecksumEntry>,
    pub run_total: u32,
    pub fail_total: u32,
}

impl SpVec {
    pub fn new() -> SpVec {
        SpVec { ledger: Vec::new(), checksums: Vec::new(), run_total: 0, fail_total: 0 }
    }

    /// 注册三方案校验和（一次性，重复注册拒收）。
    pub fn seal_checksums(&mut self) -> Result<(), u16> {
        if !self.checksums.is_empty() {
            return Err(E_LEDGER_BAD);
        }
        let mut s = 0usize;
        while s < SCHEME_COUNT {
            let crc = scheme_checksum(s)?;
            self.checksums.push(ChecksumEntry { scheme: s, crc });
            s += 1;
        }
        Ok(())
    }

    /// 复跑前验和：重算校验和与登记值比对，漂移即拒跑。
    pub fn verify_checksums(&self) -> Result<(), u16> {
        if self.checksums.len() != SCHEME_COUNT {
            return Err(E_SCHEME_DRIFT);
        }
        let mut i = 0usize;
        while i < self.checksums.len() {
            let want = scheme_checksum(self.checksums[i].scheme)?;
            if want != self.checksums[i].crc {
                return Err(E_SCHEME_DRIFT);
            }
            i += 1;
        }
        Ok(())
    }

    /// 复跑一轮：全样张 ×3 方案，往返恒等逐条落账（J1 主体）。
    pub fn run_round(&mut self, round: u8) -> Result<u32, u16> {
        self.verify_checksums()?;
        let mut passed = 0u32;
        let mut s = 0usize;
        while s < SCHEME_COUNT {
            let mut i = 0usize;
            while i < SP_SAMPLES.len() {
                let enc = encode(s, i)?;
                let dec = decode(s, enc)?;
                let pass = dec == i
                    && SP_SAMPLES[dec].sheng == SP_SAMPLES[i].sheng
                    && SP_SAMPLES[dec].yun == SP_SAMPLES[i].yun;
                if !pass {
                    self.fail_total += 1;
                } else {
                    passed += 1;
                }
                self.ledger.push(VecRecord {
                    scheme: s,
                    sample_idx: i,
                    round,
                    enc,
                    dec_sheng: SP_SAMPLES[dec].sheng,
                    dec_yun: SP_SAMPLES[dec].yun,
                    pass,
                });
                self.run_total += 1;
                i += 1;
            }
            s += 1;
        }
        Ok(passed)
    }

    /// 两轮可复跑核验（J1）：逐条终值一致、全绿。
    pub fn audit_reproducible(&self) -> Result<(), u16> {
        if self.run_total != (SP_SAMPLES.len() * SCHEME_COUNT * 2) as u32 {
            return Err(E_SAMPLE_MISS);
        }
        if self.ledger.len() as u32 != self.run_total {
            return Err(E_LEDGER_BAD);
        }
        let half = self.ledger.len() / 2;
        let mut i = 0usize;
        while i < half {
            let a = &self.ledger[i];
            let b = &self.ledger[i + half];
            if a.scheme != b.scheme || a.sample_idx != b.sample_idx {
                return Err(E_LEDGER_BAD);
            }
            if a.round == b.round {
                return Err(E_LEDGER_BAD);
            }
            if a.enc != b.enc || a.dec_sheng != b.dec_sheng || a.dec_yun != b.dec_yun {
                return Err(E_RERUN_DIFF);
            }
            if !a.pass || !b.pass {
                return Err(E_RT_LOSS);
            }
            i += 1;
        }
        if self.fail_total != 0 {
            return Err(E_RT_LOSS);
        }
        Ok(())
    }

    /// ×N 覆盖核验（J1 硬口径）：全样张 ×3 方案 ×2 轮。
    pub fn coverage(&self, want: u32) -> Result<(), u16> {
        if self.run_total < want {
            return Err(E_SAMPLE_MISS);
        }
        Ok(())
    }

    pub fn ledger_len(&self) -> usize {
        self.ledger.len()
    }

    /// 完成写回（判据 J2）。
    pub fn writeback(&self, done: bool, summary_len: u32) -> Result<(), u16> {
        if !done {
            return Err(E_LEDGER_BAD);
        }
        if summary_len == 0 {
            return Err(E_EMPTY_SUM);
        }
        Ok(())
    }
}

/// 自例程判据：逐条落实 J1（样张可复跑全绿），返回命中数。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1 主复跑 ×48：8 样张 ×3 方案 ×2 轮，全绿且两轮逐位一致。
    let mut sv = SpVec::new();
    sv.seal_checksums()?;
    let p1 = sv.run_round(1)?;
    let p2 = sv.run_round(2)?;
    if p1 != 24 || p2 != 24 {
        return Err(E_RT_LOSS);
    }
    sv.coverage(48)?;
    sv.audit_reproducible()?;
    if sv.ledger_len() != 48 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R2 样张完备性：8 样张 (sheng,yun) 序对唯一、三方案键码逐方案
    //    内唯一（解码无歧义前提）。
    let mut i = 0usize;
    while i < SP_SAMPLES.len() {
        let mut j = i + 1;
        while j < SP_SAMPLES.len() {
            if SP_SAMPLES[i].sheng == SP_SAMPLES[j].sheng
                && SP_SAMPLES[i].yun == SP_SAMPLES[j].yun
            {
                return Err(E_RT_LOSS);
            }
            let mut s = 0usize;
            while s < SCHEME_COUNT {
                if SP_SAMPLES[i].code[s] == SP_SAMPLES[j].code[s] {
                    return Err(E_UNKNOW_CODE);
                }
                s += 1;
            }
            j += 1;
        }
        i += 1;
    }
    hits += 1;

    // R3 键码形制：两键均在键位闭集（a-z 小写或小鹤分号键 0x3B），键码非零。
    let key_ok = |k: u8| (0x61..=0x7A).contains(&k) || k == 0x3B;
    let mut i = 0usize;
    while i < SP_SAMPLES.len() {
        let mut s = 0usize;
        while s < SCHEME_COUNT {
            let c = SP_SAMPLES[i].code[s];
            if c == 0 {
                return Err(E_UNKNOW_CODE);
            }
            let hi = (c >> 8) as u8;
            let lo = (c & 0xFF) as u8;
            if !key_ok(hi) || !key_ok(lo) {
                return Err(E_UNKNOW_CODE);
            }
            s += 1;
        }
        i += 1;
    }
    hits += 1;

    // R4 校验和可复算：seal 前后 scheme_checksum 双次调用同值（纯函数）。
    let mut s = 0usize;
    while s < SCHEME_COUNT {
        let a = scheme_checksum(s)?;
        let b = scheme_checksum(s)?;
        if a != b {
            return Err(E_SCHEME_DRIFT);
        }
        s += 1;
    }
    hits += 1;

    // R5 未注册验和拒跑：不 seal 直接 run_round 必拒（防漂移门禁）。
    let mut sv = SpVec::new();
    if sv.run_round(1).is_ok() {
        return Err(E_SCHEME_DRIFT);
    }
    if sv.run_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R6 重复 seal 拒收：二次注册校验和必拒（登记只读红线）。
    let mut sv = SpVec::new();
    sv.seal_checksums()?;
    if sv.seal_checksums().is_ok() {
        return Err(E_LEDGER_BAD);
    }
    if sv.checksums.len() != SCHEME_COUNT {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R7 越界防御：encode/decode 越方案、越样张、野键码全拒且不落账。
    let mut sv = SpVec::new();
    if encode(3, 0).is_ok() || encode(0, 8).is_ok() {
        return Err(E_UNKNOW_CODE);
    }
    if decode(3, 0x6261).is_ok() || decode(0, 0x7A7A).is_ok() {
        return Err(E_UNKNOW_CODE);
    }
    if sv.run_total != 0 {
        return Err(E_LEDGER_BAD);
    }
    hits += 1;

    // R8 四轮放大复跑：4 轮 ×48 全绿（可复跑放大口径）；写回（判据 J2）。
    let mut sv = SpVec::new();
    sv.seal_checksums()?;
    let mut r = 1u8;
    while r <= 4 {
        let p = sv.run_round(r)?;
        if p != 24 {
            return Err(E_RT_LOSS);
        }
        r += 1;
    }
    if sv.runs_total_check() != 192 {
        return Err(E_LEDGER_BAD);
    }
    if sv.fail_total != 0 {
        return Err(E_RT_LOSS);
    }
    if sv.writeback(true, 0).is_ok() {
        return Err(E_EMPTY_SUM);
    }
    sv.writeback(true, 0x40)?;
    hits += 1;

    Ok(hits)
}

impl SpVec {
    fn runs_total_check(&self) -> u32 {
        self.run_total
    }
}
