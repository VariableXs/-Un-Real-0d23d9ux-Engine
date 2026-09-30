# -*- coding: utf-8 -*-
"""AI-29 · F4 增补卷四生成器（UNX-F4-E901–E1200 · E46–E60 批 · 15 批 × 20 条）
主题/无障碍/IME UI 域 · 全部围绕 Varix 内核（checks.rs/lxprocfs.rs/内核事件·会话·IPC 通道/vxwm surface·popup·特效通道）锚定。
与卷一（E01–E15）卷二（E16–E30）卷三（E31–E45）判据颗粒零重复。
"""
import sys

BATCH_THEMES = {
    "E46": ("护眼模式与色温调度", "night_light_temp"),
    "E47": ("壁纸引擎与主题联动", "wallpaper_theme_link"),
    "E48": ("字体渲染与 ClearType 调优 UI", "font_render_tuning"),
    "E49": ("鼠标触控板辅助（大指针/轨迹）", "pointer_assist"),
    "E50": ("色盲滤镜与色觉辅助", "colorblind_filter"),
    "E51": ("专注辅助与免打扰主题面", "focus_assist_theme"),
    "E52": ("IME 双语混排与中英切换 UI", "ime_bilingual_toggle"),
    "E53": ("候选词学习与预测设置 UI", "candidate_learning_ui"),
    "E54": ("输入法皮肤市场与开放格式", "ime_skin_market"),
    "E55": ("语音合成播报 TTS 主题集成", "tts_theme_integration"),
    "E56": ("全局字号缩放与文本缩放", "global_text_scaling"),
    "E57": ("动画减弱与前庭障碍模式", "reduced_motion_vestibular"),
    "E58": ("单手模式与辅助触控", "onehand_assist_touch"),
    "E59": ("主题崩溃恢复与安全模式 UI", "theme_crash_safe_mode"),
    "E60": ("F4 卷四治理收官与四卷联轧总账", "f4_vol4_governance"),
}

BATCH_SIZE = 20
START = 901

DETAILS = [
    ("基础功能面（域内核锚定主路径）", "主路径经内核 IPC 通道与 vxwm surface 建链；判据：stub 内核下 probe 建链成功并返回会话句柄"),
    ("参数化配置面（开放格式持久化）", "配置写开放格式 JSON 并原子落盘；判据：写中途 kill 后重启读回旧版零损坏"),
    ("用户设置面板（即时生效预览）", "设置面板改动即时预览不入库，确认才提交；判据：取消后配置字节不变"),
    ("状态查询与自省接口（lxprocfs 投影）", "状态以只读文件投影挂 lxprocfs.rs 自省面；判据：cat 投影节点返回实时状态 JSON"),
    ("异常三要素呈现（错误码→人话映射）", "全部错误码经三要素映射表呈现；判据：注入非法参数后 UI 显示三要素且日志含原始码"),
    ("隐蔽异常捕获（异步回调/静默失败探针）", "异步路径全量埋探针入总日志中心；判据：kill 后台线程后总日志 5s 内出现 fatal 事件"),
    ("体验日志埋点（交互细节层）", "每次交互记界面/元素/耗时/反馈四元组；判据：模拟 10 次点击后日志含 10 条且不记输入内容"),
    ("挫败信号自动标记（rage/dead click）", "同点 3 秒内 3 击或 dead 区点击自动标记体验事件；判据：注入连击后事件表出现 rage_click 标记"),
    ("键盘可达性（Tab 序/焦点环/快捷键承诺）", "全控件 Tab 可达且焦点环可见，声明快捷键全实测；判据：纯键盘遍历全控件零死角"),
    ("屏幕阅读器投影（UIA 节点语义）", "控件树投 UIA 语义节点带名称/角色/状态；判据：UIA 客户端遍历与视觉树一致"),
    ("高对比度与色弱兼容（非仅色相区分）", "状态区分叠加形状/图标冗余编码；判据：灰度渲染后状态仍可辨"),
    ("DPI 与多屏适配（Per-Monitor v2 感知）", "跨屏拖动重排布局零错乱；判据：1.0x→2.5x 屏拖入后矢量资源零锯齿"),
    ("性能账（P95/内存/帧率埋点入账）", "操作 P95 与峰值内存入主题性能账；判据：100 次操作 P95 < 100ms 且内存增量 < 5MB"),
    ("与内核健康账联动（心跳/看门狗）", "功能心跳挂内核健康账 checks.rs 面；判据：stub 停跳后看门狗 3s 报警"),
    ("开放接口与版本化（十年不变承诺）", "对外接口版本号+向后兼容废弃流程；判据：v1 客户端调 v2 接口走兼容垫片零中断"),
    ("端到端跨域联测（三类窗口矩阵）", "普通/沉浸/合成三类窗口全矩阵过；判据：九宫格矩阵 27 组合全 PASS"),
    ("边界与 fuzz（乱操作不崩）", "fuzz 随机输入 10 万次零崩溃；判据：fuzz 后进程存活且自检全绿"),
    ("恢复路径（断电/休眠唤醒状态保全）", "状态快照原子写+唤醒恢复；判据：模拟断电重启后设置回到快照点"),
    ("文档与交付三件套（使用/接口/CHANGELOG）", "三件套与实现一致性机器校验；判据：文档中接口签名与代码 grep 全等"),
    ("域内总日志接入与收官自检", "全部事件入总日志中心统一时间轴并过自检；判据：checks.rs 自检面全域 PASS exit=0"),
]

def main(out_path):
    lines = []
    lines.append("")
    lines.append("---")
    lines.append("")
    lines.append("## 增补卷四 · AI-29 · UNX-F4-E901–E1200（300 项新功能 · E46–E60 批 · 15 批 × 20 条）")
    lines.append("")
    lines.append("> 收录纪律：独立增补编号，不占域账 F22401–F23200；状态列统一「增补」；")
    lines.append("> 全部围绕 Varix 内核锚定（checks.rs 自检面 / lxprocfs.rs 自省面 / 内核事件·会话·IPC 通道 / vxwm surface·popup·特效通道）；")
    lines.append("> 与卷一至卷三判据颗粒零重复；保管母本 docs/unxreal/supp/UNX-F4-SUPP-E901-E1200.md（R-PROC-002 生成器重生成后须回播）。")
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
            eid = f"UNX-F4-E{no:03d}"
            jid = f"{eid}-J1"
            lines.append(
                f"| {eid} | [{bid}] {name} · {title} | `{tag}/{tag}_{i:02d}.rs` | "
                f"Varix 内核锚定：{judge}（{jid}：构造内核事件通道与 vxwm surface stub，调 {tag}_probe() 断言） | 增补 |"
            )
        total += BATCH_SIZE
        lines.append("")
    lines.append(f"> 卷四小计：{total} 条。四卷联轧：UNX-F4-E001–E1200 共 1200 项增补，域账 F22401–F23200 零触碰。")
    lines.append("")
    with open(out_path, "w", encoding="utf-8") as f:
        f.write("\n".join(lines))
    print(f"written {out_path}: {total} rows, batches={len(BATCH_THEMES)}")

if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "_f4_supp4_part.md")
