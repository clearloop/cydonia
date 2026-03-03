//! Server trait implementation for the Gateway.

use crate::gateway::Gateway;
use anyhow::{Result, bail};
use futures_util::StreamExt;
use memory::Memory;
use protocol::api::Server;
use protocol::message::{
    AgentDetail, AgentInfoRequest, AgentList, AgentSummary, ClearSessionRequest, DownloadEvent,
    DownloadRequest, GetMemoryRequest, McpAddRequest, McpAdded, McpReloaded, McpRemoveRequest,
    McpRemoved, McpServerList, McpServerSummary, MemoryEntry, MemoryList, SendRequest,
    SendResponse, SessionCleared, SkillsReloaded, StreamEvent, StreamRequest,
};
use tokio::sync::mpsc;
use wcore::AgentEvent;

impl Server for Gateway {
    async fn send(&self, req: SendRequest) -> Result<SendResponse> {
        let response = self.runtime.send_to(&req.agent, &req.content).await?;
        Ok(SendResponse {
            agent: req.agent,
            content: response.final_response.unwrap_or_default(),
        })
    }

    fn stream(
        &self,
        req: StreamRequest,
    ) -> impl futures_core::Stream<Item = Result<StreamEvent>> + Send {
        let runtime = self.runtime.clone();
        let agent = req.agent;
        let content = req.content;
        async_stream::try_stream! {
            yield StreamEvent::Start { agent: agent.clone() };

            let stream = runtime.stream_to(&agent, &content);
            futures_util::pin_mut!(stream);
            while let Some(event) = stream.next().await {
                match event {
                    AgentEvent::TextDelta(text) => {
                        yield StreamEvent::Chunk { content: text };
                    }
                    AgentEvent::Done(_) => break,
                    _ => {}
                }
            }

            yield StreamEvent::End { agent: agent.clone() };
        }
    }

    async fn clear_session(&self, req: ClearSessionRequest) -> Result<SessionCleared> {
        self.runtime.clear_session(&req.agent).await;
        Ok(SessionCleared { agent: req.agent })
    }

    async fn list_agents(&self) -> Result<AgentList> {
        let agents = self
            .runtime
            .agents()
            .await
            .into_iter()
            .map(|a| AgentSummary {
                name: a.name.clone(),
                description: a.description.clone(),
            })
            .collect();
        Ok(AgentList { agents })
    }

    async fn agent_info(&self, req: AgentInfoRequest) -> Result<AgentDetail> {
        match self.runtime.agent(&req.agent).await {
            Some(a) => Ok(AgentDetail {
                name: a.name.clone(),
                description: a.description.clone(),
                tools: a.tools.to_vec(),
                skill_tags: a.skill_tags.to_vec(),
                system_prompt: a.system_prompt.clone(),
            }),
            None => bail!("agent not found: {}", req.agent),
        }
    }

    async fn list_memory(&self) -> Result<MemoryList> {
        let entries = self.runtime.hook().memory().entries();
        Ok(MemoryList { entries })
    }

    async fn get_memory(&self, req: GetMemoryRequest) -> Result<MemoryEntry> {
        let value = self.runtime.hook().memory().get(&req.key);
        Ok(MemoryEntry {
            key: req.key,
            value,
        })
    }

    fn download(
        &self,
        req: DownloadRequest,
    ) -> impl futures_core::Stream<Item = Result<DownloadEvent>> + Send {
        let hf_endpoint = self.hf_endpoint.clone();
        async_stream::try_stream! {
            yield DownloadEvent::Start { model: req.model.clone() };

            let (dtx, mut drx) = mpsc::unbounded_channel();
            let model_str = req.model.to_string();
            let endpoint = hf_endpoint;
            let download_handle = tokio::spawn(async move {
                model::local::download::download_model(&model_str, &endpoint, dtx).await
            });

            while let Some(event) = drx.recv().await {
                let dl_event = match event {
                    model::local::download::DownloadEvent::FileStart { filename, size } => {
                        DownloadEvent::FileStart { filename, size }
                    }
                    model::local::download::DownloadEvent::Progress { bytes } => {
                        DownloadEvent::Progress { bytes }
                    }
                    model::local::download::DownloadEvent::FileEnd { filename } => {
                        DownloadEvent::FileEnd { filename }
                    }
                };
                yield dl_event;
            }

            match download_handle.await {
                Ok(Ok(())) => {
                    yield DownloadEvent::End { model: req.model };
                }
                Ok(Err(e)) => {
                    Err(anyhow::anyhow!("download failed: {e}"))?;
                }
                Err(e) => {
                    Err(anyhow::anyhow!("download task panicked: {e}"))?;
                }
            }
        }
    }

    async fn reload_skills(&self) -> Result<SkillsReloaded> {
        let count = self.runtime.hook().skills().reload().await?;
        tracing::info!("reloaded {count} skill(s)");
        Ok(SkillsReloaded { count })
    }

    async fn mcp_add(&self, req: McpAddRequest) -> Result<McpAdded> {
        let config = mcp::McpServerConfig {
            name: req.name.clone(),
            command: req.command,
            args: req.args,
            env: req.env,
            auto_restart: true,
        };
        let tools = self.runtime.hook().mcp().add(config).await?;
        Ok(McpAdded {
            name: req.name,
            tools,
        })
    }

    async fn mcp_remove(&self, req: McpRemoveRequest) -> Result<McpRemoved> {
        let tools = self.runtime.hook().mcp().remove(&req.name).await?;
        Ok(McpRemoved {
            name: req.name,
            tools,
        })
    }

    async fn mcp_reload(&self) -> Result<McpReloaded> {
        let servers = self
            .runtime
            .hook()
            .mcp()
            .reload(|path| {
                let config = crate::DaemonConfig::load(path)?;
                Ok(config.mcp_servers)
            })
            .await?;
        let servers = servers
            .into_iter()
            .map(|(name, tools)| McpServerSummary { name, tools })
            .collect();
        Ok(McpReloaded { servers })
    }

    async fn mcp_list(&self) -> Result<McpServerList> {
        let servers = self
            .runtime
            .hook()
            .mcp()
            .list()
            .await
            .into_iter()
            .map(|(name, tools)| McpServerSummary { name, tools })
            .collect();
        Ok(McpServerList { servers })
    }

    async fn ping(&self) -> Result<()> {
        Ok(())
    }
}
