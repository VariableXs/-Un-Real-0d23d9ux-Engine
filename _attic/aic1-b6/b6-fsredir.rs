
// ---------------------------------------------------------------------------
// F010 · 深化批次六：沙盒配额告警面（应用沙盒占用的诚实显示）
//
// 主册依据（G-A-10【交互设计】）：「资源管理器显示沙盒目录为普通文件夹……
// 属性页标注『属于 XX 应用的隔离数据』」+【数据安全与信任】「占用了什么资源
// 用户能看见」——沙盒字节占用计数 + 阈值告警（可见可控行为的数据源）。
// ---------------------------------------------------------------------------

/// 沙盒配额记账（per-app 字节计数 + 阈值告警）。
#[derive(Clone, Copy, Debug)]
pub struct SandboxQuota {
    pub used_bytes: u64,
    /// 告警阈值（默认 64MB——正常应用沙盒远低于此；超限必有异常写入）。
    pub warn_bytes: u64,
    pub warned: bool,
}

impl SandboxQuota {
    pub const fn new() -> SandboxQuota {
        SandboxQuota { used_bytes: 0, warn_bytes: 64 * 1024 * 1024, warned: false }
    }

    /// 记一笔写入（字节累计；跨阈值即告警——只告警一次直到回落）。
    pub fn charge(&mut self, bytes: u64) -> bool {
        self.used_bytes = self.used_bytes.saturating_add(bytes);
        if self.used_bytes >= self.warn_bytes && !self.warned {
            self.warned = true;
            return true;
        }
        false
    }

    /// 回落重置告警臂（清理后允许再次告警——状态不粘连）。
    pub fn release(&mut self, bytes: u64) {
        self.used_bytes = self.used_bytes.saturating_sub(bytes);
        if self.used_bytes < self.warn_bytes {
            self.warned = false;
        }
    }
}

/// F010 深化批次六自检。
pub fn run_fsredir_deep5_checks() -> CheckSet {
    let mut cs = CheckSet::new("F010-fsredir-deep5");
    // 1) 阈值告警：63MB 不告警；跨 64MB 触发一次；继续写不重复告警。
    let mut q = SandboxQuota::new();
    let w1 = q.charge(63 * 1024 * 1024);
    let w2 = q.charge(2 * 1024 * 1024);
    let w3 = q.charge(1024);
    cs.add(
        "sandbox_quota_warn_once",
        !w1 && w2 && !w3 && q.warned,
        "",
    );
    // 2) 回落重置：清理后告警臂复位（再次超限仍会告警——不粘连）。
    q.release(3 * 1024 * 1024);
    let clean = !q.warned;
    let w4 = q.charge(2 * 1024 * 1024);
    cs.add("sandbox_quota_arm_reset", clean && w4, "");
    // 3) 阈值钉值锚（64MB——正常应用设置 <1MB 的告警线语义）。
    cs.add("sandbox_quota_threshold_pinned", SandboxQuota::new().warn_bytes == 67_108_864, "");
    cs
}
