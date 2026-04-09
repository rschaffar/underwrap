pub mod config;
pub mod exec;
pub mod ipc;
pub mod message;
pub mod policy;
pub mod scm_rights;

/// Default socket path for the control socket.
pub const CONTROL_SOCKET_PATH: &str = "/run/underwrap/control.sock";
