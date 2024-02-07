use std::{
    io::{Read, Write},
    process,
};

use conch_parser::ast::{self, TopLevelWord};

use super::{
    errs::ShellErr,
    runner::{ConcreteOutput, RunnerCmdOut},
    shell::Shell,
};
use crate::prelude::*;

pub fn handle_redirect(
    shell: &mut Shell,
    last_out: Option<&mut RunnerCmdOut>,
    redirect: ast::DefaultRedirect,
) -> Result<RunnerCmdOut, ShellErr> {
    Ok(match redirect {
        ast::Redirect::Write(fd, name) => {
            let dest = Target::new(shell, name)?.set_write();
            Data::new(last_out, fd)?.submit(dest)?
        }
        ast::Redirect::Append(fd, name) => {
            let dest = Target::new(shell, name)?.set_write().set_append();
            Data::new(last_out, fd)?.submit(dest)?
        }
        ast::Redirect::DupWrite(fd, name) => {
            let dest = Target::new(shell, name)?.set_write();
            Data::new(last_out, fd)?.submit(dest)?
        }
        ast::Redirect::Read(fd, name) => {
            let dest = Target::new(shell, name)?.set_read();
            Data::new(last_out, fd)?.submit(dest)?
        }
        ast::Redirect::DupRead(fd, name) => {
            let dest = Target::new(shell, name)?.set_read();
            Data::new(last_out, fd)?.submit(dest)?
        }
        ast::Redirect::ReadWrite(..) => {
            return Err(err!(
                ShellErr::BashFeatureUnsupported,
                "read-write redirection is not supported"
            ))
        }
        ast::Redirect::Heredoc(_, _) => {
            return Err(err!(
                ShellErr::BashFeatureUnsupported,
                "heredoc redirection is not supported"
            ))
        }
        ast::Redirect::Clobber(..) => {
            return Err(err!(
                ShellErr::BashFeatureUnsupported,
                "clobber redirection is not supported"
            ))
        }
    })
}

struct Target {
    variant: TargetVariant,
    write: bool,
    append: bool,
    read: bool,
}

enum TargetVariant {
    Stdout,
    Stderr,
    File(String),
    Null,
}

impl Target {
    fn new(shell: &mut Shell, name: TopLevelWord<String>) -> Result<Self, ShellErr> {
        let name = shell.process_complex_word(&name.0)?;

        if name == "/dev/stdin" || name == "0" {
            return Err(err!(
                ShellErr::BashFeatureUnsupported,
                "stdin redirection is not supported"
            ));
        } else if name.starts_with("/dev/tcp") || name.starts_with("/dev/udp") {
            return Err(err!(
                ShellErr::BashFeatureUnsupported,
                "network redirection is not supported"
            ));
        }

        let variant = {
            if let Some(fd) = name.strip_prefix("/dev/fd/") {
                match fd {
                    "0" => {
                        return Err(err!(
                            ShellErr::BashFeatureUnsupported,
                            "stdin redirection is not supported"
                        ))
                    }
                    "1" => TargetVariant::Stdout,
                    "2" => TargetVariant::Stderr,
                    _ => {
                        return Err(err!(
                            ShellErr::BashFeatureUnsupported,
                            "unsupported file descriptor: {}",
                            fd
                        ))
                    }
                }
            } else {
                match name.as_str() {
                    "/dev/stdout" => TargetVariant::Stdout,
                    "1" => TargetVariant::Stdout,
                    "/dev/stderr" => TargetVariant::Stderr,
                    "2" => TargetVariant::Stderr,
                    "/dev/null" => TargetVariant::Null,
                    _ => TargetVariant::File(name),
                }
            }
        };

        Ok(Self {
            variant,
            write: false,
            append: false,
            read: false,
        })
    }

    fn set_write(mut self) -> Self {
        self.write = true;
        self
    }

    fn set_append(mut self) -> Self {
        self.append = true;
        self
    }

    fn set_read(mut self) -> Self {
        self.read = true;
        self
    }
}

enum Data {
    StdoutHandle(process::ChildStdout),
    StderrHandle(process::ChildStderr),
    String(String),
    None,
}

impl Data {
    fn new(last: Option<&mut RunnerCmdOut>, fd: Option<u16>) -> Result<Self, ShellErr> {
        let fd = fd.unwrap_or(1);

        Ok(match fd {
            1 => {
                if let Some(last) = last {
                    match last {
                        RunnerCmdOut::Concrete(conc) => {
                            Self::String(conc.stdout.take().unwrap_or_default())
                        }
                        RunnerCmdOut::Pending(child) => {
                            if let Some(h) = child.stdout.take() {
                                Self::StdoutHandle(h)
                            } else {
                                Self::None
                            }
                        }
                    }
                } else {
                    Self::None
                }
            }
            2 => {
                if let Some(last) = last {
                    match last {
                        RunnerCmdOut::Concrete(conc) => {
                            Self::String(conc.stderr.take().unwrap_or_default())
                        }
                        RunnerCmdOut::Pending(child) => {
                            if let Some(h) = child.stderr.take() {
                                Self::StderrHandle(h)
                            } else {
                                Self::None
                            }
                        }
                    }
                } else {
                    Self::None
                }
            }
            // Not sure how other file descriptors would work currently, much less important than stderr and stdout.
            fd => {
                return Err(err!(
                    ShellErr::BashFeatureUnsupported,
                    "unsupported file descriptor: {}",
                    fd
                ))
            }
        })
    }

    fn submit(self, dest: Target) -> Result<RunnerCmdOut, ShellErr> {
        let mut conc = ConcreteOutput::default();

        match dest.variant {
            TargetVariant::Stdout => {
                if dest.write {
                    let mut buf = Vec::new();
                    self.write(&mut buf)?;
                    conc.stdout =
                        Some(String::from_utf8(buf).change_context(ShellErr::InternalError)?);
                }
            }
            TargetVariant::Stderr => {
                if dest.write {
                    let mut buf = Vec::new();
                    self.write(&mut buf)?;
                    conc.stderr =
                        Some(String::from_utf8(buf).change_context(ShellErr::InternalError)?);
                }
            }
            TargetVariant::File(name) => {
                let mut opts = std::fs::OpenOptions::new();
                if dest.read {
                    opts.read(true);
                }

                if dest.write {
                    opts.write(true).create(true);
                }

                if dest.append {
                    if !dest.write {
                        return Err(err!(
                            ShellErr::InternalError,
                            "write should definitely be true with append."
                        ));
                    }
                    opts.append(true);
                }

                let mut file = opts.open(name).change_context(ShellErr::InternalError)?;

                if dest.write {
                    self.write(&mut file)?;
                }

                // Read the contents to stdout (which would be used as stdin for the next command in a pipeline)
                if dest.read {
                    let mut buf = String::new();
                    file.read_to_string(&mut buf)
                        .change_context(ShellErr::InternalError)?;
                    conc.stdout = Some(buf);
                }
            }
            TargetVariant::Null => {}
        }

        Ok(RunnerCmdOut::Concrete(conc))
    }

    fn write(self, mut writer: impl Write) -> Result<(), ShellErr> {
        match self {
            Data::StdoutHandle(mut h) => {
                std::io::copy(&mut h, &mut writer).change_context(ShellErr::InternalError)?;
            }
            Data::StderrHandle(mut h) => {
                std::io::copy(&mut h, &mut writer).change_context(ShellErr::InternalError)?;
            }
            Data::String(s) => {
                writer
                    .write_all(s.as_bytes())
                    .change_context(ShellErr::InternalError)?;
            }
            Data::None => {}
        }

        Ok(())
    }
}
