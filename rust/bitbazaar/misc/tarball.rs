use std::{
    borrow::Cow,
    io::{Read, Write},
    path::Path,
};

use crate::prelude::*;

use super::Looper;

/// Decompress a tarball (.tar.gz).
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

/// Compress a tarball (.tar.gz).
pub fn tarball_compress<'a, R: Read>(
    dest: impl Write,
    files: impl IntoIterator<Item = (&'a Path, R)>,
) -> RResult<(), AnyErr> {
    let mut tar_src = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut tar_src);
        for (path, mut data) in files {
            let mut buf = vec![];
            data.read_to_end(&mut buf).change_context(AnyErr)?;
            let mut header = tar::Header::new_gnu();
            header.set_size(buf.len() as u64);
            tar.append_data(&mut header, path, std::io::Cursor::new(buf))
                .change_context(AnyErr)?;
        }
        tar.finish().change_context(AnyErr)?;
    }
    let mut gz = flate2::write::GzEncoder::new(dest, flate2::Compression::default());
    gz.write_all(&tar_src).change_context(AnyErr)?;

    Ok(())
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_tarball() -> RResult<(), AnyErr> {
        // Create using compress function, checking:
        // - multiple files
        // - files in directories
        let mut tarball = Vec::new();
        tarball_compress(
            &mut tarball,
            [
                (Path::new("foo.txt"), "foo".as_bytes()),
                (Path::new("bar.txt"), "bar".as_bytes()),
                (Path::new("nested/ree.txt"), "ree".as_bytes()),
            ],
        )?;

        // Decompress using decompress function, checking:
        // - multiple files
        // - files in directories
        let mut files = HashMap::new();
        tarball_decompress(&tarball[..], (), |mut looper| {
            let mut buf = vec![];
            looper
                .value_mut()
                .read_to_end(&mut buf)
                .change_context(AnyErr)?;
            files.insert(looper.value().path()?.to_string_lossy().to_string(), buf);
            Ok(looper)
        })?;
        assert_eq!(files.len(), 3, "{:#?}", files);
        assert!(files.contains_key("foo.txt"), "{:#?}", files);
        assert!(files.contains_key("bar.txt"), "{:#?}", files);
        assert!(files.contains_key("nested/ree.txt"), "{:#?}", files);
        assert_eq!(files["foo.txt"], b"foo", "{:#?}", files);
        assert_eq!(files["bar.txt"], b"bar", "{:#?}", files);
        assert_eq!(files["nested/ree.txt"], b"ree", "{:#?}", files);

        // Confirm early exit works, call stop_early(), meaning only 1 file should be output:
        let mut files = HashMap::new();
        tarball_decompress(&tarball[..], (), |mut looper| {
            looper.stop_early();
            let mut buf = vec![];
            looper
                .value_mut()
                .read_to_end(&mut buf)
                .change_context(AnyErr)?;
            files.insert(looper.value().path()?.to_string_lossy().to_string(), buf);
            Ok(looper)
        })?;
        assert_eq!(files.len(), 1, "{:#?}", files);

        Ok(())
    }
}
