pub mod client;
/// MCP — Model Context Protocol
/// RunBox actúa como servidor MCP (expone VFS, shell, console como tools/resources)
/// y como cliente MCP (se conecta a servidores externos).
pub mod protocol;
pub mod registry;
pub mod server;
pub mod transport;
