//! 字体集合（ttc）解析原语：面偏移与字体名（PostScript）定位，纯字节解析无平台依赖。
//!
//! 发现器据此把声明序号落成集合文件内的面序号；单面文件按 0 号面回答
//! （发现器侧判定）。本模块随发现器与其测试编译：发现器缺席的目标只剩
//! 测试消费这些原语。

pub(super) fn is_collection(bytes: &[u8]) -> bool {
    bytes.first_chunk::<4>() == Some(b"ttcf")
}

pub(super) fn face_offset(bytes: &[u8], index: usize) -> Result<usize, String> {
    u32_at(bytes, 12 + 4 * index)
        .map(|raw| raw as usize)
        .ok_or_else(|| "ttc face offset out of range".to_owned())
}

pub(super) fn face_postscript_name(bytes: &[u8], face: usize) -> Option<String> {
    let name_at = locate_name_table(bytes, face)?;
    let count = u16_at(bytes, name_at + 2)? as usize;
    let strings_at = name_at + u16_at(bytes, name_at + 4)? as usize;
    let mut ascii = None;
    for i in 0..count {
        let entry = name_at + 6 + 12 * i;
        let platform = u16_at(bytes, entry)?;
        let id = u16_at(bytes, entry + 6)?;
        if id != 6 {
            continue;
        }
        let length = u16_at(bytes, entry + 8)? as usize;
        let at = strings_at + u16_at(bytes, entry + 10)? as usize;
        let raw = bytes.get(at..at + length)?;
        if platform == 3 {
            let units: Vec<u16> = raw
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            return String::from_utf16(&units).ok();
        }
        if platform == 1 && ascii.is_none() && raw.is_ascii() {
            ascii = Some(raw.iter().map(|&byte| byte as char).collect::<String>());
        }
    }
    ascii
}

pub(super) fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn locate_name_table(bytes: &[u8], face: usize) -> Option<usize> {
    let num_tables = u16_at(bytes, face + 4)?;
    for i in 0..num_tables as usize {
        let record = face + 12 + 16 * i;
        if bytes.get(record..record + 4)? != b"name" {
            continue;
        }
        let at = u32_at(bytes, record + 8)? as usize;
        return (matches!(u16_at(bytes, at), Some(0 | 1))
            && u16_at(bytes, at + 2).unwrap_or(0) > 0)
            .then_some(at);
    }
    None
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}
