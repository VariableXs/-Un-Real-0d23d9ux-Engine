//! VE-F4349 · 一致性 API 参考（U08 测试与资产组）
//! 职责：一致性对外 API 参考文档——API 三族（U02 契约 API / U03 验证 API
//! / U08 工具 API）+ 文档规范（每端点四节：用途/参数/错误码/示例——缺节
//! 断言拦截发布）+ 可试调用（失真环境标注·不含真实密钥）+ 版本对齐（文档
//! 版本与 API 语义版本绑定发布——失配阻断）+ 读屏标准（域本色）。
//! 段位：0x41E0（续表 idx583）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x41E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApCode {
    /// 四节缺节（断言拦截发布）。
    SectionMissing = SEG_BASE,
    /// 版本失配（阻断发布）。
    VersionMismatch = SEG_BASE + 0x01,
    /// 试调用失真（环境标注）。
    TrialDistort = SEG_BASE + 0x02,
    /// 族号越界。
    FamilyRange = SEG_BASE + 0x03,
}

/// API 三族。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ApiFamily {
    /// U02 契约 API。
    Contract,
    /// U03 验证 API。
    Verify,
    /// U08 工具 API。
    Tool,
}

/// 端点文档卡。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Endpoint {
    pub family: ApiFamily,
    pub id: u32,
    /// 四节在位位（bit0 用途 / bit1 参数 / bit2 错误码 / bit3 示例）。
    pub sections: u8,
    /// 读屏标准（域色）。
    pub sr_ok: bool,
    pub api_ver: u32,
    pub doc_ver: u32,
}

/// API 参考台。
#[derive(Default)]
pub struct ApiDocs {
    pub endpoints: Vec<Endpoint>,
    /// 试调用记录：(端点, 结果)——None = 失真。
    pub trials: Vec<(u32, Option<&'static str>)>,
    pub blocks: u32,
}

/// 四节满位掩码。
pub const SECTIONS_FULL: u8 = 0x0F;

impl ApiDocs {
    pub fn new() -> Self {
        ApiDocs { ..Default::default() }
    }

    /// 端点登记（族校验在类型层；幂等覆盖）。
    pub fn register(&mut self, e: Endpoint) {
        if let Some(x) = self.endpoints.iter_mut().find(|x| x.id == e.id) {
            *x = e;
        } else {
            self.endpoints.push(e);
        }
    }

    /// 四节断言：缺节端点清单（发布拦截对象，计数留痕）。
    pub fn assert_sections(&mut self) -> Vec<u32> {
        let mut bad = Vec::new();
        for e in self.endpoints.iter() {
            if e.sections & SECTIONS_FULL != SECTIONS_FULL {
                bad.push(e.id);
                self.blocks += 1;
            }
        }
        bad
    }

    /// 版本对齐断言：文档版本 = API 语义版本才可发布（失配阻断）。
    pub fn version_gate(&self) -> Result<(), ApCode> {
        if self.endpoints.iter().any(|e| e.doc_ver != e.api_ver) {
            return Err(ApCode::VersionMismatch);
        }
        Ok(())
    }

    /// 试调用登记：真实密钥拒收（试调用不含真实密钥——安全前置）。
    pub fn trial(&mut self, ep: u32, with_real_key: bool, result: Option<&'static str>) -> Result<(), ApCode> {
        if with_real_key {
            return Err(ApCode::TrialDistort);
        }
        let e = self.endpoints.iter().find(|e| e.id == ep).ok_or(ApCode::TrialDistort)?;
        if result.is_none() {
            // 失真环境标注（返回 Ok 但记 None——诚实标注）。
            self.trials.push((ep, None));
            return Ok(());
        }
        let _ = e;
        self.trials.push((ep, result));
        Ok(())
    }

    /// 试调用失真率（None 占比）。
    pub fn distort_bp(&self) -> u32 {
        if self.trials.is_empty() {
            return 0;
        }
        let bad = self.trials.iter().filter(|t| t.1.is_none()).count();
        (bad as u64 * 10000 / self.trials.len() as u64) as u32
    }

    /// 全册读屏标准断言（域本色）。
    pub fn sr_clean(&self) -> bool {
        self.endpoints.iter().all(|e| e.sr_ok)
    }

    /// 发布就绪：四节全齐 + 版本对齐 + 读屏洁。
    pub fn ready(&mut self) -> bool {
        self.assert_sections().is_empty() && self.version_gate().is_ok() && self.sr_clean()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves583_checks() -> CheckSet {
    let mut set = CheckSet::new("ves583");
    let mut ad = ApiDocs::new();

    // C583-01 三族端点登记（四节全齐 + 版本对齐）。
    let fams = [ApiFamily::Contract, ApiFamily::Verify, ApiFamily::Tool];
    for (i, f) in fams.iter().enumerate() {
        ad.register(Endpoint { family: *f, id: i as u32 + 1, sections: 0x0F, sr_ok: true, api_ver: 7, doc_ver: 7 });
    }
    chk(&mut set, "C583-01", ad.endpoints.len() == 3, "三族端点齐备");

    // C583-02 四节缺节断言拦截（计数）。
    ad.register(Endpoint { family: ApiFamily::Contract, id: 4, sections: 0x07, sr_ok: true, api_ver: 7, doc_ver: 7 });
    let bad = ad.assert_sections();
    chk(&mut set, "C583-02", bad == [4] && ad.blocks == 1, "缺节断言拦截");

    // C583-03 修复后断言清零。
    ad.register(Endpoint { family: ApiFamily::Contract, id: 4, sections: 0x0F, sr_ok: true, api_ver: 7, doc_ver: 7 });
    chk(&mut set, "C583-03", ad.assert_sections().is_empty(), "修复断言清零");

    // C583-04 版本失配阻断发布。
    ad.register(Endpoint { family: ApiFamily::Tool, id: 5, sections: 0x0F, sr_ok: true, api_ver: 8, doc_ver: 7 });
    chk(&mut set, "C583-04", ad.version_gate() == Err(ApCode::VersionMismatch), "版本失配阻断");

    // C583-05 版本对齐后放行。
    ad.register(Endpoint { family: ApiFamily::Tool, id: 5, sections: 0x0F, sr_ok: true, api_ver: 8, doc_ver: 8 });
    chk(&mut set, "C583-05", ad.version_gate().is_ok(), "版本对齐放行");

    // C583-06 试调用成功登记。
    chk(&mut set, "C583-06", ad.trial(1, false, Some("200 OK")).is_ok() && ad.trials.len() == 1, "试调用成功");

    // C583-07 真实密钥拒收（安全前置）。
    chk(&mut set, "C583-07", ad.trial(1, true, Some("200 OK")) == Err(ApCode::TrialDistort), "真实密钥拒");

    // C583-08 失真环境标注（None 诚实入账）。
    let _ = ad.trial(2, false, None);
    chk(&mut set, "C583-08", ad.distort_bp() == 5000, "失真标注 50%");

    // C583-09 未知端点试调用拒。
    chk(&mut set, "C583-09", ad.trial(99, false, Some("x")).is_err(), "未知端点拒");

    // C583-10 读屏标准断言（域色）。
    ad.register(Endpoint { family: ApiFamily::Verify, id: 6, sections: 0x0F, sr_ok: false, api_ver: 7, doc_ver: 7 });
    chk(&mut set, "C583-10", !ad.sr_clean(), "读屏缺检出");

    // C583-11 修复后全册洁。
    ad.register(Endpoint { family: ApiFamily::Verify, id: 6, sections: 0x0F, sr_ok: true, api_ver: 7, doc_ver: 7 });
    chk(&mut set, "C583-11", ad.sr_clean(), "全册读屏洁");

    // C583-12 发布就绪总闸。
    chk(&mut set, "C583-12", ad.ready(), "发布就绪");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn section_assert() {
        let mut ad = ApiDocs::new();
        ad.register(Endpoint { family: ApiFamily::Contract, id: 1, sections: 0x0F, sr_ok: true, api_ver: 1, doc_ver: 1 });
        ad.register(Endpoint { family: ApiFamily::Verify, id: 2, sections: 0x03, sr_ok: true, api_ver: 1, doc_ver: 1 });
        assert_eq!(ad.assert_sections(), [2]);
        assert_eq!(ad.assert_sections(), [2]); // 未修复复扫仍拦
    }

    #[test]
    fn real_key_rejected() {
        let mut ad = ApiDocs::new();
        ad.register(Endpoint { family: ApiFamily::Tool, id: 1, sections: 0x0F, sr_ok: true, api_ver: 1, doc_ver: 1 });
        assert_eq!(ad.trial(1, true, Some("ok")), Err(ApCode::TrialDistort));
        assert!(ad.trials.is_empty());
    }
}
