pub mod audit;
pub mod directory_channel;
pub mod secretsd_channel;
pub mod server;
pub mod storage;
pub mod unlock;

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
