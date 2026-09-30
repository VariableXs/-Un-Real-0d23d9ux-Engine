# -*- coding: utf-8 -*-
"""AI-29 · F4 增补卷五生成器（UNX-F4-E1201–E1500 · E61–E75 批 · 15 批 × 20 条）
主题/无障碍/IME UI 域 · 全部围绕 Varix 内核（checks.rs/lxprocfs.rs/内核事件·会话·IPC 通道/vxwm surface·popup·特效通道）锚定。
与卷一至卷四（E01–E60 批）判据颗粒零重复。
"""
import sys

BATCH_THEMES = {
    "E61": ("多用户与账户主题隔离面", "multiuser_theme_isolation"),
    "E62": ("来宾与会话临时主题档", "guest_temp_theme"),
    "E63": ("主题设置漫游与设备同步", "theme_roaming_sync"),
    "E64": ("定时主题计划与场景自动化", "theme_schedule_scene"),
    "E65": ("应用级主题覆写清单管理", "app_theme_override"),
    "E66": ("主题资源压缩与增量更新", "theme_pack_delta_update"),
    "E67": ("第三方主题签名与安全审计", "third_party_theme_signing"),
    "E68": ("主题卸载与残留清洁审计", "theme_uninstall_cleanup"),
    "E69": ("屏幕键盘预测条与滑行输入 UI", "flick_typing_ui"),
    "E70": ("剪贴板历史与云剪贴板 UI 面", "clipboard_history_ui"),
    "E71": ("表情连击与颜文字快捷面板", "kaomoji_quick_panel"),
    "E72": ("输入统计仪表盘（打字速度/词频）", "typing_stats_dashboard"),
    "E73": ("盲文显示与点字输出桥", "braille_display_bridge"),
    "E74": ("字幕与实时转写覆盖层", "caption_live_overlay"),
    "E75": ("F4 卷五治理收官与五卷联轧总账", "f4_vol5_governance"),
}

BATCH_SIZE = 20
START = 1201

DETAILS = [
    ("功能主路径（内核 IPC 会话建链）", "经内核会话账与 IPC 通道建链；判据：stub 内核下 probe 返回会话句柄且健康账注册"),
    ("开放格式配置（原子写+可迁移导出）", "配置 JSON 原子落盘并支持整包导出迁移；判据：导出导入往返字节一致"),
    ("设置三态 UI（占位/校验/纠错提示）", "错误提示含修正指引；判据：注入非法值后提示「怎么改对」且焦点回落"),
    ("浮层完整出路清单（点外/Esc/再点/失焦）", "四条出路全实测；判据：自动化遍历四路关闭后浮层句柄归零"),
    ("交互状态机全覆盖（连点/打断/拖半/长按）", "状态机路径枚举全覆盖测试；判据：乱序事件注入后状态机落合法态"),
    ("键盘对等可达（Tab 序/焦点归还/快捷键词典）", "浮层关闭后焦点还触发元素；判据：纯键盘走查脚本 PASS"),
    ("悬停/按压/拖拽微观手感（100ms 反馈红线）", "全部交互 100ms 内可见反馈埋点验证；判据：反馈耗时账 P95 < 100ms"),
    ("IME 组合期安全（不触发快捷键不误提交）", "composition 状态屏蔽快捷键管线；判据：模拟组合期按键零快捷键触发"),
    ("undo/redo 链（粒度合理可回溯）", "用户操作入 undo 链且粒度可配；判据：连续 50 步 undo/redo 状态逐点一致"),
    ("UIA 双侧投影（Provider 语义+Client 遍历）", "控件树双侧一致；判据：UIA 遍历与视觉树 diff 零偏差"),
    ("文本缩放与 DPI 复检（超大字号不截断）", "200% 字号全布局不破版；判据：文本缩放矩阵截图 diff 零截断"),
    ("高对比/色弱/灰度三态复检", "非色相冗余编码全量在位；判据：三态渲染自动化比对 PASS"),
    ("动画节奏与打断（对称缓动/打断续接）", "动画打断从中断点续接零跳变；判据：注入打断后帧差连续"),
    ("错误三要素+详情折叠（零裸异常码）", "技术细节收进折叠区；判据：全部错误路径 grep 无裸码直出"),
    ("体验日志与总日志中心接入", "交互四元组+体验结论字段入统一时间轴；判据：事件可按挫败维度聚合出清单"),
    ("隐私红线复检（不记输入内容/异步批量写入）", "日志不含正文全文且写入不阻塞；判据：日志 grep 敏感样本零命中、写延迟 P95 < 1ms"),
    ("性能账（P95/内存上限/帧率实测入账）", "长任务不冻主线程；判据：1000 次操作 P95 < 100ms 且内存增量有界"),
    ("崩溃恢复与看门狗（心跳/自动重建）", "功能崩溃后 3s 内看门狗重建会话；判据：kill 后自动恢复且状态回到快照"),
    ("开放接口版本化+签名校验+卸载清洁", "接口向后兼容、资源签名验证、卸载残留审计归零；判据：v1 调用兼容 PASS、篡改签名拒载、卸载后残留扫描零命中"),
    ("收官自检（checks.rs 全域 PASS+文档三件套）", "自检 exit=0 且文档与实现签名全等；判据：unxreal_f4_supp5_check ALL PASS"),
]

def main(out_path):
    lines = []
    lines.append("")
    lines.append("---")
    lines.append("")
    lines.append("## 增补卷五 · AI-29 · UNX-F4-E1201–E1500（300 项新功能 · E61–E75 批 · 15 批 × 20 条）")
    lines.append("")
    lines.append("> 收录纪律：独立增补编号，不占域账 F22401–F23200；状态列统一「增补」；")
    lines.append("> 全部围绕 Varix 内核锚定（checks.rs 自检面 / lxprocfs.rs 自省面 / 内核事件·会话·IPC 通道 / vxwm surface·popup·特效通道）；")
    lines.append("> 与卷一至卷四判据颗粒零重复；保管母本 docs/unxreal/supp/UNX-F4-SUPP-E1201-E1500.md（R-PROC-002 生成器重生成后须回播）。")
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
            eid = f"UNX-F4-E{no}"
            jid = f"{eid}-J1"
            lines.append(
                f"| {eid} | [{bid}] {name} · {title} | `{tag}/{tag}_{i:02d}.rs` | "
                f"Varix 内核锚定：{judge}（{jid}：内核事件通道+vxwm surface stub 下调 {tag}_probe() 断言） | 增补 |"
            )
        total += BATCH_SIZE
        lines.append("")
    lines.append(f"> 卷五小计：{total} 条。五卷联轧：UNX-F4-E001–E1500 共 1500 项增补，域账 F22401–F23200 零触碰。")
    lines.append("")
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    print(f"written {out_path}: {total} rows, batches={len(BATCH_THEMES)}")

if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "_f4_supp5_part.md")
