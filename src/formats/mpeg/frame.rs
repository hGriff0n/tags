
use super::tag;
use formats::utils;

use std::cmp::min;
use std::convert;
use std::io::{Error, ErrorKind};
use std::str;

use byteorder::{BigEndian, ByteOrder};

pub(crate) struct Frame {
    pub size: usize,
    pub frame_id: String,
    pub sub: SubClass
}

impl Frame {
    fn get_header(buf: &mut [u8], version: u8) -> Result<Header, Error> {
        let mut header = Header::default(version);

        if version < 3 {
            if buf.len() < 3 {
                return Err(Error::new(ErrorKind::Other, "Frame ID not specified"));
            }

            header.frame_id = utils::from_ascii(&buf[0..3]);
            header.size = if buf.len() < 6 {
                0
            } else {
                let tmp = [buf[3], buf[4], buf[5], 0];
                BigEndian::read_u32(&tmp) as u64
            }


        } else if version == 3 {
            if buf.len() < 4 {
                return Err(Error::new(ErrorKind::Other, "Frame ID not specified"));
            }

            header.frame_id = utils::from_ascii(&buf[0..4]);
            if buf.len() >= 10 {
                header.size = BigEndian::read_u32(&buf[4..8]) as u64;

                header.tag_alter_preservation = buf[8] & 0b10000000 != 0;
                header.file_alter_preservation = buf[8] & 0b1000000 != 0;
                header.read_only = buf[8] & 0b100000 != 0;

                header.compression = buf[9] & 0b10000000 != 0;
                header.encryption = buf[9] & 0b1000000 != 0;
                header.grouping_ident = buf[9] & 0b100000 != 0;
            }

        } else {
            if buf.len() < 4 {
                return Err(Error::new(ErrorKind::Other, "Frame ID not specified"));
            }

            header.frame_id = utils::from_ascii(&buf[0..4]);

            if buf.len() >= 10 {
                header.size = tag::synch::int_from_buf(&buf[4..8]) as u64;

                // itunes hacks
                // iTunes writes v2.4 tags with v2.3-like frame sizes
                if header.size > 127 {
                    // TODO: This currently causes issues with "index out-of-range" errors
                    // let frame_start = (header.size + 10) as usize;
                    // if !valid_frame_id(&buf[frame_start..(frame_start + 4)]) {
                    //     let size = BigEndian::read_u32(&buf[4..8]) as usize;
                    //     if valid_frame_id(&buf[(size+10)..(size+14)]) {
                    //         header.size = size as u64;
                    //     }
                    // }
                }

                header.tag_alter_preservation = buf[8] & 0b1000000 != 0;
                header.file_alter_preservation = buf[8] & 0b100000 != 0;
                header.read_only = buf[8] & 0b10000 != 0;

                header.grouping_ident = buf[9] & 0b1000000 != 0;
                header.compression = buf[9] & 0b1000 != 0;
                header.encryption = buf[9] & 0b100 != 0;
                header.unsynch = buf[9] & 0b10 != 0;
                header.data_length_indicator = buf[9] & 0b1 != 0;
            }
        }

        Ok(header)
    }

    pub(crate) fn from_buffer(buf: &mut [u8], header: &tag::TagHeader) -> Result<Option<Frame>, Error> {
        let version = header.major_version;
        let mut frame_header = Frame::get_header(buf, version)?;

        if frame_header.frame_id.len() != (if version < 3 { 3 } else { 4 })
            || frame_header.size <= (if frame_header.data_length_indicator { 4 } else { 0 })
            || frame_header.size as usize > buf.len()
        {
            // return Err(Error::new(ErrorKind::InvalidData, "Invalid frame length"));
            return Ok(None);
        }

        // iTunes hacks
        let last_char = frame_header.frame_id.chars().last().unwrap_or('a');
        if version == 3 && frame_header.frame_id.len() == 4 && last_char == '\0' {
            frame_header.frame_id.pop();
            frame_header.update(2);
        }

        for ch in frame_header.frame_id.chars() {
            if (ch < 'A' || ch > 'Z') && (ch < '0' || ch > '9') {
                return Err(Error::new(ErrorKind::InvalidData, "Frame ID was not 4 uppercase Latin1 Letters"));
            }
        }

        if version > 3 && (header.unsynch || frame_header.unsynch) {
            let size = sizeof_frame_header(header.major_version) as usize;
            let tmp = tag::synch::decode_slice(&buf[size..(size + header.size as usize)]);
            for i in 0..tmp.len() {
                let i = i as usize;
                buf[size + i] = tmp[i];
            }
        }

        if frame_header.compression {
            return Err(Error::new(ErrorKind::Other, "Compressed frames not currently supported"));
        }

        if frame_header.encryption {
            return Err(Error::new(ErrorKind::Other, "Encrypted frames not currently supported"));
        }

        let mut frame = Frame{
            size: frame_header.size as usize,
            frame_id: frame_header.frame_id.clone(),
            sub: SubClass::Unknown
        };
        if !frame_header.update(version) {
            return Ok(Some(frame))
        }

        // Extract the frame subclass information
        let first_char = frame_header.frame_id.chars().next().unwrap_or('\0');
        frame.sub = match frame_header.frame_id.as_str() {
            // Text frames
            tag if first_char == 'T' || tag == "WFED" || tag == "MVNM" || tag == "MVIN" => {
                let data = Frame::field_data(buf, &frame_header)?;

                if data.len() < 2 {
                    SubClass::Text("".to_string(), StringType::UTF16)

                } else {
                    let encoding = StringType::from(data[0]);
                    let alignment = match encoding {
                        StringType::Latin1 | StringType::UTF8 => 1,
                        _ => 2
                    };

                    let mut len = data.len() - 1;

                    while len > 0 && data[len] == 0 {
                        len -= 1;
                    }

                    while len % alignment != 0 {
                        len += 1;
                    }

                    // TODO: I don't split the data based on a text delimeter
                    // taglib:textidentificationframe.cpp:211

                    let end = min(len + 1, data.len());
                    let text = match encoding {
                        StringType::Latin1 => utils::from_ascii(&data[1..end]),


                        // TODO: Fix errors in extract of utf16 strings (every other character is chinese, I think only half the string is there)
                            // NOTE: It also seems like the "tag size" field indicates the number of characters, not the number of bytes (though I don't have anything to prove this)
                            // NOTE: We can somewhat deal with this by using the ID3v1 tags, but it's not a perfect solution
                        StringType::UTF16 | StringType::UTF16be | StringType::UTF16le => {
                            let mut utf16_buf = Vec::new();
                            let buf = &data[1..end];

                            let swap = buf[0] == 0xff && buf[1] == 0xfe;
                            for i in 1..(buf.len() / 2) {
                                let val = if swap {
                                        let fst_byte = (buf[i+1] as u16) & 0xff;
                                        let snd_byte = (buf[i] as u16) & 0xff;
                                        (fst_byte << 8) | snd_byte
                                    } else {
                                        let fst_byte = (buf[i] as u16) & 0xff;
                                        let snd_byte = (buf[i+1] as u16) & 0xff;
                                        (fst_byte << 8) | snd_byte
                                    };

                                utf16_buf.push(val);
                            }

                            match String::from_utf16(&utf16_buf) {
                                Ok(s) => s,
                                Err(_) => return Err(Error::new(ErrorKind::InvalidData, "Failed to convert string from utf16"))
                            }
                        },
                        _ => match str::from_utf8(&data[1..end]) {
                            Ok(s) => s.to_string(),
                            Err(_) => return Err(Error::new(ErrorKind::InvalidData, "Failed to convert string from utf8"))
                        }
                    };

                    SubClass::Text(text, encoding)
                }
            },

            // Comments
            "COMM" => SubClass::Unknown,

            // Picture
            "APIC" => SubClass::Unknown,
            "PIC" => SubClass::Unknown,

            // Relative Volume Adjustment
            "RVA2" => SubClass::Unknown,

            // Unique File Identifier
            "UFID" => SubClass::Unknown,

            // General Encapsulated Object
            "GEOB" => SubClass::Unknown,

            // URL
            _url if first_char == 'W' => SubClass::Unknown,

            // Lyrics
            "USLT" => SubClass::Unknown,
            "SYLT" => SubClass::Unknown,

            // Event timing
            "ETCO" => SubClass::Unknown,

            // Popularimeter
            "POPM" => SubClass::Unknown,

            // Private
            "PRIV" => SubClass::Unknown,

            // Ownership
            "OWNE" => SubClass::Unknown,

            // Chapter
            "CHAP" => SubClass::Unknown,

            // Table of Contents
            "CTOC" => SubClass::Unknown,

            // Podcast
            "PCST" => SubClass::Unknown,

            // Unknown
            _ => SubClass::Unknown
        };

        Ok(Some(frame))
    }

    fn field_data(buf: &[u8], header: &Header) -> Result<Vec<u8>, Error> {
        let header_size = sizeof_frame_header(header.version) as usize;

        let mut offset = header_size;
        let mut len = header.size as usize;

        if header.compression || header.data_length_indicator {
            len = tag::synch::int_from_buf(&buf[header_size..(header_size+4)]) as usize;
            offset += 4;
        }

        if header.compression && !header.encryption {
            return Err(Error::new(ErrorKind::Other, "Compressed frames not currently supported"));
        }

        let end = min(buf.len(), offset+len);
        Ok(buf[offset..end].to_vec())
    }
}

#[allow(dead_code)]
fn valid_frame_id(buf: &[u8]) -> bool {
    if buf.len() != 4 {
        return false;
    }

    for byte in buf {
        if (*byte < 'A' as u8 || *byte > 'Z' as u8) && (*byte < '0' as u8 || *byte > '9' as u8) {
            return false;
        }
    }

    true
}

#[derive(Debug, Clone)]
pub enum StringType {
    Latin1 = 0,
    UTF16 = 1,
    UTF16be = 2,
    UTF8 = 3,
    UTF16le = 4,
    Invalid
}

impl From<u8> for StringType {
    fn from(val: u8) -> Self {
        match val {
            0 => StringType::Latin1,
            1 => StringType::UTF16,
            2 => StringType::UTF16be,
            3 => StringType::UTF8,
            4 => StringType::UTF16le,
            _ => StringType::Invalid
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) enum SubClass {
    Text(String, StringType),
    Uint(u64),
    Unknown
}

struct Header {
    pub frame_id: String,
    pub size: u64,
    pub version: u8,
    pub data_length_indicator: bool,
    pub unsynch: bool,
    pub tag_alter_preservation: bool,
    pub file_alter_preservation: bool,
    pub read_only: bool,
    pub compression: bool,
    pub encryption: bool,
    pub grouping_ident: bool
}

impl Header {
    pub fn default(version: u8) -> Self {
        Self{
            frame_id: "".to_string(),
            size: 0,
            version: version,
            data_length_indicator: false,
            unsynch: false,
            tag_alter_preservation: false,
            file_alter_preservation: false,
            read_only: false,
            compression: false,
            encryption: false,
            grouping_ident: false,
        }
    }

    pub fn update(&mut self, version: u8) -> bool {
        match self.frame_id.as_str() {
            "TORY" => {
                self.frame_id = "TDOR".to_string();
            },
            "TYER" => {
                self.frame_id = "TDRC".to_string();
            }
            "IPLS" => {
                self.frame_id = "TIPL".to_string();
            },
            _ => ()
        };

        match version {
            2 => match self.frame_id.as_str() {
                "CRM" => false,
                "EQU" => false,
                "LNK" => false,
                "RVA" => false,
                "TIM" => false,
                "TDA" => false,
                "TSI" => false,
                _ => true,
            },
            3 => match self.frame_id.as_str() {
                "EQUA" => false,
                "RVAD" => false,
                "TIME" => false,
                "TRDA" => false,
                "TSIZ" => false,
                "TDAT" => false,
                _ => true
            },
            _ => true
        }
    }
}

pub fn sizeof_frame_header(version: u8) -> u64 {
    if version < 3 {
        6
    } else {
        10
    }
}
