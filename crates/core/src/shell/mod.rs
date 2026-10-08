//! The engine behind the in-app terminal: a stateful REPL over any
//! [`DeviceBackend`], ported from the web prototype's `cli.ts`.

pub mod exec;

pub use exec::{OutputKind, ShellLine, ShellReply, ShellSession};
