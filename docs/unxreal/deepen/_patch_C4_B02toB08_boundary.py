# -*- coding: utf-8 -*-
"""C4-B02/03/04/05/06/07/08 边界注记批量插入 + 状态标记修复
（AI-14 · finalize ⑤ 四项齐备修复 · 34 条注记 + F10434/F10474 状态翻牌）
"""
import re
import sys

PATCHES_BY_FILE = {
    "C4-B02.md": {
        "UNX-F10435": "边界注记：挂接等价断言=同一回调符号级引用（非复制实现）；注册可见性（epoll 注册 FIFO 后立即可 wait）格 10/10 声明；差异账恒空为绿（非空即分派层红）。",
    },
    "C4-B03.md": {
        "UNX-F10454": "边界注记：快照原子（位图读取持掩码账锁，防读半态）；解除递送判定点=下一信号检查点（不抢当前执行流，时序声明）；被阻塞位在场断言含 SIGKILL/SIGSTOP 特例（不可阻塞位照常排除注账）。",
    },
    "C4-B04.md": {
        "UNX-F10471": "边界注记：快照与执行绑定于 pending 节点（投递即快照，执行不回查）。",
    },
    "C4-B05.md": {
        "UNX-F10491": "边界注记：上限判定序=fd 账闸先于 pty 全局闸（双闸判定序单点）；串扰探针含跨对写读双向（A→B 与 B→A 两向）；释放-再开复用同一 fd 号验证（回收完整性旁证）。",
        "UNX-F10498": "边界注记：事件打点原子（建/删动作与打点同锁，防漏打）；无人订阅不丢单（事件仍入账，订阅侧回放声明）。",
    },
    "C4-B06.md": {
        "UNX-F10504": "边界注记：九位叠加次序=规则表登记序（表序即执行序，冲突结果落账）；BREAK 预埋位不参与本判据（声明隔离）；位关闭=字节直通断言含全关基线（九位全零输入恒等输出）。",
        "UNX-F10506": "边界注记：位账三栏登记制（语义/出处/是否生效，生效栏诚实标注不装实现）；掩码操作不扰邻位断言（读-改-写同锁）；PARENB 位无 UART 面下纯存取（无行为效应声明）。",
        "UNX-F10511": "边界注记：全零 winsize 合法（清屏语义声明）；防抖比对与写账同锁（TOCTOU 防线）。",
        "UNX-F10518": "边界注记：判序单点=NONBLOCK 先于象限（压倒关系可观测）；交错账含切旗标中途等待态用例（等待被解除即返）；8 格表与 Linux 行为对照注出处。",
    },
    "C4-B07.md": {
        "UNX-F10521": "边界注记：判界集合单点（NL/EOF/VEOL 统一查表，F10528 扩展位）；提交唤醒先查后挂（B01 纪律）。",
        "UNX-F10522": "边界注记：冲突格（同开）行为落账不装未定义；次序表与 B06 规则表同源（单点索引）。",
        "UNX-F10524": "边界注记：清行不触发信号（VSUSP/VINTR 语义隔离断言）；^D 提交空行返 0 字节（与 EOF 终态区分声明）；ECHOK=0 回显按 echo 字符面（行为账注 termios 条文）。",
        "UNX-F10528": "边界注记：禁用值判界=_POSIX_VDISABLE（0x00 定义注账）；切换即时生效探针含输入中途改值用例。",
        "UNX-F10529": "边界注记：VLNEXT 字面属性单次有效（下一字节消费即失效，注账）；VWERASE 删界=空白判定（空格/制表定义落账）；重打回显序=缓冲原序（不重排声明）。",
        "UNX-F10530": "边界注记：界符永在缓冲位（4096 满时界符仍入，行不丢界）；响铃 0x07 预埋不占判据（可配置声明）。",
        "UNX-F10531": "边界注记：可打印化映射表单点（^X 两字节定版，0x7F→^? 注账）；旁路队列满丢回显不丢输入（主链不受影响声明）。",
        "UNX-F10532": "边界注记：OPOST=0 全零执行（管线短路断言）；OXTABS 制表扩展=TAB→空格（宽度 8 注账）；三规则次序表与位面判据场同源。",
        "UNX-F10533": "边界注记：tcsetattr 原子=管线锁内换态（无半态窗口探针）；重放按新模式重判界（旧模式半行不延续）。",
        "UNX-F10534": "边界注记：截断读只切已提交数据（不跨行界取半行，行原子纪律）；三段读续读内容精确（偏移账）；VMIN 协同=行提交即数据可得（等待面语义声明）。",
        "UNX-F10535": "边界注记：两点唯一性断言（grep+运行期登记表双验）；raw 直通含 IXON 消费声明外全零加工；进出计数对平挂 F10538（联动账）。",
        "UNX-F10536": "边界注记：行段原子出队=队列项级原子（读者取整行不劈半，探针常跑）；守恒账含截断读分段（分段量计入得数和）；读者并发取锁序=队列锁单点（无读者间次序承诺，先到先得声明）；100 轮探针含读者中途退出用例（余行归还队列守恒）。",
        "UNX-F10537": "边界注记：ISIG=0 三字节按普通字符（若非界符则入队可读，探针）；恢复即时=下一检查点生效（不重放历史输入）；短路点单点（生成面入口，grep 断言）。",
        "UNX-F10538": "边界注记：四计原子计数（各计独立原子量）；对平恒等式落账（加工=提交+丢弃等）；散点断言=中途任意时点读账不违恒等（瞬态一致性声明）。",
        "UNX-F10539": "边界注记：种子入账可复现（伪随机确定性）；透明断言扣除项显性化（IXON 消费、界符、编辑字节三账面对推）；稳账含管线复位断言（轮间零残留）。",
        "UNX-F10540": "边界注记：基线账三要素定版（模式序列/字节流摘要/判据锚点缺一不成基线）；挂号核验含分派项三对应。",
    },
    "C4-B08.md": {
        "UNX-F10542": "边界注记：组账锁单点（改籍与查籍同锁）；旧组解散账=组长迁出后成员表空即销组（守恒断言）；关窗钩子=exec 完成点（关窗后互设 EACCES 注账）。",
        "UNX-F10544": "边界注记：判定序=tty 主查询→归属判定→赋/拒（序单点不可换）；强夺限缩=成员且无主（POSIX 注账）；并发竞取同锁（零双主探针）。",
        "UNX-F10547": "边界注记：orphaned 判定与停止事件同锁原子（防判定竞态）；SIGHUP/SIGCONT 链不可分隔（双发注账，不可只发其一）；反向断言含继续事件路径（对称性）。",
        "UNX-F10551": "边界注记：双向清空同锁（会话侧/tty 侧两写原子）；错会话 ENOTTY 判定先于清空动作（序声明）。",
        "UNX-F10552": "边界注记：引用归零探测与 close 链同锁（防漏报）；快照发送=挂位时成员表快照（广播中退组不追投）；四拍时间戳单调断言（乱序即红）。",
        "UNX-F10554": "边界注记：快照与挂位同锁（成员表变更竞态防线）；错误判定序=ESRCH（组不存在）先于 EPERM（归属判定）；跳过账=已死成员计入快照数（守恒口径声明）。",
        "UNX-F10556": "边界注记：切换原子=锁内读-改-写（无双前台窗口探针）；投递目标唯一断言含锁外瞬态（交错注入器验证）；封装绕行 grep 断言常跑（改账必须走单点）。",
        "UNX-F10557": "边界注记：断言器全账遍历原子快照（扫描期不与变更交错，取账快照后断言）；违例红账含进程 id 与违反项二元组（可定位）；五条出处逐条注账（POSIX 条文）；审计场含极端序列（setsid 后立即 setpgid 等 8 组边界用例）。",
        "UNX-F10559": "边界注记：随机种子入账可复现（轮转序列确定性）；零悬挂线=60 秒完成探针（超时即红不静默）；SIGTTIN/TTOU 计数推算与实测对平（后台读写停止语义）；守恒账含中途退出作业（成员减员计入）。",
    },
}

# 状态/格式修复：(文件, 旧串, 新串, 说明)
STATUS_FIXES = [
    (
        "C4-B02.md",
        "｜状态：[深化]",
        "｜状态：[已深化]",
        "F10434 状态翻牌",
    ),
    (
        "C4-B04.md",
        "- 域/批：C4/B04｜纯功能行数：240｜状态：[骨架]｜判据：UNX-F10474-J1 尺寸变更事件→SIGWINCH 挂位→前台组递送链可观测（事件源 B06 挂接），链路探针全绿",
        "- 域/批：C4/B04｜判据：UNX-F10474-J1 尺寸变更事件→SIGWINCH 挂位→前台组递送链可观测（事件源 B06 挂接），链路探针全绿｜纯功能行数：240 行（链路注册面 80 + 注入递送面 80 + 接线升级面 80；测试段不计）｜状态：[已深化]",
        "F10474 域/批行重排为标准格式（判据前置/行数补分解与「行」后缀/状态翻牌）",
    ),
]

def patch_file(fn, patches):
    with open(fn, encoding="utf-8") as f:
        text = f.read()
    applied, skipped = [], []
    for fid, note in patches.items():
        m = re.search(r"(### " + fid + r"\b.*?\n)(- 正文：[^\n]*)", text, re.S)
        if not m:
            skipped.append((fid, "正文行未定位"))
            continue
        line = m.group(2)
        if "边界注记：" in line:
            skipped.append((fid, "已含边界注记，跳过"))
            continue
        idx = line.find("与现存内核衔接点：")
        if idx < 0:
            skipped.append((fid, "衔接点锚未命中"))
            continue
        end = line.find("。", idx)
        if end < 0:
            skipped.append((fid, "衔接点句号未命中"))
            continue
        if text.count(line) != 1:
            skipped.append((fid, "正文行不唯一，拒绝盲替换"))
            continue
        text = text.replace(line, line[:end + 1] + note + line[end + 1:], 1)
        applied.append(fid)
    with open(fn, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    return applied, skipped

def main():
    total_applied, all_skipped, fail = 0, [], False
    for fn, patches in PATCHES_BY_FILE.items():
        applied, skipped = patch_file(fn, patches)
        total_applied += len(applied)
        all_skipped += [(fn,) + s for s in skipped]
        print(f"{fn}: applied={len(applied)}/{len(patches)}")
        if applied:
            print(f"  {', '.join(applied)}")
        if len(applied) != len(patches):
            fail = True

    for fn, old, new, why in STATUS_FIXES:
        with open(fn, encoding="utf-8") as f:
            text = f.read()
        cnt = text.count(old)
        if cnt != 1:
            print(f"STATUS-FIX {fn} FAILED: 旧串命中 {cnt} 次（预期 1）—— {why}")
            fail = True
            continue
        text = text.replace(old, new, 1)
        with open(fn, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
        print(f"STATUS-FIX {fn}: OK —— {why}")

    if all_skipped:
        print("skipped:")
        for s in all_skipped:
            print(f"  {s[0]} {s[1]}: {s[2]}")
    print(f"TOTAL applied={total_applied}/34")
    if fail:
        sys.exit(1)

if __name__ == "__main__":
    main()
