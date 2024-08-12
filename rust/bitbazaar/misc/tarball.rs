use std::{borrow::Cow, io::Read, path::Path};

use crate::prelude::*;

use super::Looper;

/// Decompress an in-memory .tar.gz to a hashmap of in-memory files.
///
/// Uses a callback to hide away some of the complexity of the tarball crate.
/// Mutable state can be passed to persist data in and out.
/// a bool should also be returned, if false, no further files will be processed.
pub fn tarball_decompress<S, R: Read>(
    src: R,
    mut state: S,
    mut with_file: impl FnMut(Looper<S, TarballFile<R>>) -> RResult<Looper<S, TarballFile<R>>, AnyErr>,
) -> RResult<S, AnyErr> {
    // Get rid of ".gz" first which is the gzip compression:
    let tar = flate2::read::GzDecoder::new(src);

    // Decode the tarball to get the inner files:
    let mut archive = tar::Archive::new(tar);
    for entry in archive.entries().change_context(AnyErr)? {
        let entry = entry.change_context(AnyErr)?;
        let looper = with_file(Looper::new(state, TarballFile(entry))).change_context(AnyErr)?;
        state = looper.state;
        if looper.stop_early {
            break;
        }
    }

    Ok(state)
}

/// A decompressed file from a tarball wrapped in a simpler interface.
/// Implements [`std::io::Read`] for easy lazy reading.
pub struct TarballFile<'a, R: 'a + std::io::Read>(tar::Entry<'a, flate2::read::GzDecoder<R>>);

impl<'a, R: 'a + std::io::Read> Read for TarballFile<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl<'a, R: 'a + std::io::Read> TarballFile<'a, R> {
    /// Get the path of the file in the tarball.
    pub fn path(&self) -> RResult<Cow<Path>, AnyErr> {
        self.0.path().change_context(AnyErr)
    }
}
