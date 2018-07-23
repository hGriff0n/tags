#![allow(dead_code)]
#![allow(unused_imports)]

use formats::meta;
use super::frame;

use std::collections::HashMap;
use std::fs;
use std::io::{Error, ErrorKind, Read, Seek, SeekFrom};
use std::mem;

use byteorder::{BigEndian, ByteOrder};


pub struct Tag {
    frame_map: HashMap<String, frame::SubClass>,
}

impl meta::Tag for Tag {
    fn title(&self) -> Option<String> {
        if let Some(frame::SubClass::Text(title)) = self.frame_map.get("TIT2") {
            Some(title.to_string())
        } else {
            None
        }
    }
    fn artist(&self) -> Option<String> {
        if let Some(frame::SubClass::Text(artist)) = self.frame_map.get("TPE1") {
            Some(artist.to_string())
        } else {
            None
        }
    }
    fn album(&self) -> Option<String> {
        if let Some(frame::SubClass::Text(album)) = self.frame_map.get("TALB") {
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
        if let Some(frame::SubClass::Text(comment)) = self.frame_map.get("COMM") {
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
        if let Some(frame::SubClass::Text(genre)) = self.frame_map.get("TCON") {
            Some(genre.to_string())
        } else {
            None
        }
    }
}

impl Tag {
    pub fn id3v2_from_file(file: &mut fs::File, offset: u64) -> Result<Self, Error> {
        file.seek(SeekFrom::Start(offset))?;

        let mut header = vec![0; 10];
        file.read_exact(&mut header)?;
        let header = parse_tag_header(&header)?;

        if header.size != 0 {
            let mut buf = vec![0; header.size as usize];
            file.read_exact(&mut buf)?;
            Tag::from_buffer(&mut buf, &header)

        } else {
            Err(Error::new(ErrorKind::InvalidData, "Tags must contain at least 1 frame"))
        }

        // NOTE: Taglib has some stuff about ignoring duplicate flags (I'm ignoring that for now)
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

            let mut new_frame = match frame::Frame::from_buffer(&mut buf[pos..], pos, buf_end, &header)? {
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
        size: synch::int_from_buf(&buf[6..10]).into(),
        unsynch: buf[5] & 0b10000000 != 0,
        extended: buf[5] & 0b1000000 != 0,
        experimental: buf[5] & 0b100000 != 0,
        footer: buf[5] & 0b00000 != 0
    })
}



// TODO: Move this to a separate (utility?) file
pub(crate) mod synch {
    use byteorder::{BigEndian, ByteOrder};

    // taglib:
    pub fn int_from_buf(buf: &[u8]) -> u32 {
        let mut sum = 0 as u32;
        let mut is_sync_safe = true;
        let mut len = buf.len() - 1;

        for byte in buf {
            if byte & 0x80 != 0x80 {
                if let Some(val) = ((byte & 0x7f) as u32).checked_shl((len * 7) as u32) {
                    sum = sum | val;
                    if len == 0 {
                        break;
                    } else {
                        len -= 1;
                        continue;
                    }
                }
            }

            is_sync_safe = false;
            break;
        }

        // Assume that the tag was written by software which doesn't maintain "synch" safety
        if !is_sync_safe {
            sum = BigEndian::read_u32(buf);
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
