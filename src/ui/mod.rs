mod ascii_art;
mod config_selection;
mod confirmation;
mod env_setup;
mod error;
mod installing;

mod keycloak_config;
mod local_llm_config;
mod registry;
mod success;
mod update;

pub use ascii_art::{ASCII_HEADER, get_orange_accent, get_orange_color};
pub use config_selection::{ConfigSelectionView, render_config_selection};
pub use confirmation::{ConfirmationView, render_confirmation};
pub use env_setup::{EnvSetupView, render_env_setup};
pub use error::{ErrorView, render_error};
pub use installing::{InstallingView, render_installing};

pub use keycloak_config::{KeycloakConfigView, render_keycloak_config};
pub use local_llm_config::{LocalLlmConfigView, render_local_llm_config};
pub use registry::{RegistrySetupView, render_registry_setup};
pub use success::{SuccessView, render_success};
pub use update::{UpdateListView, render_update_list};
