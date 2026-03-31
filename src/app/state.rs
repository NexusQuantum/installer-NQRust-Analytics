#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    RegistrySetup,
    Confirmation,
    EnvSetup,
    ConfigSelection,
    LocalLlmConfig,
    KeycloakConfig,
    UpdateList,
    UpdatePulling,
    Installing,
    Success,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MenuSelection {
    Proceed,
    GenerateEnv,
    GenerateConfig,
    ConfigureKeycloak,
    UpdateToken,
    CheckUpdates,
    Cancel,
}
