# tuai

> 在终端里，和任何模型对话。

一个跑在终端里的 AI 聊天客户端。单二进制、零依赖、数据全在本地——拷贝走就能用。

```
┌──────────────────────────────────────────────────────────────────┐
│ tuai                                        ★ deepseek-chat      │
├──────────────┬───────────────────────────────────────────────────┤
│ 会话          │  你：帮我写一个 Rust 的冒泡排序                     │
│              │                                                   │
│ ● 冒泡排序    │  AI：                                              │
│   会话 2      │  ```rust                                          │
│   会话 3      │  fn bubble_sort(arr: &mut [i32]) {                │
│              │      for i in 0..arr.len() {                      │
│  [n] 新建     │          for j in 0..arr.len()-i-1 {             │
│  [d] 删除     │              if arr[j] > arr[j+1] {              │
│              │                  arr.swap(j, j+1);                │
│              │              }                                     │
│              │          }                                        │
│              │      }                                            │
│              │  }                                                │
│              │  ```                                              │
├──────────────┴───────────────────────────────────────────────────┤
│ > 在这里输入…                                  Enter 发送         │
├──────────────────────────────────────────────────────────────────┤
│ deepseek · deepseek-chat                 ? 帮助    Ctrl+D 退出    │
└──────────────────────────────────────────────────────────────────┘
```

## 特性

**聊起来**
- **流式输出**——SSE 逐 token 实时渲染，`Esc` 随时停止（已生成的部分保留）
- **Markdown 渲染**——标题 / 列表 / 表格 / 引用，代码块带 syntect 语法高亮，`c` 一键复制
- **思考过程**——兼容 DeepSeek / Kimi 的 `reasoning_content`，折叠展示
- **多行输入 + 排队**——`Shift+Enter` 换行；生成中照常输入，结束后自动发送
- **消息编辑**——`e` 改任意消息，改完 user 消息自动重新生成；`dd` 删除单条

**管起来**
- **多会话**——侧栏 新建 / 切换 / 重命名 / 删除，SQLite 持久化，重启不丢
- **任意 OpenAI 兼容提供商**——DeepSeek / Kimi / 智谱 / 通义 / OpenRouter / Ollama……填 `base_url + api_key` 即可
- **命令面板**——`/` 触发，fuzzy 过滤；提供商 / 模型在 TUI 内直接增删改
- **模型收藏**——常用模型打 ★ 置顶，切换更快
- **上下文记忆**——记住上次用的提供商与模型，新会话直接沿用

**省心**
- **零手动配置**——首次运行自动建目录、写配置、建库
- **自动代理跟随**——Clash 开系统代理就走代理，关了直连，全程无感
- **可靠优先**——网络错误、流中断都在界面提示，不 panic；每条消息完成即落盘

## 快速开始

```bash
# Windows / Linux / macOS，把 release 二进制拷到任意位置即可
./tuai
```

1. 首次启动自动弹出「添加提供商」表单：名称 / Base URL / API Key / 默认模型
2. 例如 DeepSeek：`https://api.deepseek.com/v1` + `deepseek-chat`
3. 回车发送第一条消息

所有数据都在**可执行文件同目录**的 `.tuai/` 下——一个二进制 + 一个数据目录，换机器直接拷贝：

```
.tuai/
├── config.toml   # 提供商、API key、收藏、代理等配置
├── tuai.db       # 会话与消息（SQLite）
├── exports/      # /export 导出的 Markdown
└── logs/         # 调试日志
```

> 也可以启动前在 `.tuai/config.toml` 里填好 `api_key`，或用环境变量 `TUAI_API_KEY` 覆盖。

## 键位

| 键 | 功能 |
| --- | --- |
| `Enter` | 发送 · `Shift/Alt+Enter` 换行 |
| `Tab` | 切换焦点：输入 ↔ 消息 ↔ 侧栏 |
| `Esc` | 停止生成 / 关闭面板 / 返回输入 |
| `j` `k` `↑` `↓` | 选择消息 / 列表导航（首尾循环） |
| `g` / `G` | 跳首条 / 末条消息 |
| `e` | 编辑选中消息（user 消息提交后重新生成） |
| `d` `d` | 删除选中消息（按两次） |
| `c` | 复制最近代码块 |
| `n` / `r` / `d` `d` | 侧栏：新建 / 重命名 / 删除会话 |
| `f` | 模型面板中收藏模型 |
| `Ctrl+R` | 重新生成回复 |
| `/` | 命令面板 · `?` 完整帮助 |
| `Ctrl+C` ×2 / `Ctrl+D` | 退出 |

常用命令：`/model` `/provider` `/prompt` `/clear` `/export` `/copy` `/regen` `/paste` `/quit`

## 配置示例

```toml
default_provider = "deepseek"
system_prompt = "你是一个简洁的助手"

[[providers]]
name = "deepseek"
base_url = "https://api.deepseek.com/v1"
api_key = "sk-xxx"
default_model = "deepseek-chat"

[[providers]]
name = "ollama"
base_url = "http://localhost:11434/v1"
api_key = ""
default_model = "qwen3"

# 可选：强制代理（留空则自动检测系统代理）
# proxy = "http://127.0.0.1:7890"
```

## 从源码构建

需要 [Rust](https://rustup.rs)（1.75+）：

```bash
cargo build --release   # 当前平台
cargo test              # 运行测试
```

代码跨平台（Windows / Linux / macOS），在各自平台执行上面的命令即可编译。

## 设计原则

- **纯聊天**——请求体只有 `{model, messages, stream}`，不带任何工具字段，零多余 token
- **可靠优先**——错误只在界面提示，不崩溃；每条消息完成即落盘
- **可移植**——单静态二进制 + 数据全在程序目录，无钥匙串、无外部数据库、无运行时依赖
- **不阻塞**——生成期间照常输入，消息排队自动发出

## 平台

| 平台 | 状态 |
| --- | --- |
| Windows x64 | 已验证 |
| Linux / macOS | 代码跨平台，可自行 `cargo build --release` 编译 |

## License

[Apache-2.0](LICENSE)
