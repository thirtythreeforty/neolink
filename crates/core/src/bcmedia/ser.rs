use super::model::*;
use cookie_factory::bytes::*;
use cookie_factory::sequence::tuple;
use cookie_factory::{combinator::*, gen};
use cookie_factory::{GenError, SerializeFn, WriteContext};
use err_derive::Error;
use log::error;
use std::io::Write;

/// The error types used during serialisation
#[derive(Debug, Error)]
pub enum Error {
    /// A Cookie Factor  GenError
    #[error(display = "Cookie GenError")]
    GenError(#[error(source)] GenError),
}

impl BcMedia {
    pub(crate) fn serialize<W: Write>(&self, buf: W) -> Result<W, Error> {
        // match &self {}

        Ok(buf)
    }
}
