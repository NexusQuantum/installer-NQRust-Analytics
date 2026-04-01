use color_eyre::{Result, eyre::eyre};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame};
use reqwest::Client;
use serde::Deserialize;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Stdio;
use std::{env, fs};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::templates::{self, ConfigTemplate};
use crate::ui::{
    self, ConfigSelectionView, ConfirmationView, EnvSetupView, ErrorView, InstallingView,
    KeycloakConfigView, LocalLlmConfigView, RegistrySetupView, SuccessView, UpdateListView,
};
use crate::utils;

pub mod form_data;
pub mod keycloak_form_data;
pub mod local_llm_form_data;
pub mod registry_form;
pub mod state;
mod updates;

pub use form_data::FormData;
pub use keycloak_form_data::KeycloakFormData;
pub use local_llm_form_data::LocalLlmFormData;
use registry_form::RegistryForm;
pub use state::{AppState, MenuSelection};
pub use updates::UpdateInfo;
use updates::{collect_update_infos, get_local_image_created};

enum UpdateListAction {
    Pull,
    Refresh,
    Back,
}

enum RegistryAction {
    Submit,
    Skip,
}

#[derive(Debug)]
pub struct App {
    running: bool,
    pub(crate) state: AppState,
    logs: Vec<String>,
    progress: f64,
    current_service: String,
    total_services: usize,
    completed_services: usize,
    pub(crate) env_exists: bool,
    pub(crate) config_exists: bool,
    pub(crate) form_data: FormData,
    pub(crate) keycloak_form_data: KeycloakFormData,
    pub(crate) local_llm_form_data: LocalLlmFormData,
    pub(crate) menu_selection: MenuSelection,
    config_selection_index: usize,
    update_infos: Vec<UpdateInfo>,
    update_selection_index: usize,
    update_message: Option<String>,
    registry_form: RegistryForm,
    registry_status: Option<String>,
    ghcr_token: Option<String>,
    /// Temporarily store selected template key before generating config.yaml
    selected_template_key: Option<String>,
    /// True when running as nqrust-analytics-airgapped (offline mode, no image pull)
    pub(crate) airgapped: bool,
}

impl App {
    pub fn new() -> Self {
        let env_exists = utils::find_file(".env");
        let config_exists = utils::find_file("config.yaml");

        let token_from_env = env::var("GHCR_TOKEN")
            .or_else(|_| env::var("GITHUB_TOKEN"))
            .or_else(|_| env::var("GH_TOKEN"))
            .ok();
        let token_from_disk = App::load_token_from_disk();
        let initial_token = token_from_env.clone().or(token_from_disk.clone());

        let mut registry_form = RegistryForm::new();
        if let Some(token) = initial_token.clone() {
            registry_form.token = token;
        }

        let airgapped = crate::airgapped::is_airgapped_binary().unwrap_or(false);

        // Skip registry setup if token already available, go straight to confirmation
        let initial_state = if initial_token.is_some() || airgapped {
            AppState::Confirmation
        } else {
            AppState::RegistrySetup
        };

        let mut app = Self {
            running: true,
            state: initial_state,
            logs: Vec::new(),
            progress: 0.0,
            current_service: String::new(),
            total_services: 4,
            completed_services: 0,
            env_exists,
            config_exists,
            form_data: FormData::new(),
            keycloak_form_data: KeycloakFormData::new(),
            local_llm_form_data: LocalLlmFormData::new(),
            menu_selection: MenuSelection::Proceed,
            config_selection_index: 0,
            update_infos: Vec::new(),
            update_selection_index: 0,
            update_message: None,
            registry_form,
            registry_status: None,
            ghcr_token: initial_token,
            selected_template_key: None,
            airgapped,
        };

        app.ensure_menu_selection();
        app
    }

    pub async fn run(mut self, mut terminal: DefaultTerminal) -> Result<()> {
        while self.running {
            terminal.draw(|frame| self.render(frame))?;

            match &self.state {
                AppState::RegistrySetup => {
                    if let Some(action) = self.handle_registry_setup_events()? {
                        match action {
                            RegistryAction::Submit => match self.try_registry_login().await {
                                Ok(true) => {
                                    self.state = AppState::Confirmation;
                                    self.ensure_menu_selection();
                                }
                                Ok(false) => {}
                                Err(e) => {
                                    self.registry_status =
                                        Some(format!("Failed to run docker login: {}", e));
                                }
                            },
                            RegistryAction::Skip => {
                                self.registry_status = Some(
                                    "Skipped GHCR login; you can authenticate later from the menu."
                                        .to_string(),
                                );
                                self.state = AppState::Confirmation;
                                self.ensure_menu_selection();
                            }
                        }
                    }
                }
                AppState::Confirmation => {
                    if let Some(action) = self.handle_confirmation_events()? {
                        match action {
                            MenuSelection::Proceed => {
                                if self.env_exists && self.config_exists {
                                    self.state = AppState::Installing;
                                    self.logs
                                        .push("🚀 Starting Analytics installation...".to_string());

                                    let result = self.run_docker_compose(&mut terminal).await;

                                    match result {
                                        Ok(_) => {
                                            self.state = AppState::Success;
                                            self.progress = 100.0;
                                        }
                                        Err(e) => {
                                            self.state = AppState::Error(format!(
                                                "Installation failed: {}",
                                                e
                                            ));
                                        }
                                    }
                                }
                            }
                            MenuSelection::GenerateEnv => {
                                // Pastikan config sudah dipilih
                                if !self.config_exists {
                                    // Should not happen, but safety check - go to config selection
                                    if templates::CONFIG_TEMPLATES.is_empty() {
                                        self.state = AppState::Error(
                                            "No configuration templates available".to_string(),
                                        );
                                    } else {
                                        self.config_selection_index = 0;
                                        self.state = AppState::ConfigSelection;
                                    }
                                } else if self.form_data.selected_provider.is_empty() {
                                    // Provider belum dipilih - go to config selection first
                                    if templates::CONFIG_TEMPLATES.is_empty() {
                                        self.state = AppState::Error(
                                            "No configuration templates available".to_string(),
                                        );
                                    } else {
                                        self.config_selection_index = 0;
                                        self.state = AppState::ConfigSelection;
                                    }
                                } else {
                                    self.state = AppState::EnvSetup;
                                }
                            }
                            MenuSelection::GenerateConfig => {
                                if templates::CONFIG_TEMPLATES.is_empty() {
                                    self.state = AppState::Error(
                                        "No configuration templates available".to_string(),
                                    );
                                } else {
                                    self.config_selection_index = 0;
                                    self.state = AppState::ConfigSelection;
                                }
                            }
                            MenuSelection::CheckUpdates => {
                                if self.ghcr_token.is_none() {
                                    self.registry_status = Some(
                                        "Authentication required to check for updates.".to_string(),
                                    );
                                    self.state = AppState::RegistrySetup;
                                    self.registry_form.focus_state =
                                        crate::app::registry_form::FocusState::Field(0);
                                } else {
                                    match self.load_updates().await {
                                        Ok(_) => {
                                            self.state = AppState::UpdateList;
                                            self.ensure_update_selection();
                                        }
                                        Err(e) => {
                                            self.state = AppState::Error(format!(
                                                "Failed to check updates: {}",
                                                e
                                            ));
                                        }
                                    }
                                }
                            }
                            MenuSelection::UpdateToken => {
                                self.registry_status = Some(
                                    "Update token and submit (Ctrl+S). Esc to cancel.".to_string(),
                                );
                                self.registry_form.focus_state =
                                    crate::app::registry_form::FocusState::Field(0);
                                self.registry_form.error_message.clear();
                                self.registry_form.token =
                                    self.ghcr_token.clone().unwrap_or_default();
                                self.state = AppState::RegistrySetup;
                            }
                            MenuSelection::ConfigureKeycloak => {
                                self.keycloak_form_data.focus_state =
                                    crate::app::keycloak_form_data::FocusState::EnableToggle;
                                self.keycloak_form_data.error_message.clear();
                                self.state = AppState::KeycloakConfig;
                            }
                            MenuSelection::Cancel => {
                                self.running = false;
                            }
                        }
                    }
                }
                AppState::EnvSetup => {
                    if let Some(proceed) = self.handle_form_events()? {
                        if proceed {
                            // Generate config.yaml first using stored template key
                            if let Some(template_key) = &self.selected_template_key {
                                if let Some(template) = crate::templates::CONFIG_TEMPLATES
                                    .iter()
                                    .find(|t| t.key == template_key.as_str())
                                {
                                    if let Err(e) = self.write_config_yaml(template) {
                                        self.state = AppState::Error(format!(
                                            "Failed to generate config.yaml: {}",
                                            e
                                        ));
                                        return Ok(());
                                    }
                                    self.config_exists = true;
                                }
                            }

                            // Then generate .env file
                            if let Err(e) = self.generate_env_file() {
                                self.state =
                                    AppState::Error(format!("Failed to generate .env: {}", e));
                            } else {
                                self.env_exists = true;
                                self.state = AppState::Confirmation;
                                if !self.config_exists {
                                    self.menu_selection = MenuSelection::GenerateConfig;
                                } else {
                                    self.menu_selection = MenuSelection::Proceed;
                                }
                            }
                        } else {
                            // User canceled - reset selected provider and template key to force re-selection
                            self.form_data.selected_provider.clear();
                            self.selected_template_key = None;
                            self.state = AppState::Confirmation;
                        }
                    }
                }
                AppState::ConfigSelection => {
                    self.handle_config_selection_events()?;
                }
                AppState::KeycloakConfig => {
                    if let Some(saved) = self.handle_keycloak_config_events()? {
                        if saved {
                            // Persist Keycloak config into .env if it already exists
                            if self.env_exists {
                                if let Err(e) = self.apply_keycloak_to_env() {
                                    self.state = AppState::Error(format!(
                                        "Failed to update .env with Keycloak settings: {}",
                                        e
                                    ));
                                    return Ok(());
                                }
                            }
                        }
                        self.state = AppState::Confirmation;
                        self.menu_selection = if self.env_exists && self.config_exists {
                            MenuSelection::Proceed
                        } else {
                            MenuSelection::GenerateConfig
                        };
                    }
                }
                AppState::LocalLlmConfig => {
                    if let Some(proceed) = self.handle_local_llm_config_events()? {
                        if proceed {
                            if let Err(e) = self.generate_local_llm_config() {
                                self.state = AppState::Error(format!(
                                    "Failed to generate config.yaml: {}",
                                    e
                                ));
                            } else {
                                self.config_exists = true;
                                // Set provider to local_llm for env generation
                                self.form_data.selected_provider = "local_llm".to_string();
                                self.form_data.api_key.clear();
                                self.form_data.openai_api_key.clear();
                                self.form_data.focus_state =
                                    crate::app::form_data::FocusState::Field(0);
                                self.form_data.error_message.clear();

                                // Auto-generate .env file for Local LLM
                                if let Err(e) = self.generate_env_file() {
                                    self.state =
                                        AppState::Error(format!("Failed to generate .env: {}", e));
                                } else {
                                    self.env_exists = true;
                                    self.state = AppState::Confirmation;
                                    self.menu_selection = MenuSelection::Proceed;
                                }
                            }
                        } else {
                            // User canceled - reset selection and go back to confirmation
                            self.form_data.selected_provider.clear();
                            self.selected_template_key = None;
                            self.state = AppState::Confirmation;
                        }
                    }
                }
                AppState::UpdateList => {
                    if let Some(action) = self.handle_update_list_events()? {
                        match action {
                            UpdateListAction::Pull => {
                                self.state = AppState::UpdatePulling;
                                if let Err(e) = self.pull_selected_update(&mut terminal).await {
                                    self.state =
                                        AppState::Error(format!("Failed to pull image: {}", e));
                                } else {
                                    self.state = AppState::UpdateList;
                                    self.update_message = Some(
                                        "Image refreshed. Press R to fetch remote metadata again."
                                            .to_string(),
                                    );
                                }
                            }
                            UpdateListAction::Refresh => {
                                if let Err(e) = self.load_updates().await {
                                    self.state = AppState::Error(format!(
                                        "Failed to refresh updates: {}",
                                        e
                                    ));
                                }
                            }
                            UpdateListAction::Back => {
                                self.state = AppState::Confirmation;
                                self.ensure_menu_selection();
                            }
                        }
                    }
                }
                AppState::UpdatePulling => {
                    if event::poll(std::time::Duration::from_millis(100))? {
                        if let Event::Key(key) = event::read()? {
                            if key.kind == KeyEventKind::Press {
                                if let KeyCode::Char('c') = key.code {
                                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                                        self.running = false;
                                    }
                                }
                            }
                        }
                    }
                }
                AppState::Installing => {
                    if event::poll(std::time::Duration::from_millis(100))? {
                        if let Event::Key(key) = event::read()? {
                            if key.kind == KeyEventKind::Press {
                                if let KeyCode::Char('c') = key.code {
                                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                                        self.running = false;
                                    }
                                }
                            }
                        }
                    }
                }
                AppState::Success | AppState::Error(_) => {
                    if event::poll(std::time::Duration::from_millis(100))? {
                        if let Event::Key(key) = event::read()? {
                            if key.kind == KeyEventKind::Press {
                                if let KeyCode::Char('c') = key.code {
                                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                                        self.running = false;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn menu_options(&self) -> Vec<MenuSelection> {
        let mut options = Vec::new();

        if !self.config_exists {
            options.push(MenuSelection::GenerateConfig);
        }

        if self.config_exists && !self.env_exists {
            options.push(MenuSelection::GenerateEnv);
        }

        // Token and Check for updates require network; skip in airgapped/offline mode
        if !self.airgapped {
            if self.ghcr_token.is_some() {
                options.push(MenuSelection::UpdateToken);
            }
            options.push(MenuSelection::CheckUpdates);
        }

        if self.env_exists && self.config_exists {
            options.push(MenuSelection::Proceed);
            options.push(MenuSelection::ConfigureKeycloak);
        }

        options.push(MenuSelection::Cancel);
        options
    }

    fn ensure_menu_selection(&mut self) {
        let options = self.menu_options();

        if !options.contains(&self.menu_selection) {
            if let Some(first) = options.first() {
                self.menu_selection = first.clone();
            }
        }
    }

    fn ensure_update_selection(&mut self) {
        if self.update_selection_index >= self.update_infos.len() {
            self.update_selection_index = self.update_infos.len().saturating_sub(1);
        }
    }

    fn token_file_path() -> PathBuf {
        utils::project_root().join(".ghcr_token")
    }

    fn load_token_from_disk() -> Option<String> {
        fs::read_to_string(Self::token_file_path())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn persist_token(&self, token: &str) -> Result<()> {
        let path = Self::token_file_path();
        fs::write(&path, token)?;
        #[cfg(unix)]
        {
            let perms = std::fs::Permissions::from_mode(0o600);
            fs::set_permissions(&path, perms)?;
        }
        Ok(())
    }

    fn handle_registry_setup_events(&mut self) -> Result<Option<RegistryAction>> {
        use crate::app::registry_form::FocusState;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Up => {
                            self.registry_form.focus_state = match &self.registry_form.focus_state {
                                FocusState::Field(_) => FocusState::Field(0), // Only one field
                                FocusState::SaveButton => FocusState::Field(0),
                                FocusState::CancelButton => FocusState::SaveButton,
                            };
                        }
                        KeyCode::Down => {
                            self.registry_form.focus_state = match &self.registry_form.focus_state {
                                FocusState::Field(_) => FocusState::SaveButton,
                                FocusState::SaveButton => FocusState::CancelButton,
                                FocusState::CancelButton => FocusState::CancelButton,
                            };
                        }
                        KeyCode::Tab => {
                            self.registry_form.focus_state = match &self.registry_form.focus_state {
                                FocusState::Field(_) => FocusState::SaveButton,
                                FocusState::SaveButton => FocusState::CancelButton,
                                FocusState::CancelButton => FocusState::Field(0),
                            };
                        }
                        KeyCode::BackTab => {
                            self.registry_form.focus_state = match &self.registry_form.focus_state {
                                FocusState::Field(_) => FocusState::CancelButton,
                                FocusState::SaveButton => FocusState::Field(0),
                                FocusState::CancelButton => FocusState::SaveButton,
                            };
                        }
                        KeyCode::Enter => match &self.registry_form.focus_state {
                            FocusState::SaveButton => {
                                return Ok(Some(RegistryAction::Submit));
                            }
                            FocusState::CancelButton => {
                                return Ok(Some(RegistryAction::Skip));
                            }
                            _ => {}
                        },
                        KeyCode::Esc => {
                            return Ok(Some(RegistryAction::Skip));
                        }
                        KeyCode::Char(c) => {
                            if !key.modifiers.contains(KeyModifiers::CONTROL) {
                                if matches!(&self.registry_form.focus_state, FocusState::Field(_)) {
                                    self.registry_form.get_current_value_mut().push(c);
                                }
                            } else if c == 'c' {
                                self.running = false;
                            }
                        }
                        KeyCode::Backspace => {
                            if matches!(&self.registry_form.focus_state, FocusState::Field(_)) {
                                self.registry_form.get_current_value_mut().pop();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(None)
    }

    async fn try_registry_login(&mut self) -> Result<bool> {
        if !self.registry_form.validate() {
            self.registry_status = Some(self.registry_form.error_message.clone());
            return Ok(false);
        }

        let token = self.registry_form.token.trim().to_string();

        if token.is_empty() {
            self.registry_status = Some("Token is required".to_string());
            return Ok(false);
        }

        self.registry_status = Some("Resolving GitHub username from token...".to_string());

        let username = match self.fetch_github_username(&token).await {
            Ok(name) => name,
            Err(e) => {
                self.registry_status = Some(format!("Failed to resolve username: {}", e));
                return Ok(false);
            }
        };

        self.registry_status = Some("Logging in to ghcr.io...".to_string());
        self.add_log(&format!(
            "🔐 Executing: docker login ghcr.io as {}",
            username
        ));

        let mut child = Command::new("docker")
            .args(["login", "ghcr.io", "-u", &username, "--password-stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(format!("{}\n", token).as_bytes()).await?;
        } else {
            self.registry_status = Some("Failed to communicate with docker login".to_string());
            return Ok(false);
        }

        let output = child.wait_with_output().await?;

        if output.status.success() {
            self.registry_status = Some("Authenticated with ghcr.io successfully".to_string());
            self.ghcr_token = Some(token.clone());
            self.registry_form.error_message.clear();
            // Persist so users don't have to paste again
            if let Err(e) = self.persist_token(&token) {
                self.registry_status = Some(format!(
                    "Authenticated, but failed to cache token locally: {}",
                    e
                ));
            }
            Ok(true)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let first_line = stderr.lines().find(|line| !line.trim().is_empty());
            self.registry_status = Some(format!(
                "Docker login failed: {}",
                first_line.unwrap_or("unknown error")
            ));
            Ok(false)
        }
    }

    async fn fetch_github_username(&self, token: &str) -> Result<String> {
        #[derive(Deserialize)]
        struct GitHubUser {
            login: String,
        }

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()?;

        let response = client
            .get("https://api.github.com/user")
            .header("User-Agent", "nqrust-analytics")
            .header("Accept", "application/vnd.github+json")
            .bearer_auth(token)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(eyre!(
                "GitHub API returned {} when fetching user info: {}",
                status,
                body
            ));
        }

        let user: GitHubUser = response.json().await?;
        Ok(user.login)
    }

    async fn load_updates(&mut self) -> Result<()> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()?;

        self.logs.clear();
        self.progress = 0.0;

        let token = if let Some(token) = self.ghcr_token.clone() {
            Some(token)
        } else {
            let env_token = env::var("GHCR_TOKEN")
                .or_else(|_| env::var("GITHUB_TOKEN"))
                .or_else(|_| env::var("GH_TOKEN"))
                .ok();
            if let Some(token) = env_token.clone() {
                self.ghcr_token = Some(token.clone());
                Some(token)
            } else if let Some(token) = App::load_token_from_disk() {
                self.ghcr_token = Some(token.clone());
                Some(token)
            } else {
                None
            }
        };

        self.update_infos = collect_update_infos(&client, token.as_deref()).await?;
        self.ensure_update_selection();

        if self.update_infos.is_empty() {
            self.update_message =
                Some("No GHCR-backed services were found in docker-compose.yaml".to_string());
        } else {
            self.update_message = Some(
                "Use ↑/↓ to pick a service, Enter or P to pull :latest, R to refresh, Esc to go back"
                    .to_string(),
            );
        }

        Ok(())
    }

    fn redraw(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        terminal.draw(|frame| self.render(frame))?;
        Ok(())
    }

    fn add_log_and_redraw(&mut self, terminal: &mut DefaultTerminal, message: &str) {
        self.add_log(message);
        let _ = self.redraw(terminal);
    }

    async fn pull_selected_update(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        if self.update_infos.is_empty() {
            return Ok(());
        }

        // Reset progress for pull/self-update flows
        self.progress = 0.0;

        let index = self.update_selection_index.min(self.update_infos.len() - 1);
        let info = self.update_infos[index].clone();

        if info.is_self {
            return self.self_update(info, terminal).await;
        }

        let reference = info.pull_reference();
        let image = info.image.clone();
        let tag = info.current_tag.clone();

        self.logs.clear();
        self.add_log_and_redraw(
            terminal,
            &format!("⬇️  Executing: docker pull {}", reference),
        );

        let mut child = Command::new("docker")
            .args(["pull", &reference])
            .env("DOCKER_CLI_PROGRESS", "plain")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| eyre!("Failed to capture docker pull stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| eyre!("Failed to capture docker pull stderr"))?;

        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        loop {
            tokio::select! {
                output = stdout_reader.next_line() => {
                    match output {
                        Ok(Some(line)) => {
                            self.add_log_and_redraw(terminal, &format!("ℹ️  {}", line));
                        }
                        Ok(None) => break,
                        Err(e) => {
                            self.add_log_and_redraw(terminal, &format!("❌ Error reading stdout: {}", e));
                            break;
                        }
                    }
                }
                output = stderr_reader.next_line() => {
                    match output {
                        Ok(Some(line)) => {
                            self.add_log_and_redraw(terminal, &format!("⚠️  {}", line));
                        }
                        Ok(None) => break,
                        Err(e) => {
                            self.add_log_and_redraw(terminal, &format!("❌ Error reading stderr: {}", e));
                            break;
                        }
                    }
                }
            }
        }

        let status = child.wait().await?;

        if !status.success() {
            return Err(eyre!("docker pull exited with a non-zero status"));
        }

        self.add_log_and_redraw(terminal, "✅ Image pulled successfully");

        match get_local_image_created(&image, &tag).await {
            Ok(created) => {
                if let Some(info) = self.update_infos.get_mut(index) {
                    info.clear_local_error();
                    info.apply_local_created(created);
                }
            }
            Err(e) => {
                if let Some(info) = self.update_infos.get_mut(index) {
                    info.append_status(&format!("Failed to inspect local image: {}", e));
                    info.apply_local_created(None);
                }
            }
        }

        Ok(())
    }

    async fn self_update(
        &mut self,
        info: UpdateInfo,
        terminal: &mut DefaultTerminal,
    ) -> Result<()> {
        let download_url = info
            .download_url
            .clone()
            .ok_or_else(|| eyre!("No download URL available for installer update"))?;

        let version_label = info
            .latest_release_tag
            .clone()
            .unwrap_or_else(|| "latest".to_string());

        let checksum_url = info.checksum_url.clone();

        self.logs.clear();
        self.add_log_and_redraw(
            terminal,
            &format!("⬇️  Downloading installer {}", version_label),
        );
        self.progress = 0.0;

        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?;

        let mut response = client
            .get(&download_url)
            .header("User-Agent", "nqrust-analytics")
            .send()
            .await?
            .error_for_status()?;

        let total = response.content_length();
        let mut downloaded: u64 = 0;
        let mut last_logged: u64 = 0;
        let mut deb_bytes: Vec<u8> = Vec::new();

        while let Some(chunk) = response.chunk().await? {
            downloaded += chunk.len() as u64;
            deb_bytes.extend_from_slice(&chunk);

            if let Some(total) = total {
                let pct = ((downloaded * 100) / total).min(100);
                if pct >= last_logged + 5 || downloaded == total {
                    self.add_log_and_redraw(terminal, &format!("⬇️  Downloading... {}%", pct));
                    self.progress = pct as f64;
                    let _ = self.redraw(terminal);
                    last_logged = pct;
                }
            } else {
                // No content-length; log every ~5 MB
                let mb = downloaded / (1024 * 1024);
                if mb >= last_logged + 5 {
                    self.add_log_and_redraw(terminal, &format!("⬇️  Downloaded {} MB", mb));
                    last_logged = mb;
                }
            }
        }

        if let Some(total) = total {
            let pct = ((downloaded * 100) / total).min(100);
            self.add_log_and_redraw(terminal, &format!("⬇️  Download complete ({}%)", pct));
            self.progress = pct as f64;
        } else {
            self.add_log_and_redraw(terminal, "⬇️  Download complete");
        }

        let deb_path = env::temp_dir().join(format!("nqrust-analytics-{}.deb", version_label));
        fs::write(&deb_path, &deb_bytes)?;

        if let Some(sum_url) = checksum_url {
            self.add_log_and_redraw(terminal, "🔍 Verifying checksum");

            let sums = client
                .get(&sum_url)
                .header("User-Agent", "nqrust-analytics")
                .send()
                .await?
                .error_for_status()?;

            let sums_bytes = sums.bytes().await?;
            let sums_path = env::temp_dir().join("nqrust-analytics-SHA256SUMS");
            fs::write(&sums_path, &sums_bytes)?;

            let expected = fs::read_to_string(&sums_path).ok().and_then(|content| {
                content.lines().find_map(|line| {
                    let mut parts = line.split_whitespace();
                    let hash = parts.next()?;
                    let name = parts.next()?;
                    if name.ends_with(
                        deb_path
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default()
                            .as_str(),
                    ) {
                        Some(hash.to_string())
                    } else {
                        None
                    }
                })
            });

            if let Some(expected_hash) = expected {
                let output = Command::new("sha256sum").arg(&deb_path).output().await?;

                if !output.status.success() {
                    return Err(eyre!("Failed to run sha256sum on downloaded package"));
                }

                let actual = String::from_utf8_lossy(&output.stdout)
                    .split_whitespace()
                    .next()
                    .map(|s| s.to_string())
                    .ok_or_else(|| eyre!("Unable to parse sha256sum output"))?;

                if actual != expected_hash {
                    return Err(eyre!("Checksum mismatch for downloaded installer"));
                }

                self.add_log_and_redraw(terminal, "✅ Checksum verified");
            } else {
                self.add_log_and_redraw(
                    terminal,
                    "⚠️  Could not find matching entry in SHA256SUMS; skipping checksum check",
                );
            }
        }

        self.add_log_and_redraw(
            terminal,
            &format!("📦 Executing: sudo dpkg -i {}", deb_path.display()),
        );

        let deb_arg = deb_path.to_string_lossy().to_string();

        let status = Command::new("sudo")
            .args(["dpkg", "-i", &deb_arg])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let output = status.wait_with_output().await?;

        if !output.status.success() {
            self.add_log_and_redraw(
                terminal,
                &format!(
                    "❌ dpkg failed: {}",
                    String::from_utf8_lossy(&output.stderr)
                ),
            );
            return Err(eyre!("dpkg -i failed"));
        }

        self.add_log_and_redraw(
            terminal,
            "✅ Installer updated. Restart this program to use the new version.",
        );
        self.progress = 100.0;
        let _ = self.redraw(terminal);

        Ok(())
    }

    fn handle_update_list_events(&mut self) -> Result<Option<UpdateListAction>> {
        self.ensure_update_selection();

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Up => {
                            if !self.update_infos.is_empty() {
                                if self.update_selection_index == 0 {
                                    self.update_selection_index = self.update_infos.len() - 1;
                                } else {
                                    self.update_selection_index -= 1;
                                }
                            }
                        }
                        KeyCode::Down | KeyCode::Tab => {
                            if !self.update_infos.is_empty() {
                                self.update_selection_index =
                                    (self.update_selection_index + 1) % self.update_infos.len();
                            }
                        }
                        KeyCode::Enter => {
                            if !self.update_infos.is_empty() {
                                return Ok(Some(UpdateListAction::Pull));
                            }
                        }
                        KeyCode::Char('p') | KeyCode::Char('P') => {
                            if !self.update_infos.is_empty() {
                                return Ok(Some(UpdateListAction::Pull));
                            }
                        }
                        KeyCode::Char('r') | KeyCode::Char('R') => {
                            return Ok(Some(UpdateListAction::Refresh));
                        }
                        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                            return Ok(Some(UpdateListAction::Back));
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.running = false;
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(None)
    }

    fn handle_confirmation_events(&mut self) -> Result<Option<MenuSelection>> {
        self.ensure_menu_selection();

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let options = self.menu_options();
                    if options.is_empty() {
                        return Ok(None);
                    }

                    let mut index = options
                        .iter()
                        .position(|option| option == &self.menu_selection)
                        .unwrap_or(0);

                    match key.code {
                        KeyCode::Up => {
                            if index == 0 {
                                index = options.len() - 1;
                            } else {
                                index -= 1;
                            }
                            self.menu_selection = options[index].clone();
                        }
                        KeyCode::Down | KeyCode::Tab => {
                            index = (index + 1) % options.len();
                            self.menu_selection = options[index].clone();
                        }
                        KeyCode::Enter => {
                            return Ok(Some(self.menu_selection.clone()));
                        }
                        KeyCode::Esc | KeyCode::Char('q') => {
                            return Ok(Some(MenuSelection::Cancel));
                        }
                        KeyCode::Char('c') => {
                            if key.modifiers.contains(KeyModifiers::CONTROL) {
                                return Ok(Some(MenuSelection::Cancel));
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(None)
    }

    async fn detect_compose_command(&self) -> Result<Vec<String>> {
        // Prefer the integrated Docker CLI plugin first
        let docker_compose = Command::new("docker")
            .args(["compose", "version"])
            .output()
            .await;

        if let Ok(output) = docker_compose {
            if output.status.success() {
                return Ok(vec!["docker".to_string(), "compose".to_string()]);
            }
        }

        // Fallback to standalone docker-compose
        let standalone = Command::new("docker-compose").arg("version").output().await;

        if let Ok(output) = standalone {
            if output.status.success() {
                return Ok(vec!["docker-compose".to_string()]);
            }
        }

        Err(eyre!(
            "Could not find Docker Compose. Tried `docker compose` and `docker-compose`. Install Docker Compose v2 or the standalone docker-compose."
        ))
    }

    fn handle_form_events(&mut self) -> Result<Option<bool>> {
        use crate::app::form_data::FocusState;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let total_fields = self.form_data.get_total_fields();

                    match key.code {
                        KeyCode::Up => {
                            self.form_data.focus_state = match &self.form_data.focus_state {
                                FocusState::Field(idx) if *idx > 0 => FocusState::Field(idx - 1),
                                FocusState::SaveButton => FocusState::Field(total_fields - 1),
                                FocusState::CancelButton => FocusState::SaveButton,
                                _ => self.form_data.focus_state.clone(),
                            };
                        }
                        KeyCode::Down => {
                            self.form_data.focus_state = match &self.form_data.focus_state {
                                FocusState::Field(idx) if *idx < total_fields - 1 => {
                                    FocusState::Field(idx + 1)
                                }
                                FocusState::Field(_) => FocusState::SaveButton,
                                FocusState::SaveButton => FocusState::CancelButton,
                                _ => self.form_data.focus_state.clone(),
                            };
                        }
                        KeyCode::Tab => {
                            self.form_data.focus_state = match &self.form_data.focus_state {
                                FocusState::Field(idx) if *idx < total_fields - 1 => {
                                    FocusState::Field(idx + 1)
                                }
                                FocusState::Field(_) => FocusState::SaveButton,
                                FocusState::SaveButton => FocusState::CancelButton,
                                FocusState::CancelButton => FocusState::Field(0),
                            };
                        }
                        KeyCode::BackTab => {
                            self.form_data.focus_state = match &self.form_data.focus_state {
                                FocusState::Field(idx) if *idx > 0 => FocusState::Field(idx - 1),
                                FocusState::Field(_) => FocusState::CancelButton,
                                FocusState::SaveButton => FocusState::Field(total_fields - 1),
                                FocusState::CancelButton => FocusState::SaveButton,
                            };
                        }
                        KeyCode::Enter => match &self.form_data.focus_state {
                            FocusState::SaveButton => {
                                if self.form_data.validate() {
                                    return Ok(Some(true));
                                }
                            }
                            FocusState::CancelButton => {
                                return Ok(Some(false));
                            }
                            _ => {}
                        },
                        KeyCode::Esc => {
                            return Ok(Some(false));
                        }
                        KeyCode::Char(c) => {
                            if !key.modifiers.contains(KeyModifiers::CONTROL) {
                                if matches!(&self.form_data.focus_state, FocusState::Field(_)) {
                                    self.form_data.get_current_value_mut().push(c);
                                }
                            } else if c == 'c' {
                                self.running = false;
                            }
                        }
                        KeyCode::Backspace => {
                            if matches!(&self.form_data.focus_state, FocusState::Field(_)) {
                                self.form_data.get_current_value_mut().pop();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(None)
    }

    fn apply_keycloak_to_env(&self) -> Result<()> {
        let project_root = utils::project_root();
        let env_path = project_root.join(".env");
        let content = fs::read_to_string(&env_path)?;
        let kc = &self.keycloak_form_data;

        let replacements: &[(&str, String)] = &[
            (
                "KEYCLOAK_OAUTH_ENABLED=",
                format!(
                    "KEYCLOAK_OAUTH_ENABLED={}",
                    if kc.enabled { "true" } else { "false" }
                ),
            ),
            (
                "KEYCLOAK_PUBLIC_URL=",
                format!("KEYCLOAK_PUBLIC_URL={}", kc.public_url.trim()),
            ),
            (
                "KEYCLOAK_URL=",
                format!("KEYCLOAK_URL={}", kc.keycloak_url()),
            ),
            (
                "KEYCLOAK_REALM=",
                format!(
                    "KEYCLOAK_REALM={}",
                    if kc.realm.trim().is_empty() {
                        "master".to_string()
                    } else {
                        kc.realm.trim().to_string()
                    }
                ),
            ),
            (
                "KEYCLOAK_CLIENT_ID=",
                format!("KEYCLOAK_CLIENT_ID={}", kc.client_id.trim()),
            ),
            (
                "KEYCLOAK_CLIENT_SECRET=",
                format!("KEYCLOAK_CLIENT_SECRET={}", kc.client_secret.trim()),
            ),
            (
                "KEYCLOAK_DEFAULT_ROLE=",
                format!("KEYCLOAK_DEFAULT_ROLE={}", kc.default_role()),
            ),
            (
                "KEYCLOAK_AUTO_REGISTER=",
                format!(
                    "KEYCLOAK_AUTO_REGISTER={}",
                    if kc.auto_register { "true" } else { "false" }
                ),
            ),
            (
                "NEXTAUTH_URL=",
                format!("NEXTAUTH_URL={}", self.form_data.nextauth_url()),
            ),
        ];

        let updated = content
            .lines()
            .map(|line| {
                for (prefix, replacement) in replacements {
                    if line.starts_with(prefix) {
                        return replacement.clone();
                    }
                }
                line.to_string()
            })
            .collect::<Vec<_>>()
            .join("\n");

        // Preserve trailing newline if original had one
        let updated = if content.ends_with('\n') {
            format!("{}\n", updated)
        } else {
            updated
        };
        fs::write(&env_path, updated)?;
        Ok(())
    }

    fn handle_keycloak_config_events(&mut self) -> Result<Option<bool>> {
        use crate::app::keycloak_form_data::FocusState;

        if !event::poll(std::time::Duration::from_millis(100))? {
            return Ok(None);
        }

        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                return Ok(None);
            }

            match key.code {
                // Save
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if self.keycloak_form_data.validate() {
                        return Ok(Some(true));
                    }
                }
                KeyCode::Enter => match &self.keycloak_form_data.focus_state {
                    FocusState::SaveButton => {
                        if self.keycloak_form_data.validate() {
                            return Ok(Some(true));
                        }
                    }
                    FocusState::CancelButton => {
                        return Ok(Some(false));
                    }
                    FocusState::EnableToggle => {
                        self.keycloak_form_data.enabled = !self.keycloak_form_data.enabled;
                    }
                    FocusState::AutoRegisterToggle => {
                        self.keycloak_form_data.auto_register =
                            !self.keycloak_form_data.auto_register;
                    }
                    FocusState::Field(_) | FocusState::RoleSelect => {}
                },
                // Toggle booleans with space
                KeyCode::Char(' ') => match &self.keycloak_form_data.focus_state {
                    FocusState::EnableToggle => {
                        self.keycloak_form_data.enabled = !self.keycloak_form_data.enabled;
                    }
                    FocusState::AutoRegisterToggle => {
                        self.keycloak_form_data.auto_register =
                            !self.keycloak_form_data.auto_register;
                    }
                    _ => {}
                },
                // Role select: left/right
                KeyCode::Left => {
                    if matches!(self.keycloak_form_data.focus_state, FocusState::RoleSelect) {
                        let len = crate::app::keycloak_form_data::ROLES.len();
                        self.keycloak_form_data.default_role_index =
                            (self.keycloak_form_data.default_role_index + len - 1) % len;
                    }
                }
                KeyCode::Right => {
                    if matches!(self.keycloak_form_data.focus_state, FocusState::RoleSelect) {
                        let len = crate::app::keycloak_form_data::ROLES.len();
                        self.keycloak_form_data.default_role_index =
                            (self.keycloak_form_data.default_role_index + 1) % len;
                    }
                }
                // Text input for active field
                KeyCode::Char(c) => {
                    if let Some(val) = self.keycloak_form_data.get_current_value_mut() {
                        val.push(c);
                        self.keycloak_form_data.error_message.clear();
                    }
                }
                KeyCode::Backspace => {
                    if let Some(val) = self.keycloak_form_data.get_current_value_mut() {
                        val.pop();
                    }
                }
                // Navigation: Tab / Down / Up
                KeyCode::Tab | KeyCode::Down => {
                    let enabled = self.keycloak_form_data.enabled;
                    self.keycloak_form_data.focus_state = match &self.keycloak_form_data.focus_state
                    {
                        FocusState::EnableToggle => {
                            if enabled {
                                FocusState::Field(0)
                            } else {
                                FocusState::SaveButton
                            }
                        }
                        FocusState::Field(i) => {
                            let next = i + 1;
                            if next < KeycloakFormData::total_text_fields() {
                                FocusState::Field(next)
                            } else {
                                FocusState::RoleSelect
                            }
                        }
                        FocusState::RoleSelect => FocusState::AutoRegisterToggle,
                        FocusState::AutoRegisterToggle => FocusState::SaveButton,
                        FocusState::SaveButton => FocusState::CancelButton,
                        FocusState::CancelButton => FocusState::EnableToggle,
                    };
                }
                KeyCode::BackTab | KeyCode::Up => {
                    let enabled = self.keycloak_form_data.enabled;
                    self.keycloak_form_data.focus_state = match &self.keycloak_form_data.focus_state
                    {
                        FocusState::EnableToggle => FocusState::CancelButton,
                        FocusState::Field(0) => FocusState::EnableToggle,
                        FocusState::Field(i) => FocusState::Field(i - 1),
                        FocusState::RoleSelect => {
                            FocusState::Field(KeycloakFormData::total_text_fields() - 1)
                        }
                        FocusState::AutoRegisterToggle => FocusState::RoleSelect,
                        FocusState::SaveButton => {
                            if enabled {
                                FocusState::AutoRegisterToggle
                            } else {
                                FocusState::EnableToggle
                            }
                        }
                        FocusState::CancelButton => FocusState::SaveButton,
                    };
                }
                KeyCode::Esc => {
                    return Ok(Some(false));
                }
                _ => {}
            }
        }
        Ok(None)
    }

    fn handle_local_llm_config_events(&mut self) -> Result<Option<bool>> {
        use crate::app::local_llm_form_data::FocusState;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    let total_fields = self.local_llm_form_data.get_total_fields();

                    match key.code {
                        KeyCode::Up => {
                            self.local_llm_form_data.focus_state = match &self
                                .local_llm_form_data
                                .focus_state
                            {
                                FocusState::Field(idx) if *idx > 0 => FocusState::Field(idx - 1),
                                FocusState::SaveButton => FocusState::Field(total_fields - 1),
                                FocusState::CancelButton => FocusState::SaveButton,
                                _ => self.local_llm_form_data.focus_state.clone(),
                            };
                        }
                        KeyCode::Down => {
                            self.local_llm_form_data.focus_state =
                                match &self.local_llm_form_data.focus_state {
                                    FocusState::Field(idx) if *idx < total_fields - 1 => {
                                        FocusState::Field(idx + 1)
                                    }
                                    FocusState::Field(_) => FocusState::SaveButton,
                                    FocusState::SaveButton => FocusState::CancelButton,
                                    _ => self.local_llm_form_data.focus_state.clone(),
                                };
                        }
                        KeyCode::Tab => {
                            self.local_llm_form_data.focus_state =
                                match &self.local_llm_form_data.focus_state {
                                    FocusState::Field(idx) if *idx < total_fields - 1 => {
                                        FocusState::Field(idx + 1)
                                    }
                                    FocusState::Field(_) => FocusState::SaveButton,
                                    FocusState::SaveButton => FocusState::CancelButton,
                                    FocusState::CancelButton => FocusState::Field(0),
                                };
                        }
                        KeyCode::BackTab => {
                            self.local_llm_form_data.focus_state = match &self
                                .local_llm_form_data
                                .focus_state
                            {
                                FocusState::Field(idx) if *idx > 0 => FocusState::Field(idx - 1),
                                FocusState::Field(_) => FocusState::CancelButton,
                                FocusState::SaveButton => FocusState::Field(total_fields - 1),
                                FocusState::CancelButton => FocusState::SaveButton,
                            };
                        }
                        KeyCode::Enter => match &self.local_llm_form_data.focus_state {
                            FocusState::SaveButton => {
                                if self.local_llm_form_data.validate() {
                                    return Ok(Some(true));
                                }
                            }
                            FocusState::CancelButton => {
                                return Ok(Some(false));
                            }
                            _ => {}
                        },
                        KeyCode::Esc => {
                            return Ok(Some(false));
                        }
                        KeyCode::Char(c) => {
                            if !key.modifiers.contains(KeyModifiers::CONTROL) {
                                if matches!(
                                    &self.local_llm_form_data.focus_state,
                                    FocusState::Field(_)
                                ) {
                                    self.local_llm_form_data.get_current_value_mut().push(c);
                                }
                            } else if c == 'c' {
                                self.running = false;
                            }
                        }
                        KeyCode::Backspace => {
                            if matches!(&self.local_llm_form_data.focus_state, FocusState::Field(_))
                            {
                                self.local_llm_form_data.get_current_value_mut().pop();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        Ok(None)
    }

    fn handle_config_selection_events(&mut self) -> Result<()> {
        let total = templates::CONFIG_TEMPLATES.len();

        if total == 0 {
            self.state = AppState::Error("No configuration templates available".to_string());
            return Ok(());
        }

        if self.config_selection_index >= total {
            self.config_selection_index = total - 1;
        }

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Up => {
                            // Grid navigation: move up one row (4 columns)
                            let cols = 4;
                            if self.config_selection_index >= cols {
                                self.config_selection_index -= cols;
                            }
                        }
                        KeyCode::Down | KeyCode::Tab => {
                            // Grid navigation: move down one row (4 columns)
                            let cols = 4;
                            if self.config_selection_index + cols < total {
                                self.config_selection_index += cols;
                            } else {
                                // Wrap to next item if at bottom
                                self.config_selection_index =
                                    (self.config_selection_index + 1) % total;
                            }
                        }
                        KeyCode::Left => {
                            // Grid navigation: move left
                            if self.config_selection_index > 0 {
                                self.config_selection_index -= 1;
                            }
                        }
                        KeyCode::Right => {
                            // Grid navigation: move right
                            if self.config_selection_index < total - 1 {
                                self.config_selection_index += 1;
                            }
                        }
                        KeyCode::Enter => {
                            if let Some(template) =
                                templates::CONFIG_TEMPLATES.get(self.config_selection_index)
                            {
                                // Check if this is Local LLM template
                                if template.key == "local_llm" {
                                    // Go to Local LLM form
                                    self.local_llm_form_data = LocalLlmFormData::new();
                                    self.state = AppState::LocalLlmConfig;
                                } else {
                                    // Store template key for later config generation
                                    self.selected_template_key = Some(template.key.to_string());
                                    // Set selected provider and go to env setup
                                    self.form_data.selected_provider = template.key.to_string();
                                    self.form_data.api_key.clear();
                                    self.form_data.openai_api_key.clear();
                                    self.form_data.focus_state =
                                        crate::app::form_data::FocusState::Field(0);
                                    self.form_data.error_message.clear();

                                    self.state = AppState::EnvSetup;
                                }
                            }
                        }
                        KeyCode::Esc | KeyCode::Char('q') => {
                            self.state = AppState::Confirmation;
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            self.running = false;
                        }
                        _ => {}
                    }
                }
            }
        }

        Ok(())
    }

    fn generate_env_file(&self) -> Result<()> {
        let project_root = utils::project_root();
        let env_path = project_root.join(".env");

        let uuid_fragment = uuid::Uuid::new_v4()
            .to_string()
            .split('-')
            .next()
            .unwrap_or("123")
            .to_string();
        let user_uuid = format!("demo-user-{}", uuid_fragment);

        // Generate secure secrets (64 hex characters = 256 bits each)
        let jwt_secret = format!(
            "{}{}",
            uuid::Uuid::new_v4().to_string().replace("-", ""),
            uuid::Uuid::new_v4().to_string().replace("-", "")
        );
        let nextauth_secret = format!(
            "{}{}",
            uuid::Uuid::new_v4().to_string().replace("-", ""),
            uuid::Uuid::new_v4().to_string().replace("-", "")
        );

        let mut env_content = utils::ENV_TEMPLATE.to_string();

        let kc = &self.keycloak_form_data;
        let nextauth_url = self.form_data.nextauth_url();

        // Default values
        env_content = env_content.replace("{{ANALYTICS_AI_SERVICE_PORT}}", "5555");
        env_content = env_content.replace("{{USER_UUID}}", user_uuid.as_str());
        env_content = env_content.replace("{{GENERATION_MODEL}}", "default");
        env_content = env_content.replace("{{HOST_PORT}}", self.form_data.ui_port.trim());
        env_content = env_content.replace("{{AI_SERVICE_FORWARD_PORT}}", "5555");
        env_content = env_content.replace("{{JWT_SECRET}}", &jwt_secret);
        env_content = env_content.replace("{{NEXTAUTH_SECRET}}", &nextauth_secret);
        env_content = env_content.replace(
            "NEXTAUTH_URL=http://localhost:3000",
            &format!("NEXTAUTH_URL={}", nextauth_url),
        );
        env_content = env_content.replace(
            "{{KEYCLOAK_OAUTH_ENABLED}}",
            if kc.enabled { "true" } else { "false" },
        );
        env_content = env_content.replace("{{KEYCLOAK_PUBLIC_URL}}", kc.public_url.trim());
        env_content = env_content.replace("{{KEYCLOAK_URL}}", &kc.keycloak_url());
        env_content = env_content.replace(
            "{{KEYCLOAK_REALM}}",
            if kc.realm.trim().is_empty() {
                "master"
            } else {
                kc.realm.trim()
            },
        );
        env_content = env_content.replace("{{KEYCLOAK_CLIENT_ID}}", kc.client_id.trim());
        env_content = env_content.replace("{{KEYCLOAK_CLIENT_SECRET}}", kc.client_secret.trim());
        env_content = env_content.replace("{{KEYCLOAK_DEFAULT_ROLE}}", kc.default_role());
        env_content = env_content.replace(
            "{{KEYCLOAK_AUTO_REGISTER}}",
            if kc.auto_register { "true" } else { "false" },
        );

        // License key — left empty; user activates via the web UI after installation
        env_content = env_content.replace("{{LICENSE_KEY}}", "");

        // Set API key based on provider
        let env_key = self.form_data.get_env_key_name();
        let api_key_value = self.form_data.api_key.trim();
        let openai_api_key_value = self.form_data.openai_api_key.trim();

        // Handle different providers
        if self.form_data.selected_provider == "local_llm" {
            // Local LLM uses dummy API key
            env_content = env_content.replace("{{OPENAI_API_KEY}}", "sk-dummy-key-for-local-llm");
        } else if env_key.is_empty() {
            // No API key needed (ollama)
            env_content = env_content.replace("{{OPENAI_API_KEY}}", "");
        } else {
            // First, handle provider-specific API key
            if env_key == "OPENAI_API_KEY" {
                // If provider is OpenAI itself, use the main API key
                env_content = env_content.replace("{{OPENAI_API_KEY}}", api_key_value);
            } else {
                // Process all lines to handle both provider key and OpenAI key
                let lines: Vec<&str> = env_content.lines().collect();
                let mut new_lines: Vec<String> = Vec::new();
                let mut added_provider_key = false;
                let mut added_openai_key = false;
                let needs_openai =
                    self.form_data.needs_openai_embedding() && !openai_api_key_value.is_empty();

                for line in lines {
                    let trimmed = line.trim();

                    // Skip the placeholder line for OPENAI_API_KEY if we're not using it as main key
                    if trimmed == "OPENAI_API_KEY={{OPENAI_API_KEY}}"
                        || trimmed == "OPENAI_API_KEY="
                    {
                        // Skip this line, we'll add it later if needed
                        continue;
                    }

                    // Check if this line already has the provider key we want to add
                    if trimmed.starts_with(&format!("{}=", env_key)) {
                        // Replace existing key
                        new_lines.push(format!("{}={}", env_key, api_key_value));
                        added_provider_key = true;
                    } else if trimmed.starts_with("OPENAI_API_KEY=") {
                        // Handle existing OPENAI_API_KEY line
                        if needs_openai {
                            new_lines.push(format!("OPENAI_API_KEY={}", openai_api_key_value));
                            added_openai_key = true;
                        }
                        // If not needed, skip this line
                    } else {
                        new_lines.push(line.to_string());
                        // Add after vendor keys comment if not added yet
                        if line.contains("# vendor keys") {
                            if !added_provider_key {
                                new_lines.push(format!("{}={}", env_key, api_key_value));
                                added_provider_key = true;
                            }
                            if needs_openai && !added_openai_key {
                                new_lines.push(format!("OPENAI_API_KEY={}", openai_api_key_value));
                                added_openai_key = true;
                            }
                        }
                    }
                }

                // Add provider key if not found
                if !added_provider_key {
                    new_lines.push(format!("{}={}", env_key, api_key_value));
                }

                // Add OpenAI key if needed and not found
                if needs_openai && !added_openai_key {
                    new_lines.push(format!("OPENAI_API_KEY={}", openai_api_key_value));
                }

                env_content = new_lines.join("\n");
            }
        }

        fs::write(env_path, env_content)?;
        Ok(())
    }

    fn write_config_yaml(&self, template: &ConfigTemplate) -> Result<()> {
        let project_root = utils::project_root();
        let config_path = project_root.join("config.yaml");
        fs::write(config_path, template.render())?;
        Ok(())
    }

    fn generate_local_llm_config(&self) -> Result<()> {
        let project_root = utils::project_root();
        let config_path = project_root.join("config.yaml");

        // Get the Local LLM template
        let template = templates::CONFIG_TEMPLATES
            .iter()
            .find(|t| t.key == "local_llm")
            .ok_or_else(|| eyre!("Local LLM template not found"))?;

        // Render the template with placeholders
        let mut content = template.render();

        // Replace Local LLM specific placeholders
        content = content.replace("{{LLM_MODEL}}", &self.local_llm_form_data.llm_model);
        content = content.replace("{{LLM_API_BASE}}", &self.local_llm_form_data.llm_api_base);
        content = content.replace("{{MAX_TOKENS}}", &self.local_llm_form_data.max_tokens);
        content = content.replace(
            "{{EMBEDDING_MODEL}}",
            &self.local_llm_form_data.embedding_model,
        );
        content = content.replace(
            "{{EMBEDDING_API_BASE}}",
            &self.local_llm_form_data.embedding_api_base,
        );
        content = content.replace("{{EMBEDDING_DIM}}", &self.local_llm_form_data.embedding_dim);

        fs::write(config_path, content)?;
        Ok(())
    }

    async fn run_docker_compose(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        let compose_cmd = self.detect_compose_command().await?;

        let project_root = utils::project_root();
        utils::ensure_compose_bundle(&project_root)?;
        let compose_files = [
            "docker-compose.yml",
            "docker-compose.yaml",
            "compose.yml",
            "compose.yaml",
        ];

        let has_compose = compose_files
            .iter()
            .any(|name| project_root.join(name).exists());

        if !has_compose {
            let mut msg = format!(
                "Docker Compose file not found in {}. Please run the installer from the project directory containing docker-compose.yml",
                project_root.display()
            );
            msg.push_str(" or provide the compose file there.");
            return Err(color_eyre::eyre::eyre!(msg));
        }

        self.add_log("🔨 Step 1/2: Building images...");
        self.add_log(&format!("📦 Executing: {} build", compose_cmd.join(" ")));
        let _ = self.redraw(terminal);

        let buildkit_available = self.buildkit_available().await.unwrap_or(false);
        if buildkit_available {
            self.add_log_and_redraw(terminal, "🛠 Using BuildKit for builds");
        } else {
            self.add_log_and_redraw(
                terminal,
                "⚠️ BuildKit (docker buildx) not available. Please install docker-buildx-plugin and retry.",
            );
            return Err(color_eyre::eyre::eyre!(
                "BuildKit is required but docker buildx is missing"
            ));
        }

        let mut build_child = {
            let mut cmd = Command::new(&compose_cmd[0]);
            if compose_cmd.len() > 1 {
                cmd.arg(&compose_cmd[1]);
            }
            cmd.arg("build");
            if buildkit_available {
                cmd.env("DOCKER_BUILDKIT", "1");
            }
            cmd.env("DOCKER_CLI_PROGRESS", "plain")
                .current_dir(&project_root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?
        };

        let build_stdout = build_child.stdout.take().expect("Failed to capture stdout");
        let build_stderr = build_child.stderr.take().expect("Failed to capture stderr");

        let mut build_stdout_reader = BufReader::new(build_stdout).lines();
        let mut build_stderr_reader = BufReader::new(build_stderr).lines();

        loop {
            tokio::select! {
                result = build_stdout_reader.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            self.process_log_line(&line);
                            let _ = self.redraw(terminal);
                        }
                        Ok(None) => break,
                        Err(e) => {
                            self.add_log(&format!("❌ Error reading stdout: {}", e));
                            let _ = self.redraw(terminal);
                            break;
                        }
                    }
                }
                result = build_stderr_reader.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            self.process_log_line(&line);
                            let _ = self.redraw(terminal);
                        }
                        Ok(None) => break,
                        Err(e) => {
                            self.add_log(&format!("❌ Error reading stderr: {}", e));
                            let _ = self.redraw(terminal);
                            break;
                        }
                    }
                }
            }
        }

        let build_status = build_child.wait().await?;

        if !build_status.success() {
            return Err(color_eyre::eyre::eyre!("Docker Compose build failed"));
        }

        self.add_log("✅ Build completed successfully!");
        self.progress = 40.0;
        let _ = self.redraw(terminal);

        self.add_log("🛑 Step 2/3: Stopping existing containers...");
        self.add_log(&format!("📦 Executing: {} down", compose_cmd.join(" ")));
        let _ = self.redraw(terminal);

        let down_status = {
            let mut cmd = Command::new(&compose_cmd[0]);
            if compose_cmd.len() > 1 {
                cmd.arg(&compose_cmd[1]);
            }
            cmd.args(["down", "--remove-orphans"])
                .env("DOCKER_CLI_PROGRESS", "plain")
                .current_dir(&project_root)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .await?
        };
        if !down_status.success() {
            self.add_log("⚠️ Warning: docker compose down returned non-zero (may be no containers running, continuing...)");
        } else {
            self.add_log("✅ Existing containers stopped.");
        }
        self.progress = 60.0;
        let _ = self.redraw(terminal);

        self.add_log("🚀 Step 3/3: Starting services...");
        self.add_log(&format!("📦 Executing: {} up -d", compose_cmd.join(" ")));
        let _ = self.redraw(terminal);

        let mut up_child = {
            let mut cmd = Command::new(&compose_cmd[0]);
            if compose_cmd.len() > 1 {
                cmd.arg(&compose_cmd[1]);
            }
            cmd.args(["up", "-d", "--remove-orphans"])
                .env("DOCKER_CLI_PROGRESS", "plain")
                .current_dir(&project_root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?
        };

        let up_stdout = up_child.stdout.take().expect("Failed to capture stdout");
        let up_stderr = up_child.stderr.take().expect("Failed to capture stderr");

        let mut up_stdout_reader = BufReader::new(up_stdout).lines();
        let mut up_stderr_reader = BufReader::new(up_stderr).lines();

        loop {
            tokio::select! {
                result = up_stdout_reader.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            self.process_log_line(&line);
                            let _ = self.redraw(terminal);
                        }
                        Ok(None) => break,
                        Err(e) => {
                            self.add_log(&format!("❌ Error reading stdout: {}", e));
                            let _ = self.redraw(terminal);
                            break;
                        }
                    }
                }
                result = up_stderr_reader.next_line() => {
                    match result {
                        Ok(Some(line)) => {
                            self.process_log_line(&line);
                            let _ = self.redraw(terminal);
                        }
                        Ok(None) => break,
                        Err(e) => {
                            self.add_log(&format!("❌ Error reading stderr: {}", e));
                            let _ = self.redraw(terminal);
                            break;
                        }
                    }
                }
            }
        }

        let up_status = up_child.wait().await?;

        if up_status.success() {
            self.add_log("✅ All services started successfully!");
            self.progress = 100.0;
            Ok(())
        } else {
            Err(color_eyre::eyre::eyre!("Docker Compose up failed"))
        }
    }

    fn process_log_line(&mut self, line: &str) {
        let lower = line.to_lowercase();

        // Update progress during Docker build steps when available (e.g., "Step 1/4 : FROM busybox").
        if let Some((step, total)) = Self::parse_build_step(line) {
            if total > 0 {
                let pct = 5.0 + (step as f64 / total as f64) * 45.0; // 5-50% during build phase
                self.progress = self.progress.max(pct.min(50.0));
            }
        }

        if lower.contains("pulling") {
            if let Some(service) = self.extract_service_name(line) {
                self.current_service = service.clone();
                self.add_log(&format!("⬇️  Pulling image for {}...", service));
            }
        } else if lower.contains("pulled") {
            self.add_log("✓ Image pulled");
        } else if lower.contains("creating") {
            if let Some(service) = self.extract_service_name(line) {
                self.current_service = service.clone();
                self.add_log(&format!("🔨 Creating container {}...", service));
            }
        } else if lower.contains("created") {
            self.add_log("✓ Container created");
        } else if lower.contains("starting") {
            if let Some(service) = self.extract_service_name(line) {
                self.current_service = service.clone();
                self.add_log(&format!("▶️  Starting service {}...", service));
            }
        } else if lower.contains("started") {
            self.completed_services += 1;
            self.progress =
                50.0 + (self.completed_services as f64 / self.total_services as f64) * 50.0;
            self.add_log(&format!(
                "✅ Service started ({}/{})",
                self.completed_services, self.total_services
            ));
        } else if lower.contains("running") {
            self.add_log("🟢 Service is running");
        } else if lower.contains("error") || lower.contains("failed") {
            self.add_log(&format!("❌ {}", line));
        } else if !line.trim().is_empty() {
            self.add_log(&format!("ℹ️  {}", line));
        }
    }

    fn parse_build_step(line: &str) -> Option<(u32, u32)> {
        // Expected format prefix: "Step X/Y" or "Step X/Y :"
        let trimmed = line.trim();
        if !trimmed.starts_with("Step ") {
            return None;
        }

        let after = trimmed.strip_prefix("Step ")?;
        let mut parts = after.split_whitespace();
        let frac = parts.next()?; // e.g., "1/4"
        let mut nums = frac.split('/');
        let step: u32 = nums.next()?.parse().ok()?;
        let total: u32 = nums.next()?.parse().ok()?;
        Some((step, total))
    }

    async fn buildkit_available(&self) -> Result<bool> {
        let status = Command::new("docker")
            .args(["buildx", "version"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await?
            .success();
        Ok(status)
    }

    fn extract_service_name(&self, line: &str) -> Option<String> {
        let services = [
            "analytics-service",
            "qdrant",
            "northwind-db",
            "analytics-ui",
        ];

        for service in services {
            if line.to_lowercase().contains(service) {
                return Some(service.to_string());
            }
        }
        None
    }

    fn add_log(&mut self, message: &str) {
        self.logs.push(message.to_string());

        if self.logs.len() > 100 {
            self.logs.remove(0);
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        match &self.state {
            AppState::RegistrySetup => {
                let view = RegistrySetupView {
                    form: &self.registry_form,
                    status: self.registry_status.as_deref(),
                };
                ui::render_registry_setup(frame, &view);
            }
            AppState::Confirmation => {
                let menu_options = self.menu_options();
                let view = ConfirmationView {
                    env_exists: self.env_exists,
                    config_exists: self.config_exists,
                    menu_selection: &self.menu_selection,
                    menu_options: &menu_options,
                    airgapped: self.airgapped,
                };
                ui::render_confirmation(frame, &view);
            }
            AppState::EnvSetup => {
                let view = EnvSetupView {
                    form_data: &self.form_data,
                };
                ui::render_env_setup(frame, &view);
            }
            AppState::ConfigSelection => {
                let view = ConfigSelectionView {
                    templates: templates::CONFIG_TEMPLATES,
                    selected_index: self.config_selection_index,
                };
                ui::render_config_selection(frame, &view);
            }
            AppState::KeycloakConfig => {
                let view = KeycloakConfigView {
                    form_data: &self.keycloak_form_data,
                };
                ui::render_keycloak_config(frame, &view);
            }
            AppState::LocalLlmConfig => {
                let view = LocalLlmConfigView {
                    form_data: &self.local_llm_form_data,
                };
                ui::render_local_llm_config(frame, &view);
            }
            AppState::UpdateList => {
                let view = UpdateListView {
                    updates: &self.update_infos,
                    selected_index: self.update_selection_index,
                    message: self.update_message.as_deref(),
                    logs: &self.logs,
                    pulling: false,
                    progress: None,
                };
                ui::render_update_list(frame, &view);
            }
            AppState::UpdatePulling => {
                let view = UpdateListView {
                    updates: &self.update_infos,
                    selected_index: self.update_selection_index,
                    message: self.update_message.as_deref(),
                    logs: &self.logs,
                    pulling: true,
                    progress: Some(self.progress),
                };
                ui::render_update_list(frame, &view);
            }
            AppState::Installing => {
                let view = InstallingView {
                    progress: self.progress,
                    current_service: &self.current_service,
                    completed_services: self.completed_services,
                    total_services: self.total_services,
                    logs: &self.logs,
                    airgapped: self.airgapped,
                };
                ui::render_installing(frame, &view);
            }
            AppState::Success => {
                let view = SuccessView { logs: &self.logs };
                ui::render_success(frame, &view);
            }
            AppState::Error(error) => {
                let view = ErrorView {
                    error,
                    logs: &self.logs,
                };
                ui::render_error(frame, &view);
            }
        }
    }
}
