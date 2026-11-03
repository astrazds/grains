const MAX_EDGE: u32 = 4096;
const MAX_CELLS: usize = 4096 * 2048;

pub const FILE_NAME: &str = "world.grains";
pub const LINK_LIMIT: usize = 16_384;

const MAGIC: &[u8; 4] = b"GRNS";
const VERSION: u16 = 1;
const HEADER: usize = 10;
pub const MAX_FILE: usize = HEADER + MAX_CELLS * 8;

const NOT_WORLD: &str = "this is not a grains world";
const CUT_OFF: &str = "this world file is cut off";
const DAMAGED: &str = "this world file is damaged";
pub const TOO_LARGE: &str = "this file is too large to be a grains world";
const TOO_MANY: &str = "this world file has too many cells";
const LINK_TOO_BIG: &str = "this world is too big for a link";

#[derive(Debug)]
pub struct Saved {
    pub width: u32,
    pub height: u32,
    pub words: Vec<u32>,
}

#[cfg(not(target_arch = "wasm32"))]
pub fn replace_file(directory: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut pending = tempfile::NamedTempFile::new_in(directory)?;
    pending.write_all(bytes)?;
    pending.as_file().sync_all()?;
    pending.persist(directory.join(FILE_NAME))?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn read_file() -> std::io::Result<Vec<u8>> {
    use std::io::{Error, ErrorKind, Read};
    let file = std::fs::File::open(FILE_NAME)?;
    if file.metadata()?.len() > MAX_FILE as u64 {
        return Err(Error::new(ErrorKind::InvalidData, TOO_LARGE));
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE as u64 + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

pub fn encode(words: &[u32], width: u32, height: u32) -> Result<Vec<u8>, String> {
    if width == 0
        || height == 0
        || width > MAX_EDGE
        || height > MAX_EDGE
        || words.len() > MAX_CELLS
        || words.len() != width as usize * height as usize
    {
        return Err("the grid is the wrong size to save".to_string());
    }
    let mut out = Vec::with_capacity(HEADER + 8);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(width as u16).to_le_bytes());
    out.extend_from_slice(&(height as u16).to_le_bytes());
    let mut index = 0;
    while index < words.len() {
        let value = words[index];
        let mut run = 1usize;
        while index + run < words.len() && words[index + run] == value {
            run += 1;
        }
        out.extend_from_slice(&value.to_le_bytes());
        out.extend_from_slice(&(run as u32).to_le_bytes());
        index += run;
    }
    Ok(out)
}

pub fn decode(bytes: &[u8]) -> Result<Saved, String> {
    if bytes.len() > MAX_FILE {
        return Err(TOO_LARGE.to_string());
    }
    if bytes.len() < 4 {
        return Err(CUT_OFF.to_string());
    }
    if &bytes[..4] != MAGIC {
        return Err(NOT_WORLD.to_string());
    }
    if bytes.len() < HEADER {
        return Err(CUT_OFF.to_string());
    }
    let version = u16::from_le_bytes([bytes[4], bytes[5]]);
    if version != VERSION {
        return Err(format!(
            "this world is version {version}, and grains reads version {VERSION}"
        ));
    }
    let width = u16::from_le_bytes([bytes[6], bytes[7]]);
    let height = u16::from_le_bytes([bytes[8], bytes[9]]);
    let count = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || u32::from(width) > MAX_EDGE
        || u32::from(height) > MAX_EDGE
        || count > MAX_CELLS as u64
    {
        return Err(format!(
            "this world is {width} by {height}, which is larger than grains can load"
        ));
    }
    let count = count as usize;
    let mut words = vec![0u32; count];
    let mut filled = 0usize;
    let mut cursor = HEADER;
    while filled < count {
        if cursor + 8 > bytes.len() {
            return Err(CUT_OFF.to_string());
        }
        let value = u32::from_le_bytes([
            bytes[cursor],
            bytes[cursor + 1],
            bytes[cursor + 2],
            bytes[cursor + 3],
        ]);
        let run = u32::from_le_bytes([
            bytes[cursor + 4],
            bytes[cursor + 5],
            bytes[cursor + 6],
            bytes[cursor + 7],
        ]);
        cursor += 8;
        if run == 0 {
            return Err(DAMAGED.to_string());
        }
        let run = run as usize;
        if run > count - filled {
            return Err(TOO_MANY.to_string());
        }
        words[filled..filled + run].fill(value);
        filled += run;
    }
    if cursor != bytes.len() {
        return Err(DAMAGED.to_string());
    }
    Ok(Saved {
        width: u32::from(width),
        height: u32::from(height),
        words,
    })
}

pub fn hash_fragment(file: &[u8]) -> Result<String, String> {
    if file.len() > LINK_LIMIT {
        return Err(LINK_TOO_BIG.to_string());
    }
    Ok(base64url(file))
}

#[cfg(not(target_arch = "wasm32"))]
pub fn desktop_fragment(words: &[u32]) -> Result<String, String> {
    let (width, height) = crate::grid::world_size();
    let (web_w, web_h) = crate::grid::WEB_WORLD;
    if words.len() != width * height {
        return Err("the grid is the wrong size to save".to_string());
    }
    // Centre the browser-sized crop around all occupied cells. Never drop a word.
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
    for (y, row) in words.chunks_exact(width).enumerate() {
        if let Some(x) = row.iter().position(|&word| word != 0) {
            left = left.min(x);
            right = right.max(row.iter().rposition(|&word| word != 0).unwrap() + 1);
            top = top.min(y);
            bottom = y + 1;
        }
    }
    if right.saturating_sub(left) > web_w || bottom.saturating_sub(top) > web_h {
        return Err("the world is too wide or tall for a browser link".to_string());
    }
    let x = (left + right)
        .saturating_sub(web_w)
        .div_euclid(2)
        .min(width - web_w);
    let y = (top + bottom)
        .saturating_sub(web_h)
        .div_euclid(2)
        .min(height - web_h);
    let mut crop = Vec::with_capacity(web_w * web_h);
    for row in words.chunks_exact(width).skip(y).take(web_h) {
        crop.extend_from_slice(&row[x..x + web_w]);
    }
    hash_fragment(&encode(&crop, web_w as u32, web_h as u32)?)
}

#[cfg(any(test, target_arch = "wasm32"))]
pub fn bytes_from_hash(hash: &str) -> Result<Vec<u8>, String> {
    let text = hash.trim().trim_start_matches('#');
    if text.is_empty() {
        return Err("this link has no world".to_string());
    }
    if text.len() > (LINK_LIMIT * 4).div_ceil(3) {
        return Err(LINK_TOO_BIG.to_string());
    }
    base64url_decode(text)
}

fn base64url(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let value = (u32::from(bytes[index]) << 16)
            | (u32::from(bytes[index + 1]) << 8)
            | u32::from(bytes[index + 2]);
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
        out.push(TABLE[((value >> 6) & 63) as usize] as char);
        out.push(TABLE[(value & 63) as usize] as char);
        index += 3;
    }
    let rest = &bytes[index..];
    if rest.len() == 1 {
        let value = u32::from(rest[0]) << 16;
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
    } else if rest.len() == 2 {
        let value = (u32::from(rest[0]) << 16) | (u32::from(rest[1]) << 8);
        out.push(TABLE[((value >> 18) & 63) as usize] as char);
        out.push(TABLE[((value >> 12) & 63) as usize] as char);
        out.push(TABLE[((value >> 6) & 63) as usize] as char);
    }
    out
}

#[cfg(any(test, target_arch = "wasm32"))]
fn base64url_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut alphabet = [0xffu8; 256];
    for (index, byte) in b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
        .iter()
        .enumerate()
    {
        alphabet[*byte as usize] = index as u8;
    }
    let bytes = text.as_bytes();
    if bytes.len() % 4 == 1 {
        return Err("this link is damaged".to_string());
    }
    let mut out = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let mut take = [0u8; 4];
        let mut count = 0;
        while count < 4 && index < bytes.len() {
            let value = alphabet[bytes[index] as usize];
            if value == 0xff {
                return Err("this link is damaged".to_string());
            }
            take[count] = value;
            count += 1;
            index += 1;
        }
        if count == 1 {
            return Err("this link is damaged".to_string());
        }
        out.push((take[0] << 2) | (take[1] >> 4));
        if count > 2 {
            out.push((take[1] << 4) | (take[2] >> 2));
        }
        if count > 3 {
            out.push((take[2] << 6) | take[3]);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        CUT_OFF, DAMAGED, LINK_TOO_BIG, NOT_WORLD, TOO_LARGE, TOO_MANY, bytes_from_hash, decode,
        encode, hash_fragment,
    };
    use crate::grid::{CELL_COUNT, Cell, SAND, STONE};

    #[test]
    fn loading_during_a_save_reads_one_complete_world() {
        use std::io::Read;
        let dir = tempfile::tempdir().unwrap();
        let first = encode(&[0], 1, 1).unwrap();
        let second = encode(&[2, 3], 2, 1).unwrap();
        super::replace_file(dir.path(), &first).unwrap();
        let path = dir.path().join(super::FILE_NAME);
        let mut reader = std::fs::File::open(&path).unwrap();
        let replace = super::replace_file(dir.path(), &second);
        let mut prior = Vec::new();
        reader.read_to_end(&mut prior).unwrap();
        assert_eq!(prior, first);
        drop(reader);
        if let Err(error) = replace {
            // Windows can refuse to replace an open file. The original must
            // still be intact, and saving again after the reader closes works.
            assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
            assert_eq!(std::fs::read(&path).unwrap(), first);
            super::replace_file(dir.path(), &second).unwrap();
        }
        assert_eq!(std::fs::read(path).unwrap(), second);
    }

    #[test]
    fn oversized_hashes_are_rejected_on_input() {
        let bytes = vec![0; super::LINK_LIMIT + 1];
        assert_eq!(
            bytes_from_hash(&super::base64url(&bytes)).unwrap_err(),
            LINK_TOO_BIG
        );
    }

    #[test]
    fn encoder_refuses_unreadable_dimensions() {
        assert!(encode(&vec![0; 65537], 65537, 1).is_err());
    }

    #[test]
    fn loading_a_legacy_world_brings_its_cells_into_view() {
        let mut app = crate::App::new();
        let mut words = vec![0; 512 * 288];
        words[256 + 144 * 512] = 0xab01;
        app.apply_file(&encode(&words, 512, 288).unwrap());
        assert_eq!(app.grid.cell(256, 144).to_bits(), 0xab01);
        let x = (256.0 - app.camera.cell_x) * app.camera.zoom;
        let y = (144.0 - app.camera.cell_y) * app.camera.zoom;
        assert!((0.0..1024.0).contains(&x) && (0.0..600.0).contains(&y));
        app.apply_file(b"nope");
        assert_eq!(app.grid.cell(256, 144).to_bits(), 0xab01);
    }

    #[test]
    fn a_desktop_link_fits_the_browser_world() {
        let mut words = vec![0; 2048 * 1152];
        words[800 * 2048 + 1600] = 0xab01;
        let fragment = super::desktop_fragment(&words).unwrap();
        let saved = decode(&bytes_from_hash(&fragment).unwrap()).unwrap();
        assert!(saved.width <= 1024 && saved.height <= 576);
        assert_eq!(saved.words[288 * 1024 + 576], 0xab01);
        words[0] = 2;
        assert!(super::desktop_fragment(&words).is_err());
    }

    #[test]
    fn a_world_keeps_its_colour_bytes() {
        let mut words = vec![0u32; CELL_COUNT];
        words[0] = Cell::pack(SAND, 0xab, 0x11, 0x04).to_bits();
        words[1] = Cell::pack(STONE, 0x03, 0, 0).to_bits();
        let file = encode(&words, 512, 288).expect("encode");
        assert_eq!(&file[..4], b"GRNS");
        assert_eq!(u16::from_le_bytes([file[4], file[5]]), 1);
        assert_eq!(u16::from_le_bytes([file[6], file[7]]), 512);
        assert_eq!(u16::from_le_bytes([file[8], file[9]]), 288);
        assert!(file.len() < words.len() * 4, "the file was not compact");
        let loaded = decode(&file).expect("decode");
        assert_eq!((loaded.width, loaded.height), (512, 288));
        assert_eq!(loaded.words[0], words[0]);
        assert_eq!(loaded.words[1], words[1]);
        assert_eq!(loaded.words[2], 0);
        let link = hash_fragment(&file).expect("link");
        let from_link =
            decode(&bytes_from_hash(&format!("#{link}")).expect("hash")).expect("world");
        assert_eq!(from_link.words[0], words[0]);
        let big = vec![7u32; 128 * 64];
        let wide = decode(&encode(&big, 128, 64).expect("wide")).expect("wide decode");
        assert_eq!((wide.width, wide.height, wide.words[0]), (128, 64, 7));
    }

    #[test]
    fn corrupt_and_truncated_files_are_rejected() {
        let words = vec![0u32; CELL_COUNT];
        let file = encode(&words, 512, 288).expect("encode");
        assert_eq!(decode(b"nope").unwrap_err(), NOT_WORLD);
        assert_eq!(decode(b"GR").unwrap_err(), CUT_OFF);
        assert_eq!(decode(&file[..file.len() - 1]).unwrap_err(), CUT_OFF);
        let mut version = file.clone();
        version[4] = 2;
        assert_eq!(
            decode(&version).unwrap_err(),
            "this world is version 2, and grains reads version 1"
        );
        let mut wide = file.clone();
        wide[6] = 0x40;
        wide[7] = 0x9c;
        wide[8] = 0x40;
        wide[9] = 0x9c;
        assert_eq!(
            decode(&wide).unwrap_err(),
            "this world is 40000 by 40000, which is larger than grains can load"
        );
        let mut small = file.clone();
        small[6] = 1;
        small[7] = 0;
        small[8] = 1;
        small[9] = 0;
        assert_eq!(decode(&small).unwrap_err(), TOO_MANY);
        let mut zero_run = file.clone();
        let len = zero_run.len();
        zero_run[len - 4..].fill(0);
        assert_eq!(decode(&zero_run).unwrap_err(), DAMAGED);
        let mut extra = file.clone();
        extra.push(0);
        assert_eq!(decode(&extra).unwrap_err(), DAMAGED);
        let mut huge = vec![0u8; super::MAX_FILE + 1];
        huge[..4].copy_from_slice(b"GRNS");
        assert_eq!(decode(&huge).unwrap_err(), TOO_LARGE);
        let dense: Vec<u32> = (0..CELL_COUNT as u32).collect();
        let dense_file = encode(&dense, 512, 288).expect("dense");
        assert_eq!(hash_fragment(&dense_file).unwrap_err(), LINK_TOO_BIG);
    }
}
