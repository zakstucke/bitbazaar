#[cfg(test)]
use super::{generic_err::GenericErr, TracedErr};

#[cfg(test)]
pub fn create_err_from_err() -> TracedErr {
    TracedErr::from(GenericErr::new("Hello world"))
}

#[cfg(test)]
pub fn create_err_from_str(msg: String) -> TracedErr {
    TracedErr::from_str(msg)
}

#[cfg(test)]
use crate::err;
#[cfg(test)]
pub fn create_err_macro_from_str(msg: String) -> TracedErr {
    err!(msg)
}

#[cfg(test)]
pub fn create_err_macro_from_err() -> TracedErr {
    err!(GenericErr::new("Hello world"))
}
