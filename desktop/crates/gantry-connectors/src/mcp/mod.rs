//! The MCP runtime (docs/plan/03 §6): sessions, the `Connector` adaptation, and the risk
//! mapping for discovered tools. These three modules are the only ones that import rmcp.

pub mod connector;
pub mod risk;
pub mod session;

pub use connector::McpConnector;
pub use risk::tier_for;
pub use session::{Endpoint, McpError, McpSession};
