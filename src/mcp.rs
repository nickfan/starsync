use crate::{
    models::{
        EventSubscriptionCreate, EventSubscriptionPatch, MetaPatch, RepoFilters, RepoIdentity,
    },
    service::StarSyncService,
};
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

pub async fn run_stdio(service: StarSyncService) -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let mut lines = BufReader::new(stdin).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                write_response(
                    &mut stdout,
                    error_response(Value::Null, -32700, error.to_string()),
                )
                .await?;
                continue;
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        if id.is_null() && method.starts_with("notifications/") {
            continue;
        }
        let response = match handle_request(
            &service,
            method,
            request.get("params").cloned().unwrap_or(Value::Null),
        )
        .await
        {
            Ok(result) => success_response(id, result),
            Err(error) => error_response(id, -32000, error.to_string()),
        };
        write_response(&mut stdout, response).await?;
    }
    Ok(())
}

async fn handle_request(service: &StarSyncService, method: &str, params: Value) -> Result<Value> {
    match method {
        "initialize" => Ok(json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {
                "tools": {},
                "resources": {},
                "prompts": {}
            },
            "serverInfo": {
                "name": "starsync",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        "tools/list" => Ok(json!({ "tools": tools() })),
        "tools/call" => call_tool(service, params).await,
        "resources/list" => Ok(json!({
            "resources": [
                {
                    "uri": "starsync://index",
                    "name": "StarSync repository index",
                    "mimeType": "application/json"
                }
            ]
        })),
        "resources/read" => read_resource(service, params),
        "prompts/list" => Ok(json!({ "prompts": prompts() })),
        "prompts/get" => get_prompt(params),
        other => Err(anyhow!("unsupported MCP method: {other}")),
    }
}

async fn call_tool(service: &StarSyncService, params: Value) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("tools/call requires name"))?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let value = match name {
        "search_repos" => {
            let filters = filters_from_arguments(arguments)?;
            json!(service.search_repos(filters)?)
        }
        "list_repos" => {
            let filters = filters_from_arguments(arguments)?;
            json!(service.list_repos(filters)?)
        }
        "get_repo" => {
            let identity = identity_from_arguments(&arguments)?;
            json!(service.get_repo(&identity)?)
        }
        "update_repo_meta" => {
            let identity = identity_from_arguments(&arguments)?;
            let patch: MetaPatch = serde_json::from_value(
                arguments
                    .get("patch")
                    .cloned()
                    .ok_or_else(|| anyhow!("update_repo_meta requires patch"))?,
            )?;
            json!(service.patch_meta(&identity, patch)?)
        }
        "sync_stars" => json!(service.sync().await?),
        "enrich_readme" => {
            let limit = arguments
                .get("limit")
                .and_then(Value::as_u64)
                .map(|value| value as usize);
            json!({ "updated": service.enrich_readmes(limit).await? })
        }
        "enrich_lists" => json!(service.enrich_lists().await?),
        "list_recent_events" => {
            let limit = arguments
                .get("limit")
                .and_then(Value::as_u64)
                .map(|value| value as usize)
                .unwrap_or(50);
            json!(service.recent_events(limit)?)
        }
        "list_event_subscriptions" => json!(service.list_event_subscriptions()),
        "create_event_subscription" => {
            let create: EventSubscriptionCreate = serde_json::from_value(arguments)?;
            json!(service.create_event_subscription(create)?)
        }
        "update_event_subscription" => {
            let id = arguments
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("update_event_subscription requires id"))?;
            let patch: EventSubscriptionPatch = serde_json::from_value(
                arguments
                    .get("patch")
                    .cloned()
                    .ok_or_else(|| anyhow!("update_event_subscription requires patch"))?,
            )?;
            json!(service.patch_event_subscription(id, patch)?)
        }
        "delete_event_subscription" => {
            let id = arguments
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| anyhow!("delete_event_subscription requires id"))?;
            json!(service.delete_event_subscription(id)?)
        }
        other => return Err(anyhow!("unknown tool: {other}")),
    };
    Ok(json!({
        "content": [
            {
                "type": "text",
                "text": serde_json::to_string_pretty(&value)?
            }
        ]
    }))
}

fn read_resource(service: &StarSyncService, params: Value) -> Result<Value> {
    let uri = params
        .get("uri")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("resources/read requires uri"))?;
    let value = if uri == "starsync://index" {
        json!(service.list_repos(RepoFilters::default())?)
    } else if let Some(rest) = uri.strip_prefix("starsync://repo/") {
        let (owner, repo) = rest.split_once('/').ok_or_else(|| {
            anyhow!("repo resource URI must be starsync://repo/{{owner}}/{{repo}}")
        })?;
        json!(service.get_repo(&RepoIdentity::new(owner, repo))?)
    } else {
        return Err(anyhow!("unknown resource URI: {uri}"));
    };
    Ok(json!({
        "contents": [
            {
                "uri": uri,
                "mimeType": "application/json",
                "text": serde_json::to_string_pretty(&value)?
            }
        ]
    }))
}

fn get_prompt(params: Value) -> Result<Value> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("prompts/get requires name"))?;
    let text = match name {
        "find_repos_by_topic" => {
            "Use search_repos and list_repos to find starred repositories by topic, tag, language, and local notes. Prefer local meta before broad remote assumptions."
        }
        "summarize_starred_library" => {
            "Use list_repos with pagination, group by language/topic/tag/status, and summarize the user's local StarSync knowledge base."
        }
        "maintain_repo_meta" => {
            "Read starsync://repo/{owner}/{repo}, suggest concise tags/status/summary, then call update_repo_meta. Never star or unstar GitHub repositories."
        }
        other => return Err(anyhow!("unknown prompt: {other}")),
    };
    Ok(json!({
        "description": text,
        "messages": [
            {
                "role": "user",
                "content": { "type": "text", "text": text }
            }
        ]
    }))
}

fn filters_from_arguments(arguments: Value) -> Result<RepoFilters> {
    Ok(serde_json::from_value(arguments)?)
}

fn identity_from_arguments(arguments: &Value) -> Result<RepoIdentity> {
    let owner = arguments
        .get("owner")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("owner is required"))?;
    let repo = arguments
        .get("repo")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("repo is required"))?;
    Ok(RepoIdentity::new(owner, repo))
}

fn tools() -> Value {
    json!([
        tool(
            "search_repos",
            "Full-text search over GitHub repo facts, README snippets, and local Markdown meta."
        ),
        tool(
            "list_repos",
            "Browse fused starred repositories and local meta with pagination and filters."
        ),
        tool(
            "get_repo",
            "Read one fused repo record by owner and repo name."
        ),
        tool(
            "update_repo_meta",
            "Update local Markdown meta only; this never writes GitHub star state."
        ),
        tool(
            "sync_stars",
            "Pull the authenticated user's GitHub starred repository list into the local mirror."
        ),
        tool(
            "enrich_readme",
            "Refresh README text snippets for current starred repositories."
        ),
        tool(
            "enrich_lists",
            "Import GitHub Star Lists via official GraphQL and join them to the local starred mirror."
        ),
        tool(
            "list_recent_events",
            "Return recent durable StarSync events from the local event log."
        ),
        tool(
            "list_event_subscriptions",
            "List configured webhook event subscriptions."
        ),
        tool(
            "create_event_subscription",
            "Create a webhook subscription for event names such as repo.added, meta.changed, or *."
        ),
        tool(
            "update_event_subscription",
            "Patch a webhook subscription by id."
        ),
        tool(
            "delete_event_subscription",
            "Delete a webhook subscription by id."
        )
    ])
}

fn tool(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "additionalProperties": true
        }
    })
}

fn prompts() -> Value {
    json!([
        {"name": "find_repos_by_topic", "description": "Find useful repos by topic or local tag."},
        {"name": "summarize_starred_library", "description": "Summarize the local starred repository knowledge base."},
        {"name": "maintain_repo_meta", "description": "Suggest and update local meta for a repo."}
    ])
}

fn success_response(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: String) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

async fn write_response(stdout: &mut tokio::io::Stdout, value: Value) -> Result<()> {
    stdout
        .write_all(serde_json::to_string(&value)?.as_bytes())
        .await?;
    stdout.write_all(b"\n").await?;
    stdout.flush().await?;
    Ok(())
}
