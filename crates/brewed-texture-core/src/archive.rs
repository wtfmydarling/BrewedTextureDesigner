//! Many files in one: the part of ZIP (PKWARE's APPNOTE 6.3) that packages are written in. Every
//! archive tool and every language reads and writes it, and this reads what they write within
//! it: entries stored or deflated, the ZIP64 extensions included, sizes before an entry or in a
//! descriptor after it.
//!
//! Written: each entry's sizes and CRC-32 before its bytes (no descriptors), names in UTF-8, the
//! time of every entry the same (a package written twice is the same bytes), ZIP64 records where
//! a size, an offset or the count of entries goes past what the classic records hold.
//!
//! Read: the central directory, then each entry where it says, as much as it may hold and no
//! more (see [`Archive::read`]): an archive can be made to unpack to anything, say to be
//! anything, and put its entries anywhere; what it says is checked before it is believed.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::io::{self, Read, Seek, SeekFrom, Write};

use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;
const END64: u32 = 0x0606_4b50;
const LOCATOR64: u32 = 0x0706_4b50;
/// The extra field of ZIP64: the sizes and the offset the classic fields cannot hold.
const ZIP64_EXTRA: u16 = 0x0001;
/// What a classic field says when the value is in the ZIP64 records instead.
const FULL16: u16 = 0xffff;
const FULL32: u32 = 0xffff_ffff;
/// The version of the specification an entry needs: 2.0 (deflate), 4.5 (ZIP64).
const NEEDS: u16 = 20;
const NEEDS64: u16 = 45;
/// Made by: UNIX (its attributes, and names as UTF-8 says, which every reader takes; some take
/// the names of an archive made by MS-DOS as its code page says, whatever the flag), 4.5.
const MADE_BY: u16 = (3 << 8) | NEEDS64;
/// The attributes of every entry: a file anyone may read, its owner write (`rw-r--r--`).
const FILE_MODE: u32 = 0o100_644 << 16;
const STORED: u16 = 0;
const DEFLATED: u16 = 8;
/// General purpose flags: encrypted; the name is UTF-8.
const ENCRYPTED: u16 = 1;
const UTF8: u16 = 1 << 11;
/// 1980-01-01 00:00, the first time an MS-DOS date says: the time of every entry written.
const DOS_DATE: u16 = (1 << 5) | 1;
const DOS_TIME: u16 = 0;

/// The most entries an archive read may have, and the most its central directory may take.
pub const MAX_ENTRIES: u64 = 100_000;
const MAX_DIRECTORY: u64 = 64 << 20;

/// How an entry's bytes are kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// As they are (what is compressed already: a PNG, a JPEG, an EXR).
    Stored,
    /// Deflated, unless that makes them no smaller.
    Deflated,
}

/// An entry's bytes made ready to be written: their CRC-32 and, deflated, what they deflate to.
/// Entries are made ready anywhere, all at once on every core say, and written one after the
/// other (see [`Writer::add_prepared`]).
pub struct Prepared<'a> {
    size: u64,
    crc: u32,
    method: u16,
    /// What is written: the bytes as they are, or deflated.
    data: Cow<'a, [u8]>,
}

impl<'a> Prepared<'a> {
    /// `bytes`, to be kept as `method` says.
    pub fn new(bytes: Cow<'a, [u8]>, method: Method) -> io::Result<Prepared<'a>> {
        let (size, crc) = (bytes.len() as u64, crc32fast::hash(&bytes));
        if method == Method::Deflated {
            let mut encoder = DeflateEncoder::new(Vec::with_capacity(bytes.len() / 2), Compression::default());
            encoder.write_all(&bytes)?;
            let deflated = encoder.finish()?;
            if deflated.len() < bytes.len() {
                return Ok(Prepared { size, crc, method: DEFLATED, data: Cow::Owned(deflated) });
            }
        }
        Ok(Prepared { size, crc, method: STORED, data: bytes })
    }
}

/// An entry written, as the central directory says it.
struct Written {
    name: String,
    method: u16,
    crc: u32,
    compressed: u64,
    size: u64,
    offset: u64,
}

/// A writer that counts what it wrote: where the next entry begins.
struct Counted<W> {
    inner: W,
    at: u64,
}

impl<W: Write> Write for Counted<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(bytes)?;
        self.at += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// An archive being written into `W`, entry after entry; [`Writer::finish`] ends it.
pub struct Writer<W: Write> {
    out: Counted<W>,
    entries: Vec<Written>,
    names: HashSet<String>,
    /// ZIP64 records for every entry and the end, needed or not (to test readers of them).
    zip64: bool,
}

/// What a name of an entry may be: parts between `/`, none empty, `.` or `..`, and nothing a
/// system would read otherwise (a `\`, a drive's `:`, a NUL).
pub fn is_entry_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= usize::from(FULL16)
        && !name.contains(['\\', '\0', ':'])
        && name.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

impl<W: Write> Writer<W> {
    pub fn new(out: W) -> Writer<W> {
        Writer { out: Counted { inner: out, at: 0 }, entries: Vec::new(), names: HashSet::new(), zip64: false }
    }

    /// A writer that gives every entry and the end ZIP64 records, as a writer must when sizes
    /// or offsets go past 4 GB: what reads archives is tested on them without writing that
    /// much.
    #[cfg(test)]
    pub fn always_zip64(mut self) -> Writer<W> {
        self.zip64 = true;
        self
    }

    /// Add the entry `name` holding `bytes`, kept as `method` says.
    pub fn add(&mut self, name: &str, bytes: &[u8], method: Method) -> io::Result<()> {
        self.add_prepared(name, Prepared::new(Cow::Borrowed(bytes), method)?)
    }

    /// Add the entry `name` holding what `prepared` is made of.
    pub fn add_prepared(&mut self, name: &str, prepared: Prepared<'_>) -> io::Result<()> {
        if !is_entry_name(name) || !self.names.insert(name.to_string()) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, format!("'{name}' cannot name an entry of the archive")));
        }
        let Prepared { size, crc, method, data } = prepared;
        let (compressed, offset) = (data.len() as u64, self.out.at);
        let zip64 = self.zip64 || size >= u64::from(FULL32) || compressed >= u64::from(FULL32);
        let mut header = Vec::with_capacity(30 + name.len() + 20);
        header.extend(LOCAL.to_le_bytes());
        header.extend((if zip64 { NEEDS64 } else { NEEDS }).to_le_bytes());
        header.extend(flags(name).to_le_bytes());
        header.extend(method.to_le_bytes());
        header.extend(DOS_TIME.to_le_bytes());
        header.extend(DOS_DATE.to_le_bytes());
        header.extend(crc.to_le_bytes());
        header.extend((if zip64 { FULL32 } else { compressed as u32 }).to_le_bytes());
        header.extend((if zip64 { FULL32 } else { size as u32 }).to_le_bytes());
        header.extend((name.len() as u16).to_le_bytes());
        header.extend((if zip64 { 20u16 } else { 0 }).to_le_bytes());
        header.extend(name.as_bytes());
        if zip64 {
            header.extend(ZIP64_EXTRA.to_le_bytes());
            header.extend(16u16.to_le_bytes());
            header.extend(size.to_le_bytes());
            header.extend(compressed.to_le_bytes());
        }
        self.out.write_all(&header)?;
        self.out.write_all(&data)?;
        self.entries.push(Written { name: name.to_string(), method, crc, compressed, size, offset });
        Ok(())
    }

    /// Write the central directory and the end of the archive: its entries as they are now.
    pub fn finish(mut self) -> io::Result<W> {
        let directory = self.out.at;
        for e in &self.entries {
            // The fields that do not fit, in the order the ZIP64 extra field holds them.
            let over: Vec<u64> = [e.size, e.compressed, e.offset].into_iter().filter(|v| self.zip64 || *v >= u64::from(FULL32)).collect();
            let long = |v: u64| if self.zip64 || v >= u64::from(FULL32) { FULL32 } else { v as u32 };
            let mut record = Vec::with_capacity(46 + e.name.len() + 28);
            record.extend(CENTRAL.to_le_bytes());
            record.extend(MADE_BY.to_le_bytes());
            record.extend((if over.is_empty() { NEEDS } else { NEEDS64 }).to_le_bytes());
            record.extend(flags(&e.name).to_le_bytes());
            record.extend(e.method.to_le_bytes());
            record.extend(DOS_TIME.to_le_bytes());
            record.extend(DOS_DATE.to_le_bytes());
            record.extend(e.crc.to_le_bytes());
            record.extend(long(e.compressed).to_le_bytes());
            record.extend(long(e.size).to_le_bytes());
            record.extend((e.name.len() as u16).to_le_bytes());
            record.extend((if over.is_empty() { 0 } else { 4 + 8 * over.len() as u16 }).to_le_bytes());
            record.extend(0u16.to_le_bytes()); // comment
            record.extend(0u16.to_le_bytes()); // disk
            record.extend(0u16.to_le_bytes()); // internal attributes
            record.extend(FILE_MODE.to_le_bytes()); // external attributes
            record.extend(long(e.offset).to_le_bytes());
            record.extend(e.name.as_bytes());
            if !over.is_empty() {
                record.extend(ZIP64_EXTRA.to_le_bytes());
                record.extend((8 * over.len() as u16).to_le_bytes());
                for v in over {
                    record.extend(v.to_le_bytes());
                }
            }
            self.out.write_all(&record)?;
        }
        let (count, size) = (self.entries.len() as u64, self.out.at - directory);
        let zip64 = self.zip64 || count >= u64::from(FULL16) || size >= u64::from(FULL32) || directory >= u64::from(FULL32);
        if zip64 {
            let end64 = self.out.at;
            let mut record = Vec::with_capacity(56 + 20);
            record.extend(END64.to_le_bytes());
            record.extend(44u64.to_le_bytes()); // the size of what follows
            record.extend(NEEDS64.to_le_bytes());
            record.extend(NEEDS64.to_le_bytes());
            record.extend(0u32.to_le_bytes()); // this disk
            record.extend(0u32.to_le_bytes()); // the disk of the directory
            record.extend(count.to_le_bytes());
            record.extend(count.to_le_bytes());
            record.extend(size.to_le_bytes());
            record.extend(directory.to_le_bytes());
            record.extend(LOCATOR64.to_le_bytes());
            record.extend(0u32.to_le_bytes());
            record.extend(end64.to_le_bytes());
            record.extend(1u32.to_le_bytes()); // disks
            self.out.write_all(&record)?;
        }
        let short = |v: u64, full: u32| if zip64 || v >= u64::from(full) { full } else { v as u32 };
        let mut end = Vec::with_capacity(22);
        end.extend(END.to_le_bytes());
        end.extend(0u16.to_le_bytes());
        end.extend(0u16.to_le_bytes());
        end.extend((short(count, u32::from(FULL16)) as u16).to_le_bytes());
        end.extend((short(count, u32::from(FULL16)) as u16).to_le_bytes());
        end.extend(short(size, FULL32).to_le_bytes());
        end.extend(short(directory, FULL32).to_le_bytes());
        end.extend(0u16.to_le_bytes()); // comment
        self.out.write_all(&end)?;
        self.out.flush()?;
        Ok(self.out.inner)
    }
}

/// The general purpose flags of an entry named `name`: UTF-8, where it is more than ASCII.
fn flags(name: &str) -> u16 {
    if name.is_ascii() { 0 } else { UTF8 }
}

/// An entry of an archive read, as its central directory says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    /// How many bytes it holds.
    pub size: u64,
    method: u16,
    flags: u16,
    crc: u32,
    compressed: u64,
    offset: u64,
}

/// An archive read from `R`: its entries are read one by one, when asked for.
pub struct Archive<R> {
    inner: R,
    entries: Vec<Entry>,
    by_name: HashMap<String, usize>,
    /// Where the central directory begins: every entry lies before it.
    directory: u64,
}

/// Little-endian fields of a record, one after the other; what a record lacks reads as damage.
struct Fields<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Fields<'a> {
    fn new(bytes: &'a [u8]) -> Fields<'a> {
        Fields { bytes, at: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let part = self.bytes.get(self.at..self.at + n).ok_or_else(|| damaged("a record is cut short"))?;
        self.at += n;
        Ok(part)
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().expect("two bytes")))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().expect("four bytes")))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().expect("eight bytes")))
    }
}

fn damaged(what: &str) -> String {
    format!("the archive is damaged: {what}")
}

impl<R: Read + Seek> Archive<R> {
    /// Read the directory of the archive `inner` holds.
    pub fn new(mut inner: R) -> Result<Archive<R>, String> {
        let io = |e: io::Error| e.to_string();
        let len = inner.seek(SeekFrom::End(0)).map_err(io)?;
        // The end record: the last 22 bytes, or before a comment of up to 64 KB.
        let tail_len = len.min(22 + u64::from(FULL16));
        inner.seek(SeekFrom::Start(len - tail_len)).map_err(io)?;
        let mut tail = vec![0; tail_len as usize];
        inner.read_exact(&mut tail).map_err(io)?;
        let at = (0..tail.len().saturating_sub(21))
            .rev()
            .find(|&i| tail[i..i + 4] == END.to_le_bytes() && usize::from(u16::from_le_bytes([tail[i + 20], tail[i + 21]])) == tail.len() - i - 22)
            .ok_or("this is not an archive (a ZIP file)")?;
        let end_at = len - tail_len + at as u64;
        let mut end = Fields::new(&tail[at + 4..]);
        let (disk, directory_disk) = (end.u16()?, end.u16()?);
        let (_, count16) = (end.u16()?, end.u16()?);
        let (size32, offset32) = (end.u32()?, end.u32()?);
        let (mut count, mut size, mut offset) = (u64::from(count16), u64::from(size32), u64::from(offset32));
        let mut directory_end = end_at;
        // ZIP64, where a classic field is full: a locator right before the end record says where
        // the ZIP64 end record is, which holds them all.
        if count16 == FULL16 || size32 == FULL32 || offset32 == FULL32 {
            let locator_at = end_at.checked_sub(20).ok_or_else(|| damaged("its ZIP64 records are missing"))?;
            inner.seek(SeekFrom::Start(locator_at)).map_err(io)?;
            let mut locator = [0; 20];
            inner.read_exact(&mut locator).map_err(io)?;
            let mut l = Fields::new(&locator);
            if l.u32()? != LOCATOR64 {
                return Err(damaged("its ZIP64 records are missing"));
            }
            let (_, end64_at) = (l.u32()?, l.u64()?);
            if end64_at.checked_add(56).is_none_or(|e| e > locator_at) {
                return Err(damaged("its ZIP64 records are out of place"));
            }
            inner.seek(SeekFrom::Start(end64_at)).map_err(io)?;
            let mut record = [0; 56];
            inner.read_exact(&mut record).map_err(io)?;
            let mut r = Fields::new(&record);
            if r.u32()? != END64 {
                return Err(damaged("its ZIP64 end record is missing"));
            }
            let _ = (r.u64()?, r.u16()?, r.u16()?, r.u32()?, r.u32()?, r.u64()?);
            (count, size, offset) = (r.u64()?, r.u64()?, r.u64()?);
            directory_end = end64_at;
        }
        if (disk != 0 && disk != FULL16) || (directory_disk != 0 && directory_disk != FULL16) {
            return Err("archives split in parts are not read".into());
        }
        if count > MAX_ENTRIES || size > MAX_DIRECTORY {
            return Err(format!("the archive has more than {MAX_ENTRIES} entries"));
        }
        if offset.checked_add(size).is_none_or(|e| e > directory_end) {
            return Err(damaged("its directory is out of place"));
        }
        inner.seek(SeekFrom::Start(offset)).map_err(io)?;
        let mut listing = vec![0; size as usize];
        inner.read_exact(&mut listing).map_err(io)?;
        let mut d = Fields::new(&listing);
        let (mut entries, mut by_name) = (Vec::new(), HashMap::new());
        for _ in 0..count {
            if d.u32()? != CENTRAL {
                return Err(damaged("an entry of its directory is not one"));
            }
            let _made_by = d.u16()?;
            let _needs = d.u16()?;
            let (flags, method) = (d.u16()?, d.u16()?);
            let _time = (d.u16()?, d.u16()?);
            let crc = d.u32()?;
            let (compressed32, size32) = (d.u32()?, d.u32()?);
            let (name_len, extra_len, comment_len) = (d.u16()?, d.u16()?, d.u16()?);
            let disk = d.u16()?;
            let _attributes = (d.u16()?, d.u32()?);
            let offset32 = d.u32()?;
            let name_bytes = d.take(usize::from(name_len))?;
            let extra = d.take(usize::from(extra_len))?;
            let _comment = d.take(usize::from(comment_len))?;
            let (mut size, mut compressed, mut offset) = (u64::from(size32), u64::from(compressed32), u64::from(offset32));
            // The values the classic fields could not hold, in this order, in the ZIP64 field.
            let mut fields = Fields::new(extra);
            while fields.at + 4 <= extra.len() {
                let (id, len) = (fields.u16()?, fields.u16()?);
                let data = fields.take(usize::from(len))?;
                if id == ZIP64_EXTRA {
                    let mut z = Fields::new(data);
                    if size32 == FULL32 {
                        size = z.u64()?;
                    }
                    if compressed32 == FULL32 {
                        compressed = z.u64()?;
                    }
                    if offset32 == FULL32 {
                        offset = z.u64()?;
                    }
                }
            }
            if disk != 0 && disk != FULL16 {
                return Err("archives split in parts are not read".into());
            }
            // UTF-8 as the flag says, and as most writers write without saying; the rest as it
            // comes (what is looked for is ASCII).
            let name = String::from_utf8_lossy(name_bytes).into_owned();
            if by_name.insert(name.clone(), entries.len()).is_some() {
                return Err(damaged(&format!("two of its entries are named '{name}'")));
            }
            entries.push(Entry { name, size, method, flags, crc, compressed, offset });
        }
        Ok(Archive { inner, entries, by_name, directory: offset })
    }

    /// Its entries, in the order its directory lists them.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The entry named `name`, if it has one.
    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.by_name.get(name).map(|&i| &self.entries[i])
    }

    /// The bytes of the entry `name`, which may hold at most `limit` of them: else why they
    /// cannot be read (it is not there, holds more, is damaged or kept as this reads not).
    pub fn read(&mut self, name: &str, limit: u64) -> Result<Vec<u8>, String> {
        let entry = self.entry(name).ok_or_else(|| format!("'{name}' is not in the archive"))?.clone();
        let failed = |what: &str| format!("'{name}': {what}");
        if entry.size > limit {
            return Err(failed(&format!("it holds {} bytes, more than {limit} are not read", entry.size)));
        }
        if entry.flags & ENCRYPTED != 0 {
            return Err(failed("it is encrypted"));
        }
        let io = |e: io::Error| failed(&e.to_string());
        self.inner.seek(SeekFrom::Start(entry.offset)).map_err(io)?;
        let mut header = [0; 30];
        self.inner.read_exact(&mut header).map_err(io)?;
        let mut h = Fields::new(&header);
        if h.u32()? != LOCAL {
            return Err(failed("the archive is damaged: the entry is not where its directory says"));
        }
        let _ = (h.u16()?, h.u16()?, h.u16()?, h.u16()?, h.u16()?, h.u32()?, h.u32()?, h.u32()?);
        let (name_len, extra_len) = (h.u16()?, h.u16()?);
        let start = entry.offset.checked_add(30 + u64::from(name_len) + u64::from(extra_len));
        let Some(start) = start.filter(|s| s.checked_add(entry.compressed).is_some_and(|end| end <= self.directory)) else {
            return Err(failed("the archive is damaged: the entry runs past its end"));
        };
        self.inner.seek(SeekFrom::Start(start)).map_err(io)?;
        let source = (&mut self.inner).take(entry.compressed);
        // Room for what it says it holds, as much as its bytes can unpack to (deflate makes at
        // most 1032 bytes of one): a size it lies about takes no memory it does not fill.
        let mut bytes = Vec::with_capacity(entry.size.min(entry.compressed.saturating_mul(1032)) as usize);
        match entry.method {
            STORED => {
                if entry.compressed != entry.size {
                    return Err(failed("the archive is damaged: its sizes differ"));
                }
                source.take(limit).read_to_end(&mut bytes).map_err(io)?;
            }
            // What it unpacks to is read to one byte past what it may hold: so it says.
            DEFLATED => {
                DeflateDecoder::new(source).take(limit.saturating_add(1)).read_to_end(&mut bytes).map_err(io)?;
            }
            other => return Err(failed(&format!("it is compressed as method {other}, which is not read"))),
        }
        if bytes.len() as u64 > limit {
            return Err(failed(&format!("it unpacks to more than {limit} bytes")));
        }
        if bytes.len() as u64 != entry.size || crc32fast::hash(&bytes) != entry.crc {
            return Err(failed("the archive is damaged: the entry is not what it was"));
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn written(zip64: bool, entries: &[(&str, &[u8], Method)]) -> Vec<u8> {
        let mut w = Writer::new(Vec::new());
        if zip64 {
            w = w.always_zip64();
        }
        for (name, bytes, method) in entries {
            w.add(name, bytes, *method).unwrap();
        }
        w.finish().unwrap()
    }

    /// What is written is read as it was: stored, deflated, empty, named in any script, with and
    /// without ZIP64 records; the same entries are the same bytes whenever they are written.
    #[test]
    fn entries_are_read_as_they_were_written() {
        let text = "{\"format\": \"x\"}".repeat(200);
        let noise: Vec<u8> = (0..70_000u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8).collect();
        let entries: [(&str, &[u8], Method); 5] = [
            ("mimetype", b"application/x-test", Method::Stored),
            ("package.json", text.as_bytes(), Method::Deflated),
            ("files/ab/кирпич.png", &noise, Method::Deflated),
            ("empty", b"", Method::Deflated),
            ("graphs/1.json", b"{}", Method::Stored),
        ];
        for zip64 in [false, true] {
            let bytes = written(zip64, &entries);
            assert_eq!(&bytes[30..38], b"mimetype", "the type is where it can be told by the first bytes");
            let mut archive = Archive::new(Cursor::new(bytes.clone())).unwrap();
            let names: Vec<&str> = archive.entries().iter().map(|e| e.name.as_str()).collect();
            assert_eq!(names, entries.iter().map(|e| e.0).collect::<Vec<_>>());
            for (name, content, _) in entries {
                assert_eq!(archive.read(name, 1 << 20).unwrap(), content, "{name}, ZIP64 {zip64}");
            }
            assert_eq!(written(zip64, &entries), bytes, "the same bytes");
        }
        // Deflating what does not shrink keeps it as it is.
        assert!(written(false, &[("n", &noise, Method::Deflated)]).len() < noise.len() + 200);
    }

    /// An archive holds no entry twice, nor one named to be put anywhere else.
    #[test]
    fn names_are_what_an_entry_may_be_named() {
        let mut w = Writer::new(Vec::new());
        w.add("a/b.json", b"1", Method::Stored).unwrap();
        for bad in ["a/b.json", "", "/abs", "a/../b", "./a", "a//b", "c:/x", "a\\b", "x\0"] {
            assert!(w.add(bad, b"2", Method::Stored).is_err(), "{bad:?}");
        }
        assert!(is_entry_name("files/0f/кирпич.png") && !is_entry_name("files/"));
    }

    /// What an archive says is checked before it is believed: what is not one, is cut short,
    /// damaged, holds more than may be read or unpacks to more, or is kept as this reads not,
    /// says so; nothing takes more memory than may be read.
    #[test]
    fn what_an_archive_says_is_checked() {
        let big = vec![b'a'; 1 << 20];
        let bytes = written(false, &[("a", b"hello", Method::Stored), ("bomb", &big, Method::Deflated)]);
        assert!(bytes.len() < 10_000, "a megabyte of one letter deflates to little: {}", bytes.len());
        let open = |b: &[u8]| Archive::new(Cursor::new(b.to_vec()));
        let mut archive = open(&bytes).unwrap();
        assert!(archive.read("bomb", 1000).unwrap_err().contains("more than 1000"));
        assert!(archive.read("nope", 10).unwrap_err().contains("not in the archive"));
        assert!(open(b"").err().unwrap().contains("not an archive"));
        assert!(open(b"\x89PNG\r\n\x1a\n not a zip").err().unwrap().contains("not an archive"));
        assert!(open(&bytes[..bytes.len() - 30]).is_err(), "cut short");
        // A byte of an entry changed: its CRC says so.
        let mut changed = bytes.clone();
        let at = changed.windows(5).position(|w| w == b"hello").unwrap();
        changed[at] = b'j';
        assert!(open(&changed).unwrap().read("a", 10).unwrap_err().contains("not what it was"));
        // An entry that says it holds more than it does.
        let mut lying = bytes.clone();
        let central = lying.windows(4).position(|w| w == CENTRAL.to_le_bytes()).unwrap();
        lying[central + 24..central + 28].copy_from_slice(&6u32.to_le_bytes());
        assert!(open(&lying).unwrap().read("a", 10).unwrap_err().contains("damaged"));
        // Encrypted, or compressed another way.
        let mut locked = bytes.clone();
        locked[central + 8] |= 1;
        assert!(open(&locked).unwrap().read("a", 10).unwrap_err().contains("encrypted"));
        let mut other = bytes;
        other[central + 10] = 12;
        assert!(open(&other).unwrap().read("a", 10).unwrap_err().contains("method 12"));
    }

    /// Archives written elsewhere are read: by Python's zipfile (the sizes after each entry, in
    /// a descriptor, as a writer that cannot go back writes them), by Info-ZIP with a comment,
    /// and with ZIP64 records as 7-Zip and others write them.
    #[test]
    fn archives_written_elsewhere_are_read() {
        // `zipfile` writing to a stream it cannot seek: descriptors after deflated entries.
        const DESCRIPTORS: &[u8] = include_bytes!("../tests/fixtures/python-descriptors.zip");
        let mut archive = Archive::new(Cursor::new(DESCRIPTORS)).unwrap();
        assert_eq!(archive.read("hello.txt", 100).unwrap(), b"hello, world\n".repeat(3));
        assert_eq!(archive.read("dir/deflated.json", 10_000).unwrap(), br#"{"a": [1, 2, 3]}"#.repeat(40));
        // `zipfile` told to write ZIP64 records in the headers of an entry.
        const ZIP64: &[u8] = include_bytes!("../tests/fixtures/python-zip64.zip");
        let mut archive = Archive::new(Cursor::new(ZIP64)).unwrap();
        assert_eq!(archive.read("big.txt", 10_000).unwrap(), b"zip64 ".repeat(1000));
        assert_eq!(archive.read("stored.bin", 256).unwrap(), (0..=255u8).collect::<Vec<_>>());
        // A comment after the end record.
        let mut commented = written(false, &[("a", b"x", Method::Stored)]);
        let n = commented.len();
        commented[n - 2..].copy_from_slice(&7u16.to_le_bytes());
        commented.extend(b"comment");
        assert_eq!(Archive::new(Cursor::new(commented)).unwrap().read("a", 1).unwrap(), b"x");
    }
}
