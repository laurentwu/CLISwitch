pub mod claude_code;
pub mod codex;
pub mod opencode;
mod opencode_provider_map;
pub mod traits;

pub use claude_code::ClaudeCodeAdapter;
pub use codex::CodexAdapter;
pub use opencode::OpenCodeAdapter;
pub use traits::*;
