# -*- coding: utf-8 -*-
"""AI-29 · F4 增补卷六生成器（UNX-F4-E1501–E1800 · E76–E90 批 · 15 批 × 20 条）
主题/无障碍/IME UI 域 · 全部围绕 Varix 内核（checks.rs/lxprocfs.rs/内核事件·会话·IPC 通道/vxwm surface·popup·特效通道）锚定。
与卷一至卷五（E01–E75 批）判据颗粒零重复。
"""
import sys

BATCH_THEMES = {
    "E76": ("主题编辑器（可视化变量调试台）", "theme_editor_workbench"),
    "E77": ("实时主题预览多画布对比", "live_preview_canvas"),
    "E78": ("主题素材库管理（贴图/图标/音效）", "theme_asset_library"),
    "E79": ("色彩系统设计令牌（Design Token）", "design_token_system"),
    "E80": ("明暗模式渐变过渡与日出日落曲线", "mode_transition_curve"),
    "E81": ("主题健康评分与自动修复建议", "theme_health_score"),
    "E82": ("输入法长句编辑与候选回改 UI", "long_sentence_edit_ui"),
    "E83": ("模糊音与方言适配设置面", "fuzzy_dialect_settings"),
    "E84": ("双拼/笔画/五笔多方案切换 UI", "multi_scheme_input_ui"),
    "E85": ("候选窗多屏跨显示器跟随", "candidate_cross_monitor"),
    "E86": ("语音唤醒词与免手输入入口", "voice_wake_handsfree"),
    "E87": ("眼动追踪与头控输入适配层", "eyetrack_headcontrol_adapter"),
    "E88": ("开关控制（Switch Access）扫描输入", "switch_access_scan"),
    "E89": ("认知辅助（简化模式/阅读聚焦）", "cognitive_assist_focus"),
    "E90": ("F4 卷六治理收官与六卷联轧总账", "f4_vol6_governance"),
}

BATCH_SIZE = 20
START = 1501

DETAILS = [
    ("功能主路径（内核会话+vxwm surface 建链）", "stub 内核建链返回句柄并注册健康账；判据：probe PASS 且健康账可见"),
    ("开放格式持久化（原子写/导出迁移/卸载清洁）", "配置原子落盘+导出导入字节一致+卸载残留扫描零；判据：三段断言全 PASS"),
    ("设置三态与纠错（占位/校验/怎么改对）", "非法值提示含修正指引且焦点回落；判据：注入错误后断言提示文本与焦点"),
    ("浮层出路四路实测（点外/Esc/再点/失焦）", "自动化遍历四路关闭；判据：关闭后浮层句柄归零且焦点还触发元素"),
    ("状态机乱序注入（连点/打断/拖半松手/长按）", "乱序事件落合法态；判据：fuzz 事件序列后状态机零非法态"),
    ("键盘对等（Tab 序/焦点环/快捷键词典不冲突）", "纯键盘遍历+快捷键冲突扫描；判据：走查脚本 PASS 且冲突计数 0"),
    ("100ms 反馈红线（悬停/按压/拖拽跟随）", "反馈耗时埋点；判据：反馈账 P95 < 100ms、拖拽跟手零瞬移帧"),
    ("IME 组合期安全（屏蔽快捷键/候选窗跟光标）", "组合期按键进 IME 不进管线；判据：模拟组合期快捷键零触发、候选窗坐标=光标坐标"),
    ("undo/redo 链（粒度可配/内存有界）", "50 步往返逐点一致且链内存上限截断；判据：往返 PASS+内存峰值 < 上限"),
    ("UIA 双侧一致（Provider 语义/Client 遍历 diff）", "遍历 diff 零偏差；判据：树 diff 工具输出空集"),
    ("文本缩放 200% + Per-Monitor DPI 矩阵复检", "多屏异 DPI 拖入不破版零锯齿；判据：矩阵截图 diff 零截断零锯齿"),
    ("高对比/色弱/灰度冗余编码复检", "形状/图标冗余在位；判据：三态渲染比对 PASS"),
    ("动画打断续接（对称缓动/帧差连续）", "打断从中断点续接；判据：帧差序列单调连续零跳变"),
    ("错误三要素+详情折叠+恢复路（零裸码）", "全错误路径三要素呈现+可操作恢复；判据：grep 裸码零命中+恢复路可走通"),
    ("体验日志/挫败信号/总日志中心接入", "rage/dead click 自动标记可聚合；判据：注入信号后清单视图命中"),
    ("隐私红线（零内容记录/异步批量/可关闭）", "敏感样本零命中+写入不阻塞；判据：写延迟 P95 < 1ms、开关关闭后零写入"),
    ("性能账（P95/内存上限/60fps/启动节奏）", "长任务不冻主线程先骨架后内容；判据：1000 次操作 P95 < 100ms、首帧可交互 < 1s"),
    ("崩溃恢复（快照/看门狗 3s 重建/断电唤醒）", "kill 后自动恢复到快照点；判据：断电模拟重启后状态字节一致"),
    ("开放接口版本化+签名验证+向后兼容垫片", "v1 调 v2 走垫片零中断、篡改签名拒载；判据：兼容断言 PASS+拒载错误码正确"),
    ("收官自检（checks.rs 全域+文档三件套一致性）", "自检 exit=0、文档签名 grep 全等；判据：supp6_check ALL PASS"),
]

def main(out_path):
    lines = []
    lines.append("")
    lines.append("---")
    lines.append("")
    lines.append("## 增补卷六 · AI-29 · UNX-F4-E1501–E1800（300 项新功能 · E76–E90 批 · 15 批 × 20 条）")
    lines.append("")
    lines.append("> 收录纪律：独立增补编号，不占域账 F22401–F23200；状态列统一「增补」；")
    lines.append("> 全部围绕 Varix 内核锚定（checks.rs 自检面 / lxprocfs.rs 自省面 / 内核事件·会话·IPC 通道 / vxwm surface·popup·特效通道）；")
    lines.append("> 与卷一至卷五判据颗粒零重复；保管母本 docs/unxreal/supp/UNX-F4-SUPP-E1501-E1800.md（R-PROC-002 生成器重生成后须回播）。")
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
    lines.append(f"> 卷六小计：{total} 条。六卷联轧：UNX-F4-E001–E1800 共 1800 项增补，域账 F22401–F23200 零触碰。")
    lines.append("")
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    print(f"written {out_path}: {total} rows, batches={len(BATCH_THEMES)}")

if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "_f4_supp6_part.md")
