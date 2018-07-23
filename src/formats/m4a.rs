#![allow(non_snake_case)]

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Error, ErrorKind, Read, Seek, SeekFrom};
use std::iter::FromIterator;
use std::path;
use std::rc;
use std::{str, u32};

use byteorder::{BigEndian, ByteOrder};

use super::meta;

struct Properties;

// TODO: How do I restrict the visibility to just this file ???
#[derive(Debug)]
pub(crate) enum Atom {
    Atom(u64, u64, String, Vec<Atom>),
}

pub struct File {
    tag: rc::Rc<Tag>,
    _atoms: Vec<Atom>,
    _props: Vec<Properties>
}

impl File {
    pub fn open<P: AsRef<path::Path>>(path: P) -> Result<Self, Error> {
        let mut file = fs::File::open(path)?;

        let mut atoms = Vec::new();
        while let Ok(atom) = read_atom(&mut file) {
            atoms.push(atom);
        }

        // Must have a 'moov' atom
        let moov = atoms.iter().position(|Atom::Atom(_, _, name, _)| name == "moov");
        if let Some(moov_index) = moov {
            if let Some(tag) = read_tag(moov_index, &atoms, &mut file) {

                // TODO: Read properties
                // if let Some(props) = read_properties(moov_index, &atoms, &mut file) {}

                return Ok(Self{
                    tag: rc::Rc::new(tag),
                    _atoms: atoms,
                    _props: Vec::new()
                });
            }
        }

        Err(Error::new(ErrorKind::InvalidData, "Required atom (moov > udta > meta > ilst) not found"))
    }
}

impl meta::File for File {
    fn tag(&self) -> rc::Rc<meta::Tag> {
        self.tag.clone()
    }
}

// TODO: I feel this can be vastly simplified (and still satisfy the borrow checker)
// Tag is at "moov" > "udta" > "meta" > "ilst"
fn read_tag(moov_index: usize, atoms: &Vec<Atom>, file: &mut fs::File) -> Option<Tag> {
    let Atom::Atom(_, _, _, udta_atoms) = atoms.get(moov_index).unwrap();
    let udta = udta_atoms.iter().position(|Atom::Atom(_, _, name, _)| name == "udta");

    if let Some(udta_index) = udta {
        let Atom::Atom(_, _, _, meta_atoms) = udta_atoms.get(udta_index).unwrap();
        let meta = meta_atoms.iter().position(|Atom::Atom(_, _, name, _)| name == "meta");

        if let Some(meta_index) = meta {
            let Atom::Atom(_, _, _, ilst_atoms) = meta_atoms.get(meta_index).unwrap();

            // For some reason, using the same "formula" doesn't find the ilst tag
            for Atom::Atom(_, _, name, atoms) in ilst_atoms {
                if name == "ilst" {
                    return Tag::from_atom(atoms, file).ok();
                }
            }
        }
    }

    None
}

// Tag names are presented in ascii, but rust has no defined way of extracting them
fn from_ascii(buf: &[u8]) -> String {
    let mut s = "".to_owned();

    for c in buf {
        let c = *c as char;
        s.push(c);
    }

    s
}

// TODO: I need someone else to comment this stuff because I don't know the formats
fn read_atom(file: &mut fs::File) -> Result<Atom, Error> {
    let mut buf: Vec<u8> = vec![0, 0, 0, 0, 0, 0, 0, 0];
    let offset = file.seek(SeekFrom::Current(0))?;

    file.read_exact(&mut buf)?;
    let mut length = BigEndian::read_u32(&buf[0..4]);

    if length == 1 {
        file.read_exact(&mut buf)?;
        length = BigEndian::read_u32(&buf[0..4]);
    }

    if length < 8 {
        return Err(Error::new(ErrorKind::InvalidData, "Mp4: Invalid Atom Size"));
    }

    let name = from_ascii(&buf[4..]);
    if name == "meta" {
        file.seek(SeekFrom::Current(4))?;
    } else if name == "stsd" {
        file.seek(SeekFrom::Current(8))?;
    }

    let mut children = Vec::new();
    let containers: HashSet<&str> =
        [ "moov", "udta", "mdia", "meta", "ilst", "stbl", "minf", "moof", "traf", "trak", "stsd" ].iter().cloned().collect();
    if containers.contains(name.as_str()) {
        while let Ok(atom) = read_atom(file) {
            children.push(atom);

            if let Ok(false) = file.seek(SeekFrom::Current(0)).and_then(|pos| Ok(pos < offset + length as u64)) {
                break;
            }
        }
    }

    file.seek(SeekFrom::Start(offset + length as u64))?;
    return Ok(Atom::Atom(offset, length as u64, name, children));
}


pub struct Tag {
    items: HashMap<String, meta::TagData>
}

impl Tag {
    pub(crate) fn from_atom(atoms: &Vec<Atom>, file: &mut fs::File) -> Result<Self, Error> {
        let mut tag = Tag{ items: HashMap::new() };

        for Atom::Atom(off, len, name, children) in atoms {
            file.seek(SeekFrom::Start(off + 8))?;

            match name.as_str() {
                "----" =>
                    match parseFreeForm(len, children, file)? {
                        (_, meta::TagData::Empty) => (),
                        (name, item) => { tag.items.insert(name, item); }
                    },
                "trkn" => { tag.items.insert("trkn".to_owned(), parseIntPair(len, children, file)?); },
                "disk" => { tag.items.insert("disk".to_owned(), parseIntPair(len, children, file)?); },
                "cpil" => { tag.items.insert("cpil".to_owned(), parseBool(len, children, file)?); },
                "pgap" => { tag.items.insert("pgap".to_owned(), parseBool(len, children, file)?); },
                "pcst" => { tag.items.insert("pcst".to_owned(), parseBool(len, children, file)?); },
                "hdvd" => { tag.items.insert("hdvd".to_owned(), parseBool(len, children, file)?); },
                "tmpo" => { tag.items.insert("tmpo".to_owned(), parseInt(len, children, file)?); },
                "tvsn" => { tag.items.insert("tvsn".to_owned(), parseInt(len, children, file)?); },
                "tves" => { tag.items.insert("tves".to_owned(), parseInt(len, children, file)?); },
                "cnID" => { tag.items.insert("cnID".to_owned(), parseInt(len, children, file)?); },
                "sfID" => { tag.items.insert("sfID".to_owned(), parseInt(len, children, file)?); },
                "atID" => { tag.items.insert("atID".to_owned(), parseInt(len, children, file)?); },
                "geID" => { tag.items.insert("geID".to_owned(), parseInt(len, children, file)?); },
                "plID" => { tag.items.insert("plID".to_owned(), parseInt(len, children, file)?); },
                "stik" => { tag.items.insert("stik".to_owned(), parseByte(len, children, file)?); },
                "rtng" => { tag.items.insert("rtng".to_owned(), parseByte(len, children, file)?); },
                "akID" => { tag.items.insert("akID".to_owned(), parseByte(len, children, file)?); },
                "gnre" => { tag.items.insert("gnre".to_owned(), parseGenre(len, children, file)?); },
                "covr" => { tag.items.insert("covr".to_owned(), parseCover(len, children, file)?); },
                name => { tag.items.insert(name.to_owned(), parseString(len, children, file)?); },
            }
        }

        Ok(tag)
    }
}

fn parseInt(len: &u64, children: &Vec<Atom>, file: &mut fs::File) -> Result<meta::TagData, Error> {
    let buf = parseData(len, children, file, u32::MAX, false)?;

    let ret =
        if buf.is_empty() {
            meta::TagData::Empty
        } else {
            meta::TagData::Uint(BigEndian::read_u16(&buf[0].1[0..2]) as u64)
        };
    Ok(ret)
}

fn parseByte(len: &u64, children: &Vec<Atom>, file: &mut fs::File) -> Result<meta::TagData, Error> {
    let buf = parseData(len, children, file, u32::MAX, false)?;

    let ret =
        if buf.is_empty() {
            meta::TagData::Empty
        } else {
            meta::TagData::Uint(buf[0].1[0] as u64)
        };

    Ok(ret)
}

fn parseIntPair(len: &u64, children: &Vec<Atom>, file: &mut fs::File) -> Result<meta::TagData, Error> {
    let buf = parseData(len, children, file, u32::MAX, false)?;

    let ret =
        if buf.is_empty() {
            meta::TagData::Empty
        } else {
            let fst = BigEndian::read_u16(&buf[0].1[2..4]);
            let snd = BigEndian::read_u16(&buf[0].1[4..6]);

            meta::TagData::IntPair(fst as u32, snd as u32)
        };

    Ok(ret)
}

fn parseString(len: &u64, children: &Vec<Atom>, file: &mut fs::File) -> Result<meta::TagData, Error> {
    let buf = parseData(len, children, file, 1, false)?;

    let ret =
        if buf.is_empty() {
            meta::TagData::Empty
        } else {
            let strs = Vec::from_iter(buf
                .iter()
                .map(|buf| str::from_utf8(&buf.1)
                    .and_then(|s| Ok(s.to_owned())))
                .filter(|r| r.is_ok())
                .map(|r| r.unwrap()))
                .join(", ");

            meta::TagData::Str(strs)
        };

    Ok(ret)
}

fn parseCover(_len: &u64, _children: &Vec<Atom>, _file: &mut fs::File) -> Result<meta::TagData, Error> {
    Ok(meta::TagData::Unimplemented)
}

fn parseGenre(len: &u64, children: &Vec<Atom>, file: &mut fs::File) -> Result<meta::TagData, Error> {
    let buf = parseData(len, children, file, u32::MAX, false)?;

    let ret =
        if buf.is_empty() {
            meta::TagData::Empty
        } else {
            let index = BigEndian::read_u16(&buf[0].1[0..2]) as usize;
            meta::TagData::Str(meta::GENRE_LIST.get(index - 1).unwrap().to_string())
        };

    Ok(ret)
}

fn parseFreeForm(len: &u64, children: &Vec<Atom>, file: &mut fs::File) -> Result<(String, meta::TagData), Error> {
    let buf = parseData(len, children, file, u32::MAX, true)?;

    if buf.len() > 2 {
        let data_type = buf[2].0;
        let name = format!("----:{}:{}", str::from_utf8(&buf[0].1).unwrap(), str::from_utf8(&buf[1].1).unwrap());

        let mut strs = Vec::new();
        for (_, str_buf) in &buf[2..] {
            if data_type == 0 {
                if let Ok(r) = str::from_utf8(str_buf).and_then(|s| Ok(s.to_owned())) {
                    strs.push(r);
                }

            } else {
                strs.push(from_ascii(str_buf).to_owned());
            }
        }

        return Ok((name, meta::TagData::Str(strs.join(", "))));
    }

    Ok(("".to_owned(), meta::TagData::Empty))
}

fn parseBool(len: &u64, children: &Vec<Atom>, file: &mut fs::File) -> Result<meta::TagData, Error> {
    let buf = parseData(len, children, file, u32::MAX, false)?;

    let ret =
        if buf.is_empty() {
            meta::TagData::Empty
        } else {
            meta::TagData::Bool(!buf[0].1.is_empty() && buf[0].1[0] != 0)
        };

    Ok(ret)
}


fn parseData(len: &u64, _children: &Vec<Atom>, file: &mut fs::File, expected_flags: u32, free_form: bool) -> Result<Vec<(u32, Vec<u8>)>, Error> {
    let mut buf = vec![0; (*len - 8) as usize];
    file.read_exact(&mut buf)?;

    let mut offset = 0;
    let mut iter = 0 as u32;
    let mut ret_buf = Vec::new();

    while offset < buf.len() {
        let length = BigEndian::read_u32(&buf[offset..(offset+4)]) as usize;
        if length < 12 {
            return Err(Error::new(ErrorKind::InvalidData, "Mp4 atom is too short"));
        }

        let name = from_ascii(&buf[(offset+4)..(offset+8)]);
        let flags = BigEndian::read_u32(&buf[(offset+8)..(offset+12)]);

        if free_form && iter < 2 {
            if iter == 0 && name != "mean" {
                return Err(Error::new(ErrorKind::InvalidData, "Unexpected atom: Expected `mean`"));

            } else if iter == 1 && name != "name" {
                return Err(Error::new(ErrorKind::InvalidData, "Unexpected atom: Expected `name`"));
            }

        } else if name != "data" {
            return Err(Error::new(ErrorKind::InvalidData, "Unexpected atom: Expected `data`"));

        }

        if expected_flags == u32::MAX || flags == expected_flags {
            ret_buf.push((flags, Vec::from_iter(buf[(offset + 16)..(offset+length)].iter().cloned())));
        }

        offset += length;
        iter += 1;
    }

    Ok(ret_buf)
}

impl meta::Tag for Tag {
    fn title(&self) -> Option<String> {
        // Bug with RLS (All these methods have "two" definitions)
        assert!(('©' as u8) == 169);

        if let Some(meta::TagData::Str(title)) = self.items.get("©nam") {
            return Some(title.to_owned());
        }

        None
    }
    fn artist(&self) -> Option<String> {
        if let Some(meta::TagData::Str(artist)) = self.items.get("©ART") {
            return Some(artist.to_owned());
        }

        None
    }
    fn album(&self) -> Option<String> {
        if let Some(meta::TagData::Str(album)) = self.items.get("©alb") {
            return Some(album.to_owned());
        }

        None
    }
    fn year(&self) -> Option<u64> {
        if let Some(meta::TagData::Uint(year)) = self.items.get("©nam") {
            return Some(*year);
        }

        None
    }
    fn comment(&self) -> Option<String> {
        if let Some(meta::TagData::Str(title)) = self.items.get("©nam") {
            return Some(title.to_owned());
        }

        None
    }
    fn track(&self) -> Option<u32> {
        if let Some(meta::TagData::Uint(track)) = self.items.get("trkn") {
            return Some(*track as u32);
        }

        None
    }
    fn genre(&self) -> Option<String> {
        if let Some(meta::TagData::Str(genre)) = self.items.get("gnre") {
            return Some(genre.to_owned());
        }

        None
    }
}

pub struct _Audio {

}

impl meta::Audio for _Audio {
    fn bitrate(&self) -> u32 {
        0
    }
    fn samplerate(&self) -> u32 {
        0
    }
    fn channels(&self) -> u32 {
        0
    }
    fn length(&self) -> u32 {
        0
    }
}
