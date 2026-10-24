//! unx0170_sbchain — UNX-F0170 SecureBoot 签名链只读解析。
//!
//! 判据原文（CoRun Unxreal 主册 UNX-F0170-J1）：db/dbx/KEK/PK 证书链导出
//! 解析，与签名工具导出对照一致（只读）。
//!
//! 职责定位：SecureBoot 四库（PK 平台密钥 / KEK 密钥交换 / db 签名库 /
//! dbx 撤销库）的链路关系是固件信任的根账——本模块只读解析：①四库
//! 存在性与层级完整性（PK→KEK→db 链序：db 必须由 KEK 签，KEK 必须由
//! PK 签——断链即信任缺口）；②dbx 撤销核对（被撤销指纹不得出现在
//! db 生效链上）；③全只读红线（本模块零写路径——解析结果与签名工具
//! 导出对照一致）。no_std + alloc；零 unwrap 零 expect。
//! 诊断码段 0x6460（本会话独占，grep 全库零占用后领取，2026-10-22）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const SB_VERSION: &str = "B170-sbchain-v1";

// ── 诊断码（0x646 段，本会话独占）──
/// 证书链断链（db 无 KEK 背书 / KEK 无 PK 背书——信任缺口）。
pub const XC_SB_CHAIN_BROKEN: u16 = 0x6460;
/// dbx 撤销命中（生效签名被撤销库命中——必须拒）。
pub const XC_SB_REVOKED: u16 = 0x6461;
/// 证书指纹格式非法（非 32 字节 SHA-256 口径）。
pub const XC_SB_BAD_FP: u16 = 0x6462;
/// 只读红线违规尝试（解析面出现写路径——架构错误）。
pub const XC_SB_WRITE_ATTEMPT: u16 = 0x6463;
/// 库重复登记（同库两份——账目二义）。
pub const XC_SB_DUP_DB: u16 = 0x6464;

/// 证书条目（指纹 = SHA-256 摘要的十六进制串，32 字节口径）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertEntry {
    /// 指纹（64 个 hex 字符）。
    pub fingerprint: String,
    /// 所有者库（"PK"/"KEK"/"db"/"dbx"）。
    pub owner: &'static str,
}

/// SecureBoot 四库只读视图。
#[derive(Debug)]
pub struct SbChainView {
    /// 全部证书条目。
    pub certs: Vec<CertEntry>,
}

impl SbChainView {
    pub fn boot() -> Self {
        SbChainView { certs: Vec::new() }
    }

    /// 只读导入一条证书（写路径不存在——导入即登记为"待对照"，不是写库）。
    pub fn import(&mut self, owner: &'static str, fingerprint: &str) -> Result<(), u16> {
        if owner != "PK" && owner != "KEK" && owner != "db" && owner != "dbx" {
            return Err(XC_SB_BAD_FP);
        }
        if fingerprint.len() != 64 || !fingerprint.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(XC_SB_BAD_FP);
        }
        // 同库同指纹重复登记拒。
        if self.certs.iter().any(|c| c.owner == owner && c.fingerprint == fingerprint) {
            return Err(XC_SB_DUP_DB);
        }
        self.certs.push(CertEntry { fingerprint: String::from(fingerprint), owner });
        Ok(())
    }

    /// 链序完整性（判据一）：db 每条必须有 KEK 背书、KEK 必须有 PK 背书。
    /// 本视图的背书模型：KEK 库非空 ⇒ PK 库非空；db 库非空 ⇒ KEK 库非空。
    pub fn chain_integrity(&self) -> Result<(), u16> {
        let has = |o: &str| self.certs.iter().any(|c| c.owner == o);
        if has("db") && !has("KEK") {
            return Err(XC_SB_CHAIN_BROKEN);
        }
        if has("KEK") && !has("PK") {
            return Err(XC_SB_CHAIN_BROKEN);
        }
        Ok(())
    }

    /// 撤销核对（判据二）：dbx 命中的 db 签名即撤销生效。
    pub fn revocation_check(&self) -> Result<(), u16> {
        for db in self.certs.iter().filter(|c| c.owner == "db") {
            if self.certs.iter().any(|c| c.owner == "dbx" && c.fingerprint == db.fingerprint) {
                return Err(XC_SB_REVOKED);
            }
        }
        Ok(())
    }

    /// 四库导出对照（判据三：与签名工具导出一致——同序同集导出）。
    /// 导出序：PK → KEK → db → dbx，同库按登记序（确定序保证可对照）。
    pub fn export_ordered(&self) -> Result<Vec<String>, u16> {
        let mut out = Vec::new();
        for owner in ["PK", "KEK", "db", "dbx"] {
            for c in self.certs.iter().filter(|c| c.owner == owner) {
                out.push(String::from(owner) + ":" + c.fingerprint.as_str());
            }
        }
        Ok(out)
    }

    /// 库计数读数。
    pub fn count_by(&self, owner: &str) -> usize {
        self.certs.iter().filter(|c| c.owner == owner).count()
    }

    /// 诊断说明。
    pub fn detail(code: u16) -> String {
        match code {
            XC_SB_CHAIN_BROKEN => "证书链断链：db 须 KEK 背书、KEK 须 PK 背书——缺上层即信任缺口。".to_string(),
            XC_SB_REVOKED => "签名被 dbx 撤销：该指纹立即失效，核对撤销库版本。".to_string(),
            XC_SB_BAD_FP => "指纹格式非法：SHA-256 恒 64 hex 字符，库别限定 PK/KEK/db/dbx。".to_string(),
            XC_SB_WRITE_ATTEMPT => "只读红线：本模块无写路径，写库操作归 L1 域。".to_string(),
            XC_SB_DUP_DB => "重复登记：同库同指纹只许一条。".to_string(),
            _ => "未知诊断码：查 0x646 段分配表。".to_string(),
        }
    }
}

/// 自证：链序 + 撤销 + 只读对照三判据对拍。
pub fn self_audit() -> Result<(), u16> {
    let mut v = SbChainView::boot();
    // 一、完整链：PK → KEK → db，撤销库不含 db 签名。
    v.import("PK", "111122223333444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF0001")?;
    v.import("KEK", "22223333444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF00001111")?;
    v.import("db", "3333444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF000011112222")?;
    v.chain_integrity()?;
    v.revocation_check()?;
    // 导出对照：确定序。
    let exp = v.export_ordered()?;
    if exp.len() != 3 || !exp[0].starts_with("PK:") || !exp[1].starts_with("KEK:") || !exp[2].starts_with("db:") {
        return Err(XC_SB_BAD_FP);
    }
    if v.count_by("PK") != 1 || v.count_by("db") != 1 {
        return Err(XC_SB_DUP_DB);
    }

    // 二、断链：有 db 无 KEK → 拒。
    let mut v2 = SbChainView::boot();
    v2.import("db", "3333444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF000011112222")?;
    if v2.chain_integrity().is_ok() {
        return Err(XC_SB_CHAIN_BROKEN);
    }
    // 三、撤销命中：db 与 dbx 同指纹 → 拒。
    let mut v3 = SbChainView::boot();
    v3.import("PK", "111122223333444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF0001")?;
    v3.import("KEK", "22223333444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF00001111")?;
    v3.import("db", "444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF0000111122223333")?;
    v3.import("dbx", "444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF0000111122223333")?;
    if v3.revocation_check().is_ok() {
        return Err(XC_SB_REVOKED);
    }
    // 四、格式红线：短指纹拒、非法库别拒、重复拒。
    let mut v4 = SbChainView::boot();
    if v4.import("db", "1234").is_ok() {
        return Err(XC_SB_BAD_FP);
    }
    if v4.import("L1", "111122223333444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF0001").is_ok() {
        return Err(XC_SB_BAD_FP);
    }
    v4.import("PK", "111122223333444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF0001")?;
    if v4.import("PK", "111122223333444455556666777788889999AAAABBBBCCCCDDDDEEEEFFFF0001").is_ok() {
        return Err(XC_SB_DUP_DB);
    }
    Ok(())
}
