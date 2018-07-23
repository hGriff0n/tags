
use super::super::meta;

use std::fs;
use std::io::{Error, ErrorKind, Read, Seek, SeekFrom};
use std::path;
use std::rc;

use super::tag;

pub struct File {
    tag: rc::Rc<tag::Tag>,
}

impl File {
    #[allow(dead_code)]
    pub fn open<P: AsRef<path::Path>>(path: P) -> Result<Self, Error> {
        let mut file = fs::File::open(path)?;

        use self::Id3Version::*;
        if let (ID3v2, location) = find_mpeg_tag(&mut file)? {
            Ok(File{ tag: rc::Rc::new(tag::Tag::id3v2_from_file(&mut file, location)?) })
        } else {
            Err(Error::new(ErrorKind::Other, "Non-id3v2 tags are not supported"))
        }
        // let tag = match find_mpeg_tag(&mut file)? {
        //     (ID3v2, location) => Tag::from_id3v2(&mut file, location)?,
        //     (ID3v1, location) => Tag::from_id3v1(&mut file, location)?,
        //     (APE, location) => Tag::from_ape(&mut file, location)?,
        // };

        // Ok(File{ tag: rc::Rc::new(tag) })
    }
}

enum Id3Version {
    ID3v2,
    ID3v1,
    APE
}

impl meta::File for File {
    fn tag(&self) -> rc::Rc<meta::Tag> {
        self.tag.clone()
    }
}

fn find_mpeg_tag(file: &mut fs::File) -> Result<(Id3Version, u64), Error> {
    // Are all of the tags possible on one file ?
    if let Some(location) = find_id3v2(file)? {
        return Ok((Id3Version::ID3v2, location));
    }

    if let Some(location) = find_id3v1(file)? {
        return Ok((Id3Version::ID3v1, location));
    }

    if let Some(location) = find_ape(file)? {
        return Ok((Id3Version::APE, location));
    }

    Err(Error::new(ErrorKind::InvalidData, "Could not find mpeg tag location"))
}

fn find_id3v2(file: &mut fs::File) -> Result<Option<u64>, Error> {
    let header_id = vec!['I' as u8, 'D' as u8, '3' as u8];
    let mut buf = vec![0 as u8; header_id.len()];

    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut buf)?;

    if buf == header_id {
        return Ok(Some(0));
    }

    if buf[0] == 0xff && (buf[1] != 0xff && ((buf[1] & 0xe0) == 0xe0)) {
        return Ok(None)
    }

    Err(Error::new(ErrorKind::InvalidData, "ID3v2 tag possibly not at front of file"))
    // const long tagOffset = find(headerID);
    // if(tagOffset < 0)
    //     return -1;

    // const long frameOffset = firstFrameOffset();
    // if(frameOffset < tagOffset)
    //     return -1;

    // return tagOffset;
}

fn find_id3v1(_file: &mut fs::File) -> Result<Option<u64>, Error> {
    Ok(None)
}

fn find_ape(_file: &mut fs::File) -> Result<Option<u64>, Error> {
    Ok(None)
}
