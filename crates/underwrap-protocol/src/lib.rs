pub mod config;
pub mod exec;
pub mod ipc;
pub mod message;
pub mod policy;
pub mod scm_rights;

/// Default socket path for the control socket.
pub const CONTROL_SOCKET_PATH: &str = "/run/underwrap/control.sock";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_socket_path_is_absolute() {
        assert!(CONTROL_SOCKET_PATH.starts_with('/'));
        assert!(CONTROL_SOCKET_PATH.ends_with("control.sock"));
    }
}
