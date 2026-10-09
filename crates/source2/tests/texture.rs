//! Reads real textures from a Deadlock install, read-only. The pack is taken from
//! `SOURCE2_VPK_SAMPLES` (a directory holding `pak01_dir.vpk`, searched two levels deep) or,
//! when that is unset, from the default Steam path. When no pack is found the tests pass
//! without checking anything. No Valve data is written to disk.
#![cfg(all(feature = "vpk", feature = "resource", feature = "texture"))]
#![allow(missing_docs)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use source2::resource::Resource;
use source2::texture::{Error, Image, Texture};
use source2::vpk::Vpk;

const DEFAULT_PACK: &str =
    r"D:\Programs\Steam\steamapps\common\Deadlock\game\citadel\pak01_dir.vpk";
const MAX_SCANNED: usize = 600;

fn find_pack(dir: &Path, depth: usize) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            if depth > 0
                && let Some(found) = find_pack(&path, depth - 1)
            {
                return Some(found);
            }
        } else if path.file_name().is_some_and(|n| n == "pak01_dir.vpk") {
            return Some(path);
        }
    }
    None
}

fn open_pack() -> Option<Vpk> {
    let path = match std::env::var_os("SOURCE2_VPK_SAMPLES") {
        Some(dir) => find_pack(Path::new(&dir), 2),
        None => Some(PathBuf::from(DEFAULT_PACK)),
    }
    .filter(|p| p.is_file());
    let Some(path) = path else {
        eprintln!("skipped: no pak01_dir.vpk found");
        return None;
    };
    Some(Vpk::open(path).expect("open pack"))
}

fn with_texture<T>(
    pack: &mut Vpk,
    path: &str,
    f: impl FnOnce(&Texture<'_>) -> T,
) -> Result<T, Error> {
    let bytes = pack.read_path(path).expect("read entry");
    let resource = Resource::parse(&bytes).expect("parse resource");
    let data = resource.data().expect("DATA block");
    let texture = Texture::parse(data, &resource.trailing)?;
    Ok(f(&texture))
}

fn visible_pixels(img: &Image) -> usize {
    img.rgba
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[3] > 0 && p[..3] != [0, 0, 0])
        .count()
}

#[test]
fn real_hero_art_decodes() {
    let Some(mut pack) = open_pack() else {
        return;
    };
    for path in [
        "panorama/images/heroes/inferno_sm_psd.vtex_c",
        "panorama/images/heroes/inferno_card_psd.vtex_c",
        "panorama/images/ranked/badges/rank05_lg_psd.vtex_c",
    ] {
        let result = with_texture(&mut pack, path, |t| {
            let h = t.header();
            eprintln!(
                "{path}: {}x{} {} mips={} flags={:#x} extra={:?}",
                h.width, h.height, h.format, h.mip_count, h.flags, h.extra
            );
            let count = u32::from(h.mip_count);
            for level in 0..count {
                let img = t.decode_rgba8(level).expect("decode mip");
                let (w, hh) = t.mip_dimensions(level).unwrap();
                assert_eq!((img.width, img.height), (w, hh));
                assert_eq!(img.rgba.len(), (w * hh * 4) as usize);
            }
            let largest = t.decode_rgba8(0).unwrap();
            assert!(visible_pixels(&largest) > 0, "{path}: image is blank");
        });
        match result {
            Ok(()) => {}
            Err(e) => panic!("{path}: {e}"),
        }
    }
}

#[test]
fn real_textures_either_decode_or_name_what_is_unsupported() {
    let Some(pack) = open_pack() else {
        return;
    };
    let paths: Vec<String> = pack
        .entries()
        .iter()
        .filter(|e| e.path.ends_with(".vtex_c") && e.size() <= (8 << 20))
        .map(|e| e.path.clone())
        .collect();
    let step = (paths.len() / MAX_SCANNED).max(1);
    let mut outcomes: BTreeMap<String, usize> = BTreeMap::new();
    let mut bad = Vec::new();
    for path in paths.iter().step_by(step).take(MAX_SCANNED) {
        let bytes = pack.read_path(path).expect("read entry");
        let Ok(resource) = Resource::parse(&bytes) else {
            continue;
        };
        let Some(data) = resource.data() else {
            continue;
        };
        let outcome = match Texture::parse(data, &resource.trailing) {
            Ok(t) => {
                let mut ok = true;
                for level in 0..u32::from(t.header().mip_count) {
                    ok &= t.decode_rgba8(level).is_ok();
                }
                if ok {
                    format!("decoded {}", t.header().format)
                } else {
                    bad.push(format!("{path}: decode failed"));
                    "decode failed".to_owned()
                }
            }
            Err(
                e @ (Error::UnsupportedFormat { .. }
                | Error::UnsupportedLayout { .. }
                | Error::UnsupportedCompressedMips),
            ) => format!("unsupported: {e}"),
            Err(e) => {
                bad.push(format!("{path}: {e}"));
                "unexpected error".to_owned()
            }
        };
        *outcomes.entry(outcome).or_default() += 1;
    }
    eprintln!("{outcomes:#?}");
    for b in bad.iter().take(20) {
        eprintln!("BAD {b}");
    }
    assert!(bad.is_empty(), "{} unexpected failures", bad.len());
}

#[cfg(feature = "kv3")]
mod ycocg {
    use super::*;
    use source2::ext::ResourceTexture;

    fn read_resource(pack: &Vpk, path: &str) -> Resource {
        Resource::parse(&pack.read_path(path).expect("read entry")).expect("parse resource")
    }

    fn max_channel_diff(a: &Image, b: &[u8]) -> u8 {
        assert_eq!(a.rgba.len(), b.len());
        a.rgba
            .iter()
            .zip(b)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0)
    }

    #[test]
    fn marked_icons_are_converted_and_plain_bc3_is_not() {
        let Some(pack) = open_pack() else {
            return;
        };
        let mut marked = 0;
        let mut plain = 0;
        let paths = pack
            .entries()
            .iter()
            .filter(|e| e.path.starts_with("panorama/images/") && e.path.ends_with(".vtex_c"))
            .filter(|e| e.size() <= (2 << 20))
            .map(|e| e.path.clone());
        for path in paths {
            let resource = read_resource(&pack, &path);
            let Some(data) = resource.data() else {
                continue;
            };
            let Ok(texture) = Texture::parse(data, &resource.trailing) else {
                continue;
            };
            if texture.header().format != source2::texture::Format::Bc3 {
                continue;
            }
            let raw = texture.decode_rgba8(0).unwrap();
            let decoded = resource.decode_texture_rgba8(0).unwrap();
            if resource.texture_is_ycocg().unwrap() {
                marked += 1;
                assert!(decoded.rgba.chunks(4).all(|p| p[3] == 255), "{path}");
                assert_ne!(decoded, raw, "{path}: marked but unchanged");
            } else {
                plain += 1;
                assert_eq!(decoded, raw, "{path}: unmarked but changed");
            }
            if marked >= 40 && plain >= 40 {
                break;
            }
        }
        eprintln!("marked={marked} plain={plain}");
        assert!(marked > 0 && plain > 0, "marked={marked} plain={plain}");
    }

    #[test]
    fn named_icons_have_the_expected_mark() {
        let Some(pack) = open_pack() else {
            return;
        };
        for (path, expected) in [
            ("panorama/images/heroes/yakuza_sm_psd.vtex_c", true),
            ("panorama/images/items/spirit/curse_psd.vtex_c", true),
            ("panorama/images/heroes/inferno_sm_psd.vtex_c", false),
        ] {
            let resource = read_resource(&pack, path);
            assert_eq!(resource.texture_is_ycocg().unwrap(), expected, "{path}");
        }
    }

    #[test]
    fn yakuza_icon_matches_the_published_image() {
        let Some(pack) = open_pack() else {
            return;
        };
        let resource = read_resource(&pack, "panorama/images/heroes/yakuza_sm_psd.vtex_c");
        let image = resource.decode_texture_rgba8(0).unwrap();
        assert_eq!((image.width, image.height), (128, 128));
        assert!(image.rgba.chunks(4).all(|p| p[3] == 255));
        assert!(visible_pixels(&image) > 0);

        // Optional: a raw dump of the published icon (u32 LE width, u32 LE height, RGBA8),
        // so the check needs neither the network nor a PNG decoder.
        let Some(reference) = std::env::var_os("SOURCE2_YCOCG_REFERENCE_RAW") else {
            eprintln!("skipped reference comparison: SOURCE2_YCOCG_REFERENCE_RAW unset");
            return;
        };
        let bytes = std::fs::read(reference).expect("read reference");
        let (w, h) = (
            u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
        );
        assert_eq!((w, h), (image.width, image.height));
        let worst = max_channel_diff(&image, &bytes[8..]);
        eprintln!("max channel difference from the published image: {worst}");
        assert!(worst <= 3, "max channel difference {worst}");
    }
}
