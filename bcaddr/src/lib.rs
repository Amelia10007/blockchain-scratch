use blockchain_core::SecretAddress;
use std::fmt::{self, Display, Formatter};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

pub fn read_address(path: impl AsRef<Path>) -> Result<SecretAddress, Error> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut buf = vec![];
    reader.read_to_end(&mut buf)?;
    let address = bincode::deserialize(&buf)?;

    Ok(address)
}

pub fn write_address(path: impl AsRef<Path>, addr: &SecretAddress) -> Result<(), Error> {
    let file = File::create(path)?;
    let mut writer = BufWriter::new(file);
    let mut buf = bincode::serialize(addr)?;
    writer.write_all(&mut buf)?;

    Ok(())
}

#[derive(Debug)]
pub enum Error {
    IO(std::io::Error),
    Serde(bincode::Error),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IO(e)
    }
}

impl From<bincode::Error> for Error {
    fn from(e: bincode::Error) -> Self {
        Error::Serde(e)
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Error::IO(e) => e.fmt(f),
            Error::Serde(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::IO(e) => Some(e),
            Error::Serde(e) => Some(e),
        }
    }
}
