//! Host bindings generated from `plugin-api/wit`.
//!
//! Calls are synchronous: a Plugin call runs on a blocking thread, and
//! imports that wait on the network block that thread on the runtime. Every
//! import is trappable, so the host can end a call that breaks its bounds.

wasmtime::component::bindgen!({
    path: "../../../plugin-api/wit",
    world: "full-provider",
    imports: { default: trappable },
    with: {
        "ristle:plugin/connection.connection": crate::host::ConnectionHandle,
        "ristle:plugin/socket.tcp": crate::host::TcpHandle,
    },
});
