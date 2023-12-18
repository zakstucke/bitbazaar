use std::{error::Error, fmt};

#[derive(Debug)]
pub struct GenericErr {
    msg: String,
}

impl GenericErr {
    pub fn new<S: Into<String>>(msg: S) -> Self {
        Self { msg: msg.into() }
    }

    pub fn modify_msg<F>(&mut self, f: F)
    where
        F: FnOnce(&str) -> String,
    {
        self.msg = f(&self.msg);
    }
}

impl Error for GenericErr {}

impl fmt::Display for GenericErr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "GenericErr: {}", self.msg)
    }
}
