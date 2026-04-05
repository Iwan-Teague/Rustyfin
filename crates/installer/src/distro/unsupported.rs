use crate::NativeUserContext;
use crate::distro::DistroAdapter;
use anyhow::{Result, bail};

pub struct UnsupportedAdapter {
    id: String,
    version: String,
}

impl UnsupportedAdapter {
    pub fn new(id: &str, version: &str) -> Self {
        Self {
            id: id.to_string(),
            version: version.to_string(),
        }
    }
}

impl DistroAdapter for UnsupportedAdapter {
    fn name(&self) -> &str {
        "unsupported"
    }

    fn install_packages(&self, _user_context: &NativeUserContext) -> Result<()> {
        bail!(
            "Unsupported distribution: {} {}. Currently only Debian 12, Debian 13, Ubuntu 22.04, and Ubuntu 24.04 are supported.",
            self.id,
            self.version
        );
    }
}
