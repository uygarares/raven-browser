//! A small reader for SQLite database files: enough to read every row of one
//! table. Used to import history from Chrome, Edge and Brave (their "History"
//! file is SQLite) without a C library. Read-only; journals and WAL files are
//! not read, so the very latest visits may be missing.

/// A column value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

impl Value {
    pub fn as_text(&self) -> &str {
        match self {
            Value::Text(text) => text,
            _ => "",
        }
    }
    pub fn as_int(&self) -> i64 {
        match self {
            Value::Int(n) => *n,
            Value::Real(f) => *f as i64,
            _ => 0,
        }
    }
}

pub struct Database<'a> {
    bytes: &'a [u8],
    page_size: usize,
    usable: usize,
}

fn varint(bytes: &[u8], at: usize) -> Option<(u64, usize)> {
    let mut value: u64 = 0;
    for i in 0..9 {
        let byte = *bytes.get(at + i)?;
        if i == 8 {
            return Some(((value << 8) | byte as u64, 9));
        }
        value = (value << 7) | (byte & 0x7f) as u64;
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

fn be(bytes: &[u8], at: usize, len: usize) -> Option<u64> {
    let slice = bytes.get(at..at + len)?;
    Some(slice.iter().fold(0u64, |acc, b| (acc << 8) | *b as u64))
}

impl<'a> Database<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Self, String> {
        if !bytes.starts_with(b"SQLite format 3\0") {
            return Err("Not an SQLite file".into());
        }
        let size = be(bytes, 16, 2).ok_or("Short file")? as usize;
        let page_size = if size == 1 { 65_536 } else { size };
        let reserved = *bytes.get(20).ok_or("Short file")? as usize;
        if page_size < 512 {
            return Err("Bad page size".into());
        }
        Ok(Database { bytes, page_size, usable: page_size - reserved })
    }

    fn page(&self, number: u64) -> Option<&'a [u8]> {
        let start = (number as usize).checked_sub(1)? * self.page_size;
        self.bytes.get(start..start + self.page_size)
    }

    /// The payload of a cell, following overflow pages.
    fn payload(&self, page: &[u8], at: usize, size: usize) -> Option<Vec<u8>> {
        let u = self.usable;
        let x = u - 35;
        if size <= x {
            return page.get(at..at + size).map(|s| s.to_vec());
        }
        let m = ((u - 12) * 32 / 255) - 23;
        let k = m + ((size - m) % (u - 4));
        let local = if k <= x { k } else { m };
        let mut out = page.get(at..at + local)?.to_vec();
        let mut next = be(page, at + local, 4)?;
        let mut guard = 0;
        while out.len() < size && next != 0 && guard < 100_000 {
            let overflow = self.page(next)?;
            next = be(overflow, 0, 4)?;
            let take = (size - out.len()).min(u - 4);
            out.extend_from_slice(overflow.get(4..4 + take)?);
            guard += 1;
        }
        Some(out)
    }

    /// Every row of the table b-tree starting at `root`, as decoded records.
    pub fn rows(&self, root: u64) -> Vec<Vec<Value>> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        let mut visited = 0usize;
        while let Some(number) = stack.pop() {
            visited += 1;
            if visited > 1_000_000 {
                break;
            }
            let Some(page) = self.page(number) else { continue };
            let header = if number == 1 { 100 } else { 0 };
            let Some(kind) = page.get(header) else { continue };
            let cells = be(page, header + 3, 2).unwrap_or(0) as usize;
            match kind {
                0x05 => {
                    // Interior table page: children, then the rightmost one.
                    let pointers = header + 12;
                    let mut children = Vec::with_capacity(cells + 1);
                    for i in 0..cells {
                        if let Some(cell) = be(page, pointers + i * 2, 2) {
                            if let Some(child) = be(page, cell as usize, 4) {
                                children.push(child);
                            }
                        }
                    }
                    if let Some(right) = be(page, header + 8, 4) {
                        children.push(right);
                    }
                    stack.extend(children.into_iter().rev());
                }
                0x0d => {
                    let pointers = header + 8;
                    for i in 0..cells {
                        let Some(cell) = be(page, pointers + i * 2, 2) else { continue };
                        let at = cell as usize;
                        let Some((size, n1)) = varint(page, at) else { continue };
                        let Some((rowid, n2)) = varint(page, at + n1) else { continue };
                        let Some(payload) = self.payload(page, at + n1 + n2, size as usize) else { continue };
                        if let Some(mut record) = record(&payload) {
                            // An INTEGER PRIMARY KEY column is stored as NULL: it is the rowid.
                            if let Some(first) = record.first_mut() {
                                if *first == Value::Null {
                                    *first = Value::Int(rowid as i64);
                                }
                            }
                            out.push(record);
                        }
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// The root page of a table, from sqlite_master.
    pub fn table_root(&self, name: &str) -> Option<u64> {
        self.rows(1).into_iter().find_map(|row| {
            if row.first()?.as_text() == "table" && row.get(1)?.as_text() == name {
                Some(row.get(3)?.as_int() as u64)
            } else {
                None
            }
        })
    }
}

fn record(payload: &[u8]) -> Option<Vec<Value>> {
    let (header_size, mut at) = varint(payload, 0)?;
    let mut types = Vec::new();
    while at < header_size as usize {
        let (kind, n) = varint(payload, at)?;
        types.push(kind);
        at += n;
    }
    let mut body = header_size as usize;
    let mut values = Vec::with_capacity(types.len());
    for kind in types {
        let value = match kind {
            0 => Value::Null,
            1..=6 => {
                let len = [0, 1, 2, 3, 4, 6, 8][kind as usize];
                let raw = be(payload, body, len)?;
                body += len;
                let shift = 64 - len * 8;
                Value::Int(((raw << shift) as i64) >> shift)
            }
            7 => {
                let raw = be(payload, body, 8)?;
                body += 8;
                Value::Real(f64::from_bits(raw))
            }
            8 => Value::Int(0),
            9 => Value::Int(1),
            n if n >= 12 && n % 2 == 0 => {
                let len = ((n - 12) / 2) as usize;
                let data = payload.get(body..body + len)?.to_vec();
                body += len;
                Value::Blob(data)
            }
            n if n >= 13 => {
                let len = ((n - 13) / 2) as usize;
                let data = payload.get(body..body + len)?;
                body += len;
                Value::Text(String::from_utf8_lossy(data).into_owned())
            }
            _ => Value::Null,
        };
        values.push(value);
    }
    Some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_varints_and_records() {
        assert_eq!(varint(&[0x81, 0x00], 0), Some((128, 2)));
        // Header size 3, types: int8, text of 2.
        let payload = [3u8, 1, 17, 0xff, b'h', b'i'];
        assert_eq!(record(&payload), Some(vec![Value::Int(-1), Value::Text("hi".into())]));
    }

    #[test]
    fn reads_a_real_database() {
        // tests/history.sqlite: a "urls" table with 3000 rows (some long), made by sqlite3.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/history.sqlite");
        let Ok(bytes) = std::fs::read(path) else { return };
        let db = Database::open(&bytes).unwrap();
        let root = db.table_root("urls").unwrap();
        let rows = db.rows(root);
        assert_eq!(rows.len(), 3000);
        let row = rows.iter().find(|r| r[0] == Value::Int(1234)).unwrap();
        assert_eq!(row[1].as_text(), "https://example.com/page/1234");
        assert_eq!(row[3].as_int(), 1234 % 17);
        let long = rows.iter().find(|r| r[0] == Value::Int(7)).unwrap();
        assert_eq!(long[2].as_text().len(), 9000);
    }
}
