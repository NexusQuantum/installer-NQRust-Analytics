#[derive(Debug, Clone, PartialEq)]
pub enum FocusState {
    Field(usize),
    SaveButton,
    CancelButton,
}

// Field indices:
//   0 = host
//   1 = ui_port
//   2 = provider api_key
//   3 = openai_api_key (only if needs_openai_embedding)

#[derive(Debug, Clone)]
pub struct FormData {
    pub(crate) host: String,
    pub(crate) ui_port: String,
    pub(crate) api_key: String,
    pub(crate) openai_api_key: String, // For embedding models that use OpenAI
    pub(crate) selected_provider: String,
    pub(crate) focus_state: FocusState,
    pub(crate) error_message: String,
}

impl FormData {
    pub fn new() -> Self {
        Self {
            host: String::new(),
            ui_port: "3000".to_string(),
            api_key: String::new(),
            openai_api_key: String::new(),
            selected_provider: String::new(),
            focus_state: FocusState::Field(0),
            error_message: String::new(),
        }
    }

    pub fn validate(&mut self) -> bool {
        if self.host.trim().is_empty() {
            self.error_message = "Host is required! (e.g. 192.168.1.100 or localhost)".to_string();
            return false;
        }

        if self.ui_port.trim().is_empty() {
            self.error_message = "UI Port is required! (default: 3000)".to_string();
            return false;
        }

        // Local services don't need API key
        if self.selected_provider == "lm_studio" || self.selected_provider == "ollama" {
            self.error_message.clear();
            return true;
        }

        if self.api_key.trim().is_empty() {
            self.error_message = format!("{} API Key is required!", self.get_api_key_name());
            return false;
        }

        // Check if provider needs OpenAI API key for embedding
        if self.needs_openai_embedding() && self.openai_api_key.trim().is_empty() {
            self.error_message = "OpenAI API Key is required for embedding model!".to_string();
            return false;
        }

        self.error_message.clear();
        true
    }

    pub fn get_current_value_mut(&mut self) -> &mut String {
        match &self.focus_state {
            FocusState::Field(idx) => match idx {
                0 => &mut self.host,
                1 => &mut self.ui_port,
                2 => &mut self.api_key,
                3 => &mut self.openai_api_key,
                _ => &mut self.api_key,
            },
            _ => &mut self.api_key,
        }
    }

    pub fn nextauth_url(&self) -> String {
        let host = if self.host.trim().is_empty() {
            "localhost"
        } else {
            self.host.trim()
        };
        let port = if self.ui_port.trim().is_empty() {
            "3000"
        } else {
            self.ui_port.trim()
        };
        format!("http://{}:{}", host, port)
    }

    pub fn needs_openai_embedding(&self) -> bool {
        // Providers that use OpenAI embedding model (text-embedding-3-large)
        matches!(
            self.selected_provider.as_str(),
            "deepseek" | "anthropic" | "groq" | "grok" | "zhipu" | "qwen3"
        )
    }

    pub fn get_total_fields(&self) -> usize {
        if self.needs_openai_embedding() {
            4 // host + ui_port + provider api_key + openai api_key
        } else {
            3 // host + ui_port + provider api_key
        }
    }

    pub fn get_api_key_name(&self) -> &str {
        match self.selected_provider.as_str() {
            "anthropic" => "Anthropic",
            "openai" => "OpenAI",
            "deepseek" => "DeepSeek",
            "azure" => "Azure OpenAI",
            "bedrock" => "AWS",
            "google_ai_studio" => "Google AI Studio",
            "google_vertexai" => "Google Vertex AI",
            "grok" => "xAI Grok",
            "groq" => "Groq",
            "lm_studio" => "LM Studio (Local - No API Key)",
            "ollama" => "Ollama (Local - No API Key)",
            "open_router" => "OpenRouter",
            "qwen3" => "Qwen",
            "zhipu" => "Zhipu",
            _ => "API",
        }
    }

    pub fn get_env_key_name(&self) -> &str {
        match self.selected_provider.as_str() {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai" => "OPENAI_API_KEY",
            "deepseek" => "DEEPSEEK_API_KEY",
            "azure" => "AZURE_OPENAI_API_KEY",
            "google_ai_studio" => "GEMINI_API_KEY",
            "google_vertexai" => "GOOGLE_APPLICATION_CREDENTIALS",
            "grok" => "XAI_API_KEY",
            "groq" => "GROQ_API_KEY",
            "lm_studio" => "LM_STUDIO_API_KEY",
            "ollama" => "", // No API key needed
            "open_router" => "OPENROUTER_API_KEY",
            "qwen3" => "OPENROUTER_API_KEY",
            "zhipu" => "ZHIPU_API_KEY",
            "local_llm" => "OPENAI_API_KEY", // Local LLM uses OPENAI_API_KEY with dummy value
            _ => "API_KEY",
        }
    }
}
