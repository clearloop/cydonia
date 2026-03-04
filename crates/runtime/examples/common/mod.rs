//! Shared helpers for runtime examples.

#![allow(dead_code)]

use model::ProviderManager;
use walrus_runtime::{Hook, Memory, ToolRegistry, prelude::*};
use wcore::AgentEvent;

/// Example hook providing event observation and memory.
pub struct ExampleHook {
    pub memory: InMemory,
}

impl ExampleHook {
    /// Create a new ExampleHook.
    pub fn new() -> Self {
        Self {
            memory: InMemory::new(),
        }
    }
}

impl Hook for ExampleHook {
    fn on_build_agent(&self, config: AgentConfig) -> AgentConfig {
        self.memory.on_build_agent(config)
    }

    fn on_register_tools(
        &self,
        tools: &mut ToolRegistry,
    ) -> impl std::future::Future<Output = ()> + Send {
        self.memory.on_register_tools(tools)
    }
}

/// Initialize tracing with env-filter support.
pub fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
}

/// Load DEEPSEEK_API_KEY from .env file, falling back to environment.
pub fn load_api_key() -> String {
    let _ = dotenvy::dotenv();
    std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY must be set")
}

/// Build a ProviderManager from the default DeepSeek config.
pub fn build_provider() -> ProviderManager {
    let key = load_api_key();
    let config = model::ProviderConfig {
        model: "deepseek-chat".into(),
        api_key: Some(key),
        base_url: None,
        loader: None,
        quantization: None,
        chat_template: None,
    };
    let provider =
        model::openai::OpenAI::deepseek(model::Client::new(), config.api_key.as_ref().unwrap())
            .expect("failed to create provider");
    ProviderManager::single(config, model::Provider::OpenAI(provider))
}

/// Build a default ExampleHook (empty memory).
pub fn build_hook() -> ExampleHook {
    ExampleHook::new()
}

/// Build a Runtime with the default provider and ExampleHook.
pub async fn build_runtime() -> Runtime<ProviderManager, ExampleHook> {
    let hook = build_hook();
    let provider = build_provider();
    Runtime::new(provider, hook).await
}

/// Simple REPL loop: read lines from stdin, stream to agent.
pub async fn repl<M: wcore::model::Model + Send + Sync + Clone + 'static, H: Hook + 'static>(
    runtime: &Runtime<M, H>,
    agent: &str,
) {
    use futures_util::StreamExt;
    use std::io::{BufRead, Write};

    loop {
        print!("> ");
        std::io::stdout().flush().unwrap();
        let mut input = String::new();
        if std::io::stdin().lock().read_line(&mut input).unwrap() == 0 {
            break;
        }
        let input = input.trim();
        if input.is_empty() || input == "exit" || input == "quit" {
            break;
        }
        let mut stream = std::pin::pin!(runtime.stream_to(agent, input));
        while let Some(event) = stream.next().await {
            if let AgentEvent::TextDelta(text) = &event {
                print!("{text}");
                std::io::stdout().flush().ok();
            }
        }
        println!();
    }
}

/// REPL loop that prints memory entries after each exchange.
pub async fn repl_with_memory(runtime: &Runtime<ProviderManager, ExampleHook>, agent: &str) {
    use futures_util::StreamExt;
    use std::io::{BufRead, Write};

    loop {
        print!("> ");
        std::io::stdout().flush().unwrap();
        let mut input = String::new();
        if std::io::stdin().lock().read_line(&mut input).unwrap() == 0 {
            break;
        }
        let input = input.trim();
        if input.is_empty() || input == "exit" || input == "quit" {
            break;
        }

        {
            let mut stream = std::pin::pin!(runtime.stream_to(agent, input));
            while let Some(event) = stream.next().await {
                if let AgentEvent::TextDelta(text) = &event {
                    print!("{text}");
                    std::io::stdout().flush().ok();
                }
            }
            println!();
        }

        // Print current memory state.
        let entries = runtime.hook.memory.entries();
        if entries.is_empty() {
            println!("[Memory: empty]");
        } else {
            println!("[Memory: {} entries]", entries.len());
            for (key, value) in &entries {
                let display = if value.len() > 60 {
                    let end = value
                        .char_indices()
                        .take_while(|&(i, _)| i <= 57)
                        .last()
                        .map(|(i, c)| i + c.len_utf8())
                        .unwrap_or(0);
                    format!("{}...", &value[..end])
                } else {
                    value.clone()
                };
                println!("  {key} = {display}");
            }
        }
    }
}
