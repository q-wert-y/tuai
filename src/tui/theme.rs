//! 配色主题（Nord 风格暗色）。

use ratatui::style::Color;

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// 强调色（边框高亮 / 标题）
    pub accent: Color,
    /// 次要文字
    pub dim: Color,
    /// 更暗的分隔色
    pub faint: Color,
    /// 用户消息标签
    pub user: Color,
    /// 助手消息标签
    pub assistant: Color,
    /// 思考过程
    pub reasoning: Color,
    /// 错误
    pub error: Color,
    /// 代码块语言标签 / 边框
    pub code: Color,
    /// 引用
    pub quote: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            accent: Color::Rgb(136, 192, 208),
            dim: Color::Rgb(129, 161, 193),
            faint: Color::Rgb(76, 86, 106),
            user: Color::Rgb(143, 188, 187),
            assistant: Color::Rgb(180, 142, 192),
            reasoning: Color::Rgb(108, 122, 147),
            error: Color::Rgb(191, 97, 106),
            code: Color::Rgb(94, 129, 172),
            quote: Color::Rgb(108, 122, 147),
        }
    }
}
