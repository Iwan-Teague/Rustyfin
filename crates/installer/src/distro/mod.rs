use crate::utils::{HostPlatform, NativeUserContext};
use anyhow::Result;

pub trait DistroAdapter {
    fn name(&self) -> &str;
    fn install_packages(&self, user_context: &NativeUserContext) -> Result<()>;
    fn install_gpu_support(&self, _user_context: &NativeUserContext) -> Result<()> {
        Ok(())
    }
}

pub mod debian;
pub mod ubuntu;
pub mod unsupported;

pub fn resolve_adapter(host: &HostPlatform) -> Box<dyn DistroAdapter> {
    let id = host.id.as_deref().unwrap_or("unknown");
    let version = host.version_id.as_deref().unwrap_or("unknown");

    match id {
        "debian" => match version {
            "12" | "13" => Box::new(debian::DebianAdapter::new(version)),
            _ => Box::new(unsupported::UnsupportedAdapter::new(id, version)),
        },
        "ubuntu" => match version {
            "22.04" | "24.04" => Box::new(ubuntu::UbuntuAdapter::new(version)),
            _ => Box::new(unsupported::UnsupportedAdapter::new(id, version)),
        },
        _ => Box::new(unsupported::UnsupportedAdapter::new(id, version)),
    }
}
