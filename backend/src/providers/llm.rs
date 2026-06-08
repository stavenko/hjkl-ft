use arti_pipes::llm_executors::qwen::Qwen;

use crate::config::LlmConfig;

pub fn create_executor(config: &LlmConfig) -> Qwen {
    let mut builder = Qwen::builder()
        .api_base(&config.api_base)
        .model(&config.model)
        .think(config.think);

    if let Some(ref key) = config.api_key {
        builder = builder.api_key(key);
    }

    builder.build()
}
