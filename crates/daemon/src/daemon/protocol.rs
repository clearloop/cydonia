//! Server trait implementation for the Daemon.

use crate::{daemon::Daemon, ext::hub};
use anyhow::Result;
use compact_str::CompactString;
use futures_util::{StreamExt, pin_mut};
use std::sync::Arc;
use wcore::AgentEvent;
use wcore::protocol::{
    api::Server,
    message::{
        DownloadEvent, DownloadRequest, HubAction, HubEvent, SendRequest, SendResponse,
        StreamEvent, StreamRequest,
    },
};

impl Server for Daemon {
    async fn send(&self, req: SendRequest) -> Result<SendResponse> {
        let rt: Arc<_> = self.runtime.read().await.clone();
        let response = rt.send_to(&req.agent, &req.content).await?;
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

            let rt: Arc<_> = runtime.read().await.clone();
            let stream = rt.stream_to(&agent, &content);
            pin_mut!(stream);
            while let Some(event) = stream.next().await {
                match event {
                    AgentEvent::TextDelta(text) => {
                        yield StreamEvent::Chunk { content: text };
                    }
                    AgentEvent::Done(resp) => {
                        if let wcore::AgentStopReason::Error(e) = &resp.stop_reason {
                            Err(anyhow::anyhow!("{e}"))?;
                        }
                        break;
                    }
                    _ => {}
                }
            }

            yield StreamEvent::End { agent: agent.clone() };
        }
    }

    fn download(
        &self,
        req: DownloadRequest,
    ) -> impl futures_core::Stream<Item = Result<DownloadEvent>> + Send {
        #[cfg(feature = "local")]
        {
            use tokio::sync::mpsc;
            async_stream::try_stream! {
                // Only registry models are supported.
                let entry = model::local::registry::find(&req.model)
                    .ok_or_else(|| anyhow::anyhow!(
                        "model '{}' is not in the registry", req.model
                    ))?;

                if !entry.fits() {
                    let required = entry.memory_requirement();
                    let actual = model::local::system_memory() / (1024 * 1024 * 1024);
                    Err(anyhow::anyhow!(
                        "model '{}' requires at least {} RAM, your system has {}GB",
                        entry.name, required, actual
                    ))?;
                }

                yield DownloadEvent::Start { model: req.model.clone() };

                let (dtx, mut drx) = mpsc::unbounded_channel();
                let model_str = req.model.to_string();
                let download_handle = tokio::spawn(async move {
                    model::local::download::download_model(&model_str, dtx).await
                });

                while let Some(event) = drx.recv().await {
                    let dl_event = match event {
                        model::local::download::DownloadEvent::FileStart { filename, size } => {
                            DownloadEvent::FileStart { model: req.model.clone(), filename, size }
                        }
                        model::local::download::DownloadEvent::Progress { bytes } => {
                            DownloadEvent::Progress { model: req.model.clone(), bytes }
                        }
                        model::local::download::DownloadEvent::FileEnd { filename } => {
                            DownloadEvent::FileEnd { model: req.model.clone(), filename }
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
        #[cfg(not(feature = "local"))]
        {
            let _ = req;
            async_stream::stream! {
                yield Err(anyhow::anyhow!("this daemon was built without local model support"));
            }
        }
    }

    async fn ping(&self) -> Result<()> {
        Ok(())
    }

    fn hub(
        &self,
        package: CompactString,
        action: HubAction,
    ) -> impl futures_core::Stream<Item = Result<HubEvent>> + Send {
        async_stream::try_stream! {
            match action {
                HubAction::Install => {
                    let s = hub::install(package);
                    pin_mut!(s);
                    while let Some(event) = s.next().await {
                        yield event?;
                    }
                }
                HubAction::Uninstall => {
                    let s = hub::uninstall(package);
                    pin_mut!(s);
                    while let Some(event) = s.next().await {
                        yield event?;
                    }
                }
            }
        }
    }
}
