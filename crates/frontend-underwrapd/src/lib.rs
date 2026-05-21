pub mod audit;
pub mod backend_client;
pub(crate) mod peer_auth;
pub mod server;
pub(crate) mod startup;
pub mod storage;
pub(crate) mod wrapper_bindings;
pub(crate) mod wrapper_flow;
pub mod wrapper_resource_channel;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_modules_are_accessible() {
        let _ = audit::read_granted as fn(&str, &str, Option<u32>, Option<&str>);
        let _ = server::run;
        let _ = storage::open;
    }
}
