use crate::config::{Config, ConfigFile, VmConfig, VmType};
use std::net::IpAddr;

mod logging;
pub mod network;
mod provision;
mod qemu;

#[derive(err_derive::Error, Debug)]
pub enum Error {
    #[error(display = "Failed to update config")]
    UpdateConfig(super::config::Error),
    #[error(display = "Could not find config")]
    ConfigNotFound(String),
    #[error(display = "QEMU module failed")]
    Qemu(qemu::Error),
    #[error(display = "Provisioning failed")]
    Provision(provision::Error),
}

#[async_trait::async_trait]
pub trait VmInstance {
    /// Path to pty on the host that corresponds to the serial device
    fn get_pty(&self) -> &str;

    /// Get initial IP address of guest
    fn get_ip(&self) -> &IpAddr;

    /// Wait for VM to destruct
    async fn wait(&mut self);
}

pub async fn set_config(
    config: &mut ConfigFile,
    vm_name: &str,
    vm_config: VmConfig,
) -> Result<(), Error> {
    config
        .edit(|config| {
            config.vms.insert(vm_name.to_owned(), vm_config);
        })
        .await
        .map_err(Error::UpdateConfig)
}

pub async fn run(config: &Config, name: &str) -> Result<Box<dyn VmInstance>, Error> {
    let vm_conf = get_vm_config(config, name)?;

    log::info!("Starting \"{name}\"");

    let instance = match vm_conf.vm_type {
        VmType::Qemu => Box::new(qemu::run(config, vm_conf).await.map_err(Error::Qemu)?),
    };

    log::info!("Started instance of \"{name}\" vm");

    Ok(instance)
}

pub async fn provision(
    config: &Config,
    name: &str,
    instance: &Box<dyn VmInstance>,
) -> Result<String, Error> {
    let vm_conf = get_vm_config(config, name)?;
    provision::provision(vm_conf, instance)
        .await
        .map_err(Error::Provision)
}

pub fn get_vm_config<'a>(config: &'a Config, name: &str) -> Result<&'a VmConfig, Error> {
    config
        .get_vm(name)
        .ok_or_else(|| Error::ConfigNotFound(name.to_owned()))
}
