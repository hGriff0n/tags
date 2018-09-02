#![allow(dead_code)]
#![allow(unused_imports)]

use formats::meta;
use super::frame;

use std::collections::HashMap;
use std::fs;
use std::io::{Error, ErrorKind, Read, Seek, SeekFrom};
use std::mem;
use std::rc;

use byteorder::{BigEndian, ByteOrder};


pub struct Tag {
    frame_map: HashMap<String, frame::SubClass>,
}

impl meta::Tag for Tag {
    fn title(&self) -> Option<String> {
        if let Some(frame::SubClass::Text(title, _)) = self.frame_map.get("TIT2") {
            Some(title.to_string())
        } else {
            None
        }
    }
    fn artist(&self) -> Option<String> {
        if let Some(frame::SubClass::Text(artist, _)) = self.frame_map.get("TPE1") {
            Some(artist.to_string())
        } else {
            None
        }
    }
    fn album(&self) -> Option<String> {
        if let Some(frame::SubClass::Text(album, _)) = self.frame_map.get("TALB") {
            Some(album.to_string())
        } else {
            None
        }
    }
    fn year(&self) -> Option<u64> {
        if let Some(frame::SubClass::Uint(year)) = self.frame_map.get("TDRC") {
            Some(*year)
        } else {
            None
        }
    }
    fn comment(&self) -> Option<String> {
        if let Some(frame::SubClass::Text(comment, _)) = self.frame_map.get("COMM") {
            Some(comment.to_string())
        } else {
            None
        }
    }
    fn track(&self) -> Option<u32> {
        if let Some(frame::SubClass::Uint(track)) = self.frame_map.get("TRCK") {
            Some(*track as u32)
        } else {
            None
        }
    }

    // TODO: This needs to be built up when I construct the tag
    fn genre(&self) -> Option<String> {
        if let Some(frame::SubClass::Text(genre, _)) = self.frame_map.get("TCON") {
            Some(genre.to_string())
        } else {
            None
        }
    }
}

impl Tag {
    // TODO: Improve the process for unifying id3v1 and id3v2 tags
    pub fn unify(tags: Vec<rc::Rc<Self>>) -> Self {
        let mut ret_tag = Self::default();

        for tag in tags {
            for (key, value) in &tag.frame_map {

                use std::collections::hash_map::Entry::*;
                match ret_tag.frame_map.entry(key.to_string()) {
                    // TODO: Temporary workaround for bad utf16 parsing (see frame.rs)
                    Occupied(mut ent) => {
                        match ent.get() {
                            frame::SubClass::Text(_, frame::StringType::UTF16) => { ent.insert(value.clone()); },
                            _ => ()
                        };
                    },
                    Vacant(ent) => { ent.insert(value.clone()); },
                };
            }
        }

        ret_tag
    }

    pub fn id3v2_from_file(file: &mut fs::File, offset: u64) -> Result<Self, Error> {
        file.seek(SeekFrom::Start(offset))?;

        let mut header = vec![0; 10];
        file.read_exact(&mut header)?;

        let header = parse_tag_header(&header)?;
        if header.size != 0 {
            let mut buf = vec![0; header.size as usize];
            file.read_exact(&mut buf)?;
            return Tag::from_buffer(&mut buf, &header);

        }

        // TODO: This could only be causing us to "skip" some stuff, not fail at parsing
        // NOTE: Taglib has some stuff about ignoring duplicate flags (I'm ignoring that for now)

        Err(Error::new(ErrorKind::InvalidData, "Tags must contain at least 1 frame"))
    }

    pub fn id3v1_from_file(file: &mut fs::File, offset: u64) -> Result<Self, Error> {
        file.seek(SeekFrom::Start(offset))?;

        let mut block = vec![0; 128];
        file.read_exact(&mut block)?;

        let mut tag = Tag{ frame_map: HashMap::new() };
        // from_ascii(&block[3..33]);

        use self::frame::StringType;

        tag.frame_map.insert("TIT2".to_string(), frame::SubClass::Text(id3::from_ascii(&block[3..33]), StringType::UTF8));
        tag.frame_map.insert("TPE1".to_string(), frame::SubClass::Text(id3::from_ascii(&block[33..63]), StringType::UTF8));
        tag.frame_map.insert("TALB".to_string(), frame::SubClass::Text(id3::from_ascii(&block[63..93]), StringType::UTF8));
        // tag.frame_map.insert("TDRC".to_string(), frame::SubClass::Uint(&block[93..97]));

        if block[125] == 0 && block[126] != 0 {
            tag.frame_map.insert("COMM".to_string(), frame::SubClass::Text(id3::from_ascii(&block[97..125]), StringType::UTF8));
            tag.frame_map.insert("TRCK".to_string(), frame::SubClass::Uint(block[126] as u64));
        } else {
            tag.frame_map.insert("COMM".to_string(), frame::SubClass::Text(id3::from_ascii(&block[97..127]), StringType::UTF8));
        }

        // tag.frame_map.insert("TCON".to_string(), frame::SubClass::Uint(block[127] as u64));

        Ok(tag)

    }

    fn from_buffer(buf: &mut Vec<u8>, header: &TagHeader) -> Result<Self, Error> {
        if header.unsynch && header.major_version <= 3 {
            let mut tmp_vec = synch::decode(&buf);
            mem::swap(buf, &mut tmp_vec);
        }

        let mut pos = 0;
        let mut buf_end = buf.len();

        // TODO: Parse extended header
        let _ext_header = if header.extended {
            // taglib:id3v2tag.cpp:904
            /*
            d->extendedHeader = new ExtendedHeader();
            d->extendedHeader->setData(data);
            if(d->extendedHeader->size() <= data.size()) {
                frameDataPosition += d->extendedHeader->size();
                frameDataLength -= d->extendedHeader->size();
            }
             */
            ()
        };

        if header.footer && sizeof_footer() <= buf_end {
            buf_end -= sizeof_footer();
        }

        let mut frame_map = HashMap::new();
        while pos < buf_end - frame::sizeof_frame_header(header.major_version) as usize {
            if buf[pos] == 0 {
                if header.footer {
                    return Err(Error::new(ErrorKind::InvalidData, "Padding and footers are not allowed by the spec"));
                }

                break;
            }

            let mut new_frame = match frame::Frame::from_buffer(&mut buf[pos..], &header)? {
                Some(frame) => frame,
                None => break
            };
            if new_frame.size == 0 {
                break;
                // return Err(Error::new(ErrorKind::InvalidData, "Found size 0 frame"));
            }

            // Convert integer frames to an expected format
            match new_frame.frame_id.as_str() {
                "TDRC" => {
                    new_frame.sub = frame::SubClass::Uint(0);
                }
                "TRCK" => {
                    new_frame.sub = frame::SubClass::Uint(0);
                }

                _ => ()
            }

            let size = new_frame.size + frame::sizeof_frame_header(header.major_version) as usize;
            pos += size;
            frame_map.insert(new_frame.frame_id.to_string(), new_frame.sub);
        }

        Ok(Tag{
            frame_map: frame_map
        })
    }

    pub fn default() -> Self {
        Self{
            frame_map: HashMap::new()
        }
    }
}



pub(crate) struct TagHeader {
    pub major_version: u8,
    pub rev_num: u8,
    pub size: u64,
    pub unsynch: bool,
    pub extended: bool,
    pub experimental: bool,
    pub footer: bool
}

fn parse_tag_header(buf: &Vec<u8>) -> Result<TagHeader, Error> {
    if buf.len() < 10 {
        return Err(Error::new(ErrorKind::InvalidData, "Header too small"));
    }

    for byte in &buf[6..10] {
        if *byte >= 128 {
            return Err(Error::new(ErrorKind::InvalidData, "Size byte greater than allowed 128"));
        }
    }

    Ok(TagHeader{
        major_version: buf[3],
        rev_num: buf[4],
        size: synch::int_from_buf(&buf[6..10]) as u64,
        unsynch: buf[5] & 0b10000000 != 0,
        extended: buf[5] & 0b1000000 != 0,
        experimental: buf[5] & 0b100000 != 0,
        footer: buf[5] & 0b00000 != 0
    })
}



// TODO: Move this to a separate (utility?) file
pub(crate) mod synch {
    use byteorder::{BigEndian, ByteOrder};
    use std::cmp::min;

    // taglib:
    pub fn int_from_buf(buf: &[u8]) -> u32 {
        let mut sum = 0 as u32;
        let mut not_sync_safe = false;
        let len = min(buf.len() - 1, 3);

        for i in 0..(len+1) {
            let byte = buf[i];
            if byte & 0x80 != 0 {
                not_sync_safe = true;
                break;
            }

            let addition = (buf[i] & 0x7f) as u32;
            sum = sum | (addition << ((len - i) * 7));
        }

        // Assume that the tag was written by software which doesn't maintain "synch" safety
        if not_sync_safe {
            if buf.len() >= 4 {
                sum = BigEndian::read_u32(buf);
            } else {
                let mut buf = buf.iter().cloned().collect::<Vec<u8>>();
                buf.resize(4, 0);
                sum = BigEndian::read_u32(&buf);
            }
        }

        sum
    }

    pub fn decode_slice(buf: &[u8]) -> Vec<u8>{
        let mut new = vec![0; buf.len()];

        let mut dst = 0;
        let mut last = [0, 0];
        for byte in buf {
            if last[0] != 0xff || last[1] != 0 {
                new[dst] = *byte;
                dst += 1;
            }

            last = [last[1], *byte];
        }

        new.resize(dst, 0);
        new
    }

    // taglib: SynchData::decode
    pub fn decode(buf: &Vec<u8>) -> Vec<u8> {
        decode_slice(&buf)
    }
}

fn sizeof_footer() -> usize {
    10
}


// This is a duplicate of a function declared in m4a.rs and frame.rs
mod id3 {
    pub fn from_ascii(buf: &[u8]) -> String {
        let idx =
            if let Some(idx) = buf.iter().rposition(|x| (*x as char).is_alphanumeric()) {
                idx + 1
            } else {
                buf.len()
            };

        let mut s = "".to_string();
        for c in &buf[0..idx] {
            s.push(*c as char);
        }

        s
    }
}
