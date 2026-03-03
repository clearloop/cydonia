//! Shared helpers for runtime examples.

#![allow(dead_code)]

use anyhow::Result;
use compact_str::CompactString;
use model::ProviderManager;
use std::collections::BTreeMap;
use std::sync::Arc;
use walrus_runtime::{AgentDispatcher, Handler, Hook, Memory, Tool, prelude::*};
use wcore::AgentEvent;

/// Example hook providing optional tool dispatch.
pub struct ExampleHook {
    memory: InMemory,
    tools: BTreeMap<CompactString, (Tool, Handler)>,
}

impl ExampleHook {
    /// Create a new ExampleHook.
    pub fn new() -> Self {
        Self {
            memory: InMemory::new(),
            tools: BTreeMap::new(),
        }
    }

    /// Register a tool with its handler.
    pub fn register<F, Fut>(&mut self, tool: Tool, handler: F)
    where
        F: Fn(String) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = String> + Send + 'static,
    {
        let name = tool.name.clone();
        let handler: Handler = Arc::new(move |args| Box::pin(handler(args)));
        self.tools.insert(name, (tool, handler));
    }

    /// Access the memory backend.
    pub fn memory(&self) -> &InMemory {
        &self.memory
    }
}

impl Hook for ExampleHook {
    fn tools(&self, _agent: &str) -> Vec<Tool> {
        self.tools.values().map(|(t, _)| t.clone()).collect()
    }

    fn dispatch(
        &self,
        _agent: &str,
        calls: &[(&str, &str)],
    ) -> impl std::future::Future<Output = Vec<Result<String>>> + Send {
        let calls: Vec<(String, String)> = calls
            .iter()
            .map(|(m, p)| (m.to_string(), p.to_string()))
            .collect();
        let handlers: Vec<_> = calls
            .iter()
            .map(|(method, _)| self.tools.get(method.as_str()).map(|(_, h)| Arc::clone(h)))
            .collect();

        async move {
            let mut results = Vec::with_capacity(calls.len());
            for (i, (method, params)) in calls.iter().enumerate() {
                let output = if let Some(ref handler) = handlers[i] {
                    Ok(handler(params.clone()).await)
                } else {
                    Ok(format!("function {method} not available"))
                };
                results.push(output);
            }
            results
        }
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
        model::deepseek::DeepSeek::new(model::Client::new(), config.api_key.as_ref().unwrap())
            .expect("failed to create provider");
    ProviderManager::single(config, model::Provider::DeepSeek(provider))
}

/// Build a default ExampleHook (no tools, empty memory).
pub fn build_hook() -> ExampleHook {
    ExampleHook::new()
}

/// Build a Runtime with the default provider and ExampleHook.
pub fn build_runtime() -> (Runtime<ProviderManager, ExampleHook>, Arc<ExampleHook>) {
    let hook = Arc::new(build_hook());
    let provider = build_provider();
    let runtime = Runtime::new(provider, Arc::clone(&hook));
    (runtime, hook)
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
        let Some(mut agent_instance) = runtime.take_agent(agent).await else {
            eprintln!("agent '{agent}' not registered");
            break;
        };
        agent_instance.push_message(Message::user(input));
        {
            let dispatcher = AgentDispatcher {
                hook: runtime.hook(),
                agent,
            };
            let mut stream = std::pin::pin!(agent_instance.run_stream(&dispatcher));
            while let Some(event) = stream.next().await {
                if let AgentEvent::TextDelta(text) = &event {
                    print!("{text}");
                    std::io::stdout().flush().ok();
                }
            }
        }
        println!();
        runtime.put_agent(agent_instance).await;
    }
}

/// REPL loop that prints memory entries after each exchange.
pub async fn repl_with_memory(
    runtime: &Runtime<ProviderManager, ExampleHook>,
    hook: &ExampleHook,
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
        {
            let Some(mut agent_instance) = runtime.take_agent(agent).await else {
                eprintln!("agent '{agent}' not registered");
                break;
            };
            agent_instance.push_message(Message::user(input));
            {
                let dispatcher = AgentDispatcher {
                    hook: runtime.hook(),
                    agent,
                };
                let mut stream = std::pin::pin!(agent_instance.run_stream(&dispatcher));
                while let Some(event) = stream.next().await {
                    if let AgentEvent::TextDelta(text) = &event {
                        print!("{text}");
                        std::io::stdout().flush().ok();
                    }
                }
            }
            println!();
            runtime.put_agent(agent_instance).await;
        }

        // Print current memory state.
        let entries = hook.memory().entries();
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
