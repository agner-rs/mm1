use masked::Masked;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct Config {
    #[serde(default = "Default::default")]
    pub(crate) authc: AuthcConfig,
}

#[derive(Default, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AuthcConfig {
    #[default]
    Trusted,
    Cookie(Masked<String>),
    SharedSecret(#[allow(unused)] Masked<String>),
}
