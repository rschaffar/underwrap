pub mod audit;
pub mod directory_channel;
pub(crate) mod peer_auth;
pub mod secretsd_channel;
pub mod server;
pub(crate) mod status;
pub mod storage;
pub mod unlock;
pub(crate) mod wrapper_bindings;

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
