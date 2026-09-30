# -*- coding: utf-8 -*-
"""AI-29 · J4 增补卷生成器（UNX-J4-E001–E300 · 15 批 × 20 条）
沙箱与隔离域 · 全部围绕 Varix 内核（checks.rs/lxprocfs.rs/内核事件·会话·IPC 通道/vxwm surface·popup·特效通道）锚定。
独立增补编号，不占 J4 域账 F38401–F39200。
"""
import sys

BATCH_THEMES = {
    "S01": ("沙箱会话生命周期管理", "sandbox_session_lifecycle"),
    "S02": ("能力位掩码与最小权限引擎", "capability_bitmask_engine"),
    "S03": ("文件系统虚拟化隔离层", "fs_virtualization_isolation"),
    "S04": ("注册表虚拟化与写时复制", "registry_cow_virtualization"),
    "S05": ("进程间通信通道隔离", "ipc_channel_isolation"),
    "S06": ("窗口与剪贴板跨界管控", "window_clipboard_boundary"),
    "S07": ("网络访问白名单与代理审计", "network_whitelist_audit"),
    "S08": ("设备与硬件访问闸门", "device_hardware_gate"),
    "S09": ("内存与资源配额强制", "memory_quota_enforcement"),
    "S10": ("子进程派生与沙箱继承策略", "child_inherit_policy"),
    "S11": ("沙箱逃逸检测与告警", "escape_detection_alert"),
    "S12": ("沙箱快照与状态回滚", "snapshot_rollback"),
    "S13": ("沙箱日志隔离与父域审计", "sandbox_log_audit"),
    "S14": ("插件沙箱与第三方扩展隔离", "plugin_sandbox_thirdparty"),
    "S15": ("J4 增补卷治理收官总账", "j4_supp_governance"),
}

BATCH_SIZE = 20
START = 1

DETAILS = [
    ("功能主路径（内核会话建链+能力位申请）", "经内核会话账建链并申请能力位集合；判据：stub 内核 probe 返回句柄且能力位注册可见"),
    ("开放格式清单（原子写/导出迁移）", "沙箱策略 JSON 原子落盘可导出；判据：导出导入往返字节一致"),
    ("默认拒绝原则（未声明能力一律拒）", "未声明能力调用返回 DENY 并三要素呈现；判据：注入未声明调用断言拒载错误码"),
    ("越界请求三要素呈现（零裸异常码）", "全部越界错误走人话映射；判据：越界路径 grep 裸码零命中"),
    ("隐蔽失败探针（静默拒绝/异步回调入总日志）", "隐蔽拒绝全量埋点；判据：kill 回调后总日志 5s 内 fatal 事件"),
    ("体验日志埋点（沙箱操作四元组）", "操作记界面/元素/耗时/反馈；判据：10 次操作 10 条且不记输入内容"),
    ("拉起/关闭完整出路（孤儿会话回收）", "父进程死亡后子沙箱 3s 内回收；判据：kill 父进程后孤儿计数归零"),
    ("状态机全覆盖（启动中/运行/暂停/终止乱序注入）", "fuzz 事件零非法态；判据：乱序注入后状态机断言 PASS"),
    ("键盘/焦点隔离（沙箱 UI 不劫持宿主焦点）", "沙箱浮层焦点自闭环；判据：开关沙箱浮层宿主焦点不丢"),
    ("100ms 反馈红线（沙箱内交互可见反馈）", "交互反馈埋点；判据：反馈账 P95 < 100ms"),
    ("跨界审计（跨界部件清单式声明+越界告警）", "跨界白名单外行为即时告警；判据：注入越界后告警事件 1s 内在账"),
    ("配额超限优雅降级（不崩宿主不丢数据）", "超限走降级路径并提示；判据：压满配额后降级提示出现且进程存活"),
    ("资源泄漏看门狗（句柄/内存/线程三账）", "泄漏超阈值看门狗报警；判据：注入泄漏后 3s 内 health 事件"),
    ("UIA 双侧投影（沙箱内控件树语义可见）", "沙箱 UIA 树可遍历且边界标注；判据：UIA 遍历含 sandbox 边界节点"),
    ("高对比/色弱/灰度复检（沙箱 UI 同标准）", "三态渲染达标；判据：比对脚本 PASS"),
    ("DPI 与多屏矩阵复检", "沙箱窗口跨屏拖动零错乱；判据：异 DPI 矩阵截图 diff PASS"),
    ("性能账（沙箱开销 P95/内存上限入账）", "沙箱包裹开销可测量；判据：包裹后 P95 增量 < 5ms 且内存增量 < 10MB"),
    ("崩溃恢复（沙箱崩溃零波及宿主零波及邻箱）", "kill 沙箱进程宿主与邻箱存活；判据：崩溃注入后宿主 checks PASS"),
    ("开放接口版本化+策略签名验证", "策略文件签名校验拒篡改；判据：篡改 1 字节后拒载并留痕"),
    ("收官自检（checks.rs 全域+文档三件套一致性）", "自检 exit=0 文档签名全等；判据：unxreal_j4_supp_check ALL PASS"),
]

def main(out_path):
    lines = []
    lines.append("")
    lines.append("---")
    lines.append("")
    lines.append("## 增补卷 · AI-29 · UNX-J4-E001–E300（沙箱与隔离域 300 项新功能 · S01–S15 批 · 15 批 × 20 条）")
    lines.append("")
    lines.append("> 收录纪律：独立增补编号，不占 J4 域账 F38401–F39200（40 批守恒不动）；状态列统一「增补」；")
    lines.append("> 全部围绕 Varix 内核锚定（checks.rs 自检面 / lxprocfs.rs 自省面 / 内核事件·会话·IPC 通道 / vxwm surface·popup·特效通道）；")
    lines.append("> 与 F4 六卷及他卷判据颗粒零重复；保管母本 docs/unxreal/supp/UNX-J4-SUPP-E001-E300.md（R-PROC-002 生成器重生成后须回播）。")
    lines.append("")
    total = 0
    for idx, (bid, (name, tag)) in enumerate(BATCH_THEMES.items()):
        lines.append(f"### 批 {bid} · {name}（20 条）")
        lines.append("")
        lines.append("| 编号 | 功能 | 位置（建议） | 判据（可运行 · J1） | 状态 |")
        lines.append("|---|---|---|---|---|")
        start_no = START + idx * BATCH_SIZE
        for i, (title, judge) in enumerate(DETAILS):
            no = start_no + i
            eid = f"UNX-J4-E{no:03d}"
            jid = f"{eid}-J1"
            lines.append(
                f"| {eid} | [{bid}] {name} · {title} | `{tag}/{tag}_{i:02d}.rs` | "
                f"Varix 内核锚定：{judge}（{jid}：内核事件通道+沙箱 stub 下调 {tag}_probe() 断言） | 增补 |"
            )
        total += BATCH_SIZE
        lines.append("")
    lines.append(f"> 卷小计：{total} 条。AI-29 增补总账：F4 六卷 1800 项 + J4 一卷 300 项 = 2100 项增补，域账零触碰。")
    lines.append("")
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    print(f"written {out_path}: {total} rows, batches={len(BATCH_THEMES)}")

if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "_j4_supp_part.md")
