#[derive(Debug, Clone, PartialEq)]
pub enum FocusState {
    EnableToggle,
    Field(usize), // 0=public_url, 1=realm, 2=client_id, 3=client_secret
    RoleSelect,
    AutoRegisterToggle,
    SaveButton,
    CancelButton,
}

pub const ROLES: &[&str] = &["viewer", "editor", "admin"];

#[derive(Debug, Clone)]
pub struct KeycloakFormData {
    pub(crate) enabled: bool,
    pub(crate) public_url: String,
    pub(crate) realm: String,
    pub(crate) client_id: String,
    pub(crate) client_secret: String,
    pub(crate) default_role_index: usize, // index into ROLES
    pub(crate) auto_register: bool,
    pub(crate) focus_state: FocusState,
    pub(crate) error_message: String,
}

impl KeycloakFormData {
    pub fn new() -> Self {
        Self {
            enabled: false,
            public_url: String::new(),
            realm: String::from("master"),
            client_id: String::new(),
            client_secret: String::new(),
            default_role_index: 0, // "viewer"
            auto_register: true,
            focus_state: FocusState::EnableToggle,
            error_message: String::new(),
        }
    }

    pub fn default_role(&self) -> &str {
        ROLES[self.default_role_index]
    }

    /// Derive KEYCLOAK_URL from KEYCLOAK_PUBLIC_URL:
    /// if public_url contains "localhost", replace with "host.docker.internal"
    /// so the container can reach the host OS directly.
    pub fn keycloak_url(&self) -> String {
        if self.public_url.contains("localhost") {
            self.public_url.replace("localhost", "host.docker.internal")
        } else {
            self.public_url.clone()
        }
    }

    pub fn validate(&mut self) -> bool {
        if !self.enabled {
            self.error_message.clear();
            return true;
        }

        if self.public_url.trim().is_empty() {
            self.error_message = "Keycloak Public URL is required!".to_string();
            return false;
        }
        if !self.public_url.starts_with("http://") && !self.public_url.starts_with("https://") {
            self.error_message = "Keycloak Public URL must start with http:// or https://".to_string();
            return false;
        }
        if self.realm.trim().is_empty() {
            self.error_message = "Realm is required!".to_string();
            return false;
        }
        if self.client_id.trim().is_empty() {
            self.error_message = "Client ID is required!".to_string();
            return false;
        }
        if self.client_secret.trim().is_empty() {
            self.error_message = "Client Secret is required!".to_string();
            return false;
        }

        self.error_message.clear();
        true
    }

    pub fn get_current_value_mut(&mut self) -> Option<&mut String> {
        match &self.focus_state {
            FocusState::Field(idx) => match idx {
                0 => Some(&mut self.public_url),
                1 => Some(&mut self.realm),
                2 => Some(&mut self.client_id),
                3 => Some(&mut self.client_secret),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn field_name(idx: usize) -> &'static str {
        match idx {
            0 => "Keycloak Public URL",
            1 => "Realm",
            2 => "Client ID",
            3 => "Client Secret",
            _ => "",
        }
    }

    pub fn total_text_fields() -> usize {
        4
    }
}
