use color_eyre::{Result, eyre::eyre};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{DefaultTerminal, Frame};
use reqwest::Client;
use serde::Deserialize;
use std::path::PathBuf;
use std::process::Stdio;
use std::{env, fs};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

use crate::templates::{self, ConfigTemplate};
use crate::ui::{
    self, ConfigSelectionView, ConfirmationView, EnvSetupView, ErrorView, InstallingView,
    RegistrySetupView, SuccessView, UpdateListView,
};
use crate::utils;

pub mod form_data;
pub mod registry_form;
pub mod state;
mod updates;

pub use form_data::FormData;
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
    pub(crate) menu_selection: MenuSelection,
    config_selection_index: usize,
    update_infos: Vec<UpdateInfo>,
    update_selection_index: usize,
    update_message: Option<String>,
    registry_form: RegistryForm,
    registry_status: Option<String>,
    ghcr_token: Option<String>,
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

        let initial_state = if initial_token.is_some() {
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
            menu_selection: MenuSelection::Proceed,
            config_selection_index: 0,
            update_infos: Vec::new(),
            update_selection_index: 0,
            update_message: None,
            registry_form,
            registry_status: None,
            ghcr_token: initial_token,
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

                                    let result = self.run_docker_compose().await;

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
                                    self.registry_form.current_field = 0;
                                    self.registry_form.editing = false;
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
                                self.registry_status =
                                    Some("Update token and submit (Ctrl+S). Esc to cancel.".to_string());
                                self.registry_form.current_field = 0;
                                self.registry_form.editing = false;
                                self.registry_form.error_message.clear();
                                self.registry_form.token = self.ghcr_token.clone().unwrap_or_default();
                                self.state = AppState::RegistrySetup;
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
                            self.state = AppState::Confirmation;
                        }
                    }
                }
                AppState::ConfigSelection => {
                    self.handle_config_selection_events()?;
                }
                AppState::UpdateList => {
                    if let Some(action) = self.handle_update_list_events()? {
                        match action {
                            UpdateListAction::Pull => {
                                self.state = AppState::UpdatePulling;
                                if let Err(e) = self.pull_selected_update().await {
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

        if self.ghcr_token.is_some() {
            options.push(MenuSelection::UpdateToken);
        }

        options.push(MenuSelection::CheckUpdates);

        if self.env_exists && self.config_exists {
            options.push(MenuSelection::Proceed);
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
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if self.registry_form.editing
                        && RegistryForm::is_input_field(self.registry_form.current_field)
                    {
                        match key.code {
                            KeyCode::Enter | KeyCode::Esc => {
                                self.registry_form.editing = false;
                            }
                            KeyCode::Backspace => {
                                self.registry_form.get_current_value_mut().pop();
                            }
                            KeyCode::Char(c) => {
                                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                                    self.registry_form.get_current_value_mut().push(c);
                                }
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Up => {
                                if self.registry_form.current_field == 0 {
                                    self.registry_form.current_field =
                                        self.registry_form.total_items() - 1;
                                } else {
                                    self.registry_form.current_field -= 1;
                                }
                            }
                            KeyCode::Down | KeyCode::Tab => {
                                self.registry_form.current_field =
                                    (self.registry_form.current_field + 1)
                                        % self.registry_form.total_items();
                            }
                            KeyCode::Enter => {
                                if RegistryForm::is_input_field(self.registry_form.current_field) {
                                    self.registry_form.editing = true;
                                } else {
                                    return Ok(Some(RegistryAction::Submit));
                                }
                            }
                            KeyCode::Char('s') => {
                                if key.modifiers.contains(KeyModifiers::CONTROL) {
                                    return Ok(Some(RegistryAction::Submit));
                                }
                            }
                            KeyCode::Esc | KeyCode::Char('q') => {
                                return Ok(Some(RegistryAction::Skip));
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                self.running = false;
                            }
                            _ => {}
                        }
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

    async fn pull_selected_update(&mut self) -> Result<()> {
        if self.update_infos.is_empty() {
            return Ok(());
        }

        let index = self.update_selection_index.min(self.update_infos.len() - 1);
        let reference;
        let image;
        let tag;

        {
            let info = &self.update_infos[index];
            reference = info.pull_reference();
            image = info.image.clone();
            tag = info.current_tag.clone();
        }

        self.logs.clear();
        self.add_log(&format!("⬇️  Executing: docker pull {}", reference));

        let mut child = Command::new("docker")
            .args(["pull", &reference])
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
                        Ok(Some(line)) => self.add_log(&format!("ℹ️  {}", line)),
                        Ok(None) => break,
                        Err(e) => {
                            self.add_log(&format!("❌ Error reading stdout: {}", e));
                            break;
                        }
                    }
                }
                output = stderr_reader.next_line() => {
                    match output {
                        Ok(Some(line)) => self.add_log(&format!("⚠️  {}", line)),
                        Ok(None) => break,
                        Err(e) => {
                            self.add_log(&format!("❌ Error reading stderr: {}", e));
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

        self.add_log("✅ Image pulled successfully");

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

    fn handle_form_events(&mut self) -> Result<Option<bool>> {
        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if self.form_data.editing {
                        match key.code {
                            KeyCode::Enter | KeyCode::Esc => {
                                self.form_data.editing = false;
                            }
                            KeyCode::Char(c) => {
                                self.form_data.get_current_value_mut().push(c);
                            }
                            KeyCode::Backspace => {
                                self.form_data.get_current_value_mut().pop();
                            }
                            _ => {}
                        }
                    } else {
                        match key.code {
                            KeyCode::Enter => {
                                if self.form_data.current_field < self.form_data.get_total_fields()
                                {
                                    self.form_data.editing = true;
                                }
                            }
                            KeyCode::Up => {
                                if self.form_data.current_field > 0 {
                                    self.form_data.current_field -= 1;
                                }
                            }
                            KeyCode::Down | KeyCode::Tab => {
                                if self.form_data.current_field
                                    < self.form_data.get_total_fields() - 1
                                {
                                    self.form_data.current_field += 1;
                                }
                            }
                            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                if self.form_data.validate() {
                                    return Ok(Some(true));
                                }
                            }
                            KeyCode::Esc | KeyCode::Char('q') => {
                                return Ok(Some(false));
                            }
                            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                return Ok(Some(false));
                            }
                            _ => {}
                        }
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
                            if self.config_selection_index == 0 {
                                self.config_selection_index = total - 1;
                            } else {
                                self.config_selection_index -= 1;
                            }
                        }
                        KeyCode::Down | KeyCode::Tab => {
                            self.config_selection_index = (self.config_selection_index + 1) % total;
                        }
                        KeyCode::Enter => {
                            if let Some(template) =
                                templates::CONFIG_TEMPLATES.get(self.config_selection_index)
                            {
                                match self.write_config_yaml(template) {
                                    Ok(_) => {
                                        self.config_exists = true;
                                        // Set selected provider and go to env setup
                                        self.form_data.selected_provider = template.key.to_string();
                                        self.form_data.api_key.clear();
                                        self.form_data.openai_api_key.clear();
                                        self.form_data.current_field = 0;
                                        self.form_data.editing = false;
                                        self.form_data.error_message.clear();

                                        if !self.env_exists {
                                            self.state = AppState::EnvSetup;
                                        } else {
                                            self.state = AppState::Confirmation;
                                            self.menu_selection = MenuSelection::Proceed;
                                        }
                                    }
                                    Err(e) => {
                                        self.state = AppState::Error(format!(
                                            "Failed to generate config.yaml: {}",
                                            e
                                        ));
                                    }
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

        let mut env_content = utils::ENV_TEMPLATE.to_string();

        // Default values
        env_content = env_content.replace("{{ANALYTICS_AI_SERVICE_PORT}}", "5555");
        env_content = env_content.replace("{{USER_UUID}}", user_uuid.as_str());
        env_content = env_content.replace("{{GENERATION_MODEL}}", "default");
        env_content = env_content.replace("{{HOST_PORT}}", "3000");
        env_content = env_content.replace("{{AI_SERVICE_FORWARD_PORT}}", "5555");

        // Set API key based on provider
        let env_key = self.form_data.get_env_key_name();
        let api_key_value = self.form_data.api_key.trim();
        let openai_api_key_value = self.form_data.openai_api_key.trim();

        // Handle different providers
        if env_key.is_empty() {
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

    async fn run_docker_compose(&mut self) -> Result<()> {
        self.add_log("🔨 Step 1/2: Building images...");
        self.add_log("📦 Executing: docker compose build");

        let mut build_child = Command::new("docker")
            .args(["compose", "build"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let build_stdout = build_child.stdout.take().expect("Failed to capture stdout");
        let build_stderr = build_child.stderr.take().expect("Failed to capture stderr");

        let mut build_stdout_reader = BufReader::new(build_stdout).lines();
        let mut build_stderr_reader = BufReader::new(build_stderr).lines();

        loop {
            tokio::select! {
                result = build_stdout_reader.next_line() => {
                    match result {
                        Ok(Some(line)) => self.process_log_line(&line),
                        Ok(None) => break,
                        Err(e) => {
                            self.add_log(&format!("❌ Error reading stdout: {}", e));
                            break;
                        }
                    }
                }
                result = build_stderr_reader.next_line() => {
                    match result {
                        Ok(Some(line)) => self.process_log_line(&line),
                        Ok(None) => break,
                        Err(e) => {
                            self.add_log(&format!("❌ Error reading stderr: {}", e));
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
        self.progress = 50.0;

        self.add_log("🚀 Step 2/2: Starting services...");
        self.add_log("📦 Executing: docker compose up -d");

        let mut up_child = Command::new("docker")
            .args(["compose", "up", "-d"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let up_stdout = up_child.stdout.take().expect("Failed to capture stdout");
        let up_stderr = up_child.stderr.take().expect("Failed to capture stderr");

        let mut up_stdout_reader = BufReader::new(up_stdout).lines();
        let mut up_stderr_reader = BufReader::new(up_stderr).lines();

        loop {
            tokio::select! {
                result = up_stdout_reader.next_line() => {
                    match result {
                        Ok(Some(line)) => self.process_log_line(&line),
                        Ok(None) => break,
                        Err(e) => {
                            self.add_log(&format!("❌ Error reading stdout: {}", e));
                            break;
                        }
                    }
                }
                result = up_stderr_reader.next_line() => {
                    match result {
                        Ok(Some(line)) => self.process_log_line(&line),
                        Ok(None) => break,
                        Err(e) => {
                            self.add_log(&format!("❌ Error reading stderr: {}", e));
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
            AppState::UpdateList => {
                let view = UpdateListView {
                    updates: &self.update_infos,
                    selected_index: self.update_selection_index,
                    message: self.update_message.as_deref(),
                    logs: &self.logs,
                    pulling: false,
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
                };
                ui::render_installing(frame, &view);
            }
            AppState::Success => {
                let view = SuccessView { logs: &self.logs };
                ui::render_success(frame, &view);
            }
            AppState::Error(err) => {
                let view = ErrorView { logs: &self.logs };
                ui::render_error(frame, err, &view);
            }
        }
    }
}
