//! # Tasks
//!
//! `tasks` is the module to execute shell tasks
//! Tasks are more than a process, they're the entire execution pipeline of an expression
//! This means a task is made up of n processes which can be put in sequence

//
//   Shell-Core
//   Developed by Christian Visintin
//
// MIT License
// Copyright (c) 2020 Christian Visintin
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.
//

pub(crate) mod manager;
mod process;
pub mod task;

use crate::Redirection;
use process::Process;

use std::sync::{Arc, mpsc, Mutex};
use std::thread;

/// ## TaskErrorCode
///
/// The task error code represents the of error generated by the execution of a Task
///
/// - CoultNotStart: it was not possible to start the task. The command is invalid or you don't have enough permissions to run it
/// - IoError: there was an IO error due to the impossibility to redirect the IO
/// - BrokenPipe: the pipe broke
#[derive(Copy, Clone, PartialEq, std::fmt::Debug)]
pub enum TaskErrorCode {
    CouldNotStartTask,
    IoError,
    BrokenPipe,
    ProcessTerminated,
    KillError,
    AlreadyRunning
}

/// ## TaskError
///
/// The task error represents the error raised by a task. It is made up of the error code and of a certain message
#[derive(PartialEq, std::fmt::Debug)]
pub struct TaskError {
    code: TaskErrorCode,
    message: String,
}

/// ## Task
///
/// Task is the entity which describes a single Task and the relation with the next Task in the pipeline
#[derive(std::fmt::Debug)]
pub struct Task {
    pub(crate) command: Vec<String>,        //Command argv
    process: Option<Process>,               //Current process in task
    stdout_redirection: Redirection,        //Stdout Redirection type
    stderr_redirection: Redirection,        //Stderr Redirection type
    relation: TaskRelation,                 //Task Relation with the next one
    pub(crate) next: Option<Box<Task>>,     //Next process in task
    exit_code: Option<u8>,                  //Task exit code
}

/// ## TaskManager
///
/// TaskManager is the struct which handles the Task pipeline execution
pub(crate) struct TaskManager {
    running: Arc<Mutex<bool>>, //Running state
    joined: Arc<Mutex<bool>>, //Tells thread it can terminate
    m_loop: Option<thread::JoinHandle<u8>>, //Returns exitcode or TaskError in join handle
    receiver: Option<mpsc::Receiver<TaskMessageRx>>, //Receive messages from tasks
    sender: Option<mpsc::Sender<TaskMessageTx>>, //Sends Task messages
    next: Option<Task> //NOTE: Option because has to be taken by thread
}

/// ## TaskRelation
///
/// The task relation describes the behaviour the task manager should apply in the task execution for the next command
///
/// - And: the next command is executed only if the current one has finished with success; the task result is Ok, if all the commands are successful
/// - Or: the next command is executed only if the current one has failed; the task result is Ok if one of the two has returned with success
/// - Pipe: the commands are chained through a pipeline. This means they're executed at the same time and the output of the first is redirect to the output of the seconds one
/// - Unrelated: the commands are executed without any relation. The return code is the return code of the last command executed
#[derive(Copy, Clone, PartialEq, std::fmt::Debug)]
pub enum TaskRelation {
    And,
    Or,
    Pipe,
    Unrelated,
}

/// ## TaskMessageTx
/// 
/// Messages to be sent from shell to Task
pub(crate) enum TaskMessageTx {
    Input(String), //Send Input
    Kill, //Kill process
    Signal(crate::UnixSignal) //Send signal
}

/// ## TaskMessageRx
/// 
/// Messages to be sent from Task back to shell
pub(crate) enum TaskMessageRx {
    Output((Option<String>, Option<String>)), //Task Output (Stdout, Stderr)
    Error(TaskError) //Report error
}

//@! TaskError
impl TaskError {
    /// ## new
    ///
    /// Instantiate a new Task Error struct
    pub(crate) fn new(code: TaskErrorCode, message: String) -> TaskError {
        TaskError {
            code: code,
            message: message,
        }
    }
}
