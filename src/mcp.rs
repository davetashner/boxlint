use rmcp::{
    handler::server::tool::ToolRouter,
    model::{
        CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt,
};

use crate::RuleRegistry;

#[derive(Clone)]
pub struct BoxlintMcpServer {
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[schemars(description = "Parameters for linting a Unicode box-drawing diagram")]
pub struct LintDiagramParams {
    #[schemars(description = "The diagram text content to lint")]
    pub content: String,

    #[schemars(
        description = "Optional filename to use in diagnostic messages (defaults to '<stdin>')"
    )]
    pub filename: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[schemars(description = "Parameters for fixing a Unicode box-drawing diagram")]
pub struct FixDiagramParams {
    #[schemars(description = "The diagram text content to fix")]
    pub content: String,
}

#[cfg(not(tarpaulin_include))]
impl Default for BoxlintMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(not(tarpaulin_include))]
#[tool_router]
impl BoxlintMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Lint a Unicode box-drawing diagram and return diagnostics")]
    fn lint_diagram(
        &self,
        rmcp::handler::server::wrapper::Parameters(params): rmcp::handler::server::wrapper::Parameters<LintDiagramParams>,
    ) -> Result<CallToolResult, McpError> {
        let registry = RuleRegistry::new();
        let mut diags = registry.run_lint(&params.content);
        let filename = params.filename.unwrap_or_else(|| "<stdin>".to_string());
        for d in &mut diags {
            if d.file.is_empty() {
                d.file.clone_from(&filename);
            }
        }
        let json = serde_json::to_string(&diags).map_err(|e| {
            McpError::internal_error(format!("JSON serialization error: {e}"), None)
        })?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Auto-fix a Unicode box-drawing diagram and return the corrected text")]
    fn fix_diagram(
        &self,
        rmcp::handler::server::wrapper::Parameters(params): rmcp::handler::server::wrapper::Parameters<FixDiagramParams>,
    ) -> Result<CallToolResult, McpError> {
        let registry = RuleRegistry::new();
        let fixed = registry.run_fix(&params.content);
        let changed = fixed != params.content;
        let result = serde_json::json!({
            "changed": changed,
            "fixed_text": fixed,
        });
        let json = serde_json::to_string(&result).map_err(|e| {
            McpError::internal_error(format!("JSON serialization error: {e}"), None)
        })?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

#[cfg(not(tarpaulin_include))]
#[tool_handler]
impl ServerHandler for BoxlintMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "boxlint".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                description: Some(
                    "A linter and auto-fixer for Unicode box-drawing diagrams".to_string(),
                ),
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "boxlint is a linter and auto-fixer for Unicode box-drawing diagrams. \
                 Use lint_diagram to check for issues, and fix_diagram to auto-correct them."
                    .to_string(),
            ),
        }
    }
}

#[cfg(not(tarpaulin_include))]
pub fn install_mcp_config(project: bool) -> Result<(), String> {
    let settings_path = if project {
        let cwd =
            std::env::current_dir().map_err(|e| format!("could not get current directory: {e}"))?;
        cwd.join(".claude").join("settings.local.json")
    } else {
        let home =
            std::env::var("HOME").map_err(|_| "HOME environment variable not set".to_string())?;
        std::path::PathBuf::from(home)
            .join(".claude")
            .join("settings.json")
    };

    install_mcp_config_to(&settings_path)?;
    println!(
        "Installed boxlint MCP server in {}",
        settings_path.display()
    );
    Ok(())
}

/// Install MCP config into a specific settings file path (for testability).
pub fn install_mcp_config_to(settings_path: &std::path::Path) -> Result<(), String> {
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("could not determine boxlint binary path: {e}"))?;
    let exe_str = exe.to_string_lossy().to_string();

    // Read existing file or start with empty object
    let existing = if settings_path.exists() {
        std::fs::read_to_string(settings_path)
            .map_err(|e| format!("could not read {}: {e}", settings_path.display()))?
    } else {
        "{}".to_string()
    };

    let mut root: serde_json::Value = serde_json::from_str(&existing)
        .map_err(|e| format!("could not parse {}: {e}", settings_path.display()))?;

    let obj = root
        .as_object_mut()
        .ok_or_else(|| format!("{} is not a JSON object", settings_path.display()))?;

    if !obj.contains_key("mcpServers") {
        obj.insert(
            "mcpServers".to_string(),
            serde_json::Value::Object(serde_json::Map::new()),
        );
    }
    let mcp_servers = obj
        .get_mut("mcpServers")
        .and_then(|v| v.as_object_mut())
        .ok_or_else(|| {
            format!(
                "mcpServers in {} is not a JSON object",
                settings_path.display()
            )
        })?;

    mcp_servers.insert(
        "boxlint".to_string(),
        serde_json::json!({
            "command": exe_str,
            "args": ["mcp"]
        }),
    );

    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("could not create directory {}: {e}", parent.display()))?;
    }

    let output = serde_json::to_string_pretty(&root)
        .map_err(|e| format!("could not serialize JSON: {e}"))?;
    std::fs::write(settings_path, output)
        .map_err(|e| format!("could not write {}: {e}", settings_path.display()))?;

    Ok(())
}

#[cfg(not(tarpaulin_include))]
pub async fn run_mcp_server() -> Result<(), Box<dyn std::error::Error>> {
    let service = BoxlintMcpServer::new()
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_creates_new_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join(".claude").join("settings.json");

        install_mcp_config_to(&settings_path).unwrap();

        let content = std::fs::read_to_string(&settings_path).unwrap();
        let root: serde_json::Value = serde_json::from_str(&content).unwrap();

        let servers = root.get("mcpServers").unwrap().as_object().unwrap();
        let boxlint = servers.get("boxlint").unwrap();
        assert_eq!(boxlint.get("args").unwrap(), &serde_json::json!(["mcp"]));
        assert!(!boxlint.get("command").unwrap().as_str().unwrap().is_empty());
    }

    #[test]
    fn install_merges_into_existing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");

        // Write existing settings with another key
        let existing = serde_json::json!({
            "apiKey": "test-key",
            "mcpServers": {
                "other-server": {
                    "command": "/usr/bin/other",
                    "args": []
                }
            }
        });
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        install_mcp_config_to(&settings_path).unwrap();

        let content = std::fs::read_to_string(&settings_path).unwrap();
        let root: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Existing keys preserved
        assert_eq!(root.get("apiKey").unwrap().as_str().unwrap(), "test-key");

        let servers = root.get("mcpServers").unwrap().as_object().unwrap();
        // Existing server preserved
        assert!(servers.contains_key("other-server"));
        // boxlint added
        assert!(servers.contains_key("boxlint"));
        assert_eq!(
            servers.get("boxlint").unwrap().get("args").unwrap(),
            &serde_json::json!(["mcp"])
        );
    }
}
