use crate::grid::{CELL_COUNT, GRID_HEIGHT, GRID_WIDTH};

pub const FILE_NAME: &str = "world.grains";
pub const LINK_LIMIT: usize = 16_384;

const MAGIC: &[u8; 4] = b"GRNS";
const VERSION: u16 = 1;
const HEADER: usize = 10;
const MAX_FILE: usize = HEADER + CELL_COUNT * 8;

const NOT_WORLD: &str = "this is not a grains world";
const CUT_OFF: &str = "this world file is cut off";
const DAMAGED: &str = "this world file is damaged";
const TOO_LARGE: &str = "this file is too large to be a grains world";
const TOO_MANY: &str = "this world file has too many cells";
const LINK_TOO_BIG: &str = "this world is too big for a link";

pub fn encode(words: &[u32]) -> Result<Vec<u8>, String> {
    if words.len() != CELL_COUNT {
        return Err("the grid is the wrong size to save".to_string());
    }
    let mut out = Vec::with_capacity(HEADER + 8);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(GRID_WIDTH as u16).to_le_bytes());
    out.extend_from_slice(&(GRID_HEIGHT as u16).to_le_bytes());
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

pub fn decode(bytes: &[u8]) -> Result<Vec<u32>, String> {
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
    if u32::from(width) > GRID_WIDTH as u32
        || u32::from(height) > GRID_HEIGHT as u32
        || count > CELL_COUNT as u64
    {
        return Err(format!(
            "this world is {width} by {height}, which is larger than grains can load"
        ));
    }
    if width != GRID_WIDTH as u16 || height != GRID_HEIGHT as u16 {
        return Err(format!(
            "this world is {width} by {height}, and grains is {GRID_WIDTH} by {GRID_HEIGHT}"
        ));
    }
    let mut words = vec![0u32; CELL_COUNT];
    let mut filled = 0usize;
    let mut cursor = HEADER;
    while filled < CELL_COUNT {
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
        if filled + run > CELL_COUNT {
            return Err(TOO_MANY.to_string());
        }
        words[filled..filled + run].fill(value);
        filled += run;
    }
    if cursor != bytes.len() {
        return Err(DAMAGED.to_string());
    }
    Ok(words)
}

pub fn hash_fragment(file: &[u8]) -> Result<String, String> {
    if file.len() > LINK_LIMIT {
        return Err(LINK_TOO_BIG.to_string());
    }
    Ok(base64url(file))
}

#[cfg(any(test, target_arch = "wasm32"))]
pub fn bytes_from_hash(hash: &str) -> Result<Vec<u8>, String> {
    let text = hash.trim().trim_start_matches('#');
    if text.is_empty() {
        return Err("this link has no world".to_string());
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
        CUT_OFF, DAMAGED, LINK_TOO_BIG, NOT_WORLD, TOO_LARGE, bytes_from_hash, decode, encode,
        hash_fragment,
    };
    use crate::grid::{CELL_COUNT, Cell, SAND, STONE};

    #[test]
    fn a_world_keeps_its_colour_bytes() {
        let mut words = vec![0u32; CELL_COUNT];
        words[0] = Cell::pack(SAND, 0xab, 0x11, 0x04).to_bits();
        words[1] = Cell::pack(STONE, 0x03, 0, 0).to_bits();
        let file = encode(&words).expect("encode");
        assert_eq!(&file[..4], b"GRNS");
        assert_eq!(u16::from_le_bytes([file[4], file[5]]), 1);
        assert_eq!(u16::from_le_bytes([file[6], file[7]]), 512);
        assert_eq!(u16::from_le_bytes([file[8], file[9]]), 288);
        assert!(file.len() < words.len() * 4, "the file was not compact");
        let loaded = decode(&file).expect("decode");
        assert_eq!(loaded[0], words[0]);
        assert_eq!(loaded[1], words[1]);
        assert_eq!(loaded[2], 0);
        let link = hash_fragment(&file).expect("link");
        let from_link =
            decode(&bytes_from_hash(&format!("#{link}")).expect("hash")).expect("world");
        assert_eq!(from_link[0], words[0]);
    }

    #[test]
    fn corrupt_and_truncated_files_are_rejected() {
        let words = vec![0u32; CELL_COUNT];
        let file = encode(&words).expect("encode");
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
        assert_eq!(
            decode(&small).unwrap_err(),
            "this world is 1 by 1, and grains is 512 by 288"
        );
        let mut zero_run = file.clone();
        let len = zero_run.len();
        zero_run[len - 4..].fill(0);
        assert_eq!(decode(&zero_run).unwrap_err(), DAMAGED);
        let mut extra = file.clone();
        extra.push(0);
        assert_eq!(decode(&extra).unwrap_err(), DAMAGED);
        let mut huge = file.clone();
        huge.resize(file.len() + CELL_COUNT * 8, 0);
        assert_eq!(decode(&huge).unwrap_err(), TOO_LARGE);
        let dense: Vec<u32> = (0..CELL_COUNT as u32).collect();
        let dense_file = encode(&dense).expect("dense");
        assert_eq!(hash_fragment(&dense_file).unwrap_err(), LINK_TOO_BIG);
    }
}
