
extern crate byteorder;

mod formats;

use formats::*;

use std::io;
use std::path::Path;

// Function to abstract out the encoding details of the specific file
pub fn get(path: &Path) -> Result<Box<File>, io::Error> {
    match path.extension().and_then(|ext| ext.to_str()) {
        // Some("mp3") => Ok(Box::new(mp3::File{})),
        Some("m4a") => Ok(Box::new(m4a::File::open(path)?)),
        Some("mp4") => Ok(Box::new(m4a::File::open(path)?)),
        _ => Err(io::Error::new(io::ErrorKind::Other, "Unimplemented"))
    }
}
