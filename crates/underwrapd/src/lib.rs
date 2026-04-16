pub mod audit;
pub mod directory_channel;
pub(crate) mod peer_auth;
pub(crate) mod policy;
pub mod secretsd_channel;
pub mod server;
pub(crate) mod startup;
pub mod storage;
pub mod unlock;
pub(crate) mod unlock_flow;
pub(crate) mod wrapper_bindings;
pub(crate) mod wrapper_flow;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_modules_are_accessible() {
        let _ = audit::read_granted as fn(&str, &str, Option<u32>, Option<&str>);
        let _ = server::run;
        let _ = storage::open;
        let _ = unlock::UnlockState::new();
    }
}
