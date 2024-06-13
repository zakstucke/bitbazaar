/// Byte manipulation utilities, e.g. transfer speed.
pub mod bytes;

mod in_ci;
mod is_tcp_port_listening;

pub use in_ci::in_ci;
pub use is_tcp_port_listening::is_tcp_port_listening;
