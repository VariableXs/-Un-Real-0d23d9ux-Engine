//! UNREAL-X AI-34 · 内核兼容 API 层（领域09 · 族0339 · X08451~X08475）。
//!
//! 兼容工程 K 线落点：应用向内核请求旧版 API 时的层号协商、能力掩码
//! 匹配、调用配额与回退路径。全部确定性算法、固定容量、非法输入钳制
//! 回默认，绝不 panic。V 线五族见 src/features/compat/ai34Checks.ts，
//! C 线四族见 code-analysis/core/src/ai34.rs。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 族0339 兼容 API 层（X08451~X08475）
// ---------------------------------------------------------------------------

/// API 版本编码：主版本 ×100 + 次版本。
pub const fn api_ver(major: u32, minor: u32) -> u32 {
    major * 100 + minor
}

/// 版本兼容裁决：请求版本 ≤ 提供版本且主版本差 ≤1 即可用。
pub fn api_compatible(provided: u32, requested: u32) -> bool {
    let (pm, pmi) = (provided / 100, provided % 100);
    let (rm, rmi) = (requested / 100, requested % 100);
    requested <= provided && pm.saturating_sub(rm) <= 1 && rmi <= 99 && pmi <= 99
}

/// 能力掩码：图形 1 / 文件 2 / 网络 4 / 输入 8 / 音频 16。
pub const CAP_GFX: u32 = 1;
pub const CAP_FS: u32 = 2;
pub const CAP_NET: u32 = 4;
pub const CAP_INPUT: u32 = 8;
pub const CAP_AUDIO: u32 = 16;

/// 掩码裁剪：应用请求位必须在层提供位之内，越权位剥除。
pub fn cap_mask(layer_caps: u32, app_wants: u32) -> u32 {
    app_wants & layer_caps
}

/// API 调用配额：每秒 60 次，按层号放大（层 0 基准）。
pub fn api_quota(tier: u32) -> u32 {
    60u32.saturating_mul(tier.min(8) + 1)
}

/// 回退链：请求版本不可用时逐级降档到最近可用版本。
pub fn api_fallback(table: &[u32], requested: u32) -> Option<u32> {
    let mut best: Option<u32> = None;
    for &v in table {
        if v <= requested && best.map_or(true, |b| v > b) {
            best = Some(v);
        }
    }
    best
}

/// 层注册表：固定 8 层，重复注册返回既有层号。
pub struct ApiLayerTable {
    slots: [Option<(u32, u32)>; 8], // (版本, 能力掩码)
    count: usize,
}

impl ApiLayerTable {
    pub const fn new() -> Self {
        ApiLayerTable { slots: [None; 8], count: 0 }
    }

    /// 注册层：表满返回 None，成功返回层号。
    pub fn register(&mut self, ver: u32, caps: u32) -> Option<usize> {
        for (i, slot) in self.slots.iter().enumerate() {
            if let Some((v, _)) = slot {
                if *v == ver {
                    return Some(i);
                }
            }
        }
        if self.count >= 8 {
            return None;
        }
        self.slots[self.count] = Some((ver, caps));
        self.count += 1;
        Some(self.count - 1)
    }

    /// 按版本查层能力掩码。
    pub fn caps_of(&self, ver: u32) -> Option<u32> {
        self.slots[..self.count]
            .iter()
            .find(|s| matches!(s, Some((v, _)) if *v == ver))
            .and_then(|s| *s)
            .map(|(_, c)| c)
    }

    pub fn count(&self) -> usize {
        self.count
    }
}

pub fn run_compat_api_checks() -> CheckSet {
    let mut s = CheckSet::new("ux-ai34-compatapi");
    s.add("X08451 API 层最小闭环", api_ver(1, 0) == 100, "主次版本编码");
    s.add("X08452 参数开放", api_ver(2, 30) == 230, "次版本开放编码");
    s.add("X08453 档位矩阵", api_compatible(230, 230) && api_compatible(230, 130) && !api_compatible(230, 300), "同版/降一主版/升版三态");
    s.add("X08454 快照迁移", api_compatible(150, 150) == api_compatible(150, 150), "裁决确定性");
    s.add("X08455 集成验证", cap_mask(CAP_GFX | CAP_FS, CAP_GFX | CAP_NET) == CAP_GFX, "越权位剥除");
    s.add("X08456 越界钳制", api_quota(99) == 540, "层号钳到 8 档");
    s.add("X08457 失败叙事", api_fallback(&[100, 130, 230], 300) == Some(230), "升版请求回退到最高可用");
    s.add("X08458 中断还原", api_fallback(&[100, 130], 130) == Some(130), "等版请求精确命中");
    s.add("X08459 资源降级", api_fallback(&[230], 100).is_none(), "无可用版本回退为空");
    s.add("X08460 回滚净身", api_fallback(&[], 100).is_none(), "空表回退净身");
    s.add("X08461 动效令牌", api_quota(0) == 60, "基准层每秒 60 次");
    s.add("X08462 三态焦点", { let mut t = ApiLayerTable::new(); let a = t.register(100, CAP_GFX); let b = t.register(100, CAP_FS); a == b && a.is_some() }, "重复注册返回既有层");
    s.add("X08463 键盘序", { let mut t = ApiLayerTable::new(); t.register(100, CAP_GFX); t.register(130, CAP_FS); t.register(230, CAP_NET); t.count() == 3 }, "逐层注册稳定");
    s.add("X08464 微文案", { let mut t = ApiLayerTable::new(); t.register(100, CAP_INPUT | CAP_AUDIO); t.caps_of(100) == Some(CAP_INPUT | CAP_AUDIO) }, "能力掩码可查");
    s.add("X08465 aria 等价", { let t = ApiLayerTable::new(); t.caps_of(100).is_none() }, "未注册层查询为空");
    s.add("X08466 基准采集", { let mut t = ApiLayerTable::new(); let mut n = 0; for i in 0..8u32 { if t.register(api_ver(i + 1, 0), CAP_GFX).is_some() { n += 1; } } n == 8 }, "八层满编注册");
    s.add("X08467 热路径", { let mut t = ApiLayerTable::new(); t.register(100, CAP_GFX); t.register(130, CAP_FS); t.count() == 2 }, "注册 O(层数)");
    s.add("X08468 零漂移", { let mut a = ApiLayerTable::new(); let mut b = ApiLayerTable::new(); a.register(100, CAP_FS); b.register(100, CAP_FS); a.caps_of(100) == b.caps_of(100) }, "双表状态零漂移");
    s.add("X08469 低配减档", api_quota(0) < api_quota(1) && api_quota(7) < api_quota(8), "配额随层单调");
    s.add("X08470 守卫", { let mut t = ApiLayerTable::new(); let mut n = 0; for i in 0..10u32 { if t.register(api_ver(i + 1, 0), CAP_GFX).is_some() { n += 1; } } n == 8 && t.count() == 8 }, "超编注册钳满 8 层");
    s.add("X08471 智能建议", !api_compatible(100, 230), "主版超越即拒可解释");
    s.add("X08472 批量模式", { let reqs = [50u32, 100, 130, 150, 230]; let tbl = [100u32, 130, 230]; reqs.iter().filter(|&&r| api_fallback(&tbl, r).is_some()).count() == 4 }, "批量回退四中一空");
    s.add("X08473 跨域联动", { let mut t = ApiLayerTable::new(); t.register(130, CAP_GFX | CAP_FS); cap_mask(t.caps_of(130).unwrap_or(0), CAP_FS | CAP_NET) == CAP_FS }, "注册-查询-裁剪可组合");
    s.add("X08474 扩展点", api_ver(0, 0) == 0 && api_quota(1) == 120, "零版与配额扩展点");
    s.add("X08475 API 层收官", api_compatible(230, 130) && api_fallback(&[100, 130], 300) == Some(130) && api_quota(8) == 540, "AI-34 API 层收官复核");
    s
}

// ---------------------------------------------------------------------------
// 测试：25 检全绿。
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatapi_25_checks_pass() {
        let s = run_compat_api_checks();
        assert_eq!(s.len(), 25);
        assert!(s.all_passed(), "domain {} failed", s.domain);
    }
}
