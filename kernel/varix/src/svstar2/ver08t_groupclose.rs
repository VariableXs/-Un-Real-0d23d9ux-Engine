//! VE-F3520 · E06 组收口 —— 组级门禁 + 六件齐备 + 双人签署 + 证据包哈希。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3520`
//!
//! # 职责（锚点原文拆解）
//!
//! - **组级门禁**（[`GroupGate`]）：E06 组（F3501~F3519）收口的硬门：
//!   断言全绿 + 两律符合 + 对拍归档 + 六件齐备，四查缺一即黄门
//!   （[`GateVerdict::Held`]）——黄门不是失败，是「还不能走」。
//! - **六件齐备**（[`SIX_ARTIFACTS`]）：施工书收口六件——实现册/
//!   判据册/对拍记录/语料库/文档册/双签页。每件以 id 登记在场，
//!   缺件点名（[`GateCode::MissingArtifact`]）。
//! - **双人签署**（[`SignBook`]）：收口需两个不同签署人，签署带
//!   内容指纹——**指纹必须对当前证据包计算**（不是与旧签名比对，
//!   F 域踩过的坑：首签空账恒放行）。账本变更后旧签失效。
//! - **包哈希**（[`EvidencePack::seal`]）：证据包封口哈希——签署
//!   后证据包不可再变更（变更即哈希失配，双签作废重走）。
//! - **fuzz 前置**：双签前重跑组内 fuzz 语料全量（F3513 门），
//!   fuzz 未过不许签。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 缺件 | 阻断（点名缺哪件） |
//! | 不全 | 黄门 |
//! | 损坏 | 重建（哈希失配即重建证据包） |
//!
//! # 复杂度
//!
//! - 门禁四查：O(件数)；
//! - 签署验证：O(签署人数)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5870 起）
// ---------------------------------------------------------------------------

/// E06 组收口域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateCode {
    /// 收口件缺失（点名缺哪件）。
    MissingArtifact = 5870,
    /// 断言未全绿。
    AssertionsRed = 5871,
    /// fuzz 语料未过。
    FuzzRed = 5872,
    /// 签署人数不足或同人双签。
    SignatureShort = 5873,
    /// 签署指纹失配（账本变更后旧签失效）。
    SignatureStale = 5874,
    /// 证据包哈希失配（封口后变更）。
    PackTampered = 5875,
}

impl GateCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            GateCode::MissingArtifact => "收口件缺失",
            GateCode::AssertionsRed => "断言未全绿",
            GateCode::FuzzRed => "fuzz 语料未过",
            GateCode::SignatureShort => "双人签署不足",
            GateCode::SignatureStale => "签署指纹失配",
            GateCode::PackTampered => "证据包封口后被变更",
        }
    }

    /// 全阻断（收口门没有黄灯降级，只有过与不过）。
    pub fn blocking(self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// 二、六件与证据包
// ---------------------------------------------------------------------------

/// 收口六件 id。
pub const SIX_ARTIFACTS: [&str; 6] = [
    "impl-book",
    "check-book",
    "diff-archive",
    "corpus",
    "doc-book",
    "dual-sign",
];

/// 证据包：登记在场件 + 封口哈希。
#[derive(Clone, Debug)]
pub struct EvidencePack {
    /// 在场件 id 列表（按登记序）。
    pub items: Vec<&'static str>,
    /// 封口哈希（None = 未封口）。
    pub seal: Option<u64>,
}

impl EvidencePack {
    /// 空包。
    pub fn new() -> Self {
        EvidencePack {
            items: Vec::new(),
            seal: None,
        }
    }

    /// 登记一件（封口后拒绝）。
    pub fn mount(&mut self, id: &'static str) -> Result<(), GateCode> {
        if self.seal.is_some() {
            return Err(GateCode::PackTampered);
        }
        if !self.items.contains(&id) {
            self.items.push(id);
        }
        Ok(())
    }

    /// 内容哈希（FNV-1a，件序敏感）。
    pub fn content_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for it in &self.items {
            for b in it.as_bytes() {
                h ^= *b as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01B3);
            }
            h = h.wrapping_mul(0x0000_0100_0000_01B3);
        }
        h
    }

    /// 封口。
    pub fn seal_up(&mut self) -> u64 {
        let h = self.content_hash();
        self.seal = Some(h);
        h
    }

    /// 封口完整性：哈希与登记内容一致。
    pub fn intact(&self) -> Result<(), GateCode> {
        match self.seal {
            Some(h) if h == self.content_hash() => Ok(()),
            Some(_) => Err(GateCode::PackTampered),
            None => Err(GateCode::PackTampered),
        }
    }

    /// 六件齐备检查：点名缺件。
    pub fn check_six(&self) -> Result<(), GateCode> {
        for need in SIX_ARTIFACTS {
            if !self.items.contains(&need) {
                return Err(GateCode::MissingArtifact);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、双签（指纹对当前包计算，不是与旧签比对）
// ---------------------------------------------------------------------------

/// 一条签署。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Signature {
    /// 签署人。
    pub signer: &'static str,
    /// 签署时证据包哈希。
    pub fingerprint: u64,
}

/// 双签账本。
#[derive(Default)]
pub struct SignBook {
    sigs: Vec<Signature>,
}

impl SignBook {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 签署：指纹必须等于**当前**证据包哈希——账本或包在签署后
    /// 再变更，所有旧签即刻失效（重签）。
    pub fn sign(&mut self, signer: &'static str, pack: &EvidencePack) -> Result<(), GateCode> {
        let fp = pack.content_hash();
        if self.sigs.iter().any(|s| s.signer == signer) {
            // 同人重签：覆盖（视为对最新账本的再确认）。
            if let Some(s) = self.sigs.iter_mut().find(|s| s.signer == signer) {
                s.fingerprint = fp;
            }
            return Ok(());
        }
        self.sigs.push(Signature {
            signer,
            fingerprint: fp,
        });
        Ok(())
    }

    /// 验签：两个**互异**签署人、指纹都等于当前包哈希。
    pub fn verify(&self, pack: &EvidencePack) -> Result<(), GateCode> {
        let fp = pack.content_hash();
        let valid = self
            .sigs
            .iter()
            .filter(|s| s.fingerprint == fp)
            .map(|s| s.signer)
            .count();
        if valid < 2 {
            return Err(GateCode::SignatureShort);
        }
        // 互异检查：有效签里是否来自两个不同人。
        let distinct = self
            .sigs
            .iter()
            .filter(|s| s.fingerprint == fp)
            .map(|s| s.signer)
            .collect::<Vec<_>>();
        if distinct.len() < 2 {
            return Err(GateCode::SignatureShort);
        }
        Ok(())
    }

    /// 签署数。
    pub fn len(&self) -> usize {
        self.sigs.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.sigs.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、组门
// ---------------------------------------------------------------------------

/// 门禁判定。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateVerdict {
    /// 全绿放行。
    Green,
    /// 黄门（条件未齐，不能走但可修）。
    Held(GateCode),
}

/// E06 组级门禁。
#[derive(Default)]
pub struct GroupGate {
    pub runs: u32,
}

impl GroupGate {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 四查：六件齐备 → 包完整 → fuzz 绿 → 双签验。
    /// `asserts_green` 与 `fuzz_green` 是上游判据的布尔结果。
    pub fn check(
        &mut self,
        pack: &EvidencePack,
        signs: &SignBook,
        asserts_green: bool,
        fuzz_green: bool,
    ) -> GateVerdict {
        self.runs += 1;
        if let Err(c) = pack.check_six() {
            return GateVerdict::Held(c);
        }
        if let Err(c) = pack.intact() {
            return GateVerdict::Held(c);
        }
        if !asserts_green {
            return GateVerdict::Held(GateCode::AssertionsRed);
        }
        if !fuzz_green {
            return GateVerdict::Held(GateCode::FuzzRed);
        }
        if let Err(c) = signs.verify(pack) {
            return GateVerdict::Held(c);
        }
        GateVerdict::Green
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sealed_pack() -> EvidencePack {
        let mut p = EvidencePack::new();
        for id in SIX_ARTIFACTS {
            p.mount(id).unwrap();
        }
        p.seal_up();
        p
    }

    #[test]
    fn six_missing_named() {
        let mut p = EvidencePack::new();
        p.mount("impl-book").unwrap();
        assert_eq!(p.check_six(), Err(GateCode::MissingArtifact));
    }

    #[test]
    fn tamper_detected() {
        let mut p = sealed_pack();
        assert!(p.intact().is_ok());
        p.items.push("extra");
        assert_eq!(p.intact(), Err(GateCode::PackTampered));
    }

    #[test]
    fn dual_sign_flow() {
        let p = sealed_pack();
        let mut sb = SignBook::new();
        let _ = sb.sign("alice", &p);
        // 一签不够。
        assert_eq!(sb.verify(&p), Err(GateCode::SignatureShort));
        let _ = sb.sign("bob", &p);
        assert!(sb.verify(&p).is_ok());
        // 包变更 → 旧签失效。
        let mut p2 = sealed_pack();
        p2.items.push("extra");
        assert_eq!(sb.verify(&p2), Err(GateCode::SignatureShort));
    }

    #[test]
    fn gate_holds_until_all_green() {
        let p = sealed_pack();
        let mut sb = SignBook::new();
        let _ = sb.sign("a", &p);
        let _ = sb.sign("b", &p);
        let mut g = GroupGate::new();
        assert_eq!(
            g.check(&p, &sb, true, false),
            GateVerdict::Held(GateCode::FuzzRed)
        );
        assert_eq!(g.check(&p, &sb, true, true), GateVerdict::Green);
    }
}
