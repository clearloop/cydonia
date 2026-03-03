//! Server trait implementation for the Gateway.

use crate::gateway::Gateway;
use crate::gateway::dispatch;
use memory::Memory;
use protocol::api::Server;
use protocol::error::ProtocolError;
use protocol::message::{
    AgentDetail, AgentInfoRequest, AgentList, AgentSummary, ClearSessionRequest, DownloadEvent,
    DownloadRequest, GetMemoryRequest, McpAddRequest, McpAdded, McpReloaded, McpRemoveRequest,
    McpRemoved, McpServerList, McpServerSummary, MemoryEntry, MemoryList, SendRequest,
    SendResponse, SessionCleared, SkillsReloaded, StreamEvent, StreamRequest,
};
use tokio::sync::mpsc;

impl Server for Gateway {
    async fn send(&self, req: SendRequest) -> Result<SendResponse, ProtocolError> {
        let content =
            dispatch::dispatch_send(&self.runtime, &self.locks, &req.agent, &req.content).await?;
        Ok(SendResponse {
            agent: req.agent,
            content,
        })
    }

    fn stream(
        &self,
        req: StreamRequest,
    ) -> impl futures_util::Stream<Item = Result<StreamEvent, ProtocolError>> + Send {
        dispatch::dispatch_stream(
            self.runtime.clone(),
            self.locks.clone(),
            req.agent,
            req.content,
        )
    }

    async fn clear_session(
        &self,
        req: ClearSessionRequest,
    ) -> Result<SessionCleared, ProtocolError> {
        self.runtime.clear_session(&req.agent).await;
        Ok(SessionCleared { agent: req.agent })
    }

    async fn list_agents(&self) -> Result<AgentList, ProtocolError> {
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

    async fn agent_info(&self, req: AgentInfoRequest) -> Result<AgentDetail, ProtocolError> {
        match self.runtime.agent(&req.agent).await {
            Some(a) => Ok(AgentDetail {
                name: a.name.clone(),
                description: a.description.clone(),
                tools: a.tools.to_vec(),
                skill_tags: a.skill_tags.to_vec(),
                system_prompt: a.system_prompt.clone(),
            }),
            None => Err(ProtocolError::new(
                404,
                format!("agent not found: {}", req.agent),
            )),
        }
    }

    async fn list_memory(&self) -> Result<MemoryList, ProtocolError> {
        let entries = self.runtime.hook().memory().entries();
        Ok(MemoryList { entries })
    }

    async fn get_memory(&self, req: GetMemoryRequest) -> Result<MemoryEntry, ProtocolError> {
        let value = self.runtime.hook().memory().get(&req.key);
        Ok(MemoryEntry {
            key: req.key,
            value,
        })
    }

    fn download(
        &self,
        req: DownloadRequest,
    ) -> impl futures_util::Stream<Item = Result<DownloadEvent, ProtocolError>> + Send {
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
                    Err(ProtocolError::new(500, format!("download failed: {e}")))?;
                }
                Err(e) => {
                    Err(ProtocolError::new(500, format!("download task panicked: {e}")))?;
                }
            }
        }
    }

    async fn reload_skills(&self) -> Result<SkillsReloaded, ProtocolError> {
        match self.runtime.hook().skills().reload().await {
            Ok(count) => {
                tracing::info!("reloaded {count} skill(s)");
                Ok(SkillsReloaded { count })
            }
            Err(e) => Err(ProtocolError::new(
                500,
                format!("failed to reload skills: {e}"),
            )),
        }
    }

    async fn mcp_add(&self, req: McpAddRequest) -> Result<McpAdded, ProtocolError> {
        let config = mcp::McpServerConfig {
            name: req.name.clone(),
            command: req.command,
            args: req.args,
            env: req.env,
            auto_restart: true,
        };
        match self.runtime.hook().mcp().add(config).await {
            Ok(tools) => Ok(McpAdded {
                name: req.name,
                tools,
            }),
            Err(e) => Err(ProtocolError::new(
                500,
                format!("failed to add MCP server: {e}"),
            )),
        }
    }

    async fn mcp_remove(&self, req: McpRemoveRequest) -> Result<McpRemoved, ProtocolError> {
        match self.runtime.hook().mcp().remove(&req.name).await {
            Ok(tools) => Ok(McpRemoved {
                name: req.name,
                tools,
            }),
            Err(e) => Err(ProtocolError::new(
                500,
                format!("failed to remove MCP server: {e}"),
            )),
        }
    }

    async fn mcp_reload(&self) -> Result<McpReloaded, ProtocolError> {
        match self
            .runtime
            .hook()
            .mcp()
            .reload(|path| {
                let config = crate::DaemonConfig::load(path)?;
                Ok(config.mcp_servers)
            })
            .await
        {
            Ok(servers) => {
                let servers = servers
                    .into_iter()
                    .map(|(name, tools)| McpServerSummary { name, tools })
                    .collect();
                Ok(McpReloaded { servers })
            }
            Err(e) => Err(ProtocolError::new(500, format!("MCP reload failed: {e}"))),
        }
    }

    async fn mcp_list(&self) -> Result<McpServerList, ProtocolError> {
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

    async fn ping(&self) -> Result<(), ProtocolError> {
        Ok(())
    }
}
