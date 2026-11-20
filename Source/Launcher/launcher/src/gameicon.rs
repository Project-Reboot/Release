use std::path::Path;

const MAX_ICON_BYTES: usize = 4 * 1024 * 1024;

pub fn load(exe: &Path) -> Option<image::RgbaImage> {
    let picture = platform::load(exe)?;
    (picture.width() >= 16 && picture.height() >= 16 && picture.width() <= 1024 && picture.height() <= 1024).then_some(picture)
}

fn decode_icon_entry(entry: &[u8], width: u8, height: u8, colors: u8, planes: u16, bits: u16) -> Option<image::RgbaImage> {
    if entry.len() > MAX_ICON_BYTES {
        return None;
    }
    if entry.starts_with(b"\x89PNG\r\n\x1a\n") {
        return image::load_from_memory_with_format(entry, image::ImageFormat::Png).ok().map(|i| i.to_rgba8());
    }
    let mut ico = Vec::with_capacity(22 + entry.len());
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0, width, height, colors, 0]);
    ico.extend_from_slice(&planes.to_le_bytes());
    ico.extend_from_slice(&bits.to_le_bytes());
    ico.extend_from_slice(&(entry.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(entry);
    image::load_from_memory_with_format(&ico, image::ImageFormat::Ico).ok().map(|i| i.to_rgba8())
}

pub fn best_entry(group: &[u8]) -> Option<(u16, u8, u8, u8, u16, u16)> {
    let count = u16::from_le_bytes([*group.get(4)?, *group.get(5)?]) as usize;
    let mut best: Option<(u32, u16, (u16, u8, u8, u8, u16, u16))> = None;
    for i in 0..count {
        let at = 6 + i * 14;
        let e = group.get(at..at + 14)?;
        let size = if e[0] == 0 { 256 } else { e[0] as u32 };
        let planes = u16::from_le_bytes([e[4], e[5]]);
        let bits = u16::from_le_bytes([e[6], e[7]]);
        let id = u16::from_le_bytes([e[12], e[13]]);
        if best.as_ref().map(|(s, b, _)| (size, bits) > (*s, *b)).unwrap_or(true) {
            best = Some((size, bits, (id, e[0], e[1], e[2], planes, bits)));
        }
    }
    best.map(|(_, _, entry)| entry)
}

#[cfg(windows)]
mod platform {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    use windows_sys::Win32::Foundation::{FreeLibrary, BOOL, HMODULE};
    use windows_sys::Win32::System::LibraryLoader::{
        EnumResourceNamesW, FindResourceW, LoadLibraryExW, LoadResource, LockResource, SizeofResource, LOAD_LIBRARY_AS_DATAFILE, LOAD_LIBRARY_AS_IMAGE_RESOURCE,
    };

    const RT_ICON: usize = 3;
    const RT_GROUP_ICON: usize = 14;

    enum Name {
        Id(usize),
        Text(Vec<u16>),
    }

    unsafe extern "system" fn first_name(_module: HMODULE, _kind: *const u16, name: *const u16, found: isize) -> BOOL {
        let found = &mut *(found as *mut Option<Name>);
        if (name as usize) >> 16 == 0 {
            *found = Some(Name::Id(name as usize));
        } else {
            let mut text = Vec::new();
            let mut at = name;
            while *at != 0 && text.len() < 256 {
                text.push(*at);
                at = at.add(1);
            }
            text.push(0);
            *found = Some(Name::Text(text));
        }
        0
    }

    unsafe fn resource(module: HMODULE, name: *const u16, kind: usize) -> Option<Vec<u8>> {
        let info = FindResourceW(module, name, kind as *const u16);
        if info.is_null() {
            return None;
        }
        let size = SizeofResource(module, info) as usize;
        let handle = LoadResource(module, info);
        if handle.is_null() || size == 0 || size > super::MAX_ICON_BYTES {
            return None;
        }
        let data = LockResource(handle) as *const u8;
        (!data.is_null()).then(|| std::slice::from_raw_parts(data, size).to_vec())
    }

    pub fn load(exe: &Path) -> Option<image::RgbaImage> {
        let wide: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            let module = LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE);
            if module.is_null() {
                return None;
            }
            let mut found: Option<Name> = None;
            EnumResourceNamesW(module, RT_GROUP_ICON as *const u16, Some(first_name), &mut found as *mut Option<Name> as isize);
            let picture = found.and_then(|name| {
                let name_ptr = match &name {
                    Name::Id(id) => *id as *const u16,
                    Name::Text(text) => text.as_ptr(),
                };
                let group = resource(module, name_ptr, RT_GROUP_ICON)?;
                let (id, width, height, colors, planes, bits) = super::best_entry(&group)?;
                let entry = resource(module, id as usize as *const u16, RT_ICON)?;
                super::decode_icon_entry(&entry, width, height, colors, planes, bits)
            });
            FreeLibrary(module);
            picture
        }
    }
}

#[cfg(not(windows))]
mod platform {
    use std::path::Path;

    pub fn load(exe: &Path) -> Option<image::RgbaImage> {
        let dir = exe.parent()?;
        let stem = exe.file_stem()?.to_string_lossy().to_string();
        let file = dir.join(format!("{stem}_Data")).join("Resources").join("UnityPlayer.png");
        let bytes = std::fs::read(file).ok().filter(|b| b.len() <= super::MAX_ICON_BYTES)?;
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok().map(|i| i.to_rgba8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_largest_and_deepest_icon() {
        let mut group = vec![0, 0, 1, 0, 3, 0];
        for (w, bits, id) in [(32u8, 32u16, 1u16), (0, 32, 2), (0, 8, 3)] {
            group.extend_from_slice(&[w, w, 0, 0, 1, 0]);
            group.extend_from_slice(&bits.to_le_bytes());
            group.extend_from_slice(&100u32.to_le_bytes());
            group.extend_from_slice(&id.to_le_bytes());
        }
        assert_eq!(best_entry(&group).map(|e| e.0), Some(2));
        assert_eq!(best_entry(&group[..10]), None);
    }

    #[test]
    fn decodes_png_and_bitmap_icon_entries() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(32, 32, image::Rgba([10, 20, 30, 255])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        assert_eq!(decode_icon_entry(&png, 32, 32, 0, 1, 32).map(|i| i.dimensions()), Some((32, 32)));
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes());
        dib.extend_from_slice(&16i32.to_le_bytes());
        dib.extend_from_slice(&32i32.to_le_bytes());
        dib.extend_from_slice(&1u16.to_le_bytes());
        dib.extend_from_slice(&32u16.to_le_bytes());
        dib.extend_from_slice(&[0u8; 24]);
        dib.extend(std::iter::repeat([200u8, 100, 50, 255]).take(16 * 16).flatten());
        dib.extend(std::iter::repeat(0u8).take(16 * 4));
        let picture = decode_icon_entry(&dib, 16, 16, 0, 1, 32).expect("bitmap entry decodes");
        assert_eq!(picture.dimensions(), (16, 16));
        assert_eq!(picture.get_pixel(0, 0).0, [50, 100, 200, 255]);
        assert!(decode_icon_entry(b"not an icon", 16, 16, 0, 1, 32).is_none());
    }
}
