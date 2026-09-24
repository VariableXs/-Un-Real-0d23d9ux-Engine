# 大测试总闸 run-tests · 收编台账（2026-09-24）

**任务来源**：Variable 明令（2026-09-23）——将 `_attic/` 与 `scripts/` 散装战役脚本整合为统一测试入口，速度优先，旧脚本原样收编不重写。
**交付物**：`scripts/run-tests.py`（总闸本体）+ `scripts/test-registry.json`（自动生成注册表，含手工覆盖层）。

## 一、用法速查

| 命令 | 内容 | 速度 | 场景 |
| --- | --- | --- | --- |
| `python scripts/run-tests.py fast` | cargo test（kernel workspace 宿主全量） | ~5 分钟（含编译） | 日常、每包收口 |
| `python scripts/run-tests.py selftest` | 已登记自检类脚本 | 秒~分钟级 | 日常 |
| `python scripts/run-tests.py full` | fast + selftest + QEMU 批队列（串行继续跑） | 小时级 | 里程碑闸门、过夜无人值守 |
| `python scripts/run-tests.py single <文件名>` | 单跑任一收编脚本 | 视脚本 | 定向复跑 |
| `python scripts/run-tests.py list [--class C]` | 台账浏览 | — | — |
| `python scripts/run-tests.py regen` | 重扫重建注册表（保留手工覆盖） | — | 新脚本入库后 |

各档支持 `--dry-run` 预演队列；`single` 可 `--timeout`；`full` 单项默认超时 1h 防挂死。日志统一落 `_attic/run-tests-logs/<时间戳>.log`。

## 二、注册表与分类（regen 自动 + 手工覆盖）

共收编 **272** 个脚本：danger=97 / probe=76 / oneshot=58 / qemu=32 / demo=8 / selftest=2。

| 类别 | 规则 | 安全级 | 参与档位 |
| --- | --- | --- | --- |
| selftest | 名含 selftest + 手工登记 | 安全 | fast 之外日常可跑 |
| probe | 探针/诊断/只读/校验/冒烟/走查 | 安全 | single / 队列可调 |
| demo | 功能演示 | 安全 | single |
| qemu | QEMU/HMP/战役/走查/断电注入 | 安全(慢) | 仅 full 队列（串行继续跑） |
| danger | 写盘/提权/部署/引导/注册表写 | ⚠危险 | **永不入 fast/selftest/full**；single 必须 `--yes` |
| oneshot | 一次性补丁/战役残件 | 存档 | 总闸不跑 |

**手工覆盖（overrides，regen 保留）**：
- `portable/engine/hardening/run_all.py` → selftest（WP-103 四板斧自检）
- `_attic/usb-unplug-drill.py` → danger（物理拔盘演练需真机在场，禁入队列）

**红线合规**：danger 类 97 个（bcd 系/0xED 修复系/部署系/提权系）全部隔离在人工确认墙后；`single` 无 `--yes` 时拒绝执行（exit 3）并输出红线警告——已实测验证。

## 三、验证证据三件套

- **数据**：`fast` 端到端真跑全绿——cargo test 3249 + fuzz 1 + fuzz_parsers 6 = **3256 通过 / 0 失败**，耗时 5m19s（含编译 22.9s）；`selftest --dry-run`、`full --dry-run` 队列正确；`single list-volumes.py`（probe 类）真实执行 exit 0；`single vx-fix-0xed.py`（danger 类）无 `--yes` 被拒 exit 3。
- **复现命令**：`python scripts/run-tests.py fast`；`python scripts/run-tests.py list`。
- **日期**：2026-09-24。

## 四、已知边界与后续

1. 分类按文件名规则 + 少量手工覆盖，个别脚本归类可能与实际行为有出入——`single` 前可用 `list --class` 核对；发现归类错误直接改 `test-registry.json` 的 `overrides`（regen 不丢）。
2. `fast` 默认只跑内核 workspace；前端 vitest 用 `fast --with-front` 附加（量大，默认不进日常档）。
3. QEMU 队列项之间无依赖排序（按文件名字典序），后续可按战役依赖手工排序 overrides。
4. 本总闸自身零系统写入（仅日志落 `_attic/run-tests-logs/`），符合硬件与数据安全红线。
