use crate::execution::file::*;
use crate::executor::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// The identifier of an execution, it's globally unique and it identifies an
/// execution only during a single evaluation.
pub type ExecutionUuid = Uuid;

/// Type of the callback called when an Execution starts
pub type OnStartCallback = Fn(WorkerUuid) -> ();

/// Type of the callback called when an Execution ends
pub type OnDoneCallback = Fn(WorkerResult) -> ();

/// Type of the callback called when an Execution is skipped
pub type OnSkipCallback = Fn() -> ();

/// Command of an Execution to execute
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionCommand {
    /// A system command, the workers will search in their PATH for the
    /// executable if it's relative
    System(String),
    /// A command relative to the sandbox directory
    Local(String),
}

/// An input of an Execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionInput {
    /// Path relative to the sandbox directory
    pub path: PathBuf,
    /// Uuid of the file
    pub file: FileUuid,
    /// Whether this file should be marked as executable
    pub executable: bool,
}

/// The supported callbacks of an execution
pub struct ExecutionCallbacks {
    pub on_start: Option<Box<OnStartCallback>>,
    pub on_done: Option<Box<OnDoneCallback>>,
    pub on_skip: Option<Box<OnSkipCallback>>,
}

/// An Execution is a process that will be executed by a worker inside a
/// sandbox
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Execution {
    /// Uuid of the execution
    pub uuid: ExecutionUuid,
    /// Description of the execution
    pub description: String,
    /// Which command to execute
    pub command: ExecutionCommand,
    /// The list of command line arguments
    pub args: Vec<String>,

    /// Optional standard input to pass to the program
    pub stdin: Option<FileUuid>,
    /// Optional standard output to capture
    pub stdout: Option<File>,
    /// Optional standard error to capture
    pub stderr: Option<File>,
    /// List of input files that should be put inside the sandbox
    pub inputs: Vec<ExecutionInput>, // TODO change to HashMap
    /// List of the output files that should be capture from the sandbox
    pub outputs: HashMap<PathBuf, File>,

    /// Limits on the execution
    pub limits: ExecutionLimits,
}

/// Limits on the execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLimits {
    /// Limit on the userspace cpu time of the process
    pub cpu_time: Option<f32>,
    /// Limit on the kernelspace cpu time of the process
    pub sys_time: Option<f32>,
    /// Limit on the total time of execution
    pub wall_time: Option<f32>,
    /// Limit on the number of KiB the process can use in any moment
    pub memory: Option<u64>,
    /// Limit on the number of threads/processes the process can spawn
    pub nproc: Option<u32>,
    /// Limit on the number of file descriptors the process can keep open
    pub nofile: Option<u32>,
    /// Maximum size of the files (in bytes) the process can write/create
    pub fsize: Option<u64>,
    /// RLIMIT_MEMLOCK
    pub memlock: Option<u64>,
    /// Limit on the stack size for the process. 0 means unlimited.
    pub stack: Option<u64>,
}

/// Status of a completed Execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionStatus {
    /// The program exited with 0 within the limits
    Success,
    /// The program exited with a non-zero status code
    ReturnCode(u32),
    /// The program stopped due to a signal
    Signal(u32, String),
    /// The program hasn't exited within the time limit constraint
    TimeLimitExceeded,
    /// The program hasn't exited within the sys time limit constraint
    SysTimeLimitExceeded,
    /// The program hasn't exited within the wall time limit constraint
    WallTimeLimitExceeded,
    /// The program has exceeded the memory limit
    MemoryLimitExceeded,
    /// The sandbox failed to execute the program
    InternalError(String),
}

/// Resources used during the execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResourcesUsage {
    /// Number of seconds the process used in userspace
    pub cpu_time: f32,
    /// Number of seconds the process used in kernelspace
    pub sys_time: f32,
    /// Number of seconds from the start to the end of the process
    pub wall_time: f32,
    /// Number of KiB used at most by the process
    pub memory: u64,
}

/// The result of an execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Uuid of the completed execution
    pub uuid: ExecutionUuid,
    /// Status of the completed execution
    pub status: ExecutionStatus,
    /// Resources used by the execution
    pub resources: ExecutionResourcesUsage,
}

impl ExecutionLimits {
    /// Make an empty limits where all the limits are disabled. You may want to
    /// use `default` instead of this
    pub fn new() -> ExecutionLimits {
        ExecutionLimits {
            cpu_time: None,
            sys_time: None,
            wall_time: None,
            memory: None,
            nproc: None,
            nofile: None,
            fsize: None,
            memlock: None,
            stack: None,
        }
    }

    /// Set the cpu time limit
    pub fn cpu_time(&mut self, limit: f32) -> &mut Self {
        self.cpu_time = Some(limit);
        self
    }

    /// Set the sys time limit
    pub fn sys_time(&mut self, limit: f32) -> &mut Self {
        self.sys_time = Some(limit);
        self
    }

    /// Set the wall time limit
    pub fn wall_time(&mut self, limit: f32) -> &mut Self {
        self.sys_time = Some(limit);
        self
    }

    /// Set the memory limit
    pub fn memory(&mut self, limit: u64) -> &mut Self {
        self.memory = Some(limit);
        self
    }

    /// Set the nproc limit
    pub fn nproc(&mut self, limit: u32) -> &mut Self {
        self.nproc = Some(limit);
        self
    }

    /// Set the nofile limit
    pub fn nofile(&mut self, limit: u32) -> &mut Self {
        self.nofile = Some(limit);
        self
    }

    /// Set the fsize limit
    pub fn fsize(&mut self, limit: u64) -> &mut Self {
        self.fsize = Some(limit);
        self
    }

    /// Set the memlock limit
    pub fn memlock(&mut self, limit: u64) -> &mut Self {
        self.memlock = Some(limit);
        self
    }

    /// Set the stack limit
    pub fn stack(&mut self, limit: u64) -> &mut Self {
        self.stack = Some(limit);
        self
    }
}

impl std::default::Default for ExecutionLimits {
    /// Default sane values for the execution limits, the limits listed here
    /// should be safe enough for most of the executions.
    fn default() -> Self {
        ExecutionLimits {
            cpu_time: None,
            sys_time: None,
            wall_time: None,
            memory: None,
            nproc: Some(1),
            nofile: None,
            fsize: Some(1024u64.pow(3)),
            memlock: None,
            stack: Some(0),
        }
    }
}

impl Execution {
    /// Create a basic Execution
    pub fn new(description: &str, command: ExecutionCommand) -> Execution {
        Execution {
            uuid: Uuid::new_v4(),

            description: description.to_owned(),
            command,
            args: vec![],

            stdin: None,
            stdout: None,
            stderr: None,
            inputs: vec![],
            outputs: HashMap::new(),

            limits: ExecutionLimits::default(),
        }
    }

    /// List of all the File dependencies of the execution
    pub fn dependencies(&self) -> Vec<FileUuid> {
        let mut deps = vec![];
        if let Some(stdin) = self.stdin {
            deps.push(stdin);
        }
        for input in self.inputs.iter() {
            deps.push(input.file);
        }
        deps
    }

    /// List of all the File produced by the execution
    pub fn outputs(&self) -> Vec<FileUuid> {
        let mut outs = vec![];
        if let Some(stdout) = &self.stdout {
            outs.push(stdout.uuid.clone());
        }
        if let Some(stderr) = &self.stderr {
            outs.push(stderr.uuid.clone());
        }
        for output in self.outputs.values() {
            outs.push(output.uuid.clone());
        }
        outs
    }

    /// Bind the standard input to the specified file. Calling again this
    /// method will overwrite the previous value
    pub fn stdin(&mut self, stdin: &File) -> &mut Self {
        self.stdin = Some(stdin.uuid.clone());
        self
    }

    /// Handle to the standard output of the execution. This should be called
    /// at least once before the evaluation starts in order to track the file
    pub fn stdout(&mut self) -> File {
        if self.stdout.is_none() {
            let file = File::new(&format!("Stdout of '{}'", self.description));
            self.stdout = Some(file);
        }
        self.stdout.as_ref().unwrap().clone()
    }

    /// Handle to the standard error of the execution. This should be called
    /// at least once before the evaluation starts in order to track the file
    pub fn stderr(&mut self) -> File {
        if self.stderr.is_none() {
            let file = File::new(&format!("Stderr of '{}'", self.description));
            self.stderr = Some(file);
        }
        self.stderr.as_ref().unwrap().clone()
    }

    /// Bind a file inside the sandbox to the specified file. Calling again this
    /// method will overwrite the previous value
    pub fn input(&mut self, file: &File, path: &Path, executable: bool) -> &mut Self {
        self.inputs.push(ExecutionInput {
            path: path.to_owned(),
            file: file.uuid.clone(),
            executable,
        });
        self
    }

    /// Handle to a file produced by the execution. This should be called at
    /// least once before the evaluation starts in order to track the file
    pub fn output(&mut self, path: &Path) -> File {
        if self.outputs.contains_key(path) {
            return self.outputs.get(path).unwrap().clone();
        }
        let file = File::new(&format!("Output of '{}' at {:?}", self.description, path));
        self.outputs.insert(path.to_owned(), file);
        self.outputs.get(path).unwrap().clone()
    }

    /// Set the limits for the execution
    pub fn limits(&mut self, limits: ExecutionLimits) -> &mut Self {
        self.limits = limits;
        self
    }
}

impl std::fmt::Debug for ExecutionCallbacks {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> Result<(), std::fmt::Error> {
        formatter
            .debug_struct("ExecutionCallbacks")
            .field("on_start", &self.on_start.is_some())
            .field("on_done", &self.on_done.is_some())
            .field("on_skip", &self.on_skip.is_some())
            .finish()?;
        Ok(())
    }
}

impl std::default::Default for ExecutionCallbacks {
    fn default() -> Self {
        ExecutionCallbacks {
            on_start: None,
            on_done: None,
            on_skip: None,
        }
    }
}
