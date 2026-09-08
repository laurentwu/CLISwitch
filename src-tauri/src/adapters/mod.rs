pub mod claude_code;
pub mod codex;
pub mod opencode;
pub mod qwen;
pub mod traits;

pub use claude_code::ClaudeCodeAdapter;
pub use codex::CodexAdapter;
pub use opencode::OpenCodeAdapter;
pub use qwen::QwenAdapter;
pub use traits::*;
