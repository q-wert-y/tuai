//! 斜杠命令 / 命令面板条目定义。

/// 面板动作。
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    SelectModel,
    ProviderAdd,
    ProviderEdit,
    ProviderDelete,
    ProviderSwitch,
    EditSystemPrompt,
    ClearSession,
    ExportSession,
    CopyCode,
    Regenerate,
    Paste,
    Quit,
    /// 面板运行时动作：选择某个模型
    SetModel(String),
    /// 面板运行时动作：切换会话的提供商
    SwitchProvider(String),
    /// 面板运行时动作：删除某个提供商
    RemoveProvider(String),
}

/// 一条命令定义。
pub struct CommandDef {
    pub label: &'static str,
    pub hint: &'static str,
    pub action: Action,
}

/// 全部可用命令。
pub fn all() -> Vec<CommandDef> {
    vec![
        CommandDef {
            label: "选择模型",
            hint: "/model",
            action: Action::SelectModel,
        },
        CommandDef {
            label: "添加提供商",
            hint: "/provider add",
            action: Action::ProviderAdd,
        },
        CommandDef {
            label: "编辑提供商",
            hint: "/provider edit",
            action: Action::ProviderEdit,
        },
        CommandDef {
            label: "删除提供商",
            hint: "/provider del",
            action: Action::ProviderDelete,
        },
        CommandDef {
            label: "切换提供商",
            hint: "/provider use",
            action: Action::ProviderSwitch,
        },
        CommandDef {
            label: "设置系统提示词",
            hint: "/prompt",
            action: Action::EditSystemPrompt,
        },
        CommandDef {
            label: "清空会话消息",
            hint: "/clear",
            action: Action::ClearSession,
        },
        CommandDef {
            label: "导出会话为 Markdown",
            hint: "/export",
            action: Action::ExportSession,
        },
        CommandDef {
            label: "复制最近代码块",
            hint: "/copy",
            action: Action::CopyCode,
        },
        CommandDef {
            label: "重新生成回复",
            hint: "/regen",
            action: Action::Regenerate,
        },
        CommandDef {
            label: "粘贴剪贴板内容",
            hint: "/paste",
            action: Action::Paste,
        },
        CommandDef {
            label: "退出",
            hint: "/quit",
            action: Action::Quit,
        },
    ]
}

/// 解析斜杠命令文本（不含开头 '/'）。返回第一个词匹配的命令。
pub fn parse_slash(text: &str) -> Option<Action> {
    let text = text.trim().trim_start_matches('/');
    let mut parts = text.split_whitespace();
    let first = parts.next()?.to_lowercase();
    let second = parts.next().map(|s| s.to_lowercase());
    for c in all() {
        let hint = c.hint.trim_start_matches('/');
        let mut hp = hint.split_whitespace();
        let h1 = hp.next().unwrap_or_default().to_lowercase();
        let h2 = hp.next().map(|s| s.to_lowercase());
        if h1 == first && h2 == second {
            return Some(c.action);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_parse() {
        assert_eq!(parse_slash("/paste"), Some(Action::Paste));
        assert_eq!(parse_slash("paste"), Some(Action::Paste));
        assert_eq!(parse_slash("/provider add"), Some(Action::ProviderAdd));
        assert_eq!(parse_slash("/model"), Some(Action::SelectModel));
        assert_eq!(parse_slash("/unknown"), None);
        assert_eq!(parse_slash(""), None);
        // 已删除的命令不再解析
        assert_eq!(parse_slash("/new"), None);
        assert_eq!(parse_slash("/fav"), None);
    }
}
