use anyhow::Result;
use serde_json::{json, Value};

pub fn openapi_json() -> Value {
    json!({
        "openapi": "3.1.0",
        "info": {
            "title": "StarSync API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Local-first GitHub starred repository mirror, Markdown meta, search, and MCP support."
        },
        "paths": {
            "/health": {
                "get": {
                    "operationId": "getHealth",
                    "responses": {"200": {"description": "Service health"}}
                }
            },
            "/repos": {
                "get": {
                    "operationId": "listRepos",
                    "parameters": list_parameters(),
                    "responses": {"200": {"description": "Fused repository and local meta list"}}
                }
            },
            "/repos/{owner}/{repo}": {
                "get": {
                    "operationId": "getRepo",
                    "parameters": path_repo_parameters(),
                    "responses": {"200": {"description": "Fused repository detail"}, "404": {"description": "Repo not found"}}
                }
            },
            "/repos/{owner}/{repo}/meta": {
                "get": {
                    "operationId": "getRepoMeta",
                    "parameters": path_repo_parameters(),
                    "responses": {"200": {"description": "Local Markdown meta document"}}
                },
                "patch": {
                    "operationId": "updateRepoMeta",
                    "parameters": path_repo_parameters(),
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/MetaPatch"}}}},
                    "responses": {"200": {"description": "Updated local Markdown meta document"}}
                },
                "delete": {
                    "operationId": "deleteRepoMeta",
                    "parameters": path_repo_parameters(),
                    "responses": {"200": {"description": "Archived local Markdown meta document"}}
                }
            },
            "/search": {
                "get": {
                    "operationId": "searchRepos",
                    "parameters": list_parameters(),
                    "responses": {"200": {"description": "Full-text or structured expression search results with snippets and matched fields"}}
                }
            },
            "/sync": {
                "post": {
                    "operationId": "syncStars",
                    "responses": {"202": {"description": "Background sync task accepted", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/BackgroundJobAccepted"}}}}, "409": {"description": "A sync task is already running"}}
                }
            },
            "/enrich/readme": {
                "post": {
                    "operationId": "enrichReadme",
                    "parameters": [{"name": "limit", "in": "query", "schema": {"type": "integer", "minimum": 1}}],
                    "responses": {"202": {"description": "Background README enrichment task accepted", "content": {"application/json": {"schema": {"$ref": "#/components/schemas/BackgroundJobAccepted"}}}}, "409": {"description": "A README enrichment task is already running"}}
                }
            },
            "/events": {
                "get": {
                    "operationId": "streamEvents",
                    "responses": {"200": {"description": "Server-Sent Events stream of EventEnvelope payloads"}}
                }
            },
            "/events/recent": {
                "get": {
                    "operationId": "listRecentEvents",
                    "parameters": [{"name": "limit", "in": "query", "schema": {"type": "integer", "minimum": 1, "maximum": 500}}],
                    "responses": {"200": {"description": "Recent durable event envelopes"}}
                }
            },
            "/event-subscriptions": {
                "get": {
                    "operationId": "listEventSubscriptions",
                    "responses": {"200": {"description": "Configured webhook event subscriptions"}}
                },
                "post": {
                    "operationId": "createEventSubscription",
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/EventSubscriptionCreate"}}}},
                    "responses": {"200": {"description": "Created webhook event subscription"}}
                }
            },
            "/event-subscriptions/{id}": {
                "patch": {
                    "operationId": "updateEventSubscription",
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "requestBody": {"required": true, "content": {"application/json": {"schema": {"$ref": "#/components/schemas/EventSubscriptionPatch"}}}},
                    "responses": {"200": {"description": "Updated webhook event subscription"}, "404": {"description": "Subscription not found"}}
                },
                "delete": {
                    "operationId": "deleteEventSubscription",
                    "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "string"}}],
                    "responses": {"200": {"description": "Deleted webhook event subscription"}, "404": {"description": "Subscription not found"}}
                }
            },
            "/openapi.json": {
                "get": {"operationId": "getOpenApiJson", "responses": {"200": {"description": "OpenAPI JSON"}}}
            },
            "/openapi.yaml": {
                "get": {"operationId": "getOpenApiYaml", "responses": {"200": {"description": "OpenAPI YAML"}}}
            }
        },
        "components": {
            "schemas": {
                "MetaPatch": {
                    "type": "object",
                    "properties": {
                        "tags": {"type": "array", "items": {"type": "string"}},
                        "status": {"anyOf": [{"type": "string"}, {"type": "null"}]},
                        "summary": {"anyOf": [{"type": "string"}, {"type": "null"}]},
                        "notes": {"anyOf": [{"type": "string"}, {"type": "null"}]},
                        "archived": {"type": "boolean"}
                    }
                },
                "BackgroundJobAccepted": {
                    "type": "object",
                    "required": ["job_id", "kind", "accepted", "message"],
                    "properties": {
                        "job_id": {"type": "string"},
                        "kind": {"type": "string", "enum": ["sync", "enrich_readme"]},
                        "accepted": {"type": "boolean"},
                        "message": {"type": "string"}
                    }
                },
                "EventSubscriptionCreate": {
                    "type": "object",
                    "required": ["url"],
                    "properties": {
                        "url": {"type": "string", "format": "uri"},
                        "events": {"type": "array", "items": {"type": "string"}, "description": "Event names such as repo.added, meta.changed, sync.completed, or *."},
                        "enabled": {"type": "boolean", "default": true},
                        "secret": {"type": ["string", "null"], "description": "Optional HMAC secret. Never returned by read APIs."}
                    }
                },
                "EventSubscriptionPatch": {
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "format": "uri"},
                        "events": {"type": "array", "items": {"type": "string"}},
                        "enabled": {"type": "boolean"},
                        "secret": {"anyOf": [{"type": "string"}, {"type": "null"}], "description": "Set a new HMAC secret or null to clear it."}
                    }
                }
            }
        }
    })
}

pub fn openapi_yaml() -> Result<String> {
    Ok(serde_yaml::to_string(&openapi_json())?)
}

fn path_repo_parameters() -> Value {
    json!([
        {"name": "owner", "in": "path", "required": true, "schema": {"type": "string"}},
        {"name": "repo", "in": "path", "required": true, "schema": {"type": "string"}}
    ])
}

fn list_parameters() -> Value {
    json!([
        {"name": "limit", "in": "query", "schema": {"type": "integer", "minimum": 1, "maximum": 200}},
        {"name": "cursor", "in": "query", "schema": {"type": "string"}},
        {"name": "page", "in": "query", "schema": {"type": "integer", "minimum": 1}},
        {"name": "per_page", "in": "query", "schema": {"type": "integer", "minimum": 1, "maximum": 200}},
        {"name": "sort", "in": "query", "description": "Sort field. created means the time you starred the repo; stars means GitHub stargazer count; forks means GitHub fork count.", "schema": {"type": "string", "enum": ["created", "updated", "name", "stars", "forks"]}},
        {"name": "direction", "in": "query", "description": "Sort direction.", "schema": {"type": "string", "enum": ["asc", "desc"]}},
        {"name": "language", "in": "query", "schema": {"type": "string"}},
        {"name": "topic", "in": "query", "schema": {"type": "string"}},
        {"name": "owner", "in": "query", "schema": {"type": "string"}},
        {"name": "tag", "in": "query", "schema": {"type": "string"}},
        {"name": "status", "in": "query", "schema": {"type": "string"}},
        {"name": "archived", "in": "query", "schema": {"type": "boolean"}},
        {"name": "q", "in": "query", "description": "Full-text query or GitHub-style structured expression, for example owner:nickfan AND name:^T", "schema": {"type": "string"}}
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exports_openapi_31() {
        let spec = openapi_json();
        assert_eq!(spec["openapi"], "3.1.0");
        assert!(spec["paths"]["/repos"].is_object());
        assert!(openapi_yaml().unwrap().contains("openapi: 3.1.0"));
    }
}
