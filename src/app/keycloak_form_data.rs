#[derive(Debug, Clone, PartialEq)]
pub enum FocusState {
    EnableToggle,
    SchemeSelect,
    Field(usize), // 0=host, 1=port, 2=realm, 3=client_id, 4=client_secret
    RoleSelect,
    AutoRegisterToggle,
    SaveButton,
    CancelButton,
}

pub const ROLES: &[&str] = &["viewer", "editor", "admin"];
pub const SCHEMES: &[&str] = &["http", "https"];

#[derive(Debug, Clone)]
pub struct KeycloakFormData {
    pub(crate) enabled: bool,
    pub(crate) scheme_index: usize, // index into SCHEMES
    pub(crate) host: String,
    pub(crate) port: String,
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
            scheme_index: 0, // "http"
            host: String::new(),
            port: String::new(),
            realm: String::from("myrealm"),
            client_id: String::from("nqrust-analytics"),
            client_secret: String::new(),
            default_role_index: 0, // "viewer"
            auto_register: true,
            focus_state: FocusState::EnableToggle,
            error_message: String::new(),
        }
    }

    pub fn scheme(&self) -> &str {
        SCHEMES[self.scheme_index]
    }

    pub fn default_role(&self) -> &str {
        ROLES[self.default_role_index]
    }

    /// Combine scheme + host + port into the public URL shown to the browser.
    pub fn public_url(&self) -> String {
        let host = self.host.trim();
        let port = self.port.trim();
        if port.is_empty() {
            format!("{}://{}", self.scheme(), host)
        } else {
            format!("{}://{}:{}", self.scheme(), host, port)
        }
    }

    /// Derive KEYCLOAK_URL (server-side) from the public URL.
    /// Same scheme as public URL; replace "localhost" with "host.docker.internal"
    /// so the analytics container can reach the host OS.
    pub fn keycloak_url(&self) -> String {
        let host = if self.host.trim() == "localhost" {
            "host.docker.internal"
        } else {
            self.host.trim()
        };
        let port = self.port.trim();
        if port.is_empty() {
            format!("{}://{}", self.scheme(), host)
        } else {
            format!("{}://{}:{}", self.scheme(), host, port)
        }
    }

    pub fn validate(&mut self) -> bool {
        if !self.enabled {
            self.error_message.clear();
            return true;
        }

        if self.host.trim().is_empty() {
            self.error_message = "Host is required! (e.g. 192.168.1.100)".to_string();
            return false;
        }
        if self.port.trim().is_empty() {
            self.error_message = "Port is required! (e.g. 8082)".to_string();
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
                0 => Some(&mut self.host),
                1 => Some(&mut self.port),
                2 => Some(&mut self.realm),
                3 => Some(&mut self.client_id),
                4 => Some(&mut self.client_secret),
                _ => None,
            },
            _ => None,
        }
    }

    pub fn field_name(idx: usize) -> &'static str {
        match idx {
            0 => "Host",
            1 => "Port",
            2 => "Realm",
            3 => "Client ID",
            4 => "Client Secret",
            _ => "",
        }
    }

    pub fn total_text_fields() -> usize {
        5 // host, port, realm, client_id, client_secret
    }
}
