#[derive(Debug, Clone, PartialEq)]
pub enum AppState {
    RegistrySetup,
    Confirmation,
    EnvSetup,
    ConfigSelection,
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
    UpdateToken,
    CheckUpdates,
    Cancel,
}
