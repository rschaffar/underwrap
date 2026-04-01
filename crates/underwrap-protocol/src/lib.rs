pub mod config;
pub mod ipc;
pub mod message;
pub mod policy;

/// Default socket paths.
pub const READ_SOCKET_PATH: &str = "/run/underwrap/read.sock";
pub const CONTROL_SOCKET_PATH: &str = "/run/underwrap/control.sock";
