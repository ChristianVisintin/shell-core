//! ## Process
//!
//! `process` is the module which takes care of executing processes and handling the process execution

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

extern crate nix;
extern crate subprocess;

use crate::UnixSignal;

//Fmt
use std::fmt;
//I/O
use std::io::{Read, Write};
//UNIX stuff
use nix::sys::select;
use nix::sys::signal;
use nix::sys::time::TimeVal;
use nix::sys::time::TimeValLike;
use nix::unistd::Pid;
use std::os::unix::io::IntoRawFd;
use std::os::unix::io::RawFd;
//Subprocess
use subprocess::{ExitStatus, Popen, PopenConfig, Redirection};

/// ### Process
///
/// Process represents a shell process execution instance
/// it contains the command and the arguments passed at start and the process pipe
#[derive(std::fmt::Debug)]
pub struct Process {
    pub command: String,
    pub args: Vec<String>,
    pub exit_status: Option<u8>,
    stdout_fd: Option<RawFd>,
    stderr_fd: Option<RawFd>,
    process: Popen,
}

#[derive(Copy, Clone, PartialEq, fmt::Debug)]
pub enum ProcessError {
    NoArgs,
    CouldNotStartProcess,
}

impl fmt::Display for ProcessError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let code_str: &str = match self {
            ProcessError::NoArgs => "Process was not provided of enough process",
            ProcessError::CouldNotStartProcess => "Could not start process",
        };
        write!(f, "{}", code_str)
    }
}

impl Process {
    /// ### exec
    ///
    /// Start a new process and returns a Process struct
    /// If process failed to start, returns a PopenError
    pub fn exec(argv: &Vec<String>) -> Result<Process, ProcessError> {
        if argv.len() == 0 {
            return Err(ProcessError::NoArgs);
        }
        let p = Popen::create(
            &argv,
            PopenConfig {
                stdin: Redirection::Pipe,
                stdout: Redirection::Pipe,
                stderr: Redirection::Pipe,
                detached: false,
                ..Default::default()
            },
        );
        let process: Popen = match p {
            Ok(p) => p,
            Err(_) => return Err(ProcessError::CouldNotStartProcess),
        };
        let command: String = String::from(&argv[0]);
        let mut args: Vec<String> = Vec::with_capacity(argv.len() - 1);
        if argv.len() > 1 {
            for arg in &argv[1..] {
                args.push(String::from(arg));
            }
        }
        Ok(Process {
            command: command,
            args: args,
            process: process,
            stdout_fd: None,
            stderr_fd: None,
            exit_status: None,
        })
    }

    /// ### read
    ///
    /// Read process output
    pub fn read(&mut self) -> std::io::Result<(Option<String>, Option<String>)> {
        //NOTE: WHY Not communicate? Well, because the author of this crate,
        //arbitrary decided that it would have been a great idea closing
        //the stream after calling communicate, so you can't read/write twice or more times to the process
        /*
        match self.process.communicate(Some("")) {
            Ok((stdout, stderr)) => Ok((stdout, stderr)),
            Err(err) => Err(err),
        }
        */
        /*
        NOTE: deleted due to blocking pipe; use select instead
        let mut stdout: &std::fs::File = &self.process.stdout.as_ref().unwrap();
        let mut output_byte: [u8; 8192] = [0; 8192];
        if let Err(err) = stdout.read(&mut output_byte) {
            return Err(err);
        }
        let raw_output: String = match std::str::from_utf8(&output_byte) {
            Ok(s) => String::from(s),
            Err(_) => return Err(std::io::Error::from(std::io::ErrorKind::InvalidData)),
        };
        //Trim null terminators
        let output = String::from(raw_output.trim_matches(char::from(0)));
        Ok((Some(output), None))
        */
        //Check if file descriptors exist
        let mut stdout: &std::fs::File = match &self.process.stdout.as_ref() {
            Some(out) => out,
            None => return Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe)),
        };
        let mut stderr: &std::fs::File = match &self.process.stderr.as_ref() {
            Some(err) => err,
            None => return Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe)),
        };
        //Set file descriptors if None
        if self.stderr_fd.is_none() {
            //Copy file descriptors and convert to raw fd
            let stderr_copy: std::fs::File = match stderr.try_clone() {
                Ok(f) => f,
                Err(err) => return Err(err),
            };
            self.stderr_fd = Some(stderr_copy.into_raw_fd());
        }
        if self.stdout_fd.is_none() {
            //Copy file descriptors and convert to raw fd
            let stdout_copy: std::fs::File = match stdout.try_clone() {
                Ok(f) => f,
                Err(err) => return Err(err),
            };
            self.stdout_fd = Some(stdout_copy.into_raw_fd());
        }
        //Prepare FD Set
        let mut rd_fdset: select::FdSet = select::FdSet::new();
        rd_fdset.insert(self.stdout_fd.unwrap());
        rd_fdset.insert(self.stderr_fd.unwrap());
        let mut timeout = TimeVal::milliseconds(50);
        let select_result = select::select(None, &mut rd_fdset, None, None, &mut timeout);
        //Select
        let mut stdout_str: Option<String> = None;
        let mut stderr_str: Option<String> = None;
        match select_result {
            Ok(fds) => match fds {
                0 => return Ok((None, None)),
                -1 => return Err(std::io::Error::from(std::io::ErrorKind::InvalidData)),
                _ => {
                    //Check if fd is set for stdout
                    if rd_fdset.contains(self.stdout_fd.unwrap()) {
                        //If stdout ISSET, read stdout
                        let mut output_byte: [u8; 8192] = [0; 8192];
                        if let Err(err) = stdout.read(&mut output_byte) {
                            return Err(err);
                        }
                        let raw_output: String = match std::str::from_utf8(&output_byte) {
                            Ok(s) => String::from(s),
                            Err(_) => {
                                return Err(std::io::Error::from(std::io::ErrorKind::InvalidData))
                            }
                        };
                        stdout_str = Some(String::from(raw_output.trim_matches(char::from(0))));
                    }
                    //Check if fd is set for stderr
                    if rd_fdset.contains(self.stderr_fd.unwrap()) {
                        //If stderr ISSET, read stderr
                        let mut output_byte: [u8; 8192] = [0; 8192];
                        if let Err(err) = stderr.read(&mut output_byte) {
                            return Err(err);
                        }
                        let raw_output: String = match std::str::from_utf8(&output_byte) {
                            Ok(s) => String::from(s),
                            Err(_) => {
                                return Err(std::io::Error::from(std::io::ErrorKind::InvalidData))
                            }
                        };
                        stderr_str = Some(String::from(raw_output.trim_matches(char::from(0))));
                    }
                }
            },
            Err(_) => return Err(std::io::Error::from(std::io::ErrorKind::InvalidData)),
        }
        Ok((stdout_str, stderr_str))
    }

    /// ### write
    ///
    /// Write input string to stdin
    pub fn write(&mut self, input: String) -> std::io::Result<()> {
        if self.process.stdin.is_none() {
            return Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
        }
        let mut stdin: &std::fs::File = &self.process.stdin.as_ref().unwrap();
        stdin.write_all(input.as_bytes())
    }

    /// ### is_running
    ///
    /// Returns whether the process is still running or not
    pub fn is_running(&mut self) -> bool {
        if self.exit_status.is_some() {
            return false; //Don't complicate it if you already know the result
        }
        match self.process.poll() {
            None => true,
            Some(exit_status) => {
                self.process.stderr = None;
                self.process.stdin = None;
                self.process.stdout = None;
                match exit_status {
                    //This is fu***** ridicoulous
                    ExitStatus::Exited(rc) => {
                        self.exit_status = Some(rc as u8);
                    }
                    ExitStatus::Signaled(rc) => {
                        self.exit_status = Some(rc);
                    }
                    ExitStatus::Other(rc) => {
                        self.exit_status = Some(rc as u8);
                    }
                    ExitStatus::Undetermined => {
                        self.exit_status = None;
                    }
                };
                false
            }
        }
    }

    /// ### pid
    ///
    /// Get process pid
    pub fn pid(&self) -> Option<u32> {
        self.process.pid()
    }

    /// ### raise
    ///
    /// Send a signal to the running process
    pub fn raise(&mut self, signal: UnixSignal) -> Result<(), ()> {
        let signal: signal::Signal = signal.to_nix_signal();
        match self.process.pid() {
            Some(pid) => {
                let unix_pid: Pid = Pid::from_raw(pid as i32);
                match signal::kill(unix_pid, signal) {
                    Ok(_) => {
                        //Wait timeout
                        match self
                            .process
                            .wait_timeout(std::time::Duration::from_millis(100))
                        {
                            Ok(exit_status_opt) => match exit_status_opt {
                                Some(exit_status) => match exit_status {
                                    //This is fu***** ridicoulous
                                    ExitStatus::Exited(rc) => {
                                        self.exit_status = Some(rc as u8);
                                    }
                                    ExitStatus::Signaled(rc) => {
                                        self.exit_status = Some(rc);
                                    }
                                    ExitStatus::Other(rc) => {
                                        self.exit_status = Some(rc as u8);
                                    }
                                    ExitStatus::Undetermined => {
                                        self.exit_status = None;
                                    }
                                },
                                None => {}
                            },
                            Err(_) => return Err(()),
                        }
                        Ok(())
                    }
                    Err(_) => Err(()),
                }
            }
            None => Err(()),
        }
    }

    /// ### kill
    ///
    /// Kill using SIGKILL the sub process
    pub fn kill(&mut self) -> Result<(), ()> {
        match self.process.kill() {
            Ok(_) => {
                match self.process.wait() {
                    Ok(exit_status) => match exit_status {
                        //This is fu***** ridicoulous
                        ExitStatus::Exited(rc) => {
                            self.exit_status = Some(rc as u8);
                        }
                        ExitStatus::Signaled(rc) => {
                            self.exit_status = Some(rc);
                        }
                        ExitStatus::Other(rc) => {
                            self.exit_status = Some(rc as u8);
                        }
                        ExitStatus::Undetermined => {
                            self.exit_status = None;
                        }
                    },
                    Err(_) => return Err(()),
                }
                Ok(())
            }
            Err(_) => Err(()),
        }
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.process.terminate();
        self.stderr_fd = None;
        self.stdout_fd = None;
    }
}

impl UnixSignal {

    /// ### to_nix_signal
    /// 
    /// Converts a UnixSignal to a nix::signal
    pub(self) fn to_nix_signal(&self) -> signal::Signal {
        match self {
            UnixSignal::Sigabrt => signal::Signal::SIGABRT,
            UnixSignal::Sigalrm => signal::Signal::SIGALRM,
            UnixSignal::Sigbus => signal::Signal::SIGBUS,
            UnixSignal::Sigchld => signal::Signal::SIGCHLD,
            UnixSignal::Sigcont => signal::Signal::SIGCONT,
            UnixSignal::Sigfpe => signal::Signal::SIGFPE,
            UnixSignal::Sighup => signal::Signal::SIGHUP,
            UnixSignal::Sigill => signal::Signal::SIGILL,
            UnixSignal::Sigint => signal::Signal::SIGINT,
            UnixSignal::Sigio => signal::Signal::SIGIO,
            UnixSignal::Sigkill => signal::Signal::SIGKILL,
            UnixSignal::Sigpipe => signal::Signal::SIGPIPE,
            UnixSignal::Sigprof => signal::Signal::SIGPROF,
            UnixSignal::Sigpwr => signal::Signal::SIGPWR,
            UnixSignal::Sigquit => signal::Signal::SIGQUIT,
            UnixSignal::Sigsegv => signal::Signal::SIGSEGV,
            UnixSignal::Sigstkflt => signal::Signal::SIGSTKFLT,
            UnixSignal::Sigstop => signal::Signal::SIGSTOP,
            UnixSignal::Sigsys => signal::Signal::SIGSYS,
            UnixSignal::Sigterm => signal::Signal::SIGTERM,
            UnixSignal::Sigtrap => signal::Signal::SIGTRAP,
            UnixSignal::Sigtstp => signal::Signal::SIGTSTP,
            UnixSignal::Sigttin => signal::Signal::SIGTTIN,
            UnixSignal::Sigttou => signal::Signal::SIGTTOU,
            UnixSignal::Sigurg => signal::Signal::SIGURG,
            UnixSignal::Sigusr1 => signal::Signal::SIGUSR1,
            UnixSignal::Sigusr2 => signal::Signal::SIGUSR2,
            UnixSignal::Sigvtalrm => signal::Signal::SIGVTALRM,
            UnixSignal::Sigwinch => signal::Signal::SIGWINCH,
            UnixSignal::Sigxcpu => signal::Signal::SIGXCPU,
            UnixSignal::Sigxfsz => signal::Signal::SIGXFSZ
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::time::{Duration, Instant};
    use std::thread::sleep;

    #[test]
    fn test_process_output_only() {
        let argv: Vec<String> = vec![
            String::from("echo"),
            String::from("foo"),
            String::from("bar"),
        ];
        let mut process: Process = match Process::exec(&argv) {
            Ok(p) => p,
            Err(error) => panic!("Could not start process 'echo foo bar': {}", error),
        };
        //We do not expect any input, go straight with the output
        let t_start_loop: Instant = Instant::now();
        loop {
            if t_start_loop.elapsed().as_millis() >= 5000 {
                break; //It's okay, on travis multi threading is just broken...
            }
            //Read stdout
            match process.read() {
                Ok((stdout, _)) => match stdout {
                    Some(output) => {
                        if output.len() == 0 {
                        } else {
                            println!("Echo Output: '{}'", output);
                            assert_eq!(output, String::from("foo bar\n"));
                        }
                    }
                    None => {}
                },
                Err(error) => {
                    panic!("Could not read process stdout: {}", error);
                }
            }
            //If process is not running, exit
            if !process.is_running() {
                break;
            }
        }
        println!(
            "Process exited with exit status: {}",
            process.exit_status.unwrap()
        );
        assert_eq!(process.exit_status.unwrap(), 0); //Should be 0
    }

    #[test]
    fn test_process_subprocess_io() {
        //the best and simplest example with this is CAT command :D
        let argv: Vec<String> = vec![String::from("cat")]; //No extra arg
        let mut process: Process = match Process::exec(&argv) {
            Ok(p) => p,
            Err(error) => panic!("Could not start process 'cat': {}", error),
        };
        //Check if running and waiting
        assert!(process.is_running());
        println!("cat process started");
        assert!(process.pid().is_some());
        //Write something, that should be echoed
        let input: String = String::from("Hello World!\n");
        if let Err(err) = process.write(input.clone()) {
            panic!("Could not write to cat stdin: {}", err);
        }
        println!("Wrote {}", input.clone());
        //Read, output should be equal to input
        match process.read() {
            Ok((stdout, _)) => match stdout {
                Some(output) => {
                    println!("Cat Output: '{}'", output);
                    assert_eq!(output, input);
                }
                None => {
                    panic!("No input from cat");
                }
            },
            Err(error) => {
                panic!("Could not read process stdout: {}", error);
            }
        }
        //Process should still be running
        assert!(process.is_running());
        //Write something else
        let input: String = String::from("I don't care if monday's blue!\nTuesday's gray and Wednesday too\nThursday I don't care about you\nIt's Friday I'm in love\n");
        if let Err(err) = process.write(input.clone()) {
            panic!("Could not write to cat stdin: {}", err);
        }
        println!("Wrote {}", input.clone());
        //Read, output should be equal to input
        match process.read() {
            Ok((stdout, _)) => match stdout {
                Some(output) => {
                    println!("Cat Output: '{}'", output);
                    assert_eq!(output, input);
                }
                None => {
                    panic!("No input from cat");
                }
            },
            Err(error) => {
                panic!("Could not read process stdout: {}", error);
            }
        }
        //Finally Send SIGINT
        if let Err(err) = process.raise(UnixSignal::Sigint) {
            panic!("Could not send SIGINT to cat process: {:?}", err);
        }
        //Process should be terminated
        assert!(!process.is_running());
        //Exit code should be 2
        assert_eq!(process.exit_status.unwrap(), 2);
    }

    #[test]
    fn test_process_kill() {
        let argv: Vec<String> = vec![String::from("yes")];
        let mut process: Process = match Process::exec(&argv) {
            Ok(p) => p,
            Err(error) => panic!("Could not start process 'yes': {}", error),
        };
        //Check if running and waiting
        assert!(process.is_running());
        println!("yes process started");
        //Kill process
        if let Err(err) = process.kill() {
            panic!("Could not kill 'yes' process: {:?}", err);
        }
        assert!(!process.is_running());
        //Exit code should be 9
        assert_eq!(process.exit_status.unwrap(), 9);
    }

    #[test]
    #[should_panic]
    fn test_process_no_argv() {
        let argv: Vec<String> = vec![];
        Process::exec(&argv).ok().unwrap();
    }

    #[test]
    #[should_panic]
    fn test_process_unknown_command() {
        let argv: Vec<String> = vec![String::from("piroporopero")];
        Process::exec(&argv).ok().unwrap();
    }

    #[test]
    #[should_panic]
    fn test_process_terminated_write() {
        let argv: Vec<String> = vec![String::from("echo"), String::from("0")];
        let mut process: Process = match Process::exec(&argv) {
            Ok(p) => p,
            Err(error) => panic!("Could not start process 'echo foo bar': {}", error),
        };
        let t_start_loop: Instant = Instant::now();
        loop {
            if t_start_loop.elapsed().as_millis() >= 5000 {
                panic!("Echo command timeout"); //It's okay, on travis multi threading is just broken...
            }
            if !process.is_running() {
                println!("Okay, echo has terminated!");
                break;
            }
        }
        sleep(Duration::from_millis(500));
        process.write(String::from("foobar")).ok().unwrap();
    }

    #[test]
    #[should_panic]
    fn test_process_terminated_read() {
        let argv: Vec<String> = vec![String::from("echo"), String::from("0")];
        let mut process: Process = match Process::exec(&argv) {
            Ok(p) => p,
            Err(error) => panic!("Could not start process 'echo foo bar': {}", error),
        };
        let t_start_loop: Instant = Instant::now();
        loop {
            if t_start_loop.elapsed().as_millis() >= 5000 {
                panic!("Echo command timeout"); //It's okay, on travis multi threading is just broken...
            }
            if !process.is_running() {
                println!("Okay, echo has terminated!");
                break;
            }
        }
        sleep(Duration::from_millis(500));
        process.read().ok().unwrap();
    }

    #[test]
    #[should_panic]
    fn test_process_stderr_broken_pipe() {
        let argv: Vec<String> = vec![String::from("echo"), String::from("0")];
        let mut process: Process = match Process::exec(&argv) {
            Ok(p) => p,
            Err(error) => panic!("Could not start process 'echo foo bar': {}", error),
        };
        process.process.stderr = None;
        process.read().ok().unwrap();
    }

    #[test]
    fn test_process_signaled() {
        let argv: Vec<String> = vec![String::from("cat")];
        let mut process: Process = match Process::exec(&argv) {
            Ok(p) => p,
            Err(error) => panic!("Could not start process 'echo foo bar': {}", error),
        };
        let unix_pid: Pid = Pid::from_raw(process.process.pid().unwrap() as i32);
        signal::kill(unix_pid, signal::Signal::SIGINT).expect("Failed to kill process");
        sleep(Duration::from_millis(500));
        //Process should be terminated
        assert!(!process.is_running());
        //Exit code should be 2
        assert_eq!(process.exit_status.unwrap(), 2);
    }

    #[test]
    fn test_process_display_error() {
        println!("{}; {}", ProcessError::CouldNotStartProcess, ProcessError::NoArgs);
    }

    #[test]
    fn test_process_unix_signals() {
        assert_eq!(UnixSignal::Sigabrt.to_nix_signal(), signal::SIGABRT);
        assert_eq!(UnixSignal::Sighup.to_nix_signal(), signal::SIGHUP);
        assert_eq!(UnixSignal::Sigint.to_nix_signal(), signal::SIGINT);
        assert_eq!(UnixSignal::Sigquit.to_nix_signal(), signal::SIGQUIT);
        assert_eq!(UnixSignal::Sigill.to_nix_signal(), signal::SIGILL);
        assert_eq!(UnixSignal::Sigtrap.to_nix_signal(), signal::SIGTRAP);
        assert_eq!(UnixSignal::Sigbus.to_nix_signal(), signal::SIGBUS);
        assert_eq!(UnixSignal::Sigfpe.to_nix_signal(), signal::SIGFPE);
        assert_eq!(UnixSignal::Sigkill.to_nix_signal(), signal::SIGKILL);
        assert_eq!(UnixSignal::Sigusr1.to_nix_signal(), signal::SIGUSR1);
        assert_eq!(UnixSignal::Sigsegv.to_nix_signal(), signal::SIGSEGV);
        assert_eq!(UnixSignal::Sigusr2.to_nix_signal(), signal::SIGUSR2);
        assert_eq!(UnixSignal::Sigpipe.to_nix_signal(), signal::SIGPIPE);
        assert_eq!(UnixSignal::Sigalrm.to_nix_signal(), signal::SIGALRM);
        assert_eq!(UnixSignal::Sigterm.to_nix_signal(), signal::SIGTERM);
        assert_eq!(UnixSignal::Sigstkflt.to_nix_signal(), signal::SIGSTKFLT);
        assert_eq!(UnixSignal::Sigchld.to_nix_signal(), signal::SIGCHLD);
        assert_eq!(UnixSignal::Sigcont.to_nix_signal(), signal::SIGCONT);
        assert_eq!(UnixSignal::Sigstop.to_nix_signal(), signal::SIGSTOP);
        assert_eq!(UnixSignal::Sigtstp.to_nix_signal(), signal::SIGTSTP);
        assert_eq!(UnixSignal::Sigttin.to_nix_signal(), signal::SIGTTIN);
        assert_eq!(UnixSignal::Sigttou.to_nix_signal(), signal::SIGTTOU);
        assert_eq!(UnixSignal::Sigurg.to_nix_signal(), signal::SIGURG);
        assert_eq!(UnixSignal::Sigxcpu.to_nix_signal(), signal::SIGXCPU);
        assert_eq!(UnixSignal::Sigxfsz.to_nix_signal(), signal::SIGXFSZ);
        assert_eq!(UnixSignal::Sigvtalrm.to_nix_signal(), signal::SIGVTALRM);
        assert_eq!(UnixSignal::Sigprof.to_nix_signal(), signal::SIGPROF);
        assert_eq!(UnixSignal::Sigwinch.to_nix_signal(), signal::SIGWINCH);
        assert_eq!(UnixSignal::Sigio.to_nix_signal(), signal::SIGIO);
        assert_eq!(UnixSignal::Sigpwr.to_nix_signal(), signal::SIGPWR);
        assert_eq!(UnixSignal::Sigsys.to_nix_signal(), signal::SIGSYS);
    }
}
