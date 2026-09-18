use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const AGENTS_CONFIG_FILE_NAME: &str = "agents.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChoice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptVia {
    #[default]
    Arg,
    Stdin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputVia {
    #[default]
    Stdout,
    File,
}

/// 議事録生成に使うエージェント CLI の定義。`{prompt}` / `{model}` / `{modelArgs}` / `{output}` を
/// 引数テンプレートに書ける。`{modelArgs}` はモデル指定が空のとき丸ごと消える。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDefinition {
    pub id: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub binary: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub model_args: Vec<String>,
    #[serde(default)]
    pub prompt_via: PromptVia,
    #[serde(default)]
    pub output_via: OutputVia,
    #[serde(default)]
    pub models: Vec<ModelChoice>,
    #[serde(default)]
    pub default_model: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub disabled: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentsConfig {
    #[serde(default)]
    pub agents: Vec<AgentDefinition>,
}

fn model(id: &str, name: &str) -> ModelChoice {
    ModelChoice {
        id: id.into(),
        name: name.into(),
    }
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

impl AgentDefinition {
    pub fn builtin() -> Vec<AgentDefinition> {
        vec![
            AgentDefinition {
                id: "claude".into(),
                display_name: "Claude Code".into(),
                binary: "claude".into(),
                args: strings(&["-p", "{modelArgs}", "--output-format", "text", "{prompt}"]),
                model_args: strings(&["--model", "{model}"]),
                prompt_via: PromptVia::Arg,
                output_via: OutputVia::Stdout,
                models: vec![
                    model("sonnet", "Sonnet"),
                    model("opus", "Opus"),
                    model("haiku", "Haiku"),
                ],
                default_model: "sonnet".into(),
                env: BTreeMap::new(),
                disabled: false,
            },
            AgentDefinition {
                id: "codex".into(),
                display_name: "Codex".into(),
                binary: "codex".into(),
                args: strings(&[
                    "exec",
                    "--skip-git-repo-check",
                    "--ephemeral",
                    "{modelArgs}",
                    "-o",
                    "{output}",
                    "-",
                ]),
                model_args: strings(&["--model", "{model}"]),
                prompt_via: PromptVia::Stdin,
                output_via: OutputVia::File,
                models: vec![model("", "デフォルト")],
                default_model: String::new(),
                env: BTreeMap::new(),
                disabled: false,
            },
            AgentDefinition {
                id: "gemini".into(),
                display_name: "Gemini".into(),
                binary: "gemini".into(),
                args: strings(&["-p", "{prompt}", "{modelArgs}"]),
                model_args: strings(&["-m", "{model}"]),
                prompt_via: PromptVia::Arg,
                output_via: OutputVia::Stdout,
                models: vec![
                    model("gemini-2.5-pro", "2.5 Pro"),
                    model("gemini-2.5-flash", "2.5 Flash"),
                ],
                default_model: "gemini-2.5-flash".into(),
                env: BTreeMap::new(),
                disabled: false,
            },
        ]
    }

    pub fn display_name(&self) -> &str {
        if self.display_name.is_empty() {
            &self.id
        } else {
            &self.display_name
        }
    }

    pub fn binary_name(&self) -> &str {
        if self.binary.is_empty() {
            &self.id
        } else {
            &self.binary
        }
    }

    pub fn uses_stdin_prompt(&self) -> bool {
        self.prompt_via == PromptVia::Stdin
    }

    pub fn uses_output_file(&self) -> bool {
        self.output_via == OutputVia::File
    }

    pub fn build_arguments(
        &self,
        model: &str,
        prompt: &str,
        output_file: Option<&Path>,
    ) -> Vec<String> {
        let output = output_file
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let substitute = |arg: &str| {
            arg.replace("{prompt}", prompt)
                .replace("{model}", model)
                .replace("{output}", &output)
        };
        let mut result = Vec::with_capacity(self.args.len() + self.model_args.len());
        for arg in &self.args {
            if arg == "{modelArgs}" {
                if !model.is_empty() {
                    result.extend(self.model_args.iter().map(|a| substitute(a)));
                }
            } else {
                result.push(substitute(arg));
            }
        }
        result
    }
}

/// 組み込み定義にユーザー設定ファイルを重ねる。同じ `id` は上書き、`disabled` は除外。
pub fn merge_agents(user: &[AgentDefinition]) -> Vec<AgentDefinition> {
    let mut merged = AgentDefinition::builtin();
    for def in user {
        match merged.iter_mut().find(|b| b.id == def.id) {
            Some(existing) => *existing = def.clone(),
            None => merged.push(def.clone()),
        }
    }
    merged.retain(|d| !d.disabled && !d.id.is_empty());
    merged
}

pub fn load_agents(path: &Path) -> Result<Vec<AgentDefinition>, String> {
    let user = match std::fs::read_to_string(path) {
        Ok(text) => {
            serde_json::from_str::<AgentsConfig>(&text)
                .map_err(|e| format!("{}: {e}", path.display()))?
                .agents
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    Ok(merge_agents(&user))
}

/// 初回に書き出すひな形。組み込み 3 種をそのまま並べ（編集しなければ挙動は変わらない）、
/// 追加例として無効化した OpenCode を末尾に置く。
pub fn config_template() -> String {
    let mut agents: Vec<serde_json::Value> = AgentDefinition::builtin()
        .iter()
        .map(|d| serde_json::to_value(d).unwrap())
        .collect();
    agents.push(serde_json::json!({
        "_comment": "追加例。disabled を false にすると議事録ダイアログに現れる。{prompt} {model} {modelArgs} {output} が使える",
        "id": "opencode",
        "displayName": "OpenCode",
        "binary": "opencode",
        "args": ["run", "{modelArgs}", "{prompt}"],
        "modelArgs": ["--model", "{model}"],
        "promptVia": "arg",
        "outputVia": "stdout",
        "models": [{ "id": "anthropic/claude-sonnet-4", "name": "Sonnet 4" }],
        "defaultModel": "anthropic/claude-sonnet-4",
        "env": {},
        "disabled": true
    }));
    serde_json::to_string_pretty(&serde_json::json!({ "agents": agents })).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin(id: &str) -> AgentDefinition {
        AgentDefinition::builtin()
            .into_iter()
            .find(|d| d.id == id)
            .unwrap()
    }

    #[test]
    fn codex_args_with_model_and_output() {
        let args = builtin("codex").build_arguments("gpt-5", "prompt", Some(Path::new("/tmp/out.md")));
        assert_eq!(
            args,
            vec![
                "exec",
                "--skip-git-repo-check",
                "--ephemeral",
                "--model",
                "gpt-5",
                "-o",
                "/tmp/out.md",
                "-"
            ]
        );
    }

    #[test]
    fn codex_args_default_model_omitted() {
        let args = builtin("codex").build_arguments("", "prompt", Some(Path::new("/tmp/o.md")));
        assert_eq!(
            args,
            vec!["exec", "--skip-git-repo-check", "--ephemeral", "-o", "/tmp/o.md", "-"]
        );
    }

    #[test]
    fn claude_and_gemini_args_match_previous_hardcoded() {
        assert_eq!(
            builtin("claude").build_arguments("sonnet", "P", None),
            vec!["-p", "--model", "sonnet", "--output-format", "text", "P"]
        );
        assert_eq!(
            builtin("gemini").build_arguments("gemini-2.5-flash", "P", None),
            vec!["-p", "P", "-m", "gemini-2.5-flash"]
        );
    }

    #[test]
    fn placeholder_inside_arg_is_substituted() {
        let def = AgentDefinition {
            args: strings(&["--model={model}", "--prompt={prompt}"]),
            ..builtin("claude")
        };
        assert_eq!(
            def.build_arguments("m", "hello", None),
            vec!["--model=m", "--prompt=hello"]
        );
    }

    #[test]
    fn merge_overrides_by_id_and_appends_new() {
        let user = vec![
            AgentDefinition {
                display_name: "My Claude".into(),
                ..builtin("claude")
            },
            AgentDefinition {
                id: "opencode".into(),
                display_name: String::new(),
                binary: String::new(),
                args: strings(&["run", "{prompt}"]),
                model_args: Vec::new(),
                prompt_via: PromptVia::Arg,
                output_via: OutputVia::Stdout,
                models: Vec::new(),
                default_model: String::new(),
                env: BTreeMap::new(),
                disabled: false,
            },
            AgentDefinition {
                disabled: true,
                ..builtin("gemini")
            },
        ];
        let merged = merge_agents(&user);
        let ids: Vec<&str> = merged.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(ids, vec!["claude", "codex", "opencode"]);
        assert_eq!(merged[0].display_name(), "My Claude");
        assert_eq!(merged[2].display_name(), "opencode");
        assert_eq!(merged[2].binary_name(), "opencode");
    }

    #[test]
    fn load_missing_file_gives_builtin() {
        let agents = load_agents(Path::new("/nonexistent/agents.json")).unwrap();
        assert_eq!(agents, AgentDefinition::builtin());
    }

    #[test]
    fn load_parses_minimal_user_definition() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agents.json");
        std::fs::write(
            &path,
            r#"{"agents":[{"id":"aider","args":["--message","{prompt}"],"promptVia":"arg"}]}"#,
        )
        .unwrap();
        let agents = load_agents(&path).unwrap();
        let aider = agents.iter().find(|d| d.id == "aider").unwrap();
        assert_eq!(aider.build_arguments("", "hi", None), vec!["--message", "hi"]);
        assert_eq!(aider.output_via, OutputVia::Stdout);
    }

    #[test]
    fn load_reports_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agents.json");
        std::fs::write(&path, "{ not json").unwrap();
        let err = load_agents(&path).unwrap_err();
        assert!(err.contains("agents.json"));
    }

    #[test]
    fn template_is_noop_until_edited() {
        let parsed: AgentsConfig = serde_json::from_str(&config_template()).unwrap();
        assert_eq!(parsed.agents.len(), 4);
        assert_eq!(merge_agents(&parsed.agents), AgentDefinition::builtin());
    }
}
