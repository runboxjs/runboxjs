// AI tool definitions and provider-specific serializers.
// Compatible with OpenAI function calling, Anthropic tool use, and Gemini declarations.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub name: String,
    pub content: Value,
    pub error: Option<String>,
}

pub fn all_tools() -> Vec<ToolDef> {
    vec![
        read_file(),
        write_file(),
        list_dir(),
        exec_command(),
        search_code(),
        get_console_logs(),
        reload_sandbox(),
        install_packages(),
        get_file_tree(),
        preview_start(),
        preview_stop(),
        preview_configure(),
        preview_share(),
        debug_error(),
        refactor_code(),
        generate_tests(),
        explain_project(),
        patch_file(),
        fetch_url(),
        scaffold_project(),
        agent_memo(),
        get_agent_journal(),
        search_history(),
    ]
}

pub fn to_openai_format(tools: &[ToolDef]) -> Value {
    json!(
        tools
            .iter()
            .map(|t| json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                }
            }))
            .collect::<Vec<_>>()
    )
}

pub fn to_anthropic_format(tools: &[ToolDef]) -> Value {
    json!(
        tools
            .iter()
            .map(|t| json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            }))
            .collect::<Vec<_>>()
    )
}

pub fn to_gemini_format(tools: &[ToolDef]) -> Value {
    json!([{
        "function_declarations": tools.iter().map(|t| json!({
            "name": t.name,
            "description": t.description,
            "parameters": t.parameters,
        })).collect::<Vec<_>>()
    }])
}

fn read_file() -> ToolDef {
    ToolDef {
        name: "read_file",
        description: "Read a file from the virtual project filesystem.",
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute file path, for example /src/index.ts"
                }
            },
            "required": ["path"]
        }),
    }
}

fn write_file() -> ToolDef {
    ToolDef {
        name: "write_file",
        description: "Create or overwrite a file in the virtual project filesystem.",
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute file path to write"
                },
                "content": {
                    "type": "string",
                    "description": "Full file content"
                }
            },
            "required": ["path", "content"]
        }),
    }
}

fn list_dir() -> ToolDef {
    ToolDef {
        name: "list_dir",
        description: "List files and directories inside a path.",
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path. Defaults to /."
                }
            },
            "required": []
        }),
    }
}

fn exec_command() -> ToolDef {
    ToolDef {
        name: "exec_command",
        description: "Execute a shell command in the sandbox runtime (bun/node/npm/pnpm/yarn/git/python/pip).",
        parameters: json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Command line to execute, for example npm run start"
                }
            },
            "required": ["command"]
        }),
    }
}

fn search_code() -> ToolDef {
    ToolDef {
        name: "search_code",
        description: "Search text across project files with optional directory and extension filters.",
        parameters: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Text pattern to find"
                },
                "path": {
                    "type": "string",
                    "description": "Directory root for the search. Defaults to /."
                },
                "extension": {
                    "type": "string",
                    "description": "Optional extension filter, for example .ts or .py"
                }
            },
            "required": ["query"]
        }),
    }
}

fn get_console_logs() -> ToolDef {
    ToolDef {
        name: "get_console_logs",
        description: "Read console logs from the sandbox. Supports level and incremental filtering.",
        parameters: json!({
            "type": "object",
            "properties": {
                "level": {
                    "type": "string",
                    "enum": ["log", "info", "warn", "error", "debug"],
                    "description": "Optional log level filter"
                },
                "since_id": {
                    "type": "number",
                    "description": "Optional cursor. Return entries with id greater than this value"
                }
            },
            "required": []
        }),
    }
}

fn reload_sandbox() -> ToolDef {
    ToolDef {
        name: "reload_sandbox",
        description: "Request sandbox reload action metadata (hard or soft reload).",
        parameters: json!({
            "type": "object",
            "properties": {
                "hard": {
                    "type": "boolean",
                    "description": "true for full reload, false for soft reload signal"
                }
            },
            "required": []
        }),
    }
}

fn install_packages() -> ToolDef {
    ToolDef {
        name: "install_packages",
        description: "Install project dependencies with auto-detected or explicit package manager.",
        parameters: json!({
            "type": "object",
            "properties": {
                "packages": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Package list. Empty array means install from package.json"
                },
                "dev": {
                    "type": "boolean",
                    "description": "Install as development dependency"
                },
                "manager": {
                    "type": "string",
                    "enum": ["bun", "npm", "pnpm", "yarn"],
                    "description": "Optional package manager override"
                }
            },
            "required": []
        }),
    }
}

fn get_file_tree() -> ToolDef {
    ToolDef {
        name: "get_file_tree",
        description: "Return a recursive JSON tree of files and directories.",
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Root directory path. Defaults to /."
                },
                "depth": {
                    "type": "number",
                    "description": "Maximum recursion depth. Defaults to 5"
                }
            },
            "required": []
        }),
    }
}

fn preview_start() -> ToolDef {
    ToolDef {
        name: "preview_start",
        description: "Start a preview session for the current project. Optionally configure domain, port, metadata, CORS, and live-reload settings.",
        parameters: json!({
            "type": "object",
            "properties": {
                "domain": {
                    "type": "string",
                    "description": "Custom domain for the preview (e.g., 'preview.myapp.com'). Users must point DNS to the RunBox host."
                },
                "port": {
                    "type": "number",
                    "description": "Port number for localhost preview. Default: 3000"
                },
                "base_path": {
                    "type": "string",
                    "description": "Base URL path prefix (e.g., '/app'). Default: '/'"
                },
                "https": {
                    "type": "boolean",
                    "description": "Use HTTPS in generated URLs. Default: false"
                },
                "spa": {
                    "type": "boolean",
                    "description": "Enable SPA mode (serve index.html for non-file routes). Default: true"
                },
                "live_reload": {
                    "type": "boolean",
                    "description": "Inject live-reload script into HTML. Default: true"
                },
                "title": {
                    "type": "string",
                    "description": "Preview title for browser tab and social sharing"
                },
                "description": {
                    "type": "string",
                    "description": "Preview description for social sharing metadata"
                }
            },
            "required": []
        }),
    }
}

fn preview_stop() -> ToolDef {
    ToolDef {
        name: "preview_stop",
        description: "Stop the current preview session.",
        parameters: json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
    }
}

fn preview_configure() -> ToolDef {
    ToolDef {
        name: "preview_configure",
        description: "Update preview configuration: set custom domain, update metadata, configure CORS, or change other settings.",
        parameters: json!({
            "type": "object",
            "properties": {
                "domain": {
                    "type": "string",
                    "description": "Custom domain to set (e.g., 'preview.myapp.com')"
                },
                "title": {
                    "type": "string",
                    "description": "Update preview title"
                },
                "description": {
                    "type": "string",
                    "description": "Update preview description"
                },
                "image": {
                    "type": "string",
                    "description": "URL to preview image for social sharing"
                },
                "favicon": {
                    "type": "string",
                    "description": "URL to favicon"
                },
                "cors_origins": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Allowed CORS origins. Use ['*'] for any origin"
                },
                "spa": {
                    "type": "boolean",
                    "description": "Enable/disable SPA mode"
                },
                "live_reload": {
                    "type": "boolean",
                    "description": "Enable/disable live-reload injection"
                }
            },
            "required": []
        }),
    }
}

fn preview_share() -> ToolDef {
    ToolDef {
        name: "preview_share",
        description: "Generate a shareable URL for the current preview. If a custom domain is set, the URL uses that domain so others can access the project.",
        parameters: json!({
            "type": "object",
            "properties": {},
            "required": []
        }),
    }
}

// ── Phase 5.4 AI Tools Avanzados ─────────────────────────────────────────────

fn debug_error() -> ToolDef {
    ToolDef {
        name: "debug_error",
        description: "Analyze an error message or stack trace, search for context in the codebase, and propose a fix.",
        parameters: json!({
            "type": "object",
            "properties": {
                "error_message": {
                    "type": "string",
                    "description": "The exact error message or stack trace to debug"
                },
                "related_file": {
                    "type": "string",
                    "description": "Optional file path where the error occurred"
                }
            },
            "required": ["error_message"]
        }),
    }
}

fn refactor_code() -> ToolDef {
    ToolDef {
        name: "refactor_code",
        description: "Safely refactor a specified code block or file, providing a unified diff of the required changes.",
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to refactor"
                },
                "instructions": {
                    "type": "string",
                    "description": "Description of the refactoring to perform (e.g., 'extract to a custom React hook')"
                }
            },
            "required": ["path", "instructions"]
        }),
    }
}

fn generate_tests() -> ToolDef {
    ToolDef {
        name: "generate_tests",
        description: "Generate unit tests for a specific file, function, or component.",
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the source file to test"
                },
                "framework": {
                    "type": "string",
                    "enum": ["jest", "vitest", "mocha", "node:test", "deno"],
                    "description": "The test framework to use"
                }
            },
            "required": ["path"]
        }),
    }
}

fn explain_project() -> ToolDef {
    ToolDef {
        name: "explain_project",
        description: "Provide a comprehensive architectural and dependency overview of the current project.",
        parameters: json!({
            "type": "object",
            "properties": {
                "depth": {
                    "type": "number",
                    "description": "Level of detail required. Defaults to 2."
                }
            },
            "required": []
        }),
    }
}

fn patch_file() -> ToolDef {
    ToolDef {
        name: "patch_file",
        description: "Precise file editing using target blocks. Best for small changes in large files to avoid emitting the whole file.",
        parameters: json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to file" },
                "target_content": { "type": "string", "description": "Exact text to find and replace" },
                "replacement_content": { "type": "string", "description": "Text to insert in place of target_content" }
            },
            "required": ["path", "target_content", "replacement_content"]
        }),
    }
}

fn fetch_url() -> ToolDef {
    ToolDef {
        name: "fetch_url",
        description: "Fetch contents from an HTTP URL (GET method) to read documentation or external scripts.",
        parameters: json!({
            "type": "object",
            "properties": {
                "url": { "type": "string", "description": "URL to fetch" }
            },
            "required": ["url"]
        }),
    }
}

fn scaffold_project() -> ToolDef {
    ToolDef {
        name: "scaffold_project",
        description: "Quickly bootstrap an empty directory with a predefined template (e.g. react, node-api, vue).",
        parameters: json!({
            "type": "object",
            "properties": {
                "template": { "type": "string", "enum": ["react", "vite-react-ts", "express", "hono"], "description": "Template ID" },
                "path": { "type": "string", "description": "Target extraction path. Defaults to /." }
            },
            "required": ["template"]
        }),
    }
}

fn agent_memo() -> ToolDef {
    ToolDef {
        name: "agent_memo",
        description: "Record a reasoning note in the agent journal. Use this to explain why you are about to make a change, so future agents or developers can understand the intent behind the action.",
        parameters: json!({
            "type": "object",
            "properties": {
                "tool": {
                    "type": "string",
                    "description": "Name of the tool or action this reasoning is for (e.g. write_file, exec_command)"
                },
                "reason": {
                    "type": "string",
                    "description": "Explanation of why this action is being taken"
                },
                "files_affected": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of file paths that will be affected"
                },
                "result_summary": {
                    "type": "string",
                    "description": "Optional summary of the outcome (can be filled in after the action)"
                }
            },
            "required": ["tool", "reason"]
        }),
    }
}

fn get_agent_journal() -> ToolDef {
    ToolDef {
        name: "get_agent_journal",
        description: "Read the agent journal — a structured log of reasoning entries that explain why changes were made. Use this to understand what a previous agent was doing, or to resume work with full context.",
        parameters: json!({
            "type": "object",
            "properties": {
                "since_id": {
                    "type": "number",
                    "description": "Optional cursor. Return entries with id greater than this value"
                },
                "file": {
                    "type": "string",
                    "description": "Optional file path filter. Return only entries that affected this file"
                }
            },
            "required": []
        }),
    }
}

fn search_history() -> ToolDef {
    ToolDef {
        name: "search_history",
        description: "Search past agent journal entries by text, tool, or file. Returns ranked results with context snippets so you can understand what previous agents did and why.",
        parameters: json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Natural language or keyword search across agent reasoning and results"
                },
                "tool": {
                    "type": "string",
                    "description": "Filter by tool name (write_file, exec_command, patch_file, agent_memo, etc.)"
                },
                "file": {
                    "type": "string",
                    "description": "Filter by affected file path"
                },
                "limit": {
                    "type": "number",
                    "description": "Maximum number of results to return. Defaults to 10."
                }
            },
            "required": []
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_serialize() {
        let tools = all_tools();
        assert!(!tools.is_empty());

        let openai = to_openai_format(&tools);
        assert!(openai.is_array());

        let anthropic = to_anthropic_format(&tools);
        assert!(anthropic.is_array());

        let gemini = to_gemini_format(&tools);
        assert!(gemini.is_array());
    }
}
