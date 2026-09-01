# tuai

终端里的 AI 聊天客户端。简洁、快速、单二进制、零依赖——拷到哪台电脑都能直接跑。

## 特性

- **任意 OpenAI 兼容提供商**：DeepSeek / Kimi / 智谱 / 通义 / OpenRouter / SiliconFlow / Ollama……只要有 `base_url + api_key` 就能接
- **流式输出**：SSE 逐 token 渲染，Esc 随时停止（已生成部分保留）
- **Markdown 渲染**：标题 / 列表 / 表格 / 代码块语法高亮（syntect），一键复制代码
- **多会话**：侧栏管理，创建 / 切换 / 重命名 / 删除，SQLite 持久化，重启不丢
- **消息编辑与重发**：编辑任意消息，改 user 消息后自动重新生成；删除单条消息
- **DeepSeek / Kimi 兼容**：`reasoning_content` 思考过程展示
- **命令面板**：`/` 触发，fuzzy 过滤，斜杠命令同义
- **模型收藏**：常用模型一键收藏（★），切换更快
- **自动代理跟随**：Clash 开系统代理就走代理，关了直连，无需手动配置
- **上下文记忆**：记住上次的提供商与模型，新建会话直接沿用

## 快速开始

1. 运行 `tuai`（或 `tuai.exe`）
2. 首次启动自动弹出「添加提供商」表单，填入名称 / Base URL / API Key / 默认模型
3. 开始聊天

> 也可以先在 `.tuai/config.toml` 里填好 `api_key` 再启动，或设置环境变量 `TUAI_API_KEY`。
> 例：Base URL `https://api.deepseek.com/v1`，默认模型 `deepseek-chat`。

所有数据都在**可执行文件同目录**的 `.tuai/` 下，整个程序就是「一个二进制 + 一个数据目录」，换电脑拷贝这两个即可。

```
.tuai/
├── config.toml   # 提供商、API key、收藏、代理等配置
├── tuai.db       # 会话与消息（SQLite）
├── exports/      # /export 导出的 Markdown
└── logs/         # 调试日志
```

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
| `f` | 模型面板中收藏模型（或 `/fav`） |
| `Ctrl+R` | 重新生成回复 |
| `/` | 命令面板 · `?` 完整帮助 |
| `Ctrl+C` ×2 / `Ctrl+D` | 退出 |

常用斜杠命令：`/new` `/sessions` `/model` `/provider` `/clear` `/export` `/rename` `/prompt` `/fav` `/regen` `/help` `/quit`

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

环境变量 `TUAI_API_KEY` / `TUAI_BASE_URL` 可临时覆盖第一个提供商的对应字段。

## 从源码构建

需要 [Rust](https://rustup.rs)（1.75+）。

```bash
cargo build --release          # 当前平台
cargo test                     # 运行测试
```

Linux/macOS 下在同一目录执行 `cargo build --release` 即可（代码本身跨平台，只是本项目未提供预编译包）。

## 设计原则

- **纯聊天**：请求体只有 `{model, messages, stream}`，不带任何工具字段，零多余 token
- **可靠优先**：网络错误、流中断、非法响应均以 UI 提示呈现，不崩溃；每条消息完成即落盘
- **可移植**：单静态二进制 + 全数据在程序目录，无系统钥匙串、无外部数据库、无运行时依赖
- **生成中不阻塞输入**：流式期间发送的消息排队，结束后自动发出

## 平台

| 平台 | 状态 |
| --- | --- |
| Windows x64 | 已验证 |
| Linux / macOS | 代码跨平台，可自行 `cargo build --release` 编译 |

## License

MIT
