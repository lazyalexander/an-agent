//! MCP transport reuses the official SDK; authority remains in the host bridge.
use super::Bridge;
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, model::*, service::RequestContext};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, ReadBuf};

// The SDK may drain handlers after a transport closes. Signal host cancellation
// at EOF itself, before waiting for those handlers to settle their native work.
struct Input {
    stdin: tokio::io::Stdin,
    bridge: Arc<Bridge>,
}
impl AsyncRead for Input {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let remaining = buf.remaining();
        let result = Pin::new(&mut self.stdin).poll_read(cx, buf);
        if matches!(&result, Poll::Ready(Err(_)))
            || (remaining > 0
                && matches!(&result, Poll::Ready(Ok(())))
                && buf.filled().len() == before)
        {
            self.bridge.shutdown();
        }
        result
    }
}

#[derive(Clone)]
struct Server(Arc<Bridge>);

fn tools() -> Vec<Tool> {
    [
        (
            "read_source",
            "Read the one selected source; no arbitrary paths.",
            vec!["invocation"],
        ),
        (
            "propose_source",
            "Submit candidate source; never compile or execute it.",
            vec!["invocation", "text"],
        ),
        (
            "run_kit",
            "Run a granted kit with data; no commands, paths or grants.",
            vec!["invocation", "kit", "text"],
        ),
    ]
    .into_iter()
    .map(|(name, description, required)| {
        let mut properties = serde_json::Map::new();
        for key in &required {
            properties.insert((*key).into(), json!({"type":"string"}));
        }
        Tool::new(
            name,
            description,
            json!({"type":"object", "properties":properties,
            "required":required,"additionalProperties":false})
            .as_object()
            .cloned()
            .unwrap_or_default(),
        )
    })
    .collect()
}

impl ServerHandler for Server {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult {
            tools: tools(),
            ..Default::default()
        })
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let stop = Arc::new(AtomicBool::new(false));
        struct Cancel(Arc<AtomicBool>);
        impl Drop for Cancel {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let _cancel = Cancel(stop.clone());
        let bridge = self.0.clone();
        let name = request.name.to_string();
        let args = Value::Object(request.arguments.unwrap_or_default());
        let worker_stop = stop.clone();
        let mut worker = tokio::task::spawn_blocking(move || {
            bridge
                .invoke(&name, args, worker_stop)
                .map_err(|e| e.to_string())
        });
        let result = tokio::select! {
            result=&mut worker=>result,
            _=context.ct.cancelled()=>{stop.store(true,Ordering::SeqCst);worker.await}
        }
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let value = result.unwrap_or_else(|e| json!({"status":"refused-or-uncertain","error":e}));
        let mut response = CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
        response.is_error = Some(value["status"] == "refused-or-uncertain");
        response.structured_content = Some(value);
        Ok(response.into())
    }
}

pub(super) async fn serve(bridge: Arc<Bridge>) -> crate::execution::Result<()> {
    let service = Server(bridge.clone())
        .serve((
            Input {
                stdin: tokio::io::stdin(),
                bridge: bridge.clone(),
            },
            tokio::io::stdout(),
        ))
        .await?;
    let cancel = service.cancellation_token();
    #[cfg(unix)]
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let signal = async {
        #[cfg(unix)]
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
        #[cfg(not(unix))]
        let _ = tokio::signal::ctrl_c().await;
    };
    tokio::pin!(signal);
    tokio::select! {
        result=service.waiting()=>{result?;},
        _=&mut signal=>{bridge.shutdown();cancel.cancel();}
    }
    bridge.shutdown();
    // The supervisor's five-second bound governs active native work; wait for
    // its receipt rather than exiting and abandoning the blocking worker.
    loop {
        if bridge
            .active
            .lock()
            .map_err(|_| "poisoned bridge state")?
            .is_none()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    Ok(())
}
