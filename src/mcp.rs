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

impl Default for BoxlintMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

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

pub async fn run_mcp_server() -> Result<(), Box<dyn std::error::Error>> {
    let service = BoxlintMcpServer::new()
        .serve(rmcp::transport::stdio())
        .await?;
    service.waiting().await?;
    Ok(())
}
