use crate::error::{Result, RunboxError};
use serde::{Deserialize, Serialize};
/// Process manager — gestiona procesos virtuales dentro del sandbox.
use std::collections::HashMap;

pub type Pid = u32;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProcessStatus {
    Running,
    Exited(i32),
    Killed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Process {
    pub pid: Pid,
    pub command: String,
    pub args: Vec<String>,
    pub status: ProcessStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug)]
pub struct ProcessManager {
    processes: HashMap<Pid, Process>,
    next_pid: Pid,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: HashMap::new(),
            next_pid: 1,
        }
    }

    pub fn spawn(&mut self, command: impl Into<String>, args: Vec<String>) -> Pid {
        let pid = self.next_pid;
        self.next_pid = self.next_pid.wrapping_add(1);
        if self.next_pid == 0 {
            self.next_pid = 1; // Skip PID 0 on overflow
        }
        self.processes.insert(
            pid,
            Process {
                pid,
                command: command.into(),
                args,
                status: ProcessStatus::Running,
                stdout: vec![],
                stderr: vec![],
            },
        );
        pid
    }

    pub fn get(&self, pid: Pid) -> Result<&Process> {
        self.processes
            .get(&pid)
            .ok_or_else(|| RunboxError::Process(format!("pid {pid} not found")))
    }

    pub fn get_mut(&mut self, pid: Pid) -> Result<&mut Process> {
        self.processes
            .get_mut(&pid)
            .ok_or_else(|| RunboxError::Process(format!("pid {pid} not found")))
    }

    pub fn exit(&mut self, pid: Pid, code: i32) -> Result<()> {
        self.get_mut(pid)?.status = ProcessStatus::Exited(code);
        Ok(())
    }

    pub fn kill(&mut self, pid: Pid) -> Result<()> {
        self.get_mut(pid)?.status = ProcessStatus::Killed;
        Ok(())
    }

    pub fn running(&self) -> Vec<&Process> {
        self.processes
            .values()
            .filter(|p| p.status == ProcessStatus::Running)
            .collect()
    }

    /// Removes all terminated (Exited or Killed) processes to free memory.
    pub fn cleanup(&mut self) {
        self.processes
            .retain(|_, p| p.status == ProcessStatus::Running);
    }

    /// Returns the total number of tracked processes.
    pub fn count(&self) -> usize {
        self.processes.len()
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}
