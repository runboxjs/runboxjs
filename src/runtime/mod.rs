pub mod bun;
pub mod deno;
pub mod git;
pub mod js_engine;
pub mod npm;
pub mod python;
pub mod shell_builtins;

use crate::error::Result;
use crate::process::ProcessManager;
use crate::shell::Command;
use crate::vfs::Vfs;

#[derive(Debug, Clone)]
pub struct ExecOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub exit_code: i32,
}

impl ExecOutput {
    pub fn stdout_str(&self) -> &str {
        std::str::from_utf8(&self.stdout).unwrap_or("[binary output]")
    }

    pub fn stderr_str(&self) -> &str {
        std::str::from_utf8(&self.stderr).unwrap_or("[binary output]")
    }
}

pub trait Runtime {
    fn name(&self) -> &'static str;
    fn exec(&self, cmd: &Command, vfs: &mut Vfs, pm: &mut ProcessManager) -> Result<ExecOutput>;
}
